//! Réglages persistés en JSON (%APPDATA%\Lumo\settings.json) + parsing des raccourcis.

use serde::{Deserialize, Serialize};

/// Couleur RGBA, sérialisée en "#RRGGBB" ou "#RRGGBBAA".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub fn parse(s: &str) -> Option<Rgba> {
        let h = s.trim().trim_start_matches('#');
        let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
        match h.len() {
            6 => Some(Rgba(p(0)?, p(2)?, p(4)?, 255)),
            8 => Some(Rgba(p(0)?, p(2)?, p(4)?, p(6)?)),
            _ => None,
        }
    }
    pub fn with_alpha(self, a: f32) -> Rgba {
        Rgba(self.0, self.1, self.2, (a.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

impl From<Rgba> for String {
    fn from(c: Rgba) -> String {
        if c.3 == 255 {
            format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", c.0, c.1, c.2, c.3)
        }
    }
}

impl TryFrom<String> for Rgba {
    type Error = String;
    fn try_from(s: String) -> Result<Rgba, String> {
        Rgba::parse(&s).ok_or_else(|| format!("couleur invalide: {s}"))
    }
}

impl Serialize for Rgba {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&String::from(*self))
    }
}
impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Rgba, D::Error> {
        let s = String::deserialize(d)?;
        Rgba::try_from(s).map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum SpotShape {
    Circle,
    HBar,
    VBar,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum TrailStyle {
    Line,
    Comet,
    Glow,
    Dots,
    Stars,
    Hearts,
    Rainbow,
    /// Utilise cursortrail.png du skin osu! (retombe sur Glow sans skin).
    Skin,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum ClickStyle {
    Ring,
    Ripple,
    Pulse,
    Burst,
    Confetti,
    Stars,
    Hearts,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum CursorMode {
    /// Curseur Windows inchangé.
    Off,
    /// Remplace les curseurs système par une image (latence nulle).
    System,
    /// Masque le curseur natif et le dessine dans l'overlay (échelle au clic, rotation…).
    Overlay,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Spotlight {
    pub enabled: bool,
    pub shape: SpotShape,
    /// Rayon du cercle, ou demi-épaisseur de la barre (px physiques).
    pub size: f32,
    /// Opacité de l'assombrissement (0..1).
    pub dim: f32,
    /// Douceur du bord (0 = net, 1 = très flou).
    pub softness: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Ring {
    pub enabled: bool,
    pub size: f32,
    pub thickness: f32,
    pub color: Rgba,
    pub pulse: bool,
    /// Halo lumineux (équivalent « Neon Glow »).
    pub glow: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Trail {
    pub enabled: bool,
    pub style: TrailStyle,
    pub color: Rgba,
    pub size: f32,
    /// Durée de vie de la traînée, en secondes.
    pub length: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct ClickFx {
    pub enabled: bool,
    pub style: ClickStyle,
    pub color: Rgba,
    pub right_color: Rgba,
    pub size: f32,
    pub duration: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Sound {
    pub enabled: bool,
    /// Chemin d'un .wav ; vide = son intégré.
    pub file: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct CursorCfg {
    pub mode: CursorMode,
    /// Taille de l'image du curseur (px).
    pub size: f32,
    /// Couleur du curseur généré (si pas de skin).
    pub color: Rgba,
    /// Dossier d'un skin osu! (cursor.png, cursormiddle.png, cursortrail.png). Vide = curseur généré.
    pub skin_dir: String,
    /// Échelle atteinte pendant un clic maintenu (mode overlay).
    pub press_scale: f32,
    /// Fait tourner cursor.png lentement, comme dans osu!.
    pub rotate: bool,
}

/// Format : "Ctrl+Alt+S", "Win+Shift+F5"… Vide = désactivé (aucun raccourci par défaut, comme Mouzz).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Hotkeys {
    pub toggle_all: String,
    pub toggle_spotlight: String,
    pub toggle_ring: String,
    pub toggle_trail: String,
    pub toggle_clicks: String,
    pub toggle_cursor: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Settings {
    pub enabled: bool,
    pub spotlight: Spotlight,
    pub ring: Ring,
    pub trail: Trail,
    pub click: ClickFx,
    pub sound: Sound,
    pub shake_to_locate: bool,
    pub cursor: CursorCfg,
    pub hotkeys: Hotkeys,
}

impl Default for Spotlight {
    fn default() -> Self {
        Self { enabled: false, shape: SpotShape::Circle, size: 180.0, dim: 0.65, softness: 0.25 }
    }
}
impl Default for Ring {
    fn default() -> Self {
        Self {
            enabled: true,
            size: 30.0,
            thickness: 4.0,
            color: Rgba(255, 196, 0, 220),
            pulse: false,
            glow: false,
        }
    }
}
impl Default for Trail {
    fn default() -> Self {
        Self { enabled: false, style: TrailStyle::Comet, color: Rgba(80, 200, 255, 255), size: 14.0, length: 0.45 }
    }
}
impl Default for ClickFx {
    fn default() -> Self {
        Self {
            enabled: true,
            style: ClickStyle::Ripple,
            color: Rgba(255, 90, 120, 255),
            right_color: Rgba(90, 160, 255, 255),
            size: 60.0,
            duration: 0.55,
        }
    }
}
impl Default for Sound {
    fn default() -> Self {
        Self { enabled: false, file: String::new() }
    }
}
impl Default for CursorCfg {
    fn default() -> Self {
        Self {
            mode: CursorMode::Off,
            size: 48.0,
            color: Rgba(255, 102, 170, 255),
            skin_dir: String::new(),
            press_scale: 1.25,
            rotate: false,
        }
    }
}
impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            toggle_all: String::new(),
            toggle_spotlight: String::new(),
            toggle_ring: String::new(),
            toggle_trail: String::new(),
            toggle_clicks: String::new(),
            toggle_cursor: String::new(),
        }
    }
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            spotlight: Spotlight::default(),
            ring: Ring::default(),
            trail: Trail::default(),
            click: ClickFx::default(),
            sound: Sound::default(),
            shake_to_locate: true,
            cursor: CursorCfg::default(),
            hotkeys: Hotkeys::default(),
        }
    }
}

impl Settings {
    pub fn from_json(s: &str) -> Result<Settings, serde_json::Error> {
        serde_json::from_str(s)
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
    /// Garde-fous : évite les valeurs qui feraient planter le rendu.
    pub fn sanitize(&mut self) {
        self.spotlight.size = self.spotlight.size.clamp(20.0, 2000.0);
        self.spotlight.dim = self.spotlight.dim.clamp(0.0, 1.0);
        self.spotlight.softness = self.spotlight.softness.clamp(0.0, 1.0);
        self.ring.size = self.ring.size.clamp(8.0, 400.0);
        self.ring.thickness = self.ring.thickness.clamp(1.0, 40.0);
        self.trail.size = self.trail.size.clamp(2.0, 80.0);
        self.trail.length = self.trail.length.clamp(0.1, 3.0);
        self.click.size = self.click.size.clamp(10.0, 400.0);
        self.click.duration = self.click.duration.clamp(0.15, 3.0);
        self.cursor.size = self.cursor.size.clamp(16.0, 256.0);
        self.cursor.press_scale = self.cursor.press_scale.clamp(0.5, 2.5);
    }
}

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

/// "Ctrl+Alt+S" -> (modificateurs, code VK Windows). None si vide ou invalide.
pub fn parse_hotkey(s: &str) -> Option<(u32, u32)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut mods = 0;
    let mut vk = None;
    for part in s.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "meta" | "super" => mods |= MOD_WIN,
            k if k.len() == 1 => {
                let c = k.as_bytes()[0];
                vk = match c {
                    b'a'..=b'z' => Some((c - b'a') as u32 + 0x41),
                    b'0'..=b'9' => Some((c - b'0') as u32 + 0x30),
                    _ => None,
                };
            }
            k if k.starts_with('f') => {
                let n: u32 = k[1..].parse().ok()?;
                if (1..=24).contains(&n) {
                    vk = Some(0x6F + n);
                }
            }
            _ => return None,
        }
    }
    if mods == 0 {
        return None; // un raccourci global sans modificateur capturerait la touche partout
    }
    vk.map(|v| (mods, v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys() {
        assert_eq!(parse_hotkey("Ctrl+Alt+S"), Some((MOD_CONTROL | MOD_ALT, 0x53)));
        assert_eq!(parse_hotkey("win+shift+F5"), Some((MOD_WIN | MOD_SHIFT, 0x74)));
        assert_eq!(parse_hotkey(""), None);
        assert_eq!(parse_hotkey("S"), None);
        assert_eq!(parse_hotkey("Ctrl+Nope"), None);
    }

    #[test]
    fn roundtrip() {
        let s = Settings::default();
        let j = s.to_json();
        let back = Settings::from_json(&j).unwrap();
        assert_eq!(back.ring.color, s.ring.color);
        // un JSON partiel complète avec les défauts
        let p = Settings::from_json(r##"{"ring":{"color":"#00ff00"}}"##).unwrap();
        assert_eq!(p.ring.color, Rgba(0, 255, 0, 255));
        assert!(p.click.enabled);
    }
}
