//! Grille du calendrier mensuel (lundi en premier, semaines ISO), sans dépendance Windows.
#![cfg_attr(not(windows), allow(dead_code))]

use chrono::{Datelike, NaiveDate};

pub const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];
pub const DAYS: [&str; 7] = ["lu", "ma", "me", "je", "ve", "sa", "di"];

#[derive(Debug, Clone, PartialEq)]
pub struct Week {
    pub iso: u32,
    pub days: [Option<u32>; 7],
}

#[derive(Debug, Clone, PartialEq)]
pub struct MonthGrid {
    pub title: String,
    pub weeks: Vec<Week>,
    /// Jour du mois à mettre en évidence (aujourd'hui), si ce mois est le mois courant.
    pub today: Option<u32>,
}

pub fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let idx = year * 12 + month as i32 - 1 + delta;
    (idx.div_euclid(12), idx.rem_euclid(12) as u32 + 1)
}

pub fn month_grid(year: i32, month: u32, today: NaiveDate) -> MonthGrid {
    let first = NaiveDate::from_ymd_opt(year, month, 1).expect("mois valide");
    let (ny, nm) = shift_month(year, month, 1);
    let len = NaiveDate::from_ymd_opt(ny, nm, 1).expect("mois suivant").signed_duration_since(first).num_days() as u32;
    let lead = first.weekday().num_days_from_monday();
    let mut weeks = Vec::new();
    let mut day = 1 - lead as i32;
    while day <= len as i32 {
        let mut days = [None; 7];
        for (i, d) in days.iter_mut().enumerate() {
            let n = day + i as i32;
            if (1..=len as i32).contains(&n) {
                *d = Some(n as u32);
            }
        }
        let anchor = days.iter().flatten().next().copied().unwrap_or(1);
        let iso = NaiveDate::from_ymd_opt(year, month, anchor).expect("jour valide").iso_week().week();
        weeks.push(Week { iso, days });
        day += 7;
    }
    let is_current = today.year() == year && today.month() == month;
    MonthGrid {
        title: format!("{} {year}", MONTHS[month as usize - 1]),
        weeks,
        today: is_current.then_some(today.day()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn october_2026_starts_on_thursday() {
        let g = month_grid(2026, 10, NaiveDate::from_ymd_opt(2026, 10, 15).unwrap());
        assert_eq!(g.title, "octobre 2026");
        assert_eq!(g.weeks[0].days, [None, None, None, Some(1), Some(2), Some(3), Some(4)]);
        assert_eq!(g.weeks.len(), 5);
        assert_eq!(g.weeks[4].days[5], Some(31));
        assert_eq!(g.today, Some(15));
        assert_eq!(g.weeks[0].iso, 40);
    }

    #[test]
    fn shifting_wraps_years() {
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 3, 0), (2026, 3));
    }

    #[test]
    fn other_month_has_no_today() {
        let g = month_grid(2026, 2, NaiveDate::from_ymd_opt(2026, 10, 15).unwrap());
        assert_eq!(g.today, None);
        assert_eq!(g.weeks.iter().flat_map(|w| w.days).flatten().count(), 28);
    }
}
