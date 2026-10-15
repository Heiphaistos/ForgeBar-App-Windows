//! Minuteur et chronomètre du widget « timer » : logique pure (l'heure est passée en paramètre) pour être testable.

use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Stopwatch,
    /// Minuteur : durée totale.
    Countdown(Duration),
}

#[derive(Debug, Clone)]
pub struct Timer {
    pub mode: Mode,
    /// Temps déjà écoulé avant le dernier démarrage.
    elapsed: Duration,
    /// Instant (fourni par l'appelant) du dernier démarrage ; `None` à l'arrêt.
    since: Option<Duration>,
}

impl Default for Timer {
    fn default() -> Self {
        Self { mode: Mode::Stopwatch, elapsed: Duration::ZERO, since: None }
    }
}

impl Timer {
    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    /// Temps écoulé à l'instant `now` (horloge monotone de l'appelant).
    pub fn elapsed(&self, now: Duration) -> Duration {
        self.elapsed + self.since.map_or(Duration::ZERO, |s| now.saturating_sub(s))
    }

    pub fn toggle(&mut self, now: Duration) {
        match self.since.take() {
            Some(s) => self.elapsed += now.saturating_sub(s),
            None => self.since = Some(now),
        }
    }

    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
        self.since = None;
    }

    /// Change de mode, remet à zéro et démarre le minuteur aussitôt.
    pub fn start_countdown(&mut self, total: Duration, now: Duration) {
        self.mode = Mode::Countdown(total);
        self.elapsed = Duration::ZERO;
        self.since = Some(now);
    }

    pub fn set_stopwatch(&mut self) {
        self.mode = Mode::Stopwatch;
        self.reset();
    }

    /// Vrai une fois quand un minuteur en marche arrive à zéro (il s'arrête alors).
    pub fn finished(&mut self, now: Duration) -> bool {
        if let (Mode::Countdown(total), true) = (self.mode, self.running()) {
            if self.elapsed(now) >= total {
                self.elapsed = total;
                self.since = None;
                return true;
            }
        }
        false
    }

    /// « 12:34 » ou « 1:02:03 » ; le minuteur affiche le temps restant.
    pub fn text(&self, now: Duration) -> String {
        let shown = match self.mode {
            Mode::Stopwatch => self.elapsed(now),
            Mode::Countdown(total) => total.saturating_sub(self.elapsed(now)),
        };
        let s = shown.as_secs();
        if s >= 3600 {
            format!("{}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
        } else {
            format!("{:02}:{:02}", s / 60, s % 60)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn stopwatch_runs_pauses_and_resets() {
        let mut t = Timer::default();
        assert_eq!(t.text(s(100)), "00:00");
        t.toggle(s(100));
        assert_eq!(t.text(s(165)), "01:05");
        t.toggle(s(165));
        assert_eq!(t.text(s(900)), "01:05", "en pause, le temps ne bouge plus");
        t.toggle(s(900));
        assert_eq!(t.text(s(910)), "01:15");
        t.reset();
        assert!(!t.running());
        assert_eq!(t.text(s(2000)), "00:00");
    }

    #[test]
    fn countdown_shows_remaining_and_finishes_once() {
        let mut t = Timer::default();
        t.start_countdown(s(90), s(10));
        assert_eq!(t.text(s(10)), "01:30");
        assert_eq!(t.text(s(40)), "01:00");
        assert!(!t.finished(s(99)));
        assert!(t.finished(s(100)));
        assert!(!t.finished(s(101)), "ne sonne qu'une fois");
        assert_eq!(t.text(s(500)), "00:00");
        assert_eq!(Timer { mode: Mode::Stopwatch, ..Timer::default() }.text(s(3661)), "00:00");
        let mut long = Timer::default();
        long.toggle(s(0));
        assert_eq!(long.text(s(3723)), "1:02:03");
    }
}
