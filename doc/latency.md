# Latency

Measured 2026-10-27, Windows 11, home broadband (west coast North America), release builds.

## Medians (`cargo run -p blindradio-core --example tune --release`)
- Warm swap (focus to first rendered audio): 7 ms over 21 swaps. Target under 100 ms: met. This is when the mixer renders the first samples; the device buffer (10 ms requested) comes on top before the speakers.
- Cold start: 278 ms over 10 starts, including the 1 s start cushion. Target under 1000 ms, ideally about 300 ms: met.
- Between stations there is no silence: hiss fills the gap from the moment focus moves until the next station sounds.

## Cold start per station (with the cushion)
- Groove Salad MP3 (SomaFM): 289 ms
- Groove Salad AAC (SomaFM): 239 ms
- Drone Zone MP3 (SomaFM): 217 ms
- Radio Paradise AAC: 204 ms
- Fox News Radio HE-AAC (amperwave): 1183 ms (0.4 to 1.3 s across runs; its server dribbles)
- Deutschlandfunk Opus: 1440 ms (server: redirect to a per-session host, two TLS handshakes to Europe)
- Secret Agent PLS (SomaFM, playlist then stream): 339 ms
- BBC Radio 4 HLS (TS segments): 267 ms
- FIP AAC (Radio France): 1267 ms (server waits about 500 ms before its first byte)

## Network stages (curl, same machine, seconds: DNS, connect, TLS done, first byte)
- icecast.radiofrance.fr (FIP): 0.005, 0.148, 0.300, 0.810
- st01.sslstream.dlf.de (DLF): 0.007, 0.166, 0.331, 0.491 to the redirect; the session host adds as much again (first byte 0.95 to 1.05 s)
- live.amperwave.net (Fox News): 0.014, 0.085, 0.234, 0.307 to the redirect; the session host answers at about 0.6 s
- ice1.somafm.com: 0.011, 0.037, 0.079, 0.106
- stream.radioparadise.com (plain http): 0.011, 0.067, none, 0.152

## How fast audio arrives after the first byte
- Burst servers (SomaFM, Radio Paradise, DLF, BBC HLS) reach 1 s of decoded audio 2 to 56 ms after the first frame; FIP about 145 ms.
- Fox News (amperwave) sends about 0.5 s of audio, then lumps up to a second apart for its first 1 to 2 seconds, then about 15 s of audio by 4.5 s. Starting on the first frame ran dry right after starting; the 1 s cushion removed that.

## Playback health (`cargo run -p blindradio-core --example probe --release [warm_secs]`)
All 9 stations at once through the real mixer, 10 ms ticks, 20 s. "Dry" is a tick where the playing ring was empty.
- First version (ring dropped its oldest audio when full, even while playing): SomaFM lost about 13.5 s of audio in 20 s to overflow, so playback kept jumping forward (the choppiness the owner heard). BBC HLS lost 28 s to overflow and starved for 11 s. Fox News had 7 short underruns.
- Warm then swap with the old ring (`probe 5`): DLF Opus came up 750 ms short and BBC HLS 6.8 s short, because both deliver in lumps and the warm ring had thrown the backlog away.
- Now (no ring drops audio, 1 s cushion): 0 dry ticks on all 9, cold, after 0.4 s warm and after 1 s warm.

## Owner checks of the desktop shell (Task 11)
- NVDA reads the Stations list and each station; landing on a station plays it; the window title follows the song.
- Heard and fixed, in order: choppiness (ring overflow), choppiness on fast steps (fades cut short), HLS and Opus buffering after a swap (warm rings dropped audio), Fox News buffering (dribbling server), pauses between stations (now hiss, analogue feel).
- One warm swap back onto Fox News took 2.15 s in a tune run: the reconnect reused the cached per-session URL, whose first byte took 1.4 to 1.7 s. Per-session URLs are no longer cached.

## Notes
- HLS: BBC `as-hls-ww-live.akamaized.net/.../*.isml/*.norewind.m3u8` media playlists use MPEG-TS segments (`.ts`), AAC LC 48 kHz at 128k. The `a.files.bbci.co.uk/.../pc_hd_abr_v2/...` masters point at the same TS kind (HE-AAC, `mp4a.40.5`). No fMP4 BBC HLS was found in Radio Browser.
- SomaFM drops any User-Agent containing "blindradio" (any case) with an empty reply, so the client sends `Blind-Radio/<version>`.
- Debug builds of the desktop shell keep a console and print each warm and cold latency.
