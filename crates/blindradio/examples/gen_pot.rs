//! Regenerates `po/blindradio.pot` from the marked strings.
//! Run from anywhere: `cargo run -p blindradio --example gen_pot`.
//! Needs the gettext tools (xgettext) on PATH.

use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"));
	patois_build::gen_pot(root, root.join("po"), env!("CARGO_PKG_NAME"))
}
