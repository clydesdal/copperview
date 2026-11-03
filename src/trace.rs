//! Follows copper from one piece to everything it touches, and optionally on
//! through plated holes into the other layers.

use crate::board::{Board, Layer};
use crate::geom::*;
use crate::gerber::{Feature, Kind};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub struct TraceLayer {
    pub layer: usize,
    pub features: Vec<usize>,
    /// Length of the tracks and arcs on this layer.
    pub length: f64,
    pub tracks: usize,
    pub pads: usize,
    pub regions: usize,
}

#[derive(Clone, Debug)]
pub struct Trace {
    pub layers: Vec<TraceLayer>,
    /// The plated holes it passes through: layer and feature.
    pub holes: Vec<(usize, usize)>,
    /// Followed through the holes into other layers.
    pub through: bool,
    /// The other half of a differential pair, when this runs alongside another trace.
    pub partner: Option<Box<Trace>>,
    /// How much of the two run side by side.
    pub coupled: f64,
}

impl Trace {
    pub fn length(&self) -> f64 {
        self.layers.iter().map(|l| l.length).sum()
    }

    /// Whether the piece is in this trace or its pair partner.
    pub fn contains(&self, layer: usize, feature: usize) -> bool {
        self.has(layer, feature) || self.partner.as_ref().is_some_and(|p| p.has(layer, feature))
    }

    pub fn has(&self, layer: usize, feature: usize) -> bool {
        self.layers.iter().any(|l| l.layer == layer && l.features.contains(&feature))
    }

    pub fn sum(&self, count: impl Fn(&TraceLayer) -> usize) -> usize {
        self.layers.iter().map(count).sum()
    }
}

/// One layer's shapes, ready for asking what touches what.
struct Geo<'a> {
    layer: &'a Layer,
    rings: Vec<Vec<Vec<P>>>,
    /// Clear-polarity shapes cut holes in the copper drawn before them: the
    /// gaps around a pour's neighbours are usually drawn this way.
    cutters: Vec<(usize, BBox)>,
    /// Shapes closer than this count as touching.
    gap: f64,
}

fn is_copper(f: &Feature) -> bool {
    f.dark && !matches!(f.kind, Kind::Hole | Kind::Slot)
}

impl<'a> Geo<'a> {
    fn new(layer: &'a Layer) -> Self {
        let feats = &layer.data.features;
        let rings = feats.iter().map(|f| f.outline(&layer.data.apertures)).collect();
        let cutters = feats.iter().enumerate().filter(|(_, f)| !f.dark).map(|(i, f)| (i, f.bbox)).collect();
        Geo { layer, rings, cutters, gap: 1e-3 }
    }

    fn feats(&self) -> &[Feature] {
        &self.layer.data.features
    }

    fn cleared(&self, q: P, after: usize) -> bool {
        self.cutters.iter().any(|(k, b)| *k > after && b.contains(q) && self.rings[*k].iter().any(|r| point_in_ring(q, r)))
    }

    /// Points that stand for a shape: its outline corners, its centre line and its middle.
    fn samples(&self, i: usize) -> Vec<P> {
        let mut pts: Vec<P> = self.rings[i].iter().flat_map(|r| r.iter().copied()).collect();
        let f = &self.feats()[i];
        if f.kind != Kind::Region {
            pts.extend(f.path.iter().copied());
            pts.extend(f.path.windows(2).map(|w| w[0].add(w[1]).scale(0.5)));
        }
        pts
    }

    fn in_pour(&self, pour: usize, other: usize) -> bool {
        let feats = self.feats();
        self.samples(other).into_iter().any(|q| {
            feats[pour].bbox.contains(q) && self.rings[pour].iter().any(|r| point_in_ring(q, r)) && !self.cleared(q, pour)
        })
    }

    fn touching(&self, i: usize, j: usize) -> bool {
        let feats = self.feats();
        let (a, b) = (&feats[i].bbox, &feats[j].bbox);
        let g = self.gap;
        if a.min.x > b.max.x + g || a.max.x < b.min.x - g || a.min.y > b.max.y + g || a.max.y < b.min.y - g {
            return false;
        }
        match (feats[i].kind == Kind::Region, feats[j].kind == Kind::Region) {
            (true, true) => self.in_pour(i, j) || self.in_pour(j, i),
            (true, false) => self.in_pour(i, j),
            (false, true) => self.in_pour(j, i),
            _ => self.rings[i].iter().any(|x| self.rings[j].iter().any(|y| ring_gap(x, y) < self.gap)),
        }
    }

    /// The copper under a point, such as the middle of a hole.
    fn covering(&self, q: P) -> Vec<usize> {
        self.layer
            .grid
            .near(q, 0.0)
            .into_iter()
            .map(|i| i as usize)
            .filter(|i| {
                let f = &self.feats()[*i];
                is_copper(f)
                    && f.bbox.contains(q)
                    && self.rings[*i].iter().any(|r| point_in_ring(q, r))
                    && (f.kind != Kind::Region || !self.cleared(q, *i))
            })
            .collect()
    }

    /// Adds to `seen` everything that touches the seeds, directly or through others.
    fn flood(&self, seen: &mut HashSet<usize>, seeds: &[usize]) {
        let n = self.feats().len();
        let mut queue: Vec<usize> = Vec::new();
        for s in seeds {
            if seen.insert(*s) {
                queue.push(*s);
            }
        }
        while let Some(i) = queue.pop() {
            for j in 0..n {
                if !seen.contains(&j) && is_copper(&self.feats()[j]) && self.touching(i, j) {
                    seen.insert(j);
                    queue.push(j);
                }
            }
        }
    }
}

/// Follows connected strokes on a layer that isn't copper, such as silkscreen
/// or fab notes, building the layer's shapes once for many questions.
pub struct Linker<'a>(Geo<'a>);

impl<'a> Linker<'a> {
    /// Strokes closer than `gap` mm count as joined, so the letters of a word hang together.
    pub fn new(layer: &'a Layer, gap: f64) -> Self {
        let mut geo = Geo::new(layer);
        geo.gap = gap;
        Linker(geo)
    }

    /// Everything on the layer that touches this piece, directly or through others.
    pub fn connected(&self, feature: usize) -> Vec<usize> {
        let mut seen = HashSet::new();
        self.0.flood(&mut seen, &[feature]);
        seen.into_iter().collect()
    }
}

/// Everything on the layer that touches this piece of copper, directly or through others.
pub fn connected(board: &Board, layer: usize, feature: usize) -> Option<Vec<usize>> {
    let t = follow_one(board, layer, feature, false)?;
    t.layers.into_iter().find(|l| l.layer == layer).map(|l| l.features)
}

/// Follows the copper and, when another trace runs alongside it for much of
/// its length, finds that one too: the other half of a differential pair.
pub fn follow(board: &Board, layer: usize, feature: usize, through: bool) -> Option<Trace> {
    let mut trace = follow_one(board, layer, feature, through)?;
    if trace.layers.iter().any(|l| l.regions > 0) || trace.length() <= 0.0 {
        return Some(trace);
    }
    // Tracks of this trace, each against its neighbours: how long they run side by side.
    let mut alongside: HashMap<(usize, usize), f64> = HashMap::new();
    for tl in &trace.layers {
        let l = &board.layers[tl.layer];
        for fi in &tl.features {
            let f = &l.data.features[*fi];
            if !matches!(f.kind, Kind::Track | Kind::Arc) {
                continue;
            }
            for seg in f.path.windows(2) {
                let (a0, a1) = (seg[0], seg[1]);
                let len = a0.dist(a1);
                if len < 1e-6 {
                    continue;
                }
                let reach = len / 2.0 + 1.0 + 3.0 * f.width;
                for ci in l.grid.near(a0.add(a1).scale(0.5), reach) {
                    let ci = ci as usize;
                    let c = &l.data.features[ci];
                    if !c.dark || !matches!(c.kind, Kind::Track | Kind::Arc) || tl.features.contains(&ci) {
                        continue;
                    }
                    for cs in c.path.windows(2) {
                        let along = side_by_side(a0, a1, f.width, cs[0], cs[1], c.width);
                        if along > 0.0 {
                            *alongside.entry((tl.layer, ci)).or_default() += along;
                        }
                    }
                }
            }
        }
    }
    // Try the likeliest neighbours, best first, until one passes the strict test.
    let mut ranked: Vec<((usize, usize), f64)> = alongside.iter().map(|(k, v)| (*k, *v)).collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut tried: Vec<Trace> = Vec::new();
    for ((bl, bf), _) in ranked.into_iter().take(12) {
        if tried.len() >= 3 || tried.iter().any(|t| t.has(bl, bf)) {
            continue;
        }
        let Some(partner) = follow_one(board, bl, bf, through) else { continue };
        let disjoint = !partner.layers.iter().any(|l| l.features.iter().any(|f| trace.has(l.layer, *f)));
        if disjoint
            && partner.layers.iter().all(|l| l.regions == 0)
            && let Some(coupled) = is_pair(board, &trace, &partner)
        {
            trace.coupled = coupled;
            trace.partner = Some(Box::new(partner));
            break;
        }
        tried.push(partner);
    }
    Some(trace)
}

/// Layers where a trace is only a short hop, such as to a via, aren't judged.
const SHORT_RUN: f64 = 5.0;
/// Near a pad or via the two traces may fan out, so those stretches aren't judged.
const FAN_OUT: f64 = 3.0;
/// Widths must match this closely.
const WIDTH_TOL: f64 = 0.01;
/// The spacing must hold over this much of the run.
const KEPT: f64 = 0.95;
const STEP: f64 = 0.1;
const MAX_GAP: f64 = 1.0;

struct Run {
    a: P,
    b: P,
    w: f64,
}

fn runs(layer: &Layer, features: &[usize]) -> Vec<Run> {
    let mut out = Vec::new();
    for fi in features {
        let f = &layer.data.features[*fi];
        if matches!(f.kind, Kind::Track | Kind::Arc) {
            out.extend(f.path.windows(2).map(|w| Run { a: w[0], b: w[1], w: f.width }));
        }
    }
    out
}

/// For points along one trace: the gap to the nearest copper of the other, when the widths match.
fn gaps(from: &[Run], to: &[Run]) -> Vec<(P, Option<f64>)> {
    let mut out = Vec::new();
    for r in from {
        let len = r.a.dist(r.b);
        let n = ((len / STEP).ceil() as usize).max(1);
        for i in 0..n {
            let at = r.a.add(r.b.sub(r.a).scale((i as f64 + 0.5) / n as f64));
            let near = to
                .iter()
                .map(|c| (dist_to_segment(at, c.a, c.b), c.w))
                .min_by(|x, y| x.0.total_cmp(&y.0));
            let gap = near.filter(|(_, w)| (w - r.w).abs() <= WIDTH_TOL).map(|(d, w)| d - (r.w + w) / 2.0);
            out.push((at, gap));
        }
    }
    out
}

/// Whether two traces are a differential pair: the same width, with the same
/// spacing kept for nearly all of the run on each layer they share. Returns how
/// much of the run was side by side.
fn is_pair(board: &Board, a: &Trace, b: &Trace) -> Option<f64> {
    // Plated holes: the traces fan out around them.
    let holes: Vec<P> = board
        .layers
        .iter()
        .filter(|l| !l.role.copper && l.data.plated != Some(false))
        .flat_map(|l| l.data.features.iter().filter(|f| f.kind == Kind::Hole).map(|f| f.path[0]))
        .collect();
    let mut side_by_side = 0.0;
    let mut judged = 0.0;
    let mut layers: Vec<usize> = a.layers.iter().chain(b.layers.iter()).map(|l| l.layer).collect();
    layers.sort_unstable();
    layers.dedup();
    for li in layers {
        let layer = &board.layers[li];
        let (ta, tb) = (a.layers.iter().find(|l| l.layer == li), b.layers.iter().find(|l| l.layer == li));
        let (la, lb) = (ta.map_or(0.0, |l| l.length), tb.map_or(0.0, |l| l.length));
        if la.max(lb) < SHORT_RUN || la.min(lb) < SHORT_RUN && la.min(lb) > 0.0 {
            continue;
        }
        if la.min(lb) <= 0.0 {
            // One runs a long way on a layer the other never uses.
            return None;
        }
        let (ra, rb) = (runs(layer, &ta?.features), runs(layer, &tb?.features));
        let ends: Vec<P> = [ta?, tb?]
            .iter()
            .flat_map(|t| t.features.iter())
            .filter(|f| layer.data.features[**f].kind == Kind::Pad)
            .map(|f| layer.data.features[*f].path[0])
            .chain(holes.iter().copied())
            .collect();
        let near_end = |q: P| ends.iter().any(|e| e.dist(q) < FAN_OUT);
        let (ga, gb) = (gaps(&ra, &rb), gaps(&rb, &ra));
        // The usual spacing: the commonest gap, in 0.01 mm steps.
        let mut counts: HashMap<i64, usize> = HashMap::new();
        for (_, g) in ga.iter().chain(gb.iter()) {
            if let Some(g) = g.filter(|g| (-0.01..=MAX_GAP).contains(g)) {
                *counts.entry((g * 100.0).round() as i64).or_default() += 1;
            }
        }
        let Some((&bin, _)) = counts.iter().max_by_key(|(_, n)| **n) else { return None };
        let usual = bin as f64 / 100.0;
        let tol = (0.03f64).max(0.15 * usual);
        let kept = |g: &Option<f64>| g.is_some_and(|g| (g - usual).abs() <= tol);
        for samples in [&ga, &gb] {
            let judged_here: Vec<&(P, Option<f64>)> = samples.iter().filter(|(q, _)| !near_end(*q)).collect();
            let good = judged_here.iter().filter(|(_, g)| kept(g)).count();
            if !judged_here.is_empty() && (good as f64) < KEPT * judged_here.len() as f64 {
                return None;
            }
            judged += judged_here.len() as f64 * STEP;
        }
        side_by_side += ga.iter().filter(|(_, g)| kept(g)).count() as f64 * STEP;
    }
    // Enough run was actually judged, not just exempted.
    (judged >= 2.0 && side_by_side >= 1.0).then_some(side_by_side)
}

/// How far a neighbouring segment runs parallel to this one and close beside it.
fn side_by_side(a0: P, a1: P, wa: f64, c0: P, c1: P, wc: f64) -> f64 {
    let ab = a1.sub(a0);
    let len = ab.len();
    let dir = ab.scale(1.0 / len);
    let cd = c1.sub(c0);
    let clen = cd.len();
    if clen < 1e-6 || (dir.x * cd.y - dir.y * cd.x).abs() / clen > 0.17 || (dir.x * cd.x + dir.y * cd.y).abs() / clen < 0.9 {
        return 0.0;
    }
    let (t0, t1) = (c0.sub(a0), c1.sub(a0));
    if (wa - wc).abs() > WIDTH_TOL {
        return 0.0;
    }
    let (u0, u1) = (t0.x * dir.x + t0.y * dir.y, t1.x * dir.x + t1.y * dir.y);
    let (v0, v1) = ((t0.x * dir.y - t0.y * dir.x).abs(), (t1.x * dir.y - t1.y * dir.x).abs());
    let gap = (v0 + v1) / 2.0 - (wa + wc) / 2.0;
    if gap < -0.01 || gap > (3.0 * wa).max(0.6) {
        return 0.0;
    }
    (u0.max(u1).min(len) - u0.min(u1).max(0.0)).max(0.0)
}

fn follow_one(board: &Board, layer: usize, feature: usize, through: bool) -> Option<Trace> {
    let start_layer = board.layers.get(layer)?;
    if !start_layer.role.copper || !is_copper(start_layer.data.features.get(feature)?) {
        return None;
    }
    let mut geos: HashMap<usize, Geo> = HashMap::new();
    geos.insert(layer, Geo::new(start_layer));
    let mut sets: BTreeMap<usize, HashSet<usize>> = BTreeMap::new();
    let mut first = HashSet::new();
    geos[&layer].flood(&mut first, &[feature]);
    sets.insert(layer, first);

    let mut holes: Vec<(usize, usize)> = Vec::new();
    if through {
        // Every plated hole on the board, with its centre.
        let all: Vec<(usize, usize, P)> = board
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| !l.role.copper && l.data.plated != Some(false))
            .flat_map(|(li, l)| {
                l.data.features.iter().enumerate().filter(|(_, f)| f.kind == Kind::Hole && f.dark).map(move |(fi, f)| (li, fi, f.path[0]))
            })
            .collect();
        let coppers: Vec<usize> = board.layers.iter().enumerate().filter(|(_, l)| l.role.copper).map(|(i, _)| i).collect();
        let mut used = vec![false; all.len()];
        loop {
            let mut changed = false;
            for (n, (dl, df, q)) in all.iter().enumerate() {
                if used[n] {
                    continue;
                }
                let mut hit = false;
                for (l, set) in &sets {
                    if geos.get(l).is_some_and(|g| g.covering(*q).iter().any(|i| set.contains(i))) {
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    continue;
                }
                used[n] = true;
                holes.push((*dl, *df));
                for l in &coppers {
                    let geo = geos.entry(*l).or_insert_with(|| Geo::new(&board.layers[*l]));
                    let set = sets.entry(*l).or_default();
                    let fresh: Vec<usize> = geo.covering(*q).into_iter().filter(|i| !set.contains(i)).collect();
                    if !fresh.is_empty() {
                        geo.flood(set, &fresh);
                    }
                }
                changed = true;
            }
            if !changed {
                break;
            }
        }
    }

    let mut layers = Vec::new();
    for (l, set) in sets {
        if set.is_empty() {
            continue;
        }
        let feats = &board.layers[l].data.features;
        let mut tl = TraceLayer { layer: l, ..Default::default() };
        let mut found: Vec<usize> = set.into_iter().collect();
        found.sort_unstable();
        for i in &found {
            let f = &feats[*i];
            match f.kind {
                Kind::Track | Kind::Arc => {
                    tl.tracks += 1;
                    tl.length += f.length();
                }
                Kind::Pad => tl.pads += 1,
                _ => tl.regions += 1,
            }
        }
        tl.features = found;
        layers.push(tl);
    }
    Some(Trace { layers, holes, through, partner: None, coupled: 0.0 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::from_memory;

    /// A top copper layer made of straight tracks: (x1, y1, x2, y2, width) in mm.
    fn board(tracks: &[(f64, f64, f64, f64, f64)]) -> Board {
        let mut widths: Vec<f64> = Vec::new();
        for t in tracks {
            if !widths.contains(&t.4) {
                widths.push(t.4);
            }
        }
        let mut s = String::from("%FSLAX46Y46*%\n%MOMM*%\n");
        for (i, w) in widths.iter().enumerate() {
            s.push_str(&format!("%ADD{}C,{w}*%\n", 10 + i));
        }
        let m = |v: f64| (v * 1e6).round() as i64;
        for t in tracks {
            let d = 10 + widths.iter().position(|w| *w == t.4).unwrap();
            s.push_str(&format!("D{d}*\nX{}Y{}D02*\nX{}Y{}D01*\n", m(t.0), m(t.1), m(t.2), m(t.3)));
        }
        s.push_str("M02*\n");
        from_memory("t", vec![("t.GTL".to_string(), s)])
    }

    fn follow_first(b: &Board) -> Trace {
        let layer = b.layers.iter().position(|l| l.role.copper).expect("a copper layer");
        follow(b, layer, 0, false).expect("a trace")
    }

    #[test]
    fn two_matched_tracks_are_a_pair() {
        let b = board(&[(0.0, 0.0, 20.0, 0.0, 0.15), (0.0, 0.35, 20.0, 0.35, 0.15)]);
        let t = follow_first(&b);
        let partner = t.partner.expect("a partner");
        assert!((partner.length() - 20.0).abs() < 1e-3);
        assert!(t.coupled > 19.0, "{}", t.coupled);
    }

    #[test]
    fn different_widths_are_not_a_pair() {
        let b = board(&[(0.0, 0.0, 20.0, 0.0, 0.15), (0.0, 0.35, 20.0, 0.35, 0.25)]);
        assert!(follow_first(&b).partner.is_none());
    }

    #[test]
    fn spacing_that_is_lost_is_not_a_pair() {
        // The second track wanders off after 10 mm, so only half the run keeps its spacing.
        let b = board(&[
            (0.0, 0.0, 20.0, 0.0, 0.15),
            (0.0, 0.35, 10.0, 0.35, 0.15),
            (10.0, 0.35, 12.0, 2.0, 0.15),
            (12.0, 2.0, 20.0, 2.0, 0.15),
        ]);
        assert!(follow_first(&b).partner.is_none());
    }

    #[test]
    fn a_lone_track_has_no_partner() {
        let b = board(&[(0.0, 0.0, 20.0, 0.0, 0.15)]);
        assert!(follow_first(&b).partner.is_none());
    }
}
