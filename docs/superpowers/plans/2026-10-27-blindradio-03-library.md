# BlindRadio Plan 3: Library, Settings and Audio Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Everything the user owns and sets, kept in plain JSON in the right folder (portable or installed): favorites, recently played, custom stations, collections, playlist import and export, settings, the session song history, the sleep timer, the three-band equalizer and output device choice with fallback.

**Architecture:** Pure data types with their rules in `blindradio-core` (no UI), each persisted through one small atomic JSON store. Audio controls extend the plan 1 mixer and output: an EQ stage inside `MixerCore::render`, and `Output` learning to list devices, switch at runtime and fall back when one disappears. The desktop UI in plan 4 calls these and saves after each change.

**Tech Stack:** Rust 2024, serde/serde_json, `dirs` for per-user folders, cpal, uuid. Nothing else new.

**Spec:** `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md` (Feature list: Custom stations, Favorites, Recently played, Collections, Playlist import, Song history, Sleep timer, EQ, Settings; Audio: output device fallback, EQ; Data and delivery: portable mode, JSON). Behaviour reference: `docs/reference/yarsp-manual.html` (Favorites, Importing, Exporting, Adding your own stations, Collections, Song history, Sleep timer, Equalizer, Choosing a sound card, Settings).

## Global Constraints

- Edition 2024, hard tabs, dependencies `version = "*"`, `cargo update` before each task's first build. No wx dependency in `blindradio-core`.
- Storage: plain JSON files in the data folder. Portable: a `data` folder (or a `portable.txt` marker) next to the executable means portable and data lives in `<exe dir>/data`. Installed: `dirs::config_dir()/BlindRadio` (Linux: `blindradio` lowercase). macOS always uses the per-user folder.
- A missing or corrupt file never loses the user's other data and never stops the app: it loads as empty or default, and a corrupt file is kept beside it as `<name>.bad`.
- Every user-visible string terse, exactly as given in the tasks (YARSP wording where a task quotes it).
- Stations are matched by `id` everywhere.

## Review Focus

1. A save interrupted mid-write (crash, power loss): the previous file is intact, never truncated. Test: `save_is_atomic` (Task 1).
2. A playlist file that is UTF-8 with BOM, Latin-1, or uses CRLF, or lists the same URL twice: names read correctly, the duplicate counts as Had. Test: `import_counts_duplicates_and_encodings` (Task 4).
3. Custom Stations collection: cannot be renamed, deleted or reordered away from first; removing from it deletes the station. Test: `custom_collection_rules` (Task 3).
4. EQ at +12 dB on all bands with a full-scale stream: output never exceeds ±1.0. Test: `eq_never_clips` (Task 7).
5. The chosen audio device unplugged mid-play: playback continues on the default within about 2 s and returns to the chosen device when it is back, with one announcement each way. Test: `device_fallback_and_return` (Task 8, against a fake device list).

---

### Task 1: Data folder and JSON store

**Files:**
- Create: `crates/blindradio-core/src/store.rs`
- Modify: `crates/blindradio-core/src/lib.rs` (module; `Engine::new` callers keep taking a path)

**Interfaces:**
- Produces:
  - `pub fn resolve_data_dir(exe_dir: &Path, config_dir: Option<PathBuf>) -> PathBuf` (pure rule above; `config_dir` is `dirs::config_dir()` in real use) and `pub fn data_dir() -> PathBuf` (uses `std::env::current_exe()`, creates the folder).
  - `pub fn is_portable() -> bool`.
  - `pub fn load<T: DeserializeOwned + Default>(path: &Path) -> T`: missing gives `T::default()`; unreadable or invalid JSON renames the file to `<file>.bad` (replacing an older `.bad`) and gives the default.
  - `pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()>`: pretty JSON to `<file>.tmp` in the same folder, flushed, then renamed over the target.
  - File names (used by later tasks and plan 4): `settings.json`, `library.json`, `resolve.json`.

- [ ] **Step 1: Write failing tests** in `store.rs`: `portable_when_data_folder_or_marker`; `installed_uses_config_dir` (`<config>/BlindRadio`, `blindradio` on Linux); `load_missing_is_default`; `corrupt_file_kept_as_bad`; `save_is_atomic` (a leftover `.tmp` from a crash does not affect `load`; the target is replaced only by a complete file).
- [ ] **Step 2: Run** `cargo test -p blindradio-core store`. Expected: FAIL.
- [ ] **Step 3: Implement** with `dirs = "*"`.
- [ ] **Step 4: Run** same. Expected: pass.
- [ ] **Step 5: Commit** `feat(core): data folder and JSON store`.

### Task 2: Settings

**Files:**
- Create: `crates/blindradio-core/src/settings.rs`

**Interfaces:**
- Produces: `#[serde(default)] pub struct Settings` (`Clone, Debug, PartialEq, Serialize, Deserialize`, `Default` with these values): `announce_titles: bool` (true), `announce_status: bool` (true), `close_lists_when_playing: bool` (false), `play_last_at_startup: bool` (false), `audio_device: Option<String>` (None = default device), `volume: f32` (0.8, clamped 0 to 1 on load), `muted: bool` (false), `eq: EqSettings` (Task 7 type: `enabled` false, preset Flat, all bands 0), `hotkeys: BTreeMap<String, String>` (action id to chord text; empty means defaults), `last_station: Option<Station>`. `pub fn load(dir: &Path) -> Settings` and `pub fn save(&self, dir: &Path) -> Result<()>` over `settings.json`.
- Define `EqSettings { pub enabled: bool, pub preset: EqPreset, pub bass: i8, pub mid: i8, pub treble: i8 }` and `EqPreset { Flat, BassBoost, TrebleBoost, Loudness, Voice, Warm, Custom }` here (serde as kebab-case strings); Task 7 adds their DSP.

- [ ] **Step 1: Write failing tests:** `defaults_match_spec`; `unknown_and_missing_fields_tolerated` (`{"volume":2.0,"future":1}` loads with volume 1.0 and defaults elsewhere); `round_trip`.
- [ ] **Step 2: Run** `cargo test -p blindradio-core settings`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same. Expected: pass.
- [ ] **Step 5: Commit** `feat(core): settings`.

### Task 3: Library (favorites, recent, custom stations, collections)

**Files:**
- Create: `crates/blindradio-core/src/library.rs`, `crates/blindradio-core/tests/library.rs`

**Interfaces:**
- Produces: `pub struct Library { pub favorites: Vec<Station>, pub recent: Vec<Station>, pub collections: Vec<Collection> }` (`Serialize, Deserialize, Default`), `pub struct Collection { pub id: String, pub name: String, pub stations: Vec<Station> }`, `pub const CUSTOM: &str = "custom"` (the Custom Stations collection id, name `Custom Stations`, always present and first; `load` re-adds it if missing). Methods (all match by station id):
  - `load(dir) -> Library`, `save(&self, dir) -> Result<()>` over `library.json`.
  - Favorites: `is_favorite(&self, id) -> bool`, `toggle_favorite(&mut self, s: &Station) -> bool` (true = now a favorite, added at the end), `remove_favorite(&mut self, id)`, `move_favorite(&mut self, index: usize, up: bool) -> usize` (new index; no-op at the ends).
  - Recent: `add_recent(&mut self, s: &Station)` (to the front, removing an older copy, capped at 50).
  - Custom: `add_custom(&mut self, name: &str, url: &str) -> Station` (id `custom:<uuid>`, source `Custom`, appended to Custom Stations), `delete_custom(&mut self, id)` (removes it from Custom Stations only; copies in favorites or other collections stay, Ruling R1 of plan 3).
  - Collections: `create_collection(&mut self, name: &str) -> String` (id `col:<uuid>`; a name that already exists, compared case-insensitively, returns that collection's id), `rename_collection(&mut self, id, name) -> Result<(), LibraryError>`, `delete_collection(&mut self, id) -> Result<(), LibraryError>` (`LibraryError::CustomFixed` for `CUSTOM` in both), `toggle_in_collection(&mut self, id, s: &Station) -> bool`, `remove_from_collection(&mut self, id, station_id)` (on `CUSTOM` this is `delete_custom`), `move_in_collection(&mut self, id, index, up) -> usize`, `move_collection(&mut self, id, up)` (Custom Stations stays first), `add_all_to_collection(&mut self, from, to) -> usize` (count added; ones already there left as they are), `collections_containing(&self, station_id) -> Vec<String>` (ids, for the ticked Add to Collection menu).

- [ ] **Step 1: Write failing tests** in `tests/library.rs`: `favorites_toggle_order_and_move`; `recent_front_dedupe_cap_50`; `custom_stations_added_and_deleted`; `custom_collection_rules`; `collection_create_reuses_name_case_insensitive`; `toggle_and_remove_in_collection`; `delete_collection_keeps_stations_elsewhere`; `add_all_skips_existing_and_counts`; `collections_containing_station`; `library_round_trip_and_custom_readded` (a file without the custom collection loads with it first).
- [ ] **Step 2: Run** `cargo test -p blindradio-core --test library`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same. Expected: pass.
- [ ] **Step 5: Commit** `feat(core): favorites, recent, custom stations and collections`.

### Task 4: Playlist import and export

**Files:**
- Create: `crates/blindradio-core/src/import.rs`, `crates/blindradio-core/tests/import.rs`, `crates/blindradio-core/tests/fixtures/import/*`
- Modify: `crates/blindradio-core/src/playlist.rs` (counting of skipped entries)

**Interfaces:**
- Consumes: plan 1 `parse_m3u`, `parse_pls`, `sniff`; Task 3 `Library`.
- Produces:
  - `pub enum ImportTarget { Custom, CollectionNamedAfterFile }`, `pub struct ImportReport { pub added: usize, pub had: usize, pub skipped: usize }` with `pub fn message(&self) -> String` giving exactly `Added 12. Had 3. Skipped 1.` (parts with 0 are kept so the user hears all three, e.g. `Added 0. Had 2. Skipped 0.`).
  - `pub fn import_playlist(lib: &mut Library, path: &Path, target: ImportTarget) -> Result<ImportReport>`: kind from the extension (`.pls`, `.m3u`, `.m3u8`), else sniffed; text decoded as UTF-8 (BOM stripped), else Latin-1. Entries that are not `http(s)://` count as skipped. A URL already in the target (Custom Stations, or the collection) counts as had. New stations: Custom target uses `add_custom` (name from the file, else the URL); Collection target creates or reuses the collection named after the file stem (`My Stations.m3u` makes `My Stations`) and adds `custom:` stations to it without putting them in Custom Stations.
  - `pub fn export_m3u(stations: &[Station], path: &Path) -> Result<()>`: `#EXTM3U`, then `#EXTINF:-1,<name>` and the URL per station, UTF-8, `\n` line ends.
  - `pub fn parse_m3u_counted(body: &str) -> (Vec<PlaylistEntry>, usize)` and `pub fn parse_pls_counted(body: &str) -> (Vec<PlaylistEntry>, usize)` in `playlist.rs` (entries plus the count of skipped non-URL entries); the existing functions keep working.

- [ ] **Step 1: Write failing tests** with fixture files (one `.m3u` with a local file entry, one `.m3u8` UTF-8 with BOM and CRLF, one Latin-1 `.pls`, one with a duplicated URL): `import_custom_reports_counts`; `import_as_collection_named_after_file`; `import_into_existing_collection_counts_had`; `import_counts_duplicates_and_encodings`; `nameless_entry_named_after_url`; `export_then_import_round_trip`; `report_message_exact` (`Added 12. Had 3. Skipped 1.`).
- [ ] **Step 2: Run** `cargo test -p blindradio-core --test import`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same. Expected: pass.
- [ ] **Step 5: Commit** `feat(core): playlist import and export`.

### Task 5: Song history view

**Files:**
- Create: `crates/blindradio-core/src/history.rs`

**Interfaces:**
- Consumes: plan 2 `HistoryItem { title, played_at: Option<SystemTime> }`.
- Produces: `pub struct SessionHistory` with `pub fn station_changed(&mut self, id: &str)` (clears when the id differs), `pub fn title(&mut self, title: Option<&str>, now: SystemTime)` (ignores `None` and repeats of the newest), `pub fn items(&self) -> Vec<HistoryItem>` (newest first); `pub enum HistoryKind { Published, Session, Nothing }`; `pub fn view(published: Option<Vec<HistoryItem>>, session: &SessionHistory) -> (HistoryKind, Vec<HistoryItem>)` (published wins when non-empty); `pub fn heading(kind: HistoryKind) -> &'static str` giving exactly `Station history`, `Heard since you tuned in`, `No song information`; `pub fn ago(t: SystemTime, now: SystemTime) -> String` giving `Just now` (under 1 min), `5 min`, `1 h`, `1 h 20 min`, `2 d` (24 h and over).

- [ ] **Step 1: Write failing tests:** `session_clears_on_station_change`; `session_skips_repeats_and_none`; `published_wins_when_present`; `headings_exact`; `ago_formats`.
- [ ] **Step 2: Run** `cargo test -p blindradio-core history`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same. Expected: pass.
- [ ] **Step 5: Commit** `feat(core): song history view`.

### Task 6: Sleep timer

**Files:**
- Create: `crates/blindradio-core/src/sleep.rs`

**Interfaces:**
- Produces: `pub const SLEEP_MINUTES: [u32; 6] = [15, 30, 45, 60, 90, 120]` (Ctrl+1 to Ctrl+6 in this order); `pub struct SleepTimer` with `pub fn set(&mut self, minutes: u32, now: Instant) -> String` (replaces any running timer; returns the announcement: `Sleep 15 min`, `Sleep 30 min`, `Sleep 45 min`, `Sleep 1 h`, `Sleep 90 min`, `Sleep 2 h`), `pub fn cancel(&mut self) -> String` (`Sleep off`), `pub fn remaining(&self, now: Instant) -> Option<Duration>`, `pub fn due(&mut self, now: Instant) -> bool` (true once when the time is up, then the timer is off). The UI stops playback and says `Sleep timer: stopping` only if something was playing (YARSP).

- [ ] **Step 1: Write failing tests:** `labels_exact`; `set_replaces_and_cancel`; `due_fires_once`.
- [ ] **Step 2: Run** `cargo test -p blindradio-core sleep`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same. Expected: pass.
- [ ] **Step 5: Commit** `feat(core): sleep timer`.

### Task 7: Three-band equalizer in the mixer

**Files:**
- Create: `crates/blindradio-core/src/eq.rs`
- Modify: `crates/blindradio-core/src/output.rs` (`MixerCmd::Eq`, EQ stage in `render`), `crates/blindradio-core/src/settings.rs` (preset values)

**Interfaces:**
- Consumes: Task 2 `EqSettings`, `EqPreset`.
- Produces:
  - `impl EqPreset { pub fn bands(self) -> Option<(i8, i8, i8)> }` (`None` for Custom): Flat (0, 0, 0), Bass Boost (6, 0, 0), Treble Boost (0, 0, 6), Loudness (5, -1, 4), Voice (-3, 4, 1), Warm (3, 1, -2). `pub fn label(self) -> &'static str`: `Flat`, `Bass Boost`, `Treble Boost`, `Loudness`, `Voice`, `Warm`, `Custom`.
  - `pub struct Eq` (per-channel RBJ biquads: low shelf 120 Hz, peaking 1 kHz Q 0.9, high shelf 6 kHz; gains clamped -12 to +12 dB) with `pub fn new(rate: u32) -> Eq`, `pub fn set(&mut self, s: &EqSettings)` (coefficients recomputed; filter state kept so a change does not click), `pub fn process(&mut self, stereo: &mut [f32])` (no-op when disabled or all bands 0).
  - `MixerCmd::Eq(EqSettings)`; `MixerCore` runs the EQ after mixing and before volume, then clamps the output to -1 to 1.
  - `pub fn describe(s: &EqSettings) -> String` for the announcement after choosing a preset: `Bass 6 dB, Mid 0 dB, Treble 0 dB`.

- [ ] **Step 1: Write failing tests** in `eq.rs`: `flat_is_passthrough` (bit-exact); `bass_boost_raises_lows_only` (+6 dB bass: a 50 Hz sine gains 6 dB ± 0.7, a 5 kHz sine stays within ± 0.7 dB); `treble_cut_lowers_highs`; `disabled_is_passthrough`; `eq_never_clips` (all bands +12 on a full-scale 100 Hz sine through `MixerCore`: every output sample within ±1.0); `preset_values_and_labels`; `describe_exact`.
- [ ] **Step 2: Run** `cargo test -p blindradio-core eq output`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same. Expected: pass.
- [ ] **Step 5: Commit** `feat(core): three-band equalizer`.

### Task 8: Output devices, switching and fallback

**Files:**
- Modify: `crates/blindradio-core/src/output.rs`
- Create: `crates/blindradio-core/tests/devices.rs`

**Interfaces:**
- Produces:
  - `pub fn output_devices() -> Vec<String>` (names of every output device of the default host, default first).
  - `Output::start(device: Option<&str>) -> Result<Output>` (`start_default()` stays as `start(None)`); a name not found plays through the default.
  - `pub fn switch(&mut self, device: Option<&str>) -> Result<()>`: rebuilds the cpal stream on the new device keeping the same `MixerCore` (active ring, fades, volume, EQ), so playback continues. The new stream runs at the current `sample_rate` when the device supports it; otherwise at the device default and `rate_changed()` returns `Some(new_rate)` once, so the app restarts the playing station at the new rate.
  - `pub enum DeviceEvent { Lost, Back(String) }` and `pub fn poll_devices(&mut self, list: &dyn Fn() -> Vec<String>) -> Option<DeviceEvent>`, called by the UI about every 2 s: when the chosen device has errored or is missing from `list()`, switch to the default and return `Lost` (UI says `Device lost. Using default.`); when on the default because of a loss and the chosen name is back in `list()`, switch back and return `Back(name)` (UI says `Using <name>`). The chosen name is kept either way. The cpal error callback only sets a flag the poll reads.
  - `pub fn chosen(&self) -> Option<&str>`.
- The fallback decision is a pure function so it can be tested without a device: `pub fn fallback_step(chosen: Option<&str>, active: Option<&str>, errored: bool, available: &[String]) -> Option<FallbackAction>` with `FallbackAction { UseDefault, UseChosen }`.

- [ ] **Step 1: Write failing tests** in `tests/devices.rs` (pure): `device_fallback_and_return` (chosen "USB", active "USB", available without it: `UseDefault`; then available with it again: `UseChosen`; no action while chosen is None); `errored_device_falls_back`. Plus an `#[ignore]` test `lists_real_devices` (at least one name).
- [ ] **Step 2: Run** `cargo test -p blindradio-core --test devices`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p blindradio-core`. Expected: all pass. Run the ignored test once: `cargo test -p blindradio-core --test devices -- --ignored`.
- [ ] **Step 5: Commit** `feat(core): choose output device with fallback`.
