//! Windows only: the status bar lives in its own borderless window
//! glued to the main frame's bottom edge.
//!
//! Why: NVDA's status line command (NVDA+End) asks for the object at
//! the foreground window's bottom-left corner and walks UP from it to
//! a status bar. A status bar inside the frame is a child window
//! clipped to the client area, and the corner point sits on the
//! frame's border, below and left of the client area, so the walk
//! starts at the frame itself and never reaches the bar — "Status
//! line not found", however the in-frame bar is built or raised
//! (tried in 0.2.16 and 0.2.19). Brandon's rule: no NVDA add-on, it
//! has to just work. So the bar's window covers the frame's whole
//! bottom edge, border included: the corner point lands on the bar
//! itself. The strip is an owned window (always just above its
//! owner), never activates, and the 50 ms poll keeps it glued.

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
const WS_EX_NOACTIVATE: isize = 0x0800_0000;
const SW_HIDE: i32 = 0;
const SW_SHOWNA: i32 = 8;
const SWP_NOACTIVATE: u32 = 0x0010;

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
		let ex = GetWindowLongPtrW(strip_hwnd, GWL_EXSTYLE);
		SetWindowLongPtrW(strip_hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE);
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
		let height = win.status_bar.get_size().height.max(1) + bottom_border(&win.frame);
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
		ShowWindow(strip_hwnd, SW_SHOWNA);
	}
}
