//! Window layout: panels, overlays, keyboard shortcuts.

use std::sync::Arc;

pub mod artist;
pub mod collection;
pub(crate) mod devices;
mod dialogs;
pub mod home;
pub(crate) mod keys;
pub mod library;
pub mod local;
pub mod login;
mod lyrics;
mod mini;
pub mod player_bar;
pub mod queue;
pub mod radio;
pub mod search;
pub mod settings;
pub mod show;
pub mod sidebar;
pub mod topbar;
pub mod tour;
mod update;
pub mod recap;
mod views_panel;
pub mod widgets;
pub mod calm;
pub mod extra_window;

use egui::{Align2, Color32, Context, CornerRadius, Frame, Id, Margin, Rect, Stroke, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::backend::AuthStatus;
use crate::model::{Action, Loadable, Page, ToastKind};
use crate::theme::{self, Icon};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let ctx = &ctx;
    keys::handle(app, ctx);
    let signed_in = app.is_connected() && app.user.is_some();
    let connecting = matches!(app.auth, AuthStatus::Connecting | AuthStatus::Starting)
        || (app.is_connected() && app.user.is_none());
    // Someone was signed in last time: open the window at once and let the
    // sign-in finish behind it, rather than waiting on a login screen.
    let optimistic = !signed_in
        && app.optimistic_user.is_some()
        && matches!(
            app.auth,
            AuthStatus::Starting | AuthStatus::Connecting | AuthStatus::Connected { .. }
        );
    if !signed_in && !optimistic && !app.guest {
        player_bar::end_tint_session(ctx);
        login::show(app, ui, connecting);
        update::show(app, ctx);
        toasts(app, ctx, 20.0);
        window_controls(ui, &app.palette, app.locale);
        window_resize(ui);
        return;
    }
    if titlebar_spans_window(cfg!(target_os = "macos"), crate::window::custom_titlebar()) {
        titlebar_drag(
            ui,
            Rect::from_min_size(
                ui.max_rect().min,
                vec2(
                    ui.max_rect().width(),
                    theme::TOP_BAR_HEIGHT + theme::titlebar_inset(ui.ctx()),
                ),
            ),
        );
    }
    // F: the visualizer gets the whole window. Nothing else is drawn — no
    // sidebar, no panels, no player bar — so the one thing on screen is
    // the thing that moves with the music. Pressing it again brings the
    // window back exactly as it was.
    app.sync_mini_level(ctx);
    // The extra visualizer window, if one is open, in whatever view this is.
    extra_window::show(app, ctx);
    // The mini player window, if it is open, in whatever view this is.
    mini::window(app, ctx);
    // Calm mode: the whole window is one slow, quiet picture.
    if app.calm_mode {
        calm::show(app, ui);
        keys::handle(app, ctx);
        views_panel::show(app, ctx);
        dialogs::show(app, ctx);
        toasts(app, ctx, 20.0);
        window_controls(ui, &app.palette, app.locale);
        window_resize(ui);
        return;
    }
    if app.fullscreen_vis {
        player_bar::show(app, ui);
        lyrics::vis_overlay(app, ui);
        keys::handle(app, ctx);
        views_panel::show(app, ctx);
        dialogs::show(app, ctx);
        toasts(app, ctx, 20.0);
        window_controls(ui, &app.palette, app.locale);
        window_resize(ui);
        return;
    }
    // The mini player is a window of its own (see `mini::window`), so the
    // main window is never the mini player.
    app.mini_active = false;
    app.sync_mini_level(ctx);
    app.window_alpha = 1.0;
    // The bass-jump key: a short flash of the accent over the whole window.
    let jump = app.jump_level();
    if jump > 0.0 {
        let screen = ctx.content_rect();
        // The same two colours the bars and the flow run between.
        let (low, high) = player_bar::vis_gradient(app, ui, crate::settings::PlayerBarVis::Flow);
        let fade = 0.28 * jump;
        let (top, bottom) = (high.gamma_multiply(fade), low.gamma_multiply(fade));
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(screen.left_top(), top);
        mesh.colored_vertex(screen.right_top(), top);
        mesh.colored_vertex(screen.right_bottom(), bottom);
        mesh.colored_vertex(screen.left_bottom(), bottom);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("bass-jump")))
            .add(egui::Shape::mesh(mesh));
        ctx.request_repaint();
    }
    player_bar::show(app, ui);
    if app.lyrics_fullscreen.is_some() {
        lyrics::fullscreen(app, ui);
    } else {
        if app.settings.sidebar_visible {
            sidebar::show(app, ui);
        }
        if app.show_queue_panel {
            queue::side_panel(app, ui);
        }
        if app.show_lyrics_panel {
            lyrics::side_panel(app, ui);
        }
        // When the window is too narrow for the page beside the panels, they
        // close one by one: the side lyrics first, then the queue, then the
        // library, so the page itself is never squeezed away.
        let page_w = ui.available_width();
        let least = topbar::least_width(ctx).min(560.0);
        if page_w < least - 8.0 && ctx.content_rect().width() > 300.0 {
            if app.show_lyrics_panel {
                app.show_lyrics_panel = false;
            } else if app.show_queue_panel {
                app.show_queue_panel = false;
            } else if app.settings.sidebar_visible {
                app.settings.sidebar_visible = false;
                app.mark_settings_dirty();
            }
            ctx.request_repaint();
        }
        central(app, ui);
        keep_room_for_panels(app, ctx);
    }
    devices::popup(app, ctx);
    dialogs::show(app, ctx);
    views_panel::show(app, ctx);
    recap::show(app, ctx);
    update::show(app, ctx);
    widgets::drag_ghost(ctx, &app.palette, app.locale);
    toasts(app, ctx, theme::PLAYER_BAR_HEIGHT + 16.0);
    hint_banner(app, ctx);
    guest_banner(app, ctx);
    tour::show(app, ctx);
    window_controls(ui, &app.palette, app.locale);
    window_resize(ui);
}

/// The main window's narrowest width with these panels open: their least
/// widths and the page's.
fn main_min_width(page: f32, sidebar: bool, right_panel: bool) -> f32 {
    let sidebar = if sidebar { SIDEBAR_MIN_WIDTH } else { 0.0 };
    let right = if right_panel {
        theme::SIDE_PANEL_MIN_WIDTH
    } else {
        0.0
    };
    (sidebar + right + page).max(crate::window::MAIN_MIN_SIZE[0])
}

/// The sidebar's narrowest width.
pub(crate) const SIDEBAR_MIN_WIDTH: f32 = 210.0;

/// Raise the window's minimum width while the Queue or Lyrics panel is open,
/// so even at its narrowest the page beside the panels keeps the room its
/// top bar needs (#624), and lower it again once they close. Window managers
/// that tile ignore the minimum; the panels still give way there.
fn keep_room_for_panels(app: &App, ctx: &Context) {
    if crate::window::fixed_size() {
        return;
    }
    let width = main_min_width(
        topbar::least_width(ctx),
        app.settings.sidebar_visible,
        app.show_queue_panel || app.show_lyrics_panel,
    )
    .round();
    let id = Id::new("main-min-width");
    let sent = ctx.data(|data| data.get_temp::<f32>(id));
    // The window starts at MAIN_MIN_SIZE, so there is nothing to send until
    // the panels need more.
    if sent.unwrap_or(crate::window::MAIN_MIN_SIZE[0]) != width {
        ctx.data_mut(|data| data.insert_temp(id, width));
        // Back to the small floor (the mini player lives down there) once
        // the panels no longer need the room.
        let min = if width <= crate::window::MAIN_MIN_SIZE[0] {
            vec2(crate::window::MAIN_MIN_SIZE[0], crate::window::MAIN_MIN_SIZE[1])
        } else {
            vec2(width, crate::window::MAIN_MIN_SIZE[1])
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::MinInnerSize(min));
    }
}

/// How a resizable side panel fits beside the page.
pub(crate) struct PanelFit {
    /// The widths the panel may take this frame.
    pub range: std::ops::RangeInclusive<f32>,
    /// Whether the room the page needs holds the panel under the width the
    /// person chose for it.
    pub yielding: bool,
}

/// Fit a side panel between `range` and the page beside it. The panel gives
/// up width, down to its minimum, so the page keeps `room_for_page`, the
/// least its top bar needs (#624). The width chosen for the panel stays in
/// the settings and comes back once the window widens again.
///
/// `room` is what the panel and everything after it share.
pub(crate) fn yielding_panel(
    ctx: &Context,
    id: &str,
    range: std::ops::RangeInclusive<f32>,
    chosen: f32,
    room: f32,
) -> PanelFit {
    let (min, max) = range.into_inner();
    let max = max.min(room).max(min);
    let width = chosen.clamp(min, max);
    // egui remembers the width the panel last drew at, a clamped one too.
    // Hand it back the chosen width whenever there is room for more.
    let id = Id::new(id);
    if let Some(mut state) = egui::containers::panel::PanelState::load(ctx, id)
        && state.outer_rect.width() + 0.5 < width
    {
        state.outer_rect.set_width(width);
        ctx.data_mut(|data| data.insert_persisted(id, state));
    }
    PanelFit {
        range: min..=max,
        yielding: max < chosen - 0.5,
    }
}

/// Whether a panel's new width is one the person chose: it moved while not
/// held back by the page, or by their own drag.
pub(crate) fn panel_width_chosen(ctx: &Context, id: &str, fit: &PanelFit) -> bool {
    !fit.yielding || ctx.is_being_dragged(Id::new(id).with("__resize"))
}

/// Spotify artwork width used by the library grid and its page preview.
const GRID_ART_TARGET_WIDTH: u32 = 300;

/// Keeps the most recent loading preview of each metadata type available to
/// the loaded hero as an artwork fallback. The fixed typed slot bounds this to
/// one playlist, album, artist, and show instead of scanning known metadata on
/// every loaded frame.
fn loading_preview<T>(
    ctx: &Context,
    key: &str,
    details: &Loadable<T>,
    known: impl FnOnce() -> Option<T>,
) -> Option<Arc<T>>
where
    T: Send + Sync + 'static,
{
    let memory_id = Id::new("collection-loading-preview");
    let key = Id::new(key);
    if details.get().is_some() {
        ctx.data(|data| data.get_temp::<(Id, Arc<T>)>(memory_id))
            .filter(|(stored_key, _)| *stored_key == key)
            .map(|(_, preview)| preview)
    } else {
        let preview = known().map(Arc::new);
        ctx.data_mut(|data| match &preview {
            Some(preview) => {
                data.insert_temp(memory_id, (key, Arc::clone(preview)));
            }
            None => data.remove::<(Id, Arc<T>)>(memory_id),
        });
        preview
    }
}

fn page_tint(app: &mut App) -> Option<Color32> {
    let page = app.page().clone();
    let image = match &page {
        Page::Playlist(id) => app
            .playlist_pages
            .get(id)
            .and_then(|page| page.playlist.get())
            .or_else(|| app.known_playlist(id))
            .and_then(|playlist| pick_image(&playlist.images, 64))
            .map(str::to_string),
        Page::Album(id) => app
            .album_pages
            .get(id)
            .and_then(|page| page.album.get())
            .or_else(|| app.known_album(id))
            .and_then(|album| pick_image(&album.images, 64))
            .map(str::to_string),
        Page::Artist(id) => app
            .artist_pages
            .get(id)
            .and_then(|page| page.artist.get())
            .or_else(|| app.known_artist(id))
            .and_then(|artist| pick_image(&artist.images, 64))
            .map(str::to_string),
        Page::Show(id) => app
            .show_pages
            .get(id)
            .and_then(|page| page.show.get())
            .or_else(|| app.known_show(id))
            .and_then(|show| pick_image(&show.images, 64))
            .map(str::to_string),
        Page::Radio(seed) => pick_image(&app.radio_images(seed), 64).map(str::to_string),
        Page::LikedSongs => return Some(Color32::from_rgb(0x50, 0x38, 0xc8)),
        _ => None,
    };
    // A playlist or album page always shows its own cover softly behind the
    // header, whether or not the accent follows the song's cover.
    let has_header_art = header_art(app).is_some();
    if !app.settings.accent_from_art && image.is_some() && !has_header_art {
        return None;
    }
    match image {
        Some(url) => app
            .tint_for(Some(&url))
            .or_else(|| app.now_playing_tint())
            .or_else(|| has_header_art.then_some(app.palette.accent)),
        None => app.now_playing_tint(),
    }
}

/// The cover of the playlist or album page being shown, for the soft
/// picture behind its header.
fn header_art(app: &App) -> Option<String> {
    match app.page() {
        Page::Playlist(id) => app
            .playlist_pages
            .get(id)
            .and_then(|page| page.playlist.get())
            .or_else(|| app.known_playlist(id))
            .and_then(|playlist| pick_image(&playlist.images, 64))
            .map(str::to_string),
        Page::Album(id) => app
            .album_pages
            .get(id)
            .and_then(|page| page.album.get())
            .or_else(|| app.known_album(id))
            .and_then(|album| pick_image(&album.images, 64))
            .map(str::to_string),
        _ => None,
    }
}

fn central(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let tint = page_tint(app);
    egui::CentralPanel::default()
        .frame(Frame::new().fill(palette.window))
        .show(ui, |ui| {
            let rect = ui.max_rect();
            if !app.is_connected() {
                // Still signing in behind the window.
                ui.add_space(rect.height() * 0.4);
                ui.vertical_centered(|ui| {
                    theme::spinner(ui, 22.0, palette.accent);
                    ui.add_space(10.0);
                    theme::text(ui, "Connecting to Spotify…", theme::medium(15.0), palette.secondary);
                });
                return;
            }
            if let Some(tint) = tint {
                let strength = if matches!(
                    app.page(),
                    Page::Home | Page::Search | Page::Settings | Page::Queue
                ) {
                    0.45
                } else {
                    0.85
                };
                let top = blend(palette.window, tint, strength);
                let header = Rect::from_min_size(rect.min, vec2(rect.width(), 340.0));
                widgets::paint_vertical_gradient(ui, header, top, palette.window);
                // The cover again, blown up and soft, behind the header only:
                // it fades out before the song list so nothing in the list
                // sits on it.
                if let Some(url) = header_art(app) {
                    // The app's own softened (blurred) cover first: the egui
                    // image loader may never be asked for these addresses.
                    let art = app.backend.art().clone();
                    let image = egui::Image::new(url.as_str()).show_loading_spinner(false);
                    let soft = app.softened_covers.texture(ui.ctx(), &art, &url);
                    let loaded = match soft {
                        Some(handle) => Some(egui::load::SizedTexture::from_handle(&handle)),
                        None => {
                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
                            match image.load_for_size(ui.ctx(), vec2(64.0, 64.0)) {
                                Ok(egui::load::TexturePoll::Ready { texture }) => Some(texture),
                                _ => None,
                            }
                        }
                    };
                    if let Some(texture) = loaded {
                        // The cover keeps its own proportions: only the strip
                        // that fits the header is taken, never stretched.
                        let aspect = (texture.size.x / texture.size.y.max(1.0)).max(0.1);
                        let band = (header.height() / header.width() * aspect).clamp(0.05, 1.0);
                        let uv = Rect::from_min_max(
                            egui::pos2(0.0, 0.5 - band / 2.0),
                            egui::pos2(1.0, 0.5 + band / 2.0),
                        );
                        egui::Image::new(texture)
                            .uv(uv)
                            .tint(Color32::WHITE.gamma_multiply(if palette.dark { 0.38 } else { 0.5 }))
                            .paint_at(ui, header);
                        let fade = Rect::from_min_max(
                            egui::pos2(header.left(), header.bottom() - 150.0),
                            header.right_bottom(),
                        );
                        widgets::paint_vertical_gradient(ui, fade, Color32::TRANSPARENT, palette.window);
                    }
                }
            }
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            // egui fades a scrolled page's edge into the panel's plain
            // colour, which shows as a pale band over a cover's tint; the
            // page casts a shadow under the header instead.
            ui.spacing_mut().scroll.fade.strength = 0.0;
            // The page scrolls under the bar: only the search, the buttons and
            // the profile have backgrounds, drawn over it afterwards.
            let bar_height = topbar::height(app, ui);
            let bar_rect = Rect::from_min_size(
                ui.cursor().min,
                vec2(ui.available_width(), bar_height),
            );
            let page = app.page().clone();
            let mut area = egui::ScrollArea::vertical()
                .id_salt(("page", page.encode()))
                .auto_shrink([false, false]);
            if std::mem::take(&mut app.scroll_top) {
                area = area.vertical_scroll_offset(0.0);
            }
            let scroll = crate::autoscroll::show(
                ui,
                area,
                egui::Vec2b::new(false, true),
                |ui| {
                    Frame::new()
                        .inner_margin(Margin {
                            left: widgets::PAGE_PADDING as i8,
                            right: widgets::PAGE_PADDING as i8,
                            top: (4.0 + bar_height).min(120.0) as i8,
                            bottom: 48,
                        })
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            match page {
                                Page::Home => home::show(app, ui),
                                Page::TopSongs => collection::top_songs(app, ui),
                                Page::Search => search::show(app, ui),
                                Page::LikedSongs => collection::liked(app, ui),
                                Page::Albums | Page::Artists | Page::Podcasts | Page::Episodes => {
                                    library::show(app, ui, page)
                                }
                                Page::Playlist(id) => collection::playlist(app, ui, &id),
                                Page::Album(id) => collection::album(app, ui, &id),
                                Page::Artist(id) => artist::show(app, ui, &id),
                                Page::Show(id) => show::show(app, ui, &id),
                                Page::Radio(seed) => radio::radio(app, ui, &seed),
                                Page::Queue => queue::page(app, ui),
                                Page::Settings => settings::show(app, ui),
                                Page::Local => local::show(app, ui),
                            }
                        });
                },
            );
            let mut bar = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(bar_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            topbar::show(app, &mut bar);
        });
}

/// The shadow the header casts on a page scrolled under it, deepening over
/// the first few points of scrolling. Dark in both themes, lighter over a
/// light page.
fn header_shadow(ui: &egui::Ui, page: Rect, scrolled: f32, dark: bool) {
    let depth = (scrolled / 24.0).clamp(0.0, 1.0);
    if depth <= 0.0 {
        return;
    }
    let strength = if dark { 110.0 } else { 36.0 };
    let rect = Rect::from_min_size(page.min, vec2(page.width(), 14.0));
    widgets::paint_vertical_gradient(
        ui,
        rect,
        egui::Color32::from_black_alpha((strength * depth) as u8),
        egui::Color32::TRANSPARENT,
    );
}

/// Makes `rect` drag the borderless window. Register it before child widgets so
/// they keep their clicks.
pub fn titlebar_drag(ui: &mut egui::Ui, rect: egui::Rect) {
    let fullscreen = ui
        .ctx()
        .input(|input| input.viewport().fullscreen.unwrap_or(false));
    if !cfg!(any(target_os = "macos", windows)) || fullscreen {
        return;
    }
    let response = ui.interact(
        rect,
        ui.id().with("titlebar-drag"),
        egui::Sense::click_and_drag(),
    );
    // Only the maximize button maximizes: a double-click here does nothing.
    if (crate::window::custom_titlebar() && response.drag_started())
        || (cfg!(target_os = "macos")
            && response.is_pointer_button_down_on()
            && ui.input(|input| input.pointer.primary_pressed())
            && crate::window::macos_titlebar_should_drag())
    {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// Without a native title bar, the top of every panel drags the window, not
/// only the page's top bar.
const fn titlebar_spans_window(on_macos: bool, custom_titlebar: bool) -> bool {
    on_macos || custom_titlebar
}

const WINDOW_RESIZE_BORDER: f32 = 5.0;
const WINDOW_RESIZE_CORNER: f32 = 10.0;
const WINDOWS_WINDOW_CONTROLS_WIDTH: f32 = 3.0 * 36.0 + 8.0 + WINDOW_RESIZE_BORDER;
const WINDOWS_WINDOW_CONTROLS_HEIGHT: f32 = 36.0 + WINDOW_RESIZE_BORDER;
// The 760-point minimum with the default 250-point sidebar leaves 510 points.
const WINDOWS_MIN_INLINE_TOPBAR_WIDTH: f32 = 510.0 + WINDOWS_WINDOW_CONTROLS_WIDTH;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct WindowControlsReservation {
    pub topbar_width: f32,
    pub topbar_top: f32,
    pub queue_top: f32,
    pub lyrics_top: f32,
}

const fn windows_chrome_visible(on_windows: bool, fullscreen: bool) -> bool {
    on_windows && !fullscreen
}

fn windows_chrome_visible_here(ctx: &egui::Context) -> bool {
    let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
    windows_chrome_visible(crate::window::custom_titlebar(), fullscreen)
}

const fn windows_controls_reservation(
    on_windows: bool,
    fullscreen: bool,
    queue: bool,
    lyrics: bool,
    topbar_width: f32,
) -> WindowControlsReservation {
    let mut space = WindowControlsReservation {
        topbar_width: 0.0,
        topbar_top: 0.0,
        queue_top: 0.0,
        lyrics_top: 0.0,
    };
    if windows_chrome_visible(on_windows, fullscreen) {
        if queue {
            space.queue_top = WINDOWS_WINDOW_CONTROLS_HEIGHT;
        } else if lyrics {
            space.lyrics_top = WINDOWS_WINDOW_CONTROLS_HEIGHT;
        } else if topbar_width < WINDOWS_MIN_INLINE_TOPBAR_WIDTH {
            space.topbar_top = WINDOWS_WINDOW_CONTROLS_HEIGHT;
        } else {
            space.topbar_width = WINDOWS_WINDOW_CONTROLS_WIDTH;
        }
    }
    space
}

pub(super) fn window_controls_reservation(
    ctx: &egui::Context,
    queue: bool,
    lyrics: bool,
    topbar_width: f32,
) -> WindowControlsReservation {
    let fullscreen = ctx.input(|input| input.viewport().fullscreen.unwrap_or(false));
    windows_controls_reservation(
        crate::window::custom_titlebar(),
        fullscreen,
        queue,
        lyrics,
        topbar_width,
    )
}

/// Draws the Windows caption controls over the outermost top-right header.
pub fn window_controls(ui: &mut egui::Ui, palette: &theme::Palette, locale: crate::i18n::Locale) {
    use crate::i18n::gettext;
    if !windows_chrome_visible_here(ui.ctx()) {
        return;
    }
    let maximized = ui
        .ctx()
        .input(|input| input.viewport().maximized.unwrap_or(false));
    egui::Area::new(egui::Id::new("windows-window-controls"))
        .anchor(
            Align2::RIGHT_TOP,
            vec2(-WINDOW_RESIZE_BORDER, WINDOW_RESIZE_BORDER),
        )
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.horizontal(|ui| {
                for (icon, tooltip, command) in [
                    (
                        Icon::Minus,
                        gettext(locale, "Minimize"),
                        egui::ViewportCommand::Minimized(true),
                    ),
                    (
                        if maximized { Icon::Copy } else { Icon::Square },
                        if maximized {
                            gettext(locale, "Restore")
                        } else {
                            gettext(locale, "Maximize")
                        },
                        egui::ViewportCommand::Maximized(!maximized),
                    ),
                    (
                        Icon::X,
                        gettext(locale, "Close"),
                        egui::ViewportCommand::Close,
                    ),
                ] {
                    let image = icon
                        .image(palette.secondary, 14.0)
                        .alt_text(tooltip.as_ref());
                    // Each control has its own small solid background, not a bar.
                    let button = egui::Button::image(image)
                        .fill(palette.surface)
                        .corner_radius(egui::CornerRadius::same(10));
                    if ui
                        .add_sized(egui::Vec2::splat(36.0), button)
                        .on_hover_text(tooltip.as_ref())
                        .clicked()
                    {
                        ui.ctx().send_viewport_cmd(command);
                    }
                }
            });
        });
}

pub(super) fn window_resize(ui: &mut egui::Ui) {
    let (fullscreen, maximized) = ui.ctx().input(|input| {
        (
            input.viewport().fullscreen.unwrap_or(false),
            input.viewport().maximized.unwrap_or(false),
        )
    });
    if !window_resize_enabled(crate::window::custom_titlebar(), fullscreen, maximized) {
        return;
    }

    let Some(position) = ui.input(|input| input.pointer.hover_pos()) else {
        return;
    };
    let Some(direction) = resize_direction(ui.ctx().content_rect(), position) else {
        return;
    };
    ui.ctx().set_cursor_icon(direction.1);
    if ui.input(|input| input.pointer.primary_pressed()) {
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction.0));
    }
}

const fn window_resize_enabled(on_windows: bool, fullscreen: bool, maximized: bool) -> bool {
    on_windows && !fullscreen && !maximized
}

fn resize_direction(
    window: Rect,
    position: egui::Pos2,
) -> Option<(egui::ResizeDirection, egui::CursorIcon)> {
    if !window.contains(position) {
        return None;
    }
    let [left, right, top, bottom] = [
        position.x - window.left(),
        window.right() - position.x,
        position.y - window.top(),
        window.bottom() - position.y,
    ];
    let mut x = i8::from(right <= WINDOW_RESIZE_BORDER) - i8::from(left <= WINDOW_RESIZE_BORDER);
    let mut y = i8::from(bottom <= WINDOW_RESIZE_BORDER) - i8::from(top <= WINDOW_RESIZE_BORDER);
    if y != 0 {
        x = i8::from(right <= WINDOW_RESIZE_CORNER) - i8::from(left <= WINDOW_RESIZE_CORNER);
    }
    if x != 0 {
        y = i8::from(bottom <= WINDOW_RESIZE_CORNER) - i8::from(top <= WINDOW_RESIZE_CORNER);
    }
    use egui::{CursorIcon as C, ResizeDirection as D};
    match (x, y) {
        (-1, -1) => Some((D::NorthWest, C::ResizeNwSe)),
        (1, -1) => Some((D::NorthEast, C::ResizeNeSw)),
        (-1, 1) => Some((D::SouthWest, C::ResizeNeSw)),
        (1, 1) => Some((D::SouthEast, C::ResizeNwSe)),
        (-1, 0) => Some((D::West, C::ResizeHorizontal)),
        (1, 0) => Some((D::East, C::ResizeHorizontal)),
        (0, -1) => Some((D::North, C::ResizeVertical)),
        (0, 1) => Some((D::South, C::ResizeVertical)),
        _ => None,
    }
}

pub fn blend(base: Color32, tint: Color32, amount: f32) -> Color32 {
    let a = egui::Rgba::from(base);
    let b = egui::Rgba::from(tint);
    let mixed = a * (1.0 - amount) + b * amount;
    let mut color = Color32::from(mixed);
    color[3] = 255;
    color
}

/// A short line at the top on every launch: where the tutorial is. It shows
/// for five seconds the first time and three after that.
/// While the app is open without Spotify: a small note, and the way in.
fn guest_banner(app: &mut App, ctx: &egui::Context) {
    if !app.guest || app.is_connected() {
        return;
    }
    let palette = app.palette;
    let mut sign_in = false;
    egui::Area::new(egui::Id::new("guest-banner"))
        .anchor(Align2::RIGHT_TOP, vec2(-16.0, theme::TOP_BAR_HEIGHT + 8.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            Frame::new()
                .fill(palette.overlay)
                .stroke(Stroke::new(1.0, palette.outline))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        theme::text(ui, "Not signed in to Spotify", theme::regular(12.5), palette.secondary);
                        if theme::link(ui, "Sign in", theme::medium(12.5), palette.accent).clicked() {
                            sign_in = true;
                        }
                    });
                });
        });
    if sign_in {
        app.actions.push(Action::SignIn);
    }
}

fn hint_banner(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|input| input.time);
    let first = !app.settings.hint_seen;
    let show_for = if first { 5.0 } else { 3.0 };
    if now > show_for {
        if first {
            app.settings.hint_seen = true;
            app.actions.push(Action::SettingsChanged);
        }
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
    let alpha = (((show_for - now) / 0.5).clamp(0.0, 1.0).min((now / 0.3).clamp(0.0, 1.0))) as f32;
    let palette = app.palette;
    let label = format!(
        "push {} for the keyboard shortcuts",
        keys::chord_label(app, "tutorial").unwrap_or_else(|| "ctrl + /".to_string()).to_lowercase(),
    );
    egui::Area::new(egui::Id::new("hotkey-hint"))
        .anchor(Align2::CENTER_TOP, vec2(0.0, 10.0))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            ui.set_opacity(alpha);
            Frame::new()
                .fill(palette.overlay)
                .stroke(Stroke::new(1.0, palette.outline))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    theme::text(ui, &label, theme::medium(13.5), palette.text);
                });
        });
}

fn undo_bar(app: &mut App, ctx: &egui::Context, bottom_offset: f32) {
    let Some(undo) = &app.undo_removal else {
        return;
    };
    if undo.created.elapsed().as_secs_f32() > 12.0 {
        app.undo_removal = None;
        return;
    }
    ctx.request_repaint_after(std::time::Duration::from_millis(500));
    let palette = app.palette;
    let count = undo.items.len();
    let mut clicked = false;
    egui::Area::new(egui::Id::new("undo-bar"))
        .anchor(Align2::RIGHT_BOTTOM, vec2(-20.0, -(bottom_offset + 56.0)))
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            Frame::new()
                .fill(palette.overlay)
                .stroke(Stroke::new(1.0, palette.outline))
                .corner_radius(CornerRadius::same(theme::RADIUS))
                .inner_margin(Margin::symmetric(14, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let text = if count == 1 {
                            "Removed 1 song".to_string()
                        } else {
                            format!("Removed {count} songs")
                        };
                        theme::text(ui, &text, theme::medium(13.5), palette.text);
                        if theme::pill_button(ui, &palette, "Undo", true).clicked() {
                            clicked = true;
                        }
                    });
                });
        });
    if clicked {
        app.actions.push(Action::UndoRemoval);
    }
}

fn toasts(app: &mut App, ctx: &egui::Context, bottom_offset: f32) {
    undo_bar(app, ctx, bottom_offset);
    if app.toasts.is_empty() {
        return;
    }
    let palette = app.palette;
    egui::Area::new(egui::Id::new("toasts"))
        .anchor(Align2::RIGHT_BOTTOM, vec2(-20.0, -bottom_offset))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 8.0;
            for toast in &app.toasts {
                let age = toast.created.elapsed().as_secs_f32();
                let alpha = if age < 0.15 {
                    age / 0.15
                } else if age > 2.8 {
                    ((3.2 - age) / 0.4).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                ui.set_opacity(alpha);
                Frame::new()
                    .fill(palette.overlay)
                    .stroke(Stroke::new(1.0, palette.outline))
                    .corner_radius(CornerRadius::same(theme::RADIUS))
                    .inner_margin(Margin::symmetric(14, 10))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 16,
                        spread: 0,
                        color: palette.shadow,
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (icon, color) = match toast.kind {
                                ToastKind::Info => (Icon::CircleCheck, palette.accent),
                                ToastKind::Error => (Icon::CircleAlert, palette.danger),
                            };
                            theme::icon(ui, icon, 16.0, color);
                            // Laid out at its own width. The area
                            // remembers its size, so after a short toast a
                            // label left to wrap at the area's width broke
                            // long messages on every word.
                            let galley = ui.painter().layout(
                                toast.message.clone(),
                                theme::medium(13.5),
                                palette.text,
                                280.0,
                            );
                            ui.add(egui::Label::new(galley));
                        });
                    });
            }
        });
}

#[cfg(test)]
mod window_chrome_tests {
    use super::*;

    #[test]
    fn chrome_visibility_matches_window_state() {
        assert!(windows_chrome_visible(true, false));
        assert!(!windows_chrome_visible(true, true));
        assert!(!windows_chrome_visible(false, false));
        assert!(window_resize_enabled(true, false, false));
        assert!(!window_resize_enabled(true, false, true));
        assert!(!window_resize_enabled(true, true, false));
        assert!(!window_resize_enabled(false, false, false));
    }

    #[test]
    fn the_side_panels_drag_a_window_without_a_native_title_bar() {
        assert!(titlebar_spans_window(true, false));
        assert!(titlebar_spans_window(false, true));
        assert!(!titlebar_spans_window(false, false));
    }

    #[test]
    fn caption_space_belongs_to_the_outermost_header() {
        let values = |queue, lyrics| {
            let space = windows_controls_reservation(true, false, queue, lyrics, f32::INFINITY);
            [
                space.topbar_width,
                space.topbar_top,
                space.queue_top,
                space.lyrics_top,
            ]
        };
        assert_eq!(
            values(false, false),
            [WINDOWS_WINDOW_CONTROLS_WIDTH, 0.0, 0.0, 0.0]
        );
        assert_eq!(
            values(true, false),
            [0.0, 0.0, WINDOWS_WINDOW_CONTROLS_HEIGHT, 0.0]
        );
        assert_eq!(
            values(false, true),
            [0.0, 0.0, 0.0, WINDOWS_WINDOW_CONTROLS_HEIGHT]
        );
        assert_eq!(
            values(true, true),
            [0.0, 0.0, WINDOWS_WINDOW_CONTROLS_HEIGHT, 0.0]
        );
        assert_eq!(
            windows_controls_reservation(true, true, true, true, f32::INFINITY),
            WindowControlsReservation {
                topbar_width: 0.0,
                topbar_top: 0.0,
                queue_top: 0.0,
                lyrics_top: 0.0,
            }
        );
    }

    #[test]
    fn minimum_windows_window_stacks_caption_space_above_the_topbar() {
        let available = 760.0 - 250.0;
        let space = windows_controls_reservation(true, false, false, false, available);
        assert_eq!(space.topbar_width, 0.0);
        assert_eq!(space.topbar_top, WINDOWS_WINDOW_CONTROLS_HEIGHT);

        let inline = windows_controls_reservation(
            true,
            false,
            false,
            false,
            WINDOWS_MIN_INLINE_TOPBAR_WIDTH,
        );
        assert_eq!(inline.topbar_width, WINDOWS_WINDOW_CONTROLS_WIDTH);
        assert_eq!(inline.topbar_top, 0.0);
    }

    #[test]
    fn resize_hit_test_covers_edges_and_corners() {
        use egui::ResizeDirection as D;

        let window = Rect::from_min_max(egui::pos2(20.0, 30.0), egui::pos2(120.0, 110.0));
        for (position, expected) in [
            (egui::pos2(21.0, 31.0), Some(D::NorthWest)),
            (egui::pos2(70.0, 31.0), Some(D::North)),
            (egui::pos2(119.0, 31.0), Some(D::NorthEast)),
            (egui::pos2(21.0, 70.0), Some(D::West)),
            (egui::pos2(70.0, 70.0), None),
            (egui::pos2(119.0, 70.0), Some(D::East)),
            (egui::pos2(21.0, 109.0), Some(D::SouthWest)),
            (egui::pos2(70.0, 109.0), Some(D::South)),
            (egui::pos2(119.0, 109.0), Some(D::SouthEast)),
        ] {
            assert_eq!(
                resize_direction(window, position).map(|hit| hit.0),
                expected
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header casts a shadow only on a page scrolled under it, and it
    /// is black in both themes, never the page's own colour.
    #[test]
    fn the_header_shadow_appears_once_the_page_scrolls() {
        let ctx = egui::Context::default();
        let page = Rect::from_min_size(egui::pos2(0.0, 80.0), vec2(800.0, 600.0));
        let shadows = |scrolled: f32, dark: bool| {
            let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                header_shadow(ui, page, scrolled, dark);
            });
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) => Some(mesh.vertices.clone()),
                    _ => None,
                })
                .flatten()
                .collect::<Vec<_>>()
        };
        assert!(
            shadows(0.0, true).is_empty(),
            "nothing at the top of the page"
        );
        for dark in [true, false] {
            let vertices = shadows(40.0, dark);
            let top = vertices
                .iter()
                .find(|vertex| vertex.pos.y == page.top())
                .expect("a shadow along the page's top edge");
            assert!(top.color.a() > 0);
            assert_eq!((top.color.r(), top.color.g(), top.color.b()), (0, 0, 0));
        }
    }
}
