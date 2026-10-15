use crate::color::{parse_color, Rgba};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

include!(concat!(env!("OUT_DIR"), "/builtin_themes.rs"));

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Backdrop {
    #[default]
    None,
    /// Windows 11 22H2 et plus ; repli sur le fond du thème ailleurs.
    Mica,
    Acrylic,
    Tabbed,
}

/// Thème qui se recalcule selon l'environnement Windows au lieu d'avoir des couleurs figées.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Adaptive {
    #[default]
    None,
    /// Mode clair ou sombre et couleur d'accentuation de Windows.
    System,
    /// Teinte tirée du fond d'écran, en plus du mode clair ou sombre.
    Wallpaper,
}

/// Ce que Windows dit de l'environnement, pour les thèmes adaptatifs.
#[derive(Debug, Clone, Copy)]
pub struct Env {
    pub dark: bool,
    pub accent: Rgba,
    pub wallpaper: Option<Rgba>,
}

const WHITE: Rgba = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };

fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
    let m = |x: f32, y: f32| x + (y - x) * t;
    Rgba { r: m(a.r, b.r), g: m(a.g, b.g), b: m(a.b, b.b), a: 1.0 }
}

fn luma(c: Rgba) -> f32 {
    0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b
}

fn hex(c: Rgba, alpha: f32) -> String {
    let b = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}{:02X}", b(c.r), b(c.g), b(c.b), b(alpha))
}

/// Éclaircit (fond sombre) ou assombrit (fond clair) une couleur jusqu'à ce qu'elle se lise bien dessus.
fn readable(mut c: Rgba, dark_bg: bool) -> Rgba {
    for _ in 0..10 {
        let l = luma(c);
        if (dark_bg && l >= 0.5) || (!dark_bg && l <= 0.3) {
            break;
        }
        c = mix(c, if dark_bg { WHITE } else { BLACK }, 0.15);
    }
    c
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundKind {
    #[default]
    Solid,
    Linear,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Background {
    pub kind: BackgroundKind,
    pub colors: Vec<String>,
    /// Angle du dégradé en degrés (0 = début vers fin de la barre).
    pub angle: f32,
    pub opacity: f32,
}

impl Default for Background {
    fn default() -> Self {
        Self { kind: BackgroundKind::Solid, colors: vec!["#202020".into()], angle: 0.0, opacity: 1.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeColors {
    pub text: String,
    pub text_secondary: String,
    pub accent: String,
    pub border: String,
    pub warning: String,
    pub critical: String,
}

impl Default for ThemeColors {
    fn default() -> Self {
        Self {
            text: "#FFFFFF".into(),
            text_secondary: "#C8C8C8".into(),
            accent: "#4CC2FF".into(),
            border: String::new(),
            warning: "#FCE100".into(),
            critical: "#FF6B6B".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub name: String,
    pub family: String,
    pub backdrop: Backdrop,
    pub radius: f32,
    pub border_width: f32,
    pub font_family: String,
    pub mono_family: String,
    pub font_size: f32,
    pub spacing: f32,
    pub adaptive: Adaptive,
    pub background: Background,
    pub colors: ThemeColors,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "Fluent sombre".into(),
            family: "windows".into(),
            backdrop: Backdrop::None,
            radius: 0.0,
            border_width: 0.0,
            font_family: "Segoe UI Variable Text".into(),
            mono_family: "Cascadia Mono".into(),
            font_size: 12.0,
            spacing: 14.0,
            adaptive: Adaptive::None,
            background: Background::default(),
            colors: ThemeColors::default(),
        }
    }
}

fn color_or(s: &str, fallback: Rgba) -> Rgba {
    parse_color(s).unwrap_or(fallback)
}

impl Theme {
    /// Thème adaptatif résolu pour l'environnement donné ; un thème ordinaire ressort inchangé.
    pub fn adapted(mut self, env: &Env) -> Theme {
        let (tint, neutral) = match self.adaptive {
            Adaptive::None => return self,
            Adaptive::System => (env.accent, if env.dark { 0.0 } else { 1.0 }),
            Adaptive::Wallpaper => (env.wallpaper.unwrap_or(env.accent), -1.0),
        };
        // Système : fond neutre ; fond d'écran : fond teinté par la couleur dominante.
        let bg = if neutral >= 0.0 {
            if env.dark {
                Rgba { r: 0.125, g: 0.125, b: 0.125, a: 1.0 }
            } else {
                Rgba { r: 0.953, g: 0.953, b: 0.953, a: 1.0 }
            }
        } else if env.dark {
            mix(tint, BLACK, 0.8)
        } else {
            mix(tint, WHITE, 0.85)
        };
        let ink = if env.dark { WHITE } else { Rgba { r: 0.06, g: 0.06, b: 0.06, a: 1.0 } };
        self.background.kind = BackgroundKind::Solid;
        self.background.colors = vec![hex(bg, 1.0)];
        self.colors.text = hex(ink, 1.0);
        self.colors.text_secondary = hex(ink, 0.7);
        self.colors.accent = hex(readable(tint, env.dark), 1.0);
        self.colors.border = hex(ink, 0.16);
        self
    }

    pub fn text(&self) -> Rgba {
        color_or(&self.colors.text, Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 })
    }
    pub fn text_secondary(&self) -> Rgba {
        color_or(&self.colors.text_secondary, self.text().with_alpha(0.75))
    }
    pub fn accent(&self) -> Rgba {
        color_or(&self.colors.accent, Rgba { r: 0.3, g: 0.76, b: 1.0, a: 1.0 })
    }
    pub fn warning(&self) -> Rgba {
        color_or(&self.colors.warning, Rgba { r: 0.99, g: 0.88, b: 0.0, a: 1.0 })
    }
    pub fn critical(&self) -> Rgba {
        color_or(&self.colors.critical, Rgba { r: 1.0, g: 0.42, b: 0.42, a: 1.0 })
    }
    pub fn border(&self) -> Option<Rgba> {
        parse_color(&self.colors.border)
    }
    /// Couleurs du fond, opacité appliquée.
    pub fn background_colors(&self) -> Vec<Rgba> {
        let mut v: Vec<Rgba> = self
            .background
            .colors
            .iter()
            .filter_map(|c| parse_color(c))
            .map(|c| c.with_alpha(self.background.opacity))
            .collect();
        if v.is_empty() {
            v.push(Rgba { r: 0.12, g: 0.12, b: 0.12, a: self.background.opacity });
        }
        v
    }
}

/// Fusionne `over` dans `base`, table par table.
fn merge(base: &mut toml::Value, over: toml::Value) {
    match (base, over) {
        (toml::Value::Table(b), toml::Value::Table(o)) => {
            for (k, v) in o {
                match b.get_mut(&k) {
                    Some(existing) => merge(existing, v),
                    None => {
                        b.insert(k, v);
                    }
                }
            }
        }
        (b, o) => *b = o,
    }
}

/// Tous les thèmes connus : intégrés au binaire, puis ceux de l'utilisateur, qui priment.
pub struct ThemeStore {
    raw: HashMap<String, toml::Value>,
}

impl ThemeStore {
    pub fn load(user_dir: Option<&Path>) -> Self {
        let mut raw = HashMap::new();
        for (id, text) in BUILTIN_THEMES {
            if let Ok(v) = text.parse::<toml::Value>() {
                raw.insert(id.to_string(), v);
            }
        }
        if let Some(entries) = user_dir.and_then(|d| std::fs::read_dir(d).ok()) {
            for p in entries.flatten().map(|e| e.path()) {
                if p.extension().is_some_and(|x| x == "toml") {
                    if let (Some(id), Ok(text)) = (p.file_stem(), std::fs::read_to_string(&p)) {
                        if let Ok(v) = text.parse::<toml::Value>() {
                            raw.insert(id.to_string_lossy().to_string(), v);
                        }
                    }
                }
            }
        }
        Self { raw }
    }

    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.raw.keys().cloned().collect();
        ids.sort();
        ids
    }

    /// Résout un thème et sa chaîne `extends`, avec repli sur le thème par défaut.
    pub fn get(&self, id: &str) -> Theme {
        let mut chain: Vec<String> = Vec::new();
        let mut cur = Some(id.to_string());
        while let Some(name) = cur.take() {
            if chain.contains(&name) || chain.len() > 8 {
                break;
            }
            let Some(v) = self.raw.get(&name) else { break };
            cur = v.get("extends").and_then(|e| e.as_str()).map(String::from);
            chain.push(name);
        }
        let mut value = toml::Value::Table(Default::default());
        for name in chain.iter().rev() {
            let mut layer = self.raw[name].clone();
            if let Some(t) = layer.as_table_mut() {
                t.remove("extends");
            }
            merge(&mut value, layer);
        }
        value.try_into().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_is_complete() {
        let store = ThemeStore::load(None);
        assert!(store.ids().len() >= 50, "seulement {} thèmes", store.ids().len());
        for id in store.ids() {
            let t = store.get(&id);
            assert!(!t.name.is_empty(), "{id} sans nom");
            assert!(parse_color(&t.colors.text).is_some(), "{id} : couleur de texte invalide");
            for c in &t.background.colors {
                assert!(parse_color(c).is_some(), "{id} : couleur de fond invalide {c}");
            }
        }
    }

    #[test]
    fn adaptive_theme_follows_the_environment() {
        let mut t = Theme { adaptive: Adaptive::System, ..Theme::default() };
        let accent = Rgba { r: 0.0, g: 0.47, b: 0.83, a: 1.0 };
        let dark = t.clone().adapted(&Env { dark: true, accent, wallpaper: None });
        let light = t.clone().adapted(&Env { dark: false, accent, wallpaper: None });
        assert_eq!(dark.colors.text, "#FFFFFFFF");
        assert!(light.colors.text.starts_with("#0F0F0F"));
        assert_ne!(dark.background.colors, light.background.colors);
        assert!(luma(parse_color(&dark.colors.accent).unwrap()) >= 0.5, "accent lisible sur fond sombre");
        t.adaptive = Adaptive::None;
        assert_eq!(t.clone().adapted(&Env { dark: true, accent, wallpaper: None }).colors.text, t.colors.text);
    }

    #[test]
    fn extends_overrides_only_given_keys() {
        let mut store = ThemeStore::load(None);
        store.raw.insert(
            "mine".into(),
            "extends = \"nord\"\nname = \"Le mien\"\n[colors]\naccent = \"#FF0000\"\n".parse().unwrap(),
        );
        let base = store.get("nord");
        let mine = store.get("mine");
        assert_eq!(mine.name, "Le mien");
        assert_eq!(mine.colors.accent, "#FF0000");
        assert_eq!(mine.colors.text, base.colors.text);
    }
}
