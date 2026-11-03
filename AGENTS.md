# BlindRadio

Screen-reader-first internet radio. Rust. Windows, macOS arm64, Linux.
`crates/blindradio-core` = engine, no UI. `crates/blindradio` = wxDragon app. `crates/xtask` = packaging, release.

## Build and test
- Build: `cargo build`. Gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets`.
- Desktop UI, real window, takes keyboard: `cargo test -p blindradio --test ui -- --ignored --test-threads=1`.
- First UI run after rebuild loses keys, tests fail at random. Rerun before believing. Rare single flake remains, passes alone.
- Network tests: `tests/live*.rs`, `#[ignore]`.
- Ear check: `cargo run -p blindradio-core --example probe --release [warm_secs]`. Latency: `--example tune`, numbers in `doc/latency.md`.

## Windows prerequisites
- MSVC Build Tools 2022, CMake, Ninja, LLVM (`winget install LLVM.LLVM`).
- bindgen needs libclang: `LIBCLANG_PATH=C:\Program Files\LLVM\bin`.
- `cl.exe` missing: run `"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"` first. Ninja comes from there too.
- Link fail `wxd_PrintData_*`: wxdragon feature variants share one CMake cache, OFF sticks. Delete `target/debug/wxdragon_sys_cmake_build/build/CMakeCache.txt`, `target/debug/build/wxdragon-sys-*`, `target/debug/.fingerprint/wxdragon-sys-*`. Rebuild inside vcvars.

## Rules
- Deps never pinned: `version = "*"`, newest. `cargo update` before release.
- No FFmpeg, no shipped codec DLLs. Decoders pure Rust, compiled in. FFmpeg only makes test fixtures.
- Strings terse. Error Display = inner text only. UI adds prefix.
- Hard tabs, edition 2024. Never `cargo fmt --all`: repo not fmt-clean, churns ~30 files. Format touched lines only.
- Audio by ear: no pause between stations, hiss fills gap (`MixerCmd::Tune`). Ring never drops audio. Start cushion stays 1 s (`ring::CUSHION_MS`). Never trade gaps for latency.
- Every user-facing change: `CHANGELOG.md` under next version, never under published one. Update `doc/readme.md`.
- Known shortcut = `ponytail:` comment. List: `git grep 'ponytail:'`.

## UI traps
- Screen reader names control from StaticText created just before it. Build label first. Creation order = name.
- wxDragon passes events on. Consume with `event.skip(false)`, or key acts twice. Unhandled: `skip(true)`.
- `app.selected` survives move onto main tree folder. Station command uses `App::station_here` (Ctrl+C, Ctrl+X: `focused_station`). Dialogs empty `selected` on folders themselves.
- Same station id can sit in several folders. Never find row by id first. Use row's parent (`DialTree::selected_home`).
- Settings tabs: new control takes panel as parent. Alt letter unique per tab. Notebook fit ignores tab strip width: long tab title hides tabs behind arrows. UI test catches it (`Spinner`).
- Hidden page control cannot take focus. Dialog `first` focus = notebook.
- New `Action`: `action.rs`, `keys.rs` (enum, `ALL` count, `tab`, `id`, `label`, `action`, `menu_key`), `menus.rs`, `app.rs` no-op arm if modal, `main_window::dispatch`.
- UI tests: uiautomation `{space}` never delivers Space, use `press` table. Browser opening steals foreground, next key lost.
- `TaskBarIcon::on_left_up` Windows only. Linux `on_left_down`. macOS no click event.
- SomaFM empty-replies any UA containing `blindradio`. Use `Blind-Radio/x`.

## Docs
- Spec: `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md`.
- Plans: `docs/superpowers/plans/`. All done, history only.
- User guide: `doc/readme.md`. F1 in app, converted at build. Changelog embedded same way.
- NVDA manual test: `doc/nvda-test-plan.md`.
- Directories: `docs/radio-directory-sources.md`. Encoded ring: `docs/encoded-ring-buffer.md`.

## Release
- Owner default: cloud. `gh workflow run release.yml -f version=X`. `## X` in `CHANGELOG.md` on main first. Workflow gates, bumps, tags, builds 3 OS, verifies, publishes. 20-30 min. Dispatch again resumes unpublished draft. Windows assets Authenticode-unsigned there.
- Local: `cargo xtask release <version>`. Order: clean check, tests, clippy, bump commit, Windows build, Linux over SSH, notes, tag + push, draft, minisign + upload, macOS dispatch, verify, publish. `--windows-only` skips macOS + Linux. `--dry-run` stops after clippy.
- Local needs `sh` on PATH, `MINISIGN_KEY`, Inno Setup, owner cert for `signtool`.
- Missing `## <ver>`: local stops after bump commit and builds, nothing pushed. `git reset --hard HEAD~1`, add section, rerun.
- Never cloud and local together: both bump from latest tag.
- Publish needs 10 files: zip, setup.exe, dmg, AppImage, tar.gz, each with `.minisig`.
- `cargo xtask dist-windows`: `target/dist/BlindRadio.zip`, `BlindRadio_setup.exe`. `BLINDRADIO_UNSIGNED` skips signtool.
- macOS: `macos-release.yml`, arm64. Linux local: `tools/build_linux_remote.sh`, container on `root@serrebiradio.com`. Launch check passes only as exit 124.
- Update check: `cargo xtask update-check [version]`: asks GitHub, downloads, verifies.

## Signing key
- `cargo xtask minisign-key <dir>` writes `minisign.key`, `minisign.pub`. Sign any file: `cargo xtask sign <file>…`.
- Secret = repo secret `MINISIGN_KEY`, never git. GitHub cannot read it back. Keep key file backed up.
- Key lost = rotate = installed copies never verify again. Public half: `crates/blindradio/assets/minisign.pub`. Change takes new release.

## Audit 2026-11-02
Open. Worst first. Fix = delete line.
- `zip` 8.6.0: RUSTSEC-2026-0367, 0368, 0369 (critical), 0429. Only via `shipfitter` 0.2.4, newest. Build + xtask only, not in shipped exe. Needs shipfitter bump to zip 9. `paste` unmaintained, transitive.
- No CI on push or PR. Tests run only at release.
- `wait_for_macos` reads newest run, no tag match. Old success can return early. `verify_assets` still blocks publish on missing dmg.
- Local release pushes tag before draft + signing. Fail after = tag on remote, no resume. Cloud resumes.
- `release` clean check = `git diff --quiet`. Misses staged + untracked.
- README says Windows builds signed. Cloud releases unsigned. README release text local only. README format list lacks AC-3, E-AC-3, MP2, TS.
- `settings.json` holds AudD, ListenBrainz, last.fm secrets in plain text. Fields unmasked.
- `Action::Play` still takes `selected_or_playing`: Play button on folder plays last station.
- Big files: `app.rs` ~2500 lines, `dialogs/browse.rs` ~2400. Unsafe clusters in `status_strip.rs` (69), Win32.
- Stale: xtask usage omits `update-check`. `menus.rs` comment on plans 5, 6. `app.rs` "later tasks" comment. `doc/nvda-test-plan.md` "four File check items", "(plan 5)".
