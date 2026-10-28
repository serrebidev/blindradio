# Radio directories implementation plan

> **For agentic workers:** Use superpowers:subagent-driven-development or superpowers:executing-plans. Track steps below.

**Goal:** Add Icecast, SHOUTcast, Internet-Radio and verified Radio Paradise, NTS and WFMU streams.

**Architecture:** Three source adapters return existing Node and Station types. Sources merges their results with the existing providers; native browse/search controls consume them without a new interface.

**Tech Stack:** Rust 2024, reqwest, tokio, serde, quick-xml and scraper.

**Spec:** `docs/superpowers/specs/2026-10-28-radio-directories-design.md` (approved; Internet-Radio subsequently requested).

## Global constraints

- Dependencies use version "*"; hard tabs; terse errors.
- Preserve existing source serialization, network IDs, playback and user data.
- Existing search timeout is eight seconds. Filter-only search stays on Radio Browser.
- Reuse Load More and existing keyboard/screen-reader behavior.
- Implementation branches use the original target directory to reuse Windows build artifacts.

## Review focus

- Invalid or credential-bearing URLs must not become playable entries.
- Genre names containing spaces, punctuation, Unicode or @ must round-trip through node IDs.
- Pagination must not silently drop entries or navigate to another website.
- Concurrent Icecast calls must share a bounded download; errors must leave retries possible.
- An unavailable directory must not prevent results from other directories.

### Task 1: Icecast

Files: new `crates/blindradio-core/src/sources/icecast.rs`, `tests/sources_icecast.rs`.
Interface: `Icecast::new(base: impl Into<String>)`, async `children(&self, &reqwest::Client, &str) -> Result<Vec<Node>>`, async `search(&self, &reqwest::Client, &SearchQuery) -> Result<Vec<Station>>`.

- [x] Add failing fixtures for XML entities, invalid URLs/types, URL deduplication, genre IDs, 100-entry pagination, cache reuse and concurrent calls, malformed XML and retry.
- [x] Run focused tests to establish failure, then implement XML parsing and a shared 15-minute cache with a 16 MiB streaming download limit.
- [x] Re-run tests and commit the adapter with its tests.

### Task 2: SHOUTcast

Files: new `crates/blindradio-core/src/sources/shoutcast.rs`, `tests/sources_shoutcast.rs`.
Interface: `Shoutcast::new(base: impl Into<String>)`, same children/search signatures as Task 1.

- [x] Add failing fixtures for top/genre POST requests, public HTML genre links, station mapping, encoded names, pagination and malformed replies.
- [x] Run focused tests, then implement website JSON POST endpoints and public PLS URLs, with eight-second HTTP timeouts.
- [x] Re-run tests and commit the adapter with its tests.

### Task 3: Internet-Radio

Files: new `crates/blindradio-core/src/sources/internet_radio.rs`, `tests/sources_internet_radio.rs`.
Interface: `InternetRadio::new(base: impl Into<String>)`, same children/search signatures as Task 1.

- [x] Add failing HTML fixtures for featured/genre/search pages, Unicode/entities, duplicate desktop/mobile playlist links, bad stream URLs and external pagination rejection.
- [x] Run focused tests, then parse table rows and genre links with scraper. Follow same-site page links using Load More and bounded search.
- [x] Re-run tests and commit the adapter with its tests.

### Task 4: Integrate and expand Networks

Files: `sources/mod.rs`, `station.rs`, core `Cargo.toml`, `Cargo.lock`, core `tests/sources.rs`, `tests/sources_networks.rs`, app `dialogs/browse.rs`, `doc/readme.md`, `CHANGELOG.md`, `data/networks.json`.

- [x] Add dependencies and adapter module declarations needed by Tasks 1–3. Add source labels, endpoint fields and dispatch; preserve old JSON forms.
- [x] Add failing assertions for browse roots, source serialization, combined search merging/deduplication and timeout/filter-only isolation; run before changing expectations.
- [x] Add the three providers to the existing concurrent merger; update browse root expectations and guide.
- [x] Verify official stream endpoints, append only working Radio Paradise/NTS/WFMU channels, and update data tests.
- [x] Run `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo build` using vcvars64 and LIBCLANG_PATH.
- [x] Run live directory and stream checks, review the complete diff, fix actionable findings and report manual checks not performed.

## Execution notes

The user's instruction to implement remains authorization. Independent adapters can be delegated with exclusive file ownership; integration stays with the main agent. Do not ask for another approval of the already approved scope.
