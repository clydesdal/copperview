//! Acts out mouse and key steps through the same path real input takes, so a
//! check run can prove that clicking really works.

use crate::app::App;
use crate::view::PLAY_FROM;
use eframe::egui::{self, Event, Key, Modifiers, MouseWheelUnit, PointerButton, pos2, vec2};

#[derive(Clone, Debug)]
pub enum Step {
    Move(f32, f32),
    Down(f32, f32),
    Up(f32, f32),
    Scroll(f32),
    Pinch(f32),
    Type(String),
    Mark(usize),
    Save(String),
    Key(Key),
    Wait,
    Sleep(u64),
}

pub fn parse(text: &str) -> Vec<Step> {
    let mut steps = Vec::new();
    for action in text.split(';') {
        let words: Vec<&str> = action.split_whitespace().collect();
        let num = |i: usize| words.get(i).and_then(|t| t.parse::<f32>().ok()).unwrap_or(0.0);
        let (x, y) = (num(1), num(2));
        match words.first().copied() {
            Some("move") => steps.push(Step::Move(x, y)),
            Some("click") => {
                steps.extend([Step::Move(x, y), Step::Wait, Step::Down(x, y), Step::Wait, Step::Up(x, y), Step::Wait]);
            }
            Some("dblclick") => {
                steps.extend([Step::Move(x, y), Step::Wait, Step::Down(x, y), Step::Up(x, y), Step::Wait, Step::Down(x, y), Step::Up(x, y), Step::Wait]);
            }
            Some("drag") => {
                let (x2, y2) = (num(3), num(4));
                steps.extend([Step::Move(x, y), Step::Wait, Step::Down(x, y), Step::Wait]);
                for i in 1..=4 {
                    let t = i as f32 / 4.0;
                    steps.push(Step::Move(x + (x2 - x) * t, y + (y2 - y) * t));
                }
                steps.extend([Step::Up(x2, y2), Step::Wait]);
            }
            Some("scroll") => steps.extend([Step::Move(x, y), Step::Wait, Step::Scroll(num(3)), Step::Wait]),
            Some("pinch") => steps.extend([Step::Move(x, y), Step::Wait, Step::Pinch(num(3)), Step::Wait]),
            Some("type") => {
                steps.extend([Step::Type(words[1..].join(" ")), Step::Wait]);
            }
            Some("mark") => steps.extend([Step::Mark(x as usize), Step::Wait]),
            Some("save") => {
                if let Some(path) = words.get(1) {
                    steps.extend([Step::Save(path.to_string()), Step::Wait, Step::Wait]);
                }
            }
            Some("key") => {
                if let Some(key) = words.get(1).and_then(|k| Key::from_name(k).or_else(|| Key::from_name(&k.to_uppercase()))) {
                    steps.extend([Step::Key(key), Step::Wait]);
                }
            }
            Some("sleep") => steps.extend([Step::Sleep(x as u64), Step::Wait]),
            Some("wait") => steps.extend(std::iter::repeat_n(Step::Wait, x as usize)),
            _ => {}
        }
    }
    steps.reverse();
    steps
}

impl App {
    pub fn play_step(&mut self, raw: &mut egui::RawInput) {
        if self.frames < PLAY_FROM || self.loading() {
            return;
        }
        let Some(step) = self.play.pop() else { return };
        let button = |x: f32, y: f32, pressed: bool| Event::PointerButton {
            pos: pos2(x, y),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        match step {
            Step::Move(x, y) => raw.events.push(Event::PointerMoved(pos2(x, y))),
            Step::Down(x, y) => raw.events.push(button(x, y, true)),
            Step::Up(x, y) => raw.events.push(button(x, y, false)),
            Step::Scroll(amount) => raw.events.push(Event::MouseWheel {
                unit: MouseWheelUnit::Point,
                delta: vec2(0.0, amount),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            }),
            Step::Pinch(zoom) => self.pinch.add(zoom),
            Step::Type(text) => raw.events.push(Event::Text(text)),
            Step::Mark(kind) => self.markup.kind = crate::markup::MarkKind::ALL[kind.min(8)].0,
            Step::Save(path) => self.save_pending = Some(path.into()),
            Step::Key(key) => {
                for pressed in [true, false] {
                    raw.events.push(Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    });
                }
            }
            Step::Sleep(ms) => std::thread::sleep(std::time::Duration::from_millis(ms)),
            Step::Wait => {}
        }
    }
}
