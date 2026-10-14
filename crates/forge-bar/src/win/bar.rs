//! Une barre = une fenêtre sur un écran. Positionnement par l'API AppBar
//! (espace réservé) ou en superposition, rendu Direct2D à la demande.

use crate::widgets::{self, Level, Segment, SysData};
use forge_claude::Summary;
use forge_config::{Backdrop, BackgroundKind, ClaudeConfig, Edge, Mode, Rgba, ScreenConfig, Theme};
use windows::core::{Result, HSTRING};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_POINT_2F, D2D_RECT_F,
    D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Factory, ID2D1HwndRenderTarget, D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_HWND_RENDER_TARGET_PROPERTIES, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW,
    DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DWMWCP_ROUND,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{
    SHAppBarMessage, ABE_BOTTOM, ABE_LEFT, ABE_RIGHT, ABE_TOP, ABM_NEW, ABM_QUERYPOS, ABM_REMOVE, ABM_SETPOS,
    APPBARDATA,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
};

pub const WM_APPBAR: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 1;

/// Ce dont le rendu a besoin, prêté par l'application à chaque image.
pub struct Ctx<'a> {
    pub d2d: &'a ID2D1Factory,
    pub dwrite: &'a IDWriteFactory,
    pub icon_font: &'a str,
    pub sys: &'a SysData,
    pub claude: &'a Summary,
    pub claude_cfg: &'a ClaudeConfig,
}

pub struct Bar {
    pub hwnd: HWND,
    pub screen: ScreenConfig,
    pub theme: Theme,
    pub monitor: RECT,
    pub dpi: u32,
    pub hidden: bool,
    pub claude_expanded: bool,
    pub rect: RECT,
    registered: bool,
    rt: Option<ID2D1HwndRenderTarget>,
    fonts: Option<(IDWriteTextFormat, IDWriteTextFormat)>,
    /// Zones cliquables de la dernière image, en pixels logiques.
    pub hits: Vec<(D2D_RECT_F, String)>,
}

fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.r, g: c.g, b: c.b, a: c.a }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
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
            claude_expanded: false,
            rect: RECT::default(),
            registered: false,
            rt: None,
            fonts: None,
            hits: Vec::new(),
        };
        bar.apply_backdrop();
        bar
    }

    fn scale(&self) -> f32 {
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
            let kind = match self.theme.backdrop {
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
    fn band(edge: Edge, rc: RECT, size: i32) -> RECT {
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
        let thick = (self.screen.thickness as f32 * s).round() as i32;
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

    fn ensure_target(&mut self, d2d: &ID2D1Factory) -> Result<()> {
        if self.rt.is_some() {
            return Ok(());
        }
        let props = D2D1_RENDER_TARGET_PROPERTIES {
            pixelFormat: D2D1_PIXEL_FORMAT {
                format: DXGI_FORMAT_B8G8R8A8_UNORM,
                alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
            },
            ..Default::default()
        };
        let hwnd_props = D2D1_HWND_RENDER_TARGET_PROPERTIES {
            hwnd: self.hwnd,
            pixelSize: D2D_SIZE_U {
                width: (self.rect.right - self.rect.left).max(1) as u32,
                height: (self.rect.bottom - self.rect.top).max(1) as u32,
            },
            presentOptions: D2D1_PRESENT_OPTIONS_NONE,
        };
        let rt = unsafe { d2d.CreateHwndRenderTarget(&props, &hwnd_props)? };
        unsafe { rt.SetDpi(self.dpi as f32, self.dpi as f32) };
        self.rt = Some(rt);
        Ok(())
    }

    fn ensure_fonts(&mut self, dwrite: &IDWriteFactory, icon_font: &str) -> Result<()> {
        if self.fonts.is_some() {
            return Ok(());
        }
        let size = self.theme.font_size * 96.0 / 72.0;
        let make = |family: &str, size: f32, bold: bool| -> Result<IDWriteTextFormat> {
            let f = unsafe {
                dwrite.CreateTextFormat(
                    &HSTRING::from(family),
                    None,
                    if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL },
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    &HSTRING::from("fr-FR"),
                )?
            };
            unsafe {
                f.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
                f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP)?;
            }
            Ok(f)
        };
        let text = make(&self.theme.font_family, size, false)?;
        let icon = make(icon_font, size + 1.0, false)?;
        unsafe { icon.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)? };
        self.fonts = Some((text, icon));
        Ok(())
    }

    fn fill_brush(&self, rt: &ID2D1HwndRenderTarget, area: D2D_RECT_F) -> Result<ID2D1Brush> {
        let cols = self.theme.background_colors();
        unsafe {
            if self.theme.background.kind == BackgroundKind::Solid || cols.len() < 2 {
                return Ok(rt.CreateSolidColorBrush(&color(cols[0]), None)?.into());
            }
            let n = (cols.len() - 1) as f32;
            let stops: Vec<D2D1_GRADIENT_STOP> = cols
                .iter()
                .enumerate()
                .map(|(i, c)| D2D1_GRADIENT_STOP { position: i as f32 / n, color: color(*c) })
                .collect();
            let coll = rt.CreateGradientStopCollection(&stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP)?;
            // Le dégradé suit la longueur de la barre, quelle que soit son orientation.
            let mut angle = self.theme.background.angle.to_radians();
            if self.screen.edge.is_vertical() {
                angle += std::f32::consts::FRAC_PI_2;
            }
            let (w, h) = (area.right - area.left, area.bottom - area.top);
            let (cx, cy) = (area.left + w / 2.0, area.top + h / 2.0);
            let half = (w * angle.cos().abs() + h * angle.sin().abs()) / 2.0;
            let props = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: D2D_POINT_2F { x: cx - half * angle.cos(), y: cy - half * angle.sin() },
                endPoint: D2D_POINT_2F { x: cx + half * angle.cos(), y: cy + half * angle.sin() },
            };
            Ok(rt.CreateLinearGradientBrush(&props, None, &coll)?.into())
        }
    }

    fn level_color(&self, l: Level) -> Rgba {
        match l {
            Level::Normal => self.theme.text(),
            Level::Secondary => self.theme.text_secondary(),
            Level::Accent => self.theme.accent(),
            Level::Warning => self.theme.warning(),
            Level::Critical => self.theme.critical(),
        }
    }

    fn layout(
        dwrite: &IDWriteFactory,
        text: &str,
        fmt: &IDWriteTextFormat,
        w: f32,
        h: f32,
    ) -> Result<(IDWriteTextLayout, f32)> {
        let layout = unsafe { dwrite.CreateTextLayout(&wide(text), fmt, w, h)? };
        let mut m = DWRITE_TEXT_METRICS::default();
        unsafe { layout.GetMetrics(&mut m)? };
        Ok((layout, m.widthIncludingTrailingWhitespace))
    }

    /// Dessine une image complète. Ne coûte rien entre deux changements de données.
    pub fn paint(&mut self, ctx: &Ctx) -> Result<()> {
        self.ensure_target(ctx.d2d)?;
        self.ensure_fonts(ctx.dwrite, ctx.icon_font)?;
        let rt = self.rt.clone().unwrap();
        let (text_fmt, icon_fmt) = self.fonts.clone().unwrap();
        let s = self.scale();
        let w = (self.rect.right - self.rect.left) as f32 / s;
        let h = (self.rect.bottom - self.rect.top) as f32 / s;
        let vertical = self.screen.edge.is_vertical();
        let layout_cfg = &self.screen.layout;
        let zones = layout_cfg.zones();
        let icons_only = layout_cfg.icons_only() || vertical;
        let pills = layout_cfg.pills();
        let pad = if pills { 10.0 } else { 4.0 };
        let gap = self.theme.spacing;
        let row_h = if vertical { (self.theme.font_size * 2.4).max(w * 0.8) } else { h };

        struct Item {
            id: String,
            level: Level,
            icon: Option<(IDWriteTextLayout, f32)>,
            text: Option<(IDWriteTextLayout, f32)>,
            width: f32,
        }
        let build = |ids: &[String]| -> Result<Vec<Item>> {
            let mut out = Vec::new();
            for id in ids {
                let Some(Segment { id, icon, text, level }) =
                    widgets::build(id, ctx.sys, ctx.claude, ctx.claude_cfg, icons_only, self.claude_expanded)
                else {
                    continue;
                };
                // En vertical, un widget sans icône garde son texte.
                let text = if vertical && icon.is_some() { String::new() } else { text };
                let icon_l = match icon {
                    Some(c) => Some(Self::layout(ctx.dwrite, &c.to_string(), &icon_fmt, 64.0, row_h)?),
                    None => None,
                };
                let text_l = if text.is_empty() {
                    None
                } else {
                    Some(Self::layout(ctx.dwrite, &text, &text_fmt, 4000.0, row_h)?)
                };
                let iw = icon_l.as_ref().map_or(0.0, |(_, w)| w.max(16.0));
                let tw = text_l.as_ref().map_or(0.0, |(_, w)| *w);
                let both = if icon_l.is_some() && text_l.is_some() { 6.0 } else { 0.0 };
                out.push(Item { id, level, icon: icon_l, text: text_l, width: pad * 2.0 + iw + both + tw });
            }
            Ok(out)
        };
        let groups = [build(&zones.left)?, build(&zones.center)?, build(&zones.right)?];

        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&color(Rgba::TRANSPARENT)));
            let full = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };
            let radius = self.theme.radius;
            let bg = self.fill_brush(&rt, full)?;
            if !pills {
                rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: full, radiusX: radius, radiusY: radius }, &bg);
                if let (Some(b), true) = (self.theme.border(), self.theme.border_width > 0.0) {
                    let brush = rt.CreateSolidColorBrush(&color(b), None)?;
                    let bw = self.theme.border_width;
                    let inner = D2D_RECT_F { left: bw / 2.0, top: bw / 2.0, right: w - bw / 2.0, bottom: h - bw / 2.0 };
                    rt.DrawRoundedRectangle(
                        &D2D1_ROUNDED_RECT { rect: inner, radiusX: radius, radiusY: radius },
                        &brush,
                        bw,
                        None,
                    );
                }
            }
            self.hits.clear();
            let edge_pad = 8.0;
            for (zone, items) in groups.iter().enumerate() {
                let extent = |it: &Item| if vertical { row_h } else { it.width };
                let total: f32 = items.iter().map(extent).sum::<f32>() + gap * items.len().saturating_sub(1) as f32;
                let length = if vertical { h } else { w };
                let mut pos = match zone {
                    0 => edge_pad,
                    1 => (length - total) / 2.0,
                    _ => length - edge_pad - total,
                };
                for it in items {
                    let cell = if vertical {
                        D2D_RECT_F { left: 0.0, top: pos, right: w, bottom: pos + row_h }
                    } else {
                        D2D_RECT_F { left: pos, top: 0.0, right: pos + it.width, bottom: h }
                    };
                    if pills {
                        let inset = (row_h * 0.12).max(2.0);
                        let pill = if vertical {
                            D2D_RECT_F { left: inset, top: cell.top + 2.0, right: w - inset, bottom: cell.bottom - 2.0 }
                        } else {
                            D2D_RECT_F { left: cell.left, top: inset, right: cell.right, bottom: h - inset }
                        };
                        let r = radius.max((pill.bottom - pill.top) / 2.0).min((pill.bottom - pill.top) / 2.0);
                        rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: pill, radiusX: r, radiusY: r }, &bg);
                    }
                    let brush = rt.CreateSolidColorBrush(&color(self.level_color(it.level)), None)?;
                    let mut x = cell.left + pad;
                    if vertical {
                        let content =
                            it.icon.as_ref().map_or_else(|| it.text.as_ref().map_or(0.0, |t| t.1), |i| i.1.max(16.0));
                        x = (w - content) / 2.0;
                    }
                    if let Some((l, iw)) = &it.icon {
                        rt.DrawTextLayout(D2D_POINT_2F { x, y: cell.top }, l, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                        x += iw.max(16.0) + 6.0;
                    }
                    if let Some((l, _)) = &it.text {
                        rt.DrawTextLayout(D2D_POINT_2F { x, y: cell.top }, l, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                    }
                    self.hits.push((cell, it.id.clone()));
                    pos += extent(it) + gap;
                }
            }
            if rt.EndDraw(None, None).is_err() {
                // Périphérique graphique perdu (pilote, veille) : on recrée la cible à la prochaine image.
                self.rt = None;
            }
        }
        Ok(())
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
