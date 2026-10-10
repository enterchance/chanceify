//! Calm mode: one slow, quiet screen for when the rest is too much.
//!
//! A gentle ocean drifts in the background, the song's name sits in the
//! middle, and there is one small play button. Nothing reacts to the music,
//! nothing flashes, nothing moves fast. Home (or Esc) comes back.

use egui::{Color32, Frame, Rect, pos2, vec2};

use crate::app::App;
use crate::model::Action;
use crate::theme::{self, Icon};

/// Paints the ocean: a soft grid of deep blues and sea greens that drift
/// into each other over about a minute.
fn ocean(painter: &egui::Painter, rect: Rect, time: f64, tones: [[f32; 3]; 3]) {
    let cols = 36_usize;
    let rows = 22_usize;
    let t = time as f32;
    let [deep, mid, glow] = tones;
    let mut mesh = egui::Mesh::default();
    for j in 0..=rows {
        for i in 0..=cols {
            let u = i as f32 / cols as f32;
            let v = j as f32 / rows as f32;
            let a = 0.5 + 0.25 * (u * 3.1 + t * 0.07 + (v * 2.3 + t * 0.05).sin() * 1.3).sin();
            let b = 0.25 * (v * 4.0 - t * 0.06 + (u * 2.1 - t * 0.04).sin() * 1.1).sin();
            let field = (a + b).clamp(0.0, 1.0);
            let lift = ((field - 0.7) / 0.3).clamp(0.0, 1.0) * 0.5;
            let mut channel = [0.0_f32; 3];
            for k in 0..3 {
                let base = deep[k] + (mid[k] - deep[k]) * field;
                channel[k] = base + (glow[k] - base) * lift;
            }
            mesh.vertices.push(egui::epaint::Vertex {
                pos: pos2(rect.left() + u * rect.width(), rect.top() + v * rect.height()),
                uv: egui::epaint::WHITE_UV,
                color: Color32::from_rgb(channel[0] as u8, channel[1] as u8, channel[2] as u8),
            });
        }
    }
    let stride = (cols + 1) as u32;
    for j in 0..rows as u32 {
        for i in 0..cols as u32 {
            let a = j * stride + i;
            mesh.indices
                .extend_from_slice(&[a, a + 1, a + stride, a + 1, a + stride + 1, a + stride]);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

fn rgb(color: Color32) -> [f32; 3] {
    [f32::from(color.r()), f32::from(color.g()), f32::from(color.b())]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        app.calm_mode = false;
        return;
    }
    let now = app.now_playing();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(Color32::from_rgb(6, 18, 34)))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            let time = ctx.input(|input| input.time);
            // Colours follow the theme (which follows the album art when
            // that is on), or the old blue ocean, or a blurred cover.
            let window = rgb(app.palette.window);
            let accent = rgb(app.palette.accent);
            let tones = match app.settings.calm_look {
                2 => [[6.0, 18.0, 34.0], [20.0, 64.0, 88.0], [38.0, 104.0, 124.0]],
                _ => [mix(window, [0.0; 3], 0.35), mix(window, accent, 0.35), mix(window, accent, 0.65)],
            };
            ocean(&ui.painter().with_clip_rect(rect), rect, time, tones);
            let look = app.settings.calm_look;
            if look == 3 {
                // The album-art swirl, over the ocean. The swirl is forced on
                // for this one draw and the visualizer settings put back.
                let saved = (app.settings.vis_shapes, app.settings.vis_shapes_set);
                app.settings.vis_shapes = crate::settings::Settings::SHAPE_SWIRL;
                app.settings.vis_shapes_set = true;
                super::player_bar::lyrics_backdrop(app, ui, rect, now.as_ref(), 3);
                (app.settings.vis_shapes, app.settings.vis_shapes_set) = saved;
                ui.painter().with_clip_rect(rect).rect_filled(rect, 0.0, Color32::from_black_alpha(90));
                ctx.request_repaint_after(std::time::Duration::from_micros(33_000));
            }
            if (look == 1 || look == 4)
                && let Some(now) = &now
            {
                cover_background(app, ui, rect, now, look == 4, time);
            }
            let quiet = Color32::from_gray(185);
            let softer = Color32::from_gray(140);
            let middle = rect.center();
            match &now {
                Some(now) => {
                    ui.painter().text(
                        pos2(middle.x, middle.y - 18.0),
                        egui::Align2::CENTER_CENTER,
                        &now.title,
                        theme::semibold(26.0),
                        quiet,
                    );
                    ui.painter().text(
                        pos2(middle.x, middle.y + 16.0),
                        egui::Align2::CENTER_CENTER,
                        &now.subtitle,
                        theme::regular(15.0),
                        softer,
                    );
                    // The song-length bar above the pause button.
                    {
                        let bar = Rect::from_center_size(pos2(middle.x, middle.y + 62.0), vec2(280.0, 6.0));
                        let hit = bar.expand2(vec2(0.0, 10.0));
                        let response = ui.interact(hit, egui::Id::new("calm-seek"), egui::Sense::click_and_drag());
                        let duration = now.duration_ms.max(1);
                        let mut fraction = (now.position_ms as f32 / duration as f32).clamp(0.0, 1.0);
                        let preview_id = egui::Id::new("calm-seek-preview");
                        if let Some(at) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked()) {
                            fraction = ((at.x - bar.left()) / bar.width()).clamp(0.0, 1.0);
                            ctx.data_mut(|data| data.insert_temp(preview_id, fraction));
                        }
                        if response.drag_stopped() || response.clicked() {
                            app.actions.push(Action::Seek((fraction * duration as f32) as u32));
                            ctx.data_mut(|data| data.remove::<f32>(preview_id));
                        }
                        ui.painter().rect_filled(bar, 3.0, Color32::from_white_alpha(40));
                        let filled = Rect::from_min_max(bar.min, pos2(bar.left() + bar.width() * fraction, bar.bottom()));
                        ui.painter().rect_filled(filled, 3.0, Color32::from_gray(200));
                        let clock = |ms: u32| crate::util::format_duration_ms(ms);
                        ui.painter().text(pos2(bar.left(), bar.bottom() + 12.0), egui::Align2::LEFT_CENTER, clock((fraction * duration as f32) as u32), theme::regular(11.0), softer);
                        ui.painter().text(pos2(bar.right(), bar.bottom() + 12.0), egui::Align2::RIGHT_CENTER, clock(now.duration_ms), theme::regular(11.0), softer);
                    }
                    let button = Rect::from_center_size(pos2(middle.x, middle.y + 112.0), vec2(44.0, 44.0));
                    let mut child = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(button)
                            .layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
                    );
                    let icon = if now.playing { Icon::PauseFilled } else { Icon::PlayFilled };
                    if theme::icon_button(
                        &mut child,
                        icon,
                        24.0,
                        Color32::from_gray(160),
                        Color32::from_gray(220),
                        if now.playing { "Pause" } else { "Play" },
                    )
                    .clicked()
                    {
                        app.actions.push(Action::TogglePlay);
                    }
                }
                None => {
                    ui.painter().text(
                        middle,
                        egui::Align2::CENTER_CENTER,
                        "Nothing is playing",
                        theme::regular(18.0),
                        softer,
                    );
                }
            }
            playlist_picker(app, ui, rect);
            // How the background looks.
            {
                let strip = Rect::from_min_size(pos2(rect.right() - 330.0, rect.bottom() - 40.0), vec2(318.0, 26.0));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(strip)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                for (value, label) in [(4u8, "Spin"), (3, "Swirl"), (2, "Ocean"), (1, "Album"), (0, "Theme")] {
                    if child.selectable_label(app.settings.calm_look == value, label).clicked() {
                        app.settings.calm_look = value;
                        app.mark_settings_dirty();
                    }
                }
            }
            if matches!(app.settings.calm_look, 1 | 4) {
                let strip = Rect::from_min_size(pos2(rect.right() - 330.0, rect.bottom() - 70.0), vec2(318.0, 24.0));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(strip)
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                );
                let mut blur = app.settings.calm_blur.clamp(0.0, 1.0);
                if child
                    .add(egui::Slider::new(&mut blur, 0.0..=1.0).show_value(false).text("Blur"))
                    .changed()
                {
                    app.settings.calm_blur = blur;
                    app.mark_settings_dirty();
                }
            }
            ui.painter().text(
                pos2(middle.x, rect.bottom() - 22.0),
                egui::Align2::CENTER_CENTER,
                "Calm mode. Press Home to come back.",
                theme::regular(12.0),
                Color32::from_gray(95),
            );
        });
    // Slow on purpose: a gentle frame rate is all the drift needs.
    ctx.request_repaint_after(std::time::Duration::from_millis(66));
}

/// The album cover as the background: blurred by `calm_blur` (0 is the sharp
/// cover, 1 is the app's soft copy), and slowly turning when `spin` is on.
fn cover_background(app: &mut App, ui: &mut egui::Ui, rect: Rect, now: &crate::app::NowPlaying, spin: bool, time: f64) {
    let ctx = ui.ctx().clone();
    let Some(url) = now.art_url.clone().or_else(|| now.art_small.clone()) else {
        return;
    };
    let art = app.backend.art().clone();
    let blur = app.settings.calm_blur.clamp(0.0, 1.0);
    let Some(handle) = app.softened_covers.texture(&ctx, &art, &url) else {
        ctx.request_repaint_after(std::time::Duration::from_millis(150));
        return;
    };
    // A square that covers the window even when it is turned.
    let side = if spin { rect.size().length() } else { rect.width().max(rect.height()) };
    let square = Rect::from_center_size(rect.center(), vec2(side, side));
    let angle = (time as f32) * 0.04;
    let painter_clip = rect;
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_clip_rect(painter_clip);
        let soft = egui::Image::from_texture(egui::load::SizedTexture::from_handle(&handle));
        let soft = if spin { soft.rotate(angle, egui::Vec2::splat(0.5)) } else { soft };
        soft.paint_at(ui, square);
        if blur < 1.0 {
            let sharp = egui::Image::new(url.clone())
                .show_loading_spinner(false)
                .tint(Color32::from_white_alpha(((1.0 - blur) * 255.0) as u8));
            let sharp = if spin { sharp.rotate(angle, egui::Vec2::splat(0.5)) } else { sharp };
            sharp.paint_at(ui, square);
        }
    });
    ui.painter().with_clip_rect(rect).rect_filled(rect, 0.0, Color32::from_black_alpha(130));
}

/// A small arrow on the left edge: opens the playlists, and a click plays one.
fn playlist_picker(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    let ctx = ui.ctx().clone();
    let open_id = egui::Id::new("calm-picker-open");
    let open: bool = ctx.data(|data| data.get_temp(open_id)).unwrap_or(false);
    let arrow = Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), vec2(34.0, 60.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(arrow)
            .layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
    );
    let icon = if open { Icon::ChevronLeft } else { Icon::ChevronRight };
    if theme::icon_button(&mut child, icon, 20.0, Color32::from_gray(140), Color32::from_gray(230), "Play from a playlist").clicked() {
        ctx.data_mut(|data| data.insert_temp(open_id, !open));
    }
    if !open {
        return;
    }
    let playlists: Vec<(String, String)> = app
        .library
        .playlists
        .get()
        .map(|list| list.iter().map(|p| (p.id.clone(), p.name.clone())).collect())
        .unwrap_or_default();
    let mut chosen: Option<String> = None;
    egui::Area::new(egui::Id::new("calm-picker"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(rect.left() + 48.0, rect.center().y - 180.0))
        .show(&ctx, |ui| {
            Frame::new()
                .fill(Color32::from_black_alpha(215))
                .corner_radius(12.0)
                .inner_margin(10.0)
                .show(ui, |ui| {
                    ui.set_width(260.0);
                    ui.label(egui::RichText::new("Play from").color(Color32::from_gray(150)));
                    egui::ScrollArea::vertical().max_height(340.0).show(ui, |ui| {
                        if playlists.is_empty() {
                            ui.label("No playlists yet.");
                        }
                        for (id, name) in &playlists {
                            if ui.selectable_label(false, name).clicked() {
                                chosen = Some(id.clone());
                            }
                        }
                    });
                });
        });
    if let Some(id) = chosen {
        app.actions.push(Action::PlayContext {
            uri: format!("spotify:playlist:{id}"),
            offset_uri: None,
            offset_index: None,
        });
        ctx.data_mut(|data| data.insert_temp(open_id, false));
    }
}
