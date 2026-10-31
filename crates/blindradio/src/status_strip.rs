//! Windows only: the status bar lives in its own borderless window
//! glued to the main frame's bottom edge.
//!
//! Why: NVDA's status line command (NVDA+End) asks for the object at
//! the foreground window's bottom-left corner and walks UP from it to
//! a status bar (source/api.py getStatusBar: objectFromPoint(left,
//! top+height-1), then parents until role STATUSBAR). A status bar
//! inside the frame is a child window clipped to the client area, and
//! the corner point sits on the frame's border, below and left of the
//! client area, so the walk starts at the frame itself and never
//! reaches the bar — "Status line not found", however the in-frame
//! bar is built or raised (tried in 0.2.16 and 0.2.19). Brandon's
//! rule: no NVDA add-on, it has to just work. So the bar's window
//! covers the frame's whole bottom edge, border included.
//!
//! The strip must be TRULY borderless — 0.2.20/0.2.21 shipped it as
//! a framed window, and the corner point landed on the strip's own
//! bottom border instead of the bar. build() strips every frame bit
//! from the window style, so the window rect is the client rect and
//! the bar (a native msctls_statusbar32 under wxMSW, whose own MSAA
//! identity is a textbook status bar) fills it corner to corner. The
//! strip is an owned window (always just above its owner), never
//! activates, and the 50 ms poll keeps it glued.

use std::sync::Mutex;

use wxdragon::prelude::*;

use crate::main_window::MainWindow;

type Hwnd = *mut std::ffi::c_void;

#[repr(C)]
#[derive(Default)]
struct Rect {
	left: i32,
	top: i32,
	right: i32,
	bottom: i32,
}

#[repr(C)]
#[derive(Default)]
struct Point {
	x: i32,
	y: i32,
}

const GWL_STYLE: i32 = -16;
const GWL_EXSTYLE: i32 = -20;
const GWLP_HWNDPARENT: i32 = -8;
const WS_CHILD: isize = 0x4000_0000;
const WS_POPUP: isize = 0x8000_0000;
const WS_CAPTION: isize = 0x00C0_0000;
const WS_THICKFRAME: isize = 0x0004_0000;
const WS_SYSMENU: isize = 0x0008_0000;
const WS_MINIMIZEBOX: isize = 0x0002_0000;
const WS_MAXIMIZEBOX: isize = 0x0001_0000;
const WS_EX_DLGMODALFRAME: isize = 0x0000_0001;
const WS_EX_WINDOWEDGE: isize = 0x0000_0100;
const WS_EX_CLIENTEDGE: isize = 0x0000_0200;
const WS_EX_STATICEDGE: isize = 0x0002_0000;
const WS_EX_NOACTIVATE: isize = 0x0800_0000;
const SW_HIDE: i32 = 0;
const SW_SHOWNA: i32 = 8;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_FRAMECHANGED: u32 = 0x0020;

#[link(name = "user32")]
unsafe extern "system" {
	fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
	fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
	fn ClientToScreen(hwnd: Hwnd, point: *mut Point) -> i32;
	fn SetWindowPos(hwnd: Hwnd, after: Hwnd, x: i32, y: i32, cx: i32, cy: i32, flags: u32) -> i32;
	fn ShowWindow(hwnd: Hwnd, cmd: i32) -> i32;
	fn IsIconic(hwnd: Hwnd) -> i32;
	fn IsWindowVisible(hwnd: Hwnd) -> i32;
	fn GetWindowLongPtrW(hwnd: Hwnd, index: i32) -> isize;
	fn SetWindowLongPtrW(hwnd: Hwnd, index: i32, value: isize) -> isize;
	fn SetParent(hwnd: Hwnd, parent: Hwnd) -> Hwnd;
}

/// Create the strip and its status bar for `frame`. Returns the strip
/// (kept alive in MainWindow), the bar itself — the usual status_bar
/// field, so every status text call site is unchanged — and the
/// height the frame's layout must leave free at its bottom, where
/// the strip overlays the panel.
pub fn build(frame: &Frame) -> (Option<Frame>, StatusBar, i32) {
	let strip = Frame::builder()
		.with_parent(frame)
		.with_title("")
		.with_size(Size::new(640, 30))
		.with_style(FrameStyle::NoTaskbar)
		.build();
	let bar = strip.create_status_bar(1, 0x10131, ID_ANY as i32, "statusBar");
	let strip_hwnd = strip.get_handle();
	uia::install(bar.get_handle(), strip_hwnd);
	uia::install(strip_hwnd, strip_hwnd);
	let frame_hwnd = frame.get_handle();
	// SAFETY: both handles are live windows of this thread. wx may
	// have made the strip a child of the frame; a child is clipped to
	// the client area and useless here, so force a top-level owned
	// window, owned by the frame, that never activates.
	unsafe {
		if GetWindowLongPtrW(strip_hwnd, GWL_STYLE) & WS_CHILD != 0 {
			SetParent(strip_hwnd, std::ptr::null_mut());
		}
		SetWindowLongPtrW(strip_hwnd, GWLP_HWNDPARENT, frame_hwnd as isize);
		// No chrome at all — this is the fix 0.2.20/0.2.21 missed.
		// The strip was a framed window, and NVDA's hit point (the
		// foreground window's bottom-left corner) lands on the
		// strip's bottom row, which was the frame's own border:
		// objectFromPoint returned the strip frame (a pane), its
		// ancestry holds no status bar, and the walk failed before
		// it ever reached the bar inside. Borderless, the window
		// rect IS the client rect, the bar (a real msctls_statusbar32
		// under wxMSW, with a native status-bar MSAA identity) fills
		// it, and the corner point lands on the bar itself.
		let style = GetWindowLongPtrW(strip_hwnd, GWL_STYLE);
		let bare = (style
			& !(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX))
			| WS_POPUP;
		SetWindowLongPtrW(strip_hwnd, GWL_STYLE, bare);
		let ex = GetWindowLongPtrW(strip_hwnd, GWL_EXSTYLE);
		let bare_ex = (ex
			& !(WS_EX_DLGMODALFRAME | WS_EX_WINDOWEDGE | WS_EX_CLIENTEDGE | WS_EX_STATICEDGE))
			| WS_EX_NOACTIVATE;
		SetWindowLongPtrW(strip_hwnd, GWL_EXSTYLE, bare_ex);
		SetWindowPos(
			strip_hwnd,
			std::ptr::null_mut(),
			0,
			0,
			0,
			0,
			SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
		);
	}
	let pad = bar.get_size().height.max(20) + bottom_border(frame);
	(Some(strip), bar, pad)
}

/// The frame's bottom border thickness: window rect bottom minus
/// client area bottom, in screen pixels.
fn bottom_border(frame: &Frame) -> i32 {
	let hwnd = frame.get_handle();
	unsafe {
		let mut window = Rect::default();
		let mut client = Rect::default();
		if GetWindowRect(hwnd, &mut window) == 0 || GetClientRect(hwnd, &mut client) == 0 {
			return 0;
		}
		let mut origin = Point::default();
		if ClientToScreen(hwnd, &mut origin) == 0 {
			return 0;
		}
		(window.bottom - (origin.y + client.bottom)).max(0)
	}
}

/// Glue the strip to the frame's bottom edge; hide it while the
/// frame is hidden or minimized. Runs every poll (50 ms).
pub fn sync(win: &MainWindow) {
	let Some(strip) = &win.status_strip else {
		return;
	};
	let frame_hwnd = win.frame.get_handle();
	let strip_hwnd = strip.get_handle();
	// SAFETY: live windows of this thread; all reads are plain rects.
	unsafe {
		let mut window = Rect::default();
		if GetWindowRect(frame_hwnd, &mut window) == 0 {
			return;
		}
		if IsWindowVisible(frame_hwnd) == 0 || IsIconic(frame_hwnd) != 0 {
			ShowWindow(strip_hwnd, SW_HIDE);
			return;
		}
		// The strip is exactly the bar's height now: borderless, the
		// bar fills the whole window, top row to the corner row.
		let height = win.status_bar.get_size().height.max(1);
		let mut current = Rect::default();
		GetWindowRect(strip_hwnd, &mut current);
		if current.left != window.left
			|| current.right != window.right
			|| current.bottom != window.bottom
			|| current.top != window.bottom - height
		{
			SetWindowPos(
				strip_hwnd,
				std::ptr::null_mut(),
				window.left,
				window.bottom - height,
				window.right - window.left,
				height,
				SWP_NOACTIVATE,
			);
		}
		// Visibility is raw Win32 only, never wx's Show: showing a
		// frame through wx activates it, and 0.2.21's wx show here
		// stole the foreground from the main window at startup (and
		// after every restore) — NVDA announced the nameless strip
		// ("Pane") and the station tree lost focus until Alt+Tab.
		// SW_SHOWNA shows without activating, as 0.2.20 did.
		ShowWindow(strip_hwnd, SW_SHOWNA);
	}
}

/// The current status line, mirrored for the UIA provider's Name
/// (NVDA's status text assembly reads the bar object's name first).
static TEXT: Mutex<String> = Mutex::new(String::new());

pub fn set_text(text: &str) {
	if let Ok(mut held) = TEXT.lock() {
		*held = text.to_string();
	}
}

fn current_text() -> Vec<u16> {
	TEXT.lock()
		.map(|held| held.encode_utf16().collect())
		.unwrap_or_default()
}

/// A UIA provider for the strip's windows, hand-rolled COM.
///
/// The bar's own MSAA identity is the primary path (see the module
/// comment), but if NVDA asks UIA for the element at that corner,
/// the element comes from a provider — and the system proxy for the
/// bar reports a non-native element NVDA then ignores in favour of
/// MSAA. These providers answer instead, one for the bar window and
/// one for the strip window itself: control type Status Bar, name =
/// the live status line, and a bounding rectangle covering the whole
/// strip — the frame's bottom edge, corner included — so an
/// element-from-point query lands on a status bar directly.
mod uia {
	use std::ffi::c_void;
	use std::sync::atomic::{AtomicU32, Ordering};

	use super::{GetWindowRect, Hwnd, Rect, current_text};

	const WM_GETOBJECT: u32 = 0x003D;
	const UIA_ROOT_OBJECT_ID: isize = -25;
	const S_OK: i32 = 0;
	const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
	const E_POINTER: i32 = 0x8000_4003u32 as i32;

	type SubclassProc =
		unsafe extern "system" fn(Hwnd, u32, usize, isize, usize, usize) -> isize;

	#[link(name = "comctl32")]
	unsafe extern "system" {
		fn SetWindowSubclass(hwnd: Hwnd, proc_: SubclassProc, id: usize, data: usize) -> i32;
		fn DefSubclassProc(hwnd: Hwnd, msg: u32, wparam: usize, lparam: isize) -> isize;
	}

	#[link(name = "uiautomationcore")]
	unsafe extern "system" {
		fn UiaReturnRawElementProvider(
			hwnd: Hwnd,
			wparam: usize,
			lparam: isize,
			provider: *mut c_void,
		) -> isize;
		fn UiaHostProviderFromHwnd(hwnd: Hwnd, out: *mut *mut c_void) -> i32;
	}

	#[link(name = "oleaut32")]
	unsafe extern "system" {
		fn SysAllocStringLen(chars: *const u16, len: u32) -> *mut u16;
	}

	#[repr(C)]
	#[derive(PartialEq, Eq)]
	struct Guid {
		d1: u32,
		d2: u16,
		d3: u16,
		d4: [u8; 8],
	}

	const IID_IUNKNOWN: Guid = Guid {
		d1: 0x0000_0000,
		d2: 0x0000,
		d3: 0x0000,
		d4: [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
	};
	// IRawElementProviderSimple {d6dd68d1-86fd-4332-8666-9abedea2d24c}
	const IID_SIMPLE: Guid = Guid {
		d1: 0xd6dd_68d1,
		d2: 0x86fd,
		d3: 0x4332,
		d4: [0x86, 0x66, 0x9a, 0xbe, 0xde, 0xa2, 0xd2, 0x4c],
	};
	// IRawElementProviderFragment {f7063da8-8359-439c-9297-bbc5299a7d87}
	const IID_FRAGMENT: Guid = Guid {
		d1: 0xf706_3da8,
		d2: 0x8359,
		d3: 0x439c,
		d4: [0x92, 0x97, 0xbb, 0xc5, 0x29, 0x9a, 0x7d, 0x87],
	};

	/// The OLE VARIANT, 24 bytes on 64-bit Windows.
	#[repr(C)]
	struct Variant {
		vt: u16,
		r1: u16,
		r2: u16,
		r3: u16,
		lo: usize,
		hi: usize,
	}

	impl Variant {
		fn empty() -> Variant {
			Variant {
				vt: 0,
				r1: 0,
				r2: 0,
				r3: 0,
				lo: 0,
				hi: 0,
			}
		}
		fn i4(v: i32) -> Variant {
			Variant {
				vt: 3,
				lo: v as i64 as usize,
				..Variant::empty()
			}
		}
		fn bstr(text: &[u16]) -> Variant {
			// SAFETY: SysAllocStringLen copies the text; ownership of
			// the BSTR passes to the caller, per COM rules.
			let ptr = unsafe { SysAllocStringLen(text.as_ptr(), text.len() as u32) };
			Variant {
				vt: 8,
				lo: ptr as usize,
				..Variant::empty()
			}
		}
	}

	/// UIA's bounding rectangle is in doubles.
	#[repr(C)]
	struct UiaRect {
		left: f64,
		top: f64,
		width: f64,
		height: f64,
	}

	type QiFn =
		unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
	type CountFn = unsafe extern "system" fn(*mut c_void) -> u32;

	#[repr(C)]
	struct SimpleVtbl {
		qi: QiFn,
		add_ref: CountFn,
		release: CountFn,
		provider_options: unsafe extern "system" fn(*mut c_void, *mut i32) -> i32,
		get_pattern: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32,
		get_property: unsafe extern "system" fn(*mut c_void, i32, *mut Variant) -> i32,
		host_provider: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
	}

	#[repr(C)]
	struct FragmentVtbl {
		qi: QiFn,
		add_ref: CountFn,
		release: CountFn,
		navigate: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> i32,
		runtime_id: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
		bounding_rect: unsafe extern "system" fn(*mut c_void, *mut UiaRect) -> i32,
		embedded_roots: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
		set_focus: unsafe extern "system" fn(*mut c_void) -> i32,
		fragment_root: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> i32,
	}

	/// One allocation, two interfaces: Simple at offset 0, Fragment
	/// behind the second vtable pointer. Lives for the app's whole
	/// run (the reference count never reaches zero by design), like
	/// the window it serves. One provider serves the bar window and
	/// a second the strip window itself, so whichever of the two a
	/// UIA element-from-point lands on, it reports a status bar.
	struct Provider {
		simple_vtbl: *const SimpleVtbl,
		fragment_vtbl: *const FragmentVtbl,
		refs: AtomicU32,
		hwnd: Hwnd,
		strip: Hwnd,
	}

	// SAFETY: the provider is only ever touched through COM calls on
	// UI threads, and every mutable field is atomic or read-only.
	unsafe impl Send for Provider {}
	unsafe impl Sync for Provider {}

	fn from_simple(this: *mut c_void) -> *mut Provider {
		this as *mut Provider
	}

	fn from_fragment(this: *mut c_void) -> *mut Provider {
		// SAFETY: the fragment interface pointer addresses the second
		// field of the allocation.
		unsafe { (this as *mut u8).sub(8) as *mut Provider }
	}

	fn fragment_iface(p: *mut Provider) -> *mut c_void {
		// SAFETY: the fragment interface pointer addresses the second
		// field of the allocation.
		unsafe { (p as *mut u8).add(8) as *mut c_void }
	}

	// AddRef/Release exist per interface (the fragment wrappers
	// recover the base through from_fragment), because several
	// providers are alive at once and no global can tell two
	// interface pointers apart.

	unsafe fn qi_on(p: *mut Provider, iid: *const Guid, out: *mut *mut c_void) -> i32 {
		// SAFETY: iid points at a caller-owned GUID; out was checked.
		let iid = unsafe { &*iid };
		let iface = if *iid == IID_IUNKNOWN || *iid == IID_SIMPLE {
			p as *mut c_void
		} else if *iid == IID_FRAGMENT {
			fragment_iface(p)
		} else {
			unsafe { *out = std::ptr::null_mut() };
			return E_NOINTERFACE;
		};
		// SAFETY: out is a caller-owned slot.
		unsafe {
			(*p).refs.fetch_add(1, Ordering::Relaxed);
			*out = iface;
		}
		S_OK
	}

	unsafe extern "system" fn qi_simple(
		this: *mut c_void,
		iid: *const Guid,
		out: *mut *mut c_void,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// SAFETY: `this` is the Simple interface of a live provider.
		unsafe { qi_on(from_simple(this), iid, out) }
	}

	unsafe extern "system" fn qi_fragment(
		this: *mut c_void,
		iid: *const Guid,
		out: *mut *mut c_void,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// SAFETY: `this` is the Fragment interface of a live provider.
		unsafe { qi_on(from_fragment(this), iid, out) }
	}

	unsafe fn add_ref_on(p: *mut Provider) -> u32 {
		// SAFETY: p is a live provider.
		unsafe { (*p).refs.fetch_add(1, Ordering::Relaxed) + 1 }
	}

	unsafe fn release_on(p: *mut Provider) -> u32 {
		// The provider is never freed: one reference is held for the
		// app's lifetime, and the count floors at it.
		// SAFETY: p is a live provider.
		let refs = unsafe { &(*p).refs };
		let mut cur = refs.load(Ordering::Relaxed);
		while cur > 1 {
			match refs.compare_exchange(cur, cur - 1, Ordering::Relaxed, Ordering::Relaxed) {
				Ok(_) => return cur - 1,
				Err(seen) => cur = seen,
			}
		}
		1
	}

	unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
		// SAFETY: `this` is the provider's Simple interface.
		unsafe { add_ref_on(from_simple(this)) }
	}

	unsafe extern "system" fn release(this: *mut c_void) -> u32 {
		// SAFETY: `this` is the provider's Simple interface.
		unsafe { release_on(from_simple(this)) }
	}

	unsafe extern "system" fn add_ref_fragment(this: *mut c_void) -> u32 {
		// SAFETY: `this` is the provider's Fragment interface.
		unsafe { add_ref_on(from_fragment(this)) }
	}

	unsafe extern "system" fn release_fragment(this: *mut c_void) -> u32 {
		// SAFETY: `this` is the provider's Fragment interface.
		unsafe { release_on(from_fragment(this)) }
	}

	unsafe extern "system" fn provider_options(
		_this: *mut c_void,
		out: *mut i32,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// SAFETY: out is a caller-owned slot. 1 = server-side provider.
		unsafe { *out = 1 };
		S_OK
	}

	unsafe extern "system" fn get_pattern(
		_this: *mut c_void,
		_pattern: i32,
		out: *mut *mut c_void,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// SAFETY: out is a caller-owned slot. No patterns are served.
		unsafe { *out = std::ptr::null_mut() };
		S_OK
	}

	unsafe extern "system" fn get_property(
		_this: *mut c_void,
		property: i32,
		out: *mut Variant,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// Property ids: ControlType 30003, LocalizedControlType
		// 30004, Name 30005. Control type 50026 is StatusBar.
		let value = match property {
			30003 => Variant::i4(50026),
			30004 => Variant::bstr(&"status bar".encode_utf16().collect::<Vec<_>>()),
			30005 => Variant::bstr(&current_text()),
			_ => Variant::empty(),
		};
		// SAFETY: out is a caller-owned VARIANT slot.
		unsafe { *out = value };
		S_OK
	}

	unsafe extern "system" fn host_provider(
		this: *mut c_void,
		out: *mut *mut c_void,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		let p = from_simple(this);
		// SAFETY: delegates to UIA's own host provider for the window.
		unsafe { UiaHostProviderFromHwnd((*p).hwnd, out) }
	}

	unsafe extern "system" fn navigate(
		this: *mut c_void,
		direction: i32,
		out: *mut *mut c_void,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// SAFETY: out is a caller-owned slot.
		unsafe { *out = std::ptr::null_mut() };
		if direction != 0 {
			return S_OK; // Only Parent (0) is answered; no siblings or children.
		}
		// The parent comes from the host provider's fragment, the
		// standard stitch: ask it where its parent is.
		let p = from_fragment(this);
		// SAFETY: every handle names a live window of this process.
		unsafe {
			let mut host: *mut c_void = std::ptr::null_mut();
			if UiaHostProviderFromHwnd((*p).hwnd, &mut host) != S_OK || host.is_null() {
				return S_OK;
			}
			let mut host_fragment: *mut c_void = std::ptr::null_mut();
			let qi = (*(host as *const SimpleVtbl)).qi;
			if qi(host, &IID_FRAGMENT, &mut host_fragment) == S_OK && !host_fragment.is_null() {
				let nav = (*(host_fragment as *const FragmentVtbl)).navigate;
				nav(host_fragment, 0, out);
				let rel = (*(host_fragment as *const FragmentVtbl)).release;
				rel(host_fragment);
			}
			let rel = (*(host as *const SimpleVtbl)).release;
			rel(host);
		}
		S_OK
	}

	unsafe extern "system" fn runtime_id(
		_this: *mut c_void,
		out: *mut *mut c_void,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// SAFETY: null means UIA falls back to the host's runtime id.
		unsafe { *out = std::ptr::null_mut() };
		S_OK
	}

	unsafe extern "system" fn bounding_rect(this: *mut c_void, out: *mut UiaRect) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		let p = from_fragment(this);
		// SAFETY: the strip handle is a live window; the rect claims
		// the strip's whole area for the bar (see the module comment).
		unsafe {
			let mut r = Rect::default();
			if GetWindowRect((*p).strip, &mut r) == 0 {
				GetWindowRect((*p).hwnd, &mut r);
			}
			*out = UiaRect {
				left: r.left as f64,
				top: r.top as f64,
				width: (r.right - r.left) as f64,
				height: (r.bottom - r.top) as f64,
			};
		}
		S_OK
	}

	unsafe extern "system" fn embedded_roots(
		_this: *mut c_void,
		out: *mut *mut c_void,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// SAFETY: out is a caller-owned slot; there are no roots.
		unsafe { *out = std::ptr::null_mut() };
		S_OK
	}

	unsafe extern "system" fn set_focus(_this: *mut c_void) -> i32 {
		S_OK
	}

	unsafe extern "system" fn fragment_root(
		this: *mut c_void,
		out: *mut *mut c_void,
	) -> i32 {
		if out.is_null() {
			return E_POINTER;
		}
		// This provider is the root of its window's fragment.
		let p = from_fragment(this);
		// SAFETY: out is a caller-owned slot.
		unsafe {
			(*p).refs.fetch_add(1, Ordering::Relaxed);
			*out = this;
		}
		S_OK
	}

	static SIMPLE_VTBL: SimpleVtbl = SimpleVtbl {
		qi: qi_simple,
		add_ref,
		release,
		provider_options,
		get_pattern,
		get_property,
		host_provider,
	};

	static FRAGMENT_VTBL: FragmentVtbl = FragmentVtbl {
		qi: qi_fragment,
		add_ref: add_ref_fragment,
		release: release_fragment,
		navigate,
		runtime_id,
		bounding_rect,
		embedded_roots,
		set_focus,
		fragment_root,
	};

	unsafe extern "system" fn subclass_proc(
		hwnd: Hwnd,
		msg: u32,
		wparam: usize,
		lparam: isize,
		_id: usize,
		data: usize,
	) -> isize {
		if msg == WM_GETOBJECT && lparam == UIA_ROOT_OBJECT_ID {
			// SAFETY: data is the provider base set at install; the
			// interface UIA wants as the root is the fragment one.
			let provider = data as *mut Provider;
			return unsafe {
				UiaReturnRawElementProvider(hwnd, wparam, lparam, fragment_iface(provider))
			};
		}
		// SAFETY: everything else takes the window's normal path.
		unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
	}

	/// Serve a provider for `hwnd` (the bar window, and the strip
	/// window itself). Called from the strip's construction, once
	/// per window; each call creates that window's own provider.
	pub fn install(hwnd: Hwnd, strip: Hwnd) {
		let provider = Box::new(Provider {
			simple_vtbl: &SIMPLE_VTBL,
			fragment_vtbl: &FRAGMENT_VTBL,
			refs: AtomicU32::new(1),
			hwnd,
			strip,
		});
		let raw = Box::into_raw(provider);
		// SAFETY: hwnd is a live window of this thread; the subclass
		// only intercepts the UIA root-object request.
		unsafe {
			SetWindowSubclass(hwnd, subclass_proc, 1, raw as usize);
		}
	}
}
