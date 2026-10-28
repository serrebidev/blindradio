//! One copy at a time: a second launch raises the first and exits.

use std::sync::mpsc::{Receiver, channel};

use wxdragon::prelude::*;

/// The mutex and the IPC service both carry this, so it is per user.
pub fn service() -> String {
	let user = std::env::var("USERNAME")
		.or_else(|_| std::env::var("USER"))
		.unwrap_or_default();
	format!("BlindRadio-{user}")
}

/// The first copy's claim on the name: dropping it lets another copy start.
pub struct Guard {
	_checker: SingleInstanceChecker,
	_server: IPCServer,
	/// A later copy asked to be shown.
	pub raises: Receiver<()>,
}

/// Claims the name. A second copy pokes the first, then exits 0 with no window at all.
pub fn claim() -> Option<Guard> {
	let service = service();
	let checker = SingleInstanceChecker::new(&service, None)?;
	if checker.is_another_running() {
		if let Some(connection) = IPCClient::new().make_connection("localhost", &service, "raise") {
			connection.execute_string("raise");
		}
		std::process::exit(0);
	}
	let (tx, raises) = channel();
	let server = IPCServer::new(move |_topic| {
		// The IPC thread: the UI thread picks it up in `poll`.
		let _ = tx.send(());
		Some(IPCConnection::builder().build())
	});
	let _ = server.create(&service);
	Some(Guard {
		_checker: checker,
		_server: server,
		raises,
	})
}
