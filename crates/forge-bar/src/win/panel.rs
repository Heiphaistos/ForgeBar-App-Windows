//! Panneaux : fenêtre contextuelle (calendrier, détail Claude, presse-papiers) et HUD flottant.
//! Le contenu vient de `content` (liste d'opérations) ; ici on ne fait que le dessiner et le placer.

use super::render::{color, text_format};
use crate::content::{Action, Align, Op, Role, Scene, R};
use forge_config::{Corner, Edge, Rgba, Theme};
use std::collections::HashMap;
use windows::core::{w, Interface, Result};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteTextFormat, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING,
};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SetForegroundWindow, SetLayeredWindowAttributes, SetWindowPos, HWND_TOPMOST,
    LWA_ALPHA, SWP_NOACTIVATE, SWP_SHOWWINDOW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Calendar,
    Claude,
    Clipboard,
    Floating,
}

pub struct Panel {
    pub hwnd: HWND,
    pub kind: Kind,
    pub scene: Scene,
    theme: Theme,
    dpi: u32,
    opacity: f32,
    solid: bool,
    rt: Option<ID2D1HwndRenderTarget>,
    fmt: HashMap<(u32, bool, u8), IDWriteTextFormat>,
    /// Mois affiché par le calendrier.
    pub month: (i32, u32),
}

pub fn role_color(theme: &Theme, role: Role) -> Rgba {
    match role {
        Role::Text => theme.text(),
        Role::Dim => theme.text_secondary(),
        Role::Accent => theme.accent(),
        Role::Warn => theme.warning(),
        Role::Crit => theme.critical(),
        Role::Track => theme.text_secondary().with_alpha(0.18),
    }
}

/// Fond d'un panneau : la première couleur du thème, rendue assez opaque pour rester lisible.
pub fn panel_background(theme: &Theme) -> Rgba {
    let c = theme.background_colors()[0];
    c.with_alpha(c.a.max(0.96))
}

/// Dessine `scene` dans `rt` ; `solid` remplit un rectangle plein (HUD flottant) plutôt qu'un rectangle arrondi.
#[allow(clippy::too_many_arguments)]
pub fn render_scene(
    rt: &ID2D1RenderTarget,
    dwrite: &IDWriteFactory,
    theme: &Theme,
    scene: &Scene,
    opacity: f32,
    solid: bool,
    cache: &mut HashMap<(u32, bool, u8), IDWriteTextFormat>,
) -> Result<()> {
    unsafe {
        let bg = rt.CreateSolidColorBrush(&color(panel_background(theme).with_alpha(opacity.min(1.0))), None)?;
        let full = D2D_RECT_F { left: 0.0, top: 0.0, right: scene.w, bottom: scene.h };
        let radius = if solid { 0.0 } else { 10.0 };
        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: full, radiusX: radius, radiusY: radius }, &bg);
        let border = rt.CreateSolidColorBrush(&color(theme.text_secondary().with_alpha(0.3)), None)?;
        let inner = D2D_RECT_F { left: 0.5, top: 0.5, right: scene.w - 0.5, bottom: scene.h - 0.5 };
        rt.DrawRoundedRectangle(
            &D2D1_ROUNDED_RECT { rect: inner, radiusX: radius, radiusY: radius },
            &border,
            1.0,
            None,
        );
        for op in &scene.ops {
            match op {
                Op::Fill { r, role, radius } => {
                    let b = rt.CreateSolidColorBrush(&color(role_color(theme, *role)), None)?;
                    rt.FillRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: rect(*r), radiusX: *radius, radiusY: *radius },
                        &b,
                    );
                }
                Op::Text { r, s, role, size, bold, align } => {
                    let key = ((size * 10.0) as u32, *bold, *align as u8);
                    if let std::collections::hash_map::Entry::Vacant(slot) = cache.entry(key) {
                        let f = text_format(dwrite, &theme.font_family, *size * 96.0 / 72.0 * 0.9, *bold, false)?;
                        f.SetTextAlignment(match align {
                            Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                            Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                            Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
                        })?;
                        slot.insert(f);
                    }
                    let b = rt.CreateSolidColorBrush(&color(role_color(theme, *role)), None)?;
                    let wide: Vec<u16> = s.encode_utf16().collect();
                    rt.DrawText(&wide, &cache[&key], &rect(*r), &b, D2D1_DRAW_TEXT_OPTIONS_NONE, Default::default());
                }
                Op::Hit { .. } => {}
            }
        }
    }
    Ok(())
}

fn rect(r: R) -> D2D_RECT_F {
    D2D_RECT_F { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h }
}

impl Panel {
    fn create(
        ex_style: windows::Win32::UI::WindowsAndMessaging::WINDOW_EX_STYLE,
        theme: Theme,
        scene: Scene,
        kind: Kind,
        dpi: u32,
        opacity: f32,
    ) -> Option<Self> {
        let hinstance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None).ok()? };
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                w!("ForgeBarPanel"),
                w!("Forge Bar"),
                WS_POPUP,
                0,
                0,
                10,
                10,
                None,
                None,
                hinstance,
                None,
            )
        }
        .ok()?;
        Some(Self {
            hwnd,
            kind,
            scene,
            theme,
            dpi,
            opacity,
            solid: kind == Kind::Floating,
            rt: None,
            fmt: HashMap::new(),
            month: (0, 0),
        })
    }

    fn px(&self, v: f32) -> i32 {
        (v * self.dpi as f32 / 96.0).round() as i32
    }

    /// Fenêtre contextuelle ancrée sous (ou au-dessus de, selon le bord) la cellule cliquée.
    pub fn popup(
        kind: Kind,
        scene: Scene,
        theme: Theme,
        anchor: RECT,
        edge: Edge,
        monitor: RECT,
        dpi: u32,
    ) -> Option<Self> {
        let mut p = Self::create(WS_EX_TOOLWINDOW | WS_EX_TOPMOST, theme, scene, kind, dpi, 1.0)?;
        unsafe {
            let m = MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 };
            let _ = DwmExtendFrameIntoClientArea(p.hwnd, &m);
            let corner = DWMWCP_ROUND;
            let _ = DwmSetWindowAttribute(
                p.hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &corner as *const _ as _,
                std::mem::size_of_val(&corner) as u32,
            );
        }
        p.place_near(anchor, edge, monitor);
        unsafe {
            let _ = SetForegroundWindow(p.hwnd);
        }
        Some(p)
    }

    fn place_near(&mut self, anchor: RECT, edge: Edge, monitor: RECT) {
        let (w, h) = (self.px(self.scene.w), self.px(self.scene.h));
        let gap = self.px(6.0);
        let m = self.px(8.0);
        let (cx, cy) = ((anchor.left + anchor.right) / 2, (anchor.top + anchor.bottom) / 2);
        let (mut x, mut y) = match edge {
            Edge::Top => (cx - w / 2, anchor.bottom + gap),
            Edge::Bottom => (cx - w / 2, anchor.top - h - gap),
            Edge::Left => (anchor.right + gap, cy - h / 2),
            Edge::Right => (anchor.left - w - gap, cy - h / 2),
        };
        x = x.clamp(monitor.left + m, (monitor.right - w - m).max(monitor.left + m));
        y = y.clamp(monitor.top + m, (monitor.bottom - h - m).max(monitor.top + m));
        self.set_rect(x, y, w, h);
    }

    #[allow(clippy::too_many_arguments)]
    /// HUD flottant, toujours visible, éventuellement transparent aux clics.
    pub fn floating(
        scene: Scene,
        theme: Theme,
        corner: Corner,
        margin: u32,
        monitor: RECT,
        dpi: u32,
        opacity: f32,
        click_through: bool,
    ) -> Option<Self> {
        let mut style = WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_LAYERED;
        if click_through {
            style |= WS_EX_TRANSPARENT;
        }
        let mut p = Self::create(style, theme, scene, Kind::Floating, dpi, 1.0)?;
        unsafe {
            let _ = SetLayeredWindowAttributes(
                p.hwnd,
                windows::Win32::Foundation::COLORREF(0),
                (opacity.clamp(0.1, 1.0) * 255.0) as u8,
                LWA_ALPHA,
            );
        }
        p.opacity = 1.0;
        p.place_corner(corner, margin, monitor);
        Some(p)
    }

    fn place_corner(&mut self, corner: Corner, margin: u32, monitor: RECT) {
        let (w, h, m) = (self.px(self.scene.w), self.px(self.scene.h), self.px(margin as f32));
        let x = match corner {
            Corner::TopLeft | Corner::BottomLeft => monitor.left + m,
            _ => monitor.right - w - m,
        };
        let y = match corner {
            Corner::TopLeft | Corner::TopRight => monitor.top + m,
            _ => monitor.bottom - h - m,
        };
        self.set_rect(x, y, w, h);
    }

    fn set_rect(&mut self, x: i32, y: i32, w: i32, h: i32) {
        unsafe {
            let _ = SetWindowPos(self.hwnd, HWND_TOPMOST, x, y, w, h, SWP_NOACTIVATE | SWP_SHOWWINDOW);
        }
        if let Some(rt) = &self.rt {
            if unsafe { rt.Resize(&D2D_SIZE_U { width: w as u32, height: h as u32 }) }.is_err() {
                self.rt = None;
            }
        }
    }

    /// Remplace le contenu ; redimensionne la fenêtre si la hauteur change, en gardant son coin haut-gauche
    /// (ou son ancrage pour le HUD, repositionné par l'appelant).
    pub fn set_scene(&mut self, scene: Scene) {
        let changed = (scene.w, scene.h) != (self.scene.w, self.scene.h);
        self.scene = scene;
        if changed {
            let mut r = RECT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut r);
            }
            self.set_rect(r.left, r.top, self.px(self.scene.w), self.px(self.scene.h));
        }
    }

    pub fn reposition_floating(&mut self, corner: Corner, margin: u32, monitor: RECT) {
        self.place_corner(corner, margin, monitor);
    }

    pub fn paint(&mut self, d2d: &ID2D1Factory, dwrite: &IDWriteFactory) -> Result<()> {
        if self.rt.is_none() {
            let mut r = RECT::default();
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(self.hwnd, &mut r);
            }
            let props = D2D1_RENDER_TARGET_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                ..Default::default()
            };
            let hp = D2D1_HWND_RENDER_TARGET_PROPERTIES {
                hwnd: self.hwnd,
                pixelSize: D2D_SIZE_U {
                    width: (r.right - r.left).max(1) as u32,
                    height: (r.bottom - r.top).max(1) as u32,
                },
                presentOptions: D2D1_PRESENT_OPTIONS_NONE,
            };
            let rt = unsafe { d2d.CreateHwndRenderTarget(&props, &hp)? };
            unsafe { rt.SetDpi(self.dpi as f32, self.dpi as f32) };
            self.rt = Some(rt);
        }
        let hwnd_rt = self.rt.clone().expect("cible créée");
        let rt: ID2D1RenderTarget = hwnd_rt.cast()?;
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&color(Rgba::TRANSPARENT)));
        }
        let drawn = render_scene(&rt, dwrite, &self.theme, &self.scene, self.opacity, self.solid, &mut self.fmt);
        let ended = unsafe { rt.EndDraw(None, None) };
        if drawn.is_err() || ended.is_err() {
            self.rt = None;
        }
        drawn
    }

    /// Action sous un point en pixels physiques relatifs à la fenêtre.
    pub fn hit(&self, x: i32, y: i32) -> Option<Action> {
        let s = self.dpi as f32 / 96.0;
        let (x, y) = (x as f32 / s, y as f32 / s);
        self.scene.ops.iter().find_map(|op| match op {
            Op::Hit { r, action } if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h => Some(*action),
            _ => None,
        })
    }

    pub fn destroy(&mut self) {
        self.rt = None;
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
