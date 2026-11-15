//! `tg`: the Telegram window (docs/tg-window.md). A native Zoraya program over
//! `zoraya-app`: it holds the compositor's channel and nothing else of the system: no
//! network, no file. What it shows comes from a `Backend`: in stage A the sample one
//! built into it (an invented account), in stage B `tgd` over a Unix socket.
//!
//! Launched by the panel as any application: slot 1 the console, 5 its compositor
//! channel (6 a chooser channel and 7 a data directory, which it does not use).
//! With synthetic data it says on the console what it shows, for the GUI test.

#![no_std]
#![no_main]

extern crate alloc;

mod heap;
mod platform;

use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::RefCell;

use tg_window::model::{Model, Role, Stats};
use tg_window::proto::Backend;
use tg_window::sample::SampleBackend;
use tg_window::view::{self, HEIGHT, WIDTH};
use zoraya_abi::{DEADLINE_INFINITE, Handle, Mode, Packet, channel_signals};
use zoraya_app::{App, Happened};
use zoraya_rt::{clock_monotonic, handle_type, port_create, port_subscribe, port_wait, println};
use zoraya_tokens::Appearance;

use platform::Gui;

const KEY_GUI: u64 = 1;
/// The paint times are said on the console every this many frames.
const FRAMES_A_REPORT: u32 = 50;

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
        println!("tg: no compositor");
        return 1;
    };
    let Some(mut app) = App::connect(Gui::new(compositor), Appearance::Light) else {
        println!("tg: no compositor");
        return 1;
    };
    let model = Rc::new(RefCell::new(Model::new()));
    let mut backend = SampleBackend::new();
    pump(&mut backend, &model);
    let tree = view::build(&model, clock_monotonic);
    let mut roles = tree.roles;
    let mut built = model.borrow().tree;
    let Some(window) = app.open("Telegram", tree.root, WIDTH, HEIGHT) else {
        println!("tg: no window");
        return 1;
    };
    let Ok(port) = port_create() else { return 1 };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, compositor, wanted, KEY_GUI, Mode::Level).is_err() {
        return 1;
    }
    let mut packets = [Packet::default(); 4];
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
                        println!("tg: copied {} characters", text.chars().count());
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
            println!("{line}");
        }
        if synthetic && stats.frames >= FRAMES_A_REPORT {
            println!(
                "tg: paint {} frames: mean {} us, longest {} us",
                stats.frames,
                stats.total_us / u64::from(stats.frames),
                stats.max_us
            );
            model.borrow_mut().stats = Stats::default();
        }
        let deadline = backend
            .next_wake()
            .map_or(DEADLINE_INFINITE, |ms| ms.saturating_mul(1_000_000));
        let _ = port_wait(port, deadline, &mut packets);
    }
}
