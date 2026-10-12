//! Embeds the app icon and version in the executable, where Explorer, shortcuts and the window
//! find them.

use std::env;

fn main() {
    // Cargo.toml is the one source of the version: the release bump changes it, and with it what
    // Properties → Details shows.
    let version = |part| env::var(format!("CARGO_PKG_VERSION{part}")).expect("set by Cargo");
    let macros = [
        format!("VERSION_MAJOR={}", version("_MAJOR")),
        format!("VERSION_MINOR={}", version("_MINOR")),
        format!("VERSION_PATCH={}", version("_PATCH")),
        // Quoted, as rc.exe wants for a VALUE string.
        format!("VERSION_TEXT=\"{}\"", version("")),
    ];
    embed_resource::compile("assets/superslide.rc", macros)
        .manifest_required()
        .expect("the Windows SDK's resource compiler builds assets/superslide.rc");
    println!("cargo::rerun-if-changed=assets/superslide.rc");
    println!("cargo::rerun-if-changed=assets/icon.ico");
}
