//! Fenêtre de réglages moderne : une fenêtre Win32 créée par lumo.exe, qui héberge une WebView2
//! (crate `wry`) affichant `ui/index.html`, embarquée dans l'exe. Aucun serveur, aucun réseau,
//! aucun navigateur externe.
//!
//! - Rust -> page : le schéma (`schema::to_json`) et les réglages sont injectés avant le chargement
//!   (`window.LUMO_INIT`), puis `lumoRefresh(json)`, `lumoStatus(kind, texte)`, `lumoFrame(png)`.
//! - Page -> Rust : `window.ipc.postMessage(json)`. Le gestionnaire ne fait que mettre le message
//!   en file et poster `WM_IPC` : le traitement a lieu dans la procédure de fenêtre, hors de tout
//!   emprunt de l'état, comme le reste de l'application (un seul thread).
//!
//! Si WebView2 est absent ou ne démarre pas, `open` renvoie `false` et `winui` ouvre l'ancienne
//! fenêtre native.

use crate::win::{make_icon, wide, with_app};
use lumo::demo::{base64, Demo};
use lumo::schema::{self, Kind};
use lumo::settings::{Rgba, Settings};
use lumo::skin::Skin;
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::mem::{size_of, zeroed};
use std::num::NonZeroIsize;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::Instant;
use tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow};
use windows_sys::Win32::UI::WindowsAndMessaging::*;
use wry::raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle};
use wry::{WebContext, WebView, WebViewBuilder, WebViewBuilderExtWindows};

const HTML: &str = include_str!("../ui/index.html");

/// Un message de la page attend dans `INBOX`.
const WM_IPC: u32 = WM_APP + 2;
const TIMER_PREVIEW: usize = 1;
/// Filet de sécurité : la fenêtre s'affiche même si la page ne signale jamais « ready ».
const TIMER_REVEAL: usize = 2;
const PREVIEW_MS: u32 = 33;

// taille en pixels logiques (96 dpi)
const W: i32 = 860;
const H: i32 = 620;
const MIN_W: i32 = 720;
const MIN_H: i32 = 520;

const BG_LIGHT: (u8, u8, u8) = (0xf7, 0xf6, 0xf9);
const BG_DARK: (u8, u8, u8) = (0x1c, 0x1b, 0x22);

struct Web {
    view: WebView,
    _ctx: WebContext,
    shown: bool,
    preview: (u32, u32),
    demo: Option<Demo>,
    demo_skin: String,
    last_frame: Instant,
}

thread_local! {
    static WEB: RefCell<Option<Web>> = const { RefCell::new(None) };
    static WEB_HWND: Cell<usize> = const { Cell::new(0) };
    static INBOX: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Page affichée, retrouvée à la réouverture de la fenêtre.
    static LAST_PAGE: RefCell<String> = const { RefCell::new(String::new()) };
}

fn with_web<R>(f: impl FnOnce(&mut Web) -> R) -> Option<R> {
    WEB.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

/// Poignée de la fenêtre ouverte (nulle si fermée).
pub fn hwnd() -> HWND {
    WEB_HWND.with(|c| c.get()) as HWND
}

/// Poignée Win32 à donner à `wry` (raw-window-handle).
struct Parent(HWND);

impl HasWindowHandle for Parent {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let h = NonZeroIsize::new(self.0 as isize).ok_or(HandleError::Unavailable)?;
        // SAFETY : la fenêtre existe pendant toute la construction de la WebView.
        Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(Win32WindowHandle::new(h))) })
    }
}

/// Dossier de données de WebView2 (cache, profil) : sans lui, il serait créé à côté de l'exe.
fn data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("APPDATA")).map(PathBuf::from).unwrap_or_else(|| ".".into());
    base.join("Lumo").join("WebView2")
}

/// Thème des applications Windows (Paramètres > Personnalisation > Couleurs).
fn is_dark() -> bool {
    let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
    let name = wide("AppsUseLightTheme");
    let mut v: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    let r = unsafe {
        RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), RRF_RT_REG_DWORD, null_mut(), &mut v as *mut u32 as *mut _, &mut size)
    };
    r == 0 && v == 0
}

unsafe fn apply_theme(hwnd: HWND) {
    let dark: BOOL = is_dark() as BOOL;
    DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &dark as *const BOOL as *const _, size_of::<BOOL>() as u32);
}

fn load_skin(dir: &str) -> Skin {
    if dir.trim().is_empty() {
        Skin::default()
    } else {
        Skin::load(Path::new(dir))
    }
}

// ------------------------------------------------------------------ création

/// Ramène la fenêtre au premier plan si elle est déjà ouverte.
pub unsafe fn focus_existing() -> bool {
    let h = hwnd();
    if h.is_null() || IsWindow(h) == 0 {
        return false;
    }
    if IsIconic(h) != 0 {
        ShowWindow(h, SW_RESTORE);
    }
    SetForegroundWindow(h);
    true
}

/// Ouvre la fenêtre. `false` si WebView2 n'a pas pu démarrer (rien n'est laissé ouvert).
pub unsafe fn open(hinst: HINSTANCE, settings: &Settings) -> bool {
    if focus_existing() {
        return true;
    }

    let class = wide("LumoSettingsWeb");
    let icon = make_icon(&lumo::fx::generated_cursor(32, Rgba(255, 102, 170, 255), true), false);
    let mut wc: WNDCLASSEXW = zeroed();
    wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(proc_);
    wc.hInstance = hinst;
    wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
    wc.hIcon = icon;
    wc.hIconSm = icon;
    wc.lpszClassName = class.as_ptr();
    RegisterClassExW(&wc); // échoue sans conséquence si la classe existe déjà

    let style = WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN;
    let hwnd = CreateWindowExW(
        0,
        class.as_ptr(),
        wide("Lumo — Réglages").as_ptr(),
        style,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        W,
        H,
        null_mut(),
        null_mut(),
        hinst,
        null(),
    );
    if hwnd.is_null() {
        return false;
    }
    SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, icon as LPARAM);
    SendMessageW(hwnd, WM_SETICON, ICON_SMALL as WPARAM, icon as LPARAM);
    apply_theme(hwnd);
    place(hwnd, style);

    let dark = is_dark();
    let (r, g, b) = if dark { BG_DARK } else { BG_LIGHT };
    let init = json!({
        "schema": schema::to_json(),
        "settings": settings,
        "page": LAST_PAGE.with(|p| p.borrow().clone()),
        "version": env!("CARGO_PKG_VERSION"),
    });
    let target = hwnd as usize;
    let mut ctx = WebContext::new(Some(data_dir()));
    let built = WebViewBuilder::new_with_web_context(&mut ctx)
        .with_html(HTML)
        .with_initialization_script(format!("window.LUMO_INIT = {init};"))
        .with_ipc_handler(move |req| {
            INBOX.with(|b| b.borrow_mut().push(req.into_body()));
            PostMessageW(target as HWND, WM_IPC, 0, 0);
        })
        .with_background_color((r, g, b, 255))
        .with_devtools(cfg!(debug_assertions))
        .with_default_context_menus(cfg!(debug_assertions))
        .with_browser_accelerator_keys(false)
        .with_hotkeys_zoom(false)
        .build(&Parent(hwnd));

    let view = match built {
        Ok(v) => v,
        Err(e) => {
            eprintln!("WebView2 indisponible ({e}) : fenêtre de réglages native");
            DestroyWindow(hwnd);
            return false;
        }
    };
    WEB.with(|c| {
        *c.borrow_mut() = Some(Web {
            view,
            _ctx: ctx,
            shown: false,
            preview: (0, 0),
            demo: None,
            demo_skin: String::new(),
            last_frame: Instant::now(),
        })
    });
    WEB_HWND.with(|c| c.set(hwnd as usize));
    SetTimer(hwnd, TIMER_REVEAL, 2500, None);
    true
}

/// Taille W x H logiques, centrée sur la zone de travail de l'écran où se trouve la souris.
unsafe fn place(hwnd: HWND, style: u32) {
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    let mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
    let mut mi: MONITORINFO = zeroed();
    mi.cbSize = size_of::<MONITORINFO>() as u32;
    GetMonitorInfoW(mon, &mut mi);
    let work = mi.rcWork;
    // la fenêtre adopte le dpi de l'écran où elle est : on la déplace d'abord sur le bon écran
    SetWindowPos(hwnd, null_mut(), work.left, work.top, 100, 100, SWP_NOZORDER | SWP_NOACTIVATE);
    let dpi = GetDpiForWindow(hwnd).max(96);
    let s = |x: i32| (x as i64 * dpi as i64 / 96) as i32;
    let mut rc = RECT { left: 0, top: 0, right: s(W), bottom: s(H) };
    AdjustWindowRectExForDpi(&mut rc, style, 0, 0, dpi);
    let (ww, wh) = ((rc.right - rc.left).min(work.right - work.left), (rc.bottom - rc.top).min(work.bottom - work.top));
    let x = work.left + ((work.right - work.left) - ww) / 2;
    let y = work.top + ((work.bottom - work.top) - wh) / 2;
    SetWindowPos(hwnd, null_mut(), x, y, ww, wh, SWP_NOZORDER | SWP_NOACTIVATE);
}

unsafe fn reveal(hwnd: HWND) {
    KillTimer(hwnd, TIMER_REVEAL);
    let first = with_web(|w| !std::mem::replace(&mut w.shown, true)).unwrap_or(false);
    if first {
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        with_web(|w| {
            let _ = w.view.focus();
        });
    }
}

unsafe fn close(hwnd: HWND) {
    KillTimer(hwnd, TIMER_PREVIEW);
    KillTimer(hwnd, TIMER_REVEAL);
    // la WebView est détruite avant sa fenêtre parente
    let web = WEB.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.take()));
    drop(web);
    DestroyWindow(hwnd);
}

// ------------------------------------------------------------------ Rust -> page

fn script(js: &str) {
    with_web(|w| {
        let _ = w.view.evaluate_script(js);
    });
}

fn status(ok: bool, text: &str) {
    script(&format!("lumoStatus({}, {})", json!(if ok { "ok" } else { "error" }), json!(text)));
}

/// Recopie les réglages de l'application dans la page (et dans l'aperçu).
fn push_settings() {
    let Some(s) = with_app(|a| a.ui_snapshot()) else { return };
    script(&format!("lumoRefresh({})", serde_json::to_string(&s).unwrap_or_else(|_| "null".into())));
    with_web(|w| {
        if let Some(d) = w.demo.as_mut() {
            let skin = (s.cursor.skin_dir != w.demo_skin).then(|| {
                w.demo_skin = s.cursor.skin_dir.clone();
                load_skin(&s.cursor.skin_dir)
            });
            d.set_settings(s, skin);
        }
    });
}

// ------------------------------------------------------------------ page -> Rust

unsafe fn on_ipc(hwnd: HWND) {
    let msgs = INBOX.with(|b| std::mem::take(&mut *b.borrow_mut()));
    for raw in msgs {
        let Ok(m) = serde_json::from_str::<Value>(&raw) else { continue };
        let s = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        match m.get("t").and_then(Value::as_str).unwrap_or("") {
            "ready" => reveal(hwnd),
            "set" => set_value(&s("path"), m.get("value").cloned().unwrap_or(Value::Null)),
            "action" => action(&s("name")),
            "browse" => browse(hwnd, &s("path")),
            "page" => {
                let id = s("id");
                with_web(|w| w.demo.as_mut().map(|d| d.set_page(&id)));
                LAST_PAGE.with(|p| *p.borrow_mut() = id);
            }
            "preview" => {
                let dim = |k: &str| m.get(k).and_then(Value::as_u64).unwrap_or(0).min(4096) as u32;
                set_preview(hwnd, &s("page"), dim("w"), dim("h"));
            }
            "close" => {
                close(hwnd);
                return;
            }
            _ => {}
        }
    }
}

/// Valide, applique et enregistre une valeur venue de la page.
fn set_value(path: &str, v: Value) {
    let v = match schema::validate(path, &v) {
        Ok(v) => v,
        Err(msg) => {
            status(false, msg);
            push_settings(); // remet le contrôle sur la valeur en vigueur
            return;
        }
    };
    let Some(current) = with_app(|a| a.ui_snapshot()) else {
        status(false, "Lumo est occupé, réessayez");
        return;
    };
    let mut j = serde_json::to_value(&current).unwrap_or(Value::Null);
    schema::set(&mut j, path, v);
    let Ok(s) = serde_json::from_value::<Settings>(j) else {
        status(false, "Valeur refusée");
        return;
    };
    if with_app(|a| unsafe { a.ui_apply(s) }).is_none() {
        status(false, "Lumo est occupé, réessayez");
        return;
    }
    let taken = with_app(|a| a.ui_hotkey_taken(path)).unwrap_or(false);
    if taken {
        status(false, "Raccourci déjà utilisé par Windows ou une autre application");
    } else {
        status(true, "Enregistré");
    }
}

fn action(name: &str) {
    let msg = match name {
        "clear_annotations" => "Annotations effacées",
        "restore_cursors" => "Curseurs Windows restaurés",
        "open_json" => "settings.json ouvert",
        _ => return,
    };
    match with_app(|a| unsafe { a.ui_action(name) }) {
        Some(()) => status(true, msg),
        None => status(false, "Lumo est occupé, réessayez"),
    }
}

unsafe fn browse(hwnd: HWND, path: &str) {
    let Some(field) = schema::field(path) else { return };
    let current = with_app(|a| serde_json::to_value(a.ui_snapshot()).ok())
        .flatten()
        .and_then(|v| schema::get(&v, path).and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default();
    let picked = match field.kind {
        Kind::File(ext) => pick(hwnd, false, ext, &current),
        Kind::Folder => pick(hwnd, true, "", &current),
        _ => None,
    };
    if let Some(p) = picked {
        set_value(path, Value::from(p));
    }
}

/// Boîte de dialogue Windows moderne (IFileOpenDialog), modale. `None` si annulée.
unsafe fn pick(owner: HWND, folder: bool, ext: &str, current: &str) -> Option<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
        FOS_PICKFOLDERS, SIGDN_FILESYSPATH,
    };

    let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
    let opts = dlg.GetOptions().ok()? | FOS_FORCEFILESYSTEM | if folder { FOS_PICKFOLDERS } else { FOS_FILEMUSTEXIST };
    dlg.SetOptions(opts).ok()?;
    let title = HSTRING::from(if folder { "Choisir un dossier" } else { "Choisir un fichier" });
    let _ = dlg.SetTitle(&title);

    let (name, spec, all_name, all_spec) =
        (HSTRING::from(format!("Fichiers .{ext}")), HSTRING::from(format!("*.{ext}")), HSTRING::from("Tous les fichiers"), HSTRING::from("*.*"));
    if !folder && !ext.is_empty() {
        let filters = [
            COMDLG_FILTERSPEC { pszName: PCWSTR(name.as_ptr()), pszSpec: PCWSTR(spec.as_ptr()) },
            COMDLG_FILTERSPEC { pszName: PCWSTR(all_name.as_ptr()), pszSpec: PCWSTR(all_spec.as_ptr()) },
        ];
        let _ = dlg.SetFileTypes(&filters);
    }

    // ouvre sur le dossier de la valeur actuelle, s'il existe
    let cur = Path::new(current.trim());
    let start = if folder { Some(cur) } else { cur.parent() };
    if let Some(dir) = start.filter(|d| !d.as_os_str().is_empty() && d.is_dir()) {
        if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(dir.as_os_str()), None) {
            let _ = dlg.SetFolder(&item);
        }
    }

    dlg.Show(Some(windows::Win32::Foundation::HWND(owner as _))).ok()?;
    let item = dlg.GetResult().ok()?;
    let p = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
    let out = p.to_string().ok();
    CoTaskMemFree(Some(p.0 as *const _));
    out
}

// ------------------------------------------------------------------ aperçu en direct

unsafe fn set_preview(hwnd: HWND, page: &str, w: u32, h: u32) {
    let on = w > 0 && h > 0;
    let snapshot = if on { with_app(|a| a.ui_snapshot()) } else { None };
    with_web(|web| {
        web.preview = (w, h);
        if on && web.demo.is_none() {
            if let Some(s) = snapshot {
                web.demo_skin = s.cursor.skin_dir.clone();
                web.demo = Some(Demo::new(s.clone(), load_skin(&s.cursor.skin_dir)));
            }
        }
        if let Some(d) = web.demo.as_mut() {
            d.set_page(page);
        }
        web.last_frame = Instant::now();
    });
    if on {
        SetTimer(hwnd, TIMER_PREVIEW, PREVIEW_MS, None);
    } else {
        KillTimer(hwnd, TIMER_PREVIEW);
    }
}

unsafe fn preview_tick(hwnd: HWND) {
    if IsIconic(hwnd) != 0 || IsWindowVisible(hwnd) == 0 {
        return;
    }
    let frame = with_web(|w| {
        let now = Instant::now();
        let dt = now.duration_since(w.last_frame).as_secs_f32();
        w.last_frame = now;
        let (pw, ph) = w.preview;
        let d = w.demo.as_mut()?;
        d.step(dt, pw as f32, ph as f32);
        let mut pm = Pixmap::new(pw, ph)?;
        d.render(&mut pm);
        pm.encode_png().ok()
    })
    .flatten();
    if let Some(png) = frame {
        script(&format!("lumoFrame('{}')", base64(&png)));
    }
}

// ------------------------------------------------------------------ procédure de fenêtre

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_IPC => {
            on_ipc(hwnd);
            0
        }
        crate::winui::WM_REFRESH => {
            push_settings();
            0
        }
        WM_TIMER => {
            match wp {
                TIMER_PREVIEW => preview_tick(hwnd),
                TIMER_REVEAL => reveal(hwnd),
                _ => {}
            }
            0
        }
        WM_SETFOCUS => {
            with_web(|w| {
                let _ = w.view.focus();
            });
            0
        }
        WM_GETMINMAXINFO => {
            let dpi = GetDpiForWindow(hwnd).max(96) as i32;
            let mmi = &mut *(lp as *mut MINMAXINFO);
            mmi.ptMinTrackSize = POINT { x: MIN_W * dpi / 96, y: MIN_H * dpi / 96 };
            0
        }
        WM_DPICHANGED => {
            let r = &*(lp as *const RECT);
            SetWindowPos(hwnd, null_mut(), r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
            0
        }
        WM_SETTINGCHANGE => {
            apply_theme(hwnd);
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_ERASEBKGND => {
            let (r, g, b) = if is_dark() { BG_DARK } else { BG_LIGHT };
            let mut rc: RECT = zeroed();
            GetClientRect(hwnd, &mut rc);
            let brush = CreateSolidBrush(r as u32 | (g as u32) << 8 | (b as u32) << 16);
            FillRect(wp as HDC, &rc, brush);
            DeleteObject(brush as HGDIOBJ);
            1
        }
        WM_CLOSE => {
            close(hwnd);
            0
        }
        WM_DESTROY => {
            WEB_HWND.with(|c| c.set(0));
            let web = WEB.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.take()));
            drop(web);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
