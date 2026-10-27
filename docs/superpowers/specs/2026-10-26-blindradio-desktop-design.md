# BlindRadio: shared core and desktop app

Date: 2026-10-26. Status: draft for owner review.

This covers phase 1 of 3: the shared Rust core plus the desktop app for Windows, macOS and Linux. Phase 2 (Apple: iPhone, iPad, Vision Pro, Apple TV) and phase 3 (Android) get their own design documents later. Phase 1 must not block them.

Decisions are marked **owner** (the user said so) or **builder call** (Claude chose it; owner can overrule while reviewing).

## Intent

BlindRadio is a screen-reader-first internet radio player. The owner is blind and uses NVDA.

Two things drive it:

1. **Tuning feels like turning an FM dial.** (owner) In any station list, moving focus onto a station starts it playing. No Enter needed. Sound should arrive as close to instantly as possible.
2. **The interface is as good as YARSP's, and terse.** (owner) YARSP (https://www.l-works.net/yarsp.php, manual https://l-works.net/docs/yarspmanual.html) is the quality bar for keyboard and screen reader behaviour. No wordiness anywhere: labels, announcements, menus, messages, dialogs.

Success: the owner arrows through a list of stations and hears each one almost as focus lands on it, and never hates using the UI.

## Goals

- Every YARSP 0.8 feature (owner), listed below, plus dial tuning.
- Runs on Windows, macOS and Linux (owner).
- Lowest practical latency from focus change to audio (owner).
- Core written so phase 2 and 3 apps can call it through UniFFI (builder call, from the Paperback model).

## Non-goals (phase 1)

- Mobile, tablet, TV and headset apps. Phase 2 and 3.
- Translations. English only. Keep strings in one place so patois can be added later (builder call).
- Recording, podcasts, on-demand audio.
- Intel macOS build. Apple Silicon only (owner's standing rule for all repos).

## Anti-goals

- A UI the owner hates. In practice: extra speech, focus that jumps unasked, controls with missing or long names, dialogs that are not real dialogs, keys that act on the wrong item, wordy messages.
- Custom-drawn widgets. Native controls only, so NVDA, VoiceOver and Orca read them.
- Dial tuning that floods the network or plays the wrong station after fast arrowing.

## Stack (owner unless marked)

- Rust.
- UI: wxDragon (https://github.com/trypsynth/wxDragon, main branch, wxWidgets 3.3.4 which carries the accessibility fixes Paperback needed).
- wx-utils (https://github.com/trypsynth/wx-utils): dialog footers, `confirm`, prompts, shortcut editor (`shortcuts` feature), global hotkeys (`global-hotkeys` feature, Windows only).
- shipfitter (https://github.com/trypsynth/shipfitter): build script helpers, Windows manifest and version resource, zip, Inno Setup installer, AppImage, macOS bundle, signing, notarizing, minisign signatures. Note: shipfitter is packaging only. The updater itself is ours (see Updates).
- Reference app: Paperback (https://github.com/trypsynth/paperback), same author and stack. Copy its repo layout, xtask approach, updater approach and UIA-based UI tests where they fit (builder call).
- Dependencies unpinned, newest versions, prereleases allowed (owner's standing rule). Git dependencies track their default branch.

## Repo layout (builder call)

Cargo workspace, GitHub repo `serrebidev/BlindRadio`:

- `crates/blindradio-core`: directories, search, station model, favorites, recent, collections, custom stations, playlist import/export, settings, the tuning engine, audio decode and output, EQ, metadata, song history, sleep timer, reconnect logic, update check. No wx dependency. UniFFI-ready API (the UniFFI bindings themselves can wait for phase 2).
- `crates/blindradio`: the wxDragon desktop app.
- `crates/xtask`: build and release tasks.
- `doc/readme.md`: user guide. F1 opens it. Embedded in the binary via shipfitter `convert_deflated`.
- `ios/` and `android/` arrive in later phases.

## Dial tuning (owner: behaviour; builder call: mechanism)

Behaviour:

- Applies to every station list: Search results, Browse tree, Favorites, Recently Played, Collections, and the main window's favourites if it shows a list.
- Focus lands on a station: it plays. Enter still plays too (and is harmless on the station already playing).
- Focus lands on a non-station (a Browse folder, Load More, a collection node): nothing changes, current audio keeps playing.
- After fast arrowing past several stations, only the station focus stops on ends up playing. Never a stale one.
- No extra speech while tuning. The screen reader reads the focused item; the audio is the confirmation. Connection status speech ("Connecting", "Buffering") is suppressed during dial tuning regardless of the setting. Failures still speak, tersely ("Failed: <station>").
- Song titles while tuning (builder call): not announced while focus is moving. Once focus has rested on the playing station for about 1.5 seconds, the current title is announced once (if Announce Song Titles is on), then each later change as normal. The window title updates immediately regardless.
- Only user-driven focus moves inside a list tune: arrows, letter jumps, Home/End, Ctrl+Home/End, PageUp/PageDown, and landing on the first new station after Load More. Focus restored by Alt+Tab, returning from a dialog, or a delete or reorder never changes the station (builder call).
- Opening a list (Favorites, Recent, Collections, Browse restoring its remembered station) and a search landing on its first result: the focused station starts playing at once (owner).
- After Stop or pause: the next arrow move or landing on a new station starts playing again. Stop does not turn tuning off (owner).
- Close Lists When Playing applies to Enter only, never to dial tuning (builder call).

Mechanism (builder call; builder may improve it, targets matter more than method):

- **Warm neighbours.** While a station list has focus, keep the stations directly above and below the focused one connected and decoding into a small buffer, muted. Moving onto a warm station swaps audio immediately.
- **Instant swap.** Play a warm station from its live edge, not from the start of its buffer, so it sounds current.
- **Cold start.** A station that is not warm starts connecting at once (no debounce). Moving away cancels it. Small start buffer: begin output after the first decodable frames, then grow the buffer in the background to ride out jitter.
- **Bandwidth limit.** At most the playing station plus 2 warm neighbours. Warm connections close when the list closes or focus has not moved for a short idle period (builder picks, around 20 to 30 seconds).
- **Targets.** Warm swap: under 100 ms to audible sound. Cold start on a typical Icecast MP3 or AAC stream on broadband: under 1 second, ideally around 300 ms. Measure and log both in debug builds.

## Audio (builder call)

- Formats: MP3, AAC including HE-AAC v1 and v2 (ADTS and in MP4), Ogg Vorbis, Opus, FLAC. HLS streams too, since several networks (BBC among them) publish HLS only.
- Decoders sit behind one trait in the core. Desktop: symphonia for most formats; all AAC through fdk-aac and Opus through libopus (see Audio and network stack). Phases 2 and 3 may swap in platform decoders (AudioToolbox, MediaCodec) behind the same trait; the core's tuning engine, buffering and metadata logic stay shared. Output on desktop via cpal; mobile phases decide their own output path.
- Licence: MIT, like Paperback. Keep third-party licence notices (fdk-aac's included) in an About > Licenses page.
- Stream inputs: direct HTTP(S) streams, PLS and M3U playlist URLs (resolve to the first working entry), HLS m3u8.
- ICY metadata for song titles. HLS ID3 or EXT-X metadata where present.
- Output device choice with fallback: if the chosen device disappears, announce it once, play through the default, switch back when it returns, keep the choice saved.
- Volume, mute. Three-band EQ (bass, mid, treble), each -12 to +12 dB, 1 dB steps, presets Flat, Bass Boost, Treble Boost, Loudness, Voice, Warm, plus Custom.
- Short drops ridden out silently. Lost stream: "Connection lost. Reconnecting.", retry for about a minute, then "Reconnected" or a terse failure.

## Feature list (owner: all of YARSP 0.8)

Behaviour and keys follow the YARSP manual unless this document says otherwise. The builder should read the manual in full; it is the detailed spec. Summary:

- **Search** (Ctrl+F): name, plus genre, country, language (genre and country are editable combo boxes). Searches all directories at once. Lands on the first result. Last column says which directory.
- **Browse** (Ctrl+G): tree with Radio Browser (genre, country, big countries split by state or region with "All stations" first, Load More), TuneIn (continent to city), iHeartRadio (all stations, genres; US only), Networks (BBC, NPR, CBC, ABC Australia, Radio France, Deutschlandfunk, SomaFM, Stream Madness; hand-curated data shipped with the app), Collections, Custom Stations. Right opens, Left closes, letters jump, Home/End stay on the current level, Ctrl+Home/Ctrl+End go to the whole tree's ends. Remembers position for the session, falling back to the nearest category. Enter on Load More lands on the first new station.
- **Custom stations** (Ctrl+N): name and URL. Delete key deletes.
- **Favorites** (Ctrl+B, Ctrl+D toggles): Alt+Up/Down reorder, Delete removes, Export button. Add/Remove Favorites button next to Play in main window and every list window; label flips with state. Alt+1 to Alt+0 play the first ten.
- **Recently played** (Ctrl+H).
- **Collections** (Ctrl+L, Ctrl+Shift+L adds what's playing): as in YARSP 0.8, including Custom Stations as an undeletable collection, Add All to Collection, F2 rename, Delete with confirmation, Move Up/Down, Export.
- **Playlist import** (Ctrl+I, Ctrl+Shift+I as collection): M3U, M3U8, PLS. Skip non-URL entries. Report added, already had, skipped, in one short line.
- **Context menu** (Applications key, Shift+F10) about the focused item. On a non-station, non-collection item, say so tersely instead of opening a menu.
- **Copy stream URL** (Ctrl+U): selected station, else playing one. Verify the copy.
- **Song history** (Ctrl+Y): station-published history where available (with "how long ago" column when times exist), else songs heard this session. Enter or Copy copies.
- **Sleep timer**: Playback menu, Ctrl+1 to Ctrl+6 (15, 30, 45, 60, 90, 120 minutes), Ctrl+0 cancels. Survives station changes.
- **EQ** (Ctrl+E): Preset, then Bass/Mid/Treble sliders announcing dB, Enabled checkbox. Ctrl+Alt+Shift+E global toggle.
- **Playback keys everywhere in the app**: Ctrl+P play/pause, Ctrl+S stop, Ctrl+M mute, Ctrl+Up/Down volume. Alt+V to volume slider.
- **Global hotkeys** (rebindable, Restore Defaults): Ctrl+Alt+Shift+P play/pause, S stop, PageUp/PageDown volume, Right/Left next/previous favorite, N announce now playing, Y hide/show window, E EQ toggle. Use the wx-utils shortcut dialog with a Global tab.
- **Now playing**: window title "Song - Station" when a song is known, else station. New songs announced (setting). Ctrl+Alt+Shift+N repeats.
- **Tray icon**: Play/Pause, Stop, favorites, Show, Exit. Showing the window gives it keyboard focus.
- **Single instance**: second launch focuses the running one.
- **Settings in File menu**: Announce Song Titles (on), Announce Connection Status (on), Close Lists When Playing (off), Play Last Station at Startup (off), Audio Device submenu, Global Hotkeys.
- **Update check**: daily, tells after startup, Help > Check for Updates on demand.
- **F1**: user guide.

YARSP UI rules that are hard requirements here (owner's "don't hate the UI", made concrete):

- Real modal dialogs: screen reader says "dialog", Escape closes, Enter presses the default button.
- Windows open quietly: no app speech on top of the screen reader's own.
- One window open at a time; a window's shortcut returns to it rather than reopening.
- Menus open on their first item.
- A letter in a list jumps to the next station starting with it; it never presses a button. Buttons use Alt plus their underlined letter.
- Focus returns to the control last used when the window regains focus.
- App starts with focus on Play. Startup says "BlindRadio ready" only if the screen reader would otherwise say nothing useful (builder decides; err on silence).
- Messages are spoken and also shown on screen, briefly.
- Every control has a short accessible name. No meaning by position, colour or icon alone.
- Status text on screen does not churn (Braille users).

## Terse language (owner)

Write every string as short as it can be while still clear. Examples of the tone: "Connecting", "Failed: BBC Radio 1", "Added 12. Had 3. Skipped 1.", "Sleep 30 min", "Device lost. Using default." Not "The station could not be loaded because..." Errors stay plain words, never raw error text unless in a diagnostics view.

## Speech per platform (builder call)

- Windows: UIA notifications so NVDA, JAWS and Narrator all speak. Check trypsynth's live-region crate first (wx-utils already uses it).
- macOS: VoiceOver announcement notifications.
- Linux: AT-SPI announcement, or speech-dispatcher if AT-SPI has no path. Best effort.

## Global hotkeys per platform (builder call)

- Windows: wx-utils `global-hotkeys`.
- macOS: Carbon `RegisterEventHotKey` (needs no accessibility permission).
- Linux: X11 `XGrabKey`; on Wayland use the xdg-desktop-portal GlobalShortcuts portal where available, otherwise tell the user once, tersely, that hotkeys are unavailable.
- Ideally contribute macOS and Linux back to wx-utils.

## Audio and network stack (researched 2026-10-26, builder call)

Closest prior art: radiotrope (github.com/goten002/radiotrope, Rust, MIT): rodio + symphonia + fdk-aac + bundled libopus, MPEG-TS HLS, ICY, producer/consumer split of network and decode. rmpd (github.com/m0rf30/rmpd, MIT/Apache) has pure-Rust HLS: master playlist reduced to one audio variant, live reload at target duration, ADTS / MP3 / fMP4 / MPEG-TS segments, AES-128. Read both before writing the pipeline.

Rejected: libmpv (libmpv2 crate), GStreamer (what Shortwave uses), libVLC. All play everything, but they are large LGPL C libraries, hard to ship to iOS, tvOS and visionOS, and hide the buffer control dial tuning needs.

Chosen pipeline, per station:

- Network: one shared `reqwest` client (rustls) on tokio, so DNS, TLS session resumption and happy eyeballs carry across stations. Headers `Icy-MetaData: 1`, `Accept-Encoding: identity`.
- ICY: `icy-metadata` crate (aschey) to strip metadata blocks and read `StreamTitle`.
- Decode: symphonia 0.6 (MP3, FLAC, Vorbis, Ogg, MP4 demux). All AAC through `symphonia-adapter-fdk-aac` (symphonia's own HE-AAC is unimplemented). Opus through `symphonia-adapter-libopus` (symphonia's own Opus is unimplemented).
- HLS: `m3u8-rs` for playlists, an MPEG-TS demuxer crate or a small PES extractor for ADTS audio, symphonia for fMP4. Follow rmpd's approach.
- Resample: `rubato` to the device rate, low-latency settings.
- Output: `cpal` directly, with a small own mixer (not rodio), so a station swap is sample-accurate, EQ biquads run in the callback, and output buffer stays around 10 to 20 ms.

Latency tactics:

- Icecast and Shoutcast servers "burst on connect" (default 64 KB, about 4 seconds of 128 kbps audio) the moment a listener connects. So start output on the first decoded frame instead of waiting to fill a buffer; the burst refills the cushion behind it.
- Cache each station's resolved final stream URL (after PLS/M3U and redirects) on disk, so later starts skip the extra round trips.
- Pre-resolve DNS for stations in the visible list.
- Warm neighbours keep decoding into a short PCM ring (2 to 3 seconds). A swap starts playback about 250 ms behind the ring's newest sample, which gives a safety cushion without sounding stale.
- A 20 to 30 ms crossfade on every swap to avoid clicks.
- Short connect timeout; if a station has several stream URLs, try the next quickly.

## Data and delivery (owner: both portable and installer)

- Windows ships two forms from each release: a portable zip and an Inno Setup installer.
- Portable mode (builder call): a `data` folder (or marker file) next to the executable means portable; settings and stations live there. Installed mode uses the per-user config directory. Same on Linux (AppImage next to a `data` folder counts as portable). macOS always uses the per-user directory.
- Storage: plain JSON files (owner's standing preference).
- macOS: Apple Silicon only. No paid Developer ID exists, so no notarization: ad-hoc signed `.app` in a zip, same as the owner's other Mac releases (builder call). Linux: AppImage plus tarball.
- Signing (builder call): Windows binaries and installer signed with the owner's existing self-signed code signing certificate (repo secrets `WINDOWS_CODESIGN_PFX` and `WINDOWS_CODESIGN_PASSWORD`, as in the other serrebidev repos). The builder generates a new minisign key pair before the first release; private key goes in repo secret `MINISIGN_KEY` plus the owner's Vaultwarden, public key is compiled into the updater.

## Updates (builder call)

- Check GitHub Releases of `serrebidev/BlindRadio` daily and on demand. Show the changelog, terse.
- Verify downloads with minisign signatures that shipfitter writes.
- Portable Windows: download zip, swap files in place via a small helper after exit, relaunch, keep `data`. Installed Windows: download and run the new installer. macOS and Linux: download and replace the app bundle or AppImage; fall back to opening the release page if replacement fails.
- Single stable channel for now.

## Releases (owner's standing rules)

- Version 0.x to start.
- On this machine: Windows built locally by a release script; macOS on a GitHub runner; Linux built over SSH on `root@serrebiradio.com`. A `cloud-release.yml` exists for cloud agents only. Never run a cloud and local release at the same time.
- Repo carries an `AGENTS.md` describing build, test and release.

## Testing (builder call)

- Core: unit tests for directory parsing, playlist import/export, collections, settings, ICY parsing, EQ math, tuning engine state (fast arrowing ends on the right station, warm-set limits, cancellation).
- A latency check in debug builds that logs focus-to-audio time.
- Desktop: Paperback-style UIA UI tests on Windows for the key flows (tuning through a list, dialogs open and close, Escape, context menu on non-station).
- Manual NVDA test plan in `doc/`.

## Left to the builder

- Exact crates for HTTP, decoding, HLS, output, resampling and async runtime.
- Directory API details: Radio Browser API, TuneIn OPML, iHeartRadio's public endpoints, and gathering the curated Networks data.
- Buffer sizes, idle timeouts, retry timing, within the targets above.
- How station-published song history is fetched per network.
- Window layout of the main window, following YARSP's.
