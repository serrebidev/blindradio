//! Speaking through JAWS itself, with the `FreedomSci.JawsApi` object JAWS installs.
//!
//! UI Automation notifications are treated differently by each screen reader, so JAWS
//! gets told directly, the way NVDA does through its controller client.
//!
//! Windows-only and fully optional: JAWS not running or a failed call return false and
//! the caller falls back to the live region. COM is asked for nothing while JAWS is not
//! running, so this never starts it.

/// Speak `text` through JAWS; `interrupt` cancels what JAWS is saying first. True when
/// JAWS took the text.
#[cfg(windows)]
pub fn speak(text: &str, interrupt: bool) -> bool {
	imp::speak(text, interrupt)
}

#[cfg(not(windows))]
pub fn speak(_text: &str, _interrupt: bool) -> bool {
	false
}

#[cfg(windows)]
mod imp {
	use std::cell::RefCell;

	use windows::Win32::System::Com::{
		CLSCTX_ALL, CLSIDFromProgID, CoCreateInstance, DISPATCH_METHOD, DISPPARAMS, IDispatch,
	};
	use windows::Win32::System::Variant::VARIANT;
	use windows::Win32::UI::WindowsAndMessaging::FindWindowW;
	use windows::core::{GUID, PCWSTR, w};

	const LOCALE_USER_DEFAULT: u32 = 0x0400;

	thread_local! {
		/// The JAWS object and the number of its SayString method.
		static API: RefCell<Option<(IDispatch, i32)>> = const { RefCell::new(None) };
	}

	fn running() -> bool {
		unsafe { FindWindowW(w!("JFWUI2"), PCWSTR::null()) }.is_ok_and(|window| !window.is_invalid())
	}

	fn api() -> Option<(IDispatch, i32)> {
		API.with_borrow_mut(|api| {
			if api.is_none() {
				*api = unsafe {
					let class = CLSIDFromProgID(w!("FreedomSci.JawsApi")).ok()?;
					let dispatch: IDispatch = CoCreateInstance(&class, None, CLSCTX_ALL).ok()?;
					let name = w!("SayString");
					let mut id = 0;
					dispatch
						.GetIDsOfNames(&GUID::zeroed(), &name, 1, LOCALE_USER_DEFAULT, &mut id)
						.ok()?;
					Some((dispatch, id))
				};
			}
			api.clone()
		})
	}

	/// `SayString(text, flush)`: IDispatch takes arguments last first.
	fn say_string_arguments(text: &str, interrupt: bool) -> [VARIANT; 2] {
		[VARIANT::from(interrupt), VARIANT::from(text)]
	}

	pub fn speak(text: &str, interrupt: bool) -> bool {
		if !running() {
			return false;
		}
		let Some((dispatch, id)) = api() else {
			return false;
		};
		let mut arguments = say_string_arguments(text, interrupt);
		let parameters = DISPPARAMS {
			rgvarg: arguments.as_mut_ptr(),
			rgdispidNamedArgs: std::ptr::null_mut(),
			cArgs: 2,
			cNamedArgs: 0,
		};
		let said = unsafe {
			dispatch.Invoke(
				id,
				&GUID::zeroed(),
				LOCALE_USER_DEFAULT,
				DISPATCH_METHOD,
				&parameters,
				None,
				None,
				None,
			)
		};
		if said.is_err() {
			// JAWS may have restarted: ask for its object again next time.
			API.with_borrow_mut(|api| *api = None);
		}
		said.is_ok()
	}

	#[cfg(test)]
	mod tests {
		use windows::Win32::System::Variant::{VT_BOOL, VT_BSTR};

		use super::say_string_arguments;

		#[test]
		fn flush_goes_first_and_text_last() {
			let arguments = say_string_arguments("Saved", true);
			assert_eq!(arguments[0].vt(), VT_BOOL);
			assert!(bool::try_from(&arguments[0]).unwrap());
			assert_eq!(arguments[1].vt(), VT_BSTR);
			assert!(!bool::try_from(&say_string_arguments("Saved", false)[0]).unwrap());
		}
	}
}
