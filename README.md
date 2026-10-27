# BlindRadio

A screen-reader-first internet radio player for Windows, written in Rust. Moving through a station list tunes like an analogue dial: the station under focus plays at once, its neighbours stay connected so a swap is near instant, and soft hiss fills the moment between stations.

## Status

Early. This release is plan 1 of 6: the audio engine and a bare test window with nine sample stations. Station search, favourites, settings, speech and the full interface come in later plans.

## Use the test window

1. Download the zip from the latest release and unzip it anywhere.
2. Run `blindradio.exe`. The Stations list has focus and the first station plays.
3. Arrow up and down to tune. The window title shows the song when the station sends one.
4. Ctrl+S stops. Escape closes.

## Formats

MP3, AAC and HE-AAC, Ogg Vorbis, Opus, FLAC, PLS and M3U playlists, and HLS live streams with MPEG-TS segments. Song titles come from ICY metadata and HLS ID3 or EXTINF tags.

## Build

See `AGENTS.md` for prerequisites (MSVC Build Tools 2022, CMake, Ninja, LLVM for libclang). Then:

```
cargo build --release
cargo test --workspace
```

`cargo run -p blindradio-core --example tune --release` measures warm and cold tuning latency; `doc/latency.md` has the latest numbers.

## Licence

MIT. AAC decoding uses the Fraunhofer FDK AAC library, which has its own licence.
