//! Couleur dominante d'une image : celle qui a du caractère, pas la moyenne grisâtre.
#![cfg_attr(not(windows), allow(dead_code))]

use forge_config::Rgba;

const BUCKETS: usize = 12;

/// `bgra` : pixels BGRA, 4 octets chacun. Les teintes sont regroupées en 12 familles pondérées par
/// saturation et luminosité ; la famille gagnante donne sa couleur moyenne, éclaircie si trop sombre.
/// Une image sans couleur (gris, noir, blanc) donne `None` : on garde alors l'accent du thème.
pub fn dominant(bgra: &[u8]) -> Option<Rgba> {
    let mut weight = [0.0f32; BUCKETS];
    let mut sum = [[0.0f32; 3]; BUCKETS];
    for p in bgra.chunks_exact(4) {
        let (b, g, r) = (p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let sat = if max > 0.0 { (max - min) / max } else { 0.0 };
        if sat < 0.2 || max < 0.15 {
            continue;
        }
        let d = max - min;
        let hue = if max == r {
            ((g - b) / d).rem_euclid(6.0)
        } else if max == g {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };
        let i = ((hue / 6.0 * BUCKETS as f32) as usize).min(BUCKETS - 1);
        let w = sat * max;
        weight[i] += w;
        sum[i][0] += r * w;
        sum[i][1] += g * w;
        sum[i][2] += b * w;
    }
    let (best, &w) = weight.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))?;
    if w <= 0.0 {
        return None;
    }
    let mut c = [sum[best][0] / w, sum[best][1] / w, sum[best][2] / w];
    let max = c[0].max(c[1]).max(c[2]);
    if max < 0.6 {
        let k = 0.6 / max;
        c = c.map(|v| (v * k).min(1.0));
    }
    Some(Rgba { r: c[0], g: c[1], b: c[2], a: 1.0 })
}

pub fn hex(c: Rgba) -> String {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", q(c.r), q(c.g), q(c.b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(px: &[[u8; 3]]) -> Vec<u8> {
        px.iter().flat_map(|c| [c[2], c[1], c[0], 255]).collect()
    }

    #[test]
    fn picks_the_vivid_color_over_the_gray_majority() {
        let mut px = vec![[128, 128, 128]; 80];
        px.extend(vec![[220, 40, 40]; 20]);
        let c = dominant(&img(&px)).unwrap();
        assert!(c.r > 0.8 && c.g < 0.3 && c.b < 0.3, "{c:?}");
        assert_eq!(hex(c).len(), 7);
    }

    #[test]
    fn gray_image_has_no_accent_and_dark_color_is_lifted() {
        assert!(dominant(&img(&[[100, 100, 100]; 50])).is_none());
        let c = dominant(&img(&[[0, 0, 60]; 50])).unwrap();
        assert!(c.b >= 0.59);
    }

    #[test]
    fn biggest_family_wins() {
        let mut px = vec![[0, 200, 0]; 30];
        px.extend(vec![[200, 0, 0]; 10]);
        let c = dominant(&img(&px)).unwrap();
        assert!(c.g > c.r);
    }
}
