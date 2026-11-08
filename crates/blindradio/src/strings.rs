//! Every user-visible string, in one place. Each function returns the
//! translation of its English text via patois (`t()`); the extractor
//! collects the literals into `po/blindradio.pot`. Terse on purpose.

pub const APP_NAME: &str = "BlindRadio";
/// Prefix: `Failed: <name>`.
pub fn failed() -> String {
	patois::t("Failed: ")
}
pub fn reconnecting() -> String {
	patois::t("Connection lost. Reconnecting.")
}
pub fn reconnected() -> String {
	patois::t("Reconnected")
}
pub fn stopped() -> String {
	patois::t("Stopped")
}
pub fn nothing_to_play() -> String {
	patois::t("Nothing to play")
}
pub fn playing_not_listed() -> String {
	patois::t("The playing station is in no list")
}
pub fn nothing_playing() -> String {
	patois::t("Nothing playing")
}
pub fn no_station_here() -> String {
	patois::t("No station here")
}
/// Prefix: `No favorite <n>`.
pub fn no_favorite() -> String {
	patois::t("No favorite ")
}
pub fn added_to_favorites() -> String {
	patois::t("Added to favorites")
}
pub fn removed_from_favorites() -> String {
	patois::t("Removed from favorites")
}
pub fn sleep_stopping() -> String {
	patois::t("Sleep timer: stopping")
}
pub fn device_lost() -> String {
	patois::t("Device lost. Using default.")
}
/// Prefix: `Using <device>`.
pub fn using() -> String {
	patois::t("Using ")
}
pub fn default_device() -> String {
	patois::t("Default")
}
pub fn not_saved() -> String {
	patois::t("Not saved")
}
pub fn starter_not_loaded() -> String {
	patois::t("Starter stations not loaded")
}
pub fn about_description() -> String {
	patois::t("Screen-reader-first internet radio.")
}
pub const WEBSITE: &str = "https://github.com/serrebidev/blindradio";

// Main window. Its buttons carry no Alt keys (YARSP): their Ctrl keys work everywhere.
pub fn play() -> String {
	patois::t("Play")
}
pub fn pause() -> String {
	patois::t("Pause")
}
pub fn stop() -> String {
	patois::t("Stop")
}
pub fn mute() -> String {
	patois::t("Mute")
}
pub fn unmute() -> String {
	patois::t("Unmute")
}
pub fn volume() -> String {
	patois::t("&Volume")
}

// Menus. `&` marks the Alt letter; F is favorites everywhere.
pub fn menu_file() -> String {
	patois::t("&File")
}
pub fn menu_stations() -> String {
	patois::t("&Stations")
}
pub fn menu_playback() -> String {
	patois::t("&Playback")
}
pub fn menu_recording() -> String {
	patois::t("&Recording")
}
pub fn menu_help() -> String {
	patois::t("&Help")
}
pub fn announce_titles() -> String {
	patois::t("&Announce Song Titles")
}
pub fn announce_status() -> String {
	patois::t("Announce &Connection Status")
}
pub fn announce_actions() -> String {
	patois::t("Announce Ac&tions")
}
pub fn close_lists() -> String {
	patois::t("C&lose Lists When Playing")
}
pub fn play_last() -> String {
	patois::t("Play Last Station at &Startup")
}
pub fn audio_device() -> String {
	patois::t("Audio De&vice")
}
pub fn exit() -> String {
	patois::t("E&xit")
}
pub fn search() -> String {
	patois::t("&Search…")
}
pub fn browse() -> String {
	patois::t("&Browse…")
}
pub fn favorites() -> String {
	patois::t("&Favorites…")
}
pub fn recent() -> String {
	patois::t("&Recently Played…")
}
pub fn collections() -> String {
	patois::t("&Collections…")
}
pub fn add_custom() -> String {
	patois::t("Add Cus&tom Station…")
}
pub fn export_current_list() -> String {
	patois::t("Export &Current Playlist…")
}
pub fn export_all_playlists() -> String {
	patois::t("Export &All Playlists…")
}
pub fn export_favorites() -> String {
	patois::t("Export Fa&vorites…")
}
pub fn export_all_name() -> String {
	patois::t("All Playlists")
}
pub fn move_to_playing_station() -> String {
	patois::t("&Move to Playing Station")
}
pub fn import() -> String {
	patois::t("&Import Playlist…")
}
pub fn import_as_collection() -> String {
	patois::t("Import Playlist as Coll&ection…")
}
pub fn copy_url() -> String {
	patois::t("Copy Stream &URL")
}
pub fn get_starter() -> String {
	patois::t("&Get SerrebiRadio Stations")
}
pub fn play_pause() -> String {
	patois::t("&Play/Pause")
}
pub fn stop_menu() -> String {
	patois::t("&Stop")
}
pub fn record() -> String {
	patois::t("&Record")
}
pub fn skip_back() -> String {
	patois::t("Skip &Back")
}
pub fn skip_forward() -> String {
	patois::t("Skip F&orward")
}
pub fn jump_start() -> String {
	patois::t("&Jump to Start of Buffer")
}
pub fn go_live() -> String {
	patois::t("Go to &Live")
}
pub fn previous_station() -> String {
	patois::t("Prev&ious Station")
}
pub fn next_station() -> String {
	patois::t("&Next Station")
}
pub fn no_earlier_station() -> String {
	patois::t("No earlier station")
}
pub fn filter() -> String {
	patois::t("Filter")
}
pub fn no_matches() -> String {
	patois::t("no matching stations")
}
pub fn matches() -> String {
	patois::t("matching stations")
}
pub fn no_later_station() -> String {
	patois::t("No later station")
}
pub fn paused() -> String {
	patois::t("Paused")
}
pub fn resumed() -> String {
	patois::t("Resumed")
}
pub fn live() -> String {
	patois::t("Live")
}
pub fn record_pause() -> String {
	patois::t("P&ause Recording")
}
pub fn record_songs() -> String {
	patois::t("Record Each Song to Its Own &File")
}
pub fn rec_change_title() -> String {
	patois::t("Recording in Progress")
}
pub fn rec_change_ask() -> String {
	patois::t("A recording is in progress. Changing stations stops the recording; what has been recorded so far is saved. Change stations anyway?")
}
pub fn rec_close_ask() -> String {
	patois::t("A recording is in progress. Closing BlindRadio stops the recording; what has been recorded so far is saved. Close anyway?")
}
pub fn title_recording() -> String {
	patois::t("Recording")
}
pub fn rec_songs_on() -> String {
	patois::t("Record each song to its own file on")
}
pub fn rec_songs_off() -> String {
	patois::t("Record each song to its own file off")
}
pub fn save_buffer() -> String {
	patois::t("&Write Buffer…")
}
pub fn scheduled_recordings() -> String {
	patois::t("Scheduled Recordin&gs…")
}
pub fn open_record_folder() -> String {
	patois::t("&Open Record Folder")
}
pub fn no_record_folder() -> String {
	patois::t("Could not open the record folder")
}
pub fn no_buffer() -> String {
	patois::t("No buffer to save")
}
/// Prefix: `Buffer saved to <path>`.
pub fn buffer_saved() -> String {
	patois::t("Buffer saved to ")
}
pub fn previous_segment() -> String {
	patois::t("P&revious Segment")
}
pub fn next_segment() -> String {
	patois::t("&Next Segment")
}
pub fn write_current_segment() -> String {
	patois::t("Write &Current Segment")
}
pub fn write_previous_segment() -> String {
	patois::t("Write Pre&vious Segment")
}
pub fn write_next_segment() -> String {
	patois::t("Write &Next Segment")
}
pub fn write_all_segments() -> String {
	patois::t("Write A&ll Segments")
}
pub fn no_segments() -> String {
	patois::t("No segments")
}
pub fn no_previous_segment() -> String {
	patois::t("No previous segment")
}
pub fn no_next_segment() -> String {
	patois::t("No next segment")
}
/// Prefix: `Writing <title>`, `Writing 5 segments`.
pub fn writing_segment() -> String {
	patois::t("Writing ")
}
/// Suffix: `5 segments`.
pub fn segments() -> String {
	patois::t(" segments")
}
/// Prefix: `Segment saved to <path>`.
pub fn segment_saved() -> String {
	patois::t("Segment saved to ")
}
/// Between: `5 segments saved to <folder>`.
pub fn segments_saved() -> String {
	patois::t(" saved to ")
}
pub fn rec_started() -> String {
	patois::t("Recording started")
}
pub fn rec_paused() -> String {
	patois::t("Recording paused")
}
pub fn rec_resumed() -> String {
	patois::t("Recording resumed")
}
pub fn not_recording() -> String {
	patois::t("Not recording")
}
/// Prefix: `Recording saved to <path>`.
pub fn rec_saved() -> String {
	patois::t("Recording saved to ")
}
/// Suffix: ` - <time> behind live`.
pub fn behind_live() -> String {
	patois::t(" behind live")
}
/// Suffix: ` - <time> played`.
pub fn played_for() -> String {
	patois::t(" played")
}
pub fn status_paused() -> String {
	patois::t(" - Paused")
}
pub fn status_live() -> String {
	patois::t(" - Live")
}
pub fn status_recording() -> String {
	patois::t(" - Recording")
}
pub fn status_rec_paused() -> String {
	patois::t(" - Recording paused")
}
pub fn muted() -> String {
	patois::t("Muted")
}
pub fn unmuted() -> String {
	patois::t("Unmuted")
}
pub fn mute_menu() -> String {
	patois::t("&Mute")
}
pub fn volume_up() -> String {
	patois::t("Volume &Up")
}
pub fn volume_down() -> String {
	patois::t("Volume &Down")
}
pub fn add_to_favorites() -> String {
	patois::t("&Add to Favorites")
}
pub fn remove_from_favorites() -> String {
	patois::t("Re&move from Favorites")
}
pub fn add_to_collection() -> String {
	patois::t("Add to Co&llection…")
}
pub fn song_history() -> String {
	patois::t("Song &History…")
}
pub fn sleep_timer() -> String {
	patois::t("Sleep &Timer")
}
/// In `SLEEP_MINUTES` order.
pub fn sleep_items() -> [String; 6] {
	[patois::t("&15 Minutes"), patois::t("&30 Minutes"), patois::t("&45 Minutes"), patois::t("1 &Hour"), patois::t("&90 Minutes"), patois::t("&2 Hours")]
}
pub fn sleep_off() -> String {
	patois::t("&Off")
}
pub fn equalizer() -> String {
	patois::t("&Equalizer…")
}
pub fn user_guide() -> String {
	patois::t("User &Guide")
}
pub fn view_changelog() -> String {
	patois::t("View &Changelog")
}
pub fn github_page() -> String {
	patois::t("Go to BlindRadio Git&Hub Page")
}
pub fn no_browser() -> String {
	patois::t("Could not open the browser")
}
pub fn check_updates() -> String {
	patois::t("Check for &Updates")
}
pub fn about() -> String {
	patois::t("&About BlindRadio")
}

// Station lists.
pub fn stations() -> String {
	patois::t("Stations")
}
pub fn column_name() -> String {
	patois::t("Name")
}
pub fn column_info() -> String {
	patois::t("Info")
}
pub fn column_directory() -> String {
	patois::t("Directory")
}
pub fn nothing_to_do() -> String {
	patois::t("Nothing to do here")
}

// Context menu.
pub fn play_item() -> String {
	patois::t("&Play")
}
pub fn add_to_collection_sub() -> String {
	patois::t("Add to &Collection")
}
pub fn new_collection() -> String {
	patois::t("&New Collection…")
}
pub fn add_to_fav_folder() -> String {
	patois::t("Add to Favorites &Folder")
}
pub fn add_to_collection_folder() -> String {
	patois::t("Add to Collection F&older")
}
pub fn move_to_folder() -> String {
	patois::t("Move to &Folder")
}
pub fn new_folder() -> String {
	patois::t("New &Folder…")
}
pub fn new_folder_title() -> String {
	patois::t("New Folder")
}
pub fn folder_name() -> String {
	patois::t("Folder name")
}
pub fn new_collection_title() -> String {
	patois::t("New Collection")
}
pub fn collection_name() -> String {
	patois::t("Collection name")
}
pub fn delete_custom() -> String {
	patois::t("&Delete Custom Station")
}
pub fn rename_item() -> String {
	patois::t("&Rename")
}
pub fn delete_item() -> String {
	patois::t("&Delete")
}
/// Prefix: `Added to <collection>`.
pub fn added_to() -> String {
	patois::t("Added to ")
}
/// Prefix: `Removed from <collection>`.
pub fn removed_from() -> String {
	patois::t("Removed from ")
}
/// Prefix: `Adding <folder>`.
pub fn adding() -> String {
	patois::t("Adding ")
}
pub fn nothing_to_add() -> String {
	patois::t("Nothing to add")
}
/// Suffix when a folder held more than is filed in one go.
pub fn stopped_there() -> String {
	patois::t(", stopped there")
}

// Search.
pub fn search_title() -> String {
	patois::t("Search")
}
pub fn name() -> String {
	patois::t("Name")
}
pub fn genre() -> String {
	patois::t("Genre")
}
pub fn country() -> String {
	patois::t("Country")
}
pub fn language() -> String {
	patois::t("Language")
}
pub fn search_button() -> String {
	patois::t("&Search")
}
pub fn results() -> String {
	patois::t("Results")
}
pub fn close() -> String {
	patois::t("Close")
}
pub fn searching() -> String {
	patois::t("Searching")
}
pub fn no_results() -> String {
	patois::t("No results")
}
/// No filter in Search's boxes.
pub fn any() -> String {
	patois::t("Any")
}

// Browse.
pub fn browse_title() -> String {
	patois::t("Browse")
}
pub fn collections_folder() -> String {
	patois::t("Collections")
}
pub fn loading() -> String {
	patois::t("Loading")
}
pub fn load_more() -> String {
	patois::t("Load More")
}
pub fn sort() -> String {
	patois::t("Sort")
}
pub fn reverse() -> String {
	patois::t("&Reverse")
}
pub fn only_own() -> String {
	patois::t("Only your own stations can be deleted.")
}
/// Prefix: `Delete <collection>?`.
pub fn delete_prefix() -> String {
	patois::t("Delete ")
}

// Favorites and Recently Played.
pub fn favorites_title() -> String {
	patois::t("Favorites")
}
pub fn favorites_list() -> String {
	patois::t("Favorites")
}
pub fn recent_title() -> String {
	patois::t("Recently Played")
}
pub fn recent_list() -> String {
	patois::t("Recently played")
}
pub fn remove() -> String {
	patois::t("&Remove")
}
pub fn move_up() -> String {
	patois::t("Move &Up")
}
pub fn move_down() -> String {
	patois::t("Move &Down")
}
pub fn export() -> String {
	patois::t("&Export…")
}
/// Prefix: `Exported <n>`.
pub fn exported() -> String {
	patois::t("Exported ")
}
pub fn favorites_file() -> String {
	patois::t("Favorites.m3u")
}
pub fn name_automatically() -> String {
	patois::t("Name Automatically")
}
pub fn looking_up_name() -> String {
	patois::t("Looking up the name")
}
pub fn no_name_found() -> String {
	patois::t("No name found")
}
pub fn buffer_start() -> String {
	patois::t("Start buffer, in milliseconds:")
}
pub fn buffer_refill() -> String {
	patois::t("Refill limit after dropouts, in seconds:")
}
pub fn buffer_hls() -> String {
	patois::t("HLS segments behind live:")
}
pub fn slider_suffix() -> String {
	patois::t(" slider")
}
pub fn playlists_filter() -> String {
	patois::t("Playlists (*.m3u;*.m3u8;*.m3u_plus;*.pls)|*.m3u;*.m3u8;*.m3u_plus;*.pls")
}

// Add Custom Station.
pub fn add_custom_title() -> String {
	patois::t("Add Custom Station")
}
pub fn stream_url() -> String {
	patois::t("Stream &URL")
}
pub fn destination() -> String {
	patois::t("Add to")
}
pub fn not_a_stream_url() -> String {
	patois::t("Not a stream URL")
}
/// Prefix: `Added <name>`.
pub fn added() -> String {
	patois::t("Added ")
}
pub fn ok() -> String {
	patois::t("OK")
}
pub fn cancel() -> String {
	patois::t("Cancel")
}

// Collections and Add to Collection.
pub fn collections_title() -> String {
	patois::t("Collections")
}
pub fn collections_list() -> String {
	patois::t("Collections")
}
pub fn new_collection_button() -> String {
	patois::t("&New")
}
pub fn remove_station() -> String {
	patois::t("Remove &Station")
}
pub fn add_all_sub() -> String {
	patois::t("Add &All to Collection")
}
pub fn custom_fixed() -> String {
	patois::t("Custom Stations cannot be changed")
}
/// Joins `Added <n>` and the collection: ` to `.
pub fn to() -> String {
	patois::t(" to ")
}
pub fn m3u_extension() -> String {
	patois::t(".m3u")
}
pub fn import_title() -> String {
	patois::t("Import Playlist")
}

// Copy, Song History and Equalizer.
pub fn copied() -> String {
	patois::t("Copied")
}
pub fn copy_failed() -> String {
	patois::t("Copy failed")
}
pub fn nothing_to_copy() -> String {
	patois::t("Nothing to copy")
}
pub fn nothing_to_cut() -> String {
	patois::t("Nothing to cut")
}
pub fn song_history_title() -> String {
	patois::t("Song History")
}
pub fn songs() -> String {
	patois::t("Songs")
}
pub fn column_song() -> String {
	patois::t("Song")
}
pub fn column_played() -> String {
	patois::t("Played")
}
pub fn copy() -> String {
	patois::t("&Copy")
}
pub fn equalizer_title() -> String {
	patois::t("Equalizer")
}
pub fn equalizer_own_title() -> String {
	patois::t("Equalizer, global. This station has its own")
}
pub fn preset() -> String {
	patois::t("&Preset")
}
pub fn preamp_db() -> String {
	patois::t("Preamp (dB)")
}
pub fn preamp_enabled() -> String {
	patois::t("Enable preamp")
}
pub fn enabled() -> String {
	patois::t("&Enabled")
}
pub fn preset_name() -> String {
	patois::t("&Name for a saved preset")
}
pub fn save_preset() -> String {
	patois::t("&Save Preset")
}
pub fn delete_preset() -> String {
	patois::t("De&lete Preset")
}

// Scheduled recordings.
pub fn schedules_title() -> String {
	patois::t("Scheduled Recordings")
}
pub fn schedules_list() -> String {
	patois::t("Scheduled recordings")
}
pub fn add_button() -> String {
	patois::t("&Add…")
}
pub fn edit_button() -> String {
	patois::t("&Edit…")
}
pub fn delete_button() -> String {
	patois::t("&Delete")
}
pub fn schedule_station() -> String {
	patois::t("Station")
}
pub fn schedule_kind() -> String {
	patois::t("Kind")
}
pub fn kind_recording() -> String {
	patois::t("Recording")
}
pub fn kind_alarm() -> String {
	patois::t("Alarm")
}
/// Prefix: `Alarm: <station>`.
pub fn alarm_started() -> String {
	patois::t("Alarm: ")
}
pub fn schedule_start() -> String {
	patois::t("Start (YYYY-MM-DD HH:MM, local)")
}
pub fn schedule_duration() -> String {
	patois::t("Duration (minutes)")
}
pub fn schedule_repeat() -> String {
	patois::t("Repeat")
}
pub fn bad_schedule_time() -> String {
	patois::t("Use YYYY-MM-DD HH:MM for the start")
}
pub fn bad_schedule_duration() -> String {
	patois::t("Duration must be 1 to 1440 minutes")
}
/// Prefix: `Scheduled recording started: <station>`.
pub fn schedule_started() -> String {
	patois::t("Scheduled recording started: ")
}
/// Prefix: `Scheduled recording saved to <path>`.
pub fn schedule_saved() -> String {
	patois::t("Scheduled recording saved to ")
}
pub fn schedule_skipped() -> String {
	patois::t("Scheduled recording skipped: already recording")
}
pub fn no_schedules() -> String {
	patois::t("No scheduled recordings")
}

// Feature settings (global and per station).
pub fn feature_settings() -> String {
	patois::t("&Settings…")
}
pub fn features_title() -> String {
	patois::t("Settings")
}
pub fn tab_general() -> String {
	patois::t("General")
}
pub fn tab_playback() -> String {
	patois::t("Playback")
}
pub fn tab_recording() -> String {
	patois::t("Recording")
}
pub fn tab_songs() -> String {
	patois::t("Songs")
}
pub fn tab_sources() -> String {
	patois::t("Sources")
}
pub fn sources_directories() -> String {
	patois::t("&Directories, Space turns on or off")
}
pub fn sources_networks() -> String {
	patois::t("&Networks, Space turns on or off")
}
pub fn source_on() -> String {
	patois::t("on")
}
pub fn source_off() -> String {
	patois::t("off")
}
pub fn directories_all_on() -> String {
	patois::t("All Directories On")
}
pub fn directories_all_off() -> String {
	patois::t("All Directories Off")
}
pub fn networks_all_on() -> String {
	patois::t("All Networks On")
}
pub fn networks_all_off() -> String {
	patois::t("All Networks Off")
}
pub fn startup_list() -> String {
	patois::t("Startup L&ist")
}
/// The Language choice's first entry: follow the system language.
pub fn language_system() -> String {
	patois::t("System Default")
}
pub fn startup_default() -> String {
	patois::t("Default Stations")
}
pub fn startup_all() -> String {
	patois::t("All Playlists")
}
pub fn startup_favorites() -> String {
	patois::t("Favorites")
}
pub fn check_updates_auto() -> String {
	patois::t("Check for &Updates automatically")
}
pub fn report_plays() -> String {
	patois::t("&Report plays to Radio Browser")
}
pub fn open_url() -> String {
	patois::t("&Open URL…")
}
pub fn open_url_title() -> String {
	patois::t("Open URL")
}
pub fn detect_streams() -> String {
	patois::t("&Detect Audio Stream on Page…")
}
pub fn detect_title() -> String {
	patois::t("Detect Audio Stream on Page")
}
pub fn page_url() -> String {
	patois::t("Page &URL")
}
pub fn detect_button() -> String {
	patois::t("&Detect")
}
pub fn streams_found() -> String {
	patois::t("&Streams")
}
pub fn detecting() -> String {
	patois::t("Looking for streams")
}
pub fn no_streams() -> String {
	patois::t("No audio streams found on this page")
}
/// Suffix: `2 streams found`.
pub fn streams_suffix() -> String {
	patois::t(" found")
}
pub fn station_info_item() -> String {
	patois::t("Station &Info…")
}
pub fn station_info_title() -> String {
	patois::t("Station Info")
}
pub fn station_info_label() -> String {
	patois::t("&Details")
}
pub fn info_name() -> String {
	patois::t("Name")
}
pub fn info_genre() -> String {
	patois::t("Genre")
}
pub fn info_country() -> String {
	patois::t("Country")
}
pub fn info_location() -> String {
	patois::t("Location")
}
pub fn info_slogan() -> String {
	patois::t("Slogan")
}
pub fn info_language() -> String {
	patois::t("Language")
}
pub fn info_call_sign() -> String {
	patois::t("Call sign")
}
pub fn info_codec() -> String {
	patois::t("Codec")
}
pub fn info_bitrate() -> String {
	patois::t("Bitrate")
}
pub fn info_sample_rate() -> String {
	patois::t("Sample rate")
}
pub fn info_channels() -> String {
	patois::t("Channels")
}
pub fn info_homepage() -> String {
	patois::t("Homepage")
}
pub fn info_stream_url() -> String {
	patois::t("Stream URL")
}
pub fn info_while_playing() -> String {
	patois::t("Known while the station plays")
}
pub fn copy_station_item() -> String {
	patois::t("&Copy Station")
}
pub fn cut_station_item() -> String {
	patois::t("Cu&t Station")
}
pub fn paste_station_item() -> String {
	patois::t("&Paste Station")
}
pub fn copied_station() -> String {
	patois::t("Copied ")
}
pub fn cut_said() -> String {
	patois::t("Cut ")
}
pub fn pasted() -> String {
	patois::t("Pasted ")
}
pub fn into() -> String {
	patois::t(" into ")
}
pub fn is_already_in() -> String {
	patois::t(" is already in ")
}
pub fn nothing_to_paste() -> String {
	patois::t("Nothing to paste")
}
pub fn nowhere_to_paste() -> String {
	patois::t("Nowhere to paste here")
}
pub fn edit_station_item() -> String {
	patois::t("&Edit Station…")
}
pub fn edit_station_title() -> String {
	patois::t("Edit Station")
}
pub fn saved() -> String {
	patois::t("Saved ")
}
pub fn play_on_move() -> String {
	patois::t("Play a station as I mo&ve to it")
}
pub fn expand_dial_folders() -> String {
	patois::t("&Expand all directories in the main window")
}
pub fn fm_sound() -> String {
	patois::t("&FM sound between stations")
}
pub fn close_to_tray() -> String {
	patois::t("&Hide to the system tray when closed")
}
pub fn maximize_start() -> String {
	patois::t("Ma&ximize the window at startup")
}
pub fn ts_enable() -> String {
	patois::t("&Time-Shift Playback")
}
pub fn ts_minutes() -> String {
	patois::t("&Buffer Minutes")
}
pub fn ts_ring() -> String {
	patois::t("Encoded Rin&g Buffer (DVBViewer-style, optional)")
}
pub fn ts_max_mb() -> String {
	patois::t("Ring Si&ze (MB, 0 is the 200 MB default)")
}
pub fn seek_step() -> String {
	patois::t("&Seek Step (Seconds)")
}
pub fn rec_format() -> String {
	patois::t("Recording &Format")
}
pub fn rec_bitrate() -> String {
	patois::t("MP3 &Bitrate (kbps)")
}
pub fn rec_vbr() -> String {
	patois::t("MP3 &VBR")
}
pub fn rec_dir() -> String {
	patois::t("Recordings F&older (blank is the default)")
}
pub fn fmt_original() -> String {
	patois::t("Original stream format")
}
pub fn fmt_wav() -> String {
	patois::t("WAV")
}
pub fn fmt_mp3() -> String {
	patois::t("MP3")
}
pub fn fmt_flac() -> String {
	patois::t("FLAC")
}
pub fn recog_enable() -> String {
	patois::t("&Recognize Songs Without Metadata")
}
pub fn recog_provider() -> String {
	patois::t("Recognition &Service")
}
pub fn recog_token() -> String {
	patois::t("AudD &Token")
}
pub fn recog_only_meta() -> String {
	patois::t("Only when the stream gives no t&itle")
}
pub fn recog_announce() -> String {
	patois::t("&Announce Recognized Songs")
}
pub fn recog_no_token() -> String {
	patois::t("Song recognition is on, but no AudD token is set, so nothing will be recognized. Add a token in Settings.")
}
pub fn prov_metadata() -> String {
	patois::t("Stream metadata only")
}
pub fn prov_audd() -> String {
	patois::t("AudD")
}
pub fn scrob_enable() -> String {
	patois::t("S&crobble Listened Songs")
}
pub fn scrob_lb_token() -> String {
	patois::t("ListenBrainz T&oken")
}
pub fn scrob_key() -> String {
	patois::t("last.fm API &Key")
}
pub fn scrob_secret() -> String {
	patois::t("&last.fm Secret")
}
pub fn scrob_session() -> String {
	patois::t("last.fm Session K&ey")
}
pub fn scrob_base() -> String {
	patois::t("last.fm &Base URL (blank is last.fm)")
}
pub fn use_global() -> String {
	patois::t("Use Global")
}
pub fn on() -> String {
	patois::t("On")
}
pub fn off() -> String {
	patois::t("Off")
}
pub fn st_ts_minutes() -> String {
	patois::t("Buffer &Minutes (blank is global)")
}
pub fn st_recog() -> String {
	patois::t("Song &Recognition")
}
pub fn st_scrob() -> String {
	patois::t("Scro&bbling")
}
pub fn st_eq() -> String {
	patois::t("&Equalizer")
}
pub fn st_eq_preset() -> String {
	patois::t("Equalizer Pre&set")
}
pub fn st_metadata_offset() -> String {
	patois::t("Metadata offset, seconds (blank = Default; positive = early, negative = late)")
}
pub fn st_metadata_offset_invalid() -> String {
	patois::t("Enter a whole number from -30 to 30, or leave blank for Default.")
}

// Global hotkeys.
pub fn global_hotkeys() -> String {
	patois::t("&Global Hotkeys…")
}
pub fn tab_global() -> String {
	patois::t("Global")
}
pub fn keyboard_shortcuts() -> String {
	patois::t("&Keyboard Shortcuts…")
}
pub fn kb_tab_app() -> String {
	patois::t("App")
}
pub fn kb_play_favorite() -> String {
	patois::t("Play Favorite")
}
pub fn kb_alternate() -> String {
	patois::t(" (alternate key)")
}
pub fn kb_focus_volume() -> String {
	patois::t("Focus the Volume Slider")
}
pub fn kb_close_dialog() -> String {
	patois::t("Close Dialog")
}
pub fn hk_play_pause() -> String {
	patois::t("Play/Pause")
}
pub fn hk_stop() -> String {
	patois::t("Stop")
}
pub fn hk_volume_up() -> String {
	patois::t("Volume Up")
}
pub fn hk_volume_down() -> String {
	patois::t("Volume Down")
}
pub fn hk_next_favorite() -> String {
	patois::t("Next Favorite")
}
pub fn hk_previous_favorite() -> String {
	patois::t("Previous Favorite")
}
pub fn hk_announce() -> String {
	patois::t("Announce Now Playing")
}
pub fn hk_toggle_window() -> String {
	patois::t("Hide or Show BlindRadio")
}
pub fn hk_toggle_eq() -> String {
	patois::t("Equalizer On or Off")
}
pub fn hk_record() -> String {
	patois::t("Record")
}
pub fn hk_seek_back() -> String {
	patois::t("Skip Back")
}
pub fn hk_seek_forward() -> String {
	patois::t("Skip Forward")
}
pub fn hk_go_live() -> String {
	patois::t("Go to Live")
}
/// Prefix: `Hotkey taken: <chord>, <chord>`.
pub fn hotkey_taken() -> String {
	patois::t("Hotkey taken: ")
}
pub fn hotkeys_unavailable() -> String {
	patois::t("Hotkeys unavailable")
}
pub fn eq_on() -> String {
	patois::t("Equalizer on")
}
pub fn eq_off() -> String {
	patois::t("Equalizer off")
}
pub fn eq_own() -> String {
	patois::t(". This station has its own")
}
pub fn no_favorites() -> String {
	patois::t("No favorites")
}
pub fn show_window() -> String {
	patois::t("&Show BlindRadio")
}

// Help.
pub fn about_title() -> String {
	patois::t("About BlindRadio")
}
pub fn licenses_button() -> String {
	patois::t("&Licenses…")
}
