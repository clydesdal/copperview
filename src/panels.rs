//! The bars and side panels around the board view.

use crate::altium::Group;
use crate::app::{App, BACKGROUNDS, PAPER, Pin, Side, Slot, Unit, ViewMode, slot_status};
use crate::board::{self, Layer, PICK_NAMES};
use crate::compare::Status;
use crate::gerber::Feature;
use crate::gpu::Mix;
use crate::icons::{self, Icon};
use crate::theme::{self, ACCENT, ACCENT_SOFT, DIM, FAINT, FG, LINE, RAISED, SELECTION, semibold};
use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Rect, Response, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let inner = Frame::new()
        .fill(theme::CARD)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(theme::CARD_RADIUS))
        .inner_margin(egui::Margin::same(14))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner;
    ui.add_space(10.0);
    inner
}

fn heading(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(14.0).color(DIM));
    ui.add_space(2.0);
}

/// A label on the left and a value on the right.
fn row(ui: &mut Ui, key: &str, value: &str) {
    row_font(ui, key, value, theme::mono(12.5), FG);
}

fn row_font(ui: &mut Ui, key: &str, value: &str, font: FontId, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new(key).color(DIM));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let text = RichText::new(value).font(font).color(color);
            ui.add(egui::Label::new(text).truncate());
        });
    });
}

/// A pill-shaped choice of a few options, as at the bottom of Craft Anything.
pub fn segmented<T: Copy + PartialEq>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) -> bool {
    let font = FontId::proportional(13.5);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, t)| ui.painter().layout_no_wrap(t.to_string(), font.clone(), FG).size().x + 20.0)
        .collect();
    let total: f32 = widths.iter().sum::<f32>() + 6.0;
    let (rect, _) = ui.allocate_exact_size(vec2(total, 30.0), Sense::hover());
    let painter = ui.painter_at(rect.expand(1.0));
    painter.rect(rect, 9.0, theme::WELL, Stroke::new(1.0, LINE), StrokeKind::Inside);
    let mut x = rect.left() + 3.0;
    let mut changed = false;
    for (i, (option, text)) in options.iter().enumerate() {
        let seg = Rect::from_min_size(pos2(x, rect.top() + 3.0), vec2(widths[i], rect.height() - 6.0));
        x += widths[i];
        let response = ui.interact(seg, ui.scope_id().with(("seg", i, *text)), Sense::click());
        let on = *value == *option;
        if on {
            painter.rect_filled(seg, 7.0, SELECTION);
        } else if response.hovered() {
            painter.rect_filled(seg, 7.0, RAISED);
        }
        let color = if on || response.hovered() { FG } else { DIM };
        painter.text(seg.center(), Align2::CENTER_CENTER, *text, font.clone(), color);
        if response.clicked() && !on {
            *value = *option;
            changed = true;
        }
    }
    changed
}

/// An outlined button with an icon and words.
pub fn tool_button(ui: &mut Ui, icon: Icon, text: &str, enabled: bool) -> Response {
    let font = FontId::proportional(14.0);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, FG);
    let size = vec2(galley.size().x + 42.0, 34.0);
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, response) = ui.allocate_exact_size(size, sense);
    let hot = enabled && response.hovered();
    let fill = if hot { SELECTION } else { Color32::TRANSPARENT };
    ui.painter().rect(rect, theme::BUTTON_RADIUS, fill, Stroke::new(1.0, LINE), StrokeKind::Inside);
    let color = if enabled { FG } else { FAINT };
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 20.0, rect.center().y), Vec2::splat(16.0));
    icons::draw(ui.painter(), icon_rect, icon, color);
    ui.painter().galley(pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0), galley, color);
    response
}

pub fn segmented_tool(ui: &mut Ui, icon: Icon, text: &str, on: bool) -> Response {
    let font = FontId::proportional(13.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, FG);
    let (rect, response) = ui.allocate_exact_size(vec2(galley.size().x + 36.0, 30.0), Sense::click());
    let fill = if on {
        ACCENT_SOFT
    } else if response.hovered() {
        RAISED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let color = if on { ACCENT } else if response.hovered() { FG } else { DIM };
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 17.0, rect.center().y), Vec2::splat(15.0));
    icons::draw(ui.painter(), icon_rect, icon, color);
    ui.painter().galley(pos2(rect.left() + 28.0, rect.center().y - galley.size().y / 2.0), galley, color);
    response
}

pub fn icon_button(ui: &mut Ui, icon: Icon, on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let fill = if on {
        ACCENT_SOFT
    } else if response.hovered() {
        RAISED
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    let color = if on { ACCENT } else if response.hovered() { FG } else { DIM };
    icons::draw(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(15.0)), icon, color);
    response
}

fn small_icon(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 6.0, RAISED);
    }
    let color = if response.hovered() { FG } else { FAINT };
    icons::draw(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(14.0)), icon, color);
    response.on_hover_text(tip)
}

fn chip(ui: &mut Ui, text: &str) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, FG);
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(14.0, 8.0), Sense::click());
    let fill = if response.hovered() { SELECTION } else { RAISED };
    ui.painter().rect_filled(rect, 7.0, fill);
    let color = if response.hovered() { FG } else { DIM };
    ui.painter().galley(rect.min + vec2(7.0, 4.0), galley, color);
    response
}

fn rail_button(ui: &mut Ui, text: &str, on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let fill = if on {
        ACCENT_SOFT
    } else if response.hovered() {
        SELECTION
    } else {
        RAISED
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    let color = if on { ACCENT } else if response.hovered() { FG } else { DIM };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(12.5), color);
    response
}

fn rail_chip(ui: &mut Ui, text: &str, on: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(68.0, 28.0), Sense::click());
    let fill = if on {
        ACCENT_SOFT
    } else if response.hovered() {
        SELECTION
    } else {
        RAISED
    };
    ui.painter().rect_filled(rect, 7.0, fill);
    let color = if on { ACCENT } else { DIM };
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(13.0), color);
    response
}

/// A few letters that say which layer this is: Altium's ending for most,
/// and the kind of hole for drill files, which all end in the same way.
fn rail_tag(slot: &Slot) -> String {
    let key = slot.key.split('#').next().unwrap_or(&slot.key);
    let tag = match key {
        "PTH-SLOT" => "SLOT",
        "NPTH-SLOT" => "NSLT",
        "DRILL" => "DRL",
        _ if key.starts_with("FILE:") => slot.tag.as_str(),
        _ => key,
    };
    tag.chars().take(4).collect()
}

fn status_look(status: Status) -> (Color32, String) {
    match status {
        Status::Identical => (FAINT, "Identical".to_string()),
        Status::SameDrawing => (FAINT, "Same drawing".to_string()),
        Status::Changed { removed, added } => (theme::YELLOW, format!("−{removed} +{added}")),
        Status::OnlyA => (theme::RED, "Only in A".to_string()),
        Status::OnlyB => (theme::GREEN, "Only in B".to_string()),
    }
}

impl App {
    pub fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_height(30.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(2.0);
                let icon = if self.show_right { Icon::Right } else { Icon::Left };
                if small_icon(ui, icon, "Show or hide the details").clicked() {
                    self.show_right = !self.show_right;
                }
                let mut unit = self.unit;
                if segmented(ui, &mut unit, &[(Unit::Inch, "in"), (Unit::Mil, "mil"), (Unit::Mm, "mm")]) {
                    self.unit = unit;
                }
                if self.b.is_some() {
                    ui.add_space(10.0);
                    let mut mode = self.mode;
                    let options = [
                        (ViewMode::Swipe, "Swipe"),
                        (ViewMode::Diff, "Differences"),
                        (ViewMode::Stack, "Upper / lower"),
                        (ViewMode::Side, "Side by side"),
                        (ViewMode::OnlyB, "B"),
                        (ViewMode::OnlyA, "A"),
                    ];
                    if segmented(ui, &mut mode, &options) {
                        let before = self.pane_size(self.canvas_rect);
                        self.mode = mode;
                        // Choosing a flat view leaves 3D.
                        self.want_flat = true;
                        if before != self.pane_size(self.canvas_rect) {
                            self.fit_pending = true;
                        }
                    }
                }
                // What's left over goes to the name, which shortens to fit.
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let icon = if self.show_left { Icon::Left } else { Icon::Right };
                    if small_icon(ui, icon, "Fold the layer list down to its buttons, or open it").clicked() {
                        self.show_left = !self.show_left;
                    }
                    let (rect, logo_hit) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                    if let Some(logo) = &self.logo {
                        let whole = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                        ui.painter().image(logo.id(), rect, whole, Color32::WHITE);
                    }
                    let name = ui.add(egui::Label::new(RichText::new("Copperview").font(semibold(15.0))).sense(Sense::click()));
                    if logo_hit.on_hover_text("About Copperview").clicked() || name.on_hover_text("About Copperview").clicked() {
                        self.show_about = true;
                    }
                    ui.add_space(6.0);
                    let title = match (&self.a, &self.b) {
                        (Some(a), Some(b)) => format!("{}  and  {}", a.name, b.name),
                        (Some(a), None) => a.name.clone(),
                        _ => String::new(),
                    };
                    if ui.available_width() > 60.0 {
                        ui.add(egui::Label::new(RichText::new(title).color(DIM)).truncate());
                    }
                });
            });
        });
    }

    /// The About window: the name, the version and what each version changed.
    pub fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.show_about = false;
            return;
        }
        let mut close = false;
        let frame = Frame::new()
            .fill(theme::CARD)
            .stroke(Stroke::new(1.0, LINE))
            .corner_radius(theme::CARD_RADIUS)
            .inner_margin(egui::Margin::same(16));
        egui::Window::new("About Copperview")
            .title_bar(false)
            .frame(frame)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .fixed_size(vec2(440.0, 440.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Copperview").font(semibold(20.0)).color(FG));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if small_icon(ui, Icon::Close, "Close (Esc)").clicked() {
                            close = true;
                        }
                    });
                });
                ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(theme::mono(13.0)).color(ACCENT));
                ui.add_space(4.0);
                ui.label(RichText::new("Look at PCB Gerber files, measure anything you click, and compare two sets.").color(DIM));
                ui.add_space(8.0);
                ui.separator();
                ui.label(RichText::new("What each version changed").color(FAINT));
                ScrollArea::vertical().id_salt("about log").max_height(320.0).auto_shrink([false, false]).show(ui, |ui| {
                    for line in include_str!("../CHANGELOG.md").lines().skip_while(|l| !l.starts_with("## ")) {
                        if let Some(v) = line.strip_prefix("## ") {
                            ui.add_space(8.0);
                            ui.label(RichText::new(format!("Version {v}")).font(semibold(15.0)).color(FG));
                        } else if let Some(h) = line.strip_prefix("### ") {
                            ui.add_space(4.0);
                            ui.label(RichText::new(h).size(13.0).color(ACCENT));
                        } else if let Some(item) = line.strip_prefix("- ") {
                            ui.label(RichText::new(format!("•  {item}")).size(13.0).color(FG));
                        }
                    }
                });
            });
        if close {
            self.show_about = false;
        }
    }

    pub fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.set_height(18.0);
            let u = self.unit;
            if let Some((side, q)) = self.hover_q {
                let at = q.sub(self.origin);
                let text = format!("X {}  Y {} {}", u.num(at.x), u.num(at.y), u.name());
                ui.label(RichText::new(text).font(theme::mono(12.0)).color(DIM));
                if self.b.is_some() {
                    ui.label(RichText::new(side.letter()).font(theme::mono(12.0)).color(FAINT));
                }
                if self.origin.len() > 0.0 && ui.small_button("Reset origin").clicked() {
                    self.origin = Default::default();
                }
            }
            if let Some(t) = self.hover
                && let Some((layer, f)) = self.feature(&t)
            {
                let text = format!("{} on {}: {}", f.kind.label(), layer.role.label, self.size_text(layer, f));
                ui.label(RichText::new(text).font(theme::mono(12.0)).color(FAINT));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(notice) = self.notice.clone() {
                    if small_icon(ui, Icon::Close, "Dismiss").clicked() {
                        self.notice = None;
                    }
                    ui.add(egui::Label::new(RichText::new(notice).size(12.5).color(theme::YELLOW)).truncate());
                } else if self.loading() {
                    ui.spinner();
                    ui.label(RichText::new("Working…").size(12.5).color(DIM));
                } else if self.view3d.is_some() {
                    let hint = "Drag to turn · Right-drag to move · Scroll or pinch to zoom";
                    ui.label(RichText::new(hint).size(12.5).color(FAINT));
                } else if self.selected_pin.is_some() || self.hover_pin.is_some() {
                    let hint = if self.selected_pin.is_some() {
                        "Delete or the red button removes this measurement"
                    } else {
                        "Click to select this measurement"
                    };
                    ui.label(RichText::new(hint).size(12.5).color(FAINT));
                } else if self.hover.is_none() {
                    let hint = match self.tool {
                        crate::app::Tool::Select => "Click to measure · Drag to move · Scroll to zoom",
                        crate::app::Tool::Measure => "Click two points · Shift keeps it straight · Esc cancels",
                        crate::app::Tool::Markup => "Drag to draw · Click a mark to select it · Right-drag to move the board",
                    };
                    ui.label(RichText::new(hint).size(12.5).color(FAINT));
                }
            });
        });
    }

    fn source_row(&mut self, ui: &mut Ui, side: Side) {
        let name = self.board(side).map(|b| (b.name.clone(), b.is_zip));
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let (badge, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            let color = if side == Side::A { ACCENT } else { theme::YELLOW };
            ui.painter().rect_filled(badge, 6.0, color.gamma_multiply(0.16));
            ui.painter().text(badge.center(), Align2::CENTER_CENTER, side.letter(), semibold(12.5), color);
            ui.add_space(6.0);
            let width = (ui.available_width() - 80.0).max(40.0);
            let (rect, response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
            let (text, color) = match &name {
                Some((n, _)) => (n.as_str(), FG),
                None => ("Add one to compare", FAINT),
            };
            let job = egui::text::LayoutJob::simple_singleline(text.to_string(), FontId::proportional(14.0), color);
            let mut job = job;
            job.wrap = egui::text::TextWrapping::truncate_at_width(width);
            let galley = ui.painter().layout_job(job);
            ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, color);
            if name.is_some() {
                response.on_hover_text(text.to_string());
            }
            if small_icon(ui, Icon::Folder, "Open a folder here").clicked() {
                self.ask_folder(side, &ctx);
            }
            if small_icon(ui, Icon::Zip, "Open a zip here").clicked() {
                self.ask_zip(side, &ctx);
            }
            if name.is_some() {
                if small_icon(ui, Icon::Close, "Close this one").clicked() {
                    self.close(side);
                }
            } else {
                ui.add_space(24.0);
            }
        });
    }

    pub fn left_panel(&mut self, ui: &mut Ui) {
        self.source_row(ui, Side::A);
        self.source_row(ui, Side::B);
        self.other_zips(ui);
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(4.0);

        ui.label(RichText::new("Layers").color(DIM));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for name in ["All", "Top", "Bottom", "Copper", "None"] {
                if chip(ui, name).clicked() {
                    self.preset(name);
                }
            }
        });
        ui.add_space(4.0);

        let bottom_height = 136.0;
        let room = (ui.available_height() - bottom_height).max(80.0);
        ScrollArea::vertical().id_salt("list").max_height(room).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            self.diff_row(ui);
            self.markup_row(ui);
            let mut last = None;
            for i in 0..self.slots.len() {
                let group = self.slots[i].group;
                if last != Some(group) {
                    last = Some(group);
                    ui.add_space(6.0);
                    ui.label(RichText::new(group.label()).size(12.5).color(FAINT));
                }
                self.slot_row(ui, i);
                if self.active == Some(i) {
                    self.slot_details(ui, i);
                }
            }
            self.side_files(ui);
        });

        ui.add_space(6.0);
        ui.separator();
        ui.add_space(2.0);
        ui.label(RichText::new("Color mixing").color(DIM));
        let mut mix = self.mix;
        let options = [(Mix::Normal, "See-through"), (Mix::Light, "Light"), (Mix::Ink, "Ink on paper")];
        if segmented(ui, &mut mix, &options) {
            self.set_mix(mix);
        }
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("All layers").color(DIM));
            ui.spacing_mut().slider_width = 100.0;
            ui.add(egui::Slider::new(&mut self.master, 0.1..=1.0).show_value(false));
            ui.add_space(2.0);
            for (i, (name, color)) in BACKGROUNDS.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
                let on = self.bg == i;
                ui.painter().circle_filled(rect.center(), 8.0, *color);
                let edge = if on { ACCENT } else if response.hovered() { FG } else { LINE };
                ui.painter().circle_stroke(rect.center(), 8.5, Stroke::new(1.3, edge));
                if response.on_hover_text(*name).clicked() {
                    self.bg = i;
                    if i == PAPER && self.mix == Mix::Light {
                        self.mix = Mix::Normal;
                    }
                    if i != PAPER && self.mix == Mix::Ink {
                        self.mix = Mix::Normal;
                    }
                }
            }
        });
    }

    /// Other zips or revisions found alongside what's open, to pick from.
    fn other_zips(&mut self, ui: &mut Ui) {
        let open_one = self.choices.len() == 1 && self.a.as_ref().is_some_and(|a| a.path == self.choices[0].path);
        if self.choices.is_empty() || open_one {
            return;
        }
        ui.add_space(2.0);
        ui.label(RichText::new("Also here").size(12.5).color(FAINT));
        let list = self.choices.clone();
        for choice in list.iter().take(12) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let short = choice.name.rsplit('/').next().unwrap_or(&choice.name);
                let width = (ui.available_width() - 60.0).max(40.0);
                ui.allocate_ui_with_layout(vec2(width, 18.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.set_width(width);
                    ui.add(egui::Label::new(RichText::new(short).size(13.0).color(DIM)).truncate());
                });
                if chip(ui, "A").on_hover_text("Open as A").clicked() {
                    self.open_set(Side::A, choice.path.clone(), choice.set.clone());
                }
                if chip(ui, "B").on_hover_text("Compare as B").clicked() {
                    self.open_set(Side::B, choice.path.clone(), choice.set.clone());
                }
            });
        }
        if list.len() > 12 {
            ui.label(RichText::new(format!("and {} more", list.len() - 12)).size(12.5).color(FAINT));
        }
    }

    /// The menu of what a click can pick.
    pub fn pick_menu(&mut self, ui: &mut Ui) {
        ui.set_width(292.0);
        ui.label(RichText::new("A click can pick").size(13.0).color(DIM));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (i, name) in PICK_NAMES.iter().enumerate() {
                let on = self.rules.on[i];
                if rail_chip(ui, name, on).on_hover_text("Click to turn on or off").clicked() {
                    self.rules.on[i] = !on;
                    if on && self.rules.first == Some(i) {
                        self.rules.first = None;
                    }
                }
            }
        });
        ui.add_space(6.0);
        ui.label(RichText::new("When things are stacked, pick first").size(13.0).color(DIM));
        let mut first = self.rules.first;
        let options = [
            (None, "Auto"),
            (Some(0), PICK_NAMES[0]),
            (Some(1), PICK_NAMES[1]),
            (Some(2), PICK_NAMES[2]),
            (Some(3), PICK_NAMES[3]),
        ];
        if segmented(ui, &mut first, &options) {
            self.rules.first = first;
            if let Some(i) = first {
                self.rules.on[i] = true;
            }
        }
        ui.add_space(4.0);
        ui.label(RichText::new("Auto picks small things before the fills around them. Click the same spot again to step through what's stacked there.").size(12.0).color(FAINT));
        if !self.rules.is_default() {
            ui.add_space(4.0);
            if chip(ui, "Reset").clicked() {
                self.rules = Default::default();
            }
        }
    }

    /// The markup layer: it joins the list once there's a mark on the board.
    fn diff_count(&self) -> usize {
        self.comparison.as_ref().map_or(0, |c| c.changes.len())
    }

    /// The differences, as a layer of their own that can be shown or hidden.
    fn diff_row(&mut self, ui: &mut Ui) {
        let count = self.diff_count();
        if count == 0 {
            return;
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Compare").size(12.5).color(FAINT));
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        let painter = ui.painter().clone();
        if response.hovered() {
            painter.rect_filled(rect, 8.0, RAISED);
        }
        let eye = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(24.0));
        let eye_response = ui.interact(eye, ui.scope_id().with("diff eye"), Sense::click());
        let visible = self.show_diffs;
        let eye_color = if visible { FG } else if eye_response.hovered() { DIM } else { FAINT };
        icons::draw(&painter, Rect::from_center_size(eye.center(), Vec2::splat(15.0)), if visible { Icon::Eye } else { Icon::EyeOff }, eye_color);
        let swatch = Rect::from_center_size(pos2(rect.left() + 39.0, rect.center().y), Vec2::splat(13.0));
        painter.rect_filled(swatch, 4.0, Color32::from_rgb(0xff, 0xe6, 0x00).gamma_multiply(if visible { 1.0 } else { 0.2 }));
        painter.rect_stroke(swatch, 4.0, Stroke::new(1.0, Color32::from_rgb(0xff, 0xe6, 0x00)), StrokeKind::Inside);
        painter.text(pos2(rect.left() + 54.0, rect.center().y), Align2::LEFT_CENTER, "Differences", FontId::proportional(14.0), if visible { FG } else { DIM });
        painter.text(pos2(rect.right() - 8.0, rect.center().y), Align2::RIGHT_CENTER, count.to_string(), theme::mono(11.0), FAINT);
        if eye_response.on_hover_text("Show or hide the difference markers").clicked() || response.clicked() {
            self.show_diffs = !visible;
        }
    }

    fn rail_diffs(&mut self, ui: &mut Ui) {
        if self.diff_count() == 0 {
            return;
        }
        let visible = self.show_diffs;
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 7.0, RAISED);
        }
        let swatch = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), Vec2::splat(11.0));
        let yellow = Color32::from_rgb(0xff, 0xe6, 0x00);
        if visible {
            ui.painter().rect_filled(swatch, 3.0, yellow);
        }
        ui.painter().rect_stroke(swatch, 3.0, Stroke::new(1.0, yellow.gamma_multiply(0.7)), StrokeKind::Inside);
        ui.painter().text(pos2(rect.left() + 20.0, rect.center().y), Align2::LEFT_CENTER, "DIFF", theme::mono(10.5), if visible { FG } else { FAINT });
        if response.on_hover_text("Differences. Click to show or hide.").clicked() {
            self.show_diffs = !visible;
        }
        ui.add_space(3.0);
    }

    fn markup_row(&mut self, ui: &mut Ui) {
        let count = self.markup.marks.len();
        if count == 0 {
            self.markup_open = false;
            return;
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Markup").size(12.5).color(FAINT));
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
        let painter = ui.painter().clone();
        if self.markup_open {
            painter.rect_filled(rect, 8.0, SELECTION);
        } else if response.hovered() {
            painter.rect_filled(rect, 8.0, RAISED);
        }
        let eye = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(24.0));
        let eye_response = ui.interact(eye, ui.scope_id().with("markup eye"), Sense::click());
        let visible = self.markup.visible;
        let eye_color = if visible { FG } else if eye_response.hovered() { DIM } else { FAINT };
        let icon = if visible { Icon::Eye } else { Icon::EyeOff };
        icons::draw(&painter, Rect::from_center_size(eye.center(), Vec2::splat(15.0)), icon, eye_color);
        if eye_response.on_hover_text("Show or hide the markup").clicked() {
            self.markup.visible = !visible;
        } else if response.clicked() {
            self.markup_open = !self.markup_open;
        }
        let tint = self.markup.marks.last().map_or(FG, |m| m.color);
        let pen = Rect::from_center_size(pos2(rect.left() + 39.0, rect.center().y), Vec2::splat(14.0));
        icons::draw(&painter, pen, Icon::Pen, if visible { tint } else { FAINT });
        let text_color = if visible { FG } else { DIM };
        let at = pos2(rect.left() + 54.0, rect.center().y);
        painter.text(at, Align2::LEFT_CENTER, "Markups", FontId::proportional(14.0), text_color);
        let right = pos2(rect.right() - 8.0, rect.center().y);
        painter.text(right, Align2::RIGHT_CENTER, count.to_string(), theme::mono(11.0), FAINT);

        if self.markup_open {
            let inner = ui.available_width();
            Frame::new().inner_margin(egui::Margin { left: 8, right: 6, top: 4, bottom: 8 }).show(ui, |ui| {
                ui.set_width(inner - 14.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal(|ui| {
                    ui.label(RichText::new("See-through").size(13.0).color(DIM));
                    ui.spacing_mut().slider_width = 104.0;
                    ui.add(egui::Slider::new(&mut self.markup.alpha, 0.1..=1.0).show_value(false));
                    let shown = format!("{:.0}%", self.markup.alpha * 100.0);
                    ui.label(RichText::new(shown).font(theme::mono(11.5)).color(FAINT));
                });
                ui.horizontal(|ui| {
                    if chip(ui, "Draw more").on_hover_text("Turn on the Mark up tool").clicked() {
                        self.tool = crate::app::Tool::Markup;
                        self.ruler_start = None;
                    }
                    if chip(ui, "Remove all").on_hover_text("Ctrl+Z brings them back").clicked() {
                        self.markup.remove_all();
                    }
                });
            });
        }
    }

    fn rail_markup(&mut self, ui: &mut Ui) {
        if self.markup.marks.is_empty() {
            return;
        }
        let visible = self.markup.visible;
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 7.0, RAISED);
        }
        let tint = self.markup.marks.last().map_or(FG, |m| m.color);
        let pen = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), Vec2::splat(12.0));
        icons::draw(ui.painter(), pen, Icon::Pen, if visible { tint } else { FAINT });
        let at = pos2(rect.left() + 20.0, rect.center().y);
        ui.painter().text(at, Align2::LEFT_CENTER, "MARK", theme::mono(10.5), if visible { FG } else { FAINT });
        let tip = format!("Markups: {}\nClick to show or hide.", self.markup.marks.len());
        if response.on_hover_text(tip).clicked() {
            self.markup.visible = !visible;
        }
        ui.add_space(3.0);
    }

    fn toggle_slot(&mut self, i: usize, only: bool) {
        if only {
            let already = self.slots.iter().enumerate().all(|(n, s)| s.visible == (n == i));
            for (n, s) in self.slots.iter_mut().enumerate() {
                s.visible = already || n == i;
            }
        } else {
            self.slots[i].visible = !self.slots[i].visible;
        }
    }

    /// The layer list folded small: every layer and mixing button, without the words.
    pub fn left_rail(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 3.0;
        for (name, short, tip) in [
            ("All", "All", "Show every layer"),
            ("Top", "Top", "Show the top side"),
            ("Bottom", "Bot", "Show the bottom side"),
            ("Copper", "Cu", "Show copper and holes"),
            ("None", "None", "Hide every layer"),
        ] {
            if rail_button(ui, short, false).on_hover_text(tip).clicked() {
                self.preset(name);
            }
        }
        ui.add_space(3.0);
        ui.separator();
        let room = (ui.available_height() - 96.0).max(60.0);
        ScrollArea::vertical().id_salt("rail").max_height(room).auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.spacing_mut().scroll.bar_width = 4.0;
            self.rail_diffs(ui);
            self.rail_markup(ui);
            for i in 0..self.slots.len() {
                self.rail_layer(ui, i);
            }
        });
        ui.add_space(3.0);
        ui.separator();
        for (mix, short, tip) in [
            (Mix::Normal, "See", "Color mixing: see-through"),
            (Mix::Light, "Light", "Color mixing: light"),
            (Mix::Ink, "Ink", "Color mixing: ink on paper"),
        ] {
            if rail_button(ui, short, self.mix == mix).on_hover_text(tip).clicked() {
                self.set_mix(mix);
            }
        }
    }

    fn rail_layer(&mut self, ui: &mut Ui, i: usize) {
        let slot = self.slots[i].clone();
        let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(rect, 7.0, RAISED);
        }
        let color = Color32::from_rgb(slot.color[0], slot.color[1], slot.color[2]);
        let swatch = Rect::from_center_size(pos2(rect.left() + 10.0, rect.center().y), Vec2::splat(11.0));
        if slot.visible {
            ui.painter().rect_filled(swatch, 3.0, color);
        }
        ui.painter().rect_stroke(swatch, 3.0, Stroke::new(1.0, color.gamma_multiply(0.7)), StrokeKind::Inside);
        let short = rail_tag(&slot);
        let text_color = if slot.visible { FG } else { FAINT };
        let at = pos2(rect.left() + 20.0, rect.center().y);
        ui.painter().text(at, Align2::LEFT_CENTER, short, theme::mono(10.5), text_color);
        let status = slot_status(self, &slot);
        if status.is_some_and(Status::differs) {
            ui.painter().circle_filled(rect.right_top() + vec2(-4.0, 5.0), 2.5, theme::YELLOW);
        }
        let mut tip = slot.label.clone();
        if let Some(status) = status {
            tip.push_str(&format!("  ·  {}", status_look(status).1));
        }
        tip.push_str("\nClick to show or hide. Shift-click to show only this one.");
        if response.on_hover_text(tip).clicked() {
            self.toggle_slot(i, ui.input(|input| input.modifiers.shift));
        }
    }

    fn slot_row(&mut self, ui: &mut Ui, i: usize) {
        let slot = self.slots[i].clone();
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
        let active = self.active == Some(i);
        let painter = ui.painter().clone();
        if active {
            painter.rect_filled(rect, 8.0, SELECTION);
        } else if response.hovered() {
            painter.rect_filled(rect, 8.0, RAISED);
        }

        let eye = Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(24.0));
        let eye_response = ui.interact(eye, ui.scope_id().with(("eye", &slot.key)), Sense::click());
        let eye_color = if slot.visible { FG } else if eye_response.hovered() { DIM } else { FAINT };
        let icon = if slot.visible { Icon::Eye } else { Icon::EyeOff };
        icons::draw(&painter, Rect::from_center_size(eye.center(), Vec2::splat(15.0)), icon, eye_color);
        if eye_response.on_hover_text("Show or hide. Shift-click to show only this one.").clicked() {
            self.toggle_slot(i, ui.input(|input| input.modifiers.shift));
        } else if response.clicked() {
            self.active = if active { None } else { Some(i) };
        }

        let c = slot.color;
        let swatch = Rect::from_center_size(pos2(rect.left() + 39.0, rect.center().y), Vec2::splat(13.0));
        let alpha = if slot.visible { slot.alpha.max(0.25) } else { 0.15 };
        painter.rect_filled(swatch, 4.0, Color32::from_rgb(c[0], c[1], c[2]).gamma_multiply(alpha));
        painter.rect_stroke(swatch, 4.0, Stroke::new(1.0, Color32::from_rgb(c[0], c[1], c[2])), StrokeKind::Inside);

        let text_color = if slot.visible { FG } else { DIM };
        let right_edge = rect.right() - 8.0;
        let mut text_limit = right_edge;
        if let Some(status) = slot_status(self, &slot) {
            let (dot, words) = status_look(status);
            let changed = status.differs();
            if changed {
                let galley = painter.layout_no_wrap(words, theme::mono(11.0), dot);
                let at = pos2(right_edge - galley.size().x, rect.center().y - galley.size().y / 2.0);
                text_limit = at.x - 6.0;
                painter.galley(at, galley, dot);
            } else {
                painter.circle_filled(pos2(right_edge - 3.0, rect.center().y), 2.5, FAINT.gamma_multiply(0.7));
                text_limit = right_edge - 12.0;
            }
            let _ = ui.interact(rect, ui.scope_id().with(("tip", &slot.key)), Sense::hover()).on_hover_text(status_look(status).1);
        } else {
            let galley = painter.layout_no_wrap(slot.tag.clone(), theme::mono(11.0), FAINT);
            let at = pos2(right_edge - galley.size().x, rect.center().y - galley.size().y / 2.0);
            text_limit = text_limit.min(at.x - 6.0);
            painter.galley(at, galley, FAINT);
        }
        let start = rect.left() + 54.0;
        let mut job = egui::text::LayoutJob::simple_singleline(slot.label.clone(), FontId::proportional(14.0), text_color);
        job.wrap = egui::text::TextWrapping::truncate_at_width((text_limit - start).max(20.0));
        let galley = painter.layout_job(job);
        painter.galley(pos2(start, rect.center().y - galley.size().y / 2.0), galley, text_color);
    }

    fn slot_details(&mut self, ui: &mut Ui, i: usize) {
        let inner = ui.available_width();
        Frame::new().inner_margin(egui::Margin { left: 8, right: 6, top: 4, bottom: 8 }).show(ui, |ui| {
            ui.set_width(inner - 14.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new("See-through").size(13.0).color(DIM));
                ui.spacing_mut().slider_width = 104.0;
                ui.add(egui::Slider::new(&mut self.slots[i].alpha, 0.05..=1.0).show_value(false));
                ui.label(RichText::new(format!("{:.0}%", self.slots[i].alpha * 100.0)).font(theme::mono(11.5)).color(FAINT));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("Color").size(13.0).color(DIM));
                ui.color_edit_button_srgb(&mut self.slots[i].color);
                ui.checkbox(&mut self.slots[i].invert, "Invert").on_hover_text(
                    "Show the empty areas instead. Useful for solder mask: the openings become gaps in a solid color.",
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if small_icon(ui, Icon::Down, "Move down the stack").clicked() && i + 1 < self.slots.len() {
                        self.slots.swap(i, i + 1);
                        self.active = Some(i + 1);
                    }
                    if small_icon(ui, Icon::Up, "Move up the stack").clicked() && i > 0 {
                        self.slots.swap(i, i - 1);
                        self.active = Some(i - 1);
                    }
                });
            });
        });
    }

    fn side_files(&self, ui: &mut Ui) {
        let mut files: Vec<(String, String)> = Vec::new();
        for board in [&self.a, &self.b].into_iter().flatten() {
            for f in &board.side_files {
                if !files.iter().any(|(n, _)| *n == f.name) {
                    files.push((f.name.clone(), f.note.clone()));
                }
            }
        }
        if files.is_empty() {
            return;
        }
        ui.add_space(6.0);
        egui::CollapsingHeader::new(RichText::new(format!("{} other files", files.len())).size(12.5).color(FAINT))
            .id_salt("others")
            .show(ui, |ui| {
                for (name, note) in files {
                    ui.add(egui::Label::new(RichText::new(name).size(12.5).color(DIM)).truncate()).on_hover_text(note);
                }
            });
    }

    pub fn right_panel(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().id_salt("details").auto_shrink([false, false]).show(ui, |ui| {
            self.board_card(ui);
            self.clicked_card(ui);
            self.ruler_card(ui);
            self.pins_card(ui);
            self.compare_card(ui);
            self.layer_card(ui);
            self.install_card(ui);
        });
    }

    /// Offers to install, until it's done. The check run's picture leaves it out.
    fn install_card(&mut self, ui: &mut Ui) {
        if !crate::platform::CAN_INSTALL || self.installed || self.launch.screenshot.is_some() {
            return;
        }
        card(ui, |ui| {
            ui.label(RichText::new("Keep Copperview").font(semibold(15.5)));
            ui.label(RichText::new("Adds it to your Applications folder and app launcher.").size(13.0).color(DIM));
            ui.add_space(4.0);
            if tool_button(ui, Icon::Down, "Install", true).clicked() {
                match crate::platform::install() {
                    Ok(()) => {
                        self.installed = true;
                        self.notice = Some("Installed. Find Copperview in your app launcher.".to_string());
                    }
                    Err(error) => self.notice = Some(error),
                }
            }
        });
    }

    fn board_card(&mut self, ui: &mut Ui) {
        let Some(a) = self.a.clone() else { return };
        let two = self.b.is_some();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let (tile, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                ui.painter().rect_filled(tile, 10.0, ACCENT_SOFT);
                let (s, c) = (tile.width() / 16.0 * 0.62, tile.center());
                let at = Rect::from_center_size(c, Vec2::splat(16.0 * s));
                icons::draw(ui.painter(), at, if a.is_zip { Icon::Zip } else { Icon::Folder }, ACCENT);
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 3.0;
                    let title = if self.sample { "Sample board" } else { a.name.as_str() };
                    ui.add(egui::Label::new(RichText::new(title).font(semibold(15.5))).truncate());
                    let tag = match (self.sample, two) {
                        (true, _) => "Made up, to show the app".to_string(),
                        (_, true) => "Comparing two sets".to_string(),
                        _ if a.layers.len() == 1 => "1 layer".to_string(),
                        _ => format!("{} layers", a.layers.len()),
                    };
                    ui.label(RichText::new(tag).size(12.5).color(DIM));
                });
            });
            ui.add_space(8.0);
            let u = self.unit;
            let sides: Vec<_> = [&self.sa, &self.sb].into_iter().flatten().cloned().collect();
            let line = |ui: &mut Ui, key: &str, value: &dyn Fn(&crate::board::Summary) -> String| {
                let parts: Vec<String> = sides.iter().map(|s| value(s)).collect();
                let text = if parts.len() == 2 && parts[0] != parts[1] {
                    format!("{}  ·  {}", parts[0], parts[1])
                } else {
                    parts[0].clone()
                };
                row(ui, key, &text);
            };
            line(ui, "Size", &|s| format!("{} × {}", u.num(s.w), u.len(s.h)));
            line(ui, "Copper layers", &|s| s.copper.to_string());
            line(ui, "Holes", &|s| s.holes.to_string());
            line(ui, "Smallest track", &|s| s.min_track.map_or_else(|| "—".to_string(), |v| u.len(v)));
            line(ui, "Smallest hole", &|s| s.min_hole.map_or_else(|| "—".to_string(), |v| u.len(v)));
            line(ui, "Pads", &|s| s.pads.to_string());
            if sides.iter().any(|s| s.nets > 0) {
                line(ui, "Nets", &|s| s.nets.to_string());
            }
            if a.outline.is_none() {
                ui.add_space(4.0);
                ui.label(RichText::new("No board edge found, so the size comes from the copper.").size(12.5).color(theme::YELLOW));
            }
        });
    }

    fn clicked_card(&mut self, ui: &mut Ui) {
        let Some(t) = self.selected else {
            card(ui, |ui| {
                heading(ui, "Measurements");
                ui.label(RichText::new("Click a pad, track, hole or shape to see its size here and on the board. Double-click copper to select its whole trace.").size(13.0).color(FAINT));
            });
            return;
        };
        let Some((layer, f)) = self.feature(&t) else { return };
        let (layer_label, mut rows) = (layer.role.label.clone(), self.feature_rows(layer, f));
        if let Some(r) = self.trace_rows(&t) {
            rows = r;
        }
        let net = layer.data.text(f.net).map(str::to_string);
        let at = f.path[0];
        let other_text = self.counterpart(&t, at).map(|found| match found {
            Some(o) => match self.feature(&o) {
                Some((l, g)) => format!("{}  {}", g.kind.label(), self.size_text(l, g)),
                None => "Nothing here".to_string(),
            },
            None => "Nothing here".to_string(),
        });
        let mine_text = format!("{}  {}", f.kind.label(), self.size_text(layer, f));
        let kept = self.pins.iter().position(|q| matches!(q, Pin::Feature(o) if *o == t));
        let ctx = ui.ctx().clone();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(f_title(&rows)).font(semibold(15.5)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if small_icon(ui, Icon::Close, "Clear (Esc)").clicked() {
                        self.clear_selection();
                    }
                    if let Some(index) = kept {
                        if small_icon(ui, Icon::Trash, "Remove from the board (Delete)").clicked() {
                            self.delete_pin(index);
                        }
                    } else if small_icon(ui, Icon::Pin, "Keep this on the board (K)").clicked() {
                        self.keep_selected();
                    }
                });
            });
            ui.label(RichText::new(layer_label).size(12.5).color(DIM));
            ui.add_space(4.0);
            for (k, v) in rows.iter().skip(1) {
                row(ui, k, v);
            }
            if let Some(other) = other_text {
                ui.add_space(4.0);
                ui.separator();
                let same = other == mine_text;
                let color = if same { FAINT } else { theme::YELLOW };
                row_font(ui, &format!("In {} here", t.side.other().letter()), if same { "The same" } else { &other }, theme::mono(12.5), color);
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if chip(ui, "Set origin here").on_hover_text("Measure positions from this point").clicked() {
                    self.origin = at;
                }
                if net.is_some() {
                    let label = if self.show_net { "Hide net" } else { "Show whole net" };
                    if chip(ui, label).clicked() {
                        self.show_net = !self.show_net;
                    }
                }
                if chip(ui, "Copy").clicked() {
                    let text: Vec<String> = rows.iter().map(|(k, v)| format!("{k}: {v}")).collect();
                    ctx.copy_text(text.join("\n"));
                }
            });
        });
    }

    /// The details of the trace this piece of copper belongs to, if it's part of the selected one.
    pub fn trace_rows(&self, t: &crate::app::Target) -> Option<Vec<(String, String)>> {
        let tc = self.trace.as_ref().filter(|tc| t.side == self.trace_side && tc.contains(t.layer, t.feature))?;
        {
            let u = self.unit;
            let mut rows: Vec<(String, String)> = Vec::new();
            let pair = tc.partner.is_some();
            let title = match (pair, tc.through) {
                (true, true) => "Differential pair through holes",
                (true, false) => "Differential pair",
                (false, true) => "Trace through holes",
                (false, false) => "Trace",
            };
            rows.push((String::new(), title.to_string()));
            // Each trace's total, with its length on each layer that has some.
            let report = |rows: &mut Vec<(String, String)>, label: &str, one: &crate::trace::Trace| {
                rows.push((label.to_string(), u.len(one.length())));
                if let Some(board) = self.board(t.side) {
                    // Layers a trace only lands on with a pad have no length to report.
                    for tl in one.layers.iter().filter(|tl| tl.tracks > 0 || tl.regions > 0) {
                        let name = board.layers.get(tl.layer).map_or(String::new(), |l| l.role.label.clone());
                        let value = if tl.tracks > 0 { u.len(tl.length) } else { "pour".to_string() };
                        rows.push((format!("   {name}"), value));
                    }
                }
            };
            if let Some(partner) = &tc.partner {
                report(&mut rows, "Trace 1 length", tc);
                report(&mut rows, "Trace 2 length", partner);
                rows.push(("Difference".to_string(), u.len((tc.length() - partner.length()).abs())));
                rows.push(("Side by side".to_string(), u.len(tc.coupled)));
            } else {
                report(&mut rows, "Total length", tc);
                rows.push(("Tracks".to_string(), tc.sum(|l| l.tracks).to_string()));
                rows.push(("Pads".to_string(), tc.sum(|l| l.pads).to_string()));
                let pours = tc.sum(|l| l.regions);
                if pours > 0 {
                    rows.push(("Pours".to_string(), pours.to_string()));
                }
            }
            if tc.through {
                let holes = tc.holes.len() + tc.partner.as_ref().map_or(0, |p| p.holes.len());
                rows.push(("Holes".to_string(), holes.to_string()));
            } else {
                rows.push(("Again".to_string(), "Double-click to follow holes".to_string()));
            }
            Some(rows)
        }
    }

    /// What the little label on the board says about the selected piece. With
    /// the details panel open it stays short; with it closed, it says everything.
    pub fn popup_text(&self, t: &crate::app::Target) -> Option<String> {
        let u = self.unit;
        let rows = if self.selected == Some(*t) { self.trace_rows(t) } else { None };
        if self.right_open {
            let tc = self.trace.as_ref().filter(|_| rows.is_some())?;
            return Some(match &tc.partner {
                Some(p) => format!(
                    "{}\n1  {}   2  {}\nDifference  {}",
                    if tc.through { "Differential pair through holes" } else { "Differential pair" },
                    u.len(tc.length()),
                    u.len(p.length()),
                    u.len((tc.length() - p.length()).abs())
                ),
                None => format!("{}  {}", if tc.through { "Trace through holes" } else { "Trace" }, u.len(tc.length())),
            });
        }
        let (layer, f) = self.feature(t)?;
        let rows = rows.unwrap_or_else(|| {
            let mut rows = self.feature_rows(layer, f);
            rows.insert(1, ("Layer".to_string(), layer.role.label.clone()));
            rows
        });
        let mut lines = vec![rows.first().map_or(String::new(), |r| r.1.clone())];
        lines.extend(rows.iter().skip(1).map(|(k, v)| format!("{k}  {v}")));
        Some(lines.join("\n"))
    }

    fn feature_rows(&self, layer: &Layer, f: &Feature) -> Vec<(String, String)> {
        let u = self.unit;
        let mut rows = vec![(String::new(), f.kind.label().to_string())];
        let mut add = |k: &str, v: String| rows.push((k.to_string(), v));
        use crate::gerber::Kind;
        let ap = layer.data.apertures.get(f.ap as usize);
        match f.kind {
            Kind::Pad => {
                if let Some(a) = ap {
                    add("Width", u.len(a.w));
                    add("Height", u.len(a.h));
                    add("Shape", a.describe().to_string());
                    add("Area", u.area(f.outline(&layer.data.apertures).first().map_or(0.0, |r| ring_area(r))));
                    if a.hole > 0.0 {
                        add("Hole", u.len(a.hole));
                    }
                    add("Pad code", format!("D{}", a.code));
                }
            }
            Kind::Hole => {
                add("Diameter", u.len(f.width));
                if let Some(plated) = layer.data.plated {
                    add("Plating", if plated { "Plated" } else { "Not plated" }.to_string());
                }
            }
            Kind::Slot => {
                add("Width", u.len(f.width));
                add("Length", u.len(f.length() + f.width));
                if let Some(plated) = layer.data.plated {
                    add("Plating", if plated { "Plated" } else { "Not plated" }.to_string());
                }
            }
            Kind::Track => {
                add("Width", u.len(f.width));
                add("Length", u.len(f.length()));
                let d = f.path[1].sub(f.path[0]);
                add("Angle", format!("{:.1}°", d.y.atan2(d.x).to_degrees().rem_euclid(180.0)));
                if let Some(a) = ap {
                    add("Pen", format!("D{}", a.code));
                }
            }
            Kind::Arc => {
                add("Width", u.len(f.width));
                add("Length", u.len(f.length()));
                if let Some(arc) = f.arc {
                    add("Radius", u.len(arc.radius));
                    add("Sweep", format!("{:.1}°", arc.sweep_deg));
                }
            }
            Kind::Region => {
                add("Width", u.len(f.bbox.w()));
                add("Height", u.len(f.bbox.h()));
                add("Area", u.area(ring_area(&f.path)));
                add("Corners", f.path.len().to_string());
            }
        }
        let at = if f.kind == Kind::Region { f.bbox.center() } else { f.path[0] }.sub(self.origin);
        add("X", u.len(at.x));
        add("Y", u.len(at.y));
        if let Some(net) = layer.data.text(f.net) {
            add("Net", net.to_string());
        }
        if let Some(comp) = layer.data.text(f.comp) {
            let pin = layer.data.text(f.pin).map(|p| format!(" pin {p}")).unwrap_or_default();
            add("Part", format!("{comp}{pin}"));
        }
        if let Some(func) = layer.data.text(f.func) {
            add("Purpose", func.to_string());
        }
        if !f.dark {
            add("Polarity", "Cut-out".to_string());
        }
        rows
    }

    fn ruler_card(&mut self, ui: &mut Ui) {
        let Some(r) = self.last_ruler else { return };
        let u = self.unit;
        let d = r.b.sub(r.a);
        let kept = self.pins.iter().position(|q| matches!(q, Pin::Ruler(o) if o.a == r.a && o.b == r.b));
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Ruler").font(semibold(15.5)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if let Some(index) = kept {
                        if small_icon(ui, Icon::Trash, "Remove this ruler (Delete)").clicked() {
                            self.delete_pin(index);
                        }
                    } else if small_icon(ui, Icon::Close, "Hide").clicked() {
                        self.last_ruler = None;
                    }
                });
            });
            ui.add_space(2.0);
            row(ui, "Distance", &u.len(d.len()));
            row(ui, "Across", &u.len(d.x.abs()));
            row(ui, "Up", &u.len(d.y.abs()));
            row(ui, "Angle", &format!("{:.1}°", d.y.atan2(d.x).to_degrees()));
            if let Some(gap) = r.gap {
                row_font(ui, "Edge to edge", &u.len(gap), theme::mono(12.5), theme::YELLOW);
            }
        });
    }

    fn pins_card(&mut self, ui: &mut Ui) {
        if self.pins.is_empty() {
            return;
        }
        let u = self.unit;
        let lines: Vec<String> = self
            .pins
            .iter()
            .map(|pin| match pin {
                Pin::Feature(t) => match self.feature(t) {
                    Some((l, f)) => format!("{} on {}: {}", f.kind.label(), l.role.label, self.size_text(l, f)),
                    None => "Gone".to_string(),
                },
                Pin::Ruler(r) => match r.gap {
                    Some(g) => format!("Ruler {}, gap {}", u.len(r.b.sub(r.a).len()), u.len(g)),
                    None => format!("Ruler {}", u.len(r.b.sub(r.a).len())),
                },
            })
            .collect();
        let ctx = ui.ctx().clone();
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Kept on the board").font(semibold(15.5)));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if small_icon(ui, Icon::Trash, "Clear them all").clicked() {
                        self.clear_pins();
                    }
                    if small_icon(ui, Icon::Copy, "Copy them as text").clicked() {
                        ctx.copy_text(lines.join("\n"));
                    }
                });
            });
            ui.add_space(2.0);
            let (mut delete, mut choose) = (None, None);
            for (i, text) in lines.iter().enumerate() {
                let on = self.selected_pin == Some(i);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let width = (ui.available_width() - 28.0).max(40.0);
                    ui.allocate_ui_with_layout(vec2(width, 20.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.set_width(width);
                        let mut words = RichText::new(text).font(theme::mono(11.5)).color(if on { FG } else { DIM });
                        if on {
                            words = words.background_color(SELECTION);
                        }
                        let row = ui.add(egui::Label::new(words).truncate().sense(Sense::click()));
                        if row.on_hover_text(text).clicked() {
                            choose = Some(i);
                        }
                    });
                    if small_icon(ui, Icon::Close, "Remove").clicked() {
                        delete = Some(i);
                    }
                });
            }
            if let Some(i) = delete {
                self.delete_pin(i);
            }
            if let Some(i) = choose {
                self.select_pin(i);
            }
        });
    }

    fn compare_card(&mut self, ui: &mut Ui) {
        if self.b.is_none() {
            return;
        }
        let Some(cmp) = &self.comparison else {
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Comparing…").color(DIM));
                });
            });
            return;
        };
        let count = cmp.changes.len();
        let truncated = cmp.truncated;
        let different = cmp.statuses.values().filter(|s| s.differs()).count();
        let same = cmp.statuses.len() - different;
        let current = self.change.and_then(|i| cmp.changes.get(i).cloned().map(|c| (i, c)));
        let objects = current.as_ref().map(|(_, c)| self.change_objects(c, self.change_group)).unwrap_or_default();
        let (mut step, mut align) = (0, self.align);
        let mut pick: Option<Option<crate::compare::Item>> = None;
        let shifted = self.shift.len() > 0.0;
        card(ui, |ui| {
            ui.label(RichText::new("Compare").font(semibold(15.5)));
            ui.add_space(2.0);
            if different == 0 {
                ui.label(RichText::new("Every layer draws the same picture.").color(theme::GREEN));
            } else {
                row(ui, "Layers that differ", &different.to_string());
                row(ui, "Layers the same", &same.to_string());
                row(ui, "Places to look", &count.to_string());
            }
            if truncated {
                ui.label(RichText::new("So much changed that only the first part is listed.").size(12.5).color(theme::YELLOW));
            }
            if count > 0 {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if tool_button(ui, Icon::Up, "Previous", true).on_hover_text("P").clicked() {
                        step = -1;
                    }
                    if tool_button(ui, Icon::Down, "Next", true).on_hover_text("N").clicked() {
                        step = 1;
                    }
                });
                if let Some((i, c)) = &current {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("{} of {count}: {}", i + 1, c.label)).size(13.0).color(FG));
                    let what = match (c.removed, c.added) {
                        (0, n) => format!("{n} added"),
                        (n, 0) => format!("{n} removed"),
                        (r, a) => format!("{r} removed, {a} added"),
                    };
                    ui.label(RichText::new(what).size(12.5).color(DIM));
                    ui.add_space(4.0);
                    // Each difference can be picked: it then blinks on the board.
                    ScrollArea::vertical().id_salt("diff list").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
                        for (item, text) in objects.iter().take(300) {
                            let (dot, tag) = if item.added { (theme::GREEN, "B") } else { (theme::RED, "A") };
                            let (what, place) = text.split_once("  at ").unwrap_or((text, ""));
                            let chosen = self.blink == Some(*item);
                            let back = ui.painter().add(egui::Shape::Noop);
                            let row_rect = ui
                                .vertical(|ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.horizontal(|ui| {
                                        ui.label(RichText::new(format!("Only in {tag}")).font(theme::mono(11.0)).color(dot));
                                        ui.add(egui::Label::new(RichText::new(what).size(12.5).color(FG)).truncate());
                                    });
                                    if !place.is_empty() {
                                        ui.label(RichText::new(format!("at {place}")).font(theme::mono(11.0)).color(FAINT));
                                    }
                                })
                                .response
                                .rect;
                            let hit = ui.interact(row_rect, ui.scope_id().with(("diff row", item.layer, item.feature, item.added)), Sense::click());
                            if chosen || hit.hovered() {
                                let fill = if chosen { SELECTION } else { RAISED };
                                ui.painter().set(back, egui::epaint::RectShape::filled(row_rect.expand2(vec2(4.0, 1.0)), 6.0, fill));
                            }
                            if hit.on_hover_text("Click to make this difference blink on the board").clicked() {
                                pick = Some(if chosen { None } else { Some(*item) });
                            }
                            ui.add_space(3.0);
                        }
                    });
                }
            }
            ui.add_space(4.0);
            ui.checkbox(&mut align, "Line B up with A").on_hover_text(
                "When both boards are the same size but B's origin moved, slide B so only real changes show.",
            );
            if shifted {
                let u = self.unit;
                row(ui, "B moved by", &format!("{}, {}", u.num(self.shift.x), u.len(self.shift.y)));
            }
        });
        if let Some(pick) = pick {
            self.blink = pick;
            self.blink_epoch.set(None);
        }
        if step != 0 {
            self.go_to_change(step);
        }
        if align != self.align {
            self.set_align(align);
        }
    }

    /// What's different at one place: each feature that's on only one side.
    fn change_objects(&self, c: &crate::compare::Change, group: Option<usize>) -> Vec<(crate::compare::Item, String)> {
        let u = self.unit;
        let items = group.and_then(|g| c.groups.get(g)).map_or(&c.items, |g| &g.items);
        items
            .iter()
            .filter_map(|item| {
                let side = if item.added { Side::B } else { Side::A };
                let (layer, f) = self.feature(&crate::app::Target { side, layer: item.layer, feature: item.feature })?;
                let at = if f.kind == crate::gerber::Kind::Region { f.bbox.center() } else { f.path[0] };
                let at = if item.added { at.add(self.shift) } else { at }.sub(self.origin);
                Some((*item, format!("{} {}  at {}, {}", f.kind.label(), self.size_text(layer, f), u.num(at.x), u.len(at.y))))
            })
            .collect()
    }

    fn layer_card(&mut self, ui: &mut Ui) {
        let Some(slot) = self.active.and_then(|i| self.slots.get(i)).cloned() else { return };
        for side in [Side::A, Side::B] {
            let Some(index) = slot.layer(side) else { continue };
            let Some(board) = self.board(side) else { continue };
            let layer = board.layers[index].clone();
            let two = self.b.is_some();
            card(ui, |ui| {
                let title = if two { format!("{}, {}", layer.role.label, side.letter()) } else { layer.role.label.clone() };
                ui.label(RichText::new(title).font(semibold(15.5)));
                ui.add(egui::Label::new(RichText::new(&layer.file).size(12.5).color(DIM)).truncate()).on_hover_text(&layer.file);
                ui.add_space(4.0);
                self.layer_rows(ui, &slot, &layer);
            });
        }
    }

    fn layer_rows(&self, ui: &mut Ui, slot: &Slot, layer: &Layer) {
        let u = self.unit;
        let s = &layer.stats;
        row(ui, "File units", layer.data.units);
        if !layer.data.format.is_empty() {
            row(ui, "Number format", &layer.data.format);
        }
        if slot.group == Group::Drill {
            row(ui, "Holes", &s.holes.to_string());
            if s.slots > 0 {
                row(ui, "Slots", &s.slots.to_string());
            }
            if !layer.data.tools.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new("Drill sizes").size(12.5).color(FAINT));
                for tool in &layer.data.tools {
                    row(ui, &format!("T{:02}  {}", tool.number, u.len(tool.dia)), &format!("× {}", tool.count));
                }
            }
            return;
        }
        row(ui, "Pads", &s.pads.to_string());
        row(ui, "Tracks", &s.tracks.to_string());
        row(ui, "Filled shapes", &s.regions.to_string());
        if s.track_length > 0.0 {
            row(ui, "Track length", &u.len(s.track_length));
        }
        if let Some(v) = s.min_track {
            row(ui, "Smallest track", &u.len(v));
        }
        let table = board::shape_table(layer);
        if !table.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new("Pad and pen shapes").size(12.5).color(FAINT));
            let parts = &layer.data.apertures;
            for (code, text, used) in table.iter().take(8) {
                let size = parts
                    .iter()
                    .find(|a| format!("D{}", a.code) == *code)
                    .map_or(String::new(), |a| format!("{} × {}", u.num(a.w), u.num(a.h)));
                row(ui, &format!("{code} {text}"), &format!("{size}  ×{used}"));
            }
            if table.len() > 8 {
                ui.label(RichText::new(format!("and {} more", table.len() - 8)).size(12.5).color(FAINT));
            }
        }
    }
}

fn f_title(rows: &[(String, String)]) -> String {
    rows.first().map_or(String::new(), |r| r.1.clone())
}

use crate::geom::ring_area;
