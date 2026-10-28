# BlindRadio NVDA test plan

Build `cargo build -p blindradio --release`, run `target\release\blindradio.exe` with NVDA on.
Expected speech is quoted. Numbered steps are one pass; note the step number with any failure.

## Start and main window

1. First run: the window is `BlindRadio`; focus lands on `Stations` once the starter list
   arrives; the first station plays; no speech until the audio. The list reads
   `Stations, Name <station>, Info <group>, Directory Custom`.
2. Second run with Play Last Station at Startup off: focus on `Stations`, nothing plays, title
   `BlindRadio`. Down plays the station under focus.
3. With Play Last Station at Startup on: that station plays at once.
4. Empty SerrebiRadio list: focus is on `Play`.
5. Tab order: `Stations`, `Play`, `Stop`, `Mute`, `Favorites`, `Volume`, message line.
6. `Play` reads `Play`; playing, it reads `Pause`. `Mute` reads `Mute`, then `Unmute`.
7. Volume slider: Up raises by 5 and reads the value; PageUp by 10; Down and PageDown lower.
8. Now playing line shows `Stopped`, then `<station>`, then `<song> - <station>`; the title
   matches.

## Menus

9. Alt+F, Alt+S, Alt+P, Alt+H open on their first item.
10. File: the four check items tick and stay ticked; Audio Device opens on `Default`, the
    active device is checked.
11. Stations: every item reads with its shortcut (`Search… Ctrl+F`, and so on).
12. Playback: `Add to Favorites Ctrl+D` reads `Remove from Favorites Ctrl+D` while a favorite
    is focused or playing.
13. Help: `About BlindRadio` opens the about box; Escape closes it.

## Dial tuning and lists

14. In `Stations`, one Down plays the next station at once, no title speech.
15. A letter jumps to the next name starting with it and never presses a button.
16. Enter plays the focused station.
17. Delete takes the station out of the list it sits in; the playing station plays on.
18. Applications key on a station: `Play`, `Add to Favorites`/`Remove from Favorites`,
    `Copy Stream URL`, `Add to Collection`, and `Delete Custom Station` only for a station in
    Custom Stations. The menu opens on its first item.
19. Applications key on a category (Browse root): `Nothing to do here`, no menu.
20. Ctrl+U: `Copied`, and the URL pastes elsewhere. Nothing focused or playing: `Nothing to
    copy`.

## Search (Ctrl+F)

21. Name, Genre, Country, Language; each box starts on `Any`, not `blank`.
22. Tab to `Search`, Enter: `Searching`, results replace the list, the first result plays.
23. A letter in the results jumps; Down tunes. `Add to Favorites` flips to `Remove from
    Favorites` on an already-favorite station.
24. Ctrl+B inside Search closes it and opens Favorites; Ctrl+F returns to the first control.
25. No results: `No results`.

## Browse (Ctrl+G)

26. `Stations` tree: `Radio Browser`, `TuneIn`, `iHeartRadio`, `Networks`, `Collections`,
    `Custom Stations`.
27. Right opens a folder; the placeholder reads `Loading` and focus does not move while it
    loads; Down moves past it freely.
28. Enter on `Load More` loads the next page and focus lands on its first new station.
29. Home and End stay on the level; Ctrl+Home and Ctrl+End go to the whole tree's ends.
30. Focus on a station tunes; Delete on a directory station says `Only your own stations can
    be deleted.`
31. A failed load says `Failed: <directory>` once and leaves the folder closed: Right retries.
32. Escape, reopen: focus is back on the last station, its folders open.

## Favorites (Ctrl+B)

33. `Favorites`, buttons `Remove`, `Move Up`, `Move Down`, `Export…`, `Close`.
34. Opening plays the first favorite; Down tunes through them.
35. Delete removes and focus stays on the neighbour; the playing station plays on.
36. Alt+Up and Alt+Down reorder; focus follows the station; the sound does not change.
37. `Export…` defaults to `Favorites.m3u`, then `Exported <n>`.

## Recently Played (Ctrl+H)

38. `Recently played`, newest first; `Add to Favorites` acts on the focused station.

## Custom station (Ctrl+N)

39. `Name`, `Stream URL`; OK is default, Enter submits, Escape cancels.
40. `ftp://x` keeps the window open and says `Not a stream URL`.
41. A good URL says `Added <name>` and the station is in `Stations`.

## Collections (Ctrl+L)

42. `Collections` tree: `Custom Stations` first, then each collection with its stations.
43. Enter on a collection opens it; Enter on a station plays it; focus tunes.
44. F2 renames; Delete asks `Delete <name>?` and deletes on Yes.
45. On `Custom Stations`: F2 and Delete say `Custom Stations cannot be changed`.
46. Delete on a station removes it from that collection; on `Custom Stations` it deletes it.
47. Alt+Up and Alt+Down move a station in its collection, or a collection among collections;
    `Custom Stations` stays first.
48. Applications key on a collection: `Rename`, `Delete`, `Add All to Collection` (every other
    collection); on `Custom Stations`: `Add All to Collection` only.
49. Add All: `Added <n> to <collection>`.
50. `Export…` defaults to `<collection>.m3u`, then `Exported <n>`.

## Add to Collection (Ctrl+Shift+L and the station menu)

51. A popup of collections, ticked where the station already is, then `New Collection…`.
52. Choosing a ticked collection says `Removed from <collection>`; an unticked one
    `Added to <collection>`.
53. `New Collection…` asks `Collection name` and adds the station.
54. With nothing selected or playing: `Nothing playing`.

## Import (Ctrl+I, Ctrl+Shift+I)

55. The file dialog filters `Playlists (*.m3u;*.m3u8;*.pls)`.
56. After a file: `Added <n>. Had <n>. Skipped <n>.` Try
    `crates\blindradio-core\tests\fixtures\import\*.m3u`; as a collection, the file name is the
    collection name.

## Song History (Ctrl+Y)

57. Not playing: `Nothing playing`.
58. Playing: heading `Heard since you tuned in`, list `Songs` with one column; published
    history replaces it with `Station history` and a `Played` column (`Just now`, `5 min`).
59. `Copy` and Enter say `Copied`; the song pastes elsewhere.

## Sleep timer (Ctrl+1 to Ctrl+6, Ctrl+0)

60. Each says the timer text; Off says it is off. Stopping and starting stations does not reset
    it. When it is due and playing: `Sleep timer: stopping`. Stopped: silence.

## Equalizer (Ctrl+E)

61. `Preset` lists Flat, Bass Boost, Treble Boost, Loudness, Voice, Warm, Custom.
62. Choosing a preset moves the sliders and says `Bass <n> dB, Mid <n> dB, Treble <n> dB`.
63. Bass, Mid and Treble sliders run -12 to 12; Up and Right raise by one, Down and Left lower,
    Home and End hit the ends; the value is audible as the slider's position.
64. A hand move changes the preset to Custom.
65. `Enabled` toggles; every change is heard at once and kept after a restart.

## Devices and settings

66. Audio Device: choosing a device plays through it and is remembered.
67. Unplug it while playing: `Device lost. Using default.` and the submenu checks `Default`.
68. Plug it back: `Using <name>` and the submenu checks it.
69. The four File check items survive a restart.
70. Exit saves the station and volume; the next start honours both.

## App keys in every window

71. Ctrl+P, Ctrl+S, Ctrl+M, Ctrl+Up, Ctrl+Down, Ctrl+D act in the main window and inside every
    dialog, including while typing in a text box, and plain letters still type.
72. Ctrl+F, Ctrl+G, Ctrl+B, Ctrl+H, Ctrl+L, Ctrl+N close the open window and open that one; only
    one window is ever open.
73. Escape closes any window; F1 opens the user guide (plan 5).
