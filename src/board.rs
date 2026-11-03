//! A set of Gerber files opened from a folder or a zip, ready to draw and to click on.

use crate::altium::{self, Group, Role};
use crate::excellon;
use crate::geom::*;
use crate::gerber::{self, ApShape, Hit, Kind, Parsed};
use std::collections::{HashMap, HashSet};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A zip can claim to unpack to anything; stop well before that hurts.
const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_ZIP_DEPTH: u32 = 3;
const MAX_FOLDER_DEPTH: u32 = 4;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Finds which features sit near a point without testing all of them.
pub struct HitGrid {
    origin: P,
    cell: f64,
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    /// Shapes too big to be worth filing cell by cell, such as whole pours.
    large: Vec<u32>,
}

impl HitGrid {
    fn new(parsed: &Parsed) -> Self {
        let b = parsed.bbox;
        let n = parsed.features.len().max(1) as f64;
        let side = (n.sqrt().ceil() as usize).clamp(1, 512);
        let span = b.w().max(b.h()).max(0.001);
        let cell = span / side as f64;
        let nx = ((b.w() / cell).ceil() as usize + 1).min(1024);
        let ny = ((b.h() / cell).ceil() as usize + 1).min(1024);
        let mut grid = HitGrid { origin: b.min, cell, nx, ny, cells: vec![Vec::new(); nx * ny], large: Vec::new() };
        if b.is_empty() {
            return grid;
        }
        for (i, f) in parsed.features.iter().enumerate() {
            let (x0, y0) = grid.index(f.bbox.min);
            let (x1, y1) = grid.index(f.bbox.max);
            if (x1 - x0 + 1) * (y1 - y0 + 1) > 256 {
                grid.large.push(i as u32);
                continue;
            }
            for y in y0..=y1 {
                for x in x0..=x1 {
                    grid.cells[y * nx + x].push(i as u32);
                }
            }
        }
        grid
    }

    fn index(&self, q: P) -> (usize, usize) {
        let x = ((q.x - self.origin.x) / self.cell).floor().max(0.0) as usize;
        let y = ((q.y - self.origin.y) / self.cell).floor().max(0.0) as usize;
        (x.min(self.nx - 1), y.min(self.ny - 1))
    }

    /// Features around a point, newest first.
    pub fn near(&self, q: P, tol: f64) -> Vec<u32> {
        let (x0, y0) = self.index(p(q.x - tol, q.y - tol));
        let (x1, y1) = self.index(p(q.x + tol, q.y + tol));
        let mut out = self.large.clone();
        for y in y0..=y1 {
            for x in x0..=x1 {
                out.extend_from_slice(&self.cells[y * self.nx + x]);
            }
        }
        out.sort_unstable_by(|a, b| b.cmp(a));
        out.dedup();
        out
    }
}

pub struct Stats {
    pub pads: usize,
    pub tracks: usize,
    pub regions: usize,
    pub holes: usize,
    pub slots: usize,
    pub min_track: Option<f64>,
    pub min_hole: Option<f64>,
    pub track_length: f64,
}

impl Stats {
    fn of(parsed: &Parsed) -> Self {
        let mut s = Stats {
            pads: 0,
            tracks: 0,
            regions: 0,
            holes: 0,
            slots: 0,
            min_track: None,
            min_hole: None,
            track_length: 0.0,
        };
        let least = |slot: &mut Option<f64>, v: f64| {
            if v > 0.0 {
                *slot = Some(slot.map_or(v, |o: f64| o.min(v)));
            }
        };
        for f in &parsed.features {
            match f.kind {
                Kind::Pad => s.pads += 1,
                Kind::Track | Kind::Arc => {
                    s.tracks += 1;
                    s.track_length += f.length();
                    if f.dark && f.length() > 1e-6 {
                        least(&mut s.min_track, f.width);
                    }
                }
                Kind::Region => s.regions += 1,
                Kind::Hole => {
                    s.holes += 1;
                    least(&mut s.min_hole, f.width);
                }
                Kind::Slot => {
                    s.slots += 1;
                    least(&mut s.min_hole, f.width);
                }
            }
        }
        s
    }
}

pub struct Layer {
    /// Unique for this run, so the drawing code can find its copy.
    pub id: u64,
    pub file: String,
    pub role: Role,
    pub data: Parsed,
    pub mesh: Mesh,
    pub grid: HitGrid,
    pub stats: Stats,
    pub byte_hash: u64,
}

#[derive(Clone, Debug)]
pub struct SideFile {
    pub name: String,
    pub note: String,
}

/// Something else in the place that was opened which could go on either side:
/// another zip in the folder, or another revision's folder in the zip.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Choice {
    pub path: PathBuf,
    pub set: Option<String>,
    pub name: String,
}

pub struct Board {
    pub name: String,
    pub path: PathBuf,
    pub is_zip: bool,
    pub layers: Vec<Arc<Layer>>,
    pub side_files: Vec<SideFile>,
    pub choices: Vec<Choice>,
    pub bbox: BBox,
    pub outline: Option<usize>,
    /// The board's edge, measured down the middle of the outline's lines.
    pub edge: BBox,
    pub warnings: Vec<String>,
}

impl Board {
    pub fn outline_box(&self) -> BBox {
        self.edge
    }

    pub fn copper_layers(&self) -> usize {
        self.layers.iter().filter(|l| l.role.copper).count()
    }
}

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn split_name(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext)) => (stem, ext),
        None => (name, ""),
    }
}

fn base_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn is_zip_name(name: &str) -> bool {
    name.to_lowercase().ends_with(".zip")
}

struct Found {
    name: String,
    bytes: Vec<u8>,
}

fn read_zip(bytes: &[u8], prefix: &str, depth: u32, out: &mut Vec<Found>, total: &mut u64) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("That zip couldn't be read: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(entry) => entry,
            Err(e) => return Err(format!("Part of that zip couldn't be read: {e}")),
        };
        if !entry.is_file() {
            continue;
        }
        let name = entry.name().map_err(|e| format!("A name in that zip couldn't be read: {e}"))?.replace('\\', "/");
        if name.starts_with("__MACOSX/") || base_name(&name).starts_with("._") {
            continue;
        }
        if entry.size() > MAX_FILE_BYTES || *total + entry.size() > MAX_TOTAL_BYTES {
            return Err(format!("{name} inside the zip is too big to open."));
        }
        let mut data = Vec::with_capacity(entry.size().min(64 * 1024 * 1024) as usize);
        // The stated size isn't trusted: read no further than the limit.
        (&mut entry)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| format!("{name} in the zip couldn't be read: {e}"))?;
        if data.len() as u64 > MAX_FILE_BYTES {
            return Err(format!("{name} inside the zip is too big to open."));
        }
        *total += data.len() as u64;
        let full = format!("{prefix}{name}");
        if is_zip_name(&name) && depth < MAX_ZIP_DEPTH {
            drop(entry);
            // A broken zip inside a zip shouldn't stop the good files around it.
            let _ = read_zip(&data, &format!("{full}/"), depth + 1, out, total);
        } else {
            out.push(Found { name: full, bytes: data });
        }
    }
    Ok(())
}

fn walk_folder(dir: &Path, prefix: &str, depth: u32, out: &mut Vec<Found>, zips: &mut Vec<Choice>, total: &mut u64) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("{} couldn't be opened: {e}", dir.display()))?;
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().to_string()) else { continue };
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if meta.is_dir() {
            if depth < MAX_FOLDER_DEPTH && !name.starts_with('.') {
                // Unreadable subfolders are skipped; the rest still opens.
                let _ = walk_folder(&path, &format!("{prefix}{name}/"), depth + 1, out, zips, total);
            }
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        if is_zip_name(&name) {
            zips.push(Choice { path, set: None, name: format!("{prefix}{name}") });
            continue;
        }
        if meta.len() > MAX_FILE_BYTES || *total + meta.len() > MAX_TOTAL_BYTES {
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            *total += bytes.len() as u64;
            out.push(Found { name: format!("{prefix}{name}"), bytes });
        }
    }
    Ok(())
}

enum Read2 {
    Layer(Box<Layer>),
    Side(SideFile),
    Skip,
}

fn read_one(found: &Found) -> Read2 {
    let name = base_name(&found.name);
    let (stem, ext) = split_name(name);
    let byte_hash = hash_bytes(&found.bytes);
    let side = |note: &str| Read2::Side(SideFile { name: found.name.clone(), note: note.to_string() });
    if altium::is_side_file(ext) {
        return side("Report or settings");
    }
    let text = String::from_utf8_lossy(&found.bytes);
    let named = altium::from_extension(ext);
    let as_drill = excellon::looks_like_drill(&text);
    let parsed = if as_drill || gerber::looks_like_gerber(&text) {
        // One bad file shouldn't take the rest of the folder down with it.
        let read = std::panic::catch_unwind(|| if as_drill { excellon::parse(&text) } else { gerber::parse(&text) });
        match read {
            Ok(parsed) => parsed,
            Err(_) => return side("Couldn't be read"),
        }
    } else {
        // Altium's Status Report is a .txt; its old .DRL files are binary.
        return match ext.to_uppercase().as_str() {
            "TXT" | "DRL" | "XLN" | "NC" | "CSV" | "MD" => side("Not a drawing"),
            _ if named.is_some() => side("Couldn't be read as a Gerber"),
            _ => Read2::Skip,
        };
    };
    let role = if parsed.is_drill {
        altium::drill_role(stem, parsed.plated)
    } else {
        named
            .or_else(|| parsed.file_function.as_deref().and_then(altium::from_file_function))
            .or_else(|| altium::from_name(stem))
            .unwrap_or_else(|| altium::unknown_role(name))
    };
    let drawn = std::panic::catch_unwind(|| {
        let mut mesh = Mesh::default();
        for f in &parsed.features {
            f.tessellate(&parsed.apertures, &mut mesh);
        }
        mesh
    });
    let Ok(mesh) = drawn else { return side("Couldn't be drawn") };
    let grid = HitGrid::new(&parsed);
    let stats = Stats::of(&parsed);
    Read2::Layer(Box::new(Layer {
        id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        file: found.name.clone(),
        role,
        data: parsed,
        mesh,
        grid,
        stats,
        byte_hash,
    }))
}

/// Picks the layer that draws the board's edge. Altium boards use the keep-out
/// or a mechanical layer depending on the designer, so this looks at the shapes.
fn find_outline(layers: &[Arc<Layer>]) -> Option<usize> {
    let mut copper = BBox::EMPTY;
    for l in layers.iter().filter(|l| l.role.copper) {
        copper.union(&l.data.bbox);
    }
    let candidates = layers.iter().enumerate().filter(|(_, l)| {
        let key = &l.role.key;
        key == "GKO" || key.starts_with("GM")
    });
    let mut best: Option<(i64, usize)> = None;
    for (i, l) in candidates {
        let b = l.data.bbox;
        if b.is_empty() || l.data.features.is_empty() {
            continue;
        }
        let drawn_as_lines = l.data.features.iter().filter(|f| matches!(f.kind, Kind::Track | Kind::Arc)).count();
        let share = drawn_as_lines as f64 / l.data.features.len() as f64;
        let covers = copper.is_empty()
            || (b.min.x <= copper.min.x + 0.5
                && b.min.y <= copper.min.y + 0.5
                && b.max.x >= copper.max.x - 0.5
                && b.max.y >= copper.max.y - 0.5);
        // Simple outlines are short; a mechanical layer with drawings on it isn't one.
        let mut score = (share * 40.0) as i64;
        if covers {
            score += 100;
        }
        if l.data.features.len() < 400 {
            score += 20;
        }
        if l.role.key == "GKO" {
            score += 5;
        }
        if l.role.key == "GM1" {
            score += 4;
        }
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, i));
        }
    }
    best.filter(|(score, _)| *score >= 100).map(|(_, i)| i)
}

fn assemble(found: Vec<Found>, choices: Vec<Choice>, name: String, path: PathBuf, is_zip: bool) -> Board {
    let queue = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<Read2> = std::thread::scope(|scope| {
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8);
        let found = &found;
        let queue = &queue;
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(move || {
                    let mut done = Vec::new();
                    loop {
                        let i = queue.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = found.get(i) else { break };
                        done.push((i, read_one(item)));
                    }
                    done
                })
            })
            .collect();
        let mut all: Vec<(usize, Read2)> = workers.into_iter().flat_map(|w| w.join().unwrap_or_default()).collect();
        all.sort_by_key(|(i, _)| *i);
        all.into_iter().map(|(_, r)| r).collect()
    });

    let mut layers: Vec<Layer> = Vec::new();
    let mut side_files = Vec::new();
    for r in results {
        match r {
            Read2::Layer(l) => layers.push(*l),
            Read2::Side(s) => side_files.push(s),
            Read2::Skip => {}
        }
    }
    layers.sort_by(|a, b| a.role.order.cmp(&b.role.order).then(a.file.cmp(&b.file)));

    // Two boards in one place: keep both, numbering the repeats so each pairs up.
    let mut seen: HashMap<String, u32> = HashMap::new();
    for l in &mut layers {
        let n = seen.entry(l.role.key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            l.role.key = format!("{}#{n}", l.role.key);
            l.role.label = format!("{} ({n})", l.role.label);
        }
    }

    let mut warnings = Vec::new();
    let mut bbox = BBox::EMPTY;
    for l in &layers {
        // A stray file with no shapes shouldn't stretch the view.
        bbox.union(&l.data.bbox);
        for w in &l.data.warnings {
            warnings.push(format!("{}: {w}", base_name(&l.file)));
        }
    }
    let layers: Vec<Arc<Layer>> = layers.into_iter().map(Arc::new).collect();
    let outline = find_outline(&layers);
    let mut edge = BBox::EMPTY;
    if let Some(i) = outline {
        for f in &layers[i].data.features {
            f.path.iter().for_each(|q| edge.add(*q));
        }
    } else {
        edge = bbox;
    }
    Board { name, path, is_zip, layers, side_files, choices, bbox, outline, edge, warnings }
}

/// Finds separate sets of Gerbers in one place, such as RevA/ and RevB/ inside a
/// zip. Altium's own Gerber/ and NC Drill/ folders are one set, because only one
/// of them holds a top copper file. Returns the folder they share and the sets.
fn find_sets(found: &[Found]) -> (String, Vec<String>) {
    let mut dirs: Vec<&str> = Vec::new();
    for tag in ["GTL", "GBL", "GKO"] {
        dirs = found
            .iter()
            .filter(|f| split_name(base_name(&f.name)).1.eq_ignore_ascii_case(tag))
            .map(|f| f.name.rsplit_once('/').map_or("", |(dir, _)| dir))
            .collect();
        dirs.sort_unstable();
        dirs.dedup();
        if !dirs.is_empty() {
            break;
        }
    }
    if dirs.len() < 2 {
        return (String::new(), Vec::new());
    }
    let parts: Vec<Vec<&str>> = dirs.iter().map(|d| d.split('/').filter(|c| !c.is_empty()).collect()).collect();
    let shared = (0..parts[0].len())
        .take_while(|i| parts.iter().all(|q| q.get(*i) == Some(&parts[0][*i])))
        .count();
    let mut sets: Vec<String> = parts.iter().map(|q| q.get(shared).copied().unwrap_or("").to_string()).collect();
    sets.sort();
    sets.dedup();
    if sets.len() < 2 {
        return (String::new(), Vec::new());
    }
    let prefix = parts[0][..shared].join("/");
    (if prefix.is_empty() { prefix } else { format!("{prefix}/") }, sets)
}

fn in_set(name: &str, prefix: &str, set: &str) -> bool {
    let Some(rest) = name.strip_prefix(prefix) else { return false };
    if set.is_empty() {
        !rest.contains('/')
    } else {
        rest.strip_prefix(set).is_some_and(|tail| tail.starts_with('/'))
    }
}

/// Turns what was found into boards: one, or one for each set inside. Without
/// a choice of set, the first two come back so they open as a comparison.
fn build(found: Vec<Found>, zips: Vec<Choice>, name: String, path: PathBuf, is_zip: bool, set: Option<&str>) -> Vec<Board> {
    let (prefix, sets) = find_sets(&found);
    if sets.is_empty() {
        return vec![assemble(found, zips, name, path, is_zip)];
    }
    let label = |s: &str| if s.is_empty() { "Top folder".to_string() } else { s.to_string() };
    let choices: Vec<Choice> = sets
        .iter()
        .map(|s| Choice { path: path.clone(), set: Some(s.clone()), name: label(s) })
        .collect();
    let wanted: Vec<&String> = match set {
        Some(one) => sets.iter().filter(|s| s.as_str() == one).collect(),
        None => sets.iter().take(2).collect(),
    };
    let mut groups: Vec<Vec<Found>> = wanted.iter().map(|_| Vec::new()).collect();
    for f in found {
        if let Some(i) = wanted.iter().position(|s| in_set(&f.name, &prefix, s)) {
            groups[i].push(f);
        }
    }
    groups
        .into_iter()
        .zip(&wanted)
        .map(|(files, s)| assemble(files, choices.clone(), format!("{name} / {}", label(s)), path.clone(), is_zip))
        .collect()
}

/// Opens a folder or a zip, or one set inside it. A folder with no Gerbers of
/// its own but with zips in it opens the first two, ready to compare, and
/// lists the rest. Usually gives one board; two when there's a pair to compare.
pub fn open(path: &Path, set: Option<&str>) -> Result<Vec<Board>, String> {
    let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string());
    let mut total = 0;
    if path.is_dir() {
        let (mut found, mut zips) = (Vec::new(), Vec::new());
        walk_folder(path, "", 0, &mut found, &mut zips, &mut total)?;
        let boards = build(found, zips.clone(), name.clone(), path.to_path_buf(), false, set);
        if boards.iter().all(|b| b.layers.is_empty()) {
            let mut opened = Vec::new();
            for zip in zips.iter().take(2) {
                let mut inside = open(&zip.path, None)?;
                inside.truncate(1);
                opened.extend(inside);
            }
            if opened.is_empty() {
                return Err(format!("No Gerber or drill files were found in {name}."));
            }
            for board in &mut opened {
                board.choices = zips.clone();
            }
            return Ok(opened);
        }
        return Ok(boards);
    }
    if !path.is_file() {
        return Err(format!("{} wasn't found.", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{name} couldn't be read: {e}"))?;
    if !is_zip_name(&name) && !bytes.starts_with(b"PK") {
        // A single Gerber file: open the folder it's in.
        if let Some(parent) = path.parent().filter(|q| !q.as_os_str().is_empty()) {
            return open(parent, set);
        }
    }
    let mut found = Vec::new();
    read_zip(&bytes, "", 0, &mut found, &mut total)?;
    let boards = build(found, Vec::new(), name.clone(), path.to_path_buf(), true, set);
    if boards.iter().all(|b| b.layers.is_empty()) {
        return Err(format!("No Gerber or drill files were found inside {name}."));
    }
    Ok(boards)
}

/// Builds a board from text files in memory: used for the sample.
pub fn from_memory(name: &str, files: Vec<(String, String)>) -> Board {
    let found = files.into_iter().map(|(name, text)| Found { name, bytes: text.into_bytes() }).collect();
    assemble(found, Vec::new(), name.to_string(), PathBuf::new(), false)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Picked {
    pub layer: usize,
    pub feature: usize,
    pub near: bool,
}

pub const PICK_NAMES: [&str; 4] = ["Pads", "Tracks", "Holes", "Shapes"];

/// Which of the four groups a kind of feature belongs to.
pub fn pick_group(kind: Kind) -> usize {
    match kind {
        Kind::Pad => 0,
        Kind::Track | Kind::Arc => 1,
        Kind::Hole | Kind::Slot => 2,
        Kind::Region => 3,
    }
}

/// What a click can land on, and which wins when several things are stacked.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct PickRules {
    pub on: [bool; 4],
    pub first: Option<usize>,
}

impl Default for PickRules {
    fn default() -> Self {
        PickRules { on: [true; 4], first: None }
    }
}

impl PickRules {
    pub fn allows(&self, kind: Kind) -> bool {
        self.on[pick_group(kind)]
    }

    pub fn is_default(&self) -> bool {
        *self == PickRules::default()
    }

    pub fn label(&self) -> String {
        let count = self.on.iter().filter(|v| **v).count();
        match (self.first, count) {
            (Some(i), _) => format!("{} first", PICK_NAMES[i]),
            (None, 4) => "Pick all".to_string(),
            (None, 0) => "Pick none".to_string(),
            (None, 1) => {
                let i = self.on.iter().position(|v| *v).unwrap_or(0);
                format!("{} only", PICK_NAMES[i])
            }
            (None, n) => format!("Pick {n} of 4"),
        }
    }
}

/// What's under a point on one layer, respecting cut-outs: a clear shape on top
/// means there's nothing there to click. Kinds the rules turn off are looked
/// straight through.
pub fn pick_layer(board: &Board, layer: usize, q: P, tol: f64, rules: &PickRules, out: &mut Vec<Picked>) {
    let l = &board.layers[layer];
    let mut found_near = None;
    for i in l.grid.near(q, tol) {
        let f = &l.data.features[i as usize];
        let hit = f.hit(q, tol, &l.data.apertures);
        if hit == Hit::On && !f.dark {
            return;
        }
        if !rules.allows(f.kind) {
            continue;
        }
        match hit {
            Hit::On => {
                out.push(Picked { layer, feature: i as usize, near: false });
                // Stacked pads on the same spot can each be reached by clicking
                // again. A fill hides what's under it unless something else is asked for.
                if f.kind == Kind::Region && rules.first.is_none() {
                    return;
                }
            }
            Hit::Near if f.dark && found_near.is_none() => found_near = Some(i as usize),
            _ => {}
        }
    }
    if let Some(feature) = found_near
        && !out.iter().any(|h| h.layer == layer && !h.near)
    {
        out.push(Picked { layer, feature, near: true });
    }
}

/// The nice places to stick a measurement end to, near a point.
pub fn snap_points(board: &Board, layer: usize, q: P, tol: f64, rules: &PickRules) -> Option<(P, usize)> {
    let l = &board.layers[layer];
    let mut best: Option<(f64, P, usize)> = None;
    for i in l.grid.near(q, tol) {
        let f = &l.data.features[i as usize];
        if !f.dark || !rules.allows(f.kind) {
            continue;
        }
        let mut consider = |c: P| {
            let d = c.dist(q);
            if d <= tol && best.is_none_or(|b| d < b.0) {
                best = Some((d, c, i as usize));
            }
        };
        match f.kind {
            Kind::Pad | Kind::Hole => consider(f.path[0]),
            Kind::Track | Kind::Slot => {
                consider(f.path[0]);
                consider(f.path[1]);
            }
            Kind::Arc => {
                consider(f.path[0]);
                consider(f.path[f.path.len() - 1]);
                if let Some(arc) = f.arc {
                    consider(arc.centre);
                }
            }
            Kind::Region => f.path.iter().take(4096).for_each(|c| consider(*c)),
        }
    }
    best.map(|(_, c, i)| (c, i))
}

/// How many pads, holes and so on the whole board has, for the summary card.
pub struct Summary {
    pub w: f64,
    pub h: f64,
    pub copper: usize,
    pub holes: usize,
    pub min_track: Option<f64>,
    pub min_hole: Option<f64>,
    pub pads: usize,
    pub nets: usize,
}

pub fn summarize(board: &Board) -> Summary {
    let b = board.outline_box();
    let mut s = Summary {
        w: b.w(),
        h: b.h(),
        copper: board.copper_layers(),
        holes: 0,
        min_track: None,
        min_hole: None,
        pads: 0,
        nets: 0,
    };
    let mut nets = HashSet::new();
    let least = |slot: &mut Option<f64>, v: Option<f64>| {
        if let Some(v) = v {
            *slot = Some(slot.map_or(v, |o: f64| o.min(v)));
        }
    };
    for l in &board.layers {
        if l.role.group == Group::Drill {
            s.holes += l.stats.holes + l.stats.slots;
            least(&mut s.min_hole, l.stats.min_hole);
        }
        if l.role.copper {
            s.pads += l.stats.pads;
            least(&mut s.min_track, l.stats.min_track);
            for f in &l.data.features {
                if f.net != 0
                    && let Some(n) = l.data.text(f.net)
                {
                    nets.insert(n.to_string());
                }
            }
        }
    }
    s.nets = nets.len();
    s
}

/// A short "what is this" for the file's list of pad shapes.
pub fn shape_table(layer: &Layer) -> Vec<(String, String, u32)> {
    let mut used: Vec<_> = layer
        .data
        .apertures
        .iter()
        .filter(|a| a.used > 0 && !matches!(a.shape, ApShape::Block))
        .collect();
    used.sort_by_key(|a| std::cmp::Reverse(a.used));
    used.iter()
        .map(|a| (format!("D{}", a.code), a.describe().to_string(), a.used))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::samples;
    use std::io::Write;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("copperview-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn zip_bytes(files: &[(String, Vec<u8>)]) -> Vec<u8> {
        let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, bytes) in files {
            let opts = zip::write::SimpleFileOptions::default();
            out.start_file(name.as_str(), opts).unwrap();
            out.write_all(bytes).unwrap();
        }
        out.finish().unwrap().into_inner()
    }

    fn sample_files() -> Vec<(String, Vec<u8>)> {
        samples::board('A').into_iter().map(|(n, t)| (n, t.into_bytes())).collect()
    }

    #[test]
    fn folder_of_altium_files_opens_with_outline() {
        let dir = temp_dir("folder");
        for (name, bytes) in sample_files() {
            std::fs::write(dir.join(name), bytes).unwrap();
        }
        let b = open(&dir, None).unwrap().remove(0);
        assert!(b.layers.iter().any(|l| l.role.label == "Top copper"));
        assert!(b.layers.iter().any(|l| l.role.key == "PTH"));
        assert_eq!(b.layers[b.outline.unwrap()].role.key, "GKO");
        assert_eq!(b.side_files.len(), 1, "{:?}", b.side_files);
        assert!((b.outline_box().w() - samples::BOARD_W).abs() < 0.01);
    }

    #[test]
    fn zip_opens_the_same_as_a_folder() {
        let dir = temp_dir("zip");
        let path = dir.join("Output.zip");
        // Altium's usual shape: gerbers and drills in different subfolders.
        let files: Vec<_> = sample_files()
            .into_iter()
            .map(|(n, b)| (format!("Project Outputs/Gerber/{n}"), b))
            .collect();
        std::fs::write(&path, zip_bytes(&files)).unwrap();
        let b = open(&path, None).unwrap().remove(0);
        assert!(b.is_zip);
        assert_eq!(b.layers.len(), samples::board('A').len() - 1);
    }

    #[test]
    fn zip_inside_a_zip_is_opened() {
        let dir = temp_dir("nested");
        let inner = zip_bytes(&sample_files());
        let path = dir.join("Outer.zip");
        std::fs::write(&path, zip_bytes(&[("rev1/Gerbers.zip".to_string(), inner)])).unwrap();
        let b = open(&path, None).unwrap().remove(0);
        assert!(b.layers.len() >= 5);
    }

    #[test]
    fn folder_holding_only_zips_opens_the_first_and_lists_all() {
        let dir = temp_dir("onlyzips");
        std::fs::write(dir.join("RevA.zip"), zip_bytes(&sample_files())).unwrap();
        std::fs::write(dir.join("RevB.zip"), zip_bytes(&sample_files())).unwrap();
        let boards = open(&dir, None).unwrap();
        assert_eq!(boards.len(), 2, "two zips open as a comparison");
        assert_eq!(boards[0].name, "RevA.zip");
        assert_eq!(boards[1].name, "RevB.zip");
        assert_eq!(boards[0].choices.len(), 2);
    }

    #[test]
    fn two_revisions_inside_one_zip_open_as_a_pair() {
        let dir = temp_dir("pair");
        let mut files = Vec::new();
        for (rev, tag) in [('A', "RevA"), ('B', "RevB")] {
            for (name, text) in samples::board(rev) {
                let sub = if name.ends_with("TXT") { "NC Drill" } else { "Gerber" };
                files.push((format!("Release/{tag}/{sub}/{name}"), text.into_bytes()));
            }
        }
        let path = dir.join("Both.zip");
        std::fs::write(&path, zip_bytes(&files)).unwrap();
        let boards = open(&path, None).unwrap();
        assert_eq!(boards.len(), 2);
        assert_eq!(boards[0].name, "Both.zip / RevA");
        assert_eq!(boards[1].name, "Both.zip / RevB");
        assert!(boards[0].layers.iter().any(|l| l.role.key == "PTH"), "drills stay with their gerbers");
        assert_eq!(boards[0].choices.len(), 2);
        let one = open(&path, Some("RevB")).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!(one[0].name, "Both.zip / RevB");
        let c = crate::compare::run(&boards[0], &boards[1], P::default());
        assert!(!c.changes.is_empty());
    }

    #[test]
    fn altiums_own_subfolders_stay_one_board() {
        let dir = temp_dir("subfolders");
        let path = dir.join("Output.zip");
        let files: Vec<_> = sample_files()
            .into_iter()
            .map(|(n, b)| (format!("{}/{n}", if n.ends_with("TXT") { "NC Drill" } else { "Gerber" }), b))
            .collect();
        std::fs::write(&path, zip_bytes(&files)).unwrap();
        let boards = open(&path, None).unwrap();
        assert_eq!(boards.len(), 1);
        assert!(boards[0].layers.iter().any(|l| l.role.key == "GTL"));
        assert!(boards[0].layers.iter().any(|l| l.role.key == "PTH"));
    }

    #[test]
    fn huge_pours_stay_off_the_cell_lists() {
        let mut text = String::from("%FSLAX26Y26*%\n%MOMM*%\n%ADD10C,0.2*%\nD10*\n");
        for i in 0..2000 {
            text.push_str(&format!("X{}Y0D03*\n", i * 100_000));
        }
        for _ in 0..500 {
            text.push_str("G36*\nX0Y0D02*\nG01*\nX200000000D01*\nY100000000D01*\nX0D01*\nY0D01*\nG37*\n");
        }
        let b = from_memory("big", vec![("x.GP1".to_string(), text)]);
        let grid = &b.layers[0].grid;
        assert_eq!(grid.large.len(), 500);
        assert!(grid.cells.iter().map(Vec::len).sum::<usize>() < 20_000);
        let mut hits = Vec::new();
        pick_layer(&b, 0, p(50.0, 50.0), 0.0, &PickRules::default(), &mut hits);
        assert_eq!(hits.len(), 1, "the newest fill answers");
    }

    #[test]
    fn broken_zip_gives_a_plain_error() {
        let dir = temp_dir("broken");
        let path = dir.join("Bad.zip");
        std::fs::write(&path, b"this is not a zip").unwrap();
        assert!(open(&path, None).is_err());
    }

    #[test]
    fn empty_folder_says_so() {
        let dir = temp_dir("empty");
        assert!(open(&dir, None).err().unwrap().contains("No Gerber"));
    }

    #[test]
    fn clicking_finds_the_pad_and_not_the_cut_out() {
        let b = from_memory(
            "t",
            vec![(
                "x.GTL".to_string(),
                "%FSLAX26Y26*%\n%MOMM*%\nG36*\nX0Y0D02*\nG01*\nX20000000D01*\nY20000000D01*\nX0D01*\nY0D01*\nG37*\n\
                 %ADD10C,4*%\n%LPC*%\nD10*\nX10000000Y10000000D03*\n%LPD*%\n%ADD11C,1*%\nD11*\nX10000000Y10000000D03*\nM02*\n"
                    .to_string(),
            )],
        );
        let at = |q: P| {
            let mut hits = Vec::new();
            pick_layer(&b, 0, q, 0.0, &PickRules::default(), &mut hits);
            hits.first().map(|h| b.layers[0].data.features[h.feature].kind)
        };
        assert_eq!(at(p(10.0, 10.0)), Some(Kind::Pad));
        assert_eq!(at(p(11.5, 10.0)), None, "inside the clearance ring");
        assert_eq!(at(p(15.0, 15.0)), Some(Kind::Region));
    }

    fn stacked() -> Board {
        // A pad drawn first, then a track over it, then a fill over both.
        from_memory(
            "t",
            vec![(
                "x.GTL".to_string(),
                "%FSLAX26Y26*%\n%MOMM*%\n%ADD10C,2*%\n%ADD11C,0.5*%\nD10*\nX10000000Y10000000D03*\n\
                 D11*\nX0Y10000000D02*\nX20000000Y10000000D01*\n\
                 G36*\nX0Y0D02*\nG01*\nX20000000D01*\nY20000000D01*\nX0D01*\nY0D01*\nG37*\nM02*\n"
                    .to_string(),
            )],
        )
    }

    fn kinds(b: &Board, rules: &PickRules) -> Vec<Kind> {
        let mut hits = Vec::new();
        pick_layer(b, 0, p(10.0, 10.0), 0.0, rules, &mut hits);
        hits.iter().map(|h| b.layers[0].data.features[h.feature].kind).collect()
    }

    #[test]
    fn rules_see_through_what_is_turned_off() {
        let b = stacked();
        assert_eq!(kinds(&b, &PickRules::default()), vec![Kind::Region], "a fill hides what's beneath");
        let mut rules = PickRules::default();
        rules.on[3] = false;
        assert_eq!(kinds(&b, &rules), vec![Kind::Track, Kind::Pad]);
        rules.on[1] = false;
        assert_eq!(kinds(&b, &rules), vec![Kind::Pad]);
        rules.on[0] = false;
        assert!(kinds(&b, &rules).is_empty());
    }

    #[test]
    fn asking_for_a_kind_first_reaches_under_a_fill() {
        let b = stacked();
        let rules = PickRules { first: Some(0), ..PickRules::default() };
        assert!(kinds(&b, &rules).contains(&Kind::Pad));
    }

    #[test]
    fn snapping_skips_what_is_turned_off() {
        let b = stacked();
        let mut rules = PickRules::default();
        rules.on[0] = false;
        rules.on[3] = false;
        let (at, _) = snap_points(&b, 0, p(0.2, 10.0), 0.5, &rules).unwrap();
        assert_eq!(at, p(0.0, 10.0));
        rules.on[1] = false;
        assert!(snap_points(&b, 0, p(0.2, 10.0), 0.5, &rules).is_none());
    }

    #[test]
    fn rule_labels_read_plainly() {
        let mut rules = PickRules::default();
        assert_eq!(rules.label(), "Pick all");
        rules.on = [false, false, true, false];
        assert_eq!(rules.label(), "Holes only");
        rules.on = [true, false, true, false];
        assert_eq!(rules.label(), "Pick 2 of 4");
        rules.first = Some(2);
        assert_eq!(rules.label(), "Holes first");
    }

    #[test]
    fn snapping_prefers_the_nearest_pad_centre() {
        let b = from_memory("s", samples::board('A'));
        let copper = b.layers.iter().position(|l| l.role.key == "GTL").unwrap();
        let pad = b.layers[copper].data.features.iter().find(|f| f.kind == Kind::Pad).unwrap().path[0];
        let (c, _) = snap_points(&b, copper, pad.add(p(0.1, 0.05)), 0.5, &PickRules::default()).unwrap();
        assert_eq!(c, pad);
    }
}
