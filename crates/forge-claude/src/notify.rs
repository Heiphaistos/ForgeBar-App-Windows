//! Seuils d'alerte : décide quand une notification Windows doit partir.
//!
//! Une notification par seuil franchi et par période : le contexte d'une session, un budget du jour,
//! de la semaine ou du mois, la fenêtre de 5 heures. Franchir le seuil rouge après l'orange en envoie
//! une seconde ; rester au-dessus n'en renvoie aucune. Un nouveau jour, une nouvelle session ou un
//! nouveau bloc repart de zéro.

use crate::{budgets, Summary};
use chrono::{Datelike, Local};
use forge_config::ClaudeConfig;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    pub critical: bool,
}

#[derive(Default)]
pub struct Notifier {
    /// Niveau déjà notifié (1 = orange, 2 = rouge) par clé de période.
    sent: HashMap<String, u8>,
    primed: bool,
}

fn thresholds(cfg: &ClaudeConfig) -> (f32, f32) {
    match cfg.alerts.as_slice() {
        [w, c, ..] => (*w as f32, *c as f32),
        [w] => (*w as f32, 100.0),
        [] => (80.0, 95.0),
    }
}

impl Notifier {
    /// À appeler après chaque lecture. Le tout premier appel enregistre l'état courant sans notifier :
    /// démarrer la barre avec un budget déjà dépassé ne doit pas déclencher d'alerte au lancement.
    pub fn check(&mut self, s: &Summary, cfg: &ClaudeConfig, now: chrono::DateTime<Local>) -> Vec<Notice> {
        let (warn, crit) = thresholds(cfg);
        let mut candidates: Vec<(String, f32, String)> = Vec::new();
        if let Some(sess) = &s.session {
            candidates.push((format!("ctx:{}", sess.id), sess.context_pct, "Contexte Claude".into()));
        }
        let d = now.date_naive();
        for b in budgets(s, &cfg.budgets) {
            let period = match b.label {
                "jour" => d.to_string(),
                "semaine" => format!("{}-w{}", d.year(), d.iso_week().week()),
                _ => format!("{}-{}", d.year(), d.month()),
            };
            candidates.push((format!("budget:{}:{period}", b.label), b.pct, format!("Budget {}", b.label)));
        }
        if let Some(pct) = s.five_hour.as_ref().and_then(|b| b.pct.map(|p| (b.start, p))) {
            candidates.push((format!("5h:{}", pct.0.timestamp()), pct.1, "Fenêtre 5 h".into()));
        }
        let mut out = Vec::new();
        for (key, pct, label) in candidates {
            let level = if pct >= crit {
                2
            } else if pct >= warn {
                1
            } else {
                0
            };
            let prev = self.sent.get(&key).copied().unwrap_or(0);
            if level > prev {
                self.sent.insert(key, level);
                if self.primed {
                    out.push(Notice {
                        title: format!("{label} : {pct:.0} %"),
                        body: if level == 2 {
                            "Seuil critique atteint.".into()
                        } else {
                            "Seuil d'alerte atteint.".into()
                        },
                        critical: level == 2,
                    });
                }
            }
        }
        self.primed = true;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Session, Totals};
    use chrono::Utc;

    fn summary(pct: f32) -> Summary {
        Summary {
            session: Some(Session {
                id: "s1".into(),
                model: "m".into(),
                context_tokens: 0,
                context_pct: pct,
                last: Utc::now(),
            }),
            ..Summary::default()
        }
    }

    #[test]
    fn notifies_once_per_threshold_and_not_at_startup() {
        let cfg = ClaudeConfig::default();
        let now = Local::now();
        let mut n = Notifier::default();
        assert!(n.check(&summary(90.0), &cfg, now).is_empty(), "premier passage : on mémorise seulement");
        assert!(n.check(&summary(91.0), &cfg, now).is_empty(), "déjà notifié (silencieusement)");
        let crit = n.check(&summary(96.0), &cfg, now);
        assert_eq!(crit.len(), 1);
        assert!(crit[0].critical);
        assert!(n.check(&summary(97.0), &cfg, now).is_empty(), "pas de répétition");
    }

    #[test]
    fn warning_then_new_session_resets() {
        let cfg = ClaudeConfig::default();
        let now = Local::now();
        let mut n = Notifier::default();
        n.check(&summary(10.0), &cfg, now);
        assert_eq!(n.check(&summary(85.0), &cfg, now).len(), 1);
        let mut other = summary(85.0);
        other.session.as_mut().unwrap().id = "s2".into();
        assert_eq!(n.check(&other, &cfg, now).len(), 1, "autre session : nouvelle alerte");
    }

    #[test]
    fn budget_alert() {
        let mut cfg = ClaudeConfig::default();
        cfg.budgets.daily_usd = Some(10.0);
        let now = Local::now();
        let mut n = Notifier::default();
        let mut s = Summary::default();
        n.check(&s, &cfg, now);
        s.today = Totals { cost_usd: 8.5, ..Totals::default() };
        let got = n.check(&s, &cfg, now);
        assert_eq!(got.len(), 1);
        assert!(got[0].title.contains("Budget jour"));
    }
}
