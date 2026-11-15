//! Moteur d'effets : état (curseur, traînée, particules, clics) + rendu tiny-skia.
//! Totalement indépendant de la plateforme : testable sous Linux via `cargo run --bin preview`.

use crate::settings::*;
use crate::skin::Skin;
use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};
use tiny_skia::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, PartialEq)]
enum PKind {
    Star,
    Heart,
    Dot,
    Square,
}

struct Particle {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    age: f32,
    life: f32,
    size: f32,
    rot: f32,
    vrot: f32,
    kind: PKind,
    color: Rgba,
    gravity: f32,
}

struct TrailPt {
    x: f32,
    y: f32,
    age: f32,
}

/// Un tracé d'annotation : actif tant que la touche est tenue, puis s'efface après `fade_delay`.
struct Scribble {
    pts: Vec<(f32, f32)>,
    released: Option<f32>,
}

struct ClickEv {
    x: f32,
    y: f32,
    age: f32,
    dur: f32,
    size: f32,
    color: Rgba,
    style: ClickStyle,
}

/// Détecte le « secouage » de la souris sur un axe : N demi-tours rapides et amples.
#[derive(Default)]
struct Shake {
    ext: f32,
    dir: i8,
    revs: VecDeque<f32>,
    init: bool,
}

impl Shake {
    const AMPLITUDE: f32 = 40.0;
    const WINDOW: f32 = 0.7;
    const REVERSALS: usize = 4;

    fn feed(&mut self, v: f32, t: f32) -> bool {
        if !self.init {
            self.ext = v;
            self.init = true;
        }
        if self.dir == 0 {
            if (v - self.ext).abs() >= Self::AMPLITUDE {
                self.dir = if v > self.ext { 1 } else { -1 };
                self.ext = v;
            }
        } else if self.dir > 0 {
            if v > self.ext {
                self.ext = v;
            } else if self.ext - v >= Self::AMPLITUDE {
                self.dir = -1;
                self.ext = v;
                self.revs.push_back(t);
            }
        } else if v < self.ext {
            self.ext = v;
        } else if v - self.ext >= Self::AMPLITUDE {
            self.dir = 1;
            self.ext = v;
            self.revs.push_back(t);
        }
        while self.revs.front().map_or(false, |&r| t - r > Self::WINDOW) {
            self.revs.pop_front();
        }
        if self.revs.len() >= Self::REVERSALS {
            self.revs.clear();
            return true;
        }
        false
    }
}

pub struct Fx {
    pub s: Settings,
    pub skin: Skin,
    cur_main: Pixmap,
    cur_mid: Option<Pixmap>,
    pos: (f32, f32),
    has_pos: bool,
    trail: VecDeque<TrailPt>,
    particles: Vec<Particle>,
    clicks: Vec<ClickEv>,
    strokes: Vec<Scribble>,
    annotating: bool,
    pressed: u8,
    press_anim: f32,
    t: f32,
    rot: f32,
    shake_x: Shake,
    shake_y: Shake,
    /// >= 0 pendant l'animation « localiser le curseur », -1 sinon.
    shake_age: f32,
    spawn_acc: f32,
    rng: u64,
}

const SHAKE_DURATION: f32 = 1.2;
const MAX_TRAIL: usize = 900;
const MAX_PARTICLES: usize = 400;
const MAX_STROKES: usize = 60;
const MAX_STROKE_PTS: usize = 6000;
const STROKE_FADE: f32 = 0.6;

impl Fx {
    pub fn new(mut s: Settings, skin: Skin) -> Fx {
        s.sanitize();
        let (cur_main, cur_mid) = build_cursor_cache(&s.cursor, &skin);
        Fx {
            s,
            skin,
            cur_main,
            cur_mid,
            pos: (0.0, 0.0),
            has_pos: false,
            trail: VecDeque::new(),
            particles: Vec::new(),
            clicks: Vec::new(),
            strokes: Vec::new(),
            annotating: false,
            pressed: 0,
            press_anim: 0.0,
            t: 0.0,
            rot: 0.0,
            shake_x: Shake::default(),
            shake_y: Shake::default(),
            shake_age: -1.0,
            spawn_acc: 0.0,
            rng: 0x9E37_79B9_7F4A_7C15,
        }
    }

    pub fn set_settings(&mut self, mut s: Settings, skin: Option<Skin>) {
        s.sanitize();
        if let Some(k) = skin {
            self.skin = k;
        }
        self.s = s;
        let (m, mid) = build_cursor_cache(&self.s.cursor, &self.skin);
        self.cur_main = m;
        self.cur_mid = mid;
    }

    /// Curseur composé (sans rotation ni échelle), pour fabriquer un curseur système.
    pub fn cursor_image(&self) -> Pixmap {
        let n = self.cur_main.width().max(self.cur_mid.as_ref().map_or(0, |p| p.width()));
        let mut out = Pixmap::new(n, n).unwrap();
        let pp = PixmapPaint::default();
        let off = |p: &Pixmap| (((n - p.width()) / 2) as i32, ((n - p.height()) / 2) as i32);
        let (x, y) = off(&self.cur_main);
        out.draw_pixmap(x, y, self.cur_main.as_ref(), &pp, Transform::identity(), None);
        if let Some(m) = &self.cur_mid {
            let (x, y) = off(m);
            out.draw_pixmap(x, y, m.as_ref(), &pp, Transform::identity(), None);
        }
        out
    }

    fn rand(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let v = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((v >> 40) as f32) / ((1u64 << 24) as f32)
    }

    pub fn pos(&self) -> (f32, f32) {
        self.pos
    }

    // ---------------------------------------------------------------- entrées

    pub fn set_pos(&mut self, p: (f32, f32)) {
        if !self.has_pos {
            self.pos = p;
            self.has_pos = true;
            return;
        }
        let (dx, dy) = (p.0 - self.pos.0, p.1 - self.pos.1);
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 0.5 {
            return;
        }
        if self.s.enabled {
            if self.annotating {
                if let Some(st) = self.strokes.last_mut().filter(|st| st.released.is_none()) {
                    let last = st.pts.last().copied().unwrap_or(p);
                    let d2 = (p.0 - last.0).powi(2) + (p.1 - last.1).powi(2);
                    if d2 >= 2.25 && st.pts.len() < MAX_STROKE_PTS {
                        st.pts.push(p);
                    }
                }
            }
            if self.s.trail.enabled {
                // points interpolés tous les ~4 px pour qu'une souris rapide ne laisse pas de trous
                let steps = (dist / 4.0).ceil().max(1.0) as usize;
                for i in 1..=steps {
                    let k = i as f32 / steps as f32;
                    self.trail.push_back(TrailPt { x: self.pos.0 + dx * k, y: self.pos.1 + dy * k, age: 0.0 });
                }
                while self.trail.len() > MAX_TRAIL {
                    self.trail.pop_front();
                }
                self.spawn_trail_particles(dist, (self.pos, p));
            }
            if self.s.shake_to_locate && self.shake_age < 0.0 {
                let sx = self.shake_x.feed(p.0, self.t);
                let sy = self.shake_y.feed(p.1, self.t);
                if sx || sy {
                    self.shake_age = 0.0;
                }
            }
        }
        self.pos = p;
    }

    fn spawn_trail_particles(&mut self, dist: f32, seg: ((f32, f32), (f32, f32))) {
        let kind = match self.s.trail.style {
            TrailStyle::Stars => PKind::Star,
            TrailStyle::Hearts => PKind::Heart,
            _ => return,
        };
        self.spawn_acc += dist;
        let spacing = (self.s.trail.size * 1.6).max(14.0);
        while self.spawn_acc >= spacing && self.particles.len() < MAX_PARTICLES {
            self.spawn_acc -= spacing;
            let k = self.rand();
            let x = seg.0 .0 + (seg.1 .0 - seg.0 .0) * k;
            let y = seg.0 .1 + (seg.1 .1 - seg.0 .1) * k;
            let (a, b, c) = (self.rand(), self.rand(), self.rand());
            let hue_shift = if kind == PKind::Heart { 0.0 } else { (a - 0.5) * 30.0 };
            let color = shift_hue(self.s.trail.color, hue_shift);
            self.particles.push(Particle {
                x,
                y,
                vx: (b - 0.5) * 50.0,
                vy: (c - 0.5) * 50.0 + if kind == PKind::Heart { -30.0 } else { 0.0 },
                age: 0.0,
                life: self.s.trail.length * 1.6,
                size: self.s.trail.size * (0.7 + a * 0.6),
                rot: a * 360.0,
                vrot: (b - 0.5) * 240.0,
                kind,
                color,
                gravity: 0.0,
            });
        }
    }

    /// À appeler AVANT `set_pos` à chaque image : `on` = la touche d'annotation est tenue.
    pub fn set_annotating(&mut self, on: bool) {
        if on == self.annotating {
            return;
        }
        self.annotating = on;
        if on {
            if self.s.enabled && self.s.annotate.enabled && self.has_pos {
                self.strokes.push(Scribble { pts: vec![self.pos], released: None });
                if self.strokes.len() > MAX_STROKES {
                    self.strokes.remove(0);
                }
            }
        } else if let Some(st) = self.strokes.last_mut() {
            if st.released.is_none() {
                st.released = Some(self.t);
            }
        }
    }

    pub fn clear_annotations(&mut self) {
        self.strokes.clear();
    }

    pub fn mouse_button(&mut self, p: (f32, f32), button: Button, down: bool) {
        let bit = match button {
            Button::Left => 1,
            Button::Right => 2,
            Button::Middle => 4,
        };
        if down {
            self.pressed |= bit;
        } else {
            self.pressed &= !bit;
        }
        if !down || !self.s.enabled || !self.s.click.enabled {
            return;
        }
        let c = self.s.click.clone();
        let color = if button == Button::Right { c.right_color } else { c.color };
        self.clicks.push(ClickEv { x: p.0, y: p.1, age: 0.0, dur: c.duration, size: c.size, color, style: c.style });
        if self.clicks.len() > 32 {
            self.clicks.remove(0);
        }
        match c.style {
            ClickStyle::Confetti => {
                for i in 0..26 {
                    let (a, b, d) = (self.rand(), self.rand(), self.rand());
                    let ang = a * TAU;
                    let sp = 140.0 + b * 260.0;
                    let hue = (i as f32 / 26.0) * 360.0;
                    self.particles.push(Particle {
                        x: p.0,
                        y: p.1,
                        vx: ang.cos() * sp,
                        vy: ang.sin() * sp - 120.0,
                        age: 0.0,
                        life: c.duration * (1.0 + d * 0.8),
                        size: 5.0 + d * 6.0,
                        rot: a * 360.0,
                        vrot: (b - 0.5) * 900.0,
                        kind: if i % 3 == 0 { PKind::Dot } else { PKind::Square },
                        color: hsv(hue, 0.75, 1.0, 255),
                        gravity: 700.0,
                    });
                }
            }
            ClickStyle::Stars | ClickStyle::Hearts => {
                let (kind, n) = if c.style == ClickStyle::Stars { (PKind::Star, 8) } else { (PKind::Heart, 7) };
                for i in 0..n {
                    let (a, b) = (self.rand(), self.rand());
                    let ang = (i as f32 / n as f32) * TAU + a * 0.5;
                    let sp = 90.0 + b * 120.0;
                    self.particles.push(Particle {
                        x: p.0,
                        y: p.1,
                        vx: ang.cos() * sp,
                        vy: ang.sin() * sp - if kind == PKind::Heart { 80.0 } else { 0.0 },
                        age: 0.0,
                        life: c.duration * 1.3,
                        size: c.size * (0.22 + b * 0.12),
                        rot: a * 360.0,
                        vrot: (b - 0.5) * 360.0,
                        kind,
                        color,
                        gravity: if kind == PKind::Heart { -60.0 } else { 0.0 },
                    });
                }
            }
            _ => {}
        }
    }

    // ---------------------------------------------------------------- simulation

    pub fn update(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        self.t += dt;
        self.rot = (self.rot + dt * 60.0) % 360.0;

        let target = if self.pressed != 0 { 1.0 } else { 0.0 };
        let k = 1.0 - (-dt * 22.0).exp();
        self.press_anim += (target - self.press_anim) * k;
        if (self.press_anim - target).abs() < 0.002 {
            self.press_anim = target;
        }

        let life = self.s.trail.length;
        for p in self.trail.iter_mut() {
            p.age += dt;
        }
        while self.trail.front().map_or(false, |p| p.age > life) {
            self.trail.pop_front();
        }

        for p in self.particles.iter_mut() {
            p.age += dt;
            p.x += p.vx * dt;
            p.y += p.vy * dt;
            p.vy += p.gravity * dt;
            let drag = (0.5f32).powf(dt * 3.0);
            p.vx *= drag;
            if p.gravity == 0.0 {
                p.vy *= drag;
            }
            p.rot += p.vrot * dt;
        }
        self.particles.retain(|p| p.age < p.life);

        for c in self.clicks.iter_mut() {
            c.age += dt;
        }
        self.clicks.retain(|c| c.age < c.dur);

        let delay = self.s.annotate.fade_delay;
        if delay > 0.0 {
            let t = self.t;
            self.strokes.retain(|st| st.released.map_or(true, |r| t - r < delay + STROKE_FADE));
        }

        if self.shake_age >= 0.0 {
            self.shake_age += dt;
            if self.shake_age > SHAKE_DURATION {
                self.shake_age = -1.0;
            }
        }
    }

    /// Faut-il continuer à rendre des images même si la souris est immobile ?
    pub fn animating(&self) -> bool {
        if !self.s.enabled {
            return false;
        }
        (self.s.ring.enabled && self.s.ring.pulse)
            || !self.trail.is_empty()
            || !self.particles.is_empty()
            || !self.clicks.is_empty()
            || (self.s.annotate.fade_delay > 0.0 && self.strokes.iter().any(|st| st.released.is_some()))
            || self.shake_age >= 0.0
            || (self.press_anim > 0.0 && self.press_anim < 1.0)
            || (self.s.cursor.mode == CursorMode::Overlay && self.s.cursor.rotate)
    }

    /// Y a-t-il quelque chose à dessiner dans ce rectangle d'écran (x, y, w, h) ?
    pub fn visible_in(&self, r: (i32, i32, i32, i32)) -> bool {
        if !self.s.enabled {
            return false;
        }
        if self.s.spotlight.enabled || !self.strokes.is_empty() {
            return true;
        }
        let m = 400.0;
        let (rx, ry, rw, rh) = (r.0 as f32 - m, r.1 as f32 - m, r.2 as f32 + 2.0 * m, r.3 as f32 + 2.0 * m);
        let inside = |x: f32, y: f32| x >= rx && y >= ry && x <= rx + rw && y <= ry + rh;
        inside(self.pos.0, self.pos.1)
            || self.trail.iter().any(|p| inside(p.x, p.y))
            || self.particles.iter().any(|p| inside(p.x, p.y))
            || self.clicks.iter().any(|c| inside(c.x, c.y))
    }

    // ---------------------------------------------------------------- rendu

    /// Dessine l'overlay pour un écran dont le coin haut-gauche est `origin` (coordonnées bureau virtuel).
    pub fn render(&self, pm: &mut Pixmap, origin: (i32, i32)) {
        pm.fill(Color::TRANSPARENT);
        if !self.s.enabled {
            return;
        }
        let tf = Transform::from_translate(-(origin.0 as f32), -(origin.1 as f32));
        let (w, h) = (pm.width() as f32, pm.height() as f32);
        let (px, py) = self.pos;

        if self.s.spotlight.enabled && self.has_pos {
            self.draw_spotlight(pm, tf, origin, (w, h));
        }
        if self.s.trail.enabled {
            self.draw_trail(pm, tf);
        }
        self.draw_particles(pm, tf);
        self.draw_strokes(pm, tf);
        if self.s.ring.enabled && self.has_pos {
            self.draw_ring(pm, tf, px, py);
        }
        for c in &self.clicks {
            draw_click(pm, tf, c);
        }
        if self.shake_age >= 0.0 && self.has_pos {
            self.draw_locator(pm, tf, px, py);
        }
        if self.s.cursor.mode == CursorMode::Overlay && self.has_pos {
            self.draw_cursor(pm, tf, px, py);
        }
    }

    fn draw_spotlight(&self, pm: &mut Pixmap, tf: Transform, origin: (i32, i32), size: (f32, f32)) {
        let sp = &self.s.spotlight;
        let (ox, oy) = (origin.0 as f32, origin.1 as f32);
        let Some(full) = Rect::from_xywh(ox, oy, size.0, size.1) else { return };
        let dim = solid(Rgba(0, 0, 0, 255), sp.dim);
        pm.fill_rect(full, &dim, tf, None);

        let soft = sp.softness.clamp(0.0, 0.95);
        let solid_to = (1.0 - soft).max(0.0);
        let a1 = Color::from_rgba8(0, 0, 0, 255);
        let a0 = Color::from_rgba8(0, 0, 0, 0);
        let mut hole = Paint::default();
        hole.blend_mode = BlendMode::DestinationOut;
        hole.anti_alias = true;
        let (cx, cy) = self.pos;
        match sp.shape {
            SpotShape::Circle => {
                let sh = RadialGradient::new(
                    Point::from_xy(cx, cy),
                    Point::from_xy(cx, cy),
                    sp.size,
                    vec![GradientStop::new(0.0, a1), GradientStop::new(solid_to, a1), GradientStop::new(1.0, a0)],
                    SpreadMode::Pad,
                    Transform::identity(),
                );
                if let (Some(sh), Some(path)) = (sh, PathBuilder::from_circle(cx, cy, sp.size)) {
                    hole.shader = sh;
                    pm.fill_path(&path, &hole, FillRule::Winding, tf, None);
                }
            }
            SpotShape::HBar | SpotShape::VBar => {
                let horiz = sp.shape == SpotShape::HBar;
                let (a, b) = if horiz {
                    (Point::from_xy(0.0, cy - sp.size), Point::from_xy(0.0, cy + sp.size))
                } else {
                    (Point::from_xy(cx - sp.size, 0.0), Point::from_xy(cx + sp.size, 0.0))
                };
                let edge = soft / 2.0;
                let sh = LinearGradient::new(
                    a,
                    b,
                    vec![
                        GradientStop::new(0.0, a0),
                        GradientStop::new(edge.max(0.001), a1),
                        GradientStop::new((1.0 - edge).min(0.999), a1),
                        GradientStop::new(1.0, a0),
                    ],
                    SpreadMode::Pad,
                    Transform::identity(),
                );
                let rect = if horiz {
                    Rect::from_xywh(ox, cy - sp.size, size.0, sp.size * 2.0)
                } else {
                    Rect::from_xywh(cx - sp.size, oy, sp.size * 2.0, size.1)
                };
                if let (Some(sh), Some(rect)) = (sh, rect) {
                    hole.shader = sh;
                    pm.fill_rect(rect, &hole, tf, None);
                }
            }
        }
    }

    /// Teinte de départ de l'anneau RGB à l'instant courant.
    fn rgb_hue(&self) -> f32 {
        (self.t * self.s.ring.rgb_speed).rem_euclid(360.0)
    }

    fn draw_ring(&self, pm: &mut Pixmap, tf: Transform, x: f32, y: f32) {
        let r = &self.s.ring;
        let pulse = if r.pulse { 1.0 + 0.12 * (self.t * TAU * 0.9).sin() } else { 1.0 };
        let rad = r.size * pulse;
        let hue0 = self.rgb_hue();
        // en mode RGB, le halo prend la teinte courante, avec l'opacité de `color`
        let base = if r.rgb { hsv(hue0, 0.9, 1.0, r.color.3) } else { r.color };
        if r.glow {
            if let Some(p) = glow_paint(x, y, rad * 1.9, base, 0.55) {
                if let Some(path) = PathBuilder::from_circle(x, y, rad * 1.9) {
                    pm.fill_path(&path, &p, FillRule::Winding, tf, None);
                }
            }
            if r.rgb {
                rgb_ring(pm, tf, x, y, rad, r.thickness * 2.6, 0.25 * r.color.3 as f32 / 255.0, hue0);
            } else {
                stroke_circle(pm, tf, x, y, rad, r.thickness * 2.6, r.color, 0.25);
            }
        }
        if r.rgb {
            rgb_ring(pm, tf, x, y, rad, r.thickness, r.color.3 as f32 / 255.0, hue0);
        } else {
            stroke_circle(pm, tf, x, y, rad, r.thickness, r.color, 1.0);
        }
    }

    fn draw_locator(&self, pm: &mut Pixmap, tf: Transform, x: f32, y: f32) {
        let k = (self.shake_age / SHAKE_DURATION).clamp(0.0, 1.0);
        let swell = (k * PI).sin().max(0.0);
        let rad = self.s.ring.size * (1.0 + 2.6 * swell);
        let col = self.s.ring.color;
        if let (Some(p), Some(path)) = (glow_paint(x, y, rad * 1.5, col, 0.35 * swell), PathBuilder::from_circle(x, y, rad * 1.5)) {
            pm.fill_path(&path, &p, FillRule::Winding, tf, None);
        }
        if self.s.ring.rgb {
            rgb_ring(pm, tf, x, y, rad, self.s.ring.thickness * 1.6, swell.min(1.0), self.rgb_hue());
        } else {
            stroke_circle(pm, tf, x, y, rad, self.s.ring.thickness * 1.6, col, swell.min(1.0));
        }
    }

    fn draw_trail(&self, pm: &mut Pixmap, tf: Transform) {
        let t = &self.s.trail;
        let n = self.trail.len();
        if n == 0 {
            return;
        }
        let life = t.length.max(0.01);
        let f_of = |p: &TrailPt| (1.0 - p.age / life).clamp(0.0, 1.0);
        let pts: Vec<&TrailPt> = self.trail.iter().collect();

        match t.style {
            TrailStyle::Line | TrailStyle::Comet | TrailStyle::Rainbow => {
                for i in 1..n {
                    let (a, b) = (pts[i - 1], pts[i]);
                    let f = f_of(b);
                    let (width, alpha, color) = match t.style {
                        TrailStyle::Line => ((t.size * 0.28).max(2.0), f, t.color),
                        TrailStyle::Comet => (t.size * f.powf(0.9), f.powf(0.8), t.color),
                        _ => {
                            let hue = (self.t * 120.0 + i as f32 * 2.2) % 360.0;
                            (t.size * 0.7 * f.max(0.15), f.powf(0.7), hsv(hue, 0.85, 1.0, 255))
                        }
                    };
                    if width < 0.4 || alpha <= 0.01 {
                        continue;
                    }
                    let mut pb = PathBuilder::new();
                    pb.move_to(a.x, a.y);
                    pb.line_to(b.x, b.y);
                    if let Some(path) = pb.finish() {
                        let st = Stroke { width, line_cap: LineCap::Round, ..Stroke::default() };
                        pm.stroke_path(&path, &solid(color, alpha), &st, tf, None);
                    }
                }
                if t.style == TrailStyle::Comet {
                    if let Some(h) = pts.last() {
                        fill_glow(pm, tf, h.x, h.y, t.size * 1.4, t.color, 0.5);
                    }
                }
            }
            TrailStyle::Glow => {
                for (i, p) in pts.iter().enumerate() {
                    if i % 2 != 0 {
                        continue;
                    }
                    let f = f_of(p);
                    fill_glow(pm, tf, p.x, p.y, t.size * (0.7 + f), t.color, 0.30 * f);
                }
            }
            TrailStyle::Dots => {
                let mut last: Option<(f32, f32)> = None;
                for p in pts.iter().rev() {
                    if let Some((lx, ly)) = last {
                        if ((p.x - lx).powi(2) + (p.y - ly).powi(2)).sqrt() < t.size * 0.95 {
                            continue;
                        }
                    }
                    last = Some((p.x, p.y));
                    let f = f_of(p);
                    if let Some(path) = PathBuilder::from_circle(p.x, p.y, (t.size * 0.38 * f).max(0.5)) {
                        pm.fill_path(&path, &solid(t.color, f), FillRule::Winding, tf, None);
                    }
                }
            }
            TrailStyle::Skin => {
                if let Some(img) = &self.skin.trail {
                    let sc = t.size * 2.0 / img.width().max(img.height()) as f32;
                    let mut last: Option<(f32, f32)> = None;
                    for p in pts.iter() {
                        if let Some((lx, ly)) = last {
                            if ((p.x - lx).powi(2) + (p.y - ly).powi(2)).sqrt() < 6.0 {
                                continue;
                            }
                        }
                        last = Some((p.x, p.y));
                        let pp = PixmapPaint { opacity: f_of(p), quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
                        let xf = tf.pre_translate(p.x, p.y).pre_scale(sc, sc);
                        pm.draw_pixmap(-(img.width() as i32) / 2, -(img.height() as i32) / 2, img.as_ref(), &pp, xf, None);
                    }
                } else {
                    for (i, p) in pts.iter().enumerate() {
                        if i % 2 == 0 {
                            let f = f_of(p);
                            fill_glow(pm, tf, p.x, p.y, t.size * (0.7 + f), t.color, 0.30 * f);
                        }
                    }
                }
            }
            TrailStyle::Stars | TrailStyle::Hearts => {} // particules uniquement
        }
    }

    fn draw_particles(&self, pm: &mut Pixmap, tf: Transform) {
        for p in &self.particles {
            let k = (p.age / p.life).clamp(0.0, 1.0);
            let alpha = (1.0 - k).powf(0.6);
            let size = p.size * (1.0 - 0.45 * k);
            let xf = tf.pre_translate(p.x, p.y).pre_rotate(p.rot);
            let path = match p.kind {
                PKind::Star => star_path(size),
                PKind::Heart => heart_path(size),
                PKind::Dot => PathBuilder::from_circle(0.0, 0.0, size * 0.5),
                PKind::Square => Rect::from_xywh(-size * 0.5, -size * 0.3, size, size * 0.6).map(PathBuilder::from_rect),
            };
            if let Some(path) = path {
                pm.fill_path(&path, &solid(p.color, alpha), FillRule::Winding, xf, None);
            }
        }
    }

    fn draw_strokes(&self, pm: &mut Pixmap, tf: Transform) {
        let a = &self.s.annotate;
        for st in &self.strokes {
            let alpha = match st.released {
                Some(r) if a.fade_delay > 0.0 => (1.0 - (self.t - r - a.fade_delay) / STROKE_FADE).clamp(0.0, 1.0),
                _ => 1.0,
            };
            if alpha <= 0.0 || st.pts.is_empty() {
                continue;
            }
            if st.pts.len() == 1 {
                let (x, y) = st.pts[0];
                if let Some(path) = PathBuilder::from_circle(x, y, a.width * 0.5) {
                    pm.fill_path(&path, &solid(a.color, alpha), FillRule::Winding, tf, None);
                }
                continue;
            }
            // courbe lissée : quadratiques passant par les milieux des segments
            let pts = &st.pts;
            let mut pb = PathBuilder::new();
            pb.move_to(pts[0].0, pts[0].1);
            for i in 1..pts.len() - 1 {
                let mid = ((pts[i].0 + pts[i + 1].0) * 0.5, (pts[i].1 + pts[i + 1].1) * 0.5);
                pb.quad_to(pts[i].0, pts[i].1, mid.0, mid.1);
            }
            let last = pts[pts.len() - 1];
            pb.line_to(last.0, last.1);
            let Some(path) = pb.finish() else { continue };
            let shadow = Stroke { width: a.width + 4.0, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
            pm.stroke_path(&path, &solid(Rgba(0, 0, 0, 255), alpha * 0.22), &shadow, tf, None);
            let line = Stroke { width: a.width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
            pm.stroke_path(&path, &solid(a.color, alpha), &line, tf, None);
        }
    }

    fn draw_cursor(&self, pm: &mut Pixmap, tf: Transform, x: f32, y: f32) {
        let c = &self.s.cursor;
        let scale = 1.0 + (c.press_scale - 1.0) * self.press_anim;
        let pp = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
        let rot = if c.rotate { self.rot } else { 0.0 };
        let m = &self.cur_main;
        let xf = tf.pre_translate(x, y).pre_scale(scale, scale).pre_rotate(rot);
        pm.draw_pixmap(-(m.width() as i32) / 2, -(m.height() as i32) / 2, m.as_ref(), &pp, xf, None);
        if let Some(mid) = &self.cur_mid {
            let xf = tf.pre_translate(x, y).pre_scale(scale, scale);
            pm.draw_pixmap(-(mid.width() as i32) / 2, -(mid.height() as i32) / 2, mid.as_ref(), &pp, xf, None);
        }
    }
}

// ------------------------------------------------------------------ clics

fn draw_click(pm: &mut Pixmap, tf: Transform, c: &ClickEv) {
    let k = (c.age / c.dur).clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - k).powi(3); // ease-out cubic
    let fade = 1.0 - k;
    match c.style {
        ClickStyle::Ring => {
            stroke_circle(pm, tf, c.x, c.y, c.size * (0.25 + 0.75 * e), 2.0 + 5.0 * fade, c.color, fade);
        }
        ClickStyle::Ripple => {
            for i in 0..3 {
                let delay = i as f32 * 0.16;
                let kk = ((k - delay) / (1.0 - delay)).clamp(0.0, 1.0);
                if kk <= 0.0 {
                    continue;
                }
                let ee = 1.0 - (1.0 - kk).powi(3);
                stroke_circle(pm, tf, c.x, c.y, c.size * (0.15 + 0.85 * ee), 1.5 + 3.5 * (1.0 - kk), c.color, (1.0 - kk) * 0.9);
            }
        }
        ClickStyle::Pulse => {
            let r = c.size * (0.2 + 0.8 * e);
            if let (Some(p), Some(path)) = (glow_paint(c.x, c.y, r, c.color, 0.75 * fade), PathBuilder::from_circle(c.x, c.y, r)) {
                pm.fill_path(&path, &p, FillRule::Winding, tf, None);
            }
        }
        ClickStyle::Burst => {
            let n = 12;
            for i in 0..n {
                let a = i as f32 / n as f32 * TAU + 0.26;
                let (r1, r2) = (c.size * (0.25 + 0.45 * e), c.size * (0.45 + 0.65 * e));
                let mut pb = PathBuilder::new();
                pb.move_to(c.x + a.cos() * r1, c.y + a.sin() * r1);
                pb.line_to(c.x + a.cos() * r2, c.y + a.sin() * r2);
                if let Some(path) = pb.finish() {
                    let st = Stroke { width: 2.0 + 3.0 * fade, line_cap: LineCap::Round, ..Stroke::default() };
                    pm.stroke_path(&path, &solid(c.color, fade), &st, tf, None);
                }
            }
        }
        // les autres styles sont portés par des particules ; on garde un petit flash
        ClickStyle::Confetti | ClickStyle::Stars | ClickStyle::Hearts => {
            stroke_circle(pm, tf, c.x, c.y, c.size * 0.35 * (0.3 + 0.7 * e), 2.0 + 2.0 * fade, c.color, fade * 0.8);
        }
    }
}

// ------------------------------------------------------------------ helpers de dessin

fn solid(c: Rgba, a: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.0, c.1, c.2, (c.3 as f32 * a.clamp(0.0, 1.0)).round() as u8);
    p.anti_alias = true;
    p
}

fn glow_paint(cx: f32, cy: f32, r: f32, c: Rgba, a: f32) -> Option<Paint<'static>> {
    let col = |al: f32| Color::from_rgba8(c.0, c.1, c.2, (c.3 as f32 * al.clamp(0.0, 1.0)) as u8);
    let sh = RadialGradient::new(
        Point::from_xy(cx, cy),
        Point::from_xy(cx, cy),
        r.max(0.5),
        vec![GradientStop::new(0.0, col(a)), GradientStop::new(1.0, col(0.0))],
        SpreadMode::Pad,
        Transform::identity(),
    )?;
    let mut p = Paint::default();
    p.shader = sh;
    p.anti_alias = true;
    Some(p)
}

fn fill_glow(pm: &mut Pixmap, tf: Transform, x: f32, y: f32, r: f32, c: Rgba, a: f32) {
    if let (Some(p), Some(path)) = (glow_paint(x, y, r, c, a), PathBuilder::from_circle(x, y, r.max(0.5))) {
        pm.fill_path(&path, &p, FillRule::Winding, tf, None);
    }
}

fn stroke_circle(pm: &mut Pixmap, tf: Transform, x: f32, y: f32, r: f32, width: f32, c: Rgba, a: f32) {
    if a <= 0.003 {
        return;
    }
    if let Some(path) = PathBuilder::from_circle(x, y, r.max(0.5)) {
        let st = Stroke { width, ..Stroke::default() };
        pm.stroke_path(&path, &solid(c, a), &st, tf, None);
    }
}

/// Anneau à dégradé arc-en-ciel : 72 segments opaques dessinés sur un calque temporaire,
/// puis composés avec l'opacité voulue (évite les superpositions visibles si l'alpha < 255).
fn rgb_ring(pm: &mut Pixmap, tf: Transform, x: f32, y: f32, r: f32, width: f32, alpha: f32, hue0: f32) {
    if alpha <= 0.003 {
        return;
    }
    let r = r.min(700.0);
    let half = (r + width * 0.5 + 3.0).ceil();
    let sz = (half * 2.0) as u32;
    let Some(mut tmp) = Pixmap::new(sz, sz) else { return };
    let n = 72;
    for i in 0..n {
        let a0 = i as f32 / n as f32 * TAU;
        let a1 = (i + 1) as f32 / n as f32 * TAU;
        let mut pb = PathBuilder::new();
        pb.move_to(half + a0.cos() * r, half + a0.sin() * r);
        pb.line_to(half + a1.cos() * r, half + a1.sin() * r);
        if let Some(path) = pb.finish() {
            let col = hsv(hue0 + i as f32 / n as f32 * 360.0, 0.9, 1.0, 255);
            let st = Stroke { width, line_cap: LineCap::Round, ..Stroke::default() };
            tmp.stroke_path(&path, &solid(col, 1.0), &st, Transform::identity(), None);
        }
    }
    let pp = PixmapPaint { opacity: alpha.clamp(0.0, 1.0), quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    pm.draw_pixmap(0, 0, tmp.as_ref(), &pp, tf.pre_translate(x - half, y - half), None);
}

fn star_path(size: f32) -> Option<Path> {
    let (ro, ri) = (size * 0.55, size * 0.24);
    let mut pb = PathBuilder::new();
    for i in 0..10 {
        let a = -PI / 2.0 + i as f32 * PI / 5.0;
        let r = if i % 2 == 0 { ro } else { ri };
        let (x, y) = (a.cos() * r, a.sin() * r);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    pb.finish()
}

fn heart_path(size: f32) -> Option<Path> {
    let s = size * 0.5;
    let mut pb = PathBuilder::new();
    pb.move_to(0.0, 0.9 * s);
    pb.cubic_to(-0.2 * s, 0.7 * s, -1.0 * s, 0.2 * s, -1.0 * s, -0.3 * s);
    pb.cubic_to(-1.0 * s, -0.8 * s, -0.35 * s, -0.95 * s, 0.0, -0.5 * s);
    pb.cubic_to(0.35 * s, -0.95 * s, 1.0 * s, -0.8 * s, 1.0 * s, -0.3 * s);
    pb.cubic_to(1.0 * s, 0.2 * s, 0.2 * s, 0.7 * s, 0.0, 0.9 * s);
    pb.close();
    pb.finish()
}

pub fn hsv(h: f32, s: f32, v: f32, a: u8) -> Rgba {
    let h = h.rem_euclid(360.0) / 60.0;
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    Rgba(((r + m) * 255.0) as u8, ((g + m) * 255.0) as u8, ((b + m) * 255.0) as u8, a)
}

fn shift_hue(c: Rgba, deg: f32) -> Rgba {
    let (r, g, b) = (c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let s = if mx == 0.0 { 0.0 } else { d / mx };
    hsv(h + deg, s, mx, c.3)
}

// ------------------------------------------------------------------ curseur (généré ou skin)

fn build_cursor_cache(cfg: &CursorCfg, skin: &Skin) -> (Pixmap, Option<Pixmap>) {
    let size = cfg.size.round().max(16.0) as u32;
    match &skin.cursor {
        Some(img) => {
            let sc = size as f32 / img.width().max(img.height()) as f32;
            let main = scaled(img, sc);
            let mid = skin.middle.as_ref().map(|m| scaled(m, sc));
            (main, mid)
        }
        None => (generated_cursor(size, cfg.color, cfg.outline), None),
    }
}

fn scaled(img: &Pixmap, sc: f32) -> Pixmap {
    let w = ((img.width() as f32 * sc).round() as u32).max(1);
    let h = ((img.height() as f32 * sc).round() as u32).max(1);
    let mut out = Pixmap::new(w, h).unwrap();
    let pp = PixmapPaint { quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
    out.draw_pixmap(0, 0, img.as_ref(), &pp, Transform::from_scale(sc, sc), None);
    out
}

/// Curseur par défaut : disque coloré avec halo et point central. `outline` ajoute l'anneau blanc façon osu!.
pub fn generated_cursor(size: u32, color: Rgba, outline: bool) -> Pixmap {
    let mut pm = Pixmap::new(size, size).unwrap();
    let s = size as f32;
    let c = s / 2.0;
    let tf = Transform::identity();
    fill_glow(&mut pm, tf, c, c, s * 0.5, color, 0.55);
    if outline {
        if let (Some(p), Some(path)) = (glow_paint(c, c, s * 0.36, color, 0.55), PathBuilder::from_circle(c, c, s * 0.36)) {
            pm.fill_path(&path, &p, FillRule::Winding, tf, None);
        }
        stroke_circle(&mut pm, tf, c, c, s * 0.34, (s * 0.07).max(2.0), Rgba(255, 255, 255, 255), 1.0);
    } else if let Some(path) = PathBuilder::from_circle(c, c, s * 0.27) {
        pm.fill_path(&path, &solid(color, 0.92), FillRule::Winding, tf, None);
    }
    if let Some(path) = PathBuilder::from_circle(c, c, (s * 0.045).max(1.5)) {
        pm.fill_path(&path, &solid(Rgba(255, 255, 255, 255), 1.0), FillRule::Winding, tf, None);
    }
    pm
}

// ------------------------------------------------------------------ son de clic intégré

/// WAV mono 16 bits : petit « tic » (sinus descendant, enveloppe exponentielle), ~45 ms.
pub fn click_wav() -> Vec<u8> {
    let rate = 44_100u32;
    let n = (rate as f32 * 0.045) as usize;
    let mut pcm = Vec::with_capacity(n * 2);
    let mut phase = 0.0f32;
    for i in 0..n {
        let t = i as f32 / rate as f32;
        let freq = 1500.0 - 700.0 * (t / 0.045);
        phase += TAU * freq / rate as f32;
        let env = (-t * 95.0).exp();
        let v = (phase.sin() * env * 0.55 * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(&pcm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shake_detects_back_and_forth() {
        let mut fx = Fx::new(Settings::default(), Skin::default());
        let mut x = 500.0;
        let mut dir = 1.0;
        fx.set_pos((x, 300.0));
        for i in 0..40 {
            if i % 4 == 0 {
                dir = -dir;
            }
            x += dir * 25.0;
            fx.update(0.016);
            fx.set_pos((x, 300.0));
            if fx.shake_age >= 0.0 {
                return;
            }
        }
        panic!("shake non détecté");
    }

    #[test]
    fn slow_move_is_not_shake() {
        let mut fx = Fx::new(Settings::default(), Skin::default());
        for i in 0..200 {
            fx.update(0.016);
            fx.set_pos((100.0 + i as f32 * 3.0, 300.0));
        }
        assert!(fx.shake_age < 0.0);
    }

    #[test]
    fn annotation_draws_then_fades() {
        let mut fx = Fx::new(Settings::default(), Skin::default());
        fx.set_pos((100.0, 100.0));
        fx.set_annotating(true);
        for i in 1..=20 {
            fx.update(0.016);
            fx.set_annotating(true);
            fx.set_pos((100.0 + i as f32 * 10.0, 100.0 + (i as f32 * 0.5).sin() * 20.0));
        }
        assert_eq!(fx.strokes.len(), 1);
        assert!(fx.strokes[0].pts.len() > 10);
        fx.set_annotating(false);
        fx.set_pos((900.0, 900.0)); // après le relâchement : plus de points ajoutés
        assert!(fx.strokes[0].pts.iter().all(|p| p.0 < 400.0));
        assert!(fx.animating());
        for _ in 0..300 {
            fx.update(0.016);
        }
        assert!(fx.strokes.is_empty());
    }

    #[test]
    fn manual_clear() {
        let mut s = Settings::default();
        s.annotate.fade_delay = 0.0;
        let mut fx = Fx::new(s, Skin::default());
        fx.set_pos((10.0, 10.0));
        fx.set_annotating(true);
        fx.set_pos((50.0, 50.0));
        fx.set_annotating(false);
        for _ in 0..500 {
            fx.update(0.016);
        }
        assert_eq!(fx.strokes.len(), 1); // persiste
        fx.clear_annotations();
        assert!(fx.strokes.is_empty());
    }

    #[test]
    fn wav_is_valid() {
        let w = click_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert!(w.len() > 1000);
    }

    #[test]
    fn click_expires() {
        let mut fx = Fx::new(Settings::default(), Skin::default());
        fx.set_pos((10.0, 10.0));
        fx.mouse_button((10.0, 10.0), Button::Left, true);
        assert!(fx.animating());
        for _ in 0..200 {
            fx.update(0.016);
        }
        fx.mouse_button((10.0, 10.0), Button::Left, false);
        for _ in 0..30 {
            fx.update(0.016);
        }
        assert!(!fx.animating());
    }
}
