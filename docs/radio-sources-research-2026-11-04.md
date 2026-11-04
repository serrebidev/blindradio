# Airable, Online Radio Box, vTuner and song title sources

Researched on 2026-11-04 after a suggestion from Paweł Masarczyk. All numbers come from requests
made that day with `Blind-Radio/0.2`, from this PC. Samples are small and show a rough size, not an
exact rate.

## What was built

The owner chose to use what others use where it works, so 0.2.61 has:

- Online Radio Box as a directory, and as a week of song history (`sources/orb.rs`). Its terms do
  not offer this use. That was the owner's decision. Nobody has asked the company.
- The open song lists below, chosen by stream URL (`sources/played.rs`).

Airable and vTuner were not added, for the reasons below.

## Summary

- **Airly.fm is airable.fm.** Its catalogue is closed behind an NDA, so it cannot be added today.
- **Online Radio Box** is the only source found with days of song history. There is no public API.
- **vTuner** still answers, but offers little. Skip it.
- **Triton, Live365, AzuraCast, laut.fm and others** publish song history openly, and the app can
  find them from a station's stream URL.

## Airable (Airly.fm)

`airly.fm` has no DNS record. `airable.fm` is the portal, and the rest of the description fits
airable: [Receiver gets its catalogue from airable](https://www.airablenow.com/airable-and-receiver-ios-app/),
and [Frontier Smart radios moved to airable](https://airablenow.com/fs-nuvola-shutdown/) when Nuvola
ended in October 2024. Treating the two names as one is an inference; Paweł could confirm it.

- Size: [more than 85,000 stations and 105,000 podcasts](https://bluos.io/2026/02/24/bluos-partners-with-airable-for-radio-and-podcasts/), February 2026.
- Access: [the API needs an NDA, then certification](https://www.airablenow.com/airable/airable-api/). There is no self-serve sign-up, and no price is published.
- Portal: airable.fm asks for a login before showing stations. Its [terms](https://airable.fm/terms-of-use) describe a personal list of favourites, not a data service.
- Radios: a [captured request](https://github.com/Half-Shot/fairable/blob/main/docs/capture.md) shows them sending an `Authorization` token. Without one, `airable.wifiradiofrontier.com` answered 500.
- Other projects: [YCast](https://github.com/milaq/YCast), [YTuner](https://github.com/coffeegreg/YTuner), [Radio-API](https://github.com/KIMB-technologies/Radio-API) and [frontier-airable](https://github.com/seife/frontier-airable) replace the server for a radio. None gives a desktop app the catalogue.

Do not reuse a radio's token. That would pose as a licensed device. The honest route is to email
info@airablenow.com, but an NDA may not suit a public repository, which is an open question for airable.

Song history: none found.

## Online Radio Box

### What it offers

- Current title: `https://scraper.onlineradiobox.com/<country>.<alias>` needs no key and returns JSON with `title`, `iArtist`, `iName` and `updated`. `updated` of 0 means it has nothing. `scraper2` answers the same.
- History: `https://onlineradiobox.com/us/977todayshits/playlist/` is today, and `/playlist/N` is N days ago. It is HTML: `td.tablelist-schedule__time` and `td.track_history_item`.
- Directory: `/search?q=`, `/genre/<name>/` and `/<country>/` list stations in `.station_play` buttons with `stream`, `streamType`, `radioId` and `radioName`. Search and genre pages gave about 21 rows, a country page 46. Paging uses `?p=`.

### Retention

The page promises 7 days and links six older days. The app reads today and seven days back. More was served:

- .977 Today's Hits: days 1 to 8 had 398 to 404 rows each, day 9 had 115, day 10 had none.
- Chilltrax: 456 rows on day 5, 453 on day 8.
- KEXP: 385 rows on day 6. BBC Radio 1: 397 rows on day 6.

[playlist_scraper](https://github.com/ThereOHM/playlist_scraper) warns that most stations freeze
after about 5 days. These four did not.

### Times

Rows hold `HH:MM` with no date. The same page showed 00:50 from this PC and 02:50 through a New York
scraper, so times follow the viewer's location. `?useStationLocation=1` switches them to station time
and the page then prints that clock, as in `now in London 08:12`. Comparing that with UTC gives the
offset. `?cs=<alias>` changed nothing. Some stations, such as KEXP, show no clock and stay in the
viewer's time, so the newest row's time gives the offset, rounded up to 15 minutes.

### Coverage

- 30 random stations from `networks.json`: 4 matched by exact stream URL and 7 by name only. Several name matches were the wrong station, such as Big R's The Wave against `de.thewave`.
- 30 popular Radio Browser stations: 7 matched by URL, 11 more by name and country, and 9 had a title.
- Those titles are not all songs: `ADWTAG 60000` is an ad tag, and `Deutschlandfunk Kultur` is the station name.
- First 12 rows of a listing with a title: US 10, Canada 5, jazz 9.
- Talk stations have nothing: `us.foxnewstalk` and `ca.985fm` report `updated` 0.

A wrong match would show another station's songs. Match by exact URL, or by name and country, and
never by name alone.

### Permission

- There is no public API. The [IINA plugin's README](https://github.com/pinedigga/iina-plugin-onlineradiobox) says the same.
- [The terms](https://onlineradiobox.com/terms) license "personal, non-commercial, entertainment purposes". They also say "It is prohibited to modify, copy, reproduce, republish" the content, and that audio plays "solely by means of the audio player embedded in or facilitated by the Service".
- [robots.txt](https://onlineradiobox.com/robots.txt) disallows `/json/*` and `/*/*/player/*` for everyone. Dated playlist pages are disallowed for Googlebot only.
- Others do use it: [ampcast](https://github.com/rekkyrosso/ampcast/blob/main/src/services/internetRadio/onlineRadioBox/onlineRadioBoxApi.ts) (392 stars, pushed 2026-10-07) calls `scraper2` and `/json`. That shows it works, not that it is allowed.

Playing streams through it conflicts most directly with the terms. Some also carry its own tag, such
as `?dist=onlineradiobox`. Pages are fetched only when the user opens a folder, searches or presses
Ctrl+Y. If the company objects, removing `sources/orb.rs` ends the use.

## vTuner

- Status: `vtuner.com` still serves pages. [TEAC ended its vTuner service](https://teac.jp/int/support/news/7814) in March 2025 after an outage from January. [One account](https://victorantos.com/posts/i-bought-a-denon-for-my-wedding-then-i-vibe-coded-its-radio-back/) says the directory went paid around 2020 and was largely gone for older receivers by 2023.
- Device API: `denon.vtuner.com/.../loginxml.asp` answers `Login Error`.
- Web directory: `BrowseStations/BrowsePremiumStations.asp?sCategory=Jazz&sBrowseType=Format&iCurrPage=N` is HTML with 20 stations a page. `func/dynampls.asp?id=<id>` redirects to the stream URL with no key. [junguler's script](https://github.com/junguler/m3u-radio-music-playlists/blob/main/stuff/vtuner.sh) already harvests it.
- Health: of 40 sampled stations, 14 sent audio, 18 did not, and 8 gave no URL. Some failures may be ports blocked from here.
- Terms: [personal use, with no right to reproduce](https://www.vtuner.com/terms-of-use.html).
- Song titles: none found.

Many of the working links are laut.fm, Live365 and similar hosts the app can already reach another
way. vTuner replacements such as YCast and YTuner use Radio Browser for the same reason.

## Song history that is open

Each source below answered with no key, and all are found from a stream URL, so they also work for
directory stations. Radio Paradise's KFAT and Sanctuary answered under no channel number tried.

- **Triton (StreamTheWorld):** `https://np.tritondigital.com/public/nowplaying?mountName=977_HITS&numberToFetch=50&eventType=track`. It returns XML with `cue_title` and `cue_time_start`. 65 of the app's 94 StreamTheWorld stations gave titles once `_SC` and `AAC` were removed from the mount name. It returned at most 50 rows, 2.9 hours on .977. NPR, Vision Australia and NZME mounts mostly gave nothing. No official documentation was found. [ProgRadio](https://github.com/conradfr/ProgRadio/blob/master/progradio_api/lib/progradio_api/song_provider/streamtheworld.ex) uses it.
- **Live365:** `https://api.live365.com/station/<mount>` has `current-track` and `last-played`, with start times. The app already reads this host's stations.
- **AzuraCast:** `https://<host>/api/nowplaying/<shortcode>` has `song_history`, 5 rows on WHFF. The shortcode comes from `/listen/<shortcode>/`. 19 stations in `networks.json` use `/listen/`. [Documented API](https://docs.azuracast.com/en/developers/apis).
- **laut.fm:** `https://api.laut.fm/station/<name>/last_songs` gives about 10 rows with start times. The directory was replaced by IPRD on purpose, but `*.stream.laut.fm` URLs still arrive through saved stations and other directories. `/station_names` lists 16,907 stations.
- **BBC:** `https://rms.api.bbc.co.uk/v2/services/<service>/segments/latest?limit=10`. It answers 400 from `limit=20` up. The service id is in the stream URL. 15 of 64 services returned songs at 07:57 UTC, so expect fewer on speech-led services and at some hours. Times come only as phrases such as `4 Minutes Ago`.
- **KEXP:** `https://api.kexp.org/v2/plays/?limit=N&ordering=-airdate`. Without the order it can answer a stale list.
- **Radio Paradise:** `https://api.radioparadise.com/api/nowplaying_list_v2022?chan=0&list_num=N`.

TuneIn's `/profiles/<id>/nowPlaying` gave only the station name for BBC Radio 1, so it does not help.
The radio.net check used a guessed id and proves nothing. fmstream.org answered but was not evaluated.

## Also relevant

[airable reports](https://www.airablenow.com/ard-restricting-access/) that from 2026-10-01 ARD live
streams are no longer available outside Germany through third-party services. Stations such as 1LIVE
in any directory may fail abroad. This was not tested in the app.

## Not done

- Titles from these sources do not feed the status line or recordings for a station with no title
  in its stream. Only Ctrl+Y uses them. Doing it means polling, and a title that must line up with
  the playhead (`Tuner::heard_title`).
- Airable: email info@airablenow.com if its catalogue is wanted enough to accept an NDA.

## How the search was done

For each name, GitHub repositories were searched by stars and by recently updated, up to 25 results
each. Web search took 25 results per name, newest first.

- Online Radio Box has only 10 matching repositories, so GitHub code search filled the gap. Its web results were mostly social posts.
- `airly.fm` and `airlyfm` have no repositories. `airable` matches unrelated "air" projects, so the useful hits came from code search.
- `gh search` repeated the same query terms loosely for the broader title queries, and most of those results were noise.
