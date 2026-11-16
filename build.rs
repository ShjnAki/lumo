//! Incorpore lumo.manifest (contrôles Windows modernes comctl32 v6, DPI par écran) et l'icône
//! de l'exécutable (lumo.rc). Uniquement avec la toolchain MSVC ; avec GNU, l'app fonctionne
//! mais les contrôles ont l'ancien style et l'exe n'a pas d'icône.
fn main() {
    println!("cargo:rerun-if-changed=lumo.manifest");
    println!("cargo:rerun-if-changed=lumo.rc");
    println!("cargo:rerun-if-changed=assets/lumo.ico");
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if os == "windows" && env == "msvc" {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{dir}\\lumo.manifest");
        // L'icône n'est liée qu'à lumo.exe (pas aux petits outils preview/defaults/icon).
        // (ignorée si assets/lumo.ico manque, ce qui permet de la générer avec `cargo run --bin icon`)
        if std::path::Path::new(&dir).join("assets").join("lumo.ico").exists() {
            embed_resource::compile_for("lumo.rc", ["lumo"], embed_resource::NONE)
                .manifest_optional()
                .unwrap();
        }
    }
}
