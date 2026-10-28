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

## 181.fm

The [legacy links page](https://www.181.fm/legacy.html) lists every channel as a
numbered mount plus a named `181-<slug>_128k.mp3` endpoint. The named endpoints are
used, which is why the channel slug is also the station slug. 77 channels, MP3 128k.
The page repeats a few channels; each slug appears once here.

## .977

977music.com itself did not answer. The network's streams are served by StreamTheWorld,
which publishes one redirect endpoint per channel:

- `.977` Today's Hits, 80s, 90s, Country, Jamz, Oldies, Smooth Jazz, The Mix, Comedy.

Each is `https://playerservices.streamtheworld.com/api/livestream-redirect/977_<CODE>_SC.mp3`,
MP3 128k. The host is the one Radio Browser also lists the channels under, so it is
the network's own delivery path rather than a borrowed mirror.

## Big R Radio

[bigrradio.com](https://bigrradio.com/) is a script-driven page, but its own
`/assets/app.js` carries the station table: 51 channels, each with a name and an
`ais-edge82-bigr-dal03.cdnstream.com/<id>_128` stream. Names come from that table,
so they match the site. MP3 128k.

## Radio Swiss, KEXP and WFUV

- Radio Swiss Jazz, Classic and Pop: `stream.srg-ssr.ch/m/<rsj|rsc_de|rsp>/mp3_128`.
- KEXP: `kexp-mp3-128.streamguys1.com/kexp128.mp3`.
- WFUV: `wfuv-onair.streamguys.org/onair-hi`.

KCRW's published stream refused the request at check time, so it was not added.

## ooTunes

Not added. `http://ootunes.com/stations` still serves its 38,008-station table, but a
row carries only name, genre and comment: the stream URL and the per-station detail
page are behind a session-gated app, and the search form posts opaque Tapestry state.
There is no public URL to hand to a player, so a directory entry would browse but never
play.

## Live verification

Every one of the 142 added URLs returned HTTP 200 to a GET request, an audio
content type (`audio/aac`, `audio/aacp`, or `audio/mpeg`), and 4096 bytes of stream
data. Requests used `Blind-Radio/0.2` and closed after reading the sample. This
verifies live stream delivery at the time checked; it does not guarantee future uptime
or exercise playback through the desktop app.
