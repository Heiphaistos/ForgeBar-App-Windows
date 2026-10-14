//! Lecteur SVG minimal pour les packs d'icônes : `<path>`, `<circle>`, `<rect>`, `<line>`,
//! `<polyline>`, `<polygon>`, `<ellipse>`, remplissage ou contour, couleur `currentColor`.
//! Tout est ramené à des courbes de Bézier cubiques, que Direct2D sait tracer.
//! Pas de dégradés, de transformations, de `<use>` ni de CSS : un pack d'icônes n'en a pas besoin.
#![cfg_attr(not(windows), allow(dead_code))]

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    Move(f32, f32),
    Line(f32, f32),
    Cubic([f32; 6]),
    Close,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub cmds: Vec<Cmd>,
    pub fill: bool,
    /// Épaisseur du contour dans le repère du viewBox.
    pub stroke: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Icon {
    pub view: [f32; 4],
    pub shapes: Vec<Shape>,
}

fn attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && !(b[i].is_ascii_alphabetic()) {
            i += 1;
        }
        let s = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b':') {
            i += 1;
        }
        let name = &tag[s..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let vs = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                out.push((name.to_string(), tag[vs..i].to_string()));
                i += 1;
            }
        }
    }
    out
}

fn get<'a>(a: &'a [(String, String)], k: &str) -> Option<&'a str> {
    a.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
}

fn num(a: &[(String, String)], k: &str) -> f32 {
    get(a, k).and_then(|v| v.trim().trim_end_matches("px").parse().ok()).unwrap_or(0.0)
}

fn numbers(s: &str) -> Vec<f32> {
    let mut out = Vec::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() || b[i] == b'.' || b[i] == b'-' || b[i] == b'+' {
            let st = i;
            i += 1;
            let mut seen_dot = b[st] == b'.';
            while i < b.len() {
                let c = b[i];
                if c.is_ascii_digit() {
                    i += 1;
                } else if c == b'.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else if (c == b'e' || c == b'E') && i + 1 < b.len() {
                    i += 1;
                    if b[i] == b'-' || b[i] == b'+' {
                        i += 1;
                    }
                } else {
                    break;
                }
            }
            if let Ok(v) = s[st..i].parse() {
                out.push(v);
            }
        } else {
            i += 1;
        }
    }
    out
}

/// Arc elliptique SVG vers des cubiques (algorithme de l'annexe F.6 de la spécification).
fn arc(out: &mut Vec<Cmd>, from: (f32, f32), r: (f32, f32), rot: f32, large: bool, sweep: bool, to: (f32, f32)) {
    let (mut rx, mut ry) = (r.0.abs(), r.1.abs());
    if (from.0 == to.0 && from.1 == to.1) || rx == 0.0 || ry == 0.0 {
        if from != to {
            out.push(Cmd::Line(to.0, to.1));
        }
        return;
    }
    let phi = rot.to_radians();
    let (sp, cp) = phi.sin_cos();
    let dx = (from.0 - to.0) / 2.0;
    let dy = (from.1 - to.1) / 2.0;
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        let s = lambda.sqrt();
        rx *= s;
        ry *= s;
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let cxp = co * rx * y1 / ry;
    let cyp = -co * ry * x1 / rx;
    let cx = cp * cxp - sp * cyp + (from.0 + to.0) / 2.0;
    let cy = sp * cxp + cp * cyp + (from.1 + to.1) / 2.0;
    let angle = |ux: f32, uy: f32, vx: f32, vy: f32| (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    let th1 = angle(1.0, 0.0, (x1 - cxp) / rx, (y1 - cyp) / ry);
    let mut dth = angle((x1 - cxp) / rx, (y1 - cyp) / ry, (-x1 - cxp) / rx, (-y1 - cyp) / ry);
    if !sweep && dth > 0.0 {
        dth -= std::f32::consts::TAU;
    } else if sweep && dth < 0.0 {
        dth += std::f32::consts::TAU;
    }
    let segs = (dth.abs() / std::f32::consts::FRAC_PI_2).ceil().max(1.0) as usize;
    let step = dth / segs as f32;
    let t = 4.0 / 3.0 * (step / 4.0).tan();
    let point = |a: f32| {
        let (sa, ca) = a.sin_cos();
        (cx + rx * ca * cp - ry * sa * sp, cy + rx * ca * sp + ry * sa * cp)
    };
    let deriv = |a: f32| {
        let (sa, ca) = a.sin_cos();
        (-rx * sa * cp - ry * ca * sp, -rx * sa * sp + ry * ca * cp)
    };
    let mut a = th1;
    for _ in 0..segs {
        let (p0, d0) = (point(a), deriv(a));
        let (p1, d1) = (point(a + step), deriv(a + step));
        out.push(Cmd::Cubic([p0.0 + t * d0.0, p0.1 + t * d0.1, p1.0 - t * d1.0, p1.1 - t * d1.1, p1.0, p1.1]));
        a += step;
    }
}

/// Données d'un attribut `d` vers des commandes absolues.
pub fn parse_path(d: &str) -> Vec<Cmd> {
    let mut out = Vec::new();
    let b = d.as_bytes();
    let (mut cur, mut start) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32));
    let mut last_ctrl: Option<(f32, f32)> = None; // pour S et T
    let mut last_cmd = b' ';
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let c = b[i];
        i += 1;
        let st = i;
        while i < b.len() && !(b[i].is_ascii_alphabetic() && b[i] != b'e' && b[i] != b'E') {
            i += 1;
        }
        let args = numbers(&d[st..i]);
        let rel = c.is_ascii_lowercase();
        let up = c.to_ascii_uppercase();
        let (ox, oy) = if rel { cur } else { (0.0, 0.0) };
        let step = match up {
            b'M' | b'L' | b'T' => 2,
            b'H' | b'V' => 1,
            b'C' => 6,
            b'S' | b'Q' => 4,
            b'A' => 7,
            _ => 0,
        };
        if up == b'Z' {
            out.push(Cmd::Close);
            cur = start;
            last_ctrl = None;
            last_cmd = c;
            continue;
        }
        if step == 0 || args.len() < step {
            continue;
        }
        let mut first = true;
        for a in args.chunks_exact(step) {
            match up {
                b'M' if first => {
                    cur = (ox + a[0], oy + a[1]);
                    start = cur;
                    out.push(Cmd::Move(cur.0, cur.1));
                    last_ctrl = None;
                }
                b'M' | b'L' => {
                    let o = if rel { cur } else { (0.0, 0.0) };
                    cur = (o.0 + a[0], o.1 + a[1]);
                    out.push(Cmd::Line(cur.0, cur.1));
                    last_ctrl = None;
                }
                b'H' => {
                    cur.0 = if rel { cur.0 + a[0] } else { a[0] };
                    out.push(Cmd::Line(cur.0, cur.1));
                    last_ctrl = None;
                }
                b'V' => {
                    cur.1 = if rel { cur.1 + a[0] } else { a[0] };
                    out.push(Cmd::Line(cur.0, cur.1));
                    last_ctrl = None;
                }
                b'C' => {
                    let o = if rel { cur } else { (0.0, 0.0) };
                    let v = [o.0 + a[0], o.1 + a[1], o.0 + a[2], o.1 + a[3], o.0 + a[4], o.1 + a[5]];
                    out.push(Cmd::Cubic(v));
                    last_ctrl = Some((v[2], v[3]));
                    cur = (v[4], v[5]);
                }
                b'S' => {
                    let o = if rel { cur } else { (0.0, 0.0) };
                    let prev_is_curve = matches!(last_cmd.to_ascii_uppercase(), b'C' | b'S');
                    let c1 = match (last_ctrl, prev_is_curve) {
                        (Some(lc), true) => (2.0 * cur.0 - lc.0, 2.0 * cur.1 - lc.1),
                        _ => cur,
                    };
                    let v = [c1.0, c1.1, o.0 + a[0], o.1 + a[1], o.0 + a[2], o.1 + a[3]];
                    out.push(Cmd::Cubic(v));
                    last_ctrl = Some((v[2], v[3]));
                    cur = (v[4], v[5]);
                }
                b'Q' | b'T' => {
                    let o = if rel { cur } else { (0.0, 0.0) };
                    let (q, end) = if up == b'Q' {
                        ((o.0 + a[0], o.1 + a[1]), (o.0 + a[2], o.1 + a[3]))
                    } else {
                        let prev_is_quad = matches!(last_cmd.to_ascii_uppercase(), b'Q' | b'T');
                        let q = match (last_ctrl, prev_is_quad) {
                            (Some(lc), true) => (2.0 * cur.0 - lc.0, 2.0 * cur.1 - lc.1),
                            _ => cur,
                        };
                        (q, (o.0 + a[0], o.1 + a[1]))
                    };
                    out.push(Cmd::Cubic([
                        cur.0 + 2.0 / 3.0 * (q.0 - cur.0),
                        cur.1 + 2.0 / 3.0 * (q.1 - cur.1),
                        end.0 + 2.0 / 3.0 * (q.0 - end.0),
                        end.1 + 2.0 / 3.0 * (q.1 - end.1),
                        end.0,
                        end.1,
                    ]));
                    last_ctrl = Some(q);
                    cur = end;
                }
                b'A' => {
                    let o = if rel { cur } else { (0.0, 0.0) };
                    let to = (o.0 + a[5], o.1 + a[6]);
                    arc(&mut out, cur, (a[0], a[1]), a[2], a[3] != 0.0, a[4] != 0.0, to);
                    cur = to;
                    last_ctrl = None;
                }
                _ => {}
            }
            first = false;
        }
        last_cmd = c;
    }
    out
}

fn ellipse(cx: f32, cy: f32, rx: f32, ry: f32) -> Vec<Cmd> {
    const K: f32 = 0.552_284_8;
    vec![
        Cmd::Move(cx + rx, cy),
        Cmd::Cubic([cx + rx, cy + ry * K, cx + rx * K, cy + ry, cx, cy + ry]),
        Cmd::Cubic([cx - rx * K, cy + ry, cx - rx, cy + ry * K, cx - rx, cy]),
        Cmd::Cubic([cx - rx, cy - ry * K, cx - rx * K, cy - ry, cx, cy - ry]),
        Cmd::Cubic([cx + rx * K, cy - ry, cx + rx, cy - ry * K, cx + rx, cy]),
        Cmd::Close,
    ]
}

fn polyline(points: &str, close: bool) -> Vec<Cmd> {
    let n = numbers(points);
    let mut out: Vec<Cmd> = n
        .chunks_exact(2)
        .enumerate()
        .map(|(i, p)| if i == 0 { Cmd::Move(p[0], p[1]) } else { Cmd::Line(p[0], p[1]) })
        .collect();
    if close && !out.is_empty() {
        out.push(Cmd::Close);
    }
    out
}

/// Lit un fichier SVG. `None` si aucun tracé exploitable.
pub fn parse_svg(src: &str) -> Option<Icon> {
    let mut view = [0.0, 0.0, 24.0, 24.0];
    let mut root_fill = true;
    let mut root_stroke: Option<f32> = None;
    let mut shapes = Vec::new();
    let mut rest = src;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        let Some(gt) = rest.find('>') else { break };
        let tag = &rest[..gt];
        rest = &rest[gt + 1..];
        let name_end = tag.find(|c: char| c.is_whitespace() || c == '/').unwrap_or(tag.len());
        let (name, body) = (&tag[..name_end], &tag[name_end..]);
        let a = attrs(body);
        let paint = |a: &[(String, String)], inh_fill: bool, inh_stroke: Option<f32>| {
            let fill = match get(a, "fill") {
                Some("none") => false,
                Some(_) => true,
                None => inh_fill,
            };
            let stroke = match get(a, "stroke") {
                Some("none") => None,
                Some(_) => {
                    Some(get(a, "stroke-width").and_then(|v| v.parse().ok()).unwrap_or(inh_stroke.unwrap_or(1.0)))
                }
                None => inh_stroke,
            };
            (fill, stroke)
        };
        match name {
            "svg" => {
                if let Some(v) = get(&a, "viewBox") {
                    let n = numbers(v);
                    if n.len() == 4 && n[2] > 0.0 && n[3] > 0.0 {
                        view = [n[0], n[1], n[2], n[3]];
                    }
                }
                let (f, s) = paint(&a, true, None);
                root_fill = f;
                root_stroke = s;
                if let (Some(sw), Some(_)) = (get(&a, "stroke-width").and_then(|v| v.parse::<f32>().ok()), s) {
                    root_stroke = Some(sw);
                }
            }
            "path" | "circle" | "rect" | "line" | "polyline" | "polygon" | "ellipse" => {
                let cmds = match name {
                    "path" => parse_path(get(&a, "d").unwrap_or("")),
                    "circle" => ellipse(num(&a, "cx"), num(&a, "cy"), num(&a, "r"), num(&a, "r")),
                    "ellipse" => ellipse(num(&a, "cx"), num(&a, "cy"), num(&a, "rx"), num(&a, "ry")),
                    "rect" => {
                        let (x, y, w, h) = (num(&a, "x"), num(&a, "y"), num(&a, "width"), num(&a, "height"));
                        let r = num(&a, "rx").max(num(&a, "ry")).min(w / 2.0).min(h / 2.0);
                        if r > 0.0 {
                            parse_path(&format!(
                                "M{} {}H{}A{r} {r} 0 0 1 {} {}V{}A{r} {r} 0 0 1 {} {}H{}A{r} {r} 0 0 1 {} {}V{}A{r} {r} 0 0 1 {} {}Z",
                                x + r, y, x + w - r, x + w, y + r, y + h - r, x + w - r, y + h, x + r, x, y + h - r, y + r, x + r, y
                            ))
                        } else {
                            parse_path(&format!("M{x} {y}H{}V{}H{x}Z", x + w, y + h))
                        }
                    }
                    "line" => vec![Cmd::Move(num(&a, "x1"), num(&a, "y1")), Cmd::Line(num(&a, "x2"), num(&a, "y2"))],
                    "polyline" => polyline(get(&a, "points").unwrap_or(""), false),
                    _ => polyline(get(&a, "points").unwrap_or(""), true),
                };
                if cmds.is_empty() {
                    continue;
                }
                let (mut fill, stroke) = paint(&a, root_fill, root_stroke);
                if matches!(name, "line" | "polyline") && get(&a, "fill").is_none() {
                    fill = false;
                }
                shapes.push(Shape { cmds, fill, stroke });
            }
            _ => {}
        }
    }
    (!shapes.is_empty()).then_some(Icon { view, shapes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relative_and_implicit_commands() {
        let c = parse_path("M10 10 l5 0 0 5 z");
        assert_eq!(c[0], Cmd::Move(10.0, 10.0));
        assert_eq!(c[1], Cmd::Line(15.0, 10.0));
        assert_eq!(c[2], Cmd::Line(15.0, 15.0));
        assert_eq!(c[3], Cmd::Close);
    }

    #[test]
    fn horizontal_vertical_and_compact_numbers() {
        let c = parse_path("M0,0H10V5h-2.5.5");
        assert_eq!(c[1], Cmd::Line(10.0, 0.0));
        assert_eq!(c[2], Cmd::Line(10.0, 5.0));
        assert_eq!(c[3], Cmd::Line(7.5, 5.0));
    }

    #[test]
    fn arc_ends_where_requested() {
        let c = parse_path("M0 0 A 5 5 0 0 1 10 0");
        let Some(Cmd::Cubic(v)) = c.last() else { panic!("arc attendu") };
        assert!((v[4] - 10.0).abs() < 1e-3 && v[5].abs() < 1e-3);
        assert!(c.len() >= 3, "un demi-cercle donne au moins deux cubiques");
    }

    #[test]
    fn svg_with_stroke_and_fill() {
        let svg = r##"<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="5"/><path d="M1 1L2 2" fill="currentColor"/></svg>"##;
        let icon = parse_svg(svg).unwrap();
        assert_eq!(icon.view, [0.0, 0.0, 24.0, 24.0]);
        assert_eq!(icon.shapes.len(), 2);
        assert!(!icon.shapes[0].fill);
        assert_eq!(icon.shapes[0].stroke, Some(2.0));
        assert!(icon.shapes[1].fill);
    }

    #[test]
    fn rect_and_garbage() {
        let icon =
            parse_svg(r#"<svg viewBox="0 0 10 10"><rect x="1" y="1" width="8" height="8" rx="2"/></svg>"#).unwrap();
        assert!(icon.shapes[0].fill);
        assert!(parse_svg("pas un svg").is_none());
        assert!(parse_svg("<svg><path d=''/></svg>").is_none());
    }
}
