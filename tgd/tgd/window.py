"""The window's front end (docs/tg-window.md, stage B): protocol 3 over a Unix socket file
in tgd's own directory (Zoraya's ADR-0465), served by tgd/vm's WindowServer.

- On Zoraya the socket is a SOCK_SEQPACKET socket: each packet is a piece of one byte
  stream of JSON lines, at most PACKET bytes (a channel message is 64 KiB), so a line may
  be longer than a packet. On the host it is a SOCK_STREAM socket with the same lines.
- One window at a time: a new connection replaces the one before it, and the window's
  state starts again (hello, the login's state, a new chat list window).
- The socket and each connection have their own threads, as LineReader has for stdin:
  blocking calls only, which libposix has for sockets; the loop hears of them through
  call_soon_threadsafe.
- Nothing here logs a line's content: a refusal names the op and why, never a value.
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import logging
import os
import queue
import socket
import sys
import threading
from pathlib import Path
from typing import Any

log = logging.getLogger("tgd.window")

ON_ZORAYA = sys.platform == "zoraya"
PACKET = 60 * 1024
MAX_LINE = 1 << 20  # a request is short; a longer line is dropped whole
SOCKET_NAME = "window.sock"


def socket_kind() -> int:
    if ON_ZORAYA:
        return getattr(socket, "SOCK_SEQPACKET", 5)
    return socket.SOCK_STREAM


class Connection:
    """One window: a reader thread (bytes to lines, lines to the loop) and a writer thread
    (lines in packets)."""

    def __init__(self, sock: socket.socket, number: int, front: WindowFrontEnd) -> None:
        self.sock = sock
        self.number = number
        self._front = front
        self._out: queue.Queue[bytes | None] = queue.Queue()
        self.closed = False
        threading.Thread(target=self._read, name=f"tgd-window-{number}-in", daemon=True).start()
        threading.Thread(target=self._write, name=f"tgd-window-{number}-out", daemon=True).start()

    def send_line(self, line: bytes) -> None:
        if not self.closed:
            self._out.put(line)

    def close(self) -> None:
        if self.closed:
            return
        self.closed = True
        self._out.put(None)
        with contextlib.suppress(OSError):
            self.sock.shutdown(socket.SHUT_RDWR)
        with contextlib.suppress(OSError):
            self.sock.close()

    def _read(self) -> None:
        pending = b""
        skipping = False
        while True:
            try:
                chunk = self.sock.recv(65536)
            except InterruptedError:
                continue
            except OSError:
                chunk = b""
            if not chunk:
                self._front.call(self._front.gone, self)
                return
            pending += chunk
            while b"\n" in pending:
                line, pending = pending.split(b"\n", 1)
                if skipping:
                    skipping = False
                    continue
                self._front.call(self._front.line, self, line)
            if len(pending) > MAX_LINE:
                pending, skipping = b"", True
                self._front.call(self._front.refuse, self, "", "the line is too long")

    def _write(self) -> None:
        while True:
            line = self._out.get()
            if line is None:
                return
            try:
                for at in range(0, len(line), PACKET):
                    piece = line[at:at + PACKET]
                    sent = 0
                    while sent < len(piece):  # a stream socket may take a part
                        sent += self.sock.send(piece[sent:])
            except OSError:
                self._front.call(self._front.gone, self)
                return


class WindowFrontEnd:
    """The socket at `path`, and the server whose events go to the window connected now."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.server: Any = None
        self._conn: Connection | None = None
        self._count = 0
        self._loop: asyncio.AbstractEventLoop | None = None
        self._listener: socket.socket | None = None
        self._stopped = asyncio.Event()
        self.stopped_by_signal = False
        self._tasks: set[asyncio.Task[Any]] = set()

    # --- the socket ---------------------------------------------------------------------------

    def listen(self) -> None:
        """Binds and listens (a stale file of a tgd that ended is removed first)."""
        self._loop = asyncio.get_running_loop()
        with contextlib.suppress(FileNotFoundError):
            os.unlink(self.path)
        sock = socket.socket(socket.AF_UNIX, socket_kind())
        try:
            sock.bind(str(self.path))
            sock.listen(4)
        except OSError:
            sock.close()
            raise
        self._listener = sock
        threading.Thread(target=self._accept, name="tgd-window-accept", daemon=True).start()

    def _accept(self) -> None:
        sock = self._listener
        assert sock is not None
        while True:
            try:
                conn, _ = sock.accept()
            except InterruptedError:
                continue
            except OSError as e:
                if self._listener is not None:
                    log.error("the window socket stopped: %s", e.strerror or e)
                    self.call(self._stopped.set)
                return
            self.call(self._connected, conn)

    def call(self, fn: Any, *args: Any) -> None:
        loop = self._loop
        if loop is not None:
            with contextlib.suppress(RuntimeError):  # the loop has closed
                loop.call_soon_threadsafe(fn, *args)

    def close(self) -> None:
        sock, self._listener = self._listener, None
        if sock is not None:
            with contextlib.suppress(OSError):
                sock.close()
            with contextlib.suppress(OSError):
                os.unlink(self.path)
        if self._conn is not None:
            self._conn.close()
            self._conn = None

    def stop(self) -> None:
        """Ctrl-C or kill: tgd ends cleanly (the caller closes the socket and the core)."""
        self.stopped_by_signal = True
        self._stopped.set()

    async def run(self) -> None:
        """Until a signal (see `watch_signals`) or the listener stops."""
        self.watch_signals()
        await self._stopped.wait()

    def watch_signals(self) -> None:
        """SIGINT and SIGTERM through the loop: on Zoraya the interpreter's thread is not the
        one a signal is delivered to while TDLib's threads run (tgd_main.c blocks them in the
        main thread), so Python's handler alone would wait for the loop to wake; the loop's
        own handler is woken through its wakeup descriptor whichever thread takes it."""
        import signal  # noqa: PLC0415

        # By hand, as loop.add_signal_handler does it: Zoraya's python has no
        # signal.siginterrupt, which that calls.
        loop = asyncio.get_running_loop()
        try:
            ours, theirs = socket.socketpair()
            ours.setblocking(False)
            theirs.setblocking(False)
            signal.set_wakeup_fd(theirs.fileno(), warn_on_full_buffer=False)
        except (OSError, ValueError):
            return
        self._wakeup = (ours, theirs)

        def drain() -> None:
            with contextlib.suppress(OSError):
                while ours.recv(256):
                    pass

        loop.add_reader(ours.fileno(), drain)
        for number in (signal.SIGINT, signal.SIGTERM):
            with contextlib.suppress(ValueError, OSError):
                signal.signal(number, lambda _n, _f: loop.call_soon(self.stop))

    # --- connections --------------------------------------------------------------------------

    def _connected(self, sock: socket.socket) -> None:
        self._count += 1
        if self._conn is not None:
            log.info("window %d replaces window %d", self._count, self._conn.number)
            self._conn.close()
        self._conn = Connection(sock, self._count, self)
        log.info("window %d connected", self._count)
        if self.server is not None:
            self.server.restart()

    def gone(self, conn: Connection) -> None:
        conn.close()
        if self._conn is conn:
            self._conn = None
            log.info("window %d went", conn.number)
            if self.server is not None:
                self.server.window_gone()

    def emit(self, event: dict[str, Any]) -> None:
        """An event of the server: to the window connected now, if one is."""
        conn = self._conn
        if conn is None:
            return
        data = json.dumps(event, ensure_ascii=False, separators=(",", ":")).encode() + b"\n"
        conn.send_line(data)

    def refuse(self, conn: Connection, op: str, why: str) -> None:
        if conn is self._conn:
            self.emit({"ev": "refused", "op": op, "why": why})

    def line(self, conn: Connection, raw: bytes) -> None:
        if conn is not self._conn or self.server is None:
            return
        try:
            request = json.loads(raw.decode("utf-8"))
        except (UnicodeDecodeError, ValueError):
            self.refuse(conn, "", "not JSON")
            return
        if not isinstance(request, dict):
            self.refuse(conn, "", "not an object")
            return
        task = asyncio.ensure_future(self._one(conn, request))
        self._tasks.add(task)
        task.add_done_callback(self._tasks.discard)

    async def _one(self, conn: Connection, request: dict[str, Any]) -> None:
        from vm.server import RequestError  # noqa: PLC0415

        op = str(request.get("op", ""))
        try:
            await self.server.handle(request)
        except RequestError as e:
            self.refuse(conn, op, str(e))
        except Exception as e:  # noqa: BLE001
            log.exception("window request %s failed", op)
            self.refuse(conn, op, f"internal error ({type(e).__name__}); see tgd.log")
