//! Incorpore lumo.manifest : contrôles Windows modernes (comctl32 v6) et DPI par écran.
//! Uniquement avec la toolchain MSVC ; avec GNU, l'app fonctionne mais les contrôles ont l'ancien style.
fn main() {
    println!("cargo:rerun-if-changed=lumo.manifest");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{dir}\\lumo.manifest");
    }
}
