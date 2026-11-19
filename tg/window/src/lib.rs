//! The Telegram window (docs/tg-window.md): the protocol's types and the `Backend`
//! trait (`proto`), the window's state (`model`), the sample backend with its invented
//! account (`sample`), `tgd` over a socket (`socket`, with the wire form in `wire` and
//! `json`), and the `zoraya-ui` widgets that draw it (`view` and the modules under it). No runtime and no channel: the program `tg` is the loop around it, and
//! everything here is tested on the host.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod chat;
pub mod draw;
pub mod json;
pub mod list;
pub mod model;
pub mod proto;
pub mod sample;
pub mod socket;
pub mod time;
pub mod view;
pub mod widgets;
pub mod wire;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_wire;

// A runaway test fails at 512 MiB instead of taking the host.
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
