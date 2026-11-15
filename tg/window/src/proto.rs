//! The window's protocol (docs/tg-window.md, "The protocol"): what the window asks of its
//! backend and what the backend tells it. `SampleBackend` speaks these types directly;
//! stage B's `SocketBackend` writes each [`Request`] as a JSON line (`op`) and reads each
//! [`Event`] from one (`ev`), with the same names and fields.

use alloc::string::String;
use alloc::vec::Vec;

/// A chat's or a message's id (TDLib's int64; a string on the wire).
pub type Id = i64;

/// What a stretch of a text is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    Bold,
    Italic,
    Code,
    Link,
    Mention,
}

/// A stretch of a text, in code points: `[start, end)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: u32,
    pub end: u32,
    pub kind: SpanKind,
}

/// Text with its spans (`{"text": ..., "spans": [[start, end, kind], ...]}`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rich {
    pub text: String,
    pub spans: Vec<Span>,
}

impl Rich {
    pub fn plain(text: &str) -> Rich {
        Rich {
            text: String::from(text),
            spans: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatKind {
    Private,
    Group,
    Channel,
    /// Saved Messages.
    Saved,
}

/// A row of the chat list model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatRow {
    pub id: Id,
    pub title: String,
    pub kind: ChatKind,
    /// The last message as one line (with its sender's name in a group).
    pub last: String,
    /// The last message's time, Unix seconds.
    pub date: i64,
    pub unread: u32,
    pub pinned: bool,
    pub muted: bool,
}

/// What a message is. Everything but `Text` and `Service` is drawn as a labelled
/// placeholder until Zoraya has decoders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Text,
    /// A line of the chat itself ("Olena joined the group").
    Service,
    Photo,
    Sticker,
    Voice,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendState {
    /// Sent from here, not yet confirmed.
    Pending,
    Sent,
    /// Read by the other side.
    Read,
}

/// The message a reply answers, as one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub sender: String,
    pub text: String,
}

/// A row of the message list model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageRow {
    pub id: Id,
    /// Unix seconds.
    pub date: i64,
    pub out: bool,
    /// The sender's name; shown on incoming messages of a group.
    pub sender: String,
    pub kind: MessageKind,
    /// The text, or a media message's caption.
    pub text: Rich,
    /// A placeholder's detail: "1280 × 960", "0:12", "report.pdf · 248 KB".
    pub detail: String,
    pub reply: Option<Quote>,
    /// "Forwarded from" this name.
    pub forward: Option<String>,
    pub edited: bool,
    pub state: SendState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    /// Not yet said.
    Starting,
    WaitPhone,
    WaitCode,
    WaitPassword,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Unread,
    Day,
    Week,
}

impl Scope {
    pub fn word(self) -> &'static str {
        match self {
            Scope::Unread => "unread",
            Scope::Day => "day",
            Scope::Week => "week",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SummaryState {
    Running,
    Done,
    Failed,
}

/// A setting the backend keeps: its key, the words shown, on or off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setting {
    pub key: String,
    pub label: String,
    pub on: bool,
}

/// What the window asks (`{"op": ...}`). None of them blocks; answers come as events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `auth.phone`, `auth.code`, `auth.password`: the value typed.
    AuthPhone(String),
    AuthCode(String),
    AuthPassword(String),
    /// `auth.logout`.
    Logout,
    /// `chats.window`: the rows `[first, first + count)` of the chat list filtered by
    /// `query` (empty: all), and from now on its changes. Answered by `ChatsRows`.
    ChatsWindow {
        first: u32,
        count: u32,
        query: String,
    },
    /// `chat.open`: the newest `count` messages, then its changes; the chat is read.
    ChatOpen { chat: Id, count: u32 },
    /// `chat.close`.
    ChatClose,
    /// `messages.older`: the page of at most `count` messages before `before`.
    MessagesOlder { chat: Id, before: Id, count: u32 },
    /// `messages.send`.
    MessagesSend {
        chat: Id,
        text: String,
        reply_to: Option<Id>,
    },
    /// `ai.summarize`, answered by `Summary` events.
    Summarize { chat: Id, scope: Scope },
    /// `ai.cancel`.
    SummaryCancel,
    /// `settings.set`.
    SettingsSet { key: String, on: bool },
}

/// What the backend tells (`{"ev": ...}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `hello`: the wall clock (Unix seconds) and the zone's offset in seconds; whether
    /// the data is synthetic (then the window may print it on the console for tests).
    Hello {
        now: i64,
        utc_offset: i32,
        synthetic: bool,
    },
    /// `auth.state`: what is asked now, a hint for it, and why the last answer was
    /// refused (empty: it was not).
    Auth {
        state: AuthState,
        hint: String,
        error: String,
    },
    /// `chats.rows`: the window asked for, whole.
    ChatsRows {
        first: u32,
        total: u32,
        rows: Vec<ChatRow>,
    },
    /// `chats.splice`: in the window, `remove` rows from absolute index `at` are
    /// replaced by `rows`.
    ChatsSplice {
        at: u32,
        remove: u32,
        rows: Vec<ChatRow>,
        total: u32,
    },
    /// `messages.reset`: the open chat's newest messages, oldest first.
    MessagesReset {
        chat: Id,
        title: String,
        subtitle: String,
        kind: ChatKind,
        rows: Vec<MessageRow>,
        /// Older ones exist.
        more: bool,
    },
    /// `messages.page`: a page of older messages, oldest first.
    MessagesPage {
        chat: Id,
        rows: Vec<MessageRow>,
        more: bool,
    },
    /// `message.new`: a message at the end of the open chat.
    MessageNew { chat: Id, row: MessageRow },
    /// `message.update`: the message `old_id` is now `row` (edited, confirmed with its
    /// final id, read).
    MessageUpdate { chat: Id, old_id: Id, row: MessageRow },
    /// `message.delete`.
    MessageDelete { chat: Id, ids: Vec<Id> },
    /// `ai.summary`: `text` is added to the summary shown.
    Summary {
        chat: Id,
        state: SummaryState,
        text: String,
    },
    /// `settings`: every setting.
    Settings { items: Vec<Setting> },
}

/// A backend: `SampleBackend` now, `SocketBackend` (tgd over a Unix socket) in stage B.
pub trait Backend {
    /// Takes a request; never blocks.
    fn send(&mut self, r: Request);
    /// The next event that is ready at `now_ms` (a monotonic clock), if any.
    fn poll(&mut self, now_ms: u64) -> Option<Event>;
    /// When `poll` will next have something without a request, if it knows (the sample's
    /// timers); a socket backend wakes the loop by its handle instead.
    fn next_wake(&self) -> Option<u64>;
}
