//! What's open and what's selected. The board view itself is in view.rs.

use crate::altium::Group;
use crate::board::{self, Board, Choice, Layer, PickRules, Picked, Summary};
use crate::compare::{self, Comparison, Status};
use crate::geom::*;
use crate::gerber::{Feature, Kind};
use crate::gpu::{Gpu, Mix};
use crate::markup::Markup;
use crate::pinch::Pinch;
use crate::samples;
use crate::theme;
use crate::trace::{self, Trace};
use crate::view3d::View3d;
use eframe::egui::{self, Color32, ColorImage, Key, Panel, Rect, Ui, vec2};
use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    A,
    B,
}

impl Side {
    pub fn letter(self) -> &'static str {
        match self {
            Side::A => "A",
            Side::B => "B",
        }
    }
    pub fn other(self) -> Side {
        match self {
            Side::A => Side::B,
            Side::B => Side::A,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Target {
    pub side: Side,
    pub layer: usize,
    pub feature: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Unit {
    Mm,
    Mil,
    Inch,
}

impl Unit {
    pub fn name(self) -> &'static str {
        match self {
            Unit::Mm => "mm",
            Unit::Mil => "mil",
            Unit::Inch => "in",
        }
    }

    /// A length as bare digits, with sensible precision for the unit.
    pub fn num(self, mm: f64) -> String {
        match self {
            Unit::Mm => trim(format!("{mm:.3}"), 2),
            Unit::Mil => trim(format!("{:.2}", mm / 0.0254), 1),
            Unit::Inch => format!("{:.4}", mm / 25.4),
        }
    }

    pub fn len(self, mm: f64) -> String {
        format!("{} {}", self.num(mm), self.name())
    }

    pub fn area(self, mm2: f64) -> String {
        match self {
            Unit::Mm => format!("{mm2:.2} mm²"),
            Unit::Mil => format!("{:.0} mil²", mm2 / (0.0254 * 0.0254)),
            Unit::Inch => format!("{:.4} in²", mm2 / (25.4 * 25.4)),
        }
    }
}

fn trim(mut text: String, keep: usize) -> String {
    let Some(dot) = text.find('.') else { return text };
    while text.len() > dot + 1 + keep && text.ends_with('0') {
        text.pop();
    }
    text
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ViewMode {
    OnlyA,
    OnlyB,
    /// A on the left, B on the right.
    Side,
    /// A above, B below.
    Stack,
    Diff,
    Swipe,
}

impl ViewMode {
    pub fn is_split(self) -> bool {
        matches!(self, ViewMode::Side | ViewMode::Stack)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Measure,
    Markup,
}

/// One row in the layer list: a layer, in one board or both.
#[derive(Clone)]
pub struct Slot {
    pub key: String,
    pub label: String,
    pub group: Group,
    pub tag: String,
    pub color: [u8; 3],
    pub alpha: f32,
    pub visible: bool,
    pub invert: bool,
    pub copper: bool,
    pub a: Option<usize>,
    pub b: Option<usize>,
}

impl Slot {
    pub fn layer(&self, side: Side) -> Option<usize> {
        match side {
            Side::A => self.a,
            Side::B => self.b,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Pin {
    Feature(Target),
    Ruler(Ruler),
}

#[derive(Clone, Copy, Debug)]
pub struct Ruler {
    pub side: Side,
    pub a: P,
    pub b: P,
    /// Edge-to-edge distance, when both ends landed on features.
    pub gap: Option<f64>,
}

/// Where a measurement sits on screen, so it can be clicked.
#[derive(Clone, Copy, Debug)]
pub struct PinHit {
    pub index: usize,
    pub label: Rect,
    pub line: Option<(egui::Pos2, egui::Pos2)>,
    pub close: Option<Rect>,
}

#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub at: P,
    pub target: Option<Target>,
}

pub const BACKGROUNDS: [(&str, Color32); 4] = [
    ("Black", theme::rgb(0x0b0c0f)),
    ("Navy", theme::rgb(0x0e1a33)),
    ("Board green", theme::rgb(0x0f3a2a)),
    ("Paper", theme::rgb(0xf7f6f2)),
];
pub const PAPER: usize = 3;

#[derive(Default, PartialEq)]
pub struct Launch {
    pub screenshot: Option<PathBuf>,
    pub a: Option<PathBuf>,
    pub b: Option<PathBuf>,
    pub view: Option<String>,
    pub mix: Option<String>,
    pub unit: Option<String>,
    pub pick: Option<P>,
    pub ruler: Option<(P, P)>,
    pub zoom: Option<(P, f64)>,
    pub all_layers: bool,
    pub flip: bool,
    pub no_details: bool,
    pub three_d: bool,
    pub play: Option<String>,
}

pub struct Pending {
    side: Side,
    rx: mpsc::Receiver<Result<Vec<Board>, String>>,
}

pub enum Asked {
    Open(Side, PathBuf),
    Saved(PathBuf),
    PictureDone(String),
    Say(String),
}

pub struct App {
    pub launch: Launch,
    pub gpu: Arc<Mutex<Option<Result<Gpu, String>>>>,
    pub gpu_error: Arc<Mutex<Option<String>>>,

    pub a: Option<Arc<Board>>,
    pub b: Option<Arc<Board>>,
    pub sa: Option<Arc<Summary>>,
    pub sb: Option<Arc<Summary>>,
    pub slots: Vec<Slot>,
    pub active: Option<usize>,
    pub sample: bool,

    pub center: P,
    pub scale: f64,
    pub mirror: bool,
    pub fit_pending: bool,
    pub canvas_rect: Rect,

    pub mode: ViewMode,
    pub tool: Tool,
    pub unit: Unit,
    pub mix: Mix,
    pub master: f32,
    pub bg: usize,
    pub grid: bool,
    pub swipe: f32,
    pub show_left: bool,
    pub show_right: bool,
    pub show_about: bool,
    /// The system menu's About item has been pointed at the About window.
    pub about_hooked: bool,
    /// Whether the details panel is actually showing this frame.
    pub right_open: bool,
    /// The board was fitted and hasn't been moved since, so it follows the room.
    pub fitted: bool,
    pub pinch: Pinch,

    pub hover: Option<Target>,
    pub hover_q: Option<(Side, P)>,
    pub selected: Option<Target>,
    pub show_net: bool,
    pub trace: Option<Trace>,
    /// The trace as it was before the last click, so a double-click can build on it.
    pub prev_trace: Option<Trace>,
    pub trace_side: Side,
    /// Whether differences are marked on the board, like a layer's eye.
    pub show_diffs: bool,
    pub click_memo: Option<(P, usize)>,
    pub pins: Vec<Pin>,
    pub selected_pin: Option<usize>,
    pub hover_pin: Option<usize>,
    pub pin_hits: RefCell<Vec<PinHit>>,
    pub rules: PickRules,
    pub markup: Markup,
    pub markup_open: bool,
    pub view3d: Option<View3d>,
    pub want_flat: bool,
    pub flat_layers: Vec<(String, bool)>,
    /// Drawing the picture to save: no toolbars, handles or hover marks.
    pub clean: bool,
    pub clean_from: u64,
    pub ruler_start: Option<Snap>,
    pub snap: Option<Snap>,
    pub last_ruler: Option<Ruler>,
    pub origin: P,

    pub shift: P,
    pub align: bool,
    pub comparison: Option<Comparison>,
    pub compare_job: Option<mpsc::Receiver<Comparison>>,
    pub change: Option<usize>,
    /// Which marker of the selected change was chosen, when one was clicked.
    pub change_group: Option<usize>,
    /// One difference picked from the list, which blinks on the board.
    pub blink: Option<crate::compare::Item>,
    /// When the current blinking began, so it always starts with A.
    pub blink_epoch: Cell<Option<f64>>,

    pub pending: Vec<Pending>,
    pub asked_tx: mpsc::Sender<Asked>,
    pub asked_rx: mpsc::Receiver<Asked>,
    pub notice: Option<String>,
    pub logo: Option<egui::TextureHandle>,
    pub installed: bool,
    pub choices: Vec<Choice>,
    pub play: Vec<crate::play::Step>,
    pub frames: u32,
    pub shot_sent: bool,
    pub applied_launch: bool,
    pub save_pending: Option<PathBuf>,
}

fn state_file() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("last-opened"))
}

fn remembered() -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(text) = state_file().and_then(|f| std::fs::read_to_string(f).ok()) else { return (None, None) };
    let find = |tag: &str| {
        text.lines().find_map(|l| l.strip_prefix(tag)).map(PathBuf::from).filter(|q| q.exists())
    };
    (find("A="), find("B="))
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> Self {
        theme::install_fonts(&cc.egui_ctx);
        cc.egui_ctx.set_visuals(theme::visuals());
        cc.egui_ctx.all_styles_mut(|s| {
            s.spacing.item_spacing = vec2(8.0, 6.0);
            s.spacing.button_padding = vec2(10.0, 5.0);
            s.spacing.interact_size.y = 24.0;
            s.visuals.handle_shape = egui::style::HandleShape::Circle;
            s.visuals.slider_trailing_fill = true;
            s.text_styles.insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            s.text_styles.insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        });
        let (asked_tx, asked_rx) = mpsc::channel();
        let mut app = App {
            launch,
            gpu: Arc::new(Mutex::new(None)),
            gpu_error: Arc::new(Mutex::new(None)),
            a: None,
            b: None,
            sa: None,
            sb: None,
            slots: Vec::new(),
            active: None,
            sample: false,
            center: P::default(),
            scale: 10.0,
            mirror: false,
            fit_pending: true,
            canvas_rect: Rect::NOTHING,
            mode: ViewMode::OnlyA,
            tool: Tool::Select,
            unit: Unit::Mm,
            mix: Mix::Normal,
            master: 1.0,
            bg: 0,
            grid: false,
            swipe: 0.5,
            show_left: true,
            show_right: true,
            show_about: false,
            about_hooked: false,
            right_open: true,
            fitted: true,
            pinch: Pinch::default(),
            hover: None,
            hover_q: None,
            selected: None,
            show_net: false,
            trace: None,
            prev_trace: None,
            trace_side: Side::A,
            show_diffs: true,
            click_memo: None,
            pins: Vec::new(),
            selected_pin: None,
            hover_pin: None,
            pin_hits: RefCell::new(Vec::new()),
            rules: PickRules::default(),
            markup: Markup::default(),
            markup_open: false,
            view3d: None,
            want_flat: false,
            flat_layers: Vec::new(),
            clean: false,
            clean_from: 0,
            ruler_start: None,
            snap: None,
            last_ruler: None,
            origin: P::default(),
            shift: P::default(),
            align: true,
            comparison: None,
            compare_job: None,
            change: None,
            change_group: None,
            blink: None,
            blink_epoch: Cell::new(None),
            pending: Vec::new(),
            asked_tx,
            asked_rx,
            notice: None,
            logo: crate::icons::picture_pixels(64).map(|(pixels, side)| {
                let side = side as usize;
                let picture = ColorImage::from_rgba_unmultiplied([side, side], &pixels);
                cc.egui_ctx.load_texture("logo", picture, egui::TextureOptions::LINEAR)
            }),
            installed: crate::platform::is_current(),
            choices: Vec::new(),
            play: Vec::new(),
            frames: 0,
            shot_sent: false,
            applied_launch: false,
            save_pending: None,
        };
        app.show_right = !app.launch.no_details;
        app.play = crate::play::parse(app.launch.play.as_deref().unwrap_or(""));
        if let Ok(handle) = cc.display_handle()
            && let RawDisplayHandle::Wayland(wayland) = handle.as_raw()
        {
            app.pinch.listen(wayland.display.as_ptr(), &cc.egui_ctx);
        }
        let (mut first, mut second) = (app.launch.a.take(), app.launch.b.take());
        // A check run always shows the sample, whatever was open last time.
        if first.is_none() && app.launch.screenshot.is_none() {
            (first, second) = remembered();
        }
        match first {
            Some(path) => {
                app.open(Side::A, path);
                if let Some(path) = second {
                    app.open(Side::B, path);
                }
            }
            // With nothing to open, the welcome screen waits for a folder or
            // zip. Only a check run starts on the sample.
            None if app.launch.screenshot.is_some() => app.open_sample(),
            None => {}
        }
        app
    }

    pub fn open_sample(&mut self) {
        let a = board::from_memory("Demo_Board rev A", samples::board('A'));
        let b = board::from_memory("Demo_Board rev B", samples::board('B'));
        self.sample = true;
        self.install(Side::A, a);
        self.install(Side::B, b);
        self.mode = ViewMode::Diff;
    }

    pub fn board(&self, side: Side) -> Option<&Arc<Board>> {
        match side {
            Side::A => self.a.as_ref(),
            Side::B => self.b.as_ref(),
        }
    }

    pub fn loading(&self) -> bool {
        !self.pending.is_empty() || self.compare_job.is_some()
    }

    pub fn open(&mut self, side: Side, path: PathBuf) {
        self.open_set(side, path, None);
    }

    pub fn open_set(&mut self, side: Side, path: PathBuf, set: Option<String>) {
        if self.sample {
            self.close_all();
        }
        let (tx, rx) = mpsc::channel();
        self.pending.retain(|p| p.side != side);
        self.pending.push(Pending { side, rx });
        std::thread::spawn(move || {
            let _ = tx.send(board::open(&path, set.as_deref()));
        });
    }

    fn close_all(&mut self) {
        self.sample = false;
        self.a = None;
        self.b = None;
        self.sa = None;
        self.sb = None;
        self.clear_marks();
        self.rebuild_slots();
    }

    fn clear_marks(&mut self) {
        self.pins.clear();
        self.selected_pin = None;
        self.hover_pin = None;
        self.selected = None;
        self.hover = None;
        self.ruler_start = None;
        self.last_ruler = None;
        self.change = None;
        self.comparison = None;
        self.compare_job = None;
        self.show_net = false;
        self.trace = None;
        self.prev_trace = None;
        self.click_memo = None;
    }

    pub fn close(&mut self, side: Side) {
        match side {
            Side::A => {
                self.a = self.b.take();
                self.sa = self.sb.take();
            }
            Side::B => {
                self.b = None;
                self.sb = None;
            }
        }
        self.sample = false;
        self.clear_marks();
        self.after_change();
        self.remember();
    }

    fn install(&mut self, side: Side, board: Board) {
        let summary = Arc::new(board::summarize(&board));
        let board = Arc::new(board);
        match side {
            Side::A => {
                // Marks belong to the board they were drawn on.
                self.markup.clear();
                self.a = Some(board);
                self.sa = Some(summary);
            }
            Side::B => {
                self.b = Some(board);
                self.sb = Some(summary);
            }
        }
        self.clear_marks();
        self.after_change();
    }

    fn after_change(&mut self) {
        self.rebuild_slots();
        self.mode = match (&self.a, &self.b, self.mode) {
            (Some(_), Some(_), ViewMode::OnlyA | ViewMode::OnlyB) => ViewMode::Diff,
            (_, None, _) => ViewMode::OnlyA,
            (_, _, mode) => mode,
        };
        self.shift = match (&self.a, &self.b) {
            (Some(a), Some(b)) if self.align => compare::auto_shift(a, b),
            _ => P::default(),
        };
        self.start_compare();
        self.fit_pending = true;
    }

    pub fn start_compare(&mut self) {
        self.comparison = None;
        self.change = None;
        self.compare_job = None;
        if let (Some(a), Some(b)) = (self.a.clone(), self.b.clone()) {
            let (tx, rx) = mpsc::channel();
            let shift = self.shift;
            self.compare_job = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(compare::run(&a, &b, shift));
            });
        }
    }

    pub fn set_align(&mut self, on: bool) {
        self.align = on;
        self.shift = match (&self.a, &self.b) {
            (Some(a), Some(b)) if on => compare::auto_shift(a, b),
            _ => P::default(),
        };
        self.start_compare();
    }

    fn rebuild_slots(&mut self) {
        let old = std::mem::take(&mut self.slots);
        let mut rows: Vec<(i32, Slot)> = Vec::new();
        for side in [Side::A, Side::B] {
            let Some(board) = self.board(side).cloned() else { continue };
            for (i, layer) in board.layers.iter().enumerate() {
                let role = &layer.role;
                if let Some((_, slot)) = rows.iter_mut().find(|(_, s)| s.key == role.key) {
                    match side {
                        Side::A => slot.a = Some(i),
                        Side::B => slot.b = Some(i),
                    }
                    continue;
                }
                let remembered = old.iter().find(|s| s.key == role.key);
                let mut slot = Slot {
                    key: role.key.clone(),
                    label: role.label.clone(),
                    group: role.group,
                    tag: file_tag(&layer.file),
                    color: remembered.map_or(role.color, |s| s.color),
                    alpha: remembered.map_or(0.72, |s| s.alpha),
                    visible: remembered.map_or(role.show || board.outline == Some(i), |s| s.visible),
                    invert: remembered.is_some_and(|s| s.invert),
                    copper: role.copper,
                    a: None,
                    b: None,
                };
                match side {
                    Side::A => slot.a = Some(i),
                    Side::B => slot.b = Some(i),
                }
                rows.push((role.order, slot));
            }
        }
        rows.sort_by_key(|(order, _)| *order);
        self.slots = rows.into_iter().map(|(_, s)| s).collect();
        self.active = None;
    }

    /// The size of each view of the board in this much room.
    pub fn pane_size(&self, rect: Rect) -> egui::Vec2 {
        match self.mode {
            ViewMode::Side => vec2(rect.width() / 2.0, rect.height()),
            ViewMode::Stack => vec2(rect.width(), rect.height() / 2.0),
            _ => rect.size(),
        }
    }

    /// Fills the room the board has, stopping just short of the labels at the
    /// top and the toolbar at the bottom.
    pub fn fit(&mut self, rect: Rect) {
        const SIDES: f32 = 28.0;
        const ABOVE: f32 = 48.0;
        const BELOW: f32 = 58.0;
        let mut b = BBox::EMPTY;
        if let Some(board) = &self.a {
            b.union(&board.bbox);
        }
        if let Some(board) = &self.b {
            b.union(&board.bbox.shifted(self.shift));
        }
        if b.is_empty() {
            return;
        }
        let room = self.pane_size(rect);
        let sx = (room.x - SIDES).max(50.0) as f64 / b.w().max(0.1);
        let sy = (room.y - ABOVE - BELOW).max(50.0) as f64 / b.h().max(0.1);
        self.scale = sx.min(sy);
        self.center = b.center();
        self.center.y -= ((BELOW - ABOVE) / 2.0) as f64 / self.scale;
        self.fitted = true;
    }

    pub fn zoom_to(&mut self, b: &BBox, rect: Rect) {
        let pane = self.pane_size(rect);
        let room = (pane.x.min(pane.y) * 0.5) as f64;
        self.scale = (room / b.w().max(b.h()).max(1.0)).clamp(self.scale.min(20.0), 400.0);
        self.center = b.center();
        self.fitted = false;
    }

    /// Selects one difference, as if stepped to with Next.
    pub fn select_change(&mut self, i: usize, group: Option<usize>) {
        let Some(count) = self.comparison.as_ref().map(|c| c.changes.len()) else { return };
        if i < count {
            self.change = Some(i);
            self.change_group = group;
            self.blink = None;
            self.blink_epoch.set(None);
            let key = self.comparison.as_ref().map(|c| c.changes[i].key.clone());
            if let Some(slot) = self.slots.iter_mut().find(|s| Some(&s.key) == key.as_ref()) {
                slot.visible = true;
            }
        }
    }

    pub fn go_to_change(&mut self, step: i32) {
        let Some(count) = self.comparison.as_ref().map(|c| c.changes.len()) else { return };
        if count == 0 {
            return;
        }
        let next = match self.change {
            None if step >= 0 => 0,
            None => count - 1,
            Some(i) => (i as i32 + step).rem_euclid(count as i32) as usize,
        };
        self.change = Some(next);
        self.change_group = None;
        self.blink = None;
        self.blink_epoch.set(None);
        let c = self.comparison.as_ref().map(|c| c.changes[next].clone());
        if let Some(c) = c {
            // Make sure the changed layer can be seen.
            if let Some(slot) = self.slots.iter_mut().find(|s| s.key == c.key) {
                slot.visible = true;
            }
            self.zoom_to(&c.bbox, self.canvas_rect);
        }
    }

    fn remember(&self) {
        let Some(file) = state_file() else { return };
        if self.launch.screenshot.is_some() {
            return;
        }
        let mut text = String::new();
        for (tag, board) in [("A=", &self.a), ("B=", &self.b)] {
            if let Some(board) = board
                && !board.path.as_os_str().is_empty()
            {
                text.push_str(&format!("{tag}{}\n", board.path.display()));
            }
        }
        if let Some(dir) = file.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        // Not being able to remember isn't worth interrupting anyone for.
        let _ = std::fs::write(file, text);
    }

    pub fn ask_folder(&self, side: Side, ctx: &egui::Context) {
        let (tx, ctx) = (self.asked_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            if let Some(path) = rfd::FileDialog::new().set_title("Choose a folder of Gerber files").pick_folder() {
                let _ = tx.send(Asked::Open(side, path));
                ctx.request_repaint();
            }
        });
    }

    pub fn ask_zip(&self, side: Side, ctx: &egui::Context) {
        let (tx, ctx) = (self.asked_tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new().set_title("Choose a zip of Gerber files").add_filter("Zip", &["zip", "ZIP"]);
            if let Some(path) = dialog.pick_file() {
                let _ = tx.send(Asked::Open(side, path));
                ctx.request_repaint();
            }
        });
    }

    pub fn ask_save_picture(&self, ctx: &egui::Context) {
        let (tx, ctx) = (self.asked_tx.clone(), ctx.clone());
        let stem = self.a.as_ref().map_or("board", |b| b.name.as_str());
        let stem: String = stem.chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '-' }).collect();
        let name = format!("{}{}.png", stem.trim_end_matches(".zip"), if self.markup.marks.is_empty() { "" } else { "-markup" });
        std::thread::spawn(move || {
            let dialog = rfd::FileDialog::new()
                .set_title("Save a picture of the view")
                .set_file_name(&name)
                .add_filter("PNG picture", &["png"]);
            if let Some(path) = dialog.save_file() {
                let _ = tx.send(Asked::Saved(path));
                ctx.request_repaint();
            }
        });
    }

    /// Turns Ctrl+C, X and V into what the text box listens for.
    fn clipboard_keys(&self, raw: &mut egui::RawInput) {
        for event in &mut raw.events {
            let egui::Event::Key { key, pressed: true, modifiers, .. } = event else { continue };
            if !modifiers.command {
                continue;
            }
            match key {
                Key::C => *event = egui::Event::Copy,
                Key::X => *event = egui::Event::Cut,
                // Asking the clipboard takes a moment, so only when there's somewhere to put it.
                Key::V if self.markup.editing.is_some() => {
                    if let Some(text) = crate::platform::clipboard_paste() {
                        *event = egui::Event::Paste(text);
                    }
                }
                _ => {}
            }
        }
    }

    fn send_copies(&self, ctx: &egui::Context) {
        let texts: Vec<String> = ctx.output(|o| {
            o.commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(text) => Some(text.clone()),
                    _ => None,
                })
                .collect()
        });
        for text in texts.into_iter().filter(|t| !t.is_empty()) {
            let (tx, wake) = (self.asked_tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                if !crate::platform::clipboard_copy(&text) {
                    let _ = tx.send(Asked::Say(crate::platform::clipboard_missing_hint().to_string()));
                    wake.request_repaint();
                }
            });
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(asked) = self.asked_rx.try_recv() {
            match asked {
                Asked::Open(side, path) => self.open(side, path),
                Asked::Saved(path) => self.save_pending = Some(path),
                Asked::Say(text) => self.notice = Some(text),
                Asked::PictureDone(text) => {
                    self.notice = Some(text);
                    self.clean = false;
                }
            }
        }
        let mut finished = Vec::new();
        self.pending.retain(|job| match job.rx.try_recv() {
            Ok(result) => {
                finished.push((job.side, result));
                false
            }
            Err(mpsc::TryRecvError::Empty) => true,
            Err(mpsc::TryRecvError::Disconnected) => {
                finished.push((job.side, Err("Opening that stopped unexpectedly.".to_string())));
                false
            }
        });
        for (side, result) in finished {
            match result {
                Ok(boards) => {
                    let mut boards = boards.into_iter();
                    let Some(board) = boards.next() else { continue };
                    // A second board loaded with no first one becomes the first.
                    let side = if side == Side::B && self.a.is_none() { Side::A } else { side };
                    self.notice = board.warnings.first().cloned();
                    if !board.choices.is_empty() {
                        self.choices = board.choices.clone();
                    } else if !self.choices.iter().any(|c| c.path == board.path) {
                        self.choices.clear();
                    }
                    self.install(side, board);
                    // A pair found together opens as a comparison.
                    if let Some(second) = boards.next().filter(|_| side == Side::A) {
                        self.install(Side::B, second);
                    }
                    self.remember();
                }
                Err(error) => self.notice = Some(error),
            }
        }
        if let Some(rx) = &self.compare_job
            && let Ok(done) = rx.try_recv()
        {
            self.comparison = Some(done);
            self.compare_job = None;
        }
        if self.loading() {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
    }

    fn take_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        match dropped.as_slice() {
            [] => {}
            [one] => {
                let side = if self.a.is_some() && !self.sample { Side::B } else { Side::A };
                self.open(side, one.clone());
            }
            [first, second, ..] => {
                self.open(Side::A, first.clone());
                self.open(Side::B, second.clone());
            }
        }
    }

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |key: Key| ctx.input(|i| i.key_pressed(key));
        if pressed(Key::Num3) {
            if self.view3d.is_some() {
                self.leave_3d(ctx);
            } else {
                self.enter_3d();
            }
        }
        if self.view3d.is_some() {
            if pressed(Key::Escape) {
                self.leave_3d(ctx);
            }
            if pressed(Key::F)
                && let Some(view) = self.view3d.as_mut()
            {
                view.auto_fit = true;
                self.fit_pending = true;
            }
            return;
        }
        if pressed(Key::Escape) {
            if self.ruler_start.is_some() {
                self.ruler_start = None;
            } else if self.selected.is_some() || self.selected_pin.is_some() {
                self.clear_selection();
            } else if self.change.is_some() {
                self.change = None;
                self.change_group = None;
                self.blink = None;
                self.blink_epoch.set(None);
            } else if self.tool != Tool::Markup || self.markup.selected.is_none() {
                self.tool = Tool::Select;
            }
        }
        if ctx.input(|i| i.modifiers.command) && pressed(Key::Z) {
            self.markup.step_back();
        }
        if pressed(Key::T) {
            self.tool = if self.tool == Tool::Markup { Tool::Select } else { Tool::Markup };
            self.ruler_start = None;
        }
        if self.tool == Tool::Markup && self.markup.selected.is_some() {
            if pressed(Key::Delete) || pressed(Key::Backspace) {
                self.markup.delete_selected();
            }
            if pressed(Key::Escape) {
                self.markup.selected = None;
            }
            return;
        }
        if pressed(Key::Delete) || pressed(Key::Backspace) {
            match self.selected_pin {
                Some(index) => self.delete_pin(index),
                None => self.clear_selection(),
            }
        }
        if pressed(Key::F) {
            self.fit_pending = true;
        }
        if pressed(Key::M) {
            self.tool = if self.tool == Tool::Measure { Tool::Select } else { Tool::Measure };
            self.ruler_start = None;
        }
        if pressed(Key::G) {
            self.grid = !self.grid;
        }
        if pressed(Key::X) {
            self.mirror = !self.mirror;
        }
        if pressed(Key::K) {
            self.keep_selected();
        }
        if pressed(Key::N) {
            self.go_to_change(1);
        }
        if pressed(Key::P) {
            self.go_to_change(-1);
        }
        if pressed(Key::U) {
            self.unit = match self.unit {
                Unit::Mm => Unit::Mil,
                Unit::Mil => Unit::Inch,
                Unit::Inch => Unit::Mm,
            };
        }
    }

    pub fn keep_selected(&mut self) {
        if let Some(t) = self.selected
            && !self.pins.iter().any(|q| matches!(q, Pin::Feature(o) if *o == t))
        {
            self.pins.push(Pin::Feature(t));
            self.selected_pin = Some(self.pins.len() - 1);
        }
    }

    pub fn clear_selection(&mut self) {
        self.trace = None;
        self.prev_trace = None;
        self.selected = None;
        self.selected_pin = None;
        self.show_net = false;
    }

    /// Selects a kept measurement, showing its details on the right.
    pub fn select_pin(&mut self, index: usize) {
        let Some(pin) = self.pins.get(index).cloned() else { return };
        self.selected_pin = Some(index);
        self.show_net = false;
        match pin {
            Pin::Feature(t) => self.selected = Some(t),
            Pin::Ruler(r) => {
                self.selected = None;
                self.last_ruler = Some(r);
            }
        }
    }

    pub fn set_mix(&mut self, mix: Mix) {
        self.mix = mix;
        // Inks need paper to print on, and light needs the dark.
        if mix == Mix::Ink {
            self.bg = PAPER;
        } else if self.bg == PAPER {
            self.bg = 0;
        }
    }

    pub fn preset(&mut self, name: &str) {
        match name {
            "All" => self.markup.visible = true,
            "None" => self.markup.visible = false,
            _ => {}
        }
        for slot in &mut self.slots {
            slot.visible = match name {
                "All" => true,
                "None" => false,
                "Copper" => slot.copper || slot.group == Group::Drill,
                "Top" => matches!(slot.group, Group::Top | Group::Drill),
                "Bottom" => matches!(slot.group, Group::Bottom | Group::Drill),
                _ => slot.visible,
            };
        }
    }

    /// Selects the whole trace the piece of copper belongs to: just its own
    /// layer, or on through the plated holes into the other layers.
    pub fn select_trace(&mut self, t: Target, through: bool) {
        let Some(board) = self.board(t.side) else { return };
        let Some(trace) = trace::follow(board, t.layer, t.feature, through) else { return };
        if through {
            let both = trace.layers.iter().chain(trace.partner.iter().flat_map(|p| p.layers.iter()));
            for tl in both.filter(|tl| tl.tracks > 0) {
                for slot in self.slots.iter_mut().filter(|s| s.layer(t.side) == Some(tl.layer)) {
                    slot.visible = true;
                }
            }
        }
        self.trace_side = t.side;
        self.selected = Some(t);
        self.trace = Some(trace);
        self.show_net = false;
    }

    pub fn feature<'a>(&'a self, t: &Target) -> Option<(&'a Layer, &'a Feature)> {
        let layer = self.board(t.side)?.layers.get(t.layer)?;
        let f = layer.data.features.get(t.feature)?;
        Some((layer, f))
    }

    pub fn pick(&self, side: Side, q: P, tol: f64) -> Vec<Target> {
        let Some(board) = self.board(side) else { return Vec::new() };
        let mut hits: Vec<Picked> = Vec::new();
        for slot in self.slots.iter().filter(|s| s.visible) {
            if let Some(layer) = slot.layer(side) {
                board::pick_layer(board, layer, q, tol, &self.rules, &mut hits);
            }
        }
        // Things you can aim at beat the big fills around them.
        // The layer chosen in the list comes first.
        let chosen = self.active.and_then(|i| self.slots.get(i)).and_then(|s| s.layer(side));
        let rank = |h: &Picked| {
            let f = &board.layers[h.layer].data.features[h.feature];
            let wanted = self.rules.first.is_some_and(|i| board::pick_group(f.kind) == i);
            // Holes first, then traces, then pads, then fills.
            let kind = match f.kind {
                Kind::Hole | Kind::Slot => 0,
                Kind::Track | Kind::Arc => 1,
                Kind::Pad => 2,
                Kind::Region => 3,
            };
            (self.rules.first.is_some() && !wanted, h.near, kind, chosen.is_some() && chosen != Some(h.layer))
        };
        hits.sort_by_key(rank);
        hits.into_iter().map(|h| Target { side, layer: h.layer, feature: h.feature }).collect()
    }

    /// What's at this same spot on the other board: None when that board
    /// has no such layer, Some(None) when the layer is empty there.
    pub fn counterpart(&self, t: &Target, q: P) -> Option<Option<Target>> {
        let other = t.side.other();
        let (layer, _) = self.feature(t)?;
        let slot = self.slots.iter().find(|s| s.key == layer.role.key)?;
        let board = self.board(other)?;
        let index = slot.layer(other)?;
        let shift = match t.side {
            Side::A => self.shift.scale(-1.0),
            Side::B => self.shift,
        };
        let mut hits = Vec::new();
        board::pick_layer(board, index, q.add(shift), 0.0, &PickRules::default(), &mut hits);
        Some(hits.first().map(|h| Target { side: other, layer: h.layer, feature: h.feature }))
    }

    pub fn snap_at(&self, side: Side, q: P, tol: f64) -> Snap {
        let mut best: Option<(f64, Snap)> = None;
        if let Some(board) = self.board(side) {
            for slot in self.slots.iter().filter(|s| s.visible) {
                let Some(layer) = slot.layer(side) else { continue };
                if let Some((at, feature)) = board::snap_points(board, layer, q, tol, &self.rules) {
                    let d = at.dist(q);
                    if best.as_ref().is_none_or(|b| d < b.0) {
                        best = Some((d, Snap { at, target: Some(Target { side, layer, feature }) }));
                    }
                }
            }
        }
        best.map_or(Snap { at: q, target: None }, |b| b.1)
    }

    fn edge_gap(&self, a: Target, b: Target) -> Option<f64> {
        let (la, fa) = self.feature(&a)?;
        let (lb, fb) = self.feature(&b)?;
        if a == b {
            return None;
        }
        let (ra, rb) = (fa.outline(&la.data.apertures), fb.outline(&lb.data.apertures));
        let total: usize = ra.iter().map(Vec::len).sum::<usize>() * rb.iter().map(Vec::len).sum::<usize>();
        if total > 40_000_000 {
            return None;
        }
        let mut best = f64::INFINITY;
        for x in &ra {
            for y in &rb {
                best = best.min(ring_gap(x, y));
            }
        }
        best.is_finite().then_some(best)
    }

    pub fn finish_ruler(&mut self, side: Side, a: Snap, b: Snap) {
        let gap = match (a.target, b.target) {
            (Some(x), Some(y)) => self.edge_gap(x, y),
            _ => None,
        };
        let ruler = Ruler { side, a: a.at, b: b.at, gap };
        self.pins.push(Pin::Ruler(ruler));
        self.last_ruler = Some(ruler);
    }

    pub fn delete_pin(&mut self, index: usize) {
        if index >= self.pins.len() {
            return;
        }
        let pin = self.pins.remove(index);
        self.hover_pin = None;
        match self.selected_pin {
            Some(i) if i == index => {
                self.selected_pin = None;
                if matches!(pin, Pin::Feature(_)) {
                    self.selected = None;
                }
            }
            Some(i) if i > index => self.selected_pin = Some(i - 1),
            _ => {}
        }
        if let (Pin::Ruler(gone), Some(shown)) = (&pin, self.last_ruler)
            && gone.a == shown.a
            && gone.b == shown.b
        {
            self.last_ruler = None;
        }
    }

    pub fn clear_pins(&mut self) {
        self.pins.clear();
        self.selected_pin = None;
        self.hover_pin = None;
        self.last_ruler = None;
    }

    /// Where a feature is, measured from the origin: its centre, or both ends of a line.
    pub fn coord_text(&self, f: &Feature) -> String {
        let u = self.unit;
        let at = |q: P| {
            let q = q.sub(self.origin);
            format!("X {}  Y {}", u.num(q.x), u.num(q.y))
        };
        match f.kind {
            Kind::Pad | Kind::Hole => format!("{} {}", at(f.path[0]), u.name()),
            Kind::Region => format!("{} {}", at(f.bbox.center()), u.name()),
            Kind::Track | Kind::Slot | Kind::Arc => {
                format!("{}  to  {} {}", at(f.path[0]), at(f.path[f.path.len() - 1]), u.name())
            }
        }
    }

    pub fn size_text(&self, layer: &Layer, f: &Feature) -> String {
        let u = self.unit;
        match f.kind {
            Kind::Pad => {
                let a = layer.data.apertures.get(f.ap as usize);
                let (w, h) = a.map_or((f.width, f.width), |a| (a.w, a.h));
                if (w - h).abs() < 1e-6 {
                    format!("Ø {}", u.len(w))
                } else {
                    format!("{} × {}", u.num(w), u.len(h))
                }
            }
            Kind::Hole => format!("Ø {}", u.len(f.width)),
            Kind::Slot => format!("Ø {}  L {}", u.num(f.width), u.len(f.length())),
            Kind::Track => format!("W {}  L {}", u.num(f.width), u.len(f.length())),
            Kind::Arc => {
                let r = f.arc.map_or(0.0, |a| a.radius);
                format!("W {}  R {}  L {}", u.num(f.width), u.num(r), u.len(f.length()))
            }
            Kind::Region => format!("{} × {}", u.num(f.bbox.w()), u.len(f.bbox.h())),
        }
    }
}

fn file_tag(file: &str) -> String {
    let name = file.rsplit('/').next().unwrap_or(file);
    match name.rsplit_once('.') {
        Some((_, ext)) => ext.to_uppercase(),
        None => String::new(),
    }
}

pub fn save_png(picture: &ColorImage, crop: Option<[usize; 4]>, path: &Path) -> Result<(), String> {
    let [full_w, full_h] = picture.size;
    let [x0, y0, x1, y1] = crop.unwrap_or([0, 0, full_w, full_h]);
    let (x1, y1) = (x1.min(full_w), y1.min(full_h));
    let (w, h) = (x1.saturating_sub(x0), y1.saturating_sub(y0));
    if w == 0 || h == 0 {
        return Err("There was nothing to save.".into());
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in y0..y1 {
        for px in &picture.pixels[y * full_w + x0..y * full_w + x1] {
            let [r, g, b, _] = px.to_srgba_unmultiplied();
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&rgba).map_err(|e| e.to_string())
}

/// How one row in the list stands between the two boards.
pub fn slot_status(app: &App, slot: &Slot) -> Option<Status> {
    app.comparison.as_ref()?.statuses.get(&slot.key).copied()
}

impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.pinch.stop();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.play_step(raw_input);
        self.clipboard_keys(raw_input);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll(&ctx);
        self.prepare_clean_frame(&ctx);
        self.take_dropped(&ctx);
        self.keys(&ctx);
        self.drive_save(&ctx);
        self.drive_screenshot(&ctx);
        if std::mem::take(&mut self.want_flat) && self.view3d.is_some() {
            self.finish_leaving_3d();
        }
        let width = ctx.content_rect().width();
        let top_frame = egui::Frame::new().fill(theme::WELL).inner_margin(egui::Margin::symmetric(12, 7));
        Panel::top("top").resizable(false).frame(top_frame).show(ui, |ui| self.top_bar(ui));
        if self.show_left && width >= 470.0 {
            let frame = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(10, 10));
            Panel::left("layers").resizable(false).exact_size(286.0).frame(frame).show(ui, |ui| self.left_panel(ui));
        } else {
            let frame = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(6, 8));
            Panel::left("rail").resizable(false).exact_size(64.0).frame(frame).show(ui, |ui| self.left_rail(ui));
        }
        self.right_open = self.show_right && width >= 860.0;
        if self.right_open {
            let frame = egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(12, 12));
            Panel::right("details").resizable(false).exact_size(292.0).frame(frame).show(ui, |ui| self.right_panel(ui));
        }
        let status_frame = egui::Frame::new().fill(theme::WELL).inner_margin(egui::Margin::symmetric(12, 5));
        Panel::bottom("status").resizable(false).frame(status_frame).show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::WELL)).show(ui, |ui| self.canvas(ui));
        if !self.about_hooked {
            self.about_hooked = crate::platform::hook_about_menu(&ctx);
        }
        if crate::platform::about_requested() {
            self.show_about = true;
        }
        self.about_window(&ctx);
        self.send_copies(&ctx);
    }
}
