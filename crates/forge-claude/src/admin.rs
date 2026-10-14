//! Admin API d'Anthropic : coûts réels de l'organisation (`GET /v1/organizations/cost_report`).
//!
//! Ce module ne fait aucun accès réseau : il construit la requête et lit la réponse.
//! L'appel HTTP et la lecture de la clé (Gestionnaire d'identification) sont dans l'exécutable.
//! Les montants arrivent dans la plus petite unité de la devise (centimes) sous forme de chaîne décimale.

use chrono::{DateTime, Datelike, Local, Utc};
use serde_json::Value;

pub const HOST: &str = "api.anthropic.com";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct OrgCost {
    pub today_usd: f64,
    pub month_usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Bucket {
    pub start: DateTime<Utc>,
    pub usd: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub buckets: Vec<Bucket>,
    pub next_page: Option<String>,
}

/// Chemin et paramètres de la requête : tranches d'un jour depuis `starting_at`, 31 au maximum par page.
pub fn request_path(starting_at: DateTime<Utc>, page: Option<&str>) -> String {
    let mut p = format!(
        "/v1/organizations/cost_report?starting_at={}&bucket_width=1d&limit=31",
        starting_at.format("%Y-%m-%dT%H:%M:%SZ")
    );
    if let Some(page) = page {
        p.push_str("&page=");
        p.extend(page.bytes().map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        }));
    }
    p
}

pub fn parse_page(body: &str) -> Result<Page, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| format!("réponse illisible : {e}"))?;
    if let Some(err) = v.get("error") {
        let msg = err.get("message").and_then(Value::as_str).unwrap_or("erreur inconnue");
        return Err(format!("Admin API : {msg}"));
    }
    let data = v.get("data").and_then(Value::as_array).ok_or("réponse sans champ data")?;
    let mut buckets = Vec::new();
    for b in data {
        let start = b.get("starting_at").and_then(Value::as_str).ok_or("tranche sans starting_at")?;
        let start = DateTime::parse_from_rfc3339(start).map_err(|e| e.to_string())?.with_timezone(&Utc);
        let cents: f64 = b
            .get("results")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter(|x| x.get("currency").and_then(Value::as_str).unwrap_or("USD") == "USD"))
            .into_iter()
            .flatten()
            .filter_map(|x| x.get("amount").and_then(Value::as_str)?.parse::<f64>().ok())
            .sum();
        buckets.push(Bucket { start, usd: cents / 100.0 });
    }
    let next_page = v.get("next_page").and_then(Value::as_str).map(str::to_string);
    Ok(Page { buckets, next_page })
}

/// Coût du jour et du mois à partir des tranches journalières, qui couvrent chacune un jour UTC
/// (c'est le découpage de la facturation Anthropic).
pub fn org_cost(buckets: &[Bucket], now: DateTime<Local>) -> OrgCost {
    let today = now.with_timezone(&Utc).date_naive();
    let mut out = OrgCost::default();
    for b in buckets {
        let d = b.start.date_naive();
        if d.year() == today.year() && d.month() == today.month() {
            out.month_usd += b.usd;
        }
        if d == today {
            out.today_usd += b.usd;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const BODY: &str = r#"{"data":[
      {"starting_at":"2026-10-14T00:00:00Z","ending_at":"2026-10-15T00:00:00Z","results":[{"amount":"250.5","currency":"USD"},{"amount":"49.5","currency":"USD"}]},
      {"starting_at":"2026-10-15T00:00:00Z","ending_at":"2026-10-16T00:00:00Z","results":[]}],
      "has_more":true,"next_page":"page_abc=="}"#;

    #[test]
    fn parses_cents_into_dollars() {
        let p = parse_page(BODY).unwrap();
        assert_eq!(p.buckets.len(), 2);
        assert!((p.buckets[0].usd - 3.0).abs() < 1e-9, "300 centimes = 3 $");
        assert_eq!(p.buckets[1].usd, 0.0);
        assert_eq!(p.next_page.as_deref(), Some("page_abc=="));
    }

    #[test]
    fn reports_api_errors_and_garbage() {
        let e = parse_page(r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#);
        assert!(e.unwrap_err().contains("invalid x-api-key"));
        assert!(parse_page("pas du json").is_err());
        assert!(parse_page("{}").is_err());
    }

    #[test]
    fn totals_for_today_and_month() {
        let p = parse_page(BODY).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 10, 15, 12, 0, 0).unwrap().with_timezone(&Local);
        let c = org_cost(&p.buckets, now);
        assert!((c.month_usd - 3.0).abs() < 1e-9);
        assert_eq!(c.today_usd, 0.0);
    }

    #[test]
    fn path_encodes_page_token() {
        let t = Utc.with_ymd_and_hms(2026, 10, 1, 0, 0, 0).unwrap();
        let p = request_path(t, Some("page_a=="));
        assert!(p.starts_with("/v1/organizations/cost_report?starting_at=2026-10-01T00:00:00Z"));
        assert!(p.ends_with("&page=page_a%3D%3D"));
    }
}
