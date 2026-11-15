# Status (TG-04, stage A)

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

**Stage B waits for** 10g, 10j, F2 and F7 in Zoraya's `origin/main`.
