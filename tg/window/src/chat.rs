//! The chat view: the header, the messages as bubbles (in and out, replies, forwards,
//! edited, dates, links, service lines, and photos, stickers, voice and files as
//! labelled placeholders), the composer under them, and the summary card over them.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use zoraya_paint::{self as paint, Canvas};
use zoraya_tokens::{Logical, space, text};
use zoraya_ui::{Event, LRect, Painter, Response, WheelSteps, Widget, focus_ring, keys};

use crate::draw::{self, Line, Style};
use crate::model::{Model, Role, kind_word};
use crate::proto::{ChatKind, Id, MessageKind, MessageRow, Scope, SendState, SummaryState};
use crate::time;
use crate::widgets::{Btn, Shared, id, key, mods, press};

/// The header's height, a body line's, and the measures of a bubble, logical.
pub const HEADER: i32 = 48;
pub const LINE: i32 = 19;
const SIDE: i32 = 16;
const PAD_X: i32 = 10;
const PAD_Y: i32 = 7;
const MAX_BUBBLE: i32 = 520;
/// Logical pixels a notch of the wheel moves the messages (three lines of
/// `zoraya-ui`'s rows, a little more than a body line each).
const WHEEL_ROW: i32 = 24;
/// The composer shows at most this many lines.
const COMPOSER_LINES: i32 = 6;
const REPLY_BAR: i32 = 30;

/// A message laid out for one width and scale.
struct Laid {
    rev: u32,
    k: i32,
    w: i32,
    h: i32,
    lines: Vec<Line>,
    /// Offsets from the bubble's top of the parts it has.
    name: Option<i32>,
    forward: Option<i32>,
    reply: Option<i32>,
    tile: Option<(i32, i32, i32)>,
    body: i32,
    /// The time: on the last line's row, or on a row of its own.
    foot: i32,
    foot_w: i32,
}

/// What the view keeps of a message it is not showing: whose it is, its height (a
/// layout's, or an estimate until it had one), and what stands above it.
#[derive(Clone, Copy, Default)]
struct Slot {
    id: Id,
    rev: u32,
    h: i32,
    /// The message before it when `above` and `day` were found.
    before: Id,
    /// The gap above it, and whether a day's line stands above that.
    above: i32,
    day: bool,
}

#[derive(Clone, Copy)]
enum Item {
    Day(i64),
    Msg(usize),
}

pub struct Messages {
    pub id: u32,
    pub model: Shared,
    rect: LRect,
    laid: BTreeMap<Id, Laid>,
    /// A slot a message, in the messages' order.
    slots: Vec<Slot>,
    /// The width, the scale and the zone the slots and the layouts were made for.
    key: (i32, i32, i32),
    /// The items in and near the view, with their tops in the content and their
    /// heights, physical.
    placed: Vec<(Item, i32, i32)>,
    total: i32,
    /// The most the view scrolls up, logical.
    max_scroll: i32,
    /// The visible messages' bubbles from the last paint.
    hits: Vec<(usize, paint::Rect)>,
    wheel: WheelSteps,
    held: bool,
    /// The scale's factor at the last paint.
    k: i32,
}

fn foot_text(m: &MessageRow, offset: i32) -> String {
    let t = time::clock(m.date, offset);
    if m.out && m.state == SendState::Failed {
        format!("Not sent, Enter to retry · {t}")
    } else if m.edited {
        format!("edited {t}")
    } else {
        t
    }
}

impl Messages {
    pub fn new(id: u32, model: Shared) -> Messages {
        Messages {
            id,
            model,
            rect: LRect::default(),
            laid: BTreeMap::new(),
            slots: Vec::new(),
            key: (0, 0, 0),
            placed: Vec::new(),
            total: 0,
            max_scroll: 0,
            hits: Vec::new(),
            wheel: WheelSteps::default(),
            held: false,
            k: 1,
        }
    }

    /// Where the messages are drawn: the widget's rectangle under the header.
    fn region(&self, p: &Painter) -> paint::Rect {
        let k = p.scale.factor() as i32;
        let r = self.rect.physical(p.scale);
        draw::rect(r.x, r.y + HEADER * k, r.w, r.h - HEADER * k)
    }

    fn lay(p: &mut Painter, m: &MessageRow, rev: u32, group: bool, avail: i32, offset: i32) -> Laid {
        let k = p.scale.factor() as i32;
        let px = text::BODY.physical(p.scale);
        let cpx = text::CAPTION.physical(p.scale);
        let mut l = Laid {
            rev,
            k,
            w: 0,
            h: 0,
            lines: Vec::new(),
            name: None,
            forward: None,
            reply: None,
            tile: None,
            body: 0,
            foot: 0,
            foot_w: 0,
        };
        if m.kind == MessageKind::Service {
            l.w = (p.ui.measure(&m.text.text, cpx) + 20 * k).min(avail);
            l.h = 24 * k;
            return l;
        }
        let max_inner = (avail * 7 / 10).min(MAX_BUBBLE * k).max(120 * k) - 2 * PAD_X * k;
        let mut inner = 0;
        let mut y = PAD_Y * k;
        if group && !m.out {
            l.name = Some(y);
            inner = inner.max(p.medium.measure(&m.sender, px).min(max_inner));
            y += 18 * k;
        }
        if let Some(f) = &m.forward {
            l.forward = Some(y);
            let w = p.ui.measure(&format!("Forwarded from {f}"), cpx);
            inner = inner.max(w.min(max_inner));
            y += 17 * k;
        }
        if let Some(q) = &m.reply {
            l.reply = Some(y);
            let w = p
                .ui
                .measure(&q.text, cpx)
                .max(p.medium.measure(&q.sender, cpx))
                + 10 * k;
            inner = inner.max(w.min(max_inner));
            y += 36 * k;
        }
        if m.kind != MessageKind::Text {
            // A placeholder until Zoraya has decoders: a tile with what it is.
            let (tw, th) = match m.kind {
                MessageKind::Photo => (220, 124),
                MessageKind::Sticker => (124, 124),
                _ => (220, 44),
            };
            let tw = (tw * k).min(max_inner);
            l.tile = Some((y, tw, th * k));
            inner = inner.max(tw);
            y += th * k + 4 * k;
        }
        l.body = y;
        let foot = foot_text(m, offset);
        l.foot_w = p.ui.measure(&foot, cpx) + if m.out { 20 * k } else { 0 };
        if !m.text.text.is_empty() {
            let segs = draw::styles(&m.text);
            l.lines = draw::wrap(p, &m.text.text, &segs, px, max_inner);
            for line in &l.lines {
                inner = inner.max(line.w);
            }
            y += l.lines.len() as i32 * LINE * k;
        }
        // The time shares the last line's row when it fits beside it.
        let last = l.lines.last().map(|x| x.w);
        match last {
            Some(w) if w + 10 * k + l.foot_w <= max_inner => {
                inner = inner.max(w + 10 * k + l.foot_w);
                l.foot = y - LINE * k + 3 * k;
            }
            _ => {
                inner = inner.max(l.foot_w);
                l.foot = y;
                y += 15 * k;
            }
        }
        l.w = inner + 2 * PAD_X * k;
        l.h = y + PAD_Y * k;
        l
    }

    /// A message's height before it is laid out: its parts, and its text's lines guessed
    /// from its length. It stands in for the real height while the message is far from
    /// the view.
    fn estimate(m: &MessageRow, group: bool, avail: i32, k: i32) -> i32 {
        if m.kind == MessageKind::Service {
            return 24 * k;
        }
        let max_inner = (avail * 7 / 10).min(MAX_BUBBLE * k).max(120 * k) - 2 * PAD_X * k;
        let mut h = 2 * PAD_Y * k;
        if group && !m.out {
            h += 18 * k;
        }
        if m.forward.is_some() {
            h += 17 * k;
        }
        if m.reply.is_some() {
            h += 36 * k;
        }
        h += match m.kind {
            MessageKind::Text => 0,
            MessageKind::Photo | MessageKind::Sticker => 128 * k,
            _ => 48 * k,
        };
        if m.text.text.is_empty() {
            return h + 15 * k;
        }
        // About seven pixels a character at the body size.
        let per_line = (max_inner / (7 * k)).max(1) as usize;
        let lines: usize = m
            .text
            .text
            .split('\n')
            .map(|l| l.chars().count().div_ceil(per_line).max(1))
            .sum();
        h + lines as i32 * LINE * k
    }

    /// Places the items in and near the view, from the newest message up, laying out
    /// only the messages within `margin` of the view (and `must`, the keyboard's message
    /// when it is to be brought into view). Every other message costs an addition: its
    /// height is kept in its slot, a layout's once it had one, else an estimate. Returns
    /// how many were laid out.
    fn place(&mut self, p: &mut Painter, m: &Model, region: paint::Rect, must: Option<usize>) -> u32 {
        let k = self.k;
        let avail = region.w - 2 * SIDE * k;
        let offset = m.utc_offset;
        let group = m.kind == ChatKind::Group;
        let n = m.messages.len();
        if self.key != (avail, k, offset) {
            self.key = (avail, k, offset);
            self.slots.clear();
            self.laid.clear();
        }
        // A page of older messages came: the slots move with their messages.
        if let Some(first) = self.slots.first()
            && n > self.slots.len()
            && m.messages[n - self.slots.len()].row.id == first.id
        {
            let added = n - self.slots.len();
            self.slots.splice(0..0, core::iter::repeat_n(Slot::default(), added));
        }
        self.slots.resize(n, Slot::default());
        if self.laid.len() > 256 {
            // Those in view are made again below; the heights stay in the slots.
            self.laid.clear();
        }
        let margin = region.h / 2;
        // The view's bottom and top, as distances up from the content's bottom.
        let low = m.scroll * k - 8 * k - margin;
        let high = m.scroll * k + region.h + margin;
        self.placed.clear();
        let mut laid_now = 0;
        // `below` is the distance from the content's bottom to the item's bottom.
        let mut below = 0;
        for i in (0..n).rev() {
            let msg = &m.messages[i];
            let before = if i > 0 { m.messages[i - 1].row.id } else { 0 };
            let slot = &mut self.slots[i];
            if slot.id != msg.row.id || slot.rev != msg.rev {
                *slot = Slot {
                    id: msg.row.id,
                    rev: msg.rev,
                    h: Messages::estimate(&msg.row, group, avail, k),
                    before: Id::MIN,
                    above: 0,
                    day: false,
                };
            }
            if slot.before != before {
                // What stands above it: the day's line when it is the day's first, else
                // a gap, smaller between two messages of one sender.
                slot.before = before;
                let day = |d: i64| (d + i64::from(offset)).div_euclid(86_400);
                match i.checked_sub(1).map(|x| &m.messages[x].row) {
                    Some(prev) if day(prev.date) == day(msg.row.date) => {
                        let same = prev.kind != MessageKind::Service
                            && prev.out == msg.row.out
                            && prev.sender == msg.row.sender;
                        slot.day = false;
                        slot.above = if same { 3 * k } else { 8 * k };
                    }
                    _ => {
                        slot.day = true;
                        slot.above = 8 * k;
                    }
                }
            }
            let near = must == Some(i) || (below <= high && below + slot.h >= low);
            if near {
                let fresh = self.laid.get(&msg.row.id).is_some_and(|l| l.rev == msg.rev);
                if !fresh {
                    let l = Messages::lay(p, &msg.row, msg.rev, group, avail, offset);
                    slot.h = l.h;
                    self.laid.insert(msg.row.id, l);
                    laid_now += 1;
                }
                self.placed.push((Item::Msg(i), below, slot.h));
            }
            below += slot.h + slot.above;
            if slot.day {
                if near {
                    self.placed.push((Item::Day(msg.row.date), below, 34 * k));
                }
                below += 34 * k;
            }
        }
        self.total = below;
        // Top down, each with its top in the content.
        self.placed.reverse();
        for (_, at, h) in &mut self.placed {
            *at = below - *at - *h;
        }
        laid_now
    }

    /// The top of the item `i` of `placed` from the region's top, at `scroll`.
    fn screen_top(&self, i: usize, scroll: i32, region_h: i32) -> i32 {
        region_h - 8 * self.k + scroll * self.k - self.total + self.placed[i].1
    }

    /// Lays out the messages in and near the view, places every item, and keeps the
    /// view where it was: on the message at its top when heights above or below it
    /// changed (an estimate became a layout, a message came), and within the content.
    fn arrange(&mut self, p: &mut Painter) {
        let k = p.scale.factor() as i32;
        self.k = k;
        let region = self.region(p);
        let model = self.model.clone();
        let mut m = model.borrow_mut();
        let must = m.reveal.then_some(m.msg_cursor).flatten();
        let asked = m.scroll;
        let mut laid_now = 0;
        // The scroll may move for the anchor, the cursor or the content's end: each
        // time, what is then in view is laid out; it settles in a round or two.
        for round in 0..4 {
            laid_now += self.place(p, &m, region, must);
            let was = m.scroll;
            // The message that was at the view's top stays where it was, moved by what
            // the person scrolled since; at the newest message the view stays at the end.
            if must.is_none()
                && asked > 0
                && let Some((id, top, scroll)) = m.anchor
                && let Some(i) = self.placed.iter().position(
                    |(it, ..)| matches!(it, Item::Msg(x) if m.messages[*x].row.id == id),
                )
            {
                let want = (top + asked - scroll) * k;
                let now = self.screen_top(i, m.scroll, region.h);
                m.scroll = (m.scroll + (want - now) / k).max(1);
            }
            self.max_scroll = ((self.total + 16 * k - region.h) / k).max(0);
            if round == 0
                && let Some(c) = must
                && let Some((_, top, h)) = self
                    .placed
                    .iter()
                    .find(|(it, ..)| matches!(it, Item::Msg(i) if *i == c))
            {
                // The content's bottom is 8 above the region's when not scrolled.
                let below = (self.total - (top + h)) / k;
                let above = (self.total - top) / k + 16 - region.h / k;
                if m.scroll > below {
                    m.scroll = below;
                } else if m.scroll < above {
                    m.scroll = above;
                }
            }
            m.scroll = m.scroll.clamp(0, self.max_scroll);
            if m.scroll == was {
                break;
            }
        }
        m.reveal = false;
        // The anchor for the next time: the first message that reaches into the view.
        m.anchor = None;
        for (i, (item, _, h)) in self.placed.iter().enumerate() {
            let top = self.screen_top(i, m.scroll, region.h);
            if let Item::Msg(x) = item
                && top + h > 0
            {
                m.anchor = Some((m.messages[*x].row.id, top / k, m.scroll));
                break;
            }
        }
        m.stats.laid = laid_now;
        m.stats.held = self.laid.len() as u32;
        if m.scroll + 240 >= self.max_scroll {
            m.want_older();
        }
    }

    fn pill(p: &mut Painter, c: &mut Canvas<'_>, s: &str, mid: i32, y: i32, clip: paint::Rect) {
        let k = p.scale.factor() as i32;
        let cpx = text::CAPTION.physical(p.scale);
        let w = p.ui.measure(s, cpx) + 20 * k;
        let r = draw::rect(mid - w / 2, y, w, 22 * k).intersect(&clip);
        let full = draw::rect(mid - w / 2, y, w, 22 * k);
        if r == full {
            draw::round(c, full, (11 * k) as u32, draw::mix(p.palette.bg_window, p.palette.fg_text, 70));
        } else {
            c.fill(r, draw::mix(p.palette.bg_window, p.palette.fg_text, 70));
        }
        let base = draw::baseline(y, 22 * k, cpx);
        let fg = p.palette.fg_text_secondary;
        draw::text(c, &mut p.ui, s, cpx, full.x + 10 * k, base, fg, clip);
    }

    /// One or two ticks, or a ring while the message is not confirmed.
    fn ticks(c: &mut Canvas<'_>, p: &Painter, state: SendState, x: i32, y: i32) {
        let k = p.scale.factor() as i32;
        let fg = p.palette.fg_text_secondary;
        let t = (k * 3 / 2).max(1) as u32;
        match state {
            SendState::Pending => {
                draw::ring(c, draw::rect(x + 4 * k, y + k, 9 * k, 9 * k), k as u32, fg)
            }
            // A mark in the error colour: a bar and a dot in a ring.
            SendState::Failed => {
                let e = p.palette.error;
                draw::ring(c, draw::rect(x + 2 * k, y, 11 * k, 11 * k), k as u32, e);
                c.fill(draw::rect(x + 7 * k, y + 2 * k, k, 4 * k), e);
                c.fill(draw::rect(x + 7 * k, y + 7 * k, k, k), e);
            }
            SendState::Sent | SendState::Read => {
                let s = 12 * k;
                let mut tick = |dx: i32, fg| {
                    draw::stroke(c, x + dx, y, s, (k, 6 * k), (4 * k, 9 * k), t, fg);
                    draw::stroke(c, x + dx, y, s, (4 * k, 9 * k), (10 * k, 2 * k), t, fg);
                };
                tick(0, fg);
                if state == SendState::Read {
                    tick(5 * k, p.palette.accent);
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
    fn paint_message(
        &self,
        p: &mut Painter,
        c: &mut Canvas<'_>,
        m: &MessageRow,
        l: &Laid,
        b: paint::Rect,
        clip: paint::Rect,
        offset: i32,
        selected: bool,
        ring: bool,
    ) {
        let k = l.k;
        let pal = p.palette;
        let px = text::BODY.physical(p.scale);
        let cpx = text::CAPTION.physical(p.scale);
        if m.kind == MessageKind::Service {
            Messages::pill(p, c, &m.text.text, b.x + b.w / 2, b.y + k, clip);
            return;
        }
        let radius = space::RADIUS_POPUP.physical(p.scale);
        let old = c.set_clip(clip.intersect(&c.clip));
        let bg = if m.out { pal.bg_selection } else { pal.bg_control };
        draw::round(c, b, radius, bg);
        if !m.out || selected {
            let edge = if selected { pal.border_strong } else { pal.border };
            draw::outline(c, b, radius, k as u32, edge);
        }
        if ring {
            draw::outline(c, b, radius, 2 * k as u32, pal.accent);
        }
        let x = b.x + PAD_X * k;
        let inner_w = b.w - 2 * PAD_X * k;
        if let Some(y) = l.name {
            let base = draw::baseline(b.y + y, 18 * k, px);
            let s = draw::fit(&mut p.medium, &m.sender, px, inner_w);
            draw::text(c, &mut p.medium, &s, px, x, base, pal.accent, b);
        }
        if let (Some(y), Some(f)) = (l.forward, &m.forward) {
            let base = draw::baseline(b.y + y, 17 * k, cpx);
            let s = draw::fit(&mut p.ui, &format!("Forwarded from {f}"), cpx, inner_w);
            draw::text(c, &mut p.ui, &s, cpx, x, base, pal.fg_text_secondary, b);
        }
        if let (Some(y), Some(q)) = (l.reply, &m.reply) {
            // The message answered: a bar, who, and its first line.
            c.fill(draw::rect(x, b.y + y + k, 2 * k, 30 * k), pal.accent);
            let who = draw::fit(&mut p.medium, &q.sender, cpx, inner_w - 10 * k);
            let base = draw::baseline(b.y + y, 16 * k, cpx);
            draw::text(c, &mut p.medium, &who, cpx, x + 10 * k, base, pal.accent, b);
            let what = draw::fit(&mut p.ui, &q.text, cpx, inner_w - 10 * k);
            let base = draw::baseline(b.y + y + 16 * k, 16 * k, cpx);
            draw::text(c, &mut p.ui, &what, cpx, x + 10 * k, base, pal.fg_text_secondary, b);
        }
        if let Some((y, tw, th)) = l.tile {
            let t = draw::rect(x, b.y + y, tw, th);
            draw::round(c, t, space::RADIUS_CONTROL.physical(p.scale), pal.bg_control_pressed);
            let word = kind_word(m.kind);
            if th > 60 * k {
                // A picture's place: its name and size in the middle.
                let w = p.medium.measure(word, px);
                let base = draw::baseline(t.y + th / 2 - 18 * k, 18 * k, px);
                draw::text(c, &mut p.medium, word, px, t.x + (tw - w) / 2, base, pal.fg_text_secondary, t);
                let d = draw::fit(&mut p.ui, &m.detail, cpx, tw - 12 * k);
                let w = p.ui.measure(&d, cpx);
                let base = draw::baseline(t.y + th / 2, 18 * k, cpx);
                draw::text(c, &mut p.ui, &d, cpx, t.x + (tw - w) / 2, base, pal.fg_text_secondary, t);
            } else {
                let base = draw::baseline(t.y + 4 * k, 18 * k, px);
                draw::text(c, &mut p.medium, word, px, t.x + 10 * k, base, pal.fg_text, t);
                let d = draw::fit(&mut p.ui, &m.detail, cpx, tw - 20 * k);
                let base = draw::baseline(t.y + 22 * k, 16 * k, cpx);
                draw::text(c, &mut p.ui, &d, cpx, t.x + 10 * k, base, pal.fg_text_secondary, t);
            }
        }
        draw::lines(c, p, &m.text.text, &l.lines, px, x, b.y + l.body, LINE * k, pal.fg_text, b);
        let foot = foot_text(m, offset);
        let fx = b.x + b.w - PAD_X * k - l.foot_w;
        let base = draw::baseline(b.y + l.foot, 15 * k, cpx);
        let foot_fg = if m.out && m.state == SendState::Failed {
            pal.error
        } else {
            pal.fg_text_secondary
        };
        let w = draw::text(c, &mut p.ui, &foot, cpx, fx, base, foot_fg, b);
        if m.out {
            Messages::ticks(c, p, m.state, fx + w + 4 * k, b.y + l.foot + k);
        }
        c.clip = old;
    }

    fn paint_header(&self, p: &mut Painter, c: &mut Canvas<'_>, m: &Model) {
        let k = p.scale.factor() as i32;
        let r = self.rect.physical(p.scale);
        let pal = p.palette;
        let h = draw::rect(r.x, r.y, r.w, HEADER * k);
        c.fill(h, pal.bg_window);
        c.fill(draw::rect(h.x, h.bottom() - k, h.w, k), pal.border);
        let hpx = text::HEADING.physical(p.scale);
        let cpx = text::CAPTION.physical(p.scale);
        // The Summary button stands at the header's right.
        let room = h.w - 2 * SIDE * k - 110 * k;
        let title = draw::fit(&mut p.medium, &m.title, hpx, room);
        let base = draw::baseline(h.y + 6 * k, 20 * k, hpx);
        draw::text(c, &mut p.medium, &title, hpx, h.x + SIDE * k, base, pal.fg_text, h);
        let sub = draw::fit(&mut p.ui, &m.subtitle, cpx, room);
        let base = draw::baseline(h.y + 26 * k, 16 * k, cpx);
        draw::text(c, &mut p.ui, &sub, cpx, h.x + SIDE * k, base, pal.fg_text_secondary, h);
    }
}

impl Widget for Messages {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (draw::lg(320), draw::lg(240))
    }
    fn layout(&mut self, r: LRect, _p: &mut Painter) {
        self.rect = r;
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, focused: bool) {
        self.arrange(p);
        let k = p.scale.factor() as i32;
        let region = self.region(p);
        let m = self.model.borrow();
        let pal = p.palette;
        c.fill(self.rect.physical(p.scale), pal.bg_window);
        self.hits.clear();
        let offset = m.utc_offset;
        let now = m.now();
        // The content's last pixel stands 8 above the region's bottom when not scrolled.
        let origin = region.bottom() - 8 * k + m.scroll * k - self.total;
        let selection = m.selection();
        for (item, top, h) in &self.placed {
            let y = origin + top;
            if y + h < region.y || y > region.bottom() {
                continue;
            }
            match *item {
                Item::Day(date) => {
                    let s = time::day_label(date, now, offset);
                    Messages::pill(p, c, &s, region.x + region.w / 2, y + 8 * k, region);
                }
                Item::Msg(i) => {
                    let msg = &m.messages[i];
                    let Some(l) = self.laid.get(&msg.row.id) else {
                        continue;
                    };
                    let x = if msg.row.kind == MessageKind::Service {
                        region.x + (region.w - l.w) / 2
                    } else if msg.row.out {
                        region.right() - SIDE * k - l.w
                    } else {
                        region.x + SIDE * k
                    };
                    let b = draw::rect(x, y, l.w, l.h);
                    let selected = selection.is_some_and(|(a, z)| i >= a && i <= z);
                    if selected {
                        let band = draw::rect(region.x, y - k, region.w, l.h + 2 * k);
                        c.fill(band.intersect(&region), pal.bg_control_pressed);
                    }
                    let ring = focused && m.msg_cursor == Some(i);
                    self.paint_message(p, c, &msg.row, l, b, region, offset, selected, ring);
                    self.hits.push((i, b.intersect(&region)));
                }
            }
        }
        if m.open.is_some() && m.messages.is_empty() {
            let px = text::BODY.physical(p.scale);
            let s = "No messages here yet";
            let w = p.ui.measure(s, px);
            let base = draw::baseline(region.y + region.h / 2 - 10 * k, 20 * k, px);
            draw::text(c, &mut p.ui, s, px, region.x + (region.w - w) / 2, base, pal.fg_text_secondary, region);
        }
        self.paint_header(p, c, &m);
        if focused {
            focus_ring(p, c, LRect::new(
                self.rect.x,
                self.rect.y + draw::lg(HEADER),
                self.rect.w,
                self.rect.h - draw::lg(HEADER),
            ));
        }
    }
    fn event(&mut self, e: &Event) -> Response {
        let mut m = self.model.borrow_mut();
        let page = (draw::num(self.rect.h) - HEADER - 2 * LINE).max(LINE);
        match *e {
            Event::Key {
                code, modifiers, ..
            } => {
                let shift = modifiers & mods::SHIFT != 0;
                match code {
                    keys::UP => m.msg_move(-1, shift),
                    keys::DOWN => m.msg_move(1, shift),
                    key::PAGE_UP => m.scroll = (m.scroll + page).min(self.max_scroll),
                    key::PAGE_DOWN => m.scroll = (m.scroll - page).max(0),
                    keys::HOME => m.scroll = self.max_scroll,
                    keys::END => m.scroll = 0,
                    keys::ENTER => m.activate_cursor(),
                    key::R => m.reply_to_cursor(),
                    keys::ESC => {
                        if !m.escape() {
                            return Response::None;
                        }
                    }
                    _ => return Response::None,
                }
                Response::Changed(self.id)
            }
            Event::Pointer { x, y, .. } => {
                let Some((.., modifiers)) = press(e, &mut self.held) else {
                    return Response::None;
                };
                // The hits are physical, the event logical.
                let (px, py) = (draw::num(x) * self.k, draw::num(y) * self.k);
                match self.hits.iter().find(|(_, r)| r.contains(px, py)) {
                    // A press on a failed message that is already chosen sends it again.
                    Some((i, _))
                        if m.msg_cursor == Some(*i)
                            && modifiers & mods::SHIFT == 0
                            && m.retry_cursor() => {}
                    Some((i, _)) => m.msg_select(*i, modifiers & mods::SHIFT != 0),
                    None => {
                        m.msg_cursor = None;
                        m.msg_anchor = None;
                    }
                }
                Response::Changed(self.id)
            }
            Event::Wheel { dy, .. } => {
                // Toward the person: newer messages, so less scrolled up.
                let can = if dy > 0 {
                    m.scroll > 0
                } else {
                    dy < 0 && m.scroll < self.max_scroll
                };
                if !can {
                    self.wheel.stop();
                    return Response::None;
                }
                let rows = self.wheel.turn(dy);
                m.scroll = (m.scroll - rows * WHEEL_ROW).clamp(0, self.max_scroll);
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

/// The composer's lines for a width, and its height, logical: the text's lines (at
/// most six shown), its padding, and the reply's bar.
pub fn composer_height(p: &mut Painter, m: &Model, width: Logical) -> Logical {
    let k = p.scale.factor() as i32;
    let px = text::BODY.physical(p.scale);
    let inner = (draw::num(width) - 2 * PAD_X) * k;
    let n = draw::wrap(p, &m.composer.text, &[(0, m.composer.text.len(), Style::Plain)], px, inner)
        .len() as i32;
    let bar = if m.reply_to.is_some() { REPLY_BAR } else { 0 };
    draw::lg(n.clamp(1, COMPOSER_LINES) * LINE + 12 + bar)
}

/// The message being written: several lines, Enter sends, Shift+Enter starts a line.
pub struct Composer {
    pub id: u32,
    pub model: Shared,
    rect: LRect,
    /// The lines of the last paint, for Up and Down.
    lines: Vec<Line>,
    /// The first line shown, when there are more than fit.
    first: usize,
    held: bool,
}

impl Composer {
    pub fn new(id: u32, model: Shared) -> Composer {
        Composer {
            id,
            model,
            rect: LRect::default(),
            lines: Vec::new(),
            first: 0,
            held: false,
        }
    }

    /// The line the cursor is on.
    fn line_of(lines: &[Line], cursor: usize) -> usize {
        lines
            .iter()
            .rposition(|l| l.a <= cursor)
            .unwrap_or(0)
    }

    /// The cursor moved a line up or down, keeping its column in characters.
    fn vertical(&self, m: &mut Model, down: bool) -> bool {
        let text = &m.composer.text;
        let at = Composer::line_of(&self.lines, m.composer.cursor);
        let to = if down { at + 1 } else { at.wrapping_sub(1) };
        let (Some(from), Some(target)) = (self.lines.get(at), self.lines.get(to)) else {
            return false;
        };
        let end = m.composer.cursor.clamp(from.a, text.len());
        let column = text[from.a..end].chars().count();
        let line = &text[target.a..target.b.max(target.a)];
        let byte = line
            .char_indices()
            .nth(column)
            .map_or(line.len(), |(i, _)| i);
        m.composer.cursor = target.a + byte;
        true
    }
}

impl Widget for Composer {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (draw::lg(240), draw::lg(LINE + 12))
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
        let pal = p.palette;
        let px = text::BODY.physical(p.scale);
        let cpx = text::CAPTION.physical(p.scale);
        let m = self.model.borrow();
        let mut field = r;
        if let Some(q) = m.reply_row() {
            // The message being answered, above the field.
            let bar = draw::rect(r.x, r.y, r.w, REPLY_BAR * k);
            c.fill(draw::rect(bar.x + 2 * k, bar.y + 2 * k, 2 * k, 26 * k), pal.accent);
            let who = format!("Reply to {}", if q.out { "yourself" } else { q.sender.as_str() });
            let who = draw::fit(&mut p.medium, &who, cpx, bar.w - 14 * k);
            let base = draw::baseline(bar.y, 15 * k, cpx);
            draw::text(c, &mut p.medium, &who, cpx, bar.x + 12 * k, base, pal.accent, bar);
            let what = match q.kind {
                MessageKind::Text | MessageKind::Service => q.text.text.replace('\n', " "),
                kind => String::from(kind_word(kind)),
            };
            let what = draw::fit(&mut p.ui, &what, cpx, bar.w - 14 * k);
            let base = draw::baseline(bar.y + 14 * k, 15 * k, cpx);
            draw::text(c, &mut p.ui, &what, cpx, bar.x + 12 * k, base, pal.fg_text_secondary, bar);
            field = draw::rect(r.x, r.y + REPLY_BAR * k, r.w, r.h - REPLY_BAR * k);
        }
        let radius = space::RADIUS_CONTROL.physical(p.scale);
        draw::round(c, field, radius, pal.bg_view);
        draw::outline(c, field, radius, k as u32, pal.border_strong);
        let inner = draw::rect(field.x + PAD_X * k, field.y + 6 * k, field.w - 2 * PAD_X * k, field.h - 12 * k);
        let t = &m.composer.text;
        self.lines = draw::wrap(p, t, &[(0, t.len(), Style::Plain)], px, inner.w);
        let shown = (inner.h / (LINE * k)).max(1) as usize;
        let at = Composer::line_of(&self.lines, m.composer.cursor);
        // The cursor's line is always among those shown.
        self.first = self.first.min(self.lines.len().saturating_sub(shown));
        if at < self.first {
            self.first = at;
        } else if at >= self.first + shown {
            self.first = at + 1 - shown;
        }
        if t.is_empty() {
            let base = draw::baseline(inner.y, LINE * k, px);
            if !focused {
                draw::text(c, &mut p.ui, "Message", px, inner.x, base, pal.fg_text_disabled, inner);
            }
        }
        let y = inner.y - self.first as i32 * LINE * k;
        draw::lines(c, p, t, &self.lines, px, inner.x, y, LINE * k, pal.fg_text, inner);
        if focused {
            let line = &self.lines[at.min(self.lines.len() - 1)];
            let upto = m.composer.cursor.clamp(line.a, t.len());
            let x = inner.x + p.ui.measure(&t[line.a..upto], px);
            let top = y + at as i32 * LINE * k;
            c.fill(draw::rect(x, top + 2 * k, k, (LINE - 4) * k).intersect(&inner), pal.fg_text);
            let ring = LRect::new(
                self.rect.x,
                self.rect.y + draw::lg((field.y - r.y) / k),
                self.rect.w,
                draw::lg(field.h / k),
            );
            focus_ring(p, c, ring);
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
        let chord = modifiers & (mods::CTRL | mods::SYSTEM) != 0;
        match code {
            keys::ENTER if modifiers & mods::SHIFT != 0 => m.composer.insert('\n'),
            keys::ENTER => m.send(),
            keys::ESC => {
                if !m.escape() {
                    return Response::None;
                }
            }
            keys::BACKSPACE => {
                m.composer.backspace();
            }
            keys::DELETE => {
                m.composer.delete();
            }
            keys::LEFT => m.composer.left(),
            keys::RIGHT => m.composer.right(),
            keys::HOME => m.composer.home(),
            keys::END => m.composer.end(),
            keys::UP => {
                // From the first line, Up goes to the messages.
                if !self.vertical(&mut m, false) {
                    m.msg_move(-1, false);
                    m.focus = Some(Role::Messages);
                }
            }
            keys::DOWN => {
                self.vertical(&mut m, true);
            }
            key::PAGE_UP => m.scroll += 10 * LINE,
            key::PAGE_DOWN => m.scroll = (m.scroll - 10 * LINE).max(0),
            key::F if chord => m.focus = Some(Role::Search),
            _ => match text {
                Some(ch) if !ch.is_control() && !chord => m.composer.insert(ch),
                _ => return Response::None,
            },
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

/// The summary: the application's own card over the messages (no glow: that is the
/// compositor's alone), with the scope's buttons, the text as it comes, and Close.
pub struct Card {
    pub model: Shared,
    kids: Vec<Box<dyn Widget>>,
    rect: LRect,
}

pub const SCOPES: [(Scope, &str); 3] = [
    (Scope::Unread, "Unread"),
    (Scope::Day, "Last day"),
    (Scope::Week, "Last week"),
];

impl Card {
    pub fn new(model: Shared) -> Card {
        let scope = model.borrow().summary.as_ref().map(|s| s.scope);
        let mut kids: Vec<Box<dyn Widget>> = Vec::new();
        for (i, (s, label)) in SCOPES.iter().enumerate() {
            let mut b = Btn::new(id::SCOPE + i as u32, label, scope == Some(*s));
            b.model = Some(model.clone());
            kids.push(Box::new(b));
        }
        let mut close = Btn::new(id::CLOSE, "Close", false);
        close.model = Some(model.clone());
        kids.push(Box::new(close));
        Card {
            model,
            kids,
            rect: LRect::default(),
        }
    }
}

impl Widget for Card {
    fn measure(&mut self, _p: &mut Painter) -> (Logical, Logical) {
        (draw::lg(440), draw::lg(280))
    }
    fn layout(&mut self, r: LRect, p: &mut Painter) {
        self.rect = r;
        let pad = draw::lg(16);
        let mut x = r.x + pad;
        let y = r.y + draw::lg(48);
        for kid in self.kids.iter_mut().take(3) {
            let (w, h) = kid.measure(p);
            kid.layout(LRect::new(x, y, w, h), p);
            x = x + w + space::GAP_CONTROLS;
        }
        let (w, h) = self.kids[3].measure(p);
        self.kids[3].layout(
            LRect::new(r.right() - pad - w, r.bottom() - pad - h, w, h),
            p,
        );
    }
    fn rect(&self) -> LRect {
        self.rect
    }
    fn paint(&mut self, p: &mut Painter, c: &mut Canvas<'_>, _focused: bool) {
        let k = p.scale.factor() as i32;
        let r = self.rect.physical(p.scale);
        let pal = p.palette;
        let radius = space::RADIUS_POPUP.physical(p.scale);
        draw::round(c, r, radius, pal.bg_view);
        draw::outline(c, r, radius, k as u32, pal.border_strong);
        let m = self.model.borrow();
        let Some(s) = &m.summary else { return };
        let hpx = text::HEADING.physical(p.scale);
        let px = text::BODY.physical(p.scale);
        let x = r.x + 16 * k;
        let inner_w = r.w - 32 * k;
        let title = draw::fit(&mut p.medium, &format!("Summary · {}", m.title), hpx, inner_w);
        let base = draw::baseline(r.y + 14 * k, 22 * k, hpx);
        draw::text(c, &mut p.medium, &title, hpx, x, base, pal.fg_text, r);
        let area = draw::rect(x, r.y + 90 * k, inner_w, r.h - 90 * k - 56 * k);
        let fg = if s.state == SummaryState::Failed {
            pal.error
        } else {
            pal.fg_text
        };
        let segs = [(0, s.text.len(), Style::Plain)];
        let lines = draw::wrap(p, &s.text, &segs, px, inner_w);
        draw::lines(c, p, &s.text, &lines, px, x, area.y, LINE * k, fg, area);
        if s.state == SummaryState::Running {
            let base = draw::baseline(r.bottom() - 44 * k, 28 * k, px);
            draw::text(c, &mut p.ui, "Summarising…", px, x, base, pal.fg_text_secondary, r);
        }
    }
    fn children(&mut self) -> &mut [Box<dyn Widget>] {
        &mut self.kids
    }
}
