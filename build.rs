//! Embeds the app icon in the executable, where Explorer, shortcuts and the window find it.

fn main() {
    embed_resource::compile("assets/superslide.rc", embed_resource::NONE)
        .manifest_required()
        .expect("the Windows SDK's resource compiler builds assets/superslide.rc");
    println!("cargo::rerun-if-changed=assets/icon.ico");
}
