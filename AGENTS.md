# BlindRadio

Screen-reader-first internet radio. Rust. Windows, macOS arm64, Linux.
`crates/blindradio-core` = engine, no UI. `crates/blindradio` = wxDragon app. `crates/xtask` = packaging.

## Build and test
- Build: `cargo build`. Gate: `cargo test --workspace`, `cargo clippy --workspace --all-targets`. CI runs gate on push + PR (`ci.yml`), Windows only.
- Desktop UI, real window, takes keyboard: `cargo test -p blindradio --test ui -- --ignored --test-threads=1`.
- First UI run after rebuild loses keys, tests fail at random. Rerun before believing. Rare single flake remains, passes alone.
- Installed copy running = test exe exits, "the window appears" times out. Instance name comes from `USERNAME`: set it to anything else for the test run.
- Network tests: `tests/live*.rs`, `#[ignore]`.
- Ear check: `cargo run -p blindradio-core --example probe --release [warm_secs]`. Latency: `--example tune`, numbers in `doc/latency.md`.

## Windows prerequisites
- MSVC Build Tools 2022, CMake, Ninja, LLVM (`winget install LLVM.LLVM`).
- bindgen needs libclang: `LIBCLANG_PATH=C:\Program Files\LLVM\bin`.
- `cl.exe` missing: run `"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"` first. Ninja comes from there too.
- Link fail `wxd_PrintData_*`: wxdragon feature variants share one CMake cache, OFF sticks. Delete `target/debug/wxdragon_sys_cmake_build/build/CMakeCache.txt`, `target/debug/build/wxdragon-sys-*`, `target/debug/.fingerprint/wxdragon-sys-*`. Rebuild inside vcvars.

## Rules
- Deps never pinned: `version = "*"`, newest. `cargo update` before release. `cargo audit` clean except accepted below.
- No FFmpeg, no shipped codec DLLs. Decoders pure Rust, compiled in. FFmpeg only makes test fixtures.
- Strings terse. Error Display = inner text only. UI adds prefix.
- Hard tabs, edition 2024. Never `cargo fmt --all`: repo not fmt-clean, churns ~30 files. Format touched lines only.
- Audio by ear: no pause between stations, hiss fills gap (`MixerCmd::Tune`). Ring never drops audio. Start cushion stays 1 s (`ring::CUSHION_MS`). Never trade gaps for latency.
- Every user-facing change: `CHANGELOG.md` under next version, never under published one. Update `doc/readme.md`.
- Known shortcut = `ponytail:` comment. List: `git grep 'ponytail:'`.

## Code map
- `app.rs` = `App`, `run`, `poll`. Children in `app/`: `status`, `dial`, `stations`, `recording`, `global_keys`. Child sees App privates. Child method called elsewhere = `pub(super)`.
- `dialogs/browse.rs` = model + tests. `dialogs/browse/view.rs` = window.
- `speech.rs` = message path: NVDA (`nvda.rs`), then JAWS (`jaws.rs`), then UIA live region. JAWS path never run under JAWS: won't launch it beside owner's NVDA.
- `Settings.disabled_sources` = browse ids turned off (`tunein`, `net/bbc`), Sources tab in `dialogs/features.rs`. `Sources::set_disabled` filters Browse + Search. New directory: add to `Sources::directories`, gate its `search` with `self.on(id)`.
- `status_strip.rs` = own window + hand-built UIA provider, so NVDA+End finds status bar. Read its header before touching.
- `sources/live365.rs` reads web page data, no open API. Site rewrite empties it. How: `docs/radio-directory-sources.md`.
- `sources/orb.rs` = Online Radio Box: reads web pages, no API. Terms do not offer this use, owner's call. Browse, search, week of songs. Match by stream, or name + country: name alone shows wrong station's songs. How: `docs/radio-sources-research-2026-11-04.md`.
- `live.rs` = YouTube, Twitch links: own nightly yt-dlp + Deno in cache dir, process not crate, no FFmpeg. Station saves link, never stream: expires, holds IP. Hook = `net::open_at`. Never warmed, never in `ResolveCache`. `yt-dlp` crate = GPL + FFmpeg.
- `sources/played.rs` = open song lists, picked from stream URL: Triton, Live365, laut.fm, AzuraCast, BBC, KEXP, Radio Paradise. `Sources::history`: own list first, ORB adds older rows.
- Live = what ring plays next, not newest audio. Servers burst on connect (WNYC ~15 s). Burst sits in `PcmRing`, stays ahead of listener. `ShiftStatus::live_frame` takes it off: seek, pause, behind-live use that, never `live_edge`.
- Recording taps: playing station PCM = mixer as-heard tap (`MixerCmd::RecordSlot`), never decoder: decoder runs ahead by ring. Original = `Tuner::record_buffered` paced to playhead, live too. Live playhead steps back a little between mixer passes (edge per decoder push, backlog per pass): anchor only on jump count `ShiftStatus::seeks`, never on time going back, else chunks repeat (0.2.83 crackle). New playhead jump = `ShiftStatus::jump`. Background stream: ring closed (nobody drains it, else decoder stalls), `play_slot` None so playing station never writes into it.
- `core/segments.rs` = title bookmarks, jump + write. Marks in ms on tuner timeline: decoded = frame time, ring = session time. Mode switch clears them. Title at playhead = `Tuner::heard_title`, never wall clock.

## UI traps
- Screen reader names control from StaticText created just before it. Build label first. Creation order = name.
- NVDA reads focus ~125 ms after menu closes, cancels speech. New path that returns focus: call `speech::focus_returning()`, message waits 500 ms (`FOCUS_SETTLES`), flushed in poll.
- wxDragon passes events on. Consume with `event.skip(false)`, or key acts twice. Unhandled: `skip(true)`.
- `app.selected` survives move onto main tree folder. Station command uses `App::station_here` (Ctrl+C, Ctrl+X: `focused_station`). Set a station first, then dispatch `Play` = trap on folder: use `App::play` or `PlayFavorite`. Dialogs empty `selected` on folders themselves.
- Same station id can sit in several folders. Never find row by id first. Use row's parent (`DialTree::selected_home`).
- Per-key path stays cheap: no `refresh_dial`, no disk, no window text. Each costs NVDA more than the app. Tree rebuild was ~600 ms on 210 rows. `now_playing` defers to poll, volume uses `save_settings_soon`.
- Settings tabs: new control takes panel as parent. Alt letter unique per tab. Notebook fit ignores tab strip width: long tab title hides tabs behind arrows. UI test catches it (`Spinner`).
- Hidden page control cannot take focus. Dialog `first` focus = notebook.
- New `Action`: `action.rs`, `keys.rs` (enum, `ALL` count, `tab`, `id`, `label`, `action`, `menu_key`), `menus.rs`, `app.rs` no-op arm if modal, `main_window::dispatch`.
- UI tests: uiautomation `{space}` never delivers Space, use `press` table. Browser opening steals foreground, next key lost.
- `TaskBarIcon::on_left_up` Windows only. Linux `on_left_down`. macOS no click event.
- SomaFM empty-replies any UA containing `blindradio`. Use `Blind-Radio/x`.

## Secrets
- New token in settings: `#[serde(serialize_with = "crate::secret::seal", deserialize_with = "crate::secret::open")]`.
- Windows: DPAPI, this user only. Other PC or account reads blank, next save writes blank. macOS, Linux: plain, file mode 600.
- Token fields stay unmasked on purpose: screen reader reads masked field as stars, user cannot check paste.

## Docs
- Spec: `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md`.
- Plans: `docs/superpowers/plans/`. All done, history only. Older ones mention local release, gone now.
- User guide: `doc/readme.md`. F1 in app, converted at build. Changelog embedded same way.
- NVDA manual test: `doc/nvda-test-plan.md`.
- Directories: `docs/radio-directory-sources.md`. Encoded ring: `docs/encoded-ring-buffer.md`.

## Release
- Cloud only, Forgejo (GitHub suspended): `POST git.serrebiradio.com/api/v1/repos/serrebi/blindradio/actions/workflows/release.yml/dispatches` body `{"ref":"main","inputs":{"version":"X"}}`. `## X` in `CHANGELOG.md` on main first. Windows, Linux, macOS legs run at once after `bump`, alongside gate. Runners: Windows = this PC `desknasbox-windows-x64`, macOS = owner's Mac `mac-mini-arm64`, Linux = WSL on this PC `wsl-linux-x64` (label `wsl-linux`, owner rule: Linux builds in WSL). Starts at boot, no logon: task "Forgejo Runner (WSL)", S4U. Gate on Windows runner (2 jobs at once, so gate + Windows build overlap); bump, publish on server. Gate target = `C:/forgejo-runner/cargo-target/blindradio/gate`, push CI = `.../test`. Shared folder = push run's tests lock exes gate relinks, LNK1104 both.
- Workflow: gate (`ci.yml`) and bump start together. Bump pushes `release/vX` branch, no tag. Windows, Linux, macOS build off it during gate. Publish needs gate + all three: tags, release, uploads, main takes bump, branch deleted. Failed run = nothing published, rerun starts over. Tag exists = refused.
- Bump edits own version in `Cargo.lock` too, then `cargo metadata --locked`. Manifest alone = cargo re-resolves `*` deps, drops to older copies in lock, ships what gate never tested.
- Windows assets Authenticode-unsigned. Minisign on everything.
- Publish needs 10 files: zip, setup.exe, dmg, AppImage, tar.gz, each with `.minisig`.
- Check after: `cargo xtask update-check <old version>`: asks Forgejo, downloads, verifies.
- Build one OS: `cargo xtask dist-windows` (`BLINDRADIO_UNSIGNED=1` skips signtool), `dist-macos`, `dist-linux` in `tools/linux` container. AppImage launch check passes only as exit 124.

## Signing key
- `cargo xtask minisign-key <dir>` writes `minisign.key`, `minisign.pub`. Sign any file: `cargo xtask sign <file>…`.
- Secret = repo secret `MINISIGN_KEY`, never git. Forgejo cannot read it back. Keep key file backed up.
- Key lost = rotate = installed copies never verify again. Public half: `crates/blindradio/assets/minisign.pub`. Change takes new release.

## Accepted, not fixable here
- `shipfitter` patched to fork `serrebidev/shipfitter` branch `zip-9` (`Cargo.toml`): upstream pins zip 8, RUSTSEC-2026-0367, 0368, 0369, 0429. Upstream takes zip 9 = drop patch, delete fork.
- `paste` unmaintained, RUSTSEC-2024-0436: proc macro inside `wxdragon`, build time. Waits on wxdragon.
- `unsafe` in `status_strip.rs`: COM vtables for UIA provider. No safe form without new dep.
