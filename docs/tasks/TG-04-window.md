# TG-04: `tg` — a native Zoraya window for the Telegram client

## Where and how this runs

On the architect's **MacBook Air M1 (8 GB)**, in `~/workspace/zoraya-tg` (private). Model: **Opus**. **Do not ask; decide, record, continue.** The machine is shared with TASK-34 (a building dispatcher) and TASK-35: **one build on this machine at a time** — before any `cargo`, `make` or QEMU run, check that no other build is running (`pgrep -fl "cargo|rustc|qemu-system|make "`), and wait if one is. `CARGO_BUILD_JOBS=2`, one QEMU guest at a time.

Read first, in `zoraya-tg`: `docs/analysis.md`, `docs/plan.md`, `docs/tgd-report.md`, `docs/tgd-protocol.md`, `docs/requests.md`. In Zoraya (`origin/main`): `CLAUDE.md`, `docs/design.md` (GUI, tokens and text, windows and frames, the toolkit, the file manager, the wheel), ADR-0160 to ADR-0172, ADR-0317, ADR-0341, ADR-0396 to ADR-0399, ADR-0411 to ADR-0413, the code of `zoraya-ui`, `zoraya-app`, Files and Edit (the model to follow), and `docs/research/desktop/mockups/` (the look).

## Secrets and privacy (hard rules, unchanged)

- No api id or hash, phone number, code, password, key, session, database, message text, chat or user name from real data: not read, not printed, not committed. All data in this task is **synthetic** (invented names and messages, in English, Ukrainian and Russian, so Cyrillic is exercised).
- **Nothing about Telegram in the public `arturkjhs/zoraya`:** no file, branch, commit, ADR or comment. What Zoraya itself must change stays a **generic request** in `docs/requests.md`.
- Zoraya's source for this task is a **local clone with no GitHub remote**: `git clone ~/workspace/zoraya ~/workspace/zoraya-tgw && git -C ~/workspace/zoraya-tgw remote remove origin`, updated by `git pull ~/workspace/zoraya <commit of origin/main>`. Local changes to it (an overlay) live as patch files in `zoraya-tg/overlay/` and are applied there; they can never be pushed from it.

## Why two stages

A window needs things Zoraya is still building: a program installed on the volume with a window and saved grants (stage 10's 10g and 10j), Unix sockets and socket files so the window and `tgd` find each other (TASK-34's F2 and F7), and a way to build a native program outside the repository (requested; TASK-35 decides the route). **Stage A needs none of them and starts now.** Stage B starts when they land.

## Stage A (now): the window with a built-in sample backend

1. **Design** (`docs/tg-window.md`): the screens, drawn as text mockups and matched to Zoraya's tokens and the Field look: the chat list (avatar initials, name, last line, time, unread count, pinned, muted), the chat view (bubbles in and out, replies, forwards, edited, dates, links, service lines; photos, stickers, voice and files as labelled placeholders until Zoraya has decoders), the input (multi-line, send on Enter), search, the AI summary sheet, the login screens (phone, code, password with hidden input), settings; light and dark, scale 1 and 2; the keyboard (every action reachable), the wheel, selection and copy by gesture. And the **protocol**: what the window needs from `tgd` beyond `docs/tgd-protocol.md` (models with windows of rows: a chat list model, a message list model that loads by pages, updates as diffs), written so the Qt view-models of tgclient (TG-01's 40 %) map onto it.
2. **The program `tg`:** Rust, `no_std`, over `zoraya-ui`/`zoraya-app`, built **from `zoraya-tg`** against the crates of `~/workspace/zoraya-tgw` by path. Find the smallest way to do that; if it needs changes in Zoraya, keep them as patches in `overlay/` and write each as a generic request (native programs built outside the repository; a launch record and dock tile for a program on the volume). Behind one `Backend` trait: `SampleBackend` (compiled-in synthetic data, with a timer that delivers new messages, so updates are real) now, `SocketBackend` in stage B.
3. **In Zoraya:** an image through `EXTRA_ROOT` plus the overlay; `tg` starts from the dock or a terminal; the compositor draws its frame and pills (no network pill: the window holds none).
4. **Done when:** under HVF, scripted through Zoraya's GUI test runner (`make test-tg-window`, run twice): the chat list shows and scrolls with the wheel and the keys; opening a chat shows its messages with Cyrillic; a typed message appears in the view and in the list's last line; a message from the sample timer arrives and raises the unread count; search filters; light and dark and both scales render; the window resizes. Screens in `docs/shots/` (synthetic data only) and the measurements (memory in `top`, frame time while scrolling) in `docs/tg-window-report.md`.

## Stage B (when 10g, 10j, F2 and F7 are merged in Zoraya; check `origin/main`'s tags and reports)

1. **View-models in `tgd`:** tgclient's Qt view-models and controllers adapted (method bodies kept, Qt signals and list notifications to an event bus) under `tgd/vm/` in this repository, with a table in the report mapping each to its tgclient source, so the same change can later be made inside tgclient itself. They serve the protocol of stage A.
2. **`SocketBackend`:** `tgd` listens on a Unix socket file in its own directory (ADR-0341, ADR-0412); `tg` connects to it; the grant that lets `tg` reach that one socket is on its record (ADR-0397). `tg` holds no network and none of `tgd`'s files; `tgd` holds no window.
3. **Installed:** both as programs on the volume (ADR-0396 to ADR-0399): `install`, the saved grants (the nine IPv4 ranges on 443, `openrouter.ai:443`, the own directory), `programs show`; the overlay shrinks to nothing or to what is still a request.
4. **Done when:** with the sample account data replaced by a real login **done by the architect** (hidden input in the window's password field): chats, history, sending, the AI summary; a restart keeps the login; `grants`/`programs show` list exactly what each program holds; the report's hand-check steps are exact.

## Report and git

`docs/tg-window-report.md` with a status block at the top: what works, screens, measurements, the overlay's patches and the generic request each stands for, what stage B waits for. Configured identity of the Zoraya work; commit and push `zoraya-tg` after each step (`git pull --rebase` first, `git diff --cached` checked for secrets). On a usage-limit warning: commit, push, write `docs/status.md`, stop.
