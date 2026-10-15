use serde::{Deserialize, Serialize};

/// Dispositions prêtes à l'emploi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutPreset {
    /// Tous les widgets alignés au début de la barre.
    Classic,
    /// Groupe principal au centre, comme la barre des tâches de Windows 11.
    Centered,
    /// Raccourcis au début, HUD Claude au centre, système à la fin.
    Split,
    /// Chaque widget dans sa pastille, fond invisible entre elles.
    Islands,
    /// Icônes seules quand le widget en a une.
    Compact,
    /// HUD Claude mis en avant, système réduit.
    HudFirst,
    /// Deux rangées : système en haut, météo, média et réseau en bas.
    DoubleRow,
    /// Zones définies à la main dans `left`, `center`, `right`.
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Zones {
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
}

/// Zones de la deuxième rangée (dispositions double ligne), écrites à la main.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RowZones {
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub preset: LayoutPreset,
    pub left: Vec<String>,
    pub center: Vec<String>,
    pub right: Vec<String>,
    /// Deuxième rangée, prioritaire sur celle du préréglage.
    pub row2: RowZones,
    /// Widgets que cette disposition connaît déjà : un widget apparu dans une version plus récente et absent d'ici
    /// est ajouté automatiquement aux zones écrites à la main (voir `adopt_new_widgets`).
    pub seen: Vec<String>,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            preset: LayoutPreset::Split,
            left: vec![],
            center: vec![],
            right: vec![],
            row2: RowZones::default(),
            seen: vec![],
        }
    }
}

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub const ALL: &[&str] = &[
    "desktop",
    "apps",
    "media",
    "claude",
    "cpu",
    "ram",
    "gpu",
    "temp",
    "disk",
    "fps",
    "net",
    "weather",
    "clipboard",
    "wifi",
    "sound",
    "battery",
    "clock",
    "toggle",
];

/// Widgets qui existaient avant l'introduction de `Layout::seen` (0.5.0) : une disposition ancienne les connaît.
const LEGACY: &[&str] = &[
    "desktop",
    "media",
    "claude",
    "cpu",
    "ram",
    "gpu",
    "net",
    "weather",
    "clipboard",
    "wifi",
    "sound",
    "battery",
    "clock",
];

impl Layout {
    /// Une disposition écrite à la main ne reçoit pas les widgets créés après elle : l'utilisateur mettait à jour la
    /// barre et ne voyait jamais les FPS ou les températures. Les widgets jamais vus sont donc ajoutés (miniatures
    /// au début, le reste avant le réseau) puis marqués comme vus : un widget retiré ensuite à la main reste retiré.
    /// Renvoie vrai si la disposition a changé (à réécrire sur disque).
    pub fn adopt_new_widgets(&mut self) -> bool {
        let hand_written = !(self.left.is_empty() && self.center.is_empty() && self.right.is_empty());
        let placed: Vec<String> = self
            .left
            .iter()
            .chain(&self.center)
            .chain(&self.right)
            .chain(&self.row2.left)
            .chain(&self.row2.center)
            .chain(&self.row2.right)
            .cloned()
            .collect();
        let mut seen: Vec<String> =
            if self.seen.is_empty() { LEGACY.iter().map(|s| s.to_string()).collect() } else { self.seen.clone() };
        let mut changed = false;
        if hand_written {
            for id in ALL.iter().filter(|id| !seen.iter().any(|s| s == *id) && !placed.iter().any(|p| p == *id)) {
                match *id {
                    "apps" => {
                        let at = self.left.iter().position(|w| w == "desktop").map_or(0, |i| i + 1);
                        self.left.insert(at, id.to_string());
                    }
                    "toggle" => self.right.push(id.to_string()),
                    _ => {
                        let at = self.right.iter().position(|w| w == "net").unwrap_or(self.right.len());
                        self.right.insert(at, id.to_string());
                    }
                }
                changed = true;
            }
        }
        for id in ALL {
            if !seen.iter().any(|s| s == id) {
                seen.push(id.to_string());
            }
        }
        if self.seen != seen {
            self.seen = seen;
            changed = true;
        }
        changed
    }

    /// Zones effectives : celles écrites à la main si présentes, sinon celles du préréglage.
    pub fn zones(&self) -> Zones {
        let hand_written = !(self.left.is_empty() && self.center.is_empty() && self.right.is_empty());
        if self.preset == LayoutPreset::Custom || hand_written {
            return Zones { left: self.left.clone(), center: self.center.clone(), right: self.right.clone() };
        }
        match self.preset {
            LayoutPreset::Classic | LayoutPreset::Compact => Zones { left: v(ALL), ..Zones::default() },
            LayoutPreset::Centered => Zones { center: v(ALL), ..Zones::default() },
            LayoutPreset::Split | LayoutPreset::Islands => Zones {
                left: v(&["desktop", "apps", "media"]),
                center: v(&["claude"]),
                right: v(&[
                    "cpu", "ram", "gpu", "temp", "disk", "fps", "net", "weather", "wifi", "sound", "battery", "clock",
                    "toggle",
                ]),
            },
            LayoutPreset::DoubleRow => {
                Zones { left: v(&["desktop"]), center: v(&["claude"]), right: v(&["cpu", "ram", "gpu", "clock"]) }
            }
            LayoutPreset::HudFirst => Zones {
                left: v(&["claude"]),
                center: vec![],
                right: v(&["weather", "wifi", "sound", "battery", "clock"]),
            },
            LayoutPreset::Custom => unreachable!(),
        }
    }

    /// Rangées à dessiner, de la première à la dernière. Une seule sauf en double ligne.
    pub fn rows(&self) -> Vec<Zones> {
        let mut rows = vec![self.zones()];
        let r = &self.row2;
        let hand_written = !(r.left.is_empty() && r.center.is_empty() && r.right.is_empty());
        if hand_written {
            rows.push(Zones { left: r.left.clone(), center: r.center.clone(), right: r.right.clone() });
        } else if self.preset == LayoutPreset::DoubleRow {
            rows.push(Zones {
                left: v(&["media", "weather"]),
                center: vec![],
                right: v(&["net", "wifi", "sound", "battery"]),
            });
        }
        rows
    }

    pub fn icons_only(&self) -> bool {
        self.preset == LayoutPreset::Compact
    }

    pub fn pills(&self) -> bool {
        self.preset == LayoutPreset::Islands
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hand_written_zones_override_preset() {
        let l = Layout { preset: LayoutPreset::Centered, left: v(&["clock"]), ..Layout::default() };
        assert_eq!(l.zones().left, v(&["clock"]));
    }

    #[test]
    fn old_hand_written_layout_gets_new_widgets_once() {
        let mut l = Layout {
            left: v(&["desktop", "media"]),
            center: v(&["claude"]),
            right: v(&["cpu", "ram", "net", "clock"]),
            ..Layout::default()
        };
        assert!(l.adopt_new_widgets());
        assert_eq!(l.left, v(&["desktop", "apps", "media"]));
        assert_eq!(l.right, v(&["cpu", "ram", "temp", "disk", "fps", "net", "clock", "toggle"]));
        // Retiré à la main ensuite : il ne revient pas.
        l.right.retain(|w| w != "fps");
        assert!(!l.adopt_new_widgets());
        assert!(!l.right.contains(&"fps".to_string()));
    }

    #[test]
    fn preset_layout_is_left_alone() {
        let mut l = Layout::default();
        l.adopt_new_widgets();
        assert!(l.left.is_empty() && l.right.is_empty(), "un préréglage fournit déjà ses widgets");
    }

    #[test]
    fn double_row_has_two_rows() {
        let l = Layout { preset: LayoutPreset::DoubleRow, ..Layout::default() };
        assert_eq!(l.rows().len(), 2);
        assert_eq!(Layout::default().rows().len(), 1);
    }

    #[test]
    fn split_puts_claude_in_center() {
        assert_eq!(Layout::default().zones().center, v(&["claude"]));
    }
}
