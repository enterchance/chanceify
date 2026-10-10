//! The recap card: a picture of the last week, month or all time, saved as a
//! PNG in the chanceify folder to share. Nothing is sent anywhere.

use egui::{Context, CornerRadius, Frame, Id, Margin, Rect, Stroke, pos2, vec2};

use crate::app::App;
use crate::stats::Range;
use crate::theme::{self, Palette};

/// The card's size, in points.
const CARD: egui::Vec2 = egui::vec2(480.0, 620.0);

/// What the open recap window remembers.
#[derive(Clone, Debug)]
pub struct Recap {
    pub range: Range,
    /// Where the card was drawn last frame, to crop the screenshot to.
    rect: Option<Rect>,
    /// A picture has been asked for and has not come back yet.
    saving: bool,
}

impl Recap {
    pub fn new() -> Self {
        Self {
            range: Range::Week,
            rect: None,
            saving: false,
        }
    }
}

fn title(range: Range) -> &'static str {
    match range {
        Range::Week => "My week in music",
        Range::Month => "My month in music",
        Range::Ever => "All my music",
    }
}

fn file_tag(range: Range) -> &'static str {
    match range {
        Range::Week => "7-days",
        Range::Month => "30-days",
        Range::Ever => "all-time",
    }
}

pub fn show(app: &mut App, ctx: &Context) {
    let Some(mut recap) = app.recap.take() else {
        return;
    };
    // A screenshot asked for last frame may have arrived.
    if recap.saving {
        let image = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            recap.saving = false;
            save(app, ctx, &image, recap.rect, recap.range);
        } else {
            ctx.request_repaint();
        }
    }

    let palette = app.palette;
    let now = jiff::Timestamp::now().as_second();
    let offset = crate::stats::local_offset();
    let (artists, songs) = app.stats.ranking(now, offset, recap.range);
    let summary = app.stats.summary(now, offset);
    let (plays, minutes) = match recap.range {
        Range::Week => (summary.week, summary.minutes_week),
        Range::Month => (summary.month, summary.minutes_month),
        Range::Ever => (summary.ever, summary.minutes_ever),
    };
    let streak = summary.streak_days;

    let mut open = true;
    let mut next_range = recap.range;
    let mut want_save = false;
    let mut card_rect = None;
    egui::Window::new("Recap picture")
        .id(Id::new("recap-window"))
        .open(&mut open)
        .order(egui::Order::Foreground)
        .default_pos(pos2(120.0, 60.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                for (label, choice) in [
                    ("Last 7 days", Range::Week),
                    ("Last 30 days", Range::Month),
                    ("All time", Range::Ever),
                ] {
                    if theme::pill_button(ui, &palette, label, recap.range == choice).clicked() {
                        next_range = choice;
                    }
                }
            });
            ui.add_space(8.0);
            card_rect = Some(card(ui, &palette, recap.range, plays, minutes, streak, &artists, &songs));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if theme::pill_button(ui, &palette, "Save picture", false).clicked() {
                    want_save = true;
                }
                if theme::pill_button(ui, &palette, "Open folder", false).clicked() {
                    app.actions.push(crate::model::Action::OpenRecapFolder);
                }
            });
            theme::subtle(
                ui,
                &palette,
                "The picture is saved in the recaps folder next to chanceify. It stays on this computer until you send it.",
            );
        });
    recap.rect = card_rect;
    recap.range = next_range;
    if want_save && !recap.saving && recap.rect.is_some() {
        recap.saving = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        ctx.request_repaint();
    }
    if open {
        app.recap = Some(recap);
    }
}

#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut egui::Ui,
    palette: &Palette,
    range: Range,
    plays: u32,
    minutes: u64,
    streak: u32,
    artists: &[crate::stats::Ranked],
    songs: &[crate::stats::Ranked],
) -> Rect {
    let (rect, _) = ui.allocate_exact_size(CARD, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, CornerRadius::same(18), palette.window);
    painter.rect_stroke(
        rect,
        CornerRadius::same(18),
        Stroke::new(1.0, palette.outline),
        egui::StrokeKind::Inside,
    );
    // An accent stripe across the top.
    painter.rect_filled(
        Rect::from_min_size(rect.min, vec2(CARD.x, 8.0)),
        CornerRadius { nw: 18, ne: 18, sw: 0, se: 0 },
        palette.accent,
    );
    // A soft glow of the accent under the stripe, so the top is not flat.
    super::widgets::paint_vertical_gradient(
        ui,
        Rect::from_min_size(rect.min + vec2(1.0, 8.0), vec2(CARD.x - 2.0, 170.0)),
        palette.accent.gamma_multiply(0.28),
        egui::Color32::TRANSPARENT,
    );
    // The chanceify logo, the vinyl, at the top right.
    let logo = logo_texture(ui.ctx());
    painter.image(
        logo.id(),
        Rect::from_min_size(pos2(rect.right() - 28.0 - 46.0, rect.top() + 26.0), vec2(46.0, 46.0)),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    let left = rect.left() + 28.0;
    let mut y = rect.top() + 34.0;
    painter.text(
        pos2(left, y),
        egui::Align2::LEFT_TOP,
        title(range),
        theme::semibold(26.0),
        palette.text,
    );
    y += 44.0;
    let hours = if minutes >= 120 {
        format!("{} hours", minutes / 60)
    } else {
        format!("{minutes} min")
    };
    let mut x = left;
    for (big, small) in [
        (plays.to_string(), "songs"),
        (hours, "listened"),
        (streak.to_string(), "day streak"),
    ] {
        painter.rect_filled(
            Rect::from_min_size(pos2(x - 10.0, y - 8.0), vec2(128.0, 60.0)),
            CornerRadius::same(10),
            palette.surface,
        );
        painter.text(pos2(x, y), egui::Align2::LEFT_TOP, &big, theme::semibold(24.0), palette.accent);
        painter.text(pos2(x, y + 30.0), egui::Align2::LEFT_TOP, small, theme::regular(12.5), palette.secondary);
        x += 140.0;
    }
    y += 72.0;
    for (heading, rows, with_sub) in [("Top artists", artists, false), ("Top songs", songs, true)] {
        painter.text(pos2(left, y), egui::Align2::LEFT_TOP, heading, theme::semibold(15.0), palette.text);
        y += 26.0;
        if rows.is_empty() {
            painter.text(pos2(left, y), egui::Align2::LEFT_TOP, "Nothing yet", theme::regular(13.0), palette.secondary);
            y += 24.0;
        }
        for (index, row) in rows.iter().take(5).enumerate() {
            painter.text(
                pos2(left, y),
                egui::Align2::LEFT_TOP,
                format!("{}", index + 1),
                theme::semibold(13.5),
                palette.accent,
            );
            let name = if with_sub && !row.sub.is_empty() {
                format!("{} - {}", row.name, row.sub)
            } else {
                row.name.clone()
            };
            painter.text(
                pos2(left + 24.0, y),
                egui::Align2::LEFT_TOP,
                shorten(&name, 38),
                theme::regular(13.5),
                palette.text,
            );
            painter.text(
                pos2(rect.right() - 28.0, y),
                egui::Align2::RIGHT_TOP,
                format!("{}x", row.plays),
                theme::regular(12.5),
                palette.secondary,
            );
            y += 24.0;
        }
        y += 14.0;
    }
    painter.image(
        logo.id(),
        Rect::from_min_size(pos2(left, rect.bottom() - 36.0), vec2(22.0, 22.0)),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
    painter.text(
        pos2(left + 30.0, rect.bottom() - 32.0),
        egui::Align2::LEFT_TOP,
        "chanceify\u{2122}  -  built with love by chance",
        theme::regular(12.0),
        palette.secondary,
    );
    rect
}

/// The vinyl logo as a texture, made once.
fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("recap-logo");
    if let Some(texture) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(id)) {
        return texture;
    }
    let texture = ctx.load_texture(
        "recap-logo",
        crate::app_icons::thumbnail(3),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        let cut: String = text.chars().take(max.saturating_sub(1)).collect();
        format!("{}\u{2026}", cut.trim_end())
    }
}

fn save(app: &mut App, ctx: &Context, image: &egui::ColorImage, rect: Option<Rect>, range: Range) {
    let Some(rect) = rect else {
        return;
    };
    let ppp = ctx.pixels_per_point();
    // Crop the card out of the whole-window picture, in pixels.
    let [full_w, full_h] = image.size;
    let x0 = ((rect.left() * ppp).round().max(0.0) as usize).min(full_w);
    let y0 = ((rect.top() * ppp).round().max(0.0) as usize).min(full_h);
    let x1 = ((rect.right() * ppp).round().max(0.0) as usize).min(full_w);
    let y1 = ((rect.bottom() * ppp).round().max(0.0) as usize).min(full_h);
    if x1 <= x0 || y1 <= y0 {
        app.toast_error("Could not make the picture.");
        return;
    }
    let (width, height) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let mut pixels: Vec<u8> = Vec::with_capacity((width * height * 4) as usize);
    for row in y0..y1 {
        for pixel in &image.pixels[row * full_w + x0..row * full_w + x1] {
            pixels.extend_from_slice(&pixel.to_srgba_unmultiplied());
        }
    }
    let Some(buffer) = image::RgbaImage::from_raw(width, height, pixels) else {
        app.toast_error("Could not make the picture.");
        return;
    };
    let folder = app.dirs.index_dir().join("recaps");
    let date = jiff::Timestamp::now().strftime("%Y-%m-%d").to_string();
    let path = folder.join(format!("recap-{date}-{}.png", file_tag(range)));
    let saved = std::fs::create_dir_all(&folder)
        .map_err(|error| error.to_string())
        .and_then(|()| buffer.save(&path).map_err(|error| error.to_string()));
    match saved {
        Ok(()) => app.toast("Recap picture saved in the recaps folder."),
        Err(error) => app.toast_error(format!("Could not save the picture: {error}")),
    }
}
