// Embed an application manifest: common controls v6 (modern control styles) and system DPI awareness,
// which matches native-windows-gui's "high-dpi" scaling.
use embed_manifest::{embed_manifest, manifest::DpiAwareness, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("RotaryKnob").dpi_awareness(DpiAwareness::System)).expect("embed manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
