//! The board view: drawing, zooming, clicking, and the measurements on top.

use crate::app::*;
use crate::geom::*;
use crate::gerber::Kind;
use crate::gpu::{self, Item, Xf};
use crate::icons::{self, Icon};
use crate::panels::{icon_button, segmented_tool, tool_button};
use crate::theme::{self, ACCENT, DIM, FAINT, FG, LINE, RAISED, semibold};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, Painter, PointerButton, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui, pos2, vec2,
};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

const SHOT_FRAMES: u32 = 120;
/// Scripted input starts once the picture has settled.
pub const PLAY_FROM: u32 = 20;
const CLEAN_FRAMES_MOST: u64 = 30;

/// Maps a board onto a piece of the screen.
#[derive(Clone, Copy)]
pub struct Tr {
    pub rect: Rect,
    pub c: P,
    pub s: f64,
    pub m: f64,
    pub shift: P,
}

impl Tr {
    pub fn pt(&self, q: P) -> Pos2 {
        let w = q.add(self.shift);
        let mid = self.rect.center();
        pos2(mid.x + ((w.x - self.c.x) * self.s * self.m) as f32, mid.y - ((w.y - self.c.y) * self.s) as f32)
    }

    pub fn board(&self, at: Pos2) -> P {
        let mid = self.rect.center();
        p(
            (at.x - mid.x) as f64 / (self.s * self.m) + self.c.x - self.shift.x,
            -(at.y - mid.y) as f64 / self.s + self.c.y - self.shift.y,
        )
    }

    pub fn screen_box(&self, b: &BBox) -> Rect {
        Rect::from_two_pos(self.pt(b.min), self.pt(b.max))
    }

    fn xf(&self) -> Xf {
        let sx = 2.0 * self.s * self.m / self.rect.width() as f64;
        let sy = 2.0 * self.s / self.rect.height() as f64;
        Xf {
            sx: sx as f32,
            sy: sy as f32,
            tx: ((self.shift.x - self.c.x) * sx) as f32,
            ty: ((self.shift.y - self.c.y) * sy) as f32,
        }
    }
}

/// How fast differences blink: this many on-off cycles a second.
const BLINK_HZ: f64 = 1.0;

/// The other half of a differential pair.
const PAIR_CYAN: Color32 = Color32::from_rgb(0x00, 0xe5, 0xff);

/// A bright yellow that shows up on dark, paper, copper and mask alike.
const CHANGE_YELLOW: Color32 = Color32::from_rgb(0xff, 0xe6, 0x00);

fn nice_step(want: f64) -> f64 {
    let base = 10f64.powf(want.log10().floor());
    [1.0, 2.0, 5.0, 10.0].iter().map(|k| k * base).find(|v| *v >= want).unwrap_or(base)
}

/// The little remove button beside a selected measurement.
fn close_mark(painter: &Painter, label: Rect, pane: Rect) -> Rect {
    let mut mark = Rect::from_center_size(label.right_center() + vec2(13.0, 0.0), egui::Vec2::splat(20.0));
    if mark.right() > pane.right() {
        mark = Rect::from_center_size(label.left_center() - vec2(13.0, 0.0), egui::Vec2::splat(20.0));
    }
    painter.circle(mark.center(), 10.0, theme::RED, Stroke::new(1.0, Color32::from_black_alpha(90)));
    icons::draw(painter, mark.shrink(5.0), Icon::Close, Color32::WHITE);
    mark
}

impl App {
    fn items(&self, side: Side, tr: &Tr, clip: Option<(f32, f32)>) -> Vec<Item> {
        let Some(board) = self.board(side) else { return Vec::new() };
        let xf = tr.xf();
        self.slots
            .iter()
            .rev()
            .filter(|s| s.visible)
            .filter_map(|slot| {
                let layer = board.layers.get(slot.layer(side)?)?.clone();
                Some(Item {
                    layer,
                    xf,
                    color: slot.color.map(|c| c as f32 / 255.0),
                    alpha: slot.alpha * self.master,
                    invert: slot.invert,
                    other: None,
                    clip,
                })
            })
            .collect()
    }

    fn diff_items(&self, ta: &Tr, tb: &Tr) -> Vec<Item> {
        let (Some(a), Some(b)) = (&self.a, &self.b) else { return Vec::new() };
        let (xa, xb) = (ta.xf(), tb.xf());
        let only = |c: Color32| [c.r(), c.g(), c.b()].map(|v| v as f32 / 255.0);
        self.slots
            .iter()
            .rev()
            .filter(|s| s.visible)
            .filter_map(|slot| {
                let color = slot.color.map(|c| c as f32 / 255.0);
                let alpha = slot.alpha * self.master;
                let (layer, xf, other, color) = match (slot.a, slot.b) {
                    (Some(i), Some(j)) => (a.layers.get(i)?, xa, Some((b.layers.get(j)?.clone(), xb)), color),
                    (Some(i), None) => (a.layers.get(i)?, xa, None, only(theme::RED)),
                    (None, Some(j)) => (b.layers.get(j)?, xb, None, only(theme::GREEN)),
                    (None, None) => return None,
                };
                Some(Item { layer: layer.clone(), xf, color, alpha, invert: slot.invert, other, clip: None })
            })
            .collect()
    }

    fn keep_ids(&self) -> Arc<HashSet<u64>> {
        let mut ids = HashSet::new();
        for board in [&self.a, &self.b].into_iter().flatten() {
            ids.extend(board.layers.iter().map(|l| l.id));
        }
        Arc::new(ids)
    }

    /// The view with no board's own shift: what marks are pinned to.
    pub fn view_tr(&self, rect: Rect) -> Tr {
        self.tr(rect, Side::A)
    }

    fn tr(&self, rect: Rect, side: Side) -> Tr {
        Tr {
            rect,
            c: self.center,
            s: self.scale,
            m: if self.mirror { -1.0 } else { 1.0 },
            shift: if side == Side::B { self.shift } else { P::default() },
        }
    }

    pub fn canvas(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        // A fitted board keeps filling the room when panels fold or the window resizes.
        if self.fitted && (full.size() - self.canvas_rect.size()).length() > 0.5 {
            self.fit_pending = true;
        }
        self.canvas_rect = full;
        let bg = BACKGROUNDS[self.bg].1;
        ui.painter().rect_filled(full, 0.0, bg);
        if self.a.is_none() {
            self.welcome(ui, full);
            return;
        }
        if self.view3d.is_some() {
            self.canvas_3d(ui, full);
            return;
        }
        if self.fit_pending {
            self.fit(full);
            self.fit_pending = false;
        }
        self.apply_launch(full);

        let panes: Vec<(Rect, Side)> = match self.mode {
            ViewMode::Side if self.b.is_some() => {
                let w = (full.width() - 2.0) / 2.0;
                vec![
                    (Rect::from_min_size(full.min, vec2(w, full.height())), Side::A),
                    (Rect::from_min_size(full.min + vec2(w + 2.0, 0.0), vec2(w, full.height())), Side::B),
                ]
            }
            ViewMode::Stack if self.b.is_some() => {
                let h = (full.height() - 2.0) / 2.0;
                vec![
                    (Rect::from_min_size(full.min, vec2(full.width(), h)), Side::A),
                    (Rect::from_min_size(full.min + vec2(0.0, h + 2.0), vec2(full.width(), h)), Side::B),
                ]
            }
            ViewMode::OnlyB if self.b.is_some() => vec![(full, Side::B)],
            _ => vec![(full, Side::A)],
        };

        // Zoom and pan act on the shared view, whichever pane the pointer is in.
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let mut over: Option<Rect> = None;
        self.hover = None;
        self.hover_pin = None;
        self.hover_q = None;
        self.snap = None;

        for &(rect, side) in &panes {
            let tr = self.tr(rect, side);
            let response = ui.interact(rect, ui.scope_id().with(("pane", side.letter())), Sense::click_and_drag());
            if response.hovered() {
                over = Some(rect);
            }
            self.paint_layers(ui, rect, side);
            // While drawing, the left button belongs to the pen.
            let moves = response.dragged_by(PointerButton::Middle)
                || response.dragged_by(PointerButton::Secondary)
                || (self.tool != Tool::Markup && response.dragged_by(PointerButton::Primary));
            if moves {
                let d = response.drag_delta();
                self.center.x -= d.x as f64 / (self.scale * tr.m);
                self.center.y += d.y as f64 / self.scale;
                self.fitted = false;
                ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
            }
            if response.hovered() {
                self.interact_pane(ui, &response, rect, side);
            }
            if self.tool == Tool::Markup {
                self.markup_input(ui, &response, rect);
            }
        }

        // A pinch counts wherever it lands on the view; away from it, it's dropped.
        let (pinched, moved) = self.pinch.take();
        if let Some(rect) = over
            && let Some(at) = pointer
        {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta().y, i.zoom_delta()));
            let factor = (zoom * pinched) as f64 * (scroll as f64 * 0.003).exp();
            if (factor - 1.0).abs() > 1e-4 {
                let before = self.tr(rect, Side::A).board(at);
                self.scale = (self.scale * factor).clamp(0.2, 4000.0);
                let after = self.tr(rect, Side::A).board(at);
                self.center = self.center.add(before.sub(after));
                self.fitted = false;
            }
            // Two fingers sliding together during a pinch carry the board along.
            if moved != [0.0, 0.0] {
                self.center.x -= moved[0] as f64 / (self.scale * self.tr(rect, Side::A).m);
                self.center.y += moved[1] as f64 / self.scale;
                self.fitted = false;
            }
        }

        self.pin_hits.borrow_mut().clear();
        self.markup.word_boxes.borrow_mut().clear();
        for &(rect, side) in &panes {
            let painter = ui.painter_at(rect);
            self.overlay(&painter, rect, side);
        }
        if self.mode == ViewMode::Swipe {
            self.swipe_handle(ui, full);
        }
        if panes.len() == 2 {
            let second = panes[1].0;
            if self.mode == ViewMode::Side {
                ui.painter().vline(second.left() - 1.0, full.y_range(), Stroke::new(2.0, LINE));
            } else {
                ui.painter().hline(full.x_range(), second.top() - 1.0, Stroke::new(2.0, LINE));
            }
        }
        if !self.clean {
            self.floating_tools(ui, full);
            if self.tool == Tool::Markup {
                self.markup_bar(ui, full);
            }
            self.mark_editor(ui);
        }
        if let Some(error) = self.gpu_error.lock().ok().and_then(|e| e.clone()) {
            self.notice = Some(format!("Drawing problem: {error}"));
        }
    }

    fn paint_layers(&self, ui: &Ui, rect: Rect, side: Side) {
        let keep = self.keep_ids();
        let (ta, tb) = (self.tr(rect, Side::A), self.tr(rect, Side::B));
        let items = match self.mode {
            ViewMode::Diff if self.b.is_some() => self.diff_items(&ta, &tb),
            ViewMode::Swipe if self.b.is_some() => {
                let mut list = self.items(Side::A, &ta, Some((0.0, self.swipe)));
                list.extend(self.items(Side::B, &tb, Some((self.swipe, 1.0))));
                list
            }
            _ => self.items(side, &self.tr(rect, side), None),
        };
        ui.painter().add(gpu::callback(rect, self.gpu.clone(), items, self.mix, keep, self.gpu_error.clone()));
    }

    /// Which board a point in this pane belongs to.
    fn side_at(&self, side: Side, x: f32, rect: Rect) -> Side {
        match self.mode {
            ViewMode::Swipe if x > rect.left() + rect.width() * self.swipe => Side::B,
            _ => side,
        }
    }

    fn interact_pane(&mut self, ui: &mut Ui, response: &egui::Response, rect: Rect, side: Side) {
        let Some(at) = response.hover_pos() else { return };
        let side = self.side_at(side, at.x, rect);
        let q = self.tr(rect, side).board(at);
        self.hover_q = Some((side, q));
        let tol = 5.0 / self.scale;
        match self.tool {
            Tool::Select => {
                // Measurements sit on top of the board, so they're reached first.
                if let Some((index, on_close)) = self.pin_at(at) {
                    self.hover_pin = Some(index);
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    if response.clicked() {
                        if on_close {
                            self.delete_pin(index);
                        } else {
                            self.select_pin(index);
                        }
                    }
                    return;
                }
                // A click away from the lit-up differences ends the difference view.
                if self.change.is_some() && response.clicked() && !self.diff_item_at(rect, at) {
                    self.change = None;
                    self.change_group = None;
                    self.blink = None;
                    self.blink_epoch.set(None);
                    return;
                }
                if let Some((i, g)) = self.change_pin_at(rect, at) {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                    if response.clicked() {
                        self.select_change(i, Some(g));
                    }
                    return;
                }
                let mut hits = self.pick(side, q, tol);
                let mut spot = q;
                if hits.is_empty() && self.mode == ViewMode::Diff {
                    spot = self.tr(rect, Side::B).board(at);
                    hits = self.pick(Side::B, spot, tol);
                }
                self.hover = hits.first().copied();
                if self.hover.is_some() {
                    ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                }
                if response.double_clicked() {
                    // A second double-click on a lit-up trace follows it on through the holes.
                    let known = self.trace.take().or(self.prev_trace.take());
                    let inside = known.as_ref().and_then(|tc| hits.iter().find(|h| h.side == self.trace_side && tc.contains(h.layer, h.feature)).copied());
                    if let Some(t) = inside {
                        if known.as_ref().is_some_and(|tc| tc.through) {
                            self.selected = Some(t);
                            self.trace = known;
                        } else {
                            self.select_trace(t, true);
                        }
                    } else if let Some(t) = self.selected.filter(|t| hits.contains(t)).or(hits.first().copied()) {
                        self.select_trace(t, false);
                    }
                } else if response.clicked() {
                    self.select_from(&hits, spot);
                }
                if response.secondary_clicked() {
                    self.clear_selection();
                }
            }
            Tool::Markup => {}
            Tool::Measure => {
                ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                let mut snap = self.snap_at(side, q, 10.0 / self.scale);
                if let Some(start) = self.ruler_start
                    && ui.input(|i| i.modifiers.shift)
                {
                    let d = snap.at.sub(start.at);
                    snap.at = if d.x.abs() > d.y.abs() { p(snap.at.x, start.at.y) } else { p(start.at.x, snap.at.y) };
                    snap.target = None;
                }
                self.snap = Some(snap);
                if response.clicked() {
                    match self.ruler_start.take() {
                        None => self.ruler_start = Some(snap),
                        Some(start) => self.finish_ruler(side, start, snap),
                    }
                }
                if response.secondary_clicked() {
                    self.ruler_start = None;
                }
            }
        }
    }

    /// Whether the pointer is on one of the differences lit up for the selected change.
    fn diff_item_at(&self, rect: Rect, at: Pos2) -> bool {
        let (Some(i), Some(cmp)) = (self.change, &self.comparison) else { return false };
        let Some(c) = cmp.changes.get(i) else { return false };
        let items = match self.change_group.and_then(|g| c.groups.get(g)) {
            Some(group) => &group.items,
            None => &c.items,
        };
        let tol = 5.0 / self.scale;
        items.iter().take(2000).any(|item| {
            let side = if item.added { Side::B } else { Side::A };
            let t = Target { side, layer: item.layer, feature: item.feature };
            let Some((layer, f)) = self.feature(&t) else { return false };
            let q = self.tr(rect, side).board(at);
            f.hit(q, tol, &layer.data.apertures) != crate::gerber::Hit::Miss
        })
    }

    /// The marker under the pointer: which change, and which group of connected differences in it.
    fn change_pin_at(&self, rect: Rect, at: Pos2) -> Option<(usize, usize)> {
        // Markers are hidden while a difference is selected.
        let cmp = self.comparison.as_ref().filter(|_| self.show_diffs && self.change.is_none())?;
        let tr = self.tr(rect, Side::A);
        cmp.changes.iter().enumerate().rev().find_map(|(i, c)| {
            c.groups.iter().position(|g| {
                let (tip, head, r) = self.pin_shape(&tr, &g.at, false);
                (at - head).length() <= r + 3.0 || dist_to_segment(p(at.x as f64, at.y as f64), p(tip.x as f64, tip.y as f64), p(head.x as f64, head.y as f64)) <= 3.0
            }).map(|g| (i, g))
        })
    }

    /// The measurement at this spot, and whether that's its remove button.
    fn pin_at(&self, at: Pos2) -> Option<(usize, bool)> {
        for hit in self.pin_hits.borrow().iter().rev() {
            if hit.close.is_some_and(|r| r.contains(at)) {
                return Some((hit.index, true));
            }
            let on_line = hit.line.is_some_and(|(a, b)| {
                let (a, b, q) = (p(a.x as f64, a.y as f64), p(b.x as f64, b.y as f64), p(at.x as f64, at.y as f64));
                dist_to_segment(q, a, b) <= 5.0
            });
            if on_line || hit.label.expand(2.0).contains(at) {
                return Some((hit.index, false));
            }
        }
        None
    }

    pub fn select_from(&mut self, hits: &[Target], q: P) {
        if hits.is_empty() {
            self.clear_selection();
            self.click_memo = None;
            return;
        }
        // Clicking the same spot again steps through what's stacked there.
        let index = match self.click_memo {
            Some((at, i)) if at.dist(q) * self.scale < 3.0 => (i + 1) % hits.len(),
            _ => 0,
        };
        self.click_memo = Some((q, index));
        self.prev_trace = self.trace.take();
        self.selected = Some(hits[index]);
        self.selected_pin = self.pins.iter().position(|q| matches!(q, Pin::Feature(t) if *t == hits[index]));
        self.show_net = false;
    }

    fn swipe_handle(&mut self, ui: &mut Ui, full: Rect) {
        let x = full.left() + full.width() * self.swipe;
        let grab = Rect::from_center_size(pos2(x, full.center().y), vec2(22.0, full.height()));
        let response = ui.interact(grab, ui.scope_id().with("swipe"), Sense::drag());
        if response.dragged() {
            self.swipe = ((x + response.drag_delta().x - full.left()) / full.width()).clamp(0.02, 0.98);
        }
        if response.hovered() || response.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        let p = ui.painter();
        p.vline(x, full.y_range(), Stroke::new(1.5, FG));
        let knob = Rect::from_center_size(pos2(x, full.center().y), vec2(28.0, 28.0));
        p.rect_filled(knob, 14.0, RAISED);
        p.rect_stroke(knob, 14.0, Stroke::new(1.0, LINE), StrokeKind::Inside);
        icons::draw(p, Rect::from_center_size(knob.center() - vec2(5.0, 0.0), vec2(12.0, 12.0)), Icon::Left, FG);
        icons::draw(p, Rect::from_center_size(knob.center() + vec2(5.0, 0.0), vec2(12.0, 12.0)), Icon::Right, FG);
        let ink = if self.bg == PAPER { Color32::BLACK } else { FG };
        p.text(pos2(x - 26.0, full.center().y), Align2::RIGHT_CENTER, "A", semibold(13.0), ink);
        p.text(pos2(x + 26.0, full.center().y), Align2::LEFT_CENTER, "B", semibold(13.0), ink);
    }

    fn overlay(&self, painter: &Painter, rect: Rect, side: Side) {
        let tr = self.tr(rect, side);
        if self.grid {
            self.draw_grid(painter, &tr);
        }
        self.draw_change_focus(painter, rect);
        let shown_here = |t: &Target| match self.mode {
            ViewMode::Side | ViewMode::Stack | ViewMode::OnlyA | ViewMode::OnlyB => t.side == side,
            _ => true,
        };
        let hi = if self.bg == PAPER { Color32::BLACK } else { Color32::WHITE };

        if let Some(t) = self.selected.filter(|_| self.show_net)
            && let Some((layer, f)) = self.feature(&t)
            && let Some(name) = layer.data.text(f.net)
        {
            self.draw_net(painter, &tr, name, side);
        }
        if let Some(t) = self.hover.filter(|t| shown_here(t) && !self.clean)
            && self.selected != Some(t)
        {
            self.draw_outline(painter, &t, rect, Stroke::new(1.2, ACCENT));
        }
        for (index, pin) in self.pins.iter().enumerate() {
            let chosen = self.selected_pin == Some(index);
            let mark = if chosen {
                Some(FG)
            } else if self.hover_pin == Some(index) {
                Some(ACCENT)
            } else {
                None
            };
            let (label, line) = match pin {
                Pin::Feature(t) if shown_here(t) => {
                    self.draw_outline(painter, t, rect, Stroke::new(1.2, hi));
                    (self.draw_measure(painter, t, rect, mark), None)
                }
                Pin::Ruler(r) if r.side == side || !self.mode.is_split() => {
                    let tr = self.tr(rect, r.side);
                    (Some(self.draw_ruler(painter, &tr, r, mark)), Some((tr.pt(r.a), tr.pt(r.b))))
                }
                _ => (None, None),
            };
            if let Some(label) = label {
                let close = (chosen && !self.clean).then(|| close_mark(painter, label, rect));
                self.pin_hits.borrow_mut().push(PinHit { index, label, line, close });
            }
        }
        if let Some(t) = self.selected.filter(|t| shown_here(t)) {
            self.draw_outline(painter, &t, rect, Stroke::new(3.5, ACCENT.gamma_multiply(0.45)));
            self.draw_outline(painter, &t, rect, Stroke::new(1.4, hi));
            if !self.pins.iter().any(|q| matches!(q, Pin::Feature(o) if *o == t)) {
                self.draw_measure(painter, &t, rect, None);
            }
        }

        if let Some(tc) = self.trace.as_ref().filter(|_| self.selected.is_some_and(|t| shown_here(&t))) {
            let side = self.trace_side;
            let shown = |layer: usize| self.slots.iter().any(|s| s.visible && s.layer(side) == Some(layer));
            // The pair partner gets its own colour, so the two can be told apart.
            for (one, color) in [(Some(tc), CHANGE_YELLOW), (tc.partner.as_deref(), PAIR_CYAN)] {
                let Some(one) = one else { continue };
                let lit = one.layers.iter().filter(|l| shown(l.layer)).flat_map(|l| l.features.iter().map(move |f| (l.layer, *f)));
                for (layer, feature) in lit.chain(one.holes.iter().copied().filter(|(l, _)| shown(*l))) {
                    let t = Target { side, layer, feature };
                    self.draw_outline(painter, &t, rect, Stroke::new(3.5, color.gamma_multiply(0.35)));
                    self.draw_outline(painter, &t, rect, Stroke::new(1.4, color));
                }
            }
        }

        self.draw_marks(painter, rect);

        if let Some(snap) = self.snap.filter(|_| !self.clean && self.hover_q.is_some_and(|(s, _)| s == side)) {
            let at = tr.pt(snap.at);
            let color = if snap.target.is_some() { ACCENT } else { FAINT };
            painter.circle_stroke(at, 6.0, Stroke::new(1.3, color));
            painter.line_segment([at - vec2(10.0, 0.0), at + vec2(10.0, 0.0)], Stroke::new(1.0, color));
            painter.line_segment([at - vec2(0.0, 10.0), at + vec2(0.0, 10.0)], Stroke::new(1.0, color));
            if let Some(start) = self.ruler_start {
                let live = Ruler { side, a: start.at, b: snap.at, gap: None };
                self.draw_ruler(painter, &tr, &live, None);
            }
        }

        // The same spot in the other pane, so the eye can follow across.
        if self.mode.is_split()
            && !self.clean
            && let Some((from, q)) = self.hover_q
            && from != side
        {
            let world = if from == Side::B { q.add(self.shift) } else { q };
            let at = self.tr(rect, Side::A).pt(world);
            let color = hi.gamma_multiply(0.6);
            painter.line_segment([at - vec2(12.0, 0.0), at + vec2(12.0, 0.0)], Stroke::new(1.0, color));
            painter.line_segment([at - vec2(0.0, 12.0), at + vec2(0.0, 12.0)], Stroke::new(1.0, color));
        }

        self.draw_change_pins(painter, rect);

        self.draw_badge(painter, rect, side);
        self.draw_scale(painter, rect);
    }

    /// With a difference selected, everything else goes dim and the changed
    /// features are drawn bright on top: red for what's only in A, green for B.
    fn draw_change_focus(&self, painter: &Painter, rect: Rect) {
        let (Some(i), Some(cmp)) = (self.change, &self.comparison) else { return };
        if !self.show_diffs {
            return;
        }
        let Some(c) = cmp.changes.get(i) else { return };
        let dim = if self.bg == PAPER { Color32::from_white_alpha(170) } else { Color32::from_black_alpha(185) };
        painter.rect_filled(rect, 0.0, dim);
        // A marker lights up the differences connected to it; stepping with Next lights the whole place.
        let items = match self.change_group.and_then(|g| c.groups.get(g)) {
            Some(group) => &group.items,
            None => &c.items,
        };
        // Everything lit up blinks, starting with A: A's pieces, then B's, and so on. A difference
        // picked from the list blinks on and off by itself while the rest of the set stays calm.
        let now = painter.ctx().input(|i| i.time);
        let epoch = self.blink_epoch.get().unwrap_or_else(|| {
            self.blink_epoch.set(Some(now));
            now
        });
        let beat = ((now - epoch) * 2.0 * BLINK_HZ).floor() as i64;
        let first_half = beat % 2 == 0;
        painter.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        if self.blink.is_none() {
            let (text, dot) = if first_half { ("Showing A", theme::RED) } else { ("Showing B", theme::GREEN) };
            self.pill(painter, pos2(rect.center().x - 45.0, rect.top() + 14.0), text, dot);
        }
        for item in items.iter().take(2000) {
            let side = if item.added { Side::B } else { Side::A };
            let t = Target { side, layer: item.layer, feature: item.feature };
            let Some((layer, f)) = self.feature(&t) else { continue };
            let color = if item.added { theme::GREEN } else { theme::RED };
            let tr = self.tr(rect, side);
            let picked = self.blink == Some(*item);
            let others_calm = self.blink.is_some() && !picked;
            let shown = if self.blink.is_some() { !picked || first_half } else { item.added != first_half };
            if !shown {
                continue;
            }
            let (fill, width) = if picked { (0.95, 3.0) } else if others_calm { (0.25, 1.2) } else { (0.7, 2.4) };
            for ring in f.outline(&layer.data.apertures) {
                if ring.len() < 3 {
                    continue;
                }
                let pts: Vec<Pos2> = ring.iter().map(|q| tr.pt(*q)).collect();
                if picked {
                    // A halo so it can't be missed.
                    painter.add(Shape::closed_line(pts.clone(), Stroke::new(width + 4.0, Color32::WHITE.gamma_multiply(0.7))));
                }
                if matches!(f.kind, Kind::Pad | Kind::Track | Kind::Hole | Kind::Slot) {
                    painter.add(Shape::convex_polygon(pts.clone(), color.gamma_multiply(fill), Stroke::NONE));
                }
                painter.add(Shape::closed_line(pts, Stroke::new(width, color)));
            }
        }
    }

    /// Where a change's pin sits: the point on the object, the round head above it, and the head's radius.
    fn pin_shape(&self, tr: &Tr, spot: &BBox, chosen: bool) -> (Pos2, Pos2, f32) {
        let k = if chosen { 1.35 } else { 1.0 };
        let tip = tr.screen_box(spot).center();
        (tip, tip - vec2(0.0, 15.0 * k), 5.5 * k)
    }

    /// A small yellow location pin over every difference, its point on the changed object.
    /// Changes are kept in A's frame, which is the frame both panes draw in.
    fn draw_change_pins(&self, painter: &Painter, rect: Rect) {
        // With a difference selected, it is lit up instead and the markers step aside.
        let Some(cmp) = self.comparison.as_ref().filter(|_| self.show_diffs && self.change.is_none()) else { return };
        let tr = self.tr(rect, Side::A);
        let visible = rect.expand(60.0);
        let outline = Color32::from_black_alpha(220);
        for c in &cmp.changes {
            for group in &c.groups {
                let (tip, head, r) = self.pin_shape(&tr, &group.at, false);
                if !visible.contains(tip) {
                    continue;
                }
                for (grow, color) in [(1.6, outline), (0.0, CHANGE_YELLOW)] {
                    let (hl, hr) = (head + vec2(-(r + grow) * 0.82, (r + grow) * 0.58), head + vec2((r + grow) * 0.82, (r + grow) * 0.58));
                    painter.add(Shape::convex_polygon(vec![tip + vec2(0.0, -grow * 0.5), hr, hl], color, Stroke::NONE));
                    painter.circle_filled(head, r + grow, color);
                }
                painter.circle_filled(head, r * 0.4, outline);
            }
        }
    }

    fn draw_badge(&self, painter: &Painter, rect: Rect, side: Side) {
        let Some(board) = self.board(side) else { return };
        let at = rect.left_top() + vec2(14.0, 14.0);
        if self.mode == ViewMode::Diff {
            let mut x = at.x;
            for (color, text) in [(theme::RED, "Only in A"), (theme::GREEN, "Only in B"), (DIM, "In both")] {
                x += self.pill(painter, pos2(x, at.y), text, color) + 8.0;
            }
            return;
        }
        if self.mode == ViewMode::Swipe || self.b.is_none() {
            return;
        }
        let mut text = format!("{}   {}", side.letter(), board.name);
        if side == Side::B && self.shift.len() > 0.0 {
            text.push_str("  (lined up with A)");
        }
        self.pill(painter, at, &text, if side == Side::A { ACCENT } else { theme::YELLOW });
    }

    fn pill(&self, painter: &Painter, at: Pos2, text: &str, dot: Color32) -> f32 {
        let galley = painter.layout_no_wrap(text.to_string(), egui::FontId::proportional(13.0), FG);
        let size = galley.size() + vec2(32.0, 12.0);
        let rect = Rect::from_min_size(at, size);
        painter.rect(rect, 9.0, theme::CARD.gamma_multiply(0.92), Stroke::new(1.0, LINE), StrokeKind::Inside);
        painter.circle_filled(at + vec2(14.0, size.y / 2.0), 3.5, dot);
        painter.galley(at + vec2(24.0, 6.0), galley, FG);
        size.x
    }

    fn draw_scale(&self, painter: &Painter, rect: Rect) {
        let step = nice_step(40.0 / self.scale);
        let w = (step * self.scale) as f32;
        // Above the toolbar, which sits along the bottom edge.
        let (x, y) = (rect.left() + 18.0, rect.bottom() - 66.0);
        let c = if self.bg == PAPER { Color32::from_black_alpha(190) } else { DIM };
        painter.line(vec![pos2(x, y - 4.0), pos2(x, y), pos2(x + w, y), pos2(x + w, y - 4.0)], Stroke::new(1.2, c));
        painter.text(pos2(x, y - 7.0), Align2::LEFT_BOTTOM, self.unit.len(step), theme::mono(11.0), c);
    }

    fn draw_grid(&self, painter: &Painter, tr: &Tr) {
        let step = nice_step(48.0 / self.scale);
        let (a, b) = (tr.board(tr.rect.left_top()), tr.board(tr.rect.right_bottom()));
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        let color = if self.bg == PAPER { Color32::from_black_alpha(26) } else { Color32::from_white_alpha(14) };
        let mut x = (x0 / step).ceil() * step;
        while x <= x1 {
            painter.vline(tr.pt(p(x, 0.0)).x, tr.rect.y_range(), Stroke::new(1.0, color));
            x += step;
        }
        let mut y = (y0 / step).ceil() * step;
        while y <= y1 {
            painter.hline(tr.rect.x_range(), tr.pt(p(0.0, y)).y, Stroke::new(1.0, color));
            y += step;
        }
    }

    fn draw_outline(&self, painter: &Painter, t: &Target, rect: Rect, stroke: Stroke) {
        let Some((layer, f)) = self.feature(t) else { return };
        let tr = self.tr(rect, t.side);
        for ring in f.outline(&layer.data.apertures) {
            if ring.len() >= 2 {
                painter.add(Shape::closed_line(ring.iter().map(|q| tr.pt(*q)).collect(), stroke));
            }
        }
    }

    fn draw_net(&self, painter: &Painter, tr: &Tr, name: &str, side: Side) {
        let Some(board) = self.board(side) else { return };
        let mut budget = 4000;
        for slot in self.slots.iter().filter(|s| s.visible) {
            let Some(index) = slot.layer(side) else { continue };
            let layer = &board.layers[index];
            for f in &layer.data.features {
                if budget > 0 && f.net != 0 && layer.data.text(f.net) == Some(name) {
                    budget -= 1;
                    for ring in f.outline(&layer.data.apertures) {
                        let pts = ring.iter().map(|q| tr.pt(*q)).collect();
                        painter.add(Shape::closed_line(pts, Stroke::new(1.0, theme::YELLOW)));
                    }
                }
            }
        }
    }

    fn draw_measure(&self, painter: &Painter, t: &Target, rect: Rect, mark: Option<Color32>) -> Option<Rect> {
        let (layer, f) = self.feature(t)?;
        let tr = self.tr(rect, t.side);
        let b = tr.screen_box(&f.bbox);
        let stroke = Stroke::new(1.0, ACCENT);

        // Proper dimension lines once the shape is big enough to carry them.
        if f.kind == Kind::Pad {
            let (w, h) = layer.data.apertures.get(f.ap as usize).map_or((f.width, f.width), |a| (a.w, a.h));
            let font = theme::mono(11.0);
            if b.width() >= 34.0 {
                let y = b.top() - 12.0;
                painter.line_segment([pos2(b.left(), y - 4.0), pos2(b.left(), y + 8.0)], stroke);
                painter.line_segment([pos2(b.right(), y - 4.0), pos2(b.right(), y + 8.0)], stroke);
                painter.line_segment([pos2(b.left(), y), pos2(b.right(), y)], stroke);
                painter.text(pos2(b.center().x, y - 3.0), Align2::CENTER_BOTTOM, self.unit.num(w), font.clone(), ACCENT);
            }
            if b.height() >= 22.0 {
                let x = b.right() + 12.0;
                painter.line_segment([pos2(x - 8.0, b.top()), pos2(x + 4.0, b.top())], stroke);
                painter.line_segment([pos2(x - 8.0, b.bottom()), pos2(x + 4.0, b.bottom())], stroke);
                painter.line_segment([pos2(x, b.top()), pos2(x, b.bottom())], stroke);
                painter.text(pos2(x + 5.0, b.center().y), Align2::LEFT_CENTER, self.unit.num(h), font, ACCENT);
            }
        }
        if f.kind == Kind::Hole {
            let c = tr.pt(f.path[0]);
            painter.line_segment([c - vec2(5.0, 0.0), c + vec2(5.0, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, 5.0), c + vec2(0.0, 5.0)], stroke);
        }

        let anchor = match f.kind {
            Kind::Track | Kind::Slot | Kind::Arc => {
                tr.pt(f.path[f.path.len() / 2].add(f.path[(f.path.len() - 1) / 2]).scale(0.5))
            }
            _ => b.right_top(),
        };
        // A long track's middle can be off screen: keep the label's end of the leader in view.
        let room = Rect::from_min_max(rect.left_top() + vec2(30.0, 40.0), rect.right_bottom() - vec2(30.0, 100.0));
        let anchor = if room.width() > 0.0 && room.height() > 0.0 { anchor.clamp(room.min, room.max) } else { anchor };
        let text = self.popup_text(t).unwrap_or_else(|| format!("{}  {}\n{}", f.kind.label(), self.size_text(layer, f), self.coord_text(f)));
        Some(self.tag(painter, rect, anchor, &text, mark))
    }

    /// A little label on a leader line, kept inside the pane.
    fn tag(&self, painter: &Painter, rect: Rect, anchor: Pos2, text: &str, mark: Option<Color32>) -> Rect {
        let galley = painter.layout_no_wrap(text.to_string(), theme::mono(12.0), FG);
        let size = galley.size() + vec2(16.0, 10.0);
        let mut min = anchor + vec2(20.0, -34.0 - size.y / 2.0);
        min.x = min.x.clamp(rect.left() + 6.0, (rect.right() - size.x - 6.0).max(rect.left() + 6.0));
        min.y = min.y.clamp(rect.top() + 6.0, (rect.bottom() - size.y - 6.0).max(rect.top() + 6.0));
        let tag = Rect::from_min_size(min, size);
        let near = pos2(anchor.x.clamp(tag.left(), tag.right()), anchor.y.clamp(tag.top(), tag.bottom()));
        painter.line_segment([anchor, near], Stroke::new(1.0, ACCENT));
        painter.circle_filled(anchor, 2.5, ACCENT);
        let edge = mark.map_or(Stroke::new(1.0, theme::ACCENT_STRONG), |c| Stroke::new(1.6, c));
        painter.rect(tag, 7.0, theme::ACCENT_SOFT, edge, StrokeKind::Inside);
        painter.galley(min + vec2(8.0, 5.0), galley, FG);
        tag
    }

    fn draw_ruler(&self, painter: &Painter, tr: &Tr, r: &Ruler, mark: Option<Color32>) -> Rect {
        let (a, b) = (tr.pt(r.a), tr.pt(r.b));
        let color = theme::YELLOW;
        let stroke = Stroke::new(if mark.is_some() { 2.4 } else { 1.3 }, color);
        painter.line_segment([a, b], stroke);
        let along = (b - a).normalized();
        let cross = vec2(-along.y, along.x) * 6.0;
        for end in [a, b] {
            painter.line_segment([end - cross, end + cross], stroke);
        }
        let mut text = self.unit.len(r.b.sub(r.a).len());
        if let Some(gap) = r.gap {
            text.push_str(&format!("  gap {}", self.unit.len(gap)));
        }
        let ink = Color32::from_rgb(0x1a, 0x16, 0x0c);
        let galley = painter.layout_no_wrap(text, theme::mono(12.0), ink);
        let size = galley.size() + vec2(14.0, 8.0);
        let label = Rect::from_center_size(a + (b - a) * 0.5, size);
        painter.rect_filled(label, 7.0, color);
        if let Some(edge) = mark {
            painter.rect_stroke(label.expand(1.5), 8.0, Stroke::new(1.6, edge), StrokeKind::Outside);
        }
        painter.galley(label.min + vec2(7.0, 4.0), galley, ink);
        label
    }

    fn welcome(&mut self, ui: &mut Ui, full: Rect) {
        let rect = Rect::from_center_size(full.center(), vec2(420.0, 244.0));
        ui.painter().rect(rect, 12.0, theme::CARD, Stroke::new(1.0, LINE), StrokeKind::Inside);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(26.0)));
        child.label(RichText::new("Open your Gerber files").font(semibold(20.0)));
        child.add_space(4.0);
        child.label(
            RichText::new("Choose a folder or a zip of Altium output, or drop one here. Add a second to compare them.")
                .color(DIM),
        );
        child.add_space(14.0);
        child.horizontal(|ui| {
            if tool_button(ui, Icon::Folder, "Open folder", true).clicked() {
                self.ask_folder(Side::A, ui.ctx());
            }
            if tool_button(ui, Icon::Zip, "Open zip", true).clicked() {
                self.ask_zip(Side::A, ui.ctx());
            }
        });
        if !self.pending.is_empty() {
            child.add_space(8.0);
            child.horizontal(|ui| {
                ui.spinner();
                ui.label("Opening…");
            });
        }
    }

    fn floating_tools(&mut self, ui: &mut Ui, full: Rect) {
        let area = egui::Area::new(ui.scope_id().with("tools"))
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(pos2(full.center().x, full.bottom() - 12.0))
            .order(egui::Order::Foreground)
            .interactable(true);
        area.show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(theme::CARD.gamma_multiply(0.96))
                .stroke(Stroke::new(1.0, LINE))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(5))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let tips = [
                            (Tool::Select, Icon::Cursor, "Select", "Click anything to measure it"),
                            (Tool::Measure, Icon::Ruler, "Ruler", "Click two points. Shift keeps it straight. (M)"),
                            (Tool::Markup, Icon::Pen, "Mark up", "Draw arrows, words and shapes on the board (T)"),
                        ];
                        for (tool, icon, label, tip) in tips {
                            let response = segmented_tool(ui, icon, label, self.tool == tool).on_hover_text(tip);
                            if response.clicked() {
                                self.tool = tool;
                                self.ruler_start = None;
                            }
                        }
                        let drawing = self.tool == Tool::Markup;
                        if !drawing {
                            let rules = segmented_tool(ui, Icon::Filter, &self.rules.label(), !self.rules.is_default());
                            let rules = rules.on_hover_text("Choose what a click can pick, and what comes first");
                            egui::Popup::from_toggle_button_response(&rules)
                                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                .align(egui::RectAlign::TOP_START)
                                .show(|ui| self.pick_menu(ui));
                        }
                        ui.add_space(2.0);
                        ui.separator();
                        ui.add_space(2.0);
                        let fit = segmented_tool(ui, Icon::Fit, "Fit", self.fitted);
                        if fit.on_hover_text("Make the board fill all the room there is (F)").clicked() {
                            self.fit_pending = true;
                        }
                        let flip = icon_button(ui, Icon::Flip, self.mirror);
                        if flip.on_hover_text("Flip, to see it from the bottom side (X)").clicked() {
                            self.mirror = !self.mirror;
                        }
                        if icon_button(ui, Icon::Grid, self.grid).on_hover_text("Grid (G)").clicked() {
                            self.grid = !self.grid;
                        }
                        let cube = segmented_tool(ui, Icon::Cube, "3D", false);
                        if cube.on_hover_text("See the board as a solid (3)").clicked() {
                            self.enter_3d();
                        }
                        if self.tool == Tool::Markup {
                            ui.add_space(2.0);
                            ui.separator();
                            self.markup_actions(ui);
                        } else {
                            let save = icon_button(ui, Icon::Picture, false);
                            if save.on_hover_text("Save a picture of this view").clicked() {
                                self.ask_save_picture(ui.ctx());
                            }
                        }
                        if !drawing {
                            ui.add_space(2.0);
                            ui.separator();
                            ui.label(RichText::new(format!("{:.0} px/mm", self.scale)).font(theme::mono(11.5)).color(FAINT));
                        }
                    });
                });
        });
    }

    fn apply_launch(&mut self, full: Rect) {
        // A picture run settles its size first, so spots land where they should.
        let settling = self.launch.screenshot.is_some() && self.frames < 12;
        if self.applied_launch || self.loading() || settling {
            return;
        }
        self.applied_launch = true;
        if let Some(view) = self.launch.view.take() {
            self.mode = match view.as_str() {
                "a" => ViewMode::OnlyA,
                "b" if self.b.is_some() => ViewMode::OnlyB,
                "diff" if self.b.is_some() => ViewMode::Diff,
                "swipe" if self.b.is_some() => ViewMode::Swipe,
                "side" if self.b.is_some() => ViewMode::Side,
                "stack" if self.b.is_some() => ViewMode::Stack,
                _ => self.mode,
            };
            self.fit(full);
        }
        match self.launch.mix.as_deref() {
            Some("light") => self.set_mix(crate::gpu::Mix::Light),
            Some("ink") => self.set_mix(crate::gpu::Mix::Ink),
            _ => {}
        }
        match self.launch.unit.as_deref() {
            Some("mil") => self.unit = Unit::Mil,
            Some("in") => self.unit = Unit::Inch,
            _ => {}
        }
        if self.launch.all_layers {
            self.preset("All");
        }
        if self.launch.flip {
            self.mirror = true;
        }
        if self.launch.three_d {
            self.enter_3d();
        }
        if let Some((at, width)) = self.launch.zoom {
            self.center = at;
            self.scale = self.pane_size(full).x as f64 / width;
            self.fitted = false;
        }
        if let Some(q) = self.launch.pick {
            let hits = self.pick(Side::A, q, 0.1);
            self.select_from(&hits, q);
        }
        if let Some((a, b)) = self.launch.ruler {
            let (x, y) = (self.snap_at(Side::A, a, 0.5), self.snap_at(Side::A, b, 0.5));
            self.finish_ruler(Side::A, x, y);
        }
    }

    pub fn drive_screenshot(&mut self, ctx: &egui::Context) {
        let Some(path) = self.launch.screenshot.clone() else { return };
        // A tiled window can be narrow: step back so the picture shows the whole layout.
        if self.frames < 10 {
            let width = ctx.content_rect().width();
            if width < 1300.0 {
                ctx.set_zoom_factor(ctx.zoom_factor() * width / 1320.0);
            }
            self.fit_pending = true;
        }
        // Keeps drawing when the window isn't on show, or the screen is asleep.
        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaintWhileHidden);
        ctx.request_repaint();
        if !self.loading() {
            self.frames += 1;
        }
        if self.frames < SHOT_FRAMES || self.shot_sent || !self.play.is_empty() {
            return;
        }
        self.shot_sent = true;
        let finish = ctx.clone();
        ctx.request_screenshot(move |picture| {
            let code = match save_png(&picture, None, &path) {
                Ok(()) => 0,
                Err(error) => {
                    eprintln!("copperview: couldn't save the picture: {error}");
                    1
                }
            };
            finish.send_viewport_cmd(egui::ViewportCommand::Close);
            finish.request_repaint();
            std::thread::spawn(move || {
                // The window closes on the next frame; this is the backstop.
                std::thread::sleep(Duration::from_secs(1));
                std::process::exit(code);
            });
        });
    }

    /// The toolbars go away for a frame before the picture is asked for, and stay
    /// away until it has been taken, which happens a little after it's asked for.
    pub fn prepare_clean_frame(&mut self, ctx: &egui::Context) {
        let frame = ctx.cumulative_frame_nr();
        if self.clean {
            // Never leave the toolbars gone, even if the picture doesn't arrive.
            if frame >= self.clean_from + CLEAN_FRAMES_MOST {
                self.clean = false;
            }
        } else if self.save_pending.is_some() {
            self.clean = true;
            self.clean_from = frame;
        }
        if self.clean {
            ctx.request_repaint();
        }
    }

    pub fn drive_save(&mut self, ctx: &egui::Context) {
        if !self.clean || ctx.cumulative_frame_nr() <= self.clean_from {
            return;
        }
        let Some(mut path) = self.save_pending.take() else { return };
        if path.extension().is_none() {
            path.set_extension("png");
        }
        let ppp = ctx.pixels_per_point();
        let r = self.canvas_rect;
        let crop = [
            (r.left() * ppp) as usize,
            (r.top() * ppp) as usize,
            (r.right() * ppp) as usize,
            (r.bottom() * ppp) as usize,
        ];
        let (tx, wake) = (self.asked_tx.clone(), ctx.clone());
        // Taken from this frame, which leaves the toolbars out.
        ctx.request_screenshot(move |picture| {
            let note = match save_png(&picture, Some(crop), &path) {
                Ok(()) => format!("Saved {}", path.display()),
                Err(error) => format!("The picture couldn't be saved: {error}"),
            };
            let _ = tx.send(Asked::PictureDone(note));
            wake.request_repaint();
        });
        ctx.request_repaint();
    }
}
