//! Application Windows : une fenêtre de messages invisible porte les minuteurs,
//! une fenêtre de barre par écran configuré, des panneaux à la demande.

mod apps;
mod audio;
mod bar;
mod clipboard;
mod cred;
mod events;
mod http;
mod icons;
mod media;
mod metrics;
mod panel;
mod render;
pub mod snapshot;
mod speedtest;
mod sys;
mod tray;
mod updater;
mod wallpaper;
mod workers;

use bar::Bar;
use forge_claude::notify::Notifier;
use forge_claude::Summary;
use forge_config::{config_path, user_themes_dir, ClaudeConfig, Config, Mode, Rgba, ThemeStore, Weather};
use panel::Panel;
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};
use sys::Sampler;
use windows::core::{w, Result, HSTRING};
use windows::Win32::Foundation::{GetLastError, BOOL, ERROR_ALREADY_EXISTS, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Direct2D::{D2D1CreateFactory, ID2D1Factory, D2D1_FACTORY_TYPE_SINGLE_THREADED};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteFontCollection, DWRITE_FACTORY_TYPE_SHARED,
};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, InvalidateRect, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows::Win32::System::Registry::{RegDeleteKeyValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    GetDpiForMonitor, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, MDT_EFFECTIVE_DPI,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT,
};
use windows::Win32::UI::WindowsAndMessaging::*;

pub(crate) const WM_APP_CLAUDE: u32 = WM_APP + 2;
pub(crate) const WM_APP_REBUILD: u32 = WM_APP + 3;
pub(crate) const WM_APP_AUDIO: u32 = WM_APP + 4;
pub(crate) const WM_APP_MEDIA: u32 = WM_APP + 6;
pub(crate) const WM_APP_WEATHER: u32 = WM_APP + 7;
pub(crate) const WM_APP_CLOSE_PANEL: u32 = WM_APP + 8;
pub(crate) const WM_APP_SPEED: u32 = WM_APP + 9;
pub(crate) const WM_APP_UPDATE: u32 = WM_APP + 10;
pub(crate) const HOTKEY_TOGGLE: i32 = 1;
pub(crate) const TIMER_TICK: usize = 1;
pub(crate) const TIMER_CONFIG: usize = 2;
pub(crate) const TIMER_AUTOHIDE: usize = 3;

pub(crate) struct MonitorInfo {
    pub handle: HMONITOR,
    pub rect: RECT,
    pub primary: bool,
}

pub(crate) struct App {
    pub cfg: Config,
    pub cfg_mtime: Option<SystemTime>,
    pub themes: ThemeStore,
    pub d2d: ID2D1Factory,
    pub dwrite: IDWriteFactory,
    pub icon_font: String,
    pub main: HWND,
    pub bars: Vec<Bar>,
    pub sampler: Sampler,
    pub claude: Arc<Mutex<Summary>>,
    pub claude_cfg: Arc<Mutex<ClaudeConfig>>,
    pub claude_wake: Arc<AtomicBool>,
    pub media: Arc<Mutex<Option<crate::widgets::Media>>>,
    pub weather: Arc<Mutex<Option<crate::weather::Now>>>,
    pub weather_cfg: Arc<Mutex<Weather>>,
    pub weather_wake: Arc<AtomicBool>,
    pub pack: icons::Pack,
    pub pack_name: String,
    /// Accent tiré du fond d'écran, si activé.
    pub accent: Option<Rgba>,
    pub popup: Option<Panel>,
    /// Dernier panneau fermé : un clic sur le widget qui vient de le fermer ne doit pas le rouvrir.
    pub popup_closed: Option<(panel::Kind, Instant)>,
    pub floating: Option<Panel>,
    pub clipboard: Vec<String>,
    pub clipboard_listening: bool,
    pub notifier: Notifier,
    /// Barres masquées par l'utilisateur ; reste vrai à travers les reconstructions.
    pub bars_off: bool,
    pub speed: speedtest::Shared,
    /// Dernier résultat du test de débit, affiché dans le menu du widget réseau.
    pub speed_last: Option<String>,
    pub update: updater::Shared,
    /// Dernière version annoncée pour une copie qui ne peut pas s'installer seule (évite de répéter la bulle).
    pub update_notified: Option<String>,
}

thread_local! {
    pub(crate) static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Exécute `f` sur l'application si elle n'est pas déjà empruntée.
/// Les messages reçus pendant un appel Windows réentrant tombent dans le traitement par défaut.
pub(crate) fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| cell.try_borrow_mut().ok().and_then(|mut a| a.as_mut().map(f)))
}

pub fn fatal(msg: &str) {
    unsafe { MessageBoxW(None, &HSTRING::from(msg), w!("Forge Bar"), MB_ICONERROR) };
}

/// Message non bloquant, affiché depuis un autre fil pour ne pas suspendre la barre.
pub(crate) fn warn(msg: String) {
    std::thread::spawn(move || unsafe {
        MessageBoxW(None, &HSTRING::from(msg), w!("Forge Bar"), MB_ICONWARNING);
    });
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

pub(crate) fn exe_path() -> String {
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

unsafe extern "system" fn collect_monitor(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let list = &mut *(data.0 as *mut Vec<MonitorInfo>);
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(h, &mut mi).as_bool() {
        list.push(MonitorInfo { handle: h, rect: mi.rcMonitor, primary: mi.dwFlags & 1 != 0 });
    }
    BOOL(1)
}

pub(crate) fn monitors() -> Vec<MonitorInfo> {
    let mut list: Vec<MonitorInfo> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(&mut list as *mut _ as isize));
    }
    // Écran principal d'abord, puis de gauche à droite : c'est l'ordre des numéros d'écran.
    list.sort_by_key(|m| (!m.primary, m.rect.left, m.rect.top));
    list
}

fn layout_uses(cfg: &Config, id: &str) -> bool {
    cfg.screen
        .iter()
        .filter(|s| s.enabled)
        .any(|s| s.layout.rows().iter().any(|z| z.left.iter().chain(&z.center).chain(&z.right).any(|w| w == id)))
}

impl App {
    /// Thème prêt à dessiner : adaptatif résolu selon Windows, sinon accent du fond d'écran si demandé.
    pub(crate) fn theme_for(&self, id: &str) -> forge_config::Theme {
        let mut theme = self.themes.get(id);
        if theme.adaptive != forge_config::Adaptive::None {
            let wallpaper = theme.adaptive == forge_config::Adaptive::Wallpaper;
            return theme.adapted(&wallpaper::env(wallpaper));
        }
        if let Some(a) = self.accent {
            theme.colors.accent = crate::dominant::hex(a);
        }
        theme
    }

    pub(crate) fn rebuild(&mut self) {
        self.close_popup();
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
            let theme = self.theme_for(&screen.theme);
            let mut bar = Bar::new(hwnd, screen, theme, mon.rect, dx);
            bar.off = self.bars_off;
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
        self.sampler.want_gpu = layout_uses(&self.cfg, "gpu");
        self.sampler.want_net = layout_uses(&self.cfg, "net");
        self.sampler.want_disk = layout_uses(&self.cfg, "disk");
        self.sampler.want_apps = layout_uses(&self.cfg, "apps");
        self.refresh_floating();
        self.invalidate();
    }

    pub(crate) fn reload(&mut self, force: bool) {
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
                *self.weather_cfg.lock().unwrap() = self.cfg.weather.clone();
                self.weather_wake.store(true, Ordering::Relaxed);
                set_autostart(self.cfg.general.autostart);
                self.update.enabled.store(self.cfg.general.auto_update, Ordering::Relaxed);
                if self.pack_name != self.cfg.general.icon_pack {
                    self.pack_name = self.cfg.general.icon_pack.clone();
                    self.pack = icons::load(&self.pack_name, &self.d2d);
                }
                self.accent = self.cfg.general.accent_from_wallpaper.then(wallpaper::accent).flatten();
                self.sync_clipboard_listener();
                self.rebuild();
            }
            Err(e) => warn(format!("{}\n\nLa configuration précédente reste active.\n{}", e, path.display())),
        }
    }

    pub(crate) fn sync_clipboard_listener(&mut self) {
        let on = self.cfg.clipboard.enabled;
        if on != self.clipboard_listening {
            clipboard::listen(self.main, on);
            self.clipboard_listening = on;
        }
        if !on {
            self.clipboard.clear();
        }
        self.sampler.data.clipboard = on.then(|| self.clipboard.clone());
    }

    pub(crate) fn invalidate(&self) {
        for b in &self.bars {
            unsafe {
                let _ = InvalidateRect(b.hwnd, None, false);
            }
        }
    }

    /// Cache ou ré-affiche toutes les barres (bouton, raccourci Ctrl+Alt+B, icône de notification).
    pub(crate) fn toggle_bars(&mut self) {
        self.bars_off = !self.bars_off;
        self.close_popup();
        for b in &mut self.bars {
            b.set_off(self.bars_off);
        }
        self.invalidate();
    }

    pub(crate) fn autohide(&mut self) {
        let mut p = windows::Win32::Foundation::POINT::default();
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

    /// Recopie les données des fils de fond dans les mesures affichées.
    pub(crate) fn sync_shared(&mut self) {
        self.sampler.data.media = self.media.lock().unwrap().clone();
        self.sampler.data.weather = self.weather.lock().unwrap().clone();
    }

    pub(crate) fn close_popup(&mut self) {
        if let Some(mut p) = self.popup.take() {
            self.popup_closed = Some((p.kind, Instant::now()));
            p.destroy();
        }
    }
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
        let class = |name: windows::core::PCWSTR, proc: WNDPROC| WNDCLASSW {
            lpfnWndProc: proc,
            hInstance: hinstance.into(),
            hCursor: cursor,
            lpszClassName: name,
            ..Default::default()
        };
        RegisterClassW(&class(w!("ForgeBarWindow"), Some(events::bar_proc)));
        RegisterClassW(&class(w!("ForgeBarPanel"), Some(events::panel_proc)));
        RegisterClassW(&class(w!("ForgeBarMain"), Some(events::main_proc)));
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
        let claude_wake = Arc::new(AtomicBool::new(false));
        let media = Arc::new(Mutex::new(None));
        let weather = Arc::new(Mutex::new(None));
        let weather_cfg = Arc::new(Mutex::new(cfg.weather.clone()));
        let weather_wake = Arc::new(AtomicBool::new(false));
        set_autostart(cfg.general.autostart);
        let pack = icons::load(&cfg.general.icon_pack, &d2d);
        let mut app = App {
            cfg_mtime: mtime(&path),
            themes: ThemeStore::load(Some(&user_themes_dir())),
            icon_font: icon_font(&dwrite),
            accent: cfg.general.accent_from_wallpaper.then(wallpaper::accent).flatten(),
            pack_name: cfg.general.icon_pack.clone(),
            pack,
            cfg,
            d2d,
            dwrite,
            main,
            bars: Vec::new(),
            sampler: Sampler::default(),
            claude: claude.clone(),
            claude_cfg: claude_cfg.clone(),
            claude_wake: claude_wake.clone(),
            media: media.clone(),
            weather: weather.clone(),
            weather_cfg: weather_cfg.clone(),
            weather_wake: weather_wake.clone(),
            popup: None,
            popup_closed: None,
            floating: None,
            clipboard: Vec::new(),
            clipboard_listening: false,
            notifier: Notifier::default(),
            bars_off: false,
            speed: speedtest::Shared::default(),
            speed_last: None,
            update: updater::Shared::default(),
            update_notified: None,
        };
        app.update.enabled.store(app.cfg.general.auto_update, Ordering::Relaxed);
        let update = app.update.clone();
        app.sampler.watch_audio(main.0 as isize, WM_APP_AUDIO);
        app.sync_clipboard_listener();
        app.sampler.want_gpu = layout_uses(&app.cfg, "gpu");
        app.sampler.want_net = layout_uses(&app.cfg, "net");
        app.sampler.want_disk = layout_uses(&app.cfg, "disk");
        app.sampler.want_apps = layout_uses(&app.cfg, "apps");
        app.sampler.sample();
        APP.with(|cell| *cell.borrow_mut() = Some(app));
        with_app(|app| app.rebuild());

        SetTimer(main, TIMER_TICK, 1000, None);
        SetTimer(main, TIMER_CONFIG, 2000, None);
        workers::spawn_claude(main, WM_APP_CLAUDE, claude, claude_cfg, claude_wake);
        workers::spawn_weather(main, WM_APP_WEATHER, weather, weather_cfg, weather_wake);
        media::spawn(main, WM_APP_MEDIA, media);
        updater::spawn(main, WM_APP_UPDATE, &update);
        tray::add(main);
        // Ctrl + Alt + B : afficher / masquer la barre. Silencieux si une autre application a déjà pris la combinaison.
        let _ = RegisterHotKey(main, HOTKEY_TOGGLE, MOD_CONTROL | MOD_ALT | MOD_NOREPEAT, 'B' as u32);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = UnregisterHotKey(main, HOTKEY_TOGGLE);
        tray::remove(main);
        clipboard::listen(main, false);
        with_app(|app| {
            app.close_popup();
            if let Some(mut f) = app.floating.take() {
                f.destroy();
            }
            for mut b in app.bars.drain(..) {
                b.destroy();
            }
        });
    }
    Ok(())
}
