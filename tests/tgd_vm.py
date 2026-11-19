"""Host test of tgd's view-models (tgd/vm) and protocol 3, without TDLib or an account.

tgclient's own TdClient runs over a fake hub that answers TDLib's requests and sends its
updates from an invented account (English, Ukrainian and Russian, so Cyrillic and UTF-16
offsets are exercised); tgclient's stores and the adapted view-models are the real ones.
The window's side applies the events as `tg_window` does (a splice replaces a range), and
after every step its rows must equal the list computed from the store.

    python3 -I tests/tgd_vm.py [TGCLIENT_SRC]     (default: ~/workspace/tgclient/src)
"""

from __future__ import annotations

import asyncio
import dataclasses
import faulthandler
import logging
import random
import sys
import traceback
from pathlib import Path
from typing import Any

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent / "tgd"))
sys.path.insert(0, sys.argv[1] if len(sys.argv) > 1 else str(Path.home() / "workspace/tgclient/src"))

from tgclient.store.chats import MAIN, ChatStore  # noqa: E402
from tgclient.store.files import FileManager  # noqa: E402
from tgclient.store.users import UserStore  # noqa: E402
from tgclient.td.client import TdClient  # noqa: E402

from vm.auth import AuthController  # noqa: E402
from vm.server import RequestError, WindowServer  # noqa: E402

from tgd.synthetic import (  # noqa: E402
    FAIL, ME, NOW, TEMP, FakeAi, FakeTd, SummaryState, account, entity, marked, text, utf16,
)


# --- the window's side ------------------------------------------------------------------------

class Window:
    """What `tg_window` keeps: the chat window's rows and the open chat's messages."""

    def __init__(self) -> None:
        self.events: list[dict[str, Any]] = []
        self.first = 0
        self.rows: list[dict[str, Any]] = []
        self.total = 0
        self.messages: list[dict[str, Any]] = []  # oldest first
        self.more = False

    def __call__(self, event: dict[str, Any]) -> None:
        self.events.append(event)
        ev = event["ev"]
        if ev == "chats.rows":
            self.first, self.rows, self.total = event["first"], list(event["rows"]), event["total"]
        elif ev == "chats.splice":
            at = event["at"] - self.first
            assert 0 <= at <= len(self.rows), event
            self.rows[at:at + event["remove"]] = event["rows"]
            self.total = event["total"]
        elif ev == "messages.reset":
            self.messages, self.more = list(event["rows"]), event["more"]
        elif ev == "messages.page":
            self.messages[0:0] = event["rows"]
            self.more = event["more"]
        elif ev == "message.new":
            self.messages.append(event["row"])
        elif ev == "message.update":
            for i, m in enumerate(self.messages):
                if m["id"] == event["old_id"]:
                    self.messages[i] = event["row"]
                    break
            else:
                raise AssertionError(f"update of a message the window has not: {event}")
        elif ev == "message.delete":
            self.messages = [m for m in self.messages if m["id"] not in event["ids"]]

    def take(self, ev: str | None = None) -> list[dict[str, Any]]:
        taken = [e for e in self.events if ev is None or e["ev"] == ev]
        self.events = [e for e in self.events if not (ev is None or e["ev"] == ev)]
        return taken


async def settle(rounds: int = 20) -> None:
    for _ in range(rounds):
        await asyncio.sleep(0)


async def wait_for(condition: Any, what: str, seconds: float = 2.0) -> None:
    loop = asyncio.get_running_loop()
    end = loop.time() + seconds
    while not condition():
        if loop.time() > end:
            raise AssertionError(f"timed out waiting for {what}")
        await asyncio.sleep(0.002)


# --- the checks -------------------------------------------------------------------------------

def expected_main(server: WindowServer, chats: ChatStore) -> list[str]:
    return [str(c.id) for c in chats.chats_in(MAIN)]


async def check_chat_list(td: FakeTd, server: WindowServer, chats: ChatStore,
                          w: Window) -> int:
    await server.handle({"op": "chats.window", "first": 0, "count": 7, "query": ""})
    rows = w.take("chats.rows")
    assert len(rows) == 1 and rows[0]["total"] == 7, rows
    assert [r["id"] for r in w.rows] == expected_main(server, chats)
    pinned = w.rows[0]
    assert pinned["id"] == "16" and pinned["pinned"] and pinned["last"] == "Voice message", pinned
    saved = next(r for r in w.rows if r["id"] == "15")
    assert saved["kind"] == "saved", saved
    club = next(r for r in w.rows if r["id"] == "13")
    assert club["kind"] == "group" and club["unread"] == 2, club
    muted = next(r for r in w.rows if r["id"] == "12")
    assert muted["muted"] and muted["last"] == "Встреча в пятницу, секрет", muted
    await server.handle({"op": "chats.window", "first": 0, "count": 5, "query": ""})
    assert [r["id"] for r in w.take("chats.rows")[0]["rows"]] == expected_main(server, chats)[:5]

    # A message in the chat at the window's end moves it up: one splice.
    bottom = w.rows[-1]["id"]
    td.message(int(bottom), 1 if bottom == "10" else 2, text("Ще одне повідомлення"))
    await settle()
    splices = w.take("chats.splice")
    assert len(splices) == 1, splices
    assert [r["id"] for r in w.rows] == expected_main(server, chats)[:5]
    assert w.rows[1]["id"] == bottom and w.rows[1]["last"] == "Ще одне повідомлення"

    # Random steps: the window's rows always equal the list (tests.rs's check, here).
    rng = random.Random(4)
    steps = 0
    next_chat = 100
    for step in range(600):
        action = rng.randrange(8)
        ids = [c for c in td.chats if td.chats[c]["positions"][0]["order"] != "0"]
        if action <= 2 and ids:
            chat_id = rng.choice(ids)
            td.message(chat_id, ME if rng.random() < 0.3 else 1, text(f"step {step}"))
        elif action == 3 and ids:
            td.position(rng.choice(ids), int(td.chats[rng.choice(ids)]["positions"][0]["order"]) - 1)
        elif action == 4 and ids:
            chat_id = rng.choice(ids)
            pinned = not td.chats[chat_id]["positions"][0]["is_pinned"]
            td.position(chat_id, int(td.chats[chat_id]["positions"][0]["order"]), pinned)
        elif action == 5 and len(ids) > 3:
            td.position(rng.choice(ids), 0)  # the chat leaves the list
        elif action == 6:
            next_chat += 1
            td.chat(next_chat, rng.choice(["Нова група", "New chat", "Новый чат"]) + f" {next_chat}",
                    "group", peer=next_chat)
        else:
            first = rng.randrange(0, max(1, len(ids) - 2))
            await server.handle({"op": "chats.window", "first": first,
                                 "count": rng.randrange(1, 12), "query": ""})
        await settle(4)
        everything = expected_main(server, chats)
        assert w.total == len(everything), (step, w.total, len(everything))
        assert [r["id"] for r in w.rows] == everything[w.first:w.first + len(w.rows)], step
        for r in w.rows:  # and each row's content is the store's
            chat = chats.chats[int(r["id"])]
            assert r["title"] == chat.title and r["unread"] == chat.unread_count, (step, r)
        steps += 1
    w.take()
    return steps


async def check_search(td: FakeTd, server: WindowServer, w: Window) -> None:
    td.order += 10
    td.position(12, td.order)  # back in the list, if a random step took it out
    await settle()
    await server.handle({"op": "chats.window", "first": 0, "count": 10, "query": "Ан"})
    rows = w.take("chats.rows")[0]
    titles = [r["title"] for r in rows["rows"]]
    assert titles and titles[0] == "Анна Смирнова", titles  # starts with it: first
    assert all("ан" in t.lower() for t in titles), titles
    await server.handle({"op": "chats.window", "first": 0, "count": 10, "query": "zzz"})
    assert w.take("chats.rows")[0]["total"] == 0
    await server.handle({"op": "chats.window", "first": 0, "count": 6, "query": ""})
    w.take()


async def check_messages(td: FakeTd, server: WindowServer, chats: ChatStore, w: Window,
                         refs: dict[str, Any]) -> None:
    await server.handle({"op": "chat.open", "chat": "13", "count": 30})
    reset = w.take("messages.reset")
    assert len(reset) == 1, reset
    reset = reset[0]
    assert reset["title"] == "Книжковий клуб" and reset["kind"] == "group" and reset["more"]
    assert len(w.messages) == 30
    ids = [int(m["id"]) for m in w.messages]
    assert ids == sorted(ids), "oldest first"
    # Opening read it: viewMessages for what was shown, and the list's unread count went.
    await wait_for(lambda: chats.chats[13].unread_count == 0, "the club read")
    assert any(chat == 13 for chat, _ in td.viewed)

    # Older pages, until the start: contiguous, oldest first.
    for _ in range(20):
        if not w.more:
            break
        await server.handle({"op": "messages.older", "chat": "13", "before": w.messages[0]["id"],
                             "count": 25})
        assert len(w.take("messages.page")) == 1
    ids = [int(m["id"]) for m in w.messages]
    assert ids == sorted(ids) and ids == [m["id"] for m in td.history[13]], len(ids)
    by_id = {m["id"]: m for m in w.messages}
    photo, question = by_id[str(refs["photo"])], by_id[str(refs["question"])]
    assert photo["kind"] == "photo" and photo["detail"] == "1280 × 960", photo
    assert photo["text"] == {"text": "Обкладинка", "spans": []}, photo
    # UTF-16 to code points: the book emoji is two UTF-16 units and one code point.
    t = question["text"]
    start = t["text"].index("так")
    assert t["spans"] == [[start, start + 3, "bold"]], t
    assert question["sender"] == "Дмитро Шевченко" and not question["out"]
    assert question["reply"] == {"sender": "Дмитро Шевченко", "text": "Повідомлення номер 69"}, question

    # Sending: new with a temporary id, pending; then the same row with its final id.
    w.take()
    await server.handle({"op": "messages.send", "chat": "13", "text": "Я прочитав! ✅",
                         "reply_to": str(refs["question"])})
    await wait_for(lambda: any(e["ev"] == "message.new" for e in w.events), "message.new")
    new = w.take("message.new")[0]["row"]
    assert new["state"] == "pending" and new["out"] and int(new["id"]) > TEMP, new
    await wait_for(lambda: w.messages[-1]["state"] == "sent", "the confirmation")
    final = w.messages[-1]
    assert int(final["id"]) < TEMP and final["text"]["text"] == "Я прочитав! ✅", final
    assert final["reply"] and final["reply"]["sender"] == "Дмитро Шевченко", final
    assert all(m["id"] != new["id"] for m in w.messages)
    assert not w.take("message.delete"), "a confirmation is no deletion"

    # Someone answers: a new message at the end, read at once (the chat is open).
    m = td.message(13, 1, text("Чудово, обговоримо в п'ятницю"))
    await settle()
    assert w.messages[-1]["id"] == str(m["id"]) and w.messages[-1]["sender"] == "Olena Kovalenko"
    await wait_for(lambda: chats.chats[13].unread_count == 0, "read on arrival")

    # Read by the other side, edited, deleted.
    td.update({"@type": "updateChatReadOutbox", "chat_id": 13,
               "last_read_outbox_message_id": int(final["id"])})
    await settle()
    assert next(x for x in w.messages if x["id"] == final["id"])["state"] == "read"
    td.update({"@type": "updateMessageContent", "chat_id": 13, "message_id": m["id"],
               "new_content": text("Чудово, обговоримо в суботу")})
    td.update({"@type": "updateMessageEdited", "chat_id": 13, "message_id": m["id"],
               "edit_date": NOW + 5})
    await settle()
    edited = w.messages[-1]
    assert edited["edited"] and edited["text"]["text"].endswith("суботу"), edited
    td.update({"@type": "updateDeleteMessages", "chat_id": 13, "message_ids": [m["id"]],
               "is_permanent": True, "from_cache": False})
    await settle()
    assert w.take("message.delete")[-1]["ids"] == [str(m["id"])]
    assert all(x["id"] != str(m["id"]) for x in w.messages)

    # Protocol 3: a failed send is "failed", never "pending"; messages.retry sends it again
    # (TDLib's resendMessages: the failed one deleted, a new one sent and confirmed).
    w.take()
    await server.handle({"op": "messages.send", "chat": "13", "text": FAIL + " не дійде",
                         "reply_to": None})
    await wait_for(lambda: w.messages[-1]["state"] == "failed", "the failure")
    failed = w.messages[-1]
    assert failed["out"] and failed["text"]["text"] == FAIL + " не дійде", failed
    assert all(e["row"]["state"] != "pending" for e in w.events
               if e["ev"] == "message.update"), "a failure is never shown as pending"
    try:
        await server.handle({"op": "messages.retry", "chat": "13", "id": final["id"]})
        raise AssertionError("a message that did not fail is not sent again")
    except RequestError:
        pass
    w.take()
    await server.handle({"op": "messages.retry", "chat": "13", "id": failed["id"]})
    await wait_for(lambda: w.messages[-1]["state"] == "sent", "the retry's confirmation")
    assert w.take("message.delete")[0]["ids"] == [failed["id"]]
    assert all(m["id"] != failed["id"] for m in w.messages)
    assert w.messages[-1]["text"]["text"] == FAIL + " не дійде"
    assert "resendMessages" in td.requests

    # Other chats: a spoiler stays hidden; a file and a voice message are placeholders.
    def shown(key: str) -> dict[str, Any]:
        return next(m for m in w.messages if m["id"] == str(refs[key]))

    await server.handle({"op": "chat.open", "chat": "12", "count": 100})
    spoiler = shown("spoiler")["text"]
    assert spoiler["text"] == "Встреча в пятницу, ░░░░░░" and spoiler["spans"] == [], spoiler
    await server.handle({"op": "chat.open", "chat": "14", "count": 100})
    assert (shown("doc")["kind"], shown("doc")["detail"]) == ("file", "report.pdf · 248.0 KB")
    await server.handle({"op": "chat.open", "chat": "16", "count": 100})
    assert (shown("voice")["kind"], shown("voice")["detail"]) == ("voice", "0:42")
    await server.handle({"op": "chat.open", "chat": "10", "count": 100})
    link = shown("link")["text"]
    start = link["text"].index("zoraya.example")
    assert link["spans"] == [[start, start + 14, "link"]], link

    # A message to a chat that is not open changes the list, not the messages.
    before = list(w.messages)
    td.message(11, 2, text("Ти тут?"))
    await settle()
    assert w.messages == before
    await server.handle({"op": "chat.close"})
    try:
        await server.handle({"op": "messages.send", "chat": "10", "text": "x"})
        raise AssertionError("sent to a closed chat")
    except RequestError:
        pass
    w.take()


async def check_auth() -> None:
    w = Window()
    td = FakeTd()
    files = FileManager(td.client)
    chats, users = ChatStore(td.client, files), UserStore(td.client, files)
    auth = AuthController()
    server = WindowServer(client=td.client, chats=chats, users=users, emit=w, auth=auth,
                          synthetic=True, clock=lambda: NOW)
    server.start()
    hello = w.events[0]
    assert hello["ev"] == "hello" and hello["protocol"] == 3 and hello["now"] == NOW
    assert hello["synthetic"] is True and "utc_offset" in hello
    assert w.take("auth.state")[-1]["state"] == "starting"

    phone = asyncio.ensure_future(auth.ask_phone())
    await settle()
    state = w.take("auth.state")[-1]
    assert state["state"] == "wait_phone" and state["hint"], state
    try:
        await server.handle({"op": "auth.code", "value": "13579"})
        raise AssertionError("a code taken while the phone is asked")
    except RequestError as e:
        assert "13579" not in str(e)
    await server.handle({"op": "auth.phone", "value": "+0 000 000 0000"})
    assert await phone == "+0 000 000 0000"
    await auth.show_error("PHONE_CODE_INVALID")
    assert w.take("auth.state")[-1]["error"] == "Wrong code. Check it and try again."
    code = asyncio.ensure_future(auth.ask_code({"type": {
        "@type": "authenticationCodeTypeTelegramMessage", "length": 5}}))
    await settle()
    assert w.take("auth.state")[-1]["state"] == "wait_code"
    await server.handle({"op": "auth.code", "value": "13579"})
    assert await code == "13579"
    password = asyncio.ensure_future(auth.ask_password("the usual"))
    await settle()
    state = w.take("auth.state")[-1]
    assert state["state"] == "wait_password" and "the usual" in state["hint"], state
    await server.handle({"op": "auth.password", "value": "invented"})
    assert await password == "invented"
    auth.set_ready()
    assert w.take("auth.state")[-1]["state"] == "ready"
    for e in w.events:  # nothing typed comes back out
        assert "invented" not in str(e) and "13579" not in str(e), e


async def check_ai(server: WindowServer, ai: FakeAi, w: Window) -> None:
    await server.handle({"op": "chat.open", "chat": "13", "count": 5})
    settings = w.take("settings")[-1]["items"]
    assert settings == [{"key": "ai.chat", "label": "AI summaries of this chat", "on": False}]
    await server.handle({"op": "ai.summarize", "chat": "13", "scope": "day"})
    failed = w.take("ai.summary")
    assert failed[-1]["state"] == "failed" and "off" in failed[-1]["text"], failed
    await server.handle({"op": "settings.set", "key": "ai.chat", "on": True})
    assert w.take("settings")[-1]["items"][0]["on"] is True
    await server.handle({"op": "ai.summarize", "chat": "13", "scope": "day"})
    await wait_for(lambda: any(e["ev"] == "ai.summary" and e["state"] == "done"
                               for e in w.events), "the summary")
    parts = w.take("ai.summary")
    assert [p["state"] for p in parts] == ["running", "done"], parts
    assert "".join(p["text"] for p in parts) == "Клуб обговорює книжки. 📚"
    await server.handle({"op": "chat.close"})
    w.take()


async def main() -> int:
    faulthandler.dump_traceback_later(120, exit=True)  # a hang that never yields ends here
    logging.basicConfig(level=logging.WARNING, format="%(name)s %(levelname)s %(message)s")
    checks = 0
    td = FakeTd()
    files = FileManager(td.client)
    chats, users = ChatStore(td.client, files), UserStore(td.client, files)
    ai = FakeAi()
    w = Window()
    server = WindowServer(client=td.client, chats=chats, users=users, emit=w, ai_service=ai,
                          synthetic=True, clock=lambda: NOW)
    server.start()
    refs = account(td)
    await settle()
    w.take()
    for name, check in (
        ("the chat list: rows, one splice a change, 600 random steps",
         lambda: check_chat_list(td, server, chats, w)),
        ("search filters the list (Cyrillic)", lambda: check_search(td, server, w)),
        ("messages: reset, pages, send and confirm, new, read, edit, delete, failed and retry, kinds",
         lambda: check_messages(td, server, chats, w, refs)),
        ("AI: off, on, running, done", lambda: check_ai(server, ai, w)),
        ("auth: phone, code, password, refusals, hello", check_auth),
    ):
        try:
            result = await asyncio.wait_for(check(), 30)
            extra = f" ({result} steps)" if isinstance(result, int) else ""
            print(f"ok   {name}{extra}")
            checks += 1
        except Exception:  # noqa: BLE001
            print(f"FAIL {name}")
            traceback.print_exc()
            return 1
    print(f"tgd_vm: {checks} checks passed; {len(td.requests)} TDLib requests answered by the fake")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
