//! The now-playing bar along the bottom of the window.

use egui::{Align, Color32, Frame, Layout, Margin, Rect, Sense, UiBuilder, Vec2, pos2, vec2};

use crate::app::{App, NowPlaying};
use crate::i18n::gettext;
use crate::model::{Action, DragTrack, Page};
use crate::player::RepeatMode;
use crate::theme::{self, Icon};
use crate::util;

use super::widgets::{SliderEvent, thin_slider_scaled};

/// How much of the playing art's tint the bar's fill carries.
const TINT_STRENGTH: f32 = 0.12;
/// How long the bar takes to cross over to a new song's tint.
const TINT_FADE_SECONDS: f32 = 0.45;
const TINT_SESSION_ID: &str = "player-bar-tint-session";
/// How strongly the visualizer shows through behind the controls: the
/// spectrum's bars fade from their foot to their top, with a glow around
/// them, a brighter cap above, and a pulse along the foot with the bass.
const SPECTRUM_ALPHA: (f32, f32) = (0.28, 0.08);
const GLOW_ALPHA: f32 = 0.12;
const PEAK_ALPHA: f32 = 0.7;
const PULSE_ALPHA: f32 = 0.35;
/// The bass glow's strength under the flow, where the sheets themselves
/// are already faint and translucent.
const FLOW_GLOW: f32 = 0.5;
/// The peak cap's thickness, and its gap above the band.
const PEAK_HEIGHT: f32 = 2.0;
const PEAK_GAP: f32 = 2.0;
/// The waveform's line, as layers from the widest glow to the core, the
/// shading under it, and its mirrored echo.
const WAVE_LAYERS: [(f32, f32); 3] = [(9.0, 0.06), (4.0, 0.16), (1.6, 0.85)];
const WAVE_FILL_ALPHA: f32 = 0.14;
const WAVE_ECHO_ALPHA: f32 = 0.18;
/// How much more strongly the visualizer paints over a light bar.
const LIGHT_STRENGTH: f32 = 1.5;
/// The gap between spectrum bars, when the reader has not chosen one.
pub(crate) const SPECTRUM_GAP: f32 = 2.0;
/// How often a moving visualizer is drawn: sixty times a second, as the
/// mini player's.
const VIS_FRAME: std::time::Duration = std::time::Duration::from_micros(16_667);

/// Forget this bar's animation session while the sign-in screen is shown.
pub(crate) fn end_tint_session(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<u64>(egui::Id::new(TINT_SESSION_ID)));
}

/// The mouse wheel over the visualizer steps through its shapes: up one way,
/// down the other.
#[allow(dead_code)]
fn scroll_vis_mode(app: &mut App, ui: &egui::Ui, over: &egui::Response) {
    if !over.hovered() {
        return;
    }
    let dy = ui.input(|input| input.smooth_scroll_delta.y);
    if dy == 0.0 {
        return;
    }
    let id = egui::Id::new("vis-scroll-total");
    let total = ui.ctx().data_mut(|data| {
        let total = data.get_temp_mut_or_default::<f32>(id);
        *total += dy;
        *total
    });
    if total.abs() > 60.0 {
        ui.ctx().data_mut(|data| data.insert_temp(id, 0.0_f32));
        use crate::settings::PlayerBarVis::{Flow, Off, Spectrum, Swirl};
        let order = [Off, Spectrum, Flow, Swirl];
        let at = order
            .iter()
            .position(|mode| *mode == app.settings.player_bar_vis)
            .unwrap_or(3);
        let step = if total > 0.0 { 1 } else { order.len() - 1 };
        app.actions
            .push(Action::SetVisMode(order[(at + step) % order.len()]));
    }
}

/// The mouse wheel over the volume: it lands on the nearest multiple of 5
/// in the direction of the turn (32 up is 35, 32 down is 30), then moves in
/// steps of 5. `None` when the wheel was not turned over `area`.
pub(crate) fn wheel_volume(ui: &egui::Ui, area: Rect, shown: u8) -> Option<u8> {
    if !ui.rect_contains_pointer(area) {
        return None;
    }
    wheel_steps(ui, shown)
}

/// The same, for the pop-out controls: that panel is painted over a bare
/// layer whose own size is empty, so the layer test above never says "over
/// it". The pointer's place is what counts there.
fn wheel_volume_at(ui: &egui::Ui, area: Rect, shown: u8) -> Option<u8> {
    let over = ui.input(|input| input.pointer.hover_pos()).is_some_and(|p| area.contains(p));
    if !over {
        return None;
    }
    wheel_steps(ui, shown)
}

fn wheel_steps(ui: &egui::Ui, shown: u8) -> Option<u8> {
    let turned: f32 = ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel { delta, .. } => Some(delta.y),
                _ => None,
            })
            .sum()
    });
    if turned == 0.0 {
        return None;
    }
    let now = u16::from(shown.min(100));
    let next = if turned > 0.0 {
        (now / 5 + 1) * 5
    } else if now % 5 == 0 {
        now.saturating_sub(5)
    } else {
        now / 5 * 5
    };
    Some(next.min(100) as u8)
}

/// Runs a piece of drawing so that a mistake in it costs one frame of that
/// picture and never the program. The panic is logged by the panic hook;
/// what comes back is the type's empty value (nothing drawn, not moving).
fn guarded<R: Default>(what: &str, draw: impl FnOnce() -> R) -> R {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(draw)) {
        Ok(value) => value,
        Err(_) => {
            log::error!("{what} skipped a frame after an error");
            R::default()
        }
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    eq_preview_watchdog(app, &ui.ctx().clone());
    {
        let ctx = ui.ctx().clone();
        crate::crash::stage("visualizer panel: begin");
        let fine = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            vis_panel_window(app, &ctx);
        }))
        .is_ok();
        crate::crash::stage("visualizer panel: end");
        if !fine {
            // The panel closes rather than taking the program with it.
            log::error!("the visualizer panel stopped after an error and was closed");
            app.vis_panel = false;
        }
    }
    // The inspector, once a frame over everything, so it can see what every
    // other layer drew this frame rather than guessing from the last.
    crate::inspect::panel(app, ui);
    // The scenes panel, likewise over everything: it is about the sound,
    // not about any one part of the page.
    crate::scenes::panel(app, ui);
    // Fullscreen: the whole window is the visualizer and nothing else is
    // drawn over it. The panel fills the window rather than the bar, the
    // controls are skipped, and the same click still opens the menu.
    if app.fullscreen_vis {
        let area = ui.ctx().input(|input| input.raw.screen_rect);
        let Some(area) = area else {
            return;
        };
        egui::Panel::bottom("player-bar")
            .exact_size(area.height())
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(Color32::TRANSPARENT))
            .show(ui, |ui| {
                let now = app.now_playing();
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::new("vis-row-h"), 0.0_f32));
                if guarded("visualizer", || visualizer(app, ui, ui.max_rect(), now.as_ref())) {
                    ui.ctx().request_repaint_after(VIS_FRAME);
                }
                let empty = ui.interact(
                    ui.max_rect(),
                    ui.id().with("player-bar-visualizer"),
                    Sense::click(),
                );
                vis_menu(app, &empty);
                    // Left click does nothing here either; see the note in the
                // ordinary bar below.
            });
        return;
    }
    // Under full-screen lyrics the bar is not drawn at all, background and
    // controls both: the page takes the whole window and a small box under
    // the album art holds the controls. Leaving the page brings it back.
    if app.lyrics_fullscreen.is_some() {
        let bottom = ui.ctx().input(|input| input.raw.screen_rect).map_or(0.0, |rect| rect.bottom());
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("vis-row-h"), 0.0_f32);
            data.insert_temp(egui::Id::new("vis-top"), bottom);
        });
        egui::Panel::bottom("player-bar")
            .exact_size(0.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(Color32::TRANSPARENT))
            .show(ui, |_| {});
        return;
    }
    // Clean screen: the controls are popped out and the reader wants the
    // whole bottom bar gone, track info included.
    if (app.settings.float_hide_bar || app.settings.vis_shapes_value() == 0)
        && !app.mini_active
        && app.float_slot(super::views_panel::default_float(ui.ctx())).is_some()
    {
        let bottom = ui.ctx().input(|input| input.raw.screen_rect).map_or(0.0, |rect| rect.bottom());
        ui.ctx().data_mut(|data| {
            data.insert_temp(egui::Id::new("vis-row-h"), 0.0_f32);
            data.insert_temp(egui::Id::new("vis-top"), bottom);
        });
        egui::Panel::bottom("player-bar")
            .exact_size(0.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(Color32::TRANSPARENT))
            .show(ui, |_| {});
        return;
    }
    // The bar is one fixed dark colour; the cover's colour lives in the
    // visualizer, not in the bar behind it.
    let lyrics_page = false;
    let popped_out = app.float_slot(super::views_panel::default_float(ui.ctx())).is_some() && !app.mini_active;
    let fill = if popped_out {
        // The controls are out on their own panel: the bar's background goes.
        Color32::TRANSPARENT
    } else if palette.dark {
        Color32::from_rgb(0x11, 0x12, 0x13)
    } else {
        palette.panel
    };
    // The visualizer may be scaled up and raised, and a panel clips
    // whatever it draws. So the panel grows to whatever the visualizer is
    // asking for, and the controls stay in a band at its foot where they
    // always were. At the default scale and rise this is exactly the bar's
    // own height and nothing moves.
    // The only thing that sizes the visualizer is the grip on its top edge.
    let scale = 1.0_f32;
    let shapes_on = app.settings.vis_shapes_value() != 0;
    // With no shape on there is no visualizer: the bar closes to the one row.
    let bar_h = if shapes_on { theme::PLAYER_BAR_HEIGHT } else { 0.0 };
    let foot = if shapes_on { 0.0 } else { 10.0 };
    // However tall it is raised, it never takes the whole window.
    let screen_h = ui
        .ctx()
        .input(|input| input.raw.screen_rect)
        .map_or(900.0, |rect| rect.height());
    let rise = if shapes_on {
        // Under full-screen lyrics the picture may not eat the page: it
        // stops at 30% of the window so a few lines always stay readable.
        let cap = if app.lyrics_fullscreen.is_some() {
            screen_h * 0.30
        } else {
            f32::MAX
        };
        (app.settings.player_bar_vis_rise() * theme::PLAYER_BAR_HEIGHT)
            .min((screen_h - 200.0 - theme::PLAYER_BAR_HEIGHT).max(0.0))
            .min(cap)
    } else {
        0.0
    };
    // One row above the visualizer holds everything you press: the play
    // buttons, then the song-length bar, then the volume. The row grows with
    // the controls' size setting, so nothing is drawn over the picture.
    // One size for the controls now: the bar's own row no longer resizes.
    let controls_k = 1.0_f32;
    // However the reader set it, a narrow window stacks the controls: two
    // rows when it is getting tight, three when it is small, so the buttons
    // always sit together instead of spreading across the bar.
    let window_w = ui.ctx().content_rect().width();
    let auto_layout = if window_w < 760.0 {
        2
    } else if window_w < 1300.0 {
        1
    } else {
        0
    };
    let layout = match app.settings.bar_layout_value() {
        3 => 3,
        chosen => chosen.max(auto_layout),
    };
    let stacked = layout != 0;
    // The controls are their own panel, so the bar keeps no row for them: a
    // slim one for the song's name, none at all with big album art.
    // With the controls popped out the bar is only the visualizer: the song's
    // name and cover sit above the controls instead.
    let strip_h = if lyrics_page || popped_out {
        0.0
    } else {
        (46.0 * controls_k).clamp(46.0, 84.0)
            * match layout {
                0 => 1.0,
                2 => 2.2,
                _ => 1.6,
            }
    };
    let height = bar_h + foot + rise + theme::PLAYER_BAR_HEIGHT * (scale - 1.0) + strip_h;
    egui::Panel::bottom("player-bar")
        .exact_size(height.max(bar_h + foot + strip_h))
        .resizable(false)
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(fill)
                .inner_margin(Margin::symmetric(16, 0)),
        )
        .show(ui, |ui| {
            let grown = ui.max_rect();
            // The controls live in the bar's own height at the foot of the
            // panel, whatever else the panel has grown to hold.
            let rect = Rect::from_min_max(
                pos2(grown.left(), grown.bottom() - foot - bar_h),
                pos2(grown.right(), grown.bottom() - foot),
            );
            // Where the visualizer starts, so its panel can sit above it.
            ui.ctx().data_mut(|data| {
                data.insert_temp(
                    egui::Id::new("vis-top"),
                    rect.top() - rise - strip_h,
                )
            });
            let now = app.now_playing();
            // The area the visualizer is drawn over, margins included.
            let behind = Rect::from_min_max(
                pos2(
                    grown.left() - 16.0,
                    rect.top() - rise - theme::PLAYER_BAR_HEIGHT * (scale - 1.0),
                ),
                pos2(grown.right() + 16.0, rect.bottom() + 1.0),
            );
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("vis-row-h"), strip_h);
                data.insert_temp(egui::Id::new("tour-visual"), behind);
            });
            if guarded("visualizer", || visualizer(app, ui, behind, now.as_ref())) {
                ui.ctx().request_repaint_after(VIS_FRAME);
            }
            // Its empty space is the visualizer's control, as Winamp's
            // visualizer was: a right-click opens its menu. A left click
            // does nothing at all — it used to step to the next mode, which
            // meant a stray click while reaching for the play button
            // silently changed what the bar was drawing. The controls drawn
            // after it still take their own left clicks.
            let empty = ui.interact(
                behind,
                ui.id().with("player-bar-visualizer"),
                Sense::click(),
            );
            vis_menu(app, &empty);
            // A grip along the top edge: drag it up or down to make the
            // visualizer taller or shorter.
            if shapes_on {
                let grip = Rect::from_min_size(behind.min, vec2(behind.width(), 14.0));
                let drag = ui.interact(grip, ui.id().with("vis-grip"), Sense::drag());
                if drag.hovered() || drag.dragged() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
                    ui.painter().rect_filled(
                        Rect::from_center_size(grip.center_top() + vec2(0.0, 6.0), vec2(56.0, 4.0)),
                        2.0,
                        Color32::WHITE.gamma_multiply(0.5),
                    );
                }
                if drag.dragged() {
                    let rise = app.settings.player_bar_vis_rise()
                        - drag.drag_delta().y / theme::PLAYER_BAR_HEIGHT;
                    app.actions.push(Action::SetVisRise(rise));
                }
            }
            if lyrics_page {
                return;
            }
            // No tooltip here. It held the pointer's text over the whole
            // width of the bar.
            //
            // No separator line either. The one hairline along the top of
            // the bar read as a black rule cutting the window in two above
            // the play button, and the visualizer behind it already makes
            // where the bar starts obvious.
            let width = rect.width();
            let side = (width * 0.26).clamp(190.0, 420.0);

            let row = Rect::from_min_max(
                pos2(grown.left(), behind.top() - strip_h),
                pos2(grown.right(), behind.top()),
            );
            // For the guided tour: the row of controls (not the visualizer).
            ui.ctx()
                .data_mut(|data| data.insert_temp(egui::Id::new("tour-player"), row));
            // The cover and the song's name at the left of the row.
            {
                // Popped out, the slim row is only as wide as the library
                // sidebar above it, and only there.
                let side = if popped_out {
                    let sidebar_w = if app.settings.sidebar_visible {
                        ui.ctx()
                            .data(|data| data.get_temp::<Rect>(egui::Id::new("tour-sidebar")))
                            .map_or(app.settings.sidebar_width, |rect| rect.width())
                    } else {
                        260.0
                    };
                    (sidebar_w - 28.0).max(150.0)
                } else {
                    side
                };
                let region = Rect::from_min_max(
                    pos2(grown.left(), row.top()),
                    pos2(grown.left() + side, row.bottom()),
                );
                if popped_out && region.height() >= 20.0 {
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            pos2(grown.left() - 16.0, row.top()),
                            pos2(region.right() + 16.0, row.bottom() + 1.0),
                        ),
                        0.0,
                        palette.panel,
                    );
                }
                if region.height() >= 20.0 {
                    now_playing_block(app, ui, region, now.as_ref());
                }
            }
            // Then the media controls, the song-length bar, and the volume,
            // all in the same row, in the order the reader chose: the groups
            // before the song bar pack to the left, the ones after it to
            // the right, and the bar sits in what is left.
            let presets = app.settings.volume_presets_value();
            // The volume bar is never longer than the song bar: whatever the
            // room, it keeps to at most half of what the two share.
            let controls_w = 218.0 * (1.0 + (controls_k - 1.0) * 0.5);
            let extra_w = VOLUME_BLOCK - 100.0 + 44.0 * presets.len() as f32;
            let room = (grown.right() - (grown.left() + side + 8.0)) - controls_w - extra_w - 36.0;
            let volume_slider = volume_bar_width(app).min((room / 2.0).max(60.0));
            ui.ctx()
                .data_mut(|data| data.insert_temp(egui::Id::new("volume-w"), volume_slider));
            let volume_w = VOLUME_BLOCK + (volume_slider - 100.0) + 44.0 * presets.len() as f32;
            // Stacked, the controls and volume take the upper band of the
            // row and the song bar the lower one.
            let slice = |from: f32, to: f32| {
                Rect::from_min_max(
                    pos2(row.left(), row.top() + row.height() * from),
                    pos2(row.right(), row.top() + row.height() * to),
                )
            };
            // `band` is where the controls sit, `band_volume` where the
            // volume does, `seek_row` the song bar's.
            let (band, band_volume, seek_row) = match layout {
                // The song bar under the controls and volume.
                1 => (slice(0.0, 1.0 / 1.6), slice(0.0, 1.0 / 1.6), slice(1.0 / 1.6, 1.0)),
                // Three rows: controls, volume, song bar.
                2 => (slice(0.0, 1.0 / 3.0), slice(1.0 / 3.0, 2.0 / 3.0), slice(2.0 / 3.0, 1.0)),
                // The song bar above the controls and volume.
                3 => (slice(0.6 / 1.6, 1.0), slice(0.6 / 1.6, 1.0), slice(0.0, 0.6 / 1.6)),
                _ => (row, row, row),
            };
            let band_of = |block: u8| if block == 2 { band_volume } else { band };
            let editing_row = row;
            let block_w = [controls_w, 0.0, volume_w];
            let order = ROW_ORDERS[(app.settings.row_order as usize).min(5)];
            let gap = 18.0;
            let avail_left = grown.left() + side + 8.0;
            // A strip at the far right is kept clear for the six dots.
            let avail_right = grown.right() - 16.0;
            let seek_at = order.iter().position(|b| *b == 1).unwrap_or(1);
            // Where each part sits: left, middle or right. Never chosen
            // means by its place in the order, before or after the bar.
            let place_of = |block: u8| order.iter().position(|b| *b == block).unwrap_or(0);
            let anchors = [
                app.settings
                    .anchor_controls
                    .map_or(if place_of(0) < seek_at { 0 } else { 2 }, |a| a.min(2)),
                1u8,
                app.settings
                    .anchor_volume
                    .map_or(if place_of(2) < seek_at { 0 } else { 2 }, |a| a.min(2)),
            ];
            let mut blocks = [Rect::NOTHING; 3];
            let mut taken: Vec<(f32, f32)> = Vec::new();
            let mut left_cursor = avail_left;
            let mut right_cursor = avail_right;
            for block in order.iter().filter(|b| **b != 1) {
                if anchors[*block as usize] == 0 {
                    let w = block_w[*block as usize];
                    let band = band_of(*block);
                    blocks[*block as usize] = Rect::from_min_max(
                        pos2(left_cursor, band.top()),
                        pos2(left_cursor + w, band.bottom()),
                    );
                    taken.push((left_cursor, left_cursor + w));
                    left_cursor += w + gap;
                }
            }
            for block in order.iter().rev().filter(|b| **b != 1) {
                if anchors[*block as usize] == 2 {
                    let w = block_w[*block as usize];
                    right_cursor -= w;
                    let band = band_of(*block);
                    blocks[*block as usize] = Rect::from_min_max(
                        pos2(right_cursor, band.top()),
                        pos2(right_cursor + w, band.bottom()),
                    );
                    taken.push((right_cursor, right_cursor + w));
                    right_cursor -= gap;
                }
            }
            let middle: Vec<u8> = order
                .iter()
                .copied()
                .filter(|b| *b != 1 && anchors[*b as usize] == 1)
                .collect();
            if !middle.is_empty() {
                let total: f32 = middle.iter().map(|b| block_w[*b as usize]).sum::<f32>()
                    + gap * (middle.len() as f32 - 1.0);
                let mut x = ((avail_left + avail_right) / 2.0 - total / 2.0)
                    .min(right_cursor - total)
                    .max(left_cursor);
                for block in middle {
                    let w = block_w[block as usize];
                    let band = band_of(block);
                    blocks[block as usize] = Rect::from_min_max(
                        pos2(x, band.top()),
                        pos2(x + w, band.bottom()),
                    );
                    taken.push((x, x + w));
                    x += w + gap;
                }
            }
            if layout == 2 {
                // Each on its own row: nothing shares a row, so each sits
                // where its own Left, Middle or Right says.
                for b in [0u8, 2u8] {
                    let w = block_w[b as usize];
                    let x = match anchors[b as usize] {
                        0 => avail_left,
                        1 => ((avail_left + avail_right) / 2.0 - w / 2.0).max(avail_left),
                        _ => (avail_right - w).max(avail_left),
                    };
                    let band = band_of(b);
                    blocks[b as usize] =
                        Rect::from_min_max(pos2(x, band.top()), pos2(x + w, band.bottom()));
                }
            }
            // The reader may have dragged the controls or the volume aside,
            // in steps of a grid; they stay inside the row and never overlap.
            let grid = 12.0_f32;
            let editing = ui.rect_contains_pointer(editing_row);
            for (b, steps) in [(0usize, app.settings.nudge_controls), (2usize, app.settings.nudge_volume)] {
                let w = block_w[b];
                let band = band_of(b as u8);
                let base = blocks[b].left();
                let free = app.settings.bar_free;
                // "My own layout": the controls and the volume sit wherever
                // they were dragged, across the whole bar.
                let (x, top) = if free {
                    let (fx, fy) = app
                        .settings
                        .bar_free_pos
                        .map(|p| p[b / 2])
                        .map_or((blocks[b].left(), blocks[b].top()), |p| (grown.left() + p[0], row.top() + p[1]));
                    (
                        fx.clamp(avail_left, (avail_right - w).max(avail_left)),
                        fy.clamp(row.top(), (row.bottom() - band.height()).max(row.top())),
                    )
                } else {
                    (
                        (base + f32::from(steps) * grid).clamp(avail_left, (avail_right - w).max(avail_left)),
                        band.top(),
                    )
                };
                blocks[b] = Rect::from_min_size(pos2(x, top), vec2(w, band.height()));
                if editing {
                    // A handle at the block's start: drag it along the row.
                    let handle = Rect::from_min_size(pos2(x - 2.0, top + 1.0), vec2(14.0, 12.0));
                    let drag = ui.interact(handle, egui::Id::new(("block-handle", b)), Sense::drag());
                    let color = if drag.hovered() || drag.dragged() {
                        palette.text
                    } else {
                        palette.dim
                    };
                    let _ = color;
                    if drag.hovered() || drag.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                    if drag.dragged() && free {
                        let delta = drag.drag_delta();
                        app.actions.push(Action::SetFreePos(
                            (b / 2) as u8,
                            [x + delta.x - grown.left(), top + delta.y - row.top()],
                        ));
                    } else if drag.dragged() {
                        let key = egui::Id::new(("block-residual", b));
                        let mut residual = ui.ctx().data(|data| data.get_temp::<f32>(key)).unwrap_or(0.0);
                        residual += drag.drag_delta().x;
                        let moved = (residual / grid).trunc();
                        residual -= moved * grid;
                        ui.ctx().data_mut(|data| data.insert_temp(key, residual));
                        if moved != 0.0 {
                            app.actions
                                .push(Action::SetBlockNudge(b as u8, steps + moved as i16));
                        }
                    }
                }
            }
            if app.settings.bar_free && app.settings.bar_free_pos.is_none() {
                // First time in: start from where they are now.
                for (which, b) in [(0u8, 0usize), (1u8, 2usize)] {
                    app.actions.push(Action::SetFreePos(
                        which,
                        [blocks[b].left() - grown.left(), blocks[b].top() - row.top()],
                    ));
                }
            }
            if layout != 2
                && !app.settings.bar_free
                && blocks[0].left() < blocks[2].right()
                && blocks[2].left() < blocks[0].right()
            {
                // They touch: the volume steps aside, or else the controls.
                let right = (blocks[0].right() + gap).min(avail_right - block_w[2]);
                if right >= blocks[0].right() {
                    blocks[2] = Rect::from_min_max(pos2(right, band_volume.top()), pos2(right + block_w[2], band_volume.bottom()));
                } else {
                    let left = (blocks[2].left() - gap - block_w[0]).max(avail_left);
                    blocks[0] = Rect::from_min_max(pos2(left, band.top()), pos2(left + block_w[0], band.bottom()));
                }
            }
            taken = vec![
                (blocks[0].left(), blocks[0].right()),
                (blocks[2].left(), blocks[2].right()),
            ];
            // The song-length bar takes the widest free stretch left over.
            taken.sort_by(|a, b| a.0.total_cmp(&b.0));
            let mut zone = (avail_left, avail_left);
            let mut cursor = avail_left;
            for (start, end) in taken {
                if start - gap - cursor > zone.1 - zone.0 {
                    zone = (cursor, start - gap);
                }
                cursor = cursor.max(end + gap);
            }
            if avail_right - cursor > zone.1 - zone.0 {
                zone = (cursor, avail_right);
            }
            let (left_cursor, right_cursor) = if stacked { (avail_left, avail_right) } else { zone };
            // "My own layout": the song bar too sits wherever it was dragged.
            let (seek_row, left_cursor) = if app.settings.bar_free {
                let h = 26.0_f32.min(row.height());
                let [fx, fy] = app.settings.bar_free_seek.unwrap_or([
                    left_cursor - grown.left(),
                    seek_row.center().y - h / 2.0 - row.top(),
                ]);
                let x = (grown.left() + fx).clamp(avail_left, (avail_right - 120.0).max(avail_left));
                let y = (row.top() + fy).clamp(row.top(), (row.bottom() - h).max(row.top()));
                if editing {
                    let handle = Rect::from_min_size(pos2(x - 2.0, y + h / 2.0 - 6.0), vec2(14.0, 12.0));
                    let drag = ui.interact(handle, egui::Id::new("seek-handle"), Sense::drag());
                    let color = if drag.hovered() || drag.dragged() { palette.text } else { palette.dim };
                    let _ = color;
                    if drag.hovered() || drag.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                    if drag.dragged() {
                        let delta = drag.drag_delta();
                        app.actions
                            .push(Action::SetFreeSeek([x + delta.x - grown.left(), y + delta.y - row.top()]));
                    }
                }
                (Rect::from_min_size(pos2(row.left(), y), vec2(row.width(), h)), x)
            } else {
                (seek_row, left_cursor)
            };
            let right_cursor = if app.settings.bar_free { avail_right } else { right_cursor };
            // While the controls are popped out (by choice, or because the
            // album art is big) the bar keeps their room but draws nothing.
            let popped = app.float_slot(super::views_panel::default_float(ui.ctx())).is_some();
            // The pop-out controls and the bar's own song-length bar are
            // exclusive: while the controls are out nothing of the bar's own
            // is drawn or answers the pointer.
            let seek_rect = if popped {
                seek_row
            } else {
                transport(
                    app,
                    ui,
                    now.as_ref(),
                    seek_row,
                    band,
                    blocks[0].left(),
                    (left_cursor, right_cursor),
                )
            };
            blocks[1] = seek_rect;
            let mut right_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(blocks[2])
                    .layout(Layout::left_to_right(Align::Center)),
            );
            if !popped {
                extras(app, &mut right_ui, now.as_ref(), &presets);
            }
            blocks[0] = Rect::from_min_max(
                pos2(blocks[0].left(), band.top()),
                pos2(blocks[0].left() + block_w[0], band.bottom()),
            );
            // For the guided tour.
            ui.ctx().data_mut(|data| {
                data.insert_temp(egui::Id::new("tour-controls"), blocks[0]);
                data.insert_temp(egui::Id::new("tour-seek"), blocks[1]);
                data.insert_temp(egui::Id::new("tour-volume"), blocks[2]);
            });
            let name_region = Rect::from_min_max(
                pos2(grown.left(), row.top()),
                pos2(grown.left() + if popped_out { 150.0 } else { side }, row.bottom()),
            );
            row_menus(app, ui, row, blocks, name_region);
        });
}

/// The six orders the bottom row's groups can be in: 0 controls, 1 song
/// bar, 2 volume.
const ROW_ORDERS: [[u8; 3]; 6] = [
    [0, 1, 2],
    [0, 2, 1],
    [1, 0, 2],
    [1, 2, 0],
    [2, 0, 1],
    [2, 1, 0],
];

/// How wide the volume slider and its percent box are together, with the
/// slider at its standard 100.
const VOLUME_BLOCK: f32 = 160.0;

/// The volume slider's own width: the standard 100, or the reader's choice.
fn volume_bar_width(app: &App) -> f32 {
    if app.settings.volume_custom_width > 0.0 {
        app.settings.volume_custom_width
    } else {
        // Grows with the row, a little faster than the buttons.
        100.0 * (1.0 + (app.settings.controls_scale_value() - 1.0) * 1.2).max(0.8)
    }
}

/// Draws the chosen spectrum or waveform of the song playing on this
/// computer across `rect`, in colours drawn from the cover. It reads the
/// same post-equalizer, pre-volume sound as the mini player's visualizer,
/// so the volume never moves it. Returns whether it is still moving.
fn visualizer(app: &mut App, ui: &egui::Ui, rect: Rect, now: Option<&NowPlaying>) -> bool {
    use crate::settings::{PlayerBarVis, Settings};
    let shapes = app.settings.vis_shapes_value();
    if shapes == 0 {
        return false;
    }
    let mut moving = false;
    crate::crash::stage(&format!("visualizer: begin (shapes {shapes}, full screen {})", app.fullscreen_vis));
    // Swirl is the picture at the back; bars and flow are drawn over it.
    if shapes & Settings::SHAPE_SWIRL != 0 {
        crate::crash::stage("visualizer: swirl");
        let art = app.swirl_art(ui.ctx());
        let previous = app.swirl_previous(ui.ctx());
        let dark = ui.visuals().dark_mode;
        let painter = ui.painter().with_clip_rect(rect);
        let strength = if dark { 1.0 } else { LIGHT_STRENGTH };
        let phase = swirl_phase(app, ui, now);
        // "Reverse the visualizer" turns the swirl's drift around too.
        let phase = if app.settings.vis_reverse_swirl { -phase } else { phase };
        let run = SwirlRun {
            bright: app.settings.swirl_tune_value(10) / 100.0,
            wash: app.settings.swirl_tune_value(11) / 100.0,
            bands: app.settings.swirl_tune_value(12),
            art: app.settings.swirl_art_scroll,
            mirror: !app.settings.swirl_art_single,
        };
        swirl(
            &painter,
            rect,
            &[],
            strength,
            phase,
            app.fullscreen_vis,
            art.as_ref(),
            previous,
            run,
            swirl_wash(app, ui.input(|input| input.time)),
        );
        moving = true;
    }
    if shapes & Settings::SHAPE_FLOW != 0 {
        crate::crash::stage("visualizer: flow");
        // Fullscreen, the flow lives under the cover: it starts 20 pixels
        // below the picture and is never taller than 300.
        let flow_rect = if app.lyrics_fullscreen.is_some() {
            // Behind the lyrics it is a low strip along the bottom.
            let height = (rect.height() * 0.16).clamp(40.0, 150.0);
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - height), rect.max)
        } else if app.fullscreen_vis {
            let below = fullscreen_cover(rect, 0).bottom() + 20.0;
            let height = (rect.bottom() - below).clamp(40.0, 300.0);
            Rect::from_min_max(pos2(rect.left(), rect.bottom() - height), rect.max)
        } else {
            rect
        };
        moving |= visualizer_shape(app, ui, flow_rect, now, PlayerBarVis::Flow);
    }
    if shapes & Settings::SHAPE_BARS != 0 {
        crate::crash::stage("visualizer: bars");
        if app.fullscreen_vis || app.lyrics_fullscreen.is_some() {
            moving |= fullscreen_edge_bars(app, ui, rect, now);
        } else {
            // Bars alone may rise off the visualizer onto the song-length
            // row above it; flow and swirl stay on the visualizer.
            let row_h = ui
                .ctx()
                .data(|data| data.get_temp::<f32>(egui::Id::new("vis-row-h")))
                .unwrap_or(0.0);
            let bars = if app.settings.vis_bars_stay || row_h <= 0.0 {
                rect
            } else {
                Rect::from_min_max(pos2(rect.left(), rect.top() - row_h), rect.max)
            };
            moving |= visualizer_shape(app, ui, bars, now, PlayerBarVis::Spectrum);
        }
    }
    // The cover and the song's name are drawn last, so the bars and the flow
    // pass behind them instead of across them.
    if app.fullscreen_vis && shapes & Settings::SHAPE_SWIRL != 0 {
        swirl_scene(app, ui, rect, now);
    }
    crate::crash::stage("visualizer: end");
    moving
}

/// Whether the swirl fills the window behind the side panels and top bar.
pub(super) fn swirl_behind_panels(app: &App) -> bool {
    app.settings.swirl_behind_panels
        && app.settings.vis_shapes_value() & crate::settings::Settings::SHAPE_SWIRL != 0
        && !app.fullscreen_vis
        && app.lyrics_fullscreen.is_none()
}

/// The swirl laid over `rect` and clipped to `clip`, so two pieces drawn
/// with the same `rect` meet seamlessly.
pub(super) fn swirl_fill(app: &mut App, ui: &egui::Ui, rect: Rect, clip: Rect) {
    let now = app.now_playing();
    let art = app.swirl_art(ui.ctx());
    let previous = app.swirl_previous(ui.ctx());
    let dark = ui.visuals().dark_mode;
    let painter = ui.painter().with_clip_rect(clip);
    let strength = if dark { 1.0 } else { LIGHT_STRENGTH };
    let phase = swirl_phase(app, ui, now.as_ref());
    let phase = if app.settings.vis_reverse_swirl { -phase } else { phase };
    let run = SwirlRun {
        bright: app.settings.swirl_tune_value(10) / 100.0,
        wash: app.settings.swirl_tune_value(11) / 100.0,
        bands: app.settings.swirl_tune_value(12),
        art: app.settings.swirl_art_scroll,
        mirror: !app.settings.swirl_art_single,
    };
    swirl(
        &painter,
        rect,
        &[],
        strength,
        phase,
        false,
        art.as_ref(),
        previous,
        run,
        swirl_wash(app, ui.input(|input| input.time)),
    );
    ui.ctx().request_repaint();
}

/// What moves behind the full-screen lyrics: the bars (0), the flow (1) or
/// the swirl (2), over the whole page. Returns whether it is still moving.
pub(super) fn lyrics_backdrop(
    app: &mut App,
    ui: &egui::Ui,
    rect: Rect,
    now: Option<&NowPlaying>,
    mode: u8,
) -> bool {
    use crate::settings::PlayerBarVis;
    match mode {
        // Whatever the visualizer settings say: the same shapes, colours
        // and sliders as the bar's and the full-screen visualizer's.
        3 => visualizer(app, ui, rect, now),
        // Just the bars along the edges, with no cover or words.
        4 => fullscreen_edge_bars(app, ui, rect, now),
        1 => visualizer_shape(app, ui, rect, now, PlayerBarVis::Flow),
        2 => {
            let art = app.swirl_art(ui.ctx());
            let previous = app.swirl_previous(ui.ctx());
            let dark = ui.visuals().dark_mode;
            let painter = ui.painter().with_clip_rect(rect);
            let strength = if dark { 1.0 } else { LIGHT_STRENGTH };
            let phase = swirl_phase(app, ui, now);
        // "Reverse the visualizer" turns the swirl's drift around too.
        let phase = if app.settings.vis_reverse_swirl { -phase } else { phase };
            let run = SwirlRun {
                bright: app.settings.swirl_tune_value(10) / 100.0,
                wash: app.settings.swirl_tune_value(11) / 100.0,
                bands: app.settings.swirl_tune_value(12),
                art: app.settings.swirl_art_scroll,
                mirror: !app.settings.swirl_art_single,
            };
            swirl(
                &painter,
                rect,
                &[],
                strength,
                phase,
                false,
                art.as_ref(),
                previous,
                run,
                swirl_wash(app, ui.input(|input| input.time)),
            );
            true
        }
        _ => visualizer_shape(app, ui, rect, now, PlayerBarVis::Spectrum),
    }
}

/// The fullscreen bars: ragged bars hanging from whichever edges are on,
/// in the cover's colour.
fn fullscreen_edge_bars(
    app: &mut App,
    ui: &egui::Ui,
    rect: Rect,
    now: Option<&NowPlaying>,
) -> bool {
    use crate::vis;
    let sounding = now.is_some_and(|now| (now.playing || now.loading) && now.local);
    if !sounding && app.player_bar_analyser.settled() {
        return false;
    }
    let samples = if sounding {
        app.audio.tap.window(vis::WIDE_SAMPLES, vis::LAG)
    } else {
        Vec::new()
    };
    let levels = app
        .player_bar_analyser
        .step(&samples, std::time::Instant::now());
    let painter = ui.painter().with_clip_rect(rect);
    let sides = app.settings.vis_bar_sides_value();
    let height = app.settings.player_bar_vis_bar_height_value();
    // The same bar count, gap, height, opacity and colours as the bars on
    // the normal screen.
    let count = app.settings.player_bar_vis_bar_count().max(2);
    let gap = app.settings.player_bar_vis_gap().min(8.0);
    // Every edge has bars of the same thickness whatever the window's
    // shape: the shorter edge gets the chosen count and the longer one as
    // many as fit, each edge reading the whole spectrum from end to end.
    let thickness = (rect.width().min(rect.height()) / count as f32).max(2.0);
    let across = ((rect.width() / thickness).round() as usize).max(2);
    let down = ((rect.height() / thickness).round() as usize).max(2);
    let reverse = app.settings.vis_reverse;
    let bands = |n: usize| {
        let mut merged = if n <= levels.len() {
            merge_bands(&levels, n)
        } else {
            spread_bands(&levels, n)
        };
        if reverse {
            merged.reverse();
        }
        merged
    };
    let merged_x = bands(across);
    let merged_y = bands(down);
    let step_x = rect.width() / across as f32;
    let step_y = rect.height() / down as f32;
    // Every edge's bars share one longest length: the room between the
    // window's left edge and the cover, less a margin, so no bar on any
    // side ever reaches the picture.
    let reach = if app.lyrics_fullscreen.is_some() {
        // Behind the lyrics the bars stay short, so they never climb onto
        // the words.
        (rect.width().min(rect.height()) * 0.14).clamp(30.0, 140.0)
    } else {
        (fullscreen_cover(rect, 0).left() - rect.left() - 16.0).max(10.0)
    };
    let boost = if app.settings.vis_shapes_value() == crate::settings::Settings::SHAPE_BARS {
        1.0
    } else {
        1.8
    };
    let opacity = (app.settings.vis_opacity(true) * boost).min(1.0);
    // The bars sweep through the gradient along each edge, or stay one
    // colour when the gradient is switched off.
    let (low, high) = vis_gradient(app, ui, crate::settings::PlayerBarVis::Spectrum);
    let colour_at = |t: f32| low.lerp_to_gamma(high, t.clamp(0.0, 1.0)).gamma_multiply(opacity);
    // Where two edges meet, the bars are cut along the corner's diagonal so
    // the two sets never draw over each other.
    // A little room is kept clear at every corner (the disc sits in the
    // top-left one), so the two sets part with a gap instead of touching.
    // The bars run right into the corner; the disc is drawn over them.
    let corner_cap = |_from_corner: f32, _cell: f32| f32::MAX;
    for (i, level) in merged_x.iter().enumerate() {
        let mut h = (soft_height(level * height) * reach).max(2.0);
        let x = rect.left() + i as f32 * step_x;
        let w = (step_x - gap).max(1.0);
        let colour = colour_at(i as f32 / (across - 1) as f32);
        let centre = x + w * 0.5;
        if sides & 2 != 0 {
            h = h.min(corner_cap(centre - rect.left(), step_x));
        }
        if sides & 8 != 0 {
            h = h.min(corner_cap(rect.right() - centre, step_x));
        }
        if sides & 1 != 0 {
            painter.rect_filled(Rect::from_min_size(pos2(x, rect.top()), vec2(w, h)), 0.0, colour);
        }
        if sides & 4 != 0 {
            painter.rect_filled(
                Rect::from_min_size(pos2(x, rect.bottom() - h), vec2(w, h)),
                0.0,
                colour,
            );
        }
    }
    for (i, level) in merged_y.iter().enumerate() {
        let mut w = (soft_height(level * height) * reach).max(2.0);
        let y = rect.top() + i as f32 * step_y;
        let thick = (step_y - gap).max(1.0);
        let colour = colour_at(i as f32 / (down - 1) as f32);
        let centre = y + thick * 0.5;
        if sides & 1 != 0 {
            w = w.min(corner_cap(centre - rect.top(), step_y));
        }
        if sides & 4 != 0 {
            w = w.min(corner_cap(rect.bottom() - centre, step_y));
        }
        if sides & 2 != 0 {
            painter.rect_filled(Rect::from_min_size(pos2(rect.left(), y), vec2(w, thick)), 0.0, colour);
        }
        if sides & 8 != 0 {
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.right() - w, y), vec2(w, thick)),
                0.0,
                colour,
            );
        }
    }
    true
}

/// A bar's height with a soft ceiling: a higher Height setting makes quiet
/// bars taller, but loud ones ease towards the top instead of all hitting
/// it and turning into a wall.
fn soft_height(value: f32) -> f32 {
    let value = value.max(0.0);
    if value <= 0.6 {
        value
    } else {
        0.6 + 0.4 * ((value - 0.6) / 0.4).tanh()
    }
}

/// The two cover colours washed over the swirl: the picked one, and the next.
/// What the swirl's playing-time sliders say.
#[derive(Clone, Copy)]
struct SwirlRun {
    bright: f32,
    wash: f32,
    bands: f32,
    /// Plain scrolling cover art instead of the waves.
    art: bool,
    /// With `art`: mirrored copies rather than one repeating cover.
    mirror: bool,
}

/// The swirl's own clock. It runs at the Swirl speed and speeds up with how
/// hard the music is playing, and it never runs backwards or jumps, so the
/// picture never resets.
fn swirl_phase(app: &mut App, ui: &egui::Ui, now: Option<&NowPlaying>) -> f64 {
    use crate::vis;
    let t = ui.input(|input| input.time);
    let id = egui::Id::new("swirl-phase");
    let (mut phase, last, mut smooth) = ui
        .ctx()
        .data(|data| data.get_temp::<(f64, f64, f32)>(id))
        .unwrap_or((0.0, t, 0.0));
    let dt = (t - last).clamp(0.0, 0.1);
    let sounding = now.is_some_and(|now| (now.playing || now.loading) && now.local);
    // What the swirl listens to: 0 the loudness, 1 the bass, 2 the beat
    // (a kick shoves it, quiet in between), 3 nothing.
    let mode = app.settings.swirl_react_mode.min(3);
    let energy = if sounding && mode != 3 {
        let samples = app.audio.tap.window(vis::FFT_SAMPLES, vis::LAG);
        if samples.is_empty() {
            0.0
        } else if mode == 1 {
            // A slow one-pole filter keeps the low notes and drops the rest.
            let mut low = 0.0_f32;
            let mut sum = 0.0_f32;
            for x in &samples {
                low += 0.03 * (x - low);
                sum += low * low;
            }
            (sum / samples.len() as f32).sqrt() * 3.0
        } else {
            (samples.iter().map(|x| x * x).sum::<f32>() / samples.len() as f32).sqrt()
        }
    } else {
        0.0
    };
    let target = if mode == 2 {
        // A beat is the moment the sound jumps above its own recent average.
        let avg_id = egui::Id::new("swirl-beat-average");
        let average = ui
            .ctx()
            .data(|data| data.get_temp::<f32>(avg_id))
            .unwrap_or(energy);
        let average = average + (energy - average) * 0.04;
        ui.ctx().data_mut(|data| data.insert_temp(avg_id, average));
        ((energy / average.max(0.02)) - 1.1).clamp(0.0, 1.0)
    } else {
        energy.min(1.0)
    };
    let rate = if mode == 2 && target > smooth { 0.45 } else if mode == 2 { 0.07 } else { 0.12 };
    smooth += (target - smooth) * rate;
    let speed = f64::from(app.settings.swirl_tune_value(8) / 100.0);
    let react = app.settings.swirl_tune_value(9) / 100.0;
    phase += dt * speed * (1.0 + f64::from(react * smooth * 8.0));
    ui.ctx().data_mut(|data| data.insert_temp(id, (phase, t, smooth)));
    phase
}

/// The two colours the visualizer's colour pair is at this moment (what the
/// bars and flow run between), for the lights feed.
#[allow(dead_code)]
pub(crate) fn lights_pair(app: &App, time: f64) -> Option<(Color32, Color32)> {
    swirl_wash_raw(app, time).map(|(a, b)| (within_brightness(a, true), within_brightness(b, true)))
}

/// The wash as the picture draws it: the cover's colours, brought down to
/// the chosen depth.
fn swirl_wash(app: &App, time: f64) -> Option<(Color32, Color32)> {
    let depth = app.settings.vis_depth;
    swirl_wash_raw(app, time).map(|(a, b)| (deepen(a, depth), deepen(b, depth)))
}

/// A colour darkened to a depth (0 leaves it alone), hue kept, so the
/// picture can sit as dark as the cover itself instead of washed to pastel.
pub(crate) fn deepen(colour: Color32, depth: u8) -> Color32 {
    if depth == 0 {
        return colour;
    }
    let look = &crate::settings::VIS_LOOKS[(depth as usize).min(crate::settings::VIS_LOOKS.len() - 1)];
    let mut hsva = egui::ecolor::Hsva::from(colour);
    // A grey cover stays grey: only its brightness moves.
    if hsva.s >= MONOCHROME_SATURATION {
        hsva.h = (hsva.h + look.hue).rem_euclid(1.0);
        hsva.s = (hsva.s * look.sat).max(look.sat_min).min(1.0);
    }
    hsva.v = (hsva.v * look.value).min(1.0);
    Color32::from(hsva)
}

fn swirl_wash_raw(app: &App, time: f64) -> Option<(Color32, Color32)> {
    let colours = app.swirl_colours();
    if colours.len() < 2 {
        return None;
    }
    if app.settings.swirl_colours_fixed {
        let pick = (app.settings.player_bar_vis_colour_pick as usize).min(colours.len() - 1);
        return Some((colours[pick], colours[(pick + 1) % colours.len()]));
    }
    // Drift through the cover's colours: every nine seconds the pair
    // eases to the next two.
    let n = colours.len();
    let t = (time / app.settings.swirl_drift_secs()) as f32;
    let index = t.floor() as usize;
    // Hold each colour for three quarters of the time and only blend near
    // the end, so the bar spends its time on the cover's own colours and
    // not on the muddy half-way mixes between them.
    let f = ((t.fract() - 0.75) / 0.25).clamp(0.0, 1.0);
    let f = f * f * (3.0 - 2.0 * f);
    let mix = |a: Color32, b: Color32| {
        Color32::from_rgb(
            (f32::from(a.r()) * (1.0 - f) + f32::from(b.r()) * f) as u8,
            (f32::from(a.g()) * (1.0 - f) + f32::from(b.g()) * f) as u8,
            (f32::from(a.b()) * (1.0 - f) + f32::from(b.b()) * f) as u8,
        )
    };
    Some((
        mix(colours[index % n], colours[(index + 1) % n]),
        mix(colours[(index + 1) % n], colours[(index + 2) % n]),
    ))
}

/// The two colours the bars and the flow run between, on the normal screen
/// and in the full-screen visualizer alike, so a colour change in the panel
/// moves both.
pub(super) fn vis_gradient(
    app: &mut App,
    ui: &egui::Ui,
    mode: crate::settings::PlayerBarVis,
) -> (Color32, Color32) {
    vis_gradient_ctx(app, ui.ctx(), ui.visuals().dark_mode, mode)
}

/// [`vis_gradient`] without a `Ui`, so the lights feed gets exactly the two
/// colours the bars are drawn between (the picked colour, the gradient
/// switch, the look) rather than its own idea of them.
pub(crate) fn vis_gradient_ctx(
    app: &mut App,
    ctx: &egui::Context,
    dark: bool,
    mode: crate::settings::PlayerBarVis,
) -> (Color32, Color32) {
    use crate::settings::PlayerBarVis;
    // The bars take the cover's own accent by default; a reader who wants
    // them to hold still can name a colour instead.
    let base = match app.settings.player_bar_vis_color {
        crate::settings::VisColor::AlbumArt => {
            // The cover's own vivid colour; asking is what starts the work.
            app.swirl_art(ctx);
            app.swirl_accent()
                .or_else(|| app.now_playing_cover_colour())
                .unwrap_or(app.palette.accent)
        }
        _ => app.settings.player_bar_vis_color_value(app.palette.accent),
    };
    // The flow follows the swirl's drifting colours when they are on Auto.
    let base = if matches!(mode, PlayerBarVis::Flow | PlayerBarVis::Spectrum)
        && !app.settings.swirl_colours_fixed
        && matches!(
            app.settings.player_bar_vis_color,
            crate::settings::VisColor::AlbumArt
        ) {
        swirl_wash_raw(app, ctx.input(|input| input.time)).map_or(base, |(first, _)| first)
    } else {
        base
    };
    // The cover's own colours, as extracted, for the two ends: no hue is
    // turned or invented. The gradient toggle chooses between the cover's
    // second colour and one flat colour.
    let gradient = !app.settings.vis_no_gradient;
    let album = matches!(
        app.settings.player_bar_vis_color,
        crate::settings::VisColor::AlbumArt
    );
    let (low, high) = if album {
        let time = ctx.input(|input| input.time);
        let second = if matches!(mode, PlayerBarVis::Flow | PlayerBarVis::Spectrum)
            && !app.settings.swirl_colours_fixed
        {
            swirl_wash_raw(app, time).map(|(_, second)| second)
        } else {
            None
        }
        .or_else(|| {
            let colours = app.swirl_colours();
            (colours.len() >= 2).then(|| colours[1])
        });
        let low = within_brightness(base, dark);
        match second {
            Some(second) if gradient => (low, within_brightness(second, dark)),
            Some(_) => (low, low),
            None if gradient => vis_colours(base, dark),
            None => (low, low),
        }
    } else if gradient {
        vis_colours(base, dark)
    } else {
        let (low, _) = vis_colours(base, dark);
        (low, low)
    };
    let depth = app.settings.vis_depth;
    (deepen(low, depth), deepen(high, depth))
}

pub(super) fn visualizer_shape(
    app: &mut App,
    ui: &egui::Ui,
    rect: Rect,
    now: Option<&NowPlaying>,
    mode: crate::settings::PlayerBarVis,
) -> bool {
    use crate::settings::PlayerBarVis;
    use crate::vis;
    // With flow or swirl under them, bars paint stronger so they stay
    // easy to see.
    let boost = if app.settings.vis_shapes_value() == crate::settings::Settings::SHAPE_BARS {
        1.0
    } else {
        1.8
    };
    let sounding = now.is_some_and(|now| (now.playing || now.loading) && now.local);
    let dark = ui.visuals().dark_mode;
    let (low, high) = vis_gradient(app, ui, mode);
    // Over a light bar the same colour paints more strongly, or white
    // would wash it out.
    let strength = if dark { 1.0 } else { LIGHT_STRENGTH };
    let strength = if mode == PlayerBarVis::Spectrum { strength * boost } else { strength };
    let strength = strength
        * app
            .settings
            .vis_opacity(mode == PlayerBarVis::Spectrum);
    let painter = ui.painter().with_clip_rect(rect);
    match mode {
        PlayerBarVis::Off => false,
        PlayerBarVis::Spectrum => {
            if !sounding && app.player_bar_analyser.settled() {
                return false;
            }
            let samples = if sounding {
                app.audio.tap.window(vis::WIDE_SAMPLES, vis::LAG)
            } else {
                Vec::new()
            };
            let levels = app
                .player_bar_analyser
                .step(&samples, std::time::Instant::now());
            let peaks = app.player_bar_analyser.peaks();
            let wanted = app.settings.player_bar_vis_bar_count();
            let gap = app.settings.player_bar_vis_gap();
            // The analyser works in its own fixed bands. Fewer bars is a
            // coarser reading of the same sound, not a different one, so
            // each bar takes the loudest band of its group. More bars than
            // it has bands are spread across them instead — merging would
            // silently hand back fewer bars than were asked for, which is
            // how a slider set to 200 came to draw 75.
            let (levels, peaks) = if wanted <= levels.len() {
                (merge_bands(&levels, wanted), merge_bands(&peaks, wanted))
            } else {
                (spread_bands(&levels, wanted), spread_bands(&peaks, wanted))
            };
            let height = app.settings.player_bar_vis_bar_height_value();
            let mut levels: Vec<f32> = levels.iter().map(|level| soft_height(level * height)).collect();
            let mut peaks: Vec<f32> = peaks.iter().map(|peak| soft_height(peak * height)).collect();
            if app.settings.vis_reverse {
                levels.reverse();
                peaks.reverse();
            }
            spectrum(&painter, rect, &levels, &peaks, (low, high), strength, gap);
            sounding || !app.player_bar_analyser.settled()
        }
        PlayerBarVis::Flow => {
            if !sounding && app.player_bar_analyser.settled() {
                return false;
            }
            // The sheets drift on a clock of their own, so they keep
            // moving while the sound is quiet and the levels are at rest.
            app.player_bar_vis_flow = (app.player_bar_vis_flow
                + FLOW_DRIFT * app.settings.player_bar_vis_flow_speed() / 60.0)
                .fract();
            let samples = if sounding {
                app.audio.tap.window(vis::WIDE_SAMPLES, vis::LAG)
            } else {
                Vec::new()
            };
            let levels = app
                .player_bar_analyser
                .step(&samples, std::time::Instant::now());
            // The sheets are far wider than the analyser reads, so the
            // bands are spread across the bar rather than merged down to
            // it: a flow that has only the analyser's own resolution looks
            // like a bar chart with the corners filed off.
            let mut flow = spread_bands(&levels, FLOW_POINTS);
            if app.settings.vis_reverse_flow {
                flow.reverse();
            }
            // Full screen, the other edges use the whole window.
            let flow_full = if app.fullscreen_vis { ui.ctx().content_rect() } else { rect };
            flow_sheets(
                &painter,
                rect,
                &flow,
                (low, high),
                strength,
                app.player_bar_vis_flow,
                FlowStyle {
                    sides: app.settings.vis_flow_sides_value(),
                    full: flow_full,
                    sheets: app.settings.player_bar_vis_flow_sheets() as usize,
                    // The lyrics page is the whole window, so the same depth
                    // is a lot taller there than in the bar: it is eased down.
                    // In the mini player the picture is the whole window, so a
                    // full-height flow would climb to the very top of it.
                    depth: app.settings.player_bar_vis_flow_depth()
                        * if app.lyrics_fullscreen.is_some() {
                            0.55
                        } else if app.mini_active {
                            0.5
                        } else {
                            1.0
                        },
                    offset: app.settings.player_bar_vis_flow_offset(),
                },
            );
            sounding || !app.player_bar_analyser.settled()
        }
        // The swirl is the picture behind the others, drawn by `visualizer`.
        PlayerBarVis::Swirl => false,
        PlayerBarVis::Waveform => {
            if !sounding {
                return false;
            }
            // Winamp's scope with a column every eight points or so.
            let count = (rect.width() / 8.0).clamp(75.0, 320.0) as usize;
            let samples = app.audio.tap.window(count * vis::SCOPE_STEP, vis::LAG);
            waveform(
                &painter,
                rect,
                &vis::scope_line(&samples, count),
                (low, high),
                strength,
            );
            true
        }
    }
}

/// What a right-click on the bars offers.
///
/// These are all things judged by looking at the bar, so they live on the
/// bar rather than in Settings, where the effect of a choice cannot be seen
/// without leaving the page.
/// The visualizer panel, opened from the button in the player bar.
///
/// Laid out shape-first: **which** shape it is comes before anything else,
/// then the settings that only mean something for the shape in force, then
/// the settings every shape shares, then the colour. It used to lead with
/// three lines about where the colour comes from and then a flat list of
/// four shapes and a flat list of sliders, which meant the reader had to
/// know the vocabulary before the menu would tell them anything.
fn vis_menu(app: &mut App, response: &egui::Response) {
    // Right-click opens the same panel as V, and right-clicking again
    // closes it, so there is only one menu.
    if response.secondary_clicked() {
        app.vis_panel = !app.vis_panel;
        // Remember where it was clicked, so the panel opens right there.
        if let Some(pos) = response.interact_pointer_pos() {
            response
                .ctx
                .data_mut(|data| data.insert_temp(egui::Id::new("vis-click-x"), pos.x));
        }
    }
}

/// The window's rectangle as a layer scaled by `fit` about `pivot` (the
/// transform `TSTransform::new(pivot * (1 - fit), fit)`) sees it: the area of
/// the layer's own coordinates that lands exactly on the screen.
pub(super) fn unscaled_screen(ctx: &egui::Context, pivot: egui::Pos2, fit: f32) -> egui::Rect {
    let screen = ctx.content_rect();
    let fit = fit.max(0.05);
    let shift = pivot.to_vec2() * (1.0 - fit);
    let back = |point: egui::Pos2| ((point.to_vec2() - shift) / fit).to_pos2();
    egui::Rect::from_min_max(back(screen.min), back(screen.max))
}

/// The visualizer settings window, opened with V.
pub(super) fn vis_panel_window(app: &mut App, ctx: &egui::Context) {
    if !app.vis_panel {
        return;
    }
    // The panel sits above the visualizer, never over it; fullscreen has no
    // bar to sit above, so there it floats over the picture.
    let screen_h = ctx
        .input(|input| input.raw.screen_rect)
        .map_or(900.0, |rect| rect.height());
    let floating = app.fullscreen_vis || app.lyrics_fullscreen.is_some() || app.mini_active;
    let above = if floating {
        14.0
    } else {
        ctx.data(|data| data.get_temp::<f32>(egui::Id::new("vis-top")))
            .map_or(120.0, |top| (screen_h - top + 8.0).max(60.0))
    };
    // Above the spot that was right-clicked; with no click on record (the V
    // key), the old place at the right.
    let clicked_x = ctx.data(|data| data.get_temp::<f32>(egui::Id::new("vis-click-x")));
    // The panel is as big as what is on its tab, and when the window is too
    // small for that it is drawn smaller, all of it, instead of being cut off.
    let screen = ctx.content_rect();
    // The mini player's panel is its own window, so what it measured there
    // (its smaller room) can never squeeze the one in the main window.
    // (v2 ids: the old windows' remembered sizes were saved to disk, and a
    // squashed one stayed squashed.)
    let win_id = egui::Id::new(if app.mini_active {
        "visualizer-panel-mini-v2"
    } else if floating {
        "visualizer-panel-full-v2"
    } else {
        "visualizer-panel-v2"
    });
    let default_x = match (floating, clicked_x) {
        (false, Some(x)) => x - 180.0,
        _ => screen.right() - (screen.width() * 0.5).min(560.0) - 14.0,
    };
    // One page: as wide as its three columns.
    // Fixed content width: three columns plus the scroll bar's lane. The
    // outer width (frame included) is measured, but only to keep the panel
    // inside the window; the size the panel is drawn at never depends on it.
    let natural_w = vis_panel_inner_w() + 34.0;
    let outer_id = egui::Id::new(("vis-panel-outer-w", app.mini_active));
    let outer_w = ctx
        .data(|data| data.get_temp::<f32>(outer_id))
        .filter(|w| (natural_w..=natural_w + 80.0).contains(w))
        .unwrap_or(natural_w + 24.0);
    let height_id = egui::Id::new(("vis-panel-natural-h", app.mini_active));
    let natural_h = ctx
        .data(|data| data.get_temp::<f32>(height_id))
        .map(|h| h.max(VIS_PANEL_MIN_H + 40.0))
        .unwrap_or(440.0);
    // Only the width decides the scale; the page scrolls if it is too tall.
    let room = (screen.width() - 16.0) / outer_w;
    // The reader's own size for the panel (the grip in its corner), never
    // bigger than the window has room for.
    let user = if app.settings.vis_panel_w > 0.0 { app.settings.vis_panel_w.clamp(0.4, 1.4) } else { 1.0 };
    let fit = (room.clamp(0.35, 1.0) * user).min(room.max(0.35)).clamp(0.3, 1.4);
    ctx.data_mut(|data| data.insert_temp(egui::Id::new("vis-panel-fit"), fit));
    // Where the reader left it (dragged by the strip along its top edge);
    // until then, the old place.
    let pos_id = egui::Id::new(("vis-panel-pos", win_id));
    let default_pos = egui::pos2(
        default_x.clamp(screen.left() + 8.0, (screen.right() - outer_w * fit - 8.0).max(screen.left() + 8.0)),
        (screen_h - above - natural_h * fit)
            .clamp(screen.top() + 8.0, (screen.bottom() - 8.0 - natural_h * fit).max(screen.top() + 8.0)),
    );
    let stored: Option<egui::Pos2> = ctx.data(|data| data.get_temp(pos_id));
    // Never off the window: the whole panel stays inside, whatever its size.
    let max_x = (screen.right() - outer_w * fit - 2.0).max(screen.left());
    let max_y = (screen.bottom() - natural_h * fit - 2.0).max(screen.top());
    let keep_inside = |at: egui::Pos2| egui::pos2(at.x.clamp(screen.left(), max_x), at.y.clamp(screen.top(), max_y));
    let fixed_pos = keep_inside(stored.unwrap_or(default_pos));
    let panel = egui::Window::new("visualizer-panel")
        .id(win_id)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .min_width(natural_w)
        .max_width(natural_w)
        .movable(false)
        .constrain(false)
        // The window paints inside this rectangle, in its own (unscaled)
        // coordinates. The default is the screen rectangle, which the scale
        // shrinks towards the panel's corner: the right and bottom of a
        // panel near the screen's edge were cut off. This is the screen
        // seen through the scale instead.
        .constrain_to(unscaled_screen(ctx, fixed_pos, fit))
        .fixed_pos(fixed_pos);
    let layer = egui::LayerId::new(egui::Order::Middle, win_id);
    ctx.set_transform_layer(
        layer,
        egui::emath::TSTransform::new(fixed_pos.to_vec2() * (1.0 - fit), fit),
    );
    let scaled = true;
    let window = panel.show(ctx, |ui| vis_menu_body(app, ui, true));
    let window = window.map(|window| {
        let local = window.response.rect;
        // (Never reset every window's state here: that hid the control bar
        // and the window buttons along with the panel.)
        ctx.data_mut(|data| {
            data.insert_temp(height_id, local.height().max(VIS_PANEL_MIN_H + 40.0));
            data.insert_temp(outer_id, local.width());
        });
        // One line a second in the trail, so a bad panel can be read from it.
        let line = format!(
            "vis panel local {:?} fit {fit:.2} screen {:?} pos {fixed_pos:?}",
            local, screen
        );
        let stamp = (ctx.input(|input| input.time) / 2.0) as i64;
        let last = ctx.data(|data| data.get_temp::<i64>(egui::Id::new("vis-panel-trail")));
        if last != Some(stamp) {
            ctx.data_mut(|data| data.insert_temp(egui::Id::new("vis-panel-trail"), stamp));
            crate::crash::trail(&line);
        }
        // Where it really is on the screen.
        let shown = if scaled {
            egui::Rect::from_min_size(local.min, local.size() * fit)
        } else {
            local
        };
        (window, shown)
    });
    // The panel stays until the X in its top right corner is clicked (or
    // its key or the right-click on the visualizer is used again): clicking
    // elsewhere leaves it where it is.
    if let Some((_window, shown)) = window {
        // A strip along the top edge moves the whole panel.
        let strip = egui::Rect::from_min_size(shown.min, egui::vec2((shown.width() - 36.0).max(20.0), 12.0));
        egui::Area::new(egui::Id::new("vis-panel-move"))
            .order(egui::Order::Foreground)
            .fixed_pos(strip.min)
            .show(ctx, |ui| {
                let (rect, response) = ui.allocate_exact_size(strip.size(), egui::Sense::drag());
                if response.hovered() || response.dragged() {
                    ui.ctx().set_cursor_icon(if response.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                }
                let colour = if response.hovered() || response.dragged() { app.palette.text } else { app.palette.dim };
                for dx in [-8.0, 0.0, 8.0] {
                    ui.painter().circle_filled(egui::pos2(rect.center().x + dx, rect.top() + 5.0), 1.2, colour);
                }
                if response.dragged() {
                    let moved = keep_inside(fixed_pos + response.drag_delta());
                    ui.ctx().data_mut(|data| data.insert_temp(pos_id, moved));
                }
                response.on_hover_text("Drag to move");
            });
        // A grip in the bottom right corner: drag to make the panel smaller or larger.
        let grip = egui::Rect::from_center_size(shown.right_bottom() - egui::vec2(10.0, 10.0), egui::vec2(18.0, 18.0));
        egui::Area::new(egui::Id::new("vis-panel-grip"))
            .order(egui::Order::Foreground)
            .fixed_pos(grip.min)
            .show(ctx, |ui| {
                let palette = app.palette;
                let (rect, response) = ui.allocate_exact_size(grip.size(), egui::Sense::click_and_drag());
                let hot = response.hovered() || response.dragged();
                if hot {
                    ui.ctx().set_cursor_icon(if response.dragged_by(egui::PointerButton::Secondary) {
                        egui::CursorIcon::ResizeVertical
                    } else {
                        egui::CursorIcon::ResizeNwSe
                    });
                }
                let colour = if hot { palette.text } else { palette.dim };
                for (dx, dy) in [(0.0, 0.0), (-4.0, 0.0), (0.0, -4.0), (-8.0, 0.0), (-4.0, -4.0), (0.0, -8.0)] {
                    ui.painter().circle_filled(egui::pos2(rect.center().x + dx + 4.0, rect.center().y + dy + 4.0), 1.0, colour);
                }
                if response.dragged_by(egui::PointerButton::Primary) {
                    let now_scale = if app.settings.vis_panel_w > 0.0 { app.settings.vis_panel_w } else { 1.0 };
                    let next = now_scale + response.drag_delta().x / natural_w.max(200.0);
                    app.settings.vis_panel_w = next.clamp(0.4, 1.4);
                    app.mark_settings_dirty();
                }
                if response.dragged_by(egui::PointerButton::Secondary) {
                    // Right-drag: the window's height, not its scale. Dragging
                    // up makes it taller (the bottom edge stays where it is,
                    // the top moves up with the pointer); down makes it shorter.
                    let most = ((screen_h - 110.0) / fit).max(VIS_PANEL_MIN_H);
                    let base = if app.settings.vis_panel_h > 0.0 {
                        app.settings.vis_panel_h
                    } else {
                        (natural_h - 40.0).max(VIS_PANEL_MIN_H)
                    };
                    let next = (base - response.drag_delta().y / fit).clamp(VIS_PANEL_MIN_H, most);
                    if (next - base).abs() > 0.01 {
                        app.settings.vis_panel_h = next;
                        let top = ui.ctx().data(|data| data.get_temp::<egui::Pos2>(pos_id)).unwrap_or(fixed_pos);
                        let moved = egui::pos2(top.x, top.y - (next - base) * fit);
                        ui.ctx().data_mut(|data| data.insert_temp(pos_id, keep_inside(moved)));
                        app.mark_settings_dirty();
                    }
                }
                if response.double_clicked() {
                    app.settings.vis_panel_w = 0.0;
                    app.settings.vis_panel_h = 0.0;
                    app.mark_settings_dirty();
                }
                response.on_hover_text("Drag to scale. Right-drag up or down for height. Double-click to reset.");
            });
        let size = 22.0;
        egui::Area::new(egui::Id::new("vis-panel-close"))
            .order(egui::Order::Foreground)
            .fixed_pos(shown.right_top() + egui::vec2(-size - 6.0, 6.0))
            .show(ctx, |ui| {
                let palette = app.palette;
                let (rect, response) = ui.allocate_exact_size(egui::Vec2::splat(size), egui::Sense::click());
                if response.hovered() {
                    ui.painter().circle_filled(rect.center(), size / 2.0, palette.surface_hover);
                }
                let colour = if response.hovered() { palette.text } else { palette.secondary };
                theme::paint_icon(ui, Icon::X, rect, 12.0, colour);
                if response.clicked() {
                    app.vis_panel = false;
                }
            });
    }
}

/// Puts the real equalizer back if a hover-preview was left behind, for
/// instance because the menu closed while the pointer was on a chip.
fn eq_preview_watchdog(app: &mut App, ctx: &egui::Context) {
    let flag = egui::Id::new("eq-preview");
    if !ctx.data(|data| data.get_temp::<bool>(flag)).unwrap_or(false) {
        return;
    }
    let seen = ctx
        .data(|data| data.get_temp::<f64>(egui::Id::new("eq-preview-at")))
        .unwrap_or(0.0);
    if ctx.input(|input| input.time) - seen > 0.3 {
        if let Ok(mut shared) = app.audio.eq.lock() {
            *shared = crate::app::eq_settings(&app.settings);
        }
        ctx.data_mut(|data| data.insert_temp(flag, false));
    }
}

/// Speed, scenes and the equalizer: three separate settings, opened by
/// right-clicking the volume.
fn audio_menu_body(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.spacing_mut().item_spacing.y = 3.0;
    theme::subtle(ui, &palette, &gettext(app.locale, "VOLUME BAR LENGTH"));
    slider_row(
        ui,
        &palette,
        &gettext(app.locale, "Length"),
        60.0..=600.0,
        volume_bar_width(app),
        |value| app.actions.push(Action::SetVolumeWidth(value.round())),
    );
    {
        {
            // The rate: a minus, the slider, a plus, then a plain box you can
            // type a number into. The slider edits a local and the action
            // carries the value, so the setting is never written mid-drag.
            theme::subtle(ui, &palette, &gettext(app.locale, "SPEED"));
            let mut speed = crate::speed::clamped(app.settings.playback_speed);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if ui.small_button("-").clicked() {
                    speed = crate::speed::clamped(((speed * 100.0).round() - 1.0) / 100.0);
                }
                ui.spacing_mut().slider_width = (ui.available_width() - 28.0 - 58.0 - 18.0).max(60.0);
                ui.add(
                    egui::Slider::new(
                        &mut speed,
                        crate::speed::MIN_SPEED..=crate::speed::MAX_SPEED,
                    )
                    .step_by(0.01)
                    .show_value(false),
                );
                if ui.small_button("+").clicked() {
                    speed = crate::speed::clamped(((speed * 100.0).round() + 1.0) / 100.0);
                }
                if let Some(text) = number_field(
                    ui,
                    egui::Id::new("speed-field"),
                    format!("{:.0}%", speed * 100.0),
                    52.0,
                    palette.accent,
                ) && let Ok(percent) = text.trim().trim_end_matches('%').trim().parse::<f32>()
                {
                    speed = crate::speed::clamped(percent / 100.0);
                }
            });
            if speed != crate::speed::clamped(app.settings.playback_speed) {
                app.actions.push(Action::SetSpeed(speed));
            }
            super::widgets::menu_separator(ui, &palette);
            // Scenes are off: the menu is the speed and the equalizer.
            if false {
            let scenes_collapse = egui::CollapsingHeader::new(gettext(app.locale, "Scenes")).default_open(true).show(ui, |ui| {
                // Built-in scenes (minus any removed) and the reader's own.
                // Right-click one to remove it.
                let current = crate::speed::index_of_speed(crate::speed::clamped(
                    app.settings.playback_speed,
                ));
                let scene_width = ((ui.available_width() - 12.0) / 3.0).floor().max(70.0);
                enum Scene {
                    Builtin(usize),
                    Own(usize),
                }
                let mut scenes: Vec<(String, Scene, bool)> = Vec::new();
                for (index, preset) in crate::speed::PRESETS.iter().enumerate() {
                    if app.settings.hidden_presets.iter().any(|name| name == preset.name) {
                        continue;
                    }
                    scenes.push((
                        gettext(app.locale, preset.name).to_string(),
                        Scene::Builtin(index),
                        index == current,
                    ));
                }
                for (index, (name, speed, bands)) in app.settings.custom_scenes.iter().enumerate() {
                    let active = (crate::speed::clamped(app.settings.playback_speed) - speed).abs() < 0.005
                        && *bands == app.settings.eq_bands_db;
                    scenes.push((name.clone(), Scene::Own(index), active));
                }
                for row in scenes.chunks(3) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 4.0);
                        for (label, kind, active) in row {
                            let response = chip(ui, &palette, label, *active, scene_width);
                            if response.clicked() {
                                match kind {
                                    Scene::Builtin(index) => {
                                        app.actions.push(Action::SetSpeedPreset(*index));
                                    }
                                    Scene::Own(index) => {
                                        app.actions.push(Action::ApplyCustomScene(*index));
                                    }
                                }
                            }
                            let info = match kind {
                                Scene::Builtin(index) => {
                                    let preset = &crate::speed::PRESETS[*index];
                                    let bands = preset
                                        .eq
                                        .and_then(|name| {
                                            crate::eq::PRESETS.iter().find(|eq| eq.name == name)
                                        })
                                        .map_or(app.settings.eq_bands_db, |eq| eq.bands_db);
                                    PresetInfo {
                                        key: preset.name.to_string(),
                                        speed: Some(preset.speed),
                                        bands,
                                    }
                                }
                                Scene::Own(index) => {
                                    let (name, speed, bands) = app.settings.custom_scenes[*index].clone();
                                    PresetInfo { key: name, speed: Some(speed), bands }
                                }
                            };
                            remove_menu(app, &response, &palette, info);
                        }
                    });
                }
                super::widgets::menu_separator(ui, &palette);
            });
            // Right-click the heading to keep the current speed and curve as a new scene.
            egui::Popup::context_menu(&scenes_collapse.header_response)
                .frame(super::widgets::menu_frame(&palette))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| {
                    ui.set_width(270.0);
                    theme::subtle(ui, &palette, &gettext(app.locale, "SAVE CURRENT SPEED AND CURVE AS A SCENE"));
                    save_row(ui, app, &palette, true);
                    share_rows(ui, app, &palette);
                });
            }
        }
    }
}

/// How wide one column of the visualizer menu is for a window this wide.
fn vis_col_w(_screen_w: f32) -> f32 {
    220.0
}

/// The least height the visualizer settings are ever shown at, so they can
/// never be reduced to a few pixels.
const VIS_PANEL_MIN_H: f32 = 340.0;

/// The room the settings page needs: the wider of the two rows. Bars and flow
/// side by side, or the swirl beside the bass jump.
fn vis_panel_inner_w() -> f32 {
    (2.0 * vis_col_w(0.0) + 18.0).max(vis_col_w(0.0) + 18.0 + VIS_SINGLE_W)
}

/// Width of the one column on the tabs that have only one.
const VIS_SINGLE_W: f32 = 272.0;


fn vis_menu_body(app: &mut App, ui: &mut egui::Ui, in_panel: bool) {
    if !in_panel {
        vis_menu_inner(app, ui);
        return;
    }
    let palette = app.palette;
    theme::subtle(ui, &palette, &gettext(app.locale, "SETTINGS"));
    let fit = ui
        .ctx()
        .data(|data| data.get_temp::<f32>(egui::Id::new("vis-panel-fit")))
        .unwrap_or(1.0)
        .max(0.3);
    // The page scrolls inside the window; the window itself is never taller
    // than the screen.
    let screen_h = ui.ctx().content_rect().height();
    let max_h = ((screen_h - 110.0) / fit).max(200.0);
    // The reader's own height (right-drag on the corner grip), if set.
    let own_h = app.settings.vis_panel_h;
    let (scroll_max, scroll_min) = if own_h.is_finite() && own_h > 0.0 {
        let h = own_h.clamp(VIS_PANEL_MIN_H.min(max_h), max_h);
        (h, h)
    } else {
        (max_h, VIS_PANEL_MIN_H.min(max_h))
    };
    egui::ScrollArea::vertical()
        .id_salt("vis-panel-scroll")
        .max_height(scroll_max)
        // Never squeezed to a sliver, whatever room the place it opened at
        // leaves: it keeps a usable height and the window is moved up to fit.
        .min_scrolled_height(scroll_min)
        .auto_shrink([false, true])
        .show(ui, |ui| vis_menu_inner(app, ui));
}

fn vis_menu_inner(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    {
        {
            ui.set_max_width(vis_panel_inner_w());
            ui.spacing_mut().item_spacing.y = 3.0;
            // Three independent switches: any mix, or none (which closes
            // the visualizer down to the song-length bar).
            // Two columns, or everything in one row that scrolls sideways.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (label, stacked) in [("2 columns", false), ("In a row", true)] {
                    if chip(ui, &palette, &gettext(app.locale, label), app.settings.vis_panel_stacked == stacked, 110.0).clicked() {
                        app.settings.vis_panel_stacked = stacked;
                        app.mark_settings_dirty();
                    }
                }
            });
            ui.add_space(4.0);
            let shapes = app.settings.vis_shapes_value();
            // Three columns side by side when the window is wide enough for
            // them, one under another when it is not, so the panel never
            // reaches past the window whatever its size.
            let screen_w = ui.ctx().content_rect().width();
            // Two columns from a modest width when the swirl's column is
            // not open, so the panel is shorter and fits one screen.
            let columns_fit = !app.settings.vis_panel_stacked;
            let col_w = vis_col_w(screen_w);
            // One page, top to bottom, with a bar between the parts: the
            // visualizer, bass and colour, then (in the full screen views) the
            // title and the lyrics, and the equalizer at the very end.
            // `tab` is the part being drawn right now.
            let view = app.vis_view_now.unwrap_or(0);
            let last_tab: u8 = match view {
                3 => 1,
                0 => 2,
                _ => 3,
            };
            let tab = std::cell::Cell::new(0u8);
            let mut columns = |app: &mut App, ui: &mut egui::Ui, which: u8| {
            ui.spacing_mut().item_spacing.x = 18.0;
            if which == 0 { ui.vertical(|ui| {
            ui.set_width(col_w);
            {
                let on = shapes & crate::settings::Settings::SHAPE_BARS != 0;
                if shape_button(ui, &palette, ShapeIcon::Bars, &gettext(app.locale, "Bars"), on, col_w).clicked() {
                    app.actions.push(Action::ToggleVisShape(crate::settings::Settings::SHAPE_BARS));
                }
                ui.add_space(2.0);
            }
            // Only the settings of the shapes that are on.
            crate::crash::stage("menu: bars column");
            if shapes & crate::settings::Settings::SHAPE_BARS != 0 {
                super::widgets::menu_separator(ui, &palette);
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Bar opacity"),
                    10.0..=100.0,
                    app.settings.vis_opacity(true) * 100.0,
                    |value| app.actions.push(Action::SetVisOpacity(true, value / 100.0)),
                );
                slider_row(
                ui,
                &palette,
                &gettext(app.locale, "Bar count"),
                crate::settings::Settings::VIS_BARS_MIN as f32
                    ..=crate::settings::Settings::VIS_BARS_MAX as f32,
                app.settings.player_bar_vis_bars as f32,
                |value| app.actions.push(Action::SetVisBars(value.round() as u16)),
            );
            slider_row(
                ui,
                &palette,
                &gettext(app.locale, "Bar height"),
                0.5..=3.0,
                app.settings.player_bar_vis_bar_height_value(),
                |value| app.actions.push(Action::SetVisBarHeight(value)),
            );
            slider_row(
                ui,
                &palette,
                &gettext(app.locale, "Bar gap"),
                0.0..=crate::settings::Settings::VIS_GAP_MAX,
                app.settings.player_bar_vis_gap(),
                |value| app.actions.push(Action::SetVisGap(value)),
            );
                if app.fullscreen_vis || app.mini_active {
                    // Fullscreen bars hang from the edges you pick.
                    let sides = app.settings.vis_bar_sides_value();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for (bit, label) in [(1u8, "Top"), (2, "Left"), (4, "Bottom"), (8, "Right")] {
                            if chip(ui, &palette, &gettext(app.locale, label), sides & bit != 0, 66.0)
                                .clicked()
                            {
                                app.actions.push(Action::ToggleBarSide(bit));
                            }
                        }
                    });
                }
                if chip(
                    ui,
                    &palette,
                    if app.settings.vis_reverse { "Scrolls right to left" } else { "Scrolls left to right" },
                    app.settings.vis_reverse,
                    ui.available_width().min(290.0),
                )
                .clicked()
                {
                    app.actions.push(Action::ToggleVisReverseOne(0));
                }
                // Ready-made looks: opacity, count, height, gap.
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (name, opacity, count, height, gap) in [
                        ("Fine", 50.0f32, 1024u16, 0.8f32, 0.0f32),
                        ("Classic", 100.0, 128, 1.0, 2.0),
                        ("Chunky", 100.0, 48, 1.6, 4.0),
                        ("Tall", 90.0, 96, 2.6, 1.0),
                    ] {
                        if chip(ui, &palette, &gettext(app.locale, name), false, 62.0).clicked() {
                            app.actions.push(Action::SetVisOpacity(true, opacity / 100.0));
                            app.actions.push(Action::SetVisBars(count));
                            app.actions.push(Action::SetVisBarHeight(height));
                            app.actions.push(Action::SetVisGap(gap));
                        }
                    }
                });
            }
            }); }
            if which == 0 { ui.vertical(|ui| {
            ui.set_width(col_w);
            {
                let on = shapes & crate::settings::Settings::SHAPE_FLOW != 0;
                if shape_button(ui, &palette, ShapeIcon::Flow, &gettext(app.locale, "Flow"), on, col_w).clicked() {
                    app.actions.push(Action::ToggleVisShape(crate::settings::Settings::SHAPE_FLOW));
                }
                ui.add_space(2.0);
            }
            if shapes & crate::settings::Settings::SHAPE_FLOW != 0 {
                super::widgets::menu_separator(ui, &palette);
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Flow opacity"),
                    10.0..=100.0,
                    app.settings.vis_opacity(false) * 100.0,
                    |value| app.actions.push(Action::SetVisOpacity(false, value / 100.0)),
                );
                // The flow has settings of its own, and none of the bars':
                // its sheets are folded out of a fixed number of readings,
                // so a bar count would not change what it draws.
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Flow sheets"),
                    *crate::settings::Settings::VIS_FLOW_SHEETS.start() as f32
                        ..=*crate::settings::Settings::VIS_FLOW_SHEETS.end() as f32,
                    app.settings.player_bar_vis_flow_sheets() as f32,
                    |value| {
                        app.actions.push(Action::SetFlowSheets(value.round() as u8));
                    },
                );
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Flow height"),
                    *crate::settings::Settings::VIS_FLOW_DEPTH.start()
                        ..=*crate::settings::Settings::VIS_FLOW_DEPTH.end(),
                    app.settings.player_bar_vis_flow_depth(),
                    |value| app.actions.push(Action::SetFlowDepth(value)),
                );
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Flow speed"),
                    *crate::settings::Settings::VIS_FLOW_SPEED.start()
                        ..=*crate::settings::Settings::VIS_FLOW_SPEED.end(),
                    app.settings.player_bar_vis_flow_speed(),
                    |value| app.actions.push(Action::SetFlowSpeed(value)),
                );
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Flow offset"),
                    *crate::settings::Settings::VIS_FLOW_OFFSET.start()
                        ..=*crate::settings::Settings::VIS_FLOW_OFFSET.end(),
                    app.settings.player_bar_vis_flow_offset(),
                    |value| app.actions.push(Action::SetFlowOffset(value)),
                );
                // Which edges the flow rises from.
                if app.fullscreen_vis || app.mini_active {
                    let sides = app.settings.vis_flow_sides_value();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for (bit, label) in [(1u8, "Top"), (2, "Left"), (4, "Bottom"), (8, "Right")] {
                            if chip(ui, &palette, &gettext(app.locale, label), sides & bit != 0, 48.0).clicked() {
                                app.actions.push(Action::ToggleFlowSide(bit));
                            }
                        }
                    });
                }
                if chip(
                    ui,
                    &palette,
                    if app.settings.vis_reverse_flow { "Scrolls right to left" } else { "Scrolls left to right" },
                    app.settings.vis_reverse_flow,
                    ui.available_width().min(290.0),
                )
                .clicked()
                {
                    app.actions.push(Action::ToggleVisReverseOne(1));
                }
                // Ready-made looks: sheets, height, opacity, speed.
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (name, sheets, depth, opacity, speed) in [
                        ("Calm", 3u8, 0.5f32, 70.0f32, 0.6f32),
                        ("Rolling", 5, 0.86, 100.0, 1.0),
                        ("Dense", 10, 0.7, 85.0, 1.6),
                        ("Storm", 8, 1.1, 100.0, 3.5),
                    ] {
                        if chip(ui, &palette, &gettext(app.locale, name), false, 62.0).clicked() {
                            app.actions.push(Action::SetFlowSheets(sheets));
                            app.actions.push(Action::SetFlowDepth(depth));
                            app.actions.push(Action::SetVisOpacity(false, opacity / 100.0));
                            app.actions.push(Action::SetFlowSpeed(speed));
                        }
                    }
                });

            }
            }); }
            if which == 1 { ui.vertical(|ui| {
            ui.set_width(col_w);
            {
                let on = shapes & crate::settings::Settings::SHAPE_SWIRL != 0;
                if shape_button(ui, &palette, ShapeIcon::Swirl, &gettext(app.locale, "Swirl"), on, col_w).clicked() {
                    app.actions.push(Action::ToggleVisShape(crate::settings::Settings::SHAPE_SWIRL));
                }
                ui.add_space(2.0);
            }
            if shapes & crate::settings::Settings::SHAPE_SWIRL != 0 {
                crate::crash::stage("menu: swirl column");
                super::widgets::menu_separator(ui, &palette);
                let art_scroll = app.settings.swirl_art_scroll;
                for (index, (label, low, high, _)) in
                    crate::settings::Settings::SWIRL_TUNE.iter().enumerate()
                {
                    // Scrolling art only has a speed; the ripple, blur and
                    // colour sliders are for the waves.
                    // Only the two speeds are left; the ripples, blur and
                    // colour are what the looks above are for.
                    if index != 8 && index != 9 {
                        continue;
                    }
                    slider_row(
                        ui,
                        &palette,
                        &gettext(app.locale, label),
                        *low..=*high,
                        app.settings.swirl_tune_value(index),
                        |value| app.actions.push(Action::SetSwirlTune(index, value)),
                    );
                }
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Colour drift"),
                    0.2..=4.0,
                    (9.0 / app.settings.swirl_drift_secs()) as f32,
                    |value| app.actions.push(Action::SetSwirlDrift(value)),
                );
                // One-click speed, music boost and colour drift together.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let speed_now = app.settings.swirl_tune_value(8);
                    let boost_now = app.settings.swirl_tune_value(9);
                    let drift_now = (9.0 / app.settings.swirl_drift_secs()) as f32;
                    let width = ((col_w - 12.0) / 4.0).max(40.0);
                    for (label, speed, boost, drift) in [
                        ("Slow", 30.0f32, 30.0f32, 0.6f32),
                        ("Calm", 60.0, 60.0, 1.2),
                        ("Lively", 130.0, 140.0, 2.6),
                        ("Fast", 220.0, 220.0, 4.0),
                    ] {
                        let active = (speed_now - speed).abs() < 1.0
                            && (boost_now - boost).abs() < 1.0
                            && (drift_now - drift).abs() < 0.05;
                        if chip(ui, &palette, &gettext(app.locale, label), active, width).clicked() {
                            app.actions.push(Action::SetSwirlTune(8, speed));
                            app.actions.push(Action::SetSwirlTune(9, boost));
                            app.actions.push(Action::SetSwirlDrift(drift));
                        }
                    }
                });
                if art_scroll {
                    slider_row(
                        ui,
                        &palette,
                        &gettext(app.locale, "Soft edges between covers"),
                        0.0..=100.0,
                        app.settings.swirl_art_edge_value(),
                        |value| app.actions.push(Action::SetSwirlArtEdge(value)),
                    );
                }
                // Pictures, no words: what the swirl is made of, then what it moves with.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if glyph_button(ui, &palette, Glyph::Scroll, "Scrolling album art instead of waves", art_scroll).clicked() {
                        app.actions.push(Action::ToggleSwirlArtScroll);
                    }
                    if art_scroll
                        && glyph_button(
                            ui,
                            &palette,
                            Glyph::Mirror,
                            "Mirrored covers (off: one cover, not mirrored)",
                            !app.settings.swirl_art_single,
                        )
                        .clicked()
                    {
                        app.actions.push(Action::ToggleSwirlArtSingle);
                    }
                });
                // Only the default view has a library, queue and top bar to go behind.
                if app.vis_view_now.unwrap_or(0) == 0 {
                    let behind = app.settings.swirl_behind_panels;
                    if chip(ui, &palette, "Behind the side panels and top bar", behind, ui.available_width().min(290.0)).clicked() {
                        app.settings.swirl_behind_panels = !behind;
                        app.mark_settings_dirty();
                    }
                }
                theme::subtle(ui, &palette, &gettext(app.locale, "MOVES WITH"));
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (value, glyph, tip) in [
                        (0u8, Glyph::Icon(Icon::Volume2), "Loudness"),
                        (1, Glyph::Icon(Icon::Speaker), "Bass"),
                        (2, Glyph::Icon(Icon::Zap), "Beat"),
                        (3, Glyph::Nothing, "Nothing"),
                    ] {
                        if glyph_button(ui, &palette, glyph, tip, app.settings.swirl_react_mode.min(3) == value).clicked() {
                            app.actions.push(Action::SetSwirlReact(value));
                        }
                    }
                });
                // The swirl's own direction; the bars and the flow keep theirs.
                if chip(
                    ui,
                    &palette,
                    if app.settings.vis_reverse_swirl { "Scrolls right to left" } else { "Scrolls left to right" },
                    app.settings.vis_reverse_swirl,
                    ui.available_width().min(290.0),
                )
                .clicked()
                {
                    app.actions.push(Action::ToggleVisReverseOne(2));
                }
                if !art_scroll {
                    let active = (0..crate::settings::Settings::SWIRL_PRESETS.len())
                        .find(|index| app.settings.swirl_preset_active(*index));
                    let names: Vec<&str> = crate::settings::Settings::SWIRL_PRESETS
                        .iter()
                        .map(|(name, _)| *name)
                        .collect();
                    // Six looks for each family, numbered; no words.
                    if let Some(index) = numbered_rows(
                        ui,
                        &palette,
                        &gettext(app.locale, "LOOK"),
                        &[(Icon::Disc, 0..6), (Icon::Sparkles, 6..12), (Icon::Waves, 12..18)],
                        &names,
                        active,
                    ) {
                        app.actions.push(Action::SetSwirlPreset(index));
                    }
                }
            }
            }); }
            };
            // Two columns across the top (bars, flow); the swirl goes below,
            // beside the bass jump, so the panel is a third narrower.
            if columns_fit {
                ui.horizontal_top(|ui| columns(&mut *app, ui, 0));
            }
            // The settings every shape shares, then the title, then the lyrics.
            let mut shared = |app: &mut App, ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing.x = 18.0;
            if tab.get() == 1 { ui.vertical(|ui| {
            ui.set_width(VIS_SINGLE_W);
            theme::subtle(ui, &palette, &gettext(app.locale, "BASS JUMP"));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if chip(ui, &palette, &gettext(app.locale, "On"), app.settings.vis_jump_on, 60.0).clicked() {
                    app.actions.push(Action::ToggleJumpAuto);
                }
            });
            // Ready-made feels for the reaction picked below: strength,
            // sensitivity, rest, cutoff, follow.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let react = app.settings.vis_jump_react.min(3);
                let sets: [(&str, f32, f32, f32, f32, f32); 4] = match react {
                    0 => [
                        ("Gentle", 0.4, 0.8, 50.0, 5.0, 5.0),
                        ("Swell", 0.8, 1.2, 40.0, 4.0, 10.0),
                        ("Surge", 1.4, 1.6, 28.0, 2.0, 8.0),
                        ("Crash", 2.2, 2.2, 18.0, 1.0, 14.0),
                    ],
                    1 => [
                        ("Soft", 0.4, 0.9, 45.0, 4.0, 5.0),
                        ("Steady", 0.5, 1.0, 32.0, 3.0, 5.0),
                        ("Thump", 1.2, 1.4, 24.0, 2.0, 8.0),
                        ("Wild", 2.2, 2.0, 14.0, 0.0, 15.0),
                    ],
                    2 => [
                        ("Light", 0.4, 0.9, 40.0, 4.0, 8.0),
                        ("Groove", 0.8, 1.1, 30.0, 3.0, 10.0),
                        ("Snap", 1.4, 1.5, 22.0, 2.0, 12.0),
                        ("Frantic", 2.2, 2.1, 12.0, 0.0, 18.0),
                    ],
                    _ => [
                        ("Faint", 0.3, 1.0, 30.0, 0.0, 5.0),
                        ("Pulse", 0.6, 1.0, 30.0, 0.0, 5.0),
                        ("Pound", 1.2, 1.0, 30.0, 0.0, 5.0),
                        ("Slam", 2.2, 1.0, 30.0, 0.0, 5.0),
                    ],
                };
                for (name, shake, sens, gap, floor, adapt) in sets {
                    if chip(ui, &palette, &gettext(app.locale, name), false, 56.0).clicked() {
                        app.actions.push(Action::SetJumpShake(shake));
                        app.actions.push(Action::SetJumpTune(0, sens));
                        app.actions.push(Action::SetJumpTune(1, gap));
                        app.actions.push(Action::SetJumpTune(2, floor));
                        app.actions.push(Action::SetJumpTune(3, adapt));
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (value, glyph, tip) in [
                    (0u8, Glyph::Icon(Icon::Volume2), "Jumps with the loudness"),
                    (1, Glyph::Icon(Icon::Speaker), "Jumps with the bass"),
                    (2, Glyph::Icon(Icon::Zap), "Jumps on the beat"),
                    (3, Glyph::Icon(Icon::Music), "Jumps on the song's tempo (BPM)"),
                ] {
                    if glyph_button(ui, &palette, glyph, tip, app.settings.vis_jump_react.min(3) == value).clicked() {
                        app.actions.push(Action::SetJumpReact(value));
                    }
                }
            });
            slider_row(
                ui,
                &palette,
                &gettext(app.locale, "Jump strength"),
                0.2..=3.0,
                app.settings.jump_shake,
                |value| {
                    app.actions.push(Action::SetJumpShake(value));
                },
            );
            // How the jump reacts, so a finicky song can be tuned by hand.
            slider_row(ui, &palette, &gettext(app.locale, "Sensitivity"), 40.0..=250.0,
                app.settings.jump_sens * 100.0,
                |value| app.actions.push(Action::SetJumpTune(0, value / 100.0)));
            slider_row(ui, &palette, &gettext(app.locale, "Rest between"), 10.0..=100.0,
                app.settings.jump_gap,
                |value| app.actions.push(Action::SetJumpTune(1, value)));
            slider_row(ui, &palette, &gettext(app.locale, "Quiet cutoff"), 0.0..=20.0,
                app.settings.jump_floor,
                |value| app.actions.push(Action::SetJumpTune(2, value)));
            slider_row(ui, &palette, &gettext(app.locale, "Follows the song"), 1.0..=30.0,
                app.settings.jump_adapt,
                |value| app.actions.push(Action::SetJumpTune(3, value)));
            ui.add_space(6.0);
            theme::subtle(ui, &palette, &gettext(app.locale, "COLOUR"));
            // How dark the colours run, for all three shapes: Bright is the
            // cover lifted to full colour, Deep and Night keep it nearer
            // the cover's own darkness.
            // Each look is a small swatch of this very cover's colours as
            // that look would draw them; the name is on hover.
            let sample = {
                let colours = app.swirl_colours();
                let first = colours.first().copied().unwrap_or(palette.accent);
                let second = colours.get(1).copied().unwrap_or(first);
                (first, second)
            };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for (value, look) in crate::settings::VIS_LOOKS.iter().enumerate() {
                    let (rect, response) = ui.allocate_exact_size(vec2(40.0, 28.0), Sense::click());
                    let (a, b) = (deepen(sample.0, value as u8), deepen(sample.1, value as u8));
                    let half = rect.width() / 2.0;
                    let left = Rect::from_min_size(rect.min, vec2(half, rect.height()));
                    let right = Rect::from_min_size(pos2(rect.left() + half, rect.top()), vec2(half, rect.height()));
                    ui.painter().rect_filled(
                        left,
                        egui::CornerRadius { nw: 7, sw: 7, ne: 0, se: 0 },
                        a,
                    );
                    ui.painter().rect_filled(
                        right,
                        egui::CornerRadius { nw: 0, sw: 0, ne: 7, se: 7 },
                        b,
                    );
                    let active = app.settings.vis_depth as usize == value;
                    if active {
                        ui.painter().rect_stroke(
                            rect.expand(2.0),
                            egui::CornerRadius::same(9),
                            egui::Stroke::new(2.0, palette.text),
                            egui::StrokeKind::Outside,
                        );
                    } else if response.hovered() {
                        ui.painter().rect_stroke(
                            rect.expand(1.0),
                            egui::CornerRadius::same(8),
                            egui::Stroke::new(1.0, palette.secondary),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.on_hover_text(gettext(app.locale, look.name).into_owned()).clicked() {
                        app.actions.push(Action::SetVisDepth(value as u8));
                    }
                }
            });
            if chip(
                ui,
                &palette,
                &gettext(app.locale, "Gradient to the cover's second colour"),
                !app.settings.vis_no_gradient,
                ui.available_width().min(290.0),
            )
            .clicked()
            {
                app.actions.push(Action::ToggleVisGradient);
            }
            // The bars always take the cover's colours; this picks which one.
            ui.add_space(4.0);
            let colours = app.swirl_colours();
            if colours.len() > 1 {
                ui.horizontal(|ui| {
                    ui.label(gettext(app.locale, "Colour"));
                    let picked = app.settings.player_bar_vis_colour_pick as usize;
                    let fixed = app.settings.swirl_colours_fixed;
                    // The last swatch is every colour in turn: the swirl
                    // drifts through them on its own.
                    let (rect, response) = ui.allocate_exact_size(vec2(34.0, 20.0), Sense::click());
                    let part = rect.width() / colours.len() as f32;
                    for (index, colour) in colours.iter().enumerate() {
                        let piece = Rect::from_min_size(
                            pos2(rect.left() + part * index as f32, rect.top()),
                            vec2(part + 0.5, rect.height()),
                        );
                        let round = egui::CornerRadius {
                            nw: if index == 0 { 5 } else { 0 },
                            sw: if index == 0 { 5 } else { 0 },
                            ne: if index + 1 == colours.len() { 5 } else { 0 },
                            se: if index + 1 == colours.len() { 5 } else { 0 },
                        };
                        ui.painter().rect_filled(piece, round, *colour);
                    }
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Auto",
                        theme::bold(theme::TINY_TEXT),
                        Color32::WHITE,
                    );
                    if !fixed {
                        ui.painter().rect_stroke(
                            rect.expand(2.0),
                            egui::CornerRadius::same(6),
                            egui::Stroke::new(2.0, palette.text),
                            egui::StrokeKind::Outside,
                        );
                    }
                    if response.clicked() {
                        app.actions.push(Action::SetSwirlColoursFixed(false));
                    }
                    for (index, colour) in colours.iter().enumerate() {
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(26.0, 20.0), Sense::click());
                        ui.painter()
                            .rect_filled(rect, egui::CornerRadius::same(5), *colour);
                        if index == picked && fixed {
                            ui.painter().rect_stroke(
                                rect.expand(2.0),
                                egui::CornerRadius::same(6),
                                egui::Stroke::new(2.0, palette.text),
                                egui::StrokeKind::Outside,
                            );
                        }
                        if response.clicked() {
                            app.actions.push(Action::SetVisColourPick(index as u8));
                            app.actions.push(Action::SetSwirlColoursFixed(true));
                        }
                    }
                });
            }
            }); }
            if tab.get() == 2 { ui.horizontal_top(|ui| { ui.vertical(|ui| {
            ui.set_width(VIS_SINGLE_W);
            theme::subtle(ui, &palette, &gettext(app.locale, "TITLE"));
            // Ready-made feels for the vibration and the art border.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // name, still, react, sens, strength, border
                for (name, still, react, sens, strength, border) in [
                    ("Still", true, 0u8, 1.0f32, 1.0f32, false),
                    ("Soft", false, 0, 0.8, 0.5, false),
                    ("Lively", false, 1, 1.3, 1.2, true),
                    ("Heavy", false, 2, 1.8, 2.2, true),
                ] {
                    if chip(ui, &palette, &gettext(app.locale, name), false, 52.0).clicked() {
                        app.settings.vis_text_still = still;
                        app.settings.vis_sway_react = react;
                        app.settings.vis_sway_beat = react == 2;
                        app.settings.sway_sens = sens;
                        app.settings.vis_sway_amount = strength;
                        app.settings.vis_art_border = border;
                        app.mark_settings_dirty();
                    }
                }
            });
            if chip_i(
                ui,
                &palette,
                "Title vibration",
                !app.settings.vis_text_still,
                ui.available_width().min(290.0),
            )
            .clicked()
            {
                app.actions.push(Action::ToggleVisTextSway);
            }
            if chip_i(
                ui,
                &palette,
                "Art border",
                app.settings.vis_art_border,
                ui.available_width().min(290.0),
            )
            .clicked()
            {
                app.settings.vis_art_border = !app.settings.vis_art_border;
                app.mark_settings_dirty();
            }
            {
                theme::subtle(ui, &palette, &gettext(app.locale, "VIBRATION REACTS TO THE MUSIC"));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if chip(ui, &palette, &gettext(app.locale, "On"), app.settings.vis_sway_beat, 60.0).clicked() {
                        app.actions.push(Action::ToggleVisSwayBeat);
                    }
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (value, glyph, tip) in [
                        (0u8, Glyph::Icon(Icon::Volume2), "Vibrates with the loudness"),
                        (1, Glyph::Icon(Icon::Speaker), "Vibrates with the bass"),
                        (2, Glyph::Icon(Icon::Zap), "Vibrates on the beat"),
                    ] {
                        if glyph_button(ui, &palette, glyph, tip, app.settings.vis_sway_react.min(2) == value).clicked() {
                            app.actions.push(Action::SetSwayReact(value));
                        }
                    }
                });
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Sensitivity"),
                    40.0..=250.0,
                    app.settings.sway_sens * 100.0,
                    |value| app.actions.push(Action::SetSwaySens(value / 100.0)),
                );
                slider_row(
                    ui,
                    &palette,
                    &gettext(app.locale, "Vibration strength"),
                    20.0..=300.0,
                    app.settings.vis_sway_strength() * 100.0,
                    |value| app.actions.push(Action::SetVisSwayAmount(value / 100.0)),
                );
            }
            }); }); }
            let on_page = app.lyrics_fullscreen.is_some();
            if tab.get() == 3 { ui.horizontal_top(|ui| { ui.vertical(|ui| {
            ui.set_width(VIS_SINGLE_W);
            theme::subtle(ui, &palette, &gettext(app.locale, "FULL-SCREEN LYRICS"));
            // These two belong to the lyrics drawn over the visualizer, so the
            // lyrics page itself does not show them.
            if !on_page {
                if chip_i(
                    ui,
                    &palette,
                    "Show lyrics over it",
                    app.settings.vis_lyrics,
                    ui.available_width().min(290.0),
                )
                .clicked()
                {
                    app.actions.push(Action::ToggleVisLyrics);
                }
                if chip_i(
                    ui,
                    &palette,
                    "Dark background behind the lyrics",
                    !app.settings.vis_lyrics_no_back,
                    ui.available_width().min(290.0),
                )
                .clicked()
                {
                    app.actions.push(Action::ToggleVisLyricsBack);
                }
            }
            if !(on_page || app.settings.vis_lyrics) {
                use crate::settings::Settings;
                let on = app.settings.lyrics_flag(Settings::LYRICS_BOUNCE_ART);
                if chip_i(ui, &palette, "Art bounces to the music", on, ui.available_width().min(290.0)).clicked() {
                    app.actions.push(Action::ToggleLyricsFlag(Settings::LYRICS_BOUNCE_ART));
                }
                let floats = app.settings.lyrics_flag(Settings::LYRICS_FLOAT_ART);
                if chip_i(ui, &palette, "Floating art", floats, ui.available_width().min(290.0)).clicked() {
                    app.actions.push(Action::ToggleLyricsFlag(Settings::LYRICS_FLOAT_ART));
                }
            }
            // The lyrics options, one set for the page and for the lyrics over
            // the visualizer.
            if on_page || app.settings.vis_lyrics {
                use crate::settings::Settings;
                for (bit, label, inverted) in [
                    (Settings::LYRICS_HIDE_ART, "Album art", true),
                    (Settings::LYRICS_FLOAT_ART, "Floating art", false),
                    (Settings::LYRICS_BOUNCE_ART, "Art bounces to the music", false),
                    (Settings::LYRICS_HIDE_STAMPS, "Timestamps", true),
                    (Settings::LYRICS_COUNTDOWN, "Countdown", false),
                    (Settings::LYRICS_HIDE_ARTIST, "Artist name", true),
                ] {
                    // The art options only mean something on the page.
                    if !on_page && matches!(bit, Settings::LYRICS_HIDE_ART | Settings::LYRICS_HIDE_ARTIST) {
                        continue;
                    }
                    let on = app.settings.lyrics_flag(bit) != inverted;
                    if chip_i(ui, &palette, label, on, ui.available_width().min(290.0)).clicked() {
                        app.actions.push(Action::ToggleLyricsFlag(bit));
                    }
                }
            }
            }); ui.vertical(|ui| {
            ui.set_width(VIS_SINGLE_W);
            if on_page || app.settings.vis_lyrics {
                let right = app.settings.lyrics_right();
                for (value, label) in [(0u8, "Left"), (2, "Right")] {
                    if chip_i(ui, &palette, label, (value == 2) == right, ui.available_width().min(290.0)).clicked() {
                        app.actions.push(Action::SetLyricsAlign(value));
                    }
                }
                let focus = app.settings.lyrics_focus();
                if chip_i(ui, &palette, "Focus on the current line", focus, ui.available_width().min(290.0)).clicked() {
                    app.actions.push(Action::ToggleLyricsFlag(crate::settings::Settings::LYRICS_FOCUS));
                }
            }
            if app.extra_vis {
                ui.add_space(6.0);
                theme::subtle(ui, &palette, "NEW WINDOW LYRICS");
                for (label, on, which) in [
                    ("Song name", app.settings.extra_show_title, 0u8),
                    ("Artist", app.settings.extra_show_artist, 1),
                    ("Heart", app.settings.extra_show_heart, 2),
                ] {
                    if chip_i(ui, &palette, label, on, ui.available_width().min(290.0)).clicked() {
                        match which {
                            0 => app.settings.extra_show_title = !on,
                            1 => app.settings.extra_show_artist = !on,
                            _ => app.settings.extra_show_heart = !on,
                        }
                        app.mark_settings_dirty();
                    }
                }
            }
            }); }); }
            };
            if columns_fit {
                for part in 1..=last_tab {
                    super::widgets::menu_separator(ui, &palette);
                    tab.set(part);
                    if part == 1 {
                        ui.horizontal_top(|ui| {
                            columns(&mut *app, ui, 1);
                            shared(&mut *app, ui);
                        });
                    } else {
                        ui.horizontal_top(|ui| shared(&mut *app, ui));
                    }
                }
            } else {
                // Every part in one row, with a bar to scroll sideways.
                egui::ScrollArea::horizontal()
                    .id_salt("vis-panel-row")
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal_top(|ui| {
                            columns(&mut *app, ui, 0);
                            for part in 1..=last_tab {
                                tab.set(part);
                                if part == 1 {
                                    columns(&mut *app, ui, 1);
                                }
                                shared(&mut *app, ui);
                            }
                        });
                    });
            }
            super::widgets::menu_separator(ui, &palette);
            theme::subtle(ui, &palette, &gettext(app.locale, "EQUALIZER"));
            super::settings::eq_panel(app, ui);
        }
    }
}


/// What a picture-only button draws.
#[derive(Clone, Copy)]
enum Glyph {
    Icon(Icon),
    /// Covers sliding sideways.
    Scroll,
    /// A cover and its reflection about a centre line.
    Mirror,
    /// A circle with a line through it.
    Nothing,
    /// The title under the cover, beside it, beside it and big.
    TitleUnder,
    TitleBeside,
    TitleBig,
}

/// A small square button with a picture and no text; the words are the tooltip.
fn glyph_button(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    glyph: Glyph,
    tip: &str,
    on: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, theme::CONTROL_HEIGHT), Sense::click());
    let fill = if on {
        palette.accent.gamma_multiply(0.35)
    } else if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(theme::CONTROL_RADIUS), fill);
    let ink = if on { palette.text } else { palette.secondary };
    let c = rect.center();
    let painter = ui.painter();
    match glyph {
        Glyph::Icon(icon) => theme::paint_icon(ui, icon, rect, 17.0, ink),
        Glyph::Scroll => {
            for (i, alpha) in [1.0_f32, 0.65, 0.35].iter().enumerate() {
                let x = c.x - 14.0 + i as f32 * 9.5;
                painter.rect_filled(
                    Rect::from_center_size(pos2(x + 3.5, c.y - 2.0), vec2(7.0, 7.0)),
                    1.5,
                    ink.gamma_multiply(*alpha),
                );
            }
            let y = c.y + 8.0;
            painter.line_segment([pos2(c.x - 12.0, y), pos2(c.x + 12.0, y)], egui::Stroke::new(1.4, ink));
            painter.line_segment([pos2(c.x + 8.0, y - 3.0), pos2(c.x + 12.0, y)], egui::Stroke::new(1.4, ink));
            painter.line_segment([pos2(c.x + 8.0, y + 3.0), pos2(c.x + 12.0, y)], egui::Stroke::new(1.4, ink));
        }
        Glyph::Mirror => {
            let mut y = c.y - 9.0;
            while y < c.y + 9.0 {
                painter.line_segment([pos2(c.x, y), pos2(c.x, y + 3.0)], egui::Stroke::new(1.2, ink));
                y += 5.0;
            }
            for side in [-1.0_f32, 1.0] {
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        pos2(c.x + side * 3.0, c.y - 7.0),
                        pos2(c.x + side * 3.0, c.y + 7.0),
                        pos2(c.x + side * 12.0, c.y + 7.0),
                    ],
                    ink.gamma_multiply(if side < 0.0 { 1.0 } else { 0.55 }),
                    egui::Stroke::NONE,
                ));
            }
        }
        Glyph::TitleUnder => {
            painter.rect_filled(Rect::from_center_size(pos2(c.x, c.y - 4.0), vec2(11.0, 11.0)), 2.0, ink);
            painter.line_segment([pos2(c.x - 9.0, c.y + 9.0), pos2(c.x + 9.0, c.y + 9.0)], egui::Stroke::new(2.0, ink));
        }
        Glyph::TitleBeside => {
            painter.rect_filled(Rect::from_center_size(pos2(c.x - 8.0, c.y), vec2(10.0, 10.0)), 2.0, ink);
            painter.line_segment([pos2(c.x + 1.0, c.y - 2.5), pos2(c.x + 11.0, c.y - 2.5)], egui::Stroke::new(1.6, ink));
            painter.line_segment([pos2(c.x + 1.0, c.y + 2.5), pos2(c.x + 8.0, c.y + 2.5)], egui::Stroke::new(1.6, ink));
        }
        Glyph::TitleBig => {
            painter.rect_filled(Rect::from_center_size(pos2(c.x - 8.0, c.y), vec2(10.0, 10.0)), 2.0, ink);
            for dy in [-5.5_f32, 0.0, 5.5] {
                painter.line_segment([pos2(c.x + 1.0, c.y + dy), pos2(c.x + 11.0, c.y + dy)], egui::Stroke::new(3.0, ink));
            }
        }
        Glyph::Nothing => {
            painter.circle_stroke(c, 7.5, egui::Stroke::new(1.5, ink));
            painter.line_segment(
                [c + vec2(-5.3, 5.3), c + vec2(5.3, -5.3)],
                egui::Stroke::new(1.5, ink),
            );
        }
    }
    response.on_hover_text(tip)
}

/// How much of the sway shows right now: all of it, or what the music asks
/// for while "reacts to the music" is on (loudness, bass or beat).
fn sway_gate(app: &App) -> f32 {
    if !app.settings.vis_sway_beat {
        return 1.0;
    }
    let sens = app.settings.sway_sens.clamp(0.4, 2.5);
    match app.settings.vis_sway_react.min(2) {
        0 => {
            let window = app.audio.tap.window(1536, 0);
            if window.is_empty() {
                return 0.0;
            }
            let loud = (window.iter().map(|x| x * x).sum::<f32>() / window.len() as f32).sqrt() * 2.0;
            ((loud * sens - 0.05) / 0.35).clamp(0.0, 1.0)
        }
        1 => (app.music_bass_level() * sens * 1.4).clamp(0.0, 1.0),
        _ => ((app.music_bass_level() * sens - 0.12) / 0.4).clamp(0.0, 1.0),
    }
}

/// Every title font as a cell showing the playing song in that very font,
/// so the choice is made by looking. With the live preview on, hovering a
/// cell puts that font on the visualizer's title straight away.
#[allow(dead_code)]
fn font_grid(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.horizontal(|ui| {
        theme::subtle(ui, &palette, "TITLE FONT");
        if chip(ui, &palette, "Live preview", app.settings.vis_live_preview, 100.0).clicked() {
            app.actions.push(Action::SetVisLivePreview(!app.settings.vis_live_preview));
        }
        if chip(ui, &palette, "Random each song", app.settings.vis_font_random, 120.0).clicked() {
            app.actions.push(Action::SetVisFontRandom(!app.settings.vis_font_random));
        }
    });
    let sample = app
        .now_playing()
        .map_or_else(|| "Song title".to_string(), |now| now.title.clone());
    let current = usize::from(app.settings.vis_text_font).min(crate::system_fonts::VIS_FONTS.len() - 1);
    let mut hovered: Option<u8> = None;
    // A drop-down: one button showing the chosen face, a list under it when
    // it is opened. Built into the panel rather than a pop-up on a pop-up.
    let open_id = egui::Id::new("vis-font-open");
    let open = ui.ctx().data(|data| data.get_temp::<bool>(open_id)).unwrap_or(false);
    let width = ui.available_width().min(290.0);
    let family_now = vis_font_family(ui.ctx(), current);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let fill = if response.hovered() { palette.surface_hover } else { palette.surface };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), fill);
    ui.painter().with_clip_rect(rect.shrink2(vec2(6.0, 0.0))).text(
        pos2(rect.left() + 10.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        crate::system_fonts::VIS_FONTS[current].0,
        egui::FontId::new(15.0, family_now),
        palette.text,
    );
    theme::paint_icon(
        ui,
        if open { Icon::ChevronUp } else { Icon::ChevronDown },
        Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(20.0, 20.0)),
        15.0,
        palette.secondary,
    );
    if response.clicked() {
        ui.ctx().data_mut(|data| data.insert_temp(open_id, !open));
    }
    if open {
        egui::ScrollArea::vertical()
            .id_salt("vis-font-list")
            .max_height(240.0)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for index in 0..crate::system_fonts::VIS_FONTS.len() {
                    let (rect, response) = ui.allocate_exact_size(vec2(width - 8.0, 30.0), Sense::click());
                    let fill = if index == current {
                        palette.accent.gamma_multiply(0.35)
                    } else if response.hovered() {
                        palette.surface_hover
                    } else {
                        palette.surface
                    };
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(6), fill);
                    let family = vis_font_family(ui.ctx(), index);
                    ui.painter().with_clip_rect(rect.shrink2(vec2(4.0, 0.0))).text(
                        pos2(rect.left() + 10.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        format!("{}   {}", crate::system_fonts::VIS_FONTS[index].0, sample),
                        egui::FontId::new(15.0, family),
                        palette.text,
                    );
                    if response.hovered() || ui.rect_contains_pointer(rect) {
                        hovered = Some(index as u8);
                    }
                    if response.clicked() {
                        app.actions.push(Action::SetVisFont(index as u8));
                        ui.ctx().data_mut(|data| data.insert_temp(open_id, false));
                    }
                }
            });
    }
    let pass = ui.ctx().cumulative_pass_nr();
    if app.settings.vis_live_preview
        && let Some(index) = hovered
    {
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("vis-font-preview"), (index, pass)));
        ui.ctx().request_repaint();
    }
}

/// The font the title is drawn in this frame: the one the reader is hovering
/// in the grid when the live preview is on, otherwise the chosen one.
fn vis_font_in_use(_ctx: &egui::Context, _chosen: u8, _random: bool, _uri: &str) -> usize {
    // The title font is not a setting any more: choosing one made the
    // picture draw with a face the renderer had not been given yet. The
    // title always uses the app's own face.
    0
}

/// The family for visualizer font `font_index`, loading the file the first
/// time it is asked for. Falls back to Inter while it loads or is missing.
#[allow(dead_code)]
fn vis_font_family(ctx: &egui::Context, font_index: usize) -> egui::FontFamily {
    let mut family = egui::FontFamily::Proportional;
    if font_index > 0 {
        let name = format!("vis-font-{font_index}");
        let flag = egui::Id::new(("vis-font-loaded", font_index));
        let state = ctx.data(|data| data.get_temp::<bool>(flag));
        if state.is_none() {
            // The display face first, then everything the normal text uses, so
            // Cyrillic, Chinese and other scripts the display face lacks still draw.
            let loaded = crate::system_fonts::vis_font_bytes(font_index).is_some_and(|bytes| {
                let mut fonts = ctx.fonts(|fonts| fonts.definitions().clone());
                fonts
                    .font_data
                    .insert(name.clone(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                let mut list = vec![name.clone()];
                if let Some(base) = fonts.families.get(&egui::FontFamily::Proportional) {
                    list.extend(base.iter().cloned());
                }
                fonts.families.insert(egui::FontFamily::Name(name.clone().into()), list);
                ctx.set_fonts(fonts);
                true
            });
            ctx.data_mut(|data| data.insert_temp(flag, loaded));
            ctx.request_repaint();
        } else if state == Some(true) {
            family = egui::FontFamily::Name(name.into());
        }
    }
    family
}

/// Saved visualizer looks: pick one to apply, x to remove, or save the
/// current look under a name.
#[allow(dead_code)]
fn vis_presets_row(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(8.0);
    theme::subtle(ui, &palette, "SAVED LOOKS");
    let name_id = egui::Id::new("vis-preset-name");
    let mut name: String = ui.data(|data| data.get_temp(name_id)).unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        for (index, preset) in app.settings.vis_presets.iter().enumerate() {
            if ui.add(egui::Button::new(preset.name.as_str())).clicked() {
                app.actions.push(Action::ApplyVisPreset(index));
            }
            if theme::icon_button(ui, Icon::X, 12.0, palette.dim, palette.text, "Remove this look").clicked() {
                app.actions.push(Action::RemoveVisPreset(index));
            }
            ui.add_space(6.0);
        }
    });
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut name).hint_text("Name this look").desired_width(160.0));
        if ui.add_enabled(!name.trim().is_empty(), egui::Button::new("Save this look")).clicked() {
            app.actions.push(Action::SaveVisPreset(name.clone()));
            name.clear();
        }
    });
    ui.data_mut(|data| data.insert_temp(name_id, name));
}

/// A plain text box for a number. While it has focus the text being typed
/// is kept between frames; it hands back the text once, when the reader
/// presses Enter or clicks away.
fn number_field(
    ui: &mut egui::Ui,
    id: egui::Id,
    shown: String,
    width: f32,
    colour: Color32,
) -> Option<String> {
    number_field_framed(ui, id, shown, width, colour, true)
}

fn number_field_framed(
    ui: &mut egui::Ui,
    id: egui::Id,
    shown: String,
    width: f32,
    colour: Color32,
    frame: bool,
) -> Option<String> {
    let stored = ui.data(|data| data.get_temp::<String>(id));
    let mut text = stored.unwrap_or(shown);
    let mut edit = egui::TextEdit::singleline(&mut text)
        .id(id)
        .desired_width(width)
        .text_color(colour);
    if !frame {
        edit = edit.frame(egui::Frame::NONE);
    }
    let out = edit.show(ui);
    let submitted = out.response.lost_focus()
        || (out.response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)));
    if submitted {
        ui.data_mut(|data| data.remove::<String>(id));
        return Some(text);
    }
    if out.response.has_focus() || out.response.changed() {
        ui.data_mut(|data| data.insert_temp(id, text));
    }
    None
}

/// What a right-click on a scene or curve shows about it.
struct PresetInfo {
    key: String,
    /// The speed, for a scene.
    speed: Option<f32>,
    bands: [f32; 10],
}

/// The curve itself, drawn: ten gains left to right, with the zero line.
fn curve_preview(ui: &mut egui::Ui, palette: &crate::theme::Palette, bands: &[f32; 10]) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(260.0), 64.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(6), palette.surface);
    ui.painter().hline(
        rect.x_range().shrink(6.0),
        rect.center().y,
        egui::Stroke::new(1.0, palette.dim.gamma_multiply(0.6)),
    );
    let span = crate::eq::RANGE_DB.max(1.0);
    let points: Vec<egui::Pos2> = bands
        .iter()
        .enumerate()
        .map(|(i, gain)| {
            let x = rect.left() + 10.0 + (rect.width() - 20.0) * i as f32 / 9.0;
            let y = rect.center().y - (gain / span).clamp(-1.0, 1.0) * (rect.height() / 2.0 - 6.0);
            pos2(x, y)
        })
        .collect();
    ui.painter()
        .add(egui::Shape::line(points.clone(), egui::Stroke::new(2.0, palette.accent)));
    for point in points {
        ui.painter().circle_filled(point, 2.5, palette.accent);
    }
}

/// Right-click a scene or curve: see what it is (its curve, and for a scene
/// its speed), copy it out, paste some in, or remove it. Your own is
/// deleted; a built-in one is only hidden, and "Restore removed" brings it
/// back.
fn remove_menu(
    app: &mut App,
    response: &egui::Response,
    palette: &crate::theme::Palette,
    info: PresetInfo,
) {
    egui::Popup::context_menu(response)
        .frame(super::widgets::menu_frame(palette))
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.set_width(270.0);
            let title = match info.speed {
                Some(speed) => format!("{}  \u{b7}  {:.0}% speed", info.key, speed * 100.0),
                None => info.key.clone(),
            };
            theme::subtle(ui, palette, &title.to_uppercase());
            curve_preview(ui, palette, &info.bands);
            ui.add_space(4.0);
            if super::widgets::menu_item(ui, palette, None, &gettext(app.locale, "Export this (copy)")) {
                let gains = info
                    .bands
                    .iter()
                    .map(|gain| format!("{gain:.1}"))
                    .collect::<Vec<_>>()
                    .join(",");
                let name = info.key.replace('|', "/");
                let line = match info.speed {
                    Some(speed) => format!("SCENE|{name}|{speed:.2}|{gains}\n"),
                    None => format!("EQ|{name}|{gains}\n"),
                };
                ui.ctx().copy_text(line);
                app.toast(gettext(app.locale, "Copied"));
            }
            if super::widgets::menu_item(ui, palette, None, &gettext(app.locale, "Remove")) {
                app.actions.push(Action::RemovePreset(info.key.clone()));
            }
            super::widgets::menu_separator(ui, palette);
            share_rows(ui, app, palette);
        });
}

/// A name box and a Save button: keeps the current curve (or, for a scene,
/// the current speed and curve) under that name.
fn save_row(ui: &mut egui::Ui, app: &mut App, palette: &crate::theme::Palette, scene: bool) {
    let id = egui::Id::new(if scene { "save-scene-name" } else { "save-eq-name" });
    let mut name = ui.data(|data| data.get_temp::<String>(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let edit = egui::TextEdit::singleline(&mut name)
            .hint_text(
                gettext(
                    app.locale,
                    if scene { "Name your scene" } else { "Name your curve" },
                )
                .to_string(),
            )
            .desired_width((ui.available_width() - 96.0).max(60.0))
            .show(ui);
        let enter = edit.response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        let clicked = chip(ui, palette, &gettext(app.locale, "Save"), false, 84.0).clicked();
        if (clicked || enter) && !name.trim().is_empty() {
            app.actions.push(if scene {
                Action::SaveCustomScene(name.clone())
            } else {
                Action::SaveCustomEq(name.clone())
            });
            name.clear();
        }
    });
    ui.data_mut(|data| data.insert_temp(id, name));
}

/// Copy your saved curves and scenes out as text, paste some in, and bring
/// back any built-in ones that were removed.
fn share_rows(ui: &mut egui::Ui, app: &mut App, palette: &crate::theme::Palette) {
    let id = egui::Id::new("import-presets");
    let mut pasted = ui.data(|data| data.get_temp::<String>(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if chip(ui, palette, &gettext(app.locale, "Export (copy)"), false, 150.0).clicked() {
            let text = app.settings.export_presets();
            ui.ctx().copy_text(text);
            app.toast(gettext(app.locale, "Copied your curves and scenes"));
        }
        if !app.settings.hidden_presets.is_empty()
            && chip(ui, palette, &gettext(app.locale, "Restore removed"), false, 150.0).clicked()
        {
            app.actions.push(Action::RestoreHiddenPresets);
        }
    });
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        egui::TextEdit::singleline(&mut pasted)
            .hint_text(gettext(app.locale, "Paste exported text, then Import").to_string())
            .desired_width((ui.available_width() - 96.0).max(60.0))
            .show(ui);
        if chip(ui, palette, &gettext(app.locale, "Import"), false, 84.0).clicked()
            && !pasted.trim().is_empty()
        {
            app.actions.push(Action::ImportPresets(pasted.clone()));
            pasted.clear();
        }
    });
    ui.data_mut(|data| data.insert_temp(id, pasted));
}

#[derive(Clone, Copy)]
enum ShapeIcon {
    Bars,
    Flow,
    Swirl,
}

/// A toggle for one visualizer shape, with a small drawn icon: a bar graph,
/// a wave, and a spiral galaxy.
fn shape_button(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    kind: ShapeIcon,
    label: &str,
    on: bool,
    width: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 32.0), Sense::click());
    let fill = if on {
        palette.accent.gamma_multiply(0.35)
    } else if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(theme::CONTROL_RADIUS), fill);
    let ink = if on { palette.text } else { palette.secondary };
    let icon = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), vec2(18.0, 16.0));
    let painter = ui.painter();
    match kind {
        ShapeIcon::Bars => {
            for (i, h) in [0.45_f32, 0.9, 0.65, 1.0].iter().enumerate() {
                let x = icon.left() + 1.0 + i as f32 * 4.5;
                painter.rect_filled(
                    Rect::from_min_max(
                        pos2(x, icon.bottom() - icon.height() * h),
                        pos2(x + 3.0, icon.bottom()),
                    ),
                    1.0,
                    ink,
                );
            }
        }
        ShapeIcon::Flow => {
            for row in 0..2 {
                let y0 = icon.center().y - 3.0 + row as f32 * 6.0;
                let points: Vec<egui::Pos2> = (0..=16)
                    .map(|i| {
                        let t = i as f32 / 16.0;
                        pos2(
                            icon.left() + t * icon.width(),
                            y0 + (t * std::f32::consts::TAU * 1.5 + row as f32).sin() * 2.6,
                        )
                    })
                    .collect();
                painter.add(egui::Shape::line(points, egui::Stroke::new(1.8, ink)));
            }
        }
        ShapeIcon::Swirl => {
            let centre = icon.center();
            for arm in 0..2 {
                let points: Vec<egui::Pos2> = (0..=24)
                    .map(|i| {
                        let t = i as f32 / 24.0;
                        let angle = t * std::f32::consts::TAU * 1.1 + arm as f32 * std::f32::consts::PI;
                        let radius = 1.0 + t * 7.5;
                        pos2(centre.x + angle.cos() * radius, centre.y + angle.sin() * radius)
                    })
                    .collect();
                painter.add(egui::Shape::line(points, egui::Stroke::new(1.6, ink)));
            }
            painter.circle_filled(centre, 1.4, ink);
        }
    }
    let galley = crate::bidi::layout(
        painter,
        label,
        theme::regular(theme::BODY_TEXT),
        ink,
        (width - 40.0).max(10.0),
        1,
        Some(crate::bidi::ELLIPSIS),
    );
    painter.galley(
        pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink,
    );
    response
}

/// A title and every choice as chips right under it, two to a row, with
/// nothing to open first. Returns the one clicked this frame.
/// The chosen sway style, moving: a small word that sways the way the title
/// will, so a style can be judged without leaving the panel.
fn sway_preview(ui: &mut egui::Ui, palette: &crate::theme::Palette, preset: crate::settings::SwayPreset, strength: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(290.0), 34.0), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let time = ui.input(|input| input.time) as f32;
    let painter = ui.painter().with_clip_rect(rect);
    let font = theme::bold(theme::SECTION_TEXT);
    let word = "sway";
    let glyphs: Vec<_> = word
        .chars()
        .map(|c| painter.layout_no_wrap(c.to_string(), font.clone(), palette.text))
        .collect();
    let total: f32 = glyphs.iter().map(|g| g.size().x).sum();
    let mut x = rect.left() + 6.0;
    let base = rect.bottom() - 6.0;
    let t = time * preset.speed;
    // A steady beat for the preview, so bass-driven styles show their pump.
    let pulse = ((time * 2.0).sin().max(0.0)).powi(3);
    let lean = |phase: f32| -> f32 {
        if matches!(preset.kind, 1 | 2 | 4) {
            return 0.0;
        }
        ((0.05 * (t * 0.9 - phase * 0.6).sin() + 0.03 * (t * 1.7 - phase * 0.9 + 1.0).sin())
            * (1.0 + 2.0 * pulse * preset.bass * 0.5)
            * preset.lean
            * strength)
            .clamp(-0.6, 0.6)
    };
    for (i, galley) in glyphs.iter().enumerate() {
        let w = galley.size().x;
        let h = galley.size().y;
        let bob = (time * 3.0 * preset.speed - i as f32 * 0.55).sin() * preset.bob * h * strength;
        let drive = 0.4 + 0.6 * pulse * preset.bass.min(1.6);
        let motion = match preset.kind {
            1 => vec2(0.0, -(t * 2.4 - i as f32 * 0.12).sin().abs() * preset.amp * h * strength * drive),
            2 => vec2((t * 1.6 - i as f32 * 0.1).sin() * preset.amp * h * strength * (0.6 + 0.4 * drive), 0.0),
            4 => vec2(
                (time * 41.0 + i as f32 * 7.3).sin() * preset.amp * h * strength * drive,
                (time * 37.0 + i as f32 * 5.1).cos() * preset.amp * h * strength * drive,
            ),
            _ => vec2(0.0, 0.0),
        };
        let origin = pos2(x, base - h + bob) + motion;
        let ripple = if preset.kind == 3 { 0.0 } else { 0.22 };
        let (pivot, angle) = if preset.stalk > 0.0 {
            (pos2(x + w / 2.0, base + bob), lean(i as f32 * ripple))
        } else {
            (pos2(rect.left() + 6.0 + total / 2.0, base), lean(0.0))
        };
        let (sin, cos) = angle.sin_cos();
        let d = origin - pivot;
        let mut shape = egui::epaint::TextShape::new(origin, galley.clone(), palette.text);
        shape.pos = pivot + vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos);
        shape.angle = angle;
        painter.add(egui::Shape::Text(shape));
        x += w;
    }
    ui.ctx().request_repaint();
}

#[allow(dead_code)]
fn open_choices(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    title: &str,
    choices: &[&str],
    selected: Option<usize>,
) -> Option<usize> {
    let mut picked = None;
    theme::subtle(ui, palette, title);
    let width = ((ui.available_width() - 6.0) / 2.0).floor().clamp(60.0, 200.0);
    let mut index = 0usize;
    for row in choices.chunks(2) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for name in row {
                if chip(ui, palette, name, selected == Some(index), width).clicked() {
                    picked = Some(index);
                }
                index += 1;
            }
        });
    }
    picked
}

/// Presets as rows of numbered buttons: a picture for the family, then 1, 2,
/// 3... for each of its looks. The words are the tooltips.
fn numbered_rows(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    title: &str,
    rows: &[(Icon, std::ops::Range<usize>)],
    names: &[&str],
    selected: Option<usize>,
) -> Option<usize> {
    let mut picked = None;
    theme::subtle(ui, palette, title);
    for (icon, range) in rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let (rect, _) = ui.allocate_exact_size(vec2(26.0, 24.0), Sense::hover());
            theme::paint_icon(ui, *icon, rect, 17.0, palette.secondary);
            for (number, index) in range.clone().enumerate() {
                let name = names.get(index).copied().unwrap_or("");
                let label = format!("{}", number + 1);
                if chip(ui, palette, &label, selected == Some(index), 30.0).on_hover_text(name).clicked() {
                    picked = Some(index);
                }
            }
        });
    }
    picked
}

/// A fixed-width selectable chip.
/// A small dot at the top right of a button that is switched on, so its state
/// reads even when the accent colour matches what is around it.
fn on_dot(ui: &egui::Ui, rect: egui::Rect, palette: &crate::theme::Palette) {
    let at = rect.right_top() + egui::vec2(-3.0, 3.0);
    ui.painter().circle_filled(at, 3.4, palette.window);
    ui.painter().circle_filled(at, 2.4, palette.text);
}

fn chip(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    label: &str,
    active: bool,
    width: f32,
) -> egui::Response {
    chip_sensing(ui, palette, label, active, width, Sense::click())
}

/// The little picture a menu option carries, when it has one.
fn icon_for(label: &str) -> Option<Icon> {
    Some(match label {
        "Title vibration" | "Title sway" => Icon::Wind,
        "Art border" => Icon::Square,
        "Sway on the beat only" => Icon::Zap,
        "Title outline" => Icon::Pencil,
        "Panel behind the title" => Icon::Tv,
        "Artist name under the title" | "Artist name" | "Artist" => Icon::User,
        "Show lyrics over it" => Icon::Mic,
        "Dark background behind the lyrics" => Icon::Moon,
        "Art bounces to the music" | "Song name" => Icon::Music,
        "Floating art" => Icon::Sparkles,
        "Album art" => Icon::Disc,
        "Timestamps" => Icon::Clock,
        "Countdown" => Icon::Watch,
        "Left" => Icon::ArrowLeft,
        "Right" => Icon::ArrowRight,
        "Focus on the current line" => Icon::Expand,
        "Heart" => Icon::Heart,
        _ => return None,
    })
}

/// A chip with its picture before the words (words only when it has none).
fn chip_i(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    label: &str,
    active: bool,
    width: f32,
) -> egui::Response {
    let Some(icon) = icon_for(label) else {
        return chip(ui, palette, label, active, width);
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, theme::CONTROL_HEIGHT), Sense::click());
    let fill = if active {
        palette.accent.gamma_multiply(0.35)
    } else if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(theme::CONTROL_RADIUS), fill);
    let ink = if active { palette.text } else { palette.secondary };
    let slot = Rect::from_min_size(pos2(rect.left() + 4.0, rect.top()), vec2(22.0, theme::CONTROL_HEIGHT));
    theme::paint_icon(ui, icon, slot, 14.0, ink);
    let galley = crate::bidi::layout(
        ui.painter(),
        label,
        theme::regular(theme::SMALL_TEXT),
        ink,
        (width - 34.0).max(10.0),
        1,
        Some(crate::bidi::ELLIPSIS),
    );
    ui.painter().galley(
        pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text,
    );
    response
}

fn chip_sensing(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    label: &str,
    active: bool,
    width: f32,
    sense: Sense,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, theme::CONTROL_HEIGHT), sense);
    let fill = if active {
        palette.accent.gamma_multiply(0.35)
    } else if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(theme::CONTROL_RADIUS), fill);
    let galley = crate::bidi::layout(
        ui.painter(),
        label,
        theme::regular(theme::SMALL_TEXT),
        if active { palette.text } else { palette.secondary },
        (width - 8.0).max(10.0),
        1,
        Some(crate::bidi::ELLIPSIS),
    );
    ui.painter().galley(
        pos2(rect.center().x - galley.size().x / 2.0, rect.center().y - galley.size().y / 2.0),
        galley,
        palette.text,
    );
    response
}

/// A labelled slider with a number at the end that can also be typed into.
///
/// The steppers this replaced could only walk a list of a handful of
/// sizes, and the list was shorter than the range they stood for: a bar
/// count capped at the analyser's seventy-five bands, and a gap capped at
/// ten pixels. A slider reaches anywhere in the range in one drag.
///
/// Dragging is not the only way to a value, though. Reaching 200 bars by
/// dragging is fine; reaching it *again* next week, or setting 7.4 exactly,
/// is not. So the number at the end is a real field: click it, type, press
/// Enter or click away. It only submits on one of those two, never as you
/// type — setting the bar count to 1 as the first digit of 128 is worse
/// than not setting it at all.
pub(super) fn slider_row(
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    label: &str,
    range: std::ops::RangeInclusive<f32>,
    value: f32,
    mut changed: impl FnMut(f32),
) {
    crate::crash::stage(&format!("menu: slider {label}"));
    // The track and the field are one row. Laid out as a full-width row and
    // then a field after it, the field falls to the next line the moment the
    // menu is wide — which is how it ended up under its own slider.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let width = (ui.available_width() - FIELD_WIDTH - 8.0).max(80.0);
        // The label gets what the row can spare, never more than the usual
        // column, so a narrow menu keeps a track to drag.
        let label_col = (width * 0.58).clamp(84.0, LABEL_COLUMN);
        let (rect, _) = ui.allocate_exact_size(vec2(width, theme::CONTROL_HEIGHT), Sense::hover());
        let galley = crate::bidi::layout(
            ui.painter(),
            label,
            theme::regular(theme::BODY_TEXT),
            palette.text,
            label_col - 14.0,
            1,
            Some(crate::bidi::ELLIPSIS),
        );
        ui.painter().galley(
            pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
            galley.clone(),
            palette.text,
        );
        let span = range.end() - range.start();

        // The field, on the right of the row and the same on every one of them.
        let decimals = if range.start().fract() == 0.0 && range.end().fract() == 0.0 {
            0
        } else if span <= 0.5 {
            3
        } else if span <= 1.5 {
            2
        } else if span <= 20.0 {
            1
        } else {
            0
        };
        if let Some(text) = number_field(
            ui,
            ui.id().with(("vis-field", label)),
            format!("{value:.decimals$}"),
            FIELD_WIDTH,
            palette.accent,
        ) && let Ok(parsed) = text.trim().parse::<f32>()
        {
            changed(parsed.clamp(*range.start(), *range.end()));
        }

        // The track runs between the label and the field, so nothing ever
        // overlaps however long either of them is.
        let left = rect.left() + label_col;
        let right = rect.right() - FIELD_WIDTH - 16.0;
        let track = Rect::from_center_size(
            pos2((left + right) / 2.0, rect.center().y),
            vec2((right - left).max(36.0), 18.0),
        );
        let response = ui.interact(
            track,
            ui.id().with(("vis-slider", label)),
            Sense::click_and_drag(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Slider, ui.is_enabled(), label)
        });
        theme::focus_ring(ui, &response);
        if span > 0.0 && ui.is_rect_visible(track) {
            let fraction = ((value - range.start()) / span).clamp(0.0, 1.0);
            let rail = Rect::from_center_size(track.center(), vec2(track.width(), 4.0));
            ui.painter()
                .rect_filled(rail, egui::CornerRadius::same(2), palette.surface_hover);
            ui.painter().rect_filled(
                Rect::from_min_max(
                    rail.left_top(),
                    pos2(rail.left() + rail.width() * fraction, rail.bottom()),
                ),
                egui::CornerRadius::same(2),
                palette.accent,
            );
            ui.painter().circle_filled(
                pos2(rail.left() + rail.width() * fraction, rail.center().y),
                5.0,
                palette.accent,
            );
        }
        if (response.dragged() || response.clicked())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let fraction = ((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0);
            changed(range.start() + span * fraction);
        }
    });
}

/// How wide the number field beside a slider is. Enough for "256" and a
/// decimal, which is the widest any of these ranges can honestly be.
const FIELD_WIDTH: f32 = 52.0;

/// Every slider's track starts this far from the row's left edge, so the
/// tracks line up whatever the label says. Longer labels are cut short.
const LABEL_COLUMN: f32 = 128.0;

/// Fewer bars is a coarser reading of the same sound: each bar keeps the
/// loudest band of its group rather than an average, which would let a
/// quiet group vanish between two loud ones.
#[test]
fn merging_bands_keeps_the_loudest_of_each_group() {
    let bands = [0.1, 0.9, 0.2, 0.05, 0.4, 0.3];
    assert_eq!(merge_bands(&bands, 6), bands.to_vec());
    assert_eq!(merge_bands(&bands, 3), vec![0.9, 0.2, 0.4]);
    assert_eq!(merge_bands(&bands, 2), vec![0.9, 0.4]);
    // More bars than bands must not invent any.
    assert_eq!(merge_bands(&bands, 99).len(), bands.len());
    // Silence stays silence at every density.
    assert_eq!(merge_bands(&[0.0; 75], 12), vec![0.0; 12]);
}

/// The loudest band of each group, for a bar count below the analyser's
/// own. Taking the loudest rather than the mean keeps a quiet group from
/// disappearing into its neighbours.
pub(crate) fn merge_bands(bands: &[f32], wanted: usize) -> Vec<f32> {
    if bands.is_empty() {
        return vec![0.0; wanted.max(1)];
    }
    let wanted = wanted.clamp(1, bands.len());
    let count = bands.len();
    (0..wanted)
        .map(|bar| {
            // The groups are cut by proportion rather than by a fixed size,
            // so the reader gets the number of bars they asked for. Fixed
            // chunks drop the remainder: 75 bands into 12 bars would give
            // 11 and leave the last one showing nothing at all.
            let start = bar * count / wanted;
            let end = ((bar + 1) * count / wanted).max(start + 1).min(count);
            bands[start..end].iter().copied().fold(0.0, f32::max)
        })
        .collect()
}

/// Spreads the analyser's bands across `points` readings, holding the ends
/// and interpolating between. Used where the drawing wants more resolution
/// than the analyser has, which is the opposite of [`merge_bands`]'s job:
/// nothing is lost, the extra points are the ones in between.
pub(crate) fn spread_bands(bands: &[f32], points: usize) -> Vec<f32> {
    if bands.is_empty() || points < 2 {
        return vec![0.0; points.max(2)];
    }
    if bands.len() == 1 {
        return vec![bands[0]; points];
    }
    (0..points)
        .map(|point| {
            // Where this point falls in the band list, 0 at the first band
            // and `len - 1` at the last.
            let at = point as f32 * (bands.len() - 1) as f32 / (points - 1) as f32;
            let low = at.floor() as usize;
            let high = (low + 1).min(bands.len() - 1);
            let fraction = at - low as f32;
            bands[low] * (1.0 - fraction) + bands[high] * fraction
        })
        .collect()
}

/// The bar drawn as a swirl: the album cover, warped into a spiral and
/// carried sideways across the whole width of the bar.
///
/// This is the cover itself, not the *shape* of a swirl drawn in the
/// cover's colours. Two earlier attempts were rings — a spectrum drawn as
/// closed curves — which was a waveform that happened to be round, and then a chain
/// of those rings across the bar, which was the same idea stretched. What
/// makes a swirl a swirl is that it is the picture being dragged around a
/// centre, which is what a desktop engine does with whatever is on its
/// screen. `crate::swirl_art` does the dragging; this does the travelling.
///
/// The cover is drawn as ONE picture across the whole width of the bar and
/// moved along x by sliding the texture's own coordinates, rather than
/// repeated. Tiling it was the mistake: a bar-width tile of a warped cover
/// reads as the same blob over and over, which is a pattern rather than a
/// picture. One copy, stretched to fill, wrapping seamlessly because
/// `swirl_art` makes its own edges meet. The spectrum no longer draws it —
/// it only decides how bright it is, so a loud track is a brighter swirl
/// rather than a different picture.
fn swirl(
    painter: &egui::Painter,
    rect: Rect,
    levels: &[f32],
    strength: f32,
    phase: f64,
    vertical: bool,
    art: Option<&egui::TextureId>,
    previous: Option<(egui::TextureId, f32)>,
    run: SwirlRun,
    wash: Option<(Color32, Color32)>,
) {
    let Some(art) = art else {
        return;
    };
    if rect.width() < 24.0 || rect.height() < 8.0 {
        return;
    }
    // Copies of the (tileable) backdrop across the bar.
    let scale = 1.8f32;
    let time = phase;
    let bass = levels.iter().take(levels.len() / 8).copied().sum::<f32>()
        / (levels.len() / 8).max(1) as f32;
    let alpha = if run.art {
        strength.min(1.0)
    } else {
        (0.6 + bass * 0.4).clamp(0.45, 1.0) * strength.min(1.0) * run.bright.clamp(0.0, 1.0)
    };
    let tint = Color32::WHITE.gamma_multiply(alpha);
    // The slide is measured in copies of the picture, and the picture
    // repeats every whole copy, so wrapping at one copy is invisible. (It
    // used to wrap at a fraction of a copy, which is the jump that showed
    // up now and then.)
    let slide = if run.art {
        ((time * 0.25).rem_euclid(1.0)) as f32
    } else {
        ((time * 0.03 * f64::from(scale)).rem_euclid(1.0)) as f32
    };
    // In fullscreen it also sinks slowly, so it moves left and down.
    let sink = if vertical && !run.art { ((time * 0.012).rem_euclid(1.0)) as f32 } else { 0.0 };
    let natural = 16.0 / 9.0;
    let (uv_width, tall, top) = if run.art {
        // The cover always fits the height of the bar, top to bottom: each
        // half of the picture is one square cover, so the strip is as wide
        // as the bar needs and the height is never cropped.
        ((if run.mirror { 0.5 } else { 1.0 }) * rect.width() / rect.height(), 1.0, 0.0)
    } else {
        let tall = (rect.height() * natural / rect.width() * scale).clamp(0.02, 4.0);
        (scale, tall, 0.5 - tall / 2.0 - sink)
    };
    let uv = egui::Rect::from_min_max(pos2(-slide, top), pos2(uv_width - slide, top + tall));
    let warp = 0.0;
    warped_image(&painter.with_clip_rect(rect), rect, *art, uv, tint, warp, time);
    if let Some((old, fade)) = previous {
        warped_image(
            &painter.with_clip_rect(rect),
            rect,
            old,
            uv,
            Color32::WHITE.gamma_multiply(alpha * fade),
            warp,
            time,
        );
    }
    // The picked colour: two of the cover's colours washed over the swirl in
    // slow bands that travel with it, so choosing a different swatch visibly
    // recolours the whole backdrop.
    if let Some((first, second)) = wash.filter(|_| !run.art) {
        // Enough columns that every band is drawn with a good many slices;
        // too few made high band counts into streaky, stepped stripes.
        let columns: usize = ((run.bands * 40.0) as usize).clamp(96, 900);
        let mut mesh = egui::Mesh::default();
        let (hi, lo) = (0.5_f32 * run.wash, 0.5_f32 * run.wash);
        for column in 0..=columns {
            let frac = column as f32 / columns as f32;
            let x = rect.left() + rect.width() * frac;
            let phase = std::f32::consts::TAU * run.bands * (frac - (time * 0.03) as f32);
            let mix = 0.5 + 0.5 * phase.sin();
            let mix_low = 0.5 + 0.5 * (phase + 1.9).sin();
            let lerp = |a: Color32, b: Color32, t: f32| {
                Color32::from_rgb(
                    (f32::from(a.r()) * (1.0 - t) + f32::from(b.r()) * t).round() as u8,
                    (f32::from(a.g()) * (1.0 - t) + f32::from(b.g()) * t).round() as u8,
                    (f32::from(a.b()) * (1.0 - t) + f32::from(b.b()) * t).round() as u8,
                )
            };
            mesh.colored_vertex(
                pos2(x, rect.top()),
                lerp(first, second, mix).gamma_multiply((hi * strength.min(1.0)).clamp(0.0, 1.0)),
            );
            mesh.colored_vertex(
                pos2(x, rect.bottom()),
                lerp(second, first, mix_low).gamma_multiply((lo * strength.min(1.0)).clamp(0.0, 1.0)),
            );
        }
        for column in 0..columns as u32 {
            let i = column * 2;
            mesh.add_triangle(i, i + 1, i + 2);
            mesh.add_triangle(i + 1, i + 3, i + 2);
        }
        painter.with_clip_rect(rect).add(egui::Shape::mesh(mesh));
    }
    // A wash along the foot, so words in front of it stay readable.
    let mut glow = egui::Mesh::default();
    shaded(
        &mut glow,
        Rect::from_min_max(
            pos2(rect.left(), rect.center().y),
            pos2(rect.right(), rect.bottom()),
        ),
        Color32::TRANSPARENT,
        Color32::BLACK.gamma_multiply(SWIRL_SHADE * strength.min(1.0)),
    );
    painter.add(egui::Shape::mesh(glow));
}

/// The backdrop picture, bent as it drifts: a grid of points whose texture
/// coordinates are pushed around by slow waves, so "Swirliness" changes the
/// shape of the swirl itself and not just how big it is. The picture tiles,
/// so any coordinates are fine.
fn warped_image(
    painter: &egui::Painter,
    rect: Rect,
    texture: egui::TextureId,
    uv: egui::Rect,
    tint: Color32,
    warp: f32,
    time: f64,
) {
    if warp <= 0.01 {
        painter.image(texture, rect, uv, tint);
        return;
    }
    const COLS: usize = 72;
    const ROWS: usize = 6;
    let t = time as f32;
    let tau = std::f32::consts::TAU;
    let mut mesh = egui::Mesh::with_texture(texture);
    for j in 0..=ROWS {
        for i in 0..=COLS {
            let fx = i as f32 / COLS as f32;
            let fy = j as f32 / ROWS as f32;
            let dx = (tau * (fx * 2.0 + fy * 0.7) + t * 0.35).sin() * 0.07 * warp * uv.width();
            let dy = (tau * (fx * 1.3 - fy * 0.9) + t * 0.27).cos() * 0.9 * warp * uv.height();
            mesh.vertices.push(egui::epaint::Vertex {
                pos: pos2(rect.left() + fx * rect.width(), rect.top() + fy * rect.height()),
                uv: pos2(
                    uv.left() + fx * uv.width() + dx,
                    uv.top() + fy * uv.height() + dy,
                ),
                color: tint,
            });
        }
    }
    for j in 0..ROWS {
        for i in 0..COLS {
            let a = (j * (COLS + 1) + i) as u32;
            let b = a + 1;
            let c = a + (COLS + 1) as u32;
            let d = c + 1;
            mesh.add_triangle(a, b, c);
            mesh.add_triangle(b, d, c);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The fullscreen Swirl scene: ragged black bars hanging from the top and
/// growing from the left edge, the cover as a square at the left, and the
/// title large and translucent at the right with no box behind it.
/// Where the big cover sits in the fullscreen visualizer. The bars and the
/// flow measure themselves against it so they never touch the picture.
/// `layout` is the title layout the reader chose, because the words take
/// room round the cover and the cover moves to leave it.
pub(crate) fn fullscreen_cover(rect: Rect, layout: u8) -> Rect {
    let left = rect.left() + rect.width() * 0.08;
    match layout {
        // The words go beside the cover: a little smaller, so they have a
        // column of their own, and centred up and down.
        1 | 2 => {
            let side = (rect.height() * 0.5).min(450.0).min(rect.width() * 0.34);
            let top = ((rect.height() - side) / 2.0 - rect.height() * 0.03).max(24.0);
            Rect::from_min_size(pos2(left, rect.top() + top), vec2(side, side))
        }
        // The title above the cover and the artist below it.
        3 => {
            let side = (rect.height() * 0.5).min(450.0).min(rect.width() * 0.4);
            let size = fullscreen_title_size(rect);
            let above = 18.0 + size * 1.3;
            let below = 14.0 + size * 0.7;
            let block = side + above + below;
            let top = ((rect.height() - block) / 2.0 - rect.height() * 0.03).max(24.0) + above;
            Rect::from_min_size(pos2(left, rect.top() + top), vec2(side, side))
        }
        // The cover, the title and the artist are one block, a little above
        // the middle so the words still clear the bars at the foot.
        _ => {
            let side = (rect.height() * 0.5).min(450.0).min(rect.width() * 0.4);
            let block = side + 18.0 + fullscreen_title_size(rect) * 1.9;
            let top = ((rect.height() - block) / 2.0 - rect.height() * 0.03).max(24.0);
            Rect::from_min_size(pos2(left, rect.top() + top), vec2(side, side))
        }
    }
}

/// The title's size under the cover.
fn fullscreen_title_size(rect: Rect) -> f32 {
    (rect.height() * 0.05).clamp(20.0, 64.0)
}

/// Draws the cover as a card that floats about on its own (when `floating`)
/// and turns to face the pointer while it is over it, in 3D, with a shadow
/// that turns with it: the full-screen lyrics' card. Returns whether it drew
/// the cover; with the card flat and still, the caller draws it as usual.
fn tilted_cover(ui: &egui::Ui, cover: Rect, url: Option<&str>, floating: bool, border: bool) -> bool {
    let ctx = ui.ctx().clone();
    let side = cover.width();
    let pointer = ui.input(|input| input.pointer.hover_pos());
    let over = pointer.is_some_and(|p| cover.expand(30.0).contains(p));
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
        ctx.request_repaint();
    }
    let id = egui::Id::new("vis-cover-tilt");
    let nx = ctx.animate_value_with_time(id.with("x"), nx, 0.15);
    let ny = ctx.animate_value_with_time(id.with("y"), ny, 0.15);
    if nx.abs() <= 0.002 && ny.abs() <= 0.002 && !over {
        return false;
    }
    let Some(url) = url else {
        return false;
    };
    let image = egui::Image::new(url).show_loading_spinner(false);
    let Ok(egui::load::TexturePoll::Ready { texture }) = image.load_for_size(&ctx, cover.size()) else {
        return false;
    };
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
    // A soft shadow right behind the card, wider the further it falls.
    for layer in 0..10 {
        let grow = layer as f32 * 3.5;
        let points: Vec<egui::Pos2> = corners
            .iter()
            .map(|p| {
                let out = (*p - cover.center()).normalized();
                *p + out * grow + vec2(0.0, 8.0)
            })
            .collect();
        ui.painter().add(egui::Shape::convex_polygon(
            points,
            Color32::from_black_alpha(9),
            egui::Stroke::NONE,
        ));
    }
    let n = 12;
    let mut mesh = egui::epaint::Mesh::with_texture(texture.id);
    for row in 0..=n {
        for column_index in 0..=n {
            let u = column_index as f32 / n as f32;
            let v = row as f32 / n as f32;
            let x = u * 2.0 - 1.0;
            let y = v * 2.0 - 1.0;
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
    // The border follows the tilted card's own corners.
    if border {
        ui.painter().add(egui::Shape::closed_line(
            corners,
            egui::Stroke::new(3.0, Color32::from_white_alpha(230)),
        ));
    }
    true
}

fn swirl_scene(app: &mut App, ui: &egui::Ui, rect: Rect, now: Option<&NowPlaying>) {
    let painter = ui.painter().with_clip_rect(rect);
    let palette = app.palette;
    let Some(now) = now else {
        return;
    };
    let resting = fullscreen_cover(rect, 0);
    // The art bounces to the music, as the full screen lyrics' does, when
    // that is switched on: the card swells a little with the bass while the
    // words keep to its resting size.
    let swell = if app.settings.lyrics_flag(crate::settings::Settings::LYRICS_BOUNCE_ART) {
        let target = app.music_bass_level();
        ui.ctx().request_repaint();
        ui.ctx()
            .animate_value_with_time(egui::Id::new("vis-cover-bounce"), target, 0.07)
    } else {
        0.0
    };
    let cover = Rect::from_center_size(resting.center(), resting.size() * (1.0 + 0.07 * swell));
    let loader = app.backend.art().clone();
    // Floating and tilting to the pointer, the same card as the full-screen
    // lyrics have.
    let floating = app.settings.lyrics_flag(crate::settings::Settings::LYRICS_FLOAT_ART);
    let url = now.art_url.clone().or_else(|| now.art_small.clone());
    let tilted = tilted_cover(ui, cover, url.as_deref(), floating, app.settings.vis_art_border);
    if !tilted {
        super::widgets::paint_cover(
            ui,
            &palette,
            url.as_deref(),
            cover,
            0.0,
            Icon::Music,
            Some(&loader),
        );
    }
    big_art_missing_mark(app, ui, cover, "fullscreen-vis");
    if app.settings.vis_art_border && !tilted {
        painter.rect_stroke(
            cover.expand(1.5),
            3.0,
            egui::Stroke::new(3.0, Color32::from_white_alpha(230)),
            egui::StrokeKind::Middle,
        );
    }
    // With the controls up, the song's name lives on their card (above the
    // controls), so it is not written a second time under the art. The extra
    // visualizer window has its own controls card too.
    let in_extra: bool = ui.ctx().data(|data| data.get_temp(egui::Id::new("in-extra-window"))).unwrap_or(false);
    let _ = in_extra;
    if app.fullscreen_vis
        && app.float_slot(super::views_panel::default_float(ui.ctx())).is_some()
    {
        return;
    }
    // The words keep to the card's resting size, and sit below the lowest the
    // art can reach: a bounce swells it, and a floating card tips towards the
    // viewer and grows past its edge.
    let cover = resting;
    let clear = cover.height()
        * (if app.settings.lyrics_flag(crate::settings::Settings::LYRICS_BOUNCE_ART) { 0.04 } else { 0.0 }
            + if floating { 0.18 } else { 0.0 });
    let ink = if app.swirl_art_is_light() {
        Color32::BLACK
    } else {
        Color32::WHITE
    };
    // The face: Inter, or one of the installed ones, loaded the first time
    // it is asked for.
    let font_index = vis_font_in_use(ui.ctx(), app.settings.vis_text_font, app.settings.vis_font_random, &now.uri);
    let family = vis_font_family(ui.ctx(), font_index);
    // Where the words go depends on the layout the reader chose: under the
    // cover (0), beside it (1), beside it and big (2), or above it with the
    // artist below (3).
    let layout = 0;
    let beside = layout == 1 || layout == 2;
    let title_text = now.title.to_uppercase();
    let chars: Vec<char> = title_text.chars().collect();
    let mut title_size = match layout {
        1 => (rect.height() * 0.065).clamp(22.0, 80.0),
        2 => (rect.height() * 0.115).clamp(34.0, 150.0),
        _ => fullscreen_title_size(rect),
    };
    let text_left = cover.right() + (rect.width() * 0.03).clamp(18.0, 48.0);
    let (max_width, max_lines) = if beside {
        (
            (rect.right() - text_left - rect.width() * 0.05).max(80.0),
            if layout == 2 { 3 } else { 2 },
        )
    } else {
        (
            (cover.width() * 1.3)
                .min(2.0 * (cover.center().x - rect.left() - 12.0))
                .max(80.0),
            1,
        )
    };
    let format = |size: f32, alpha: f32| egui::TextFormat {
        font_id: egui::FontId::new(size, family.clone()),
        color: ink.gamma_multiply(alpha),
        italics: family == egui::FontFamily::Proportional,
        ..Default::default()
    };
    // One galley per letter, so each can bend about its own foot.
    let letters = |size: f32| -> Vec<std::sync::Arc<egui::Galley>> {
        chars
            .iter()
            .map(|letter| {
                painter.layout_job(egui::text::LayoutJob::single_section(
                    letter.to_string(),
                    format(size, 0.6),
                ))
            })
            .collect()
    };
    // The words are broken into lines from the letters' widths at the
    // starting size; a width grows in step with the size, so shrinking is
    // done on the numbers and the letters are laid out once, at the end.
    let widths: Vec<f32> = letters(title_size).iter().map(|glyph| glyph.size().x).collect();
    let wrap = |scale: f32| -> Vec<(usize, usize)> {
        let mut lines: Vec<(usize, usize)> = Vec::new();
        let mut first = 0usize;
        while first < chars.len() {
            let mut last = first;
            let mut used = 0.0_f32;
            let mut space = None;
            while last < chars.len() {
                let width = widths[last] * scale;
                if used + width > max_width && last > first {
                    break;
                }
                used += width;
                if chars[last] == ' ' {
                    space = Some(last);
                }
                last += 1;
            }
            let next = if last < chars.len() {
                space.map_or(last, |at| at + 1)
            } else {
                last
            };
            let mut shown = next;
            while shown > first && chars[shown - 1] == ' ' {
                shown -= 1;
            }
            if shown > first {
                lines.push((first, shown));
            }
            first = next.max(first + 1);
        }
        lines
    };
    // With the lyrics showing beside the cover, the words keep to the upper
    // half of that column and the lyrics take the lower half.
    let lyrics_shown = app.settings.vis_lyrics
        && matches!(
            &app.lyrics,
            crate::model::Loadable::Loaded(Some(lyrics)) if lyrics.synced && !lyrics.instrumental
        );
    let budget = if beside && lyrics_shown {
        cover.height() * 0.46
    } else {
        f32::INFINITY
    };
    let fits = |count: usize, size: f32| {
        count <= max_lines && count as f32 * size * 1.3 + size * 0.65 + 8.0 <= budget
    };
    let mut scale = 1.0_f32;
    let mut lines = wrap(scale);
    let mut guard = 0;
    while !fits(lines.len(), title_size * scale) && guard < 40 && title_size * scale > 14.0 {
        scale *= 0.92;
        lines = wrap(scale);
        guard += 1;
    }
    lines.truncate(max_lines);
    title_size *= scale;
    let glyphs = letters(title_size);
    let line_h = glyphs.iter().map(|glyph| glyph.size().y).fold(title_size, f32::max);
    let line_width = |line: &(usize, usize)| -> f32 {
        glyphs[line.0..line.1].iter().map(|glyph| glyph.size().x).sum()
    };
    let time = ui.input(|input| input.time) as f32;
    // The bass makes it sway far harder: the low notes of what is playing,
    // smoothed a little so the letters lean into each hit rather than jitter.
    let bass = if now.playing && now.local {
        let samples = app.audio.tap.window(crate::vis::FFT_SAMPLES, crate::vis::LAG);
        let mut low = 0.0_f32;
        let mut sum = 0.0_f32;
        for x in &samples {
            low += 0.03 * (x - low);
            sum += low * low;
        }
        if samples.is_empty() {
            0.0
        } else {
            ((sum / samples.len() as f32).sqrt() * 4.0).clamp(0.0, 1.0)
        }
    } else {
        app.music_bass_level()
    };
    // Auto-gain: a quiet song or a low volume file moved the text far less
    // than a loud one. The loudest recent hit sets the scale (it slowly
    // forgets), so the text reacts to the shape of the music, not its level.
    let bass = {
        let id = egui::Id::new("title-bass-peak");
        let before: f32 = ui.ctx().data(|data| data.get_temp(id)).unwrap_or(0.3);
        let peak = bass.max(before * 0.998).max(0.3);
        ui.ctx().data_mut(|data| data.insert_temp(id, peak));
        (bass / peak * 0.9).clamp(0.0, 1.0)
    };
    let bass = ui
        .ctx()
        .animate_value_with_time(egui::Id::new("title-bass"), bass, 0.05);
    let still = app.settings.vis_text_still;
    let sway = app.settings.vis_sway_preset();
    // On the beat only: the title moves with each hit and holds still between.
    let gate = sway_gate(app);
    let strength = app.settings.vis_sway_strength() * gate;
    // How hard the music is hitting right now, 0 to 1: what the sways follow.
    // Quiet passages barely move it; only a hard hit gets near the top (the
    // curve is squared, so a half-strength hit is a quarter of the motion).
    let hit = ((bass - 0.25) / 0.6).clamp(0.0, 1.0);
    let punch = hit * hit;
    // Shake moves the whole text together, never letter by letter.
    let shake = if still || sway.kind != 4 {
        vec2(0.0, 0.0)
    } else {
        let reach = sway.amp * title_size * strength * punch * (0.5 + 0.9 * punch);
        vec2((time * 43.0).sin() * reach, (time * 37.0).cos() * reach - punch * 0.05 * title_size * strength)
    };
    let contrast = if ink == Color32::WHITE { Color32::BLACK } else { Color32::WHITE };
    let outline = !app.settings.vis_text_no_outline;
    // The artists as the song lists them; the subtitle is only a fallback
    // (it can be empty for a song whose artists are listed separately).
    let artist_line = if now.artists.is_empty() {
        now.subtitle.clone()
    } else {
        now.artists.iter().map(|artist| artist.name.as_str()).collect::<Vec<_>>().join(", ")
    };
    // The artist shows only while the pointer is over the cover or the words
    // under it, faded in so it grows out of the title.
    let hover_zone = Rect::from_min_max(
        pos2(cover.center().x - max_width / 2.0, cover.top()),
        pos2(cover.center().x + max_width / 2.0, cover.bottom() + clear + title_size * 3.0),
    );
    let hovered = ui
        .ctx()
        .input(|input| input.pointer.hover_pos())
        .is_some_and(|at| hover_zone.contains(at));
    let reveal = ui
        .ctx()
        .animate_bool_with_time(egui::Id::new("vis-artist-reveal"), hovered, 0.15);
    let show_artist = reveal > 0.02 && !artist_line.is_empty();
    let artist_size = (title_size * 0.5).max(12.0);
    let artist = show_artist.then(|| {
        let mut job = egui::text::LayoutJob::single_section(
            artist_line.clone(),
            format(artist_size, 0.75 * reveal),
        );
        job.wrap.max_width = max_width;
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        painter.layout_job(job)
    });
    let words_height = lines.len() as f32 * line_h;
    // Where the title's first line starts, and where the artist goes.
    let (top, artist_top) = match layout {
        // Above the cover, the artist below it.
        3 => (cover.top() - 18.0 - line_h, cover.bottom() + 14.0 + clear),
        // Beside it: the words as one block, centred on the cover.
        1 | 2 => {
            let block = words_height
                + artist.as_ref().map_or(0.0, |a| 8.0 + a.size().y);
            let top = if lyrics_shown {
                cover.top()
            } else {
                cover.center().y - block / 2.0
            };
            (top, top + words_height + 8.0)
        }
        // Under it, the artist beneath the title.
        _ => (cover.bottom() + 18.0 + clear, cover.bottom() + 18.0 + clear + line_h + 6.0),
    };
    // Each line's left edge: centred on the cover, or flush with the text
    // column beside it.
    let line_left = |width: f32| -> f32 {
        if beside {
            text_left
        } else {
            cover.center().x - width.min(max_width) / 2.0
        }
    };
    if app.settings.vis_text_back {
        // A backing only as big as the words on it: one strip behind each
        // line of the title and one behind the artist, or a single block
        // round all of it, never a panel over the picture.
        let back = contrast.gamma_multiply(app.settings.vis_back_alpha.clamp(0.1, 1.0));
        let pad = app.settings.vis_back_pad.clamp(0.0, 40.0);
        let round = app.settings.vis_back_round.clamp(0.0, 30.0);
        let mut strips: Vec<Rect> = Vec::new();
        for (row, line) in lines.iter().enumerate() {
            let width = line_width(line);
            let left = line_left(width);
            let y = top + row as f32 * line_h;
            let vpad = pad * 0.5 + 4.0;
            strips.push(Rect::from_min_max(
                pos2(left - pad, y - vpad),
                pos2(left + width + pad, y + line_h + vpad),
            ));
        }
        if let Some(artist) = artist.as_ref() {
            let left = line_left(artist.size().x);
            strips.push(Rect::from_min_max(
                pos2(left - pad, artist_top),
                pos2(left + artist.size().x + pad, artist_top + artist.size().y),
            ));
        }
        // Always one block round all of it.
        if let Some(first) = strips.first().copied() {
            let whole = strips.iter().fold(first, |all, strip| all.union(*strip));
            painter.rect_filled(whole.expand(2.0), round, back);
        }
    }
    let mut moving = false;
    for (row, line) in lines.iter().enumerate() {
        let whole_width = line_width(line);
        let mut x = line_left(whole_width);
        let line_top = top + row as f32 * line_h;
        // Sway: the whole line leans about the middle of its foot (or, for the
        // stalk styles, every letter leans about its own foot so only the
        // tops move). A long line leans less so its ends never swing far.
        let line_pivot = pos2(x + whole_width / 2.0, line_top + line_h);
        let back_on = app.settings.vis_text_back;
        let back_pad = app.settings.vis_back_pad.clamp(0.0, 40.0);
        let back_vpad = back_pad * 0.5 + 4.0;
        // With a backing, the letters may not lean past it.
        let max_lean = if back_on {
            // The ends of a long line swing up and down as it leans, so the
            // lean is capped where that stays inside the block.
            ((back_vpad + 1.0) / (whole_width / 2.0).max(1.0)).clamp(0.0, 1.0).asin()
        } else {
            0.6
        };
        let lean_at = |phase: f32| -> f32 {
            if still || matches!(sway.kind, 1 | 2 | 4) {
                return 0.0;
            }
            let damp = if sway.stalk > 0.0 {
                1.0
            } else {
                (420.0 / whole_width.max(1.0)).clamp(0.3, 1.0)
            };
            let t = time * sway.speed;
            let bass_lean = 0.6 + 4.0 * punch * sway.bass;
            (((0.05 * (t * 0.9 - phase * 0.6).sin() + 0.03 * (t * 1.7 - phase * 0.9 + 1.0).sin())
                * bass_lean
                + 0.07 * punch * sway.bass * (t * 4.1 - phase * 0.7).sin())
                * damp
                * sway.lean
                * strength)
                .clamp(-max_lean, max_lean)
        };
        let angle = if sway.stalk > 0.0 { 0.0 } else { lean_at(row as f32) };
        if !still && (sway.stalk > 0.0 || angle != 0.0 || sway.bob > 0.0 || sway.kind != 0) {
            moving = true;
        }
        let (sin, cos) = angle.sin_cos();
        let turn = |p: egui::Pos2| {
            let d = p - line_pivot;
            line_pivot + vec2(d.x * cos - d.y * sin, d.x * sin + d.y * cos)
        };
        let line_right = x + whole_width;
        let line_left_edge = x;
        for i in line.0..line.1 {
            let glyph = &glyphs[i];
            let w = glyph.size().x;
            let h = glyph.size().y;
            // Each letter bobbing on its own, for the wave-like sways.
            let mut bob = if still || sway.bob <= 0.0 {
                0.0
            } else {
                (time * 3.0 * sway.speed - i as f32 * 0.55).sin() * sway.bob * h * strength * (0.3 + 1.6 * punch)
            };
            if back_on {
                bob = bob.clamp(-back_vpad.max(3.0), back_vpad.max(3.0));
            }
            // Bounce and slide move letters without turning them. Slide moves a
            // whole line together so letters never run into each other, and
            // shake moves everything together.
            let motion = if still || matches!(sway.kind, 0 | 3) {
                vec2(0.0, 0.0)
            } else {
                let t = time * sway.speed;
                match sway.kind {
                    1 => {
                        let hop = (t * 2.4 - i as f32 * 0.08).sin().powi(2);
                        let lift = 0.2 * hop + punch * (0.8 + 0.2 * hop);
                        vec2(0.0, -lift * sway.amp * line_h * strength)
                    }
                    2 => vec2(
                        (t * 1.1 + row as f32 * 0.8).sin() * sway.amp * line_h * strength * (0.25 + punch),
                        0.0,
                    ),
                    _ => shake,
                }
            };
            let motion = if back_on {
                vec2(motion.x, motion.y.clamp(-back_vpad.max(3.0), back_vpad.max(3.0)))
            } else {
                motion
            };
            let origin = pos2(x, line_top + (line_h - h) + bob) + motion;
            // Stalk styles: this letter leans about its own foot, a little out
            // of step with its neighbour so the line ripples like wheat.
            let (angle, turn_here): (f32, Box<dyn Fn(egui::Pos2) -> egui::Pos2 + '_>) =
                if sway.stalk > 0.0 {
                    // Lean: every letter tilts together, so the line slants
                    // one way and then the other with its feet fixed.
                    let ripple = if sway.kind == 3 { 0.0 } else { 0.22 };
                    let a = lean_at(row as f32 + i as f32 * ripple);
                    let foot = pos2(x + w / 2.0, line_top + line_h + bob);
                    let (s2, c2) = a.sin_cos();
                    (
                        a,
                        Box::new(move |p: egui::Pos2| {
                            let d = p - foot;
                            foot + vec2(d.x * c2 - d.y * s2, d.x * s2 + d.y * c2)
                        }),
                    )
                } else {
                    (angle, Box::new(turn))
                };
            // Keep every letter on the line's own width when it has a backing.
            let keep = |mut p: egui::Pos2| {
                if back_on {
                    p.x = p.x.clamp(
                        line_left_edge - back_pad + 1.0,
                        (line_right + back_pad - w - 1.0).max(line_left_edge - back_pad + 1.0),
                    );
                }
                p
            };
            if outline {
                let reach = (h * 0.04).clamp(1.5, 4.0);
                for step in 0..8 {
                    let around = step as f32 * std::f32::consts::TAU / 8.0;
                    let mut shape = egui::epaint::TextShape::new(origin, glyph.clone(), ink);
                    shape.pos = keep(turn_here(origin + vec2(around.cos() * reach, around.sin() * reach)));
                    shape.angle = angle;
                    shape.override_text_color = Some(contrast.gamma_multiply(0.85));
                    painter.add(egui::Shape::Text(shape));
                }
            }
            let mut shape = egui::epaint::TextShape::new(origin, glyph.clone(), ink);
            shape.pos = keep(turn_here(origin));
            shape.angle = angle;
            painter.add(egui::Shape::Text(shape));
            x += w;
        }
    }
    if let Some(artist) = artist {
        let at = pos2(line_left(artist.size().x), artist_top) + shake;
        if outline {
            let reach = (artist_size * 0.05).clamp(1.0, 3.0);
            for step in 0..8 {
                let around = step as f32 * std::f32::consts::TAU / 8.0;
                let mut shape = egui::epaint::TextShape::new(
                    at + vec2(around.cos() * reach, around.sin() * reach),
                    artist.clone(),
                    ink,
                );
                shape.override_text_color = Some(contrast.gamma_multiply(0.85 * reveal));
                painter.add(egui::Shape::Text(shape));
            }
        }
        painter.galley(at, artist, ink);
    }
    if moving {
        ui.ctx().request_repaint();
    }
}

/// How much the foot of the bar is darkened under the cover, so the song
/// title in front of it can still be read.
const SWIRL_SHADE: f32 = 0.34;

/// How many readings one flow sheet is drawn from. Fixed, so the sheets
/// always have the same shape to fold into one another however many bars
/// the reader chose: the bar count is a bars setting, not a flow one.
const FLOW_POINTS: usize = 96;
/// The default number of sheets folded together, front to back.
pub(crate) const FLOW_SHEETS: usize = 5;
/// How far a sheet's crest travels sideways, as a fraction of the bar's
/// height, over one full cycle. Enough to read as movement rather than a
/// wobble, slow enough not to strobe.
const FLOW_SWING: f32 = 0.34;
/// How quickly the sheets drift past one another, in cycles a second,
/// before the reader's own speed multiplier.
const FLOW_DRIFT: f32 = 0.13;

/// How the flow is drawn: how many sheets, how tall the stack reaches, and
/// how fast it drifts. All three are the reader's to set, because the
/// defaults suit one bar and not another.
#[derive(Clone, Copy)]
struct FlowStyle {
    /// Which edges it rises from (1 top, 2 left, 4 bottom, 8 right).
    sides: u8,
    /// The whole picture's area, for the edges other than the bottom.
    full: Rect,
    sheets: usize,
    /// How far along the spectrum each sheet is shifted from the one in
    /// front of it, as a fraction of the whole run of readings. Zero has
    /// every sheet tracing the same shape, which is one thick ribbon; a
    /// fifth has them visibly out of step, so the stack folds rather than
    /// sitting on top of itself.
    offset: f32,
    /// How much of the bar's height the stack covers, from 0 to 1. At the
    /// default the sheets run the whole height rather than pooling in the
    /// bottom third and leaving the rest of the bar empty.
    depth: f32,
}

/// Sheets of colour folded over one another, each one a curve that rides on
/// the spectrum and swings slowly from side to side, in the spirit of a
/// fluid wallpaper rather than a row of separate bars.
///
/// The sheets are drawn back to front and fainter as they go, so the
/// nearest reads as the surface and the ones behind it show through
/// wherever they cross — which is where the marbling comes from. Each is
/// closed down to the foot of the bar, so the stack fills the space
/// rather than floating in it.
fn flow_sheets(
    painter: &egui::Painter,
    rect: Rect,
    levels: &[f32],
    colours: (Color32, Color32),
    strength: f32,
    phase: f32,
    style: FlowStyle,
) {
    // The bottom edge is the flow as it always was, in `rect`; any other
    // edge is the same picture turned to rise from that edge of `style.full`.
    let sides = if style.sides == 0 { 4 } else { style.sides };
    let full = style.full;
    for bit in [4u8, 1, 2, 8] {
        if sides & bit == 0 {
            continue;
        }
        let area = if bit == 4 { rect } else { full };
        let Some(mut mesh) = flow_mesh(area, levels, colours, strength, phase, style) else {
            continue;
        };
        if bit != 4 {
            for vertex in &mut mesh.vertices {
                // Across the edge (0 to 1) and how far it rises (0 to 1).
                let across = (vertex.pos.x - area.left()) / area.width();
                let rise = (area.bottom() - vertex.pos.y) / area.height();
                vertex.pos = match bit {
                    1 => pos2(vertex.pos.x, area.top() + rise * area.height()),
                    2 => pos2(area.left() + rise * area.width(), area.top() + across * area.height()),
                    _ => pos2(area.right() - rise * area.width(), area.top() + across * area.height()),
                };
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }
}

fn flow_mesh(
    rect: Rect,
    levels: &[f32],
    (low, high): (Color32, Color32),
    strength: f32,
    phase: f32,
    style: FlowStyle,
) -> Option<egui::Mesh> {
    if levels.is_empty() || rect.width() < 8.0 || rect.height() < 8.0 {
        return None;
    }
    let sheets = style.sheets.clamp(1, 12);
    let depth = style.depth.clamp(0.1, 1.4);
    let offset = style.offset.clamp(0.0, 0.5);
    let bass = levels.iter().take(levels.len() / 8).copied().sum::<f32>()
        / (levels.len() / 8).max(1) as f32;
    let mut mesh = egui::Mesh::default();
    // The bass glow along the foot, as the bars have, so the two modes
    // share a sense of weight.
    let pulse = Rect::from_min_max(pos2(rect.left(), rect.center().y), rect.right_bottom());
    shaded(
        &mut mesh,
        pulse,
        Color32::TRANSPARENT,
        low.gamma_multiply(FLOW_GLOW * strength * (0.4 + bass * bass)),
    );

    for sheet in 0..sheets {
        // Sheets further back sit higher and swing the other way, so the
        // stack crosses itself instead of moving as one slab.
        let back = sheet as f32 / (sheets - 1).max(1) as f32;
        let direction = if sheet % 2 == 0 { 1.0 } else { -1.0 };
        let swing = (phase * std::f32::consts::TAU + back * 1.7) * direction;
        // The stack climbs from the foot to near the top of the bar, and
        // each sheet's own crest can rise most of the way out of the
        // space left for it. Anything less leaves the upper half of the
        // bar showing nothing but background.
        let rest = rect.bottom() - rect.height() * (0.04 + back * depth * 0.62);
        let reach = rect.height() * depth * (0.72 - back * 0.3);
        let alpha = strength * (0.5 - back * 0.3).max(0.08);
        let base = mesh.vertices.len() as u32;
        // The crest: one point per reading, walked left to right. Each
        // sheet reads the spectrum a little further along than the one in
        // front, which is what makes a stack of them look like one body of
        // water folded over itself rather than a single shape drawn six
        // times.
        let shift = ((sheet as f32 * offset) * levels.len() as f32).round() as usize % levels.len();
        for (index, _) in levels.iter().enumerate() {
            let across = index as f32 / (levels.len() - 1).max(1) as f32;
            let x = rect.left() + across * rect.width();
            let level = levels[(index + shift) % levels.len()];
            // The swing is a slow wave along the bar, not a slide: each
            // sheet is a ribbon of moving water rather than a bar chart
            // leaning about.
            let wave = (across * std::f32::consts::TAU * 1.3 + swing).sin();
            let crest = rest
                - level * reach * (0.75 + 0.25 * wave)
                - wave * rect.height() * FLOW_SWING * 0.28;
            mesh.colored_vertex(
                pos2(x, crest),
                low.lerp_to_gamma(high, across).gamma_multiply(alpha),
            );
        }
        // A second point per reading, along the foot of the bar, so each
        // pair of readings makes a quad: a filled shape rather than a
        // line, and one that reaches all the way down.
        let foot = mesh.vertices.len() as u32;
        for index in 0..levels.len() {
            let across = index as f32 / (levels.len() - 1).max(1) as f32;
            mesh.colored_vertex(
                pos2(rect.left() + across * rect.width(), rect.bottom()),
                Color32::TRANSPARENT,
            );
        }
        for index in 0..levels.len() - 1 {
            let step = index as u32;
            let crest = base + step;
            mesh.add_triangle(crest, crest + 1, foot + step + 1);
            mesh.add_triangle(crest, foot + step + 1, foot + step);
        }
    }
    Some(mesh)
}

/// The visualizer's two colours: the cover's own for the bass, and the same
/// turned a sixth of the way round the colour wheel for the treble. Both
/// are made vivid, then kept to a band of brightness, so the words the bars
/// pass behind always stand out: no lighter than [`DARK_CEILING`] under the
/// dark theme's white text, no darker than [`LIGHT_FLOOR`] under the light
/// theme's dark text.
///
/// The treble is not simply the bass brightened. A single cover gives one
/// accent, and spreading that over the whole bar as a straight lerp to a
/// neighbour reads as one flat colour that happens to fade. Instead the
/// sweep walks the hue a quarter of the way round while dropping the
/// saturation and pushing the value, which is roughly how a colour reads
/// when it moves from the shadowed part of a picture into its lit part —
/// so the bar carries a second colour rather than a second brightness of
/// the first.
pub(crate) fn vis_colours(base: Color32, dark: bool) -> (Color32, Color32) {
    let source = egui::ecolor::Hsva::from(base);
    // A cover with no colour in it — the black-and-white ones, the greys,
    // the line drawings — has no hue to honour and no saturation to keep.
    // Inventing one here was the bug: a monochrome cover came out red or
    // green, which is the opposite of respecting the album art. Below the
    // floor the bar stays monochrome and sweeps in brightness only, which
    // is what such a cover looks like.
    let monochrome = source.s < MONOCHROME_SATURATION;
    let mut low = source;
    if monochrome {
        // No hue to turn, so the treble end is lighter rather than a
        // different colour: a grey sweep, not a grey turning pink.
        //
        // Both ends sit *under* the brightness ceiling rather than above
        // it. Above it, `within_brightness` scales each one down to the
        // ceiling separately, which lands both on the same grey and leaves
        // the bar a flat slab with nothing to sweep between.
        low.v = if dark { 0.34 } else { 0.38 };
        low.s = 0.0;
    } else {
        // A washed-out but genuinely coloured cover still has to read as a
        // colour here, so the floor is well above what the cover may be.
        low.s = low.s.max(0.62);
        low.v = 1.0;
    }
    low.a = 1.0;
    let mut high = low;
    if monochrome {
        high.v = if dark { 0.14 } else { 0.24 };
    } else {
        // A quarter turn rather than a sixth: a sixth lands on the same
        // neighbouring pair of hues for a third of the wheel's worth of
        // covers and is barely a change at all.
        high.h = (high.h + 0.25).fract();
        // Less saturated and slightly darker at the treble end, so the two
        // ends are separable by eye and not only by position.
        high.s = (high.s * 0.82).max(0.3);
        high.v = if dark { 0.92 } else { 1.0 };
    }
    let keep = |colour: egui::ecolor::Hsva| within_brightness(Color32::from(colour), dark);
    (keep(low), keep(high))
}

/// How little saturation a cover may carry before its visualizer is drawn
/// without a hue. Low enough that a faintly tinted print still counts as
/// the colour it is, high enough that a black-and-white cover never picks
/// one up off a single stray pixel.
const MONOCHROME_SATURATION: f32 = 0.08;

/// The most luminance a visualizer colour has under the dark theme, and the
/// band it is kept in under the light theme, as relative luminance from 0
/// to 1. On white the colour must be dark enough to show at all yet light
/// enough that dark words stay clear over it.
const DARK_CEILING: f32 = 0.35;
const LIGHT_FLOOR: f32 = 0.2;
const LIGHT_CEILING: f32 = 0.4;

/// `colour` with its hue kept and its brightness moved into the band the
/// theme's text reads against: darkened towards black when too light,
/// lightened towards white when too dark.
fn within_brightness(colour: Color32, dark: bool) -> Color32 {
    let linear = egui::Rgba::from(colour);
    let luminance = 0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b();
    let ceiling = if dark { DARK_CEILING } else { LIGHT_CEILING };
    let adjusted = if luminance > ceiling {
        linear * (ceiling / luminance)
    } else if !dark && luminance < LIGHT_FLOOR {
        let toward_white = (LIGHT_FLOOR - luminance) / (1.0 - luminance).max(f32::EPSILON);
        egui::Rgba::from_rgb(
            linear.r() + (1.0 - linear.r()) * toward_white,
            linear.g() + (1.0 - linear.g()) * toward_white,
            linear.b() + (1.0 - linear.b()) * toward_white,
        )
    } else {
        linear
    };
    Color32::from(egui::Rgba::from_rgb(
        adjusted.r(),
        adjusted.g(),
        adjusted.b(),
    ))
}

/// Adds a rectangle shaded from `top` to `bottom`.
fn shaded(mesh: &mut egui::Mesh, rect: Rect, top: Color32, bottom: Color32) {
    let base = mesh.vertices.len() as u32;
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(base, base + 1, base + 2);
    mesh.add_triangle(base, base + 2, base + 3);
}

/// Bars rising from the foot of the player bar, swept from the bass colour
/// to the treble colour, with a soft glow, a peak cap hanging above each,
/// and the whole foot of the bar breathing with the bass.
fn spectrum(
    painter: &egui::Painter,
    rect: Rect,
    levels: &[f32],
    peaks: &[f32],
    (low, high): (Color32, Color32),
    strength: f32,
    gap: f32,
) {
    let bands = levels.len() as f32;
    let width = ((rect.width() - gap * (bands - 1.0)) / bands).max(1.0);
    // A full band reaches the top, with room above for its cap.
    let reach = rect.height() - PEAK_GAP - PEAK_HEIGHT - 1.0;
    let mut mesh = egui::Mesh::default();

    // The bass pulse: a glow along the foot, as strong as the low bands.
    let bass = levels.iter().take(levels.len() / 8).copied().sum::<f32>()
        / (levels.len() / 8).max(1) as f32;
    let pulse = Rect::from_min_max(pos2(rect.left(), rect.center().y), rect.right_bottom());
    shaded(
        &mut mesh,
        pulse,
        Color32::TRANSPARENT,
        low.gamma_multiply(PULSE_ALPHA * strength * bass * bass),
    );

    for (index, (level, peak)) in levels.iter().zip(peaks).enumerate() {
        let left = rect.left() + index as f32 * (width + gap);
        let colour = low.lerp_to_gamma(high, index as f32 / (bands - 1.0));
        let height = level * reach;
        if height >= 1.0 {
            let bar = Rect::from_min_max(
                pos2(left, rect.bottom() - height),
                pos2(left + width, rect.bottom()),
            );
            // The glow: the bar again, wider and fainter.
            shaded(
                &mut mesh,
                bar.expand2(vec2(gap, 3.0)),
                colour.gamma_multiply(GLOW_ALPHA * strength * 0.3),
                colour.gamma_multiply(GLOW_ALPHA * strength),
            );
            let (foot, top) = (SPECTRUM_ALPHA.0 * strength, SPECTRUM_ALPHA.1 * strength);
            shaded(
                &mut mesh,
                bar,
                colour.gamma_multiply(foot + (top - foot) * level),
                colour.gamma_multiply(foot),
            );
        }
        // No cap that hangs at the top after the sound has dropped.
        let cap = 0.0 * peak * reach;
        if cap >= 2.0 {
            let y = rect.bottom() - cap - PEAK_GAP;
            shaded(
                &mut mesh,
                Rect::from_min_max(pos2(left, y - PEAK_HEIGHT), pos2(left + width, y)),
                colour.gamma_multiply(PEAK_ALPHA),
                colour.gamma_multiply(PEAK_ALPHA),
            );
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// The wave as a neon line in layers of glow, swept from the bass colour to
/// the treble colour, over a faint shading down to the midline and a dim
/// mirrored echo.
fn waveform(
    painter: &egui::Painter,
    rect: Rect,
    trace: &[f32],
    (low, high): (Color32, Color32),
    strength: f32,
) {
    if trace.len() < 2 {
        return;
    }
    let reach = rect.height() * 0.46;
    let step = rect.width() / (trace.len() - 1) as f32;
    let midline = rect.center().y;
    let point =
        |index: usize, value: f32| pos2(rect.left() + index as f32 * step, midline - value * reach);
    let colour_at = |index: usize| low.lerp_to_gamma(high, index as f32 / (trace.len() - 1) as f32);

    // The shading between the wave and the midline.
    let mut mesh = egui::Mesh::default();
    for index in 0..trace.len() - 1 {
        let colour = colour_at(index).gamma_multiply(WAVE_FILL_ALPHA * strength);
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(point(index, trace[index]), colour);
        mesh.colored_vertex(point(index + 1, trace[index + 1]), colour);
        mesh.colored_vertex(pos2(point(index + 1, 0.0).x, midline), Color32::TRANSPARENT);
        mesh.colored_vertex(pos2(point(index, 0.0).x, midline), Color32::TRANSPARENT);
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(egui::Shape::mesh(mesh));

    // Short runs of line, so the colour can sweep along the wave.
    let run = 8;
    let stroke = |mirror: f32, width: f32, alpha: f32| {
        let mut start = 0;
        while start < trace.len() - 1 {
            let end = (start + run).min(trace.len() - 1);
            let points = (start..=end)
                .map(|index| point(index, trace[index] * mirror))
                .collect();
            let colour = colour_at((start + end) / 2).gamma_multiply(alpha);
            painter.add(egui::Shape::line(points, egui::Stroke::new(width, colour)));
            start = end;
        }
    };
    stroke(-0.6, 1.0, WAVE_ECHO_ALPHA);
    for (width, alpha) in WAVE_LAYERS {
        stroke(1.0, width, alpha);
    }
}

/// Ease the final fill's RGB. Untinted custom panels keep their alpha while
/// the colour returns to the panel colour.
fn eased_fill(ctx: &egui::Context, panel: Color32, tint: Option<Color32>) -> Color32 {
    let target = tint.map_or(panel, |tint| super::blend(panel, tint, TINT_STRENGTH));
    // Color32 stores premultiplied RGB, so interpolate unmultiplied channels.
    let [r, g, b, _] = target.to_srgba_unmultiplied();
    // A new pass after sign-out gets new ids without clearing other animations.
    let pass = ctx.cumulative_pass_nr();
    let session = ctx.data_mut(|data| {
        *data.get_temp_mut_or_insert_with(egui::Id::new(TINT_SESSION_ID), || pass)
    });
    let channel = |axis: &'static str, value: u8| {
        ctx.animate_value_with_time(
            egui::Id::new(("player-bar-tint", session, axis)),
            f32::from(value),
            TINT_FADE_SECONDS,
        )
        .round() as u8
    };
    let eased = [channel("r", r), channel("g", g), channel("b", b)];
    if eased == [r, g, b] {
        target
    } else {
        Color32::from_rgba_unmultiplied(eased[0], eased[1], eased[2], target.a())
    }
}

/// The colour of the like heart: the cover's own colour when the interface
/// is following covers, the accent otherwise.
fn heart_colour(app: &App) -> Color32 {
    app.now_playing_tint().unwrap_or(app.palette.accent)
}

/// A heart for the playing song, drawn at `centre`. Filled in the cover's
/// colour when the song is in Liked Songs; a click saves or removes it.
pub(super) fn like_heart(app: &mut App, ui: &mut egui::Ui, id: &str, centre: egui::Pos2, size: f32, uri: &str) {
    let saved = app.is_saved(uri) == Some(true);
    let rect = Rect::from_center_size(centre, Vec2::splat(size + 10.0));
    let response = ui
        .interact(rect, egui::Id::new(("np-heart", id)), Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let palette = app.palette;
    let (icon, colour) = if saved {
        (Icon::HeartFilled, heart_colour(app))
    } else if response.hovered() {
        (Icon::Heart, palette.text)
    } else {
        (Icon::Heart, palette.secondary)
    };
    theme::paint_icon(ui, icon, rect, size, colour);
    let tip = if saved { "Remove from Liked Songs" } else { "Save to Liked Songs" };
    if response.on_hover_text(tip).clicked() {
        app.actions.push(Action::ToggleSaved(uri.to_string()));
    }
}

pub(super) fn now_playing_block(app: &mut App, ui: &mut egui::Ui, region: Rect, now: Option<&NowPlaying>) {
    // The big lyrics screen already shows the cover, so the small one goes.
    if app.lyrics_fullscreen.is_some() {
        return;
    }
    let palette = app.palette;
    let cy = region.center().y;
    // The cover is never taller than the row it sits in, so it cannot spill
    // up or down onto the visualizer.
    let cover_side = (region.height() - 8.0).clamp(30.0, 80.0);
    let cover_rect = Rect::from_min_size(
        pos2(region.left() + 4.0, cy - cover_side / 2.0),
        Vec2::splat(cover_side),
    );
    // With the cover already sitting large at the bottom of the sidebar,
    // a second copy of it here says the same thing twice in two sizes.
    let cover_shown = !app.settings.art_expanded || !app.settings.sidebar_visible;

    let Some(now) = now else {
        super::widgets::paint_cover(ui, &palette, None, cover_rect, 6.0, Icon::Music, None);
        let text_left = cover_rect.right() + 12.0;
        let text_rect = Rect::from_min_size(
            pos2(text_left, cy - 17.0),
            vec2((region.right() - text_left - 8.0).max(40.0), 34.0),
        );
        let mut text_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(text_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        text_ui.spacing_mut().item_spacing.y = 2.0;
        theme::text(
            &mut text_ui,
            gettext(app.locale, "Nothing playing"),
            theme::medium(theme::BODY_TEXT),
            palette.secondary,
        );
        theme::text(
            &mut text_ui,
            gettext(app.locale, "Pick a song, album, or playlist"),
            theme::regular(theme::SMALL_TEXT),
            palette.dim,
        );
        return;
    };

    if cover_shown {
        super::widgets::paint_sticky_cover(
            ui,
            &palette,
            "bar-cover",
            now.art_small.as_deref().or(now.art_url.as_deref()),
            cover_rect,
            6.0,
            Icon::Music,
            app.backend.art(),
        );
    } else {
        // The big art is showing in the sidebar, and the words sit on it
        // there. Repeating them here as well would say the same thing
        // twice, so the bar's own block is left empty and the visualizer
        // runs through it.
        return;
    }
    let song = app.now_playing_item();
    let drag_sense = if song.is_some() {
        Sense::click_and_drag()
    } else {
        Sense::click()
    };
    let cover_response = ui
        .interact(cover_rect, egui::Id::new("now-playing-cover"), drag_sense)
        .on_hover_cursor(if song.is_some() {
            egui::CursorIcon::Grab
        } else {
            egui::CursorIcon::Default
        });
    // Last.fm has no cover for this album: a small mark on the corner. The
    // cover is saved in the lastfm-art folder, ready to upload.
    if app.art_missing_now && app.settings.missing_mark_big && song.is_some() {
        let badge = Rect::from_center_size(cover_rect.right_top() + vec2(-6.0, 6.0), vec2(14.0, 14.0));
        ui.painter().circle_filled(badge.center(), 7.0, palette.warning);
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            "!",
            theme::bold(theme::TINY_TEXT),
            palette.window,
        );
        ui.interact(badge, egui::Id::new("lastfm-missing-badge"), Sense::hover()).on_hover_text(
            "Last.fm has no cover for this album. It is saved in the lastfm-art folder, ready to upload.",
        );
    }
    // Hovering the cover offers to dock the art large at the sidebar's
    // bottom, the way Spotify expands it. (#92)
    // No expand arrow: the A key toggles the big picture. Clicking the cover
    // jumps to the song's place in the playlist it is playing from.
    if cover_response.clicked() {
        if let Some(item) = &song {
            app.actions.push(Action::RevealSong {
                uri: item.uri().to_string(),
            });
        } else if let Some(id) = &now.album_id {
            app.actions.push(Action::Open(Page::Album(id.clone())));
        } else if let Some(id) = &now.show_id {
            app.actions.push(Action::Open(Page::Show(id.clone())));
        }
    }
    // A heart beside the cover, filled when the song is in Liked Songs.
    let heart_room = match &song {
        Some(item) if item.is_track() && region.right() - cover_rect.right() > 150.0 => 26.0,
        _ => 0.0,
    };
    if heart_room > 0.0
        && let Some(item) = &song
    {
        like_heart(
            app,
            ui,
            "bar",
            pos2(cover_rect.right() + 8.0 + heart_room / 2.0 - 4.0, cy),
            16.0,
            item.uri(),
        );
    }
    let text_left = cover_rect.right() + 12.0 + heart_room;
    let text_width = (region.right() - text_left).max(40.0);
    // Big and centred by default; on mouse-over the title shrinks and the
    // artist appears under it.
    let hovering_block = ui.rect_contains_pointer(region);
    let shown = ui.ctx().animate_bool_with_time(egui::Id::new("np-title-hover"), hovering_block, 0.12);
    let top_offset = 15.0 + 3.0 * shown;
    let text_rect = Rect::from_min_size(pos2(text_left, cy - top_offset), vec2(text_width, 44.0));
    let info_response = ui.interact(text_rect, egui::Id::new("now-playing-info"), drag_sense);
    let mut text_ui = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
    text_ui.spacing_mut().item_spacing.y = 2.0;
    // The name scales down to fit the room beside the cover.
    // The biggest size at which the whole name fits, measured with the real
    // font rather than guessed from the number of letters, so a name that
    // fits is never shortened. Only a name too long even at the smallest
    // size is cut.
    let scale = app.settings.controls_scale_value().clamp(0.85, 1.4);
    let largest_fit = |lo: f32, hi: f32| {
        let mut size = hi;
        while size > lo {
            let width = ui
                .painter()
                .layout_no_wrap(now.title.clone(), theme::medium(size), palette.text)
                .size()
                .x;
            if width <= text_width - 4.0 {
                break;
            }
            size -= 0.5;
        }
        size.max(lo)
    };
    let small = largest_fit(8.0, 16.5 * scale);
    let big = largest_fit(8.5, 24.0 * scale);
    let title_size = big + (small - big) * shown;
    let title_response =
        theme::link(&mut text_ui, &now.title, theme::medium(title_size), palette.text);
    // Shift + right-click copies the song name.
    if title_response.secondary_clicked() && ui.input(|input| input.modifiers.shift) {
        ui.ctx().copy_text(now.title.clone());
        app.toast("Song name copied");
    }
    if title_response.clicked() {
        // Playing from a list: jump to the song's place in it.
        let from_list = app
            .playing_context_uri()
            .and_then(|context| Page::from_uri(&context))
            .is_some_and(|page| matches!(page, Page::Playlist(_) | Page::Album(_)));
        if from_list && !now.uri.is_empty() {
            app.actions.push(Action::RevealSong { uri: now.uri.clone() });
        } else if let Some(id) = &now.album_id {
            app.actions.push(Action::Open(Page::Album(id.clone())));
        } else if let Some(id) = &now.show_id {
            app.actions.push(Action::Open(Page::Show(id.clone())));
        }
    }
    // In the small mode the artist only shows while the pointer is over it.
    let show_artist = ui.rect_contains_pointer(region);
    text_ui.horizontal_top(|ui| {
        if !show_artist {
            return;
        }
        if now.artists.is_empty() {
            if theme::link(ui, &now.subtitle, theme::regular(theme::BODY_TEXT), palette.secondary).clicked()
                && let Some(id) = &now.show_id
            {
                app.actions.push(Action::Open(Page::Show(id.clone())));
            }
        } else {
            super::widgets::artist_links(
                ui,
                app,
                &now.artists,
                theme::regular(theme::BODY_TEXT),
                palette.secondary,
            );
        }
    });
    if (cover_response.drag_started_by(egui::PointerButton::Primary)
        || info_response.drag_started_by(egui::PointerButton::Primary))
        && let Some(item) = &song
    {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DragTrack {
                title: item.name().to_string(),
                image: item.image(64).map(str::to_string),
                items: vec![item.clone()],
                from: None,
                source_playlist: None,
            },
        );
    }

    // The playing thing answers the same right-click menu as a table row,
    // from the cover, the empty space around the words, or the words.
    if let Some(item) = song {
        let context = app.editable_context_playlist();
        for response in [&cover_response, &info_response, &title_response] {
            egui::Popup::context_menu(response)
                .frame(super::widgets::menu_frame(&palette))
                .show(|ui| super::widgets::item_menu(ui, app, &item, context.as_ref(), None));
        }
    }
}

/// The song's words on a small card resting on the foot of the big album art.
///
/// The card is only as big as the words in it. It used to be a full-width
/// slab a third of the picture tall with the title pinned to its top edge,
/// which left the text floating in a box much bigger than it. Now the title
/// is laid out first and the card is wrapped around the result: a short name
/// gets a large size and a snug card, a long one drops to a smaller size and
/// two lines, and the card follows the text either way. The artists grow in
/// underneath when the pointer is over the art, and the card grows with them.
pub fn now_playing_overlay(app: &mut App, ui: &mut egui::Ui, art: Rect, now: &NowPlaying) {
    let palette = app.palette;
    if art.height() < 96.0 {
        return;
    }

    // The picture's own click target comes FIRST. egui gives a click to the
    // widget added last where several overlap, so one added after the title
    // and the artist links lay on top of them and took every click: the title
    // could not be pressed, and neither could an artist. Added first, it sits
    // underneath and only gets the clicks nothing else wants.
    let art_response = ui.interact(art, egui::Id::new("sidebar-art-overlay"), Sense::click_and_drag());

    let margin = 6.0;
    let pad_x = 12.0;
    let pad_y = 7.0;
    let gap = 3.0;
    let liked_item = app.now_playing_item().filter(|item| item.is_track());
    let heart_extra = if liked_item.is_some() { 30.0 } else { 0.0 };
    let max_text_width = (art.width() - margin * 2.0 - pad_x * 2.0 - heart_extra).max(40.0);

    // The title is sized to fit, and the card to the title.
    let biggest = (art.width() * 0.2).clamp(20.0, 56.0);
    // The same face as the visualizer's title, and it sways the same way.
    let font_index = vis_font_in_use(ui.ctx(), app.settings.vis_text_font, app.settings.vis_font_random, &now.uri);
    let family = vis_font_family(ui.ctx(), font_index);
    let (title_galley, title_size) = fit_title(
        ui.painter(),
        &now.title,
        max_text_width,
        biggest,
        palette.text,
        &family,
    );

    // The artists show while the pointer is over the art, faded in over a
    // fraction of a second so they grow out of the title instead of
    // blinking in under it.
    let revealed = ui.rect_contains_pointer(art);
    let detail = ui.ctx().animate_value_with_time(
        ui.id().with("now-playing-detail"),
        if revealed { 1.0 } else { 0.0 },
        0.15,
    );
    let sub_size = (title_size * 0.42).clamp(12.0, 16.0);
    let sub_height = sub_size * 1.3;
    let artists_text = if now.artists.is_empty() {
        now.subtitle.clone()
    } else {
        now.artists
            .iter()
            .map(|artist| artist.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let artists_width = ui
        .painter()
        .layout_no_wrap(artists_text, theme::regular(sub_size), palette.secondary)
        .size()
        .x
        .min(max_text_width);

    let title_extent = title_galley.size();
    let card_width = (title_extent.x + heart_extra).max(artists_width * detail) + pad_x * 2.0;
    let card_height = title_extent.y + pad_y * 2.0 + (sub_height + gap) * detail;
    let card = Rect::from_min_size(
        pos2(art.left() + margin, art.bottom() - margin - card_height),
        vec2(card_width, card_height),
    );
    let radius = egui::CornerRadius::same(8);
    // A shadow under the card, so it sits above the art rather than in it.
    ui.painter().add(
        egui::epaint::Shadow {
            offset: [0, 3],
            blur: 12,
            spread: 0,
            color: egui::Color32::from_black_alpha(if palette.dark { 130 } else { 70 }),
        }
        .as_shape(card, radius),
    );
    // Translucent, so the cover is still the picture behind the words.
    ui.painter()
        .rect_filled(card, radius, palette.panel.gamma_multiply(0.82));
    ui.painter().rect_stroke(
        card,
        radius,
        egui::Stroke::new(1.0, palette.outline),
        egui::StrokeKind::Inside,
    );

    // The title: painted from its own galley, with its own click target
    // above the picture's.
    let title_pos = pos2(card.left() + pad_x, card.top() + pad_y);
    let title_rect = Rect::from_min_size(title_pos, title_extent);
    let title = ui
        .interact(title_rect, egui::Id::new("now-playing-card-title"), Sense::click_and_drag())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    {
        let sway = app.settings.vis_sway_preset();
        let beat_gate = sway_gate(app);
        let strength = app.settings.vis_sway_strength() * beat_gate;
        let still = app.settings.vis_text_still;
        let angle = if still || matches!(sway.kind, 1 | 2 | 4) {
            0.0
        } else {
            let time = ui.input(|input| input.time) as f32;
            let bass = app.music_bass_level();
            let t = time * sway.speed;
            let bass_lean = 1.0 + 2.0 * bass * sway.bass;
            let damp = (420.0 / title_extent.x.max(1.0)).clamp(0.3, 1.0);
            ((0.05 * (t * 0.9).sin() + 0.03 * (t * 1.7 + 1.0).sin()) * bass_lean
                + 0.04 * bass * sway.bass * (t * 4.1).sin())
                * damp
                * sway.lean
                * strength
        };
        // Bounce, slide and shake move the whole title without turning it.
        let motion = if still || matches!(sway.kind, 0 | 3) {
            vec2(0.0, 0.0)
        } else {
            let time = ui.input(|input| input.time) as f32;
            let t = time * sway.speed;
            let reach = title_extent.y * sway.amp * strength;
            let punch = ((app.music_bass_level() - 0.05) / 0.35).clamp(0.0, 1.0);
            let drive = if sway.kind == 4 { punch * punch } else { 0.25 + 0.75 * punch };
            match sway.kind {
                1 => vec2(0.0, -(t * 2.4).sin().abs() * reach * drive),
                2 => vec2((t * 1.6).sin() * reach * (0.6 + 0.4 * drive), 0.0),
                _ => vec2((time * 41.0).sin() * reach * drive, (time * 37.0).cos() * reach * drive),
            }
        };
        // The title stays inside the card however it sways: the tilt is eased
        // until the turned words fit, then the words are slid back inside.
        let size = title_galley.size();
        let bounds = card.shrink(2.0);
        let corners = |angle: f32, at: egui::Pos2| -> Rect {
            let pivot = at + vec2(size.x / 2.0, size.y);
            let (s, c) = angle.sin_cos();
            let turned = [vec2(0.0, 0.0), vec2(size.x, 0.0), vec2(0.0, size.y), vec2(size.x, size.y)]
                .map(|corner| {
                    let d = at + corner - pivot;
                    pivot + vec2(d.x * c - d.y * s, d.x * s + d.y * c)
                });
            Rect::from_points(&turned)
        };
        let mut angle = angle;
        let mut at = title_pos + motion;
        for _ in 0..8 {
            let spread = corners(angle, at);
            if angle == 0.0 || (spread.width() <= bounds.width() + 0.5 && spread.height() <= bounds.height() + 0.5) {
                break;
            }
            angle *= 0.6;
        }
        let spread = corners(angle, at);
        let shift = vec2(
            if spread.left() < bounds.left() { bounds.left() - spread.left() }
            else if spread.right() > bounds.right() { bounds.right() - spread.right() }
            else { 0.0 },
            if spread.top() < bounds.top() { bounds.top() - spread.top() }
            else if spread.bottom() > bounds.bottom() { bounds.bottom() - spread.bottom() }
            else { 0.0 },
        );
        at += shift;
        let shape = egui::epaint::TextShape::new(at, title_galley.clone(), palette.text)
            .with_angle_and_anchor(angle, egui::Align2::CENTER_BOTTOM);
        ui.painter().add(shape);
        if angle != 0.0 || motion != vec2(0.0, 0.0) {
            ui.ctx().request_repaint();
        }
    }
    if title.hovered() {
        ui.painter().hline(
            title_rect.x_range(),
            title_rect.bottom() - 1.0,
            egui::Stroke::new(1.0, palette.text),
        );
    }
    if crate::inspect::armed() {
        crate::inspect::note(title_rect, &now.title);
    }
    if title.clicked() {
        if let Some(id) = &now.album_id {
            app.actions.push(Action::Open(Page::Album(id.clone())));
        } else if let Some(id) = &now.show_id {
            app.actions.push(Action::Open(Page::Show(id.clone())));
        }
    }
    // Drag the song name or the picture onto a playlist in the library.
    if (title.drag_started_by(egui::PointerButton::Primary) || art_response.drag_started_by(egui::PointerButton::Primary))
        && let Some(item) = app.now_playing_item().filter(|item| item.is_track())
    {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            crate::model::DragTrack {
                title: item.name().to_string(),
                image: item.image(64).map(str::to_string),
                items: vec![item],
                from: None,
                source_playlist: None,
            },
        );
    }
    // Right-click the song name for the song's own menu too.
    if let Some(item) = app.now_playing_item() {
        let context = app.editable_context_playlist();
        egui::Popup::context_menu(&title)
            .id(egui::Id::new("overlay-title-menu"))
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| super::widgets::item_menu(ui, app, &item, context.as_ref(), None));
    }
    // The heart sits at the end of the title, in the cover's colour once liked.
    if let Some(item) = &liked_item {
        let size = (title_size * 0.6).clamp(16.0, 28.0);
        like_heart(
            app,
            ui,
            "overlay",
            pos2(title_rect.right() + 6.0 + size / 2.0 + 4.0, title_rect.center().y),
            size,
            item.uri(),
        );
    }

    // The artists, once the pointer is over the art.
    if detail > 0.01 {
        let color = palette.secondary.gamma_multiply(detail);
        let row = Rect::from_min_size(
            pos2(card.left() + pad_x, title_rect.bottom() + gap),
            vec2(max_text_width, sub_height),
        );
        let mut text_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(row)
                .layout(Layout::top_down(Align::Min)),
        );
        text_ui.set_clip_rect(row.intersect(ui.clip_rect()));
        text_ui.horizontal_top(|ui| {
            if now.artists.is_empty() {
                if theme::link(ui, &now.subtitle, theme::regular(sub_size), color).clicked()
                    && let Some(id) = &now.show_id
                {
                    app.actions.push(Action::Open(Page::Show(id.clone())));
                }
            } else {
                super::widgets::artist_links(ui, app, &now.artists, theme::regular(sub_size), color);
            }
        });
    }

    // The picture answers three clicks, because it is the biggest thing on the
    // page and every one of them is a question a reader has about it. Left:
    // where is this song in my library? Middle: what album is it on? Right:
    // the song's own menu, as everywhere else.
    if let Some(item) = app.now_playing_item() {
        let response = art_response;
        if response.clicked() {
            app.actions.push(Action::RevealSong {
                uri: item.uri().to_string(),
            });
        }
        if response.middle_clicked()
            && let crate::api::models::PlayableItem::Track(track) = &item
            && let Some(album) = track.album.as_ref().filter(|album| !album.id.is_empty())
        {
            app.actions
                .push(Action::Open(Page::Album(album.id.clone())));
        }
        let context = app.editable_context_playlist();
        egui::Popup::context_menu(&response)
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| super::widgets::item_menu(ui, app, &item, context.as_ref(), None));
    }
}

/// The title laid out as large as it can be, and the size that was.
///
/// A short name is set big and a long one small, by trying sizes from the
/// biggest down until the name fits: on one line above 25 points,
/// where two lines are allowed instead. Measured with an unwrapped layout, so
/// the answer is what the font really does rather than a guess from the
/// number of letters. Past the smallest size the name is cut with an
/// ellipsis rather than shrunk until it cannot be read.
fn fit_title(
    painter: &egui::Painter,
    title: &str,
    wrap: f32,
    biggest: f32,
    color: Color32,
    family: &egui::FontFamily,
) -> (std::sync::Arc<egui::Galley>, f32) {
    const LADDER: [f32; 8] = [46.0, 40.0, 34.0, 29.0, 25.0, 22.0, 19.0, 16.0];
    /// Sizes above this must fit on a single line; at or below it a second
    /// line is allowed, so a long name is small rather than enormous.
    const TWO_ROW_BELOW: f32 = 25.0;
    // The visualizer's title face, or Inter's semibold when it is the default.
    let font = |size: f32| {
        if *family == egui::FontFamily::Proportional {
            theme::semibold(size)
        } else {
            egui::FontId::new(size, family.clone())
        }
    };
    let width_at = |size: f32| {
        painter
            .layout_no_wrap(title.to_owned(), font(size), color)
            .size()
            .x
    };
    let mut chosen = 16.0;
    for size in LADDER {
        if size > biggest.max(16.0) {
            continue;
        }
        let width = width_at(size);
        let fits = if size > TWO_ROW_BELOW {
            width <= wrap
        } else {
            // A little under two full lines, because words do not break
            // exactly where the width runs out.
            width <= wrap * 1.85
        };
        if fits {
            chosen = size;
            break;
        }
    }
    let rows = if width_at(chosen) <= wrap { 1 } else { 2 };
    let galley = crate::bidi::layout(
        painter,
        title,
        font(chosen),
        color,
        wrap,
        rows,
        Some(crate::bidi::ELLIPSIS),
    );
    (galley, chosen)
}

pub(super) fn transport(
    app: &mut App,
    ui: &mut egui::Ui,
    now: Option<&NowPlaying>,
    row: Rect,
    buttons_row: Rect,
    left: f32,
    zone: (f32, f32),
) -> Rect {
    transport_impl(app, ui, now, row, buttons_row, left, zone, true)
}

/// Only the five buttons, no song-length bar (full-screen lyrics).
pub(super) fn transport_buttons(
    app: &mut App,
    ui: &mut egui::Ui,
    now: Option<&NowPlaying>,
    buttons_row: Rect,
    left: f32,
) -> Rect {
    transport_impl(app, ui, now, buttons_row, buttons_row, left, (left, left), false)
}

fn transport_impl(
    app: &mut App,
    ui: &mut egui::Ui,
    now: Option<&NowPlaying>,
    row: Rect,
    buttons_row: Rect,
    left: f32,
    zone: (f32, f32),
    with_seek: bool,
) -> Rect {
    let palette = app.palette;
    // Everything here is placed with explicit rects: egui's implicit rows
    // centre each widget in the row height known when it is added, which
    // left earlier icons riding high next to the play disc.
    //
    // The buttons row (36) and the progress row (~15, after a 6px gap) form
    // one cluster, centred as a group in the 88px bar: the buttons sit 8px
    // above the bar's midline and the progress row 23px below it. Measured
    // on screen this puts equal breathing room above and beneath the
    // cluster.
    // The controls grow with the visualizer as it is raised and never shrink
    // below their usual size.
    let k = app.settings.controls_scale_value();
    // The buttons grow more gently than the bars: at full size they are
    // about half as much bigger as the song-length bar is.
    let kc = (1.0 + (k - 1.0) * 0.5) * if with_seek { 1.0 } else { 0.8 };
    let cy = buttons_row.center().y;
    let enabled = now.is_some_and(|now| now.can_control) || app.is_connected();
    let playing = now.is_some_and(|now| now.playing);
    let loading = now.is_some_and(|now| now.loading);
    let shuffle = now.map_or_else(|| app.playing_context_shuffle(), |now| now.shuffle);
    let repeat = now.map(|now| now.repeat).unwrap_or_default();
    let dim = if enabled {
        palette.secondary
    } else {
        palette.dim
    };

    // Button widths: icon buttons occupy icon size + 12; the disc is 36.
    let widths = [30.0 * kc, 36.0 * kc, 30.0 * kc, 29.0 * kc, 29.0 * kc];
    let gap = 10.0 * kc;
    // Back, play and next sit together; shuffle and repeat stand a little
    // apart at either end.
    let apart = 22.0 * kc;
    let mut x = left;
    let mut index = 0;
    let mut slot = |width: f32| {
        let rect = Rect::from_center_size(pos2(x + width / 2.0, cy), vec2(width, 36.0 * kc));
        x += width + if index == 2 { apart } else { gap };
        index += 1;
        rect
    };
    // The buttons only show while the pointer is over the middle of the bar
    // (or the song is loading), so the visualizer has the bar to itself.
    // The buttons have a row to themselves above the seek bar, so they are
    // always there.
    let fade = 1.0_f32;
    let centered = |ui: &mut egui::Ui, rect: Rect| {
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
        );
        // Multiplied, not set: the controls may be fading as a whole.
        child.multiply_opacity(fade);
        child
    };

    let mut cell = centered(ui, slot(widths[0]));
    if theme::icon_button(
        &mut cell,
        Icon::SkipBackFilled,
        18.0 * kc,
        dim,
        palette.text,
        &gettext(app.locale, "Previous"),
    )
    .clicked()
    {
        app.actions.push(Action::Previous);
    }

    let disc = slot(widths[1]);
    if loading || app.any_play_pending() {
        ui.painter()
            .circle_filled(disc.center(), 18.0 * kc, palette.text);
        let mut cell = centered(ui, disc);
        theme::spinner(&mut cell, 22.0 * kc, palette.window);
    } else {
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let hover = if palette.dark {
            egui::Color32::WHITE
        } else {
            palette.text
        };
        let mut cell = centered(ui, disc);
        if app.settings.float_keep_play {
            cell.set_opacity(1.0);
        }
        if theme::circle_button(
            &mut cell,
            icon,
            36.0 * kc,
            palette.text,
            hover,
            palette.window,
            &if playing {
                gettext(app.locale, "Pause")
            } else {
                gettext(app.locale, "Play")
            },
        )
        .clicked()
        {
            app.actions.push(Action::TogglePlay);
        }
    }

    let mut cell = centered(ui, slot(widths[2]));
    if theme::icon_button(
        &mut cell,
        Icon::SkipForwardFilled,
        18.0 * kc,
        dim,
        palette.text,
        &gettext(app.locale, "Next"),
    )
    .clicked()
    {
        app.actions.push(Action::Next);
    }

    let shuffle_color = if shuffle { palette.accent } else { dim };
    let mut cell = centered(ui, slot(widths[3]));
    let shuffle_button = theme::icon_button(
        &mut cell,
        Icon::Shuffle,
        17.0 * kc,
        shuffle_color,
        if shuffle {
            palette.accent_hover
        } else {
            palette.text
        },
        &gettext(app.locale, "Shuffle"),
    );
    shuffle_button.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            cell.is_enabled(),
            shuffle,
            gettext(app.locale, "Shuffle"),
        )
    });
    if shuffle {
        on_dot(ui, shuffle_button.rect, &palette);
    }
    if shuffle_button.clicked() {
        app.actions.push(Action::ToggleShuffle);
    }

    let (repeat_icon, repeat_color, tooltip) = match repeat {
        RepeatMode::Off => (Icon::Repeat, dim, gettext(app.locale, "Repeat")),
        RepeatMode::Context => (
            Icon::Repeat,
            palette.accent,
            gettext(app.locale, "Repeat one"),
        ),
        RepeatMode::Track => (
            Icon::Repeat1,
            palette.accent,
            gettext(app.locale, "Repeat off"),
        ),
    };
    let mut cell = centered(ui, slot(widths[4]));
    let repeat_button = theme::icon_button(
        &mut cell,
        repeat_icon,
        17.0 * kc,
        repeat_color,
        if repeat == RepeatMode::Off {
            palette.text
        } else {
            palette.accent_hover
        },
        &tooltip,
    );
    if repeat != RepeatMode::Off {
        on_dot(ui, repeat_button.rect, &palette);
    }
    if repeat_button.clicked() {
        app.actions.push(Action::CycleRepeat);
    }

    if !with_seek {
        return buttons_row;
    }
    // The song-length bar, in the zone the layout left for it.
    let mode = app.settings.seek_time_mode_value();
    let time_size = 12.0 * k.clamp(0.85, 2.0);
    let (position, duration) = now
        .map(|now| (now.position_ms, now.duration_ms))
        .unwrap_or((0, 0));
    // The room the time needs is measured from the text itself, so it is
    // never cut off whatever its length: a song over an hour has a longer
    // time than one of three minutes.
    let end_text = util::format_duration_ms(duration);
    let both_text = |from: u32| {
        format!("{} / {}", util::format_duration_ms(from), end_text)
    };
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), theme::regular(time_size), Color32::WHITE)
            .size()
            .x
            .ceil()
            + 2.0
    };
    let one_w = measure(&end_text).max(measure("0:00"));
    let both_w = measure(&both_text(duration)).max(measure("0:00 / 0:00"));
    let (time_width, right_label) = match mode {
        0 => (one_w, 14.0 + one_w),
        1 => (both_w, 14.0),
        2 => (0.0, 14.0 + both_w),
        _ => (0.0, 14.0),
    };
    let row_cy = row.center().y;
    let full_width = (zone.1 - zone.0 - time_width - 8.0 - right_label).clamp(100.0, 1600.0);
    // S steps the bar through short, medium and full width.
    let force_full = ui.ctx().data(|data| data.get_temp::<bool>(egui::Id::new("popout-full-seek"))).unwrap_or(false);
    let slider_width = if force_full {
        full_width
    } else if app.settings.seek_custom_width > 0.0 {
        app.settings.seek_custom_width.min(full_width).max(100.0)
    } else {
        match app.settings.seek_width {
            0 => 240.0_f32.min(full_width),
            1 => 380.0_f32.min(full_width),
            _ => full_width.min(900.0),
        }
    };
    let volume_w = ui
        .ctx()
        .data(|data| data.get_temp::<f32>(egui::Id::new("volume-w")))
        .unwrap_or(0.0);
    let slider_width = slider_width.max(volume_w.min(full_width));
    let shown_position = match app.seek_preview {
        Some(fraction) => (fraction * duration as f32) as u32,
        None => position,
    };
    let time_color = if now.is_some() {
        palette.secondary
    } else {
        palette.dim
    };
    let min_left = zone.0 + time_width + 8.0;
    let max_left = (zone.1 - right_label - slider_width).max(min_left);
    let slider_left = match app.settings.seek_anchor {
        // Centred as a whole: the time, the bar and the end time together
        // sit in the middle of the room the layout gave this group.
        1 => {
            let group = time_width + 8.0 + slider_width + right_label;
            (((zone.0 + zone.1) / 2.0 - group / 2.0) + time_width + 8.0).clamp(min_left, max_left)
        }
        2 => max_left,
        _ => min_left,
    };
    // The reader may have dragged the bar along the row (the dots over its
    // left end), in steps of a grid, inside the room it has.
    let seek_steps = app.settings.nudge_seek;
    // Dragged right past the end of the room, the bar gets shorter instead of
    // refusing to move, so a full-length bar can be moved too.
    let slider_left = (slider_left + f32::from(seek_steps) * 12.0)
        .clamp(min_left, (zone.1 - right_label - 100.0).max(min_left));
    let slider_width = slider_width.min((zone.1 - right_label - slider_left).max(100.0));
    let joined_text = both_text(shown_position);
    let before = match mode {
        0 => Some(util::format_duration_ms(shown_position)),
        1 => Some(joined_text.clone()),
        _ => None,
    };
    if let Some(before) = before {
        ui.painter().text(
            pos2(slider_left - 8.0, row_cy),
            egui::Align2::RIGHT_CENTER,
            before,
            theme::regular(time_size),
            time_color,
        );
    }
    let slider_rect = Rect::from_min_size(
        pos2(slider_left, row_cy - 13.0 * k),
        vec2(slider_width, 26.0 * k),
    );
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
    match thin_slider_scaled(
        &mut slider_ui,
        &palette,
        egui::Id::new("seek-slider"),
        &gettext(app.locale, "Playback position (%)"),
        fraction,
        slider_width,
        None,
        k,
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
    // With the time switched off, hovering the bar says where the song is.
    if mode == 3
        && duration > 0
        && let Some(pointer) = ui.input(|input| input.pointer.hover_pos())
        && slider_rect.contains(pointer)
    {
        let text = both_text(shown_position);
        egui::Area::new(egui::Id::new("seek-hover-time"))
            .order(egui::Order::Tooltip)
            .interactable(false)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(pos2(pointer.x, slider_rect.top() - 6.0))
            .show(ui.ctx(), |ui| {
                super::widgets::menu_frame(&palette).show(ui, |ui| {
                    ui.label(egui::RichText::new(text).color(palette.text));
                });
            });
        ui.ctx().request_repaint();
    }
    // Dots over the bar's left end while the pointer is over the row: drag
    // them to move the bar along the row, as with the controls and volume.
    if ui.rect_contains_pointer(row) {
        let handle = Rect::from_min_size(pos2(slider_left - 2.0, row.top() + 1.0), vec2(14.0, 12.0));
        let drag = ui.interact(handle, egui::Id::new(("block-handle", 1u8)), Sense::drag());
        let color = if drag.hovered() || drag.dragged() { palette.text } else { palette.dim };
        let _ = color;
        if drag.hovered() || drag.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        if drag.dragged() {
            let key = egui::Id::new(("block-residual", 1u8));
            let mut residual = ui.ctx().data(|data| data.get_temp::<f32>(key)).unwrap_or(0.0);
            residual += drag.drag_delta().x;
            let moved = (residual / 12.0).trunc();
            residual -= moved * 12.0;
            ui.ctx().data_mut(|data| data.insert_temp(key, residual));
            if moved != 0.0 {
                app.actions.push(Action::SetBlockNudge(1, seek_steps + moved as i16));
            }
        }
    }
    let mut seek_right = slider_left + slider_width + 14.0;
    let after = match mode {
        0 => Some(end_text.clone()),
        2 => Some(joined_text),
        _ => None,
    };
    if let Some(after) = after {
        let end_rect = ui.painter().text(
            pos2(slider_left + slider_width + 14.0, row_cy),
            egui::Align2::LEFT_CENTER,
            after,
            theme::regular(time_size),
            time_color,
        );
        seek_right = end_rect.right();
    }
    Rect::from_min_max(
        pos2(slider_left - time_width - 8.0, row.top()),
        pos2(seek_right + 4.0, row.bottom()),
    )
}

/// Right-click menus for the bottom row. Each group has its own, and the
/// background between them has one for arranging the row.
///
/// 0 is the play controls (their size), 1 the song-length bar (where it
/// sits and how long), 2 the volume (speed, scenes and the equalizer), 3
/// the background (the order of the groups and everything about the bar).
fn row_menus(app: &mut App, ui: &mut egui::Ui, row: Rect, zones: [Rect; 3], name_region: Rect) {
    let ctx = ui.ctx().clone();
    let id = egui::Id::new("row-menu");
    let mut open = ctx.data(|data| data.get_temp::<(u8, egui::Pos2)>(id));
    let mut opened_now = false;
    // Six dots in the gutter at the row's far right: click them for every
    // setting of the row, without hunting for the right spot to right-click.
    let grip = Rect::from_min_size(pos2(row.right() - 14.0, row.bottom() - 20.0), vec2(14.0, 18.0));
    {
        let response = ui.interact(grip, egui::Id::new("row-menu-grip"), Sense::click());
        let colour = if response.hovered() || open.is_some_and(|(kind, _)| kind == 3) {
            app.palette.text
        } else {
            app.palette.dim
        };
        let _ = colour;
        if response.hovered() {
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if response.clicked() || response.secondary_clicked() {
            if open.is_some() {
                ctx.data_mut(|data| data.remove::<(u8, egui::Pos2)>(id));
                open = None;
            } else {
                let at = grip.center_top();
                open = Some((3, at));
                opened_now = true;
                ctx.data_mut(|data| data.insert_temp(id, (3, at)));
            }
        }
    }
    if ui.rect_contains_pointer(row)
        && ui.input(|input| input.pointer.button_clicked(egui::PointerButton::Secondary))
        && let Some(pos) = ui.input(|input| input.pointer.interact_pos())
    {
        // The volume buttons have menus of their own.
        let on_volume_button = zones[2].contains(pos) && pos.x > zones[2].left() + 164.0;
        // The cover and the song's name have their own menus.
        if !on_volume_button && !name_region.contains(pos) && !grip.contains(pos) {
            let kind = if zones[2].contains(pos) {
                2
            } else if zones[0].contains(pos) {
                0
            } else if zones[1].contains(pos) {
                1
            } else {
                3
            };
            open = Some((kind, pos));
            opened_now = true;
            ctx.data_mut(|data| data.insert_temp(id, (kind, pos)));
        }
    }
    let Some((kind, pos)) = open else {
        return;
    };
    let palette = app.palette;
    let screen_h = ctx
        .input(|input| input.raw.screen_rect)
        .map_or(900.0, |rect| rect.height());
    // The menu can be dragged by its top strip and widened with the corner
    // at the strip's right end. Both are remembered while it is open.
    let width_id = id.with("width");
    let default_width = if kind == 2 { 340.0 } else { 290.0 };
    let menu_width = ctx
        .data(|data| data.get_temp::<f32>(width_id))
        .unwrap_or(default_width)
        .clamp(240.0, 640.0);
    let mut moved = Vec2::ZERO;
    let mut widened = 0.0_f32;
    let area = egui::Area::new(id.with("area"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_BOTTOM)
        .fixed_pos(pos - vec2(0.0, 8.0))
        .constrain(true)
        .show(&ctx, |ui| {
            super::widgets::menu_frame(&palette).show(ui, |ui| {
                ui.set_width(menu_width);
                let _ = screen_h;
                {
                    let strip_w = ui.available_width();
                    let (strip, _) = ui.allocate_exact_size(vec2(strip_w, 12.0), Sense::hover());
                    let grab = Rect::from_min_max(strip.min, pos2(strip.right() - 22.0, strip.bottom()));
                    let corner = Rect::from_min_max(pos2(strip.right() - 20.0, strip.top()), strip.max);
                    let mover = ui.interact(grab, id.with("move"), Sense::drag());
                    let sizer = ui.interact(corner, id.with("size"), Sense::drag());
                    if mover.hovered() || mover.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                    }
                    if sizer.hovered() || sizer.dragged() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if mover.dragged() {
                        moved = mover.drag_delta();
                    }
                    if sizer.dragged() {
                        widened = sizer.drag_delta().x;
                    }
                    ui.painter().rect_filled(
                        Rect::from_center_size(grab.center(), vec2(40.0, 3.0)),
                        2.0,
                        palette.dim,
                    );
                    ui.painter().text(
                        corner.center(),
                        egui::Align2::CENTER_CENTER,
                        "<>",
                        theme::regular(theme::SMALL_TEXT),
                        palette.dim,
                    );
                }
                (|ui: &mut egui::Ui| match kind {
                        1 => bar_rows(app, ui, &palette),
                        2 => audio_menu_body(app, ui),
                        // The play controls: only their own size and place.
                        0 => {
                            let size = app.settings.controls_scale_value();
                            slider_row(
                                ui,
                                &palette,
                                &gettext(app.locale, "Size"),
                                70.0..=180.0,
                                size * 100.0,
                                |value| app.actions.push(Action::SetControlsScale(value / 100.0)),
                            );
                            anchor_rows(app, ui, &palette, Some(0));
                            if super::widgets::menu_item(
                                ui,
                                &palette,
                                None,
                                &gettext(app.locale, "Put the controls back"),
                            ) {
                                app.actions.push(Action::SetBlockNudge(0, 0));
                            }
                        }
                        // The empty space between the groups: everything
                        // about the row at once, the layout first. The
                        // row is resized by dragging its empty space.
                        _ => {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let free = app.settings.bar_free;
                                if chip(ui, &palette, "Spotify layout", !free, 130.0).clicked() {
                                    app.actions.push(Action::SpotifyBar);
                                }
                                if chip(ui, &palette, "My own layout", free, 130.0).clicked() && !free {
                                    app.actions.push(Action::SetBarFree(true));
                                }
                            });
                            theme::subtle(
                                ui,
                                &palette,
                                "My own layout: drag the dotted handle on the controls, the song bar or the volume anywhere in the bar.",
                            );
                            super::widgets::menu_separator(ui, &palette);
                            layout_rows(app, ui, &palette);
                            super::widgets::menu_separator(ui, &palette);
                            arrange_strip(app, ui, &palette);
                            anchor_rows(app, ui, &palette, None);
                            super::widgets::menu_separator(ui, &palette);
                            bar_rows(app, ui, &palette);
                        }
                    })(ui);
            });
        });
    if moved != Vec2::ZERO {
        ctx.data_mut(|data| data.insert_temp(id, (kind, pos + moved)));
    }
    if widened != 0.0 {
        ctx.data_mut(|data| data.insert_temp(width_id, (menu_width + widened * 2.0).clamp(240.0, 640.0)));
    }
    let outside = ctx.input(|input| {
        input.pointer.any_pressed()
            && input
                .pointer
                .interact_pos()
                .is_some_and(|at| !area.response.rect.contains(at) && !grip.contains(at))
    });
    if (outside && !opened_now) || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
        ctx.data_mut(|data| data.remove::<(u8, egui::Pos2)>(id));
    }
}


/// The three groups of the row as three tiles, in the order they sit in the
/// row. Drag a tile along the strip to move that group.
/// Left, middle or right for the play controls and for the volume, each on
/// its own (the song-length bar has its own row of the same chips).
fn anchor_rows(
    app: &mut App,
    ui: &mut egui::Ui,
    palette: &crate::theme::Palette,
    only: Option<u8>,
) {
    let order = ROW_ORDERS[(app.settings.row_order as usize).min(5)];
    let seek_at = order.iter().position(|b| *b == 1).unwrap_or(1);
    for (label, block) in [("Controls", 0u8), ("Volume", 2u8)] {
        if only.is_some_and(|only| only != block) {
            continue;
        }
        let place = order.iter().position(|b| *b == block).unwrap_or(0);
        let chosen = match block {
            0 => app.settings.anchor_controls,
            _ => app.settings.anchor_volume,
        }
        .unwrap_or(if place < seek_at { 0 } else { 2 });
        theme::subtle(ui, palette, &gettext(app.locale, label).to_uppercase());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (value, name) in [(0u8, "Left"), (1, "Middle"), (2, "Right")] {
                if chip(ui, palette, &gettext(app.locale, name), chosen == value, 84.0).clicked() {
                    app.actions.push(Action::SetBlockAnchor(block, value));
                }
            }
        });
    }
}

/// How the three parts of the bottom row are stacked: side by side, the
/// song bar under or above the other two, or each on its own row.
fn layout_rows(app: &mut App, ui: &mut egui::Ui, palette: &crate::theme::Palette) {
    let current = app.settings.bar_layout_value();
    for (value, label) in [
        (0u8, "All in one row"),
        (1, "Song bar under the controls and volume"),
        (3, "Song bar above the controls and volume"),
        (2, "Controls, volume and song bar, each on its own row"),
    ] {
        if super::widgets::menu_item(
            ui,
            palette,
            if current == value { Some(Icon::Check) } else { None },
            &gettext(app.locale, label),
        ) {
            app.actions.push(Action::SetBarLayout(value));
        }
    }
}

fn arrange_strip(app: &mut App, ui: &mut egui::Ui, palette: &crate::theme::Palette) {
    let order = ROW_ORDERS[(app.settings.row_order as usize).min(5)];
    let names = ["Controls", "Bar", "Volume"];
    let gap = 6.0;
    let width = ((ui.available_width() - gap * 2.0) / 3.0).floor();
    let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
    for (slot, item) in order.iter().enumerate() {
        let tile = Rect::from_min_size(
            pos2(strip.left() + slot as f32 * (width + gap), strip.top()),
            vec2(width, 32.0),
        );
        let response = ui.interact(
            tile,
            egui::Id::new(("row-arrange", *item)),
            Sense::click_and_drag(),
        );
        let held = response.dragged();
        if held {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
        }
        let fill = if held {
            palette.accent.gamma_multiply(0.45)
        } else if response.hovered() {
            palette.surface_hover
        } else {
            palette.surface
        };
        ui.painter().rect_filled(tile, egui::CornerRadius::same(8), fill);
        ui.painter().text(
            tile.center(),
            egui::Align2::CENTER_CENTER,
            gettext(app.locale, names[*item as usize]),
            theme::semibold(theme::BODY_TEXT),
            palette.text,
        );
        if held && let Some(pointer) = ui.input(|input| input.pointer.interact_pos()) {
            let target = (((pointer.x - strip.left()) / (width + gap)).floor() as i32).clamp(0, 2)
                as usize;
            if target != slot {
                let mut next = order.to_vec();
                next.remove(slot);
                next.insert(target, *item);
                if let Some(index) = ROW_ORDERS.iter().position(|perm| perm[..] == next[..]) {
                    app.actions.push(Action::SetRowOrder(index as u8));
                }
            }
        }
    }
}

fn controls_size_row(app: &mut App, ui: &mut egui::Ui, palette: &crate::theme::Palette) {
    slider_row(
        ui,
        palette,
        &gettext(app.locale, "Size"),
        0.7..=1.8,
        app.settings.controls_scale_value(),
        |value| app.actions.push(Action::SetControlsScale(value)),
    );
}

/// Where the song-length bar sits, how long it is, and how its time reads.
fn bar_rows(app: &mut App, ui: &mut egui::Ui, palette: &crate::theme::Palette) {
    let anchor = app.settings.seek_anchor;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (value, label) in [(0u8, "Left"), (1, "Middle"), (2, "Right")] {
            if chip(ui, palette, &gettext(app.locale, label), anchor == value, 84.0).clicked() {
                app.actions.push(Action::SetSeekAnchor(value));
            }
        }
    });
    let current = if app.settings.seek_custom_width > 0.0 {
        app.settings.seek_custom_width
    } else {
        match app.settings.seek_width {
            0 => 240.0,
            1 => 380.0,
            _ => 900.0,
        }
    };
    slider_row(
        ui,
        palette,
        &gettext(app.locale, "Length"),
        100.0..=1400.0,
        current,
        |value| app.actions.push(Action::SetSeekWidth(value.round())),
    );
    let mode = app.settings.seek_time_mode_value();
    for pair in [[(0u8, "Time: both ends"), (1, "Time: joined at start")], [(2, "Time: joined at end"), (3, "Time: off")]] {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (value, label) in pair {
                if chip(ui, palette, &gettext(app.locale, label), mode == value, 138.0).clicked() {
                    app.actions.push(Action::SetSeekTimeMode(value));
                }
            }
        });
    }
    if super::widgets::menu_item(ui, palette, None, &gettext(app.locale, "Reset length")) {
        app.actions.push(Action::SetSeekWidth(0.0));
    }
}

/// The right-click menu of a volume button, the same in the pop-out controls
/// and in the bar: change it, add one, remove it (never the last one), and
/// open the equalizer.
fn preset_menu(app: &mut App, ui: &mut egui::Ui, palette: &crate::theme::Palette, list: &[u8], index: usize, shown: u8) {
    if index >= list.len() {
        return;
    }
    ui.set_width(210.0);
    theme::subtle(ui, palette, &format!("{}% BUTTON", list[index]));
    if super::widgets::menu_item(ui, palette, None, &format!("Set to {shown}% (current)")) {
        let mut next = list.to_vec();
        next[index] = shown;
        app.actions.push(Action::SetVolumePresets(next));
    }
    if list.len() < 4 && super::widgets::menu_item(ui, palette, None, &gettext(app.locale, "Add button at current volume")) {
        let mut next = list.to_vec();
        next.push(shown);
        app.actions.push(Action::SetVolumePresets(next));
    }
    // The last button cannot be removed: there is always one.
    if list.len() > 1 && super::widgets::menu_item(ui, palette, None, &gettext(app.locale, "Remove button")) {
        let mut next = list.to_vec();
        next.remove(index);
        app.actions.push(Action::SetVolumePresets(next));
    }
    if super::widgets::menu_item(ui, palette, None, "equalizer") {
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("eq-window-open"), true));
        ui.close();
    }
}

/// The menu on the volume's speaker and percentage: add a volume button, or
/// open the equalizer. (Right-clicking the bar itself jumps to that spot.)
fn volume_menu(app: &mut App, ui: &mut egui::Ui, palette: &crate::theme::Palette, shown: u8) {
    let list = app.settings.volume_presets_value();
    ui.set_width(210.0);
    theme::subtle(ui, palette, "volume buttons");
    if list.len() < 4 && super::widgets::menu_item(ui, palette, None, &gettext(app.locale, "Add button at current volume")) {
        let mut next = list.clone();
        next.push(shown);
        app.actions.push(Action::SetVolumePresets(next));
    }
    if super::widgets::menu_item(ui, palette, None, "equalizer") {
        ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("eq-window-open"), true));
        ui.close();
    }
}

/// The equalizer in a small window of its own, opened from a volume button's
/// right-click menu. Drawn once per frame wherever the buttons are.
fn eq_window(app: &mut App, ctx: &egui::Context) {
    let open_id = egui::Id::new("eq-window-open");
    let mut open = ctx.data(|data| data.get_temp::<bool>(open_id)).unwrap_or(false);
    if !open {
        return;
    }
    let time = ctx.input(|input| input.time);
    let drawn_id = egui::Id::new("eq-window-drawn");
    if ctx.data(|data| data.get_temp::<f64>(drawn_id)) == Some(time) {
        return;
    }
    ctx.data_mut(|data| data.insert_temp(drawn_id, time));
    let palette = app.palette;
    egui::Window::new("")
        .id(egui::Id::new("eq-window"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .constrain(true)
        .frame(super::widgets::menu_frame(&palette))
        .show(ctx, |ui| {
            super::settings::eq_panel(app, ui);
        });
    ctx.data_mut(|data| data.insert_temp(open_id, open));
}

/// The volume buttons for the pop-out panel: click one to jump to it,
/// right-click one for its settings and the equalizer.
pub(super) fn popout_presets(app: &mut App, ui: &mut egui::Ui, rect: Rect, presets: &[u8]) {
    let palette = app.palette;
    let now = app.now_playing();
    let volume = now
        .as_ref()
        .map(|now| now.volume_percent)
        .unwrap_or_else(|| crate::app::volume_to_percent(app.local.volume));
    let shown = match app.volume_preview {
        Some(fraction) => (fraction * 100.0).round() as u8,
        None => volume,
    };
    let adjustable = now.as_ref().is_none_or(|now| now.can_set_volume);
    // Small chips squeezed into whatever height the row has, centred over
    // the volume bar, so they never need a row of their own.
    let h = rect.height().clamp(10.0, 18.0);
    let gap = 3.0;
    let n = presets.len().max(1) as f32;
    let w = ((rect.width() - gap * (n - 1.0)) / n).clamp(18.0, 34.0);
    let total = w * n + gap * (n - 1.0);
    let mut x = rect.center().x - total / 2.0;
    for (index, preset) in presets.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(x, rect.center().y - h / 2.0), vec2(w, h));
        x += w + gap;
        let response = ui.interact(cell.expand(2.0), ui.id().with(("popout-preset", index)), Sense::click());
        let active = shown == *preset;
        let fill = if active {
            palette.accent.gamma_multiply(0.35)
        } else if response.hovered() {
            palette.surface_hover
        } else {
            palette.surface
        };
        ui.painter().rect_filled(cell, egui::CornerRadius::same(5), fill);
        ui.painter().text(
            cell.center(),
            egui::Align2::CENTER_CENTER,
            format!("{preset}"),
            theme::regular((h * 0.62).clamp(8.5, 11.5)),
            if active { palette.text } else { palette.secondary },
        );
        if response.clicked() && adjustable {
            app.volume_preview = None;
            app.actions.push(Action::SetVolume(*preset));
        }
        let list = presets.to_vec();
        egui::Popup::context_menu(&response)
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| preset_menu(app, ui, &palette, &list, index, shown));
    }
    eq_window(app, ui.ctx());
}

fn extras(app: &mut App, ui: &mut egui::Ui, now: Option<&NowPlaying>, presets: &[u8]) {
    let palette = app.palette;
    ui.spacing_mut().item_spacing.x = 6.0;
    let volume = now
        .map(|now| now.volume_percent)
        .unwrap_or_else(|| crate::app::volume_to_percent(app.local.volume));
    let shown = match app.volume_preview {
        Some(fraction) => (fraction * 100.0).round() as u8,
        None => volume,
    };
    // Some remote devices, such as a phone playing over Bluetooth, refuse
    // volume changes from other apps: show their volume but don't offer to
    // change it.
    let adjustable = now.is_none_or(|now| now.can_set_volume);
    let slider_origin = ui.next_widget_position();
    let slider_w = ui
        .ctx()
        .data(|data| data.get_temp::<f32>(egui::Id::new("volume-w")))
        .unwrap_or_else(|| volume_bar_width(app));
    let controls = ui.add_enabled_ui(adjustable, |ui| {
        match thin_slider_scaled(
            ui,
            &palette,
            egui::Id::new("volume-slider"),
            &gettext(app.locale, "Volume (%)"),
            shown as f32 / 100.0,
            ui.ctx()
                .data(|data| data.get_temp::<f32>(egui::Id::new("volume-w")))
                .unwrap_or_else(|| volume_bar_width(app)),
            Some(0.05),
            app.settings.controls_scale_value(),
        ) {
            SliderEvent::Dragging(value) => {
                app.volume_preview = Some(value);
                // Local volume is cheap to apply continuously; remote goes on release.
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
        // No speaker icon: the slider is the volume, and the plain box beside
        // it can be typed into.
        if let Some(text) = number_field_framed(
            ui,
            egui::Id::new("volume-field"),
            format!("{shown}%"),
            46.0,
            palette.secondary,
            false,
        ) && let Ok(percent) = text.trim().trim_end_matches('%').trim().parse::<f32>()
        {
            let percent = percent.round().clamp(0.0, 100.0) as u8;
            app.volume_preview = None;
            app.actions.push(Action::SetVolume(percent));
        }
    });
    if adjustable && let Some(next) = wheel_volume(ui, controls.response.rect, shown) {
        app.volume_preview = None;
        app.actions.push(Action::SetVolume(next));
    }
    if !adjustable {
        ui.interact(
            controls.response.rect,
            egui::Id::new("volume-fixed"),
            egui::Sense::hover(),
        )
        .on_hover_text(gettext(
            app.locale,
            "This device's volume can't be changed from chanceify",
        ));
    }
    // Shortcut buttons. Click one to jump to that volume; right-click to
    // change, add or remove.
    for (index, preset) in presets.iter().enumerate() {
        let response = chip_sensing(
            ui,
            &palette,
            &format!("{preset}"),
            shown == *preset,
            40.0,
            Sense::click_and_drag(),
        );
        // Drag a button along the row to put it in another place.
        {
            let key = egui::Id::new(("preset-drag", index));
            if response.dragged() {
                let travelled = ui.ctx().data(|data| data.get_temp::<f32>(key)).unwrap_or(0.0)
                    + response.drag_delta().x;
                ui.ctx().data_mut(|data| data.insert_temp(key, travelled));
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            if response.drag_stopped() {
                let travelled = ui.ctx().data(|data| data.get_temp::<f32>(key)).unwrap_or(0.0);
                ui.ctx().data_mut(|data| data.remove::<f32>(key));
                let target = (index as i32 + (travelled / 46.0).round() as i32)
                    .clamp(0, presets.len() as i32 - 1) as usize;
                if target != index {
                    let mut next = presets.to_vec();
                    let moved = next.remove(index);
                    next.insert(target, moved);
                    app.actions.push(Action::SetVolumePresets(next));
                }
            }
        }
        if response.clicked() {
            app.volume_preview = None;
            app.actions.push(Action::SetVolume(*preset));
        }
        let list = presets.to_vec();
        egui::Popup::context_menu(&response)
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| preset_menu(app, ui, &palette, &list, index, shown));
    }
    eq_window(app, ui.ctx());
    if presets.is_empty()
        && chip(ui, &palette, "+", false, 40.0).clicked()
    {
        app.actions.push(Action::SetVolumePresets(vec![shown]));
    }
}

#[cfg(test)]
mod player_bar_tint_tests {
    use super::*;
    use crate::theme::Palette;

    /// Run one frame at `time` and report the bar's actual fill.
    fn frame(ctx: &egui::Context, time: f64, panel: Color32, tint: Option<Color32>) -> Color32 {
        let mut fill = Color32::PLACEHOLDER;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |ui| fill = eased_fill(ui.ctx(), panel, tint),
        );
        output.textures_delta.clear();
        fill
    }

    #[test]
    fn a_song_without_art_preserves_translucent_panel() {
        for panel in [
            Palette::dark().panel,
            Palette::light().panel,
            Color32::from_rgba_unmultiplied(80, 120, 180, 128),
        ] {
            let ctx = egui::Context::default();
            assert_eq!(frame(&ctx, 0.0, panel, None), panel);
            let art = Color32::from_rgb(200, 40, 90);
            frame(&ctx, 0.1, panel, Some(art));
            frame(&ctx, 0.6, panel, Some(art));
            assert_eq!(frame(&ctx, 0.7, panel, None).a(), panel.a());
            assert_eq!(frame(&ctx, 1.2, panel, None), panel);
        }
    }

    #[test]
    fn the_first_frame_shows_the_tint_without_fading_in() {
        let ctx = egui::Context::default();
        let panel = Palette::dark().panel;
        let art = Color32::from_rgb(200, 40, 90);
        assert_eq!(
            frame(&ctx, 0.0, panel, Some(art)),
            super::super::blend(panel, art, TINT_STRENGTH)
        );
    }

    #[test]
    fn first_art_after_an_untinted_frame_fades_in() {
        let ctx = egui::Context::default();
        let panel = Palette::dark().panel;
        let art = Color32::from_rgb(220, 140, 160);
        let target = super::super::blend(panel, art, TINT_STRENGTH);
        let fade = f64::from(TINT_FADE_SECONDS);

        assert_eq!(frame(&ctx, 0.0, panel, None), panel);
        assert_eq!(frame(&ctx, 0.1, panel, Some(art)), panel);
        let middle = frame(&ctx, 0.1 + fade / 2.0, panel, Some(art));
        assert_ne!(middle, panel);
        assert_ne!(middle, target);
        assert_eq!(frame(&ctx, 0.1 + fade, panel, Some(art)), target);
    }

    #[test]
    fn changing_songs_mid_fade_continues_from_the_visible_colour() {
        let ctx = egui::Context::default();
        let panel = Palette::dark().panel;
        let first = Color32::from_rgb(20, 40, 60);
        let second = Color32::from_rgb(220, 140, 160);
        let third = Color32::from_rgb(40, 230, 30);
        let first_fill = super::super::blend(panel, first, TINT_STRENGTH);
        let second_fill = super::super::blend(panel, second, TINT_STRENGTH);
        let third_fill = super::super::blend(panel, third, TINT_STRENGTH);
        let fade = f64::from(TINT_FADE_SECONDS);

        assert_eq!(frame(&ctx, 0.0, panel, Some(first)), first_fill);
        assert_eq!(frame(&ctx, 0.05, panel, Some(second)), first_fill);
        let middle = frame(&ctx, 0.2, panel, Some(second));
        assert_ne!(middle, first_fill);
        assert_ne!(middle, second_fill);
        assert_eq!(frame(&ctx, 0.2, panel, Some(third)), middle);
        assert_ne!(frame(&ctx, 0.2 + fade / 2.0, panel, Some(third)), middle);
        assert_eq!(frame(&ctx, 0.2 + fade, panel, Some(third)), third_fill);
    }

    #[test]
    fn a_new_session_does_not_reuse_the_previous_tint() {
        let ctx = egui::Context::default();
        let panel = Palette::dark().panel;
        let first = Color32::from_rgb(20, 40, 60);
        let second = Color32::from_rgb(220, 140, 160);
        let next_session = Color32::from_rgb(40, 230, 30);

        frame(&ctx, 0.0, panel, Some(first));
        frame(&ctx, 0.1, panel, Some(second));
        assert_ne!(
            frame(&ctx, 0.2, panel, Some(second)),
            super::super::blend(panel, second, TINT_STRENGTH)
        );

        end_tint_session(&ctx);
        assert_eq!(
            frame(&ctx, 0.21, panel, Some(next_session)),
            super::super::blend(panel, next_session, TINT_STRENGTH)
        );

        end_tint_session(&ctx);
        assert_eq!(frame(&ctx, 0.22, panel, None), panel);
    }

    /// However light or dark the cover, the visualizer's colours stay in the
    /// band the theme's text reads against, and keep their hue.
    #[test]
    fn visualizer_colours_stay_behind_the_words() {
        let luminance = |colour: Color32| {
            let linear = egui::Rgba::from(colour);
            0.2126 * linear.r() + 0.7152 * linear.g() + 0.0722 * linear.b()
        };
        for base in [
            Color32::from_rgb(255, 240, 80),
            Color32::from_rgb(20, 30, 90),
            Color32::WHITE,
            Color32::BLACK,
            Color32::from_rgb(30, 215, 96),
        ] {
            let (low, high) = vis_colours(base, true);
            for colour in [low, high] {
                assert!(
                    luminance(colour) <= DARK_CEILING + 0.01,
                    "{base:?} on dark: {colour:?}"
                );
            }
            let (low, high) = vis_colours(base, false);
            for colour in [low, high] {
                assert!(
                    (LIGHT_FLOOR - 0.01..=LIGHT_CEILING + 0.01).contains(&luminance(colour)),
                    "{base:?} on light: {colour:?}"
                );
            }
        }
        // A yellow cover stays yellow, only deeper.
        let (low, _) = vis_colours(Color32::from_rgb(255, 240, 80), true);
        assert!(low.r() > low.b() && low.g() > low.b(), "{low:?}");
    }
}

/// The yellow "!" on the big album art when Last.fm has no cover for it.
/// Off with the toggle in the Last.fm settings.
pub(super) fn big_art_missing_mark(app: &App, ui: &egui::Ui, art: Rect, id: &str) {
    if !(app.art_missing_now && app.settings.missing_mark_big && app.now_playing_item().is_some()) {
        return;
    }
    let palette = app.palette;
    let badge = Rect::from_center_size(art.right_top() + vec2(-14.0, 14.0), vec2(24.0, 24.0));
    ui.painter().circle_filled(badge.center(), 12.0, palette.warning);
    ui.painter().text(
        badge.center(),
        egui::Align2::CENTER_CENTER,
        "!",
        theme::bold(theme::SECTION_TEXT),
        palette.window,
    );
    ui.interact(badge, egui::Id::new(("big-art-missing", id)), Sense::hover()).on_hover_text(
        "Last.fm has no cover for this album. It is saved in the lastfm-art folder, ready to upload.",
    );
}

/// A compact volume bar for the pop-out controls panel: a small speaker, a
/// slim bar with a round knob, and the percentage in a fixed column on the
/// right, all centred on one line.
pub(super) fn volume_row(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    let palette = app.palette;
    let now = app.now_playing();
    let volume = now
        .as_ref()
        .map(|now| now.volume_percent)
        .unwrap_or_else(|| crate::app::volume_to_percent(app.local.volume));
    let shown = match app.volume_preview {
        Some(fraction) => (fraction * 100.0).round() as u8,
        None => volume,
    };
    let adjustable = now.as_ref().is_none_or(|now| now.can_set_volume);
    let k = app.settings.controls_scale_value().clamp(0.8, 1.6);
    let mid = rect.center().y;
    let icon_box = Rect::from_center_size(pos2(rect.left() + 10.0 * k, mid), vec2(18.0 * k, 18.0 * k));
    let icon = if shown == 0 {
        Icon::VolumeX
    } else if shown < 50 {
        Icon::Volume1
    } else {
        Icon::Volume2
    };
    theme::paint_icon(ui, icon, icon_box, 16.0 * k, palette.secondary);
    let text_w = 40.0 * k;
    let track = Rect::from_min_max(
        pos2(icon_box.right() + 8.0, rect.top()),
        pos2(rect.right() - text_w - 6.0, rect.bottom()),
    );
    if track.width() < 30.0 {
        return;
    }
    // In the pop-out the bar only answers a right-click, a double-click or the
    // wheel (see lyrics.rs), so a press and drag moves the panel instead.
    let locked = ui.data(|data| data.get_temp::<bool>(egui::Id::new("sliders-locked"))).unwrap_or(false);
    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("pop-volume-slider").with("rect"), track));
    let response = ui.interact(
        track.expand2(vec2(6.0, 0.0)),
        egui::Id::new("pop-volume-slider"),
        // Still a drag target when locked, so the panel's own handle does not
        // also move (lyrics.rs moves the panel for a drag that starts here).
        Sense::click_and_drag(),
    );
    let adjustable_now = adjustable && !locked;
    if adjustable && let Some(next) = wheel_volume_at(ui, rect.expand2(vec2(0.0, 4.0)), shown) {
        app.volume_preview = None;
        app.actions.push(Action::SetVolume(next));
    }
    let at = |x: f32| ((x - track.left()) / track.width()).clamp(0.0, 1.0);
    if adjustable_now && let Some(p) = response.interact_pointer_pos().filter(|_| response.dragged() || response.clicked()) {
        let value = at(p.x);
        if response.dragged() {
            app.volume_preview = Some(value);
            if now.as_ref().is_none_or(|now| now.local) {
                app.actions.push(Action::PreviewVolume((value * 100.0).round() as u8));
            }
        }
    }
    if adjustable_now && response.drag_stopped() {
        let value = app.volume_preview.take().unwrap_or(shown as f32 / 100.0);
        app.actions.push(Action::SetVolume((value * 100.0).round() as u8));
    } else if adjustable_now && response.clicked() && let Some(p) = response.interact_pointer_pos() {
        app.volume_preview = None;
        app.actions.push(Action::SetVolume((at(p.x) * 100.0).round() as u8));
    }
    let fraction = shown as f32 / 100.0;
    let line = Rect::from_center_size(pos2(track.center().x, mid), vec2(track.width(), 4.0 * k));
    ui.painter().rect_filled(line, 2.0 * k, palette.dim.gamma_multiply(0.6));
    let filled = Rect::from_min_max(line.min, pos2(line.left() + line.width() * fraction, line.bottom()));
    let hot = response.hovered() || response.dragged();
    ui.painter().rect_filled(filled, 2.0 * k, if hot { palette.accent } else { palette.text });
    let knob = pos2(line.left() + line.width() * fraction, mid);
    ui.painter().circle_filled(knob, if hot { 6.5 * k } else { 5.0 * k }, palette.text);
    ui.painter().text(
        pos2(rect.right(), mid),
        egui::Align2::RIGHT_CENTER,
        format!("{shown}%"),
        theme::regular(12.5 * k),
        palette.secondary,
    );
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // Right-click the speaker or the percentage for the volume menu (add a
    // button, the equalizer). The bar itself keeps right-click as "jump here".
    let zones = [
        icon_box.expand(5.0),
        Rect::from_min_max(pos2(track.right() + 8.0, rect.top()), pos2(rect.right() + 4.0, rect.bottom())),
    ];
    for (i, zone) in zones.iter().enumerate() {
        let response = ui.interact(*zone, egui::Id::new(("pop-volume-menu", i)), Sense::click());
        egui::Popup::context_menu(&response)
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| volume_menu(app, ui, &palette, shown));
    }
}
