# Status (TG-04, stage B)

2026-11-15: the window `tg` with its sample backend is written, tested on the host (`make test-tg-host`, 33 tests)
and running inside Zoraya from the dock (`make tg-image`, `make run-tg`); the design is `docs/tg-window.md`, the
report `docs/tg-window-report.md`.

`make test-tg-window` passes under HVF at scale 1 and at scale 2, each twice (111 steps, with a Ukrainian and a
Russian message typed through Zoraya's `ua` and `ru` layouts); the screens are in `docs/shots/`. Messages are laid
out only in and near the view: at 1000 messages the first frame of a new tree takes 1.5 to 2.8 ms at scale 1 (it was
6.3 to 8.4 ms) and a frame while scrolling 1.45 ms; `tg` holds 5.2 MiB in `top`. **Stage A is done.**

Zoraya's source for this task is the local clone `../zoraya-tgw` (no remote), at `overlay/BASE` with `overlay/*.patch`.

**Still waiting for the architect (TG-03):** the real-account login, by hand, on the host and then inside Zoraya
(`make run-tgd`), with the steps in `docs/tgd-report.md`.

**Stage B** started 2026-11-19: 10j and the own-directory sockets (ADR-0465) are on `origin/main` (`754d2b49`).

2026-11-19: **stage B step 1 is done**: tgclient's view-models in `tgd/vm/` serving protocol 2, with the mapping
table in `docs/tg-window-report.md` ("Stage B step 1"). `make test-tgd-vm` (host Python against a fake TDLib) passes
twice. It was host Python only while the TASK-31 dispatcher held the builds. Steps 2 and 3 (the socket, installing)
wait for the dispatcher to pause; then `CARGO_BUILD_JOBS=4`, one QEMU at a time.

**Stage B is done but for the real login (2026-11-19).** Steps 2 to 4 are built and tested; the window is ready
for the architect's real login: `make run-tg-login`, then the steps in `docs/tg-window-report.md` ("The real
login"). `make test-tg-socket` (installed, over the socket, synthetic account, scale 1 and 2) and
`make test-tg-real-start` (the real tgd with TDLib, placeholder credentials, no network) pass twice; `make
test-tg-window` (stage A) passes. `../zoraya-tgw` is at `f9fcaa51` with overlay 0001 to 0003; TDLib has patch 0006.
Requests R23 and R24 are new; R22's second half is built in overlay 0003.

**Waiting for the architect:** the real login (stage B's "done when": chats, history, sending, the AI summary, a
restart keeping the login, `programs tgd`/`programs tg` listing exactly what each holds).

2026-11-19, evening: the architect's hand run of `make run-tg-login` failed (the cocoa display shrank the desktop to
640x480; a reopened first terminal had no channel to the panel). Reproduced in that mode, fixed (run-tg-login's
display without zoom-to-fit and at scale 1; overlay 0004, R25, R26; tg's smaller first sizes), and the path walked
by a scripted GUI run in run-tg-login's own mode on a copy of the kept volume (`make walk-tg-login`, twice). The tests
no longer touch the kept volume. **Ready again for the architect's login.**
