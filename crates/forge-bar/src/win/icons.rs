//! Packs d'icônes SVG : lecture, géométries Direct2D et dessin.
//!
//! Un pack est un dossier `%APPDATA%\ForgeBar\icons\<nom>\` contenant `<widget>.svg`
//! (desktop, wifi, volume, mute, battery, clock, cpu, ram, gpu, net, weather, media, clipboard, claude),
//! ou le pack intégré `forge`. Un widget sans fichier garde son glyphe Segoe.

use crate::svg::{self, Cmd, Icon};
use std::collections::HashMap;
use windows::Foundation::Numerics::Matrix3x2;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_BEZIER_SEGMENT, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED,
    D2D1_FIGURE_END_OPEN, D2D_POINT_2F,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Brush, ID2D1Factory, ID2D1PathGeometry, ID2D1RenderTarget, ID2D1StrokeStyle, D2D1_CAP_STYLE_ROUND,
    D2D1_LINE_JOIN_ROUND, D2D1_STROKE_STYLE_PROPERTIES,
};

const BUILTIN: &[(&str, &str)] = &[
    ("desktop", include_str!("../../../../assets/icons/forge/desktop.svg")),
    ("wifi", include_str!("../../../../assets/icons/forge/wifi.svg")),
    ("volume", include_str!("../../../../assets/icons/forge/volume.svg")),
    ("mute", include_str!("../../../../assets/icons/forge/mute.svg")),
    ("battery", include_str!("../../../../assets/icons/forge/battery.svg")),
    ("clock", include_str!("../../../../assets/icons/forge/clock.svg")),
    ("cpu", include_str!("../../../../assets/icons/forge/cpu.svg")),
    ("ram", include_str!("../../../../assets/icons/forge/ram.svg")),
    ("gpu", include_str!("../../../../assets/icons/forge/gpu.svg")),
    ("disk", include_str!("../../../../assets/icons/forge/disk.svg")),
    ("net", include_str!("../../../../assets/icons/forge/net.svg")),
    ("weather", include_str!("../../../../assets/icons/forge/weather.svg")),
    ("media", include_str!("../../../../assets/icons/forge/media.svg")),
    ("clipboard", include_str!("../../../../assets/icons/forge/clipboard.svg")),
    ("claude", include_str!("../../../../assets/icons/forge/claude.svg")),
];

/// Noms que l'application cherche dans un pack.
pub const NAMES: &[&str] = &[
    "desktop",
    "wifi",
    "volume",
    "mute",
    "battery",
    "clock",
    "cpu",
    "ram",
    "gpu",
    "disk",
    "net",
    "weather",
    "media",
    "clipboard",
    "claude",
];

struct Compiled {
    icon: Icon,
    geoms: Vec<Option<ID2D1PathGeometry>>,
}

#[derive(Default)]
pub struct Pack {
    icons: HashMap<String, Compiled>,
    stroke: Option<ID2D1StrokeStyle>,
}

/// Lit le pack `name` ; un pack vide ou inconnu donne un pack sans icône.
pub fn load(name: &str, factory: &ID2D1Factory) -> Pack {
    let mut pack = Pack::default();
    if name.is_empty() {
        return pack;
    }
    let sources: Vec<(String, String)> = if name == "forge" {
        BUILTIN.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    } else {
        // Le nom vient de la configuration : un seul composant de chemin, jamais de séparateur.
        if name.contains(['/', '\\', ':']) || name.starts_with('.') {
            return pack;
        }
        let dir = forge_config::app_dir().join("icons").join(name);
        NAMES
            .iter()
            .filter_map(|n| {
                let text = std::fs::read_to_string(dir.join(format!("{n}.svg"))).ok()?;
                Some((n.to_string(), text))
            })
            .collect()
    };
    pack.stroke = unsafe {
        let props = D2D1_STROKE_STYLE_PROPERTIES {
            startCap: D2D1_CAP_STYLE_ROUND,
            endCap: D2D1_CAP_STYLE_ROUND,
            lineJoin: D2D1_LINE_JOIN_ROUND,
            ..Default::default()
        };
        factory.CreateStrokeStyle(&props, None).ok()
    };
    for (n, text) in sources {
        let Some(icon) = svg::parse_svg(&text) else { continue };
        let geoms = icon.shapes.iter().map(|s| geometry(factory, &s.cmds, s.fill)).collect();
        pack.icons.insert(n, Compiled { icon, geoms });
    }
    pack
}

fn pt(x: f32, y: f32) -> D2D_POINT_2F {
    D2D_POINT_2F { x, y }
}

fn geometry(factory: &ID2D1Factory, cmds: &[Cmd], filled: bool) -> Option<ID2D1PathGeometry> {
    unsafe {
        let g = factory.CreatePathGeometry().ok()?;
        let sink = g.Open().ok()?;
        let begin = if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW };
        let mut open = false;
        for c in cmds {
            match *c {
                Cmd::Move(x, y) => {
                    if open {
                        sink.EndFigure(D2D1_FIGURE_END_OPEN);
                    }
                    sink.BeginFigure(pt(x, y), begin);
                    open = true;
                }
                Cmd::Line(x, y) if open => sink.AddLine(pt(x, y)),
                Cmd::Cubic(v) if open => sink.AddBezier(&D2D1_BEZIER_SEGMENT {
                    point1: pt(v[0], v[1]),
                    point2: pt(v[2], v[3]),
                    point3: pt(v[4], v[5]),
                }),
                Cmd::Close if open => {
                    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
                    open = false;
                }
                _ => {}
            }
        }
        if open {
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
        }
        sink.Close().ok()?;
        Some(g)
    }
}

impl Pack {
    pub fn has(&self, name: &str) -> bool {
        self.icons.contains_key(name)
    }

    /// Dessine l'icône `name` dans le carré `size` x `size` dont le coin haut-gauche est `(x, y)`.
    pub fn draw(&self, rt: &ID2D1RenderTarget, name: &str, x: f32, y: f32, size: f32, brush: &ID2D1Brush) {
        let Some(c) = self.icons.get(name) else { return };
        let [vx, vy, vw, vh] = c.icon.view;
        let s = size / vw.max(vh);
        unsafe {
            // Centre le dessin si le viewBox n'est pas carré.
            let ox = x + (size - vw * s) / 2.0 - vx * s;
            let oy = y + (size - vh * s) / 2.0 - vy * s;
            rt.SetTransform(&Matrix3x2 { M11: s, M12: 0.0, M21: 0.0, M22: s, M31: ox, M32: oy });
            for (shape, geom) in c.icon.shapes.iter().zip(&c.geoms) {
                let Some(g) = geom else { continue };
                if shape.fill {
                    rt.FillGeometry(g, brush, None);
                }
                if let Some(w) = shape.stroke {
                    rt.DrawGeometry(g, brush, w, self.stroke.as_ref());
                }
            }
            rt.SetTransform(&Matrix3x2 { M11: 1.0, M12: 0.0, M21: 0.0, M22: 1.0, M31: 0.0, M32: 0.0 });
        }
    }
}
