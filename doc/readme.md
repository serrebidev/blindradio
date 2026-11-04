# BlindRadio

Screen-reader-first internet radio for Windows, with the whole interface usable from the
keyboard and every message spoken through your screen reader.

## Getting started

Start BlindRadio: focus lands on the Stations dial, and the first station plays. With the dial
empty, focus is on Play.

- Ctrl+P or Space on a list play or pause, Ctrl+S stop, Ctrl+M mute, Ctrl+Up and Ctrl+Down volume.
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
- Enter plays the focused station — always, even the one already playing: it reconnects, so after editing a station's URL, Enter on it hears the new address at once. Enter also starts a new time-shift buffer, and so does moving away from a station and back. When a station reconnects by itself, it keeps its buffer.
- Volume is remembered per station: the level you set while a station plays is the level it starts at next time. A station you have never set a level on uses the current volume.
- Delete removes it from that list.
- Alt+Up and Alt+Down reorder where the list allows it.
- The Applications key opens the station's menu.

Stations > Move to Playing Station reveals and focuses the station that is playing without
restarting it. Previous Station, Next Station, the favorite shortcuts, and playback from
Browse, Favorites or Recently Played also select the playing station in the main tree.
A station no list holds, such as an opened URL or a search result, plays without a row there.
Focus stays on the row you were on, and until you move, commands such as Add to Favorites act
on the playing station. Move to Playing Station then says the station is in no list.

## Searching

Ctrl+F opens Search: Name, Genre, Country and Language. Every box starts on Any. Enter in the
Name box starts the search; the results replace the list and the first result plays.

Search combines Radio Browser, TuneIn, iHeartRadio, Icecast, SHOUTcast, Internet-Radio,
IPRD, Radio.net, Live365, Online Radio Box and the curated networks. Genre, Country and Language filters apply to Radio
Browser; Icecast also matches Genre when a Name is given. Other directories use the Name
box. With Name empty, only Radio Browser searches the selected filters. A directory that
fails or takes too long is skipped so the others can still return results.

TuneIn lists some stations it says will not play here. They stay in the list with TuneIn's
reason beside the name, and BlindRadio still tries them when you play one. If it fails, the
message gives that reason, such as Not available in your country.

A station is listed only if every word in the Name box is in its name, genre or call sign.
Case, accents and spaces do not matter: serrebiradio finds Serrebi Radio, and radio finds
Rádio. A word must start or end a word in the station's name, so cknw does not find
BlacknWhita. Numbers are not spelled out: radio 1 does not find Radio One.

## Browsing

Ctrl+G opens Browse: Directories, Networks, Collections and Custom Stations. Directories
holds the BlindRadio Directory, Radio Browser, TuneIn, iHeartRadio, Icecast, SHOUTcast,
Internet-Radio, IPRD, Recommended Radio Streams,
Radio.net, Live365 and Online Radio Box. The BlindRadio Directory is the community's own submitted stations,
kept as playlist files in the BlindRadio-directory repository on GitHub: each file is a
folder (a country or a category), each playlist's groups are its subfolders, and the
app reads the repository live, so new
submissions appear in Browse without an app update;
Networks holds the curated networks. Right opens a folder, Left closes it, and a folder
that is still loading says
`Loading`. Enter on `Load More` loads the next page.
Shift+Up and Shift+Down move the focused row among its own kind on its level: a folder
among the folders, a station among the stations, a collection among the collections
(Alt+Up and Alt+Down work too). Rows in
Collections or Custom Stations move in your library itself — a pasted or new station
lands last in its folder, and Shift+Up walks it up into place; Custom Stations stays
ahead of the other collections wherever they list together. A directory folder keeps
the order you give it, remembered between sessions, since the directory's own order is
not yours to change, and a directory's stations keep the source's order. The four top
rows move the same way.
A playlist's or folder's name shows how many stations live under it — English 24,
Misc 15 — in Browse and in the main window's list alike; a remote directory's name
stays bare, since its full count is not known until every level under it has loaded.
Paste Station on a playlist or folder row puts the clipboard station at that row's own
level, beside its folders: a station copied or cut out of a subfolder can be pasted
straight into the parent playlist, without a subfolder having to hold it. A new
station's destination picker offers every playlist itself as well as its folders.
Ctrl+V pastes where you are standing, in Browse and in the main window alike: on a
folder or playlist row, into that row's level; on a station, beside it. Cutting a
station and pasting it where a copy of it already lives completes the move instead of
refusing: the source copy goes away and the station stays filed where it already was. The main window's list takes
the new order immediately, and Browse updates after a paste without reopening it.
Delete on a station in Collections or Custom Stations removes it from that
collection. Delete on a folder there, or on a collection itself, removes it too — after
asking, and naming how many stations go with it; a directory's folders are the directory's
own and cannot be deleted. A folder in Collections or Custom Stations also
renames and deletes from its context menu (Rename, Delete), at any depth; a remote directory's
folders are the directory's own and cannot be renamed. New Folder, on a
collection's name or inside one of its folders, creates a folder there — the
way to make a home for stations someone suggests for a playlist.

Icecast offers All Stations and By Genre. Its live directory stays cached for fifteen
minutes. SHOUTcast offers Top Stations and By Genre. Internet-Radio offers Featured
Stations and By Genre. IPRD offers By Country and By Genre, its hundred most used genres;
its catalog downloads the first time you open it or search, and stays cached for an hour.
Recommended Radio Streams offers one folder per category, read live from its GitHub
repository, and is not part of Search. Radio.net offers Top Stations, Local Stations, By
Genre, By Country, By Language and By Topic. TuneIn offers Local Radio, Music, Talk, Sports
and By Location. iHeartRadio offers All Stations, Genres and By City. Live365
offers one folder per genre, with its sub-genres inside. Online Radio Box offers By Genre and
one folder per region of the world, with that region's countries inside. Radio
Browser offers By Genre, By Country, By Language, By State and Top Stations. A listed station may be offline or unavailable in your region.

Networks carries English-language broadcasters and internet networks, among them BBC, NPR,
CBC, ABC Australia, RTÉ, RNZ, KCRW, WNYC, 181.fm, .977, Big R Radio, 1.FM, Capital, Heart,
Smooth, LBC, Virgin Radio UK, Triple M, NZME, SABC, K-LOVE, Pacifica, Nightride FM, Power Hitz,
WHFF, Radio Paradise, NTS, WFMU, and services for blind listeners. To ask for a non-English network, open an issue on GitHub.

The `Sort` choice at the top orders station lists: the API default, name, votes, clicks,
click trend, bitrate, codec, country, state, language, tags and more, with `Reverse` for
the other direction. Changing it loads the Radio Browser branches again in the new order.

Icecast, SHOUTcast and IPRD order by name, bitrate, codec, genre and URL. SHOUTcast also
orders by listeners, which is what Votes, Clicks and Click trend use for it. A field a
directory does not publish, or a list where every station is equal, keeps that directory's
own order. Internet-Radio, TuneIn, iHeartRadio and Networks have no ordering.

## Favorites and recently played

Ctrl+B lists Favorites as a tree: the root list plus folders you make with New Folder, such as
News, Music or BBC. Remove, Move Up, Move Down and Export work on stations and folders alike,
Shift+Up and Shift+Down move the focused station or folder, and a station's context menu moves
it into another folder. Ctrl+H lists Recently Played.
On the main window, Alt+1 to Alt+0 play favorites one to ten.

## Importing

Ctrl+I imports a playlist into Custom Stations as a folder named after the file; Ctrl+Shift+I
imports it as a collection named after the file. The playlist's groups become subfolders —
a group written as a path (News / World) becomes subfolders inside subfolders — and
its order is kept. M3U, M3U8, M3U Plus and PLS files are read. Local files and addresses that
are not HTTP are counted as skipped.

BlindRadio plays HLS and MPEG-TS streams as well as ordinary internet radio, taking the audio
from channels that also carry video. The audio can be AAC, including DVB's LATM carriage, MP3,
MP2, AC-3 or E-AC-3, with surround mixed down to stereo. HLS can be in TS or fragmented MP4 and
encrypted with AES-128. DRM, such as SAMPLE-AES or FairPlay, answers `Unsupported format`.

Opus and Vorbis streams, which send song titles in their own tags, show them like any other.

## Buffers

The Playback tab of Settings (Ctrl+T) has three buffer settings. Each is a text field you can type any number
into, followed by a slider that moves with it. Lower numbers play closer to live; higher ones ride
out a poor connection. A change applies to the next station you open, and the start buffer and
refill limit also apply to the one playing.

- Start buffer: audio held before a station starts, 0 to 30000 milliseconds. The default is 1000.
- Refill limit after dropouts: a stream that keeps running dry must refill more each time before
  it sounds again, up to this many seconds, 1 to 120. The default is 15.
- HLS segments behind live: where a live HLS stream starts, 1 to 10 segments back from the newest.
  The default is 2. At 1 a segment can arrive only just in time, so it can stutter.

For one stream right now, File → Open URL plays an address you paste without adding it
anywhere — handy for a quick listen or testing a stream. It still appears under Recently
Played, and from there it can be favorited or added to a collection like any station.

When you only have a web page with a player on it, File → Detect Audio Stream on Page… finds the
address for you. Type or paste the page's address and press Enter. BlindRadio reads the page, its
frames and its scripts, tries every stream address they name, and lists the ones that work; the
first one plays, as in Search. Enter plays, and the Applications key offers the usual station menu,
so a stream can be favorited, filed in a collection or copied with Copy Stream URL. If what you
paste is already a stream, it is listed alone. BlindRadio reads the page without running it, so a
player that only builds its address while running in a browser says `No audio streams found on this page`.

## Exporting

File > Export Current Playlist exports the selected playlist, Export All Playlists exports
all collections including Custom Stations, and Export Favorites exports Favorites. Each
opens a save dialog and preserves folders in the M3U file.

Favorites and Collections each have an Export button that writes an M3U through a save dialog.
The main window's tree exports too: a station from its menu, and a folder — a directory,
a subdirectory, or a whole collection in the All Lists view — from the folder's own menu,
with everything under it written as M3U groups — a subfolder's group is its full path
(News / World) — the way Browse exports them, so importing the file rebuilds the same
folders.

## Your own stations

Ctrl+N adds a station: a name and a stream URL. Anything but an http or https URL is refused
with `Not a stream URL` and the window stays open. Name Automatically, here and in Edit Station,
asks the URL for the station's own name and fills it in; if it comes out odd, edit it. Add to
chooses where the station is
filed: Custom Stations, any collection or folder, or Favorites and its folders — the
default is Custom Stations, where new stations went before the choice existed.

## Context menu

The Applications key on a station offers Play, Add to Favorites, Copy Stream URL, Add to
Collection, Edit Station and, when you have favorites folders, Add to Favorites Folder.
Edit Station holds the station's name, its info (the text after the name in the lists, which you can change or clear), its stream URL and its own settings — buffer
minutes, song recognition, scrobbling and equalizer for that station alone; there is no separate
Station Settings dialog anymore.
When a collection has folders it also offers Add to Collection Folder, which files the
station straight into one — Custom Stations' folders included, so a station found in
Search or Browse can be filed into an imported playlist's folder, the same places
Ctrl+N offers. For your own station it also offers Delete Custom Station. In the main window and
in Browse, a station in a collection also offers Delete, which takes out the copy you are on. On a
category it says `Nothing to do here`.

Station Info, on every station's menu, opens a read-only window you can read line by line and copy
from: name, info, slogan, genre, country, location, language, codec, bitrate, directory, homepage and
stream URL, whichever the station's directory gives. Radio Browser gives the most. A TuneIn station
is looked up as the window opens, so its slogan, genre, location, language and homepage join the
lines a moment later, even for a station TuneIn will not play where you are. Sample rate and channels come
from the stream itself, so they show while that station plays, and what the stream says about its
codec and bitrate wins over the directory. Station Info has no key until you give it one in Keyboard Shortcuts.

## Sound card

Playback, Audio Device lists the default device and every output device, with the active
one checked. If a device disappears the app says `Device lost. Using default.` and keeps playing
through the default.

## Equalizer

Ctrl+E opens the equalizer: Preset, Bass, Mid, Treble and Enabled. Moving a band enables
the equalizer and is heard at once. Choosing a preset only moves the sliders and reads
the bands — Enabled stays exactly as you left it, so arrowing through presets to compare
them never switches the equalizer on; check Enabled yourself to hear one. The presets are Flat, Bass
Boost, Treble Boost, Bass and Treble, Loudness, Voice, Warm, Clarity, Laptop Speakers
and Custom. The bands reach further into the middle of the spectrum than they used to,
so a preset is plainly heard on speakers as well as headphones; Loudness is the
strongest smile of the set. Loudness also levels the volume itself: it rides the
gain the way FFmpeg's dynaudnorm and mpv's normalization do, bringing quiet
stations up to the level of loud ones and hot ones down, without clipping —
near-silence is left alone, so dead air is not amplified. Leaving the Loudness
preset eases the gain back to normal.

A station can have its own equalizer. In Edit Station, Equalizer is Use Global, On or
Off. Use Global follows the Ctrl+E settings. Off plays that station with no equalizer. On
plays it through the preset you pick in Equalizer Preset, even with the equalizer off in
Ctrl+E: the built-in ones, Custom and your saved presets. The choice is applied when you
press OK and each time the station starts, and moving to another station returns to that
station's setting. So a very quiet station can always play through Loudness while the
rest stay as they are. If you delete a saved preset, stations using it go back to Use
Global. Ctrl+E and the equalizer hotkey always change the global settings; when the
playing station has its own, the title of the dialog and the hotkey's message say so.

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
one collection to another. Ctrl+Shift+L adds the focused station to a collection, or the playing one when no row is
focused.

Stations can also be copied between places: Copy Station on a station's context menu (or
Ctrl+C) copies it — any station, including one found in Browse or a directory — Cut Station
(Ctrl+X) marks it to move, and Paste Station (Ctrl+V) puts it beside the focused station, or
into the folder or collection the Applications key was pressed on. Pasting a cut station into
another folder of its own collection moves it there; a plain copy leaves the original where
it was. Ctrl+C and Ctrl+X work only on a station: on a folder they say so and change nothing. Copy
Stream URL, Add to Favorites, Add to Collection and Edit Station also do nothing on a folder. Paste into Favorites works the same way from the Favorites window.

## Song history

Ctrl+Y shows what played: the station's published history when it has one, with when each song
played, otherwise what you have heard since you tuned in. Copy and Enter copy the focused song.

Published history comes from the station's own list where one exists: iHeartRadio, SomaFM,
StreamTheWorld stations, Live365, laut.fm, AzuraCast stations, BBC, KEXP and Radio Paradise.
These usually cover hours. Online Radio Box continues from there, up to a week back. It is used
only when it clearly lists the same station: the same stream, or the same name in the same
country. So some stations have no history, and some of what it lists is not a song, such as a
show's name. Times on its older rows can be an hour out across a clock change.

## Recording

Ctrl+R starts and stops a recording of the playing station; files land in the recordings
folder, in the format Settings chooses. While a recording is in progress, the window title
ends with the word Recording and Record in the Recording menu is checked. Closing the app ends a recording too, so closing (or Exit)
asks first the same way; hiding to the tray keeps recording and never asks. Changing stations ends a recording, so the app asks first:
a confirmation names what happens — the recording stops, what is recorded so far is
saved — and only your Yes changes the station. The Recording menu holds it all: Record,
Pause Recording, Record Each Song to Its Own File, Write Buffer, the segment writes,
Scheduled Recordings and Open Record Folder, which opens the recordings folder.
Recording, Write Buffer writes the whole time-shift buffer — everything the station has
played since you tuned in, up to the buffer's length — to a WAV file in the same folder,
named like a recording.

The buffer also bookmarks itself. Every song or program title a station sends marks where
that segment starts, so stations that name what they play are divided up with nothing to
set. Playback, Previous Segment (Shift+F6) goes to the start of the segment you are in, or
to the one before if it only just began; Next Segment (Shift+F7) goes to the next one, and
after the last it goes back to live. Each jump says the title and how far behind live it is.

The Recording menu writes segments to their own files, in your recording format: Write
Current Segment (Ctrl+F8), Write Previous Segment (Ctrl+F6), Write Next Segment (Ctrl+F7)
and Write All Segments (Ctrl+F5), one file for each. Current, previous and next follow
where you are listening, and each says which title it took. Files land in the recordings
folder, named for the station, the title and when it played, and an earlier file is never
overwritten. The window stays free while they write, and you can change stations. The
segment still playing is written as far as it has got, and the oldest segment starts where
the buffer does.

Segments need the time-shift buffer and a station that sends titles, or song recognition.
A station that sends none says `No segments`. The boundaries carry the station's own
timing, so a second either side is normal, and a recognized song is marked when it is
recognized, which can be well after it began. A blank title marks nothing, so the audio
stays with the title before it.

Original stream format keeps the compressed MPEG audio or ADTS AAC bytes, including
when recording from the buffer. Header-dependent formats such as Ogg and FLAC use WAV
so recordings remain playable after seeking or splitting into separate songs.

Recording, Record Each Song to Its Own File is a recording mode for stations that name
their songs: while it is checked, a recording closes the current file when a new song
starts and opens one named for the station and the song, and so on until you stop the
recording. Stations that never name a song simply record as one file, as usual. Pausing
a recording carries across a song change.

Listen back in the time-shift buffer and the recording still follows what you hear: a file
ends and the next begins where the song changes at the buffer's position, so each file
holds a whole song taken from the buffer itself. Live, the split happens at the live edge
as the songs come in. The exact boundary carries the station's own timing — its song
information arrives when the station sends it — so a second either side is normal.

Recording, Scheduled Recordings sets recordings up ahead of time: a station, a start time,
a duration in minutes, and Once, Daily or Weekly. Add, Edit and Delete manage the list;
an entry can be switched off without deleting it. When the time comes the app tunes to
the station and records, and stops when the duration is up; a recording already running
by hand is never interrupted, and a Once entry switches itself off after it has run.
Start times are your local time.

An entry can be an alarm instead of a recording: set its Kind to Alarm and it just plays
its station at the start time — wake up to it. Duration does not apply to alarms. An
alarm that comes due while the app is closed is not replayed later; if the app starts
within five minutes of the alarm time, it still plays.

## Sleep timer

Ctrl+1 to Ctrl+6 sleep after 15, 30, 45, 60, 90 or 120 minutes; Ctrl+0 turns it off. When it is
due, playback stops.

## Settings

Ctrl+T opens Settings (File menu). It has four tabs: General, Playback, Recording, and Songs,
which holds song recognition and scrobbling. Focus starts on the tabs: Left and Right change tab, Tab moves into
the page, and Ctrl+Tab changes tab from anywhere in the window. OK saves all four.

On Windows, the service tokens on the Songs tab are stored encrypted for your Windows account.
If you copy your data folder to another PC or account, those fields open blank; enter them again
there.

The General checkboxes are Announce Song Titles, Announce
Connection Status, Announce Actions, Close Lists When Playing, Play Last Station at Startup, Play a Station as
I Move to It, Hide to the System Tray When Closed, Maximize the Window at Startup, Check
for Updates Automatically (the daily update check; Help, Check for Updates always works), Report
Plays to Radio Browser, and Expand All Directories in the Main Window. Report Plays is on by
default: when a station from Radio Browser has played for ten seconds, BlindRadio tells Radio
Browser which station, as its rules ask of every app, and that is what its popular lists are built
from. Radio Browser sees your IP address and the station, nothing else. With Play a Station as I Move to It off, moving
through a list only selects the station; Enter plays it. With Expand All Directories off,
the main window's directories start collapsed, so a long list opens as a short list of
directories to walk into; a directory you open or close by hand keeps its state either way.
A message after a menu command or a closed dialog waits about half a second, so your screen
reader reads the station list first and then the message, rather than cutting it off.
FM Sound Between Stations, on the Playback tab, is the soft hiss BlindRadio plays while a new station tunes in, like a dial
crossing the band, so tuning never has dead air; turn it off and the gap is silent. The Startup List chooses what the main list shows when the app starts: the default
stations, All Playlists, or one of your collections —
an imported playlist is a collection, so your own playlists can be what opens. The main list
is a tree: All Playlists shows one folder per playlist with its directories inside, in the
order you give them, and a
single playlist shows its directories directly. The Filter box above the tree narrows it
to the stations whose names contain what you type; clearing the box brings the whole tree
back. In the tree itself, typing a letter jumps to the next station starting with it, the
way lists usually work. Shift+Up and Shift+Down move the focused station, folder or, in
All Playlists, collection, as they do in Browse (Alt+Up and Alt+Down work too); not while
the Filter box has text in it. Delete on a station takes the copy you are on out of that
list. Delete on a folder or collection removes it after asking, naming how many stations go with it, and
Rename and Delete are on its context menu. Custom Stations itself stays. A station in
one of your playlists, your favorites, or Custom Stations can be renamed and re-pointed:
Edit Station on its context menu edits its name, info and stream URL everywhere it appears.
Collections themselves are renamed in the Collections window (Rename button, or Rename on
the context menu). The time-shift options are on the Playback tab, the recording options on
the Recording tab, and the recognition and scrobbling options on the Songs tab.

## Recognizing a song when a station sends no title

BlindRadio does not use Shazam. It uses AudD, and only when you turn it on: open
Settings (Ctrl+T), go to its Songs tab, check Recognize Songs Without Metadata, set Recognition Service
to AudD, and paste an AudD token. A token is free from audd.io: sign up there and
copy the API token from your account. Without a token nothing is recognized, and
saving Settings with recognition on but no token says so. With a token, a short
sample of the playing station is sent to AudD every few minutes; the recognized
song shows where stream titles show, and Announce Recognized Songs speaks it as
it changes. Recognition can also be switched per station in Edit Station.

## When a stream drops

`Connection lost. Reconnecting.` is always spoken, then `Reconnected`, or `Failed: <station>`.
Announce Connection Status only governs playing, buffering and stopped messages.

## Updates

Once a day, ten seconds after it starts, BlindRadio asks GitHub whether a newer release is
published and offers it. Help, Check for Updates asks at once and says when there is nothing new.
The download is verified before it is used; a portable copy updates itself in place and keeps its
data folder, and an installed copy runs the new installer. BlindRadio restarts by itself.

Help, View Changelog opens the release history in your browser, and works offline. Help, Go to
BlindRadio GitHub Page opens the project's page.

## Tray icon

The app keeps running in the tray. Ctrl+Alt+Shift+Y hides it and shows it again. The tray menu
plays or stops, plays any favorite, shows the window and exits. Closing the window exits,
unless Hide to the System Tray When Closed is set in Settings: then closing hides the window
and playback continues.

## Now playing

The now-playing line shows `Station` or `Station - Song`, with how long it has played and
the stream's format, such as `MP3, 128 kbps, 44.1 kHz, stereo`. A stream that declares no bitrate,
such as HLS, gets one worked out from its audio about four seconds in. The window title shows
the app's name and the station and song, as it always has. Ctrl+Alt+Shift+N says the
whole line whatever your announcement settings are.

When you are listening back in the time-shift buffer, the title names the song playing
at the buffer's position, not the live one; go live and it names the live song again.
Song changes are announced in the buffer too, under the same Announce Song Titles
setting as live ones: as playback moves forward through the buffer and the next song
starts, it is spoken.

The global hot keys work from any app. With NVDA or JAWS running, announcements are spoken
by that screen reader itself, so they reach you wherever you are working, not only in the
BlindRadio window. The status bar holds the position line — how long the station has played, live
or how far behind, whether a recording is running, and the stream's format; the station
and song stay in the title, where they do not crowd it out. On Windows it is the standard
Windows status bar control in its own borderless window along the main window's bottom
edge, so NVDA's status line command (NVDA+End) reads it like any other app's status bar,
and on Linux it is a standard part of the main window, which Orca reads the same way.

## Keyboard shortcuts

These are the defaults. Every one can be changed, cleared, or given to an action that
has none — Write Buffer, for example — in File, Keyboard Shortcuts, which lists them
under Playback, Stations, Recording and App. The menus always print the keys as they
are now.
Global Hotkeys, below, are the separate set that works from other apps.

| Key | Action |
| --- | --- |
| Ctrl+P | Play or pause |
| Space | Play or pause, on a list, a tree or the volume slider; elsewhere Space types or presses as usual |
| Ctrl+S | Stop |
| Ctrl+M | Mute, saying `Muted` or `Unmuted` |
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
| Shift+F6, Shift+F7 | Previous segment, next segment |
| Ctrl+F8 | Write current segment |
| Ctrl+F6, Ctrl+F7 | Write previous segment, write next segment |
| Ctrl+F5 | Write all segments |
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
