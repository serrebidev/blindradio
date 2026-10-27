# BlindRadio Plan 1: Audio Engine and Dial Tuning

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Rust core that plays any internet radio stream with the lowest practical latency and tunes like an FM dial, proven by a latency harness and a bare wxDragon window that dial-tunes a list of real stations.

**Architecture:** Cargo workspace. `blindradio-core` holds networking, playlist resolution, decoding, resampling, a PCM ring per station, a cpal output mixer and the `Tuner` that keeps the focused station playing and its neighbours warm. `blindradio` is the wxDragon desktop app; in this plan it is only a test shell. Network runs on a tokio runtime; each station decodes on its own thread into its ring; the cpal callback mixes from the active ring.

**Tech Stack:** Rust 2024, tokio, reqwest 0.13 (rustls), icy-metadata 0.6, symphonia 0.6, symphonia-adapter-fdk-aac 0.2, symphonia-adapter-libopus 0.3, m3u8-rs 6, mpeg2ts-reader 0.18, rubato 5, cpal 0.18, parking_lot, crossbeam-channel, serde/serde_json, thiserror, wxdragon (git, trypsynth/wxDragon main).

**Spec:** `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md`

## Plan series

This is plan 1 of 6. Each later plan gets written after the previous one ships and is checked.

1. Audio engine and dial tuning (this plan).
2. Station sources: Radio Browser, TuneIn, iHeartRadio, curated Networks, combined search.
3. Library: favorites, recent, custom stations, collections, playlist import and export, settings storage, portable vs installed data dir.
4. Desktop UI: main window, Search, Browse, Favorites, Recent, Collections, context menus, EQ, sleep timer, song history, audio device menu with fallback, all keys from the YARSP manual.
5. Platform integration: speech announcements, global hotkeys, tray, single instance, F1 guide.
6. Updater, packaging (portable zip, installer, AppImage, macOS zip), signing, release scripts, AGENTS.md release section.

## Global Constraints

- Edition 2024. Rust stable from `~/.cargo/bin` (rustup). MSVC target on Windows.
- Dependencies unpinned: `version = "*"` for crates.io deps; git deps track default branch. Run `cargo update` before each task's first build. (Owner rule: bleeding edge, never pinned.)
- `wxdragon = { git = "https://github.com/trypsynth/wxDragon" }`; later plans add `wx-utils = { git = "https://github.com/trypsynth/wx-utils" }`.
- No wx dependency in `blindradio-core`.
- English only. Every user-visible string terse: "Connecting", "Failed: <name>", "Reconnecting", "Reconnected".
- Licence MIT.
- Native controls only. Every control has a short accessible name.
- Latency targets: warm swap under 100 ms focus-to-sound; cold start under 1 s, ideally about 300 ms, on a typical Icecast MP3/AAC stream on broadband.
- At most 3 open stations: playing plus 2 warm neighbours. Warm idle close after 25 s without focus movement.
- Warm swap plays from 1 s behind the ring's newest sample (was 250 ms; see Changes after owner listening). Ring holds 3 s. Crossfade 25 ms on every swap.

## Review Focus

1. Holding an arrow key down (key repeat about 30 per second) through 200 stations: no connection flood, never more than 3 streams plus 1 cold connect in flight, and the station focus stops on is the one that plays. Test: `tuner_key_repeat_ends_on_last_station` (Task 9).
2. A dead station (404, HTML page, connection refused, server that accepts and sends nothing): `Failed` within 5 s, silence instead of the previous station, UI thread never blocks. Tests: `open_rejects_html`, `stream_fails_on_silent_server` (Tasks 3, 6).
3. Titles that are empty, missing, Latin-1 rather than UTF-8, or contain `';` inside the title: empty means no song, Latin-1 decodes correctly, title not truncated. Test: `icy_title_latin1_and_quotes` (Task 6).
4. Streams that are mono, 22.05 kHz, 48 kHz, or 5.1, on a 48 kHz device: correct pitch, stereo out. Tests: `resample_keeps_duration`, `decode_mono_to_stereo` (Task 4).
5. Server drops mid-play and comes back within the ring cushion: no speech, no gap beyond the outage; longer outage: `Reconnecting` once, then `Reconnected` or `Failed` after about 60 s. Test: `stream_reconnects_silently` (Task 6).

---

### Task 1: Workspace, toolchain, AGENTS.md

**Files:**
- Create: `Cargo.toml`, `rustfmt.toml`, `.gitignore`, `LICENSE`, `AGENTS.md`, `crates/blindradio-core/Cargo.toml`, `crates/blindradio-core/src/lib.rs`, `crates/blindradio-core/src/error.rs`

**Interfaces:**
- Produces: `blindradio_core::CoreError` (thiserror enum: `Http(String)`, `Playlist(String)`, `Decode(String)`, `Output(String)`, `Io(#[from] std::io::Error)`), `pub type Result<T> = std::result::Result<T, CoreError>`.

- [x] **Step 1: Toolchain.** Put `~/.cargo/bin` on PATH for the shell in use. Install Ninja: `winget install --id=Ninja-build.Ninja -e`. Run `rustup update stable`. Check: `cargo --version`, `ninja --version`, `cmake --version` all print versions.
- [x] **Step 2: Workspace.** Root `Cargo.toml` with `[workspace] members = ["crates/*"]`, `resolver = "3"`, `[workspace.package] edition = "2024"`, `license = "MIT"`, version `0.1.0`. `rustfmt.toml` with `hard_tabs = true` (matches wx-utils/Paperback). `.gitignore` with `/target`. MIT `LICENSE` for "serrebidev".
- [x] **Step 3: Core crate** with `lib.rs` declaring `pub mod error; pub use error::{CoreError, Result};`. Add a test `error_displays_terse` asserting `CoreError::Http("timeout".into()).to_string() == "timeout"` (error Display is the inner text only; UI prefixes it).
- [x] **Step 4: Run** `cargo test -p blindradio-core`. Expected: 1 passed.
- [x] **Step 5: AGENTS.md** with: build (`cargo build`), test (`cargo test --workspace`), Windows prerequisites (MSVC Build Tools 2022, CMake, Ninja, run from a shell where `cl.exe` is found, otherwise `"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"` first), deps-unpinned rule, terse-strings rule, spec and plan paths. Commit file normally (owner rule: AGENTS.md is committed).
- [x] **Step 6: Commit** `chore: workspace and core crate skeleton`.

### Task 2: Playlist parsing

**Files:**
- Create: `crates/blindradio-core/src/playlist.rs`

**Interfaces:**
- Produces:
  - `pub struct PlaylistEntry { pub url: String, pub title: Option<String> }`
  - `pub enum PlaylistKind { Pls, M3u, Hls }`
  - `pub fn sniff(body: &str) -> Option<PlaylistKind>` — `[playlist]` header (case-insensitive) is Pls; any line starting `#EXT-X-` is Hls; a line starting `#EXTM3U` or any `http(s)://` line is M3u; else None.
  - `pub fn parse_pls(body: &str) -> Vec<PlaylistEntry>` — `FileN=`/`TitleN=` paired by N, ordered by N.
  - `pub fn parse_m3u(body: &str) -> Vec<PlaylistEntry>` — title from the preceding `#EXTINF:-1,Title`; skips lines that are not `http://` or `https://` (owner spec: non-URL entries skipped). Handles BOM, CRLF, blank lines.
  - Plan 3's import reuses these.

- [x] **Step 1: Write failing tests** in `playlist.rs` `mod tests`: `sniff_detects_each_kind` (Pls, Hls with `#EXT-X-TARGETDURATION`, M3u with `#EXTM3U`, M3u bare URL, None for `<html>`); `pls_pairs_titles_by_number` (File2 before File1 in text, output ordered 1,2, titles attached); `m3u_takes_extinf_titles_and_skips_local_files` (`C:\music\a.mp3` and `song.mp3` skipped); `m3u_handles_bom_and_crlf`.
- [x] **Step 2: Run** `cargo test -p blindradio-core playlist`. Expected: FAIL (not defined).
- [x] **Step 3: Implement** the four functions.
- [x] **Step 4: Run** same command. Expected: all pass.
- [x] **Step 5: Commit** `feat(core): parse PLS and M3U playlists`.

### Task 3: Shared HTTP client, opening a stream, resolve cache

**Files:**
- Create: `crates/blindradio-core/src/net.rs`, `crates/blindradio-core/tests/common/mod.rs`, `crates/blindradio-core/tests/net.rs`

**Interfaces:**
- Consumes: Task 2 `sniff`, `parse_pls`, `parse_m3u`.
- Produces:
  - `pub fn client() -> reqwest::Client` — one process-wide client (`LazyLock`), rustls, connect timeout 4 s, no total timeout, user agent `BlindRadio/<version>`, redirects followed (max 10).
  - `pub enum Opened { Audio { response: reqwest::Response, final_url: String }, Hls { playlist_url: String } }`
  - `pub async fn open(client: &reqwest::Client, url: &str) -> Result<Opened>` — sends `Icy-MetaData: 1`, `Accept-Encoding: identity`. If the response is audio, returns it still open so streaming continues on the same connection (saves a round trip). If it is a playlist (by Content-Type `audio/x-scpls`, `audio/x-mpegurl`, `audio/mpegurl`, `application/vnd.apple.mpegurl`, `application/x-mpegurl`, or by sniffing the first 64 KB when Content-Type is `text/*` or missing), PLS/M3U recurse into the first entry that opens, max depth 3; HLS returns `Hls`. HTML or anything else: `CoreError::Playlist("Not a stream")`. `final_url` is the URL after redirects.
  - `pub async fn warm_dns(hosts: Vec<String>)` — `tokio::net::lookup_host` on each host, errors ignored; warms the OS resolver cache.
  - `pub struct ResolveCache` with `pub fn load(path: &Path) -> ResolveCache` (missing or bad file gives empty), `pub fn get(&self, url: &str) -> Option<String>`, `pub fn put(&mut self, url: &str, final_url: &str)`, `pub fn save(&self) -> Result<()>`. JSON object `{ "<station url>": "<final url>" }`.
  - `tests/common/mod.rs`: `pub struct TestServer { pub base: String }`, `pub fn serve(routes: Vec<Route>) -> TestServer` (tokio TCP listener on 127.0.0.1:0 in a background runtime), `pub enum Route { Static { path, content_type, body: Vec<u8> }, Stream { path, content_type, data: Vec<u8>, metaint: Option<usize>, titles: Vec<String>, burst: usize, bytes_per_sec: usize, close_after: Option<usize> }, Silent { path }, Redirect { path, to } }`. `Stream` loops `data`, sends `burst` bytes at once then paces at `bytes_per_sec`, inserts ICY blocks every `metaint` bytes cycling `titles`, closes after `close_after` bytes if set. Tasks 6, 7, 9 reuse it.

- [x] **Step 1: Write failing tests** in `tests/net.rs`: `open_direct_audio_keeps_response` (Stream route, `Opened::Audio`, `final_url` equals route URL, first body chunk non-empty); `open_follows_pls_to_audio`; `open_follows_m3u_in_text_plain`; `open_follows_redirect_and_reports_final_url`; `open_detects_hls`; `open_rejects_html` (error text `Not a stream`); `open_stops_at_depth_3` (playlist pointing to itself); `resolve_cache_round_trip` (temp dir, put, save, load, get).
- [x] **Step 2: Run** `cargo test -p blindradio-core --test net`. Expected: FAIL.
- [x] **Step 3: Implement** `net.rs` and the test server.
- [x] **Step 4: Run** same. Expected: all pass.
- [x] **Step 5: Commit** `feat(core): open streams through playlists and redirects`.

### Task 4: Decode to stereo f32 at the device rate

**Files:**
- Create: `crates/blindradio-core/src/decode.rs`, `crates/blindradio-core/src/resample.rs`, `crates/blindradio-core/tests/fixtures/` (tone.mp3, tone.aac, tone.opus, tone.ogg, tone.flac, mono22k.mp3, surround.flac, `make-fixtures.sh`)

**Interfaces:**
- Produces:
  - `pub struct ByteReader` — `impl std::io::Read` over a `crossbeam_channel::Receiver<bytes::Bytes>`; returns 0 (EOF) when the sender is dropped. `pub fn byte_pipe() -> (crossbeam_channel::Sender<Bytes>, ByteReader)`, bounded to 64 chunks.
  - `pub struct Decoder` with `pub fn new(reader: Box<dyn std::io::Read + Send + Sync>, ext_hint: Option<&str>) -> Result<Decoder>` (wraps the reader as a non-seekable symphonia `ReadOnlySource`; registers default codecs plus `symphonia-adapter-fdk-aac` for all AAC and `symphonia-adapter-libopus` for Opus; probes formats incl. ADTS, MP3, Ogg, FLAC, MP4), `pub fn sample_rate(&self) -> u32`, `pub fn next_stereo(&mut self, out: &mut Vec<f32>) -> Result<bool>` (appends one packet as interleaved stereo; mono duplicated, more than 2 channels take front left/right; returns `false` at end of stream; a corrupt packet is skipped, not an error).
  - `pub struct Resampler` with `pub fn new(in_rate: u32, out_rate: u32) -> Resampler`, `pub fn process(&mut self, stereo_in: &[f32], out: &mut Vec<f32>)`. Passthrough when rates match. Otherwise rubato's polynomial (cubic) resampler for minimal delay; switch to sinc only if artefacts are audible in Task 10.
- Fixtures: `make-fixtures.sh` holds the ffmpeg commands (5 s 440 Hz sine; tone.* stereo 44.1 kHz; mono22k.mp3 mono 22.05 kHz; surround.flac 6 channels 48 kHz). Commit the generated files.

- [x] **Step 1: Generate fixtures** with `make-fixtures.sh`. Check each file exists and is under 100 KB.
- [x] **Step 2: Write failing tests** in `decode.rs`/`resample.rs` `mod tests`: `decodes_each_fixture` (for each tone.*: sample_rate 44100, total stereo frames within 2% of 5 s × 44100); `decode_mono_to_stereo` (mono22k.mp3: left == right for every frame, rate 22050); `decode_surround_takes_front_pair` (surround.flac: 2 channels out); `resample_keeps_duration` (1 s of 44100 stereo to 48000 gives 48000 ± 64 frames); `resample_passthrough_is_identity`; `byte_reader_eof_on_sender_drop`.
- [x] **Step 3: Run** `cargo test -p blindradio-core decode resample`. Expected: FAIL.
- [x] **Step 4: Implement.** fdk-aac and libopus build from source; if the build fails, fix the toolchain (cl, cmake on PATH) rather than dropping the adapter.
- [x] **Step 5: Run** same. Expected: all pass.
- [x] **Step 6: Commit** `feat(core): decode streams to stereo PCM`.

### Task 5: PCM ring

**Files:**
- Create: `crates/blindradio-core/src/ring.rs`

**Interfaces:**
- Produces: `pub struct PcmRing` (parking_lot `Mutex<VecDeque<f32>>`; `// ponytail: mutex shared with audio callback, held microseconds; lock-free ring if underruns show up`), `pub fn new(rate: u32, seconds: f32) -> PcmRing` (capacity in stereo samples), `pub fn push(&self, stereo: &[f32])` (drops oldest beyond capacity), `pub fn pop_into(&self, out: &mut [f32]) -> usize` (samples written, rest untouched), `pub fn len_ms(&self) -> u32`, `pub fn trim_to_ms(&self, keep_ms: u32)` (keeps newest `keep_ms`), `pub fn clear(&self)`.

- [x] **Step 1: Write failing tests:** `push_beyond_capacity_drops_oldest` (3 s ring, push 4 s of ramp values, first popped value is from second 1); `trim_keeps_newest` (`trim_to_ms(250)` leaves 250 ms, newest samples kept); `pop_partial_returns_count`; `len_ms_counts_stereo_frames`.
- [x] **Step 2: Run** `cargo test -p blindradio-core ring`. Expected: FAIL.
- [x] **Step 3: Implement.**
- [x] **Step 4: Run** same. Expected: pass.
- [x] **Step 5: Commit** `feat(core): PCM ring`.

### Task 6: Station stream (connect, ICY, decode, reconnect)

**Files:**
- Create: `crates/blindradio-core/src/station.rs`, `crates/blindradio-core/src/stream.rs`, `crates/blindradio-core/tests/stream.rs`

**Interfaces:**
- Consumes: Task 3 `open`, `client`, `ResolveCache`; Task 4 `byte_pipe`, `Decoder`, `Resampler`; Task 5 `PcmRing`.
- Produces:
  - `pub struct Station { pub id: String, pub name: String, pub url: String }` (`Clone`, `Serialize`, `Deserialize`). Plans 2 and 3 add fields.
  - `pub struct EngineCtx { pub rt: tokio::runtime::Handle, pub client: reqwest::Client, pub cache: Arc<parking_lot::Mutex<ResolveCache>>, pub out_rate: u32, pub retry_window: Duration }` (`Clone`; `retry_window` 60 s in real use)
  - `pub enum StreamEvent { FirstAudio, Title(Option<String>), Reconnecting, Reconnected, Failed(String) }`
  - `pub struct StationStream { pub ring: Arc<PcmRing>, pub station: Station }`, `pub fn start(station: &Station, ctx: &EngineCtx, on_event: Box<dyn Fn(StreamEvent) + Send + Sync>) -> StationStream`. Dropping it aborts the network task and ends the decode thread (pipe sender dropped).
  - Behaviour: opens the cached final URL first, falls back to the station URL if that fails, updates the cache on success. ICY via `icy-metadata`; title decoded as UTF-8, else Latin-1; empty `StreamTitle` is `Title(None)`. HLS streams hand off to Task 7's `HlsReader` (stub returning `Failed("HLS")` until Task 7). Decode thread: `Decoder` → `Resampler` to `out_rate` → `ring.push`; `FirstAudio` once, after the first push. Ring 3 s.
  - Failure: no audio bytes within 5 s of connect, or open error: `Failed(<terse reason>)`. Reasons: `Not a stream`, `No data`, `Unreachable`, `Unsupported format`.
  - Reconnect: on read error or EOF, reconnect at once without an event while the ring still has audio; emit `Reconnecting` once only if the ring runs dry; keep retrying (1 s, 2 s, 4 s, then every 5 s) up to 60 s; then `Reconnected` (if `Reconnecting` was sent) or `Failed("Lost")`. Decoder is recreated per connection.

- [x] **Step 1: Write failing tests** in `tests/stream.rs` using the Task 3 server with tone.mp3 data: `stream_fills_ring_and_reports_first_audio` (FirstAudio within 1 s, ring `len_ms` > 0); `stream_reports_icy_titles` (metaint 8192, titles `["A - One", ""]`: events `Title(Some("A - One"))` then `Title(None)`); `icy_title_latin1_and_quotes` (server sends the title `Café - It's` encoded as Latin-1 bytes: event is `Title(Some("Café - It's"))`, apostrophe kept); `stream_fails_on_silent_server` (Silent route: `Failed("No data")` within 6 s); `stream_reconnects_silently` (close_after 64 KB at 128 kbps pacing, server keeps accepting: no `Reconnecting` event, ring never empty for more than 300 ms); `stream_reports_lost_after_retries` (server stops accepting, `EngineCtx.retry_window` set to 3 s: `Reconnecting` then `Failed("Lost")`).
- [x] **Step 2: Run** `cargo test -p blindradio-core --test stream`. Expected: FAIL.
- [x] **Step 3: Implement.**
- [x] **Step 4: Run** same. Expected: all pass.
- [x] **Step 5: Commit** `feat(core): station stream with ICY titles and reconnect`.

### Task 7: HLS reader

**Files:**
- Create: `crates/blindradio-core/src/hls.rs`, `crates/blindradio-core/tests/hls.rs`, `crates/blindradio-core/tests/fixtures/hls/` (`live.m3u8`, `master.m3u8`, `seg0.ts`..`seg4.ts`, `aac0.aac`..`aac4.aac`)
- Modify: `crates/blindradio-core/src/stream.rs` (replace the HLS stub)

**Interfaces:**
- Consumes: Task 3 `client`; Task 4 `byte_pipe`.
- Produces: `pub async fn run_hls(client: reqwest::Client, playlist_url: String, tx: crossbeam_channel::Sender<Bytes>, on_title: Box<dyn Fn(Option<String>) + Send + Sync>) -> Result<()>` — master playlist: pick the audio-only variant, else lowest bandwidth one with audio (fastest start). Media playlist: start at the third-from-last segment (live edge with cushion), fetch segments in order, reload every target duration, skip segments already sent (by media sequence). Segment payload: `.ts` → audio elementary stream (ADTS or MP3) extracted with mpeg2ts-reader; raw `.aac`/`.mp3` passed through. fMP4 (`#EXT-X-MAP`) returns `CoreError::Decode("Unsupported format")` — `// ponytail: fMP4 HLS unsupported; add when a wanted network needs it`. ID3 `TIT2` in TS timed metadata or `#EXTINF` titles feed `on_title` when present. AES-128 unsupported for now, same error.
- Fixtures: ffmpeg `-f hls -hls_time 1` for TS and `-hls_segment_type mpegts` with `-c:a aac`; raw AAC segments via `-f segment`. Commands appended to `make-fixtures.sh`.

- [x] **Step 1: Write failing tests:** `hls_ts_segments_decode` (served live.m3u8 + TS; Decoder over the pipe yields at least 3 s of audio); `hls_master_picks_audio_only_variant`; `hls_live_reload_appends_new_segments` (server adds seg5 after first load; it is fetched once); `hls_raw_aac_segments_decode`; `hls_fmp4_is_unsupported`.
- [x] **Step 2: Run** `cargo test -p blindradio-core --test hls`. Expected: FAIL.
- [x] **Step 3: Implement** and wire into `stream.rs` (an `Opened::Hls` spawns `run_hls` feeding the same decode thread, ext hint from the segment type).
- [x] **Step 4: Run** `cargo test -p blindradio-core`. Expected: all pass.
- [x] **Step 5: Commit** `feat(core): HLS live streams`.

### Task 8: Output mixer

**Files:**
- Create: `crates/blindradio-core/src/output.rs`

**Interfaces:**
- Consumes: Task 5 `PcmRing`.
- Produces:
  - `pub enum MixerCmd { SetActive { ring: Option<Arc<PcmRing>>, token: u64 }, Volume(f32), Mute(bool) }`
  - `pub struct MixerCore` with `pub fn new(rate: u32) -> MixerCore` (crossfade 25 ms), `pub fn handle(&mut self, cmd: MixerCmd)`, `pub fn render(&mut self, out: &mut [f32]) -> Option<u64>` (stereo interleaved; pulls from active ring; during a crossfade, mixes old ring fading out and new fading in, equal-power; ring underrun fills silence; returns the token the first time real samples from that token's ring are written). Volume 0.0 to 1.0, applied after mix; mute outputs silence but keeps pulling so audio stays live.
  - `pub struct Output` with `pub fn start_default() -> Result<Output>`, `pub sample_rate: u32`, `pub fn send(&self, cmd: MixerCmd)`, `pub fn rendered(&self) -> &crossbeam_channel::Receiver<(u64, std::time::Instant)>`. Requests f32 stereo at the device default rate and `BufferSize::Fixed` of about 10 ms of frames when the device range allows, else default. Device with more than 2 channels: stereo goes to the first two, rest silent. Not `Send` (cpal stream); lives on the UI thread. Commands reach the callback through a crossbeam channel drained at the start of each `render`.

- [x] **Step 1: Write failing tests** on `MixerCore` only (no device): `render_reads_active_ring`; `crossfade_spans_25_ms` (switch from ring of 1.0s to ring of -1.0s; after exactly 25 ms of frames output equals new ring); `render_reports_token_on_first_real_sample` (empty ring: None; after push: Some(token) once); `underrun_outputs_silence`; `mute_keeps_draining_ring`; `volume_scales_output`.
- [x] **Step 2: Run** `cargo test -p blindradio-core output`. Expected: FAIL.
- [x] **Step 3: Implement** `MixerCore`, then `Output` around cpal (not unit tested).
- [x] **Step 4: Run** same. Expected: pass.
- [x] **Step 5: Commit** `feat(core): output mixer with crossfade`.

### Task 9: Tuner (dial tuning)

**Files:**
- Create: `crates/blindradio-core/src/tuner.rs`, `crates/blindradio-core/tests/tuner.rs`

**Interfaces:**
- Consumes: Task 6 `Station`, `EngineCtx`, `StationStream`, `StreamEvent`; Task 8 `Output`, `MixerCmd`.
- Produces:
  - `pub fn wanted(len: usize, index: usize) -> (usize, Vec<usize>)` — the station to play and its warm neighbours (index-1 and index+1 when they exist).
  - `pub struct Settle` — title announcement rule: `pub fn focused(&mut self, now: Instant)`, `pub fn title(&mut self, title: Option<String>, now: Instant) -> Option<String>` (returns the title to announce now, or holds it), `pub fn tick(&mut self, now: Instant) -> Option<String>` (releases a held title once focus has rested 1.5 s). After release, later titles announce at once until the next `focused`.
  - `pub enum TunerEvent { Playing { station_id: String }, Title { station_id: String, title: Option<String> }, Announce(String), Reconnecting { station_id: String }, Reconnected { station_id: String }, Failed { station_id: String, name: String, reason: String }, Latency { station_id: String, ms: u32, warm: bool } }`
  - `pub trait StreamFactory { fn start(&self, station: &Station, on_event: Box<dyn Fn(StreamEvent) + Send + Sync>) -> (Arc<PcmRing>, Box<dyn Send>); }` with the real impl over `StationStream` and a fake in tests (the `Box<dyn Send>` is the drop guard).
  - `pub struct LiveFactory(pub EngineCtx)` — the real `StreamFactory`; also exposes `pub fn warm_dns(&self, stations: &[Station])` spawning `net::warm_dns` for hosts not warmed before. `StreamFactory` gets a default method `fn warm_dns(&self, _stations: &[Station]) {}` so the fake ignores it.
  - `pub trait Sink { fn send(&self, cmd: MixerCmd); }` implemented by `Output` and a test recorder.
  - `pub struct Tuner<F: StreamFactory, S: Sink>` with `pub fn new(factory: F, sink: S, events: crossbeam_channel::Sender<TunerEvent>) -> Tuner<F, S>`, `pub fn focus(&mut self, stations: &[Station], index: usize, now: Instant)`, `pub fn play(&mut self, station: &Station, now: Instant)`, `pub fn stop(&mut self)`, `pub fn poll(&mut self, now: Instant, rendered: &[(u64, Instant)])`, `pub fn playing(&self) -> Option<&Station>`.
  - Rules:
    - `focus`: if the target is already open (warm), swap to it at once: `trim_to_ms(250)` then `SetActive`. If not, `SetActive { ring: None }` immediately (silence, never the old station), then start it cold. Exception for key repeat: if the previous `focus` was under 100 ms ago, defer the cold start until 100 ms pass with no further `focus` (`poll` starts it). Warm swaps are never deferred.
    - After each `focus`, drop every open stream not in `wanted` except the one playing. Never more than 3 open plus 1 deferred.
    - Each `focus` calls `factory.warm_dns` with the stations within 5 of `index`.
    - After `stop`, the next `focus` or `play` plays again (owner rule).
    - `poll`: starts due deferred cold starts; turns rendered tokens into `Latency` (focus instant to render instant; `warm` true if the swap was warm) and `Playing`; closes warm (non-playing) streams after 25 s with no `focus`; forwards events of the playing station only; keeps the latest title of warm stations so a warm swap emits `Title` at once; drives `Settle`.
    - `Failed` uses the station name: UI speaks `Failed: <name>`.

- [x] **Step 1: Write failing tests** in `tests/tuner.rs` with the fake factory and recording sink: `wanted_edges` (len 1, first, last, middle); `focus_plays_target_and_warms_neighbours` (3 starts: 4, 3, 5 order irrelevant; SetActive ring is station 4's); `moving_to_warm_neighbour_swaps_without_new_connect_for_it` (focus 4 then 5: no new start for 5, starts 6, drops 3); `cold_focus_silences_old_station_first` (first command after focus to a cold station is `SetActive { ring: None }`); `tuner_key_repeat_ends_on_last_station` (200 stations, focus 0..199 at 33 ms apart; open streams never exceed 3 + 1 deferred; after final poll at +150 ms the active ring is station 199's); `stop_then_focus_plays_again`; `warm_streams_close_after_25_s_idle`; `latency_event_marks_warm_swaps`; `settle_holds_title_until_rested` (title at +0.5 s held, released at +1.5 s, next title at +3 s immediate, new focus resets); `events_from_warm_stations_not_forwarded`.
- [x] **Step 2: Run** `cargo test -p blindradio-core --test tuner`. Expected: FAIL.
- [x] **Step 3: Implement.**
- [x] **Step 4: Run** `cargo test -p blindradio-core`. Expected: all pass.
- [x] **Step 5: Commit** `feat(core): dial tuner with warm neighbours`.

### Task 10: Latency harness and real-station check

**Files:**
- Create: `crates/blindradio-core/examples/tune.rs`, `crates/blindradio-core/examples/stations.json`, `crates/blindradio-core/tests/live.rs`
- Modify: `crates/blindradio-core/src/lib.rs` (pub `Engine` convenience: `pub struct Engine { pub rt: tokio::runtime::Runtime, pub ctx: EngineCtx }`, `impl Engine { pub fn new(cache_path: &Path, out_rate: u32) -> Result<Engine> }` (multi-thread runtime, 2 workers, `retry_window` 60 s) — used by the example and the desktop app)

**Interfaces:**
- Consumes: Task 9 `Tuner`, Task 8 `Output`, Task 6 `EngineCtx`.
- `stations.json`: `[Station]` with at least: SomaFM Groove Salad MP3 (`https://ice1.somafm.com/groovesalad-128-mp3`), SomaFM Groove Salad AAC (`https://ice1.somafm.com/groovesalad-128-aac`), SomaFM Drone Zone MP3, Radio Paradise AAC 128 (`http://stream.radioparadise.com/aac-128`), one HE-AAC station (find one in Radio Browser with codec `AAC+`), one Ogg Opus station, one PLS-only URL, one HLS station with TS segments (find via Radio Browser `hls=1`; record which BBC streams are TS vs fMP4), FIP (`https://icecast.radiofrance.fr/fip-hifi.aac`). Verify each URL still works before committing.

- [x] **Step 1: Write** `examples/tune.rs`: loads `stations.json`, `Output::start_default()`, `Tuner`; walks focus 0→N→0 every 3 s, then a burst of fast steps (100 ms apart); prints one line per `Latency` (`<name> warm=<bool> <ms> ms`) and per `Failed`; prints median warm and median cold at the end.
- [x] **Step 2: Write** `tests/live.rs`, every test `#[ignore]` (network): `each_dev_station_produces_audio` (FirstAudio within 3 s per station, via `StationStream`, no output device needed); `he_aac_station_decodes`.
- [x] **Step 3: Run** `cargo test -p blindradio-core --test live -- --ignored`. Expected: all pass. Replace any dead station URL.
- [x] **Step 4: Run** `cargo run -p blindradio-core --example tune --release`. Expected: median warm under 100 ms, median cold under 1000 ms. Listen: no clicks on swaps, correct pitch, no stale audio after fast steps. If a target misses, profile where the time goes (DNS, connect, TLS, first byte, probe, first render) with timestamps logged per stage before changing anything.
- [x] **Step 5: Record** the measured medians and per-stage timings in `doc/latency.md` (plain list).
- [x] **Step 6: Commit** `feat(core): latency harness and live station checks`.

### Task 11: Desktop test shell

**Files:**
- Create: `crates/blindradio/Cargo.toml`, `crates/blindradio/build.rs`, `crates/blindradio/src/main.rs`

**Interfaces:**
- Consumes: Task 10 `Engine`, Task 9 `Tuner`, Task 8 `Output`, `examples/stations.json` (embedded with `include_str!`).
- Window: one `Frame` titled `BlindRadio`, a `StaticText` `Stations` created before a `ListBox` (wxMSW reads the preceding label as the list's name), list of station names. First item selected and focused at start, which plays it (owner rule: landing on a station plays it). Selection change calls `tuner.focus(&stations, index, Instant::now())`. A wx `Timer` every 50 ms calls `tuner.poll(now, &drained_rendered)` and handles events: `Title` sets the window title to `<song> - <station>` or `<station>`; `Failed` and `Announce` only print to stderr in this plan (speech arrives in plan 5). Ctrl+S calls `stop`. Escape closes. No other controls.
- `build.rs`: Windows manifest as in the wxDragon README (Common Controls v6, PerMonitorV2, UTF-8) via `embed-manifest`. Shipfitter replaces this in plan 6.

- [x] **Step 1: Write** the crate. Run `cargo build -p blindradio`. Expected: builds (first build downloads prebuilt wxWidgets).
- [x] **Step 2: Manual NVDA check.** Run `cargo run -p blindradio --release`. Expected: NVDA reads `Stations list`, then the first station; audio starts. Arrow down and up: each station plays as focus lands, nothing extra spoken, window title shows the song when known. Hold Down arrow: lands and plays the final station. Ctrl+S silences; next arrow plays. Escape closes and audio stops. Note anything that felt slow or wrong in `doc/latency.md`.
- [x] **Step 3: Commit** `feat(desktop): dial tuning test shell`.

## Changes after owner listening (2026-10-27)

The owner tested the desktop shell by ear. Each change was measured first with `examples/probe.rs` (real mixer, 10 ms ticks) and is recorded in `doc/latency.md`.

- Ring (Task 5): never drops audio. A full ring makes its producer wait, so the connection pauses. The plan's "drops oldest beyond capacity" lost about 13.5 s per 20 s on SomaFM bursts and starved BBC HLS between segments. `push_beyond_capacity_drops_oldest` became `full_ring_waits_until_closed` and `closed_ring_discards_what_does_not_fit`. `StationStream` closes its ring on drop.
- Mixer (Task 8): each fading station keeps its own fade; audio fades in from silence; an underrun eases out over 5 ms. Playback starts, and resumes after running dry, only once 1 s is buffered (`ring::CUSHION_MS`): amperwave (Fox News) sends its first seconds in lumps up to a second apart.
- Tuner (Task 9): a warm swap trims to the 1 s cushion instead of 250 ms. A cold focus sends `MixerCmd::Tune`: the old station fades into soft hiss until the new one sounds (owner: feel analogue, no pauses). Stop stays silent. No DNS warming under key repeat. A warm station's earlier `Reconnecting` is passed on when it is swapped to.
- Resolve cache (Task 3): final URLs carrying their own query (session id, token) are not cached.
- User-Agent (Task 3): `Blind-Radio/<version>`. SomaFM drops any agent containing "blindradio".
- HLS (Task 7): ID3 `TIT2` titles come from TS timed metadata (stream type 0x15) and packed-audio tags.
