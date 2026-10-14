//! Météo Open-Meteo : construction des URL et lecture des réponses (pas d'accès réseau ici).
#![cfg_attr(not(windows), allow(dead_code))]

use forge_config::TempUnit;
use serde_json::Value;

pub const FORECAST_HOST: &str = "api.open-meteo.com";
pub const GEOCODING_HOST: &str = "geocoding-api.open-meteo.com";

#[derive(Debug, Clone, PartialEq)]
pub struct Now {
    pub temp: f32,
    pub code: u32,
    pub is_day: bool,
    pub unit: TempUnit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

pub fn geocoding_path(city: &str) -> String {
    format!("/v1/search?name={}&count=1&language=fr&format=json", encode(city))
}

pub fn forecast_path(lat: f64, lon: f64, unit: TempUnit) -> String {
    let u = if unit == TempUnit::Fahrenheit { "fahrenheit" } else { "celsius" };
    format!("/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,weather_code,is_day&temperature_unit={u}")
}

pub fn parse_place(body: &str) -> Result<Place, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let r = v.get("results").and_then(|r| r.get(0)).ok_or("ville introuvable")?;
    Ok(Place {
        name: r.get("name").and_then(Value::as_str).unwrap_or_default().to_string(),
        lat: r.get("latitude").and_then(Value::as_f64).ok_or("latitude absente")?,
        lon: r.get("longitude").and_then(Value::as_f64).ok_or("longitude absente")?,
    })
}

pub fn parse_now(body: &str, unit: TempUnit) -> Result<Now, String> {
    let v: Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let c = v.get("current").ok_or("réponse sans bloc current")?;
    Ok(Now {
        temp: c.get("temperature_2m").and_then(Value::as_f64).ok_or("température absente")? as f32,
        code: c.get("weather_code").and_then(Value::as_u64).unwrap_or(0) as u32,
        is_day: c.get("is_day").and_then(Value::as_u64).unwrap_or(1) != 0,
        unit,
    })
}

impl Now {
    /// Symbole Unicode (présent dans Segoe UI Symbol) correspondant au code WMO.
    pub fn symbol(&self) -> char {
        match self.code {
            0 | 1 => {
                if self.is_day {
                    '\u{2600}'
                } else {
                    '\u{263E}'
                }
            }
            2 => '\u{26C5}',
            3 | 45 | 48 => '\u{2601}',
            51..=67 | 80..=82 => '\u{2602}',
            71..=77 | 85 | 86 => '\u{2744}',
            95..=99 => '\u{26A1}',
            _ => '\u{2601}',
        }
    }

    pub fn text(&self) -> String {
        let u = if self.unit == TempUnit::Fahrenheit { "°F" } else { "°C" };
        format!("{:.0}{u}", self.temp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forecast() {
        let n = parse_now(
            r#"{"current":{"time":"2026-10-15T12:00","temperature_2m":12.6,"weather_code":61,"is_day":1}}"#,
            TempUnit::Celsius,
        )
        .unwrap();
        assert_eq!(n.text(), "13°C");
        assert_eq!(n.symbol(), '\u{2602}');
        assert!(parse_now("{}", TempUnit::Celsius).is_err());
    }

    #[test]
    fn parses_geocoding_and_builds_paths() {
        let p = parse_place(r#"{"results":[{"name":"Lyon","latitude":45.75,"longitude":4.85}]}"#).unwrap();
        assert_eq!(p, Place { name: "Lyon".into(), lat: 45.75, lon: 4.85 });
        assert!(parse_place(r#"{"generationtime_ms":0.2}"#).is_err());
        assert!(geocoding_path("Saint-Étienne").contains("name=Saint-%C3%89tienne"));
        assert!(forecast_path(45.75, 4.85, TempUnit::Fahrenheit).contains("temperature_unit=fahrenheit"));
    }

    #[test]
    fn night_clear_is_moon() {
        let n = Now { temp: 3.0, code: 0, is_day: false, unit: TempUnit::Celsius };
        assert_eq!(n.symbol(), '\u{263E}');
    }
}
