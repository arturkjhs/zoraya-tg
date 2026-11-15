//! The chat list: a window of the backend's rows, a row a chat (the avatar's initials,
//! the name, the last line, the time, the unread count, pinned, muted), scrolled by the
//! wheel and the keys, a row opened by a press or Enter.

use alloc::format;
use alloc::string::String;

use zoraya_paint::Canvas;
use zoraya_tokens::{Logical, space, text};
use zoraya_ui::{Event, LRect, Painter, Response, WheelSteps, Widget, keys};

use crate::draw;
use crate::model::Role;
use crate::proto::{ChatKind, ChatRow};
use crate::time;
use crate::widgets::{Shared, key, mods, press};

/// A row's height and its parts, logical.
pub const ROW: i32 = 62;
const AVATAR: i32 = 44;
const PAD: i32 = 12;
const TEXT_X: i32 = PAD + AVATAR + 12;

pub struct ChatList {
    pub id: u32,
    pub model: Shared,
    rect: LRect,
    wheel: WheelSteps,
    held: bool,
}

impl ChatList {
    pub fn new(id: u32, model: Shared) -> ChatList {
        ChatList {
            id,
            model,
            rect: LRect::default(),
            wheel: WheelSteps::default(),
            held: false,
        }
    }

    #[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
    fn paint_row(
        &self,
        p: &mut Painter,
        c: &mut Canvas<'_>,
        row: &ChatRow,
        r: zoraya_paint::Rect,
        k: i32,
        open: bool,
        now: i64,
        offset: i32,
    ) {
        let pal = p.palette;
        if open {
            c.fill(r, pal.bg_selection);
        }
        // The avatar: a neutral disc with initials (colour means a room, so none here).
        let a = draw::rect(r.x + PAD * k, r.y + (ROW - AVATAR) * k / 2, AVATAR * k, AVATAR * k);
        draw::disc(c, a, draw::mix(pal.bg_view, pal.fg_text, 110));
        let ini = if row.kind == ChatKind::Saved {
            String::from("SM")
        } else {
            draw::initials(&row.title)
        };
        let hpx = text::HEADING.physical(p.scale);
        let iw = p.medium.measure(&ini, hpx);
        let base = draw::baseline(a.y, a.h, hpx);
        draw::text(c, &mut p.medium, &ini, hpx, a.x + (a.w - iw) / 2, base, pal.fg_text, a);

        let px = text::BODY.physical(p.scale);
        let cpx = text::CAPTION.physical(p.scale);
        let right = r.x + r.w - PAD * k;
        let x = r.x + TEXT_X * k;
        // The first line: the name, muted's mark, the time at the right.
        let when = time::list_time(row.date, now, offset);
        let tw = p.ui.measure(&when, cpx);
        let line1 = draw::baseline(r.y + 10 * k, 20 * k, px);
        draw::text(c, &mut p.ui, &when, cpx, right - tw, line1, pal.fg_text_secondary, r);
        let mark = if row.muted { 16 * k } else { 0 };
        let title = draw::fit(&mut p.medium, &row.title, px, right - tw - 8 * k - mark - x);
        let w = draw::text(c, &mut p.medium, &title, px, x, line1, pal.fg_text, r);
        if row.muted {
            // A circle struck through: no sound.
            let (mx, my, s) = (x + w + 6 * k, r.y + 15 * k, 10 * k);
            draw::ring(c, draw::rect(mx, my, s, s), k as u32, pal.fg_text_disabled);
            draw::stroke(c, mx, my, s, (2 * k, 8 * k), (8 * k, 2 * k), k as u32, pal.fg_text_disabled);
        }
        // The second line: the last message, then the unread count or the pin.
        let line2 = draw::baseline(r.y + 31 * k, 20 * k, px);
        let mut end = right;
        if row.unread > 0 {
            let n = if row.unread > 999 {
                String::from("999+")
            } else {
                format!("{}", row.unread)
            };
            let nw = p.medium.measure(&n, cpx);
            let bw = (nw + 12 * k).max(20 * k);
            let b = draw::rect(right - bw, r.y + 32 * k, bw, 20 * k);
            let bg = if row.muted {
                pal.border_strong
            } else {
                pal.accent
            };
            draw::round(c, b, (10 * k) as u32, bg);
            let nb = draw::baseline(b.y, b.h, cpx);
            draw::text(c, &mut p.medium, &n, cpx, b.x + (bw - nw) / 2, nb, pal.fg_text_on_accent, b);
            end = b.x - 8 * k;
        } else if row.pinned {
            // A pin: a head and its needle.
            let (px0, py0, s) = (right - 12 * k, r.y + 36 * k, 12 * k);
            draw::disc(c, draw::rect(px0 + 5 * k, py0, 6 * k, 6 * k), pal.fg_text_disabled);
            draw::stroke(c, px0, py0, s, (6 * k, 5 * k), (k, 11 * k), (k * 3 / 2).max(1) as u32, pal.fg_text_disabled);
            end = px0 - 8 * k;
        }
        let last = draw::fit(&mut p.ui, &row.last, px, end - x);
        draw::text(c, &mut p.ui, &last, px, x, line2, pal.fg_text_secondary, r);
    }
}

impl Widget for ChatList {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (draw::lg(300), draw::lg(ROW * 4))
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
        let rows = (draw::num(r.h) / ROW).max(1) as u32;
        self.model.borrow_mut().set_list_rows(rows);
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        let k = p.scale.factor() as i32;
        let r = self.rect.physical(p.scale);
        let pal = p.palette;
        c.fill(r, pal.bg_view);
        let m = self.model.borrow();
        let (now, offset) = (m.now(), m.utc_offset);
        if m.total == 0 {
            let s = if m.query.text.is_empty() {
                String::from("No chats")
            } else {
                format!("No chats match “{}”", m.query.text)
            };
            let px = text::BODY.physical(p.scale);
            let s = draw::fit(&mut p.ui, &s, px, r.w - 24 * k);
            let w = p.ui.measure(&s, px);
            let base = draw::baseline(r.y + 24 * k, 20 * k, px);
            draw::text(c, &mut p.ui, &s, px, r.x + (r.w - w) / 2, base, pal.fg_text_secondary, r);
        }
        for j in 0..=m.list_rows {
            let i = m.top + j;
            if i >= m.total {
                break;
            }
            let row_r = draw::rect(r.x, r.y + j as i32 * ROW * k, r.w, ROW * k);
            match m.row_at(i) {
                Some(row) => {
                    self.paint_row(p, c, row, row_r, k, m.open == Some(row.id), now, offset)
                }
                // A row asked for and not here yet: its place.
                None => {
                    let a = draw::rect(row_r.x + PAD * k, row_r.y + 9 * k, AVATAR * k, AVATAR * k);
                    draw::disc(c, a, pal.bg_control_pressed);
                }
            }
            if focused && m.cursor == Some(i) {
                let inner = draw::rect(row_r.x + 2 * k, row_r.y + 2 * k, row_r.w - 4 * k, row_r.h - 4 * k);
                draw::outline(
                    c,
                    inner.intersect(&r),
                    space::RADIUS_CONTROL.physical(p.scale),
                    2 * k as u32,
                    pal.accent,
                );
            }
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        let mut m = self.model.borrow_mut();
        match *e {
            Event::Key {
                code,
                text,
                modifiers,
            } => {
                let page = m.list_rows.max(1) as i32;
                match code {
                    keys::DOWN => m.list_move(1),
                    keys::UP => m.list_move(-1),
                    key::PAGE_DOWN => m.list_move(page),
                    key::PAGE_UP => m.list_move(-page),
                    keys::HOME => m.list_cursor(0),
                    keys::END => {
                        let last = m.total.saturating_sub(1);
                        m.list_cursor(last)
                    }
                    keys::ENTER => match m.cursor {
                        Some(i) => m.open_at(i),
                        None => return Response::None,
                    },
                    keys::ESC => {
                        if !m.escape() {
                            return Response::None;
                        }
                    }
                    // Typing in the list searches.
                    _ => match text {
                        Some(ch)
                            if !ch.is_control()
                                && modifiers & (mods::CTRL | mods::SYSTEM | mods::ALT) == 0 =>
                        {
                            m.query.insert(ch);
                            m.query_changed();
                            m.focus = Some(Role::Search);
                        }
                        _ => return Response::None,
                    },
                }
                Response::Changed(self.id)
            }
            Event::Pointer { .. } => {
                let Some((_, y, _)) = press(e, &mut self.held) else {
                    return Response::None;
                };
                let row = (draw::num(y) - draw::num(self.rect.y)) / ROW;
                let i = m.top + row.max(0) as u32;
                if i >= m.total {
                    return Response::None;
                }
                m.open_at(i);
                Response::Changed(self.id)
            }
            Event::Wheel { dy, .. } => {
                if dy == 0 || !m.list_can_scroll(dy > 0) {
                    self.wheel.stop();
                    return Response::None;
                }
                let rows = self.wheel.turn(dy);
                m.list_scroll(rows);
                if !m.list_can_scroll(dy > 0) {
                    self.wheel.stop();
                }
                Response::Changed(self.id)
            }
            Event::Focus(_) => Response::Repaint,
        }
    }
    fn focusable(&self) -> bool {
        true
    }
    fn id(&self) -> Option<u32> {
        Some(self.id)
    }
}
