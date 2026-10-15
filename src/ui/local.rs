//! The Local songs page: audio files in the index's songs folder, with the
//! tempo, key and waveform measured from the sound, and playlists imported
//! from Exportify files that can be browsed with no internet.

use egui::{Align2, Color32, CornerRadius, Rect, Sense, Stroke, pos2, vec2};

use crate::app::App;
use crate::model::Action;
use crate::theme::{self, Icon};

const ROW: f32 = 34.0;

/// One line of whichever table is open, already turned into text.
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
    /// Whether that song is in the account's Liked Songs.
    liked: bool,
}

const HEADINGS: [&str; 10] =
    ["#", "TITLE", "ARTIST", "PLAYLIST", "ALBUM", "ADDED", "RELEASED", "BPM", "LENGTH", "LIKED"];
const SHARES: [f32; 10] = [0.04, 0.20, 0.15, 0.13, 0.14, 0.08, 0.07, 0.05, 0.06, 0.05];

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
    theme::text(ui, "Local songs", theme::bold(28.0), palette.text);
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
            // Files that are songs Spotify no longer lets you play say which
            // playlist they were taken out of.
            let user = app.user.as_ref().map(|user| user.id.clone()).unwrap_or_default();
            let first = |artists: &str| artists.split([',', ';', '&']).next().unwrap_or("").trim().to_string();
            let was_in: std::collections::HashMap<String, String> = app
                .unavailable
                .entries_for(&user)
                .iter()
                .map(|entry| {
                    (
                        crate::exportify::match_key(&entry.title, &first(&entry.artists)),
                        entry.playlist_name.clone(),
                    )
                })
                .collect();
            for (number, song) in app.local_songs.songs.iter().enumerate() {
                let was = was_in
                    .get(&crate::exportify::match_key(&song.title, &first(&song.artist)))
                    .cloned();
                let liked = app
                    .local_matches
                    .uri_of(&song.path)
                    .is_some_and(|uri| app.is_saved(uri).unwrap_or(false));
                lines.push(Line {
                    cells: [
                        format!("{}", number + 1),
                        if let Some(error) = &song.error {
                            format!("{} - {error}", song.title)
                        } else {
                            song.title.clone()
                        },
                        song.artist.clone(),
                        was.unwrap_or_default(),
                        song.album.clone(),
                        day_text(song.modified),
                        song.year.clone(),
                        song.bpm.map_or(String::new(), |b| format!("{}", b.round())),
                        length(song.duration_ms),
                        if liked { "♥".to_string() } else { String::new() },
                    ],
                    bpm: song.bpm,
                    length_ms: song.duration_ms,
                    have_file: None,
                    path: Some(song.path.clone()),
                    spotify: app.local_matches.uri_of(&song.path).map(str::to_string),
                    liked,
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
                        playlist.name.clone(),
                        track.album.clone(),
                        String::new(),
                        String::new(),
                        track.tempo.map_or(String::new(), |t| format!("{}", t.round())),
                        length(track.duration_ms),
                        String::new(),
                    ],
                    bpm: track.tempo,
                    length_ms: track.duration_ms,
                    have_file: Some(have),
                    spotify: None,
                    liked: false,
                    path,
                });
            }
        }
    }
    if !needle.is_empty() {
        lines.retain(|line| line.cells.iter().any(|c| c.to_lowercase().contains(&needle)));
    }
    let sort = app.local_ui.sort.min(9) as usize;
    let descending = app.local_ui.descending;
    lines.sort_by(|a, b| {
        let order = match sort {
            0 => a
                .cells[0]
                .parse::<u32>()
                .unwrap_or(0)
                .cmp(&b.cells[0].parse::<u32>().unwrap_or(0)),
            7 => a
                .bpm
                .unwrap_or(f32::MAX)
                .total_cmp(&b.bpm.unwrap_or(f32::MAX)),
            8 => a.length_ms.cmp(&b.length_ms),
            9 => b.liked.cmp(&a.liked),
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

    // Header.
    let width = ui.available_width();
    let (header, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
    let mut x = header.left() + 8.0;
    let mut clicked_sort: Option<u8> = None;
    for (i, name) in HEADINGS.iter().enumerate() {
        let w = (width - 16.0) * SHARES[i];
        let cell = Rect::from_min_size(pos2(x, header.top()), vec2(w, header.height()));
        let response = ui.interact(cell, ui.id().with(("local-head", i)), Sense::click());
        let active = sort == i;
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
        ui.painter().text(
            pos2(cell.left(), cell.center().y),
            Align2::LEFT_CENTER,
            format!("{name}{arrow}"),
            theme::regular(12.0),
            color,
        );
        if response.clicked() {
            clicked_sort = Some(i as u8);
        }
        x += w;
    }
    ui.painter()
        .hline(header.x_range().shrink(8.0), header.bottom() - 0.5, Stroke::new(1.0, palette.outline));
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
                // A small green dot: this file is also on Spotify (a ring
                // heart-coloured when it is in Liked Songs).
                if line.spotify.is_some() {
                    let centre = pos2(rect.left() + 4.0, rect.center().y);
                    painter.circle_filled(centre, 2.5, palette.accent);
                    if line.liked {
                        painter.circle_stroke(
                            centre,
                            4.0,
                            Stroke::new(1.0, Color32::from_rgb(0xff, 0x5c, 0x7a)),
                        );
                    }
                }
                let mut x = rect.left() + 8.0;
                for (i, cell) in line.cells.iter().enumerate() {
                    let w = (width - 16.0) * SHARES[i];
                    let clip = Rect::from_min_size(pos2(x, rect.top()), vec2(w - 8.0, ROW));
                    let mut text = cell.clone();
                    if i == 1 {
                        match line.have_file {
                            Some(true) => text = format!("✓ {text}"),
                            Some(false) => text = format!("  {text}"),
                            None => {}
                        }
                    }
                    let color = if i == 1 && is_playing {
                        palette.accent
                    } else if i == 1 {
                        palette.text
                    } else {
                        palette.secondary
                    };
                    painter.with_clip_rect(clip).text(
                        pos2(clip.left(), rect.center().y),
                        Align2::LEFT_CENTER,
                        text,
                        theme::regular(13.0),
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
                response.context_menu(|ui| {
                    if line.path.is_some() && ui.button("Play from here").clicked() {
                        play_from_here(&mut pending);
                        ui.close();
                    }
                    if let Some(path) = &line.path
                        && ui.button("Show file in folder").clicked()
                    {
                        if let Err(error) = crate::opener::reveal(std::path::Path::new(path)) {
                            log::warn!("could not show the file in its folder: {error}");
                        }
                        ui.close();
                    }
                    if ui.button("Search this song on Spotify").clicked() && !search.is_empty() {
                        pending.push(Action::Search(search.clone()));
                        ui.close();
                    }
                    if let Some(uri) = &line.spotify {
                        let label = if line.liked {
                            "Remove from Liked Songs on Spotify"
                        } else {
                            "Like on Spotify"
                        };
                        if ui.button(label).clicked() {
                            pending.push(Action::ToggleSaved(uri.clone()));
                            ui.close();
                        }
                        if ui.button("Open its album on Spotify").clicked() {
                            pending.push(Action::OpenLink(uri.clone()));
                            ui.close();
                        }
                    }
                });
            }
        });
    app.actions.extend(pending);
}
