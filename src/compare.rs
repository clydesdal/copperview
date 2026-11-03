//! Lines up the layers of two boards and works out what changed between them.

use crate::board::{Board, Layer};
use crate::geom::*;
use crate::gerber::{Feature, Kind};
use crate::trace;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

/// Nearby changes count as one place to look at.
const CLUSTER_MM: f64 = 2.0;
const MAX_ITEMS: usize = 50_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// Byte for byte the same file.
    Identical,
    /// Same drawing; only the file's header, date or ordering differs.
    SameDrawing,
    Changed { removed: usize, added: usize },
    OnlyA,
    OnlyB,
}

impl Status {
    pub fn differs(self) -> bool {
        !matches!(self, Status::Identical | Status::SameDrawing)
    }
}

/// One feature that is only on one side.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Item {
    /// True when it's only in B; false when it's only in A.
    pub added: bool,
    pub layer: usize,
    pub feature: usize,
    /// Where it is, in A's frame.
    pub bbox: BBox,
}

/// Differences that belong together: pieces of one connected trace, or an old
/// and a new piece sitting in the same place. They share one marker.
#[derive(Clone, Debug)]
pub struct Group {
    /// Where the marker's point goes: the bounds of the piece nearest the middle.
    pub at: BBox,
    pub items: Vec<Item>,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub key: String,
    pub label: String,
    pub bbox: BBox,
    pub removed: usize,
    pub added: usize,
    /// The features themselves, so they can be lit up.
    pub items: Vec<Item>,
    /// The markers: connected differences share one.
    pub groups: Vec<Group>,
}

pub struct Comparison {
    pub statuses: HashMap<String, Status>,
    pub changes: Vec<Change>,
    pub truncated: bool,
}

fn grid(v: f64) -> i64 {
    (v * 1000.0).round() as i64
}

fn cell(q: P, shift: P) -> (i64, i64) {
    (grid(q.x + shift.x), grid(q.y + shift.y))
}

/// A fingerprint of what a feature looks like, ignoring pad-shape numbering
/// (which Altium can change on every export).
fn fingerprint(f: &Feature, layer: &Layer, shift: P) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    f.dark.hash(&mut h);
    match f.kind {
        Kind::Region => {
            0u8.hash(&mut h);
            let mut pts: Vec<(i64, i64)> = f.path.iter().map(|q| cell(*q, shift)).collect();
            pts.sort_unstable();
            pts.dedup();
            pts.hash(&mut h);
        }
        Kind::Pad | Kind::Hole => {
            1u8.hash(&mut h);
            cell(f.path[0], shift).hash(&mut h);
            grid(f.width).hash(&mut h);
            if let Some(ap) = layer.data.apertures.get(f.ap as usize) {
                grid(ap.w).hash(&mut h);
                grid(ap.h).hash(&mut h);
                ap.parts.iter().map(|part| part.rings.iter().map(Vec::len).sum::<usize>()).sum::<usize>().hash(&mut h);
            }
        }
        Kind::Track | Kind::Arc | Kind::Slot => {
            2u8.hash(&mut h);
            grid(f.width).hash(&mut h);
            let mut ends = [cell(f.path[0], shift), cell(f.path[f.path.len() - 1], shift)];
            ends.sort_unstable();
            ends.hash(&mut h);
            grid(f.length()).hash(&mut h);
        }
    }
    h.finish()
}

fn layer_changes(a: &Layer, b: &Layer, shift: P) -> (Vec<(BBox, usize)>, Vec<(BBox, usize)>) {
    let mut counts: HashMap<u64, Vec<usize>> = HashMap::with_capacity(a.data.features.len());
    for (i, f) in a.data.features.iter().enumerate() {
        counts.entry(fingerprint(f, a, P::default())).or_default().push(i);
    }
    let mut added = Vec::new();
    for (bi, f) in b.data.features.iter().enumerate() {
        let print = fingerprint(f, b, shift);
        match counts.get_mut(&print) {
            Some(list) if !list.is_empty() => {
                list.pop();
            }
            _ => added.push((f.bbox.shifted(shift), bi)),
        }
    }
    let mut removed: Vec<(BBox, usize)> = Vec::new();
    for list in counts.values() {
        removed.extend(list.iter().map(|i| (a.data.features[*i].bbox, *i)));
    }
    (removed, added)
}

fn near(a: &BBox, b: &BBox) -> bool {
    let g = a.grown(CLUSTER_MM);
    g.min.x <= b.max.x && g.max.x >= b.min.x && g.min.y <= b.max.y && g.max.y >= b.min.y
}

fn cluster(key: &str, label: &str, layers: (usize, usize), removed: Vec<(BBox, usize)>, added: Vec<(BBox, usize)>, out: &mut Vec<Change>) {
    let start = out.len();
    let mut items: Vec<(BBox, Item)> = removed
        .into_iter()
        .map(|(b, i)| (b, Item { added: false, layer: layers.0, feature: i, bbox: b }))
        .chain(added.into_iter().map(|(b, i)| (b, Item { added: true, layer: layers.1, feature: i, bbox: b })))
        .collect();
    items.sort_by(|a, b| a.0.min.x.total_cmp(&b.0.min.x));
    for (bbox, item) in items {
        // A piece can sit between two places that were started apart: it joins them into one.
        let near_ones: Vec<usize> = (start..out.len()).filter(|k| near(&out[*k].bbox, &bbox)).collect();
        match near_ones.split_first() {
            Some((first, rest)) => {
                for k in rest.iter().rev() {
                    let other = out.remove(*k);
                    let c = &mut out[*first];
                    c.bbox.union(&other.bbox);
                    c.removed += other.removed;
                    c.added += other.added;
                    c.items.extend(other.items);
                }
                let c = &mut out[*first];
                c.bbox.union(&bbox);
                c.items.push(item);
                if item.added {
                    c.added += 1;
                } else {
                    c.removed += 1;
                }
            }
            None => out.push(Change {
                key: key.to_string(),
                label: label.to_string(),
                bbox,
                items: vec![item],
                groups: Vec::new(),
                removed: !item.added as usize,
                added: item.added as usize,
            }),
        }
    }
}

/// Layers where connected strokes, such as the lines of a letter, count as one thing:
/// the silkscreens and Mechanical 99 (fab notes).
fn strokes_layer(l: &crate::board::Layer) -> bool {
    !l.role.copper && matches!(l.role.key.as_str(), "GTO" | "GBO" | "GM99")
}

/// Strokes this close count as joined, so the letters of a word share a marker.
const STROKE_GAP: f64 = 0.25;

/// How many traces will be followed in all, so a board that changed everywhere stays quick.
/// Strokes on silkscreen and fab notes are cheap and have their own, larger allowance.
struct Budget {
    traces: usize,
    strokes: usize,
}

/// Gathers a change's differences into markers: everything that is part of
/// the same trace shares one, and so do an old and a new piece that overlap.
fn group_items(a: &Board, b: &Board, items: &[Item], budget: &mut Budget) -> Vec<Group> {
    let n = items.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while parent[r] != r {
            r = parent[r];
        }
        let mut i = i;
        while parent[i] != r {
            let next = parent[i];
            parent[i] = r;
            i = next;
        }
        r
    }
    let join = |parent: &mut Vec<usize>, x: usize, y: usize| {
        let (rx, ry) = (find(parent, x), find(parent, y));
        if rx != ry {
            parent[ry] = rx;
        }
    };
    // Tracks pull in everything on their trace; on silkscreen and fab notes, connected strokes of any kind.
    let mut done = vec![false; n];
    let mut linkers: HashMap<(bool, usize), trace::Linker> = HashMap::new();
    for i in 0..n {
        let it = items[i];
        let board = if it.added { b } else { a };
        let Some(layer) = board.layers.get(it.layer) else { continue };
        let Some(f) = layer.data.features.get(it.feature) else { continue };
        let strokes = strokes_layer(layer);
        let is_track = matches!(f.kind, Kind::Track | Kind::Arc);
        let left = if strokes { &mut budget.strokes } else { &mut budget.traces };
        if done[i] || !(is_track || strokes && f.dark) || *left == 0 {
            continue;
        }
        *left -= 1;
        let members = if strokes {
            linkers.entry((it.added, it.layer)).or_insert_with(|| trace::Linker::new(layer, STROKE_GAP)).connected(it.feature)
        } else {
            let Some(members) = trace::connected(board, it.layer, it.feature) else { continue };
            members
        };
        let set: std::collections::HashSet<usize> = members.into_iter().collect();
        for (j, other) in items.iter().enumerate() {
            if other.added == it.added && other.layer == it.layer && set.contains(&other.feature) {
                join(&mut parent, i, j);
                done[j] = true;
            }
        }
    }
    // An old piece and a new piece in the same place are one difference.
    for i in 0..n {
        for j in (i + 1)..n {
            if items[i].added != items[j].added && items[i].bbox.grown(0.05).overlaps(&items[j].bbox) {
                join(&mut parent, i, j);
            }
        }
    }
    let mut by_root: Vec<(usize, Vec<Item>)> = Vec::new();
    for i in 0..n {
        let r = find(&mut parent, i);
        match by_root.iter_mut().find(|(root, _)| *root == r) {
            Some((_, list)) => list.push(items[i]),
            None => by_root.push((r, vec![items[i]])),
        }
    }
    by_root
        .into_iter()
        .map(|(_, list)| {
            let mut all = list[0].bbox;
            for it in &list {
                all.union(&it.bbox);
            }
            let mid = all.center();
            let nearest = list.iter().min_by(|x, y| x.bbox.center().dist(mid).total_cmp(&y.bbox.center().dist(mid))).copied().unwrap_or(list[0]);
            Group { at: nearest.bbox, items: list }
        })
        .collect()
}

/// B's shift to sit on top of A: when the two boards are the same size but
/// their origin moved, nothing else about them should count as changed.
pub fn auto_shift(a: &Board, b: &Board) -> P {
    let (x, y) = (a.outline_box(), b.outline_box());
    if x.is_empty() || y.is_empty() {
        return P::default();
    }
    let same_size = (x.w() - y.w()).abs() < 0.05 && (x.h() - y.h()).abs() < 0.05;
    let moved = x.min.dist(y.min);
    if same_size && moved > 0.001 { x.min.sub(y.min) } else { P::default() }
}

pub fn run(a: &Board, b: &Board, shift: P) -> Comparison {
    let mut statuses = HashMap::new();
    let mut changes = Vec::new();
    let mut truncated = false;
    let mut budget = MAX_ITEMS;
    for (ai, la) in a.layers.iter().enumerate() {
        let Some(bi) = b.layers.iter().position(|l| l.role.key == la.role.key) else {
            statuses.insert(la.role.key.clone(), Status::OnlyA);
            continue;
        };
        let lb = &b.layers[bi];
        let (removed, added) = layer_changes(la, lb, shift);
        let status = if removed.is_empty() && added.is_empty() {
            if la.byte_hash == lb.byte_hash { Status::Identical } else { Status::SameDrawing }
        } else {
            Status::Changed { removed: removed.len(), added: added.len() }
        };
        statuses.insert(la.role.key.clone(), status);
        if removed.len() + added.len() > budget {
            truncated = true;
            continue;
        }
        budget -= removed.len() + added.len();
        cluster(&la.role.key, &la.role.label, (ai, bi), removed, added, &mut changes);
    }
    for lb in &b.layers {
        statuses.entry(lb.role.key.clone()).or_insert(Status::OnlyB);
    }
    changes.sort_by(|x, y| {
        let (a, b) = (x.bbox.center(), y.bbox.center());
        b.y.total_cmp(&a.y).then(a.x.total_cmp(&b.x))
    });
    let mut budget = Budget { traces: 600, strokes: 20_000 };
    for c in &mut changes {
        c.groups = group_items(a, b, &c.items, &mut budget);
    }
    Comparison { statuses, changes, truncated }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::from_memory;
    use crate::samples;

    #[test]
    fn same_board_twice_is_identical() {
        let a = from_memory("a", samples::board('A'));
        let b = from_memory("b", samples::board('A'));
        let c = run(&a, &b, P::default());
        assert!(c.changes.is_empty());
        assert!(c.statuses.values().all(|s| *s == Status::Identical), "{:?}", c.statuses);
    }

    #[test]
    fn a_new_header_is_not_a_change() {
        let a = from_memory("a", samples::board('A'));
        let files: Vec<_> =
            samples::board('A')
            .into_iter()
            .map(|(n, t)| if n.starts_with("Demo_Board.") { (n, format!("G04 exported later*\n{t}")) } else { (n, t) })
            .collect();
        let b = from_memory("b", files);
        let c = run(&a, &b, P::default());
        assert!(c.changes.is_empty());
        assert_eq!(c.statuses["GTL"], Status::SameDrawing);
    }

    #[test]
    fn revision_changes_are_found_by_layer() {
        let a = from_memory("a", samples::board('A'));
        let b = from_memory("b", samples::board('B'));
        let c = run(&a, &b, P::default());
        assert!(matches!(c.statuses["GTL"], Status::Changed { .. }), "{:?}", c.statuses);
        assert!(matches!(c.statuses["PTH"], Status::Changed { .. }));
        assert_eq!(c.statuses["GTO"], Status::Identical);
        assert!(!c.changes.is_empty());
    }

    #[test]
    fn every_difference_belongs_to_one_marker() {
        let a = from_memory("a", samples::board('A'));
        let b = from_memory("b", samples::board('B'));
        let c = run(&a, &b, P::default());
        for change in &c.changes {
            assert!(!change.groups.is_empty());
            let grouped: usize = change.groups.iter().map(|g| g.items.len()).sum();
            assert_eq!(grouped, change.items.len());
            assert!(change.groups.len() <= change.items.len());
        }
        let markers: usize = c.changes.iter().map(|c| c.groups.len()).sum();
        let pieces: usize = c.changes.iter().map(|c| c.items.len()).sum();
        assert!(markers < pieces, "{markers} markers for {pieces} pieces");
    }

    /// A board with one file of straight strokes: (x1, y1, x2, y2) in mm, 0.15 mm wide.
    fn strokes(ext: &str, lines: &[(f64, f64, f64, f64)]) -> Board {
        let mut s = String::from("%FSLAX46Y46*%\n%MOMM*%\n%ADD10C,0.15*%\nD10*\n");
        let m = |v: f64| (v * 1e6).round() as i64;
        for l in lines {
            s.push_str(&format!("X{}Y{}D02*\nX{}Y{}D01*\n", m(l.0), m(l.1), m(l.2), m(l.3)));
        }
        s.push_str("M02*\n");
        from_memory("t", vec![(format!("t.{ext}"), s)])
    }

    #[test]
    fn connected_silk_strokes_share_one_marker() {
        // A bent line of three strokes in A, moved well away in B.
        let a = strokes("GTO", &[(0.0, 0.0, 3.0, 0.0), (3.0, 0.0, 3.0, 3.0), (3.0, 3.0, 6.0, 3.0)]);
        let b = strokes("GTO", &[(0.0, 10.0, 3.0, 10.0), (3.0, 10.0, 3.0, 13.0), (3.0, 13.0, 6.0, 13.0)]);
        let c = run(&a, &b, P::default());
        let groups: usize = c.changes.iter().map(|c| c.groups.len()).sum();
        let pieces: usize = c.changes.iter().map(|c| c.items.len()).sum();
        assert_eq!(pieces, 6);
        assert_eq!(groups, 2, "one marker for the old line and one for the new");
    }

    #[test]
    fn separate_silk_strokes_keep_their_own_markers() {
        let a = strokes("GTO", &[(0.0, 0.0, 3.0, 0.0), (0.0, 5.0, 3.0, 5.0)]);
        let b = strokes("GTO", &[(0.0, 10.0, 3.0, 10.0)]);
        let c = run(&a, &b, P::default());
        let groups: usize = c.changes.iter().map(|c| c.groups.len()).sum();
        assert_eq!(groups, 3);
    }

    #[test]
    fn letters_of_a_word_on_the_silk_share_a_marker() {
        // Two letters 0.2 mm apart, each of two strokes: they are one word, one marker per side.
        let word = |dy: f64| {
            strokes(
                "GBO",
                &[(0.0, dy, 0.0, dy + 1.0), (0.0, dy + 1.0, 0.6, dy + 1.0), (0.95, dy, 0.95, dy + 1.0), (0.95, dy + 1.0, 1.5, dy + 1.0)],
            )
        };
        let c = run(&word(0.0), &word(20.0), P::default());
        let groups: usize = c.changes.iter().map(|c| c.groups.len()).sum();
        assert_eq!(groups, 2);
    }

    #[test]
    fn connected_fab_note_strokes_share_one_marker() {
        let a = strokes("GM99", &[(0.0, 0.0, 3.0, 0.0), (3.0, 0.0, 3.0, 3.0)]);
        let b = strokes("GM99", &[(0.0, 10.0, 3.0, 10.0), (3.0, 10.0, 3.0, 13.0)]);
        let c = run(&a, &b, P::default());
        let groups: usize = c.changes.iter().map(|c| c.groups.len()).sum();
        assert_eq!(groups, 2);
    }

    #[test]
    fn moved_origin_is_lined_up_and_not_a_change() {
        let a = from_memory("a", samples::board('A'));
        let b = from_memory("b", samples::shifted(5.0, 3.0));
        let shift = auto_shift(&a, &b);
        assert!((shift.x + 5.0).abs() < 1e-6 && (shift.y + 3.0).abs() < 1e-6, "{shift:?}");
        let c = run(&a, &b, shift);
        assert!(c.changes.is_empty(), "{:?}", c.changes);
    }

    #[test]
    fn layer_in_one_side_only() {
        let a = from_memory("a", samples::board('A'));
        let mut files = samples::board('B');
        files.retain(|(n, _)| !n.ends_with(".GBO"));
        files.push(("Demo.GM2".to_string(), samples::board('A').iter().find(|(n, _)| n.ends_with(".GKO")).unwrap().1.clone()));
        let b = from_memory("b", files);
        let c = run(&a, &b, P::default());
        assert_eq!(c.statuses["GBO"], Status::OnlyA);
        assert_eq!(c.statuses["GM2"], Status::OnlyB);
    }
}
