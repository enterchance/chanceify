//! One extra window that shows the visualizer, for a second monitor.
//!
//! It draws with the very same settings and the very same audio as the main
//! window, so it needs no settings of its own: it copies what the main
//! window shows. Double-click (or F11) makes it full screen on whichever
//! monitor it sits on; drag it to another monitor first. Esc leaves full
//! screen, and closes the window when it is not full screen. The X closes it
//! too, and so does the End key or the Views panel in the main window, so it
//! can never be stuck on a screen you cannot see.

use egui::{Color32, Context, Frame, ViewportBuilder, ViewportClass, ViewportCommand, ViewportId, pos2};

use crate::app::App;
use crate::theme;

pub fn show(app: &mut App, ctx: &Context) {
    if !app.extra_vis {
        app.extra_vis_since = None;
        return;
    }
    let since = *app
        .extra_vis_since
        .get_or_insert_with(std::time::Instant::now);
    let id = ViewportId::from_hash_of("chanceify-extra-visualizer");
    let saved = app.settings.extra_window;
    let mut builder = ViewportBuilder::default()
        .with_title("chanceify visualizer")
        .with_decorations(false)
        .with_inner_size(saved.map_or([960.0, 540.0], |s| [s[2].max(240.0), s[3].max(160.0)]))
        .with_min_inner_size([240.0, 160.0]);
    if let Some(s) = saved
        && s[0] > -20000.0
        && s[1] > -20000.0
    {
        builder = builder.with_position([s[0], s[1]]);
    }
    let mut geometry: Option<[f32; 4]> = None;
    let mut close = false;
    let now = app.now_playing();
    let was_full = app.fullscreen_vis;
    ctx.show_viewport_immediate(id, builder, |ui, class| {
        let ctx = ui.ctx().clone();
        // Where the platform cannot open a second window, egui would draw it
        // as a floating box inside the main one. That is not what this is for.
        if class != ViewportClass::Immediate && class != ViewportClass::Root {
            close = true;
            return;
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            close = true;
            return;
        }
        let full = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
        if ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            if full {
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
            } else {
                close = true;
                return;
            }
        }
        if ctx.input(|input| input.key_pressed(egui::Key::F11)) {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!full));
        }
        let lyrics_mode = app.extra_mode == 1;
        let rect = ctx.content_rect();
        // The page: the visualizer, or the full-screen lyrics page.
        if lyrics_mode {
            let had = app.lyrics_fullscreen;
            if had.is_none() {
                app.lyrics_fullscreen = Some(false);
            }
            let before = app.actions.len();
            super::lyrics::fullscreen(app, ui);
            // The page's right-click opens the settings: in this window that
            // is this window's own panel.
            let mut i = before;
            while i < app.actions.len() {
                if matches!(app.actions[i], crate::model::Action::ToggleVisPanel) {
                    app.actions.remove(i);
                    app.extra_vis_panel = !app.extra_vis_panel;
                } else {
                    i += 1;
                }
            }
            if had.is_none() {
                app.lyrics_fullscreen = None;
            }
        } else {
            egui::CentralPanel::default()
                .frame(Frame::new().fill(Color32::BLACK))
                .show(ui, |ui| {
                    // The visualizer paints as it does full screen.
                    app.fullscreen_vis = true;
                    let moving = super::player_bar::lyrics_backdrop(app, ui, rect, now.as_ref(), 3);
                    app.fullscreen_vis = was_full;
                    if !moving {
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "The visualizer is off. Turn it on in the main window (Views > Normal).",
                            theme::regular(15.0),
                            Color32::from_gray(150),
                        );
                    }
                });
        }
        // Over the page: the whole window drags it, double-click is full
        // screen, right-click is the settings.
        // (Over the lyrics only the top strip is a handle, so the lines keep
        // their own clicks.)
        let handle = if lyrics_mode {
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 56.0))
        } else {
            rect
        };
        let response = ui.interact(handle, ui.id().with("extra-vis"), egui::Sense::click_and_drag());
        if response.double_clicked() {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!full));
        }
        if response.drag_started() && !full {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if response.secondary_clicked() && !lyrics_mode {
            app.extra_vis_panel = !app.extra_vis_panel;
        }
        // The visualizer's settings, in a panel over the picture. It has its
        // own open/closed state, apart from the main window's.
        let was_open = app.extra_vis_panel;
        let (saved_panel, saved_full) = (app.vis_panel, app.fullscreen_vis);
        app.vis_panel = was_open;
        app.fullscreen_vis = true;
        super::player_bar::vis_panel_window(app, &ctx);
        app.extra_vis_panel = app.vis_panel;
        app.vis_panel = saved_panel;
        app.fullscreen_vis = saved_full;
        // Three small buttons: settings, visualizer or lyrics, and close.
        let (pointer, pressed) = ctx.input(|input| (input.pointer.hover_pos(), input.pointer.primary_pressed()));
        let near = pointer.is_some_and(|p| rect.contains(p) && p.y < rect.top() + 70.0);
        let alpha = if near { 200 } else { 70 };
        let size = egui::vec2(32.0, 32.0);
        let wave = egui::Rect::from_min_size(pos2(rect.left() + 10.0, rect.top() + 10.0), size);
        let swap = egui::Rect::from_min_size(pos2(rect.left() + 48.0, rect.top() + 10.0), size);
        let exit = egui::Rect::from_min_size(pos2(rect.right() - 42.0, rect.top() + 10.0), size);
        let swap_icon = if lyrics_mode { theme::Icon::AudioLines } else { theme::Icon::Mic };
        let painter = ui.painter().clone();
        for (button, icon) in [(wave, theme::Icon::Settings), (swap, swap_icon), (exit, theme::Icon::X)] {
            let hot = pointer.is_some_and(|p| button.contains(p));
            painter.circle_filled(button.center(), 16.0, Color32::from_black_alpha(if hot { 190 } else { 120 }));
            let icon_rect = egui::Rect::from_center_size(button.center(), egui::vec2(18.0, 18.0));
            theme::paint_icon(ui, icon, icon_rect, 18.0, Color32::from_white_alpha(if hot { 255 } else { alpha }));
            if hot {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        if pressed && let Some(p) = pointer {
            if wave.contains(p) {
                app.extra_vis_panel = !was_open;
            } else if swap.contains(p) {
                app.extra_mode = if lyrics_mode { 0 } else { 1 };
            } else if exit.contains(p) {
                close = true;
            }
        }
        super::window_resize(ui);
        if !lyrics_mode {
            let age = since.elapsed().as_secs_f32();
            if age < 7.0 {
                let fade = (1.0 - (age - 5.0).max(0.0) / 2.0).clamp(0.0, 1.0);
                painter.text(
                    pos2(rect.center().x, rect.bottom() - 24.0),
                    egui::Align2::CENTER_CENTER,
                    "Double-click for full screen. Right-click for settings. Esc closes.",
                    theme::regular(13.0),
                    Color32::from_white_alpha((150.0 * fade) as u8),
                );
            }
        }
        // Remember where it sits and how big, unless it is full screen.
        if !full {
            let (outer, inner) = ctx.input(|input| (input.viewport().outer_rect, input.viewport().inner_rect));
            if let (Some(outer), Some(inner)) = (outer, inner) {
                geometry = Some([outer.min.x, outer.min.y, inner.width(), inner.height()]);
            }
        }
        ctx.request_repaint();
    });
    app.fullscreen_vis = was_full;
    if close {
        app.extra_vis = false;
        app.extra_vis_since = None;
    } else if let Some(now) = geometry
        && saved.is_none_or(|old| old.iter().zip(now).any(|(a, b)| (a - b).abs() > 1.0))
    {
        app.settings.extra_window = Some(now);
        app.mark_settings_dirty();
    }
}
