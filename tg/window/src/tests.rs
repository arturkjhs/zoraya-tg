//! Host tests: the dates, the text's wrapping, the model over the sample backend (the
//! login, the chat list's window and its diffs, opening, sending, the timer, search,
//! paging, the summary, settings), and the widgets through `zoraya-ui`'s window
//! (presses, keys, the wheel, painting at both scales in both themes, a resize).

use std::cell::RefCell;
use std::rc::Rc;
use std::string::String;
use std::vec::Vec;

use zoraya_tokens::{Appearance, Scale};
use zoraya_ui::{Event, Painter, Response, Window, keys};

use crate::draw::{self, Style, lg};
use crate::list::ROW;
use crate::model::{Model, Role, Screen};
use crate::proto::{
    AuthState, Backend, ChatRow, Event as Ev, MessageKind, Request, Scope, SendState, SpanKind,
    SummaryState,
};
use crate::sample::{self, SampleBackend};
use crate::view::{self, HEIGHT, LIST_W, WIDTH};
use crate::widgets::{Shared, key, mods};
use crate::{chat, time};

fn clock() -> u64 {
    0
}

/// The window as the program runs it, on the host.
struct Ui {
    model: Shared,
    backend: SampleBackend,
    now: u64,
    win: Window,
    p: Painter,
    roles: Vec<(Role, Vec<usize>)>,
    built: u32,
    scale: Scale,
    lines: Vec<String>,
}

impl Ui {
    fn new(scale: Scale, look: Appearance, w: u32, h: u32, logged_in: bool) -> Ui {
        let model = Rc::new(RefCell::new(Model::new()));
        let backend = if logged_in {
            SampleBackend::logged_in()
        } else {
            SampleBackend::new()
        };
        let mut backend = backend;
        // What the backend says first, before the first tree (as the program does).
        while let Some(e) = backend.poll(0) {
            model.borrow_mut().apply(e);
        }
        let mut p = Painter::new(scale, look);
        let tree = view::build(&model, clock);
        let mut win = Window::new(tree.root, (lg(w as i32), lg(h as i32)));
        win.layout(&mut p);
        let built = model.borrow().tree;
        let mut ui = Ui {
            built,
            model,
            backend,
            now: 0,
            win,
            p,
            roles: tree.roles,
            scale,
            lines: Vec::new(),
        };
        ui.pump();
        ui
    }

    fn main() -> Ui {
        Ui::new(Scale::ONE, Appearance::Light, WIDTH, HEIGHT, true)
    }

    /// As the program's round: requests out, events in, a new tree when the model's
    /// changed, the focus where the model wants it.
    fn pump(&mut self) {
        loop {
            let requests = {
                let mut m = self.model.borrow_mut();
                m.set_clock(self.now);
                m.take_requests()
            };
            let asked = !requests.is_empty();
            for r in requests {
                self.backend.send(r);
            }
            let mut told = false;
            while let Some(e) = self.backend.poll(self.now) {
                self.model.borrow_mut().apply(e);
                told = true;
            }
            if self.model.borrow().tree != self.built {
                let was = self
                    .win
                    .focused()
                    .and_then(|p| self.roles.iter().find(|(_, x)| *x == p).map(|(r, _)| *r));
                let tree = view::build(&self.model, clock);
                self.built = self.model.borrow().tree;
                self.roles = tree.roles;
                self.win.root = tree.root;
                self.win.layout(&mut self.p);
                let mut m = self.model.borrow_mut();
                if m.focus.is_none() {
                    m.focus = was;
                }
                continue;
            }
            if !asked && !told {
                break;
            }
        }
        let wanted = self.model.borrow_mut().focus.take();
        if let Some(role) = wanted {
            self.focus(role);
        }
        self.lines.extend(self.model.borrow_mut().take_trace());
    }

    fn focus(&mut self, role: Role) {
        let Some((_, path)) = self.roles.iter().find(|(r, _)| *r == role) else {
            panic!("no {role:?} in the tree");
        };
        for _ in 0..self.roles.len() + 1 {
            if self.win.focused().as_ref() == Some(path) {
                return;
            }
            self.win.move_focus(false);
        }
        panic!("{role:?} takes no focus");
    }

    fn focused(&mut self) -> Option<Role> {
        let p = self.win.focused()?;
        self.roles.iter().find(|(_, x)| *x == p).map(|(r, _)| *r)
    }

    fn advance(&mut self, ms: u64) {
        self.now += ms;
        self.pump();
    }

    fn event(&mut self, e: Event) -> Response {
        let r = self.win.event(e);
        view::respond(&mut self.model.borrow_mut(), r);
        self.win.layout(&mut self.p);
        self.pump();
        r
    }

    fn key(&mut self, code: u32) -> Response {
        self.event(Event::Key {
            code,
            text: None,
            modifiers: 0,
        })
    }

    fn chord(&mut self, code: u32, modifiers: u32) -> Response {
        self.event(Event::Key {
            code,
            text: None,
            modifiers,
        })
    }

    fn type_text(&mut self, s: &str) {
        for ch in s.chars() {
            self.event(Event::Key {
                code: 0,
                text: Some(ch),
                modifiers: 0,
            });
        }
    }

    fn click(&mut self, x: i32, y: i32) {
        for buttons in [0, 1, 0] {
            self.event(Event::Pointer {
                x: lg(x),
                y: lg(y),
                buttons,
                modifiers: 0,
            });
        }
    }

    fn wheel(&mut self, x: i32, y: i32, notches: i32) -> Response {
        self.event(Event::Wheel {
            x: lg(x),
            y: lg(y),
            dx: 0,
            dy: notches * 120,
        })
    }

    fn paint(&mut self) -> (Vec<u32>, usize) {
        let k = self.scale.factor() as usize;
        let (w, h) = (
            draw::num(self.win.size.0) as usize * k,
            draw::num(self.win.size.1) as usize * k,
        );
        let mut pixels = vec![0u32; w * h];
        self.win.paint(&mut self.p, &mut pixels, w);
        (pixels, w)
    }

    /// The pixel at a logical point.
    fn pixel(&mut self, x: i32, y: i32) -> u32 {
        let k = self.scale.factor() as usize;
        let (pixels, w) = self.paint();
        pixels[y as usize * k * w + x as usize * k] & 0x00ff_ffff
    }

    /// The absolute index of the chat titled `title`, with the list scrolled to it.
    fn find(&mut self, title: &str) -> u32 {
        let total = self.model.borrow().total;
        for i in 0..total {
            self.model.borrow_mut().list_cursor(i);
            self.pump();
            if self.model.borrow().row_at(i).is_some_and(|r| r.title == title) {
                return i;
            }
        }
        panic!("no chat {title}");
    }

    fn open(&mut self, title: &str) {
        let i = self.find(title);
        self.model.borrow_mut().open_at(i);
        self.pump();
        assert_eq!(self.model.borrow().title, title);
    }

    fn said(&self, line: &str) -> bool {
        self.lines.iter().any(|l| l.contains(line))
    }

    fn row(&self, title: &str) -> Option<ChatRow> {
        self.model
            .borrow()
            .chats
            .iter()
            .find(|r| r.title == title)
            .cloned()
    }
}

// ---------------------------------------------------------------- dates and text

#[test]
fn civil_time_of_the_samples_start() {
    let c = time::civil(sample::START, sample::UTC_OFFSET);
    assert_eq!((c.year, c.month, c.day, c.hour, c.minute), (2026, 11, 14, 14, 0));
    // A Saturday.
    assert_eq!(c.weekday, 5);
    assert_eq!(time::days_from_civil(2026, 11, 14), 20771);
    assert_eq!(time::civil(0, 0).weekday, 3);
    let leap = time::civil(time::days_from_civil(2024, 2, 29) * 86_400, 0);
    assert_eq!((leap.year, leap.month, leap.day), (2024, 2, 29));
    let before = time::civil(-1, 0);
    assert_eq!((before.year, before.month, before.day, before.hour), (1969, 12, 31, 23));
}

#[test]
fn times_as_the_list_and_the_chat_write_them() {
    let (now, off) = (sample::START, sample::UTC_OFFSET);
    assert_eq!(time::clock(now, off), "14:00");
    assert_eq!(time::list_time(now - 3600, now, off), "13:00");
    assert_eq!(time::list_time(now - 86_400, now, off), "Fri");
    assert_eq!(time::list_time(now - 10 * 86_400, now, off), "4 Nov");
    assert_eq!(time::list_time(now - 400 * 86_400, now, off), "10.10.2025");
    assert_eq!(time::day_label(now, now, off), "Today");
    assert_eq!(time::day_label(now - 86_400, now, off), "Yesterday");
    assert_eq!(time::day_label(now - 3 * 86_400, now, off), "11 November");
    assert_eq!(time::day_label(now - 400 * 86_400, now, off), "10 October 2025");
    // Just after midnight there, yesterday's evening is yesterday.
    let midnight = time::days_from_civil(2026, 11, 14) * 86_400 - i64::from(off) + 60;
    assert_eq!(time::day_label(midnight - 120, midnight, off), "Yesterday");
}

#[test]
fn the_samples_markup_gives_spans_in_code_points() {
    let r = sample::rich("Я. @taras, глянь *журнал* і `tgd.log` на <https://example.org/a_b>");
    assert_eq!(r.text, "Я. @taras, глянь журнал і tgd.log на https://example.org/a_b");
    let of = |kind: SpanKind| -> String {
        let s = r.spans.iter().find(|s| s.kind == kind).expect("a span");
        r.text
            .chars()
            .skip(s.start as usize)
            .take((s.end - s.start) as usize)
            .collect()
    };
    assert_eq!(of(SpanKind::Mention), "@taras");
    assert_eq!(of(SpanKind::Bold), "журнал");
    assert_eq!(of(SpanKind::Code), "tgd.log");
    assert_eq!(of(SpanKind::Link), "https://example.org/a_b");
}

#[test]
fn initials_of_latin_and_cyrillic_titles() {
    assert_eq!(draw::initials("Оксана Коваль"), "ОК");
    assert_eq!(draw::initials("Zoraya — розробка"), "ZР");
    assert_eq!(draw::initials("Сусіди, буд. 12"), "СБ");
    assert_eq!(draw::initials("ґанок"), "Ґ");
    assert_eq!(draw::initials("—"), "");
}

fn wrapped(text: &str, max: i32) -> (Vec<String>, Vec<i32>) {
    let mut p = Painter::new(Scale::ONE, Appearance::Light);
    let r = sample::rich(text);
    let segs = draw::styles(&r);
    let lines = draw::wrap(&mut p, &r.text, &segs, 13, max);
    (
        lines.iter().map(|l| String::from(&r.text[l.a..l.b])).collect(),
        lines.iter().map(|l| l.w).collect(),
    )
}

#[test]
fn text_wraps_at_spaces_and_never_passes_the_width() {
    let text = "Там тепер є вікно зі списком чатів. Дуже зручно: ліворуч чати, праворуч розмова.";
    let (lines, widths) = wrapped(text, 200);
    assert!(lines.len() >= 3, "{lines:?}");
    for (l, w) in lines.iter().zip(&widths) {
        assert!(*w <= 200, "{l}: {w}");
        assert!(!l.starts_with(' ') && !l.ends_with(' '), "{l:?}");
    }
    // Nothing but the spaces at the breaks is lost.
    assert_eq!(lines.join(" "), text);
    // A wide box: one line.
    assert_eq!(wrapped(text, 5000).0.len(), 1);
}

#[test]
fn a_word_longer_than_a_line_is_cut_and_newlines_break() {
    let (lines, widths) = wrapped("https://example.org/a/very/long/path/that/does/not/fit/anywhere", 120);
    assert!(lines.len() > 2);
    assert!(widths.iter().all(|w| *w <= 120), "{widths:?}");
    assert_eq!(lines.concat(), "https://example.org/a/very/long/path/that/does/not/fit/anywhere");
    assert_eq!(wrapped("one\ntwo\n\nfour", 500).0, ["one", "two", "", "four"]);
    assert_eq!(wrapped("", 500).0, [""]);
    assert_eq!(wrapped("line\n", 500).0, ["line", ""]);
    // A width of nothing still ends: a character a line.
    assert_eq!(wrapped("abc", 1).0, ["a", "b", "c"]);
}

#[test]
fn an_emoji_the_fonts_lack_is_one_small_mark_not_a_box() {
    use zoraya_text::Source;
    let mut p = Painter::new(Scale::ONE, Appearance::Light);
    let boxes = |p: &Painter, s: &str| s.chars().filter(|&c| p.ui.fonts.lookup(c).0 == Source::Box).count();
    // What the fonts have is left as it is, borrowed.
    for s in ["plain", "Книжковий клуб", "ґанок — «так»"] {
        assert!(matches!(draw::shown(&p.ui, s), std::borrow::Cow::Borrowed(_)), "{s}");
    }
    let mark = draw::shown(&p.ui, "\u{1F600}").into_owned();
    assert_eq!(mark.chars().count(), 1);
    assert_eq!(boxes(&p, &mark), 0, "the mark is a glyph of the fonts");
    // A flag (two regional indicators), a family joined with U+200D, a thumb with a skin
    // tone, a selector, a keycap and a subdivision's flag with its tags: one mark each.
    let cases = [
        ("\u{1F1FA}\u{1F1E6} Україна", format!("{mark} Україна")),
        ("\u{1F1FA}\u{1F1E6}\u{1F1F5}\u{1F1F1}", format!("{mark}{mark}")),
        ("a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}b", format!("a{mark}b")),
        ("\u{1F44D}\u{1F3FD}\u{1F44D}", format!("{mark}{mark}")),
        ("ok\u{1F642}\u{FE0F}!", format!("ok{mark}!")),
        ("1\u{FE0F}\u{20E3}", String::from("1")),
        ("\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}", mark.clone()),
    ];
    for (s, want) in cases {
        let got = draw::shown(&p.ui, s);
        assert_eq!(got, want, "{s:?}");
        assert_eq!(boxes(&p, &got), 0, "{s:?}");
        // Measured as it is drawn, and narrower than the boxes were.
        assert_eq!(draw::measure(&mut p.ui, s, 13), p.ui.measure(&got, 13));
    }
    // A script with no font keeps the box: that one is honest.
    assert_eq!(draw::shown(&p.ui, "\u{4E2D}"), "\u{4E2D}");
    // A title cut to a width still ends in the ellipsis, with no box before it.
    let t = draw::fit(&mut p.medium, "\u{1F1FA}\u{1F1E6} Дуже довга назва чату з прапором", 13, 120);
    assert!(t.ends_with('…') && boxes(&p, &draw::shown(&p.medium, &t)) == 0, "{t}");
}

#[test]
fn a_shape_from_its_corners_is_the_shape_from_one_mask() {
    use zoraya_paint::{self as paint, Canvas};
    use zoraya_tokens::shape;
    let color = Appearance::Light.palette().accent;
    for (w, h, radius, t) in [(200, 60, 12, 1), (97, 41, 6, 2), (300, 300, 24, 4), (64, 39, 12, 2), (20, 20, 12, 1)] {
        let r = draw::rect(7, 5, w, h);
        let size = ((w + 20) * (h + 20)) as usize;
        let (mut a, mut b) = (vec![0xffffffu32; size], vec![0xffffffu32; size]);
        let stride = (w + 20) as usize;
        draw::round(&mut Canvas::new(&mut a, stride), r, radius, color);
        let m = paint::rounded_rect(w as usize, h as usize, radius, shape::CORNER);
        Canvas::new(&mut b, stride).mask(&m, r.x, r.y, color, 255);
        assert!(a == b, "round {w} x {h}, radius {radius}");
        let (mut a, mut b) = (vec![0xffffffu32; size], vec![0xffffffu32; size]);
        draw::outline(&mut Canvas::new(&mut a, stride), r, radius, t, color);
        let m = paint::rounded_outline(w as usize, h as usize, radius, t, shape::CORNER);
        Canvas::new(&mut b, stride).mask(&m, r.x, r.y, color, 255);
        assert!(a == b, "outline {w} x {h}, radius {radius}, {t} thick");
    }
}

#[test]
fn styles_follow_the_spans() {
    let r = sample::rich("a *b* `c` d");
    let s = draw::styles(&r);
    let kinds: Vec<Style> = s.iter().map(|x| x.2).collect();
    assert_eq!(
        kinds,
        [Style::Plain, Style::Bold, Style::Plain, Style::Code, Style::Plain]
    );
    assert_eq!(s.last().map(|x| x.1), Some(r.text.len()));
}

#[test]
fn a_line_is_cut_with_an_ellipsis() {
    let mut p = Painter::new(Scale::ONE, Appearance::Light);
    let s = draw::fit(&mut p.ui, "Велоклуб «Дніпро» та друзі", 13, 90);
    assert!(s.ends_with('…') && s.starts_with("Велоклуб"), "{s}");
    assert!(p.ui.measure(&s, 13) <= 90);
    assert_eq!(draw::fit(&mut p.ui, "short", 13, 90), "short");
}

// ---------------------------------------------------------------- the model and the sample

#[test]
fn the_login_asks_phone_code_and_password_and_keeps_no_value() {
    let mut ui = Ui::new(Scale::ONE, Appearance::Light, WIDTH, HEIGHT, false);
    assert_eq!(ui.model.borrow().screen, Screen::Login);
    assert_eq!(ui.model.borrow().auth, AuthState::WaitPhone);
    assert_eq!(ui.focused(), Some(Role::LoginField));
    // Letters are no number.
    ui.type_text("abc");
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().auth_error, "PHONE_NUMBER_INVALID");
    assert_eq!(ui.model.borrow().login.text, "");
    ui.type_text("+000 0000");
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().auth, AuthState::WaitCode);
    ui.type_text("11111");
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().auth_error, "PHONE_CODE_INVALID");
    ui.type_text(sample::CODE);
    // The Next button does what Enter does.
    ui.focus(Role::LoginNext);
    ui.key(keys::SPACE);
    assert_eq!(ui.model.borrow().auth, AuthState::WaitPassword);
    assert_eq!(ui.focused(), Some(Role::LoginField));
    ui.type_text(sample::PASSWORD);
    assert_eq!(ui.model.borrow().login.text, sample::PASSWORD);
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().screen, Screen::Main);
    assert_eq!(ui.model.borrow().login.text, "");
    assert!(ui.said("tg: ready"));
    // Nothing typed at the login is on the console.
    for l in &ui.lines {
        assert!(!l.contains(sample::PASSWORD) && !l.contains(sample::CODE) && !l.contains("0000"), "{l}");
    }
    assert!(ui.model.borrow().total > 20);
}

#[test]
fn a_password_is_drawn_as_dots() {
    let mut ui = Ui::new(Scale::ONE, Appearance::Light, WIDTH, HEIGHT, false);
    ui.type_text("0000");
    ui.key(keys::ENTER);
    ui.type_text(sample::CODE);
    ui.key(keys::ENTER);
    let (empty, _) = ui.paint();
    ui.type_text("zzzz");
    let (four, _) = ui.paint();
    // Other letters of the same count: the same pixels.
    for _ in 0..4 {
        ui.key(keys::BACKSPACE);
    }
    ui.type_text("WMWM");
    let (other, _) = ui.paint();
    assert!(four != empty);
    assert!(four == other, "a password's letters change what is drawn");
}

#[test]
fn the_list_holds_the_window_it_shows() {
    let mut ui = Ui::main();
    let rows = ((HEIGHT as i32 - 48) / ROW) as u32;
    {
        let m = ui.model.borrow();
        assert_eq!(m.list_rows, rows);
        assert_eq!(m.total, 28);
        assert_eq!(m.first, 0);
        assert_eq!(m.chats.len() as u32, rows + 1);
        // The pinned ones first, the newest of them first; then the newest.
        let titles: Vec<&str> = m.chats.iter().take(3).map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Zoraya — розробка", "Saved Messages", "Марина Ткаченко"]);
        assert!(m.chats.windows(2).skip(2).all(|w| w[0].date >= w[1].date));
    }
    assert!(ui.said("tg: chats 28 from 0: \"Zoraya — розробка\" \"Saved Messages\""));
    // The wheel: three rows a notch, the window follows.
    assert_eq!(ui.wheel(100, 200, 1), Response::Changed(crate::widgets::id::LIST));
    assert_eq!((ui.model.borrow().top, ui.model.borrow().first), (3, 3));
    ui.wheel(100, 200, 20);
    let m = ui.model.borrow();
    assert_eq!(m.top, 28 - rows);
    assert_eq!(m.chats.len() as u32, rows);
    drop(m);
    // At its end the list takes no more turns that way, and goes back.
    assert_eq!(ui.wheel(100, 200, 1), Response::None);
    ui.wheel(100, 200, -20);
    assert_eq!(ui.model.borrow().top, 0);
    assert_eq!(ui.wheel(100, 200, -1), Response::None);
}

#[test]
fn the_keys_move_the_lists_row_and_enter_opens_it() {
    let mut ui = Ui::main();
    ui.focus(Role::List);
    ui.key(keys::DOWN);
    assert_eq!(ui.model.borrow().cursor, Some(0));
    for _ in 0..9 {
        ui.key(keys::DOWN);
    }
    let rows = ui.model.borrow().list_rows;
    assert_eq!(ui.model.borrow().cursor, Some(9));
    assert_eq!(ui.model.borrow().top, 10 - rows);
    ui.key(keys::END);
    assert_eq!(ui.model.borrow().cursor, Some(27));
    ui.key(key::PAGE_UP);
    assert_eq!(ui.model.borrow().cursor, Some(27 - rows));
    ui.key(keys::HOME);
    assert_eq!((ui.model.borrow().cursor, ui.model.borrow().top), (Some(0), 0));
    ui.key(keys::DOWN);
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().title, "Saved Messages");
    assert_eq!(ui.focused(), Some(Role::Composer));
    assert!(ui.said("tg: opened \"Saved Messages\": 4 messages"));
}

#[test]
fn opening_a_chat_shows_its_messages_and_reads_it() {
    let mut ui = Ui::main();
    assert_eq!(ui.row("Оксана Коваль").map(|r| r.unread), None.or(Some(2)));
    ui.open("Оксана Коваль");
    let m = ui.model.borrow();
    assert_eq!(m.messages.len(), 13);
    assert_eq!(m.subtitle, "last seen recently");
    assert!(!m.more);
    assert_eq!(m.messages[5].row.kind, MessageKind::Photo);
    assert_eq!(m.messages[7].row.kind, MessageKind::Voice);
    assert!(m.messages[3].row.edited);
    assert!(m.messages[4].row.reply.is_some());
    assert_eq!(m.messages[12].row.text.text, "Не запізнюйся :)");
    drop(m);
    assert_eq!(ui.row("Оксана Коваль").map(|r| r.unread), Some(0));
    assert!(ui.said("tg: opened \"Оксана Коваль\": 13 messages, last: Не запізнюйся :)"));
    assert!(ui.said("tg: chat \"Оксана Коваль\" unread 0"));
}

#[test]
fn a_sent_message_is_in_the_view_and_the_lists_last_line() {
    let mut ui = Ui::main();
    ui.open("Оксана Коваль");
    assert_eq!(ui.focused(), Some(Role::Composer));
    ui.type_text("Привіт із Zoraya");
    ui.key(keys::ENTER);
    {
        let m = ui.model.borrow();
        let last = &m.messages.last().expect("a message").row;
        assert_eq!(last.text.text, "Привіт із Zoraya");
        assert!(last.out);
        assert_eq!(last.state, SendState::Pending);
        assert!(last.id < 0);
        assert_eq!(m.composer.text, "");
    }
    let row = ui.row("Оксана Коваль").expect("in the window");
    assert_eq!(row.last, "Привіт із Zoraya");
    // The newest chat that is not pinned: the third row.
    assert_eq!(ui.model.borrow().chats[2].title, "Оксана Коваль");
    assert!(ui.said("tg: message \"Оксана Коваль\" out: Привіт із Zoraya"));
    assert!(ui.said("tg: chat \"Оксана Коваль\" unread 0: Привіт із Zoraya"));
    ui.advance(sample::DELIVER_MS);
    {
        let m = ui.model.borrow();
        let last = &m.messages.last().expect("a message").row;
        assert_eq!(last.state, SendState::Sent);
        assert!(last.id > 0);
    }
    assert!(ui.said("tg: delivered: Привіт із Zoraya"));
    ui.advance(sample::READ_MS);
    assert_eq!(
        ui.model.borrow().messages.last().map(|m| m.row.state),
        Some(SendState::Read)
    );
    // Nothing is sent of spaces alone; Shift+Enter starts a line.
    let n = ui.model.borrow().messages.len();
    ui.type_text("   ");
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().messages.len(), n);
    ui.model.borrow_mut().composer.clear();
    ui.type_text("one");
    ui.chord(keys::ENTER, mods::SHIFT);
    ui.type_text("two");
    assert_eq!(ui.model.borrow().composer.text, "one\ntwo");
    ui.key(keys::ENTER);
    assert_eq!(
        ui.model.borrow().messages.last().map(|m| m.row.text.text.clone()),
        Some(String::from("one\ntwo"))
    );
    assert_eq!(ui.row("Оксана Коваль").map(|r| r.last), Some(String::from("one two")));
}

#[test]
fn the_timers_message_raises_the_unread_count_or_shows_in_the_open_chat() {
    let mut ui = Ui::main();
    assert_eq!(ui.row("Оксана Коваль").map(|r| r.unread), Some(2));
    ui.advance(sample::FIRST_INCOMING_MS);
    let row = ui.row("Оксана Коваль").expect("in the window");
    assert_eq!(row.unread, 3);
    assert_eq!(row.last, "Нагадую: зустріч о 15:30, кімната 4.");
    assert!(ui.said("tg: chat \"Оксана Коваль\" unread 3: Нагадую: зустріч о 15:30, кімната 4."));
    // It is the newest now: first after the pinned two.
    assert_eq!(ui.model.borrow().chats[2].title, "Оксана Коваль");
    // The next one goes to a chat that is open: no count, a message in the view.
    ui.open("Иван Соколов");
    let n = ui.model.borrow().messages.len();
    ui.advance(sample::INCOMING_EVERY_MS);
    let m = ui.model.borrow();
    assert_eq!(m.messages.len(), n + 1);
    assert_eq!(
        m.messages.last().map(|x| x.row.text.text.as_str()),
        Some("Отчёт отправил, посмотри, когда будет время.")
    );
    drop(m);
    assert_eq!(ui.row("Иван Соколов").map(|r| r.unread), Some(0));
    assert!(ui.said("tg: message \"Иван Соколов\": Отчёт отправил"));
}

#[test]
fn search_filters_the_list_without_case() {
    let mut ui = Ui::main();
    ui.focus(Role::Search);
    ui.type_text("КОВ");
    {
        let m = ui.model.borrow();
        let titles: Vec<&str> = m.chats.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, ["Оксана Коваль", "Василь Ковальчук"]);
        assert_eq!(m.total, 2);
    }
    assert!(ui.said("tg: search \"КОВ\""));
    assert!(ui.said("tg: chats 2 from 0: \"Оксана Коваль\" \"Василь Ковальчук\""));
    // Enter goes to the first chat found; Enter again opens it.
    ui.key(keys::ENTER);
    assert_eq!(ui.focused(), Some(Role::List));
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().title, "Оксана Коваль");
    // Nothing matches; Escape clears the search.
    ui.focus(Role::Search);
    ui.type_text("zzz");
    assert_eq!(ui.model.borrow().total, 0);
    ui.key(keys::ESC);
    assert_eq!(ui.model.borrow().query.text, "");
    assert_eq!(ui.model.borrow().total, 28);
    // Typing in the list searches.
    ui.focus(Role::List);
    ui.type_text("rust");
    assert_eq!(ui.focused(), Some(Role::Search));
    assert_eq!(ui.model.borrow().total, 1);
    assert_eq!(ui.model.borrow().chats[0].title, "Rust Kyiv");
}

#[test]
fn older_messages_come_a_page_at_a_time() {
    let mut ui = Ui::main();
    ui.open("Zoraya — розробка");
    assert_eq!(ui.model.borrow().messages.len(), 40);
    assert!(ui.model.borrow().more);
    let newest = ui.model.borrow().messages[39].row.id;
    // The view asks when it is near the top: painting it does.
    ui.focus(Role::Messages);
    ui.key(keys::HOME);
    ui.paint();
    ui.pump();
    let mut pages = 1;
    while ui.model.borrow().more {
        ui.key(keys::HOME);
        ui.paint();
        ui.pump();
        pages += 1;
        assert!(pages < 10);
    }
    let m = ui.model.borrow();
    assert_eq!(m.messages.len(), 153);
    assert_eq!(m.messages[152].row.id, newest);
    assert!(m.messages.windows(2).all(|w| w[0].row.id < w[1].row.id));
    assert_eq!(m.messages[20].row.kind, MessageKind::Service);
    drop(m);
    assert!(ui.said("tg: older 40, 80 loaded, more"));
    assert!(ui.said("tg: older 33, 153 loaded"));
}

#[test]
fn the_wheel_and_the_keys_scroll_the_messages() {
    let mut ui = Ui::main();
    ui.open("Zoraya — розробка");
    ui.paint();
    assert_eq!(ui.model.borrow().scroll, 0);
    // Away from the person: older messages.
    ui.wheel(600, 250, -1);
    assert_eq!(ui.model.borrow().scroll, 72);
    ui.wheel(600, 250, 1);
    assert_eq!(ui.model.borrow().scroll, 0);
    assert_eq!(ui.wheel(600, 250, 1), Response::None);
    ui.focus(Role::Messages);
    ui.key(key::PAGE_UP);
    let page = ui.model.borrow().scroll;
    assert!(page > 200);
    // A message that comes while scrolled up leaves the view where it is.
    ui.paint();
    let before = ui.model.borrow().messages.len();
    ui.advance(sample::FIRST_INCOMING_MS + 4 * sample::INCOMING_EVERY_MS);
    assert_eq!(ui.model.borrow().messages.len(), before + 1);
    ui.paint();
    assert!(ui.model.borrow().scroll > page, "the view moved with the new message");
    ui.key(keys::END);
    assert_eq!(ui.model.borrow().scroll, 0);
    // Up and Down move the keyboard's message; Shift extends; the text is copied.
    ui.key(keys::UP);
    ui.chord(keys::UP, mods::SHIFT);
    let m = ui.model.borrow();
    let n = m.messages.len();
    assert_eq!(m.selection(), Some((n - 2, n - 1)));
    let text = m.selected_text();
    assert!(text.contains("You: Журнал чистий, 78 кроків пройшли."), "{text}");
    assert!(text.contains("Ірина Бойко: Нова збірка готова"), "{text}");
}

#[test]
fn a_reply_names_its_message() {
    let mut ui = Ui::main();
    ui.open("Оксана Коваль");
    ui.focus(Role::Messages);
    ui.key(keys::UP);
    ui.key(keys::UP);
    ui.key(keys::ENTER);
    assert_eq!(ui.focused(), Some(Role::Composer));
    assert_eq!(
        ui.model.borrow().reply_row().map(|r| r.text.text.clone()),
        Some(String::from("Добре. І ще: зустріч перенесли на 15:30."))
    );
    ui.type_text("Буду вчасно");
    ui.key(keys::ENTER);
    let m = ui.model.borrow();
    let q = m.messages.last().and_then(|x| x.row.reply.clone()).expect("a quote");
    assert_eq!(q.sender, "Оксана Коваль");
    assert_eq!(q.text, "Добре. І ще: зустріч перенесли на 15:30.");
    assert_eq!(m.reply_to, None);
}

#[test]
fn the_summary_comes_in_parts_and_closes() {
    let mut ui = Ui::main();
    ui.open("Оксана Коваль");
    ui.focus(Role::SummaryButton);
    ui.key(keys::ENTER);
    assert!(ui.model.borrow().summary.is_some());
    assert_eq!(ui.focused(), Some(Role::Card(3)));
    ui.advance(sample::SUMMARY_PART_MS);
    {
        let m = ui.model.borrow();
        let s = m.summary.as_ref().expect("a summary");
        assert_eq!(s.state, SummaryState::Running);
        assert_eq!(s.text, "Оксана Коваль: 2 unread messages. ");
    }
    ui.advance(3 * sample::SUMMARY_PART_MS);
    assert_eq!(
        ui.model.borrow().summary.as_ref().map(|s| s.state),
        Some(SummaryState::Done)
    );
    assert!(ui.said("tg: summary unread done:"));
    // Another scope starts again.
    ui.focus(Role::Card(2));
    ui.key(keys::ENTER);
    {
        let m = ui.model.borrow();
        let s = m.summary.as_ref().expect("a summary");
        assert_eq!((s.scope, s.state, s.text.as_str()), (Scope::Week, SummaryState::Running, ""));
    }
    assert_eq!(ui.focused(), Some(Role::Card(2)));
    ui.advance(4 * sample::SUMMARY_PART_MS);
    assert!(ui.said("tg: summary week done:"));
    // Escape on the card's button closes it.
    ui.key(keys::ESC);
    assert!(ui.model.borrow().summary.is_none());
    assert_eq!(ui.focused(), Some(Role::Composer));
}

#[test]
fn settings_are_the_backends_and_log_out_forgets_the_account() {
    let mut ui = Ui::main();
    ui.open("Оксана Коваль");
    ui.focus(Role::SettingsButton);
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().screen, Screen::Settings);
    assert_eq!(ui.focused(), Some(Role::Setting(0)));
    // AI off: a summary is refused, in words.
    ui.key(keys::SPACE);
    assert!(!ui.model.borrow().settings[0].on);
    assert!(ui.said("tg: setting ai.enabled off"));
    ui.focus(Role::Setting(1));
    ui.key(keys::SPACE);
    ui.key(keys::ESC);
    assert_eq!(ui.model.borrow().screen, Screen::Main);
    assert_eq!(ui.row("Оксана Коваль").map(|r| r.last), Some(String::from("Message")));
    ui.focus(Role::SummaryButton);
    ui.key(keys::ENTER);
    ui.advance(1);
    {
        let m = ui.model.borrow();
        let s = m.summary.as_ref().expect("a summary");
        assert_eq!(s.state, SummaryState::Failed);
        assert_eq!(s.text, "AI summaries are turned off in Settings.");
    }
    ui.key(keys::ESC);
    ui.focus(Role::SettingsButton);
    ui.key(keys::ENTER);
    ui.focus(Role::LogOut);
    ui.key(keys::ENTER);
    let m = ui.model.borrow();
    assert_eq!((m.screen, m.auth), (Screen::Login, AuthState::WaitPhone));
    assert!(m.chats.is_empty() && m.messages.is_empty() && m.open.is_none());
}

/// A small generator, the same on every host.
struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) % n
    }
}

#[test]
fn the_lists_diffs_keep_the_window_equal_to_the_backends_list() {
    let mut rng = Rng(7);
    let mut model = Model::new();
    let mut backend = SampleBackend::logged_in();
    let mut now = 0u64;
    let mut splices = 0;
    for step in 0..600 {
        match rng.next(8) {
            0 => {
                let rows = 1 + rng.next(12) as u32;
                model.set_list_rows(rows);
            }
            1 => {
                model.list_scroll(rng.next(9) as i32 - 4);
            }
            2 => {
                let i = model.first + rng.next(8) as u32;
                model.open_at(i);
            }
            3 => {
                if model.open.is_some() {
                    model.composer.insert_str("повідомлення");
                    model.send();
                }
            }
            4 => {
                model.query.clear();
                if rng.next(3) == 0 {
                    model.query.insert_str(["о", "an", "к"][rng.next(3) as usize]);
                }
                model.query_changed();
            }
            5 => {
                let i = rng.next(3) as usize;
                model.toggle_setting(i);
            }
            _ => now += rng.next(5000),
        }
        loop {
            model.set_clock(now);
            let requests = model.take_requests();
            let asked = !requests.is_empty();
            for r in requests {
                backend.send(r);
            }
            let mut told = false;
            while let Some(e) = backend.poll(now) {
                if matches!(e, Ev::ChatsSplice { .. }) {
                    splices += 1;
                }
                model.apply(e);
                told = true;
            }
            if !asked && !told {
                break;
            }
        }
        if model.screen != Screen::Main {
            continue;
        }
        let all = backend.list(&model.query.text);
        let want: Vec<ChatRow> = all
            .iter()
            .skip(model.top as usize)
            .take(model.list_rows as usize + 1)
            .cloned()
            .collect();
        assert_eq!(model.total as usize, all.len(), "step {step}");
        assert_eq!(model.first, model.top, "step {step}");
        assert_eq!(model.chats, want, "step {step}");
    }
    assert!(splices > 50, "{splices} splices: the test met too few diffs");
}

#[test]
fn a_request_before_the_login_is_answered_by_nothing() {
    let mut b = SampleBackend::new();
    assert!(matches!(b.poll(0), Some(Ev::Hello { synthetic: true, .. })));
    assert!(matches!(
        b.poll(0),
        Some(Ev::Auth {
            state: AuthState::WaitPhone,
            ..
        })
    ));
    b.send(Request::ChatsWindow {
        first: 0,
        count: 10,
        query: String::new(),
    });
    b.send(Request::ChatOpen { chat: 1000, count: 10 });
    assert_eq!(b.poll(100_000), None);
    assert_eq!(b.next_wake(), None);
}

// ---------------------------------------------------------------- the widgets

#[test]
fn a_press_on_a_row_opens_its_chat_and_marks_the_row() {
    let mut ui = Ui::main();
    let palette = Appearance::Light.palette();
    assert_eq!(ui.pixel(LIST_W - 4, 48 + 2 * ROW + 4), palette.bg_view.pixel() & 0xff_ffff);
    // The third row: the newest chat that is not pinned.
    ui.click(150, 48 + 2 * ROW + 30);
    assert_eq!(ui.model.borrow().title, "Марина Ткаченко");
    assert_eq!(ui.focused(), Some(Role::Composer));
    assert_eq!(
        ui.pixel(LIST_W - 4, 48 + 2 * ROW + 4),
        palette.bg_selection.pixel() & 0xff_ffff
    );
    // The chat's side: the header's ground, and something drawn in the messages.
    assert_eq!(ui.pixel(LIST_W + 300, 4), palette.bg_window.pixel() & 0xff_ffff);
    let (pixels, w) = ui.paint();
    let ground = palette.bg_window.pixel() & 0xff_ffff;
    let ink = (60..HEIGHT as usize - 60)
        .flat_map(|y| (LIST_W as usize + 10..WIDTH as usize - 10).map(move |x| (x, y)))
        .filter(|(x, y)| pixels[y * w + x] & 0xff_ffff != ground)
        .count();
    assert!(ink > 5000, "{ink} pixels drawn in the messages");
}

#[test]
fn a_press_on_a_message_selects_it() {
    let mut ui = Ui::main();
    ui.open("Оксана Коваль");
    ui.paint();
    // The last message is incoming: its bubble is at the left, above the composer.
    ui.click(LIST_W + 40, HEIGHT as i32 - 70);
    let m = ui.model.borrow();
    assert_eq!(m.msg_cursor, Some(12));
    assert_eq!(m.selected_text(), "Не запізнюйся :)");
    drop(m);
    // A press on no message clears it.
    ui.click(WIDTH as i32 - 30, HEIGHT as i32 - 70);
    assert_eq!(ui.model.borrow().msg_cursor, None);
}

#[test]
fn the_composer_grows_with_its_lines_and_moves_between_them() {
    let mut ui = Ui::main();
    ui.open("Saved Messages");
    let h = |ui: &mut Ui| {
        let m = ui.model.borrow();
        draw::num(chat::composer_height(&mut ui.p, &m, lg(400)))
    };
    let one = h(&mut ui);
    ui.type_text("перший");
    ui.chord(keys::ENTER, mods::SHIFT);
    ui.type_text("другий рядок");
    assert_eq!(h(&mut ui), one + chat::LINE);
    ui.paint();
    // Up keeps the column (in characters), within the line above.
    ui.key(keys::UP);
    assert_eq!(ui.model.borrow().composer.cursor, "перший".len());
    ui.key(keys::HOME);
    ui.key(keys::DOWN);
    assert_eq!(ui.model.borrow().composer.cursor, "перший\n".len());
    ui.key(keys::END);
    ui.key(keys::BACKSPACE);
    assert_eq!(ui.model.borrow().composer.text, "перший\nдругий рядо");
    // Many lines: at most six are shown.
    for _ in 0..12 {
        ui.chord(keys::ENTER, mods::SHIFT);
    }
    assert_eq!(h(&mut ui), one + 5 * chat::LINE);
    ui.paint();
}

#[test]
fn every_screen_paints_at_both_scales_in_both_themes() {
    for scale in [Scale::ONE, Scale::TWO] {
        for look in [Appearance::Light, Appearance::Dark] {
            let palette = look.palette();
            let mut ui = Ui::new(scale, look, WIDTH, HEIGHT, false);
            ui.paint();
            ui.type_text("0000");
            ui.key(keys::ENTER);
            ui.type_text("1");
            ui.key(keys::ENTER);
            // The refusal, in the error colour, somewhere under the field.
            let (pixels, _) = ui.paint();
            assert!(pixels.iter().any(|p| p & 0xff_ffff == palette.error.pixel() & 0xff_ffff));
            ui.type_text(sample::CODE);
            ui.key(keys::ENTER);
            ui.type_text(sample::PASSWORD);
            ui.key(keys::ENTER);
            let (pixels, _) = ui.paint();
            // Unread counts in the accent.
            let accent = palette.accent.pixel() & 0xff_ffff;
            assert!(pixels.iter().filter(|p| **p & 0xff_ffff == accent).count() > 200);
            for title in ["Оксана Коваль", "Иван Соколов", "Zoraya — розробка", "Zoraya News", "Saved Messages"] {
                ui.model.borrow_mut().query.clear();
                ui.open(title);
                ui.paint();
            }
            ui.focus(Role::SummaryButton);
            ui.key(keys::ENTER);
            ui.advance(2000);
            ui.paint();
            ui.key(keys::ESC);
            ui.focus(Role::SettingsButton);
            ui.key(keys::ENTER);
            ui.paint();
            assert_eq!(ui.model.borrow().stats.frames, 10);
        }
    }
}

#[test]
fn a_narrow_window_shows_one_pane_and_escape_goes_back() {
    let mut ui = Ui::main();
    assert!(!ui.model.borrow().narrow);
    assert!(ui.said("tg: size 840 520"));
    ui.win.size = (lg(480), lg(520));
    ui.win.layout(&mut ui.p);
    ui.pump();
    assert!(ui.model.borrow().narrow);
    assert!(ui.said("tg: size 480 520"));
    // Nothing open: the list, the window's width.
    assert!(ui.roles.iter().any(|(r, _)| *r == Role::List));
    assert!(!ui.roles.iter().any(|(r, _)| *r == Role::Messages));
    ui.click(400, 48 + 30);
    assert_eq!(ui.model.borrow().title, "Zoraya — розробка");
    // The chat alone.
    assert!(!ui.roles.iter().any(|(r, _)| *r == Role::List));
    assert!(ui.roles.iter().any(|(r, _)| *r == Role::Composer));
    ui.paint();
    ui.key(keys::ESC);
    assert!(ui.model.borrow().open.is_none());
    assert!(ui.roles.iter().any(|(r, _)| *r == Role::List));
    ui.paint();
    // Wide again: both.
    ui.win.size = (lg(900), lg(600));
    ui.win.layout(&mut ui.p);
    ui.pump();
    assert!(!ui.model.borrow().narrow);
    ui.click(150, 48 + 30);
    assert!(ui.roles.iter().any(|(r, _)| *r == Role::List));
    assert!(ui.roles.iter().any(|(r, _)| *r == Role::Messages));
    assert_eq!(ui.model.borrow().list_rows, ((600 - 48) / ROW) as u32);
}

#[test]
fn tab_reaches_every_control() {
    let mut ui = Ui::main();
    ui.open("Оксана Коваль");
    ui.focus(Role::SummaryButton);
    ui.key(keys::ENTER);
    ui.focus(Role::Search);
    let mut seen = vec![Role::Search];
    for _ in 0..16 {
        ui.key(keys::TAB);
        let r = ui.focused().expect("a focus");
        if r == Role::Search {
            break;
        }
        seen.push(r);
    }
    assert_eq!(
        seen,
        [
            Role::Search,
            Role::SettingsButton,
            Role::List,
            Role::Messages,
            Role::SummaryButton,
            Role::Composer,
            Role::Send,
            Role::Card(0),
            Role::Card(1),
            Role::Card(2),
            Role::Card(3),
        ]
    );
}

/// A chat with every page of its history loaded.
fn whole(title: &str) -> Ui {
    let mut ui = Ui::main();
    ui.open(title);
    while ui.model.borrow().more {
        ui.model.borrow_mut().want_older();
        ui.pump();
    }
    ui
}

/// The archive's thousand messages, all loaded.
fn archive() -> Ui {
    let ui = whole("Project archive");
    assert_eq!(ui.model.borrow().messages.len(), sample::ARCHIVE);
    ui
}

#[test]
fn only_the_messages_near_the_view_are_laid_out() {
    let mut ui = archive();
    ui.paint();
    let s = ui.model.borrow().stats;
    // A view and half a view each way, of a thousand.
    assert!(s.laid > 5 && s.laid < 40, "{} laid out by the first frame", s.laid);
    assert_eq!(s.held, s.laid);
    // The next frame lays out nothing; a page up, about a page.
    ui.paint();
    assert_eq!(ui.model.borrow().stats.laid, 0);
    ui.focus(Role::Messages);
    ui.key(key::PAGE_UP);
    ui.paint();
    let s = ui.model.borrow().stats;
    assert!(s.laid > 0 && s.laid < 25, "{} laid out by a page", s.laid);
    // The far end: again a view's worth, whatever lies between.
    ui.key(keys::HOME);
    ui.paint();
    let s = ui.model.borrow().stats;
    assert!(s.laid < 40 && s.held < 110, "{} laid out, {} held", s.laid, s.held);
    // A new tree (the card) starts with no layouts and makes a view's worth.
    ui.focus(Role::SummaryButton);
    ui.key(keys::ENTER);
    ui.paint();
    let s = ui.model.borrow().stats;
    assert!(s.laid < 40 && s.held == s.laid, "{} laid out, {} held", s.laid, s.held);
}

/// The message under a point of the messages (by a press there).
fn message_at(ui: &mut Ui, x: i32, y: i32) -> Option<crate::proto::Id> {
    ui.click(x, y);
    let m = ui.model.borrow();
    m.msg_cursor.map(|c| m.messages[c].row.id)
}

#[test]
fn the_view_stays_on_its_messages_when_layouts_replace_estimates() {
    // Short messages (the estimates are near), and long ones that wrap (they are not).
    stays(archive(), 30, 5000);
    stays(whole("Zoraya — розробка"), 6, 1500);
}

fn stays(mut ui: Ui, pages: usize, least: i32) {
    ui.focus(Role::Messages);
    // Far up, by pages: everything between was laid out on the way.
    for _ in 0..pages {
        ui.key(key::PAGE_UP);
        ui.paint();
    }
    let scroll = ui.model.borrow().scroll;
    assert!(scroll > least, "{scroll}");
    // The messages at two places of the view (incoming ones are at the left).
    let find = |ui: &mut Ui, from: i32| {
        (from..from + 120)
            .step_by(6)
            .find_map(|y| message_at(ui, LIST_W + 40, y).map(|id| (y, id)))
            .expect("a message there")
    };
    let top = find(&mut ui, 70);
    let low = find(&mut ui, 300);
    assert!(top.1 != low.1);
    // A new tree forgets every layout: the messages under the view are estimates
    // again, and the view must not move.
    ui.model.borrow_mut().msg_cursor = None;
    ui.focus(Role::SummaryButton);
    ui.key(keys::ENTER);
    ui.paint();
    ui.key(keys::ESC);
    ui.paint();
    assert_eq!(message_at(&mut ui, LIST_W + 40, top.0), Some(top.1));
    assert_eq!(message_at(&mut ui, LIST_W + 40, low.0), Some(low.1));
    // Down a page and up a page: the same messages at the same places.
    ui.model.borrow_mut().msg_cursor = None;
    ui.focus(Role::Messages);
    for code in [key::PAGE_DOWN, key::PAGE_DOWN, key::PAGE_UP, key::PAGE_UP] {
        ui.key(code);
        ui.paint();
    }
    assert_eq!(message_at(&mut ui, LIST_W + 40, top.0), Some(top.1));
    assert_eq!(message_at(&mut ui, LIST_W + 40, low.0), Some(low.1));
    // To the newest and the view is at its end again.
    ui.key(keys::END);
    ui.paint();
    assert_eq!(ui.model.borrow().scroll, 0);
}

/// Times frames of the archive on the host (`cargo test --release -- --ignored timing
/// --nocapture`): a look at where a frame's time goes.
#[test]
#[ignore]
fn timing() {
    let mut ui = archive();
    ui.focus(Role::Messages);
    let frame = |ui: &mut Ui, what: &str, n: u32| {
        let t = std::time::Instant::now();
        for _ in 0..n {
            ui.paint();
        }
        println!("{what}: {:?} a frame", t.elapsed() / n);
    };
    frame(&mut ui, "first", 1);
    frame(&mut ui, "at the newest", 200);
    for _ in 0..20 {
        ui.key(key::PAGE_UP);
        ui.paint();
    }
    frame(&mut ui, "20 pages up", 200);
    let t = std::time::Instant::now();
    for i in 0..200 {
        ui.wheel(600, 250, if i % 2 == 0 { -1 } else { 1 });
        ui.paint();
    }
    println!("wheel and frame: {:?}", t.elapsed() / 200);
    ui.key(keys::HOME);
    frame(&mut ui, "at the oldest", 200);
    // The chat with long messages: the wheel up and down over one stretch, and how many
    // layouts each pass makes.
    let mut ui = whole("Zoraya — розробка");
    ui.paint();
    for pass in 0..4 {
        let (t, mut laid) = (std::time::Instant::now(), 0);
        for _ in 0..30 {
            ui.wheel(600, 250, if pass % 2 == 0 { -1 } else { 1 });
            ui.paint();
            laid += ui.model.borrow().stats.laid;
        }
        println!("pass {pass}: {:?} a frame, {laid} layouts", t.elapsed() / 30);
    }
}

/// Writes the screens as PPM files when `TG_SHOTS` names a directory: a look at the
/// window without booting Zoraya.
#[test]
fn shots() {
    let Ok(dir) = std::env::var("TG_SHOTS") else {
        return;
    };
    let save = |ui: &mut Ui, name: &str| {
        let (pixels, w) = ui.paint();
        let mut out = format!("P6\n{} {}\n255\n", w, pixels.len() / w).into_bytes();
        for p in pixels {
            out.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
        }
        std::fs::write(format!("{dir}/{name}.ppm"), out).expect("a shot");
    };
    for (look, tag) in [(Appearance::Light, ""), (Appearance::Dark, "-dark")] {
        let mut ui = Ui::new(Scale::ONE, look, WIDTH, HEIGHT, false);
        save(&mut ui, &format!("login{tag}"));
        let mut ui = Ui::new(Scale::ONE, look, WIDTH, HEIGHT, true);
        save(&mut ui, &format!("list{tag}"));
        ui.open("Оксана Коваль");
        ui.type_text("Привіт! Пишу з нового вікна");
        save(&mut ui, &format!("chat{tag}"));
        ui.model.borrow_mut().composer.clear();
        ui.open("Zoraya — розробка");
        save(&mut ui, &format!("group{tag}"));
        ui.open("Иван Соколов");
        ui.focus(Role::SummaryButton);
        ui.key(keys::ENTER);
        ui.advance(2000);
        save(&mut ui, &format!("summary{tag}"));
        ui.key(keys::ESC);
        ui.focus(Role::SettingsButton);
        ui.key(keys::ENTER);
        save(&mut ui, &format!("settings{tag}"));
    }
    let mut ui = Ui::new(Scale::TWO, Appearance::Light, WIDTH, HEIGHT, true);
    ui.open("Оксана Коваль");
    save(&mut ui, "chat@2x");
}

// ---------------------------------------------------------------- step 4: where the view is

#[test]
fn the_position_and_a_jump_to_the_first_unread_message() {
    let mut ui = Ui::main();
    let palette = Appearance::Light.palette();
    let accent = palette.accent.pixel() & 0xff_ffff;
    ui.open("Project archive");
    ui.paint();
    {
        let m = ui.model.borrow();
        assert_eq!(m.unread_open, 120);
        assert!(m.unread_not_loaded(), "120 unread, a page of {}", m.messages.len());
        assert_eq!(m.scroll, 0);
    }
    assert!(ui.said("tg: unread 120"));
    // The pill at the top right says how many and how to get there.
    assert_eq!(ui.pixel(WIDTH as i32 - 18, 70), accent);
    // U in the messages: an older page first, then the first unread message at the view's
    // top, chosen, with the line above it.
    ui.focus(Role::Messages);
    ui.key(key::U);
    ui.paint();
    ui.pump();
    ui.paint();
    {
        let m = ui.model.borrow();
        assert!(!m.want_unread && !m.unread_not_loaded());
        let i = m.first_unread().expect("loaded now");
        assert_eq!(m.msg_cursor, Some(i));
        assert!(!m.messages[i].row.out && m.messages[i - 1].row.id == m.last_read.expect("set"));
        assert_eq!(m.messages.len() - i, 120, "the first of the 120");
        assert!(m.scroll > 0);
    }
    assert!(ui.said("tg: first unread:"));
    // It is in view: the unread pill has gone; the newer one is there.
    assert_ne!(ui.pixel(WIDTH as i32 - 18, 70), accent);
    // End: back to the newest, and the newer pill goes.
    ui.key(keys::END);
    ui.paint();
    assert_eq!(ui.model.borrow().scroll, 0);
    assert!(ui.said("tg: newest"));
    // And the message chosen up there is not chosen any more.
    assert_eq!(ui.model.borrow().msg_cursor, None);
    // Sending something takes the line away, as tgclient's separator.
    ui.focus(Role::Composer);
    ui.type_text("seen");
    ui.key(keys::ENTER);
    assert_eq!(ui.model.borrow().last_read, None);
    assert_eq!(ui.model.borrow().first_unread(), None);
    // Up from the composer chooses the newest message, the one just sent.
    ui.key(keys::UP);
    let m = ui.model.borrow();
    assert_eq!(m.msg_cursor, Some(m.messages.len() - 1));
    assert_eq!(m.messages[m.messages.len() - 1].row.text.text, "seen");
}

#[test]
fn up_from_the_composer_chooses_the_newest_message_after_a_jump_up() {
    // U chose a message far up; without End, Up from the composer still goes to the newest
    // (the failed send just above it, in tests/tg-socket.script), not next to that one.
    let mut ui = Ui::main();
    ui.open("Book club");
    ui.paint();
    ui.focus(Role::Messages);
    ui.key(key::U);
    let first = ui.model.borrow().msg_cursor.expect("chosen");
    assert!(first + 1 < ui.model.borrow().messages.len());
    ui.focus(Role::Composer);
    ui.key(keys::UP);
    let m = ui.model.borrow();
    assert_eq!(m.msg_cursor, Some(m.messages.len() - 1));
}

#[test]
fn a_chat_with_its_unread_messages_loaded_jumps_at_once() {
    let mut ui = Ui::main();
    ui.open("Book club");
    ui.paint();
    let first = {
        let m = ui.model.borrow();
        assert_eq!(m.unread_open, 3);
        assert!(!m.unread_not_loaded());
        m.first_unread().expect("loaded")
    };
    ui.focus(Role::Messages);
    ui.key(key::U);
    assert_eq!(ui.model.borrow().msg_cursor, Some(first));
    // No request for older pages was needed.
    assert!(!ui.model.borrow().want_unread);
}
