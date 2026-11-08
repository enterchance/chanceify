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

/// A row of the song table, as tall as a row of the playlist table.
const ROW: f32 = theme::ROW_HEIGHT;
/// The picture at the start of the title cell, the playlist table's size.
const COVER: f32 = 40.0;
/// The room the picture takes before the song's name, as the playlist
/// table's cover column does.
const COVER_COLUMN: f32 = 52.0;
/// The heading row, as tall as the playlist table's.
const HEADER: f32 = 34.0;
/// Space between a cell's edge and its text.
const PAD: f32 = 6.0;
/// The # column, as wide as the playlist table's.
const NUMBER_WIDTH: f32 = 44.0;
/// The widest a column can be dragged.
const MAX_WIDTH: f32 = 640.0;
/// The grab area of a column edge, as the playlist table's.
const HANDLE: f32 = 14.0;
/// The playlists column, by id (see [`COLUMNS`]).
const PLAYLISTS_COLUMN: usize = 9;
/// The playlist tiles on the Playlists view aim for about this width.
const TILE_TARGET: f32 = 170.0;

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
/// columns menu, and its width in points when first shown. The title takes
/// whatever room the others leave, as the song name does in the playlist
/// table; every other column keeps the width it was dragged to.
const COLUMNS: [(&str, &str, f32); 10] = [
    ("#", "number", NUMBER_WIDTH),
    ("title", "title", 0.0),
    ("artist", "artist", 160.0),
    ("album", "album", 160.0),
    ("date added", "date added", 104.0),
    ("bpm", "bpm", 52.0),
    ("key", "key", 56.0),
    ("length", "length", 60.0),
    ("genre", "genre", 140.0),
    ("playlists", "playlists", 60.0),
];

/// The title column, by id.
const TITLE_COLUMN: u8 = 1;
/// The artist column, by id. The artist is written under the song's name,
/// as in the playlist table, unless this column is shown.
const ARTIST_COLUMN: u8 = 2;

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
    /// The songs picked with a click, Ctrl+click or Shift+click, by the
    /// file a line stands for (`Line::entry`), and the table they were
    /// picked in.
    picked: Vec<String>,
    picked_in: Open,
    /// Where a Shift+click range starts.
    anchor: Option<String>,
    /// The playable file under the pointer on the last frame.
    hovered: Option<String>,
}

fn view_id() -> egui::Id {
    egui::Id::new("local-songs-view")
}

fn keep_view(ui: &egui::Ui, view: &View) {
    let view = view.clone();
    ui.ctx().data_mut(|data| data.insert_temp(view_id(), view));
}

/// The columns shown, in order, with their saved widths (see [`COLUMNS`]);
/// a saved list with nothing usable in it falls back to the usual set.
///
/// Files from before the columns were sized like the playlist table's kept
/// a share of the room (1 or less) for the title, artist, album and genre.
/// Such a share now means the column's first width, and an artist column
/// saved that way is left out, because the artist is written under the
/// song's name instead (ticking it again in the columns menu brings it
/// back, with a width in points).
fn shown_columns(app: &App) -> Vec<(u8, f32)> {
    let mut seen = [false; COLUMNS.len()];
    let cols: Vec<(u8, f32)> = app
        .settings
        .local_columns
        .iter()
        .filter(|(id, value)| {
            let at = *id as usize;
            at < COLUMNS.len()
                && !(*id == ARTIST_COLUMN && *value <= 1.0)
                && !std::mem::replace(&mut seen[at], true)
        })
        .map(|(id, value)| {
            let value = if value.is_finite() && *value > 1.0 {
                value.min(MAX_WIDTH)
            } else {
                0.0
            };
            (*id, value)
        })
        .collect();
    if cols.iter().any(|(id, _)| *id == TITLE_COLUMN) {
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
    let body = theme::regular(13.0);
    // The heading, with room for the sort arrow beside it.
    let heading = measure(COLUMNS[id].0, theme::regular(theme::SMALL_TEXT)) + 16.0;
    let content = match id {
        0 => return NUMBER_WIDTH,
        1 => return COVER_COLUMN + 100.0,
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

/// The width a column asks for: the width it was dragged to (a saved value
/// over one point), or else its first width, and never under `min`. The #
/// column is always the same width.
fn wanted_width(id: usize, value: f32, min: f32) -> f32 {
    if id == 0 {
        NUMBER_WIDTH
    } else if value > 1.0 {
        value.clamp(min, MAX_WIDTH.max(min))
    } else {
        COLUMNS[id].2.max(min)
    }
}

/// Lays the table out the way the playlist table is: every column keeps
/// the width it asks for (`wanted`) and the title, at `title`, takes the
/// room that is left of `room`. When the table is `centred` and the title
/// would grow past `cap`, the title stops at `cap` and the table sits in the
/// middle; the second value is how far it moves right. When the room is too
/// small, the other columns give way towards their minimums before the title
/// goes under its own; past that the table is cut at the edge.
fn lay_out(
    wanted: &[f32],
    mins: &[f32],
    title: usize,
    room: f32,
    cap: f32,
    centred: bool,
) -> (Vec<f32>, f32) {
    let mut widths = wanted.to_vec();
    if title >= widths.len() {
        return (widths, 0.0);
    }
    widths[title] = 0.0;
    let others: f32 = widths.iter().sum();
    let title_min = mins.get(title).copied().unwrap_or(0.0);
    let mut left = room - others;
    if left < title_min {
        let need = title_min - left;
        let slack: f32 = widths
            .iter()
            .zip(mins)
            .enumerate()
            .filter(|(at, _)| *at != title)
            .map(|(_, (width, min))| (width - min).max(0.0))
            .sum();
        if slack > 0.0 {
            let take = need.min(slack);
            for (at, (width, min)) in widths.iter_mut().zip(mins).enumerate() {
                if at != title {
                    *width -= (*width - min).max(0.0) / slack * take;
                }
            }
            left += take;
        }
    }
    let title_width = left.max(title_min);
    let shift = if centred && title_width > cap {
        (title_width - cap) / 2.0
    } else {
        0.0
    };
    widths[title] = title_width - shift * 2.0;
    (widths, shift)
}

/// The width a column dragged from `width` by `delta` points ends up at.
fn dragged_width(width: f32, delta: f32, min: f32) -> f32 {
    (width + delta).clamp(min, MAX_WIDTH.max(min))
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


/// The lines of the open table (every song, or one playlist), searched and
/// sorted as the page says.
fn build_lines(app: &App, open: Open, local_names: &[String]) -> Vec<Line> {
    let needle = app.local_ui.filter.trim().to_lowercase();
    let mut lines: Vec<Line> = Vec::new();
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
    let image_of = |spotify: Option<&String>| spotify.and_then(|uri| spotify_image(app, uri));
    match open {
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
                        lines.push(song_line(number + 1, song, spotify, image, places, local_names));
                    }
                    None => {
                        // The file has moved or gone: still listed, so it
                        // can be taken out, but there is nothing to play.
                        let stem = std::path::Path::new(path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| path.clone());
                        let names = playlist_names(&places, local_names);
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
                            playlist_names(&places, local_names),
                        ],
                        bpm: track.tempo,
                        length_ms: track.duration_ms,
                        have_file: Some(have),
                        stamp,
                        spotify: None,
                        image: spotify_image(app, &track.uri),
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
                    local_names,
                ));
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
            0 => a.cells[0]
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
    lines
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
    // Songs picked in one table are not picked in another.
    if view.picked_in != view.open {
        view.picked.clear();
        view.anchor = None;
        view.picked_in = view.open;
    }

    let local_names: Vec<String> = app
        .settings
        .local_playlists
        .iter()
        .map(|playlist| playlist.name.clone())
        .collect();
    let lines = if view.open == Open::Playlists {
        Vec::new()
    } else {
        build_lines(app, view.open, &local_names)
    };
    let playable: Vec<String> = lines.iter().filter_map(|line| line.path.clone()).collect();

    ui.add_space(4.0);
    let before = view.open;
    header_row(app, ui, &palette, &mut view, &scan, &index, &playable);
    if view.open != before {
        // Another table was picked: its lines are built on the next frame.
        keep_view(ui, &view);
        ui.ctx().request_repaint();
        return;
    }
    // One dim line, only while there is something to say.
    if scan.running {
        theme::hint(
            ui,
            &palette,
            &format!("Measuring {} of {}: {}", scan.done, scan.total, scan.current),
        );
    } else if !app.local_ui.message.is_empty() {
        theme::hint(ui, &palette, &app.local_ui.message.clone());
    }
    ui.add_space(6.0);
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

    if lines.is_empty() {
        let (title, body) = if !app.local_ui.filter.trim().is_empty() {
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

    // Escape lets go of the picked songs. Queueing them is the right-click
    // menu, or your own queue keys over a row.
    if !ui.ctx().text_edit_focused() {
        if !view.picked.is_empty() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            view.picked.clear();
            view.anchor = None;
        }
    }

    song_table(app, ui, &palette, &mut view, &lines, &playable, &local_names, open_local, &index);
    keep_view(ui, &view);
}

/// The playable files of the picked lines, in the table's order.
fn picked_paths(lines: &[Line], picked: &[String]) -> Vec<String> {
    if picked.is_empty() {
        return Vec::new();
    }
    lines
        .iter()
        .filter(|line| line.entry.as_ref().is_some_and(|entry| picked.contains(entry)))
        .filter_map(|line| line.path.clone())
        .collect()
}

/// A [`theme::soft_button`] with an icon, and with only the icon when
/// `label` is empty: a round pill as tall as the others.
fn pill(ui: &mut egui::Ui, palette: &Palette, icon: Icon, label: &str, active: bool) -> egui::Response {
    if !label.is_empty() {
        return theme::soft_button(ui, palette, Some(icon), label, active);
    }
    let edge = pill_height(ui);
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(edge), Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if active {
            palette.text
        } else if response.hovered() {
            palette.surface_hover
        } else {
            palette.surface
        };
        ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
        let color = if active { palette.window } else { palette.text };
        icon.image(color, 15.0)
            .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(15.0)));
    }
    theme::focus_ring(ui, &response);
    response
}

/// How tall a [`theme::soft_button`] is.
fn pill_height(ui: &egui::Ui) -> f32 {
    ui.painter()
        .layout_no_wrap("Ag".to_string(), theme::medium(theme::BODY_TEXT), Color32::WHITE)
        .size()
        .y
        + 14.0
}

/// How wide [`pill`] draws a button.
fn pill_width(ui: &egui::Ui, label: &str) -> f32 {
    if label.is_empty() {
        pill_height(ui)
    } else {
        theme::soft_button_width(ui, label) + 21.0
    }
}

/// The one row at the top of the page: the title; which table (every song,
/// or the playlists); playing the songs shown; and at the right end the
/// search, the Spotify match and the Folders menu, which holds everything
/// about where the songs come from. The buttons drop their words when the
/// row is short of room, and only a window too narrow even for that puts
/// the right end on a second row.
fn header_row(
    app: &mut App,
    ui: &mut egui::Ui,
    palette: &Palette,
    view: &mut View,
    scan: &ScanView,
    index: &Path,
    playable: &[String],
) {
    let matching = app.local_sync.progress();
    let found = app.local_found();
    let (match_label, match_tip) = match matching {
        Some((done, total)) => (
            format!("{done}/{total}"),
            format!("Matching with Spotify… {done} of {total}"),
        ),
        None if found > 0 => (
            format!("{found}"),
            format!(
                "Match with Spotify ({found} found): find each song file on Spotify, to show its album picture and open it there"
            ),
        ),
        None => (
            String::new(),
            "Match with Spotify: find each song file on Spotify, to show its album picture and open it there"
                .to_string(),
        ),
    };
    let total = app.local_songs.songs.len();
    let playlists = app.settings.local_playlists.len() + app.offline_playlists.len();
    let all_label = format!("All songs ({total})");
    let playlists_label = format!("Playlists ({playlists})");
    let songs_shown = view.open != Open::Playlists && !playable.is_empty();
    let stop = app.deck_current_path().is_some();
    let title = "Local songs";

    // Measured before drawing, to choose between words and bare icons.
    let gap = 8.0;
    let title_width = ui
        .painter()
        .layout_no_wrap(title.to_string(), theme::bold(theme::TITLE_TEXT), Color32::WHITE)
        .size()
        .x;
    let toggles = pill_width(ui, &all_label) + pill_width(ui, &playlists_label) + gap * 2.0;
    let playing = |words: bool| {
        let mut width = 0.0;
        if songs_shown {
            width += pill_width(ui, if words { "Play all" } else { "" })
                + pill_width(ui, if words { "Shuffle" } else { "" })
                + gap * 2.0;
        }
        if stop {
            width += pill_width(ui, if words { "Stop" } else { "" }) + gap;
        }
        width
    };
    let field_most = 220.0;
    let field_least = 140.0;
    let actions = pill_width(ui, &match_label) + pill_width(ui, "") + gap * 2.0;
    let available = ui.available_width();
    let left_words = title_width + gap + toggles + playing(true);
    let words = left_words + field_most + actions + gap <= available;
    let left = title_width + gap + toggles + playing(words);
    let field = (available - left - actions - gap * 2.0).clamp(field_least, field_most);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(gap, gap);
        theme::text(ui, title, theme::bold(theme::TITLE_TEXT), palette.text);
        if theme::soft_button(ui, palette, Some(Icon::Music), &all_label, view.open == Open::All).clicked() {
            view.open = Open::All;
        }
        if theme::soft_button(
            ui,
            palette,
            Some(Icon::ListMusic),
            &playlists_label,
            view.open != Open::All,
        )
        .clicked()
        {
            view.open = Open::Playlists;
        }
        if songs_shown {
            let response = pill(ui, palette, Icon::Play, if words { "Play all" } else { "" }, false);
            let response = if words { response } else { response.on_hover_text("Play all") };
            if response.clicked() {
                app.actions.push(Action::PlayLocalFiles {
                    paths: playable.to_vec(),
                    index: 0,
                    shuffle: Some(false),
                });
            }
            let response = pill(ui, palette, Icon::Shuffle, if words { "Shuffle" } else { "" }, false);
            let response = if words { response } else { response.on_hover_text("Shuffle") };
            if response.clicked() {
                let index = rand::random_range(0..playable.len());
                app.actions.push(Action::PlayLocalFiles {
                    paths: playable.to_vec(),
                    index,
                    shuffle: Some(true),
                });
            }
        }
        if stop {
            let response = pill(ui, palette, Icon::Square, if words { "Stop" } else { "" }, false);
            let response = if words { response } else { response.on_hover_text("Stop") };
            if response.clicked() {
                app.actions.push(Action::StopLocalFiles);
            }
        }
        // The right end: on this row when it fits, else on its own.
        let right = field + actions;
        let room = ui.available_width() - right - 1.0;
        if room >= 0.0 {
            ui.add_space(room);
        } else {
            ui.end_row();
            ui.add_space((ui.available_width() - right - 1.0).max(0.0));
        }
        ui.add(
            egui::TextEdit::singleline(&mut app.local_ui.filter)
                .hint_text(if view.open == Open::Playlists {
                    "Search playlists"
                } else {
                    "Search these songs"
                })
                .desired_width(field)
                .min_size(vec2(0.0, theme::CONTROL_HEIGHT)),
        );
        if pill(ui, palette, Icon::Disc, &match_label, matching.is_some())
            .on_hover_text(match_tip)
            .clicked()
            && matching.is_none()
        {
            app.actions.push(Action::MatchSpotify);
        }
        let folders = pill(ui, palette, Icon::Settings, "", false).on_hover_text(format!(
            "Folders: songs come from {} and the folders you add (mp3, flac, ogg, wav, m4a). Each file is measured once and remembered.",
            crate::local_library::songs_dir(index).display()
        ));
        egui::Popup::menu(&folders)
            .frame(super::widgets::menu_frame(palette))
            .show(|ui| folders_menu(app, ui, palette, scan, index));
    });
}

/// A column edge that can be dragged, drawn as the playlist table's are: a
/// thin rule with a three-dot grip that lights up under the pointer and
/// turns the accent colour while it is dragged.
fn grip(ui: &egui::Ui, palette: &Palette, header: Rect, x: f32, id: egui::Id) -> egui::Response {
    let grab = Rect::from_center_size(pos2(x, header.center().y), vec2(HANDLE, header.height()));
    let response = ui.interact(grab, id, Sense::click_and_drag());
    let dragging = response.dragged();
    if ui.is_rect_visible(grab) {
        let color = if dragging {
            palette.accent
        } else if response.hovered() {
            palette.secondary
        } else {
            palette.outline
        };
        if dragging || response.hovered() {
            ui.painter().rect_filled(
                Rect::from_center_size(pos2(x, header.center().y), vec2(8.0, header.height() - 6.0)),
                CornerRadius::same(4),
                color.gamma_multiply(0.18),
            );
        }
        ui.painter().vline(x, header.y_range().shrink(4.0), Stroke::new(1.0, color));
        let dot = Rect::from_center_size(pos2(x, header.center().y), vec2(1.5, 1.5));
        for offset in [-4.0f32, 0.0, 4.0] {
            ui.painter()
                .rect_filled(dot.translate(vec2(0.0, offset)), CornerRadius::same(1), color);
        }
    }
    response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
}

/// The song table: a header to sort by (click), move (drag a heading) and
/// size (drag a column edge) the columns, then a row per song.
#[expect(clippy::too_many_arguments)]
fn song_table(
    app: &mut App,
    ui: &mut egui::Ui,
    palette: &Palette,
    view: &mut View,
    lines: &[Line],
    playable: &[String],
    local_names: &[String],
    open_local: Option<usize>,
    index: &Path,
) {
    let cols = shown_columns(app);
    let width = ui.available_width();
    let shape = app.settings.track_columns;
    let spread = shape.spread;
    // The table's room inside the row, with the playlist table's margins.
    let room = (width - 16.0).max(1.0);
    let title_at = cols
        .iter()
        .position(|(id, _)| *id == TITLE_COLUMN)
        .unwrap_or(0);
    let mins: Vec<f32> = cols.iter().map(|(id, _)| min_width(ui, *id as usize)).collect();
    let mut wanted: Vec<f32> = cols
        .iter()
        .zip(&mins)
        .map(|((id, value), min)| wanted_width(*id as usize, *value, *min))
        .collect();
    if spread {
        // Spread evenly, as the playlist table can be: the columns share
        // the room, the title taking two shares.
        let sharing = cols
            .iter()
            .filter(|(id, _)| *id != 0 && *id != TITLE_COLUMN)
            .count();
        let numbers: f32 = cols
            .iter()
            .filter(|(id, _)| *id == 0)
            .map(|_| NUMBER_WIDTH)
            .sum();
        let each = (room - numbers) / (sharing as f32 + 2.0);
        for ((id, _), (width, min)) in cols.iter().zip(wanted.iter_mut().zip(&mins)) {
            if *id != 0 && *id != TITLE_COLUMN {
                *width = each.max(*min);
            }
        }
    }
    let centred = !shape.lock_left && !spread;
    let (widths, shift) = lay_out(&wanted, &mins, title_at, room, shape.title_cap(), centred);
    let table_left = |rect: Rect| rect.left() + 8.0 + shift;
    let table_width: f32 = widths.iter().sum();
    let artist_column = cols.iter().any(|(id, _)| *id == ARTIST_COLUMN);

    // Header.
    let sort = (app.local_ui.sort as usize).min(COLUMNS.len() - 1);
    let descending = app.local_ui.descending;
    let (header, header_response) = ui.allocate_exact_size(vec2(width, HEADER), Sense::click());
    let mut clicked_sort: Option<u8> = None;
    let mut new_cols: Option<Vec<(u8, f32)>> = None;
    let mut new_title_cap: Option<f32> = None;
    let drag_id = ui.id().with("local-head-drag");
    let dragging: Option<usize> = ui.data(|data| data.get_temp(drag_id));
    // Right-clicking a heading or an edge opens the header's menu too.
    let mut menu_anchor = header_response.clone();
    let font = theme::regular(theme::SMALL_TEXT);
    let starts: Vec<f32> = widths
        .iter()
        .scan(table_left(header), |x, w| {
            let at = *x;
            *x += w;
            Some(at)
        })
        .collect();
    // Where the pointer would drop a dragged heading: before the first
    // column whose middle is past it.
    let drop_at = |pointer_x: f32| {
        for (at, (left, w)) in starts.iter().zip(&widths).enumerate() {
            if pointer_x < left + w / 2.0 {
                return (at, *left);
            }
        }
        (widths.len(), table_left(header) + table_width)
    };
    for (at, (id, _)) in cols.iter().enumerate() {
        let w = widths[at];
        let cell = Rect::from_min_size(pos2(starts[at], header.top()), vec2(w, header.height()));
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
        // Lowercase and small, as the playlist table's headings are: the #
        // in the middle of its column, the title over the song names, the
        // others at the start of their cell. The sorting one is lit, with a
        // small drawn arrow.
        let text_left = match *id {
            0 => cell.left() + 18.0,
            TITLE_COLUMN => cell.left() + COVER_COLUMN,
            _ => cell.left() + PAD,
        };
        let clip = Rect::from_min_max(
            pos2(text_left - 1.0, header.top()),
            pos2((cell.right() - 2.0).max(text_left), header.bottom()),
        );
        let painter = ui.painter().with_clip_rect(clip);
        let galley = painter.layout_no_wrap(COLUMNS[*id as usize].0.to_string(), font.clone(), color);
        let size = galley.size();
        let top_left = pos2(text_left, header.center().y - size.y / 2.0);
        painter.galley(top_left, galley, color);
        if active {
            let center = pos2(top_left.x + size.x + 8.0, header.center().y);
            let (wing, tip) = if descending { (-2.8, 3.2) } else { (2.8, -3.2) };
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
    }
    // The edges that size a column, drawn over the headings. As in the
    // playlist table the title gives way to its neighbours: a column before
    // it is sized from its right edge, one after it from its left edge. The
    // # keeps its width. The title's own edge sets how wide the song names
    // may grow while the table sits in the middle, as it does there.
    if !spread {
        let mut placed: Vec<f32> = Vec::new();
        for (at, (id, _)) in cols.iter().enumerate() {
            if *id == 0 || *id == TITLE_COLUMN {
                continue;
            }
            let grows_right = at < title_at;
            let x = if grows_right { starts[at] + widths[at] } else { starts[at] };
            if x <= header.left() + 4.0 || x >= header.right() - 4.0 {
                continue;
            }
            if placed.iter().any(|other| (other - x).abs() < HANDLE) {
                continue;
            }
            placed.push(x);
            let handle = grip(ui, palette, header, x, ui.id().with(("local-head-size", *id)));
            if handle.dragged() {
                let delta = ui.input(|input| input.pointer.delta()).x;
                if delta != 0.0 {
                    let sign = if grows_right { delta } else { -delta };
                    let mut changed = new_cols.clone().unwrap_or_else(|| cols.clone());
                    changed[at].1 = dragged_width(widths[at], sign, mins[at]);
                    new_cols = Some(changed);
                }
            }
            if handle.double_clicked() {
                // Back to its first width.
                let mut changed = new_cols.clone().unwrap_or_else(|| cols.clone());
                changed[at].1 = 0.0;
                new_cols = Some(changed);
            }
            menu_anchor = menu_anchor.union(handle);
        }
        let x = starts[title_at];
        if centred
            && title_at > 0
            && x > header.left() + 4.0
            && !placed.iter().any(|other| (other - x).abs() < HANDLE)
        {
            let handle = grip(ui, palette, header, x, ui.id().with("local-head-title"));
            if handle.dragged() {
                let delta = ui.input(|input| input.pointer.delta()).x;
                if delta != 0.0 {
                    // The table grows out from the middle, so twice the
                    // pointer's travel keeps the edge under it.
                    new_title_cap = Some((shape.title_cap() - delta * 2.0).clamp(200.0, 1400.0));
                }
            }
            if handle.double_clicked() {
                new_title_cap = Some(0.0);
            }
            menu_anchor = menu_anchor.union(handle);
        }
    }
    // While a heading is dragged, a line shows where it will land.
    if dragging.is_some()
        && let Some(pointer) = ui.ctx().pointer_hover_pos()
        && header.expand(20.0).contains(pointer)
    {
        let (_, mark) = drop_at(pointer.x);
        ui.painter().vline(mark, header.y_range().shrink(3.0), Stroke::new(2.0, palette.accent));
    }
    ui.painter().hline(
        header.x_range().shrink(8.0),
        header.bottom() - 0.5,
        Stroke::new(1.0, palette.outline),
    );
    // Right-click the header: which columns show, as in the playlist
    // table's columns menu. Sorting is done by clicking a heading.
    egui::Popup::context_menu(&menu_anchor)
        .frame(super::widgets::menu_frame(palette))
        .show(|ui| {
            ui.set_width(220.0);
            ui.spacing_mut().item_spacing.y = 1.0;
            for (id, (_, label, _)) in COLUMNS.iter().enumerate() {
                let id = id as u8;
                // The title always shows.
                if id == TITLE_COLUMN {
                    super::widgets::checkbox_row_ex(ui, palette, label, true, false, "");
                    continue;
                }
                let on = cols.iter().any(|(shown, _)| *shown == id);
                if super::widgets::checkbox_row(ui, palette, label, on) {
                    let mut changed = new_cols.clone().unwrap_or_else(|| cols.clone());
                    if on {
                        changed.retain(|(shown, _)| *shown != id);
                    } else {
                        // Saved in points, so a column ticked here is never
                        // taken for one from an older file.
                        changed.push((id, COLUMNS[id as usize].2));
                    }
                    new_cols = Some(changed);
                }
            }
            super::widgets::menu_separator(ui, palette);
            if super::widgets::menu_item(ui, palette, None, "reset column widths") {
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
    if let Some(cap) = new_title_cap {
        app.settings.track_columns.title_w = cap;
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
    let playing_path = app
        .deck_current_path()
        .map(|path| path.to_string_lossy().to_string());
    let deck_playing = app.deck_playing();
    let mut pending: Vec<Action> = Vec::new();
    let mut open_playlist: Option<usize> = None;
    let mut hovered: Option<String> = None;
    let mut pick: Option<(usize, egui::Modifiers)> = None;
    let picked_count = view.picked.len();
    egui::ScrollArea::vertical()
        .id_salt("local-songs-scroll")
        .auto_shrink([false, false])
        .show_rows(ui, ROW, lines.len(), |ui, range| {
            for index_in_list in range {
                let line = &lines[index_in_list];
                let (rect, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
                let is_current = line.path.is_some() && line.path == playing_path;
                let pointer_in_row = ui.rect_contains_pointer(rect);
                let picked = line
                    .entry
                    .as_ref()
                    .is_some_and(|entry| view.picked.contains(entry));
                if pointer_in_row && line.path.is_some() {
                    hovered.clone_from(&line.path);
                }
                if picked {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(6),
                        palette
                            .secondary
                            .gamma_multiply(if pointer_in_row { 0.30 } else { 0.20 }),
                    );
                } else if pointer_in_row {
                    ui.painter().rect_filled(
                        rect,
                        CornerRadius::same(6),
                        palette
                            .surface_hover
                            .gamma_multiply(if palette.dark { 0.7 } else { 1.0 }),
                    );
                }
                let painter = ui.painter().with_clip_rect(rect);
                // A small green dot: this file is also on Spotify.
                if line.spotify.is_some() {
                    let centre = pos2(rect.left() + 3.0, rect.center().y);
                    painter.circle_filled(centre, 2.0, palette.accent);
                }
                let play_from_here = |pending: &mut Vec<Action>| {
                    if let Some(at) = playable.iter().position(|path| Some(path) == line.path.as_ref()) {
                        pending.push(Action::PlayLocalFiles {
                            paths: playable.to_vec(),
                            index: at,
                            shuffle: None,
                        });
                    }
                };
                let mut used_button = false;
                let mut x = table_left(rect);
                for (at, (id, _)) in cols.iter().enumerate() {
                    let w = widths[at];
                    let id = *id as usize;
                    let cell = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
                    x += w;
                    // A faint rule at the left of each column after the
                    // song name, as in the playlist table.
                    if id > 1 && at > 0 {
                        painter.vline(
                            cell.left(),
                            rect.y_range().shrink(ROW * 0.28),
                            Stroke::new(1.0, palette.outline.gamma_multiply(0.35)),
                        );
                    }
                    // The text area, inside its padding.
                    let content = Rect::from_min_max(
                        pos2(cell.left() + PAD, rect.top()),
                        pos2((cell.right() - PAD).max(cell.left() + PAD + 1.0), rect.bottom()),
                    );
                    match id {
                        0 => {
                            // The number, or a play button under the pointer,
                            // or the bars of the song playing.
                            if pointer_in_row && line.path.is_some() {
                                let button = ui
                                    .interact(cell, ui.id().with(("local-number", index_in_list)), Sense::click())
                                    .on_hover_text("Play");
                                let tint = if button.hovered() { palette.text } else { palette.secondary };
                                theme::paint_icon(ui, Icon::PlayFilled, cell, 14.0, tint);
                                if button.clicked() {
                                    used_button = true;
                                    play_from_here(&mut pending);
                                }
                            } else if is_current && deck_playing {
                                theme::paint_icon(ui, Icon::AudioLines, cell, 16.0, palette.accent);
                            } else {
                                painter.text(
                                    cell.center(),
                                    Align2::CENTER_CENTER,
                                    &line.cells[0],
                                    theme::regular(theme::BODY_TEXT),
                                    if is_current { palette.accent } else { palette.secondary },
                                );
                            }
                        }
                        // The title.
                        1 => {
                            // The song's picture, then its name over its
                            // artist, as the playlist table draws a song.
                            let tile = Rect::from_center_size(
                                pos2(cell.left() + COVER / 2.0 + 2.0, rect.center().y),
                                Vec2::splat(COVER),
                            );
                            let url = cover_for(
                                ui,
                                &art,
                                &covers,
                                index,
                                line.path.as_deref().map(|path| (path, line.stamp)),
                                line.image.as_deref(),
                            );
                            super::widgets::paint_cover(
                                ui,
                                palette,
                                url.as_deref(),
                                tile,
                                4.0,
                                Icon::Music,
                                Some(&art),
                            );
                            let text = Rect::from_min_max(
                                pos2(cell.left() + COVER_COLUMN, rect.top()),
                                pos2((cell.right() - PAD).max(cell.left() + COVER_COLUMN + 1.0), rect.bottom()),
                            );
                            let mut name = line.cells[1].clone();
                            match line.have_file {
                                Some(true) => name = format!("✓ {name}"),
                                Some(false) => name = format!("  {name}"),
                                None => {}
                            }
                            let color = if is_current { palette.accent } else { palette.text };
                            let artist = &line.cells[2];
                            if artist_column || artist.is_empty() {
                                paint_fitted(&painter, text, Align2::LEFT_CENTER, &name, theme::medium(14.5), color);
                            } else {
                                paint_fitted(
                                    &painter,
                                    Rect::from_min_max(text.min, pos2(text.right(), rect.center().y + 1.0)),
                                    Align2::LEFT_BOTTOM,
                                    &name,
                                    theme::medium(14.5),
                                    color,
                                );
                                paint_fitted(
                                    &painter,
                                    Rect::from_min_max(pos2(text.left(), rect.center().y + 3.0), text.max),
                                    Align2::LEFT_TOP,
                                    artist,
                                    theme::regular(theme::SMALL_TEXT),
                                    palette.secondary,
                                );
                            }
                        }
                        PLAYLISTS_COLUMN => {
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
                                    .interact(
                                        tile,
                                        ui.id().with(("local-member", index_in_list, *place)),
                                        Sense::click(),
                                    )
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
                                    used_button = true;
                                    open_playlist = Some(*place);
                                }
                                left += size + gap;
                            }
                        }
                        _ => {
                            paint_fitted(
                                &painter,
                                content,
                                Align2::LEFT_CENTER,
                                &line.cells[id],
                                theme::regular(13.0),
                                palette.secondary,
                            );
                        }
                    }
                }
                // The song as Spotify would search for it.
                let search = format!("{} {}", line.cells[1], line.cells[2]).trim().to_string();
                // A small button over the end of the row: look the song up
                // on Spotify.
                if pointer_in_row && !search.is_empty() {
                    let edge = 24.0;
                    let right = (table_left(rect) + table_width).min(rect.right());
                    let search_rect = Rect::from_min_size(
                        pos2(right - edge - 4.0, rect.center().y - edge / 2.0),
                        vec2(edge, edge),
                    );
                    ui.painter().rect_filled(
                        search_rect.expand(3.0),
                        CornerRadius::same(6),
                        palette.surface_hover,
                    );
                    let button = ui
                        .interact(search_rect, ui.id().with(("local-find", index_in_list)), Sense::click())
                        .on_hover_text("Search this song on Spotify");
                    let tint = if button.hovered() { palette.text } else { palette.secondary };
                    theme::paint_icon(ui, Icon::Search, search_rect, 14.0, tint);
                    if button.clicked() {
                        used_button = true;
                        pending.push(Action::Search(search.clone()));
                    }
                }
                if response.double_clicked() && !used_button {
                    play_from_here(&mut pending);
                } else if response.clicked() && !used_button {
                    pick = Some((index_in_list, ui.input(|input| input.modifiers)));
                }
                // The songs a menu item acts on: every picked one when this
                // row is one of several picked, else this row alone.
                let several = picked && picked_count > 1;
                let targets: Vec<&Line> = if several {
                    lines
                        .iter()
                        .filter(|other| {
                            other
                                .entry
                                .as_ref()
                                .is_some_and(|entry| view.picked.contains(entry))
                        })
                        .collect()
                } else {
                    vec![line]
                };
                egui::Popup::context_menu(&response)
                    .frame(super::widgets::menu_frame(palette))
                    .show(|ui| {
                        super::widgets::fit_menu(ui, 280.0);
                        let files: Vec<String> = targets.iter().filter_map(|line| line.path.clone()).collect();
                        let entries: Vec<String> = targets.iter().filter_map(|line| line.entry.clone()).collect();
                        let count = if several { format!(" ({})", song_count(files.len())) } else { String::new() };
                        if !several
                            && line.path.is_some()
                            && super::widgets::menu_item(ui, palette, Some(Icon::Play), "Play from here")
                        {
                            play_from_here(&mut pending);
                        }
                        if !files.is_empty() {
                            if super::widgets::menu_item(ui, palette, Some(Icon::SkipForward), &format!("Play next{count}")) {
                                pending.push(Action::QueueLocalFiles {
                                    paths: files.clone(),
                                    next: true,
                                });
                            }
                            if super::widgets::menu_item(ui, palette, Some(Icon::ListEnd), &format!("Add to queue{count}")) {
                                pending.push(Action::QueueLocalFiles {
                                    paths: files.clone(),
                                    next: false,
                                });
                            }
                        }
                        if !several {
                            if let Some(path) = &line.path
                                && super::widgets::menu_item(ui, palette, Some(Icon::ExternalLink), "Show file in folder")
                                && let Err(error) = crate::opener::reveal(std::path::Path::new(path))
                            {
                                log::warn!("could not show the file in its folder: {error}");
                            }
                            if super::widgets::menu_item(ui, palette, Some(Icon::Search), "Search this song on Spotify")
                                && !search.is_empty()
                            {
                                pending.push(Action::Search(search.clone()));
                            }
                            if let Some(uri) = &line.spotify
                                && super::widgets::menu_item(ui, palette, Some(Icon::Disc), "Open it on Spotify")
                            {
                                pending.push(Action::OpenLink(uri.clone()));
                            }
                        }
                        if !entries.is_empty() {
                            super::widgets::menu_separator(ui, palette);
                            // Songs with a file can go into any local
                            // playlist, or start a new one.
                            if !files.is_empty() {
                                super::widgets::menu_submenu(
                                    ui,
                                    palette,
                                    Some(Icon::ListPlus),
                                    &format!("Add to playlist{count}"),
                                    |ui| {
                                        super::widgets::fit_menu(ui, 260.0);
                                        let mut listed = false;
                                        for (place, name) in local_names.iter().enumerate() {
                                            if Some(place) == open_local {
                                                continue;
                                            }
                                            listed = true;
                                            let already = targets.iter().all(|line| line.playlists.contains(&place));
                                            if super::widgets::menu_item_enabled(
                                                ui,
                                                palette,
                                                Some(if already { Icon::Check } else { Icon::ListMusic }),
                                                name,
                                                !already,
                                            ) {
                                                pending.push(Action::AddToLocalPlaylist {
                                                    playlist: place,
                                                    paths: files.clone(),
                                                });
                                            }
                                        }
                                        if listed {
                                            super::widgets::menu_separator(ui, palette);
                                        }
                                        if super::widgets::menu_item(ui, palette, Some(Icon::Plus), "New playlist") {
                                            pending.push(Action::CreateLocalPlaylist {
                                                name: format!("Playlist {}", local_names.len() + 1),
                                                paths: files.clone(),
                                            });
                                        }
                                    },
                                );
                            }
                            if let Some(place) = open_local
                                && super::widgets::menu_item(
                                    ui,
                                    palette,
                                    Some(Icon::Minus),
                                    &format!("Remove from this playlist{count}"),
                                )
                            {
                                pending.push(Action::RemoveFromLocalPlaylist {
                                    playlist: place,
                                    paths: entries.clone(),
                                });
                            }
                        }
                    });
            }
        });
    // A click picks the song; Ctrl+click adds it to (or takes it from) the
    // picked ones; Shift+click picks every song from the last one clicked.
    if let Some((at, modifiers)) = pick
        && let Some(entry) = lines[at].entry.clone()
    {
        if modifiers.shift
            && let Some(from) = view
                .anchor
                .as_ref()
                .and_then(|anchor| lines.iter().position(|line| line.entry.as_ref() == Some(anchor)))
        {
            let (low, high) = if from <= at { (from, at) } else { (at, from) };
            view.picked = lines[low..=high]
                .iter()
                .filter_map(|line| line.entry.clone())
                .collect();
        } else if modifiers.command {
            if let Some(found) = view.picked.iter().position(|picked| *picked == entry) {
                view.picked.remove(found);
            } else {
                view.picked.push(entry.clone());
            }
            view.anchor = Some(entry);
        } else {
            view.picked = vec![entry.clone()];
            view.anchor = Some(entry);
        }
    }
    if let Some(path) = &hovered {
        app.hovered_local_file = Some(path.clone());
    }
    view.hovered = hovered;
    app.actions.extend(pending);
    if let Some(place) = open_playlist {
        view.open = Open::Local(place);
    }
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

/// One playlist on the Playlists view.
struct Tile {
    open: Open,
    name: String,
    count: usize,
    /// The picture: the first song's own, or its Spotify album picture.
    file: Option<(String, (u64, u64))>,
    image: Option<String>,
}

/// The list of playlists: a field to make a new one, then the ones made
/// here and the ones imported from Exportify, each as a grid of covers with
/// names, as the library on the left shows playlists in its grid view. The
/// search at the top finds a playlist by name.
fn playlists_page(app: &mut App, ui: &mut egui::Ui, palette: &Palette, view: &mut View) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        let field = ui.add(
            egui::TextEdit::singleline(&mut view.new_name)
                .hint_text("New playlist name")
                .desired_width(220.0)
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
    let needle = app.local_ui.filter.trim().to_lowercase();
    let by_path: HashMap<&str, &LocalSong> = app
        .local_songs
        .songs
        .iter()
        .map(|song| (song.path.as_str(), song))
        .collect();
    let wanted = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);
    let local: Vec<Tile> = app
        .settings
        .local_playlists
        .iter()
        .enumerate()
        .filter(|(_, playlist)| wanted(&playlist.name))
        .map(|(at, playlist)| {
            let first = playlist.paths.iter().find_map(|path| by_path.get(path.as_str()));
            // The first song's Spotify album picture when it is matched,
            // else the picture inside its file.
            let image = first
                .and_then(|song| app.local_matches.uri_of(&song.path))
                .and_then(|uri| spotify_image(app, uri));
            Tile {
                open: Open::Local(at),
                name: playlist.name.clone(),
                count: playlist.paths.len(),
                file: first.map(|song| (song.path.clone(), (song.size, song.modified))),
                image,
            }
        })
        .collect();
    let imported: Vec<Tile> = app
        .offline_playlists
        .iter()
        .enumerate()
        .filter(|(_, playlist)| wanted(&playlist.name))
        .map(|(at, playlist)| Tile {
            open: Open::Imported(at),
            name: playlist.name.clone(),
            count: playlist.tracks.len(),
            file: None,
            image: playlist
                .tracks
                .iter()
                .take(4)
                .find_map(|track| spotify_image(app, &track.uri)),
        })
        .collect();
    if local.is_empty() && imported.is_empty() {
        super::widgets::empty_state(ui, palette, Icon::Search, "Nothing here", "No playlists match your search.");
        return;
    }
    let art = app.backend.art().clone();
    let covers = app.local_covers.clone();
    let index = app.dirs.index_dir();
    let mut pending: Vec<Action> = Vec::new();
    let mut play: Option<Open> = None;
    egui::ScrollArea::vertical()
        .id_salt("local-playlists-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();
            let columns = ((width / TILE_TARGET).floor() as usize).clamp(2, 8);
            let card = (width / columns as f32).floor().max(1.0);
            for (heading, tiles) in [("Your playlists", &local), ("Imported from Exportify", &imported)] {
                if tiles.is_empty() {
                    continue;
                }
                super::widgets::menu_group(ui, palette, heading);
                for row in tiles.chunks(columns) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for tile in row {
                            let url = cover_for(
                                ui,
                                &art,
                                &covers,
                                &index,
                                tile.file.as_ref().map(|(path, stamp)| (path.as_str(), *stamp)),
                                tile.image.as_deref(),
                            );
                            let (response, play_clicked) = playlist_tile(ui, palette, tile, url.as_deref(), &art, card);
                            if play_clicked {
                                play = Some(tile.open);
                            } else if response.clicked() {
                                view.open = tile.open;
                            }
                            egui::Popup::context_menu(&response)
                                .frame(super::widgets::menu_frame(palette))
                                .show(|ui| {
                                    super::widgets::fit_menu(ui, 260.0);
                                    if super::widgets::menu_item(ui, palette, Some(Icon::ListMusic), "Open") {
                                        view.open = tile.open;
                                    }
                                    if super::widgets::menu_item(ui, palette, Some(Icon::Play), "Play") {
                                        play = Some(tile.open);
                                    }
                                    if let Open::Local(at) = tile.open {
                                        let files = playable_files(app, tile.open);
                                        if super::widgets::menu_item_enabled(
                                            ui,
                                            palette,
                                            Some(Icon::ListEnd),
                                            "Add to queue",
                                            !files.is_empty(),
                                        ) {
                                            pending.push(Action::QueueLocalFiles { paths: files, next: false });
                                        }
                                        if super::widgets::menu_item(ui, palette, Some(Icon::Pencil), "Rename") {
                                            view.open = Open::Local(at);
                                            view.renaming = Some((at, tile.name.clone()));
                                            view.focus_rename = true;
                                        }
                                        super::widgets::menu_separator(ui, palette);
                                        if super::widgets::menu_item(ui, palette, Some(Icon::Trash), "Delete playlist") {
                                            pending.push(Action::DeleteLocalPlaylist(at));
                                        }
                                    }
                                });
                        }
                    });
                }
                ui.add_space(12.0);
            }
        });
    if let Some(open) = play {
        let paths = playable_files(app, open);
        if paths.is_empty() {
            app.toast("None of this playlist's songs has a file here.");
        } else {
            pending.push(Action::PlayLocalFiles {
                paths,
                index: 0,
                shuffle: Some(false),
            });
        }
    }
    app.actions.extend(pending);
}

/// The files a playlist can play, in its order: a local playlist's files
/// that are still in the library, or the songs of an imported playlist that
/// have a file here.
fn playable_files(app: &App, open: Open) -> Vec<String> {
    match open {
        Open::Local(at) => {
            let library: std::collections::HashSet<&str> =
                app.local_songs.songs.iter().map(|song| song.path.as_str()).collect();
            app.settings
                .local_playlists
                .get(at)
                .map(|playlist| {
                    playlist
                        .paths
                        .iter()
                        .filter(|path| library.contains(path.as_str()))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        }
        Open::Imported(at) => app
            .offline_playlists
            .get(at)
            .map(|playlist| {
                playlist
                    .tracks
                    .iter()
                    .filter_map(|track| app.local_songs.find_match(&track.name, &track.artists))
                    .map(|song| song.path.clone())
                    .collect()
            })
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// One playlist in the grid, drawn as a card in the library's grid view
/// is: the cover, the name under it and how many songs it holds, with the
/// round play button on the cover under the pointer. Answers the card, and
/// whether the play button was the one clicked.
fn playlist_tile(
    ui: &mut egui::Ui,
    palette: &Palette,
    tile: &Tile,
    url: Option<&str>,
    art: &crate::images::ArtLoader,
    card: f32,
) -> (egui::Response, bool) {
    let padding = 8.0;
    let base = super::sidebar::grid_name_size(card).min(16.0);
    let line = base * 1.34;
    let (rect, response) = ui.allocate_exact_size(vec2(card, card + line * 2.0 + 8.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let cover = Rect::from_min_size(rect.min + Vec2::splat(padding), Vec2::splat(card - padding * 2.0));
    let size = (cover.width() * 0.3).clamp(32.0, 44.0);
    let inset = padding + size / 2.0;
    let button = Rect::from_center_size(
        pos2(cover.right() - inset, cover.bottom() - inset),
        Vec2::splat(size),
    );
    let play = ui
        .interact(button, response.id.with("play"), Sense::click())
        .on_hover_text("Play");
    if ui.is_rect_visible(rect) {
        if response.hovered() || play.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(6), palette.surface_hover.gamma_multiply(0.6));
        }
        if url.is_some() {
            super::widgets::paint_cover(ui, palette, url, cover, 6.0, Icon::Music, Some(art));
        } else {
            super::widgets::paint_cover(ui, palette, None, cover, 6.0, Icon::ListMusic, None);
        }
        if response.hovered() || play.hovered() {
            let fill = if play.hovered() { palette.accent_hover } else { palette.accent };
            ui.painter().add(
                egui::epaint::Shadow {
                    offset: [0, 4],
                    blur: 12,
                    spread: 0,
                    color: Color32::from_black_alpha(90),
                }
                .as_shape(button, CornerRadius::same(127)),
            );
            ui.painter().circle_filled(button.center(), button.width() / 2.0, fill);
            let size = button.width() * 0.42;
            Icon::PlayFilled.image(palette.on_accent, size).paint_at(
                ui,
                Rect::from_center_size(
                    button.center() + theme::play_glyph_offset(Icon::PlayFilled, size),
                    Vec2::splat(size),
                ),
            );
        }
        let text_left = rect.left() + padding;
        let text_width = (rect.width() - padding * 2.0).max(1.0);
        let painter = ui.painter();
        paint_fitted(
            painter,
            Rect::from_min_size(pos2(text_left, cover.bottom() + 4.0), vec2(text_width, line)),
            Align2::LEFT_CENTER,
            &tile.name,
            theme::medium(base),
            palette.text,
        );
        let detail = match tile.open {
            Open::Imported(_) => format!("{} · imported", song_count(tile.count)),
            _ => song_count(tile.count),
        };
        paint_fitted(
            painter,
            Rect::from_min_size(pos2(text_left, cover.bottom() + 4.0 + line), vec2(text_width, line)),
            Align2::LEFT_CENTER,
            &detail,
            theme::regular(base * 0.89),
            palette.secondary,
        );
    }
    let played = play.clicked();
    (response, played)
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
    fn columns_keep_their_widths_and_the_title_takes_the_rest() {
        // number, title, album (dragged to 200), bpm.
        let wanted = [44.0, 0.0, 200.0, 52.0];
        let mins = [44.0, 152.0, 76.0, 40.0];
        let (widths, shift) = lay_out(&wanted, &mins, 1, 1000.0, 2000.0, true);
        assert_eq!(shift, 0.0);
        assert_eq!(widths, vec![44.0, 704.0, 200.0, 52.0]);
    }

    #[test]
    fn a_wide_table_sits_in_the_middle_with_the_title_capped() {
        let wanted = [44.0, 0.0, 200.0, 52.0];
        let mins = [44.0, 152.0, 76.0, 40.0];
        let (widths, shift) = lay_out(&wanted, &mins, 1, 1000.0, 380.0, true);
        assert_eq!(widths[1], 380.0);
        assert!((shift - 162.0).abs() < 0.01);
        // Kept to the left, the title takes it all.
        let (widths, shift) = lay_out(&wanted, &mins, 1, 1000.0, 380.0, false);
        assert_eq!(shift, 0.0);
        assert_eq!(widths[1], 704.0);
    }

    #[test]
    fn a_narrow_table_squeezes_the_columns_before_the_title() {
        let wanted = [44.0, 0.0, 200.0, 52.0];
        let mins = [44.0, 152.0, 76.0, 40.0];
        let (widths, _) = lay_out(&wanted, &mins, 1, 400.0, 380.0, true);
        assert!((widths.iter().sum::<f32>() - 400.0).abs() < 0.01);
        assert_eq!(widths[1], 152.0);
        for (width, min) in widths.iter().zip(mins) {
            assert!(*width >= min - 0.01, "{width} under {min}");
        }
        // Too narrow for even the minimums: nothing goes under them.
        let (widths, _) = lay_out(&wanted, &mins, 1, 100.0, 380.0, true);
        for (width, min) in widths.iter().zip(mins) {
            assert!(*width >= min - 0.01, "{width} under {min}");
        }
    }

    #[test]
    fn a_dragged_column_stops_at_its_minimum() {
        assert_eq!(dragged_width(100.0, 20.0, 60.0), 120.0);
        assert_eq!(dragged_width(100.0, -80.0, 60.0), 60.0);
        assert_eq!(dragged_width(600.0, 100.0, 60.0), MAX_WIDTH);
        // An old share (1 or less) means the first width.
        assert_eq!(wanted_width(3, 0.22, 70.0), 160.0);
        assert_eq!(wanted_width(3, 230.0, 70.0), 230.0);
        assert_eq!(wanted_width(0, 300.0, 44.0), NUMBER_WIDTH);
    }
}
