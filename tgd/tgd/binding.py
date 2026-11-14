"""The TDLib binding switch: one small interface, two implementations.

- `builtin`: the module `_tdjson` built into Zoraya's python3 (modules/_tdjson.c);
- `ctypes`: tgclient's own binding (`tgclient.td.tdjson.TdJson`, libtdjson through ctypes),
  on the host.

Both give tgclient's `TdLib` protocol (td/client.py) plus `execute`: requests and responses
are dicts. `install()` must run before anything imports `tgclient.td`: tgclient's
`td/__init__.py` imports `td/tdjson.py`, which imports `ctypes`, absent on Zoraya; with the
built-in binding a module of the same name, without ctypes, is put in its place.
"""

from __future__ import annotations

import json
import sys
import types
from typing import Any

KINDS = ("builtin", "ctypes")


class BuiltinTdJson:
    """tgclient's TdJson over the built-in module _tdjson."""

    def __init__(self, path: str | None = None) -> None:
        import _tdjson  # noqa: PLC0415 - only present in Zoraya's python3 of zoraya-tg

        self._td = _tdjson

    def create_client_id(self) -> int:
        return self._td.create_client_id()

    def send(self, client_id: int, request: dict[str, Any]) -> None:
        self._td.send(client_id, json.dumps(request))

    def receive(self, timeout: float) -> dict[str, Any] | None:
        raw = self._td.receive(timeout)  # waits with the GIL released
        return json.loads(raw) if raw else None

    def execute(self, request: dict[str, Any]) -> dict[str, Any] | None:
        raw = self._td.execute(json.dumps(request))
        return json.loads(raw) if raw else None


class TdJsonNotFound(RuntimeError):
    pass


def available() -> list[str]:
    kinds = []
    try:
        import _tdjson  # noqa: F401, PLC0415

        kinds.append("builtin")
    except ImportError:
        pass
    try:
        import ctypes  # noqa: F401, PLC0415

        kinds.append("ctypes")
    except ImportError:
        pass
    return kinds


def choose(wanted: str | None) -> str:
    """`wanted` or the default: the built-in module where there is one (Zoraya)."""
    kinds = available()
    if wanted:
        if wanted not in KINDS:
            raise SystemExit(f"tgd: unknown binding {wanted!r} (builtin or ctypes)")
        if wanted not in kinds:
            raise SystemExit(f"tgd: the {wanted} binding is not available in this python")
        return wanted
    if not kinds:
        raise SystemExit("tgd: neither _tdjson nor ctypes is available in this python")
    return kinds[0]


def install(kind: str) -> None:
    """Make `tgclient.td` importable with this binding."""
    if kind == "builtin" and "tgclient.td.tdjson" not in sys.modules:
        module = types.ModuleType("tgclient.td.tdjson", "tgd: TdJson over _tdjson")
        module.TdJson = BuiltinTdJson  # type: ignore[attr-defined]
        module.TdJsonNotFound = TdJsonNotFound  # type: ignore[attr-defined]
        sys.modules["tgclient.td.tdjson"] = module


def load(kind: str, path: str | None = None) -> Any:
    install(kind)
    if kind == "builtin":
        return BuiltinTdJson()
    from tgclient.td.tdjson import TdJson  # noqa: PLC0415

    return TdJson(path)
