# TG-02: TDLib on Zoraya, and `_tdjson` in `python3`

## Where and how this runs

On the architect's **MacBook Air M1** (it has the toolchain and QEMU under HVF), in clones of the **private** repository `arturkjhs/zoraya-tg` (`~/workspace/zoraya-tg`) and of `arturkjhs/zoraya` (`~/workspace/zoraya-run`, on the latest `main`, used read-only as the toolchain and image). In parallel with TG-01 (the analysis of tgclient on the Mac mini) and TASK-33. Model: **Opus**. `CARGO_BUILD_JOBS=3`, the TDLib build at `-j2` (it is memory-heavy), one QEMU guest at a time. **Do not ask; decide, record, continue.**

## Secrets and privacy (hard rules)

- No API id or hash, token, phone number, session or database anywhere: not in this repository, not in a test, not in the conversation. Tests use only what needs no account: TDLib's synchronous `td_execute` methods and a client reaching `authorizationStateWaitTdlibParameters`.
- **Nothing about Telegram or TDLib goes into the public repository `arturkjhs/zoraya`.** Everything TDLib-specific (its pin, patches, build recipe, the `_tdjson` module, tests) lives in `zoraya-tg`. If Zoraya itself must change, the change is **generic** (for example "`poll` and `ppoll` in libposix", "`python3` accepts extra built-in modules from a directory given at build time"), worded to name no program, written as a request in `zoraya-tg/docs/requests.md` with the reason and a minimal test, for the architect to pass to Zoraya's dispatcher. You may prepare such a generic change on a branch `work/<generic-name>` in the public repository only if the request says so and its words name no program; never merge it.

## What to build (in `zoraya-tg`)

1. **TDLib**, pinned (a release tag or commit with its SHA-256 in `ports/sources.lock` of this repository), cross-built statically with Zoraya's `zcc`/`zc++` and `ports/cmake/zoraya.cmake` from the Zoraya clone, against its OpenSSL and zlib. Host tools it needs at build time (`gperf`, a host build of its code generators) under `build/host`. Record what TDLib needed from Zoraya's `libc.a` and how each gap was closed: a patch in `zoraya-tg/patches/` (for example its event loop on `select` or `poll`, its wake-up on `pipe` where there is no `eventfd` or `socketpair`), or a generic request.
2. **`_tdjson`**, a CPython built-in module (C) wrapping `td_create_client_id`, `td_send`, `td_receive` and `td_execute`, linked into a `python3` with TDLib and libc++. If Zoraya's `python3` build cannot take an extra built-in module from outside, write that generic request first, and build `python3` here from the same pinned source with the same patches until it lands.
3. **A smoke test inside Zoraya** (an image through Zoraya's `EXTRA_ROOT` with this repository's output directory): `python3 -c 'import _tdjson, json; print(_tdjson.execute(json.dumps({"@type":"getOption","name":"version"})))'` prints TDLib's version; a client created with `_tdjson.create_client_id()` and `send({"@type":"getOption","name":"version"})` receives `updateAuthorizationState` with `authorizationStateWaitTdlibParameters`; both under HVF; the transcript in the report. Network to Telegram's servers is **not** part of this task.

## Report

`docs/tdlib-report.md` in `zoraya-tg`, a status block at the top: what builds, the sizes (`libtdjson` static, the `python3` with it), the build time on the M1, the patches and why each, the generic requests for Zoraya, what the next part (`tgd`) needs.

## Git

Configured identity of the Zoraya work only. Commit and push `zoraya-tg` after each step; check `git diff --cached` for secrets before each commit. In the public repository: nothing, unless a generic branch as above. On a usage-limit warning: commit, push, write `docs/status.md`, stop.
