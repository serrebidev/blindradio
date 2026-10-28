//! Every user-visible string, in one place for translation later. Terse on purpose.

pub const APP_NAME: &str = "BlindRadio";
/// Prefix: `Failed: <name>`.
pub const FAILED: &str = "Failed: ";
pub const RECONNECTING: &str = "Connection lost. Reconnecting.";
pub const RECONNECTED: &str = "Reconnected";
pub const STOPPED: &str = "Stopped";
pub const NOTHING_TO_PLAY: &str = "Nothing to play";
pub const NOTHING_PLAYING: &str = "Nothing playing";
/// Prefix: `No favorite <n>`.
pub const NO_FAVORITE: &str = "No favorite ";
pub const ADDED_TO_FAVORITES: &str = "Added to favorites";
pub const REMOVED_FROM_FAVORITES: &str = "Removed from favorites";
pub const SLEEP_STOPPING: &str = "Sleep timer: stopping";
pub const DEVICE_LOST: &str = "Device lost. Using default.";
/// Prefix: `Using <device>`.
pub const USING: &str = "Using ";
pub const DEFAULT_DEVICE: &str = "Default";
pub const NOT_SAVED: &str = "Not saved";
pub const STARTER_NOT_LOADED: &str = "Starter stations not loaded";
pub const ABOUT_DESCRIPTION: &str = "Screen-reader-first internet radio.";
pub const WEBSITE: &str = "https://github.com/serrebidev/blindradio";

// Main window. Its buttons carry no Alt keys (YARSP): their Ctrl keys work everywhere.
pub const PLAY: &str = "Play";
pub const PAUSE: &str = "Pause";
pub const STOP: &str = "Stop";
pub const MUTE: &str = "Mute";
pub const UNMUTE: &str = "Unmute";
pub const VOLUME: &str = "&Volume";

// Menus. `&` marks the Alt letter; F is favorites everywhere.
pub const MENU_FILE: &str = "&File";
pub const MENU_STATIONS: &str = "&Stations";
pub const MENU_PLAYBACK: &str = "&Playback";
pub const MENU_HELP: &str = "&Help";
pub const ANNOUNCE_TITLES: &str = "&Announce Song Titles";
pub const ANNOUNCE_STATUS: &str = "Announce &Connection Status";
pub const CLOSE_LISTS: &str = "C&lose Lists When Playing";
pub const PLAY_LAST: &str = "Play Last Station at &Startup";
pub const AUDIO_DEVICE: &str = "Audio &Device";
pub const EXIT: &str = "E&xit";
pub const SEARCH: &str = "&Search…";
pub const BROWSE: &str = "&Browse…";
pub const FAVORITES: &str = "&Favorites…";
pub const RECENT: &str = "&Recently Played…";
pub const COLLECTIONS: &str = "&Collections…";
pub const ADD_CUSTOM: &str = "Add Cus&tom Station…";
pub const IMPORT: &str = "&Import Playlist…";
pub const IMPORT_AS_COLLECTION: &str = "Import Playlist as Coll&ection…";
pub const COPY_URL: &str = "Copy Stream &URL";
pub const GET_STARTER: &str = "&Get SerrebiRadio Stations";
pub const PLAY_PAUSE: &str = "&Play/Pause";
pub const STOP_MENU: &str = "&Stop";
pub const MUTE_MENU: &str = "&Mute";
pub const VOLUME_UP: &str = "Volume &Up";
pub const VOLUME_DOWN: &str = "Volume &Down";
pub const ADD_TO_FAVORITES: &str = "Add to &Favorites";
pub const REMOVE_FROM_FAVORITES: &str = "Remove from &Favorites";
pub const ADD_TO_COLLECTION: &str = "Add to &Collection…";
pub const SONG_HISTORY: &str = "Song &History…";
pub const SLEEP_TIMER: &str = "Sleep &Timer";
/// In `SLEEP_MINUTES` order.
pub const SLEEP_ITEMS: [&str; 6] = [
	"&15 Minutes",
	"&30 Minutes",
	"&45 Minutes",
	"1 &Hour",
	"&90 Minutes",
	"&2 Hours",
];
pub const SLEEP_OFF: &str = "&Off";
pub const EQUALIZER: &str = "&Equalizer…";
pub const USER_GUIDE: &str = "User &Guide";
pub const ABOUT: &str = "&About BlindRadio";

// Station lists.
pub const STATIONS: &str = "Stations";
pub const COLUMN_NAME: &str = "Name";
pub const COLUMN_INFO: &str = "Info";
pub const COLUMN_DIRECTORY: &str = "Directory";
pub const NOTHING_TO_DO: &str = "Nothing to do here";

// Context menu.
pub const PLAY_ITEM: &str = "&Play";
pub const ADD_TO_COLLECTION_SUB: &str = "Add to &Collection";
pub const NEW_COLLECTION: &str = "&New Collection…";
pub const NEW_COLLECTION_TITLE: &str = "New Collection";
pub const COLLECTION_NAME: &str = "Collection name";
pub const DELETE_CUSTOM: &str = "&Delete Custom Station";
pub const RENAME_ITEM: &str = "&Rename";
pub const DELETE_ITEM: &str = "&Delete";
/// Prefix: `Added to <collection>`.
pub const ADDED_TO: &str = "Added to ";
/// Prefix: `Removed from <collection>`.
pub const REMOVED_FROM: &str = "Removed from ";

// Search.
pub const SEARCH_TITLE: &str = "Search";
pub const NAME: &str = "Name";
pub const GENRE: &str = "Genre";
pub const COUNTRY: &str = "Country";
pub const LANGUAGE: &str = "Language";
pub const SEARCH_BUTTON: &str = "&Search";
pub const RESULTS: &str = "Results";
pub const CLOSE: &str = "Close";
pub const SEARCHING: &str = "Searching";
pub const NO_RESULTS: &str = "No results";
/// No filter in Search's boxes.
pub const ANY: &str = "Any";

// Browse.
pub const BROWSE_TITLE: &str = "Browse";
pub const COLLECTIONS_FOLDER: &str = "Collections";
pub const LOADING: &str = "Loading";
pub const LOAD_MORE: &str = "Load More";
pub const ONLY_OWN: &str = "Only your own stations can be deleted.";
/// Prefix: `Delete <collection>?`.
pub const DELETE_PREFIX: &str = "Delete ";

// Favorites and Recently Played.
pub const FAVORITES_TITLE: &str = "Favorites";
pub const FAVORITES_LIST: &str = "Favorites";
pub const RECENT_TITLE: &str = "Recently Played";
pub const RECENT_LIST: &str = "Recently played";
pub const REMOVE: &str = "&Remove";
pub const MOVE_UP: &str = "Move &Up";
pub const MOVE_DOWN: &str = "Move &Down";
pub const EXPORT: &str = "&Export…";
/// Prefix: `Exported <n>`.
pub const EXPORTED: &str = "Exported ";
pub const FAVORITES_FILE: &str = "Favorites.m3u";
pub const PLAYLISTS_FILTER: &str = "Playlists (*.m3u;*.m3u8;*.pls)|*.m3u;*.m3u8;*.pls";

// Add Custom Station.
pub const ADD_CUSTOM_TITLE: &str = "Add Custom Station";
pub const STREAM_URL: &str = "Stream &URL";
pub const NOT_A_STREAM_URL: &str = "Not a stream URL";
/// Prefix: `Added <name>`.
pub const ADDED: &str = "Added ";
pub const OK: &str = "OK";
pub const CANCEL: &str = "Cancel";

// Collections and Add to Collection.
pub const COLLECTIONS_TITLE: &str = "Collections";
pub const COLLECTIONS_LIST: &str = "Collections";
pub const NEW_COLLECTION_BUTTON: &str = "&New";
pub const REMOVE_STATION: &str = "Remove &Station";
pub const ADD_ALL_SUB: &str = "Add &All to Collection";
pub const CUSTOM_FIXED: &str = "Custom Stations cannot be changed";
/// Joins `Added <n>` and the collection: ` to `.
pub const TO: &str = " to ";
pub const M3U_EXTENSION: &str = ".m3u";
pub const IMPORT_TITLE: &str = "Import Playlist";

// Copy, Song History and Equalizer.
pub const COPIED: &str = "Copied";
pub const COPY_FAILED: &str = "Copy failed";
pub const NOTHING_TO_COPY: &str = "Nothing to copy";
pub const SONG_HISTORY_TITLE: &str = "Song History";
pub const SONGS: &str = "Songs";
pub const COLUMN_SONG: &str = "Song";
pub const COLUMN_PLAYED: &str = "Played";
pub const COPY: &str = "&Copy";
pub const EQUALIZER_TITLE: &str = "Equalizer";
pub const PRESET: &str = "&Preset";
pub const BASS: &str = "&Bass (dB)";
pub const MID: &str = "&Mid (dB)";
pub const TREBLE: &str = "&Treble (dB)";
pub const ENABLED: &str = "&Enabled";

// Global hotkeys.
pub const GLOBAL_HOTKEYS: &str = "&Global Hotkeys…";
pub const TAB_GLOBAL: &str = "Global";
pub const HK_PLAY_PAUSE: &str = "Play/Pause";
pub const HK_STOP: &str = "Stop";
pub const HK_VOLUME_UP: &str = "Volume Up";
pub const HK_VOLUME_DOWN: &str = "Volume Down";
pub const HK_NEXT_FAVORITE: &str = "Next Favorite";
pub const HK_PREVIOUS_FAVORITE: &str = "Previous Favorite";
pub const HK_ANNOUNCE: &str = "Announce Now Playing";
pub const HK_TOGGLE_WINDOW: &str = "Hide or Show BlindRadio";
pub const HK_TOGGLE_EQ: &str = "Equalizer On or Off";
/// Prefix: `Hotkey taken: <chord>, <chord>`.
pub const HOTKEY_TAKEN: &str = "Hotkey taken: ";
pub const HOTKEYS_UNAVAILABLE: &str = "Hotkeys unavailable";
pub const EQ_ON: &str = "Equalizer on";
pub const EQ_OFF: &str = "Equalizer off";
pub const NO_FAVORITES: &str = "No favorites";
pub const SHOW_WINDOW: &str = "&Show BlindRadio";
