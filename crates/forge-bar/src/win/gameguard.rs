//! Barre visible en jeu sans jamais gêner le jeu. Logique pure (l'heure est passée en paramètre), testable.
//!
//! Windows ne permet pas d'afficher une fenêtre par-dessus un vrai plein écran exclusif sans que le jeu en sorte
//! (il repasse en fenêtré). Si la barre s'obstine, le jeu bascule en boucle et la barre clignote. Règle : en plein écran
//! exclusif, ou si le plein écran bascule à répétition, la barre se retire d'elle-même et ne revient que quand tout
//! est calme ; en plein écran sans bordure (ou fenêtré), elle reste visible.

use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Masquer les barres : elles gêneraient le jeu.
    Hide,
    /// Les ré-afficher : le jeu est fermé ou calme.
    Show,
    /// Les remettre au premier plan (un plein écran sans bordure vient de passer devant).
    Raise,
}

#[derive(Debug, Default)]
pub struct GameGuard {
    hidden: bool,
    was_fullscreen: bool,
    flips: VecDeque<Duration>,
    calm_ticks: u32,
    since_raise: u32,
}

/// Plein écran qui bascule 3 fois en 15 s = le jeu et la barre se disputent l'écran.
const FLIP_LIMIT: usize = 3;
const FLIP_WINDOW: Duration = Duration::from_secs(15);
/// Secondes de calme (hors plein écran, hors exclusif) avant de ré-afficher.
const CALM_NEEDED: u32 = 3;

impl GameGuard {
    #[cfg(test)]
    pub fn hidden(&self) -> bool {
        self.hidden
    }

    /// Appelé une fois par seconde : `fullscreen` = une fenêtre sans cadre couvre l'écran au premier plan,
    /// `exclusive` = un jeu Direct3D est en plein écran exclusif.
    pub fn update(&mut self, now: Duration, fullscreen: bool, exclusive: bool) -> Action {
        let entered = fullscreen && !self.was_fullscreen;
        if fullscreen != self.was_fullscreen {
            self.flips.push_back(now);
            self.was_fullscreen = fullscreen;
        }
        while self.flips.front().is_some_and(|t| now.saturating_sub(*t) > FLIP_WINDOW) {
            self.flips.pop_front();
        }
        let must_hide = exclusive || self.flips.len() >= FLIP_LIMIT;
        if must_hide {
            self.calm_ticks = 0;
            if !self.hidden {
                self.hidden = true;
                return Action::Hide;
            }
            return Action::Nothing;
        }
        if self.hidden {
            self.calm_ticks = if fullscreen { 0 } else { self.calm_ticks + 1 };
            if self.calm_ticks >= CALM_NEEDED {
                self.hidden = false;
                self.flips.clear();
                return Action::Show;
            }
            return Action::Nothing;
        }
        // Plein écran sans bordure : on remonte la barre une fois, puis de temps en temps si un autre topmost passe devant.
        if fullscreen {
            self.since_raise += 1;
            if entered || self.since_raise >= 5 {
                self.since_raise = 0;
                return Action::Raise;
            }
        }
        Action::Nothing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn borderless_fullscreen_keeps_the_bar_and_raises_it() {
        let mut g = GameGuard::default();
        assert_eq!(g.update(s(0), false, false), Action::Nothing);
        assert_eq!(g.update(s(1), true, false), Action::Raise);
        assert!(!g.hidden());
        assert_eq!(g.update(s(2), true, false), Action::Nothing);
    }

    #[test]
    fn exclusive_fullscreen_hides_then_returns_after_calm() {
        let mut g = GameGuard::default();
        assert_eq!(g.update(s(0), true, true), Action::Hide);
        assert_eq!(g.update(s(1), true, true), Action::Nothing);
        // le jeu se ferme : pas de retour avant 3 secondes calmes
        assert_eq!(g.update(s(2), false, false), Action::Nothing);
        assert_eq!(g.update(s(3), false, false), Action::Nothing);
        assert_eq!(g.update(s(4), false, false), Action::Show);
        assert!(!g.hidden());
    }

    #[test]
    fn flapping_fullscreen_hides_the_bar() {
        let mut g = GameGuard::default();
        let mut hid = false;
        for (t, fs) in [(0, true), (1, false), (2, true), (3, false)] {
            hid |= g.update(s(t), fs, false) == Action::Hide;
        }
        assert!(hid, "le plein écran a basculé 3 fois en 15 s");
    }

    #[test]
    fn slow_changes_do_not_count_as_flapping() {
        let mut g = GameGuard::default();
        for (t, fs) in [(0, true), (20, false), (40, true), (60, false)] {
            assert_ne!(g.update(s(t), fs, false), Action::Hide);
        }
    }
}
