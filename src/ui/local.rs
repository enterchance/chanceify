//! The Local songs page: audio files in the index's songs folder, with the
//! tempo, key and waveform measured from the sound, playlists of those files
//! made here, and playlists imported from Exportify files that can be
//! browsed with no internet.

use std::collections::HashMap;
use std::path::Path;

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, Vec2, pos2, vec2};

use crate::app::App;
use crate::local_library::{LocalSong, ScanView};
use crate::model::Action;
use crate::theme::{self, Icon, Palette};

const ROW: f32 = 40.0;
/// The picture at the start of each row.
const COVER: f32 = 30.0;
/// The heading row, as tall as the playlist table's.
const HEADER: f32 = 34.0;
/// Space between a cell's edge and its text, on each side.
const PAD: f32 = 6.0;
/// The playlists column, by id (see [`COLUMNS`]).
const PLAYLISTS_COLUMN: usize = 9;
/// The menu button's label, measured before it is drawn.
const FOLDERS: &str = "Folders";

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
    /// The file's size and date, which name its picture (see
    /// [`crate::local_library::Covers`]).
    stamp: (u64, u64),
    /// The Spotify song this file is, once it has been matched.
    spotify: Option<String>,
    /// The Spotify album picture for this line, when the file has none of
    /// its own.
    image: Option<String>,
    /// The file as a local playlist knows it: the path, kept even when the
    /// file is no longer in the library, so it can still be taken out.
    entry: Option<String>,
    /// The local playlists this file is in, by place.
    playlists: Vec<usize>,
}

/// Every column the table can show, by id: its heading, its name in the
/// columns menu, and its width when first shown. The title, artist, album
/// and genre share the room left over (the number is their share); the
/// others are as wide as their content (0) or as wide as they were dragged,
/// in points.
const COLUMNS: [(&str, &str, f32); 10] = [
    ("#", "number", 0.0),
    ("title", "title", 0.36),
    ("artist", "artist", 0.24),
    ("album", "album", 0.22),
    ("date added", "date added", 0.0),
    ("bpm", "bpm", 0.0),
    ("key", "key", 0.0),
    ("length", "length", 0.0),
    ("genre", "genre", 0.12),
    ("playlists", "playlists", 0.0),
];

/// Whether the column shares the room left over and gives it up first when
/// the table is narrow; the others keep the width their content needs.
fn flexible(id: usize) -> bool {
    matches!(id, 1 | 2 | 3 | 8)
}

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

/// The columns shown, in order, with their widths (see [`COLUMNS`]); a
/// saved list with nothing usable in it falls back to the usual set.
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
        .map(|(id, value)| {
            let value = if value.is_finite() { *value } else { 0.0 };
            let value = if flexible(*id as usize) {
                value.clamp(0.02, 0.8)
            } else {
                value.clamp(0.0, 1000.0)
            };
            (*id, value)
        })
        .collect();
    if cols.iter().any(|(id, _)| *id == 1) {
        cols
    } else {
        crate::settings::Settings::default().local_columns
    }
}

/// The narrowest a column may be: wide enough for its heading and for its
/// widest usual content, so a number or a date is never cut to a stub.
fn min_width(ui: &egui::Ui, id: usize) -> f32 {
    let measure = |text: &str, font: egui::FontId| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font, Color32::WHITE)
            .size()
            .x
    };
    let body = theme::regular(theme::BODY_TEXT);
    // The heading, with room for the sort arrow beside it.
    let heading = measure(COLUMNS[id].0, theme::regular(theme::SMALL_TEXT)) + 16.0;
    let content = match id {
        0 => measure("0000", body),
        1 => COVER + 10.0 + 80.0,
        2 | 3 => 64.0,
        4 => measure("0000-00-00", body),
        5 => measure("000", body),
        6 => measure("12A", body),
        7 => measure("00:00", body),
        8 => 56.0,
        // Two playlist tiles.
        _ => 18.0 * 2.0 + 3.0,
    };
    content.max(heading) + PAD * 2.0
}

/// The width of each shown column, in points, for a table `inner` wide.
/// Fixed columns take the width they were dragged to, or their minimum;
/// the flexible ones share what is left. When that is too little, the
/// flexible columns give way first, then the others, and no column ever
/// goes below its minimum (the table is cut at the edge instead).
fn fit_widths(values: &[f32], mins: &[f32], flexible: &[bool], inner: f32) -> Vec<f32> {
    let fixed_total: f32 = values
        .iter()
        .zip(mins)
        .zip(flexible)
        .filter(|(_, flexible)| !**flexible)
        .map(|((value, min), _)| fixed_width(*value, *min))
        .sum();
    let shares: f32 = values
        .iter()
        .zip(flexible)
        .filter(|(_, flexible)| **flexible)
        .map(|(value, _)| value.max(0.0))
        .sum::<f32>()
        .max(0.0001);
    let left_over = (inner - fixed_total).max(0.0);
    let mut widths: Vec<f32> = values
        .iter()
        .zip(mins)
        .zip(flexible)
        .map(|((value, min), flexible)| {
            if *flexible {
                (left_over * value.max(0.0) / shares).max(*min)
            } else {
                fixed_width(*value, *min)
            }
        })
        .collect();
    let mut excess = widths.iter().sum::<f32>() - inner;
    for giving in [true, false] {
        if excess <= 0.0 {
            break;
        }
        let room: f32 = widths
            .iter()
            .zip(mins)
            .zip(flexible)
            .filter(|(_, flexible)| **flexible == giving)
            .map(|((width, min), _)| width - min)
            .sum();
        if room <= 0.0 {
            continue;
        }
        let take = excess.min(room);
        for ((width, min), _) in widths
            .iter_mut()
            .zip(mins)
            .zip(flexible)
            .filter(|(_, flexible)| **flexible == giving)
        {
            *width -= (*width - min) / room * take;
        }
        excess -= take;
    }
    widths
}

/// A fixed column's width: as dragged (a saved value over one point), or
/// else just wide enough for its content.
fn fixed_width(value: f32, min: f32) -> f32 {
    if value > 1.0 { value.max(min) } else { min }
}

/// The saved form of a table laid out at `widths`: shares for the flexible
/// columns, points for the others.
fn store_widths(cols: &[(u8, f32)], widths: &[f32]) -> Vec<(u8, f32)> {
    let flexible_total: f32 = cols
        .iter()
        .zip(widths)
        .filter(|((id, _), _)| flexible(*id as usize))
        .map(|(_, width)| *width)
        .sum::<f32>()
        .max(1.0);
    cols.iter()
        .zip(widths)
        .map(|((id, _), width)| {
            if flexible(*id as usize) {
                (*id, (width / flexible_total).clamp(0.02, 0.8))
            } else {
                (*id, width.max(2.0))
            }
        })
        .collect()
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

/// "3:05", "10:00": minutes unpadded, seconds always two digits.
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

/// The album picture of a Spotify song already fetched, by its URI.
fn spotify_image(app: &App, uri: &str) -> Option<String> {
    let id = crate::util::uri_id(uri)?;
    app.track_cache
        .get(id)
        .and_then(|track| track.image(64))
        .map(str::to_string)
}

/// The picture to show for a file: the one inside it, or when it has none,
/// the Spotify album picture `fallback`. Nothing while the file is still
/// being looked at, so a row does not show one picture and then another.
fn cover_for(
    ui: &egui::Ui,
    art: &crate::images::ArtLoader,
    covers: &crate::local_library::Covers,
    index: &Path,
    file: Option<(&str, (u64, u64))>,
    fallback: Option<&str>,
) -> Option<String> {
    let Some((path, (size, modified))) = file else {
        return fallback.map(str::to_string);
    };
    match covers.lookup(ui.ctx(), art, index, path, size, modified) {
        Some(Some(url)) => Some(url),
        Some(None) => fallback.map(str::to_string),
        None => None,
    }
}

/// One line of text, cut with an ellipsis to fit `cell`'s width, placed in
/// `cell` as `anchor` says.
fn paint_fitted(
    painter: &egui::Painter,
    cell: Rect,
    anchor: Align2,
    text: &str,
    font: egui::FontId,
    color: Color32,
) {
    if text.is_empty() || cell.width() < 4.0 {
        return;
    }
    let galley = crate::bidi::layout(
        painter,
        text,
        font,
        color,
        cell.width(),
        1,
        Some(crate::bidi::ELLIPSIS),
    );
    let rect = anchor.align_size_within_rect(vec2(cell.width(), galley.size().y), cell);
    painter.galley(crate::bidi::galley_pos(rect, &galley), galley, color);
}

/// A line for one song file.
fn song_line(
    number: usize,
    song: &LocalSong,
    spotify: Option<String>,
    image: Option<String>,
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
        stamp: (song.size, song.modified),
        spotify,
        image,
        entry: Some(song.path.clone()),
        playlists,
    }
}

/// A playlist's picture: a music note on a tile.
fn paint_playlist_tile(ui: &egui::Ui, palette: &Palette, rect: Rect) {
    super::widgets::paint_cover(ui, palette, None, rect, 4.0, Icon::Music, None);
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let index = app.dirs.index_dir();
    if let Some(library) = app.local_scan.take_finished() {
        app.adopt_local_songs(library);
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
    ui.add_space(8.0);
    toolbar(app, ui, &palette, &mut view, &scan, &index);
    // One dim line, only while there is something to say.
    if scan.running {
        ui.add_space(2.0);
        theme::hint(
            ui,
            &palette,
            &format!("Measuring {} of {}: {}", scan.done, scan.total, scan.current),
        );
    } else if !app.local_ui.message.is_empty() {
        ui.add_space(2.0);
        theme::hint(ui, &palette, &app.local_ui.message.clone());
    }
    ui.add_space(10.0);
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
        // Read only, here: the closures below share it.
        let shared: &App = app;
        let member_of = |path: &str| membership.get(path).cloned().unwrap_or_default();
        let spotify_of = |path: &str| app.local_matches.uri_of(path).map(str::to_string);
        let image_of =
            |spotify: Option<&String>| spotify.and_then(|uri| spotify_image(shared, uri));
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
                        Some(song) => {
                            let spotify = spotify_of(path.as_str());
                            let image = image_of(spotify.as_ref());
                            lines.push(song_line(
                                number + 1,
                                song,
                                spotify,
                                image,
                                places,
                                &local_names,
                            ));
                        }
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
                                stamp: (0, 0),
                                spotify: None,
                                image: None,
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
                        let stamp = found.map_or((0, 0), |song| (song.size, song.modified));
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
                            stamp,
                            spotify: None,
                            image: spotify_image(shared, &track.uri),
                            entry: path.clone(),
                            path,
                            playlists: places,
                        });
                    }
                }
            }
            _ => {
                for (number, song) in app.local_songs.songs.iter().enumerate() {
                    let spotify = spotify_of(song.path.as_str());
                    let image = image_of(spotify.as_ref());
                    lines.push(song_line(
                        number + 1,
                        song,
                        spotify,
                        image,
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

    // Play the whole list, in order or shuffled, or stop what is playing,
    // with the search at the end of the same row.
    let playable: Vec<String> = lines.iter().filter_map(|line| line.path.clone()).collect();
    let playing_path = app
        .deck_current_path()
        .map(|path| path.to_string_lossy().to_string());
    ui.horizontal(|ui| {
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
        let field = 240.0f32.min(ui.available_width());
        ui.add_space((ui.available_width() - field - 12.0).max(0.0));
        ui.add(
            egui::TextEdit::singleline(&mut app.local_ui.filter)
                .hint_text("Search these songs")
                .desired_width(field)
                .min_size(vec2(0.0, theme::CONTROL_HEIGHT)),
        );
    });
    ui.add_space(6.0);

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
                    "Copy song files into the songs folder, or add a folder of your own from Folders, then scan.",
                ),
            }
        };
        super::widgets::empty_state(ui, &palette, Icon::Music, title, body);
        keep_view(ui, &view);
        return;
    }

    // Header: click to sort, drag a heading to move the column, drag the
    // line between two headings to resize, right-click for the columns.
    let cols = shown_columns(app);
    let width = ui.available_width();
    // The table's left edge, and its width inside the row.
    let origin = 4.0;
    let inner = (width - origin * 2.0).max(1.0);
    let mins: Vec<f32> = cols.iter().map(|(id, _)| min_width(ui, *id as usize)).collect();
    let flex: Vec<bool> = cols.iter().map(|(id, _)| flexible(*id as usize)).collect();
    let values: Vec<f32> = cols.iter().map(|(_, value)| *value).collect();
    let widths = fit_widths(&values, &mins, &flex, inner);
    let (header, header_response) = ui.allocate_exact_size(vec2(width, HEADER), Sense::click());
    let mut x = header.left() + origin;
    let mut clicked_sort: Option<u8> = None;
    let mut new_cols: Option<Vec<(u8, f32)>> = None;
    let drag_id = ui.id().with("local-head-drag");
    let dragging: Option<usize> = ui.data(|data| data.get_temp(drag_id));
    // Right-clicking a heading opens the header's menu too.
    let mut menu_anchor = header_response.clone();
    let font = theme::regular(theme::SMALL_TEXT);
    // Where the pointer would drop a dragged heading: before the first
    // column whose middle is past it.
    let drop_at = |pointer_x: f32| {
        let mut left = header.left() + origin;
        for (at, w) in widths.iter().enumerate() {
            if pointer_x < left + w / 2.0 {
                return (at, left);
            }
            left += w;
        }
        (widths.len(), left)
    };
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
            pos2(cell.left() + PAD - 1.0, header.top()),
            pos2((cell.right() - PAD).max(cell.left() + PAD), header.bottom()),
        );
        let painter = ui.painter().with_clip_rect(clip);
        let galley = painter.layout_no_wrap(COLUMNS[*id as usize].0.to_string(), font.clone(), color);
        let size = galley.size();
        let top_left = pos2(cell.left() + PAD, header.center().y - size.y / 2.0);
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
            let (to, _) = drop_at(pointer.x);
            let mut moved = cols.clone();
            let column = moved.remove(from);
            let to = if to > from { to - 1 } else { to };
            moved.insert(to.min(moved.len()), column);
            new_cols = Some(moved);
            ui.data_mut(|data| data.remove::<usize>(drag_id));
        }
        // A thin line between two headings, which is also the handle that
        // resizes the column on its left. Neither column goes below its
        // minimum.
        if at + 1 < cols.len() {
            let handle = Rect::from_center_size(pos2(cell.right(), cell.center().y), vec2(8.0, header.height()));
            let grip = ui
                .interact(handle, ui.id().with(("local-head-size", *id)), Sense::drag())
                .on_hover_cursor(egui::CursorIcon::ResizeColumn);
            let line = if grip.hovered() || grip.dragged() {
                Stroke::new(1.0, palette.secondary)
            } else {
                Stroke::new(1.0, palette.outline)
            };
            ui.painter().vline(cell.right(), header.y_range().shrink(9.0), line);
            if grip.dragged() {
                let pair = widths[at] + widths[at + 1];
                let left = (widths[at] + grip.drag_delta().x)
                    .min(pair - mins[at + 1])
                    .max(mins[at]);
                let mut sized = widths.clone();
                sized[at] = left;
                sized[at + 1] = (pair - left).max(mins[at + 1]);
                new_cols = Some(store_widths(&cols, &sized));
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
        let (_, mark) = drop_at(pointer.x);
        ui.painter().vline(mark, header.y_range().shrink(3.0), Stroke::new(2.0, palette.accent));
    }
    ui.painter()
        .hline(header.x_range().shrink(origin), header.bottom() - 0.5, Stroke::new(1.0, palette.outline));
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
    ui.add_space(4.0);
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
    let art = app.backend.art().clone();
    let covers = app.local_covers.clone();
    let mut pending: Vec<Action> = Vec::new();
    let mut open_playlist: Option<usize> = None;
    egui::ScrollArea::vertical()
        .id_salt("local-songs-scroll")
        .auto_shrink([false, false])
        .show_rows(ui, ROW, lines.len(), |ui, range| {
            for index_in_list in range {
                let line = &lines[index_in_list];
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
                    let centre = pos2(rect.left() + 3.0, rect.center().y);
                    painter.circle_filled(centre, 2.0, palette.accent);
                }
                let mut x = rect.left() + origin;
                for (at, (id, _)) in cols.iter().enumerate() {
                    let w = widths[at];
                    let id = *id as usize;
                    // The cell's text area, inside its padding.
                    let content = Rect::from_min_max(
                        pos2(x + PAD, rect.top()),
                        pos2((x + w - PAD).max(x + PAD + 1.0), rect.bottom()),
                    );
                    if id == PLAYLISTS_COLUMN {
                        // Small playlist tiles, as the playlist table's
                        // playlists column shows; click one to open it.
                        let size = 18.0;
                        let gap = 3.0;
                        let mut left = content.left();
                        for place in &line.playlists {
                            if left + size > content.right() {
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
                                .interact(tile, ui.id().with(("local-member", index_in_list, *place)), Sense::click())
                                .on_hover_text(&name);
                            let tile_painter = painter.with_clip_rect(content);
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
                    let mut text_cell = content;
                    if id == 1 {
                        // The song's picture, then its title.
                        let tile = Rect::from_min_size(
                            pos2(content.left(), rect.center().y - COVER / 2.0),
                            Vec2::splat(COVER),
                        );
                        let url = cover_for(
                            ui,
                            &art,
                            &covers,
                            &index,
                            line.path.as_deref().map(|path| (path, line.stamp)),
                            line.image.as_deref(),
                        );
                        super::widgets::paint_cover(
                            ui,
                            &palette,
                            url.as_deref(),
                            tile,
                            4.0,
                            Icon::Music,
                            Some(&art),
                        );
                        text_cell.min.x = tile.right() + 10.0;
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
                    paint_fitted(
                        &painter,
                        text_cell,
                        Align2::LEFT_CENTER,
                        &text,
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
                            .interact(play_rect, ui.id().with(("local-play", index_in_list)), Sense::click())
                            .on_hover_text("Play");
                        let tint = if button.hovered() { palette.text } else { palette.secondary };
                        theme::paint_icon(ui, Icon::Play, play_rect, 14.0, tint);
                        if button.clicked() {
                            used_button = true;
                            play_from_here(&mut pending);
                        }
                    }
                    let button = ui
                        .interact(search_rect, ui.id().with(("local-find", index_in_list)), Sense::click())
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

/// The row under the title: which table (every song, or the playlists) on
/// the left, and on the right the Spotify match and the Folders menu, which
/// holds everything about where the songs come from.
fn toolbar(
    app: &mut App,
    ui: &mut egui::Ui,
    palette: &Palette,
    view: &mut View,
    scan: &ScanView,
    index: &Path,
) {
    let matching = app.local_sync.progress();
    let match_label = match matching {
        Some((done, total)) => format!("Matching with Spotify… {done}/{total}"),
        None => match app.local_found() {
            0 => "Match with Spotify".to_string(),
            found => format!("Match with Spotify ({found} found)"),
        },
    };
    let total = app.local_songs.songs.len();
    let playlists = app.settings.local_playlists.len() + app.offline_playlists.len();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if theme::soft_button(
            ui,
            palette,
            Some(Icon::Music),
            &format!("All songs ({total})"),
            view.open == Open::All,
        )
        .clicked()
        {
            view.open = Open::All;
        }
        if theme::soft_button(
            ui,
            palette,
            Some(Icon::ListMusic),
            &format!("Playlists ({playlists})"),
            view.open != Open::All,
        )
        .clicked()
        {
            view.open = Open::Playlists;
        }
        // The two actions sit at the right end when the row has room (a
        // button with an icon is its label's width plus the icon's 21).
        let actions = theme::soft_button_width(ui, &match_label)
            + theme::soft_button_width(ui, FOLDERS)
            + 21.0 * 2.0
            + 8.0;
        let room = ui.available_width() - actions - 1.0;
        if room > 8.0 {
            ui.add_space(room);
        }
        if theme::soft_button(ui, palette, Some(Icon::Search), &match_label, matching.is_some())
            .on_hover_text("Find each song file on Spotify, to show its album picture and open it there")
            .clicked()
            && matching.is_none()
        {
            app.actions.push(Action::MatchSpotify);
        }
        let folders = theme::soft_button(ui, palette, Some(Icon::Settings), FOLDERS, false)
            .on_hover_text(format!(
                "Songs come from {} and the folders you add (mp3, flac, ogg, wav, m4a). Each file is measured once and remembered.",
                crate::local_library::songs_dir(index).display()
            ));
        egui::Popup::menu(&folders)
            .frame(super::widgets::menu_frame(palette))
            .show(|ui| folders_menu(app, ui, palette, scan, index));
    });
}

/// The Folders menu: scan, the folders songs are read from (each can be
/// opened or taken away), and adding more.
fn folders_menu(app: &mut App, ui: &mut egui::Ui, palette: &Palette, scan: &ScanView, index: &Path) {
    super::widgets::fit_menu(ui, 320.0);
    let label = if scan.running { "Scanning…" } else { "Scan for new songs" };
    if super::widgets::menu_item_enabled(ui, palette, Some(Icon::Refresh), label, !scan.running) {
        app.actions.push(Action::ScanMusic);
    }
    super::widgets::menu_separator(ui, palette);
    super::widgets::menu_group(ui, palette, "Folders");
    if super::widgets::menu_item(ui, palette, Some(Icon::ExternalLink), "Open songs folder") {
        let folder = crate::local_library::songs_dir(index);
        std::fs::create_dir_all(&folder).ok();
        if let Err(error) = crate::opener::open(&folder) {
            app.toast_error(format!("Could not open the folder: {error}"));
        }
    }
    let folders = app.settings.music_folders.clone();
    for (at, folder) in folders.iter().enumerate() {
        let name = Path::new(folder)
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| folder.clone());
        super::widgets::menu_submenu(ui, palette, Some(Icon::Music), &name, |ui| {
            super::widgets::fit_menu(ui, 360.0);
            // The whole path, dim, over what can be done with it.
            super::widgets::menu_group(ui, palette, folder);
            if super::widgets::menu_item(ui, palette, Some(Icon::ExternalLink), "Open folder")
                && let Err(error) = crate::opener::open(Path::new(folder))
            {
                app.toast_error(format!("Could not open the folder: {error}"));
            }
            if super::widgets::menu_item(ui, palette, Some(Icon::X), "Stop reading this folder") {
                app.actions.push(Action::RemoveMusicFolder(at));
            }
        });
    }
    super::widgets::menu_separator(ui, palette);
    if super::widgets::menu_item(ui, palette, Some(Icon::Plus), "Add a music folder…") {
        app.actions.push(Action::AddMusicFolder);
    }
    if super::widgets::menu_item(ui, palette, Some(Icon::Plus), "Add song files…") {
        app.actions.push(Action::AddMusicFiles);
    }
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
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        if theme::soft_button(ui, palette, Some(Icon::ChevronLeft), "Playlists", false).clicked() {
            view.open = Open::Playlists;
            view.renaming = None;
        }
        let (tile, _) = ui.allocate_exact_size(Vec2::splat(theme::CONTROL_HEIGHT), Sense::hover());
        paint_playlist_tile(ui, palette, tile);
        let focus = std::mem::take(&mut view.focus_rename);
        let renaming = matches!(&view.renaming, Some((renaming, _)) if *renaming == at);
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
        // What can be done with the playlist, on the same row.
        ui.add_space(8.0);
        if theme::soft_button(ui, palette, Some(Icon::Plus), "Add songs", view.adding).clicked() {
            view.adding = !view.adding;
            view.add_filter.clear();
        }
        if !renaming
            && theme::soft_button(ui, palette, Some(Icon::Pencil), "Rename", false).clicked()
        {
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
/// it, filtered by what is typed, each with its picture, artist and length,
/// a button to hear it and a button to add it (a click on the row adds it
/// too). It takes most of the page, so a long library is easy to go
/// through.
fn add_songs_panel(app: &mut App, ui: &mut egui::Ui, palette: &Palette, view: &mut View, at: usize) {
    let Some(playlist) = app.settings.local_playlists.get(at) else {
        return;
    };
    let inside: std::collections::HashSet<&str> =
        playlist.paths.iter().map(String::as_str).collect();
    let needle = view.add_filter.trim().to_lowercase();
    let candidates: Vec<&LocalSong> = app
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
        .collect();
    let art = app.backend.art().clone();
    let covers = app.local_covers.clone();
    let index = app.dirs.index_dir();
    // Read only, inside the list: the picture of a matched song.
    let shared: &App = app;
    let playing = app
        .deck_current_path()
        .map(|path| path.to_string_lossy().to_string());
    // Leave room below for a few of the playlist's own rows.
    let list_height = (ui.available_height() - 200.0).max(220.0);
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
                        paths: candidates.iter().map(|song| song.path.clone()).collect(),
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
                        "No song files yet. Add a folder from Folders, then scan."
                    } else if needle.is_empty() {
                        "Every song file is already in this playlist."
                    } else {
                        "No songs match."
                    },
                );
                return;
            }
            let row = ROW;
            let length_width = ui
                .painter()
                .layout_no_wrap("00:00".to_string(), theme::regular(theme::SMALL_TEXT), palette.secondary)
                .size()
                .x;
            egui::ScrollArea::vertical()
                .id_salt(("local-add-songs", at))
                .max_height(list_height)
                .auto_shrink([false, true])
                .show_rows(ui, row, candidates.len(), |ui, range| {
                    for line in range {
                        let song = candidates[line];
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(ui.available_width(), row), Sense::click());
                        let hovered = ui.rect_contains_pointer(rect);
                        let is_playing = playing.as_deref() == Some(song.path.as_str());
                        if is_playing {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(6), palette.accent.gamma_multiply(0.18));
                        } else if hovered {
                            ui.painter().rect_filled(rect, CornerRadius::same(6), palette.surface_hover);
                        }
                        let center = rect.center().y;
                        // The picture.
                        let tile = Rect::from_min_size(
                            pos2(rect.left() + 6.0, center - COVER / 2.0),
                            Vec2::splat(COVER),
                        );
                        let fallback = shared
                            .local_matches
                            .uri_of(&song.path)
                            .and_then(|uri| spotify_image(shared, uri));
                        let url = cover_for(
                            ui,
                            &art,
                            &covers,
                            &index,
                            Some((song.path.as_str(), (song.size, song.modified))),
                            fallback.as_deref(),
                        );
                        super::widgets::paint_cover(ui, palette, url.as_deref(), tile, 4.0, Icon::Music, Some(&art));
                        // From the right: add, hear it, its length.
                        let plus = Rect::from_center_size(pos2(rect.right() - 18.0, center), Vec2::splat(24.0));
                        let play = Rect::from_center_size(pos2(plus.center().x - 30.0, center), Vec2::splat(24.0));
                        let length_right = play.left() - 10.0;
                        let painter = ui.painter().with_clip_rect(rect);
                        painter.text(
                            pos2(length_right, center),
                            Align2::RIGHT_CENTER,
                            length(song.duration_ms),
                            theme::regular(theme::SMALL_TEXT),
                            palette.secondary,
                        );
                        // The title over the artist.
                        let text_left = tile.right() + 10.0;
                        let text_right = (length_right - length_width - 12.0).max(text_left + 1.0);
                        paint_fitted(
                            &painter,
                            Rect::from_min_max(pos2(text_left, rect.top()), pos2(text_right, center + 1.0)),
                            Align2::LEFT_BOTTOM,
                            &song.title,
                            theme::regular(theme::BODY_TEXT),
                            if is_playing { palette.accent } else { palette.text },
                        );
                        paint_fitted(
                            &painter,
                            Rect::from_min_max(pos2(text_left, center + 2.0), pos2(text_right, rect.bottom())),
                            Align2::LEFT_TOP,
                            &song.artist,
                            theme::regular(theme::SMALL_TEXT),
                            palette.secondary,
                        );
                        // Hear it: shown on the row under the pointer, and on
                        // the one playing, where it stops it.
                        let mut used_button = false;
                        if hovered || is_playing {
                            let button = ui
                                .interact(play, ui.id().with(("local-add-play", at, line)), Sense::click())
                                .on_hover_text(if is_playing { "Stop" } else { "Play" });
                            let tint = if button.hovered() {
                                palette.text
                            } else if is_playing {
                                palette.accent
                            } else {
                                palette.secondary
                            };
                            theme::paint_icon(
                                ui,
                                if is_playing { Icon::Square } else { Icon::Play },
                                play,
                                14.0,
                                tint,
                            );
                            if button.clicked() {
                                used_button = true;
                                pending.push(if is_playing {
                                    Action::StopLocalFiles
                                } else {
                                    Action::PlayLocalFiles {
                                        paths: vec![song.path.clone()],
                                        index: 0,
                                        shuffle: None,
                                    }
                                });
                            }
                        }
                        let add = ui
                            .interact(plus, ui.id().with(("local-add-plus", at, line)), Sense::click())
                            .on_hover_text("Add to this playlist");
                        theme::paint_icon(
                            ui,
                            Icon::Plus,
                            plus,
                            16.0,
                            if add.hovered() {
                                palette.accent
                            } else if hovered {
                                palette.text
                            } else {
                                palette.secondary
                            },
                        );
                        if !used_button && (add.clicked() || response.clicked()) {
                            pending.push(Action::AddToLocalPlaylist {
                                playlist: at,
                                paths: vec![song.path.clone()],
                            });
                        }
                    }
                });
        });
    app.actions.extend(pending);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths_are_compact() {
        assert_eq!(length(0), "");
        assert_eq!(length(185_000), "3:05");
        assert_eq!(length(600_000), "10:00");
    }

    #[test]
    fn days_read_as_dates() {
        assert_eq!(day_text(0), "");
        // 2026-10-15 00:00 UTC.
        assert_eq!(day_text(1_792_022_400), "2026-10-15");
    }

    #[test]
    fn fixed_columns_hug_their_content_and_the_rest_share_the_room() {
        // number (fixed, never dragged), title, artist, bpm (dragged to 50).
        let values = [0.0, 0.6, 0.4, 50.0];
        let mins = [40.0, 120.0, 70.0, 36.0];
        let flexible = [false, true, true, false];
        let widths = fit_widths(&values, &mins, &flexible, 1000.0);
        assert_eq!(widths[0], 40.0);
        assert_eq!(widths[3], 50.0);
        assert!((widths[1] - 546.0).abs() < 0.01);
        assert!((widths[2] - 364.0).abs() < 0.01);
        assert!((widths.iter().sum::<f32>() - 1000.0).abs() < 0.01);
    }

    #[test]
    fn a_narrow_table_squeezes_only_down_to_the_minimums() {
        let values = [0.0, 0.6, 0.4, 50.0];
        let mins = [40.0, 120.0, 70.0, 36.0];
        let flexible = [false, true, true, false];
        // The flexible columns give way first.
        let widths = fit_widths(&values, &mins, &flexible, 300.0);
        assert_eq!(widths[0], 40.0);
        assert_eq!(widths[3], 50.0);
        assert!((widths.iter().sum::<f32>() - 300.0).abs() < 0.01);
        assert!(widths[1] >= 120.0 && widths[2] >= 70.0);
        // Then the dragged fixed one, and never below any minimum.
        let widths = fit_widths(&values, &mins, &flexible, 100.0);
        for (width, min) in widths.iter().zip(mins) {
            assert!(*width >= min - 0.01, "{width} under {min}");
        }
        assert_eq!(widths[3], 36.0);
    }

    #[test]
    fn stored_widths_lay_out_the_same_again() {
        let cols = vec![(0u8, 0.0), (1, 0.6), (2, 0.4), (5, 50.0)];
        let mins = [40.0, 120.0, 70.0, 36.0];
        let flexible = [false, true, true, false];
        let dragged = [40.0, 500.0, 400.0, 60.0];
        let stored = store_widths(&cols, &dragged);
        let values: Vec<f32> = stored.iter().map(|(_, value)| *value).collect();
        let widths = fit_widths(&values, &mins, &flexible, 1000.0);
        for (width, wanted) in widths.iter().zip(dragged) {
            assert!((width - wanted).abs() < 0.01, "{width} != {wanted}");
        }
    }
}
