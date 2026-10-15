//! Procédures de fenêtre : barres, panneaux et fenêtre principale (minuteurs, messages des fils de fond).

use super::bar::{Ctx, WM_APPBAR};
use super::panel::{Kind, Panel};
use super::{
    apps, clipboard, speedtest, sys, tray, updater, wallpaper, with_app, App, HOTKEY_TOGGLE, TIMER_AUTOHIDE,
    TIMER_CONFIG, TIMER_TICK, WM_APP_AUDIO, WM_APP_CLAUDE, WM_APP_CLOSE_PANEL, WM_APP_MEDIA, WM_APP_REBUILD,
    WM_APP_SPEED, WM_APP_UPDATE, WM_APP_WEATHER,
};
use crate::calendar;
use crate::content::{self, Action};
use chrono::Datelike;
use forge_config::{config_path, user_themes_dir};
use std::time::Duration;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{ScreenToClient, ValidateRect};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_CLIPBOARDUPDATE: u32 = 0x031D;
const WM_HOTKEY: u32 = 0x0312;
const SPI_SETDESKWALLPAPER: usize = 0x14;
const MENU_SETTINGS: usize = 10;
const MENU_THEMES: usize = 11;
const MENU_RELOAD: usize = 12;
const MENU_QUIT: usize = 13;
const MENU_HIDE_BAR: usize = 14;
const MENU_UPDATE: usize = 15;
const MENU_MUTE: usize = 20;
const MENU_MIXER: usize = 21;
const MENU_SOUND_SETTINGS: usize = 22;
const MENU_SPEED: usize = 30;
const MENU_NET_SETTINGS: usize = 31;
const MENU_TASKMGR: usize = 40;
const MENU_STORAGE: usize = 41;
const MENU_WIFI: usize = 42;
const MENU_BATTERY: usize = 43;
const MENU_DATETIME: usize = 44;
const MENU_CALENDAR: usize = 45;
const MENU_CLAUDE: usize = 46;
const MENU_PREV: usize = 50;
const MENU_PLAY: usize = 51;
const MENU_NEXT: usize = 52;
const MENU_APP_OPEN: usize = 60;
const MENU_APP_MINIMIZE: usize = 61;
const MENU_APP_CLOSE: usize = 62;
/// 11 paliers de volume : 100 + 0 à 10 pour 0 à 100 %.
const MENU_VOLUME: usize = 100;

fn bar_index(app: &App, hwnd: HWND) -> Option<usize> {
    app.bars.iter().position(|b| b.hwnd == hwnd)
}

fn lparam_point(lp: LPARAM) -> (i32, i32) {
    ((lp.0 & 0xFFFF) as i16 as i32, ((lp.0 >> 16) & 0xFFFF) as i16 as i32)
}

fn open_settings() {
    let exe = super::exe_path();
    let settings = std::path::Path::new(&exe).with_file_name("forge-settings.exe");
    let target = if settings.exists() { settings } else { config_path() };
    sys::open(PCWSTR(HSTRING::from(target.as_os_str()).as_ptr()));
}

impl App {
    fn popup_scene(&self, kind: Kind, month: (i32, u32)) -> content::Scene {
        match kind {
            Kind::Calendar => {
                content::calendar(&calendar::month_grid(month.0, month.1, chrono::Local::now().date_naive()))
            }
            Kind::Claude => content::claude_detail(&self.claude.lock().unwrap(), &self.cfg.claude),
            Kind::Clipboard => content::clipboard(&self.clipboard, self.cfg.clipboard.max_chars),
            Kind::Floating => content::floating(&self.claude.lock().unwrap(), &self.cfg.claude),
        }
    }

    /// Ouvre (ou ferme, si c'est déjà lui) le panneau lié à un widget de la barre `bar`.
    fn toggle_popup(&mut self, bar: usize, kind: Kind) {
        if self.popup.as_ref().is_some_and(|p| p.kind == kind) {
            self.close_popup();
            return;
        }
        // Le clic qui vient de désactiver (donc fermer) ce panneau ne doit pas le rouvrir aussitôt.
        if self.popup_closed.take().is_some_and(|(k, t)| k == kind && t.elapsed() < Duration::from_millis(300)) {
            return;
        }
        self.close_popup();
        let id = match kind {
            Kind::Calendar => "clock",
            Kind::Claude => "claude",
            _ => "clipboard",
        };
        let b = &self.bars[bar];
        let Some(anchor) = b.cell_on_screen(id) else { return };
        let today = chrono::Local::now().date_naive();
        let month = (today.year(), today.month());
        let scene = self.popup_scene(kind, month);
        if let Some(mut p) = Panel::popup(kind, scene, b.theme.clone(), anchor, b.screen.edge, b.monitor, b.dpi) {
            p.month = month;
            self.popup = Some(p);
        }
    }

    /// Recalcule le contenu du panneau ouvert (nouvelles données Claude, presse-papiers…).
    pub(crate) fn refresh_popup(&mut self) {
        let Some(kind) = self.popup.as_ref().map(|p| p.kind) else { return };
        let month = self.popup.as_ref().map_or((0, 1), |p| p.month);
        let scene = self.popup_scene(kind, month);
        if let Some(p) = self.popup.as_mut() {
            p.set_scene(scene);
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(p.hwnd, None, false);
            }
        }
    }

    /// Crée, met à jour ou détruit le HUD flottant selon la configuration.
    pub(crate) fn refresh_floating(&mut self) {
        let f = self.cfg.claude.floating.clone();
        if !(f.enabled && self.cfg.claude.enabled) {
            if let Some(mut p) = self.floating.take() {
                p.destroy();
            }
            return;
        }
        let scene = content::floating(&self.claude.lock().unwrap(), &self.cfg.claude);
        let Some(mon) = super::monitors().into_iter().next() else { return };
        if self.floating.is_none() {
            let theme = self.theme_for(&self.cfg.screen[0].theme);
            let mut dpi = (96u32, 96u32);
            unsafe {
                let _ = windows::Win32::UI::HiDpi::GetDpiForMonitor(
                    mon.handle,
                    windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI,
                    &mut dpi.0,
                    &mut dpi.1,
                );
            }
            self.floating =
                Panel::floating(scene, theme, f.corner, f.margin, mon.rect, dpi.0, f.opacity, f.click_through);
        } else if let Some(p) = self.floating.as_mut() {
            p.set_scene(scene);
            p.reposition_floating(f.corner, f.margin, mon.rect);
        }
        if let Some(p) = &self.floating {
            unsafe {
                let _ = windows::Win32::Graphics::Gdi::InvalidateRect(p.hwnd, None, false);
            }
        }
    }

    fn on_clipboard_changed(&mut self) {
        if !self.cfg.clipboard.enabled {
            return;
        }
        if let Some(text) = clipboard::read_text(self.main) {
            self.clipboard.retain(|t| *t != text);
            self.clipboard.insert(0, text);
            self.clipboard.truncate(self.cfg.clipboard.history.clamp(1, 50));
            self.sampler.data.clipboard = Some(self.clipboard.clone());
            self.invalidate();
            self.refresh_popup();
        }
    }
}

fn click(hwnd: HWND, x: i32, y: i32) {
    let hit = with_app(|app| bar_index(app, hwnd).map(|i| (i, app.bars[i].hit(x, y)))).flatten();
    let Some((i, Some(id))) = hit else { return };
    if let Some(h) = id.strip_prefix("app:").and_then(|h| h.parse().ok()) {
        apps::activate(h);
        return;
    }
    match id.as_str() {
        "desktop" => sys::toggle_desktop(),
        "wifi" => sys::open(w!("ms-settings:network-wifi")),
        "net" => sys::open(w!("ms-settings:network-status")),
        "battery" => sys::open(w!("ms-settings:batterysaver")),
        "cpu" | "ram" | "gpu" => sys::open(w!("taskmgr.exe")),
        "disk" => sys::open(w!("ms-settings:storagesense")),
        "media" => sys::media_key(0xB3),
        "toggle" => {
            with_app(|app| app.toggle_bars());
            return;
        }
        "sound" => {
            with_app(|app| app.sampler.toggle_mute());
        }
        "clock" => {
            with_app(|app| app.toggle_popup(i, Kind::Calendar));
        }
        "claude" => {
            with_app(|app| app.toggle_popup(i, Kind::Claude));
        }
        "clipboard" => {
            with_app(|app| app.toggle_popup(i, Kind::Clipboard));
        }
        _ => return,
    }
    with_app(|app| app.invalidate());
}

fn add(menu: HMENU, id: usize, text: &str, flags: MENU_ITEM_FLAGS) {
    unsafe {
        let _ = AppendMenuW(menu, MF_STRING | flags, id, &HSTRING::from(text));
    }
}

fn separator(menu: HMENU) {
    unsafe {
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
    }
}

/// Entrées propres au widget sous la souris. Renvoie vrai s'il y en a (un séparateur suit alors).
fn widget_items(menu: HMENU, id: &str, app: &App) -> bool {
    let none = MENU_ITEM_FLAGS(0);
    match id {
        "sound" => {
            let Some((v, muted)) = app.sampler.data.volume else { return false };
            add(menu, MENU_MUTE, if muted { "Réactiver le son" } else { "Couper le son" }, none);
            if let Ok(sub) = unsafe { CreatePopupMenu() } {
                let level = (v * 10.0).round() as usize;
                for step in 0..=10usize {
                    let flags = if step == level && !muted { MF_CHECKED } else { none };
                    add(sub, MENU_VOLUME + step, &format!("{} %", step * 10), flags);
                }
                unsafe {
                    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, &HSTRING::from("Volume"));
                }
            }
            add(menu, MENU_MIXER, "Mélangeur de volume…", none);
            add(menu, MENU_SOUND_SETTINGS, "Paramètres du son…", none);
        }
        "net" => {
            let running = app.speed.running.load(std::sync::atomic::Ordering::Relaxed);
            if running {
                add(menu, MENU_SPEED, "Test de débit en cours…", MF_GRAYED);
            } else {
                add(menu, MENU_SPEED, "Test de débit", none);
            }
            if let Some(last) = &app.speed_last {
                add(menu, 0, &format!("Dernier test : {last}"), MF_GRAYED);
            }
            add(menu, MENU_NET_SETTINGS, "Paramètres réseau…", none);
        }
        "cpu" | "ram" | "gpu" => add(menu, MENU_TASKMGR, "Gestionnaire des tâches…", none),
        "disk" => {
            add(menu, MENU_STORAGE, "Stockage…", none);
            add(menu, MENU_TASKMGR, "Gestionnaire des tâches…", none);
        }
        "wifi" => add(menu, MENU_WIFI, "Paramètres Wi-Fi…", none),
        "battery" => add(menu, MENU_BATTERY, "Paramètres de batterie…", none),
        "clock" => {
            add(menu, MENU_CALENDAR, "Afficher le calendrier", none);
            add(menu, MENU_DATETIME, "Date et heure…", none);
        }
        "claude" => add(menu, MENU_CLAUDE, "Afficher le détail Claude", none),
        "media" => {
            add(menu, MENU_PREV, "Piste précédente", none);
            add(menu, MENU_PLAY, "Lecture / Pause", none);
            add(menu, MENU_NEXT, "Piste suivante", none);
        }
        a if a.starts_with("app:") => {
            let title = app
                .sampler
                .data
                .apps
                .iter()
                .find(|w| format!("app:{}", w.hwnd) == a)
                .map_or(String::new(), |w| crate::widgets::truncate(&w.title, 40));
            add(menu, 0, &title, MF_GRAYED);
            add(menu, MENU_APP_OPEN, "Afficher", none);
            add(menu, MENU_APP_MINIMIZE, "Réduire", none);
            add(menu, MENU_APP_CLOSE, "Fermer la fenêtre", none);
        }
        _ => return false,
    }
    true
}

/// Menu contextuel : entrées du widget sous la souris, puis réglages de la barre.
fn context_menu(hwnd: HWND, bar: Option<usize>, widget: Option<String>) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let target = widget.as_deref().unwrap_or("");
        if with_app(|app| widget_items(menu, target, app)).unwrap_or(false) {
            separator(menu);
        }
        let none = MENU_ITEM_FLAGS(0);
        let off = with_app(|app| app.bars_off).unwrap_or(false);
        let hide = if off { "Afficher la barre   Ctrl+Alt+B" } else { "Masquer la barre   Ctrl+Alt+B" };
        add(menu, MENU_HIDE_BAR, hide, none);
        add(menu, MENU_SETTINGS, "Personnaliser la barre…", none);
        add(menu, MENU_THEMES, "Dossier des thèmes", none);
        add(menu, MENU_RELOAD, "Recharger", none);
        add(menu, MENU_UPDATE, "Rechercher une mise à jour", none);
        separator(menu);
        add(menu, MENU_QUIT, "Quitter Forge Bar", none);
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        let _ = SetForegroundWindow(hwnd);
        let cmd =
            TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, p.x, p.y, 0, hwnd, None).0 as usize;
        let _ = DestroyMenu(menu);
        let app_hwnd = target.strip_prefix("app:").and_then(|h| h.parse::<isize>().ok());
        match cmd {
            0 => {}
            MENU_SETTINGS => open_settings(),
            MENU_THEMES => {
                let dir = user_themes_dir();
                let _ = std::fs::create_dir_all(&dir);
                sys::open(PCWSTR(HSTRING::from(dir.as_os_str()).as_ptr()));
            }
            MENU_RELOAD => {
                with_app(|app| app.reload(true));
            }
            MENU_QUIT => PostQuitMessage(0),
            MENU_UPDATE => {
                with_app(|app| {
                    tray::info(app.main, "Mise à jour", "Recherche en cours…");
                    app.update.wake.store(true, std::sync::atomic::Ordering::Relaxed);
                });
            }
            MENU_HIDE_BAR => {
                with_app(|app| app.toggle_bars());
            }
            MENU_MUTE => {
                with_app(|app| app.sampler.toggle_mute());
            }
            c if (MENU_VOLUME..=MENU_VOLUME + 10).contains(&c) => {
                with_app(|app| app.sampler.set_volume((c - MENU_VOLUME) as f32 / 10.0));
            }
            MENU_MIXER => sys::open(w!("ms-settings:apps-volume")),
            MENU_SOUND_SETTINGS => sys::open(w!("ms-settings:sound")),
            MENU_SPEED => {
                with_app(|app| {
                    tray::info(app.main, "Test de débit", "Mesure en cours, environ 20 secondes…");
                    speedtest::spawn(app.main, WM_APP_SPEED, &app.speed);
                });
            }
            MENU_NET_SETTINGS => sys::open(w!("ms-settings:network-status")),
            MENU_TASKMGR => sys::open(w!("taskmgr.exe")),
            MENU_STORAGE => sys::open(w!("ms-settings:storagesense")),
            MENU_WIFI => sys::open(w!("ms-settings:network-wifi")),
            MENU_BATTERY => sys::open(w!("ms-settings:batterysaver")),
            MENU_DATETIME => sys::open(w!("ms-settings:dateandtime")),
            MENU_CALENDAR | MENU_CLAUDE => {
                let kind = if cmd == MENU_CALENDAR { Kind::Calendar } else { Kind::Claude };
                if let Some(i) = bar {
                    with_app(|app| app.toggle_popup(i, kind));
                }
            }
            MENU_PREV => sys::media_key(0xB1),
            MENU_PLAY => sys::media_key(0xB3),
            MENU_NEXT => sys::media_key(0xB0),
            MENU_APP_OPEN | MENU_APP_MINIMIZE | MENU_APP_CLOSE => {
                if let Some(h) = app_hwnd {
                    match cmd {
                        MENU_APP_OPEN => apps::activate(h),
                        MENU_APP_MINIMIZE => apps::minimize(h),
                        _ => apps::close(h),
                    }
                }
            }
            _ => {}
        }
        with_app(|app| app.invalidate());
    }
}

pub(super) unsafe extern "system" fn bar_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let painted = with_app(|app| {
                let App { bars, d2d, dwrite, icon_font, sampler, cfg, claude, pack, .. } = app;
                let summary = claude.lock().unwrap().clone();
                let ctx =
                    Ctx { d2d, dwrite, icon_font, sys: &sampler.data, claude: &summary, claude_cfg: &cfg.claude, pack };
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
            let (x, y) = lparam_point(lp);
            let hit = with_app(|app| bar_index(app, hwnd).map(|i| (i, app.bars[i].hit(x, y)))).flatten();
            let (bar, widget) = hit.map_or((None, None), |(i, w)| (Some(i), w));
            context_menu(hwnd, bar, widget);
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
        WM_DISPLAYCHANGE => {
            // Écran branché ou débranché, résolution changée : on reconstruit après coup.
            with_app(|app| {
                let _ = PostMessageW(app.main, WM_APP_REBUILD, WPARAM(0), LPARAM(0));
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_SETTINGCHANGE => {
            // Seul le changement de fond d'écran nous concerne (accent). Les changements de zone de travail
            // viennent de nos propres barres : les traiter ici bouclerait.
            // Mode clair/sombre ou couleur d'accentuation de Windows changés : les thèmes adaptatifs se recalculent.
            let colors = !PCWSTR(lp.0 as *const u16).is_null()
                && lp.0 != 0
                && unsafe { PCWSTR(lp.0 as *const u16).to_string() }.is_ok_and(|s| s == "ImmersiveColorSet");
            if colors {
                with_app(|app| {
                    let _ = PostMessageW(app.main, WM_APP_REBUILD, WPARAM(0), LPARAM(0));
                });
            }
            if wp.0 == SPI_SETDESKWALLPAPER {
                with_app(|app| {
                    let _ = PostMessageW(app.main, WM_APP_REBUILD, wp, LPARAM(0));
                });
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

pub(super) unsafe extern "system" fn panel_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            with_app(|app| {
                let App { popup, floating, d2d, dwrite, .. } = app;
                for p in [popup.as_mut(), floating.as_mut()].into_iter().flatten() {
                    if p.hwnd == hwnd {
                        let _ = p.paint(d2d, dwrite);
                    }
                }
            });
            let _ = ValidateRect(hwnd, None);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONUP => {
            let (x, y) = lparam_point(lp);
            with_app(|app| {
                let Some(p) = app.popup.as_mut().filter(|p| p.hwnd == hwnd) else { return };
                match p.hit(x, y) {
                    Some(a @ (Action::CalPrev | Action::CalNext | Action::CalToday)) => {
                        let today = chrono::Local::now().date_naive();
                        p.month = match a {
                            Action::CalPrev => calendar::shift_month(p.month.0, p.month.1, -1),
                            Action::CalNext => calendar::shift_month(p.month.0, p.month.1, 1),
                            _ => (today.year(), today.month()),
                        };
                        p.set_scene(content::calendar(&calendar::month_grid(p.month.0, p.month.1, today)));
                        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(p.hwnd, None, false);
                    }
                    Some(Action::ClipPick(i)) => {
                        let text = app.clipboard.get(i).cloned();
                        let main = app.main;
                        app.close_popup();
                        if let Some(t) = text {
                            clipboard::write_text(main, &t);
                        }
                    }
                    Some(Action::ClipClear) => {
                        app.clipboard.clear();
                        app.sampler.data.clipboard = Some(Vec::new());
                        app.invalidate();
                        app.refresh_popup();
                    }
                    None => {}
                }
            });
            LRESULT(0)
        }
        WM_ACTIVATE => {
            // Perdre le focus ferme le panneau contextuel, comme un menu.
            if wp.0 & 0xFFFF == 0 {
                with_app(|app| {
                    if app.popup.as_ref().is_some_and(|p| p.hwnd == hwnd) {
                        let _ = PostMessageW(app.main, WM_APP_CLOSE_PANEL, WPARAM(0), LPARAM(0));
                    }
                });
            }
            LRESULT(0)
        }
        WM_KEYDOWN if wp.0 as u16 == VK_ESCAPE.0 => {
            with_app(|app| {
                let _ = PostMessageW(app.main, WM_APP_CLOSE_PANEL, WPARAM(0), LPARAM(0));
            });
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// Résultat d'une recherche de mise à jour : installation seule pour la copie installée par le MSI,
/// simple annonce pour une copie lancée d'ailleurs (développement, zip).
fn on_update(app: &mut App) {
    use updater::Outcome;
    let manual = app.update.manual.load(std::sync::atomic::Ordering::Relaxed);
    let outcome = app.update.outcome.lock().unwrap().take();
    match outcome {
        Some(Outcome::Ready { version, msi }) => {
            let exe = super::exe_path();
            if updater::is_installed_copy(&exe) {
                tray::info(
                    app.main,
                    "Mise à jour",
                    &format!("Forge Bar {version} : installation, la barre redémarre…"),
                );
                updater::install_and_restart(&msi, &exe);
                unsafe { PostQuitMessage(0) };
            } else if manual || app.update_notified.as_deref() != Some(version.as_str()) {
                let text = format!(
                    "La version {version} est prête mais cette copie n'est pas celle du MSI : installez {}",
                    msi.display()
                );
                tray::info(app.main, "Mise à jour disponible", &text);
                app.update_notified = Some(version);
            }
        }
        Some(Outcome::UpToDate) if manual => {
            tray::info(app.main, "Mise à jour", &format!("Forge Bar {} est à jour.", env!("CARGO_PKG_VERSION")));
        }
        Some(Outcome::Failed(e)) if manual => tray::balloon(app.main, "Mise à jour", &format!("Échec : {e}"), true),
        _ => {}
    }
}

pub(super) unsafe extern "system" fn main_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            with_app(|app| match wp.0 {
                TIMER_TICK if !app.bars_off && app.bars.iter().any(|b| !b.hidden) => {
                    app.sync_shared();
                    app.sampler.sample();
                    app.invalidate();
                }
                TIMER_CONFIG => app.reload(false),
                TIMER_AUTOHIDE => app.autohide(),
                _ => {}
            });
            LRESULT(0)
        }
        WM_APP_CLAUDE => {
            with_app(|app| {
                if app.cfg.claude.notifications {
                    let s = app.claude.lock().unwrap().clone();
                    for n in app.notifier.check(&s, &app.cfg.claude, chrono::Local::now()) {
                        tray::balloon(app.main, &n.title, &n.body, n.critical);
                    }
                }
                app.invalidate();
                app.refresh_floating();
                if app.popup.as_ref().is_some_and(|p| p.kind == Kind::Claude) {
                    app.refresh_popup();
                }
            });
            LRESULT(0)
        }
        WM_APP_MEDIA | WM_APP_WEATHER => {
            with_app(|app| {
                app.sync_shared();
                app.invalidate();
            });
            LRESULT(0)
        }
        WM_APP_AUDIO => {
            with_app(|app| {
                app.sampler.on_audio_message(wp.0, lp.0);
                app.invalidate();
            });
            LRESULT(0)
        }
        WM_APP_REBUILD => {
            with_app(|app| {
                if wp.0 == SPI_SETDESKWALLPAPER && app.cfg.general.accent_from_wallpaper {
                    app.accent = wallpaper::accent();
                }
                app.rebuild();
            });
            LRESULT(0)
        }
        WM_APP_CLOSE_PANEL => {
            with_app(|app| app.close_popup());
            LRESULT(0)
        }
        WM_CLIPBOARDUPDATE => {
            with_app(|app| app.on_clipboard_changed());
            LRESULT(0)
        }
        WM_HOTKEY if wp.0 as i32 == HOTKEY_TOGGLE => {
            with_app(|app| app.toggle_bars());
            LRESULT(0)
        }
        WM_APP_UPDATE => {
            with_app(on_update);
            LRESULT(0)
        }
        WM_APP_SPEED => {
            with_app(|app| {
                let result = app.speed.result.lock().unwrap().take();
                match result {
                    Some(Ok(text)) => {
                        tray::info(app.main, "Test de débit", &text);
                        app.speed_last = Some(text);
                    }
                    Some(Err(e)) => tray::balloon(app.main, "Test de débit", &format!("Échec : {e}"), true),
                    None => {}
                }
            });
            LRESULT(0)
        }
        tray::WM_TRAY => {
            match (lp.0 & 0xFFFF) as u32 {
                WM_RBUTTONUP => context_menu(hwnd, None, None),
                WM_LBUTTONUP => {
                    with_app(|app| app.toggle_bars());
                }
                _ => {}
            }
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}
