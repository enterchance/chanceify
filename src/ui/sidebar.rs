//! The left panel: navigation and Your Library.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::model::{Action, Dialog, DragEntry, DragTrack, Loadable, Page};
use crate::settings::{LIKED_SONGS_KEY, LibraryShelf as Filter, LibrarySort};
use crate::theme::{self, Icon, Palette};

const DEFAULT_ROW_HEIGHT: f32 = 60.0;
const COMPACT_ROW_HEIGHT: f32 = 32.0;
/// How much of the Library header row the inline search field takes, and
/// the narrowest it is worth drawing at.
const INLINE_SEARCH_MIN: f32 = 90.0;
/// The space between the heading and the search field, and between the
/// field and the row's edge. Without it the field looks like a
/// continuation of the heading rather than a field of its own.
const HEADER_GUTTER: f32 = 14.0;
const INLINE_SEARCH_MAX: f32 = 220.0;
/// How far a name may be shrunk to fit its row, as a fraction of its
/// normal size. Past this it is an ellipsis, which reads better than a
/// name too small to make out.
const NAME_FIT_FLOOR: f32 = 0.62;

struct Entry {
    image: Option<String>,
    grid_image: Option<String>,
    name: String,
    subtitle: String,
    grid_subtitle: String,
    page: Page,
    uri: String,
    round: bool,
    liked: bool,
    /// The account's own playlist: the one it may rename and delete.
    owned: bool,
    /// A playlist the account may drop songs on.
    editable: bool,
    playlist_index: Option<usize>,
    /// A folder row: its rootlist id, whether it is rolled up, and how
    /// many playlists it holds.
    folder: Option<(String, bool, usize)>,
    /// How deep inside folders the row sits, for the indent.
    depth: u8,
    added_at: Option<i64>,
    /// A divider between categories, not a library row.
    header: bool,
}

impl Entry {
    fn ordering_key(&self) -> &str {
        if self.liked {
            LIKED_SONGS_KEY
        } else {
            &self.uri
        }
    }
}

/// The context a library row plays, matching the cover play button and the
/// right-click menu. Liked Songs has no URI of its own and plays the
/// account's collection instead.
fn entry_play_uri(app: &App, entry: &Entry) -> Option<String> {
    if entry.liked {
        app.user
            .as_ref()
            .map(|user| format!("spotify:user:{}:collection", user.id))
    } else if entry.uri.is_empty() {
        None
    } else {
        Some(entry.uri.clone())
    }
}

fn entry_is_playing_context(entry: &Entry, context: Option<&str>) -> bool {
    if entry.liked {
        context.is_some_and(|context| context.ends_with(":collection"))
    } else {
        !entry.uri.is_empty() && context == Some(entry.uri.as_str())
    }
}

fn cover_play_button(
    app: &mut App,
    ui: &mut egui::Ui,
    entry: &Entry,
    index: usize,
    cover_rect: Rect,
    parent: &egui::Response,
    playing: bool,
) -> bool {
    let Some(uri) = entry_play_uri(app, entry) else {
        return false;
    };
    // The row's playing state drives the control: the context that is
    // playing offers pause on its cover, every other row plays.
    let play = ui.interact(
        cover_rect,
        ui.id().with(("library-cover-play", index)),
        Sense::click(),
    );
    let play_hover = play.hovered();
    if play_hover || parent.hovered() {
        let radius = if entry.round {
            (cover_rect.width() / 2.0).min(127.0) as u8
        } else {
            6
        };
        ui.painter().rect_filled(
            cover_rect,
            CornerRadius::same(radius),
            egui::Color32::from_black_alpha(120),
        );
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let icon_size = (cover_rect.width() * 0.24).clamp(18.0, 26.0);
        icon.image(
            if play_hover {
                app.palette.accent
            } else {
                egui::Color32::WHITE
            },
            icon_size,
        )
        .paint_at(
            ui,
            Rect::from_center_size(
                cover_rect.center() + theme::play_glyph_offset(icon, icon_size),
                Vec2::splat(icon_size),
            ),
        );
    }
    if !play.clicked() {
        return false;
    }
    // The first click of a double click plays; the second must not play again.
    if !play.double_clicked() {
        if playing {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri,
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    true
}

/// Whether this row is a playlist holding songs that can no longer be played
/// (from the last "Unavailable songs" scan in Settings).
fn needs_attention(app: &App, entry: &Entry) -> bool {
    let Page::Playlist(id) = &entry.page else {
        return false;
    };
    let user = app.user.as_ref().map(|u| u.id.as_str()).unwrap_or_default();
    app.unavailable
        .entries_for(user)
        .iter()
        .any(|found| &found.playlist_id == id)
}

/// The small red dot for such a row.
fn attention_dot(ui: &egui::Ui, at: egui::Pos2) {
    ui.painter().circle_filled(at, 4.5, egui::Color32::from_rgb(230, 57, 70));
}

fn grid_play_rect(cover_rect: Rect) -> Rect {
    let size = (cover_rect.width() * 0.3).clamp(32.0, 44.0);
    let inset = LIBRARY_ITEM_PADDING + size / 2.0;
    Rect::from_center_size(
        pos2(cover_rect.right() - inset, cover_rect.bottom() - inset),
        Vec2::splat(size),
    )
}

fn grid_pin_rect(cover_rect: Rect) -> Rect {
    Rect::from_center_size(cover_rect.left_top() + Vec2::splat(12.0), Vec2::splat(20.0))
}

fn grid_play_button(
    app: &mut App,
    ui: &mut egui::Ui,
    entry: &Entry,
    cover_rect: Rect,
    parent: &egui::Response,
    playing_here: bool,
) -> bool {
    let Some(uri) = entry_play_uri(app, entry) else {
        return false;
    };
    let rect = grid_play_rect(cover_rect);
    let size = rect.width();
    let button = ui.interact(rect, ui.id().with("library-grid-play"), Sense::click());
    let playing = playing_here && app.believed_playing();
    let label = if playing {
        // Translators: The play button on a Library card. {name} is the playlist, album, artist, or podcast.
        gettext(app.locale, "Pause {name}")
    } else {
        // Translators: The play button on a Library card. {name} is the playlist, album, artist, or podcast.
        gettext(app.locale, "Play {name}")
    }
    .replace("{name}", &entry.name);
    button.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.clone())
    });
    if parent.hovered() || playing_here || button.has_focus() {
        let fill = if button.hovered() {
            app.palette.accent_hover
        } else {
            app.palette.accent
        };
        ui.painter().add(
            egui::epaint::Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: egui::Color32::from_black_alpha(90),
            }
            .as_shape(rect, egui::CornerRadius::same(127)),
        );
        ui.painter().circle_filled(rect.center(), size / 2.0, fill);
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let icon_size = size * 0.42;
        icon.image(app.palette.on_accent, icon_size).paint_at(
            ui,
            Rect::from_center_size(
                rect.center() + theme::play_glyph_offset(icon, icon_size),
                Vec2::splat(icon_size),
            ),
        );
    }
    if !button.clicked() {
        return false;
    }
    if playing_here {
        app.actions.push(Action::TogglePlay);
    } else {
        app.actions.push(Action::PlayContext {
            uri,
            offset_uri: None,
            offset_index: None,
        });
    }
    true
}

fn finish_entry_interaction(
    app: &mut App,
    ui: &mut egui::Ui,
    response: &egui::Response,
    entry: &Entry,
    cover_took_click: bool,
    custom_order: bool,
    drop_allowed: bool,
) {
    if response.hovered()
        && let Some(image) = &entry.image
    {
        app.actions.push(Action::PrepareTint(image.clone()));
    }
    if !entry.ordering_key().is_empty() && response.drag_started_by(egui::PointerButton::Primary) {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DragEntry {
                uri: entry.ordering_key().to_string(),
                title: entry.name.clone(),
                image: entry.image.clone(),
            },
        );
    }
    if drop_allowed
        && (entry.liked || entry.editable)
        && egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx())
        && let Some(track) = response.dnd_release_payload::<DragTrack>()
    {
        if entry.liked {
            app.actions.push(Action::SetSavedMany {
                uris: track
                    .items
                    .iter()
                    .map(|item| item.uri().to_string())
                    .collect(),
                saved: true,
            });
        } else if let Page::Playlist(id) = &entry.page {
            app.actions.push(Action::AddToPlaylist {
                playlist_id: id.clone(),
                playlist_name: entry.name.clone(),
                items: track.items.clone(),
            });
        }
    }
    theme::focus_ring(ui, response);
    if response.clicked() && !cover_took_click {
        if let Some((folder_id, _, _)) = &entry.folder {
            app.actions
                .push(Action::ToggleLibraryFolder(folder_id.clone()));
        } else {
            app.actions.push(Action::Open(entry.page.clone()));
        }
    }
    if !app.settings.sidebar_grid
        && response.double_clicked()
        && entry.folder.is_none()
        && !cover_took_click
        && let Some(uri) = entry_play_uri(app, entry)
    {
        app.actions.push(Action::PlayContext {
            uri,
            offset_uri: None,
            offset_index: None,
        });
    }
    entry_menu(app, response, entry, custom_order);
    crate::autoscroll::row(ui, response);
}

fn liked_entry(app: &App) -> Entry {
    let subtitle = match app.library.liked.total {
        Some(total) => app.locale.liked_song_count(total),
        None => gettext(app.locale, "Playlist").into_owned(),
    };
    let grid_subtitle = app
        .library
        .liked
        .total
        .map_or_else(String::new, |total| app.locale.song_count(total));
    Entry {
        image: None,
        grid_image: None,
        name: gettext(app.locale, "Liked Songs").into_owned(),
        subtitle,
        grid_subtitle,
        page: Page::LikedSongs,
        uri: String::new(),
        round: false,
        liked: true,
        owned: false,
        editable: false,
        playlist_index: None,
        folder: None,
        depth: 0,
        added_at: None,
        header: false,
    }
}

pub(crate) fn selected_sort(app: &App, shelf: Filter) -> LibrarySort {
    if let Some(sort) = app.settings.library_sort.get(&shelf).copied()
        && sort.supports(shelf)
    {
        return sort;
    }
    if shelf != Filter::Playlists {
        LibrarySort::Library
    } else if !app.settings.sidebar_order.is_empty() {
        LibrarySort::Local
    } else if app
        .rootlist
        .iter()
        .any(|row| matches!(row, crate::player::RootlistEntry::FolderStart { .. }))
    {
        LibrarySort::Spotify
    } else {
        LibrarySort::RecentlyPlayed
    }
}

/// The one button that stands in for the old row of shelf pills and the
/// order button: an icon, and everything it opens.
///
/// The shelf comes first, because that is the choice that decides what the
/// list below even is. A rule, then the orders that shelf supports, then
/// how the rows are drawn.
fn library_menu(
    app: &mut App,
    _ui: &mut egui::Ui,
    shelf: Filter,
    selected: LibrarySort,
    anchor: &egui::Response,
) {
    egui::Popup::menu(anchor)
        .frame(super::widgets::menu_frame(&app.palette))
        .show(|ui| library_menu_body(app, ui, shelf, selected));
}

/// The library menu's contents, for either entry point.
///
/// Split from [`library_menu`] so the heading's right-click menu can offer
/// the same choices plus a create entry, rather than the two drifting into
/// different menus.
fn library_menu_body(app: &mut App, ui: &mut egui::Ui, shelf: Filter, selected: LibrarySort) {
    let locale = app.locale;
    let mut widest = 140.0_f32;
    let measure = |ui: &egui::Ui, label: &str| {
        ui.painter()
            .layout_no_wrap(label.to_string(), theme::regular(13.5), app.palette.text)
            .size()
            .x
    };
    let orders = library_orders(locale);
    let shelves = library_shelves(locale);
    let view_labels = library_view_labels(locale);
    for (_, label) in &shelves {
        widest = widest.max(measure(ui, label));
    }
    for (_, label) in &orders {
        widest = widest.max(measure(ui, label));
    }
    for label in &view_labels {
        widest = widest.max(measure(ui, label));
    }
    ui.set_width((widest + 52.0).min(ui.ctx().content_rect().width() - 24.0).min(290.0));
    // Three named groups rather than one long list with rules in it: what
    // is shown, how it is ordered, and how it is drawn.
    // What is shown, as one row of buttons: Playlists is the default.
    let palette_for_shelf = app.palette;
    ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
    for pair in shelves.chunks(2) {
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            for (value, label) in pair {
                if shelf_chip(ui, &palette_for_shelf, *value, *value == shelf)
                    .on_hover_text(label.to_string())
                    .clicked()
                {
                    app.actions.push(Action::SetLibraryShelf(*value));
                }
            }
        });
    }
    super::widgets::menu_separator(ui, &app.palette);
    // The orders in one row of small buttons, each with a picture.
    let palette_for_sort = app.palette;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        ui.add_space(6.0);
        for (sort, label) in &orders {
            if !sort.supports(shelf)
                || *sort == LibrarySort::Spotify
                || (*sort == LibrarySort::Library && shelf == Filter::Playlists)
                || (*sort == LibrarySort::Local && app.settings.sidebar_order.is_empty())
            {
                continue;
            }
            let tip = match sort {
                LibrarySort::Local => gettext(locale, "Sort personal").into_owned(),
                LibrarySort::RecentlyPlayed => gettext(locale, "Sort recent").into_owned(),
                LibrarySort::Name => gettext(locale, "Sort alphabetical").into_owned(),
                LibrarySort::RecentlyAdded => gettext(locale, "Sort by recently added").into_owned(),
                _ => label.to_string(),
            };
            if sort_chip(ui, &palette_for_sort, *sort, "", *sort == selected)
                .on_hover_text(tip)
                .clicked()
            {
                app.actions
                    .push(Action::SetLibrarySort { shelf, sort: *sort });
            }
        }
    });
    super::widgets::menu_separator(ui, &app.palette);
    // Three pictures instead of three lines of words: a grid with names, a
    // grid of covers, and a stacked list. Clicking the stacked list again
    // turns its detail line on or off.
    let palette = app.palette;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_space(6.0);
        for index in 0..3usize {
            let chosen = match index {
                0 => app.settings.sidebar_grid && app.settings.library_grid_names,
                1 => app.settings.sidebar_grid && !app.settings.library_grid_names,
                _ => !app.settings.sidebar_grid,
            };
            let tip = match index {
                0 => gettext(locale, "Grid with names"),
                1 => gettext(locale, "Grid, covers only"),
                _ => gettext(locale, "Stacked vertically (click again for details)"),
            };
            if layout_icon(ui, &palette, index, chosen, app.settings.library_details_entry())
                .on_hover_text(tip)
                .clicked()
            {
                match index {
                    0 => {
                        app.actions.push(Action::SetLibraryGrid(true));
                        app.actions.push(Action::SetLibraryGridNames(true));
                    }
                    1 => {
                        app.actions.push(Action::SetLibraryGrid(true));
                        app.actions.push(Action::SetLibraryGridNames(false));
                    }
                    _ => {
                        if chosen {
                            app.actions
                                .push(Action::SetLibraryDetails(!app.settings.library_details));
                        } else {
                            app.actions.push(Action::SetLibraryGrid(false));
                        }
                    }
                }
            }
        }
    });
    // Name size for the stacked list without details, so the right size can
    // be found by eye.
    if !app.settings.sidebar_grid && !app.settings.library_details_entry() {
        super::widgets::menu_group(ui, &app.palette, &gettext(locale, "Name size"));
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            let size = if app.settings.library_name_size == 0 {
                20
            } else {
                app.settings.library_name_size
            };
            if ui.small_button("-").clicked() {
                app.settings.library_name_size = size.saturating_sub(1).max(10);
                app.actions.push(Action::SettingsChanged);
            }
            ui.label(size.to_string());
            if ui.small_button("+").clicked() {
                app.settings.library_name_size = (size + 1).min(40);
                app.actions.push(Action::SettingsChanged);
            }
        });
    }
}

/// One library order as a small button with a drawn icon: a clock for
/// recent, A-Z for name, a key for the local order, a plus for added.
fn sort_chip(
    ui: &mut egui::Ui,
    palette: &Palette,
    sort: LibrarySort,
    _label: &str,
    chosen: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::click());
    let fill = if chosen {
        palette.accent.gamma_multiply(0.30)
    } else if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    let ink = if chosen { palette.text } else { palette.secondary };
    let c = rect.center();
    let stroke = egui::Stroke::new(1.6, ink);
    match sort {
        LibrarySort::RecentlyPlayed => {
            ui.painter().circle_stroke(c, 7.0, stroke);
            ui.painter().line_segment([c, c + vec2(0.0, -4.5)], stroke);
            ui.painter().line_segment([c, c + vec2(3.5, 1.5)], stroke);
        }
        LibrarySort::Name => {
            ui.painter().text(c, egui::Align2::CENTER_CENTER, "A-Z", theme::regular(10.0), ink);
        }
        LibrarySort::Local => {
            // A key: a ring and a shaft with two teeth.
            ui.painter().circle_stroke(c + vec2(-4.0, 0.0), 3.6, stroke);
            ui.painter().line_segment([c + vec2(-0.5, 0.0), c + vec2(7.0, 0.0)], stroke);
            ui.painter().line_segment([c + vec2(4.0, 0.0), c + vec2(4.0, 3.5)], stroke);
            ui.painter().line_segment([c + vec2(7.0, 0.0), c + vec2(7.0, 3.0)], stroke);
        }
        _ => {
            ui.painter().line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], stroke);
            ui.painter().line_segment([c + vec2(0.0, -5.0), c + vec2(0.0, 5.0)], stroke);
        }
    }
    response
}

/// One layout choice drawn as a picture: 0 is four squares each with a line
/// under it, 1 is four squares, 2 is four stacked rectangles.
fn layout_icon(
    ui: &mut egui::Ui,
    palette: &Palette,
    kind: usize,
    chosen: bool,
    details: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::click());
    let fill = if chosen {
        palette.accent.gamma_multiply(0.30)
    } else if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
    let ink = if chosen { palette.text } else { palette.secondary };
    let painter = ui.painter();
    let inner = rect.shrink2(vec2(9.0, 6.0));
    match kind {
        0 => {
            // Two squares side by side, a line under each.
            let cell = 10.0;
            for column in 0..2 {
                let x = inner.left() + column as f32 * (cell + 7.0);
                let y = inner.top();
                painter.rect_filled(
                    Rect::from_min_size(pos2(x, y), vec2(cell, cell)),
                    CornerRadius::same(1),
                    ink,
                );
                painter.line_segment(
                    [pos2(x, y + cell + 3.0), pos2(x + cell, y + cell + 3.0)],
                    egui::Stroke::new(1.5, ink),
                );
            }
        }
        1 => {
            let cell = 7.0;
            for row in 0..2 {
                for column in 0..2 {
                    let x = inner.left() + column as f32 * (cell + 5.0) + 1.0;
                    let y = inner.top() + row as f32 * (cell + 3.0);
                    painter.rect_filled(
                        Rect::from_min_size(pos2(x, y), vec2(cell, cell)),
                        CornerRadius::same(1),
                        ink,
                    );
                }
            }
        }
        _ => {
            for row in 0..4 {
                let y = inner.top() + row as f32 * 5.0;
                painter.rect_filled(
                    Rect::from_min_size(pos2(inner.left(), y), vec2(4.0, 3.0)),
                    CornerRadius::same(1),
                    ink,
                );
                painter.rect_filled(
                    Rect::from_min_size(pos2(inner.left() + 7.0, y), vec2(18.0, 3.0)),
                    CornerRadius::same(1),
                    if details { ink } else { ink.gamma_multiply(0.7) },
                );
            }
        }
    }
    response
}

/// The order choices, most wanted first: what the reader arranged
/// themselves, then what they played, then what things are called, and last
/// the two orders Spotify hands over.
fn library_orders(
    locale: crate::i18n::Locale,
) -> [(LibrarySort, std::borrow::Cow<'static, str>); 6] {
    [
        (LibrarySort::Local, gettext(locale, "Local custom order")),
        (
            LibrarySort::RecentlyPlayed,
            gettext(locale, "Recently played"),
        ),
        (LibrarySort::Name, gettext(locale, "Alphabetical")),
        (
            LibrarySort::Spotify,
            gettext(locale, "Spotify custom order"),
        ),
        (LibrarySort::Library, gettext(locale, "Library order")),
        (
            LibrarySort::RecentlyAdded,
            gettext(locale, "Recently added"),
        ),
    ]
}

fn library_shelves(locale: crate::i18n::Locale) -> [(Filter, std::borrow::Cow<'static, str>); 4] {
    [
        (Filter::Playlists, gettext(locale, "Playlists")),
        (Filter::Albums, gettext(locale, "Albums")),
        (Filter::Artists, gettext(locale, "Artists")),
        (Filter::Podcasts, gettext(locale, "Podcasts")),
    ]
}

/// A shelf as a small picture with no name; the name shows on hover.
/// Playlists a list with a note, albums a rounded square with a disc in it,
/// artists a person, podcasts a microphone.
fn shelf_chip(ui: &mut egui::Ui, palette: &Palette, shelf: Filter, chosen: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(44.0, 32.0), Sense::click());
    let fill = if chosen {
        palette.accent.gamma_multiply(0.30)
    } else if response.hovered() {
        palette.surface_hover
    } else {
        palette.surface
    };
    ui.painter().rect_filled(rect, CornerRadius::same(7), fill);
    let ink = if chosen { palette.text } else { palette.secondary };
    match shelf {
        Filter::Playlists => theme::paint_icon(ui, Icon::ListMusic, rect, 18.0, ink),
        Filter::Artists => theme::paint_icon(ui, Icon::User, rect, 18.0, ink),
        Filter::Podcasts => theme::paint_icon(ui, Icon::Mic, rect, 18.0, ink),
        _ => {
            let stroke = egui::Stroke::new(1.6, ink);
            let square = Rect::from_center_size(rect.center(), vec2(18.0, 18.0));
            ui.painter().rect_stroke(square, CornerRadius::same(5), stroke, egui::StrokeKind::Inside);
            ui.painter().circle_stroke(square.center(), 5.0, stroke);
            ui.painter().circle_filled(square.center(), 1.5, ink);
        }
    }
    response
}

fn library_view_labels(locale: crate::i18n::Locale) -> [std::borrow::Cow<'static, str>; 3] {
    [
        gettext(locale, "Grid"),
        gettext(locale, "Grid, covers only"),
        gettext(locale, "Stacked vertically"),
    ]
}

fn saved_time(value: Option<&str>) -> Option<i64> {
    value
        .and_then(|text| text.parse::<jiff::Timestamp>().ok())
        .map(|time| time.as_millisecond())
}

/// The category an entry falls in under `sort`, for the dividers.
fn category_of(app: &App, sort: LibrarySort, entry: &Entry, locale: crate::i18n::Locale) -> String {
    match sort {
        LibrarySort::Name => {
            let first = entry.name.chars().find(|c| !c.is_whitespace());
            match first {
                Some(c) if c.is_alphabetic() => c.to_uppercase().collect(),
                Some(_) => "#".to_string(),
                None => "#".to_string(),
            }
        }
        LibrarySort::RecentlyPlayed => {
            let position = app.recent_contexts.iter().position(|held| {
                if entry.liked {
                    app.user_id()
                        .is_some_and(|id| held == &format!("spotify:user:{id}:collection"))
                } else {
                    held == &entry.uri
                }
            });
            match position {
                Some(rank) if rank < 5 => gettext(locale, "Just played").into_owned(),
                Some(rank) if rank < 20 => gettext(locale, "Played lately").into_owned(),
                Some(_) => gettext(locale, "Played a while ago").into_owned(),
                None => gettext(locale, "Not played lately").into_owned(),
            }
        }
        LibrarySort::RecentlyAdded => match entry.added_at {
            Some(millis) => jiff::Timestamp::from_millisecond(millis)
                .map(|time| time.strftime("%B %Y").to_string())
                .unwrap_or_else(|_| gettext(locale, "Earlier").into_owned()),
            None => gettext(locale, "Earlier").into_owned(),
        },
        _ => String::new(),
    }
}

/// Build the ranks once per frame, keeping the first occurrence just as
/// `position` did. Searching the whole order from a sort comparator makes
/// scrolling a large library quadratic even when only visible rows are drawn.
fn first_ranks<'a>(
    keys: impl Iterator<Item = &'a str>,
) -> std::collections::HashMap<&'a str, usize> {
    let mut ranks = std::collections::HashMap::new();
    for (rank, key) in keys.enumerate() {
        ranks.entry(key).or_insert(rank);
    }
    ranks
}

/// Applies the selected library order, then places pins first in their saved
/// order. New playlists precede a saved local arrangement, and pinned playlists
/// are shown at the top level rather than inside their folders.
fn order_entries(app: &App, shelf: Filter, sort: LibrarySort, entries: &mut [Entry]) {
    match sort {
        LibrarySort::Name => {
            entries.sort_by_cached_key(|entry| (entry.name.to_lowercase(), entry.uri.clone()))
        }
        LibrarySort::RecentlyPlayed => {
            let ranks = first_ranks(app.recent_contexts.iter().map(String::as_str));
            let liked_uri = app
                .user_id()
                .map(|id| format!("spotify:user:{id}:collection"));
            entries.sort_by_cached_key(|entry| {
                let key = if entry.liked {
                    liked_uri.as_deref()
                } else {
                    Some(entry.uri.as_str())
                };
                key.and_then(|key| ranks.get(key))
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
        LibrarySort::RecentlyAdded => entries
            .sort_by_key(|entry| (entry.added_at.is_none(), std::cmp::Reverse(entry.added_at))),
        LibrarySort::Local => {
            let ranks = first_ranks(app.settings.sidebar_order.iter().map(String::as_str));
            entries.sort_by_cached_key(|entry| match ranks.get(entry.ordering_key()) {
                Some(&rank) => (1, rank),
                None => (0, entry.playlist_index.unwrap_or(0)),
            });
        }
        LibrarySort::Spotify if !entries.iter().any(|entry| entry.folder.is_some()) => {
            let ranks = first_ranks(app.rootlist.iter().filter_map(|row| match row {
                crate::player::RootlistEntry::Playlist(uri) => Some(uri.as_str()),
                _ => None,
            }));
            entries.sort_by_cached_key(|entry| {
                (
                    entry.liked,
                    ranks.get(entry.uri.as_str()).copied().unwrap_or(usize::MAX),
                )
            });
        }
        LibrarySort::Spotify => entries.sort_by_key(|entry| entry.liked),
        LibrarySort::Library => {}
    }
    let pins = app.settings.library_pins();
    let ranks = first_ranks(pins.iter().map(String::as_str));
    entries.sort_by_cached_key(|entry| {
        ranks
            .get(entry.ordering_key())
            .copied()
            .unwrap_or(usize::MAX)
    });
    if shelf == Filter::Playlists {
        for entry in entries {
            if app.settings.pinned_contexts.contains(&entry.uri) {
                entry.depth = 0;
            }
        }
    }
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let expanded_art = has_expanded_art(app);
    // The list always ends above the big cover, grid or not.
    let floating_art = false && app.settings.sidebar_grid && expanded_art;
    // The traffic lights float over the top-left of the sidebar now, so the
    // first nav row has to start below them.
    let top = 12 + theme::titlebar_inset(ui.ctx()) as i8;
    let beside = if app.show_queue_panel || app.show_lyrics_panel {
        theme::SIDE_PANEL_MIN_WIDTH
    } else {
        0.0
    };
    let fit = super::yielding_panel(
        ui.ctx(),
        "sidebar",
        SIDEBAR_MIN_WIDTH..=sidebar_max_width(ui),
        app.settings.sidebar_width,
        ui.available_width() - super::topbar::least_width(ui.ctx()) - beside,
    );
    let panel = egui::Panel::left("sidebar")
        .resizable(true)
        .default_size(app.settings.sidebar_width)
        .size_range(fit.range.clone())
        .show_separator_line(false)
        .frame(Frame::new().fill(palette.panel).inner_margin(Margin {
            left: 12,
            right: 8,
            top,
            bottom: if expanded_art { 0 } else { 8 },
        }));
    let response = panel.show(ui, |ui| {
        // Small mode: the cover now sits in the player bar's controls row.
        let art_rect = expanded_art.then(|| expanded_art_rect(ui));
        if let Some(rect) = art_rect.filter(|_| !floating_art) {
            reserve_expanded_art(ui, rect);
        }
        contents(app, ui, art_rect.filter(|_| floating_art));
        if let Some(rect) = art_rect {
            if floating_art {
                paint_grid_art_mask(app, ui, rect);
            }
            paint_expanded_art(app, ui, rect);
        }
    });
    ui.ctx().data_mut(|data| {
        data.insert_temp(egui::Id::new("tour-sidebar"), response.response.rect)
    });
    let width = response.response.rect.width();
    if (width - app.settings.sidebar_width).abs() > 1.0
        && super::panel_width_chosen(ui.ctx(), "sidebar", &fit)
    {
        app.settings.sidebar_width = width;
        app.actions.push(Action::SettingsChanged);
    }
}

fn expanded_art_rect(ui: &egui::Ui) -> Rect {
    let side = expanded_art_side(ui);
    Rect::from_min_size(
        pos2(
            ui.max_rect().left(),
            ui.max_rect().bottom() - EXPANDED_ART_GAP - side,
        ),
        Vec2::splat(side),
    )
}

fn reserve_expanded_art(ui: &mut egui::Ui, rect: Rect) {
    egui::Panel::bottom("sidebar-art-space")
        .exact_size(rect.height() + EXPANDED_ART_GAP)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new())
        .show(ui, |_| {});
}

/// In grid mode content continues behind the shared fixed artwork.
fn paint_grid_art_mask(app: &App, ui: &egui::Ui, rect: Rect) {
    let bottom_fade_rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), rect.bottom() - 64.0),
        pos2(ui.max_rect().right(), rect.bottom()),
    );
    super::widgets::paint_vertical_gradient(
        ui,
        bottom_fade_rect,
        egui::Color32::TRANSPARENT,
        app.palette.panel,
    );
    ui.painter().rect_filled(
        Rect::from_min_max(
            pos2(ui.max_rect().left(), rect.bottom()),
            ui.max_rect().right_bottom(),
        ),
        0.0,
        app.palette.panel,
    );
}

fn has_expanded_art(app: &App) -> bool {
    app.settings.art_expanded
        && app
            .now_playing()
            .is_some_and(|now| now.art_url.is_some() || now.art_small.is_some())
}

fn expanded_art_side(ui: &egui::Ui) -> f32 {
    // The cover is as wide as the sidebar and no smaller: raising the
    // visualizer shortens the panel, and the art must not shrink with it
    // (it only ever grows with the sidebar).
    // Unless the panel is too short to hold it: it never leaves the screen.
    ui.max_rect()
        .width()
        .min((ui.max_rect().height() - 40.0).max(80.0))
        .max(80.0)
}

fn paint_expanded_art(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    let Some(now) = app.now_playing() else {
        return;
    };
    let Some(url) = now.art_url.clone().or_else(|| now.art_small.clone()) else {
        return;
    };
    let album_id = now.album_id.clone();
    let show_id = now.show_id.clone();
    let palette = app.palette;
    ui.painter().add(
        egui::epaint::Shadow {
            offset: [0, 0],
            blur: 28,
            spread: 0,
            color: egui::Color32::from_black_alpha(if palette.dark { 120 } else { 40 }),
        }
        .as_shape(rect, egui::CornerRadius::same(8)),
    );
    super::widgets::paint_cover(
        ui,
        &palette,
        Some(&url),
        rect,
        8.0,
        Icon::Music,
        Some(app.backend.art()),
    );
    super::player_bar::big_art_missing_mark(app, ui, rect, "sidebar");
    let art = ui.interact(rect, egui::Id::new("sidebar-art"), Sense::click());
    // The words ride on the art rather than sitting in the player bar, so
    // the bar can be as narrow as it likes and the cover keeps the width.
    let now = app.now_playing();
    if let Some(now) = now.as_ref() {
        super::player_bar::now_playing_overlay(app, ui, rect, now);
    }
    // No collapse arrow: the A key toggles the big picture. A click on the
    // picture is answered by the overlay, which jumps to the song's place in
    // its playlist.
    let _ = (art, album_id, show_id);
}

/// Playlist rows in account order, including collapsible folders (#95).
fn folder_rows(app: &App, user_id: &str, entries: &mut Vec<Entry>) {
    use crate::player::RootlistEntry;
    let Some(playlists) = app.library.playlists.get() else {
        return;
    };
    let by_uri: std::collections::HashMap<&str, (usize, &crate::api::models::Playlist)> = playlists
        .iter()
        .enumerate()
        .map(|(index, playlist)| (playlist.uri.as_str(), (index, playlist)))
        .collect();
    let mut depth = 0u8;
    // Rows inside a rolled-up folder stay off the list; the stack knows
    // how deep the rolled-up one sits.
    let mut hidden_from: Option<u8> = None;
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for row in &app.rootlist {
        match row {
            RootlistEntry::FolderStart { id, name } => {
                let collapsed = app.collapsed_folders.contains(id);
                if hidden_from.is_none() {
                    let count = folder_playlists(&app.rootlist, id);
                    entries.push(Entry {
                        image: None,
                        grid_image: None,
                        name: if name.is_empty() {
                            gettext(app.locale, "Folder").into_owned()
                        } else {
                            name.clone()
                        },
                        subtitle: app.locale.folder_playlist_count(count as u32),
                        grid_subtitle: app.locale.playlist_count(count as u32),
                        page: Page::Home,
                        uri: String::new(),
                        round: false,
                        liked: false,
                        owned: false,
                        editable: false,
                        playlist_index: None,
                        folder: Some((id.clone(), collapsed, count)),
                        depth,
                        added_at: None,
                        header: false,
                    });
                    if collapsed {
                        hidden_from = Some(depth);
                    }
                }
                depth += 1;
            }
            RootlistEntry::FolderEnd => {
                depth = depth.saturating_sub(1);
                if hidden_from == Some(depth) {
                    hidden_from = None;
                }
            }
            RootlistEntry::Playlist(uri) => {
                let Some((index, playlist)) = by_uri.get(uri.as_str()) else {
                    continue;
                };
                if !seen.insert(uri.as_str()) {
                    continue;
                }
                if hidden_from.is_some() && !app.settings.pinned_contexts.contains(uri) {
                    continue;
                }
                entries.push(playlist_entry(
                    app.locale,
                    playlist,
                    *index,
                    user_id,
                    app.can_edit_playlist(playlist),
                    depth,
                ));
            }
        }
    }
    // Playlists the rootlist has not met yet, the newly followed, wait at
    // the end rather than vanish.
    for (index, playlist) in playlists.iter().enumerate() {
        if !seen.contains(playlist.uri.as_str()) {
            entries.push(playlist_entry(
                app.locale,
                playlist,
                index,
                user_id,
                app.can_edit_playlist(playlist),
                0,
            ));
        }
    }
}

/// How many playlists a folder holds, nested ones included.
fn folder_playlists(rootlist: &[crate::player::RootlistEntry], id: &str) -> usize {
    use crate::player::RootlistEntry;
    let mut counting = false;
    let mut depth = 0usize;
    let mut count = 0;
    for row in rootlist {
        match row {
            RootlistEntry::FolderStart { id: this, .. } => {
                if counting {
                    depth += 1;
                } else if this == id {
                    counting = true;
                    depth = 1;
                }
            }
            RootlistEntry::FolderEnd if counting => {
                depth -= 1;
                if depth == 0 {
                    return count;
                }
            }
            RootlistEntry::Playlist(_) if counting => count += 1,
            _ => {}
        }
    }
    count
}

fn playlist_entry(
    locale: Locale,
    playlist: &crate::api::models::Playlist,
    index: usize,
    user_id: &str,
    editable: bool,
    depth: u8,
) -> Entry {
    Entry {
        image: pick_image(&playlist.images, 64).map(str::to_string),
        grid_image: pick_image(&playlist.images, super::GRID_ART_TARGET_WIDTH).map(str::to_string),
        name: playlist.name.clone(),
        // Translators: {owner} is the name of the playlist's owner.
        subtitle: gettext(locale, "Playlist • {owner}").replace("{owner}", playlist.owner_name()),
        grid_subtitle: playlist.owner_name().to_string(),
        page: Page::Playlist(playlist.id.clone()),
        uri: playlist.uri.clone(),
        round: false,
        liked: false,
        owned: playlist.owned_by(user_id),
        editable,
        playlist_index: Some(index),
        folder: None,
        depth,
        added_at: None,
        header: false,
    }
}

fn nav_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    active: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if active || response.hovered() {
            palette.text
        } else {
            palette.secondary
        };
        let icon_rect =
            Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), Vec2::splat(22.0));
        icon.image(color, 22.0).paint_at(ui, icon_rect);
        ui.painter().text(
            pos2(rect.left() + 46.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            theme::bold(15.0),
            color,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, label)
    });
    theme::focus_ring(ui, &response);
    response
}

fn contents(app: &mut App, ui: &mut egui::Ui, grid_art: Option<Rect>) {
    let palette = app.palette;
    let page = app.page().clone();
    let locale = app.locale;
    ui.add_space(4.0);
    let show_search_id = egui::Id::new("sidebar-show-search");
    let show_search = ui
        .data(|data| data.get_temp::<bool>(show_search_id))
        .unwrap_or(false);

    let mut focus_search = false;
    let library_button = None;
    // Set when the heading itself is right-clicked, which opens the library
    // menu with a create entry at the top.
    let mut heading_anchor: Option<egui::Response> = None;
    let field_id = egui::Id::new("sidebar-search");

    // With room to spare the Filter sits in the same row as the buttons;
    // a narrow sidebar gives it a row of its own underneath.
    let wide = ui.available_width() >= 330.0;
    ui.ctx()
        .data_mut(|data| data.insert_temp(egui::Id::new("sidebar-wide"), wide));
    // Home, back and forward share one compact row.
    ui.horizontal(|ui| {
        ui.add_space(2.0);
        let home_color = if page == Page::Home {
            palette.text
        } else {
            palette.secondary
        };
        let home_button = theme::icon_button(
            ui,
            Icon::House,
            22.0,
            home_color,
            palette.text,
            &gettext(locale, "Home"),
        );
        // Right-click Home to start a new playlist.
        egui::Popup::context_menu(&home_button)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    Some(Icon::CirclePlus),
                    &gettext(locale, "Create a playlist"),
                ) {
                    app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
                        name: String::new(),
                        public: false,
                        add_uris: Vec::new(),
                    }));
                }
            });
        if home_button.clicked() {
            app.actions.push(Action::Open(Page::Home));
        }
        ui.add_space(2.0);
        let local_button = theme::icon_button(
            ui,
            Icon::Laptop,
            22.0,
            if page == Page::Local { palette.text } else { palette.secondary },
            palette.text,
            "Local songs (right-click for more)",
        );
        if local_button.clicked() {
            app.actions.push(Action::Open(Page::Local));
        }
        // Right-click: a small menu (nothing opens until it is picked).
        egui::Popup::context_menu(&local_button)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                let has_user = !app.settings.lastfm_user.is_empty();
                if super::widgets::menu_item_enabled(
                    ui,
                    &app.palette,
                    None,
                    "Open my Last.fm profile",
                    has_user,
                ) {
                    app.actions.push(Action::OpenUrl(format!(
                        "https://www.last.fm/user/{}",
                        super::settings::last_fm_part(&app.settings.lastfm_user)
                    )));
                }
            });
        ui.add_space(6.0);
        let can_back = app.can_go_back();
        let back = theme::icon_button(
            ui,
            Icon::ChevronLeft,
            22.0,
            if can_back { palette.secondary } else { palette.dim },
            if can_back { palette.text } else { palette.dim },
            &gettext(locale, "Back"),
        );
        if can_back && back.clicked() {
            app.actions.push(Action::Back);
        }
        let can_forward = app.can_go_forward();
        let forward = theme::icon_button(
            ui,
            Icon::ChevronRight,
            22.0,
            if can_forward { palette.secondary } else { palette.dim },
            if can_forward { palette.text } else { palette.dim },
            &gettext(locale, "Forward"),
        );
        if can_forward && forward.clicked() {
            app.actions.push(Action::Forward);
        }
        // The library's own button and filter live up here with the page
        // buttons. The field says "Filter your library" so it is never mistaken
        // for the global search in the top bar.
        ui.add_space(10.0);
        let icon_label = gettext(locale, "Filter your library");
        let icon = theme::icon_button(
            ui,
            Icon::Library,
            22.0,
            palette.secondary,
            palette.text,
            &icon_label,
        );
        // Right-click: what the library shows, how it is ordered, and create.
        // Kept every frame so the popup is not dropped.
        heading_anchor = Some(icon.clone());
        // Clicking the icon jumps to the filter, which is always on show.
        if icon.clicked() {
            focus_search = true;
        }
        if wide {
            ui.add_space(6.0);
            let room = (ui.available_width() - 6.0).max(60.0);
            let filter_response = super::widgets::search_field(
                ui,
                &palette,
                locale,
                field_id,
                &mut app.library.filter,
                "Filter",
                room,
            );
            super::widgets::drag_window_from_field(ui, &filter_response, "library-filter");
        }
    });
    if !wide {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            let room = (ui.available_width() - 6.0).max(40.0);
            let filter_response = super::widgets::search_field(
                ui,
                &palette,
                locale,
                field_id,
                &mut app.library.filter,
                "Filter",
                room,
            );
            super::widgets::drag_window_from_field(ui, &filter_response, "library-filter");
        });
    }
    // No Search row: the search field in the top bar, beside the back and
    // forward arrows, is the one search, and two of them is one too many.
    ui.add_space(4.0);
    section_rule(ui, &palette);
    ui.add_space(4.0);

    // The shelf is remembered rather than held in the frame's temporary
    // memory: the picker is one button in a menu now, and a list that
    // forgot where it was every launch would be maddening.
    let filter = app.settings.library_shelf;
    let sort = selected_sort(app, filter);
    if focus_search {
        // Asked for by id rather than by response, because the click that
        // opened the field is read after this row is drawn, so the field
        // itself appears on the next frame.
        ui.ctx().memory_mut(|memory| memory.request_focus(field_id));
    }

    if let Some(button) = &library_button {
        library_menu(app, ui, filter, sort, button);
    }
    // The heading's own menu, with the create entry the plus button used to
    // be for. Drawn on every frame like any popup, so an open one stays.
    if let Some(anchor) = &heading_anchor {
        // Opens beside the library panel, never on top of the playlists.
        let panel = ui.max_rect();
        egui::Popup::context_menu(anchor)
            .anchor(pos2(panel.right() + 10.0, panel.top()))
            .align(egui::RectAlign::RIGHT_START)
            .frame(super::widgets::menu_frame(&app.palette))
            .width(250.0)
            .show(|ui| {
                ui.set_max_width(290.0);
                let _ = (filter, sort);
                library_menu_body(app, ui, filter, sort);
            });
    }
    ui.data_mut(|data| {
        data.insert_temp(show_search_id, show_search);
    });

    // Make sure the selected shelf is loading.
    match filter {
        Filter::Playlists => {
            // Warm the other shelves while the reader is still on this one,
            // one after the other, so Artists and Albums open already
            // filled instead of loading when they are clicked. Only the
            // first page of each; the rest still stream in on demand.
            if app.library.playlists.get().is_some() {
                if !app.library.artists.loaded_once
                    && !app.library.artists.loading
                    && app.library.artists.error.is_none()
                {
                    app.actions.push(Action::LoadMore(Page::Artists));
                } else if app.library.artists.loaded_once
                    && !app.library.albums.loaded_once
                    && !app.library.albums.loading
                    && app.library.albums.error.is_none()
                {
                    app.actions.push(Action::LoadMore(Page::Albums));
                }
            }
        }
        Filter::Albums => {
            if !app.library.albums.loading
                && app.library.albums.error.is_none()
                && (!app.library.albums.loaded_once
                    || (sort != LibrarySort::Library && app.library.albums.can_load_more()))
            {
                app.actions.push(Action::LoadMore(Page::Albums));
            }
        }
        Filter::Artists => {
            if !app.library.artists.loading
                && app.library.artists.error.is_none()
                && (!app.library.artists.loaded_once
                    || (sort != LibrarySort::Library && app.library.artists.can_load_more()))
            {
                app.actions.push(Action::LoadMore(Page::Artists));
            }
        }
        Filter::Podcasts => {
            if !app.library.shows.loading
                && app.library.shows.error.is_none()
                && (!app.library.shows.loaded_once
                    || (sort != LibrarySort::Library && app.library.shows.can_load_more()))
            {
                app.actions.push(Action::LoadMore(Page::Podcasts));
            }
        }
    }

    let needle = app.library.filter.trim().to_lowercase();
    let user_id = app.user_id().unwrap_or("").to_string();
    let mut entries: Vec<Entry> = Vec::new();
    let mut loading = false;
    let mut error: Option<String> = None;
    let mut more_page: Option<Page> = None;
    match filter {
        Filter::Playlists => {
            let liked = liked_entry(app);
            if needle.is_empty() || liked.name.to_lowercase().contains(&needle) {
                entries.push(liked);
            }
            let show_folders = sort == LibrarySort::Spotify && needle.is_empty();
            if show_folders {
                folder_rows(app, &user_id, &mut entries);
            }
            match &app.library.playlists {
                Loadable::Loaded(_) if show_folders => {}
                Loadable::Loaded(playlists) => {
                    for (index, playlist) in playlists.iter().enumerate() {
                        if !needle.is_empty() && !playlist.name.to_lowercase().contains(&needle) {
                            continue;
                        }
                        entries.push(playlist_entry(
                            locale,
                            playlist,
                            index,
                            &user_id,
                            app.can_edit_playlist(playlist),
                            0,
                        ));
                    }
                }
                Loadable::Loading | Loadable::NotLoaded => loading = true,
                Loadable::Failed(message) => error = Some(message.clone()),
            }
        }
        Filter::Albums => {
            for saved in &app.library.albums.items {
                let album = &saved.album;
                if !needle.is_empty()
                    && !album.name.to_lowercase().contains(&needle)
                    && !album
                        .artists
                        .iter()
                        .any(|a| a.name.to_lowercase().contains(&needle))
                {
                    continue;
                }
                let artists = crate::api::models::join_names(
                    album.artists.iter().map(|artist| artist.name.as_str()),
                );
                entries.push(Entry {
                    image: pick_image(&album.images, 64).map(str::to_string),
                    grid_image: pick_image(&album.images, super::GRID_ART_TARGET_WIDTH)
                        .map(str::to_string),
                    name: album.name.clone(),
                    subtitle: format!("{} • {artists}", app.album_kind_label(album)),
                    grid_subtitle: artists,
                    page: Page::Album(album.id.clone()),
                    uri: album.uri.clone(),
                    round: false,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: saved_time(saved.added_at.as_deref()),
                    header: false,
                });
            }
            loading = app.library.albums.loading && app.library.albums.items.is_empty();
            error = app.library.albums.error.clone();
            if app.library.albums.error.is_none() && app.library.albums.can_load_more() {
                more_page = Some(Page::Albums);
            }
        }
        Filter::Artists => {
            for artist in &app.library.artists.items {
                if !needle.is_empty() && !artist.name.to_lowercase().contains(&needle) {
                    continue;
                }
                entries.push(Entry {
                    image: pick_image(&artist.images, 64).map(str::to_string),
                    grid_image: pick_image(&artist.images, super::GRID_ART_TARGET_WIDTH)
                        .map(str::to_string),
                    name: artist.name.clone(),
                    subtitle: gettext(locale, "Artist").into_owned(),
                    grid_subtitle: String::new(),
                    page: Page::Artist(artist.id.clone()),
                    uri: artist.uri.clone(),
                    round: true,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: None,
                    header: false,
                });
            }
            loading = app.library.artists.loading && app.library.artists.items.is_empty();
            error = app.library.artists.error.clone();
            if app.library.artists.error.is_none() && app.library.artists.can_load_more() {
                more_page = Some(Page::Artists);
            }
        }
        Filter::Podcasts => {
            for saved in &app.library.shows.items {
                let show = &saved.show;
                // Audiobooks arrive as shows, but librespot can't play them.
                if app.audiobook_shows.contains(&show.uri) {
                    continue;
                }
                if !needle.is_empty() && !show.name.to_lowercase().contains(&needle) {
                    continue;
                }
                entries.push(Entry {
                    image: pick_image(&show.images, 64).map(str::to_string),
                    grid_image: pick_image(&show.images, super::GRID_ART_TARGET_WIDTH)
                        .map(str::to_string),
                    name: show.name.clone(),
                    // Translators: {publisher} is the podcast's publisher.
                    subtitle: gettext(locale, "Podcast • {publisher}")
                        .replace("{publisher}", &show.publisher),
                    grid_subtitle: show.publisher.clone(),
                    page: Page::Show(show.id.clone()),
                    uri: show.uri.clone(),
                    round: false,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: saved_time(saved.added_at.as_deref()),
                    header: false,
                });
            }
            loading = app.library.shows.loading && app.library.shows.items.is_empty();
            error = app.library.shows.error.clone();
            if app.library.shows.error.is_none() && app.library.shows.can_load_more() {
                more_page = Some(Page::Podcasts);
            }
        }
    }

    order_entries(app, filter, sort, &mut entries);
    let custom_order = filter == Filter::Playlists && sort == LibrarySort::Local;
    let pins = app.settings.library_pins();
    let pinned_rows = entries
        .iter()
        .take_while(|entry| pins.iter().any(|key| key == entry.ordering_key()))
        .count();
    // Dividers between categories in the stacked list, when the order has
    // categories to show. They are left out while a row is being dragged
    // (the drop slots count rows) and for the orders that are the reader's
    // own arrangement.
    if !app.settings.sidebar_grid
        && matches!(
            sort,
            LibrarySort::Name | LibrarySort::RecentlyPlayed | LibrarySort::RecentlyAdded
        )
        && entries.len() > 6
        && !egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx())
        && !egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx())
    {
        let mut with_headers: Vec<Entry> = Vec::with_capacity(entries.len() + 12);
        let mut last: Option<String> = None;
        for (position, entry) in std::mem::take(&mut entries).into_iter().enumerate() {
            let label = if position < pinned_rows {
                gettext(locale, "Pinned").into_owned()
            } else {
                category_of(app, sort, &entry, locale)
            };
            if last.as_ref() != Some(&label) {
                with_headers.push(Entry {
                    image: None,
                    grid_image: None,
                    name: label.clone(),
                    subtitle: String::new(),
                    grid_subtitle: String::new(),
                    page: Page::Home,
                    uri: String::new(),
                    round: false,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: None,
                    header: true,
                });
                last = Some(label);
            }
            with_headers.push(entry);
        }
        entries = with_headers;
    }
    let playing_context = app.playing_context_uri();
    let context_playing = app.believed_playing();
    let current_page = app.page().clone();

    crate::autoscroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt("sidebar-list")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .auto_shrink([false, false]),
        egui::Vec2b::new(false, true),
        |ui| {
            if egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx())
                || egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx())
            {
                super::widgets::scroll_during_drag(ui);
            }
            if loading {
                super::widgets::loading_row(ui, &palette, app.locale);
            }
            if let Some(error) = &error {
                super::widgets::error_row(ui, app, error, None);
            }
            if entries.is_empty() && !loading && error.is_none() {
                ui.add_space(12.0);
                theme::subtle(
                    ui,
                    &palette,
                    &if needle.is_empty() {
                        gettext(app.locale, "Nothing here yet.")
                    } else {
                        gettext(app.locale, "No matches.")
                    },
                );
            }
            if app.settings.sidebar_grid {
                library_grid(
                    app,
                    ui,
                    &entries,
                    filter,
                    custom_order,
                    pinned_rows,
                    grid_art,
                );
                if let Some(page) = more_page {
                    super::widgets::load_more_when_near_end(ui, app, page, true);
                }
                ui.add_space(grid_art.map_or(0.0, |rect| ui.max_rect().bottom() - rect.top()));
                return;
            }

            let compact = app.settings.sidebar_compact;
            let row_height = if compact {
                COMPACT_ROW_HEIGHT
            } else {
                DEFAULT_ROW_HEIGHT
            };
            // Calculate drop positions from fixed row height because rows shift
            // before drawing.
            let list_top = ui.cursor().top();
            let pointer = ui.ctx().pointer_latest_pos().filter(|pos| {
                ui.clip_rect().contains(*pos) && ui.rect_contains_pointer(ui.clip_rect())
            });
            // Tracks may drop on Liked Songs or playlists that take songs
            // from this account.
            let dragging_song = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
            let drop_target = dragging_song
                .then_some(pointer)
                .flatten()
                .map(|pos| ((pos.y - list_top) / row_height).floor())
                .filter(|row| *row >= 0.0 && *row < entries.len() as f32)
                .map(|row| row as usize)
                .filter(|row| entries[*row].liked || entries[*row].editable);
            // Sidebar entries, including Liked Songs, drop between rows.
            let reordering = egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx());
            let reorder_slot = reordering.then_some(pointer).flatten().map(|pos| {
                (((pos.y - list_top) / row_height).round().max(0.0) as usize).min(entries.len())
            });
            let art = app.backend.art().clone();
            super::widgets::virtual_rows(ui, entries.len(), row_height, |ui, index| {
                let entry = &entries[index];
                if entry.header {
                    let (_, rect) = ui.allocate_space(vec2(ui.available_width(), row_height));
                    if ui.is_rect_visible(rect) {
                        let text_rect = ui.painter().text(
                            pos2(rect.left() + LIBRARY_ITEM_PADDING, rect.bottom() - 11.0),
                            egui::Align2::LEFT_CENTER,
                            entry.name.to_uppercase(),
                            theme::medium(11.5),
                            palette.dim,
                        );
                        ui.painter().hline(
                            (text_rect.right() + 8.0)..=(rect.right() - 8.0),
                            rect.bottom() - 11.0,
                            egui::Stroke::new(1.0, rule_color(&palette)),
                        );
                    }
                    return;
                }
                if !app.settings.sidebar_compact
                    && let Some(image) = &entry.image
                {
                    // Prepare the enlarged preview before this row is opened.
                    app.softened_covers.texture(ui.ctx(), &art, image);
                }
                let droppable = entry.liked || entry.editable;
                let drop_hover = drop_target == Some(index);
                let active = entry.folder.is_none() && entry.page == current_page;
                // Liked Songs has no URI of its own here; Spotify plays it
                // as the account's collection context.
                let playing =
                    context_playing && entry_is_playing_context(entry, playing_context.as_deref());
                let pinned = pins.iter().any(|key| key == entry.ordering_key());
                let (_, rect) = ui.allocate_space(vec2(ui.available_width(), row_height));
                let id = ui.id().with((
                    "library-row",
                    &entry.uri,
                    entry.liked,
                    entry.folder.as_ref().map(|(id, _, _)| id),
                ));
                let response = ui.interact(rect, id, Sense::click_and_drag());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        active,
                        if let Some((_, collapsed, _)) = &entry.folder {
                            app.locale.folder_state_label(&entry.name, *collapsed)
                        } else {
                            entry.name.clone()
                        },
                    )
                });
                // Animate rows around the current track or entry drop target.
                let shift = ui.ctx().animate_value_with_time(
                    ui.id().with(("drop-shift", index)),
                    if let Some(slot) = reorder_slot {
                        if index < slot { -4.0 } else { 4.0 }
                    } else {
                        match drop_target {
                            Some(target) if index < target => -4.0,
                            Some(target) if index > target => 4.0,
                            _ => 0.0,
                        }
                    },
                    0.12,
                );
                let rect = rect.translate(vec2(0.0, shift));
                // Set when the cover play button takes a click, so a double
                // click on it does not also play from the row.
                let mut cover_took_click = false;
                if ui.is_rect_visible(rect) {
                    if active {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(6), palette.surface);
                    } else if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.surface_hover.gamma_multiply(0.6),
                        );
                    }
                    if drop_hover {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.accent.gamma_multiply(0.18),
                        );
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(6),
                            egui::Stroke::new(1.5, palette.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let name_color = if playing {
                        palette.accent
                    } else {
                        palette.text
                    };
                    let indent = f32::from(entry.depth) * 14.0;
                    if let Some((_, collapsed, _)) = &entry.folder {
                        let chevron = if *collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        };
                        let left = rect.left() + LIBRARY_ITEM_PADDING + indent;
                        chevron.image(palette.secondary, 16.0).paint_at(
                            ui,
                            Rect::from_center_size(
                                pos2(left + 8.0, rect.center().y),
                                Vec2::splat(16.0),
                            ),
                        );
                        Icon::Library.image(palette.secondary, 20.0).paint_at(
                            ui,
                            Rect::from_center_size(
                                pos2(left + 30.0, rect.center().y),
                                Vec2::splat(20.0),
                            ),
                        );
                        let text_left = left + 46.0;
                        let text_right = rect.right() - LIBRARY_ITEM_PADDING;
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y - if compact { 0.0 } else { 9.0 },
                            &entry.name,
                            theme::medium(if compact { 13.5 } else { 14.0 }),
                            name_color,
                        );
                        if !compact {
                            crate::bidi::paint_line(
                                &painter,
                                text_left,
                                text_right,
                                rect.center().y + 10.0,
                                &entry.subtitle,
                                theme::regular(12.5),
                                palette.secondary,
                            );
                        }
                    } else if compact {
                        let text_left = rect.left() + LIBRARY_ITEM_PADDING + indent;
                        let text_right = rect.right()
                            - if playing || pinned {
                                28.0
                            } else {
                                LIBRARY_ITEM_PADDING
                            };
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        // Compact rows have room for one line, and it is always the name.
                        {
                            let size = fitted_name_size(
                                ui,
                                &entry.name,
                                13.5,
                                name_color,
                                text_right - text_left,
                            );
                            crate::bidi::paint_line(
                                &painter,
                                text_left,
                                text_right,
                                rect.center().y,
                                &entry.name,
                                theme::medium(size),
                                name_color,
                            );
                        }
                    } else {
                        // With the detail line off the cover takes the
                        // whole height of the row, and the name grows to fill
                        // the width (and shrinks if it still would not fit).
                        let details_on = app.settings.library_details_entry();
                        let side = if details_on { 44.0 } else { row_height - 6.0 };
                        let cover_rect = Rect::from_center_size(
                            pos2(
                                rect.left() + LIBRARY_ITEM_PADDING + indent + side / 2.0,
                                rect.center().y,
                            ),
                            Vec2::splat(side),
                        );
                        if entry.liked {
                            liked_cover(ui, cover_rect, 6.0);
                        } else {
                            super::widgets::paint_cover(
                                ui,
                                &palette,
                                entry.image.as_deref(),
                                cover_rect,
                                if entry.round { 22.0 } else { 6.0 },
                                if entry.round { Icon::User } else { Icon::Music },
                                Some(app.backend.art()),
                            );
                        }
                        let text_left = cover_rect.right() + 12.0;
                        let text_right = rect.right()
                            - if playing || pinned {
                                28.0
                            } else {
                                LIBRARY_ITEM_PADDING
                            };
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        // The name always draws. The detail line under it — "Playlist • chan",
                        // "Album • Artist" — is what the Details toggle takes
                        // away, and with it gone the name sits centred on its
                        // own rather than riding high over empty space.
                        let below =
                            app.settings.library_details_entry() && !entry.subtitle.is_empty();
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y - if below { 9.0 } else { 0.0 },
                            &entry.name,
                            theme::medium(fitted_name_size(
                                ui,
                                &entry.name,
                                if details_on {
                                    14.0
                                } else if app.settings.library_name_size == 0 {
                                    20.0
                                } else {
                                    f32::from(app.settings.library_name_size)
                                },
                                name_color,
                                text_right - text_left,
                            )),
                            name_color,
                        );
                        if below {
                            crate::bidi::paint_line(
                                &painter,
                                text_left,
                                text_right,
                                rect.center().y + 10.0,
                                &entry.subtitle,
                                theme::regular(12.5),
                                palette.secondary,
                            );
                        }
                        // Hovering the art offers to play right from here.
                        cover_took_click = cover_play_button(
                            app, ui, entry, index, cover_rect, &response, playing,
                        );
                    }
                    if playing {
                        let icon_rect = Rect::from_center_size(
                            pos2(rect.right() - 16.0, rect.center().y),
                            Vec2::splat(16.0),
                        );
                        Icon::Volume2
                            .image(palette.accent, 16.0)
                            .paint_at(ui, icon_rect);
                    } else if pinned {
                        let icon_rect = Rect::from_center_size(
                            pos2(rect.right() - 16.0, rect.center().y),
                            Vec2::splat(13.0),
                        );
                        Icon::Pin
                            .image(palette.secondary, 13.0)
                            .paint_at(ui, icon_rect);
                    }
                    if needs_attention(app, entry) {
                        attention_dot(ui, pos2(rect.right() - 8.0, rect.top() + 9.0));
                    }
                    // Rows that cannot take the song step back a little.
                    if dragging_song && !droppable {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.panel.gamma_multiply(0.5),
                        );
                    }
                }
                finish_entry_interaction(
                    app,
                    ui,
                    &response,
                    entry,
                    cover_took_click,
                    custom_order,
                    true,
                );
            });
            if let Some(slot) = reorder_slot {
                // A line in the gap the rows opened, so the eye lands
                // where the row will.
                let y = list_top + slot as f32 * row_height;
                ui.painter().hline(
                    ui.max_rect().x_range().shrink(6.0),
                    y,
                    egui::Stroke::new(2.0, palette.accent),
                );
                if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
                    && let Some(drag) = egui::DragAndDrop::take_payload::<DragEntry>(ui.ctx())
                {
                    if filter == Filter::Playlists {
                        drop_playlist_row(app, &entries, pinned_rows, slot, &drag.uri);
                    } else {
                        drop_row(app, &entries, pinned_rows, slot, &drag.uri);
                    }
                }
            }
            if let Some(page) = more_page {
                super::widgets::load_more_when_near_end(ui, app, page, true);
            }
        },
    );
}

const EXPANDED_ART_GAP: f32 = 10.0;
const LIBRARY_ITEM_PADDING: f32 = 8.0;
use super::SIDEBAR_MIN_WIDTH;
/// How much of the expanded cover's width the sidebar has to give it,
/// and how much of its height may set the ceiling. The same figure the art
/// itself is measured against, so the two cannot drift apart.
const EXPANDED_ART_WIDTH_FRACTION: f32 = 0.45;

/// The widest the sidebar may be dragged.
///
/// The docked cover stops growing once it runs out of either room, and
/// every pixel past that is a strip of empty panel beside it: the list has
/// already run out of columns and the art cannot get any bigger. The old
/// flat 600 let the bar go well past that point, so the handle could be
/// dragged out to a sidebar holding nothing but background.
///
/// The height is the panel's own, which the sidebar takes from the window,
/// so a shorter window allows a narrower sidebar and a taller one a wider
/// one — the cover is capped by the same height it is drawn against.
fn sidebar_max_width(ui: &egui::Ui) -> f32 {
    // The window's height, not the panel's: raising the visualizer must not
    // lower how wide the sidebar may be.
    let height = ui
        .ctx()
        .input(|input| input.raw.screen_rect)
        .map_or_else(|| ui.available_height(), |rect| rect.height())
        .max(1.0);
    let art = (height * EXPANDED_ART_WIDTH_FRACTION).max(80.0);
    (art + PANEL_MARGIN_LEFT + PANEL_MARGIN_RIGHT).min(SIDEBAR_ABSOLUTE_MAX)
}

/// The art's own ceiling, so a very tall window cannot push the limit past
/// what the panel can usefully show.
const SIDEBAR_ABSOLUTE_MAX: f32 = 600.0;
/// The panel's own inner margin, which the rule reaches across so it
/// touches both edges instead of floating inside the text.
const PANEL_MARGIN_LEFT: f32 = 12.0;
const PANEL_MARGIN_RIGHT: f32 = 8.0;

/// The rule between the navigation and the Library.
///
/// It spans the panel edge to edge. The earlier one stopped short of both
/// sides and was drawn in the outline colour, which against the panel
/// background is very nearly the same value, so it read as a smudge rather
/// than a division. The clip has to be widened with it, or the panel
/// margin would trim the ends back off.
fn section_rule(ui: &egui::Ui, palette: &Palette) {
    let rect = ui.max_rect();
    let full = Rect::from_min_max(
        pos2(rect.left() - PANEL_MARGIN_LEFT, rect.top()),
        pos2(rect.right() + PANEL_MARGIN_RIGHT, rect.top()),
    );
    ui.painter().with_clip_rect(full).hline(
        full.x_range(),
        full.center().y,
        egui::Stroke::new(1.0, rule_color(palette)),
    );
}

/// A visible but quiet division: halfway between the background and the
/// outline, so it reads on the dark panel without competing with the text.
fn rule_color(palette: &Palette) -> egui::Color32 {
    let mix = |from: egui::Color32, to: egui::Color32, at: f32| {
        let blend = |a: u8, b: u8| {
            let value = f32::from(a) + (f32::from(b) - f32::from(a)) * at;
            (value.round().clamp(0.0, 255.0)) as u8
        };
        egui::Color32::from_rgb(
            blend(from.r(), to.r()),
            blend(from.g(), to.g()),
            blend(from.b(), to.b()),
        )
    };
    mix(palette.panel, palette.secondary, 0.22)
}
const GRID_MIN_CARD_WIDTH: f32 = 72.0;
const GRID_MAX_COLUMNS: usize = 8;
/// How much room under each grid card its names take, as a multiple of the
/// name's own size: one line of name plus one of subtitle, with the gaps
/// between and below them. Quoted this way rather than in pixels so the
/// room keeps its proportion when the name grows with the card.
const GRID_TEXT_LINES: f32 = 3.2;
/// How many lines a card's name may wrap onto. The card reserves room for
/// three, so a name is only cut when three lines genuinely cannot hold it.
const GRID_NAME_LINES: usize = 3;

#[derive(Clone, Copy)]
pub(crate) struct GridLayout {
    pub(crate) columns: usize,
    card_width: f32,
    card_height: f32,
    row_height: f32,
}

/// The columns the grid has when the reader has not picked a number: three
/// while the filter box sits beside the library icon (the sidebar is wide
/// enough for that), two once it drops to a row of its own.
fn auto_columns(width: f32) -> usize {
    if width >= 308.0 { 3 } else { 2 }
}

pub(crate) fn grid_layout(width: f32, text_height: f32, pinned: u8) -> GridLayout {
    let columns = if pinned >= 1 {
        (pinned as usize).clamp(1, GRID_MAX_COLUMNS)
    } else {
        auto_columns(width)
    };
    let card_width = (width / columns as f32).max(1.0);
    let card_height = card_width + text_height;
    GridLayout {
        columns,
        card_width,
        card_height,
        row_height: card_height,
    }
}

/// How much room under each grid card its names take.
///
/// Nothing at all when names are off, which is what makes the covers-only
/// grid square. Names are drawn at one size and shrink to fit their own
/// card, so two lines of room is always enough.
fn grid_text_height(app: &App, card_width: f32) -> f32 {
    if !app.settings.library_grid_names {
        return 0.0;
    }
    grid_name_size(card_width) * GRID_TEXT_LINES
}

/// The size a name is drawn at: its full size where it fits, and smaller
/// where it does not, down to a floor no name is allowed under. Fitting
/// each name to its own row beats one size for all of them, because a
/// shelf holds both "Chill" and a forty-character album title.
fn grid_card_width(width: f32, pinned: u8) -> f32 {
    let columns = if pinned >= 1 {
        (pinned as usize).clamp(1, GRID_MAX_COLUMNS)
    } else {
        auto_columns(width)
    };
    (width / columns as f32).max(1.0)
}

pub(crate) fn grid_name_size(card_width: f32) -> f32 {
    // A playlist name is the only text a grid card has, and it was sized
    // so timidly that a four-across library read as a wall of covers with
    // captions. It now takes a real share of the card and grows with it,
    // up to a point where it is a heading rather than a caption.
    (card_width * 0.155).clamp(13.0, 34.0)
}

fn fitted_name_size(ui: &egui::Ui, text: &str, base: f32, color: egui::Color32, width: f32) -> f32 {
    let full = ui
        .painter()
        .layout_no_wrap(text.to_string(), theme::medium(base), color)
        .size()
        .x;
    if full <= width || full <= 0.0 {
        return base;
    }
    (base * width / full).max(base * NAME_FIT_FLOOR)
}

/// A playlist name on a grid card, wrapped over as many lines as it takes.
///
/// This used to lay the name out on one line and end it with an ellipsis,
/// so "EVENT HORIZON 🌌" read as "EVENT …" — which is worse than useless
/// in a library, where the name is the only thing telling two playlists
/// apart and the reader is looking for a word in it. The card has room for
/// three lines, so the name wraps into them and an ellipsis only appears if
/// even three lines cannot hold it.
fn paint_grid_text(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) {
    let galley = crate::bidi::layout(
        ui.painter(),
        text,
        font,
        color,
        rect.width(),
        GRID_NAME_LINES,
        Some(crate::bidi::ELLIPSIS),
    );
    ui.painter()
        .galley(crate::bidi::galley_pos(rect, &galley), galley, color);
}

fn grid_reorder_slot(
    pointer: egui::Pos2,
    origin: egui::Pos2,
    layout: GridLayout,
    count: usize,
) -> usize {
    let row = ((pointer.y - origin.y) / layout.row_height)
        .floor()
        .max(0.0) as usize;
    let column = ((pointer.x - origin.x) / layout.card_width)
        .floor()
        .max(0.0)
        .min((layout.columns - 1) as f32) as usize;
    let after = pointer.x - origin.x - column as f32 * layout.card_width > layout.card_width / 2.0;
    (row * layout.columns + column + usize::from(after)).min(count)
}

fn library_grid(
    app: &mut App,
    ui: &mut egui::Ui,
    entries: &[Entry],
    filter: Filter,
    custom_order: bool,
    pinned_rows: usize,
    grid_art: Option<Rect>,
) {
    if entries.is_empty() {
        return;
    }
    let palette = app.palette;
    let pins = app.settings.library_pins();
    let playing_context = app.playing_context_uri();
    let context_playing = app.believed_playing();
    let current_page = app.page().clone();
    let width = ui.available_width();
    // Three across while the filter sits beside the library icon, two once
    // the sidebar is too narrow for that and it drops to its own row.
    let wide = ui
        .ctx()
        .data(|data| data.get_temp::<bool>(egui::Id::new("sidebar-wide")))
        .unwrap_or(true);
    // At the narrowest the sidebar can be, one cover is all that fits.
    let pinned = if width < 150.0 {
        1
    } else if wide {
        3
    } else {
        2
    };
    let layout = grid_layout(
        width,
        grid_text_height(app, grid_card_width(width, pinned)),
        pinned,
    );
    let origin = ui.cursor().min;
    let pointer = ui.ctx().pointer_latest_pos().filter(|pos| {
        ui.clip_rect().contains(*pos)
            && ui.rect_contains_pointer(ui.clip_rect())
            && !grid_art.is_some_and(|art| art.contains(*pos))
    });
    let dragging_song = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
    let reordering = egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx());
    let reorder_slot = reordering
        .then_some(pointer)
        .flatten()
        .map(|pointer| grid_reorder_slot(pointer, origin, layout, entries.len()));
    let row_count = entries.len().div_ceil(layout.columns);
    let grid_id = ui.unique_id().with("library-grid");

    super::widgets::virtual_rows(ui, row_count, layout.row_height, |ui, row| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let start = row * layout.columns;
            let end = (start + layout.columns).min(entries.len());
            for index in start..end {
                let entry = &entries[index];
                let entry_id = (
                    entry.ordering_key(),
                    entry.liked,
                    entry.folder.as_ref().map(|(id, _, _)| id.as_str()),
                );
                ui.scope_builder(egui::UiBuilder::new().id(grid_id.with(entry_id)), |ui| {
                    let active = entry.folder.is_none() && entry.page == current_page;
                    let playing_here = entry_is_playing_context(entry, playing_context.as_deref());
                    let playing = context_playing && playing_here;
                    let pinned = pins.iter().any(|key| key == entry.ordering_key());
                    let droppable = entry.liked || entry.editable;
                    let (rect, response) = ui.allocate_exact_size(
                        vec2(layout.card_width, layout.card_height),
                        Sense::click_and_drag(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            active,
                            if let Some((_, collapsed, _)) = &entry.folder {
                                app.locale.folder_state_label(&entry.name, *collapsed)
                            } else {
                                entry.name.clone()
                            },
                        )
                    });
                    let cover_rect = Rect::from_min_size(
                        rect.min + Vec2::splat(LIBRARY_ITEM_PADDING),
                        Vec2::splat(layout.card_width - LIBRARY_ITEM_PADDING * 2.0),
                    );
                    let mut cover_took_click = false;
                    if ui.is_rect_visible(rect) {
                        if active {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(6), palette.surface);
                        } else if response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(6),
                                palette.surface_hover.gamma_multiply(0.6),
                            );
                        }
                        if entry.liked {
                            liked_cover(ui, cover_rect, 6.0);
                        } else if entry.folder.is_some() {
                            ui.painter().rect_filled(
                                cover_rect,
                                CornerRadius::same(6),
                                palette.surface,
                            );
                            Icon::Library
                                .image(palette.secondary, layout.card_width * 0.3)
                                .paint_at(
                                    ui,
                                    Rect::from_center_size(
                                        cover_rect.center(),
                                        Vec2::splat(layout.card_width * 0.3),
                                    ),
                                );
                        } else {
                            super::widgets::paint_cover(
                                ui,
                                &palette,
                                entry.grid_image.as_deref().or(entry.image.as_deref()),
                                cover_rect,
                                if entry.round {
                                    layout.card_width / 2.0
                                } else {
                                    6.0
                                },
                                if entry.round { Icon::User } else { Icon::Music },
                                Some(app.backend.art()),
                            );
                        }

                        cover_took_click =
                            grid_play_button(app, ui, entry, cover_rect, &response, playing_here);

                        let text_left = rect.left() + LIBRARY_ITEM_PADDING;
                        let text_width = (rect.width() - LIBRARY_ITEM_PADDING * 2.0).max(0.0);
                        // Covers-only is a separate choice from hiding the detail line, so a card
                        // keeps its name while losing the line under it.
                        let names = app.settings.library_grid_names;
                        let details = names && app.settings.library_details_entry();
                        if names {
                            // Names grow with the card, so a shelf drawn two
                            // across reads as two large titles and a shelf
                            // drawn four across as four small ones, the way
                            // a cover grid is meant to look.
                            let base = grid_name_size(layout.card_width);
                            let line = base * 1.34;
                            let name_color = if playing {
                                palette.accent
                            } else {
                                palette.text
                            };
                            paint_grid_text(
                                ui,
                                Rect::from_min_size(
                                    pos2(text_left, cover_rect.bottom() + 4.0),
                                    vec2(text_width, line),
                                ),
                                &entry.name,
                                theme::medium(fitted_name_size(
                                    ui,
                                    &entry.name,
                                    base,
                                    name_color,
                                    text_width,
                                )),
                                name_color,
                            );
                            if details {
                                paint_grid_text(
                                    ui,
                                    Rect::from_min_size(
                                        pos2(text_left, cover_rect.bottom() + 4.0 + line),
                                        vec2(text_width, line * 0.94),
                                    ),
                                    &entry.grid_subtitle,
                                    theme::regular(base * 0.89),
                                    palette.secondary,
                                );
                            }
                        }

                        if needs_attention(app, entry) {
                            attention_dot(ui, cover_rect.right_top() + vec2(-8.0, 8.0));
                        }
                        if pinned {
                            let pin_rect = grid_pin_rect(cover_rect);
                            ui.painter().circle_filled(
                                pin_rect.center(),
                                pin_rect.width() / 2.0,
                                egui::Color32::from_black_alpha(170),
                            );
                            Icon::Pin.image(egui::Color32::WHITE, 12.0).paint_at(
                                ui,
                                Rect::from_center_size(pin_rect.center(), Vec2::splat(12.0)),
                            );
                        }
                        if dragging_song && !droppable {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(6),
                                palette.panel.gamma_multiply(0.5),
                            );
                        } else if dragging_song
                            && droppable
                            && pointer.is_some_and(|pos| rect.contains(pos))
                        {
                            ui.painter().rect_stroke(
                                cover_rect,
                                CornerRadius::same(6),
                                egui::Stroke::new(2.0, palette.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if reorder_slot == Some(index) {
                            ui.painter().vline(
                                rect.left(),
                                rect.y_range(),
                                egui::Stroke::new(2.0, palette.accent),
                            );
                        }
                        if index + 1 == entries.len() && reorder_slot == Some(entries.len()) {
                            ui.painter().vline(
                                rect.right(),
                                rect.y_range(),
                                egui::Stroke::new(2.0, palette.accent),
                            );
                        }
                    }

                    finish_entry_interaction(
                        app,
                        ui,
                        &response,
                        entry,
                        cover_took_click,
                        custom_order,
                        pointer.is_some(),
                    );
                });
            }
        });
    });

    if let Some(slot) = reorder_slot
        && ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
        && let Some(drag) = egui::DragAndDrop::take_payload::<DragEntry>(ui.ctx())
    {
        if filter == Filter::Playlists {
            drop_playlist_row(app, entries, pinned_rows, slot, &drag.uri);
        } else {
            drop_row(app, entries, pinned_rows, slot, &drag.uri);
        }
    }
}

fn entry_menu(app: &mut App, response: &egui::Response, entry: &Entry, custom_order: bool) {
    if !entry.uri.is_empty() {
        let owned_playlist = entry
            .owned
            .then_some(entry.playlist_index)
            .flatten()
            .and_then(|index| {
                app.library
                    .playlists
                    .get()
                    .and_then(|list| list.get(index))
                    .cloned()
            });
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                super::widgets::context_menu_items(
                    ui,
                    app,
                    &entry.uri,
                    &entry.name,
                    owned_playlist.as_ref(),
                );
                pin_menu(app, ui, entry.ordering_key());
                // A new playlist, from wherever the reader happens to be
                // standing. It was only reachable from the top bar, which
                // is a long walk with the mouse for something done once a
                // month.
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    Some(Icon::CirclePlus),
                    &gettext(app.locale, "Create playlist"),
                ) {
                    app.actions.push(Action::CreatePlaylist {
                        name: String::new(),
                        public: false,
                        add_uris: vec![],
                    });
                    ui.close();
                }
                if custom_order
                    && super::widgets::menu_item(
                        ui,
                        &app.palette,
                        Some(Icon::Clock),
                        &gettext(app.locale, "Sort by recently played"),
                    )
                {
                    app.actions.push(Action::SetLibrarySort {
                        shelf: Filter::Playlists,
                        sort: LibrarySort::RecentlyPlayed,
                    });
                }
            });
    } else if entry.liked {
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                // The same width as every other menu; without it the menu
                // stretches as wide as the window.
                ui.set_min_width(200.0);
                ui.set_max_width(300.0);
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    Some(Icon::Play),
                    &gettext(app.locale, "Play"),
                ) && let Some(user) = &app.user
                {
                    app.actions.push(Action::PlayContext {
                        uri: format!("spotify:user:{}:collection", user.id),
                        offset_uri: None,
                        offset_index: None,
                    });
                }
                pin_menu(app, ui, entry.ordering_key());
            });
    }
}

fn pin_menu(app: &mut App, ui: &mut egui::Ui, key: &str) {
    let mut pins = app.settings.library_pins();
    let pinned = pins.iter().any(|held| held == key);
    let label = if pinned {
        gettext(app.locale, "Unpin")
    } else {
        gettext(app.locale, "Pin to top")
    };
    if super::widgets::menu_item(
        ui,
        &app.palette,
        Some(if pinned { Icon::PinOff } else { Icon::Pin }),
        &label,
    ) {
        if pinned {
            pins.retain(|held| held != key);
        } else {
            pins.push(key.to_string());
        }
        app.actions.push(Action::ArrangeLibrary {
            pinned: pins,
            playlist_order: None,
        });
    }
}

/// Drops within the pin block arrange pins. Below it, a drop unpins the
/// moved row and saves the full playlist arrangement at the chosen gap.
fn drop_playlist_row(app: &mut App, entries: &[Entry], pinned_rows: usize, slot: usize, key: &str) {
    if key != LIKED_SONGS_KEY
        && !app
            .library
            .playlists
            .get()
            .is_some_and(|playlists| playlists.iter().any(|playlist| playlist.uri == key))
    {
        return;
    }
    let was_pinned = app.settings.library_pins().iter().any(|held| held == key);
    if slot < pinned_rows || (was_pinned && slot == pinned_rows) {
        drop_row(app, entries, pinned_rows, slot, key);
        return;
    }
    let mut pins = app.settings.library_pins();
    pins.retain(|held| held != key);
    let mut order = full_playlist_order(app);
    let anchor = entries
        .iter()
        .skip(slot)
        .find_map(|entry| {
            if let Some((id, _, _)) = &entry.folder {
                // A drop before a collapsed folder precedes its first child
                // when switching to the flat local arrangement.
                let start = app.rootlist.iter().position(|row| matches!(row, crate::player::RootlistEntry::FolderStart { id: found, .. } if found == id))?;
                app.rootlist[start + 1..].iter().find_map(|row| match row {
                    crate::player::RootlistEntry::Playlist(held) if held != key && order.contains(held) => Some(held.as_str()),
                    _ => None,
                })
            } else {
                let held = entry.ordering_key();
                (!held.is_empty() && held != key).then_some(held)
            }
        })
        .map(str::to_string);
    order.retain(|held| held != key);
    let at = anchor
        .and_then(|anchor| order.iter().position(|held| *held == anchor))
        .unwrap_or(order.len());
    order.insert(at, key.to_string());
    app.actions.push(Action::ArrangeLibrary {
        pinned: pins,
        playlist_order: Some(order),
    });
}

/// Every unpinned loaded playlist in the selected shelf order, including
/// rows hidden by a search or collapsed folder. A drag snapshots this
/// whole arrangement before applying its new local position.
fn full_playlist_order(app: &App) -> Vec<String> {
    let Some(playlists) = app.library.playlists.get() else {
        return Vec::new();
    };
    let mut entries: Vec<_> = playlists
        .iter()
        .enumerate()
        .map(|(index, playlist)| {
            playlist_entry(
                app.locale,
                playlist,
                index,
                app.user_id().unwrap_or(""),
                false,
                0,
            )
        })
        .collect();
    entries.push(liked_entry(app));
    order_entries(
        app,
        Filter::Playlists,
        selected_sort(app, Filter::Playlists),
        &mut entries,
    );
    let pins = app.settings.library_pins();
    entries
        .iter()
        .map(|entry| entry.ordering_key().to_string())
        .filter(|key| !pins.contains(key))
        .collect()
}

/// Reorders the pin block without disturbing pins on another shelf.
fn drop_row(app: &mut App, entries: &[Entry], pinned_rows: usize, slot: usize, key: &str) {
    let mut pins = app.settings.library_pins();
    pins.retain(|held| held != key);
    if slot <= pinned_rows {
        let anchor = entries[..pinned_rows]
            .iter()
            .skip(slot)
            .map(Entry::ordering_key)
            .find(|held| *held != key);
        let at = anchor
            .and_then(|anchor| pins.iter().position(|held| held == anchor))
            .unwrap_or(pins.len());
        pins.insert(at, key.to_string());
    }
    app.actions.push(Action::ArrangeLibrary {
        pinned: pins,
        playlist_order: None,
    });
}

/// The purple-to-blue Liked Songs tile.
pub fn liked_cover(ui: &egui::Ui, rect: Rect, radius: f32) {
    let texture_id = egui::Id::new("liked-cover-gradient");
    let texture = ui
        .data(|data| data.get_temp::<egui::TextureHandle>(texture_id))
        .unwrap_or_else(|| {
            let size = 64;
            let lerp = |a: u8, b: u8, t: f32| (a as f32 + (b as f32 - a as f32) * t) as u8;
            let top_left = [0x45, 0x0a, 0xf5];
            let top_right = [0x6a, 0x3a, 0xe8];
            let bottom_left = [0x8e, 0x9f, 0xe5];
            let bottom_right = [0xc4, 0xef, 0xd9];
            let pixels = (0..size)
                .flat_map(|y| {
                    let y = y as f32 / (size - 1) as f32;
                    (0..size).map(move |x| {
                        let x = x as f32 / (size - 1) as f32;
                        egui::Color32::from_rgb(
                            lerp(
                                lerp(top_left[0], top_right[0], x),
                                lerp(bottom_left[0], bottom_right[0], x),
                                y,
                            ),
                            lerp(
                                lerp(top_left[1], top_right[1], x),
                                lerp(bottom_left[1], bottom_right[1], x),
                                y,
                            ),
                            lerp(
                                lerp(top_left[2], top_right[2], x),
                                lerp(bottom_left[2], bottom_right[2], x),
                                y,
                            ),
                        )
                    })
                })
                .collect();
            let texture = ui.ctx().load_texture(
                "liked-cover-gradient",
                egui::ColorImage::new([size, size], pixels),
                egui::TextureOptions::LINEAR,
            );
            ui.data_mut(|data| data.insert_temp(texture_id, texture.clone()));
            texture
        });
    egui::Image::new(&texture)
        .corner_radius(CornerRadius::same(radius.min(127.0) as u8))
        .paint_at(ui, rect);
    let size = rect.width() * 0.45;
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    Icon::HeartFilled
        .image(egui::Color32::WHITE, size)
        .paint_at(ui, icon_rect);
}

#[cfg(all(test, feature = "demo"))]
mod ordering_tests {
    use super::*;
    use crate::api::models::Playlist;
    use crate::settings::Settings;

    fn app(name: &str) -> App {
        let root =
            std::env::temp_dir().join(format!("chanceify-order-{name}-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        crate::demo::populate(&mut app);
        app.library.playlists = Loadable::Loaded(
            [
                ("a", "Zebra"),
                ("b", "Alpha"),
                ("c", "alpha"),
                ("d", "Beta"),
            ]
            .into_iter()
            .map(|(id, name)| Playlist {
                id: id.into(),
                name: name.into(),
                uri: format!("spotify:playlist:{id}"),
                ..Default::default()
            })
            .collect(),
        );
        app.recent_contexts = vec![uri("d"), uri("a")];
        app
    }

    fn apply_actions(app: &mut App) {
        for action in std::mem::take(&mut app.actions) {
            app.apply(action, &egui::Context::default());
        }
    }

    fn uri(id: &str) -> String {
        format!("spotify:playlist:{id}")
    }

    fn rows(app: &App) -> Vec<Entry> {
        app.library
            .playlists
            .get()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, playlist)| playlist_entry(Locale::English, playlist, index, "", false, 0))
            .collect()
    }

    fn ids(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.uri.rsplit(':').next().unwrap())
            .collect()
    }

    #[test]
    fn grid_cards_use_smaller_art_than_page_headers() {
        let images = [64, 300, 640]
            .map(|width| crate::api::models::Image {
                url: width.to_string(),
                width: Some(width),
                height: Some(width),
            })
            .to_vec();
        let playlist = Playlist {
            images: images.clone(),
            ..Default::default()
        };
        let entry = playlist_entry(Locale::English, &playlist, 0, "", false, 0);
        assert_eq!(entry.image.as_deref(), Some("64"));
        assert_eq!(entry.grid_image.as_deref(), Some("300"));
        assert_eq!(pick_image(&images, 640), Some("640"));
    }

    #[test]
    fn library_grid_adds_columns_as_the_sidebar_grows() {
        let narrow = grid_layout(230.0, 0.0, 0);
        let medium = grid_layout(360.0, 0.0, 0);
        let wide = grid_layout(500.0, 0.0, 0);
        assert_eq!(narrow.columns, 2);
        assert_eq!(medium.columns, 3);
        assert_eq!(wide.columns, 3);
        for (width, layout) in [(230.0, narrow), (360.0, medium), (500.0, wide)] {
            let filled = layout.card_width * layout.columns as f32;
            assert!((filled - width).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn grid_pin_and_play_controls_keep_opposite_corners() {
        let cover = Rect::from_min_size(pos2(10.0, 20.0), Vec2::splat(GRID_MIN_CARD_WIDTH));
        let pin = grid_pin_rect(cover);
        let play = grid_play_rect(cover);
        assert!(pin.center().x < cover.center().x && pin.center().y < cover.center().y);
        assert!(play.center().x > cover.center().x && play.center().y > cover.center().y);
        assert!(!pin.intersects(play));
    }

    #[test]
    fn grid_drop_positions_follow_visual_row_order() {
        let layout = grid_layout(360.0, 0.0, 0);
        let origin = pos2(20.0, 40.0);
        assert_eq!(grid_reorder_slot(pos2(21.0, 41.0), origin, layout, 8), 0);
        assert_eq!(
            grid_reorder_slot(
                pos2(origin.x + layout.card_width, origin.y + 1.0),
                origin,
                layout,
                8,
            ),
            1
        );
        assert_eq!(
            grid_reorder_slot(
                pos2(origin.x + 1.0, origin.y + layout.row_height + 1.0),
                origin,
                layout,
                8,
            ),
            3
        );
    }

    #[test]
    fn name_and_recent_sorts_keep_pins_and_do_not_rewrite_library_data() {
        let mut app = app("sorts");
        let mut entries = rows(&app);
        order_entries(
            &app,
            Filter::Playlists,
            LibrarySort::RecentlyPlayed,
            &mut entries,
        );
        assert_eq!(ids(&entries), ["d", "a", "b", "c"]);
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        assert_eq!(ids(&entries), ["b", "c", "d", "a"]);
        app.settings.pinned_contexts = vec![uri("a")];
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        assert_eq!(ids(&entries), ["a", "b", "c", "d"]);
        assert_eq!(ids(&rows(&app)), ["a", "b", "c", "d"]);
        app.backend.shutdown();
    }

    /// Duplicate saved keys keep their first rank, while rows absent from
    /// the saved order keep their relative positions without changing source data.
    #[test]
    fn ranked_sorts_keep_first_duplicate_and_stable_order_for_missing_entries() {
        use crate::player::RootlistEntry;

        let mut app = app("ranked-sorts");
        app.settings.liked_songs_pinned = false;
        app.settings.pinned_contexts.clear();
        app.recent_contexts = ["c", "a", "c"].map(uri).to_vec();
        app.settings.sidebar_order = ["c", "a", "c"].map(uri).to_vec();
        app.rootlist = ["c", "a", "c"]
            .map(|id| RootlistEntry::Playlist(uri(id)))
            .to_vec();

        for (sort, expected) in [
            (LibrarySort::RecentlyPlayed, ["c", "a", "b", "d"]),
            (LibrarySort::Local, ["b", "d", "c", "a"]),
            (LibrarySort::Spotify, ["c", "a", "b", "d"]),
        ] {
            let mut entries = rows(&app);
            order_entries(&app, Filter::Playlists, sort, &mut entries);
            assert_eq!(ids(&entries), expected, "{sort:?}");
        }

        app.settings.pinned_contexts = ["c", "a", "c"].map(uri).to_vec();
        let mut entries = rows(&app);
        order_entries(&app, Filter::Playlists, LibrarySort::Library, &mut entries);
        assert_eq!(ids(&entries), ["c", "a", "b", "d"]);
        assert_eq!(ids(&rows(&app)), ["a", "b", "c", "d"]);
        app.backend.shutdown();
    }

    #[test]
    fn switching_sort_and_filtered_drag_preserve_the_full_local_arrangement() {
        let mut app = app("saved");
        app.settings.sidebar_order = ["c", "a", "b", "d"].map(uri).to_vec();
        let saved = app.settings.sidebar_order.clone();
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        assert_eq!(full_playlist_order(&app), ["b", "c", "d", "a"].map(uri));
        assert_eq!(app.settings.sidebar_order, saved);
        app.rootlist = vec![crate::player::RootlistEntry::Playlist(uri("d"))];
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Local);
        assert_eq!(
            full_playlist_order(&app),
            saved,
            "a late rootlist cannot replace the chosen local order"
        );
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        let filtered: Vec<_> = rows(&app)
            .into_iter()
            .filter(|row| row.uri == uri("c") || row.uri == uri("a"))
            .rev()
            .collect();
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        assert_eq!(
            selected_sort(&app, Filter::Playlists),
            LibrarySort::Local,
            "recreating a saved arrangement still leaves the automatic sort"
        );
        app.backend.shutdown();
    }

    #[test]
    fn spotify_order_keeps_nested_folders_and_pins_visible_when_collapsed() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("folders");
        app.rootlist = vec![
            FolderStart {
                id: "outer".into(),
                name: "Outer".into(),
            },
            Playlist(uri("a")),
            FolderStart {
                id: "inner".into(),
                name: "Inner".into(),
            },
            Playlist(uri("b")),
            Playlist(uri("c")),
            FolderEnd,
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("b")),
        ];
        app.collapsed_folders = vec!["inner".into()];
        app.settings.pinned_contexts = vec![uri("b")];
        let mut entries = vec![];
        folder_rows(&app, "", &mut entries);
        order_entries(&app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        assert_eq!(
            entries
                .iter()
                .map(|row| (row.name.as_str(), row.depth))
                .collect::<Vec<_>>(),
            [
                ("Alpha", 0),
                ("Outer", 0),
                ("Zebra", 1),
                ("Inner", 1),
                ("Beta", 0)
            ]
        );
        app.collapsed_folders.clear();
        let mut entries = vec![];
        folder_rows(&app, "", &mut entries);
        order_entries(&app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        assert_eq!(
            entries
                .iter()
                .map(|row| (row.name.as_str(), row.depth))
                .collect::<Vec<_>>(),
            [
                ("Alpha", 0),
                ("Outer", 0),
                ("Zebra", 1),
                ("Inner", 1),
                ("alpha", 2),
                ("Beta", 0)
            ]
        );
        app.backend.shutdown();
    }

    #[test]
    fn dragging_before_a_collapsed_folder_and_unpinning_keep_the_drop_position() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("folder-drop");
        app.rootlist = vec![
            FolderStart {
                id: "folder".into(),
                name: "Folder".into(),
            },
            Playlist(uri("b")),
            Playlist(uri("c")),
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("a")),
        ];
        app.collapsed_folders = vec!["folder".into()];
        let mut entries = vec![];
        folder_rows(&app, "", &mut entries);
        order_entries(&app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        drop_playlist_row(&mut app, &entries, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.sidebar_order, ["a", "b", "c", "d"].map(uri));
        app.settings.sidebar_order.clear();
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        app.settings.pinned_contexts = vec![uri("a")];
        let mut entries = rows(&app);
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        drop_playlist_row(&mut app, &entries, 1, 2, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.library_pins(), [LIKED_SONGS_KEY]);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        app.backend.shutdown();
    }

    #[test]
    fn folder_names_and_subtitles_follow_the_locale() {
        use crate::i18n::Locale;
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("folder-locale");
        app.locale = Locale::German;
        app.rootlist = vec![
            FolderStart {
                id: "folder".into(),
                name: String::new(),
            },
            Playlist(uri("a")),
            Playlist(uri("b")),
            FolderEnd,
        ];
        let mut entries = vec![];
        folder_rows(&app, "", &mut entries);
        assert_eq!(entries[0].name, "Ordner");
        assert_eq!(entries[0].subtitle, "Ordner • 2 Playlists");
        assert_eq!(entries[0].grid_subtitle, "2 Playlists");
        app.backend.shutdown();
    }

    #[test]
    fn added_sort_uses_actual_instants_and_keeps_missing_dates_last() {
        let mut app = app("dates");
        let mut entries = rows(&app);
        for (row, value) in entries.iter_mut().zip([
            Some("2026-09-09T09:00:00Z"),
            Some("2026-09-09T10:00:00+02:00"),
            None,
            Some("unknown"),
        ]) {
            row.added_at = saved_time(value);
        }
        entries.swap(0, 1);
        order_entries(
            &app,
            Filter::Albums,
            LibrarySort::RecentlyAdded,
            &mut entries,
        );
        assert_eq!(ids(&entries), ["a", "b", "c", "d"]);
        assert!(!LibrarySort::RecentlyAdded.supports(Filter::Playlists));
        assert!(!LibrarySort::RecentlyAdded.supports(Filter::Artists));
        assert!(LibrarySort::RecentlyAdded.supports(Filter::Podcasts));
        app.backend.shutdown();
    }

    #[test]
    fn preferences_round_trip_and_new_playlists_still_precede_saved_order() {
        let mut app = app("migration");
        app.settings = serde_json::from_str(
            r#"{"sidebar_order":["spotify:playlist:c","spotify:playlist:a","spotify:playlist:b"]}"#,
        )
        .unwrap();
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        assert_eq!(selected_sort(&app, Filter::Albums), LibrarySort::Library);
        assert_eq!(full_playlist_order(&app), ["d", "c", "a", "b"].map(uri));
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        app.settings
            .library_sort
            .insert(Filter::Albums, LibrarySort::RecentlyAdded);
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&app.settings).unwrap()).unwrap();
        assert_eq!(restored, app.settings);
        app.backend.shutdown();
    }

    #[test]
    fn dropping_at_the_end_of_the_pin_block_keeps_the_item_pinned() {
        let mut app = app("pin-boundary");
        app.settings.pinned_contexts = [uri("d"), uri("a")].to_vec();
        let mut entries = rows(&app);
        entries.push(liked_entry(&app));
        order_entries(
            &app,
            Filter::Playlists,
            selected_sort(&app, Filter::Playlists),
            &mut entries,
        );

        drop_playlist_row(&mut app, &entries, 3, 3, LIKED_SONGS_KEY);
        apply_actions(&mut app);

        assert_eq!(
            app.settings.library_pins(),
            [uri("d"), uri("a"), LIKED_SONGS_KEY.into()]
        );
        app.backend.shutdown();
    }

    #[test]
    fn liked_songs_moves_among_pins_and_retains_its_local_position() {
        let mut app = app("liked-position");
        app.settings.pinned_contexts = [uri("d"), uri("a")].to_vec();
        let ordered = |app: &App| {
            let mut entries = rows(app);
            entries.push(liked_entry(app));
            order_entries(
                app,
                Filter::Playlists,
                selected_sort(app, Filter::Playlists),
                &mut entries,
            );
            entries
        };
        let entries = ordered(&app);
        assert_eq!(entries[0].ordering_key(), LIKED_SONGS_KEY);
        drop_playlist_row(&mut app, &entries, 3, 2, LIKED_SONGS_KEY);
        assert_eq!(
            app.settings.library_pins()[0],
            LIKED_SONGS_KEY,
            "the view only emits an action"
        );
        apply_actions(&mut app);
        assert_eq!(
            app.settings.library_pins(),
            [uri("d"), LIKED_SONGS_KEY.into(), uri("a")]
        );
        assert!(app.settings.sidebar_order.is_empty());
        let entries = ordered(&app);
        drop_playlist_row(&mut app, &entries, 3, 4, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert!(!app.settings.liked_songs_pinned);
        let saved = [uri("b"), LIKED_SONGS_KEY.into(), uri("c")];
        assert_eq!(full_playlist_order(&app), saved);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        assert_eq!(
            full_playlist_order(&app),
            [uri("b"), uri("c"), LIKED_SONGS_KEY.into()]
        );
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Local);
        app.rootlist = vec![crate::player::RootlistEntry::Playlist(uri("c"))];
        app.user.as_mut().unwrap().id = "another-account".into();
        app.settings =
            serde_json::from_str(&serde_json::to_string(&app.settings).unwrap()).unwrap();
        assert_eq!(
            full_playlist_order(&app),
            saved,
            "restart and account/rootlist changes preserve the local placement"
        );
        let filtered: Vec<_> = ordered(&app)
            .into_iter()
            .filter(|entry| entry.uri == uri("c") || entry.uri == uri("b"))
            .collect();
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("c"));
        apply_actions(&mut app);
        assert_eq!(
            full_playlist_order(&app),
            [uri("c"), uri("b"), LIKED_SONGS_KEY.into()],
            "a hidden Liked Songs keeps its place in the full arrangement"
        );
        assert!(
            liked_entry(&app).uri.is_empty(),
            "the ordering key is never a Spotify URI"
        );
        app.backend.shutdown();
    }

    #[test]
    fn unpinned_liked_songs_follows_recent_play_and_spotify_order_without_entering_folders() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("liked-sorts");
        app.settings.liked_songs_pinned = false;
        app.recent_contexts = vec![
            uri("a"),
            format!("spotify:user:{}:collection", app.user_id().unwrap()),
            uri("c"),
        ];
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::RecentlyPlayed);
        assert_eq!(
            full_playlist_order(&app),
            [
                uri("a"),
                LIKED_SONGS_KEY.into(),
                uri("c"),
                uri("b"),
                uri("d")
            ]
        );
        app.rootlist = vec![
            FolderStart {
                id: "folder".into(),
                name: "Folder".into(),
            },
            Playlist(uri("c")),
            FolderEnd,
            Playlist(uri("a")),
        ];
        app.collapsed_folders = vec!["folder".into()];
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        let mut entries = vec![liked_entry(&app)];
        folder_rows(&app, "", &mut entries);
        order_entries(&app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        assert!(entries.last().unwrap().liked);
        assert_eq!(entries.last().unwrap().depth, 0);
        drop_playlist_row(&mut app, &entries, 0, 0, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert_eq!(
            full_playlist_order(&app),
            [
                LIKED_SONGS_KEY.into(),
                uri("c"),
                uri("a"),
                uri("b"),
                uri("d")
            ]
        );
        let mut entries = rows(&app);
        entries.push(liked_entry(&app));
        let end = entries.len();
        drop_playlist_row(&mut app, &entries, 0, end, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert_eq!(full_playlist_order(&app).last().unwrap(), LIKED_SONGS_KEY);
        app.backend.shutdown();
    }
}
