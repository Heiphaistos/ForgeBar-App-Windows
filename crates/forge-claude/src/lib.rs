//! Consommation Claude pour le HUD de Forge Bar.
//!
//! Source implémentée : les logs locaux de Claude Code (`~/.claude/projects/**/*.jsonl`).
//! Chaque réponse de l'assistant y porte son bloc `usage` (tokens d'entrée, de sortie
//! et de cache). On les additionne par jour, semaine et mois, et on estime le coût
//! avec la grille de prix de la configuration.
//!
//! Sources prévues au jalon 3 : l'Admin API d'Anthropic (coûts réels d'une
//! organisation) et les limites d'abonnement Pro / Max, qui n'ont pas d'API publique.

pub mod admin;
pub mod notify;
pub mod subscription;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};
use forge_config::{Budgets, ClaudeConfig, Price};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Une réponse de l'assistant lue dans un log.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// Identifiant de déduplication (message + requête) ; les logs répètent parfois une ligne.
    pub key: Option<u64>,
    pub ts: DateTime<Utc>,
    pub session: Arc<str>,
    pub model: Arc<str>,
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

/// Un jour d'historique, pour les graphes du panneau détaillé.
#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub cost_usd: f64,
    pub tokens: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub today: Totals,
    pub week: Totals,
    pub month: Totals,
    /// Session Claude Code la plus récente.
    pub session: Option<Session>,
    /// Les 14 derniers jours, du plus ancien au plus récent (jours sans activité compris).
    pub daily: Vec<Day>,
    /// Totaux du mois par modèle, du plus coûteux au moins coûteux.
    pub models: Vec<(String, Totals)>,
    /// Fenêtre de 5 heures en cours (estimation locale).
    pub five_hour: Option<subscription::Block>,
    /// Coûts réels de l'organisation (Admin API), si la source `api` est active.
    pub org: Option<admin::OrgCost>,
    /// Dernière erreur de l'Admin API (clé absente ou refusée, réseau), affichée dans le panneau.
    pub org_error: Option<String>,
    pub files_scanned: usize,
    pub updated: Option<DateTime<Local>>,
}

/// Nombre de jours d'historique conservés et affichés.
pub const HISTORY_DAYS: i64 = 14;

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

/// Champs utiles d'une ligne de log ; tout le reste (contenu des messages, outils) est ignoré
/// sans être alloué, ce qui compte quand les logs pèsent plusieurs gigaoctets.
#[derive(serde::Deserialize)]
struct RawLine<'a> {
    #[serde(rename = "type")]
    kind: Option<&'a str>,
    timestamp: Option<&'a str>,
    #[serde(rename = "sessionId")]
    session: Option<&'a str>,
    #[serde(rename = "requestId")]
    request: Option<&'a str>,
    message: Option<RawMessage<'a>>,
}

#[derive(serde::Deserialize)]
struct RawMessage<'a> {
    id: Option<&'a str>,
    model: Option<&'a str>,
    usage: Option<RawUsage>,
}

#[derive(serde::Deserialize, Default)]
struct RawUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}

fn hash_key(msg: &str, request: Option<&str>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    msg.hash(&mut h);
    request.hash(&mut h);
    h.finish()
}

/// Lit une ligne JSONL ; renvoie `None` si ce n'est pas une réponse avec usage.
pub fn parse_line(line: &str) -> Option<Entry> {
    // Neuf lignes sur dix (messages utilisateur, résultats d'outils) n'ont pas de bloc usage :
    // une recherche de sous-chaîne évite de les analyser.
    if !line.contains("\"usage\"") {
        return None;
    }
    let raw: RawLine = serde_json::from_str(line).ok()?;
    if raw.kind != Some("assistant") {
        return None;
    }
    let msg = raw.message?;
    let usage = msg.usage?;
    let ts = DateTime::parse_from_rfc3339(raw.timestamp?).ok()?.with_timezone(&Utc);
    Some(Entry {
        key: msg.id.map(|m| hash_key(m, raw.request)),
        ts,
        session: raw.session.unwrap_or_default().into(),
        model: msg.model.unwrap_or_default().into(),
        input: usage.input_tokens,
        output: usage.output_tokens,
        cache_write: usage.cache_creation_input_tokens,
        cache_read: usage.cache_read_input_tokens,
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
    let mut days: HashMap<NaiveDate, Totals> = HashMap::new();
    let mut models: HashMap<String, Totals> = HashMap::new();
    let mut unique: Vec<&Entry> = Vec::with_capacity(entries.len());
    for e in entries {
        if let Some(k) = e.key {
            if !seen.insert(k) {
                continue;
            }
        }
        unique.push(e);
        let day = e.ts.with_timezone(&Local).date_naive();
        if day > today - Duration::days(HISTORY_DAYS) {
            days.entry(day).or_default().add(e, &cfg.prices);
        }
        if day.year() == today.year() && day.month() == today.month() {
            models.entry(e.model.to_string()).or_default().add(e, &cfg.prices);
        }
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
    s.daily = (0..HISTORY_DAYS)
        .rev()
        .map(|back| {
            let date = today - Duration::days(back);
            let t = days.get(&date).copied().unwrap_or_default();
            Day { date, cost_usd: t.cost_usd, tokens: t.tokens() }
        })
        .collect();
    let mut models: Vec<_> = models.into_iter().filter(|(m, _)| !m.is_empty() && m != "<synthetic>").collect();
    models.sort_by(|a, b| b.1.cost_usd.total_cmp(&a.1.cost_usd).then(b.1.tokens().cmp(&a.1.tokens())));
    s.models = models;
    if cfg.subscription.enabled {
        unique.sort_by_key(|e| e.ts);
        s.five_hour =
            subscription::current_block(&unique, cfg.subscription.five_hour_limit_usd, &cfg.prices, Utc::now());
    }
    s.session = latest.map(|e| {
        let ctx = e.context_tokens();
        // Les modèles à contexte étendu dépassent 200 k : au-delà de la fenêtre configurée, on suppose 1 M.
        let window = if ctx > cfg.context_window { cfg.context_window.max(1_000_000) } else { cfg.context_window };
        Session {
            id: e.session.to_string(),
            model: e.model.to_string(),
            context_tokens: ctx,
            context_pct: (ctx as f64 / window.max(1) as f64 * 100.0).min(100.0) as f32,
            last: e.ts,
        }
    });
    s
}

/// Lecteur incrémental des logs : retient le décalage atteint dans chaque fichier et ne lit
/// que les lignes ajoutées depuis le passage précédent. Garde les réponses des 40 derniers jours.
#[derive(Default)]
pub struct Scanner {
    offsets: HashMap<PathBuf, u64>,
    entries: Vec<Entry>,
    seen: HashSet<u64>,
    /// Chaînes répétées (modèle, session) partagées entre toutes les entrées.
    names: HashMap<String, Arc<str>>,
}

const KEEP_DAYS: i64 = 40;

impl Scanner {
    fn intern(names: &mut HashMap<String, Arc<str>>, s: Arc<str>) -> Arc<str> {
        if let Some(a) = names.get(&*s) {
            return a.clone();
        }
        names.insert(s.to_string(), s.clone());
        s
    }

    fn read_new(&mut self, path: &Path) {
        let Ok(mut file) = std::fs::File::open(path) else { return };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        let mut offset = self.offsets.get(path).copied().unwrap_or(0);
        if len < offset {
            offset = 0; // fichier tronqué ou remplacé : on relit depuis le début, la déduplication évite les doublons
        }
        if file.seek(SeekFrom::Start(offset)).is_err() {
            return;
        }
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if !line.ends_with('\n') {
                        break; // ligne encore en cours d'écriture : reprise au prochain passage
                    }
                    offset += n as u64;
                    if let Some(mut e) = parse_line(&line) {
                        if e.key.is_none_or(|k| self.seen.insert(k)) {
                            e.model = Self::intern(&mut self.names, e.model);
                            e.session = Self::intern(&mut self.names, e.session);
                            self.entries.push(e);
                        }
                    }
                }
            }
        }
        self.offsets.insert(path.to_path_buf(), offset);
    }

    /// Lit ce qui est nouveau puis agrège.
    pub fn scan(&mut self, cfg: &ClaudeConfig, now: DateTime<Local>) -> Summary {
        let dir = if cfg.logs_dir.is_empty() { default_logs_dir() } else { Some(PathBuf::from(&cfg.logs_dir)) };
        let Some(dir) = dir else { return Summary::default() };
        let since = std::time::SystemTime::now() - std::time::Duration::from_secs(KEEP_DAYS as u64 * 24 * 3600);
        let mut files = Vec::new();
        recent_logs(&dir, since, &mut files);
        for f in &files {
            self.read_new(f);
        }
        self.offsets.retain(|p, _| files.contains(p));
        let cutoff = Utc::now() - Duration::days(KEEP_DAYS);
        if self.entries.iter().any(|e| e.ts < cutoff) {
            self.entries.retain(|e| e.ts >= cutoff);
            self.seen = self.entries.iter().filter_map(|e| e.key).collect();
        }
        let mut s = summarize(&self.entries, cfg, now);
        s.files_scanned = files.len();
        s
    }
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
        assert!(e.key.is_some());
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
    fn incremental_scan_reads_only_new_lines() {
        let dir = std::env::temp_dir().join(format!("forgebar-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("s.jsonl");
        let now = Local::now();
        let ts = now.with_timezone(&Utc).to_rfc3339();
        let cfg = ClaudeConfig { logs_dir: dir.display().to_string(), ..ClaudeConfig::default() };
        let mut sc = Scanner::default();

        std::fs::write(&file, format!("{}\n", line("1", &ts, "claude-sonnet-4-5", 100, 0, 0, 0))).unwrap();
        assert_eq!(sc.scan(&cfg, now).today.messages, 1);

        // une ligne complète ajoutée + une ligne en cours d'écriture (sans saut de ligne)
        let more = format!(
            "{}\n{}",
            line("2", &ts, "claude-sonnet-4-5", 100, 0, 0, 0),
            line("3", &ts, "claude-sonnet-4-5", 1, 0, 0, 0)
        );
        let mut f = std::fs::OpenOptions::new().append(true).open(&file).unwrap();
        std::io::Write::write_all(&mut f, more.as_bytes()).unwrap();
        assert_eq!(sc.scan(&cfg, now).today.messages, 2, "la ligne partielle attend");
        std::io::Write::write_all(&mut f, b"\n").unwrap();
        assert_eq!(sc.scan(&cfg, now).today.messages, 3);
        assert_eq!(sc.scan(&cfg, now).today.messages, 3, "rien de nouveau : pas de double comptage");

        // fichier remplacé par un plus court : relu, sans doublon grâce aux clés
        std::fs::write(&file, format!("{}\n", line("1", &ts, "claude-sonnet-4-5", 100, 0, 0, 0))).unwrap();
        assert_eq!(sc.scan(&cfg, now).today.messages, 3);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Mesure sur les vrais logs de la machine : `cargo test -p forge-claude -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_logs_timing() {
        let cfg = ClaudeConfig::default();
        let mut sc = Scanner::default();
        let t = std::time::Instant::now();
        let s = sc.scan(&cfg, Local::now());
        println!(
            "1er passage : {:?}, {} fichiers, {} réponses, mois {:.2} $",
            t.elapsed(),
            s.files_scanned,
            sc.entries.len(),
            s.month.cost_usd
        );
        let t = std::time::Instant::now();
        sc.scan(&cfg, Local::now());
        println!("2e passage : {:?}", t.elapsed());
    }

    #[test]
    fn history_and_models() {
        let cfg = ClaudeConfig::default();
        let now = Local.with_ymd_and_hms(2026, 10, 15, 18, 0, 0).unwrap();
        let ts = now.with_timezone(&Utc).to_rfc3339();
        let yesterday = (now - Duration::days(1)).with_timezone(&Utc).to_rfc3339();
        let a = parse_line(&line("1", &ts, "claude-sonnet-4-5", 1_000_000, 0, 0, 0)).unwrap();
        let b = parse_line(&line("2", &yesterday, "claude-opus-4-5", 1_000_000, 0, 0, 0)).unwrap();
        let s = summarize(&[a, b], &cfg, now);
        assert_eq!(s.daily.len() as i64, HISTORY_DAYS);
        assert!((s.daily.last().unwrap().cost_usd - 3.0).abs() < 1e-9);
        assert!((s.daily[s.daily.len() - 2].cost_usd - 5.0).abs() < 1e-9);
        assert_eq!(s.models[0].0, "claude-opus-4-5", "trié par coût décroissant");
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
