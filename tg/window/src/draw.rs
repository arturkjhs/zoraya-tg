//! Drawing shared by the widgets: text at a pen, shapes from the tokens, a line of text
//! cut to a width, and a text with its spans broken into lines. Everything is in
//! physical pixels (logical times the scale's factor `k`), as the file manager draws.

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_paint::{self as paint, Canvas};
use zoraya_text::{Rect as TRect, Target, Text};
use zoraya_tokens::{Color, CornerShape, Logical, Scale, shape};
use zoraya_ui::Painter;

use crate::proto::{Rich, SpanKind};

/// A logical size from a number of logical pixels (the mockup's measures).
pub fn lg(v: i32) -> Logical {
    Scale::ONE.logical(v.max(0) as u32)
}

/// A logical size as a number.
pub fn num(l: Logical) -> i32 {
    l.physical(Scale::ONE) as i32
}

pub fn rect(x: i32, y: i32, w: i32, h: i32) -> paint::Rect {
    paint::Rect::new(x, y, w.max(0), h.max(0))
}

/// `a` with `t` thousandths of `b`.
pub fn mix(a: Color, b: Color, t: u32) -> Color {
    let ch = |x: u8, y: u8| -> u32 { (u32::from(x) * (1000 - t) + u32::from(y) * t + 500) / 1000 };
    Color::of_pixel(ch(a.r(), b.r()) << 16 | ch(a.g(), b.g()) << 8 | ch(a.b(), b.b()))
}

/// The baseline that centres text of `px` in a box of height `h` at `y`.
pub fn baseline(y: i32, h: i32, px: u32) -> i32 {
    y + h / 2 + px as i32 * 7 / 20
}

/// Draws `s` with its pen at `(x, baseline)`, clipped to `clip`; its advance.
#[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
pub fn text(
    c: &mut Canvas<'_>,
    t: &mut Text<'static>,
    s: &str,
    px: u32,
    x: i32,
    baseline: i32,
    color: Color,
    clip: paint::Rect,
) -> i32 {
    let clip = clip.intersect(&c.clip);
    if clip.is_empty() || s.is_empty() {
        return 0;
    }
    let mut target = Target {
        pixels: c.pixels,
        stride: c.width,
        clip: TRect::new(clip.x, clip.y, clip.w as u32, clip.h as u32),
    };
    t.draw(&mut target, s, px, x, baseline, color)
}

/// The side of the square a corner of `radius` takes, if `r` has room for four of them
/// and a middle; else the shape is drawn from one mask of its whole size.
fn corner(r: paint::Rect, radius: u32) -> Option<i32> {
    let s = paint::corner_span(shape::CORNER, radius) as i32 + 1;
    (r.w >= 2 * s && r.h >= 2 * s).then_some(s)
}

/// Draws the four corners of `r` from `m`, a mask of the shape at `2s × 2s`: each
/// quarter of it through a clip of that corner. A frame draws dozens of bubbles and
/// boxes; this way none of them makes a mask of its own size.
fn corners(c: &mut Canvas<'_>, m: &paint::Mask, r: paint::Rect, s: i32, color: Color) {
    let old = c.clip;
    for (cx, mx) in [(r.x, r.x), (r.right() - s, r.right() - 2 * s)] {
        for (cy, my) in [(r.y, r.y), (r.bottom() - s, r.bottom() - 2 * s)] {
            c.clip = rect(cx, cy, s, s).intersect(&old);
            c.mask(m, mx, my, color, 255);
        }
    }
    c.clip = old;
}

/// A filled shape with the tokens' corners.
pub fn round(c: &mut Canvas<'_>, r: paint::Rect, radius: u32, color: Color) {
    if r.is_empty() {
        return;
    }
    let Some(s) = corner(r, radius) else {
        let m = paint::rounded_rect(r.w as usize, r.h as usize, radius, shape::CORNER);
        c.mask(&m, r.x, r.y, color, 255);
        return;
    };
    c.fill(rect(r.x + s, r.y, r.w - 2 * s, r.h), color);
    c.fill(rect(r.x, r.y + s, s, r.h - 2 * s), color);
    c.fill(rect(r.right() - s, r.y + s, s, r.h - 2 * s), color);
    let m = paint::rounded_rect(2 * s as usize, 2 * s as usize, radius, shape::CORNER);
    corners(c, &m, r, s, color);
}

/// The outline of that shape, `t` pixels thick.
pub fn outline(c: &mut Canvas<'_>, r: paint::Rect, radius: u32, t: u32, color: Color) {
    if r.is_empty() {
        return;
    }
    let ti = t as i32;
    let Some(s) = corner(r, radius).filter(|s| *s >= ti) else {
        let m = paint::rounded_outline(r.w as usize, r.h as usize, radius, t, shape::CORNER);
        c.mask(&m, r.x, r.y, color, 255);
        return;
    };
    c.fill(rect(r.x + s, r.y, r.w - 2 * s, ti), color);
    c.fill(rect(r.x + s, r.bottom() - ti, r.w - 2 * s, ti), color);
    c.fill(rect(r.x, r.y + s, ti, r.h - 2 * s), color);
    c.fill(rect(r.right() - ti, r.y + s, ti, r.h - 2 * s), color);
    let m = paint::rounded_outline(2 * s as usize, 2 * s as usize, radius, t, shape::CORNER);
    corners(c, &m, r, s, color);
}

/// A filled circle in the square `r`.
pub fn disc(c: &mut Canvas<'_>, r: paint::Rect, color: Color) {
    if r.is_empty() {
        return;
    }
    let m = paint::rounded_rect(
        r.w as usize,
        r.h as usize,
        (r.w / 2) as u32,
        CornerShape::Circle,
    );
    c.mask(&m, r.x, r.y, color, 255);
}

/// A circle's outline in the square `r`.
pub fn ring(c: &mut Canvas<'_>, r: paint::Rect, t: u32, color: Color) {
    if r.is_empty() {
        return;
    }
    let m = paint::rounded_outline(
        r.w as usize,
        r.h as usize,
        (r.w / 2) as u32,
        t,
        CornerShape::Circle,
    );
    c.mask(&m, r.x, r.y, color, 255);
}

/// A line from `a` to `b`, both relative to the square at `(x, y)` of side `side`.
#[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
pub fn stroke(
    c: &mut Canvas<'_>,
    x: i32,
    y: i32,
    side: i32,
    a: (i32, i32),
    b: (i32, i32),
    t: u32,
    color: Color,
) {
    let m = paint::segment(side.max(1) as usize, side.max(1) as usize, a, b, t);
    c.mask(&m, x, y, color, 255);
}

/// `s`, or its start with an ellipsis, so that it is at most `max` pixels wide.
pub fn fit(t: &mut Text<'static>, s: &str, px: u32, max: i32) -> String {
    if t.measure(s, px) <= max {
        return String::from(s);
    }
    let dots = t.measure("…", px);
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let mut b = [0u8; 4];
        let cw = t.measure(ch.encode_utf8(&mut b), px);
        if w + cw + dots > max {
            break;
        }
        w += cw;
        out.push(ch);
    }
    let kept = out.trim_end().len();
    out.truncate(kept);
    out.push('…');
    out
}

/// The first letters of a title's first two words, in capitals: an avatar without a
/// photo.
pub fn initials(title: &str) -> String {
    let mut out = String::new();
    for word in title.split(|c: char| !c.is_alphanumeric()) {
        if let Some(c) = word.chars().next() {
            out.extend(c.to_uppercase());
            if out.chars().count() >= 2 {
                break;
            }
        }
    }
    out
}

/// How a stretch of text is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Plain,
    /// Inter Medium.
    Bold,
    /// JetBrains Mono.
    Code,
    /// In the accent, underlined: a link or a mention.
    Link,
}

/// A stretch of one style on one line: bytes `[a, b)` of the text, `w` pixels wide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Run {
    pub a: usize,
    pub b: usize,
    pub style: Style,
    pub w: i32,
}

/// A line of a wrapped text: bytes `[a, b)` (without the break that ended it).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Line {
    pub runs: Vec<Run>,
    pub w: i32,
    pub a: usize,
    pub b: usize,
}

/// The style of every byte range of a text, from its spans (which count code points).
/// Italic has no font of its own in the system and is drawn plain.
pub fn styles(r: &Rich) -> Vec<(usize, usize, Style)> {
    let n = r.text.chars().count();
    let mut per = alloc::vec![Style::Plain; n];
    for s in &r.spans {
        let style = match s.kind {
            SpanKind::Bold => Style::Bold,
            SpanKind::Code => Style::Code,
            SpanKind::Link | SpanKind::Mention => Style::Link,
            SpanKind::Italic => continue,
        };
        for slot in per
            .iter_mut()
            .take((s.end as usize).min(n))
            .skip(s.start as usize)
        {
            *slot = style;
        }
    }
    let mut out: Vec<(usize, usize, Style)> = Vec::new();
    for ((at, ch), style) in r.text.char_indices().zip(per) {
        let end = at + ch.len_utf8();
        match out.last_mut() {
            Some((_, b, s)) if *s == style => *b = end,
            _ => out.push((at, end, style)),
        }
    }
    out
}

pub fn font<'p>(p: &'p mut Painter, style: Style) -> &'p mut Text<'static> {
    match style {
        Style::Plain | Style::Link => &mut p.ui,
        Style::Bold => &mut p.medium,
        Style::Code => &mut p.mono,
    }
}

struct Wrapper<'a> {
    text: &'a str,
    max: i32,
    lines: Vec<Line>,
    /// The line being filled: words and spaces, not yet merged.
    cur: Vec<Run>,
    w: i32,
    /// Where the line can be broken: the index in `cur` of its last space.
    space: Option<usize>,
    start: usize,
}

impl Wrapper<'_> {
    /// Ends the line with `cur[..n]` and starts the next with what follows index
    /// `skip` (the space the line broke at is dropped).
    fn emit(&mut self, n: usize, skip: usize) {
        let rest: Vec<Run> = self.cur.split_off(skip.min(self.cur.len()));
        self.cur.truncate(n);
        // Spaces at a line's end take no room.
        while self
            .cur
            .last()
            .is_some_and(|r| self.text[r.a..r.b].trim().is_empty())
        {
            self.cur.pop();
        }
        let mut line = Line {
            a: self.start,
            b: self.cur.last().map_or(self.start, |r| r.b),
            ..Line::default()
        };
        for r in self.cur.drain(..) {
            line.w += r.w;
            match line.runs.last_mut() {
                Some(l) if l.style == r.style && l.b == r.a => {
                    l.b = r.b;
                    l.w += r.w;
                }
                _ => line.runs.push(r),
            }
        }
        self.lines.push(line);
        self.start = rest.first().map_or(self.start, |r| r.a);
        self.w = rest.iter().map(|r| r.w).sum();
        self.cur = rest;
        self.space = None;
    }

    /// Adds a word or a space to the line; the first one says where the line starts.
    fn push(&mut self, r: Run) {
        if self.cur.is_empty() {
            self.start = r.a;
        }
        self.w += r.w;
        self.cur.push(r);
    }
}

/// Breaks `text` (its styles from [`styles`]) into lines of at most `max` pixels at
/// `px`: at spaces, at every newline, and inside a word longer than a line.
pub fn wrap(
    p: &mut Painter,
    text: &str,
    segs: &[(usize, usize, Style)],
    px: u32,
    max: i32,
) -> Vec<Line> {
    let mut w = Wrapper {
        text,
        max: max.max(1),
        lines: Vec::new(),
        cur: Vec::new(),
        w: 0,
        space: None,
        start: 0,
    };
    for (sa, sb, style) in segs.iter().copied() {
        let mut at = sa;
        while at < sb {
            let rest = &text[at..sb];
            let ch = rest.chars().next().unwrap_or(' ');
            if ch == '\n' {
                let n = w.cur.len();
                w.emit(n, n);
                at += 1;
                w.start = at;
                continue;
            }
            if ch == ' ' || ch == '\t' {
                let sw = font(p, style).measure(" ", px);
                w.space = Some(w.cur.len());
                w.push(Run {
                    a: at,
                    b: at + 1,
                    style,
                    w: sw,
                });
                at += 1;
                continue;
            }
            // A word: up to the next space or newline of this stretch.
            let len = rest
                .find([' ', '\t', '\n'])
                .unwrap_or(rest.len());
            let mut word = (at, at + len);
            at += len;
            loop {
                let ww = font(p, style).measure(&text[word.0..word.1], px);
                if w.w + ww <= w.max {
                    w.push(Run {
                        a: word.0,
                        b: word.1,
                        style,
                        w: ww,
                    });
                    break;
                }
                if let Some(s) = w.space {
                    w.emit(s, s + 1);
                    continue;
                }
                // No space to break at: as many characters as fit, at least one on an
                // empty line.
                let mut cut = word.0;
                let mut cw = 0;
                for (i, ch) in text[word.0..word.1].char_indices() {
                    let end = word.0 + i + ch.len_utf8();
                    let now = font(p, style).measure(&text[word.0..end], px);
                    if w.w + now > w.max && !(w.cur.is_empty() && cut == word.0) {
                        break;
                    }
                    cut = end;
                    cw = now;
                    if w.w + now > w.max {
                        break;
                    }
                }
                if cut > word.0 {
                    w.push(Run {
                        a: word.0,
                        b: cut,
                        style,
                        w: cw,
                    });
                    word.0 = cut;
                }
                let n = w.cur.len();
                w.emit(n, n);
                w.start = word.0;
                if word.0 >= word.1 {
                    break;
                }
            }
        }
    }
    if !w.cur.is_empty() || w.lines.is_empty() || text.ends_with('\n') {
        let n = w.cur.len();
        w.emit(n, n);
    }
    w.lines
}

/// Draws a wrapped text's lines from `(x, y)`, each `line_h` high.
#[allow(clippy::too_many_arguments)] // Drawing state, passed as it is used.
pub fn lines(
    c: &mut Canvas<'_>,
    p: &mut Painter,
    text_of: &str,
    lines: &[Line],
    px: u32,
    x: i32,
    y: i32,
    line_h: i32,
    color: Color,
    clip: paint::Rect,
) {
    let k = p.scale.factor() as i32;
    for (i, l) in lines.iter().enumerate() {
        let top = y + i as i32 * line_h;
        if top + line_h < clip.y || top > clip.bottom() {
            continue;
        }
        let base = baseline(top, line_h, px);
        let mut pen = x;
        for r in &l.runs {
            let colour = if r.style == Style::Link {
                p.palette.accent
            } else {
                color
            };
            text(c, font(p, r.style), &text_of[r.a..r.b], px, pen, base, colour, clip);
            if r.style == Style::Link {
                let under = rect(pen, base + 2 * k, r.w, k).intersect(&clip);
                c.fill(under, colour);
            }
            pen += r.w;
        }
    }
}
