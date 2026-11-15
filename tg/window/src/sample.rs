//! The sample backend: an account that exists only in this file. Every name and every
//! message is invented (English, Ukrainian and Russian, so that Cyrillic is drawn);
//! nothing here comes from a real account. It answers the window's requests as `tgd`
//! will, and its timers deliver new messages, confirmations and a summary in parts, so
//! that the window's updates are real ones.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::proto::{
    AuthState, Backend, ChatKind, ChatRow, Event, Id, MessageKind, MessageRow, Quote, Request,
    Rich, Scope, SendState, Setting, Span, SpanKind, SummaryState,
};
use crate::time::days_from_civil;

/// The sample's wall clock at its start: 2026-11-14 12:00:00 UTC, in a zone two hours
/// east (14:00 there).
pub const START: i64 = days_from_civil(2026, 11, 14) * 86_400 + 12 * 3600;
pub const UTC_OFFSET: i32 = 7200;
/// What the sample's login takes.
pub const CODE: &str = "12345";
pub const PASSWORD: &str = "zoraya";
/// The first message from the timer comes this long after the login, the next ones
/// this far apart.
pub const FIRST_INCOMING_MS: u64 = 4000;
pub const INCOMING_EVERY_MS: u64 = 6000;
/// A sent message is confirmed after this long, and read (in a private chat) after
/// this long.
pub const DELIVER_MS: u64 = 400;
pub const READ_MS: u64 = 1800;
/// A summary comes in parts this far apart.
pub const SUMMARY_PART_MS: u64 = 300;

/// The messages of the sample's longest chat, `Project archive`.
pub const ARCHIVE: usize = 1000;

const ME: &str = "You";

struct Chat {
    row: ChatRow,
    subtitle: String,
    messages: Vec<MessageRow>,
    next: Id,
    unread_at_open: u32,
}

enum Timed {
    Incoming,
    Delivered { chat: Id, temp: Id },
    Read { chat: Id, id: Id },
    SummaryPart(usize),
}

pub struct SampleBackend {
    started: bool,
    now_ms: u64,
    auth: AuthState,
    chats: Vec<Chat>,
    window: Option<(u32, u32, String)>,
    /// The window's rows as the window has them.
    sent: Vec<ChatRow>,
    open: Option<Id>,
    queue: VecDeque<Event>,
    timers: Vec<(u64, Timed)>,
    settings: Vec<Setting>,
    script_at: usize,
    next_temp: Id,
    summary: Option<(Id, Vec<String>)>,
    /// When the timer being fired was due.
    firing: Option<u64>,
}

/// Text with a little markup, for the sample's messages: `*bold*`, `_italic_`,
/// `` `code` ``, `<link>`, and `@name` as a mention.
pub fn rich(s: &str) -> Rich {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut open: Option<(char, u32)> = None;
    let mut n = 0u32;
    let mut mention: Option<u32> = None;
    for ch in s.chars() {
        let kind = |c: char| match c {
            '*' => SpanKind::Bold,
            '_' => SpanKind::Italic,
            '`' => SpanKind::Code,
            _ => SpanKind::Link,
        };
        if let Some(start) = mention
            && !(ch.is_alphanumeric() || ch == '_')
        {
            spans.push(Span {
                start,
                end: n,
                kind: SpanKind::Mention,
            });
            mention = None;
        }
        match (ch, open) {
            ('*' | '`', None) | ('<', None) => open = Some((ch, n)),
            ('_', None) if mention.is_none() => open = Some((ch, n)),
            ('*' | '_' | '`', Some((o, start))) if o == ch => {
                spans.push(Span {
                    start,
                    end: n,
                    kind: kind(o),
                });
                open = None;
            }
            ('>', Some(('<', start))) => {
                spans.push(Span {
                    start,
                    end: n,
                    kind: SpanKind::Link,
                });
                open = None;
            }
            _ => {
                if ch == '@' && open.is_none() {
                    mention = Some(n);
                }
                text.push(ch);
                n += 1;
            }
        }
    }
    if let Some(start) = mention {
        spans.push(Span {
            start,
            end: n,
            kind: SpanKind::Mention,
        });
    }
    Rich { text, spans }
}

fn message(id: Id, date: i64, out: bool, sender: &str, text: &str) -> MessageRow {
    MessageRow {
        id,
        date,
        out,
        sender: String::from(if out { ME } else { sender }),
        kind: MessageKind::Text,
        text: rich(text),
        detail: String::new(),
        reply: None,
        forward: None,
        edited: false,
        state: SendState::Read,
    }
}

fn one_line(s: &str) -> String {
    s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect()
}

/// A chat list's line for a message.
fn preview(kind: ChatKind, m: &MessageRow) -> String {
    let body = match m.kind {
        MessageKind::Text | MessageKind::Service => one_line(&m.text.text),
        MessageKind::Photo => String::from("Photo"),
        MessageKind::Sticker => String::from("Sticker"),
        MessageKind::Voice => String::from("Voice message"),
        MessageKind::File => format!("File: {}", m.detail),
    };
    match (kind, m.kind, m.out) {
        (_, MessageKind::Service, _) => body,
        (ChatKind::Group, _, true) => format!("You: {body}"),
        (ChatKind::Group, _, false) => {
            let first = m.sender.split(' ').next().unwrap_or("");
            format!("{first}: {body}")
        }
        _ => body,
    }
}

/// Builds one chat: messages are `(minutes before START, out, sender, text)`, oldest
/// first, then changed by `fix` (replies, placeholders, and so on).
struct Build {
    chats: Vec<Chat>,
}

impl Build {
    #[allow(clippy::too_many_arguments)] // One row of the sample's table.
    fn chat(
        &mut self,
        title: &str,
        kind: ChatKind,
        subtitle: &str,
        unread: u32,
        pinned: bool,
        muted: bool,
        lines: &[(i64, bool, &str, &str)],
    ) -> &mut Chat {
        let id = 1000 + self.chats.len() as Id;
        let base = id * 100_000;
        let messages: Vec<MessageRow> = lines
            .iter()
            .enumerate()
            .map(|(i, (ago, out, sender, text))| {
                let who = if kind == ChatKind::Private && !*out {
                    title
                } else {
                    sender
                };
                let mut m = message(base + i as Id + 1, START - ago * 60, *out, who, text);
                if kind == ChatKind::Channel {
                    m.sender = String::from(title);
                }
                m
            })
            .collect();
        self.chats.push(Chat {
            row: ChatRow {
                id,
                title: String::from(title),
                kind,
                last: String::new(),
                date: 0,
                unread,
                pinned,
                muted,
            },
            subtitle: String::from(subtitle),
            next: base + messages.len() as Id + 1,
            messages,
            unread_at_open: 0,
        });
        self.chats.last_mut().expect("just pushed")
    }
}

const EN: [&str; 10] = [
    "Did the build pass on your side?",
    "Yes, twice in a row. I will write it up tonight.",
    "The train leaves at 7:40, platform 3.",
    "Can we move the call to Thursday?",
    "Thursday works. Same time.",
    "I left the keys with the neighbour.",
    "The second chapter is much better than the first.",
    "Sending the notes in a minute.",
    "Thanks, got them.",
    "See you tomorrow.",
];
const UK: [&str; 10] = [
    "Привіт! Як справи?",
    "Все добре, дякую. Працюю над звітом.",
    "Збірка пройшла, тести зелені.",
    "Чудово. Тоді завтра показуємо.",
    "Не забудь парасольку, обіцяють дощ.",
    "Потяг о 7:40, зустрінемось на вокзалі.",
    "Надіслав тобі файл, подивись, будь ласка.",
    "Дякую, вже дивлюсь.",
    "Ґанок полагодили, можна заходити з двору.",
    "До завтра!",
];
const RU: [&str; 10] = [
    "Привет! Ты сегодня в офисе?",
    "Нет, работаю из дома. Что-то срочное?",
    "Не срочно. Хотел показать новую сборку.",
    "Давай после обеда, часа в три.",
    "Договорились.",
    "Отчёт отправил, посмотри, когда будет время.",
    "Спасибо, посмотрю вечером.",
    "Поезд в 7:40, встречаемся на вокзале.",
    "Объявили дождь, возьми зонт.",
    "До завтра!",
];

/// A conversation of `n` lines from a pool, the last one `last_ago` minutes ago.
fn talk(
    pool: &'static [&'static str; 10],
    n: usize,
    last_ago: i64,
    others: &[&'static str],
    seed: usize,
) -> Vec<(i64, bool, &'static str, &'static str)> {
    (0..n)
        .map(|i| {
            let k = (i * 7 + seed) % 10;
            let out = (i + seed) % 3 == 1;
            let who = others[(i + seed) % others.len()];
            let ago = last_ago + ((n - 1 - i) as i64) * (17 + (seed as i64 % 5) * 9);
            (ago, out, who, pool[k])
        })
        .collect()
}

fn account() -> Vec<Chat> {
    let mut b = Build { chats: Vec::new() };
    let g = ChatKind::Group;
    let p = ChatKind::Private;

    // Saved Messages: notes to oneself, a link, a file.
    let c = b.chat(
        "Saved Messages",
        ChatKind::Saved,
        "your notes",
        0,
        true,
        false,
        &[
            (4300, true, "", "Zoraya: `make smoke`, then `make smoke-tgd`"),
            (2900, true, "", "Ідеї для вікна: список чатів, пошук, підсумок"),
            (1500, true, "", "Notes of the meeting: <https://example.org/zoraya/notes>"),
            (190, true, "", "План на тиждень"),
        ],
    );
    c.messages[3].kind = MessageKind::File;
    c.messages[3].detail = String::from("plan.md · 12 KB");

    // A group with a long history, for paging.
    let team: [&str; 4] = [
        "Оксана Коваль",
        "Тарас Мельник",
        "Ірина Бойко",
        "Andrew Whitlock",
    ];
    let mut lines: Vec<(i64, bool, &str, &str)> = Vec::new();
    for i in 0..150usize {
        let pool = if i % 4 == 3 { &EN } else { &UK };
        let who = if i % 4 == 3 { team[3] } else { team[i % 3] };
        lines.push((
            40 + (149 - i as i64) * 23,
            i % 7 == 2,
            who,
            pool[(i * 3 + i / 10) % 10],
        ));
    }
    lines.push((33, false, team[1], "Хто сьогодні чергує на збірці?"));
    lines.push((31, false, team[0], "Я. @taras, глянь, будь ласка, *журнал тестів*."));
    lines.push((12, true, "", "Журнал чистий, 78 кроків пройшли."));
    let c = b.chat("Zoraya — розробка", g, "5 members", 0, true, false, &lines);
    c.messages[20].kind = MessageKind::Service;
    c.messages[20].text = Rich::plain("Ірина Бойко joined the group");
    c.messages[151].reply = Some(Quote {
        sender: String::from(team[1]),
        text: String::from("Хто сьогодні чергує на збірці?"),
    });

    // A private chat with everything the view draws.
    let c = b.chat(
        "Оксана Коваль",
        p,
        "last seen recently",
        2,
        false,
        false,
        &[
            (1600, false, "", "Привіт! Ти вже бачив нову збірку?"),
            (1598, true, "", "Привіт! Ще ні, зараз подивлюсь."),
            (1590, false, "", "Там тепер є вікно зі списком чатів. Дуже зручно: ліворуч чати, праворуч розмова, а внизу поле, де можна писати довгі повідомлення на кілька рядків."),
            (1580, true, "", "Виглядає добре. А кирилиця: ї, є, ґ, і — все на місці?"),
            (1578, false, "", "Так, усі літери є."),
            (1570, false, "", "Ось знімок екрана"),
            (1560, true, "", "Дякую! Опис тут: <https://example.org/zoraya/window>"),
            (300, false, "", ""),
            (298, true, "", "Послухаю ввечері."),
            (130, false, "", "Звіт за листопад"),
            (126, true, "", "Отримав, *дякую*. Відповім до п'ятниці."),
            (62, false, "", "Добре. І ще: зустріч перенесли на 15:30."),
            (60, false, "", "Не запізнюйся :)"),
        ],
    );
    c.messages[3].edited = true;
    c.messages[4].reply = Some(Quote {
        sender: String::from(ME),
        text: String::from("Виглядає добре. А кирилиця: ї, є, ґ, і — все на місці?"),
    });
    c.messages[5].kind = MessageKind::Photo;
    c.messages[5].detail = String::from("1280 × 800");
    c.messages[7].kind = MessageKind::Voice;
    c.messages[7].detail = String::from("0:42");
    c.messages[9].kind = MessageKind::File;
    c.messages[9].detail = String::from("звіт-листопад.pdf · 248 KB");

    let c = b.chat(
        "Иван Соколов",
        p,
        "online",
        0,
        false,
        false,
        &[
            (2000, false, "", "Привет! Ты сегодня в офисе?"),
            (1995, true, "", "Нет, работаю из дома. Что-то срочное?"),
            (1990, false, "", "Не срочно. Хотел показать новую сборку: теперь окно само подстраивается под размер."),
            (1985, false, "", "Смотри, что прислали в общий канал"),
            (1700, true, "", "Интересно. Давай после обеда, часа в три."),
            (1695, false, "", ""),
            (240, false, "", "Объявили дождь, возьми зонт."),
            (200, true, "", "Спасибо, взял. Ещё вопрос: где лежит `tgd.log`?"),
            (95, false, "", "В каталоге данных, рядом с базой."),
        ],
    );
    c.messages[3].forward = Some(String::from("Zoraya News"));
    c.messages[3].text = rich("*Zoraya 10*: programs installed on the volume keep their grants.");
    c.messages[5].kind = MessageKind::Sticker;
    c.messages[5].detail = String::from("a cat waving");

    b.chat(
        "Родина",
        g,
        "4 members",
        5,
        false,
        true,
        &[
            (900, false, "Мама", "Хто приїде на вихідні?"),
            (880, true, "", "Я буду в суботу зранку."),
            (700, false, "Тато", "Тоді в суботу їдемо по яблука."),
            (400, false, "Мама", "Пиріг уже в духовці."),
            (150, false, "Соломія", "Я привезу варення!"),
            (110, false, "Мама", "Не забудь подзвонити бабусі."),
        ],
    );

    let c = b.chat(
        "Zoraya News",
        ChatKind::Channel,
        "channel · 1 204 subscribers",
        12,
        false,
        true,
        &[
            (5000, false, "", "*Stage 9* is tagged: C++, CPython and a browser run as ordinary programs."),
            (3200, false, "", "The wheel now scrolls the file manager, the editor and settings."),
            (1300, false, "", "*Zoraya 10*: programs installed on the volume keep their grants. Details: <https://example.org/zoraya/10>"),
            (170, false, "", "Tonight's build is green on both accelerators."),
        ],
    );
    c.messages[1].edited = true;

    let pools: [&'static [&'static str; 10]; 3] = [&UK, &EN, &RU];
    let table: [(&str, ChatKind, &str, u32, bool, usize, &[&'static str]); 21] = [
        ("Марина Ткаченко", p, "last seen today at 13:10", 0, false, 0, &["Марина Ткаченко"]),
        ("Book club", g, "9 members", 3, false, 1, &["Hanna Lindqvist", "Thomas Reiter", "Lucia Ferrand"]),
        ("Дмитрий Орлов", p, "last seen yesterday", 0, false, 2, &["Дмитрий Орлов"]),
        ("Hanna Lindqvist", p, "last seen recently", 1, false, 1, &["Hanna Lindqvist"]),
        ("Велоклуб «Дніпро»", g, "38 members", 0, true, 0, &["Петро Шевчук", "Юлія Савченко", "Олег Гнатюк"]),
        ("Петро Шевчук", p, "last seen 2 hours ago", 0, false, 0, &["Петро Шевчук"]),
        ("Rust Kyiv", g, "212 members", 27, true, 1, &["Марк Левченко", "Andrew Whitlock", "Ірина Бойко"]),
        ("Анна Белова", p, "online", 0, false, 2, &["Анна Белова"]),
        ("Thomas Reiter", p, "last seen last week", 0, false, 1, &["Thomas Reiter"]),
        ("Сусіди, буд. 12", g, "64 members", 8, true, 0, &["Василь Ковальчук", "Марина Ткаченко"]),
        ("Weather Bot", p, "bot", 0, false, 1, &["Weather Bot"]),
        ("Юлія Савченко", p, "last seen recently", 0, false, 0, &["Юлія Савченко"]),
        ("Night Trains", ChatKind::Channel, "channel · 380 subscribers", 0, true, 1, &["Night Trains"]),
        ("Олег Гнатюк", p, "last seen 3 days ago", 0, false, 0, &["Олег Гнатюк"]),
        ("Екатерина Морозова", p, "last seen recently", 0, false, 2, &["Екатерина Морозова"]),
        ("Photo walk", g, "14 members", 0, false, 1, &["Lucia Ferrand", "Hanna Lindqvist"]),
        ("Василь Ковальчук", p, "last seen yesterday", 0, false, 0, &["Василь Ковальчук"]),
        ("Lucia Ferrand", p, "last seen recently", 0, false, 1, &["Lucia Ferrand"]),
        ("Команда 7", g, "7 members", 0, false, 2, &["Дмитрий Орлов", "Анна Белова"]),
        ("Марк Левченко", p, "last seen a month ago", 0, false, 0, &["Марк Левченко"]),
        ("Соломія", p, "last seen recently", 0, false, 0, &["Соломія"]),
    ];
    for (i, (title, kind, subtitle, unread, muted, pool, others)) in table.iter().enumerate() {
        // The first of them a quarter of an hour ago, the last ones weeks ago.
        let last_ago = 14 + (i as i64) * (i as i64) * 61;
        let lines = talk(pools[*pool], 6 + i % 8, last_ago, others, i);
        b.chat(title, *kind, subtitle, *unread, false, *muted, &lines);
    }
    // A thousand messages over two months, for the view's cost with a long history.
    let mut lines: Vec<(i64, bool, &str, &str)> = Vec::new();
    for i in 0..ARCHIVE {
        let pool = pools[i % 3];
        lines.push((
            40_000 + (ARCHIVE - 1 - i) as i64 * 86,
            i % 5 == 3,
            team[i % 4],
            pool[(i * 7 + i / 10) % 10],
        ));
    }
    b.chat("Project archive", g, "12 members", 0, false, true, &lines);
    for c in &mut b.chats {
        // What is not read yet came from the others.
        let n = c.messages.len();
        for m in c.messages.iter_mut().skip(n.saturating_sub(c.row.unread as usize)) {
            m.out = false;
        }
        if let Some(m) = c.messages.last() {
            c.row.last = preview(c.row.kind, m);
            c.row.date = m.date;
        }
    }
    b.chats
}

/// The messages the timer delivers, in turn: the chat's title, the sender, the text.
const INCOMING: [(&str, &str, &str); 8] = [
    ("Оксана Коваль", "Оксана Коваль", "Нагадую: зустріч о 15:30, кімната 4."),
    ("Иван Соколов", "Иван Соколов", "Отчёт отправил, посмотри, когда будет время."),
    ("Родина", "Тато", "Яблука зібрали, чекаємо в суботу."),
    ("Book club", "Thomas Reiter", "Next meeting is on Thursday at seven."),
    ("Zoraya — розробка", "Ірина Бойко", "Нова збірка готова, можна перевіряти."),
    ("Оксана Коваль", "Оксана Коваль", "І візьми, будь ласка, зарядку."),
    ("Zoraya News", "Zoraya News", "The window for chats is in tonight's build."),
    ("Hanna Lindqvist", "Hanna Lindqvist", "Are you coming to the photo walk?"),
];

impl Default for SampleBackend {
    fn default() -> SampleBackend {
        SampleBackend::new()
    }
}

impl SampleBackend {
    pub fn new() -> SampleBackend {
        let setting = |key: &str, label: &str, on: bool| Setting {
            key: String::from(key),
            label: String::from(label),
            on,
        };
        SampleBackend {
            started: false,
            now_ms: 0,
            auth: AuthState::Starting,
            chats: account(),
            window: None,
            sent: Vec::new(),
            open: None,
            queue: VecDeque::new(),
            timers: Vec::new(),
            settings: alloc::vec![
                setting("ai.enabled", "AI summaries of a chat", true),
                setting("list.previews", "Show the last message in the chat list", true),
                setting("notify.muted", "Count unread messages of muted chats", true),
            ],
            script_at: 0,
            next_temp: -1,
            summary: None,
            firing: None,
        }
    }

    /// A backend already logged in, for tests of what comes after the login.
    pub fn logged_in() -> SampleBackend {
        let mut b = SampleBackend::new();
        b.auth = AuthState::WaitPhone;
        b.send(Request::AuthPhone(String::from("0000")));
        b.send(Request::AuthCode(String::from(CODE)));
        b.send(Request::AuthPassword(String::from(PASSWORD)));
        b
    }

    fn now(&self) -> i64 {
        START + (self.now_ms / 1000) as i64
    }

    fn setting(&self, key: &str) -> bool {
        self.settings.iter().any(|s| s.key == key && s.on)
    }

    fn auth_event(&mut self, hint: &str, error: &str) {
        self.queue.push_back(Event::Auth {
            state: self.auth,
            hint: String::from(hint),
            error: String::from(error),
        });
    }

    fn ask(&mut self, error: &str) {
        match self.auth {
            AuthState::WaitPhone => {
                self.auth_event("This is the sample account: any number will do.", error)
            }
            AuthState::WaitCode => self.auth_event("The sample's code is 12345.", error),
            AuthState::WaitPassword => self.auth_event("The sample's password is zoraya.", error),
            _ => self.auth_event("", error),
        }
    }

    /// A timer `after_ms` from now, or from the moment the timer being fired was due
    /// (so that a chain of timers keeps its pace when the clock jumps).
    fn at(&mut self, after_ms: u64, t: Timed) {
        self.timers.push((self.firing.unwrap_or(self.now_ms) + after_ms, t));
    }

    fn chat(&mut self, id: Id) -> Option<&mut Chat> {
        self.chats.iter_mut().find(|c| c.row.id == id)
    }

    /// The chat list as the window sees it: pinned first, then the newest; filtered by
    /// the window's query.
    pub(crate) fn list(&self, query: &str) -> Vec<ChatRow> {
        let q = query.trim().to_lowercase();
        let mut rows: Vec<&Chat> = self
            .chats
            .iter()
            .filter(|c| q.is_empty() || c.row.title.to_lowercase().contains(&q))
            .collect();
        rows.sort_by(|a, b| {
            (b.row.pinned, b.row.date, b.row.id).cmp(&(a.row.pinned, a.row.date, a.row.id))
        });
        let previews = self.setting("list.previews");
        let muted = self.setting("notify.muted");
        rows.into_iter()
            .map(|c| {
                let mut r = c.row.clone();
                if !previews {
                    r.last = String::from("Message");
                }
                if r.muted && !muted {
                    r.unread = 0;
                }
                r
            })
            .collect()
    }

    fn window_rows(&self) -> (u32, Vec<ChatRow>) {
        let Some((first, count, query)) = &self.window else {
            return (0, Vec::new());
        };
        let all = self.list(query);
        let total = all.len() as u32;
        let rows = all
            .into_iter()
            .skip(*first as usize)
            .take(*count as usize)
            .collect();
        (total, rows)
    }

    /// Tells the window what changed in its rows: the stretch between what is the same
    /// at the start and at the end.
    fn list_changed(&mut self) {
        let Some((first, ..)) = self.window else { return };
        let (total, new) = self.window_rows();
        let old = &self.sent;
        let most = old.len().min(new.len());
        let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
        let suffix = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take(most - prefix)
            .take_while(|(a, b)| a == b)
            .count();
        if prefix == old.len() && prefix == new.len() {
            return;
        }
        self.queue.push_back(Event::ChatsSplice {
            at: first + prefix as u32,
            remove: (old.len() - prefix - suffix) as u32,
            rows: new[prefix..new.len() - suffix].to_vec(),
            total,
        });
        self.sent = new;
    }

    fn incoming(&mut self) {
        let (title, sender, text) = INCOMING[self.script_at % INCOMING.len()];
        self.script_at += 1;
        self.at(INCOMING_EVERY_MS, Timed::Incoming);
        let (now, open) = (self.now(), self.open);
        let Some(c) = self.chats.iter_mut().find(|c| c.row.title == title) else {
            return;
        };
        let m = message(c.next, now, false, sender, text);
        c.next += 1;
        c.row.last = preview(c.row.kind, &m);
        c.row.date = now;
        c.messages.push(m.clone());
        let id = c.row.id;
        if open == Some(id) {
            self.queue.push_back(Event::MessageNew { chat: id, row: m });
        } else {
            c.row.unread += 1;
        }
        self.list_changed();
    }

    fn fire(&mut self, t: Timed) {
        match t {
            Timed::Incoming => self.incoming(),
            Timed::Delivered { chat, temp } => {
                let open = self.open;
                let Some(c) = self.chat(chat) else { return };
                let id = c.next;
                let kind = c.row.kind;
                let Some(m) = c.messages.iter_mut().find(|m| m.id == temp) else {
                    return;
                };
                c.next += 1;
                m.id = id;
                // Notes to oneself are read at once; a group's or a channel's show one tick.
                m.state = if kind == ChatKind::Saved {
                    SendState::Read
                } else {
                    SendState::Sent
                };
                let row = m.clone();
                if open == Some(chat) {
                    self.queue.push_back(Event::MessageUpdate {
                        chat,
                        old_id: temp,
                        row,
                    });
                }
                if kind == ChatKind::Private {
                    self.at(READ_MS - DELIVER_MS, Timed::Read { chat, id });
                }
            }
            Timed::Read { chat, id } => {
                let open = self.open;
                let Some(m) = self
                    .chat(chat)
                    .and_then(|c| c.messages.iter_mut().find(|m| m.id == id))
                else {
                    return;
                };
                m.state = SendState::Read;
                let row = m.clone();
                if open == Some(chat) {
                    self.queue.push_back(Event::MessageUpdate {
                        chat,
                        old_id: id,
                        row,
                    });
                }
            }
            Timed::SummaryPart(i) => {
                let Some((chat, parts)) = &self.summary else {
                    return;
                };
                let (chat, last) = (*chat, i + 1 == parts.len());
                let text = parts[i].clone();
                self.queue.push_back(Event::Summary {
                    chat,
                    state: if last {
                        SummaryState::Done
                    } else {
                        SummaryState::Running
                    },
                    text,
                });
                if last {
                    self.summary = None;
                } else {
                    self.at(SUMMARY_PART_MS, Timed::SummaryPart(i + 1));
                }
            }
        }
    }

    fn summarize(&mut self, chat: Id, scope: Scope) {
        self.summary = None;
        if !self.setting("ai.enabled") {
            self.queue.push_back(Event::Summary {
                chat,
                state: SummaryState::Failed,
                text: String::from("AI summaries are turned off in Settings."),
            });
            return;
        }
        let now = self.now();
        let Some(c) = self.chat(chat) else { return };
        let within = |secs: i64| c.messages.iter().filter(|m| now - m.date <= secs).count();
        let (n, what) = match scope {
            Scope::Unread => (c.unread_at_open as usize, "unread"),
            Scope::Day => (within(86_400), "of the last day"),
            Scope::Week => (within(7 * 86_400), "of the last week"),
        };
        let what = match (scope, n) {
            (Scope::Unread, 1) => String::from("unread message"),
            (Scope::Unread, _) => String::from("unread messages"),
            (_, 1) => format!("message {what}"),
            _ => format!("messages {what}"),
        };
        let title = c.row.title.clone();
        let parts = if n == 0 {
            alloc::vec![format!("{title}: no {what} to summarise.")]
        } else {
            alloc::vec![
                format!("{title}: {n} {what}. "),
                String::from("The main points: the build passes and is shown tomorrow; "),
                String::from("the meeting moved to 15:30; a report waits for an answer by Friday.\n"),
                String::from("This is the sample backend's text: no model was asked."),
            ]
        };
        self.summary = Some((chat, parts));
        self.at(SUMMARY_PART_MS, Timed::SummaryPart(0));
    }

    fn send_message(&mut self, chat: Id, text: String, reply_to: Option<Id>) {
        let (now, temp) = (self.now(), self.next_temp);
        let Some(c) = self.chat(chat) else { return };
        let mut m = message(temp, now, true, "", "");
        m.text = Rich::plain(&text);
        m.state = SendState::Pending;
        m.reply = reply_to
            .and_then(|id| c.messages.iter().find(|m| m.id == id))
            .map(|r| Quote {
                sender: r.sender.clone(),
                text: match r.kind {
                    MessageKind::Text | MessageKind::Service => one_line(&r.text.text),
                    k => String::from(crate::model::kind_word(k)),
                },
            });
        c.row.last = preview(c.row.kind, &m);
        c.row.date = now;
        c.messages.push(m.clone());
        self.next_temp -= 1;
        self.queue.push_back(Event::MessageNew { chat, row: m });
        self.at(DELIVER_MS, Timed::Delivered { chat, temp });
        self.list_changed();
    }
}

impl Backend for SampleBackend {
    fn send(&mut self, r: Request) {
        match r {
            Request::AuthPhone(v) if self.auth == AuthState::WaitPhone => {
                let digits = v.chars().filter(char::is_ascii_digit).count();
                let other = v.chars().any(|c| !(c.is_ascii_digit() || c == '+' || c == ' '));
                if digits < 4 || other {
                    self.ask("PHONE_NUMBER_INVALID");
                } else {
                    self.auth = AuthState::WaitCode;
                    self.ask("");
                }
            }
            Request::AuthCode(v) if self.auth == AuthState::WaitCode => {
                if v == CODE {
                    self.auth = AuthState::WaitPassword;
                    self.ask("");
                } else {
                    self.ask("PHONE_CODE_INVALID");
                }
            }
            Request::AuthPassword(v) if self.auth == AuthState::WaitPassword => {
                if v == PASSWORD {
                    self.auth = AuthState::Ready;
                    self.ask("");
                    self.queue.push_back(Event::Settings {
                        items: self.settings.clone(),
                    });
                    self.at(FIRST_INCOMING_MS, Timed::Incoming);
                } else {
                    self.ask("PASSWORD_HASH_INVALID");
                }
            }
            Request::AuthPhone(_) | Request::AuthCode(_) | Request::AuthPassword(_) => {
                self.ask("this is not what is asked now")
            }
            Request::Logout => {
                // A new account of the same sample: nothing of the session stays.
                let (settings, now_ms) = (core::mem::take(&mut self.settings), self.now_ms);
                *self = SampleBackend::new();
                self.settings = settings;
                self.now_ms = now_ms;
                self.started = true;
                self.auth = AuthState::WaitPhone;
                self.ask("");
            }
            _ if self.auth != AuthState::Ready => {}
            Request::ChatsWindow {
                first,
                count,
                query,
            } => {
                self.window = Some((first, count, query));
                let (total, rows) = self.window_rows();
                self.sent = rows.clone();
                self.queue
                    .push_back(Event::ChatsRows { first, total, rows });
            }
            Request::ChatOpen { chat, count } => {
                let Some(c) = self.chat(chat) else { return };
                c.unread_at_open = c.row.unread;
                c.row.unread = 0;
                let from = c.messages.len().saturating_sub(count as usize);
                let e = Event::MessagesReset {
                    chat,
                    title: c.row.title.clone(),
                    subtitle: c.subtitle.clone(),
                    kind: c.row.kind,
                    rows: c.messages[from..].to_vec(),
                    more: from > 0,
                };
                self.open = Some(chat);
                self.queue.push_back(e);
                self.list_changed();
            }
            Request::ChatClose => self.open = None,
            Request::MessagesOlder {
                chat,
                before,
                count,
            } => {
                let Some(c) = self.chat(chat) else { return };
                let end = c
                    .messages
                    .iter()
                    .position(|m| m.id == before)
                    .unwrap_or(0);
                let from = end.saturating_sub(count as usize);
                let e = Event::MessagesPage {
                    chat,
                    rows: c.messages[from..end].to_vec(),
                    more: from > 0,
                };
                self.queue.push_back(e);
            }
            Request::MessagesSend {
                chat,
                text,
                reply_to,
            } => self.send_message(chat, text, reply_to),
            Request::Summarize { chat, scope } => self.summarize(chat, scope),
            Request::SummaryCancel => self.summary = None,
            Request::SettingsSet { key, on } => {
                if let Some(s) = self.settings.iter_mut().find(|s| s.key == key) {
                    s.on = on;
                }
                self.queue.push_back(Event::Settings {
                    items: self.settings.clone(),
                });
                self.list_changed();
            }
        }
    }

    fn poll(&mut self, now_ms: u64) -> Option<Event> {
        self.now_ms = self.now_ms.max(now_ms);
        if !self.started {
            self.started = true;
            self.queue.push_front(Event::Hello {
                now: self.now(),
                utc_offset: UTC_OFFSET,
                synthetic: true,
            });
            if self.auth == AuthState::Starting {
                self.auth = AuthState::WaitPhone;
                self.ask("");
            }
        }
        // The timers that are due, the earliest first.
        while let Some(i) = self
            .timers
            .iter()
            .enumerate()
            .filter(|(_, (due, _))| *due <= self.now_ms)
            .min_by_key(|(_, (due, _))| *due)
            .map(|(i, _)| i)
        {
            let (due, t) = self.timers.remove(i);
            self.firing = Some(due);
            self.fire(t);
            self.firing = None;
        }
        self.queue.pop_front()
    }

    fn next_wake(&self) -> Option<u64> {
        if !self.started || !self.queue.is_empty() {
            return Some(self.now_ms);
        }
        self.timers.iter().map(|(due, _)| *due).min()
    }
}
