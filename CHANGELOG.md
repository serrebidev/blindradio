# Changelog

Readable release history for BlindRadio, newest first. When adjacent releases
were part of the same fix stream, they are combined with a version range such
as `0.2.2-0.2.3`. Each release's section below is its GitHub release note,
taken from this file by the release workflow.

## 0.2.80

- Updates now come from the project's own Forgejo, git.serrebiradio.com, instead of GitHub. The check in Help, Check for Updates and the daily check both look there, and every download is still verified with minisign before it installs. One manual step: copies up to 0.2.79 still ask GitHub, so they will never be offered this one — install 0.2.80 by hand once, and later releases arrive through the updater again.

## 0.2.79

- Recordings no longer own playback. Start a recording, then change stations freely: the recording moves onto its own stream and keeps recording in the background while you listen to anything else — no confirmation, no interruption, and stopping playback leaves it running. Ctrl+R still starts and stops it, Pause Recording still works (a paused background recording closes its stream and reopens it on resume), and Record Each Song still splits its files on the recorded station's own titles. Scheduled recordings now start in the background from the outset: the app never tunes to the scheduled station at all, silent or not, so the Record Silently box no longer changes what you hear — a scheduled recording is unheard either way, and the announcement says it is recording in the background. If the recorded station's own stream is lost for good, the recording ends with its file saved and the app says so. One case still asks first: pressing Enter on the very station that is recording in the background, because an explicit play restarts that station's stream. Closing the app still ends a recording and still asks first, as before. (Brandon's order, correcting the boundary stated in 0.2.78.)

## 0.2.78

- Scheduled recordings can now record silently, and silent is the default. Each entry in Scheduled Recordings has a Record Silently box, checked for new entries: when the recording starts, playback is muted for the whole run, so the app never talks over other work or another stream, and the level is restored when the recording ends. Uncheck the box to hear the station while it records. The start of a scheduled recording was already announced; a silent one announces itself as recording silently with playback muted, and the saved file is announced at the end either way. One boundary, stated plainly: a scheduled recording still tunes the app to its station, so listening to a different station inside BlindRadio during the recording is not possible — the box controls whether you hear it, not which station the app is on. Entries saved before this release keep the audible behaviour they had. (Kaveinthran's suggestion, after his 0.2.77 retest confirmed scheduled recordings work.)
- The schedule form now opens on the station you most likely want. A new entry starts on the station your last schedule used — schedules repeat themselves — or on the station playing now when there is no earlier schedule, and the station list always opens with a station selected. Recent stations join the list too, so a station played from Browse or a network can be scheduled without favouriting it first. The list itself is unchanged otherwise: your playlists, favourites and favourite folders, in name order. (Kaveinthran's suggestion; the list he saw was indeed all his favourites and playlists.)

## 0.2.77

- Scheduled Recordings: the add form could still drop a schedule on OK. The 0.2.75 fix built the entry only once the dialog had closed with the exact OK result, and then re-read the form's fields from the closed dialog; on paths where the close did not travel exactly that way, the entry never formed. The result no longer rides on the close at all: pressing OK hands the built schedule straight to the caller, the form stays open and says why when a field cannot be accepted — including a station it cannot read, which used to fail in silence — and if the dialog closes with OK without the handover having run, the fields are read one last time as a fallback. The OK handler is also bound once now; reopening the form after a bad field no longer stacks another one on top.

## 0.2.76

- Moondust joins the directories in Browse: the open station dataset from 12moon commons, published under CC0 and read live from its GitHub repo. Stations are filed by country — 227 country lists, the United States alone holding over six thousand stations — each with its primary stream, its genre, country and format on the row. Search covers the largest country lists, or just the country a search is filtered to. Two honest limits: the dataset is young and says itself that its streams have not been checked for playback, so a Moondust station that stays silent may be dead at the source; and stations with no known country are not in this first cut.

## 0.2.75

- Fixed Scheduled Recordings adding nothing. Setting up a schedule for a station, Mushroom FM included, and pressing OK could close the form with the entry silently dropped: the entry was only ever built inside the OK button's own click handling, and how the click reached the button — mouse, Space, or Enter while OK was the default button — decided whether that handling ran at all. The form now builds the entry from its fields after OK however it was pressed, and a field it cannot accept reopens the form with what you typed still in it and says why, instead of the schedule vanishing without a word.
- Adding a station to Favorites or a playlist no longer files a second copy of a station you already have. The same stream found in different places — Radio Browser, TuneIn, a network, a playlist — carried a different identity in each, so the favorites and playlist checks, which compared identities only, let twins in. They now match by stream address as playlist import already did, toggling another place's copy of a favorite removes the one you hold instead of adding its twin, and an add that is skipped says the station is already there. (Kaveinthran's report.)
- Favorites is now a Startup List choice. If your station lists live in Favorites, Settings can start the main list on them — folders and all — alongside Default Stations, All Playlists and any single playlist. (Kaveinthran's suggestion.)
- Volume now goes to 200%. Quiet stations can be boosted past full volume, the way VLC allows: the volume slider, Volume Up and Volume Down, and each station's remembered volume all run from silent to 200%, in the same one-percent steps. Past 100% the sound is amplified and can distort on loud passages — that trade is yours to make per station, and a level you set is remembered for that station as before.

## 0.2.74

- Volume steps are now one percent. Volume Up and Volume Down, the arrow keys on the volume slider, and the other sliders' arrow keys all move by 1 instead of 5, so the whole range from silent to full is reachable one percent at a time and quiet streams no longer jump from medium straight to off. Page Up and Page Down still move by ten for covering ground fast.
- Networks audited against each network's own published channel list. Radio Record joins as a new network with all 92 of its channels that answer — the one non-English network carried, a single dance network from Russia. WNYC gains Q2 Music, its contemporary classical channel, and Minnesota Public Radio gains Celebrate the Maestro. Everything else already matched its network's own list exactly: BBC, SomaFM, 1.FM, 181.fm, NTS, WHFF and the rest were re-checked channel by channel.

## 0.2.73

- BlindRadio can now be translated. The whole interface runs through patois (gettext): the text lives in one template, `po/blindradio.pot`, translations are plain .po files anyone can edit in Poedit, and compiled catalogs ride inside the app. Settings gains a Language choice on the General tab — System Default follows your computer, and any translated language is listed by its own name; the standard dialog buttons follow too. English is the only finished language today; translations are welcome as pull requests.
- AMI Audio's address is corrected. The address 0.2.72 pointed at was the station's web player page, not a stream, so it could never play; the entry now uses the station's actual stream address, which is live again.

## 0.2.72

- 2020 The Beacon joins the built-in Networks, with the other stations for blind listeners.
- AMI Audio is back: AMI moved the stream to a new address (the old one 0.2.71 removed really was dead), and the entry now points at the working one.

## 0.2.71

- The built-in Networks now cover the blindness stations properly. The Global Voice, Radio Udaan, Treehouse Radio and Dodge Radio (with its Christmas channel) are new, Vision Australia Radio gains Sydney (2RPH) and Darwin (8RPH) alongside its seven existing cities, and they join ACB Media, Mushroom FM, RNIB Connect Radio, NFBRN, WRBH and Double Tap Live. AMI Audio is gone: its streaming host has discontinued the stream address, so the entry no longer played anything.

## 0.2.69

- Recordings in Original stream format are now named for what the stream really is, and start at a whole frame. An AAC station at an address with no file suffix, such as Classic FM, was saved as an .mp3 file holding AAC audio, which Foobar2000 and most other players refuse; it is now saved as .aac, likewise .mp2 where that fits. A recording started before the format is known, such as a scheduled one, is renamed as soon as it is. This covers whole recordings, each-song files and written segments alike.

## 0.2.68

- YouTube and Twitch live streams now play. Paste the link into Open URL, Add Custom Station or Detect Audio Stream on Page and BlindRadio plays only the audio. The first time, it downloads yt-dlp and, for YouTube, Deno, and says so while it does; both then keep themselves up to date. Saved stations keep the link, so a new stream address is found each time. A channel that is not on the air says Not live. Twitch ads play like the rest of the stream.
- BBC Persian TV joins the BBC network in Browse, as 128k audio.
- Long HLS playlists are now read whole. One past 64 KB used to be cut off, and its last address failed.

## 0.2.70

- No more "debugging check" dialogs. The interface library was being built with its developer consistency checks switched on, so an internal check a shipped build is supposed to skip could stop the app with a dialog. It is now built the way release builds are meant to be, with those checks compiled out — the same fix as the other apps.

## 0.2.67

- The Equalizer's sliders no longer switch anything on: moving a band or the Preamp changes the value only, and Enabled and Enable Preamp stay exactly as you left them — the same rule presets already followed. Check the boxes yourself when you want to hear the change.

## 0.2.66

- The Equalizer now has ten frequency bands, the requested listening presets, and an adjustable Preamp with its own on/off switch.
- Edit Station now offers a per-station metadata offset from -30 to 30 seconds. It corrects title timing, history, segment navigation and segment recordings.
- Streema station pages, including MSNBC Live Audio, resolve to playable streams.

## 0.2.65

- Open URL and Add Custom Station now accept station page links from radio.net, TuneIn, Zeno.FM, Streema (including language subdomains), and Live Online Radio. Playing a saved link looks up its stream automatically. Name Automatically also follows these pages to the stream's name.

## 0.2.64

- In Browse, the context menu on a directory's folder, such as Celine Dion under By Genre in Online Radio Box, now adds the whole folder and its stations at once: Add to Favorites, Add to Collection (with New Collection), Add to Favorites Folder and Add to Collection Folder. It arrives as a folder with its subfolders kept, and you hear how many stations came. Adding the same folder again only brings what is new. One add stops at 5000 stations and says so, so a whole directory cannot run for hours.
- Removing a station from Favorites or a collection now takes out every copy in its folders, not only the first one found.
- Export on a directory's folder in Browse now says how many stations it wrote. The file was saved before too, but the message was lost to an internal error.

## 0.2.63

- F5, F6 and F7 now measure from what you are hearing. Many stations, such as WNYC, send up to 15 seconds of audio the moment you connect, and BlindRadio counted that as time behind live, so F5 said 0:16 after two seconds of play. F5 now says how far back it really went, F6 always goes back, and F7 returns to live without a gap. Pausing at live also resumes where you stopped, not further ahead.
- The FM sound between stations now plays at one fixed level, whatever the volume of the station you are tuning to. Mute still silences it.
- In Edit Station, you now hear each Equalizer and Equalizer Preset choice as you move through them, if that station is playing. OK keeps it, Cancel puts back what was there.
- Delete on a station in the main window's tree now asks before removing it, from the key and from the context menu.

## 0.2.62

- Settings has a new Sources tab where you can turn each directory and network on or off. Press Space on a row, or use the buttons to change a whole list. One that is off leaves Browse and Search. Stations you saved from it keep playing.

## 0.2.61

- New directory: Online Radio Box, in Browse and Search. It offers By Genre and each region of the world, with that region's countries inside.
- Song History (Ctrl+Y) now reaches back up to a week for stations Online Radio Box follows. Older songs show how long ago they played, such as 2 d 3 h.
- Song History now finds many more stations' own lists, from any directory: StreamTheWorld stations such as .977 and Radio 538, Live365, laut.fm, AzuraCast stations such as WHFF, BBC, KEXP and Radio Paradise. These lists come first, then Online Radio Box continues from where they end.

## 0.2.60

- Station Info now looks up TuneIn stations, adding slogan, genre, location, language and homepage where TuneIn gives them. It works for stations TuneIn will not play where you are, such as MS NOW.
- BlindRadio now tells Radio Browser when one of its stations has played for ten seconds, as Radio Browser asks every app to do. Its popular lists depend on these counts. Radio Browser sees your IP address and the station. Turn it off with Report Plays to Radio Browser in Settings, General.

## 0.2.59

- A station can now have its own equalizer. In Edit Station, Equalizer is Use Global, On or Off, and On offers Equalizer Preset with every preset, your saved ones included. It switches by itself as you move between stations, so a very quiet station can always play through Loudness while the others stay as they are.
- With Loudness on, the hiss between stations no longer grows to nearly full volume. It now stays as soft as with the equalizer off.

## 0.2.58

- TuneIn stations it marks Not Supported are back in Search and Browse, and BlindRadio now tries them. Many were only marked that way because BlindRadio did not tell TuneIn which formats it plays, so Z100 - MyRadioZ and others now list and play normally.
- A TuneIn station that still will not play, such as MS NOW outside the United States, shows TuneIn's reason beside its name. Playing it says "Failed:", its name and that reason, not a generic failure.

- Pressing Enter on a station, or moving back onto one whose connection was lost, now always builds a new time-shift buffer. Before, the station played with no working buffer until you switched to another and back. When a station reconnects by itself it still keeps its buffer.

## 0.2.57

- Record in the Recording menu is now checked while a recording is in progress.
- With JAWS running, messages are now spoken by JAWS itself, so "Recording saved to..." and the rest reach it directly, wherever you are working.
- Ending a recording from the menu can hold the window for a moment. The saved message now waits until that is over and your screen reader has started reading the station tree, so the reading does not cut it off.

## 0.2.56

- 357 more stations in Networks, counting the new networks below. ABC Australia gains 33, mostly regional. NPR gains 39, including more states. Minnesota Public Radio gains The Current and the YourClassical streams, and NTS gains its 16 Infinite Mixtapes. Capital, Heart, Smooth, Classic FM, Radio X, RTÉ, RNZ, FRISKY and WHFF gain channels too. 1.FM gains 39 channels and its current channel names. 181.fm gains 6 Christmas channels. Radio Paradise gains Sanctuary, WFMU gains UbuWeb Radio and Mushroom FM gains Mushroom Pot.
- New networks: Virgin Radio UK, talkSPORT, Times Radio, Rainwave, Radio Caroline, Party Vibe, Linn Radio, K-LOVE, Air1, Pacifica, Triple M, ARN Australia, NZME and Rova in New Zealand, SABC in South Africa, American Family Radio, Moody Radio, DKFM, Soho Radio, Rinse FM, Worldwide FM, dublab, Amazing Radio, Boom Radio, 8Radio, Bloomberg Radio, C-SPAN Radio, Fox News Radio, ESPN Radio, Al Jazeera English and NHK World Radio.
- New directory: Live365, with thousands of independent stations by genre and sub-genre. Search includes it.
- KEXP, WFUV, Gold and XLtrax now show in Networks and in Search. They were hidden because each has one station.
- TuneIn in Browse now has Local Radio, Music, Talk and Sports as well as By Location. Long lists now end in Load More. They used to stop after about 26 stations. TuneIn repeats some stations between pages, and Browse lists each one once.
- Radio.net in Browse now has By Genre, By Country, By Language and By Topic.
- iHeartRadio in Browse now has By City, and its lists now include its stations in Mexico.

## 0.2.55

- Search no longer lists stations that only sound like what you typed. Radio.net, iHeartRadio and TuneIn guess at near spellings, so CKNW also brought up CKNB, KFI and others, and Internet-Radio matched the song playing now. Every word you type must now be in the station's name, genre or call sign.
- TuneIn no longer lists stations it marks Not Supported, in Search or in Browse. They only played a spoken refusal.
- Station Info and Search now know the genre and call sign of iHeartRadio stations, so searching WHTZ finds Z100.
- Messages after a menu command are no longer cut off. "Recording saved to..." and the like were said just as focus returned to the station tree, and your screen reader's reading of the tree stopped them. They now wait until that reading has started and follow it. The same goes for context menus, the tray menu, closing a dialog and coming back with Alt+Tab.

## 0.2.54

- New Open Record Folder on the Recording menu: it opens the folder your recordings go to. It can be given a key in Keyboard Shortcuts.
- New directory in Browse: IPRD, the International Public Radio Directory, replaces laut.fm. It offers By Country and By Genre, with about 24,000 stations, and Search includes it. The first time you open it or search takes a moment while the catalog downloads. Stations you already saved from laut.fm stay saved and keep playing.
- New directory in Browse: Recommended Radio Streams, a hand-picked list of underground electronic, freeform, jazz, ambient and other stations, one folder per category. It is not part of Search.
- Networks now lists English-language networks only. Removed: Radio France, Deutschlandfunk, RFI, ORF, SRF, RAI, RTVE, NPO, NRK, DR, Sveriges Radio, Yle, Radio Swiss, Sunshine Live, Power Hit Radio, Radio Record, PulsRadio, Planet Radio, 101.ru, DFM, Hit FM, Radioparty.pl, RauteMusik and TechnoBase.FM. Stations you already saved from them stay saved and keep playing. If you want something non-English added, [open an issue](https://github.com/serrebidev/blindradio/issues) and I will add it.

## 0.2.53

- Segment bookmarks in the time-shift buffer. Every song or program title a station sends now marks where that segment starts, by itself. Shift+F6 and Shift+F7, Previous Segment and Next Segment on the Playback menu, jump between them and say the title. Previous goes to the start of the segment you are in, or to the one before if it only just began. Next after the last segment goes back to live.
- Write a segment to its own file, on the Recording menu: Write Current Segment (Ctrl+F8), Write Previous Segment (Ctrl+F6), Write Next Segment (Ctrl+F7), and Write All Segments (Ctrl+F5), which writes one file for each. They follow where you are listening, say which song they took, and use your recording format. Files are named for the station, the title and when it played, and an earlier file is never overwritten. The window stays free while they write. The segment still playing is written as far as it has got.
- The title and song announcements in the buffer now follow where each song landed in the audio, not the clock, so they no longer drift by the few seconds a stream delivers in a burst when it connects. Record Each Song to Its Own File splits in the same place.

## 0.2.52

- The main window's station menu now has Delete on a station in a collection, as Custom Stations has Delete Custom Station. It takes out the copy you are on, as the Delete key does. Browse's station menu gains it too.
- New Station Info on the station menu, and it can be given a key in Keyboard Shortcuts: a read-only window with the station's name, info, genre, country, language, codec, bitrate, directory, homepage and stream URL, whichever its directory gives. Stations you cannot edit, in Browse and Search, now have a way to see all this. Radio Browser gives the most. While the station is playing, the window shows what the stream itself says, sample rate and channels included.
- The status bar now shows a bitrate for streams that declare none, such as HLS. BlindRadio works it out from the audio itself, so it appears about four seconds after a station starts.
- New File menu item, Detect Audio Stream on Page…: give it a web page's address and it lists the streams it finds on that page, in its frames and in its scripts. Each one is tried first, so only working streams are listed. They play and file like search results, and Copy Stream URL works on them. A player that only builds its address while running in a browser cannot be found, because BlindRadio reads the page without running it.

## 0.2.51

- Closing Browse no longer freezes the window for half a second and floods your screen reader. The main list is now rebuilt only when Browse changed something.
- Typing in the Filter box and editing a list are faster: rebuilding the main list takes about a quarter of the time and no longer sends your screen reader a focus move for every row.
- Arrowing through stations in the main list or Browse now gives your screen reader less to process. The window title, now playing line and status bar follow about a third of a second after you stop moving, not on every key.
- Moving onto a station with its own volume, and changing the volume, no longer writes to disk on every key. The volume is saved a second after you stop, and on exit.
- The tray icon's tooltip now shows the station and song, without the played time. It was redrawn every second while playing.

## 0.2.50

- On Windows, the AudD, ListenBrainz and last.fm tokens in `settings.json` are now encrypted for your Windows account, so the file can be copied or synced without them. Yours are converted the next time Settings saves. A copy of the file on another PC or account opens with those fields blank, so enter them again there. On macOS and Linux they stay in the file, which is now readable only by you.
- A key bound to Play in Keyboard Shortcuts now says `Nothing to play` on a folder in the main window. It played the station you were last on.
- This release's tools were updated to clear four security advisories in the `zip` library. BlindRadio itself never used the affected code.

## 0.2.49

- Ctrl+C and Ctrl+X in the main window now work only on a station. On a folder they copied or cut the station you were last on; now they say `Nothing to copy` or `Nothing to cut` and leave what you copied before alone.
- Copy Stream URL, Add to Favorites, Add to Collection and Edit Station no longer act on the station you were last on while a folder is focused in the main window. Copy Stream URL says `Nothing to copy`; the others say `No station here`. With no row focused they still use the playing station.
- Settings (Ctrl+T) now has four tabs: General, Playback, Recording and Songs, which holds song recognition and scrobbling. Focus starts on the tabs. Left and Right change tab there, Tab moves into the page, and Ctrl+Tab changes tab from anywhere in the window. OK saves all four.
- Some Settings Alt letters changed so that no two on a tab share one: Announce Actions is Alt+T, Startup List Alt+I, Ring Size Alt+Z, Recordings Folder Alt+O, Only When the Stream Gives No Title Alt+I and last.fm Secret Alt+L.
- The Help menu gains View Changelog, which opens the release history in your browser and works offline, and Go to BlindRadio GitHub Page. Both can be given a key in Keyboard Shortcuts.
- User Guide now says `Could not open the browser` when no browser opens. It did nothing.

## 0.2.48

- Shift+Up and Shift+Down now move a collection in Browse, and an open one keeps its rows as it moves. Custom Stations stays first among them. The four top rows of Browse move too, Collections and Custom Stations included.
- Delete on a collection in Browse now asks and deletes it, as Delete on its context menu did. It answered "Only your own stations can be deleted". The context menu's question now also says how many stations go with it. A folder's context menu in Browse gains Delete.
- Shift+Up and Shift+Down now work in the main window: a station among the stations on its level, a folder among its folders, and in All Playlists a collection among the collections. Alt+Up and Alt+Down work too. They do nothing while the Filter box has text in it.
- Folders can now be renamed and deleted in the main window, so a folder left empty by a paste no longer sends you to Browse. Delete asks first and says how many stations go with it; Rename and Delete are also on the folder's context menu. In All Playlists the same goes for a collection, except Custom Stations.
- All Playlists now lists collections in the order you give them, with Custom Stations first, instead of by name. Export All Playlists writes them in the same order.
- Delete on a station in the main window now takes out the copy you are on when the same station is filed in several folders. It took out the first one. It also works in All Playlists, where it did nothing.

## 0.2.47

- AC-3 and E-AC-3 audio now plays, in TS channels (DVB and ATSC), in HLS and as a bare stream. Surround is mixed down to stereo with the centre channel kept, so dialogue stays. No extra files are installed: the decoder is compiled into BlindRadio.
- AAC in LATM, as DVB-T2 and DVB-S2 channels carry it, now plays.
- HLS in fragmented MP4 now plays, as does HLS encrypted with AES-128 and segments cut from one file with byte ranges. DRM, such as SAMPLE-AES or FairPlay, still answers `Unsupported format`.
- Opus and Vorbis streams now show the song title. They send it in their own tags, not the ICY titles BlindRadio was looking for.
- Settings (Ctrl+T) has three new buffer settings: Start buffer, Refill limit after dropouts and HLS segments behind live. Each is a text field you can type any number into, with a slider that moves with it. The start buffer and refill limit keep their old defaults of 1000 milliseconds and 15 seconds.
- Live HLS now starts two segments behind the newest instead of three, about one segment closer to live, and looks for the next segment twice as often once it is due.
- Moving onto a warm HLS station keeps enough audio to reach its next segment, instead of cutting to one second and then dropping out.
- Shift+Up and Shift+Down now move stations in Favorites, as they do in Browse. They only moved folders there before.
- Add Custom Station and Edit Station have a Name Automatically button. It asks the URL for the station's own name: the ICY or Icecast name, the title in its playlist, or the service name in a TS channel, as DVBViewer does. A stream that names itself nowhere gets a name made from its address. The name stays an ordinary field you can edit.

## 0.2.46

- Paste in the main window now lands in the folder of the row you are on. When the same station was filed in more than one folder, BlindRadio used the first folder holding it and could answer "already in". Cut takes out the copy you are on for the same reason.
- With a station filed in several folders, moving onto a later copy in the main window no longer throws focus back to the first one. Pasting on a folder row also keeps focus on that folder instead of jumping to the top of the list.
- Stations that failed at once with "Not a stream" now play when their server compresses its playlist, as RTP's Antena 1 does (https://radioip.rtp.pt/liveradio/antena180a/chunklist.m3u8). DVBViewer read these; BlindRadio could not.
- BlindRadio now plays raw MPEG-TS streams, including `.ts` links from IPTV panels, taking the audio from channels that also carry video. The audio has to be AAC, MP3 or MP2. A channel with only video or other audio answers `Unsupported format` straight away.
- Streams and playlists sent with a generic type, such as `application/octet-stream`, or no type at all are now judged by their contents instead of being refused as "Not a stream".
- HLS channels that keep their audio in a separate playlist now play.
- M3U Plus playlists import in full. `.m3u_plus` files are offered in the import window, `tvg-name` fills in a missing channel name, `#EXTGRP` makes a folder, and Kodi-style headers after a `|` in an address are dropped so the address still works. A channel that needs those headers may refuse the connection.
- Space now plays or pauses on any list, on a tree and on the volume slider. In a text box it still types a space, and on a button or check box it still presses it. It replaces Space starting the focused row in Browse; Enter still does that. You can change or clear it in Keyboard Shortcuts, where it is the alternate Play/Pause row.
- With Play on Move off, Play/Pause now plays the row focus lands on at startup instead of answering "Nothing to play".

## 0.2.45

- After playing a station with no row in the main window's list, Add to Favorites, Copy Stream URL and Edit Station act on that station until you move, not on the row that still has focus. In 0.2.44 Add to Favorites could remove the row's station from your favorites instead.
- Move to Playing Station now says "The playing station is in no list" when there is no row to move to.
- Closing Edit Station opened from Browse, Collections or Favorites no longer makes the app think the window underneath has closed. That brought back slow arrow keys and could misplace the next paste.
- Copy, Cut and Paste in Collections and Favorites work on the row you land on when the window opens, and follow it after a folder is moved or deleted. Pasting keeps focus on the same copy of a station filed in several folders.
- Delete Custom Station no longer starts a station when another copy keeps the same row in the list.
- Edit Station also updates Recently Played and the station played at startup, so an edited name or info no longer comes back.
- A station played through a global hotkey while Keyboard Shortcuts or Global Hotkeys is open gets its row when that window closes.

## 0.2.44

- With "Play a station as I move to it" on, deleting a station now plays the station that takes its place in the two cases 0.2.42 missed: the Delete key in Browse, and deleting the last row of the main window's list. The Delete item on the main window's context menu does the same.
- The Edit Station dialog has a new Info field. It holds the text after the name in the lists, such as "Operavore, The United States Of America, AAC+ 47k" on a station added from Search. Edit it or clear it, and the long name goes away everywhere the station appears.
- A station with no row in the main window's list no longer gets a temporary one. An opened URL, a search result or a favorite no list holds plays without touching the tree, and focus stays on the row you were on.
- Moving through Search results, Favorites and other windows is fast again, and Search keeps focus when its first result starts to play. Each arrow key was rebuilding the hidden main window's list. It now catches up once, when the window closes.
- A stream that has played now gets fifteen seconds of silence before it counts as lost, not five. A slow or bursty connection, such as KCRW's 256 kbps AAC on some networks, could drop and reconnect over and over while a longer wait would have carried on.
- Copy, Cut and Paste by keyboard now follow the focused row in the Collections and Favorites windows, as they do in Browse, and those windows update at once. A paste there used to land in the main window's list, answering "already in Custom Stations".
- "Already in" now names the list the station is in, not the collection around it, so a playlist imported into Custom Stations reads as itself.

## 0.2.43

- Fixed empty Original stream format recordings when listening behind live in the time-shift buffer, including Record Each Song to Its Own File. Buffered recording now retains and writes the original compressed audio at the listening position.
- Original recordings of header-dependent streams such as Ogg and FLAC use WAV so seeking and song splits produce playable files. MPEG audio and ADTS AAC keep their original compressed format.
- Fixed copying and cutting stations from a Browse subfolder into its parent playlist, including stations filed in more than one list. Paste follows the selected destination, and Browse updates immediately after a paste.
- Added Stations > Move to Playing Station to reveal and focus the playing station without restarting it. Previous Station, Next Station, favorite shortcuts, and playback from Browse, Favorites and Recently Played also select the playing station in the main tree.
- Added File > Export Current Playlist, Export All Playlists and Export Favorites, preserving playlist folders in the exported M3U.
- Fixed a crash when a second instance raises the existing window while its timer is handling an event.

## 0.2.42

- With "Play a station as I move to it" on, deleting a station now plays the station that takes its place. Focus landed on the next station after a deletion, but nothing played it — you had to press Enter. The station that gains focus after a deletion is treated like moving onto it, in the main window's list, in Browse, and in Favorites. With the setting off, nothing changes, and deleting a station you are not standing on never disturbs what is playing.

## 0.2.41

- Slow-opening streams are no longer announced as failed while they are still opening. The first connect had five seconds for everything — connect, redirects, playlist fetches and the first audio — and a first connect that runs out of time fails at once, with no retry. A chained HLS address like MediaWorks' (a redirect, a master playlist, a variant playlist, then the first segment, each hop slow on a far server) could pass five seconds on a healthy stream. The start budget is now fifteen seconds. A stream that has played and then drops still reconnects under the existing sixty-second window.

## 0.2.40

- Fixed pasting into a parent directory for real this time. The 0.2.39 fix put Paste Station on the playlist and folder menus, but Paste itself (Ctrl+V) still only knew the selected station: from a folder row in Browse it pasted into the main list's root instead of the row you were standing on — answering "already in Custom Stations" when the station already lived there — and from a folder row in the main window, with the main list set to All Lists, it answered "Nowhere to paste". Paste now follows the focused row in both windows: on a folder or playlist row it lands at that row's level; on a station it lands beside it, as before.
- Cutting a station and pasting it where a copy of it already lives now completes the move instead of refusing with "already in": the copy in the source folder is removed and the station stays filed at the destination. A copy-paste into a list that already holds the station still says so, and a cut pasted back into its own list still has nowhere to go.

## 0.2.39

- Stations can now live at a playlist's own level: Paste Station appears on playlist and folder rows in Browse and in the main window's list, and pastes the copied or cut station at that row's level, beside its folders — so a station can be moved out of a subfolder directly into the parent playlist, with no subfolder having to hold it. (A new station's destination picker already offered every playlist itself.)
- Playlist and folder names now show how many stations live under them — "English 24", "Misc 15" — in Browse and in the main window's list. Remote directories stay bare: their count is not known until every level under them has loaded.
- Fixed the remaining Alt+Tab case where focus landed on the menu bar and NVDA read "File Stations Playback Recording Help": the activation fix in 0.2.35 missed some return paths, so the app now also watches for the frame itself holding the keyboard focus while no dialog or menu is open, and hands the focus to the station tree.

## 0.2.38

- Fixed Browse showing the old order after a reorder: moving a station (or a folder) updated the library and the main window, but the Browse tree itself kept displaying the previous order until the dialog was closed and reopened. The tree now rebuilds a level whose order changed, so a move shows where the row landed the moment it happens.

## 0.2.37

- Stations in Browse can now be reordered: Shift+Up and Shift+Down (Alt+Up and Alt+Down too) move the focused station one step among the stations in its playlist or subfolder, the same gesture folders already had. A pasted or newly created station lands last in its folder; now it can be walked up into place. The library itself is reordered, so the main window's list takes the new order as soon as Browse closes. Reported by Amir Soleimani.

## 0.2.36

- Closing BlindRadio while a recording is in progress now asks first — the window's close button, Alt+F4, and File, Exit all confirm before ending the recording, and what has been recorded so far is saved when you go ahead. A paused recording counts. Hiding to the tray never asks: the recording continues there.

## 0.2.35

- Fixed playlist exports flattening folders: exporting a folder from Browse gathered every station under it into one group named after the folder itself, so a playlist's subdirectories (the Germany directory's 37 of them) collapsed into a single "Germany" folder on re-import. Browse folder exports now carry the whole subtree with its groups intact, local folders export from the library itself, and the main window's folder export no longer wraps a folder in itself either. Exporting and re-importing rebuilds the same folders.
- Returning to the window with Alt+Tab now lands on the station tree. Windows sometimes handed the returning focus to the menu bar, which read as one line of menu names; the window puts focus back on the tree whenever it becomes active again (a dialog keeps its own focus).
- Browse can create folders: New Folder is in the context menu on a collection, on Custom Stations, and inside any local folder — a home for stations someone suggests for a playlist, without leaving Browse.
- Edit Station and Station Settings are one dialog: Edit Station now holds the name, the stream URL, and the per-station buffer minutes, song recognition and scrobbling. The separate Station Settings menu item and shortcut are gone; the Edit Station shortcut opens the merged dialog.

## 0.2.34

- iHeart stations whose stream metadata arrives as a comma-separated attribute record (WLIT 93.9 Lite FM in Chicago is one) now show and announce the song properly: the title is read from the record's own title and artist fields instead of the whole record arriving as one unreadable blob, so Announce Song Titles works on them like everywhere else.

## 0.2.33

- Song recognition no longer fails silently: it uses AudD, not Shazam, and needs a free token from audd.io — the README and the user guide now say so and give the steps. Saving Settings with recognition switched on, AudD chosen, and no token now speaks a warning that nothing will be recognized until a token is added, instead of leaving it looking broken.

## 0.2.32

- Scheduled entries can be alarms: in Scheduled Recordings, set an entry's Kind to Alarm and it plays its station at the start time instead of recording it — wake up to a station. Daily and Weekly alarms repeat; a Once alarm switches itself off after firing, and an alarm missed by more than five minutes (the app was closed) is skipped, not replayed late.
- Volume is remembered per station: the level you set while a station plays becomes that station's level, applied automatically whenever it starts — the loud station stays turned down, the quiet one stays up. Stations without a remembered level use the current volume.

## 0.2.31

- Ctrl+M now says what it did: `Muted` or `Unmuted`, under the same Announce Actions setting as every other action. The mute button says it too.
- Recording has its own menu. Record, Pause Recording, Record Each Song to Its Own File, Write Buffer and Scheduled Recordings moved out of Playback into a new Recording menu, where a keyboard or screen reader user looks for them first. The Keyboard Shortcuts editor gained the same Recording tab.
- Stations holds the station items: Previous Station and Next Station moved there from Playback, and so did Add to Favorites and Add to Collection, since they act on stations rather than on playback.
- Record Each Song Separately is now Record Each Song to Its Own File, and its announcement says the same: screen readers mangled the old word where the checked item was read out.
- Song-split recording no longer cuts songs in the wrong place when you listen back in the time-shift buffer. It split at the live song change while the recorder was writing the buffer's audio, which was some seconds behind, so every file began mid-song, ended mid-song, or held one song in pieces. The split now follows the song at the buffer's position — the audio actually being recorded — and the file names follow it too, from the first file a recording opens.

## 0.2.30

- Enter now always plays the focused station, even the one already playing: it reconnects fresh, so after editing a station's URL you can press Enter on it and hear the new address at once, without switching to another station and back. Moving focus onto the playing station still leaves it sounding untouched.

## 0.2.29

- Folders inside playlists can now be renamed in Browse: the context menu on a folder in Collections or Custom Stations offers Rename next to Export, at any depth — including an imported playlist's subfolders and folders inside them. Remote directories' folders stay the directory's own.

## 0.2.28

- The BlindRadio Directory shows each playlist's subdirectories again: a playlist's groups (BBC in UK, NPR, AZPM and the rest in Public Radio) are folders in Browse, instead of every station lumped into one flat list.
- Exporting a collection or folder to M3U now writes each folder's full path as the group, and importing splits it back into nested folders, so subfolders inside subfolders survive an export and re-import instead of flattening to the innermost folder's name. A folder name holding a bare slash (Top 40/Pop) still stays a single folder.

## 0.2.27

- Song changes are now announced while listening back in the time-shift buffer, not only live: as playback moves forward through the buffer and the next song starts, it is spoken under the same Announce Song Titles setting as live announcements.

## 0.2.26

- Browse gains the BlindRadio Directory: community-submitted stations kept in the BlindRadio-directory repository on GitHub, starting with 14 playlists and 1,270 stations (UK, Germany, France, Canada, Public Radio, Classical Music, and more). Each playlist is a folder; stations keep their names and groups, and the app reads the repository live, so stations merged there appear in Browse without an app update. Submissions are welcome by pull request to that repository.

## 0.2.25

- The status bar no longer repeats the station and song: it carries the played time, live or how far behind live, the recording state and the stream format. The station and song stay in the title and the now-playing line, where they no longer crowd the bar.
- The title's song now follows the time-shift buffer: shifted back, the title and the now-playing line name the song playing at the buffer's position instead of the live one, and going live names the live song again.
- Every in-app keyboard shortcut is now customizable: File, Keyboard Shortcuts lists all of them under Playback, Stations and App — rebind any key, clear one, or give a key to an action that had none, such as Write Buffer or Scheduled Recordings. The menus print the keys as they are now set, and the defaults are unchanged.
- The FM sound — the soft hiss between stations while tuning — is now optional: Settings has FM Sound Between Stations, on by default. Off, tuning gaps are silent.

## 0.2.24

- New Settings checkbox: Expand All Directories in the Main Window. Until now the main window's tree always opened with every directory and subdirectory expanded, which can make a long list hard to walk; untick it and directories start collapsed — the list opens as its top-level directories, and you open the ones you want. A directory you open or close by hand keeps its state across restarts of the list either way, and changing the setting takes effect as soon as Settings closes.
- Changing stations no longer ends a recording silently. A recording in progress now brings up a confirmation first — it names that the recording stops and what is recorded so far is saved — and only Yes changes the station; No leaves the station and the recording alone. This covers arrow-key tuning with Play On Move, Enter, Next/Previous, and picking a station anywhere else.
- While a recording is in progress, the window title ends with the word Recording, so the taskbar and Alt+Tab show it.

## 0.2.23

- The window title is back as it was. 0.2.22 put the whole status line in the title as a workaround; that was the wrong trade, and it is reverted — the title shows the app name, station and song, nothing more.
- The status bar itself is fixed instead. On Windows the bar lives in its own thin window along the main window's bottom edge so NVDA's status line command (NVDA+End) can find it — but that window still had a frame border, and NVDA's search starts at the window's bottom-left corner, which landed on the border (a plain pane) instead of the bar. The window is now truly borderless: the bar fills it corner to corner, and NVDA+End reads the line — station, song, played time, and the stream's format — the way it does in any other app.

## 0.2.22

- Fixed a serious startup regression from 0.2.21: as the app started, NVDA announced "Pane" and the station tree would not take focus until Alt+Tab was pressed a few times, and the same theft could recur later. The status strip was being shown through wx as well as directly, and showing a window through wx activates it — the nameless strip kept taking the foreground from the main window. The strip is shown directly again, as in 0.2.20, which never activates it.
- The status line moved into the window title. NVDA+End still found no status line on the machines that matter, so the title now carries the whole line — station, song, how long it has played, and the stream's format (bit and sample rates) — refreshed every second. A title command or Alt+Tab reads it all.
- The main window exports folders. Its tree's menu on a folder — a directory, a subdirectory, or a whole collection in the All Lists view — now offers Export, writing everything under it as an M3U with groups, like Browse does; before, only single stations could be exported there.
- Browse reorders show in the main window at once. Reordering folders in Browse wrote the new order but the main window's list kept the old one until a restart; it rebuilds as soon as Browse closes.
- Choosing an equalizer preset no longer checks Enabled by itself. Arrowing through presets to compare them kept switching the equalizer on; Enabled is manual now, for built-in and saved presets alike. Moving a band by hand still enables it.
- New recording mode: Record Each Song Separately (Playback menu). On a station that names its songs, recording closes the current file when a new song starts and opens one named for the station and the song, until you stop. Stations that name no songs record as one file, as usual; a paused recording stays paused across a song change.

## 0.2.21

- Loudness now levels the volume itself, not only the tone. A fixed EQ curve cannot make a quiet station louder than a loud one — that takes riding the gain, which is what FFmpeg's dynaudnorm filter and mpv's normalization do (the audited consensus for live sources). The Loudness preset now does the same: quiet stations are brought up toward the level of loud ones, hot stations are eased down, a soft ceiling keeps it from ever clipping, and near-silence is left alone so dead air is not amplified. Leaving the preset eases the gain back to normal.
- Browse deletes folders now. A station inside an imported playlist could be deleted, but its folder could not — Delete on a folder in Collections or Custom Stations answered "Only your own stations can be deleted", even for an empty folder left behind. Delete on such a folder removes it, after asking and naming how many stations go with it. A directory's folders still cannot be deleted; they are not yours.
- A station found in Search or Browse can be filed into an imported playlist's folder. Add to Collection Folder skipped Custom Stations entirely, and imported playlists live there as folders — so a WCMU-HD2 found by searching could only go to Favorites, another collection, or a new one, while Ctrl+N's picker offered the right folder all along. The station menu now offers every folder of every collection, Custom Stations included, in Search, Browse and the main window alike.
- NVDA+End gets a second route to the status line. The 0.2.20 thin-window status bar covers the spot NVDA samples, but the press still did not land on some machines: when NVDA asks UI Automation for the element at that corner instead, the answer came from the bar's system provider, with the bar's own small rectangle — the same geometry that failed inside the frame. The bar now answers UIA itself: control type Status Bar, its name the current status line, its rectangle the window's whole bottom edge. The strip also shows itself through wx as well as directly, so the toolkit's state and the window's agree. On Linux the status bar is a standard part of the main window, verified in the accessibility tree: a status bar holding the current line, where Orca looks for it.
- Found by the same audit, on Linux: starting the app dropped its first dial selection to a toolkit-caught panic — a tree rebuild's guard fell one step before the focus select it was meant to cover, and the selection event fired into the rebuild. The guard now covers the select; startup runs clean.

## 0.2.20

- The global hot keys are heard everywhere now. Announcements travelled through a live region tied to the BlindRadio window, so Ctrl+Alt+Shift+N spoke only while BlindRadio was the front app. With NVDA running, BlindRadio now speaks through NVDA itself — NVDA's controller client ships beside the app — so the announcement reaches you in any app. Other screen readers, and NVDA not running, keep the live region as before.
- NVDA+End reads the status line, no add-on needed. NVDA looks at the main window's bottom-left corner and climbs from there to a status bar, but a bar inside the window is clipped to the window's client area and the corner is border, outside it, so the climb never reached the bar (two in-app attempts in 0.2.16 and 0.2.19 could not change that geometry). On Windows the status bar now lives in its own thin window glued along the main window's bottom edge, border included, following it as it moves and resizes — the corner NVDA checks is the bar itself.
- Pasting a URL into Edit Station works again, for real this time: 0.2.19 stopped the main window's keys, but the dialog windows run their own key handling, and that still turned Ctrl+V into Paste Station and answered "Nothing to paste". Ctrl+C, Ctrl+X and Ctrl+V now edit text whenever a text field has the focus, in dialogs and in the main window's Filter box alike; on a station they keep their station meanings.
- The equalizer remembers your tuning. Moving a band by hand, trying another preset and coming back to Custom used to lose the hand-set bands — the named preset had overwritten them. Custom keeps its own bands now and restores them. You can also save a tuning under your own name (Name, then Save Preset) and delete it later; saved presets join the Preset list.
- The equalizer is easier to feel and finer to move. The bands reach further into the middle of the spectrum, so presets are plainly heard on speakers and not only on headphones, and Loudness is stronger (the DVBViewer kind of difference). New presets: Bass and Treble, Clarity and Laptop Speakers. PageUp and PageDown now move a slider by one dB like the arrow keys, instead of the native five.
- Browse moves folders now: Shift+Up and Shift+Down (Alt+Up and Alt+Down too) move a folder among the folders on its level. The 0.2.19 notes said Browse had this; it had only landed in Collections and Favorites. A folder in Collections or Custom Stations moves in your library itself; a directory folder keeps the order you give it between sessions.
- Streema cannot be added as a directory. The audit: Streema offers no public API (its own About page says an API is only being considered), its station pages expose no stream address to build on, and GitHub holds no usable client — the 20 most recent and 20 most starred results for "streema" and "streema api" are unrelated projects and stale scrapers, the one unofficial API a 2024 page scraper. There is nothing sound to build a source on; if Streema ships an API it can be picked up then.

## 0.2.19

- MP2 stations play now. Some stations (BBC Radio 4's 2.onj.me address is one) send MPEG Layer II audio while calling it MP3; the decoder had no Layer II codec and refused them outright, though VLC and DVBViewer play them. A Layer II decoder is in, so those stations tune and play like any other.
- The equalizer works now. Choosing a preset or moving a band changed the numbers but never switched the equalizer on — it starts off, and nothing in the dialog turned it on, so every preset sounded the same as off. Choosing a preset or moving a band now enables it and is heard at once; the Enabled box is the off switch.
- Pasting a URL into the Edit Station or Add Custom Station window works again: the main window's Paste Station shortcut was taking Ctrl+V before the focused field could see it and answering "Nothing to paste". While a dialog is open, Ctrl+V belongs to the dialog.
- The main window's filter has its own box now. Typing in the tree used to narrow the list, which took away moving through stations by their first letters; the Filter box above the tree filters, and letters in the tree jump by first letter again.
- The tree no longer reads the category name twice: a station whose playlist group matched its folder was announced with the folder's name attached ("Houston Public Media News - Houston Public Media"). The repeated name is dropped; other stations keep their extra information.
- Previous and Next Station count the station that starts with the app: the remembered startup station was played without joining the Alt+Left / Alt+Right history, so the first step back said "No earlier station". It is in the history now.
- Add Custom Station asks where the station goes: Custom Stations, any collection or folder, or Favorites and its folders, instead of always Custom Stations. A station's context menu (in Browse too) gains Add to Collection Folder, filing it straight into a collection's folder.
- Shift+Up and Shift+Down move a folder among its siblings in the Collections and Favorites windows (Alt+Up and Alt+Down keep working too).
- Playback, Write Buffer writes the whole time-shift buffer to a WAV file in the recordings folder, named like a recording — everything the station played since you tuned in, up to the buffer's length.
- Scheduled recordings: Playback, Scheduled Recordings. A station, a start time, a duration, and Once, Daily or Weekly; the app tunes in and records on its own and stops when the duration is up. A recording already running by hand is never interrupted, and a Once entry switches itself off after it runs.
- The status line gets another fix: the status bar is raised above the panel created after it, so NVDA's status line command (NVDA+End), which hit-tests the frame's bottom edge, meets the bar first instead of the panel. Announce Now Playing (Ctrl+Alt+Shift+N) now speaks the whole status line too, so the information is always reachable by that chord whatever the pixel hunt does.

## 0.2.18

- NCBI Live is removed from Networks: its stream is dead at the source. radio.co's own directory API reports the station no longer exists, and Vision Ireland's live page still points at that same dead address, so their own web player is down the same way. It was added in 0.2.15 from their published address but could never be verified from our side; it does not play anywhere. If Vision Ireland restores the stream it can come straight back.

## 0.2.17

- New directory: Radio.net, with Top Stations (the service's Top 100), Local Stations (stations near you, by the service's reckoning), and its catalogue folded into the app-wide search. Station entries arrive with their streams ready to play, no extra lookup.
- Radio Browser completes its set: By Language and By State join By Genre and By Country, and Top Stations lists the most-voted stations. State lists keep exact matches only, as under each country.

## 0.2.16

- The startup list keeps its folders now: the main window's station list is a tree, like the Collections window. With Startup List set to All Playlists, each playlist is a folder with its own directories inside; a single playlist shows its directories directly. Stations stay in the same order as before, so tuning, Play On Move, Delete and the context menus work as they always did, and folders you collapse stay collapsed.
- Filter the tree by typing: with the station tree focused, typing letters narrows it to the stations whose names contain them — the message line shows the filter and how many stations match. Backspace edits the filter, Escape clears it and brings the whole tree back.
- Previous Station and Next Station (Alt+Left and Alt+Right, in the Playback menu): step back and forward through the stations you have played this session, without hunting through lists — like a browser's back and forward buttons for stations.
- The status bar reads with NVDA's status line command (NVDA+End) now: the frame builds its own status bar the standard way instead of attaching one afterwards, which the command could not find. It shows the station, how long it has played, the live or behind-live position, and the stream's format, bit rate and sample rate.
- New setting: Check for Updates automatically (Settings, on by default). The daily update check already ran on its own; now you can turn it off and rely on Help, Check for Updates instead.
- Announce Now Playing (Ctrl+Alt+Shift+N) speaks again when Announce Actions is off: it was routed through the action announcements that the 0.2.14 setting gates, but the chord is an on-demand request and is documented to speak whatever the announcement settings say — now it does.

## 0.2.15

- Three new networks, all broadcasting for blind and print-disabled listeners: ACB Media, with all ten channels from the American Council of the Blind (Mainstream, Treasure Trove, Cafe, Community, the live and special event channels, and the convention channels); Mushroom FM (Mushroom FM and Mushroom Escape, old-time radio drama and comedy); and Vision Australia Radio (Adelaide, Albury, Bendigo, Geelong, Melbourne, Perth and Shepparton). Every stream was checked live before shipping, and the Networks list stays in alphabetical order.
- Six more blind-community radio services, each listed under Networks with its one station: NFBRN (the National Federation of the Blind's 24/7 radio network), NCBI Live (Vision Ireland), WRBH (New Orleans' reading radio), AMI Audio (Accessible Media Inc., Canada), RNIB Connect Radio (UK) and Double Tap Live (blind tech talk around the clock). Single-station networks normally stay unlisted; these named services are flagged to stay visible.

## 0.2.14

- New setting: Announce Actions (Settings, on by default). Turn it off and BlindRadio stops speaking the confirmations for actions and keyboard shortcuts — "Recording started", "Copied", position reports and the like. The message line still shows them, and failures still speak. Song titles and connection status keep their own separate settings.

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
