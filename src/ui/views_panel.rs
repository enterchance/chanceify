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

/// The Views disc: a small disc the mouse can drag anywhere and click to open
/// the Views panel, on every screen. Until it has been moved, it stays out of
/// the way where the sidebar has its own disc.
/// How much of the disc shows: all of it while the pointer is near (or the
/// panel is open), `floor` of it after a few seconds away.
fn idle_dim(ctx: &Context, panel_open: bool, floor: f32, over: bool) -> f32 {
    let seen_id = Id::new("views-disc-seen");
    let now_t = ctx.input(|input| input.time);
    // Back after being away (a view without it, or the panel just closed):
    // it shows in full again and fades only after a fresh few seconds.
    let pass = ctx.cumulative_pass_nr();
    let drawn_id = seen_id.with("drawn");
    let open_id = seen_id.with("open");
    let last_drawn: u64 = ctx.data(|data| data.get_temp(drawn_id)).unwrap_or(pass);
    let was_open: bool = ctx.data(|data| data.get_temp(open_id)).unwrap_or(panel_open);
    let returned = pass > last_drawn + 3 || (was_open && !panel_open);
    ctx.data_mut(|data| {
        data.insert_temp(drawn_id, pass);
        data.insert_temp(open_id, panel_open);
    });
    if returned {
        ctx.data_mut(|data| data.insert_temp(seen_id, now_t));
        ctx.animate_value_with_time(seen_id.with("dim"), 1.0, 0.0);
    }
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
    theme::paint_icon(ui, Icon::Disc, rect, 20.0, if over { palette.text } else { Color32::from_white_alpha(150) });
    ui.set_opacity(1.0);
    if over {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // A left click opens it where it was left; a right click opens it right
    // under the disc.
    let anchor = rect.left_bottom() + egui::vec2(0.0, 6.0);
    ui.ctx().data_mut(|data| data.insert_temp(Id::new("views-anchor"), anchor));
    if response.clicked() || response.secondary_clicked() {
        if response.secondary_clicked() {
            ui.ctx().data_mut(|data| data.insert_temp(Id::new("views-at-disc"), true));
        }
        app.actions.push(Action::ToggleViewsPanel);
    }
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
    ctx.data_mut(|data| data.insert_temp(Id::new("views-anchor"), hit.left_bottom() + egui::vec2(0.0, 6.0)));
    if right_released && pos.is_some_and(|p| hit.contains(p)) {
        toggle = true;
        ctx.data_mut(|data| data.insert_temp(Id::new("views-at-disc"), true));
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
            theme::paint_icon(
                ui,
                Icon::Disc,
                rect,
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
    // Read once, out here: asking the context for it from inside one of its
    // own data closures would lock it against itself and freeze the app.
    let vp = ctx.viewport_id();
    let Some([x, y, width]) = app.float_slot(default_float(ctx)) else {
        return;
    };
    if app.settings.float_detached && !app.calm_mode && !app.mini_active {
        // Where the app's own window is, for the controls to snap to.
        if let Some(main) = ctx.input(|input| input.viewport().outer_rect) {
            ctx.data_mut(|data| data.insert_temp(Id::new("main-outer"), main));
        }
        detached_controls(app, ctx);
        return;
    }
    if app.calm_mode || app.mini_active {
        return;
    }
    // In the full screen visualizer there is no bar, so the panel is here
    // too, fading away when the pointer is not near, like the views panel.
    let dim = if app.fullscreen_vis {
        let mut last: Option<Rect> = ctx.data(|data| data.get_temp(Id::new(("pop-out-last", vp))));
        if let (Some(rect), Some(card)) = (last, ctx.data(|data| data.get_temp::<Rect>(Id::new(("pop-out-card-last", vp))))) {
            last = Some(rect.union(card));
        }
        let near = last.is_some_and(|rect| {
            ctx.input(|input| {
                input.pointer.hover_pos().is_some_and(|p| rect.expand(30.0).contains(p))
                    || (input.pointer.any_down() && input.pointer.interact_pos().is_some_and(|p| rect.expand(40.0).contains(p)))
            })
        }) || last.is_none();
        // Resting, the buttons stay easy to see; the pointer on any part of
        // the card or the controls brings it all to full.
        fade_to(ctx, near, &format!("pop-out-seen-{vp:?}"), app.settings.float_idle.unwrap_or(0.5).clamp(0.0, 1.0))
    } else {
        1.0
    };
    let screen = ctx.content_rect();
    let width = super::lyrics::controls_width(app, width);
    let mut change: Option<Option<[f32; 3]>> = None;
    // Never off screen: if a view or window change would leave it outside,
    // it sits against the bottom or right edge instead.
    let mut top_left = egui::pos2(
        x.clamp(screen.left(), (screen.right() - width).max(screen.left())),
        y.clamp(screen.top(), (screen.bottom() - super::lyrics::controls_height(app)).max(screen.top())),
    );
    // Until it has been moved, the lyrics page's controls hang under the
    // cover's resting place, so a bouncing or tilting cover never reaches them.
    if app.lyrics_fullscreen.is_some()
        && app.settings.float_lyrics.is_none()
        && let Some(base) = ctx.data(|data| data.get_temp::<Rect>(Id::new("lyrics-base-cover")))
    {
        top_left = egui::pos2(base.left(), (base.bottom() + 96.0).min(screen.bottom() - 120.0));
    }
    // The area itself senses nothing (it starts at the window's corner, so
    // anything it sensed would sit on top of the title bar and stop it from
    // dragging the window); every control on it senses for itself.
    egui::Area::new(Id::new("pop-out-controls"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(0.0, 0.0))
        .interactable(true)
        .sense(egui::Sense::hover())
        .show(ctx, |ui| {
            ui.set_opacity(dim);
            // The whole panel is a handle: this is registered first, so the
            // buttons and bars drawn over it still take their own clicks.
            let body = Rect::from_min_size(top_left, egui::vec2(width, super::lyrics::controls_height(app)));
            let grip_response = ui.interact(body, Id::new("pop-out-grip"), egui::Sense::click_and_drag());
            let Some(outer) = super::lyrics::controls_box_inner(app, ui, top_left, width, false) else {
                return;
            };
            ui.ctx().data_mut(|data| data.insert_temp(Id::new(("pop-out-last", vp)), outer));
            // The now playing card: the cover and the song's name sit just
            // above the controls and travel with them. Under the full screen
            // lyrics the big cover is already on the page, so the card is
            // only the words. Any part of it drags the whole panel.
            let mut card_drag = egui::Vec2::ZERO;
            let mut card_dragging = false;
            let lyrics_page = app.lyrics_fullscreen.is_some();
            if (lyrics_page || app.fullscreen_vis || !app.settings.art_expanded)
                && let Some(now) = app.now_playing()
            {
                let side = if lyrics_page { 44.0 } else { (outer.width() * 0.26).clamp(64.0, 120.0) };
                let pad = 8.0;
                // Above the controls; where that would be cut off by the top of
                // the window, below them instead.
                let above_top = outer.top() - side - pad * 2.0 - 8.0;
                let top = if above_top >= screen.top() + 4.0 + pad {
                    above_top
                } else {
                    outer.bottom() + 8.0 + pad
                };
                let cover = Rect::from_min_size(egui::pos2(outer.left() + pad, top), egui::vec2(side, side));
                let palette = app.palette;
                let text_left = if lyrics_page { outer.left() + 14.0 } else { cover.right() + 12.0 };
                let text_w = (outer.right() - pad - text_left - 28.0).max(40.0);
                let title = crate::bidi::layout(
                    ui.painter(),
                    &now.title,
                    theme::bold((side * 0.2).clamp(14.0, 22.0)),
                    palette.text,
                    text_w,
                    2,
                    Some(crate::bidi::ELLIPSIS),
                );
                let artist = crate::bidi::layout(
                    ui.painter(),
                    &now.subtitle,
                    theme::regular((side * 0.15).clamp(12.0, 16.0)),
                    palette.secondary,
                    text_w,
                    1,
                    Some(crate::bidi::ELLIPSIS),
                );
                // The card is only as wide as its words need (the cover, the name,
                // the heart), never wider than the controls under it; a name
                // too long for that goes onto two lines and then ends in "…".
                let need = text_left + (title.size().x + 34.0).max(artist.size().x) + pad + 4.0;
                let card_right = need.clamp(outer.left() + 160.0_f32.min(outer.width()), outer.right());
                let card = Rect::from_min_max(cover.min - egui::vec2(pad, pad), egui::pos2(card_right, cover.bottom() + pad));
                ui.ctx().data_mut(|data| data.insert_temp(Id::new(("pop-out-card-last", vp)), card));
                let song = app.now_playing_item();
                // The whole card is a handle, registered first so the cover,
                // the name and the heart still take their own clicks.
                let handle = ui.interact(card, Id::new("pop-out-card-grip"), egui::Sense::click_and_drag());
                if handle.hovered() || handle.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                card_drag = handle.drag_delta();
                card_dragging = handle.dragged();
                // A backing, so the words read over any page.
                ui.painter().rect(
                    card,
                    12.0,
                    palette.panel.gamma_multiply(0.94),
                    Stroke::new(1.0, palette.outline),
                    egui::StrokeKind::Inside,
                );
                let open_song = |app: &mut App| {
                    if let Some(id) = &now.album_id {
                        app.actions.push(Action::Open(crate::model::Page::Album(id.clone())));
                    } else if let Some(id) = &now.show_id {
                        app.actions.push(Action::Open(crate::model::Page::Show(id.clone())));
                    }
                };
                if !lyrics_page {
                    let loader = app.backend.art().clone();
                    super::widgets::paint_sticky_cover(
                        ui,
                        &palette,
                        "pop-out-card",
                        now.art_url.as_deref().or(now.art_small.as_deref()),
                        cover,
                        8.0,
                        Icon::Music,
                        &loader,
                    );
                    let cover_click = ui
                        .interact(cover, Id::new("pop-out-card-cover"), egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if cover_click.clicked() {
                        open_song(app);
                    }
                }
                let block = title.size().y + 4.0 + artist.size().y;
                let y = cover.center().y - block / 2.0;
                let title_h = title.size().y;
                let title_w = title.size().x;
                let first_row = title_h / title.rows.len().max(1) as f32;
                let words = Rect::from_min_size(egui::pos2(text_left, y), egui::vec2(text_w, block));
                let cut = title.rows.len() > 1 || title_w >= text_w - 2.0 || artist.size().x >= text_w - 2.0;
                let mut words_click = ui
                    .interact(words, Id::new("pop-out-card-words"), egui::Sense::click())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if cut {
                    // Too long to show whole: the full name is on hover.
                    words_click = words_click.on_hover_text(format!("{}\n{}", now.title, now.subtitle));
                }
                if words_click.clicked() {
                    open_song(app);
                }
                if let Some(song) = &song {
                    words_click.context_menu(|ui| super::widgets::item_menu(ui, app, song, None, None));
                }
                ui.painter().galley(egui::pos2(text_left, y), title, palette.text);
                // The like heart, right after the song's name.
                super::player_bar::like_heart(
                    app,
                    ui,
                    "pop-out-card",
                    egui::pos2(text_left + title_w.min(text_w) + 16.0, y + first_row / 2.0),
                    18.0,
                    &now.uri,
                );
                ui.painter().galley(egui::pos2(text_left, y + title_h + 4.0), artist, palette.secondary);
            }
            if grip_response.hovered() || grip_response.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
            }
            // The panel follows the pointer exactly: where it was grabbed is
            // remembered, and every frame it is put at pointer minus that, so
            // it moves at the pointer's own speed whichever part is held (the
            // handle, the card or a bar), however the frames are paced.
            let bar_dragging = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(Id::new(("popout-bar-dragging", vp))))
                .unwrap_or(false);
            let grab_key = Id::new(("pop-out-grab", vp));
            let pointer = ui.ctx().input(|input| input.pointer.latest_pos());
            if (grip_response.dragged() || card_dragging || bar_dragging)
                && let Some(p) = pointer
            {
                let grab = ui
                    .ctx()
                    .data(|data| data.get_temp::<egui::Vec2>(grab_key))
                    .unwrap_or(p - top_left);
                ui.ctx().data_mut(|data| data.insert_temp(grab_key, grab));
                let to = p - grab;
                top_left = egui::pos2(
                    to.x.clamp(screen.left(), (screen.right() - width).max(screen.left())),
                    to.y.clamp(screen.top(), (screen.bottom() - outer.height()).max(screen.top())),
                );
                change = Some(Some([top_left.x, top_left.y, width]));
            } else {
                ui.ctx().data_mut(|data| data.remove::<egui::Vec2>(grab_key));
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
            // The same little dot as the Views panel, so it shows that the
            // corner resizes.
            if corner_right_response.hovered() || corner_right_response.dragged() {
                ui.painter().circle_filled(egui::pos2(outer.right() - 7.0, outer.bottom() - 7.0), 4.0, Color32::from_white_alpha(110));
            }
            // Wider and taller are one thing: the panel only scales, between
            // the smallest and the biggest it can be, so it is never long
            // and thin.
            let grow_right = right_response.drag_delta().x + corner_right_response.drag_delta().x;
            let grow_left = -(left_response.drag_delta().x + corner_left_response.drag_delta().x);
            let grow_down = bottom_response.drag_delta().y
                + corner_right_response.drag_delta().y
                + corner_left_response.drag_delta().y;
            let steps = (grow_right + grow_left) / 232.0 + grow_down / 56.0;
            if steps != 0.0 {
                let before = app.settings.controls_scale_value();
                let scale = (before + steps).clamp(0.7, 1.8);
                app.actions.push(Action::SetControlsScale(scale));
                if grow_left != 0.0 {
                    // The far edge stays where it is.
                    let wider = super::lyrics::controls_width_for(scale);
                    let x = (top_left.x + width - wider).max(screen.left());
                    change = Some(Some([x, top_left.y, wider]));
                }
            }
            grip_response.context_menu(|ui| {
                if app.fullscreen_vis {
                    // How much of the controls stays showing once the mouse
                    // has left them.
                    let mut idle = app.settings.float_idle.unwrap_or(0.5).clamp(0.0, 1.0) * 100.0;
                    ui.label("opacity when the mouse is away");
                    if ui.add(egui::Slider::new(&mut idle, 0.0..=100.0).suffix("%")).changed() {
                        app.settings.float_idle = Some(idle / 100.0);
                        app.mark_settings_dirty();
                    }
                    let mut keep = app.settings.float_keep_play;
                    if ui.checkbox(&mut keep, "keep the play button showing").changed() {
                        app.settings.float_keep_play = keep;
                        app.mark_settings_dirty();
                    }
                    ui.separator();
                }
                if ui.button("Move out of the window").clicked() {
                    app.settings.float_detached = true;
                    app.mark_settings_dirty();
                    ui.close();
                }
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
        // The extra visualizer window keeps its own place for the panel.
        if app.extra_drawing {
            app.actions.push(Action::SetFloatExtra(place));
        } else {
            app.actions.push(Action::SetFloatControls(place));
        }
    }
}

/// The controls in a window of their own: it can be dragged anywhere on any
/// screen, off the app. It keeps its place between launches, stays on top,
/// and scales as the panel does.
fn detached_controls(app: &mut App, ctx: &Context) {
    use egui::{ViewportBuilder, ViewportClass, ViewportCommand, ViewportId};
    let id = ViewportId::from_hash_of("chanceify-pop-out-controls");
    let width = super::lyrics::controls_width(app, 0.0);
    // The song's card (little cover and name) goes out of the window with
    // the controls, above them, unless the big album art is on (that one
    // stays in the window).
    let with_card = !app.settings.art_expanded && app.now_playing().is_some();
    let card_side = (width * 0.26).clamp(64.0, 120.0);
    let card_h = if with_card { card_side + 16.0 + 8.0 } else { 0.0 };
    let controls_h = super::lyrics::controls_height(app);
    let height = controls_h + card_h;
    let saved = app.settings.float_window;
    let mut builder = ViewportBuilder::default()
        .with_title("chanceify controls")
        .with_decorations(false)
        .with_resizable(false)
        .with_always_on_top()
        .with_inner_size([width, height]);
    if let Some(at) = saved
        && at[0] > -20000.0
        && at[1] > -20000.0
    {
        builder = builder.with_position(at);
    }
    let mut put_back = false;
    let mut place: Option<[f32; 2]> = None;
    ctx.show_viewport_immediate(id, builder, |ui, class| {
        let ctx = ui.ctx().clone();
        if class != ViewportClass::Immediate && class != ViewportClass::Root {
            put_back = true;
            return;
        }
        if ctx.input(|input| input.viewport().close_requested()) {
            put_back = true;
            return;
        }
        // The window is exactly the panel, whatever size it has been scaled to.
        let size = ctx.content_rect().size();
        // Asked for once per size, not every frame: a window that cannot be
        // exactly that size (rounding on a scaled screen) would otherwise be
        // told again each frame and shake.
        let size_key = Id::new("detached-asked-size");
        let asked: Option<egui::Vec2> = ctx.data(|data| data.get_temp(size_key));
        if ((size.x - width).abs() > 1.0 || (size.y - height).abs() > 1.0)
            && asked.is_none_or(|asked| (asked.x - width).abs() > 0.5 || (asked.y - height).abs() > 0.5)
        {
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(width, height)));
            ctx.data_mut(|data| data.insert_temp(size_key, egui::vec2(width, height)));
        }
        egui::CentralPanel::default()
            .frame(Frame::new().fill(app.palette.window))
            .show(ui, |ui| {
                let body = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(width, height));
                let grip = ui.interact(body, Id::new("detached-grip"), egui::Sense::click_and_drag());
                if grip.drag_started() {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                if with_card && let Some(now) = app.now_playing() {
                    let palette = app.palette;
                    let pad = 8.0;
                    let cover = Rect::from_min_size(egui::pos2(pad, pad), egui::vec2(card_side, card_side));
                    let card = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(width, card_side + pad * 2.0));
                    ui.painter().rect(
                        card,
                        12.0,
                        palette.panel.gamma_multiply(0.94),
                        Stroke::new(1.0, palette.outline),
                        egui::StrokeKind::Inside,
                    );
                    let loader = app.backend.art().clone();
                    super::widgets::paint_sticky_cover(
                        ui,
                        &palette,
                        "pop-out-card",
                        now.art_url.as_deref().or(now.art_small.as_deref()),
                        cover,
                        8.0,
                        Icon::Music,
                        &loader,
                    );
                    let text_left = cover.right() + 12.0;
                    let text_w = (width - pad - text_left - 28.0).max(40.0);
                    let title = crate::bidi::layout(
                        ui.painter(),
                        &now.title,
                        theme::bold((card_side * 0.2).clamp(14.0, 22.0)),
                        palette.text,
                        text_w,
                        2,
                        Some(crate::bidi::ELLIPSIS),
                    );
                    let artist = crate::bidi::layout(
                        ui.painter(),
                        &now.subtitle,
                        theme::regular((card_side * 0.15).clamp(12.0, 16.0)),
                        palette.secondary,
                        text_w,
                        1,
                        Some(crate::bidi::ELLIPSIS),
                    );
                    let block = title.size().y + 4.0 + artist.size().y;
                    let y = cover.center().y - block / 2.0;
                    let title_h = title.size().y;
                    let title_w = title.size().x;
                    let first_row = title_h / title.rows.len().max(1) as f32;
                    ui.painter().galley(egui::pos2(text_left, y), title, palette.text);
                    super::player_bar::like_heart(
                        app,
                        ui,
                        "pop-out-card",
                        egui::pos2(text_left + title_w.min(text_w) + 16.0, y + first_row / 2.0),
                        18.0,
                        &now.uri,
                    );
                    ui.painter().galley(egui::pos2(text_left, y + title_h + 4.0), artist, palette.secondary);
                }
                let _ = super::lyrics::controls_box_inner(app, ui, egui::pos2(0.0, card_h), width, false);
                // A press that begins on a bar drags the window too.
                // Once, when the press lands (asking again every frame made the
                // window judder).
                let own_vp = ctx.viewport_id();
                let bar_dragging = ctx
                    .data(|data| data.get_temp::<bool>(Id::new(("popout-bar-dragging", own_vp))))
                    .unwrap_or(false);
                let was_key = Id::new("detached-bar-was");
                let was = ctx.data(|data| data.get_temp::<bool>(was_key)).unwrap_or(false);
                if bar_dragging && !was {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                ctx.data_mut(|data| data.insert_temp(was_key, bar_dragging));
                // Scaling: the bottom-right corner, like the one in the app.
                let corner = Rect::from_min_size(body.max - egui::vec2(18.0, 18.0), egui::vec2(18.0, 18.0));
                let handle = ui.interact(corner, Id::new("detached-corner"), egui::Sense::drag());
                if handle.hovered() || handle.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                    ui.painter().circle_filled(corner.center(), 4.0, Color32::from_white_alpha(110));
                }
                if handle.dragged() {
                    let d = handle.drag_delta();
                    let scale = (app.settings.controls_scale_value() + d.x / 232.0 + d.y / 56.0).clamp(0.7, 1.8);
                    app.actions.push(Action::SetControlsScale(scale));
                }
                grip.context_menu(|ui| {
                    if ui.button("Put back in the window").clicked() {
                        put_back = true;
                        ui.close();
                    }
                });
            });
        if let Some(outer) = ctx.input(|input| input.viewport().outer_rect) {
            place = Some([outer.min.x, outer.min.y]);
            // Let go near an edge of the app's window and it sticks there,
            // flush along it (inside, or just outside, the edge).
            let main: Option<Rect> = ctx.data(|data| data.get_temp(Id::new("main-outer")));
            if let Some(main) = main
                && !ctx.input(|input| input.pointer.any_down())
            {
                const NEAR: f32 = 18.0;
                let pick = |value: f32, targets: [f32; 4]| {
                    targets.into_iter().find(|t| (value - t).abs() <= NEAR && (value - t).abs() >= 1.0)
                };
                let (w, h) = (outer.width(), outer.height());
                let x = pick(outer.min.x, [main.left(), main.right() - w, main.right(), main.left() - w]);
                let y = pick(outer.min.y, [main.top(), main.bottom() - h, main.bottom(), main.top() - h]);
                if x.is_some() || y.is_some() {
                    let to = egui::pos2(x.unwrap_or(outer.min.x), y.unwrap_or(outer.min.y));
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(to));
                    place = Some([to.x, to.y]);
                }
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
    });
    if put_back {
        app.settings.float_detached = false;
        app.mark_settings_dirty();
    } else if let Some(now) = place
        && saved.is_none_or(|old| (old[0] - now[0]).abs() > 1.0 || (old[1] - now[1]).abs() > 1.0)
    {
        app.settings.float_window = Some(now);
        app.mark_settings_dirty();
    }
}

pub fn show(app: &mut App, ctx: &Context) {
    let has_topbar = !(app.fullscreen_vis || app.lyrics_fullscreen.is_some() || app.mini_active || app.calm_mode);
    if !has_topbar {
        corner_disc(app, ctx);
    }
    floating_controls(app, ctx);
    // The panel remembers where it was left. A right click on the disc asks
    // for the place right under it instead.
    let open_id = Id::new("views-panel-was-open");
    let pos_id = Id::new("views-panel-pos");
    if !app.views_panel {
        if ctx.data(|data| data.get_temp::<bool>(open_id)).unwrap_or(false) {
            ctx.data_mut(|data| {
                data.insert_temp(open_id, false);
                data.remove::<Rect>(Id::new("views-panel-rect"));
            });
        }
        return;
    }
    if !ctx.data(|data| data.get_temp::<bool>(open_id)).unwrap_or(false) {
        // Just opened.
        let at_disc = ctx.data(|data| data.get_temp::<bool>(Id::new("views-at-disc"))).unwrap_or(false);
        let anchor: egui::Pos2 = ctx.data(|data| data.get_temp(Id::new("views-anchor"))).unwrap_or(egui::pos2(60.0, 60.0));
        let known: Option<egui::Pos2> = ctx.data(|data| data.get_temp(pos_id));
        let start = if at_disc || known.is_none() { anchor } else { known.unwrap_or(anchor) };
        ctx.data_mut(|data| {
            data.insert_temp(pos_id, start);
            data.insert_temp(Id::new("views-at-disc"), false);
        });
    }
    ctx.data_mut(|data| data.insert_temp(open_id, true));
    let panel_id = Id::new("views-panel-v4");
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
    let last_rect_local = last_rect;
    // The panel can be drawn bigger or smaller: its layer is scaled about its
    // top-left corner, drawing and pointer alike.
    let scale = if app.settings.views_scale.is_finite() { app.settings.views_scale.clamp(0.7, 1.6) } else { 1.0 };
    // Shrunk further when the window is too short for it, so it never needs a
    // scroll bar: everything on it just gets smaller.
    let scale = match last_rect {
        Some(rect) if rect.height() > 1.0 => scale.min(((ctx.content_rect().height() - 16.0) / rect.height()).clamp(0.45, 1.6)),
        _ => scale,
    };
    // The panel's corner is its pivot: it grows and shrinks from there, and
    // dragging it moves the corner by exactly as much as the pointer moved.
    let mut panel_pos: egui::Pos2 = ctx.data(|data| data.get_temp(pos_id)).unwrap_or(egui::pos2(60.0, 60.0));
    if let Some(rect) = last_rect {
        let screen = ctx.content_rect();
        panel_pos.x = panel_pos.x.clamp(screen.left(), (screen.right() - rect.width() * scale).max(screen.left()));
        panel_pos.y = panel_pos.y.clamp(screen.top(), (screen.bottom() - rect.height() * scale).max(screen.top()));
    }
    let pivot = panel_pos;
    let to_global = |p: egui::Pos2| pivot + (p - pivot) * scale;
    let panel_layer = egui::LayerId::new(egui::Order::Foreground, panel_id);
    ctx.set_transform_layer(
        panel_layer,
        egui::emath::TSTransform::new(pivot.to_vec2() * (1.0 - scale), scale),
    );
    let last_rect = last_rect.map(|rect| Rect::from_min_max(to_global(rect.min), to_global(rect.max)));
    let over = last_rect.is_some_and(|rect| {
        ctx.input(|input| input.pointer.hover_pos().is_some_and(|p| rect.expand(10.0).contains(p)) || input.pointer.any_down() && input.pointer.interact_pos().is_some_and(|p| rect.expand(30.0).contains(p)))
    });
    let dim = panel_dim(ctx, over);
    let frame = frame.multiply_with_opacity(dim);
    let width = 400.0;
    let shown = egui::Window::new("Views")
        .id(panel_id)
        .title_bar(false)
        .order(egui::Order::Foreground)
        .frame(frame)
        .fixed_pos(panel_pos)
        .min_width(width)
        .max_width(width)
        .resizable(false)
        .collapsible(false)
        .constrain(false)
        .show(ctx, |ui| {
            ui.set_opacity(dim);
            // The whole panel is a handle (registered first, so the buttons
            // on it still take their own clicks).
            if let Some(grip_rect) = last_rect_local {
                let grip = ui.interact(grip_rect, Id::new("views-panel-grip"), egui::Sense::drag());
                if grip.dragged() {
                    let moved = ui.ctx().input(|input| input.pointer.delta());
                    ui.ctx().data_mut(|data| data.insert_temp(pos_id, panel_pos + moved));
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                }
            }
            ui.visuals_mut().override_text_color = Some(palette.text);
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            ui.vertical(|ui| {
                    // Icons only, one row: the four views of this window (exactly
                    // one is on) and the mini player, which is a window of its own.
                    let views = [
                        (Icon::PanelLeft, "Normal", ViewKind::Visualizer),
                        (Icon::Mic, "Full screen lyrics", ViewKind::FullLyrics),
                        (Icon::AudioLines, "Full screen visualizer", ViewKind::FullVisualizer),
                        (Icon::Shrink, "Mini player", ViewKind::Mini),
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
                    let on_top = ("Always on top", app.settings.main_on_top, Action::ToggleMainOnTop);
                    let flag = |bit: u8, label: &'static str, inverted: bool, app: &App| {
                        (label, app.settings.lyrics_flag(bit) != inverted, Action::ToggleLyricsFlag(bit))
                    };
                    use crate::settings::Settings as S;
                    // Only what belongs to the view you are in.
                    let chips: Vec<(&str, bool, Action)> = match current {
                        ViewKind::Calm => Vec::new(),
                        ViewKind::FullVisualizer => vec![
                            ("Lyrics on it", app.settings.vis_lyrics, Action::ToggleVisLyrics),
                            flag(S::LYRICS_FLOAT_ART, "Floating art", false, app),
                            flag(S::LYRICS_BOUNCE_ART, "Art bounces", false, app),
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
                            ("Library", app.settings.sidebar_visible, Action::InDefaultView(Box::new(Action::ToggleSidebar))),
                            ("Side lyrics", app.show_lyrics_panel, Action::InDefaultView(Box::new(Action::ToggleLyricsPanel))),
                            ("Queue", app.show_queue_panel, Action::InDefaultView(Box::new(Action::ToggleQueuePanel))),
                            ("Big album art", app.settings.art_expanded, Action::InDefaultView(Box::new(Action::ToggleArtExpanded))),
                            ("Visualizer", app.settings.vis_shapes_value() != 0, Action::ToggleVisShapes),
                            ("Visualizer settings", app.vis_panel, Action::ToggleVisPanel),
                            (if app.extra_vis { "Close visualizer window" } else { "New visualizer window" }, app.extra_vis, Action::ToggleExtraWindow),
                            on_top,
                        ],
                    };
                    if !chips.is_empty() {
                        // Always one row: the buttons shrink to fit, never wrap.
                        let count = chips.len() as f32;
                        let chip_w = ((ui.available_width() - 6.0 * (count - 1.0)) / count).clamp(22.0, 38.0);
                        // The visualizer window and always-on-top sit apart on the
                        // right, with a gap after the rest.
                        let avail = ui.available_width();
                        let right_n = chips
                            .iter()
                            .filter(|(label, _, _)| label.ends_with("visualizer window") || *label == "Always on top")
                            .count();
                        let left_n = chips.len() - right_n;
                        ui.horizontal(|ui| {
                            let mut gap_done = false;
                            for (label, on, action) in chips {
                                let is_right = label.ends_with("visualizer window") || label == "Always on top";
                                if is_right && !gap_done {
                                    gap_done = true;
                                    let used = left_n as f32 * (chip_w + 6.0);
                                    let right_w = right_n as f32 * chip_w + (right_n as f32 - 1.0) * 6.0;
                                    let gap = (avail - used - right_w - 6.0).max(8.0);
                                    ui.add_space(gap);
                                }
                                let icon = match label {
                                    "Library" => Icon::Grid3x3,
                                    "Side lyrics" => Icon::Menu,
                                    "Queue" => Icon::LayoutList,
                                    "Big album art" | "Album art" => Icon::Square,
                                    "Visualizer" | "Visualizer behind" => Icon::AudioLines,
                                    "Visualizer settings" => Icon::Settings,
                                    "Always on top" => Icon::Pin,
                                    "Lyrics on it" => Icon::Mic,
                                    "Artist name" => Icon::User,
                                    "Floating art" => Icon::Expand,
                                    "Art bounces" => Icon::Zap,
                                    "Timestamps" => Icon::Clock,
                                    "Countdown" => Icon::Loader,
                                    "Close visualizer window" | "New visualizer window" => Icon::AudioLines,
                                    _ => Icon::Info,
                                };
                                let response = view_button(ui, &palette, icon, label, on, chip_w);
                                if label.ends_with("visualizer window") {
                                    // A small plus (or cross, when it is open) on the corner.
                                    let badge = egui::Rect::from_center_size(response.rect.right_top() + egui::vec2(-9.0, 9.0), egui::Vec2::splat(12.0));
                                    ui.painter().circle_filled(badge.center(), 7.0, palette.accent);
                                    theme::paint_icon(ui, if on { Icon::X } else { Icon::Plus }, badge, 10.0, palette.window);
                                }
                                if response.clicked() {
                                    app.actions.push(action);
                                }
                            }
                        });
                    }
                });
        });
    if let Some(window) = shown {
        let local = window.response.rect;
        ctx.data_mut(|data| data.insert_temp(rect_id, local));
        // Where the panel really is on the screen, scaled.
        let rect = Rect::from_min_max(to_global(local.min), to_global(local.max));
        // The bottom-right corner scales the whole panel (double-click: normal size).
        egui::Area::new(Id::new("views-panel-corner"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(rect.right() - 18.0, rect.bottom() - 18.0))
            .show(ctx, |ui| {
                let (strip, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::drag());
                if response.hovered() || response.dragged() {
                    ctx.set_cursor_icon(egui::CursorIcon::ResizeNwSe);
                    ui.painter().circle_filled(strip.center(), 4.0, Color32::from_white_alpha(110));
                }
                if response.dragged() {
                    let d = response.drag_delta();
                    app.settings.views_scale = (scale + (d.x + d.y) / 800.0).clamp(0.7, 1.6);
                    app.mark_settings_dirty();
                }
                if response.double_clicked() {
                    app.settings.views_scale = 1.0;
                    app.mark_settings_dirty();
                }
            });
    }
    if close {
        app.actions.push(Action::ToggleViewsPanel);
    }
}

/// How visible the Views panel is: full while the pointer is near it, and
/// almost gone (5%) a couple of seconds after it leaves.
fn panel_dim(ctx: &Context, over: bool) -> f32 {
    fade(ctx, over, "views-panel-seen")
}

/// The same fade for any panel: full while the pointer is near, nearly gone
/// two seconds after it leaves. `key` keeps each panel's timer apart.
fn fade(ctx: &Context, over: bool, key: &str) -> f32 {
    fade_to(ctx, over, key, 0.05)
}

/// `fade`, but resting at `floor` instead of nearly gone.
fn fade_to(ctx: &Context, over: bool, key: &str, floor: f32) -> f32 {
    let seen_id = Id::new(key);
    let now_t = ctx.input(|input| input.time);
    if over {
        ctx.data_mut(|data| data.insert_temp(seen_id, now_t));
    }
    let last_seen: f64 = ctx.data(|data| data.get_temp(seen_id)).unwrap_or(now_t);
    let away = now_t - last_seen > 2.0;
    if !away {
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }
    ctx.animate_value_with_time(seen_id.with("dim"), if away { floor } else { 1.0 }, 0.4)
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
