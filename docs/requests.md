# Generic requests for Zoraya

Changes this repository would like in Zoraya, for the architect to pass to Zoraya's dispatcher. Each is worded
to name no program: the reason is given as "a ported C++ library" or "an event loop". None has a branch in the
public repository (none of them asks for one). Until each lands, this repository closes the gap itself, as noted.

The numbers are those of TG-01's list (`docs/analysis.md`, section 3.9): TG-02 adds its evidence and a
minimal test to R1, R5 and R11, and its new requests start at R13. One correction to that list: libc.a has
no `select` either, only `pselect` (R5's "workaround: `select`" is `pselect`).

## R5: `poll` and `ppoll` (and `select`) in libposix

**What.** libc.a exports `pselect` (libposix, ADR-0106) and no `poll`, `ppoll` or `select`: musl's
`src/select/poll.c` and `select.c` are not retained, and libposix has none of its own.

**Why.** The event loops of ported libraries are written on `poll` (or on `select` where they are older); each port
otherwise carries its own wrapper over `pselect`. A wrapper cannot tell a hang-up from readability (pselect does
not say which), which `POLLHUP` would.

**Until then.** `ports/tdshim.c` here: `poll` and `select` over `pselect` (up to 1024 descriptors).

**Minimal test.** A C program: `pipe`, a thread that sleeps 100 ms and writes one byte, the main thread's
`poll({rfd, POLLIN}, 1, 5000)` returns 1 with `POLLIN` within about 100 ms; after the writer closes its end and
the byte is read, `poll` gives `POLLHUP`; `poll` with timeout 0 on an empty pipe returns 0 at once; a closed
descriptor gives `POLLNVAL`. The same through `select`.

## R1: `python3` takes extra built-in modules from a directory given at build time

**What.** `make -f ports/Makefile python` with, say, `PYTHON_EXTRA_SETUP=FILE` (lines in CPython's
`Modules/Setup` form, appended to `Modules/Setup.local` after configure) and `PYTHON_LINKCC=zc++` when one of
the modules is C++ (libc++, libc++abi, libunwind and the `.eh_frame_hdr`). The rest of the recipe unchanged.

**Why.** A program that needs a C extension module has no other way in: the interpreter is static and has no
`dlopen` (ADR-0320). Today the module's owner must run the recipe into directories of their own and link
`python3` a second time.

**Until then.** The Makefile here runs Zoraya's recipe with `PYTHON_BUILD`, `PYTHON_STAGE`, `PYTHON_ROOT` and
`LOGS` overridden, writes `Modules/Setup.local`, and runs `make python LINKCC=zc++` in that build directory.

**Minimal test.** A ten-line C module `_extra` with one function returning 42, named in a Setup file; the
built `python3 -c 'import _extra; print(_extra.f())'` prints 42 inside Zoraya; `sys.builtin_module_names`
lists it; a second module in C++ that throws and catches an exception inside the call returns normally.

## R13: `pread` and `pwrite` in libposix

**What.** libc.a exports neither (nor `preadv`, `pwritev`, `readv` under its POSIX name).

**Why.** Positioned I/O is how database and log code reads and writes a file shared by threads without a seek
race. A wrapper of `lseek` + `read` + `lseek` cannot be atomic against another thread's plain `read`.

**Until then.** `ports/tdshim.c`: `lseek`, `read` or `write`, `lseek` back, under one lock.

**Minimal test.** Write "abcdef" to a file; `pread(fd, buf, 2, 3)` gives "de" and leaves the offset at 6;
`pwrite(fd, "XY", 2, 1)` gives "aXYdef" and leaves the offset where it was; on a pipe both fail with `ESPIPE`;
two threads doing 1000 `pread`s at different offsets each read their own bytes.

## R11: `socketpair(AF_UNIX, SOCK_STREAM)` in libposix

**What.** No `socketpair` (Zoraya's python build already says it absent, and CPython's asyncio carries a patch,
`0003-asyncio-self-pipe-without-socketpair`).

**Why.** It is the portable self-wake of an event loop on systems without `eventfd`: a second patch of the same
kind for each port that wakes its poll loop from another thread.

**Until then.** A patch here (`patches/td/0001`) wakes the loop through a `pipe`.

**Minimal test.** `socketpair` gives two descriptors; a byte written to either is read from the other; closing
one makes the other's `read` return 0 and `pselect`/`poll` report it readable.

## R14: `usleep` and `mkdtemp` in libc.a

**What.** Neither is exported. `usleep` is obsolescent in POSIX but still everywhere in ported C (an embedded
database's busy wait); `mkdtemp` is POSIX.1-2008.

**Until then.** `ports/tdshim.c`: `usleep` over `nanosleep`, `mkdtemp` over `getrandom` and `mkdir`.

**Minimal test.** `usleep(200000)` returns 0 after 0.2 s or more (by `clock_gettime`); `mkdtemp("/tmp/tXXXXXX")`
makes a directory of mode 0700 whose name differs from the template, twice two different names; a template
without six `X` fails with `EINVAL`.

## R15: descriptions of POSIX programs from a port, at build time

**What.** The image's list of trusted POSIX descriptions (`user/describe/posix/*.cmd`, compiled into the shell as
`descriptions::POSIX`) takes extra `.cmd` files from a directory a port names at build time, as R1 does for
built-in modules: `make ... EXTRA_DESCRIBE=DIR` with `program /bin/NAME` in each, for programs that
`EXTRA_ROOT` puts in `/bin`.

**Why.** The shell gives a POSIX program its `+net` words only under a description the image carries (ADR-0153
rule 10, INV-0153-6; `posix_net_scope`): a ported program that is its own executable gets `+w` and `+r` from the
line but never a network connection, and `grants` shows none, with no word saying why. Nothing on a volume should
be read for it (that invariant stays); the description comes with the image build.

**Until then.** The ported program runs as a script of the described interpreter (`python3 /usr/lib/NAME/NAME.py`),
the same code; its own `/bin/NAME` runs only lines without `+net`. `image/describe/tgd.cmd` here is the
description ready for the image.

**Minimal test.** A port's `/bin/hello-net` (connects to the address of its operand, port 443, prints the errno)
and its `.cmd` with `program /bin/hello-net`: `grants +net 192.0.2.1:443 hello-net 192.0.2.1` lists
`net: 192.0.2.1:443 reach (grant +net)`, and the run gets past `EACCES`; without the `.cmd` both stay as today.

## R16: `inet_pton(AF_INET6, ...)` in libc.a

**What.** `inet_pton` parses IPv6 text (and `inet_ntop` writes it) instead of failing.

**Why.** A ported network library parses its built-in IPv6 addresses at start and treats a failure as a bug
(an assertion), even on a system where it will never connect over IPv6 (ADR-0225). Parsing is pure text; the
system can stay IPv4-only for sockets.

**Until then.** A patch per library: skip the IPv6 addresses on Zoraya (`patches/td/0005`).

**Minimal test.** `inet_pton(AF_INET6, "2001:db8::1", &a)` returns 1 with the 16 bytes
`20 01 0d b8 00 .. 00 01`; `"::ffff:1.2.3.4"` gives the mapped form; `"1::2::3"` returns 0; `inet_ntop` of each
gives the canonical text back.

## R17: `getrusage`'s `ru_maxrss`

**What.** `getrusage(RUSAGE_SELF)` fills `ru_maxrss` (peak resident memory, KiB) from what the room keeper already
counts for `top`.

**Why.** A program reports its own memory (a status line, a test's budget) without a shell command; today the
field is 0, and the only number is `top` inside a room (the session's row shows 0K outside one).

**Until then.** Measured with `top` in a room, the program stopped by Ctrl-Z (`make smoke-tgd-net`).

**Minimal test.** A C program allocates and touches 32 MiB; `ru_maxrss` before is below 16384 and after is at
least 32768; `top` in a room shows a number within a few MiB of it.

## R18: reading a line without echo

**What.** A way for a program to read one line from the shell's stream without the shell showing it: a
`termios` `ECHO` flag honoured on that stream, or a call that asks for one hidden line.

**Why.** A program that asks for a password or a key at its own prompt has no terminal from zosh (ADR-0153); the
shell echoes every key, so the secret stays on the screen and in a scrollback.

**Until then.** The program says so at the prompt; on Zoraya credentials are typed visibly.

**Minimal test.** A program turns echo off, reads a line, turns it on, prints the line's length: the typed keys do
not appear, the length does, and the next line echoes again.

## R19: the image's SQLite takes randomness from `getrandom`

**What.** The SQLite in `python3` (`_sqlite3`) gets its random bytes from `getrandom` on Zoraya, as
`patches/td/0004` does for a ported library's own SQLite.

**Why.** SQLite's unix VFS opens `/dev/urandom`, which a program is not given, so the shell prints
`hint: python3 tried to read /dev/urandom, which it was not given` on every run that opens a database, with a
`+r /dev` suggestion that would give more than needed. SQLite falls back to the time and pid, so it works; the hint
is noise and the fallback is weak.

**Until then.** The hint is ignored.

**Minimal test.** `+w ~/t python3 -c 'import sqlite3; sqlite3.connect("/home/t/x.db").execute("create table t(a)")'`
prints no hint.

## Not requested

- `bind`, `listen`, `accept`, `sendmsg`, `in6addr_any`: named by a ported library's server sockets, its
  scatter write and its IPv6 any-address, none of which a client uses on an IPv4-only system (ADR-0225). Closed
  by Zoraya's own `libzshim.a` (`bind`, `listen`, `accept` fail with `ENOSYS`), one patch (`sendmsg`) and the
  constant in `ports/tdshim.c`; see `docs/tdlib-report.md`.
- `__unix__` predefined by `zcc`: a ported library that tests `__unix__` falls into its "unknown Unix" branch
  (here: `select` and `socketpair`), worse than naming Zoraya. Patched per library instead.
- `getrandom` for a program another program starts (ADR-0310: by design). A ported interpreter's own launcher
  that `exec`s it gets no random bytes, so the interpreter does not start; closed by linking the launcher's
  `main` into the interpreter itself (`tgd/tgd_main.c`, `ports/tgd-link.mk`).
- Starting a script by its `#!` line: the shell starts ELF programs only; closed the same way.
