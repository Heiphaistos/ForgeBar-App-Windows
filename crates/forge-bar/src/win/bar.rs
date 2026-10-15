//! Une barre = une fenêtre sur un écran. Positionnement par l'API AppBar
//! (espace réservé) ou en superposition ; le dessin est dans `render`.

use super::icons::Pack;
use crate::widgets::SysData;
use forge_claude::Summary;
use forge_config::{Backdrop, ClaudeConfig, Condition, Edge, Mode, ScreenConfig, Theme};
use std::collections::HashMap;
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{D2D_RECT_F, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{ID2D1Factory, ID2D1HwndRenderTarget};
use windows::Win32::Graphics::DirectWrite::{IDWriteFactory, IDWriteTextFormat};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW,
    DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DWMWCP_ROUND,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
    APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
};

/// Flou acrylique derrière la fenêtre via `SetWindowCompositionAttribute` (user32, non documentée,
/// stable depuis Windows 10 1803 ; méthode de TranslucentTB). Renvoie false si l'API est absente.
fn set_acrylic_blur(hwnd: HWND, on: bool) -> bool {
    use windows::core::{s, w};
    use windows::Win32::Foundation::BOOL;
    use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
    #[repr(C)]
    struct AccentPolicy {
        state: u32,
        flags: u32,
        gradient: u32,
        animation: u32,
    }
    #[repr(C)]
    struct CompositionData {
        attribute: u32,
        data: *mut AccentPolicy,
        size: usize,
    }
    type SetWca = unsafe extern "system" fn(HWND, *mut CompositionData) -> BOOL;
    const WCA_ACCENT_POLICY: u32 = 19;
    const ACCENT_DISABLED: u32 = 0;
    const ACCENT_ENABLE_ACRYLICBLURBEHIND: u32 = 4;
    unsafe {
        let Ok(user32) = GetModuleHandleW(w!("user32.dll")) else { return false };
        let Some(proc) = GetProcAddress(user32, s!("SetWindowCompositionAttribute")) else { return false };
        let set: SetWca = std::mem::transmute(proc);
        // Teinte ABGR quasi nulle : la couleur et l'opacité viennent du fond dessiné par le thème
        // (alpha 0 fait clignoter certaines versions de Windows).
        let mut policy = AccentPolicy {
            state: if on { ACCENT_ENABLE_ACRYLICBLURBEHIND } else { ACCENT_DISABLED },
            flags: 0,
            gradient: 0x0100_0000,
            animation: 0,
        };
        let mut data = CompositionData {
            attribute: WCA_ACCENT_POLICY,
            data: &mut policy,
            size: std::mem::size_of::<AccentPolicy>(),
        };
        set(hwnd, &mut data).as_bool()
    }
}

pub const WM_APPBAR: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

/// Ce dont le rendu a besoin, prêté par l'application à chaque image.
pub struct Ctx<'a> {
    pub d2d: &'a ID2D1Factory,
    pub dwrite: &'a IDWriteFactory,
    pub icon_font: &'a str,
    pub sys: &'a SysData,
    pub claude: &'a Summary,
    pub claude_cfg: &'a ClaudeConfig,
    pub pack: &'a Pack,
}

pub(super) struct Fonts {
    pub text: IDWriteTextFormat,
    pub icon: IDWriteTextFormat,
    pub small: IDWriteTextFormat,
}

pub struct Bar {
    pub hwnd: HWND,
    pub screen: ScreenConfig,
    pub theme: Theme,
    pub monitor: RECT,
    pub dpi: u32,
    pub hidden: bool,
    pub rect: RECT,
    registered: bool,
    pub(super) rt: Option<ID2D1HwndRenderTarget>,
    pub(super) fonts: Option<Fonts>,
    /// Conditions d'affichage déjà analysées (`screen.show_when`).
    pub(super) conds: HashMap<String, Condition>,
    /// Zones cliquables de la dernière image, en pixels logiques.
    pub hits: Vec<(D2D_RECT_F, String)>,
}

impl Bar {
    pub fn new(hwnd: HWND, screen: ScreenConfig, theme: Theme, monitor: RECT, dpi: u32) -> Self {
        let mut bar = Self {
            hwnd,
            screen,
            theme,
            monitor,
            dpi,
            hidden: false,
            rect: RECT::default(),
            registered: false,
            rt: None,
            fonts: None,
            conds: HashMap::new(),
            hits: Vec::new(),
        };
        bar.conds =
            bar.screen.show_when.iter().filter_map(|(k, v)| Some((k.clone(), Condition::parse(v).ok()?))).collect();
        if !hwnd.is_invalid() {
            bar.apply_backdrop();
        }
        bar
    }

    pub(super) fn scale(&self) -> f32 {
        self.dpi as f32 / 96.0
    }

    /// Effets DWM : transparence par pixel partout, Mica ou Acrylic sous Windows 11.
    fn apply_backdrop(&mut self) {
        unsafe {
            // Étendre le cadre à toute la fenêtre donne une transparence par pixel,
            // y compris sous Windows 10 : le fond n'est que ce que l'on dessine.
            let m = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(self.hwnd, &m);
            let t = self.theme.text();
            let dark: i32 = ((0.299 * t.r + 0.587 * t.g + 0.114 * t.b) > 0.5) as i32;
            let _ = DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_USE_IMMERSIVE_DARK_MODE,
                &dark as *const _ as _,
                std::mem::size_of::<i32>() as u32,
            );
            // Les fonds système Acrylic/Mica deviennent opaques dès que la fenêtre perd le focus, et une
            // barre ne l'a jamais : le verre dépoli passe par le flou de composition, actif en permanence.
            // Mica et Tabbed aussi : le fond système n'est plus qu'un secours si l'API manque.
            let glass = self.theme.backdrop != Backdrop::None && set_acrylic_blur(self.hwnd, true);
            if !glass {
                set_acrylic_blur(self.hwnd, false);
            }
            let kind = match self.theme.backdrop {
                _ if glass => DWMSBT_NONE,
                Backdrop::None => DWMSBT_NONE,
                Backdrop::Mica => DWMSBT_MAINWINDOW,
                Backdrop::Acrylic => DWMSBT_TRANSIENTWINDOW,
                Backdrop::Tabbed => DWMSBT_TABBEDWINDOW,
            };
            // Échoue sans conséquence sous Windows 10 : le fond teinté du thème reste.
            let _ = DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_SYSTEMBACKDROP_TYPE,
                &kind as *const _ as _,
                std::mem::size_of_val(&kind) as u32,
            );
            let corner =
                if self.screen.margin > 0 && self.theme.radius > 0.0 { DWMWCP_ROUND } else { DWMWCP_DONOTROUND };
            let _ = DwmSetWindowAttribute(
                self.hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as _,
                std::mem::size_of_val(&corner) as u32,
            );
        }
    }

    fn appbar_data(&self) -> APPBARDATA {
        APPBARDATA {
            cbSize: std::mem::size_of::<APPBARDATA>() as u32,
            hWnd: self.hwnd,
            uCallbackMessage: WM_APPBAR,
            uEdge: match self.screen.edge {
                Edge::Top => ABE_TOP,
                Edge::Bottom => ABE_BOTTOM,
                Edge::Left => ABE_LEFT,
                Edge::Right => ABE_RIGHT,
            },
            ..Default::default()
        }
    }

    /// Bande collée au bord `edge` de `rc`, de taille `size`.
    pub(super) fn band(edge: Edge, rc: RECT, size: i32) -> RECT {
        match edge {
            Edge::Top => RECT { bottom: rc.top + size, ..rc },
            Edge::Bottom => RECT { top: rc.bottom - size, ..rc },
            Edge::Left => RECT { right: rc.left + size, ..rc },
            Edge::Right => RECT { left: rc.right - size, ..rc },
        }
    }

    /// Calcule et applique la position de la barre.
    pub fn position(&mut self) {
        let s = self.scale();
        // Deux rangées ont besoin de place : au moins 52 pixels logiques.
        let rows = self.screen.layout.rows().len() as u32;
        let base = if rows > 1 && !self.screen.edge.is_vertical() {
            self.screen.thickness.max(52)
        } else {
            self.screen.thickness
        };
        let thick = (base as f32 * s).round() as i32;
        let m = (self.screen.margin as f32 * s).round() as i32;
        let size = thick + 2 * m;
        let edge = self.screen.edge;
        let mut rc = Self::band(edge, self.monitor, size);
        if self.screen.mode == Mode::Reserved {
            let mut abd = self.appbar_data();
            unsafe {
                if !self.registered {
                    SHAppBarMessage(ABM_NEW, &mut abd);
                    self.registered = true;
                }
                abd.rc = rc;
                SHAppBarMessage(ABM_QUERYPOS, &mut abd);
                // Windows a pu décaler la bande (autre barre sur le même bord) : on garde notre épaisseur.
                abd.rc = Self::band(edge, abd.rc, size);
                SHAppBarMessage(ABM_SETPOS, &mut abd);
            }
            rc = abd.rc;
        } else {
            self.unregister();
        }
        let mut r = RECT { left: rc.left + m, top: rc.top + m, right: rc.right - m, bottom: rc.bottom - m };
        if self.screen.mode == Mode::Overlay && self.screen.autohide && self.hidden {
            // Ne laisse dépasser que 2 pixels pour pouvoir rappeler la barre au survol.
            let peek = 2;
            let (w, h) = (r.right - r.left, r.bottom - r.top);
            match edge {
                Edge::Top => r = RECT { top: self.monitor.top - h + peek, bottom: self.monitor.top + peek, ..r },
                Edge::Bottom => {
                    r = RECT { top: self.monitor.bottom - peek, bottom: self.monitor.bottom - peek + h, ..r }
                }
                Edge::Left => r = RECT { left: self.monitor.left - w + peek, right: self.monitor.left + peek, ..r },
                Edge::Right => r = RECT { left: self.monitor.right - peek, right: self.monitor.right - peek + w, ..r },
            }
        }
        self.rect = r;
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                r.left,
                r.top,
                r.right - r.left,
                r.bottom - r.top,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        if let Some(rt) = &self.rt {
            let size = D2D_SIZE_U { width: (r.right - r.left) as u32, height: (r.bottom - r.top) as u32 };
            if unsafe { rt.Resize(&size) }.is_err() {
                self.rt = None;
            }
        }
    }

    fn unregister(&mut self) {
        if self.registered {
            let mut abd = self.appbar_data();
            unsafe { SHAppBarMessage(ABM_REMOVE, &mut abd) };
            self.registered = false;
        }
    }

    pub fn destroy(&mut self) {
        self.unregister();
        self.rt = None;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }

    /// Cellule d'un widget en coordonnées d'écran (pixels physiques).
    pub fn cell_on_screen(&self, id: &str) -> Option<RECT> {
        let s = self.scale();
        let (r, _) = self.hits.iter().find(|(_, i)| i == id)?;
        Some(RECT {
            left: self.rect.left + (r.left * s) as i32,
            top: self.rect.top + (r.top * s) as i32,
            right: self.rect.left + (r.right * s) as i32,
            bottom: self.rect.top + (r.bottom * s) as i32,
        })
    }

    /// Widget sous un point en pixels physiques relatifs à la fenêtre.
    pub fn hit(&self, x: i32, y: i32) -> Option<String> {
        let s = self.scale();
        let (x, y) = (x as f32 / s, y as f32 / s);
        self.hits
            .iter()
            .find(|(r, _)| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
            .map(|(_, id)| id.clone())
    }
}
