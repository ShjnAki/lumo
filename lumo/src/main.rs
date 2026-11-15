#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
mod win;

#[cfg(not(windows))]
fn main() {
    eprintln!("Lumo est une application Windows. Sous Linux : `cargo run --release --bin preview` pour valider le rendu.");
}

#[cfg(windows)]
fn main() {
    win::run();
}
