//! The Settings page.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Stroke, Vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::{gettext, ngettext, pgettext};
use crate::model::{Action, Page};
use crate::settings::{LanguageChoice, ProxyMode, ThemeChoice};
use crate::theme::{self, Icon, Palette};

use super::widgets;

const PLAYBACK_DIRTY_ID: &str = "playback-settings-dirty";
pub(crate) const PERSONAL_APP_FOCUS_ID: &str = "focus-personal-app-setup";
const SETTINGS_FILTER_ID: &str = "settings-filter";

/// Whether a settings row matches the filter query (case-insensitive).
/// An empty query matches everything, so the page reads exactly as
/// before when the search field is empty.
pub(crate) fn row_matches(needle: &str, title: &str, description: &str) -> bool {
    let needle = needle.trim().to_lowercase();
    if needle.is_empty() {
        return true;
    }
    title.to_lowercase().contains(&needle) || description.to_lowercase().contains(&needle)
}

/// Text shared by section selection and the rendered row. Conditional rows
/// participate only on the platforms and account states where they exist.
struct RowText<'a> {
    title: std::borrow::Cow<'a, str>,
    description: std::borrow::Cow<'a, str>,
    available: bool,
    /// The second line is shown (a status), not hidden as an explanation.
    keep: bool,
}

impl<'a> RowText<'a> {
    fn new(
        title: impl Into<std::borrow::Cow<'a, str>>,
        description: impl Into<std::borrow::Cow<'a, str>>,
    ) -> Self {
        Self {
            title: title.into(),
            description: description.into(),
            available: true,
            keep: false,
        }
    }

    fn noted(mut self) -> Self {
        self.keep = true;
        self
    }

    fn when(mut self, available: bool) -> Self {
        self.available = available;
        self
    }

    fn matches(&self, needle: &str, section: &str) -> bool {
        self.available
            && (needle.is_empty()
                || section.to_lowercase().contains(needle)
                || row_matches(needle, &self.title, &self.description))
    }
}

/// The guide to writing a palette file for the themes folder.
const THEMES_GUIDE_URL: &str = "https://madlygeeked.github.io/chanceify/make-a-theme/";

fn section_matches(needle: &str, title: &str, rows: &[RowText<'_>]) -> bool {
    let needle = needle.trim().to_lowercase();
    rows.iter().any(|row| row.matches(&needle, title))
}

fn filtered_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    needle: &str,
    section: &str,
    row: &RowText<'_>,
    control: impl FnOnce(&mut egui::Ui),
) {
    if row.matches(needle, section) {
        if row.keep {
            widgets::setting_row_note(ui, palette, &row.title, &row.description, control);
        } else {
            widgets::setting_row(ui, palette, &row.title, &row.description, control);
        }
    }
}

/// `filtered_row` for a control that needs `control_width` points.
fn filtered_row_sized(
    ui: &mut egui::Ui,
    palette: &Palette,
    needle: &str,
    section: &str,
    row: &RowText<'_>,
    control_width: f32,
    control: impl FnOnce(&mut egui::Ui),
) {
    if row.matches(needle, section) {
        widgets::setting_row_sized(
            ui,
            palette,
            &row.title,
            &row.description,
            control_width,
            control,
        );
    }
}

/// Fills a settings slider's rail up to the handle with the accent. The
/// rail keeps the theme's colours and rounding: egui carries that fill past
/// the handle's centre by the widgets' corner radius, so a pill radius here
/// would show the accent beyond the handle.
fn slider_rail_style(style: &mut egui::Style, palette: &Palette) {
    style.visuals.selection.bg_fill = palette.accent;
    style.visuals.slider_trailing_fill = true;
}

/// Shapes a settings slider's value box like the pill-shaped buttons in the
/// other settings rows.
fn slider_value_style(style: &mut egui::Style, palette: &Palette) {
    style.visuals.widgets.inactive.bg_fill = palette.surface;
    style.visuals.widgets.hovered.bg_fill = palette.surface_hover;
    style.visuals.widgets.active.bg_fill = palette.surface_active;
    let pill = CornerRadius::same(14);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = pill;
    }
    style
        .text_styles
        .insert(egui::TextStyle::Button, theme::medium(theme::BODY_TEXT));
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
}

/// A settings slider with its value box beside it, where egui's own slider
/// would put it. The two are drawn apart so each keeps its own rounding;
/// `slider` and `value` set up each part for `number` over `range`.
fn setting_slider<N: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    palette: &Palette,
    number: &mut N,
    range: std::ops::RangeInclusive<N>,
    slider: impl FnOnce(egui::Slider<'_>) -> egui::Slider<'_>,
    value: impl FnOnce(egui::DragValue<'_>) -> egui::DragValue<'_>,
) -> egui::Response {
    ui.horizontal(|ui| {
        let moved = ui
            .scope(|ui| {
                slider_rail_style(ui.style_mut(), palette);
                ui.add(slider(egui::Slider::new(number, range.clone())).show_value(false))
            })
            .inner;
        let typed = ui
            .scope(|ui| {
                slider_value_style(ui.style_mut(), palette);
                ui.add(value(egui::DragValue::new(number).range(range)))
            })
            .inner;
        moved | typed
    })
    .inner
}

/// Forget the search text, so a flow that lands on a specific row (like
/// the Personal App setup) always finds that row visible and focusable.
pub(crate) fn clear_search(ctx: &egui::Context) {
    ctx.data_mut(|data| data.remove::<String>(egui::Id::new(SETTINGS_FILTER_ID)));
}
const PROXY_DIRTY_ID: &str = "proxy-settings-dirty";

/// The tabs along the top of the page.
const TABS: [&str; 6] = ["Accounts", "Playback", "Look", "Library", "Network", "About"];

/// Which tab a section sits in. A section that is not listed always shows.
fn tab_of(title: &str) -> Option<u8> {
    let title = title.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| title.contains(word));
    if has(&["account", "last.fm", "discord"]) {
        Some(0)
    } else if has(&["playback", "equalizer"]) {
        Some(1)
    } else if has(&["appearance", "song lists", "gradient", "app icon", "lights", "font"]) {
        Some(2)
    } else if has(&["storage", "unavailable"]) {
        Some(3)
    } else if has(&["proxy"]) {
        Some(4)
    } else if has(&["about", "tour"]) {
        Some(5)
    } else {
        None
    }
}

fn section(
    ui: &mut egui::Ui,
    palette: &Palette,
    title: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let (searching, tab) = ui.data(|data| {
        (
            data.get_temp::<bool>(egui::Id::new("settings-searching"))
                .unwrap_or(false),
            data.get_temp::<u8>(egui::Id::new("settings-tab")).unwrap_or(0),
        )
    });
    if !searching
        && let Some(wanted) = tab_of(title)
        && wanted != tab
    {
        return;
    }
    ui.add_space(10.0);
    // A heading that folds the section away; which ones are folded is kept.
    let closed_id = egui::Id::new("settings-closed");
    let mut closed: Vec<String> = ui.data(|data| data.get_temp(closed_id)).unwrap_or_default();
    let is_closed = !searching && closed.iter().any(|name| name == title);
    let head = ui
        .horizontal(|ui| {
            let (slot, _) = ui.allocate_exact_size(egui::vec2(20.0, 24.0), egui::Sense::hover());
            theme::paint_icon(
                ui,
                if is_closed { Icon::ChevronRight } else { Icon::ChevronDown },
                slot,
                16.0,
                palette.secondary,
            );
            theme::text(ui, crate::i18n::ui_text(title), theme::bold(theme::SECTION_TEXT), palette.text);
        })
        .response;
    let head_click = ui
        .interact(head.rect, egui::Id::new(("settings-head", title)), egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if head_click.clicked() && !searching {
        if is_closed {
            closed.retain(|name| name != title);
        } else {
            closed.push(title.to_string());
        }
        ui.data_mut(|data| data.insert_temp(closed_id, closed));
    }
    if is_closed {
        ui.add_space(6.0);
        return;
    }
    ui.add_space(8.0);
    Frame::new()
        .fill(
            palette
                .surface
                .gamma_multiply(if palette.dark { 0.7 } else { 1.0 }),
        )
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(theme::RADIUS + 2))
        .inner_margin(Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(760.0));
            add_contents(ui);
        });
    ui.add_space(8.0);
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    // Which sections are folded away is remembered between launches.
    let closed_id = egui::Id::new("settings-closed");
    if ui.data(|data| data.get_temp::<Vec<String>>(closed_id)).is_none() {
        let saved = app.settings.settings_closed.clone();
        ui.data_mut(|data| data.insert_temp(closed_id, saved));
    }
    show_inner(app, ui);
    let now: Vec<String> = ui.data(|data| data.get_temp(closed_id)).unwrap_or_default();
    if now != app.settings.settings_closed {
        app.settings.settings_closed = now;
        app.mark_settings_dirty();
    }
}

fn show_inner(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    ui.add_space(8.0);
    theme::text(
        ui,
        gettext(locale, "Settings"),
        theme::bold(theme::TITLE_TEXT),
        palette.text,
    );
    ui.add_space(4.0);
    let mut filter = ui
        .data(|data| data.get_temp::<String>(egui::Id::new(SETTINGS_FILTER_ID)))
        .unwrap_or_default();
    widgets::search_field(
        ui,
        &palette,
        app.locale,
        egui::Id::new("settings-search-field"),
        &mut filter,
        &gettext(locale, "Search settings"),
        ui.available_width().min(400.0),
    );
    let needle = filter.trim().to_lowercase();
    ui.data_mut(|data| data.insert_temp(egui::Id::new(SETTINGS_FILTER_ID), filter));
    ui.add_space(4.0);
    {
        let tab_id = egui::Id::new("settings-tab");
        let mut tab = ui.data(|data| data.get_temp::<u8>(tab_id)).unwrap_or(0);
        ui.data_mut(|data| {
            data.insert_temp(egui::Id::new("settings-searching"), !needle.is_empty());
        });
        if needle.is_empty() {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for (index, name) in TABS.iter().enumerate() {
                    if theme::soft_button(ui, &palette, None, &crate::i18n::ui_text(name), tab == index as u8).clicked() {
                        tab = index as u8;
                    }
                }
            });
            ui.data_mut(|data| data.insert_temp(tab_id, tab));
            ui.add_space(4.0);
        }
    }
    let dirty_id = egui::Id::new(PLAYBACK_DIRTY_ID);
    let mut playback_dirty = ui
        .data(|data| data.get_temp::<bool>(dirty_id))
        .unwrap_or(false);
    let proxy_dirty_id = egui::Id::new(PROXY_DIRTY_ID);
    let mut proxy_dirty = ui
        .data(|data| data.get_temp::<bool>(proxy_dirty_id))
        .unwrap_or(false);
    let mut changed = false;
    let mut any_visible = false;
    let open_folder = gettext(locale, "Open folder");

    let wanted = app
        .settings
        .web_client_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .map(str::to_string);
    let in_use = wanted
        .as_deref()
        .is_some_and(|wanted| app.web_app.as_deref() == Some(wanted));
    let account = gettext(locale, "Account");
    let sign_out = gettext(locale, "Sign out");
    let account_rows = [
        RowText::new(
            gettext(locale, "Personal Spotify app"),
            gettext(
                locale,
                "Use a personal Development Mode app for a separate API quota. The shared app stays active.",
            ),
        ),
        RowText::new(
            gettext(locale, "Create an app"),
            gettext(
                locale,
                "Create one for free in Spotify's developer dashboard.",
            ),
        )
        .when(!in_use),
        RowText::new(
            gettext(locale, "Personal app ready"),
            gettext(
                locale,
                "Supported requests use your app. Other requests use the shared app.",
            ),
        )
        .when(in_use),
        RowText::new(
            gettext(locale, "Authorize your personal app"),
            gettext(
                locale,
                "Spotify opens in your browser to verify the account.",
            ),
        )
        .when(!in_use && wanted.is_some()),
        RowText::new(
            gettext(locale, "Remove personal app"),
            gettext(locale, "Shared access remains signed in."),
        )
        .when(!in_use && wanted.is_none() && app.web_app.is_some()),
        RowText::new(sign_out.clone(), account.clone()),
    ];
    if section_matches(&needle, &account, &account_rows) {
        any_visible = true;
        section(ui, &palette, &account, |ui| {
            if row_matches(&needle, &format!("{sign_out} {account}"), "") {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    let avatar = app
                        .user
                        .as_ref()
                        .and_then(|user| pick_image(&user.images, 64).map(str::to_string));
                    widgets::cover(ui, &palette, avatar.as_deref(), 56.0, 28.0, Icon::User);
                    ui.vertical(|ui| {
                        let name = app
                            .user
                            .as_ref()
                            .map(|user| user.name().to_string())
                            .unwrap_or_default();
                        theme::text(ui, name, theme::semibold(theme::SECTION_TEXT), palette.text);
                        let product = app
                            .user
                            .as_ref()
                            .and_then(|user| user.product.clone())
                            .map(|product| match product.as_str() {
                                "premium" => "Spotify Premium".to_string(),
                                "free" | "open" => {
                                    gettext(locale, "Spotify Free, local playback needs Premium")
                                        .into_owned()
                                }
                                other => other.to_string(),
                            })
                            .unwrap_or_default();
                        theme::text(ui, product, theme::regular(theme::BODY_TEXT), palette.secondary);
                        if let Some(username) =
                            app.local.connected.then(|| app.local.username.clone())
                            && !username.is_empty()
                        {
                            theme::text(
                                ui,
                                // Translators: {username} is the Spotify account's user name.
                                gettext(locale, "Connected as {username}")
                                    .replace("{username}", &username),
                                theme::regular(theme::SMALL_TEXT),
                                palette.dim,
                            );
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::pill_button(ui, &palette, &sign_out, false).clicked() {
                            app.actions.push(Action::SignOut);
                        }
                    });
                });
                ui.add_space(10.0);
            }
            let mut client_id = app.settings.web_client_id.clone().unwrap_or_default();
            filtered_row(ui, &palette, &needle, &account, &account_rows[0], |ui| {
                let response = Frame::new()
                    .fill(palette.surface)
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        widgets::text_edit(
                            ui,
                            app.locale,
                            egui::TextEdit::singleline(&mut client_id)
                                .id(egui::Id::new("personal-web-client-id"))
                                .hint_text(
                                    egui::RichText::new(gettext(locale, "Client ID"))
                                        .color(palette.dim),
                                )
                                .font(theme::regular(theme::BODY_TEXT))
                                .frame(egui::Frame::NONE)
                                .desired_width(200.0),
                        )
                    })
                    .inner;
                if ui
                    .data_mut(|data| data.remove_temp::<bool>(egui::Id::new(PERSONAL_APP_FOCUS_ID)))
                    .unwrap_or(false)
                {
                    response.scroll_to_me(Some(Align::Center));
                    response.request_focus();
                }
                if response.changed() {
                    let trimmed = client_id.trim().to_string();
                    app.settings.web_client_id = (!trimmed.is_empty()).then_some(trimmed);
                    changed = true;
                }
            });
            filtered_row(ui, &palette, &needle, &account, &account_rows[1], |ui| {
                if theme::pill_button(ui, &palette, &gettext(locale, "Setup guide"), false)
                    .clicked()
                {
                    app.actions.push(Action::OpenUrl(
                        "https://madlygeeked.github.io/chanceify/#setup".into(),
                    ));
                }
            });
            if in_use {
                filtered_row(ui, &palette, &needle, &account, &account_rows[2], |ui| {
                    if theme::pill_button(ui, &palette, &gettext(locale, "Remove"), false).clicked()
                    {
                        app.settings.web_client_id = None;
                        app.actions.push(Action::ConfigurePersonalWebApp);
                    }
                });
            } else if wanted.is_some() {
                filtered_row(ui, &palette, &needle, &account, &account_rows[3], |ui| {
                    if theme::pill_button(ui, &palette, &gettext(locale, "Authorize"), true)
                        .clicked()
                    {
                        app.actions.push(Action::ConfigurePersonalWebApp);
                    }
                });
            } else if app.web_app.is_some() {
                filtered_row(ui, &palette, &needle, &account, &account_rows[4], |ui| {
                    if theme::pill_button(ui, &palette, &gettext(locale, "Remove"), false).clicked()
                    {
                        app.actions.push(Action::ConfigurePersonalWebApp);
                    }
                });
            }
            if needle.is_empty() && !in_use {
                ui.add_space(8.0);
                theme::text(ui, "How to connect your own Spotify app (Premium)", theme::semibold(theme::BODY_TEXT), palette.text);
                for line in [
                    "1. Open developer.spotify.com/dashboard and log in with your Spotify account.",
                    "2. Press Create app. Any name and description will do.",
                    "3. Under Redirect URIs add exactly: http://127.0.0.1:8989/login",
                    "4. Tick Web API, accept the terms and press Save.",
                    "5. Open the app's Settings and copy the Client ID. Paste it in the box above, then press Authorize.",
                ] {
                    note(ui, &palette, line);
                }
                if theme::soft_button(ui, &palette, Some(Icon::ExternalLink), "Open the Spotify dashboard", false).clicked() {
                    app.actions.push(Action::OpenUrl("https://developer.spotify.com/dashboard".into()));
                }
            }
        });
    }

    let (status, detail, action) = match &app.local_playback {
        crate::backend::LocalPlayback::Ready { .. } => (
            pgettext(locale, "playback status", "Ready"),
            std::borrow::Cow::Borrowed(""),
            None,
        ),
        crate::backend::LocalPlayback::Authorizing => (
            pgettext(locale, "playback status", "Setting up"),
            gettext(locale, "Finish authorizing in your browser."),
            None,
        ),
        crate::backend::LocalPlayback::Connecting => (
            pgettext(locale, "playback status", "Connecting"),
            gettext(locale, "Connecting to Spotify…"),
            None,
        ),
        crate::backend::LocalPlayback::Failed(message) => (
            pgettext(locale, "playback status", "Unavailable"),
            message.clone().into(),
            Some(gettext(locale, "Try again")),
        ),
        crate::backend::LocalPlayback::Unavailable => (
            pgettext(locale, "playback status", "Not set up"),
            gettext(
                locale,
                "Requires Spotify Premium and a one-time browser sign-in.",
            ),
            Some(gettext(locale, "Enable playback here")),
        ),
    };
    let playback = gettext(locale, "Playback on this computer");
    let normalize_volume = gettext(locale, "Normalize volume");
    let autoplay = gettext(locale, "Autoplay");
    let gapless = gettext(locale, "Gapless playback");
    let update_checks = gettext(locale, "Automatic update checks");
    let audio_cache = gettext(locale, "Audio cache");
    let apply_playback = gettext(locale, "Apply and restart playback");
    let apply_playback_note = gettext(locale, "Restart local playback to apply these settings.");
    let download_updates = gettext(locale, "Download updates automatically");
    let playback_rows = [
        RowText::new(
            // Translators: {status} is a playback state such as Ready or Not set up.
            gettext(locale, "Status: {status}").replace("{status}", &status),
            detail,
        )
        .noted(),
        RowText::new(
            gettext(locale, "Device name"),
            gettext(locale, "How this computer appears in Spotify Connect."),
        )
        .when(false),
        RowText::new(
            gettext(locale, "Audio quality"),
            gettext(locale, "Higher bitrates use more data and cache space."),
        ),
        RowText::new(
            normalize_volume.clone(),
            gettext(locale, "Keep loud and quiet tracks at a similar level."),
        ),
        RowText::new(
            autoplay.clone(),
            gettext(locale, "Keep playing similar songs when your music ends."),
        ),
        RowText::new(
            gapless.clone(),
            gettext(locale, "Play tracks without silence between them."),
        ),
        RowText::new(
            update_checks.clone(),
            gettext(locale, "Checks GitHub once a day. No personal data is sent."),
        )
        .when(false),
        RowText::new(
            gettext(locale, "Audio output"),
            gettext(
                locale,
                "PulseAudio also covers PipeWire. Rodio talks to ALSA directly.",
            ),
        )
        .when(cfg!(target_os = "linux")),
        RowText::new(
            gettext(locale, "Output buffer"),
            gettext(
                locale,
                "Your computer keeps a small amount of music waiting just ahead of what you hear, like a safety cushion. At 200 ms the cushion is big: the music will not crackle or skip even when the computer is busy (a game, a download), but pressing Pause or changing the volume is heard about a fifth of a second late. At 50 ms the cushion is tiny: Pause and volume react almost instantly, but on a busy or slow computer you may hear crackles or tiny skips. If you hear no problems, 100 ms is a good middle. Go up if you hear crackling; go down only if the controls feel slow.",
            ),
        )
        .when(cfg!(windows)),
        RowText::new(
            audio_cache.clone(),
            gettext(
                locale,
                "Keeps songs you have played on this computer, so replaying one never downloads it again. The slider is the most disk space it may use (2 GB holds roughly 400 songs). When it is full, the oldest songs are deleted. It lives in the chanceify folder beside the program.",
            ),
        ),
        RowText::new(apply_playback.clone(), apply_playback_note.clone()).when(playback_dirty),
        RowText::new(gettext(locale, "Playback settings applied"), "").when(!playback_dirty),
        RowText::new(
            download_updates.clone(),
            gettext(
                locale,
                "Downloads in the background. You choose when to restart.",
            ),
        )
        .when(false),
    ];
    if section_matches(&needle, &playback, &playback_rows) {
        any_visible = true;
        section(ui, &palette, &playback, |ui| {
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[0], |ui| {
                if let Some(label) = action {
                    if theme::pill_button(ui, &palette, &label, true).clicked() {
                        app.actions.push(Action::EnablePlayback);
                    }
                } else if app.local_ready
                    && theme::soft_button(
                        ui,
                        &palette,
                        Some(Icon::Refresh),
                        &gettext(locale, "Reconnect"),
                        false,
                    )
                    .clicked()
                {
                    app.actions.push(Action::RestartEngine);
                }
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[1], |ui| {
                let response = Frame::new()
                    .fill(palette.surface)
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        widgets::text_edit(
                            ui,
                            locale,
                            egui::TextEdit::singleline(&mut app.settings.device_name)
                                .font(theme::regular(theme::BODY_TEXT))
                                .frame(egui::Frame::NONE)
                                .desired_width(200.0),
                        )
                    })
                    .inner;
                if response.changed() {
                    changed = true;
                    playback_dirty = true;
                }
            });
            // Normal to Very high, left to right when side by side and top
            // to bottom in a column.
            let choices = [
                (96u16, gettext(locale, "Normal · 96 kbps")),
                (160, gettext(locale, "High · 160 kbps")),
                (320, gettext(locale, "Very high · 320 kbps")),
            ];
            let choice_gap = 6.0;
            let choices_width = choices
                .iter()
                .map(|(_, label)| theme::soft_button_width(ui, label))
                .sum::<f32>()
                + choice_gap * (choices.len() - 1) as f32;
            filtered_row_sized(
                ui,
                &palette,
                &needle,
                &playback,
                &playback_rows[2],
                choices_width,
                |ui| {
                    let mut choose = |ui: &mut egui::Ui, kbps: u16, label: &str| {
                        if theme::soft_button(
                            ui,
                            &palette,
                            None,
                            label,
                            app.settings.bitrate == kbps,
                        )
                        .clicked()
                            && app.settings.bitrate != kbps
                        {
                            app.settings.bitrate = kbps;
                            changed = true;
                            playback_dirty = true;
                        }
                    };
                    if ui.available_width() >= choices_width {
                        // Laid right to left, so the last choice goes first.
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = choice_gap;
                            for (kbps, label) in choices.iter().rev() {
                                choose(ui, *kbps, label);
                            }
                        });
                    } else {
                        // Too narrow even for a line of their own: a column.
                        ui.with_layout(Layout::top_down(Align::Max), |ui| {
                            ui.spacing_mut().item_spacing.y = choice_gap;
                            for (kbps, label) in &choices {
                                choose(ui, *kbps, label);
                            }
                        });
                    }
                },
            );
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[10], |ui| {
                // The control area lays out right-to-left: add the rightmost item first.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if widgets::switch(ui, &palette, &audio_cache, &mut app.settings.audio_cache)
                        .on_hover_text("Keeps songs you have played on this computer, so replaying one never downloads it again. The slider is the most disk space it may use (2 GB holds roughly 400 songs). When it is full, the oldest songs are deleted.")
                        .changed()
                    {
                        changed = true;
                        playback_dirty = true;
                    }
                    if app.settings.audio_cache {
                        ui.add_space(6.0);
                        let mut mb = app.settings.audio_cache_mb as f64;
                        let response = ui.add(
                            egui::Slider::new(&mut mb, 128.0..=51200.0)
                                .logarithmic(true)
                                .custom_formatter(|value, _| {
                                    if value >= 1024.0 {
                                        format!("{:.1} GB", value / 1024.0)
                                    } else {
                                        format!("{value:.0} MB")
                                    }
                                })
                                .custom_parser(|text| {
                                    let text = text.trim().to_lowercase();
                                    let number: f64 = text
                                        .trim_end_matches(|c: char| c.is_alphabetic() || c == ' ')
                                        .parse()
                                        .ok()?;
                                    Some(if text.ends_with("gb") || text.ends_with('g') {
                                        number * 1024.0
                                    } else {
                                        number
                                    })
                                }),
                        );
                        let response = response.on_hover_text("Keeps songs you have played on this computer, so replaying one never downloads it again. The slider is the most disk space it may use (2 GB holds roughly 400 songs). When it is full, the oldest songs are deleted.");
                        if response.changed() {
                            app.settings.audio_cache_mb = mb.round().max(128.0) as u64;
                            changed = true;
                            playback_dirty = true;
                        }
                    }
                });
            });
            #[cfg(windows)]
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[9], |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let current = app.settings.audio_buffer_ms;
                    for ms in [50u32, 100, 200] {
                        let label = format!("{ms} ms");
                        let tip = match ms {
                            50 => "50 ms: a tiny cushion of music. Pause and volume react almost instantly, but a busy or slow computer may crackle or skip.",
                            100 => "100 ms: the good middle. Controls feel quick and the music holds steady on most computers.",
                            _ => "200 ms: a big cushion. The music will not crackle or skip even when the computer is busy, but Pause and volume are heard about a fifth of a second late.",
                        };
                        if theme::soft_button(ui, &palette, None, &label, current == ms)
                            .on_hover_text(tip)
                            .clicked()
                            && current != ms
                        {
                            app.settings.audio_buffer_ms = ms;
                            changed = true;
                            playback_dirty = true;
                        }
                    }
                });
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[3], |ui| {
                if widgets::switch(
                    ui,
                    &palette,
                    &normalize_volume,
                    &mut app.settings.normalisation,
                )
                .changed()
                {
                    changed = true;
                    playback_dirty = true;
                }
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[4], |ui| {
                if widgets::switch(ui, &palette, &autoplay, &mut app.settings.autoplay).changed() {
                    changed = true;
                    playback_dirty = true;
                }
            });
            filtered_row(ui, &palette, &needle, &playback, &playback_rows[5], |ui| {
                if widgets::switch(ui, &palette, &gapless, &mut app.settings.gapless).changed() {
                    changed = true;
                    playback_dirty = true;
                }
            });
            if cfg!(target_os = "linux") {
                filtered_row(ui, &palette, &needle, &playback, &playback_rows[8], |ui| {
                    let current = app
                        .settings
                        .platform_backend()
                        .unwrap_or_else(|| "rodio".into());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        for backend in ["rodio", "pulseaudio"] {
                            let label = if backend == "pulseaudio" {
                                "PulseAudio / PipeWire"
                            } else {
                                "ALSA (rodio)"
                            };
                            if theme::soft_button(ui, &palette, None, label, current == backend)
                                .clicked()
                                && current != backend
                            {
                                app.settings.audio_backend = Some(backend.to_string());
                                changed = true;
                                playback_dirty = true;
                            }
                        }
                    });
                });
            }
            if needle.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    theme::text(
                        ui,
                        "Crossfade between my own song files",
                        theme::regular(theme::BODY_TEXT),
                        palette.text,
                    );
                    let current = app.settings.crossfade_secs;
                    for seconds in [0u8, 2, 4, 6, 8, 12] {
                        let label = if seconds == 0 {
                            "Off".to_string()
                        } else {
                            format!("{seconds} s")
                        };
                        if theme::soft_button(ui, &palette, None, &label, current == seconds).clicked()
                            && current != seconds
                        {
                            app.settings.crossfade_secs = seconds;
                            changed = true;
                        }
                    }
                });
                theme::subtle(
                    ui,
                    &palette,
                    "Only for songs from your own files. It starts with the next list you play.",
                );
            }
            ui.add_space(4.0);
            if playback_dirty
                || playback_rows[11].matches(&needle, &playback)
                || playback_rows[12].matches(&needle, &playback)
            {
                ui.horizontal(|ui| {
                    if playback_dirty {
                        if theme::pill_button(ui, &palette, &apply_playback, true).clicked() {
                            app.actions.push(Action::RestartEngine);
                            playback_dirty = false;
                        }
                        theme::subtle(ui, &palette, &apply_playback_note);
                    } else {
                        theme::subtle(ui, &palette, &gettext(locale, "Playback settings applied."));
                    }
                });
            }
        });
    }

    let appearance = gettext(locale, "Appearance");
    let theme_title = gettext(locale, "Theme");
    let theme_guide = gettext(locale, "How to make a theme");
    let themes_folder = gettext(locale, "Open themes folder");
    let middle_click = gettext(locale, "Middle-click autoscroll");
    let appearance_rows = [
        RowText::new(theme_title.clone(), {
            let detail = theme::catalog_detail(
                &app.custom_themes,
                locale,
                app.settings.custom_theme.as_deref(),
            );
            if !detail.is_empty() {
                detail
            } else if app.custom_themes.follows_omarchy() {
                gettext(locale, "Follow system uses your Omarchy colours.")
            } else {
                gettext(
                    locale,
                    "Follow system uses your desktop's light or dark appearance.",
                )
            }
        }),
        RowText::new(
            gettext(locale, "Language"),
            gettext(
                locale,
                "System follows your computer's language. Untranslated text stays in English.",
            ),
        ),
        RowText::new(
            middle_click.clone(),
            gettext(
                locale,
                "Middle-click a list, then move the pointer to scroll it. Off by default, because a middle click usually pastes on Linux.",
            ),
        )
        .when(cfg!(target_os = "linux")),
    ];
    if section_matches(&needle, &appearance, &appearance_rows) {
        any_visible = true;
        section(ui, &palette, &appearance, |ui| {
            filtered_row(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[1],
                |ui| {
                    ui.horizontal(|ui| language_picker(app, ui));
                },
            );
            // Wide enough for the theme's two buttons side by side.
            let theme_buttons_width = theme::soft_button_width(ui, &theme_guide)
                + theme::soft_button_width(ui, &themes_folder)
                + 6.0;
            theme_grid(app, ui, &palette);
            ui.add_space(6.0);
            filtered_row_sized(
                ui,
                &palette,
                &needle,
                &appearance,
                &appearance_rows[0],
                theme_buttons_width,
                |ui| {
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                        if app.settings.theme == ThemeChoice::Custom && app.settings.custom_theme.is_none() {
                            let mut picked = false;
                            for (label, colour) in [
                                ("Window colour", &mut app.settings.custom_bg),
                                ("Accent colour", &mut app.settings.custom_accent),
                            ] {
                                ui.horizontal(|ui| {
                                    let id = egui::Id::new(("custom-hex", label));
                                    let mut hex = ui
                                        .data(|data| data.get_temp::<(String, [u8; 3])>(id))
                                        .filter(|(_, stored)| *stored == *colour)
                                        .map(|(text, _)| text)
                                        .unwrap_or_else(|| {
                                            format!("#{:02X}{:02X}{:02X}", colour[0], colour[1], colour[2])
                                        });
                                    if ui
                                        .add(egui::TextEdit::singleline(&mut hex).desired_width(84.0))
                                        .changed()
                                    {
                                        if let Some(parsed) = crate::settings::parse_hex(&hex) {
                                            *colour = parsed;
                                            picked = true;
                                        }
                                        let keep = (hex.clone(), *colour);
                                        ui.data_mut(|data| data.insert_temp(id, keep));
                                    }
                                    if ui.color_edit_button_srgb(colour).changed() {
                                        picked = true;
                                    }
                                    theme::text(ui, label, theme::regular(theme::BODY_TEXT), palette.secondary);
                                });
                            }
                            if picked {
                                app.actions.push(Action::SettingsChanged);
                            }
                        }
                        let mut solid = app.settings.window_solidity() * 100.0;
                        ui.horizontal(|ui| {
                            theme::text(ui, "See-through window (works with every theme: 100% solid, 50% like frosted glass)", theme::regular(theme::BODY_TEXT), palette.secondary);
                            if setting_slider(
                                ui,
                                &palette,
                                &mut solid,
                                30.0..=100.0,
                                |slider| slider,
                                |value| value.suffix("%").fixed_decimals(0),
                            )
                            .changed()
                            {
                                app.settings.window_opacity = (solid / 100.0).clamp(0.3, 1.0);
                                app.actions.push(Action::SettingsChanged);
                            }
                        });
                        // The guide to writing a theme sits beside the folder
                        // it goes in, and above it when both do not fit.
                        let (guide, folder) = (&theme_guide, &themes_folder);
                        let gap = 6.0;
                        let mut buttons = |ui: &mut egui::Ui| {
                            if theme::soft_button(ui, &palette, Some(Icon::Globe), guide, false)
                                .clicked()
                            {
                                app.actions.push(Action::OpenUrl(THEMES_GUIDE_URL.into()));
                            }
                            if theme::soft_button(
                                ui,
                                &palette,
                                Some(Icon::ExternalLink),
                                folder,
                                false,
                            )
                            .clicked()
                            {
                                app.actions.push(Action::OpenThemesFolder);
                            }
                        };
                        let both = theme::soft_button_width(ui, guide)
                            + theme::soft_button_width(ui, folder)
                            + gap;
                        if ui.available_width() >= both {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                buttons(ui);
                            });
                        } else {
                            ui.spacing_mut().item_spacing.y = gap;
                            buttons(ui);
                        }
                    });
                },
            );
            if cfg!(target_os = "linux") {
                filtered_row(
                    ui,
                    &palette,
                    &needle,
                    &appearance,
                    &appearance_rows[2],
                    |ui| {
                        if widgets::switch(
                            ui,
                            &palette,
                            &middle_click,
                            &mut app.settings.middle_click_autoscroll,
                        )
                        .changed()
                        {
                            changed = true;
                        }
                    },
                );
            }
        });
    }

    let proxy = gettext(locale, "Proxy");
    let proxy_rows = [RowText::new(
        gettext(locale, "Mode, host, port, username and password"),
        gettext(
            locale,
            "Off, System, HTTP or SOCKS5 proxy for network requests.",
        ),
    )];
    if section_matches(&needle, &proxy, &proxy_rows) {
        any_visible = true;
        ui.push_id("proxy-settings", |ui| {
    section(ui, &palette, &proxy, |ui| {
        // The row's control slot is right-to-left and too narrow for four
        // choices; they sit on this line so they read Off, System, HTTP, SOCKS5.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for choice in ProxyMode::ALL {
                if theme::soft_button(
                    ui,
                    &palette,
                    None,
                    &choice.label(locale),
                    app.settings.proxy_mode == choice,
                )
                .clicked()
                    && app.settings.proxy_mode != choice
                {
                    app.settings.proxy_mode = choice;
                    changed = true;
                    app.actions.push(Action::ProxyEdited);
                    if choice.is_manual() {
                        proxy_dirty = true;
                    } else {
                        proxy_dirty = false;
                        app.actions.push(Action::ApplyProxy);
                    }
                }
            }
        });
        ui.add_space(10.0);
        if app.settings.proxy_mode.is_manual() {
            if widgets::proxy_manual_form(
                ui,
                &palette,
                app.locale,
                &mut app.settings.proxy_host,
                &mut app.settings.proxy_port,
                &mut app.settings.proxy_username,
                &mut app.settings.proxy_password,
            ) {
                changed = true;
                app.actions.push(Action::ProxyEdited);
                proxy_dirty = true;
            }
            ui.add_space(10.0);
        }
        if app.settings.proxy_mode.is_manual() {
            ui.horizontal(|ui| {
                if theme::pill_button(ui, &palette, &gettext(locale, "Apply settings"), true).clicked() {
                    app.actions.push(Action::ApplyProxy);
                    if app.settings.proxy_config().is_ok() {
                        proxy_dirty = false;
                    }
                }
            });
        }
    });
        });
    }

    let equalizer = gettext(locale, "Equalizer");
    let equalizer_rows = [
        RowText::new(
            equalizer.clone(),
            gettext(
                locale,
                "A ten-band equalizer for playback on this computer. It does not affect other devices.",
            ),
        ),
        RowText::new(
            format!(
                "{} {} {} dB",
                gettext(locale, "Presets"),
                // Translators: a search keyword for the equalizer's preamplifier slider.
                gettext(locale, "Preamp"),
                // Translators: a search keyword for the equalizer's frequency band sliders.
                gettext(locale, "Bands"),
            ),
            crate::eq::PRESETS
                .iter()
                .map(|preset| preset.name)
                .collect::<Vec<_>>()
                .join(" "),
        ),
    ];
    if false && section_matches(&needle, &equalizer, &equalizer_rows) {
        any_visible = true;
        section(ui, &palette, &equalizer, |ui| {
            filtered_row(
                ui,
                &palette,
                &needle,
                &equalizer,
                &equalizer_rows[0],
                |ui| {
                    let mut on = app.settings.eq_on;
                    if widgets::switch(ui, &palette, &equalizer, &mut on).changed() {
                        app.actions.push(Action::ToggleEq);
                    }
                },
            );
            let names: Vec<(usize, &str)> = crate::eq::PRESETS
                .iter()
                .enumerate()
                .map(|(index, preset)| (index, preset.name))
                .collect();
            let current = crate::eq::PRESETS
                .iter()
                .position(|preset| preset.bands_db == app.settings.eq_bands_db)
                .unwrap_or(usize::MAX);
            let eq_extra_visible = equalizer_rows[1].matches(&needle, &equalizer);
            if eq_extra_visible {
                if let Some(picked) = widgets::chips(ui, &palette, &names, current) {
                    app.actions.push(Action::ApplyEqPreset(picked));
                }
                ui.add_space(10.0);
                let width = ui.available_width().min(470.0);
                eq_graph_and_sliders(app, ui, width);
            }
        });
    }

    let storage = gettext(locale, "Storage");
    let storage_rows = [
        RowText::new(
            gettext(locale, "Artwork cache"),
            // Translators: {folder} is the path of a cache folder.
            gettext(locale, "Stored in {folder}")
                .replace("{folder}", &app.dirs.art_cache_dir().display().to_string()),
        ),
        RowText::new(
            gettext(locale, "Play history"),
            gettext(
                locale,
                // Translators: {file} is the path of the play history file.
                "Tracks played here are stored in {file}. This file is never uploaded.",
            )
            .replace("{file}", &app.dirs.history_file().display().to_string()),
        ),
    ];
    if section_matches(&needle, &storage, &storage_rows) {
        any_visible = true;
        section(ui, &palette, &storage, |ui| {
            filtered_row(ui, &palette, &needle, &storage, &storage_rows[0], |ui| {
                if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Trash),
                    &gettext(locale, "Clear artwork"),
                    false,
                )
                .clicked()
                {
                    app.actions.push(Action::ClearArtCache);
                }
            });
            filtered_row(ui, &palette, &needle, &storage, &storage_rows[1], |ui| {
                if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Trash),
                    &gettext(locale, "Clear history"),
                    false,
                )
                .clicked()
                {
                    app.actions.push(Action::ClearPlayHistory);
                }
            });
            note(ui, &palette, "Everything chanceify saves lives in the chanceify folder beside the program. Nothing is uploaded.");
        });
    }

    if needle.is_empty() || "unavailable songs removed deleted missing lost cleanup duplicate duplicates overlap playlists".contains(needle.as_str()) {
        any_visible = true;
        section(ui, &palette, "Unavailable songs", |ui| {
            let user = app.user.as_ref().map(|user| user.id.clone()).unwrap_or_default();
            let entries: Vec<crate::unavailable::Entry> =
                app.unavailable.entries_for(&user).to_vec();
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "Songs in your playlists that Spotify has removed or that are not available where you are.\nFind them elsewhere before you remove them, so they are not lost.",
                    )
                    .font(theme::regular(theme::BODY_TEXT))
                    .color(palette.secondary),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            {
                let mut on = app.settings.unavailable_dot;
                if widgets::switch_labeled(ui, &palette, "Show a red dot on my picture when songs are unavailable", &mut on).changed() {
                    app.settings.unavailable_dot = on;
                    app.mark_settings_dirty();
                }
                ui.add_space(8.0);
            }
            let scanning = app
                .unavailable_scan
                .as_ref()
                .map(|scan| (scan.total.saturating_sub(scan.queue.len()), scan.total));
            ui.horizontal(|ui| {
                match scanning {
                    Some((done, total)) => {
                        theme::text(
                            ui,
                            format!("Scanning your playlists… {done} of {total}"),
                            theme::regular(theme::BODY_TEXT),
                            palette.secondary,
                        );
                    }
                    None => {
                        if theme::soft_button(ui, &palette, Some(Icon::Refresh), "Scan my playlists", false)
                            .clicked()
                        {
                            app.actions.push(Action::ScanUnavailable);
                        }
                        if let Some(when) = app
                            .unavailable
                            .scanned_at
                            .as_deref()
                            .filter(|_| app.unavailable.user == user)
                        {
                            theme::text(
                                ui,
                                format!("Last scan: {}", when.get(..10).unwrap_or(when)),
                                theme::regular(theme::SMALL_TEXT),
                                palette.dim,
                            );
                        }
                    }
                }
            });
            // Songs held twice by one playlist, and playlists that mostly hold
            // the same songs. Each has a button to open the playlist, where
            // the songs can be dealt with by hand.
            if scanning.is_none() && app.unavailable.user == user {
                let doubled = app.unavailable.duplicates.clone();
                let same = app.unavailable.overlaps.clone();
                if !doubled.is_empty() {
                    ui.add_space(10.0);
                    theme::text(
                        ui,
                        format!("{} songs are in a playlist twice or more", doubled.len()),
                        theme::semibold(theme::BODY_TEXT),
                        palette.text,
                    );
                    egui::ScrollArea::vertical()
                        .id_salt("cleanup-duplicates")
                        .max_height(180.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for dup in &doubled {
                                ui.horizontal(|ui| {
                                    theme::text(
                                        ui,
                                        format!("{} - {} (x{}) in {}", dup.artists, dup.title, dup.copies, dup.playlist_name),
                                        theme::regular(theme::SMALL_TEXT),
                                        palette.secondary,
                                    );
                                    if theme::soft_button(ui, &palette, None, "Open", false).clicked() {
                                        app.actions.push(Action::Open(Page::Playlist(dup.playlist_id.clone())));
                                    }
                                });
                            }
                        });
                }
                if !same.is_empty() {
                    ui.add_space(10.0);
                    theme::text(
                        ui,
                        format!("{} pairs of playlists hold mostly the same songs", same.len()),
                        theme::semibold(theme::BODY_TEXT),
                        palette.text,
                    );
                    egui::ScrollArea::vertical()
                        .id_salt("cleanup-overlaps")
                        .max_height(180.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            for pair in &same {
                                ui.horizontal(|ui| {
                                    theme::text(
                                        ui,
                                        format!("{} and {}: {} songs shared ({}%)", pair.a_name, pair.b_name, pair.shared, pair.percent),
                                        theme::regular(theme::SMALL_TEXT),
                                        palette.secondary,
                                    );
                                    if theme::soft_button(ui, &palette, None, "Open first", false).clicked() {
                                        app.actions.push(Action::Open(Page::Playlist(pair.a_id.clone())));
                                    }
                                    if theme::soft_button(ui, &palette, None, "Open second", false).clicked() {
                                        app.actions.push(Action::Open(Page::Playlist(pair.b_id.clone())));
                                    }
                                });
                            }
                        });
                }
            }
            if entries.is_empty() {
                if app.unavailable.scanned_at.is_some() && scanning.is_none() {
                    ui.add_space(6.0);
                    theme::text(
                        ui,
                        "None found. Every song in your playlists can be played.",
                        theme::regular(theme::BODY_TEXT),
                        palette.secondary,
                    );
                }
                return;
            }
            ui.add_space(10.0);
            theme::text(
                ui,
                format!("{} unavailable songs", entries.len()),
                theme::semibold(theme::BODY_TEXT),
                palette.text,
            );
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if theme::soft_button(ui, &palette, Some(Icon::Copy), "Copy the list as text", false).clicked() {
                    app.actions.push(Action::CopyUnavailableList);
                }
                if theme::soft_button(ui, &palette, Some(Icon::ExternalLink), "Save the list as a text file", false).clicked() {
                    app.actions.push(Action::SaveUnavailableList);
                }
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                theme::text(
                    ui,
                    "Confirm removal",
                    theme::regular(theme::BODY_TEXT),
                    palette.secondary,
                );
                let mut ask = app.settings.unavailable_confirm_one;
                if widgets::switch(ui, &palette, "Confirm removal", &mut ask).changed() {
                    app.settings.unavailable_confirm_one = ask;
                    app.actions.push(Action::SettingsChanged);
                }
            });
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("unavailable-songs")
                .max_height((ui.ctx().content_rect().height() * 0.62).max(320.0))
                .min_scrolled_height((ui.ctx().content_rect().height() * 0.45).max(240.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for entry in &entries {
                        ui.horizontal_wrapped(|ui| {
                            let line = if entry.artists.is_empty() {
                                entry.title.clone()
                            } else {
                                format!("{} - {}", entry.artists, entry.title)
                            };
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(line)
                                        .font(theme::medium(theme::BODY_TEXT))
                                        .color(palette.text),
                                )
                                .wrap(),
                            );
                        });
                        ui.horizontal(|ui| {
                            theme::text(
                                ui,
                                format!("in {} (#{})", entry.playlist_name, entry.position),
                                theme::regular(theme::SMALL_TEXT),
                                palette.secondary,
                            );
                            if entry.has_title() {
                                if theme::soft_button(ui, &palette, None, "YouTube", false).clicked() {
                                    app.actions.push(Action::OpenUrl(entry.youtube_url()));
                                }
                                if theme::soft_button(ui, &palette, None, "SoundCloud", false).clicked() {
                                    app.actions.push(Action::OpenUrl(entry.soundcloud_url()));
                                }
                            }
                            if !entry.uri.is_empty() {
                                let ask_id = egui::Id::new(("unavailable-ask", &entry.playlist_id, &entry.uri));
                                let asking = ui.data(|data| data.get_temp::<bool>(ask_id)).unwrap_or(false);
                                let remove = Action::RemoveUnavailableOne {
                                    playlist_id: entry.playlist_id.clone(),
                                    uri: entry.uri.clone(),
                                };
                                if asking {
                                    theme::text(ui, "Remove it?", theme::regular(theme::SMALL_TEXT), palette.warning);
                                    if theme::soft_button(ui, &palette, Some(Icon::Trash), "Yes", true).clicked() {
                                        app.actions.push(remove);
                                        ui.data_mut(|data| data.insert_temp(ask_id, false));
                                    } else if theme::soft_button(ui, &palette, None, "No", false).clicked() {
                                        ui.data_mut(|data| data.insert_temp(ask_id, false));
                                    }
                                } else if theme::soft_button(ui, &palette, Some(Icon::Trash), "Remove this one", false).clicked() {
                                    if app.settings.unavailable_confirm_one {
                                        ui.data_mut(|data| data.insert_temp(ask_id, true));
                                    } else {
                                        app.actions.push(remove);
                                    }
                                }
                            }
                        });
                        ui.add_space(6.0);
                    }
                });
            let removable = entries.iter().filter(|entry| !entry.uri.is_empty()).count();
            if removable > 0 {
                ui.add_space(8.0);
                let confirm_id = egui::Id::new("unavailable-confirm");
                // 0 nothing asked, 1 first question, 2 second and last question.
                let stage = ui.data(|data| data.get_temp::<u8>(confirm_id)).unwrap_or(0);
                if stage == 2 {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!(
                                "Last check: remove all {removable} songs now? This cannot be undone."
                            ))
                            .font(theme::semibold(theme::BODY_TEXT))
                            .color(palette.warning),
                        )
                        .wrap(),
                    );
                    ui.horizontal(|ui| {
                        if theme::soft_button(ui, &palette, Some(Icon::Trash), "Yes, remove all of them", true)
                            .clicked()
                        {
                            app.actions.push(Action::RemoveUnavailable);
                            ui.data_mut(|data| data.insert_temp(confirm_id, 0u8));
                        }
                        if theme::soft_button(ui, &palette, None, "Cancel", false).clicked() {
                            ui.data_mut(|data| data.insert_temp(confirm_id, 0u8));
                        }
                    });
                } else if stage == 1 {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!(
                                "This takes {removable} songs out of every playlist they are in. Make sure you have found them elsewhere first."
                            ))
                            .font(theme::regular(theme::BODY_TEXT))
                            .color(palette.warning),
                        )
                        .wrap(),
                    );
                    ui.horizontal(|ui| {
                        if theme::soft_button(ui, &palette, Some(Icon::Trash), "Yes, continue", true)
                            .clicked()
                        {
                            ui.data_mut(|data| data.insert_temp(confirm_id, 2u8));
                        }
                        if theme::soft_button(ui, &palette, None, "Cancel", false).clicked() {
                            ui.data_mut(|data| data.insert_temp(confirm_id, 0u8));
                        }
                    });
                } else if theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Trash),
                    "Remove them from my playlists",
                    false,
                )
                .clicked()
                {
                    ui.data_mut(|data| data.insert_temp(confirm_id, 1u8));
                }
            }
        });
    }

    let about = gettext(locale, "About");
    let source_code = gettext(locale, "Source code");
    let about_rows = [RowText::new(
        format!("{} {}", crate::build_info::DISPLAY_NAME, crate::build_info::version()),
        source_code.clone(),
    )];
    if section_matches(&needle, &about, &about_rows) {
        any_visible = true;
        section(ui, &palette, &about, |ui| {
            ui.horizontal(|ui| {
                let (logo, _) = ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::hover());
                theme::logo(ui, logo.center(), 40.0);
                ui.vertical(|ui| {
                    theme::text(
                        ui,
                        format!(
                            "{} {}",
                            crate::build_info::DISPLAY_NAME,
                            crate::build_info::version()
                        ),
                        theme::semibold(theme::BODY_TEXT),
                        palette.text,
                    );
                    if let Some(mb) = crate::util::memory_used_mb() {
                        theme::text(
                            ui,
                            format!("memory used: {mb} MB"),
                            theme::regular(theme::SMALL_TEXT),
                            palette.dim,
                        );
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                    }
                });
            });
            ui.add_space(8.0);
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(format!(
                    "Index folder: {}",
                    app.dirs.index_dir().display()
                ))
                .font(theme::regular(theme::SMALL_TEXT))
                .color(palette.secondary),
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if theme::soft_button(ui, &palette, None, "Export my settings", false).clicked() {
                    app.actions.push(Action::ExportSettings);
                }
                if theme::soft_button(ui, &palette, None, "Import settings", false).clicked() {
                    app.actions.push(Action::ImportSettings);
                }
                if theme::soft_button(ui, &palette, None, "Load chance's defaults", false).clicked() {
                    app.actions.push(Action::LoadDefaultSettings);
                }
                if theme::soft_button(ui, &palette, None, "Reset to plain defaults", false).clicked() {
                    app.actions.push(Action::ResetSettingsToVanilla);
                }
                if theme::soft_button(ui, &palette, Some(Icon::ExternalLink), "Open index folder", false)
                    .clicked()
                {
                    app.actions.push(Action::OpenIndexFolder);
                }
            });
            ui.add_space(14.0);
            if widgets::credit(ui, &palette, locale) {
                app.actions
                    .push(Action::OpenUrl(widgets::AUTHOR_URL.to_owned()));
            }
        });
    }

    // The app icon is always the transparent one now; there is nothing to pick.
    if false {
        any_visible = true;
        section(ui, &palette, "App icon", |ui| {
            widgets::setting_row(
                ui,
                &palette,
                "App icon",
                "The picture on the window and the taskbar button. It changes as soon as you pick one.",
                |_| {},
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                for choice in 0..crate::app_icons::count() as u8 {
                    let id = egui::Id::new(("app-icon-thumb", choice));
                    let texture = ui
                        .ctx()
                        .data(|data| data.get_temp::<egui::TextureHandle>(id))
                        .unwrap_or_else(|| {
                            let texture = ui.ctx().load_texture(
                                format!("app-icon-thumb-{choice}"),
                                crate::app_icons::thumbnail(choice),
                                egui::TextureOptions::LINEAR,
                            );
                            ui.ctx()
                                .data_mut(|data| data.insert_temp(id, texture.clone()));
                            texture
                        });
                    let (rect, response) =
                        ui.allocate_exact_size(Vec2::splat(60.0), egui::Sense::click());
                    let picked = app.settings.app_icon == choice;
                    if picked || response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(10),
                            if picked {
                                palette.accent.gamma_multiply(0.35)
                            } else {
                                palette.surface_hover
                            },
                        );
                    }
                    ui.painter().image(
                        texture.id(),
                        rect.shrink(6.0),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                    let response = response.on_hover_text(crate::app_icons::name(choice));
                    if response.clicked() {
                        app.actions.push(Action::SetAppIcon(choice));
                    }
                }
            });
            ui.add_space(8.0);
        });
    }

    if cfg!(feature = "lights-feed") && (needle.is_empty() || "lights feed keyboard microphone led colours rgb".contains(needle.as_str())) {
        any_visible = true;
        section(ui, &palette, "Lights feed", |ui| {
            let mut on = app.settings.lights_feed;
            if widgets::switch_labeled(ui, &palette, "Share the visualizer's colours with light apps", &mut on).changed() {
                app.settings.lights_feed = on;
                app.mark_settings_dirty();
            }
            let path = crate::lights::Feed::path_for(&app.dirs.state);
            note(ui, &palette, "Writes the colours on screen (cover colours, Auto drift, your swatch pick, gradient) to one small file, many times a second, so a keyboard or microphone light app can match them. Nothing goes over the network. Off by default; the file is removed when this is off.");
            ui.horizontal_wrapped(|ui| {
                theme::subtle(ui, &palette, "File:");
                ui.label(egui::RichText::new(path.display().to_string()).font(theme::regular(theme::SMALL_TEXT)).color(palette.secondary));
            });
            ui.add_space(8.0);
        });
    }

    if needle.is_empty() || "discord rich presence status buttons".contains(needle.as_str()) {
        any_visible = true;
        section(ui, &palette, "Discord", |ui| {
            {
                let mut on = app.settings.discord_presence;
                if widgets::switch_labeled(ui, &palette, "Show what I'm listening to on Discord", &mut on).changed() {
                    app.settings.discord_presence = on;
                    app.mark_settings_dirty();
                }
                if app.settings.discord_presence {
                    ui.add_space(4.0);
                    ui.add_space(6.0);
                    {
                        let (text, colour) = match crate::discord::link_state() {
                            2 => ("Discord connected, song sent.", palette.accent),
                            1 => ("Discord: found. Waiting for a song to send.", palette.secondary),
                            _ => (
                                "Discord: not found yet. Open the Discord desktop app (not the website), and turn on Settings > Activity Privacy > Share my activity in Discord.",
                                palette.secondary,
                            ),
                        };
                        theme::text(ui, text, theme::regular(theme::SMALL_TEXT), colour);
                        let problem = crate::discord::last_error();
                        if !problem.is_empty() {
                            theme::text(ui, &problem, theme::regular(theme::SMALL_TEXT), palette.warning);
                        }
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs(2));
                    }
                }
            }
        });
    }

    if needle.is_empty() || "last.fm lastfm scrobble scrobbling love".contains(needle.as_str()) {
        any_visible = true;
        let view = app.lastfm.view();
        if matches!(view.status, crate::lastfm::Status::WaitingForBrowser) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(500));
        }
        section(ui, &palette, "Last.fm", |ui| {
            let signed_in = app.lastfm_ready();
            let waiting = matches!(view.status, crate::lastfm::Status::WaitingForBrowser);
            let mut status = if signed_in {
                format!("Signed in as {}.", app.settings.lastfm_user)
            } else {
                match &view.status {
                    crate::lastfm::Status::WaitingForBrowser => {
                        "Waiting for you to press Allow on the Last.fm page that just opened.".to_string()
                    }
                    crate::lastfm::Status::Failed(message) => message.clone(),
                    _ => "Not signed in.".to_string(),
                }
            };
            if signed_in {
                if let Some(total) = view.scrobbles {
                    status.push_str(&format!(" {total} songs counted."));
                }
                if view.queued > 0 {
                    status.push_str(&format!(" {} waiting to be sent.", view.queued));
                }
            }
            widgets::setting_row_note(
                ui,
                &palette,
                "Last.fm account",
                &status,
                |ui| {
                    if waiting {
                        if theme::soft_button(ui, &palette, None, "Cancel", false).clicked() {
                            app.actions.push(Action::LastfmCancel);
                        }
                    } else if signed_in {
                        if theme::soft_button(ui, &palette, None, "Sign out", false).clicked() {
                            app.actions.push(Action::LastfmSignOut);
                        }
                    } else if theme::soft_button(ui, &palette, Some(Icon::Music), "Sign in", false)
                        .clicked()
                    {
                        app.actions.push(Action::LastfmSignIn);
                    }
                },
            );
            if !signed_in && !waiting {
                note(ui, &palette, "To connect: press Sign in. Your browser opens Last.fm, where you log in and press Allow. Then come back here. Your plays go to your own Last.fm profile.");
                ui.add_space(4.0);
            }
            if crate::lastfm::DEFAULT_API_KEY.is_empty() {
                theme::text(ui, "Set up Last.fm in 4 steps", theme::semibold(theme::BODY_TEXT), palette.text);
                for line in [
                    "1. Open the Last.fm page below and sign in.",
                    "2. Type any name (chanceify) and a short description. Leave the callback box empty.",
                    "3. Press Submit. The page shows an API key and a Shared secret.",
                    "4. Paste both here, then press Sign in and Allow.",
                ] {
                    theme::subtle(ui, &palette, line);
                }
                ui.add_space(6.0);
                widgets::setting_row(
                    ui,
                    &palette,
                    "API key",
                    "Make a free one at last.fm/api/account/create (any name will do), then paste its key and shared secret here.",
                    |ui| {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut app.settings.lastfm_api_key)
                                .desired_width(220.0)
                                .hint_text("API key"),
                        );
                        if edit.changed() {
                            app.actions.push(Action::SettingsChanged);
                        }
                    },
                );
                widgets::setting_row(
                    ui,
                    &palette,
                    "Shared secret",
                    "From the same page. It stays on this computer.",
                    |ui| {
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut app.settings.lastfm_secret)
                                .desired_width(220.0)
                                .password(true)
                                .hint_text("Shared secret"),
                        );
                        if edit.changed() {
                            app.actions.push(Action::SettingsChanged);
                        }
                    },
                );
                ui.hyperlink_to(
                    "Open the Last.fm page for making a key",
                    "https://www.last.fm/api/account/create",
                );
                ui.add_space(6.0);
            }
            if let Some(url) = &view.auth_url {
                ui.add_space(4.0);
                theme::subtle(
                    ui,
                    &palette,
                    "If the page did not open, copy this address into your browser:",
                );
                let mut shown = url.clone();
                ui.add(
                    egui::TextEdit::singleline(&mut shown)
                        .desired_width(f32::INFINITY)
                        .interactive(true),
                );
                ui.add_space(6.0);
            }
            if signed_in {
                let mut scrobble = app.settings.lastfm_scrobble;
                widgets::setting_row(
                    ui,
                    &palette,
                    "Scrobble what I play",
                    "Count each song on Last.fm once you have heard half of it.",
                    |ui| {
                        if widgets::switch(ui, &palette, "Scrobble what I play", &mut scrobble)
                            .changed()
                        {
                            app.settings.lastfm_scrobble = scrobble;
                            app.actions.push(Action::SettingsChanged);
                        }
                    },
                );
                let mut now_playing = app.settings.lastfm_now_playing;
                widgets::setting_row(
                    ui,
                    &palette,
                    "Show what is playing now",
                    "Last.fm shows the song on your profile while it plays.",
                    |ui| {
                        if widgets::switch(ui, &palette, "Show what is playing now", &mut now_playing)
                            .changed()
                        {
                            app.settings.lastfm_now_playing = now_playing;
                            app.actions.push(Action::SettingsChanged);
                        }
                    },
                );
                let mut files = app.settings.lastfm_files;
                widgets::setting_row(
                    ui,
                    &palette,
                    "Include songs from my own files",
                    "Songs played on the Local songs page count too, if their tags name an artist.",
                    |ui| {
                        if widgets::switch(ui, &palette, "Include songs from my own files", &mut files)
                            .changed()
                        {
                            app.settings.lastfm_files = files;
                            app.actions.push(Action::SettingsChanged);
                        }
                    },
                );
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
                    if theme::soft_button(ui, &palette, Some(Icon::ExternalLink), "Last.fm missing images", false)
                        .clicked()
                    {
                        app.actions.push(Action::OpenMissingArtFolder);
                    }
                    {
                        let bytes = app.art_folder_size() as f64;
                        let text = if bytes >= 1e9 {
                            format!("{:.2} GB", bytes / 1e9)
                        } else if bytes >= 1e6 {
                            format!("{:.1} MB", bytes / 1e6)
                        } else {
                            format!("{:.0} KB", bytes / 1e3)
                        };
                        theme::subtle(ui, &palette, &format!("Album covers and artist pictures folder: {text}"));
                    }
                    let auto_label = if app.settings.lastfm_art_auto {
                        "Save missing pictures automatically: on"
                    } else {
                        "Save missing pictures automatically: off"
                    };
                    if theme::soft_button(ui, &palette, None, auto_label, app.settings.lastfm_art_auto)
                        .on_hover_text("As songs play, the covers and artist pictures Last.fm lacks are saved to the lists below.")
                        .clicked()
                    {
                        app.settings.lastfm_art_auto = !app.settings.lastfm_art_auto;
                        app.mark_settings_dirty();
                    }
                    let all_label = if app.settings.lastfm_art_all {
                        "Save: every cover and picture"
                    } else {
                        "Save: only the ones Last.fm is missing"
                    };
                    if theme::soft_button(ui, &palette, None, all_label, app.settings.lastfm_art_all)
                        .on_hover_text("Only missing: saves just what Last.fm lacks (to upload). Every: also keeps a copy of all the others.")
                        .clicked()
                    {
                        app.settings.lastfm_art_all = !app.settings.lastfm_art_all;
                        app.mark_settings_dirty();
                    }
                    let sweep_label = if app.art_sweep { "Stop checking my library" } else { "Check my whole library" };
                    if theme::soft_button(ui, &palette, None, sweep_label, app.art_sweep)
                        .on_hover_text("Asks Last.fm about the picture of every album and artist you have saved, a few at a time. The ones it lacks go in the missing lists.")
                        .clicked()
                    {
                        app.actions.push(Action::ToggleArtSweep);
                    }
                    let mark_label = if app.settings.missing_mark_big {
                        "Missing-cover mark: on"
                    } else {
                        "Missing-cover mark: off"
                    };
                    if theme::soft_button(ui, &palette, None, mark_label, app.settings.missing_mark_big)
                        .on_hover_text("Shows a yellow ! on the album art (small and big) when Last.fm has no cover for it.")
                        .clicked()
                    {
                        app.actions.push(Action::ToggleMissingMarkBig);
                    }
                });
                missing_lists(ui, app, &palette);
            }
        });
    }

    // The guided tour is gone: people find things by right-clicking.
    if false {
        any_visible = true;
        section(ui, &palette, "Tour", |ui| {
            ui.add(
                egui::Label::new(
                    egui::RichText::new(
                        "A short guided tour that lights up parts of the window. Change the steps below; they are saved in your index folder (tour.json).",
                    )
                    .font(theme::regular(theme::BODY_TEXT))
                    .color(palette.secondary),
                )
                .wrap(),
            );
            ui.add_space(8.0);
            let mut changed = false;
            let mut remove: Option<usize> = None;
            let mut swap: Option<(usize, usize)> = None;
            let count = app.tour.steps.len();
            for index in 0..count {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    theme::text(ui, format!("Step {}", index + 1), theme::semibold(theme::BODY_TEXT), palette.text);
                    if index > 0 && theme::pill_button(ui, &palette, "Up", false).clicked() {
                        swap = Some((index, index - 1));
                    }
                    if index + 1 < count && theme::pill_button(ui, &palette, "Down", false).clicked() {
                        swap = Some((index, index + 1));
                    }
                    if count > 1 && theme::pill_button(ui, &palette, "Remove", false).clicked() {
                        remove = Some(index);
                    }
                });
                let step = &mut app.tour.steps[index];
                changed |= ui
                    .add(egui::TextEdit::singleline(&mut step.title).desired_width(f32::INFINITY))
                    .changed();
                changed |= ui
                    .add(
                        egui::TextEdit::multiline(&mut step.text)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    )
                    .changed();
                let current = crate::tour::ANCHORS
                    .iter()
                    .find(|(id, _)| *id == step.anchor)
                    .map_or("Nothing (centre card)", |(_, label)| *label);
                egui::ComboBox::from_id_salt(("tour-anchor", index))
                    .selected_text(format!("Points at: {current}"))
                    .show_ui(ui, |ui| {
                        for (id, label) in crate::tour::ANCHORS {
                            if ui.selectable_label(step.anchor == *id, *label).clicked() {
                                step.anchor = (*id).to_string();
                                changed = true;
                            }
                        }
                    });
            }
            if let Some(index) = remove {
                app.tour.steps.remove(index);
                changed = true;
            }
            if let Some((a, b)) = swap {
                app.tour.steps.swap(a, b);
                changed = true;
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if theme::pill_button(ui, &palette, "Add step", false).clicked() {
                    app.tour.steps.push(crate::tour::Step {
                        title: "New step".into(),
                        text: "Say something helpful here.".into(),
                        anchor: "none".into(),
                        menu: Vec::new(),
                    });
                    changed = true;
                }
                if theme::pill_button(ui, &palette, "Reset to default", false).clicked() {
                    app.tour = crate::tour::Tour::default();
                    changed = true;
                }
                if theme::pill_button(ui, &palette, "Start the tour", true).clicked() {
                    app.tour_step = Some(0);
                }
            });
            if changed {
                app.tour.save(&crate::tour::file());
            }
        });
    }

    if !needle.is_empty() && !any_visible {
        widgets::empty_state(
            ui,
            &palette,
            Icon::Search,
            // Translators: {query} is the text typed into the settings search field.
            &gettext(locale, "No settings for “{query}”").replace("{query}", &needle),
            &gettext(locale, "Try fewer words, or check the spelling."),
        );
    }

    ui.data_mut(|data| data.insert_temp(dirty_id, playback_dirty));
    ui.data_mut(|data| data.insert_temp(proxy_dirty_id, proxy_dirty));
    if changed {
        app.actions.push(Action::SettingsChanged);
    }
}

/// A band's frequency the short way: 60, 170, 1K, 16K.
/// The interface language: System first, then each language by its own name,
/// so a reader can find theirs whatever language the app is showing.
fn flag_picture(locale: crate::i18n::Locale) -> (&'static str, &'static [u8]) {
    use crate::i18n::Locale as L;
    match locale {
        L::English => ("bytes://flag-gb.png", include_bytes!("../../assets/flags/gb.png")),
        L::German => ("bytes://flag-de.png", include_bytes!("../../assets/flags/de.png")),
        L::Spanish => ("bytes://flag-es.png", include_bytes!("../../assets/flags/es.png")),
        L::Dutch => ("bytes://flag-nl.png", include_bytes!("../../assets/flags/nl.png")),
        L::PortugueseBrazil => ("bytes://flag-br.png", include_bytes!("../../assets/flags/br.png")),
        L::PortuguesePortugal => ("bytes://flag-pt.png", include_bytes!("../../assets/flags/pt.png")),
        L::French => ("bytes://flag-fr.png", include_bytes!("../../assets/flags/fr.png")),
        L::Swedish => ("bytes://flag-se.png", include_bytes!("../../assets/flags/se.png")),
        L::Polish => ("bytes://flag-pl.png", include_bytes!("../../assets/flags/pl.png")),
        L::Russian => ("bytes://flag-ru.png", include_bytes!("../../assets/flags/ru.png")),
        L::Italian => ("bytes://flag-it.png", include_bytes!("../../assets/flags/it.png")),
        L::Japanese => ("bytes://flag-jp.png", include_bytes!("../../assets/flags/jp.png")),
        L::ChineseSimplified => ("bytes://flag-cn.png", include_bytes!("../../assets/flags/cn.png")),
        L::ChineseTraditional => ("bytes://flag-tw.png", include_bytes!("../../assets/flags/tw.png")),
        L::Turkish => ("bytes://flag-tr.png", include_bytes!("../../assets/flags/tr.png")),
        L::Ukrainian => ("bytes://flag-ua.png", include_bytes!("../../assets/flags/ua.png")),
    }
}

fn language_picker(app: &mut App, ui: &mut egui::Ui) {
    let locale = app.locale;
    let system = pgettext(locale, "language", "System");
    let current = app.settings.language;
    let selected = match current {
        LanguageChoice::System => system.clone(),
        LanguageChoice::Locale(chosen) => chosen.native_name().into(),
    };
    // A flag beside the language: pictures, as Windows has no flag emoji.
    if let LanguageChoice::Locale(chosen) = current {
        let (uri, bytes) = flag_picture(chosen);
        ui.add(egui::Image::from_bytes(uri, bytes).fit_to_exact_size(egui::vec2(24.0, 18.0)));
    }
    let response = egui::ComboBox::from_id_salt("interface_language")
        .selected_text(selected.as_ref())
        .width(200.0_f32.min(ui.available_width()))
        // As many languages as a menu holds before it scrolls, not five.
        .height(1000.0)
        .show_ui(ui, |ui| {
            let choices = std::iter::once((LanguageChoice::System, system.clone())).chain(
                crate::i18n::LOCALES
                    .iter()
                    .map(|&each| (LanguageChoice::Locale(each), each.native_name().into())),
            );
            for (choice, label) in choices {
                let clicked = ui
                    .horizontal(|ui| {
                        if let LanguageChoice::Locale(each) = choice {
                            let (uri, bytes) = flag_picture(each);
                            ui.add(egui::Image::from_bytes(uri, bytes).fit_to_exact_size(egui::vec2(24.0, 18.0)));
                        } else {
                            ui.add_space(24.0);
                        }
                        ui.selectable_label(current == choice, label.as_ref()).clicked()
                    })
                    .inner;
                if clicked && current != choice {
                    app.actions.push(Action::SetLanguage(choice));
                }
            }
        });
    let name = gettext(locale, "Language");
    response.response.widget_info(|| {
        let mut info =
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), name.as_ref());
        info.current_text_value = Some(selected.to_string());
        info
    });
}

fn hertz(hz: f32) -> String {
    if hz >= 1000.0 {
        format!("{}K", (hz / 1000.0).round() as u32)
    } else {
        format!("{}", hz.round() as u32)
    }
}

/// The equalizer, as a tab of the visualizer's settings (the volume bar no
/// longer opens it): the switch, the presets, the curve, the preamp and the
/// ten bands.
pub(super) fn eq_panel(app: &mut App, ui: &mut egui::Ui) {
    // Right-click anywhere on the equalizer: share codes to copy and paste.
    let response = ui
        .scope_builder(egui::UiBuilder::new().sense(egui::Sense::click()), |ui| eq_panel_body(app, ui))
        .response;
    let palette = app.palette;
    egui::Popup::context_menu(&response)
        .frame(widgets::menu_frame(&palette))
        .show(|ui| {
            widgets::fit_menu(ui, 320.0);
            if widgets::menu_item(ui, &palette, Some(Icon::Copy), "Copy code for this curve") {
                app.actions.push(Action::CopyEqCode(None));
            }
            let own: Vec<String> = app.settings.custom_eqs.iter().map(|(name, _)| name.clone()).collect();
            for (index, name) in own.iter().enumerate() {
                if widgets::menu_item(ui, &palette, Some(Icon::Copy), &format!("Copy code: {name}")) {
                    app.actions.push(Action::CopyEqCode(Some(index)));
                }
            }
            widgets::menu_separator(ui, &palette);
            if widgets::menu_item(ui, &palette, Some(Icon::ListPlus), "Paste a code…") {
                ui.ctx().data_mut(|data| data.insert_temp(egui::Id::new("eq-code-open"), true));
            }
        });
    // The paste box, under the equalizer while it is open.
    let open_id = egui::Id::new("eq-code-open");
    if ui.ctx().data(|data| data.get_temp::<bool>(open_id)).unwrap_or(false) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let id = egui::Id::new("eq-code-text");
            let mut text = ui.data(|data| data.get_temp::<String>(id)).unwrap_or_default();
            let field = egui::TextEdit::singleline(&mut text)
                .hint_text("Paste a CEQ: code")
                .desired_width(260.0)
                .show(ui)
                .response;
            let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if theme::soft_button(ui, &palette, None, "Add", false).clicked() || enter {
                app.actions.push(Action::ImportEqCode(text.clone()));
                text.clear();
                ui.ctx().data_mut(|data| data.insert_temp(open_id, false));
            }
            if theme::soft_button(ui, &palette, None, "Cancel", false).clicked() {
                text.clear();
                ui.ctx().data_mut(|data| data.insert_temp(open_id, false));
            }
            ui.data_mut(|data| data.insert_temp(id, text));
        });
    }
}

fn eq_panel_body(app: &mut App, ui: &mut egui::Ui) {
    ui.set_width(470.0);
    let width = ui.available_width().min(470.0);
    ui.spacing_mut().item_spacing.y = 6.0;
    eq_controls(app, ui);
    if !app.settings.eq_hide_defaults {
        eq_presets(app, ui);
    }
    eq_custom_curves(app, ui);
    eq_save_field(app, ui);
    ui.add_space(2.0);
    eq_graph_and_sliders(app, ui, width);
}

/// The top row: the switch, the curves file, and whether the built-in
/// presets are listed.
fn eq_controls(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    ui.horizontal(|ui| {
        ui.set_min_height(theme::CONTROL_HEIGHT);
        ui.spacing_mut().item_spacing.x = 8.0;
        let label = gettext(locale, "On");
        let mut on = app.settings.eq_on;
        if widgets::switch(ui, &palette, &label, &mut on).changed() {
            app.actions.push(Action::ToggleEq);
        }
        theme::text(ui, label, theme::regular(theme::BODY_TEXT), palette.text);
        ui.add_space(8.0);
        if theme::soft_button(ui, &palette, None, "Export", false)
            .on_hover_text("Save your own curves to eq-presets.txt in the index settings folder, and copy them")
            .clicked()
        {
            app.actions.push(Action::ExportEq);
        }
        if theme::soft_button(ui, &palette, None, "Import", false)
            .on_hover_text("Load the curves in eq-presets.txt from the index settings folder")
            .clicked()
        {
            app.actions.push(Action::ImportEq);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            theme::text(ui, "Hide defaults", theme::regular(theme::BODY_TEXT), palette.text);
            let mut hide = app.settings.eq_hide_defaults;
            if widgets::switch(ui, &palette, "Hide defaults", &mut hide)
                .on_hover_text("Leave the built-in presets out and list only your own curves")
                .changed()
            {
                app.actions.push(Action::ToggleEqHideDefaults);
            }
        });
    });
}

/// The built-in presets under their headings, each heading folding shut.
fn eq_presets(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let current = crate::eq::PRESETS
        .iter()
        .position(|preset| preset.bands_db == app.settings.eq_bands_db);
    for group in crate::eq::PRESET_GROUPS {
        let open = !app
            .settings
            .eq_closed_groups
            .iter()
            .any(|name| name == group.name);
        // A folded heading still says which of its presets is in use.
        let in_use = current
            .map(|index| crate::eq::PRESETS[index].name)
            .filter(|name| group.presets.contains(name));
        if eq_group_heading(ui, &palette, group.name, open, in_use) {
            app.actions.push(Action::ToggleEqGroup(group.name));
        }
        if open {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for name in group.presets {
                    let Some(index) = crate::eq::preset_index(name) else {
                        continue;
                    };
                    if theme::soft_button(ui, &palette, None, name, current == Some(index)).clicked() {
                        app.actions.push(Action::ApplyEqPreset(index));
                    }
                }
            });
        }
    }
}

/// A small heading over a group of presets, with the chevron that says
/// whether it is open. Returns whether it was clicked.
fn eq_group_heading(
    ui: &mut egui::Ui,
    palette: &Palette,
    label: &str,
    open: bool,
    in_use: Option<&str>,
) -> bool {
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let chevron = if open { Icon::ChevronDown } else { Icon::ChevronRight };
        theme::icon(ui, chevron, 14.0, palette.secondary);
        theme::subtle(ui, palette, label);
        if !open && let Some(name) = in_use {
            ui.add_space(4.0);
            theme::text(ui, name, theme::regular(theme::SMALL_TEXT), palette.accent);
        }
    });
    // Over the words, so the whole heading is the button.
    ui.interact(
        row.response.rect,
        egui::Id::new(("eq-group", label)),
        egui::Sense::click(),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
    .clicked()
}

/// The reader's own curves, always listed: click one to use it. The pencil
/// shows the arrows that move a curve and the cross that removes it.
fn eq_custom_curves(app: &mut App, ui: &mut egui::Ui) {
    if app.settings.custom_eqs.is_empty() {
        return;
    }
    let palette = app.palette;
    let edit_id = egui::Id::new("eq-edit-curves");
    let mut editing = ui.data(|data| data.get_temp::<bool>(edit_id)).unwrap_or(false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        theme::subtle(ui, &palette, "Your curves");
        let label = if editing { "Done" } else { "Edit" };
        if theme::soft_button(ui, &palette, Some(Icon::Pencil), label, editing)
            .on_hover_text("Reorder or remove your curves")
            .clicked()
        {
            editing = !editing;
        }
    });
    ui.data_mut(|data| data.insert_temp(edit_id, editing));
    let count = app.settings.custom_eqs.len();
    let mut apply = None;
    let mut remove = None;
    let mut shift = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
        for (index, (name, bands)) in app.settings.custom_eqs.iter().enumerate() {
            let active = *bands == app.settings.eq_bands_db;
            if !editing {
                if theme::soft_button(ui, &palette, None, name, active).clicked() {
                    apply = Some(index);
                }
                continue;
            }
            // One block per curve, sized up front, so a wrap never parts a
            // curve from its arrows.
            let arrow = 14.0 + 12.0;
            let size = egui::vec2(
                theme::soft_button_width(ui, name) + 3.0 * arrow + 3.0 * 2.0,
                theme::CONTROL_HEIGHT + 4.0,
            );
            ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let left = ui
                    .add_enabled_ui(index > 0, |ui| {
                        theme::icon_button(ui, Icon::ChevronLeft, 14.0, palette.secondary, palette.text, "Move left")
                    })
                    .inner;
                if left.clicked() {
                    shift = Some((index, index.saturating_sub(1)));
                }
                if theme::soft_button(ui, &palette, None, name, active).clicked() {
                    apply = Some(index);
                }
                let right = ui
                    .add_enabled_ui(index + 1 < count, |ui| {
                        theme::icon_button(ui, Icon::ChevronRight, 14.0, palette.secondary, palette.text, "Move right")
                    })
                    .inner;
                if right.clicked() {
                    shift = Some((index, index + 1));
                }
                if theme::icon_button(ui, Icon::X, 14.0, palette.secondary, palette.warning, "Remove this curve").clicked() {
                    remove = Some(index);
                }
            });
        }
    });
    if let Some(index) = apply {
        app.actions.push(Action::ApplyCustomEq(index));
    }
    if let Some((from, to)) = shift {
        app.actions.push(Action::MoveCustomEq(from, to));
    }
    if let Some(index) = remove {
        app.actions.push(Action::RemoveCustomEq(index));
    }
}

/// The field that keeps the curve in use under a name.
fn eq_save_field(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let id = egui::Id::new("eq-save-name");
        let mut name = ui.data(|data| data.get_temp::<String>(id)).unwrap_or_default();
        let field = egui::TextEdit::singleline(&mut name)
            .hint_text("Name this curve")
            .desired_width(180.0)
            .show(ui)
            .response;
        let enter = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if (theme::soft_button(ui, &palette, None, "Save curve", false).clicked() || enter)
            && !name.trim().is_empty()
        {
            app.actions.push(Action::SaveCustomEq(name.clone()));
            name.clear();
        }
        ui.data_mut(|data| data.insert_temp(id, name));
    });
}

/// The gap between the equalizer's columns.
const EQ_GAP: f32 = 6.0;

/// Column `index` of the equalizer's row, `width` wide from `row`'s left:
/// 0 is the preamp, 1 to 10 the bands. The curve and the sliders share
/// these, so each band's point on the curve sits over its slider.
fn eq_column(row: egui::Rect, index: usize) -> egui::Rect {
    let pitch = (row.width() + EQ_GAP) / 11.0;
    let left = row.left() + index as f32 * pitch;
    egui::Rect::from_x_y_ranges(left..=left + pitch - EQ_GAP, row.y_range())
}

/// The curve over the preamp and the ten bands, both exactly `width` wide.
fn eq_graph_and_sliders(app: &mut App, ui: &mut egui::Ui, width: f32) {
    let palette = app.palette;
    let locale = app.locale;
    let on = app.settings.eq_on;
    eq_curve(ui, &palette, &crate::app::eq_settings(&app.settings), width);
    let (row, _) = ui.allocate_exact_size(egui::vec2(width, 104.0), egui::Sense::hover());
    // Translators: short label under the equalizer's preamplifier slider.
    let pre = pgettext(locale, "equalizer", "Pre");
    let mut preamp = app.settings.eq_preamp_db;
    if eq_slider(ui, &palette, eq_column(row, 0), egui::Id::new("eq-slider-pre"), &pre, &mut preamp, on) {
        app.actions.push(Action::SetEqPreamp(preamp));
    }
    for (band, hz) in crate::eq::BANDS.iter().enumerate() {
        let mut gain = app.settings.eq_bands_db[band];
        let id = egui::Id::new(("eq-slider", band));
        if eq_slider(ui, &palette, eq_column(row, band + 1), id, &hertz(*hz), &mut gain, on) {
            app.actions.push(Action::SetEqBand(band, gain));
        }
    }
}

/// One vertical slider in the app's own style, in `rect` with its label
/// along the bottom: the track filled from 0 dB, the handle in the middle
/// when flat, a double-click to put it back there. Returns whether it moved.
fn eq_slider(
    ui: &mut egui::Ui,
    palette: &Palette,
    rect: egui::Rect,
    id: egui::Id,
    label: &str,
    value: &mut f32,
    on: bool,
) -> bool {
    use egui::{Rect, Stroke, pos2};
    let range = crate::eq::RANGE_DB;
    let response = ui.interact(rect, id, egui::Sense::click_and_drag());
    let label_height = 16.0;
    let track = Rect::from_min_max(
        pos2(rect.center().x - 2.0, rect.top() + 10.0),
        pos2(rect.center().x + 2.0, rect.bottom() - label_height - 8.0),
    );
    let y_of = |db: f32| track.bottom() - (db + range) / (2.0 * range) * track.height();
    let mut changed = false;
    if response.double_clicked() {
        *value = 0.0;
        changed = true;
    } else if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let db = (track.bottom() - pos.y) / track.height() * 2.0 * range - range;
        let db = (db.clamp(-range, range) * 10.0).round() / 10.0;
        if db != *value {
            *value = db;
            changed = true;
        }
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(track, 2.0, palette.surface_active);
        let fill = if on { palette.accent } else { palette.dim };
        let (top, bottom) = (y_of(value.max(0.0)), y_of(value.min(0.0)));
        painter.rect_filled(
            Rect::from_min_max(pos2(track.left(), top), pos2(track.right(), bottom)),
            2.0,
            fill,
        );
        painter.hline(
            (track.left() - 3.0)..=(track.right() + 3.0),
            y_of(0.0),
            Stroke::new(1.0, palette.dim),
        );
        let handle = pos2(track.center().x, y_of(*value));
        painter.circle_filled(handle, 7.0, palette.text);
        if response.hovered() || response.dragged() {
            painter.text(
                pos2(track.center().x, rect.top() - 2.0),
                egui::Align2::CENTER_TOP,
                format!("{value:+.1}"),
                theme::regular(theme::SMALL_TEXT),
                palette.secondary,
            );
        }
        painter.text(
            pos2(rect.center().x, rect.bottom()),
            egui::Align2::CENTER_BOTTOM,
            label,
            theme::regular(theme::SMALL_TEXT),
            palette.secondary,
        );
    }
    changed
}

/// The equalizer's response over the audible range, `width` wide, the
/// bands marked on it over their sliders (see [`eq_column`]); the first
/// column holds the preamp's level. The shape says what a row of numbers
/// cannot.
fn eq_curve(ui: &mut egui::Ui, palette: &Palette, settings: &crate::eq::EqSettings, width: f32) {
    use egui::{Shape, Stroke, pos2, vec2};
    let (rect, _) = ui.allocate_exact_size(vec2(width, 96.0), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    painter.rect_filled(rect, theme::RADIUS as f32, palette.surface);
    let (top, bottom) = (rect.top() + 10.0, rect.bottom() - 10.0);
    let y_of = |db: f32| {
        (top + bottom) / 2.0
            - db.clamp(-crate::eq::RANGE_DB, crate::eq::RANGE_DB) / crate::eq::RANGE_DB
                * (bottom - top)
                / 2.0
    };
    let pre = eq_column(rect, 0);
    let first = eq_column(rect, 1);
    let last = eq_column(rect, 10);
    let color = if settings.on {
        palette.accent
    } else {
        palette.dim
    };
    // The grid, over the bands' columns.
    for db in [-12.0, -6.0, 0.0, 6.0, 12.0] {
        let line = if db == 0.0 {
            palette.dim
        } else {
            palette.outline
        };
        painter.hline(first.left()..=last.right(), y_of(db), Stroke::new(1.0, line));
    }
    for band in 1..=10 {
        painter.vline(
            eq_column(rect, band).center().x,
            top..=bottom,
            Stroke::new(1.0, palette.outline),
        );
    }
    // The preamp's column: its level as a short bar over its 0 dB mark.
    let divider = (pre.right() + first.left()) / 2.0;
    painter.vline(divider, top..=bottom, Stroke::new(1.0, palette.outline));
    let half = (pre.width() / 2.0 - 4.0).max(4.0);
    painter.hline(
        (pre.center().x - half)..=(pre.center().x + half),
        y_of(0.0),
        Stroke::new(1.0, palette.dim),
    );
    painter.hline(
        (pre.center().x - half)..=(pre.center().x + half),
        y_of(settings.preamp_db),
        Stroke::new(2.0, color),
    );
    // The response, in pieces between the band centres: the space between
    // two columns is the octaves between their bands, so the curve meets
    // each band over its slider. It runs on to 20 Hz and 20 kHz at the ends.
    let mut knots: Vec<(f32, f32)> = vec![(20.0, first.left())];
    for (band, hz) in crate::eq::BANDS.iter().enumerate() {
        knots.push((*hz, eq_column(rect, band + 1).center().x));
    }
    knots.push((20_000.0, last.right()));
    let curve = settings.curve();
    let steps = 16;
    let mut points = Vec::with_capacity(knots.len() * steps);
    for pair in knots.windows(2) {
        let ((from_hz, from_x), (to_hz, to_x)) = (pair[0], pair[1]);
        let (from_log, to_log) = (from_hz.log2(), to_hz.log2());
        for step in 0..steps {
            let t = step as f32 / steps as f32;
            let hz = (from_log + t * (to_log - from_log)).exp2();
            points.push(pos2(from_x + t * (to_x - from_x), y_of(curve.db_at(hz))));
        }
    }
    points.push(pos2(last.right(), y_of(curve.db_at(20_000.0))));
    painter.add(Shape::line(points, Stroke::new(2.0, color)));
    for (band, db) in settings.bands_db.iter().enumerate() {
        let x = eq_column(rect, band + 1).center().x;
        painter.circle_filled(pos2(x, y_of(db + settings.preamp_db)), 3.0, color);
    }
}

#[cfg(test)]
mod tests {
    use super::{RowText, row_matches, section_matches};

    #[test]
    fn empty_filter_matches_everything() {
        assert!(row_matches(
            "",
            "Normalize volume",
            "Keep loud tracks level"
        ));
        assert!(section_matches(
            "",
            "Playback on this computer",
            &[RowText::new("Status", "Spotify")]
        ));
    }

    #[test]
    fn row_matches_title_or_description_case_insensitively() {
        assert!(row_matches(
            "volume",
            "Normalize volume",
            "Keep loud tracks level"
        ));
        assert!(row_matches(
            "VOLUME",
            "Normalize volume",
            "Keep loud tracks level"
        ));
        assert!(row_matches(
            "quiet",
            "Normalize volume",
            "Keep loud and quiet tracks level"
        ));
        assert!(!row_matches(
            "theme",
            "Normalize volume",
            "Keep loud tracks level"
        ));
    }

    #[test]
    fn section_matches_title_or_any_row() {
        let rows = [
            RowText::new("Normalize volume", "Keep loud and quiet tracks level"),
            RowText::new("Autoplay", "Keep playing similar songs"),
        ];
        assert!(section_matches(
            "playback",
            "Playback on this computer",
            &rows
        ));
        assert!(section_matches(
            "autoplay",
            "Playback on this computer",
            &rows
        ));
        assert!(section_matches("quiet", "Playback on this computer", &rows));
        assert!(!section_matches(
            "theme",
            "Playback on this computer",
            &rows
        ));
    }

    #[test]
    fn the_slider_fill_stops_under_its_handle() {
        let ctx = egui::Context::default();
        let palette = crate::theme::Palette::dark();
        crate::theme::install(&ctx);
        crate::theme::apply(&ctx, &palette);
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let mut style = (**ui.style()).clone();
            super::slider_rail_style(&mut style, &palette);
            // egui paints the accent fill this far past the handle's centre,
            // and the handle's radius is at least this for a slider of the
            // settings' height.
            let overshoot = f32::from(style.visuals.widgets.inactive.corner_radius.nw);
            let handle = style.spacing.interact_size.y / 2.5;
            assert!(overshoot < handle, "{overshoot} >= {handle}");

            super::slider_value_style(&mut style, &palette);
            assert_eq!(
                style.visuals.widgets.inactive.corner_radius,
                egui::CornerRadius::same(14)
            );
        });
        output.textures_delta.clear();
    }

    #[test]
    fn a_setting_slider_draws_its_rail_and_value_box() {
        let ctx = egui::Context::default();
        let palette = crate::theme::Palette::dark();
        crate::theme::install(&ctx);
        crate::theme::apply(&ctx, &palette);
        let mut value = 42u32;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            let response =
                super::setting_slider(ui, &palette, &mut value, 2..=300, |s| s, |v| v.suffix(" s"));
            // The rail and the box sit side by side, so together they are
            // wider than the rail alone.
            assert!(response.rect.width() > ui.spacing().slider_width);
        });
        output.textures_delta.clear();
    }
}

/// A few lines of plain help that wrap to the width of the page.
fn note(ui: &mut egui::Ui, palette: &Palette, text: &str) {
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .font(theme::regular(theme::BODY_TEXT))
                .color(palette.secondary),
        )
        .wrap(),
    );
}


/// Percent-encodes one part of a Last.fm address; Last.fm writes spaces as +.
pub(crate) fn last_fm_part(text: &str) -> String {
    let mut out = String::new();
    for byte in text.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            b' ' => out.push('+'),
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// What Last.fm lacks, two lists side by side: album covers and artist
/// pictures. Each row opens its Last.fm page, and "Done" takes it off the
/// list once the picture is uploaded (it is checked again in a week).
fn missing_lists(ui: &mut egui::Ui, app: &mut App, palette: &Palette) {
    let root = app.dirs.index_dir().join("lastfm-art");
    let now = ui.input(|input| input.time);
    let mut lists: Vec<std::sync::Arc<Vec<String>>> = Vec::new();
    for artists in [false, true] {
        let folder = root.join(if artists { "artists-missing" } else { "albums-missing" });
        let cache_id = egui::Id::new(("missing-list", artists));
        let cached = ui.data(|data| data.get_temp::<(f64, std::sync::Arc<Vec<String>>)>(cache_id));
        let names = match cached {
            Some((at, names)) if now - at < 5.0 => names,
            _ => {
                let mut names: Vec<String> = std::fs::read_dir(&folder)
                    .map(|entries| {
                        entries
                            .filter_map(Result::ok)
                            .filter_map(|entry| {
                                let name = entry.file_name().to_string_lossy().to_string();
                                name.strip_suffix(".jpg").map(str::to_string)
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                names.sort_by_key(|name| name.to_lowercase());
                let names = std::sync::Arc::new(names);
                ui.data_mut(|data| data.insert_temp(cache_id, (now, names.clone())));
                names
            }
        };
        lists.push(names);
    }
    ui.add_space(4.0);
    // Each list is folded away until opened, so its names are only laid out
    // (and its pictures only loaded) while it is on show.
    for index in 0..2 {
        let artists = index == 1;
        let names = lists[index].clone();
        let title = if artists {
            format!("Missing artist pictures ({})", names.len())
        } else {
            format!("Missing album covers ({})", names.len())
        };
        let folder = root.join(if artists { "artists-missing" } else { "albums-missing" });
        egui::CollapsingHeader::new(egui::RichText::new(title).font(theme::semibold(theme::BODY_TEXT)).color(palette.text))
            .id_salt(("missing-fold", artists))
            .default_open(false)
            .show(ui, |ui| {
            if names.is_empty() {
                theme::subtle(ui, palette, "None yet. They are added as you listen.");
                return;
            }
            egui::ScrollArea::vertical()
                .id_salt(("missing-scroll", artists))
                .min_scrolled_height(380.0)
                .max_height((ui.ctx().content_rect().height() * 1.3).max(900.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for name in names.iter() {
                        ui.horizontal(|ui| {
                            // The picture to upload, beside its name.
                            let path = folder.join(format!("{name}.jpg"));
                            ui.add(
                                egui::Image::new(format!("file://{}", path.display()))
                                    .fit_to_exact_size(egui::vec2(40.0, 40.0))
                                    .corner_radius(4.0),
                            );
                            let url = if artists {
                                format!("https://www.last.fm/music/{}", last_fm_part(name))
                            } else if let Some((artist, album)) = name.split_once(" - ") {
                                format!(
                                    "https://www.last.fm/music/{}/{}",
                                    last_fm_part(artist),
                                    last_fm_part(album)
                                )
                            } else {
                                format!("https://www.last.fm/search?q={}", last_fm_part(name))
                            };
                            if theme::soft_button(ui, palette, None, "Done", false)
                                .on_hover_text("Added it to Last.fm? Take it off the list.")
                                .clicked()
                            {
                                app.actions.push(Action::RemoveMissingArt {
                                    artists,
                                    file: name.clone(),
                                });
                                ui.data_mut(|data| {
                                    data.remove::<(f64, std::sync::Arc<Vec<String>>)>(egui::Id::new((
                                        "missing-list",
                                        artists,
                                    )))
                                });
                            }
                            if theme::soft_button(ui, palette, Some(Icon::ExternalLink), "Open", false)
                                .on_hover_text("Open it on Last.fm")
                                .clicked()
                            {
                                app.actions.push(Action::OpenUrl(url));
                            }
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(name.as_str())
                                        .font(theme::regular(theme::BODY_TEXT))
                                        .color(palette.text),
                                )
                                .truncate(),
                            );
                        });
                    }
                });
            });
    }
}


/// Every theme as a small picture of itself, all on show at once. Hovering
/// one shows it on the whole window for a moment (switch that off with the
/// box under the grid); clicking keeps it.
fn theme_grid(app: &mut App, ui: &mut egui::Ui, palette: &crate::theme::Palette) {
    use crate::theme::Palette;
    use egui::{Rect, pos2, vec2};
    // (name, picture, the theme to show while hovering, the action on click)
    let mut tiles: Vec<(u8, String, Palette, Option<Palette>, Action, bool)> = Vec::new();
    for choice in ThemeChoice::ALL {
        let shown = match choice {
            ThemeChoice::Dark => Some(Palette::dark()),
            ThemeChoice::Light => Some(Palette::light()),
            ThemeChoice::System => None,
            ThemeChoice::Custom => Some(Palette::from_picked(
                app.settings.custom_bg,
                app.settings.custom_accent,
            )),
            other => Palette::themed(other),
        };
        let picture = shown.unwrap_or_else(Palette::dark);
        let current = app.settings.custom_theme.is_none() && app.settings.theme == choice;
        // Follow system first, then every dark theme, then every light one.
        let rank = if choice == ThemeChoice::Custom { 0 } else if choice == ThemeChoice::System { 1 } else if picture.dark { 3 } else { 4 };
        tiles.push((
            rank,
            choice.label(app.locale).to_string(),
            picture,
            shown,
            Action::SetTheme(choice),
            current,
        ));
    }
    for theme in app.custom_themes.picker_themes() {
        let current = app.settings.custom_theme.as_deref() == Some(theme.filename.as_str());
        let name = crate::settings::fancy_theme_name(&fastframe_theme::display_name(&theme.filename)).to_string();
        // chanceify's own theme comes second.
        let rank = if name.eq_ignore_ascii_case("chanceify") { 2 } else if theme.palette.dark { 3 } else { 4 };
        tiles.push((
            rank,
            name,
            theme.palette,
            Some(theme.palette),
            Action::SetCustomTheme(theme.filename.clone()),
            current,
        ));
    }
    // All, only the dark ones, or only the light ones.
    let filter_id = egui::Id::new("theme-grid-filter");
    let mut filter: u8 = ui.data(|data| data.get_temp(filter_id)).unwrap_or(0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for (value, label) in [(0u8, "All"), (1, "Dark"), (2, "Light")] {
            if theme::soft_button(ui, palette, None, label, filter == value).clicked() {
                filter = value;
            }
        }
        ui.add_space(10.0);
        let mut point = app.settings.theme_point_preview;
        if widgets::switch_labeled(ui, palette, "Preview a theme when I point at it", &mut point).changed() {
            app.settings.theme_point_preview = point;
            app.mark_settings_dirty();
        }
        ui.add_space(10.0);
        let mut from_cover = app.settings.theme_from_cover;
        if widgets::switch_labeled(ui, palette, "Match the theme to the album cover", &mut from_cover).changed() {
            app.settings.theme_from_cover = from_cover;
            app.settings.accent_from_art = from_cover;
            app.actions.push(Action::SettingsChanged);
        }
    });
    ui.data_mut(|data| data.insert_temp(filter_id, filter));
    tiles.sort_by_key(|tile| tile.0);
    tiles.retain(|(_, _, picture, _, _, current)| match filter {
        1 => picture.dark || *current,
        2 => !picture.dark || *current,
        _ => true,
    });
    let tile = vec2(132.0, 92.0);
    let mut previewing: Option<Palette> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (_, name, picture, preview, action, current) in tiles {
            let (rect, response) = ui.allocate_exact_size(tile, egui::Sense::click());
            let painter = ui.painter();
            let art = Rect::from_min_size(rect.min, vec2(tile.x, 62.0));
            painter.rect_filled(art, 8.0, picture.window);
            // A sidebar strip, a few rows and a play button: how the app looks in it.
            painter.rect_filled(
                Rect::from_min_size(art.min + vec2(0.0, 0.0), vec2(30.0, art.height())),
                egui::CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 },
                picture.panel,
            );
            for row in 0..3 {
                painter.rect_filled(
                    Rect::from_min_size(art.min + vec2(40.0, 10.0 + row as f32 * 13.0), vec2(60.0 - row as f32 * 12.0, 6.0)),
                    3.0,
                    if row == 0 { picture.text } else { picture.secondary },
                );
            }
            painter.circle_filled(art.right_bottom() - vec2(16.0, 14.0), 7.0, picture.accent);
            painter.rect_filled(
                Rect::from_min_size(art.min + vec2(6.0, 8.0), vec2(18.0, 5.0)),
                2.0,
                picture.surface_active,
            );
            painter.rect_stroke(
                art,
                8.0,
                egui::Stroke::new(
                    if current { 2.5 } else if response.hovered() { 1.5 } else { 1.0 },
                    if current { palette.accent } else { picture.outline },
                ),
                egui::StrokeKind::Inside,
            );
            painter.text(
                pos2(rect.left() + 2.0, rect.bottom() - 14.0),
                egui::Align2::LEFT_CENTER,
                name.replace(" (light)", "").replace(" (see-through)", ""),
                theme::regular(theme::SMALL_TEXT),
                if current { palette.accent } else { palette.text },
            );
            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                if app.settings.theme_point_preview && !current {
                    previewing = preview;
                }
            }
            if response.clicked() {
                app.actions.push(action);
            }
        }
    });
    if let Some(shown) = previewing {
        app.theme_preview = Some((shown, std::time::Instant::now()));
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
    }
}
