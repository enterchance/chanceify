//! The Views and panels window: a small panel that floats above everything.
//!
//! Right-click the disc in the sidebar (or the disc shown in the corner of
//! the full-screen views) to open it. It has no backdrop, does not close when
//! you click elsewhere, and can be dragged and resized.

use egui::{Color32, Context, CornerRadius, Frame, Id, Margin, Rect, Stroke};

use crate::app::App;
use crate::model::{Action, ViewKind};
use crate::theme::{self, Icon, Palette};

use super::widgets;

/// The panel is always dark, so it reads over any theme or visualizer.
fn dark_palette(app: &App) -> Palette {
    let mut palette = Palette::dark();
    palette.accent = app.palette.accent;
    palette.accent_hover = app.palette.accent_hover;
    palette.on_accent = app.palette.on_accent;
    palette
}

/// One line of the zoom control: smaller, the size, bigger.
pub fn zoom_row(ui: &mut egui::Ui, app: &mut App, palette: &Palette) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        theme::text(ui, "Zoom", theme::medium(13.5), palette.text);
        if theme::soft_button(ui, palette, None, "-", false).clicked() {
            app.actions.push(Action::AdjustZoom(-1));
        }
        if theme::soft_button(ui, palette, None, &format!("{:.0}%", app.settings.zoom * 100.0), false)
            .on_hover_text("Back to 100%")
            .clicked()
        {
            app.actions.push(Action::AdjustZoom(0));
        }
        if theme::soft_button(ui, palette, None, "+", false).clicked() {
            app.actions.push(Action::AdjustZoom(1));
        }
    });
}

/// The Views disc: a small disc the mouse can drag anywhere and click to open
/// the Views panel, on every screen. Until it has been moved, it stays out of
/// the way where the sidebar has its own disc.
/// How much of the disc shows: all of it while the pointer is near (or the
/// panel is open), `floor` of it after a few seconds away.
fn idle_dim(ctx: &Context, panel_open: bool, floor: f32, over: bool) -> f32 {
    let seen_id = Id::new("views-disc-seen");
    let now_t = ctx.input(|input| input.time);
    if over {
        ctx.data_mut(|data| data.insert_temp(seen_id, now_t));
    }
    let last_seen: f64 = ctx.data(|data| data.get_temp(seen_id)).unwrap_or(now_t);
    let calm = !panel_open && now_t - last_seen > 2.5;
    let dim = ctx.animate_value_with_time(seen_id.with("dim"), if calm { floor.clamp(0.05, 1.0) } else { 1.0 }, 0.5);
    if !calm && !panel_open {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    dim
}

/// The disc as a fixed button in the top bar, left of the playing-from
/// cover: a click opens or closes the panel.
pub fn topbar_disc(ui: &mut egui::Ui, app: &mut App) {
    let palette = dark_palette(app);
    let size = 32.0;
    let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
    let over = response.hovered();
    let dim = idle_dim(ui.ctx(), app.views_panel, app.settings.disc_dim, over);
    ui.set_opacity(dim);
    ui.painter().circle_filled(rect.center(), size / 2.0, Color32::from_black_alpha(if over { 190 } else { 140 }));
    ui.painter().circle_stroke(rect.center(), size / 2.0, Stroke::new(1.0, Color32::from_white_alpha(if over { 90 } else { 40 })));
    mono_logo(ui, rect.center(), 20.0, if over { palette.text } else { Color32::from_white_alpha(150) });
    ui.set_opacity(1.0);
    if over {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() || response.secondary_clicked() {
        app.actions.push(Action::ToggleViewsPanel);
    }
}

/// The chanceify logo (a record with a play triangle) in one colour, for the
/// views button.
fn mono_logo(ui: &egui::Ui, center: egui::Pos2, diameter: f32, colour: Color32) {
    let radius = diameter / 2.0;
    let painter = ui.painter();
    painter.circle_stroke(center, radius, Stroke::new((diameter * 0.09).max(1.2), colour));
    painter.circle_stroke(center, radius * 0.72, Stroke::new(1.0, colour.gamma_multiply(0.5)));
    let w = radius * 0.46;
    let h = radius * 0.5;
    painter.add(egui::Shape::convex_polygon(
        vec![
            center + egui::vec2(-w * 0.55, -h),
            center + egui::vec2(w * 1.05, 0.0),
            center + egui::vec2(-w * 0.55, h),
        ],
        colour,
        Stroke::NONE,
    ));
}

pub fn corner_disc(app: &mut App, ctx: &Context) {
    // One fixed place: the top-left corner of every view without a top bar.
    let moved: Option<[f32; 2]> = None;
    let palette = dark_palette(app);
    let screen = ctx.content_rect();
    let size = if app.mini_active { 16.0 } else { 22.0 };
    let home = if app.mini_active {
        egui::pos2(4.0, 4.0)
    } else {
        egui::pos2(14.0, 14.0 + theme::titlebar_inset(ctx))
    };
    let at = moved.map_or(home, |[x, y]| egui::pos2(x, y));
    let at = egui::pos2(
        at.x.clamp(screen.left(), (screen.right() - size - 12.0).max(screen.left())),
        at.y.clamp(screen.top(), (screen.bottom() - size - 12.0).max(screen.top())),
    );
    let drop_at: Option<egui::Pos2> = None;
    let reset = false;
    let hit = egui::Rect::from_min_size(at, egui::Vec2::splat(size + 12.0));
    // The press, the move and the release are read straight from the pointer,
    // so no other layer can ever swallow a click on the disc.
    let grab_id = Id::new("views-disc-grab");
    let (pos, pressed, down, released, right_released, _now_t) = ctx.input(|input| {
        (
            input.pointer.interact_pos(),
            input.pointer.primary_pressed(),
            input.pointer.primary_down(),
            input.pointer.primary_released(),
            input.pointer.button_released(egui::PointerButton::Secondary),
            input.time,
        )
    });
    // (where the press started, offset inside the disc, travelled far enough to be a drag)
    let mut grab: Option<(egui::Pos2, egui::Vec2, bool)> = ctx
        .data(|data| data.get_temp::<Option<(egui::Pos2, egui::Vec2, bool)>>(grab_id))
        .flatten();
    let mut toggle = false;
    if pressed && let Some(p) = pos.filter(|p| hit.contains(*p)) {
        grab = Some((p, p - at, false));
    }
    if let Some((start, offset, moved)) = grab {
        if let Some(p) = pos {
            if down && (moved || (p - start).length() > 6.0) {
                grab = Some((start, offset, true));
                let _ = offset;
            }
        }
        if released || !down && !pressed {
            if !moved && released && pos.is_some_and(|p| hit.expand(6.0).contains(p)) {
                toggle = true;
            }
            grab = None;
        }
    }
    if right_released && pos.is_some_and(|p| hit.contains(p)) {
        toggle = true;
    }
    ctx.data_mut(|data| data.insert_temp(grab_id, grab));
    let over = pos.is_some_and(|p| hit.contains(p)) || grab.is_some();
    let dim = idle_dim(ctx, app.views_panel, app.settings.disc_dim, over || grab.is_some());
    if toggle {
        app.actions.push(Action::ToggleViewsPanel);
        ctx.request_repaint();
    }
    egui::Area::new(Id::new("views-corner-disc"))
        .order(egui::Order::Tooltip)
        .interactable(true)
        .fixed_pos(at)
        .show(ctx, |ui| {
            ui.set_opacity(dim);
            let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size + 12.0), egui::Sense::click_and_drag());
            let hot = over;
            // A round dark back, so the disc reads over any picture.
            ui.painter().circle_filled(
                rect.center(),
                (size + 12.0) / 2.0,
                Color32::from_rgba_unmultiplied(if hot { 20 } else { 14 }, if hot { 22 } else { 16 }, if hot { 26 } else { 20 }, if hot { 235 } else { 110 }),
            );
            ui.painter().circle_stroke(
                rect.center(),
                (size + 12.0) / 2.0,
                Stroke::new(1.0, Color32::from_white_alpha(if hot { 90 } else { 40 })),
            );
            mono_logo(
                ui,
                rect.center(),
                size,
                if hot { palette.text } else { Color32::from_white_alpha(if app.mini_active { 110 } else { 150 }) },
            );
            let _ = response;
            if hot {
                ui.ctx().set_cursor_icon(if grab.is_some_and(|g| g.2) {
                    egui::CursorIcon::Grabbing
                } else {
                    egui::CursorIcon::PointingHand
                });
            }
        });
    if reset {
        app.actions.push(Action::MoveViewsDisc(None));
    } else if let Some(place) = drop_at {
        app.actions.push(Action::MoveViewsDisc(Some([place.x, place.y])));
    }
}

/// Where the pop-out controls first appear: bottom centre, above the bar.
pub(super) fn default_float(ctx: &Context) -> [f32; 3] {
    let screen = ctx.content_rect();
    let width = 380.0;
    [
        (screen.center().x - width / 2.0).max(screen.left()),
        (screen.bottom() - theme::PLAYER_BAR_HEIGHT - 110.0).max(screen.top()),
        width,
    ]
}

/// The controls (buttons and song bar) as a panel of their own that can be
/// dragged anywhere by its top strip, widened from its right edge, snapped
/// onto the library sidebar, or put away from the right-click menu.
pub fn floating_controls(app: &mut App, ctx: &Context) {
    let Some([x, y, width]) = app.float_slot(default_float(ctx)) else {
        return;
    };
    if app.fullscreen_vis || app.calm_mode || app.mini_active {
        return;
    }
    let screen = ctx.content_rect();
    let width = super::lyrics::controls_width(app, width);
    let mut change: Option<Option<[f32; 3]>> = None;
    let mut top_left = egui::pos2(x, y);
    egui::Area::new(Id::new("pop-out-controls"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(0.0, 0.0))
        .interactable(true)
        .show(ctx, |ui| {
            // The whole panel is a handle: this is registered first, so the
            // buttons and bars drawn over it still take their own clicks.
            let body = Rect::from_min_size(top_left, egui::vec2(width, super::lyrics::controls_height(app)));
            let grip_response = ui.interact(body, Id::new("pop-out-grip"), egui::Sense::click_and_drag());
            let Some(outer) = super::lyrics::controls_box_inner(app, ui, top_left, width, false) else {
                return;
            };
            // The small album art mode: the cover and the song's name sit
            // just above the controls and travel with them.
            if !app.settings.art_expanded
                && let Some(now) = app.now_playing()
            {
                let side = (outer.width() * 0.26).clamp(64.0, 120.0);
                let gap = 8.0;
                let top = (outer.top() - side - gap).max(screen.top() + 4.0);
                let cover = Rect::from_min_size(egui::pos2(outer.left(), top), egui::vec2(side, side));
                let palette = dark_palette(app);
                let loader = app.backend.art().clone();
                super::widgets::paint_cover(
                    ui,
                    &palette,
                    now.art_url.as_deref().or(now.art_small.as_deref()),
                    cover,
                    8.0,
                    Icon::Music,
                    Some(&loader),
                );
                let text_left = cover.right() + 12.0;
                let text_w = (outer.right() - text_left).max(40.0);
                let title = crate::bidi::layout(
                    ui.painter(),
                    &now.title,
                    theme::bold((side * 0.2).clamp(14.0, 22.0)),
                    Color32::WHITE,
                    text_w,
                    2,
                    Some(crate::bidi::ELLIPSIS),
                );
                let artist = crate::bidi::layout(
                    ui.painter(),
                    &now.subtitle,
                    theme::regular((side * 0.15).clamp(12.0, 16.0)),
                    Color32::from_white_alpha(190),
                    text_w,
                    1,
                    Some(crate::bidi::ELLIPSIS),
                );
                let block = title.size().y + 4.0 + artist.size().y;
                let y = cover.center().y - block / 2.0;
                let title_h = title.size().y;
                ui.painter().galley(egui::pos2(text_left, y), title, Color32::WHITE);
                ui.painter().galley(egui::pos2(text_left, y + title_h + 4.0), artist, Color32::WHITE);
            }
            let grip = Rect::from_min_size(outer.min, egui::vec2(outer.width(), 12.0));
            let dots = Rect::from_center_size(egui::pos2(grip.center().x, grip.center().y + 1.0), egui::vec2(22.0, 4.0));
            for i in 0..3 {
                ui.painter().circle_filled(
                    egui::pos2(dots.left() + 3.0 + i as f32 * 8.0, dots.center().y),
                    1.6,
                    Color32::from_white_alpha(if grip_response.hovered() { 150 } else { 70 }),
                );
            }
            if grip_response.hovered() || grip_response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            if grip_response.dragged() {
                let to = top_left + grip_response.drag_delta();
                top_left = egui::pos2(
                    to.x.clamp(screen.left(), (screen.right() - width).max(screen.left())),
                    to.y.clamp(screen.top(), (screen.bottom() - outer.height()).max(screen.top())),
                );
                change = Some(Some([top_left.x, top_left.y, width]));
            }
            // Resizable from any edge or corner. The sides widen it (the left
            // one moves it so the far edge stays put); the bottom makes it
            // taller. Taller or wider, everything inside scales together.
            let edge = 10.0;
            let corner = 16.0;
            let right_edge = Rect::from_min_max(
                egui::pos2(outer.right() - edge, outer.top() + 14.0),
                egui::pos2(outer.right() + 6.0, outer.bottom() - corner),
            );
            let left_edge = Rect::from_min_max(
                egui::pos2(outer.left() - 6.0, outer.top() + 14.0),
                egui::pos2(outer.left() + edge, outer.bottom() - corner),
            );
            let bottom_edge = Rect::from_min_max(
                egui::pos2(outer.left() + corner, outer.bottom() - edge),
                egui::pos2(outer.right() - corner, outer.bottom() + 6.0),
            );
            let corner_right = Rect::from_min_max(
                egui::pos2(outer.right() - corner, outer.bottom() - corner),
                egui::pos2(outer.right() + 6.0, outer.bottom() + 6.0),
            );
            let corner_left = Rect::from_min_max(
                egui::pos2(outer.left() - 6.0, outer.bottom() - corner),
                egui::pos2(outer.left() + corner, outer.bottom() + 6.0),
            );
            let right_response = ui.interact(right_edge, Id::new("pop-out-edge"), egui::Sense::drag());
            let left_response = ui.interact(left_edge, Id::new("pop-out-edge-left"), egui::Sense::drag());
            let bottom_response = ui.interact(bottom_edge, Id::new("pop-out-edge-bottom"), egui::Sense::drag());
            let corner_right_response = ui.interact(corner_right, Id::new("pop-out-corner-right"), egui::Sense::drag());
            let corner_left_response = ui.interact(corner_left, Id::new("pop-out-corner-left"), egui::Sense::drag());
            for (response, cursor) in [
                (&right_response, egui::CursorIcon::ResizeHorizontal),
                (&left_response, egui::CursorIcon::ResizeHorizontal),
                (&bottom_response, egui::CursorIcon::ResizeVertical),
                (&corner_right_response, egui::CursorIcon::ResizeNwSe),
                (&corner_left_response, egui::CursorIcon::ResizeNeSw),
            ] {
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(cursor);
                }
            }
            // Never so wide that the song bar runs the whole screen.
            let max_w = (screen.width() - 16.0).clamp(300.0, 640.0);
            let grow_right = right_response.drag_delta().x + corner_right_response.drag_delta().x;
            let grow_left = -(left_response.drag_delta().x + corner_left_response.drag_delta().x);
            let grow_down = bottom_response.drag_delta().y
                + corner_right_response.drag_delta().y
                + corner_left_response.drag_delta().y;
            if grow_right != 0.0 || grow_left != 0.0 {
                let wider = (width + grow_right + grow_left).clamp(300.0, max_w);
                let x = (top_left.x + width - wider).max(screen.left());
                let x = if grow_left != 0.0 { x } else { top_left.x.min((screen.right() - wider).max(screen.left())) };
                change = Some(Some([x, top_left.y, wider]));
            }
            if grow_down != 0.0 {
                // Height is the controls' size: dragging down makes them bigger.
                let scale = (app.settings.controls_scale_value() + grow_down / 56.0).clamp(0.7, 1.8);
                app.actions.push(Action::SetControlsScale(scale));
            }
            grip_response.context_menu(|ui| {
                if ui.button("Snap to the library sidebar").clicked() {
                    let side = app.settings.sidebar_width.clamp(300.0, 460.0);
                    change = Some(Some([
                        8.0,
                        (screen.bottom() - theme::PLAYER_BAR_HEIGHT - outer.height() - 8.0).max(screen.top()),
                        side,
                    ]));
                    ui.close();
                }
                if ui.button("Put back where it started").clicked() {
                    change = Some(Some(default_float(ui.ctx())));
                    ui.close();
                }
            });
        });
    if let Some(place) = change {
        app.actions.push(Action::SetFloatControls(place));
    }
}

pub fn show(app: &mut App, ctx: &Context) {
    let has_topbar = !(app.fullscreen_vis || app.lyrics_fullscreen.is_some() || app.mini_active || app.calm_mode);
    if !has_topbar {
        corner_disc(app, ctx);
    }
    floating_controls(app, ctx);
    if !app.views_panel {
        return;
    }
    let palette = dark_palette(app);
    let frame = Frame::new()
        .fill(Color32::from_rgb(0x14, 0x16, 0x1a))
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(160),
        });
    let close = false;
    let current = current_view(app);
    // The panel fades to almost nothing when the pointer is away from it,
    // and comes straight back when it returns.
    let rect_id = Id::new("views-panel-rect");
    let last_rect: Option<Rect> = ctx.data(|data| data.get_temp(rect_id));
    let over = last_rect.is_some_and(|rect| {
        ctx.input(|input| input.pointer.hover_pos().is_some_and(|p| rect.expand(10.0).contains(p)) || input.pointer.any_down() && input.pointer.interact_pos().is_some_and(|p| rect.expand(30.0).contains(p)))
    });
    let dim = panel_dim(ctx, over);
    let frame = frame.multiply_with_opacity(dim);
    let width = app.settings.views_width.clamp(260.0, 520.0);
    let shown = egui::Window::new("Views")
        .id(Id::new("views-panel-v3"))
        .title_bar(false)
        .order(egui::Order::Foreground)
        .frame(frame)
        .default_pos(egui::pos2(60.0, 60.0))
        .min_width(width)
        .max_width(width)
        .resizable(false)
        .collapsible(false)
        .constrain(true)
        .show(ctx, |ui| {
            ui.set_opacity(dim);
            ui.visuals_mut().override_text_color = Some(palette.text);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            let screen_h = ctx.content_rect().height();
            egui::ScrollArea::vertical()
                .max_height((screen_h - 120.0).max(200.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    // Icons only, one row: the four views of this window (exactly
                    // one is on) and the mini player, which is a window of its own.
                    let views = [
                        (Icon::Shrink, "Mini player", ViewKind::Mini),
                        (Icon::PanelLeft, "Normal", ViewKind::Visualizer),
                        (Icon::AudioLines, "Full screen visualizer", ViewKind::FullVisualizer),
                        (Icon::Mic, "Full screen lyrics", ViewKind::FullLyrics),
                        (Icon::Moon, "Calm mode", ViewKind::Calm),
                    ];
                    let each = ((ui.available_width() - 6.0 * 4.0) / 5.0).max(30.0);
                    ui.horizontal(|ui| {
                        for (icon, label, kind) in views {
                            let on = if kind == ViewKind::Mini {
                                app.settings.mini_player
                            } else {
                                current == kind || (kind == ViewKind::Visualizer && current == ViewKind::Default)
                            };
                            if view_button(ui, &palette, icon, label, on, each).clicked() {
                                app.actions.push(Action::GoView(kind));
                            }
                        }
                    });
                    ui.add_space(4.0);
                    let on_top = ("Always on top", app.settings.mini_on_top, Action::ToggleMiniOnTop);
                    let flag = |bit: u8, label: &'static str, inverted: bool, app: &App| {
                        (label, app.settings.lyrics_flag(bit) != inverted, Action::ToggleLyricsFlag(bit))
                    };
                    use crate::settings::Settings as S;
                    // Only what belongs to the view you are in.
                    let chips: Vec<(&str, bool, Action)> = match current {
                        ViewKind::Calm => Vec::new(),
                        ViewKind::FullVisualizer => vec![
                            ("Lyrics on it", app.settings.vis_lyrics, Action::ToggleVisLyrics),
                            ("Artist name", !app.settings.vis_text_no_artist, Action::ToggleVisArtist),
                            ("Visualizer settings", app.vis_panel, Action::ToggleVisPanel),
                            (if app.extra_vis { "Close visualizer window" } else { "New visualizer window" }, app.extra_vis, Action::ToggleExtraWindow),
                            on_top,
                        ],
                        ViewKind::FullLyrics => vec![
                            flag(S::LYRICS_HIDE_ART, "Album art", true, app),
                            flag(S::LYRICS_FLOAT_ART, "Floating art", false, app),
                            flag(S::LYRICS_BOUNCE_ART, "Art bounces", false, app),
                            flag(S::LYRICS_HIDE_STAMPS, "Timestamps", true, app),
                            flag(S::LYRICS_COUNTDOWN, "Countdown", false, app),
                            flag(S::LYRICS_HIDE_ARTIST, "Artist name", true, app),
                            ("Visualizer behind", app.settings.lyrics_vis, Action::ToggleLyricsVis),
                            ("Visualizer settings", app.vis_panel, Action::ToggleVisPanel),
                            on_top,
                        ],
                        _ => vec![
                            ("Library only", false, Action::GoView(ViewKind::LibraryOnly)),
                            ("Default view", false, Action::GoView(ViewKind::Default)),
                            ("My view", false, Action::InDefaultView(Box::new(Action::ApplyMyView))),
                            ("Save my view", false, Action::SaveMyView),
                            ("Library", app.settings.sidebar_visible, Action::InDefaultView(Box::new(Action::ToggleSidebar))),
                            ("Queue", app.show_queue_panel, Action::InDefaultView(Box::new(Action::ToggleQueuePanel))),
                            ("Side lyrics", app.show_lyrics_panel, Action::InDefaultView(Box::new(Action::ToggleLyricsPanel))),
                            ("Big album art", app.settings.art_expanded, Action::InDefaultView(Box::new(Action::ToggleArtExpanded))),
                            ("Clean screen", app.settings.float_hide_bar, Action::SetFloatHideBar(!app.settings.float_hide_bar)),
                            ("Visualizer settings", app.vis_panel, Action::ToggleVisPanel),
                            (if app.extra_vis { "Close visualizer window" } else { "New visualizer window" }, app.extra_vis, Action::ToggleExtraWindow),
                            on_top,
                        ],
                    };
                    if !chips.is_empty() {
                        ui.horizontal_wrapped(|ui| {
                            for (label, on, action) in chips {
                                let response = theme::soft_button(ui, &palette, None, label, on);
                                let response = if label == "Clean screen" {
                                    response.on_hover_text("With the controls popped out, hides the whole bottom bar too")
                                } else {
                                    response
                                };
                                if response.clicked() {
                                    app.actions.push(action);
                                }
                            }
                        });
                    }
                    ui.add_space(2.0);
                    egui::CollapsingHeader::new(
                        egui::RichText::new("More").color(palette.secondary).font(theme::medium(13.0)),
                    )
                    .id_salt("views-more")
                    .default_open(false)
                    .show(ui, |ui| {
                        zoom_row(ui, app, &palette);
                        ui.spacing_mut().slider_width = 110.0;
                        let mut dim = app.settings.disc_dim;
                        if ui.add(egui::Slider::new(&mut dim, 0.05..=1.0).text("Disc when idle")).changed() {
                            app.settings.disc_dim = dim;
                            app.mark_settings_dirty();
                        }
                        ui.horizontal_wrapped(|ui| {
                            if theme::soft_button(ui, &palette, None, "Default controls", false).clicked() {
                                app.actions.push(Action::ResetBlockNudge);
                            }
                            if theme::soft_button(ui, &palette, Some(Icon::Info), "Bug test guide", false).clicked() {
                                app.show_bug_guide = !app.show_bug_guide;
                            }
                            if theme::soft_button(ui, &palette, Some(Icon::Info), "Shortcuts", false).clicked() {
                                app.actions.push(Action::ShowDialog(crate::model::Dialog::Shortcuts));
                            }
                        });
                        // Every key's job as a button, for those who use the mouse only.
                        egui::CollapsingHeader::new(
                            egui::RichText::new("Everything the keys do").color(palette.text).font(theme::medium(13.0)),
                        )
                        .id_salt("views-all-actions")
                        .default_open(false)
                        .show(ui, |ui| {
                            for (group, ids) in super::keys::CATEGORIES {
                                theme::subtle(ui, &palette, &group.to_uppercase());
                                ui.horizontal_wrapped(|ui| {
                                    for id in *ids {
                                        if let Some(bindable) = super::keys::BINDABLE.iter().find(|b| b.id == *id) {
                                            let hint = super::keys::chord_label(app, bindable.id);
                                            let mut button = theme::soft_button(ui, &palette, None, bindable.label, false);
                                            if let Some(hint) = hint {
                                                button = button.on_hover_text(format!("Key: {hint}"));
                                            }
                                            if button.clicked() {
                                                app.actions.push((bindable.action)());
                                            }
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }
                        });
                    });
                });
        });
    if let Some(window) = shown {
        let rect = window.response.rect;
        ctx.data_mut(|data| data.insert_temp(rect_id, rect));
        // A strip on the right edge: drag it to make the panel wider or narrower.
        egui::Area::new(Id::new("views-panel-edge"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(rect.right() - 7.0, rect.top() + 10.0))
            .show(ctx, |ui| {
                let (strip, response) = ui.allocate_exact_size(egui::vec2(10.0, (rect.height() - 20.0).max(10.0)), egui::Sense::drag());
                if response.hovered() || response.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    ui.painter().rect_filled(
                        egui::Rect::from_center_size(strip.center(), egui::vec2(3.0, 28.0)),
                        1.5,
                        Color32::from_white_alpha(90),
                    );
                }
                if response.dragged() {
                    app.settings.views_width = (width + response.drag_delta().x).clamp(260.0, 520.0);
                    app.mark_settings_dirty();
                }
            });
    }
    if close {
        app.actions.push(Action::ToggleViewsPanel);
    }
    bug_guide(app, ctx);
}

/// How visible the Views panel is: full while the pointer is near it, and
/// almost gone (5%) a couple of seconds after it leaves.
fn panel_dim(ctx: &Context, over: bool) -> f32 {
    let seen_id = Id::new("views-panel-seen");
    let now_t = ctx.input(|input| input.time);
    if over {
        ctx.data_mut(|data| data.insert_temp(seen_id, now_t));
    }
    let last_seen: f64 = ctx.data(|data| data.get_temp(seen_id)).unwrap_or(now_t);
    let away = now_t - last_seen > 2.0;
    if !away {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    ctx.animate_value_with_time(seen_id.with("dim"), if away { 0.05 } else { 1.0 }, 0.4)
}

/// A round-ended pill holding just an icon (the name is its tooltip), lit
/// when it is on.
fn view_button(ui: &mut egui::Ui, palette: &Palette, icon: Icon, label: &str, on: bool, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 38.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if on {
            palette.text
        } else if response.hovered() {
            palette.surface_hover
        } else {
            palette.surface
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        let color = if on { palette.window } else { palette.text };
        theme::paint_icon(ui, icon, rect, 18.0, color);
    }
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label));
    response.on_hover_text(label)
}

/// Which of the exclusive views is on now: the mini player, a full screen,
/// calm mode, the ordinary window with the visualizer, or the plain window.
fn current_view(app: &App) -> ViewKind {
    if app.calm_mode {
        ViewKind::Calm
    } else if app.fullscreen_vis {
        ViewKind::FullVisualizer
    } else if app.lyrics_fullscreen.is_some() {
        ViewKind::FullLyrics
    } else if app.settings.vis_shapes_value() != 0 {
        ViewKind::Visualizer
    } else {
        ViewKind::Default
    }
}

/// The bug test checklist, in a window of its own.
fn bug_guide(app: &mut App, ctx: &Context) {
    if !app.show_bug_guide {
        return;
    }
    let mut open = true;
    egui::Window::new("How to bug test chanceify")
        .id(Id::new("bug-guide"))
        .open(&mut open)
        .default_width(480.0)
        .default_height(420.0)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for line in include_str!("../../docs/BUG_TEST_GUIDE.md").lines() {
                    let line = line.trim_start_matches('#').trim();
                    if line.is_empty() {
                        ui.add_space(4.0);
                    } else {
                        ui.label(line);
                    }
                }
            });
        });
    if !open {
        app.show_bug_guide = false;
    }
}
