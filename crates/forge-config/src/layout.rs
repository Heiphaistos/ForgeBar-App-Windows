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
}

impl Default for Layout {
    fn default() -> Self {
        Self { preset: LayoutPreset::Split, left: vec![], center: vec![], right: vec![], row2: RowZones::default() }
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
    "disk",
    "net",
    "weather",
    "clipboard",
    "wifi",
    "sound",
    "battery",
    "clock",
    "toggle",
];

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
                left: v(&["desktop", "apps", "media"]),
                center: v(&["claude"]),
                right: v(&["cpu", "ram", "gpu", "disk", "net", "weather", "wifi", "sound", "battery", "clock", "toggle"]),
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
