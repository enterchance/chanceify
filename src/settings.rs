//! User preferences, stored as one readable JSON file.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Local Library identity only. Never sent to Spotify as a context URI.
pub const LIKED_SONGS_KEY: &str = "chanceify:liked-songs";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryShelf {
    #[default]
    Playlists,
    Albums,
    Artists,
    Podcasts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySort {
    Library,
    RecentlyPlayed,
    Name,
    RecentlyAdded,
    Local,
    Spotify,
}

impl LibrarySort {
    pub fn supports(self, shelf: LibraryShelf) -> bool {
        match self {
            Self::RecentlyPlayed | Self::Name | Self::Library => true,
            Self::RecentlyAdded => matches!(shelf, LibraryShelf::Albums | LibraryShelf::Podcasts),
            Self::Local | Self::Spotify => shelf == LibraryShelf::Playlists,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    Dark,
    Light,
    #[default]
    System,
    Midnight,
    Ocean,
    Sunset,
    Forest,
    Rose,
    Grape,
    Amoled,
    Mocha,
    Cream,
    /// The Night icon's deep indigo with its green and lime.
    Night,
    /// The Transparent icon: the window shows the desktop through it.
    Glass,
    /// A pale pink, the window on #FFF2F9.
    PinkMilk,
    /// Colours picked by hand: `custom_bg` and `custom_accent`.
    Custom,
}

/// How wide each optional column of a track list is.
///
/// One set of widths for every list, so a layout worked out once holds
/// everywhere, and stored so it holds next time too. A width of zero hides
/// the column outright: an empty column is worse than an absent one.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackColumns {
    pub album: f32,
    /// Kept only so an older settings file still deserialises. The column
    /// is gone: nobody reads who added a song to a list they are the owner
    /// of, and its width came straight out of the song name.
    pub added_by: f32,
    pub added: f32,
    /// The tempo column. On by default: it is the one number Spotify will
    /// not give us and a reader has to ask a stranger for, so it has to be
    /// asked for deliberately rather than found by accident.
    pub bpm: f32,
    /// The album's release date. Off until it is ticked.
    pub release: f32,
    /// The playlist icons, as a column of their own (sortable). Off until
    /// it is ticked; while it is off they sit beside the song name.
    pub playlists: f32,
    /// The song's genres, from MusicBrainz (never Spotify). Off until ticked.
    pub genre: f32,
    /// The song-count (#) column is switched off.
    pub hide_number: bool,
    /// The length column is switched off.
    pub hide_duration: bool,
    /// The heart (liked) button is switched off.
    pub hide_heart: bool,
    /// The "+" (add to a playlist) button is switched off.
    pub hide_plus: bool,
    /// The artist's name under the song name is switched off.
    pub hide_artist: bool,
    /// The table keeps to the left edge, as in Spotify. Off (the default),
    /// it sits in the middle and grows out from there.
    pub lock_left: bool,
    /// Songs as a grid of covers, whatever columns are on.
    pub grid: bool,
    /// Where each column sits, left to right: 0 title, 1 playlists, 2 album,
    /// 3 date added, 4 release date, 5 tempo. The title takes whatever room
    /// the others leave, wherever it is.
    pub order: [u8; 6],
    /// The columns share the width of the list between them instead of
    /// keeping the widths they were dragged to.
    pub spread: bool,
}

impl Default for TrackColumns {
    fn default() -> Self {
        Self {
            album: 200.0,
            added_by: 0.0,
            added: 120.0,
            bpm: 72.0,
            release: 0.0,
            playlists: 0.0,
            genre: 0.0,
            hide_number: false,
            hide_duration: false,
            hide_heart: false,
            hide_plus: false,
            hide_artist: false,
            lock_left: false,
            grid: false,
            order: Self::DEFAULT_ORDER,
            spread: false,
        }
    }
}

impl TrackColumns {
    /// Title first, then the others in the order they have always had.
    pub const DEFAULT_ORDER: [u8; 6] = [0, 1, 2, 3, 4, 5];

    /// The column order, always a full set of the six: a hand-edited file
    /// that repeats or drops one gets the standard order back.
    pub fn order(&self) -> [u8; 6] {
        let mut seen = [false; 6];
        for code in self.order {
            match seen.get_mut(usize::from(code)) {
                Some(slot) if !*slot => *slot = true,
                _ => return Self::DEFAULT_ORDER,
            }
        }
        self.order
    }

    /// The code a column has in `order`, if it can be moved.
    pub fn code(column: crate::model::SortColumn) -> Option<u8> {
        use crate::model::SortColumn as Sc;
        Some(match column {
            Sc::Title => 0,
            Sc::Playlists => 1,
            Sc::Album => 2,
            Sc::Added => 3,
            Sc::Release => 4,
            Sc::Bpm => 5,
            _ => return None,
        })
    }

    /// Moves `column` so that it sits `slot` places from the left among the
    /// columns that are showing; the hidden ones keep their relative order
    /// after them.
    pub fn move_column(&mut self, column: crate::model::SortColumn, shown: &[crate::model::SortColumn], slot: usize) {
        let Some(code) = Self::code(column) else {
            return;
        };
        let mut visible: Vec<u8> = shown
            .iter()
            .filter(|c| **c != column)
            .filter_map(|c| Self::code(*c))
            .collect();
        visible.insert(slot.min(visible.len()), code);
        let mut next: Vec<u8> = visible.clone();
        for each in self.order() {
            if !next.contains(&each) {
                next.push(each);
            }
        }
        if let Ok(order) = <[u8; 6]>::try_from(next) {
            self.order = order;
        }
    }

    /// How wide the heart and "+" buttons are together.
    pub fn buttons_width(&self) -> f32 {
        // The "+" goes with the heart: no liked column, no buttons.
        if self.hide_heart {
            0.0
        } else {
            36.0 * (1 + u8::from(!self.hide_plus)) as f32
        }
    }

    /// The narrowest a column may be dragged before it is treated as hidden.
    pub const MIN: f32 = 38.0;
    /// The widest, so one drag cannot swallow the song names.
    pub const MAX: f32 = 640.0;

    /// The width this column starts at, and returns to on a double-click.
    pub fn default_width(column: crate::model::SortColumn) -> f32 {
        match column {
            crate::model::SortColumn::Album => 200.0,
            crate::model::SortColumn::Added => 120.0,
            crate::model::SortColumn::Bpm => 72.0,
            crate::model::SortColumn::Release => 110.0,
            crate::model::SortColumn::Playlists => 150.0,
            crate::model::SortColumn::Genre => 170.0,
            _ => 0.0,
        }
    }

    /// How wide this column is drawn, clamped so a hand-edited file cannot
    /// make a column vanish or swallow the list.
    pub fn width(&self, column: crate::model::SortColumn) -> f32 {
        let raw = match column {
            crate::model::SortColumn::Album => self.album,
            crate::model::SortColumn::Added => self.added,
            crate::model::SortColumn::Bpm => self.bpm,
            crate::model::SortColumn::Release => self.release,
            crate::model::SortColumn::Playlists => self.playlists,
            // Genres are gone: the column can never be shown.
            crate::model::SortColumn::Genre => 0.0,
            _ => 0.0,
        };
        if raw.is_finite() {
            raw.clamp(0.0, Self::MAX)
        } else {
            0.0
        }
    }

    /// Whether this column is shown at all.
    pub fn shown(&self, column: crate::model::SortColumn) -> bool {
        self.width(column) > 0.0
    }

    /// Shows or hides this column, at its default width.
    pub fn toggle(&mut self, column: crate::model::SortColumn) {
        let width = if self.shown(column) {
            0.0
        } else {
            Self::default_width(column)
        };
        self.set(column, width);
    }

    /// Sets this column's width, treating anything under the minimum as a
    /// hide: a column too narrow to read its own heading is not a column.
    pub fn set(&mut self, column: crate::model::SortColumn, width: f32) {
        let width = if width.is_finite() {
            width.clamp(0.0, Self::MAX)
        } else {
            0.0
        };
        let width = if width > 0.0 && width < Self::MIN {
            0.0
        } else {
            width
        };
        match column {
            crate::model::SortColumn::Album => self.album = width,
            crate::model::SortColumn::AddedBy => self.added_by = 0.0,
            crate::model::SortColumn::Added => self.added = width,
            crate::model::SortColumn::Bpm => self.bpm = width,
            crate::model::SortColumn::Release => self.release = width,
            crate::model::SortColumn::Playlists => self.playlists = width,
            crate::model::SortColumn::Genre => self.genre = width,
            _ => {}
        }
    }

    /// Every column back to its starting width.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// How much room every optional column takes together.
    pub fn total(&self) -> f32 {
        use crate::model::SortColumn;
        [SortColumn::Album, SortColumn::Added, SortColumn::Bpm, SortColumn::Release, SortColumn::Playlists, SortColumn::Genre]
            .into_iter()
            .map(|column| self.width(column))
            .sum()
    }
}

/// Whether a Home shelf is drawn. Hidden shelves still refresh normally.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeShelfSettings {
    pub visible: bool,
}

impl Default for HomeShelfSettings {
    fn default() -> Self {
        Self { visible: true }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HomeSettings {
    pub made_for_you: HomeShelfSettings,
    pub recommendations: HomeShelfSettings,
    /// Which tab of Home is open: 0 For you, 1 Your music and stats.
    pub tab: u8,
    /// Top artists and songs: 0 last 30 days, 1 last 7 days, 2 all time.
    pub artist_range: u8,
    pub song_range: u8,
    /// How many recommended songs show: 0 is the usual 20.
    pub recommended_shown: u8,
    /// Whether the full Recently played shelf is open.
    pub recent_all: bool,
    /// Whether the New from your artists list is folded away.
    pub releases_folded: bool,
}

/// What moves behind the player bar's controls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerBarVis {
    #[default]
    Off,
    Spectrum,
    /// Smooth bands of colour that drift and fold into one another over
    /// the album art, in the spirit of a fluid wallpaper rather than a
    /// row of separate bars.
    Flow,
    /// The album art itself, drawn in rings that bend and smear with the
    /// sound: a swirl rather than a reading of it.
    Swirl,
    Waveform,
}

impl PlayerBarVis {
    /// The mode a click on the player bar moves to: spectrum, flow,
    /// swirl, waveform, then off, as Winamp's visualizer cycles.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Spectrum,
            Self::Spectrum => Self::Flow,
            Self::Flow => Self::Swirl,
            Self::Swirl => Self::Waveform,
            Self::Waveform => Self::Off,
        }
    }
}

/// Where the player bar's bars take their colour.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisColor {
    /// The playing cover's own accent, so the bars change with the song.
    #[default]
    AlbumArt,
    /// The theme's accent, so the bars stay put between songs.
    Theme,
    /// One colour of the reader's choosing, written as `#rrggbb`.
    Fixed,
}

impl VisColor {
    pub const ALL: [VisColor; 3] = [VisColor::AlbumArt, VisColor::Theme, VisColor::Fixed];
}

/// Mini-player visualizer mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VisMode {
    #[default]
    Bars,
    Scope,
    Off,
}

impl VisMode {
    /// Next mode in the display's click cycle.
    pub fn next(self) -> Self {
        match self {
            Self::Bars => Self::Scope,
            Self::Scope => Self::Off,
            Self::Off => Self::Bars,
        }
    }
}

/// The interface language: the operating system's, or one chosen in Settings.
///
/// Stored as `"system"` or a locale tag such as `"es"` or `"pt-BR"`. A file
/// without the field follows the system; a tag this version does not carry
/// also follows the system, rather than making the whole file unreadable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanguageChoice {
    #[default]
    System,
    Locale(crate::i18n::Locale),
}

impl LanguageChoice {
    /// The locale the interface is drawn in.
    pub fn resolve(self) -> crate::i18n::Locale {
        match self {
            Self::System => crate::i18n::Locale::from_system(),
            Self::Locale(locale) => locale,
        }
    }
}

impl Serialize for LanguageChoice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::System => "system",
            Self::Locale(locale) => locale.tag(),
        })
    }
}

impl<'de> Deserialize<'de> for LanguageChoice {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(value
            .as_str()
            .and_then(crate::i18n::Locale::from_tag)
            .map_or(Self::System, Self::Locale))
    }
}

/// A colour written as `#RRGGBB` or `RRGGBB`.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

/// The picker's name for a palette file that comes with the app. Files of
/// your own keep their names.
pub fn fancy_theme_name(name: &str) -> String {
    match name.to_ascii_lowercase().replace(['-', '_'], " ").as_str() {
        "catppuccin" => "Cat Cafe",
        "catppuccin latte" => "Latte",
        "nord" => "Arctic",
        "ristretto" => "Espresso",
        "rose pine" => "Pine Rose",
        "rose pine dawn" => "Rosewood",
        "rose pine moon" => "Moonlit",
        "tokyo night" => "Neon Tokyo",
        _ => return name.to_string(),
    }
    .to_string()
}

/// A way the full-screen title sways (see `Settings::SWAY_PRESETS`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwayPreset {
    pub name: &'static str,
    pub lean: f32,
    pub speed: f32,
    pub bass: f32,
    pub bob: f32,
    /// 1 anchors every letter at its foot so only the tops sway, like a
    /// stalk of wheat; 0 turns the whole line about its middle.
    pub stalk: f32,
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 16] = [
        Self::System,
        Self::Light,
        Self::Dark,
        Self::Midnight,
        Self::Ocean,
        Self::Sunset,
        Self::Forest,
        Self::Rose,
        Self::Grape,
        Self::Amoled,
        Self::Mocha,
        Self::Cream,
        Self::Night,
        Self::Glass,
        Self::PinkMilk,
        Self::Custom,
    ];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::{gettext, pgettext};
        match self {
            Self::Dark => pgettext(locale, "theme", "Dark"),
            Self::Light => pgettext(locale, "theme", "Light"),
            Self::System => gettext(locale, "Follow system"),
            Self::Midnight => "Deep Space".into(),
            Self::Ocean => "Ocean".into(),
            Self::Sunset => "Sunset".into(),
            Self::Forest => "Forest".into(),
            Self::Rose => "Rosé".into(),
            Self::Grape => "Grape".into(),
            Self::Amoled => "Pure Black".into(),
            Self::Mocha => "Mocha".into(),
            Self::Cream => "Vanilla".into(),
            Self::Night => "Night Market".into(),
            Self::Glass => "Frosted Glass".into(),
            Self::PinkMilk => "Kitty Paws".into(),
            Self::Custom => "My colours".into(),
        }
    }
}

/// How outbound HTTP traffic reaches the network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// No proxy, ignoring environment and OS proxy settings.
    Off,
    /// Environment variables and, on macOS and Windows, the OS proxy.
    #[default]
    System,
    /// A configured HTTP proxy.
    Http,
    /// A configured SOCKS5 proxy.
    Socks,
}

impl ProxyMode {
    pub const ALL: [ProxyMode; 4] = [Self::Off, Self::System, Self::Http, Self::Socks];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::pgettext;
        match self {
            Self::Off => pgettext(locale, "proxy", "Off"),
            Self::System => pgettext(locale, "proxy", "System"),
            Self::Http => "HTTP".into(),
            Self::Socks => "SOCKS5".into(),
        }
    }

    pub fn is_manual(self) -> bool {
        matches!(self, Self::Http | Self::Socks)
    }
}

/// Only confirmed proxy preferences are written with other settings. The
/// password stays in memory and the protected store, never in this snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyPreferences {
    mode: ProxyMode,
    host: String,
    port: String,
    username: String,
}

impl ProxyPreferences {
    pub fn apply_to(&self, settings: &mut Settings) {
        settings.proxy_mode = self.mode;
        settings.proxy_host.clone_from(&self.host);
        settings.proxy_port.clone_from(&self.port);
        settings.proxy_username.clone_from(&self.username);
    }
}

fn proxy_mode_is_system(mode: &ProxyMode) -> bool {
    *mode == ProxyMode::System
}

/// A visualizer look saved by name: every visualizer setting as it was.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct VisPreset {
    pub name: String,
    pub values: serde_json::Map<String, serde_json::Value>,
}

impl Settings {
    /// The settings that make up a visualizer look.
    fn is_vis_key(key: &str) -> bool {
        key.starts_with("player_bar_vis") || key.starts_with("swirl_") || key.starts_with("vis_") && key != "vis_presets"
    }

    pub fn save_vis_preset(&mut self, name: &str) {
        let Ok(serde_json::Value::Object(all)) = serde_json::to_value(&*self) else {
            return;
        };
        let values: serde_json::Map<_, _> = all.into_iter().filter(|(key, _)| Self::is_vis_key(key)).collect();
        let name = name.trim().to_string();
        if let Some(old) = self.vis_presets.iter_mut().find(|preset| preset.name == name) {
            old.values = values;
        } else {
            self.vis_presets.push(VisPreset { name, values });
        }
    }

    pub fn apply_vis_preset(&mut self, index: usize) {
        let Some(preset) = self.vis_presets.get(index).cloned() else {
            return;
        };
        let Ok(serde_json::Value::Object(mut all)) = serde_json::to_value(&*self) else {
            return;
        };
        for (key, value) in preset.values {
            all.insert(key, value);
        }
        if let Ok(mut next) = serde_json::from_value::<Settings>(serde_json::Value::Object(all)) {
            next.proxy_password = std::mem::take(&mut self.proxy_password);
            next.proxy_password_legacy = self.proxy_password_legacy;
            *self = next;
        }
    }
}

/// A layout the reader saved with "Save my view".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MyView {
    pub sidebar: bool,
    pub queue: bool,
    pub lyrics_panel: bool,
    pub shapes: u8,
    pub art_expanded: bool,
    /// The interface zoom when it was saved; 0 means not saved.
    #[serde(default)]
    pub zoom: f32,
    /// The window's inner size when it was saved.
    #[serde(default)]
    pub window: Option<[f32; 2]>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The Spotify Connect name other devices see.
    pub device_name: String,
    /// 96, 160, or 320 kbps.
    pub bitrate: u16,
    pub normalisation: bool,
    pub autoplay: bool,
    pub gapless: bool,
    /// librespot backend name; `None` picks the platform default.
    pub audio_backend: Option<String>,
    pub audio_device: Option<String>,
    /// Windows output buffer in milliseconds. Smaller values may click under
    /// load; larger values delay playback controls.
    /// See [`crate::sink::DEFAULT_BUFFER_MS`].
    #[serde(default = "default_buffer_ms")]
    pub audio_buffer_ms: u32,
    /// Seconds one of the reader's own song files fades into the next; 0 is
    /// off. Spotify's songs are never crossfaded.
    #[serde(default)]
    pub crossfade_secs: u8,
    pub audio_cache: bool,
    pub audio_cache_mb: u64,
    pub theme: ThemeChoice,
    /// The interface language; older files without it follow the system.
    pub language: LanguageChoice,
    /// Filename selected from the local themes directory.
    pub custom_theme: Option<String>,
    /// Last accepted appearance, retained if its source file becomes unavailable.
    #[serde(
        default,
        deserialize_with = "fastframe_theme::read_cached_theme",
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_theme_cache: Option<crate::theme::CustomTheme>,
    /// Last detected system palette, so following Omarchy survives a restart.
    #[serde(
        default,
        deserialize_with = "fastframe_theme::read_cached_theme",
        skip_serializing_if = "Option::is_none"
    )]
    pub system_theme_cache: Option<crate::theme::CustomTheme>,
    pub home: HomeSettings,
    /// Tint the interface with the colour of the playing album's art.
    pub accent_from_art: bool,
    /// A spectrum or waveform of the playing song behind the player bar.
    pub player_bar_vis: PlayerBarVis,
    /// Which shapes are on, as bits: 1 bars, 2 flow, 4 swirl. Only read once
    /// `vis_shapes_set` is true; before that the single `player_bar_vis`
    /// mode of an older settings file decides.
    #[serde(default)]
    pub vis_shapes: u8,
    #[serde(default)]
    pub vis_shapes_set: bool,
    /// The shapes that were on before W switched them all off.
    #[serde(default)]
    pub vis_shapes_last: u8,
    /// Which edges the fullscreen bars hang from, as bits: 1 top, 2 left,
    /// 4 bottom, 8 right. Never set means top and left.
    #[serde(default)]
    pub vis_bar_sides: Option<u8>,
    /// Where the player bar's bars take their colour.
    pub player_bar_vis_color: VisColor,
    /// That colour, as `#rrggbb`, used when the source is `Fixed`.
    pub player_bar_vis_hex: String,
    /// How many bars stand across the player bar. The analyser always works
    /// in its own 75 bands; this merges them down, taking the loudest of
    /// each group, so fewer bars is coarser rather than a different reading.
    /// Past 75 the bars are spread across the bands instead, which is why
    /// the number is wider than a byte can hold.
    pub player_bar_vis_bars: u16,
    /// The gap between those bars, in pixels.
    pub player_bar_vis_gap: f32,
    /// How tall the bars rise for the same sound; 1.0 is the old height.
    #[serde(default)]
    pub player_bar_vis_bar_height: f32,
    /// The album-art swirl drawn behind the bars.
    pub player_bar_vis_backdrop: bool,
    /// The album-art swirl drawn behind the flow.
    #[serde(default)]
    pub player_bar_vis_backdrop_flow: bool,
    /// How many copies of the swirl tile fit across the bar: more is a
    /// busier, smaller swirl. 0 (never set) means the default.
    #[serde(default)]
    pub swirl_scale: f32,
    /// Size of the play controls, 1.0 is normal. 0 (never set) means normal.
    #[serde(default)]
    pub controls_scale: f32,
    /// The reader's own equalizer curves: name and ten gains.
    #[serde(default)]
    pub custom_eqs: Vec<(String, [f32; 10])>,
    /// The reader's own scenes: name, speed and an equalizer curve.
    #[serde(default)]
    pub custom_scenes: Vec<(String, f32, [f32; 10])>,
    /// Built-in presets and scenes the reader removed from the lists.
    #[serde(default)]
    pub hidden_presets: Vec<String>,
    /// Which of the cover's colours the bars take: 0 is the main one.
    pub player_bar_vis_colour_pick: u8,
    /// How many sheets the flow folds together, front to back.
    pub player_bar_vis_flow_sheets: u8,
    /// How much of the bar's height the flow's stack covers, from 0 to 1.
    pub player_bar_vis_flow_depth: f32,
    /// A multiplier on the flow's drift speed: 1 is the default.
    pub player_bar_vis_flow_speed: f32,
    /// How far each flow sheet is pushed along the spectrum from the one in
    /// front of it, as a fraction of the whole bar's readings. At zero every
    /// sheet traces the same shape and the stack reads as one thick ribbon;
    /// at a fifth they are visibly out of step and the folds show.
    pub player_bar_vis_flow_offset: f32,
    /// How much of the window's height the visualizer is drawn over, as a
    /// fraction. 1 is the player bar's own height; larger reaches up behind
    /// the page, which is what a reader wants from a full-bleed effect.
    pub player_bar_vis_scale: f32,
    /// Where the visualizer sits, as a fraction of the window's height
    /// measured from the bottom. 0 is the player bar's midline.
    pub player_bar_vis_rise: f32,
    /// How fast the stream runs, from a half to double time. The tempo and
    /// the pitch move together, which is what the presets that go with it
    /// are for.
    pub playback_speed: f32,
    /// Scenes locked to a song, as song URI to the index into
    /// [`crate::speed::PRESETS`]. A song that sounds wrong on its own scene
    /// is remembered as that song's own, and comes back to it whatever else
    /// the reader plays in between.
    #[serde(default)]
    pub song_scenes: std::collections::HashMap<String, usize>,
    /// Last local volume, 0..=65535.
    pub volume: u16,
    /// Whether the library sidebar is visible.
    pub sidebar_visible: bool,
    /// The playing album's art docked large at the sidebar's bottom.
    pub art_expanded: bool,
    /// How long the seek bar is: 0 short, 1 medium, 2 full width.
    #[serde(default)]
    pub seek_width: u8,
    /// Where the song-length bar sits in its row: 0 left (beside the
    /// controls), 1 middle, 2 right.
    #[serde(default)]
    pub seek_anchor: u8,
    /// Order of the three groups in the bottom row, an index into six
    /// orders of (controls, song bar, volume).
    #[serde(default)]
    pub row_order: u8,
    /// Where the play controls and the volume sit in the bottom row: 0 left,
    /// 1 middle, 2 right. Never set means by their place in `row_order`.
    #[serde(default)]
    pub anchor_controls: Option<u8>,
    #[serde(default)]
    pub anchor_volume: Option<u8>,
    /// How far the play controls and the volume have been moved from where
    /// their side puts them, in grid steps of 12 pixels (right is positive).
    #[serde(default)]
    pub nudge_controls: i16,
    #[serde(default)]
    pub nudge_volume: i16,
    /// The song bar moved along the row, in steps of 12 points.
    pub nudge_seek: i16,
    /// "My own layout": the controls and the volume sit where they were dragged.
    #[serde(default)]
    pub bar_free: bool,
    /// Where, as offsets from the bar's top-left: controls, then volume.
    #[serde(default)]
    pub bar_free_pos: Option<[[f32; 2]; 2]>,
    /// Full-screen lyrics: how far the little controls were dragged from their place.
    pub lyrics_ctl_off: [f32; 2],
    /// In "My own layout": where the song bar starts (across, down) from the bar's corner.
    pub bar_free_seek: Option<[f32; 2]>,
    /// The song bar on its own row under the controls and volume.
    pub bar_stacked: bool,
    /// Where the three parts of the bottom row sit: 0 side by side, 1 the
    /// song bar under the controls and volume, 2 all three on their own
    /// rows, 3 the song bar above the controls and volume. `bar_stacked`
    /// (older) means 1 while it is set.
    #[serde(default)]
    pub bar_layout: u8,
    /// Ask before removing one unavailable song from its playlist.
    pub unavailable_confirm_one: bool,
    /// Show the time as "1:23 / 3:45" before the bar instead of at both ends.
    #[serde(default)]
    pub seek_time_joined: bool,
    /// How the song time reads: 0 at both ends of the bar, 1 joined before
    /// it, 2 joined after it, 3 off.
    #[serde(default)]
    pub seek_time_mode: u8,
    /// How much the swirl is bent while it drifts, 0 to 1; 0 (never set)
    /// means the default.
    #[serde(default)]
    pub swirl_warp: f32,
    /// Keep the swirl's two colours fixed instead of drifting through the
    /// cover's colours.
    #[serde(default)]
    pub swirl_colours_fixed: bool,
    /// How many ripples run across the swirl; 0 (never set) means the default.
    #[serde(default)]
    pub swirl_waves: u8,
    /// The swirl's sliders, in the order of `SWIRL_TUNE`; empty means defaults.
    #[serde(default)]
    pub swirl_tune: Vec<f32>,
    /// Show the cover itself, mirrored and scrolling, instead of the waves.
    #[serde(default)]
    pub swirl_art_scroll: bool,
    /// With scrolling art: one cover repeating, not mirrored.
    #[serde(default)]
    pub swirl_art_single: bool,
    /// How soft the seams between repeated covers are, 0 to 100.
    pub swirl_art_edge: f32,
    /// Where fullscreen lyrics sit: 0 left, 1 centre, 2 right.
    #[serde(default)]
    pub lyrics_align: u8,
    /// Lyrics options as bits: 1 hide the cover, 2 hide timestamps,
    /// 4 count down to the next line.
    #[serde(default)]
    pub lyrics_flags: u8,
    /// The small lyrics panel's own options (its own copy of the bits above;
    /// only the timestamps bit applies there).
    #[serde(default)]
    pub side_lyrics_flags: u8,
    /// The small lyrics panel's line size: 0 normal, 1 smaller, 2 larger.
    #[serde(default)]
    pub side_lyrics_size: u8,
    /// How far the swirl's peaks rise, 1 to 100; 0 (never set) means the default.
    #[serde(default)]
    pub swirl_peaks: u8,
    /// How fast the colours drift on their own, 1.0 is normal; 0 means normal.
    #[serde(default)]
    pub swirl_drift: f32,
    /// A width in pixels the reader dragged or typed; 0 means use the preset.
    #[serde(default)]
    pub seek_custom_width: f32,
    /// The volume bar's width in pixels; 0 means the standard 100.
    #[serde(default)]
    pub volume_custom_width: f32,
    /// Bars and flow use one colour instead of a gradient to the cover's
    /// second colour.
    #[serde(default)]
    pub vis_no_gradient: bool,
    /// Lyrics drawn over the full-screen visualizer.
    #[serde(default)]
    pub vis_lyrics: bool,
    /// The full-screen visualizer's title holds still instead of swaying.
    #[serde(default)]
    pub vis_text_still: bool,
    /// The title only sways on the beat and holds still between hits.
    #[serde(default)]
    pub vis_sway_beat: bool,
    /// How the title sways, an index into `SWAY_PRESETS`.
    #[serde(default)]
    pub vis_sway: u8,
    /// How strongly, 0.2 to 3; 0 (never set) means 1.
    #[serde(default)]
    pub vis_sway_amount: f32,
    /// ... and has no outline.
    #[serde(default)]
    pub vis_text_no_outline: bool,
    /// ... and sits on a dark (or light) panel.
    #[serde(default)]
    pub vis_text_back: bool,
    /// The panel behind the title: how solid (0.1 to 1), how round, how much
    /// room round the words, and whether it is one block or a strip a line.
    pub vis_back_alpha: f32,
    pub vis_back_round: f32,
    pub vis_back_pad: f32,
    pub vis_back_block: bool,
    /// Hovering a font shows it on the title at once.
    pub vis_live_preview: bool,
    /// Each song gets a title font of its own, picked from the ones installed.
    pub vis_font_random: bool,
    /// Which of `system_fonts::VIS_FONTS` it is drawn in.
    #[serde(default)]
    pub vis_text_font: u8,
    /// Hide the artist name under the full-screen title.
    pub vis_text_no_artist: bool,
    /// Show the playing song on the reader's Discord profile.
    pub discord_presence: bool,
    /// The Application ID of the Discord app that presence is shown as.
    pub discord_client_id: String,
    /// What the line under the name in Discord's member list says: 0 the song,
    /// 1 the artist, 2 chanceify.
    pub discord_status_line: u8,
    /// Show the album cover on the profile card.
    pub discord_cover: bool,
    /// Show chanceify's small badge in the corner of the cover.
    pub discord_badge: bool,
    /// Show a swirl in the colour of the cover as the small picture on the cover.
    #[serde(default = "yes")]
    pub discord_swirl: bool,
    /// The layout saved with "Save my view".
    pub my_view: Option<MyView>,
    /// Visualizer looks saved by name.
    pub vis_presets: Vec<VisPreset>,
    /// Where the reader dragged the Views disc to, in points from the window's top left.
    pub views_disc: Option<[f32; 2]>,
    /// How much of the Views disc shows when the pointer has been away (0.2 = a fifth).
    pub disc_dim: f32,
    /// Calm mode's background: 0 the theme's colours, 1 the blurred cover, 2 the blue ocean.
    pub calm_look: u8,
    /// The pop-out controls: [x, y, width]. None = not shown.
    pub float_controls: Option<[f32; 3]>,
    /// Where the controls panel sits while the album art is big, and in full-screen lyrics.
    pub float_hide_bar: bool,
    /// Width of the Views panel.
    pub views_width: f32,
    /// The mini player window: x, y, width, height.
    pub mini_window: Option<[f32; 4]>,
    /// The extra visualizer window: x, y, width, height.
    pub extra_window: Option<[f32; 4]>,
    /// The full window's size before the mini player, kept between launches.
    pub mini_restore_size: Option<[f32; 2]>,
    pub float_big: Option<[f32; 3]>,
    pub float_lyrics: Option<[f32; 3]>,
    /// Calm mode: how much of the cover shows soft (0 sharp, 1 fully soft).
    pub calm_blur: f32,
    /// Hovering a theme in the list shows it for the moment.
    #[serde(default)]
    pub theme_point_preview: bool,
    /// Say which playlist the song is playing from.
    pub discord_playlist: bool,
    /// A button that opens the reader's Spotify profile.
    pub discord_profile: bool,
    pub discord_playlist_button: bool,
    /// 0 album big, swirl small; 1 swirl big, album small; 2 chanceify badge only.
    pub discord_look: u8,
    pub discord_song_page: bool,
    /// Show the "Listen on Spotify" and "Get chanceify" buttons.
    pub discord_buttons: bool,
    /// Make the song and the artist links to Spotify.
    pub discord_links: bool,
    /// Show nothing while paused (otherwise the song shows as paused).
    pub discord_hide_paused: bool,
    /// Show songs played from the reader's own files.
    pub discord_files: bool,
    /// Let friends press Join on the profile to hear the same song, and join
    /// theirs. Off until asked for.
    pub discord_listen_along: bool,
    /// ... without the dark panel behind them (the panel is the default).
    #[serde(default)]
    pub vis_lyrics_no_back: bool,
    /// Bars stay on the visualizer instead of rising onto the song-length row.
    #[serde(default)]
    pub vis_bars_stay: bool,
    /// How solid the window is, 0.3 to 1, so the desktop shows through;
    /// 0 (never set) means solid (the Frosted Glass theme has its own look).
    #[serde(default)]
    pub window_opacity: f32,
    /// What moves behind the full-screen lyrics: 0 bars, 1 flow, 2 swirl.
    #[serde(default)]
    pub lyrics_vis_mode: u8,
    /// The visualizer moves behind the full-screen lyrics.
    #[serde(default)]
    pub lyrics_vis: bool,
    /// How dark the full-screen lyrics page is over that spectrum, 0.1 to
    /// 0.95; 0 (never set) means 0.6.
    #[serde(default)]
    pub lyrics_vis_dark: f32,
    /// How solid the bars and the flow are, 0.1 to 1; 0 (never set) means 1.
    #[serde(default)]
    pub vis_bars_opacity: f32,
    /// What the swirl's speed follows: 0 loudness, 1 bass, 2 beat, 3 nothing.
    #[serde(default)]
    pub swirl_react_mode: u8,
    /// What makes the screen jump by itself: 0 loudness, 1 bass, 2 beat,
    /// 3 nothing (only the bass-jump key).
    pub vis_jump_react: u8,
    /// The screen jumps by itself, following `vis_jump_react`.
    pub vis_jump_on: bool,
    #[serde(default)]
    pub vis_flow_opacity: f32,
    /// The volume shortcut buttons; never set means 50 and 100.
    #[serde(default)]
    pub volume_presets: Option<Vec<u8>>,
    /// Whether the "press T" hint has been shown at least once.
    #[serde(default)]
    pub hint_seen: bool,
    /// The preset the equalizer curve started from; empty means Flat.
    #[serde(default)]
    pub eq_base: String,
    /// Playlist name size in the stacked list with details off; 0 = default (20).
    #[serde(default)]
    pub library_name_size: u8,
    /// Use compact single-line rows without cover art in the sidebar.
    pub sidebar_compact: bool,
    /// Show the Library as responsive cover cards instead of rows.
    pub sidebar_grid: bool,
    /// Name the cards under their covers in the Library grid. Off leaves
    /// the covers alone, which is the whole point of a grid of them.
    pub library_grid_names: bool,
    /// How many cards across the library grid. Zero is as many as fit.
    pub library_grid_columns: u8,
    /// Draw each row's detail line — "Playlist • chan", "Album • Artist" —
    /// under its name. On by default.
    /// This replaced a setting that hid the *name* on the account's own
    /// playlists and left those rows reading only "Playlist", which names
    /// nothing. It was renamed rather than reused: a file still carrying the
    /// old key has no `library_details`, so it reads as true and every row
    /// keeps its detail line, which is what it had.
    pub library_details: bool,

    /// Which part of the Library the sidebar is showing. Remembered across
    /// launches, because the picker is one button inside a menu now.
    pub library_shelf: LibraryShelf,
    pub sidebar_width: f32,
    pub lyrics_width: f32,
    pub queue_width: f32,
    /// Whether the queue and recents panels draw song names alone.
    ///
    /// Off is the full row: cover art, the playlist icons beside the title,
    /// and the length. On is names and nothing else, which fits more rows
    /// in a narrow panel and is what a reader wants when they are only
    /// looking for the next song.
    pub queue_compact: bool,
    /// Whether the queue and recents rows show how long each song is.
    /// Off hands that column to the song name, which is the one thing in a
    /// queue row anybody actually reads.
    pub queue_show_numbers: bool,
    pub queue_show_time: bool,
    /// The song length column in the queue and recents (right-click the tabs).
    pub queue_show_length: bool,
    /// Whether queue rows show the artist under the song name.
    pub queue_show_artist: bool,
    /// Whether queue rows show the playlist icons.
    pub queue_show_icons: bool,
    /// Whether queue rows show the cover.
    pub queue_show_cover: bool,
    /// The Recent tab keeps its own look, apart from the queue's, so each
    /// can be set up the way its reader wants it.
    pub recents_show_numbers: bool,
    pub recents_show_artist: bool,
    pub recents_show_icons: bool,
    pub recents_show_cover: bool,
    /// How the title sits in the full-screen visualizer: 0 under the cover,
    /// 1 beside it, 2 beside it and big, 3 above it with the artist below.
    pub vis_title_layout: u8,
    /// How hard the bass jump shakes the window (1.0 = normal).
    pub jump_shake: f32,
    /// Which app icon the window and taskbar button wear. See `app_icons`.
    pub app_icon: u8,
    /// The compact mini player layout, switched on by the reader. A window
    /// shrunk small enough turns into one by itself as well.
    pub mini_player: bool,
    /// Whether the mini player lists the queue under the controls.
    pub mini_queue: bool,
    /// Show the volume bar in the mini player.
    pub mini_volume: bool,
    /// Whether the mini player stays on top of other windows.
    pub mini_on_top: bool,
    /// Whether the floating mini player fades while the pointer is away.
    pub mini_fade: bool,
    /// The click that adds a song to the queue without playing it: 0 Ctrl
    /// (the default), 1 Alt, 2 off.
    #[serde(default)]
    pub queue_click: u8,
    /// The visualizer runs from right to left instead of left to right.
    #[serde(default)]
    pub vis_reverse: bool,
    /// The missing-art mark also shows on the big album art.
    #[serde(default = "yes")]
    pub missing_mark_big: bool,
    /// What moves behind the mini player: 0 nothing, 1 bars, 2 flow, 3
    /// swirl; never chosen means bars.
    #[serde(default)]
    pub mini_vis: Option<u8>,
    /// Folders of music files, besides the songs folder, that the Local
    /// songs page reads.
    pub music_folders: Vec<String>,
    /// Last.fm: the key and secret of the registered application (empty
    /// uses the ones built in), and the signed-in account's name and
    /// session key. The session key only lets chanceify™ scrobble.
    pub lastfm_api_key: String,
    pub lastfm_secret: String,
    pub lastfm_user: String,
    pub lastfm_session: String,
    /// Count what is played on Last.fm.
    pub lastfm_scrobble: bool,
    /// Tell Last.fm what is playing right now.
    pub lastfm_now_playing: bool,
    /// Include songs played from the person's own files.
    pub lastfm_files: bool,
    /// Rebound keys: shortcut id to key name. Missing means the default.
    pub key_bindings: std::collections::BTreeMap<String, String>,
    /// Which set of default keys the saved choices were made against.
    #[serde(default)]
    pub keymap_version: u8,
    /// Colour the whole app from the playing song's cover.
    pub theme_from_cover: bool,
    /// The window colour of the "My own colours" theme.
    pub custom_bg: [u8; 3],
    /// Its accent colour.
    pub custom_accent: [u8; 3],
    /// Use compact single-line rows without cover art in track lists.
    pub tracklist_compact: bool,
    /// How wide each optional column of a track list is, in pixels, shared
    /// by every list so a layout learned once holds everywhere. Zero hides
    /// the column. Dragged in the column header, so it is remembered here
    /// rather than set in this window.
    pub track_columns: crate::settings::TrackColumns,
    /// How large the playlist icons on a song row are, as a scale on the
    /// base size. Set by right-clicking an icon rather than in this window:
    /// it is a thing you want to judge by looking at a row.
    pub membership_icon_scale: f32,
    /// Linux: middle-click a list to autoscroll it. Off by default, because
    /// Linux desktops usually paste the primary selection on middle click.
    /// Windows always autoscrolls and macOS never does.
    pub middle_click_autoscroll: bool,
    pub search_history: Vec<String>,
    pub show_shortcut_hints: bool,
    /// An optional personal Spotify Web API application id. The shared
    /// application remains active for coverage when this is present.
    pub web_client_id: Option<String>,
    /// Legacy reminder time, retained for older Chanceify versions.
    pub personal_app_nudge_at: Option<String>,
    /// The listener has dismissed or followed the personal-app introduction.
    pub personal_app_intro_seen: bool,
    /// Local playback has been authorized at least once on this machine, so
    /// the app can resume it silently instead of prompting.
    pub playback_authorized: bool,
    /// Closing the window hides to the tray and keeps the music playing.
    ///
    /// Off by default here: the X button quits rather than leaving a tray
    /// icon behind, which is what a user pressing X expects. The setting and
    /// its tray menu stay, so the old behaviour is one toggle away in
    /// Settings → Playback.
    pub keep_playing_in_background: bool,
    /// Ask GitHub once a day whether a newer release exists.
    pub check_for_updates: bool,
    pub download_updates_automatically: bool,
    /// Context URIs and the local Liked Songs key, in pin order.
    pub pinned_contexts: Vec<String>,
    /// Older settings keep Liked Songs first until it is moved or unpinned.
    pub liked_songs_pinned: bool,
    /// The sidebar's own playlist order, set by dragging rows. Kept while
    /// another sort is selected; empty means no saved local arrangement.
    pub sidebar_order: Vec<String>,
    /// Explicit order per Library shelf. Missing shelves keep their previous
    /// behaviour; selecting another order never deletes the local arrangement.
    pub library_sort: std::collections::BTreeMap<LibraryShelf, LibrarySort>,
    /// Interface zoom, egui's zoom factor; Ctrl+plus/minus changes it.
    pub zoom: f32,
    /// The Winamp window is open.
    pub winamp_window: bool,
    /// Windows and X11: keep a taskbar button while the Winamp window is visible.
    pub winamp_show_taskbar: bool,
    /// Skin file or folder name. `None` selects the built-in skin.
    pub skin: Option<String>,
    /// Pick a different skin each time the mini player opens; `skin` holds
    /// the one picked.
    pub random_skin: bool,
    /// Screen pixels per skin pixel; `None` picks double size for the
    /// display.
    pub skin_scale: Option<u8>,
    /// The Winamp window stays above other windows.
    pub winamp_on_top: bool,
    /// The mini player's visualiser: bars, scope, or off.
    pub vis: VisMode,
    /// The playlist window is open under the mini player.
    pub playlist_open: bool,
    /// How tall the playlist window is, in skin pixels.
    pub playlist_height: u32,
    /// The equalizer window is open under the mini player.
    pub eq_open: bool,
    /// The equalizer shapes local playback.
    pub eq_on: bool,
    /// The preamp, in decibels, never above zero.
    pub eq_preamp_db: f32,
    /// The ten bands, in decibels, 60 Hz to 16 kHz.
    pub eq_bands_db: [f32; 10],
    /// The balance, -1 all left to 1 all right.
    pub balance: f32,
    /// Play both channels the same.
    pub mono: bool,
    /// The playlist window is rolled up to its title bar.
    pub playlist_shaded: bool,
    /// The equalizer window is rolled up to its title bar.
    pub eq_shaded: bool,
    /// The main window is rolled up to its title bar.
    pub winamp_shaded: bool,
    /// The MilkDrop window is open (its own window, not part of the skin).
    pub milkdrop_open: bool,
    /// How long each preset plays before the next, in seconds.
    pub milkdrop_seconds: u32,
    /// How many frames a second the MilkDrop window draws; 0 is uncapped.
    pub milkdrop_fps: u32,
    /// Last reported MilkDrop screen refresh rate. The first value sets the
    /// default frame rate; this field is not directly configurable.
    pub milkdrop_screen_hz: u32,
    /// The picture's inner resolution: 1 full, 2 half, 4 quarter.
    pub milkdrop_scale: u32,
    /// The MilkDrop window fills the screen.
    pub milkdrop_fullscreen: bool,
    /// The MilkDrop window's size in logical points, when not full-screen.
    pub milkdrop_size: [f32; 2],
    /// Which proxy to use. Older files without this field stay on `system`.
    #[serde(default, skip_serializing_if = "proxy_mode_is_system")]
    pub proxy_mode: ProxyMode,
    /// Combined address from older settings files. Split into host and port
    /// on load; not written again.
    #[serde(default, skip_serializing)]
    pub proxy: String,
    /// Host of a manual HTTP or SOCKS5 proxy. Ignored when the mode is Off
    /// or System.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_host: String,
    /// Port of a manual HTTP or SOCKS5 proxy.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_port: String,
    /// Optional proxy login for Web requests.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_username: String,
    /// Optional proxy password, in memory and the platform credential store.
    /// Legacy JSON copies remain until protected migration succeeds.
    #[serde(default, skip_serializing)]
    pub proxy_password: String,
    /// Legacy settings and their password must survive until protected migration
    /// succeeds. This flag is only in memory and is never persisted.
    #[serde(skip)]
    pub proxy_password_legacy: bool,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Serialization already omits the password and old combined URL.
        // Keep the proxy username out of diagnostics as well.
        let mut preferences = serde_json::to_value(self).map_err(|_| std::fmt::Error)?;
        if let Some(fields) = preferences.as_object_mut() {
            fields.remove("proxy_username");
        }
        f.debug_struct("Settings")
            .field("preferences", &preferences)
            .finish()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            device_name: "chanceify".to_string(),
            bitrate: 320,
            normalisation: false,
            autoplay: true,
            gapless: true,
            audio_backend: None,
            audio_device: None,
            audio_buffer_ms: default_buffer_ms(),
            crossfade_secs: 0,
            audio_cache: true,
            audio_cache_mb: 1024,
            theme: ThemeChoice::System,
            language: LanguageChoice::System,
            custom_theme: None,
            custom_theme_cache: None,
            system_theme_cache: None,
            home: HomeSettings::default(),
            accent_from_art: true,
            player_bar_vis: PlayerBarVis::Off,
            vis_shapes: 0,
            vis_shapes_set: false,
            vis_shapes_last: 0,
            vis_bar_sides: None,
            player_bar_vis_color: VisColor::AlbumArt,
            player_bar_vis_hex: "#8b5cf6".into(),
            player_bar_vis_bars: crate::vis::WIDE_BANDS as u16,
            player_bar_vis_gap: 2.0,
            player_bar_vis_bar_height: 1.0,
            player_bar_vis_backdrop: false,
            player_bar_vis_backdrop_flow: false,
            swirl_scale: 0.0,
            controls_scale: 0.0,
            custom_eqs: Vec::new(),
            custom_scenes: Vec::new(),
            hidden_presets: Vec::new(),
            player_bar_vis_colour_pick: 0,
            player_bar_vis_flow_sheets: crate::ui::player_bar::FLOW_SHEETS as u8,
            player_bar_vis_flow_depth: 0.86,
            player_bar_vis_flow_speed: 1.0,
            player_bar_vis_flow_offset: 0.0,
            player_bar_vis_scale: 1.0,
            player_bar_vis_rise: 0.0,
            playback_speed: 1.0,
            song_scenes: std::collections::HashMap::new(),
            volume: (u16::MAX as u32 * 70 / 100) as u16,
            sidebar_visible: true,
            art_expanded: false,
            seek_width: 1,
            seek_anchor: 0,
            row_order: 0,
            anchor_controls: None,
            nudge_controls: 0,
            nudge_volume: 0,
            nudge_seek: 0,
            bar_free: false,
            bar_free_pos: None,
            lyrics_ctl_off: [0.0, 0.0],
            bar_free_seek: None,
            bar_stacked: false,
            bar_layout: 0,
            unavailable_confirm_one: true,
            anchor_volume: None,
            seek_time_joined: false,
            seek_time_mode: 0,
            swirl_warp: 0.0,
            swirl_colours_fixed: false,
            swirl_waves: 0,
            swirl_tune: Vec::new(),
            swirl_art_scroll: false,
            swirl_art_single: false,
            swirl_art_edge: 60.0,
            lyrics_align: 0,
            lyrics_flags: 0,
            side_lyrics_flags: 0,
            side_lyrics_size: 0,
            swirl_peaks: 0,
            swirl_drift: 0.0,
            seek_custom_width: 0.0,
            volume_custom_width: 0.0,
            vis_no_gradient: false,
            vis_lyrics: false,
            vis_text_still: false,
            vis_sway: 0,
            vis_sway_beat: false,
            vis_sway_amount: 0.0,
            vis_text_no_outline: false,
            vis_text_back: false,
            vis_back_alpha: 0.55,
            vis_back_round: 10.0,
            vis_back_pad: 12.0,
            vis_back_block: false,
            vis_live_preview: true,
            vis_font_random: false,
            vis_text_font: 0,
            vis_text_no_artist: false,
            discord_presence: false,
            discord_client_id: String::new(),
            discord_status_line: 0,
            discord_cover: true,
            discord_badge: true,
            discord_swirl: true,
            my_view: None,
            views_disc: None,
            disc_dim: 0.2,
            calm_look: 0,
            float_controls: None,
            float_hide_bar: false,
            views_width: 316.0,
            mini_window: None,
            extra_window: None,
            mini_restore_size: None,
            float_big: None,
            float_lyrics: None,
            calm_blur: 0.8,
            theme_point_preview: false,
            discord_playlist: false,
            discord_profile: false,
            discord_playlist_button: false,
            discord_look: 0,
            discord_song_page: false,
            discord_buttons: true,
            discord_links: true,
            discord_hide_paused: false,
            discord_files: true,
            discord_listen_along: false,
            vis_lyrics_no_back: false,
            vis_bars_stay: false,
            window_opacity: 0.0,
            lyrics_vis_mode: 0,
            lyrics_vis: false,
            lyrics_vis_dark: 0.0,
            vis_bars_opacity: 0.0,
            swirl_react_mode: 0,
            vis_jump_react: 1,
            vis_jump_on: false,
            vis_flow_opacity: 0.0,
            volume_presets: None,
            hint_seen: false,
            library_name_size: 0,
            eq_base: String::new(),
            sidebar_compact: false,
            sidebar_grid: false,
            library_grid_names: true,
            library_grid_columns: 0,
            library_details: true,

            library_shelf: LibraryShelf::Playlists,
            sidebar_width: 250.0,
            lyrics_width: 360.0,
            queue_width: 360.0,
            queue_compact: false,
            queue_show_numbers: true,
            queue_show_time: true,
            queue_show_length: false,
            queue_show_artist: true,
            queue_show_icons: true,
            queue_show_cover: true,
            recents_show_numbers: true,
            recents_show_artist: true,
            recents_show_icons: true,
            recents_show_cover: true,
            vis_title_layout: 0,
            vis_presets: Vec::new(),
            jump_shake: 1.0,
            app_icon: 1,
            mini_player: false,
            mini_queue: true,
            mini_volume: true,
            mini_on_top: false,
            mini_fade: true,
            mini_vis: None,
            queue_click: 0,
            vis_reverse: false,
            missing_mark_big: true,
            music_folders: Vec::new(),
            lastfm_api_key: String::new(),
            lastfm_secret: String::new(),
            lastfm_user: String::new(),
            lastfm_session: String::new(),
            lastfm_scrobble: true,
            lastfm_now_playing: true,
            lastfm_files: true,
            key_bindings: std::collections::BTreeMap::new(),
            keymap_version: 5,
            theme_from_cover: false,
            custom_bg: [0x14, 0x16, 0x20],
            custom_accent: [0x7c, 0x8c, 0xff],
            tracklist_compact: false,
            track_columns: crate::settings::TrackColumns::default(),
            membership_icon_scale: 1.0,
            middle_click_autoscroll: false,
            search_history: Vec::new(),
            show_shortcut_hints: true,
            web_client_id: None,
            personal_app_nudge_at: None,
            personal_app_intro_seen: false,
            playback_authorized: false,
            keep_playing_in_background: false,
            check_for_updates: true,
            download_updates_automatically: false,
            pinned_contexts: Vec::new(),
            liked_songs_pinned: true,
            sidebar_order: Vec::new(),
            library_sort: std::collections::BTreeMap::new(),
            zoom: 1.0,
            winamp_window: false,
            winamp_show_taskbar: true,
            skin: None,
            random_skin: false,
            skin_scale: None,
            winamp_on_top: false,
            vis: VisMode::default(),
            playlist_open: false,
            playlist_height: 174,
            eq_open: false,
            eq_on: false,
            eq_preamp_db: 0.0,
            eq_bands_db: [0.0; 10],
            balance: 0.0,
            mono: false,
            playlist_shaded: false,
            eq_shaded: false,
            winamp_shaded: false,
            milkdrop_open: false,
            milkdrop_seconds: crate::milkdrop::DEFAULT_SECONDS,
            milkdrop_fps: crate::milkdrop::DEFAULT_FPS,
            milkdrop_screen_hz: 0,
            milkdrop_scale: 1,
            milkdrop_fullscreen: false,
            milkdrop_size: crate::milkdrop::DEFAULT_SIZE,
            proxy_mode: ProxyMode::System,
            proxy: String::new(),
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_password_legacy: false,
        }
    }
}

fn yes() -> bool {
    true
}

fn default_buffer_ms() -> u32 {
    crate::sink::DEFAULT_BUFFER_MS
}

impl Settings {
    /// The bar colour the reader chose, parsed. A hand-edited file can hold
    /// anything, and a bar nobody can see is worse than a wrong one, so an
    /// unreadable value falls back to the theme's accent.
    pub fn player_bar_vis_color_value(&self, theme_accent: egui::Color32) -> egui::Color32 {
        match self.player_bar_vis_color {
            VisColor::Theme => theme_accent,
            VisColor::Fixed => {
                egui::Color32::from_hex(&self.player_bar_vis_hex).unwrap_or(theme_accent)
            }
            VisColor::AlbumArt => theme_accent,
        }
    }

    /// The fewest bars worth drawing: below this the row is all gap.
    pub const VIS_BARS_MIN: u16 = 32;
    /// The most bars worth drawing. Past the analyser's own seventy-five
    /// bands the extra bars are a smooth reading of the same bands rather
    /// than new information, which is what a dense bar field wants.
    pub const VIS_BARS_MAX: u16 = 1024;
    /// The widest gap that still leaves a bar's own width showing. Past
    /// this the row is mostly hole, which reads as a bug rather than a
    /// choice, so a gap slider stops here.
    pub const VIS_GAP_MAX: f32 = 10.0;

    /// How many bars to draw, within what is worth drawing.
    pub fn player_bar_vis_bar_count(&self) -> usize {
        (self.player_bar_vis_bars as usize)
            .clamp(Self::VIS_BARS_MIN as usize, Self::VIS_BARS_MAX as usize)
    }

    /// The gap between bars, kept inside a range that leaves a bar visible.
    /// The bar height multiplier, 1.0 when never set.
    pub fn player_bar_vis_bar_height_value(&self) -> f32 {
        if self.player_bar_vis_bar_height.is_finite() && self.player_bar_vis_bar_height > 0.0 {
            self.player_bar_vis_bar_height.clamp(0.5, 3.0)
        } else {
            1.0
        }
    }

    /// Swirl tiles across the bar; larger means smaller, busier swirls.
    pub fn swirl_scale_value(&self) -> f32 {
        if self.swirl_scale.is_finite() && self.swirl_scale > 0.0 {
            self.swirl_scale.clamp(0.5, 5.0)
        } else {
            1.8
        }
    }

    /// Every one of the reader's own equalizers and scenes as plain text, one
    /// per line: `EQ|name|ten gains` and `SCENE|name|speed|ten gains`.
    pub fn export_presets(&self) -> String {
        let gains = |bands: &[f32; 10]| {
            bands
                .iter()
                .map(|gain| format!("{gain:.1}"))
                .collect::<Vec<_>>()
                .join(",")
        };
        let mut out = String::new();
        for (name, bands) in &self.custom_eqs {
            out.push_str(&format!("EQ|{}|{}\n", name.replace('|', "/"), gains(bands)));
        }
        for (name, speed, bands) in &self.custom_scenes {
            out.push_str(&format!(
                "SCENE|{}|{speed:.2}|{}\n",
                name.replace('|', "/"),
                gains(bands)
            ));
        }
        out
    }

    /// Adds the presets in `text` (as `export_presets` writes them),
    /// replacing any of the same name. Returns how many were read; lines that
    /// do not make sense are skipped.
    pub fn import_presets(&mut self, text: &str) -> usize {
        fn bands(text: &str) -> Option<[f32; 10]> {
            let values: Vec<f32> = text
                .split(',')
                .filter_map(|part| part.trim().parse::<f32>().ok())
                .filter(|gain| gain.is_finite())
                .collect();
            let array: [f32; 10] = values.try_into().ok()?;
            Some(array.map(|gain| gain.clamp(-crate::eq::RANGE_DB, crate::eq::RANGE_DB)))
        }
        let mut count = 0;
        for line in text.lines() {
            let parts: Vec<&str> = line.trim().split('|').collect();
            match parts.as_slice() {
                ["EQ", name, gains] if !name.trim().is_empty() => {
                    if let Some(gains) = bands(gains) {
                        let name = name.trim().chars().take(40).collect::<String>();
                        self.custom_eqs.retain(|(other, _)| *other != name);
                        self.custom_eqs.push((name, gains));
                        count += 1;
                    }
                }
                ["SCENE", name, speed, gains] if !name.trim().is_empty() => {
                    if let (Ok(speed), Some(gains)) = (speed.trim().parse::<f32>(), bands(gains)) {
                        let name = name.trim().chars().take(40).collect::<String>();
                        self.custom_scenes.retain(|(other, _, _)| *other != name);
                        self.custom_scenes.push((name, crate::speed::clamped(speed), gains));
                        count += 1;
                    }
                }
                _ => {}
            }
        }
        count
    }

    pub const SHAPE_BARS: u8 = 1;
    pub const SHAPE_FLOW: u8 = 2;
    pub const SHAPE_SWIRL: u8 = 4;

    /// The shapes that are on, as bits (`SHAPE_*`). An older settings file
    /// with one mode, and the backdrop switches it had, reads as the same
    /// picture here.
    pub fn vis_shapes_value(&self) -> u8 {
        if self.vis_shapes_set {
            return self.vis_shapes & 7;
        }
        match self.player_bar_vis {
            PlayerBarVis::Off => 0,
            PlayerBarVis::Spectrum => {
                Self::SHAPE_BARS
                    | if self.player_bar_vis_backdrop { Self::SHAPE_SWIRL } else { 0 }
            }
            PlayerBarVis::Flow => {
                Self::SHAPE_FLOW
                    | if self.player_bar_vis_backdrop_flow { Self::SHAPE_SWIRL } else { 0 }
            }
            PlayerBarVis::Swirl | PlayerBarVis::Waveform => Self::SHAPE_SWIRL,
        }
    }

    /// The fullscreen bars' edges: top and left unless chosen otherwise.
    pub fn vis_bar_sides_value(&self) -> u8 {
        self.vis_bar_sides.unwrap_or(3) & 15
    }

    /// Every swirl slider: label, lowest, highest, default. The first eight
    /// are baked into the picture; the rest act while it plays.
    pub const SWIRL_TUNE: [(&'static str, f32, f32, f32); 13] = [
        ("Ripples: how many", 1.0, 8.0, 2.0),
        ("Ripples: how tall", 1.0, 100.0, 10.0),
        ("Ripples: crossing", 0.0, 200.0, 100.0),
        ("Angle", -45.0, 45.0, 0.0),
        ("Pattern position", 0.0, 100.0, 0.0),
        ("Blur", 0.0, 30.0, 7.0),
        ("Colour strength", 50.0, 250.0, 140.0),
        ("Contrast", 60.0, 200.0, 120.0),
        ("Speed", 0.0, 300.0, 100.0),
        ("Speeds up with the music", 0.0, 300.0, 100.0),
        ("Brightness", 20.0, 100.0, 100.0),
        ("Colour overlay", 0.0, 150.0, 100.0),
        ("Colour overlay: bands", 1.0, 6.0, 1.8),
    ];

    /// The bottom row's layout, 0 to 3 (see `bar_layout`).
    pub fn bar_layout_value(&self) -> u8 {
        if self.bar_stacked { 1 } else { self.bar_layout.min(3) }
    }

    /// What moves behind the mini player (see `mini_vis`).
    pub fn mini_vis_mode(&self) -> u8 {
        self.mini_vis.unwrap_or(1).min(3)
    }

    /// How strongly the title sways, 0.2 to 3.
    pub fn vis_sway_strength(&self) -> f32 {
        let value = self.vis_sway_amount;
        if value.is_finite() && value > 0.0 { value.clamp(0.2, 3.0) } else { 1.0 }
    }

    /// The chosen sway preset.
    pub fn vis_sway_preset(&self) -> SwayPreset {
        Self::SWAY_PRESETS[(self.vis_sway as usize).min(Self::SWAY_PRESETS.len() - 1)]
    }

    /// Ready-made sways for the full-screen title: a name, how far a line
    /// leans, how fast, how much the bass adds, and how far each letter
    /// bobs on its own (a fraction of its height).
    pub const SWAY_PRESETS: [SwayPreset; 12] = [
        SwayPreset { name: "Sway", lean: 0.9, speed: 1.0, bass: 1.0, bob: 0.0, stalk: 1.0 },
        SwayPreset { name: "Soft sway", lean: 0.5, speed: 0.6, bass: 0.5, bob: 0.0, stalk: 1.0 },
        SwayPreset { name: "Big sway", lean: 1.6, speed: 1.3, bass: 1.0, bob: 0.0, stalk: 1.0 },
        SwayPreset { name: "Bass pump", lean: 0.5, speed: 1.0, bass: 3.5, bob: 0.0, stalk: 0.0 },
        SwayPreset { name: "Wave", lean: 0.25, speed: 1.0, bass: 1.0, bob: 0.22, stalk: 0.0 },
        SwayPreset { name: "Dance", lean: 0.6, speed: 1.6, bass: 2.0, bob: 0.14, stalk: 0.0 },
        SwayPreset { name: "Dance calm", lean: 0.3, speed: 1.2, bass: 1.5, bob: 0.12, stalk: 0.0 },
        SwayPreset { name: "Dance bounce", lean: 0.15, speed: 1.8, bass: 3.0, bob: 0.2, stalk: 0.0 },
        SwayPreset { name: "Dance fast", lean: 0.4, speed: 2.4, bass: 2.0, bob: 0.14, stalk: 0.0 },
        SwayPreset { name: "Drunk", lean: 1.2, speed: 0.7, bass: 1.0, bob: 0.1, stalk: 0.0 },
        SwayPreset { name: "Drunk tipsy", lean: 0.7, speed: 0.6, bass: 0.8, bob: 0.07, stalk: 0.0 },
        SwayPreset { name: "Drunk wobbly", lean: 1.6, speed: 1.0, bass: 1.2, bob: 0.14, stalk: 0.0 },
    ];

    /// How solid the window is: 1 is opaque, down to 0.3.
    pub fn window_solidity(&self) -> f32 {
        let value = self.window_opacity;
        if value.is_finite() && value > 0.0 { value.clamp(0.3, 1.0) } else { 1.0 }
    }

    /// Whether the window lets the desktop show through at all.
    pub fn window_see_through(&self) -> bool {
        self.theme == ThemeChoice::Glass || self.window_solidity() < 1.0
    }

    /// How dark the full-screen lyrics page is over its visualizer.
    pub fn lyrics_vis_darkness(&self) -> f32 {
        let value = self.lyrics_vis_dark;
        if value.is_finite() && value > 0.0 { value.clamp(0.1, 0.95) } else { 0.6 }
    }

    /// How solid the bars (true) or the flow (false) are, 0.1 to 1.
    pub fn vis_opacity(&self, bars: bool) -> f32 {
        let value = if bars { self.vis_bars_opacity } else { self.vis_flow_opacity };
        if value.is_finite() && value > 0.0 { value.clamp(0.1, 1.0) } else { 1.0 }
    }

    /// Ready-made swirl looks: a name and a value for each of the sliders
    /// in `SWIRL_TUNE`, in the same order. Every slider stays editable.
    pub const SWIRL_PRESETS: [(&'static str, [f32; 13]); 18] = [
        ("Default", [2.0, 10.0, 100.0, 0.0, 0.0, 7.0, 140.0, 120.0, 100.0, 100.0, 100.0, 100.0, 1.8]),
        ("Default fast", [3.0, 14.0, 110.0, 0.0, 0.0, 6.0, 150.0, 125.0, 170.0, 180.0, 100.0, 100.0, 2.2]),
        ("Default deep", [2.0, 16.0, 120.0, 0.0, 10.0, 9.0, 160.0, 150.0, 80.0, 90.0, 85.0, 110.0, 2.0]),
        ("Default bright", [2.0, 10.0, 100.0, 0.0, 0.0, 7.0, 170.0, 130.0, 100.0, 100.0, 100.0, 100.0, 1.8]),
        ("Default soft", [2.0, 8.0, 90.0, 0.0, 5.0, 11.0, 125.0, 110.0, 80.0, 90.0, 100.0, 90.0, 1.6]),
        ("Default drift", [2.0, 12.0, 110.0, 10.0, 0.0, 8.0, 150.0, 120.0, 130.0, 140.0, 95.0, 105.0, 2.0]),
        ("Club", [3.0, 32.0, 130.0, 10.0, 0.0, 5.0, 195.0, 155.0, 180.0, 220.0, 100.0, 120.0, 2.8]),
        ("Club slow", [3.0, 28.0, 120.0, 8.0, 0.0, 5.0, 190.0, 150.0, 150.0, 190.0, 100.0, 115.0, 2.5]),
        ("Club wide", [4.0, 40.0, 200.0, -15.0, 30.0, 4.0, 210.0, 165.0, 170.0, 210.0, 100.0, 125.0, 3.2]),
        ("Club glow", [3.0, 30.0, 120.0, 0.0, 10.0, 8.0, 220.0, 150.0, 150.0, 200.0, 100.0, 130.0, 2.6]),
        ("Club smooth", [2.0, 24.0, 110.0, 5.0, 0.0, 9.0, 185.0, 140.0, 140.0, 180.0, 100.0, 115.0, 2.2]),
        ("Club pulse", [3.0, 34.0, 140.0, 0.0, 0.0, 5.0, 205.0, 160.0, 120.0, 270.0, 100.0, 125.0, 2.8]),
        ("Ocean", [3.0, 25.0, 140.0, -20.0, 20.0, 12.0, 160.0, 115.0, 70.0, 60.0, 90.0, 90.0, 2.0]),
        ("Ocean deep", [3.0, 30.0, 150.0, -25.0, 25.0, 14.0, 170.0, 125.0, 55.0, 50.0, 80.0, 95.0, 2.2]),
        ("Ocean calm", [2.0, 18.0, 120.0, -10.0, 15.0, 16.0, 150.0, 110.0, 45.0, 40.0, 90.0, 85.0, 1.8]),
        ("Ocean rolling", [4.0, 32.0, 160.0, -30.0, 30.0, 10.0, 175.0, 120.0, 90.0, 80.0, 90.0, 95.0, 2.4]),
        ("Ocean tide", [2.0, 22.0, 130.0, -15.0, 40.0, 13.0, 155.0, 112.0, 60.0, 70.0, 90.0, 88.0, 2.0]),
        ("Ocean storm", [5.0, 40.0, 180.0, -35.0, 20.0, 8.0, 190.0, 140.0, 120.0, 130.0, 85.0, 100.0, 2.8]),
    ];

    /// Whether every slider sits exactly on preset `index`.
    pub fn swirl_preset_active(&self, index: usize) -> bool {
        Self::SWIRL_PRESETS.get(index).is_some_and(|(_, values)| {
            values
                .iter()
                .enumerate()
                .all(|(slot, value)| (self.swirl_tune_value(slot) - value).abs() < 0.01)
        })
    }

    pub fn swirl_art_edge_value(&self) -> f32 {
        if self.swirl_art_edge.is_finite() {
            self.swirl_art_edge.clamp(0.0, 100.0)
        } else {
            60.0
        }
    }

    pub fn swirl_tune_value(&self, index: usize) -> f32 {
        let (_, low, high, default) = Self::SWIRL_TUNE[index];
        self.swirl_tune
            .get(index)
            .copied()
            .filter(|v| v.is_finite())
            .map_or(default, |v| v.clamp(low, high))
    }

    /// The part of the swirl that is baked into its texture.
    pub fn swirl_look(&self) -> crate::swirl_art::Look {
        let mut v = [0i16; 8];
        for (i, slot) in v.iter_mut().enumerate() {
            *slot = self.swirl_tune_value(i).round() as i16;
        }
        crate::swirl_art::Look {
            v,
            art: self.swirl_art_scroll,
            mirror: !self.swirl_art_single,
            edge: self.swirl_art_edge_value().round() as i16,
        }
    }

    /// The settings that are about how Chanceify looks and which keys it
    /// uses, and nothing about this computer or this account. These are what
    /// a settings file carries to a friend.
    pub const SHARED_KEYS: &'static [&'static str] = &[
        "accent_from_art", "art_expanded", "controls_scale", "custom_eqs", "custom_scenes",
        "custom_theme", "eq_bands_db", "eq_base", "eq_on", "eq_preamp_db",
        "hidden_presets", "key_bindings", "keymap_version", "library_details",
        "library_grid_columns", "library_grid_names", "library_name_size", "library_shelf",
        "library_sort", "lyrics_align", "lyrics_flags", "side_lyrics_flags", "side_lyrics_size", "lyrics_width", "player_bar_vis",
        "player_bar_vis_backdrop", "player_bar_vis_backdrop_flow", "player_bar_vis_bar_height",
        "player_bar_vis_bars", "player_bar_vis_color", "player_bar_vis_colour_pick",
        "player_bar_vis_flow_depth", "player_bar_vis_flow_offset", "player_bar_vis_flow_sheets",
        "player_bar_vis_flow_speed", "player_bar_vis_gap", "player_bar_vis_hex",
        "player_bar_vis_rise", "player_bar_vis_scale", "queue_compact", "queue_show_artist",
        "queue_show_cover", "queue_show_icons", "queue_show_length", "queue_show_numbers", "queue_show_time",
        "queue_width", "float_controls", "float_big", "float_lyrics", "float_hide_bar", "calm_blur", "recents_show_artist", "recents_show_cover", "recents_show_icons", "recents_show_numbers", "vis_title_layout", "jump_shake", "mini_queue", "mini_volume", "mini_vis", "queue_click", "vis_reverse", "missing_mark_big", "row_order", "seek_anchor", "seek_custom_width", "seek_time_joined", "seek_time_mode",
        "seek_width", "show_shortcut_hints", "sidebar_compact", "sidebar_grid", "sidebar_order",
        "sidebar_width", "swirl_art_scroll", "swirl_art_single", "swirl_art_edge", "swirl_colours_fixed", "swirl_drift", "swirl_peaks",
        "swirl_scale", "swirl_tune", "swirl_warp", "swirl_waves", "theme", "theme_from_cover", "custom_bg", "custom_accent",
        "track_columns", "tracklist_compact", "vis", "vis_bar_sides", "vis_shapes",
        "vis_shapes_last", "vis_shapes_set", "volume_presets", "zoom",
        "volume_custom_width", "vis_no_gradient", "vis_lyrics", "vis_text_still", "vis_sway", "vis_sway_amount", "vis_text_no_outline", "vis_text_back", "vis_back_alpha", "vis_back_round", "vis_back_pad", "vis_back_block", "vis_live_preview", "vis_font_random", "vis_text_font", "vis_text_no_artist", "vis_lyrics_no_back", "vis_bars_stay", "lyrics_vis", "lyrics_vis_dark", "lyrics_vis_mode", "window_opacity", "anchor_controls", "anchor_volume", "nudge_controls", "nudge_volume", "nudge_seek", "bar_stacked", "bar_layout", "bar_free", "bar_free_pos", "bar_free_seek", "lyrics_ctl_off", "vis_bars_opacity", "vis_flow_opacity", "swirl_react_mode", "vis_jump_react", "vis_jump_on",
    ];

    /// The shareable part of the settings, as the text of a file.
    pub fn shared_json(&self) -> Result<String, String> {
        let whole = serde_json::to_value(self).map_err(|error| error.to_string())?;
        let mut out = serde_json::Map::new();
        out.insert("chanceify_settings".into(), serde_json::Value::from(1));
        for key in Self::SHARED_KEYS {
            if let Some(value) = whole.get(*key) {
                out.insert((*key).to_string(), value.clone());
            }
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(out))
            .map_err(|error| error.to_string())
    }

    /// Takes the shareable settings out of a file's text and puts them over
    /// these. Anything not on the list stays as it is. Returns how many were
    /// taken.
    pub fn apply_shared(&mut self, text: &str) -> Result<usize, String> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let shared: serde_json::Value =
            serde_json::from_str(text).map_err(|error| error.to_string())?;
        if shared.get("chanceify_settings").is_none() {
            return Err("this is not a chanceify™ settings file".into());
        }
        let mut whole = serde_json::to_value(&*self).map_err(|error| error.to_string())?;
        let mut taken = 0;
        for key in Self::SHARED_KEYS {
            if let Some(value) = shared.get(*key) {
                whole[*key] = value.clone();
                taken += 1;
            }
        }
        let mut merged: Settings =
            serde_json::from_value(whole).map_err(|error| error.to_string())?;
        // Fields that are never written to a file are kept from here.
        merged.proxy = std::mem::take(&mut self.proxy);
        merged.proxy_password = std::mem::take(&mut self.proxy_password);
        merged.proxy_password_legacy = self.proxy_password_legacy;
        *self = merged;
        Ok(taken)
    }

    pub const LYRICS_HIDE_ART: u8 = 1;
    pub const LYRICS_HIDE_STAMPS: u8 = 2;
    pub const LYRICS_COUNTDOWN: u8 = 4;
    /// The big cover drifts about by itself, like a floating card.
    pub const LYRICS_FLOAT_ART: u8 = 8;
    /// The big cover swells with the music's bass.
    pub const LYRICS_BOUNCE_ART: u8 = 16;
    /// The artist's name is left off the lyrics page; the song's is bigger.
    pub const LYRICS_HIDE_ARTIST: u8 = 32;

    pub fn lyrics_align_value(&self) -> u8 {
        // The old plain Centre (1) became the middle focus mode.
        // Focus has one mode now: the current line in the middle.
        match self.lyrics_align {
            1 | 3 | 5 => 4,
            other => other.min(5),
        }
    }

    pub fn side_lyrics_flag(&self, bit: u8) -> bool {
        self.side_lyrics_flags & bit != 0
    }

    /// How much the small panel's lines are scaled.
    pub fn side_lyrics_scale(&self) -> f32 {
        match self.side_lyrics_size {
            1 => 0.85,
            2 => 1.2,
            _ => 1.0,
        }
    }

    pub fn lyrics_flag(&self, bit: u8) -> bool {
        self.lyrics_flags & bit != 0
    }

    pub fn swirl_waves_value(&self) -> u8 {
        if self.swirl_waves == 0 { 2 } else { self.swirl_waves.clamp(1, 8) }
    }

    pub fn swirl_peaks_value(&self) -> u8 {
        if self.swirl_peaks == 0 { 30 } else { self.swirl_peaks.clamp(1, 100) }
    }

    /// Seconds a colour pair takes to give way to the next.
    pub fn swirl_drift_secs(&self) -> f64 {
        let speed = if self.swirl_drift.is_finite() && self.swirl_drift > 0.0 {
            self.swirl_drift.clamp(0.2, 4.0)
        } else {
            1.0
        };
        9.0 / f64::from(speed)
    }

    /// How bent the swirl is.
    pub fn swirl_warp_value(&self) -> f32 {
        if self.swirl_warp.is_finite() && self.swirl_warp > 0.0 {
            self.swirl_warp.clamp(0.0, 1.0)
        } else if self.swirl_warp < 0.0 {
            0.0
        } else {
            0.45
        }
    }

    /// The volume shortcut buttons, at most four.
    pub fn volume_presets_value(&self) -> Vec<u8> {
        match &self.volume_presets {
            Some(list) => list.iter().map(|v| (*v).min(100)).take(4).collect(),
            None => vec![50, 100],
        }
    }

    /// The play controls' size multiplier.
    /// The song-time layout: 0 both ends, 1 joined before the bar, 2 joined
    /// after it, 3 off. A file saved before there were four still says
    /// "joined", which means 1.
    pub fn seek_time_mode_value(&self) -> u8 {
        if self.seek_time_mode == 0 && self.seek_time_joined {
            1
        } else {
            self.seek_time_mode.min(3)
        }
    }

    pub fn controls_scale_value(&self) -> f32 {
        if self.controls_scale.is_finite() && self.controls_scale > 0.0 {
            self.controls_scale.clamp(0.7, 1.8)
        } else {
            1.0
        }
    }

    pub fn player_bar_vis_gap(&self) -> f32 {
        if self.player_bar_vis_gap.is_finite() {
            self.player_bar_vis_gap.clamp(0.0, Self::VIS_GAP_MAX)
        } else {
            crate::ui::player_bar::SPECTRUM_GAP
        }
    }

    pub(crate) fn cached_palette(&self) -> Option<crate::theme::Palette> {
        let theme = if self.custom_theme.is_some() {
            self.custom_theme_cache.as_ref()
        } else if self.theme == ThemeChoice::System {
            self.system_theme_cache.as_ref()
        } else {
            None
        };
        theme.map(|theme| theme.palette)
    }

    pub(crate) fn proxy_password_record(
        &self,
    ) -> Result<Option<crate::credentials::ProxyPassword>, String> {
        if self.proxy_password.is_empty() {
            return Ok(None);
        }
        let manual = ManualProxy::parse(
            ManualKind::Http,
            &self.proxy_host,
            &self.proxy_port,
            &self.proxy_username,
            &self.proxy_password,
        )?;
        Ok(manual.password_record())
    }

    pub(crate) fn restore_proxy_password(
        &mut self,
        saved: &crate::credentials::ProxyPassword,
    ) -> bool {
        let Ok(mut manual) = ManualProxy::parse(
            ManualKind::Http,
            &self.proxy_host,
            &self.proxy_port,
            &self.proxy_username,
            "",
        ) else {
            return false;
        };
        if !manual.restore_password(saved) {
            return false;
        }
        self.proxy_password = manual.password;
        true
    }

    pub fn proxy_preferences(&self) -> ProxyPreferences {
        ProxyPreferences {
            mode: self.proxy_mode,
            host: self.proxy_host.clone(),
            port: self.proxy_port.clone(),
            username: self.proxy_username.clone(),
        }
    }

    /// Whether a Library row shows name text at all. Turning names off is
    /// a grid setting: stacked rows have the cover beside the name, so
    /// dropping the name would leave a row with nothing in it.
    pub fn library_names_shown(&self) -> bool {
        !self.sidebar_grid || self.library_grid_names
    }

    /// Whether this row in particular is allowed its name. The account's
    /// own playlists can go unnamed, so their covers carry them alone; a
    /// playlist saved from someone else, or a saved album, always keeps
    /// Whether a row's second line — "Playlist • chan", "Album • Artist" —
    /// is drawn under its name.
    ///
    /// The name is never hidden. This began as "name my own playlists",
    /// which hid the *name* and left those rows reading only "Playlist" —
    /// a row that cannot be told from any other. What it should have been
    /// hiding all along is the detail line, which is what the reader asked
    /// for: one clean line per playlist.
    pub fn library_details_entry(&self) -> bool {
        self.library_details
    }

    /// Steps the number of bars along by `delta`, stopping at either end.
    ///
    /// The step grows with the count, so one press moves by about an
    /// eighth of where it is: a few presses to leave the coarse readings,
    /// and the same two or three to cross the last stretch towards 256.
    /// A fixed step would make the low end unusable and the high end
    /// endless.
    pub fn step_player_bar_vis_bars(&mut self, delta: i32) {
        let current = self.player_bar_vis_bar_count() as i32;
        let step = (current / 8).max(1);
        self.player_bar_vis_bars = (current + delta.signum() * step)
            .clamp(Self::VIS_BARS_MIN as i32, Self::VIS_BARS_MAX as i32)
            as u16;
    }

    /// Steps the gap along by `delta` pixels, stopping at either end.
    pub fn step_player_bar_vis_gap(&mut self, delta: i32) {
        self.player_bar_vis_gap =
            (self.player_bar_vis_gap() + delta as f32).clamp(0.0, Self::VIS_GAP_MAX);
    }

    /// The fewest and most sheets the flow will fold together. One sheet is
    /// a single moving ridge and twelve is a thick stack; either is drawn
    /// the same way, so the bounds are simply where it stops being one
    /// thing or the other.
    pub const VIS_FLOW_SHEETS: std::ops::RangeInclusive<u8> = 1..=12;
    /// The shallowest and tallest the flow's stack may be, as a fraction of
    /// the bar's height.
    pub const VIS_FLOW_DEPTH: std::ops::RangeInclusive<f32> = 0.2..=1.2;
    /// The slowest and quickest the flow may drift, as a multiplier.
    pub const VIS_FLOW_SPEED: std::ops::RangeInclusive<f32> = 0.2..=8.0;
    /// How far one flow sheet may sit from the one in front of it.
    pub const VIS_FLOW_OFFSET: std::ops::RangeInclusive<f32> = 0.0..=0.12;

    /// How many sheets to fold, within what is worth folding.
    pub fn player_bar_vis_flow_sheets(&self) -> u8 {
        self.player_bar_vis_flow_sheets
            .clamp(*Self::VIS_FLOW_SHEETS.start(), *Self::VIS_FLOW_SHEETS.end())
    }

    /// How much of the bar's height the stack covers. A hand-edited file
    /// can hold anything, and a stack taller than the bar draws nothing
    /// useful, so it is clamped rather than trusted.
    pub fn player_bar_vis_flow_depth(&self) -> f32 {
        if self.player_bar_vis_flow_depth.is_finite() {
            self.player_bar_vis_flow_depth
                .clamp(*Self::VIS_FLOW_DEPTH.start(), *Self::VIS_FLOW_DEPTH.end())
        } else {
            0.86
        }
    }

    /// How tall the visualizer is drawn, as a multiple of the player bar's
    /// own height. A file cannot make it vanish or fill the screen.
    pub fn player_bar_vis_scale(&self) -> f32 {
        if self.player_bar_vis_scale.is_finite() {
            self.player_bar_vis_scale.clamp(1.0, 6.0)
        } else {
            1.0
        }
    }

    /// How far up the window the visualizer reaches, as a multiple of the
    /// player bar's height. Zero keeps it inside the bar.
    pub const VIS_RISE_MIN: f32 = 0.0;

    pub fn player_bar_vis_rise(&self) -> f32 {
        if self.player_bar_vis_rise.is_finite() {
            self.player_bar_vis_rise.clamp(Self::VIS_RISE_MIN, 6.0)
        } else {
            0.0
        }
    }

    /// A multiplier on the drift speed, so a file cannot make the flow
    /// crawl or strobe.
    pub fn player_bar_vis_flow_speed(&self) -> f32 {
        if self.player_bar_vis_flow_speed.is_finite() {
            self.player_bar_vis_flow_speed
                .clamp(*Self::VIS_FLOW_SPEED.start(), *Self::VIS_FLOW_SPEED.end())
        } else {
            1.0
        }
    }

    /// How far one flow sheet is pushed along the spectrum from the one in
    /// front of it, as a fraction of the bar's readings.
    pub fn player_bar_vis_flow_offset(&self) -> f32 {
        if self.player_bar_vis_flow_offset.is_finite() {
            self.player_bar_vis_flow_offset
                .clamp(*Self::VIS_FLOW_OFFSET.start(), *Self::VIS_FLOW_OFFSET.end())
        } else {
            0.0
        }
    }

    pub fn library_pins(&self) -> Vec<String> {
        let mut pins = self.pinned_contexts.clone();
        if !self.liked_songs_pinned {
            pins.retain(|key| key != LIKED_SONGS_KEY);
        } else if !pins.iter().any(|key| key == LIKED_SONGS_KEY) {
            pins.insert(0, LIKED_SONGS_KEY.into());
        }
        pins
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                // A file edited in Notepad or Windows PowerShell can start with
                // a byte order mark, which JSON does not allow.
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                let mut settings = serde_json::from_str(text).unwrap_or_else(|error| {
                    log::warn!("settings at {} are unreadable: {error}", path.display());
                    Self::default()
                });
                settings.migrate_proxy(text);
                // Always the same name in Spotify Connect, whatever an older
                // build saved.
                settings.device_name = "chanceify".into();
                for key in settings
                    .pinned_contexts
                    .iter_mut()
                    .chain(settings.sidebar_order.iter_mut())
                {
                    if key == "fastpotify:liked-songs" {
                        *key = LIKED_SONGS_KEY.into();
                    }
                }
                settings.proxy_password_legacy = !settings.proxy_password.is_empty();
                settings.with_shipped_spotify_app()
            }
            Err(_) => Self::default().with_shipped_spotify_app(),
        }
    }

    /// A copy of Chanceify can ship with `spotify_client_id.txt` beside the
    /// program, holding the Client ID of Chance's own Spotify app (a Client
    /// ID is public, not a secret). Sign-in then goes through that app, which
    /// is named Chanceify and has its own request allowance, instead of the
    /// shared one. A Client ID the listener set themselves is never replaced.
    fn with_shipped_spotify_app(mut self) -> Self {
        if self.web_client_id.as_deref().is_none_or(|id| id.trim().is_empty())
            && let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
            && let Ok(text) = std::fs::read_to_string(dir.join("spotify_client_id.txt"))
        {
            let id = text.trim().trim_start_matches('\u{feff}').trim();
            if id.len() == 32 && id.chars().all(|c| c.is_ascii_hexdigit()) {
                self.web_client_id = Some(id.to_string());
            }
        }
        self
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = match self
            .encode_for_profile(path == crate::paths::AppDirs::legacy().settings_file())
        {
            Ok(text) => text,
            Err(error) => {
                log::warn!("unable to encode settings: {error}");
                return;
            }
        };
        let temporary = path.with_extension("json.tmp");
        let written = std::fs::write(&temporary, text)
            .and_then(|()| crate::util::replace_file(&temporary, path));
        if let Err(error) = written {
            log::warn!("unable to save settings to {}: {error}", path.display());
        }
    }

    fn encode_for_profile(&self, legacy: bool) -> serde_json::Result<String> {
        if !legacy {
            return serde_json::to_string_pretty(self);
        }
        // A trial launch can still roll back to a client that only recognizes
        // the previous local key. Keep its on-disk spelling until migration.
        let mut saved = self.clone();
        for key in saved
            .pinned_contexts
            .iter_mut()
            .chain(saved.sidebar_order.iter_mut())
        {
            if key == LIKED_SONGS_KEY {
                *key = "fastpotify:liked-songs".into();
            }
        }
        serde_json::to_string_pretty(&saved)
    }

    pub fn platform_backend(&self) -> Option<String> {
        self.audio_backend.clone().or_else(|| {
            if cfg!(target_os = "linux") {
                Some("pulseaudio".to_string())
            } else {
                None
            }
        })
    }

    pub fn remember_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|entry| entry != query);
        self.search_history.insert(0, query.to_string());
        self.search_history.truncate(12);
    }

    pub(crate) fn migrate_proxy(&mut self, text: &str) {
        if !text.contains("\"proxy_mode\"") {
            self.proxy_mode = infer_legacy_proxy_mode(&self.proxy);
        }
        if self.proxy_host.is_empty() && !self.proxy.trim().is_empty() {
            let (host, port) = split_legacy_address(&self.proxy);
            self.proxy_host = host;
            if self.proxy_port.is_empty() {
                self.proxy_port = port;
            }
        }
    }

    /// The proxy the HTTP client should use. Off and System always succeed.
    /// HTTP and SOCKS5 need a host and port.
    pub fn proxy_config(&self) -> Result<ProxyConfig, String> {
        match self.proxy_mode {
            ProxyMode::Off => Ok(ProxyConfig::Off),
            ProxyMode::System => Ok(ProxyConfig::System),
            ProxyMode::Http => Ok(ProxyConfig::Http(ManualProxy::parse(
                ManualKind::Http,
                &self.proxy_host,
                &self.proxy_port,
                &self.proxy_username,
                &self.proxy_password,
            )?)),
            ProxyMode::Socks => Ok(ProxyConfig::Socks(ManualProxy::parse(
                ManualKind::Socks,
                &self.proxy_host,
                &self.proxy_port,
                &self.proxy_username,
                &self.proxy_password,
            )?)),
        }
    }
}

fn infer_legacy_proxy_mode(proxy: &str) -> ProxyMode {
    let raw = proxy.trim();
    if raw.is_empty() {
        return ProxyMode::System;
    }
    let scheme = raw.split("://").next().unwrap_or("").to_ascii_lowercase();
    match scheme.as_str() {
        "socks" | "socks5" | "socks5h" => ProxyMode::Socks,
        _ => ProxyMode::Http,
    }
}

fn validate_host(host: &str) -> Result<String, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("enter a host".into());
    }
    let host = host
        .split_once("://")
        .map_or(host, |(_, rest)| rest)
        .trim_start_matches('[')
        .trim_end_matches(']');
    if host.is_empty() {
        return Err("enter a host".into());
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(host.to_string());
    }
    if is_hostname(host) {
        return Ok(host.to_string());
    }
    Err("the host must be a hostname or IP address".into())
}

fn is_hostname(host: &str) -> bool {
    if host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(|label| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn validate_port(port: &str) -> Result<u16, String> {
    let port = port.trim();
    if port.is_empty() {
        return Err("enter a port".into());
    }
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("the port must be a number between 1 and 65535".into());
    }
    match port.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err("the port must be a number between 1 and 65535".into()),
    }
}

fn split_legacy_address(raw: &str) -> (String, String) {
    let raw = raw.trim();
    let rest = raw.split_once("://").map_or(raw, |(_, rest)| rest);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
    if let Some(inner) = rest.strip_prefix('[')
        && let Some((host, port)) = inner.split_once("]:")
    {
        return (host.to_string(), port.trim().to_string());
    }
    if let Some((host, port)) = rest.rsplit_once(':')
        && !host.contains(':')
    {
        return (host.to_string(), port.trim().to_string());
    }
    (rest.to_string(), String::new())
}

/// The resolved proxy policy used by the HTTP client and local playback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProxyConfig {
    /// Invalid saved settings block requests until corrected in the interface.
    Invalid(String),
    Off,
    #[default]
    System,
    Http(ManualProxy),
    Socks(ManualProxy),
}

impl ProxyConfig {
    pub(crate) fn password_record(&self) -> Option<crate::credentials::ProxyPassword> {
        match self {
            Self::Http(manual) | Self::Socks(manual) => manual.password_record(),
            _ => None,
        }
    }

    pub(crate) fn restore_password(&mut self, saved: &crate::credentials::ProxyPassword) -> bool {
        match self {
            Self::Http(manual) | Self::Socks(manual) => manual.restore_password(saved),
            _ => false,
        }
    }

    /// Librespot's CONNECT client only understands unauthenticated,
    /// plaintext HTTP proxies. Never give it a credential-bearing URL: the
    /// upstream client logs that URL at info level and does not send proxy
    /// authentication in either of its HTTP paths.
    pub fn librespot_url(&self) -> Option<reqwest::Url> {
        match self {
            Self::Http(manual) => manual.librespot_url(),
            Self::System => system_http_proxy(),
            Self::Invalid(_) | Self::Off | Self::Socks(_) => None,
        }
    }
}

/// The HTTP proxy System mode would hand librespot, if it can resolve one.
/// SOCKS system proxies are ignored: the engine cannot use them.
fn system_http_proxy() -> Option<reqwest::Url> {
    let matcher = hyper_util::client::proxy::matcher::Matcher::from_system();
    let dest = http::Uri::from_static("https://apresolve.spotify.com");
    let intercept = matcher.intercept(&dest)?;
    librespot_system_proxy(&intercept)
}

fn librespot_system_proxy(
    intercept: &hyper_util::client::proxy::matcher::Intercept,
) -> Option<reqwest::Url> {
    if intercept.basic_auth().is_some() || intercept.raw_auth().is_some() {
        return None;
    }
    http_proxy_from_uri(intercept.uri())
}

fn http_proxy_from_uri(uri: &http::Uri) -> Option<reqwest::Url> {
    match uri.scheme_str() {
        Some("http") => reqwest::Url::parse(&uri.to_string()).ok(),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum ManualKind {
    Http,
    Socks,
}

/// A configured HTTP or SOCKS5 endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct ManualProxy {
    endpoint: reqwest::Url,
    username: String,
    password: String,
}

impl std::fmt::Debug for ManualProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManualProxy")
            .field("url", &self.redacted())
            .finish()
    }
}

impl ManualProxy {
    fn password_record(&self) -> Option<crate::credentials::ProxyPassword> {
        if self.password.is_empty() {
            return None;
        }
        Some(crate::credentials::ProxyPassword {
            host: self.endpoint.host_str()?.to_string(),
            port: self.endpoint.port()?,
            username: self.username.clone(),
            password: self.password.clone(),
        })
    }

    fn restore_password(&mut self, saved: &crate::credentials::ProxyPassword) -> bool {
        if self.endpoint.host_str() != Some(saved.host.as_str())
            || self.endpoint.port() != Some(saved.port)
            || self.username != saved.username
        {
            return false;
        }
        self.password.clone_from(&saved.password);
        true
    }

    fn parse(
        kind: ManualKind,
        host: &str,
        port: &str,
        username: &str,
        password: &str,
    ) -> Result<Self, String> {
        let host = validate_host(host)?;
        let port = validate_port(port)?;
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let scheme = match kind {
            ManualKind::Http => "http",
            // Resolve Spotify hostnames at the proxy, so SOCKS mode does not
            // leak DNS queries or fail behind a proxy-only network.
            ManualKind::Socks => "socks5h",
        };
        let endpoint = reqwest::Url::parse(&format!("{scheme}://{host}:{port}"))
            .map_err(|error| format!("not a proxy address: {error}"))?;
        Ok(Self {
            endpoint,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    /// A reqwest proxy with credentials applied separately so build errors
    /// cannot echo a password.
    pub fn reqwest_proxy(&self) -> Result<reqwest::Proxy, reqwest::Error> {
        let mut proxy = reqwest::Proxy::all(self.endpoint.clone())?;
        if !self.username.is_empty() || !self.password.is_empty() {
            proxy = proxy.basic_auth(&self.username, &self.password);
        }
        Ok(proxy)
    }

    fn librespot_url(&self) -> Option<reqwest::Url> {
        (self.username.is_empty() && self.password.is_empty()).then(|| self.endpoint.clone())
    }

    /// The credential-free endpoint, for logs and the debugger.
    pub fn redacted(&self) -> reqwest::Url {
        self.endpoint.clone()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_bar_layout_follows_the_old_stack_switch_and_stays_in_range() {
        let mut settings = super::Settings::default();
        assert_eq!(settings.bar_layout_value(), 0);
        settings.bar_stacked = true;
        assert_eq!(settings.bar_layout_value(), 1);
        settings.bar_stacked = false;
        settings.bar_layout = 9;
        assert_eq!(settings.bar_layout_value(), 3);
    }

    #[test]
    fn the_sway_and_window_opacity_have_safe_defaults() {
        let mut settings = super::Settings::default();
        assert_eq!(settings.vis_sway_preset().name, "Grass");
        assert!((settings.vis_sway_strength() - 1.0).abs() < 1e-6);
        settings.vis_sway = 200;
        assert_eq!(settings.vis_sway_preset().name, "Drunk");
        assert!((settings.window_solidity() - 1.0).abs() < 1e-6);
        assert!(!settings.window_see_through());
        settings.window_opacity = 0.01;
        assert!((settings.window_solidity() - 0.3).abs() < 1e-6);
        assert!(settings.window_see_through());
    }

    #[test]
    fn the_lyrics_visualizer_is_off_and_dim_by_default_and_clamped() {
        let mut settings = super::Settings::default();
        assert!(!settings.lyrics_vis);
        assert!((settings.lyrics_vis_darkness() - 0.6).abs() < 1e-6);
        settings.lyrics_vis_dark = 5.0;
        assert!((settings.lyrics_vis_darkness() - 0.95).abs() < 1e-6);
        settings.lyrics_vis_dark = 0.01;
        assert!((settings.lyrics_vis_darkness() - 0.1).abs() < 1e-6);
        let old: super::Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.lyrics_vis);
    }

    #[test]
    fn trial_launch_writes_keys_the_previous_release_can_still_read() {
        let settings = super::Settings {
            pinned_contexts: vec![super::LIKED_SONGS_KEY.into(), "spotify:playlist:one".into()],
            sidebar_order: vec![super::LIKED_SONGS_KEY.into()],
            ..Default::default()
        };
        for (legacy, key) in [
            (true, "fastpotify:liked-songs"),
            (false, super::LIKED_SONGS_KEY),
        ] {
            let saved: super::Settings =
                serde_json::from_str(&settings.encode_for_profile(legacy).unwrap()).unwrap();
            assert_eq!(saved.pinned_contexts, [key, "spotify:playlist:one"]);
            assert_eq!(saved.sidebar_order, [key]);
        }
        assert_eq!(settings.pinned_contexts[0], super::LIKED_SONGS_KEY);
    }
    use super::Settings;

    #[test]
    fn new_profiles_follow_the_system_and_saved_choices_are_preserved() {
        use super::ThemeChoice;
        assert_eq!(Settings::default().theme, ThemeChoice::System);
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.theme, ThemeChoice::System);
        for (json, choice) in [("dark", ThemeChoice::Dark), ("light", ThemeChoice::Light)] {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"theme": json, "volume": 37})).unwrap();
            assert_eq!(settings.theme, choice);
            assert_eq!(settings.volume, 37);
        }
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "theme": "dark", "system_theme_cache": {"broken": true}, "volume": 37
        }))
        .unwrap();
        assert!(settings.system_theme_cache.is_none());
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.volume, 37);
    }

    #[test]
    fn language_follows_the_system_unless_a_known_locale_was_chosen() {
        use super::LanguageChoice;
        use crate::i18n::Locale;
        assert_eq!(Settings::default().language, LanguageChoice::System);
        let older: Settings = serde_json::from_value(serde_json::json!({"volume": 37})).unwrap();
        assert_eq!(older.language, LanguageChoice::System);
        assert_eq!(older.volume, 37);
        for (json, choice) in [
            (serde_json::json!("system"), LanguageChoice::System),
            (
                serde_json::json!("es"),
                LanguageChoice::Locale(Locale::Spanish),
            ),
            (
                serde_json::json!("pt-BR"),
                LanguageChoice::Locale(Locale::PortugueseBrazil),
            ),
            (
                serde_json::json!("zh-Hant"),
                LanguageChoice::Locale(Locale::ChineseTraditional),
            ),
            (
                serde_json::json!("de"),
                LanguageChoice::Locale(Locale::German),
            ),
            // A language a later version added, or a hand-edited typo, keeps
            // the rest of the file and follows the system.
            (serde_json::json!("tlh"), LanguageChoice::System),
            (serde_json::json!(7), LanguageChoice::System),
            (serde_json::Value::Null, LanguageChoice::System),
        ] {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"language": json, "volume": 37}))
                    .unwrap();
            assert_eq!(settings.language, choice, "{json}");
            assert_eq!(settings.volume, 37);
        }
        for &locale in crate::i18n::LOCALES {
            let settings = Settings {
                language: LanguageChoice::Locale(locale),
                ..Default::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            assert!(text.contains(&format!("\"language\":\"{}\"", locale.tag())));
            let saved: Settings = serde_json::from_str(&text).unwrap();
            assert_eq!(saved.language, settings.language);
        }
        let text = serde_json::to_string(&Settings::default()).unwrap();
        assert!(text.contains("\"language\":\"system\""));
        assert_eq!(
            LanguageChoice::System.resolve(),
            Locale::English,
            "tests pin English"
        );
    }

    #[test]
    fn custom_theme_cache_round_trips_and_a_bad_cache_keeps_other_settings() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.custom_theme.is_none() && settings.custom_theme_cache.is_none());
        settings.custom_theme = Some("gruvbox.json".into());
        let mut palette = crate::theme::Palette::light();
        palette.shadow = egui::Color32::from_rgba_unmultiplied(37, 128, 249, 117);
        settings.custom_theme_cache = Some(crate::theme::CustomTheme {
            filename: "gruvbox.json".into(),
            palette,
        });
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<Settings>(&encoded).unwrap(),
            settings
        );
        let mut damaged = serde_json::to_value(&settings).unwrap();
        damaged["custom_theme_cache"] = serde_json::json!({"palette": "broken"});
        damaged["audio_cache_mb"] = 777.into();
        let recovered: Settings = serde_json::from_value(damaged).unwrap();
        assert_eq!(recovered.custom_theme.as_deref(), Some("gruvbox.json"));
        assert_eq!(recovered.audio_cache_mb, 777);
        assert!(recovered.custom_theme_cache.is_none());
    }

    #[test]
    fn partial_home_preferences_keep_defaults_and_survive_a_settings_round_trip() {
        for (home, made_for_you, recommendations) in [
            ("{}", true, true),
            (r#"{"made_for_you":{"visible":false}}"#, false, true),
            (r#"{"recommendations":{"visible":false}}"#, true, false),
            (
                r#"{"made_for_you":{"visible":false},"recommendations":{"visible":false}}"#,
                false,
                false,
            ),
        ] {
            let settings: Settings =
                serde_json::from_str(&format!(r#"{{"volume":12345,"home":{home}}}"#)).unwrap();
            assert_eq!(settings.home.made_for_you.visible, made_for_you);
            assert_eq!(settings.home.recommendations.visible, recommendations);
            assert_eq!(settings.volume, 12345);
            let restored: Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(restored, settings);
        }
        let old: Settings = serde_json::from_str(r#"{"volume":12345}"#).unwrap();
        assert!(old.home.made_for_you.visible);
        assert!(old.home.recommendations.visible);
        assert_eq!(old.volume, 12345);
    }

    /// Notepad and Windows PowerShell can save UTF-8 with a byte order mark.
    /// A file edited that way, as the Home shelves guide asks, is still read
    /// rather than dropped as unreadable and replaced with the defaults.
    #[test]
    fn a_settings_file_saved_with_a_byte_order_mark_keeps_its_preferences() {
        let dir = std::env::temp_dir().join(format!("chanceify-bom-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            "\u{feff}{\"volume\":12345,\"home\":{\"made_for_you\":{\"visible\":false}}}",
        )
        .unwrap();
        let settings = Settings::load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(settings.volume, 12345);
        assert!(!settings.home.made_for_you.visible);
    }

    #[test]
    fn older_settings_keep_the_sidebar_visible() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.sidebar_visible);
    }

    #[test]
    fn older_library_settings_keep_liked_songs_ahead_of_existing_pins() {
        let settings: Settings = serde_json::from_str(
            r#"{"pinned_contexts":["spotify:playlist:one"],"sidebar_order":["spotify:playlist:two"]}"#,
        ).unwrap();
        assert!(settings.liked_songs_pinned);
        assert_eq!(
            settings.library_pins(),
            [super::LIKED_SONGS_KEY, "spotify:playlist:one"]
        );
        assert_eq!(settings.sidebar_order, ["spotify:playlist:two"]);
    }

    /// The player bar's bars: fewer of them is a coarser reading of the same
    /// sound, and the colour falls back to the theme's when a hand-edited file
    /// holds something unreadable.
    #[test]
    fn the_bar_options_survive_a_round_trip_and_survive_nonsense() {
        let mut settings = Settings::default();
        assert_eq!(settings.player_bar_vis_color, super::VisColor::AlbumArt);
        assert_eq!(
            settings.player_bar_vis_bars as usize,
            crate::vis::WIDE_BANDS
        );

        settings.player_bar_vis_color = super::VisColor::Fixed;
        settings.player_bar_vis_hex = "#ff8800".into();
        settings.player_bar_vis_bars = 24;
        settings.player_bar_vis_gap = 5.0;
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.player_bar_vis_color, super::VisColor::Fixed);
        assert_eq!(restored.player_bar_vis_hex, "#ff8800");
        assert_eq!(restored.player_bar_vis_bars, 24);

        let accent = egui::Color32::from_rgb(1, 2, 3);
        assert_eq!(
            restored.player_bar_vis_color_value(accent),
            egui::Color32::from_rgb(0xff, 0x88, 0x00)
        );
        // Nonsense in the file must not leave the bars invisible.
        let mut broken = restored.clone();
        broken.player_bar_vis_hex = "not a colour".into();
        assert_eq!(broken.player_bar_vis_color_value(accent), accent);
        broken.player_bar_vis_bars = 0;
        assert_eq!(broken.player_bar_vis_bar_count(), 4);
        broken.player_bar_vis_bars = 200;
        assert_eq!(broken.player_bar_vis_bar_count(), 200);
        broken.player_bar_vis_gap = f32::NAN;
        assert_eq!(
            broken.player_bar_vis_gap(),
            crate::ui::player_bar::SPECTRUM_GAP
        );
        broken.player_bar_vis_gap = 900.0;
        assert_eq!(broken.player_bar_vis_gap(), 10.0);

        // The steps stop at either end rather than running off it, and
        // grow with the count so the top of the range stays reachable.
        let mut steps = Settings {
            player_bar_vis_bars: 75,
            ..Default::default()
        };
        steps.step_player_bar_vis_bars(1);
        assert_eq!(steps.player_bar_vis_bars, 84);
        for _ in 0..40 {
            steps.step_player_bar_vis_bars(1);
        }
        assert_eq!(steps.player_bar_vis_bars, 256);
        steps.step_player_bar_vis_bars(1);
        assert_eq!(steps.player_bar_vis_bars, 256);
        for _ in 0..40 {
            steps.step_player_bar_vis_bars(-1);
        }
        assert_eq!(steps.player_bar_vis_bars, 4);
        // The gap starts at the default of 2 and moves by whole pixels, stopping
        // at both ends rather than running off them.
        steps.step_player_bar_vis_gap(2);
        assert_eq!(steps.player_bar_vis_gap(), 4.0);
        steps.step_player_bar_vis_gap(-6);
        assert_eq!(steps.player_bar_vis_gap(), 0.0);
        steps.step_player_bar_vis_gap(-1);
        assert_eq!(steps.player_bar_vis_gap(), 0.0);
        steps.step_player_bar_vis_gap(99);
        assert_eq!(steps.player_bar_vis_gap(), 10.0);
    }

    #[test]
    fn older_settings_keep_the_winamp_window_closed_and_the_built_in_skin() {
        let settings: Settings = serde_json::from_str(r#"{"zoom": 1.2}"#).unwrap();
        assert!(!settings.winamp_window);
        assert!(settings.winamp_show_taskbar);
        assert_eq!(settings.skin, None);
        assert_eq!(settings.skin_scale, None);
        assert!(!settings.winamp_on_top);
        assert_eq!(settings.vis, super::VisMode::Bars);
        assert!(!settings.playlist_open);
        assert_eq!(settings.playlist_height, 174);
        assert!(!settings.eq_on);
        assert_eq!(settings.eq_bands_db, [0.0; 10]);
        assert_eq!(settings.balance, 0.0);
        assert!(!settings.mono);
        assert!(!settings.playlist_shaded);
        assert!(!settings.eq_shaded);
        assert!(!settings.winamp_shaded);
    }

    #[test]
    fn the_visualiser_cycles_bars_scope_off() {
        use super::VisMode;
        assert_eq!(VisMode::Bars.next(), VisMode::Scope);
        assert_eq!(VisMode::Scope.next(), VisMode::Off);
        assert_eq!(VisMode::Off.next(), VisMode::Bars);
        let settings: Settings = serde_json::from_str(r#"{"vis": "scope"}"#).unwrap();
        assert_eq!(settings.vis, VisMode::Scope);
    }

    #[test]
    fn a_chosen_skin_round_trips() {
        let settings = Settings {
            winamp_window: true,
            skin: Some("Zaxon.wsz".into()),
            skin_scale: Some(3),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, settings);
    }

    #[test]
    fn hidden_sidebar_round_trips() {
        let settings = Settings {
            sidebar_visible: false,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(!restored.sidebar_visible);
    }

    #[test]
    fn older_settings_default_to_standard_sidebar() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.sidebar_compact);
    }

    #[test]
    fn compact_sidebar_round_trips() {
        let settings = Settings {
            sidebar_compact: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.sidebar_compact);
    }

    #[test]
    fn library_grid_round_trips_and_older_settings_keep_the_list() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.sidebar_grid);

        let settings = Settings {
            sidebar_grid: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.sidebar_grid);
    }

    #[test]
    fn older_settings_default_to_standard_tracklist() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.tracklist_compact);
    }

    #[test]
    fn track_columns_round_trip_and_survive_a_hand_edited_file() {
        use crate::model::SortColumn;
        let mut columns = crate::settings::TrackColumns::default();
        columns.set(SortColumn::Album, 320.0);
        columns.set(SortColumn::AddedBy, 0.0);
        let settings = Settings {
            track_columns: columns,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.track_columns.album, 320.0);
        assert!(!restored.track_columns.shown(SortColumn::AddedBy));
        assert!(restored.track_columns.shown(SortColumn::Added));

        // A file from before this setting existed, and one from a reader
        // who typed nonsense into it.
        let older: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(
            older.track_columns,
            crate::settings::TrackColumns::default()
        );
        let edited = crate::settings::TrackColumns {
            album: f32::NAN,
            added_by: -50.0,
            added: 9000.0,
            bpm: 60.0,
            ..Default::default()
        };
        assert_eq!(edited.width(SortColumn::Album), 0.0);
        assert_eq!(edited.width(SortColumn::AddedBy), 0.0);
        assert_eq!(
            edited.width(SortColumn::Added),
            crate::settings::TrackColumns::MAX
        );
    }

    #[test]
    fn a_column_too_narrow_for_its_heading_is_hidden_rather_than_squeezed() {
        use crate::model::SortColumn;
        let mut columns = crate::settings::TrackColumns::default();
        columns.set(SortColumn::Album, 10.0);
        assert_eq!(columns.width(SortColumn::Album), 0.0);
        columns.set(SortColumn::Album, crate::settings::TrackColumns::MIN);
        assert!(columns.shown(SortColumn::Album));

        columns.toggle(SortColumn::Added);
        assert!(!columns.shown(SortColumn::Added));
        columns.toggle(SortColumn::Added);
        assert_eq!(
            columns.width(SortColumn::Added),
            crate::settings::TrackColumns::default_width(SortColumn::Added),
            "showing it again brings back the width it started at"
        );

        columns.set(SortColumn::Album, 300.0);
        columns.reset();
        assert_eq!(columns, crate::settings::TrackColumns::default());
    }

    #[test]
    fn a_saved_sort_from_the_last_version_still_reads() {
        // The old file held one column and one direction.
        let old = r#"{"column":"Album","ascending":true}"#;
        let sort: crate::model::TableSort = serde_json::from_str(old).unwrap();
        assert_eq!(
            sort,
            crate::model::TableSort::single(crate::model::SortColumn::Album, true)
        );
        let current = r#"{"keys":[{"column":"Album","ascending":true}]}"#;
        assert_eq!(
            serde_json::from_str::<crate::model::TableSort>(current).unwrap(),
            sort
        );
        let none: crate::model::TableSort = serde_json::from_str("{}").unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn compact_tracklist_round_trips() {
        let settings = Settings {
            tracklist_compact: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.tracklist_compact);
    }

    #[test]
    fn the_player_bar_visualizer_is_opt_in_and_round_trips() {
        use super::PlayerBarVis;
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.player_bar_vis, PlayerBarVis::Off);
        for mode in [
            PlayerBarVis::Spectrum,
            PlayerBarVis::Flow,
            PlayerBarVis::Swirl,
            PlayerBarVis::Waveform,
        ] {
            let settings = Settings {
                player_bar_vis: mode,
                ..Settings::default()
            };
            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.player_bar_vis, mode);
        }
        let spectrum: Settings = serde_json::from_str(r#"{"player_bar_vis":"spectrum"}"#).unwrap();
        assert_eq!(spectrum.player_bar_vis, PlayerBarVis::Spectrum);
        assert_eq!(PlayerBarVis::Off.next(), PlayerBarVis::Spectrum);
        assert_eq!(PlayerBarVis::Spectrum.next(), PlayerBarVis::Flow);
        assert_eq!(PlayerBarVis::Flow.next(), PlayerBarVis::Swirl);
        assert_eq!(PlayerBarVis::Swirl.next(), PlayerBarVis::Waveform);
        assert_eq!(PlayerBarVis::Waveform.next(), PlayerBarVis::Off);
    }

    #[test]
    fn older_settings_keep_the_chosen_skin() {
        let settings: Settings = serde_json::from_str(r#"{"skin":"A.wsz"}"#).unwrap();
        assert!(!settings.random_skin);
        assert_eq!(settings.skin.as_deref(), Some("A.wsz"));
    }

    #[test]
    fn middle_click_autoscroll_is_opt_in_and_round_trips() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.middle_click_autoscroll);
        let settings = Settings {
            middle_click_autoscroll: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.middle_click_autoscroll);
    }

    #[test]
    fn older_settings_use_the_system_proxy() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.proxy_mode, super::ProxyMode::System);
        assert!(settings.proxy_host.is_empty());
        assert!(settings.proxy_port.is_empty());
        assert!(settings.proxy_username.is_empty());
        assert!(settings.proxy_password.is_empty());
        assert_eq!(settings.proxy_config().unwrap(), super::ProxyConfig::System);
    }

    #[test]
    fn a_proxy_round_trips_through_settings() {
        let settings = Settings {
            proxy_mode: super::ProxyMode::Socks,
            proxy_host: "127.0.0.1".into(),
            proxy_port: "1080".into(),
            proxy_username: "alice".into(),
            proxy_password: "secret".into(),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.proxy_mode, settings.proxy_mode);
        assert_eq!(restored.proxy_host, settings.proxy_host);
        assert_eq!(restored.proxy_port, settings.proxy_port);
        assert_eq!(restored.proxy_username, settings.proxy_username);
        assert!(restored.proxy_password.is_empty());
        assert!(json.contains("socks"));
        assert!(json.contains("127.0.0.1"));
        assert!(json.contains("1080"));
        assert!(json.contains("alice"));
        assert!(!json.contains("secret"));
        assert!(!json.contains("proxy_password"));
        assert!(!json.contains("\"proxy\":"));
    }

    #[test]
    fn settings_debug_keeps_proxy_credentials_private() {
        let settings = Settings {
            proxy_username: "private-proxy-user".into(),
            proxy_password: "private-proxy-password".into(),
            ..Default::default()
        };
        let debug = format!("{settings:?}");
        assert!(!debug.contains("private-proxy-user"));
        assert!(!debug.contains("private-proxy-password"));
    }

    #[test]
    fn a_saved_proxy_password_is_bound_to_its_endpoint_and_username() {
        let original = Settings {
            proxy_mode: super::ProxyMode::Http,
            proxy_host: "127.0.0.1".into(),
            proxy_port: "8080".into(),
            proxy_username: "dummy-user".into(),
            proxy_password: " dummy-private-password\n".into(),
            ..Default::default()
        };
        let password = original.proxy_password_record().unwrap().unwrap();
        let mut restored = original.clone();
        restored.proxy_password.clear();
        assert!(restored.restore_proxy_password(&password));
        assert_eq!(restored.proxy_password, original.proxy_password);
        for (host, port, username) in [
            ("other.invalid", "8080", "dummy-user"),
            ("127.0.0.1", "8081", "dummy-user"),
            ("127.0.0.1", "8080", "other-user"),
        ] {
            let mut changed = Settings {
                proxy_host: host.into(),
                proxy_port: port.into(),
                proxy_username: username.into(),
                ..restored.clone()
            };
            changed.proxy_password.clear();
            assert!(!changed.restore_proxy_password(&password));
            assert!(changed.proxy_password.is_empty());
        }
        assert!(
            !serde_json::to_string(&restored)
                .unwrap()
                .contains("dummy-private-password")
        );
    }

    #[test]
    fn an_empty_proxy_is_omitted_from_the_file() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!json.contains("proxy"));
    }

    #[test]
    fn off_is_written_and_round_trips() {
        let settings = Settings {
            proxy_mode: super::ProxyMode::Off,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"off\""));
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.proxy_mode, super::ProxyMode::Off);
        assert_eq!(restored.proxy_config().unwrap(), super::ProxyConfig::Off);
    }

    #[test]
    fn a_legacy_socks_url_becomes_socks_mode() {
        let dir = std::env::temp_dir().join(format!("chanceify-proxy-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"proxy":"socks5://127.0.0.1:1080","proxy_username":"alice"}"#,
        )
        .unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.proxy_mode, super::ProxyMode::Socks);
        assert_eq!(settings.proxy_host, "127.0.0.1");
        assert_eq!(settings.proxy_port, "1080");
        let super::ProxyConfig::Socks(manual) = settings.proxy_config().unwrap() else {
            panic!("expected a SOCKS proxy");
        };
        assert_eq!(manual.redacted().scheme(), "socks5h");
        assert_eq!(manual.redacted().host_str(), Some("127.0.0.1"));
        assert_eq!(manual.redacted().port(), Some(1080));
        assert!(manual.redacted().username().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn http_and_socks_parse_a_host_and_port() {
        let http = super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "7890", "", "")
            .unwrap();
        assert_eq!(http.redacted().as_str(), "http://127.0.0.1:7890/");
        assert!(
            super::ProxyConfig::Http(http.clone())
                .librespot_url()
                .is_some()
        );

        let socks =
            super::ManualProxy::parse(super::ManualKind::Socks, "127.0.0.1", "1080", "", "")
                .unwrap();
        assert_eq!(socks.redacted().scheme(), "socks5h");
        assert_eq!(socks.redacted().port(), Some(1080));
        assert_eq!(super::ProxyConfig::Socks(socks).librespot_url(), None);
    }

    #[test]
    fn engine_only_accepts_plain_http_proxy_urls() {
        assert_eq!(super::ProxyConfig::Off.librespot_url(), None);
        let http_uri: http::Uri = "http://127.0.0.1:8080".parse().unwrap();
        let https_uri: http::Uri = "https://127.0.0.1:8080".parse().unwrap();
        let socks_uri: http::Uri = "socks5://127.0.0.1:1080".parse().unwrap();
        assert!(super::http_proxy_from_uri(&http_uri).is_some());
        assert!(super::http_proxy_from_uri(&https_uri).is_none());
        assert!(super::http_proxy_from_uri(&socks_uri).is_none());
    }

    #[test]
    fn proxy_credentials_stay_opaque_and_out_of_urls() {
        let proxy = super::ManualProxy::parse(
            super::ManualKind::Http,
            "127.0.0.1",
            "7890",
            "alice/name",
            " secret/with spaces ",
        )
        .unwrap();
        let redacted = proxy.redacted();
        assert!(redacted.username().is_empty());
        assert_eq!(redacted.password(), None);
        assert_eq!(proxy.username, "alice/name");
        assert_eq!(proxy.password, " secret/with spaces ");
        assert_eq!(
            super::ProxyConfig::Http(proxy.clone()).librespot_url(),
            None
        );
        let debug = format!("{proxy:?}");
        assert!(!debug.contains("alice"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn system_proxy_authentication_and_tls_endpoints_stay_out_of_librespot() {
        let authenticated = hyper_util::client::proxy::matcher::Matcher::builder()
            .all("http://alice:secret@127.0.0.1:8080")
            .build();
        let destination = http::Uri::from_static("https://apresolve.spotify.com");
        let intercept = authenticated.intercept(&destination).unwrap();
        assert!(super::librespot_system_proxy(&intercept).is_none());

        let tls = hyper_util::client::proxy::matcher::Matcher::builder()
            .all("https://127.0.0.1:8080")
            .build();
        let intercept = tls.intercept(&destination).unwrap();
        assert!(super::librespot_system_proxy(&intercept).is_none());
    }

    #[test]
    fn proxy_parse_rejects_a_missing_host_or_port() {
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "", "8080", "", "")
                .unwrap_err()
                .contains("host")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "", "", "")
                .unwrap_err()
                .contains("port")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "abc", "", "")
                .unwrap_err()
                .contains("number")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "not a host", "1080", "", "")
                .unwrap_err()
                .contains("hostname")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "0", "", "")
                .unwrap_err()
                .contains("65535")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "65536", "", "")
                .unwrap_err()
                .contains("65535")
        );
        super::ManualProxy::parse(super::ManualKind::Http, "localhost", "8080", "", "").unwrap();
        super::ManualProxy::parse(super::ManualKind::Http, "::1", "8080", "", "").unwrap();
    }

    #[test]
    fn personal_app_nudge_time_is_backward_compatible_and_round_trips() {
        let older: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(older.personal_app_nudge_at, None);
        assert!(!older.personal_app_intro_seen);

        let settings = Settings {
            personal_app_nudge_at: Some("2026-09-03T15:00:00Z".into()),
            personal_app_intro_seen: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored.personal_app_nudge_at,
            settings.personal_app_nudge_at
        );
        assert!(restored.personal_app_intro_seen);
    }
}

/// The last playlist tree received for one account.
///
/// Only folder order is cached. Edit grants must always come from the live
/// session because they can be revoked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CachedRootlist {
    pub account_id: String,
    pub entries: Vec<crate::player::RootlistEntry>,
}

/// Restorable UI session: what was open when the app last closed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub last_page: Option<String>,
    /// Context URIs most recently played, newest first.
    pub recent_contexts: Vec<String>,
    /// What was playing when the app closed, to resume from a cold start.
    pub last_context: Option<String>,
    pub last_track: Option<String>,
    pub last_position_ms: u32,
    /// Manually queued songs to restore with the remembered track.
    ///
    /// Context rows are excluded to prevent duplicates. This replaced the old
    /// `last_queue` field, so sessions using that field restore no added rows.
    pub last_added_queue: Vec<String>,
    /// Queue rows displayed on the next start. Playback restores manual rows
    /// from `last_added_queue`; it does not enqueue this list.
    pub last_queue_rows: Vec<crate::api::models::PlayableItem>,
    /// Sidebar folders rolled up, by their rootlist ids.
    pub collapsed_folders: Vec<String>,
    /// Last good playlist tree, scoped to the account that supplied it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rootlist: Option<CachedRootlist>,
    /// Shuffle mode saved across contexts and restarts.
    pub shuffle_on: bool,
    /// Each table's chosen sort, by encoded page, restored at start.
    pub sorts: Vec<(String, crate::model::TableSort)>,
    /// Last window inner size, to restore on next launch.
    pub window_size: Option<[f32; 2]>,
    /// Last window outer position, to restore on next launch.
    pub window_pos: Option<[f32; 2]>,
    /// Whether the queue panel was open.
    pub queue_open: Option<bool>,
    /// Which tab the queue panel showed: `queue` or `recents`.
    pub queue_tab: Option<String>,
    /// Last outer position of the Winamp window.
    pub winamp_pos: Option<[f32; 2]>,
    /// Last outer position of the MilkDrop window.
    pub milkdrop_pos: Option<[f32; 2]>,
    /// The window mode fullscreen lyrics left, when the app closed while
    /// showing them. eframe restores the window full screen, so the next
    /// start returns it to this mode instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lyrics_fullscreen_from: Option<WindowMode>,
}

/// Whether a window was full screen, and whether it was maximized.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowMode {
    pub fullscreen: bool,
    pub maximized: bool,
}

impl SessionState {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = match serde_json::to_string(self) {
            Ok(text) => text,
            Err(error) => {
                log::warn!("unable to encode session: {error}");
                return;
            }
        };
        let temporary = path.with_extension("json.tmp");
        let written = std::fs::write(&temporary, text)
            .and_then(|()| crate::util::replace_file(&temporary, path));
        if let Err(error) = written {
            log::warn!("unable to save session to {}: {error}", path.display());
        }
    }
}

#[cfg(test)]
mod session_tests {
    use super::{CachedRootlist, SessionState};
    use crate::player::RootlistEntry;

    #[test]
    fn old_sessions_without_a_playlist_tree_remain_readable() {
        let state: SessionState = serde_json::from_str(r#"{"last_page":"home"}"#).unwrap();
        assert_eq!(state.last_page.as_deref(), Some("home"));
        assert_eq!(state.rootlist, None);
    }

    #[test]
    fn the_playlist_tree_round_trips_with_its_account() {
        let state = SessionState {
            rootlist: Some(CachedRootlist {
                account_id: "listener".into(),
                entries: vec![
                    RootlistEntry::FolderStart {
                        id: "folder".into(),
                        name: "Favorites".into(),
                    },
                    RootlistEntry::Playlist("spotify:playlist:one".into()),
                    RootlistEntry::FolderEnd,
                ],
            }),
            ..SessionState::default()
        };

        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<SessionState>(&json).unwrap(), state);
    }

    #[test]
    fn a_new_session_atomically_replaces_the_previous_one() {
        let root = std::env::temp_dir().join(format!(
            "chanceify-session-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("session.json");
        let state = |page: &str| SessionState {
            last_page: Some(page.into()),
            ..SessionState::default()
        };

        state("home").save(&path);
        state("liked").save(&path);

        assert_eq!(
            SessionState::load(&path).last_page.as_deref(),
            Some("liked")
        );
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn queue_click_reverse_and_marks_have_good_defaults() {
        let settings = Settings::default();
        assert_eq!(settings.queue_click, 0, "Ctrl+click queues by default");
        assert!(!settings.vis_reverse);
        assert!(settings.missing_mark_big);
        let columns = TrackColumns::default();
        assert!(!columns.hide_heart && !columns.hide_plus);
        assert_eq!(columns.buttons_width(), 72.0);
        let one = TrackColumns { hide_heart: true, ..columns };
        assert_eq!(one.buttons_width(), 0.0);
    }
}

#[cfg(test)]
mod column_order_tests {
    use super::TrackColumns;
    use crate::model::SortColumn as Sc;

    #[test]
    fn a_broken_order_falls_back_to_the_standard_one() {
        let mut columns = TrackColumns::default();
        columns.order = [0, 0, 2, 3, 4, 5];
        assert_eq!(columns.order(), TrackColumns::DEFAULT_ORDER);
        columns.order = [0, 1, 2, 3, 4, 9];
        assert_eq!(columns.order(), TrackColumns::DEFAULT_ORDER);
    }

    #[test]
    fn moving_the_title_into_the_middle_keeps_every_column() {
        let mut columns = TrackColumns::default();
        let shown = [Sc::Title, Sc::Album, Sc::Added, Sc::Bpm];
        columns.move_column(Sc::Title, &shown, 1);
        let order = columns.order();
        assert_eq!(&order[..4], &[2, 0, 3, 5]);
        let mut sorted = order;
        sorted.sort_unstable();
        assert_eq!(sorted, [0, 1, 2, 3, 4, 5]);
    }
}
