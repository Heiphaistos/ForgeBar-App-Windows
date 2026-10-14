//! Ce que chaque widget affiche, calculé à partir des mesures système et des
//! données Claude. Aucun appel Windows ici : tout est testable sur n'importe quel OS.
#![cfg_attr(not(windows), allow(dead_code))]

use forge_claude::{fmt_tokens, Summary};
use forge_config::ClaudeConfig;

/// Glyphes Segoe Fluent Icons (Windows 11) et Segoe MDL2 Assets (Windows 10).
pub mod glyph {
    pub const HOME: char = '\u{E80F}';
    pub const WIFI: char = '\u{E701}';
    pub const VOLUME: char = '\u{E767}';
    pub const MUTE: char = '\u{E74F}';
    pub const CLOCK: char = '\u{E823}';
    pub const BATTERY: char = '\u{E83F}';
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Normal,
    Secondary,
    Accent,
    Warning,
    Critical,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub id: String,
    pub icon: Option<char>,
    pub text: String,
    pub level: Level,
}

#[derive(Debug, Clone, Default)]
pub struct Wifi {
    pub ssid: String,
    /// Qualité du signal, de 0 à 100.
    pub signal: u32,
}

#[derive(Debug, Clone, Default)]
pub struct SysData {
    pub cpu: f32,
    pub ram: f32,
    /// Pourcentage et branchement secteur ; `None` sans batterie.
    pub battery: Option<(u8, bool)>,
    /// Volume de 0 à 1 et sourdine ; `None` sans sortie audio.
    pub volume: Option<(f32, bool)>,
    /// `None` sans carte Wi-Fi, `Some(défaut)` si déconnecté.
    pub wifi: Option<Wifi>,
    pub clock: String,
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

fn usd(x: f64) -> String {
    format!("{x:.2} $").replace('.', ",")
}

fn seg(id: &str, icon: Option<char>, text: String, level: Level) -> Option<Segment> {
    Some(Segment { id: id.into(), icon, text, level })
}

/// Texte du widget Claude selon `bar_summary`.
pub fn claude_text(s: &Summary, cfg: &ClaudeConfig, expanded: bool) -> (String, Level) {
    let mut parts = Vec::new();
    let mut level = Level::Accent;
    let mut keys: Vec<&str> = cfg.bar_summary.iter().map(String::as_str).collect();
    if expanded {
        for k in ["tokens_today", "cost_week", "cost_month"] {
            if !keys.contains(&k) {
                keys.push(k);
            }
        }
    }
    let (warn, crit) = match cfg.alerts.as_slice() {
        [w, c, ..] => (*w as f32, *c as f32),
        [w] => (*w as f32, 100.0),
        [] => (80.0, 95.0),
    };
    for k in keys {
        match k {
            "context" => {
                if let Some(sess) = &s.session {
                    parts.push(format!("ctx {:.0} %", sess.context_pct));
                    level = match level_for(sess.context_pct, warn, crit) {
                        Level::Normal => level,
                        l => l,
                    };
                }
            }
            "tokens_today" => parts.push(format!("{} tok", fmt_tokens(s.today.tokens()))),
            "cost_today" => parts.push(format!("{} auj.", usd(s.today.cost_usd))),
            "cost_week" => parts.push(format!("{} sem.", usd(s.week.cost_usd))),
            "cost_month" => parts.push(format!("{} mois", usd(s.month.cost_usd))),
            _ => {}
        }
    }
    for b in forge_claude::budgets(s, &cfg.budgets) {
        let l = level_for(b.pct, warn, crit);
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

/// Construit le segment d'un widget ; `None` s'il n'a rien à montrer sur cette machine.
pub fn build(
    id: &str,
    sys: &SysData,
    claude: &Summary,
    cfg: &ClaudeConfig,
    icons_only: bool,
    claude_expanded: bool,
) -> Option<Segment> {
    let text = |t: String| if icons_only { String::new() } else { t };
    match id {
        "desktop" => seg(id, Some(glyph::HOME), String::new(), Level::Normal),
        "wifi" => {
            let w = sys.wifi.as_ref()?;
            if w.ssid.is_empty() {
                seg(id, Some(glyph::WIFI), text("Déconnecté".into()), Level::Secondary)
            } else {
                let lvl = if w.signal < 30 { Level::Warning } else { Level::Normal };
                seg(id, Some(glyph::WIFI), text(w.ssid.clone()), lvl)
            }
        }
        "sound" => {
            let (v, muted) = sys.volume?;
            let icon = if muted { glyph::MUTE } else { glyph::VOLUME };
            let lvl = if muted { Level::Secondary } else { Level::Normal };
            seg(id, Some(icon), text(format!("{:.0} %", v * 100.0)), lvl)
        }
        "battery" => {
            let (pct, plugged) = sys.battery?;
            let lvl = if plugged { Level::Normal } else { level_for(100.0 - pct as f32, 80.0, 90.0) };
            let t = if plugged { format!("{pct} % ⚡") } else { format!("{pct} %") };
            seg(id, Some(glyph::BATTERY), text(t), lvl)
        }
        "cpu" => seg(id, None, format!("CPU {:.0} %", sys.cpu), level_for(sys.cpu, 80.0, 95.0)),
        "ram" => seg(id, None, format!("RAM {:.0} %", sys.ram), level_for(sys.ram, 85.0, 95.0)),
        "clock" => seg(id, if icons_only { Some(glyph::CLOCK) } else { None }, text(sys.clock.clone()), Level::Normal),
        "claude" if cfg.enabled => {
            let (t, lvl) = claude_text(claude, cfg, claude_expanded);
            seg(id, None, format!("✦ {t}"), lvl)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_claude::{Session, Totals};

    #[test]
    fn hides_missing_hardware() {
        let sys = SysData::default();
        let cfg = ClaudeConfig::default();
        let s = Summary::default();
        assert!(build("battery", &sys, &s, &cfg, false, false).is_none());
        assert!(build("wifi", &sys, &s, &cfg, false, false).is_none());
        assert!(build("cpu", &sys, &s, &cfg, false, false).is_some());
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
        let (t, lvl) = claude_text(&s, &cfg, false);
        assert_eq!(t, "ctx 85 % · 1,23 $ auj.");
        assert_eq!(lvl, Level::Warning);
    }

    #[test]
    fn icons_only_drops_text() {
        let sys = SysData { volume: Some((0.5, false)), ..SysData::default() };
        let seg = build("sound", &sys, &Summary::default(), &ClaudeConfig::default(), true, false).unwrap();
        assert!(seg.text.is_empty());
        assert_eq!(seg.icon, Some(glyph::VOLUME));
    }
}
