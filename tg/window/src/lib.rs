//! The Telegram window (docs/tg-window.md): the protocol's types and the `Backend`
//! trait (`proto`), the window's state (`model`), the sample backend with its invented
//! account (`sample`), and the `zoraya-ui` widgets that draw it (`view` and the modules
//! under it). No runtime and no channel: the program `tg` is the loop around it, and
//! everything here is tested on the host.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod chat;
pub mod draw;
pub mod list;
pub mod model;
pub mod proto;
pub mod sample;
pub mod time;
pub mod view;
pub mod widgets;

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host.
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
