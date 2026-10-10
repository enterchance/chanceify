//! Every speed scene on one panel, with the playing song's own lock.
//!
//! The scenes used to live only in a right-click menu and on one key. That
//! is enough to *use* one and useless to *choose* one: a list of nine names
//! in a menu says nothing about what Hardtekk does, and there is nowhere to
//! see what is on right now. This is the panel behind Shift+P — every scene
//! with its rate, the curve that goes with it and one line saying what it is
//! for, the scene playing right now named at the top, and a lock that gives
//! the song being played a scene of its own.

use crate::app::App;
use crate::i18n::gettext;
use crate::model::Action;

/// Opens the panel.
pub fn open(app: &mut App) {
    app.scene_browser = true;
}

/// Closes it.
pub fn close(app: &mut App) {
    app.scene_browser = false;
}

/// One line per scene, saying what it is for rather than repeating its
/// name. A list that only names things cannot be chosen from.
pub fn describe(preset: &crate::speed::Preset) -> &'static str {
    match preset.name {
        "Normal" => "As it was written",
        "Warm" => "Full bass, same speed",
        "Punch" => "Bolder lows for anything with a beat",
        "Nightcore" => "Faster, with the curve laptop speakers like",
        "Hardtekk" => "Fast and hard, the way the genre is mixed",
        "Gabber" => "Faster still, club curve",
        "Chipmunk" => "Very fast — for laughs as much as anything",
        "Slowed" => "Slower, softer curve",
        "Half speed" => "Half time, for the long way round",
        _ => "",
    }
}

/// The panel, drawn once a frame while it is open.
pub fn panel(app: &mut App, ui: &mut egui::Ui) {
    if !app.scene_browser {
        return;
    }
    let ctx = ui.ctx().clone();
    let palette = app.palette;
    let song = app.now_playing();
    let uri = app.current_track_uri();
    let locked = uri
        .as_ref()
        .and_then(|uri| app.settings.song_scenes.get(uri).copied())
        .filter(|index| *index < crate::speed::PRESETS.len());
    let current = crate::speed::index_of_speed(crate::speed::clamped(app.settings.playback_speed));
    let rate = crate::speed::clamped(app.settings.playback_speed);
    let locked_count = app.settings.song_scenes.len();
    egui::Area::new(egui::Id::new("chanceify-scenes"))
        .fixed_pos(egui::pos2(24.0, 72.0))
        .order(egui::Order::Tooltip)
        .show(&ctx, |ui| {
            egui::Frame::new()
                .fill(palette.overlay)
                .stroke(egui::Stroke::new(1.0, palette.outline))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_max_width(380.0);
                    ui.horizontal(|ui| {
                        ui.heading(gettext(app.locale, "Scenes"));
                        ui.label(egui::RichText::new("Shift+P").small().color(palette.dim));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("✕").clicked() {
                                close(app);
                            }
                        });
                    });
                    ui.separator();
                    // What is playing, and what it is being played at. The
                    // first question about a scene is always "is this one
                    // on?", so it is answered before the list.
                    let name = song
                        .as_ref()
                        .map(|now| now.title.clone())
                        .unwrap_or_else(|| gettext(app.locale, "Nothing is playing").into_owned());
                    ui.label(egui::RichText::new(name).strong());
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  ·  {:.2}×  ·  {}",
                            gettext(app.locale, crate::speed::preset(current).name),
                            rate,
                            gettext(
                                app.locale,
                                crate::speed::preset(current).eq.unwrap_or("Flat")
                            )
                        ))
                        .color(palette.dim),
                    );
                    ui.separator();

                    for (index, preset) in crate::speed::PRESETS.iter().enumerate() {
                        egui::Frame::new()
                            .fill(if index == current {
                                palette.surface_hover
                            } else {
                                palette.surface
                            })
                            .corner_radius(egui::CornerRadius::same(6))
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.horizontal(|ui| {
                                            if index == current {
                                                ui.label("▶");
                                            }
                                            ui.label(
                                                egui::RichText::new(gettext(
                                                    app.locale,
                                                    preset.name,
                                                ))
                                                .strong(),
                                            );
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "{:.2}×",
                                                    preset.speed
                                                ))
                                                .color(palette.dim),
                                            );
                                        });
                                        ui.label(
                                            egui::RichText::new(gettext(
                                                app.locale,
                                                describe(preset),
                                            ))
                                            .small()
                                            .color(palette.secondary),
                                        );
                                        ui.label(
                                            egui::RichText::new(gettext(
                                                app.locale,
                                                preset.eq.unwrap_or("Flat"),
                                            ))
                                            .small()
                                            .color(palette.accent),
                                        );
                                    });
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            // The two things a scene is for:
                                            // hear it, or keep it for this
                                            // song. Both on the row, so
                                            // neither has to be found.
                                            if ui
                                                .button("🔒")
                                                .on_hover_text(gettext(
                                                    app.locale,
                                                    "Give the playing song this scene",
                                                ))
                                                .clicked()
                                                && let Some(uri) = uri.clone()
                                            {
                                                app.actions.push(Action::SetSongScene {
                                                    uri,
                                                    // Pressing it on the scene
                                                    // already locked is how a
                                                    // lock is taken off again.
                                                    index: (locked != Some(index)).then_some(index),
                                                });
                                            }
                                            if ui
                                                .button("▶")
                                                .on_hover_text(gettext(
                                                    app.locale,
                                                    "Play at this rate",
                                                ))
                                                .clicked()
                                            {
                                                app.actions.push(Action::SetSpeedPreset(index));
                                            }
                                        },
                                    );
                                });
                            });
                    }
                    ui.separator();
                    // The lock, said plainly rather than left to a
                    // right-click nobody will find.
                    ui.horizontal(|ui| match uri.clone() {
                        Some(uri) => match locked {
                            Some(index) => {
                                if ui.button("Unlock this song").clicked() {
                                    app.actions.push(Action::SetSongScene { uri, index: None });
                                }
                                ui.label(
                                    egui::RichText::new(format!(
                                        "locked to {}",
                                        gettext(app.locale, crate::speed::preset(index).name)
                                    ))
                                    .small()
                                    .color(palette.dim),
                                );
                            }
                            None => {
                                if ui
                                    .button(gettext(app.locale, "Lock this song to the scene on"))
                                    .clicked()
                                {
                                    app.actions.push(Action::SetSongScene {
                                        uri,
                                        index: Some(current),
                                    });
                                }
                            }
                        },
                        None => {
                            ui.label(
                                egui::RichText::new(gettext(
                                    app.locale,
                                    "Play something to lock a scene to it",
                                ))
                                .small()
                                .color(palette.dim),
                            );
                        }
                    });
                    ui.label(
                        egui::RichText::new(format!(
                            "{} {}",
                            locked_count,
                            gettext(app.locale, "songs have a scene of their own")
                        ))
                        .small()
                        .color(palette.dim),
                    );
                });
        });
}
