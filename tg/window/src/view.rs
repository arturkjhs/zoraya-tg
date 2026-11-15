//! The window's trees: the main screen (the chat list beside the open chat), and the
//! forms (the login's three steps, settings). The program builds the tree again when
//! the model's `tree` count changes, and moves the toolkit's focus to the role the model
//! names.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::Cell;

use zoraya_paint::Canvas;
use zoraya_tokens::{Logical, text};
use zoraya_ui::{LRect, Painter, Response, Widget};

use crate::chat::{Card, Composer, HEADER, Messages, SCOPES, composer_height};
use crate::draw::{self, lg, num};
use crate::list::ChatList;
use crate::model::{Model, Role, Screen};
use crate::proto::AuthState;
use crate::widgets::{Btn, Clock, Field, Probe, Shared, Toggle, Which, id};

/// The window's first size, logical.
pub const WIDTH: u32 = 840;
pub const HEIGHT: u32 = 520;
/// The chat list's width, and the width under which one pane is shown at a time.
pub const LIST_W: i32 = 300;
pub const NARROW: i32 = 620;
const BAR: i32 = 48;

/// A tree and where each role's widget is in it (child indices from the root).
pub struct Tree {
    pub root: Box<dyn Widget>,
    pub roles: Vec<(Role, Vec<usize>)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part {
    Search,
    Settings,
    List,
    Summary,
    Messages,
    Composer,
    Send,
    Card,
    Probe,
}

/// The main screen.
pub struct MainView {
    model: Shared,
    kids: Vec<Box<dyn Widget>>,
    parts: Vec<Part>,
    started: Rc<Cell<u64>>,
    clock: Clock,
    rect: LRect,
}

impl Widget for MainView {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (lg(WIDTH as i32), lg(HEIGHT as i32))
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        let (w, h) = (num(r.w), num(r.h));
        let (narrow, open) = {
            let mut m = self.model.borrow_mut();
            m.set_size(w, h);
            let narrow = w < NARROW;
            if narrow != m.narrow {
                // One pane or two: another tree.
                m.narrow = narrow;
                m.tree += 1;
            }
            (narrow, m.open.is_some())
        };
        let has = |part: Part, parts: &[Part]| parts.contains(&part);
        let list_shown = has(Part::List, &self.parts);
        let chat_shown = has(Part::Messages, &self.parts);
        let list_w = match (list_shown, chat_shown) {
            (true, true) => LIST_W.min(w),
            (true, false) if narrow => w,
            (true, false) => LIST_W.min(w),
            (false, _) => 0,
        };
        let chat_x = if list_shown { list_w + 1 } else { 0 };
        let chat_w = (w - chat_x).max(0);
        let _ = open;
        // The widths of the buttons, then the composer's height for what is left.
        let mut size = |part: Part, kids: &mut [Box<dyn Widget>], parts: &[Part]| {
            parts
                .iter()
                .position(|x| *x == part)
                .map(|i| kids[i].measure(p))
                .map_or((0, 0), |(a, b)| (num(a), num(b)))
        };
        let settings = size(Part::Settings, &mut self.kids, &self.parts);
        let summary = size(Part::Summary, &mut self.kids, &self.parts);
        let send = size(Part::Send, &mut self.kids, &self.parts);
        let field_w = (chat_w - 12 - 8 - send.0 - 12).max(40);
        let composer_h = if chat_shown {
            num(composer_height(p, &self.model.borrow(), lg(field_w)))
        } else {
            0
        };
        let bottom = composer_h + 20;
        let at = |x: i32, y: i32, w: i32, h: i32| {
            LRect::new(r.x + lg(x), r.y + lg(y), lg(w), lg(h))
        };
        for (kid, part) in self.kids.iter_mut().zip(&self.parts) {
            let place = match part {
                Part::Search => at(12, 10, list_w - 12 - 8 - settings.0 - 12, 28),
                Part::Settings => at(list_w - 12 - settings.0, 10, settings.0, settings.1),
                Part::List => at(0, BAR, list_w, h - BAR),
                Part::Messages => at(chat_x, 0, chat_w, h - bottom),
                Part::Summary => at(w - 12 - summary.0, 10, summary.0, summary.1),
                Part::Composer => at(chat_x + 12, h - 10 - composer_h, field_w, composer_h),
                Part::Send => at(w - 12 - send.0, h - 10 - send.1, send.0, send.1),
                Part::Card => {
                    let cw = (chat_w - 40).clamp(120, 440);
                    let ch = (h - HEADER - bottom - 40).clamp(160, 300);
                    at(chat_x + (chat_w - cw) / 2, HEADER + 20, cw, ch)
                }
                Part::Probe => at(0, 0, 0, 0),
            };
            kid.layout(place, p);
        }
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        self.started.set((self.clock)());
        let k = p.scale.factor() as i32;
        let r = self.rect.physical(p.scale);
        let pal = p.palette;
        c.fill(r, pal.bg_window);
        let list = self.parts.contains(&Part::List);
        let chat = self.parts.contains(&Part::Messages);
        if list {
            let w = if chat { LIST_W * k } else { r.w.min(if r.w < NARROW * k { r.w } else { LIST_W * k }) };
            c.fill(draw::rect(r.x, r.y, w, r.h), pal.bg_view);
            c.fill(draw::rect(r.x, r.y + BAR * k - k, w, k), pal.border);
            c.fill(draw::rect(r.x + w, r.y, k, r.h), pal.border);
        }
        if !chat && r.w >= NARROW * k {
            // Nothing open: say what the right side is for.
            let px = text::BODY.physical(p.scale);
            let s = "Choose a chat to read it";
            let x0 = r.x + (LIST_W + 1) * k;
            let w = p.ui.measure(s, px);
            let base = draw::baseline(r.y + r.h / 2 - 10 * k, 20 * k, px);
            draw::text(c, &mut p.ui, s, px, x0 + (r.right() - x0 - w) / 2, base, pal.fg_text_secondary, r);
        }
    }
    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.kids
    }
}

/// A form: a heading, lines of text, and its controls in a column, in the middle of
/// the window.
pub struct FormView {
    model: Shared,
    kids: Vec<Box<dyn Widget>>,
    /// The words beside each control (a setting's label), or none.
    labels: Vec<String>,
    started: Rc<Cell<u64>>,
    clock: Clock,
    rect: LRect,
}

const FORM_W: i32 = 360;

impl FormView {
    fn top(&self) -> i32 {
        (num(self.rect.h) / 2 - 150).max(16)
    }
}

impl Widget for FormView {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (lg(WIDTH as i32), lg(HEIGHT as i32))
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        let (w, h) = (num(r.w), num(r.h));
        self.model.borrow_mut().set_size(w, h);
        let fw = FORM_W.min(w - 32).max(120);
        let x = (w - fw) / 2;
        // Under the heading and its two lines.
        let mut y = self.top() + 96;
        let n = self.kids.len();
        for (i, kid) in self.kids.iter_mut().enumerate() {
            if i + 1 == n {
                // The probe.
                kid.layout(LRect::new(r.x, r.y, Logical::ZERO, Logical::ZERO), p);
                break;
            }
            let (kw, kh) = kid.measure(p);
            let (kw, kh) = (num(kw), num(kh));
            let labelled = self.labels.get(i).is_some_and(|l| !l.is_empty());
            let place = if labelled {
                // A switch at the right of its words.
                LRect::new(r.x + lg(x + fw - kw), r.y + lg(y), lg(kw), lg(kh))
            } else if kw > fw / 2 {
                // A field: the form's width.
                LRect::new(r.x + lg(x), r.y + lg(y), lg(fw), lg(kh))
            } else {
                LRect::new(r.x + lg(x), r.y + lg(y), lg(kw), lg(kh))
            };
            kid.layout(place, p);
            y += kh + if labelled { 10 } else { 34 };
        }
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        self.started.set((self.clock)());
        let k = p.scale.factor() as i32;
        let r = self.rect.physical(p.scale);
        let pal = p.palette;
        c.fill(r, pal.bg_window);
        let m = self.model.borrow();
        let fw = FORM_W.min(num(self.rect.w) - 32).max(120) * k;
        let x = r.x + (r.w - fw) / 2;
        let top = r.y + self.top() * k;
        let (title, line) = match (m.screen, m.auth) {
            (Screen::Settings, _) => ("Settings", String::from("Kept by the backend, for this account.")),
            (_, AuthState::WaitPhone) => ("Telegram", String::from("Your phone number, with the country's code")),
            (_, AuthState::WaitCode) => ("Telegram", String::from("The code Telegram sent you")),
            (_, AuthState::WaitPassword) => ("Telegram", String::from("Your password (two-step verification)")),
            _ => ("Telegram", String::from("Connecting…")),
        };
        let tpx = text::TITLE.physical(p.scale);
        let px = text::BODY.physical(p.scale);
        let cpx = text::CAPTION.physical(p.scale);
        let clip = draw::rect(x, r.y, fw, r.h);
        draw::text(c, &mut p.medium, title, tpx, x, draw::baseline(top, 30 * k, tpx), pal.fg_text, clip);
        draw::text(c, &mut p.ui, &line, px, x, draw::baseline(top + 38 * k, 20 * k, px), pal.fg_text, clip);
        let hint = if m.screen == Screen::Settings {
            String::from("Light and dark follow Zoraya's own settings.")
        } else {
            m.auth_hint.clone()
        };
        let hint = draw::fit(&mut p.ui, &hint, cpx, fw);
        draw::text(c, &mut p.ui, &hint, cpx, x, draw::baseline(top + 62 * k, 18 * k, cpx), pal.fg_text_secondary, clip);
        // Each switch's words at its left; the login's refusal under its field.
        for (kid, label) in self.kids.iter().zip(&self.labels) {
            if label.is_empty() {
                continue;
            }
            let kr = kid.rect().physical(p.scale);
            let s = draw::fit(&mut p.ui, label, px, kr.x - x - 12 * k);
            draw::text(c, &mut p.ui, &s, px, x, draw::baseline(kr.y, kr.h, px), pal.fg_text, clip);
        }
        if m.screen == Screen::Login && !m.auth_error.is_empty() {
            let field = self.kids[0].rect().physical(p.scale);
            let s = format!("Telegram refused it: {}", m.auth_error);
            let s = draw::fit(&mut p.ui, &s, cpx, fw);
            draw::text(c, &mut p.ui, &s, cpx, x, draw::baseline(field.bottom() + 4 * k, 18 * k, cpx), pal.error, clip);
        }
    }
    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.kids
    }
}

/// The tree for what the model shows now.
pub fn build(model: &Shared, clock: Clock) -> Tree {
    let started = Rc::new(Cell::new(0));
    let probe = || Box::new(Probe::new(started.clone(), clock, model.clone()));
    let m = model.borrow();
    let mut kids: Vec<Box<dyn Widget>> = Vec::new();
    let mut roles: Vec<(Role, Vec<usize>)> = Vec::new();
    match m.screen {
        Screen::Main => {
            let mut parts = Vec::new();
            let mut add = |part: Part, role: Option<Role>, w: Box<dyn Widget>| {
                if let Some(r) = role {
                    roles.push((r, alloc::vec![kids.len()]));
                }
                kids.push(w);
                parts.push(part);
            };
            let open = m.open.is_some();
            if !(m.narrow && open) {
                add(
                    Part::Search,
                    Some(Role::Search),
                    Box::new(Field::new(id::SEARCH, Which::Search, model.clone(), "Search")),
                );
                let mut b = Btn::new(id::SETTINGS, "Settings", false);
                b.model = Some(model.clone());
                add(Part::Settings, Some(Role::SettingsButton), Box::new(b));
                add(
                    Part::List,
                    Some(Role::List),
                    Box::new(ChatList::new(id::LIST, model.clone())),
                );
            }
            if open {
                add(
                    Part::Messages,
                    Some(Role::Messages),
                    Box::new(Messages::new(id::MESSAGES, model.clone())),
                );
                let mut b = Btn::new(id::SUMMARY, "Summary", false);
                b.model = Some(model.clone());
                add(Part::Summary, Some(Role::SummaryButton), Box::new(b));
                add(
                    Part::Composer,
                    Some(Role::Composer),
                    Box::new(Composer::new(id::COMPOSER, model.clone())),
                );
                let mut b = Btn::new(id::SEND, "Send", true);
                b.model = Some(model.clone());
                add(Part::Send, Some(Role::Send), Box::new(b));
                if m.summary.is_some() {
                    add(Part::Card, None, Box::new(Card::new(model.clone())));
                }
            }
            add(Part::Probe, None, probe());
            if let Some(card) = parts.iter().position(|x| *x == Part::Card) {
                for i in 0..=SCOPES.len() {
                    roles.push((Role::Card(i as u32), alloc::vec![card, i]));
                }
            }
            drop(m);
            Tree {
                root: Box::new(MainView {
                    model: model.clone(),
                    kids,
                    parts,
                    started,
                    clock,
                    rect: LRect::default(),
                }),
                roles,
            }
        }
        screen => {
            let mut labels = Vec::new();
            let mut add = |role: Role, label: &str, w: Box<dyn Widget>| {
                roles.push((role, alloc::vec![kids.len()]));
                kids.push(w);
                labels.push(String::from(label));
            };
            match screen {
                Screen::Login => {
                    add(
                        Role::LoginField,
                        "",
                        Box::new(Field::new(id::LOGIN, Which::Login, model.clone(), "")),
                    );
                    add(Role::LoginNext, "", Box::new(Btn::new(id::NEXT, "Next", true)));
                }
                Screen::Settings => {
                    for (i, s) in m.settings.iter().enumerate() {
                        add(
                            Role::Setting(i as u32),
                            &s.label,
                            Box::new(Toggle::new(id::SETTING + i as u32, i, model.clone())),
                        );
                    }
                    let mut b = Btn::new(id::LOG_OUT, "Log out", false);
                    b.model = Some(model.clone());
                    add(Role::LogOut, "", Box::new(b));
                    let mut b = Btn::new(id::BACK, "Back", true);
                    b.model = Some(model.clone());
                    add(Role::Back, "", Box::new(b));
                }
                _ => {}
            }
            kids.push(probe());
            labels.push(String::new());
            drop(m);
            Tree {
                root: Box::new(FormView {
                    model: model.clone(),
                    kids,
                    labels,
                    started,
                    clock,
                    rect: LRect::default(),
                }),
                roles,
            }
        }
    }
}

/// What a button's activation asks of the model (the lists, the fields' editing and
/// the switches act on the model themselves).
pub fn respond(m: &mut Model, r: Response) {
    let Response::Activated(i) = r else { return };
    match i {
        id::SETTINGS => m.show_settings(),
        id::SUMMARY => m.summarize(crate::proto::Scope::Unread),
        id::SEND => {
            m.send();
            m.focus = Some(Role::Composer);
        }
        id::CLOSE => m.close_summary(),
        id::LOGIN | id::NEXT => {
            m.login_submit();
            m.focus = Some(Role::LoginField);
        }
        id::LOG_OUT => m.log_out(),
        id::BACK => m.leave_settings(),
        _ => {
            if let Some((scope, _)) = i
                .checked_sub(id::SCOPE)
                .and_then(|n| SCOPES.get(n as usize))
            {
                m.summarize(*scope);
                m.tree += 1;
                m.focus = Some(Role::Card(i - id::SCOPE));
            }
        }
    }
}
