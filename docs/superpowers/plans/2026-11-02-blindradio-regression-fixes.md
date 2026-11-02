# BlindRadio Regression Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Fix six reported regressions in deletion, Browse paste, bursty AAC playback, station editing, modal navigation, and Open URL display.

**Architecture:** Keep existing library, dialog, and stream structures. Correct state at each current ownership boundary: Browse supplies exact occurrence paths, dialogs defer hidden main-tree synchronization, station edits invalidate changed provider metadata, and HTTP stall detection grants recovery only after decoder backpressure.

**Tech Stack:** Rust 2024, wxDragon, reqwest, crossbeam-channel, existing Windows UIAutomation tests.

**Spec:** User's six regression reports dated 2026-11-02.

## Global Constraints

- Add no dependency.
- Preserve native keyboard and screen-reader behavior.
- Use failing regression tests before product changes.
- Do not change playback or paste behavior outside the reported cases.

## Review Focus

- Deleting the last main-list station selects and plays the preceding station.
- Browse Copy and Cut retain exact collection/folder occurrence when the station also exists in Custom Stations.
- Genuine silent connections still fail promptly when no downstream backpressure occurred.
- Saving only per-station options preserves provider metadata; changing name or URL clears stale metadata.
- Unsaved directory results remain revealable while throwaway Open URL stations do not become tree rows.

---

**Changes from this plan while executing:**

- Task 3: KCRW did not reproduce here (120 s core, 150 s full tuner, flat-out, paced and paused reads). No backpressure signal; a connection that has played gets a longer stall limit, which also covers it.
- Task 2: the imported-playlist layout passes in Browse by real keyboard on 0.2.43. Collections and Favorites never recorded the focused row, which yields the reported message; those were fixed. The message now names the list.
- Task 4: an editable Info field, not clearing on rename: a user keeping the name could never clear the suffix.
- Task 6: no temporary rows at all, not only for `url:` ids.

---

### Task 1: Deletion playback

**Files:**
- Modify: `crates/blindradio/src/app.rs`
- Modify: `crates/blindradio/src/dialogs/browse.rs`
- Test: `crates/blindradio/tests/ui.rs`

**Interfaces:**
- Consumes: existing `refresh_dial`, `BrowseModel::dial`, and `App::focus_station`.
- Produces: replacement focus always maps to a playable station after deletion.

- [x] Add failing desktop regressions for deleting the final main row and deleting a Browse row.
- [x] Run each test and confirm the expected selection/playback failure.
- [x] Clamp numeric fallback focus and explicitly tune Browse's programmatic replacement.
- [x] Run both tests green.

### Task 2: Exact Browse paste occurrence

**Files:**
- Modify: `crates/blindradio/src/dialogs/browse.rs`
- Modify: `crates/blindradio/tests/ui.rs`

**Interfaces:**
- Consumes: `BrowseModel::paste_home` and existing clipboard operations.
- Produces: Copy, Cut, and Paste use the currently selected Browse row's exact home.

- [x] Repair the existing desktop fixture so it reaches the reported nested-to-parent path and fails on current behavior.
- [x] Synchronize Browse selection context immediately before clipboard shortcut dispatch if the regression proves state is stale.
- [x] Verify Copy and Cut reach the parent root, preserve Custom Stations, and refresh in place.

### Task 3: Bursty AAC backpressure

**Files:**
- Modify: `crates/blindradio-core/src/stream.rs`
- Test: `crates/blindradio-core/src/stream.rs`
- Test: `crates/blindradio-core/tests/stream.rs`

**Interfaces:**
- Consumes: bounded decoder byte pipe and existing `STALL` timeout.
- Produces: send reports whether it waited for decoder capacity; the next HTTP read receives bounded recovery grace only in that case.

- [x] Add a failing deterministic test proving the ordinary five-second budget is not selected after backpressure.
- [x] Confirm the test fails for the expected timeout-budget branch.
- [x] Return backpressure state from the existing send loop and choose the recovery budget without changing no-pressure stalls.
- [x] Run stream tests and a timed live KCRW probe without reconnect events.

### Task 4: Editable station display metadata

**Files:**
- Modify: `crates/blindradio-core/src/library.rs`
- Test: `crates/blindradio-core/tests/library.rs`

**Interfaces:**
- Consumes: existing `Library::update_station(id, name, url)` caller.
- Produces: provider `info` survives options-only saves and clears when the user changes name or URL.

- [x] Add a failing core test using the WNYC composite display data.
- [x] Clear copied provider metadata only when editable identity fields changed.
- [x] Run library tests green.

### Task 5: Modal navigation and final reveal

**Files:**
- Modify: `crates/blindradio/src/app.rs`
- Modify: `crates/blindradio/src/dialogs/mod.rs`
- Test: `crates/blindradio/tests/ui.rs`

**Interfaces:**
- Consumes: `dialog_open`, `tuner.playing`, and `reveal_station`.
- Produces: modal arrows tune immediately but rebuild/reveal the hidden main tree once, after close.

- [x] Add a failing Favorites/Search-path UI regression for retained modal focus and deferred tree selection.
- [x] Skip `reveal_station` while a dialog is open; reveal the final playing station when it closes.
- [x] Run the modal UI regression green.

### Task 6: Throwaway Open URL rows

**Files:**
- Modify: `crates/blindradio/src/app.rs`
- Test: `crates/blindradio/src/app.rs`
- Test: `crates/blindradio/tests/ui.rs`

**Interfaces:**
- Consumes: `Library::holds_station`, `reveal_station`, and `dial_extra`.
- Produces: unsaved `url:` stations play without appearing in the tree; saved URLs and directory results remain revealable.

- [x] Add failing predicate and desktop regressions.
- [x] Make reveal return whether a valid real or synthetic row exists and retain prior selection otherwise.
- [x] Run regressions green.

### Task 7: Verification and release

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `doc/readme.md` only if user-visible behavior needs clarification.

**Interfaces:**
- Consumes: all fixed behavior.
- Produces: signed `v0.2.44` release for Windows, macOS, and Linux.

- [x] Run `cargo fmt --all -- --check`.
- [x] Run `cargo test --workspace`.
- [x] Run desktop UI tests serially.
- [x] Run `cargo clippy --workspace --all-targets -- -D warnings`.
- [x] Request independent code review and resolve important findings.
- [x] Commit fixes, run `cargo xtask release 0.2.44`, verify ten signed assets and updater discovery, then publish.
