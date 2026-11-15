//! Chargement d'un skin de curseur osu! (cursor.png, cursormiddle.png, cursortrail.png).

use std::fs::File;
use std::path::Path;
use tiny_skia::{IntSize, Pixmap};

#[derive(Default, Clone)]
pub struct Skin {
    pub cursor: Option<Pixmap>,
    pub middle: Option<Pixmap>,
    pub trail: Option<Pixmap>,
}

impl Skin {
    pub fn load(dir: &Path) -> Skin {
        Skin {
            cursor: load_named(dir, "cursor"),
            middle: load_named(dir, "cursormiddle"),
            trail: load_named(dir, "cursortrail"),
        }
    }
    pub fn is_empty(&self) -> bool {
        self.cursor.is_none() && self.middle.is_none() && self.trail.is_none()
    }
}

/// Préfère la variante @2x si elle existe (comme osu!).
fn load_named(dir: &Path, name: &str) -> Option<Pixmap> {
    for f in [format!("{name}@2x.png"), format!("{name}.png")] {
        let p = dir.join(f);
        if p.is_file() {
            if let Some(px) = load_png(&p) {
                return Some(px);
            }
        }
    }
    None
}

pub fn load_png(path: &Path) -> Option<Pixmap> {
    let file = File::open(path).ok()?;
    let mut dec = png::Decoder::new(file);
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let n = (w * h) as usize;
    let mut rgba = Vec::with_capacity(n * 4);
    let data = &buf[..info.buffer_size()];
    match info.color_type {
        png::ColorType::Rgba => rgba.extend_from_slice(data),
        png::ColorType::Rgb => {
            for c in data.chunks_exact(3) {
                rgba.extend_from_slice(&[c[0], c[1], c[2], 255]);
            }
        }
        png::ColorType::Grayscale => {
            for &g in data {
                rgba.extend_from_slice(&[g, g, g, 255]);
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for c in data.chunks_exact(2) {
                rgba.extend_from_slice(&[c[0], c[0], c[0], c[1]]);
            }
        }
        png::ColorType::Indexed => return None, // EXPAND convertit déjà en RGB(A)
    }
    // tiny-skia attend du RGBA prémultiplié
    for px in rgba.chunks_exact_mut(4) {
        let a = px[3] as u32;
        px[0] = ((px[0] as u32 * a + 127) / 255) as u8;
        px[1] = ((px[1] as u32 * a + 127) / 255) as u8;
        px[2] = ((px[2] as u32 * a + 127) / 255) as u8;
    }
    Pixmap::from_vec(rgba, IntSize::from_wh(w, h)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_png_and_prefers_2x() {
        let dir = std::env::temp_dir().join("lumo_skin_test");
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = Pixmap::new(16, 16).unwrap();
        a.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 128));
        a.save_png(dir.join("cursor.png")).unwrap();
        let mut b = Pixmap::new(32, 32).unwrap();
        b.fill(tiny_skia::Color::from_rgba8(0, 255, 0, 255));
        b.save_png(dir.join("cursor@2x.png")).unwrap();
        let skin = Skin::load(&dir);
        assert_eq!(skin.cursor.as_ref().unwrap().width(), 32);
        assert!(skin.middle.is_none() && !skin.is_empty());
        let px = load_png(&dir.join("cursor.png")).unwrap();
        assert_eq!(px.width(), 16);
    }
}
