//! The mini player: the cover, the song, the controls and, if wanted, the
//! queue, in a window of any size down to a small square.
//!
//! It is the same window as the full app, drawn differently. It is on when
//! the reader asked for it (Settings, or the account menu), and also when
//! the window is made smaller than the full layout can use, so shrinking
//! the window never leaves a sidebar squeezed to nothing. Everything here
//! sizes itself from the room it is given: the cover, the words, the
//! buttons and the bars grow and shrink with the window.

use egui::{Align, Color32, Frame, Layout, Rect, Sense, UiBuilder, pos2, vec2};

use crate::app::{App, NowPlaying};
use crate::model::Action;
use crate::player::RepeatMode;
use crate::theme::{self, Icon};
use crate::util;

use super::widgets::{self, SliderEvent};

/// The mini player as a window of its own, like the extra visualizer
/// window: it opens and closes beside the main window and never changes it.
/// Its size and place are kept between launches.
pub fn window(app: &mut App, ctx: &egui::Context) {
    use egui::{ViewportBuilder, ViewportClass, ViewportCommand, ViewportId};
    if !app.settings.mini_player {
        return;
    }
    let id = ViewportId::from_hash_of("chanceify-mini-player");
    let saved = app.settings.mini_window;
    let default_height = if app.settings.mini_queue { 680.0 } else { 340.0 };
    let mut builder = ViewportBuilder::default()
        .with_title("chanceify mini player")
        .with_decorations(false)
        .with_min_inner_size([240.0, 200.0])
        .with_inner_size(saved.map_or([400.0, default_height], |s| [s[2].max(240.0), s[3].max(200.0)]));
    if let Some(s) = saved
        && s[0] > -20000.0
        && s[1] > -20000.0
    {
        builder = builder.with_position([s[0], s[1]]);
    }
    if app.settings.mini_on_top {
        builder = builder.with_always_on_top();
    }
    let mut close = false;
    let mut geometry: Option<[f32; 4]> = None;
    let on_top = app.settings.mini_on_top;
    ctx.show_viewport_immediate(id, builder, |ui, class| {
        let ctx = ui.ctx().clone();
        if class != ViewportClass::Immediate && class != ViewportClass::Root {
            close = true;
            return;
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            close = true;
            return;
        }
        // "Always on top" follows the setting while the window is open.
        let level_id = egui::Id::new("mini-window-top");
        if ctx.data(|data| data.get_temp::<bool>(level_id)) != Some(on_top) {
            ctx.data_mut(|data| data.insert_temp(level_id, on_top));
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(crate::app::on_top_window_level(on_top)));
        }
        // Everything in here sees the mini player's layout.
        app.mini_active = true;
        super::keys::handle(app, &ctx);
        show(app, ui);
        close_button(app, ui);
        super::window_resize(ui);
        app.mini_active = false;
        let (outer, inner) = ctx.input(|input| (input.viewport().outer_rect, input.viewport().inner_rect));
        if let (Some(outer), Some(inner)) = (outer, inner) {
            geometry = Some([outer.min.x, outer.min.y, inner.width(), inner.height()]);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    });
    app.mini_active = false;
    if close {
        app.settings.mini_player = false;
        app.mark_settings_dirty();
    } else if let Some(now) = geometry
        && saved.is_none_or(|old| old.iter().zip(now).any(|(a, b)| (a - b).abs() > 1.0))
    {
        app.settings.mini_window = Some(now);
        app.mark_settings_dirty();
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let ctx = ui.ctx().clone();
    let now = app.now_playing();
    egui::CentralPanel::default()
        .frame(Frame::new().fill(palette.window))
        .show(ui, |ui| {
            let area = ui.max_rect();
            // Something bright moving behind everything, in the cover's
            // colours, sized to whatever the window is.
            // The very same visualizer, with the very same settings, as
            // everywhere else (Visualizer settings in the right-click menu).
            // The mini player has its own look (Off, Bars, Flow or Swirl from
            // its menu), whatever the big visualizer is set to.
            let saved_shapes = (app.settings.vis_shapes, app.settings.vis_shapes_set);
            let shapes = [
                0,
                crate::settings::Settings::SHAPE_BARS,
                crate::settings::Settings::SHAPE_FLOW,
                crate::settings::Settings::SHAPE_SWIRL,
            ][app.settings.mini_vis_mode() as usize];
            app.settings.vis_shapes = shapes;
            app.settings.vis_shapes_set = true;
            if shapes != 0 {
                let moving =
                    super::player_bar::lyrics_backdrop(app, ui, area, now.as_ref(), 3);
                if shapes != crate::settings::Settings::SHAPE_BARS {
                    // Flow and swirl fill the window: a light veil keeps
                    // the words readable.
                    ui.painter()
                        .with_clip_rect(area)
                        .rect_filled(area, 0.0, Color32::from_black_alpha(80));
                }
                if moving {
                    ui.ctx().request_repaint_after(std::time::Duration::from_micros(16_667));
                }
            }
            (app.settings.vis_shapes, app.settings.vis_shapes_set) = saved_shapes;
            // Right-click anywhere that is not a control for the mini
            // player's options. (Added first, so every control drawn after it
            // keeps its own clicks.)
            let page = ui.interact(area, egui::Id::new("mini-page"), Sense::click_and_drag());
            // Drag the little player by any bare spot.
            if page.drag_started() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }
            options_menu(app, &page);
            // The top strip holds three small buttons, and is already the
            // place that drags the window (`show` registers that for the
            // whole top of the window). Where the app draws its own window
            // buttons they sit over its right-hand end, so it is as tall as
            // they are.
            let strip_height = if super::windows_chrome_visible_here(&ctx) {
                super::WINDOWS_WINDOW_CONTROLS_HEIGHT
            } else {
                32.0
            };
            let strip = Rect::from_min_size(area.min, vec2(area.width(), strip_height));
            strip_buttons(app, ui, strip);

            let pad = (area.width() * 0.04).clamp(8.0, 22.0);
            let body = Rect::from_min_max(
                pos2(area.left() + pad, strip.bottom()),
                pos2(area.right() - pad, area.bottom() - pad),
            );
            if body.width() < 40.0 || body.height() < 40.0 {
                return;
            }
            let mut inner = ui.new_child(
                UiBuilder::new()
                    .max_rect(body)
                    .layout(Layout::top_down(Align::Center)),
            );
            if body.width() >= body.height() * 1.45 {
                landscape(app, &mut inner, body, now.as_ref());
            } else {
                portrait(app, &mut inner, body, now.as_ref());
            }
        });
}

/// The right-click menu of the mini player.
fn options_menu(app: &mut App, page: &egui::Response) {
    let palette = app.palette;
    egui::Popup::context_menu(page)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| {
            let tick = |on: bool| if on { Some(Icon::Check) } else { None };
            if widgets::menu_item(ui, &palette, tick(app.settings.mini_queue), "Queue") {
                app.actions.push(Action::ToggleMiniQueue);
            }
            if widgets::menu_item(ui, &palette, tick(app.settings.mini_volume), "Volume") {
                app.actions.push(Action::ToggleMiniVolume);
            }
            for (mode, label) in [(0u8, "Nothing moving"), (1, "Bars"), (2, "Flow"), (3, "Swirl")] {
                if widgets::menu_item(ui, &palette, tick(app.settings.mini_vis_mode() == mode), label) {
                    app.actions.push(Action::SetMiniVis(mode));
                }
            }
            widgets::menu_separator(ui, &palette);
            if cfg!(windows)
                && widgets::menu_item(
                    ui,
                    &palette,
                    tick(app.settings.mini_fade),
                    "Fade when the pointer is away",
                )
            {
                app.actions.push(Action::ToggleMiniFade);
            }
        });
}

/// Back to the full window, the queue and the stay-on-top switches.
fn strip_buttons(app: &mut App, ui: &mut egui::Ui, strip: Rect) {
    let palette = app.palette;
    let mut row = ui.new_child(
        UiBuilder::new()
            .max_rect(strip)
            .layout(Layout::left_to_right(Align::Center)),
    );
    row.add_space(28.0);
    let queue = app.settings.mini_queue;
    if theme::icon_button(
        &mut row,
        Icon::ListMusic,
        15.0,
        if queue { palette.accent } else { palette.secondary },
        if queue { palette.accent_hover } else { palette.text },
        if queue { "Hide the queue" } else { "Show the queue" },
    )
    .clicked()
    {
        app.actions.push(Action::ToggleMiniQueue);
    }
    let volume = app.settings.mini_volume;
    if theme::icon_button(
        &mut row,
        Icon::Volume2,
        15.0,
        if volume { palette.accent } else { palette.secondary },
        if volume { palette.accent_hover } else { palette.text },
        "Volume",
    )
    .clicked()
    {
        app.actions.push(Action::ToggleMiniVolume);
    }
}

/// How opaque the floating mini player is, 0 to 1: solid while the pointer
/// is on it (and for a moment after), see-through once it has been away.
/// Always 1 unless the mini player floats over everything with the fade on.
#[allow(dead_code)]
pub fn fade_level(app: &App, ctx: &egui::Context) -> f32 {
    const AWAY: f32 = 0.42;
    const LINGER: f64 = 1.0;
    if !(app.settings.mini_fade && app.settings.mini_on_top) {
        return 1.0;
    }
    let now = ctx.input(|input| input.time);
    let near = ctx.input(|input| input.pointer.hover_pos().is_some() || input.pointer.any_down());
    let id = egui::Id::new("mini-pointer-last-near");
    let last = if near {
        ctx.data_mut(|data| data.insert_temp(id, now));
        now
    } else {
        ctx.data(|data| data.get_temp::<f64>(id)).unwrap_or(f64::MIN)
    };
    let since = now - last;
    let want = if since < LINGER { 1.0 } else { AWAY };
    if !near && since < LINGER {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(
            (LINGER - since).clamp(0.05, LINGER),
        ));
    }
    ctx.animate_value_with_time(egui::Id::new("mini-fade-level"), want, 0.35)
}

/// Cover on top, then the song, the bars and the controls, then the queue
/// in whatever room is left.
fn portrait(app: &mut App, ui: &mut egui::Ui, body: Rect, now: Option<&NowPlaying>) {
    let palette = app.palette;
    let width = body.width();
    let unit = (width / 400.0).clamp(0.7, 1.7);
    let queue = app.settings.mini_queue;
    // What sits under the cover: two lines of words, the song bar and the
    // controls, and the volume when there is room for it.
    let words = 52.0 * unit;
    let bars = 30.0 * unit;
    let controls = 46.0 * unit;
    let volume = if app.settings.mini_volume && body.height() > 200.0 {
        28.0 * unit
    } else {
        0.0
    };
    let gaps = 40.0;
    let under = words + bars + controls + volume + gaps;
    let side = if queue {
        width.min(body.height() * 0.40).min(340.0)
    } else {
        width.min(body.height() - under)
    }
    .max(48.0);
    let (cover_rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
    cover(app, ui, cover_rect, now);
    ui.add_space(10.0 * unit);
    song_words(app, ui, width, unit, now);
    ui.add_space(4.0 * unit);
    seek_row(app, ui, width, unit, now);
    ui.add_space(2.0 * unit);
    control_row(app, ui, unit, now);
    if volume > 0.0 {
        ui.add_space(4.0 * unit);
        volume_row(app, ui, (width * 0.7).min(260.0), now);
    }
    if queue {
        let left = body.bottom() - ui.cursor().top();
        if left >= 90.0 {
            ui.add_space(8.0);
            queue_list(app, ui, body, palette);
        }
    }
}

/// Cover at the left, everything else in a column beside it.
fn landscape(app: &mut App, ui: &mut egui::Ui, body: Rect, now: Option<&NowPlaying>) {
    let side = body.height().min(body.width() * 0.4).max(40.0);
    let cover_rect = Rect::from_min_size(body.min, vec2(side, side));
    cover(app, ui, cover_rect, now);
    let gap = (body.width() * 0.03).clamp(8.0, 24.0);
    let column = Rect::from_min_max(pos2(cover_rect.right() + gap, body.top()), body.max);
    if column.width() < 80.0 {
        return;
    }
    let unit = (column.height() / 150.0).clamp(0.6, 1.5).min((column.width() / 260.0).max(0.6));
    let mut right = ui.new_child(
        UiBuilder::new()
            .max_rect(column)
            .layout(Layout::top_down(Align::Center)),
    );
    // The volume joins the column when there is room for it.
    let with_volume = app.settings.mini_volume && column.height() >= 110.0;
    // Centred up and down in the column.
    let content = (52.0 + 30.0 + 46.0 + if with_volume { 28.0 } else { 0.0 }) * unit + 12.0;
    right.add_space(((column.height() - content) / 2.0).max(0.0));
    song_words(app, &mut right, column.width(), unit, now);
    right.add_space(4.0 * unit);
    seek_row(app, &mut right, column.width(), unit, now);
    right.add_space(2.0 * unit);
    control_row(app, &mut right, unit, now);
    if with_volume {
        right.add_space(4.0 * unit);
        volume_row(app, &mut right, (column.width() * 0.7).min(260.0), now);
    }
}

fn cover(app: &mut App, ui: &mut egui::Ui, rect: Rect, now: Option<&NowPlaying>) {
    let palette = app.palette;
    let loader = app.backend.art().clone();
    let url = now.and_then(|now| now.art_url.as_deref().or(now.art_small.as_deref()));
    widgets::paint_cover(
        ui,
        &palette,
        url,
        rect,
        (rect.width() * 0.04).clamp(4.0, 14.0),
        Icon::Music,
        Some(&loader),
    );
}

/// The title, then the artist, each on one line cut with an ellipsis.
fn song_words(app: &mut App, ui: &mut egui::Ui, width: f32, unit: f32, now: Option<&NowPlaying>) {
    let palette = app.palette;
    let (title, artist) = match now {
        Some(now) => (now.title.as_str(), now.subtitle.as_str()),
        None => ("Nothing playing", ""),
    };
    for (text, font, color, height) in [
        (title, theme::bold(18.0 * unit), palette.text, 26.0 * unit),
        (artist, theme::regular(13.5 * unit), palette.secondary, 20.0 * unit),
    ] {
        let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        if text.is_empty() {
            continue;
        }
        let galley = crate::bidi::layout(
            ui.painter(),
            text,
            font,
            color,
            width.max(10.0),
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        ui.painter().galley(
            pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
}

/// The time so far, the song-length bar and the length.
fn seek_row(app: &mut App, ui: &mut egui::Ui, width: f32, unit: f32, now: Option<&NowPlaying>) {
    let palette = app.palette;
    let (position, duration) = now.map_or((0, 0), |now| (now.position_ms, now.duration_ms));
    let shown = match app.seek_preview {
        Some(fraction) => (fraction * duration as f32) as u32,
        None => position,
    };
    let height = 26.0 * unit;
    let (row, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let size = 12.0 * unit.clamp(0.85, 1.4);
    let left_text = util::format_duration_ms(shown);
    let right_text = util::format_duration_ms(duration);
    let text_width = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), theme::regular(size), Color32::WHITE)
            .size()
            .x
            .ceil()
            + 4.0
    };
    // Both ends take the same room, the wider of the two, so the bar sits
    // in the middle of the row whatever the times say.
    let room = text_width(&left_text).max(text_width(&right_text)).max(text_width("0:00"));
    let colour = if now.is_some() { palette.secondary } else { palette.dim };
    ui.painter().text(
        pos2(row.left(), row.center().y),
        egui::Align2::LEFT_CENTER,
        left_text,
        theme::regular(size),
        colour,
    );
    ui.painter().text(
        pos2(row.right(), row.center().y),
        egui::Align2::RIGHT_CENTER,
        right_text,
        theme::regular(size),
        colour,
    );
    let slider_width = (width - room * 2.0 - 16.0).max(30.0);
    let slider_rect = Rect::from_center_size(row.center(), vec2(slider_width, height));
    let mut slider_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(slider_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    let fraction = if duration > 0 {
        position as f32 / duration as f32
    } else {
        0.0
    };
    match widgets::thin_slider_scaled(
        &mut slider_ui,
        &palette,
        egui::Id::new("mini-seek"),
        "Playback position (%)",
        fraction,
        slider_width,
        None,
        unit.clamp(0.7, 1.6),
    ) {
        SliderEvent::Dragging(value) => app.seek_preview = Some(value),
        SliderEvent::Committed(value) => {
            app.seek_preview = None;
            if duration > 0 {
                app.actions
                    .push(Action::Seek((value * duration as f32) as u32));
            }
        }
        SliderEvent::None => {}
    }
}

/// Shuffle, back, play or pause, forward, repeat.
fn control_row(app: &mut App, ui: &mut egui::Ui, unit: f32, now: Option<&NowPlaying>) {
    let palette = app.palette;
    let enabled = now.is_some_and(|now| now.can_control) || app.is_connected();
    let playing = now.is_some_and(|now| now.playing);
    let loading = now.is_some_and(|now| now.loading);
    let shuffle = now.map_or_else(|| app.playing_context_shuffle(), |now| now.shuffle);
    let repeat = now.map(|now| now.repeat).unwrap_or_default();
    let dim = if enabled { palette.secondary } else { palette.dim };
    let widths = [29.0 * unit, 30.0 * unit, 38.0 * unit, 30.0 * unit, 29.0 * unit];
    let gap = 10.0 * unit;
    let total = widths.iter().sum::<f32>() + gap * 4.0;
    let (row, _) = ui.allocate_exact_size(vec2(total, 40.0 * unit), Sense::hover());
    let mut x = row.left();
    let mut cell = |ui: &mut egui::Ui, index: usize| {
        let rect = Rect::from_center_size(
            pos2(x + widths[index] / 2.0, row.center().y),
            vec2(widths[index], 38.0 * unit),
        );
        x += widths[index] + gap;
        ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        )
    };

    let mut shuffle_cell = cell(ui, 0);
    if theme::icon_button(
        &mut shuffle_cell,
        Icon::Shuffle,
        17.0 * unit,
        if shuffle { palette.accent } else { dim },
        if shuffle { palette.accent_hover } else { palette.text },
        "Shuffle",
    )
    .clicked()
    {
        app.actions.push(Action::ToggleShuffle);
    }

    let mut back_cell = cell(ui, 1);
    if theme::icon_button(
        &mut back_cell,
        Icon::SkipBackFilled,
        18.0 * unit,
        dim,
        palette.text,
        "Previous",
    )
    .clicked()
    {
        app.actions.push(Action::Previous);
    }

    let mut play_cell = cell(ui, 2);
    if loading || app.any_play_pending() {
        play_cell
            .painter()
            .circle_filled(play_cell.max_rect().center(), 19.0 * unit, palette.text);
        theme::spinner(&mut play_cell, 22.0 * unit, palette.window);
    } else {
        let hover = if palette.dark { Color32::WHITE } else { palette.text };
        if theme::circle_button(
            &mut play_cell,
            if playing { Icon::PauseFilled } else { Icon::PlayFilled },
            38.0 * unit,
            palette.text,
            hover,
            palette.window,
            if playing { "Pause" } else { "Play" },
        )
        .clicked()
        {
            app.actions.push(Action::TogglePlay);
        }
    }

    let mut next_cell = cell(ui, 3);
    if theme::icon_button(
        &mut next_cell,
        Icon::SkipForwardFilled,
        18.0 * unit,
        dim,
        palette.text,
        "Next",
    )
    .clicked()
    {
        app.actions.push(Action::Next);
    }

    let (repeat_icon, repeat_colour, hover, tip) = match repeat {
        RepeatMode::Off => (Icon::Repeat, dim, palette.text, "Repeat"),
        RepeatMode::Context => (Icon::Repeat, palette.accent, palette.accent_hover, "Repeat one"),
        RepeatMode::Track => (Icon::Repeat1, palette.accent, palette.accent_hover, "Repeat off"),
    };
    let mut repeat_cell = cell(ui, 4);
    if theme::icon_button(&mut repeat_cell, repeat_icon, 17.0 * unit, repeat_colour, hover, tip)
        .clicked()
    {
        app.actions.push(Action::CycleRepeat);
    }
}

/// A thin volume bar.
fn volume_row(app: &mut App, ui: &mut egui::Ui, width: f32, now: Option<&NowPlaying>) {
    let palette = app.palette;
    let volume = now
        .map(|now| now.volume_percent)
        .unwrap_or_else(|| crate::app::volume_to_percent(app.local.volume));
    let shown = match app.volume_preview {
        Some(fraction) => (fraction * 100.0).round() as u8,
        None => volume,
    };
    let (row, _) = ui.allocate_exact_size(vec2(width, 26.0), Sense::hover());
    // The number sits at the right end, so the volume can be read.
    ui.painter().text(
        pos2(row.right(), row.center().y),
        egui::Align2::RIGHT_CENTER,
        format!("{shown}%"),
        theme::regular(12.0),
        palette.secondary,
    );
    let width = (width - 38.0).max(30.0);
    let mut slider_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(Rect::from_min_size(row.min, vec2(width, row.height())))
            .layout(Layout::left_to_right(Align::Center)),
    );
    match widgets::thin_slider(
        &mut slider_ui,
        &palette,
        egui::Id::new("mini-volume"),
        "Volume (%)",
        shown as f32 / 100.0,
        width,
        Some(0.05),
    ) {
        SliderEvent::Dragging(value) => {
            app.volume_preview = Some(value);
            if now.is_none_or(|now| now.local) {
                app.actions
                    .push(Action::PreviewVolume((value * 100.0).round() as u8));
            }
        }
        SliderEvent::Committed(value) => {
            app.volume_preview = None;
            app.actions
                .push(Action::SetVolume((value * 100.0).round() as u8));
        }
        SliderEvent::None => {}
    }
    if let Some(next) = super::player_bar::wheel_volume(ui, slider_ui.min_rect().expand(6.0), shown) {
        app.volume_preview = None;
        app.actions.push(Action::SetVolume(next));
    }
}

/// The queue, scrolling in the room under the controls.
fn queue_list(app: &mut App, ui: &mut egui::Ui, body: Rect, palette: theme::Palette) {
    // The queue is only fetched for the side panel, so ask for it here the
    // moment the mini player wants it instead of waiting for a refresh.
    if matches!(app.queue, crate::model::Loadable::NotLoaded) {
        app.actions.push(Action::RefreshQueue);
    }
    let height = (body.bottom() - ui.cursor().top()).max(40.0);
    let list = Rect::from_min_size(
        pos2(body.left(), ui.cursor().top()),
        vec2(body.width(), height),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(list)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(list.intersect(ui.clip_rect()));
    child
        .painter()
        .rect_filled(list, egui::CornerRadius::same(10), palette.panel);
    let inner = list.shrink(6.0);
    let mut scroll_ui = child.new_child(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    egui::ScrollArea::vertical()
        .id_salt("mini-queue-scroll")
        .auto_shrink([false, false])
        .show(&mut scroll_ui, |ui| {
            super::queue::contents(app, ui, true);
        });
}

/// A small close button in the top-right corner, drawn only while the
/// pointer is over the window. It is the only way out besides the disc.
fn close_button(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let hovered = ctx.input(|input| input.pointer.hover_pos()).is_some_and(|p| screen.contains(p));
    let rect = Rect::from_min_size(pos2(screen.right() - 24.0, screen.top() + 4.0), vec2(20.0, 20.0));
    let id = egui::Id::new("mini-close");
    let response = ui.interact(rect, id, Sense::click());
    let strength = ctx.animate_bool_with_time(id.with("fade"), hovered, 0.15);
    if strength > 0.01 {
        let colour = if response.hovered() { palette.text } else { palette.secondary };
        if response.hovered() {
            ui.painter().rect_filled(rect, 10.0, palette.surface.gamma_multiply(strength));
        }
        let c = rect.center();
        let d = 4.0;
        let stroke = egui::Stroke::new(1.5, colour.gamma_multiply(strength));
        ui.painter().line_segment([c + vec2(-d, -d), c + vec2(d, d)], stroke);
        ui.painter().line_segment([c + vec2(-d, d), c + vec2(d, -d)], stroke);
        if response.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    if response.clicked() {
        app.actions.push(Action::ToggleMiniPlayer);
    }
}
