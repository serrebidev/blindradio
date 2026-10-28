//! Equalizer (Ctrl+E): a preset and three bands, heard and saved at once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blindradio_core::eq;
use blindradio_core::settings::{EqPreset, EqSettings};
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::strings as s;

/// Every preset the choice offers, in this order.
pub const PRESETS: [EqPreset; 7] = [
	EqPreset::Flat,
	EqPreset::BassBoost,
	EqPreset::TrebleBoost,
	EqPreset::Loudness,
	EqPreset::Voice,
	EqPreset::Warm,
	EqPreset::Custom,
];

#[derive(Clone, Copy)]
pub enum Band {
	Bass,
	Mid,
	Treble,
}

/// A band moved by hand makes the preset Custom; the value is clamped.
pub fn set_band(settings: &mut EqSettings, band: Band, db: i32) {
	let db = db.clamp(-i32::from(eq::MAX_DB), i32::from(eq::MAX_DB)) as i8;
	match band {
		Band::Bass => settings.bass = db,
		Band::Mid => settings.mid = db,
		Band::Treble => settings.treble = db,
	}
	settings.preset = EqPreset::Custom;
}

/// A preset moves the bands with it; Custom keeps them as they are.
pub fn apply_preset(settings: &mut EqSettings, preset: EqPreset) {
	settings.preset = preset;
	if let Some((bass, mid, treble)) = preset.bands() {
		(settings.bass, settings.mid, settings.treble) = (bass, mid, treble);
	}
}

/// Up and Right raise, Down and Left lower, by one dB: a native trackbar lowers on Up.
fn key_step(code: i32) -> Option<i32> {
	match code {
		WXK_UP | WXK_RIGHT => Some(1),
		WXK_DOWN | WXK_LEFT => Some(-1),
		_ => None,
	}
}

fn slider(dialog: &Dialog, sizer: &BoxSizer, label: &str, value: i8) -> Slider {
	let text = StaticText::builder(dialog).with_label(label).build();
	let slider = Slider::builder(dialog)
		.with_min_value(i32::from(-eq::MAX_DB))
		.with_max_value(i32::from(eq::MAX_DB))
		.build();
	slider.set_value(i32::from(value));
	sizer.add(
		&text,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&slider, 0, SizerFlag::Expand | SizerFlag::All, 5);
	slider
}

pub fn show(app: &Rc<RefCell<App>>) -> Option<Action> {
	let frame = app.borrow().win.frame;
	let dialog = Dialog::builder(&frame, s::EQUALIZER_TITLE).build();
	let current = app.borrow().settings.eq.clone();
	let sizer = BoxSizer::builder(Orientation::Vertical).build();

	let preset_label = StaticText::builder(&dialog).with_label(s::PRESET).build();
	let preset = Choice::builder(&dialog).build();
	for p in PRESETS {
		preset.append(p.label());
	}
	let at = PRESETS
		.iter()
		.position(|p| *p == current.preset)
		.unwrap_or(0);
	preset.set_selection(at as u32);
	sizer.add(
		&preset_label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&preset, 0, SizerFlag::Expand | SizerFlag::All, 5);

	let enabled = CheckBox::builder(&dialog).with_label(s::ENABLED).build();
	enabled.set_value(current.enabled);
	sizer.add(&enabled, 0, SizerFlag::All, 5);

	let bass = slider(&dialog, &sizer, s::BASS, current.bass);
	let mid = slider(&dialog, &sizer, s::MID, current.mid);
	let treble = slider(&dialog, &sizer, s::TREBLE, current.treble);
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	sizer.add(&close, 0, SizerFlag::All, 5);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(420, 420));

	// Moving the sliders to follow a preset must not read back as a hand move.
	let programmatic = Rc::new(Cell::new(false));
	{
		let (a, programmatic) = (app.clone(), programmatic.clone());
		preset.on_selection_changed(move |_| {
			let Some(p) = PRESETS
				.get(preset.get_selection().unwrap_or(0) as usize)
				.copied()
			else {
				return;
			};
			let mut eq = a.borrow().settings.eq.clone();
			apply_preset(&mut eq, p);
			programmatic.set(true);
			bass.set_value(i32::from(eq.bass));
			mid.set_value(i32::from(eq.mid));
			treble.set_value(i32::from(eq.treble));
			programmatic.set(false);
			let said = eq::describe(&eq);
			a.borrow_mut().set_eq(eq);
			a.borrow_mut().say(&said);
		});
	}

	for (slider, band) in [(bass, Band::Bass), (mid, Band::Mid), (treble, Band::Treble)] {
		let (a, programmatic) = (app.clone(), programmatic.clone());
		slider.on_slider(move |e| {
			if programmatic.get() {
				return;
			}
			let mut eq = a.borrow().settings.eq.clone();
			set_band(&mut eq, band, e.get_value());
			a.borrow_mut().set_eq(eq);
		});
		// One dB a press, Home and End to the ends; the native trackbar goes its own way.
		let a = app.clone();
		slider.bind_internal(EventType::KEY_DOWN, move |e: Event| {
			let Some(code) = e.get_key_code() else {
				return e.skip(true);
			};
			if e.control_down() || e.alt_down() || e.shift_down() {
				return e.skip(true);
			}
			let value = match code {
				WXK_HOME => -i32::from(eq::MAX_DB),
				WXK_END => i32::from(eq::MAX_DB),
				c => match key_step(c) {
					Some(step) => slider.get_value() + step,
					None => return e.skip(true),
				},
			};
			e.skip(false);
			slider.set_value(value);
			let mut eq = a.borrow().settings.eq.clone();
			set_band(&mut eq, band, value);
			a.borrow_mut().set_eq(eq);
		});
	}

	{
		let a = app.clone();
		enabled.on_toggled(move |_| {
			let mut eq = a.borrow().settings.eq.clone();
			eq.enabled = enabled.get_value();
			a.borrow_mut().set_eq(eq);
		});
	}

	let token = WindowToken::new();
	preset.set_focus();
	crate::dialogs::run(app, dialog, Action::Equalizer, preset, &token)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn slider_moves_preset_to_custom() {
		let mut eq = EqSettings {
			enabled: true,
			preset: EqPreset::BassBoost,
			bass: 6,
			mid: 0,
			treble: 0,
		};
		set_band(&mut eq, Band::Mid, 4);
		assert_eq!(eq.preset, EqPreset::Custom, "a hand move is Custom");
		assert_eq!((eq.bass, eq.mid, eq.treble), (6, 4, 0));
		set_band(&mut eq, Band::Treble, 99);
		assert_eq!(eq.treble, eq::MAX_DB, "clamped");
		set_band(&mut eq, Band::Bass, -99);
		assert_eq!(eq.bass, -eq::MAX_DB);
		apply_preset(&mut eq, EqPreset::Warm);
		assert_eq!((eq.bass, eq.mid, eq.treble), (3, 1, -2));
		assert_eq!(eq.preset, EqPreset::Warm);
	}
}
