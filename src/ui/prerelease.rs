//! Native counterpart of Spotify's countdown page for an upcoming album,
//! inside Chanceify's shell: artwork, artist, release countdown and the
//! ordered track-list preview. Unavailable rows are shown but never play.

use std::sync::Arc;
use std::time::Duration;

use egui::{Color32, Frame, Margin, Sense, Stroke, Vec2, vec2};

use super::widgets;
use crate::api::models::pick_image;
use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Page, RowContext};
use crate::prerelease::Prerelease;
use crate::theme::{self, Icon};

pub(super) fn show(app: &mut App, ui: &mut egui::Ui, data: &Prerelease) {
    let palette = app.palette;
    let locale = app.locale;
    let hero_text = if data.color.is_some() {
        Color32::WHITE
    } else {
        palette.text
    };
    let hero_secondary = if data.color.is_some() {
        Color32::from_white_alpha(200)
    } else {
        palette.secondary
    };
    let now = jiff::Timestamp::now().as_second();
    let cover_size = if ui.available_width() > 720.0 {
        212.0
    } else if ui.available_width() > 520.0 {
        160.0
    } else {
        116.0
    };
    if let Some([red, green, blue]) = data.color {
        let bounds = ui.max_rect().expand2(vec2(widgets::PAGE_PADDING, 4.0));
        let top = Color32::from_rgb(red, green, blue);
        let dark = Color32::from_rgb(
            (red as f32 * 0.40) as u8,
            (green as f32 * 0.40) as u8,
            (blue as f32 * 0.40) as u8,
        );
        widgets::paint_vertical_gradient(
            ui,
            egui::Rect::from_min_size(bounds.min, vec2(bounds.width(), cover_size + 104.0)),
            top,
            dark,
        );
        widgets::paint_vertical_gradient(
            ui,
            egui::Rect::from_min_size(
                bounds.min + vec2(0.0, cover_size + 104.0),
                vec2(bounds.width(), 300.0),
            ),
            dark,
            palette.window,
        );
    }
    ui.add_space(12.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 24.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(cover_size), Sense::hover());
        widgets::paint_cover(
            ui,
            &palette,
            pick_image(&data.album.images, 640),
            rect,
            6.0,
            Icon::Music,
            Some(app.backend.art()),
        );
        ui.vertical(|ui| {
            let width = ui.available_width();
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.add_space(cover_size * 0.08);
            theme::text(
                ui,
                gettext(locale, "Upcoming album"),
                theme::medium(theme::SMALL_TEXT),
                hero_text,
            );
            // The title shrinks to fit rather than wrapping, as on other pages.
            let mut size = if cover_size > 200.0 {
                56.0
            } else if cover_size > 130.0 {
                40.0
            } else {
                30.0
            };
            loop {
                let galley = ui.painter().layout_no_wrap(
                    data.album.name.clone(),
                    theme::bold(size),
                    hero_text,
                );
                if galley.size().x <= width || size <= 22.0 {
                    break;
                }
                size -= 6.0;
            }
            let title = crate::bidi::layout(
                ui.painter(),
                &data.album.name,
                theme::bold(size),
                hero_text,
                width,
                2,
                Some(crate::bidi::ELLIPSIS),
            );
            ui.add(egui::Label::new(title).selectable(false));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if let Some(url) = &data.artist_image {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::hover());
                    widgets::paint_cover(
                        ui,
                        &palette,
                        Some(url),
                        rect,
                        12.0,
                        Icon::User,
                        Some(app.backend.art()),
                    );
                }
                for (index, artist) in data.album.artists.iter().enumerate() {
                    if index > 0 {
                        theme::text(ui, ",", theme::regular(theme::BODY_TEXT), hero_secondary);
                    }
                    match &artist.id {
                        Some(id) => {
                            if theme::link(
                                ui,
                                &artist.name,
                                theme::semibold(theme::BODY_TEXT),
                                hero_text,
                            )
                            .clicked()
                            {
                                app.actions.push(Action::Open(Page::Artist(id.clone())));
                            }
                        }
                        None => {
                            theme::text(
                                ui,
                                &artist.name,
                                theme::semibold(theme::BODY_TEXT),
                                hero_text,
                            );
                        }
                    }
                }
                let date = data.album.release_date.clone().or_else(|| {
                    data.release_unix
                        .and_then(|seconds| jiff::Timestamp::from_second(seconds).ok())
                        .map(|timestamp| {
                            timestamp
                                .to_zoned(jiff::tz::TimeZone::system())
                                .strftime("%Y-%m-%d")
                                .to_string()
                        })
                });
                if let Some(date) = date {
                    theme::text(ui, "•", theme::regular(theme::BODY_TEXT), hero_secondary);
                    theme::text(
                        ui,
                        // Translators: {date} is the album's release date.
                        gettext(locale, "Releases {date}")
                            .replace("{date}", &crate::util::format_date(locale, &date)),
                        theme::regular(theme::BODY_TEXT),
                        hero_secondary,
                    );
                }
            });
            if let Some(parts) = data.countdown(now) {
                ui.add_space(6.0);
                countdown(ui, parts, locale, hero_text);
                ui.ctx().request_repaint_after(Duration::from_secs(1));
            }
        });
    });
    ui.add_space(20.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        let more = theme::icon_button(
            ui,
            Icon::Ellipsis,
            26.0,
            palette.secondary,
            palette.text,
            &gettext(locale, "More"),
        );
        egui::Popup::menu(&more)
            .frame(widgets::menu_frame(&palette))
            .show(|ui| {
                ui.set_min_width(180.0);
                if widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::Copy),
                    &gettext(locale, "Copy link"),
                ) {
                    app.actions.push(Action::CopyLink(data.uri.clone()));
                    ui.close();
                }
                if widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::ExternalLink),
                    &gettext(locale, "Open in Spotify"),
                ) {
                    app.actions.push(Action::OpenInSpotify(data.uri.clone()));
                    ui.close();
                }
            });
    });
    ui.add_space(14.0);
    theme::text(
        ui,
        gettext(locale, "Track list preview"),
        theme::bold(theme::HEADING_TEXT),
        palette.text,
    );
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        theme::text(
            ui,
            "#",
            theme::regular(theme::SMALL_TEXT),
            palette.secondary,
        );
        ui.add_space(20.0);
        theme::text(
            ui,
            crate::i18n::pgettext(locale, "column", "Title"),
            theme::regular(theme::SMALL_TEXT),
            palette.secondary,
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(30.0);
            theme::icon(ui, Icon::Clock, 14.0, palette.secondary);
        });
    });
    ui.separator();
    let playable: Arc<[String]> = data
        .tracks
        .iter()
        .flatten()
        .filter(|track| track.is_playable == Some(true))
        .map(|track| track.uri.clone())
        .collect();
    for (index, track) in data.tracks.iter().enumerate() {
        let enabled = track
            .as_ref()
            .is_some_and(|track| track.is_playable == Some(true));
        let color = if enabled { palette.text } else { palette.dim };
        let soft = if enabled {
            palette.secondary
        } else {
            palette.dim
        };
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), theme::ROW_HEIGHT),
            if enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        if response.hovered() {
            ui.painter().rect_filled(rect, 4.0, palette.surface_hover);
        }
        if response.clicked() {
            response.request_focus();
        }
        let mut play_clicked = false;
        if enabled && response.contains_pointer() {
            let button = egui::Rect::from_center_size(
                rect.left_center() + vec2(20.0, 0.0),
                Vec2::splat(24.0),
            );
            theme::paint_icon(ui, Icon::PlayFilled, button, 16.0, palette.text);
            let play = ui.interact(
                button,
                ui.make_persistent_id(("prerelease-play", &data.uri, index)),
                Sense::click(),
            );
            play.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, gettext(locale, "Play"))
            });
            play_clicked = play.clicked();
        } else {
            ui.painter().text(
                rect.left_center() + vec2(20.0, 0.0),
                egui::Align2::CENTER_CENTER,
                (index + 1).to_string(),
                theme::regular(theme::BODY_TEXT),
                palette.secondary,
            );
        }
        // A hidden position has no name yet; it reads as its number.
        let name = track
            .as_ref()
            .map(|track| track.name.clone())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                // Translators: {number} is a track's position on the album.
                gettext(locale, "Track {number}").replace("{number}", &(index + 1).to_string())
            });
        let title_rect = egui::Rect::from_min_size(
            rect.min + vec2(48.0, 9.0),
            vec2((rect.width() - 132.0).max(1.0), rect.height() - 14.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(title_rect), |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(title_rect));
            ui.spacing_mut().item_spacing.y = 1.0;
            theme::text(ui, name, theme::regular(theme::BODY_TEXT), color);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                if track.as_ref().is_some_and(|track| track.explicit) {
                    ui.add(egui::Label::new(
                        egui::RichText::new("E")
                            .font(theme::medium(9.0))
                            .color(palette.window)
                            .background_color(palette.secondary),
                    ));
                    ui.add_space(4.0);
                }
                let artists = track
                    .as_ref()
                    .map(|track| &track.artists)
                    .filter(|artists| !artists.is_empty())
                    .unwrap_or(&data.album.artists);
                for (number, artist) in artists.iter().enumerate() {
                    if number > 0 {
                        theme::text(ui, ", ", theme::regular(theme::SMALL_TEXT), soft);
                    }
                    match &artist.id {
                        Some(id) => {
                            if theme::link(
                                ui,
                                &artist.name,
                                theme::regular(theme::SMALL_TEXT),
                                soft,
                            )
                            .clicked()
                            {
                                app.actions.push(Action::Open(Page::Artist(id.clone())));
                            }
                        }
                        None => {
                            theme::text(ui, &artist.name, theme::regular(theme::SMALL_TEXT), soft);
                        }
                    }
                }
            });
        });
        let duration = track
            .as_ref()
            .filter(|track| track.duration_ms > 0)
            .map(|track| {
                let seconds = track.duration_ms / 1000;
                format!("{}:{:02}", seconds / 60, seconds % 60)
            })
            .unwrap_or_else(|| "-:--".into());
        ui.painter().text(
            rect.right_center() - vec2(30.0, 0.0),
            egui::Align2::RIGHT_CENTER,
            duration,
            theme::regular(theme::BODY_TEXT),
            soft,
        );
        if enabled
            && (play_clicked
                || response.double_clicked()
                || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter))))
            && let Some(track) = track
        {
            // The index among the playable rows, so a song listed twice
            // starts at the occurrence that was clicked.
            let start = data
                .tracks
                .iter()
                .take(index)
                .flatten()
                .filter(|track| track.is_playable == Some(true))
                .count();
            app.actions.push(Action::PlayFromRow {
                context: RowContext::View {
                    uris: Arc::clone(&playable),
                    context_uri: data.album.uri.clone(),
                    editable_playlist: None,
                },
                uri: track.uri.clone(),
                index: start as u32,
            });
        }
    }
    ui.add_space(24.0);
    for copyright in &data.album.copyrights {
        theme::text(
            ui,
            &copyright.text,
            theme::regular(theme::TINY_TEXT),
            palette.dim,
        );
    }
}

fn countdown(ui: &mut egui::Ui, parts: [i64; 4], locale: crate::i18n::Locale, color: Color32) {
    let column_width = ((ui.available_width() - 27.0) / 4.0).clamp(36.0, 72.0);
    let values = parts.into_iter().zip([
        gettext(locale, "days"),
        gettext(locale, "hours"),
        gettext(locale, "minutes"),
        gettext(locale, "seconds"),
    ]);
    Frame::new()
        .fill(Color32::from_black_alpha(105))
        .corner_radius(egui::CornerRadius::same(theme::RADIUS))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for (index, (number, label)) in values.enumerate() {
                    if index > 0 {
                        let (rect, _) = ui.allocate_exact_size(vec2(1.0, 24.0), Sense::hover());
                        ui.painter().line_segment(
                            [rect.center_top(), rect.center_bottom()],
                            Stroke::new(1.0, Color32::from_white_alpha(65)),
                        );
                    }
                    ui.allocate_ui_with_layout(
                        vec2(column_width, 42.0),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            theme::text(
                                ui,
                                number.to_string(),
                                theme::bold(theme::HEADING_TEXT),
                                color,
                            );
                            theme::text(
                                ui,
                                label,
                                theme::regular(theme::TINY_TEXT),
                                color.gamma_multiply(0.7),
                            );
                        },
                    );
                }
            });
        });
}
