pub mod decode;
pub mod error;
pub mod hls;
pub mod net;
pub mod output;
pub mod playlist;
pub mod resample;
pub mod ring;
pub mod station;
pub mod stream;
pub mod tuner;

pub use error::{CoreError, Result};

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use stream::EngineCtx;

/// Owns the network runtime; `ctx` is what streams and the tuner need.
pub struct Engine {
	pub rt: tokio::runtime::Runtime,
	pub ctx: EngineCtx,
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
		Ok(Engine { rt, ctx })
	}
}
