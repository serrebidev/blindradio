# BlindRadio Plan 6: Updates, Packaging and Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship BlindRadio on Windows (portable zip and installer), macOS (Apple Silicon `.dmg`) and Linux (AppImage and tarball), signed, with an in-app updater, from one release command, and publish version 0.2.0.

**Architecture:** shipfitter in `build.rs` (manifest, version resource, icon, commit info) and a `crates/xtask` binary for packaging (zip, Inno Setup, minisign). ship-shape is the updater (GitHub Releases of `serrebidev/blindradio`, minisign-verified). Windows builds locally and is signed with the owner's certificate; macOS builds on a GitHub runner; Linux builds over SSH on `root@serrebiradio.com` in a container. `cargo xtask release <version>` drives all three.

**Tech Stack:** shipfitter (`windows-resources`, `package`, `docs`), ship-shape, minisign (via shipfitter `sign::Minisign`), Inno Setup 6, signtool, GitHub Actions (macos-15, arm64), Docker on the Linux host.

**Spec:** `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md` (Data and delivery, Updates, Releases). Owner rules: macOS Apple Silicon only, one Mac build; Windows built locally on this machine, never by cloud release in parallel; `cloud-release.yml` exists for cloud agents only.

## Global Constraints

- Release assets, named for ship-shape: `BlindRadio.zip` (portable, contains an empty `data` folder so it runs portable), `BlindRadio_setup.exe` (Inno Setup, per-user install, Start menu shortcut `BlindRadio`, silent `/silent` install works), `BlindRadio.dmg` (ad-hoc signed `.app`), `BlindRadio.AppImage`, `BlindRadio.tar.gz`, each with `<asset>.minisig` beside it.
- Windows `blindradio.exe` and `BlindRadio_setup.exe` signed: `signtool sign /fd SHA256 /tr http://timestamp.digicert.com /td SHA256 /a <file>` (the owner's certificate in the user store; signtool at `C:\Program Files (x86)\Windows Kits\10\bin\10.0.26100.0\x64\signtool.exe`, overridable with `SIGNTOOL_PATH`). A missing certificate fails the release, not the dev build.
- The VC++ runtime is linked statically (`+crt-static`) or its three DLLs ship beside the exe; the portable zip must run on a clean Windows 11.
- minisign: a new key pair generated before the first signed release; private key into GitHub secret `MINISIGN_KEY` (and `MINISIGN_PASSWORD` if set) and the owner's Vaultwarden (`bw` CLI) item `BlindRadio minisign`; the public key compiled into the updater.
- Versions 0.x; the release is a normal release (not a pre-release), since ship-shape's stable channel ignores pre-releases.
- Never run a cloud and a local release at the same time.

## Review Focus

1. Updating a portable copy: the `data` folder (favorites, settings) survives the update untouched. Test: `portable_update_keeps_data` (Task 3, manual on a scratch copy, recorded in the task report).
2. Installer over an older install, and uninstall: settings in `%APPDATA%\BlindRadio` stay on upgrade and uninstall. Test: manual, recorded (Task 2).
3. A release whose macOS or Linux leg fails: no half-published release; the script stops before publishing and says which leg failed. Test: `release_aborts_on_failed_leg` (Task 6, dry run with a forced failure).
4. A tampered download: ship-shape refuses it (minisign). Test: `tampered_asset_rejected` (Task 3).
5. Running the portable zip from a read-only or synced folder (Seafile, OneDrive): it starts and says `Settings not saved` once if `data` is not writable. Test: `read_only_data_says_once` (Task 1).

---

### Task 1: Build script, resources and portable data check

**Files:**
- Modify: `crates/blindradio/build.rs`, `crates/blindradio/Cargo.toml`, `crates/blindradio/src/app.rs`
- Create: `.cargo/config.toml` (static CRT for `x86_64-pc-windows-msvc` if it builds and links with wxDragon; otherwise a ruling and the DLLs ship instead)

**Interfaces:**
- Produces: build.rs uses shipfitter to embed the manifest (as plan 1's, Common Controls v6, PerMonitorV2, UTF-8), the version resource (`BlindRadio`, product version from the workspace), `assets/blindradio.ico`, and `BLINDRADIO_COMMIT_HASH`. On startup, if the data folder is not writable, `say` `Settings not saved` once and keep running in memory.

- [ ] **Step 1: Write failing test** `read_only_data_says_once` (pure check over a writability probe result).
- [ ] **Step 2: Run** `cargo test -p blindradio`. Expected: FAIL.
- [ ] **Step 3: Implement**; verify the exe's Properties > Details show the version and the icon.
- [ ] **Step 4: Run** tests. Expected: pass.
- [ ] **Step 5: Commit** `build: version resource, icon and static CRT`.

### Task 2: xtask packaging for Windows

**Files:**
- Create: `crates/xtask/Cargo.toml`, `crates/xtask/src/main.rs`, `installer/blindradio.iss.in`, `.cargo/config.toml` alias `xtask = "run -p xtask --"`

**Interfaces:**
- Produces: `cargo xtask dist-windows` → `target/dist/BlindRadio.zip`, `target/dist/BlindRadio_setup.exe`, both `.minisig`, after `cargo build --release -p blindradio` and signing the exe (then the installer). The zip holds `BlindRadio/blindradio.exe`, `BlindRadio/data/` (empty), `BlindRadio/readme.html`, `LICENSE`. The Inno script installs per-user to `{localappdata}\Programs\BlindRadio`, Start menu `BlindRadio`, optional desktop icon unchecked, uninstall keeps `%APPDATA%\BlindRadio`.

- [ ] **Step 1: Implement** the xtask with shipfitter `package` (`cargo_build_release`, `Zip`, `inno_setup`) and `sign::Minisign`.
- [ ] **Step 2: Run** `cargo xtask dist-windows`. Expected: both assets and signatures in `target/dist`; `signtool verify /pa` passes on both.
- [ ] **Step 3: Manual checks** (record in the report): install, run, upgrade over itself, uninstall keeps settings; unzip the portable zip into a scratch folder, run it, favorites land in its `data`.
- [ ] **Step 4: Commit** `build: Windows zip and installer`.

### Task 3: Updater

**Files:**
- Create: `crates/blindradio/src/update.rs`
- Modify: `crates/blindradio/src/menus.rs` (Help > Check for Updates), `crates/blindradio/src/app.rs`, `crates/blindradio-core/src/settings.rs` (`last_update_check: Option<u64>`)

**Interfaces:**
- Produces: ship-shape `UpdaterConfig::new("serrebidev/blindradio", "BlindRadio", "BlindRadio", <minisign public key>, env!("CARGO_PKG_VERSION"))` with `with_commit(env!("BLINDRADIO_COMMIT_HASH"))` and `with_install_kind` (Portable when `store::is_portable()`, else Installer); an automatic check once a day, 10 s after startup (`CheckTrigger::Automatic`, timestamp in settings); Help > Check for Updates runs `CheckTrigger::Manual`; `ui::bring_to_front_after_update` after the first show.

- [ ] **Step 1: Generate the minisign key pair** (`minisign -G`, or shipfitter's helper): store the secret key in GitHub secrets `MINISIGN_KEY`/`MINISIGN_PASSWORD` (`gh secret set`) and in Vaultwarden (`bw create item` named `BlindRadio minisign`); commit only the public key.
- [ ] **Step 2: Write failing tests:** `daily_check_due` (pure: last check over 24 h ago or never → due); `install_kind_from_portable`.
- [ ] **Step 3: Run** `cargo test -p blindradio update`. Expected: FAIL.
- [ ] **Step 4: Implement**; test `tampered_asset_rejected` and `portable_update_keeps_data` manually against a scratch draft release on a fork or a local file server (record in the report).
- [ ] **Step 5: Run** tests. Expected: pass.
- [ ] **Step 6: Commit** `feat(desktop): updates`.

### Task 4: macOS build on GitHub Actions

**Files:**
- Create: `.github/workflows/macos-release.yml`, `crates/xtask/src/macos.rs`

**Interfaces:**
- Produces: `workflow_dispatch` with input `tag` on `macos-15` (arm64; fail if `uname -m` is not `arm64`): installs CMake/Ninja/LLVM via brew, `cargo xtask dist-macos` (shipfitter `macos` Info.plist + `.app` bundle, ad-hoc `codesign --force --deep -s -`, `hdiutil` `.dmg`), minisign with `MINISIGN_KEY`, uploads `BlindRadio.dmg` and `.minisig` to the release `tag` (`gh release upload --clobber`), `permissions: contents: write`.
- macOS code paths compile: speech (live-region), hotkeys (`global-hotkey`), tray, single instance, audio (CoreAudio via cpal).

- [ ] **Step 1: Implement**; push the branch; run `gh workflow run macos-release.yml -f tag=<a test draft tag>`; watch `gh run watch --exit-status`.
- [ ] **Step 2: Fix until green**; check the `.dmg` mounts and the app's binary is arm64 (`lipo -archs`).
- [ ] **Step 3: Commit** `build: macOS app on GitHub Actions`.

### Task 5: Linux build over SSH

**Files:**
- Create: `tools/build_linux_remote.sh`, `tools/linux/Dockerfile`, `crates/xtask/src/linux.rs`

**Interfaces:**
- Produces: `cargo xtask dist-linux` copies the source (git archive of HEAD) to `root@serrebiradio.com:/tmp/blindradio-build/`, builds in a `ubuntu:24.04` container (GTK 3 dev, libasound2-dev, clang, cmake, ninja, rustup stable), makes `BlindRadio.AppImage` (shipfitter AppImage, appimagetool) and `BlindRadio.tar.gz`, runs a launch check (`timeout 10 xvfb-run -a ./BlindRadio.AppImage`, exit 124 means it stayed up), copies both back to `target/dist`, and minisign-signs them locally. Linux code paths compile: speech (ATK), hotkeys (X11 `global-hotkey`, Wayland message), tray, ALSA audio.

- [ ] **Step 1: Implement.**
- [ ] **Step 2: Run** `cargo xtask dist-linux`. Expected: both assets, launch check passes.
- [ ] **Step 3: Commit** `build: Linux AppImage and tarball over SSH`.

### Task 6: Release command, docs and v0.2.0

**Files:**
- Modify: `crates/xtask/src/main.rs`, `AGENTS.md`, `README.md`, `Cargo.toml` (workspace version)
- Create: `CHANGELOG.md`, `.github/workflows/cloud-release.yml`

**Interfaces:**
- Produces: `cargo xtask release <version> [--dry-run]`: checks a clean tree on `main`, runs `cargo test --workspace` and clippy, bumps the workspace version, regenerates `doc/licenses.html`, commits `chore: release <version>`, builds Windows, Linux (SSH) and creates a draft release with them, tags and pushes, dispatches the macOS workflow and waits for it, verifies all ten files are on the draft (REST `gh api repos/.../releases/tags/<tag>`), then publishes. Any failed leg stops before publishing and names the leg (`release_aborts_on_failed_leg` with `--dry-run --fail-leg linux`). `CHANGELOG.md` section per version, terse, used as the release notes. `AGENTS.md` gains a Release section (the command, the three hosts, the never-parallel rule, secrets). `cloud-release.yml` builds all three on runners for cloud agents only (Windows signing skipped there, documented).

- [ ] **Step 1: Implement** and run `cargo xtask release 0.2.0 --dry-run`, then the forced-failure dry run.
- [ ] **Step 2: Run** `cargo xtask release 0.2.0`. Expected: release 0.2.0 published with ten assets; download each Windows asset back and run it once.
- [ ] **Step 3: Commit** (the release command commits itself; commit any doc fixes) `docs: release process`.
