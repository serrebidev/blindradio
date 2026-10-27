# Latency

Measured 2026-10-27, Windows 11, home broadband (west coast North America), release build, `cargo run -p blindradio-core --example tune --release`.

## Medians
- Warm swap (focus to first rendered audio): 5 ms over 19 swaps. Target under 100 ms: met. This is when the mixer renders the first samples; the device buffer (10 ms requested) comes on top before the speakers.
- Cold start: 269 ms over 10 starts. Target under 1000 ms, ideally about 300 ms: met.

## Cold start per station
- Groove Salad MP3 (SomaFM): 269 ms
- Groove Salad AAC (SomaFM): 165 ms
- Drone Zone MP3 (SomaFM): 210 ms
- Radio Paradise AAC: 150 ms
- Fox News Radio HE-AAC (amperwave): 685 ms
- Deutschlandfunk Opus: 1235 ms
- Secret Agent PLS (SomaFM, playlist then stream): 250 ms
- BBC Radio 4 HLS (TS segments): 400 ms
- FIP AAC (Radio France): 1346 ms

## Network stages (curl, same machine, seconds: DNS, connect, TLS done, first byte)
- icecast.radiofrance.fr (FIP): 0.005, 0.148, 0.300, 0.810
- st01.sslstream.dlf.de (DLF): 0.007, 0.166, 0.331, 0.491
- live.amperwave.net (Fox News): 0.014, 0.085, 0.234, 0.307
- ice1.somafm.com: 0.011, 0.037, 0.079, 0.106
- stream.radioparadise.com (plain http): 0.011, 0.067, none, 0.152

## Playback health (`cargo run -p blindradio-core --example probe --release`)
Each station drained in 10 ms ticks for 20 s, all 9 at once.
- Before (ring dropped its oldest audio when full, even while playing): SomaFM lost about 13.5 s of audio in 20 s to overflow (its connect burst is far faster than real time), so playback kept jumping forward. BBC HLS lost 28 s to overflow (every 6.4 s segment arrives at once) and then starved for 11 s. Fox News had 7 short underruns.
- After (a ring being played makes the decoder wait for room; only warm neighbours drop their oldest audio): 0 ms overflow and 0 underruns on all 9 stations. The playing ring now sits near its full 3 s on bursty servers. First audio times are unchanged.
- Mixer: each fading station keeps its own 25 ms fade, so fast steps never cut a fade short; audio starting from silence fades in; a ring that runs dry eases out over 5 ms and resumes only once 100 ms is buffered.

## Notes
- FIP and DLF miss 1 s on their own because of distance and server delay: about 150 ms per round trip to Europe, and FIP's server waits about 500 ms before its first byte. After the first byte, FIP takes about 450 ms more and DLF about 700 ms more to reach first render (probe, decode, ring). That part is not profiled per stage yet. Profile it first if European stations need to start faster.
- Fast steps (100 ms apart) land on neighbours that opened only one step earlier. They count as warm (already open), but they wait for that connection. Example: FIP at 755 ms. Most of them report no latency, because the next focus comes before first audio.
- HLS: BBC `as-hls-ww-live.akamaized.net/.../*.isml/*.norewind.m3u8` media playlists use MPEG-TS segments (`.ts`), AAC LC 48 kHz at 128k. The `a.files.bbci.co.uk/.../pc_hd_abr_v2/...` masters point at the same TS kind (HE-AAC, `mp4a.40.5`). No fMP4 BBC HLS was found in Radio Browser.
- SomaFM drops any User-Agent containing "blindradio" (any case) with an empty reply, so the client sends `Blind-Radio/<version>`.
