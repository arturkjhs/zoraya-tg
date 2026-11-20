//! The small widgets: a one-line field over the model's text (search, the login's
//! values, a password drawn as dots), a button and a switch that answer a press once,
//! and the probe that times a frame. They look as `zoraya-ui`'s own do.

use alloc::rc::Rc;
use alloc::string::String;
use core::cell::{Cell, RefCell};

use zoraya_paint::Canvas;
use zoraya_tokens::{Logical, space, text};
use zoraya_ui::{Event, LRect, Painter, Response, Widget, focus_ring, keys};

use crate::draw;
use crate::model::{Edit, Model, Role};
use crate::proto::AuthState;

pub type Shared = Rc<RefCell<Model>>;
/// The monotonic clock, in nanoseconds.
pub type Clock = fn() -> u64;

/// `zoraya-proto-gui`'s modifier bits.
pub mod mods {
    pub const SHIFT: u32 = 1;
    pub const CTRL: u32 = 2;
    pub const ALT: u32 = 4;
    pub const SYSTEM: u32 = 16;
}

/// Key codes the toolkit does not name (evdev).
pub mod key {
    pub const PAGE_UP: u32 = 104;
    pub const PAGE_DOWN: u32 = 109;
    pub const F: u32 = 33;
    pub const R: u32 = 19;
    /// U: to the first unread message.
    pub const U: u32 = 22;
}

/// The widgets' ids, which their `Activated` and `Changed` carry.
pub mod id {
    pub const SEARCH: u32 = 1;
    pub const SETTINGS: u32 = 2;
    pub const LIST: u32 = 3;
    pub const SUMMARY: u32 = 4;
    pub const MESSAGES: u32 = 5;
    pub const COMPOSER: u32 = 6;
    pub const SEND: u32 = 7;
    /// The summary's scopes: unread, day, week.
    pub const SCOPE: u32 = 10;
    pub const CLOSE: u32 = 13;
    pub const LOGIN: u32 = 20;
    pub const NEXT: u32 = 21;
    pub const LOG_OUT: u32 = 28;
    pub const BACK: u32 = 29;
    /// The settings' switches, from this id on.
    pub const SETTING: u32 = 30;
}

/// A press of the left button that this event starts: `held` is what the widget saw
/// before.
pub fn press(e: &Event, held: &mut bool) -> Option<(Logical, Logical, u32)> {
    let Event::Pointer {
        x,
        y,
        buttons,
        modifiers,
    } = *e
    else {
        return None;
    };
    let down = buttons & 1 != 0;
    let started = down && !*held;
    *held = down;
    started.then_some((x, y, modifiers))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    Search,
    Login,
}

/// One line of the model's text.
pub struct Field {
    pub id: u32,
    pub which: Which,
    pub model: Shared,
    pub placeholder: &'static str,
    rect: LRect,
    held: bool,
}

impl Field {
    pub fn new(id: u32, which: Which, model: Shared, placeholder: &'static str) -> Field {
        Field {
            id,
            which,
            model,
            placeholder,
            rect: LRect::default(),
            held: false,
        }
    }

    fn edit<'a>(&self, m: &'a mut Model) -> &'a mut Edit {
        match self.which {
            Which::Search => &mut m.query,
            Which::Login => &mut m.login,
        }
    }
}

impl Widget for Field {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (space::CONTROL_HEIGHT.times(8), space::CONTROL_HEIGHT)
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let k = p.scale.factor() as i32;
        let r = self.rect.physical(p.scale);
        let radius = space::RADIUS_CONTROL.physical(p.scale);
        draw::round(c, r, radius, p.palette.bg_view);
        draw::outline(c, r, radius, k as u32, p.palette.border_strong);
        let mut m = self.model.borrow_mut();
        let hidden = self.which == Which::Login && m.auth == AuthState::WaitPassword;
        let e = self.edit(&mut m);
        // A password is never drawn: a dot a character.
        let (shown, cursor) = if hidden {
            let dots = |n: usize| "•".repeat(n);
            (
                dots(e.text.chars().count()),
                dots(e.text[..e.cursor].chars().count()).len(),
            )
        } else {
            (e.text.clone(), e.cursor)
        };
        let px = text::BODY.physical(p.scale);
        let pad = 8 * k;
        let inner = draw::rect(r.x + pad, r.y, r.w - 2 * pad, r.h);
        let base = draw::baseline(r.y, r.h, px);
        if shown.is_empty() && !focused {
            let fg = p.palette.fg_text_disabled;
            draw::text(c, &mut p.ui, self.placeholder, px, inner.x, base, fg, inner);
        }
        // The text moves left when the cursor would leave the field.
        let before = draw::measure(&mut p.ui, &shown[..cursor], px);
        let shift = (before - inner.w + k).max(0);
        let fg = p.palette.fg_text;
        draw::text(c, &mut p.ui, &shown, px, inner.x - shift, base, fg, inner);
        if focused {
            let bar = draw::rect(inner.x + before - shift, r.y + 5 * k, k, r.h - 10 * k);
            c.fill(bar.intersect(&r), p.palette.fg_text);
            focus_ring(p, c, self.rect);
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        if press(e, &mut self.held).is_some() {
            return Response::Repaint;
        }
        let Event::Key {
            code,
            text,
            modifiers,
        } = *e
        else {
            return match e {
                Event::Focus(_) => Response::Repaint,
                _ => Response::None,
            };
        };
        let mut m = self.model.borrow_mut();
        let search = self.which == Which::Search;
        let changed = match code {
            keys::ESC => {
                return if m.escape() {
                    Response::Changed(self.id)
                } else {
                    Response::None
                };
            }
            keys::ENTER if search => {
                // The first chat found, under the keyboard.
                m.list_cursor(0);
                m.focus = Some(Role::List);
                return Response::Changed(self.id);
            }
            keys::DOWN if search => {
                let at = m.cursor.unwrap_or(m.top);
                m.list_cursor(at);
                m.focus = Some(Role::List);
                return Response::Changed(self.id);
            }
            keys::ENTER => return Response::Activated(self.id),
            keys::LEFT => {
                self.edit(&mut m).left();
                false
            }
            keys::RIGHT => {
                self.edit(&mut m).right();
                false
            }
            keys::HOME => {
                self.edit(&mut m).home();
                false
            }
            keys::END => {
                self.edit(&mut m).end();
                false
            }
            keys::BACKSPACE => self.edit(&mut m).backspace(),
            keys::DELETE => self.edit(&mut m).delete(),
            _ => match text {
                Some(ch)
                    if !ch.is_control() && modifiers & (mods::CTRL | mods::SYSTEM) == 0 =>
                {
                    self.edit(&mut m).insert(ch);
                    true
                }
                _ => return Response::None,
            },
        };
        if !changed {
            return Response::Repaint;
        }
        if search {
            m.query_changed();
        }
        Response::Changed(self.id)
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
}

/// A button: Enter, Space or the start of a press activate it, once.
pub struct Btn {
    pub id: u32,
    pub label: String,
    /// The default one is drawn in the accent.
    pub default: bool,
    /// With the model, Escape on the button closes what is open.
    pub model: Option<Shared>,
    rect: LRect,
    held: bool,
}

impl Btn {
    pub fn new(id: u32, label: &str, default: bool) -> Btn {
        Btn {
            id,
            label: String::from(label),
            default,
            model: None,
            rect: LRect::default(),
            held: false,
        }
    }
}

impl Widget for Btn {
    fn measure(&mut self, p: &mut Painter) -> (Logical, Logical) {
        (
            p.text_width(&self.label) + space::PADDING_CONTROL.times(3),
            space::CONTROL_HEIGHT,
        )
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let k = p.scale.factor();
        let r = self.rect.physical(p.scale);
        let radius = space::RADIUS_CONTROL.physical(p.scale);
        let (bg, fg) = match (self.default, self.held) {
            (true, _) => (p.palette.accent, p.palette.fg_text_on_accent),
            (false, true) => (p.palette.bg_control_pressed, p.palette.fg_text),
            (false, false) => (p.palette.bg_control, p.palette.fg_text),
        };
        draw::round(c, r, radius, bg);
        draw::outline(c, r, radius, k, p.palette.border);
        let px = text::BODY.physical(p.scale);
        let w = draw::measure(&mut p.medium, &self.label, px);
        let base = draw::baseline(r.y, r.h, px);
        draw::text(c, &mut p.medium, &self.label, px, r.x + (r.w - w) / 2, base, fg, r);
        if focused {
            focus_ring(p, c, self.rect);
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        match e {
            Event::Key {
                code: keys::ENTER | keys::SPACE,
                ..
            } => Response::Activated(self.id),
            Event::Key {
                code: keys::ESC, ..
            } => match &self.model {
                Some(m) if m.borrow_mut().escape() => Response::Changed(self.id),
                _ => Response::None,
            },
            Event::Key { .. } => Response::None,
            Event::Focus(_) => Response::Repaint,
            Event::Wheel { .. } => Response::None,
            Event::Pointer { .. } => {
                let was = self.held;
                match press(e, &mut self.held) {
                    Some(_) => Response::Activated(self.id),
                    None if was != self.held => Response::Repaint,
                    None => Response::None,
                }
            }
        }
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
}

/// A switch for the model's setting `index`: Space, Enter or a press asks the backend
/// to turn it; it shows what the backend last said.
pub struct Toggle {
    pub id: u32,
    pub index: usize,
    pub model: Shared,
    rect: LRect,
    held: bool,
}

impl Toggle {
    pub fn new(id: u32, index: usize, model: Shared) -> Toggle {
        Toggle {
            id,
            index,
            model,
            rect: LRect::default(),
            held: false,
        }
    }
}

impl Widget for Toggle {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (
            space::CONTROL_HEIGHT + space::PADDING_WINDOW,
            space::CONTROL_HEIGHT,
        )
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let k = p.scale.factor() as i32;
        let r = self.rect.physical(p.scale);
        let on = self
            .model
            .borrow()
            .settings
            .get(self.index)
            .is_some_and(|s| s.on);
        let track = if on {
            p.palette.accent
        } else {
            p.palette.border_strong
        };
        draw::round(c, r, (r.h / 2) as u32, track);
        let knob = r.h - 4 * k;
        let x = if on { r.x + r.w - knob - 2 * k } else { r.x + 2 * k };
        draw::disc(c, draw::rect(x, r.y + 2 * k, knob, knob), p.palette.bg_control);
        if focused {
            focus_ring(p, c, self.rect);
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        let flip = match e {
            Event::Key {
                code: keys::ENTER | keys::SPACE,
                ..
            } => true,
            Event::Key {
                code: keys::ESC, ..
            } => {
                return if self.model.borrow_mut().escape() {
                    Response::Changed(self.id)
                } else {
                    Response::None
                };
            }
            Event::Pointer { .. } => press(e, &mut self.held).is_some(),
            Event::Focus(_) => return Response::Repaint,
            _ => false,
        };
        if !flip {
            return Response::None;
        }
        self.model.borrow_mut().toggle_setting(self.index);
        Response::Changed(self.id)
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
}

/// The last child of a tree, of no size: its paint comes after every other, so the
/// time since the root's paint began is the frame's.
pub struct Probe {
    pub started: Rc<Cell<u64>>,
    pub clock: Clock,
    pub model: Shared,
    rect: LRect,
}

impl Probe {
    pub fn new(started: Rc<Cell<u64>>, clock: Clock, model: Shared) -> Probe {
        Probe {
            started,
            clock,
            model,
            rect: LRect::default(),
        }
    }
}

impl Widget for Probe {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (Logical::ZERO, Logical::ZERO)
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = LRect::new(r.x, r.y, Logical::ZERO, Logical::ZERO);
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, _p: &mut Painter, _c: &mut Canvas<'_>, _focused: bool) {
        let us = ((self.clock)().saturating_sub(self.started.get()) / 1000).min(u64::from(u32::MAX));
        let mut m = self.model.borrow_mut();
        m.stats.frames += 1;
        m.stats.total_us += us;
        m.stats.max_us = m.stats.max_us.max(us as u32);
        if m.stats.mark {
            m.stats.mark = false;
            m.stats.first_us = Some(us as u32);
        }
    }
}
