//! Host tests of protocol 3 on the wire: JSON (what it takes and refuses, never a panic),
//! every request written and read back, the events as `tgd` writes them, the socket
//! backend over a fake transport (lines across packets, several in one, a lost and a new
//! connection, a full socket, an over-long line), and the failed send with its retry
//! through the sample backend and the keys.

use std::collections::VecDeque;
use std::rc::Rc;
use std::cell::RefCell;
use std::string::{String, ToString};
use std::vec::Vec;
use std::vec;

use crate::json::{self, Value};
use crate::model::Model;
use crate::proto::{
    AuthState, Backend, ChatKind, Event, MessageKind, Request, Scope, SendState, SpanKind,
    SummaryState,
};
use crate::sample::{self, SampleBackend};
use crate::socket::{Down, PACKET, RETRY_MS, SocketBackend, Transport};
use crate::wire;

#[test]
fn json_takes_what_rfc_8259_allows() {
    let v = json::parse(r#" {"a": [1, -2, 3.5, 1e3, true, false, null], "b": "x\"\\\/\n\u00e9\ud83d\ude00", "c": {}} "#)
        .expect("JSON");
    let a = v.get("a").and_then(Value::as_array).expect("array");
    assert_eq!(a[0], Value::Int(1));
    assert_eq!(a[1], Value::Int(-2));
    assert_eq!(a[2], Value::Float(3.5));
    assert_eq!(a[3], Value::Float(1000.0));
    assert_eq!(a[6], Value::Null);
    assert_eq!(v.get("b").and_then(Value::as_str), Some("x\"\\/\né😀"));
    assert_eq!(v.get("c"), Some(&Value::Object(Vec::new())));
    // An id beyond 2^53 stays exact as an integer.
    assert_eq!(json::parse("9007199254740993"), Some(Value::Int(9_007_199_254_740_993)));
}

#[test]
fn json_refuses_what_is_not_json_and_never_panics() {
    for bad in [
        "", "{", "}", "[1,]", "{\"a\":}", "{a:1}", "01", "1.", "-", "1e", "\"\x01\"", "\"\\x\"",
        "\"\\ud83d\"", "\"\\ud83dx\"", "tru", "[1 2]", "{\"a\" 1}", "1 2", "\"abc",
    ] {
        assert_eq!(json::parse(bad), None, "{bad:?}");
    }
    let deep = "[".repeat(json::MAX_DEPTH + 2) + &"]".repeat(json::MAX_DEPTH + 2);
    assert_eq!(json::parse(&deep), None);
    let fine = "[".repeat(json::MAX_DEPTH) + &"]".repeat(json::MAX_DEPTH);
    assert!(json::parse(&fine).is_some());
    // Random bytes and cut lines: an answer, never a panic.
    let mut x: u32 = 0x1234_5678;
    let pieces = ["{", "}", "[", "]", "\"", "\\", "u", ":", ",", "1", "-", ".", "e", "n", "ü", " "];
    for _ in 0..20_000 {
        let mut s = String::new();
        for _ in 0..(x % 24) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            s.push_str(pieces[(x % pieces.len() as u32) as usize]);
        }
        let _ = json::parse(&s);
        let _ = wire::decode(&s);
    }
}

#[test]
fn every_request_is_written_and_read_back() {
    let all = vec![
        Request::AuthPhone("+380 00 000 0000".to_string()),
        Request::AuthCode("13579".to_string()),
        Request::AuthPassword("пароль \"з\" лапками\n".to_string()),
        Request::Logout,
        Request::ChatsWindow {
            first: 20,
            count: 15,
            query: "Ан".to_string(),
        },
        Request::ChatOpen {
            chat: -1_001_234_567_890,
            count: 40,
        },
        Request::ChatClose,
        Request::MessagesOlder {
            chat: 10,
            before: 1_048_576,
            count: 40,
        },
        Request::MessagesSend {
            chat: 10,
            text: "Привіт\tсвіт 😀".to_string(),
            reply_to: Some(7),
        },
        Request::MessagesSend {
            chat: 10,
            text: String::new(),
            reply_to: None,
        },
        Request::MessagesRetry {
            chat: 10,
            id: 1_099_511_627_777,
        },
        Request::Summarize {
            chat: 11,
            scope: Scope::Week,
        },
        Request::SummaryCancel,
        Request::SettingsSet {
            key: "ai.chat".to_string(),
            on: true,
        },
    ];
    for r in all {
        let line = wire::encode(&r);
        assert!(!line.contains('\n'), "{line}");
        assert_eq!(wire::decode_request(&line), Some(r.clone()), "{line}");
    }
    // Ids are strings on the wire.
    assert_eq!(
        wire::encode(&Request::MessagesRetry { chat: 10, id: 5 }),
        r#"{"op":"messages.retry","chat":"10","id":"5"}"#
    );
}

#[test]
fn events_are_read_as_tgd_writes_them() {
    let e = wire::decode(
        r#"{"ev": "messages.reset", "chat": "-100", "title": "Дмитро", "subtitle": "", "kind": "group", "more": true, "rows": [{"id": "5", "date": 1763500000, "out": false, "sender": "Анна", "kind": "text", "text": {"text": "жирний і посилання", "spans": [[0, 6, "bold"], [9, 18, "link"], [3, 99, "bold"], [5, 2, "italic"], [0, 1, "strike"]]}, "detail": "", "reply": {"sender": "Mark", "text": "?"}, "forward": null, "edited": true, "state": "sent"}, {"id": "6", "date": 1, "out": true, "sender": "", "kind": "voice", "text": {"text": "", "spans": []}, "detail": "0:12", "reply": null, "forward": "Olena", "edited": false, "state": "failed"}, {"id": "x"}]}"#,
    );
    let Some(Event::MessagesReset {
        chat,
        title,
        kind,
        rows,
        more,
        ..
    }) = e
    else {
        panic!("{e:?}");
    };
    assert_eq!((chat, title.as_str(), kind, more), (-100, "Дмитро", ChatKind::Group, true));
    assert_eq!(rows.len(), 2, "a row without a numeric id is dropped");
    // Spans past the text, reversed, or of an unknown kind are dropped.
    let spans: Vec<_> = rows[0].text.spans.iter().map(|s| (s.start, s.end, s.kind)).collect();
    assert_eq!(spans, vec![(0, 6, SpanKind::Bold), (9, 18, SpanKind::Link)]);
    assert_eq!(rows[0].reply.as_ref().map(|q| q.sender.as_str()), Some("Mark"));
    assert!(rows[0].edited);
    assert_eq!(rows[1].kind, MessageKind::Voice);
    assert_eq!(rows[1].forward.as_deref(), Some("Olena"));
    assert_eq!(rows[1].state, SendState::Failed);

    assert_eq!(
        wire::decode(r#"{"ev":"auth.state","state":"wait_code","hint":"SMS","error":"PHONE_CODE_INVALID"}"#),
        Some(Event::Auth {
            state: AuthState::WaitCode,
            hint: "SMS".to_string(),
            error: "PHONE_CODE_INVALID".to_string()
        })
    );
    // A step the window has no form for waits on the starting screen with words.
    let Some(Event::Auth { state, hint, .. }) =
        wire::decode(r#"{"ev":"auth.state","state":"wait_other_device","hint":"","error":""}"#)
    else {
        panic!()
    };
    assert_eq!(state, AuthState::Starting);
    assert!(hint.contains("another device"));
    assert_eq!(
        wire::decode(r#"{"ev":"chats.splice","at":3,"remove":1,"total":9,"rows":[{"id":"15","title":"Saved Messages","kind":"saved","last":"","date":0,"unread":0,"pinned":false,"muted":true}]}"#)
            .map(|e| match e {
                Event::ChatsSplice { at, remove, rows, total } => (at, remove, rows[0].kind, rows[0].muted, total),
                _ => panic!(),
            }),
        Some((3, 1, ChatKind::Saved, true, 9))
    );
    assert_eq!(
        wire::decode(r#"{"ev":"message.delete","chat":"10","ids":["1","2"]}"#),
        Some(Event::MessageDelete { chat: 10, ids: vec![1, 2] })
    );
    assert_eq!(
        wire::decode(r#"{"ev":"ai.summary","chat":"10","state":"running","text":"…"}"#),
        Some(Event::Summary { chat: 10, state: SummaryState::Running, text: "…".to_string() })
    );
    assert!(matches!(
        wire::decode(r#"{"ev":"hello","protocol":3,"now":1763500000,"utc_offset":7200,"synthetic":true}"#),
        Some(Event::Hello { now: 1_763_500_000, utc_offset: 7200, synthetic: true })
    ));
    // What the window does not know: a reply of protocol 1, a refusal, a newer event.
    for line in [r#"{"re":"x","ok":true}"#, r#"{"ev":"refused","op":"x","why":"y"}"#, r#"{"ev":"chat.typing"}"#] {
        assert_eq!(wire::decode(line), None);
    }
}

/// A socket as a list of packets each way, with switches for a lost connection and a
/// full socket.
#[derive(Default)]
struct Fake {
    up: bool,
    refuse: Option<String>,
    full: bool,
    lost: bool,
    connects: u32,
    sent: Vec<Vec<u8>>,
    incoming: VecDeque<Vec<u8>>,
}

#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Fake>>);

impl Transport for Shared {
    fn connect(&mut self) -> Result<(), Down> {
        let mut f = self.0.borrow_mut();
        if let Some(why) = &f.refuse {
            return Err(Down(why.clone()));
        }
        f.up = true;
        f.lost = false;
        f.connects += 1;
        f.incoming.clear(); // what the old connection held went with it
        Ok(())
    }
    fn send(&mut self, packet: &[u8]) -> Result<bool, Down> {
        let mut f = self.0.borrow_mut();
        assert!(f.up && packet.len() <= PACKET);
        if f.lost {
            return Err(Down("tgd went away".to_string()));
        }
        if f.full {
            return Ok(false);
        }
        f.sent.push(packet.to_vec());
        Ok(true)
    }
    fn recv(&mut self, into: &mut Vec<u8>) -> Result<bool, Down> {
        let mut f = self.0.borrow_mut();
        if f.lost {
            return Err(Down("tgd went away".to_string()));
        }
        match f.incoming.pop_front() {
            Some(p) => {
                into.extend_from_slice(&p);
                Ok(true)
            }
            None => Ok(false),
        }
    }
    fn close(&mut self) {
        self.0.borrow_mut().up = false;
    }
}

fn drain<T: Transport>(b: &mut SocketBackend<T>, now: u64) -> Vec<Event> {
    let mut out = Vec::new();
    while let Some(e) = b.poll(now) {
        out.push(e);
    }
    out
}

#[test]
fn lines_cross_packets_and_packets_hold_several_lines() {
    let fake = Shared::default();
    let mut b = SocketBackend::new(fake.clone());
    assert!(drain(&mut b, 0).is_empty());
    assert!(b.is_up());
    // A page of long messages: one line of about 3 packets, cut anywhere (even inside a
    // character), then two short lines in one packet.
    let long_text = "Довге повідомлення. ".repeat(9000);
    let line = format!(
        r#"{{"ev":"message.new","chat":"1","row":{{"id":"2","date":0,"out":false,"sender":"","kind":"text","text":{{"text":"{long_text}","spans":[]}},"detail":"","reply":null,"forward":null,"edited":false,"state":"sent"}}}}"#
    ) + "\n";
    let bytes = line.as_bytes();
    assert!(bytes.len() > 2 * PACKET);
    for piece in bytes.chunks(PACKET - 7) {
        fake.0.borrow_mut().incoming.push_back(piece.to_vec());
    }
    fake.0.borrow_mut().incoming.push_back(
        b"{\"ev\":\"chat.close\"}\n{\"ev\":\"message.delete\",\"chat\":\"1\",\"ids\":[\"2\"]}\n{\"ev\":\"message.del".to_vec(),
    );
    fake.0.borrow_mut().incoming.push_back(b"ete\",\"chat\":\"1\",\"ids\":[\"3\"]}\n".to_vec());
    let got = drain(&mut b, 10);
    assert_eq!(got.len(), 3, "{got:?}");
    match &got[0] {
        Event::MessageNew { row, .. } => assert_eq!(row.text.text, long_text),
        e => panic!("{e:?}"),
    }
    assert_eq!(got[1], Event::MessageDelete { chat: 1, ids: vec![2] });
    assert_eq!(got[2], Event::MessageDelete { chat: 1, ids: vec![3] });
    assert_eq!(b.ignored, 1, "the unknown event");

    // A long request goes out in packets that join to its line.
    let text = "ї".repeat(PACKET);
    b.send(Request::MessagesSend { chat: 1, text: text.clone(), reply_to: None });
    let sent: Vec<u8> = fake.0.borrow().sent.concat();
    assert!(fake.0.borrow().sent.len() >= 3);
    let sent = String::from_utf8(sent).expect("UTF-8 when joined");
    assert!(sent.ends_with('\n'));
    assert_eq!(
        wire::decode_request(sent.trim_end()),
        Some(Request::MessagesSend { chat: 1, text, reply_to: None })
    );
}

#[test]
fn a_lost_tgd_is_said_once_and_found_again() {
    let fake = Shared::default();
    fake.0.borrow_mut().refuse = Some("Start tgd: tgd --window".to_string());
    let mut b = SocketBackend::new(fake.clone());
    let got = drain(&mut b, 0);
    assert_eq!(
        got,
        vec![Event::Auth {
            state: AuthState::Starting,
            hint: "Start tgd: tgd --window".to_string(),
            error: String::new()
        }]
    );
    assert_eq!(b.next_wake(), Some(RETRY_MS));
    // Requests while down are dropped: the window asks again once tgd is ready.
    b.send(Request::ChatClose);
    assert!(drain(&mut b, RETRY_MS - 1).is_empty());
    assert!(drain(&mut b, RETRY_MS).is_empty(), "the same words are not said again");
    fake.0.borrow_mut().refuse = None;
    assert!(drain(&mut b, 2 * RETRY_MS).is_empty());
    assert!(b.is_up());
    assert_eq!(b.next_wake(), None);
    assert!(fake.0.borrow().sent.is_empty());
    // tgd goes: said, then found again by the next try.
    fake.0.borrow_mut().incoming.push_back(b"{\"ev\":\"chats.rows\",\"first\":0,\"total\":0,\"rows\":[]}\n{\"ev\":\"cha".to_vec());
    fake.0.borrow_mut().lost = true;
    let got = drain(&mut b, 5000);
    assert!(matches!(got.as_slice(), [Event::Auth { state: AuthState::Starting, .. }]), "{got:?}");
    assert!(!b.is_up());
    assert!(drain(&mut b, 5000 + RETRY_MS).is_empty());
    assert!(b.is_up());
    assert_eq!(fake.0.borrow().connects, 2);
    // The half line of the old connection is gone.
    fake.0.borrow_mut().incoming.push_back(b"{\"ev\":\"chat.close\"}\n".to_vec());
    let ignored = b.ignored;
    assert!(drain(&mut b, 5000 + RETRY_MS).is_empty());
    assert_eq!(b.ignored, ignored + 1);
}

#[test]
fn a_full_socket_keeps_the_requests_in_order() {
    let fake = Shared::default();
    let mut b = SocketBackend::new(fake.clone());
    drain(&mut b, 0);
    fake.0.borrow_mut().full = true;
    b.send(Request::ChatClose);
    b.send(Request::SummaryCancel);
    assert!(fake.0.borrow().sent.is_empty());
    assert_eq!(b.next_wake(), Some(20));
    fake.0.borrow_mut().full = false;
    drain(&mut b, 20);
    let sent = String::from_utf8(fake.0.borrow().sent.concat()).expect("UTF-8");
    assert_eq!(sent, "{\"op\":\"chat.close\"}\n{\"op\":\"ai.cancel\"}\n");
    assert_eq!(b.next_wake(), None);
}

#[test]
fn an_over_long_line_is_skipped_whole() {
    let fake = Shared::default();
    let mut b = SocketBackend::new(fake.clone());
    drain(&mut b, 0);
    let chunk = vec![b'x'; PACKET];
    for _ in 0..(crate::socket::MAX_LINE / PACKET + 2) {
        fake.0.borrow_mut().incoming.push_back(chunk.clone());
    }
    fake.0
        .borrow_mut()
        .incoming
        .push_back(b"xxx\n{\"ev\":\"message.delete\",\"chat\":\"1\",\"ids\":[\"9\"]}\n".to_vec());
    let got = drain(&mut b, 1);
    assert_eq!(got, vec![Event::MessageDelete { chat: 1, ids: vec![9] }]);
    assert_eq!(b.ignored, 1);
}

/// The model over the sample backend, logged in, with one chat open.
fn sample_with_chat() -> (Model, SampleBackend, u64) {
    let mut m = Model::new();
    let mut b = SampleBackend::logged_in();
    // The list's height, as the view tells it: the model then asks for its window.
    m.set_list_rows(10);
    rounds(&mut m, &mut b, 0);
    m.open_at(2);
    rounds(&mut m, &mut b, 0);
    assert!(m.open.is_some());
    rounds(&mut m, &mut b, 1);
    (m, b, 1)
}

fn rounds(m: &mut Model, b: &mut SampleBackend, now: u64) {
    for _ in 0..8 {
        m.set_clock(now);
        for r in m.take_requests() {
            b.send(r);
        }
        while let Some(e) = b.poll(now) {
            m.apply(e);
        }
    }
}

#[test]
fn a_failed_send_is_failed_not_pending_and_enter_sends_it_again() {
    let (mut m, mut b, mut now) = sample_with_chat();
    m.composer.insert_str("/fail це не дійде");
    m.send();
    rounds(&mut m, &mut b, now);
    assert_eq!(m.messages.last().map(|x| x.row.state), Some(SendState::Pending));
    now += sample::DELIVER_MS;
    rounds(&mut m, &mut b, now);
    let last = m.messages.last().expect("the message").row.clone();
    assert_eq!(last.state, SendState::Failed);
    assert!(m.take_trace().iter().any(|l| l == "tg: not sent: /fail це не дійде"));
    // Enter on it: `messages.retry`, and the sample sends it again.
    let n = m.messages.len();
    m.msg_move(0, false);
    assert_eq!(m.msg_cursor, Some(n - 1));
    m.activate_cursor();
    assert_eq!(m.reply_to, None, "Enter on a failed message does not answer it");
    let asked = m.take_requests();
    assert_eq!(asked, vec![Request::MessagesRetry { chat: m.open.expect("open"), id: last.id }]);
    for r in asked {
        b.send(r);
    }
    rounds(&mut m, &mut b, now);
    assert_eq!(m.messages.len(), n);
    let again = m.messages.last().expect("sent again").row.clone();
    assert_ne!(again.id, last.id);
    assert_eq!(again.state, SendState::Pending);
    assert_eq!(again.text.text, last.text.text);
    now += sample::DELIVER_MS;
    rounds(&mut m, &mut b, now);
    assert_eq!(m.messages.last().map(|x| x.row.state), Some(SendState::Sent));
    // On a message that did not fail, Enter answers it as before.
    m.msg_move(0, false);
    m.activate_cursor();
    assert!(m.take_requests().is_empty());
    assert!(m.reply_to.is_some());
}
