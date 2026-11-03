//! A real ICY stream through the real stream task and tuner: its titles must
//! become segments on the audio's own timeline, and each must write out as
//! the audio between its title and the next.

mod common;

use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use blindradio_core::net::{ResolveCache, client};
use blindradio_core::output::MixerCmd;
use blindradio_core::ring::PcmRing;
use blindradio_core::segments::{Jump, Which};
use blindradio_core::settings::{RecordFormat, Settings};
use blindradio_core::station::{Source, Station};
use blindradio_core::stream::EngineCtx;
use blindradio_core::tuner::{LiveFactory, Sink, Tuner};
use common::{Route, serve};
use parking_lot::Mutex;

const RATE: u32 = 48_000;
/// Half a second of the fixture's 64 kbps between titles: its 5 s must hold them all.
const BETWEEN_TITLES: usize = 4_000;

static RT: LazyLock<tokio::runtime::Runtime> = LazyLock::new(|| {
	tokio::runtime::Builder::new_multi_thread()
		.enable_all()
		.build()
		.unwrap()
});

/// Stands in for the audio device: the ring must drain, or the decoder waits
/// on it, the stream stops being read and no more titles arrive.
#[derive(Clone, Default)]
struct DrainSink(Arc<Mutex<Option<Arc<PcmRing>>>>);

impl Sink for DrainSink {
	fn send(&self, cmd: MixerCmd) {
		if let MixerCmd::SetActive { ring, .. } = cmd {
			*self.0.lock() = ring;
		}
	}
}

impl DrainSink {
	fn drain(&self) {
		if let Some(ring) = &*self.0.lock() {
			ring.pop_into(&mut vec![0.0; RATE as usize / 5]);
		}
	}
}

#[test]
fn icy_titles_become_segments_that_write_their_own_audio() {
	let tone = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone.mp3")).unwrap();
	let server = serve(vec![Route::Stream {
		path: "/live".into(),
		content_type: "audio/mpeg".into(),
		data: tone,
		metaint: Some(BETWEEN_TITLES),
		titles: vec![b"One".to_vec(), b"Two".to_vec(), b"Three".to_vec(), b"Four".to_vec()],
		burst: BETWEEN_TITLES,
		bytes_per_sec: BETWEEN_TITLES * 2,
		close_after: None,
	}]);
	let dir = tempfile::tempdir().unwrap();
	let mut settings = Settings::default();
	settings.recording.dir = Some(dir.path().to_string_lossy().into_owned());
	settings.recording.format = RecordFormat::Wav;
	let cache = ResolveCache::load(&std::env::temp_dir().join("blindradio-no-such-dir/cache.json"));
	let factory = LiveFactory(EngineCtx {
		rt: RT.handle().clone(),
		client: client(),
		cache: Arc::new(Mutex::new(cache)),
		taps: Default::default(),
		out_rate: RATE,
		retry_window: Duration::from_secs(60),
	});
	let (tx, events) = crossbeam_channel::unbounded();
	let sink = DrainSink::default();
	let mut tuner = Tuner::new(factory, sink.clone(), tx);
	tuner.set_settings(Arc::new(Mutex::new(settings)));
	let station = Station {
		id: "test".into(),
		name: "Test FM".into(),
		url: server.url("/live"),
		source: Source::Custom,
		info: None,
		details: Default::default(),
	};
	tuner.play(&station, Instant::now());
	let end = Instant::now() + Duration::from_secs(20);
	while tuner.segments().len() < 4 && Instant::now() < end {
		tuner.poll(Instant::now(), &[]);
		sink.drain();
		std::thread::sleep(Duration::from_millis(20));
	}

	let segments = tuner.segments();
	let titles: Vec<&str> = segments.iter().take(4).map(|s| s.title.as_str()).collect();
	assert_eq!(titles, ["One", "Two", "Three", "Four"]);
	assert_eq!(tuner.heard_title().as_deref(), Some("Four"), "live is the newest");

	// Jumps stand on the marks: within 3 s of a start, Previous steps back.
	assert!(matches!(tuner.segment_jump(false), Jump::To(s) if s.title == "Three"));
	assert_eq!(tuner.heard_title().as_deref(), Some("Three"));
	assert_eq!(tuner.segment(Which::Current).unwrap().title, "Three");
	assert_eq!(tuner.segment(Which::Previous).unwrap().title, "Two");
	assert_eq!(tuner.segment(Which::Next).unwrap().title, "Four");

	drop(events);
	// The finished ones run title to title: half a second of audio each.
	let finished = &segments[1..3];
	let paths = tuner.segment_writer().unwrap().write(finished).unwrap();
	for (segment, path) in finished.iter().zip(&paths) {
		let name = path.file_name().unwrap().to_str().unwrap();
		assert!(name.starts_with(&format!("Test FM - {} ", segment.title)), "{name}");
		let reader = hound::WavReader::open(path).unwrap();
		assert_eq!(reader.spec().sample_rate, RATE);
		let seconds = reader.duration() as f32 / RATE as f32;
		assert!((0.25..0.85).contains(&seconds), "{name}: {seconds} s");
	}
	// Tuning away keeps nothing: the next station's buffer starts clean.
	tuner.stop();
	assert!(tuner.segments().is_empty());
}
