# Status (TG-03)

2026-11-14: steps 1-2 done (commits 38e15e7, d197bdf). `make smoke-tgd` passes inside Zoraya
under HVF; `make test-tgd-host` passes; host `tgd --test-dc` reaches
`authorizationStateWaitPhoneNumber` with the architect's credentials (read by tgd from
`.env`, never by the agent; `.env` is git-ignored).

**Waiting for the architect:** the login on the host with a test number (docs/tasks/TG-03,
"Host"). Then: the same inside Zoraya, memory, `docs/tgd-report.md`.
