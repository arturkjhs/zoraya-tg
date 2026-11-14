"""tgd's entry point: options, the network probe, the data directory and its lock, logging,
then the core and one front end on one asyncio loop."""

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

from . import VERSION, net

ON_ZORAYA = sys.platform == "zoraya"


def default_data_dir() -> Path:
    if ON_ZORAYA:
        return Path(os.path.expanduser("~")) / "tg"  # granted with +w ~/tg on the line
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
    p.add_argument("--data", type=Path, help=f"data directory (default {default_data_dir()})")
    p.add_argument("--binding", choices=("builtin", "ctypes"),
                   help="TDLib binding (default: builtin _tdjson where present, else ctypes)")
    p.add_argument("--tdjson", help="libtdjson for the ctypes binding (else TDJSON_PATH)")
    p.add_argument("--api-file", type=Path,
                   help="TG_API_ID=/TG_API_HASH= file (host default ~/.config/zoraya-tg/api)")
    p.add_argument("--no-probe", action="store_true", help="skip the network probe")
    p.add_argument("--td-log", type=int, default=1, help="TDLib's log verbosity (1)")
    p.add_argument("--grant", action="store_true",
                   help="print the line that starts tgd on Zoraya, and end")
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
        shown = "~/tg" if args.data is None else str(data_dir)
        print(net.grant_line(shown, args.test_dc))
        return 0

    from . import binding  # noqa: PLC0415

    kind = binding.choose(args.binding)

    if not args.no_probe:
        try:
            reason = net.probe(args.test_dc)
        except net.Refused as e:
            line = net.grant_line("~/tg" if args.data is None else str(data_dir), args.test_dc)
            say(args, {"ev": "refused", "reason": "network", "detail": str(e), "grant": line},
                f"tgd: no network for Telegram: {e}.\n"
                f"On Zoraya, start it with the grants on the line:\n  {line}")
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
        hint = f" (on Zoraya: +w {data_dir} on the line)" if ON_ZORAYA else ""
        say(args, {"ev": "refused", "reason": "data", "detail": f"{data_dir}: {e.strerror}"},
            f"tgd: cannot write the data directory {data_dir}: {e.strerror}{hint}")
        return 4

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
