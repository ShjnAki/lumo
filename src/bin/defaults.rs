//! Affiche les réglages par défaut (utile pour régénérer settings.example.json).
fn main() {
    println!("{}", lumo::settings::Settings::default().to_json());
}
