# TG-01: tgclient on Zoraya — the plan

Built on `docs/analysis.md` (section numbers below refer to it). Decisions taken without
the architect (travelling) are marked **Decided** with the reason, so they can be reversed.

## Decisions

1. **The working plan's backend stands: `tgd`** = tgclient's Python without Qt, TDLib in a
   static `python3` through a built-in `_tdjson` module, asyncio + threads in place of Qt's
   loop. Confirmed: tgclient already has no `QThread`, `QSettings` or
   `QNetworkAccessManager`; its TDLib layer is a 4-function binding behind a `Protocol`
   (§1.3, §1.4).
2. **Decided: the view-model refactor happens in tgclient, not in a fork.** The 40 % of the
   Python that is Qt-shaped logic (`models/`, controllers) becomes Qt-free view-models
   (`tgclient/vm/`) plus thin Qt shims, so the desktop client and `tgd` share one copy.
   *Reason:* a copy would drift from the desktop client within weeks; tgclient's own
   invariant ("`store/` does not depend on Qt") is the same rule taken one layer up. This is
   the architect's repository and needs their go-ahead; this repository only holds the
   Zoraya-specific build and glue.
3. **Decided: the first front end is a terminal UI, `tg-term`, in C++ (`zc++`) with
   ncurses — not the native Rust window.** *Reasons* (§3.7):
   - a native `zoraya-app` program is `no_std` Rust that lives **inside** the Zoraya tree
     (crate + `user/src/bin` + the panel's const `APPS` table); there is no out-of-tree SDK
     and no Rust `std` (ADR-0166). Building it would put the program into the public
     repository, which this task forbids;
   - a POSIX program can have a window (ADR-0317), but only through an image description;
   - the terminal path needs nothing from Zoraya beyond stage 10's own parts 10a–10d, and
     can be developed and tested on the host today against a host `tgd`;
   - the window's advantages (photos, emoji) are not available on Zoraya yet anyway: no
     system image decoders, no emoji font, no audio (§3.3).
   C++ rather than Python for the front end, because the later window front end (part 7)
   must call `zoraya_window_*`/`zoraya_text_*`, which Python without `ctypes` cannot; one
   C++ client core (protocol, state, layout of the chat list and the feed) then serves both
   renderers.
4. **Decided: process layout** (§3.8). The shell starts `tgd` (it gets the network and
   `getrandom`); `tgd` forks first thing and the child `exec`s `tg-term` with a pipe pair on
   fds 3/4; JSON lines both ways. `tgd` never touches the terminal; `tg-term` never opens a
   socket.
5. **Decided: what is left out of the first versions** — semantic search (numpy,
   fastembed), self-update, QR login, system notifications, audio, video, animated
   stickers, GIF playback, voice recording, video notes. Each comes back when Zoraya gains
   the piece (requests R4–R9).
6. **Decided: where things live.** This repository (`zoraya-tg`, private) holds: the TDLib
   build recipe and patches, the `python3` + `_tdjson` recipe, `tgd`'s Zoraya glue
   (entry point, paths, grants, lock), `tg-term`, the protocol spec, and the generic
   requests. TDLib and CPython sources are fetched by pinned hash, never committed.
   Nothing goes into `arturkjhs/zoraya`.

## The protocol between `tgd` and its front end (sketch, fixed in part 3)

- JSON lines over a pipe pair; each line ≤ 64 KiB; UTF-8; ids as strings where TDLib's are
  int64.
- Front end → `tgd`: `{"op": "<command>", "id": n, ...}` — the view-models' slots
  (`chats.open`, `messages.send`, `messages.loadOlder`, `messages.jumpTo`,
  `search.query`, `ai.summarize`, `auth.phone`, …).
- `tgd` → front end: `{"ev": "<event>", ...}` — state snapshots and deltas:
  `chatlist.reset/insert/remove/move/change`, `feed.reset/insert/remove/change`,
  `chat.status`, `auth.state`, `toast`, `file.progress`, `ai.chunk`; replies
  `{"re": n, "ok": …}`.
- Text is sent as `{"text": "...", "spans": [[start, end, kind, arg], …]}` in code points
  (the existing UTF-16 map and entity logic in `store/richtext.py` produces them).
- Files are paths under `td_files/` (the front end reads them itself; read-only).
- A recorded session of the protocol is the test fixture for both ends.

## Parts

Each part is about the same size (one to two weeks of a session), ends in something you can
see, and has a **done when**.

### Part 1 — TDLib for Zoraya (this repository; can start now)

Cross-build TDLib at tgclient's pinned commit (`42e6a525…`, 1.8.67) with `zc++` and
`ports/cmake/zoraya.cmake`, out of tree:
- host step: `prepare_cross_compiling` with the host compiler (TL generators, `gperf`);
- target step: `-DCMAKE_TOOLCHAIN_FILE=<zoraya>/ports/cmake/zoraya.cmake`,
  `OPENSSL_ROOT_DIR`/`ZLIB_ROOT` into `<zoraya>/build/ports/deps`, static, `tdjson_static`;
- patches (kept small, in `patches/td/`): platform detection for Zoraya (or rely on the
  `__unix__` → select fallback, §3.1); `EventFdBsd` over `pipe2` instead of `socketpair`;
  `PRAGMA locking_mode=EXCLUSIVE` before WAL in `SqliteConnectionSafe.cpp`; stubs for any
  missing call the link reports (affinity, `sysconf` names).
- a probe program `td-probe` (C++, ~100 lines, `td_json_client`): starts a client on the
  **test DC**, prints the TDLib version and the authorization states.

**Done when:** on Zoraya (QEMU), `+net 149.154.160.0/20:443 +net 91.108.0.0/16:443 +w
/tmp/td td-probe` prints the version and reaches `authorizationStateWaitPhoneNumber` on the
test DC, and closes cleanly; the build is one script, reproducible from a clean checkout.
Measure: binary size, resident memory after reaching the state.

Zoraya needs: nothing (R5 `poll` optional).

### Part 2 — `python3` with `_tdjson` (this repository; can start after part 1)

- `_tdjson.c` (~150 lines): `create_client_id()`, `send(int, bytes)`,
  `receive(float) -> bytes | None` (releases the GIL), `execute(bytes) -> bytes | None`.
- Build Zoraya's CPython recipe privately with a `Setup.local` naming `_tdjson` and the
  TDLib archives, linked with `zc++` (libc++ for TDLib).
- A ~30-line `TdJson` replacement for tgclient's `td/tdjson.py` that uses `_tdjson`.

**Done when:** on Zoraya, `python3-tg -c` running tgclient's own `TdHub`/`TdClient` with the
new binding reaches `authorizationStateWaitPhoneNumber` on the test DC, with the asyncio loop
and the `td-receive` thread working together; `selftest.py`'s TDLib checks (version,
`parseMarkdown`) pass.

Zoraya needs: **R1** (a hook for built-in modules); without it the recipe is copied and
kept in step by hand.

### Part 3 — `tgd` on the host (tgclient + this repository; can start now, in parallel with 1–2)

- In tgclient (architect's go-ahead): `tgclient/vm/` — Qt-free view-models extracted from
  `models/` and the logic controllers, starting with chat list, messages, composer, auth,
  search, AI; Qt shims keep the desktop client working; spans emitter beside the HTML one.
- `tgd` entry point: `asyncio.run`, the event bus → JSON lines, the fork-and-exec of the
  front end, logging to a file, the single-instance lock directory.
- The protocol spec (`docs/protocol.md`) and a line-mode test client (`tg-line`, Python)
  that prints the chat list and a chat and sends text.

**Done when:** on macOS/Linux, `tgd` + `tg-line` log in to a real account, list chats, open
one, page older history, send and receive a message; tgclient's test suite still passes with
the shims; a recorded protocol session is committed (no message content: test DC).

Zoraya needs: nothing.

### Part 4 — `tgd` on Zoraya (this repository; after parts 2 and 3)

- `vault.py`: AES-GCM through `_hashlib`'s OpenSSL (a tiny `_aead` addition to the
  built-in module) instead of `cryptography`; `TGC_KEY_STORE=file` (no keyring).
- numpy and fastembed imports made optional (keyword search only); `services/updates.py`
  off; `platformdirs` replaced by `TG_DATA_DIR`; httpx with `ssl.create_default_context()`
  (Zoraya's roots).
- Pure-Python packages vendored into `tgd`'s `site` directory; one install script that puts
  `python3-tg`, the library and `tgd` on the volume.
- The command line: `+net` words for the DC prefixes and `openrouter.ai:443`, `+w ~/tg`.
- TDLib media cache bounded (`optimizeStorage` with a size at start).

**Done when:** on Zoraya, `tgd` with `tg-line` (piped) logs in to a real account, stays
connected for an hour (updates arriving), sends a message, and an AI summary of a chat with
AI turned on streams back from OpenRouter; memory measured with a large account and written
down.

Zoraya needs: R2 (to stop typing the grants), R8 (until then the lock directory).

### Part 5 — `tg-term`, the terminal front end (this repository)

C++ (`zc++` on Zoraya, the host compiler on the host), ncurses, one client core:
login screens; chat list with folders, unread counts, typing; chat view with text and
entities (bold, italic, code, links, spoilers), replies, forwards, service messages,
documents and photos as one-line placeholders (name, size, "saved to …"); composer with
reply/edit, drafts, mentions; in-chat and global search; the AI summary panel; settings
(AI, notifications in-app, privacy, sessions). Emoji as short names where the terminal
cannot draw them.

**Done when:** on the host, `tgd` + `tg-term` cover §2's **[core]** list for text chats; on
Zoraya after stage 10 parts 10a–10d are merged, the same binary runs in Zoraya's terminal and
a whole day of text use is possible.

Waits for: stage 10 10a–10c (terminal for POSIX programs) and 10d (ncurses). Development and
host tests can start as soon as part 3's protocol is fixed.

### Part 6 — The rest of the logic in `tgd` (tgclient + this repository)

The **[later]** items that need no new Zoraya feature: forums and comments, bot keyboards,
polls creation, scheduled messages, folders editor, contacts and new chats, group admin,
proxies, passcode lock, several accounts, the other AI screens (person summary,
translation, reply suggestions, digest), person messages.

**Done when:** each item has a protocol test, and `tg-term` exposes it.

### Part 7 — A window front end (waits for R4, or R3)

The client core of part 5 with a second renderer over `zoraya_window_*` and `zoraya_text_*`
(as NetSurf): libjpeg and libpng (already ported for NetSurf) and libwebp linked into the
front end; avatars, photos, static stickers, the media viewer; mouse; clipboard and the
chooser when R12 lands.

**Done when:** the same feature list as part 5 in a Zoraya window, with photos and static
stickers drawn.

Waits for: **R4** (a window for a volume program); R7 (emoji) to look right; R12 for files.
If R3 lands instead, the renderer moves to `zoraya-ui` but the client core and protocol stay.

### Part 8 — Media (waits for Zoraya)

TGS stickers (rlottie, C++), WebM stickers (libvpx), voice notes (Opus + R9), GIFs and video
(an H.264 decoder — only if Zoraya ever gets video), system notifications (R6).

## What can start now, and what waits

| Part | Can start | Waits for |
|---|---|---|
| 1 TDLib | **now** (C++ stage is tagged) | — |
| 2 `_tdjson` + `python3` | after 1 | R1 makes it clean |
| 3 `tgd` on the host | **now** | architect's go-ahead for `tgclient/vm/` |
| 4 `tgd` on Zoraya | after 2 and 3 | R2, R8 nice to have; 10h not needed (`tgd` is the line's program) |
| 5 `tg-term` | host work after 3's protocol | **stage 10 10a–10d** to run on Zoraya |
| 6 rest of logic | after 3 | — |
| 7 window | after 5 | **R4** (or R3), R7, R12 |
| 8 media | — | R6, R9, video |

TASK-33 (browser-engine foundations: `AF_UNIX`, `poll`, shared memory) does not exist as a
task yet (§3); **nothing here waits for it**: TDLib is patched to need neither
`socketpair` nor shared `mmap`, and `select` replaces `poll`.

## The generic requests for Zoraya (to pass on)

Worded to name no program; details in §3.9.

1. **R1** A documented hook to add built-in C/C++ modules to `python3` (`Setup.local`, extra
   link archives, a C++ link).
2. **R2** A user-written description (or saved grant line) for a program installed on the
   volume: `+net`, `+w`, and after 10g a `data` line.
3. **R4** A window for a POSIX program on the volume (a `+window` word, or `window` in the
   user description of R2).
4. **R7** An emoji font and wide-character handling in the text stack and the terminal grid.
5. **R6** A desktop notification interface.
6. **R8** `fcntl` record locks across processes, and `flock`.
7. **R12** Clipboard and the file chooser for POSIX programs with a window.
8. **R9** Audio output (then input) for POSIX programs.
9. **R5** `poll` in libposix.
10. **R3** Native `zoraya-app` programs built and installed outside the image.
11. **R10**, **R11** Shared file-backed `mmap`; `socketpair`/`AF_UNIX` (already on
    TASK-30's list; not needed here).

## First part to start

**Part 1 (TDLib for Zoraya).** It carries the largest unknown (TDLib's portability layer
against libposix, its memory on a 512 MiB guest), needs nobody's go-ahead and no stage 10
part, and everything else stands on it. Part 3 can run beside it on the host once the
architect agrees to the `tgclient/vm/` split.

## Risks

- **Memory**: TDLib with its message database on a large account, in a system without
  demand paging; measured in parts 1 and 4. Fallback: `use_message_database = false`
  (slower history, less disk and memory).
- **Single CPU, `no-asm` OpenSSL**: MTProto crypto and JSON on one core under QEMU TCG may be
  slow at login (initial sync); measured in part 4.
- **The select back end** has an `FD_SETSIZE` cap (1 024); TDLib's sockets per account are
  few, so this is fine, but it is the reason for R5.
- **TDLib's API drifts** between versions; tgd stays on tgclient's pinned commit and moves
  only with it.
- **Policy**: Telegram's API terms require an own `api_id` per application; the architect's
  existing one is used, never committed.
