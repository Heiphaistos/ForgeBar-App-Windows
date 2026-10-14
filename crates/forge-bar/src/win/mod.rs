//! Application Windows : une fenêtre de messages invisible porte les minuteurs,
//! une fenêtre de barre par écran configuré.

mod bar;
mod sys;

use bar::{Bar, Ctx, WM_APPBAR};
use forge_claude::Summary;
use forge_config::{config_path, user_themes_dir, ClaudeConfig, Config, Mode, ThemeStore};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use sys::Sampler;
use windows::core::{w, Result, HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, BOOL, ERROR_ALREADY_EXISTS, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
use windows::Win32::Graphics::Direct2D::{D2D1CreateFactory, ID2D1Factory, D2D1_FACTORY_TYPE_SINGLE_THREADED};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, DWRITE_FACTORY_TYPE_SHARED,
};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, InvalidateRect, ScreenToClient, ValidateRect, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_APP_CLAUDE: u32 = WM_APP + 2;
const WM_APP_REBUILD: u32 = WM_APP + 3;
const TIMER_TICK: usize = 1;
const TIMER_CONFIG: usize = 2;
const TIMER_AUTOHIDE: usize = 3;
const MENU_SETTINGS: usize = 10;
const MENU_THEMES: usize = 11;
const MENU_RELOAD: usize = 12;
const MENU_QUIT: usize = 13;

struct App {
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    themes: ThemeStore,
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    icon_font: String,
    main: HWND,
    bars: Vec<Bar>,
    sampler: Sampler,
    claude: Arc<Mutex<Summary>>,
    claude_cfg: Arc<Mutex<ClaudeConfig>>,
    claude_wake: Arc<AtomicBool>,
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Exécute `f` sur l'application si elle n'est pas déjà empruntée.
/// Les messages reçus pendant un appel Windows réentrant tombent dans le traitement par défaut.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

pub fn fatal(msg: &str) {
    unsafe { MessageBoxW(None, &HSTRING::from(msg), w!("Forge Bar"), MB_ICONERROR) };
}

/// Message non bloquant, affiché depuis un autre fil pour ne pas suspendre la barre.
fn warn(msg: String) {
    std::thread::spawn(move || unsafe {
        MessageBoxW(None, &HSTRING::from(msg), w!("Forge Bar"), MB_ICONWARNING);
    });
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn exe_path() -> String {
    let mut buf = [0u16; 1024];
    let n = unsafe { GetModuleFileNameW(None, &mut buf) } as usize;
    String::from_utf16_lossy(&buf[..n])
}

fn set_autostart(on: bool) {
    let key = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    unsafe {
        if on {
            let value: Vec<u16> = format!("\"{}\"", exe_path()).encode_utf16().chain([0]).collect();
            let _ = RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key,
                w!("ForgeBar"),
                REG_SZ.0,
                Some(value.as_ptr() as _),
                (value.len() * 2) as u32,
            );
        } else {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, key, w!("ForgeBar"));
        }
    }
}

/// Segoe Fluent Icons sous Windows 11, Segoe MDL2 Assets sous Windows 10.
fn icon_font(dwrite: &IDWriteFactory) -> String {
    unsafe {
        let mut coll: Option<IDWriteFontCollection> = None;
        if dwrite.GetSystemFontCollection(&mut coll, false).is_ok() {
            if let Some(coll) = coll {
                let (mut idx, mut exists) = (0u32, BOOL(0));
                if coll.FindFamilyName(w!("Segoe Fluent Icons"), &mut idx, &mut exists).is_ok() && exists.as_bool() {
                    return "Segoe Fluent Icons".into();
                }
            }
        }
    }
    "Segoe MDL2 Assets".into()
}

struct MonitorInfo {
    handle: HMONITOR,
    rect: RECT,
    primary: bool,
}

unsafe extern "system" fn collect_monitor(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<MonitorInfo>);
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(h, &mut mi).as_bool() {
        list.push(MonitorInfo { handle: h, rect: mi.rcMonitor, primary: mi.dwFlags & 1 != 0 });
    }
    BOOL(1)
}

fn monitors() -> Vec<MonitorInfo> {
    let mut list: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(&mut list as *mut _ as isize));
    }
    // Écran principal d'abord, puis de gauche à droite : c'est l'ordre des numéros d'écran.
    list.sort_by_key(|m| (!m.primary, m.rect.left, m.rect.top));
    list
}

impl App {
    fn rebuild(&mut self) {
        for mut b in self.bars.drain(..) {
            b.destroy();
        }
        let hinstance = unsafe { GetModuleHandleW(None).unwrap_or_default() };
        for (i, mon) in monitors().iter().enumerate() {
            let number = (i + 1).to_string();
            let Some(screen) = self
                .cfg
                .screen
                .iter()
                .find(|s| s.id == "all" || s.id == number || (s.id == "primary" && mon.primary))
                .filter(|s| s.enabled)
                .cloned()
            else {
                continue;
            };
            let (mut dx, mut dy) = (96u32, 96u32);
            unsafe {
                let _ = GetDpiForMonitor(mon.handle, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
            }
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                    w!("ForgeBarWindow"),
                    w!("Forge Bar"),
                    WS_POPUP,
                    mon.rect.left,
                    mon.rect.top,
                    10,
                    10,
                    None,
                    None,
                    hinstance,
                    None,
                )
            };
            let Ok(hwnd) = hwnd else { continue };
            let theme = self.themes.get(&screen.theme);
            let mut bar = Bar::new(hwnd, screen, theme, mon.rect, dx);
            bar.position();
            self.bars.push(bar);
        }
        let autohide = self.bars.iter().any(|b| b.screen.mode == Mode::Overlay && b.screen.autohide);
        unsafe {
            if autohide {
                SetTimer(self.main, TIMER_AUTOHIDE, 250, None);
            } else {
                let _ = KillTimer(self.main, TIMER_AUTOHIDE);
            }
        }
        self.invalidate();
    }

    fn reload(&mut self, force: bool) {
        let path = config_path();
        let m = mtime(&path);
        if !force && m == self.cfg_mtime {
            return;
        }
        self.cfg_mtime = m;
        match Config::load_or_create(&path) {
            Ok(cfg) => {
                self.cfg = cfg;
                self.themes = ThemeStore::load(Some(&user_themes_dir()));
                *self.claude_cfg.lock().unwrap() = self.cfg.claude.clone();
                self.claude_wake.store(true, Ordering::Relaxed);
                set_autostart(self.cfg.general.autostart);
                self.rebuild();
            }
            Err(e) => warn(format!("{}\n\nLa configuration précédente reste active.\n{}", e, path.display())),
        }
    }

    fn invalidate(&self) {
        for b in &self.bars {
            unsafe {
                let _ = InvalidateRect(b.hwnd, None, false);
            }
        }
    }

    fn autohide(&mut self) {
        let mut p = POINT::default();
        if unsafe { GetCursorPos(&mut p) }.is_err() {
            return;
        }
        for b in self.bars.iter_mut().filter(|b| b.screen.mode == Mode::Overlay && b.screen.autohide) {
            let r = b.rect;
            let inside = p.x >= r.left - 2 && p.x < r.right + 2 && p.y >= r.top - 2 && p.y < r.bottom + 2;
            if inside == b.hidden {
                b.hidden = !inside;
                b.position();
            }
        }
    }
}

fn bar_index(app: &App, hwnd: HWND) -> Option<usize> {
    app.bars.iter().position(|b| b.hwnd == hwnd)
}

fn lparam_point(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

fn click(hwnd: HWND, x: i32, y: i32) {
    let id = with_app(|app| bar_index(app, hwnd).and_then(|i| app.bars[i].hit(x, y))).flatten();
    match id.as_deref() {
        Some("desktop") => sys::toggle_desktop(),
        Some("wifi") => sys::open(w!("ms-settings:network-wifi")),
        Some("battery") => sys::open(w!("ms-settings:batterysaver")),
        Some("cpu") | Some("ram") => sys::open(w!("taskmgr.exe")),
        Some("clock") => sys::open(w!("ms-settings:dateandtime")),
        Some("sound") => {
            with_app(|app| app.sampler.toggle_mute());
        }
        Some("claude") => {
            with_app(|app| {
                if let Some(i) = bar_index(app, hwnd) {
                    app.bars[i].claude_expanded = !app.bars[i].claude_expanded;
                }
            });
        }
        _ => return,
    }
    with_app(|app| app.invalidate());
}

fn open_settings() {
    let exe = exe_path();
    let settings = std::path::Path::new(&exe).with_file_name("forge-settings.exe");
    let target = if settings.exists() { settings } else { config_path() };
    sys::open(PCWSTR(HSTRING::from(target.as_os_str()).as_ptr()));
}

fn context_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, MENU_SETTINGS, w!("Paramètres"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_THEMES, w!("Dossier des thèmes"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_RELOAD, w!("Recharger"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quitter Forge Bar"));
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = SetForegroundWindow(hwnd);
        let cmd =
            TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, None).0 as usize;
        let _ = DestroyMenu(menu);
        match cmd {
            MENU_SETTINGS => open_settings(),
            MENU_THEMES => {
                let dir = user_themes_dir();
                let _ = std::fs::create_dir_all(&dir);
                sys::open(PCWSTR(HSTRING::from(dir.as_os_str()).as_ptr()));
            }
            MENU_RELOAD => {
                with_app(|app| app.reload(true));
            }
            MENU_QUIT => {
                with_app(|app| {
                    for mut b in app.bars.drain(..) {
                        b.destroy();
                    }
                });
                PostQuitMessage(0);
            }
            _ => {}
        }
    }
}

unsafe extern "system" fn bar_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let painted = with_app(|app| {
                let App { bars, d2d, dwrite, icon_font, sampler, cfg, claude, .. } = app;
                let summary = claude.lock().unwrap().clone();
                let ctx = Ctx { d2d, dwrite, icon_font, sys: &sampler.data, claude: &summary, claude_cfg: &cfg.claude };
                if let Some(b) = bars.iter_mut().find(|b| b.hwnd == hwnd) {
                    let _ = b.paint(&ctx);
                }
            });
            let _ = ValidateRect(hwnd, None);
            if painted.is_none() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONUP => {
            let (x, y) = lparam_point(lp);
            click(hwnd, x, y);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            context_menu(hwnd);
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let delta = ((wp.0 >> 16) & 0xFFFF) as i16 as f32 / 120.0;
            let (sx, sy) = lparam_point(lp);
            let mut p = POINT { x: sx, y: sy };
            let _ = ScreenToClient(hwnd, &mut p);
            with_app(|app| {
                let over_sound =
                    bar_index(app, hwnd).and_then(|i| app.bars[i].hit(p.x, p.y)).as_deref() == Some("sound");
                if over_sound {
                    app.sampler.change_volume(delta * 0.02);
                    app.invalidate();
                }
            });
            LRESULT(0)
        }
        WM_APPBAR => {
            with_app(|app| {
                if let Some(i) = bar_index(app, hwnd) {
                    match wp.0 as u32 {
                        windows::Win32::UI::Shell::ABN_POSCHANGED => app.bars[i].position(),
                        windows::Win32::UI::Shell::ABN_FULLSCREENAPP if app.cfg.general.hide_on_fullscreen => {
                            let _ = ShowWindow(hwnd, if lp.0 != 0 { SW_HIDE } else { SW_SHOWNOACTIVATE });
                        }
                        _ => {}
                    }
                }
            });
            LRESULT(0)
        }
        WM_DPICHANGED => {
            with_app(|app| {
                if let Some(i) = bar_index(app, hwnd) {
                    app.bars[i].dpi = (wp.0 & 0xFFFF) as u32;
                    app.bars[i].position();
                }
            });
            LRESULT(0)
        }
        WM_DISPLAYCHANGE | WM_SETTINGCHANGE => {
            // Écran branché, débranché ou zone de travail modifiée : on reconstruit après coup.
            with_app(|app| {
                let _ = PostMessageW(app.main, WM_APP_REBUILD, WPARAM(0), LPARAM(0));
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            with_app(|app| match wp.0 {
                TIMER_TICK => {
                    if app.bars.iter().any(|b| !b.hidden) {
                        app.sampler.sample();
                        app.invalidate();
                    }
                }
                TIMER_CONFIG => app.reload(false),
                TIMER_AUTOHIDE => app.autohide(),
                _ => {}
            });
            LRESULT(0)
        }
        WM_APP_CLAUDE => {
            with_app(|app| app.invalidate());
            LRESULT(0)
        }
        WM_APP_REBUILD => {
            with_app(|app| app.rebuild());
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Fil de fond : relit les logs Claude Code au rythme configuré, sans toucher à l'interface.
fn spawn_claude(main: HWND, out: Arc<Mutex<Summary>>, cfg: Arc<Mutex<ClaudeConfig>>, wake: Arc<AtomicBool>) {
    let main = main.0 as isize;
    std::thread::spawn(move || loop {
        let c = cfg.lock().unwrap().clone();
        if c.enabled && c.sources.iter().any(|s| s == "claude_code") {
            let s = forge_claude::scan(&c, chrono::Local::now());
            *out.lock().unwrap() = s;
            unsafe {
                let _ = PostMessageW(HWND(main as _), WM_APP_CLAUDE, WPARAM(0), LPARAM(0));
            }
        }
        for _ in 0..c.refresh_seconds.max(5) {
            std::thread::sleep(std::time::Duration::from_secs(1));
            if wake.swap(false, Ordering::Relaxed) {
                break;
            }
        }
    });
}

pub fn run() -> Result<()> {
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\ForgeBar.SingleInstance"))?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return Ok(());
        }
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let path = config_path();
        let cfg = match Config::load_or_create(&path) {
            Ok(c) => c,
            Err(e) => {
                warn(format!("{e}\n\nForge Bar démarre avec la configuration par défaut.\n{}", path.display()));
                Config::default()
            }
        };
        let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
        let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

        let hinstance = GetModuleHandleW(None)?;
        let cursor = LoadCursorW(None, IDC_ARROW)?;
        let bar_class = WNDCLASSW {
            lpfnWndProc: Some(bar_proc),
            hInstance: hinstance.into(),
            hCursor: cursor,
            lpszClassName: w!("ForgeBarWindow"),
            ..Default::default()
        };
        let main_class = WNDCLASSW {
            lpfnWndProc: Some(main_proc),
            hInstance: hinstance.into(),
            lpszClassName: w!("ForgeBarMain"),
            ..Default::default()
        };
        RegisterClassW(&bar_class);
        RegisterClassW(&main_class);
        let main = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("ForgeBarMain"),
            w!("Forge Bar"),
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            None,
            hinstance,
            None,
        )?;

        let claude = Arc::new(Mutex::new(Summary::default()));
        let claude_cfg = Arc::new(Mutex::new(cfg.claude.clone()));
        let wake = Arc::new(AtomicBool::new(false));
        set_autostart(cfg.general.autostart);
        let mut app = App {
            cfg_mtime: mtime(&path),
            themes: ThemeStore::load(Some(&user_themes_dir())),
            icon_font: icon_font(&dwrite),
            cfg,
            d2d,
            dwrite,
            main,
            bars: Vec::new(),
            sampler: Sampler::default(),
            claude: claude.clone(),
            claude_cfg: claude_cfg.clone(),
            claude_wake: wake.clone(),
        };
        app.sampler.sample();
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        with_app(|app| app.rebuild());

        SetTimer(main, TIMER_TICK, 1000, None);
        SetTimer(main, TIMER_CONFIG, 2000, None);
        spawn_claude(main, claude, claude_cfg, wake);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        with_app(|app| {
            for mut b in app.bars.drain(..) {
                b.destroy();
            }
        });
    }
    Ok(())
}
