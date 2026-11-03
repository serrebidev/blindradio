# BlindRadio

A vibe-coded, screen-reader-first internet radio player for Windows, macOS, and Linux, built in Rust. Moving through a station list tunes like an analogue dial: the station under focus plays at once, its neighbours stay connected so a swap is near instant, and soft hiss fills the moment between stations.

[![Join SerrebiProjects on Telegram](https://img.shields.io/badge/Telegram-SerrebiProjects-2CA5E0?style=for-the-badge&logo=telegram&logoColor=white)](https://t.me/SerrebiProjects)
[![License: MIT](https://img.shields.io/badge/License-MIT-green?style=for-the-badge)](LICENSE)

**Questions, bugs, or release news?** Join the [SerrebiProjects Telegram group](https://t.me/SerrebiProjects), the fastest place to get help.

## Features

- Every station list tunes as you move through it: the main dial, Search, Browse, Favorites, Recently Played and Collections all play the station under focus.
- Browse is grouped into Directories and Networks: Radio Browser, TuneIn, iHeartRadio, Icecast, SHOUTcast, Internet-Radio, IPRD and Recommended Radio Streams, plus English-language networks of public broadcasters and channel groups (BBC, NPR, Radio Paradise, NTS, 181.fm and many more).
- A Sort choice on every directory list, in any order the directory offers (name, votes, clicks, bitrate, codec, country and more), with Reverse, and search carries the same order.
- Letter jump, Enter, Delete and the Applications key work in every list; folders load lazily and Load More pages in more stations.
- Favorites, Recently Played, custom stations, and Collections you can create, rename, reorder and fill one station or a whole list at a time.
- Playlist import and M3U export, Copy Stream URL, and a SerrebiRadio starter list that downloads on first run.
- Time-shift: pause holds the stream instead of disconnecting, skip back or forward in steps (15 seconds by default) or jump back to live, with a 30 minute buffer settable globally and per station.
- Segment bookmarks: each title a station sends marks the buffer, so you can jump between songs and program segments and write the current, previous, next or all of them to their own files.
- Recording on Ctrl+R: the stream's own format untouched, or WAV, MP3 (constant or variable bitrate) or FLAC decoded.
- Song recognition for stations that send no titles, through AudD (not Shazam; needs a free token from audd.io, set in Settings); the recognized song shows in the status line and can be announced as it changes.
- Scrobbling to ListenBrainz and to last.fm or Libre.fm for songs that play a minute or more.
- Song History, published and for the session, and titles from ICY metadata and HLS ID3 or EXTINF tags.
- Streams that deliver slower than their bitrate refill a deeper cushion after running dry (three seconds, then nine, up to fifteen) instead of stuttering every few seconds; a clean minute earns the normal cushion back.
- Sleep Timer, an equalizer, and an Audio Device menu that follows devices as they are lost and return.
- Speech through a live region, so every message is heard; connection status and song titles each have their own setting.
- Thirteen global hotkeys with a shared shortcuts editor.
- A status line that shows the station, how long it has played, how far behind live you are while time-shifted, and when a recording is running.
- Tray icon with Play/Pause, Stop, favorites, Show and Exit; a second launch raises the first copy and exits.
- Plays MP3, MP2, AAC and HE-AAC (LATM too), AC-3 and E-AC-3, Ogg Vorbis, Opus and FLAC, MPEG-TS channels, PLS and M3U playlists, and HLS live streams, including fragmented MP4 and AES-128.
- Daily update check and Help, Check for Updates; every download is verified with minisign before it installs.

## Help

The end-user guide lives inside the application: press **F1** anywhere and it opens at the front. It works offline and is fully keyboard-accessible. Its source is [`doc/readme.md`](doc/readme.md).

## Download and install

Grab the latest build from the [Releases page](https://github.com/serrebidev/BlindRadio/releases).
For a version-by-version history, see the [changelog](CHANGELOG.md).

**Windows**

Download `BlindRadio.zip` (portable: extract anywhere and run `blindradio.exe`) or `BlindRadio_setup.exe` (per-user installer). Windows builds have no Authenticode signature, so SmartScreen may warn the first time.

**macOS**

Download `BlindRadio.dmg`, open it and run BlindRadio.

**Linux**

Download `BlindRadio.AppImage` (run it as-is) or `BlindRadio.tar.gz`.

Every asset has a `.minisig` beside it, and the in-app updater verifies that signature before installing an update; portable and installed copies each update the right way.

## Privacy policy

This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it.

BlindRadio has no telemetry, analytics or accounts of its own. It connects to the stations you play and the directories you browse, each under its own policy, and checks this repository's GitHub releases for app updates. Scrobbling sends the songs you play to ListenBrainz or last.fm/Libre.fm only when you turn it on and sign in; song recognition sends a short sample to AudD only when you turn it on for a station or globally.

## Building

See `AGENTS.md` for the Windows prerequisites (MSVC Build Tools 2022, CMake, Ninja, LLVM for libclang). Then:

```sh
cargo build --release
cargo test --workspace
```

`cargo run -p blindradio-core --example tune --release` measures warm and cold tuning latency; `doc/latency.md` has the latest numbers.

Releases run on GitHub Actions: `gh workflow run release.yml -f version=<version>`. It runs the tests and clippy, bumps the version, builds Windows, macOS and Linux, and takes the release notes from that version's section of `CHANGELOG.md`, which must already be on `main`.

## Contributing

Pull requests are welcome. If BlindRadio has been useful to you, open a PR with a fix or feature and I'll review it.

## License

BlindRadio is under the [MIT license](LICENSE) — use it, change it, redistribute it, or package it for a distro repository, no permission needed.

AAC decoding uses the Fraunhofer FDK AAC library, which has its own licence.

## Community and support

Report bugs and request features in [Issues](https://github.com/serrebidev/BlindRadio/issues). For questions, feedback, and release news, join the [SerrebiProjects Telegram group](https://t.me/SerrebiProjects).
