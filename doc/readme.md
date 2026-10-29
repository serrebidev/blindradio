# BlindRadio

Screen-reader-first internet radio for Windows, with the whole interface usable from the
keyboard and every message spoken through your screen reader.

## Getting started

Start BlindRadio: focus lands on the Stations dial, and the first station plays. With the dial
empty, focus is on Play.

- Ctrl+P play or pause, Ctrl+S stop, Ctrl+M mute, Ctrl+Up and Ctrl+Down volume.
- Ctrl+D adds or removes the focused station from Favorites.
- F1 opens this guide.

Your favorites, settings and collections live in `%APPDATA%\BlindRadio`. Run the app from a
folder containing a `data` folder and it uses that folder instead, which is how the portable
copy keeps its own settings.

## Dial tuning

Every station list tunes as you move: arrowing, a letter jump, Home and End all play the station
under focus at once, with no speech beyond your screen reader's own. Opening a list lands on a
station and plays it.

- Letters jump to the next name starting with that letter.
- Enter plays the focused station.
- Delete removes it from that list.
- Alt+Up and Alt+Down reorder where the list allows it.
- The Applications key opens the station's menu.

## Searching

Ctrl+F opens Search: Name, Genre, Country and Language. Every box starts on Any. Enter in the
Name box starts the search; the results replace the list and the first result plays.

Search combines Radio Browser, TuneIn, iHeartRadio, Icecast, SHOUTcast, Internet-Radio,
laut.fm and the curated networks. Genre, Country and Language filters apply to Radio
Browser; Icecast also matches Genre when a Name is given. Other directories use the Name
box. With Name empty, only Radio Browser searches the selected filters. A directory that
fails or takes too long is skipped so the others can still return results.

## Browsing

Ctrl+G opens Browse: Directories, Networks, Collections and Custom Stations. Directories
holds Radio Browser, TuneIn, iHeartRadio, Icecast, SHOUTcast, Internet-Radio and laut.fm;
Networks holds the curated networks. Right opens a folder, Left closes it, and a folder
that is still loading says
`Loading`. Enter on `Load More` loads the next page.

Icecast offers All Stations and By Genre. Its live directory stays cached for fifteen
minutes. SHOUTcast offers Top Stations and By Genre. Internet-Radio offers Featured
Stations and By Genre. laut.fm offers its sixty most popular genres, each listing up to
250 stations. A listed station may be offline or unavailable in your region.

Networks carries the broadcasters and internet networks, among them RTÉ, RNZ, Deutsche
Welle, RFI, ORF, SRF, RAI, RTVE, NPO, NRK, DR, Sveriges Radio, Yle, KCRW, WNYC, MPR,
181.fm, .977, Big R Radio, Radio Swiss, 1.FM, 101.ru, Sunshine Live, Radio 538, SLAM!,
Capital, Heart, Smooth, Classic FM, Gold, LBC, Radio X, Planet Radio, Power Hit Radio in
Estonia and Lithuania, Dance Wave!, PulsRadio, Nightride FM, FRISKY, XLtrax, Radio FG,
Power Hitz, WHFF, Radio Record, KEXP, WFUV, Radio Paradise, NTS and WFMU.

The `Sort` choice at the top orders station lists: the API default, name, votes, clicks,
click trend, bitrate, codec, country, state, language, tags and more, with `Reverse` for
the other direction. Changing it loads the Radio Browser branches again in the new order.

Icecast, SHOUTcast and laut.fm order by name, bitrate, codec, genre and URL. SHOUTcast also
orders by listeners, which is what Votes, Clicks and Click trend use for it. A field a
directory does not publish, or a list where every station is equal, keeps that directory's
own order. Internet-Radio, TuneIn, iHeartRadio and Networks have no ordering.

## Favorites and recently played

Ctrl+B lists Favorites with Remove, Move Up, Move Down and Export. Ctrl+H lists Recently Played.
On the main window, Alt+1 to Alt+0 play favorites one to ten.

## Importing

Ctrl+I imports a playlist into Custom Stations; Ctrl+Shift+I imports it as a collection named
after the file. M3U, M3U8 and PLS files are read.

## Exporting

Favorites and Collections each have an Export button that writes an M3U through a save dialog.

## Your own stations

Ctrl+N adds a station: a name and a stream URL. Anything but an http or https URL is refused
with `Not a stream URL` and the window stays open.

## Context menu

The Applications key on a station offers Play, Add to Favorites, Copy Stream URL, Add to
Collection and, for your own station, Delete Custom Station. On a category it says
`Nothing to do here`.

## Sound card

File, Audio Device lists the default device and every output device, with the active one
checked. If a device disappears the app says `Device lost. Using default.` and keeps playing
through the default.

## Equalizer

Ctrl+E opens the equalizer: Preset, Bass, Mid, Treble and Enabled. Choosing a preset moves the
sliders and reads the bands. Up and Down move a band by one dB, Home and End go to the ends,
PageUp and PageDown by five. Moving a band by hand makes the preset Custom.

## Collections

Ctrl+L lists Custom Stations and every collection with its stations. F2 renames a collection,
Delete deletes it after asking, Delete on a station removes it, Alt+Up and Alt+Down reorder, and
the Applications key adds every station of one collection to another. Ctrl+Shift+L adds the
playing station to a collection.

## Song history

Ctrl+Y shows what played: the station's published history when it has one, with when each song
played, otherwise what you have heard since you tuned in. Copy and Enter copy the focused song.

## Sleep timer

Ctrl+1 to Ctrl+6 sleep after 15, 30, 45, 60, 90 or 120 minutes; Ctrl+0 turns it off. When it is
due, playback stops.

## Settings

File holds Announce Song Titles, Announce Connection Status, Close Lists When Playing and Play
Last Station at Startup. Each one applies and is saved the moment you toggle it.

## When a stream drops

`Connection lost. Reconnecting.` is always spoken, then `Reconnected`, or `Failed: <station>`.
Announce Connection Status only governs playing, buffering and stopped messages.

## Updates

Once a day, ten seconds after it starts, BlindRadio asks GitHub whether a newer release is
published and offers it. Help, Check for Updates asks at once and says when there is nothing new.
The download is verified before it is used; a portable copy updates itself in place and keeps its
data folder, and an installed copy runs the new installer. BlindRadio restarts by itself.

## Tray icon

The app keeps running in the tray. Ctrl+Alt+Shift+Y hides it and shows it again. The tray menu
plays or stops, plays any favorite, shows the window and exits. Closing the window exits.

## Now playing

The title and the now-playing line show `Station` or `Song - Station`. Ctrl+Alt+Shift+N says it
whatever your announcement settings are.

## Keyboard shortcuts

| Key | Action |
| --- | --- |
| Ctrl+P | Play or pause |
| Ctrl+S | Stop |
| Ctrl+M | Mute |
| Ctrl+Up, Ctrl+Down | Volume up, down |
| Ctrl+D | Add to or remove from Favorites |
| Ctrl+F | Search |
| Ctrl+G | Browse |
| Ctrl+B | Favorites |
| Ctrl+H | Recently Played |
| Ctrl+L | Collections |
| Ctrl+N | Add Custom Station |
| Ctrl+I, Ctrl+Shift+I | Import, import as collection |
| Ctrl+Shift+L | Add to Collection |
| Ctrl+U | Copy Stream URL |
| Ctrl+Y | Song History |
| Ctrl+E | Equalizer |
| Ctrl+1 to Ctrl+6, Ctrl+0 | Sleep timer, off |
| Alt+1 to Alt+0 | Play favorite 1 to 10 |
| Alt+V | Volume slider |
| F1 | This guide |
| Escape | Close the window |

## Global hotkeys

These work while BlindRadio is in the background, and File, Global Hotkeys rebinds them.

| Keys | Action |
| --- | --- |
| Ctrl+Alt+Shift+P | Play or pause |
| Ctrl+Alt+Shift+S | Stop |
| Ctrl+Alt+Shift+PageUp, PageDown | Volume up, down |
| Ctrl+Alt+Shift+Right, Left | Next, previous favorite |
| Ctrl+Alt+Shift+N | Announce what is playing |
| Ctrl+Alt+Shift+Y | Hide or show the window |
| Ctrl+Alt+Shift+E | Equalizer on or off |

A chord another program already holds is named once at startup as `Hotkey taken: <chord>`.
