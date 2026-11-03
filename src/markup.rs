//! Drawing on top of the board: arrows, words and shapes for talking about it
//! with other people. Marks are pinned to the board, so they stay on their spot
//! through zooming and panning, while their lines and text stay a readable size.

use crate::app::{App, Tool};
use crate::geom::*;
use crate::icons::Icon;
use crate::panels::{icon_button, tool_button};
use crate::theme::{self, ACCENT, ACCENT_SOFT, FAINT, FG, LINE, RAISED, SELECTION};
use crate::view::Tr;
use eframe::egui::{
    self, Align2, Color32, CornerRadius, CursorIcon, FontId, Key, Modifiers, Painter, PointerButton, Pos2, Rect,
    Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MarkKind {
    Arrow,
    OpenArrow,
    DoubleArrow,
    DashedArrow,
    Callout,
    Text,
    Box,
    Rounded,
    Circle,
}

impl MarkKind {
    pub const ALL: [(MarkKind, &'static str); 9] = [
        (MarkKind::Arrow, "Arrow"),
        (MarkKind::OpenArrow, "Open arrow"),
        (MarkKind::DoubleArrow, "Double arrow"),
        (MarkKind::DashedArrow, "Dashed arrow"),
        (MarkKind::Callout, "Arrow with words"),
        (MarkKind::Text, "Words"),
        (MarkKind::Box, "Box"),
        (MarkKind::Rounded, "Rounded box"),
        (MarkKind::Circle, "Circle"),
    ];

    fn is_shape(self) -> bool {
        matches!(self, MarkKind::Box | MarkKind::Rounded | MarkKind::Circle)
    }

    fn has_words(self) -> bool {
        matches!(self, MarkKind::Text | MarkKind::Callout)
    }
}

pub const COLORS: [Color32; 6] = [
    theme::rgb(0xffd23f),
    theme::rgb(0xff4d4f),
    theme::rgb(0x4ddb7a),
    theme::rgb(0x4fa3ff),
    theme::rgb(0xffffff),
    theme::rgb(0x111216),
];
const WIDTHS: [f32; 3] = [1.5, 2.5, 4.5];
const SIZES: [(f32, &str); 3] = [(14.0, "S"), (18.0, "M"), (26.0, "L")];
/// How close the pointer has to be to a line or a handle, on screen.
const REACH: f32 = 7.0;
/// Shorter than this on screen, a drag was probably a slip.
const SMALLEST_DRAG: f32 = 5.0;
const UNDO_STEPS: usize = 60;

#[derive(Clone, Debug)]
pub struct Mark {
    pub kind: MarkKind,
    pub a: P,
    pub b: P,
    pub color: Color32,
    pub width: f32,
    pub fill: bool,
    pub size: f32,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarkDrag {
    /// Drawing a new mark.
    Draw,
    /// Moving the whole mark from where it was last frame.
    Move(P),
    /// Dragging one end: false for the first, true for the second.
    End(bool),
}

/// The drawing tool's settings and the marks themselves.
pub struct Markup {
    pub marks: Vec<Mark>,
    pub kind: MarkKind,
    pub color: Color32,
    pub width: f32,
    pub fill: bool,
    pub size: f32,
    /// The markup layer, in the layer list.
    pub visible: bool,
    pub alpha: f32,
    pub selected: Option<usize>,
    pub drag: Option<MarkDrag>,
    pub editing: Option<usize>,
    pub edit_focus: bool,
    pub edit_pane: Rect,
    pub undo: Vec<Vec<Mark>>,
    pub word_boxes: std::cell::RefCell<Vec<(usize, Rect)>>,
}

impl Default for Markup {
    fn default() -> Self {
        Markup {
            marks: Vec::new(),
            kind: MarkKind::Arrow,
            color: COLORS[0],
            width: WIDTHS[1],
            fill: false,
            size: SIZES[1].0,
            visible: true,
            alpha: 1.0,
            selected: None,
            drag: None,
            editing: None,
            edit_focus: false,
            edit_pane: Rect::NOTHING,
            undo: Vec::new(),
            word_boxes: Default::default(),
        }
    }
}

impl Markup {
    pub fn remember(&mut self) {
        if self.undo.len() == UNDO_STEPS {
            self.undo.remove(0);
        }
        self.undo.push(self.marks.clone());
    }

    pub fn step_back(&mut self) {
        if let Some(earlier) = self.undo.pop() {
            self.marks = earlier;
            self.selected = None;
            self.editing = None;
            self.drag = None;
        }
    }

    pub fn delete_selected(&mut self) -> bool {
        let Some(i) = self.selected.filter(|i| *i < self.marks.len()) else { return false };
        self.remember();
        self.marks.remove(i);
        self.selected = None;
        true
    }

    /// Takes every mark off, in a way Undo can bring back.
    pub fn remove_all(&mut self) {
        self.remember();
        self.marks.clear();
        self.selected = None;
        self.editing = None;
        self.drag = None;
    }

    pub fn clear(&mut self) {
        self.visible = true;
        self.alpha = 1.0;
        self.marks.clear();
        self.undo.clear();
        self.selected = None;
        self.editing = None;
        self.drag = None;
    }

    /// Ends typing. Words with nothing typed aren't worth keeping.
    pub fn finish_editing(&mut self) {
        let Some(i) = self.editing.take() else { return };
        let Some(mark) = self.marks.get_mut(i) else { return };
        mark.text = mark.text.trim_end().to_string();
        if mark.text.is_empty() && mark.kind == MarkKind::Text {
            self.marks.remove(i);
            self.selected = None;
        }
    }
}

fn head(painter: &Painter, tip: Pos2, along: Vec2, size: f32, color: Color32, solid: bool, width: f32) {
    let side = vec2(-along.y, along.x);
    let back = tip - along * size;
    let (left, right) = (back + side * size * 0.48, back - side * size * 0.48);
    if solid {
        painter.add(Shape::convex_polygon(vec![tip, left, right], color, Stroke::NONE));
    } else {
        painter.add(Shape::line(vec![left, tip, right], Stroke::new(width, color)));
    }
}

/// Draws one arrow shaft and its heads.
fn arrow(painter: &Painter, kind: MarkKind, a: Pos2, b: Pos2, color: Color32, width: f32) {
    arrow_sized(painter, kind, a, b, color, width, 10.0 + width * 2.6);
}

fn arrow_sized(painter: &Painter, kind: MarkKind, a: Pos2, b: Pos2, color: Color32, width: f32, size: f32) {
    let span = b - a;
    if span.length() < 1.0 {
        return;
    }
    let along = span.normalized();
    let stroke = Stroke::new(width, color);
    let solid = kind != MarkKind::OpenArrow;
    // The shaft stops inside a solid head, so its end doesn't poke past the point.
    let end = if solid { b - along * size * 0.7 } else { b };
    let start = if kind == MarkKind::DoubleArrow { a + along * size * 0.7 } else { a };
    if kind == MarkKind::DashedArrow {
        painter.extend(Shape::dashed_line(&[start, end], stroke, 4.0 + width * 2.0, 3.0 + width * 1.5));
    } else {
        painter.line_segment([start, end], stroke);
    }
    head(painter, b, along, size, color, solid, width);
    if kind == MarkKind::DoubleArrow {
        head(painter, a, -along, size, color, true, width);
    }
}

fn shape(painter: &Painter, kind: MarkKind, area: Rect, color: Color32, width: f32, fill: bool) {
    let stroke = Stroke::new(width, color);
    let inside = if fill { color.gamma_multiply(0.22) } else { Color32::TRANSPARENT };
    if kind == MarkKind::Circle {
        painter.add(Shape::ellipse_filled(area.center(), area.size() / 2.0, inside));
        painter.add(Shape::ellipse_stroke(area.center(), area.size() / 2.0, stroke));
    } else {
        let round = if kind == MarkKind::Rounded { (area.width().min(area.height()) * 0.22).min(16.0) } else { 0.0 };
        painter.rect(area, round, inside, stroke, StrokeKind::Middle);
    }
}

/// Words on a dark plate, so they read over any layer color.
fn words(painter: &Painter, at: Pos2, centred: bool, mark: &Mark) -> Rect {
    let font = FontId::proportional(mark.size);
    let text_color = if mark.color == COLORS[5] { Color32::WHITE } else { mark.color };
    let galley = painter.layout_no_wrap(mark.text.clone(), font, text_color);
    let size = galley.size() + vec2(14.0, 8.0);
    let min = if centred { at - size / 2.0 } else { at };
    let plate = Rect::from_min_size(min, size);
    painter.rect(plate, 6.0, Color32::from_black_alpha(205), Stroke::new(1.0, mark.color), StrokeKind::Inside);
    painter.galley(min + vec2(7.0, 4.0), galley, text_color);
    plate
}

fn draw_mark(painter: &Painter, tr: &Tr, mark: &Mark, typing: bool) -> Option<Rect> {
    let (a, b) = (tr.pt(mark.a), tr.pt(mark.b));
    match mark.kind {
        MarkKind::Text => (!typing).then(|| words(painter, a, false, mark)),
        MarkKind::Callout => {
            arrow(painter, MarkKind::Arrow, a, b, mark.color, mark.width);
            (!typing && !mark.text.is_empty()).then(|| words(painter, a, true, mark))
        }
        MarkKind::Box | MarkKind::Rounded | MarkKind::Circle => {
            shape(painter, mark.kind, Rect::from_two_pos(a, b), mark.color, mark.width, mark.fill);
            None
        }
        _ => {
            arrow(painter, mark.kind, a, b, mark.color, mark.width);
            None
        }
    }
}

fn near_segment(q: Pos2, a: Pos2, b: Pos2) -> f32 {
    let at = |r: Pos2| p(r.x as f64, r.y as f64);
    dist_to_segment(at(q), at(a), at(b)) as f32
}

fn touches(mark: &Mark, tr: &Tr, q: Pos2) -> bool {
    let (a, b) = (tr.pt(mark.a), tr.pt(mark.b));
    let area = Rect::from_two_pos(a, b);
    match mark.kind {
        MarkKind::Text => false,
        MarkKind::Box | MarkKind::Rounded => {
            let edge = [
                (area.left_top(), area.right_top()),
                (area.right_top(), area.right_bottom()),
                (area.right_bottom(), area.left_bottom()),
                (area.left_bottom(), area.left_top()),
            ];
            (mark.fill && area.contains(q)) || edge.iter().any(|(x, y)| near_segment(q, *x, *y) <= REACH)
        }
        MarkKind::Circle => {
            let radius = area.size() / 2.0;
            if radius.x < 1.0 || radius.y < 1.0 {
                return false;
            }
            let off = q - area.center();
            let ring = (off.x / radius.x).hypot(off.y / radius.y);
            (mark.fill && ring <= 1.0) || (ring - 1.0).abs() * radius.x.min(radius.y) <= REACH
        }
        _ => near_segment(q, a, b) <= REACH,
    }
}

impl App {
    fn mark_tr(&self, rect: Rect) -> Tr {
        self.view_tr(rect)
    }

    pub fn draw_marks(&self, painter: &Painter, rect: Rect) {
        let tr = self.mark_tr(rect);
        let m = &self.markup;
        if !m.visible {
            return;
        }
        for (i, mark) in m.marks.iter().enumerate() {
            let mut shown = mark.clone();
            shown.color = shown.color.gamma_multiply(m.alpha);
            if let Some(plate) = draw_mark(painter, &tr, &shown, m.editing == Some(i)) {
                m.word_boxes.borrow_mut().push((i, plate));
            }
        }
        // The handles are for working, so a saved picture leaves them out.
        if self.clean || self.tool != Tool::Markup {
            return;
        }
        let Some(mark) = m.selected.and_then(|i| m.marks.get(i)) else { return };
        let mut ends = vec![tr.pt(mark.a)];
        if mark.kind != MarkKind::Text {
            ends.push(tr.pt(mark.b));
        }
        for end in ends {
            let handle = Rect::from_center_size(end, Vec2::splat(9.0));
            painter.rect(handle, 2.0, Color32::WHITE, Stroke::new(1.0, Color32::BLACK), StrokeKind::Middle);
        }
    }

    fn mark_at(&self, tr: &Tr, q: Pos2) -> Option<usize> {
        let m = &self.markup;
        if !m.visible {
            return None;
        }
        let boxes = m.word_boxes.borrow();
        (0..m.marks.len()).rev().find(|i| {
            boxes.iter().any(|(n, plate)| n == i && plate.expand(2.0).contains(q)) || touches(&m.marks[*i], tr, q)
        })
    }

    fn end_at(&self, tr: &Tr, q: Pos2) -> Option<bool> {
        let mark = self.markup.selected.and_then(|i| self.markup.marks.get(i))?;
        if tr.pt(mark.a).distance(q) <= REACH + 2.0 {
            return Some(false);
        }
        (mark.kind != MarkKind::Text && tr.pt(mark.b).distance(q) <= REACH + 2.0).then_some(true)
    }

    fn start_typing(&mut self, index: usize, pane: Rect) {
        self.markup.selected = Some(index);
        self.markup.editing = Some(index);
        self.markup.edit_focus = true;
        self.markup.edit_pane = pane;
    }

    fn new_mark(&self, kind: MarkKind, at: P) -> Mark {
        let m = &self.markup;
        Mark { kind, a: at, b: at, color: m.color, width: m.width, fill: m.fill, size: m.size, text: String::new() }
    }

    /// Mouse work on the board while the drawing tool is on.
    pub fn markup_input(&mut self, ui: &mut Ui, response: &Response, rect: Rect) {
        if self.markup.editing.is_some() {
            return;
        }
        let tr = self.mark_tr(rect);
        let world = |at: Pos2| tr.board(at);
        if let Some(at) = response.hover_pos() {
            let over = self.end_at(&tr, at).is_some() || self.mark_at(&tr, at).is_some();
            ui.ctx().set_cursor_icon(if over { CursorIcon::Move } else { CursorIcon::Crosshair });
        }

        if response.drag_started_by(PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
        {
            if let Some(end) = self.end_at(&tr, origin) {
                self.markup.remember();
                self.markup.drag = Some(MarkDrag::End(end));
            } else if let Some(i) = self.mark_at(&tr, origin) {
                self.markup.remember();
                self.markup.selected = Some(i);
                self.markup.drag = Some(MarkDrag::Move(world(origin)));
            } else {
                self.markup.remember();
                self.markup.visible = true;
                let mark = self.new_mark(self.markup.kind, world(origin));
                self.markup.marks.push(mark);
                self.markup.selected = Some(self.markup.marks.len() - 1);
                self.markup.drag = Some(MarkDrag::Draw);
            }
        }

        if response.dragged_by(PointerButton::Primary)
            && let (Some(at), Some(drag), Some(i)) =
                (response.interact_pointer_pos(), self.markup.drag, self.markup.selected)
        {
            let square = ui.input(|input| input.modifiers.shift);
            let q = world(at);
            if let Some(mark) = self.markup.marks.get_mut(i) {
                match drag {
                    MarkDrag::Draw | MarkDrag::End(true) if mark.kind == MarkKind::Text => mark.a = q,
                    MarkDrag::Draw | MarkDrag::End(true) => mark.b = held(mark, q, square),
                    MarkDrag::End(false) => mark.a = q,
                    MarkDrag::Move(last) => {
                        let by = q.sub(last);
                        mark.a = mark.a.add(by);
                        mark.b = mark.b.add(by);
                        self.markup.drag = Some(MarkDrag::Move(q));
                    }
                }
            }
        }

        if response.drag_stopped_by(PointerButton::Primary) {
            let drag = self.markup.drag.take();
            if drag == Some(MarkDrag::Draw)
                && let Some(i) = self.markup.selected
                && let Some(mark) = self.markup.marks.get(i)
            {
                let small = tr.pt(mark.a).distance(tr.pt(mark.b)) < SMALLEST_DRAG;
                let kind = mark.kind;
                if kind.has_words() {
                    self.start_typing(i, rect);
                } else if small {
                    self.markup.marks.remove(i);
                    self.markup.selected = None;
                    self.markup.undo.pop();
                }
            }
        }

        if response.clicked_by(PointerButton::Primary)
            && let Some(at) = response.interact_pointer_pos()
        {
            if let Some(i) = self.mark_at(&tr, at) {
                let second = self.markup.selected == Some(i);
                self.markup.selected = Some(i);
                if second && self.markup.marks[i].kind.has_words() {
                    self.start_typing(i, rect);
                }
            } else if self.markup.kind == MarkKind::Text {
                self.markup.remember();
                self.markup.visible = true;
                let mark = self.new_mark(MarkKind::Text, world(at));
                self.markup.marks.push(mark);
                self.start_typing(self.markup.marks.len() - 1, rect);
            } else {
                self.markup.selected = None;
            }
        }
        if response.secondary_clicked() {
            self.markup.selected = None;
        }
    }

    /// The little box for typing words into, right where they'll sit.
    pub fn mark_editor(&mut self, ui: &mut Ui) {
        let Some(i) = self.markup.editing else { return };
        let Some(mark) = self.markup.marks.get(i) else {
            self.markup.editing = None;
            return;
        };
        let at = self.mark_tr(self.markup.edit_pane).pt(mark.a);
        let (color, size) = (if mark.color == COLORS[5] { Color32::WHITE } else { mark.color }, mark.size);
        let mut done = false;
        egui::Area::new(ui.scope_id().with("mark words")).fixed_pos(at).order(egui::Order::Foreground).show(
            ui.ctx(),
            |ui| {
                egui::Frame::new()
                    .fill(Color32::from_black_alpha(225))
                    .stroke(Stroke::new(1.0, color))
                    .corner_radius(CornerRadius::same(6))
                    .inner_margin(egui::Margin::symmetric(7, 4))
                    .show(ui, |ui| {
                        done = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Enter));
                        let edit = egui::TextEdit::multiline(&mut self.markup.marks[i].text)
                            .id_salt("mark words")
                            .desired_rows(1)
                            .desired_width(240.0)
                            .frame(egui::Frame::NONE)
                            .font(FontId::proportional(size))
                            .text_color(color)
                            .hint_text("Type, then press Enter");
                        let response = ui.add(edit);
                        if self.markup.edit_focus {
                            response.request_focus();
                            self.markup.edit_focus = false;
                        } else if response.lost_focus() {
                            done = true;
                        }
                    });
            },
        );
        if done {
            self.markup.finish_editing();
        }
    }

    fn style_selected(&mut self, change: impl Fn(&mut Mark)) {
        if let Some(mark) = self.markup.selected.and_then(|i| self.markup.marks.get_mut(i)) {
            change(mark);
        }
    }

    /// The drawing tool's controls, floating just above the main toolbar.
    pub fn markup_bar(&mut self, ui: &mut Ui, full: Rect) {
        egui::Area::new(ui.scope_id().with("markup bar"))
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(pos2(full.center().x, full.bottom() - 62.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(theme::CARD.gamma_multiply(0.97))
                    .stroke(Stroke::new(1.0, LINE))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(3.0, 5.0);
                        ui.horizontal(|ui| self.kind_buttons(ui));
                        ui.horizontal(|ui| self.style_buttons(ui));
                    });
            });
    }

    fn kind_buttons(&mut self, ui: &mut Ui) {
        for (kind, name) in MarkKind::ALL {
            let (rect, response) = ui.allocate_exact_size(vec2(38.0, 30.0), Sense::click());
            let on = self.markup.kind == kind;
            let fill = if on {
                ACCENT_SOFT
            } else if response.hovered() {
                RAISED
            } else {
                Color32::TRANSPARENT
            };
            ui.painter().rect_filled(rect, 8.0, fill);
            kind_picture(ui.painter(), rect.shrink2(vec2(7.0, 5.0)), kind, if on { ACCENT } else { FG });
            if response.on_hover_text(name).clicked() {
                self.markup.kind = kind;
                self.markup.selected = None;
            }
        }
    }

    fn style_buttons(&mut self, ui: &mut Ui) {
        for color in COLORS {
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
            ui.painter().circle_filled(rect.center(), 8.5, color);
            let on = self.markup.color == color;
            let edge = if on { FG } else if response.hovered() { FAINT } else { LINE };
            ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(if on { 1.6 } else { 1.0 }, edge));
            if response.on_hover_text("Color").clicked() {
                self.markup.color = color;
                self.style_selected(|m| m.color = color);
            }
        }
        ui.add_space(4.0);
        for (i, width) in WIDTHS.iter().enumerate() {
            let on = self.markup.width == *width;
            let (rect, response) = small_toggle(ui, on, "Line thickness");
            let (left, right) = (rect.left_center() + vec2(6.0, 0.0), rect.right_center() - vec2(6.0, 0.0));
            ui.painter().line_segment([left, right], Stroke::new(1.0 + i as f32 * 1.4, if on { ACCENT } else { FG }));
            if response.clicked() {
                self.markup.width = *width;
                let chosen = *width;
                self.style_selected(|m| m.width = chosen);
            }
        }
        ui.add_space(4.0);
        for (size, name) in SIZES {
            let on = self.markup.size == size;
            let (rect, response) = small_toggle(ui, on, "Size of the words");
            let color = if on { ACCENT } else { FG };
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, name, FontId::proportional(12.5), color);
            if response.clicked() {
                self.markup.size = size;
                self.style_selected(|m| m.size = size);
            }
        }
        ui.add_space(4.0);
        let fill = self.markup.fill;
        let (rect, response) = small_toggle(ui, fill, "Tint the inside of boxes and circles");
        let tile = rect.shrink(6.0);
        let color = if fill { ACCENT } else { FG };
        ui.painter().rect_stroke(tile, 3.0, Stroke::new(1.2, color), StrokeKind::Inside);
        if fill {
            ui.painter().rect_filled(tile.shrink(2.5), 1.0, color.gamma_multiply(0.6));
        }
        if response.clicked() {
            self.markup.fill = !fill;
            let on = !fill;
            self.style_selected(|m| {
                if m.kind.is_shape() {
                    m.fill = on;
                }
            });
        }
    }

    /// The buttons that act on the marks, on the toolbar's row of their own.
    pub fn markup_actions(&mut self, ui: &mut Ui) {
        if icon_button(ui, Icon::Undo, false).on_hover_text("Undo (Ctrl+Z)").clicked() {
            self.markup.step_back();
        }
        if icon_button(ui, Icon::Trash, false).on_hover_text("Remove the selected mark (Delete)").clicked() {
            self.markup.delete_selected();
        }
        if tool_button(ui, Icon::Picture, "Export PNG", true).on_hover_text("Save the view and its marks as a picture").clicked() {
            self.ask_save_picture(ui.ctx());
        }
    }
}

/// With Shift held, arrows go straight and shapes go square.
fn held(mark: &Mark, q: P, square: bool) -> P {
    if !square {
        return q;
    }
    let d = q.sub(mark.a);
    if mark.kind.is_shape() {
        let side = d.x.abs().max(d.y.abs());
        p(mark.a.x + side.copysign(d.x), mark.a.y + side.copysign(d.y))
    } else if d.x.abs() > d.y.abs() {
        p(q.x, mark.a.y)
    } else {
        p(mark.a.x, q.y)
    }
}

fn small_toggle(ui: &mut Ui, on: bool, tip: &str) -> (Rect, Response) {
    let (rect, response) = ui.allocate_exact_size(vec2(28.0, 26.0), Sense::click());
    let fill = if on {
        ACCENT_SOFT
    } else if response.hovered() {
        SELECTION
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    (rect, response.on_hover_text(tip))
}

/// A tiny drawing of what each kind of mark makes.
fn kind_picture(painter: &Painter, area: Rect, kind: MarkKind, color: Color32) {
    let (a, b) = (area.left_bottom(), area.right_top());
    match kind {
        MarkKind::Text => {
            painter.text(area.center(), Align2::CENTER_CENTER, "T", theme::semibold(17.0), color);
        }
        MarkKind::Callout => {
            arrow_sized(painter, MarkKind::Arrow, area.left_bottom() + vec2(8.0, -5.0), b, color, 1.3, 7.0);
            let plate = Rect::from_min_size(area.left_bottom() - vec2(0.0, 9.0), vec2(13.0, 9.0));
            painter.rect_stroke(plate, 1.5, Stroke::new(1.2, color), StrokeKind::Inside);
        }
        MarkKind::Box | MarkKind::Rounded | MarkKind::Circle => shape(painter, kind, area, color, 1.4, false),
        _ => arrow_sized(painter, kind, a, b, color, 1.4, 8.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr() -> Tr {
        Tr { rect: Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0)), c: p(0.0, 0.0), s: 10.0, m: 1.0, shift: p(0.0, 0.0) }
    }

    fn mark(kind: MarkKind, fill: bool) -> Mark {
        Mark { kind, a: p(-5.0, -5.0), b: p(5.0, 5.0), color: COLORS[0], width: 2.5, fill, size: 18.0, text: String::new() }
    }

    #[test]
    fn an_empty_box_is_touched_on_its_edge_only() {
        let (t, m) = (tr(), mark(MarkKind::Box, false));
        assert!(touches(&m, &t, t.pt(p(5.0, 0.0))));
        assert!(!touches(&m, &t, t.pt(p(0.0, 0.0))));
        assert!(touches(&mark(MarkKind::Box, true), &t, t.pt(p(0.0, 0.0))));
    }

    #[test]
    fn a_circle_is_touched_on_its_ring() {
        let (t, m) = (tr(), mark(MarkKind::Circle, false));
        assert!(touches(&m, &t, t.pt(p(5.0, 0.0))));
        assert!(!touches(&m, &t, t.pt(p(0.0, 0.0))));
        assert!(!touches(&m, &t, t.pt(p(5.0, 5.0))), "the box's corner is outside the circle");
    }

    #[test]
    fn an_arrow_is_touched_along_its_length() {
        let (t, m) = (tr(), mark(MarkKind::Arrow, false));
        assert!(touches(&m, &t, t.pt(p(0.0, 0.0))));
        assert!(!touches(&m, &t, t.pt(p(4.0, -4.0))));
    }

    #[test]
    fn shift_makes_squares_and_straight_lines() {
        let m = mark(MarkKind::Box, false);
        assert_eq!(held(&m, p(1.0, 3.0), true), p(3.0, 3.0));
        let m = mark(MarkKind::Arrow, false);
        assert_eq!(held(&m, p(1.0, 3.0), true), p(-5.0, 3.0));
        assert_eq!(held(&m, p(9.0, -4.0), true), p(9.0, -5.0));
    }

    #[test]
    fn undo_brings_back_a_deleted_mark_and_empty_words_go() {
        let mut m = Markup::default();
        m.marks.push(mark(MarkKind::Arrow, false));
        m.selected = Some(0);
        assert!(m.delete_selected());
        assert!(m.marks.is_empty());
        m.step_back();
        assert_eq!(m.marks.len(), 1);

        m.marks.push(mark(MarkKind::Text, false));
        m.editing = Some(1);
        m.finish_editing();
        assert_eq!(m.marks.len(), 1, "nothing was typed");
    }
}
