"""Host test of tgd's window front end (tgd/tgd/window.py): protocol 3 over a real Unix
socket file, served by tgd/vm over the fake TDLib of tests/tgd_vm.py (an invented
account, no TDLib, no network). The host's socket is SOCK_STREAM; Zoraya's is
SOCK_SEQPACKET with the same lines cut into packets of at most PACKET bytes.

    python3 -I tests/tgd_window.py [TGCLIENT_SRC]
"""

from __future__ import annotations

import asyncio
import faulthandler
import json
import logging
import sys
import tempfile
import traceback
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import tgd_vm  # noqa: E402  (its path setup: tgd/ and tgclient)
from tgd_vm import FAIL, NOW, FakeAi, FakeTd, account, settle  # noqa: E402

from tgclient.store.chats import ChatStore  # noqa: E402
from tgclient.store.files import FileManager  # noqa: E402
from tgclient.store.users import UserStore  # noqa: E402

from tgd.window import PACKET, WindowFrontEnd  # noqa: E402
from vm.server import WindowServer  # noqa: E402


class Client:
    """The window's side: lines out, events in."""

    def __init__(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        self.reader, self.writer = reader, writer
        self.events: list[dict[str, Any]] = []

    @classmethod
    async def connect(cls, path: Path) -> Client:
        reader, writer = await asyncio.open_unix_connection(str(path), limit=16 << 20)
        return cls(reader, writer)

    def send(self, request: dict[str, Any]) -> None:
        self.writer.write(json.dumps(request, ensure_ascii=False).encode() + b"\n")

    def raw(self, data: bytes) -> None:
        self.writer.write(data)

    async def until(self, ev: str, test: Any = None, timeout: float = 5) -> dict[str, Any]:
        """The first event `ev` (passing `test`), reading on; the ones before it are kept."""
        for i, e in enumerate(self.events):
            if e["ev"] == ev and (test is None or test(e)):
                return self.events.pop(i)
        while True:
            line = await asyncio.wait_for(self.reader.readline(), timeout)
            if not line:
                raise AssertionError(f"the socket closed while waiting for {ev}")
            e = json.loads(line)
            if e["ev"] == ev and (test is None or test(e)):
                return e
            self.events.append(e)

    async def closed(self, timeout: float = 5) -> bool:
        while True:
            line = await asyncio.wait_for(self.reader.readline(), timeout)
            if not line:
                return True


async def main() -> int:
    faulthandler.dump_traceback_later(60, exit=True)
    logging.basicConfig(level=logging.WARNING, format="%(name)s %(levelname)s %(message)s")
    td = FakeTd()
    files = FileManager(td.client)
    chats, users = ChatStore(td.client, files), UserStore(td.client, files)
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "window.sock"
        path.write_text("")  # a stale file of a tgd that ended: removed by listen
        front = WindowFrontEnd(path)
        server = WindowServer(client=td.client, chats=chats, users=users, emit=front.emit,
                              ai_service=FakeAi(), synthetic=True, clock=lambda: NOW)
        front.server = server
        front.listen()
        server.start()
        account(td)
        await settle()
        steps = 0
        try:
            steps = await check(td, path)
        except Exception:  # noqa: BLE001
            print("FAIL the window's socket")
            traceback.print_exc()
            return 1
        finally:
            front.close()
        assert not path.exists(), "the socket file goes with tgd"
    print(f"ok   the window's socket: {steps} steps")
    return 0


async def check(td: FakeTd, path: Path) -> int:
    steps = 0
    c = await Client.connect(path)
    hello = await c.until("hello")
    assert hello["protocol"] == 3 and hello["now"] == NOW and hello["synthetic"], hello
    await c.until("settings")
    steps += 1

    c.send({"op": "chats.window", "first": 0, "count": 10, "query": ""})
    rows = await c.until("chats.rows")
    assert rows["total"] == 7 and rows["rows"][0]["title"], rows
    steps += 1

    c.send({"op": "chat.open", "chat": "13", "count": 30})
    reset = await c.until("messages.reset")
    assert reset["title"] == "Книжковий клуб" and len(reset["rows"]) == 30, reset["title"]
    steps += 1

    # Lines longer than a packet, both ways, whole: nine messages of 4000 characters (the
    # most Telegram takes is 4096), then the chat opened again: its reset is one line of
    # more than PACKET bytes.
    long_text = ("Довге повідомлення, ї ґ є. " * 200)[:4000]
    for _ in range(9):
        c.send({"op": "messages.send", "chat": "13", "text": long_text, "reply_to": None})
        new = await c.until("message.new")
        assert new["row"]["text"]["text"] == long_text and new["row"]["state"] == "pending"
        sent = await c.until("message.update", lambda e, i=new["row"]["id"]: e["old_id"] == i)
        assert sent["row"]["state"] == "sent"
    c.send({"op": "chat.open", "chat": "14", "count": 5})
    await c.until("messages.reset")
    c.send({"op": "chat.open", "chat": "13", "count": 30})
    reset = await c.until("messages.reset")
    size = len(json.dumps(reset, ensure_ascii=False).encode())
    assert size > PACKET and [r["text"]["text"] for r in reset["rows"][-9:]] == [long_text] * 9, size
    steps += 1

    # Protocol 3: failed, then sent again.
    c.send({"op": "messages.send", "chat": "13", "text": FAIL + " ще раз", "reply_to": None})
    new = await c.until("message.new")
    failed = await c.until("message.update", lambda e: e["old_id"] == new["row"]["id"])
    assert failed["row"]["state"] == "failed", failed
    c.send({"op": "messages.retry", "chat": "13", "id": failed["row"]["id"]})
    gone = await c.until("message.delete")
    assert gone["ids"] == [failed["row"]["id"]]
    again = await c.until("message.new")
    assert again["row"]["text"]["text"] == FAIL + " ще раз"
    done = await c.until("message.update", lambda e: e["old_id"] == again["row"]["id"])
    assert done["row"]["state"] == "sent"
    steps += 1

    # Refusals: named, with no value in them; the connection stays.
    c.send({"op": "messages.retry", "chat": "13", "id": done["row"]["id"]})
    r = await c.until("refused")
    assert r["op"] == "messages.retry" and r["why"] == "that message did not fail", r
    c.raw(b"{not json\n")
    assert (await c.until("refused"))["why"] == "not JSON"
    c.raw(b"[1, 2]\n")
    assert (await c.until("refused"))["why"] == "not an object"
    c.send({"op": "chat.typing"})
    assert (await c.until("refused"))["why"].startswith("unknown op")
    c.send({"op": "auth.code", "value": "13579"})
    assert (await c.until("refused"))["why"] == "no login here"
    # Two requests in one write, and one cut in two writes.
    c.raw(b'{"op":"chats.window","first":0,"count":3,"query":""}\n{"op":"chats.win')
    await c.writer.drain()
    await asyncio.sleep(0.05)
    c.raw(b'dow","first":0,"count":2,"query":"\xd0\x90"}\n')
    assert len((await c.until("chats.rows"))["rows"]) == 3
    assert (await c.until("chats.rows"))["first"] == 0
    steps += 1

    # A second window: the first is closed; the second hears everything from the start.
    d = await Client.connect(path)
    assert (await d.until("hello"))["protocol"] == 3
    assert await c.closed()
    d.send({"op": "chats.window", "first": 0, "count": 5, "query": ""})
    assert len((await d.until("chats.rows"))["rows"]) == 5
    # The chat of the first window is no longer open for the second.
    d.send({"op": "messages.send", "chat": "13", "text": "ні", "reply_to": None})
    assert (await d.until("refused"))["why"] == "that chat is not open"
    d.send({"op": "chat.open", "chat": "13", "count": 5})
    assert len((await d.until("messages.reset"))["rows"]) == 5
    steps += 1

    # The window goes and comes back.
    d.writer.close()
    await asyncio.sleep(0.1)
    e = await Client.connect(path)
    assert (await e.until("hello"))["protocol"] == 3
    td.message(13, 1, tgd_vm.text("Хтось тут?"))
    await settle()
    e.send({"op": "chats.window", "first": 0, "count": 3, "query": ""})
    rows = await e.until("chats.rows")
    # Below the pinned chat, the club moved up with its new last line.
    assert [r["id"] for r in rows["rows"][:2]] == ["16", "13"], rows["rows"]
    assert rows["rows"][1]["last"].endswith("Хтось тут?"), rows["rows"][1]
    e.writer.close()
    steps += 1
    return steps


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
