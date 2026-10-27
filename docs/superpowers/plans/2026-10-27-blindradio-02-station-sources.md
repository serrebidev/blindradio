# BlindRadio Plan 2: Station Sources Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Every station directory the spec names, searchable at once and browsable as one tree: Radio Browser, TuneIn, iHeartRadio and the curated Networks, plus station-published song history where a network offers it.

**Architecture:** One module per directory under `crates/blindradio-core/src/sources/`, each turning its API into the shared `Node` and `Station` types. A `Sources` facade owns the endpoints, dispatches browse node ids to the right directory, runs a combined search across all of them concurrently, and fetches published song history. Endpoints are injectable so tests run against the plan 1 test server with recorded JSON fixtures; live checks are `#[ignore]`.

**Tech Stack:** Rust 2024, reqwest (shared `net::client()`), tokio, serde/serde_json. No new crates unless a step names one.

**Spec:** `docs/superpowers/specs/2026-10-26-blindradio-desktop-design.md` (Feature list: Search, Browse, Song history; Left to the builder: directory API details). Behaviour reference: `docs/reference/yarsp-manual.html` (Searching, Browsing, Song history).

## Global Constraints

- Edition 2024, hard tabs, dependencies `version = "*"`, run `cargo update` before the first build of each task.
- No wx dependency in `blindradio-core`.
- Every user-visible string terse. Directory labels exactly: `Radio Browser`, `TuneIn`, `iHeartRadio`, and each network's own name (`BBC`, `NPR`, `CBC`, `ABC Australia`, `Radio France`, `Deutschlandfunk`, `SomaFM`, `Stream Madness`).
- User-Agent stays `Blind-Radio/<version>` (SomaFM drops agents containing "blindradio").
- A directory that fails or times out never fails the search or the tree; it is left out.
- Station JSON written by plan 1 (`{id, name, url}`) must still load.

## Review Focus

1. A directory that is down, slow (over 8 s) or returns HTML instead of JSON: the combined search still returns the others' results within 8 s. Test: `search_survives_dead_directory` (Task 6).
2. The same stream found in two directories: listed once, from the first directory in order Radio Browser, TuneIn, iHeartRadio, Networks. Test: `search_dedupes_by_stream_url` (Task 6).
3. Station names with HTML entities, surrounding whitespace or empty names (Radio Browser has all three): names are decoded and trimmed, an empty name falls back to the stream host. Test: `rb_station_names_cleaned` (Task 2).
4. Load More past the end: an empty page gives no stations and no further Load More. Test: `rb_load_more_ends` (Task 2).
5. TuneIn outlines that are not playable (artists, shows, `notcompatible` audio): never become stations. Test: `tunein_skips_non_stations` (Task 3).

---

### Task 1: Station source, ids and JSON fetch

**Files:**
- Modify: `crates/blindradio-core/src/station.rs`, `crates/blindradio-core/src/net.rs`, `crates/blindradio-core/src/lib.rs`
- Create: `crates/blindradio-core/src/sources/mod.rs`

**Interfaces:**
- Produces:
  - `pub enum Source { RadioBrowser, TuneIn, IHeart, Network(String), Custom }` (`Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default` with `Custom` as default) and `pub fn label(&self) -> &str` (the directory labels above; `Custom` is `Custom`).
  - `Station` gains `#[serde(default)] pub source: Source` and `#[serde(default, skip_serializing_if = "Option::is_none")] pub info: Option<String>` (a short second column, e.g. `Jazz, France, AAC 128k`).
  - Id scheme (one string, stable across runs): `rb:<stationuuid>`, `tunein:<guide_id>`, `iheart:<id>`, `net:<network-slug>:<slug>`, `custom:<uuid-v4>`.
  - `pub async fn get_json<T: DeserializeOwned>(client: &reqwest::Client, url: &str) -> Result<T>` in `net.rs`: 8 s total timeout, HTTP status >= 400 is `CoreError::Http("HTTP <code>")`, a body that is not JSON is `CoreError::Http("Bad reply")`.
  - `pub enum Node { Folder { id: String, name: String }, Station(Station), LoadMore { id: String } }` in `sources/mod.rs` (`Clone, Debug, PartialEq`).

- [ ] **Step 1: Write failing tests** in `station.rs`: `plan1_json_still_loads` (`{"id":"a","name":"A","url":"http://x"}` deserializes with `source == Source::Custom`, `info == None`); `source_labels` (each variant's label as listed). In `tests/net.rs`: `get_json_reads_and_rejects` (Static JSON route parses; `text/html` route gives `Bad reply`; 404 gives `HTTP 404`).
- [ ] **Step 2: Run** `cargo test -p blindradio-core station get_json`. Expected: FAIL.
- [ ] **Step 3: Implement.** Add `uuid = { version = "*", features = ["v4"] }` for custom ids. Update every `Station { .. }` literal in the crate and tests (`..` with `source`/`info`).
- [ ] **Step 4: Run** `cargo test -p blindradio-core`. Expected: all pass.
- [ ] **Step 5: Commit** `feat(core): station source and directory ids`.

### Task 2: Radio Browser

**Files:**
- Create: `crates/blindradio-core/src/sources/radio_browser.rs`, `crates/blindradio-core/tests/sources_rb.rs`, `crates/blindradio-core/tests/fixtures/sources/rb_*.json`

**Interfaces:**
- Consumes: Task 1 `get_json`, `Node`, `Station`, `Source`.
- Produces: `pub struct RadioBrowser { base: String }` with `pub fn new(base: impl Into<String>) -> RadioBrowser` (live base `https://de1.api.radio-browser.info`; `all.api.radio-browser.info/json/servers` lists only de1 as of 2026-10-27), and:
  - `pub async fn children(&self, client, id: &str) -> Result<Vec<Node>>` for ids `rb` (Folders `rb/genres` "By Genre", `rb/countries` "By Country"), `rb/genres` (top 200 tags by `stationcount`, from `/json/tags?order=stationcount&reverse=true&limit=200&hidebroken=true`), `rb/genre/<tag>@<offset>`, `rb/countries` (`/json/countries?order=name&hidebroken=true`, name with count is not shown, just the name), `rb/country/<iso>` (if `stationcount >= 1000`: Folder `rb/country/<iso>/all` "All stations" first, then one Folder per state from `/json/states/<country name>/?order=name&hidebroken=true`; else the stations), `rb/country/<iso>/all@<offset>`, `rb/country/<iso>/state/<state>@<offset>`.
  - Station pages: 100 per page, `order=clickcount&reverse=true&hidebroken=true`; a full page ends with `Node::LoadMore { id: <same id>@<offset+100> }`.
  - `pub async fn search(&self, client, q: &SearchQuery) -> Result<Vec<Station>>` via `/json/stations/search?name=&tag=&country=&language=&limit=100&order=clickcount&reverse=true&hidebroken=true` (empty fields omitted).
  - `pub async fn genres(&self, client) -> Result<Vec<String>>`, `countries(&self, client) -> Result<Vec<String>>` (names), `languages(&self, client) -> Result<Vec<String>>` for the Search combos (top 200 each by stationcount, then sorted by name).
  - `SearchQuery { pub name: String, pub genre: Option<String>, pub country: Option<String>, pub language: Option<String> }` lives in `sources/mod.rs`.
  - Station mapping: id `rb:<stationuuid>`, url `url_resolved` else `url`, info from first tag, `country`, `codec` and `bitrate` (`Jazz, France, AAC 128k`, parts omitted when empty or 0), source `RadioBrowser`. Names HTML-entity-decoded and trimmed; empty name falls back to the URL host. Path segments in ids are percent-encoded.

- [ ] **Step 1: Record fixtures** from the live API into `tests/fixtures/sources/` (`rb_tags.json`, `rb_countries.json`, `rb_states_us.json`, `rb_stations_page.json` with exactly 100 entries, `rb_stations_short.json` with 3, `rb_search.json`), then hand-edit `rb_stations_short.json` so one name has `&amp;`, one has surrounding spaces and one is empty.
- [ ] **Step 2: Write failing tests** in `tests/sources_rb.rs` against Static routes: `rb_root_has_genre_and_country`; `rb_big_country_splits_by_state` (`US` children start with "All stations", then states); `rb_small_country_lists_stations`; `rb_full_page_ends_with_load_more` (101 nodes, last is `LoadMore` with `@100`); `rb_load_more_ends` (an empty page gives `[]`); `rb_station_names_cleaned` (`A & B`, trimmed, host fallback); `rb_station_info_and_id`; `rb_search_sends_only_given_filters` (route records the query string; `genre` set, `country` unset → `tag=` present, `country=` absent).
- [ ] **Step 3: Run** `cargo test -p blindradio-core --test sources_rb`. Expected: FAIL.
- [ ] **Step 4: Implement.** If the test server cannot assert query strings, extend `tests/common/mod.rs` with `TestServer::last_query(path) -> Option<String>`.
- [ ] **Step 5: Run** same. Expected: all pass.
- [ ] **Step 6: Commit** `feat(core): Radio Browser directory`.

### Task 3: TuneIn

**Files:**
- Create: `crates/blindradio-core/src/sources/tunein.rs`, `crates/blindradio-core/tests/sources_tunein.rs`, `tests/fixtures/sources/tunein_*.json`

**Interfaces:**
- Produces: `pub struct TuneIn { base: String }` (live `https://opml.radiotime.com`), `pub async fn children(&self, client, id: &str)` for `tunein` (= `Browse.ashx?id=r0&render=json`, "By Location") and `tunein/<guide_id>` (`Browse.ashx?id=<guide_id>&render=json`), and `pub async fn search(&self, client, q: &SearchQuery)` (`Search.ashx?query=<name>&types=station&render=json`; genre, country and language are not supported by TuneIn and ignored).
- Mapping: outline `type == "link"` with a `guide_id` is a Folder `tunein/<guide_id>`; outline `type == "audio"` with `item == "station"` and a `guide_id` starting `s` is a Station: id `tunein:<guide_id>`, name `text`, url `https://opml.radiotime.com/Tune.ashx?id=<guide_id>&formats=mp3,aac,ogg,hls` (plain-text reply of stream URLs that `net::open` already resolves), info from `subtext`. Outlines nested under `children` (TuneIn groups "Stations", "Shows") are flattened, keeping only those kinds. Everything else is skipped.

- [ ] **Step 1: Record fixtures:** `tunein_root.json` (`r0`), one continent, one country, one city with stations and grouped `children`, `tunein_search.json` (contains artist links that must be skipped).
- [ ] **Step 2: Write failing tests:** `tunein_location_folders`; `tunein_city_stations_flattened` (stations from inside `children` groups appear); `tunein_station_url_uses_tune`; `tunein_skips_non_stations` (artists, shows, `notcompatible` and outlines without `guide_id` are dropped); `tunein_search_ignores_filters`.
- [ ] **Step 3: Run** `cargo test -p blindradio-core --test sources_tunein`. Expected: FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** same. Expected: pass. Add an `#[ignore]` test in `tests/live.rs` `tunein_station_plays` (a station from live `r0` drill-down reaches FirstAudio within 5 s).
- [ ] **Step 6: Commit** `feat(core): TuneIn directory`.

### Task 4: iHeartRadio

**Files:**
- Create: `crates/blindradio-core/src/sources/iheart.rs`, `crates/blindradio-core/tests/sources_iheart.rs`, `tests/fixtures/sources/iheart_*.json`

**Interfaces:**
- Produces: `pub struct IHeart { base: String }` (live `https://api.iheart.com`), `children` for `iheart` (Folders `iheart/all` "All Stations", `iheart/genres` "Genres"), `iheart/genres` (`/api/v2/content/genre?limit=100`, Folders `iheart/genre/<id>` sorted by `sort`), `iheart/all@<offset>` and `iheart/genre/<id>@<offset>` (`/api/v2/content/liveStations?countryCode=US&limit=100&offset=<n>[&genreId=<id>]`, LoadMore while `offset + 100 < total`), and `search` (`/api/v3/search/all?keywords=<name>&maxRows=50&station=true&artist=false&album=false&track=false&playlist=false&podcast=false`, then the station ids looked up with `/api/v2/content/liveStations?id=<id>&id=<id>...` in one request to get streams; filters ignored).
- Mapping: id `iheart:<id>`, name `name`, info `<freq> <band>, <city>` when present, url the first present of `streams.secure_shoutcast_stream`, `streams.shoutcast_stream`, `streams.secure_hls_stream`, `streams.hls_stream` (ICY first: titles and the lowest start latency); a station with none is skipped.
- `pub async fn history(&self, client, id: &str) -> Result<Vec<HistoryItem>>` via `/api/v3/live-meta/stream/<id>/trackHistory?limit=20` (`HistoryItem { pub title: String, pub played_at: Option<std::time::SystemTime> }` in `sources/mod.rs`; title `Artist - Title`; `startTime` is epoch seconds).

- [ ] **Step 1: Record fixtures** (`iheart_genres.json`, `iheart_live_page.json` with total > 100, `iheart_live_last.json`, `iheart_search.json`, `iheart_lookup.json`, `iheart_history.json`).
- [ ] **Step 2: Write failing tests:** `iheart_root`; `iheart_genres_sorted`; `iheart_pages_until_total`; `iheart_prefers_icy_stream`; `iheart_search_looks_up_streams` (two requests, results keep search order); `iheart_history_titles_and_times`.
- [ ] **Step 3: Run** `cargo test -p blindradio-core --test sources_iheart`. Expected: FAIL.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** same. Expected: pass.
- [ ] **Step 6: Commit** `feat(core): iHeartRadio directory`.

### Task 5: Curated Networks

**Files:**
- Create: `crates/blindradio-core/data/networks.json`, `crates/blindradio-core/src/sources/networks.rs`, `crates/blindradio-core/examples/check_networks.rs`, `crates/blindradio-core/tests/sources_networks.rs`

**Interfaces:**
- Produces: `networks.json` is `[{ "name": "BBC", "slug": "bbc", "stations": [{ "slug": "radio-4", "name": "Radio 4", "url": "...", "info": "..." , "history": "<optional provider key>" }] }]`, embedded with `include_str!`. `pub fn networks() -> &'static [Network]` (parsed once, `LazyLock`), `pub fn children(id: &str) -> Vec<Node>` for `net` (one Folder per network `net/<slug>`, in the order above) and `net/<slug>` (its stations, id `net:<slug>:<station-slug>`, source `Network(<name>)`), `pub fn search(q: &SearchQuery) -> Vec<Station>` (case-insensitive name contains, filters ignored).
- SomaFM: generated, not hand-written: `check_networks` has a `--refresh-somafm` flag that rewrites the SomaFM entry from `https://api.somafm.com/channels.json` (url `https://ice1.somafm.com/<id>-128-mp3`, falling back to the highest `playlists` mp3 entry; info from `genre`; history key `somafm:<id>`).
- Content to gather (each URL checked live; prefer direct Icecast or HLS that plays without tokens; record in the entry's `info` the codec):
  - BBC: the national networks (Radio 1, 1Xtra, 2, 3, 4, 4 Extra, 5 Live, 6 Music, Asian Network, World Service), the nations (Scotland, nan Gàidheal, Wales, Cymru, Ulster, Foyle) and all English local stations. Use the `as-hls-ww-live.akamaized.net/.../*.isml/*-audio%3d128000.norewind.m3u8` form (TS, plays outside the UK); World Service also has `http://stream.live.vc.bbcmedia.co.uk/bbc_world_service`.
  - NPR: NPR's own streams plus the 30 largest member stations' main streams.
  - CBC: Radio One for each region, CBC Music for each region, Ici Première Montréal.
  - ABC Australia: Radio National, Classic, triple j, Double J, Unearthed, NewsRadio, Jazz, Country, and each capital-city local station.
  - Radio France: every `icecast.radiofrance.fr` channel (France Inter, franceinfo, France Culture, France Musique and its webradios, FIP and its webradios, Mouv', the 44 ici locales), `-hifi.aac` where offered.
  - Deutschlandfunk: Deutschlandfunk, Deutschlandfunk Kultur, Deutschlandfunk Nova (Opus or AAC, both checked).
  - SomaFM: all channels (generated).
  - Stream Madness: every station on streammadness.net with its stream URL.
- `check_networks` (no flag) opens every station with `net::open` concurrently (16 at a time, 8 s each) and prints `FAIL <network>/<slug> <reason>` lines and a final `ok N/M`.

- [ ] **Step 1: Write failing tests:** `networks_parse_and_order` (8 networks, names and order exactly as listed above, each with at least one station, every id unique, every url `http(s)://`); `networks_children_and_ids`; `networks_search_by_name`.
- [ ] **Step 2: Run** `cargo test -p blindradio-core --test sources_networks`. Expected: FAIL.
- [ ] **Step 3: Gather the data** into `networks.json` and implement `networks.rs` and `check_networks`.
- [ ] **Step 4: Run** `cargo run -p blindradio-core --example check_networks --release`. Expected: every entry ok; replace or remove any that fail (note removals in the commit body).
- [ ] **Step 5: Run** the tests. Expected: pass.
- [ ] **Step 6: Commit** `feat(core): curated networks`.

### Task 6: Sources facade, combined search, published history

**Files:**
- Modify: `crates/blindradio-core/src/sources/mod.rs`, `crates/blindradio-core/tests/live.rs`
- Create: `crates/blindradio-core/tests/sources.rs`

**Interfaces:**
- Consumes: Tasks 2 to 5.
- Produces: `pub struct Endpoints { pub radio_browser: String, pub tunein: String, pub iheart: String, pub somafm: String }` with `impl Default` (live bases; SomaFM `https://somafm.com`); `pub struct Sources { client: reqwest::Client, rb, tunein, iheart, somafm_base }` with:
  - `pub fn new(client: reqwest::Client, endpoints: Endpoints) -> Sources`.
  - `pub fn roots() -> Vec<Node>`: Folders `rb` "Radio Browser", `tunein` "TuneIn", `iheart` "iHeartRadio", `net` "Networks" (Collections and Custom Stations are added by the UI in plan 4).
  - `pub async fn children(&self, id: &str) -> Result<Vec<Node>>` dispatching on the id prefix.
  - `pub async fn search(&self, q: &SearchQuery) -> Vec<Station>`: all four directories concurrently, each under an 8 s timeout, failures dropped; results concatenated in order Radio Browser, TuneIn, iHeartRadio, Networks, deduped by stream URL (first kept).
  - `pub async fn filters(&self) -> Filters` (`Filters { pub genres, pub countries, pub languages: Vec<String> }` from Radio Browser; empty vectors if it fails).
  - `pub async fn history(&self, station: &Station) -> Option<Vec<HistoryItem>>`: iHeart stations via Task 4; Networks stations with history key `somafm:<id>` via `<somafm>/songs/<id>.json` (`"<artist> - <title>"`, `date` epoch seconds); `None` for everything else or on failure.

- [ ] **Step 1: Write failing tests** in `tests/sources.rs` with all four endpoints on one test server: `roots_in_order`; `children_dispatch_by_prefix`; `search_merges_in_directory_order`; `search_dedupes_by_stream_url`; `search_survives_dead_directory` (TuneIn route is Silent: the search returns the other three within 9 s); `filters_from_radio_browser`; `history_somafm_and_iheart` (`None` for a Radio Browser station).
- [ ] **Step 2: Run** `cargo test -p blindradio-core --test sources`. Expected: FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p blindradio-core`. Expected: all pass. Add `#[ignore]` live tests in `tests/live.rs`: `live_search_jazz` (at least 20 results, at least 3 directories represented) and `live_browse_each_root` (every root has children).
- [ ] **Step 5: Run** `cargo test -p blindradio-core --test live -- --ignored`. Expected: pass.
- [ ] **Step 6: Commit** `feat(core): combined search and browse across directories`.
