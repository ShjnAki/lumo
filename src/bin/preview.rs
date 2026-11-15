//! Rendu de contrôle sous Linux : écrit des planches PNG dans ./preview/
use lumo::fx::{Button, Fx};
use lumo::settings::*;
use lumo::skin::Skin;
use tiny_skia::*;

const W: u32 = 360;
const H: u32 = 220;

fn background() -> Pixmap {
    let mut pm = Pixmap::new(W, H).unwrap();
    pm.fill(Color::from_rgba8(34, 38, 52, 255));
    let mut p = Paint::default();
    p.set_color_rgba8(54, 60, 82, 255);
    for i in 0..4 {
        let r = Rect::from_xywh(16.0, 16.0 + i as f32 * 50.0, 328.0, 38.0).unwrap();
        pm.fill_rect(r, &p, Transform::identity(), None);
    }
    p.set_color_rgba8(235, 235, 240, 255);
    for i in 0..4 {
        let r = Rect::from_xywh(26.0, 30.0 + i as f32 * 50.0, 120.0 + i as f32 * 25.0, 8.0).unwrap();
        pm.fill_rect(r, &p, Transform::identity(), None);
    }
    pm
}

fn scenario(s: Settings, click_after: Option<usize>, frames: usize) -> Pixmap {
    let mut fx = Fx::new(s, Skin::default());
    let n = frames;
    for i in 0..n {
        let k = i as f32 / (n - 1) as f32;
        let x = 50.0 + k * 250.0;
        let y = 120.0 + (k * 7.0).sin() * 40.0;
        fx.update(1.0 / 60.0);
        fx.set_pos((x, y));
        if Some(i) == click_after {
            fx.mouse_button((x, y), Button::Left, true);
        }
    }
    let mut over = Pixmap::new(W, H).unwrap();
    fx.render(&mut over, (0, 0));
    let mut bg = background();
    bg.draw_pixmap(0, 0, over.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    bg
}

fn annotated(color: Rgba, width: f32, release: bool) -> Pixmap {
    let mut s = Settings::default();
    s.annotate.color = color;
    s.annotate.width = width;
    let mut fx = Fx::new(s, Skin::default());
    fx.set_pos((40.0, 150.0));
    for i in 0..60 {
        let k = i as f32 / 59.0;
        fx.update(1.0 / 60.0);
        fx.set_annotating(true);
        fx.set_pos((40.0 + k * 280.0, 110.0 + (k * 9.0).sin() * 45.0));
    }
    if release {
        fx.set_annotating(false);
        for _ in 0..170 {
            fx.update(1.0 / 60.0); // 2,8 s : le tracé est en train de s'effacer
        }
    }
    let mut over = Pixmap::new(W, H).unwrap();
    fx.render(&mut over, (0, 0));
    let mut bg = background();
    bg.draw_pixmap(0, 0, over.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    bg
}

fn sheet(name: &str, cells: Vec<Pixmap>, cols: u32) {
    let rows = (cells.len() as u32 + cols - 1) / cols;
    let mut out = Pixmap::new(cols * W, rows * H).unwrap();
    for (i, c) in cells.iter().enumerate() {
        let (cx, cy) = ((i as u32 % cols) * W, (i as u32 / cols) * H);
        out.draw_pixmap(cx as i32, cy as i32, c.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
    std::fs::create_dir_all("preview").unwrap();
    out.save_png(format!("preview/{name}.png")).unwrap();
}

fn main() {
    // 1) spotlight / ring / curseur
    let mut cells = vec![];
    let mut s = Settings::default();
    s.spotlight.enabled = true;
    cells.push(scenario(s.clone(), None, 40));
    s.spotlight.shape = SpotShape::HBar;
    s.spotlight.size = 45.0;
    cells.push(scenario(s.clone(), None, 40));
    s.spotlight.shape = SpotShape::VBar;
    cells.push(scenario(s.clone(), None, 40));
    let mut s2 = Settings::default();
    s2.ring.glow = true;
    s2.ring.color = Rgba(0, 255, 200, 230);
    cells.push(scenario(s2, None, 40));
    let mut s2b = Settings::default();
    s2b.ring.rgb = true;
    s2b.ring.size = 34.0;
    s2b.ring.thickness = 6.0;
    s2b.ring.color = Rgba(255, 255, 255, 255);
    s2b.ring.glow = true;
    cells.push(scenario(s2b, None, 40));
    let mut s3 = Settings::default();
    s3.ring.enabled = false;
    s3.cursor.mode = CursorMode::Overlay;
    cells.push(scenario(s3.clone(), Some(38), 60));
    s3.cursor.outline = true; // pour comparer avec l'ancien rendu
    s3.cursor.rotate = true;
    s3.ring.enabled = true;
    cells.push(scenario(s3, None, 40));
    sheet("1_spot_ring_cursor", cells, 3);

    // 2) traînées
    let styles = [TrailStyle::Line, TrailStyle::Comet, TrailStyle::Glow, TrailStyle::Dots, TrailStyle::Stars, TrailStyle::Hearts, TrailStyle::Rainbow, TrailStyle::Skin];
    let mut cells = vec![];
    for st in styles {
        let mut s = Settings::default();
        s.ring.enabled = false;
        s.trail.enabled = true;
        s.trail.style = st;
        if st == TrailStyle::Hearts {
            s.trail.color = Rgba(255, 105, 180, 255);
        }
        cells.push(scenario(s, None, 70));
    }
    sheet("2_trails", cells, 4);

    // 3) clics
    let cs = [ClickStyle::Ring, ClickStyle::Ripple, ClickStyle::Pulse, ClickStyle::Burst, ClickStyle::Confetti, ClickStyle::Stars, ClickStyle::Hearts];
    let mut cells = vec![];
    for st in cs {
        let mut s = Settings::default();
        s.click.style = st;
        s.click.size = 70.0;
        cells.push(scenario(s, Some(30), 44));
    }
    sheet("3_clicks", cells, 4);
    // 4) annotations
    sheet(
        "4_annotations",
        vec![
            annotated(Rgba(255, 59, 48, 255), 5.0, false),
            annotated(Rgba(255, 214, 10, 255), 10.0, false),
            annotated(Rgba(10, 200, 120, 255), 5.0, true),
        ],
        3,
    );
    println!("ok -> preview/");
}
