//! Keep idle sleep from interrupting playback or recording, without keeping the screen on.

use std::sync::atomic::{AtomicU64, Ordering};

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Power::{
	DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY, PowerCreateRequest,
	PowerRegisterSuspendResumeNotification, PowerRequestExecutionRequired,
	PowerRequestSystemRequired, PowerSetRequest, PowerUnregisterSuspendResumeNotification,
};
use windows::Win32::System::Threading::{
	POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
};
use windows::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMESUSPEND};
use windows::core::{Owned, PWSTR};

static RESUME_GENERATION: AtomicU64 = AtomicU64::new(0);

unsafe extern "system" fn resumed(
	_context: *const core::ffi::c_void,
	kind: u32,
	_setting: *const core::ffi::c_void,
) -> u32 {
	// Only a user resume renews requests; automatic maintenance wakes must not defeat battery policy.
	if kind == PBT_APMRESUMESUSPEND {
		RESUME_GENERATION.fetch_add(1, Ordering::Relaxed);
	}
	0
}

#[derive(Default)]
pub struct Awake {
	request: Option<Owned<HANDLE>>,
	active: bool,
	notification: Option<HPOWERNOTIFY>,
	resume_generation: u64,
}

impl Awake {
	pub fn update(&mut self, playing: bool, recording: bool) -> windows::core::Result<()> {
		let active = playing || recording;
		let generation = RESUME_GENERATION.load(Ordering::Relaxed);
		if self.active == active && self.resume_generation == generation {
			return Ok(());
		}
		// A failed activation is reported once; stopping and restarting permits another attempt.
		self.active = active;
		self.resume_generation = generation;
		self.request = None;
		if active {
			if self.notification.is_none() {
				let subscribe = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
					Callback: Some(resumed),
					Context: std::ptr::null_mut(),
				};
				let mut registration = std::ptr::null_mut();
				// The callback touches only a process-lifetime atomic, never App or a borrowed context.
				unsafe {
					PowerRegisterSuspendResumeNotification(
						DEVICE_NOTIFY_CALLBACK,
						HANDLE(
							(&subscribe as *const DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS)
								.cast_mut()
								.cast(),
						),
						&mut registration,
					)
					.ok()?;
				}
				self.notification = Some(HPOWERNOTIFY(registration as isize));
			}
			let mut reason: Vec<u16> = crate::strings::power_reason()
				.encode_utf16()
				.chain(Some(0))
				.collect();
			let context = REASON_CONTEXT {
				Version: 0,
				Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
				Reason: REASON_CONTEXT_0 {
					SimpleReasonString: PWSTR(reason.as_mut_ptr()),
				},
			};
			// Windows copies the reason; Owned closes even a partially activated request on error.
			let request = unsafe { Owned::new(PowerCreateRequest(&context)?) };
			unsafe {
				PowerSetRequest(*request, PowerRequestSystemRequired)?;
				PowerSetRequest(*request, PowerRequestExecutionRequired)?;
			}
			self.request = Some(request);
		}
		Ok(())
	}
}

impl Drop for Awake {
	fn drop(&mut self) {
		if let Some(notification) = self.notification.take() {
			let _ = unsafe { PowerUnregisterSuspendResumeNotification(notification) };
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use windows::Win32::UI::WindowsAndMessaging::PBT_APMRESUMEAUTOMATIC;
	static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
	use windows::Win32::System::Power::{
		PowerClearRequest, PowerRequestAwayModeRequired, PowerRequestDisplayRequired,
		PowerRequestExecutionRequired, PowerRequestSystemRequired, PowerSetRequest,
	};

	fn assert_requests(handle: HANDLE) {
		unsafe {
			for kind in [PowerRequestSystemRequired, PowerRequestExecutionRequired] {
				PowerClearRequest(handle, kind).expect("activity holds this request");
				assert!(
					PowerClearRequest(handle, kind).is_err(),
					"exactly one request"
				);
				PowerSetRequest(handle, kind).unwrap();
			}
			for kind in [PowerRequestDisplayRequired, PowerRequestAwayModeRequired] {
				assert!(
					PowerClearRequest(handle, kind).is_err(),
					"screen and explicit sleep stay available"
				);
			}
		}
	}

	#[test]
	fn activity_holds_one_request_until_both_stop() {
		let _lock = TEST_LOCK.lock().unwrap();
		let mut awake = Awake::default();
		awake.update(false, false).unwrap();
		assert!(awake.request.is_none());
		for (playing, recording) in [(true, false), (true, true), (false, true)] {
			awake.update(playing, recording).unwrap();
			let handle = awake
				.request
				.as_deref()
				.copied()
				.expect("playback or recording prevents idle sleep");
			assert_requests(handle);
			awake.update(playing, recording).unwrap();
			assert_eq!(
				awake.request.as_deref(),
				Some(&handle),
				"polls reuse the same request"
			);
			assert_requests(handle);
		}
		awake.update(false, false).unwrap();
		assert!(awake.request.is_none());
		awake.update(true, false).unwrap();
		assert_requests(**awake.request.as_ref().unwrap());
	}

	#[test]
	fn background_recording_can_start_without_playback() {
		let _lock = TEST_LOCK.lock().unwrap();
		let mut awake = Awake::default();
		awake.update(false, true).unwrap();
		let handle = awake
			.request
			.as_deref()
			.copied()
			.expect("background recording holds a request");
		assert_requests(handle);
		awake.update(false, false).unwrap();
		assert!(awake.request.is_none());
	}

	#[test]
	fn user_resume_renews_requests_cleared_by_windows() {
		let _lock = TEST_LOCK.lock().unwrap();
		let mut awake = Awake::default();
		awake.update(true, false).unwrap();
		let handle = **awake.request.as_ref().unwrap();
		unsafe {
			PowerClearRequest(handle, PowerRequestSystemRequired).unwrap();
			PowerClearRequest(handle, PowerRequestExecutionRequired).unwrap();
			resumed(std::ptr::null(), PBT_APMRESUMEAUTOMATIC, std::ptr::null());
		}
		awake.update(true, false).unwrap();
		unsafe {
			assert!(
				PowerClearRequest(handle, PowerRequestSystemRequired).is_err(),
				"automatic wakes do not renew requests"
			);
			resumed(std::ptr::null(), PBT_APMRESUMESUSPEND, std::ptr::null());
		}
		awake.update(true, false).unwrap();
		assert_requests(**awake.request.as_ref().unwrap());
	}
}
