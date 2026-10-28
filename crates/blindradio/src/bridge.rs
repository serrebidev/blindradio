//! Runs async work on the engine runtime and hands each result back on the UI thread.
#![cfg_attr(
	not(test),
	expect(
		dead_code,
		reason = "Search and Browse spawn through it (Tasks 3 and 4)"
	)
)]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crossbeam_channel::{Receiver, Sender};
use tokio::runtime::Handle;

/// Open while its window is; `close` when the window closes, so late results for it are dropped.
#[derive(Clone, Default)]
pub struct WindowToken(Rc<Cell<bool>>);

impl WindowToken {
	pub fn new() -> WindowToken {
		WindowToken(Rc::new(Cell::new(true)))
	}

	pub fn close(&self) {
		self.0.set(false);
	}

	pub fn is_open(&self) -> bool {
		self.0.get()
	}
}

type Done<C> = Box<dyn FnOnce(&mut C, Box<dyn Any + Send>)>;

/// `C` is the `App`; generic so the delivery rules are testable without one.
pub struct Bridge<C> {
	rt: Handle,
	tx: Sender<(u64, Box<dyn Any + Send>)>,
	rx: Receiver<(u64, Box<dyn Any + Send>)>,
	waiting: RefCell<HashMap<u64, (WindowToken, Done<C>)>>,
	next: Cell<u64>,
}

impl<C> Bridge<C> {
	pub fn new(rt: Handle) -> Bridge<C> {
		let (tx, rx) = crossbeam_channel::unbounded();
		Bridge {
			rt,
			tx,
			rx,
			waiting: RefCell::new(HashMap::new()),
			next: Cell::new(0),
		}
	}

	/// `done` runs in a later `ready` call, on the UI thread, only if `owner` is still open.
	pub fn spawn<T: Send + 'static>(
		&self,
		owner: WindowToken,
		fut: impl Future<Output = T> + Send + 'static,
		done: impl FnOnce(&mut C, T) + 'static,
	) {
		let id = self.next.get();
		self.next.set(id + 1);
		let done: Done<C> = Box::new(move |c, any| match any.downcast::<T>() {
			Ok(t) => done(c, *t),
			Err(_) => debug_assert!(false, "bridge result of the wrong type"),
		});
		self.waiting.borrow_mut().insert(id, (owner, done));
		let tx = self.tx.clone();
		drop(self.rt.spawn(async move {
			let _ = tx.send((id, Box::new(fut.await) as Box<dyn Any + Send>));
		}));
	}

	/// The callbacks of every result that arrived for a window still open; the caller runs them.
	/// Each re-checks its window when run: an earlier callback in the batch may have closed it.
	pub fn ready(&self) -> Vec<impl FnOnce(&mut C) + use<C>> {
		let mut waiting = self.waiting.borrow_mut();
		self.rx
			.try_iter()
			.filter_map(|(id, value)| {
				let (owner, done) = waiting.remove(&id)?;
				owner.is_open().then_some(move |c: &mut C| {
					if owner.is_open() {
						done(c, value);
					}
				})
			})
			.collect()
	}

	#[cfg(test)]
	fn idle(&self) -> bool {
		self.waiting.borrow().is_empty()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::time::{Duration, Instant};

	#[test]
	fn late_result_after_close_dropped() {
		let rt = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(1)
			.enable_all()
			.build()
			.unwrap();
		let bridge: Bridge<Vec<&str>> = Bridge::new(rt.handle().clone());
		let closed = WindowToken::new();
		let open = WindowToken::new();
		bridge.spawn(
			closed.clone(),
			async {
				tokio::time::sleep(Duration::from_millis(50)).await;
				1
			},
			|got, _| got.push("closed"),
		);
		bridge.spawn(open.clone(), async { "x" }, |got, v| got.push(v));
		// The window closes while its result is in flight.
		closed.close();
		let mut got = Vec::new();
		let deadline = Instant::now() + Duration::from_secs(5);
		while !bridge.idle() && Instant::now() < deadline {
			for f in bridge.ready() {
				f(&mut got);
			}
			std::thread::sleep(Duration::from_millis(5));
		}
		assert!(bridge.idle(), "both results arrived");
		assert_eq!(got, ["x"], "only the open window's result is delivered");
	}

	#[test]
	fn closed_by_earlier_result_in_batch_dropped() {
		let rt = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(1)
			.build()
			.unwrap();
		let bridge: Bridge<Vec<&str>> = Bridge::new(rt.handle().clone());
		let first = WindowToken::new();
		let second = WindowToken::new();
		let closes = second.clone();
		bridge.spawn(first, async {}, move |got, ()| {
			got.push("first");
			closes.close();
		});
		bridge.spawn(second, async {}, |got, ()| got.push("second"));
		let deadline = Instant::now() + Duration::from_secs(5);
		while bridge.rx.len() < 2 && Instant::now() < deadline {
			std::thread::sleep(Duration::from_millis(5));
		}
		let ready = bridge.ready();
		assert_eq!(ready.len(), 2, "both results in one batch");
		let mut got = Vec::new();
		for f in ready {
			f(&mut got);
		}
		assert_eq!(
			got,
			["first"],
			"the window closed by the first result gets nothing"
		);
	}
}
