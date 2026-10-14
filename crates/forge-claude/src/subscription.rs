//! Estimation de la fenêtre de 5 heures des abonnements Pro / Max.
//!
//! Module optionnel et isolé : Anthropic ne publie aucune API pour les limites d'abonnement.
//! On reconstruit donc les blocs de 5 heures à partir de l'activité Claude Code locale (un bloc
//! démarre à l'heure pleine de la première réponse et dure 5 heures) et on compare son coût
//! estimé à un budget fixé par l'utilisateur. C'est un repère, pas la limite réelle du compte.

use crate::{price_for, Entry};
use chrono::{DateTime, Duration, DurationRound, Utc};
use forge_config::Price;

#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub cost_usd: f64,
    pub limit_usd: Option<f64>,
    /// Part du budget consommée ; `None` sans budget.
    pub pct: Option<f32>,
}

impl Block {
    pub fn remaining(&self, now: DateTime<Utc>) -> Duration {
        (self.end - now).max(Duration::zero())
    }
}

/// Bloc actif à `now`, à partir d'entrées triées par date. `None` si le dernier bloc est terminé.
pub fn current_block(entries: &[&Entry], limit: Option<f64>, prices: &[Price], now: DateTime<Utc>) -> Option<Block> {
    let five = Duration::hours(5);
    // (début, fin, coût, dernière activité)
    type Open = (DateTime<Utc>, DateTime<Utc>, f64, DateTime<Utc>);
    let mut block: Option<Open> = None;
    for e in entries {
        let cost = price_for(&e.model, prices).map_or(0.0, |p| crate::cost(e, p));
        match &mut block {
            Some((_, end, c, last)) if e.ts < *end && e.ts - *last < five => {
                *c += cost;
                *last = e.ts;
            }
            _ => {
                let start = e.ts.duration_trunc(Duration::hours(1)).unwrap_or(e.ts);
                block = Some((start, start + five, cost, e.ts));
            }
        }
    }
    let (start, end, cost_usd, _) = block?;
    if now >= end {
        return None;
    }
    let limit_usd = limit.filter(|l| *l > 0.0);
    Some(Block { start, end, cost_usd, limit_usd, pct: limit_usd.map(|l| (cost_usd / l * 100.0) as f32) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn entry(h: u32, m: u32, input: u64) -> Entry {
        Entry {
            key: None,
            ts: Utc.with_ymd_and_hms(2026, 10, 15, h, m, 0).unwrap(),
            session: "s".into(),
            model: "claude-sonnet-4-5".into(),
            input,
            output: 0,
            cache_write: 0,
            cache_read: 0,
        }
    }

    fn at(h: u32, m: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 15, h, m, 0).unwrap()
    }

    #[test]
    fn block_starts_on_the_hour_and_sums_cost() {
        let (a, b) = (entry(9, 40, 1_000_000), entry(11, 0, 1_000_000));
        let prices = forge_config::default_prices();
        let blk = current_block(&[&a, &b], Some(12.0), &prices, at(12, 0)).unwrap();
        assert_eq!(blk.start, at(9, 0));
        assert_eq!(blk.end, at(14, 0));
        assert!((blk.cost_usd - 6.0).abs() < 1e-9);
        assert!((blk.pct.unwrap() - 50.0).abs() < 0.01);
        assert_eq!(blk.remaining(at(12, 0)), Duration::hours(2));
    }

    #[test]
    fn expired_or_new_block() {
        let prices = forge_config::default_prices();
        let a = entry(1, 10, 1_000_000);
        assert!(current_block(&[&a], None, &prices, at(8, 0)).is_none(), "bloc terminé");
        let b = entry(8, 30, 1_000_000);
        let blk = current_block(&[&a, &b], None, &prices, at(9, 0)).unwrap();
        assert_eq!(blk.start, at(8, 0), "après un trou de plus de 5 h, nouveau bloc");
        assert!((blk.cost_usd - 3.0).abs() < 1e-9);
        assert!(blk.pct.is_none());
    }
}
