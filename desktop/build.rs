fn main() {
    println!("cargo:rerun-if-changed=app.manifest");
    // MSVC embeds the manifest: native common-control styling and per-monitor DPI.
    // No build dependency, resource compiler, downloaded runtime, or external manifest.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("app.manifest");
        println!("cargo:rustc-link-arg-bin=datapack-desktop=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-bin=datapack-desktop=/MANIFESTINPUT:{}",
            manifest.display()
        );
    }
}
