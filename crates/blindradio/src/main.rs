// Plan 1 test shell: a station list that dial-tunes. The real UI arrives in plan 4.
#![windows_subsystem = "windows"]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use blindradio_core::Engine;
use blindradio_core::output::Output;
use blindradio_core::station::Station;
use blindradio_core::tuner::{LiveFactory, Tuner, TunerEvent};
use crossbeam_channel::Receiver;
use wxdragon::prelude::*;

const STATIONS: &str = include_str!("../../blindradio-core/examples/stations.json");
const POLL_MS: i32 = 50;

struct App {
	tuner: Tuner<LiveFactory, Output>,
	stations: Vec<Station>,
	rendered: Receiver<(u64, Instant)>,
	events: Receiver<TunerEvent>,
	frame: Frame,
	// Dropped last: the tuner's streams run on its runtime.
	_engine: Engine,
}

impl App {
	fn focus(&mut self, index: usize) {
		if let Some(station) = self.stations.get(index) {
			// The window title follows focus at once; the song is added when known.
			self.frame.set_title(&station.name);
		}
		self.tuner.focus(&self.stations, index, Instant::now());
	}

	fn poll(&mut self) {
		let rendered: Vec<_> = self.rendered.try_iter().collect();
		self.tuner.poll(Instant::now(), &rendered);
		for event in self.events.try_iter() {
			match event {
				TunerEvent::Title { station_id, title } => {
					let Some(station) = self.stations.iter().find(|s| s.id == station_id) else {
						continue;
					};
					match title {
						Some(song) => self.frame.set_title(&format!("{song} - {}", station.name)),
						None => self.frame.set_title(&station.name),
					}
				}
				// Speech arrives in plan 5.
				TunerEvent::Failed { name, reason, .. } => eprintln!("Failed: {name} ({reason})"),
				TunerEvent::Announce(song) => eprintln!("{song}"),
				_ => {}
			}
		}
	}
}

fn main() {
	let stations: Vec<Station> = serde_json::from_str(STATIONS).expect("embedded stations.json");
	let _ = wxdragon::main(move |_| {
		let output = match Output::start_default() {
			Ok(o) => o,
			Err(e) => {
				eprintln!("Failed: {e}");
				return;
			}
		};
		let cache = std::env::temp_dir().join("blindradio/cache.json");
		let engine = match Engine::new(&cache, output.sample_rate) {
			Ok(e) => e,
			Err(e) => {
				eprintln!("Failed: {e}");
				return;
			}
		};

		let frame = Frame::builder().with_title("BlindRadio").build();
		let panel = Panel::builder(&frame).build();
		// wxMSW names the list after the label created just before it.
		let label = StaticText::builder(&panel).with_label("Stations").build();
		let list = ListBox::builder(&panel)
			.with_choices(stations.iter().map(|s| s.name.clone()).collect())
			.build();
		let sizer = BoxSizer::builder(Orientation::Vertical).build();
		sizer.add(&label, 0, SizerFlag::All, 5);
		sizer.add(&list, 1, SizerFlag::Expand | SizerFlag::All, 5);
		panel.set_sizer(sizer, true);

		let rendered = output.rendered().clone();
		let (tx, events) = crossbeam_channel::unbounded();
		let app = Rc::new(RefCell::new(App {
			tuner: Tuner::new(LiveFactory(engine.ctx.clone()), output, tx),
			stations,
			rendered,
			events,
			frame,
			_engine: engine,
		}));

		let a = app.clone();
		list.on_selection_changed(move |e| {
			if let Some(i) = e.get_selection().and_then(|i| usize::try_from(i).ok()) {
				a.borrow_mut().focus(i);
			}
		});

		let a = app.clone();
		list.on_key_down(move |e| {
			let WindowEventData::Keyboard(key) = e else {
				return;
			};
			match key.get_key_code() {
				Some(c) if c == 'S' as i32 && key.control_down() => a.borrow_mut().tuner.stop(),
				Some(WXK_ESCAPE) => frame.close(true),
				_ => key.event.skip(true),
			}
		});

		let a = app.clone();
		frame.on_close(move |e| {
			a.borrow_mut().tuner.stop();
			if let WindowEventData::General(e) = e {
				e.skip(true);
			}
		});

		let timer = Timer::new(&frame);
		let a = app.clone();
		timer.on_tick(move |_| a.borrow_mut().poll());
		timer.start(POLL_MS, false);
		// The timer must outlive this closure; it lives as long as the process.
		std::mem::forget(timer);

		frame.show(true);
		frame.centre();
		// Landing on a station plays it (owner rule), including the first one at start.
		list.set_selection(0, true);
		list.set_focus();
		app.borrow_mut().focus(0);
	});
}
