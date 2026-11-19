//! `SocketBackend`: `tgd` over a Unix socket (docs/tg-window.md, "Stage B"). The protocol
//! is JSON lines (`wire.rs`); the socket is a packet socket (`SOCK_SEQPACKET`), whose
//! packets this backend treats as pieces of one byte stream: a line may be longer than a
//! packet (a page of long messages), so the lines are cut into packets of at most
//! [`PACKET`] bytes and joined again on the other side at each newline.
//!
//! The transport (Zoraya's channel end, a host socket in the tests) is behind
//! [`Transport`]. While `tgd` is not there, the window shows the starting screen with
//! what to do, and the backend tries again every [`RETRY_MS`].

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use crate::proto::{AuthState, Backend, Event, Request};
use crate::wire;

/// The most bytes of one packet (Zoraya's channel message is 64 KiB with a byte of header).
pub const PACKET: usize = 60 * 1024;
/// How long after a failed connect the next one is tried.
pub const RETRY_MS: u64 = 2000;
/// The longest line taken: a longer one is dropped whole (the backend's rows are bounded
/// far below it).
pub const MAX_LINE: usize = 8 << 20;

/// Why a connection could not be made or was lost: the words the window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Down(pub String);

pub trait Transport {
    /// Connects, or says why not.
    fn connect(&mut self) -> Result<(), Down>;
    /// Sends one packet: `Ok(false)` if there is no room now (try again later).
    fn send(&mut self, packet: &[u8]) -> Result<bool, Down>;
    /// Appends the next packet's bytes to `into`: `Ok(false)` if none is ready.
    fn recv(&mut self, into: &mut Vec<u8>) -> Result<bool, Down>;
    /// Drops the connection.
    fn close(&mut self);
}

pub struct SocketBackend<T: Transport> {
    t: T,
    up: bool,
    /// When to try to connect again, on the monotonic clock (0: now).
    retry_at: u64,
    /// What was last said about being down, so that it is said once.
    said: Option<String>,
    out: VecDeque<u8>,
    inbuf: Vec<u8>,
    /// Bytes of an over-long line still to skip, up to its newline.
    skipping: bool,
    events: VecDeque<Event>,
    /// Lines that were not an event the window knows (a newer backend's, or a refusal).
    pub ignored: u32,
    /// The last time `poll` was called, for `next_wake`.
    now: u64,
    /// Output waits for room in the socket.
    blocked: bool,
}

impl<T: Transport> SocketBackend<T> {
    pub fn new(t: T) -> SocketBackend<T> {
        SocketBackend {
            t,
            up: false,
            retry_at: 0,
            said: None,
            out: VecDeque::new(),
            inbuf: Vec::new(),
            skipping: false,
            events: VecDeque::new(),
            ignored: 0,
            now: 0,
            blocked: false,
        }
    }

    pub fn is_up(&self) -> bool {
        self.up
    }

    pub fn transport(&mut self) -> &mut T {
        &mut self.t
    }

    fn down(&mut self, why: Down, now: u64) {
        if self.up {
            self.t.close();
        }
        self.up = false;
        self.blocked = false;
        self.out.clear();
        self.inbuf.clear();
        self.skipping = false;
        self.retry_at = now + RETRY_MS;
        if self.said.as_deref() != Some(why.0.as_str()) {
            self.events.push_back(Event::Auth {
                state: AuthState::Starting,
                hint: why.0.clone(),
                error: String::new(),
            });
            self.said = Some(why.0);
        }
    }

    fn flush(&mut self, now: u64) {
        self.blocked = false;
        while !self.out.is_empty() {
            let n = self.out.len().min(PACKET);
            let packet: Vec<u8> = self.out.iter().take(n).copied().collect();
            match self.t.send(&packet) {
                Ok(true) => {
                    self.out.drain(..n);
                }
                Ok(false) => {
                    self.blocked = true;
                    return;
                }
                Err(why) => return self.down(why, now),
            }
        }
    }

    fn read(&mut self, now: u64) {
        loop {
            let before = self.inbuf.len();
            match self.t.recv(&mut self.inbuf) {
                Ok(true) => {}
                Ok(false) => return,
                Err(why) => return self.down(why, now),
            }
            // The lines this packet ended.
            let mut start = 0;
            let mut scan = before;
            while let Some(i) = self.inbuf[scan..].iter().position(|&b| b == b'\n') {
                let end = scan + i;
                if self.skipping {
                    self.skipping = false;
                } else {
                    self.line(start, end);
                }
                start = end + 1;
                scan = start;
            }
            self.inbuf.drain(..start);
            if self.inbuf.len() > MAX_LINE {
                self.inbuf.clear();
                self.skipping = true;
                self.ignored += 1;
            }
            if self.skipping {
                self.inbuf.clear();
            }
        }
    }

    fn line(&mut self, start: usize, end: usize) {
        let event = core::str::from_utf8(&self.inbuf[start..end])
            .ok()
            .and_then(wire::decode);
        match event {
            Some(e) => self.events.push_back(e),
            None => self.ignored += 1,
        }
    }
}

impl<T: Transport> Backend for SocketBackend<T> {
    fn send(&mut self, r: Request) {
        if !self.up {
            // The window asks again when tgd says it is ready (`chats.window` then).
            return;
        }
        self.out.extend(wire::encode(&r).bytes());
        self.out.push_back(b'\n');
        let now = self.now;
        self.flush(now);
    }

    fn poll(&mut self, now_ms: u64) -> Option<Event> {
        self.now = now_ms;
        if let Some(e) = self.events.pop_front() {
            return Some(e);
        }
        if !self.up && now_ms >= self.retry_at {
            match self.t.connect() {
                Ok(()) => {
                    self.up = true;
                    self.said = None;
                }
                Err(why) => self.down(why, now_ms),
            }
        }
        if self.up {
            if self.blocked {
                self.flush(now_ms);
            }
            self.read(now_ms);
        }
        self.events.pop_front()
    }

    fn next_wake(&self) -> Option<u64> {
        if !self.up {
            Some(self.retry_at)
        } else if self.blocked {
            Some(self.now + 20)
        } else {
            None
        }
    }
}
