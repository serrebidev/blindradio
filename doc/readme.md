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
laut.fm, Radio.net and the curated networks. Genre, Country and Language filters apply to Radio
Browser; Icecast also matches Genre when a Name is given. Other directories use the Name
box. With Name empty, only Radio Browser searches the selected filters. A directory that
fails or takes too long is skipped so the others can still return results.

## Browsing

Ctrl+G opens Browse: Directories, Networks, Collections and Custom Stations. Directories
holds Radio Browser, TuneIn, iHeartRadio, Icecast, SHOUTcast, Internet-Radio, laut.fm
and Radio.net;
Networks holds the curated networks. Right opens a folder, Left closes it, and a folder
that is still loading says
`Loading`. Enter on `Load More` loads the next page.
Shift+Up and Shift+Down move a folder among the folders on its level (Alt+Up and
Alt+Down work too). A folder in Collections or Custom Stations moves in your library
itself; a directory folder keeps the order you give it, remembered between sessions,
since the directory's own order is not yours to change.
Delete on a station in Collections or Custom Stations removes it from that
collection. Delete on a folder there removes the folder too — after asking, and
naming how many stations go with it; a directory's folders are the directory's
own and cannot be deleted.

Icecast offers All Stations and By Genre. Its live directory stays cached for fifteen
minutes. SHOUTcast offers Top Stations and By Genre. Internet-Radio offers Featured
Stations and By Genre. laut.fm offers its sixty most popular genres, each listing up to
250 stations. Radio.net offers its Top Stations and Local Stations. Radio Browser
offers By Genre, By Country, By Language, By State and Top Stations. A listed station
may be offline or unavailable in your region.

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

Ctrl+B lists Favorites as a tree: the root list plus folders you make with New Folder, such as
News, Music or BBC. Remove, Move Up, Move Down and Export work on stations and folders alike,
Shift+Up and Shift+Down move a folder among its siblings, and a station's context menu moves
it into another folder. Ctrl+H lists Recently Played.
On the main window, Alt+1 to Alt+0 play favorites one to ten.

## Importing

Ctrl+I imports a playlist into Custom Stations as a folder named after the file; Ctrl+Shift+I
imports it as a collection named after the file. The playlist's groups become subfolders and
its order is kept. M3U, M3U8 and PLS files are read.

For one stream right now, File → Open URL plays an address you paste without adding it
anywhere — handy for a quick listen or testing a stream. It still appears under Recently
Played, and from there it can be favorited or added to a collection like any station.

## Exporting

Favorites and Collections each have an Export button that writes an M3U through a save dialog.

## Your own stations

Ctrl+N adds a station: a name and a stream URL. Anything but an http or https URL is refused
with `Not a stream URL` and the window stays open. Add to chooses where the station is
filed: Custom Stations, any collection or folder, or Favorites and its folders — the
default is Custom Stations, where new stations went before the choice existed.

## Context menu

The Applications key on a station offers Play, Add to Favorites, Copy Stream URL, Add to
Collection, Station Settings and, when you have favorites folders, Add to Favorites Folder.
When a collection has folders it also offers Add to Collection Folder, which files the
station straight into one — Custom Stations' folders included, so a station found in
Search or Browse can be filed into an imported playlist's folder, the same places
Ctrl+N offers. For your own station it also offers Delete Custom Station. On a
category it says `Nothing to do here`.

## Sound card

File, Audio Device lists the default device and every output device, with the active one
checked. If a device disappears the app says `Device lost. Using default.` and keeps playing
through the default.

## Equalizer

Ctrl+E opens the equalizer: Preset, Bass, Mid, Treble and Enabled. Choosing a preset or
moving a band enables the equalizer and is heard at once; Enabled is the off switch.
Choosing a preset moves the sliders and reads the bands. The presets are Flat, Bass
Boost, Treble Boost, Bass and Treble, Loudness, Voice, Warm, Clarity, Laptop Speakers
and Custom. The bands reach further into the middle of the spectrum than they used to,
so a preset is plainly heard on speakers as well as headphones; Loudness is the
strongest smile of the set. Loudness also levels the volume itself: it rides the
gain the way FFmpeg's dynaudnorm and mpv's normalization do, bringing quiet
stations up to the level of loud ones and hot ones down, without clipping —
near-silence is left alone, so dead air is not amplified. Leaving the Loudness
preset eases the gain back to normal.
Up and Down move a band by one dB, PageUp and PageDown by one dB too, and Home and End
go to the ends. Moving a band by hand makes the preset Custom — and Custom remembers
your bands: switch to another preset and back and your tuning is still there.
To keep a tuning under your own name, type it in the Name box and choose Save Preset;
it joins the Preset list. Delete Preset removes the saved preset showing in the list.

## Collections

Ctrl+L lists Custom Stations and every collection as a tree, folders included. New Folder
makes a folder inside the focused one. F2 renames a collection or a folder, Delete deletes it
after asking, Delete on a station removes it, Alt+Up and Alt+Down reorder, Shift+Up and
Shift+Down move a folder among its siblings, a station's context
menu moves it between folders, and the Applications key on a collection adds every station of
one collection to another. Ctrl+Shift+L adds the playing station to a collection.

Stations can also be copied between places: Copy Station on a station's context menu (or
Ctrl+C) copies it — any station, including one found in Browse or a directory — Cut Station
(Ctrl+X) marks it to move, and Paste Station (Ctrl+V) puts it beside the focused station, or
into the folder or collection the Applications key was pressed on. Pasting a cut station into
another folder of its own collection moves it there; a plain copy leaves the original where
it was. Paste into Favorites works the same way from the Favorites window.

## Song history

Ctrl+Y shows what played: the station's published history when it has one, with when each song
played, otherwise what you have heard since you tuned in. Copy and Enter copy the focused song.

## Recording

Ctrl+R starts and stops a recording of the playing station; files land in the recordings
folder, in the format Settings chooses. Playback, Write Buffer writes the whole time-shift
buffer — everything the station has played since you tuned in, up to the buffer's length —
to a WAV file in the same folder, named like a recording.

Playback, Scheduled Recordings sets recordings up ahead of time: a station, a start time,
a duration in minutes, and Once, Daily or Weekly. Add, Edit and Delete manage the list;
an entry can be switched off without deleting it. When the time comes the app tunes to
the station and records, and stops when the duration is up; a recording already running
by hand is never interrupted, and a Once entry switches itself off after it has run.
Start times are your local time.

## Sleep timer

Ctrl+1 to Ctrl+6 sleep after 15, 30, 45, 60, 90 or 120 minutes; Ctrl+0 turns it off. When it is
due, playback stops.

## Settings

Ctrl+T opens Settings (File menu). The General checkboxes are Announce Song Titles, Announce
Connection Status, Close Lists When Playing, Play Last Station at Startup, Play a Station as
I Move to It, Hide to the System Tray When Closed, Maximize the Window at Startup, and Check
for Updates Automatically (the daily update check; Help, Check for Updates always works). With
Play a Station as I Move to It off, moving through a list only selects the station; Enter
plays it. The Startup List chooses what the main list shows when the app starts: the default
stations, All Playlists, or one of your collections —
an imported playlist is a collection, so your own playlists can be what opens. The main list
is a tree: All Playlists shows one folder per playlist with its directories inside, and a
single playlist shows its directories directly. The Filter box above the tree narrows it
to the stations whose names contain what you type; clearing the box brings the whole tree
back. In the tree itself, typing a letter jumps to the next station starting with it, the
way lists usually work. A station in
one of your playlists, your favorites, or Custom Stations can be renamed and re-pointed:
Edit Station on its context menu edits its name and stream URL everywhere it appears.
Collections themselves are renamed in the Collections window (Rename button, or Rename on
the context menu). The time-shift, recording, recognition and scrobbling options live in
the same window.

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
plays or stops, plays any favorite, shows the window and exits. Closing the window exits,
unless Hide to the System Tray When Closed is set in Settings: then closing hides the window
and playback continues.

## Now playing

The window title is the playing station's name alone. The now-playing line shows `Station` or
`Station - Song`, with how long it has played and the stream's format, such as
`MP3, 128 kbps, 44.1 kHz, stereo`. Ctrl+Alt+Shift+N says it whatever your announcement
settings are.

The global hot keys work from any app. With NVDA running, announcements are spoken by
NVDA itself, so they reach you wherever you are working, not only in the BlindRadio
window. NVDA's own status line command, NVDA+End, reads the same line; on Windows the
status bar sits in its own thin window along the main window's bottom edge, exactly
where that command looks, and the bar also answers UI Automation queries itself —
as a status bar, named with the current line, over the whole bottom edge — so the
command works whichever way NVDA asks, with no add-on and nothing to install. On
Linux the status bar is a standard part of the main window, which Orca reads the
same way.

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
| F5 | Jump to start of buffer |
| F6, Ctrl+Left | Skip back |
| F7, Ctrl+Right | Skip forward |
| F8, Ctrl+Shift+Right, Ctrl+J | Jump to live |
| Ctrl+Shift+Left | Jump to start of buffer |
| Ctrl+1 to Ctrl+6, Ctrl+0 | Sleep timer, off |
| Alt+1 to Alt+0 | Play favorite 1 to 10 |
| Alt+Left, Alt+Right | Previous / next station played this session |
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
