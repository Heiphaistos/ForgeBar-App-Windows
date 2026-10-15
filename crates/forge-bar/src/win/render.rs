//! Dessin d'une barre : fond, widgets, styles (jauge, anneau, mini-graphe), icônes SVG ou glyphes.
//! Le dessin cible n'importe quelle `ID2D1RenderTarget` : la fenêtre, ou une image pour les aperçus.

use super::apps::{MAX_APPS, SIZE as ICON_PX};
use super::bar::{Bar, Ctx, Fonts};
use crate::widgets::{self, AppWin, Inputs, Level, Segment, Series};
use forge_config::{BackgroundKind, Rgba, WidgetStyle};
use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN,
    D2D1_GRADIENT_STOP, D2D_POINT_2F, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Factory, ID2D1HwndRenderTarget, ID2D1RenderTarget, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE,
    D2D1_ARC_SIZE_SMALL, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, D2D1_BITMAP_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_GAMMA_2_2, D2D1_HWND_RENDER_TARGET_PROPERTIES,
    D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_PRESENT_OPTIONS_NONE, D2D1_RENDER_TARGET_PROPERTIES, D2D1_ROUNDED_RECT,
    D2D1_SWEEP_DIRECTION_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    IDWriteFactory, IDWriteTextFormat, IDWriteTextLayout, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER,
    DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

pub(super) fn color(c: Rgba) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.r, g: c.g, b: c.b, a: c.a }
}

pub(super) fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

pub(super) fn text_layout(
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

pub(super) fn text_format(
    dwrite: &IDWriteFactory,
    family: &str,
    size: f32,
    bold: bool,
    center: bool,
) -> Result<IDWriteTextFormat> {
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
        if center {
            f.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        }
    }
    Ok(f)
}

enum IconKind {
    Glyph(IDWriteTextLayout, f32),
    Svg(&'static str),
    None,
}

struct Item {
    seg: Segment,
    style: WidgetStyle,
    icon: IconKind,
    /// Libellé du haut en pile verticale (« CPU »), vide sinon.
    caption: String,
    text: Option<(IDWriteTextLayout, f32)>,
    width: f32,
}

/// Widgets sacrifiés, dans cet ordre, quand la barre est trop étroite pour tout afficher.
const DROP_ORDER: [&str; 9] = ["media", "apps", "weather", "net", "disk", "gpu", "clipboard", "ram", "cpu"];

fn zone_total(v: &[Item], gap: f32) -> f32 {
    v.iter().map(|i| i.width).sum::<f32>() + gap * v.len().saturating_sub(1) as f32
}

/// Début de chaque zone : la zone centrale reste centrée tant que possible, sinon elle glisse
/// pour ne pas chevaucher ses voisines. `None` si les trois zones ne tiennent pas côte à côte.
fn zone_starts(t: [f32; 3], length: f32, edge: f32, gap: f32) -> Option<[f32; 3]> {
    let right_start = length - edge - t[2];
    let lo = if t[0] > 0.0 { edge + t[0] + gap } else { edge };
    let hi = if t[2] > 0.0 { right_start - gap - t[1] } else { length - edge - t[1] };
    let center = ((length - t[1]) / 2.0).max(lo).min(hi);
    let fits = (t[1] == 0.0 || hi >= lo) && (t[0] == 0.0 || t[2] == 0.0 || edge + t[0] + gap <= right_start);
    fits.then_some([edge, center, right_start])
}

/// Retire des widgets (par priorité) tant que les trois zones d'une rangée ne tiennent pas.
fn fit(zones: &mut [Vec<Item>; 3], length: f32, gap: f32, edge: f32) {
    loop {
        let t = [zone_total(&zones[0], gap), zone_total(&zones[1], gap), zone_total(&zones[2], gap)];
        if zone_starts(t, length, edge, gap).is_some() {
            return;
        }
        let victim = DROP_ORDER.iter().find_map(|id| {
            zones.iter().enumerate().find_map(|(z, v)| v.iter().position(|i| i.seg.id == *id).map(|p| (z, p)))
        });
        match victim {
            Some((z, p)) => {
                zones[z].remove(p);
            }
            None => return,
        }
    }
}

const GAP_ICON_TEXT: f32 = 6.0;
const RING_D: f32 = 16.0;
const GRAPH_W: f32 = 44.0;
/// Largeur d'une miniature d'application et taille de son icône.
const APP_SLOT: f32 = 26.0;
const APP_ICON: f32 = 18.0;

/// Zone cliquable de la `i`-ième miniature dans la cellule du widget « apps ».
fn app_slot(cell: D2D_RECT_F, pad: f32, i: usize) -> D2D_RECT_F {
    let left = cell.left + pad + i as f32 * APP_SLOT;
    D2D_RECT_F { left, top: cell.top, right: left + APP_SLOT, bottom: cell.bottom }
}

impl Bar {
    pub(super) fn ensure_target(&mut self, d2d: &ID2D1Factory) -> Result<()> {
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

    pub(super) fn ensure_fonts(&mut self, dwrite: &IDWriteFactory, icon_font: &str) -> Result<()> {
        if self.fonts.is_some() {
            return Ok(());
        }
        let size = self.theme.font_size * 96.0 / 72.0;
        self.fonts = Some(Fonts {
            text: text_format(dwrite, &self.theme.font_family, size, false, false)?,
            icon: text_format(dwrite, icon_font, size + 1.0, false, true)?,
            small: text_format(dwrite, &self.theme.font_family, (size * 0.8).max(9.0), false, true)?,
        });
        Ok(())
    }

    fn fill_brush(&self, rt: &ID2D1RenderTarget, area: D2D_RECT_F) -> Result<ID2D1Brush> {
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

    /// Bitmaps Direct2D des icônes d'applications, recréés si la cible de dessin change
    /// (un bitmap n'est utilisable que sur la cible qui l'a créé).
    fn sync_bitmaps(&mut self, rt: &ID2D1RenderTarget, apps: &[AppWin]) {
        let raw = rt.as_raw() as usize;
        if self.bitmap_rt != raw {
            self.bitmaps.clear();
            self.bitmap_rt = raw;
        }
        self.bitmaps.retain(|h, _| apps.iter().any(|a| a.hwnd == *h));
        for a in apps {
            let Some((key, px)) = &a.icon else { continue };
            if self.bitmaps.get(&a.hwnd).is_some_and(|(k, _)| k == key) {
                continue;
            }
            let props = D2D1_BITMAP_PROPERTIES {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
            };
            let size = D2D_SIZE_U { width: ICON_PX as u32, height: ICON_PX as u32 };
            if let Ok(b) = unsafe { rt.CreateBitmap(size, Some(px.as_ptr() as *const _), size.width * 4, &props) } {
                self.bitmaps.insert(a.hwnd, (*key, b));
            }
        }
    }

    /// Dessin sur la fenêtre. Ne coûte rien entre deux changements de données.
    pub fn paint(&mut self, ctx: &Ctx) -> Result<()> {
        self.ensure_target(ctx.d2d)?;
        let hwnd_rt: ID2D1HwndRenderTarget = self.rt.clone().expect("cible créée");
        let rt: ID2D1RenderTarget = hwnd_rt.clone().into();
        let s = self.scale();
        let (w, h) = ((self.rect.right - self.rect.left) as f32 / s, (self.rect.bottom - self.rect.top) as f32 / s);
        unsafe {
            rt.BeginDraw();
            rt.Clear(Some(&color(Rgba::TRANSPARENT)));
        }
        let drawn = self.draw(&rt, ctx, w, h);
        let ended = unsafe { rt.EndDraw(None, None) };
        if drawn.is_err() || ended.is_err() {
            // Périphérique graphique perdu (pilote, veille) : on recrée la cible à la prochaine image.
            self.rt = None;
        }
        drawn
    }

    fn items(
        &mut self,
        ctx: &Ctx,
        ids: &[String],
        icons_only: bool,
        stack: bool,
        row_h: f32,
        pad: f32,
    ) -> Result<Vec<Item>> {
        self.ensure_fonts(ctx.dwrite, ctx.icon_font)?;
        let fonts = self.fonts.as_ref().expect("polices créées");
        let inputs = Inputs { sys: ctx.sys, claude: ctx.claude, cfg: ctx.claude_cfg, icons_only };
        let mut out = Vec::new();
        for id in ids {
            if let Some(cond) = self.conds.get(id) {
                if !widgets::visible(cond, ctx.sys, ctx.claude) {
                    continue;
                }
            }
            let Some(seg) = widgets::build(id, &inputs) else { continue };
            if seg.id == "apps" {
                // Rangée de miniatures : seulement sur une barre horizontale.
                if !self.screen.edge.is_vertical() {
                    let width = pad * 2.0 + ctx.sys.apps.len().min(MAX_APPS) as f32 * APP_SLOT;
                    let (style, icon, caption) = (WidgetStyle::Text, IconKind::None, String::new());
                    out.push(Item { seg, style, icon, caption, text: None, width });
                }
                continue;
            }
            let style = match self.screen.styles.get(id).copied().unwrap_or_default() {
                WidgetStyle::Gauge | WidgetStyle::Ring if seg.value.is_none() => WidgetStyle::Text,
                WidgetStyle::Graph if seg.series.is_none() => WidgetStyle::Text,
                st => st,
            };
            let svg = ctx.pack.has(seg.svg);
            // Une icône remplace le libellé (« 42 % » au lieu de « CPU 42 % »).
            let text = if svg || stack { seg.short.clone() } else { seg.text.clone() };
            // Colonne étroite : on ne garde que l'essentiel de chaque widget.
            let text = match (stack, seg.id.as_str()) {
                (true, "claude") => text.split(" · ").next().unwrap_or("").to_string(),
                (true, "net") => text.replace("/s", ""),
                _ => text,
            };
            let has_icon = svg || seg.icon.is_some();
            // Barre étroite : l'icône parle seule ; sans icône, Claude se réduit à son étoile.
            let text = match (icons_only, has_icon, seg.id.as_str()) {
                (true, true, _) => String::new(),
                (true, false, "claude") => "✦".to_string(),
                _ => text,
            };
            let icon = if svg {
                IconKind::Svg(seg.svg)
            } else if let Some(c) = seg.icon {
                let (l, _) = text_layout(ctx.dwrite, &c.to_string(), &fonts.icon, 18.0, row_h)?;
                IconKind::Glyph(l, 18.0)
            } else {
                IconKind::None
            };
            let caption = if stack && matches!(icon, IconKind::None) {
                match seg.id.as_str() {
                    "cpu" | "ram" | "gpu" => seg.id.to_uppercase(),
                    "net" => "RÉSEAU".into(),
                    _ => String::new(),
                }
            } else {
                String::new()
            };
            let layout = if text.is_empty() {
                None
            } else if stack {
                Some(text_layout(ctx.dwrite, &text, &fonts.small, 4000.0, 14.0)?)
            } else {
                Some(text_layout(ctx.dwrite, &text, &fonts.text, 4000.0, row_h)?)
            };
            let icon_w = match &icon {
                IconKind::Glyph(_, w) => *w,
                IconKind::Svg(_) => 18.0,
                IconKind::None => 0.0,
            };
            let tw = layout.as_ref().map_or(0.0, |(_, w)| *w);
            let extra = match style {
                WidgetStyle::Ring => RING_D + GAP_ICON_TEXT,
                WidgetStyle::Graph => GRAPH_W + GAP_ICON_TEXT,
                _ => 0.0,
            };
            let both = if icon_w > 0.0 && tw > 0.0 { GAP_ICON_TEXT } else { 0.0 };
            let width = pad * 2.0 + icon_w + both + extra + tw;
            out.push(Item { seg, style, icon, caption, text: layout, width });
        }
        Ok(out)
    }

    /// Dessine la barre dans `rt` (taille logique `w` x `h`), sans Begin/EndDraw.
    pub fn draw(&mut self, rt: &ID2D1RenderTarget, ctx: &Ctx, w: f32, h: f32) -> Result<()> {
        self.ensure_fonts(ctx.dwrite, ctx.icon_font)?;
        let vertical = self.screen.edge.is_vertical();
        let layout_cfg = self.screen.layout.clone();
        let mut rows = layout_cfg.rows();
        if vertical {
            // Un dock vertical n'a qu'une colonne : les rangées sont mises bout à bout.
            let mut all = forge_config::Zones::default();
            for r in rows.drain(..) {
                all.left.extend(r.left);
                all.center.extend(r.center);
                all.right.extend(r.right);
            }
            rows = vec![all];
        }
        // Pile verticale : assez large pour une icône au-dessus d'une valeur.
        let stack = vertical && w >= 56.0;
        let icons_only = (layout_cfg.icons_only() || vertical) && !stack;
        let pills = layout_cfg.pills();
        let pad = if pills { 10.0 } else { 4.0 };
        let gap = self.theme.spacing;
        let n_rows = rows.len() as f32;
        let row_h = if stack {
            46.0
        } else if vertical {
            (self.theme.font_size * 2.4).max(w * 0.8)
        } else {
            h / n_rows
        };
        let mut groups: Vec<[Vec<Item>; 3]> = Vec::new();
        for z in &rows {
            let (a, b, c) = (
                self.items(ctx, &z.left, icons_only, stack, row_h, pad)?,
                self.items(ctx, &z.center, icons_only, stack, row_h, pad)?,
                self.items(ctx, &z.right, icons_only, stack, row_h, pad)?,
            );
            groups.push([a, b, c]);
        }
        if !vertical {
            for z in groups.iter_mut() {
                fit(z, w, gap, 8.0);
            }
        }

        if groups.iter().flatten().flatten().any(|i| i.seg.id == "apps") {
            self.sync_bitmaps(rt, &ctx.sys.apps);
        }
        unsafe {
            let full = D2D_RECT_F { left: 0.0, top: 0.0, right: w, bottom: h };
            let radius = self.theme.radius;
            let bg = self.fill_brush(rt, full)?;
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
            for (ri, zones) in groups.iter().enumerate() {
                let y0 = if vertical { 0.0 } else { ri as f32 * row_h };
                let length = if vertical { h } else { w };
                let totals: [f32; 3] = std::array::from_fn(|z| {
                    let v = &zones[z];
                    let sum: f32 = v.iter().map(|it| if vertical { row_h } else { it.width }).sum();
                    sum + gap * v.len().saturating_sub(1) as f32
                });
                let starts = zone_starts(totals, length, edge_pad, gap).unwrap_or([
                    edge_pad,
                    (length - totals[1]) / 2.0,
                    length - edge_pad - totals[2],
                ]);
                for (zone, items) in zones.iter().enumerate() {
                    let extent = |it: &Item| if vertical { row_h } else { it.width };
                    let mut pos = starts[zone];
                    for it in items {
                        let cell = if vertical {
                            D2D_RECT_F { left: 0.0, top: pos, right: w, bottom: pos + row_h }
                        } else {
                            D2D_RECT_F { left: pos, top: y0, right: pos + it.width, bottom: y0 + row_h }
                        };
                        if pills {
                            let inset = (row_h * 0.12).max(2.0);
                            let pill = if vertical {
                                D2D_RECT_F {
                                    left: inset,
                                    top: cell.top + 2.0,
                                    right: w - inset,
                                    bottom: cell.bottom - 2.0,
                                }
                            } else {
                                D2D_RECT_F {
                                    left: cell.left,
                                    top: cell.top + inset,
                                    right: cell.right,
                                    bottom: cell.bottom - inset,
                                }
                            };
                            let r = radius.max((pill.bottom - pill.top) / 2.0).min((pill.bottom - pill.top) / 2.0);
                            rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: pill, radiusX: r, radiusY: r }, &bg);
                        }
                        self.draw_item(rt, ctx, it, cell, pad, stack)?;
                        if it.seg.id == "apps" {
                            for (i, a) in ctx.sys.apps.iter().take(MAX_APPS).enumerate() {
                                self.hits.push((app_slot(cell, pad, i), format!("app:{}", a.hwnd)));
                            }
                        } else {
                            self.hits.push((cell, it.seg.id.clone()));
                        }
                        pos += extent(it) + gap;
                    }
                }
            }
        }
        Ok(())
    }

    unsafe fn draw_item(
        &self,
        rt: &ID2D1RenderTarget,
        ctx: &Ctx,
        it: &Item,
        cell: D2D_RECT_F,
        pad: f32,
        stack: bool,
    ) -> Result<()> {
        let col = self.level_color(it.seg.level);
        let brush = rt.CreateSolidColorBrush(&color(col), None)?;
        let fonts = self.fonts.as_ref().expect("polices créées");
        let (cw, ch) = (cell.right - cell.left, cell.bottom - cell.top);
        let cy = cell.top + ch / 2.0;
        if it.seg.id == "apps" {
            let hi = rt.CreateSolidColorBrush(&color(self.theme.accent().with_alpha(0.3)), None)?;
            for (i, app) in ctx.sys.apps.iter().take(MAX_APPS).enumerate() {
                let slot = app_slot(cell, pad, i);
                let x = slot.left + (APP_SLOT - APP_ICON) / 2.0;
                if app.active {
                    let rect = D2D_RECT_F {
                        left: slot.left + 1.0,
                        top: cy - APP_SLOT / 2.0,
                        right: slot.right - 1.0,
                        bottom: cy + APP_SLOT / 2.0,
                    };
                    rt.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect, radiusX: 5.0, radiusY: 5.0 }, &hi);
                }
                if let Some((_, b)) = self.bitmaps.get(&app.hwnd) {
                    let dest = D2D_RECT_F {
                        left: x,
                        top: cy - APP_ICON / 2.0,
                        right: x + APP_ICON,
                        bottom: cy + APP_ICON / 2.0,
                    };
                    rt.DrawBitmap(b, Some(&dest), 1.0, D2D1_BITMAP_INTERPOLATION_MODE_LINEAR, None);
                } else {
                    // Pas d'icône exploitable : initiale de la fenêtre.
                    let letter: String = app.title.chars().next().into_iter().flat_map(char::to_uppercase).collect();
                    let (l, _) = text_layout(ctx.dwrite, &letter, &fonts.small, APP_SLOT, ch)?;
                    rt.DrawTextLayout(
                        D2D_POINT_2F { x: slot.left, y: cell.top },
                        &l,
                        &brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
            }
            return Ok(());
        }
        if stack {
            // Légende (ou icône) en haut, valeur dessous, le tout centré dans la barre.
            match &it.icon {
                IconKind::Glyph(l, _) => {
                    rt.DrawTextLayout(
                        D2D_POINT_2F { x: cell.left + (cw - 18.0) / 2.0, y: cell.top - 9.0 },
                        l,
                        &brush,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
                IconKind::Svg(name) => ctx.pack.draw(
                    rt,
                    name,
                    cell.left + (cw - 18.0) / 2.0,
                    cell.top + 5.0,
                    18.0,
                    &brush.cast::<ID2D1Brush>()?,
                ),
                IconKind::None if !it.caption.is_empty() => {
                    let dim = rt.CreateSolidColorBrush(&color(self.theme.text_secondary()), None)?;
                    let (l, _) = text_layout(ctx.dwrite, &it.caption, &fonts.small, cw, 14.0)?;
                    rt.DrawTextLayout(
                        D2D_POINT_2F { x: cell.left, y: cell.top + 4.0 },
                        &l,
                        &dim,
                        D2D1_DRAW_TEXT_OPTIONS_NONE,
                    );
                }
                IconKind::None => {}
            }
            if it.text.is_some() {
                let (l, _) = text_layout(ctx.dwrite, &it.seg.short, &fonts.small, cw, 14.0)?;
                rt.DrawTextLayout(
                    D2D_POINT_2F { x: cell.left, y: cell.bottom - 20.0 },
                    &l,
                    &brush,
                    D2D1_DRAW_TEXT_OPTIONS_NONE,
                );
            }
            if let (WidgetStyle::Gauge | WidgetStyle::Ring, Some(v)) = (it.style, it.seg.value) {
                self.gauge(rt, col, cell.left + 8.0, cell.bottom - 4.0, cw - 16.0, v)?;
            }
            return Ok(());
        }

        let mut x = cell.left + pad;
        match &it.icon {
            IconKind::Glyph(l, iw) => {
                rt.DrawTextLayout(D2D_POINT_2F { x, y: cell.top }, l, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
                x += iw + GAP_ICON_TEXT;
            }
            IconKind::Svg(name) => {
                ctx.pack.draw(rt, name, x, cy - 9.0, 18.0, &brush.cast::<ID2D1Brush>()?);
                x += 18.0 + GAP_ICON_TEXT;
            }
            IconKind::None => {}
        }
        match (it.style, it.seg.value, it.seg.series) {
            (WidgetStyle::Ring, Some(v), _) => {
                self.ring(rt, ctx.d2d, col, x + RING_D / 2.0, cy, RING_D / 2.0 - 1.5, v)?;
                x += RING_D + GAP_ICON_TEXT;
            }
            (WidgetStyle::Graph, _, Some(series)) => {
                self.graph(rt, ctx, col, series, x, cy - ch * 0.2, GRAPH_W, ch * 0.4)?;
                x += GRAPH_W + GAP_ICON_TEXT;
            }
            _ => {}
        }
        if let Some((l, tw)) = &it.text {
            rt.DrawTextLayout(D2D_POINT_2F { x, y: cell.top }, l, &brush, D2D1_DRAW_TEXT_OPTIONS_NONE);
            if let (WidgetStyle::Gauge, Some(v)) = (it.style, it.seg.value) {
                self.gauge(rt, col, x, cell.bottom - (ch * 0.18).max(4.0), tw.max(24.0), v)?;
            }
        }
        Ok(())
    }

    /// Jauge fine : piste atténuée et remplissage proportionnel.
    unsafe fn gauge(&self, rt: &ID2D1RenderTarget, c: Rgba, x: f32, y: f32, w: f32, pct: f32) -> Result<()> {
        let track = rt.CreateSolidColorBrush(&color(self.theme.text_secondary().with_alpha(0.25)), None)?;
        let bar = rt.CreateSolidColorBrush(&color(c), None)?;
        let r = |w: f32| D2D1_ROUNDED_RECT {
            rect: D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + 2.5 },
            radiusX: 1.25,
            radiusY: 1.25,
        };
        rt.FillRoundedRectangle(&r(w), &track);
        let filled = (pct / 100.0).clamp(0.0, 1.0) * w;
        if filled > 0.5 {
            rt.FillRoundedRectangle(&r(filled), &bar);
        }
        Ok(())
    }

    /// Anneau de progression qui démarre en haut et tourne dans le sens horaire.
    #[allow(clippy::too_many_arguments)]
    unsafe fn ring(
        &self,
        rt: &ID2D1RenderTarget,
        d2d: &ID2D1Factory,
        c: Rgba,
        cx: f32,
        cy: f32,
        r: f32,
        pct: f32,
    ) -> Result<()> {
        let track = rt.CreateSolidColorBrush(&color(self.theme.text_secondary().with_alpha(0.25)), None)?;
        let bar = rt.CreateSolidColorBrush(&color(c), None)?;
        let center = D2D_POINT_2F { x: cx, y: cy };
        rt.DrawEllipse(&D2D1_ELLIPSE { point: center, radiusX: r, radiusY: r }, &track, 2.2, None);
        let frac = (pct / 100.0).clamp(0.0, 1.0);
        if frac >= 0.995 {
            rt.DrawEllipse(&D2D1_ELLIPSE { point: center, radiusX: r, radiusY: r }, &bar, 2.2, None);
        } else if frac > 0.01 {
            let ang = frac * std::f32::consts::TAU;
            let end = D2D_POINT_2F { x: cx + r * ang.sin(), y: cy - r * ang.cos() };
            let g = d2d.CreatePathGeometry()?;
            let sink = g.Open()?;
            sink.BeginFigure(D2D_POINT_2F { x: cx, y: cy - r }, D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: end,
                size: D2D_SIZE_F { width: r, height: r },
                rotationAngle: 0.0,
                sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                arcSize: if frac > 0.5 { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close()?;
            rt.DrawGeometry(&g, &bar, 2.2, None);
        }
        Ok(())
    }

    /// Mini-graphe des dernières mesures, avec une aire légère sous la courbe.
    #[allow(clippy::too_many_arguments)]
    unsafe fn graph(
        &self,
        rt: &ID2D1RenderTarget,
        ctx: &Ctx,
        c: Rgba,
        series: Series,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> Result<()> {
        let data = ctx.sys.hist.get(series);
        let n = data.len().min(GRAPH_W as usize);
        if n < 2 {
            return Ok(());
        }
        let pts: Vec<f32> = data.iter().skip(data.len() - n).copied().collect();
        let top = if series == Series::Net { pts.iter().copied().fold(100_000.0_f32, f32::max) } else { 100.0 };
        let at = |i: usize, v: f32| D2D_POINT_2F {
            x: x + i as f32 * w / (n - 1) as f32,
            y: y + h - (v / top).clamp(0.0, 1.0) * h,
        };
        let g = ctx.d2d.CreatePathGeometry()?;
        let sink = g.Open()?;
        sink.BeginFigure(D2D_POINT_2F { x, y: y + h }, D2D1_FIGURE_BEGIN_FILLED);
        for (i, v) in pts.iter().enumerate() {
            sink.AddLine(at(i, *v));
        }
        sink.AddLine(D2D_POINT_2F { x: x + w, y: y + h });
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        let area = rt.CreateSolidColorBrush(&color(c.with_alpha(0.22)), None)?;
        rt.FillGeometry(&g, &area, None);
        let line = rt.CreateSolidColorBrush(&color(c), None)?;
        for i in 1..n {
            rt.DrawLine(at(i - 1, pts[i - 1]), at(i, pts[i]), &line, 1.4, None);
        }
        Ok(())
    }
}
