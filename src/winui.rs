//! Point d'entrée de la fenêtre de réglages (`open`) : l'interface WebView2 (`webui.rs`) si
//! possible, sinon cette fenêtre native de repli (Win32 + comctl32 v6) : onglets, curseurs, cases
//! à cocher, listes déroulantes, sélecteur de couleur. Les champs viennent de `lumo::schema` ;
//! chaque modification est appliquée immédiatement et enregistrée.
//!
//! `lumo.exe --native-ui` force la fenêtre native (utile pour tester le repli).
//!
//! Tout vit sur le thread de la boucle de messages. L'état de la fenêtre est dans un
//! `thread_local` accédé uniquement via `try_borrow_mut` : si un message arrive pendant qu'on
//! tient déjà l'état (messages envoyés par Windows pendant la création des contrôles, boîte de
//! couleur modale…), il est ignoré au lieu de paniquer.

use crate::win::{wide, with_app};
use lumo::schema::{self, Field, Kind, MAX_ROWS, PAGES};
use lumo::settings::Settings;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::UI::Controls::Dialogs::{ChooseColorW, CC_FULLOPEN, CC_RGBINIT, CHOOSECOLORW};
use windows_sys::Win32::UI::Controls::{
    InitCommonControlsEx, DRAWITEMSTRUCT, ICC_BAR_CLASSES, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX, NMHDR, TBM_SETPOS,
    TBM_SETRANGE, TBS_NOTICKS, TCIF_TEXT, TCITEMW, TCM_ADJUSTRECT, TCM_GETCURSEL, TCM_INSERTITEMW, TCN_SELCHANGE,
    TCS_MULTILINE, BST_CHECKED,
};
use windows_sys::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForWindow, SystemParametersInfoForDpi};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetFocus, SetFocus};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Message posté par l'application quand les réglages ont changé ailleurs (menu, raccourcis…).
pub const WM_REFRESH: u32 = WM_APP + 1;

const TBM_GETPOS_: u32 = 0x0400; // WM_USER
const SS_LEFT: u32 = 0; // style STATIC par défaut (texte aligné à gauche)
const ID_TAB: usize = 800;
const ID_CLEAR: usize = 900;
const ID_RESTORE: usize = 901;
const ID_JSON: usize = 902;
const ID_BASE: usize = 1000;
const IDOK_: usize = 1;
const IDCANCEL_: usize = 2;

// disposition en pixels « logiques » (96 dpi), mis à l'échelle avec le dpi de la fenêtre
const W: i32 = 640;
const H: i32 = 500;
const ROW_H: i32 = 36;

struct Ctl {
    field: &'static Field,
    page: usize,
    id: usize,
    label: HWND,
    ctl: HWND,
    value: HWND,
}

struct Ui {
    tab: HWND,
    font: HFONT,
    status: HWND,
    ctls: Vec<Ctl>,
    notes: Vec<HWND>,
    cur: usize,
    value: Value,
}

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
    static UI_HWND: Cell<usize> = const { Cell::new(0) };
}

fn with_ui<R>(f: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|c| c.try_borrow_mut().ok().and_then(|mut b| b.as_mut().map(f)))
}

/// Poignée de la fenêtre native ouverte (nulle si fermée) : sert à `IsDialogMessageW`.
pub fn hwnd() -> HWND {
    UI_HWND.with(|c| c.get()) as HWND
}

/// Prévient la fenêtre de réglages ouverte (web ou native) que les réglages ont changé ailleurs.
pub fn notify_refresh() {
    for h in [hwnd(), crate::webui::hwnd()] {
        if !h.is_null() {
            unsafe {
                PostMessageW(h, WM_REFRESH, 0, 0);
            }
        }
    }
}

/// Ouvre la fenêtre de réglages, ou ramène au premier plan celle qui est déjà ouverte.
pub unsafe fn open(hinst: HINSTANCE, settings: &Settings) {
    if crate::webui::focus_existing() {
        return;
    }
    let native = hwnd();
    if !native.is_null() && IsWindow(native) != 0 {
        ShowWindow(native, SW_RESTORE);
        SetForegroundWindow(native);
        return;
    }
    let force_native = std::env::args().any(|a| a == "--native-ui");
    if force_native || !crate::webui::open(hinst, settings) {
        open_native(hinst, settings);
    }
}

unsafe fn set_text(h: HWND, text: &str) {
    if !h.is_null() {
        SetWindowTextW(h, wide(text).as_ptr());
    }
}

unsafe fn read_text(h: HWND) -> String {
    let len = GetWindowTextLengthW(h);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(h, buf.as_mut_ptr(), buf.len() as i32);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

// ------------------------------------------------------------------ création

unsafe fn open_native(hinst: HINSTANCE, settings: &Settings) {
    let icc = INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_TAB_CLASSES | ICC_BAR_CLASSES };
    InitCommonControlsEx(&icc);

    let class = wide("LumoSettings");
    let mut wc: WNDCLASSEXW = zeroed();
    wc.cbSize = size_of::<WNDCLASSEXW>() as u32;
    wc.lpfnWndProc = Some(ui_proc);
    wc.hInstance = hinst;
    wc.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
    wc.hbrBackground = (COLOR_WINDOW + 1) as usize as HBRUSH;
    wc.lpszClassName = class.as_ptr();
    RegisterClassExW(&wc); // échoue sans conséquence si la classe existe déjà

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
    let hwnd = CreateWindowExW(
        WS_EX_CONTROLPARENT,
        class.as_ptr(),
        wide("Lumo — Réglages").as_ptr(),
        style,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        100,
        100,
        null_mut(),
        null_mut(),
        hinst,
        null(),
    );
    if hwnd.is_null() {
        return;
    }

    let dpi = GetDpiForWindow(hwnd).max(96);
    let s = move |x: i32| (x as i64 * dpi as i64 / 96) as i32;

    // taille de la fenêtre pour une zone client de W x H (logiques), centrée sur l'écran
    let mut rc = RECT { left: 0, top: 0, right: s(W), bottom: s(H) };
    AdjustWindowRectExForDpi(&mut rc, style, 0, WS_EX_CONTROLPARENT, dpi);
    let (ww, wh) = (rc.right - rc.left, rc.bottom - rc.top);
    let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
    SetWindowPos(hwnd, null_mut(), ((sw - ww) / 2).max(0), ((sh - wh) / 2).max(0), ww, wh, SWP_NOZORDER);

    // police système (Segoe UI 9 pt) à la bonne échelle
    let mut ncm: NONCLIENTMETRICSW = zeroed();
    ncm.cbSize = size_of::<NONCLIENTMETRICSW>() as u32;
    SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS, ncm.cbSize, &mut ncm as *mut _ as *mut c_void, 0, dpi);
    let font = CreateFontIndirectW(&ncm.lfMessageFont);

    let mk = |ex: u32, cls: &str, text: &str, st: u32, x: i32, y: i32, w: i32, h: i32, id: usize| -> HWND {
        let c = CreateWindowExW(ex, wide(cls).as_ptr(), wide(text).as_ptr(), WS_CHILD | st, x, y, w, h, hwnd, id as HMENU, hinst, null());
        if !c.is_null() {
            SendMessageW(c, WM_SETFONT, font as WPARAM, 1);
        }
        c
    };

    // onglets
    let tab = mk(0, "SysTabControl32", "", WS_VISIBLE | WS_TABSTOP | WS_CLIPSIBLINGS | TCS_MULTILINE, s(8), s(8), s(W - 16), s(404), ID_TAB);
    for (i, p) in PAGES.iter().enumerate() {
        let mut title = wide(p.title);
        let mut item: TCITEMW = zeroed();
        item.mask = TCIF_TEXT;
        item.pszText = title.as_mut_ptr();
        SendMessageW(tab, TCM_INSERTITEMW, i as WPARAM, &item as *const _ as LPARAM);
    }
    let mut disp = RECT { left: 0, top: 0, right: s(W - 16), bottom: s(404) };
    SendMessageW(tab, TCM_ADJUSTRECT, 0, &mut disp as *mut RECT as LPARAM);
    let top = s(8) + disp.top + s(14);

    // champs
    let mut ctls = Vec::new();
    let mut notes = Vec::new();
    let mut next_id = ID_BASE;
    for (pi, page) in PAGES.iter().enumerate() {
        for (row, field) in page.fields.iter().enumerate() {
            let y = top + row as i32 * s(ROW_H);
            let id = next_id;
            next_id += 1;
            let (mut label, mut value) = (null_mut(), null_mut());
            let ctl: HWND;
            let lbl = |hw: &mut HWND| *hw = mk(0, "STATIC", field.label, SS_LEFT, s(28), y + s(4), s(270), s(22), 0);
            match &field.kind {
                Kind::Bool => {
                    ctl = mk(0, "BUTTON", field.label, WS_TABSTOP | BS_AUTOCHECKBOX as u32, s(28), y, s(580), s(24), id);
                }
                Kind::Select(opts) => {
                    lbl(&mut label);
                    ctl = mk(0, "COMBOBOX", "", WS_TABSTOP | WS_VSCROLL | CBS_DROPDOWNLIST as u32, s(310), y, s(290), s(220), id);
                    for (_, name) in opts.iter() {
                        SendMessageW(ctl, CB_ADDSTRING, 0, wide(name).as_ptr() as LPARAM);
                    }
                }
                Kind::Num { min, max, step, .. } => {
                    lbl(&mut label);
                    ctl = mk(0, "msctls_trackbar32", "", WS_TABSTOP | TBS_NOTICKS, s(306), y, s(220), s(26), id);
                    let n = schema::ticks(*min, *max, *step) as u32;
                    SendMessageW(ctl, TBM_SETRANGE, 1, (n << 16) as LPARAM);
                    value = mk(0, "STATIC", "", SS_LEFT, s(534), y + s(4), s(90), s(22), 0);
                }
                Kind::Color => {
                    lbl(&mut label);
                    ctl = mk(0, "BUTTON", "", WS_TABSTOP | BS_OWNERDRAW as u32, s(310), y, s(64), s(24), id);
                }
                Kind::Text | Kind::Hotkey | Kind::Hold | Kind::File(_) | Kind::Folder => {
                    lbl(&mut label);
                    ctl = mk(WS_EX_CLIENTEDGE, "EDIT", "", WS_TABSTOP | ES_AUTOHSCROLL as u32, s(310), y, s(300), s(24), id);
                }
            }
            ctls.push(Ctl { field, page: pi, id, label, ctl, value });
        }
        let note_y = top + MAX_ROWS as i32 * s(ROW_H);
        notes.push(mk(0, "STATIC", page.note, SS_LEFT, s(28), note_y, s(W - 56), s(62), 0));
    }

    // boutons du bas + statut
    let by = s(8 + 404 + 12);
    mk(0, "BUTTON", "Effacer les annotations", WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32, s(16), by, s(190), s(30), ID_CLEAR);
    mk(0, "BUTTON", "Restaurer les curseurs Windows", WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32, s(214), by, s(215), s(30), ID_RESTORE);
    mk(0, "BUTTON", "Ouvrir settings.json", WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32, s(437), by, s(187), s(30), ID_JSON);
    let status = mk(0, "STATIC", "", WS_VISIBLE | SS_LEFT, s(20), s(H - 38), s(W - 40), s(22), 0);

    let mut ui = Ui { tab, font, status, ctls, notes, cur: 0, value: serde_json::to_value(settings).unwrap_or(Value::Null) };
    refresh(&mut ui);
    show_page(&mut ui);
    UI.with(|c| *c.borrow_mut() = Some(ui));
    UI_HWND.with(|c| c.set(hwnd as usize));

    ShowWindow(hwnd, SW_SHOW);
    SetForegroundWindow(hwnd);
}

// ------------------------------------------------------------------ valeurs <-> contrôles

unsafe fn show_page(ui: &mut Ui) {
    for c in &ui.ctls {
        let cmd = if c.page == ui.cur { SW_SHOW } else { SW_HIDE };
        for h in [c.label, c.ctl, c.value] {
            if !h.is_null() {
                ShowWindow(h, cmd);
            }
        }
    }
    for (i, n) in ui.notes.iter().enumerate() {
        ShowWindow(*n, if i == ui.cur { SW_SHOW } else { SW_HIDE });
    }
}

/// Recopie `ui.value` dans les contrôles (sans déclencher de notification).
unsafe fn refresh(ui: &mut Ui) {
    for c in &ui.ctls {
        let Some(v) = schema::get(&ui.value, c.field.path) else { continue };
        match &c.field.kind {
            Kind::Bool => {
                let on = v.as_bool().unwrap_or(false);
                SendMessageW(c.ctl, BM_SETCHECK, if on { BST_CHECKED as WPARAM } else { 0 }, 0);
            }
            Kind::Select(opts) => {
                let cur = v.as_str().unwrap_or("");
                if let Some(i) = opts.iter().position(|o| o.0 == cur) {
                    SendMessageW(c.ctl, CB_SETCURSEL, i as WPARAM, 0);
                }
            }
            Kind::Num { min, max, step, unit, pct } => {
                let x = v.as_f64().unwrap_or(*min);
                let pos = schema::pos_of(x, *min, *max, *step);
                if SendMessageW(c.ctl, TBM_GETPOS_, 0, 0) as i32 != pos {
                    SendMessageW(c.ctl, TBM_SETPOS, 1, pos as LPARAM);
                }
                set_text(c.value, &schema::format_num(x, *step, unit, *pct));
            }
            Kind::Color => {
                InvalidateRect(c.ctl, null(), 1);
            }
            Kind::Text | Kind::Hotkey | Kind::Hold | Kind::File(_) | Kind::Folder => {
                if GetFocus() != c.ctl {
                    set_text(c.ctl, v.as_str().unwrap_or(""));
                }
            }
        }
    }
}

/// Écrit une valeur dans le JSON local, reconstruit les réglages et les envoie à l'application.
unsafe fn apply(path: &str, v: Value) {
    let v = match schema::validate(path, &v) {
        Ok(v) => v,
        Err(msg) => {
            with_ui(|u| set_text(u.status, msg));
            on_refresh();
            return;
        }
    };
    let parsed = with_ui(|u| {
        schema::set(&mut u.value, path, v);
        serde_json::from_value::<Settings>(u.value.clone())
    });
    let msg = match parsed {
        Some(Ok(s)) => match with_app(|a| a.ui_apply(s)) {
            Some(()) => "Enregistré ✓",
            None => "Lumo est occupé, réessayez",
        },
        Some(Err(_)) => "Valeur refusée",
        None => return,
    };
    with_ui(|u| set_text(u.status, msg));
}

unsafe fn pick_color(owner: HWND, initial: u32) -> Option<u32> {
    let mut custom = [0x00ff_ffffu32; 16];
    let mut cc: CHOOSECOLORW = zeroed();
    cc.lStructSize = size_of::<CHOOSECOLORW>() as u32;
    cc.hwndOwner = owner;
    cc.rgbResult = initial;
    cc.lpCustColors = custom.as_mut_ptr();
    cc.Flags = CC_RGBINIT | CC_FULLOPEN;
    if ChooseColorW(&mut cc) != 0 {
        Some(cc.rgbResult)
    } else {
        None
    }
}

// ------------------------------------------------------------------ messages

unsafe fn find_by_id(id: usize) -> Option<(&'static Field, HWND)> {
    with_ui(|u| u.ctls.iter().find(|c| c.id == id).map(|c| (c.field, c.ctl))).flatten()
}

unsafe fn on_command(hwnd: HWND, wp: WPARAM, lp: LPARAM) {
    let id = wp & 0xFFFF;
    let code = ((wp >> 16) & 0xFFFF) as u32;
    match id {
        IDOK_ => {
            // Entrée : retire le focus du champ texte en cours, ce qui valide sa saisie
            with_ui(|u| SetFocus(u.tab));
            return;
        }
        IDCANCEL_ => {
            DestroyWindow(hwnd);
            return;
        }
        ID_CLEAR | ID_RESTORE | ID_JSON => {
            if code == BN_CLICKED {
                let name = match id {
                    ID_CLEAR => "clear_annotations",
                    ID_RESTORE => "restore_cursors",
                    _ => "open_json",
                };
                with_app(|a| a.ui_action(name));
            }
            return;
        }
        _ => {}
    }
    let Some((field, ch)) = find_by_id(id) else { return };
    let _ = lp;
    let new = match &field.kind {
        Kind::Bool if code == BN_CLICKED => Some(Value::Bool(SendMessageW(ch, BM_GETCHECK, 0, 0) == BST_CHECKED as isize)),
        Kind::Select(opts) if code == CBN_SELCHANGE => {
            let i = SendMessageW(ch, CB_GETCURSEL, 0, 0);
            if i >= 0 { opts.get(i as usize).map(|o| Value::from(o.0)) } else { None }
        }
        Kind::Color if code == BN_CLICKED => {
            let old = with_ui(|u| schema::get(&u.value, field.path).and_then(|v| v.as_str()).unwrap_or("#ffffff").to_string()).unwrap_or_default();
            // la boîte de couleur est modale : aucun emprunt de l'état ne doit être en cours
            pick_color(hwnd, schema::hex_to_colorref(&old)).map(|c| Value::from(schema::colorref_to_hex(c, &old)))
        }
        Kind::Text | Kind::Hotkey | Kind::Hold | Kind::File(_) | Kind::Folder if code == EN_KILLFOCUS => {
            Some(Value::from(read_text(ch).trim().to_string()))
        }
        _ => None,
    };
    if let Some(v) = new {
        apply(field.path, v);
    }
}

unsafe fn on_hscroll(trackbar: HWND) {
    let found = with_ui(|u| u.ctls.iter().find(|c| c.ctl == trackbar).map(|c| (c.field, c.value))).flatten();
    let Some((field, label)) = found else { return };
    if let Kind::Num { min, step, unit, pct, .. } = &field.kind {
        let pos = SendMessageW(trackbar, TBM_GETPOS_, 0, 0) as i32;
        let x = schema::value_of(pos, *min, *step);
        set_text(label, &schema::format_num(x, *step, unit, *pct));
        apply(field.path, Value::from(x));
    }
}

unsafe fn on_notify(lp: LPARAM) {
    let nm = &*(lp as *const NMHDR);
    if nm.code == TCN_SELCHANGE {
        with_ui(|u| {
            if nm.hwndFrom == u.tab {
                u.cur = SendMessageW(u.tab, TCM_GETCURSEL, 0, 0).max(0) as usize;
                show_page(u);
            }
        });
    }
}

/// Pastille de couleur (bouton « owner-draw »).
unsafe fn on_drawitem(lp: LPARAM) -> LRESULT {
    let dis = &*(lp as *const DRAWITEMSTRUCT);
    let id = dis.CtlID as usize;
    let color = with_ui(|u| {
        u.ctls
            .iter()
            .find(|c| c.id == id)
            .and_then(|c| schema::get(&u.value, c.field.path))
            .and_then(|v| v.as_str())
            .map(schema::hex_to_colorref)
    })
    .flatten();
    if let Some(c) = color {
        let fill = CreateSolidBrush(c);
        FillRect(dis.hDC, &dis.rcItem, fill);
        DeleteObject(fill as HGDIOBJ);
        let edge = CreateSolidBrush(0x0080_8080);
        FrameRect(dis.hDC, &dis.rcItem, edge);
        DeleteObject(edge as HGDIOBJ);
    }
    1
}

unsafe fn on_refresh() {
    let Some(s) = with_app(|a| a.ui_snapshot()) else { return };
    with_ui(|u| {
        u.value = serde_json::to_value(&s).unwrap_or(Value::Null);
        refresh(u);
    });
}

unsafe fn on_destroy() {
    UI_HWND.with(|c| c.set(0));
    UI.with(|c| {
        if let Ok(mut b) = c.try_borrow_mut() {
            if let Some(u) = b.take() {
                DeleteObject(u.font as HGDIOBJ);
            }
        }
    });
}

unsafe extern "system" fn ui_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_COMMAND => {
            on_command(hwnd, wp, lp);
            0
        }
        WM_HSCROLL => {
            on_hscroll(lp as HWND);
            0
        }
        WM_NOTIFY => {
            on_notify(lp);
            0
        }
        WM_DRAWITEM => on_drawitem(lp),
        // fond blanc des textes et cases à cocher, pour coller au corps de l'onglet (thème Windows)
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            SetBkColor(wp as HDC, GetSysColor(COLOR_WINDOW));
            GetSysColorBrush(COLOR_WINDOW) as LRESULT
        }
        WM_REFRESH => {
            on_refresh();
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            on_destroy();
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
