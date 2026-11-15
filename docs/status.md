# Status (TG-04, stage A)

2026-11-15: the window `tg` with its sample backend is written, tested on the host (`make test-tg-host`, 30 tests)
and running inside Zoraya from the dock (`make tg-image`, `make run-tg`); the design is `docs/tg-window.md`, the
report `docs/tg-window-report.md`.

In progress: `make test-tg-window` (the GUI script under HVF at scale 1 and 2, run twice), the screens in
`docs/shots/`, and the measurements (memory in `top`, frame time while scrolling).

Zoraya's source for this task is the local clone `../zoraya-tgw` (no remote), at `overlay/BASE` with `overlay/*.patch`.

**Still waiting for the architect (TG-03):** the real-account login, by hand, on the host and then inside Zoraya
(`make run-tgd`), with the steps in `docs/tgd-report.md`.

**Stage B waits for** 10g, 10j, F2 and F7 in Zoraya's `origin/main`.
