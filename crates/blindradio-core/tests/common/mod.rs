//! Local HTTP test server shared by integration tests.
//! `{base}` in a Static or Sequence body or Redirect `to` is replaced with the server base URL.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub enum Route {
	Static {
		path: String,
		content_type: String,
		body: Vec<u8>,
	},
	/// Like Static, but each request gets the next body; the last one repeats.
	Sequence {
		path: String,
		content_type: String,
		bodies: Vec<Vec<u8>>,
	},
	/// Endless loop of `data`. `burst` bytes go out at once, then `bytes_per_sec`
	/// (0 = unpaced). An ICY block follows every `metaint` audio bytes, cycling
	/// `titles` (raw bytes, so tests can send Latin-1). `close_after` counts audio bytes.
	Stream {
		path: String,
		content_type: String,
		data: Vec<u8>,
		metaint: Option<usize>,
		titles: Vec<Vec<u8>>,
		burst: usize,
		bytes_per_sec: usize,
		close_after: Option<usize>,
	},
	/// Accepts the connection and never sends a byte.
	Silent {
		path: String,
	},
	Redirect {
		path: String,
		to: String,
	},
}

impl Route {
	fn path(&self) -> &str {
		match self {
			Route::Static { path, .. }
			| Route::Sequence { path, .. }
			| Route::Stream { path, .. }
			| Route::Silent { path }
			| Route::Redirect { path, .. } => path,
		}
	}
}

pub struct TestServer {
	pub base: String,
	hits: Hits,
	queries: Queries,
}

/// Query string of the latest request per path.
type Queries = Arc<Mutex<HashMap<String, String>>>;

/// Requests seen per path, 404s included.
type Hits = Arc<Mutex<HashMap<String, usize>>>;

impl TestServer {
	pub fn url(&self, path: &str) -> String {
		format!("{}{}", self.base, path)
	}

	/// Query string of the latest request to `path` (empty when it had none).
	pub fn last_query(&self, path: &str) -> Option<String> {
		self.queries.lock().unwrap().get(path).cloned()
	}

	pub fn hits(&self, path: &str) -> usize {
		self.hits.lock().unwrap().get(path).copied().unwrap_or(0)
	}
}

pub fn serve(routes: Vec<Route>) -> TestServer {
	let (tx, rx) = std::sync::mpsc::channel();
	let hits = Hits::default();
	let server_hits = hits.clone();
	let queries = Queries::default();
	let server_queries = queries.clone();
	std::thread::spawn(move || {
		let rt = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(2)
			.enable_all()
			.build()
			.unwrap();
		rt.block_on(async move {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			let base = format!("http://{}", listener.local_addr().unwrap());
			tx.send(base.clone()).unwrap();
			let routes = Arc::new(routes);
			loop {
				let Ok((sock, _)) = listener.accept().await else {
					continue;
				};
				let routes = routes.clone();
				let base = base.clone();
				let hits = server_hits.clone();
				let queries = server_queries.clone();
				tokio::spawn(async move {
					let _ = handle(sock, &routes, &base, &hits, &queries).await;
				});
			}
		});
	});
	TestServer {
		base: rx.recv().unwrap(),
		hits,
		queries,
	}
}

/// Binary bodies (not UTF-8) go out untouched.
fn fill(bytes: &[u8], base: &str) -> Vec<u8> {
	match std::str::from_utf8(bytes) {
		Ok(text) => text.replace("{base}", base).into_bytes(),
		Err(_) => bytes.to_vec(),
	}
}

async fn handle(
	mut sock: TcpStream,
	routes: &[Route],
	base: &str,
	hits: &Hits,
	queries: &Queries,
) -> std::io::Result<()> {
	let mut req = Vec::new();
	let mut buf = [0u8; 1024];
	while !req.windows(4).any(|w| w == b"\r\n\r\n") {
		let n = sock.read(&mut buf).await?;
		if n == 0 {
			return Ok(());
		}
		req.extend_from_slice(&buf[..n]);
	}
	let line = String::from_utf8_lossy(&req);
	let target = line.split_whitespace().nth(1).unwrap_or("/");
	let path = target.split('?').next().unwrap_or("/");
	queries.lock().unwrap().insert(
		path.to_owned(),
		target.split_once('?').map_or("", |(_, q)| q).to_owned(),
	);
	let hit = {
		let mut hits = hits.lock().unwrap();
		let n = hits.entry(path.to_owned()).or_default();
		*n += 1;
		*n
	};
	let Some(route) = routes.iter().find(|r| r.path() == path) else {
		return sock
			.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
			.await;
	};
	match route {
		Route::Static {
			content_type, body, ..
		} => static_reply(&mut sock, content_type, &fill(body, base)).await,
		Route::Sequence {
			content_type,
			bodies,
			..
		} => {
			let body = &bodies[(hit - 1).min(bodies.len() - 1)];
			static_reply(&mut sock, content_type, &fill(body, base)).await
		}
		Route::Redirect { to, .. } => {
			let to = String::from_utf8(fill(to.as_bytes(), base)).unwrap();
			let head = format!(
				"HTTP/1.1 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
			);
			sock.write_all(head.as_bytes()).await
		}
		Route::Silent { .. } => {
			tokio::time::sleep(Duration::from_secs(3600)).await;
			Ok(())
		}
		Route::Stream {
			content_type,
			data,
			metaint,
			titles,
			burst,
			bytes_per_sec,
			close_after,
			..
		} => {
			let mut head =
				format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n");
			if let Some(m) = metaint {
				head.push_str(&format!("icy-metaint: {m}\r\n"));
			}
			head.push_str("\r\n");
			sock.write_all(head.as_bytes()).await?;
			let mut gen_ = StreamGen {
				data,
				metaint: *metaint,
				titles,
				pos: 0,
				since_meta: 0,
				title_i: 0,
			};
			let mut left = close_after.unwrap_or(usize::MAX);
			let first = (*burst).min(left);
			if first > 0 {
				sock.write_all(&gen_.next(first)).await?;
				left -= first;
			}
			// 20 ticks a second keeps pacing smooth without a timer per byte.
			let tick = (bytes_per_sec / 20).max(1);
			while left > 0 {
				let n = tick.min(left);
				sock.write_all(&gen_.next(n)).await?;
				left -= n;
				if *bytes_per_sec > 0 {
					tokio::time::sleep(Duration::from_millis(50)).await;
				}
			}
			sock.shutdown().await
		}
	}
}

async fn static_reply(
	sock: &mut TcpStream,
	content_type: &str,
	body: &[u8],
) -> std::io::Result<()> {
	let head = format!(
		"HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
		body.len()
	);
	sock.write_all(head.as_bytes()).await?;
	sock.write_all(body).await
}

struct StreamGen<'a> {
	data: &'a [u8],
	metaint: Option<usize>,
	titles: &'a [Vec<u8>],
	pos: usize,
	since_meta: usize,
	title_i: usize,
}

impl StreamGen<'_> {
	/// Next `n` audio bytes, with ICY blocks inserted at each metaint boundary.
	fn next(&mut self, mut n: usize) -> Vec<u8> {
		let mut out = Vec::new();
		while n > 0 {
			// Emitted lazily so a stream cut at a boundary ends on audio.
			if self.metaint == Some(self.since_meta) {
				self.since_meta = 0;
				out.extend(self.meta_block());
			}
			let room = self.metaint.map_or(n, |m| m - self.since_meta);
			let take = n.min(room);
			for _ in 0..take {
				out.push(self.data[self.pos % self.data.len()]);
				self.pos += 1;
			}
			self.since_meta += take;
			n -= take;
		}
		out
	}

	fn meta_block(&mut self) -> Vec<u8> {
		if self.titles.is_empty() {
			return vec![0];
		}
		let title = &self.titles[self.title_i % self.titles.len()];
		self.title_i += 1;
		let mut text = b"StreamTitle='".to_vec();
		text.extend_from_slice(title);
		text.extend_from_slice(b"';");
		let blocks = text.len().div_ceil(16);
		assert!(blocks <= 255, "ICY title too long");
		text.resize(blocks * 16, 0);
		let mut out = vec![blocks as u8];
		out.extend(text);
		out
	}
}
