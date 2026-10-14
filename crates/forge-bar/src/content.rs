//! Contenu des panneaux (calendrier, détail Claude, presse-papiers, HUD flottant) décrit comme une
//! liste d'opérations de dessin. Aucune dépendance Windows : testable partout, rendue par `win::panel`.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::calendar::{MonthGrid, DAYS};
use crate::widgets::{thresholds, truncate, usd};
use forge_claude::{fmt_tokens, Summary};
use forge_config::ClaudeConfig;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Role {
    Text,
    Dim,
    Accent,
    Warn,
    Crit,
    /// Fond de jauge et de cases discrètes.
    Track,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct R {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CalPrev,
    CalNext,
    CalToday,
    ClipPick(usize),
    ClipClear,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Fill { r: R, role: Role, radius: f32 },
    Text { r: R, s: String, role: Role, size: f32, bold: bool, align: Align },
    Hit { r: R, action: Action },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub w: f32,
    pub h: f32,
    pub ops: Vec<Op>,
}

const PAD: f32 = 16.0;

struct Builder {
    w: f32,
    y: f32,
    ops: Vec<Op>,
}

impl Builder {
    fn new(w: f32) -> Self {
        Self { w, y: PAD, ops: Vec::new() }
    }

    #[allow(clippy::too_many_arguments)]
    fn text(&mut self, x: f32, w: f32, h: f32, s: impl Into<String>, role: Role, size: f32, bold: bool, align: Align) {
        self.ops.push(Op::Text { r: R { x, y: self.y, w, h }, s: s.into(), role, size, bold, align });
    }

    fn line(&mut self, s: impl Into<String>, role: Role, size: f32, bold: bool) {
        let h = size + 8.0;
        self.text(PAD, self.w - 2.0 * PAD, h, s, role, size, bold, Align::Left);
        self.y += h;
    }

    /// Libellé à gauche, valeur à droite.
    fn pair(&mut self, left: &str, right: String, right_role: Role) {
        let inner = self.w - 2.0 * PAD;
        self.text(PAD, inner * 0.5, 20.0, left, Role::Dim, 12.0, false, Align::Left);
        self.text(PAD + inner * 0.4, inner * 0.6, 20.0, right, right_role, 12.0, false, Align::Right);
        self.y += 20.0;
    }

    fn gauge(&mut self, pct: f32, role: Role) {
        let track = R { x: PAD, y: self.y, w: self.w - 2.0 * PAD, h: 6.0 };
        self.ops.push(Op::Fill { r: track, role: Role::Track, radius: 3.0 });
        let fill = (pct / 100.0).clamp(0.0, 1.0) * track.w;
        if fill > 0.5 {
            self.ops.push(Op::Fill { r: R { w: fill.max(6.0), ..track }, role, radius: 3.0 });
        }
        self.y += 14.0;
    }

    /// Titre de section précédé d'un filet : sépare nettement les catégories du panneau.
    fn section(&mut self, title: &str) {
        self.y += 8.0;
        let rule = R { x: PAD, y: self.y, w: self.w - 2.0 * PAD, h: 1.0 };
        self.ops.push(Op::Fill { r: rule, role: Role::Track, radius: 0.0 });
        self.y += 8.0;
        self.text(PAD, self.w - 2.0 * PAD, 14.0, title.to_uppercase(), Role::Accent, 10.0, true, Align::Left);
        self.y += 20.0;
    }

    /// Ligne de tableau : libellé à gauche, deux colonnes alignées à droite.
    fn row3(&mut self, a: &str, b: &str, c: &str, role_a: Role, role_b: Role, bold: bool) {
        let inner = self.w - 2.0 * PAD;
        let h = 20.0;
        self.text(PAD, inner * 0.40, h, a, role_a, 12.0, bold, Align::Left);
        self.text(PAD + inner * 0.30, inner * 0.28, h, b, role_b, 12.0, bold, Align::Right);
        self.text(PAD + inner * 0.58, inner * 0.42, h, c, Role::Dim, 12.0, false, Align::Right);
        self.y += h;
    }

    fn gap(&mut self, h: f32) {
        self.y += h;
    }

    fn finish(self) -> Scene {
        Scene { w: self.w, h: self.y + PAD, ops: self.ops }
    }
}

fn role_for(pct: f32, cfg: &ClaudeConfig) -> Role {
    let (warn, crit) = thresholds(cfg);
    if pct >= crit {
        Role::Crit
    } else if pct >= warn {
        Role::Warn
    } else {
        Role::Accent
    }
}

fn hm(d: chrono::Duration) -> String {
    let m = d.num_minutes().max(0);
    format!("{} h {:02}", m / 60, m % 60)
}

/// Panneau détaillé du clic sur le widget Claude, rangé en catégories.
pub fn claude_detail(s: &Summary, cfg: &ClaudeConfig) -> Scene {
    let mut b = Builder::new(440.0);
    let inner = b.w - 2.0 * PAD;
    b.text(PAD, inner * 0.5, 24.0, "✦ Claude", Role::Accent, 16.0, true, Align::Left);
    let updated = s.updated.map_or("pas encore lu".to_string(), |t| t.format("%H:%M:%S").to_string());
    b.text(PAD + inner * 0.4, inner * 0.6, 24.0, format!("mis à jour {updated}"), Role::Dim, 11.0, false, Align::Right);
    b.y += 26.0;

    b.section("Session en cours");
    if let Some(sess) = &s.session {
        let role = role_for(sess.context_pct, cfg);
        b.line(truncate(&sess.model, 40), Role::Text, 13.0, true);
        b.gauge(sess.context_pct, role);
        b.row3(
            "Contexte utilisé",
            &format!("{:.0} %", sess.context_pct),
            &format!("{} tok", fmt_tokens(sess.context_tokens)),
            Role::Dim,
            role,
            false,
        );
    } else {
        b.line("Aucune session Claude Code récente.", Role::Dim, 12.0, false);
    }

    b.section("Consommation (estimée)");
    b.row3("", "Coût", "Tokens", Role::Dim, Role::Dim, false);
    for (label, t) in [("Aujourd'hui", &s.today), ("Cette semaine", &s.week), ("Ce mois", &s.month)] {
        b.row3(label, &usd(t.cost_usd), &fmt_tokens(t.tokens()), Role::Text, Role::Text, false);
    }
    if s.month.unpriced_tokens > 0 {
        b.line(
            format!("{} tok sans prix connu (modèle absent de la grille)", fmt_tokens(s.month.unpriced_tokens)),
            Role::Warn,
            11.0,
            false,
        );
    }

    let budgets = forge_claude::budgets(s, &cfg.budgets);
    if !budgets.is_empty() {
        b.section("Budgets");
        for bud in &budgets {
            let role = role_for(bud.pct, cfg);
            b.row3(
                &format!("Budget {}", bud.label),
                &format!("{:.0} %", bud.pct),
                &format!("{} / {}", usd(bud.used_usd), usd(bud.limit_usd)),
                Role::Text,
                role,
                false,
            );
            b.gauge(bud.pct, role);
        }
    }

    if let Some(blk) = &s.five_hour {
        b.section("Fenêtre de 5 heures (estimation)");
        let left = hm(blk.remaining(chrono::Utc::now()));
        match blk.pct {
            Some(p) => {
                let role = role_for(p, cfg);
                b.gauge(p, role);
                let limit = usd(blk.limit_usd.unwrap_or(0.0));
                b.row3(
                    &format!("Reste {left}"),
                    &format!("{p:.0} %"),
                    &format!("{} / {limit}", usd(blk.cost_usd)),
                    Role::Text,
                    role,
                    false,
                );
            }
            None => b.row3(&format!("Reste {left}"), &usd(blk.cost_usd), "", Role::Text, Role::Text, false),
        }
    }

    match (&s.org, &s.org_error) {
        (Some(o), _) => {
            b.section("Organisation (Admin API)");
            b.row3("Aujourd'hui", &usd(o.today_usd), "", Role::Text, Role::Text, false);
            b.row3("Ce mois", &usd(o.month_usd), "", Role::Text, Role::Text, false);
        }
        (None, Some(e)) => {
            b.section("Organisation (Admin API)");
            b.line(truncate(e, 64), Role::Warn, 11.0, false);
        }
        _ => {}
    }

    if !s.daily.is_empty() {
        b.section("14 derniers jours");
        let chart_h = 56.0;
        let max = s.daily.iter().map(|d| d.cost_usd).fold(0.0_f64, f64::max).max(0.01);
        let slot = (b.w - 2.0 * PAD) / s.daily.len() as f32;
        for (i, d) in s.daily.iter().enumerate() {
            let h = ((d.cost_usd / max) as f32 * chart_h).max(if d.cost_usd > 0.0 { 2.0 } else { 0.0 });
            let x = PAD + i as f32 * slot;
            let bar = R { x: x + 2.0, y: b.y + chart_h - h, w: slot - 4.0, h };
            b.ops.push(Op::Fill { r: bar, role: Role::Accent, radius: 2.0 });
            if i % 2 == 0 || i + 1 == s.daily.len() {
                b.ops.push(Op::Text {
                    r: R { x, y: b.y + chart_h + 2.0, w: slot, h: 14.0 },
                    s: d.date.format("%d").to_string(),
                    role: Role::Dim,
                    size: 9.0,
                    bold: false,
                    align: Align::Center,
                });
            }
        }
        b.y += chart_h + 18.0;
    }

    if !s.models.is_empty() {
        b.section("Modèles du mois");
        for (m, t) in s.models.iter().take(4) {
            b.row3(&truncate(m, 28), &usd(t.cost_usd), &fmt_tokens(t.tokens()), Role::Text, Role::Text, false);
        }
    }
    b.gap(8.0);
    b.line(format!("Logs Claude Code, {} fichier(s) · coûts estimés", s.files_scanned), Role::Dim, 10.0, false);
    b.finish()
}

/// Petit HUD toujours visible.
pub fn floating(s: &Summary, cfg: &ClaudeConfig) -> Scene {
    let mut b = Builder::new(250.0);
    b.line("✦ Claude", Role::Accent, 13.0, true);
    if let Some(sess) = &s.session {
        b.gauge(sess.context_pct, role_for(sess.context_pct, cfg));
        b.pair("Contexte", format!("{:.0} %", sess.context_pct), role_for(sess.context_pct, cfg));
    }
    b.pair("Aujourd'hui", usd(s.today.cost_usd), Role::Text);
    b.pair("Semaine", usd(s.week.cost_usd), Role::Text);
    if let Some(blk) = &s.five_hour {
        let role = blk.pct.map_or(Role::Text, |p| role_for(p, cfg));
        b.pair("5 h", blk.pct.map_or(usd(blk.cost_usd), |p| format!("{p:.0} %")), role);
    }
    b.finish()
}

pub fn clipboard(items: &[String], max_chars: usize) -> Scene {
    let mut b = Builder::new(380.0);
    b.line("Presse-papiers", Role::Accent, 14.0, true);
    if items.is_empty() {
        b.line("Rien de copié depuis le lancement de Forge Bar.", Role::Dim, 12.0, false);
    }
    for (i, it) in items.iter().enumerate() {
        let r = R { x: 8.0, y: b.y, w: b.w - 16.0, h: 28.0 };
        b.ops.push(Op::Fill { r, role: Role::Track, radius: 6.0 });
        b.ops.push(Op::Text {
            r: R { x: PAD, y: b.y, w: b.w - 2.0 * PAD, h: 28.0 },
            s: truncate(it, max_chars),
            role: Role::Text,
            size: 12.0,
            bold: false,
            align: Align::Left,
        });
        b.ops.push(Op::Hit { r, action: Action::ClipPick(i) });
        b.y += 32.0;
    }
    if !items.is_empty() {
        b.gap(4.0);
        let r = R { x: PAD, y: b.y, w: b.w - 2.0 * PAD, h: 22.0 };
        b.ops.push(Op::Text {
            r,
            s: "Effacer l'historique".into(),
            role: Role::Dim,
            size: 11.0,
            bold: false,
            align: Align::Center,
        });
        b.ops.push(Op::Hit { r, action: Action::ClipClear });
        b.y += 22.0;
    }
    b.finish()
}

pub fn calendar(g: &MonthGrid) -> Scene {
    let w = 296.0;
    let mut b = Builder::new(w);
    let head = R { x: PAD, y: b.y, w: 28.0, h: 28.0 };
    b.ops.push(Op::Text { r: head, s: "‹".into(), role: Role::Accent, size: 18.0, bold: true, align: Align::Center });
    b.ops.push(Op::Hit { r: head, action: Action::CalPrev });
    let next = R { x: w - PAD - 28.0, ..head };
    b.ops.push(Op::Text { r: next, s: "›".into(), role: Role::Accent, size: 18.0, bold: true, align: Align::Center });
    b.ops.push(Op::Hit { r: next, action: Action::CalNext });
    b.text(PAD + 32.0, w - 2.0 * PAD - 64.0, 28.0, capitalize(&g.title), Role::Text, 14.0, true, Align::Center);
    b.y += 34.0;

    let (wk_w, cell) = (26.0, (w - 2.0 * PAD - 26.0) / 7.0);
    for (i, d) in DAYS.iter().enumerate() {
        b.ops.push(Op::Text {
            r: R { x: PAD + wk_w + i as f32 * cell, y: b.y, w: cell, h: 18.0 },
            s: (*d).into(),
            role: Role::Dim,
            size: 11.0,
            bold: false,
            align: Align::Center,
        });
    }
    b.y += 22.0;
    for week in &g.weeks {
        b.ops.push(Op::Text {
            r: R { x: PAD, y: b.y, w: wk_w - 4.0, h: 28.0 },
            s: week.iso.to_string(),
            role: Role::Dim,
            size: 9.0,
            bold: false,
            align: Align::Center,
        });
        for (i, d) in week.days.iter().enumerate() {
            let Some(d) = d else { continue };
            let r = R { x: PAD + wk_w + i as f32 * cell, y: b.y, w: cell, h: 28.0 };
            let today = g.today == Some(*d);
            if today {
                let pill = R { x: r.x + 2.0, y: r.y + 1.0, w: r.w - 4.0, h: r.h - 2.0 };
                b.ops.push(Op::Fill { r: pill, role: Role::Accent, radius: 8.0 });
            }
            let role = if i >= 5 { Role::Dim } else { Role::Text };
            b.ops.push(Op::Text { r, s: d.to_string(), role, size: 12.0, bold: today, align: Align::Center });
        }
        b.y += 30.0;
    }
    let today = R { x: PAD, y: b.y + 4.0, w: w - 2.0 * PAD, h: 22.0 };
    b.ops.push(Op::Text {
        r: today,
        s: "Aujourd'hui".into(),
        role: Role::Accent,
        size: 11.0,
        bold: false,
        align: Align::Center,
    });
    b.ops.push(Op::Hit { r: today, action: Action::CalToday });
    b.y += 30.0;
    b.finish()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or_else(String::new, |f| f.to_uppercase().collect::<String>() + c.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn hits(s: &Scene) -> Vec<Action> {
        s.ops.iter().filter_map(|o| if let Op::Hit { action, .. } = o { Some(*action) } else { None }).collect()
    }

    #[test]
    fn calendar_has_nav_and_today() {
        let g = crate::calendar::month_grid(2026, 10, NaiveDate::from_ymd_opt(2026, 10, 15).unwrap());
        let s = calendar(&g);
        assert_eq!(hits(&s), vec![Action::CalPrev, Action::CalNext, Action::CalToday]);
        assert!(s.ops.iter().any(|o| matches!(o, Op::Text { s, .. } if s == "Octobre 2026")));
        assert!(s.h > 200.0 && s.h < 400.0);
    }

    #[test]
    fn clipboard_rows_are_clickable_and_truncated() {
        let items = vec!["a".repeat(200), "deux".into()];
        let s = clipboard(&items, 30);
        assert_eq!(hits(&s), vec![Action::ClipPick(0), Action::ClipPick(1), Action::ClipClear]);
        let longest =
            s.ops.iter().filter_map(|o| if let Op::Text { s, .. } = o { Some(s.chars().count()) } else { None }).max();
        assert!(longest.unwrap() <= 60);
        assert!(hits(&clipboard(&[], 30)).is_empty());
    }

    #[test]
    fn claude_detail_grows_with_data() {
        let cfg = ClaudeConfig::default();
        let empty = claude_detail(&Summary::default(), &cfg);
        let s = Summary {
            daily: (0..14)
                .map(|i| forge_claude::Day {
                    date: NaiveDate::from_ymd_opt(2026, 10, 1 + i).unwrap(),
                    cost_usd: i as f64,
                    tokens: 1000,
                })
                .collect(),
            models: vec![("claude-opus".into(), forge_claude::Totals::default())],
            ..Summary::default()
        };
        assert!(claude_detail(&s, &cfg).h > empty.h);
    }
}
