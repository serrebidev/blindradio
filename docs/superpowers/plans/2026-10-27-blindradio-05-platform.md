# BlindRadio Plan 5: Platform Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** BlindRadio talks and lives in the background like YARSP: spoken messages through the screen reader, rebindable global hotkeys, a tray icon, a single running copy, the F1 user guide and an About box with licences.

**Architecture:** Thin platform layers in `crates/blindradio`, each behind a small module with a pure, tested policy part: `speech` (live-region), `hotkeys` (wx-utils on Windows, the `global-hotkey` crate on macOS and X11 Linux), `tray` (wx `TaskBarIcon`), `instance` (wx single-instance checker plus a local IPC poke to raise the running window), `help` (user guide embedded at build time, opened in the browser).

**Tech Stack:** wxDragon, wx-utils (`shortcuts`, `global-hotkeys` features), live-region, global-hotkey (macOS and Linux only), shipfitter (`docs` feature, build dependency), cargo-about for the licence page.

**Spec:** `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md` (Speech per platform, Global hotkeys per platform, Feature list: Global hotkeys, Now playing, Tray icon, Single instance, F1). Behaviour reference: `docs/reference/yarsp-manual.html` (Global hotkeys, The tray icon, Now playing, Settings).

## Global Constraints

- Messages are spoken and also shown. Windows open quietly. No speech on startup unless the screen reader would say nothing useful (the builder errs on silence: none).
- Announce Connection Status off silences `Connecting`, `Buffering` and `Stopped` only; failures, `Connection lost. Reconnecting.` and `Reconnected` are always spoken.
- Global hotkey defaults exactly: Ctrl+Alt+Shift+P play or pause, Ctrl+Alt+Shift+S stop, Ctrl+Alt+Shift+PageUp / PageDown volume, Ctrl+Alt+Shift+Right / Left next / previous favorite, Ctrl+Alt+Shift+N announce what's playing, Ctrl+Alt+Shift+Y hide or show the window, Ctrl+Alt+Shift+E equalizer on or off. Rebindable in File > Global Hotkeys… with Restore Defaults; Delete clears one.
- Showing the window, from any route, gives it keyboard focus.
- Terse strings exactly as written here.

## Review Focus

1. A global hotkey already taken by another program: the others still work and the user hears `Hotkey taken: <chord>` once at startup. Test: `taken_hotkeys_reported_once` (Task 2).
2. Song title announcements while dial tuning: none until focus rests 1.5 s (plan 1 Settle), and none at all with Announce Song Titles off, while Ctrl+Alt+Shift+N still reads it. Test: `announce_now_playing_ignores_setting` (Task 2).
3. A second launch while the first is hidden in the tray: the first shows, gets focus, and the second exits without opening a window. Test: `second_instance_raises_first` (Task 4, `#[ignore]` desktop test).
4. Closing the tray menu while hidden: no empty window appears and focus returns where the user was. Manual check in `doc/nvda-test-plan.md` (Task 3).
5. Wayland with no GlobalShortcuts portal: one `Hotkeys unavailable` message, the app otherwise normal. Test: `wayland_without_portal_says_once` (Task 2, pure policy).

---

### Task 1: Speech

**Files:**
- Create: `crates/blindradio/src/speech.rs`
- Modify: `crates/blindradio/src/app.rs` (`say` speaks), `crates/blindradio/Cargo.toml`

**Interfaces:**
- Produces: `pub struct Speech` wrapping a `live_region` region on the main frame with `pub fn say(&self, text: &str, priority: Priority)`; `pub enum Kind { Status, Title, Failure, Info }` and `pub fn should_speak(kind: Kind, settings: &Settings, tuning: bool) -> bool` (Status only with Announce Connection Status on and not while dial tuning; Title only with Announce Song Titles on; Failure and Info always); `App::say_kind(kind, text)` used by every caller of `say` from plan 4 (Failure and reconnect messages use high priority, Title medium, the rest low).

- [ ] **Step 1: Write failing tests:** `should_speak_policy` (all combinations).
- [ ] **Step 2: Run** `cargo test -p blindradio speech`. Expected: FAIL.
- [ ] **Step 3: Implement** with `live-region = { git = "https://github.com/trypsynth/live-region" }`.
- [ ] **Step 4: Run** same; NVDA check: a failure and a song title are spoken once, a dialog opening says nothing extra.
- [ ] **Step 5: Commit** `feat(desktop): speech`.

### Task 2: Global hotkeys and their editor

**Files:**
- Create: `crates/blindradio/src/hotkeys.rs`, `crates/blindradio/src/dialogs/hotkeys.rs`

**Interfaces:**
- Consumes: settings `hotkeys`, `App::run`, favorites.
- Produces: `pub enum HotkeyAction { PlayPause, Stop, VolumeUp, VolumeDown, NextFavorite, PreviousFavorite, AnnounceNowPlaying, ToggleWindow, ToggleEq }` with stable ids (`play_pause`, …) and the defaults above; `pub fn effective(settings: &Settings) -> Vec<(HotkeyAction, Option<KeyChord>)>`; registration through wx-utils `global_hotkeys::GlobalHotkeys::register` on Windows and `global-hotkey` on macOS and X11, events forwarded to the UI thread; unregistered chords say `Hotkey taken: <chord>` (one message listing all). `pub fn next_favorite(favorites: &[Station], playing: Option<&str>, forward: bool) -> Option<usize>` (wraps; from nothing playing, forward starts at the first). Announce now playing says `<song> - <station>`, `<station>`, or `Stopped`. Toggle EQ says `Equalizer on` / `Equalizer off`. Toggle window hides to the tray or shows with focus. The editor (File > Global Hotkeys…) is wx-utils `shortcuts::prompt_for_shortcuts` with one Global tab over a `ShortcutModel` on `settings.hotkeys`; saving re-registers. On Wayland without the portal: `Hotkeys unavailable` once.

- [ ] **Step 1: Write failing tests:** `defaults_exact`; `user_binding_overrides_default_and_clear_removes`; `next_favorite_wraps`; `announce_now_playing_ignores_setting`; `taken_hotkeys_reported_once`; `wayland_without_portal_says_once`.
- [ ] **Step 2: Run** `cargo test -p blindradio hotkeys`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same; NVDA check every default hotkey with BlindRadio in the background.
- [ ] **Step 5: Commit** `feat(desktop): global hotkeys`.

### Task 3: Tray icon and hide/show

**Files:**
- Create: `crates/blindradio/src/tray.rs`, `crates/blindradio/assets/blindradio.ico`, `crates/blindradio/assets/blindradio.png`

**Interfaces:**
- Produces: a `TaskBarIcon` with tooltip `BlindRadio` (then the now-playing text), menu Play / Pause, Stop, Favorites ▸ (each favorite plays it), separator, Show BlindRadio, Exit. Left click and Show raise the window with keyboard focus (on Windows, `SetForegroundWindow` through the frame's handle if wx does not). Closing the main window's close box hides to the tray only when hidden by the hotkey before? No: the close box exits (YARSP); Ctrl+Alt+Shift+Y hides. Dismissing the tray menu while hidden returns focus to the previous foreground window and shows nothing. The icon is a simple generated radio glyph (16, 32, 48, 256 px in the `.ico`).
- Add the manual checks for tray behaviour and Review Focus 4 to `doc/nvda-test-plan.md`.

- [ ] **Step 1: Write failing tests:** `tray_menu_items` (pure menu model from favorites and play state).
- [ ] **Step 2: Run** `cargo test -p blindradio tray`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** same; NVDA check: Win+B to the tray, menu, Show gives focus, hide and show by hotkey.
- [ ] **Step 5: Commit** `feat(desktop): tray icon`.

### Task 4: Single instance

**Files:**
- Create: `crates/blindradio/src/instance.rs`

**Interfaces:**
- Produces: at startup, wx `SingleInstanceChecker` named `BlindRadio-<user>`; if another copy runs, send it a `raise` message over wx IPC (`ipc.rs` in wxDragon, as Paperback's `ipc.rs` does) and exit 0 before creating any window; the running copy shows its window with focus on `raise`.

- [ ] **Step 1: Write the `#[ignore]` desktop test** `second_instance_raises_first` in `crates/blindradio/tests/ui.rs` (start the app, hide it with the hotkey, start a second copy: the second exits within 3 s with code 0, the first is the foreground window).
- [ ] **Step 2: Run** it. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** it. Expected: pass.
- [ ] **Step 5: Commit** `feat(desktop): single instance`.

### Task 5: User guide, About and licences

**Files:**
- Create: `doc/readme.md`, `crates/blindradio/build/docs.rs` (or in `build.rs`), `crates/blindradio/src/help.rs`, `about.toml`, `about.hbs`
- Modify: `crates/blindradio/build.rs`

**Interfaces:**
- Produces: `doc/readme.md`, the user guide: every feature and key of plans 1 to 5, terse, in YARSP's section order (Getting started, Dial tuning, Searching, Browsing, Favorites and recently played, Importing, Exporting, Your own stations, Context menu, Sound card, Equalizer, Collections, Song history, Sleep timer, Settings, When a stream drops, Updates, Tray icon, Now playing, Keyboard shortcuts, Global hotkeys). `build.rs` converts it with shipfitter `docs::convert_deflated` into `OUT_DIR`; F1 and Help > User Guide inflate it to `<temp>/BlindRadio/readme.html` and open it in the default browser. Help > About BlindRadio: wx-utils `AboutBoxBuilder` (name, version, `Screen-reader-first internet radio.`, website `https://github.com/serrebidev/blindradio`) plus a Licenses button that opens `licenses.html`, generated by `cargo about generate about.hbs` into `doc/licenses.html` (committed, regenerated by the release task) and embedded the same way. It must include FDK AAC's licence notice.

- [ ] **Step 1: Write failing tests:** `guide_embedded_and_inflates` (the embedded bytes inflate to HTML containing `Keyboard shortcuts`); `licenses_include_fdk` (embedded licence HTML contains `Fraunhofer`).
- [ ] **Step 2: Run** `cargo test -p blindradio help`. Expected: FAIL.
- [ ] **Step 3: Write the guide and implement.**
- [ ] **Step 4: Run** same; F1 opens the guide in the browser.
- [ ] **Step 5: Commit** `docs: user guide; feat(desktop): F1 help and About`.
