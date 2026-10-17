//! Modal dialogs: playlist details, confirmations, shortcuts.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Stroke};

use crate::app::App;
use crate::i18n::{Locale, gettext, ngettext};
use crate::model::{Action, Dialog};
use crate::theme;

pub fn show(app: &mut App, ctx: &egui::Context) {
    let Some(dialog) = app.dialog.clone() else {
        return;
    };
    let palette = app.palette;
    let locale = app.locale;
    let frame = Frame::new()
        .fill(palette.overlay)
        .stroke(Stroke::new(1.0, palette.outline))
        .corner_radius(CornerRadius::same(theme::RADIUS + 4))
        .inner_margin(Margin::same(24))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 40,
            spread: 0,
            color: palette.shadow,
        });
    // The shortcuts list floats instead of blocking the window, so the app
    // can still be moved and resized while it is open.
    let floating = matches!(dialog, Dialog::Shortcuts);
    let body = |ui: &mut egui::Ui| {
            if !floating {
                ui.set_width(420.0);
            }
            match dialog {
                Dialog::PersonalAppIntro => {
                    theme::text(ui, gettext(locale, "Spotify is making you wait"), theme::bold(theme::HEADING_TEXT), palette.text);
                    ui.add_space(10.0);
                    ui.add(egui::Label::new(egui::RichText::new(gettext(locale, "When it gets busy, Spotify slows down shared connections. With Premium, you can set up your own faster one.")).font(theme::regular(theme::BODY_TEXT)).color(palette.secondary)).wrap());
                    ui.add_space(14.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 34.0), Layout::right_to_left(Align::Center), |ui| {
                        if theme::pill_button(ui, &palette, &gettext(locale, "Set up personal app"), true).clicked() {
                            app.actions.push(Action::OpenPersonalAppSetup);
                        }
                        if theme::pill_button(ui, &palette, &gettext(locale, "Not now"), false).clicked() {
                            app.actions.push(Action::CloseDialog);
                        }
                    });
                }
                Dialog::CreatePlaylist { .. } => create_playlist(app, ui),
                Dialog::EditPlaylist { .. } => edit_playlist(app, ui),
                Dialog::ConfirmDeletePlaylist { id, name, owned } => {
                    theme::text(
                        ui,
                        if owned {
                            gettext(locale, "Delete playlist?")
                        } else {
                            gettext(locale, "Remove from Your Library?")
                        },
                        theme::bold(theme::HEADING_TEXT),
                        palette.text,
                    );
                    ui.add_space(8.0);
                    let body = if owned {
                        // Translators: {name} is a playlist name.
                        gettext(locale, "Delete “{name}”? You can recover it from Spotify for 90 days.")
                    } else {
                        // Translators: {name} is a playlist name.
                        gettext(locale, "“{name}” will no longer appear in Your Library.")
                    }
                    .replace("{name}", &name);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(body)
                                .font(theme::regular(theme::BODY_TEXT))
                                .color(palette.secondary),
                        )
                        .wrap(),
                    );
                    ui.add_space(20.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 34.0), Layout::right_to_left(Align::Center), |ui| {
                        if theme::pill_button(
                            ui,
                            &palette,
                            &if owned {
                                gettext(locale, "Delete")
                            } else {
                                gettext(locale, "Remove")
                            },
                            true,
                        )
                        .clicked()
                        {
                            app.actions.push(Action::DeletePlaylist(id.clone()));
                        }
                        if theme::pill_button(ui, &palette, &gettext(locale, "Cancel"), false).clicked() {
                            app.actions.push(Action::CloseDialog);
                        }
                    });
                }
                Dialog::ConfirmPlaylistDuplicates {
                    playlist_id,
                    playlist_name,
                    items,
                    position,
                    duplicate_uris,
                } => {
                    let multiple = items.len() > 1;
                    theme::text(
                        ui,
                        if multiple {
                            gettext(locale, "Songs already in this playlist")
                        } else {
                            gettext(locale, "Song already in this playlist")
                        },
                        theme::bold(theme::HEADING_TEXT),
                        palette.text,
                    );
                    ui.add_space(8.0);
                    let body = duplicate_message(locale, &playlist_name, &items, &duplicate_uris);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(body)
                                .font(theme::regular(theme::BODY_TEXT))
                                .color(palette.secondary),
                        )
                        .wrap(),
                    );
                    ui.add_space(20.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 34.0), Layout::right_to_left(Align::Center), |ui| {
                        if theme::pill_button(ui, &palette, &gettext(locale, "Add anyway"), true).clicked() {
                            app.actions.push(Action::ConfirmAddToPlaylist {
                                playlist_id: playlist_id.clone(),
                                playlist_name: playlist_name.clone(),
                                items: items.clone(),
                                position,
                            });
                        }
                        if theme::pill_button(ui, &palette, &gettext(locale, "Cancel"), false).clicked() {
                            app.actions.push(Action::CloseDialog);
                        }
                    });
                }
                Dialog::Shortcuts => {
                    // Only while a key is waiting for its new key.
                    if super::keys::rebinding(ui.ctx()).is_some() {
                        theme::text(
                            ui,
                            "Press a key (Shift and Ctrl count too), or right-click it to clear. Escape cancels.",
                            theme::regular(theme::SMALL_TEXT),
                            palette.accent,
                        );
                    } else {
                        ui.add_space(16.0);
                    }
                    ui.add_space(6.0);
                    // One live list: what is shown here is what the keys do now.
                    // The window can be dragged to any size; the list fills it.
                    ui.set_min_width(260.0);
                    // The list's height follows the window's own height, not what
                    // the content asks for, so the window can be made any size
                    // (it used to spring back to the full screen height).
                    // The window's own room, less the footer measured last frame:
                    // nothing here depends on the content, so the window can be
                    // dragged to any size.
                    let footer_id = egui::Id::new("keybinds-footer-height");
                    let footer_h = ctx.data(|data| data.get_temp::<f32>(footer_id)).unwrap_or(90.0);
                    let outer_width = ui.available_width();
                    let list_height = (ui.available_height() - footer_h - 8.0).clamp(60.0, ctx.content_rect().height());
                    egui::ScrollArea::vertical()
                        .max_height(list_height)
                        .min_scrolled_height(60.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                    let waiting = super::keys::rebinding(ui.ctx());
                    // Anything not placed in a group still shows, under "More".
                    let placed: Vec<&str> = super::keys::CATEGORIES
                        .iter()
                        .flat_map(|(_, ids)| ids.iter().copied())
                        .collect();
                    let mut groups: Vec<(&str, Vec<&super::keys::Bindable>)> = super::keys::CATEGORIES
                        .iter()
                        .map(|(name, ids)| {
                            let list = ids
                                .iter()
                                .filter_map(|id| super::keys::BINDABLE.iter().find(|b| b.id == *id))
                                .collect();
                            (*name, list)
                        })
                        .collect();
                    let rest: Vec<_> = super::keys::BINDABLE
                        .iter()
                        .filter(|b| !placed.contains(&b.id))
                        .collect();
                    if !rest.is_empty() {
                        groups.push(("More", rest));
                    }
                    // Each category is a vertical column of its own, side by
                    // side; a category goes under the shortest column so the
                    // columns stay about level.
                    let shown: Vec<&(&str, Vec<&super::keys::Bindable>)> =
                        groups
                            .iter()
                            .filter(|(name, list)| !list.is_empty() || !super::keys::fixed_for(name).is_empty())
                            .collect();
                    // The room is the window's own width (the list lives inside a
                    // scroll area, which can report less), so the columns follow the
                    // size the window is dragged to: as many as keep every label readable.
                    let total = (outer_width - 14.0).max(240.0);
                    let column_count = (((total + 16.0) / 316.0).floor() as usize).clamp(1, 5);
                    let mut lanes: Vec<Vec<usize>> = vec![Vec::new(); column_count];
                    let mut heights = vec![0usize; column_count];
                    for (index, (group, list)) in shown.iter().enumerate() {
                        let lane = (0..column_count).min_by_key(|lane| heights[*lane]).unwrap_or(0);
                        lanes[lane].push(index);
                        heights[lane] += list.len() + super::keys::fixed_for(group).len() + 2;
                    }
                    let lane_width = ((total - 16.0 * column_count as f32) / column_count as f32).max(240.0);
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 16.0;
                        for lane in &lanes {
                            ui.vertical(|ui| {
                                ui.set_width(lane_width);
                                for index in lane {
                                    let (group_name, bindables) = shown[*index];
                                    theme::text(ui, *group_name, theme::semibold(theme::BODY_TEXT), palette.text);
                                    ui.add_space(4.0);
                                    for bindable in bindables {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 8.0;
                                            let is_waiting = waiting.as_deref() == Some(bindable.id);
                                            let label = if is_waiting {
                                                "Press a key…".to_string()
                                            } else if let Some(text) = super::keys::chord_label(app, bindable.id) {
                                                text
                                            } else {
                                                "Not set".to_string()
                                            };
                                            let pill = ui
                                                .allocate_ui(egui::vec2(112.0, 26.0), |ui| {
                                                    theme::pill_button(ui, &palette, &label, is_waiting)
                                                })
                                                .inner;
                                            if pill.clicked() {
                                                super::keys::start_rebinding(ui.ctx(), bindable.id);
                                            }
                                            // Right-click clears a key.
                                            if pill.secondary_clicked() {
                                                app.settings.key_bindings.insert(
                                                    bindable.id.to_string(),
                                                    super::keys::UNSET.name().to_string(),
                                                );
                                                app.actions.push(Action::SettingsChanged);
                                            }
                                            ui.allocate_ui_with_layout(egui::vec2((lane_width - 120.0).max(60.0), 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        egui::RichText::new(bindable.label)
                                                            .font(theme::regular(theme::SMALL_TEXT))
                                                            .color(palette.text),
                                                    )
                                                    .truncate()
                                                    .selectable(false),
                                                );
                                            });
                                        });
                                    }
                                    for (keys, what) in super::keys::fixed_for(group_name) {
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 8.0;
                                            // A key that cannot move: the same pill shape
                                            // as the others, only not clickable.
                                            let (key_box, _) = ui.allocate_exact_size(
                                                egui::vec2(112.0, 26.0),
                                                egui::Sense::hover(),
                                            );
                                            ui.painter().rect_filled(
                                                key_box,
                                                egui::CornerRadius::same(13),
                                                palette.surface,
                                            );
                                            ui.painter().text(
                                                key_box.center(),
                                                egui::Align2::CENTER_CENTER,
                                                keys,
                                                theme::semibold(theme::SMALL_TEXT),
                                                palette.dim,
                                            );
                                            ui.allocate_ui_with_layout(egui::vec2((lane_width - 120.0).max(60.0), 26.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        egui::RichText::new(what)
                                                            .font(theme::regular(theme::SMALL_TEXT))
                                                            .color(palette.secondary),
                                                    )
                                                    .truncate()
                                                    .selectable(false),
                                                );
                                            });
                                        });
                                    }
                                    ui.add_space(12.0);
                                }
                            });
                        }
                    });
                        });
                    let footer_top = ui.cursor().top();
                    ui.add_space(12.0);
                    // Wrapping rows, so a narrow window never has to grow to
                    // fit the footer.
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new("Ctrl+click picks songs. Queue a song with the key bound here.")
                                .font(theme::regular(theme::BODY_TEXT))
                                .color(palette.secondary),
                        )
                        .wrap()
                        .selectable(false),
                    );
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if theme::pill_button(ui, &palette, "Reset", false).clicked() {
                            app.settings.key_bindings.clear();
                            app.actions.push(Action::SettingsChanged);
                        }
                        if theme::pill_button(ui, &palette, "Export", false).clicked() {
                            app.actions.push(Action::ExportKeys);
                        }
                        if theme::pill_button(ui, &palette, "Import", false).clicked() {
                            app.actions.push(Action::ImportKeys);
                        }
                        if theme::pill_button(ui, &palette, "Save as the default for everyone", false).clicked() {
                            app.actions.push(Action::SaveKeysAsDefault);
                        }
                        if theme::pill_button(ui, &palette, &gettext(locale, "Done"), true).clicked() {
                            app.actions.push(Action::CloseDialog);
                        }
                    });
                    let used = ui.min_rect().bottom() - footer_top;
                    ctx.data_mut(|data| data.insert_temp(footer_id, used.max(40.0)));
                }
                Dialog::Views => {
                    theme::text(ui, "Views", theme::bold(theme::HEADING_TEXT), palette.text);
                    ui.add_space(4.0);
                    ui.add_space(10.0);
                    let rows: [(&str, bool, Action); 10] = [
                        ("Library", app.settings.sidebar_visible, Action::ToggleSidebar),
                        ("Queue", app.show_queue_panel, Action::ToggleQueuePanel),
                        ("Side lyrics", app.show_lyrics_panel, Action::ToggleLyricsPanel),
                        ("Full screen lyrics", app.lyrics_fullscreen.is_some(), Action::ToggleLyricsFullscreen),
                        ("Full screen visualizer", app.fullscreen_vis, Action::ToggleFullscreenVis),
                        ("Settings", app.vis_panel, Action::ToggleVisPanel),
                        ("Visualizer", app.settings.vis_shapes_value() != 0, Action::ToggleVisShapes),
                        ("Big album art", app.settings.art_expanded, Action::ToggleArtExpanded),
                        ("Mini player", app.mini_active, Action::ToggleMiniPlayer),
                        ("Keybinds", false, Action::ShowDialog(Dialog::Shortcuts)),
                    ];
                    for (label, on, action) in rows {
                        let mut value = on;
                        if super::widgets::switch(ui, &palette, label, &mut value).changed() {
                            app.actions.push(action);
                        }
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if theme::pill_button(ui, &palette, "Normal view", false).clicked() {
                            app.actions.push(Action::NormalView);
                        }
                        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 34.0), Layout::right_to_left(Align::Center), |ui| {
                            if theme::pill_button(ui, &palette, &gettext(locale, "Done"), true).clicked() {
                                app.actions.push(Action::CloseDialog);
                            }
                        });
                    });
                }
                Dialog::PremiumNeeded => {
                    theme::text(
                        ui,
                        gettext(locale, "This account cannot play music here"),
                        theme::bold(theme::HEADING_TEXT),
                        palette.text,
                    );
                    ui.add_space(8.0);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(gettext(
                                locale,
                                "Playback needs Spotify Premium. Free accounts can browse and search, but cannot play music through chanceify™.",
                            ))
                            .font(theme::regular(theme::BODY_TEXT))
                            .color(palette.secondary),
                        )
                        .wrap(),
                    );
                    ui.add_space(20.0);
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 34.0), Layout::right_to_left(Align::Center), |ui| {
                        if theme::pill_button(ui, &palette, &gettext(locale, "OK"), true).clicked() {
                            app.actions.push(Action::CloseDialog);
                        }
                    });
                }
            }
    };
    if floating {
        let mut open = true;
        let screen = ctx.content_rect();
        let shown = egui::Window::new("")
            .id(egui::Id::new("dialog"))
            .open(&mut open)
            .order(egui::Order::Foreground)
            .frame(frame)
            .default_pos(egui::pos2(80.0, 60.0))
            .default_size([
                (screen.width() - 160.0).clamp(420.0, 1200.0),
                (screen.height() - 200.0).max(240.0),
            ])
            .min_size([300.0, 240.0])
            .resizable(true)
            .collapsible(false)
            .show(ctx, body);
        if let Some(shown) = shown {
            app.dialog_rect = Some(shown.response.rect);
        }
        if !open {
            app.actions.push(Action::CloseDialog);
        }
    } else {
        let response = egui::Modal::new(egui::Id::new("dialog"))
            .frame(frame)
            .backdrop_color(egui::Color32::from_black_alpha(if palette.dark {
                150
            } else {
                80
            }))
            .show(ctx, body);
        app.dialog_rect = Some(response.response.rect);
        if response.should_close() {
            app.actions.push(Action::CloseDialog);
        }
    }
}

fn duplicate_message(
    locale: Locale,
    playlist_name: &str,
    items: &[crate::api::models::PlayableItem],
    duplicate_uris: &[String],
) -> String {
    let mut seen = std::collections::HashSet::new();
    let names: Vec<&str> = items
        .iter()
        .filter(|item| duplicate_uris.iter().any(|uri| uri == item.uri()))
        .map(crate::api::models::PlayableItem::name)
        .filter(|name| seen.insert(*name))
        .collect();
    let one_item = items.len() == 1;
    let message = match names.as_slice() {
        [] if one_item => {
            // Translators: {playlist} is a playlist name.
            gettext(
                locale,
                "This song is already in “{playlist}”. Add it again?",
            )
        }
        [] => {
            // Translators: {playlist} is a playlist name.
            gettext(
                locale,
                "This song is already in “{playlist}”. Add them anyway?",
            )
        }
        [name] if one_item => {
            // Translators: {name} is a song name and {playlist} a playlist name.
            gettext(locale, "“{name}” is already in “{playlist}”. Add it again?")
                .replace("{name}", name)
                .into()
        }
        [name] => {
            // Translators: {name} is a song name and {playlist} a playlist name.
            gettext(
                locale,
                "“{name}” is already in “{playlist}”. Add them anyway?",
            )
            .replace("{name}", name)
            .into()
        }
        [first, second] => {
            // Translators: {first} and {second} are song names, {playlist} a playlist name.
            gettext(
                locale,
                "“{first}” and “{second}” are already in “{playlist}”. Add them anyway?",
            )
            .replace("{first}", first)
            .replace("{second}", second)
            .into()
        }
        [first, second, rest @ ..] => ngettext(
            locale,
            // Translators: {first} and {second} are song names, {count} is how many more songs are already in the playlist, and {playlist} is a playlist name.
            "“{first}”, “{second}”, and {count} more are already in “{playlist}”. Add them anyway?",
            "“{first}”, “{second}”, and {count} more are already in “{playlist}”. Add them anyway?",
            rest.len() as u32,
        )
        .replace("{first}", first)
        .replace("{second}", second)
        .replace("{count}", &rest.len().to_string())
        .into(),
    };
    message.replace("{playlist}", playlist_name)
}

fn text_field(
    ui: &mut egui::Ui,
    palette: &theme::Palette,
    locale: crate::i18n::Locale,
    id: &str,
    text: &mut String,
    hint: &str,
    focus: bool,
) -> egui::Response {
    let response = Frame::new()
        .fill(palette.surface)
        .corner_radius(CornerRadius::same(6))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            super::widgets::text_edit(
                ui,
                locale,
                egui::TextEdit::singleline(text)
                    .id(egui::Id::new(id))
                    .hint_text(egui::RichText::new(hint).color(palette.dim))
                    .font(theme::regular(theme::BODY_TEXT))
                    .frame(egui::Frame::NONE)
                    .desired_width(f32::INFINITY),
            )
        })
        .inner;
    if focus && ui.memory(|memory| memory.focused().is_none()) {
        response.request_focus();
    }
    response
}

fn create_playlist(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let busy = app.playlist_busy;
    let Some(Dialog::CreatePlaylist {
        name,
        public,
        add_uris,
    }) = &mut app.dialog
    else {
        return;
    };
    theme::text(
        ui,
        gettext(locale, "New playlist"),
        theme::bold(theme::HEADING_TEXT),
        palette.text,
    );
    ui.add_space(12.0);
    theme::text(
        ui,
        gettext(locale, "Name"),
        theme::medium(theme::BODY_TEXT),
        palette.secondary,
    );
    let field = text_field(
        ui,
        &palette,
        locale,
        "playlist-name",
        name,
        &gettext(locale, "My playlist"),
        true,
    );
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        super::widgets::switch(ui, &palette, &gettext(locale, "Public playlist"), public);
        theme::text(
            ui,
            gettext(locale, "Public playlist"),
            theme::regular(theme::BODY_TEXT),
            palette.text,
        );
    });
    if !add_uris.is_empty() {
        ui.add_space(6.0);
        let count = add_uris.len();
        theme::text(
            ui,
            ngettext(
                locale,
                // Translators: {count} is the number of songs.
                "{count} song will be added.",
                "{count} songs will be added.",
                count as u32,
            )
            .replace("{count}", &count.to_string()),
            theme::regular(theme::BODY_TEXT),
            palette.secondary,
        );
    }
    ui.add_space(20.0);
    let submit = field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
    let name_value = name.trim().to_string();
    let public_value = *public;
    let uris = add_uris.clone();
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 34.0), Layout::right_to_left(Align::Center), |ui| {
        if busy {
            theme::spinner(ui, 18.0, palette.accent);
        } else {
            let create = theme::pill_button(ui, &palette, &gettext(locale, "Create"), true)
                .clicked()
                || submit;
            if create && !name_value.is_empty() {
                app.actions.push(Action::CreatePlaylist {
                    name: name_value.clone(),
                    public: public_value,
                    add_uris: uris.clone(),
                });
            }
            if theme::pill_button(ui, &palette, &gettext(locale, "Cancel"), false).clicked() {
                app.actions.push(Action::CloseDialog);
            }
        }
    });
}

fn edit_playlist(app: &mut App, ui: &mut egui::Ui) {
    let existing_image = if let Some(Dialog::EditPlaylist { id, .. }) = &app.dialog {
        app.library
            .playlists
            .get()
            .and_then(|playlists| playlists.iter().find(|playlist| &playlist.id == id))
            .and_then(|playlist| crate::api::models::pick_image(&playlist.images, 100))
            .map(str::to_owned)
    } else {
        None
    };
    let palette = app.palette;
    let locale = app.locale;
    let busy = app.playlist_busy;
    let Some(Dialog::EditPlaylist {
        cover,
        id,
        name,
        description,
        public,
    }) = &mut app.dialog
    else {
        return;
    };
    theme::text(
        ui,
        gettext(locale, "Edit details"),
        theme::bold(theme::HEADING_TEXT),
        palette.text,
    );
    ui.add_space(12.0);
    egui::ScrollArea::vertical()
        .max_height((ui.ctx().content_rect().height() - 210.0).max(100.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(selected) = &cover.selection {
                    ui.ctx()
                        .include_bytes(selected.uri.clone(), selected.jpeg.clone());
                    ui.add(
                        egui::Image::new(selected.uri.clone())
                            .fit_to_exact_size(egui::vec2(72.0, 72.0))
                            .alt_text(gettext(locale, "Selected playlist cover")),
                    );
                } else if let Some(url) = &existing_image {
                    ui.add(
                        egui::Image::new(url)
                            .fit_to_exact_size(egui::vec2(72.0, 72.0))
                            .alt_text(gettext(locale, "Current playlist cover")),
                    );
                }
                ui.vertical(|ui| {
                    if cover.request.is_some() || cover.uploading.is_some() {
                        theme::spinner(ui, 18.0, palette.accent);
                        theme::text(
                            ui,
                            if cover.uploading.is_some() {
                                gettext(locale, "Uploading cover…")
                            } else {
                                gettext(locale, "Choosing cover…")
                            },
                            theme::regular(theme::BODY_TEXT),
                            palette.secondary,
                        );
                    } else {
                        if theme::pill_button(ui, &palette, &gettext(locale, "Change cover"), false)
                            .clicked()
                        {
                            app.actions.push(Action::ChoosePlaylistCover(id.clone()));
                        }
                        if cover.selection.is_some()
                            && theme::pill_button(
                                ui,
                                &palette,
                                &gettext(locale, "Upload cover"),
                                true,
                            )
                            .clicked()
                        {
                            app.actions.push(Action::UploadPlaylistCover(id.clone()));
                        }
                    }
                });
            });
            if let Some(error) = &cover.error {
                ui.add(egui::Label::new(egui::RichText::new(error).color(palette.text)).wrap());
            }
            ui.add_space(10.0);
            theme::text(
                ui,
                gettext(locale, "Name"),
                theme::medium(theme::BODY_TEXT),
                palette.secondary,
            );
            text_field(
                ui,
                &palette,
                locale,
                "edit-name",
                name,
                &gettext(locale, "Playlist name"),
                true,
            );
            ui.add_space(10.0);
            theme::text(
                ui,
                gettext(locale, "Description"),
                theme::medium(theme::BODY_TEXT),
                palette.secondary,
            );
            Frame::new()
                .fill(palette.surface)
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    super::widgets::text_edit(
                        ui,
                        locale,
                        egui::TextEdit::multiline(description)
                            .id(egui::Id::new("edit-description"))
                            .hint_text(
                                egui::RichText::new(gettext(locale, "Optional description"))
                                    .color(palette.dim),
                            )
                            .font(theme::regular(theme::BODY_TEXT))
                            .frame(egui::Frame::NONE)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                // Unknown shows as off; only a change of the switch is sent, so
                // a playlist nothing has described keeps whatever it was.
                let mut shown = public.unwrap_or(false);
                if super::widgets::switch(
                    ui,
                    &palette,
                    &gettext(locale, "Public playlist"),
                    &mut shown,
                )
                .changed()
                {
                    *public = Some(shown);
                }
                theme::text(
                    ui,
                    gettext(locale, "Public playlist"),
                    theme::regular(theme::BODY_TEXT),
                    palette.text,
                );
            });
        });
    ui.add_space(20.0);
    let id = id.clone();
    let busy = busy || cover.uploading.is_some() || cover.request.is_some();
    let name_value = name.trim().to_string();
    let description_value = description.trim().to_string();
    let public_value = *public;
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 34.0), Layout::right_to_left(Align::Center), |ui| {
        if busy {
            theme::spinner(ui, 18.0, palette.accent);
        } else {
            if theme::pill_button(ui, &palette, &gettext(locale, "Save"), true).clicked()
                && !name_value.is_empty()
            {
                app.actions.push(Action::UpdatePlaylist {
                    id: id.clone(),
                    name: name_value.clone(),
                    description: description_value.clone(),
                    public: public_value,
                });
            }
            if theme::pill_button(ui, &palette, &gettext(locale, "Cancel"), false).clicked() {
                app.actions.push(Action::CloseDialog);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::duplicate_message;
    use crate::api::models::{PlayableItem, Track};
    use crate::i18n::Locale;

    fn song(uri: &str, name: &str) -> PlayableItem {
        PlayableItem::Track(Track {
            uri: uri.into(),
            name: name.into(),
            ..Default::default()
        })
    }

    #[test]
    fn duplicate_dialog_names_the_song() {
        let items = vec![song("spotify:track:honey", "Honey")];
        let message = duplicate_message(
            Locale::English,
            "The best music ever",
            &items,
            &["spotify:track:honey".into()],
        );

        assert_eq!(
            message,
            "“Honey” is already in “The best music ever”. Add it again?"
        );
    }

    #[test]
    fn duplicate_dialog_names_only_the_duplicates_in_a_selection() {
        let items = vec![
            song("spotify:track:honey", "Honey"),
            song("spotify:track:new", "New song"),
        ];
        let message = duplicate_message(
            Locale::English,
            "The best music ever",
            &items,
            &["spotify:track:honey".into()],
        );

        assert_eq!(
            message,
            "“Honey” is already in “The best music ever”. Add them anyway?"
        );
    }
}
