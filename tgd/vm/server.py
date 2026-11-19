"""Protocol 2 (docs/tg-window.md, "The protocol"): the window's requests in, events out,
served by tgclient's view-models.

What QML did with the models' notifications, this does for the window:

- the chat list model's `rowsInserted`, `rowsRemoved`, `rowsMoved`, `dataChanged` and
  `modelReset` all mean "the window may have changed": its rows are computed again and the
  middle that differs from what was sent goes out as one `chats.splice`;
- the message list model's notifications are kept in a mirror of its ids (newest first),
  so a removal still knows what it removed: an insertion at the newest end is a
  `message.new`, a changed row whose protocol row differs from what was sent a
  `message.update`, a removal a `message.delete`. A sent message's temporary id becoming
  its final one (TDLib's updateMessageSendSucceeded: ChatHistory removes the old row and
  inserts the new) is one `message.update` with `old_id`;
- the auth controller's `changed` is an `auth.state`, the AI controller's `summaryChanged`
  an `ai.summary`.

`emit` takes one event (a dict with "ev"); `handle` takes one request (a dict with "op").
Ids are strings on the wire. Nothing here logs message text, a name, a code or a password.
"""

from __future__ import annotations

import asyncio
import logging
import time
from collections.abc import Callable
from typing import Any

from tgclient.store.chats import MAIN
from tgclient.store.format import message_preview

from . import rows
from .ai import AiController
from .auth import AuthController
from .chat_list import ChatListModel
from .composer import ComposerModel
from .messages import MessageListModel

log = logging.getLogger("tgd.vm")

Emit = Callable[[dict[str, Any]], None]

PROTOCOL = 2
MAX_WINDOW = 200  # rows of the chat list one window may ask for
MAX_PAGE = 200  # messages of one reset or page
AUTH_STATES = {"loading": "starting", "phone": "wait_phone", "code": "wait_code",
               "password": "wait_password", "email": "wait_email", "emailCode": "wait_email_code",
               "qr": "wait_other_device", "ready": "ready", "failed": "failed"}
AUTH_OPS = {"auth.phone": "phone", "auth.code": "code", "auth.password": "password"}
SUMMARY_STATES = {"pending": "running", "done": "done", "error": "failed"}


class RequestError(Exception):
    """A request the server refuses; the message names what was wrong, never a value."""


def _id(value: Any, what: str) -> int:
    try:
        return int(str(value))
    except (TypeError, ValueError):
        raise RequestError(f"{what} is not an id") from None


def _count(value: Any, default: int, limit: int) -> int:
    try:
        return max(1, min(int(value if value is not None else default), limit))
    except (TypeError, ValueError):
        raise RequestError("count is not a number") from None


class _OpenChat:
    def __init__(self, chat_id: int, count: int) -> None:
        self.chat_id = chat_id
        self.count = count
        self.ready = False  # messages.reset sent: from now on the changes
        self.sent: dict[int, dict[str, Any]] = {}  # message id -> the row the window has
        self.ids: list[int] = []  # the model's rows (newest first), mirrored


class WindowServer:
    def __init__(self, *, client: Any, chats: Any, users: Any, emit: Emit,
                 auth: AuthController | None = None, ai_service: Any = None,
                 synthetic: bool = False, clock: Callable[[], float] = time.time) -> None:
        self._client = client
        self._chats = chats
        self._users = users
        self._emit = emit
        self._synthetic = synthetic
        self._clock = clock
        self._renamed: dict[int, int] = {}  # final message id -> the temporary id it replaces
        # Before any ChatHistory subscribes: this sees a send's result first.
        client.on("updateMessageSendSucceeded", self._on_send_result)
        client.on("updateMessageSendFailed", self._on_send_result)

        self.chat_list = ChatListModel(chats, users)
        for signal in (self.chat_list.rowsInserted, self.chat_list.rowsRemoved,
                       self.chat_list.rowsMoved, self.chat_list.dataChanged,
                       self.chat_list.modelReset):
            signal.connect(self._chats_changed)
        chats.subscribe(self._on_store)  # a filtered window follows the store itself
        users.subscribe(lambda _user: self._chats_changed())  # names in the last line
        self._window: tuple[int, int, str] | None = None
        self._window_rows: list[dict[str, Any]] = []
        self._window_total = 0
        self._recompute_scheduled = False

        self.messages = MessageListModel(client, chats, users)
        self.messages.rowsInserted.connect(self._messages_inserted)
        self.messages.rowsRemoved.connect(self._messages_removed)
        self.messages.dataChanged.connect(self._messages_changed)
        self.messages.modelReset.connect(self._messages_reset)
        self.messages.loadingChanged.connect(self._loading_changed)
        self.composer = ComposerModel(client, chats, self.messages)
        self._open: _OpenChat | None = None
        self._loaded: asyncio.Event = asyncio.Event()

        self.auth = auth
        if auth is not None:
            auth.changed.connect(self._auth_changed)
        self.ai = AiController(ai_service, chats) if ai_service is not None else None
        # (chat id, text sent so far, "running" sent): the summary the window waits for
        self._summary: tuple[int, str, bool] | None = None
        if self.ai is not None:
            self.ai.summaryChanged.connect(self._summary_changed)
            self.ai.chatChanged.connect(self._send_settings)

    # --- start -----------------------------------------------------------------------------

    def start(self) -> None:
        """The hello and the chat list's first load. Call on the running loop."""
        now = self._clock()
        offset = int(time.localtime(now).tm_gmtoff or 0)
        self._emit({"ev": "hello", "protocol": PROTOCOL, "now": int(now), "utc_offset": offset,
                    "synthetic": self._synthetic})
        if self.auth is not None:
            self._auth_changed()
        self.chat_list.setList(MAIN)
        self._send_settings()

    # --- requests --------------------------------------------------------------------------

    async def handle(self, request: dict[str, Any]) -> None:
        """One request. Refusals raise RequestError (the caller decides how to tell)."""
        op = request.get("op")
        if op in AUTH_OPS:
            self._answer_auth(AUTH_OPS[op], str(request.get("value", "")))
        elif op == "auth.logout":
            await self._client.send({"@type": "logOut"})
        elif op == "chats.window":
            first = max(0, int(request.get("first") or 0))
            count = _count(request.get("count"), 20, MAX_WINDOW)
            self._set_window(first, count, str(request.get("query") or ""))
        elif op == "chat.open":
            await self._open_chat(_id(request.get("chat"), "chat"),
                                  _count(request.get("count"), 30, MAX_PAGE))
        elif op == "chat.close":
            self._close_chat()
        elif op == "messages.older":
            await self._older(_id(request.get("chat"), "chat"), _id(request.get("before"), "before"),
                              _count(request.get("count"), 30, MAX_PAGE))
        elif op == "messages.send":
            self._send(_id(request.get("chat"), "chat"), str(request.get("text") or ""),
                       request.get("reply_to"))
        elif op == "ai.summarize":
            self._summarize(_id(request.get("chat"), "chat"), str(request.get("scope") or ""))
        elif op == "ai.cancel":
            self._summary = None  # tgclient's job runs on; the window hears no more of it
        elif op == "settings.set":
            self._set_setting(str(request.get("key") or ""), bool(request.get("on")))
        else:
            raise RequestError(f"unknown op {op!r}")

    # --- the chat list ---------------------------------------------------------------------

    def _set_window(self, first: int, count: int, query: str) -> None:
        self._window = (first, count, query)
        total, window = self._compute_window()
        self._window_rows, self._window_total = window, total
        self._emit({"ev": "chats.rows", "first": first, "total": total, "rows": window})
        if first + count > self.chat_list.rowCount() and not query:
            self.chat_list.loadMore()

    def _compute_window(self) -> tuple[int, list[dict[str, Any]]]:
        assert self._window is not None
        first, count, query = self._window
        if not query.strip():
            total = self.chat_list.rowCount()
            return total, [rows.chat_row(self.chat_list, r)
                           for r in range(first, min(first + count, total))]
        matches = self._search(query.strip())
        return len(matches), [self._chat_out(c) for c in matches[first:first + count]]

    def _search(self, query: str) -> list[Any]:
        """tgclient SearchModel._chat_rows' match and order (models/search.py), all of them:
        the window's list is the filtered list, not the search panel's first five."""
        needle = query.lower()
        matches = [c for c in self._chats.chats.values()
                   if needle in c.title.lower() and c.positions]
        matches.sort(key=lambda c: (not c.title.lower().startswith(needle),
                                    -max(p.order for p in c.positions.values())))
        return matches

    def _chat_out(self, chat: Any) -> dict[str, Any]:
        position = chat.positions.get(MAIN)
        return rows.chat_out(chat, chat.title, message_preview(chat, self._users),
                             chat.unread_count, bool(position and position.is_pinned),
                             self._chats.is_muted(chat), self._users.my_id)

    def _on_store(self, kind: str, payload: Any) -> None:
        if self._window is not None and self._window[2].strip():
            self._chats_changed()

    def _chats_changed(self, *_: Any) -> None:
        if self._window is None or self._recompute_scheduled:
            return
        self._recompute_scheduled = True
        asyncio.get_running_loop().call_soon(self._send_splice)

    def _send_splice(self) -> None:
        """The window's rows again; what differs from the last sent goes out as one splice."""
        self._recompute_scheduled = False
        if self._window is None:
            return
        total, new = self._compute_window()
        old = self._window_rows
        head = 0
        while head < len(old) and head < len(new) and old[head] == new[head]:
            head += 1
        tail = 0
        while (tail < len(old) - head and tail < len(new) - head
               and old[len(old) - 1 - tail] == new[len(new) - 1 - tail]):
            tail += 1
        if head == len(old) == len(new) and total == self._window_total:
            return
        self._window_rows, self._window_total = new, total
        self._emit({"ev": "chats.splice", "at": self._window[0] + head,
                    "remove": len(old) - head - tail, "rows": new[head:len(new) - tail],
                    "total": total})

    # --- the open chat ---------------------------------------------------------------------

    async def _open_chat(self, chat_id: int, count: int) -> None:
        chat = self._chats.chats.get(chat_id)
        if chat is None:
            raise RequestError("no such chat")
        self._close_chat()
        state = self._open = _OpenChat(chat_id, count)
        self._loaded.clear()
        self.messages.open(chat_id)
        if self.ai is not None:
            self.ai.chatId = chat_id
        await self._loaded.wait()
        if self._open is not state:
            return  # another chat was opened meanwhile
        history = self.messages.history
        assert history is not None
        state.ids = [m["id"] for m in history.messages]
        newest = range(min(count, len(state.ids)))
        out = [self._row(state, r) for r in reversed(newest)]
        state.ready = True
        self._emit({"ev": "messages.reset", "chat": str(chat_id), "title": chat.title,
                    "subtitle": "", "kind": rows.chat_kind(chat, self._users.my_id),
                    "rows": out, "more": len(state.ids) > count or not history.reached_start})
        if out:
            self.messages.markViewed(0, len(out) - 1)  # the chat is read: what is shown

    def _close_chat(self) -> None:
        if self._open is None:
            return
        self._open = None
        self.messages.close()
        if self.ai is not None:
            self.ai.chatId = 0

    def _row(self, state: _OpenChat, row: int) -> dict[str, Any]:
        out = rows.message_row(self.messages, row)
        state.sent[state.ids[row]] = out
        return out

    def _loading_changed(self) -> None:
        if not self.messages.loading:
            self._loaded.set()

    async def _older(self, chat_id: int, before: int, count: int) -> None:
        state = self._open
        if state is None or state.chat_id != chat_id or not state.ready:
            raise RequestError("that chat is not open")
        history = self.messages.history
        assert history is not None
        while True:
            older = [r for r, i in enumerate(state.ids) if i < before]
            if len(older) >= count or history.reached_start:
                break
            self._loaded.clear()
            if not history.loading:
                self.messages.loadOlder()  # its task sets loading, then clears it: _loaded
            await self._loaded.wait()
            if self._open is not state:
                return
        page = older[:count]
        out = [self._row(state, r) for r in reversed(page)]
        more = len(older) > count or not history.reached_start
        self._emit({"ev": "messages.page", "chat": str(chat_id), "rows": out, "more": more})

    def _send(self, chat_id: int, text: str, reply_to: Any) -> None:
        state = self._open
        if state is None or state.chat_id != chat_id:
            raise RequestError("that chat is not open")
        if not text.strip():
            raise RequestError("nothing to send")
        self.composer.sent()
        self.messages.send(text, _id(reply_to, "reply_to") if reply_to else 0)

    def _on_send_result(self, event: dict[str, Any]) -> None:
        self._renamed[int(event["message"]["id"])] = int(event["old_message_id"])

    def _messages_reset(self) -> None:
        state = self._open
        history = self.messages.history
        if state is not None:
            state.ids = [m["id"] for m in history.messages] if history else []

    def _messages_inserted(self, _parent: Any, first: int, last: int) -> None:
        state = self._open
        history = self.messages.history
        if state is None or history is None:
            return
        new_ids = [m["id"] for m in history.messages[first:last + 1]]
        state.ids[first:first] = new_ids
        if not state.ready:
            return
        oldest_known = min(state.sent) if state.sent else 0
        incoming: list[int] = []
        for row in range(last, first - 1, -1):  # oldest first
            message_id = state.ids[row]
            old_id = self._renamed.pop(message_id, 0)
            if old_id and old_id in state.sent:
                del state.sent[old_id]
                self._emit({"ev": "message.update", "chat": str(state.chat_id),
                            "old_id": str(old_id), "row": self._row(state, row)})
            elif row == 0 or (state.sent and message_id > max(state.sent)):
                self._emit({"ev": "message.new", "chat": str(state.chat_id),
                            "row": self._row(state, row)})
                if not history.messages[row].get("is_outgoing"):
                    incoming.append(row)
            elif message_id > oldest_known:
                # Between two the window has: protocol 2 has no insertion in the middle.
                log.info("a message arrived between two shown ones; not sent to the window")
        if incoming:
            self.messages.markViewed(min(incoming), max(incoming))

    def _messages_removed(self, _parent: Any, first: int, last: int) -> None:
        state = self._open
        if state is None:
            return
        gone = state.ids[first:last + 1]
        del state.ids[first:last + 1]
        if not state.ready:
            return
        renamed = set(self._renamed.values())
        ids = [i for i in gone if i in state.sent and i not in renamed]
        for i in ids:
            del state.sent[i]
        if ids:
            self._emit({"ev": "message.delete", "chat": str(state.chat_id),
                        "ids": [str(i) for i in ids]})

    def _messages_changed(self, top: Any, bottom: Any, _roles: Any = None) -> None:
        state = self._open
        if state is None or not state.ready:
            return
        for row in range(top.row(), bottom.row() + 1):
            if not 0 <= row < len(state.ids):
                continue
            message_id = state.ids[row]
            before = state.sent.get(message_id)
            if before is None:
                continue
            now = rows.message_row(self.messages, row)
            if now != before:
                state.sent[message_id] = now
                self._emit({"ev": "message.update", "chat": str(state.chat_id),
                            "old_id": str(message_id), "row": now})

    # --- authorization ---------------------------------------------------------------------

    def _auth_changed(self) -> None:
        auth = self.auth
        assert auth is not None
        self._emit({"ev": "auth.state", "state": AUTH_STATES.get(auth.step, auth.step),
                    "hint": auth.hint, "error": auth.error})

    def _answer_auth(self, step: str, value: str) -> None:
        auth = self.auth
        if auth is None:
            raise RequestError("no login here")
        if auth.step != step:
            raise RequestError(f"the {step} is not asked now")
        auth.submit(value)

    # --- AI and settings -------------------------------------------------------------------

    def _summarize(self, chat_id: int, scope: str) -> None:
        if self.ai is None:
            self._emit({"ev": "ai.summary", "chat": str(chat_id), "state": "failed",
                        "text": "AI is not set up in tgd (no OpenRouter key)."})
            return
        if scope not in ("unread", "day", "week"):
            raise RequestError("scope is unread, day or week")
        self.ai.chatId = chat_id
        self._summary = (chat_id, "", False)
        self.ai.summarize(scope)
        self._summary_changed()  # tgclient says nothing when a summary is running already

    def _summary_changed(self) -> None:
        ai = self.ai
        if ai is None or self._summary is None or self._summary[0] != ai.chatId:
            return
        chat_id, shown, running = self._summary
        state = SUMMARY_STATES.get(ai.summaryState)
        if state is None or (state == "running" and running):
            return
        if state == "failed":
            self._summary = None
            self._emit({"ev": "ai.summary", "chat": str(chat_id), "state": state,
                        "text": ai.summaryError})
            return
        text = ai.summaryText if state == "done" else ""
        # tgclient's summary arrives whole; the protocol adds text: send what is new.
        added = text[len(shown):] if text.startswith(shown) else text
        self._summary = None if state == "done" else (chat_id, shown + added, True)
        self._emit({"ev": "ai.summary", "chat": str(chat_id), "state": state, "text": added})

    def _settings(self) -> list[dict[str, Any]]:
        if self.ai is None or not self.ai.chatId or not self.ai.available:
            return []
        return [{"key": "ai.chat", "label": "AI summaries of this chat",
                 "on": bool(self.ai.enabled)}]

    def _send_settings(self) -> None:
        self._emit({"ev": "settings", "items": self._settings()})

    def _set_setting(self, key: str, on: bool) -> None:
        if key != "ai.chat" or self.ai is None or not self.ai.chatId:
            raise RequestError(f"no setting {key!r} now")
        self.ai.setEnabled(on)
        self._send_settings()
