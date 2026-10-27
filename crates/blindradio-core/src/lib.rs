pub mod decode;
pub mod eq;
pub mod error;
pub mod history;
pub mod hls;
pub mod import;
pub mod library;
pub mod net;
pub mod output;
pub mod playlist;
pub mod resample;
pub mod ring;
pub mod settings;
pub mod sleep;
pub mod sources;
pub mod station;
pub mod store;
pub mod stream;
pub mod tuner;

pub use error::{CoreError, Result};

use std::mem::ManuallyDrop;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use stream::EngineCtx;

/// Owns the network runtime; `ctx` is what streams and the tuner need.
pub struct Engine {
	/// Shut down in `Drop` without waiting, so a slow DNS lookup cannot hold up exit.
	pub rt: ManuallyDrop<tokio::runtime::Runtime>,
	pub ctx: EngineCtx,
}

impl Drop for Engine {
	fn drop(&mut self) {
		// Keeps the resolved final URLs for faster cold starts next run.
		let _ = self.ctx.cache.lock().save();
		// SAFETY: `rt` is never used again; this is the only place it is taken.
		let rt = unsafe { ManuallyDrop::take(&mut self.rt) };
		rt.shutdown_background();
	}
}

impl Engine {
	pub fn new(cache_path: &Path, out_rate: u32) -> Result<Engine> {
		let rt = tokio::runtime::Builder::new_multi_thread()
			.worker_threads(2)
			.enable_all()
			.build()?;
		let ctx = EngineCtx {
			rt: rt.handle().clone(),
			client: net::client(),
			cache: Arc::new(parking_lot::Mutex::new(net::ResolveCache::load(cache_path))),
			out_rate,
			retry_window: Duration::from_secs(60),
		};
		Ok(Engine {
			rt: ManuallyDrop::new(rt),
			ctx,
		})
	}
}
