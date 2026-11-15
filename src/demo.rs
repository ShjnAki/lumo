//! Aperçu en direct de la fenêtre de réglages : le vrai moteur (`Fx`) piloté par une souris
//! simulée qui décrit une boucle, clique de temps en temps et, selon la page, dessine une
//! annotation ou secoue le curseur. Portable et testé ; la couche Windows se contente
//! d'appeler `step` puis `render` et d'afficher l'image.

use crate::fx::{Button, Fx};
use crate::settings::{CursorMode, Settings};
use crate::skin::Skin;
use std::f32::consts::TAU;
use tiny_skia::*;

/// Durée d'une boucle du scénario (s).
const CYCLE: f32 = 4.8;

pub struct Demo {
    fx: Fx,
    page: String,
    t: f32,
    next_click: f32,
    release_at: Option<(Button, f32)>,
    right: bool,
    cycle: u32,
}

impl Demo {
    pub fn new(s: Settings, skin: Skin) -> Demo {
        Demo { fx: Fx::new(s, skin), page: String::new(), t: 0.0, next_click: 0.8, release_at: None, right: false, cycle: 0 }
    }

    pub fn settings(&self) -> &Settings {
        &self.fx.s
    }

    pub fn set_settings(&mut self, s: Settings, skin: Option<Skin>) {
        self.fx.set_settings(s, skin);
    }

    /// Change de scénario (identifiant de page du schéma) et repart d'une scène vide.
    pub fn set_page(&mut self, id: &str) {
        if id != self.page {
            self.page = id.to_string();
            let (s, skin) = (self.fx.s.clone(), self.fx.skin.clone());
            *self = Demo { page: id.to_string(), ..Demo::new(s, skin) };
        }
    }

    fn click_every(&self) -> f32 {
        match self.page.as_str() {
            "click" => 0.9,
            "cursor" => 1.6,
            _ => 2.4,
        }
    }

    /// Position de la souris simulée à l'instant `t` dans une zone de `w` x `h` pixels.
    fn path(&self, t: f32, w: f32, h: f32) -> (f32, f32) {
        let a = t / CYCLE * TAU;
        let (cx, cy) = (w * 0.5, h * 0.5);
        let (ax, ay) = ((w * 0.5 - 70.0).max(w * 0.2), (h * 0.5 - 30.0).max(h * 0.15));
        let (mut x, y) = (cx + ax * a.sin(), cy + ay * (2.0 * a + 0.6).sin());
        // page Général : une secousse par boucle, pour montrer « secouer pour localiser »
        if self.page == "general" {
            let k = t % CYCLE;
            if (1.0..1.6).contains(&k) {
                x += 70.0 * (k * 50.0).sin().signum();
            }
        }
        (x, y)
    }

    /// Avance la simulation de `dt` secondes pour une zone de `w` x `h` pixels physiques.
    pub fn step(&mut self, dt: f32, w: f32, h: f32) {
        let dt = dt.clamp(0.0, 0.1);
        self.t += dt;
        let t = self.t;

        let cycle = (t / CYCLE) as u32;
        if cycle != self.cycle {
            self.cycle = cycle;
            if self.fx.s.annotate.fade_delay <= 0.0 {
                self.fx.clear_annotations(); // sinon les tracés s'accumulent dans l'aperçu
            }
        }
        let p = self.path(t, w, h);
        if self.fx.pos() == (0.0, 0.0) {
            self.fx.set_pos(p); // première image : un tracé ne peut démarrer qu'avec une position connue
        }
        // page Annotations : on « tient la touche » pendant la première moitié de la boucle
        let drawing = self.page == "annotate" && (t % CYCLE) < CYCLE * 0.5;
        self.fx.set_annotating(drawing);
        self.fx.set_pos(p);

        if let Some((b, at)) = self.release_at {
            if t >= at {
                self.fx.mouse_button(p, b, false);
                self.release_at = None;
            }
        }
        if t >= self.next_click && self.release_at.is_none() && !drawing {
            let b = if self.right && self.page == "click" { Button::Right } else { Button::Left };
            self.right = !self.right;
            self.fx.mouse_button(p, b, true);
            self.release_at = Some((b, t + 0.18));
            self.next_click = t + self.click_every();
        }
        self.fx.update(dt);
    }

    /// Dessine l'image courante (fond transparent) : effets puis pointeur.
    pub fn render(&self, pm: &mut Pixmap) {
        self.fx.render(pm, (0, 0));
        let s = &self.fx.s;
        let (x, y) = self.fx.pos();
        let mode = if s.enabled { s.cursor.mode } else { CursorMode::Off };
        match mode {
            CursorMode::Overlay => {} // déjà dessiné par le moteur
            CursorMode::System => {
                let img = self.fx.cursor_image();
                let pp = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
                pm.draw_pixmap(
                    (x - img.width() as f32 / 2.0) as i32,
                    (y - img.height() as f32 / 2.0) as i32,
                    img.as_ref(),
                    &pp,
                    Transform::identity(),
                    None,
                );
            }
            CursorMode::Off => draw_arrow(pm, x, y),
        }
    }
}

/// Flèche Windows simplifiée (pointe en `x`, `y`), pour représenter le curseur inchangé.
fn draw_arrow(pm: &mut Pixmap, x: f32, y: f32) {
    let mut pb = PathBuilder::new();
    let pts = [(0.0, 0.0), (0.0, 17.0), (4.2, 13.2), (7.0, 19.5), (9.6, 18.4), (6.9, 12.3), (12.2, 12.1)];
    pb.move_to(x + pts[0].0, y + pts[0].1);
    for (px, py) in &pts[1..] {
        pb.line_to(x + px, y + py);
    }
    pb.close();
    let Some(path) = pb.finish() else { return };
    let mut paint = Paint::default();
    paint.anti_alias = true;
    paint.set_color_rgba8(255, 255, 255, 255);
    pm.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    paint.set_color_rgba8(0, 0, 0, 255);
    let stroke = Stroke { width: 1.2, line_join: LineJoin::Round, ..Stroke::default() };
    pm.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

/// Encodage base64 (alphabet standard, avec remplissage), pour envoyer l'image à la page.
pub fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(A[(n >> 18) as usize & 63] as char);
        out.push(A[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { A[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { A[n as usize & 63] as char } else { '=' });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque_pixels(pm: &Pixmap) -> usize {
        pm.pixels().iter().filter(|p| p.alpha() > 0).count()
    }

    fn run(page: &str, s: Settings, secs: f32) -> Pixmap {
        let mut d = Demo::new(s, Skin::default());
        d.set_page(page);
        let (w, h) = (480u32, 160u32);
        let mut t = 0.0;
        while t < secs {
            d.step(1.0 / 30.0, w as f32, h as f32);
            t += 1.0 / 30.0;
        }
        let mut pm = Pixmap::new(w, h).unwrap();
        d.render(&mut pm);
        pm
    }

    #[test]
    fn renders_something_on_every_page() {
        for p in crate::schema::PAGES.iter().filter(|p| p.preview) {
            let pm = run(p.id, Settings::default(), 1.0);
            assert!(opaque_pixels(&pm) > 50, "aperçu vide pour {}", p.id);
        }
    }

    #[test]
    fn disabled_effects_leave_only_the_arrow() {
        let mut s = Settings::default();
        s.enabled = false;
        let n = opaque_pixels(&run("ring", s, 1.0));
        assert!(n > 20 && n < 400, "{n} pixels");
    }

    #[test]
    fn annotation_page_draws_a_stroke() {
        let mut s = Settings::default();
        s.ring.enabled = false;
        s.click.enabled = false;
        let with = opaque_pixels(&run("annotate", s.clone(), 2.0));
        s.annotate.enabled = false;
        let without = opaque_pixels(&run("annotate", s, 2.0));
        assert!(with > without + 500, "{with} vs {without}");
    }

    #[test]
    fn general_page_triggers_shake_to_locate() {
        let mut s = Settings::default();
        s.ring.enabled = false;
        s.click.enabled = false;
        let on = opaque_pixels(&run("general", s.clone(), 1.5));
        s.shake_to_locate = false;
        let off = opaque_pixels(&run("general", s, 1.5));
        assert!(on > off + 200, "{on} vs {off}");
    }

    #[test]
    fn path_stays_inside() {
        let mut d = Demo::new(Settings::default(), Skin::default());
        for page in ["ring", "general"] {
            d.set_page(page);
            for i in 0..500 {
                let (x, y) = d.path(i as f32 * 0.02, 480.0, 160.0);
                assert!((0.0..=480.0).contains(&x) && (0.0..=160.0).contains(&y), "{page}: {x},{y}");
            }
        }
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
    }
}
