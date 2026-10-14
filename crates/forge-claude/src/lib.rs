//! Consommation Claude pour le HUD de Forge Bar.
//!
//! Source implémentée : les logs locaux de Claude Code (`~/.claude/projects/**/*.jsonl`).
//! Chaque réponse de l'assistant y porte son bloc `usage` (tokens d'entrée, de sortie
//! et de cache). On les additionne par jour, semaine et mois, et on estime le coût
//! avec la grille de prix de la configuration.
//!
//! Sources prévues au jalon 3 : l'Admin API d'Anthropic (coûts réels d'une
//! organisation) et les limites d'abonnement Pro / Max, qui n'ont pas d'API publique.

use chrono::{DateTime, Datelike, Duration, Local, Utc};
use forge_config::{Budgets, ClaudeConfig, Price};
use serde_json::Value;
use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Une réponse de l'assistant lue dans un log.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Identifiant de déduplication (message + requête) ; les logs répètent parfois une ligne.
    pub key: Option<String>,
    pub ts: DateTime<Utc>,
    pub session: String,
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

impl Entry {
    /// Tokens présents dans le contexte au moment de cette réponse.
    pub fn context_tokens(&self) -> u64 {
        self.input + self.cache_write + self.cache_read
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Totals {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub cost_usd: f64,
    /// Tokens d'un modèle absent de la grille de prix : comptés, mais sans coût.
    pub unpriced_tokens: u64,
    pub messages: u64,
}

impl Totals {
    pub fn tokens(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }

    fn add(&mut self, e: &Entry, prices: &[Price]) {
        self.input += e.input;
        self.output += e.output;
        self.cache_write += e.cache_write;
        self.cache_read += e.cache_read;
        self.messages += 1;
        match price_for(&e.model, prices) {
            Some(p) => self.cost_usd += cost(e, p),
            None => self.unpriced_tokens += e.input + e.output + e.cache_write + e.cache_read,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub model: String,
    pub context_tokens: u64,
    /// Part de la fenêtre de contexte utilisée, de 0 à 100.
    pub context_pct: f32,
    pub last: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub today: Totals,
    pub week: Totals,
    pub month: Totals,
    /// Session Claude Code la plus récente.
    pub session: Option<Session>,
    pub files_scanned: usize,
    pub updated: Option<DateTime<Local>>,
}

pub fn price_for<'a>(model: &str, prices: &'a [Price]) -> Option<&'a Price> {
    prices.iter().find(|p| model.contains(&p.model_match))
}

pub fn cost(e: &Entry, p: &Price) -> f64 {
    (e.input as f64 * p.input
        + e.output as f64 * p.output
        + e.cache_write as f64 * p.cache_write
        + e.cache_read as f64 * p.cache_read)
        / 1_000_000.0
}

fn u(v: &Value, k: &str) -> u64 {
    v.get(k).and_then(Value::as_u64).unwrap_or(0)
}

/// Lit une ligne JSONL ; renvoie `None` si ce n'est pas une réponse avec usage.
pub fn parse_line(line: &str) -> Option<Entry> {
    let v: Value = serde_json::from_str(line).ok()?;
    if v.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let msg = v.get("message")?;
    let usage = msg.get("usage")?;
    let ts = v.get("timestamp").and_then(Value::as_str)?;
    let ts = DateTime::parse_from_rfc3339(ts).ok()?.with_timezone(&Utc);
    let key = match (msg.get("id").and_then(Value::as_str), v.get("requestId").and_then(Value::as_str)) {
        (Some(m), Some(r)) => Some(format!("{m}:{r}")),
        (Some(m), None) => Some(m.to_string()),
        _ => None,
    };
    Some(Entry {
        key,
        ts,
        session: v.get("sessionId").and_then(Value::as_str).unwrap_or_default().to_string(),
        model: msg.get("model").and_then(Value::as_str).unwrap_or_default().to_string(),
        input: u(usage, "input_tokens"),
        output: u(usage, "output_tokens"),
        cache_write: u(usage, "cache_creation_input_tokens"),
        cache_read: u(usage, "cache_read_input_tokens"),
    })
}

/// Dossier des logs : `CLAUDE_CONFIG_DIR/projects`, sinon `~/.claude/projects`.
pub fn default_logs_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("projects"));
    }
    let home = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).ok()?;
    Some(PathBuf::from(home).join(".claude").join("projects"))
}

/// Fichiers `.jsonl` modifiés depuis `since`, en descendant dans les sous-dossiers.
fn recent_logs(dir: &Path, since: std::time::SystemTime, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        let Ok(meta) = e.metadata() else { continue };
        if meta.is_dir() {
            recent_logs(&p, since, out);
        } else if p.extension().is_some_and(|x| x == "jsonl") && meta.modified().is_ok_and(|m| m >= since) {
            out.push(p);
        }
    }
}

/// Agrège des entrées déjà lues. Séparé de la lecture disque pour être testable.
pub fn summarize(entries: &[Entry], cfg: &ClaudeConfig, now: DateTime<Local>) -> Summary {
    let today = now.date_naive();
    let week_start = today - Duration::days(now.weekday().num_days_from_monday() as i64);
    let mut s = Summary { updated: Some(now), ..Summary::default() };
    let mut seen = HashSet::new();
    let mut latest: Option<&Entry> = None;
    for e in entries {
        if let Some(k) = &e.key {
            if !seen.insert(k.clone()) {
                continue;
            }
        }
        let day = e.ts.with_timezone(&Local).date_naive();
        if day == today {
            s.today.add(e, &cfg.prices);
        }
        if day >= week_start && day <= today {
            s.week.add(e, &cfg.prices);
        }
        if day.year() == today.year() && day.month() == today.month() {
            s.month.add(e, &cfg.prices);
        }
        if latest.is_none_or(|l| e.ts > l.ts) {
            latest = Some(e);
        }
    }
    s.session = latest.map(|e| {
        let ctx = e.context_tokens();
        Session {
            id: e.session.clone(),
            model: e.model.clone(),
            context_tokens: ctx,
            context_pct: (ctx as f64 / cfg.context_window.max(1) as f64 * 100.0).min(100.0) as f32,
            last: e.ts,
        }
    });
    s
}

/// Lit les logs du mois en cours (et des 7 derniers jours) puis agrège.
pub fn scan(cfg: &ClaudeConfig, now: DateTime<Local>) -> Summary {
    let dir = if cfg.logs_dir.is_empty() { default_logs_dir() } else { Some(PathBuf::from(&cfg.logs_dir)) };
    let Some(dir) = dir else { return Summary::default() };
    let since = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 24 * 3600);
    let mut files = Vec::new();
    recent_logs(&dir, since, &mut files);
    let mut entries = Vec::new();
    for f in &files {
        let Ok(file) = std::fs::File::open(f) else { continue };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if let Some(e) = parse_line(&line) {
                entries.push(e);
            }
        }
    }
    let mut s = summarize(&entries, cfg, now);
    s.files_scanned = files.len();
    s
}

/// État d'un budget personnel fixé par l'utilisateur.
#[derive(Debug, Clone, PartialEq)]
pub struct BudgetStatus {
    pub label: &'static str,
    pub used_usd: f64,
    pub limit_usd: f64,
    pub pct: f32,
}

pub fn budgets(s: &Summary, b: &Budgets) -> Vec<BudgetStatus> {
    [
        ("jour", s.today.cost_usd, b.daily_usd),
        ("semaine", s.week.cost_usd, b.weekly_usd),
        ("mois", s.month.cost_usd, b.monthly_usd),
    ]
    .into_iter()
    .filter_map(|(label, used, limit)| {
        let limit = limit.filter(|l| *l > 0.0)?;
        Some(BudgetStatus { label, used_usd: used, limit_usd: limit, pct: (used / limit * 100.0) as f32 })
    })
    .collect()
}

/// Formate un nombre de tokens : 950, 12,4 k, 3,1 M.
pub fn fmt_tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1} k", n as f64 / 1e3).replace('.', ","),
        _ => format!("{:.1} M", n as f64 / 1e6).replace('.', ","),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn line(id: &str, ts: &str, model: &str, input: u64, output: u64, cw: u64, cr: u64) -> String {
        format!(
            r#"{{"type":"assistant","sessionId":"s1","requestId":"r-{id}","timestamp":"{ts}","message":{{"id":"m-{id}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_creation_input_tokens":{cw},"cache_read_input_tokens":{cr}}}}}}}"#
        )
    }

    #[test]
    fn parses_assistant_usage() {
        let e = parse_line(&line("1", "2026-10-15T10:00:00Z", "claude-sonnet-4-5", 10, 20, 30, 40)).unwrap();
        assert_eq!((e.input, e.output, e.cache_write, e.cache_read), (10, 20, 30, 40));
        assert_eq!(e.context_tokens(), 80);
        assert_eq!(e.key.as_deref(), Some("m-1:r-1"));
    }

    #[test]
    fn ignores_other_lines() {
        assert!(parse_line(r#"{"type":"user","message":{"content":"hi"}}"#).is_none());
        assert!(parse_line("pas du json").is_none());
    }

    #[test]
    fn totals_dedup_and_cost() {
        let cfg = ClaudeConfig::default();
        let now = Local.with_ymd_and_hms(2026, 10, 15, 18, 0, 0).unwrap();
        let today = now.with_timezone(&Utc).to_rfc3339();
        let a = parse_line(&line("1", &today, "claude-sonnet-4-5", 1_000_000, 0, 0, 0)).unwrap();
        let dup = a.clone();
        let b = parse_line(&line("2", &today, "claude-mystery", 500, 0, 0, 0)).unwrap();
        let s = summarize(&[a, dup, b], &cfg, now);
        assert_eq!(s.today.messages, 2);
        assert!((s.today.cost_usd - 3.0).abs() < 1e-9, "Sonnet : 3 $ par million de tokens d'entrée");
        assert_eq!(s.today.unpriced_tokens, 500);
        assert_eq!(s.month.messages, 2);
    }

    #[test]
    fn context_percentage() {
        let cfg = ClaudeConfig { context_window: 200_000, ..ClaudeConfig::default() };
        let now = Local.with_ymd_and_hms(2026, 10, 15, 18, 0, 0).unwrap();
        let ts = now.with_timezone(&Utc).to_rfc3339();
        let e = parse_line(&line("1", &ts, "claude-opus-4-5", 1_000, 500, 9_000, 40_000)).unwrap();
        let s = summarize(&[e], &cfg, now);
        assert!((s.session.unwrap().context_pct - 25.0).abs() < 0.01);
    }

    #[test]
    fn budget_percentages() {
        let s = Summary { today: Totals { cost_usd: 2.5, ..Totals::default() }, ..Summary::default() };
        let b = Budgets { daily_usd: Some(5.0), ..Budgets::default() };
        let st = budgets(&s, &b);
        assert_eq!(st.len(), 1);
        assert!((st[0].pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn token_format() {
        assert_eq!(fmt_tokens(950), "950");
        assert_eq!(fmt_tokens(12_400), "12,4 k");
        assert_eq!(fmt_tokens(3_100_000), "3,1 M");
    }
}
