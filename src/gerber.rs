//! Reads Gerber files (RS-274X and X2): the shapes, and everything the file
//! says about them, such as nets, components and pad functions.

use crate::geom::*;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Kind {
    Pad,
    Track,
    Arc,
    Region,
    Hole,
    Slot,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Pad => "Pad",
            Kind::Track => "Track",
            Kind::Arc => "Arc",
            Kind::Region => "Shape",
            Kind::Hole => "Hole",
            Kind::Slot => "Slot",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArcInfo {
    pub centre: P,
    pub radius: f64,
    pub sweep_deg: f64,
}

pub const NONE: u32 = u32::MAX;

/// One thing drawn in a file. Strings are numbers into `Parsed::strings`; 0 means none.
#[derive(Clone, Debug)]
pub struct Feature {
    pub kind: Kind,
    pub dark: bool,
    /// Pad and Hole: the centre. Track and Slot: both ends. Arc: points along
    /// it. Region: the outline.
    pub path: Vec<P>,
    pub ap: u32,
    pub width: f64,
    pub arc: Option<ArcInfo>,
    pub net: u32,
    pub comp: u32,
    pub pin: u32,
    pub func: u32,
    pub bbox: BBox,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApShape {
    Circle,
    Rect,
    Obround,
    Polygon(u32),
    Macro(String),
    Block,
}

/// A solid piece of an aperture. `rings[0]` is its outside; the rest are holes.
#[derive(Clone, Default)]
pub struct Part {
    pub dark: bool,
    pub rings: Vec<Vec<P>>,
    pub tris: Tris,
}

impl Part {
    fn new(dark: bool, rings: Vec<Vec<P>>) -> Self {
        let tris = triangulate(&rings);
        Part { dark, rings, tris }
    }
}

/// A pad shape or drawing pen.
#[derive(Clone)]
pub struct Aperture {
    pub code: u32,
    pub shape: ApShape,
    pub w: f64,
    pub h: f64,
    pub hole: f64,
    pub func: u32,
    pub parts: Arc<Vec<Part>>,
    pub used: u32,
}

impl Aperture {
    pub fn describe(&self) -> &str {
        match &self.shape {
            ApShape::Circle => "Round",
            ApShape::Rect => "Rectangle",
            ApShape::Obround => "Oblong",
            ApShape::Polygon(_) => "Polygon",
            ApShape::Macro(name) if name.to_lowercase().contains("round") => "Rounded rectangle",
            ApShape::Macro(name) if name.to_lowercase().contains("oct") => "Octagon",
            ApShape::Macro(_) => "Custom",
            ApShape::Block => "Block",
        }
    }
}

#[derive(Default, Clone, Debug)]
pub struct Tool {
    pub number: u32,
    pub dia: f64,
    pub count: u32,
}

#[derive(Default)]
pub struct Parsed {
    pub features: Vec<Feature>,
    pub apertures: Vec<Aperture>,
    /// Entry 0 is always empty.
    pub strings: Vec<String>,
    pub bbox: BBox,
    pub warnings: Vec<String>,
    pub file_function: Option<String>,
    pub software: Option<String>,
    pub units: &'static str,
    pub format: String,
    pub negative: bool,
    pub is_drill: bool,
    pub plated: Option<bool>,
    pub tools: Vec<Tool>,
}

impl Parsed {
    pub fn text(&self, id: u32) -> Option<&str> {
        match id {
            0 => None,
            _ => self.strings.get(id as usize).map(String::as_str),
        }
    }

    pub fn finish(&mut self) {
        let mut bbox = BBox::EMPTY;
        for f in &self.features {
            bbox.union(&f.bbox);
        }
        self.bbox = bbox;
    }
}

pub fn feature_bbox(path: &[P], grow: f64) -> BBox {
    let mut b = BBox::EMPTY;
    for &q in path {
        b.add(q);
    }
    b.grown(grow)
}

impl Feature {
    /// The feature's outside edge, as closed rings.
    pub fn outline(&self, apertures: &[Aperture]) -> Vec<Vec<P>> {
        match self.kind {
            Kind::Pad => {
                let Some(ap) = apertures.get(self.ap as usize) else { return vec![] };
                ap.parts
                    .iter()
                    .filter(|part| part.dark)
                    .flat_map(|part| part.rings.iter())
                    .map(|ring| ring.iter().map(|q| q.add(self.path[0])).collect())
                    .collect()
            }
            Kind::Hole => vec![circle_ring(self.path[0], self.width)],
            Kind::Track | Kind::Slot => match self.stroke_hull(apertures) {
                Some(hull) => vec![hull],
                None => vec![capsule_ring(self.path[0], self.path[1], self.width)],
            },
            Kind::Arc => vec![offset_ring(&self.path, self.width)],
            Kind::Region => vec![self.path.clone()],
        }
    }

    /// Tracks drawn with a square pen have square ends.
    fn stroke_hull(&self, apertures: &[Aperture]) -> Option<Vec<P>> {
        let ap = apertures.get(self.ap as usize)?;
        if ap.shape == ApShape::Circle || ap.parts.is_empty() {
            return None;
        }
        let mut pts = Vec::new();
        for ring in ap.parts.iter().flat_map(|part| part.rings.first()) {
            for &q in ring {
                pts.push(q.add(self.path[0]));
                pts.push(q.add(self.path[1]));
            }
        }
        Some(hull(pts))
    }

    /// Whether this point is on the feature, and if not whether it's within `tol`.
    pub fn hit(&self, q: P, tol: f64, apertures: &[Aperture]) -> Hit {
        if !self.bbox.grown(tol).contains(q) {
            return Hit::Miss;
        }
        let near = |d: f64| {
            if d <= 0.0 {
                Hit::On
            } else if d <= tol {
                Hit::Near
            } else {
                Hit::Miss
            }
        };
        match self.kind {
            Kind::Pad => {
                let Some(ap) = apertures.get(self.ap as usize) else { return Hit::Miss };
                let local = q.sub(self.path[0]);
                let mut best = Hit::Miss;
                for part in ap.parts.iter().filter(|part| part.dark) {
                    let inside = part.rings.iter().filter(|r| point_in_ring(local, r)).count();
                    if inside % 2 == 1 {
                        return Hit::On;
                    }
                    if part.rings.iter().any(|r| dist_to_ring(local, r) <= tol) {
                        best = Hit::Near;
                    }
                }
                best
            }
            Kind::Hole => near(q.dist(self.path[0]) - self.width / 2.0),
            Kind::Track | Kind::Slot => {
                if let Some(hull) = self.stroke_hull(apertures) {
                    return if point_in_ring(q, &hull) { Hit::On } else { near(dist_to_ring(q, &hull)) };
                }
                near(dist_to_segment(q, self.path[0], self.path[1]) - self.width / 2.0)
            }
            Kind::Arc => {
                let mut best = f64::INFINITY;
                for pair in self.path.windows(2) {
                    best = best.min(dist_to_segment(q, pair[0], pair[1]));
                }
                near(best - self.width / 2.0)
            }
            Kind::Region => {
                if point_in_ring(q, &self.path) {
                    Hit::On
                } else {
                    near(dist_to_ring(q, &self.path))
                }
            }
        }
    }

    pub fn length(&self) -> f64 {
        match self.kind {
            Kind::Track | Kind::Slot | Kind::Arc => self.path.windows(2).map(|w| w[0].dist(w[1])).sum(),
            _ => 0.0,
        }
    }

    pub fn tessellate(&self, apertures: &[Aperture], mesh: &mut Mesh) {
        let value = if self.dark { 1.0 } else { 0.0 };
        match self.kind {
            Kind::Pad => {
                let Some(ap) = apertures.get(self.ap as usize) else { return };
                for part in ap.parts.iter() {
                    // Exposure-off pieces of a pad clear what's beneath them.
                    let v = if part.dark {
                        value
                    } else if self.dark {
                        0.0
                    } else {
                        continue;
                    };
                    mesh.soup(&part.tris, self.path[0], v);
                }
            }
            Kind::Hole => mesh.convex(&circle_ring(self.path[0], self.width), value),
            Kind::Track | Kind::Slot => {
                if self.width <= 0.0 {
                    return;
                }
                match self.stroke_hull(apertures) {
                    Some(hull) => mesh.convex(&hull, value),
                    None => mesh.convex(&capsule_ring(self.path[0], self.path[1], self.width), value),
                }
            }
            Kind::Arc => {
                if self.width > 0.0 {
                    mesh.strip(&self.path, self.width, value);
                }
            }
            Kind::Region => {
                mesh.soup(&triangulate(std::slice::from_ref(&self.path)), p(0.0, 0.0), value);
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    On,
    Near,
    Miss,
}

#[derive(Clone, Copy, PartialEq)]
struct ApXf {
    mirror_x: bool,
    mirror_y: bool,
    rot: f64,
    scale: f64,
}

impl ApXf {
    const NONE: ApXf = ApXf { mirror_x: false, mirror_y: false, rot: 0.0, scale: 1.0 };

    fn apply(&self, q: P) -> P {
        let m = p(if self.mirror_x { -q.x } else { q.x }, if self.mirror_y { -q.y } else { q.y });
        m.rot(self.rot).scale(self.scale)
    }
}

struct Stepper {
    start: usize,
    nx: u32,
    ny: u32,
    dx: f64,
    dy: f64,
}

struct Reader {
    out: Parsed,
    interned: HashMap<String, u32>,
    macros: HashMap<String, Vec<String>>,
    by_code: HashMap<u32, u32>,
    blocks: HashMap<u32, Vec<Feature>>,
    block_stack: Vec<(u32, usize)>,
    xf_memo: HashMap<(u32, bool, bool, u64, u64), u32>,
    step: Option<Stepper>,

    int_x: u32,
    dec_x: u32,
    int_y: u32,
    dec_y: u32,
    trailing: bool,
    incremental: bool,
    have_format: bool,
    have_units: bool,
    unit: f64,

    cur: P,
    raw: (f64, f64),
    ap: u32,
    interp: u8, // 1 line, 2 clockwise, 3 anticlockwise
    multi_quadrant: bool,
    last_op: u32,
    dark: bool,
    xf: ApXf,

    region: bool,
    contour: Vec<P>,

    net: u32,
    comp: u32,
    pin: u32,
    func: u32,

    axis_swap: bool,
    img_mirror: (bool, bool),
    img_rot: f64,
    img_scale: (f64, f64),
    img_offset: (f64, f64),
}

fn number(text: &str) -> Option<f64> {
    text.trim().parse::<f64>().ok()
}

fn hash_f(v: f64) -> u64 {
    v.to_bits()
}

/// Works out one arithmetic expression in an aperture macro.
struct Expr<'a> {
    s: &'a [u8],
    i: usize,
    vars: &'a HashMap<u32, f64>,
}

impl Expr<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }
    fn sum(&mut self) -> f64 {
        let mut v = self.product();
        while let Some(c @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.product();
            if c == b'+' {
                v += r;
            } else {
                v -= r;
            }
        }
        v
    }
    fn product(&mut self) -> f64 {
        let mut v = self.unary();
        while let Some(c @ (b'x' | b'X' | b'/')) = self.peek() {
            self.i += 1;
            let r = self.unary();
            if c == b'/' {
                v /= r;
            } else {
                v *= r;
            }
        }
        v
    }
    fn unary(&mut self) -> f64 {
        match self.peek() {
            Some(b'-') => {
                self.i += 1;
                -self.unary()
            }
            Some(b'+') => {
                self.i += 1;
                self.unary()
            }
            Some(b'(') => {
                self.i += 1;
                let v = self.sum();
                if self.peek() == Some(b')') {
                    self.i += 1;
                }
                v
            }
            Some(b'$') => {
                self.i += 1;
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    self.i += 1;
                }
                let id = std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|t| t.parse().ok());
                id.and_then(|id| self.vars.get(&id)).copied().unwrap_or(0.0)
            }
            _ => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == b'.') {
                    self.i += 1;
                }
                if self.i == start {
                    // Not a number at all: step over it rather than get stuck.
                    self.i += 1;
                    return 0.0;
                }
                std::str::from_utf8(&self.s[start..self.i]).ok().and_then(number).unwrap_or(0.0)
            }
        }
    }
}

fn eval(text: &str, vars: &HashMap<u32, f64>) -> f64 {
    let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    Expr { s: clean.as_bytes(), i: 0, vars }.sum()
}

fn exposure(value: f64, previous: bool) -> bool {
    if value >= 1.5 { !previous } else { value >= 0.5 }
}

/// Runs an aperture macro, giving the pieces it makes.
fn expand_macro(words: &[String], params: &[f64], unit: f64) -> Vec<Part> {
    let mut vars: HashMap<u32, f64> =
        params.iter().enumerate().map(|(i, v)| (i as u32 + 1, *v)).collect();
    let mut parts = Vec::new();
    let mut last = true;
    for word in words {
        let word = word.trim();
        if word.is_empty() || word.starts_with("0 ") || word == "0" {
            continue;
        }
        if let Some(rest) = word.strip_prefix('$')
            && let Some((name, value)) = rest.split_once('=')
        {
            if let Ok(id) = name.trim().parse::<u32>() {
                let v = eval(value, &vars);
                vars.insert(id, v);
            }
            continue;
        }
        let v: Vec<f64> = word.split(',').map(|t| eval(t, &vars)).collect();
        let get = |i: usize| v.get(i).copied().unwrap_or(0.0);
        let prim = get(0) as i32;
        let place = |q: P, rot: f64| q.scale(unit).rot(rot);
        match prim {
            1 => {
                let on = exposure(get(1), last);
                last = on;
                let c = place(p(get(3), get(4)), get(5));
                parts.push(Part::new(on, vec![circle_ring(c, get(2) * unit)]));
            }
            20 => {
                let on = exposure(get(1), last);
                last = on;
                let (w, a, b, rot) = (get(2), p(get(3), get(4)), p(get(5), get(6)), get(7));
                let d = b.sub(a);
                let l = d.len().max(1e-12);
                let n = p(-d.y / l * w / 2.0, d.x / l * w / 2.0);
                let ring = [a.add(n), b.add(n), b.sub(n), a.sub(n)];
                parts.push(Part::new(on, vec![ring.iter().map(|q| place(*q, rot)).collect()]));
            }
            21 | 22 => {
                let on = exposure(get(1), last);
                last = on;
                let (w, h, rot) = (get(2), get(3), get(6));
                let c = if prim == 21 {
                    p(get(4), get(5))
                } else {
                    p(get(4) + w / 2.0, get(5) + h / 2.0)
                };
                let ring = rect_ring(c, w, h).iter().map(|q| place(*q, rot)).collect();
                parts.push(Part::new(on, vec![ring]));
            }
            4 => {
                let on = exposure(get(1), last);
                last = on;
                let n = (get(2) as usize).min(v.len());
                let rot = get(5 + 2 * n);
                let mut ring: Vec<P> =
                    (0..=n).map(|i| place(p(get(3 + 2 * i), get(4 + 2 * i)), rot)).collect();
                if ring.len() > 1 && ring.first() == ring.last() {
                    ring.pop();
                }
                parts.push(Part::new(on, vec![ring]));
            }
            5 => {
                let on = exposure(get(1), last);
                last = on;
                let (n, c, d, rot) = (get(2) as usize, p(get(3), get(4)), get(5), get(6));
                let n = n.clamp(3, 64);
                let ring = (0..n)
                    .map(|i| {
                        let a = 2.0 * PI * i as f64 / n as f64;
                        place(c.add(p(a.cos(), a.sin()).scale(d / 2.0)), rot)
                    })
                    .collect();
                parts.push(Part::new(on, vec![ring]));
            }
            6 => {
                let (c, od, thick, gap, rings, cross, cross_len, rot) =
                    (p(get(1), get(2)), get(3), get(4), get(5), get(6) as u32, get(7), get(8), get(9));
                let mut d = od;
                for _ in 0..rings.min(32) {
                    if d <= 0.0 {
                        break;
                    }
                    let centre = place(c, rot);
                    let inner = (d - 2.0 * thick).max(0.0);
                    let mut r = vec![circle_ring(centre, d * unit)];
                    if inner > 0.0 {
                        r.push(circle_ring(centre, inner * unit));
                    }
                    parts.push(Part::new(true, r));
                    d = inner - 2.0 * gap;
                }
                for (w, h) in [(cross_len, cross), (cross, cross_len)] {
                    let ring = rect_ring(c, w, h).iter().map(|q| place(*q, rot)).collect();
                    parts.push(Part::new(true, vec![ring]));
                }
            }
            7 => {
                let (c, od, id, gap, rot) = (p(get(1), get(2)), get(3), get(4), get(5), get(6));
                let (ro, ri) = (od / 2.0, id / 2.0);
                let half = gap / 2.0;
                let ao = (half / ro.max(1e-9)).clamp(-1.0, 1.0).asin();
                let ai = (half / ri.max(1e-9)).clamp(-1.0, 1.0).asin();
                for q in 0..4 {
                    let base = q as f64 * PI / 2.0;
                    let mut ring = Vec::new();
                    let n = circle_steps(ro * unit) / 4 + 2;
                    for i in 0..=n {
                        let t = base + ao + (PI / 2.0 - 2.0 * ao) * i as f64 / n as f64;
                        ring.push(c.add(p(t.cos(), t.sin()).scale(ro)));
                    }
                    for i in (0..=n).rev() {
                        let t = base + ai + (PI / 2.0 - 2.0 * ai) * i as f64 / n as f64;
                        ring.push(c.add(p(t.cos(), t.sin()).scale(ri)));
                    }
                    let ring = ring.iter().map(|q| place(*q, rot)).collect();
                    parts.push(Part::new(true, vec![ring]));
                }
            }
            _ => {}
        }
    }
    parts
}

impl Reader {
    fn new() -> Self {
        Reader {
            out: Parsed { strings: vec![String::new()], units: "mm", ..Default::default() },
            interned: HashMap::new(),
            macros: HashMap::new(),
            by_code: HashMap::new(),
            blocks: HashMap::new(),
            block_stack: Vec::new(),
            xf_memo: HashMap::new(),
            step: None,
            int_x: 2,
            dec_x: 6,
            int_y: 2,
            dec_y: 6,
            trailing: false,
            incremental: false,
            have_format: false,
            have_units: false,
            unit: 25.4,
            cur: P::default(),
            raw: (0.0, 0.0),
            ap: NONE,
            interp: 1,
            multi_quadrant: true,
            last_op: 2,
            dark: true,
            xf: ApXf::NONE,
            region: false,
            contour: Vec::new(),
            net: 0,
            comp: 0,
            pin: 0,
            func: 0,
            axis_swap: false,
            img_mirror: (false, false),
            img_rot: 0.0,
            img_scale: (1.0, 1.0),
            img_offset: (0.0, 0.0),
        }
    }

    fn intern(&mut self, text: &str) -> u32 {
        if let Some(id) = self.interned.get(text) {
            return *id;
        }
        let id = self.out.strings.len() as u32;
        self.out.strings.push(text.to_string());
        self.interned.insert(text.to_string(), id);
        id
    }

    fn warn(&mut self, text: String) {
        if self.out.warnings.len() < 20 && !self.out.warnings.contains(&text) {
            self.out.warnings.push(text);
        }
    }

    fn set_units(&mut self, inch: bool) {
        self.unit = if inch { 25.4 } else { 1.0 };
        self.out.units = if inch { "inch" } else { "mm" };
        self.have_units = true;
    }

    fn coord(&self, text: &str, y: bool) -> f64 {
        let (int, dec) = if y { (self.int_y, self.dec_y) } else { (self.int_x, self.dec_x) };
        if text.contains('.') {
            return number(text).unwrap_or(0.0);
        }
        let negative = text.starts_with('-');
        let digits = text.trim_start_matches(['-', '+']);
        let mut value: f64 = digits.parse().unwrap_or(0.0);
        if self.trailing {
            let missing = (int + dec) as i32 - digits.len() as i32;
            value *= 10f64.powi(missing);
        }
        value /= 10f64.powi(dec as i32);
        if negative { -value } else { value }
    }

    /// Applies the old whole-image settings some older files still use.
    fn image(&self, x: f64, y: f64) -> P {
        let (mut x, mut y) = if self.axis_swap { (y, x) } else { (x, y) };
        if self.img_mirror.0 {
            x = -x;
        }
        if self.img_mirror.1 {
            y = -y;
        }
        let q = p(x * self.img_scale.0 + self.img_offset.0, y * self.img_scale.1 + self.img_offset.1);
        q.rot(self.img_rot).scale(self.unit)
    }

    fn aperture(&self) -> Option<&Aperture> {
        self.out.apertures.get(self.ap as usize)
    }

    fn pen_width(&self) -> f64 {
        self.aperture().map_or(0.0, |a| a.w.min(a.h))
    }

    /// The current aperture with any mirror, rotate and scale settings applied.
    fn placed_aperture(&mut self, code: u32) -> u32 {
        if self.xf == ApXf::NONE || code == NONE {
            return code;
        }
        let key = (code, self.xf.mirror_x, self.xf.mirror_y, hash_f(self.xf.rot), hash_f(self.xf.scale));
        if let Some(found) = self.xf_memo.get(&key) {
            return *found;
        }
        let xf = self.xf;
        let mut ap = self.out.apertures[code as usize].clone();
        let parts: Vec<Part> = ap
            .parts
            .iter()
            .map(|part| {
                let rings = part.rings.iter().map(|r| r.iter().map(|q| xf.apply(*q)).collect()).collect();
                Part::new(part.dark, rings)
            })
            .collect();
        let mut b = BBox::EMPTY;
        parts.iter().flat_map(|part| part.rings.iter().flatten()).for_each(|q| b.add(*q));
        ap.w = b.w();
        ap.h = b.h();
        ap.parts = Arc::new(parts);
        ap.used = 0;
        let id = self.out.apertures.len() as u32;
        self.out.apertures.push(ap);
        self.xf_memo.insert(key, id);
        id
    }

    fn emit(&mut self, kind: Kind, path: Vec<P>, ap: u32, width: f64, arc: Option<ArcInfo>) {
        let grow = match kind {
            Kind::Region => 0.0,
            _ => width / 2.0,
        };
        let func = match self.out.apertures.get(ap as usize) {
            Some(a) if a.func != 0 => a.func,
            _ => self.func,
        };
        if let Some(a) = self.out.apertures.get_mut(ap as usize) {
            a.used += 1;
        }
        let mut f = Feature {
            kind,
            dark: self.dark,
            bbox: feature_bbox(&path, grow),
            path,
            ap,
            width,
            arc,
            net: self.net,
            comp: self.comp,
            pin: self.pin,
            func,
        };
        if let Some(a) = self.out.apertures.get(ap as usize)
            && !matches!(a.shape, ApShape::Circle)
            && kind != Kind::Region
        {
            let b = feature_bbox(&f.path, 0.0);
            f.bbox = BBox {
                min: p(b.min.x - a.w / 2.0, b.min.y - a.h / 2.0),
                max: p(b.max.x + a.w / 2.0, b.max.y + a.h / 2.0),
            };
            if kind == Kind::Pad {
                let mut local = BBox::EMPTY;
                a.parts.iter().flat_map(|part| part.rings.iter().flatten()).for_each(|q| local.add(*q));
                f.bbox = local.shifted(f.path[0]);
            }
        }
        self.out.features.push(f);
    }

    fn flash(&mut self, at: P) {
        if self.ap == NONE {
            self.warn("A pad was drawn before any pad shape was chosen.".into());
            return;
        }
        if let Some(block) = self.blocks.get(&self.ap).cloned() {
            let xf = self.xf;
            let (dark, mut items) = (self.dark, block);
            for f in &mut items {
                for q in &mut f.path {
                    *q = xf.apply(*q).add(at);
                }
                if let Some(arc) = &mut f.arc {
                    arc.centre = xf.apply(arc.centre).add(at);
                }
                f.dark = f.dark == dark;
                f.ap = self.placed_aperture(f.ap);
                f.bbox = feature_bbox(&f.path, f.width / 2.0);
                if let Some(a) = self.out.apertures.get(f.ap as usize)
                    && f.kind == Kind::Pad
                {
                    f.bbox = feature_bbox(&f.path, a.w.max(a.h) / 2.0);
                }
            }
            self.out.features.extend(items);
            return;
        }
        let ap = self.placed_aperture(self.ap);
        let width = self.out.apertures[ap as usize].w;
        self.emit(Kind::Pad, vec![at], ap, width, None);
    }

    fn arc_points(&mut self, end: P, i: f64, j: f64) -> Option<(Vec<P>, ArcInfo)> {
        let start = self.cur;
        let cw = self.interp == 2;
        let sweep_of = |c: P| {
            let a0 = (start.y - c.y).atan2(start.x - c.x);
            let a1 = (end.y - c.y).atan2(end.x - c.x);
            let mut sweep = if cw { a0 - a1 } else { a1 - a0 };
            while sweep < 0.0 {
                sweep += 2.0 * PI;
            }
            (a0, sweep)
        };
        let (centre, a0, sweep) = if self.multi_quadrant {
            let c = start.add(p(i, j));
            let (a0, mut sweep) = sweep_of(c);
            if start.dist(end) < 1e-6 && sweep < 1e-6 {
                sweep = 2.0 * PI;
            }
            (c, a0, sweep)
        } else {
            // Single-quadrant arcs give an unsigned centre: find the one that fits.
            let mut best: Option<(f64, P, f64, f64)> = None;
            for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
                let c = start.add(p(i * sx, j * sy));
                let error = (c.dist(start) - c.dist(end)).abs();
                let (a0, sweep) = sweep_of(c);
                if sweep <= PI / 2.0 + 1e-3 && best.is_none_or(|b| error < b.0) {
                    best = Some((error, c, a0, sweep));
                }
            }
            let (_, c, a0, sweep) = best?;
            (c, a0, sweep)
        };
        let r0 = centre.dist(start);
        let r1 = centre.dist(end);
        if r0 < 1e-9 {
            return None;
        }
        let w = self.pen_width();
        let outer = r0.max(r1) + w / 2.0;
        let step = 2.0 * (1.0 - (0.001 / outer).min(1.0)).acos().max(0.02);
        let n = ((sweep / step).ceil() as usize).clamp(2, 720);
        let sign = if cw { -1.0 } else { 1.0 };
        let mut pts = Vec::with_capacity(n + 1);
        for k in 0..=n {
            let f = k as f64 / n as f64;
            let t = a0 + sign * sweep * f;
            let r = r0 + (r1 - r0) * f;
            pts.push(centre.add(p(t.cos(), t.sin()).scale(r)));
        }
        pts[0] = start;
        pts[n] = end;
        let info = ArcInfo { centre, radius: (r0 + r1) / 2.0, sweep_deg: sweep.to_degrees() };
        Some((pts, info))
    }

    fn close_contour(&mut self) {
        let mut ring = std::mem::take(&mut self.contour);
        if ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        if ring.len() >= 3 {
            self.emit(Kind::Region, ring, NONE, 0.0, None);
        }
    }

    fn operate(&mut self, op: u32, x: Option<&str>, y: Option<&str>, i: Option<&str>, j: Option<&str>) {
        let mut raw = self.raw;
        if let Some(t) = x {
            let v = self.coord(t, false);
            raw.0 = if self.incremental { raw.0 + v } else { v };
        }
        if let Some(t) = y {
            let v = self.coord(t, true);
            raw.1 = if self.incremental { raw.1 + v } else { v };
        }
        self.raw = raw;
        let end = self.image(raw.0, raw.1);
        let off_i = i.map_or(0.0, |t| self.coord(t, false)) * self.unit;
        let off_j = j.map_or(0.0, |t| self.coord(t, true)) * self.unit;
        match op {
            1 => {
                let arc = if self.interp == 1 { None } else { self.arc_points(end, off_i, off_j) };
                if self.region {
                    if self.contour.is_empty() {
                        self.contour.push(self.cur);
                    }
                    match arc {
                        Some((pts, _)) => self.contour.extend_from_slice(&pts[1..]),
                        None => self.contour.push(end),
                    }
                } else if self.ap == NONE {
                    self.warn("A track was drawn before any pen was chosen.".into());
                } else {
                    let ap = self.placed_aperture(self.ap);
                    let w = self.out.apertures[ap as usize].w.min(self.out.apertures[ap as usize].h);
                    match arc {
                        Some((pts, info)) => self.emit(Kind::Arc, pts, ap, w, Some(info)),
                        None => self.emit(Kind::Track, vec![self.cur, end], ap, w, None),
                    }
                }
            }
            2 => {
                if self.region {
                    self.close_contour();
                }
            }
            3 => {
                if self.region {
                    self.warn("A pad flash inside a filled shape was skipped.".into());
                } else {
                    self.flash(end);
                }
            }
            _ => {}
        }
        self.cur = end;
    }

    fn word(&mut self, word: &str) {
        let word: String = word.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = word.as_bytes();
        if bytes.is_empty() || !word.is_ascii() {
            return;
        }
        if word.starts_with("G04") || word.starts_with("G4X") {
            return;
        }
        let (mut x, mut y, mut i, mut j) = (None, None, None, None);
        let mut op: Option<u32> = None;
        let mut at = 0;
        while at < bytes.len() {
            let letter = bytes[at];
            at += 1;
            let start = at;
            while at < bytes.len() && !bytes[at].is_ascii_alphabetic() {
                at += 1;
            }
            let value = &word[start..at];
            match letter.to_ascii_uppercase() {
                b'X' => x = Some(value),
                b'Y' => y = Some(value),
                b'I' => i = Some(value),
                b'J' => j = Some(value),
                b'G' => match value.parse::<u32>().unwrap_or(999) {
                    1 => self.interp = 1,
                    2 => self.interp = 2,
                    3 => self.interp = 3,
                    36 => {
                        self.region = true;
                        self.contour.clear();
                    }
                    37 => {
                        self.close_contour();
                        self.region = false;
                    }
                    70 => self.set_units(true),
                    71 => self.set_units(false),
                    74 => self.multi_quadrant = false,
                    75 => self.multi_quadrant = true,
                    90 => self.incremental = false,
                    91 => self.incremental = true,
                    _ => {}
                },
                b'D' => {
                    let code = value.parse::<u32>().unwrap_or(0);
                    if code >= 10 {
                        match self.by_code.get(&code) {
                            Some(id) => self.ap = *id,
                            None => self.warn(format!("Shape D{code} is used but never defined.")),
                        }
                    } else {
                        op = Some(code);
                    }
                }
                _ => {}
            }
        }
        if x.is_some() || y.is_some() || op.is_some() {
            let code = op.unwrap_or(self.last_op);
            self.last_op = code;
            if op.is_some() || x.is_some() || y.is_some() {
                self.operate(code, x, y, i, j);
            }
        }
    }

    fn define_aperture(&mut self, body: &str) {
        // ADD10C,0.5X0.2
        let rest = &body[3..];
        let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
        let Ok(code) = rest[..digits].parse::<u32>() else { return };
        let rest = &rest[digits..];
        let (name, args) = rest.split_once(',').unwrap_or((rest, ""));
        let vals: Vec<f64> = args.split(['X', 'x']).filter_map(number).collect();
        let u = self.unit;
        let get = |i: usize| vals.get(i).copied().unwrap_or(0.0);
        let o = P::default();
        let with_hole = |ring: Vec<P>, hole: f64| {
            if hole > 0.0 {
                vec![ring, circle_ring(o, hole * u)]
            } else {
                vec![ring]
            }
        };
        let (shape, parts, w, h, hole) = match name {
            "C" => {
                let d = get(0) * u;
                let ring = if d > 0.0 { circle_ring(o, d) } else { Vec::new() };
                (ApShape::Circle, vec![Part::new(true, with_hole(ring, get(1)))], d, d, get(1) * u)
            }
            "R" => {
                let (w, h) = (get(0) * u, get(1) * u);
                let ring = rect_ring(o, w, h);
                (ApShape::Rect, vec![Part::new(true, with_hole(ring, get(2)))], w, h, get(2) * u)
            }
            "O" => {
                let (w, h) = (get(0) * u, get(1) * u);
                let ring = if w >= h {
                    capsule_ring(p(-(w - h) / 2.0, 0.0), p((w - h) / 2.0, 0.0), h)
                } else {
                    capsule_ring(p(0.0, -(h - w) / 2.0), p(0.0, (h - w) / 2.0), w)
                };
                (ApShape::Obround, vec![Part::new(true, with_hole(ring, get(2)))], w, h, get(2) * u)
            }
            "P" => {
                let (d, n, rot) = (get(0) * u, (get(1) as u32).clamp(3, 64), get(2));
                let ring: Vec<P> = (0..n)
                    .map(|i| {
                        let a = 2.0 * PI * i as f64 / n as f64;
                        p(a.cos(), a.sin()).scale(d / 2.0).rot(rot)
                    })
                    .collect();
                let mut b = BBox::EMPTY;
                ring.iter().for_each(|q| b.add(*q));
                let hole = get(3);
                (ApShape::Polygon(n), vec![Part::new(true, with_hole(ring, hole))], b.w(), b.h(), hole * u)
            }
            _ => {
                let Some(words) = self.macros.get(name).cloned() else {
                    self.warn(format!("Pad shape {name} isn't defined."));
                    return;
                };
                let parts = expand_macro(&words, &vals, u);
                let mut b = BBox::EMPTY;
                parts.iter().filter(|part| part.dark).flat_map(|part| part.rings.iter().flatten()).for_each(|q| b.add(*q));
                let (w, h) = (b.w(), b.h());
                (ApShape::Macro(name.to_string()), parts, w, h, 0.0)
            }
        };
        let ap = Aperture { code, shape, w, h, hole, func: self.func, parts: Arc::new(parts), used: 0 };
        self.add_aperture(ap);
    }

    fn add_aperture(&mut self, ap: Aperture) {
        let id = self.out.apertures.len() as u32;
        self.by_code.insert(ap.code, id);
        self.out.apertures.push(ap);
    }

    fn attribute(&mut self, cmd: &str) {
        let Some(body) = cmd.get(2..) else { return };
        let mut fields = body.split(',');
        let name = fields.next().unwrap_or("");
        let args: Vec<&str> = fields.collect();
        match (cmd.get(..2).unwrap_or(""), name) {
            ("TF", ".FileFunction") => self.out.file_function = Some(args.join(",")),
            ("TF", ".GenerationSoftware") => self.out.software = Some(args.join(" ")),
            ("TA", ".AperFunction") | ("TO", ".AperFunction") => {
                self.func = self.intern(&args.join(", "));
            }
            ("TO", ".N") => self.net = self.intern(&args.join(",")),
            ("TO", ".C") => self.comp = self.intern(args.first().copied().unwrap_or("")),
            ("TO", ".P") => {
                if let Some(c) = args.first() {
                    self.comp = self.intern(c);
                }
                let pin = args.get(1..).unwrap_or_default().join(" ");
                self.pin = self.intern(&pin);
            }
            _ => {}
        }
    }

    fn delete_attribute(&mut self, name: &str) {
        match name {
            "" => {
                (self.net, self.comp, self.pin, self.func) = (0, 0, 0, 0);
            }
            ".N" => self.net = 0,
            ".C" => self.comp = 0,
            ".P" => self.pin = 0,
            ".AperFunction" => self.func = 0,
            _ => {}
        }
    }

    fn close_step(&mut self) {
        let Some(step) = self.step.take() else { return };
        let cell: Vec<Feature> = self.out.features[step.start..].to_vec();
        for iy in 0..step.ny {
            for ix in 0..step.nx {
                if ix == 0 && iy == 0 {
                    continue;
                }
                let by = p(ix as f64 * step.dx, iy as f64 * step.dy);
                for f in &cell {
                    let mut f = f.clone();
                    f.path.iter_mut().for_each(|q| *q = q.add(by));
                    f.bbox = f.bbox.shifted(by);
                    if let Some(arc) = &mut f.arc {
                        arc.centre = arc.centre.add(by);
                    }
                    self.out.features.push(f);
                }
            }
        }
    }

    fn extended(&mut self, cmd: &str) {
        if cmd.len() < 2 || !cmd.is_ascii() && !cmd.starts_with('T') {
            return;
        }
        let Some(head) = cmd.get(..2) else { return };
        match head {
            "FS" => {
                let b = cmd.as_bytes();
                self.trailing = b.get(2) == Some(&b'T');
                self.incremental = b.get(3) == Some(&b'I');
                let digits = |letter: char| {
                    let at = cmd.find(letter)?;
                    let d: Vec<u32> = cmd[at + 1..].chars().take(2).filter_map(|c| c.to_digit(10)).collect();
                    (d.len() == 2).then(|| (d[0], d[1]))
                };
                if let (Some(x), Some(y)) = (digits('X'), digits('Y')) {
                    (self.int_x, self.dec_x) = x;
                    (self.int_y, self.dec_y) = y;
                    self.have_format = true;
                    self.out.format = format!("{}:{}", x.0, x.1);
                }
            }
            "MO" => self.set_units(&cmd[2..] == "IN"),
            "AD" => self.define_aperture(cmd),
            "AB" => {
                if cmd.len() > 2 {
                    if let Ok(code) = cmd[3..].parse::<u32>() {
                        self.block_stack.push((code, self.out.features.len()));
                    }
                } else if let Some((code, start)) = self.block_stack.pop() {
                    let items: Vec<Feature> = self.out.features.drain(start..).collect();
                    let id = self.out.apertures.len() as u32;
                    let mut b = BBox::EMPTY;
                    items.iter().for_each(|f| b.union(&f.bbox));
                    self.add_aperture(Aperture {
                        code,
                        shape: ApShape::Block,
                        w: b.w(),
                        h: b.h(),
                        hole: 0.0,
                        func: 0,
                        parts: Arc::new(Vec::new()),
                        used: 0,
                    });
                    self.blocks.insert(id, items);
                }
            }
            "LP" => self.dark = &cmd[2..] != "C",
            "LM" => {
                let v = &cmd[2..];
                self.xf.mirror_x = v.contains('X');
                self.xf.mirror_y = v.contains('Y');
            }
            "LR" => self.xf.rot = number(&cmd[2..]).unwrap_or(0.0),
            "LS" => self.xf.scale = number(&cmd[2..]).unwrap_or(1.0),
            "SR" => {
                self.close_step();
                if cmd.len() > 2 {
                    let field = |letter: char| {
                        let at = cmd.find(letter)?;
                        let rest = &cmd[at + 1..];
                        let end = rest.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(rest.len());
                        number(&rest[..end])
                    };
                    let (nx, ny) = (field('X').unwrap_or(1.0), field('Y').unwrap_or(1.0));
                    let (dx, dy) = (field('I').unwrap_or(0.0), field('J').unwrap_or(0.0));
                    // A panel of tens of thousands of copies would be a broken file.
                    if nx * ny > 100_000.0 {
                        self.warn("A panel asked for too many copies, so it wasn't repeated.".into());
                    } else if nx > 1.0 || ny > 1.0 {
                        self.step = Some(Stepper {
                            start: self.out.features.len(),
                            nx: nx as u32,
                            ny: ny as u32,
                            dx: dx * self.unit,
                            dy: dy * self.unit,
                        });
                    }
                }
            }
            "TF" | "TA" | "TO" => self.attribute(cmd),
            "TD" => self.delete_attribute(cmd.get(2..).unwrap_or("")),
            "IP" => self.out.negative = cmd.ends_with("NEG"),
            "IN" | "LN" => {}
            "AS" => self.axis_swap = cmd.ends_with("AYBX"),
            "IR" => self.img_rot = number(&cmd[2..]).unwrap_or(0.0),
            "MI" => {
                let on = |letter: char| {
                    cmd.find(letter).is_some_and(|at| cmd[at + 1..].starts_with('1'))
                };
                self.img_mirror = (on('A'), on('B'));
            }
            "OF" | "SF" => {
                let field = |letter: char| {
                    let at = cmd.find(letter)?;
                    let rest = &cmd[at + 1..];
                    let end = rest.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(rest.len());
                    number(&rest[..end])
                };
                if head == "OF" {
                    self.img_offset = (field('A').unwrap_or(0.0), field('B').unwrap_or(0.0));
                } else {
                    self.img_scale = (field('A').unwrap_or(1.0), field('B').unwrap_or(1.0));
                }
            }
            _ => self.word(cmd),
        }
    }

    fn block(&mut self, block: &str) {
        let trimmed = block.trim_start();
        if trimmed.starts_with("AM") {
            let mut words = block.split('*');
            let name: String =
                words.next().unwrap_or("").chars().filter(|c| !c.is_whitespace()).collect();
            let body: Vec<String> = words
                .map(|w| w.trim().to_string())
                .filter(|w| !w.is_empty())
                .collect();
            self.macros.insert(name[2..].to_string(), body);
            return;
        }
        for cmd in block.split('*') {
            let cmd: String = cmd.chars().filter(|c| *c != '\n' && *c != '\r').collect();
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                self.extended(cmd);
            }
        }
    }

    fn comment(&mut self, word: &str) {
        // Some programs write attributes as comments.
        let text = word.trim_start();
        if let Some(rest) = text.strip_prefix("G04 #@! ").or_else(|| text.strip_prefix("G04#@!")) {
            let rest = rest.trim();
            if rest.starts_with("TF") || rest.starts_with("TA") || rest.starts_with("TO") {
                self.attribute(rest);
            } else if rest.starts_with("TD") {
                self.delete_attribute(rest[2..].trim());
            }
        }
    }

    fn run(&mut self, text: &str) {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'%' => {
                    let start = i + 1;
                    let Some(len) = text[start..].find('%') else { break };
                    self.block(&text[start..start + len]);
                    i = start + len + 1;
                }
                b' ' | b'\n' | b'\r' | b'\t' => i += 1,
                _ => {
                    let Some(len) = text[i..].find('*') else { break };
                    let word = &text[i..i + len];
                    let squeezed = word.trim_start();
                    if squeezed.starts_with("G04") {
                        self.comment(word);
                    } else if squeezed.starts_with("M02") || squeezed.starts_with("M00") {
                        break;
                    } else {
                        self.word(word);
                    }
                    i += len + 1;
                }
            }
        }
    }
}

/// Whether some text looks like a Gerber file.
pub fn looks_like_gerber(text: &str) -> bool {
    let head = &text[..text.floor_char_boundary(20_000)];
    head.contains("%FS") || head.contains("%MO") || (head.contains("%AD") && head.contains('*'))
}

pub fn parse(text: &str) -> Parsed {
    let mut r = Reader::new();
    r.run(text);
    if r.region {
        r.close_contour();
        r.region = false;
    }
    r.close_step();
    if !r.have_format {
        r.warn("The file doesn't say its number format; a common one was assumed.".into());
    }
    if !r.have_units {
        r.out.units = "inch";
        r.warn("The file doesn't say its units; inches were assumed.".into());
    }
    let mut out = r.out;
    if out.negative {
        // The file describes what's clear: paint the whole area first, then cut.
        let mut bbox = BBox::EMPTY;
        out.features.iter().for_each(|f| bbox.union(&f.bbox));
        if !bbox.is_empty() {
            let area = bbox.grown(1.0);
            let ring = vec![area.min, p(area.max.x, area.min.y), area.max, p(area.min.x, area.max.y)];
            let sheet = Feature {
                kind: Kind::Region,
                dark: true,
                bbox: area,
                path: ring,
                ap: NONE,
                width: 0.0,
                arc: None,
                net: 0,
                comp: 0,
                pin: 0,
                func: 0,
            };
            out.features.iter_mut().for_each(|f| f.dark = !f.dark);
            out.features.insert(0, sheet);
        }
    }
    out.finish();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "%FSLAX26Y26*%\n%MOMM*%\n";

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn pad_position_and_size() {
        let g = parse(&format!("{HEAD}%ADD10R,1.5X0.8*%\nD10*\nX10000000Y-5000000D03*\nM02*\n"));
        assert_eq!(g.features.len(), 1);
        let f = &g.features[0];
        assert_eq!(f.kind, Kind::Pad);
        assert!(close(f.path[0].x, 10.0) && close(f.path[0].y, -5.0));
        assert!(close(f.bbox.w(), 1.5) && close(f.bbox.h(), 0.8));
    }

    #[test]
    fn inches_become_millimetres() {
        let g = parse("%FSLAX25Y25*%\n%MOIN*%\n%ADD10C,0.01*%\nD10*\nX100000Y0D03*\nM02*\n");
        assert!(close(g.features[0].path[0].x, 25.4));
        assert!(close(g.apertures[0].w, 0.254));
    }

    #[test]
    fn modal_coordinates_and_track_length() {
        let g = parse(&format!(
            "{HEAD}%ADD11C,0.25*%\nD11*\nG01*\nX0Y0D02*\nX10000000D01*\nY5000000D01*\nM02*\n"
        ));
        assert_eq!(g.features.len(), 2);
        assert!(close(g.features[0].length(), 10.0));
        assert!(close(g.features[1].length(), 5.0));
        assert!(close(g.features[1].path[1].x, 10.0));
    }

    #[test]
    fn full_circle_arc_is_a_full_length() {
        let g = parse(&format!(
            "{HEAD}%ADD11C,0.1*%\nD11*\nG75*\nX5000000Y0D02*\nG03*\nX5000000Y0I-5000000J0D01*\nM02*\n"
        ));
        assert_eq!(g.features[0].kind, Kind::Arc);
        assert!((g.features[0].length() - 2.0 * PI * 5.0).abs() < 0.01);
    }

    #[test]
    fn quarter_arc_single_quadrant() {
        let g = parse(&format!(
            "{HEAD}%ADD11C,0.1*%\nD11*\nG74*\nX5000000Y0D02*\nG03*\nX0Y5000000I5000000J0D01*\nM02*\n"
        ));
        let arc = g.features[0].arc.unwrap();
        assert!(close(arc.centre.x, 0.0) && close(arc.centre.y, 0.0));
        assert!(close(arc.sweep_deg, 90.0));
    }

    #[test]
    fn region_and_clear_polarity_cut_out() {
        let g = parse(&format!(
            "{HEAD}G36*\nX0Y0D02*\nG01*\nX10000000D01*\nY10000000D01*\nX0D01*\nY0D01*\nG37*\n\
             %ADD10C,2*%\n%LPC*%\nD10*\nX5000000Y5000000D03*\nM02*\n"
        ));
        assert_eq!(g.features.len(), 2);
        assert_eq!(g.features[0].kind, Kind::Region);
        assert!(!g.features[1].dark);
    }

    #[test]
    fn rounded_macro_with_expressions() {
        let g = parse(&format!(
            "{HEAD}%AMRR*\n0 corner rounded rectangle*\n$4=$1x2*\n21,1,$4,$2,0,0,0*\n1,1,$2,$4/2,0*\n%\n\
             %ADD10RR,1X0.5*%\nD10*\nX0Y0D03*\nM02*\n"
        ));
        let ap = &g.apertures[0];
        assert_eq!(ap.parts.len(), 2);
        assert!(close(ap.w, 2.25), "width {}", ap.w);
        assert!(close(ap.h, 0.5));
        assert_eq!(ap.describe(), "Custom");
    }

    #[test]
    fn x2_attributes_reach_the_feature() {
        let g = parse(&format!(
            "%TF.FileFunction,Copper,L1,Top*%\n{HEAD}%TA.AperFunction,SMDPad,CuDef*%\n%ADD10R,1X1*%\n%TD*%\n\
             %TO.N,GND*%\n%TO.P,R1,2*%\nD10*\nX0Y0D03*\n%TD*%\nX1000000Y0D03*\nM02*\n"
        ));
        assert_eq!(g.file_function.as_deref(), Some("Copper,L1,Top"));
        let f = &g.features[0];
        assert_eq!(g.text(f.net), Some("GND"));
        assert_eq!(g.text(f.comp), Some("R1"));
        assert_eq!(g.text(f.pin), Some("2"));
        assert_eq!(g.text(f.func), Some("SMDPad, CuDef"));
        assert_eq!(g.text(g.features[1].net), None);
    }

    #[test]
    fn step_and_repeat_copies_the_panel() {
        let g = parse(&format!(
            "{HEAD}%ADD10C,1*%\n%SRX3Y2I10J20*%\nD10*\nX0Y0D03*\n%SR*%\nM02*\n"
        ));
        assert_eq!(g.features.len(), 6);
        assert!(close(g.bbox.max.x, 20.5) && close(g.bbox.max.y, 20.5));
    }

    #[test]
    fn block_aperture_flashes_its_contents() {
        let g = parse(&format!(
            "{HEAD}%ADD10C,1*%\n%ABD12*%\nD10*\nX0Y0D03*\nX2000000Y0D03*\n%AB*%\nD12*\nX10000000Y0D03*\nM02*\n"
        ));
        assert_eq!(g.features.len(), 2);
        assert!(close(g.features[1].path[0].x, 12.0));
    }

    #[test]
    fn hit_test_finds_a_track_and_misses_beside_it() {
        let g = parse(&format!("{HEAD}%ADD11C,1*%\nD11*\nX0Y0D02*\nX10000000Y0D01*\nM02*\n"));
        let f = &g.features[0];
        assert_eq!(f.hit(p(5.0, 0.4), 0.0, &g.apertures), Hit::On);
        assert_eq!(f.hit(p(5.0, 0.7), 0.5, &g.apertures), Hit::Near);
        assert_eq!(f.hit(p(5.0, 2.0), 0.5, &g.apertures), Hit::Miss);
    }

    #[test]
    fn altium_style_file_reads_without_warnings() {
        let text = "G04 Layer_Physical_Order=1*\nG04 Layer_Color=255*\n%FSLAX25Y25*%\n%MOIN*%\n\
                    %TF.GenerationSoftware,Altium Limited,Altium Designer,23.0*%\n\
                    G70*\nG01*\nG75*\n%OFA0B0*%\n%ADD10R,0.05X0.03*%\nG54D10*\nX100000Y100000D03*\nM02*\n";
        let g = parse(text);
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
        assert_eq!(g.features.len(), 1);
        assert!(close(g.features[0].path[0].x, 25.4));
        assert!(g.software.unwrap().contains("Altium"));
    }

    #[test]
    fn nonsense_never_stops_the_program() {
        let mut seed = 12345u64;
        let bytes = b"%*GXYDIJ0123456789.,-ADMOFSLAX26TF.\n\xc3\xa9$CROP";
        for round in 0..400 {
            let mut text = Vec::new();
            for _ in 0..600 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                text.push(bytes[(seed >> 33) as usize % bytes.len()]);
            }
            let body = String::from_utf8_lossy(&text);
            let prefixed = if round % 2 == 0 { format!("{HEAD}%ADD10C,1*%\nD10*\n{body}") } else { body.to_string() };
            let parsed = parse(&prefixed);
            let mut mesh = Mesh::default();
            parsed.features.iter().for_each(|f| f.tessellate(&parsed.apertures, &mut mesh));
            crate::excellon::parse(&prefixed);
            let _ = crate::excellon::looks_like_drill(&prefixed);
        }
    }

    #[test]
    fn negative_file_is_inverted() {
        let g = parse(&format!("%IPNEG*%\n{HEAD}%ADD10C,2*%\nD10*\nX5000000Y5000000D03*\nM02*\n"));
        assert_eq!(g.features.len(), 2);
        assert!(g.features[0].dark && !g.features[1].dark);
    }
}
