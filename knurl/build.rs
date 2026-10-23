// Embed an application manifest: common controls v6 (modern control styles) and system DPI awareness,
// which matches native-windows-gui's "high-dpi" scaling. Plus the icons (knurl.rc).
use embed_manifest::{embed_manifest, manifest::DpiAwareness, new_manifest};

fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_manifest(new_manifest("Knurl").dpi_awareness(DpiAwareness::System)).expect("embed manifest");
        embed_resource::compile("knurl.rc", embed_resource::NONE).manifest_optional().expect("embed icons");
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=knurl.rc");
    println!("cargo:rerun-if-changed=assets");
}
