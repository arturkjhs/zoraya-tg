//! `tg`: the Telegram window (docs/tg-window.md). A native Zoraya program over
//! `zoraya-app`: it holds the compositor's channel and one socket file of `tgd`, and
//! nothing else of the system: no network, no file. What it shows comes from a `Backend`:
//!
//! - started by the shell as an installed program (a start-up record at slot 1000, its
//!   window from the panel at slot 5): `tgd` over the socket its saved `+sock` grant
//!   names (`link.rs`; the path is its argument, by default `/data/home/tgd/window.sock`);
//! - started by the panel from the dock (slot 5, no start-up record, so no grant): the
//!   sample backend built into it, an invented account, with "(sample)" in its title.
//!
//! With synthetic data it says on its output what it shows, for the GUI tests.

#![no_std]
#![no_main]

extern crate alloc;

mod heap;
mod link;
mod platform;

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use tg_window::model::{Model, Role, Stats};
use tg_window::proto::{Backend, Event, Request};
use tg_window::sample::SampleBackend;
use tg_window::socket::SocketBackend;
use tg_window::view::{self, HEIGHT, WIDTH};
use zoraya_abi::{DEADLINE_INFINITE, Handle, Mode, Packet, channel_signals};
use zoraya_app::{App, Happened};
use zoraya_rt::{
    clock_monotonic, console_write, handle_type, port_create, port_subscribe, port_unsubscribe,
    port_wait, println, startup,
};
use zoraya_tokens::Appearance;

use platform::Gui;

const KEY_GUI: u64 = 1;
const KEY_TGD: u64 = 2;

/// Where the window's data comes from.
enum Source {
    Sample(SampleBackend),
    Tgd(SocketBackend<link::Socket>),
}

impl Source {
    fn end(&mut self) -> Option<Handle> {
        match self {
            Source::Sample(_) => None,
            Source::Tgd(b) => b.transport().end(),
        }
    }
}

impl Backend for Source {
    fn send(&mut self, r: Request) {
        match self {
            Source::Sample(b) => b.send(r),
            Source::Tgd(b) => b.send(r),
        }
    }
    fn poll(&mut self, now_ms: u64) -> Option<Event> {
        match self {
            Source::Sample(b) => b.poll(now_ms),
            Source::Tgd(b) => b.poll(now_ms),
        }
    }
    fn next_wake(&self) -> Option<u64> {
        match self {
            Source::Sample(b) => b.next_wake(),
            Source::Tgd(b) => b.next_wake(),
        }
    }
}
/// The paint times are said on the console every this many frames.
const FRAMES_A_REPORT: u32 = 50;

/// A line of what the window shows (synthetic data only) or of a failure: on the console
/// when it holds one (from the dock, and from the shell as an installed window program),
/// where the GUI tests read it; else on its output (a terminal).
fn say(line: &str) {
    let console = Handle::new(1).filter(|h| handle_type(*h) == Ok("console"));
    match console {
        Some(c) => {
            let _ = console_write(c, line.as_bytes());
            let _ = console_write(c, b"\n");
        }
        None => println!("{line}"),
    }
}

/// Requests to the backend and its events to the model, until both are quiet.
/// Whether the model changed.
fn pump(backend: &mut dyn Backend, model: &Rc<RefCell<Model>>) -> bool {
    let mut changed = false;
    loop {
        let ms = clock_monotonic() / 1_000_000;
        let requests = {
            let mut m = model.borrow_mut();
            m.set_clock(ms);
            m.take_requests()
        };
        let asked = !requests.is_empty();
        for r in requests {
            backend.send(r);
        }
        let mut told = false;
        while let Some(e) = backend.poll(ms) {
            model.borrow_mut().apply(e);
            told = true;
        }
        changed |= told;
        if !asked && !told {
            return changed;
        }
    }
}

/// The role whose widget is at `path`.
fn role_at(roles: &[(Role, Vec<usize>)], path: &[usize]) -> Option<Role> {
    roles.iter().find(|(_, p)| p == path).map(|(r, _)| *r)
}

/// Text from the clipboard as a field takes it: no control characters; newlines only
/// where the field has lines.
fn pasted(text: &str, lines: bool) -> String {
    let text = if lines {
        text
    } else {
        text.lines().next().unwrap_or("")
    };
    text.chars()
        .filter(|c| !c.is_control() || (lines && *c == '\n'))
        .collect()
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let Some(compositor) = Handle::new(5).filter(|h| handle_type(*h) == Ok("channel")) else {
        say("tg: no compositor");
        return 1;
    };
    let Some(mut app) = App::connect(Gui::new(compositor), Appearance::Light) else {
        say("tg: no compositor");
        return 1;
    };
    let model = Rc::new(RefCell::new(Model::new()));
    let (mut backend, title) = match link::startup_entries() {
        Some(entries) => {
            let arg = startup().argv.next().and_then(|a| core::str::from_utf8(a).ok());
            let path = arg.unwrap_or(link::DEFAULT_SOCKET);
            let socket = link::Socket::new(path, entries);
            (Source::Tgd(SocketBackend::new(socket)), "Telegram")
        }
        None => (Source::Sample(SampleBackend::new()), "Telegram (sample)"),
    };
    pump(&mut backend, &model);
    let tree = view::build(&model, clock_monotonic);
    let mut roles = tree.roles;
    let mut built = model.borrow().tree;
    let Some(window) = app.open(title, tree.root, WIDTH, HEIGHT) else {
        say("tg: no window");
        return 1;
    };
    let Ok(port) = port_create() else { return 1 };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, compositor, wanted, KEY_GUI, Mode::Level).is_err() {
        return 1;
    }
    let mut packets = [Packet::default(); 4];
    // The socket's end the port watches (it changes when tgd goes and comes back).
    let mut watched: Option<Handle> = None;
    loop {
        let mut dirty = false;
        while let Some(h) = app.poll() {
            match h {
                Happened::Gone | Happened::CloseRequested(_) => return 0,
                Happened::Ui { response, .. } => {
                    view::respond(&mut model.borrow_mut(), response);
                    dirty = true;
                }
                // The clipboard: the selected messages offered; a paste typed into the
                // field that has the keyboard.
                Happened::Copy { .. } => {
                    let text = model.borrow().selected_text();
                    if !text.is_empty() && app.offer(&text) && model.borrow().synthetic {
                        say(&alloc::format!("tg: copied {} characters", text.chars().count()));
                    }
                }
                Happened::Pasted(_) => {
                    let Some(text) = app.take_paste() else {
                        continue;
                    };
                    let focused = app.window(window).and_then(|w| w.focused());
                    let role = focused.and_then(|p| role_at(&roles, &p));
                    let mut m = model.borrow_mut();
                    match role {
                        Some(Role::Composer) => {
                            m.composer.insert_str(&pasted(&text, true));
                            // The composer may have grown: the tree is laid out again.
                            m.tree += 1;
                        }
                        Some(Role::Search) => {
                            m.query.insert_str(&pasted(&text, false));
                            m.query_changed();
                        }
                        Some(Role::LoginField) => m.login.insert_str(&pasted(&text, false)),
                        _ => {}
                    }
                    dirty = true;
                }
                _ => {}
            }
        }
        dirty |= pump(&mut backend, &model);
        // What the model is made of changed (another screen, a chat opened, the card):
        // a new tree, the keyboard where it was unless the model names another place.
        for _ in 0..4 {
            if model.borrow().tree == built {
                break;
            }
            let was = app
                .window(window)
                .and_then(|w| w.focused())
                .and_then(|p| role_at(&roles, &p));
            let tree = view::build(&model, clock_monotonic);
            built = model.borrow().tree;
            roles = tree.roles;
            model.borrow_mut().stats.mark = true;
            app.replace(window, tree.root);
            let mut m = model.borrow_mut();
            if m.focus.is_none() {
                m.focus = was;
            }
            dirty = true;
            drop(m);
            dirty |= pump(&mut backend, &model);
        }
        let wanted = model.borrow_mut().focus.take();
        if let Some(role) = wanted
            && let Some((_, path)) = roles.iter().find(|(r, _)| *r == role)
            && let Some(w) = app.window(window)
        {
            for _ in 0..roles.len() + 1 {
                if w.focused().as_ref() == Some(path) {
                    break;
                }
                w.move_focus(false);
            }
            dirty = true;
        }
        if dirty {
            app.repaint(window);
        }
        let (lines, stats, synthetic) = {
            let mut m = model.borrow_mut();
            (m.take_trace(), m.stats, m.synthetic)
        };
        for line in lines {
            say(&line);
        }
        if synthetic && let Some(us) = stats.first_us {
            let m = &mut *model.borrow_mut();
            say(&alloc::format!(
                "tg: first frame of a new tree: {us} us, {} messages loaded, {} laid out, {} held",
                m.messages.len(),
                m.stats.laid,
                m.stats.held
            ));
            m.stats.first_us = None;
        }
        if synthetic && stats.frames >= FRAMES_A_REPORT {
            say(&alloc::format!(
                "tg: paint {} frames: mean {} us, longest {} us",
                stats.frames,
                stats.total_us / u64::from(stats.frames),
                stats.max_us
            ));
            let m = &mut *model.borrow_mut();
            m.stats = Stats {
                mark: m.stats.mark,
                first_us: m.stats.first_us,
                laid: m.stats.laid,
                held: m.stats.held,
                ..Stats::default()
            };
        }
        let end = backend.end();
        if end != watched {
            if watched.is_some() {
                let _ = port_unsubscribe(port, KEY_TGD);
            }
            if let Some(e) = end {
                let signals = channel_signals::READABLE | channel_signals::PEER_CLOSED;
                let _ = port_subscribe(port, e, signals, KEY_TGD, Mode::Level);
            }
            watched = end;
        }
        let deadline = backend
            .next_wake()
            .map_or(DEADLINE_INFINITE, |ms| ms.saturating_mul(1_000_000));
        let _ = port_wait(port, deadline, &mut packets);
    }
}
