//! The Local songs page: audio files in the index's songs folder, with the
//! tempo, key and waveform measured from the sound, and playlists imported
//! from Exportify files that can be browsed with no internet.

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

use crate::app::App;
use crate::model::Action;
use crate::theme::{self, Icon};

const ROW: f32 = 34.0;

/// One line of whichever table is open, already turned into text, one
/// cell per column id (see [`COLUMNS`]).
struct Line {
    cells: [String; 9],
    /// For the sort: the number behind the BPM column, if any.
    bpm: Option<f32>,
    length_ms: u64,
    /// A tick or a note beside the title (an imported song with a file).
    have_file: Option<bool>,
    /// The file to play for this line, if there is one.
    path: Option<String>,
    /// The Spotify song this file is, once it has been matched.
    spotify: Option<String>,
}

/// Every column the table can show, by id: its heading and its share of the
/// width when first shown.
const COLUMNS: [(&str, f32); 9] = [
    ("#", 0.05),
    ("TITLE", 0.30),
    ("ARTIST", 0.20),
    ("ALBUM", 0.18),
    ("ADDED", 0.09),
    ("BPM", 0.06),
    ("KEY", 0.06),
    ("LENGTH", 0.07),
    ("GENRE", 0.10),
];

/// The columns shown, in order, with their shares; a saved list with
/// nothing usable in it falls back to the usual set.
fn shown_columns(app: &App) -> Vec<(u8, f32)> {
    let mut seen = [false; COLUMNS.len()];
    let cols: Vec<(u8, f32)> = app
        .settings
        .local_columns
        .iter()
        .filter(|(id, _)| {
            let id = *id as usize;
            id < COLUMNS.len() && !std::mem::replace(&mut seen[id], true)
        })
        .map(|(id, share)| (*id, share.clamp(0.02, 0.8)))
        .collect();
    if cols.iter().any(|(id, _)| *id == 1) {
        cols
    } else {
        crate::settings::Settings::default().local_columns
    }
}

/// A day count since 1970 as `YYYY-MM-DD` (no time zone: the day the file
/// was last written, which is when it was added to the folder).
fn day_text(seconds: u64) -> String {
    if seconds == 0 {
        return String::new();
    }
    let z = (seconds / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn length(ms: u64) -> String {
    let s = ms / 1000;
    if s == 0 { String::new() } else { format!("{}:{:02}", s / 60, s % 60) }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let index = app.dirs.index_dir();
    if let Some(library) = app.local_scan.take_finished() {
        app.local_songs = library;
        app.local_sync.forget_genres();
    }
    app.refresh_local_genres();
    app.request_local_saved();
    let scan = app.local_scan.view();
    if scan.running {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }

    ui.add_space(8.0);
    theme::text(ui, "Local songs", theme::bold(theme::TITLE_TEXT), palette.text);
    ui.add_space(4.0);
    theme::subtle(
        ui,
        &palette,
        &format!(
            "Put song files (mp3, flac, ogg, wav, m4a) in {} or add a folder of your own, then press Scan. Double-click a song to play it. Each file is measured once and remembered.",
            crate::local_library::songs_dir(&index).display()
        ),
    );
    ui.add_space(10.0);

    // Buttons.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let label = if scan.running { "Scanning…" } else { "Scan songs folder" };
        if theme::soft_button(ui, &palette, Some(Icon::Refresh), label, scan.running).clicked()
            && !scan.running
        {
            app.actions.push(Action::ScanMusic);
        }
        if theme::soft_button(ui, &palette, Some(Icon::Plus), "Add a music folder", false)
            .clicked()
        {
            app.actions.push(Action::AddMusicFolder);
        }
        if theme::soft_button(ui, &palette, Some(Icon::Plus), "Add song files", false).clicked() {
            app.actions.push(Action::AddMusicFiles);
        }
        let matching = app.local_sync.progress();
        let match_label = match matching {
            Some((done, total)) => format!("Matching with Spotify… {done}/{total}"),
            None => match app.local_matches.matched() {
                0 => "Match with Spotify".to_string(),
                found => format!("Match with Spotify ({found} found)"),
            },
        };
        if theme::soft_button(ui, &palette, Some(Icon::Search), &match_label, matching.is_some())
            .clicked()
            && matching.is_none()
        {
            app.actions.push(Action::MatchSpotify);
        }
        if theme::soft_button(ui, &palette, Some(Icon::ExternalLink), "Open songs folder", false)
            .clicked()
        {
            let folder = crate::local_library::songs_dir(&index);
            std::fs::create_dir_all(&folder).ok();
            if let Err(error) = crate::opener::open(&folder) {
                app.toast_error(format!("Could not open the folder: {error}"));
            }
        }
    });
    // The folders being read, each with a way to stop reading it.
    if !app.settings.music_folders.is_empty() {
        ui.add_space(6.0);
        let folders = app.settings.music_folders.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            for (at, folder) in folders.iter().enumerate() {
                let (_, remove) = theme::soft_button_dismiss(ui, &palette, Icon::Music, folder);
                if remove {
                    app.actions.push(Action::RemoveMusicFolder(at));
                }
            }
        });
    }
    if scan.running {
        ui.add_space(6.0);
        theme::subtle(
            ui,
            &palette,
            &format!("Measuring {} of {}: {}", scan.done, scan.total, scan.current),
        );
    } else if !app.local_ui.message.is_empty() {
        ui.add_space(6.0);
        theme::subtle(ui, &palette, &app.local_ui.message.clone());
    }
    ui.add_space(10.0);

    // Which table: all local songs, or one imported playlist.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let total = app.local_songs.songs.len();
        if theme::soft_button(
            ui,
            &palette,
            None,
            &format!("On this computer ({total})"),
            app.local_ui.open_playlist.is_none(),
        )
        .clicked()
        {
            app.local_ui.open_playlist = None;
        }
        for (i, playlist) in app.offline_playlists.iter().enumerate() {
            if theme::soft_button(
                ui,
                &palette,
                None,
                &format!("{} ({})", playlist.name, playlist.tracks.len()),
                app.local_ui.open_playlist == Some(i),
            )
            .clicked()
            {
                app.local_ui.open_playlist = Some(i);
            }
        }
    });
    ui.add_space(8.0);
    ui.add(
        egui::TextEdit::singleline(&mut app.local_ui.filter)
            .hint_text("Search these songs")
            .desired_width(280.0),
    );
    ui.add_space(8.0);

    // Build the lines of the open table.
    let needle = app.local_ui.filter.trim().to_lowercase();
    let mut lines: Vec<Line> = Vec::new();
    match app.local_ui.open_playlist.and_then(|i| app.offline_playlists.get(i)) {
        None => {
            for (number, song) in app.local_songs.songs.iter().enumerate() {
                lines.push(Line {
                    cells: [
                        format!("{}", number + 1),
                        if let Some(error) = &song.error {
                            format!("{} - {error}", song.title)
                        } else {
                            song.title.clone()
                        },
                        song.artist.clone(),
                        song.album.clone(),
                        day_text(song.modified),
                        song.bpm.map_or(String::new(), |b| format!("{}", b.round())),
                        song.camelot.clone().or_else(|| song.key.clone()).unwrap_or_default(),
                        length(song.duration_ms),
                        song.genre.clone(),
                    ],
                    bpm: song.bpm,
                    length_ms: song.duration_ms,
                    have_file: None,
                    path: Some(song.path.clone()),
                    spotify: app.local_matches.uri_of(&song.path).map(str::to_string),
                });
            }
        }
        Some(playlist) => {
            for (number, track) in playlist.tracks.iter().enumerate() {
                let found = app.local_songs.find_match(&track.name, &track.artists);
                let have = found.is_some();
                let path = found.map(|song| song.path.clone());
                lines.push(Line {
                    cells: [
                        format!("{}", number + 1),
                        track.name.clone(),
                        track.artists.clone(),
                        track.album.clone(),
                        String::new(),
                        track.tempo.map_or(String::new(), |t| format!("{}", t.round())),
                        String::new(),
                        length(track.duration_ms),
                        String::new(),
                    ],
                    bpm: track.tempo,
                    length_ms: track.duration_ms,
                    have_file: Some(have),
                    spotify: None,
                    path,
                });
            }
        }
    }
    if !needle.is_empty() {
        lines.retain(|line| line.cells.iter().any(|c| c.to_lowercase().contains(&needle)));
    }
    let sort = (app.local_ui.sort as usize).min(COLUMNS.len() - 1);
    let descending = app.local_ui.descending;
    lines.sort_by(|a, b| {
        let order = match sort {
            0 => a
                .cells[0]
                .parse::<u32>()
                .unwrap_or(0)
                .cmp(&b.cells[0].parse::<u32>().unwrap_or(0)),
            5 => a
                .bpm
                .unwrap_or(f32::MAX)
                .total_cmp(&b.bpm.unwrap_or(f32::MAX)),
            7 => a.length_ms.cmp(&b.length_ms),
            _ => a.cells[sort].to_lowercase().cmp(&b.cells[sort].to_lowercase()),
        };
        if descending { order.reverse() } else { order }
    });

    if lines.is_empty() {
        let (title, body) = if app.local_ui.open_playlist.is_some() {
            ("Nothing here", "This playlist has no songs, or none match your search.")
        } else {
            (
                "No local songs yet",
                "Copy song files into the songs folder, then press Scan songs folder.",
            )
        };
        super::widgets::empty_state(ui, &palette, Icon::Music, title, body);
        return;
    }

    // Play the whole list, in order or shuffled, or stop what is playing.
    let playable: Vec<String> = lines.iter().filter_map(|line| line.path.clone()).collect();
    let playing_path = app
        .deck_current_path()
        .map(|path| path.to_string_lossy().to_string());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if !playable.is_empty() {
            if theme::soft_button(ui, &palette, Some(Icon::Play), "Play all", false).clicked() {
                app.actions.push(Action::PlayLocalFiles {
                    paths: playable.clone(),
                    index: 0,
                    shuffle: Some(false),
                });
            }
            if theme::soft_button(ui, &palette, Some(Icon::Shuffle), "Shuffle", false).clicked() {
                let index = rand::random_range(0..playable.len());
                app.actions.push(Action::PlayLocalFiles {
                    paths: playable.clone(),
                    index,
                    shuffle: Some(true),
                });
            }
        }
        if playing_path.is_some()
            && theme::soft_button(ui, &palette, Some(Icon::Square), "Stop", false).clicked()
        {
            app.actions.push(Action::StopLocalFiles);
        }
    });
    ui.add_space(6.0);

    // Header: click to sort, drag a heading to move the column, drag the
    // line between two headings to resize, right-click for the columns.
    let cols = shown_columns(app);
    let width = ui.available_width();
    let inner = width - 16.0;
    let total: f32 = cols.iter().map(|(_, share)| share).sum::<f32>().max(0.01);
    let widths: Vec<f32> = cols.iter().map(|(_, share)| inner * share / total).collect();
    let (header, header_response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let mut x = header.left() + 8.0;
    let mut clicked_sort: Option<u8> = None;
    let mut new_cols: Option<Vec<(u8, f32)>> = None;
    let drag_id = ui.id().with("local-head-drag");
    let dragging: Option<usize> = ui.data(|data| data.get_temp(drag_id));
    // Right-clicking a heading opens the header's menu too.
    let mut menu_anchor = header_response.clone();
    for (at, (id, _)) in cols.iter().enumerate() {
        let w = widths[at];
        let cell = Rect::from_min_size(pos2(x, header.top()), vec2(w, header.height()));
        let response = ui.interact(cell, ui.id().with(("local-head", *id)), Sense::click_and_drag());
        let active = sort == *id as usize;
        let color = if active {
            palette.accent
        } else if response.hovered() {
            palette.text
        } else {
            palette.secondary
        };
        let arrow = if active {
            if descending { " ↓" } else { " ↑" }
        } else {
            ""
        };
        ui.painter().with_clip_rect(cell.shrink2(vec2(2.0, 0.0))).text(
            pos2(cell.left(), cell.center().y),
            Align2::LEFT_CENTER,
            format!("{}{arrow}", COLUMNS[*id as usize].0),
            theme::regular(theme::SMALL_TEXT),
            color,
        );
        if response.clicked() {
            clicked_sort = Some(*id);
        }
        menu_anchor = menu_anchor.union(response.clone());
        if response.drag_started() {
            ui.data_mut(|data| data.insert_temp(drag_id, at));
        }
        if response.drag_stopped()
            && let Some(from) = dragging
            && let Some(pointer) = ui.ctx().pointer_interact_pos()
        {
            // Drop where the pointer is: before the first column whose middle
            // is past it.
            let mut left = header.left() + 8.0;
            let mut to = cols.len();
            for (i, w) in widths.iter().enumerate() {
                if pointer.x < left + w / 2.0 {
                    to = i;
                    break;
                }
                left += w;
            }
            let mut moved = cols.clone();
            let column = moved.remove(from);
            let to = if to > from { to - 1 } else { to };
            moved.insert(to.min(moved.len()), column);
            new_cols = Some(moved);
            ui.data_mut(|data| data.remove::<usize>(drag_id));
        }
        // The resize handle on the right edge of every heading but the last.
        if at + 1 < cols.len() {
            let handle = Rect::from_center_size(pos2(cell.right(), cell.center().y), vec2(8.0, header.height()));
            let grip = ui
                .interact(handle, ui.id().with(("local-head-size", *id)), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeColumn);
            if grip.hovered() || grip.dragged() {
                ui.painter().vline(cell.right(), header.y_range().shrink(6.0), Stroke::new(1.0, palette.outline));
            }
            if grip.dragged() {
                let delta = grip.drag_delta().x / inner * total;
                let mut sized = cols.clone();
                let room = sized[at].1 + sized[at + 1].1;
                let left = (sized[at].1 + delta).clamp(0.02, room - 0.02);
                sized[at].1 = left;
                sized[at + 1].1 = room - left;
                new_cols = Some(sized);
            }
        }
        x += w;
    }
    // While a heading is dragged, a line shows where it will land.
    if dragging.is_some()
        && let Some(pointer) = ui.ctx().pointer_hover_pos()
        && header.expand(20.0).contains(pointer)
    {
        let mut left = header.left() + 8.0;
        let mut mark = left + inner;
        for w in &widths {
            if pointer.x < left + w / 2.0 {
                mark = left;
                break;
            }
            left += w;
        }
        ui.painter().vline(mark, header.y_range(), Stroke::new(2.0, palette.accent));
    }
    ui.painter()
        .hline(header.x_range().shrink(8.0), header.bottom() - 0.5, Stroke::new(1.0, palette.outline));
    // Right-click the header: which columns show, and the sort.
    egui::Popup::context_menu(&menu_anchor)
        .frame(super::widgets::menu_frame(&palette))
        .show(|ui| {
            super::widgets::fit_menu(ui, 260.0);
            super::widgets::menu_group(ui, &palette, "Columns");
            for (id, (name, share)) in COLUMNS.iter().enumerate() {
                let id = id as u8;
                let on = cols.iter().any(|(shown, _)| *shown == id);
                // The title always shows.
                let enabled = id != 1;
                if super::widgets::menu_item_enabled(
                    ui,
                    &palette,
                    on.then_some(Icon::Check),
                    &title_case(name),
                    enabled,
                ) {
                    let mut changed = cols.clone();
                    if on {
                        changed.retain(|(shown, _)| *shown != id);
                    } else {
                        changed.push((id, *share));
                    }
                    new_cols = Some(changed);
                }
            }
            super::widgets::menu_separator(ui, &palette);
            super::widgets::menu_group(ui, &palette, "Sort by");
            for (id, _) in &cols {
                let active = sort == *id as usize;
                if super::widgets::menu_item(
                    ui,
                    &palette,
                    active.then_some(if descending { Icon::ArrowDown } else { Icon::ArrowUp }),
                    &title_case(COLUMNS[*id as usize].0),
                ) {
                    clicked_sort = Some(*id);
                }
            }
            super::widgets::menu_separator(ui, &palette);
            if super::widgets::menu_item(ui, &palette, Some(Icon::Refresh), "Reset columns") {
                new_cols = Some(crate::settings::Settings::default().local_columns);
            }
        });
    if let Some(changed) = new_cols {
        app.settings.local_columns = changed;
        app.mark_settings_dirty();
    }
    if let Some(column) = clicked_sort {
        if app.local_ui.sort == column {
            app.local_ui.descending = !app.local_ui.descending;
        } else {
            app.local_ui.sort = column;
            app.local_ui.descending = false;
        }
    }

    // Rows.
    let mut pending: Vec<Action> = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("local-songs-scroll")
        .auto_shrink([false, false])
        .show_rows(ui, ROW, lines.len(), |ui, range| {
            for index in range {
                let line = &lines[index];
                let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                let is_playing = line.path.is_some() && line.path == playing_path;
                let pointer_in_row = ui.rect_contains_pointer(rect);
                if is_playing {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(6), palette.accent.gamma_multiply(0.18));
                } else if pointer_in_row {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::same(6), palette.surface_hover);
                }
                let painter = ui.painter().with_clip_rect(rect);
                // A small green dot: this file is also on Spotify.
                if line.spotify.is_some() {
                    let centre = pos2(rect.left() + 4.0, rect.center().y);
                    painter.circle_filled(centre, 2.5, palette.accent);
                }
                let mut x = rect.left() + 8.0;
                for (at, (id, _)) in cols.iter().enumerate() {
                    let w = widths[at];
                    let id = *id as usize;
                    let clip = Rect::from_min_size(pos2(x, rect.top()), vec2((w - 8.0).max(1.0), ROW));
                    let mut text = line.cells[id].clone();
                    if id == 1 {
                        match line.have_file {
                            Some(true) => text = format!("✓ {text}"),
                            Some(false) => text = format!("  {text}"),
                            None => {}
                        }
                    }
                    let color = if id == 1 && is_playing {
                        palette.accent
                    } else if id == 1 {
                        palette.text
                    } else {
                        palette.secondary
                    };
                    painter.with_clip_rect(clip).text(
                        pos2(clip.left(), rect.center().y),
                        Align2::LEFT_CENTER,
                        text,
                        theme::regular(theme::BODY_TEXT),
                        color,
                    );
                    x += w;
                }
                // The song as Spotify would search for it.
                let search = format!("{} {}", line.cells[1], line.cells[2]).trim().to_string();
                let play_from_here = |pending: &mut Vec<Action>| {
                    if let Some(at) = playable
                        .iter()
                        .position(|path| Some(path) == line.path.as_ref())
                    {
                        pending.push(Action::PlayLocalFiles {
                            paths: playable.clone(),
                            index: at,
                            shuffle: None,
                        });
                    }
                };
                // Two small buttons over the end of the row: play it, or
                // look it up on Spotify.
                let mut used_button = false;
                if pointer_in_row {
                    let edge = 24.0;
                    let play_rect = Rect::from_min_size(
                        pos2(rect.right() - edge * 2.0 - 10.0, rect.center().y - edge / 2.0),
                        vec2(edge, edge),
                    );
                    let search_rect = Rect::from_min_size(
                        pos2(rect.right() - edge - 4.0, rect.center().y - edge / 2.0),
                        vec2(edge, edge),
                    );
                    ui.painter().rect_filled(
                        Rect::from_min_max(
                            pos2(play_rect.left() - 6.0, rect.top() + 2.0),
                            pos2(rect.right() - 2.0, rect.bottom() - 2.0),
                        ),
                        CornerRadius::same(6),
                        if is_playing {
                            palette.surface_active
                        } else {
                            palette.surface_hover
                        },
                    );
                    if line.path.is_some() {
                        let button = ui
                            .interact(play_rect, ui.id().with(("local-play", index)), Sense::click())
                            .on_hover_text("Play");
                        let tint = if button.hovered() { palette.text } else { palette.secondary };
                        theme::paint_icon(ui, Icon::Play, play_rect, 14.0, tint);
                        if button.clicked() {
                            used_button = true;
                            play_from_here(&mut pending);
                        }
                    }
                    let button = ui
                        .interact(search_rect, ui.id().with(("local-find", index)), Sense::click())
                        .on_hover_text("Search this song on Spotify");
                    let tint = if button.hovered() { palette.text } else { palette.secondary };
                    theme::paint_icon(ui, Icon::Search, search_rect, 14.0, tint);
                    if button.clicked() && !search.is_empty() {
                        used_button = true;
                        pending.push(Action::Search(search.clone()));
                    }
                }
                if response.double_clicked() && !used_button {
                    play_from_here(&mut pending);
                }
                egui::Popup::context_menu(&response)
                    .frame(super::widgets::menu_frame(&palette))
                    .show(|ui| {
                        super::widgets::fit_menu(ui, 280.0);
                        if line.path.is_some()
                            && super::widgets::menu_item(ui, &palette, Some(Icon::Play), "Play from here")
                        {
                            play_from_here(&mut pending);
                        }
                        if let Some(path) = &line.path
                            && super::widgets::menu_item(ui, &palette, Some(Icon::ExternalLink), "Show file in folder")
                            && let Err(error) = crate::opener::reveal(std::path::Path::new(path))
                        {
                            log::warn!("could not show the file in its folder: {error}");
                        }
                        if super::widgets::menu_item(ui, &palette, Some(Icon::Search), "Search this song on Spotify")
                            && !search.is_empty()
                        {
                            pending.push(Action::Search(search.clone()));
                        }
                        if let Some(uri) = &line.spotify
                            && super::widgets::menu_item(ui, &palette, Some(Icon::Disc), "Open it on Spotify")
                        {
                            pending.push(Action::OpenLink(uri.clone()));
                        }
                    });
            }
        });
    app.actions.extend(pending);
}

/// "TITLE" as "Title", for the menus.
fn title_case(heading: &str) -> String {
    let mut chars = heading.chars();
    match chars.next() {
        Some(first) => first.to_string() + &chars.as_str().to_lowercase(),
        None => String::new(),
    }
}
