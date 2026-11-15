# Status (TG-03)

2026-11-15: everything that needs no account is done and tested (`docs/tgd-report.md`):
`make test-tgd-host`, `make smoke-tgd` (55 steps, run twice and again), `make smoke-tgd-net`
(78 steps: the exact line, `grants`, a round trip to Telegram's test DC, memory 72 MiB at
`wait_phone`, the text front end), the data kept across a reboot.

The test-number login is impossible: Telegram disabled test accounts (tdlib/td#3083,
2026-01-05), so `PHONE_CODE_INVALID` for every `99966XYYYY`; not a tgd fault.

**Waiting for the architect:** the real-account login, by hand, on the host and then inside
Zoraya (`make run-tgd`), with the steps in `docs/tgd-report.md`; then the logged-in memory
goes in its status block.
