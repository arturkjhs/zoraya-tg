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

**Stage B, what remains** (the architect, 2026-11-19; each step committed and pushed; nothing about Telegram in the
public repository):
2. `tgd` listens on a socket file in its own directory (ADR-0465); `tg`'s `SocketBackend` connects with a saved
   `+sock` grant. With it, **protocol 3**: a `failed` state for a message whose send failed (tgclient's
   `messageSendingStateFailed`), never shown as pending, and a retry action in the window.
3. Both programs installed with `install` and saved grants: for `tgd` the nine IPv4 ranges on 443 and
   `openrouter.ai:443`, for `tg` the `+sock`. `programs show` lists exactly what each holds.
4. In the chat view, a position indicator and a jump to the first unread message.
Then stop: the real-account login is the architect's, by hand.

Step 1's choices were accepted: the per-chat AI switch as the one settings item, the summary as running then done,
the empty subtitle until presence, the fake AI service in `tests/tgd_vm.py`, `python3.12` on this host (the Mac mini).
**Steps 2 to 4 wait until the architect says the TASK-31 dispatcher has paused.** Then `CARGO_BUILD_JOBS=4`, one
QEMU at a time; `~/workspace/zoraya-tgw` is updated from `~/workspace/zoraya-8` by commit, and the overlay patches
that main now covers are dropped; the dispatcher's worktrees are not touched.
