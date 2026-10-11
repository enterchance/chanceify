# Chanceify — local changes to Chanceify

Every customisation made to this fork, why it exists, and what to check after
an upstream update. This file is the map: when Chanceify moves under us, work
top to bottom and see which of these still apply.

Upstream: <https://github.com/crmne/spotifast> · Branch: `chanceify` ·
Base commit at time of writing: `d201d05` (v0.11.2)

## Building

The toolchain is Rust 1.98.0 (pinned by `rust-toolchain.toml`) plus the MSVC
Build Tools. Two traps on Windows, both handled by the env script:

- Git Bash ships a Unix `link.exe` in `/usr/bin` that shadows the MSVC linker,
  so every build fails with `extra operand`. MSVC must come first on `PATH`.
- `set` reports `PATH` and `LIB` with `;` separators. Bash reads that as one
  directory name, so nothing resolves.

```sh
source /c/Users/Chance/AppData/Local/Temp/spotifast_env.sh
cargo build --locked --no-default-features
```

**To build while a copy is already running**, put the output somewhere else
with `CARGO_TARGET_DIR=target/chanceify`. The running executable is locked,
so an ordinary build fails with `Access is denied. (os error 5)` and the
only way past it is to kill the app — which is no good if it is playing
music. A separate target directory never touches the running binary, at the
cost of a full first build:

```sh
CARGO_TARGET_DIR=target/chanceify cargo build --locked --no-default-features
target/chanceify/debug/spotifast.exe
```

The gates upstream expects, all of which our changes must pass:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --no-default-features --lib
```

## Themes (no fork needed)

Palette files live in
`%APPDATA%\paolino\spotifast\config\themes\*.json` and are picked in
Settings → Theme. `chanceify.json` is ours: a dark palette with a violet
accent. Only the sixteen `BASE_COLORS` names in `fastframe-theme` are
understood; `shadow` takes an alpha channel, the rest are `#RRGGBB`.

Two settings matter and both are easy to forget:

- `custom_theme` must name the file, or the palette is never read.
- `accent_from_art` must be `false`, or the album-art tint overrides the
  accent from the palette.

**Themes do not hot-reload on Windows.** `fastframe-theme`'s filesystem
watcher is `#[cfg(target_os = "linux")]`; `Catalog::needs_reload()` returns
`false` everywhere else. Re-pick the theme or restart to see an edit. This is
upstream behaviour, not something we broke.

## Features

Each entry says what it changes, which files, and how to tell it still works.

### 1. Build stamp

Reports the version, short Git revision, and a `*` for a dirty tree, so a local
build is never mistaken for a released one. It lives in the **window title**
(hover the title bar to read it when idle) and in Settings → About, with the
build date. Nothing is drawn in the interface itself.

- `build.rs` — emits `SPOTIFAST_REVISION` and `SPOTIFAST_BUILD_EPOCH` from Git
  and the clock, honouring `SOURCE_DATE_EPOCH` for reproducible builds.
- `src/build_info.rs` (new) — formats the stamp, converts the epoch to a
  date, with unit tests pinning the leap-day and non-leap-century cases.
- `src/lib.rs` — declares the module.
- `src/app.rs` — the idle window title, and the tray.
- `src/ui/settings.rs` — the About row.

**Check after an update:** Settings → About reads
`Chanceify 0.11.2 (d201d05+dirty built ...)`.

**Note:** there was a pill beside the search field at first. It fought the top
bar's width budget — see the layout note below — and it sat in the middle of
the interface for information that belongs out of the way. The title and About
carry it instead.

### 2. Number keys seek by percentage

`0`-`9` jump to that tenth of the playing song, the way a CD player numbers
its tracks. `0` is the start, `5` the halfway point, `9` the last tenth. There
is no key for 100% — the end of the song is one keypress away anyway.

- `src/model.rs` — `Action::SeekToPercent(u8)`, alongside the existing
  `Action::Seek`. It takes a percentage because it needs the song's length to
  resolve, so it acts on what is playing rather than an absolute position.
- `src/app.rs` — resolves the percentage against `now.duration_ms` and reuses
  the existing `self.seek`. Skipped when the duration is 0, which is what a
  live radio stream reports.
- `src/ui/keys.rs` — the bindings, and a row in the shortcuts dialog.

**Check after an update:** press `5` mid-song and the position bar jumps to
halfway. egui names the digit keys individually (`Num0`..`Num9`) rather than by
value, so the bindings go through `Key::from_name` with winit's `Digit0` and
`Numpad0`. Both rows are handled, so the number pad works too.

**Why it is its own action rather than a computed `Action::Seek`:** the keys
cannot know the song's length, so the percentage has to survive until the app
sees the currently-playing track.

### 3. Renamed to Chanceify, and X quits

Two changes that make this build feel like its own thing.

**Display name.** `build_info::DISPLAY_NAME` is `Chanceify`, used for the
window title, the tray item and its menu, the login screen, the About box, and
the update toasts. A local build should not present itself as released
Chanceify.

Only the *display* name changed. These keep their upstream names on purpose,
and `tests/branding.rs` checks the last two:

- the profile directory, so your existing sign-in keeps working
- the updater slug and legacy names, so updates still apply
- the single-instance lock id, the `spotify:` link handler, the PulseAudio
  property, and the HTTP user agent

So the binary is still `spotifast.exe` and `--version` still prints
`spotifast 0.11.2`. That is deliberate: renaming the binary would fork the
profile and break the updater. Only the window says Chanceify.

**X quits instead of hiding.** `keep_playing_in_background` now defaults to
`false`, and the close request sets `quit_requested` directly, so the X button
ends the process whatever the saved setting says. Upstream's behaviour is still
reachable from the tray menu, which still shows and hides the window.

**Check after an update:** the window title and About box read Chanceify; X
closes the app; `spotifast.exe --version` still says spotifast.

### 4. Playlist membership icons

Each song row shows small covers for the playlists of yours that contain it,
at the right of the title column. Hover one for its name; click to open it.

Spotify has no "which playlists hold this track" endpoint, so the index is
built by walking the playlists you own and reading each one's tracks — a lot
of requests for a large library, which is why it runs in the background:

- never on the UI thread, one playlist at a time, only once the playlist list
  has arrived so it never competes with a page you are waiting on
- rows show whatever has arrived and nothing blocks on it, so a playlist opens
  instantly regardless
- cached in the cache directory, so the walk happens once rather than every
  launch, and the icons are right from the first frame
- only playlists you own. A followed playlist belongs to someone else, and
  walking every one of those would be far larger still.

The playlist a row is already inside is left out: on a playlist page every
song is in it, so an icon would say nothing.

- `src/membership.rs` (new) — the index, with unit tests for the mapping,
  deduplication, deletion, cache round-trip and a broken cache.
- `src/backend.rs` — `ApiRequest::MembershipPlaylist`, which walks every page
  of one playlist rather than trusting the first, bounded by the playlist's
  own total so a wrongly reported total cannot make it spin.
- `src/app.rs` — `Library::membership`, the walk driver, and cache load/save.
- `src/model.rs` — the two `Library` fields.
- `src/ui/widgets.rs` — `membership_icons`, the column space, and the
  accessible name each icon needs.

**Check after an update:** open a playlist, and songs shared with another of
your playlists show that playlist's cover between the title and the album.
The icon's playlist is the one *other* than the page you are on.

**Most likely thing to break:** upstream rewrites the row layout often, and
the icons need reserved width from `title_rect`. If titles start overlapping
the icons, that is where to look.

### 5. X always closes the app

Folded into the rename above: the close request sets `quit_requested`
directly instead of consulting `keep_playing_in_background`.

### 6. The library sidebar: one menu, three layouts, optional names

The four shelf pills (Playlists / Albums / Artists / Podcasts) and the
separate text button that read `Local custom order` are gone. One icon in the
Library header opens a menu holding all of it: the shelf first, then the
orders that shelf supports, then how the rows are drawn.

Three ways to draw the list, all from that menu:

- **Grid** — covers with names under them.
- **Grid, covers only** — square cards, no text at all.
- **Stacked vertically** — cover left, names beside, as before.

One name setting sits at the bottom of the same menu:

- **Name my own playlists** — off, your own playlists are identified by
  their covers alone. A playlist saved from another account, or a saved
  album, always keeps its name: nothing else on the row says what it is.

There is no name size setting. Each name is drawn at the size its own row
allows, shrinking to fit down to `NAME_FIT_FLOOR` (78% of normal) before it
turns into an ellipsis — see feature 12.

The shelf is now remembered in `settings.library_shelf` instead of the
frame's temporary egui memory, so it survives a restart.

- `src/settings.rs` — `library_grid_names`, `library_own_playlist_names`,
  `library_shelf`, `library_names_shown()`, `library_names_entry()`.
- `src/model.rs` — `Action::SetLibraryShelf`, `SetLibraryGridNames`,
  `SetOwnPlaylistNames`.
- `src/app.rs` — the three handlers. Changing shelf clears the shelf filter,
  which would otherwise hide rows for a reason the reader cannot see.
- `src/ui/sidebar.rs` — `library_menu()` replaces `sort_menu()`; `grid_text_height()`
  returns 0 for the covers-only grid; `grid_layout()` takes the text height.

**Check after an update:** the Library header has three icon buttons and no
pill row; the menu switches shelf and layout and both survive a restart.

### 7. The nav/Library rule, and the slow first playlist load

**The rule.** The line between Home/Search and the Library was drawn in
`palette.outline` (`#2a3038`) on the panel background (`#15181c`) — nearly
the same value — and stopped 4px short of each side, so it read as a smudge
rather than a division. `section_rule()` now spans the panel edge to edge,
widening the clip rect to get past the panel's own margin, in a colour mixed
22% from the background toward `secondary`.

**The slow load.** Two causes, both fixed:

1. `ApiRequest::MyPlaylists` was classed `background()`, so it shared the
   four background permits with `PlaybackState`, `RecentlyPlayed`,
   `TopTracks`, `TopArtists`, `Discover`, `PlaylistSample` and `Contains` —
   requests that repeat on a timer anyway. It is now a foreground request.
2. `walk_next_playlist()` started walking playlists for the membership
   index as soon as the *first* page of `/me/playlists` landed. That walk
   pages through every song in a playlist, so it was flooding the same
   Web API in-flight slots and rate-limit budget the list itself needed. It
   now waits until `playlists_next.is_none()` and no page is in flight, and
   is kicked off from the place where the list is known to be complete.

- `src/ui/sidebar.rs` — `section_rule()`, `rule_color()`,
  `PANEL_MARGIN_LEFT` / `PANEL_MARGIN_RIGHT`.
- `src/backend.rs` — `ApiRequest::background()`, one line removed.
- `src/app.rs` — `walk_next_playlist()` and its two call sites.

**Check after an update:** first open of the Library fills noticeably faster;
the membership icons still fill in afterwards, in the background.

### 8. The stuck "Waiting for Spotify" spinner — Spotify rate limiting

The log settled this one:

```
WARN Spotify rate limit source=shared wait=24s
INFO Spotify cooldown source=shared duration_ms=24000
WARN Spotify rate limit source=shared wait=1s
WARN Spotify rate limit source=shared wait=28s
... repeating for as long as the app is left open
```

Spotify was answering `429` with `Retry-After` of 24-28 seconds, forever.
Two things made that our fault rather than bad luck:

1. **The membership walk asked again straight away.** On a refused walk the
   playlist was simply not recorded and `walk_next_playlist()` immediately
   requested the next one. Asking into a refusal earns a longer refusal, so
   a throttled library kept the throttling on itself.
2. **A parked request looked like a busy one.** `send_body` calls
   `activity.begin()` before the retry loop, so a request sleeping inside
   `wait_for_cooldown()` counts as in-flight for the whole 24 seconds. The
   walk always had at least one request parked, so `in_flight` never reached
   zero and `NetActivity::busy()` never cleared — the spinner at the top of
   the window stayed on indefinitely. It was not a repaint bug; the state
   really was busy, forever.

Both are fixed:

- `ApiClient::cooldown_remaining()` is public. The `MembershipPlaylist` arm
  in `handle()` answers `Err(ApiError::RateLimited)` **without sending
  anything** while the session is cooling down, so it spends no quota and
  holds no activity signal.
- `ApiError::is_rate_limited()` covers both a bare 429 that outlived its
  retries and a plain 429 status.
- The walk sets `library.membership_paused_until` on such a refusal and
  `walk_next_playlist()` stands down for `MEMBERSHIP_RETRY` (120s). The
  playlist is left unwalked, so the walk returns to it later. A failure that
  is *not* about rate limiting does not pause, so an offline blip does not
  stall the icons for two minutes.
- The 429 warning now logs the endpoint (`path=`), which is what made this
  diagnosable at all: without it the log only said `source=shared`, and
  every shared-source endpoint looked equally guilty.

**Check after an update:** with the log open, a throttled session shows
`membership walk paused` once, not a request every 28 seconds.

### 9. The sidebar stops where the cover stops

The resize handle could be dragged to a flat 600px, well past the point
where anything in the panel gets wider. The docked cover is sized
`min(width, height * 0.45)`, so past `height * 0.45` the art cannot grow
any further and the extra width is a strip of empty background beside it.

`sidebar_max_width()` now caps the panel at the cover's own ceiling plus
the panel margins, sharing the `EXPANDED_ART_WIDTH_FRACTION` constant with
`expanded_art_side()` so the two cannot drift apart. It scales with the
window height, so a shorter window allows a narrower sidebar.

- `src/ui/sidebar.rs` — `sidebar_max_width()`, `SIDEBAR_MIN_WIDTH`,
  `EXPANDED_ART_WIDTH_FRACTION`, `SIDEBAR_ABSOLUTE_MAX`; the old
  `size_range(210.0..=600.0)` literal is gone.

**Check after an update:** drag the handle to its limit; the cover should be
at its largest and there should be no dead space to its right.

### 10. The library reads the reader's own API app

**This is why the icons were missing, and the reader was not at fault.**

`plan()` in `src/api/gateway.rs` routed `Operation::PlaylistLibrary`
unconditionally to `ApiSource::Shared`. That covers `/me/playlists` and the
membership walk, so **a reader who set up their own Spotify app never saw a
single request from it.** Their personal quota sat completely idle while
every read went to the shared app, which every other person running this
program shares and which was returning `429` with 28-second `Retry-After`
headers. The log proved it:

```
grep -c "source=personal" spotifast.log   ->  0
25 rate limits, all path=/playlists/{id}/items
```

Two playlists got walked before the throttling began; the rest never did,
which is exactly "only a few of the icons show".

`plan()` now sends `PlaylistLibrary` to the personal app when one is ready.
Everything else is untouched: `CanonicalAccount`, `PlaylistSearch` and the
external/unknown playlist reads still go to the shared app for coverage.

**The fallback is the important part.** Routing the library to the reader's
own app is a saving, not a requirement — a personal app without the library
scope answers a refusal, and a refusal there would cost the reader their
whole library rather than a background nicety. So:

- `ApiGateway::shared_client()` and `served_personal()` are new.
- `ApiError::is_refused()` covers 401/403/404.
- The `MyPlaylists` arm and the membership walk both fall back to the
  shared client when the personal one refuses, and log that they did.
- `walk_playlist()` was lifted out of `handle()` so both clients can run the
  same walk.

**Check after an update:** `grep -c "source=personal" spotifast.log` should
be greater than zero, and the icons should fill in completely.

### 11. Player bar bar options

The bars were already drawn in colours taken from the cover's own accent.
What was missing was any way to choose. Right-click the player bar (the same
gesture that already cycles the visualizer mode) for:

- **Colour from the cover** (default) / **Colour from the theme** /
  **One colour of my choosing**, with a swatch and a `#rrggbb` field.
- **More bars (n)** — steps 12 / 24 / 40 / 60 / 75 and wraps.
- **Wider gap (n)** — steps 0 / 2 / 5 / 10 px and wraps.

The analyser still works in its own 75 bands. Fewer bars merges them,
keeping **the loudest band of each group** rather than a mean, so a quiet
group cannot vanish between two loud ones. The groups are cut by proportion,
not by a fixed chunk size: fixed chunks dropped the remainder, so 75 bands
into 12 bars gave 11 bars with the last one showing nothing.

These live on the bar rather than in Settings because each one is judged by
looking at the bar, which is the same reason the membership icon size is a
right-click rather than a settings row.

- `src/settings.rs` — `VisColor`, `player_bar_vis_color` / `_hex` /
  `_bars` / `_gap`, `VIS_BAR_COUNTS`, `VIS_GAPS`, and clamped accessors so
  a hand-edited file cannot leave the bars invisible.
- `src/model.rs` — `SetVisColor`, `SetVisHex`, `CycleVisBars`, `CycleVisGap`.
- `src/ui/player_bar.rs` — `vis_menu()`, `merge_bands()`, and `spectrum()`
  taking the gap as an argument.

**Check after an update:** right-click the player bar; the menu opens and
each choice changes the bars at once.

### 12. The Library header, cleaned out

The header had grown to five icon buttons beside the heading — library
view/order, hide sidebar, grid/list, create playlist, search — and a search
field on a row of its own underneath. There was also a **Search** nav row
under Home, next to a search field that already lives in the top bar beside
the back and forward arrows.

- The **Search** nav row is gone. One search, in the top bar.
- The search icon is gone. **The "Library" heading itself is the toggle**,
  and the field opens **beside the heading, in the same row**. An underline
  appears under the heading on hover so it reads as a control. Clicking
  again closes it and clears the filter.
- The section **icon** is the second way in, and the only one left when the
  sidebar is too narrow for the heading and the field together: below
  `INLINE_SEARCH_MIN` (90px) of room the heading gives way and the field
  takes the whole row (`INLINE_SEARCH_MAX` is 220px).
- The **grid/list button** is gone; that choice is in the menu already, as
  Grid / Grid, covers only / Stacked vertically. Three buttons left: the
  menu, create playlist, hide sidebar.
- **Bigger names** is gone, with it `Action::CycleLibraryNameSize`,
  `Settings::NAME_SCALES`, `library_name_scale` and
  `next_library_name_scale()`. Names now **autoscale per name**:
  `fitted_name_size()` measures the name against the width its row has and
  shrinks it, floored at `NAME_FIT_FLOOR`, so a shelf holding both "Chill"
  and a forty-character album title reads well. Card height is fixed again
  (`GRID_TEXT_HEIGHT`), because a name never grows past its base size.
- The heading's clickable rect is **only as wide as its words**. It was once
  the whole leftover row, which reached under the buttons and swallowed
  their clicks.

- `src/ui/sidebar.rs` — the header row, `fitted_name_size()`,
  `NAME_FIT_FLOOR`, `INLINE_SEARCH_MIN` / `_MAX`, and the `focus_search`
  request now made by widget id, because the click that opens the field is
  read after the row is drawn and so the field appears on the next frame.
- `src/demo.rs` — `the_library_heading_never_overlaps_its_buttons` (now
  measures against the menu button), `library_grid_toggle_is_accessible_and_persistent`
  (drives the menu instead of the deleted button), and the new
  `the_library_search_shares_the_heading_row`.

**Check after an update:** click "Library" — the field appears beside it,
not under it; the header has three buttons and no Search row; long playlist
names shrink rather than being cut off.

### 13. Multi-key sort, resizable columns, sorting by playlist

One click on a heading cycled a single column. The header now carries a
right-click menu, and the sort it holds is a **list of keys**, not one.

- `TableSort` is `{ keys: Vec<SortKey> }` with `single()`, `is_empty()`,
  `primary()`, `uses()`, `direction()`, `rank()`, `promoted()`,
  `without()`, `reversed()`. A hand-edited `settings.json` from the last
  version (`{column, ascending}`) still reads — `TableSort` has a custom
  `Deserialize` for it.
- New `SortColumn::Playlists`. A song sorts under the **first of its
  playlists alphabetically** (ties broken by how many playlists it is in,
  descending), and songs in no playlist at all always go last.
  `playlist_key()` excludes the playlist being read, via `Page::Playlist(id)`,
  so a playlist's own contents never change its own ordering.
- The right-click menu lists every sortable column with a check, its **rank
  number**, and a direction arrow; clicking promotes the column to first, and
  clicking it again cycles its direction. Below that a **Columns** section
  shows/hides each one and resets the widths.
- Column widths are stored in `Settings::TrackColumns { album, added_by,
  added }` (`MIN` 56, `MAX` 640, and a width below `MIN` means *hidden*).
  `table_layout()` is the single place that works out the geometry, and
  `column_divider()` gives each heading a drag handle whose response is
  returned so a right-click on the divider can anchor the menu.
- Sort keys are built **once per row** into a `SortValue::{Text, Number,
  Playlists}` (`sort_visible()`), and `TableCache.membership_scanned` is what
  invalidates the table when the membership index changes underneath it.

- `src/model.rs` — `TableSort`/`SortKey`/`SortColumn`, the back-compat
  `Deserialize`, `HeaderAction`'s home (`src/ui/widgets.rs`).
- `src/ui/collection.rs` — `sort_visible()`, `sort_value()`,
  `compare_values()`, `playlist_key()`, `cycle_single_sort()`, `TableCache`.
- `src/ui/widgets.rs` — `table_header()` now returns `HeaderAction`, plus
  `SORTABLE`, `RESIZABLE`, `column_label()`, `sort_menu_row()`,
  `cycle_sort()` (now `pub(crate)`), `column_divider()`, `table_layout()`,
  `columns()`, `TITLE_MIN`.
- `src/settings.rs` — `TrackColumns` and its accessors; `src/membership.rs` —
  `Index::add_test()`.

**Check after an update:** right-click a heading — the menu names each column
and shows which keys are in play; sorting by Playlists leaves playlist-less
songs at the bottom; a dragged divider survives a restart.

### 14. Shortcuts, the visualizer menu, and the player-bar text

A grab-bag of things that were either mis-bound or hostile to use.

- **B hides the sidebar.** It was `ToggleSaved`, which had no business being
  on a bare letter key; Ctrl+B is untouched. `F1` is new and toggles the
  custom titlebar (`Action::ToggleCustomTitlebar`).
- **The visualizer menu no longer closes when you use it.** "More bars (n)"
  and "Wider gap (n)" each cycled one value and shut the menu, so reaching
  40 meant 40 right-clicks. Both are now `stepper_row()`: the label reads
  **Bars (75)** / **Gap (5)** and a **− / +** pair either side steps it in
  place. `Action::CycleVisBars`/`CycleVisGap` became
  `StepVisBars(i32)`/`StepVisGap(i32)`, backed by
  `Settings::step_player_bar_vis_bars()`/`_gap()` (both wrap).
- **Colour from the cover actually works.** `visualizer()` now asks
  `App::now_playing_cover_colour()`, which calls `tint_for()` — the live
  cover, every frame — instead of `now_playing_tint()`, which returned `None`
  unless the separate `accent_from_art` setting happened to be on.
- **Bigger now-playing text, and no doubled cover.** The title is
  `theme::medium(16.0)` and the artist/subtitle `theme::regular(13.0)`. The
  small cover is only drawn when the big one is not (`art_expanded` and a
  visible sidebar).
- **"Name my own playlists" only clears the name.** The stacked row draws
  the name and the owner line independently (`named = entry.owned`, with a
  second row drawn only when the subtitle is non-empty), each centred when
  it is the only line, so the user's own name survives the setting.

- `src/ui/keys.rs` — the two bindings and `b_hides_the_sidebar`.
- `src/ui/player_bar.rs` — `stepper_row()`, `vis_menu()`, `visualizer()`,
  `now_playing_block()`.
- `src/ui/sidebar.rs` — the stacked row; `src/app.rs` — the new handlers and
  `now_playing_cover_colour()`.

**Check after an update:** F1 removes and restores the Windows titlebar; the
visualizer menu stays open while you hold it down on −/+; colour-from-cover
needs no second setting enabled.

### 15. The `hm://collection` base64 warning

Every launch logged, at `warn`, twice:

```
librespot_core::dealer ... base64 decoding failed: Invalid symbol 123, offset 0
librespot_connect::state ... context is not available. type: Default
```

Both are librespot talking about a **response we never needed**: the dealer
chokes unpacking a paging envelope the pinned build does not understand, and
`context` is simply not loaded yet. Neither is a fault in chanceify, and
neither is actionable. Both loggers are pinned to `error` in
`default_filter` (`src/entrypoint.rs`) — in the verbose filter too, so
`--verbose` does not bring them back.

**Check after an update:** the two lines are gone from `spotifast.log`; if a
new `librespot_*` module starts complaining, add it there rather than
chasing it.

### 16. The visualizer, rebuilt around a panel that stays open

The right-click panel over the player bar **closed on every press inside
it**. That is egui's default — `PopupCloseBehavior::CloseOnClick` — and it
made the panel useless for anything but a one-shot choice: press a stepper,
the panel vanished, press again, re-open, press again.

- The panel is now `CloseOnClickOutside`. It stays open through a whole
  sequence of adjustments and closes only when the click lands elsewhere.
- The bar count and the gap are **sliders**, not menus of fixed sizes. The
  range is **4 to 256 bars** and **0 to 24 pixels of gap**; before, the
  count was capped at the analyser's 75 bands and the gap at 12, and both
  were reachable only by stepping through a five-item and a four-item
  list. `player_bar_vis_bars` widened from `u8` to `u16` to hold 256.
- The panel names the three shapes: **Bars**, **Flow**, **Waveform**.

**Flow** is the new mode, in the spirit of the fluid wallpapers rather
than a row of separate bars. `flow_sheets()` draws `FLOW_SHEETS` (5) filled
ribbons, each riding on the spectrum and swinging sideways on a wave of its
own; the ones behind sit higher, swing the other way, and are fainter, so
the stack crosses itself and the marbling falls out of that. The drift runs
off `App::player_bar_vis_flow`, one continuous clock rather than a restart
per frame. It reads `FLOW_POINTS` (96) points spread across the analyser's
bands by `spread_bands()` — the opposite of `merge_bands()`, which merges
down. The gradient and colour-from-the-cover are untouched, so the sheets
sweep from the bass colour to the treble colour exactly as the bars do.

**Check after an update:** right-click the bar, drag both sliders, and the
panel is still there; pick Flow and the sheets drift and fold; 256 bars is
reachable in one drag.

### 17. Playlist names, and the words on the big art

- **The library menu item is now "Playlist names"**, not "Name my own
  playlists". It was a checkbox worded as a command, so pressing it turned
  the names *off* and those rows fell back to a line reading only
  "Playlist" — which names nothing, and leaves the row indistinguishable
  from any other. It reads as the state it is now, and the names are on by
  default.
- **The song's words left the player bar.** With the big art up in the
  sidebar, the title, artist and heart are drawn on a rounded, opaque card
  straddling the foot of the art (`now_playing_overlay()`), so the panel
  can be as narrow as it likes and the cover keeps the full width. The card
  is opaque rather than tinted on purpose: a translucent one takes the
  cover's own colour and loses both the words and the contrast that keeps
  them legible. Collapse the art and the words return to the bar, so the
  song is never left unnamed.

- `src/ui/player_bar.rs` — `vis_menu()`, `slider_row()`, `flow_sheets()`,
  `spread_bands()`, `now_playing_overlay()`; `src/settings.rs` —
  `PlayerBarVis::Flow`, `VIS_BARS_MIN`/`_MAX`, `VIS_GAP_MAX`, the widening
  of `player_bar_vis_bars`; `src/model.rs` — `SetVisMode`/`SetVisBars`/
  `SetVisGap`; `src/app.rs` — those three handlers and
  `player_bar_vis_flow`.

**Check after an update:** "Playlist names" turns names off and on when
pressed, and the row always says what it is; with the art up the words sit
on the art and not on the bar; collapsing the art brings them back.

### 18. Details instead of names, a taller flow, and a grouped library menu

**The bar cap was real, and it was mine.** `visualizer()` merged the
analyser's bands down to whatever count was asked for, and `merge_bands()`
clamps to the number of bands it is handed — 75. So every count above 75
silently drew 75 bars, which is why a slider dragged to the far end looked
as though it had not moved. It now spreads the bands out above that point
instead of merging them in, and `a_bar_count_past_the_analyser_is_drawn_in_full`
guards it. Nothing is capped below the number the reader asked for.

**Flow fills the bar and gained three sliders.** The stack used to pool in
the bottom third. `FlowStyle { sheets, depth }` now carries how many sheets
fold and how much of the bar's height they cover (default 0.86), and the
drift takes a speed multiplier. The panel offers **Sheets**, **Height** and
**Speed** while Flow is chosen, and **Count** and **Gap** while Bars is —
each shape only what means something for it, since a bar count changes
nothing about a flow and a gap is invisible without separate bars. The
slider is labelled **Count**, not **Bars**, because the panel already has
a shape called Bars above it. Gap now stops at 10px: past that the row is
mostly hole.

**Colour from the cover sweeps further.** The treble end was the bass hue
rotated a sixth of the way round, which for a third of the wheel lands on
nearly the same pair of hues and barely changes. It is now a quarter turn,
and the saturation and value drop slightly at the top end, so the two ends
are separable by eye and not only by position.

**The library's "Playlist names" is now "Details", and hides the second
line.** You were right that I had it backwards: it was hiding the *name*
and leaving the rows reading only "Playlist", which names nothing. It now
takes away the line underneath — "Playlist • chan", "Album • Artist" — and
the name always draws, centred on its own when the line is gone. The field
was renamed `library_own_playlist_names` → `library_details` rather than
reused, so an existing file with no `library_details` reads as `true` and
keeps the detail lines it had.

**The library menu is three named groups** — *Showing*, *Order*, *Layout* —
instead of one list of a dozen rows with rules in it, via
`widgets::menu_group()`. The sort order is custom → recent → name →
Spotify, with Recently added alongside the others.

- `src/ui/player_bar.rs` — the up/down split in `visualizer()`,
  `FlowStyle`, `vis_colours()`, the per-shape sliders;
  `src/settings.rs` — the three flow fields and their bounds,
  `library_details`; `src/ui/widgets.rs` — `menu_group()`;
  `src/ui/sidebar.rs` — the grouped menu, the Details item, and the rows
  that no longer hide names.

**Check after an update:** drag Count to 256 and count the bars; switch to
Flow and turn Height up until the stack fills the bar; turn Details off and
every playlist still shows its name.

### 19. The membership walk reads the playlist cache instead of Spotify

**The cause of the exhausted Development Mode quota.** Spotify has no
endpoint for "which playlists contain this track", so the icons added in
feature 4 build a reverse index the only way it can: walk every playlist
the reader owns, fifty songs per request, and record the mapping. On a
library of ~12 playlists with several hundred to a thousand songs each,
that is 60–100 requests at every launch. Dev-mode quota is deliberately
tiny, so it runs out before the reader has looked at anything. The log
names it plainly:

```
path=/playlists/<id>/items   (repeated, 1–2s apart, while throttled)
```

**The fix.** Those same rows are *already on disk*. Every playlist the
reader has ever opened was cached to
`cache/playlists/<account>/<id>.json` by the playlist page's own
incremental cache. `walk_playlist()` therefore reads that cache first and
returns without sending anything when the cache is **complete**
(`next_offset.is_none()` and the row count matches the total). A partial
cache is deliberately *not* treated as the whole answer — that would have
the index claiming songs are in a playlist it never actually read — so it
falls through to the network walk as before.

The cache path rides along on `ApiRequest::MembershipPlaylist { cache }`,
built in `App::playlist_cache_path()` from the signed-in user's own id,
which is the account the cache is filed under. It is the same string the
worker's gateway reports; the interface already knows it from `/me`.

There is **no way to bypass the quota** and none is attempted: it is a
server-side limit tied to the app's client id, and there is no cache trick
or token swap that avoids it. Reducing the number of requests we make is
the only honest route, which is what this does.

Tests: `a_fully_cached_playlist_is_walked_without_asking_spotify` and
`a_partial_cache_is_not_taken_for_the_whole_playlist`.

**Check after an update:** launch with a warm playlist cache and the log
shows no `/items` requests for a playlist whose rows are already cached.

### 20. Header right-click, resizable columns, Swirl, and a calmer waveform

**Right-click anywhere in the header opens the sort menu.** Every heading,
the `#`, and the clock are interactive in their own right, and each
swallowed the right-click before the row ever saw it — so the menu only
opened on empty space. `table_header()` now keeps each element's response
as a menu anchor and draws the menu for whichever one was actually
right-clicked.

**The column dividers now work as a reader expects.** Three fixes:
- The **grab area is 14px wide** and drawn over the full header height,
  rather than 9px of near-invisible target.
- The handle is **always drawn**, faintly, and brightens on hover — before
  there was nothing to find.
- The drag direction was **inverted**. The columns are laid out from the
  right inwards, so dragging an edge *right* should *narrow* the column to
  its left, and `+ delta` was doing the opposite of what the edge did.
  It also carries a hover hint and keeps the double-click reset.

**Swirl** is a fourth visualizer shape, between Flow and Waveform in the
cycle. Seven rings, each a closed curve whose radius is the spectrum
sampled *around the circle* — so loud parts push their ring out and quiet
parts pull it in — plus two waves of different lengths so it does not read
as a circle with a wobble. It is deliberately **not** the artwork warped:
that would need a shader and would end up showing the picture again, which
is what the big art is already for. This draws the *shape* a swirl makes,
in the cover's own colours. It shares Flow's three sliders.

**Monochrome covers no longer get an invented hue.** `vis_colours()` was
forcing saturation to 0.62 and then rotating the hue a quarter turn — on a
black-and-white cover that meant picking a hue out of nothing, which is why
a white-line drawing came out red or green. Below `MONOCHROME_SATURATION`
(0.08) the bar now sweeps in **brightness only**, and both ends sit *under*
the brightness ceiling rather than above it, because above it each is scaled
down to the ceiling separately and the two ends land on the same grey.

**The waveform reads an envelope, not an instant.** `scope_line()` took one
sample every seventh place, which on a track that is not mixed well — where
the waveform fills most of the range and jumps — came out as a band of
spikes with no shape. Each point now takes the extreme of the run it
covers, widened with the bar so the line stays smooth at any point count.

Tests: `a_right_click_anywhere_in_the_header_opens_the_sort_menu`,
`a_monochrome_cover_never_gets_an_invented_hue`,
`the_scope_line_reads_the_envelope_of_each_run`.

**Check after an update:** right-click a heading and the menu opens; drag a
divider right and the column to its left narrows; Swirl sits between Flow
and Waveform; a black-and-white cover gives a grey bar.

### 21. A bar-wide swirl, a movable visualizer, and a queue that can be trimmed

**The header menu stays open.** `table_header()` only drew the popup for an
anchor that reported `secondary_clicked()` *that* frame. egui reads a popup
that is not drawn as a close, so the menu appeared for one frame and vanished
before anything could be clicked. The menu is now drawn for **every** anchor
on every frame, which is what a panel menu must do.

**A divider can no longer get stuck.** Two causes, both fixed:
- Overlapping grab areas. `column_divider()` now takes `placed: &mut Vec<f32>`
  and skips any handle within `DIVIDER_HANDLE_WIDTH` (14px) of one already
  drawn. Two handles on top of each other left the topmost taking every click
  and the one beneath unreachable.
- A drag past the minimum **hid** the column, which took its own handle with
  it — so there was nothing left to drag back. Dragging now clamps at
  `TrackColumns::MIN`; hiding is a menu-only choice.

**Swirl crosses the whole bar.** It was one small medallion of seven rings
behind the transport. It is now a **chain of nine rings** (`SWIRL_RINGS = 9`)
strung edge to edge, each a little further round than the last and each
riding its own slow drift, so the crossings pile up across the full width.
Ring radius is `rect.height() * 0.62` and the chain is `rect.width() + 2 *
reach` wide, so the first and last rings run off the ends rather than
stopping short. The soft core became a wash along the foot of the bar.

**The visualizer can be moved and scaled.** Two new settings,
`player_bar_vis_scale` (1.0–6.0) and `player_bar_vis_rise` (0.0–6.0), with
`Action::SetVisScale` / `SetVisRise` and two new sliders in the vis panel.
The bottom panel grows to `PLAYER_BAR_HEIGHT + rise + PLAYER_BAR_HEIGHT *
(scale - 1)`, the controls sit in a `PLAYER_BAR_HEIGHT` band at its foot, and
`behind` extends upward.

**The queue comes in two readings.** `Settings::queue_compact` (default
`false`) picks song names alone, or the full row with art, playlist icons and
the length. It lives on a **right-click of the queue header row**, not as a
third button: a button there wrapped the chips onto two lines at minimum
panel width and broke `the_narrowest_panels_keep_their_headers_on_one_row`.

**Grid names scale with their card.** `grid_name_size(card_width)` returns
`card_width * 0.125` clamped to 12–26, and the room under the card is
`grid_name_size * GRID_TEXT_LINES` (3.2) rather than a fixed 44px, so the two
keep their proportion as the shelf changes. A two-across shelf now reads as
two large titles instead of two captions.

**Also:** "Click to change the visualizer" is gone from the player bar; the
library header's `+` and hide-sidebar icons are gone (creating is a
right-click on the **Library heading**, which opens the full library menu plus
*Create a playlist*; hiding is the B hotkey); `HEADER_GUTTER` (14px) spaces
the inline search field off both neighbours.

Tests: `the_header_sort_menu_stays_open_after_the_pointer_leaves`,
`dragging_a_divider_past_the_minimum_leaves_the_column_shown`,
`column_handles_stay_a_handle_width_apart`,
`the_queue_header_right_click_switches_between_names_and_the_full_row`,
`grid_names_scale_with_the_card_they_sit_under`,
`the_player_bar_shows_no_visualizer_tooltip`.

**Check after an update:** right-click a heading and drag away — the menu
follows; shove a divider into its neighbour — neither sticks and neither
disappears; Swirl runs the length of the bar; the queue header right-click
switches names-only on and off; the grid names grow with the cards.

### 22. A BPM column, fetched once and kept forever

**Spotify's tempo is gone, and so is its key.** `GET /v1/audio-features/{id}`
was the endpoint that carried both `tempo` and `key`. Spotify retired it on
27 November 2024 for every app without a quota extension from before that
date. Probed against this build's own stored credential, it answers:

```
/v1/audio-features/4uLU6hMCjMI75M1A2tKUQC -> 403
```

The Spotify client's own key and tempo come from data it already holds; there
is no server call behind them, and nothing an app may ask for. So there is no
Spotify BPM to cross-check Deezer's against, and no Spotify key to show.
`examples/audio_features_probe.rs` is the probe that proved it and will
re-prove it if the endpoint ever returns.

**Deezer supplies the tempo.** It publishes one for free with no account, and
`https://api.deezer.com/track/isrc:{isrc}` is an exact lookup. The join key is
the ISRC, which the two catalogues share for one recording, and which Spotify
already hands us on every track it returns — `Track::recording_key()` already
existed for the saved-state work. No title-and-artist guessing is involved.

- A tempo belongs to a **recording**, so the cache is keyed by ISRC and every
  market's release of a song shares one value. Two versions of one song sort
  next to each other rather than a decimal apart.
- **A tempo is never asked for twice.** A recording Deezer has no tempo for
  is stored as `null` exactly as one with a tempo is stored as a number, so an
  ambient or spoken-word playlist costs one request for the life of the
  profile rather than one per launch. This is the difference between a
  feature that is merely nice and one that quietly re-fetches forever.
- **Only rows on screen are asked about**, so a thousand-song playlist costs
  a thousand requests only if it is scrolled a thousand songs.
- An in-flight request is tracked **apart from** the cache. Marking a request
  as answered when it starts would turn a failed attempt into a permanent
  "no tempo" for a song that does have one.
- The cache is written once per burst, not once per row, and on shutdown.

**The column is on by default**, sits next to the song names as the leftmost
and narrowest optional column, and is **the last to be dropped** when the
window is too tight — it is three digits and a label, where an album name is
not something that shrinks. Hide it, drag it, or sort by it from the header
menu like any other. A tempo is drawn to a tenth where it has one and to a
whole number where it does not: dance records cluster at 126-130 where the
tenth is the difference between two tracks, and a clean 174 needs no decimal.
A song whose tempo has not arrived yet is left blank rather than dashed — the
column is filling in as you scroll, and a column of dashes reads as a column
of answers that are all "unknown".

**Key remains unavailable.** Neither Spotify nor Deezer publishes one. It
would need a different, key-based service with its own API key and rate
limits.

Tests: `a_tempo_is_read_from_a_deezer_track`,
`an_unmeasured_recording_has_no_tempo`,
`a_recording_with_no_tempo_is_never_asked_about_twice`,
`a_cache_round_trips_through_the_disk`,
`a_visible_row_is_asked_for_its_tempo_exactly_once`,
`songs_sort_by_tempo_with_the_untempoed_ones_last`,
`a_tempo_is_shared_by_every_release_of_one_recording`,
`a_narrow_window_drops_the_wide_columns_before_the_tempo`,
`a_tempo_is_drawn_at_a_precision_that_can_be_told_apart`.

**Check after an update:** open a playlist and the BPM column fills in as you
scroll; quit and reopen and it is already there; sort by BPM and the untempoed
songs sit at the bottom either way round.

### 23. Columns that line up, a menu that shouts, and artwork that swirls

**The heading row and the song rows disagreed.** `table_header` drew ALBUM,
ADDED BY, DATE ADDED, BPM while `track_row` drew ALBUM, ADDED BY, DATE ADDED,
BPM — the same order now, but the cursor also failed to step over the DATE
ADDED column, so BPM was painted at DATE ADDED's own left edge and two
headings sat on top of each other with every tempo under the wrong label.
Both are fixed: the headings are drawn in the order the cells are, and
`cx += laid.added` follows. `every_heading_sits_above_the_column_its_rows_use`
draws a header and a row side by side and checks both the order of the
headings and that each value starts at its own heading.

**The header menu no longer has a dead button.** The "Sort by" line over the
sort entries looked like an item, could not be pressed, and said nothing the
entries did not. It is gone, every remaining label is uppercased — `RESET
COLUMN WIDTHS` included, which was the last one still whispering — and
"Original order" plus "Clear sorting" have become one entry, `NEWEST FIRST`.

**Clearing the sort now means something.** With no sort at all a list falls
through to `TableSort::single(SortColumn::Added, false)`: the most recently
added song is at the top. Rows with no date all tie, so a list Spotify never
dated keeps the order it arrived in and nothing moves.
`an_unsorted_list_puts_the_newest_songs_at_the_top` proves it; the shared
collection fixture was given one shared date so the copy, delete and
arrow-key tests keep testing row order rather than sorting.

**The dividers look like handles.** A hairline between two columns read as a
rule, not as something to grab. Each one now carries a three-dot grip down its
whole height, a soft band behind it under the pointer, a
`ResizeHorizontal` cursor, and the accent colour while it is being dragged.

**Song names scroll instead of being cut.** `song_name_cell` runs a marquee
over egui's own memory and its own clock, parks at the start, holds at each
end, and eases back when the pointer leaves. It takes a `reserve` so the
artists and the inline date keep their room: a name that is too long scrolls
through its own column rather than pushing the rest of the line off the edge.
It is used on the two-line row too, which is the queue and recents panel, and
those rows give up their album and tempo columns so the name can have it. The
playlist icons only draw when the name would still have
`MEMBERSHIP_MIN_NAME` (104px) without them.

**Swirl is the album art, warped and carried sideways.** `src/swirl_art.rs`
fetches the cover, warps it in a 128x128 polar pass (`TURNS` 1.5, `PULL`
0.55, `powf(0.7)` sampling, 24MB ceiling) on the loader's runtime, and hands
back a `TextureId`. `player_bar::swirl` tiles that square across the whole
bar at 1.6 bar-heights and scrolls it along x with `SWIRL_TRAVELS`,
tinting by bass and washing the foot dark. The old ring maths is gone.
`ArtLoader::spawn_processed` exists for it, because `ArtLoader::inner` is
private.

**Flow can be out of step with itself.** `player_bar_vis_flow_offset`
(`VIS_FLOW_OFFSET` 0.0..=0.5, default 0.0) pushes each sheet that far along
the spectrum from the one in front. At zero the stack is one thick ribbon;
above about a fifth the folds show. Sliders in the vis menu for it have a
number box on the right that submits on Enter or focus loss.

**The visualizer menu says what it is for.** `SHAPE` over Bars/Flow/Swirl/
Waveform, then a per-mode heading and only that mode's own settings — Swirl
no longer borrows Flow's sheets. The four colour sources became one `COLOUR`
row of three choices (`From the cover`, `From the theme`, `My own`), with the
hex field only for the third. Sliders are 24px rows with a 52px number field.

**The now-playing card opens up on hover.** Taller (46% of the art, 76-132px),
a title at 30% of the card height, the heart in its own bottom-right corner
faded to nothing until the pointer is on the card, and the artists line
arriving over 0.15s rather than blinking in.

Tests: `a_cover_becomes_a_square_swirl`, `the_warp_actually_moves_the_picture`,
`the_warp_keeps_most_of_the_cover`,
`something_that_is_not_a_cover_yields_nothing`,
`every_heading_sits_above_the_column_its_rows_use`,
`the_header_menu_offers_newest_first_where_it_used_to_offer_nothing`,
`an_unsorted_list_puts_the_newest_songs_at_the_top`,
`a_flow_offset_is_a_clamped_setting_that_starts_at_none`.

**Check after an update:** open a playlist and every heading sits above its own
column; right-click the header and there is no "Sort by" line and no lowercase
option; clear the sort and the newest song is at the top; drag a divider and
you can see the grip; hover a long song name in the queue and it scrolls.

### 24. A tempo Deezer actually has, a shorter card, and a queue that can say no to the clock

**Deezer does not put the tempo on an ISRC lookup.** This is why the column
was empty for everything. `https://api.deezer.com/track/isrc:GBARL9300135`
answers with the whole track record *except* `bpm` and `gain`; only
`https://api.deezer.com/track/15646529` carries them. So the lookup is now two
requests: read the ISRC body, take the numeric `id` out of it
(`bpm::track_id`), then read `bpm` from that track's page (`bpm::track_url`).
A body with no `id` at all is Deezer saying it has never heard of the
recording, which is an answer and is cached as one.

**The poisoned cache was thrown away.** Every recording the broken lookup
touched had been written down as `null` — "asked, and there is none" — and
that would have been trusted for the life of the profile. The cache file is
now `{"version":2,"tempos":{…}}` and anything that is not version 2 is
discarded on load. Most current indie tracks genuinely have no Deezer tempo
(`"bpm":0`); the ones that do now show one.

**The now-playing card is a caption again.** 30% of the art (64-104px rather
than 76-132), a title at 34% of the card, and the heart back in plain sight in
its own bottom-right corner — it is the one control on the card, and hiding it
until hover hid it from anyone who never pointed at it. Only the artists line
is still the hover's job.

**The queue's length is a right-click.** `queue_show_time` (on by default)
sits beside the existing names-or-full-row switch. Turning it off takes the
column's width straight back and gives it to the song name, which is the only
thing in a queue row anybody reads. `TrackRow` gained `show_time` for it.

**The playlist icons wait for real room.** 104px of name was not enough: a
narrow panel still produced "HAUNT". They now also need 62% of the name column
to survive them.

**ADDED BY is gone.** Nobody reads who added a song to a playlist they own,
and its width came out of the song name. `TrackColumns::added_by` is kept only
so an older settings file still deserialises; `width()` and `set()` treat it
as zero, and it is out of `SORTABLE` and `RESIZABLE`. The header menu is also
a row taller for less, at one pixel of spacing between items.

Tests: `an_isrc_answer_hands_over_the_id_the_tempo_is_read_from`,
`the_queue_time_is_a_setting_and_the_added_by_column_is_gone`.

### 25. An inspector, gentler swirls, three clicks on the art, and no waveform

**You can now ask the app what it is drawing.** Right-click the player bar
and choose `Inspect what I am hovering`. A panel opens on the right listing
the egui layer under the pointer and every string drawn there with its
position; `Copy` puts it on the clipboard. `theme::text` and `theme::link`
call `inspect::note` as they draw, and nothing is recorded while it is off.
This is the difference between "the BPM column is empty" and "the BPM column
is drawing something that is not a number".

**Swirl is a wallpaper, not a pinwheel.** `TURNS` was 1.5 and `PULL` 0.55,
which wound the corners so far that a cover became a repeating row of
spirals. They are 0.8 and 0.3 now, the warp is 160px across, and the left
and right edges of the warped image are cross-faded over 12 pixels into each
other — the picture is tiled along the bar, and a seam every tile-width is
what made it read as blobs.

**Waveform is gone.** It was the one shape that was a line through the sound
rather than a picture of it, its four settings changed nothing anyone could
see, and it was the row that made the vis menu tall. A profile that still
says Waveform draws Swirl.

**The vis menu is a menu again.** `ui.set_max_width(320.0)` stopped it
stretching to the window's width, and `slider_row` lays the track and the
number field out in one `ui.horizontal` — it was allocating a full-width row
and then a field after it, which is why the number sat *under* its own
slider.

**The album art answers three questions.** Left click finds the song in your
own library through the membership index and opens that playlist scrolled to
the row (`Action::RevealSong`, `App::reveal_song`, consumed in
`collection::table` before the virtual rows are drawn, because a row at
position 900 cannot be scrolled to after the fact). Middle click opens the
album. Right click is the song's menu, as everywhere else.

**The card is see-through and the heart works.** `palette.panel` at 0.82
rather than solid, the title in the semibold face, and — the actual bug — the
card's hover response was asking for `Sense::hover`, which does not take
clicks, but the heart was drawn inside a child that never got its own
interact. The heart is now clickable wherever it sits.

**Grid names fill their cards**, at 15.5% of the card width up to 34pt
instead of 12.5% up to 26. **Cards across** is in the library's Layout menu:
Auto, or pinned at 2 to 6. **Create playlist** is now in a playlist's own
right-click menu in the sidebar, not only in the library heading's.

Tests: `nothing_is_recorded_until_it_is_armed`,
`a_pinned_library_grid_keeps_its_columns_whatever_the_width`.

### 26. No tray, a selector on Shift+Q, F for the whole window, and a swirl that is one picture

**No tray item, ever, unless something asks for one by name.** The Windows
tray icon is created by `fastframe-tray` and removed only when the process
exits cleanly; a forced kill leaves an orphan in the notification area that
Explorer keeps until it is hovered. `AppOptions::tray` now defaults to
`false`, so nothing registers one. With no tray, X and Alt+F4 close the app
outright, which `app.rs` already did — `close_requested` sets
`quit_requested` rather than hiding.

**Shift+Q opens the selector**: one panel of every switch that decides
whether something is on screen at all — Fullscreen visualizer (F), Sidebar
(B), Queue panel (Q), Lyrics panel (L), Inspect hover. Each row names its
key, so the panel teaches the hotkeys by using them. Plain Q is still the
queue panel and still does not fire for Shift+Q.

**F gives the visualizer the whole window.** `App::fullscreen_vis` makes
`ui::show` draw the player bar's panel at full window height and return
before the sidebar, the queue, the lyrics and the controls. The click still
opens the visualizer menu, and pressing F again puts the window back exactly
as it was.

**The swirl is one picture now.** It was drawing a bar-height *tile* of the
warped cover repeatedly across the bar, which is why it read as one distorted
image smeared a dozen times rather than a swirl: the eye locks onto the
repeat. It now draws the warped cover **once**, stretched across the full
width, and slides it by its own width using the texture's `uv` rect — a
second copy a width to the left covers the wrap. Because `swirl_art` makes
the warped image's left and right edges meet, the join is invisible.

Tests: `this_fork_never_registers_a_tray_item`,
`the_selector_key_is_not_the_queue_key`.

### 27. Scenes: nightcore, hardtekk, and one-key curves

**Speed is a resampler, because there is nothing else.** librespot has no
rate control and Spotify's API has none either, so `src/speed.rs` does what
every player does: reads the samples faster than they arrive. That moves the
tempo *and* the pitch together, which is the sound those scenes actually
are — a hardtekk track at 1.35x with the bass up is a genre, not a mistake.
`Resampler` keeps the samples either side of the read position between
packets and interpolates with a Catmull-Rom spline, so a packet boundary is
not a click. At 1.0 it is the identity, so a reader who never touches it pays
nothing.

**A scene is a rate and a curve, because that is how it is heard.** Nine of
them, in `speed::PRESETS`: Normal, Warm, Punch, Nightcore (1.25 + a laptop
curve), Hardtekk (1.35 + Techno), Gabber (1.5 + Club), Chipmunk (1.75 +
Loudness), Slowed (0.8 + Soft), Half speed (0.6 + Soft Rock). Each carries the
name of a real `eq::PRESETS` curve, and `every_preset_names_a_curve_and_a_speed`
holds them to that.

**Reachable three ways, because experimenting costs nothing.** `Shift+S`
walks the scenes and wraps, so one more press is always Normal. The player
bar's right-click menu lists them under `SCENES` with the current one ticked,
then a bare rate slider for anyone who wants 1.15, then an `EQUALIZER` list
of curves on their own. `App::apply_eq_preset` turns the equalizer *on* — a
preset that does nothing because the reader never opened the EQ window is the
most disappointing thing a preset can do.

The rate is read from a `SharedSpeed` handle **once per packet** on the
audio thread, never per sample: a lock at 44kHz is an outage, a lock at
packet rate is free.

Tests: `one_to_one_is_the_identity`,
`a_faster_speed_returns_proportionally_fewer_samples`,
`a_packet_boundary_does_not_discontinuity`,
`changing_speed_mid_stream_keeps_playing`, `an_impossible_speed_is_normal_speed`,
`every_preset_names_a_curve_and_a_speed`,
`a_speed_scene_is_a_rate_and_a_curve_together`.

## Known gaps

- **No track prefetching.** Songs are fetched when you press play, so a first
  play of an uncached song has some latency. Everything relevant is already
  on (`bitrate: 320`, `gapless: true`, a 1 GB audio cache). The one setting
  worth trying is `audio_buffer_ms` in the config, currently 100.
- **The membership walk is per launch until it completes.** It resumes from
  the cache, so later launches skip finished playlists. It now deliberately
  waits for the whole playlist list before starting, so a first open is
  fast and the icons arrive a moment later.

Show, on each song row, which of the user's own playlists contain that song,
as small cover thumbnails between TITLE and ALBUM. The sidebar already draws
playlist covers, so that rendering is reused.

Shape: `my_playlists()` then `playlist_items()` for each, cached as
`track_uri → [playlist]` on the backend runtime, never the UI thread, loaded
lazily so a playlist opens instantly. Lands in `TrackRow`
(`src/ui/widgets.rs`) and renders in `src/ui/collection.rs`.

## Where upstream fights back

**The top bar's width budget is guarded by a test.**
`demo::tests::the_top_bar_badges_never_cover_the_search_field` asserts the
device and update badges never overlap the search field, at every width from
760px up. The build pill broke this on the first attempt: it took width from
the search field, pushing the field left under the badge. The pill has since
moved out of the bar entirely, but any future addition to the top bar must
respect that test.

**The build script is load-bearing.** `build.rs` now emits two environment
variables that `src/build_info.rs` reads with `env!`. If a future upstream
`build.rs` rewrite drops `build_stamp()`, the crate fails to compile rather
than degrading — which is the failure we want, but expect it.

**The dependency pins are fragile.** `Cargo.toml` tracks `crmne/fastframe` by
git tag and patches egui and winit to maintainer forks through
`[patch.crates-io]`. Cargo honours that section only in the root workspace, so
this fork must carry its own. Upstream admits in fastframe's README that a
`forks.toml` to centralise the pins "does not exist yet". If `cargo build`
starts complaining about egui or winit revisions, that is why.

## Rebasing onto an update

```sh
git fetch origin
git rebase origin/main      # or: git merge origin/main
source /c/Users/Chance/AppData/Local/Temp/spotifast_env.sh
cargo build --locked --no-default-features
cargo test --locked --no-default-features --lib
```

Then walk the list above and confirm each feature still behaves. Features to
watch, in order of how easily upstream breaks them:

1. Top bar layout — most fragile, the badge test guards it.
2. About section — cosmetic, upstream rewrites this text freely.
3. Build stamp — only breaks if `build.rs` stops emitting the variables.
4. Number-key seek — upstream rewrites the whole key table most often, so the
   bindings are the part to re-check. The `Action` variant survives longer.
5. Display name — the toast strings are asserted by two tests in `app.rs`, so
   a rename shows up as a test failure rather than a silent regression.
   `tests/branding.rs` guards the parts that must *not* change.
6. Playlist membership icons — the most upstream-facing change here: a new
   module, a new backend request, two `Library` fields and new row drawing.
   `demo::tests::a_row_shows_the_playlists_its_song_is_in` covers the drawing;
   the `membership` unit tests cover the mapping.

## Working notes

`core.autocrlf` is `true` here, so `git status` shows packaging scripts and
`.github` workflows as modified with `0 insertions, 0 deletions`. Those are
line-ending artefacts, not edits. Check `git diff --numstat` before believing
a dirty file.

Upstream AGENTS.md forbids broadening a task, adding a browser engine or
telemetry, and blocking the UI thread. New work here stays inside those rules,
which is also why the playlist index belongs on the backend runtime.