//! The protocol on the wire (docs/tg-window.md, "The protocol"): each [`Request`] as one
//! JSON object (`{"op": ...}`), each event line (`{"ev": ...}`) as an [`Event`]. Ids are
//! strings. A line that is not an event the window knows is `None` (the caller counts
//! it); a field missing or of the wrong type takes its empty value, so a newer backend's
//! additions do not break an older window.

use alloc::string::String;
use alloc::vec::Vec;

use crate::json::{self, Value, Writer};
use crate::proto::{
    AuthState, ChatKind, ChatRow, Event, Id, MessageKind, MessageRow, Quote, Request, Rich,
    Scope, SendState, Setting, Span, SpanKind, SummaryState,
};

/// The request as one line of JSON, without its newline.
pub fn encode(r: &Request) -> String {
    let mut w = Writer::new();
    let op = |w: &mut Writer, name: &str| {
        w.raw("{");
        w.key("op", true).str(name);
    };
    let id = |w: &mut Writer, key: &str, v: Id| {
        w.key(key, false).str(&alloc::format!("{v}"));
    };
    match r {
        Request::AuthPhone(v) | Request::AuthCode(v) | Request::AuthPassword(v) => {
            let name = match r {
                Request::AuthPhone(_) => "auth.phone",
                Request::AuthCode(_) => "auth.code",
                _ => "auth.password",
            };
            op(&mut w, name);
            w.key("value", false).str(v);
        }
        Request::Logout => op(&mut w, "auth.logout"),
        Request::ChatsWindow {
            first,
            count,
            query,
        } => {
            op(&mut w, "chats.window");
            w.key("first", false).int(i64::from(*first));
            w.key("count", false).int(i64::from(*count));
            w.key("query", false).str(query);
        }
        Request::ChatOpen { chat, count } => {
            op(&mut w, "chat.open");
            id(&mut w, "chat", *chat);
            w.key("count", false).int(i64::from(*count));
        }
        Request::ChatClose => op(&mut w, "chat.close"),
        Request::MessagesOlder {
            chat,
            before,
            count,
        } => {
            op(&mut w, "messages.older");
            id(&mut w, "chat", *chat);
            id(&mut w, "before", *before);
            w.key("count", false).int(i64::from(*count));
        }
        Request::MessagesSend {
            chat,
            text,
            reply_to,
        } => {
            op(&mut w, "messages.send");
            id(&mut w, "chat", *chat);
            w.key("text", false).str(text);
            match reply_to {
                Some(r) => id(&mut w, "reply_to", *r),
                None => {
                    w.key("reply_to", false).raw("null");
                }
            }
        }
        Request::MessagesRetry { chat, id: m } => {
            op(&mut w, "messages.retry");
            id(&mut w, "chat", *chat);
            id(&mut w, "id", *m);
        }
        Request::Summarize { chat, scope } => {
            op(&mut w, "ai.summarize");
            id(&mut w, "chat", *chat);
            w.key("scope", false).str(scope.word());
        }
        Request::SummaryCancel => op(&mut w, "ai.cancel"),
        Request::SettingsSet { key, on } => {
            op(&mut w, "settings.set");
            w.key("key", false).str(key);
            w.key("on", false).bool(*on);
        }
    }
    w.raw("}");
    w.out
}

/// What the event line `line` says, if it is one the window knows.
pub fn decode(line: &str) -> Option<Event> {
    let v = json::parse(line)?;
    let ev = v.get("ev")?.as_str()?;
    let s = |key: &str| str_of(v.get(key));
    let n = |key: &str| v.get(key).and_then(Value::as_i64).unwrap_or(0);
    let u = |key: &str| u32::try_from(n(key).max(0)).unwrap_or(u32::MAX);
    let b = |key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    let chat = || id_of(v.get("chat"));
    Some(match ev {
        "hello" => Event::Hello {
            now: n("now"),
            utc_offset: i32::try_from(n("utc_offset")).unwrap_or(0),
            synthetic: b("synthetic"),
        },
        "auth.state" => {
            let (state, hint) = match s("state").as_str() {
                "wait_phone" => (AuthState::WaitPhone, s("hint")),
                "wait_code" => (AuthState::WaitCode, s("hint")),
                "wait_password" => (AuthState::WaitPassword, s("hint")),
                "ready" => (AuthState::Ready, s("hint")),
                // A step the window has no form for: it says so and waits.
                "wait_email" | "wait_email_code" => (
                    AuthState::Starting,
                    String::from("Telegram asks for an e-mail login, which the window does not do yet"),
                ),
                "wait_other_device" => (
                    AuthState::Starting,
                    String::from("Confirm the login on another device"),
                ),
                // The backend's words (a logout, TDLib's refusal), else the log.
                "failed" if !s("error").is_empty() => (AuthState::Starting, s("error")),
                "failed" => (AuthState::Starting, String::from("The login failed; see tgd's log")),
                _ => (AuthState::Starting, s("hint")),
            };
            Event::Auth {
                state,
                hint,
                error: s("error"),
            }
        }
        "chats.rows" => Event::ChatsRows {
            first: u("first"),
            total: u("total"),
            rows: rows_of(v.get("rows"), chat_row),
        },
        "chats.splice" => Event::ChatsSplice {
            at: u("at"),
            remove: u("remove"),
            rows: rows_of(v.get("rows"), chat_row),
            total: u("total"),
        },
        "messages.reset" => Event::MessagesReset {
            chat: chat()?,
            title: s("title"),
            subtitle: s("subtitle"),
            kind: chat_kind(&s("kind")),
            rows: rows_of(v.get("rows"), message_row),
            more: b("more"),
        },
        "messages.page" => Event::MessagesPage {
            chat: chat()?,
            rows: rows_of(v.get("rows"), message_row),
            more: b("more"),
        },
        "message.new" => Event::MessageNew {
            chat: chat()?,
            row: message_row(v.get("row")?)?,
        },
        "message.update" => Event::MessageUpdate {
            chat: chat()?,
            old_id: id_of(v.get("old_id"))?,
            row: message_row(v.get("row")?)?,
        },
        "message.delete" => Event::MessageDelete {
            chat: chat()?,
            ids: v
                .get("ids")
                .and_then(Value::as_array)
                .unwrap_or(&[])
                .iter()
                .filter_map(|i| id_of(Some(i)))
                .collect(),
        },
        "ai.summary" => Event::Summary {
            chat: chat()?,
            state: match s("state").as_str() {
                "running" => SummaryState::Running,
                "done" => SummaryState::Done,
                _ => SummaryState::Failed,
            },
            text: s("text"),
        },
        "settings" => Event::Settings {
            items: rows_of(v.get("items"), |i| {
                Some(Setting {
                    key: str_of(i.get("key")),
                    label: str_of(i.get("label")),
                    on: i.get("on").and_then(Value::as_bool).unwrap_or(false),
                })
            }),
        },
        _ => return None,
    })
}

fn str_of(v: Option<&Value>) -> String {
    v.and_then(Value::as_str).map(String::from).unwrap_or_default()
}

/// An id: a string of digits (a number is taken too).
fn id_of(v: Option<&Value>) -> Option<Id> {
    match v? {
        Value::Str(s) => s.parse().ok(),
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

fn rows_of<T>(v: Option<&Value>, f: impl Fn(&Value) -> Option<T>) -> Vec<T> {
    v.and_then(Value::as_array)
        .unwrap_or(&[])
        .iter()
        .filter_map(f)
        .collect()
}

fn chat_kind(s: &str) -> ChatKind {
    match s {
        "group" => ChatKind::Group,
        "channel" => ChatKind::Channel,
        "saved" => ChatKind::Saved,
        _ => ChatKind::Private,
    }
}

fn chat_row(v: &Value) -> Option<ChatRow> {
    Some(ChatRow {
        id: id_of(v.get("id"))?,
        title: str_of(v.get("title")),
        kind: chat_kind(v.get("kind").and_then(Value::as_str).unwrap_or("")),
        last: str_of(v.get("last")),
        date: v.get("date").and_then(Value::as_i64).unwrap_or(0),
        unread: u32::try_from(v.get("unread").and_then(Value::as_i64).unwrap_or(0).max(0))
            .unwrap_or(u32::MAX),
        pinned: v.get("pinned").and_then(Value::as_bool).unwrap_or(false),
        muted: v.get("muted").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// `{"text", "spans": [[start, end, kind], ...]}`; spans outside the text or reversed are
/// dropped, so the view never cuts a string where it cannot.
fn rich(v: Option<&Value>) -> Rich {
    let Some(v) = v else {
        return Rich::default();
    };
    let text = str_of(v.get("text"));
    let len = text.chars().count() as i64;
    let spans = rows_of(v.get("spans"), |s| {
        let a = s.as_array()?;
        let (start, end) = (a.first()?.as_i64()?, a.get(1)?.as_i64()?);
        if start < 0 || end <= start || end > len {
            return None;
        }
        let kind = match a.get(2)?.as_str()? {
            "bold" => SpanKind::Bold,
            "italic" => SpanKind::Italic,
            "code" => SpanKind::Code,
            "link" => SpanKind::Link,
            "mention" => SpanKind::Mention,
            _ => return None,
        };
        Some(Span {
            start: start as u32,
            end: end as u32,
            kind,
        })
    });
    Rich { text, spans }
}

fn message_row(v: &Value) -> Option<MessageRow> {
    let s = |key: &str| str_of(v.get(key));
    let b = |key: &str| v.get(key).and_then(Value::as_bool).unwrap_or(false);
    Some(MessageRow {
        id: id_of(v.get("id"))?,
        date: v.get("date").and_then(Value::as_i64).unwrap_or(0),
        out: b("out"),
        sender: s("sender"),
        kind: match s("kind").as_str() {
            "service" => MessageKind::Service,
            "photo" => MessageKind::Photo,
            "sticker" => MessageKind::Sticker,
            "voice" => MessageKind::Voice,
            "file" => MessageKind::File,
            _ => MessageKind::Text,
        },
        text: rich(v.get("text")),
        detail: s("detail"),
        reply: match v.get("reply") {
            Some(q @ Value::Object(_)) => Some(Quote {
                sender: str_of(q.get("sender")),
                text: str_of(q.get("text")),
            }),
            _ => None,
        },
        forward: v.get("forward").and_then(Value::as_str).map(String::from),
        edited: b("edited"),
        state: match s("state").as_str() {
            "pending" => SendState::Pending,
            "read" => SendState::Read,
            "failed" => SendState::Failed,
            _ => SendState::Sent,
        },
    })
}

/// A request as the backend reads it (the inverse of [`encode`]), for the tests and for
/// a backend written in Rust.
pub fn decode_request(line: &str) -> Option<Request> {
    let v = json::parse(line)?;
    let s = |key: &str| str_of(v.get(key));
    let n = |key: &str| u32::try_from(v.get(key).and_then(Value::as_i64)?).ok();
    let id = |key: &str| id_of(v.get(key));
    Some(match v.get("op")?.as_str()? {
        "auth.phone" => Request::AuthPhone(s("value")),
        "auth.code" => Request::AuthCode(s("value")),
        "auth.password" => Request::AuthPassword(s("value")),
        "auth.logout" => Request::Logout,
        "chats.window" => Request::ChatsWindow {
            first: n("first")?,
            count: n("count")?,
            query: s("query"),
        },
        "chat.open" => Request::ChatOpen {
            chat: id("chat")?,
            count: n("count")?,
        },
        "chat.close" => Request::ChatClose,
        "messages.older" => Request::MessagesOlder {
            chat: id("chat")?,
            before: id("before")?,
            count: n("count")?,
        },
        "messages.send" => Request::MessagesSend {
            chat: id("chat")?,
            text: s("text"),
            reply_to: id("reply_to"),
        },
        "messages.retry" => Request::MessagesRetry {
            chat: id("chat")?,
            id: id("id")?,
        },
        "ai.summarize" => Request::Summarize {
            chat: id("chat")?,
            scope: match s("scope").as_str() {
                "unread" => Scope::Unread,
                "week" => Scope::Week,
                _ => Scope::Day,
            },
        },
        "ai.cancel" => Request::SummaryCancel,
        "settings.set" => Request::SettingsSet {
            key: s("key"),
            on: v.get("on").and_then(Value::as_bool).unwrap_or(false),
        },
        _ => return None,
    })
}
