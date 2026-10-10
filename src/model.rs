//! UI state, loaded data, and pending actions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use crate::api::models::*;

/// One table row: the playable, when it was added, who added it.
pub type TableItem = (PlayableItem, Option<String>, Option<String>);

/// Cached track-table rows for one page.
///
/// Lives on the app, not in egui temp data, so it dies with page eviction,
/// sign-out, and `reset_data`. A generation token stops a recreated page
/// from reusing a stale copy that still has the old revision number.
pub struct TableRowsCache {
    pub generation: u64,
    pub items_revision: u64,
    pub user_names_revision: u64,
    pub items: Arc<Vec<TableItem>>,
    /// Playlist rows retain their server slots, including gaps for unavailable items.
    pub playlist_positions: Option<Arc<Vec<usize>>>,
    pub playlist_raw_count: usize,
    pub playlist_duration_ms: u64,
    pub playlist_owner: Option<(Option<String>, String)>,
    /// Set only when the next revision extends this cached playlist prefix.
    pub playlist_append_revision: Option<u64>,
}

impl TableRowsCache {
    /// Retained heap for this cache: nested track/episode metadata, not just
    /// the top-level URI and title.
    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.items.capacity() * std::mem::size_of::<TableItem>()
            + self.playlist_positions.as_ref().map_or(0, |positions| {
                positions.capacity() * std::mem::size_of::<usize>()
            })
            + self.playlist_owner.as_ref().map_or(0, |(id, name)| {
                id.as_ref().map_or(0, String::len) + name.len()
            })
            + self
                .items
                .iter()
                .map(|(item, added, by)| {
                    playable_retained_bytes(item)
                        + added.as_ref().map(String::len).unwrap_or(0)
                        + by.as_ref().map(String::len).unwrap_or(0)
                })
                .sum::<usize>()
    }
}

fn playable_retained_bytes(item: &PlayableItem) -> usize {
    match item {
        PlayableItem::Track(track) => track_retained_bytes(track),
        PlayableItem::Episode(episode) => episode_retained_bytes(episode),
    }
}

fn artist_ref_retained_bytes(artist: &ArtistRef) -> usize {
    artist.name.len()
        + artist.id.as_ref().map(String::len).unwrap_or(0)
        + artist.uri.as_ref().map(String::len).unwrap_or(0)
}

fn image_retained_bytes(images: &[Image]) -> usize {
    images.iter().map(|image| image.url.len()).sum()
}

fn album_retained_bytes(album: &Album) -> usize {
    album.id.len()
        + album.name.len()
        + album.uri.len()
        + album.album_type.as_ref().map(String::len).unwrap_or(0)
        + album.release_date.as_ref().map(String::len).unwrap_or(0)
        + image_retained_bytes(&album.images)
        + album
            .artists
            .iter()
            .map(artist_ref_retained_bytes)
            .sum::<usize>()
}

fn track_retained_bytes(track: &Track) -> usize {
    std::mem::size_of::<Track>()
        + track.name.len()
        + track.uri.len()
        + track.id.as_ref().map(String::len).unwrap_or(0)
        + track
            .artists
            .iter()
            .map(artist_ref_retained_bytes)
            .sum::<usize>()
        + track.album.as_ref().map(album_retained_bytes).unwrap_or(0)
}

fn episode_retained_bytes(episode: &Episode) -> usize {
    std::mem::size_of::<Episode>()
        + episode.id.len()
        + episode.name.len()
        + episode.uri.len()
        + episode.description.len()
        + image_retained_bytes(&episode.images)
}

/// Every screen the central panel can show.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Page {
    Home,
    TopSongs,
    Search,
    LikedSongs,
    Albums,
    Artists,
    Podcasts,
    Episodes,
    Playlist(String),
    Album(String),
    Artist(String),
    Show(String),
    /// Spotify's radio seeded by a song, playlist, album, or artist, by the
    /// seed's URI.
    Radio(String),
    Queue,
    Settings,
    /// Songs that are files on this computer, and imported playlists.
    Local,
}

impl Page {
    pub fn encode(&self) -> String {
        match self {
            Page::Home => "home".into(),
            Page::TopSongs => "top-songs".into(),
            Page::Search => "search".into(),
            Page::LikedSongs => "liked".into(),
            Page::Albums => "albums".into(),
            Page::Artists => "artists".into(),
            Page::Podcasts => "podcasts".into(),
            Page::Episodes => "episodes".into(),
            Page::Playlist(id) => format!("playlist:{id}"),
            Page::Album(id) => format!("album:{id}"),
            Page::Artist(id) => format!("artist:{id}"),
            Page::Show(id) => format!("show:{id}"),
            Page::Radio(seed) => format!("radio:{seed}"),
            Page::Queue => "queue".into(),
            Page::Settings => "settings".into(),
            Page::Local => "local".into(),
        }
    }

    pub fn decode(text: &str) -> Option<Self> {
        Some(match text {
            "home" => Page::Home,
            "top-songs" => Page::TopSongs,
            "search" => Page::Search,
            "liked" => Page::LikedSongs,
            "albums" => Page::Albums,
            "artists" => Page::Artists,
            "podcasts" => Page::Podcasts,
            "episodes" => Page::Episodes,
            "queue" => Page::Queue,
            "local" => Page::Local,
            "settings" => Page::Settings,
            other => {
                let (kind, id) = other.split_once(':')?;
                match kind {
                    "playlist" => Page::Playlist(id.into()),
                    "album" => Page::Album(id.into()),
                    "artist" => Page::Artist(id.into()),
                    "show" => Page::Show(id.into()),
                    "radio" if crate::util::station_uri(id).is_some() => Page::Radio(id.into()),
                    _ => return None,
                }
            }
        })
    }

    /// Opens whatever a Spotify URI points at.
    pub fn from_uri(uri: &str) -> Option<Self> {
        let mut parts = uri.split(':');
        let _ = parts.next()?;
        let kind = parts.next()?;
        let id = parts.next()?.to_string();
        Some(match kind {
            "playlist" => Page::Playlist(id),
            "album" => Page::Album(id),
            "artist" => Page::Artist(id),
            "show" => Page::Show(id),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum QueueTab {
    #[default]
    Queue,
    Recents,
}

impl QueueTab {
    pub fn encode(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Recents => "recents",
        }
    }

    pub fn decode(text: &str) -> Option<Self> {
        match text {
            "queue" => Some(Self::Queue),
            "recents" => Some(Self::Recents),
            // Backward compatibility with the old tab name.
            "recently_played" => Some(Self::Recents),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    NotLoaded,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn get(&self) -> Option<&T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        match self {
            Loadable::Loaded(value) => Some(value),
            _ => None,
        }
    }

    pub fn is_loading(&self) -> bool {
        matches!(self, Loadable::Loading)
    }

    pub fn needs_load(&self) -> bool {
        matches!(self, Loadable::NotLoaded | Loadable::Failed(_))
    }

    pub fn from_result<E: std::fmt::Display>(result: Result<T, E>) -> Self {
        match result {
            Ok(value) => Loadable::Loaded(value),
            Err(error) => Loadable::Failed(error.to_string()),
        }
    }

    /// Keeps an already loaded value when a refresh fails.
    pub fn refresh<E: std::fmt::Display>(&mut self, result: Result<T, E>) {
        if result.is_ok() || self.get().is_none() {
            *self = Self::from_result(result);
        }
    }
}

/// An offset-paginated list that loads on demand as the user scrolls.
#[derive(Clone, Debug)]
pub struct PagedList<T> {
    pub items: Vec<T>,
    /// Spotify offset of the first item. Nonzero for a directly opened page.
    pub base_offset: u32,
    pub windows: std::collections::BTreeMap<u32, Vec<T>>,
    pub window_request: Option<u32>,
    pub total: Option<u32>,
    pub next_offset: Option<u32>,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_once: bool,
    pub revision: u64,
}

impl<T> Default for PagedList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            base_offset: 0,
            windows: Default::default(),
            window_request: None,
            total: None,
            next_offset: Some(0),
            loading: false,
            error: None,
            loaded_once: false,
            revision: 0,
        }
    }
}

impl<T> PagedList<T> {
    /// Select a cached window, or reserve a direct request for the visible row.
    /// Adjacent windows merge on arrival, so a viewport can cross page edges.
    pub fn window_at(&mut self, position: u32, size: u32) -> Option<u32> {
        if self.loading
            || self.error.is_some()
            || size == 0
            || self.total.is_none_or(|total| position >= total)
            || (self.base_offset..self.base_offset.saturating_add(self.items.len() as u32))
                .contains(&position)
        {
            return None;
        }
        let cached = self
            .windows
            .range(..=position)
            .next_back()
            .filter(|(start, items)| position < start.saturating_add(items.len() as u32))
            .map(|(start, _)| *start);
        if let Some(start) = cached {
            self.save_window();
            self.items = self.windows.remove(&start).unwrap();
            self.base_offset = start;
            self.loaded_once = true;
            self.join_windows();
            self.revision = self.revision.wrapping_add(1);
            return None;
        }
        let offset = position / size * size;
        let end = self.base_offset.saturating_add(self.items.len() as u32);
        // Adjacent reads extend the visible window; only a distant jump swaps it.
        if offset > end || offset.saturating_add(size) < self.base_offset {
            self.save_window();
            self.base_offset = offset;
            self.loaded_once = false;
            self.next_offset = Some(offset);
            self.revision = self.revision.wrapping_add(1);
        }
        self.window_request = Some(offset);
        self.loading = true;
        Some(offset)
    }

    fn save_window(&mut self) {
        if !self.items.is_empty() {
            self.windows
                .insert(self.base_offset, std::mem::take(&mut self.items));
        }
    }

    /// Edits or refreshes invalidate other windows' server positions.
    pub fn clear_windows(&mut self) {
        self.windows.clear();
        self.window_request = None;
    }

    fn join_windows(&mut self) {
        // The incoming page wins overlaps; retain cached rows on either side.
        loop {
            let end = self.base_offset.saturating_add(self.items.len() as u32);
            let adjacent = self
                .windows
                .iter()
                .find(|(start, items)| {
                    **start <= end && start.saturating_add(items.len() as u32) >= self.base_offset
                })
                .map(|(start, _)| *start);
            let Some(start) = adjacent else {
                break;
            };
            let mut other = self.windows.remove(&start).unwrap();
            let other_end = start.saturating_add(other.len() as u32);
            if start < self.base_offset {
                let tail = if other_end > end {
                    other.split_off((end - start) as usize)
                } else {
                    Vec::new()
                };
                other.truncate((self.base_offset - start) as usize);
                other.append(&mut self.items);
                other.extend(tail);
                self.items = other;
                self.base_offset = start;
            } else if other_end > end {
                self.items
                    .extend(other.into_iter().skip((end - start) as usize));
            }
        }
        self.next_offset = Some(self.base_offset.saturating_add(self.items.len() as u32))
            .filter(|end| self.total.is_some_and(|total| *end < total));
    }

    pub fn reset(&mut self) {
        *self = Self {
            revision: self.revision.wrapping_add(1),
            ..Default::default()
        };
    }

    pub fn can_load_more(&self) -> bool {
        !self.loading && (self.window_request.is_some() || self.next_offset.is_some())
    }

    pub fn is_complete(&self) -> bool {
        self.loaded_once && self.base_offset == 0 && self.next_offset.is_none()
    }

    pub fn absorb(&mut self, offset: u32, page: Page_<T>) {
        let window = self.window_request.take().is_some();
        let next_offset = page.next_offset();
        if window {
            if self.total.is_some_and(|total| total != page.total) {
                self.clear_windows();
                self.items.clear();
            } else {
                self.save_window();
            }
            self.base_offset = offset;
            self.items = page.items;
        } else {
            if offset == 0 {
                self.clear_windows();
                self.items.clear();
                self.base_offset = 0;
            } else if !self.loaded_once {
                self.items.clear();
                self.base_offset = offset;
            }
            let relative = offset.saturating_sub(self.base_offset) as usize;
            if relative < self.items.len() {
                self.items.truncate(relative);
            }
            self.items.extend(page.items);
        }
        self.total = Some(page.total);
        self.next_offset = next_offset;
        if window || !self.windows.is_empty() {
            self.join_windows();
        }
        self.loading = false;
        self.error = None;
        self.loaded_once = true;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn retain<F>(&mut self, f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.items.retain(f);
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn reorder(&mut self, from: usize, to: usize) {
        if from < self.items.len() && to <= self.items.len() {
            let item = self.items.remove(from);
            let insert_at = if to > from { to - 1 } else { to };
            self.items.insert(insert_at.min(self.items.len()), item);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    /// Adopt a disk prefix without replacing a distant viewport or its request.
    pub fn adopt_cached_prefix(&mut self, items: Vec<T>, total: u32, next_offset: Option<u32>) {
        if self.base_offset == 0 && self.window_request.is_none() {
            self.restore_cached(items, total, next_offset);
        } else {
            if self.base_offset == 0 {
                self.items = items;
                self.next_offset = next_offset;
                self.loaded_once = true;
            } else {
                self.windows.insert(0, items);
            }
            self.total = Some(total);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    pub fn restore_cached(&mut self, items: Vec<T>, total: u32, next_offset: Option<u32>) {
        self.clear_windows();
        self.items = items;
        self.base_offset = 0;
        self.total = Some(total);
        self.next_offset = next_offset;
        self.loading = false;
        self.loaded_once = true;
        self.error = None;
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn fail(&mut self, error: String) {
        self.loading = false;
        self.error = Some(error);
        self.loaded_once = true;
    }

    pub fn reset_at(&mut self, offset: u32) {
        self.reset();
        self.base_offset = offset;
        self.next_offset = Some(offset);
    }
}

type Page_<T> = crate::api::models::Page<T>;

/// Selected track-table rows for batch actions.
///
/// Selection belongs to one page and clears when sorting, filtering, or paging
/// changes the row order.
#[derive(Clone, Debug, Default)]
pub struct RowSelection {
    pub rows: std::collections::BTreeSet<usize>,
    /// Row used as the anchor for shift-click ranges.
    pub anchor: Option<usize>,
}

/// Selection behavior for a row click.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowPick {
    /// Select only this row.
    Only,
    /// Toggle this row.
    Toggle,
    /// Everything from the anchor to here.
    Range,
}

/// A cursor-paginated list (followed artists).
#[derive(Clone, Debug)]
pub struct CursorList<T> {
    pub items: Vec<T>,
    pub after: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    pub loaded_once: bool,
    pub complete: bool,
}

impl<T> Default for CursorList<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            after: None,
            loading: false,
            error: None,
            loaded_once: false,
            complete: false,
        }
    }
}

impl<T> CursorList<T> {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn can_load_more(&self) -> bool {
        !self.loading && !self.complete
    }
}

#[derive(Default)]
pub struct Library {
    pub playlists: Loadable<Vec<Playlist>>,
    pub playlists_next: Option<u32>,
    /// Which of the user's own playlists contain each song, for the icons on
    /// a row. Built in the background from the playlists above.
    pub membership: crate::membership::Index,
    /// The playlist on its way to be walked, so the next is not asked for
    /// until this one answers.
    pub membership_asked: Option<String>,
    /// When the membership walk was told to wait. Spotify throttles the
    /// shared session this reads with, and asking again straight away after
    /// a refusal only earns a longer refusal, so a throttled walk pauses
    /// instead of turning into a loop.
    pub membership_paused_until: Option<Instant>,
    /// Which load of the playlists the pages on their way belong to. Every
    /// load from the top takes a new one, so a page asked for by an earlier
    /// load is not taken into the new list, even at the same offset.
    pub playlists_generation: u64,
    /// The later playlist page on its way, so a second answer for a page
    /// already taken adds nothing.
    pub playlists_asked: Option<u32>,
    pub liked: PagedList<SavedTrack>,
    pub albums: PagedList<SavedAlbum>,
    pub artists: CursorList<Artist>,
    pub shows: PagedList<SavedShow>,
    pub episodes: PagedList<SavedEpisode>,
    pub filter: String,
}

#[derive(Default)]
pub struct HomeData {
    pub recently_played: Loadable<Vec<PlayHistory>>,
    pub top_artists: Loadable<Vec<Artist>>,
    /// The 20-track preview shown on Home.
    pub top_tracks: Loadable<Vec<Track>>,
    /// The separately loaded, complete ranking shown by the Top Songs page.
    pub top_songs: Loadable<Vec<Track>>,
    pub top_songs_loading: bool,
    pub top_songs_complete: bool,
    pub recommendations: Loadable<Vec<Track>>,
    pub discover: HashMap<String, Loadable<Vec<Playlist>>>,
    pub discover_pending: HashMap<String, Loadable<Vec<Playlist>>>,
    /// Saved podcasts with their newest episodes, in library order, for the
    /// podcast shelf. A refresh replaces them only once it answers.
    pub podcasts: Vec<(Show, Vec<Episode>)>,
    /// The Home generation whose podcast episodes were last asked for.
    pub podcasts_generation: u64,
    pub generation: u64,
    pub top_songs_generation: u64,
    pub requested: bool,
    pub loaded_at: Option<Instant>,
}

pub const DISCOVER_TERMS: &[&str] = &["Discover Weekly", "Release Radar", "Daily Mix", "daylist"];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchFilter {
    #[default]
    All,
    Songs,
    Artists,
    Albums,
    Playlists,
    Podcasts,
    Episodes,
}

impl SearchFilter {
    pub const ALL: [SearchFilter; 7] = [
        Self::All,
        Self::Songs,
        Self::Albums,
        Self::Playlists,
        Self::Artists,
        Self::Podcasts,
        Self::Episodes,
    ];

    /// The same pills, in the order Spotify shows them for a search that is
    /// about an artist: Artists comes straight after All.
    pub const ARTIST_FIRST: [SearchFilter; 7] = [
        Self::All,
        Self::Artists,
        Self::Songs,
        Self::Albums,
        Self::Playlists,
        Self::Podcasts,
        Self::Episodes,
    ];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::{gettext, pgettext};
        match self {
            Self::All => pgettext(locale, "filter", "All"),
            Self::Songs => gettext(locale, "Songs"),
            Self::Artists => gettext(locale, "Artists"),
            Self::Albums => gettext(locale, "Albums"),
            Self::Playlists => gettext(locale, "Playlists"),
            Self::Podcasts => gettext(locale, "Podcasts"),
            Self::Episodes => gettext(locale, "Episodes"),
        }
    }
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub committed: String,
    pub serial: u64,
    pub results: Loadable<SearchResults>,
    pub results_serial: u64,
    pub playlists: Option<(u64, crate::api::models::Page<Playlist>)>,
    pub catalogue_pending: bool,
    pub playlists_pending: bool,
    pub error: Option<String>,
    pub filter: SearchFilter,
    pub typed_at: Option<Instant>,
    pub focus_requested: bool,
}

#[derive(Default)]
pub struct PlaylistPage {
    pub generation: u64,
    /// Generation that produced the rows, which may lag during refresh.
    pub items_generation: u64,
    pub playlist: Loadable<Playlist>,
    pub items: PagedList<PlaylistItem>,
    pub filter: String,
    /// Contributor IDs from loaded pages and a sample of the final page.
    pub contributors: std::collections::BTreeSet<String>,
    /// Whether contributors were sampled from the final page.
    pub tail_checked: bool,
    /// Whether this generation's disk cache read has finished.
    pub cache_checked: bool,
    /// The Spotify offset through which this snapshot is saved on disk.
    pub cache_saved_through: Option<u32>,
    pub cache_saved_total: Option<u32>,
    /// Number of rows in that checkpoint. Spotify offsets may skip positions.
    pub cache_saved_rows: usize,
    /// Identifies an in-flight checkpoint across generation resets until it completes.
    /// One checkpoint at a time lets failed appends be retried safely.
    pub cache_write_pending: Option<PlaylistCachePending>,
    /// False when existing rows may have changed since the saved checkpoint.
    pub cache_append_valid: bool,
    /// End of the prefix restored for this generation. An initial response
    /// below it is stale and must not replace the longer cached prefix.
    pub cache_restored_through: Option<u32>,
    /// Items read from disk, waiting for the live snapshot to confirm.
    pub pending_cache: Option<PlaylistCache>,
    /// Songs added here that may sit beyond the loaded prefix. They are known
    /// members immediately, even before Spotify's next read catches up.
    pub local_additions: std::collections::BTreeSet<String>,
    /// Snapshot returned by the latest successful write. A lagging metadata
    /// read must not replace it with the snapshot from before that write.
    pub optimistic_snapshot: Option<String>,
    /// Writes still awaiting a result. Keep their optimistic rows in memory
    /// even when navigation moves beyond the usual page-cache limit.
    pub pending_writes: usize,
    /// A manual refresh waiting for pending writes and their snapshot to be
    /// confirmed before requesting replacement rows.
    pub refresh_after_write: bool,
    /// Number of immediate metadata reads made while Spotify still reported
    /// the pre-write snapshot.
    pub snapshot_rechecks: u8,
}

/// A contiguous playlist prefix on disk, valid for exactly one snapshot.
#[derive(Clone, Debug)]
pub struct PlaylistCache {
    pub snapshot: String,
    pub items: Vec<PlaylistItem>,
    pub total: u32,
    pub next_offset: Option<u32>,
    /// The on-disk prefix can accept new blocks without a full rewrite.
    pub appendable: bool,
}

pub struct PlaylistCachePending {
    pub generation: u64,
    pub snapshot: String,
    pub through: u32,
    pub rows: usize,
    pub total: u32,
    pub replacing: bool,
}

#[derive(Default)]
pub struct AlbumPage {
    pub generation: u64,
    pub album: Loadable<Album>,
    pub tracks: PagedList<Track>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DiscographyFilter {
    #[default]
    All,
    Albums,
    Singles,
    AppearsOn,
}

impl DiscographyFilter {
    pub const ALL: [DiscographyFilter; 4] =
        [Self::All, Self::Albums, Self::Singles, Self::AppearsOn];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::{gettext, pgettext};
        match self {
            Self::All => pgettext(locale, "filter", "All"),
            Self::Albums => gettext(locale, "Albums"),
            Self::Singles => gettext(locale, "Singles & EPs"),
            Self::AppearsOn => gettext(locale, "Appears On"),
        }
    }

    pub fn groups(self) -> &'static str {
        match self {
            Self::All => "album,single,compilation",
            Self::Albums => "album",
            Self::Singles => "single",
            Self::AppearsOn => "appears_on",
        }
    }
}

#[derive(Default)]
pub struct ArtistPage {
    pub artist: Loadable<Artist>,
    pub top_tracks: Loadable<Vec<Track>>,
    pub albums: HashMap<String, PagedList<Album>>,
    pub related: Loadable<Vec<Artist>>,
    pub filter: DiscographyFilter,
    pub show_all_top: bool,
}

/// A radio page: the songs Spotify mixed for its seed, which are the songs
/// its Play button plays.
#[derive(Default)]
pub struct RadioPage {
    pub songs: Loadable<Vec<Track>>,
    /// The name and artwork known when the page opened, kept should the
    /// seed's own details be let go while the page stays.
    pub name: Option<String>,
    pub images: Vec<crate::api::models::Image>,
    /// Identifies the request whose answer may fill `songs`.
    pub generation: u64,
    /// A new mix is on its way; the songs shown stay until it arrives.
    pub refreshing: bool,
}

#[derive(Default)]
pub struct ShowPage {
    pub show: Loadable<Show>,
    pub episodes: PagedList<Episode>,
}

/// A table's sort: an ordered list of keys, most significant first.
///
/// Several keys can be on at once, so a shelf can read "by album, then by
/// song number within the album" without the reader having to choose.
/// An empty list is the list's own order.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, serde::Serialize)]
pub struct TableSort {
    pub keys: Vec<SortKey>,
}

impl TableSort {
    /// A single-key sort, which is what a plain click on a heading makes.
    pub fn single(column: SortColumn, ascending: bool) -> Self {
        Self {
            keys: vec![SortKey { column, ascending }],
        }
    }

    /// Whether nothing is sorted.
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The key that decides the order first, if there is one.
    pub fn primary(&self) -> Option<SortKey> {
        self.keys.first().copied()
    }

    /// Whether this column is sorting at all.
    pub fn uses(&self, column: SortColumn) -> bool {
        self.keys.iter().any(|key| key.column == column)
    }

    /// Which way this column sorts, if it sorts at all.
    pub fn direction(&self, column: SortColumn) -> Option<bool> {
        self.keys
            .iter()
            .find(|key| key.column == column)
            .map(|key| key.ascending)
    }

    /// Where this column sits in the order of keys, counting from one.
    /// The heading shows it so a stack of sorts can be read at a glance.
    pub fn rank(&self, column: SortColumn) -> Option<usize> {
        self.keys
            .iter()
            .position(|key| key.column == column)
            .map(|position| position + 1)
    }

    /// The same sort with this column moved to the front.
    pub fn promoted(&self, column: SortColumn) -> Self {
        let mut keys = self.keys.clone();
        if let Some(position) = keys.iter().position(|key| key.column == column) {
            let key = keys.remove(position);
            keys.insert(0, key);
        }
        Self { keys }
    }

    /// The same sort with this column taken out.
    pub fn without(&self, column: SortColumn) -> Self {
        Self {
            keys: self
                .keys
                .iter()
                .copied()
                .filter(|key| key.column != column)
                .collect(),
        }
    }

    /// The same sort with this column turned the other way.
    pub fn reversed(&self, column: SortColumn) -> Self {
        Self {
            keys: self
                .keys
                .iter()
                .map(|key| {
                    if key.column == column {
                        SortKey {
                            column: key.column,
                            ascending: !key.ascending,
                        }
                    } else {
                        *key
                    }
                })
                .collect(),
        }
    }
}

/// Reads both the current list of keys and the older file that held one
/// column and one direction, so a settings file written by the last version
/// still restores the sort the reader chose.
impl<'de> serde::Deserialize<'de> for TableSort {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Stored {
            #[serde(default)]
            keys: Vec<SortKey>,
            #[serde(default)]
            column: Option<SortColumn>,
            #[serde(default)]
            ascending: bool,
        }
        let stored = Stored::deserialize(deserializer)?;
        if !stored.keys.is_empty() {
            return Ok(Self { keys: stored.keys });
        }
        Ok(Self {
            keys: stored
                .column
                .map(|column| SortKey {
                    column,
                    ascending: stored.ascending,
                })
                .into_iter()
                .collect(),
        })
    }
}

/// One column of a table's sort, and which way along it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SortKey {
    pub column: SortColumn,
    pub ascending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SortColumn {
    Title,
    Album,
    Added,
    Duration,
    AddedBy,
    /// The list's own order, for playing it reversed from the # heading.
    Index,
    /// Which of the reader's own playlists hold the song, by the first
    /// name alphabetically and then by how many hold it. A song in no
    /// playlist of their own sorts last, whichever way this runs.
    Playlists,
    /// The song's tempo, in beats a minute, from Deezer and the on-disk
    /// cache. A song with no tempo sorts last whichever way this runs,
    /// the same as a song in no playlist.
    Bpm,
    /// The date the song's album came out.
    Release,
    /// The song's genres (from MusicBrainz), by the first one's name. A
    /// song with none sorts last whichever way this runs.
    Genre,
}

/// Playback and action context for a track row.
#[derive(Clone, Debug, PartialEq)]
pub enum RowContext {
    /// A Spotify context (playlist, album) that can be played from an offset.
    Context {
        uri: String,
        /// The playlist id when the user owns it, enabling removal.
        editable_playlist: Option<(String, Option<String>)>,
    },
    /// A loose list of tracks, played as a queue of URIs.
    Uris(Arc<[String]>),
    /// A Next up row. Playing it consumes that row and all rows before it.
    Queue,
    /// A sorted or filtered context view that plays the displayed rows.
    View {
        uris: Arc<[String]>,
        context_uri: String,
        /// The playlist id when the user can edit it, enabling removal.
        /// Screen positions no longer match server positions, so moves
        /// stay disabled; removal is URI-based and safe.
        editable_playlist: Option<(String, Option<String>)>,
    },
}

/// Track data held during a drag.
#[derive(Clone, Debug)]
pub struct DragTrack {
    pub title: String,
    /// Cover art for the drag preview.
    pub image: Option<String>,
    /// Full row data, so dropping can update an open playlist immediately.
    /// A picked table row carries the whole selection in table order.
    pub items: Vec<PlayableItem>,
    /// Source playlist ID and row index for moves within an editable playlist.
    pub from: Option<(String, u32)>,
    /// The editable playlist the rows were dragged from, however many there
    /// are. Several rows have no `from`, and dropping them back into their
    /// own playlist must not add copies of them (#669).
    pub source_playlist: Option<String>,
}

/// Where the playing songs come from, as the queue's header names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayingFrom {
    pub name: String,
    /// The page that opens on click, when there is one.
    pub page: Option<Page>,
}

/// Sidebar entry held during a drag.
#[derive(Clone, Debug)]
pub struct DragEntry {
    pub uri: String,
    pub title: String,
    pub image: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Dialog {
    CreatePlaylist {
        name: String,
        public: bool,
        add_uris: Vec<String>,
    },
    EditPlaylist {
        cover: crate::playlist_cover::Draft,
        id: String,
        name: String,
        description: String,
        /// `None` while nothing has said whether the playlist is public;
        /// left untouched on save unless the switch is used.
        public: Option<bool>,
    },
    ConfirmDeletePlaylist {
        id: String,
        name: String,
        owned: bool,
    },
    ConfirmPlaylistDuplicates {
        playlist_id: String,
        playlist_name: String,
        items: Vec<PlayableItem>,
        position: Option<u32>,
        duplicate_uris: Vec<String>,
    },
    Shortcuts,
    /// Switches for every panel and full-screen view, in one place.
    Views,
    /// The signed-in account is not Premium, so nothing will play.
    PremiumNeeded,
    /// Introduce personal Spotify apps to eligible listeners once.
    PersonalAppIntro,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Error,
}

/// Songs just taken out of a playlist, kept briefly so they can be put back.
#[derive(Clone, Debug)]
pub struct UndoRemoval {
    pub playlist_id: String,
    pub playlist_name: String,
    pub items: Vec<PlayableItem>,
    pub position: u32,
    pub created: Instant,
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub message: String,
    pub kind: ToastKind,
    pub created: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Mini,
    Visualizer,
    FullVisualizer,
    FullLyrics,
    LibraryOnly,
    Default,
    Calm,
}

/// Actions emitted while drawing and applied afterward to avoid borrow conflicts.
#[derive(Clone, Debug)]
pub enum Action {
    Open(Page),
    /// Opens the radio of a song shown in a list, after caching the row's
    /// song so the page has its name and cover.
    OpenSongRadio {
        uri: String,
        track: Box<Track>,
    },
    /// Run the stream at a chosen rate, 0.5 to 2.0. Applied without a gap,
    /// so it is a thing to do mid-song rather than a setting to set first.
    SetSpeed(f32),
    /// Apply one of the named speed scenes — nightcore, hardtekk and the
    /// rest — which is a rate *and* an equalizer curve, because those are
    /// one decision to a listener and two sliders to make.
    SetSpeedPreset(usize),
    /// The next scene along. One key, from Normal through to Slowed and
    /// back, so experimenting costs nothing.
    CycleSpeedPreset,
    /// Put a named equalizer curve on at once, leaving the speed alone.
    SetEqPreset(&'static str),
    /// Open or close the scenes panel: every rate and curve side by side,
    /// with the current song's own scene and the lock to it.
    ToggleSceneBrowser,
    /// Give one song a scene of its own, or `None` to give it back the
    /// one the reader is in. Saved by song URI, so it survives a restart.
    SetSongScene {
        uri: String,
        index: Option<usize>,
    },
    /// Open or close the selector: one panel of the switches that change
    /// what the window shows, so they are a keystroke away rather than a
    /// right-click menu.
    ToggleInspector,
    /// Freeze the inspector's reading of what is under the pointer, so the
    /// pointer can be moved away while the report is written. Ctrl+Shift+L.
    ToggleInspectLock,
    /// Give the visualizer the whole window, or take the window back.
    ToggleFullscreenVis,
    /// Find this song in the reader's own library and open the list it is in,
    /// scrolled to the row. Asked for by clicking the album art.
    RevealSong {
        uri: String,
    },
    /// Extracts a page's tint while its library row is hovered.
    PrepareTint(String),
    OpenUri(String),
    /// A Spotify link from outside the app: its page opens and the window
    /// comes forward, once the account is signed in.
    OpenLink(String),
    Back,
    Forward,
    PlayContext {
        uri: String,
        offset_uri: Option<String>,
        offset_index: Option<u32>,
    },
    /// Play one episode, from `resume_ms` when the row, card or button
    /// that asked showed it as started.
    PlayEpisode {
        uri: String,
        resume_ms: Option<u32>,
    },
    PlayUris {
        uris: Vec<String>,
        index: u32,
    },
    PlayFromRow {
        context: RowContext,
        uri: String,
        index: u32,
    },
    ShufflePlay(String),
    /// Shuffle a loaded playlist so neighbouring songs have similar tempos.
    SmartShufflePlay(String),
    TogglePlay,
    /// Plays if paused; does nothing if already playing.
    PlayOnly,
    /// Pauses if playing; does nothing if already paused.
    PauseOnly,
    Next,
    Previous,
    Seek(u32),
    SeekBy(i64),
    /// Jump to a percentage of the current song: 0 is the start, 100 the end.
    /// Unlike [`Action::Seek`] this needs the song's length, so it resolves
    /// against what is playing and does nothing when nothing is.
    SeekToPercent(u8),
    SetVolume(u8),
    /// Preview volume locally during a drag; send it to Spotify on release.
    PreviewVolume(u8),
    VolumeBy(i8),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    SetShuffle(bool),
    SetRepeat(crate::player::RepeatMode),
    AddToQueue {
        uri: String,
        label: String,
    },
    ToggleSaved(String),
    /// Queue several songs in order and show one notification.
    QueueMany {
        songs: Vec<(String, String)>,
    },
    /// Move a row within the manually queued "Playing next" section, from a
    /// drag dropped back on the queue. Only applied while the local player
    /// is the active target; see [`crate::app::App::queue_locally_reorderable`].
    MoveInQueue {
        from: usize,
        to: usize,
    },
    /// Insert dragged songs into "Playing next" at a position, instead of
    /// always appending like `QueueMany`. Falls back to appending when the
    /// local player is not the active target.
    InsertInQueue {
        items: Vec<PlayableItem>,
        position: usize,
    },
    /// Set saved state for several songs explicitly.
    SetSavedMany {
        uris: Vec<String>,
        saved: bool,
    },
    AddToPlaylist {
        playlist_id: String,
        playlist_name: String,
        items: Vec<PlayableItem>,
    },
    /// Copy dragged songs into an open playlist at an absolute position.
    InsertInPlaylist {
        playlist_id: String,
        position: u32,
        items: Vec<PlayableItem>,
    },
    ConfirmAddToPlaylist {
        playlist_id: String,
        playlist_name: String,
        items: Vec<PlayableItem>,
        position: Option<u32>,
    },
    RemoveFromPlaylist {
        playlist_id: String,
        uris: Vec<String>,
    },
    MoveInPlaylist {
        playlist_id: String,
        from: u32,
        to: u32,
    },
    ShowDialog(Dialog),
    CloseDialog,
    CreatePlaylist {
        name: String,
        public: bool,
        add_uris: Vec<String>,
    },
    ChoosePlaylistCover(String),
    UploadPlaylistCover(String),
    UpdatePlaylist {
        id: String,
        name: String,
        description: String,
        public: Option<bool>,
    },
    DeletePlaylist(String),
    Transfer(String),
    /// Send the account to a receiver found on the local network.
    ActivateReceiver(Box<crate::zeroconf::Receiver>),
    RefreshDevices,
    /// Empty Next up of its queued songs, keeping the context's own.
    ClearQueue,
    /// Save the current and upcoming queue as a playlist.
    SaveQueueAsPlaylist,
    /// Save a radio page's songs to a new playlist, by the seed's URI.
    SaveRadio(String),
    RefreshQueue,
    CopyLink(String),
    /// Copy a line about the playing song, with its Spotify link, to paste in Discord.
    ShareToDiscord,
    /// Copy picked songs' links, one per line, and remember the songs so a
    /// paste of the same links can show their rows at once.
    CopySongs(Vec<PlayableItem>),
    /// Append the Spotify song links in pasted text to an editable playlist.
    PasteSongs {
        playlist_id: String,
        text: String,
    },
    /// Open a web page in the browser.
    OpenUrl(String),
    OpenInSpotify(String),
    Search(String),
    ForgetSearch(String),
    SetSearchFilter(SearchFilter),
    FocusSearch,
    LoadMore(Page),
    LoadWindow {
        page: Page,
        position: u32,
    },
    RetryWindow(Page),
    LoadMoreRecents,
    ReloadRecents,
    SetQueueTab(QueueTab),
    LoadMoreArtistAlbums(String),
    SetDiscographyFilter {
        artist_id: String,
        filter: DiscographyFilter,
    },
    ToggleShowAllTop(String),
    Reload(Page),
    SignIn,
    CancelSignIn,
    SignOut,
    /// Add, replace, or remove the optional personal Web API app.
    ConfigurePersonalWebApp,
    OpenPersonalAppSetup,
    ToggleSidebar,
    ToggleQueuePanel,
    ToggleLyricsPanel,
    SetLyricsFullscreen(bool),
    LyricsLineShown(Option<usize>),
    FollowLyrics,
    PauseLyricsFollow,
    RetryLyrics,
    ToggleDevicesPopup,
    /// Ask GitHub for the latest release and report the result to the user.
    CheckForUpdates,
    ShowUpdate,
    DownloadUpdate,
    InstallUpdate,
    SettingsChanged,
    SetTheme(crate::settings::ThemeChoice),
    /// Draw the interface in this language from the next frame on.
    SetLanguage(crate::settings::LanguageChoice),
    OpenThemesFolder,
    SetCustomTheme(String),
    ReloadThemes,
    SetLibrarySort {
        shelf: crate::settings::LibraryShelf,
        sort: crate::settings::LibrarySort,
    },
    SetLibraryGrid(bool),
    /// Show the part of the Library the sidebar lists.
    SetLibraryShelf(crate::settings::LibraryShelf),
    /// Name the cards in the Library grid, or leave the covers alone.
    SetLibraryGridNames(bool),
    /// Name the account's own playlists, or leave their covers to speak.
    SetLibraryDetails(bool),

    ToggleLibraryFolder(String),
    /// Where the player bar's bars take their colour.
    SetVisColor(crate::settings::VisColor),
    /// Set the bar colour outright, as `#rrggbb`.
    SetVisHex(String),
    /// Which shape the player bar's visualizer takes.
    SetVisMode(crate::settings::PlayerBarVis),
    /// Turn one shape (bars, flow or swirl) on or off, independently of the others.
    ToggleVisShape(u8),
    /// Z: back to the normal view: no fullscreen, no lyrics, no queue, library showing.
    NormalView,
    /// Put back the songs that were just removed from a playlist.
    UndoRemoval,
    /// W: switch the whole visualizer off, or back to the shapes it had.
    ToggleVisShapes,
    /// Turn one fullscreen bar edge on or off (1 top, 2 left, 4 bottom, 8 right).
    ToggleBarSide(u8),
    /// How many bars stand across the player bar.
    SetVisBars(u16),
    /// The gap between those bars, in pixels.
    SetVisGap(f32),
    /// How tall the bars rise.
    SetVisBarHeight(f32),
    ToggleVisBackdrop,
    ToggleVisBackdropFlow,
    /// Swirl tiles across the bar.
    SetSwirlScale(f32),
    /// Size of the play controls.
    SetControlsScale(f32),
    /// Where the song-length bar sits: 0 left, 1 middle, 2 right.
    SetSeekAnchor(u8),
    /// Which order the bottom row's three groups are in (0 to 5).
    SetRowOrder(u8),
    ToggleSeekTimeJoined,
    /// The song-time layout: 0 both ends, 1 joined before, 2 joined after, 3 off.
    SetSeekTimeMode(u8),
    /// How bent the swirl is, 0 to 1 (negative means none).
    SetSwirlWarp(f32),
    ToggleSwirlColoursFixed,
    SetSwirlWaves(u8),
    SetLyricsAlign(u8),
    ExportSettings,
    ImportSettings,
    LoadDefaultSettings,
    OpenIndexFolder,
    /// Opens or closes the floating Views and panels window.
    ToggleViewsPanel,
    /// Opens the recap picture window.
    OpenRecap,
    OpenRecapFolder,
    RegisterLinks,
    /// Writes the key binds to a file in the settings folder.
    ExportKeys,
    ImportKeys,
    /// Writes the key binds as the set every new install starts with.
    SaveKeysAsDefault,
    /// Likes or unlikes the song the pointer is over.
    LikeHovered,
    /// The library sidebar alone: no queue, no lyrics panel, no full screen.
    LibraryOnlyView,
    /// Calm mode on or off: a slow ocean, no reaction to the music.
    ToggleCalm,
    /// The bottom bar as Spotify has it: song bar above, controls in the middle, volume right.
    SpotifyBar,
    SetBarFree(bool),
    SetFreePos(u8, [f32; 2]),
    SetFreeSeek([f32; 2]),
    /// Go to one view, leaving whichever special view is on (mini player,
    /// full screen, calm) first. Choosing the view already shown goes back
    /// to the default one.
    GoView(ViewKind),
    /// With the controls popped out, hide the whole bottom bar too.
    SetFloatHideBar(bool),
    /// Opens the Views panel (never closes it: only the disc does that).
    OpenViewsPanel,
    /// Run an action from the normal window: any special view is left first.
    InDefaultView(Box<Action>),
    SetLyricsCtlOff([f32; 2]),
    /// Opens the extra visualizer window, or closes it.
    ToggleExtraWindow,
    /// Closes every extra window.
    CloseExtraWindows,
    /// Shows, moves, resizes or puts away the pop-out controls.
    SetFloatControls(Option<[f32; 3]>),
    SaveMyView,
    ApplyMyView,
    /// The Views disc was dragged to this place (or `None`: back to its own).
    MoveViewsDisc(Option<[f32; 2]>),
    /// Queues the song under the pointer; `true` puts it at the top of the queue.
    QueueHovered(bool),
    /// Likes or unlikes the playing song.
    LikePlaying,
    /// -1 smaller, 1 bigger, 0 back to 100%.
    AdjustZoom(i8),
    OpenMissingArtFolder,
    /// Leaves the full-screen views and shows the top of the playlist or
    /// album that is playing (the Escape key).
    GoToPlayingTop,
    /// Opens the queue and lyrics panels together, or closes both.
    ToggleQueueAndLyrics,
    SetSwirlTune(usize, f32),
    ResetSwirlTune,
    ToggleSwirlArtScroll,
    ToggleSwirlArtSingle,
    /// Put every swirl slider on one of the nine presets.
    SetSwirlPreset(usize),
    /// What the swirl moves with: 0 loudness, 1 bass, 2 beat, 3 nothing.
    SetSwirlReact(u8),
    SetJumpReact(u8),
    ToggleJumpAuto,
    SetJumpShake(f32),
    /// Opacity of the bars (true) or the flow (false), 0.1 to 1.
    SetVisOpacity(bool, f32),
    SetSwirlArtEdge(f32),
    /// Keep the swirl on the picked colour (true) or let it drift (false).
    SetSwirlColoursFixed(bool),
    /// Where one part of the bottom row sits: (0 controls or 2 volume, 0 left / 1 middle / 2 right).
    SetBlockAnchor(u8, u8),
    /// Open the app without Spotify: your own song files, and what was saved
    /// from the last time. Signing in later joins Spotify back in.
    ContinueWithoutSpotify,
    /// Look for albums and singles from the last weeks by the artists in
    /// every playlist (`None`) or in one playlist (its ID).
    CheckNewReleases(Option<String>),
    /// Move the play controls (0) or the volume (2) to a new place, in grid steps.
    SetBlockNudge(u8, i16),
    /// Put the play controls and the volume back where their side puts them.
    ResetBlockNudge,
    /// Puts the song bar on its own row under the controls and volume, or back.
    ToggleBarStack,
    RemoveMissingArt { artists: bool, file: String },
    SetBarLayout(u8),
    ToggleLyricsFlag(u8),
    /// The small lyrics panel's own option bits and line size.
    ToggleSideLyricsFlag(u8),
    SetSideLyricsSize(u8),
    SetSwirlPeaks(u8),
    SetSwirlDrift(f32),
    /// A song-length bar width in pixels; 0 goes back to the preset.
    SetSeekWidth(f32),
    /// The volume bar's width in pixels; 0 goes back to the standard.
    SetVolumeWidth(f32),
    ToggleVisGradient,
    ToggleVisLyrics,
    ToggleVisTextSway,
    ToggleVisSwayBeat,
    SetVisSway(u8),
    SetVisSwayAmount(f32),
    ToggleVisTextOutline,
    ToggleVisTextBack,
    /// The panel behind the title: 0 solidity, 1 roundness, 2 padding, 3 one block (0 or 1).
    SetVisBack(u8, f32),
    SetVisLivePreview(bool),
    SetVisFontRandom(bool),
    /// Pick the full-screen title font (index into VIS_FONTS).
    SetVisFont(u8),
    ToggleVisArtist,
    SaveVisPreset(String),
    ApplyVisPreset(usize),
    RemoveVisPreset(usize),
    ToggleVisLyricsBack,
    ToggleLyricsVis,
    SetLyricsVisMode(u8),
    SetLyricsVisDark(f32),
    /// Where the full-screen title sits: 0 under the cover, 1 beside it,
    /// 2 beside it and big, 3 above it with the artist below.
    SetVisTitleLayout(u8),
    /// Choose the app icon the window and taskbar button wear.
    SetAppIcon(u8),
    /// Turn the compact mini player on or off, resizing the window to match.
    ToggleMiniPlayer,
    ToggleMiniQueue,
    ToggleMiniVolume,
    SetMiniVis(u8),
    /// Cycles what clicking a song with a modifier held does: queue with Ctrl, with Alt, or nothing.
    CycleQueueClick,
    /// Checks Last.fm for the picture of every album and artist in the library, a few at a time.
    ToggleArtSweep,
    ToggleVisReverse,
    ToggleMissingMarkBig,
    ToggleMiniOnTop,
    ToggleMiniFade,
    /// Play song files from the Local songs page: these paths, in this
    /// order, starting at `index`. `shuffle` sets shuffle first; `None`
    /// leaves it as it is.
    PlayLocalFiles {
        paths: Vec<String>,
        index: usize,
        shuffle: Option<bool>,
    },
    /// Stop the song from a file.
    StopLocalFiles,
    /// Choose a folder of music to read on the Local songs page.
    AddMusicFolder,
    /// Pick song files and copy them into the song index.
    AddMusicFiles,
    RemoveMusicFolder(usize),
    /// Read the songs folder and the music folders again.
    ScanMusic,
    /// Look the music files up on Spotify, in the background.
    MatchSpotify,
    /// Last.fm: open the browser to allow chanceify™, then wait for it.
    LastfmSignIn,
    LastfmCancel,
    LastfmSignOut,
    /// Love the playing song on Last.fm.
    LastfmLove,
    /// Love any song (from its right-click menu) on Last.fm.
    LastfmLoveSong {
        artist: String,
        title: String,
        album: String,
        duration_s: u32,
    },
    /// Ask Last.fm for the account's numbers again.
    LastfmRefresh,
    /// Tests the Last.fm connection and says how it went.
    LastfmPing,
    ToggleVisBarsStay,
    /// The volume shortcut buttons.
    SetVolumePresets(Vec<u8>),
    /// Put a whole equalizer curve on, under a name.
    SetEqBands([f32; 10], String),
    /// Keep the current equalizer curve under a name.
    SaveCustomEq(String),
    /// Keep the current speed and curve together under a name.
    SaveCustomScene(String),
    /// Put one of the reader's own scenes on.
    ApplyCustomScene(usize),
    /// Remove an equalizer or scene: the reader's own is deleted, a built-in
    /// is hidden from the list (and can be restored).
    RemovePreset(String),
    RestoreHiddenPresets,
    /// Add presets pasted as text (see `Settings::export_presets`).
    ImportPresets(String),
    ToggleVisPanel,
    ToggleArtExpanded,
    CycleSeekWidth,
    ToggleLyricsFullscreen,
    /// One tap of the tap-tempo key.
    TapTempo,
    /// Start the guided tour from its first step.
    StartTour,
    /// Write the open playlist (names, artists, albums, lengths, genres,
    /// tempos) into the index folder, so it can be browsed offline.
    SavePlaylistToIndex(String),
    /// Save the playing song's album art as a picture file.
    SaveAlbumArt,
    /// Look through every playlist for songs that can no longer be played.
    ScanUnavailable,
    /// Remove every unavailable song that can be removed from its playlist.
    RemoveUnavailable,
    /// Takes one unavailable song out of one playlist.
    RemoveUnavailableOne { playlist_id: String, uri: String },
    CopyUnavailableList,
    SaveUnavailableList,
    SetVisColourPick(u8),
    /// How many sheets the flow folds together.
    SetFlowSheets(u8),
    /// How much of the bar's height the flow's stack covers.
    SetFlowDepth(f32),
    /// A multiplier on the flow's drift speed.
    SetFlowSpeed(f32),
    SetFlowOffset(f32),
    /// How tall the visualizer is drawn, as a multiple of the bar.
    SetVisScale(f32),
    /// How far up the window it reaches, as a multiple of the bar.
    SetVisRise(f32),
    ArrangeLibrary {
        pinned: Vec<String>,
        /// A drag outside the pin block selects this local playlist order.
        playlist_order: Option<Vec<String>>,
    },
    RestartEngine,
    /// Rebuild the HTTP client with the proxy in settings. Local playback
    /// restarts only when its HTTP proxy changed.
    ApplyProxy,
    ProxyEdited,
    EnablePlayback,
    ShowWindow,
    HideWindow,
    ClearArtCache,
    /// Clear local play history.
    ClearPlayHistory,
    /// Open or close the Winamp window.
    ToggleWinampWindow,
    /// Select a skin, or the built-in skin for `None`.
    SetSkin(Option<String>),
    /// Install and select a skin file.
    InstallSkin(std::path::PathBuf),
    /// Screen pixels per skin pixel in the Winamp window.
    SetSkinScale(u8),
    ToggleWinampOnTop,
    SetWinampTaskbar(bool),
    /// Step the playlist icons on every row up to the next size. Right-click
    /// an icon rather than hunt through Settings for something only a row can
    /// judge.
    CycleMembershipIconSize,
    OpenSkinsFolder,
    /// Pick a different skin each time the mini player opens.
    SetRandomSkin(bool),
    /// Cycle bars, scope, and off.
    CycleVisualiser,
    /// A click on the player bar's empty space: spectrum, waveform, off.
    CyclePlayerBarVis,
    /// Set the visualizer mode directly.
    SetVisualiser(crate::settings::VisMode),
    /// Open or close the playlist window under the mini player.
    ToggleWinampPlaylist,
    /// The playlist window's height, in skin pixels.
    SetPlaylistHeight(u32),
    /// Open or close the equalizer window under the mini player.
    ToggleWinampEq,
    /// Switch the equalizer's effect on the sound on or off.
    ToggleEq,
    SetEqBand(usize, f32),
    SetEqPreamp(f32),
    /// One of Winamp's presets, by its place in the list.
    ApplyEqPreset(usize),
    /// The balance, -1 all left to 1 all right.
    SetBalance(f32),
    ToggleMono,
    /// Roll the playlist window up to its title bar, or down again.
    ToggleWinampPlaylistShade,
    /// Roll the equalizer window up to its title bar, or down again.
    ToggleWinampEqShade,
    /// Close the window the way its close button does: into the tray when
    /// that is on, out of the app otherwise.
    CloseWindow,
    /// Roll the main window up to its title bar, or down again.
    ToggleWinampShade,
    /// Open or close the MilkDrop window.
    ToggleWinampMilkdrop,
    /// How long each MilkDrop preset plays, in seconds.
    SetMilkdropSeconds(u32),
    SetMilkdropScale(u32),
    /// How many frames a second the MilkDrop window draws; 0 is uncapped.
    SetMilkdropFps(u32),
    OpenMilkdropFolder,
    /// Fetch one of projectM's preset packs into the folder, by its place
    /// in the list.
    DownloadMilkdropPack(usize),
    Quit,
}

#[cfg(test)]
mod finite_scroll_tests {
    use super::*;

    fn page(offset: u32, count: u32, total: u32) -> Page_<u32> {
        Page_ {
            items: (offset..offset + count).collect(),
            offset,
            limit: count,
            total,
            next: (offset + count < total).then(|| "next".into()),
        }
    }

    #[test]
    fn a_failed_backward_window_can_retry_from_the_end() {
        let mut list = PagedList::default();
        list.absorb(950, page(950, 50, 1000));
        assert_eq!(list.next_offset, None);
        assert_eq!(list.window_at(920, 50), Some(900));
        list.fail("Offline".into());
        assert!(list.can_load_more());
        assert_eq!(list.window_request, Some(900));
    }

    #[test]
    fn late_cache_preserves_an_adjacent_request_and_its_cached_tail() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 1000));
        list.window_at(60, 50);
        list.adopt_cached_prefix((0..500).collect(), 1000, Some(500));
        assert!(list.loading);
        list.absorb(50, page(50, 50, 1000));
        assert_eq!(list.items.len(), 500);
        assert_eq!(list.items[60], 60);
    }

    #[test]
    fn catalog_pages_keep_rows_when_nulls_shorten_a_page() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 49, 100));
        list.absorb(50, page(50, 50, 100));
        assert_eq!(list.base_offset, 0);
        assert_eq!(list.items.len(), 99);
        assert_eq!(list.items[0], 0);
    }

    #[test]
    fn distant_windows_keep_the_total_and_return_without_a_request() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 1000));
        assert_eq!(list.window_at(720, 50), Some(700));
        assert_eq!(list.total, Some(1000));
        list.absorb(700, page(700, 50, 1000));
        assert_eq!(list.items[20], 720);
        assert_eq!(list.window_at(10, 50), None);
        assert_eq!(list.base_offset, 0);
        assert_eq!(list.items[10], 10);
        assert_eq!(list.window_at(720, 50), None);
        assert_eq!(list.base_offset, 700);
    }

    #[test]
    fn adjacent_windows_join_in_both_directions() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 200));
        assert_eq!(list.window_at(55, 50), Some(50));
        list.absorb(50, page(50, 50, 200));
        assert_eq!(list.base_offset, 0);
        assert_eq!(list.items, (0..100).collect::<Vec<_>>());
        assert_eq!(list.window_at(155, 50), Some(150));
        list.absorb(150, page(150, 50, 200));
        assert!(!list.is_complete());
        assert_eq!(list.window_at(120, 50), Some(100));
        list.absorb(100, page(100, 50, 200));
        assert_eq!(list.items, (0..200).collect::<Vec<_>>());
        assert!(list.is_complete());
    }

    #[test]
    fn overlapping_initial_album_page_is_replaced_by_the_full_window() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 20, 200));
        assert_eq!(list.window_at(25, 50), Some(0));
        list.absorb(0, page(0, 50, 200));
        assert!(
            list.windows.is_empty(),
            "superseded prefix must not stay cached"
        );
    }

    #[test]
    fn filling_a_partial_window_keeps_existing_rows_visible() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 49, 1000));
        assert_eq!(list.window_at(49, 50), Some(0));
        assert_eq!(list.items, (0..49).collect::<Vec<_>>());
        list.absorb(0, page(0, 50, 1000));
        assert_eq!(list.items, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn overlapping_windows_keep_server_positions_after_an_edit() {
        let mut list = PagedList::default();
        list.absorb(701, page(701, 50, 1000));
        assert_eq!(list.window_at(700, 50), Some(700));
        assert_eq!(
            list.items[0], 701,
            "loaded songs stay visible during the request"
        );
        list.absorb(700, page(700, 50, 1000));
        assert_eq!(list.base_offset, 700);
        assert_eq!(list.items, (700..751).collect::<Vec<_>>());
    }

    #[test]
    fn failed_window_does_not_retry_every_frame() {
        let mut list = PagedList::default();
        list.absorb(0, page(0, 50, 200));
        assert_eq!(list.window_at(150, 50), Some(150));
        assert_eq!(list.window_at(150, 50), None);
        list.fail("offline".into());
        assert_eq!(list.window_at(150, 50), None);
    }
}
