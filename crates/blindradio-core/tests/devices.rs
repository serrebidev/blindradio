use blindradio_core::output::{FallbackAction, fallback_step, output_devices};

fn names(list: &[&str]) -> Vec<String> {
	list.iter().map(|s| s.to_string()).collect()
}

#[test]
fn device_fallback_and_return() {
	let without = names(&["Speakers"]);
	let with = names(&["Speakers", "USB"]);
	assert_eq!(
		fallback_step(Some("USB"), Some("USB"), false, &without),
		Some(FallbackAction::UseDefault)
	);
	// On the default after the loss, the chosen name is kept.
	assert_eq!(fallback_step(Some("USB"), None, false, &without), None);
	assert_eq!(
		fallback_step(Some("USB"), None, false, &with),
		Some(FallbackAction::UseChosen)
	);
	assert_eq!(fallback_step(Some("USB"), Some("USB"), false, &with), None);
	// No choice: always the default, nothing to fall back from.
	assert_eq!(fallback_step(None, None, false, &without), None);
	assert_eq!(fallback_step(None, None, false, &with), None);
}

#[test]
fn errored_device_falls_back() {
	let with = names(&["Speakers", "USB"]);
	assert_eq!(
		fallback_step(Some("USB"), Some("USB"), true, &with),
		Some(FallbackAction::UseDefault)
	);
}

#[test]
#[ignore = "needs a sound card"]
fn lists_real_devices() {
	let list = output_devices();
	println!("{list:?}");
	assert!(!list.is_empty());
}
