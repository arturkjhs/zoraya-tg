"""tgd's entry point: options, the network probe, the data directory and its lock, logging,
then the core and one front end on one asyncio loop: the text one, the JSON lines on
stdin/stdout (--json), or the window's socket (--window, protocol 3)."""

from __future__ import annotations

import argparse
import asyncio
import contextlib
import json
import logging
import logging.handlers
import os
import sys
from pathlib import Path
from typing import Any

from . import VERSION, net

ON_ZORAYA = sys.platform == "zoraya"


def default_data_dir() -> Path:
    if ON_ZORAYA:
        # Its own directory (ADR-0341, the saved +own ~/.config/tgd of its install): no
        # other program reads it, and the window's socket can be made only there.
        return Path(os.path.expanduser("~")) / ".config" / "tgd"
    return Path.home() / ".local" / "share" / "tgd"


def default_api_file() -> Path | None:
    if ON_ZORAYA:
        return None  # typed at tgd's prompt
    return Path.home() / ".config" / "zoraya-tg" / "api"


def parse(argv: list[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(prog="tgd", description="tgclient's backend: TDLib, the "
                                "stores, the services; JSON lines or a text front end.")
    p.add_argument("--test-dc", action="store_true", help="Telegram's test data centres")
    p.add_argument("--json", action="store_true", help="the line protocol on stdin/stdout")
    p.add_argument("--window", action="store_true",
                   help="serve the window tg on a socket file in the data directory")
    p.add_argument("--socket", type=Path,
                   help="the window's socket (default: window.sock in the data directory)")
    p.add_argument("--synthetic", action="store_true",
                   help="with --window: an invented account, no TDLib and no network (tests)")
    p.add_argument("--data", type=Path, help=f"data directory (default {default_data_dir()})")
    p.add_argument("--binding", choices=("builtin", "ctypes"),
                   help="TDLib binding (default: builtin _tdjson where present, else ctypes)")
    p.add_argument("--tdjson", help="libtdjson for the ctypes binding (else TDJSON_PATH)")
    p.add_argument("--api-file", type=Path,
                   help="TG_API_ID=/TG_API_HASH= file (host default ~/.config/zoraya-tg/api)")
    p.add_argument("--no-probe", action="store_true", help="skip the network probe")
    p.add_argument("--td-log", type=int, default=1, help="TDLib's log verbosity (1)")
    p.add_argument("--grant", action="store_true",
                   help="print the line that starts tgd on Zoraya for one run, and end")
    p.add_argument("--install-line", action="store_true",
                   help="print the line that installs tgd on Zoraya with its grants, and end")
    p.add_argument("--version", action="version", version=f"tgd {VERSION}")
    return p.parse_args(argv)


def say(args: argparse.Namespace, event: dict, text: str) -> None:
    if args.json:
        sys.stdout.write(json.dumps(event) + "\n")
        sys.stdout.flush()
    else:
        sys.stderr.write(text + "\n")
        sys.stderr.flush()


class Lock:
    """One tgd per data directory: a directory made with mkdir (atomic), holding the pid.
    TDLib's own lock is fcntl's, which on Zoraya is only within a process (ADR-0319)."""

    def __init__(self, data_dir: Path) -> None:
        self.path = data_dir / "tgd.lock"

    def take(self) -> str:
        for _ in range(2):
            try:
                self.path.mkdir()
                (self.path / "pid").write_text(str(os.getpid()), encoding="ascii")
                return ""
            except FileExistsError:
                pid = self._pid()
                if pid and pid != os.getpid() and self._alive(pid):
                    return f"another tgd (pid {pid}) uses this data directory"
                self.release()  # stale: the tgd that made it is gone
        return f"cannot take {self.path}"

    def _pid(self) -> int:
        try:
            return int((self.path / "pid").read_text(encoding="ascii").strip() or 0)
        except (OSError, ValueError):
            return 0

    @staticmethod
    def _alive(pid: int) -> bool:
        try:
            os.kill(pid, 0)
        except ProcessLookupError:
            return False
        except PermissionError:
            return True
        except OSError:
            return False
        return True

    def release(self) -> None:
        with contextlib.suppress(OSError):
            (self.path / "pid").unlink()
        with contextlib.suppress(OSError):
            self.path.rmdir()


def setup_logging(data_dir: Path) -> None:
    handler = logging.handlers.RotatingFileHandler(
        data_dir / "tgd.log", maxBytes=1 << 20, backupCount=2, encoding="utf-8")
    handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
    root = logging.getLogger()
    root.handlers[:] = [handler]
    root.setLevel(logging.WARNING)
    logging.getLogger("tgd").setLevel(logging.INFO)


def main(argv: list[str] | None = None) -> int:
    args = parse(sys.argv[1:] if argv is None else argv)
    data_dir = (args.data or default_data_dir()).expanduser()
    if args.grant:
        shown = "~/tg" if args.data is None else str(args.data)
        print(net.grant_line(shown, args.test_dc))
        return 0
    if args.install_line:
        print(net.install_line())
        return 0

    from . import binding  # noqa: PLC0415

    kind = binding.choose(args.binding)

    if not args.no_probe and not args.synthetic:
        try:
            reason = net.probe(args.test_dc)
        except net.Refused as e:
            line = net.grant_line("~/tg" if args.data is None else str(args.data), args.test_dc)
            say(args, {"ev": "refused", "reason": "network", "detail": str(e), "grant": line},
                f"tgd: no network for Telegram: {e}.\n"
                f"On Zoraya, install it with its grants:\n  {net.install_line()}\n"
                f"or start it for one run with them on the line:\n  {line}")
            return 3
        if reason != "ok":
            say(args, {"ev": "warning", "reason": "network", "detail": reason},
                f"tgd: {reason}; TDLib will keep trying")

    try:
        data_dir.mkdir(parents=True, exist_ok=True)
        probe_file = data_dir / ".tgd-write"
        probe_file.write_text("", encoding="ascii")
        probe_file.unlink()
    except OSError as e:
        hint = (f" (on Zoraya: installed with +own ~/.config/tgd, or +w {data_dir} on the line)"
                if ON_ZORAYA else "")
        say(args, {"ev": "refused", "reason": "data", "detail": f"{data_dir}: {e.strerror}"},
            f"tgd: cannot write the data directory {data_dir}: {e.strerror}{hint}")
        return 4

    # Ctrl-C and kill end tgd cleanly (the lock, the window's socket file): a job started
    # with & may have inherited SIGINT ignored, and SIGTERM would end it at once.
    import signal  # noqa: PLC0415

    def _stop(_number: int, _frame: object) -> None:
        raise KeyboardInterrupt

    with contextlib.suppress(ValueError, OSError):
        signal.signal(signal.SIGINT, signal.default_int_handler)
        signal.signal(signal.SIGTERM, _stop)
    lock = Lock(data_dir)
    problem = lock.take()
    if problem:
        say(args, {"ev": "refused", "reason": "lock", "detail": problem}, f"tgd: {problem}")
        return 5
    try:
        setup_logging(data_dir)
        log = logging.getLogger("tgd")
        log.info("start: tgd %s, binding %s, %s DC", VERSION, kind,
                 "test" if args.test_dc else "production")
        api_file = args.api_file or default_api_file()
        code = asyncio.run(_run(args, data_dir, kind, api_file))
        log.info("end: %d", code)
        return code
    except KeyboardInterrupt:
        return 130
    finally:
        lock.release()


async def _run(args: argparse.Namespace, data_dir: Path, kind: str,
               api_file: Path | None) -> int:
    from .core import Core  # noqa: PLC0415
    from .frontends import JsonFrontEnd, LineReader, TextFrontEnd  # noqa: PLC0415

    if args.window:
        return await _run_window(args, data_dir, kind, api_file)

    front = JsonFrontEnd() if args.json else TextFrontEnd()
    core = Core(data_dir=data_dir, test_dc=args.test_dc, binding=kind,
                tdjson_path=args.tdjson or os.environ.get("TDJSON_PATH"),
                api_file=api_file, emit=front.emit, td_log_level=args.td_log)
    reader = LineReader(asyncio.get_running_loop())
    try:
        await core.start()
        await front.run(core, reader)
    finally:
        await core.close()
    front.emit({"ev": "bye", "td": core.td_state or "-"})
    return 0


def _window_say(event: dict) -> None:
    """What the core says (protocol 1's events), on the terminal tgd --window runs in:
    the version and the login's state, never a name or a value."""
    kind = event.get("ev")
    if kind == "hello":
        sys.stderr.write(f"tgd {event.get('tgd')}, TDLib {event.get('tdlib')}, "
                         f"data in {event.get('data')}\n")
    elif kind == "auth.state":
        state = event.get("state")
        why = f" ({event['message']})" if state == "failed" and event.get("message") else ""
        sys.stderr.write("tgd: logged in\n" if state == "ready" else f"tgd: login: {state}{why}\n")
    elif kind == "auth.error":
        sys.stderr.write(f"tgd: Telegram refused it: {event.get('message', '')}\n")
    sys.stderr.flush()


async def _run_window(args: argparse.Namespace, data_dir: Path, kind: str,
                      api_file: Path | None) -> int:
    from . import binding  # noqa: PLC0415

    binding.install(kind)  # tgclient.td imports tdjson, which needs ctypes without it
    from vm.auth import AuthController  # noqa: PLC0415
    from vm.server import WindowServer  # noqa: PLC0415

    from .core import Core, CoreError  # noqa: PLC0415
    from .frontends import LineReader, TextFrontEnd  # noqa: PLC0415
    from .window import SOCKET_NAME, WindowFrontEnd  # noqa: PLC0415

    log = logging.getLogger("tgd")
    auth = AuthController()
    front = WindowFrontEnd((args.socket or data_dir / SOCKET_NAME).expanduser())
    if args.synthetic:
        return await _run_synthetic(front, auth, log, kind)
    core = Core(data_dir=data_dir, test_dc=args.test_dc, binding=kind,
                tdjson_path=args.tdjson or os.environ.get("TDJSON_PATH"),
                api_file=api_file, emit=_window_say, td_log_level=args.td_log, auth_ui=auth)
    try:
        await core.start()
        # The api id and hash, once, on this terminal (the window has no form for them).
        text: Any = None
        while core.client is None:
            if text is None:  # one reader of the terminal
                text = TextFrontEnd()
                text.reader = LineReader(asyncio.get_running_loop())
            sys.stderr.write("api id and hash from my.telegram.org (kept in the data directory)\n")
            api_id = await text._ask("api id: ")
            api_hash = await text._ask("api hash: ", secret=True) if api_id is not None else None
            if api_id is None or api_hash is None:
                return 0
            # The AI's key, optional, once (kept with the credentials, which start TDLib):
            # the window has no field for it.
            if not core.credentials.openrouter_key:
                key = await text._ask("OpenRouter key for the AI summary (Enter: none): ",
                                      secret=True)
                core.credentials.openrouter_key = (key or "").strip()
            try:
                core.set_credentials(api_id, api_hash)
            except CoreError as e:
                sys.stderr.write(f"tgd: {e}\n")
        server = WindowServer(client=core.client, chats=core.chats, users=core.users,
                              emit=front.emit, auth=auth, ai_service=core.ai)
        front.server = server
        try:
            front.listen()
        except OSError as e:
            sys.stderr.write(f"tgd: cannot make the window's socket {front.path}: "
                             f"{e.strerror or e}\n")
            return 6
        server.start()
        log.info("window socket %s", front.path)
        sys.stderr.write(f"tgd: the window's socket is {front.path}; start tg (Ctrl-C ends tgd)\n")
        sys.stderr.flush()
        await front.run()
        if front.stopped_by_signal:
            sys.stderr.write("tgd: ended\n")
            return 0
        return 7
    finally:
        front.close()
        await core.close()


async def _run_synthetic(front: Any, auth: Any, log: logging.Logger, kind: str) -> int:
    """tgd --window --synthetic: the invented account of synthetic.py through the same
    view-models and socket, for the window's tests and a look before a login. TDLib is not
    started; its binding is only installed, so that tgclient.td imports (on Zoraya without
    ctypes, binding.py)."""
    import time  # noqa: PLC0415

    from . import binding  # noqa: PLC0415

    binding.install(kind)
    from vm.server import WindowServer  # noqa: PLC0415

    from .synthetic import NOW, Account, login  # noqa: PLC0415

    account = Account()
    started = time.monotonic()
    server = WindowServer(client=account.client, chats=account.chats, users=account.users,
                          emit=front.emit, auth=auth, ai_service=account.ai, synthetic=True,
                          clock=lambda: NOW + (time.monotonic() - started))
    front.server = server
    try:
        front.listen()
    except OSError as e:
        sys.stderr.write(f"tgd: cannot make the window's socket {front.path}: "
                         f"{e.strerror or e}\n")
        return 6
    server.start()
    tasks = [asyncio.ensure_future(login(auth)), asyncio.ensure_future(account.timer())]
    log.info("window socket %s (synthetic)", front.path)
    sys.stderr.write(f"tgd: the window's socket is {front.path}; an invented account "
                     "(code 12345, password zoraya); start tg (Ctrl-C ends tgd)\n")
    sys.stderr.flush()
    try:
        await front.run()
        if front.stopped_by_signal:
            sys.stderr.write("tgd: ended\n")
            return 0
        return 7
    finally:
        for t in tasks:
            t.cancel()
        front.close()
