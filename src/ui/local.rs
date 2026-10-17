//! The Local songs page: audio files in the index's songs folder, with the
//! tempo, key and waveform measured from the sound, playlists of those files
//! made here, and playlists imported from Exportify files that can be
//! browsed with no internet.

use std::collections::HashMap;

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::App;
use crate::local_library::LocalSong;
use crate::model::Action;
use crate::theme::{self, Icon, Palette};

const ROW: f32 = 34.0;
/// The heading row, as tall as the playlist table's.
const HEADER: f32 = 34.0;
/// The playlists column, by id (see [`COLUMNS`]).
const PLAYLISTS_COLUMN: usize = 9;

/// One line of whichever table is open, already turned into text, one
/// cell per column id (see [`COLUMNS`]).
struct Line {
    cells: [String; 10],
    /// For the sort: the number behind the BPM column, if any.
    bpm: Option<f32>,
    length_ms: u64,
    /// A tick or a note beside the title (an imported song with a file).
    have_file: Option<bool>,
    /// The file to play for this line, if there is one.
    path: Option<String>,
    /// The Spotify song this file is, once it has been matched.
    spotify: Option<String>,
    /// The file as a local playlist knows it: the path, kept even when the
    /// file is no longer in the library, so it can still be taken out.
    entry: Option<String>,
    /// The local playlists this file is in, by place.
    playlists: Vec<usize>,
}

/// Every column the table can show, by id: its heading, its name in the
/// columns menu, and its share of the width when first shown.
const COLUMNS: [(&str, &str, f32); 10] = [
    ("#", "number", 0.05),
    ("title", "title", 0.30),
    ("artist", "artist", 0.20),
    ("album", "album", 0.18),
    ("date added", "date added", 0.09),
    ("bpm", "bpm", 0.06),
    ("key", "key", 0.06),
    ("length", "length", 0.07),
    ("genre", "genre", 0.10),
    ("playlists", "playlists", 0.08),
];

/// Which table the page shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Open {
    /// Every song file.
    #[default]
    All,
    /// The list of playlists, local and imported.
    Playlists,
    /// One local playlist, by place in `settings.local_playlists`.
    Local(usize),
    /// One imported Exportify playlist, by place in `app.offline_playlists`.
    Imported(usize),
}

/// What the page remembers between frames that is only about the view.
#[derive(Clone, Debug, Default)]
struct View {
    open: Open,
    /// The open local playlist's song picker is showing.
    adding: bool,
    add_filter: String,
    /// The name typed for a new playlist.
    new_name: String,
    /// The local playlist being renamed, and the name typed so far.
    renaming: Option<(usize, String)>,
    /// Put the cursor in the rename field on the next frame.
    focus_rename: bool,
}

fn view_id() -> egui::Id {
    egui::Id::new("local-songs-view")
}

fn keep_view(ui: &egui::Ui, view: &View) {
    let view = view.clone();
    ui.ctx().data_mut(|data| data.insert_temp(view_id(), view));
}

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

/// "1 song", "12 songs".
fn song_count(count: usize) -> String {
    if count == 1 { "1 song".to_string() } else { format!("{count} songs") }
}

/// The names of the local playlists at these places, for the playlists
/// column's text (searched and sorted by).
fn playlist_names(places: &[usize], names: &[String]) -> String {
    places
        .iter()
        .filter_map(|at| names.get(*at).cloned())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A line for one song file.
fn song_line(
    number: usize,
    song: &LocalSong,
    spotify: Option<String>,
    playlists: Vec<usize>,
    names: &[String],
) -> Line {
    Line {
        cells: [
            format!("{number}"),
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
            playlist_names(&playlists, names),
        ],
        bpm: song.bpm,
        length_ms: song.duration_ms,
        have_file: None,
        path: Some(song.path.clone()),
        spotify,
        entry: Some(song.path.clone()),
        playlists,
    }
}

/// A playlist's picture: a music note on a tile (song files carry no
/// picture chanceify reads yet).
fn paint_playlist_tile(ui: &egui::Ui, palette: &Palette, rect: Rect) {
    super::widgets::paint_cover(ui, palette, None, rect, 4.0, Icon::Music, None);
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
    let mut view: View = ui
        .ctx()
        .data(|data| data.get_temp::<View>(view_id()))
        .unwrap_or_default();
    // A playlist that is gone (deleted, or the imports were read again)
    // leaves the list of playlists showing.
    match view.open {
        Open::Local(at) if at >= app.settings.local_playlists.len() => {
            view.open = Open::Playlists;
        }
        Open::Imported(at) if at >= app.offline_playlists.len() => view.open = Open::Playlists,
        _ => {}
    }
    if !matches!(view.open, Open::Local(_)) {
        view.adding = false;
    }
    if view
        .renaming
        .as_ref()
        .is_some_and(|(at, _)| view.open != Open::Local(*at))
    {
        view.renaming = None;
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

    // Which table: every song, or the playlists (made here or imported).
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let total = app.local_songs.songs.len();
        if theme::soft_button(
            ui,
            &palette,
            Some(Icon::Music),
            &format!("All songs ({total})"),
            view.open == Open::All,
        )
        .clicked()
        {
            view.open = Open::All;
        }
        let playlists = app.settings.local_playlists.len() + app.offline_playlists.len();
        if theme::soft_button(
            ui,
            &palette,
            Some(Icon::ListMusic),
            &format!("Playlists ({playlists})"),
            view.open != Open::All,
        )
        .clicked()
        {
            view.open = Open::Playlists;
        }
    });
    ui.add_space(8.0);
    app.local_ui.open_playlist = match view.open {
        Open::Imported(at) => Some(at),
        _ => None,
    };

    if view.open == Open::Playlists {
        playlists_page(app, ui, &palette, &mut view);
        keep_view(ui, &view);
        return;
    }

    // The open playlist's own row: its name, and what can be done with it.
    let open_local = match view.open {
        Open::Local(at) => Some(at),
        _ => None,
    };
    match view.open {
        Open::Local(at) => playlist_header(app, ui, &palette, &mut view, at),
        Open::Imported(at) => {
            let (name, count) = app
                .offline_playlists
                .get(at)
                .map(|playlist| (playlist.name.clone(), playlist.tracks.len()))
                .unwrap_or_default();
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                if theme::soft_button(ui, &palette, Some(Icon::ChevronLeft), "Playlists", false)
                    .clicked()
                {
                    view.open = Open::Playlists;
                }
                theme::text(ui, name, theme::bold(theme::SECTION_TEXT), palette.text);
                theme::subtle(ui, &palette, &format!("{} · imported", song_count(count)));
            });
            ui.add_space(8.0);
        }
        _ => {}
    }
    // Back to the list (or the playlist was deleted): show it next frame.
    if view.open == Open::Playlists {
        keep_view(ui, &view);
        ui.ctx().request_repaint();
        return;
    }

    ui.add(
        egui::TextEdit::singleline(&mut app.local_ui.filter)
            .hint_text("Search these songs")
            .desired_width(280.0),
    );
    ui.add_space(8.0);

    // Build the lines of the open table.
    let local_names: Vec<String> = app
        .settings
        .local_playlists
        .iter()
        .map(|playlist| playlist.name.clone())
        .collect();
    let needle = app.local_ui.filter.trim().to_lowercase();
    let mut lines: Vec<Line> = Vec::new();
    {
        // Which local playlists each file is in.
        let mut membership: HashMap<&str, Vec<usize>> = HashMap::new();
        for (at, playlist) in app.settings.local_playlists.iter().enumerate() {
            for path in &playlist.paths {
                let places = membership.entry(path.as_str()).or_default();
                if !places.contains(&at) {
                    places.push(at);
                }
            }
        }
        let member_of = |path: &str| membership.get(path).cloned().unwrap_or_default();
        let spotify_of = |path: &str| app.local_matches.uri_of(path).map(str::to_string);
        match view.open {
            Open::Local(at) => {
                let by_path: HashMap<&str, &LocalSong> = app
                    .local_songs
                    .songs
                    .iter()
                    .map(|song| (song.path.as_str(), song))
                    .collect();
                let paths = app
                    .settings
                    .local_playlists
                    .get(at)
                    .map(|playlist| playlist.paths.as_slice())
                    .unwrap_or_default();
                for (number, path) in paths.iter().enumerate() {
                    let places = member_of(path.as_str());
                    match by_path.get(path.as_str()) {
                        Some(song) => lines.push(song_line(
                            number + 1,
                            song,
                            spotify_of(path.as_str()),
                            places,
                            &local_names,
                        )),
                        None => {
                            // The file has moved or gone: still listed, so it
                            // can be taken out, but there is nothing to play.
                            let stem = std::path::Path::new(path)
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_else(|| path.clone());
                            let names = playlist_names(&places, &local_names);
                            lines.push(Line {
                                cells: [
                                    format!("{}", number + 1),
                                    format!("{stem} - file not found"),
                                    String::new(),
                                    String::new(),
                                    String::new(),
                                    String::new(),
                                    String::new(),
                                    String::new(),
                                    String::new(),
                                    names,
                                ],
                                bpm: None,
                                length_ms: 0,
                                have_file: None,
                                path: None,
                                spotify: None,
                                entry: Some(path.clone()),
                                playlists: places,
                            });
                        }
                    }
                }
            }
            Open::Imported(at) => {
                if let Some(playlist) = app.offline_playlists.get(at) {
                    for (number, track) in playlist.tracks.iter().enumerate() {
                        let found = app.local_songs.find_match(&track.name, &track.artists);
                        let have = found.is_some();
                        let path = found.map(|song| song.path.clone());
                        let places = path.as_deref().map(&member_of).unwrap_or_default();
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
                                playlist_names(&places, &local_names),
                            ],
                            bpm: track.tempo,
                            length_ms: track.duration_ms,
                            have_file: Some(have),
                            spotify: None,
                            entry: path.clone(),
                            path,
                            playlists: places,
                        });
                    }
                }
            }
            _ => {
                for (number, song) in app.local_songs.songs.iter().enumerate() {
                    lines.push(song_line(
                        number + 1,
                        song,
                        spotify_of(song.path.as_str()),
                        member_of(song.path.as_str()),
                        &local_names,
                    ));
                }
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
        let (title, body) = if !needle.is_empty() {
            ("Nothing here", "No songs match your search.")
        } else {
            match view.open {
                Open::Local(_) => (
                    "This playlist is empty",
                    "Press Add songs to pick songs from your files, or right-click a song in All songs.",
                ),
                Open::Imported(_) => ("Nothing here", "This playlist has no songs."),
                _ => (
                    "No local songs yet",
                    "Copy song files into the songs folder, then press Scan songs folder.",
                ),
            }
        };
        super::widgets::empty_state(ui, &palette, Icon::Music, title, body);
        keep_view(ui, &view);
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
    let (header, header_response) = ui.allocate_exact_size(vec2(width, HEADER), Sense::click());
    let mut x = header.left() + 8.0;
    let mut clicked_sort: Option<u8> = None;
    let mut new_cols: Option<Vec<(u8, f32)>> = None;
    let drag_id = ui.id().with("local-head-drag");
    let dragging: Option<usize> = ui.data(|data| data.get_temp(drag_id));
    // Right-clicking a heading opens the header's menu too.
    let mut menu_anchor = header_response.clone();
    let font = theme::regular(theme::SMALL_TEXT);
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
        if response.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        }
        // Lowercase and small, centred in the row, as the playlist table's
        // headings are; the sorting one is lit with a small drawn arrow.
        let clip = Rect::from_min_max(
            pos2(cell.left() - 1.0, header.top()),
            pos2((cell.right() - 6.0).max(cell.left()), header.bottom()),
        );
        let painter = ui.painter().with_clip_rect(clip);
        let galley = painter.layout_no_wrap(COLUMNS[*id as usize].0.to_string(), font.clone(), color);
        let size = galley.size();
        let top_left = pos2(cell.left(), header.center().y - size.y / 2.0);
        painter.galley(top_left, galley, color);
        if active {
            let ascending = !descending;
            let center = pos2(top_left.x + size.x + 8.0, header.center().y);
            let (wing, tip) = if ascending { (2.8, -3.2) } else { (-2.8, 3.2) };
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + vec2(-4.0, wing),
                    center + vec2(4.0, wing),
                    center + vec2(0.0, tip),
                ],
                color,
                Stroke::NONE,
            ));
        }
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
            menu_anchor = menu_anchor.union(grip);
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
        ui.painter().vline(mark, header.y_range().shrink(3.0), Stroke::new(2.0, palette.accent));
    }
    ui.painter()
        .hline(header.x_range().shrink(8.0), header.bottom() - 0.5, Stroke::new(1.0, palette.outline));
    // Right-click the header: which columns show, as in the playlist
    // table's columns menu. Sorting is done by clicking a heading.
    egui::Popup::context_menu(&menu_anchor)
        .frame(super::widgets::menu_frame(&palette))
        .show(|ui| {
            ui.set_width(220.0);
            ui.spacing_mut().item_spacing.y = 1.0;
            for (id, (_, label, share)) in COLUMNS.iter().enumerate() {
                let id = id as u8;
                // The title always shows.
                if id == 1 {
                    super::widgets::checkbox_row_ex(ui, &palette, label, true, false, "");
                    continue;
                }
                let on = cols.iter().any(|(shown, _)| *shown == id);
                if super::widgets::checkbox_row(ui, &palette, label, on) {
                    let mut changed = new_cols.clone().unwrap_or_else(|| cols.clone());
                    if on {
                        changed.retain(|(shown, _)| *shown != id);
                    } else {
                        changed.push((id, *share));
                    }
                    new_cols = Some(changed);
                }
            }
            super::widgets::menu_separator(ui, &palette);
            if super::widgets::menu_item(ui, &palette, None, "reset column widths") {
                // The same columns in the same order, each back to its
                // first width.
                new_cols = Some(
                    cols.iter()
                        .map(|(id, _)| (*id, COLUMNS[*id as usize].2))
                        .collect(),
                );
            }
        });
    ui.add_space(6.0);
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
    let mut open_playlist: Option<usize> = None;
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
                    if id == PLAYLISTS_COLUMN {
                        // Small playlist tiles, as the playlist table's
                        // playlists column shows; click one to open it.
                        let size = 18.0;
                        let gap = 3.0;
                        let mut left = clip.left();
                        for place in &line.playlists {
                            if left + size > clip.right() {
                                break;
                            }
                            let tile = Rect::from_min_size(
                                pos2(left, rect.center().y - size / 2.0),
                                Vec2::splat(size),
                            );
                            let name = local_names.get(*place).cloned().unwrap_or_default();
                            let initial = name
                                .chars()
                                .find(|c| c.is_alphanumeric())
                                .or_else(|| name.chars().next())
                                .map_or(' ', |c| c.to_uppercase().next().unwrap_or(c));
                            let hit = ui
                                .interact(tile, ui.id().with(("local-member", index, *place)), Sense::click())
                                .on_hover_text(&name);
                            let tile_painter = painter.with_clip_rect(clip);
                            tile_painter.rect_filled(
                                tile,
                                CornerRadius::same(4),
                                if hit.hovered() { palette.surface_active } else { palette.surface_hover },
                            );
                            tile_painter.text(
                                tile.center(),
                                Align2::CENTER_CENTER,
                                initial.to_string(),
                                theme::semibold(size * 0.5),
                                palette.secondary,
                            );
                            if hit.clicked() {
                                open_playlist = Some(*place);
                            }
                            left += size + gap;
                        }
                        x += w;
                        continue;
                    }
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
                        if let Some(file) = &line.entry {
                            super::widgets::menu_separator(ui, &palette);
                            // Songs with a file can go into any local
                            // playlist, or start a new one.
                            if line.path.is_some() {
                                super::widgets::menu_submenu(
                                    ui,
                                    &palette,
                                    Some(Icon::ListPlus),
                                    "Add to playlist",
                                    |ui| {
                                        super::widgets::fit_menu(ui, 260.0);
                                        let mut listed = false;
                                        for (place, name) in local_names.iter().enumerate() {
                                            if Some(place) == open_local {
                                                continue;
                                            }
                                            listed = true;
                                            let already = line.playlists.contains(&place);
                                            if super::widgets::menu_item_enabled(
                                                ui,
                                                &palette,
                                                Some(if already { Icon::Check } else { Icon::ListMusic }),
                                                name,
                                                !already,
                                            ) {
                                                pending.push(Action::AddToLocalPlaylist {
                                                    playlist: place,
                                                    paths: vec![file.clone()],
                                                });
                                            }
                                        }
                                        if listed {
                                            super::widgets::menu_separator(ui, &palette);
                                        }
                                        if super::widgets::menu_item(ui, &palette, Some(Icon::Plus), "New playlist") {
                                            pending.push(Action::CreateLocalPlaylist {
                                                name: format!("Playlist {}", local_names.len() + 1),
                                                paths: vec![file.clone()],
                                            });
                                        }
                                    },
                                );
                            }
                            if let Some(place) = open_local
                                && super::widgets::menu_item(
                                    ui,
                                    &palette,
                                    Some(Icon::Minus),
                                    "Remove from this playlist",
                                )
                            {
                                pending.push(Action::RemoveFromLocalPlaylist {
                                    playlist: place,
                                    paths: vec![file.clone()],
                                });
                            }
                        }
                    });
            }
        });
    app.actions.extend(pending);
    if let Some(place) = open_playlist {
        view.open = Open::Local(place);
    }
    keep_view(ui, &view);
}

/// The list of playlists: a field to make a new one, the ones made here,
/// then the ones imported from Exportify.
fn playlists_page(app: &mut App, ui: &mut egui::Ui, palette: &Palette, view: &mut View) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut view.new_name)
                .hint_text("New playlist name")
                .desired_width(240.0)
                .min_size(vec2(0.0, theme::CONTROL_HEIGHT)),
        );
        let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        let create = theme::soft_button(ui, palette, Some(Icon::Plus), "Create playlist", false).clicked();
        let name = view.new_name.trim().to_string();
        if (create || entered) && !name.is_empty() {
            // Open the new playlist with its song picker, ready to fill.
            view.open = Open::Local(app.settings.local_playlists.len());
            view.adding = true;
            view.add_filter.clear();
            view.new_name.clear();
            app.actions.push(Action::CreateLocalPlaylist { name, paths: Vec::new() });
        }
    });
    ui.add_space(10.0);
    if app.settings.local_playlists.is_empty() && app.offline_playlists.is_empty() {
        super::widgets::empty_state(
            ui,
            palette,
            Icon::ListMusic,
            "No playlists yet",
            "Name a playlist above and press Create playlist, then add songs to it.",
        );
        return;
    }
    let library: std::collections::HashSet<&str> =
        app.local_songs.songs.iter().map(|song| song.path.as_str()).collect();
    // (name, number of songs, files that can be played)
    let local: Vec<(String, usize, Vec<String>)> = app
        .settings
        .local_playlists
        .iter()
        .map(|playlist| {
            let playable = playlist
                .paths
                .iter()
                .filter(|path| library.contains(path.as_str()))
                .cloned()
                .collect();
            (playlist.name.clone(), playlist.paths.len(), playable)
        })
        .collect();
    let imported: Vec<(String, usize)> = app
        .offline_playlists
        .iter()
        .map(|playlist| (playlist.name.clone(), playlist.tracks.len()))
        .collect();
    let mut pending: Vec<Action> = Vec::new();
    egui::ScrollArea::vertical()
        .id_salt("local-playlists-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if !local.is_empty() {
                super::widgets::menu_group(ui, palette, "Your playlists");
                for (at, (name, count, playable)) in local.iter().enumerate() {
                    let response = playlist_row(ui, palette, name, &song_count(*count));
                    if response.clicked() {
                        view.open = Open::Local(at);
                    }
                    egui::Popup::context_menu(&response)
                        .frame(super::widgets::menu_frame(palette))
                        .show(|ui| {
                            super::widgets::fit_menu(ui, 260.0);
                            if super::widgets::menu_item(ui, palette, Some(Icon::ListMusic), "Open") {
                                view.open = Open::Local(at);
                            }
                            if super::widgets::menu_item_enabled(
                                ui,
                                palette,
                                Some(Icon::Play),
                                "Play",
                                !playable.is_empty(),
                            ) {
                                pending.push(Action::PlayLocalFiles {
                                    paths: playable.clone(),
                                    index: 0,
                                    shuffle: Some(false),
                                });
                            }
                            if super::widgets::menu_item(ui, palette, Some(Icon::Pencil), "Rename") {
                                view.open = Open::Local(at);
                                view.renaming = Some((at, name.clone()));
                                view.focus_rename = true;
                            }
                            super::widgets::menu_separator(ui, palette);
                            if super::widgets::menu_item(ui, palette, Some(Icon::Trash), "Delete playlist") {
                                pending.push(Action::DeleteLocalPlaylist(at));
                            }
                        });
                }
                ui.add_space(10.0);
            }
            if !imported.is_empty() {
                super::widgets::menu_group(ui, palette, "Imported from Exportify");
                for (at, (name, count)) in imported.iter().enumerate() {
                    if playlist_row(ui, palette, name, &song_count(*count)).clicked() {
                        view.open = Open::Imported(at);
                    }
                }
            }
        });
    app.actions.extend(pending);
}

/// One playlist in the list: its tile, its name and how many songs it holds.
fn playlist_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    name: &str,
    detail: &str,
) -> egui::Response {
    let height = 52.0;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(6), palette.surface_hover);
        }
        let tile = Rect::from_min_size(pos2(rect.left() + 6.0, rect.top() + 6.0), Vec2::splat(40.0));
        paint_playlist_tile(ui, palette, tile);
        let text_left = tile.right() + 12.0;
        let painter = ui
            .painter()
            .with_clip_rect(Rect::from_min_max(pos2(text_left, rect.top()), rect.max));
        painter.text(
            pos2(text_left, rect.center().y - 1.0),
            Align2::LEFT_BOTTOM,
            name,
            theme::medium(theme::BODY_TEXT),
            palette.text,
        );
        painter.text(
            pos2(text_left, rect.center().y + 2.0),
            Align2::LEFT_TOP,
            detail,
            theme::regular(theme::SMALL_TEXT),
            palette.secondary,
        );
    }
    response
}

/// The open local playlist's row: back to the list, its tile and name (or
/// the field to rename it), and its controls, then the song picker when it
/// is open.
fn playlist_header(app: &mut App, ui: &mut egui::Ui, palette: &Palette, view: &mut View, at: usize) {
    let Some((name, count)) = app
        .settings
        .local_playlists
        .get(at)
        .map(|playlist| (playlist.name.clone(), playlist.paths.len()))
    else {
        return;
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if theme::soft_button(ui, palette, Some(Icon::ChevronLeft), "Playlists", false).clicked() {
            view.open = Open::Playlists;
            view.renaming = None;
        }
        let (tile, _) = ui.allocate_exact_size(Vec2::splat(theme::CONTROL_HEIGHT), Sense::hover());
        paint_playlist_tile(ui, palette, tile);
        let focus = std::mem::take(&mut view.focus_rename);
        match &mut view.renaming {
            Some((renaming, text)) if *renaming == at => {
                let field = ui.add(
                    egui::TextEdit::singleline(text)
                        .hint_text("Playlist name")
                        .desired_width(240.0)
                        .min_size(vec2(0.0, theme::CONTROL_HEIGHT)),
                );
                if focus {
                    field.request_focus();
                }
                let entered = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                let save = theme::soft_button(ui, palette, Some(Icon::Check), "Save", false).clicked();
                let cancel = theme::soft_button(ui, palette, Some(Icon::X), "Cancel", false).clicked()
                    || ui.input(|input| input.key_pressed(egui::Key::Escape));
                let typed = text.trim().to_string();
                if (save || entered) && !typed.is_empty() {
                    app.actions.push(Action::RenameLocalPlaylist(at, typed));
                    view.renaming = None;
                } else if cancel {
                    view.renaming = None;
                }
            }
            _ => {
                theme::text(ui, name.clone(), theme::bold(theme::SECTION_TEXT), palette.text);
                theme::subtle(ui, palette, &song_count(count));
            }
        }
    });
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if theme::soft_button(ui, palette, Some(Icon::Plus), "Add songs", view.adding).clicked() {
            view.adding = !view.adding;
            view.add_filter.clear();
        }
        if theme::soft_button(ui, palette, Some(Icon::Pencil), "Rename", false).clicked() {
            view.renaming = Some((at, name.clone()));
            view.focus_rename = true;
        }
        if theme::soft_button(ui, palette, Some(Icon::Trash), "Delete playlist", false).clicked() {
            app.actions.push(Action::DeleteLocalPlaylist(at));
            view.open = Open::Playlists;
            view.renaming = None;
        }
    });
    ui.add_space(8.0);
    if view.adding {
        add_songs_panel(app, ui, palette, view, at);
        ui.add_space(8.0);
    }
}

/// The song picker inside a local playlist: every song file not already in
/// it, filtered by what is typed, each added with a click.
fn add_songs_panel(app: &mut App, ui: &mut egui::Ui, palette: &Palette, view: &mut View, at: usize) {
    let Some(playlist) = app.settings.local_playlists.get(at) else {
        return;
    };
    let inside: std::collections::HashSet<&str> =
        playlist.paths.iter().map(String::as_str).collect();
    let needle = view.add_filter.trim().to_lowercase();
    // (path, title, artist)
    let candidates: Vec<(String, String, String)> = app
        .local_songs
        .songs
        .iter()
        .filter(|song| song.error.is_none() && !inside.contains(song.path.as_str()))
        .filter(|song| {
            needle.is_empty()
                || song.title.to_lowercase().contains(&needle)
                || song.artist.to_lowercase().contains(&needle)
                || song.album.to_lowercase().contains(&needle)
        })
        .map(|song| (song.path.clone(), song.title.clone(), song.artist.clone()))
        .collect();
    let mut pending: Vec<Action> = Vec::new();
    egui::Frame::new()
        .fill(palette.surface)
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(theme::RADIUS))
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut view.add_filter)
                        .hint_text("Find songs to add")
                        .desired_width(240.0)
                        .min_size(vec2(0.0, theme::CONTROL_HEIGHT)),
                );
                if !candidates.is_empty()
                    && theme::soft_button(
                        ui,
                        palette,
                        Some(Icon::ListPlus),
                        &format!("Add all {}", candidates.len()),
                        false,
                    )
                    .clicked()
                {
                    pending.push(Action::AddToLocalPlaylist {
                        playlist: at,
                        paths: candidates.iter().map(|(path, _, _)| path.clone()).collect(),
                    });
                }
                if theme::soft_button(ui, palette, Some(Icon::Check), "Done", false).clicked() {
                    view.adding = false;
                }
            });
            ui.add_space(6.0);
            if candidates.is_empty() {
                theme::subtle(
                    ui,
                    palette,
                    if app.local_songs.songs.is_empty() {
                        "No song files yet. Scan the songs folder first."
                    } else if needle.is_empty() {
                        "Every song file is already in this playlist."
                    } else {
                        "No songs match."
                    },
                );
                return;
            }
            let row = 30.0;
            egui::ScrollArea::vertical()
                .id_salt(("local-add-songs", at))
                .max_height(240.0)
                .auto_shrink([false, true])
                .show_rows(ui, row, candidates.len(), |ui, range| {
                    for index in range {
                        let (path, title, artist) = &candidates[index];
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(ui.available_width(), row), Sense::click());
                        let response = response.on_hover_text("Add to this playlist");
                        if response.hovered() {
                            ui.painter().rect_filled(rect, CornerRadius::same(6), palette.surface_hover);
                        }
                        let plus = Rect::from_center_size(
                            pos2(rect.right() - 16.0, rect.center().y),
                            Vec2::splat(20.0),
                        );
                        theme::paint_icon(
                            ui,
                            Icon::Plus,
                            plus,
                            14.0,
                            if response.hovered() { palette.accent } else { palette.secondary },
                        );
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            rect.min,
                            pos2(plus.left() - 8.0, rect.bottom()),
                        ));
                        let title_galley = painter.layout_no_wrap(
                            title.clone(),
                            theme::regular(theme::BODY_TEXT),
                            palette.text,
                        );
                        let title_width = title_galley.size().x;
                        let title_top = rect.center().y - title_galley.size().y / 2.0;
                        painter.galley(pos2(rect.left() + 8.0, title_top), title_galley, palette.text);
                        if !artist.is_empty() {
                            painter.text(
                                pos2(rect.left() + 8.0 + title_width + 8.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                artist,
                                theme::regular(theme::SMALL_TEXT),
                                palette.secondary,
                            );
                        }
                        if response.clicked() {
                            pending.push(Action::AddToLocalPlaylist {
                                playlist: at,
                                paths: vec![path.clone()],
                            });
                        }
                    }
                });
        });
    app.actions.extend(pending);
}
