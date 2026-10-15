//! Configuration de Forge Bar : fichier `config.toml`, thèmes et dispositions.
//!
//! Le fichier TOML est la source de vérité. La barre le relit à chaud et la
//! fenêtre de paramètres l'écrit.

mod color;
mod condition;
mod layout;
mod theme;

pub use color::{parse_color, Rgba};
pub use condition::Condition;
pub use layout::ALL as ALL_WIDGETS;
pub use layout::{Layout, LayoutPreset, RowZones, Zones};
pub use theme::{Adaptive, Backdrop, Background, BackgroundKind, Env, Theme, ThemeColors, ThemeStore};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

impl Edge {
    pub fn is_vertical(self) -> bool {
        matches!(self, Edge::Left | Edge::Right)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Réserve l'espace à l'écran, comme la barre des tâches (API AppBar).
    Reserved,
    /// Flotte au-dessus des fenêtres, avec masquage automatique optionnel.
    Overlay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub autostart: bool,
    /// Masque la barre quand une application passe en plein écran.
    pub hide_on_fullscreen: bool,
    /// Remplace l'accent du thème par la couleur dominante du fond d'écran.
    pub accent_from_wallpaper: bool,
    /// Cherche, vérifie et installe seule les nouvelles versions (copie installée par le MSI).
    pub auto_update: bool,
    /// Pack d'icônes SVG : sous-dossier de `%APPDATA%\ForgeBar\icons`, ou `forge` (intégré). Vide = glyphes Segoe.
    pub icon_pack: String,
}

impl Default for General {
    fn default() -> Self {
        Self {
            autostart: false,
            hide_on_fullscreen: true,
            accent_from_wallpaper: false,
            auto_update: true,
            icon_pack: String::new(),
        }
    }
}

/// Présentation d'un widget numérique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum WidgetStyle {
    #[default]
    Text,
    /// Texte avec une jauge fine dessous.
    Gauge,
    /// Anneau de progression à côté du texte.
    Ring,
    /// Mini-graphe de l'historique des dernières minutes.
    Graph,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenConfig {
    /// `primary`, `all`, ou numéro d'écran à partir de 1 (`"2"`).
    pub id: String,
    pub enabled: bool,
    pub edge: Edge,
    pub mode: Mode,
    pub autohide: bool,
    pub theme: String,
    /// Épaisseur en pixels logiques (100 % d'échelle).
    pub thickness: u32,
    /// Marge autour de la barre ; au-delà de 0 la barre devient flottante.
    pub margin: u32,
    pub layout: Layout,
    /// Style par widget : `cpu = "ring"`, `ram = "graph"`…
    pub styles: BTreeMap<String, WidgetStyle>,
    /// Affichage conditionnel : `battery = "battery<20"`, `media = "media"`… (voir `Condition`).
    pub show_when: BTreeMap<String, String>,
}

impl Default for ScreenConfig {
    fn default() -> Self {
        Self {
            id: "primary".into(),
            enabled: true,
            edge: Edge::Top,
            mode: Mode::Reserved,
            autohide: false,
            theme: "fluent-dark".into(),
            thickness: 32,
            margin: 0,
            layout: Layout::default(),
            styles: BTreeMap::new(),
            show_when: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price {
    /// Sous-chaîne recherchée dans l'identifiant du modèle ; la première qui correspond gagne.
    pub model_match: String,
    /// Prix en dollars par million de tokens.
    pub input: f64,
    pub output: f64,
    pub cache_write: f64,
    pub cache_read: f64,
}

impl Price {
    fn new(m: &str, input: f64, output: f64) -> Self {
        Self { model_match: m.into(), input, output, cache_write: input * 1.25, cache_read: input * 0.1 }
    }
}

/// Grille par défaut, en dollars par million de tokens.
/// À vérifier sur la page de tarifs d'Anthropic : elle est éditable dans `config.toml`.
pub fn default_prices() -> Vec<Price> {
    vec![
        Price::new("opus-4-1", 15.0, 75.0),
        Price::new("opus-4-2025", 15.0, 75.0),
        Price::new("opus", 5.0, 25.0),
        Price::new("sonnet", 3.0, 15.0),
        Price::new("haiku", 1.0, 5.0),
    ]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Budgets {
    pub daily_usd: Option<f64>,
    pub weekly_usd: Option<f64>,
    pub monthly_usd: Option<f64>,
}

/// Estimation de la fenêtre de 5 heures des abonnements Pro / Max, calculée sur les logs locaux.
/// Aucune API publique ne donne la limite réelle : `five_hour_limit_usd` est un budget que vous fixez.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Subscription {
    pub enabled: bool,
    pub five_hour_limit_usd: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Corner {
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Panneau flottant toujours visible, indépendant de la barre.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Floating {
    pub enabled: bool,
    pub opacity: f32,
    /// Les clics traversent le panneau.
    pub click_through: bool,
    pub corner: Corner,
    pub margin: u32,
}

impl Default for Floating {
    fn default() -> Self {
        Self { enabled: false, opacity: 0.85, click_through: true, corner: Corner::TopRight, margin: 16 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ClaudeConfig {
    pub enabled: bool,
    /// `claude_code` (logs locaux) et `api` (Admin API, clé dans le Gestionnaire d'identification).
    pub sources: Vec<String>,
    /// Ce que la barre affiche : `context`, `tokens_today`, `cost_today`, `cost_week`, `cost_month`.
    pub bar_summary: Vec<String>,
    pub alerts: Vec<u8>,
    pub context_window: u64,
    pub refresh_seconds: u64,
    /// Dossier des logs Claude Code ; vide = `%USERPROFILE%\.claude\projects`.
    pub logs_dir: String,
    pub budgets: Budgets,
    pub prices: Vec<Price>,
    /// Notification Windows quand une alerte ou un budget franchit un seuil.
    pub notifications: bool,
    pub subscription: Subscription,
    pub floating: Floating,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sources: vec!["claude_code".into()],
            bar_summary: vec!["context".into(), "cost_today".into()],
            alerts: vec![80, 95],
            context_window: 1_000_000,
            refresh_seconds: 60,
            logs_dir: String::new(),
            budgets: Budgets::default(),
            prices: default_prices(),
            notifications: true,
            subscription: Subscription::default(),
            floating: Floating::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TempUnit {
    #[default]
    Celsius,
    Fahrenheit,
}

/// Météo Open-Meteo, sans clé. Renseignez `city` ou `latitude` et `longitude`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Weather {
    pub enabled: bool,
    pub city: String,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub unit: TempUnit,
    pub refresh_minutes: u64,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            enabled: false,
            city: String::new(),
            latitude: None,
            longitude: None,
            unit: TempUnit::Celsius,
            refresh_minutes: 30,
        }
    }
}

/// Historique du presse-papiers, gardé en mémoire uniquement (jamais écrit sur disque).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Clipboard {
    pub enabled: bool,
    pub history: usize,
    pub max_chars: usize,
}

impl Default for Clipboard {
    fn default() -> Self {
        Self { enabled: false, history: 10, max_chars: 60 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub screen: Vec<ScreenConfig>,
    pub claude: ClaudeConfig,
    pub weather: Weather,
    pub clipboard: Clipboard,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            screen: vec![ScreenConfig::default()],
            claude: ClaudeConfig::default(),
            weather: Weather::default(),
            clipboard: Clipboard::default(),
        }
    }
}

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(String),
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "lecture impossible : {e}"),
            ConfigError::Parse(e) => write!(f, "TOML invalide : {e}"),
            ConfigError::Invalid(e) => write!(f, "configuration invalide : {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let cfg: Config = toml::from_str(text).map_err(|e| ConfigError::Parse(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).expect("sérialisation de la configuration")
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.screen.is_empty() {
            return Err(ConfigError::Invalid("au moins une section [[screen]] est requise".into()));
        }
        for s in &self.screen {
            if !(16..=96).contains(&s.thickness) {
                return Err(ConfigError::Invalid(format!("épaisseur {} hors limites (16 à 96)", s.thickness)));
            }
        }
        for (w, c) in self.screen.iter().flat_map(|s| s.show_when.iter()) {
            Condition::parse(c).map_err(|e| ConfigError::Invalid(format!("show_when.{w} : {e}")))?;
        }
        if !(0.1..=1.0).contains(&self.claude.floating.opacity) {
            return Err(ConfigError::Invalid("claude.floating.opacity doit être entre 0,1 et 1".into()));
        }
        if self.claude.refresh_seconds < 5 {
            return Err(ConfigError::Invalid("claude.refresh_seconds doit valoir au moins 5".into()));
        }
        Ok(())
    }

    /// Charge le fichier ; s'il n'existe pas, écrit la configuration par défaut.
    pub fn load_or_create(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let cfg = Self::default();
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(ConfigError::Io)?;
                }
                std::fs::write(path, cfg.to_toml()).map_err(ConfigError::Io)?;
                Ok(cfg)
            }
            Err(e) => Err(ConfigError::Io(e)),
        }
    }
}

/// `%APPDATA%\ForgeBar` sous Windows, `~/.config/forgebar` ailleurs (tests).
pub fn app_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata).join("ForgeBar");
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".config").join("forgebar")
}

/// Cible du Gestionnaire d'identification Windows qui porte la clé Admin API d'Anthropic.
pub const ADMIN_KEY_TARGET: &str = "ForgeBar/AnthropicAdminKey";

pub fn config_path() -> PathBuf {
    app_dir().join("config.toml")
}

pub fn user_themes_dir() -> PathBuf {
    app_dir().join("themes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let cfg = Config::default();
        let back = Config::from_toml(&cfg.to_toml()).unwrap();
        assert_eq!(back.screen.len(), 1);
        assert_eq!(back.screen[0].edge, Edge::Top);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let cfg = Config::from_toml("[[screen]]\nedge = \"left\"\nmode = \"overlay\"\n").unwrap();
        assert_eq!(cfg.screen[0].edge, Edge::Left);
        assert_eq!(cfg.screen[0].mode, Mode::Overlay);
        assert_eq!(cfg.screen[0].thickness, 32);
        assert!(cfg.claude.enabled);
    }

    #[test]
    fn rejects_bad_thickness() {
        assert!(Config::from_toml("[[screen]]\nthickness = 4\n").is_err());
    }
}
