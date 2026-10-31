//! Equalizer (Ctrl+E): a preset and three bands, heard and saved at once.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use blindradio_core::eq;
use blindradio_core::settings::{EqPreset, EqSettings, SavedEqPreset};
use wxdragon::event::{Event, EventType, WxEvtHandler};
use wxdragon::prelude::*;

use crate::action::Action;
use crate::app::App;
use crate::bridge::WindowToken;
use crate::strings as s;

/// Every built-in preset the choice offers, in this order. The user's
/// saved presets follow them in the list.
pub const PRESETS: [EqPreset; 10] = [
	EqPreset::Flat,
	EqPreset::BassBoost,
	EqPreset::TrebleBoost,
	EqPreset::BassAndTreble,
	EqPreset::Loudness,
	EqPreset::Voice,
	EqPreset::Warm,
	EqPreset::Clarity,
	EqPreset::LaptopSpeakers,
	EqPreset::Custom,
];

#[derive(Clone, Copy)]
pub enum Band {
	Bass,
	Mid,
	Treble,
}

/// A band moved by hand makes the preset Custom; the value is clamped.
/// The hand-tuned set is also written to the Custom store, so it
/// survives a trip through a named preset and back.
/// Moving a band also enables the equalizer: a hand move means "use
/// this", and a separate trip to the Enabled box is how the dialog
/// shipped presets that changed nothing anyone could hear.
pub fn set_band(settings: &mut EqSettings, band: Band, db: i32) {
	let db = db.clamp(-i32::from(eq::MAX_DB), i32::from(eq::MAX_DB)) as i8;
	match band {
		Band::Bass => settings.bass = db,
		Band::Mid => settings.mid = db,
		Band::Treble => settings.treble = db,
	}
	settings.preset = EqPreset::Custom;
	settings.saved_name = None;
	(settings.custom_bass, settings.custom_mid, settings.custom_treble) =
		(settings.bass, settings.mid, settings.treble);
	settings.enabled = true;
}

/// A preset moves the bands with it; Custom restores its own stored
/// bands (the shipped bug: coming back to Custom kept whichever named
/// preset had overwritten the fields last, so the hand-tuned set was
/// gone). Choosing a preset never touches Enabled (Brandon's rule):
/// arrowing through presets to compare them must not switch the
/// equalizer on — the Enabled box is the user's switch alone.
pub fn apply_preset(settings: &mut EqSettings, preset: EqPreset) {
	settings.preset = preset;
	settings.saved_name = None;
	if preset == EqPreset::Custom {
		(settings.bass, settings.mid, settings.treble) = (
			settings.custom_bass,
			settings.custom_mid,
			settings.custom_treble,
		);
	} else if let Some((bass, mid, treble)) = preset.bands() {
		(settings.bass, settings.mid, settings.treble) = (bass, mid, treble);
	}
}

/// A saved preset applies like a built-in one and becomes the Custom
/// store too, so it is still there if the user then moves a band.
pub fn apply_saved(settings: &mut EqSettings, saved: &SavedEqPreset) {
	(settings.bass, settings.mid, settings.treble) = (saved.bass, saved.mid, saved.treble);
	(settings.custom_bass, settings.custom_mid, settings.custom_treble) =
		(saved.bass, saved.mid, saved.treble);
	settings.preset = EqPreset::Custom;
	settings.saved_name = Some(saved.name.clone());
}

/// Save the current bands under `name`, replacing a preset of the same
/// name. Returns false for an empty name.
pub fn save_preset(settings: &mut EqSettings, name: &str) -> bool {
	let name = name.trim();
	if name.is_empty() {
		return false;
	}
	let entry = SavedEqPreset {
		name: name.to_string(),
		bass: settings.bass,
		mid: settings.mid,
		treble: settings.treble,
	};
	if let Some(at) = settings.saved.iter().position(|p| p.name == name) {
		settings.saved[at] = entry;
	} else {
		settings.saved.push(entry);
	}
	settings.saved_name = Some(name.to_string());
	true
}

/// Remove the saved preset `name`; true when one was there.
pub fn delete_saved(settings: &mut EqSettings, name: &str) -> bool {
	let before = settings.saved.len();
	settings.saved.retain(|p| p.name != name);
	if settings.saved_name.as_deref() == Some(name) {
		settings.saved_name = None;
	}
	settings.saved.len() != before
}

/// Where the current settings sit in the choice list: the built-ins
/// first, then the saved presets in saved order.
pub fn selection_index(settings: &EqSettings) -> usize {
	if let Some(name) = &settings.saved_name
		&& let Some(at) = settings.saved.iter().position(|p| &p.name == name)
	{
		return PRESETS.len() + at;
	}
	PRESETS
		.iter()
		.position(|p| *p == settings.preset)
		.unwrap_or(0)
}

/// Fill the preset choice: the built-ins, then the saved presets, with
/// the selection on whatever the settings say is in use.
fn fill_presets(preset: &Choice, settings: &EqSettings) {
	preset.clear();
	for p in PRESETS {
		preset.append(p.label());
	}
	for p in &settings.saved {
		preset.append(&p.name);
	}
	preset.set_selection(selection_index(settings) as u32);
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
	fill_presets(&preset, &current);
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

	let name_label = StaticText::builder(&dialog)
		.with_label(s::PRESET_NAME)
		.build();
	let name = TextCtrl::builder(&dialog).build();
	name.set_value(current.saved_name.as_deref().unwrap_or(""));
	sizer.add(
		&name_label,
		0,
		SizerFlag::Left | SizerFlag::Right | SizerFlag::Top,
		5,
	);
	sizer.add(&name, 0, SizerFlag::Expand | SizerFlag::All, 5);
	let save = Button::builder(&dialog).with_label(s::SAVE_PRESET).build();
	sizer.add(&save, 0, SizerFlag::All, 5);
	let delete = Button::builder(&dialog)
		.with_label(s::DELETE_PRESET)
		.build();
	sizer.add(&delete, 0, SizerFlag::All, 5);

	let bass = slider(&dialog, &sizer, s::BASS, current.bass);
	let mid = slider(&dialog, &sizer, s::MID, current.mid);
	let treble = slider(&dialog, &sizer, s::TREBLE, current.treble);
	let close = Button::builder(&dialog)
		.with_id(ID_CANCEL)
		.with_label(s::CLOSE)
		.build();
	sizer.add(&close, 0, SizerFlag::All, 5);
	dialog.set_sizer(sizer, true);
	dialog.set_size(Size::new(420, 560));

	// Moving the sliders to follow a preset must not read back as a hand move.
	let programmatic = Rc::new(Cell::new(false));
	// Refilling the choice after a save or delete is not a user choice.
	let rebuilding = Rc::new(Cell::new(false));
	{
		let (a, programmatic, rebuilding) =
			(app.clone(), programmatic.clone(), rebuilding.clone());
		preset.on_selection_changed(move |_| {
			if rebuilding.get() {
				return;
			}
			let at = preset.get_selection().unwrap_or(0) as usize;
			let mut eq = a.borrow().settings.eq.clone();
			if at < PRESETS.len() {
				apply_preset(&mut eq, PRESETS[at]);
			} else if let Some(saved) = eq.saved.get(at - PRESETS.len()).cloned() {
				apply_saved(&mut eq, &saved);
			} else {
				return;
			}
			programmatic.set(true);
			bass.set_value(i32::from(eq.bass));
			mid.set_value(i32::from(eq.mid));
			treble.set_value(i32::from(eq.treble));
			programmatic.set(false);
			enabled.set_value(eq.enabled);
			name.set_value(eq.saved_name.as_deref().unwrap_or(""));
			let said = eq::describe(&eq);
			a.borrow_mut().set_eq(eq);
			a.borrow_mut().say(&said);
		});
	}

	{
		let (a, rebuilding) = (app.clone(), rebuilding.clone());
		save.on_click(move |_| {
			let text = name.get_value();
			let mut eq = a.borrow().settings.eq.clone();
			if !save_preset(&mut eq, &text) {
				a.borrow_mut().say("Type a name for the preset first.");
				return;
			}
			let said = format!(
				"Saved preset {}. {}",
				eq.saved_name.as_deref().unwrap_or_default(),
				eq::describe(&eq)
			);
			rebuilding.set(true);
			fill_presets(&preset, &eq);
			rebuilding.set(false);
			a.borrow_mut().set_eq(eq);
			a.borrow_mut().say(&said);
		});
	}

	{
		let (a, rebuilding) = (app.clone(), rebuilding.clone());
		delete.on_click(move |_| {
			let mut eq = a.borrow().settings.eq.clone();
			let Some(gone) = eq.saved_name.clone() else {
				a.borrow_mut()
					.say("The selected preset is not one of your saved presets.");
				return;
			};
			if !delete_saved(&mut eq, &gone) {
				return;
			}
			rebuilding.set(true);
			fill_presets(&preset, &eq);
			rebuilding.set(false);
			name.set_value("");
			a.borrow_mut().set_eq(eq);
			a.borrow_mut().say(&format!("Deleted preset {gone}."));
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
			enabled.set_value(eq.enabled);
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
			// Page Up and Page Down step one dB too: the native
			// trackbar page step is five, which Brandon hit as
			// "sliders move by 5, not by 1".
			let value = match code {
				WXK_HOME => -i32::from(eq::MAX_DB),
				WXK_END => i32::from(eq::MAX_DB),
				WXK_PAGEUP => slider.get_value() + 1,
				WXK_PAGEDOWN => slider.get_value() - 1,
				c => match key_step(c) {
					Some(step) => slider.get_value() + step,
					None => return e.skip(true),
				},
			};
			e.skip(false);
			slider.set_value(value);
			let mut eq = a.borrow().settings.eq.clone();
			set_band(&mut eq, band, value);
			enabled.set_value(eq.enabled);
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
			..EqSettings::default()
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

	#[test]
	fn custom_bands_survive_a_preset_round_trip() {
		// Brandon's bug: tune by hand, try Loudness, come back to
		// Custom — the hand-tuned set was gone.
		let mut eq = EqSettings::default();
		set_band(&mut eq, Band::Bass, 4);
		set_band(&mut eq, Band::Treble, -2);
		apply_preset(&mut eq, EqPreset::Loudness);
		assert_eq!((eq.bass, eq.mid, eq.treble), (8, -2, 6));
		apply_preset(&mut eq, EqPreset::Custom);
		assert_eq!((eq.bass, eq.mid, eq.treble), (4, 0, -2), "restored");
	}

	#[test]
	fn saved_presets_round_trip() {
		let mut eq = EqSettings::default();
		set_band(&mut eq, Band::Mid, 5);
		assert!(!save_preset(&mut eq, "  "), "no empty names");
		assert!(save_preset(&mut eq, "Headphones"));
		assert_eq!(eq.saved.len(), 1);
		assert_eq!(selection_index(&eq), PRESETS.len());
		apply_preset(&mut eq, EqPreset::Flat);
		assert_eq!(selection_index(&eq), 0);
		let saved = eq.saved[0].clone();
		apply_saved(&mut eq, &saved);
		assert_eq!((eq.bass, eq.mid, eq.treble), (0, 5, 0));
		assert_eq!(eq.saved_name.as_deref(), Some("Headphones"));
		// Saving again under the same name updates in place.
		set_band(&mut eq, Band::Bass, 2);
		assert!(save_preset(&mut eq, "Headphones"));
		assert_eq!(eq.saved.len(), 1);
		assert_eq!(eq.saved[0].bass, 2);
		assert!(delete_saved(&mut eq, "Headphones"));
		assert!(eq.saved.is_empty());
		assert!(!delete_saved(&mut eq, "Headphones"));
	}

	#[test]
	fn choosing_a_preset_never_enables_the_equalizer() {
		// Brandon's rule: arrowing through presets to compare them
		// kept checking Enabled on its own. The box is manual; only a
		// hand band move (a deliberate edit) still enables.
		let mut eq = EqSettings::default();
		assert!(!eq.enabled);
		apply_preset(&mut eq, EqPreset::BassBoost);
		assert!(!eq.enabled, "a chosen preset leaves Enabled alone");
		let saved = SavedEqPreset {
			name: "X".into(),
			bass: 1,
			mid: 2,
			treble: 3,
		};
		apply_saved(&mut eq, &saved);
		assert!(!eq.enabled, "a saved preset leaves Enabled alone");
		set_band(&mut eq, Band::Bass, 3);
		assert!(eq.enabled, "a hand move is heard at once");
	}
}
