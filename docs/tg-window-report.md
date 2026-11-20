# TG-04 report: `tg`, a native Zoraya window for the Telegram client

## Status

| | |
|---|---|
| **Result** | **Stage B steps 1 to 4 are done; the window is ready for the architect's real login** ("The real login", below). Stage A is done: the window with its sample backend. |
| What works | The chat list (initials, name, last line, time, unread count, pinned, muted) as a window of the backend's rows, scrolled by the wheel and the keys; the chat view (bubbles in and out, replies, forwards, edited, dates, links, bold and code, service lines; photos, stickers, voice and files as labelled placeholders), anchored at the newest message, older pages loaded on the way up; the composer (several lines, Enter sends, Shift+Enter a line, replies); search; the AI summary card with three scopes, its text arriving in parts; the login (phone, code, a password drawn as dots); settings and log out; light and dark; scale 1 and 2; one pane in a narrow window; every control by Tab and keys; selection of whole messages and copy (System+C); paste (System+V). New messages, confirmations and read marks come from the sample's timers as the protocol's diffs. |
| Tests | Host: `cargo test -p tg-window` 44 (the model, the widgets, protocol 3's wire and JSON, the socket backend over a fake transport, the jump to the first unread); `make test-tgd-vm` 5 checks and `make test-tgd-window` 8 steps (tgd's view-models and its socket front end over a fake TDLib). Under HVF: `make test-tg-window` (stage A, 111 steps a scale), `make test-tg-socket` (stage B: 47 steps on the console, then 61 on the desktop, at scale 1 and 2) and `make test-tg-real-start` (the real tgd with TDLib, 29 steps): **PASS**, each twice. |
| Screens | `docs/shots/tg-*.png` (stage A), `tgs-*.png` (stage B, synthetic account), `tgr-*.png` (the real tgd's login form), each at scale 1 and `@2x`; synthetic data only. |
| Zoraya | `origin/main` at `f9fcaa51` (10j, ADR-0465, ADR-0466's BIND), as the local clone `../zoraya-tgw` with no remote; three overlay patches (below). Nothing of this is in the public repository. |
| Requests | `docs/requests.md`: R20, R21, R22 (with its second half as built), R23, R24. |
| Secrets | None read, printed or committed. Every name and message in the window is invented (`tg/window/src/sample.rs`); the sample's login takes any number, the code `12345` and the password `zoraya`, which are not credentials of anything. |

## How it is built

- **`tg/window`** (crate `tg-window`, `no_std`, no `unsafe`, 4 950 lines and 1 280 of tests): `proto` (the
  protocol's requests and events and the `Backend` trait), `model` (the window's state; it gathers requests and
  applies events), `sample` (`SampleBackend`: 28 invented chats in English, Ukrainian and Russian, one of 153
  messages for paging and one of 1000 for the view's cost; timers for incoming messages every 6 s, a confirmation 0.4 s and a read mark 1.8 s after a
  send, a summary in four parts), `view`, `list`, `chat`, `widgets`, `draw` (the `zoraya-ui` widgets; text wrapped
  with its spans). It follows `zoraya-files`: a model and widgets tested on the host, a thin program.
- **`tg/tg`** (the program, 434 lines): `zoraya-app`'s loop, the model's requests to the backend and the backend's
  events to the model, a new tree when the screen changes, the toolkit's focus moved where the model asks, the
  clipboard. With synthetic data it says on the console what it shows (the GUI test reads that); with a real
  backend it will say nothing of the data.
- **Built from this repository** against the crates of `../zoraya-tgw` by path (`Cargo.toml`: a workspace of the two
  crates, Zoraya's toolchain file and `user` profile; `tg/tg/build.rs` names Zoraya's `user/user.ld`). The smallest
  way found: the program depends on `zoraya-abi`, `-rt`, `-heap`, `-app`, `-ui`, `-tokens` and not on `zoraya-user`
  (which is every ramdisk program at once and brings the network stack and TLS); the heap and the compositor
  platform, which live inside `zoraya-user`, are copied (`tg/tg/src/heap.rs`, `platform.rs`; R20). `tg` is 1 384 KiB
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
| `0003-a-window-for-an-installed-native-program.patch` | `zoraya-describe`: a volume description may say `window TITLE`; `zosh`: an installed native program with it gets its window from the panel (ADR-0317's request, the line's grants as pills) at slot 5 and the console at slot 1, beside its start-up record with the saved words; `zoraya-panel`: a `+sock PATH` pill is taken (R24); a volume description may say `cwd none`, which the native launch keeps (R28). About 185 changed lines with their tests. | R22's second half; R24; R28 |
| `0004-a-new-first-terminal-gets-a-desktop.patch` | `init`: when the first terminal closes, the panel and its chooser start again with the new terminal, so its shell has the channel to the panel for windows. 17 changed lines. | R25 |
| (no patch) | the heap and the platform copied into `tg/tg` | R20: the parts of a native window program as crates of their own |

`make overlay` applies them once and refuses a clone that has a remote.

## The test (`make test-tg-window`)

`tests/tg-window.script`, under HVF at `--gui-scale 1` and `2`, on a fresh volume each time. What it checks, by the
serial line (`tg`'s own words) and by pixels:

| Done when | The script's evidence |
|---|---|
| The chat list shows | `tg: chats 28 from 0: "Zoraya — розробка" "Saved Messages" …`; screen `tg-list` |
| It scrolls with the wheel and the keys | a notch: `chats 28 from 3`, back: `from 0`; End: `from 21`; Home: `from 0` |
| Opening a chat shows its messages with Cyrillic | `tg: opened "Оксана Коваль": 14 messages, last: Нагадую: зустріч о 15:30, кімната 4.`; the row in `bg.selection`; at least 3 000 pixels drawn in the messages; screen `tg-chat` |
| A typed message appears in the view and in the list's last line | `tg: message "Оксана Коваль" out: hello from Zoraya`, `tg: chat "Оксана Коваль" unread 0: hello from Zoraya`, then `tg: delivered: …` |
| Cyrillic typed from the keyboard, in both layouts | System+Space to `ua`, then to `ru` (the volume's `gui.keyboard-layouts = us ua ru`, `tests/tg-window.root`): `tg: message "Оксана Коваль" out: Привіт це їжак і ґанок` and `tg: chat "Оксана Коваль" unread 0: Привіт це їжак і ґанок`; the same two lines for `Привет это съёмка и эхо`; screen `tg-cyrillic` |
| A message from the sample timer arrives and raises the unread count | `tg: chat "Оксана Коваль" unread 3: …` (2 before), `tg: chat "Иван Соколов" unread 1: …`; and one into the open chat: `tg: message "Оксана Коваль": І візьми, будь ласка, зарядку.` |
| Search filters | `tg: search "book"`, `tg: chats 1 from 0: "Book club"`; Escape: `chats 28 from 0` |
| Light and dark render | Zoraya's settings window turns the theme; `tg`'s list ground becomes the dark `bg.view`; screens `tg-dark` |
| Both scales render | the same script at scale 2 (`tg-*@2x.png`) |
| The window resizes | its right edge dragged: `tg: size 720 520` |
| Also | the login with a refusal and the hidden password; older pages (`tg: older 40, 80 loaded, more`); the summary from the keyboard (`running`, `done`); settings' switch off and on |

Runs (2026-11-15, the M1 Air, QEMU under HVF, `--mem 512M`), with the final code: scale 1 **PASS** twice and scale
2 **PASS** twice (111 steps each), every one alone on the machine. Earlier the same day the script without the
Cyrillic steps passed twice at each scale (94 and 95 steps); two of those four ran while another task's guest was
running (my mistake: the run was not gated on a free machine). The very first run stopped at the resize (the drag
began 2 pixels outside the frame) and the next at the dark check (it looked at a pixel under the settings window);
both were the script's.

## Measurements

| | Scale 1 (840 × 520) | Scale 2 (1680 × 1040 pixels) |
|---|---|---|
| A frame while scrolling a chat of 153 messages with the wheel (the whole window painted, about a dozen bubbles in view), mean of 50 | 1.9 ms (1.83 to 1.91 over eight reports in two runs) | 6.7 ms (6.73, 6.76) |
| The longest such frame | 2.0 to 2.9 ms | 7.7 to 8.2 ms |
| A frame of the login and the chat list (the first 50 frames of a run) | 1.1 to 1.3 ms mean | 3.8 ms mean |
| **1000 messages loaded** (`make measure-tg-layout`): the first frame of a new tree | **1.5 to 1.8 ms** (the first of the six: 2.8 ms), 15 or 16 messages laid out | 5.6 to 6.7 ms, 15 to 17 laid out |
| 1000 messages loaded: a frame while scrolling, mean of 50 | 1.45 to 1.47 ms | 5.5 ms |

A frame is `tg`'s own paint of its whole tree (from the root's paint to after the last widget's), measured in the
program with the monotonic clock and said on the serial line every 50 frames (`tg: paint 50 frames: mean … us,
longest … us`) and for the first frame of every new tree (`tg: first frame of a new tree: … us, N messages loaded, M
laid out, …`); the commit and the compositor's composition are not in it. The program is built as Zoraya's user
programs are (`opt-level = 1`). At scale 2 the first frame of a new tree is an ordinary frame's 5.5 ms and about a
millisecond.

### The layout of a long chat, before and after

The first version laid out every loaded message whenever the tree was new (another screen, the card opened or
closed), and walked every message on every frame. Measured at 1000 messages, scale 1, by the same script
(`tests/tg-layout.script`: the sample's `Project archive`, every page loaded, the card opened and closed three times,
then the wheel):

| | Before | After |
|---|---|---|
| The first frame of a new tree | 6.3 to 8.4 ms, 1000 messages laid out | 1.5 to 1.8 ms (once 2.8 ms), 15 or 16 laid out |
| A frame while scrolling | 2.4 ms | 1.45 ms |

What changed (`tg/window/src/chat.rs`, `draw.rs`):

- **Only the rows in the view and half a view each way are laid out.** Every other message has a slot with a height:
  a layout's once it had one, else an estimate from its parts and its text's length. A frame adds the slots up from
  the newest message and builds positions only near the view; a page of older messages moves the slots with their
  messages. At most 256 layouts are held.
- **The view stays on its messages** when an estimate becomes a layout or a message arrives: the message at the
  view's top is its anchor, kept in the model so that a new tree keeps it too (host test: the same messages are under
  the same points after a new tree and after paging down and up, in the chat with long wrapped messages as well).
- **Shapes without masks of their own size.** A second cost showed while measuring: with the 1000-message chat
  added to the sample, every frame of every screen became slower (the same script: 1.75 ms to 2.7 ms while scrolling
  the 153-message chat), though nothing about those frames had changed. Each bubble and box was drawn through a
  coverage mask of its whole size, allocated per frame, and the allocator was slower on the larger heap. Shapes are
  now filled directly, with one small mask for the four corners (pixel for pixel the same: a host test compares).
  That brought the same script back to 1.9 ms; it is 0.15 ms more than before the sample grew, which I have not
  traced further.

**Memory**: **5 343 K** (5.2 MiB) in `top`, for `tg` alone in a room with a 96 MiB limit, at scale 1, logged in to
the sample, with the 153-message chat loaded (`make measure-tg-memory`, `tests/tg-memory.script`; the screen is
`docs/shots/tg-memory-top.png`). That holds its 1.7 MB window buffer, the fonts' glyph caches and the whole sample
account, which is about 1 MB larger than at the first measurement (4 313 K) because of the 1000-message chat.
Measured once with the final code.

Sizes: `tg` 1 384 KiB; Zoraya's ramdisk with it 29.7 MB.

## Known limits

- **Text**: no emoji (no font: R7; they draw as boxes), italic drawn upright, no selection of part of a message, a
  press in the composer does not place the cursor (the keys do).
- **Layout**: a message far from the view stands at an estimated height until it is first shown, so the distance
  the view can still scroll is approximate until then (there is no scroll bar to show it). A new tree estimates
  every loaded message again (about 0.3 ms at 1000).
- **The list** asks for its rows again at every scroll step (one screenful); with the sample that is instant, over a
  socket a row appears as a grey disc until its `chats.rows` comes.
- **Buttons**: a press is taken at its start, and the toolkit gives a widget no pointer once it left it, so a button
  pressed, left and released outside ignores the next press once.
- **Not in stage A**: search in messages, drafts, typing, mentions, reactions, editing and deleting, media, several
  accounts; the protocol's table in `docs/tg-window.md` names where each goes.
- **`tg` from a terminal**: as an installed program, with overlay 0003 (R22). From the dock it is `tg-sample`, the sample backend.

## What stage B waits for

In Zoraya's `origin/main` (checked at `3292aa28`: none is merged: 10g and 10j are scheduled, TASK-34 has started): 10g and
10j (a program installed on the volume with a window and saved grants), TASK-34's F2 and F7 (Unix sockets and
socket files), and a route for native programs built outside the repository (R20 to R22; TASK-35). Then: tgclient's
view-models under `tgd/vm/` serving the protocol of `docs/tg-window.md`, `SocketBackend` in place of
`SampleBackend` behind the same trait, both programs installed, and the architect's real login in the window's
password field.

## Stage B step 1: tgclient's view-models in `tgd/vm/` (2026-11-19)

Zoraya's `origin/main` at `754d2b49` has 10j (installed programs, saved grants) and the socket files in a program's
own directory reached by `+sock` (ADR-0465, accepted; its BIND right is queued as `work/bind-right`). Step 1 needs
none of it: it is host Python only, and was done while the TASK-31 dispatcher held the machine's builds (no cargo, no
make of Zoraya, no QEMU).

**What it is.** `tgd/vm/` (2 186 lines): tgclient's Qt view-models and controllers with their method bodies kept,
over `qt.py` (161 lines), the part of `PySide6.QtCore` they use without Qt. A `Signal` is a list of callables, and
`QAbstractListModel`'s begin/end pairs emit `rowsInserted`, `rowsRemoved`, `rowsMoved`, `dataChanged` and
`modelReset` with Qt's arguments. `server.py` (`WindowServer`) listens to those notifications as QML did and speaks
protocol 2 (`docs/tg-window.md`, "The protocol"): requests in (`handle`), events out (`emit`), ids as strings, the
names and fields of `tg_window::proto`.
- **The chat list:** every notification means "the window may have changed". Its rows are computed again and the
  middle that differs from what was sent goes out as one `chats.splice`.
- **The messages:** a mirror of the model's ids lets a removal know what it removed. An insertion at the newest end
  is a `message.new`. A changed row whose protocol row differs from the one sent is a `message.update`. A removal is
  a `message.delete`. A sent message's temporary id becoming its final one (ChatHistory removes the old row, then
  inserts the new) is one `message.update` with `old_id`.
- **The text:** `rows.rich` is the emitter beside tgclient's `formatted_to_html`. It uses the same entities and the
  same UTF-16 map, and writes code points and spans. Spoilers stay hidden as in tgclient: their characters become
  U+2591, so no offset moves.

| `tgd/vm/` | tgclient (`ccba758`, read-only) | Kept | Changed or left for later |
|---|---|---|---|
| `qt.py` | PySide6.QtCore | `Signal`, `Slot`, `Property`, `QObject`, `QAbstractListModel` (begin/end, `index`), `QModelIndex`, `Qt.ItemDataRole`, `QByteArray` | No event loop: tgclient's models already run on asyncio. |
| `chat_list.py` | `models/chat_list.py` `ChatListModel` | Everything. | Only the imports (`.qt`, `tgclient.store`). |
| `messages.py` | `models/messages.py` `MessageListModel`, `_Adapter` | `_Adapter`, `open`, `_start`, `_stop`, `close`, `loadOlder`, `loadNewer`, `jumpToLatest`, `send`, `sendMessage`, `markViewed`, `_load_initial`, `_send`, `_formatted`, `_run_load`, `_row_changed`, `_on_chat_store`, `_status`, `_sender_name`, `_forwarded_from`, the forum/thread queries, `_album_range`, `_refresh_albums` | `__init__`: no font, palette, files, presence, forums or custom emoji. `data()` answers the roles a protocol row needs (`Html` becomes `Body`, the formattedText; `Time` is the Unix time) and does not prefetch: the window asks with `messages.older`. Later: selection and its actions, search in a chat, pinned bar, scheduled, keyboards, polls, reactions, edit, delete, forward, links, media files, translation, transcripts. |
| `composer.py` | `models/composer.py` `ComposerModel` | `setDraft`, `sent`, `flush`, `close`, `_on_chats` (the cloud draft back), `_save_later`, `_save`, `_send_typing` | `_on_chat_changed` without staged files, link preview, @mentions. `messages.send` calls `sent()`; `draft.set` comes later. |
| `server.py` `_search` | `models/search.py` `SearchModel._chat_rows` | The match (title contains the query, chats in a list) and the order (starts with it first, then Telegram's order). | All matches, not the panel's first five: they are the filtered list `chats.window` asks for. Message hits (SearchService, numpy) later. |
| `auth.py` | `ui/auth_controller.py` `AuthController` | Everything else, including the AuthUI protocol that tgclient's AuthFlow calls. | No QR login (`qrSource`, `requestQr`; TG-01, decision 5). |
| `ai.py` | `ui/ai_controller.py` `AiController`, over tgclient's `services/ai.py` `AiService` unchanged | `chatId`, `available`, `enabled`, `setEnabled`, `summarize`, `summaryState`/`Scope`/`Text`/`Error`, `_on_service`, `_on_chats` | The key, models, budget, questions, events, answers, documents, replies, translation, digest later. tgclient's summary arrives whole, so `ai.summary` is `running` and then `done` with the whole text; `ai.cancel` stops telling the window (tgclient's job runs on). |
| `rows.py` | `store/richtext.py` (`formatted_to_html`), `store/format.py`, `store/media.py` | The entities, the UTF-16 map, the previews and labels, `extract`, `human_size`, `duration_text` | New: the protocol's chat and message rows; video, GIF and video messages as photo placeholders with their label and duration; audio and documents as files; tgclient's `failed` stays `pending` (protocol 2 has no failed state yet). |

Protocol choices recorded here:
- **Settings:** `settings` has one item, `ai.chat` ("AI summaries of this chat"), while a chat that AI may serve is
  open. It is tgclient's per-chat switch, so a summary is never asked of a chat whose AI is off.
- **Login refusals:** an answer for a step not asked now, or a message to a chat not open, raises `RequestError`.
  Its text names what was wrong, never the value.
- **`messages.reset`:** `subtitle` is empty until presence is wired (`PresenceStore`, later). Opening a chat marks
  what the reset showed as viewed, and a new incoming message in the open chat is viewed at once.

**Tested** on the host by `tests/tgd_vm.py` (`make test-tgd-vm`, Python 3.12, under a second). It runs tgclient's own
`TdClient`, `ChatStore`, `UserStore`, `FileManager` and `ChatHistory` over a fake hub that answers TDLib's requests
and sends its updates from an invented account in English, Ukrainian and Russian. There is no TDLib and no account,
and nothing of tgclient's data or sessions is opened. Five checks, **passed twice**:
- **The chat list:** whole rows, one splice for a move, and 600 random steps (messages, reorders, pins, a chat
  leaving, new chats, new windows). After each step the window's rows equal the store's list, in order and content.
- **Search** with a Cyrillic query.
- **Messages:**
  - the reset (oldest first, `more`), with spans in code points after an emoji;
  - a reply's quote;
  - pages to the start (contiguous);
  - a send: `message.new` pending with the temporary id, then `message.update` to the final id, with no
    `message.delete`;
  - a new incoming message, read on arrival;
  - read by the other side, edited, deleted;
  - a hidden spoiler, the placeholders' details, a link span;
  - a closed chat refused.
- **AI:** refused while off for the chat, switched on through `settings.set`, then `running` and `done`.
- **The login:** `hello` (protocol 2, the clock, the zone), phone, code and password through the AuthController,
  a code refused while the phone is asked, TDLib's error message shown. No typed value comes back in an event.

**Next (step 2):** `tgd` creates the `WindowServer` over its core's client and stores, with the `AuthController` as
the AuthFlow's UI. It listens on a socket file in its own directory (ADR-0465). `tg`'s `SocketBackend` serialises
`tg_window::proto` over it, with a saved `+sock` grant.

## Stage B steps 2 to 4 (2026-11-19)

The TASK-31 dispatcher paused; `../zoraya-tgw` is at `origin/main`'s `f9fcaa51`. Built here (the Mac mini) with
`CARGO_BUILD_JOBS=4`, one QEMU at a time: Zoraya's ports (C++, zlib, OpenSSL, Python 3.15.2) in `../zoraya-tgw`,
TDLib for Zoraya (192 s at `-j4`), python3 with `_tdjson`, `/bin/tgd` (64 MiB), the image with the overlay.

**Step 2: the socket, and protocol 3.**
- `tgd --window` listens on `window.sock` in its own directory (`~/.config/tgd`, which is `/data/home/tgd`): on
  Zoraya a `SOCK_SEQPACKET` socket whose packets carry the JSON lines in pieces of at most 60 KiB (a line may be
  longer than a packet: a `messages.reset` of long messages is). One window at a time; a new one hears everything
  from the start. The window's `AuthController` is AuthFlow's UI; the core tells it ready, failed or logged out. The
  api id and hash are asked on tgd's terminal, once (kept in its own directory). Ctrl-C and `kill` end it cleanly
  (signals reach the loop through a wakeup descriptor: on Zoraya they land on TDLib's threads).
- `tg` finds the `+sock` entry of its start-up record, opens the node with `CONNECT` and sends `SocketConnect`
  with a seqpacket channel end, as `libposix`'s `connect` does (`tg/tg/src/link.rs`). While tgd is not there the
  window says what to do and tries again every 2 s. From the dock (no start-up record, no grant) it is `tg-sample`,
  the sample backend, titled `Telegram (sample)`.
- Protocol 3: a failed send is `failed` (an error mark, `Not sent, Enter to retry`), never pending; Enter or a second
  press sends `messages.retry`, which tgd turns into TDLib's `resendMessages`.
- `tgd --window --synthetic`: an invented account (`tgd/tgd/synthetic.py`, the fake TDLib of the host tests) through
  the same view-models and socket, with no TDLib and no network; the GUI test uses it.

**Step 3: installed.** Install needs the volume's root, which only the console's shell holds (a terminal of the
desktop holds what `home` gives, ADR-0242's condition), so the volume is prepared on the console and used on the
desktop. The lines (`tgd --install-line` prints the first):

```
install /packages/tgd +own ~/.config/tgd +net 91.108.56.0/22:443 +net 91.108.4.0/22:443 +net 91.108.8.0/22:443 +net 91.108.16.0/22:443 +net 91.108.12.0/22:443 +net 149.154.160.0/20:443 +net 91.105.192.0/23:443 +net 91.108.20.0/22:443 +net 185.76.151.0/24:443 +net openrouter.ai:443
install /packages/tg +sock /data/home/tgd/window.sock
```

`programs tgd` and `programs tg` (ADR-0399's page; there is no `programs show` word) list exactly these:
`tests/tg-install.script` checks each saved row line by line (`^`), so a row more or less fails it. tgd's page:
`+own ~/.config/tgd` and the ten `+net` words; tg's: `+sock /data/home/tgd/window.sock`. tg's window shows the pills
`+r /home` (the current directory), `+r /apps/tg` (its package) and `+sock /data/home/tgd/window.sock`; it holds no
network and none of tgd's files. The volume has no `/bin/tgd`: `/bin` is on `shell.path`, which comes before the
installed names. Known: tgd asks for `/dev/urandom` once (TDLib); the terminal offers `+r /dev`, which is not needed
(the randomness is `getrandom`, `patches/td/0004`).

**Step 4: where the view is.** A thumb at the messages' edge; `N newer · End` while above the newest; `N unread · U`
while the first unread message is above the view or not loaded; tgclient's `Unread messages` line above it. U or a
press on the pill loads older pages (200 at a time) until it is there and brings it, with its line, to the view's
top. `messages.reset` gives `last_read` and `unread` (the server keeps tgclient's boundary and loads the newest page).

**Tests.** `make test-tg-socket` (twice, scale 1 and 2): the console's boot installs both and checks both pages; the
desktop's boot runs `tgd --window --synthetic &` and `tg &`: the login through tgd's AuthController (a wrong code
refused), the list, the club opened with 47 unread and the jump through an older page, a send confirmed, a failed
send and its retry, tgd killed (`tgd went away`) and started again (found again, the login asked again).
`make test-tg-real-start` (twice): the real tgd with TDLib, placeholder credentials on the test DC and no network:
the api id and hash asked on the terminal, Ctrl-C, `tgd --window &` (nothing asked now), `tg &`: the window shows
the phone form (`docs/shots/tgr-window.png`).

**Found and fixed on the way:** the panel refused a window with a `+sock` pill (R24, overlay 0003); TDLib's
`realpath` failed in the own directory (R23, `patches/td/0006`); tgclient's `td` imports `ctypes` (the binding is
installed before the view-models); `install` refused `tg` while the ramdisk had a program of that name (the dock's
copy is `tg-sample`); a SIGINT inherited ignored by a job started with `&`, and a signal that never woke the loop.

## The hand run that failed, and why (2026-11-19, evening)

The architect's `make run-tg-login` did not work: the first terminal said "it needs a window, and this session has no
desktop to give one", the dock's terminal tile seemed to do nothing, and a closed terminal came back at once. Every
test before had booted with `--test` (no display), which none of this touches. Reproduced in run-tg-login's own mode
(interactive, the cocoa display) through the GUI runner, on copies of the kept volume:

1. **The display.** QEMU's cocoa display with `zoom-to-fit` tells the guest its window's size, and on this Mac mini's
   1920x1080 screen that is 640x480 right after the boot (`gpud: display 640x480`), at scale 2 and at scale 1 alike.
   Zoraya's desktop follows: tg's 840x520 window is refused (`tg: no window`), the dock and terminals are crowded, a
   press lands elsewhere. Fixed: run-tg-login uses `cocoa,left-command-key=on` (no zoom-to-fit) and scale 1 (R26);
   and tg now tries 600x420 and 440x320 when its first size is refused (the view has one pane below 620; on a new
   volume: the kept volume's installed tg is the one installed with it).
2. **The closed terminal.** `init` starts a new first terminal beside the running panel, and the shell's channel to
   the panel for a window is made only when the panel starts, so the new shell has none: exactly the message the
   architect saw (`make walk-tg-login`'s first steps reproduce it without the fix). Fixed in `overlay/0004` (R25).
3. **The tile** does what it should: a press raises the terminal that is open (one window an application); on the
   640x480 desktop that was hard to see.
4. **And a danger found on the way:** `make test-tg-socket` and `make test-tg-real-start` used to make the same volume
   as run-tg-login again, which would have deleted the kept volume and its login. The tests now have their own
   (`build/tgs-test/`); `tg-install` refuses to replace a kept volume without `FRESH=1`. The kept volume was not
   touched: it holds the api id and hash the architect typed, and the walks below ran on copies without network.

**Walked by hand, scripted** (`make walk-tg-login`, `tests/tg-login-path.script`, passes twice): run-tg-login's own
command (interactive, the same cocoa display, scale 1) on a copy of the kept volume, without network: the first
terminal closed and opened again by init; in it `tgd --window --no-probe &` (the saved api id and hash: nothing
asked) and `tg &`; the window shows the phone form (`docs/shots/tgl-phone.png`); the terminal tile brings the
terminal forward. After the fixes `make test-tg-socket`, `make test-tg-window` and
`make test-tg-real-start` pass again.

## After the real login: emoji and the window's pills (2026-11-20)

The architect's login worked (the list, chats, Cyrillic, replies and links). Two things were left:

1. **Emoji were boxes**, in titles (a flag) and in messages: the system has no emoji font. The generic request is R27
   (Noto Emoji in `/system/fonts` through `zoraya-text`, flags from regional-indicator pairs, wide characters).
   Until it lands the window draws each emoji sequence its fonts lack as one small neutral mark (`◦`, the first of
   `◦` `•` `·` the fonts have): a flag's two indicators, a sequence joined with U+200D and its skin tones and
   selectors are one mark; a keycap's parts vanish and leave the digit. Measuring goes the same way, so lines wrap
   and titles are cut where they are drawn. A script with no font keeps the box. `draw::shown`, with a host test.
2. **The frame showed `+r /home`.** Not a saved grant (`programs tg` listed only `+sock`) and not the panel: the
   launch. Every program a shell starts gets its current directory read only (ADR-0153 rule 1) unless its
   description says `cwd none`, and an installed program's description could not say it (ADR-0396 item 5); the
   native launch planned it as an undescribed program, `cwd read`. tg reads no file, so it is not needed. Now
   `tg.cmd` says `cwd none`, and `overlay/0003` lets an installed description say it (only `none`: `cwd read` is
   still refused) and the native launch keep it (R28). The frame's pills are `+r /apps/tg` (its package, which every
   installed program is given: ADR-0397 item 5) and `+sock /data/home/tgd/window.sock`.

**The kept volume, upgraded in place.** It holds the login, so it is not made again: `make tg-upgrade` boots it on the
console with a network that reaches one host server only (127.0.0.1, serving the new package's two files through a
guestfwd rule; restrict=on), `fetch`es `tg` and `tg.cmd` over `/packages/tg`, and runs `install --keep /packages/tg`,
which replaces tg and keeps its saved `+sock`; `programs tg` then lists only `+sock`. tgd and its files (the api id
and hash, TDLib's session) are not touched. Tried first on a copy (`make walk-tg-login UPGRADE=1`: the upgrade, 25
steps, then the path, 20 steps: the window opens from a reopened terminal and its frame has no `+r /home` pill).
`make walk-tg-login` takes no screenshot any more: the volume holds the account; it checks two strips of the frame.
`make test-tg-socket` checks the pill too, at both scales.

## The real login (the architect, by hand)

1. On the Mac mini, in `~/workspace/zoraya-tg`: `make run-tg-login`. It keeps the volume (`build/tgs/disk.img`, made
   and installed on the first run; `FRESH=1` makes it again and loses the login) and starts the desktop at 1024x768
   with the Internet. `SCALE=2` asks for 2048x1536, which this screen cannot show whole.
2. The api id and hash are already on the kept volume (typed in the first hand run). On a new volume, type
   `tgd --window` once without `&`: it asks for them and the OpenRouter key (Enter: none); then Ctrl-C.
3. In the desktop's terminal: `tgd --window &`, then `tg &`. The window opens with the pills `+r /apps/tg` (its
   package) and `+sock /data/home/tgd/window.sock`. (Closing the first terminal is fine now: the new one can start tg too.)
4. In the window: the phone number (with the country's code), Next; the code; the password if the account has one
   (drawn as dots). The list comes.
5. To check: open a chat, send a message, the summary (Settings: turn on AI summaries of the chat, then Summary),
   the jump to the first unread (U).
6. A restart keeps the login: `kill %1` (tgd; the window says `tgd went away`), `tgd --window &`: the window finds
   it again and shows the list without asking. Power off with `poweroff` in a terminal, `make run-tg-login` again:
   `tgd --window &`, `tg &`, the list.

tgd's log is `~/.config/tgd/tgd.log` inside Zoraya (TDLib's `td.log` beside it); neither holds message text, names,
codes or passwords. The window prints nothing of the account (it prints only for synthetic data).
