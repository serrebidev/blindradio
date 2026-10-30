# Changelog

Readable release history for BlindRadio, newest first. When adjacent releases
were part of the same fix stream, they are combined with a version range such
as `0.2.2-0.2.3`. Each release's section below is its GitHub release note,
taken from this file by `cargo xtask release`.

## 0.2.13

- Export playlists from the main list: the context menu on a station, a collection (Custom Stations included), a category, a network, or the Favorites header now has Export…, which writes an M3U playlist file you can open anywhere. The Collections and Favorites windows offer the same Export in their menus.
- Pause and resume recordings: Ctrl+Shift+R (Playback menu, Pause Recording) pauses a recording and later resumes it into the same file, leaving the paused span out — skip an ad break inside one recording. Changing stations or pressing Stop now saves the recording properly instead of leaving it unfinished.
- Recording now starts where you are in the buffer in the standard time-shift mode too: if you are listening ten minutes behind live, the recording starts at your position, not at the live edge. It already worked this way in the encoded-ring mode.
- The first press of a seek key no longer announces the position from before the seek: skipping back from live with F6 no longer says "Live" on the first press, and F5 reports the buffer depth it jumped to.
- Opening the Favorites window no longer pops the wxWidgets debug dialog about expanding a hidden root node.

## 0.2.12

- Copied stream URLs now survive closing the app. The clipboard was
  never flushed, so the copied text was owned by the running process
  and vanished the moment BlindRadio closed completely.
- Pasted URLs are cleaned before they are checked: a BOM or zero-width
  characters copied along with the address no longer make a good URL
  announce "Not a stream URL" in Open URL, Add Custom Station or Edit
  Station.
- Streams whose HTTPS connection never completes now fall back to the
  same address over plain HTTP. Some station hosts (AZPM's among them)
  listen on port 443 but their TLS handshake never finishes, while the
  stream itself answers fine on HTTP; players like DVBViewer and VLC
  have always tolerated this.

## 0.2.11

- Copy, Cut and Paste for stations, in the context menu and on
  Ctrl+C, Ctrl+X and Ctrl+V. Copy copies any station — including one
  found in Browse or a directory listing — and Paste puts it into a
  collection, a folder, or Favorites. Cut takes the station out of
  its home when it is pasted, so cutting and pasting into another
  folder or collection moves it there.

## 0.2.10

- Fixed portable self-update never applying: the downloaded update was
  verified and extracted, but the zip (and the Linux tarball) wrapped
  everything in a `BlindRadio` folder, so extraction landed beside the
  running app instead of over it and the old version relaunched. The
  archives are flat now, so updating from any older portable install to
  this release — and every release after — applies for real.
- Skip forward now reaches live: with the encoded ring buffer, a skip
  that lands at or past the live edge goes live instead of stopping
  short at the replay window's edge, and a skip past the window
  re-primes at its target rather than clamping.
- New time-shift jumps on a function-key row: F5 jumps to the start of
  the buffer, F6 skips back, F7 skips forward, F8 jumps to live.
  Ctrl+Shift+Left and Ctrl+Shift+Right do the two jumps too; the Ctrl
  arrows and Ctrl+J are unchanged.
- Settings gains a Startup List: the main list can open on the default
  stations, on every playlist merged (All Playlists), or on any one
  collection — an imported playlist is a collection, so your own
  playlists can be what the app starts with instead of the defaults.
- A station you hold (in a playlist, your favorites, or Custom Stations)
  can now be edited: Edit Station on its context menu changes its name
  and stream URL everywhere it appears. Station Settings never did
  this; it holds per-station playback options only.
- Networks and the station directories are listed in alphabetical
  order, and so are the stations inside each network.
- Slow-starting servers connect more reliably: the connection timeout
  rose from 4 to 10 seconds, after stations on slow-handshaking
  servers (AZPM's, for one) failed here while playing in other apps.
- Menus reorganized: Settings moved from Playback to the File menu,
  playlist imports moved from Stations to File, and the Audio Device
  submenu moved from File to Playback.
- File gains Open URL: paste a stream address and it plays at once,
  for a quick listen or a test — nothing is added to any list (it
  still shows under Recently Played).
- The status line is now in the window's real status bar — station,
  song, time played, Live or how far behind live you are, and the
  stream format (for example `MP3, 128 kbps, 44.1 kHz, stereo`) — and
  the window title reads `BlindRadio - <station> - <song>`, so Alt+Tab
  and the title command name the app first.

## 0.2.9

- Five networks join the catalog: DFM (Russia, 45 stations) and Hit FM
  (Russia, 4 stations), Radioparty.pl (6 stations), RauteMusik (Germany,
  48 stations — its Techno channel is the former Techno4Ever), and
  TechnoBase.FM with its sister stations TranceBase.FM, HardBase.FM and
  HouseTime.FM. Every stream was verified live before inclusion.

## 0.2.8

- Optional encoded ring buffer for time-shifting, the DVBViewer model
  (Settings, off by default): the stream's own compressed bytes are kept
  up to a size budget (200 MB by default, about 3.5 hours at 128 kbps),
  the oldest dropped first, instead of decoded audio by the minute.
  Pause, resume and seeking work from the ring, and recording can start
  anywhere inside it. Only self-synchronizing streams (MP3 and ADTS
  AAC) can replay from the middle; Ogg, FLAC and MP4 stations keep
  the decoded buffer. With the ring off, the standard decoded minutes
  buffer is unchanged.

## 0.2.5

- Browse is grouped: the top level is Directories (Radio Browser, TuneIn, iHeartRadio,
  Icecast, SHOUTcast, Internet-Radio, laut.fm) and Networks, not eight loose entries.
- A network with a single station is no longer listed under Networks.

- Time-shift playback: pause now holds the stream and resumes where you paused, and
  you can skip back or forward in steps (15 seconds by default) or jump back to live.
  The buffer is 30 minutes by default, settable globally and per station.
- Recording: Ctrl+R records the playing station — the stream's own format untouched,
  or WAV, MP3 (constant or variable bitrate) or FLAC decoded.
- Song recognition: stations that send no titles can be identified with AudD; the
  recognized song shows in the status line and can be announced as it changes.
- Scrobbling: songs that play for a minute or more can be scrobbled to ListenBrainz
  and to last.fm or Libre.fm.
- The status line now shows how long the station has been playing, how far behind
  live you are while time-shifted, and when a recording is running.
- New Playback menu items and global hotkeys for Record, Skip Back, Skip Forward and
  Go to Live; Settings (Ctrl+T) holds the new options, and a station's own
  overrides live under Station Settings in its context menu.

- Playlist imports keep their structure: an imported file becomes a folder named
  after the playlist under Custom Stations, the playlist's groups become
  subfolders, and stations stay in the playlist's order instead of landing in
  one flat A-to-Z list. Exporting writes the folders back as playlist groups.
- Collections are folder trees now: create, rename, delete and reorder folders,
  and move stations between folders from the context menu. Browse shows the
  same folders.
- Favorites are folders too: make folders such as News, Music or BBC, move
  stations between them, and add a station straight into a favorites folder
  from its context menu.
- A real Settings window (Ctrl+T): the File menu's check items moved here,
  joined by Play a station as I move to it, Hide to the system tray when
  closed, and Maximize the window at startup.
- Closing the window really exits now; with Hide to the system tray on, it
  hides to the tray instead and playback continues. The window comes back
  from the tray by clicking the icon, double-clicking it or pressing Enter
  on it, or through the tray menu's Show Window.
- The window title is the playing station's name alone — no song, no stream
  details — so a screen reader's title command reads the station first. The
  now-playing line is station first too, then the song.
- Provider metadata dumps are cleaned out of song titles: an iHeart title
  that arrived as a record of attributes now reads as the song's name.
- The status line now also shows the stream's codec, bitrate and sample rate.

## 0.2.4

- Browse and search laut.fm as a new directory: its sixty most popular genres, up to 250
  stations each with Load More, and the same Sort choice as the other directories.
- Networks adds sixteen public broadcasters — RTÉ, RNZ, Deutsche Welle, RFI, ORF, SRF,
  RAI, RTVE, NPO, NRK, DR, Sveriges Radio, Yle, KCRW, WNYC and Minnesota Public Radio —
  plus 1.FM (34 channels) and 101.ru (19), 158 streams in all.
- Every added stream was checked live for an audio reply before shipping, and the plain
  http mounts were moved to https wherever the host answered there.
- Left out: NHK (only unrelated third-party relays could be verified), KBS (its HLS
  addresses carry expiring signed tokens), CNN, Bloomberg, Fox News Radio and ESPN Radio
  (aggregator relays only), ooTunes and Live365 (no public stream list to resolve).

## 0.2.3

- Networks adds Sunshine Live (7), Radio 538, SLAM!, Capital (4), Heart (5), Smooth (3),
  Classic FM, Gold, LBC (2), Radio X, Power Hit Radio Estonia and Lithuania (3 total),
  Dance Wave!, PulsRadio (8), Power Hitz (15), WHFF (18) and Radio Record (92). 101.ru
  is left out because its stream URLs are IP-bound tokens; ampradio.ru no longer
  resolves.
- Networks adds Planet Radio, the 33-channel FFH group, Nightride FM (7), FRISKY,
  XLtrax and Radio FG. AMG Radio, m2o, 1Club.FM and DI.fm are left out: their streams
  are behind player scripts, refuse the request, or no longer exist.

## 0.2.2

- Browse and search Icecast, SHOUTcast and Internet-Radio, with genre browsing and
  Load More for station pages. Networks adds Radio Paradise, NTS and WFMU.
- Icecast and SHOUTcast honour the Sort choice: name, bitrate, codec, genre and URL,
  and SHOUTcast also listeners for Votes, Clicks and Click trend. A field they cannot
  provide keeps their own order.
- Networks adds 181.fm (77 channels), .977 (9), Big R Radio (51), Radio Swiss (3),
  KEXP and WFUV. Every added stream was checked live before shipping.
- Streams whose server delivers slower than the bitrate (the TopRadio mounts are the known
  case) no longer stutter every few seconds: after a stream runs dry, it must refill a
  deeper cushion before it sounds again — three seconds, then nine, up to fifteen — so
  playback comes in long stretches between clean gaps. A clean minute earns the normal
  one-second cushion back, changing station resets it, and streams that never run dry
  behave exactly as before.

## 0.2.1

- Browse has a Sort choice: Radio Browser station lists in any order the API offers (the
  API default, name, votes, clicks, click trend, bitrate, codec, country, state, language,
  tags and more), with Reverse for the other direction. Search carries the same order.
- The Linux AppImage starts on a system that has no libjpeg installed.

## 0.2.0

The whole desktop interface, from the empty window to a shipped app.

- Main window with the Stations dial, every menu, the playback buttons, the volume slider and
  the message line; the SerrebiRadio starter list downloads on first run.
- Every station list tunes as you move: Search, Browse, Favorites, Recently Played, Collections
  and the main dial. Letter jump, Enter, Delete and the Applications key work in all of them.
- Browse over Radio Browser, TuneIn, iHeartRadio and Networks, with lazy folders, Load More and
  a remembered place.
- Favorites, Recently Played, custom stations, Collections, Add to Collection, Add All to
  Collection, custom-collection rename and reorder, playlist import and M3U export.
- Copy Stream URL, Song History (published and session), Sleep Timer, Equalizer, the Audio
  Device menu that follows lost and returned devices, and every setting saved at once.
- Speech through a live region, so every message is heard, with connection status and song
  titles each governable by its own setting.
- Nine global hotkeys with defaults and the shared shortcuts editor.
- Tray icon with Play/Pause, Stop, favorites, Show and Exit.
- One copy at a time: a second launch raises the first and exits.
- F1 user guide embedded in the app, About box with the licences page.
- Updates: a daily check and Help > Check for Updates, the download verified with minisign before
  it installs, portable and installed copies each updating the right way.
- Windows portable zip and per-user installer, signed; the release build embeds the version
  resource, icon and commit hash.

## 0.1.0

- Audio engine: three sources resolved and played warm, gapless station changes, ICY titles,
  session history, device switching, equalizer and sleep timer.
- Station sources and the library: Radio Browser, TuneIn, iHeartRadio, networks, playlists,
  favorites, collections, settings.
