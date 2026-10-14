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
    /// Zones définies à la main dans `left`, `center`, `right`.
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Zones {
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
}

impl Default for Layout {
    fn default() -> Self {
        Self { preset: LayoutPreset::Split, left: vec![], center: vec![], right: vec![] }
    }
}

fn v(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

const ALL: &[&str] = &["desktop", "claude", "cpu", "ram", "wifi", "sound", "battery", "clock"];

impl Layout {
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
                left: v(&["desktop"]),
                center: v(&["claude"]),
                right: v(&["cpu", "ram", "wifi", "sound", "battery", "clock"]),
            },
            LayoutPreset::HudFirst => {
                Zones { left: v(&["claude"]), center: vec![], right: v(&["wifi", "sound", "battery", "clock"]) }
            }
            LayoutPreset::Custom => unreachable!(),
        }
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
        let l = Layout { preset: LayoutPreset::Centered, left: v(&["clock"]), center: vec![], right: vec![] };
        assert_eq!(l.zones().left, v(&["clock"]));
    }

    #[test]
    fn split_puts_claude_in_center() {
        assert_eq!(Layout::default().zones().center, v(&["claude"]));
    }
}
