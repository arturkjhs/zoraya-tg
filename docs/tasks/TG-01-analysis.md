# TG-01: How tgclient can run on Zoraya — analysis and plan (documents only)

## Where and how this runs

On the Mac mini, as a **third session beside the stage 10 dispatcher, building nothing** (no `make`, no `cargo`, no QEMU, no TDLib build): the two build slots belong to TASK-31. Model: **Opus**; Haiku subagents may fetch and summarise public sources. The architect is travelling: **do not ask; decide, record, continue.**

Work in a clone of the **private** repository `arturkjhs/zoraya-tg` at `~/workspace/zoraya-tg`. Read, never write:
- `~/workspace/tgclient`: the architect's Telegram client (Python, PySide6, TDLib). **Read-only.** Do not run it, do not change it, do not commit in it, do not copy it.
- `~/workspace/zoraya-8` (or a fresh read-only look at `origin/main` of `arturkjhs/zoraya`): what Zoraya has.

## Secrets and privacy (hard rules)

- **Never copy, quote, print into the conversation or commit:** API id or hash, bot or OpenRouter tokens, phone numbers, TDLib databases or session files, files under any `tdlib`/`td_files`/data directory, `.env` files, keychains, logs with message content, user names or chat names from data. If a file may hold one of these, do not open it; name it in the report as "not read".
- Quote tgclient's **code** only as far as the analysis needs (function names, short excerpts), never configuration values.
- **Nothing about Telegram goes into the public repository `arturkjhs/zoraya`:** no file, branch, commit message, ADR, issue or comment. Changes Zoraya itself needs are written in this repository as **generic requests** (for example "`poll` in libposix", "a hook to add built-in modules to `python3`"), worded so that they name no program, for the architect to pass on.

## The question

tgclient is to run on Zoraya **without porting Qt**: keep TDLib and the Python logic, replace the PySide6 layer. The working plan (to be confirmed or changed by this analysis):

- **`tgd`**, the backend: tgclient's Python code without Qt (TDLib through a built-in `_tdjson` module linked into a static `python3`, since Zoraya has no `ctypes` and no `dlopen`; the AI features over HTTPS); Qt's signals, `QThread`s and timers replaced by `asyncio` and threads.
- **`tg`**, the front end: a native Zoraya window in Rust over `zoraya-ui`/`zoraya-app` (as Files and Edit are), talking to `tgd` by JSON messages over a channel; or, as a faster first step, a full-screen terminal UI with curses after Zoraya's stage 10 (part 10d).
- The window never holds the network; `tgd` never holds the screen.

## What to write (`docs/analysis.md`, then `docs/plan.md`)

1. **tgclient's structure:** modules, their lines of code, and which are UI, which logic, which glue; where Qt reaches into the logic (signals in models, `QThread` workers, `QSettings`, `QImage` decoding, `QNetworkAccessManager`, timers); how it talks to TDLib (the binding, `td_json_client` or another, threads and the receive loop); the TDLib methods and updates it uses; its dependencies besides Qt (with versions and whether each is pure Python); its files on disk (by kind, not by content); its network peers (Telegram, OpenRouter, others) by host and port.
2. **What the UI does,** screen by screen, as a list the native front end must cover: chat list, chat view, message kinds (text, entities, replies, forwards, photos, stickers and their formats, voice, video, documents), input, search, the AI summary screens, settings, notifications.
3. **What Zoraya has and lacks for it,** checked against `docs/design.md`, `docs/adr/index.md` and the TASK-27, TASK-30 and stage 10 plan documents: TDLib's build needs (C++17, OpenSSL, zlib, threads, sockets, `poll`/`select`, `eventfd` or `socketpair`/`pipe`, file locking, `mmap`), Python's (`asyncio`, `ssl`, `sqlite3`), images (which decoders the front end needs: JPEG, PNG, WebP, and animated stickers as TGS/Lottie and WebM), fonts and emoji, the network grant (Telegram's data centres are addresses, not names; ports), a program's own directory (ADR-0341, stage 10's 10g), notifications (none in Zoraya yet).
4. **The plan:** parts of comparable size, each with a visible result and a "done when": the TDLib port and `_tdjson` (built outside the public repository, against Zoraya's `zc++` and CMake toolchain file); `tgd`; the front end; the generic Zoraya changes each part needs, as requests; what can start now and what waits for stage 10 or the browser-engine foundations (TASK-33: `AF_UNIX`, `poll`, shared memory).

## Git

Configured identity of the Zoraya work only. Commit and push to `arturkjhs/zoraya-tg` (`main` there is fine; it is the architect's private repository) after each step. Before every commit: `git diff --cached` checked for anything under the secrets rules. On a usage-limit warning: commit, push, write `docs/status.md`, stop.

## Definition of done

`docs/analysis.md` and `docs/plan.md` pushed; a final report: the share of tgclient that moves unchanged, the share rewritten, the front end recommended (native window or curses first) and why, the generic requests for Zoraya, the first part to start.
