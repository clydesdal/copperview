//! Reads drill files (Excellon), including what Altium writes in the comments.

use crate::geom::*;
use crate::gerber::{Feature, Kind, NONE, Parsed, Tool, feature_bbox};
use std::collections::HashMap;

pub fn looks_like_drill(text: &str) -> bool {
    let head = &text[..text.floor_char_boundary(4_000)];
    let starts = head.trim_start().lines().next().unwrap_or("");
    head.contains("M48") || starts.starts_with(";FILE_FORMAT") || starts.starts_with(";TYPE=")
}

struct Number {
    int: u32,
    dec: u32,
    /// True when trailing zeros are dropped, so digits line up from the left.
    from_left: bool,
}

impl Number {
    fn read(&self, text: &str) -> f64 {
        if text.contains('.') {
            return text.parse().unwrap_or(0.0);
        }
        let negative = text.starts_with('-');
        let digits = text.trim_start_matches(['-', '+']);
        let mut value: f64 = digits.parse().unwrap_or(0.0);
        if self.from_left {
            value *= 10f64.powi((self.int + self.dec) as i32 - digits.len() as i32);
        }
        value /= 10f64.powi(self.dec as i32);
        if negative { -value } else { value }
    }
}

fn field(line: &str, letter: char) -> Option<&str> {
    let at = line.find(letter)?;
    let rest = &line[at + 1..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

pub fn parse(text: &str) -> Parsed {
    let mut out = Parsed {
        strings: vec![String::new()],
        units: "inch",
        is_drill: true,
        format: String::new(),
        ..Default::default()
    };
    let mut unit = 25.4;
    let mut number = Number { int: 2, dec: 4, from_left: true };
    let mut format_given = false;
    let mut tools: HashMap<u32, f64> = HashMap::new();
    let mut counts: HashMap<u32, u32> = HashMap::new();
    let mut tool = 0u32;
    let mut in_header = false;
    let (mut x, mut y) = (0.0f64, 0.0f64);
    let mut incremental = false;
    let mut routing = false; // G00 positions, then M15 / G01 ... M16 cuts a slot
    let mut pen_down = false;
    let mut rout_start: Option<P> = None;

    let add_slot = |out: &mut Parsed, a: P, b: P, dia: f64| {
        out.features.push(Feature {
            kind: Kind::Slot,
            dark: true,
            bbox: feature_bbox(&[a, b], dia / 2.0),
            path: vec![a, b],
            ap: NONE,
            width: dia,
            arc: None,
            net: 0,
            comp: 0,
            pin: 0,
            func: 0,
        });
    };
    let add_hole = |out: &mut Parsed, at: P, dia: f64| {
        out.features.push(Feature {
            kind: Kind::Hole,
            dark: true,
            bbox: feature_bbox(&[at], dia / 2.0),
            path: vec![at],
            ap: NONE,
            width: dia,
            arc: None,
            net: 0,
            comp: 0,
            pin: 0,
            func: 0,
        });
    };

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix(';') {
            let comment = comment.trim();
            if let Some(v) = comment.strip_prefix("FILE_FORMAT=") {
                let mut parts = v.split(':');
                if let (Some(Ok(i)), Some(Ok(d))) =
                    (parts.next().map(str::parse), parts.next().map(str::parse))
                {
                    number.int = i;
                    number.dec = d;
                    format_given = true;
                }
            } else if let Some(v) = comment.strip_prefix("TYPE=") {
                out.plated = Some(!v.to_uppercase().starts_with("NON"));
            }
            continue;
        }
        let upper = line.to_uppercase();
        if upper == "M48" {
            in_header = true;
            continue;
        }
        if upper == "%" || upper == "M95" {
            in_header = false;
            continue;
        }
        if upper == "M30" || upper == "M00" {
            break;
        }
        if upper.starts_with("METRIC") || upper.starts_with("INCH") {
            let metric = upper.starts_with("METRIC");
            unit = if metric { 1.0 } else { 25.4 };
            out.units = if metric { "mm" } else { "inch" };
            if !format_given {
                number.int = if metric { 3 } else { 2 };
                number.dec = if metric { 3 } else { 4 };
            }
            // In Excellon "TZ" means trailing zeros are kept; "LZ" means leading ones are.
            if upper.contains("TZ") {
                number.from_left = false;
            } else if upper.contains("LZ") {
                number.from_left = true;
            }
            // Some programs spell the format: METRIC,TZ,000.000
            if let Some(pattern) = upper.split(',').find(|t| t.contains('.'))
                && let Some((a, b)) = pattern.split_once('.')
            {
                number.int = a.len() as u32;
                number.dec = b.len() as u32;
                format_given = true;
            }
            continue;
        }
        if upper.starts_with("FMAT") || upper.starts_with("VER") || upper.starts_with("ICI") {
            continue;
        }
        if upper.starts_with('T') {
            let number_end = upper[1..]
                .find(|c: char| !c.is_ascii_digit())
                .map_or(upper.len(), |n| n + 1);
            let id: u32 = upper[1..number_end].parse().unwrap_or(0);
            if let Some(c) = field(&upper, 'C') {
                let dia: f64 = c.parse().unwrap_or(0.0);
                tools.insert(id, dia * unit);
                if !in_header {
                    tool = id;
                }
            } else if !in_header {
                tool = id;
            }
            // Coordinates can sit on the same line as a tool change.
            if !upper.contains('X') && !upper.contains('Y') {
                continue;
            }
        }
        if in_header {
            continue;
        }
        if upper.starts_with("G90") {
            incremental = false;
            continue;
        }
        if upper.starts_with("G91") {
            incremental = true;
            continue;
        }
        if upper.starts_with("G05") || upper.starts_with("G5") {
            routing = false;
            pen_down = false;
            continue;
        }
        if upper.starts_with("M15") {
            pen_down = true;
            rout_start = Some(p(x, y).scale(unit));
            continue;
        }
        if upper.starts_with("M16") || upper.starts_with("M17") {
            pen_down = false;
            continue;
        }
        let g00 = upper.starts_with("G00");
        let g01 = upper.starts_with("G01");
        if g00 || g01 {
            routing = true;
        }

        let dia = tools.get(&tool).copied().unwrap_or(0.0);
        let step = |x: &mut f64, y: &mut f64, text: &str| {
            if let Some(v) = field(text, 'X') {
                let v = number.read(v);
                *x = if incremental { *x + v } else { v };
            }
            if let Some(v) = field(text, 'Y') {
                let v = number.read(v);
                *y = if incremental { *y + v } else { v };
            }
        };
        let has_coord = upper.contains('X') || upper.contains('Y');
        if let Some(at) = upper.find("G85") {
            // X1Y1G85X2Y2: a drilled slot from one point to the other.
            step(&mut x, &mut y, &upper[..at]);
            let a = p(x, y).scale(unit);
            step(&mut x, &mut y, &upper[at + 3..]);
            add_slot(&mut out, a, p(x, y).scale(unit), dia);
            *counts.entry(tool).or_default() += 1;
            continue;
        }
        if let Some(repeat) = upper.strip_prefix('R') {
            // R5X0.1: repeat the last hole five more times, stepping each time.
            let n_end = repeat.find(|c: char| !c.is_ascii_digit()).unwrap_or(repeat.len());
            let n: u32 = repeat[..n_end].parse().unwrap_or(0);
            let dx = field(repeat, 'X').map_or(0.0, |v| number.read(v));
            let dy = field(repeat, 'Y').map_or(0.0, |v| number.read(v));
            for _ in 0..n.min(100_000) {
                x += dx;
                y += dy;
                add_hole(&mut out, p(x, y).scale(unit), dia);
                *counts.entry(tool).or_default() += 1;
            }
            continue;
        }
        if !has_coord {
            continue;
        }
        step(&mut x, &mut y, &upper);
        let at = p(x, y).scale(unit);
        if routing {
            if pen_down
                && g01
                && let Some(start) = rout_start
            {
                add_slot(&mut out, start, at, dia);
                *counts.entry(tool).or_default() += 1;
                rout_start = Some(at);
            }
            if g00 {
                rout_start = Some(at);
            }
            continue;
        }
        add_hole(&mut out, at, dia);
        *counts.entry(tool).or_default() += 1;
    }

    let mut list: Vec<Tool> = tools
        .iter()
        .map(|(n, d)| Tool { number: *n, dia: *d, count: counts.get(n).copied().unwrap_or(0) })
        .collect();
    list.sort_by_key(|t| t.number);
    out.tools = list;
    out.format = format!("{}:{}", number.int, number.dec);
    if out.features.iter().any(|f| f.width <= 0.0) {
        out.warnings.push("Some holes use a drill size the file doesn't define.".into());
    }
    out.finish();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn altium_style_inch_file() {
        let d = parse(
            ";Layer_Color=9474304\n;FILE_FORMAT=2:5\nM48\nINCH\n;TYPE=PLATED\nT1F00S00C0.01181\n\
             T2F00S00C0.03937\n%\nT01\nX010000Y005000\nX02Y005\nT02\nX030000Y005000\nM30\n",
        );
        assert_eq!(d.plated, Some(true));
        assert_eq!(d.features.len(), 3);
        assert!((d.features[0].path[0].x - 25.4).abs() < 1e-6, "{:?}", d.features[0].path);
        assert!((d.features[0].width - 0.3).abs() < 0.001);
        assert!((d.features[1].path[0].x - 50.8).abs() < 1e-6, "{:?}", d.features[1].path);
        assert_eq!(d.tools[0].count, 2);
        assert_eq!(d.tools[1].count, 1);
    }

    #[test]
    fn metric_with_decimal_points_and_slot() {
        let d = parse(
            "M48\nMETRIC,TZ\n;TYPE=NON_PLATED\nT1C1.0\n%\nT1\nX10.0Y20.0\nX30.0Y20.0G85X35.0Y20.0\nM30\n",
        );
        assert_eq!(d.plated, Some(false));
        assert_eq!(d.features.len(), 2);
        assert_eq!(d.features[1].kind, Kind::Slot);
        assert!((d.features[1].length() - 5.0).abs() < 1e-9);
    }

    #[test]
    fn routed_slot_commands() {
        let d = parse(
            "M48\nMETRIC\nT1C0.8\n%\nT1\nG00X1.0Y1.0\nM15\nG01X6.0Y1.0\nM16\nG05\nX2.0Y2.0\nM30\n",
        );
        assert_eq!(d.features.len(), 2);
        assert_eq!(d.features[0].kind, Kind::Slot);
        assert!((d.features[0].length() - 5.0).abs() < 1e-9);
        assert_eq!(d.features[1].kind, Kind::Hole);
    }

    #[test]
    fn report_text_is_not_a_drill_file() {
        assert!(!looks_like_drill("Output: Gerber Files\nType : Gerber\n"));
        assert!(looks_like_drill("M48\nINCH\n"));
    }
}
