//! The Home page.

use std::sync::Arc;

use egui::{CornerRadius, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{Episode, PlayableItem, Playlist, Show, pick_image};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, DISCOVER_TERMS, Loadable, Page, RowContext};
use crate::theme::{self, Icon};

use super::widgets::{self, TrackRow};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(6.0);
    let greeting = crate::util::greeting(app.locale);
    theme::text(ui, greeting.as_ref(), theme::bold(30.0), palette.text);
    ui.add_space(10.0);
    let tab = app.settings.home.tab.min(1);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        for (index, label) in ["For you", "Music stats"].into_iter().enumerate() {
            if theme::pill_button(ui, &palette, &gettext(app.locale, label), tab as usize == index)
                .clicked()
                && tab as usize != index
            {
                app.settings.home.tab = index as u8;
                app.mark_settings_dirty();
            }
        }
    });
    ui.add_space(14.0);
    if tab == 0 {
        if app.settings.home.made_for_you.visible {
            made_for_you(app, ui);
        }
        new_releases(app, ui);
        podcasts(app, ui);
        if app.settings.home.recommendations.visible {
            recommendations(app, ui);
        }
    } else {
        stats_section(app, ui);
        top_artists(app, ui);
        top_tracks(app, ui);
        recently_played(app, ui);
    }
}

/// Albums and singles from the last weeks by the artists in the playlists.
fn new_releases(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let user = app.user_id().map(str::to_string).unwrap_or_default();
    let scanning = app
        .releases_scan
        .as_ref()
        .map(|scan| (scan.total.saturating_sub(scan.queue.len()), scan.total));
    let releases: Vec<crate::releases::Release> = if app.releases.user == user {
        app.releases.found.clone()
    } else {
        Vec::new()
    };
    let checked = app.releases.checked_at.is_some() && app.releases.user == user;
    if releases.is_empty() && !checked && scanning.is_none() && app.offline {
        return;
    }
    let folded = app.settings.home.releases_folded;
    ui.horizontal(|ui| {
        if theme::icon_button(
            ui,
            if folded { Icon::ChevronRight } else { Icon::ChevronDown },
            16.0,
            palette.text,
            palette.accent,
            if folded { "Show the list" } else { "Fold the list away" },
        )
        .clicked()
        {
            app.settings.home.releases_folded = !folded;
            app.mark_settings_dirty();
        }
        theme::section_title(ui, &palette, &gettext(app.locale, "New from your artists"));
        if folded && !releases.is_empty() {
            theme::subtle(ui, &palette, &format!("{} new", releases.len()));
        }
        match scanning {
            Some((done, total)) => {
                theme::subtle(ui, &palette, &format!("Checking {done} of {total}…"));
            }
            None => {
                if theme::pill_button(ui, &palette, &gettext(app.locale, "Check now"), false).clicked() {
                    app.actions.push(Action::CheckNewReleases(None));
                }
            }
        }
    });
    if folded {
        ui.add_space(10.0);
        return;
    }
    if let Some(name) = app.releases.scope.clone().filter(|_| checked) {
        theme::subtle(ui, &palette, &format!("From the artists in {name}"));
    }
    if releases.is_empty() {
        if scanning.is_none() {
            theme::subtle(
                ui,
                &palette,
                &gettext(
                    app.locale,
                    "Nothing new in the last 3 months. Press Check now to look at the artists in your playlists.",
                ),
            );
        }
        ui.add_space(14.0);
        return;
    }
    ui.add_space(4.0);
    // One numbered list, the newest first, as far back as the check goes
    // (about three months). It scrolls inside its own box.
    let mut shown: Vec<_> = releases.iter().collect();
    shown.sort_by(|a, b| b.date.cmp(&a.date));
    egui::ScrollArea::vertical()
        .id_salt("new-releases-list")
        .max_height(440.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            for (index, release) in shown.iter().enumerate() {
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 60.0), Sense::click());
                if !ui.is_rect_visible(rect) {
                    continue;
                }
                let hovered = ui.rect_contains_pointer(rect);
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(6),
                    if hovered { palette.surface_hover } else { palette.surface },
                );
                let number = Rect::from_min_size(rect.min, vec2(40.0, 60.0));
                ui.painter().text(
                    number.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("{}", index + 1),
                    theme::semibold(13.0),
                    palette.dim,
                );
                let cover = Rect::from_min_size(pos2(rect.left() + 40.0, rect.top()), Vec2::splat(60.0));
                widgets::paint_cover(
                    ui,
                    &palette,
                    release.image.as_deref(),
                    cover,
                    6.0,
                    Icon::Music,
                    Some(app.backend.art()),
                );
                let text = Rect::from_min_max(
                    pos2(cover.right() + 12.0, rect.top()),
                    pos2(rect.right() - 12.0, rect.bottom()),
                );
                let painter = ui.painter().with_clip_rect(text);
                crate::bidi::paint_line(
                    &painter,
                    text.left(),
                    text.right(),
                    rect.center().y - 10.0,
                    &release.name,
                    theme::bold(14.0),
                    palette.text,
                );
                crate::bidi::paint_line(
                    &painter,
                    text.left(),
                    text.right(),
                    rect.center().y + 10.0,
                    &format!(
                        "{} · {} · {}",
                        release.artists,
                        if release.kind == "single" { "Single" } else { "Album" },
                        release.date
                    ),
                    theme::regular(12.0),
                    palette.secondary,
                );
                if response.clicked() {
                    app.actions.push(Action::Open(Page::Album(release.album_id.clone())));
                }
            }
        });
    ui.add_space(16.0);
}

/// What has been heard, from the long listening log in the chanceify folder.
fn stats_section(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    theme::section_title(ui, &palette, &gettext(app.locale, "Your listening"));
    ui.add_space(6.0);
    if app.stats.is_empty() {
        theme::subtle(
            ui,
            &palette,
            &gettext(
                app.locale,
                "Nothing counted yet. Songs you listen to for a while are written down here, on this computer only.",
            ),
        );
        ui.add_space(16.0);
        return;
    }
    let now = jiff::Timestamp::now().as_second();
    let summary = app.stats.summary(now, crate::stats::local_offset());
    let hours = |minutes: u64| {
        if minutes >= 120 {
            format!("{} hours", minutes / 60)
        } else {
            format!("{minutes} minutes")
        }
    };
    ui.horizontal(|ui| {
        theme::text(
            ui,
            &format!("{}:", gettext(app.locale, "Last 30 days")),
            theme::semibold(14.0),
            palette.text,
        );
        theme::text(
            ui,
            &format!("{} songs, {}", summary.month, hours(summary.minutes_month)),
            theme::regular(14.0),
            palette.secondary,
        );
    });
    if summary.streak_days > 1 {
        theme::subtle(ui, &palette, &format!("{} days in a row", summary.streak_days));
    }
    ui.add_space(6.0);
    if theme::pill_button(ui, &palette, &gettext(app.locale, "Make a recap picture"), false).clicked() {
        app.actions.push(Action::OpenRecap);
    }
    ui.add_space(10.0);
    // The last two weeks, one bar a day.
    let top = summary.per_day.iter().copied().max().unwrap_or(0).max(1) as f32;
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width().min(520.0), 84.0), Sense::hover());
    let slot = rect.width() / 14.0;
    for (index, count) in summary.per_day.iter().enumerate() {
        let height = (rect.height() - 16.0) * (*count as f32 / top);
        let bar = Rect::from_min_max(
            pos2(rect.left() + slot * index as f32 + slot * 0.18, rect.bottom() - 14.0 - height),
            pos2(rect.left() + slot * (index as f32 + 1.0) - slot * 0.18, rect.bottom() - 14.0),
        );
        ui.painter().rect_filled(
            bar,
            CornerRadius::same(3),
            if *count == 0 { palette.surface } else { palette.accent },
        );
        if index == 0 || index == 13 {
            ui.painter().text(
                pos2(bar.center().x, rect.bottom()),
                egui::Align2::CENTER_BOTTOM,
                if index == 13 { "today" } else { "14 days ago" },
                theme::regular(10.5),
                palette.dim,
            );
        }
    }
    response.on_hover_text(gettext(app.locale, "Songs played each day, the last two weeks").to_string());
    ui.add_space(12.0);
    for songs in [false, true] {
        ranked_shelf(app, ui, songs, now);
    }
    ui.add_space(8.0);
}

/// "Your artists" or "Your songs": a row that scrolls sideways, for the last
/// 7 days, the last 30 days or all time.
fn ranked_shelf(app: &mut App, ui: &mut egui::Ui, songs: bool, now: i64) {
    let palette = app.palette;
    let saved = if songs { app.settings.home.song_range } else { app.settings.home.artist_range };
    let range = crate::stats::Range::from_saved(saved);
    let title = if songs { "Your songs" } else { "Your artists" };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        theme::section_title(ui, &palette, &gettext(app.locale, title));
        for (label, choice) in [
            ("Last 7 days", crate::stats::Range::Week),
            ("Last 30 days", crate::stats::Range::Month),
            ("All time", crate::stats::Range::Ever),
        ] {
            if theme::pill_button(ui, &palette, &gettext(app.locale, label), range == choice).clicked()
                && range != choice
            {
                if songs {
                    app.settings.home.song_range = choice.saved();
                } else {
                    app.settings.home.artist_range = choice.saved();
                }
                app.mark_settings_dirty();
            }
        }
    });
    ui.add_space(4.0);
    let (artists, tracks) = app.stats.ranking(now, crate::stats::local_offset(), range);
    let rows = if songs { tracks } else { artists };
    if rows.is_empty() {
        theme::subtle(ui, &palette, &gettext(app.locale, "Nothing in this time yet."));
        ui.add_space(12.0);
        return;
    }
    // A grid that fills the page's width, no sideways scrolling.
    let gap = 10.0;
    let available = ui.available_width();
    let columns = ((available / 260.0).floor() as usize).clamp(1, 6);
    let width = (available - gap * (columns as f32 - 1.0)) / columns as f32;
    let mut index = 0;
    for chunk in rows.chunks(columns) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for row in chunk {
                stat_tile(app, ui, row, songs, index, width);
                index += 1;
            }
        });
    }
    ui.add_space(14.0);
}

/// One ranked artist or song. Clicking it opens the artist or plays the
/// song; clicking the artist's name under a song opens that artist.
fn stat_tile(
    app: &mut App,
    ui: &mut egui::Ui,
    row: &crate::stats::Ranked,
    song: bool,
    index: usize,
    width: f32,
) {
    let palette = app.palette;
    let (rect, response) = ui.allocate_exact_size(vec2(width, 60.0), Sense::click());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let image = app.stat_picture(&row.uri, &row.artist_id, song);
    let hovered = response.hovered();
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(6),
        if hovered { palette.surface_hover } else { palette.surface },
    );
    let badge = Rect::from_min_size(rect.min, Vec2::splat(60.0));
    widgets::paint_cover(
        ui,
        &palette,
        image.as_deref(),
        badge,
        6.0,
        if song { Icon::Music } else { Icon::Headphones },
        Some(app.backend.art()),
    );
    // The rank, on a small dark chip so it reads over any picture.
    let chip = Rect::from_min_size(badge.min + vec2(3.0, 3.0), vec2(20.0, 16.0));
    ui.painter()
        .rect_filled(chip, CornerRadius::same(4), egui::Color32::from_black_alpha(150));
    ui.painter().text(
        chip.center(),
        egui::Align2::CENTER_CENTER,
        format!("{}", index + 1),
        theme::semibold(11.0),
        egui::Color32::WHITE,
    );
    let text = Rect::from_min_max(
        pos2(badge.right() + 10.0, rect.top()),
        pos2(rect.right() - 10.0, rect.bottom()),
    );
    let painter = ui.painter().with_clip_rect(text);
    crate::bidi::paint_line(
        &painter,
        text.left(),
        text.right(),
        rect.center().y - 11.0,
        &row.name,
        theme::bold(13.5),
        palette.text,
    );
    if song && !row.uri.is_empty() && response.hovered() {
        app.hovered_track = Some(row.uri.clone());
    }
    let plays = if row.plays == 1 { "1 play".to_string() } else { format!("{} plays", row.plays) };
    let mut artist_clicked = false;
    if song {
        let sub_rect = crate::bidi::paint_line(
            &painter,
            text.left(),
            text.right(),
            rect.center().y + 3.0,
            &row.sub,
            theme::regular(12.0),
            if row.artist_id.is_empty() { palette.secondary } else { palette.text },
        );
        if !row.artist_id.is_empty() {
            let area = sub_rect.intersect(text).expand2(vec2(0.0, 2.0));
            let link = ui.interact(area, ui.id().with(("stat-artist", index, &row.uri)), Sense::click());
            if link.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                painter.line_segment(
                    [pos2(area.left(), area.bottom() - 1.0), pos2(area.right(), area.bottom() - 1.0)],
                    egui::Stroke::new(1.0, palette.text),
                );
            }
            artist_clicked = link.clicked();
        }
        crate::bidi::paint_line(
            &painter,
            text.left(),
            text.right(),
            rect.center().y + 19.0,
            &plays,
            theme::regular(11.5),
            palette.dim,
        );
    } else {
        crate::bidi::paint_line(
            &painter,
            text.left(),
            text.right(),
            rect.center().y + 9.0,
            &plays,
            theme::regular(12.0),
            palette.secondary,
        );
    }
    if artist_clicked {
        app.actions.push(Action::Open(Page::Artist(row.artist_id.clone())));
    } else if response.clicked() && song && !row.uri.is_empty() && super::widgets::queue_click_held(app, ui) {
        app.actions.push(Action::AddToQueue {
            uri: row.uri.clone(),
            label: row.name.clone(),
        });
    } else if response.clicked() {
        if song {
            if !row.uri.is_empty() {
                app.actions.push(Action::PlayUris {
                    uris: vec![row.uri.clone()],
                    index: 0,
                });
            }
        } else if !row.artist_id.is_empty() {
            app.actions.push(Action::Open(Page::Artist(row.artist_id.clone())));
        } else {
            app.actions.push(Action::Search(row.name.clone()));
        }
    }
    response.on_hover_text(if song {
        "Click to play. Click the artist to open them."
    } else {
        "Click to open this artist."
    });
}

struct Tile {
    image: Option<String>,
    name: String,
    page: Page,
    uri: Option<String>,
    liked: bool,
    owned_playlist: Option<Playlist>,
}

#[allow(dead_code)]
fn quick_access(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let mut tiles: Vec<Tile> = vec![Tile {
        image: None,
        name: gettext(app.locale, "Liked Songs").into_owned(),
        page: Page::LikedSongs,
        uri: app
            .user
            .as_ref()
            .map(|user| format!("spotify:user:{}:collection", user.id)),
        liked: true,
        owned_playlist: None,
    }];
    if let Some(playlists) = app.library.playlists.get() {
        for playlist in playlists.iter().take(7) {
            tiles.push(Tile {
                image: pick_image(&playlist.images, 64).map(str::to_string),
                name: playlist.name.clone(),
                page: Page::Playlist(playlist.id.clone()),
                uri: Some(playlist.uri.clone()),
                liked: false,
                owned_playlist: app
                    .user_id()
                    .is_some_and(|id| playlist.owned_by(id))
                    .then(|| playlist.clone()),
            });
        }
    }
    let available = ui.available_width();
    let columns = ((available / 300.0).floor() as usize).clamp(2, 4);
    let gap = 10.0;
    let tile_width = (available - gap * (columns as f32 - 1.0)) / columns as f32;
    let rows = tiles.len().div_ceil(columns);
    for row in 0..rows {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for column in 0..columns {
                let Some(Tile {
                    image,
                    name,
                    page,
                    uri,
                    liked,
                    owned_playlist,
                }) = tiles.get(row * columns + column)
                else {
                    break;
                };
                let (rect, response) =
                    ui.allocate_exact_size(vec2(tile_width, 60.0), Sense::click());
                if ui.is_rect_visible(rect) {
                    let hovered = ui.rect_contains_pointer(rect);
                    let fill = if hovered {
                        palette.surface_hover
                    } else {
                        palette.surface
                    };
                    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                    let cover = Rect::from_min_size(rect.min, Vec2::splat(60.0));
                    if *liked {
                        super::sidebar::liked_cover(ui, cover, 6.0);
                    } else {
                        widgets::paint_cover(
                            ui,
                            &palette,
                            image.as_deref(),
                            cover,
                            6.0,
                            Icon::Music,
                            Some(app.backend.art()),
                        );
                    }
                    let play_room = if hovered && uri.is_some() { 52.0 } else { 12.0 };
                    let text_rect = Rect::from_min_max(
                        pos2(cover.right() + 12.0, rect.top()),
                        pos2(rect.right() - play_room, rect.bottom()),
                    );
                    crate::bidi::paint_line(
                        &ui.painter().with_clip_rect(text_rect),
                        text_rect.left(),
                        text_rect.right(),
                        rect.center().y,
                        name,
                        theme::bold(14.5),
                        palette.text,
                    );
                    if hovered && let Some(uri) = uri {
                        let playing_here = app.playing_context_uri().as_deref()
                            == Some(uri.as_str())
                            && app.believed_playing();
                        let button = Rect::from_center_size(
                            pos2(rect.right() - 28.0, rect.center().y),
                            Vec2::splat(40.0),
                        );
                        let mut child =
                            ui.new_child(egui::UiBuilder::new().max_rect(button).layout(
                                egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            ));
                        if theme::circle_button(
                            &mut child,
                            if playing_here {
                                Icon::PauseFilled
                            } else {
                                Icon::PlayFilled
                            },
                            40.0,
                            palette.accent,
                            palette.accent_hover,
                            palette.on_accent,
                            &gettext(app.locale, if playing_here { "Pause" } else { "Play" }),
                        )
                        .clicked()
                        {
                            if playing_here {
                                app.actions.push(Action::TogglePlay);
                            } else {
                                app.actions.push(Action::PlayContext {
                                    uri: uri.clone(),
                                    offset_uri: None,
                                    offset_index: None,
                                });
                            }
                        }
                    }
                }
                if response.clicked() {
                    app.actions.push(Action::Open(page.clone()));
                }
                if !liked && let Some(uri) = uri {
                    egui::Popup::context_menu(&response)
                        .id(ui.make_persistent_id(("quick-access-menu", uri)))
                        .frame(widgets::menu_frame(&palette))
                        .show(|ui| {
                            widgets::context_menu_items(
                                ui,
                                app,
                                uri,
                                name,
                                owned_playlist.as_ref(),
                            );
                        });
                }
            }
        });
    }
}

fn made_for_you(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let mut playlists: Vec<Playlist> = Vec::new();
    let mut loading = false;
    let mut failed = false;
    for term in DISCOVER_TERMS {
        match app.home.discover.get(*term) {
            Some(Loadable::Loaded(list)) => {
                for playlist in list {
                    let duplicate = playlists.iter().any(|existing| {
                        existing.id == playlist.id
                            || existing.name.eq_ignore_ascii_case(&playlist.name)
                    });
                    if !duplicate {
                        playlists.push(playlist.clone());
                    }
                }
            }
            Some(Loadable::Loading) => loading = true,
            Some(Loadable::Failed(_)) => failed = true,
            _ => {}
        }
    }
    if playlists.is_empty() && !loading && !failed {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "made-for-you",
        &gettext(app.locale, "Made for you"),
        |ui| {
            if playlists.is_empty() && loading {
                widgets::loading_row(ui, &palette, app.locale);
            } else if playlists.is_empty() && failed {
                let message = gettext(app.locale, "Couldn't load this shelf");
                widgets::error_row(ui, app, &message, Some(Page::Home));
            }
            for playlist in &playlists {
                let subtitle = playlist
                    .description
                    .as_deref()
                    .map(crate::util::strip_html)
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| {
                        // Translators: {owner} is the name of the playlist's owner.
                        gettext(app.locale, "By {owner}").replace("{owner}", playlist.owner_name())
                    });
                let playing_here = app.playing_context_uri().as_deref()
                    == Some(playlist.uri.as_str())
                    && app.believed_playing();
                let card = widgets::card(
                    ui,
                    app,
                    pick_image(&playlist.images, 640),
                    &playlist.name,
                    &subtitle,
                    widgets::CardCover::square(playing_here),
                );
                if card.play {
                    if playing_here {
                        app.actions.push(Action::TogglePlay);
                    } else {
                        app.actions.push(Action::PlayContext {
                            uri: playlist.uri.clone(),
                            offset_uri: None,
                            offset_index: None,
                        });
                    }
                }
                if card.clicked {
                    app.actions
                        .push(Action::Open(Page::Playlist(playlist.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(ui.make_persistent_id(("home-made_for_you-menu", &playlist.uri)))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        let owned = app.user_id().is_some_and(|id| playlist.owned_by(id));
                        widgets::context_menu_items(
                            ui,
                            app,
                            &playlist.uri,
                            &playlist.name,
                            owned.then_some(playlist),
                        );
                    });
            }
        },
    );
}

fn recently_played(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let history = match app.home.recently_played.clone() {
        Loadable::Loaded(history) => history,
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::shelf(
                ui,
                &palette,
                "recent",
                &gettext(app.locale, "Recently played"),
                |ui| widgets::loading_row(ui, &palette, app.locale),
            );
            return;
        }
        Loadable::Failed(message) => {
            widgets::shelf(
                ui,
                &palette,
                "recent",
                &gettext(app.locale, "Recently played"),
                |ui| {
                    widgets::error_row(ui, app, &message, Some(Page::Home));
                },
            );
            return;
        }
    };
    let mut seen = std::collections::HashSet::new();
    let tracks: Vec<_> = history
        .into_iter()
        .filter(|entry| {
            entry
                .track
                .id
                .as_ref()
                .is_some_and(|id| seen.insert(id.clone()))
        })
        .collect();
    if tracks.is_empty() {
        return;
    }
    let all = app.settings.home.recent_all;
    let total = tracks.len();
    let tracks: Vec<_> = tracks.into_iter().take(if all { 50 } else { 16 }).collect();
    if total > 16 {
        let label = if all { "Show fewer" } else { "Show all recently played" };
        ui.horizontal(|ui| {
            theme::section_title(ui, &palette, &gettext(app.locale, "Recently played"));
            if theme::pill_button(ui, &palette, label, false).clicked() {
                app.settings.home.recent_all = !all;
                app.mark_settings_dirty();
            }
        });
    }
    let shelf_title = if total > 16 { String::new() } else { gettext(app.locale, "Recently played").into_owned() };
    widgets::shelf(
        ui,
        &palette,
        "recent",
        &shelf_title,
        |ui| {
            for entry in &tracks {
                let track = &entry.track;
                let card = widgets::card(
                    ui,
                    app,
                    track.image(640),
                    &track.name,
                    &track.artist_names(),
                    widgets::CardCover::square(false),
                );
                if card.play {
                    app.actions.push(Action::PlayUris {
                        uris: vec![track.uri.clone()],
                        index: 0,
                    });
                }
                if card.clicked
                    && let Some(album) = &track.album
                    && !album.id.is_empty()
                {
                    app.actions
                        .push(Action::Open(Page::Album(album.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(ui.make_persistent_id(("home-recently_played-menu", &track.uri)))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::item_menu(
                            ui,
                            app,
                            &PlayableItem::Track(track.clone()),
                            None,
                            None,
                        );
                    });
            }
        },
    );
}

/// Why an episode is on the podcast shelf.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EpisodeReason {
    /// Started and not finished, with this much left.
    Continue { left_ms: u32 },
    /// The show's newest episode, recently released and not yet started.
    New,
}

/// How many days after its release an unplayed episode still counts as new.
const NEW_EPISODE_DAYS: i64 = 30;
/// The podcast shelf's card limit, as for Recently played.
const PODCAST_CARDS: usize = 16;

/// The episodes on the podcast shelf: those to continue first, then each
/// show's newest unstarted episode from the last month, each group newest
/// first. `skip` leaves out shows that are audiobooks or no longer saved.
pub(crate) fn podcast_episodes(
    podcasts: &[(Show, Vec<Episode>)],
    skip: impl Fn(&Show) -> bool,
    today: jiff::civil::Date,
) -> Vec<(Show, Episode, EpisodeReason)> {
    let oldest_new = today
        .checked_sub(jiff::Span::new().days(NEW_EPISODE_DAYS))
        .unwrap_or(today);
    let released = |episode: &Episode| {
        episode
            .release_date
            .as_deref()
            .and_then(|date| date.get(..10))
            .and_then(|date| date.parse::<jiff::civil::Date>().ok())
    };
    let mut continuing = Vec::new();
    let mut new = Vec::new();
    for (show, episodes) in podcasts {
        if skip(show) {
            continue;
        }
        for episode in episodes {
            if let Some(resume) = &episode.resume_point
                && !resume.fully_played
                && resume.resume_position_ms > 0
            {
                let left_ms = episode
                    .duration_ms
                    .saturating_sub(resume.resume_position_ms);
                continuing.push((show, episode, EpisodeReason::Continue { left_ms }));
            }
        }
        // Spotify lists a show's episodes newest first.
        if let Some(newest) = episodes.first()
            && newest
                .resume_point
                .as_ref()
                .is_none_or(|resume| !resume.fully_played && resume.resume_position_ms == 0)
            && released(newest).is_some_and(|date| date >= oldest_new)
        {
            new.push((show, newest, EpisodeReason::New));
        }
    }
    for group in [&mut continuing, &mut new] {
        group.sort_by_key(|(_, episode, _)| std::cmp::Reverse(released(episode)));
    }
    let mut seen = std::collections::HashSet::new();
    continuing
        .into_iter()
        .chain(new)
        .filter(|(_, episode, _)| !episode.uri.is_empty() && seen.insert(&episode.uri))
        .take(PODCAST_CARDS)
        .map(|(show, episode, reason)| {
            let mut episode = episode.clone();
            // A show's episode list leaves out the show; menus need it.
            if episode.show.is_none() {
                episode.show = Some(show.clone());
            }
            (show.clone(), episode, reason)
        })
        .collect()
}

fn podcasts(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let episodes = podcast_episodes(
        &app.home.podcasts,
        |show| app.audiobook_shows.contains(&show.uri) || app.is_saved(&show.uri) == Some(false),
        jiff::Zoned::now().date(),
    );
    if episodes.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "podcasts",
        &gettext(app.locale, "Your podcasts"),
        |ui| {
            for (show, episode, reason) in &episodes {
                let subtitle = match reason {
                    EpisodeReason::Continue { left_ms } => {
                        // Translators: {time} is the time left in an episode, such as 12 min; {show} is the podcast's name.
                        gettext(app.locale, "{time} left • {show}")
                            .replace(
                                "{time}",
                                &crate::util::format_episode_ms(app.locale, *left_ms),
                            )
                            .replace("{show}", &show.name)
                    }
                    EpisodeReason::New => {
                        // Translators: {show} is the podcast's name.
                        gettext(app.locale, "New • {show}").replace("{show}", &show.name)
                    }
                };
                let card = widgets::card(
                    ui,
                    app,
                    pick_image(&episode.images, 640).or_else(|| pick_image(&show.images, 640)),
                    &episode.name,
                    &subtitle,
                    widgets::CardCover::square(false),
                );
                if card.play {
                    app.actions.push(Action::PlayEpisode {
                        uri: episode.uri.clone(),
                        resume_ms: episode.resume_ms(),
                    });
                }
                if card.clicked && !show.id.is_empty() {
                    app.actions.push(Action::Open(Page::Show(show.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(ui.make_persistent_id(("home-podcasts-menu", &episode.uri)))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::item_menu(
                            ui,
                            app,
                            &PlayableItem::Episode(episode.clone()),
                            None,
                            None,
                        );
                    });
            }
        },
    );
}

fn top_artists(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let artists = match app.home.top_artists.clone() {
        Loadable::Loaded(artists) => artists,
        Loadable::Loading | Loadable::NotLoaded => {
            widgets::shelf(
                ui,
                &palette,
                "top-artists",
                &gettext(app.locale, "Your top artists"),
                |ui| widgets::loading_row(ui, &palette, app.locale),
            );
            return;
        }
        Loadable::Failed(message) => {
            widgets::shelf(
                ui,
                &palette,
                "top-artists",
                &gettext(app.locale, "Your top artists"),
                |ui| {
                    widgets::error_row(ui, app, &message, Some(Page::Home));
                },
            );
            return;
        }
    };
    if artists.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "top-artists",
        &gettext(app.locale, "Your top artists"),
        |ui| {
            for artist in &artists {
                let playing_here = app.playing_context_uri().as_deref()
                    == Some(artist.uri.as_str())
                    && app.believed_playing();
                let card = widgets::card(
                    ui,
                    app,
                    pick_image(&artist.images, 640),
                    &artist.name,
                    &gettext(app.locale, "Artist"),
                    widgets::CardCover::portrait(playing_here),
                );
                if card.play {
                    if playing_here {
                        app.actions.push(Action::TogglePlay);
                    } else {
                        app.actions.push(Action::PlayContext {
                            uri: artist.uri.clone(),
                            offset_uri: None,
                            offset_index: None,
                        });
                    }
                }
                if card.clicked {
                    app.actions
                        .push(Action::Open(Page::Artist(artist.id.clone())));
                }
                egui::Popup::context_menu(&card.response)
                    .id(ui.make_persistent_id(("home-top_artists-menu", &artist.uri)))
                    .frame(widgets::menu_frame(&palette))
                    .show(|ui| {
                        widgets::context_menu_items(ui, app, &artist.uri, &artist.name, None);
                    });
            }
        },
    );
}

fn track_list(
    app: &mut App,
    ui: &mut egui::Ui,
    title: &str,
    tracks: Loadable<Vec<crate::api::models::Track>>,
    limit: usize,
    title_page: Option<Page>,
    more_label: Option<&str>,
) {
    let palette = app.palette;
    let tracks = match tracks {
        Loadable::Loaded(tracks) => tracks,
        Loadable::Loading | Loadable::NotLoaded => {
            if let Some(page) = title_page {
                if theme::link(ui, title, theme::bold(17.0), palette.text).clicked() {
                    app.actions.push(Action::Open(page));
                }
            } else {
                theme::section_title(ui, &palette, title);
            }
            widgets::loading_row(ui, &palette, app.locale);
            ui.add_space(12.0);
            return;
        }
        Loadable::Failed(message) => {
            theme::section_title(ui, &palette, title);
            widgets::error_row(ui, app, &message, Some(title_page.unwrap_or(Page::Home)));
            ui.add_space(12.0);
            return;
        }
    };
    if tracks.is_empty() {
        return;
    }
    if let Some(page) = title_page {
        if theme::link(ui, title, theme::bold(17.0), palette.text).clicked() {
            app.actions.push(Action::Open(page));
        }
    } else {
        theme::section_title(ui, &palette, title);
    }
    ui.add_space(4.0);
    let uris: Arc<[String]> = tracks
        .iter()
        .map(|track| track.uri.clone())
        .collect::<Vec<_>>()
        .into();
    let context = RowContext::Uris(Arc::clone(&uris));
    for (index, track) in tracks.iter().take(limit).enumerate() {
        let item = PlayableItem::Track(track.clone());
        widgets::track_row(
            ui,
            app,
            TrackRow {
                index,
                number: None,
                item: &item,
                context: &context,
                show_cover: !app.settings.tracklist_compact,
                show_album: true,
                added_at: None,
                added_by: None,
                show_added_by: false,
                show_bpm: false,
                show_time: true,
                compact: false,
                thin: app.settings.tracklist_compact,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
            },
        );
    }
    if let Some(label) = more_label
        && tracks.len() > limit
        && theme::link(ui, label, theme::semibold(14.0), palette.secondary).clicked()
    {
        app.actions.push(Action::Open(Page::TopSongs));
    }
    ui.add_space(16.0);
}

fn top_tracks(app: &mut App, ui: &mut egui::Ui) {
    let tracks = app.home.top_tracks.clone();
    let title = gettext(app.locale, "Your top songs");
    let more = gettext(app.locale, "Show more top songs");
    track_list(
        app,
        ui,
        &title,
        tracks,
        10,
        Some(Page::TopSongs),
        Some(&more),
    );
}

fn recommendations(app: &mut App, ui: &mut egui::Ui) {
    let mut tracks = app.home.recommendations.clone();
    // Only songs that are new to you: not in Liked Songs, and not in any of
    // the playlists that are open or saved in this window.
    if let Loadable::Loaded(list) = &tracks {
        app.request_contains(list.iter().map(|track| track.uri.clone()).collect());
        let mut owned: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for page in app.playlist_pages.values() {
            for item in &page.items.items {
                if let Some(entry) = item.item.as_ref().or(item.track.as_ref()) {
                    owned.insert(entry.uri());
                }
            }
        }
        let kept: Vec<_> = list
            .iter()
            .filter(|track| {
                app.saved.get(&track.uri) != Some(&true) && !owned.contains(track.uri.as_str())
            })
            .cloned()
            .collect();
        tracks = Loadable::Loaded(kept);
    }
    let title = gettext(app.locale, "Recommended for you");
    // Always the whole list.
    track_list(app, ui, &title, tracks, usize::MAX, None, None);
    ui.add_space(14.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::ResumePoint;

    fn show(id: &str) -> Show {
        Show {
            id: id.into(),
            uri: format!("spotify:show:{id}"),
            name: id.to_uppercase(),
            ..Show::default()
        }
    }

    fn episode(id: &str, date: &str, played: Option<u32>, finished: bool) -> Episode {
        Episode {
            id: id.into(),
            uri: format!("spotify:episode:{id}"),
            duration_ms: 3_600_000,
            release_date: Some(date.into()),
            resume_point: Some(ResumePoint {
                fully_played: finished,
                resume_position_ms: played.unwrap_or(0),
            }),
            ..Episode::default()
        }
    }

    fn ids(shelf: &[(Show, Episode, EpisodeReason)]) -> Vec<&str> {
        shelf
            .iter()
            .map(|(_, episode, _)| episode.id.as_str())
            .collect()
    }

    fn today() -> jiff::civil::Date {
        "2026-09-23".parse().unwrap()
    }

    #[test]
    fn episodes_to_continue_come_before_new_ones() {
        let podcasts = vec![
            (
                show("a"),
                vec![
                    episode("a-new", "2026-09-20", None, false),
                    episode("a-started", "2026-09-01", Some(600_000), false),
                    episode("a-done", "2026-08-25", Some(0), true),
                ],
            ),
            (
                show("b"),
                vec![
                    episode("b-started", "2026-09-10", Some(60_000), false),
                    episode("b-old", "2026-09-03", None, false),
                ],
            ),
            (show("c"), vec![episode("c-new", "2026-09-22", None, false)]),
        ];
        let shelf = podcast_episodes(&podcasts, |_| false, today());
        assert_eq!(ids(&shelf), ["b-started", "a-started", "c-new", "a-new"]);
        assert_eq!(shelf[1].2, EpisodeReason::Continue { left_ms: 3_000_000 });
        assert_eq!(shelf[2].2, EpisodeReason::New);
        assert_eq!(
            shelf[0].1.show.as_ref().map(|show| show.id.as_str()),
            Some("b"),
            "the episode menu can go to its podcast"
        );
    }

    #[test]
    fn only_a_recent_unstarted_newest_episode_is_new() {
        let podcasts = vec![
            (show("old"), vec![episode("old", "2026-07-01", None, false)]),
            (
                show("finished"),
                vec![episode("finished", "2026-09-20", Some(0), true)],
            ),
            (
                show("undated"),
                vec![episode("undated", "2026", None, false)],
            ),
            (
                show("second"),
                vec![
                    episode("second-done", "2026-09-21", Some(0), true),
                    episode("second-new", "2026-09-20", None, false),
                ],
            ),
        ];
        assert!(podcast_episodes(&podcasts, |_| false, today()).is_empty());
    }

    #[test]
    fn skipped_shows_leave_the_shelf_and_it_stays_bounded() {
        let podcasts: Vec<_> = (0..20)
            .map(|index| {
                let id = format!("s{index}");
                let started = episode(&format!("{id}-e"), "2026-09-01", Some(1), false);
                (show(&id), vec![started])
            })
            .collect();
        let shelf = podcast_episodes(&podcasts, |show| show.id == "s0", today());
        assert_eq!(shelf.len(), PODCAST_CARDS);
        assert!(!ids(&shelf).contains(&"s0-e"));
    }
}
