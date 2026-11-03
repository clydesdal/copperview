//! Turns Altium's file endings into layer names, colors and stacking order.

#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Group {
    Top,
    Inner,
    Bottom,
    Drill,
    Mech,
    Other,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Group::Top => "Top side",
            Group::Inner => "Inner layers",
            Group::Bottom => "Bottom side",
            Group::Drill => "Drill",
            Group::Mech => "Mechanical",
            Group::Other => "Other",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Role {
    /// What a layer in one folder is matched on in the other.
    pub key: String,
    pub label: String,
    pub group: Group,
    pub order: i32,
    pub color: [u8; 3],
    pub show: bool,
    pub copper: bool,
}

const RED: [u8; 3] = [0xe5, 0x48, 0x4d];
const BLUE: [u8; 3] = [0x4f, 0x8c, 0xff];
const YELLOW: [u8; 3] = [0xf2, 0xd2, 0x4b];
const AMBER: [u8; 3] = [0xd9, 0xa2, 0x4e];
const VIOLET: [u8; 3] = [0xb0, 0x7c, 0xe8];
const TEAL: [u8; 3] = [0x3f, 0xc1, 0xb4];
const GREY: [u8; 3] = [0xa8, 0xad, 0xb5];
const WHITE: [u8; 3] = [0xf2, 0xf1, 0xee];
const PINK: [u8; 3] = [0xf0, 0x7a, 0xc2];

/// Inner layers cycle through these so neighbours stay easy to tell apart.
const INNER: [[u8; 3]; 6] = [
    [0x5d, 0xc0, 0x91],
    [0xe0, 0x9b, 0x3f],
    [0x7f, 0xd1, 0xe6],
    [0xc5, 0x8f, 0xe0],
    [0xa5, 0xc9, 0x5a],
    [0xe5, 0x8f, 0x8f],
];

fn role(key: &str, label: String, group: Group, order: i32, color: [u8; 3], show: bool, copper: bool) -> Role {
    Role { key: key.to_string(), label, group, order, color, show, copper }
}

fn numbered(ext: &str, prefix: &str) -> Option<u32> {
    let rest = ext.strip_prefix(prefix)?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

/// Altium's Gerber endings. Anything else returns None.
pub fn from_extension(ext: &str) -> Option<Role> {
    let e = ext.to_uppercase();
    use Group::*;
    Some(match e.as_str() {
        "GTO" => role("GTO", "Top overlay".into(), Top, 10, YELLOW, true, false),
        "GTP" => role("GTP", "Top paste".into(), Top, 20, GREY, false, false),
        "GTS" => role("GTS", "Top solder mask".into(), Top, 30, VIOLET, false, false),
        "GPT" => role("GPT", "Top pad master".into(), Top, 35, GREY, false, false),
        "GTL" => role("GTL", "Top copper".into(), Top, 40, RED, true, true),
        "GBL" => role("GBL", "Bottom copper".into(), Bottom, 500, BLUE, true, true),
        "GPB" => role("GPB", "Bottom pad master".into(), Bottom, 505, GREY, false, false),
        "GBS" => role("GBS", "Bottom solder mask".into(), Bottom, 510, TEAL, false, false),
        "GBP" => role("GBP", "Bottom paste".into(), Bottom, 520, GREY, false, false),
        "GBO" => role("GBO", "Bottom overlay".into(), Bottom, 530, AMBER, false, false),
        "GKO" => role("GKO", "Keep-out".into(), Mech, 700, PINK, true, false),
        _ => {
            if let Some(n) = numbered(&e, "GM") {
                let show = n == 1;
                return Some(role(&e, format!("Mechanical {n}"), Mech, 710 + n as i32, PINK, show, false));
            }
            if let Some(n) = numbered(&e, "GP") {
                let color = INNER[(n as usize + 2) % INNER.len()];
                return Some(role(&e, format!("Power plane {n}"), Inner, 300 + n as i32, color, false, true));
            }
            if let Some(n) = numbered(&e, "GD") {
                return Some(role(&e, format!("Drill drawing {n}"), Other, 800 + n as i32, GREY, false, false));
            }
            if let Some(n) = numbered(&e, "GG") {
                return Some(role(&e, format!("Drill guide {n}"), Other, 830 + n as i32, GREY, false, false));
            }
            if let Some(n) = numbered(&e, "G") {
                let color = INNER[(n as usize - 1).min(29) % INNER.len()];
                return Some(role(&e, format!("Mid layer {n}"), Inner, 100 + n as i32, color, false, true));
            }
            return None;
        }
    })
}

/// A drill file's job, from its name: Altium splits them into plated,
/// non-plated, round holes and slots.
pub fn drill_role(stem: &str, plated: Option<bool>) -> Role {
    let s = stem.to_lowercase();
    let non_plated = s.contains("nonplated") || s.contains("non-plated") || s.contains("npth") || plated == Some(false);
    let is_plated = !non_plated && (s.contains("plated") || s.contains("pth") || plated == Some(true));
    let slots = s.contains("slot");
    let (kind_key, kind) = if non_plated {
        ("NPTH", "Holes, non-plated")
    } else if is_plated {
        ("PTH", "Holes, plated")
    } else {
        ("DRILL", "Holes")
    };
    let key = format!("{kind_key}{}", if slots { "-SLOT" } else { "" });
    let label = match (slots, non_plated, is_plated) {
        (false, ..) => kind.to_string(),
        (true, true, _) => "Slots, non-plated".to_string(),
        (true, _, true) => "Slots, plated".to_string(),
        _ => "Slots".to_string(),
    };
    let color = if non_plated { TEAL } else { WHITE };
    role(&key, label, Group::Drill, 600 + if non_plated { 5 } else { 0 } + slots as i32, color, true, false)
}

/// For Gerber files from other programs: what the file says about itself.
pub fn from_file_function(function: &str) -> Option<Role> {
    let f: Vec<&str> = function.split(',').collect();
    let side = f.iter().any(|t| *t == "Top");
    let bottom = f.iter().any(|t| *t == "Bot");
    match f.first().copied()? {
        "Copper" if side => from_extension("GTL"),
        "Copper" if bottom => from_extension("GBL"),
        "Copper" => {
            let n: u32 = f.get(1)?.trim_start_matches('L').parse().ok()?;
            from_extension(&format!("G{}", n.saturating_sub(1).max(1)))
        }
        "Soldermask" if side => from_extension("GTS"),
        "Soldermask" if bottom => from_extension("GBS"),
        "Paste" if side => from_extension("GTP"),
        "Paste" if bottom => from_extension("GBP"),
        "Legend" if side => from_extension("GTO"),
        "Legend" if bottom => from_extension("GBO"),
        "Profile" => from_extension("GKO"),
        _ => None,
    }
}

/// Last resort, for names such as board-top-copper.gbr.
pub fn from_name(stem: &str) -> Option<Role> {
    let s = stem.to_lowercase();
    let top = s.contains("top") || s.contains("f_") || s.contains("front");
    let bottom = s.contains("bot") || s.contains("b_") || s.contains("back");
    let pick = |t: &str, b: &str| {
        if top {
            from_extension(t)
        } else if bottom {
            from_extension(b)
        } else {
            None
        }
    };
    if s.contains("mask") {
        pick("GTS", "GBS")
    } else if s.contains("paste") {
        pick("GTP", "GBP")
    } else if s.contains("silk") || s.contains("overlay") || s.contains("legend") {
        pick("GTO", "GBO")
    } else if s.contains("cu") || s.contains("copper") {
        pick("GTL", "GBL")
    } else if s.contains("outline") || s.contains("profile") || s.contains("edge") {
        from_extension("GKO")
    } else {
        None
    }
}

pub fn unknown_role(file: &str) -> Role {
    role(&format!("FILE:{}", file.to_uppercase()), file.to_string(), Group::Other, 900, GREY, false, false)
}

/// Altium output that isn't a drawing: reports and settings.
pub fn is_side_file(ext: &str) -> bool {
    matches!(
        ext.to_uppercase().as_str(),
        "REP" | "EXTREP" | "RUL" | "DRR" | "LDP" | "APR" | "APR_LIB" | "CAM" | "HTML" | "HTM" | "PDF" | "OUTJOB"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn altium_endings_get_real_names() {
        for (ext, label) in [
            ("GTL", "Top copper"),
            ("gbl", "Bottom copper"),
            ("GTO", "Top overlay"),
            ("GBS", "Bottom solder mask"),
            ("GKO", "Keep-out"),
            ("G1", "Mid layer 1"),
            ("G30", "Mid layer 30"),
            ("GP2", "Power plane 2"),
            ("GM13", "Mechanical 13"),
            ("GD1", "Drill drawing 1"),
            ("GG1", "Drill guide 1"),
            ("GPT", "Top pad master"),
        ] {
            assert_eq!(from_extension(ext).expect(ext).label, label);
        }
        assert!(from_extension("TXT").is_none());
        assert!(from_extension("GX").is_none());
    }

    #[test]
    fn layers_stack_from_top_to_bottom() {
        let order = |e: &str| from_extension(e).unwrap().order;
        assert!(order("GTO") < order("GTL"));
        assert!(order("GTL") < order("G1"));
        assert!(order("G1") < order("GP1"));
        assert!(order("GP1") < order("GBL"));
        assert!(order("GBL") < order("GBO"));
    }

    #[test]
    fn drill_files_split_by_name() {
        assert_eq!(drill_role("Board-Plated", None).key, "PTH");
        assert_eq!(drill_role("Board-NonPlated", None).key, "NPTH");
        assert_eq!(drill_role("Board-SlotHoles-Plated", None).key, "PTH-SLOT");
        assert_eq!(drill_role("Board-RoundHoles-NonPlated", None).key, "NPTH");
        assert_eq!(drill_role("Board", None).key, "DRILL");
        assert_eq!(drill_role("Board", Some(false)).key, "NPTH");
    }

    #[test]
    fn x2_file_function_names_other_tools_layers() {
        assert_eq!(from_file_function("Copper,L1,Top").unwrap().key, "GTL");
        assert_eq!(from_file_function("Copper,L2,Inr").unwrap().key, "G1");
        assert_eq!(from_file_function("Soldermask,Bot").unwrap().key, "GBS");
    }
}
