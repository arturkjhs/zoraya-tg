# TG-04 report: `tg`, a native Zoraya window for the Telegram client

## Status

| | |
|---|---|
| **Result** | **Stage A: MEMORY-AND-SECOND-RUNS-PENDING.** The window `tg` with its built-in sample backend runs inside Zoraya from the dock; the design and the protocol are in `docs/tg-window.md`. |
| What works | The chat list (initials, name, last line, time, unread count, pinned, muted) as a window of the backend's rows, scrolled by the wheel and the keys; the chat view (bubbles in and out, replies, forwards, edited, dates, links, bold and code, service lines; photos, stickers, voice and files as labelled placeholders), anchored at the newest message, older pages loaded on the way up; the composer (several lines, Enter sends, Shift+Enter a line, replies); search; the AI summary card with three scopes, its text arriving in parts; the login (phone, code, a password drawn as dots); settings and log out; light and dark; scale 1 and 2; one pane in a narrow window; every control by Tab and keys; selection of whole messages and copy (System+C); paste (System+V). New messages, confirmations and read marks come from the sample's timers as the protocol's diffs. |
| Tests | `make test-tg-host`: 30 host tests (the model over the sample backend, the list's diffs over 600 random steps, the widgets through `zoraya-ui`'s window at both scales in both themes). `make test-tg-window`: `tests/tg-window.script`, 96 steps under HVF through Zoraya's GUI test runner, at scale 1 and at scale 2. |
| Screens | `docs/shots/tg-*.png` (scale 1, 1024 × 768) and `tg-*@2x.png` (scale 2, 2048 × 1536); synthetic data only. |
| Zoraya | `origin/main` at `3292aa28`, as the local clone `../zoraya-tgw` with no remote; two overlay patches (16 changed lines). Nothing of this is in the public repository. |
| Requests | `docs/requests.md`: R20, R21, R22 new. |
| Secrets | None read, printed or committed. Every name and message in the window is invented (`tg/window/src/sample.rs`); the sample's login takes any number, the code `12345` and the password `zoraya`, which are not credentials of anything. |

## How it is built

- **`tg/window`** (crate `tg-window`, `no_std`, no `unsafe`, 3 900 lines and 1 100 of tests): `proto` (the
  protocol's requests and events and the `Backend` trait), `model` (the window's state; it gathers requests and
  applies events), `sample` (`SampleBackend`: 27 invented chats in English, Ukrainian and Russian, one of 153
  messages for paging; timers for incoming messages every 6 s, a confirmation 0.4 s and a read mark 1.8 s after a
  send, a summary in four parts), `view`, `list`, `chat`, `widgets`, `draw` (the `zoraya-ui` widgets; text wrapped
  with its spans). It follows `zoraya-files`: a model and widgets tested on the host, a thin program.
- **`tg/tg`** (the program, 416 lines): `zoraya-app`'s loop, the model's requests to the backend and the backend's
  events to the model, a new tree when the screen changes, the toolkit's focus moved where the model asks, the
  clipboard. With synthetic data it says on the console what it shows (the GUI test reads that); with a real
  backend it will say nothing of the data.
- **Built from this repository** against the crates of `../zoraya-tgw` by path (`Cargo.toml`: a workspace of the two
  crates, Zoraya's toolchain file and `user` profile; `tg/tg/build.rs` names Zoraya's `user/user.ld`). The smallest
  way found: the program depends on `zoraya-abi`, `-rt`, `-heap`, `-app`, `-ui`, `-tokens` and not on `zoraya-user`
  (which is every ramdisk program at once and brings the network stack and TLS); the heap and the compositor
  platform, which live inside `zoraya-user`, are copied (`tg/tg/src/heap.rs`, `platform.rs`; R20). `tg` is 1 380 KiB
  (about 0.96 MB of it the embedded fonts, as every Zoraya program that draws text).
- **In Zoraya**: `make tg-image` applies the overlay, builds Zoraya's kernel with `EXTRA_PROGRAMS=…/tg
  EXTRA_APPS=tg` (the ramdisk grows from the program; the dock gets a fifth tile, `T`), and makes the volume through
  Zoraya's `tools/run-disk.sh` with `EXTRA_ROOT=out/root` (tgd's part, so the image is stage B's). The panel launches
  `tg` as any application: the console, its compositor channel, a chooser channel and its data directory (the pill
  `+w /data/home/tg`; `tg` uses neither of the last two). There is no network pill: the window holds none.
  It starts from the dock only: `zosh` starts no window program (R22).

## The overlay

| Patch (on `overlay/BASE`) | What it does | The generic request |
|---|---|---|
| `0001-extra-programs-in-the-ramdisk.patch` | `Makefile`: `EXTRA_PROGRAMS` (ELF files added to the ramdisk), `EXTRA_APPS` exported to the build | R21: native programs from outside the repository in the image |
| `0002-extra-applications-in-the-dock.patch` | `zoraya-panel`: `apps()` = the four of `APPS`, then the names of `ZORAYA_EXTRA_APPS`; the dock's text and a tile's press use it | R22: a dock tile and a launch record for a program that is not one of the four |
| (no patch) | the heap and the platform copied into `tg/tg` | R20: the parts of a native window program as crates of their own |

`make overlay` applies them once and refuses a clone that has a remote.

## The test (`make test-tg-window`)

`tests/tg-window.script`, under HVF at `--gui-scale 1` and `2`, on a fresh volume each time. What it checks, by the
serial line (`tg`'s own words) and by pixels:

| Done when | The script's evidence |
|---|---|
| The chat list shows | `tg: chats 27 from 0: "Zoraya — розробка" "Saved Messages" …`; screen `tg-list` |
| It scrolls with the wheel and the keys | a notch: `chats 27 from 3`, back: `from 0`; End: `from 20`; Home: `from 0` |
| Opening a chat shows its messages with Cyrillic | `tg: opened "Оксана Коваль": 14 messages, last: Нагадую: зустріч о 15:30, кімната 4.`; the row in `bg.selection`; at least 3 000 pixels drawn in the messages; screen `tg-chat` |
| A typed message appears in the view and in the list's last line | `tg: message "Оксана Коваль" out: hello from Zoraya`, `tg: chat "Оксана Коваль" unread 0: hello from Zoraya`, then `tg: delivered: …` |
| A message from the sample timer arrives and raises the unread count | `tg: chat "Оксана Коваль" unread 3: …` (2 before), `tg: chat "Иван Соколов" unread 1: …`; and one into the open chat: `tg: message "Оксана Коваль": І візьми, будь ласка, зарядку.` |
| Search filters | `tg: search "book"`, `tg: chats 1 from 0: "Book club"`; Escape: `chats 27 from 0` |
| Light and dark render | Zoraya's settings window turns the theme; `tg`'s list ground becomes the dark `bg.view`; screens `tg-dark` |
| Both scales render | the same script at scale 2 (`tg-*@2x.png`) |
| The window resizes | its right edge dragged: `tg: size 720 520` |
| Also | the login with a refusal and the hidden password; older pages (`tg: older 40, 80 loaded, more`); the summary from the keyboard (`running`, `done`); settings' switch off and on |

Runs: SEE-RUNS.

## Measurements

MEASUREMENTS.

## Known limits

- **Text**: no emoji (no font: R7; they draw as boxes), italic drawn upright, no selection of part of a message, a
  press in the composer does not place the cursor (the keys do).
- **Layout cost**: a new tree (another screen, the card opened or closed, one pane or two) lays every loaded message
  out again: the first frame after it took up to 11 ms at scale 1 with 153 messages. The cache belongs with the
  model; not done.
- **The list** asks for its rows again at every scroll step (one screenful); with the sample that is instant, over a
  socket a row appears as a grey disc until its `chats.rows` comes.
- **Buttons**: a press is taken at its start, and the toolkit gives a widget no pointer once it left it, so a button
  pressed, left and released outside ignores the next press once.
- **Not in stage A**: search in messages, drafts, typing, mentions, reactions, editing and deleting, media, several
  accounts; the protocol's table in `docs/tg-window.md` names where each goes.
- **`tg` from a terminal**: not possible in Zoraya today (R22).

## What stage B waits for

In Zoraya's `origin/main` (checked at `3292aa28`: none is merged: 10g and 10j are scheduled, TASK-34 has started): 10g and
10j (a program installed on the volume with a window and saved grants), TASK-34's F2 and F7 (Unix sockets and
socket files), and a route for native programs built outside the repository (R20 to R22; TASK-35). Then: tgclient's
view-models under `tgd/vm/` serving the protocol of `docs/tg-window.md`, `SocketBackend` in place of
`SampleBackend` behind the same trait, both programs installed, and the architect's real login in the window's
password field.
