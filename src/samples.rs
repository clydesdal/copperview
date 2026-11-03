//! A small made-up four-layer board in two revisions, written out as real
//! Gerber and drill text with Altium's file names. It shows what the program
//! does on first start, and it's what the tests read.

use std::fmt::Write;

pub const BOARD_W: f64 = 60.0;
pub const BOARD_H: f64 = 40.0;

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Round,
    Rect,
    Rounded,
}

#[derive(Clone, Copy)]
struct Pad {
    shape: Shape,
    w: f64,
    h: f64,
    x: f64,
    y: f64,
    net: &'static str,
    comp: &'static str,
    pin: u32,
    through: bool,
}

struct Track {
    w: f64,
    pts: Vec<(f64, f64)>,
    net: &'static str,
    bottom: bool,
}

struct Design {
    pads: Vec<Pad>,
    tracks: Vec<Track>,
    /// Plated holes: x, y, diameter.
    vias: Vec<(f64, f64, f64)>,
}

fn smd(shape: Shape, w: f64, h: f64, x: f64, y: f64, net: &'static str, comp: &'static str, pin: u32) -> Pad {
    Pad { shape, w, h, x, y, net, comp, pin, through: false }
}

fn tht(d: f64, x: f64, y: f64, net: &'static str, comp: &'static str, pin: u32) -> Pad {
    Pad { shape: if pin == 1 { Shape::Rect } else { Shape::Round }, w: d, h: d, x, y, net, comp, pin, through: true }
}

const NETS: [&str; 8] = ["SDA", "SCL", "MISO", "MOSI", "CLK", "CS", "IRQ", "RESET"];

fn design(rev: char) -> Design {
    let mut d = Design { pads: Vec::new(), tracks: Vec::new(), vias: Vec::new() };
    let (ux, uy) = (31.0, 20.0);

    // U1: a 32-pin square chip.
    let pitch = 0.8;
    let reach = 4.3;
    for i in 0..8 {
        let o = (i as f64 - 3.5) * pitch;
        let n = i as u32;
        d.pads.push(smd(Shape::Rounded, 1.5, 0.45, ux - reach, uy - o, NETS[i], "U1", n + 1));
        d.pads.push(smd(Shape::Rounded, 0.45, 1.5, ux + o, uy - reach, if i % 3 == 0 { "GND" } else { "+3V3" }, "U1", n + 9));
        d.pads.push(smd(Shape::Rounded, 1.5, 0.45, ux + reach, uy + o, NETS[7 - i], "U1", n + 17));
        d.pads.push(smd(Shape::Rounded, 0.45, 1.5, ux - o, uy + reach, if i % 2 == 0 { "+3V3" } else { "GND" }, "U1", n + 25));
    }

    // J1: a two-row header on the left, wired to the chip's left side.
    for i in 0..8 {
        let y = 11.11 + i as f64 * 2.54;
        d.pads.push(tht(1.7, 7.0, y, NETS[7 - i], "J1", i as u32 * 2 + 1));
        d.pads.push(tht(1.7, 9.54, y, "GND", "J1", i as u32 * 2 + 2));
        let chip_y = uy + (i as f64 - 3.5) * pitch;
        let bend = 16.0 + (i as f64 - 3.5).abs() * 0.8;
        d.tracks.push(Track {
            w: 0.25,
            pts: vec![(7.0, y), (bend - (y - chip_y).abs() * 0.0, y), (bend + (y - chip_y).abs(), chip_y), (ux - reach, chip_y)],
            net: NETS[7 - i],
            bottom: false,
        });
    }

    // Right side: small resistors and capacitors in a column.
    for i in 0..8 {
        let y = 11.5 + i as f64 * 2.4;
        let mut x = 45.5;
        if rev == 'B' && i == 5 {
            x += 1.5; // Rev B: R6 moved.
        }
        let comp = ["R1", "R2", "R3", "R4", "R5", "R6", "C1", "C2"][i];
        d.pads.push(smd(Shape::Rounded, 0.9, 0.95, x - 0.8, y, NETS[i], comp, 1));
        d.pads.push(smd(Shape::Rounded, 0.9, 0.95, x + 0.8, y, "+3V3", comp, 2));
        let chip_y = uy + (3.5 - i as f64) * pitch;
        let w = if rev == 'B' && i == 2 { 0.4 } else { 0.25 }; // Rev B: one track widened.
        let lane = 39.0 + (i as f64 - 3.5).abs() * 0.6;
        d.tracks.push(Track {
            w,
            pts: vec![(ux + reach, chip_y), (lane - (y - chip_y).abs(), chip_y), (lane, y), (x - 0.8, y)],
            net: NETS[i],
            bottom: false,
        });
        d.vias.push((x + 3.2, y, 0.3));
        d.tracks.push(Track { w: 0.3, pts: vec![(x + 0.8, y), (x + 3.2, y)], net: "+3V3", bottom: false });
        d.pads.push(tht(0.6, x + 3.2, y, "+3V3", "", 2));
    }
    // A power rail along the bottom layer joining the vias.
    d.tracks.push(Track { w: 0.6, pts: vec![(48.7, 11.5), (48.7, 28.3)], net: "+3V3", bottom: true });
    d.tracks.push(Track { w: 0.6, pts: vec![(48.7, 28.3), (44.0, 33.0), (20.0, 33.0)], net: "+3V3", bottom: true });

    // Stitching vias under the chip.
    for (x, y) in [(29.0, 18.0), (33.0, 18.0), (29.0, 22.0), (33.0, 22.0), (31.0, 20.0)] {
        d.vias.push((x, y, 0.4));
        d.pads.push(tht(0.8, x, y, "GND", "", 2));
    }
    if rev == 'B' {
        // Rev B: an extra via and a hole made bigger.
        d.vias.push((20.0, 8.0, 0.3));
        d.pads.push(tht(0.6, 20.0, 8.0, "+3V3", "", 2));
        d.tracks.push(Track { w: 0.3, pts: vec![(20.0, 8.0), (20.0, 33.0)], net: "+3V3", bottom: true });
        d.vias[8].2 = 0.5;
    }
    d
}

struct G {
    s: String,
    code: u32,
    shapes: Vec<(u64, u32)>,
    shift: (f64, f64),
}

fn coord(v: f64) -> i64 {
    (v * 1e6).round() as i64
}

impl G {
    fn new(function: &str, shift: (f64, f64)) -> Self {
        let mut s = String::new();
        s.push_str("G04 Layer_Physical_Order=1*\nG04 Layer_Color=255*\n");
        s.push_str("%FSLAX46Y46*%\n%MOMM*%\n");
        s.push_str("%TF.GenerationSoftware,Altium Limited,Altium Designer,24.0*%\n");
        let _ = writeln!(s, "%TF.FileFunction,{function}*%");
        s.push_str("%AMROUNDED*\n21,1,$1,$2-$3-$3,0,0,0*\n21,1,$1-$3-$3,$2,0,0,0*\n1,1,$3+$3,$1/2-$3,$2/2-$3*\n1,1,$3+$3,-$1/2+$3,$2/2-$3*\n1,1,$3+$3,$1/2-$3,-$2/2+$3*\n1,1,$3+$3,-$1/2+$3,-$2/2+$3*\n%\n");
        G { s, code: 10, shapes: Vec::new(), shift }
    }

    fn select(&mut self, shape: Shape, w: f64, h: f64) {
        let key = (coord(w) as u64) << 24 ^ (coord(h) as u64) << 2 ^ shape as u64;
        if let Some((_, code)) = self.shapes.iter().find(|(k, _)| *k == key) {
            let _ = writeln!(self.s, "D{code}*");
            return;
        }
        let code = self.code;
        self.code += 1;
        self.shapes.push((key, code));
        let def = match shape {
            Shape::Round => format!("C,{w:.4}"),
            Shape::Rect => format!("R,{w:.4}X{h:.4}"),
            Shape::Rounded => format!("ROUNDED,{w:.4}X{h:.4}X{:.4}", w.min(h) * 0.25),
        };
        let _ = writeln!(self.s, "%ADD{code}{def}*%\nD{code}*");
    }

    fn xy(&self, x: f64, y: f64) -> String {
        format!("X{}Y{}", coord(x + self.shift.0), coord(y + self.shift.1))
    }

    fn net(&mut self, net: &str) {
        if !net.is_empty() {
            let _ = writeln!(self.s, "%TO.N,{net}*%");
        }
    }

    fn pad(&mut self, pad: &Pad, grow: f64) {
        self.net(pad.net);
        if !pad.comp.is_empty() {
            let _ = writeln!(self.s, "%TO.P,{},{}*%", pad.comp, pad.pin);
        }
        self.select(pad.shape, pad.w + grow, pad.h + grow);
        let at = self.xy(pad.x, pad.y);
        let _ = writeln!(self.s, "{at}D03*\n%TD*%");
    }

    fn stroke(&mut self, pts: &[(f64, f64)], w: f64, net: &str) {
        self.net(net);
        self.select(Shape::Round, w, w);
        let start = self.xy(pts[0].0, pts[0].1);
        let _ = writeln!(self.s, "{start}D02*");
        for q in &pts[1..] {
            let at = self.xy(q.0, q.1);
            let _ = writeln!(self.s, "{at}D01*");
        }
        self.s.push_str("%TD*%\n");
    }

    /// An arc that turns anticlockwise from `from`, around `centre`, to `to`.
    fn arc(&mut self, from: (f64, f64), to: (f64, f64), centre: (f64, f64), w: f64) {
        self.select(Shape::Round, w, w);
        let start = self.xy(from.0, from.1);
        let end = self.xy(to.0, to.1);
        let (i, j) = (coord(centre.0 - from.0), coord(centre.1 - from.1));
        let _ = writeln!(self.s, "G75*\n{start}D02*\nG03*\n{end}I{i}J{j}D01*\nG01*");
    }

    fn fill(&mut self, ring: &[(f64, f64)], net: &str) {
        self.net(net);
        self.s.push_str("G36*\n");
        let start = self.xy(ring[0].0, ring[0].1);
        let _ = writeln!(self.s, "{start}D02*");
        for q in ring[1..].iter().chain(std::iter::once(&ring[0])) {
            let at = self.xy(q.0, q.1);
            let _ = writeln!(self.s, "{at}D01*");
        }
        self.s.push_str("G37*\n%TD*%\n");
    }

    fn dark(&mut self, on: bool) {
        self.s.push_str(if on { "%LPD*%\n" } else { "%LPC*%\n" });
    }

    fn done(mut self) -> String {
        self.s.push_str("M02*\n");
        self.s
    }
}

const POUR: [(f64, f64); 8] = [
    (2.0, 4.0),
    (4.0, 2.0),
    (56.0, 2.0),
    (58.0, 4.0),
    (58.0, 36.0),
    (56.0, 38.0),
    (4.0, 38.0),
    (2.0, 36.0),
];

const HOLES: [(f64, f64); 4] = [(4.5, 4.5), (55.5, 4.5), (4.5, 35.5), (55.5, 35.5)];

fn copper(d: &Design, bottom: bool, shift: (f64, f64)) -> String {
    let side = if bottom { "Copper,L4,Bot" } else { "Copper,L1,Top" };
    let mut g = G::new(side, shift);
    // The bottom is a ground pour with clearances cut around everything on it.
    if bottom {
        g.fill(&POUR, "GND");
        g.dark(false);
        for (x, y) in HOLES {
            g.stroke(&[(x, y), (x, y)], 4.6, "");
        }
        for t in d.tracks.iter().filter(|t| t.bottom) {
            g.stroke(&t.pts, t.w + 0.6, "");
        }
        for pad in d.pads.iter().filter(|q| q.through && q.net != "GND") {
            let mut clear = *pad;
            clear.net = "";
            clear.comp = "";
            g.pad(&clear, 0.6);
        }
        g.dark(true);
    }
    for t in d.tracks.iter().filter(|t| t.bottom == bottom) {
        g.stroke(&t.pts, t.w, t.net);
    }
    for pad in d.pads.iter().filter(|q| !bottom || q.through) {
        g.pad(pad, 0.0);
    }
    g.done()
}

fn inner(d: &Design, plane: bool, shift: (f64, f64)) -> String {
    let mut g = G::new(if plane { "Copper,L3,Inr,Plane" } else { "Copper,L2,Inr" }, shift);
    if plane {
        g.fill(&POUR, "+3V3");
        g.dark(false);
        for pad in d.pads.iter().filter(|q| q.through && q.net != "+3V3") {
            g.stroke(&[(pad.x, pad.y), (pad.x, pad.y)], pad.w + 0.8, "");
        }
    } else {
        for i in 0..6 {
            let y = 8.0 + i as f64 * 5.0;
            g.stroke(&[(12.0, y), (22.0, y), (26.0, 20.0), (36.0, 20.0)], 0.2, "SIG");
        }
    }
    g.done()
}

fn mask(d: &Design, bottom: bool, shift: (f64, f64)) -> String {
    let mut g = G::new(if bottom { "Soldermask,Bot" } else { "Soldermask,Top" }, shift);
    for pad in d.pads.iter().filter(|q| !bottom || q.through) {
        g.pad(pad, 0.1);
    }
    g.done()
}

fn paste(d: &Design, shift: (f64, f64)) -> String {
    let mut g = G::new("Paste,Top", shift);
    for pad in d.pads.iter().filter(|q| !q.through) {
        g.pad(pad, -0.05);
    }
    g.done()
}

fn glyph(c: char) -> &'static [&'static [(f64, f64)]] {
    match c {
        'U' => &[&[(0.0, 1.0), (0.0, 0.0), (0.6, 0.0), (0.6, 1.0)]],
        'J' => &[&[(0.6, 1.0), (0.6, 0.0), (0.0, 0.0), (0.0, 0.3)]],
        'R' => &[&[(0.0, 0.0), (0.0, 1.0), (0.6, 1.0), (0.6, 0.5), (0.0, 0.5)], &[(0.3, 0.5), (0.6, 0.0)]],
        'C' => &[&[(0.6, 0.0), (0.0, 0.0), (0.0, 1.0), (0.6, 1.0)]],
        '1' => &[&[(0.3, 0.0), (0.3, 1.0), (0.1, 0.8)]],
        _ => &[],
    }
}

fn label(g: &mut G, text: &str, x: f64, y: f64, size: f64) {
    for (i, c) in text.chars().enumerate() {
        for stroke in glyph(c) {
            let pts: Vec<(f64, f64)> =
                stroke.iter().map(|q| (x + (q.0 + i as f64 * 0.9) * size, y + q.1 * size)).collect();
            g.stroke(&pts, 0.15, "");
        }
    }
}

fn overlay(bottom: bool, shift: (f64, f64)) -> String {
    let mut g = G::new(if bottom { "Legend,Bot" } else { "Legend,Top" }, shift);
    if bottom {
        for (x, y) in HOLES {
            g.stroke(&[(x - 3.0, y), (x + 3.0, y)], 0.15, "");
            g.stroke(&[(x, y - 3.0), (x, y + 3.0)], 0.15, "");
        }
        return g.done();
    }
    // Chip outline with a corner mark.
    g.stroke(&[(27.3, 16.3), (34.7, 16.3), (34.7, 23.7), (27.3, 23.7), (27.3, 16.3)], 0.15, "");
    g.stroke(&[(25.2, 24.7), (25.2, 24.7)], 0.4, "");
    // Header outline.
    g.stroke(&[(5.6, 9.7), (10.9, 9.7), (10.9, 30.3), (5.6, 30.3), (5.6, 9.7)], 0.15, "");
    label(&mut g, "U1", 29.8, 25.9, 1.2);
    label(&mut g, "J1", 6.8, 31.0, 1.2);
    label(&mut g, "C1", 43.0, 7.8, 1.0);
    g.done()
}

fn outline(shift: (f64, f64)) -> String {
    let mut g = G::new("Profile,NP", shift);
    let (w, h, r) = (BOARD_W, BOARD_H, 3.0);
    g.stroke(&[(r, 0.0), (w - r, 0.0)], 0.2, "");
    g.arc((w - r, 0.0), (w, r), (w - r, r), 0.2);
    g.stroke(&[(w, r), (w, h - r)], 0.2, "");
    g.arc((w, h - r), (w - r, h), (w - r, h - r), 0.2);
    g.stroke(&[(w - r, h), (r, h)], 0.2, "");
    g.arc((r, h), (0.0, h - r), (r, h - r), 0.2);
    g.stroke(&[(0.0, h - r), (0.0, r)], 0.2, "");
    g.arc((0.0, r), (r, 0.0), (r, r), 0.2);
    g.done()
}

fn drill(plated: bool, holes: &[(f64, f64, f64)], shift: (f64, f64)) -> String {
    let mut s = String::from(";Layer_Color=9474304\n;FILE_FORMAT=4:4\nM48\nMETRIC,LZ\n");
    s.push_str(if plated { ";TYPE=PLATED\n" } else { ";TYPE=NON_PLATED\n" });
    let mut sizes: Vec<i64> = holes.iter().map(|h| coord(h.2)).collect();
    sizes.sort_unstable();
    sizes.dedup();
    for (i, size) in sizes.iter().enumerate() {
        let _ = writeln!(s, "T{}C{:.3}", i + 1, *size as f64 / 1e6);
    }
    s.push_str("%\n");
    for (i, size) in sizes.iter().enumerate() {
        let _ = writeln!(s, "T{:02}", i + 1);
        for h in holes.iter().filter(|h| coord(h.2) == *size) {
            let _ = writeln!(s, "X{:.3}Y{:.3}", h.0 + shift.0, h.1 + shift.1);
        }
    }
    s.push_str("M30\n");
    s
}

fn write(rev: char, shift: (f64, f64)) -> Vec<(String, String)> {
    let d = design(rev);
    let name = "Demo_Board";
    let mut plated = d.vias.clone();
    for pad in d.pads.iter().filter(|q| q.through && q.comp == "J1") {
        plated.push((pad.x, pad.y, 1.0));
    }
    let free: Vec<(f64, f64, f64)> = HOLES.iter().map(|h| (h.0, h.1, 3.2)).collect();
    vec![
        (format!("{name}.GTL"), copper(&d, false, shift)),
        (format!("{name}.GBL"), copper(&d, true, shift)),
        (format!("{name}.G1"), inner(&d, false, shift)),
        (format!("{name}.GP1"), inner(&d, true, shift)),
        (format!("{name}.GTS"), mask(&d, false, shift)),
        (format!("{name}.GBS"), mask(&d, true, shift)),
        (format!("{name}.GTP"), paste(&d, shift)),
        (format!("{name}.GTO"), overlay(false, shift)),
        (format!("{name}.GBO"), overlay(true, shift)),
        (format!("{name}.GKO"), outline(shift)),
        (format!("{name}-Plated.TXT"), drill(true, &plated, shift)),
        (format!("{name}-NonPlated.TXT"), drill(false, &free, shift)),
        ("Status Report.Txt".to_string(), "Output: Gerber Files\nType : Gerber\n".to_string()),
    ]
}

/// One revision ('A' or 'B') of the sample board.
pub fn board(rev: char) -> Vec<(String, String)> {
    write(rev, (0.0, 0.0))
}

/// Revision A with its origin moved.
#[cfg(test)]
pub fn shifted(x: f64, y: f64) -> Vec<(String, String)> {
    write('A', (x, y))
}
