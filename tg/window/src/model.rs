//! The window's state: what the backend's events made of it and what the person did to
//! it. The widgets (`view`) read it and call its methods; the program sends the requests
//! it gathers (`take_requests`) to the backend and hands the backend's events to `apply`.
//! It holds no layout: sizes in pixels are the view's.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::proto::{
    AuthState, ChatKind, ChatRow, Event, Id, MessageKind, MessageRow, Request, Scope, SendState,
    Setting, SummaryState,
};

/// The messages asked for at a time.
pub const PAGE: u32 = 40;
/// The longest message the composer sends, in characters (Telegram's limit).
pub const MAX_MESSAGE: usize = 4096;

/// One line or several of text being typed, with its cursor as a byte index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Edit {
    pub text: String,
    pub cursor: usize,
}

impl Edit {
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn insert(&mut self, ch: char) {
        self.text.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    fn before(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    fn after(&self) -> usize {
        self.text[self.cursor..]
            .chars()
            .next()
            .map_or(self.cursor, |c| self.cursor + c.len_utf8())
    }

    pub fn backspace(&mut self) -> bool {
        let a = self.before();
        if a == self.cursor {
            return false;
        }
        self.text.replace_range(a..self.cursor, "");
        self.cursor = a;
        true
    }

    pub fn delete(&mut self) -> bool {
        let b = self.after();
        if b == self.cursor {
            return false;
        }
        self.text.replace_range(self.cursor..b, "");
        true
    }

    pub fn left(&mut self) {
        self.cursor = self.before();
    }

    pub fn right(&mut self) {
        self.cursor = self.after();
    }

    /// To the start of the cursor's line.
    pub fn home(&mut self) {
        self.cursor = self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1);
    }

    /// To the end of the cursor's line.
    pub fn end(&mut self) {
        self.cursor = self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Before the backend said anything.
    Starting,
    Login,
    Main,
    Settings,
}

/// What can hold the keyboard, for the program to move the toolkit's focus to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Search,
    SettingsButton,
    List,
    SummaryButton,
    Messages,
    Composer,
    Send,
    /// The summary card's buttons: the three scopes, then Close.
    Card(u32),
    LoginField,
    LoginNext,
    /// The settings screen's switches, then Log out and Back.
    Setting(u32),
    LogOut,
    Back,
}

/// A message with the count of its changes, so that the view lays it out again only
/// when it changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Msg {
    pub row: MessageRow,
    pub rev: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub scope: Scope,
    pub state: SummaryState,
    pub text: String,
}

/// The view's paint times, for the report's measurements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub frames: u32,
    pub total_us: u64,
    pub max_us: u32,
    /// The program sets `mark` when it puts a new tree in the window; the next frame's
    /// time is then kept in `first_us`.
    pub mark: bool,
    pub first_us: Option<u32>,
    /// Messages laid out by the last frame, and how many layouts are held.
    pub laid: u32,
    pub held: u32,
}

pub struct Model {
    pub screen: Screen,
    pub auth: AuthState,
    pub auth_hint: String,
    pub auth_error: String,
    pub login: Edit,
    /// The backend's data is synthetic: the console may show it (tests).
    pub synthetic: bool,

    /// The chat list: how many chats the filter leaves, the absolute index of the first
    /// row held, and the rows held.
    pub total: u32,
    pub first: u32,
    pub chats: Vec<ChatRow>,
    /// The first row shown, how many fit (the view says), and the keyboard's row.
    pub top: u32,
    pub list_rows: u32,
    pub cursor: Option<u32>,
    pub query: Edit,
    asked: Option<(u32, u32, String)>,

    /// The open chat.
    pub open: Option<Id>,
    pub title: String,
    pub subtitle: String,
    pub kind: ChatKind,
    pub messages: Vec<Msg>,
    pub more: bool,
    asked_older: bool,
    rev: u32,
    /// Logical pixels the view is scrolled up from the newest message.
    pub scroll: i32,
    /// The message at the view's top after the last paint: its id, its top from the
    /// messages' top (logical), and `scroll` then. The view keeps it there when heights
    /// around it change; it is here so that a new tree keeps it too.
    pub anchor: Option<(Id, i32, i32)>,
    /// The keyboard's message and the other end of the selection, as indices.
    pub msg_cursor: Option<usize>,
    pub msg_anchor: Option<usize>,
    /// The view brings the cursor's message into view at its next layout.
    pub reveal: bool,
    pub composer: Edit,
    pub reply_to: Option<Id>,
    pub summary: Option<Summary>,

    pub settings: Vec<Setting>,
    /// The window is too narrow for two panes (the view says): Escape leaves the chat.
    pub narrow: bool,
    /// The window's body, logical (the view says).
    pub size: (i32, i32),

    /// Counts every change of what the tree is made of: the program builds the tree
    /// again when it differs.
    pub tree: u32,
    pub focus: Option<Role>,
    pub stats: Stats,

    now_base: i64,
    mono_base: u64,
    mono: u64,
    pub utc_offset: i32,
    out: Vec<Request>,
    trace: Vec<String>,
}

impl Default for Model {
    fn default() -> Model {
        Model::new()
    }
}

fn one_line(s: &str) -> String {
    s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect()
}

impl Model {
    pub fn new() -> Model {
        Model {
            screen: Screen::Starting,
            auth: AuthState::Starting,
            auth_hint: String::new(),
            auth_error: String::new(),
            login: Edit::default(),
            synthetic: false,
            total: 0,
            first: 0,
            chats: Vec::new(),
            top: 0,
            list_rows: 1,
            cursor: None,
            query: Edit::default(),
            asked: None,
            open: None,
            title: String::new(),
            subtitle: String::new(),
            kind: ChatKind::Private,
            messages: Vec::new(),
            more: false,
            asked_older: false,
            rev: 0,
            scroll: 0,
            anchor: None,
            msg_cursor: None,
            msg_anchor: None,
            reveal: false,
            composer: Edit::default(),
            reply_to: None,
            summary: None,
            settings: Vec::new(),
            narrow: false,
            size: (0, 0),
            tree: 0,
            focus: None,
            stats: Stats::default(),
            now_base: 0,
            mono_base: 0,
            mono: 0,
            utc_offset: 0,
            out: Vec::new(),
            trace: Vec::new(),
        }
    }

    /// The monotonic clock, in milliseconds: the program sets it every round.
    pub fn set_clock(&mut self, mono_ms: u64) {
        self.mono = mono_ms;
    }

    /// The view says the window's size.
    pub fn set_size(&mut self, w: i32, h: i32) {
        if (w, h) != self.size {
            self.size = (w, h);
            self.say(format!("tg: size {w} {h}"));
        }
    }

    /// The wall clock, Unix seconds: the backend's at `hello` plus the time since.
    pub fn now(&self) -> i64 {
        self.now_base + (self.mono.saturating_sub(self.mono_base) / 1000) as i64
    }

    pub fn take_requests(&mut self) -> Vec<Request> {
        core::mem::take(&mut self.out)
    }

    /// Lines for the console (the GUI test reads them); only with synthetic data.
    pub fn take_trace(&mut self) -> Vec<String> {
        core::mem::take(&mut self.trace)
    }

    fn say(&mut self, line: String) {
        if self.synthetic {
            self.trace.push(line);
        }
    }

    // ------------------------------------------------------------ events

    pub fn apply(&mut self, e: Event) {
        match e {
            Event::Hello {
                now,
                utc_offset,
                synthetic,
            } => {
                self.now_base = now;
                self.mono_base = self.mono;
                self.utc_offset = utc_offset;
                self.synthetic = synthetic;
            }
            Event::Auth { state, hint, error } => self.auth_event(state, hint, error),
            Event::ChatsRows { first, total, rows } => {
                let old = core::mem::take(&mut self.chats);
                self.note_changes(&old, &rows);
                self.total = total;
                self.first = first;
                self.chats = rows;
                let titles: Vec<String> = self
                    .chats
                    .iter()
                    .take(3)
                    .map(|c| format!(" \"{}\"", c.title))
                    .collect();
                self.say(format!(
                    "tg: chats {total} from {first}:{}",
                    titles.concat()
                ));
                self.after_list_change();
            }
            Event::ChatsSplice {
                at,
                remove,
                rows,
                total,
            } => {
                self.total = total;
                let Some(i) = at.checked_sub(self.first).map(|i| i as usize) else {
                    return;
                };
                let end = i + remove as usize;
                if end > self.chats.len() {
                    return;
                }
                // The keyboard's row stays on its chat when that chat moved.
                let on = self
                    .cursor
                    .and_then(|c| self.row_at(c))
                    .map(|r| r.id);
                let old: Vec<ChatRow> = self.chats.splice(i..end, rows.clone()).collect();
                self.note_changes(&old, &rows);
                if let Some(id) = on
                    && let Some(p) = self.chats.iter().position(|r| r.id == id)
                {
                    self.cursor = Some(self.first + p as u32);
                }
                self.after_list_change();
            }
            Event::MessagesReset {
                chat,
                title,
                subtitle,
                kind,
                rows,
                more,
            } => {
                if self.open != Some(chat) {
                    return;
                }
                let last = rows.last().map(|r| one_line(&r.text.text)).unwrap_or_default();
                self.say(format!(
                    "tg: opened \"{title}\": {} messages, last: {last}",
                    rows.len()
                ));
                self.title = title;
                self.subtitle = subtitle;
                self.kind = kind;
                self.messages = rows.into_iter().map(|row| self.stamp(row)).collect();
                self.more = more;
                self.asked_older = false;
                self.scroll = 0;
                self.anchor = None;
                self.msg_cursor = None;
                self.msg_anchor = None;
            }
            Event::MessagesPage { chat, rows, more } => {
                if self.open != Some(chat) {
                    return;
                }
                let n = rows.len();
                let mut page: Vec<Msg> = rows.into_iter().map(|row| self.stamp(row)).collect();
                page.append(&mut self.messages);
                self.messages = page;
                self.more = more;
                self.asked_older = false;
                // The cursor and the selection stay on their messages.
                self.msg_cursor = self.msg_cursor.map(|c| c + n);
                self.msg_anchor = self.msg_anchor.map(|c| c + n);
                self.say(format!(
                    "tg: older {n}, {} loaded{}",
                    self.messages.len(),
                    if more { ", more" } else { "" }
                ));
            }
            Event::MessageNew { chat, row } => {
                if self.open != Some(chat) {
                    return;
                }
                self.say(format!(
                    "tg: message \"{}\"{}: {}",
                    self.title,
                    if row.out { " out" } else { "" },
                    one_line(&row.text.text)
                ));
                let m = self.stamp(row);
                self.messages.push(m);
            }
            Event::MessageUpdate { chat, old_id, row } => {
                if self.open != Some(chat) {
                    return;
                }
                let Some(i) = self.messages.iter().position(|m| m.row.id == old_id) else {
                    return;
                };
                if self.messages[i].row.state == SendState::Pending
                    && row.state != SendState::Pending
                {
                    let what = if row.state == SendState::Failed {
                        "not sent"
                    } else {
                        "delivered"
                    };
                    self.say(format!("tg: {what}: {}", one_line(&row.text.text)));
                }
                if self.reply_to == Some(old_id) {
                    self.reply_to = Some(row.id);
                }
                self.messages[i] = self.stamp(row);
            }
            Event::MessageDelete { chat, ids } => {
                if self.open != Some(chat) {
                    return;
                }
                self.messages.retain(|m| !ids.contains(&m.row.id));
                self.msg_cursor = None;
                self.msg_anchor = None;
            }
            Event::Summary { chat, state, text } => {
                if self.open != Some(chat) {
                    return;
                }
                let Some(s) = self.summary.as_mut() else {
                    return;
                };
                s.state = state;
                s.text.push_str(&text);
                let (scope, n) = (s.scope.word(), s.text.chars().count());
                let state = match state {
                    SummaryState::Running => "running",
                    SummaryState::Done => "done",
                    SummaryState::Failed => "failed",
                };
                self.say(format!("tg: summary {scope} {state}: {n} characters"));
            }
            Event::Settings { items } => {
                for (old, new) in self.settings.iter().zip(&items) {
                    if old.key == new.key && old.on != new.on {
                        let line =
                            format!("tg: setting {} {}", new.key, if new.on { "on" } else { "off" });
                        if self.synthetic {
                            self.trace.push(line);
                        }
                    }
                }
                if self.settings.len() != items.len() {
                    self.tree += 1;
                }
                self.settings = items;
            }
        }
    }

    fn stamp(&mut self, row: MessageRow) -> Msg {
        self.rev += 1;
        Msg { row, rev: self.rev }
    }

    fn auth_event(&mut self, state: AuthState, hint: String, error: String) {
        let screen = match state {
            AuthState::Starting => Screen::Starting,
            AuthState::Ready if self.screen == Screen::Settings => Screen::Settings,
            AuthState::Ready => Screen::Main,
            _ => Screen::Login,
        };
        if !error.is_empty() && state != AuthState::Starting {
            self.say(format!("tg: login refused: {error}"));
        }
        if state == AuthState::Starting && !hint.is_empty() && hint != self.auth_hint {
            self.say(format!("tg: waiting: {hint}"));
        }
        if state != self.auth || screen != self.screen {
            self.tree += 1;
            self.login.clear();
            match state {
                AuthState::WaitPhone => self.say(String::from("tg: login phone")),
                AuthState::WaitCode => self.say(String::from("tg: login code")),
                AuthState::WaitPassword => self.say(String::from("tg: login password")),
                AuthState::Ready => self.say(String::from("tg: ready")),
                AuthState::Starting => {}
            }
        }
        if state != AuthState::Ready && self.auth == AuthState::Ready {
            // Logged out: nothing of the account stays in the window.
            self.forget();
        }
        self.auth = state;
        self.screen = screen;
        self.auth_hint = hint;
        self.auth_error = error;
        match screen {
            Screen::Login => self.focus = Some(Role::LoginField),
            Screen::Main => {
                self.focus = Some(Role::List);
                self.sync_window();
            }
            _ => {}
        }
    }

    fn forget(&mut self) {
        self.total = 0;
        self.first = 0;
        self.chats.clear();
        self.top = 0;
        self.cursor = None;
        self.query.clear();
        self.asked = None;
        self.open = None;
        self.messages.clear();
        self.title.clear();
        self.subtitle.clear();
        self.composer.clear();
        self.reply_to = None;
        self.summary = None;
        self.settings.clear();
    }

    /// Says on the console which chats' unread counts or last lines changed.
    fn note_changes(&mut self, old: &[ChatRow], new: &[ChatRow]) {
        if !self.synthetic {
            return;
        }
        for n in new {
            let before = old
                .iter()
                .chain(self.chats.iter())
                .find(|o| o.id == n.id);
            if let Some(o) = before
                && (o.unread != n.unread || o.last != n.last)
            {
                self.trace.push(format!(
                    "tg: chat \"{}\" unread {}: {}",
                    n.title, n.unread, n.last
                ));
            }
        }
    }

    // ------------------------------------------------------------ login

    pub fn login_submit(&mut self) {
        let value = self.login.text.trim();
        if value.is_empty() {
            return;
        }
        let value = String::from(value);
        let r = match self.auth {
            AuthState::WaitPhone => Request::AuthPhone(value),
            AuthState::WaitCode => Request::AuthCode(value),
            AuthState::WaitPassword => Request::AuthPassword(value),
            _ => return,
        };
        self.out.push(r);
        // The value typed is not kept: a refusal asks again with an empty field.
        self.login.clear();
    }

    // ------------------------------------------------------------ the chat list

    /// The row at absolute index `i`, if it is held.
    pub fn row_at(&self, i: u32) -> Option<&ChatRow> {
        self.chats.get(i.checked_sub(self.first)? as usize)
    }

    fn last_top(&self) -> u32 {
        self.total.saturating_sub(self.list_rows)
    }

    fn after_list_change(&mut self) {
        let top = self.top.min(self.last_top());
        if top != self.top {
            self.top = top;
            self.sync_window();
        }
        self.cursor = match self.total {
            0 => None,
            n => self.cursor.map(|c| c.min(n - 1)),
        };
    }

    /// Asks for the rows shown (and one more, the row partly shown at the bottom), if
    /// they are not what was asked last.
    fn sync_window(&mut self) {
        if !matches!(self.screen, Screen::Main) {
            return;
        }
        let want = (self.top, self.list_rows + 1, self.query.text.clone());
        if self.asked.as_ref() == Some(&want) {
            return;
        }
        self.out.push(Request::ChatsWindow {
            first: want.0,
            count: want.1,
            query: want.2.clone(),
        });
        self.asked = Some(want);
    }

    /// The view says how many whole rows its list shows.
    pub fn set_list_rows(&mut self, rows: u32) {
        let rows = rows.max(1);
        if rows == self.list_rows && self.asked.is_some() {
            return;
        }
        self.list_rows = rows;
        self.top = self.top.min(self.last_top());
        self.sync_window();
    }

    /// Whether the list can scroll further down (or up).
    pub fn list_can_scroll(&self, down: bool) -> bool {
        if down {
            self.top < self.last_top()
        } else {
            self.top > 0
        }
    }

    /// Scrolls the list by rows; whether it moved.
    pub fn list_scroll(&mut self, rows: i32) -> bool {
        let to = if rows < 0 {
            self.top.saturating_sub(rows.unsigned_abs())
        } else {
            self.top.saturating_add(rows as u32)
        }
        .min(self.last_top());
        let moved = to != self.top;
        self.top = to;
        self.sync_window();
        moved
    }

    /// Moves the keyboard's row to `to` and brings it into view.
    pub fn list_cursor(&mut self, to: u32) {
        if self.total == 0 {
            self.cursor = None;
            return;
        }
        let to = to.min(self.total - 1);
        self.cursor = Some(to);
        if to < self.top {
            self.top = to;
        } else if to >= self.top + self.list_rows {
            self.top = to + 1 - self.list_rows;
        }
        self.sync_window();
    }

    /// The keyboard's row moved by `delta` rows.
    pub fn list_move(&mut self, delta: i32) {
        let from = self.cursor.unwrap_or(self.top);
        let to = match self.cursor {
            None => from,
            Some(_) if delta < 0 => from.saturating_sub(delta.unsigned_abs()),
            Some(_) => from.saturating_add(delta as u32),
        };
        self.list_cursor(to);
    }

    /// Opens the chat at absolute index `i`.
    pub fn open_at(&mut self, i: u32) {
        let Some(row) = self.row_at(i) else { return };
        let (id, title, kind) = (row.id, row.title.clone(), row.kind);
        self.cursor = Some(i);
        if self.open == Some(id) {
            self.focus = Some(Role::Composer);
            return;
        }
        self.out.push(Request::ChatOpen {
            chat: id,
            count: PAGE,
        });
        self.open = Some(id);
        self.title = title;
        self.subtitle.clear();
        self.kind = kind;
        self.messages.clear();
        self.more = false;
        self.asked_older = false;
        self.scroll = 0;
        self.anchor = None;
        self.msg_cursor = None;
        self.msg_anchor = None;
        self.composer.clear();
        self.reply_to = None;
        self.summary = None;
        self.tree += 1;
        self.focus = Some(Role::Composer);
    }

    fn close_chat(&mut self) {
        if self.open.take().is_some() {
            self.out.push(Request::ChatClose);
            self.messages.clear();
            self.summary = None;
            self.reply_to = None;
            self.tree += 1;
            self.focus = Some(Role::List);
        }
    }

    /// The search field changed.
    pub fn query_changed(&mut self) {
        self.top = 0;
        self.cursor = None;
        let line = format!("tg: search \"{}\"", self.query.text);
        self.say(line);
        self.sync_window();
    }

    // ------------------------------------------------------------ messages

    /// The view is near the oldest message loaded: ask for the page before it, once.
    pub fn want_older(&mut self) {
        if !self.more || self.asked_older {
            return;
        }
        let (Some(chat), Some(first)) = (self.open, self.messages.first()) else {
            return;
        };
        self.out.push(Request::MessagesOlder {
            chat,
            before: first.row.id,
            count: PAGE,
        });
        self.asked_older = true;
    }

    /// Sends what the composer holds.
    pub fn send(&mut self) {
        let Some(chat) = self.open else { return };
        let text: String = self.composer.text.trim().chars().take(MAX_MESSAGE).collect();
        if text.is_empty() {
            return;
        }
        self.out.push(Request::MessagesSend {
            chat,
            text,
            reply_to: self.reply_to.take(),
        });
        self.composer.clear();
        self.scroll = 0;
        self.msg_cursor = None;
        self.msg_anchor = None;
    }

    /// The keyboard's message moved by `delta` (with `extend`, the selection grows).
    pub fn msg_move(&mut self, delta: i32, extend: bool) {
        let n = self.messages.len();
        if n == 0 {
            return;
        }
        let to = match self.msg_cursor {
            None => n - 1,
            Some(c) if delta < 0 => c.saturating_sub(delta.unsigned_abs() as usize),
            Some(c) => (c + delta as usize).min(n - 1),
        };
        self.msg_select(to, extend);
        self.reveal = true;
    }

    /// The message at `i` chosen by a press (with `extend`, from the anchor to it).
    pub fn msg_select(&mut self, i: usize, extend: bool) {
        if i >= self.messages.len() {
            return;
        }
        if !extend || self.msg_anchor.is_none() {
            self.msg_anchor = Some(i);
        }
        self.msg_cursor = Some(i);
    }

    /// The selected messages' indices, in order.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let (a, c) = (self.msg_anchor?, self.msg_cursor?);
        Some((a.min(c), a.max(c)))
    }

    /// The selected messages as text, for the clipboard: one alone as it is, several
    /// each with its sender.
    pub fn selected_text(&self) -> String {
        let Some((a, b)) = self.selection() else {
            return String::new();
        };
        let body = |m: &MessageRow| -> String {
            match m.kind {
                MessageKind::Text | MessageKind::Service => m.text.text.clone(),
                _ if m.text.text.is_empty() => format!("[{}]", kind_word(m.kind)),
                _ => format!("[{}] {}", kind_word(m.kind), m.text.text),
            }
        };
        if a == b {
            return body(&self.messages[a].row);
        }
        let mut out = String::new();
        for m in &self.messages[a..=b] {
            let who = if m.row.out { "You" } else { m.row.sender.as_str() };
            if !out.is_empty() {
                out.push('\n');
            }
            if m.row.kind == MessageKind::Service || who.is_empty() {
                out.push_str(&body(&m.row));
            } else {
                out.push_str(&format!("{who}: {}", body(&m.row)));
            }
        }
        out
    }

    /// Enter on the keyboard's message: a failed one of ours is sent again (protocol 3),
    /// any other is answered.
    pub fn activate_cursor(&mut self) {
        if !self.retry_cursor() {
            self.reply_to_cursor();
        }
    }

    /// Sends the keyboard's message again if it is ours and its send failed.
    pub fn retry_cursor(&mut self) -> bool {
        let (Some(chat), Some(m)) = (self.open, self.msg_cursor.and_then(|c| self.messages.get(c)))
        else {
            return false;
        };
        if !m.row.out || m.row.state != SendState::Failed {
            return false;
        }
        let id = m.row.id;
        self.say(format!("tg: retry: {}", one_line(&m.row.text.text)));
        self.out.push(Request::MessagesRetry { chat, id });
        true
    }

    /// Answers the keyboard's message: the composer names it.
    pub fn reply_to_cursor(&mut self) {
        let Some(m) = self.msg_cursor.and_then(|c| self.messages.get(c)) else {
            return;
        };
        if m.row.kind == MessageKind::Service {
            return;
        }
        self.reply_to = Some(m.row.id);
        self.focus = Some(Role::Composer);
    }

    /// The message being answered.
    pub fn reply_row(&self) -> Option<&MessageRow> {
        let id = self.reply_to?;
        self.messages.iter().map(|m| &m.row).find(|r| r.id == id)
    }

    // ------------------------------------------------------------ the summary sheet

    pub fn summarize(&mut self, scope: Scope) {
        let Some(chat) = self.open else { return };
        if self.summary.is_none() {
            self.tree += 1;
        }
        self.summary = Some(Summary {
            scope,
            state: SummaryState::Running,
            text: String::new(),
        });
        self.out.push(Request::Summarize { chat, scope });
        self.focus = Some(Role::Card(3));
    }

    pub fn close_summary(&mut self) {
        if let Some(s) = self.summary.take() {
            if s.state == SummaryState::Running {
                self.out.push(Request::SummaryCancel);
            }
            self.tree += 1;
            self.focus = Some(Role::Composer);
        }
    }

    // ------------------------------------------------------------ settings

    pub fn show_settings(&mut self) {
        if self.screen == Screen::Main {
            self.screen = Screen::Settings;
            self.tree += 1;
            self.focus = Some(Role::Setting(0));
            self.say(String::from("tg: settings"));
        }
    }

    pub fn leave_settings(&mut self) {
        if self.screen == Screen::Settings {
            self.screen = Screen::Main;
            self.tree += 1;
            self.focus = Some(Role::List);
            self.asked = None;
            self.sync_window();
        }
    }

    pub fn toggle_setting(&mut self, i: usize) {
        if let Some(s) = self.settings.get(i) {
            self.out.push(Request::SettingsSet {
                key: s.key.clone(),
                on: !s.on,
            });
        }
    }

    pub fn log_out(&mut self) {
        self.out.push(Request::Logout);
    }

    // ------------------------------------------------------------ keys every pane shares

    /// Escape: the nearest thing open closes. Whether anything did.
    pub fn escape(&mut self) -> bool {
        if self.screen == Screen::Settings {
            self.leave_settings();
        } else if self.summary.is_some() {
            self.close_summary();
        } else if self.reply_to.is_some() {
            self.reply_to = None;
        } else if self.msg_cursor.is_some() {
            self.msg_cursor = None;
            self.msg_anchor = None;
        } else if !self.query.text.is_empty() {
            self.query.clear();
            self.query_changed();
        } else if self.narrow && self.open.is_some() {
            self.close_chat();
        } else {
            return false;
        }
        true
    }
}

/// A placeholder's name.
pub fn kind_word(k: MessageKind) -> &'static str {
    match k {
        MessageKind::Text => "Text",
        MessageKind::Service => "Service",
        MessageKind::Photo => "Photo",
        MessageKind::Sticker => "Sticker",
        MessageKind::Voice => "Voice message",
        MessageKind::File => "File",
    }
}
