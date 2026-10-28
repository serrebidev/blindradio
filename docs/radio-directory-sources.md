# Curated radio directory sources

Verified on 2026-10-28. Existing network entries retain their order, identifiers,
and stream URLs. New networks use one direct stream per channel.

## Radio Paradise

The official [stream link picker](https://radioparadise.com/listen/stream-links)
publishes eight channels: Main Mix, Mellow Mix, Rock Mix, Global Mix, Beyond...,
Serenity, Radio 2050, and KFAT. Its
[page data](https://vsh-sdata.radioparadise.com/api/pages?filters[url][$eq]=/listen/stream-links&populate=deep)
contains the exact endpoint names and codec options in `channelData`.

Selected the published default AAC streams: `aac-128`, `mellow-128`, `rock-128`,
`global-128`, `beyond-128`, `serenity`, `radio2050-128`, and `kfat-128` under
`https://stream.radioparadise.com/`. Serenity is AAC+ 64k; the others are AAC 128k.
Sanctuary appears elsewhere on the site but has no endpoint in the picker data,
so it is omitted.

## NTS

NTS's official [TuneIn support article](https://ntslive.freshdesk.com/support/solutions/articles/77000587257-tunein)
publishes the NTS 1 and NTS 2 direct URLs under
`https://stream-relay-geo.ntslive.net/`: `stream?client=direct` and
`stream2?client=direct`. Both currently report MP3 256k.
The article offers Infinite Mixtape URLs only through support; those are omitted.

## WFMU

The official [stream help page](https://wfmu.org/ssaudionet.shtml/index.shtml)
publishes the main AAC 128k endpoint,
`https://stream0.wfmu.org/freeform-high.aac`, and links these channel playlists:

- [Rock'n'Soul Radio](https://wfmu.org/wfmu_rock.pls):
  `https://stream2.wfmu.org/rocknsoul`.
- [Give the Drummer Radio](https://wfmu.org/wfmu_drummer.pls):
  `http://stream0.wfmu.org/drummer`.
- [Sheena's Jungle Room](https://wfmu.org/wfmu_sheena.pls):
  `http://stream0.wfmu.org/sheena`.

The last two endpoints also serve audio over HTTPS, which the directory uses.
All three side channels currently report MP3 128k. The
[mobile information page](https://ftp.wfmu.org/mobile.shtml) also publishes the
main MP3 alternative.

## Live verification

Every one of the 14 added URLs returned HTTP 200 to a GET request, an audio
content type (`audio/aac`, `audio/aacp`, or `audio/mpeg`), its expected `icy-br`
bitrate, and 4096 bytes of stream data. Requests used `Blind-Radio/0.2` and closed
after reading the sample. This verifies live stream delivery at the time checked;
it does not guarantee future uptime or exercise playback through the desktop app.
