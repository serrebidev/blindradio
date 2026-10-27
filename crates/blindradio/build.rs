use embed_manifest::manifest::{ActiveCodePage, DpiAwareness, Setting, SupportedOS::*};
use embed_manifest::{embed_manifest, new_manifest};

// Shipfitter replaces this in plan 6.
fn main() {
	if std::env::var("TARGET").unwrap_or_default().contains("windows") {
		let manifest = new_manifest("BlindRadio")
			// Common Controls v6.
			.supported_os(Windows7..=Windows10)
			.active_code_page(ActiveCodePage::Utf8)
			.dpi_awareness(DpiAwareness::PerMonitorV2)
			.long_path_aware(Setting::Enabled);
		if let Err(e) = embed_manifest(manifest) {
			println!("cargo:warning=manifest not embedded: {e}");
		}
	}
	println!("cargo:rerun-if-changed=build.rs");
}
