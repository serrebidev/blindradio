# Idle Sleep Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans to implement this plan inline.

**Goal:** Prevent Windows idle sleep during active playback or recording.

**Architecture:** A Windows-only RAII power request, owned by App and updated by its existing poll. No new dependency or settings.

**Tech Stack:** Rust, existing windows crate, Windows power request APIs.

**Spec:** docs/superpowers/specs/2026-11-11-idle-sleep.md

## Global Constraints

Hard tabs; format touched code only. Keep screen-off and locking available. Respect explicit sleep and Windows battery policy. Do not alter audio buffering.

## Review Focus

Background recording survives playback stop. Paused activity releases its claim. Station changes do not accumulate requests. API failure closes partial requests and reports once per activation. Shutdown explicitly releases even if App is never dropped.

## Task 1: Power request and release

- [x] Add native Windows tests in `crates/blindradio/src/power.rs`: requests are absent at idle, system and execution requests exist during either activity, display and away requests do not, repeated updates reuse one handle, stopping releases it, and the next activation works. Handle drop uses the existing `Owned<HANDLE>` implementation; do not probe stale numeric handles in concurrent tests.
- [x] Run `cargo test -p blindradio power::tests` against a no-op implementation and observe failure.
- [x] Implement `Awake::update(playing: bool, recording: bool)` with owned Windows request cleanup. Wire module, Windows feature flags, App poll, shutdown, and localized reason/error strings. Exclude paused playback and paused recordings.
- [x] Add and observe failing tests for paused/failed playback and requests cleared by Windows on sleep; implement `Tuner::playback_active` and user-resume notification handling. Three Windows power tests and the tuner regression now pass.
- [x] Run focused tests, `cargo test --workspace`, `cargo clippy --workspace --all-targets`, `cargo audit`, and `git diff --check`. Final gate: 565 passed, 71 ignored; Clippy clean with `-D warnings`; only the accepted paste audit warning.
- [ ] Update CHANGELOG under 0.2.85 and `doc/readme.md`; review complete diff; commit and push main.
- [ ] Dispatch Forgejo release 0.2.85; monitor gate and all builds; verify ten release assets and `cargo xtask update-check 0.2.84`.

## Execution notes

Implemented inline on `fix/idle-sleep`, preserving the existing checkout and build cache. Independent review found the resume lifecycle gap; it was fixed with a native callback that touches only a process-lifetime atomic. Dependency update exposed wx-utils git main requiring an unreleased wxDragon API; switched to the latest published wx-utils through `version = "*"`, retaining both feature flags. Dependency audit reports only the accepted paste maintenance warning.
