//! `tg`'s side of the socket to `tgd` (ADR-0465): the start-up record the shell gives an
//! installed program, the `+sock` entry in it, and a packet socket made over that entry
//! as `libposix`'s `connect` makes one (`unix_file.rs`): open the node with `CONNECT`,
//! send `SocketConnect` with the far end of a new channel and the record of a
//! `SOCK_SEQPACKET`, keep the near end. A packet on it is one channel message: a count of
//! descriptors (always 0 here), then the bytes.

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use tg_window::socket::{Down, Transport};
use zoraya_abi::native::{self, Startup};
use zoraya_abi::posix::NO_HANDLE;
use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, EventObject, Handle, Mode, ObjectType, Packet,
    Status, channel_signals,
};
use zoraya_proto_file::{self as proto, rights, status};
use zoraya_protocol::{Below, Data, decode, encode};
use zoraya_rt::{
    channel_create, channel_read, channel_write, handle_close, handle_type, port_create,
    port_subscribe, port_wait,
};

/// Where `tgd --window` makes its socket, in the room `home`: its own directory
/// (`+own ~/.config/tgd`, which is `/data/home/tgd`) and the file's name.
pub const DEFAULT_SOCKET: &str = "/data/home/tgd/window.sock";
/// `SOCK_SEQPACKET` (`libposix`'s, Linux's).
const SOCK_SEQPACKET: u32 = 5;
/// One packet's channel message: the count byte and at most 64 KiB in all.
const MESSAGE: usize = 64 * 1024;

/// The entries of the start-up record (`None`: there is none, so the panel started this
/// program, not the shell).
pub fn startup_entries() -> Option<Vec<(String, Option<Handle>)>> {
    if handle_type(native::slot::STARTUP) != Ok("channel") {
        return None;
    }
    let mut bytes = vec![0u8; 64 * 1024];
    let mut handles = [native::slot::STARTUP; 64];
    let sizes = channel_read(native::slot::STARTUP, &mut bytes, &mut handles).ok()?;
    let _ = handle_close(native::slot::STARTUP);
    let got = &handles[..sizes.handles as usize];
    let record = Startup::decode(&bytes[..sizes.bytes as usize], got.len())?;
    let nth = |i: u8| (i != NO_HANDLE).then(|| got.get(usize::from(i)).copied()).flatten();
    let entries: Vec<(String, Option<Handle>)> = record
        .entries()
        .map(|e| (String::from(e.path), nth(e.handle)))
        .collect();
    // What tg does not use is given back: the report channel and the network or random
    // connections, if the shell gave them.
    for h in [record.slots.report, record.slots.net, record.slots.random]
        .into_iter()
        .filter_map(nth)
    {
        let _ = handle_close(h);
    }
    Some(entries)
}

/// The socket to `tgd`: the directory entry narrowed to its name, and the connection.
pub struct Socket {
    dir: Option<Handle>,
    name: String,
    /// What to do when the entry is missing.
    missing: String,
    end: Option<Handle>,
    reply: Vec<u8>,
}

impl Socket {
    /// The socket at `path` through the entries of the start-up record.
    pub fn new(path: &str, entries: Vec<(String, Option<Handle>)>) -> Socket {
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        let mut held = None;
        for (p, h) in entries {
            match h {
                Some(h) if p == dir && held.is_none() => held = Some(h),
                Some(h) => {
                    let _ = handle_close(h);
                }
                None => {}
            }
        }
        Socket {
            dir: held,
            name: String::from(name),
            missing: format!(
                "tg holds no socket of tgd. Type: programs grant tg +sock {path}, then start tg again"
            ),
            end: None,
            reply: vec![0; zoraya_protocol::MAX_WIRE_MESSAGE],
        }
    }

    /// The connection, for the program's port.
    pub fn end(&self) -> Option<Handle> {
        self.end
    }

    /// One request on `conn` with `moved` handles, answered by `Done`: its status.
    fn call(&mut self, conn: Handle, request: &[u8], moved: &[Handle]) -> Option<u32> {
        let entries: Vec<ChannelEntry> = moved
            .iter()
            .map(|h| ChannelEntry {
                handle: *h,
                rights: if handle_type(*h) == Ok("event") {
                    EventObject::RIGHTS
                } else {
                    ChannelObject::RIGHTS
                },
                transfer: Transfer::Move,
            })
            .collect();
        if channel_write(conn, request, &entries).is_err() {
            for h in moved {
                let _ = handle_close(*h);
            }
            return None;
        }
        wait_readable(conn)?;
        let mut got = [conn; 4];
        let sizes = channel_read(conn, &mut self.reply, &mut got).ok()?;
        for h in &got[..sizes.handles as usize] {
            let _ = handle_close(*h);
        }
        let bytes = &self.reply[..sizes.bytes as usize];
        if let Ok((_, d)) = decode::<proto::Done>(bytes, 0) {
            return Some(d.status.0);
        }
        decode::<zoraya_protocol::ErrorReply>(bytes, 0)
            .ok()
            .map(|(_, e)| e.status)
    }

    fn open_node(&mut self, dir: Handle) -> Result<Handle, u32> {
        let (mine, theirs) = channel_create().map_err(|_| status::LIMIT_REACHED)?;
        let m = proto::Open {
            flags: zoraya_protocol::Flags(0),
            rights: zoraya_protocol::Flags(rights::CONNECT),
            path: zoraya_protocol::Path(&self.name),
        };
        let mut out = vec![0u8; 512];
        let Ok(n) = encode(&m, 1, &mut out) else {
            let _ = handle_close(mine);
            let _ = handle_close(theirs);
            return Err(status::INVALID_ARGUMENT);
        };
        if channel_write(
            dir,
            &out[..n],
            &[ChannelEntry {
                handle: theirs,
                rights: ChannelObject::RIGHTS,
                transfer: Transfer::Move,
            }],
        )
        .is_err()
        {
            let _ = handle_close(mine);
            let _ = handle_close(theirs);
            return Err(status::NOT_FOUND);
        }
        let answer = wait_readable(dir).and_then(|()| {
            let mut got = [dir; 4];
            let sizes = channel_read(dir, &mut self.reply, &mut got).ok()?;
            for h in &got[..sizes.handles as usize] {
                let _ = handle_close(*h);
            }
            let bytes = &self.reply[..sizes.bytes as usize];
            match decode::<proto::OpenReply<'_>>(bytes, 0) {
                Ok((_, r)) => Some(r.status.0),
                Err(_) => decode::<proto::Done>(bytes, 0)
                    .map(|(_, d)| d.status.0)
                    .ok()
                    .or_else(|| {
                        decode::<zoraya_protocol::ErrorReply>(bytes, 0)
                            .ok()
                            .map(|(_, e)| e.status)
                    }),
            }
        });
        match answer {
            Some(status::OK) => Ok(mine),
            other => {
                let _ = handle_close(mine);
                Err(other.unwrap_or(status::NOT_FOUND))
            }
        }
    }
}

/// Waits until `end` has a message (or its peer is gone).
fn wait_readable(end: Handle) -> Option<()> {
    let port = port_create().ok()?;
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    let ok = port_subscribe(port, end, wanted, 1, Mode::Level)
        .and_then(|()| port_wait(port, DEADLINE_INFINITE, &mut [Packet::default()]))
        .is_ok();
    let _ = handle_close(port);
    ok.then_some(())
}

fn not_running() -> Down {
    Down(String::from(
        "tgd is not running. In a terminal: tgd --window (it says when it listens)",
    ))
}

impl Transport for Socket {
    fn connect(&mut self) -> Result<(), Down> {
        let Some(dir) = self.dir else {
            return Err(Down(self.missing.clone()));
        };
        let conn = match self.open_node(dir) {
            Ok(c) => c,
            Err(status::NOT_FOUND) => return Err(not_running()),
            Err(s) => {
                return Err(Down(format!(
                    "tg cannot open tgd's socket (file status {s}); see programs show tg"
                )));
            }
        };
        let Ok((near, far)) = channel_create() else {
            let _ = handle_close(conn);
            return Err(Down(String::from("tg has no room for a channel")));
        };
        // The record of the far end (`unix::connect_record(SOCK_SEQPACKET)`): kind 6, a
        // packet socket, no access bits, its type, no flags, no offsets.
        let mut record = [0u8; 24];
        record[0] = 6;
        record[2] = SOCK_SEQPACKET as u8;
        let m = proto::SocketConnect {
            sock_type: Below(SOCK_SEQPACKET),
            record: Data(&record),
        };
        let mut out = vec![0u8; 256];
        let sent = match encode(&m, 2, &mut out) {
            Ok(n) => self.call(conn, &out[..n], &[far]),
            Err(_) => {
                let _ = handle_close(far);
                None
            }
        };
        let _ = handle_close(conn);
        match sent {
            Some(status::OK) => {
                self.end = Some(near);
                Ok(())
            }
            other => {
                let _ = handle_close(near);
                match other {
                    // Nothing listens: a stale file of a tgd that ended.
                    Some(status::NOT_FOUND) => Err(not_running()),
                    Some(status::LIMIT_REACHED) => {
                        Err(Down(String::from("tgd is busy; trying again")))
                    }
                    Some(s) => Err(Down(format!("tgd's socket refused tg (status {s})"))),
                    None => Err(not_running()),
                }
            }
        }
    }

    fn send(&mut self, packet: &[u8]) -> Result<bool, Down> {
        let Some(end) = self.end else {
            return Err(not_running());
        };
        let mut message = Vec::with_capacity(packet.len() + 1);
        message.push(0); // no descriptors
        message.extend_from_slice(packet);
        match channel_write(end, &message, &[]) {
            Ok(()) => Ok(true),
            Err(Status::WouldBlock) => Ok(false),
            Err(_) => Err(Down(String::from("tgd went away. Start it again: tgd --window"))),
        }
    }

    fn recv(&mut self, into: &mut Vec<u8>) -> Result<bool, Down> {
        let Some(end) = self.end else {
            return Err(not_running());
        };
        let mut message = vec![0u8; MESSAGE];
        let mut handles = [end; 64];
        match channel_read(end, &mut message, &mut handles) {
            Ok(sizes) => {
                // Descriptors are not part of the protocol: any that come are closed.
                let got = &handles[..sizes.handles as usize];
                for h in got {
                    let _ = handle_close(*h);
                }
                let bytes = &message[..sizes.bytes as usize];
                let count = usize::from(*bytes.first().unwrap_or(&0));
                let start = 1 + 24 * count;
                if let Some(payload) = bytes.get(start..) {
                    into.extend_from_slice(payload);
                }
                Ok(true)
            }
            Err((Status::WouldBlock, _)) => Ok(false),
            Err(_) => Err(Down(String::from("tgd went away. Start it again: tgd --window"))),
        }
    }

    fn close(&mut self) {
        if let Some(end) = self.end.take() {
            let _ = handle_close(end);
        }
    }
}
