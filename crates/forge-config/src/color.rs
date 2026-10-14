/// Couleur RVBA, composantes de 0 à 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const TRANSPARENT: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub fn with_alpha(self, a: f32) -> Self {
        Self { a: self.a * a.clamp(0.0, 1.0), ..self }
    }
}

/// Lit `#RGB`, `#RRGGBB` ou `#RRGGBBAA`.
pub fn parse_color(s: &str) -> Option<Rgba> {
    let hex = s.trim().strip_prefix('#')?;
    if !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    let (r, g, b, a) = match hex.len() {
        3 => {
            let n = |i: usize| u8::from_str_radix(&hex[i..i + 1].repeat(2), 16).ok();
            (n(0)?, n(1)?, n(2)?, 255)
        }
        6 => (byte(0)?, byte(2)?, byte(4)?, 255),
        8 => (byte(0)?, byte(2)?, byte(4)?, byte(6)?),
        _ => return None,
    };
    Some(Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: a as f32 / 255.0 })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_formats() {
        assert_eq!(parse_color("#fff").unwrap().r, 1.0);
        let c = parse_color("#FF000080").unwrap();
        assert_eq!(c.r, 1.0);
        assert!((c.a - 0.502).abs() < 0.01);
        assert!(parse_color("red").is_none());
        assert!(parse_color("#12345").is_none());
    }
}
