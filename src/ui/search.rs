//! The Search page.

use std::sync::Arc;

use egui::{Align, CornerRadius, Layout, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::{Artist, ArtistRef, PlayableItem, SearchResults, pick_image};
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Loadable, Page, RowContext, SearchFilter};
use crate::theme::{self, Icon};

use super::widgets::{self, TrackRow};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    if app.search.committed.is_empty() && app.search.typed_at.is_none() {
        recent(app, ui);
        return;
    }
    ui.add_space(4.0);
    // A search for an artist puts the Artists pill first, as Spotify does.
    let about_artist = matches!(&app.search.results, Loadable::Loaded(results)
        if results.artists.as_ref().and_then(|page| page.items.first())
            .is_some_and(|artist| artist.name.to_lowercase() == app.search.committed.to_lowercase()));
    let order = if about_artist { SearchFilter::ARTIST_FIRST } else { SearchFilter::ALL };
    let labels: Vec<_> = order
        .iter()
        .map(|f| (*f, f.label(app.locale)))
        .collect();
    let options: Vec<(SearchFilter, &str)> = labels
        .iter()
        .map(|(filter, label)| (*filter, label.as_ref()))
        .collect();
    if let Some(filter) = widgets::chips(ui, &palette, &options, app.search.filter) {
        app.actions.push(Action::SetSearchFilter(filter));
    }
    ui.add_space(12.0);
    let pending = app.search.catalogue_pending || app.search.playlists_pending;
    if pending {
        widgets::loading_row(ui, &palette, app.locale);
    }
    if let Some(error) = app.search.error.clone() {
        widgets::error_row(ui, app, &error, None);
    }
    let results = match &app.search.results {
        Loadable::Loaded(results) => results.clone(),
        Loadable::Loading | Loadable::NotLoaded => {
            if !pending {
                widgets::loading_row(ui, &palette, app.locale);
            }
            return;
        }
        Loadable::Failed(error) => {
            let error = error.clone();
            if app.search.error.is_none() {
                widgets::error_row(ui, app, &error, None);
            }
            return;
        }
    };
    if results.is_empty() && (pending || app.search.error.is_some()) {
        return;
    }
    if results.is_empty() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Search,
            // Translators: {query} is the text the user searched for.
            &gettext(app.locale, "No results for “{query}”")
                .replace("{query}", &app.search.committed),
            &gettext(app.locale, "Check the spelling, or try fewer words."),
        );
        return;
    }
    match app.search.filter {
        SearchFilter::All => all(app, ui, &results),
        SearchFilter::Songs => songs(app, ui, &results, usize::MAX),
        SearchFilter::Artists => artists_grid(app, ui, &results),
        SearchFilter::Albums => albums_grid(app, ui, &results),
        SearchFilter::Playlists => playlists_grid(app, ui, &results),
        SearchFilter::Podcasts => shows_grid(app, ui, &results),
        SearchFilter::Episodes => episodes(app, ui, &results, usize::MAX),
    }
}

fn recent(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(6.0);
    if app.settings.search_history.is_empty() {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Search,
            &gettext(app.locale, "Search Spotify"),
            &gettext(
                app.locale,
                "Find songs, artists, albums, playlists, and podcasts.",
            ),
        );
        return;
    }
    theme::section_title(ui, &palette, &gettext(app.locale, "Recent searches"));
    ui.add_space(6.0);
    let history = app.settings.search_history.clone();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for query in &history {
            let (response, forget) = theme::soft_button_dismiss(ui, &palette, Icon::Clock, query);
            if forget {
                app.actions.push(Action::ForgetSearch(query.clone()));
            } else if response.clicked() {
                app.actions.push(Action::Search(query.clone()));
            }
        }
    });
}

fn all(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let palette = app.palette;
    let locale = app.locale;
    let query = app.search.committed.to_lowercase();
    let top_artist = results
        .artists
        .as_ref()
        .and_then(|page| page.items.first())
        .filter(|artist| {
            artist.name.to_lowercase() == query
                || results.tracks.as_ref().is_none_or(|t| t.items.is_empty())
        });
    ui.add_space(2.0);
        {
            if let Some(artist) = top_artist {
                top_result(
                    app,
                    ui,
                    pick_image(&artist.images, 640),
                    &artist.name,
                    TopResultSubtitle::Text(&gettext(locale, "Artist")),
                    true,
                    Some(artist.uri.clone()),
                    Page::Artist(artist.id.clone()),
                    |ui, app| {
                        widgets::context_menu_items(ui, app, &artist.uri, &artist.name, None);
                    },
                );
            } else if let Some(track) = results.tracks.as_ref().and_then(|page| page.items.first())
            {
                let page = track
                    .album
                    .as_ref()
                    .map(|album| Page::Album(album.id.clone()))
                    .unwrap_or(Page::Search);
                top_result(
                    app,
                    ui,
                    track.image(640),
                    &track.name,
                    TopResultSubtitle::SongArtists(&track.artists),
                    false,
                    Some(track.uri.clone()),
                    page,
                    |ui, app| {
                        widgets::item_menu(
                            ui,
                            app,
                            &PlayableItem::Track(track.clone()),
                            None,
                            None,
                        );
                    },
                );
            } else if let Some(album) = results.albums.as_ref().and_then(|page| page.items.first())
            {
                top_result(
                    app,
                    ui,
                    pick_image(&album.images, 640),
                    &album.name,
                    TopResultSubtitle::Text(
                        // Translators: {artists} is the album's artist names.
                        &gettext(locale, "Album • {artists}").replace(
                            "{artists}",
                            &crate::api::models::join_names(
                                album.artists.iter().map(|a| a.name.as_str()),
                            ),
                        ),
                    ),
                    false,
                    Some(album.uri.clone()),
                    Page::Album(album.id.clone()),
                    |ui, app| {
                        widgets::context_menu_items(ui, app, &album.uri, &album.name, None);
                    },
                );
            } else if let Some(playlist) = results
                .playlists
                .as_ref()
                .and_then(|page| page.items.first())
            {
                top_result(
                    app,
                    ui,
                    pick_image(&playlist.images, 640),
                    &playlist.name,
                    TopResultSubtitle::Text(
                        // Translators: {owner} is the name of the playlist's owner.
                        &gettext(locale, "Playlist • {owner}")
                            .replace("{owner}", playlist.owner_name()),
                    ),
                    false,
                    Some(playlist.uri.clone()),
                    Page::Playlist(playlist.id.clone()),
                    |ui, app| {
                        let owned = app.user_id().is_some_and(|id| playlist.owned_by(id));
                        widgets::context_menu_items(
                            ui,
                            app,
                            &playlist.uri,
                            &playlist.name,
                            owned.then_some(playlist),
                        );
                    },
                );
            } else if let Some(show) = results.shows.as_ref().and_then(|page| page.items.first()) {
                top_result(
                    app,
                    ui,
                    pick_image(&show.images, 640),
                    &show.name,
                    TopResultSubtitle::Text(
                        // Translators: {publisher} is the podcast's publisher.
                        &gettext(locale, "Podcast • {publisher}")
                            .replace("{publisher}", &show.publisher),
                    ),
                    false,
                    Some(show.uri.clone()),
                    Page::Show(show.id.clone()),
                    |ui, app| {
                        widgets::context_menu_items(ui, app, &show.uri, &show.name, None);
                    },
                );
            }
        }
    ui.add_space(10.0);
    result_rows(app, ui, results, 8);
    ui.add_space(8.0);
    shelf_artists(app, ui, results);
    shelf_albums(app, ui, results);
    shelf_playlists(app, ui, results);
    shelf_shows(app, ui, results);
    if results
        .episodes
        .as_ref()
        .is_some_and(|page| !page.items.is_empty())
    {
        theme::section_title(ui, &palette, &gettext(locale, "Episodes"));
        ui.add_space(4.0);
        episodes(app, ui, results, 4);
    }
}

/// The songs under the top result, as Spotify lays them out: cover, name and
/// artists, a small tag saying what it is, and a round add / saved mark.
fn result_rows(app: &mut App, ui: &mut egui::Ui, results: &SearchResults, limit: usize) {
    let palette = app.palette;
    let Some(page) = &results.tracks else {
        return;
    };
    let uris: Arc<[String]> = page.items.iter().map(|track| track.uri.clone()).collect::<Vec<_>>().into();
    for (index, track) in page.items.iter().take(limit).enumerate() {
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 58.0), Sense::click());
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let hovered = response.hovered();
        if hovered {
            ui.painter().rect_filled(rect, CornerRadius::same(6), palette.surface_hover);
        }
        let cover = Rect::from_min_size(rect.min + vec2(8.0, 6.0), Vec2::splat(46.0));
        widgets::paint_cover(ui, &palette, track.image(64), cover, 4.0, Icon::Music, Some(app.backend.art()));
        if hovered {
            ui.painter().rect_filled(cover, CornerRadius::same(4), egui::Color32::from_black_alpha(130));
            theme::paint_icon(ui, Icon::PlayFilled, cover, 20.0, egui::Color32::WHITE);
        }
        let tag_x = rect.left() + rect.width() * 0.62;
        let text_right = (tag_x - 12.0).max(rect.left() + 80.0);
        let clip = Rect::from_min_max(pos2(cover.right() + 12.0, rect.top()), pos2(text_right, rect.bottom()));
        let painter = ui.painter().with_clip_rect(clip);
        crate::bidi::paint_line(&painter, clip.left(), clip.right(), rect.top() + 19.0, &track.name, theme::medium(15.0), palette.text);
        let artists = crate::api::models::join_names(track.artists.iter().map(|a| a.name.as_str()));
        crate::bidi::paint_line(&painter, clip.left(), clip.right(), rect.top() + 40.0, &artists, theme::regular(13.0), palette.secondary);
        // What it is: Song, Single, EP or Album.
        let tag = match track.album.as_ref().and_then(|album| album.album_type.as_deref()) {
            Some("single") => "Single",
            Some("compilation") => "Album",
            _ => "Song",
        };
        let tag_galley = crate::bidi::layout_line(ui.painter(), tag, theme::medium(11.5), palette.secondary);
        let tag_rect = Rect::from_min_size(
            pos2(tag_x, rect.center().y - 9.0),
            vec2(tag_galley.size().x + 10.0, 18.0),
        );
        ui.painter().rect_filled(tag_rect, CornerRadius::same(3), palette.surface_hover.gamma_multiply(1.2));
        ui.painter().galley(
            pos2(tag_rect.left() + 5.0, tag_rect.center().y - tag_galley.size().y / 2.0),
            tag_galley,
            palette.secondary,
        );
        // The round mark: saved songs show a tick, others a plus.
        let saved = app.is_saved(&track.uri).unwrap_or(false);
        let mark = Rect::from_center_size(pos2(rect.right() - 24.0, rect.center().y), Vec2::splat(28.0));
        let mark_response = ui.interact(mark, ui.id().with(("result-mark", index)), Sense::click());
        theme::paint_icon(
            ui,
            if saved { Icon::CircleCheck } else { Icon::CirclePlus },
            mark,
            20.0,
            if saved {
                palette.accent
            } else if mark_response.hovered() {
                palette.text
            } else {
                palette.secondary
            },
        );
        if mark_response.clicked() {
            app.actions.push(Action::ToggleSaved(track.uri.clone()));
        } else if response.double_clicked() {
            app.actions.push(Action::PlayUris {
                uris: uris.to_vec(),
                index: index as u32,
            });
        }
        let item = PlayableItem::Track(track.clone());
        egui::Popup::context_menu(&response)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| widgets::item_menu(ui, app, &item, None, None));
    }
}

enum TopResultSubtitle<'a> {
    Text(&'a str),
    SongArtists(&'a [ArtistRef]),
}

#[allow(clippy::too_many_arguments)]
fn top_result(
    app: &mut App,
    ui: &mut egui::Ui,
    image: Option<&str>,
    title: &str,
    subtitle: TopResultSubtitle<'_>,
    round: bool,
    play_uri: Option<String>,
    page: Page,
    menu: impl FnOnce(&mut egui::Ui, &mut App),
) {
    let palette = app.palette;
    let mut subtitle_clicked = false;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 84.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let hovered = ui.rect_contains_pointer(rect);
        let fill = if hovered {
            palette.surface_hover
        } else {
            palette.surface
        };
        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::RADIUS), fill);
        let image_rect = Rect::from_min_size(rect.min + vec2(14.0, 10.0), Vec2::splat(64.0));
        widgets::paint_cover(
            ui,
            &palette,
            image,
            image_rect,
            if round { 32.0 } else { 4.0 },
            if round { Icon::User } else { Icon::Music },
            Some(app.backend.art()),
        );
        let text_clip = Rect::from_min_max(
            pos2(image_rect.right() + 16.0, rect.top()),
            pos2(rect.right() - 150.0, rect.bottom()),
        );
        let painter = ui.painter().with_clip_rect(text_clip);
        crate::bidi::paint_line(
            &painter,
            text_clip.left(),
            text_clip.right(),
            text_clip.top() + 26.0,
            title,
            theme::bold(22.0),
            palette.text,
        );
        match subtitle {
            TopResultSubtitle::SongArtists(artists) => {
                let subtitle_rect = Rect::from_min_max(
                    pos2(text_clip.left(), text_clip.top() + 46.0),
                    pos2(text_clip.right(), text_clip.top() + 66.0),
                );
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(subtitle_rect)
                        .layout(Layout::left_to_right(Align::Center)),
                );
                child.set_clip_rect(subtitle_rect.intersect(ui.clip_rect()));
                child.spacing_mut().item_spacing.x = 0.0;
                theme::text(
                    &mut child,
                    // Translators: Precedes the song's artist names, which follow as links.
                    gettext(app.locale, "Song • ").as_ref(),
                    theme::regular(13.5),
                    palette.secondary,
                );
                subtitle_clicked = widgets::artist_links(
                    &mut child,
                    app,
                    artists,
                    theme::regular(13.5),
                    palette.secondary,
                );
            }
            TopResultSubtitle::Text(subtitle) => {
                crate::bidi::paint_line(
                    &painter,
                    text_clip.left(),
                    text_clip.right(),
                    text_clip.top() + 56.0,
                    subtitle,
                    theme::regular(13.5),
                    palette.secondary,
                );
            }
        }
        if let Some(uri) = &play_uri {
            // Artists get a Follow pill beside the play button.
            if round {
                let following = app.is_saved(uri).unwrap_or(false);
                let label = if following { "Following" } else { "Follow" };
                let pill = Rect::from_center_size(pos2(rect.right() - 126.0, rect.center().y), vec2(84.0, 30.0));
                let mut child = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(pill)
                        .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
                );
                if theme::soft_button(&mut child, &palette, None, label, following).clicked() {
                    app.actions.push(Action::ToggleSaved(uri.clone()));
                }
            }
            let button = Rect::from_center_size(
                pos2(rect.right() - 40.0, rect.center().y),
                Vec2::splat(48.0),
            );
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(button)
                    .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
            );
            if theme::circle_button(
                &mut child,
                Icon::PlayFilled,
                48.0,
                palette.accent,
                palette.accent_hover,
                palette.on_accent,
                &gettext(app.locale, "Play"),
            )
            .clicked()
            {
                if uri.starts_with("spotify:track:") {
                    app.actions.push(Action::PlayUris {
                        uris: vec![uri.clone()],
                        index: 0,
                    });
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
    if response.clicked() && !subtitle_clicked && page != Page::Search {
        app.actions.push(Action::Open(page));
    }
    egui::Popup::context_menu(&response)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| menu(ui, app));
}

fn songs(app: &mut App, ui: &mut egui::Ui, results: &SearchResults, limit: usize) {
    let palette = app.palette;
    let Some(page) = &results.tracks else {
        return;
    };
    if page.items.is_empty() {
        return;
    }
    theme::section_title(ui, &palette, &gettext(app.locale, "Songs"));
    ui.add_space(4.0);
    let uris: Arc<[String]> = page
        .items
        .iter()
        .map(|track| track.uri.clone())
        .collect::<Vec<_>>()
        .into();
    let context = RowContext::Uris(Arc::clone(&uris));
    let items: Vec<PlayableItem> = page
        .items
        .iter()
        .cloned()
        .map(PlayableItem::Track)
        .collect();
    for (index, item) in items.iter().take(limit).enumerate() {
        widgets::track_row(
            ui,
            app,
            TrackRow {
                index,
                number: None,
                item,
                context: &context,
                show_cover: true,
                show_album: limit == usize::MAX,
                added_at: None,
                added_by: None,
                show_added_by: false,
                // The full result list is a track list and gets the tempo
                // column; the truncated preview on the search page does not.
                show_bpm: limit == usize::MAX
                    && app
                        .settings
                        .track_columns
                        .shown(crate::model::SortColumn::Bpm),
                show_time: true,
                compact: limit != usize::MAX,
                thin: false,
                shift: 0.0,
                picked: false,
                picked_songs: &[],
            },
        );
    }
}

fn artist_card(app: &mut App, ui: &mut egui::Ui, artist: &Artist) {
    let playing_here =
        app.playing_context_uri().as_deref() == Some(artist.uri.as_str()) && app.believed_playing();
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
        .id(ui.make_persistent_id(("search-artist-menu", &artist.uri)))
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| {
            widgets::context_menu_items(ui, app, &artist.uri, &artist.name, None);
        });
}

fn shelf_artists(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let palette = app.palette;
    let Some(page) = &results.artists else { return };
    if page.items.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "search-artists",
        &gettext(app.locale, "Artists"),
        |ui| {
            for artist in &page.items {
                artist_card(app, ui, artist);
            }
        },
    );
}

fn artists_grid(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let Some(page) = &results.artists else { return };
    widgets::grid(ui, |ui| {
        for artist in &page.items {
            artist_card(app, ui, artist);
        }
    });
}

fn album_card(app: &mut App, ui: &mut egui::Ui, album: &crate::api::models::Album) {
    let subtitle = format!(
        "{} • {}",
        album.year().unwrap_or(""),
        crate::api::models::join_names(album.artists.iter().map(|a| a.name.as_str()))
    );
    let playing_here =
        app.playing_context_uri().as_deref() == Some(album.uri.as_str()) && app.believed_playing();
    let card = widgets::card(
        ui,
        app,
        pick_image(&album.images, 640),
        &album.name,
        subtitle.trim_start_matches(" • "),
        widgets::CardCover::square(playing_here),
    );
    if card.play {
        if playing_here {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri: album.uri.clone(),
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    if card.clicked {
        app.actions
            .push(Action::Open(Page::Album(album.id.clone())));
    }
    egui::Popup::context_menu(&card.response)
        .id(ui.make_persistent_id(("search-album-menu", &album.uri)))
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| {
            widgets::context_menu_items(ui, app, &album.uri, &album.name, None);
        });
}

fn shelf_albums(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let palette = app.palette;
    let Some(page) = &results.albums else { return };
    if page.items.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "search-albums",
        &gettext(app.locale, "Albums"),
        |ui| {
            for album in &page.items {
                album_card(app, ui, album);
            }
        },
    );
}

fn albums_grid(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let Some(page) = &results.albums else { return };
    widgets::grid(ui, |ui| {
        for album in &page.items {
            album_card(app, ui, album);
        }
    });
}

fn playlist_card(app: &mut App, ui: &mut egui::Ui, playlist: &crate::api::models::Playlist) {
    let playing_here = app.playing_context_uri().as_deref() == Some(playlist.uri.as_str())
        && app.believed_playing();
    let card = widgets::card(
        ui,
        app,
        pick_image(&playlist.images, 640),
        &playlist.name,
        // Translators: {owner} is the name of the playlist's owner.
        &gettext(app.locale, "By {owner}").replace("{owner}", playlist.owner_name()),
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
        .id(ui.make_persistent_id(("search-playlist-menu", &playlist.uri)))
        .frame(widgets::menu_frame(&app.palette))
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

fn shelf_playlists(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let palette = app.palette;
    let Some(page) = &results.playlists else {
        return;
    };
    if page.items.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "search-playlists",
        &gettext(app.locale, "Playlists"),
        |ui| {
            for playlist in &page.items {
                playlist_card(app, ui, playlist);
            }
        },
    );
}

fn playlists_grid(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let Some(page) = &results.playlists else {
        return;
    };
    widgets::grid(ui, |ui| {
        for playlist in &page.items {
            playlist_card(app, ui, playlist);
        }
    });
}

fn show_card(app: &mut App, ui: &mut egui::Ui, show: &crate::api::models::Show) {
    let card = widgets::card(
        ui,
        app,
        pick_image(&show.images, 640),
        &show.name,
        &show.publisher,
        widgets::CardCover::default(),
    );
    if card.clicked {
        app.actions.push(Action::Open(Page::Show(show.id.clone())));
    }
    egui::Popup::context_menu(&card.response)
        .id(ui.make_persistent_id(("search-show-menu", &show.uri)))
        .frame(widgets::menu_frame(&app.palette))
        .show(|ui| {
            widgets::context_menu_items(ui, app, &show.uri, &show.name, None);
        });
}

fn shelf_shows(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let palette = app.palette;
    let Some(page) = &results.shows else { return };
    if page.items.is_empty() {
        return;
    }
    widgets::shelf(
        ui,
        &palette,
        "search-shows",
        &gettext(app.locale, "Podcasts"),
        |ui| {
            for show in &page.items {
                show_card(app, ui, show);
            }
        },
    );
}

fn shows_grid(app: &mut App, ui: &mut egui::Ui, results: &SearchResults) {
    let Some(page) = &results.shows else { return };
    widgets::grid(ui, |ui| {
        for show in &page.items {
            show_card(app, ui, show);
        }
    });
}

fn episodes(app: &mut App, ui: &mut egui::Ui, results: &SearchResults, limit: usize) {
    let Some(page) = &results.episodes else {
        return;
    };
    for episode in page.items.iter().take(limit) {
        super::show::episode_row(app, ui, episode, None);
    }
}

#[allow(dead_code)]
fn align_right(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(Layout::right_to_left(Align::Center), add);
}
