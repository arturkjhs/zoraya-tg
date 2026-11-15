# TG-03 report: `tgd`, tgclient's backend, on the host and on Zoraya

## Status

| | |
|---|---|
| **Result** | **Done up to the logins**: everything that needs no Telegram account works and is tested, on the host and inside Zoraya under HVF. The logins with an account (host, then Zoraya) are the architect's hand check, below. |
| Why no test-number login | Telegram disabled the test accounts (`99966XYYYY`, code `XXXXX`): TDLib's maintainer, [tdlib/td#3083](https://github.com/tdlib/td/issues/3083), 2026-01-05: "Test accounts were disabled due to extensive abuse of them." Every such number now gets `PHONE_CODE_INVALID`; a fresh one (9996627391, DC 2, code 22222, `via Sms`, length 5) did here too, with a wiped data directory. tgd passes the phone and the code unchanged and TDLib keeps the `phone_code_hash` itself; nothing to fix in tgd. The test DCs now take real numbers whose account was made in an official mobile app on the test servers. |
| tgd | 0.1.0, protocol 1 (`docs/tgd-protocol.md`); tgclient at `ccba7583` (code only, packed into `out/tgd` at build time, never into this repository) |
| TDLib | 1.8.70 (`c8828d50`), as TG-02, with patches 0004 and 0005 new |
| Host | `make host`: `out/host/bin/tgd` (Homebrew python3.14, a venv with `cryptography` by hash, `libtdjson.dylib` 24 MB built from the same TDLib tree); `make test-tgd-host` passes (11 checks); `tgd --test-dc` reaches `authorizationStateWaitPhoneNumber` and its round trip to the test DC works (an error reply from Telegram) |
| Zoraya | `make smoke-tgd` **PASS: 55 steps**, twice (no network: the refusal, the database written and reopened, the protocol, a restart that finds the credentials). `make smoke-tgd-net` **PASS: 78 steps** (QEMU's network open: the exact line, `grants`, TDLib's round trip to Telegram's test DC through the grant, memory, the text front end). The data on a kept disk survives a reboot (two boots, checked once by hand with the runner). |
| Memory | **72 MiB** (`top`: 73701K in a room of 640M) for tgd connected to the test DC, at `wait_phone`. Logged in: to be measured at the hand check (below). Zoraya at `--mem 512M` leaves 339 MiB to the session; `make smoke-tgd-net` and `make run-tgd` boot with 1 GiB so that a room's limit is not the bound. |
| Patches | TDLib: 0004, 0005 new (5 in all); Zoraya: 0 |
| Requests | `docs/requests.md`: R15 to R19 new |
| Secrets | none read, printed, logged or committed. The api file (`~/.config/zoraya-tg/api`, the architect's) is read by tgd only; the tests use the placeholder api id 1 and the public test number 9996621234. |

## The line on Zoraya

The line a person types (exact, from `tgd/tgd/telegram-cidr.txt`, Telegram's list fetched 2026-11-14, SHA-256
`3ecd072c…6ec651`; `tgd --grant` prints it, `tgd --grant --test-dc` with `--test-dc` at the end):

```
+net 91.108.56.0/22:443 +net 91.108.4.0/22:443 +net 91.108.8.0/22:443 +net 91.108.16.0/22:443 +net 91.108.12.0/22:443 +net 149.154.160.0/20:443 +net 91.105.192.0/23:443 +net 91.108.20.0/22:443 +net 185.76.151.0/24:443 +net openrouter.ai:443 +w ~/tg python3 /usr/lib/tgd/tgd.py
```

What `grants` shows for it (from `make smoke-tgd-net`'s log):

```
python3 gets (a POSIX program):
  in, out, err         the terminal
  ~                    read (current directory)
  ~/tg                 change anything in (+w)
  /system/tls          read (with the network)
  /bin /usr /etc       read (system)
  /proc/self/exe       read (system)
  /tmp                 change anything in (this line's own)
  /dev/null
net: 91.108.56.0/22:443      reach (grant +net)
net: 91.108.4.0/22:443       reach (grant +net)
net: 91.108.8.0/22:443       reach (grant +net)
net: 91.108.16.0/22:443      reach (grant +net)
net: 91.108.12.0/22:443      reach (grant +net)
net: 149.154.160.0/20:443    reach (grant +net)
net: 91.105.192.0/23:443     reach (grant +net)
net: 91.108.20.0/22:443      reach (grant +net)
net: 185.76.151.0/24:443     reach (grant +net)
net: openrouter.ai:443       reach (grant +net)
```

Exactly the nine IPv4 ranges on 443, OpenRouter by name on 443, and `~/tg`. Notes:

- **`python3 /usr/lib/tgd/tgd.py`, not `tgd`.** zosh gives a POSIX program its `+net` words only under a
  description the image carries (ADR-0153 rule 10, INV-0153-6); the image describes `python3`, not `/bin/tgd`,
  so `+net … tgd` runs with no network and `grants` lists none. It is the same program (`python3` has `_tdjson`
  and `_aead` built in), and `/bin/tgd` stays for the lines without `+net`. `image/describe/tgd.cmd` is tgd's
  description, ready for the image (request R15); with it, the line ends in `tgd`.
- **Start it from `~`** (the shell's start). TDLib takes the realpath of its database directory, which needs `~`
  readable: from `~/tg` (where `room NAME` with `+w ~/tg` puts you) it fails with `Realpath failed`.
- **Port 443 only.** TDLib reaches the data centres on 443; the list's IPv6 ranges are left out (Zoraya is IPv4 only).
- **Typed input is shown.** zosh gives a program no terminal, so tgd cannot turn echo off: the api hash and a 2FA
  password stay on the screen as typed (R18). tgd says so at the prompt.

## The architect's hand check

### Host (the real account, production DCs)

```
~/workspace/zoraya-tg/out/host/bin/tgd                 # reads ~/.config/zoraya-tg/api; data in ~/.local/share/tgd/prod
tgd> login                                             # phone (+…), then the code Telegram sends, then the 2FA password if set
tgd> chats                                             # the first 20 chats, numbered
tgd> history 1 10                                      # chat 1 of the list (or me, an id, part of a title)
tgd> send me hello from tgd                            # me: Saved Messages
tgd> history me 3
tgd> quit
~/workspace/zoraya-tg/out/host/bin/tgd                 # again: "logged in as …" with no login
tgd> status
tgd> quit
```

(With a real number on the test servers, made in an official app's test mode: add `--test-dc`; data in
`~/.local/share/tgd/test`.)

### Zoraya (under HVF, with the network)

```
make run-tgd                                           # boots Zoraya, 1 GiB, a disk kept in build/run (FRESH=1: a new one)
~> mkdir tg
~> grants <the line above>                             # check it against the listing above
~> room new tgdroom --mem 640M +w ~/tg                 # only to measure memory
~> room tgdroom
~> cd ~
~> <the line above>
tgd> login                                             # api id, api hash (shown as typed), phone, code, 2FA (shown)
tgd> chats
tgd> history 1 10
tgd> send me hello from Zoraya
tgd> (Ctrl-Z)  then  top  then  fg                     # the memory of a logged-in tgd (the tgdroom row)
tgd> quit
~> <the line above>                                    # again: "logged in as …" with no login
tgd> quit
~> poweroff
make run-tgd                                           # the same disk: still logged in
```

Then the logged-in memory goes in the status block, and the account's own data stays in `build/run` (git-ignored
`build/`), which `make clean` does not remove; delete `build/run/tgd-disk.img*` to remove it.

## Sizes

| | |
|---|---|
| `/bin/tgd` | 65.4 MB (`python3` with `_tdjson` and `_aead`, linked with tgd's `main`) |
| `/bin/python3` | 65.4 MB (the same objects; it runs the line above) |
| `/usr/lib/tgd` | 3.4 MB: tgd, tgclient's Qt-free subset (199 `.py` files in all, without `ui/`, `models/`, embeddings, search, updates), the wheels (httpx, httpcore, h11, anyio, idna, certifi, python-dotenv, platformdirs, typing-extensions, pinned to tgclient's `uv.lock` in `ports/wheels.lock`), the `cryptography` stand-in over `_aead` |
| `out/root` | 168 MB; the disk is made at 384 MB |
| Host | `libtdjson.dylib` 24 MB; the venv |
| Build | TDLib's cross build 15 min at `-j2` (TG-02); the host `libtdjson` 15.5 min |

## Patches

| | Why |
|---|---|
| `patches/td/0004-sqlite-unix-excl-vfs-and-getrandom-on-zoraya.patch` | **SQLite WAL.** The task's proposal, `locking_mode=EXCLUSIVE`, does not fit TDLib: it opens one connection to the same database per scheduler thread, and an exclusive lock taken by the first refuses the others. SQLite's own `unix-excl` VFS gives the same effect per process: one process lock, the WAL index in heap memory shared by that process's connections, no `-shm` mapping. On Zoraya it is made the default VFS. The patch also takes SQLite's randomness from `getrandom` (`/dev/urandom` is not given to a program). Checked by `tests/tdjson_db.py` in `make smoke-tgd`: `td_test.binlog` and `db_test.sqlite` written, no `-shm`, opened again. |
| `patches/td/0005-no-builtin-ipv6-dc-addresses-on-zoraya.patch` | TDLib parses its built-in IPv6 data centre addresses at start and asserts on a failure; Zoraya's `inet_pton` does not parse IPv6, so TDLib aborted (signal 6). On Zoraya those addresses are skipped (IPv4 only, ADR-0225). R16. |
| File locks | **No patch.** TDLib's binlog lock is `fcntl(F_SETLK)` (no `flock`), which Zoraya has within a process (ADR-0319). One tgd per data directory is tgd's own lock: a directory made with `mkdir` (`tgd.lock`, with the pid; exit 5 for a second tgd). |

Not patches, but the program's shape on Zoraya:

- **`/bin/tgd` is an ELF of its own** (`tgd/tgd_main.c`, `ports/tgd-link.mk`): zosh starts ELF programs only (no
  `#!`), and a launcher that `exec`s `python3` leaves it without random bytes (ADR-0310), so CPython stops at
  `_Py_HashRandomization_Init`. `tgd_main.c` is Zoraya's `Programs/python.c` (the 2 MiB thread, signals blocked)
  calling `Py_BytesMain(-I /usr/lib/tgd/tgd.py …)`.
- **`_aead`** (`modules/_aead.c`, AES-GCM over OpenSSL's EVP) in place of `cryptography`'s, which has Rust in it:
  tgclient's `vault.py` imports `AESGCM` from a stand-in package on `sys.path` on Zoraya only. Checked against
  NIST's GCM test case 4.
- **Standard input by `os.read`** in a thread: on zosh's stream `input()` stalls, `os.read` returns each line.

## Generic requests added (`docs/requests.md`)

- **R15**: descriptions of POSIX programs from a port at build time (a ported program's `+net`).
- **R16**: `inet_pton(AF_INET6)` in libc.a.
- **R17**: `getrusage`'s `ru_maxrss` (a program's own memory; today 0, and `top` counts only rooms).
- **R18**: reading a line without echo.
- **R19**: the image's SQLite takes randomness from `getrandom` (the `/dev/urandom` hint on every database).
- Not requested: `getrandom` for an exec'd program (ADR-0310, by design) and `#!` (closed by `/bin/tgd`).

## The test harness

- `make test-tgd-host`: `tests/tgd_protocol.py` (the protocol with no credentials, the lock, the grant line).
- `make smoke-tgd`: `tests/smoke-tgd.script`, no network.
- `make smoke-tgd-net`: `tests/smoke-tgd-net.script`, the Internet (QEMU's `--net --net-open`), no account: the
  placeholder api id gets `API_ID_INVALID` from Telegram's test DC in about 3 s, which is the proof of the round
  trip. The line is made by the host tgd and put in by `tests/expand_script.py`, which sends lines longer than 64
  bytes in pieces 300 ms apart: the console's input buffer drops what overflows when the runner types a whole
  line at once (`[input: N bytes dropped]`); a person typing does not hit it.

## What `tg-term` will need

- **The protocol** (`docs/tgd-protocol.md`): on the host, start `tgd --json` as a child, write requests, read
  events; match replies by `id`; requests may overlap. The `hello` event first, `bye` last.
- **Login as a dialog**: `auth.state` events with what is asked (`wait_code`: `via`, `length`, `next`), answered by
  `auth.phone` / `auth.code` / `auth.password`; `auth.error` for Telegram's refusals. Hidden input needs R18 (or
  `tg-term`'s own key handling, which a full-screen program has anyway).
- **Live updates**: today `message.new` for the one open chat. For a chat list that moves (unread counts, new
  chats, edits, deletions, typing), tgd needs more events from tgclient's stores (`chat.update`, `message.edit`,
  `message.delete`); the stores have them, the protocol does not carry them yet.
- **Paging**: `messages.history` gives the newest N; a scrolling view needs `before` (a message id).
- **Files**: photos and documents are previews (`kind`, text) only; downloads (`FileManager` is there) need ops and
  a place under `~/tg`.
- **One process, not two.** A program that another program starts gets no random service on Zoraya (ADR-0310),
  so CPython, and with it tgd, cannot start as `tg-term`'s child there. Either `tg-term` is a Python front end in
  tgd's process (a third front end beside `TextFrontEnd` and `JsonFrontEnd`, over the same `Dispatcher`: the
  shape tgd already has), or Zoraya gives a started program randomness (not requested: ADR-0310 decided it). On
  the host the JSON protocol over a pipe works as is.
- **The line**: `tg-term` is then started by the line above (its `+net` words and `+w ~/tg`).
- **Memory**: about 72 MiB before login; plan the room at 256 MiB until the logged-in figure is in.
