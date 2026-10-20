//! The playback queue, as a page or as a side panel.

use std::sync::Arc;

use egui::{Align, Frame, Layout, Margin, Sense};

use crate::api::models::PlayableItem;
use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::model::{Action, DragTrack, Loadable, Page, QueueTab, RowContext};
use crate::theme::{self, Icon};

use super::widgets::{self, TrackRow};

pub fn page(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(8.0);
    let tab = if app.queue_tab == QueueTab::Recents {
        QueueTab::Recents
    } else {
        QueueTab::Queue
    };
    // The same two tabs as the side panel, a grid or list switch, and the
    // clear button that sits with "Playing next".
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (value, label) in [
            (QueueTab::Queue, gettext(app.locale, "Queue")),
            (QueueTab::Recents, gettext(app.locale, "Recent")),
        ] {
            if theme::soft_button(ui, &palette, None, &label, value == tab).clicked() {
                app.actions.push(Action::SetQueueTab(value));
            }
        }
        let grid = app.settings.queue_page_grid;
        let (icon, label, tip) = if grid {
            (Icon::LayoutGrid, "Grid", "Show as a list")
        } else {
            (Icon::LayoutList, "List", "Show as a grid")
        };
        if theme::soft_button(ui, &palette, Some(icon), label, false)
            .on_hover_text(tip)
            .clicked()
        {
            app.settings.queue_page_grid = !grid;
            app.actions.push(Action::SettingsChanged);
        }
    });
    ui.add_space(12.0);
    if tab == QueueTab::Recents
        && !app.recents.loading
        && !app.recents.complete
        && app.recents.items.is_empty()
        && app.recents.error.is_none()
    {
        app.actions.push(Action::LoadMoreRecents);
    }
    match (tab, app.settings.queue_page_grid) {
        (QueueTab::Recents, true) => recents_grid(app, ui),
        (QueueTab::Recents, false) => recents_contents(app, ui),
        (_, true) => queue_grid(app, ui),
        (_, false) => contents(app, ui, false),
    }
}

pub fn side_panel(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let fit = super::yielding_panel(
        ui.ctx(),
        "queue-panel",
        theme::SIDE_PANEL_MIN_WIDTH..=560.0,
        app.settings.queue_width,
        ui.available_width() - super::topbar::least_width(ui.ctx()),
    );
    // Over the picture (full screen visualizer or lyrics) the panel is a
    // little see-through, so the music still shows around the list.
    let over_picture = app.fullscreen_vis || app.lyrics_fullscreen.is_some();
    let fill = if over_picture {
        palette.panel.gamma_multiply(0.78)
    } else if super::player_bar::swirl_behind_panels(app) {
        palette.panel.gamma_multiply(0.45)
    } else {
        palette.panel
    };
    let panel = egui::Panel::right("queue-panel")
        .resizable(true)
        .default_size(app.settings.queue_width)
        .size_range(fit.range.clone())
        .show_separator_line(false)
        .frame(
            Frame::new()
                .fill(fill)
                .inner_margin(Margin::symmetric(12, 12)),
        );
    let response = panel.show(ui, |ui| {
        let window_controls = super::window_controls_reservation(
            ui.ctx(),
            app.show_queue_panel,
            app.show_lyrics_panel,
            ui.available_width(),
        );
        // On Windows the Queue and Recent tabs sit on the same row as the
        // minimise, maximise and close buttons instead of under them.
        let beside = window_controls.queue_top > 0.0;
        let tab = app.queue_tab;
        let mut picked = None;
        let mut open_full = false;
        let mut toggled: Option<usize> = None;
        // The header's menu is opened from the chips and from the empty bar
        // alike; the chips fill the width, so the two tabs are the bar.
        let header_rect = egui::Rect::from_min_size(
            ui.cursor().min,
            egui::vec2(ui.available_width(), 36.0),
        );
        let header = ui.interact(header_rect, ui.id().with("queue-header"), Sense::click());
        let mut chip_responses = Vec::new();
        {
            // Fill the whole row, exactly as tall as the window buttons beside
            // them, and level with them: both start the same distance below
            // the top of the window.
            let tall = 36.0;
            let row_top = if beside {
                ui.ctx().content_rect().top() + super::WINDOW_RESIZE_BORDER
            } else {
                ui.cursor().top()
            };
            let row_rect = egui::Rect::from_min_size(
                egui::pos2(ui.cursor().left(), row_top),
                egui::vec2(ui.available_width(), tall),
            );
            let flow_height = super::WINDOWS_WINDOW_CONTROLS_HEIGHT;
            let flow_width = ui.available_width();
            let mut row_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(row_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            row_ui.spacing_mut().item_spacing.x = 8.0;
            let each = if beside {
                ((row_ui.available_width() - super::WINDOWS_WINDOW_CONTROLS_WIDTH - 8.0) / 2.0)
                    .max(40.0)
            } else {
                ((row_ui.available_width() - 8.0) / 2.0).max(40.0)
            };
            for (value, label) in [
                (QueueTab::Queue, gettext(app.locale, "Queue")),
                (QueueTab::Recents, gettext(app.locale, "Recent")),
            ] {
                let response = theme::soft_button_tall(&mut row_ui, &palette, &label, value == tab, each, tall);
                if value == tab {
                    response.clone().on_hover_text(gettext(app.locale, "Click again for the full page"));
                }
                if response.clicked() {
                    if value == tab {
                        // The tab that is showing: its full page, in the
                        // window, with the panel put away.
                        open_full = true;
                    } else {
                        picked = Some(value);
                    }
                }
                chip_responses.push(response);
            }
            // The page below keeps the room the row always took.
            ui.allocate_space(egui::vec2(flow_width, flow_height));
        }
        // The panel always shows the full row, with the album art. Names
        // alone used to be a switch here, and it was the one that left rows
        // with a cover missing and a bar full of nothing, so it is gone. The
        // length is the one column the name can do without, so that stays a
        // right-click away rather than a Settings row.
        for anchor in std::iter::once(&header).chain(chip_responses.iter()) {
            egui::Popup::context_menu(anchor)
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(230.0)
                .show(|ui| {
                    // Queue and Recent each keep their own choices: what is
                    // ticked here belongs to the tab that is showing.
                    let recents = app.queue_tab == QueueTab::Recents;
                    let settings = &app.settings;
                    let mut rows = if recents {
                        vec![
                            (0, "Numbers", Icon::ListMusic, settings.recents_show_numbers),
                            (1, "Covers", Icon::Disc, settings.recents_show_cover),
                            (2, "Artist names", Icon::User, settings.recents_show_artist),
                            (3, "Playlist icons", Icon::Music, settings.recents_show_icons),
                        ]
                    } else {
                        vec![
                            (0, "Numbers", Icon::ListMusic, settings.queue_show_numbers),
                            (1, "Covers", Icon::Disc, settings.queue_show_cover),
                            (2, "Artist names", Icon::User, settings.queue_show_artist),
                            (3, "Playlist icons", Icon::Music, settings.queue_show_icons),
                        ]
                    };
                    if recents {
                        rows.push((4, "Song times", Icon::Clock, settings.queue_show_time));
                    }
                    rows.push((5, "Song length", Icon::Clock, settings.queue_show_length));
                    for (index, name, icon, shown) in rows.iter() {
                        let index = *index;
                        let label = format!(
                            "{} {}",
                            gettext(app.locale, if *shown { "Hide" } else { "Show" }),
                            gettext(app.locale, name).to_lowercase()
                        );
                        if widgets::menu_item(ui, &palette, Some(*icon), &label) {
                            toggled = Some(index);
                        }
                    }
                });
        }
        if let Some(tab) = picked {
            app.actions.push(Action::SetQueueTab(tab));
        }
        if open_full {
            app.actions.push(Action::Open(Page::Queue));
            app.actions.push(Action::ToggleQueuePanel);
        }
        if let Some(index) = toggled {
            let settings = &mut app.settings;
            if app.queue_tab == QueueTab::Recents {
                match index {
                    0 => settings.recents_show_numbers = !settings.recents_show_numbers,
                    1 => settings.recents_show_cover = !settings.recents_show_cover,
                    2 => settings.recents_show_artist = !settings.recents_show_artist,
                    3 => settings.recents_show_icons = !settings.recents_show_icons,
                    5 => settings.queue_show_length = !settings.queue_show_length,
                    _ => settings.queue_show_time = !settings.queue_show_time,
                }
            } else {
                match index {
                    0 => settings.queue_show_numbers = !settings.queue_show_numbers,
                    1 => settings.queue_show_cover = !settings.queue_show_cover,
                    2 => settings.queue_show_artist = !settings.queue_show_artist,
                    3 => settings.queue_show_icons = !settings.queue_show_icons,
                    5 => settings.queue_show_length = !settings.queue_show_length,
                    _ => {}
                }
            }
            app.actions.push(Action::SettingsChanged);
        }
        ui.add_space(8.0);
        // Lazy load recents when tab becomes visible.
        if app.queue_tab == QueueTab::Recents
            && !app.recents.loading
            && !app.recents.complete
            && app.recents.items.is_empty()
            && app.recents.error.is_none()
        {
            app.actions.push(Action::LoadMoreRecents);
        }
        crate::autoscroll::show(
            ui,
            egui::ScrollArea::vertical()
                .id_salt("queue-panel-scroll")
                .auto_shrink([false, false]),
            egui::Vec2b::new(false, true),
            |ui| match app.queue_tab {
                QueueTab::Queue => contents(app, ui, false),
                QueueTab::Recents => recents_contents(app, ui),
                // Friends moved to Home; an old saved tab shows the queue.
                QueueTab::Friends => contents(app, ui, false),
            },
        );
    });
    let width = response.response.rect.width();
    if (width - app.settings.queue_width).abs() > 1.0
        && super::panel_width_chosen(ui.ctx(), "queue-panel", &fit)
    {
        app.settings.queue_width = width;
        app.actions.push(Action::SettingsChanged);
    }
}

/// A faint trash can beside "Playing next" that brightens under the pointer.
/// Pressing it asks first; the confirm buttons are drawn by `clear_confirm`.
fn clear_button(app: &mut App, ui: &mut egui::Ui) {
    if !app.can_clear_queue() {
        return;
    }
    let palette = app.palette;
    if theme::icon_button(
        ui,
        Icon::Trash,
        18.0,
        palette.secondary.gamma_multiply(0.4),
        palette.text,
        &gettext(app.locale, "Clear queue"),
    )
    .clicked()
    {
        ui.ctx()
            .data_mut(|data| data.insert_temp(egui::Id::new("queue-clear-ask"), true));
    }
}

/// "Yes, clear it" and "Cancel" on the heading's row, right beside the
/// trash can, once it has been pressed.
fn clear_confirm(app: &mut App, ui: &mut egui::Ui) {
    let ask_id = egui::Id::new("queue-clear-ask");
    let asking = ui.ctx().data(|data| data.get_temp::<bool>(ask_id)).unwrap_or(false);
    if !asking {
        return;
    }
    if !app.can_clear_queue() {
        ui.ctx().data_mut(|data| data.insert_temp(ask_id, false));
        return;
    }
    let palette = app.palette;
    ui.spacing_mut().item_spacing.x = 8.0;
    if theme::soft_button(ui, &palette, None, &gettext(app.locale, "Yes, clear it"), false).clicked() {
        app.actions.push(Action::ClearQueue);
        ui.ctx().data_mut(|data| data.insert_temp(ask_id, false));
    }
    if theme::soft_button(ui, &palette, None, &gettext(app.locale, "Cancel"), false).clicked() {
        ui.ctx().data_mut(|data| data.insert_temp(ask_id, false));
    }
}

/// A song was dropped on the queue this frame: the primary button was
/// released while a drag was over the given rect.
fn queue_drop(ui: &egui::Ui, rect: egui::Rect) -> Option<Arc<DragTrack>> {
    if ui.rect_contains_pointer(rect)
        && ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
    {
        egui::DragAndDrop::take_payload::<DragTrack>(ui.ctx())
    } else {
        None
    }
}

/// Space below the now-playing row and below Playing next, before the next
/// section's heading.
const SECTION_GAP: f32 = 14.0;

/// Marks the rows drawn inside as queue rows: no heart and no "more"
/// button, because a right-click on the row offers everything they did.
fn with_queue_rows(
    ui: &mut egui::Ui,
    (hide_artist, hide_icons, hide_numbers): (bool, bool, bool),
    draw: impl FnOnce(&mut egui::Ui),
) {
    let flag = egui::Id::new("queue-rows");
    ui.data_mut(|data| {
        data.insert_temp(flag, true);
        data.insert_temp(egui::Id::new("queue-hide-artist"), hide_artist);
        data.insert_temp(egui::Id::new("queue-hide-icons"), hide_icons);
        data.insert_temp(egui::Id::new("queue-hide-numbers"), hide_numbers);
    });
    draw(ui);
    ui.data_mut(|data| {
        data.insert_temp(flag, false);
        data.insert_temp(egui::Id::new("queue-hide-artist"), false);
        data.insert_temp(egui::Id::new("queue-hide-icons"), false);
        data.insert_temp(egui::Id::new("queue-hide-numbers"), false);
    });
}

pub(super) fn contents(app: &mut App, ui: &mut egui::Ui, compact: bool) {
    let hides = (
        !app.settings.queue_show_artist,
        !app.settings.queue_show_icons,
        !app.settings.queue_show_numbers,
    );
    with_queue_rows(ui, hides, |ui| contents_inner(app, ui, compact));
}

fn contents_inner(app: &mut App, ui: &mut egui::Ui, compact: bool) {
    let palette = app.palette;
    // Neither the Web API nor librespot can reorder or insert into a live
    // remote queue, so without local playback active every drop just
    // appends, same as the "Add to queue" menu item.
    let reorderable = app.queue_locally_reorderable();
    if !reorderable && let Some(track) = queue_drop(ui, ui.clip_rect()) {
        app.actions.push(Action::QueueMany {
            songs: track
                .items
                .iter()
                .map(|item| (item.uri().to_string(), item.name().to_string()))
                .collect(),
        });
    }
    match &app.queue {
        Loadable::Loaded(_) => {}
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::loading_row(ui, &palette, app.locale);
            return;
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            widgets::error_row(ui, app, &error, Some(crate::model::Page::Queue));
            return;
        }
    };
    let queue_uris: Vec<String> = app
        .queue
        .get()
        .map(|queue| queue.queue.iter().map(|item| item.uri().to_string()).collect())
        .unwrap_or_default();
    sync_selection(app, ui, "queue", &queue_uris);
    let picked_rows: std::collections::BTreeSet<usize> =
        app.picked_rows(&Page::Queue).cloned().unwrap_or_default();
    let picked_songs: Vec<PlayableItem> = app
        .queue
        .get()
        .map(|queue| {
            picked_rows
                .iter()
                .filter_map(|row| queue.queue.get(*row).cloned())
                .collect()
        })
        .unwrap_or_default();
    let now = app.now_playing();
    // Prefer the player's current track because the Web API can lag after a
    // skip.
    let current: Option<PlayableItem> = match &now {
        Some(now) => app
            .queue
            .get()
            .and_then(|queue| queue.currently_playing.clone())
            .filter(|item| item.uri() == now.uri)
            .or_else(|| app.now_playing_item()),
        None => app
            .queue
            .get()
            .and_then(|queue| queue.currently_playing.clone()),
    };
    // Where the songs come from, as a small cover beside "Now playing", and
    // only for a playlist (anyone's) or Liked Songs: an album or a radio gets
    // no mark at all.
    // (Switched off: the playing song shows no playlist cover, like the
    // songs after it.)
    let from: Option<crate::model::PlayingFrom> = None;
    if let Some(current) = current.as_ref() {
        ui.horizontal(|ui| {
            theme::text(
                ui,
                gettext(app.locale, "Now playing"),
                theme::semibold(theme::BODY_TEXT),
                palette.text,
            );
            if let Some(from) = &from {
                let (rect, response) =
                    ui.allocate_exact_size(egui::Vec2::splat(22.0), egui::Sense::click());
                match &from.page {
                    Some(Page::Playlist(id)) => {
                        let url = app
                            .known_playlist(id)
                            .and_then(|playlist| crate::api::models::pick_image(&playlist.images, 64))
                            .map(str::to_string);
                        widgets::paint_cover(
                            ui,
                            &palette,
                            url.as_deref(),
                            rect,
                            4.0,
                            Icon::Music,
                            Some(app.backend.art()),
                        );
                    }
                    _ => super::sidebar::liked_cover(ui, rect, 4.0),
                }
                if response.clicked()
                    && let Some(page) = from.page.clone()
                {
                    app.actions.push(Action::Open(page));
                }
                response.on_hover_text(from.name.clone());
            }
        });
        ui.add_space(4.0);
        let context = RowContext::Uris(Arc::from([current.uri().to_string()]));
        widgets::track_row(
            ui,
            app,
            TrackRow {
                index: 0,
                number: Some(1),
                item: current,
                context: &context,
                show_cover: app.settings.queue_show_cover,
                // Never the album: open the song to see it.
                show_album: false,
                added_at: None,
                added_by: None,
                show_added_by: false,
                // The queue row is chosen between names alone and the full
                // row, and its own column set decides which of those two
                // columns are drawn. The tempo rides with the full row.
                show_bpm: false,
                show_time: app.settings.queue_show_length,
                compact,
                thin: false,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
            },
        );
        ui.add_space(SECTION_GAP);
    }
    if queue_is_empty(app) {
        // Nothing queued at all yet: the only slot a drop could land on is
        // the first one.
        if reorderable && let Some(track) = queue_drop(ui, ui.clip_rect()) {
            app.actions.push(Action::InsertInQueue {
                items: track.items.clone(),
                position: 0,
            });
        }
        widgets::empty_state(
            ui,
            &palette,
            Icon::ListVideo,
            &gettext(app.locale, "Nothing queued"),
            &gettext(app.locale, "Queued songs appear here."),
        );
        return;
    }
    let row_height = if compact {
        theme::COMPACT_ROW_HEIGHT
    } else {
        theme::ROW_HEIGHT
    };
    let queue_len = app.queue.get().map(|queue| queue.queue.len()).unwrap_or(0);
    // The user's own songs get their own section on top; the playing
    // context's rows follow under the usual heading. One numbering runs
    // through both, because that is the order things play.
    let queued_len = app.queued_rows_len().min(queue_len);
    if queued_len > 0 {
        // The trash sits with the songs it removes: only this section is
        // the user's to clear, the context below plays itself.
        ui.horizontal(|ui| {
            theme::text(
                ui,
                gettext(
                    app.locale,
                    // Translators: Songs added manually, before the current playlist or album continues.
                    "Playing next",
                ),
                theme::semibold(theme::BODY_TEXT),
                palette.text,
            );
            clear_button(app, ui);
            clear_confirm(app, ui);
        });
        ui.add_space(4.0);
        if reorderable && egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx()) {
            widgets::scroll_during_drag(ui);
        }
        let gap = ui.spacing().item_spacing.y;
        // Calculate the nearest drop slot from fixed row height because
        // virtualized rows are not all available during drawing.
        let list_top = ui.cursor().top();
        // Bound the hit test to Playing next's own rows and the space below
        // them, where the append-at-the-end slot sits. The scroll area's
        // clip rect also covers Next up below, which is never a drop target.
        let section_rect = egui::Rect::from_min_max(
            egui::pos2(ui.clip_rect().left(), list_top),
            egui::pos2(
                ui.clip_rect().right(),
                list_top + queued_len as f32 * (row_height + gap) + SECTION_GAP,
            ),
        );
        let move_slot = reorderable
            .then(|| {
                egui::DragAndDrop::payload::<DragTrack>(ui.ctx())?;
                if !ui.rect_contains_pointer(section_rect) {
                    return None;
                }
                let pos = ui
                    .ctx()
                    .pointer_latest_pos()
                    .filter(|pos| section_rect.contains(*pos))?;
                let row = (pos.y - list_top) / (row_height + gap);
                (row >= 0.0).then(|| (row.round() as usize).min(queued_len))
            })
            .flatten();
        widgets::virtual_rows(ui, queued_len, row_height + gap, |ui, index| {
            let width = ui.available_width();
            let shift = ui.ctx().animate_value_with_time(
                ui.id().with(("queue-move-shift", index)),
                match move_slot {
                    Some(slot) if index < slot => -4.0,
                    Some(_) => 4.0,
                    None => 0.0,
                },
                0.12,
            );
            queue_row(app, ui, index, compact, shift, &picked_rows, &picked_songs);
            ui.allocate_space(egui::vec2(width, gap));
        });
        if let Some(slot) = move_slot {
            let y = list_top + slot as f32 * (row_height + gap);
            ui.painter().hline(
                ui.max_rect().x_range().shrink(8.0),
                y,
                egui::Stroke::new(2.0, palette.accent),
            );
            if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
                && let Some(track) = egui::DragAndDrop::take_payload::<DragTrack>(ui.ctx())
            {
                drop_in_queue(app, ui, &track, slot);
            }
        }
        ui.add_space(SECTION_GAP);
    }
    // With Playing next empty, the panel holds no drop target at all: every
    // row on it belongs to Next up, which plays from the context and is never
    // rewritten. The player bar's Queue button still takes the drop.
    if queue_len > queued_len {
        theme::text(
            ui,
            gettext(
                app.locale,
                // Translators: Upcoming songs from the current playlist or album, after manually queued songs.
                "Next up",
            ),
            theme::semibold(theme::BODY_TEXT),
            palette.text,
        );
        ui.add_space(4.0);
        widgets::virtual_rows(ui, queue_len - queued_len, row_height, |ui, index| {
            queue_row(app, ui, queued_len + index, compact, 0.0, &picked_rows, &picked_songs);
        });
    }
}

fn queue_is_empty(app: &App) -> bool {
    app.queue.get().is_none_or(|queue| queue.queue.is_empty())
}

/// What your Last.fm friends, and the Last.fm names you added, are playing.
pub(super) fn friends_contents(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let view = app.lastfm.view();
    let extra = app.settings.lastfm_friends.clone();
    let signed_in = matches!(view.status, crate::lastfm::Status::SignedIn);
    // Read on opening, then every 15 seconds while it shows.
    let asked_id = egui::Id::new("friends-asked");
    let asked: Option<f64> = ui.data(|data| data.get_temp(asked_id));
    let now = ui.input(|input| input.time);
    if (signed_in || !extra.is_empty())
        && !view.friends_loading
        && (view.friends.is_none() || asked.is_none_or(|at| now - at > 15.0))
    {
        app.lastfm.friends(extra.clone());
        ui.data_mut(|data| data.insert_temp(asked_id, now));
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(5));
    // Add a Last.fm name by hand.
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let id = egui::Id::new("friends-add-name");
        let mut name = ui.data(|data| data.get_temp::<String>(id)).unwrap_or_default();
        let field = egui::TextEdit::singleline(&mut name)
            .hint_text("Add a Last.fm name")
            .desired_width((ui.available_width() - 60.0).max(80.0))
            .show(ui)
            .response;
        let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if (theme::soft_button(ui, &palette, Some(Icon::Plus), "", false).on_hover_text("Add").clicked() || enter)
            && !name.trim().is_empty()
        {
            let added = name.trim().to_string();
            if !app.settings.lastfm_friends.iter().any(|have| have.eq_ignore_ascii_case(&added)) {
                app.settings.lastfm_friends.push(added);
                app.mark_settings_dirty();
                let extra = app.settings.lastfm_friends.clone();
                app.lastfm.friends(extra);
                ui.data_mut(|data| data.insert_temp(asked_id, now));
            }
            name.clear();
        }
        ui.data_mut(|data| data.insert_temp(id, name));
    });
    ui.add_space(6.0);
    let Some(friends) = view.friends.clone() else {
        if view.friends_loading {
            widgets::loading_row(ui, &palette, app.locale);
        } else if !signed_in && extra.is_empty() {
            theme::hint(ui, &palette, "Sign in to Last.fm in Settings to see your friends, or add a Last.fm name above.");
        }
        return;
    };
    if friends.is_empty() {
        theme::hint(ui, &palette, "No Last.fm friends yet. Add a Last.fm name above.");
        return;
    }
    let unix_now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    for friend in &friends {
        let row = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let picture = friend.avatar.as_deref().or(friend.image.as_deref());
            widgets::cover(ui, &palette, picture, 40.0, 20.0, Icon::User);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(&friend.user)
                            .font(theme::semibold(theme::BODY_TEXT))
                            .color(palette.text),
                    )
                    .truncate(),
                );
                let song = if let Some(error) = &friend.error {
                    error.clone()
                } else if friend.title.is_empty() {
                    "Nothing played yet".to_string()
                } else {
                    format!("{} \u{2014} {}", friend.title, friend.artist)
                };
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(song)
                            .font(theme::regular(theme::SMALL_TEXT))
                            .color(if friend.now { palette.accent } else { palette.secondary }),
                    )
                    .truncate(),
                );
                let when = if friend.now {
                    "Playing now".to_string()
                } else if friend.at > 0 {
                    let minutes = (unix_now - friend.at).max(0) / 60;
                    match minutes {
                        0 => "Just now".to_string(),
                        1..=59 => format!("{minutes} min ago"),
                        60..=1439 => format!("{} h ago", minutes / 60),
                        _ => format!("{} d ago", minutes / 1440),
                    }
                } else {
                    String::new()
                };
                if !when.is_empty() {
                    theme::hint(ui, &palette, &when);
                }
            });
        });
        let response = ui.interact(row.response.rect, ui.id().with(("friend", &friend.user)), egui::Sense::click());
        let search = format!("{} {}", friend.title, friend.artist).trim().to_string();
        if response.double_clicked() && !friend.title.is_empty() {
            app.actions.push(Action::Search(search.clone()));
        }
        egui::Popup::context_menu(&response)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| {
                widgets::fit_menu(ui, 280.0);
                if !friend.title.is_empty()
                    && widgets::menu_item(ui, &palette, Some(Icon::Search), "Find this song")
                {
                    app.actions.push(Action::Search(search.clone()));
                }
                if widgets::menu_item(ui, &palette, Some(Icon::ExternalLink), "Open their Last.fm") {
                    app.actions.push(Action::OpenUrl(format!(
                        "https://www.last.fm/user/{}",
                        super::settings::last_fm_part(&friend.user)
                    )));
                }
                let added = app.settings.lastfm_friends.iter().position(|have| have.eq_ignore_ascii_case(&friend.user));
                if let Some(at) = added
                    && widgets::menu_item(ui, &palette, Some(Icon::Trash), "Remove")
                {
                    app.settings.lastfm_friends.remove(at);
                    app.mark_settings_dirty();
                    let extra = app.settings.lastfm_friends.clone();
                    app.lastfm.friends(extra);
                }
            });
        ui.add_space(6.0);
    }
}

fn recents_contents(app: &mut App, ui: &mut egui::Ui) {
    let hides = (
        !app.settings.recents_show_artist,
        !app.settings.recents_show_icons,
        !app.settings.recents_show_numbers,
    );
    with_queue_rows(ui, hides, |ui| recents_contents_inner(app, ui));
}

fn recents_contents_inner(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    // Snapshot to avoid borrow issues while drawing. The rows are both
    // histories as one: what was played here, which Spotify is never told
    // about, and what Spotify knows of every other device.
    let items = app.recents_view.clone();
    let loading = app.recents.loading;
    let error = app.recents.error.clone();
    let complete = app.recents.complete;
    let loaded_once = app.recents.loaded_once;

    if items.is_empty() {
        if loading {
            widgets::loading_row(ui, &palette, app.locale);
            return;
        }
        if let Some(err) = error {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                theme::icon(ui, Icon::CircleAlert, 16.0, palette.danger);
                theme::text(ui, &err, theme::regular(theme::BODY_TEXT), palette.secondary);
                if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Refresh),
                    &gettext(app.locale, "Retry"),
                    false,
                )
                .clicked()
                {
                    app.actions.push(Action::ReloadRecents);
                }
            });
            return;
        }
        if loaded_once {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Clock,
                &gettext(app.locale, "No recent plays"),
                &gettext(app.locale, "Played songs appear here."),
            );
        } else {
            widgets::loading_row(ui, &palette, app.locale);
        }
        return;
    }

    // Show error inline if we have items but also an error on next page.
    if let Some(err) = error {
        ui.horizontal(|ui| {
            theme::icon(ui, Icon::CircleAlert, 14.0, palette.danger);
            theme::text(ui, &err, theme::regular(theme::SMALL_TEXT), palette.secondary);
            if theme::soft_button(
                ui,
                &palette,
                Some(Icon::Refresh),
                &gettext(app.locale, "Retry"),
                false,
            )
            .clicked()
            {
                app.actions.push(Action::LoadMoreRecents);
            }
        });
        ui.add_space(6.0);
    }

    let row_height = theme::COMPACT_ROW_HEIGHT;
    let recent_uris: Vec<String> = items.iter().map(|play| play.track.uri.clone()).collect();
    sync_selection(app, ui, "recents", &recent_uris);
    let picked_rows: std::collections::BTreeSet<usize> =
        app.picked_rows(&Page::Queue).cloned().unwrap_or_default();
    let picked_songs: Vec<PlayableItem> = picked_rows
        .iter()
        .filter_map(|row| items.get(*row))
        .map(|play| PlayableItem::Track(play.track.clone()))
        .collect();
    // Build PlayableItems on the fly; virtual_rows needs stable index.
    widgets::virtual_rows(ui, items.len(), row_height, |ui, index| {
        let entry = &items[index];
        // Need owned PlayableItem for track_row; clone track.
        let item = PlayableItem::Track(entry.track.clone());
        let context = RowContext::Uris(Arc::from([entry.track.uri.clone()]));
        let pick = widgets::track_row(
            ui,
            app,
            TrackRow {
                index,
                number: Some(index + 1),
                item: &item,
                context: &context,
                show_cover: app.settings.recents_show_cover,
                show_album: false,
                // "Song times" on Recent means how long ago it played.
                added_at: if app.settings.queue_show_time {
                    entry.played_at.as_deref()
                } else {
                    None
                },
                added_by: None,
                show_added_by: false,
                // Recents is always the narrow row, so no tempo here.
                show_bpm: false,
                show_time: app.settings.queue_show_length,
                compact: true,
                thin: false,
                shift: 0.0,
                picked: picked_rows.contains(&index),
                picked_songs: &picked_songs,
            },
        );
        if let Some(pick) = pick {
            pick_in(app, ui, "recents", index, pick, items.len());
        }
    });

    // Footer: loading more or load more trigger
    if loading {
        ui.add_space(8.0);
        widgets::loading_row(ui, &palette, app.locale);
    } else if !complete {
        ui.add_space(8.0);
        // Auto-load when near end, plus manual button as fallback.
        let can_load = app.recents.can_load_more();
        // Check if scroll is near end (same heuristic as widgets::load_more_when_near_end)
        let clip = ui.clip_rect();
        let cursor = ui.cursor().top();
        if can_load && cursor - clip.bottom() < 900.0 {
            app.actions.push(Action::LoadMoreRecents);
        }
        if theme::soft_button(
            ui,
            &palette,
            Some(Icon::Refresh),
            &gettext(app.locale, "Load more"),
            false,
        )
        .clicked()
        {
            app.actions.push(Action::LoadMoreRecents);
        }
    }
}

/// One row of the queue, numbered and indexed by its place in the whole
/// queue, whichever section it sits in. `shift` parts rows around the slot
/// a dragged row would land in, in the "Playing next" section only.
fn queue_row(
    app: &mut App,
    ui: &mut egui::Ui,
    index: usize,
    compact: bool,
    shift: f32,
    picked_rows: &std::collections::BTreeSet<usize>,
    picked_songs: &[PlayableItem],
) {
    let Some(item) = app
        .queue
        .get()
        .and_then(|queue| queue.queue.get(index))
        .cloned()
    else {
        return;
    };
    let len = app.queue.get().map_or(0, |queue| queue.queue.len());
    let pick = widgets::track_row(
        ui,
        app,
        TrackRow {
            index,
            number: Some(index + 1),
            item: &item,
            context: &RowContext::Queue,
            show_cover: app.settings.queue_show_cover,
            // No album and no tempo here. This is a side panel, and every
            // column it gives up goes to the song name — which is the only
            // thing in a queue row a reader actually reads. The cover is
            // the album.
            show_album: false,
            added_at: None,
            added_by: None,
            show_added_by: false,
            show_bpm: false,
            show_time: app.settings.queue_show_length,
            compact,
            thin: false,
            shift,
            picked: picked_rows.contains(&index),
            picked_songs,
        },
    );
    if let Some(pick) = pick {
        pick_in(app, ui, "queue", index, pick, len);
    }
}

/// Applies a click's selection and remembers which songs the rows are, so a
/// queue that moves under the selection is noticed.
fn pick_in(app: &mut App, ui: &egui::Ui, view: &str, index: usize, pick: crate::model::RowPick, len: usize) {
    app.pick_row(&Page::Queue, view, index, pick, len);
    let uris: Vec<(usize, String)> = {
        let rows = app.picked_rows(&Page::Queue).cloned().unwrap_or_default();
        let known: Vec<String> = if view == "queue" {
            app.queue
                .get()
                .map(|queue| queue.queue.iter().map(|item| item.uri().to_string()).collect())
                .unwrap_or_default()
        } else {
            app.recents_view.iter().map(|play| play.track.uri.clone()).collect()
        };
        rows.into_iter()
            .filter_map(|row| known.get(row).map(|uri| (row, uri.clone())))
            .collect()
    };
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("queue-selection-uris"), uris));
}

/// Drops the selection when the rows it names are no longer those songs
/// (the queue advanced, or something was added ahead of them), or when the
/// list shown is another one.
fn sync_selection(app: &mut App, ui: &egui::Ui, view: &str, uris: &[String]) {
    app.keep_picked_rows_for(&Page::Queue, view);
    if app.picked_rows(&Page::Queue).is_none() {
        return;
    }
    let held: Vec<(usize, String)> = ui
        .ctx()
        .data(|data| data.get_temp(egui::Id::new("queue-selection-uris")))
        .unwrap_or_default();
    let intact = !held.is_empty()
        && held
            .iter()
            .all(|(row, uri)| uris.get(*row).is_some_and(|now| now == uri));
    if !intact {
        app.clear_picked_rows();
    }
}

/// A song or songs dropped on the queue at `slot`: rows picked in the queue
/// move together, one row moves, and anything else is inserted.
fn drop_in_queue(app: &mut App, ui: &egui::Ui, track: &DragTrack, slot: usize) {
    let key = egui::Id::new("queue-drag-rows");
    let dragged: Option<Vec<usize>> = ui.ctx().data(|data| data.get_temp(key));
    ui.ctx().data_mut(|data| data.remove::<Vec<usize>>(key));
    if track.items.len() > 1
        && let Some(rows) = dragged
        && rows.len() == track.items.len()
        && rows
            .iter()
            .zip(&track.items)
            .all(|(row, item)| app.manual_queue.get(*row).map(String::as_str) == Some(item.uri()))
    {
        app.actions.push(Action::MoveManyInQueue { froms: rows, to: slot });
        return;
    }
    match track.from.as_ref() {
        Some((origin, from)) if origin == "queue" => {
            let from = *from as usize;
            let uri = track.items[0].uri();
            // Playback can advance mid-drag and shift queue indices; trust
            // the recorded one only if it still names the dragged song,
            // otherwise find where that song actually sits now. Not there at
            // all means it left the queue, so there is nothing left to move.
            let from = if app.manual_queue.get(from).map(String::as_str) == Some(uri) {
                Some(from)
            } else {
                app.manual_queue.iter().position(|queued| queued == uri)
            };
            // A row dropped back on its own edges moves nothing.
            if let Some(from) = from
                && slot != from
                && slot != from + 1
            {
                app.actions.push(Action::MoveInQueue { from, to: slot });
            }
        }
        _ => {
            app.actions.push(Action::InsertInQueue {
                items: track.items.clone(),
                position: slot,
            });
        }
    }
}

// ---- the full page, as a grid ------------------------------------------------

const TILE_PAD: f32 = 12.0;
const TILE_GAP: f32 = 8.0;

/// Where the tiles of a grid sit: `cols` to a row, each cell `cell` big,
/// from `origin`.
#[derive(Clone, Copy)]
struct TileGrid {
    origin: egui::Pos2,
    cols: usize,
    cell: egui::Vec2,
    tile: egui::Vec2,
}

impl TileGrid {
    fn new(ui: &egui::Ui, origin: egui::Pos2) -> Self {
        let width = widgets::CARD_WIDTH;
        let side = width - 2.0 * TILE_PAD;
        let tile = egui::vec2(width, TILE_PAD + side + 8.0 + 18.0 + 2.0 + 16.0 + TILE_PAD);
        let cell = tile + egui::vec2(TILE_GAP, TILE_GAP);
        let cols = (((ui.available_width() + TILE_GAP) / cell.x).floor() as usize).max(1);
        Self { origin, cols, cell, tile }
    }

    fn rect(&self, slot: usize) -> egui::Rect {
        let (row, col) = (slot / self.cols, slot % self.cols);
        egui::Rect::from_min_size(
            self.origin + egui::vec2(col as f32 * self.cell.x, row as f32 * self.cell.y),
            self.tile,
        )
    }

    /// The place between tiles a pointer at `pos` points at, from 0 to `count`.
    fn slot_at(&self, pos: egui::Pos2, count: usize) -> usize {
        let row = ((pos.y - self.origin.y) / self.cell.y).floor().max(0.0) as usize;
        let across = (pos.x - self.origin.x) / self.cell.x;
        let col = (across.floor().max(0.0) as usize).min(self.cols - 1);
        let right_half = across - col as f32 >= 0.5;
        (row * self.cols + col + usize::from(right_half)).min(count)
    }
}

/// What a tile did this frame.
#[derive(Default)]
struct TileEvents {
    pick: Option<crate::model::RowPick>,
    play: bool,
    response: Option<egui::Response>,
}

/// One song as a cover tile: its number, cover, name and artist; a hover play
/// button; a tint when picked.
fn song_tile(
    app: &mut App,
    ui: &mut egui::Ui,
    view: &str,
    rect: egui::Rect,
    index: usize,
    item: &PlayableItem,
    picked: bool,
) -> TileEvents {
    let palette = app.palette;
    let response = ui.interact(
        rect,
        ui.id().with(("queue-tile", view, index)),
        Sense::click_and_drag(),
    );
    let hovered = response.hovered() || ui.rect_contains_pointer(rect);
    if picked {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::RADIUS),
            palette.accent.gamma_multiply(0.28),
        );
    } else if hovered {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::RADIUS),
            palette.surface_hover,
        );
    }
    let side = rect.width() - 2.0 * TILE_PAD;
    let cover = egui::Rect::from_min_size(rect.min + egui::vec2(TILE_PAD, TILE_PAD), egui::Vec2::splat(side));
    widgets::paint_shadow(ui, &palette, cover, 6.0);
    widgets::paint_cover(
        ui,
        &palette,
        item.image(300),
        cover,
        6.0,
        if item.is_track() { Icon::Music } else { Icon::Mic },
        Some(app.backend.art()),
    );
    // The place in the queue, in a small chip on the cover.
    let badge = ui.painter().layout_no_wrap(
        format!("{}", index + 1),
        theme::semibold(theme::SMALL_TEXT),
        egui::Color32::WHITE,
    );
    let chip = egui::Rect::from_min_size(
        cover.min + egui::vec2(6.0, 6.0),
        badge.size() + egui::vec2(10.0, 4.0),
    );
    ui.painter()
        .rect_filled(chip, egui::CornerRadius::same(6), egui::Color32::from_black_alpha(150));
    ui.painter().galley(chip.min + egui::vec2(5.0, 2.0), badge, egui::Color32::WHITE);
    let text_width = side;
    let title = widgets::ellipsized(ui, item.name(), theme::semibold(theme::BODY_TEXT), palette.text, text_width, 1);
    let title_pos = egui::pos2(rect.left() + TILE_PAD, cover.bottom() + 8.0);
    let title_height = title.size().y;
    ui.painter().galley(title_pos, title, palette.text);
    let subtitle = widgets::ellipsized(
        ui,
        &item.subtitle(),
        theme::regular(theme::SMALL_TEXT),
        palette.secondary,
        text_width,
        1,
    );
    ui.painter().galley(
        egui::pos2(title_pos.x, title_pos.y + title_height + 2.0),
        subtitle,
        palette.secondary,
    );
    let mut events = TileEvents::default();
    if hovered && !ui.ctx().dragged_id().is_some() {
        let button = egui::Rect::from_center_size(cover.right_bottom() - egui::vec2(24.0, 24.0), egui::Vec2::splat(36.0));
        let play = ui.interact(button, ui.id().with(("queue-tile-play", view, index)), Sense::click());
        ui.painter().circle_filled(
            button.center(),
            18.0,
            if play.hovered() { palette.accent_hover } else { palette.accent },
        );
        theme::paint_icon(ui, Icon::PlayFilled, button, 16.0, palette.on_accent);
        events.play = play.clicked();
    }
    if response.clicked() && !events.play {
        let modifiers = ui.input(|input| input.modifiers);
        events.pick = Some(if modifiers.shift {
            crate::model::RowPick::Range
        } else if modifiers.command {
            crate::model::RowPick::Toggle
        } else {
            crate::model::RowPick::Only
        });
    }
    events.response = Some(response);
    events
}

/// The queue as tiles: "Playing next" can be rearranged by dragging (several
/// picked tiles together), "Next up" plays itself and can only be picked.
fn queue_grid(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    match &app.queue {
        Loadable::Loaded(_) => {}
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::loading_row(ui, &palette, app.locale);
            return;
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            widgets::error_row(ui, app, &error, Some(Page::Queue));
            return;
        }
    }
    let items: Vec<PlayableItem> = app.queue.get().map(|queue| queue.queue.clone()).unwrap_or_default();
    if items.is_empty() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::ListVideo,
            &gettext(app.locale, "Nothing queued"),
            &gettext(app.locale, "Queued songs appear here."),
        );
        return;
    }
    let uris: Vec<String> = items.iter().map(|item| item.uri().to_string()).collect();
    sync_selection(app, ui, "queue", &uris);
    let queued_len = app.queued_rows_len().min(items.len());
    let reorderable = app.queue_locally_reorderable();
    selection_bar(app, ui, &items, queued_len);
    if queued_len > 0 {
        ui.horizontal(|ui| {
            theme::text(
                ui,
                gettext(app.locale, "Playing next"),
                theme::semibold(theme::SECTION_TEXT),
                palette.text,
            );
            clear_button(app, ui);
            clear_confirm(app, ui);
        });
        ui.add_space(6.0);
        tile_section(app, ui, "queue", &items[..queued_len], 0, queued_len, reorderable);
        ui.add_space(SECTION_GAP);
    }
    if items.len() > queued_len {
        theme::text(
            ui,
            gettext(app.locale, "Next up"),
            theme::semibold(theme::SECTION_TEXT),
            palette.text,
        );
        ui.add_space(6.0);
        tile_section(app, ui, "queue", &items[queued_len..], queued_len, queued_len, false);
    }
}

/// How many are picked, with what can be done to them at once.
fn selection_bar(app: &mut App, ui: &mut egui::Ui, items: &[PlayableItem], queued_len: usize) {
    let palette = app.palette;
    let Some(rows) = app.picked_rows(&Page::Queue).cloned() else {
        return;
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        theme::text(
            ui,
            format!("{} selected", rows.len()),
            theme::semibold(theme::BODY_TEXT),
            palette.text,
        );
        let removable: Vec<(usize, String)> = rows
            .iter()
            .filter(|row| **row < queued_len && app.can_remove_from_queue(**row))
            .filter_map(|row| items.get(*row).map(|item| (*row, item.uri().to_string())))
            .collect();
        if !removable.is_empty()
            && theme::soft_button(ui, &palette, Some(Icon::Minus), &gettext(app.locale, "Remove from queue"), false)
                .clicked()
        {
            app.actions.push(Action::RemoveFromQueue { rows: removable });
        }
        if theme::soft_button(ui, &palette, Some(Icon::X), &gettext(app.locale, "Clear selection"), false).clicked() {
            app.clear_picked_rows();
        }
    });
    ui.add_space(8.0);
}

/// A run of tiles starting at queue row `first`. `movable_rows` is how many
/// rows from the top of the queue may be rearranged; `reorder` says whether
/// this run is one of them.
fn tile_section(
    app: &mut App,
    ui: &mut egui::Ui,
    view: &str,
    items: &[PlayableItem],
    first: usize,
    movable_rows: usize,
    reorder: bool,
) {
    let palette = app.palette;
    let grid_probe = TileGrid::new(ui, ui.cursor().min);
    let rows = items.len().div_ceil(grid_probe.cols);
    let (area, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), rows as f32 * grid_probe.cell.y),
        Sense::hover(),
    );
    let grid = TileGrid { origin: area.min, ..grid_probe };
    let picked_rows = app.picked_rows(&Page::Queue).cloned().unwrap_or_default();
    let picked_songs: Vec<PlayableItem> = if view == "queue" {
        app.queue
            .get()
            .map(|queue| {
                picked_rows
                    .iter()
                    .filter_map(|row| queue.queue.get(*row).cloned())
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let dragging = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
    if dragging && reorder {
        widgets::scroll_during_drag(ui);
    }
    // Where a drop would land, while something is held over the section.
    let slot = (dragging && reorder)
        .then(|| {
            let pos = ui.ctx().pointer_latest_pos()?;
            let section = area.expand(TILE_GAP * 2.0);
            section.contains(pos).then(|| grid.slot_at(pos, items.len()))
        })
        .flatten();
    let clip = ui.clip_rect();
    let first_row = (((clip.top() - area.top()) / grid.cell.y).floor().max(0.0) as usize).saturating_sub(1);
    let last_row = (((clip.bottom() - area.top()) / grid.cell.y).ceil().max(0.0) as usize + 1).min(rows);
    for row in first_row..last_row {
        for col in 0..grid.cols {
            let local = row * grid.cols + col;
            let Some(item) = items.get(local) else { break };
            let index = first + local;
            let rect = grid.rect(local);
            let picked = picked_rows.contains(&index);
            let events = song_tile(app, ui, view, rect, index, item, picked);
            if let Some(pick) = events.pick {
                let total = app.queue.get().map_or(0, |queue| queue.queue.len());
                pick_in(app, ui, view, index, pick, total);
            }
            if events.play {
                app.actions.push(Action::PlayFromRow {
                    context: RowContext::Queue,
                    uri: item.uri().to_string(),
                    index: index as u32,
                });
            }
            let Some(response) = events.response else { continue };
            if response.double_clicked() {
                app.actions.push(Action::PlayFromRow {
                    context: RowContext::Queue,
                    uri: item.uri().to_string(),
                    index: index as u32,
                });
            }
            if item.is_track() && response.drag_started_by(egui::PointerButton::Primary) {
                let several = picked && picked_songs.len() > 1;
                let dragged_items = if several { picked_songs.clone() } else { vec![item.clone()] };
                if several {
                    let rows: Vec<usize> = picked_rows.iter().copied().collect();
                    ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("queue-drag-rows"), rows));
                }
                let from = (!several && index < movable_rows && app.queue_locally_reorderable())
                    .then(|| ("queue".to_string(), index as u32));
                egui::DragAndDrop::set_payload(
                    ui.ctx(),
                    DragTrack {
                        title: item.name().to_string(),
                        image: item.image(64).map(str::to_string),
                        items: dragged_items,
                        from,
                        source_playlist: None,
                    },
                );
            }
            egui::Popup::context_menu(&response)
                .id(ui.make_persistent_id(("queue-tile-menu", view, index)))
                .frame(widgets::menu_frame(&palette))
                .show(|ui| {
                    if picked && picked_songs.len() > 1 {
                        widgets::picked_menu(ui, app, &picked_songs, None);
                        let removable: Vec<(usize, String)> = picked_rows
                            .iter()
                            .filter(|row| **row < movable_rows && app.can_remove_from_queue(**row))
                            .filter_map(|row| {
                                app.queue
                                    .get()
                                    .and_then(|queue| queue.queue.get(*row))
                                    .map(|item| (*row, item.uri().to_string()))
                            })
                            .collect();
                        if !removable.is_empty()
                            && widgets::menu_item(ui, &palette, Some(Icon::Minus), &gettext(app.locale, "Remove from queue"))
                        {
                            app.actions.push(Action::RemoveFromQueue { rows: removable });
                        }
                    } else {
                        widgets::item_menu(ui, app, item, Some(&RowContext::Queue), Some(index));
                    }
                });
        }
    }
    // The drop marker, and the drop itself when the button is let go.
    if let Some(slot) = slot {
        let at = grid.rect(slot.min(items.len().saturating_sub(1)));
        let x = if slot >= items.len() { at.right() + TILE_GAP / 2.0 } else { at.left() - TILE_GAP / 2.0 };
        ui.painter().vline(
            x,
            egui::Rangef::new(at.top() + 6.0, at.bottom() - 6.0),
            egui::Stroke::new(3.0, palette.accent),
        );
        if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
            && let Some(track) = egui::DragAndDrop::take_payload::<DragTrack>(ui.ctx())
        {
            drop_in_queue(app, ui, &track, first + slot);
        }
    }
}

/// The Recent tab as tiles, newest first.
fn recents_grid(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let plays = app.recents_view.clone();
    if plays.is_empty() {
        if app.recents.loading {
            widgets::loading_row(ui, &palette, app.locale);
        } else if let Some(error) = app.recents.error.clone() {
            theme::text(ui, &error, theme::regular(theme::BODY_TEXT), palette.secondary);
        } else if app.recents.loaded_once {
            widgets::empty_state(
                ui,
                &palette,
                Icon::Clock,
                &gettext(app.locale, "No recent plays"),
                &gettext(app.locale, "Played songs appear here."),
            );
        } else {
            widgets::loading_row(ui, &palette, app.locale);
        }
        return;
    }
    let uris: Vec<String> = plays.iter().map(|play| play.track.uri.clone()).collect();
    sync_selection(app, ui, "recents", &uris);
    let items: Vec<PlayableItem> = plays.iter().map(|play| PlayableItem::Track(play.track.clone())).collect();
    // Recent songs are not the queue: pick, play, and take them elsewhere.
    recent_tiles(app, ui, &items);
    if app.recents.loading {
        ui.add_space(8.0);
        widgets::loading_row(ui, &palette, app.locale);
    } else if !app.recents.complete {
        ui.add_space(8.0);
        if app.recents.can_load_more() {
            app.actions.push(Action::LoadMoreRecents);
        }
    }
}

fn recent_tiles(app: &mut App, ui: &mut egui::Ui, items: &[PlayableItem]) {
    let palette = app.palette;
    let grid_probe = TileGrid::new(ui, ui.cursor().min);
    let rows = items.len().div_ceil(grid_probe.cols);
    let (area, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), rows as f32 * grid_probe.cell.y),
        Sense::hover(),
    );
    let grid = TileGrid { origin: area.min, ..grid_probe };
    let picked_rows = app.picked_rows(&Page::Queue).cloned().unwrap_or_default();
    let picked_songs: Vec<PlayableItem> = picked_rows.iter().filter_map(|row| items.get(*row).cloned()).collect();
    let clip = ui.clip_rect();
    let first_row = (((clip.top() - area.top()) / grid.cell.y).floor().max(0.0) as usize).saturating_sub(1);
    let last_row = (((clip.bottom() - area.top()) / grid.cell.y).ceil().max(0.0) as usize + 1).min(rows);
    for row in first_row..last_row {
        for col in 0..grid.cols {
            let index = row * grid.cols + col;
            let Some(item) = items.get(index) else { break };
            let picked = picked_rows.contains(&index);
            let events = song_tile(app, ui, "recents", grid.rect(index), index, item, picked);
            if let Some(pick) = events.pick {
                pick_in(app, ui, "recents", index, pick, items.len());
            }
            let context = RowContext::Uris(Arc::from([item.uri().to_string()]));
            if events.play || events.response.as_ref().is_some_and(|response| response.double_clicked()) {
                app.actions.push(Action::PlayFromRow {
                    context: context.clone(),
                    uri: item.uri().to_string(),
                    index: 0,
                });
            }
            let Some(response) = events.response else { continue };
            if item.is_track() && response.drag_started_by(egui::PointerButton::Primary) {
                let several = picked && picked_songs.len() > 1;
                egui::DragAndDrop::set_payload(
                    ui.ctx(),
                    DragTrack {
                        title: item.name().to_string(),
                        image: item.image(64).map(str::to_string),
                        items: if several { picked_songs.clone() } else { vec![item.clone()] },
                        from: None,
                        source_playlist: None,
                    },
                );
            }
            egui::Popup::context_menu(&response)
                .id(ui.make_persistent_id(("recent-tile-menu", index)))
                .frame(widgets::menu_frame(&palette))
                .show(|ui| {
                    if picked && picked_songs.len() > 1 {
                        widgets::picked_menu(ui, app, &picked_songs, None);
                    } else {
                        widgets::item_menu(ui, app, item, Some(&context), Some(index));
                    }
                });
        }
    }
}
