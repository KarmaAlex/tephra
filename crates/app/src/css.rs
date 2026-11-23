//! CSS values the app understands: colours (`#rgb`, `#rrggbb[aa]`, `rgb()`, `rgba()`, `hsl()`,
//! `hsla()`, Obsidian's bare `r, g, b` triplets, the CSS named colours) and lengths. Used for
//! the HTML the editing toolbar writes and for theme / snippet variables.

use gpui_kit::{Hsla, Rgba};

/// CSS named colours (the 148 of CSS Color 4 would be overkill: these are the ones a colour
/// picker or a theme realistically writes).
const NAMED: &[(&str, u32)] = &[
    ("black", 0x000000), ("white", 0xffffff), ("red", 0xff0000), ("green", 0x008000), ("blue", 0x0000ff),
    ("yellow", 0xffff00), ("orange", 0xffa500), ("purple", 0x800080), ("pink", 0xffc0cb), ("brown", 0xa52a2a),
    ("gray", 0x808080), ("grey", 0x808080), ("silver", 0xc0c0c0), ("maroon", 0x800000), ("olive", 0x808000),
    ("lime", 0x00ff00), ("aqua", 0x00ffff), ("cyan", 0x00ffff), ("teal", 0x008080), ("navy", 0x000080),
    ("fuchsia", 0xff00ff), ("magenta", 0xff00ff), ("gold", 0xffd700), ("indigo", 0x4b0082), ("violet", 0xee82ee),
    ("crimson", 0xdc143c), ("coral", 0xff7f50), ("salmon", 0xfa8072), ("tomato", 0xff6347), ("orchid", 0xda70d6),
    ("khaki", 0xf0e68c), ("turquoise", 0x40e0d0), ("tan", 0xd2b48c), ("beige", 0xf5f5dc), ("lavender", 0xe6e6fa),
    ("darkred", 0x8b0000), ("darkgreen", 0x006400), ("darkblue", 0x00008b), ("darkorange", 0xff8c00),
    ("lightblue", 0xadd8e6), ("lightgreen", 0x90ee90), ("lightgray", 0xd3d3d3), ("lightgrey", 0xd3d3d3),
    ("darkgray", 0xa9a9a9), ("darkgrey", 0xa9a9a9), ("skyblue", 0x87ceeb), ("steelblue", 0x4682b4),
    ("royalblue", 0x4169e1), ("seagreen", 0x2e8b57), ("forestgreen", 0x228b22), ("firebrick", 0xb22222),
    ("chocolate", 0xd2691e), ("goldenrod", 0xdaa520), ("slategray", 0x708090), ("slategrey", 0x708090),
    ("dodgerblue", 0x1e90ff), ("deeppink", 0xff1493), ("hotpink", 0xff69b4), ("mediumpurple", 0x9370db),
];

fn rgba(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    Hsla::from(Rgba { r: r.clamp(0., 1.), g: g.clamp(0., 1.), b: b.clamp(0., 1.), a: a.clamp(0., 1.) })
}

/// A number or percentage (`50%` → 0.5 of `full`).
fn num(s: &str, full: f32) -> Option<f32> {
    let s = s.trim();
    match s.strip_suffix('%') {
        Some(p) => p.trim().parse::<f32>().ok().map(|v| v / 100.0 * full),
        None => s.trim_end_matches("deg").parse().ok(),
    }
}

/// Splits function arguments on commas, spaces and `/` (both CSS syntaxes).
fn args(inner: &str) -> Vec<&str> {
    inner.split([',', ' ', '/']).map(str::trim).filter(|a| !a.is_empty()).collect()
}

fn hsl_to(h: f32, s: f32, l: f32, a: f32) -> Hsla {
    Hsla { h: (h.rem_euclid(360.0)) / 360.0, s: s.clamp(0., 1.), l: l.clamp(0., 1.), a: a.clamp(0., 1.) }
}

/// Parses a CSS colour. `None` for anything else (keywords such as `inherit`, `var(…)`).
pub fn parse_color(v: &str) -> Option<Hsla> {
    let v = v.trim().trim_end_matches("!important").trim();
    let lower = v.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix('#') {
        let d: Vec<u8> = hex.chars().map(|c| c.to_digit(16).map(|x| x as u8)).collect::<Option<_>>()?;
        let (r, g, b, a) = match d.len() {
            3 | 4 => (d[0] * 17, d[1] * 17, d[2] * 17, if d.len() == 4 { d[3] * 17 } else { 255 }),
            6 | 8 => (d[0] * 16 + d[1], d[2] * 16 + d[3], d[4] * 16 + d[5], if d.len() == 8 { d[6] * 16 + d[7] } else { 255 }),
            _ => return None,
        };
        return Some(rgba(r as f32 / 255., g as f32 / 255., b as f32 / 255., a as f32 / 255.));
    }
    if let Some(inner) = lower.strip_prefix("rgba(").or_else(|| lower.strip_prefix("rgb(")).and_then(|r| r.strip_suffix(')')) {
        let a = args(inner);
        if a.len() < 3 {
            return None;
        }
        let alpha = a.get(3).and_then(|x| num(x, 1.0)).unwrap_or(1.0);
        return Some(rgba(num(a[0], 255.)? / 255., num(a[1], 255.)? / 255., num(a[2], 255.)? / 255., alpha));
    }
    if let Some(inner) = lower.strip_prefix("hsla(").or_else(|| lower.strip_prefix("hsl(")).and_then(|r| r.strip_suffix(')')) {
        let a = args(inner);
        if a.len() < 3 {
            return None;
        }
        let alpha = a.get(3).and_then(|x| num(x, 1.0)).unwrap_or(1.0);
        return Some(hsl_to(num(a[0], 360.)?, num(a[1], 1.)?, num(a[2], 1.)?, alpha));
    }
    if lower == "transparent" {
        return Some(rgba(0., 0., 0., 0.));
    }
    if let Some((_, c)) = NAMED.iter().find(|(n, _)| *n == lower) {
        return Some(rgba(((c >> 16) & 0xff) as f32 / 255., ((c >> 8) & 0xff) as f32 / 255., (c & 0xff) as f32 / 255., 1.));
    }
    // Obsidian's `--color-red-rgb: 233, 49, 71` / `--callout-color: 0, 120, 200`
    let parts: Vec<&str> = lower.split(',').map(str::trim).collect();
    if parts.len() == 3 {
        let n: Vec<f32> = parts.iter().filter_map(|p| p.parse::<f32>().ok()).collect();
        if n.len() == 3 {
            return Some(rgba(n[0] / 255., n[1] / 255., n[2] / 255., 1.));
        }
    }
    None
}

/// A length in logical pixels (`12px`, `1.2em`/`rem` relative to `em`, `0`, a bare number).
pub fn parse_length(v: &str, em: f32) -> Option<f32> {
    let v = v.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
    if let Some(n) = v.strip_suffix("px") {
        return n.trim().parse().ok();
    }
    if let Some(n) = v.strip_suffix("rem").or_else(|| v.strip_suffix("em")) {
        return n.trim().parse::<f32>().ok().map(|x| x * em);
    }
    if let Some(n) = v.strip_suffix("pt") {
        return n.trim().parse::<f32>().ok().map(|x| x * 96.0 / 72.0);
    }
    v.parse().ok()
}

/// `background:#ff0; color: red` → the value of one property of an inline `style` attribute.
pub fn style_prop(style: &str, prop: &str) -> Option<String> {
    style.split(';').find_map(|decl| {
        let (k, v) = decl.split_once(':')?;
        k.trim().eq_ignore_ascii_case(prop).then(|| v.trim().to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rgb_of(h: Hsla) -> (u8, u8, u8, u8) {
        let c = Rgba::from(h);
        ((c.r * 255.).round() as u8, (c.g * 255.).round() as u8, (c.b * 255.).round() as u8, (c.a * 255.).round() as u8)
    }

    #[test]
    fn colours() {
        assert_eq!(parse_color("#ff0000").map(rgb_of), Some((255, 0, 0, 255)));
        assert_eq!(parse_color("#0f08").map(rgb_of), Some((0, 255, 0, 136)));
        assert_eq!(parse_color("rgb(0, 0, 255)").map(rgb_of), Some((0, 0, 255, 255)));
        assert_eq!(parse_color("rgba(255 255 255 / 50%)").map(rgb_of), Some((255, 255, 255, 128)));
        assert_eq!(parse_color("hsl(0, 100%, 50%)").map(rgb_of), Some((255, 0, 0, 255)));
        assert_eq!(parse_color("Red").map(rgb_of), Some((255, 0, 0, 255)));
        assert_eq!(parse_color("0, 120, 200").map(rgb_of), Some((0, 120, 200, 255)));
        assert_eq!(parse_color("var(--x)"), None);
        assert_eq!(parse_color("#12"), None);
    }

    #[test]
    fn lengths_and_styles() {
        assert_eq!(parse_length("0px", 16.), Some(0.));
        assert_eq!(parse_length("1.5em", 16.), Some(24.));
        assert_eq!(parse_length("2", 16.), Some(2.));
        assert_eq!(style_prop("background:#ff0; color: red", "color").as_deref(), Some("red"));
        assert_eq!(style_prop("background:#ff0", "BACKGROUND").as_deref(), Some("#ff0"));
    }
}
