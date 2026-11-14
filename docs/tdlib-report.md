# TG-02 report: TDLib on Zoraya, and `_tdjson` in `python3`

## Status

| | |
|---|---|
| **Result** | **Done.** TDLib cross-builds for Zoraya; `python3` with the built-in module `_tdjson` runs inside Zoraya under HVF; both smoke checks pass (`make smoke`: `PASS: 25 steps`). |
| TDLib | commit `c8828d50f5eac2fc7089ce1089d7ce9095c61b5e` (CMake says 1.8.70; no release tag since v1.8.0), archive SHA-256 in `ports/sources.lock` |
| Zoraya | checkout `~/workspace/zoraya-main`, detached at `origin/main` `0c3ada97` (toolchain, sysroot, OpenSSL 3.5.9, zlib 1.3.1, CPython 3.15.2 recipe, kernel, image tools), used read-only except for its build outputs |
| Host | MacBook Air M1, 8 GB, macOS 26; Apple clang 21; CMake 4.5.0 (Homebrew); gperf 3.0.3 (macOS's own) |
| What builds | the 12 static archives of TDLib's JSON client (`tdjson_static` and what it needs) with `zcc`/`zc++` and Zoraya's toolchain file; `libtdshim.a`; `python3` with `_tdjson`, OpenSSL, zlib, libc++ |
| Sizes | TDLib's archives together 156.5 MB (149 MiB; `libtdcore.a` 116 MB of it). `python3` with `_tdjson`: 65.4 MB file, **39.4 MB loaded** (r-- 10.2 MB, r-x 27.5 MB, rw- 1.7 MB); Zoraya's own `python3`: 15.6 MB file, 12.3 MB loaded. So TDLib adds about 27 MB to the loaded program. The image part (`out/root`: `python3`, its library, the tests) is 101 MB; the volume is made at 256 MB. |
| Build time on the M1 | clean `make td` (unpack, patch, host build of TDLib's generators, cross configure and build at `-j2`): **945 s wall** (1639 s user), part of it alongside the python build. Zoraya's python recipe into this repository: about 11 min (with the host python). Linking `python3` again with `_tdjson`: about 1 min. `make smoke` (disk, boot, both checks, power-off): 18 s. |
| Patches | 3 to TDLib (`patches/td/`), below; 0 to Zoraya |
| Shim | `ports/tdshim.c`: `poll`, `pread`, `pwrite`, `usleep`, `mkdtemp`, `in6addr_any` |
| Requests | `docs/requests.md`: R1, R5, R11 (TG-01's numbers, with TG-02's evidence and tests), R13, R14 new |
| Secrets | none used or stored: the tests need no account (`td_execute`, and a client closed at `authorizationStateWaitTdlibParameters`) |

## How to build and run

```
make fetch                # the pinned TDLib archive into build/src (checked against the lock)
make td                   # TDLib for Zoraya: build/td-zoraya, archives in out/lib
make python               # python3 with _tdjson: build/python3-tdjson
make root                 # out/root: bin/python3, usr/lib/python3.15, home/*.py
make smoke                # the smoke test inside Zoraya under HVF; transcript in build/logs/smoke.log
```

`ZORAYA=` names the Zoraya checkout (default `~/workspace/zoraya-main`). It must have run `make fetch-ports` and
`make -f ports/Makefile cxx zlib openssl python` (the python one also builds the host python this recipe reuses).

## TDLib's build

- **Generated sources.** TDLib generates C++ from its TL schemas and MIME tables (by `gperf`) with programs it builds
  itself, which a cross build cannot run. `make td` builds them first with the host's clang in `build/host/td-gen`
  (target `prepare_cross_compiling`, against Homebrew's OpenSSL), which writes the generated files into the source
  tree; the cross build then uses them.
- **Cross build.** `ports/zoraya-tg.cmake` includes Zoraya's `ports/cmake/zoraya.cmake` (through the environment
  variable `ZORAYA`: CMake reads a toolchain file again in each `try_compile`, and a cache variable did not reach
  it) and adds Zoraya's `build/ports/deps` to `CMAKE_FIND_ROOT_PATH`, so `FindOpenSSL` and `FindZLIB` find the ports'
  static libraries and nothing of the host. `MinSizeRel`, no JNI or .NET. Every compile line in
  `compile_commands.json` runs `zcc` or `zc++`, and none defines `__linux` (checked by `make td`).
- **The archives are linked as they are.** A single merged archive lost members: extracting and archiving again
  keeps one of each name, and two libraries have a `misc.cpp.o` while `libtde2e.a` has two `e2e_api.cpp.o` (the
  link then lacked `tde2e_api::*`). lld resolves across archives in any order, so the twelve go to the link as
  they are.

## What TDLib needed from Zoraya's libc.a, and how each gap was closed

| Need | Where | Closed by |
|---|---|---|
| A platform: `zcc` predefines `__zoraya__` and neither `__linux__` nor `__unix__`, so `platform.h` stops with `#error` | `tdutils/td/utils/port/platform.h`, `config.h` | patch 0001: `TD_ZORAYA`, which takes `TD_POLL_POLL` (the poll(2) loop) and `TD_EVENTFD_BSD` |
| A wake-up for the poll loop: no `eventfd`, no `socketpair` | `tdutils/td/utils/port/detail/EventFdBsd.{h,cpp}` | patch 0001: `TD_EVENTFD_PIPE`, the BSD event fd over a non-blocking `pipe` with `FileFd` ends (a `SocketFd` would `setsockopt` on a pipe and fail); request R11 |
| `poll` (only `pselect` is exported) | the poll loop, `EventFdBsd::wait` | `tdshim.c`, `poll` over `pselect` (at most 1024 descriptors; readability and hang-up both arrive as readable, the read that follows tells them apart); request R5 |
| `select` | | Zoraya's `libzshim.a` (already in python3's link): `select` over `pselect` |
| `<unistd.h>` in SQLCipher's part of TDLib's SQLite, included only under `__unix__`/`__APPLE__` | `sqlite/sqlite/sqlite3.c` | patch 0002: `|| defined(__zoraya__)` |
| `sendmsg` (scatter write to a socket with `MSG_NOSIGNAL`) | `SocketFd::writev` | patch 0003: Zoraya takes TDLib's own `writev` path, as Android does. A write to a closed socket may then raise `SIGPIPE` where `sendmsg` would not. The library does not ignore `SIGPIPE` (only its `cli.cpp` does), but CPython sets it to `SIG_IGN` at start, so inside `python3` the write fails with `EPIPE` instead |
| `pread`, `pwrite` | `FileFd` (the binlog, the files) | `tdshim.c`: `lseek`, `read`/`write`, `lseek` back, under a lock; not atomic against a plain `read` of the same descriptor in another thread; request R13 |
| `usleep` | SQLite's busy wait | `tdshim.c`, over `nanosleep`; request R14 |
| `mkdtemp` | `tdutils/td/utils/port/path.cpp` | `tdshim.c`, over `getrandom` and `mkdir`; request R14 |
| `in6addr_any` | `IPAddress::get_any_addr` (IPv6) | `tdshim.c`, the constant; Zoraya is IPv4 only (ADR-0225), no request |
| `bind`, `listen`, `accept` | `ServerSocketFd` (a listening socket, which the JSON client never makes) | Zoraya's `libzshim.a`: they fail with `ENOSYS`; no request |
| `__libc_current_sigrtmin` (musl's `SIGRTMIN`) | `ThreadPthread::send_real_time_signal`, which nothing calls | Zoraya's `libpyshim.a` (35, musl's value); no request |

Everything else TDLib calls is there: pthreads (with `pthread_setname_np`), `pselect`, `pipe`, `fcntl(F_SETFL)`,
`clock_gettime`, `getaddrinfo`, `socket`/`connect`/`send`/`recv` (with `MSG_NOSIGNAL`), `getrandom`, `fsync`,
`ftruncate`, `mmap`. All patches of TDLib change behaviour only under `TD_ZORAYA` or `__zoraya__`.

## `_tdjson`

`modules/_tdjson.c`, a CPython built-in module (multi-phase initialization, no module state):

- `create_client_id() -> int`, `send(client_id, request)`, `receive(timeout) -> str | None`,
  `execute(request) -> str | None`. A request is a `str`, UTF-8 `bytes`, or a `dict` (serialized with `json.dumps`).
- `receive` waits with the GIL released; TDLib allows one `td_receive` at a time, so the module serializes callers
  with a lock and copies TDLib's string into a Python `str` before releasing it. `execute` keeps the GIL (it is
  synchronous and short) so that its thread-local result is copied before the thread can call TDLib again.

**How it gets into python3.** Zoraya's python build has no hook for an extra built-in module (TG-01's request R1).
`make python` therefore runs Zoraya's own recipe unchanged (`make -C $ZORAYA -f ports/Makefile` with `PYTHON_BUILD`,
`PYTHON_STAGE`, `PYTHON_ROOT` and `LOGS` pointed into `build/`), from the same pinned CPython 3.15.2 source with the
same six patches and configure answers. It then writes `Modules/Setup.local` (`*static*`, `_tdjson` with TDLib's
archives and `libtdshim.a`), lets `makesetup` regenerate the Makefile, and links `python.exe` again with
`LINKCC=zc++`, so that libc++, libc++abi, libunwind and the `.eh_frame_hdr` (TDLib uses exceptions) are in. Two
host details: CPython's binary is `python.exe` on macOS's case-insensitive file system, and `makesetup` puts the
object of an absolute source path under `Modules/` plus that path. `zoraya-elf-check` accepts the result.

## Smoke test inside Zoraya

`make smoke`: Zoraya's `tools/run-disk.sh` makes the volume with `EXTRA_ROOT=out/root` (Zoraya's `rootfs/` and
fonts with this repository's part over them, 256 MB); Zoraya's test runner plays `tests/smoke.script` on the serial
line of `tools/run-qemu.sh --test --keys --mem 512M --accel hvf`. Run on the M1 on 2026-11-14 (kernel `0c3ada97`),
twice, both `PASS: 25 steps`. The transcript (from `build/logs/smoke.log`, colour codes and the line editor's
redraws of each typed command removed):

```
Type help to begin, or help tour.
~> python3 -V
Python 3.15.2
~> python3 -c 'import _tdjson, json; print(_tdjson.execute(json.dumps({"@type":"getOption","name":"version"})))'
{"@type":"optionValueString","value":"1.8.70"}
~> status
succeeded (exit status 0)
~> python3 tdjson_client.py
log {"@type":"ok"}
client id 1
recv updateOption
recv updateOption
recv updateAuthorizationState authorizationStateWaitTdlibParameters
recv optionValueString 1.8.70
recv ok
recv updateAuthorizationState authorizationStateClosing
recv updateAuthorizationState authorizationStateClosed
waiting True version True closed True
client done
~> status
succeeded (exit status 0)
~> poweroff
```

`tests/tdjson_client.py` sets the log verbosity to 1 (`execute`), creates a client, sends
`getOption("version")`, reads until it has both `authorizationStateWaitTdlibParameters` and the option's value, then
sends `close` and reads until `authorizationStateClosed`: the client's threads, its poll loop and the pipe wake-up
all work inside Zoraya, and the program exits 0. No parameters, so no database and no network.

## What the next part (`tgd`) needs

1. **File locks.** `setTdlibParameters` opens the database directory, and TDLib locks its files with
   `fcntl(F_SETLK)` (`FileFd::lock`); libposix answers every lock command with `EINVAL`, so the database will not
   open. Either TG-01's R8 (record locks) or a patch under `TD_ZORAYA` that takes the lock as granted (one `tgd`
   per account, enforced by `tgd` itself, as TG-01's plan says).
2. **SQLite's WAL.** TDLib sets `PRAGMA journal_mode=WAL` and `.ensure()`s the statement. WAL needs a shared mapping
   of the `-shm` file, which Zoraya does not have (R10). If SQLite answers with another mode, all is well; if the
   statement fails, a patch (`PRAGMA locking_mode=EXCLUSIVE` first, which keeps the WAL index in heap memory) is the
   way. To be tried with a database in `tgd`'s first step.
3. **The network grant.** TDLib connects to data centres by IP address (built in, then from the server's config)
   and resolves some names itself (DNS over HTTPS for its config, `getaddrinfo` elsewhere). Zoraya's `+net` grants
   are `host:port`; whether a grant can name an address, or a set of them, decides how `tgd` is started. With it:
   `SIGPIPE` on a write to a closed socket (patch 0003: ignored by CPython, so harmless in `python3`; a `tgd` in C++
   must ignore it itself) and `MSG_NOSIGNAL` on `send`, which libposix honours.
4. **Positioned I/O** (`pread`/`pwrite`) is emulated (R13): fine for one binlog per process, to be watched if TDLib
   reads one file from several threads (its file loaders do).
5. **Memory.** `python3` is 39 MB loaded before TDLib allocates anything; TDLib runs several threads (a scheduler per
   CPU, plus its own), and a thread's stack is committed in full on Zoraya. The smoke run fitted in 512 MB; a logged-in
   client with its caches must be measured.
6. **Size.** TDLib is about 27 MB of the program; nothing was stripped (no `strip` for the target here: Rust's
   `rust-objcopy` lacks its `libLLVM.dylib` on this host). Leaving out the e2e calls or using `-Os` everywhere is not
   expected to change much; it is the API's size.
7. **Time.** TDLib's clean build is 16 minutes at `-j2` on the M1; the python relink is a minute. The generated
   sources are made once per TDLib pin.
