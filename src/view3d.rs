//! The 3D view: turning the solid board around and its toolbar.

use crate::app::{App, ViewMode};
use crate::app::Side;
use crate::board3d::{self, Model};
use crate::gpu3d::{self, Shot};
use crate::icons::Icon;
use crate::panels::{segmented, tool_button};
use crate::theme::{self, FAINT, FG, LINE};
use eframe::egui::{self, Align2, Color32, CornerRadius, CursorIcon, PointerButton, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use glam::{Mat4, Vec3, Vec4};
use std::f32::consts::{FRAC_PI_2, PI};
use std::sync::Arc;

const FOV: f32 = 30.0 * PI / 180.0;
const TURN_PER_POINT: f32 = 0.008;

pub const BOARD_COLORS: [(&str, [f32; 3]); 6] = [
    ("Green", [0.04, 0.30, 0.17]),
    ("Black", [0.05, 0.05, 0.06]),
    ("Blue", [0.05, 0.14, 0.42]),
    ("Red", [0.46, 0.06, 0.07]),
    ("Purple", [0.24, 0.09, 0.38]),
    ("White", [0.78, 0.78, 0.76]),
];
const THICKNESSES: [(f32, &str); 4] = [(0.8, "0.8"), (1.0, "1.0"), (1.6, "1.6"), (2.0, "2.0")];

fn look_at(eye: Vec3, target: Vec3, up: Vec3) -> Mat4 {
    let f = (target - eye).normalize();
    let s = f.cross(up).normalize();
    let u = s.cross(f);
    Mat4::from_cols(
        Vec4::new(s.x, u.x, -f.x, 0.0),
        Vec4::new(s.y, u.y, -f.y, 0.0),
        Vec4::new(s.z, u.z, -f.z, 0.0),
        Vec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0),
    )
}

fn perspective(aspect: f32, near: f32, far: f32) -> Mat4 {
    let t = 1.0 / (FOV * 0.5).tan();
    Mat4::from_cols(
        Vec4::new(t / aspect, 0.0, 0.0, 0.0),
        Vec4::new(0.0, t, 0.0, 0.0),
        Vec4::new(0.0, 0.0, (far + near) / (near - far), -1.0),
        Vec4::new(0.0, 0.0, 2.0 * far * near / (near - far), 0.0),
    )
}

pub struct View3d {
    pub side: Side,
    pub yaw: f32,
    pub pitch: f32,
    pub dist: f32,
    pub target: Vec3,
    pub color: usize,
    pub thick: f32,
    /// Fill the room, and keep doing so as it changes, until zoomed or moved.
    pub auto_fit: bool,
    pub model: Option<Arc<Model>>,
}

impl View3d {
    fn eye(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target + Vec3::new(sy * cp, -cy * cp, sp) * self.dist
    }

    fn up(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(-sy * sp, cy * sp, cp)
    }

    fn matrix(&self, aspect: f32) -> Mat4 {
        perspective(aspect, self.dist * 0.02, self.dist * 12.0) * look_at(self.eye(), self.target, self.up())
    }

    fn look(&mut self, yaw: f32, pitch: f32) {
        self.yaw = yaw;
        self.pitch = pitch;
        self.auto_fit = true;
    }

    /// Backs off until every corner of the board is inside the room, which at
    /// an angle depends on which way it's turned.
    fn fit(&mut self, room: Vec2) {
        let Some(model) = self.model.clone() else { return };
        let (min, max) = (model.min, model.max);
        self.target = Vec3::new((min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, 0.0);
        let aspect = room.x / room.y.max(1.0);
        // The toolbars sit along the bottom, so less of the height is free.
        let (room_x, room_y) = (0.95, (1.0 - 150.0 / room.y.max(300.0)).max(0.5));
        self.dist = (max[0] - min[0]).max(max[1] - min[1]) * 2.0;
        let corners = [(min[0], min[1]), (max[0], min[1]), (max[0], max[1]), (min[0], max[1])];
        for _ in 0..6 {
            let view = self.matrix(aspect);
            let mut most: f32 = 0.0;
            for (x, y) in corners {
                for z in [-1.0, 1.0] {
                    let q = view * Vec4::new(x, y, z, 1.0);
                    most = most.max((q.x / q.w).abs() / room_x).max((q.y / q.w).abs() / room_y);
                }
            }
            self.dist = (self.dist * most).clamp(1.0, 5000.0);
        }
    }
}

/// Everything that makes up the look of a board: 3D needs these on, whatever
/// the flat view was showing.
const FACE_LAYERS: [&str; 6] = ["GTL", "GTS", "GTO", "GBL", "GBS", "GBO"];

impl App {
    pub fn enter_3d(&mut self) {
        let side = if self.mode == ViewMode::OnlyB && self.b.is_some() { Side::B } else { Side::A };
        self.flat_layers = self.slots.iter().map(|s| (s.key.clone(), s.visible)).collect();
        for slot in self.slots.iter_mut().filter(|s| FACE_LAYERS.contains(&s.key.as_str())) {
            slot.visible = true;
        }
        self.view3d = Some(View3d {
            side,
            yaw: -0.42,
            pitch: 0.92,
            dist: 100.0,
            target: Vec3::ZERO,
            color: 0,
            thick: 1.6,
            auto_fit: true,
            model: None,
        });
        self.fit_pending = true;
    }

    fn face_flags(&self, side: Side, keys: [&str; 3]) -> [bool; 3] {
        // The layer list works here too: a hidden layer comes off the board.
        keys.map(|key| self.slots.iter().any(|s| s.key == key && s.visible && s.layer(side).is_some()))
    }

    pub fn canvas_3d(&mut self, ui: &mut Ui, full: Rect) {
        let Some(mut v) = self.view3d.take() else { return };
        let Some(board) = self.board(v.side).cloned() else {
            self.notice = Some("There's no board to show in 3D.".to_string());
            return;
        };
        let wanted = board.layers.first().map(|l| l.id);
        if v.model.as_ref().map(|m| m.id) != wanted {
            v.model = board3d::build(&board).map(Arc::new);
            v.auto_fit = true;
            self.fit_pending = true;
            if v.model.as_ref().is_some_and(|m| !m.traced) {
                self.notice = Some("The board edge isn't a closed shape, so 3D uses a plain rectangle.".to_string());
            }
        }
        let Some(model) = v.model.clone() else {
            self.notice = Some("This board has no shape to show in 3D.".to_string());
            return;
        };
        if self.fit_pending {
            self.fit_pending = false;
            if v.auto_fit {
                v.fit(full.size());
            }
        }
        self.hover = None;
        self.hover_q = None;

        let response = ui.interact(full, ui.scope_id().with("board 3d"), Sense::click_and_drag());
        if response.dragged_by(PointerButton::Primary) {
            let d = response.drag_delta();
            v.yaw -= d.x * TURN_PER_POINT;
            v.pitch = (v.pitch + d.y * TURN_PER_POINT).clamp(-FRAC_PI_2, FRAC_PI_2);
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.dragged_by(PointerButton::Secondary) || response.dragged_by(PointerButton::Middle) {
            let d = response.drag_delta();
            let per_point = 2.0 * v.dist * (FOV * 0.5).tan() / full.height();
            let (forward, up) = ((v.target - v.eye()).normalize(), v.up());
            let right = forward.cross(up);
            v.target += (up * d.y - right * d.x) * per_point;
            v.auto_fit = false;
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if response.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        let (pinched, _) = self.pinch.take();
        if response.hovered() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta().y, i.zoom_delta()));
            let factor = zoom * pinched * (scroll * 0.003).exp();
            if (factor - 1.0).abs() > 1e-4 {
                v.dist = (v.dist / factor).clamp(1.0, 5000.0);
                v.auto_fit = false;
            }
        }

        let view = v.matrix(full.width() / full.height().max(1.0));
        let shot = Shot {
            view: view.to_cols_array(),
            eye: v.eye().to_array(),
            board: BOARD_COLORS[v.color].1,
            thick: v.thick,
            top_on: self.face_flags(v.side, ["GTL", "GTS", "GTO"]),
            bottom_on: self.face_flags(v.side, ["GBL", "GBS", "GBO"]),
        };
        ui.painter().add(gpu3d::callback(full, self.gpu.clone(), model, shot, self.gpu_error.clone()));
        if !self.clean {
            self.toolbar_3d(ui, full, &mut v);
        }
        self.view3d = Some(v);
    }

    fn toolbar_3d(&mut self, ui: &mut Ui, full: Rect, v: &mut View3d) {
        let two_boards = self.b.is_some();
        let mut leave = false;
        let mut save = false;
        egui::Area::new(ui.scope_id().with("tools 3d"))
            .pivot(Align2::CENTER_BOTTOM)
            .fixed_pos(pos2(full.center().x, full.bottom() - 12.0))
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ui.ctx(), |ui| {
                egui::Frame::new()
                    .fill(theme::CARD.gamma_multiply(0.96))
                    .stroke(Stroke::new(1.0, LINE))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(4.0, 6.0);
                        ui.horizontal(|ui| {
                            leave = tool_button(ui, Icon::Grid, "Flat view", true).on_hover_text("Back to the flat view (3)").clicked();
                            if tool_button(ui, Icon::Up, "Top", true).on_hover_text("Look straight down").clicked() {
                                v.look(0.0, FRAC_PI_2);
                                v.fit(full.size());
                            }
                            if tool_button(ui, Icon::Down, "Bottom", true).on_hover_text("Look at the underside").clicked() {
                                v.look(PI, -FRAC_PI_2);
                                v.fit(full.size());
                            }
                            if tool_button(ui, Icon::Cube, "Angle", true).on_hover_text("Look from an angle").clicked() {
                                v.look(-0.42, 0.92);
                                v.fit(full.size());
                            }
                            if tool_button(ui, Icon::Fit, "Fit", true).on_hover_text("Make the board fill the room (F)").clicked() {
                                v.auto_fit = true;
                                v.fit(full.size());
                            }
                            save = tool_button(ui, Icon::Picture, "Export PNG", true).on_hover_text("Save this view as a picture").clicked();
                        });
                        ui.horizontal(|ui| {
                            for (i, (name, color)) in BOARD_COLORS.iter().enumerate() {
                                let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                                let shown = color.map(|c| (c * 255.0).clamp(0.0, 255.0) as u8);
                                let fill = Color32::from_rgb(shown[0], shown[1], shown[2]);
                                ui.painter().circle_filled(rect.center(), 8.5, fill);
                                let on = v.color == i;
                                let edge = if on { FG } else if response.hovered() { FAINT } else { LINE };
                                ui.painter().circle_stroke(rect.center(), 10.0, Stroke::new(if on { 1.6 } else { 1.0 }, edge));
                                if response.on_hover_text(*name).clicked() {
                                    v.color = i;
                                }
                            }
                            ui.add_space(8.0);
                            ui.label(RichText::new("Thickness").size(13.0).color(FAINT));
                            segmented(ui, &mut v.thick, &THICKNESSES);
                            ui.label(RichText::new("mm").size(13.0).color(FAINT));
                            if two_boards {
                                ui.add_space(8.0);
                                segmented(ui, &mut v.side, &[(Side::A, "A"), (Side::B, "B")]);
                            }
                        });
                    });
            });
        if save {
            self.ask_save_picture(ui.ctx());
        }
        if leave {
            self.leave_3d(ui.ctx());
        }
    }

    /// Puts the layer list back the way the flat view had it.
    pub fn finish_leaving_3d(&mut self) {
        self.view3d = None;
        for (key, visible) in std::mem::take(&mut self.flat_layers) {
            if let Some(slot) = self.slots.iter_mut().find(|s| s.key == key) {
                slot.visible = visible;
            }
        }
    }

    pub fn leave_3d(&mut self, ctx: &egui::Context) {
        self.want_flat = true;
        self.fit_pending = self.fitted;
        ctx.request_repaint();
    }
}
