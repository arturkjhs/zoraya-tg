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

## Not requested

- `bind`, `listen`, `accept`, `sendmsg`, `in6addr_any`: named by a ported library's server sockets, its
  scatter write and its IPv6 any-address, none of which a client uses on an IPv4-only system (ADR-0225). Closed
  here by stubs and one patch, see `docs/tdlib-report.md`.
- `__unix__` predefined by `zcc`: a ported library that tests `__unix__` falls into its "unknown Unix" branch
  (here: `select` and `socketpair`), worse than naming Zoraya. Patched per library instead.
