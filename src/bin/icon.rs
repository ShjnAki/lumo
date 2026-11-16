//! Génère `assets/lumo.ico` (icône de lumo.exe) avec le même dessin que l'icône de la zone de
//! notification. À relancer seulement si le dessin change : `cargo run --bin icon`.
use lumo::fx::generated_cursor;
use lumo::settings::Rgba;

fn main() {
    let sizes: [u32; 6] = [16, 32, 48, 64, 128, 256];
    let mut frames: Vec<Vec<u8>> = Vec::new();
    for &sz in &sizes {
        let pm = generated_cursor(sz, Rgba(255, 102, 170, 255), true);
        // tiny-skia est prémultiplié : on repasse en RGBA droit pour le PNG
        let mut rgba = Vec::with_capacity((sz * sz * 4) as usize);
        for p in pm.pixels() {
            let c = p.demultiply();
            rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
        }
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, sz, sz);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().expect("en-tête PNG");
            w.write_image_data(&rgba).expect("données PNG");
        }
        frames.push(out);
    }

    // ICONDIR + ICONDIRENTRY[] + images PNG
    let mut ico: Vec<u8> = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (i, &sz) in sizes.iter().enumerate() {
        let d = if sz >= 256 { 0 } else { sz as u8 };
        ico.extend_from_slice(&[d, d, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes()); // plans
        ico.extend_from_slice(&32u16.to_le_bytes()); // bits par pixel
        ico.extend_from_slice(&(frames[i].len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += frames[i].len() as u32;
    }
    for f in &frames {
        ico.extend_from_slice(f);
    }

    std::fs::create_dir_all("assets").expect("dossier assets");
    std::fs::write("assets/lumo.ico", &ico).expect("écriture de assets/lumo.ico");
    println!("assets/lumo.ico ({} octets, {} tailles)", ico.len(), sizes.len());
}
