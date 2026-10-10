//! The words of the playing track, in a side panel that follows the song.

use egui::{Align, Color32, Frame, Layout, Margin, Rect, Sense, UiBuilder, pos2, vec2};

use crate::app::App;
use crate::i18n::{gettext, pgettext};
use crate::model::{Action, Loadable};
use crate::theme::{self, Icon};

use super::widgets;

const LINE_SIZE: f32 = 19.0;
const LINE_GAP: f32 = 10.0;
/// The constant space between a timestamp and its lyric.
const STAMP_GAP: f32 = 12.0;
/// Where the line being sung sits, as a fraction of the visible lyrics from
/// the top: high up, so the lines to come fill most of the view.
const SUNG_LINE_AT: f32 = 0.2;

/// Scrolls so the middle of `line` sits `SUNG_LINE_AT` of the way down the
/// visible lyrics.
fn show_sung_line(
    ui: &egui::Ui,
    line: Rect,
    animation: Option<egui::style::ScrollAnimation>,
    at: f32,
) {
    let above = (ui.clip_rect().height() * at - line.height() / 2.0).max(0.0);
    let target = Rect::from_min_max(pos2(line.left(), line.top() - above), line.max);
    match animation {
        Some(animation) => ui.scroll_to_rect_animation(target, Some(Align::Min), animation),
        None => ui.scroll_to_rect(target, Some(Align::Min)),
    }
}
/// How long a line takes to light up or fade.
const LIGHT_UP_SECONDS: f32 = 0.22;

fn blend(from: egui::Color32, to: egui::Color32, t: f32) -> egui::Color32 {
    let t = t.clamp(0.0, 1.0);
    egui::Color32::from(egui::Rgba::from(from) * (1.0 - t) + egui::Rgba::from(to) * t)
}

/// Right-click copies the line; Shift+right-click copies every line;
/// Ctrl+right-click copies every line with its timestamp.
fn copy_lyrics(app: &mut App, ctx: &egui::Context, lyrics: &crate::lyrics::Lyrics, line: &str) {
    let (shift, ctrl) = ctx.input(|input| (input.modifiers.shift, input.modifiers.command));
    if shift || ctrl {
        let all: Vec<String> = lyrics
            .lines
            .iter()
            .map(|l| match l.at_ms.filter(|_| ctrl) {
                Some(at) => format!("[{}] {}", crate::util::format_duration_ms(at), l.text),
                None => l.text.clone(),
            })
            .collect();
        ctx.copy_text(all.join("\n"));
        app.toast(if ctrl {
            "All lyrics copied with timestamps"
        } else {
            "All lyrics copied"
        });
    } else if !line.is_empty() {
        ctx.copy_text(line.to_string());
        app.toast("Lyric copied");
    }
}

pub fn side_panel(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let fit = super::yielding_panel(
        ui.ctx(),
        "lyrics-panel",
        theme::SIDE_PANEL_MIN_WIDTH..=640.0,
        app.settings.lyrics_width,
        ui.available_width() - super::topbar::least_width(ui.ctx()),
    );
    let panel = egui::Panel::right("lyrics-panel")
        .resizable(true)
        .default_size(app.settings.lyrics_width)
        .size_range(fit.range.clone())
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(palette.panel)
                .inner_margin(Margin::symmetric(12, 12)),
        );
    let response = panel.show(ui, |ui| {
        let window_controls = super::window_controls_reservation(
            ui.ctx(),
            app.show_queue_panel,
            app.show_lyrics_panel,
            ui.available_width(),
        );
        // The close button sits on the same row as the Windows window
        // buttons (to their left), so the lyrics start right at the top.
        let beside = window_controls.lyrics_top > 0.0;
        let inset = if beside {
            super::WINDOWS_WINDOW_CONTROLS_WIDTH
        } else {
            0.0
        };
        let row_height = if beside { 28.0 } else { 22.0 };
        // Made first, so the close button drawn after it sits on top and
        // takes its own clicks (it used to be covered, and only a
        // right-click got through).
        let page = ui.interact(ui.max_rect(), ui.id().with("lyrics-side-page"), Sense::click());
        let row = egui::vec2(ui.available_width(), row_height);
        ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(inset);
            if theme::icon_button(
                ui,
                Icon::X,
                18.0,
                palette.secondary,
                palette.text,
                &gettext(app.locale, "Close"),
            )
            .clicked()
            {
                app.actions.push(Action::ToggleLyricsPanel);
            }
        });
        ui.add_space(2.0);
        lyrics_menu(app, &page, true);
        contents(app, ui);
    });
    let current_width = response.response.rect.width();
    if (app.settings.lyrics_width - current_width).abs() > 1.0
        && super::panel_width_chosen(ui.ctx(), "lyrics-panel", &fit)
    {
        app.settings.lyrics_width = current_width;
        app.actions.push(Action::SettingsChanged);
    }
}

fn contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let Some(now) = app.now_playing() else {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Mic,
            &gettext(app.locale, "Nothing playing"),
            &gettext(app.locale, "Play a song to see its lyrics."),
        );
        return;
    };
    let lyrics = match &app.lyrics {
        Loadable::NotLoaded | Loadable::Loading => {
            widgets::loading_row(ui, &palette, app.locale);
            return;
        }
        Loadable::Failed(error) => {
            let message = gettext(
                app.locale,
                // Translators: Keep {error} unchanged. It is the original failure detail.
                "Couldn't fetch the lyrics: {error}",
            )
            .replace("{error}", error);
            ui.add_space(8.0);
            theme::text(ui, message, theme::regular(13.0), palette.secondary);
            ui.add_space(8.0);
            if theme::pill_button(ui, &palette, &gettext(app.locale, "Try again"), false).clicked()
            {
                app.request_lyrics();
            }
            return;
        }
        Loadable::Loaded(None) => {
            // No words found: say nothing at all.
            return;
        }
        Loadable::Loaded(Some(lyrics)) if lyrics.instrumental => {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Music,
                &gettext(app.locale, "Instrumental"),
                &gettext(app.locale, "No timed lyrics for this track."),
            );
            return;
        }
        Loadable::Loaded(Some(lyrics)) => lyrics.clone(),
    };

    let line_size = LINE_SIZE * app.settings.side_lyrics_scale();
    let active = lyrics.active_line(now.position_ms);
    let follow = app.lyrics_following && app.lyrics_line_shown != Some(active);
    // The line being sung is bold and in the accent colour; every other
    // line is quiet, regular text, the same before and after it has been
    // sung. A line takes 220 ms to light up or fade, as in omarchy-lyrics.
    let quiet = palette.text.gamma_multiply(0.45);
    let stamps_on = !app.settings.side_lyrics_flag(crate::settings::Settings::LYRICS_HIDE_STAMPS);
    let stamp_w = ui
        .painter()
        .layout_no_wrap("00:00".to_owned(), theme::regular(line_size * 0.68), quiet)
        .size()
        .x;
    let scroll = crate::autoscroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("lyrics-scroll")
            .auto_shrink([false, false]),
        egui::Vec2b::new(false, true),
        |ui| {
            // Before the first line there is nothing to highlight, so the
            // panel sits at the top rather than wherever it was left.
            if follow && lyrics.synced && active.is_none() {
                let top = ui.cursor().min;
                ui.scroll_to_rect(
                    egui::Rect::from_min_size(top, egui::vec2(1.0, 1.0)),
                    Some(Align::Min),
                );
            }
            ui.add_space(12.0);
            for (index, line) in lyrics.lines.iter().enumerate() {
                let is_active = active == Some(index);
                let lit = ui.ctx().animate_bool_with_time(
                    egui::Id::new("lyric-line").with(index),
                    is_active,
                    LIGHT_UP_SECONDS,
                );
                let color = blend(quiet, palette.accent, lit);
                let font = if lit > 0.5 {
                    theme::bold(line_size)
                } else {
                    theme::regular(line_size)
                };
                // A timed line with no words is the band playing on.
                let text = if line.text.is_empty() && lyrics.synced {
                    "\u{266a}"
                } else {
                    line.text.as_str()
                };
                let sense = if lyrics.synced {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                // The time of the line, in a column of its own with a small
                // constant gap before the words.
                let stamp = line
                    .at_ms
                    .filter(|_| lyrics.synced && stamps_on)
                    .map(crate::util::format_duration_ms);
                let response = ui
                    .horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if stamps_on && lyrics.synced {
                            let (cell, _) =
                                ui.allocate_exact_size(vec2(stamp_w + STAMP_GAP, 1.0), Sense::hover());
                            if let Some(stamp) = &stamp {
                                ui.painter().text(
                                    pos2(cell.left(), cell.top() + line_size * 0.62),
                                    egui::Align2::LEFT_CENTER,
                                    stamp,
                                    theme::regular(line_size * 0.68),
                                    quiet.gamma_multiply(0.8),
                                );
                            }
                        }
                        if crate::bidi::is_rtl(text) {
                            let galley = crate::bidi::layout(
                                ui.painter(),
                                text,
                                font,
                                color,
                                ui.available_width(),
                                usize::MAX,
                                None,
                            );
                            ui.add(egui::Label::new(galley).sense(sense))
                        } else {
                            ui.add(
                                egui::Label::new(egui::RichText::new(text).font(font).color(color))
                                    .wrap()
                                    .sense(sense),
                            )
                        }
                    })
                    .inner;
                crate::autoscroll::row(ui, &response);
                let rect = response.rect;
                if response.secondary_clicked() {
                    copy_lyrics(app, ui.ctx(), &lyrics, &line.text);
                }
                if lyrics.synced
                    && response.clicked()
                    && let Some(at_ms) = line.at_ms
                {
                    app.actions.push(Action::Seek(at_ms));
                    app.lyrics_following = true;
                }
                if is_active && follow {
                    show_sung_line(ui, rect, None, SUNG_LINE_AT);
                }
                ui.add_space(LINE_GAP);
            }
            // Words without timing can only be followed by the clock: sit
            // at the part of the text the song is probably at.
            if app.lyrics_following && !lyrics.synced && now.duration_ms > 0 {
                let fraction =
                    (f64::from(now.position_ms) / f64::from(now.duration_ms)).clamp(0.0, 1.0);
                let content = ui.min_rect();
                let y = content.top() + content.height() * fraction as f32;
                ui.scroll_to_rect(
                    egui::Rect::from_min_max(
                        egui::pos2(content.left(), y),
                        egui::pos2(content.right(), y + 1.0),
                    ),
                    Some(Align::Center),
                );
            }
            // Room for the last line to rise to where a sung line sits.
            ui.add_space((ui.clip_rect().height() * (1.0 - SUNG_LINE_AT)).max(60.0));
        },
    );
    crate::autoscroll::lyrics(ui, scroll.id);
    // Scrolling by hand means the reader wants to look elsewhere; the
    // Follow button in the header picks the song back up.
    app.lyrics_line_shown = Some(active);
}

pub fn fullscreen(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default()
        .frame(Frame::new().fill(theme::Palette::dark().window))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            // The picture reaches a few pixels past the panel's top and sides,
            // so no thin line of the window behind shows at the edge.
            background(
                app,
                ui,
                Rect::from_min_max(rect.min - vec2(4.0, 6.0), rect.max + vec2(4.0, 0.0)),
            );
            // Right-click anywhere on the page, off a line, for the options.
            let page = ui.interact(rect, egui::Id::new("lyrics-page"), Sense::click());
            // Right-click opens the settings panel: the visualizer's and the
            // lyrics' options together, in one place with no submenus.
            if page.secondary_clicked() && !app.vis_panel {
                app.actions.push(Action::ToggleVisPanel);
            }
            let top = theme::titlebar_inset(ui.ctx()) + 24.0;
            if app.now_playing().is_some()
                && rect.width() >= COVER_BESIDE_MIN_WIDTH
                && !app.settings.lyrics_flag(crate::settings::Settings::LYRICS_HIDE_ART)
            {
                with_cover(app, ui, rect, top);
                return;
            }
            let width = fullscreen_content_width(rect.width());
            let region = Rect::from_min_max(
                pos2(rect.center().x - width / 2.0, rect.top() + top),
                pos2(rect.center().x + width / 2.0, rect.bottom()),
            );
            let mut content = ui.new_child(UiBuilder::new().max_rect(region));
            fullscreen_header(app, &mut content);
            content.add_space(20.0);
            track_heading(app, &mut content);
            content.add_space(16.0);
            fullscreen_contents(app, &mut content);
        });
}

/// The right-click menu of the lyrics page.
fn lyrics_menu(app: &mut App, page: &egui::Response, side: bool) {
    use crate::settings::Settings;
    let palette = if side { app.palette } else { theme::Palette::dark() };
    egui::Popup::context_menu(page)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| {
            let tick = |on: bool| if on { Some(Icon::Check) } else { None };
            if side {
                // The small panel has options of its own; changing them never
                // touches the full-screen lyrics.
                let stamps = !app.settings.side_lyrics_flag(Settings::LYRICS_HIDE_STAMPS);
                if widgets::menu_item(ui, &palette, tick(stamps), &gettext(app.locale, "Timestamps")) {
                    app.actions
                        .push(Action::ToggleSideLyricsFlag(Settings::LYRICS_HIDE_STAMPS));
                }
                widgets::menu_separator(ui, &palette);
                for (value, label) in [(1u8, "Smaller lines"), (0, "Normal lines"), (2, "Larger lines")] {
                    let on = app.settings.side_lyrics_size == value;
                    if widgets::menu_item(ui, &palette, tick(on), &gettext(app.locale, label)) {
                        app.actions.push(Action::SetSideLyricsSize(value));
                    }
                }
                return;
            }
            let flags = [
                (Settings::LYRICS_HIDE_ART, "Album art", true),
                (Settings::LYRICS_FLOAT_ART, "Floating art", false),
                (Settings::LYRICS_BOUNCE_ART, "Art bounces to the music", false),
                (Settings::LYRICS_HIDE_STAMPS, "Timestamps", true),
                (Settings::LYRICS_COUNTDOWN, "Countdown", false),
                (Settings::LYRICS_HIDE_ARTIST, "Artist name", true),
            ];
            for (bit, label, inverted) in flags {
                let on = app.settings.lyrics_flag(bit) != inverted;
                if widgets::menu_item(ui, &palette, tick(on), &gettext(app.locale, label)) {
                    app.actions.push(Action::ToggleLyricsFlag(bit));
                }
            }
            widgets::menu_separator(ui, &palette);
            if widgets::menu_item(
                ui,
                &palette,
                tick(app.settings.lyrics_vis),
                &gettext(app.locale, "Visualizer behind the lyrics"),
            ) {
                app.actions.push(Action::ToggleLyricsVis);
            }
            if app.settings.lyrics_vis {
                if widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::Settings),
                    &gettext(app.locale, "Visualizer settings"),
                ) {
                    app.actions.push(Action::ToggleVisPanel);
                }
                super::player_bar::slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "How dark"),
                    10.0..=95.0,
                    app.settings.lyrics_vis_darkness() * 100.0,
                    |value| app.actions.push(Action::SetLyricsVisDark(value / 100.0)),
                );
            }
            widgets::menu_separator(ui, &palette);
            let current = app.settings.lyrics_align_value();
            for (value, label) in [
                (0u8, "Left"),
                (2, "Right"),
                (4, "Focus on the current line"),
            ] {
                if widgets::menu_item(
                    ui,
                    &palette,
                    tick(current == value),
                    &gettext(app.locale, label),
                ) {
                    app.actions.push(Action::SetLyricsAlign(value));
                }
            }
        });
}

/// The widest the lyrics get beside the cover, so lines stay easy to read.
const LYRICS_BESIDE_WIDTH: f32 = 860.0;

/// The narrowest window that shows the cover beside the lyrics; narrower
/// ones keep a single column with a small cover in the heading.
const COVER_BESIDE_MIN_WIDTH: f32 = 900.0;

/// Full screen with the cover large: beside the lyrics when there are
/// words to follow, and alone in the middle when there are none, a calm
/// view of what is playing.
fn with_cover(app: &mut App, ui: &mut egui::Ui, rect: Rect, top: f32) {
    let outer = Rect::from_min_max(
        pos2(rect.left() + 48.0, rect.top() + top),
        pos2(rect.right() - 48.0, rect.bottom() - 40.0),
    );
    let mut header = ui.new_child(UiBuilder::new().max_rect(outer));
    fullscreen_header(app, &mut header);
    let below = Rect::from_min_max(
        pos2(outer.left(), header.min_rect().bottom() + 24.0),
        outer.max,
    );
    // The cover moves aside only for words to read. While they load it
    // stays in the middle, so a song that turns out to have none never
    // moves at all.
    let words = matches!(&app.lyrics, Loadable::Loaded(Some(lyrics)) if !lyrics.instrumental);
    if words {
        // The cover and the lyrics are one group, centred in the window.
        let gap = 40.0;
        // The cover grows with the room the lyrics leave, and never shrinks.
        let side = (below.width() * 0.46)
            .min(below.height() - 180.0)
            .clamp(280.0, 760.0);
        let lyrics_width = (below.width() - side - gap).min(LYRICS_BESIDE_WIDTH);
        let _ = lyrics_width;
        let left = below.left() + below.width() * 0.02;
        let column = Rect::from_min_size(
            pos2(left, below.center().y - (side + 180.0) / 2.0),
            vec2(side, side + 90.0),
        );
        big_cover(app, ui, column, Align::Min);
        // Lyrics centred in the window when that leaves room beside the
        // cover; otherwise centred in the room that is left. Everything is
        // measured from the window, so it scales with any resolution.
        let remaining = Rect::from_min_max(pos2(column.right() + gap, below.top()), below.max);
        let window_centre = rect.center().x;
        let centred_width = 2.0 * (window_centre - remaining.left());
        let (centre_x, lyrics_width) = if centred_width >= 420.0 {
            (window_centre, centred_width.min(LYRICS_BESIDE_WIDTH))
        } else {
            (remaining.center().x, remaining.width().min(LYRICS_BESIDE_WIDTH))
        };
        let lyrics = Rect::from_min_max(
            pos2(centre_x - lyrics_width / 2.0, below.top()),
            pos2(centre_x + lyrics_width / 2.0, below.bottom()),
        );
        let mut content = ui.new_child(UiBuilder::new().max_rect(lyrics));
        fullscreen_contents(app, &mut content);
    } else {
        let side = (below.height() - 230.0)
            .min(below.width() * 0.5)
            .clamp(200.0, 560.0);
        let column = Rect::from_center_size(
            below.center() - vec2(0.0, 45.0),
            vec2(side, side + 90.0),
        );
        big_cover(app, ui, column, Align::Center);
        // Why there are no words, quietly, under the song, or that they
        // are still being fetched.
        let (heading, detail) = match &app.lyrics {
            Loadable::Loaded(Some(_)) => (
                gettext(app.locale, "Instrumental"),
                gettext(app.locale, "No timed lyrics for this track."),
            ),
            Loadable::Loaded(None) => (String::new().into(), Default::default()),
            Loadable::Failed(error) => (
                gettext(
                    app.locale,
                    // Translators: Keep {error} unchanged. It is the original failure detail.
                    "Couldn't fetch the lyrics: {error}",
                )
                .replace("{error}", error)
                .into(),
                Default::default(),
            ),
            Loadable::NotLoaded | Loadable::Loading => {
                (gettext(app.locale, "Loading…"), Default::default())
            }
        };
        let heading = ui.painter().text(
            pos2(column.center().x, column.bottom() + 94.0),
            egui::Align2::CENTER_TOP,
            heading,
            theme::semibold(13.0),
            Color32::from_gray(200),
        );
        if matches!(app.lyrics, Loadable::Failed(_)) {
            let retry = Rect::from_center_size(
                pos2(column.center().x, heading.bottom() + 24.0),
                vec2(column.width(), 32.0),
            );
            let mut retry_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(retry)
                    .layout(Layout::top_down(Align::Center)),
            );
            let label = gettext(app.locale, "Try again");
            if theme::pill_button(&mut retry_ui, &theme::Palette::dark(), &label, false).clicked() {
                app.actions.push(Action::RetryLyrics);
            }
            return;
        }
        ui.painter().text(
            pos2(column.center().x, heading.bottom() + 4.0),
            egui::Align2::CENTER_TOP,
            detail,
            theme::regular(13.0),
            Color32::from_gray(170),
        );
    }
}

/// The playing song's cover filling the top of `column`, with its title and
/// artists beneath, aligned to its left edge or centred.
fn big_cover(app: &mut App, ui: &mut egui::Ui, column: Rect, align: Align) {
    let Some(now) = app.now_playing() else {
        return;
    };
    let base_side = column.width();
    let base_cover = Rect::from_min_size(column.min, vec2(base_side, base_side));
    // Bouncing to the music: the card swells a little with the bass. The words
    // below keep to the card's resting size so they never jump about.
    let bounce_on = app.settings.lyrics_flag(crate::settings::Settings::LYRICS_BOUNCE_ART);
    let swell = if bounce_on {
        let target = app.music_bass_level();
        ui.ctx().request_repaint();
        ui.ctx()
            .animate_value_with_time(egui::Id::new("lyrics-cover-bounce"), target, 0.07)
    } else {
        0.0
    };
    let side = base_side * (1.0 + 0.07 * swell);
    let cover = Rect::from_center_size(base_cover.center(), vec2(side, side));
    let radius = 10.0;
    // Holographic card: the card turns to face the pointer, in 3D. Where the
    // pointer is on the card decides which edge leans away.
    let hover_id = egui::Id::new("lyrics-cover-tilt");
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let over = pointer.is_some_and(|p| cover.expand(30.0).contains(p));
    // The cover floats about on its own when asked to, and the pointer
    // takes over while it is on the card.
    let floating = app.settings.lyrics_flag(crate::settings::Settings::LYRICS_FLOAT_ART);
    let time = ui.input(|input| input.time) as f32;
    let (nx, ny) = match pointer.filter(|_| over) {
        Some(p) => (
            ((p.x - cover.center().x) / (side * 0.5)).clamp(-1.0, 1.0),
            ((p.y - cover.center().y) / (side * 0.5)).clamp(-1.0, 1.0),
        ),
        None if floating => (
            0.75 * ((time * 0.37).sin() * 0.6 + (time * 0.91 + 1.3).sin() * 0.4),
            0.75 * ((time * 0.43 + 2.1).sin() * 0.6 + (time * 0.77 + 0.4).sin() * 0.4),
        ),
        None => (0.0, 0.0),
    };
    if floating {
        ui.ctx().request_repaint();
    }
    let ctx = ui.ctx().clone();
    let nx = ctx.animate_value_with_time(hover_id.with("x"), nx, 0.15);
    let ny = ctx.animate_value_with_time(hover_id.with("y"), ny, 0.15);
    // The shadow turns with the card. (It used to stay put as a plain box.)
    if nx.abs() > 0.002 || ny.abs() > 0.002 {
        let depth = 3.2_f32;
        let tilt = 0.55_f32;
        let half = side * 0.5;
        let corners: Vec<egui::Pos2> = [(-1.0_f32, -1.0_f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .iter()
            .map(|(x, y)| {
                let z = (x * nx + y * ny) * tilt;
                let f = depth / (depth + z);
                pos2(cover.center().x + x * half * f, cover.center().y + y * half * f)
            })
            .collect();
        for layer in 0..8 {
            let grow = layer as f32 * 6.0;
            let points: Vec<egui::Pos2> = corners
                .iter()
                .map(|p| {
                    let out = (*p - cover.center()).normalized();
                    *p + out * grow + vec2(0.0, 18.0)
                })
                .collect();
            ui.painter().add(egui::Shape::convex_polygon(
                points,
                Color32::from_black_alpha(17),
                egui::Stroke::NONE,
            ));
        }
    } else {
        ui.painter().add(
            egui::epaint::Shadow {
                offset: [0, 18],
                blur: 48,
                spread: 0,
                color: Color32::from_black_alpha(140),
            }
            .as_shape(cover, radius),
        );
    }
    let url = now.art_url.as_deref().or(now.art_small.as_deref());
    let mut drawn = false;
    if (nx.abs() > 0.002 || ny.abs() > 0.002 || over)
        && let Some(url) = url
    {
        let image = egui::Image::new(url).show_loading_spinner(false);
        if let Ok(egui::load::TexturePoll::Ready { texture }) = image.load_for_size(&ctx, cover.size())
        {
            let n = 12;
            let depth = 3.2_f32;
            let tilt = 0.55_f32;
            let half = side * 0.5;
            let mut mesh = egui::epaint::Mesh::with_texture(texture.id);
            for row in 0..=n {
                for column_index in 0..=n {
                    let u = column_index as f32 / n as f32;
                    let v = row as f32 / n as f32;
                    let x = u * 2.0 - 1.0;
                    let y = v * 2.0 - 1.0;
                    // The side the pointer is on leans away.
                    let z = (x * nx + y * ny) * tilt;
                    let f = depth / (depth + z);
                    let shade = (255.0 - z * 70.0).clamp(150.0, 255.0) as u8;
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: pos2(cover.center().x + x * half * f, cover.center().y + y * half * f),
                        uv: pos2(u, v),
                        color: Color32::from_gray(shade),
                    });
                }
            }
            for row in 0..n {
                for column_index in 0..n {
                    let a = (row * (n + 1) + column_index) as u32;
                    let b = a + 1;
                    let c = a + (n + 1) as u32;
                    let d = c + 1;
                    mesh.indices.extend_from_slice(&[a, b, c, b, d, c]);
                }
            }
            ui.painter().add(egui::Shape::mesh(mesh));
            drawn = true;
            ctx.request_repaint();
        }
    }
    if !drawn {
        widgets::paint_cover(
            ui,
            &theme::Palette::dark(),
            url,
            cover,
            radius,
            Icon::Music,
            Some(app.backend.art()),
        );
    }
    super::player_bar::big_art_missing_mark(app, ui, cover, "lyrics");
    // Right-click the cover to save it (not the lyrics options).
    let cover_response = ui.interact(cover, egui::Id::new("lyrics-cover-menu"), Sense::click());
    let menu_palette = theme::Palette::dark();
    egui::Popup::context_menu(&cover_response)
        .frame(widgets::menu_frame(&menu_palette))
        .show(|ui| {
            if widgets::menu_item(ui, &menu_palette, Some(Icon::Disc), "Save album art") {
                app.actions.push(Action::SaveAlbumArt);
            }
        });
    // The tilted card swells towards the viewer; the words follow its lowest
    // corner so a floating card never covers them.
    let lowest = [(-1.0_f32, -1.0_f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|(x, y)| {
            let z = (x * nx + y * ny) * 0.55;
            base_cover.center().y + y * base_side * 0.5 * (3.2 / (3.2 + z))
        })
        .fold(base_cover.bottom(), f32::max);
    let words_top = ctx.animate_value_with_time(hover_id.with("words"), lowest.max(base_cover.bottom()) + 18.0, 0.1);
    let words = Rect::from_min_max(pos2(column.left(), words_top), column.max);
    let mut text = ui.new_child(
        UiBuilder::new()
            .max_rect(words)
            .layout(Layout::top_down(align)),
    );
    text.spacing_mut().item_spacing.y = 4.0;
    // With the artist hidden the song's name takes the room and is bigger.
    let show_artist = !app.settings.lyrics_flag(crate::settings::Settings::LYRICS_HIDE_ARTIST);
    let title_size = if show_artist { 22.0 } else { 30.0 };
    let saved = app.is_saved(&now.uri).unwrap_or(false);
    let heart = !now.is_episode && now.uri.starts_with("spotify:track:");
    // The title's own width, so the row can sit in the middle when the
    // column is centred (and the heart hugs the name when it is not).
    let heart_w = if heart { 36.0 } else { 0.0 };
    let room = (words.width() - heart_w - 8.0).max(40.0);
    let title_w = text
        .painter()
        .layout_no_wrap(now.title.clone(), theme::semibold(title_size), Color32::WHITE)
        .size()
        .x
        .min(room)
        + 4.0;
    let lead = if align == Align::Center {
        ((words.width() - title_w - heart_w - 8.0) / 2.0).max(0.0)
    } else {
        0.0
    };
    text.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add_space(lead);
        ui.allocate_ui_with_layout(
            vec2(title_w, title_size + 8.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&now.title)
                            .font(theme::semibold(title_size))
                            .color(Color32::WHITE),
                    )
                    .truncate(),
                );
            },
        );
        if heart {
            let dark = theme::Palette::dark();
            if theme::icon_button(
                ui,
                if saved { Icon::HeartFilled } else { Icon::Heart },
                title_size.min(24.0),
                if saved { dark.accent } else { Color32::from_gray(225) },
                dark.accent_hover,
                if saved { "Remove from Liked Songs" } else { "Save to Liked Songs" },
            )
            .clicked()
            {
                app.actions.push(Action::ToggleSaved(now.uri.clone()));
            }
        }
    });
    if show_artist {
        text.add(
            egui::Label::new(
                egui::RichText::new(&now.subtitle)
                    .font(theme::regular(14.0))
                    .color(Color32::from_gray(225)),
            )
            .truncate(),
        );
    }
}

fn fullscreen_content_width(viewport_width: f32) -> f32 {
    let available = (viewport_width - 48.0).max(0.0);
    (viewport_width * 0.72).clamp(400.0, 960.0).min(available)
}

fn preferred_backdrop_art(small: Option<String>, large: Option<String>) -> Option<String> {
    small.or(large)
}

fn background(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    theme::apply_local(ui, &theme::Palette::dark());
    let art = app
        .now_playing()
        .and_then(|now| preferred_backdrop_art(now.art_small, now.art_url));
    let painter = ui.painter().with_clip_rect(rect);
    if let Some(texture) = app
        .lyrics_backdrop
        .texture(ui.ctx(), app.backend.art(), art.as_deref())
    {
        painter.image(
            texture.id(),
            rect,
            cover_uv(rect.size(), texture.size_vec2()),
            Color32::from_gray(180),
        );
    }
    if app.settings.lyrics_vis {
        // The spectrum moves over the picture, then a black veil (the
        // reader's "how dark") keeps the words easy to read.
        let now = app.now_playing();
        // Shaped by the visualizer settings (Visualizer settings, below).
        // Bars and flow stay below the song and artist names, so they never
        // cover or cut them off; the swirl fills the whole page.
        let mut area = rect;
        if app.settings.vis_shapes_value() & crate::settings::Settings::SHAPE_SWIRL == 0 {
            let top = (rect.height() * 0.36).max(190.0).min(rect.height() * 0.8);
            area = Rect::from_min_max(pos2(rect.left(), rect.top() + top), rect.max);
        }
        let moving = super::player_bar::lyrics_backdrop(app, ui, area, now.as_ref(), 3);
        if moving {
            ui.ctx().request_repaint_after(std::time::Duration::from_micros(16_667));
        }
        let veil = (app.settings.lyrics_vis_darkness() * 255.0) as u8;
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(veil));
    } else {
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(120));
    }
    widgets::paint_vertical_gradient(
        ui,
        rect,
        Color32::from_black_alpha(0),
        Color32::from_black_alpha(95),
    );
}

fn cover_uv(view: egui::Vec2, image: egui::Vec2) -> Rect {
    let ratio = (view.x / view.y.max(1.0)) / (image.x / image.y.max(1.0));
    let size = if ratio > 1.0 {
        vec2(1.0, 1.0 / ratio)
    } else {
        vec2(ratio, 1.0)
    };
    Rect::from_center_size(pos2(0.5, 0.5), size)
}

fn fullscreen_header(_app: &mut App, _ui: &mut egui::Ui) {
    // Nothing: no title, no Follow, no leave button. Esc and C leave.
}

fn track_heading(app: &App, ui: &mut egui::Ui) {
    if let Some(now) = app.now_playing() {
        ui.horizontal(|ui| {
            let size = 52.0;
            let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
            widgets::paint_cover(
                ui,
                &theme::Palette::dark(),
                now.art_small.as_deref().or(now.art_url.as_deref()),
                rect,
                4.0,
                Icon::Music,
                Some(app.backend.art()),
            );
            ui.vertical(|ui| {
                let show_artist =
                    !app.settings.lyrics_flag(crate::settings::Settings::LYRICS_HIDE_ARTIST);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&now.title)
                            .font(theme::semibold(if show_artist { 22.0 } else { 28.0 }))
                            .color(Color32::WHITE),
                    )
                    .truncate(),
                );
                if show_artist {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(&now.subtitle)
                                .font(theme::regular(13.0))
                                .color(Color32::from_gray(235)),
                        )
                        .truncate(),
                    );
                }
            });
        });
    }
}

/// The lyrics drawn over the full-screen visualizer, when the setting is on.
///
/// Not the lyrics page: a short stack beside the cover, centred on it. The
/// line being sung is large and bright; two before it and three after it
/// are smaller and fainter the further they are, and the whole stack glides
/// up as the song moves on. No timestamps, no scrolling, nothing to click.
pub fn vis_overlay(app: &mut App, ui: &mut egui::Ui) {
    if !app.settings.vis_lyrics {
        return;
    }
    let Some(now) = app.now_playing() else {
        return;
    };
    let lyrics = match &app.lyrics {
        Loadable::Loaded(Some(lyrics)) if lyrics.synced && !lyrics.instrumental => lyrics.clone(),
        _ => return,
    };
    let rect = ui.ctx().content_rect();
    let layout = app.settings.vis_title_layout.min(3);
    let cover = super::player_bar::fullscreen_cover(rect, layout);
    let left = cover.right() + rect.width() * 0.05;
    let right = rect.right() - rect.width() * 0.08;
    if right - left < 220.0 {
        return;
    }
    // With the title beside the cover, it holds the upper half of that
    // column and the lyrics take the lower half.
    let region_top = if layout == 1 || layout == 2 {
        cover.top() + cover.height() * 0.5
    } else {
        cover.top()
    };
    let region = Rect::from_min_max(
        pos2(left, region_top),
        pos2(right, (cover.bottom() + rect.height() * 0.14).min(rect.bottom() - 30.0)),
    );
    let painter = ui.painter().with_clip_rect(region.expand(20.0));
    if !app.settings.vis_lyrics_no_back {
        painter.rect_filled(region.expand(18.0), 16.0, Color32::from_black_alpha(140));
    }
    let active = lyrics.active_line(now.position_ms);
    let target = active.map_or(-1.0, |line| line as f32);
    let place = ui.ctx().animate_value_with_time(
        egui::Id::new(("vis-lyrics-place", &now.uri)),
        target,
        0.4,
    );
    let base = (region.height() * 0.11).clamp(22.0, 52.0).min(region.width() * 0.07);
    let pitch = base * 1.5;
    let centre_y = region.center().y;
    // The same anchoring and timestamps as the full-screen lyrics page.
    let align = app.settings.lyrics_align_value();
    let stamps_on = !app.settings.lyrics_flag(crate::settings::Settings::LYRICS_HIDE_STAMPS);
    let countdown = app.settings.lyrics_flag(crate::settings::Settings::LYRICS_COUNTDOWN);
    let next_line = lyrics
        .lines
        .iter()
        .position(|line| line.at_ms.is_some_and(|at| at > now.position_ms));
    let until_next = next_line
        .and_then(|i| lyrics.lines[i].at_ms)
        .map(|at| (at - now.position_ms).div_ceil(1000))
        .filter(|secs| *secs <= 30);
    let stamp_w = if stamps_on || countdown { 56.0 } else { 0.0 };
    let text_left = region.left() + stamp_w;
    let text_w = (region.width() - stamp_w).max(60.0);
    let first = (place.floor() as isize - 3).max(0) as usize;
    let last = ((place.ceil() as isize + 4).max(0) as usize).min(lyrics.lines.len());
    for index in first..last {
        let line = &lyrics.lines[index];
        let text = if line.text.is_empty() { "\u{266a}" } else { line.text.as_str() };
        let d = index as f32 - place;
        let near = d.abs().min(1.0);
        let size = base * (1.0 - 0.38 * near) * if d.abs() > 2.0 { 0.9 } else { 1.0 };
        let fade = (1.0 - 0.3 * d.abs()).clamp(0.0, 1.0);
        let alpha = if d.abs() < 1.0 { 1.0 - 0.4 * d.abs() } else { 0.6 * fade };
        // Never cut with dots: the line takes up to two rows, and a very long
        // one shrinks until all of it fits.
        let colour = Color32::WHITE.gamma_multiply(alpha.clamp(0.0, 1.0));
        let mut fit = 1.0_f32;
        let galley = loop {
            let galley = crate::bidi::layout(&painter, text, theme::bold(size * fit), colour, text_w, 2, None);
            if !galley.elided || fit < 0.4 {
                break galley;
            }
            fit *= 0.88;
        };
        let y = centre_y + d * pitch + d.signum() * near * base * 0.2 - galley.size().y / 2.0;
        let x = match align {
            2 => region.right() - galley.size().x,
            4 => text_left + (text_w - galley.size().x) / 2.0,
            _ => text_left,
        };
        let label = if countdown && next_line == Some(index) && until_next.is_some() {
            until_next.map(|secs| format!("{secs}s"))
        } else if stamps_on {
            line.at_ms.map(|at| {
                let secs = u64::from(at / 1000);
                format!("{}:{:02}", secs / 60, secs % 60)
            })
        } else {
            None
        };
        if let Some(label) = label {
            painter.text(
                pos2(region.left(), y + galley.size().y / 2.0),
                egui::Align2::LEFT_CENTER,
                label,
                theme::bold((base * 0.4).max(11.0)),
                Color32::WHITE.gamma_multiply((alpha * 0.55).clamp(0.0, 1.0)),
            );
        }
        painter.galley(pos2(x, y), galley, Color32::WHITE);
    }
    if now.playing {
        ui.ctx().request_repaint();
    }
}

fn fullscreen_contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = theme::Palette::dark();
    let Some(now) = app.now_playing() else {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Mic,
            &gettext(app.locale, "Nothing playing"),
            &gettext(app.locale, "Play a song to see its lyrics."),
        );
        return;
    };
    let lyrics = match &app.lyrics {
        Loadable::NotLoaded | Loadable::Loading => {
            widgets::loading_row(ui, &palette, app.locale);
            return;
        }
        Loadable::Failed(error) => {
            let message = gettext(
                app.locale,
                // Translators: Keep {error} unchanged. It is the original failure detail.
                "Couldn't fetch the lyrics: {error}",
            )
            .replace("{error}", error);
            ui.add_space(8.0);
            theme::text(ui, message, theme::regular(13.0), palette.text);
            ui.add_space(8.0);
            if theme::pill_button(ui, &palette, &gettext(app.locale, "Try again"), false).clicked()
            {
                app.actions.push(Action::RetryLyrics);
            }
            return;
        }
        Loadable::Loaded(None) => {
            // No words found: say nothing at all.
            return;
        }
        Loadable::Loaded(Some(lyrics)) if lyrics.instrumental => {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Music,
                &gettext(app.locale, "Instrumental"),
                &gettext(app.locale, "No timed lyrics for this track."),
            );
            return;
        }
        Loadable::Loaded(Some(lyrics)) => lyrics.clone(),
    };

    let active = lyrics.active_line(now.position_ms);
    // A Follow click resets the remembered line after drawing. Other frames
    // record the shown line before any line-click action restores following.
    if !app
        .actions
        .iter()
        .any(|action| matches!(action, Action::FollowLyrics))
    {
        app.actions.push(Action::LyricsLineShown(active));
    }
    let viewport = ui.available_rect_before_wrap();
    // Scrolling by hand never switches following off; the view simply
    // returns to the sung line when the next line starts.
    let following = app.lyrics_following;
    let follow = following && app.lyrics_line_shown != Some(active);
    let animation = egui::style::ScrollAnimation::duration(0.26);
    // Big enough to read from across a room.
    // Scaled by the height too, so a short page still shows several lines.
    let size = (ui.available_width() * 0.06)
        .min(ui.available_height() * 0.09)
        .clamp(22.0, 58.0);
    // The line being sung brightens; all lines keep the same font metrics
    // so highlighting cannot rewrap the words during a transition.
    // A line takes 300 ms to light up or fade.
    let quiet = palette.text.gamma_multiply(0.68);
    let align = app.settings.lyrics_align_value();
    // 3, 4 and 5 are the focus modes: the sung line big, the rest small.
    // At 3 the page runs backwards: the line being sung is at the bottom
    // and the next ones fade in from the top.
    let focus = align >= 3;
    let reversed = align == 3;
    let sung_at = match align {
        3 => 0.8,
        4 => 0.5,
        _ => SUNG_LINE_AT,
    };
    let stamps_on = !app.settings.lyrics_flag(crate::settings::Settings::LYRICS_HIDE_STAMPS);
    let countdown = app.settings.lyrics_flag(crate::settings::Settings::LYRICS_COUNTDOWN)
        && lyrics.synced;
    let column_on = (stamps_on || countdown) && lyrics.synced;
    let stamp_w = ui
        .painter()
        .layout_no_wrap("00:00".to_owned(), theme::bold(size * 0.62), quiet)
        .size()
        .x;
    // The first line still to come, and whole seconds until it.
    let next_line = lyrics
        .lines
        .iter()
        .position(|line| line.at_ms.is_some_and(|at| at > now.position_ms));
    let until_next = next_line
        .and_then(|i| lyrics.lines[i].at_ms)
        .map(|at| (at - now.position_ms).div_ceil(1000))
        .filter(|secs| *secs <= 30);
    if countdown && now.playing {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    ui.spacing_mut().scroll.fade.strength = 0.0;
    egui::ScrollArea::vertical()
        .id_salt(("fullscreen-lyrics-scroll", &now.uri))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Before the first line there is nothing to highlight, so the
            // panel sits at the top rather than wherever it was left.
            if follow && lyrics.synced && active.is_none() && !reversed {
                let top = ui.cursor().min;
                ui.scroll_to_rect_animation(
                    egui::Rect::from_min_size(top, egui::vec2(1.0, 1.0)),
                    Some(Align::Min),
                    animation,
                );
            }
            // Room for the first line to sit where a sung line sits, and for
            // the last to rise to it.
            let (padding, below) = if lyrics.synced {
                (
                    (viewport.height() * sung_at - size).max(12.0),
                    viewport.height() * (1.0 - sung_at),
                )
            } else {
                (12.0, 60.0)
            };
            ui.add_space(padding);
            let order: Vec<usize> = if reversed {
                (0..lyrics.lines.len()).rev().collect()
            } else {
                (0..lyrics.lines.len()).collect()
            };
            for index in order {
                let line = &lyrics.lines[index];
                let is_active = active == Some(index);
                let lit = ui.ctx().animate_bool_with_time(
                    egui::Id::new("lyric-line").with(("fullscreen", &now.uri, index)),
                    is_active,
                    0.18,
                );
                let color = if lyrics.synced {
                    blend(quiet, palette.text, lit)
                } else {
                    palette.text
                };
                // "Centre, focus": the line being sung is big, the ones around
                // it small, and each grows as its turn nears and shrinks as
                // it fades away.
                let scale = if focus && lyrics.synced {
                    let distance = match active {
                        Some(sung) => index.abs_diff(sung),
                        None => index + 1,
                    };
                    let target = match distance {
                        0 => 1.0,
                        1 => 0.78,
                        2 => 0.64,
                        _ => 0.55,
                    };
                    ui.ctx().animate_value_with_time(
                        egui::Id::new("lyric-scale").with(("fullscreen", &now.uri, index)),
                        target,
                        0.2,
                    )
                } else {
                    1.0
                };
                let font = theme::bold(size * scale);
                // A timed line with no words is the band playing on.
                let text = if line.text.is_empty() && lyrics.synced {
                    "\u{266a}"
                } else {
                    line.text.as_str()
                };
                let sense = if lyrics.synced {
                    Sense::click()
                } else {
                    Sense::hover()
                };
                let side_w = if column_on { stamp_w + STAMP_GAP } else { 0.0 };
                let wrap = if focus {
                    (ui.available_width() - 130.0).max(120.0)
                } else {
                    (ui.available_width() - side_w - 8.0).max(120.0)
                };
                let galley = crate::bidi::layout(ui.painter(), text, font, color, wrap, usize::MAX, None);
                let center = ui.cursor().top() + galley.size().y * 0.5;
                let edge = ((center - viewport.top()).min(viewport.bottom() - center)
                    / (size * 2.0))
                    .clamp(0.0, 1.0);
                let response = ui
                    .scope(|ui| {
                        ui.multiply_opacity(edge * edge * (3.0 - 2.0 * edge));
                        let row_w = ui.available_width();
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(row_w, galley.size().y),
                            sense,
                        );
                        let painter = ui.painter().clone();
                        // Left: time column, small fixed gap, words from one
                        // shared edge. Right: the mirror of that. Centre:
                        // words centred, time at the left edge.
                        let text_x = match align {
                            0 => rect.left() + side_w,
                            2 => rect.right() - side_w - galley.size().x,
                            _ => rect.center().x - galley.size().x / 2.0,
                        };
                        painter.galley(pos2(text_x, rect.top()), galley.clone(), color);
                        // The time of each timed line; the line after the
                        // sung one can show a countdown instead.
                        let shown = if countdown && next_line == Some(index) {
                            until_next.map(|secs| (format!("{secs}s"), true))
                        } else if stamps_on {
                            line.at_ms
                                .filter(|_| lyrics.synced)
                                .map(|at| (crate::util::format_duration_ms(at), false))
                        } else {
                            None
                        };
                        if let Some((label, hot)) = shown {
                            let (x, anchor) = if align == 2 {
                                (rect.right() - 2.0, egui::Align2::RIGHT_CENTER)
                            } else {
                                (rect.left() + 2.0, egui::Align2::LEFT_CENTER)
                            };
                            painter.text(
                                pos2(x, rect.top() + size * 0.62),
                                anchor,
                                label,
                                theme::bold(size * 0.62),
                                if hot {
                                    palette.text
                                } else {
                                    quiet.gamma_multiply(0.8)
                                },
                            );
                        }
                        response
                    })
                    .inner;
                let rect = response.rect;
                if response.secondary_clicked() {
                    copy_lyrics(app, ui.ctx(), &lyrics, &line.text);
                }
                // A thin line under each line, so the next one is easy to find.
                if viewport.y_range().contains(rect.bottom() + 13.0) {
                    ui.painter().with_clip_rect(viewport).hline(
                        (viewport.left() + 2.0)..=(viewport.right() - 2.0),
                        rect.bottom() + 13.0,
                        egui::Stroke::new(1.0, palette.text.gamma_multiply(0.14 * edge)),
                    );
                }
                if lyrics.synced
                    && response.clicked()
                    && let Some(at_ms) = line.at_ms
                {
                    app.actions.push(Action::Seek(at_ms));
                    app.actions.push(Action::FollowLyrics);
                }
                if is_active && follow {
                    show_sung_line(ui, rect, Some(animation), sung_at);
                }
                ui.add_space(27.0);
            }
            // Words without timing can only be followed by the clock: sit
            // at the part of the text the song is probably at.
            if following && !lyrics.synced && now.duration_ms > 0 {
                let fraction =
                    (f64::from(now.position_ms) / f64::from(now.duration_ms)).clamp(0.0, 1.0);
                let content = ui.min_rect();
                let y = content.top() + content.height() * fraction as f32;
                ui.scroll_to_rect_animation(
                    egui::Rect::from_min_max(
                        egui::pos2(content.left(), y),
                        egui::pos2(content.right(), y + 1.0),
                    ),
                    Some(Align::Center),
                    animation,
                );
            }
            if follow && lyrics.synced && active.is_none() && reversed {
                let at = ui.cursor().min + vec2(0.0, viewport.height() * (1.0 - sung_at));
                ui.scroll_to_rect_animation(
                    egui::Rect::from_min_size(at, vec2(1.0, 1.0)),
                    Some(Align::Max),
                    animation,
                );
            }
            ui.add_space(below.max(60.0));
        });
    // Scrolling by hand means the reader wants to look elsewhere; the
    // Follow button in the header picks the song back up.
    if now.playing
        && lyrics.synced
        && let Some(next) = lyrics
            .lines
            .iter()
            .filter_map(|line| line.at_ms)
            .find(|at| *at > now.position_ms)
    {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(u64::from(
                next - now.position_ms,
            )));
    }
}

#[cfg(test)]
mod tests {
    use super::{fullscreen_content_width, preferred_backdrop_art};

    #[test]
    fn fullscreen_backdrop_prefers_small_art_with_large_art_as_fallback() {
        let small = "small".to_string();
        let large = "large".to_string();
        assert_eq!(
            preferred_backdrop_art(Some(small.clone()), Some(large.clone())),
            Some(small)
        );
        assert_eq!(
            preferred_backdrop_art(None, Some(large.clone())),
            Some(large)
        );
        assert_eq!(preferred_backdrop_art(None, None), None);
    }

    #[test]
    fn fullscreen_content_width_never_inverts_a_narrow_viewport() {
        for viewport_width in [0.0, 24.0, 47.0, 48.0, 64.0, 760.0, 2_000.0] {
            let width = fullscreen_content_width(viewport_width);
            assert!(width >= 0.0);
            assert!(width <= (viewport_width - 48.0).max(0.0));
        }
        assert_eq!(fullscreen_content_width(47.0), 0.0);
        assert_eq!(fullscreen_content_width(2_000.0), 960.0);
    }
}

/// The small controls box of full-screen lyrics: play, skip, shuffle and
/// repeat, with the song's progress, under the album art. The bar along the
/// bottom of the window is not drawn on this page.
fn controls_box(app: &mut App, ui: &mut egui::Ui, top_left: egui::Pos2, width: f32, centred: bool) {
    // Fades out a few seconds after the mouse stops, so the lyrics stay
    // clean. Any movement, or a paused song, brings it back.
    let ctx = ui.ctx().clone();
    let id = egui::Id::new("fs-lyrics-box-moved");
    let now_t = ctx.input(|input| input.time);
    let moved = ctx.input(|input| input.pointer.delta() != egui::Vec2::ZERO || input.pointer.any_down());
    let mut last: f64 = ctx.data(|data| data.get_temp(id)).unwrap_or(now_t);
    if moved {
        last = now_t;
    }
    ctx.data_mut(|data| data.insert_temp(id, last));
    let paused = !app.now_playing().is_some_and(|now| now.playing);
    let idle = now_t - last;
    let target = if paused || idle < 3.0 { 1.0 } else { 0.0 };
    let fade = ctx.animate_value_with_time(id.with("fade"), target, 0.6);
    if fade <= 0.01 {
        ctx.request_repaint_after(std::time::Duration::from_millis(300));
        return;
    }
    let old = ui.opacity();
    ui.set_opacity(old * fade);
    controls_box_inner(app, ui, top_left, width, centred);
    ui.set_opacity(old);
    if target > 0.5 && !paused {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    } else if fade > 0.01 {
        ctx.request_repaint();
    }
}

/// Full-screen lyrics: just the five buttons, small, with no panel behind
/// them, at a fixed spot under the cover. Drawn last so it stays on top.
#[allow(dead_code)]
fn controls_slim(app: &mut App, ui: &mut egui::Ui, top_left: egui::Pos2, width: f32, centred: bool) {
    let Some(now) = app.now_playing() else {
        return;
    };
    let k = app.settings.controls_scale_value();
    let kc = (1.0 + (k - 1.0) * 0.5) * 0.8;
    let controls_w = 218.0 * kc;
    let left = if centred { top_left.x - controls_w / 2.0 } else { top_left.x + (width - controls_w) / 2.0 };
    let off = app.settings.lyrics_ctl_off;
    let base = Rect::from_min_size(pos2(left, top_left.y), vec2(controls_w + 20.0, 36.0 * kc));
    let rect = base.translate(vec2(off[0], off[1]));
    // Its own foreground layer: above the title and everything else, and
    // never shifted by them.
    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("fs-lyrics-slim"));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_max(pos2(rect.left() - 20.0, rect.top() - 18.0), rect.max + vec2(20.0, 0.0)))
            .layer_id(layer),
    );
    child.set_opacity(0.85);
    super::player_bar::transport_buttons(app, &mut child, Some(&now), rect, rect.left() + (left - base.left()));
    // A row of dots above the buttons: drag to move them, right-click to put back.
    let grip = Rect::from_center_size(pos2(rect.center().x, rect.top() - 6.0), vec2(40.0, 12.0));
    let grip_response = child.interact(grip, egui::Id::new("fs-lyrics-grip"), Sense::click_and_drag());
    let hot = grip_response.hovered() || grip_response.dragged();
    for dx in [-8.0, 0.0, 8.0] {
        child.painter().circle_filled(
            pos2(grip.center().x + dx, grip.center().y),
            1.6,
            Color32::from_white_alpha(if hot { 170 } else { 50 }),
        );
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    if grip_response.dragged() {
        let d = grip_response.drag_delta();
        app.actions.push(Action::SetLyricsCtlOff([off[0] + d.x, off[1] + d.y]));
    }
    if grip_response.secondary_clicked() {
        app.actions.push(Action::SetLyricsCtlOff([0.0, 0.0]));
    }
    if now.playing {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// How wide the pop-out controls panel is, for the width asked for.
pub(super) fn controls_width(app: &App, width: f32) -> f32 {
    let k = app.settings.controls_scale_value();
    let kc = 1.0 + (k - 1.0) * 0.5;
    width.max(218.0 * kc + 40.0).clamp(300.0, 640.0)
}

/// How tall the pop-out controls panel is: buttons, song bar, volume.
pub(super) fn controls_height(app: &App) -> f32 {
    let k = app.settings.controls_scale_value();
    let kc = 1.0 + (k - 1.0) * 0.5;
    10.0 + 36.0 * kc + 4.0 + 26.0 * k + 4.0 + 24.0 * kc + 10.0
}

pub(super) fn controls_box_inner(app: &mut App, ui: &mut egui::Ui, top_left: egui::Pos2, width: f32, centred: bool) -> Option<Rect> {
    let Some(now) = app.now_playing() else {
        return None;
    };
    // The very same buttons and song-length bar as the bottom bar of the
    // normal view, drawn by the same code, on a panel in the theme's colours.
    let palette = app.palette;
    let k = app.settings.controls_scale_value();
    let kc = 1.0 + (k - 1.0) * 0.5;
    let controls_w = 218.0 * kc;
    let width = controls_width(app, width);
    let left = if centred { top_left.x - width / 2.0 } else { top_left.x };
    let buttons_h = 36.0 * kc;
    let seek_h = 26.0 * k;
    let volume_h = 24.0 * kc;
    let outer = Rect::from_min_size(pos2(left, top_left.y), vec2(width, controls_height(app)));
    ui.painter()
        .rect_filled(outer, 14.0, palette.panel.gamma_multiply(0.94));
    ui.painter().rect_stroke(
        outer,
        14.0,
        egui::Stroke::new(1.0, palette.outline),
        egui::StrokeKind::Inside,
    );
    let buttons_row = Rect::from_min_size(pos2(outer.left(), outer.top() + 10.0), vec2(width, buttons_h));
    let seek_row = Rect::from_min_size(
        pos2(outer.left(), buttons_row.bottom() + 4.0),
        vec2(width, seek_h),
    );
    // The song-length bar always fills the panel, however wide it is made.
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("popout-full-seek"), true));
    super::player_bar::transport(
        app,
        ui,
        Some(&now),
        seek_row,
        buttons_row,
        outer.center().x - controls_w / 2.0,
        (outer.left() + 54.0, outer.right() - 14.0),
    );
    ui.ctx().data_mut(|data| data.remove::<bool>(egui::Id::new("popout-full-seek")));
    // The volume bar stays inside the song bar's ends: never longer.
    let volume_row = Rect::from_min_size(
        pos2(outer.left() + 60.0, seek_row.bottom() + 4.0),
        vec2(width - 120.0, volume_h),
    );
    super::player_bar::volume_row(app, ui, volume_row);
    if now.playing {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    Some(outer)
}
