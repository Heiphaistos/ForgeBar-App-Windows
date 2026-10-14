//! Affichage conditionnel d'un widget : `always`, `media`, ou une comparaison
//! `métrique<valeur` / `métrique>valeur` (`battery<20`, `cpu>50`, `claude>80`).

/// Métriques comparables ; `battery` est le niveau restant, les autres des pourcentages d'usage.
pub const METRICS: &[&str] = &["battery", "cpu", "ram", "gpu", "claude"];

#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    Always,
    /// Un média est en lecture.
    Media,
    Below(String, f32),
    Above(String, f32),
}

impl Condition {
    pub fn parse(s: &str) -> Result<Self, String> {
        let s = s.trim();
        match s {
            "" | "always" => return Ok(Self::Always),
            "media" => return Ok(Self::Media),
            _ => {}
        }
        let (op_at, below) = match (s.find('<'), s.find('>')) {
            (Some(i), None) => (i, true),
            (None, Some(i)) => (i, false),
            _ => return Err(format!("condition inconnue « {s} » (always, media, battery<20, cpu>50…)")),
        };
        let (metric, value) = (s[..op_at].trim(), s[op_at + 1..].trim());
        if !METRICS.contains(&metric) {
            return Err(format!("métrique inconnue « {metric} » ({})", METRICS.join(", ")));
        }
        let v: f32 = value.replace(',', ".").parse().map_err(|_| format!("valeur invalide « {value} »"))?;
        Ok(if below { Self::Below(metric.into(), v) } else { Self::Above(metric.into(), v) })
    }

    /// `lookup` donne la valeur courante d'une métrique, `None` si elle est indisponible
    /// (le widget est alors masqué par une comparaison, jamais affiché à tort).
    pub fn holds(&self, media_playing: bool, lookup: impl Fn(&str) -> Option<f32>) -> bool {
        match self {
            Self::Always => true,
            Self::Media => media_playing,
            Self::Below(m, v) => lookup(m).is_some_and(|x| x < *v),
            Self::Above(m, v) => lookup(m).is_some_and(|x| x > *v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_evaluates() {
        let c = Condition::parse("battery<20").unwrap();
        assert!(c.holds(false, |_| Some(15.0)));
        assert!(!c.holds(false, |_| Some(50.0)));
        assert!(!c.holds(false, |_| None));
        assert_eq!(Condition::parse("always").unwrap(), Condition::Always);
        assert!(Condition::parse("media").unwrap().holds(true, |_| None));
        assert!(Condition::parse("cpu>50").unwrap().holds(false, |_| Some(60.0)));
    }

    #[test]
    fn rejects_garbage() {
        assert!(Condition::parse("foo<3").is_err());
        assert!(Condition::parse("cpu>x").is_err());
        assert!(Condition::parse("cpu<1>2").is_err());
    }
}
