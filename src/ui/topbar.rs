//! Navigation arrows, search, and the account menu above every page.

use std::sync::Arc;

use egui::{Align, CornerRadius, Galley, Layout, Sense, Vec2, pos2, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Page};
use crate::theme::{self, Icon, Palette};

/// The gap the bar keeps between everything it lays out.
const ITEM_SPACING: f32 = 8.0;
/// The account avatar, and the icon in each of the three buttons beside it.
const AVATAR_SIZE: f32 = 36.0;
const ICON_BUTTON_ICON: f32 = 19.0;
/// `theme::icon_button` pads its icon by 12 px.
const ICON_BUTTON_SIZE: f32 = ICON_BUTTON_ICON + 12.0;
const SPINNER_SIZE: f32 = 15.0;
/// A badge is as tall as its text plus this, and as wide as its text plus
/// the padding its own label needs.
const BADGE_PADDING_Y: f32 = 12.0;
const DEVICE_BADGE_PADDING: f32 = 28.0;
/// The text starts 24 px in; leave 8 px after it to match the space before
/// the icon.
const UPDATE_BADGE_PADDING: f32 = 32.0;
/// The width the search field aims for, the most it ever takes, and the
/// least it shrinks to before the badges give up their labels instead.
const SEARCH_IDEAL: f32 = 200.0;
const SEARCH_MAX: f32 = 440.0;
const SEARCH_FLOOR: f32 = 130.0;
// After the badges collapse, a right panel can leave less than 130 points.
// Keep the original 80-point minimum inside the page's own toolbar.
const SEARCH_MIN: f32 = 80.0;
/// Everything at the right end whose width never changes: the page padding,
/// the avatar, the gap the account menu leaves, the three icon buttons, and
/// the spacing between them. The cursor stops at the left edge of the last
/// button, so this counts three gaps, not four. The spinner and the badges
/// are measured on top of it because they come and go.
const RIGHT_CONTROLS_WIDTH: f32 =
    super::widgets::PAGE_PADDING + AVATAR_SIZE + 4.0 + ITEM_SPACING;

/// What precedes the field until the bar has drawn once: the page padding,
/// the back and forward buttons and the gaps after them.
const LEAD_GUESS: f32 = super::widgets::PAGE_PADDING + 2.0 * 32.0 + 3.0 * ITEM_SPACING + 8.0;
/// A badge collapsed to its icon: a square as tall as its 12.5 pt label.
const BADGE_CHIP: f32 = 15.0 + BADGE_PADDING_Y;

fn lead_id() -> egui::Id {
    egui::Id::new("topbar-lead")
}

/// The narrowest the bar, and so the page under it, can be before its
/// controls run into each other: the narrowest field, with the spinner and
/// both badges as icons. Counting them even while they are away keeps the
/// panels and the window from changing width as they come and go.
pub fn least_width(ctx: &egui::Context) -> f32 {
    let lead = ctx
        .data(|data| data.get_temp(lead_id()))
        .unwrap_or(LEAD_GUESS);
    least_width_after(lead)
}

fn least_width_after(lead: f32) -> f32 {
    lead + SEARCH_MIN
        + RIGHT_CONTROLS_WIDTH
        + SPINNER_SIZE
        + ITEM_SPACING
        + 2.0 * (ITEM_SPACING + BADGE_CHIP)
}

/// How the top bar divides itself for one window width.
#[derive(Clone, Copy, Debug, PartialEq)]
struct TopbarFit {
    /// How wide the search field may be.
    search: f32,
    /// Whether the badges have the room to spell themselves out.
    labels: bool,
}

/// Divide the bar. The search field keeps the half it has always had, but
/// never so much that the right end has to reach over it, and the badges
/// fall back to their icons before the field shrinks past reading size.
///
/// `labelled` and `icons` are what the badges ask for with and without their
/// text, each already including the spacing that precedes it.
fn topbar_fit(room: f32, controls: f32, labelled: f32, icons: f32) -> TopbarFit {
    // SEARCH_IDEAL is above SEARCH_FLOOR, so the clamp below is well ordered.
    // The field takes the whole bar, up to the controls on the right.
    let ideal = room.max(SEARCH_IDEAL);
    let labels = room - controls - labelled >= SEARCH_FLOOR;
    let badges = if labels { labelled } else { icons };
    TopbarFit {
        search: (room - controls - badges).clamp(SEARCH_MIN, ideal),
        labels,
    }
}

/// What a badge asks of the bar, including the spacing before it.
fn badge_width(galley: Option<&Arc<Galley>>, padding: f32, labels: bool) -> f32 {
    galley.map_or(0.0, |galley| {
        ITEM_SPACING
            + if labels {
                galley.size().x + padding
            } else {
                galley.size().y + BADGE_PADDING_Y
            }
    })
}

/// A pill at the right end of the bar: an icon with its label, or the icon
/// alone once the bar is too narrow to spare the room for words.
fn badge(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    galley: Arc<Galley>,
    padding: f32,
    labels: bool,
) -> egui::Response {
    let height = galley.size().y + BADGE_PADDING_Y;
    let size = if labels {
        vec2(galley.size().x + padding, height)
    } else {
        Vec2::splat(height)
    };
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    // Collapsed to its icon the badge has no text on screen, so its label
    // reaches a screen reader as the widget's name.
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), galley.text())
    });
    ui.painter().rect_filled(
        rect,
        CornerRadius::same(14),
        palette.accent.gamma_multiply(0.16),
    );
    let icon_center = if labels {
        pos2(rect.left() + 14.0, rect.center().y)
    } else {
        rect.center()
    };
    icon.image(palette.accent, 13.0).paint_at(
        ui,
        egui::Rect::from_center_size(icon_center, Vec2::splat(13.0)),
    );
    if labels {
        ui.painter().galley(
            pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
            galley,
            palette.accent,
        );
    }
    response
}

fn nav_button(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    enabled: bool,
    tooltip: &str,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::splat(32.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if ui.is_rect_visible(rect) {
        // Solid, so the page scrolling underneath never shows through.
        let fill = if response.hovered() && enabled {
            palette.surface_hover
        } else {
            palette.surface
        };
        ui.painter().circle_filled(rect.center(), 16.0, fill);
        let color = if !enabled {
            palette.dim
        } else if response.hovered() {
            palette.text
        } else {
            palette.secondary
        };
        theme::paint_icon(ui, icon, rect, 20.0, color);
    }
    if enabled {
        response.on_hover_text(tooltip)
    } else {
        response
    }
}

/// How tall the bar is for the width `ui` has, so the page can scroll
/// underneath it.
pub fn height(app: &App, ui: &egui::Ui) -> f32 {
    let reservation = super::window_controls_reservation(
        ui.ctx(),
        app.show_queue_panel,
        app.show_lyrics_panel,
        ui.available_width(),
    );
    theme::TOP_BAR_HEIGHT + theme::titlebar_inset(ui.ctx()) + reservation.topbar_top
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let width = ui.available_width();
    let window_controls = super::window_controls_reservation(
        ui.ctx(),
        app.show_queue_panel,
        app.show_lyrics_panel,
        width,
    );
    // Where the titlebar used to be: the bar grows upwards into that space and
    // its empty parts drag the window.
    let inset = theme::titlebar_inset(ui.ctx());
    let content_height = theme::TOP_BAR_HEIGHT + inset;
    if crate::window::custom_titlebar() {
        let rect = egui::Rect::from_min_size(
            ui.cursor().min,
            vec2(width, content_height + window_controls.topbar_top),
        );
        super::titlebar_drag(ui, rect);
    }
    ui.add_space(window_controls.topbar_top);
    ui.allocate_ui_with_layout(
        vec2(width, content_height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.add_space(super::widgets::PAGE_PADDING);
            ui.spacing_mut().item_spacing.x = ITEM_SPACING;
            if !app.settings.sidebar_visible {
                if nav_button(
                    ui,
                    &palette,
                    Icon::PanelLeft,
                    true,
                    &gettext(locale, "Show sidebar"),
                )
                .clicked()
                {
                    app.actions.push(Action::ToggleSidebar);
                }
                ui.add_space(2.0);
            }
            if !app.settings.sidebar_visible {
                let home = nav_button(ui, &palette, Icon::House, true, &gettext(locale, "Home"));
                // Right-click Home to start a new playlist.
                egui::Popup::context_menu(&home)
                    .frame(super::widgets::menu_frame(&palette))
                    .show(|ui| {
                        if super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::CirclePlus),
                            &gettext(locale, "Create a playlist"),
                        ) {
                            app.actions
                                .push(Action::ShowDialog(crate::model::Dialog::CreatePlaylist {
                                    name: String::new(),
                                    public: false,
                                    add_uris: Vec::new(),
                                }));
                        }
                    });
                if home.clicked() {
                    app.actions.push(Action::Open(Page::Home));
                }
            }
            // With the sidebar showing, back and forward live there.
            if !app.settings.sidebar_visible {
                if nav_button(
                    ui,
                    &palette,
                    Icon::ChevronLeft,
                    app.can_go_back(),
                    &gettext(locale, "Back"),
                )
                .clicked()
                {
                    app.actions.push(Action::Back);
                }
                if nav_button(
                    ui,
                    &palette,
                    Icon::ChevronRight,
                    app.can_go_forward(),
                    &gettext(locale, "Forward"),
                )
                .clicked()
                {
                    app.actions.push(Action::Forward);
                }
            }
            // The Views disc: always here, left of the playing-from cover.
            super::views_panel::topbar_disc(ui, app);
            ui.add_space(6.0);
            // The cover of the playlist that is playing, left of the search
            // field: a glance says where the music is coming from, and a
            // click goes there.
            if let Some(uri) = app.playing_context_uri()
                && let Some(id) = uri.strip_prefix("spotify:playlist:")
            {
                let known = app
                    .known_playlist(id)
                    .map(|playlist| {
                        (
                            crate::api::models::pick_image(&playlist.images, 64).map(str::to_string),
                            playlist.name.clone(),
                        )
                    });
                if let Some((url, name)) = known {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::Vec2::splat(32.0), Sense::click());
                    super::widgets::paint_cover(
                        ui,
                        &palette,
                        url.as_deref(),
                        rect,
                        5.0,
                        Icon::Music,
                        Some(app.backend.art()),
                    );
                    if response.clicked() {
                        app.actions.push(Action::Open(Page::Playlist(id.to_string())));
                    }
                    response.on_hover_text(name);
                }
            }
            // Playing from Liked Songs gets its own cover in the same place.
            if app
                .playing_from()
                .is_some_and(|from| matches!(from.page, Some(Page::LikedSongs)))
            {
                let (rect, response) =
                    ui.allocate_exact_size(egui::Vec2::splat(32.0), Sense::click());
                super::sidebar::liked_cover(ui, rect, 5.0);
                if response.clicked() {
                    app.actions.push(Action::Open(Page::LikedSongs));
                }
                response.on_hover_text(gettext(locale, "Liked Songs").as_ref());
            }
            ui.add_space(8.0);

            // The badges sit at the right end but grow with their text, so
            // measure them here, before the search field takes its share.
            let device_galley = app.now_playing().filter(|now| !now.local).map(|now| {
                let label = match now.device_name {
                    Some(device) => {
                        // Translators: {device} is the name of the device playing the music.
                        gettext(locale, "Playing on {device}").replace("{device}", &device)
                    }
                    None => gettext(locale, "Playing on another device").into_owned(),
                };
                ui.painter()
                    .layout_no_wrap(label, theme::medium(12.5), palette.accent)
            });
            let update = app.update.clone();
            let update_galley = update.as_ref().map(|update| {
                let label = match &app.update_download {
                    crate::updates::DownloadState::Ready(_) => {
                        gettext(locale, "Update ready").into_owned()
                    }
                    crate::updates::DownloadState::Downloading { .. } => {
                        gettext(locale, "Downloading update…").into_owned()
                    }
                    _ => {
                        // Translators: {version} is a version number such as 1.2.0.
                        gettext(locale, "Update to {version}").replace("{version}", &update.version)
                    }
                };
                ui.painter()
                    .layout_no_wrap(label, theme::medium(12.5), palette.accent)
            });
            // Ask once, so the bar reserves room for exactly the spinner it
            // then draws.
            // The "Waiting for Spotify" spinner is off. It lit whenever any
            // Spotify request had been open for a second, and the library's
            // background walk keeps one open nearly all the time while
            // Spotify throttles it, so it spun for ever whether or not
            // anything the reader asked for was slow. The traffic is still
            // counted in `app.backend.activity()` should it be wanted back.
            let busy = false;
            let badges = |labels: bool| {
                badge_width(device_galley.as_ref(), DEVICE_BADGE_PADDING, labels)
                    + badge_width(update_galley.as_ref(), UPDATE_BADGE_PADDING, labels)
            };
            let controls = RIGHT_CONTROLS_WIDTH
                + if busy {
                    SPINNER_SIZE + ITEM_SPACING
                } else {
                    0.0
                };

            let search_room = (ui.available_width() - window_controls.topbar_width).max(0.0);
            // What sits before the field: the page padding, the navigation
            // buttons, and any window buttons. The panels beside the page
            // keep room for it (`least_width`).
            let lead = width - ui.available_width() + window_controls.topbar_width;
            ui.ctx().data_mut(|data| data.insert_temp(lead_id(), lead));
            let fit = topbar_fit(search_room, controls, badges(true), badges(false));
            let search_width = fit.search;
            let id = egui::Id::new("global-search");
            let before = app.search.query.clone();
            let response = super::widgets::search_field(
                ui,
                &palette,
                app.locale,
                id,
                &mut app.search.query,
                &gettext(locale, "What do you want to play?"),
                search_width,
            );
            // Dragging the search field (without a click first) moves the
            // window, as the rest of the bar does; a plain click still
            // starts typing. Once the field has focus, dragging selects text.
            if !response.has_focus()
                && crate::window::custom_titlebar()
                && cfg!(any(target_os = "macos", windows))
            {
                let grab = ui.interact(
                    response.rect,
                    egui::Id::new("global-search-window-drag"),
                    Sense::click_and_drag(),
                );
                if grab.drag_started_by(egui::PointerButton::Primary) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                // A plain click (no drag) steps into the field.
                if grab.clicked() || grab.double_clicked() {
                    response.request_focus();
                }
            }
            if app.search.focus_requested {
                app.search.focus_requested = false;
                response.request_focus();
            }
            // Clear empties the field and hands it focus in the same frame.
            // Neither should leave the page: only typing a query does.
            let cleared = app.search.query.is_empty() && !before.is_empty();
            if response.gained_focus() && !cleared && !matches!(app.page(), Page::Search) {
                app.actions.push(Action::Open(Page::Search));
            }
            if app.search.query != before {
                app.search.typed_at = Some(std::time::Instant::now());
                if !cleared && !matches!(app.page(), Page::Search) {
                    app.actions.push(Action::Open(Page::Search));
                }
            }
            if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                let query = app.search.query.clone();
                app.actions.push(Action::Search(query));
            }
            if response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
                response.surrender_focus();
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(window_controls.topbar_width);
                ui.add_space(super::widgets::PAGE_PADDING);
                // Account.
                let (name, avatar) = app
                    .user
                    .as_ref()
                    .or(app.optimistic_user.as_ref())
                    .map(|user| {
                        (
                            user.name().to_string(),
                            pick_image(&user.images, 64).map(str::to_string),
                        )
                    })
                    .unwrap_or_default();
                let (rect, response) =
                    ui.allocate_exact_size(Vec2::splat(AVATAR_SIZE), Sense::click());
                ui.ctx()
                    .data_mut(|data| data.insert_temp(egui::Id::new("tour-account"), rect));
                if ui.is_rect_visible(rect) {
                    let fill = if response.hovered() {
                        palette.surface_hover
                    } else {
                        palette.surface
                    };
                    ui.painter().circle_filled(rect.center(), 18.0, fill);
                    let inner = egui::Rect::from_center_size(rect.center(), Vec2::splat(28.0));
                    match avatar.as_deref() {
                        Some(url) => super::widgets::paint_cover(
                            ui,
                            &palette,
                            Some(url),
                            inner,
                            14.0,
                            Icon::User,
                            Some(app.backend.art()),
                        ),
                        None => {
                            let initial = name
                                .chars()
                                .next()
                                .unwrap_or('?')
                                .to_uppercase()
                                .to_string();
                            ui.painter()
                                .circle_filled(inner.center(), 14.0, palette.accent);
                            ui.painter().text(
                                inner.center(),
                                egui::Align2::CENTER_CENTER,
                                initial,
                                theme::bold(13.0),
                                palette.on_accent,
                            );
                        }
                    }
                }
                // A small red dot when songs in your playlists can no longer
                // be played; Settings lists them.
                let missing = {
                    let user = app.user.as_ref().map(|u| u.id.clone()).unwrap_or_default();
                    app.unavailable.entries_for(&user).len()
                };
                if missing > 0 && ui.is_rect_visible(rect) {
                    let dot = rect.right_top() + Vec2::new(-6.0, 6.0);
                    ui.painter().circle_filled(dot, 6.0, palette.window);
                    ui.painter().circle_filled(dot, 4.5, palette.danger);
                }
                let hover = if missing > 0 {
                    format!("{name}\n{missing} songs need attention (Settings, Unavailable songs)")
                } else {
                    name.clone()
                };
                let response = response.on_hover_text(hover);
                egui::Popup::menu(&response)
                    .frame(super::widgets::menu_frame(&palette))
                    .align(egui::RectAlign::BOTTOM_END)
                    .show(|ui| {
                        ui.set_width(200.0);
                        ui.add_space(4.0);
                        let product = app.user.as_ref().and_then(|user| user.product.clone());
                        ui.horizontal(|ui| {
                            ui.add_space(10.0);
                            theme::text(ui, &name, theme::semibold(14.0), palette.text);
                            if let Some(product) = product {
                                // Plan on the same line, right-anchored, in the
                                // chanceify purple that melts into the theme accent.
                                let label = product.to_lowercase();
                                let font = theme::semibold(12.0);
                                let from = egui::Color32::from_rgb(150, 90, 255);
                                let to = palette.accent;
                                let count = label.chars().count().max(2) - 1;
                                let mut job = egui::text::LayoutJob::default();
                                for (index, letter) in label.chars().enumerate() {
                                    job.append(
                                        &letter.to_string(),
                                        0.0,
                                        egui::text::TextFormat::simple(
                                            font.clone(),
                                            from.lerp_to_gamma(to, index as f32 / count as f32),
                                        ),
                                    );
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add_space(10.0);
                                        ui.add(egui::Label::new(job).selectable(false));
                                    },
                                );
                            }
                        });
                        super::widgets::menu_separator(ui, &palette);
                        if super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::Settings),
                            &gettext(locale, "Settings"),
                        ) {
                            app.actions.push(Action::Open(Page::Settings));
                        }
                        if super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::Info),
                            &gettext(locale, "Keyboard shortcuts"),
                        ) {
                            app.actions
                                .push(Action::ShowDialog(crate::model::Dialog::Shortcuts));
                        }
                        super::widgets::menu_separator(ui, &palette);
                        if super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::LogOut),
                            &gettext(locale, "Sign out"),
                        ) {
                            app.actions.push(Action::SignOut);
                        }
                    });
                // A quiet spinner once the app has been talking to Spotify for a
                // while, long enough that fast requests never flash it.
                if busy {
                    theme::spinner(ui, SPINNER_SIZE, palette.secondary)
                        .on_hover_text(gettext(locale, "Waiting for Spotify…").as_ref());
                }
                // Where playback is.
                if let Some(galley) = device_galley {
                    let device = galley.text().to_owned();
                    let response = badge(
                        ui,
                        &palette,
                        Icon::Speaker,
                        galley,
                        DEVICE_BADGE_PADDING,
                        fit.labels,
                    );
                    // Without its label the badge still has to say where
                    // playback went.
                    let response = if fit.labels {
                        response
                    } else {
                        response.on_hover_text(device)
                    };
                    // The device list opens from this badge, at the top.
                    ui.ctx().data_mut(|data| {
                        data.insert_temp(egui::Id::new(super::devices::BUTTON_RECT_ID), response.rect)
                    });
                    if response.clicked() {
                        app.actions.push(Action::ToggleDevicesPopup);
                    }
                }
                // A newer release. Most people never visit a releases page,
                // so the app says so, quietly, until they do.
                if let (Some(galley), Some(update)) = (update_galley, update)
                    && badge(
                        ui,
                        &palette,
                        Icon::Info,
                        galley,
                        UPDATE_BADGE_PADDING,
                        fit.labels,
                    )
                    .on_hover_text(
                        // Translators: {version} is a version number such as 1.2.0.
                        gettext(locale, "Version {version} is available.")
                            .replace("{version}", &update.version),
                    )
                    .clicked()
                {
                    app.actions.push(Action::ShowUpdate);
                }
            });
        },
    );
}

#[cfg(test)]
mod topbar_fit_tests {
    use super::*;

    // What the badges measure on a bar showing "Playing on MacBook de Luis"
    // and "Update to 0.7.1", each including the spacing before it.
    const DEVICE: f32 = ITEM_SPACING + 176.0;
    const UPDATE: f32 = ITEM_SPACING + 152.0;
    // Collapsed, a badge is a square chip as tall as its text.
    const CHIP: f32 = ITEM_SPACING + 15.0 + BADGE_PADDING_Y;

    /// The narrowest bar the app can produce: a 760 px window, its sidebar,
    /// and the navigation buttons all taken out.
    const NARROWEST_BAR: f32 = 398.0;

    fn right_end(room: f32, labelled: f32, icons: f32) -> f32 {
        let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, labelled, icons);
        let badges = if fit.labels { labelled } else { icons };
        RIGHT_CONTROLS_WIDTH + badges - (room - fit.search)
    }

    #[test]
    fn a_wide_bar_keeps_the_field_it_always_had() {
        let fit = topbar_fit(2000.0, RIGHT_CONTROLS_WIDTH, DEVICE + UPDATE, CHIP * 2.0);
        assert_eq!(fit.search, SEARCH_MAX);
        assert!(fit.labels);
        // Half the room, as before, while half still fits.
        let fit = topbar_fit(700.0, RIGHT_CONTROLS_WIDTH, 0.0, 0.0);
        assert_eq!(fit.search, 350.0);
    }

    #[test]
    fn the_right_end_never_reaches_over_the_search_field() {
        let mut room = NARROWEST_BAR;
        while room <= 2400.0 {
            for (labelled, icons) in [
                (0.0, 0.0),
                (DEVICE, CHIP),
                (UPDATE, CHIP),
                (DEVICE + UPDATE, CHIP * 2.0),
            ] {
                let over = right_end(room, labelled, icons);
                assert!(
                    over <= 0.0,
                    "badges overlap the field by {over} px on a {room} px bar"
                );
            }
            room += 1.0;
        }
    }

    #[test]
    fn a_right_panel_can_narrow_search_after_the_badges_collapse() {
        let room = RIGHT_CONTROLS_WIDTH + CHIP * 2.0 + 100.0;
        let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, DEVICE + UPDATE, CHIP * 2.0);
        assert!(!fit.labels);
        assert_eq!(fit.search, 100.0);
        assert_eq!(right_end(room, DEVICE + UPDATE, CHIP * 2.0), 0.0);
    }

    #[test]
    fn a_narrow_bar_trades_the_badge_labels_for_their_icons() {
        assert!(topbar_fit(1400.0, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP).labels);
        // The 1080 px window of the report that started this.
        assert!(topbar_fit(952.0, RIGHT_CONTROLS_WIDTH, DEVICE + UPDATE, CHIP * 2.0).labels);
        assert!(!topbar_fit(NARROWEST_BAR, RIGHT_CONTROLS_WIDTH, DEVICE, CHIP).labels);
    }

    /// At the least width the panels leave it, the bar still holds the
    /// spinner and both badges beside the narrowest field (#624).
    #[test]
    fn the_least_width_holds_every_control_beside_the_field() {
        let lead = LEAD_GUESS;
        let room = least_width_after(lead) - lead;
        let controls = RIGHT_CONTROLS_WIDTH + SPINNER_SIZE + ITEM_SPACING;
        let fit = topbar_fit(room, controls, DEVICE + UPDATE, CHIP * 2.0);
        assert!(!fit.labels);
        assert_eq!(fit.search, SEARCH_MIN);
        assert!(controls + CHIP * 2.0 + fit.search <= room);
    }

    #[test]
    fn the_field_stays_readable_however_tight_the_bar_gets() {
        let mut room = NARROWEST_BAR;
        while room <= 2400.0 {
            let fit = topbar_fit(room, RIGHT_CONTROLS_WIDTH, DEVICE + UPDATE, CHIP * 2.0);
            assert!(fit.search >= SEARCH_FLOOR, "field is {} px", fit.search);
            assert!(fit.search <= SEARCH_MAX);
            room += 1.0;
        }
    }
}
