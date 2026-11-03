//! Points, boxes, and turning shapes into triangles for the graphics card.
//! Every length in the program is in millimetres.

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct P {
    pub x: f64,
    pub y: f64,
}

pub const fn p(x: f64, y: f64) -> P {
    P { x, y }
}

impl P {
    pub fn add(self, o: P) -> P {
        p(self.x + o.x, self.y + o.y)
    }
    pub fn sub(self, o: P) -> P {
        p(self.x - o.x, self.y - o.y)
    }
    pub fn scale(self, k: f64) -> P {
        p(self.x * k, self.y * k)
    }
    pub fn len(self) -> f64 {
        self.x.hypot(self.y)
    }
    pub fn dist(self, o: P) -> f64 {
        self.sub(o).len()
    }
    pub fn rot(self, deg: f64) -> P {
        if deg == 0.0 {
            return self;
        }
        let (s, c) = deg.to_radians().sin_cos();
        p(self.x * c - self.y * s, self.x * s + self.y * c)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BBox {
    pub min: P,
    pub max: P,
}

impl Default for BBox {
    fn default() -> Self {
        BBox::EMPTY
    }
}

impl BBox {
    pub const EMPTY: BBox = BBox {
        min: p(f64::INFINITY, f64::INFINITY),
        max: p(f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
    pub fn add(&mut self, q: P) {
        self.min.x = self.min.x.min(q.x);
        self.min.y = self.min.y.min(q.y);
        self.max.x = self.max.x.max(q.x);
        self.max.y = self.max.y.max(q.y);
    }
    pub fn union(&mut self, o: &BBox) {
        if !o.is_empty() {
            self.add(o.min);
            self.add(o.max);
        }
    }
    pub fn grown(&self, by: f64) -> BBox {
        BBox { min: p(self.min.x - by, self.min.y - by), max: p(self.max.x + by, self.max.y + by) }
    }
    pub fn overlaps(&self, o: &BBox) -> bool {
        self.min.x <= o.max.x && self.max.x >= o.min.x && self.min.y <= o.max.y && self.max.y >= o.min.y
    }

    pub fn shifted(&self, by: P) -> BBox {
        if self.is_empty() {
            return *self;
        }
        BBox { min: self.min.add(by), max: self.max.add(by) }
    }
    pub fn w(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.x - self.min.x }
    }
    pub fn h(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.max.y - self.min.y }
    }
    pub fn center(&self) -> P {
        p((self.min.x + self.max.x) / 2.0, (self.min.y + self.max.y) / 2.0)
    }
    pub fn contains(&self, q: P) -> bool {
        q.x >= self.min.x && q.x <= self.max.x && q.y >= self.min.y && q.y <= self.max.y
    }
}

/// How finely curves are cut into straight pieces.
const CURVE_ERROR_MM: f64 = 0.001;

pub fn circle_steps(r: f64) -> usize {
    if r <= CURVE_ERROR_MM {
        return 8;
    }
    ((PI / (1.0 - CURVE_ERROR_MM / r).acos()).ceil() as usize).clamp(12, 96)
}

pub fn circle_ring(c: P, d: f64) -> Vec<P> {
    let r = d / 2.0;
    let n = circle_steps(r);
    (0..n)
        .map(|i| {
            let a = i as f64 / n as f64 * 2.0 * PI;
            p(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect()
}

pub fn rect_ring(c: P, w: f64, h: f64) -> Vec<P> {
    let (a, b) = (w / 2.0, h / 2.0);
    vec![p(c.x - a, c.y - b), p(c.x + a, c.y - b), p(c.x + a, c.y + b), p(c.x - a, c.y + b)]
}

/// A stadium: a straight bar with half-round ends, from a to b.
pub fn capsule_ring(a: P, b: P, w: f64) -> Vec<P> {
    let r = w / 2.0;
    let dir = b.sub(a);
    let len = dir.len();
    if len < 1e-9 {
        return circle_ring(a, w);
    }
    let start = dir.y.atan2(dir.x) + PI / 2.0;
    let n = circle_steps(r).div_ceil(2).max(6);
    let mut ring = Vec::with_capacity(2 * n + 2);
    for (centre, offset) in [(a, 0.0), (b, PI)] {
        for i in 0..=n {
            let t = start + offset + PI * i as f64 / n as f64;
            ring.push(p(centre.x + r * t.cos(), centre.y + r * t.sin()));
        }
    }
    ring
}

/// The outline of a curved track: out along one side, round the end, back along the other.
pub fn offset_ring(path: &[P], w: f64) -> Vec<P> {
    let r = w / 2.0;
    let n = path.len();
    if n < 2 {
        return circle_ring(path.first().copied().unwrap_or_default(), w);
    }
    let normal = |i: usize| {
        let d = path[(i + 1).min(n - 1)].sub(path[i.saturating_sub(1)]);
        let l = d.len().max(1e-12);
        p(-d.y / l, d.x / l)
    };
    let mut ring = Vec::with_capacity(2 * n + 24);
    for i in 0..n {
        ring.push(path[i].add(normal(i).scale(r)));
    }
    let cap = |ring: &mut Vec<P>, c: P, from: P| {
        let start = from.y.atan2(from.x);
        for i in 1..12 {
            let t = start - PI * i as f64 / 12.0;
            ring.push(p(c.x + r * t.cos(), c.y + r * t.sin()));
        }
    };
    cap(&mut ring, path[n - 1], normal(n - 1));
    for i in (0..n).rev() {
        ring.push(path[i].sub(normal(i).scale(r)));
    }
    cap(&mut ring, path[0], normal(0).scale(-1.0));
    ring
}

/// The smallest convex shape around some points.
pub fn hull(mut pts: Vec<P>) -> Vec<P> {
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: P, a: P, b: P| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut out: Vec<P> = Vec::with_capacity(pts.len() + 1);
    for &q in &pts {
        while out.len() >= 2 && cross(out[out.len() - 2], out[out.len() - 1], q) <= 0.0 {
            out.pop();
        }
        out.push(q);
    }
    let lower = out.len() + 1;
    for &q in pts.iter().rev().skip(1) {
        while out.len() >= lower && cross(out[out.len() - 2], out[out.len() - 1], q) <= 0.0 {
            out.pop();
        }
        out.push(q);
    }
    out.pop();
    out
}

pub fn point_in_ring(q: P, ring: &[P]) -> bool {
    let mut inside = false;
    let n = ring.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (ring[i], ring[j]);
        if (a.y > q.y) != (b.y > q.y) && q.x < (b.x - a.x) * (q.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn dist_to_segment(q: P, a: P, b: P) -> f64 {
    let ab = b.sub(a);
    let l2 = ab.x * ab.x + ab.y * ab.y;
    if l2 < 1e-18 {
        return q.dist(a);
    }
    let t = (((q.x - a.x) * ab.x + (q.y - a.y) * ab.y) / l2).clamp(0.0, 1.0);
    q.dist(a.add(ab.scale(t)))
}

pub fn dist_to_ring(q: P, ring: &[P]) -> f64 {
    let n = ring.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        best = best.min(dist_to_segment(q, ring[i], ring[(i + 1) % n]));
    }
    best
}

pub fn ring_area(ring: &[P]) -> f64 {
    let n = ring.len();
    let mut sum = 0.0;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        sum += a.x * b.y - b.x * a.y;
    }
    (sum / 2.0).abs()
}

fn segments_cross(a: P, b: P, c: P, d: P) -> bool {
    let cross = |o: P, u: P, v: P| (u.x - o.x) * (v.y - o.y) - (u.y - o.y) * (v.x - o.x);
    let (d1, d2) = (cross(c, d, a), cross(c, d, b));
    let (d3, d4) = (cross(a, b, c), cross(a, b, d));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// The shortest edge-to-edge distance between two shapes: zero when they touch or overlap.
pub fn ring_gap(a: &[P], b: &[P]) -> f64 {
    if a.is_empty() || b.is_empty() {
        return f64::INFINITY;
    }
    if point_in_ring(a[0], b) || point_in_ring(b[0], a) {
        return 0.0;
    }
    let (na, nb) = (a.len(), b.len());
    let mut best = f64::INFINITY;
    for i in 0..na {
        let (a1, a2) = (a[i], a[(i + 1) % na]);
        for j in 0..nb {
            let (b1, b2) = (b[j], b[(j + 1) % nb]);
            if segments_cross(a1, a2, b1, b2) {
                return 0.0;
            }
            best = best
                .min(dist_to_segment(a1, b1, b2))
                .min(dist_to_segment(b1, a1, a2));
        }
    }
    best
}

/// Triangles ready for the graphics card. Each corner holds x, y and whether
/// that spot is copper (1) or cleared (0), and triangles are drawn in order,
/// so a later clear shape cuts through an earlier dark one.
#[derive(Default)]
pub struct Mesh {
    pub verts: Vec<f32>,
    pub idx: Vec<u32>,
}

impl Mesh {
    fn base(&self) -> u32 {
        (self.verts.len() / 3) as u32
    }

    fn vertex(&mut self, q: P, v: f32) {
        self.verts.extend_from_slice(&[q.x as f32, q.y as f32, v]);
    }

    /// A shape with no dents.
    pub fn convex(&mut self, ring: &[P], v: f32) {
        if ring.len() < 3 {
            return;
        }
        let base = self.base();
        for &q in ring {
            self.vertex(q, v);
        }
        for i in 1..ring.len() as u32 - 1 {
            self.idx.extend_from_slice(&[base, base + i, base + i + 1]);
        }
    }

    pub fn soup(&mut self, tris: &Tris, at: P, v: f32) {
        let base = self.base();
        for &q in &tris.pts {
            self.vertex(q.add(at), v);
        }
        self.idx.extend(tris.idx.iter().map(|i| base + i));
    }

    /// A curved track: a strip along the path with round ends.
    pub fn strip(&mut self, path: &[P], w: f64, v: f32) {
        self.soup(&triangulate(std::slice::from_ref(&offset_ring(path, w))), p(0.0, 0.0), v);
    }
}

/// A shape already cut into triangles.
#[derive(Clone, Default)]
pub struct Tris {
    pub pts: Vec<P>,
    pub idx: Vec<u32>,
}

/// Cuts a shape into triangles. The first ring is the outside, the rest are holes.
pub fn triangulate(rings: &[Vec<P>]) -> Tris {
    let mut pts: Vec<P> = Vec::new();
    let mut holes: Vec<u32> = Vec::new();
    for (i, ring) in rings.iter().enumerate() {
        if i > 0 {
            holes.push(pts.len() as u32);
        }
        pts.extend_from_slice(ring);
    }
    let mut idx: Vec<u32> = Vec::new();
    if pts.len() >= 3 {
        let mut cutter = earcut::Earcut::<f64>::new();
        cutter.earcut(pts.iter().map(|q| [q.x, q.y]), &holes, &mut idx);
    }
    Tris { pts, idx }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_between_two_squares() {
        let a = rect_ring(p(0.0, 0.0), 1.0, 1.0);
        let b = rect_ring(p(3.0, 0.0), 1.0, 1.0);
        assert!((ring_gap(&a, &b) - 2.0).abs() < 1e-9);
        let c = rect_ring(p(0.5, 0.0), 1.0, 1.0);
        assert_eq!(ring_gap(&a, &c), 0.0);
    }

    #[test]
    fn triangles_cover_the_whole_area() {
        let t = triangulate(&[rect_ring(p(0.0, 0.0), 2.0, 2.0), rect_ring(p(0.0, 0.0), 1.0, 1.0)]);
        let mut area = 0.0;
        for tri in t.idx.chunks(3) {
            area += ring_area(&[t.pts[tri[0] as usize], t.pts[tri[1] as usize], t.pts[tri[2] as usize]]);
        }
        assert!((area - 3.0).abs() < 1e-9);
    }

    #[test]
    fn capsule_outline_is_as_wide_as_the_track() {
        let ring = capsule_ring(p(0.0, 0.0), p(10.0, 0.0), 2.0);
        let mut b = BBox::EMPTY;
        ring.iter().for_each(|q| b.add(*q));
        assert!((b.h() - 2.0).abs() < 1e-3);
        assert!((b.w() - 12.0).abs() < 1e-3);
    }
}
