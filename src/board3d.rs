//! Turns a set of Gerbers into a solid board: the outline is traced into a closed
//! shape, then given a top, a bottom, edges and drilled holes.

use crate::altium::Group;
use crate::board::{Board, Layer};
use crate::geom::*;
use crate::gerber::Kind;
use std::sync::Arc;

/// Line ends closer than this count as joined.
const JOIN_MM: f64 = 0.05;
const MOST_EDGE_PIECES: usize = 20_000;
/// Fewer sides per hole once a board has very many of them.
const MANY_HOLES: usize = 20_000;

/// What a wall is made of.
pub const WALL_FR4: f32 = 0.0;
pub const WALL_PLATED: f32 = 1.0;

/// Which layers paint one face of the board.
#[derive(Clone, Default)]
pub struct Face {
    pub copper: Option<Arc<Layer>>,
    pub mask: Option<Arc<Layer>>,
    pub silk: Option<Arc<Layer>>,
}

pub struct Model {
    /// Tells one board from another; it's the id of the board's first layer.
    pub id: u64,
    /// x, y, z (z is -0.5 to 0.5, scaled by the thickness), then the normal,
    /// then what a wall is made of.
    pub verts: Vec<f32>,
    pub idx: Vec<u32>,
    pub top_count: i32,
    pub bottom_count: i32,
    pub wall_count: i32,
    pub min: [f32; 2],
    pub max: [f32; 2],
    pub top: Face,
    pub bottom: Face,
    pub drills: Vec<Arc<Layer>>,
    /// False when no closed outline was found and a plain rectangle stands in.
    pub traced: bool,
}

fn ring_of_box(b: &BBox) -> Vec<P> {
    vec![b.min, p(b.max.x, b.min.y), b.max, p(b.min.x, b.max.y)]
}

/// Joins separate lines into closed loops.
fn close_loops(mut pieces: Vec<Vec<P>>) -> Vec<Vec<P>> {
    let mut loops = Vec::new();
    while let Some(mut path) = pieces.pop() {
        loop {
            let (Some(&start), Some(&end)) = (path.first(), path.last()) else { break };
            if path.len() > 2 && start.dist(end) <= JOIN_MM {
                path.pop();
                loops.push(path);
                break;
            }
            let next = pieces.iter().enumerate().find_map(|(i, q)| {
                let (first, last) = (*q.first()?, *q.last()?);
                if first.dist(end) <= JOIN_MM {
                    Some((i, false))
                } else if last.dist(end) <= JOIN_MM {
                    Some((i, true))
                } else {
                    None
                }
            });
            let Some((i, backwards)) = next else { break };
            let mut piece = pieces.swap_remove(i);
            if backwards {
                piece.reverse();
            }
            path.extend_from_slice(&piece[1..]);
        }
    }
    loops
}

/// The board's outside edge, then any cut-outs inside it.
pub fn trace_outline(board: &Board) -> Option<Vec<Vec<P>>> {
    let layer = board.layers.get(board.outline?)?;
    let mut pieces = Vec::new();
    let mut loops = Vec::new();
    for f in &layer.data.features {
        match f.kind {
            Kind::Track | Kind::Arc if f.length() > 1e-6 => pieces.push(f.path.clone()),
            Kind::Region if f.path.len() >= 3 => loops.push(f.path.clone()),
            _ => {}
        }
    }
    if pieces.len() > MOST_EDGE_PIECES {
        return None;
    }
    loops.extend(close_loops(pieces));
    loops.sort_by(|a, b| ring_area(b).total_cmp(&ring_area(a)));
    let outer = loops.first()?.clone();
    // A small closed shape on the outline layer isn't the board.
    let edge = board.outline_box();
    if ring_area(&outer) < edge.w() * edge.h() * 0.4 {
        return None;
    }
    let mut rings = vec![outer.clone()];
    rings.extend(loops.into_iter().skip(1).filter(|r| point_in_ring(r[0], &outer)));
    Some(rings)
}

fn add_face(verts: &mut Vec<f32>, idx: &mut Vec<u32>, tris: &Tris, z: f32, normal: f32) -> i32 {
    let base = (verts.len() / 7) as u32;
    for q in &tris.pts {
        verts.extend_from_slice(&[q.x as f32, q.y as f32, z, 0.0, 0.0, normal, WALL_FR4]);
    }
    idx.extend(tris.idx.iter().map(|i| base + i));
    tris.idx.len() as i32
}

fn add_wall(verts: &mut Vec<f32>, idx: &mut Vec<u32>, ring: &[P], stuff: f32, smooth: bool) -> i32 {
    let n = ring.len();
    if n < 3 {
        return 0;
    }
    let mut added = 0;
    if smooth {
        // Round holes share their corners, which makes the walls look round.
        let mid = ring.iter().fold(P::default(), |s, q| s.add(*q)).scale(1.0 / n as f64);
        let base = (verts.len() / 7) as u32;
        for q in ring {
            let out = q.sub(mid);
            let l = out.len().max(1e-9);
            for z in [0.5, -0.5] {
                let (nx, ny) = ((out.x / l) as f32, (out.y / l) as f32);
                verts.extend_from_slice(&[q.x as f32, q.y as f32, z, nx, ny, 0.0, stuff]);
            }
        }
        for i in 0..n as u32 {
            let (a, b) = (base + i * 2, base + (i + 1) % n as u32 * 2);
            idx.extend_from_slice(&[a, a + 1, b, b, a + 1, b + 1]);
            added += 6;
        }
        return added;
    }
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let d = b.sub(a);
        let l = d.len();
        if l < 1e-9 {
            continue;
        }
        let (nx, ny) = ((d.y / l) as f32, (-d.x / l) as f32);
        let base = (verts.len() / 7) as u32;
        for (q, z) in [(a, 0.5), (a, -0.5), (b, 0.5), (b, -0.5)] {
            verts.extend_from_slice(&[q.x as f32, q.y as f32, z, nx, ny, 0.0, stuff]);
        }
        idx.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
        added += 6;
    }
    added
}

fn layer_with_key(board: &Board, key: &str) -> Option<Arc<Layer>> {
    board.layers.iter().find(|l| l.role.key == key).cloned()
}

fn holes_of(drills: &[Arc<Layer>], verts: &mut Vec<f32>, idx: &mut Vec<u32>) -> i32 {
    let total: usize = drills.iter().map(|l| l.stats.holes + l.stats.slots).sum();
    let sides = if total > MANY_HOLES { 10 } else { 20 };
    let mut added = 0;
    for layer in drills {
        let plated = layer.data.plated.unwrap_or(!layer.role.key.starts_with("NPTH"));
        let stuff = if plated { WALL_PLATED } else { WALL_FR4 };
        for f in &layer.data.features {
            let ring: Vec<P> = match f.kind {
                Kind::Hole => (0..sides)
                    .map(|i| {
                        let t = i as f64 / sides as f64 * std::f64::consts::TAU;
                        f.path[0].add(p(t.cos(), t.sin()).scale(f.width / 2.0))
                    })
                    .collect(),
                Kind::Slot => capsule_ring(f.path[0], f.path[1], f.width),
                _ => continue,
            };
            added += add_wall(verts, idx, &ring, stuff, f.kind == Kind::Hole);
        }
    }
    added
}

pub fn build(board: &Board) -> Option<Model> {
    let id = board.layers.first()?.id;
    let traced = trace_outline(board);
    let is_traced = traced.is_some();
    let rings = traced.unwrap_or_else(|| {
        let edge = board.outline_box();
        vec![ring_of_box(if edge.is_empty() { &board.bbox } else { &edge })]
    });
    if rings[0].len() < 3 {
        return None;
    }
    let mut area = BBox::EMPTY;
    rings[0].iter().for_each(|q| area.add(*q));
    if area.w() <= 0.0 || area.h() <= 0.0 {
        return None;
    }

    let tris = triangulate(&rings);
    let (mut verts, mut idx) = (Vec::new(), Vec::new());
    let top_count = add_face(&mut verts, &mut idx, &tris, 0.5, 1.0);
    let bottom_count = add_face(&mut verts, &mut idx, &tris, -0.5, -1.0);
    let mut wall_count = 0;
    for ring in &rings {
        wall_count += add_wall(&mut verts, &mut idx, ring, WALL_FR4, false);
    }
    let drills: Vec<Arc<Layer>> = board.layers.iter().filter(|l| l.role.group == Group::Drill).cloned().collect();
    wall_count += holes_of(&drills, &mut verts, &mut idx);

    let face = |copper: &str, mask: &str, silk: &str| Face {
        copper: layer_with_key(board, copper),
        mask: layer_with_key(board, mask),
        silk: layer_with_key(board, silk),
    };
    Some(Model {
        id,
        verts,
        idx,
        top_count,
        bottom_count,
        wall_count,
        min: [area.min.x as f32, area.min.y as f32],
        max: [area.max.x as f32, area.max.y as f32],
        top: face("GTL", "GTS", "GTO"),
        bottom: face("GBL", "GBS", "GBO"),
        drills,
        traced: is_traced,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::from_memory;
    use crate::samples;

    #[test]
    fn the_sample_outline_closes_into_the_board_shape() {
        let b = from_memory("s", samples::board('A'));
        let rings = trace_outline(&b).expect("a closed outline");
        assert_eq!(rings.len(), 1);
        // A 60 by 40 rectangle less its rounded corners.
        let want = 60.0 * 40.0 - (4.0 - std::f64::consts::PI) * 9.0;
        assert!((ring_area(&rings[0]) - want).abs() < 0.5, "area {}", ring_area(&rings[0]));
    }

    #[test]
    fn lines_join_whichever_way_round_they_were_drawn() {
        let pieces = vec![
            vec![p(0.0, 0.0), p(10.0, 0.0)],
            vec![p(10.0, 10.0), p(10.0, 0.01)],
            vec![p(10.0, 10.0), p(0.0, 10.0)],
            vec![p(0.0, 0.0), p(0.0, 10.0)],
        ];
        let loops = close_loops(pieces);
        assert_eq!(loops.len(), 1);
        assert!((ring_area(&loops[0]) - 100.0).abs() < 0.2);
    }

    #[test]
    fn a_gap_in_the_outline_falls_back_to_a_rectangle() {
        let text = "%FSLAX26Y26*%\n%MOMM*%\n%ADD10C,0.2*%\nD10*\nX0Y0D02*\nX50000000Y0D01*\nY30000000D01*\nX0D01*\nM02*\n";
        let b = from_memory("g", vec![("x.GKO".to_string(), text.to_string())]);
        assert!(trace_outline(&b).is_none());
        let model = build(&b).expect("still a board");
        assert!(!model.traced);
        assert!((model.max[0] - model.min[0] - 50.0).abs() < 0.5);
    }

    #[test]
    fn a_cut_out_stays_inside_the_board() {
        let mut text = String::from("%FSLAX26Y26*%\n%MOMM*%\n%ADD10C,0.2*%\nD10*\n");
        for (a, b, c, d) in [(0, 0, 40, 40), (10, 10, 20, 20)] {
            text.push_str(&format!(
                "X{a}000000Y{b}000000D02*\nX{c}000000D01*\nY{d}000000D01*\nX{a}000000D01*\nY{b}000000D01*\n"
            ));
        }
        text.push_str("M02*\n");
        let b = from_memory("c", vec![("x.GKO".to_string(), text)]);
        let rings = trace_outline(&b).expect("closed");
        assert_eq!(rings.len(), 2);
        assert!(ring_area(&rings[0]) > ring_area(&rings[1]));
    }

    #[test]
    fn the_model_has_both_faces_and_drilled_walls() {
        let b = from_memory("s", samples::board('A'));
        let m = build(&b).unwrap();
        assert!(m.traced);
        assert_eq!(m.top_count, m.bottom_count);
        let edge_only = 6 * 8 * 3; // far fewer than walls with every hole added
        assert!(m.wall_count > edge_only);
        assert_eq!(m.idx.len() as i32, m.top_count + m.bottom_count + m.wall_count);
        assert!(m.top.copper.is_some() && m.bottom.mask.is_some() && m.top.silk.is_some());
    }
}
