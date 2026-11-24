// Embeds the Windows resources (icon) into obsidilight.exe; a no-op for every other target.
// Uses rc.exe (MSVC) or x86_64-w64-mingw32-windres (GNU / cross build from Linux).
fn main() {
    println!("cargo:rerun-if-changed=obsidilight.rc");
    println!("cargo:rerun-if-changed=assets/obsidilight.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("obsidilight.rc", embed_resource::NONE).manifest_required().unwrap();
    }
}
