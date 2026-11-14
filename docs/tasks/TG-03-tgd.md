# TG-03: `tgd` — tgclient's backend running on Zoraya, with a real login

## Where and how this runs

On the architect's **MacBook Air M1**, in `~/workspace/zoraya-tg` (private), with `~/workspace/zoraya-main` (Zoraya's `main`, built in TG-02) and a checkout of tgclient's code at `~/workspace/tgclient` (see "Before you start"). Model: **Opus**. `CARGO_BUILD_JOBS=3`, TDLib at `-j2`, one QEMU guest at a time. **Do not ask; decide, record, continue** — except for the login, which the architect does by hand.

Read first: `docs/analysis.md`, `docs/plan.md`, `docs/tdlib-report.md`, `docs/requests.md` in this repository, and tgclient's own docs on its layering.

## Secrets and privacy (hard rules, unchanged)

- Never read, copy, print, log or commit: api id or hash, phone numbers, login codes, 2FA passwords, OpenRouter or other keys, TDLib databases and files, session data, message content, chat or user names from real data. tgclient's `.env`, `url.txt`, data and log directories stay unread.
- Credentials reach `tgd` only **at run time, typed by the architect** at `tgd`'s own prompt inside Zoraya (or, on the host, from a file outside every repository that the architect creates, for example `~/.config/zoraya-tg/api`, read by `tgd` and never by you). `tgd` stores them only in its data directory, never echoes them, and its logs carry no credential and no message text.
- **Nothing about Telegram in the public `arturkjhs/zoraya`.** Zoraya changes stay generic requests in `docs/requests.md`.
- tgclient's code is the architect's own; it may be imported into `tgd`'s build from the checkout, but **this repository does not vendor a copy**: the build takes `TGCLIENT=<dir>` and packs the needed Qt-free packages into the image's directory under `build/`.

## Before you start

`~/workspace/tgclient` on this machine: if it does not exist, stop and write in `docs/status.md` that the architect must clone tgclient's repository here (code only, no data). Do not copy anything from the Mac mini.

## What to build

1. **`tgd`**: tgclient's Qt-free layers (TG-01's 37 % that moves unchanged and the 8 % with small changes: TDLib's async client, the login flow, the stores, the services, `vault.py` without the keyring and with an AES-GCM that the static `python3` has, numpy optional), started as one program, with:
   - a **TDLib binding switch**: tgclient's `ctypes` binding on the host, `_tdjson` on Zoraya, behind one small interface;
   - a **line protocol on stdin/stdout** (JSON lines) for front ends, and a minimal **text front end for testing** in the same program (`tgd` alone in a terminal): `login`, `chats [N]`, `open CHAT`, `history CHAT [N]`, `send CHAT TEXT`, `summary CHAT` (the AI summary over OpenRouter, only if a key was typed), `quit`. This is not `tg-term`; it is what proves `tgd` and what a person can already use.
   - The Qt event loop, signals, `QThread`s and timers replaced by `asyncio` and threads, as TG-01's plan says.
2. **On the host first** (macOS, the system's or Homebrew's Python with tgclient's own TDLib binding): `tgd` runs against Telegram's **test** data centres when `--test-dc` is given, so the architect can log in with a test number without touching the real account.
3. **On Zoraya** (under HVF, through `EXTRA_ROOT` and a disk large enough), with the patches TG-02 named and what it found next:
   - **file locks:** check how TDLib locks its files; libposix has `fcntl` record locks within a process (ADR-0319) and no `flock`; if TDLib uses `flock`, patch it to `fcntl`;
   - **SQLite WAL:** run TDLib's databases in exclusive locking mode so the WAL index stays in process memory (no shared mapping), as TG-01 proposed; verify with a real database that it opens, writes and reopens;
   - **the network grant:** Telegram's data centres by IPv4 ranges on the `+net` words (from Telegram's published CIDR list, fetched and pinned with its date in this repository), ports as TDLib uses them; IPv4 only (no IPv6 in Zoraya); OpenRouter by name on 443 for summaries;
   - **its data directory:** until Zoraya's own directory per program (ADR-0341, stage 10's 10g) exists, an explicit `+w ~/tg` on the line; the description in this repository's image directory says so;
   - **memory:** measure what one logged-in `tgd` uses; if 512 MiB is not enough, say how much and run with it.
   - The line a person types, written in the report, for example `+net 149.154.160.0/20:443 … +w ~/tg tgd` (exact, from the CIDR list), and what `grants` shows for it.

## Done when

- Host: `tgd --test-dc` reaches `authorizationStateWaitPhoneNumber`; the architect logs in by hand with a test number; `chats`, `history`, `send` work.
- Zoraya, under HVF: the same, inside a Zoraya terminal, with the exact line; the database survives `quit` and a new start without a new login; `grants` shows exactly the network ranges and `~/tg`.
- An automated `make smoke-tgd` in this repository that needs no account (start, protocol, a clean quit, the refusal without `+net`), run twice.
- `docs/tgd-report.md` with a status block: what works, the architect's hand-check steps (exact commands, both host and Zoraya), memory, sizes, the patches and why, the generic requests added, what `tg-term` will need.

## Git

Configured identity of the Zoraya work. Commit and push `zoraya-tg` after each step, `git pull --rebase` first, `git diff --cached` checked for secrets before each commit. Nothing in the public repository. On a usage-limit warning: commit, push, write `docs/status.md`, stop.
