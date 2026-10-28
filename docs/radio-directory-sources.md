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

## Sunshine Live

The network publishes one mp3-192 mount per channel under `stream.sunshine-live.de`:
`live`, `classic`, `hardstyle`, `house`, `techno`, `trance` and `chillout`. Seven
channels, all MP3 192k.

## Radio 538 and SLAM!

Both are Talpa stations delivered through StreamTheWorld:
`playerservices.streamtheworld.com/api/livestream-redirect/RADIO538.mp3` and
`.../SLAM_MP3.mp3`. SLAM! also answers on `stream.slam.nl/slam`; the StreamTheWorld
redirect is used so both Dutch entries look the same.

## Global UK

The UK brands share one ice host, `media-ice.musicradio.com/<Name>MP3`:

- Capital, Capital Dance, Capital Chill, Capital Anthems (the former Capital Xtra).
- Heart London, Heart Dance, Heart 80s, Heart 90s, Heart 70s.
- Smooth London, Smooth Chill, Smooth Country.
- Classic FM, Gold, Radio X London, LBC London, LBC News.

Regional variants (for example Capital Manchester) answer as well but are not added;
the London feed is the one the brand publishes as its main stream. Heart Xmas is
seasonal and was left out so the entry does not go quiet in January.

## Power Hit Radio

- Estonia: `ice.leviracloud.eu/phr128-aac`, AAC 128k, from the station's own player at
  raadiod.tv3.ee (the old powerhitradio.ee domain no longer resolves).
- Lithuania: `stream.rcs.revma.com/f31w7e0fveuvv` (live) and `.../asdpz3qfbhuvv`
  (Power Hit Radio Gold), the two sources the site's player switches between.

Estonia runs one stream only.

## Power Hitz

The player page carries its own stream table: `live.powerhitz.com/<channel>`. The 15
channels are the ones the site lists as distinct, excluding the Christmas channel.
Different station numbers often point at the same `hot108` mount, so only the distinct
channels were kept.

## WHFF

`azuracast.whff.radio/api/stations` lists 20 stations; 18 answer audio. "R&B Soul" and
"Explore All Stations" did not, and the latter is a directory entry rather than a
station. MP3.

## Radio Record

`radiorecord.ru/api/stations/` returns 117 channels with `stream_128`/`stream_320`
mounts on `radiorecord.hostingradio.ru`. 92 answered audio at check time; the rest,
mostly artist shows, did not. AAC.

## Dance Wave!

One stream, `dancewave.online/dance.mp3`, published on its own site.

## PulsRadio

Spelled pulsradio.com (pulseradio.com redirects to an unrelated US station). The
site's own player data lists eight channels on `icecast.pulsradio.com`, each named by
the site:

- PulsRadio Dance (`pulsHD`), Hits (`hitpartyHD`), Club (`mxHD`), Lounge (`relaxHD`),
  Trance (`pulstranceHD`), 80s (`magicradioHD`), 90s (`puls90HD`), 2000 (`puls00HD`).

All eight answer MP3 audio. The mount names do not match the channel names, which is
why they are recorded here rather than guessed.

## Not added

- **101.ru**: the API works (`/api/channel/getListServersChannel/<id>`), but every
  returned URL is a JWT bound to the caller's IP and an expiry, so it cannot be stored
  in a station list.
- **ampradio.ru**: the domain no longer resolves. ampradio.com is a different,
  non-radio site.
- **Power Hit Radio Estonia's old domain**: powerhitradio.ee no longer resolves; the
  station is carried at raadiod.tv3.ee and is added from there.
- **ooTunes**: `http://ootunes.com/stations` still serves its 38,008-station table, but
  a row carries only name, genre and comment: the stream URL and the per-station detail
  page are behind a session-gated app, and the search form posts opaque Tapestry state.
  There is no public URL to hand to a player, so a directory entry would browse but
  never play.

## Live verification

Every added URL returned HTTP 200 to a GET request, an audio content type
(`audio/aac`, `audio/aacp`, or `audio/mpeg`), and 4096 bytes of stream data. Requests
used `Blind-Radio/0.2` and closed after reading the sample. This verifies live stream
delivery at the time checked; it does not guarantee future uptime or exercise playback
through the desktop app.
