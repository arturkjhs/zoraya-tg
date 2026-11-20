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

## R20: the parts of a native window program as crates of their own

**What.** The heap of a native program (`user/src/heap.rs`), the platform of an application over the compositor's
channel (`Gui` in `user/src/app.rs`), the file link and the chooser's client, in a crate (or crates) that depends on
`zoraya-rt`, `zoraya-heap` and `zoraya-app` only, with the link script (`user/user.ld`) reachable from a build
script; `zoraya-user` then uses them itself. This is R3's first half ("native programs built outside the image").

**Why.** A native program outside the repository can name every `crates/` crate by path, but these live inside
`zoraya-user`, which is every program of the ramdisk at once: depending on it builds the network stack, TLS and all
the drivers, and needs the repository's `ring` patch and its C compiler wrapper in the outside workspace.

**Until then.** The outside program carries its own copies (here `tg/tg/src/heap.rs` and `platform.rs`, about 190
lines with two `unsafe` places) and a build script that names `user/user.ld` by path.

**Minimal test.** A crate outside the repository with `zoraya-rt`, `zoraya-app` and the new crate as path
dependencies builds for `aarch64-unknown-none` a program that opens a window with one label; `zoraya-elf-check`
(or the ramdisk packer) accepts it; its dependency tree holds no network or driver crate.

## R21: native programs from outside the repository in the image

**What.** `make build EXTRA_PROGRAMS="PATH..."`: ELF files with the native mark, added to the ramdisk after the
repository's own under their base names (the packer already checks the mark and an optional description). Later the
same through the volume: a native program installed as ADR-0396 installs a POSIX one.

**Why.** The program directory a launch looks in is the ramdisk inside the kernel image; `EXTRA_ROOT` adds only to
the volume, where a native program is never looked for. There is no way to try a native program that is not in the
repository's `USER_PROGRAMS`.

**Until then.** `overlay/0001-extra-programs-in-the-ramdisk.patch` on a local clone (8 lines of the Makefile).

**Minimal test.** A `hello`-like native ELF built outside, passed in `EXTRA_PROGRAMS`: the shell runs it by name;
the ramdisk lists it; a file without the mark fails the build with the packer's message.

## R22: a dock tile and a launch record for a program that is not one of the four

**What.** The dock's list (`zoraya_panel::APPS`, a table of four) takes more names at build time (`EXTRA_APPS="NAME
..."`), and later from the records of installed programs (ADR-0396): each gets the launch record of an application
(the console, its compositor channel, the chooser, its data directory), a tile with its initial, and its windows'
pills. Also a way to start a window program from a terminal by its name (`room NAME APP` exists as a line on the
tile, but `zosh` starts no window program).

**Why.** A window program is launched only by the panel, and the panel launches only what its table names.

**Until then.** `overlay/0002-extra-applications-in-the-dock.patch` (a function beside the table, read from the
build's environment); the program starts from the dock only, not from a terminal.

**The second half, as built in the overlay (2026-11-19).** A window program that is *installed* (ADR-0396) gets its
saved grants only when the shell starts it, and only the panel gives windows; so a window program that needs a saved
grant (a socket file, ADR-0465) cannot have both. `overlay/0003-a-window-for-an-installed-native-program.patch`
(`zoraya-describe`, `zosh`): a volume description may say `window TITLE` (the one key added to ADR-0396 item 5's
set; `for_volume` keeps it); when the shell starts an installed native program whose description has it, it asks the
panel for the window exactly as ADR-0317 asks for a POSIX program's (a trusted room, a desktop, every grant of the
line as a pill, refused when the pills do not fit), and starts the program with the client end at slot 5 and the
console at slot 1, as the panel's launch record of an application has them, beside its start-up record with the
saved words. 132 changed lines, with the tests of the key set changed to match. **Minimal test:** an installed
native program with `window Demo` and a saved `+sock` word, started by name from a terminal of the desktop, opens
its window with the pill `+sock PATH` and reaches that socket; in an untrusted room the line is refused; without the
key it starts as today, with no window.

**Minimal test.** With `EXTRA_APPS=demo` and a `demo` program in the ramdisk: the dock shows a fifth tile with `D`,
a press opens its window with the pill `+w /data/home/demo`, a second press raises it, and in a room it opens with
the room's grants; without the variable the dock is as today.

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

## R23: `realpath` inside a program's own directory

**What.** `libposix`'s `realpath` of a path inside an `Own` entry (ADR-0341: the directory seen at its view path,
`~/.config/NAME`) walks the path from `/` through the other entries, where the view path's parents (`~/.config`)
need not exist; it fails with ENOENT although every name of the path resolves through the table. It should resolve
the path as `open` does (longest prefix entry first), so that a program that canonicalises its data directory
(SQLite, TDLib, many others) works in its own directory.

**Why.** A program installed with `+own` keeps its data there; ports commonly call `realpath` on their data
directory before they open it.

**Until then.** `patches/td/0006-realpath-of-an-own-directory-on-zoraya.patch` (TDLib takes the path as given).
**Minimal test:** a program with `+own ~/.config/demo` calls `realpath("~/.config/demo/db")` after `mkdir`: the
result is that path (or its real one), not ENOENT.

## R24: a socket file's pill in a window's record

**What.** `zosh`'s `window_pills` writes a `+sock PATH` pill for a socket file's grant (ADR-0465), and the panel's
`window_record` refuses every pill that is not `+r`, `+w`, `+net` or `web`, so a window program given a socket file
gets no window at all ("the panel does not accept this window request"). The panel should take `+sock PATH` (one
prefix more in the check). In `overlay/0003` (crates/zoraya-panel, with a test).

**Minimal test:** `window_record(None, "demo", "Demo", "+sock /data/home/srv/s.sock")` is accepted; `+socket ...` is
not.

## R25: a new first terminal gets the channel to the panel

**What.** When the first terminal of the desktop is closed (or its shell exits), `init` starts a new terminal and
shell beside the running panel. The shell's channel to the panel for a POSIX program's window (ADR-0317, slot 14) is
made only when the panel starts, so the new shell has none: every window program refuses with "it needs a window,
and this session has no desktop to give one". The panel should take a new shell channel from `init` (a message on a
channel it already reads), so that the new shell gets one without the panel starting again.

**Until then.** `overlay/0004-a-new-first-terminal-gets-a-desktop.patch` (`init`): the panel and its chooser start
again with the new terminal, as after a failed GUI, which closes the windows the dock had opened.
**Minimal test:** close the first terminal; in the new one, a program with a window (`netsurf`, or an installed one
with `window`) opens it.

## R26: QEMU's cocoa display on a screen smaller than the guest's

**What.** `tools/run-qemu.sh --gui` (interactive) uses `-display cocoa,...,zoom-to-fit=on`. On a 1920x1080 screen
that is not Retina (a Mac mini with an ordinary monitor) the window reports 640x480 to the guest at once
(`gpud: display 640x480`, at scale 1 and at scale 2 alike), and Zoraya's desktop follows: too small for most windows
(an application asks for more and is refused), the dock and the terminals crowded. Without `zoom-to-fit` the guest
keeps 1024x768. Either drop `zoom-to-fit` where the screen cannot show the guest at 1:1, or have the compositor keep a
minimum size.

**Until then.** zoraya-tg's `make run-tg-login` passes `--display-opts cocoa,left-command-key=on`.
**Minimal test:** `make run-gui` on a 1920x1080 screen: the serial line says no `gpud: display 640x480`.

## R27: an emoji font, flags and wide characters

**What.** The system's fonts (`fonts/`, ADR-0326 item 5) have no emoji, so every emoji is drawn as the box: in titles,
in messages, in the terminal. A text from another system carries many of them (a flag before a name, a smile at the
end of a line), and a row of boxes reads as a broken font. Four parts:

1. **The font.** Noto Emoji (the outline font, not the colour one), under OFL-1.1, unmodified and with its licence
   text, in `fonts/` and so in `/system/fonts`, embedded by `zoraya-fonts` as the others are. A static TrueType
   instance: `zoraya-text` reads `glyf` outlines and no variations.
2. **Fallback through `zoraya-text`.** The emoji font last in every list before the box: `zoraya-ui`'s lists have
   three fonts (the style's, Inter, JetBrains Mono) and `MAX_FONTS` is 4, so it fits; the terminal's list too.
3. **Sequences.** The shaper takes an emoji sequence as one cluster: the zero-width joiner (U+200D), the variation
   selectors (U+FE0E, U+FE0F), the skin tones (U+1F3FB to U+1F3FF), the keycap (U+20E3) and the tags of a
   subdivision's flag (U+E0020 to U+E007F) are never drawn as boxes; a joined sequence the font has as one glyph is
   that glyph, otherwise its first part. **A pair of regional indicators** (U+1F1E6 to U+1F1FF) is a flag: the
   font's glyph for the pair when it has one (a `GSUB` ligature, which the simple shaper would have to read for these
   pairs only), otherwise the two letters (`U` `A`) small in a frame, so that the country is still read; a lone
   indicator is its letter in the frame.
4. **Width.** An emoji is wide: two cells in the terminal's grid (the East Asian Wide property, as `wcwidth` gives
   2), with the cursor and the line's length counted so; in a proportional layout, its advance from the font.

**Until then.** zoraya-tg's window draws each emoji sequence its fonts lack (a flag's pair, a joined sequence with
its modifiers and selectors) as one small neutral mark, the first of `◦` `•` `·` the fonts have, instead of a box per
code point; a script with no font keeps the box (`tg/window/src/draw.rs`, `shown`).
**Minimal test:** `zoraya-text` draws `"\u{1F600}"`, `"\u{1F1FA}\u{1F1E6}"`, `"\u{1F44D}\u{1F3FD}"` and
`"\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}"` with no glyph from `Source::Box`; the flag is one glyph (or the
framed letters), the family one glyph or its first; in the terminal, `echo` of a smile moves the cursor two cells.

## R28: `cwd none` in an installed program's description

**What.** Every program a shell starts is given its current directory, read only (ADR-0153 rule 1), unless its
description says `cwd none`. An installed program's description may not say it (ADR-0396 item 5 refuses the key with
the others that give authority), and the launch plans an installed program with the description of an undescribed
POSIX program, whose `cwd` is `read`. So a window program that reads no file still shows `+r /home` among its pills,
and holds it, when started from a terminal at home. `cwd none` only takes away; it should be allowed:
`volume_key_problem` takes `cwd none` (still refusing `cwd read` and every other key), `for_volume` keeps it, and the
launch (native and POSIX) plans with `cwd none` when the description checked with the program's bytes says so.

**Until then.** `overlay/0003` does it for a native installed program (crates/zoraya-describe, crates/zoraya-settings'
tests, `zosh`'s native launch); the POSIX path is not changed.
**Minimal test:** install a package whose description says `cwd none`; from a terminal at home, `jobs` after
`NAME &` shows no `~: read`, and its window has no `+r /home` pill. A description with `cwd read` is still refused.
