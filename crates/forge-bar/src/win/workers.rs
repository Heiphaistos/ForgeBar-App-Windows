//! Fils de fond : logs Claude (+ Admin API) et météo. Ils ne touchent jamais l'interface :
//! ils déposent leur résultat dans un verrou partagé puis postent un message à la fenêtre principale.

use super::{cred, http};
use crate::weather;
use chrono::{Datelike, TimeZone, Utc};
use forge_claude::{admin, Scanner, Summary};
use forge_config::{ClaudeConfig, Weather};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

fn post(main: isize, msg: u32) {
    unsafe {
        let _ = PostMessageW(HWND(main as _), msg, WPARAM(0), LPARAM(0));
    }
}

/// Dort `secs` secondes, ou moins si `wake` est levé (configuration rechargée).
fn nap(secs: u64, wake: &AtomicBool) {
    for _ in 0..secs {
        std::thread::sleep(Duration::from_secs(1));
        if wake.swap(false, Ordering::Relaxed) {
            break;
        }
    }
}

/// Coûts du mois en cours depuis l'Admin API (jusqu'à trois pages de 31 jours).
fn fetch_org() -> Result<admin::OrgCost, String> {
    let key = cred::read_admin_key()?;
    let now = chrono::Local::now();
    let month_start = Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0).single().ok_or("date invalide")?;
    let mut buckets = Vec::new();
    let mut page: Option<String> = None;
    for _ in 0..3 {
        let path = admin::request_path(month_start, page.as_deref());
        let body = http::get(
            admin::HOST,
            &path,
            &[("x-api-key", key.as_str()), ("anthropic-version", admin::ANTHROPIC_VERSION)],
        )?;
        let p = admin::parse_page(&body)?;
        buckets.extend(p.buckets);
        page = p.next_page;
        if page.is_none() {
            break;
        }
    }
    Ok(admin::org_cost(&buckets, now))
}

/// Relit les logs Claude Code (de façon incrémentale) puis, si la source `api` est active,
/// interroge l'Admin API au plus toutes les 5 minutes.
pub fn spawn_claude(
    main: HWND,
    msg: u32,
    out: Arc<Mutex<Summary>>,
    cfg: Arc<Mutex<ClaudeConfig>>,
    wake: Arc<AtomicBool>,
) {
    let main = main.0 as isize;
    std::thread::spawn(move || {
        let mut scanner = Scanner::default();
        let mut org: (Option<admin::OrgCost>, Option<String>) = (None, None);
        let mut org_at: Option<Instant> = None;
        loop {
            let c = cfg.lock().unwrap().clone();
            if c.enabled {
                let mut s = if c.sources.iter().any(|s| s == "claude_code") {
                    scanner.scan(&c, chrono::Local::now())
                } else {
                    Summary::default()
                };
                if c.sources.iter().any(|s| s == "api") {
                    if org_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(300)) {
                        org_at = Some(Instant::now());
                        org = match fetch_org() {
                            Ok(o) => (Some(o), None),
                            Err(e) => (org.0, Some(e)),
                        };
                    }
                    (s.org, s.org_error) = org.clone();
                }
                *out.lock().unwrap() = s;
                post(main, msg);
            }
            nap(c.refresh_seconds.max(5), &wake);
        }
    });
}

/// Météo Open-Meteo : résout la ville une fois, puis relit les conditions toutes les `refresh_minutes`.
pub fn spawn_weather(
    main: HWND,
    msg: u32,
    out: Arc<Mutex<Option<weather::Now>>>,
    cfg: Arc<Mutex<Weather>>,
    wake: Arc<AtomicBool>,
) {
    let main = main.0 as isize;
    std::thread::spawn(move || {
        let mut place: Option<(String, f64, f64)> = None; // (ville demandée, lat, lon)
        loop {
            let c = cfg.lock().unwrap().clone();
            let result = if !c.enabled {
                Ok(None)
            } else {
                (|| -> Result<Option<weather::Now>, String> {
                    let (lat, lon) = match (c.latitude, c.longitude) {
                        (Some(lat), Some(lon)) => (lat, lon),
                        _ if c.city.is_empty() => return Err("ville ou coordonnées requises".into()),
                        _ => match &place {
                            Some((city, lat, lon)) if *city == c.city => (*lat, *lon),
                            _ => {
                                let body = http::get(weather::GEOCODING_HOST, &weather::geocoding_path(&c.city), &[])?;
                                let p = weather::parse_place(&body)?;
                                place = Some((c.city.clone(), p.lat, p.lon));
                                (p.lat, p.lon)
                            }
                        },
                    };
                    let body = http::get(weather::FORECAST_HOST, &weather::forecast_path(lat, lon, c.unit), &[])?;
                    weather::parse_now(&body, c.unit).map(Some)
                })()
            };
            let value = result.unwrap_or(None);
            let changed = {
                let mut cur = out.lock().unwrap();
                let changed = *cur != value;
                *cur = value;
                changed
            };
            if changed {
                post(main, msg);
            }
            nap(c.refresh_minutes.max(5) * 60, &wake);
        }
    });
}
