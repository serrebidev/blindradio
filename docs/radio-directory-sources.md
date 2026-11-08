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
Sanctuary is not in the picker data, but `stream.radioparadise.com/status.xsl` lists `/sanctuary`
(AAC 64k), so it is carried.

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

## Planet Radio

planetradio.de is the FFH group. Its page carries the channel table itself:
`data-streamurl="http://mp3.ffh.de/ffhchannels/hq<channel>.mp3"` with the label in the
same element. 33 channels answered over HTTPS (the site publishes HTTP; the host serves
both). Labels are kept as the site writes them, e.g. FFH RELAX, FFH DIE 80ER.
The Christmas channels are included because they are live year-round streams there.

## Nightride FM

Seven synthwave channels on `stream.nightride.fm`: nightride, chillsynth, darksynth,
datawave, spacesynth, horrorsynth and ebsm. The page advertises `.m4a`; only the `.mp3`
mounts answer, so those are used.

## FRISKY, XLtrax and Radio FG

- FRISKY: `stream.friskyradio.com/frisky_mp3_hi`. The newer frisky.fm player offers no
  plain stream URL.
- XLtrax: `centova.radioservers.biz/proxy/xltrax/stream`, one station.
- Radio FG: `radiofg.impek.com/fg`. The site's other branded channels (Chic, Deep,
  Underground) return an HTML page rather than audio from that host, so only the main
  FG stream is added.

## 2026-11-03 audit

Each stream below answered audio, and where an ICY stream reports a name it matched.
Candidates came from official lists, GitHub, and the app's own search.

- BBC: the [station list API](https://rms.api.bbc.co.uk/v2/experience/inline/stations)
  names 71 services and 64 are carried. The other seven are 5 Sports Extra 1 to 3, 6 Indie
  Forever, Live News, Radio Wales Extra and CBeebies Radio. Where a URL could be found, its
  audio answers 403 from outside the UK, so they are omitted.
- MPR: `<name>.stream.publicradio.org/<name>.mp3`, as listed on the
  [YourClassical streams page](https://www.yourclassical.org/listen/streams). Seasonal
  streams are omitted.
- NTS: Infinite Mixtapes from `https://www.nts.live/api/v2/mixtapes`, the
  `audio_stream_endpoint` of each.
- Global UK: more mounts on `media-ice.musicradio.com`. The `LBCUKMP3` mount reports
  itself as LBC News UK, so it is not added as a separate station.
- ABC: `streaming.abc-cdn.net.au/audio/hls/local<name>.m3u8`, by region. Each playlist was seen
  advancing and serving audio. `sport.m3u8` answers but its audio is 403 from here, so it is
  omitted.
- WHFF: nine channels answered 503 at one check and two at the next, so the host is
  overloaded rather than the channels gone. All stay. Its HLS playlists answer but do not
  advance, so they are no fallback.
- SomaFM's `channels.json` matches the list exactly (51).
- 1.FM: `api.1cloud.fm/api/channels`, keyless, gives every channel's name and
  `broadcast_mp3_link`. Names follow it; a row's slug stays, so saved stations keep working.
  A List 80s has left that list but still answers, so it stays.
- 181.fm: the legacy page lists 77. The player bundle at `www.181.fm/static/js/main.*.js`
  holds the same plus six Christmas channels (`station` and `mount` pairs).
- New English networks came through the app's search: K-LOVE and Air1 (`maestro.emfcdn.com`),
  Pacifica, Triple M, ARN Australia, NZME and Rova in New Zealand, SABC, American Family Radio,
  Moody Radio, DKFM, and single services such as Bloomberg, C-SPAN, ESPN and NHK World.
  NZME's StreamTheWorld mounts `NZME_01` to `NZME_74` name themselves in ICY.
  A server's `status.xsl` lists every mount, and a web search for
  `"icecast streaming media server"` plus a name finds such pages. That is how Sanctuary, KPFT,
  UbuWeb Radio, Mushroom Pot and the public radio side channels were found.
  HLS checks must re-read one media playlist: some servers start a session per request,
  which makes a fresh fetch look frozen.

## Live365

`api.live365.com/genres` and `/station/<mount>` are open, but its station list and search
answer "Unauthorized access". Its web pages are not closed: `live365.com/listen/<genre>-music-radio`
and `/listen/search?query=` carry the whole list as React Router page data in `enqueue("...")`
script calls, with `name`, `mountId` and `genres`. A station streams from
`streaming.live365.com/<mount>`. A sample of 40 jazz stations had 39 playing. The slug rule comes
from `junguler/m3u-radio-music-playlists`, `stuff/live_365.sh`. Three of 264 genre names lead
to no list: Drum 'n' Bass, Music To ... To and Old Time Radio.

Directories gained browse paths that were already public: TuneIn's `c=music`, `talk`,
`sports` and `local`, with its `nextStations` links; radio.net's `/stations/tags` and
`/stations/by-tag`; iHeart's `/api/v2/content/markets` and `marketId`.

## Not added

- **Bauer (Absolute, KISS, Magic, Hits Radio, Greatest Hits, Planet Rock, Jazz FM)**:
  [withdrawn from third-party apps](https://community.roonlabs.com/t/bauer-media-radio-stations-will-no-longer-be-available-on-third-party-sites-including-roon/187490),
  so they are not curated here. Directories may still list them.
- **KDFC channels**: only node-specific StreamTheWorld hosts are listed, and the generic
  redirect answers 404.
- **TuneIn By Language**: 89 of its 91 languages hold no stations, French and German
  among them, so the folder is left out.
- **Bauer Ireland (Today FM, Newstalk, SPIN and others)**: left out with the rest of Bauer.
- **Radio Garden**: Cloudflare refuses non-browser requests.
- **Zeno**: `zeno.fm/api/stations` lists stations with no stream URL and ignores its
  country filter, so it would browse without playing.

- **101.ru**: the API works (`/api/channel/getListServersChannel/<id>`), but every
  returned URL is a JWT bound to the caller's IP and an expiry, so it cannot be stored
  in a station list.
- **ampradio.ru**: the domain no longer resolves. ampradio.com is a different,
  non-radio site.
- **Power Hit Radio Estonia's old domain**: powerhitradio.ee no longer resolves; the
  station is carried at raadiod.tv3.ee and is added from there.
- **AMG Radio (amgradio.ru)**: the holding page lists about twenty stations (Remix FM,
  Hype FM, Deep FM, Chilla, Retro, Classic and others), but each one's player lives at
  ru.volna.top and exposes no stream URL in its HTML or WordPress API. Leaving it out
  rather than shipping entries that cannot be played.
- **m2o (Italy)**: `m2o-live.mediaserver.it/m2o.mp3` answers with an audio content type
  but two bytes of body, which is the server refusing the request rather than audio. No
  working public URL found from here.
- **1Club.FM**: the domain no longer resolves.
- **DI Radio Digital Impulse**: no working stream, though it once had 45 or more free channels.
  `di-radio.com` does not connect. All 137 of its channel entries on `stream.rcast.net` answer
  500. Of 158 older addresses in `junguler/m3u-radio-music-playlists`, 7 answer, and each now
  reports another station's name (KRFF, Yammat FM and others), so those ports were reassigned.
  The `stream.pcradio.ru/di_radio_*` relays fail too.
- **DI.fm and Sky.fm/RadioTunes**: no free stream without an account. Their own
  `listen.di.fm/public3` and `listen.radiotunes.com/public3` lists still answer, but every
  `pub<n>` host they name no longer resolves. Only `prem<n>` hosts answer, with 401 unless a
  subscriber's listen key is given, and the web player's `routines` call answers 403. All 612 mounts on
  `prem1.di.fm/status.xsl` are marked Login and titled Premium. Sky.fm
  became RadioTunes in 2012. A subscriber could add a `prem<n>` address with their own key as
  a custom station.
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

## 2026-11-07 networks audit

Every network was compared against its owner's own published list.

- Exact matches, nothing missing: SomaFM (`channels.json`, 51), 1.FM
  (`api.1cloud.fm/api/channels`, 72 live channels; our 73rd, A List 80s, has left the
  list but still answers, so it stays), 181.fm (the player bundle's station/mount table,
  83 — the six bundle Christmas channels beyond the legacy page are the ones already
  carried), NTS (the mixtapes API's 16 plus the two live channels), WHFF (the AzuraCast
  API lists 20; "Explore All Stations" is a directory entry, not a station, and stays out).
- BBC: the station list API names 71 services; the seven not carried are the ones the
  2026-11-03 audit found geo-blocked (5 Sports Extra 1–3, 6 Indie Forever, Live News,
  Radio Wales Extra, CBeebies Radio). Unchanged.
- WNYC: Q2 Music added (`stream.wqxr.org/q2`, ICY name "New Sounds"). The Jonathan
  Channel has no working direct stream any more: WNYC's own .pls links now 301 to a
  404 on the rebuilt wnyc.org, and the old StreamGuys/js-stream and tjc.wnyc.org
  addresses are dead, so it is not carried.
- Minnesota Public Radio: Celebrate the Maestro added from the YourClassical streams
  page. Hispanic Heritage on the same page is seasonal and stays out, like Holiday.
- Radio Record: NEW network. `radiorecord.ru/api/stations/` returns 117 channels with
  `stream_128` mounts; 92 answered audio at check time and are carried, the rest are
  mostly artist shows that did not answer — the same rule as the 2026-10-28 audit.
- Global UK (media-ice.musicradio.com), Big R Radio, 1.FM's host, Power Hitz, Nightride
  FM, FRISKY, Radio Paradise, RTÉ and SomaFM's ice host expose no public mount listing
  (no status.xsl), so those were verified against the owners' published channel lists
  and player tables instead, as before.
- blindradio.com (the accessibility directory, not this app): audited as a possible
  directory source. Its genre and country listing pages are behind a JavaScript bot
  check — plain requests get a 127-byte stub and archive.org captures show the same
  "please wait while your request is being verified" wall — so no listing can be read
  without running a browser, and it is not added. Structure, for the record: genres at
  `/radio-stations-online/<slug>-<id>-0/`, countries at
  `/online-live-internet-radio-stations/<slug>-<cc>-0/` under eight region pages, and
  search by POSTing `textfield` to `/web-radio-live/`. If the wall ever comes down it
  can be scraped like Live365.
