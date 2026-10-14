//! Ce que chaque widget affiche, calculé à partir des mesures système et des
//! données Claude. Aucun appel Windows ici : tout est testable sur n'importe quel OS.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::weather;
use forge_claude::{fmt_tokens, Summary};
use forge_config::{ClaudeConfig, Condition};
use std::collections::VecDeque;

/// Glyphes Segoe Fluent Icons (Windows 11) et Segoe MDL2 Assets (Windows 10).
pub mod glyph {
    pub const HOME: char = '\u{E80F}';
    pub const WIFI: char = '\u{E701}';
    pub const VOLUME: char = '\u{E767}';
    pub const MUTE: char = '\u{E74F}';
    pub const CLOCK: char = '\u{E823}';
    pub const BATTERY: char = '\u{E83F}';
    pub const CLIPBOARD: char = '\u{E77F}';
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Normal,
    Secondary,
    Accent,
    Warning,
    Critical,
}

/// Historique d'une mesure, pour le style « mini-graphe ».
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Series {
    Cpu,
    Ram,
    Gpu,
    Net,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: String,
    pub icon: Option<char>,
    /// Nom de l'icône dans un pack SVG.
    pub svg: &'static str,
    pub text: String,
    /// Texte sans libellé (« 42 % » au lieu de « CPU 42 % »), utilisé quand une icône remplace le libellé.
    pub short: String,
    pub level: Level,
    /// Valeur de 0 à 100 pour les styles jauge et anneau.
    pub value: Option<f32>,
    pub series: Option<Series>,
}

#[derive(Debug, Clone, Default)]
pub struct Wifi {
    pub ssid: String,
    /// Qualité du signal, de 0 à 100.
    pub signal: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub title: String,
    pub artist: String,
    pub playing: bool,
}

pub const HISTORY_LEN: usize = 60;

#[derive(Debug, Clone, Default)]
pub struct History {
    pub cpu: VecDeque<f32>,
    pub ram: VecDeque<f32>,
    pub gpu: VecDeque<f32>,
    /// Débit descendant en octets par seconde.
    pub net: VecDeque<f32>,
}

impl History {
    fn push(q: &mut VecDeque<f32>, v: f32) {
        if q.len() == HISTORY_LEN {
            q.pop_front();
        }
        q.push_back(v);
    }

    pub fn record(&mut self, d: &SysData) {
        Self::push(&mut self.cpu, d.cpu);
        Self::push(&mut self.ram, d.ram);
        if let Some(g) = d.gpu {
            Self::push(&mut self.gpu, g);
        }
        if let Some((down, _)) = d.net {
            Self::push(&mut self.net, down as f32);
        }
    }

    pub fn get(&self, s: Series) -> &VecDeque<f32> {
        match s {
            Series::Cpu => &self.cpu,
            Series::Ram => &self.ram,
            Series::Gpu => &self.gpu,
            Series::Net => &self.net,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SysData {
    pub cpu: f32,
    pub ram: f32,
    /// Charge GPU en pourcentage ; `None` sans compteur disponible.
    pub gpu: Option<f32>,
    /// Débit (descendant, montant) en octets par seconde ; `None` sans carte réseau active.
    pub net: Option<(f64, f64)>,
    /// Pourcentage et branchement secteur ; `None` sans batterie.
    pub battery: Option<(u8, bool)>,
    /// Volume de 0 à 1 et sourdine ; `None` sans sortie audio.
    pub volume: Option<(f32, bool)>,
    /// `None` sans carte Wi-Fi, `Some(défaut)` si déconnecté.
    pub wifi: Option<Wifi>,
    pub media: Option<Media>,
    pub weather: Option<weather::Now>,
    /// Historique du presse-papiers, du plus récent au plus ancien ; `None` si le widget est désactivé.
    pub clipboard: Option<Vec<String>>,
    pub clock: String,
    pub hist: History,
}

fn level_for(pct: f32, warn: f32, crit: f32) -> Level {
    if pct >= crit {
        Level::Critical
    } else if pct >= warn {
        Level::Warning
    } else {
        Level::Normal
    }
}

pub fn usd(x: f64) -> String {
    format!("{x:.2} $").replace('.', ",")
}

/// Débit lisible : 850 o/s, 12,4 Ko/s, 3,1 Mo/s.
pub fn fmt_rate(bytes_per_s: f64) -> String {
    let b = bytes_per_s.max(0.0);
    if b < 1000.0 {
        format!("{b:.0} o/s")
    } else if b < 1e6 {
        format!("{:.1} Ko/s", b / 1e3).replace('.', ",")
    } else if b < 1e9 {
        format!("{:.1} Mo/s", b / 1e6).replace('.', ",")
    } else {
        format!("{:.1} Go/s", b / 1e9).replace('.', ",")
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    let flat: String = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if flat.chars().count() <= max {
        flat
    } else {
        let mut t: String = flat.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

pub fn thresholds(cfg: &ClaudeConfig) -> (f32, f32) {
    match cfg.alerts.as_slice() {
        [w, c, ..] => (*w as f32, *c as f32),
        [w] => (*w as f32, 100.0),
        [] => (80.0, 95.0),
    }
}

/// Texte du widget Claude selon `bar_summary`.
/// Clés : `context`, `tokens_today`, `cost_today`, `cost_week`, `cost_month`, `five_hour`, `org_today`, `org_month`.
pub fn claude_text(s: &Summary, cfg: &ClaudeConfig) -> (String, Level) {
    let mut parts = Vec::new();
    let mut level = Level::Accent;
    let (warn, crit) = thresholds(cfg);
    for k in cfg.bar_summary.iter().map(String::as_str) {
        match k {
            "context" => {
                if let Some(sess) = &s.session {
                    parts.push(format!("ctx {:.0} %", sess.context_pct));
                    let l = level_for(sess.context_pct, warn, crit);
                    if l != Level::Normal {
                        level = l;
                    }
                }
            }
            "tokens_today" => parts.push(format!("{} tok", fmt_tokens(s.today.tokens()))),
            "cost_today" => parts.push(format!("{} auj.", usd(s.today.cost_usd))),
            "cost_week" => parts.push(format!("{} sem.", usd(s.week.cost_usd))),
            "cost_month" => parts.push(format!("{} mois", usd(s.month.cost_usd))),
            "five_hour" => {
                if let Some(b) = &s.five_hour {
                    match b.pct {
                        Some(p) => parts.push(format!("5 h {p:.0} %")),
                        None => parts.push(format!("5 h {}", usd(b.cost_usd))),
                    }
                }
            }
            "org_today" => {
                if let Some(o) = &s.org {
                    parts.push(format!("{} org. auj.", usd(o.today_usd)));
                }
            }
            "org_month" => {
                if let Some(o) = &s.org {
                    parts.push(format!("{} org. mois", usd(o.month_usd)));
                }
            }
            _ => {}
        }
    }
    let mut worst = forge_claude::budgets(s, &cfg.budgets).into_iter().map(|b| b.pct).collect::<Vec<_>>();
    worst.extend(s.five_hour.as_ref().and_then(|b| b.pct));
    for pct in worst {
        let l = level_for(pct, warn, crit);
        if l == Level::Critical || (l == Level::Warning && level != Level::Critical) {
            level = l;
        }
    }
    if parts.is_empty() {
        parts.push("Claude".into());
        level = Level::Secondary;
    }
    (parts.join(" · "), level)
}

/// Valeur courante d'une métrique pour les conditions d'affichage.
pub fn metric(name: &str, sys: &SysData, claude: &Summary) -> Option<f32> {
    match name {
        "battery" => sys.battery.map(|(p, _)| p as f32),
        "cpu" => Some(sys.cpu),
        "ram" => Some(sys.ram),
        "gpu" => sys.gpu,
        "claude" => claude.session.as_ref().map(|s| s.context_pct),
        _ => None,
    }
}

pub fn visible(cond: &Condition, sys: &SysData, claude: &Summary) -> bool {
    let playing = sys.media.as_ref().is_some_and(|m| m.playing);
    cond.holds(playing, |m| metric(m, sys, claude))
}

pub struct Inputs<'a> {
    pub sys: &'a SysData,
    pub claude: &'a Summary,
    pub cfg: &'a ClaudeConfig,
    pub icons_only: bool,
}

fn seg(id: &str, svg: &'static str, icon: Option<char>, text: String, short: String, level: Level) -> Segment {
    Segment { id: id.into(), icon, svg, text, short, level, value: None, series: None }
}

/// Construit le segment d'un widget ; `None` s'il n'a rien à montrer sur cette machine.
pub fn build(id: &str, inp: &Inputs) -> Option<Segment> {
    let (sys, claude, cfg) = (inp.sys, inp.claude, inp.cfg);
    let hide = |t: String| if inp.icons_only { String::new() } else { t };
    let pct = |id: &str, svg, label: &str, v: f32, warn, crit, series| {
        let mut s = seg(id, svg, None, format!("{label} {v:.0} %"), format!("{v:.0} %"), level_for(v, warn, crit));
        s.value = Some(v);
        s.series = Some(series);
        s
    };
    match id {
        "desktop" => Some(seg(id, "desktop", Some(glyph::HOME), String::new(), String::new(), Level::Normal)),
        "wifi" => {
            let w = sys.wifi.as_ref()?;
            let (t, lvl) = if w.ssid.is_empty() {
                (String::new(), Level::Secondary)
            } else {
                (w.ssid.clone(), if w.signal < 30 { Level::Warning } else { Level::Normal })
            };
            let mut s = seg(id, "wifi", Some(glyph::WIFI), hide(t.clone()), hide(t), lvl);
            s.value = Some(w.signal as f32);
            Some(s)
        }
        "sound" => {
            let (v, muted) = sys.volume?;
            let (icon, svg) = if muted { (glyph::MUTE, "mute") } else { (glyph::VOLUME, "volume") };
            let lvl = if muted { Level::Secondary } else { Level::Normal };
            let t = format!("{:.0} %", v * 100.0);
            let mut s = seg(id, svg, Some(icon), hide(t.clone()), hide(t), lvl);
            s.value = Some(v * 100.0);
            Some(s)
        }
        "battery" => {
            let (p, plugged) = sys.battery?;
            let lvl = if plugged { Level::Normal } else { level_for(100.0 - p as f32, 80.0, 90.0) };
            let t = if plugged { format!("{p} % ⚡") } else { format!("{p} %") };
            let mut s = seg(id, "battery", Some(glyph::BATTERY), hide(t.clone()), hide(t), lvl);
            s.value = Some(p as f32);
            Some(s)
        }
        "cpu" => Some(pct(id, "cpu", "CPU", sys.cpu, 80.0, 95.0, Series::Cpu)),
        "ram" => Some(pct(id, "ram", "RAM", sys.ram, 85.0, 95.0, Series::Ram)),
        "gpu" => Some(pct(id, "gpu", "GPU", sys.gpu?, 85.0, 95.0, Series::Gpu)),
        "net" => {
            let (down, up) = sys.net?;
            let t = format!("↓ {}  ↑ {}", fmt_rate(down), fmt_rate(up));
            let short = format!("↓ {}", fmt_rate(down));
            let mut s = seg(id, "net", None, t, short, Level::Normal);
            s.series = Some(Series::Net);
            Some(s)
        }
        "clock" => {
            let icon = inp.icons_only.then_some(glyph::CLOCK);
            Some(seg(id, "clock", icon, hide(sys.clock.clone()), hide(sys.clock.clone()), Level::Normal))
        }
        "weather" => {
            let w = sys.weather.as_ref()?;
            let t = format!("{} {}", w.symbol(), w.text());
            Some(seg(id, "weather", None, hide(t), hide(w.text()), Level::Normal))
        }
        "media" => {
            let m = sys.media.as_ref()?;
            let name = if m.artist.is_empty() { m.title.clone() } else { format!("{} – {}", m.artist, m.title) };
            let lvl = if m.playing { Level::Normal } else { Level::Secondary };
            let t = format!("{} {}", if m.playing { '♪' } else { '❚' }, truncate(&name, 38));
            Some(seg(id, "media", None, hide(t.clone()), hide(t), lvl))
        }
        "clipboard" => {
            let n = sys.clipboard.as_ref()?.len();
            Some(seg(id, "clipboard", Some(glyph::CLIPBOARD), hide(n.to_string()), hide(n.to_string()), Level::Normal))
        }
        "claude" if cfg.enabled => {
            let (t, lvl) = claude_text(claude, cfg);
            Some(seg(id, "claude", None, format!("✦ {t}"), t, lvl))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_claude::{Session, Totals};

    fn inputs<'a>(sys: &'a SysData, s: &'a Summary, cfg: &'a ClaudeConfig, icons_only: bool) -> Inputs<'a> {
        Inputs { sys, claude: s, cfg, icons_only }
    }

    #[test]
    fn hides_missing_hardware() {
        let (sys, cfg, s) = (SysData::default(), ClaudeConfig::default(), Summary::default());
        let i = inputs(&sys, &s, &cfg, false);
        for id in ["battery", "wifi", "gpu", "net", "weather", "media", "clipboard"] {
            assert!(build(id, &i).is_none(), "{id}");
        }
        assert!(build("cpu", &i).is_some());
    }

    #[test]
    fn claude_summary_and_alert_level() {
        let cfg = ClaudeConfig::default();
        let s = Summary {
            today: Totals { cost_usd: 1.234, ..Totals::default() },
            session: Some(Session {
                id: "s".into(),
                model: "m".into(),
                context_tokens: 170_000,
                context_pct: 85.0,
                last: chrono::Utc::now(),
            }),
            ..Summary::default()
        };
        let (t, lvl) = claude_text(&s, &cfg);
        assert_eq!(t, "ctx 85 % · 1,23 $ auj.");
        assert_eq!(lvl, Level::Warning);
    }

    #[test]
    fn icons_only_drops_text() {
        let sys = SysData { volume: Some((0.5, false)), ..SysData::default() };
        let (cfg, s) = (ClaudeConfig::default(), Summary::default());
        let seg = build("sound", &inputs(&sys, &s, &cfg, true)).unwrap();
        assert!(seg.text.is_empty());
        assert_eq!(seg.icon, Some(glyph::VOLUME));
    }

    #[test]
    fn network_and_gpu_text() {
        let sys = SysData { gpu: Some(37.4), net: Some((1_250_000.0, 40_000.0)), ..SysData::default() };
        let (cfg, s) = (ClaudeConfig::default(), Summary::default());
        let i = inputs(&sys, &s, &cfg, false);
        assert_eq!(build("gpu", &i).unwrap().text, "GPU 37 %");
        assert_eq!(build("net", &i).unwrap().text, "↓ 1,2 Mo/s  ↑ 40,0 Ko/s");
        assert_eq!(fmt_rate(850.0), "850 o/s");
    }

    #[test]
    fn media_is_truncated_and_conditions_work() {
        let long = "x".repeat(80);
        let sys = SysData {
            media: Some(Media { title: long, artist: "A".into(), playing: true }),
            battery: Some((15, false)),
            ..SysData::default()
        };
        let (cfg, s) = (ClaudeConfig::default(), Summary::default());
        let seg = build("media", &inputs(&sys, &s, &cfg, false)).unwrap();
        assert!(seg.text.chars().count() <= 40);
        assert!(seg.text.ends_with('…'));
        assert!(visible(&Condition::parse("media").unwrap(), &sys, &s));
        assert!(visible(&Condition::parse("battery<20").unwrap(), &sys, &s));
        assert!(!visible(&Condition::parse("battery<10").unwrap(), &sys, &s));
        assert!(!visible(&Condition::parse("gpu>10").unwrap(), &sys, &s), "métrique indisponible = masqué");
    }

    #[test]
    fn history_is_bounded() {
        let mut h = History::default();
        let d = SysData { cpu: 5.0, ..SysData::default() };
        for _ in 0..HISTORY_LEN + 10 {
            h.record(&d);
        }
        assert_eq!(h.cpu.len(), HISTORY_LEN);
        assert!(h.gpu.is_empty());
    }
}
