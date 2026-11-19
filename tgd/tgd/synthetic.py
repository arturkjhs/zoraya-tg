"""An invented account for tgd's window (`tgd --window --synthetic`) and its tests: TDLib
as tgclient sees it (a hub answering requests by @extra and sending updates), with users and
chats in English, Ukrainian and Russian (so Cyrillic and UTF-16 offsets are exercised), an AI
service with an invented summary, and a login that takes any number, the code 12345 and the
password zoraya (not credentials of anything). No TDLib, no network, no data of anyone.

A text sent that starts with /fail fails the first time and is sent by messages.retry
(protocol 3's failed state). tests/tgd_vm.py and tests/tgd_window.py check the view-models
and the socket over it; inside Zoraya the window's test does.
"""

from __future__ import annotations

import asyncio
import dataclasses
from typing import Any

from tgclient.td.client import TdClient

ME = 1000
NOW = 1_763_500_000  # a fixed clock: 2025-11-18, invented data only
TEMP = 1 << 40  # temporary ids of messages being sent are above every final one
FAIL = "/fail"  # a sent text that starts with this fails the first time (invented)


# --- the fake TDLib --------------------------------------------------------------------------

def text(t: str, entities: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    return {"@type": "messageText", "text": {"@type": "formattedText", "text": t,
                                             "entities": entities or []}}


def entity(offset: int, length: int, kind: str) -> dict[str, Any]:
    return {"@type": "textEntity", "offset": offset, "length": length,
            "type": {"@type": kind}}


class FakeTd:
    """TDLib as tgclient sees it: requests answered by @extra, updates dispatched."""

    def __init__(self) -> None:
        self.client = TdClient(self, 1)  # type: ignore[arg-type]
        self.users: dict[int, dict[str, Any]] = {}
        self.chats: dict[int, dict[str, Any]] = {}
        self.history: dict[int, list[dict[str, Any]]] = {}  # chat id -> messages, oldest first
        self.order = 10_000
        self.next_id = 1 << 20
        self.next_temp = TEMP
        self.requests: list[str] = []
        self.viewed: list[tuple[int, list[int]]] = []
        self.drafts: list[int] = []

    # TdHub's side
    def _send(self, client_id: int, request: dict[str, Any]) -> None:
        extra = request.pop("@extra")
        self.requests.append(request["@type"])
        try:
            result = self.answer(request)
        except LookupError as e:
            result = {"@type": "error", "code": 404, "message": str(e)}
        asyncio.get_running_loop().call_soon(self.client._dispatch, {**result, "@extra": extra})

    def _forget(self, client_id: int) -> None:
        pass

    def update(self, event: dict[str, Any]) -> None:
        self.client._dispatch(event)

    def answer(self, r: dict[str, Any]) -> dict[str, Any]:
        ok = {"@type": "ok"}
        match r["@type"]:
            case "loadChats":
                raise LookupError("all chats are loaded")
            case "openChat" | "closeChat" | "sendChatAction" | "logOut":
                return ok
            case "setChatDraftMessage":
                self.drafts.append(r["chat_id"])
                return ok
            case "viewMessages":
                self.viewed.append((r["chat_id"], list(r["message_ids"])))
                chat = self.chats[r["chat_id"]]
                last = max(r["message_ids"])
                if last > chat["last_read_inbox_message_id"]:
                    chat["last_read_inbox_message_id"] = last
                    unread = sum(1 for m in self.history[r["chat_id"]]
                                 if m["id"] > last and not m["is_outgoing"])
                    asyncio.get_running_loop().call_soon(self.update, {
                        "@type": "updateChatReadInbox", "chat_id": r["chat_id"],
                        "last_read_inbox_message_id": last, "unread_count": unread})
                return ok
            case "parseMarkdown":
                return r["text"]
            case "getChatHistory":
                messages = self.history.get(r["chat_id"], [])
                start = r["from_message_id"] or (1 << 62)
                older = [m for m in reversed(messages) if m["id"] <= start]
                if r["from_message_id"] and older and older[0]["id"] == start:
                    pass  # TDLib includes from_message_id itself; ChatHistory drops it
                return {"@type": "messages", "total_count": len(older),
                        "messages": older[:r["limit"]]}
            case "getRepliedMessage":
                message = self.find(r["chat_id"], r["message_id"])
                target = (message.get("reply_to") or {}).get("message_id", 0)
                return self.find(r["chat_id"], target)
            case "getMessage":
                return self.find(r["chat_id"], r["message_id"])
            case "sendMessage":
                return self.send(r)
            case "resendMessages":
                return self.resend(r)
        raise LookupError(f"{r['@type']} is not in the fake")

    def find(self, chat_id: int, message_id: int) -> dict[str, Any]:
        for m in self.history.get(chat_id, []):
            if m["id"] == message_id:
                return m
        raise LookupError("no such message")

    # The account
    def user(self, user_id: int, first: str, last: str = "") -> None:
        self.users[user_id] = {"@type": "user", "id": user_id, "first_name": first,
                               "last_name": last, "type": {"@type": "userTypeRegular"}}
        self.update({"@type": "updateUser", "user": self.users[user_id]})

    def chat(self, chat_id: int, title: str, kind: str, peer: int = 0, pinned: bool = False,
             muted: bool = False) -> None:
        type_obj = {"private": {"@type": "chatTypePrivate", "user_id": peer},
                    "group": {"@type": "chatTypeBasicGroup", "basic_group_id": peer},
                    "channel": {"@type": "chatTypeSupergroup", "supergroup_id": peer,
                                "is_channel": True}}[kind]
        self.order += 10
        order = (1 << 40) + self.order if pinned else self.order  # TDLib: pinned chats on top
        chat = {"@type": "chat", "id": chat_id, "title": title, "type": type_obj,
                "positions": [{"@type": "chatPosition", "list": {"@type": "chatListMain"},
                               "order": str(order), "is_pinned": pinned}],
                "unread_count": 0, "last_read_inbox_message_id": 0,
                "last_read_outbox_message_id": 0, "last_message": None,
                "notification_settings": {"mute_for": 1 << 30 if muted else 0,
                                          "use_default_mute_for": False}}
        self.chats[chat_id] = chat
        self.history[chat_id] = []
        self.update({"@type": "updateNewChat", "chat": dict(chat)})

    def message(self, chat_id: int, sender: int, content: dict[str, Any], *,
                reply_to: int = 0, date: int = 0, quiet: bool = False) -> dict[str, Any]:
        self.next_id += 1 << 20  # TDLib's message ids grow in steps of 2^20
        m = {"@type": "message", "id": self.next_id, "chat_id": chat_id,
             "sender_id": {"@type": "messageSenderUser", "user_id": sender},
             "is_outgoing": sender == ME, "date": date or NOW, "content": content,
             "edit_date": 0}
        if reply_to:
            m["reply_to"] = {"@type": "messageReplyToMessage", "chat_id": chat_id,
                             "message_id": reply_to}
        self.history[chat_id].append(m)
        if not quiet:
            self.arrive(m)
        return m

    def arrive(self, m: dict[str, Any]) -> None:
        chat = self.chats[m["chat_id"]]
        if not m["is_outgoing"] and m["id"] > chat["last_read_inbox_message_id"]:
            chat["unread_count"] += 1
            self.update({"@type": "updateChatReadInbox", "chat_id": chat["id"],
                         "last_read_inbox_message_id": chat["last_read_inbox_message_id"],
                         "unread_count": chat["unread_count"]})
        self.update({"@type": "updateNewMessage", "message": m})
        self.last_message(chat["id"], m)

    def last_message(self, chat_id: int, m: dict[str, Any] | None, move: bool = True) -> None:
        chat = self.chats[chat_id]
        position = chat["positions"][0]
        if move and not position["is_pinned"]:
            self.order += 10
            position["order"] = str(self.order)
        self.update({"@type": "updateChatLastMessage", "chat_id": chat_id, "last_message": m,
                     "positions": [dict(position)]})

    def send(self, r: dict[str, Any]) -> dict[str, Any]:
        chat_id = r["chat_id"]
        self.next_temp += 1
        m = {"@type": "message", "id": self.next_temp, "chat_id": chat_id,
             "sender_id": {"@type": "messageSenderUser", "user_id": ME}, "is_outgoing": True,
             "date": NOW, "content": {"@type": "messageText",
                                      "text": r["input_message_content"]["text"]},
             "sending_state": {"@type": "messageSendingStatePending"}, "edit_date": 0}
        if r.get("reply_to"):
            m["reply_to"] = {"@type": "messageReplyToMessage", "chat_id": chat_id,
                             "message_id": r["reply_to"]["message_id"]}
        loop = asyncio.get_running_loop()
        loop.call_soon(self.update, {"@type": "updateNewMessage", "message": m})
        loop.call_soon(self.last_message, chat_id, m)
        # Invented: a text that starts with /fail fails the first time (protocol 3).
        failing = m["content"]["text"]["text"].startswith(FAIL) and not r.get("_resent")
        loop.call_later(0.01, self.fail if failing else self.confirm, m)
        return m

    def fail(self, temp: dict[str, Any]) -> None:
        """updateMessageSendFailed: the message stays, with a new id, failed."""
        self.next_temp += 1
        failed = dict(temp, id=self.next_temp, sending_state={
            "@type": "messageSendingStateFailed", "error": {"@type": "error", "code": 400,
                                                            "message": "INVENTED"},
            "can_retry": True, "need_another_sender": False, "need_another_reply_quote": False,
            "need_drop_reply": False, "required_paid_message_star_count": 0,
            "retry_after": 0.0})
        self.history[temp["chat_id"]].append(failed)
        self.update({"@type": "updateMessageSendFailed", "message": failed,
                     "old_message_id": temp["id"],
                     "error": {"@type": "error", "code": 400, "message": "INVENTED"}})

    def resend(self, r: dict[str, Any]) -> dict[str, Any]:
        """resendMessages: each failed message is deleted and sent again as a new one."""
        chat_id, sent = r["chat_id"], []
        for message_id in r["message_ids"]:
            old = self.find(chat_id, message_id)
            if (old.get("sending_state") or {}).get("@type") != "messageSendingStateFailed":
                raise LookupError("the message did not fail")
            self.history[chat_id].remove(old)
            self.update({"@type": "updateDeleteMessages", "chat_id": chat_id,
                         "message_ids": [message_id], "is_permanent": True,
                         "from_cache": False})
            sent.append(self.send({"chat_id": chat_id, "_resent": True,
                                   "input_message_content": {"text": old["content"]["text"]}}))
        return {"@type": "messages", "total_count": len(sent), "messages": sent}

    def confirm(self, temp: dict[str, Any]) -> None:
        self.next_id += 1 << 20
        final = {k: v for k, v in temp.items() if k != "sending_state"}
        final["id"] = self.next_id
        self.history[temp["chat_id"]].append(final)
        self.update({"@type": "updateMessageSendSucceeded", "message": final,
                     "old_message_id": temp["id"]})
        self.last_message(temp["chat_id"], final, move=False)

    def position(self, chat_id: int, order: int, pinned: bool = False) -> None:
        position = self.chats[chat_id]["positions"][0]
        position["order"], position["is_pinned"] = str(order), pinned
        self.update({"@type": "updateChatPosition", "chat_id": chat_id,
                     "position": dict(position)})


def utf16(s: str) -> int:
    return len(s.encode("utf-16-le")) // 2


def marked(t: str, part: str, kind: str) -> dict[str, Any]:
    """A text message with `part` of `t` as an entity, at its UTF-16 offset."""
    at = t.index(part)
    return text(t, [entity(utf16(t[:at]), utf16(part), kind)])


def account(td: FakeTd) -> dict[str, Any]:
    td.update({"@type": "updateOption", "name": "my_id",
               "value": {"@type": "optionValueInteger", "value": str(ME)}})
    td.user(ME, "Taras", "Melnyk")
    td.user(1, "Olena", "Kovalenko")
    td.user(2, "Дмитро", "Шевченко")
    td.user(3, "Анна", "Смирнова")
    td.user(4, "Mark", "Owens")
    td.chat(10, "Olena Kovalenko", "private", peer=1)
    td.chat(11, "Дмитро Шевченко", "private", peer=2)
    td.chat(12, "Анна Смирнова", "private", peer=3, muted=True)
    td.chat(13, "Книжковий клуб", "group", peer=50)
    td.chat(14, "Zoraya news", "channel", peer=60)
    td.chat(15, "Taras Melnyk", "private", peer=ME)  # Saved Messages
    td.chat(16, "Mark Owens", "private", peer=4, pinned=True)
    for i in range(70):  # a long history in the club, quietly: it is the past
        sender = (1, 2, 3, ME)[i % 4]
        td.message(13, sender, text(f"Повідомлення номер {i}"), date=NOW - 7000 + i * 60,
                   quiet=True)
    club = td.history[13]
    # Read up to the tenth message: more unread than a page of 40, so the window's jump to
    # the first unread loads older pages first.
    read = club[9]["id"]
    chat = td.chats[13]
    chat["last_read_inbox_message_id"] = read
    chat["unread_count"] = sum(1 for m in club if m["id"] > read and not m["is_outgoing"])
    td.update({"@type": "updateChatReadInbox", "chat_id": 13, "last_read_inbox_message_id": read,
               "unread_count": chat["unread_count"]})
    question = td.message(13, 2, marked("Хто прочитав «Тіні забутих предків»? 📚 так", "так",
                                        "textEntityTypeBold"), reply_to=club[-1]["id"])
    photo = td.message(13, 3, {"@type": "messagePhoto", "caption": {
        "@type": "formattedText", "text": "Обкладинка", "entities": []},
        "photo": {"sizes": [{"type": "x", "width": 1280, "height": 960,
                             "photo": {"id": 77, "size": 200_000}}]}})
    link = td.message(10, 1, marked("See you at 6? 🙂 The link: zoraya.example", "zoraya.example",
                             "textEntityTypeUrl"))
    td.message(11, 2, text("Привіт! Як справи?"))
    spoiler = td.message(12, 3, marked("Встреча в пятницу, секрет", "секрет", "textEntityTypeSpoiler"))
    doc = td.message(14, 60, {"@type": "messageDocument", "caption": {
        "@type": "formattedText", "text": "", "entities": []},
        "document": {"file_name": "report.pdf", "document": {"id": 90, "size": 253_952}}})
    td.message(15, ME, text("Note to self"))
    voice = td.message(16, 4, {"@type": "messageVoiceNote", "voice_note": {
        "duration": 42, "voice": {"id": 91, "size": 1000}}})
    return {"question": question["id"], "photo": photo["id"], "reply_to": club[-1]["id"],
            "last_read": read, "first_unread": club[10]["id"] if club[10]["sender_id"]["user_id"] != ME
            else club[11]["id"],
            "link": link["id"], "spoiler": spoiler["id"], "doc": doc["id"], "voice": voice["id"]}


@dataclasses.dataclass(frozen=True)
class SummaryState:
    """tgclient services/ai.py SummaryState's fields that AiController reads."""
    state: str
    scope: str = ""
    text: str = ""
    error: str = ""


class FakeAi:
    """tgclient AiService's surface that AiController uses, with an invented summary."""

    def __init__(self) -> None:
        self.State = SummaryState
        self.enabled: set[int] = set()
        self.listeners: list[Any] = []
        self.summaries: dict[tuple[int, str], Any] = {}

    def subscribe(self, listener: Any) -> Any:
        self.listeners.append(listener)
        return lambda: self.listeners.remove(listener)

    def _emit(self, kind: str, payload: Any) -> None:
        for listener in list(self.listeners):
            listener(kind, payload)

    def allowed(self, chat_id: int) -> bool:
        return chat_id != 0

    def is_enabled(self, chat_id: int) -> bool:
        return chat_id in self.enabled

    def set_enabled(self, chat_id: int, on: bool) -> bool:
        (self.enabled.add if on else self.enabled.discard)(chat_id)
        self._emit("enabled", chat_id)
        return True

    def summary(self, chat_id: int, subject: str = "") -> Any:
        return self.summaries.get((chat_id, subject)) or self.State("")

    def summarize(self, chat_id: int, scope: str, topic_id: int = 0, name: str = "",
                  last_read: int = 0) -> None:
        key = (chat_id, "")
        if chat_id not in self.enabled:
            self.summaries[key] = self.State("error", scope, error="AI is off for this chat")
            self._emit("summary", key)
            return
        self.summaries[key] = self.State("pending", scope)
        self._emit("summary", key)

        def done() -> None:
            self.summaries[key] = self.State("done", scope, "Клуб обговорює книжки. 📚")
            self._emit("summary", key)

        asyncio.get_running_loop().call_later(0.01, done)


# --- the login -------------------------------------------------------------------------------

CODE = "12345"
PASSWORD = "zoraya"


async def login(auth: Any) -> None:
    """AuthFlow's part, invented: the phone (any), the code, the password, then ready."""
    await asyncio.sleep(0)
    await auth.ask_phone()
    while await auth.ask_code({"type": {"@type": "authenticationCodeTypeSms", "length": 5}}) != CODE:
        await auth.show_error("PHONE_CODE_INVALID")
    while await auth.ask_password("the name of this system") != PASSWORD:
        await auth.show_error("PASSWORD_HASH_INVALID")
    auth.set_ready()


class Account:
    """The fake TDLib with the account in it, the stores over it, and the AI."""

    def __init__(self) -> None:
        from tgclient.store.chats import ChatStore  # noqa: PLC0415
        from tgclient.store.files import FileManager  # noqa: PLC0415
        from tgclient.store.users import UserStore  # noqa: PLC0415

        self.td = FakeTd()
        self.client = self.td.client
        files = FileManager(self.client)
        self.chats, self.users = ChatStore(self.client, files), UserStore(self.client, files)
        self.ai = FakeAi()
        self.refs = account(self.td)

    async def timer(self, every: float = 20.0) -> None:
        """A message now and then from someone, so the window sees updates (as the window's
        sample backend has)."""
        n = 0
        while True:
            await asyncio.sleep(every)
            n += 1
            self.td.message(11, 2, text(f"Повідомлення з таймера {n}"))
