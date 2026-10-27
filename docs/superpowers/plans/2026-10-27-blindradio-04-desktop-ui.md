# BlindRadio Plan 4: Desktop UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The whole YARSP-quality desktop interface over the core: main window with menus, Search, Browse, Favorites, Recently Played, Collections, custom stations, playlist import and export, context menus, Copy Stream URL, Song History, Sleep Timer, Equalizer, audio device menu, settings, every app key, and dial tuning in every station list.

**Architecture:** `crates/blindradio` replaces the plan 1 test shell. One `App` owns the core objects (engine, output, tuner, sources, library, settings, sleep timer, history) and executes `Action`s; menus, buttons and keys only produce `Action`s. Async work (directory calls) runs on the engine runtime and hands results back to the UI thread through a channel drained by the 50 ms poll timer. Each window is a real modal dialog, one at a time; a shared station list control gives every list dial tuning, letter jump, Enter, Delete and the context menu.

**Tech Stack:** Rust 2024, wxDragon (git main), wx-utils (git main: dialog footers, `confirm`, `prompt_text`, `menu_label`), blindradio-core (plans 1 to 3).

**Spec:** `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md` (Dial tuning behaviour, Feature list, YARSP UI rules, Terse language). The detailed behaviour spec is the YARSP manual: `docs/reference/yarsp-manual.html` — read it in full before any UI task; where this plan is silent, it decides.

## Global Constraints

- Native wx controls only; every control has a short accessible name (a `StaticText` label created immediately before the control it names, as plan 1 did for the list).
- Real modal dialogs: Escape closes, Enter presses the default button. Windows open quietly (no app speech on open). One window open at a time; a window's own shortcut inside it moves focus to its first control instead of reopening; another window's shortcut closes this one and opens that one.
- Menus open on their first item. A letter in a list jumps to the next item starting with it and never presses a button; buttons use Alt plus their underlined letter (F is Favorites everywhere).
- Focus returns to the control last used when a window regains focus. The app starts with focus on Play.
- App keys work in every window (main and dialogs, via a `CHAR_HOOK` handler on each top-level window): Ctrl+P play or pause, Ctrl+S stop, Ctrl+M mute, Ctrl+Up / Ctrl+Down volume (5 points), Ctrl+D add or remove favorite, Ctrl+F Search, Ctrl+G Browse, Ctrl+B Favorites, Ctrl+H Recently Played, Ctrl+N custom station, Ctrl+U copy stream URL, Ctrl+Y song history, Ctrl+L Collections, Ctrl+E Equalizer, Ctrl+1 to Ctrl+6 sleep 15/30/45/60/90/120 min, Ctrl+0 sleep off, F1 guide, Escape close. Main window only: Ctrl+I import, Ctrl+Shift+I import as collection, Ctrl+Shift+L add playing to a collection, Alt+1 to Alt+0 play favorites 1 to 10, Alt+V volume slider.
- Dial tuning (spec "Dial tuning" behaviour list) in every station list: Search results, Browse, Favorites, Recently Played, Collections. Only user-driven focus moves tune; programmatic selection never does. Opening a list and landing on a station plays it at once.
- Terse strings, exactly as written in this plan; messages are shown in the main window's message line and also spoken (speech arrives in plan 5 through `App::say`; until then `say` only shows).
- `BLINDRADIO_DATA` environment variable, when set, overrides the data folder (tests).

## Review Focus

1. Holding Down through a Browse branch while its children are still loading: no stale station plays, focus is never moved by the loader, and the loaded items appear without stealing focus. Test: `browse_load_does_not_move_focus` (Task 4).
2. A directory call that fails while a window is open: the window stays usable and says `Failed: <directory>` once. Test: `browse_failure_message` (Task 4).
3. Closing a dialog while an async result for it is in flight: the result is dropped, nothing touches the destroyed window. Test: `late_result_after_close_dropped` (Task 1).
4. App keys pressed inside a text field (Search name box): Ctrl+P etc. still act; plain letters still type. Test: `app_keys_in_text_field` (Task 3).
5. Deleting or reordering in Favorites or Collections: focus stays on a sensible neighbour and the station playing does not change. Test: `favorites_delete_keeps_focus_and_audio` (Task 5).

---

### Task 1: App core, actions and the async bridge

**Files:**
- Create: `crates/blindradio/src/app.rs`, `crates/blindradio/src/action.rs`, `crates/blindradio/src/bridge.rs`, `crates/blindradio/src/strings.rs`
- Modify: `crates/blindradio/src/main.rs`, `crates/blindradio/Cargo.toml` (wx-utils git dependency), `crates/blindradio-core/src/tuner.rs` (`list_closed`)

**Interfaces:**
- Consumes: core `Engine`, `Output`, `Tuner`, `LiveFactory`, `Sources`, `Library`, `Settings`, `SleepTimer`, `SessionHistory`, `store::data_dir`.
- Produces:
  - `pub enum Action` covering every menu item, button and key in this plan (Play, PlayPause, Stop, Mute, VolumeUp, VolumeDown, SetVolume(u8), ToggleFavorite, OpenSearch, OpenBrowse, OpenFavorites, OpenRecent, OpenCollections, NewCustomStation, CopyUrl, SongHistory, AddToCollection, Import, ImportAsCollection, Sleep(u32), SleepOff, Equalizer, PlayFavorite(usize), SetDevice(Option<String>), ToggleSetting(SettingId), Help, Exit, plus later tasks' additions).
  - `pub fn app_key(code: i32, ctrl: bool, alt: bool, shift: bool, main_window: bool) -> Option<Action>` (the key table above; pure, unit tested).
  - `pub struct App` (lives in `Rc<RefCell<App>>` on the UI thread) with `pub fn run(&mut self, a: Action)`, `pub fn say(&mut self, text: &str)` (sets the main window message line; plan 5 adds speech), `pub fn selected_or_playing(&self) -> Option<Station>` (the station focused in the open list, else the playing one), `pub fn poll(&mut self)` (50 ms: tuner poll, bridge results, sleep timer, device poll every 2 s via `Output::poll_devices`).
  - `bridge::Bridge` with `pub fn spawn<T: Send + 'static>(&self, owner: WindowToken, fut: impl Future<Output = T> + Send + 'static, done: impl FnOnce(&mut App, T) + 'static)`; results are delivered in `App::poll` only if `owner` is still open (`WindowToken` is invalidated when its window closes).
  - Core: `Tuner::list_closed(&mut self)` drops every open stream except the playing one (spec: warm connections close when the list closes).
  - `strings.rs`: every user-visible string as a `pub const` (terse, one place, ready for translation later).
  - Tuner event mapping in `App::poll`: `Playing` says `<station>` only if Announce Connection Status is on and the play did not come from dial tuning; `Reconnecting` says `Connection lost. Reconnecting.`; `Reconnected` says `Reconnected`; `Failed` says `Failed: <name>` (always); `Announce` says the song if Announce Song Titles is on; `Title` sets the window title `<song> - <station>` or `<station>` and feeds `SessionHistory`.

- [ ] **Step 1: Write failing tests** (`#[cfg(test)]` in `action.rs` and `bridge.rs`): `app_key_table` (every key in Global Constraints maps to its action; Ctrl+I is `None` outside the main window; plain letters are `None`); `late_result_after_close_dropped` (a bridge result whose token was invalidated never calls `done`). In core `tests/tuner.rs`: `list_closed_keeps_only_playing`.
- [ ] **Step 2: Run** `cargo test -p blindradio -p blindradio-core --test tuner`. Expected: FAIL.
- [ ] **Step 3: Implement.** Data folder: `BLINDRADIO_DATA` else `store::data_dir()`. Resolve cache path `<data>/resolve.json`.
- [ ] **Step 4: Run** same. Expected: pass.
- [ ] **Step 5: Commit** `feat(desktop): app core, actions and async bridge`.

### Task 2: Main window and menus

**Files:**
- Create: `crates/blindradio/src/main_window.rs`, `crates/blindradio/src/menus.rs`

**Interfaces:**
- Produces: the `Frame` titled `BlindRadio` (then `<song> - <station>` or `<station>` while playing) with:
  - Menu bar (Alt+F, Alt+S, Alt+P, Alt+H): **File**: Announce Song Titles, Announce Connection Status, Close Lists When Playing, Play Last Station at Startup (check items), Audio Device (submenu: `Default` then every `output_devices()` name, radio items, the active one checked), Global Hotkeys… (plan 5), Exit. **Stations**: Search… Ctrl+F, Browse… Ctrl+G, Favorites… Ctrl+B, Recently Played… Ctrl+H, Collections… Ctrl+L, Add Custom Station… Ctrl+N, Import Playlist… Ctrl+I, Import Playlist as Collection… Ctrl+Shift+I, Copy Stream URL Ctrl+U. **Playback**: Play/Pause Ctrl+P, Stop Ctrl+S, Mute Ctrl+M, Volume Up Ctrl+Up, Volume Down Ctrl+Down, Add to Favorites / Remove from Favorites Ctrl+D (label flips), Add to Collection… Ctrl+Shift+L, Song History… Ctrl+Y, Sleep Timer (submenu: 15 Minutes Ctrl+1, 30 Minutes Ctrl+2, 45 Minutes Ctrl+3, 1 Hour Ctrl+4, 90 Minutes Ctrl+5, 2 Hours Ctrl+6, Off Ctrl+0), Equalizer… Ctrl+E. **Help**: User Guide F1, Check for Updates (plan 6), About BlindRadio.
  - Panel: now-playing text (`Stopped`, or `<station>` / `<song> - <station>`; changes only when the station or song changes), buttons Play (`&Play` / `&Pause` as state changes? no: label `Play` and `Pause` without Alt keys, per YARSP "main window buttons carry no Alt keys"), Stop, Mute (`Mute` / `Unmute`), Add to Favorites / Remove from Favorites, a `Volume` slider 0 to 100 (Alt+V focuses it; Up and PageUp raise, Down and PageDown lower, by 5 for arrows and 10 for pages), and the message line (last message, stays until the next one).
  - Startup: focus on Play; Play Last Station at Startup plays `settings.last_station`; settings saved on every change; volume and mute restored.
  - Play/Pause: playing → stop (remember the station); stopped → play the remembered station, else the selected one, else say `Nothing to play`.

- [ ] **Step 1: Write failing tests** for the pure parts: `menus_have_every_shortcut` (the menu model in `menus.rs` is a data table; every Global Constraints key appears with its exact label), `favorite_label_flips`, `play_pause_rules` (pure function over (playing, remembered, selected)).
- [ ] **Step 2: Run** `cargo test -p blindradio`. Expected: FAIL.
- [ ] **Step 3: Implement.** Build the menu bar from the table with `wx_utils::menu_label`.
- [ ] **Step 4: Run** same; then `cargo run -p blindradio --release` and check with NVDA: title, menus open on the first item, focus on Play, volume slider direction.
- [ ] **Step 5: Commit** `feat(desktop): main window and menus`.

### Task 3: Station list control and Search

**Files:**
- Create: `crates/blindradio/src/station_list.rs`, `crates/blindradio/src/dialogs/mod.rs`, `crates/blindradio/src/dialogs/search.rs`, `crates/blindradio/src/context_menu.rs`

**Interfaces:**
- Produces:
  - `StationList` over a report-mode `ListCtrl` with columns Name, Info, Directory (`Station::source.label()`): `set_stations(&self, Vec<Station>, focus: usize)` (programmatic, never tunes), user selection change calls `Tuner::focus(&stations, index, now)`, Enter plays (`App::run(Action::Play)` on that station; closes the window when Close Lists When Playing is on), letter jump to the next name starting with it, Delete calls the owner's `on_delete`, Alt+Up / Alt+Down call `on_move`, Applications key / Shift+F10 opens the context menu.
  - Context menu for a station: Play, Add to &Favorites / Remove from &Favorites, Copy Stream URL, Add to Collection ▸ (each collection ticked if it holds the station, toggling; separator; New Collection…), and Delete Custom Station for `custom:` stations. On something else: say `Nothing to do here` (no menu).
  - Search dialog (Ctrl+F): `Name` text box, `Genre` and `Country` editable combo boxes and `Language` combo (filled from `Sources::filters()` once per run), Search button (default), the results `StationList` labelled `Results`, Add to Favorites button, Close. Search runs through the bridge; while running, the message line says `Searching`; results replace the list and focus lands on the first result, which plays; no results says `No results`.

- [ ] **Step 1: Write failing tests:** `context_menu_items_for` (pure: station, custom station, collection, other → item lists or `Nothing to do here`); `letter_jump_wraps` (pure next-index function); `app_keys_in_text_field` (the dialog's CHAR_HOOK routes Ctrl+P to `Action::PlayPause` and passes a plain `a` through to the text box: test the routing function).
- [ ] **Step 2: Run** `cargo test -p blindradio`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same; NVDA check: search "jazz", land on the first result playing, arrow to tune, Applications key menu, Escape.
- [ ] **Step 5: Commit** `feat(desktop): station lists, context menu and search`.

### Task 4: Browse

**Files:**
- Create: `crates/blindradio/src/dialogs/browse.rs`

**Interfaces:**
- Consumes: `Sources::roots/children`, `Library` (Collections and Custom Stations sections), `StationList` behaviours (tuning, Enter, Delete, context menu) reused for tree station items.
- Produces: Browse dialog (Ctrl+G) with a `TreeCtrl` labelled `Stations`: roots Radio Browser, TuneIn, iHeartRadio, Networks, Collections (each collection a folder of its stations), Custom Stations. Children load lazily through the bridge on expand (a placeholder child `Loading` until they arrive; the loader never moves focus). Right opens, Left closes, letters jump, Enter plays a station or, on Load More, loads the next page and moves focus to its first new station. Home / End go to the first / last item of the current level; Ctrl+Home / Ctrl+End to the top / bottom of the whole tree. Focus on a station tunes (dial tuning). Delete: own station deletes it; a station in a collection is removed from it; a directory station says `Only your own stations can be deleted.`. The last focused node path is remembered for the session; reopening expands that path and focuses it, falling back to the nearest existing ancestor. A failed load says `Failed: <directory>` once and leaves the folder collapsed so Right retries.

- [ ] **Step 1: Write failing tests** for the pure tree-state model (`BrowseModel`, no wx): `home_end_stay_on_level`; `load_more_focus_first_new`; `remember_path_falls_back_to_ancestor`; `browse_load_does_not_move_focus` (model receives children for a node while focus is elsewhere: focus unchanged); `browse_failure_message`.
- [ ] **Step 2: Run** `cargo test -p blindradio`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same; NVDA check through each root.
- [ ] **Step 5: Commit** `feat(desktop): browse tree`.

### Task 5: Favorites, Recently Played and custom stations

**Files:**
- Create: `crates/blindradio/src/dialogs/favorites.rs`, `crates/blindradio/src/dialogs/recent.rs`, `crates/blindradio/src/dialogs/custom_station.rs`

**Interfaces:**
- Produces:
  - Favorites (Ctrl+B): `StationList` labelled `Favorites`, buttons &Remove, Move &Up, Move &Down, &Export…, Close; Delete removes, Alt+Up/Down reorder (focus follows the moved station), Export writes an M3U via a save file dialog (default name `Favorites.m3u`) and says `Exported <n>`.
  - Recently Played (Ctrl+H): `StationList` labelled `Recently played`, Add to &Favorites, Close.
  - Every list window's Add to &Favorites / Remove from &Favorites button acts on the focused station and flips its label; with no list open Ctrl+D acts on the playing station. Says `Added to favorites` / `Removed from favorites`.
  - Alt+1 to Alt+0 on the main window play favorites 1 to 10 (`No favorite <n>` if missing).
  - Add Custom Station (Ctrl+N): `Name` and `Stream URL` fields, OK (default) and Cancel (wx-utils footer); a URL that is not `http(s)://` says `Not a stream URL` and keeps the dialog open; OK adds via `Library::add_custom` and says `Added <name>`.
  - Every play from a list adds to Recent (`Library::add_recent`) and saves.

- [ ] **Step 1: Write failing tests** (pure helpers): `favorites_delete_keeps_focus_and_audio` (removing index i focuses min(i, len-1) and does not call the tuner), `move_keeps_focus_on_moved`, `custom_url_validation`.
- [ ] **Step 2: Run** `cargo test -p blindradio`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same; NVDA check.
- [ ] **Step 5: Commit** `feat(desktop): favorites, recent and custom stations`.

### Task 6: Collections, Add to Collection, import and export

**Files:**
- Create: `crates/blindradio/src/dialogs/collections.rs`, `crates/blindradio/src/dialogs/add_to_collection.rs`

**Interfaces:**
- Produces:
  - Collections (Ctrl+L): `TreeCtrl` labelled `Collections` (Custom Stations first, then each collection, stations as children; Right opens, Left closes, Enter on a station plays, focus on a station tunes), buttons &New, &Rename (F2), &Delete (Delete key on a collection, asks `Delete <name>?` with wx-utils `confirm`), Remove &Station (Delete key on a station; on Custom Stations deletes it), Move &Up, Move &Down (Alt+Up/Down in the tree, stations within a collection and collections among themselves, Custom Stations stays first), &Export… (the collection under focus, default name `<collection>.m3u`), Close. Renaming or deleting Custom Stations says `Custom Stations cannot be changed`. Applications key on a collection: Rename, Delete, Add All to Collection ▸; on Custom Stations: Add All to Collection ▸ only.
  - Add to Collection (Ctrl+Shift+L for the playing station, and the context menu item for a selected one): a popup menu of collections, ticked where the station already is, choosing toggles (`Added to <collection>` / `Removed from <collection>`), then New Collection… (wx-utils `prompt_text` `Collection name`, then adds the station). Add All to Collection says `Added <n> to <collection>`.
  - Import Playlist (Ctrl+I) and Import Playlist as Collection (Ctrl+Shift+I): open file dialog filtered to `Playlists (*.m3u;*.m3u8;*.pls)`, then `import_playlist`, then say `ImportReport::message()`.

- [ ] **Step 1: Write failing tests** (pure helpers over `Library`): `collections_tree_order`, `custom_cannot_change_message`, `add_to_collection_menu_ticks`.
- [ ] **Step 2: Run** `cargo test -p blindradio`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same; NVDA check including import of `crates/blindradio-core/tests/fixtures/import/*.m3u`.
- [ ] **Step 5: Commit** `feat(desktop): collections, import and export`.

### Task 7: Copy URL, Song History, Sleep Timer, Equalizer, devices and settings

**Files:**
- Create: `crates/blindradio/src/dialogs/history.rs`, `crates/blindradio/src/dialogs/equalizer.rs`, `crates/blindradio/src/clipboard.rs`

**Interfaces:**
- Produces:
  - Copy Stream URL (Ctrl+U): `selected_or_playing()`'s URL to the clipboard, read back to verify: `Copied` or `Copy failed`; nothing selected or playing: `Nothing to copy`.
  - Song History (Ctrl+Y): the heading line from `history::heading`, a list labelled `Songs` (one column, or Song and Played when published times exist, `history::ago`), &Copy button and Enter copy the focused song (`Copied`), Close. Published history fetched through the bridge for the playing station (`Sources::history`), else the session list. Not playing: `Nothing playing`.
  - Sleep Timer: menu items and Ctrl+1..6 / Ctrl+0 call `SleepTimer::set/cancel` and say the returned text; when due and something is playing, stop and say `Sleep timer: stopping`; if already stopped, end silently. Station changes and stop/play do not reset it.
  - Equalizer (Ctrl+E): `Preset` choice (labels from `EqPreset::label`), `Bass (dB)`, `Mid (dB)`, `Treble (dB)` sliders -12 to 12 (arrows 1 dB, Home/End ends, value spoken as dB by the slider's own value text `<n> dB`), `Enabled` checkbox, Close. Choosing a preset moves the sliders and says `eq::describe`; moving a slider sets the preset to Custom. Every change sends `MixerCmd::Eq` and saves settings.
  - Audio Device submenu: choosing calls `Output::switch`, saves `settings.audio_device`; device events say `Device lost. Using default.` and `Using <name>`; a `rate_changed()` restarts the playing station.
  - Settings check items toggle and save at once.

- [ ] **Step 1: Write failing tests** (pure helpers): `copy_verify_messages`, `history_columns_for_kind`, `sleep_due_while_stopped_is_silent`, `slider_moves_preset_to_custom`.
- [ ] **Step 2: Run** `cargo test -p blindradio`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same; NVDA check of each.
- [ ] **Step 5: Commit** `feat(desktop): copy URL, song history, sleep timer, equalizer, devices`.

### Task 8: UI automation tests and manual NVDA test plan

**Files:**
- Create: `crates/blindradio/tests/ui.rs`, `doc/nvda-test-plan.md`

**Interfaces:**
- Consumes: the built app (`target/release/blindradio.exe`), `BLINDRADIO_DATA` pointing at a temp folder seeded with a `library.json` of custom stations served by a local test HTTP server started by the test (copy the plan 1 `tests/common` server into this crate's tests or depend on it as a path module).
- Produces: `#[ignore]` (needs a desktop) UIA tests with the `uiautomation` crate (Paperback's approach): `starts_on_play_button`; `search_dialog_opens_and_escape_closes` (name contains `dialog` role); `favorites_dial_tunes_on_arrow` (two custom favorites; arrowing changes the window title to the second station within 2 s); `context_menu_on_category_says_nothing_to_do` (Browse root folder, Applications key, message line reads `Nothing to do here`, no menu window); `one_window_at_a_time` (Ctrl+F then Ctrl+B: only Favorites open). `doc/nvda-test-plan.md`: a numbered manual pass over every feature in this plan with the expected speech, terse.

- [ ] **Step 1: Write the tests and the test plan.**
- [ ] **Step 2: Run** `cargo build -p blindradio --release` then `cargo test -p blindradio --test ui -- --ignored --test-threads=1`. Expected: pass.
- [ ] **Step 3: Commit** `test(desktop): UI automation and NVDA test plan`.
