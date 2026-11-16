//! Couche Windows : fenêtres overlay (une par écran), hook souris, icône de notification,
//! raccourcis globaux, sons et remplacement du curseur système.
//!
//! Tout tourne sur UN SEUL thread (celui de la boucle de messages) : le hook souris bas niveau,
//! le WM_TIMER qui cadence le rendu et la fenêtre de contrôle. L'état partagé vit donc dans des
//! `thread_local!` sans verrou.
//!
//! Rendu : seule la zone qui change (`Fx::damage`, plus la zone de l'image précédente) est
//! redessinée, convertie et envoyée à Windows (`UpdateLayeredWindowIndirect` + `prcDirty`).
//! Chaque mouvement de souris vu par le hook déclenche une image sans attendre le minuteur.

#![allow(clippy::missing_safety_doc)]

use lumo::fx::{Button, Fx};
use lumo::settings::{parse_hold, parse_hotkey, ClickStyle, CursorMode, Rgba, Settings, SpotShape, TrailStyle};
use lumo::skin::Skin;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant, SystemTime};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::Media::Audio::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, MOD_NOREPEAT};
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

#[link(name = "winmm")]
extern "system" {
    fn timeBeginPeriod(period: u32) -> u32;
}

const WM_TRAY: u32 = WM_USER + 1;
/// Posté par le hook souris : la souris a bougé, dessiner une image tout de suite.
const WM_KICK: u32 = WM_USER + 2;
const TIMER_ID: usize = 1;
const TIMER_MS: u32 = 8;
/// Écart minimal entre deux images déclenchées par la souris (une souris à 1000 Hz n'impose pas 1000 images/s).
const KICK_MIN: Duration = Duration::from_millis(3);
const WM_CONTEXTMENU_: u32 = 0x007B;
/// Ouvre la fenêtre de réglages depuis la boucle de messages, hors de tout emprunt de l'App :
/// la création de la WebView fait tourner des messages (rendu des effets compris).
const WM_OPEN_SETTINGS: u32 = WM_APP + 10;
/// Ordre des raccourcis dans `register_hotkeys` (identifiant = index + 1).
const HOTKEY_PATHS: [&str; 8] = [
    "hotkeys.toggle_all",
    "hotkeys.toggle_spotlight",
    "hotkeys.toggle_ring",
    "hotkeys.toggle_trail",
    "hotkeys.toggle_clicks",
    "hotkeys.toggle_cursor",
    "hotkeys.toggle_annotate",
    "hotkeys.clear_annotations",
];

/// Curseurs système remplacés (flèche, texte, attente, croix, redimensionnements, main, etc.).
const CURSOR_IDS: [u32; 13] = [
    32512, 32513, 32514, 32515, 32516, 32642, 32643, 32644, 32645, 32646, 32648, 32649, 32650,
];

const TRAILS: [(TrailStyle, &str); 8] = [
    (TrailStyle::Line, "Ligne"),
    (TrailStyle::Comet, "Comète"),
    (TrailStyle::Glow, "Lueur"),
    (TrailStyle::Dots, "Points"),
    (TrailStyle::Stars, "Étoiles"),
    (TrailStyle::Hearts, "Cœurs (kawaii)"),
    (TrailStyle::Rainbow, "Arc-en-ciel"),
    (TrailStyle::Skin, "Skin osu! (cursortrail.png)"),
];
const CLICKS: [(ClickStyle, &str); 7] = [
    (ClickStyle::Ring, "Anneau"),
    (ClickStyle::Ripple, "Onde (ripple)"),
    (ClickStyle::Pulse, "Pulsation"),
    (ClickStyle::Burst, "Rayons"),
    (ClickStyle::Confetti, "Confettis"),
    (ClickStyle::Stars, "Étoiles"),
    (ClickStyle::Hearts, "Cœurs"),
];
const SHAPES: [(SpotShape, &str); 3] = [
    (SpotShape::Circle, "Cercle"),
    (SpotShape::HBar, "Barre horizontale"),
    (SpotShape::VBar, "Barre verticale"),
];
const CURSORS: [(CursorMode, &str); 3] = [
    (CursorMode::Off, "Curseur Windows (inchangé)"),
    (CursorMode::System, "Image système (latence nulle)"),
    (CursorMode::Overlay, "Overlay (échelle au clic, rotation)"),
];
const COLORS: [(&str, &str); 8] = [
    ("Jaune", "#ffc400dc"),
    ("Rouge", "#ff4d6d"),
    ("Rose", "#ff66aa"),
    ("Cyan", "#00e5ff"),
    ("Vert", "#39ff88"),
    ("Violet", "#a06bff"),
    ("Orange", "#ff8a00"),
    ("Blanc", "#ffffff"),
];
const SPOT_SIZES: [f32; 4] = [120.0, 180.0, 260.0, 380.0];
const SPOT_DIMS: [f32; 4] = [0.4, 0.6, 0.75, 0.9];

pub(crate) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ------------------------------------------------------------------ état partagé (thread unique)

struct Ev {
    button: Button,
    down: bool,
    x: i32,
    y: i32,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static EVENTS: RefCell<Vec<Ev>> = const { RefCell::new(Vec::new()) };
    /// Fenêtre de contrôle (destinataire de WM_KICK) et « un WM_KICK est déjà en file ».
    static CTL_HWND: Cell<usize> = const { Cell::new(0) };
    static KICK_PENDING: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    // try_borrow_mut : si un message arrive pendant qu'on est déjà dans l'App, on l'ignore
    // plutôt que de paniquer.
    APP.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

// ------------------------------------------------------------------ overlay (une fenêtre par écran)

/// Rectangle en coordonnées locales à un écran : (x0, y0, x1, y1), x1 et y1 exclus.
type Zone = (i32, i32, i32, i32);

fn union(a: Option<Zone>, b: Option<Zone>) -> Option<Zone> {
    match (a, b) {
        (Some(a), Some(b)) => Some((a.0.min(b.0), a.1.min(b.1), a.2.max(b.2), a.3.max(b.3))),
        (a, b) => a.or(b),
    }
}

struct Overlay {
    hwnd: HWND,
    rect: (i32, i32, i32, i32),
    mem_dc: HDC,
    bmp: HBITMAP,
    old_bmp: HGDIOBJ,
    /// Image complète de l'écran (BGRA prémultiplié), mise à jour zone par zone.
    bits: *mut u8,
    shown: bool,
    /// Faux tant que Windows n'a pas reçu une image complète (création, changement de réglages).
    valid: bool,
    /// Zone dessinée à l'image précédente : à effacer si plus rien ne s'y trouve.
    prev: Option<Zone>,
}

impl Overlay {
    unsafe fn new(rect: (i32, i32, i32, i32), class: *const u16, hinst: HINSTANCE) -> Option<Overlay> {
        let (x, y, w, h) = rect;
        if w <= 0 || h <= 0 {
            return None;
        }
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class,
            wide("Lumo overlay").as_ptr(),
            WS_POPUP,
            x,
            y,
            w,
            h,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            return None;
        }
        let screen = GetDC(null_mut());
        let mem_dc = CreateCompatibleDC(screen);
        let mut bmi: BITMAPINFO = zeroed();
        bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h; // top-down
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        let mut bits: *mut c_void = null_mut();
        let bmp = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        ReleaseDC(null_mut(), screen);
        if bmp.is_null() || bits.is_null() {
            DeleteDC(mem_dc);
            DestroyWindow(hwnd);
            return None;
        }
        let old_bmp = SelectObject(mem_dc, bmp as HGDIOBJ);
        Some(Overlay { hwnd, rect, mem_dc, bmp, old_bmp, bits: bits as *mut u8, shown: false, valid: false, prev: None })
    }

    /// Dessine la zone `z` de l'écran avec `fx`, la recopie dans le DIB (RGBA -> BGRA) et
    /// l'envoie à Windows. Le reste du DIB n'est pas touché.
    unsafe fn paint(&mut self, fx: &Fx, z: Zone) {
        let (x, y, w, h) = self.rect;
        let (zw, zh) = ((z.2 - z.0) as usize, (z.3 - z.1) as usize);
        let Some(mut pm) = Pixmap::new(zw as u32, zh as u32) else { return };
        fx.render(&mut pm, (x + z.0, y + z.1));
        let src = pm.data();
        let stride = w as usize * 4;
        for row in 0..zh {
            let start = (z.1 as usize + row) * stride + z.0 as usize * 4;
            let dst = std::slice::from_raw_parts_mut(self.bits.add(start), zw * 4);
            for (d, s) in dst.chunks_exact_mut(4).zip(src[row * zw * 4..(row + 1) * zw * 4].chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = s[3];
            }
        }
        let dst_pt = POINT { x, y };
        let src_pt = POINT { x: 0, y: 0 };
        let size = SIZE { cx: w, cy: h };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let dirty = RECT { left: z.0, top: z.1, right: z.2, bottom: z.3 };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: null_mut(),
            pptDst: &dst_pt,
            psize: &size,
            hdcSrc: self.mem_dc,
            pptSrc: &src_pt,
            crKey: 0,
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            // la première image doit être complète : Windows n'a encore rien pour cette fenêtre
            prcDirty: if self.valid { &dirty } else { null() },
        };
        UpdateLayeredWindowIndirect(self.hwnd, &info);
    }

    /// Partie de `r` (coordonnées bureau, f32) qui tombe sur cet écran, en coordonnées locales.
    fn clip(&self, r: &[f32; 4]) -> Option<Zone> {
        let (x, y, w, h) = self.rect;
        let lo = |v: f32, o: i32, n: i32| ((v.floor() as f64 - o as f64).clamp(0.0, n as f64)) as i32;
        let hi = |v: f32, o: i32, n: i32| ((v.ceil() as f64 - o as f64).clamp(0.0, n as f64)) as i32;
        let z = (lo(r[0], x, w), lo(r[1], y, h), hi(r[2], x, w), hi(r[3], y, h));
        (z.2 > z.0 && z.3 > z.1).then_some(z)
    }

    unsafe fn set_shown(&mut self, show: bool) {
        if show != self.shown {
            ShowWindow(self.hwnd, if show { SW_SHOWNOACTIVATE } else { SW_HIDE });
            self.shown = show;
        }
    }
}

impl Drop for Overlay {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem_dc, self.old_bmp);
            DeleteObject(self.bmp as HGDIOBJ);
            DeleteDC(self.mem_dc);
            DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn enum_monitor(_m: HMONITOR, _dc: HDC, r: *mut RECT, lp: LPARAM) -> BOOL {
    let list = &mut *(lp as *mut Vec<(i32, i32, i32, i32)>);
    let r = &*r;
    list.push((r.left, r.top, r.right - r.left, r.bottom - r.top));
    1
}

fn monitors() -> Vec<(i32, i32, i32, i32)> {
    let mut v: Vec<(i32, i32, i32, i32)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(null_mut(), null(), Some(enum_monitor), &mut v as *mut _ as LPARAM);
    }
    v
}

// ------------------------------------------------------------------ curseurs / icônes

/// Construit un HICON/HCURSOR 32 bits avec alpha depuis un Pixmap (RGBA prémultiplié).
pub(crate) unsafe fn make_icon(img: &Pixmap, as_cursor: bool) -> HICON {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let screen = GetDC(null_mut());
    let mut bmi: BITMAPINFO = zeroed();
    bmi.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
    bmi.bmiHeader.biWidth = w;
    bmi.bmiHeader.biHeight = -h;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;
    let mut bits: *mut c_void = null_mut();
    let color = CreateDIBSection(screen, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
    ReleaseDC(null_mut(), screen);
    if color.is_null() || bits.is_null() {
        return null_mut();
    }
    let src = img.data();
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, src.len());
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
    // masque monochrome : ignoré quand la couleur porte de l'alpha, mais obligatoire
    let mask_bytes = (((w + 15) / 16) * 2 * h) as usize;
    let zeros = vec![0u8; mask_bytes];
    let mask = CreateBitmap(w, h, 1, 1, zeros.as_ptr() as *const c_void);
    let info = ICONINFO {
        fIcon: if as_cursor { 0 } else { 1 },
        xHotspot: (w / 2) as u32,
        yHotspot: (h / 2) as u32,
        hbmMask: mask,
        hbmColor: color,
    };
    let icon = CreateIconIndirect(&info);
    DeleteObject(color as HGDIOBJ);
    DeleteObject(mask as HGDIOBJ);
    icon
}

/// Les curseurs Windows dépassent rarement 128 px : on réduit si besoin.
fn fit_cursor(img: Pixmap, max: u32) -> Pixmap {
    let m = img.width().max(img.height());
    if m <= max {
        return img;
    }
    let sc = max as f32 / m as f32;
    let (w, h) = (((img.width() as f32 * sc) as u32).max(1), ((img.height() as f32 * sc) as u32).max(1));
    let mut out = Pixmap::new(w, h).unwrap();
    let pp = PixmapPaint { quality: tiny_skia::FilterQuality::Bilinear, ..PixmapPaint::default() };
    out.draw_pixmap(0, 0, img.as_ref(), &pp, Transform::from_scale(sc, sc), None);
    out
}

unsafe fn apply_cursor_image(img: &Pixmap) {
    for id in CURSOR_IDS {
        // SetSystemCursor détruit le handle reçu : il en faut un neuf par identifiant.
        let cur = make_icon(img, true);
        if !cur.is_null() {
            SetSystemCursor(cur, id);
        }
    }
}

pub unsafe fn restore_cursors() {
    SystemParametersInfoW(SPI_SETCURSORS, 0, null_mut(), 0);
}

// ------------------------------------------------------------------ application

pub(crate) struct App {
    hwnd: HWND,
    hinst: HINSTANCE,
    overlay_class: Vec<u16>,
    overlays: Vec<Overlay>,
    fx: Fx,
    path: PathBuf,
    mtime: Option<SystemTime>,
    skin_dir_loaded: String,
    last_check: Instant,
    last_tick: Instant,
    last_pos: (i32, i32),
    dirty: bool,
    /// L'image précédente était animée : une image de plus pour effacer sa dernière trace.
    was_animating: bool,
    cursor_key: Option<String>,
    hotkey_ids: Vec<i32>,
    /// Raccourcis valides que Windows a refusés (déjà pris ailleurs).
    hotkey_taken: Vec<&'static str>,
    annotate_vks: Vec<u32>,
    wav: &'static [u8],
    sound_path: Vec<u16>,
    hook: HHOOK,
}

/// 0 = inconnu (à écrire au premier passage, ce qui remet aussi à jour le chemin de l'exe s'il a
/// bougé), 1 = démarrage automatique actif, 2 = inactif.
static AUTOSTART: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Ajoute ou retire Lumo de `HKCU\...\Run` (démarrage à l'ouverture de session, sans droits admin).
/// Lancé avec `--minimized` : pas de fenêtre de réglages au démarrage, juste l'icône de notification.
unsafe fn set_autostart(on: bool) {
    use std::sync::atomic::Ordering;
    use windows_sys::Win32::System::Registry::*;
    let want = if on { 1 } else { 2 };
    if AUTOSTART.load(Ordering::Relaxed) == want {
        return;
    }
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    let name = wide("Lumo");
    let mut hkey: HKEY = null_mut();
    if RegCreateKeyExW(HKEY_CURRENT_USER, key.as_ptr(), 0, null(), 0, KEY_SET_VALUE, null(), &mut hkey, null_mut()) != 0 {
        return;
    }
    let ok = if on {
        match std::env::current_exe() {
            Ok(exe) => {
                let cmd = wide(&format!("\"{}\" --minimized", exe.display()));
                RegSetValueExW(hkey, name.as_ptr(), 0, REG_SZ, cmd.as_ptr() as *const u8, (cmd.len() * 2) as u32) == 0
            }
            Err(_) => false,
        }
    } else {
        let r = RegDeleteValueW(hkey, name.as_ptr());
        r == 0 || r == ERROR_FILE_NOT_FOUND
    };
    RegCloseKey(hkey);
    if ok {
        AUTOSTART.store(want, Ordering::Relaxed);
    }
}

fn settings_path() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    base.join("Lumo").join("settings.json")
}

fn mtime_of(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn load_skin(dir: &str) -> Skin {
    if dir.trim().is_empty() {
        Skin::default()
    } else {
        Skin::load(Path::new(dir))
    }
}

impl App {
    unsafe fn rebuild_overlays(&mut self) {
        self.overlays.clear(); // Drop détruit les fenêtres
        for rect in monitors() {
            if let Some(o) = Overlay::new(rect, self.overlay_class.as_ptr(), self.hinst) {
                self.overlays.push(o);
            }
        }
        self.dirty = true;
    }

    fn save(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.path, self.fx.s.to_json());
        self.mtime = mtime_of(&self.path);
    }

    /// À appeler après toute modification de `self.fx.s` faite par l'app (menu, raccourcis).
    unsafe fn commit(&mut self) {
        let s = self.fx.s.clone();
        self.fx.set_settings(s, None);
        self.save();
        self.after_settings_change();
    }

    /// Réglages envoyés par la fenêtre de réglages (page locale).
    unsafe fn apply_external(&mut self, s: Settings) {
        let skin = if s.cursor.skin_dir != self.skin_dir_loaded {
            self.skin_dir_loaded = s.cursor.skin_dir.clone();
            Some(load_skin(&s.cursor.skin_dir))
        } else {
            None
        };
        self.fx.set_settings(s, skin);
        self.save();
        self.after_settings_change();
    }

    /// Prévient la fenêtre de réglages (si elle est ouverte) que les réglages ont changé.
    fn publish(&self) {
        crate::winui::notify_refresh();
    }

    unsafe fn open_settings(&self) {
        PostMessageW(self.hwnd, WM_OPEN_SETTINGS, 0, 0);
    }

    // --- interface utilisée par la fenêtre de réglages (winui.rs)

    pub(crate) fn ui_snapshot(&self) -> Settings {
        self.fx.s.clone()
    }

    pub(crate) unsafe fn ui_apply(&mut self, s: Settings) {
        self.apply_external(s);
    }

    /// Vrai si `path` désigne un raccourci que Windows a refusé (déjà utilisé ailleurs).
    pub(crate) fn ui_hotkey_taken(&self, path: &str) -> bool {
        self.hotkey_taken.contains(&path)
    }

    pub(crate) unsafe fn ui_action(&mut self, name: &str) {
        match name {
            "clear_annotations" => {
                self.fx.clear_annotations();
                self.dirty = true;
            }
            "restore_cursors" => {
                restore_cursors();
                self.cursor_key = None;
            }
            "open_json" => self.open_json(),
            _ => {}
        }
    }

    unsafe fn open_json(&mut self) {
        self.save();
        shell_open(&self.path.to_string_lossy(), None);
    }

    unsafe fn after_settings_change(&mut self) {
        self.sound_path = wide(&self.fx.s.sound.file);
        set_autostart(self.fx.s.start_with_windows);
        self.annotate_vks = parse_hold(&self.fx.s.annotate.key).unwrap_or_default();
        self.register_hotkeys();
        self.sync_cursor();
        self.dirty = true;
        self.publish();
    }

    unsafe fn check_file(&mut self) {
        let m = mtime_of(&self.path);
        if m.is_none() || m == self.mtime {
            return;
        }
        self.mtime = m;
        let Ok(text) = std::fs::read_to_string(&self.path) else { return };
        // JSON invalide pendant l'édition : on garde la dernière configuration valide.
        let Ok(s) = Settings::from_json(&text) else { return };
        let skin = if s.cursor.skin_dir != self.skin_dir_loaded {
            self.skin_dir_loaded = s.cursor.skin_dir.clone();
            Some(load_skin(&s.cursor.skin_dir))
        } else {
            None
        };
        self.fx.set_settings(s, skin);
        self.after_settings_change();
    }

    unsafe fn sync_cursor(&mut self) {
        let c = &self.fx.s.cursor;
        let want = if self.fx.s.enabled { c.mode } else { CursorMode::Off };
        let key = match want {
            CursorMode::Off => None,
            m => Some(format!("{:?}|{}|{:?}|{}", m, c.size, c.color, c.skin_dir)),
        };
        if key == self.cursor_key {
            return;
        }
        restore_cursors();
        match want {
            CursorMode::Off => {}
            CursorMode::System => apply_cursor_image(&fit_cursor(self.fx.cursor_image(), 128)),
            CursorMode::Overlay => apply_cursor_image(&Pixmap::new(32, 32).unwrap()), // transparent
        }
        self.cursor_key = key;
    }

    unsafe fn register_hotkeys(&mut self) {
        for id in self.hotkey_ids.drain(..) {
            UnregisterHotKey(self.hwnd, id);
        }
        self.hotkey_taken.clear();
        let hk = self.fx.s.hotkeys.clone();
        let list = [
            hk.toggle_all,
            hk.toggle_spotlight,
            hk.toggle_ring,
            hk.toggle_trail,
            hk.toggle_clicks,
            hk.toggle_cursor,
            hk.toggle_annotate,
            hk.clear_annotations,
        ];
        for (i, text) in list.iter().enumerate() {
            let id = i as i32 + 1;
            if let Some((mods, vk)) = parse_hotkey(text) {
                if RegisterHotKey(self.hwnd, id, mods | MOD_NOREPEAT, vk) != 0 {
                    self.hotkey_ids.push(id);
                } else {
                    self.hotkey_taken.push(HOTKEY_PATHS[i]);
                }
            }
        }
    }

    unsafe fn on_hotkey(&mut self, id: i32) {
        if id == 8 {
            self.fx.clear_annotations();
            self.dirty = true;
            return;
        }
        let s = &mut self.fx.s;
        match id {
            1 => s.enabled = !s.enabled,
            2 => s.spotlight.enabled = !s.spotlight.enabled,
            3 => s.ring.enabled = !s.ring.enabled,
            4 => s.trail.enabled = !s.trail.enabled,
            5 => s.click.enabled = !s.click.enabled,
            6 => {
                s.cursor.mode = if s.cursor.mode == CursorMode::Off { CursorMode::Overlay } else { CursorMode::Off };
            }
            7 => s.annotate.enabled = !s.annotate.enabled,
            _ => return,
        }
        self.commit();
    }

    unsafe fn on_command(&mut self, id: u32) {
        let s = &mut self.fx.s;
        match id {
            1 => s.enabled = !s.enabled,
            10 => s.spotlight.enabled = !s.spotlight.enabled,
            11 => s.ring.enabled = !s.ring.enabled,
            12 => s.trail.enabled = !s.trail.enabled,
            13 => s.click.enabled = !s.click.enabled,
            14 => s.sound.enabled = !s.sound.enabled,
            15 => s.shake_to_locate = !s.shake_to_locate,
            16 => s.ring.pulse = !s.ring.pulse,
            17 => s.ring.glow = !s.ring.glow,
            18 => s.annotate.enabled = !s.annotate.enabled,
            19 => s.ring.rgb = !s.ring.rgb,
            20 => s.start_with_windows = !s.start_with_windows,
            100..=102 => s.spotlight.shape = SHAPES[(id - 100) as usize].0,
            110..=117 => s.trail.style = TRAILS[(id - 110) as usize].0,
            120..=126 => s.click.style = CLICKS[(id - 120) as usize].0,
            130..=132 => s.cursor.mode = CURSORS[(id - 130) as usize].0,
            140..=147 => {
                if let Some(c) = Rgba::parse(COLORS[(id - 140) as usize].1) {
                    s.ring.color = c;
                }
            }
            160..=163 => s.spotlight.size = SPOT_SIZES[(id - 160) as usize],
            170..=173 => s.spotlight.dim = SPOT_DIMS[(id - 170) as usize],
            200 => {
                self.open_json();
                return;
            }
            204 => {
                self.open_settings();
                return;
            }
            203 => {
                self.fx.clear_annotations();
                self.dirty = true;
                return;
            }
            201 => {
                self.mtime = None; // force le rechargement au prochain tick
                return;
            }
            202 => {
                restore_cursors();
                self.cursor_key = None;
                return;
            }
            299 => {
                self.remove_tray();
                DestroyWindow(self.hwnd);
                return;
            }
            _ => return,
        }
        self.commit();
    }

    unsafe fn add_tray(&mut self) {
        let icon = make_icon(&lumo::fx::generated_cursor(32, Rgba(255, 102, 170, 255), true), false);
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon;
        for (i, c) in "Lumo - clic : réglages".encode_utf16().take(127).enumerate() {
            nid.szTip[i] = c;
        }
        Shell_NotifyIconW(NIM_ADD, &nid);
    }

    unsafe fn remove_tray(&mut self) {
        let mut nid: NOTIFYICONDATAW = zeroed();
        nid.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = 1;
        Shell_NotifyIconW(NIM_DELETE, &nid);
    }

    unsafe fn play_click(&self) {
        let snd = &self.fx.s.sound;
        if !snd.enabled {
            return;
        }
        let flags_common = SND_ASYNC | SND_NODEFAULT;
        if snd.file.trim().is_empty() {
            PlaySoundW(self.wav.as_ptr() as *const u16, null_mut(), SND_MEMORY | flags_common);
        } else {
            PlaySoundW(self.sound_path.as_ptr(), null_mut(), SND_FILENAME | flags_common);
        }
    }

    /// Une itération de la boucle : entrées, simulation, rendu.
    unsafe fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;


        if now.duration_since(self.last_check) > Duration::from_millis(500) {
            self.last_check = now;
            self.check_file();
        }

        // annotation : toutes les touches configurées sont tenues (bit de poids fort de GetAsyncKeyState)
        let held = self.fx.s.annotate.enabled
            && !self.annotate_vks.is_empty()
            && self.annotate_vks.iter().all(|&vk| GetAsyncKeyState(vk as i32) < 0);
        self.fx.set_annotating(held);

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let moved = (pt.x, pt.y) != self.last_pos;
        self.last_pos = (pt.x, pt.y);
        self.fx.set_pos((pt.x as f32, pt.y as f32));

        let evs: Vec<Ev> = EVENTS.with(|e| std::mem::take(&mut *e.borrow_mut()));
        let had_events = !evs.is_empty();
        for ev in evs {
            self.fx.mouse_button((ev.x as f32, ev.y as f32), ev.button, ev.down);
            if ev.down {
                self.play_click();
            }
        }

        self.fx.update(dt);

        let animating = self.fx.animating();
        if moved || had_events || self.dirty || animating || self.was_animating {
            let full = std::mem::take(&mut self.dirty);
            self.render_all(full);
        }
        self.was_animating = animating;
    }

    /// Image déclenchée par un mouvement de souris (hook), au plus une toutes les `KICK_MIN`.
    unsafe fn kick(&mut self) {
        if self.last_tick.elapsed() >= KICK_MIN {
            self.tick();
        }
    }

    /// `full` : tout repeindre (réglages changés). Sinon, seulement ce qui a bougé.
    unsafe fn render_all(&mut self, full: bool) {
        let rects = self.fx.damage();
        // le fond assombri du spotlight couvre tous les écrans, mais ne change qu'avec les réglages
        let spot = self.fx.s.enabled && self.fx.s.spotlight.enabled;
        for ov in self.overlays.iter_mut() {
            let (_, _, w, h) = ov.rect;
            let cur = rects.iter().filter_map(|r| ov.clip(r)).reduce(|a, b| union(Some(a), Some(b)).unwrap());
            let needed = spot || cur.is_some();
            let zone = if full || !ov.valid {
                needed.then_some((0, 0, w, h)) // un écran inutile n'est pas repeint : il reste masqué
            } else {
                union(ov.prev, cur)
            };
            ov.prev = cur;
            match zone {
                Some(z) => {
                    ov.paint(&self.fx, z);
                    ov.valid = true;
                }
                // écran masqué alors que son image n'est plus à jour : tout repeindre au retour
                None if !needed && full => ov.valid = false,
                None => {}
            }
            ov.set_shown(needed); // affichée APRÈS le premier UpdateLayeredWindow
        }
    }
}

// ------------------------------------------------------------------ callbacks

unsafe extern "system" fn ll_mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let ms = &*(lparam as *const MSLLHOOKSTRUCT);
        let ev = match wparam as u32 {
            WM_LBUTTONDOWN => Some((Button::Left, true)),
            WM_LBUTTONUP => Some((Button::Left, false)),
            WM_RBUTTONDOWN => Some((Button::Right, true)),
            WM_RBUTTONUP => Some((Button::Right, false)),
            WM_MBUTTONDOWN => Some((Button::Middle, true)),
            WM_MBUTTONUP => Some((Button::Middle, false)),
            _ => None,
        };
        if wparam as u32 == WM_MOUSEMOVE {
            kick();
        }
        if let Some((button, down)) = ev {
            let _ = EVENTS.try_with(|e| {
                if let Ok(mut v) = e.try_borrow_mut() {
                    if v.len() < 256 {
                        v.push(Ev { button, down, x: ms.pt.x, y: ms.pt.y });
                    }
                }
            });
        }
    }
    CallNextHookEx(null_mut(), code, wparam, lparam)
}

/// Demande une image (depuis le hook) : un seul WM_KICK en file à la fois. Le hook doit rendre
/// la main très vite, le rendu se fait donc plus tard, dans la boucle de messages.
fn kick() {
    let _ = KICK_PENDING.try_with(|p| {
        let h = CTL_HWND.with(|c| c.get());
        if !p.get() && h != 0 {
            p.set(true);
            unsafe {
                PostMessageW(h as HWND, WM_KICK, 0, 0);
            }
        }
    });
}

unsafe extern "system" fn ctl_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            with_app(|a| a.tick());
            0
        }
        WM_KICK => {
            KICK_PENDING.with(|p| p.set(false));
            with_app(|a| a.kick());
            0
        }
        WM_HOTKEY => {
            with_app(|a| a.on_hotkey(wp as i32));
            0
        }
        WM_OPEN_SETTINGS => {
            // copie des réglages, emprunt relâché, puis création de la fenêtre
            if let Some((hinst, s)) = with_app(|a| (a.hinst, a.fx.s.clone())) {
                crate::winui::open(hinst, &s);
            }
            0
        }
        WM_DISPLAYCHANGE => {
            with_app(|a| a.rebuild_overlays());
            0
        }
        WM_TRAY => {
            let ev = (lp as u32) & 0xFFFF;
            if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU_ {
                show_menu(hwnd);
            } else if ev == WM_LBUTTONUP {
                with_app(|a| a.on_command(204));
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

// ------------------------------------------------------------------ menu de l'icône

unsafe fn add_item(menu: HMENU, id: u32, text: &str, checked: bool) {
    let flags = MF_STRING | if checked { MF_CHECKED } else { 0 };
    AppendMenuW(menu, flags, id as usize, wide(text).as_ptr());
}

unsafe fn add_sep(menu: HMENU) {
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
}

unsafe fn add_sub(menu: HMENU, text: &str, build: impl FnOnce(HMENU)) {
    let sub = CreatePopupMenu();
    build(sub);
    AppendMenuW(menu, MF_POPUP, sub as usize, wide(text).as_ptr());
}

/// ShellExecute « open » ; vrai si Windows a pu lancer la cible (valeur de retour > 32).
unsafe fn shell_open(file: &str, params: Option<&str>) -> bool {
    let p = params.map(wide);
    let r = ShellExecuteW(
        null_mut(),
        wide("open").as_ptr(),
        wide(file).as_ptr(),
        p.as_ref().map_or(null(), |v| v.as_ptr()),
        null(),
        SW_SHOWNORMAL,
    );
    r as usize > 32
}

unsafe fn show_menu(hwnd: HWND) {
    // On copie les réglages puis on relâche l'emprunt AVANT TrackPopupMenu (boucle modale :
    // des WM_TIMER arrivent pendant que le menu est ouvert).
    let Some(s) = with_app(|a| a.fx.s.clone()) else { return };
    let menu = CreatePopupMenu();

    add_item(menu, 204, "Réglages…", false);
    add_sep(menu);
    add_item(menu, 1, "Effets activés", s.enabled);
    add_sep(menu);
    add_item(menu, 10, "Spotlight", s.spotlight.enabled);
    add_item(menu, 11, "Anneau (ring)", s.ring.enabled);
    add_item(menu, 12, "Traînée", s.trail.enabled);
    add_item(menu, 13, "Effets de clic", s.click.enabled);
    add_item(menu, 14, "Son de clic", s.sound.enabled);
    add_item(menu, 15, "Secouer pour localiser", s.shake_to_locate);
    add_item(menu, 18, &format!("Annotations (maintenir {})", s.annotate.key), s.annotate.enabled);
    add_sep(menu);
    add_item(menu, 20, "Lancer au démarrage de Windows", s.start_with_windows);
    add_sep(menu);
    add_sub(menu, "Curseur", |m| {
        for (i, (mode, name)) in CURSORS.iter().enumerate() {
            add_item(m, 130 + i as u32, name, s.cursor.mode == *mode);
        }
    });
    add_sub(menu, "Forme du spotlight", |m| {
        for (i, (v, name)) in SHAPES.iter().enumerate() {
            add_item(m, 100 + i as u32, name, s.spotlight.shape == *v);
        }
        add_sep(m);
        for (i, sz) in SPOT_SIZES.iter().enumerate() {
            add_item(m, 160 + i as u32, &format!("Taille {sz:.0} px"), (s.spotlight.size - sz).abs() < 1.0);
        }
        add_sep(m);
        for (i, d) in SPOT_DIMS.iter().enumerate() {
            add_item(m, 170 + i as u32, &format!("Assombrissement {:.0} %", d * 100.0), (s.spotlight.dim - d).abs() < 0.01);
        }
    });
    add_sub(menu, "Anneau", |m| {
        add_item(m, 16, "Pulsation", s.ring.pulse);
        add_item(m, 17, "Halo lumineux (neon)", s.ring.glow);
        add_item(m, 19, "Effet RGB (arc-en-ciel)", s.ring.rgb);
        add_sep(m);
        for (i, (name, hex)) in COLORS.iter().enumerate() {
            let on = Rgba::parse(hex).map_or(false, |c| (c.0, c.1, c.2) == (s.ring.color.0, s.ring.color.1, s.ring.color.2));
            add_item(m, 140 + i as u32, name, on);
        }
    });
    add_sub(menu, "Style de traînée", |m| {
        for (i, (v, name)) in TRAILS.iter().enumerate() {
            add_item(m, 110 + i as u32, name, s.trail.style == *v);
        }
    });
    add_sub(menu, "Style de clic", |m| {
        for (i, (v, name)) in CLICKS.iter().enumerate() {
            add_item(m, 120 + i as u32, name, s.click.style == *v);
        }
    });
    add_sep(menu);
    add_item(menu, 200, "Ouvrir settings.json", false);
    add_item(menu, 201, "Recharger les réglages", false);
    add_item(menu, 202, "Restaurer les curseurs Windows", false);
    add_item(menu, 203, "Effacer les annotations", false);
    add_sep(menu);
    add_item(menu, 299, "Quitter Lumo", false);

    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd); // sans ça, le menu ne se ferme pas au clic ailleurs
    let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, null());
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);
    if cmd > 0 {
        with_app(|a| a.on_command(cmd as u32));
    }
}

// ------------------------------------------------------------------ point d'entrée

pub fn run() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        if std::env::args().any(|a| a == "--restore-cursors") {
            restore_cursors();
            return;
        }

        // une seule instance
        let _mutex = CreateMutexW(null(), 0, wide("Local\\LumoCursorFx").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return;
        }

        // si on plante, on rend les curseurs d'origine avant de mourir
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore_cursors();
            prev(info);
        }));

        timeBeginPeriod(1);

        let hinst = GetModuleHandleW(null());
        let ctl_class = wide("LumoControl");
        let ov_class = wide("LumoOverlay");

        let mut wc: WNDCLASSEXW = zeroed();
        wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(ctl_proc);
        wc.hInstance = hinst;
        wc.lpszClassName = ctl_class.as_ptr();
        RegisterClassExW(&wc);

        let mut wo: WNDCLASSEXW = zeroed();
        wo.cbSize = size_of::<WNDCLASSEXW>() as u32;
        wo.lpfnWndProc = Some(DefWindowProcW);
        wo.hInstance = hinst;
        wo.lpszClassName = ov_class.as_ptr();
        RegisterClassExW(&wo);

        // fenêtre de contrôle : top-level invisible (reçoit WM_DISPLAYCHANGE, hotkeys, tray)
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            ctl_class.as_ptr(),
            wide("Lumo").as_ptr(),
            WS_POPUP,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        CTL_HWND.with(|c| c.set(hwnd as usize));

        // réglages
        let path = settings_path();
        let settings = match std::fs::read_to_string(&path).ok().and_then(|t| Settings::from_json(&t).ok()) {
            Some(s) => s,
            None => {
                let s = Settings::default();
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if !path.exists() {
                    let _ = std::fs::write(&path, s.to_json());
                }
                s
            }
        };
        let skin_dir = settings.cursor.skin_dir.clone();
        let fx = Fx::new(settings, load_skin(&skin_dir));


        let wav: &'static [u8] = Box::leak(lumo::fx::click_wav().into_boxed_slice());

        let mut app = App {
            hwnd,
            hinst,
            overlay_class: ov_class.clone(),
            overlays: Vec::new(),
            fx,
            mtime: mtime_of(&path),
            path,
            skin_dir_loaded: skin_dir,
            last_check: Instant::now(),
            last_tick: Instant::now(),
            last_pos: (i32::MIN, i32::MIN),
            dirty: true,
            was_animating: false,
            cursor_key: None,
            hotkey_ids: Vec::new(),
            hotkey_taken: Vec::new(),
            annotate_vks: Vec::new(),
            wav,
            sound_path: Vec::new(),
            hook: null_mut(),
        };
        app.rebuild_overlays();
        app.add_tray();
        app.after_settings_change();
        if !std::env::args().any(|a| a == "--minimized") {
            app.open_settings();
        }
        app.hook = SetWindowsHookExW(WH_MOUSE_LL, Some(ll_mouse), hinst, 0);
        let hook = app.hook;
        APP.with(|c| *c.borrow_mut() = Some(app));

        SetTimer(hwnd, TIMER_ID, TIMER_MS, None);

        let mut msg: MSG = zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            // Tab / Entrée / Échap dans la fenêtre de réglages
            let ui = crate::winui::hwnd();
            if !ui.is_null() && IsDialogMessageW(ui, &msg) != 0 {
                continue;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        // nettoyage
        KillTimer(hwnd, TIMER_ID);
        if !hook.is_null() {
            UnhookWindowsHookEx(hook);
        }
        restore_cursors();
        APP.with(|c| *c.borrow_mut() = None); // détruit les overlays
    }
}
