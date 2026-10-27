use std::time::{Duration, Instant};

use blindradio_core::output::{DeviceEvent, Devices, output_devices};

fn names(list: &[&str]) -> Vec<String> {
	list.iter().map(|s| s.to_string()).collect()
}

/// Records every open; `ok` decides what each open of a named device returns (the default always opens).
fn opener<'a>(log: &'a mut Vec<Option<String>>, ok: bool) -> impl FnMut(Option<&str>) -> bool + 'a {
	move |d| {
		log.push(d.map(str::to_string));
		d.is_none() || ok
	}
}

#[test]
fn device_fallback_and_return() {
	let without = names(&["Speakers"]);
	let with = names(&["Speakers", "USB"]);
	let now = Instant::now();
	let mut log = Vec::new();
	let mut d = Devices::new(Some("USB"), Some("USB"));
	assert_eq!(
		d.poll(false, &without, now, opener(&mut log, true)),
		Some(DeviceEvent::Lost)
	);
	assert_eq!(d.active.as_deref(), None);
	// On the default after the loss, the chosen name is kept.
	assert_eq!(d.poll(false, &without, now, opener(&mut log, true)), None);
	assert_eq!(d.chosen.as_deref(), Some("USB"));
	assert_eq!(
		d.poll(false, &with, now, opener(&mut log, true)),
		Some(DeviceEvent::Back("USB".into()))
	);
	assert_eq!(d.poll(false, &with, now, opener(&mut log, true)), None);
	assert_eq!(log, [None, Some("USB".into())]);
	// No choice: always the default, nothing to fall back from.
	let mut d = Devices::new(None, None);
	assert_eq!(d.poll(false, &with, now, opener(&mut log, true)), None);
	assert_eq!(log.len(), 2);
}

#[test]
fn errored_but_listed_reopens_silently() {
	let with = names(&["Speakers", "USB"]);
	let mut log = Vec::new();
	let mut d = Devices::new(Some("USB"), Some("USB"));
	assert_eq!(
		d.poll(true, &with, Instant::now(), opener(&mut log, true)),
		None
	);
	assert_eq!(log, [Some("USB".to_string())]);
	assert_eq!(d.active.as_deref(), Some("USB"));
}

#[test]
fn errored_reopen_fails_lost_once_then_no_flapping() {
	let with = names(&["Speakers", "USB"]);
	let now = Instant::now();
	let mut log = Vec::new();
	let mut d = Devices::new(Some("USB"), Some("USB"));
	assert_eq!(
		d.poll(true, &with, now, opener(&mut log, false)),
		Some(DeviceEvent::Lost)
	);
	assert_eq!(log, [Some("USB".to_string()), None]);
	// Still listed and would open now, but it is held off: no Back, no reopen.
	for s in 1..15 {
		let t = now + Duration::from_secs(2 * s);
		assert_eq!(d.poll(false, &with, t, opener(&mut log, true)), None);
	}
	assert_eq!(log.len(), 2);
}

#[test]
fn held_device_back_after_leaving_and_returning() {
	let without = names(&["Speakers"]);
	let with = names(&["Speakers", "USB"]);
	let now = Instant::now();
	let mut log = Vec::new();
	let mut d = Devices::new(Some("USB"), Some("USB"));
	d.poll(true, &with, now, opener(&mut log, false));
	assert_eq!(d.poll(false, &without, now, opener(&mut log, true)), None);
	assert_eq!(
		d.poll(false, &with, now, opener(&mut log, true)),
		Some(DeviceEvent::Back("USB".into()))
	);
}

#[test]
fn held_device_back_after_30_s() {
	let with = names(&["Speakers", "USB"]);
	let now = Instant::now();
	let mut log = Vec::new();
	let mut d = Devices::new(Some("USB"), Some("USB"));
	d.poll(true, &with, now, opener(&mut log, false));
	let early = now + Duration::from_secs(29);
	assert_eq!(d.poll(false, &with, early, opener(&mut log, true)), None);
	let later = now + Duration::from_secs(30);
	assert_eq!(
		d.poll(false, &with, later, opener(&mut log, true)),
		Some(DeviceEvent::Back("USB".into()))
	);
}

#[test]
fn failed_return_is_held_too() {
	let with = names(&["Speakers", "USB"]);
	let now = Instant::now();
	let mut log = Vec::new();
	let mut d = Devices::new(Some("USB"), None);
	assert_eq!(d.poll(false, &with, now, opener(&mut log, false)), None);
	assert_eq!(d.poll(false, &with, now, opener(&mut log, false)), None);
	assert_eq!(log, [Some("USB".to_string())], "tried once, not every poll");
}

/// `Output::start(Some(name))` whose device fails to open plays through the default with this state.
#[test]
fn startup_on_default_returns_to_chosen() {
	let with = names(&["Speakers", "USB"]);
	let mut log = Vec::new();
	let mut d = Devices::new(Some("USB"), None);
	assert_eq!(
		d.poll(false, &with, Instant::now(), opener(&mut log, true)),
		Some(DeviceEvent::Back("USB".into()))
	);
}

#[test]
fn errored_default_reopens_default() {
	let with = names(&["Speakers", "USB"]);
	let without = names(&["Speakers"]);
	let now = Instant::now();
	let mut log = Vec::new();
	let mut d = Devices::new(None, None);
	assert_eq!(d.poll(true, &with, now, opener(&mut log, true)), None);
	assert_eq!(log, [None]);
	// On the default while the chosen device is away.
	let mut d = Devices::new(Some("USB"), None);
	assert_eq!(d.poll(true, &without, now, opener(&mut log, true)), None);
	assert_eq!(log, [None, None]);
}

#[test]
#[ignore = "needs a sound card"]
fn lists_real_devices() {
	let list = output_devices();
	println!("{list:?}");
	assert!(!list.is_empty());
}
