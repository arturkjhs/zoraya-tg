"""tgclient's MessageListModel (models/messages.py), the part the window's protocol serves.

What is kept, with tgclient's method bodies unchanged: `_Adapter` (history callbacks bound to
the current history), `open`, `close`, `_start`, `_stop`, `loadOlder`, `loadNewer`,
`jumpToLatest`, `send`, `sendMessage`, `markViewed`, `_load_initial`, `_send`, `_formatted`,
`_run_load`, `_sync_latest`, `_row_changed`, `_on_chat_store`, the forum and thread
queries, `_status`, `_sender_name`, `_forwarded_from`, `_short_text`, `_album_range`,
`_refresh_albums`. What changes: the imports (`.qt` for PySide6), `__init__` (no font, no
palette, no files, presence, forums or custom emoji yet), and `data()`, which answers the
roles a protocol row needs (tgclient's `data()` also prefetches older pages; here the
window asks for them with `messages.older`).

What waits for later (docs/tg-window.md, "The protocol"): selection and its actions,
search in a chat, pinned bar actions, scheduled messages, bot keyboards, polls, reactions,
editing, deleting, forwarding, links, media files, translation and transcripts.
"""

from __future__ import annotations

import asyncio
import logging
from enum import IntEnum, auto
from typing import Any

from tgclient.services.summary import sender_key
from tgclient.store.chats import ChatStore
from tgclient.store.format import content_preview, is_service, message_body
from tgclient.store.forums import Topic, topic_obj
from tgclient.store.history import DELETED_REPLY, ChatHistory, Commit, Message
from tgclient.store.media import album_id
from tgclient.store.pinned import PinnedMessages
from tgclient.store.richtext import mention_names
from tgclient.store.users import UserStore
from tgclient.td.client import TdClient, TdError

from .qt import (
    Property,
    QAbstractListModel,
    QModelIndex,
    Qt,
    Signal,
    Slot,
)

log = logging.getLogger(__name__)

TOPIC_LIST = -1  # ChatHistory.topic_id while a forum shows its list of topics: loads nothing


class Role(IntEnum):
    MessageId = Qt.ItemDataRole.UserRole + 1
    IsOutgoing = auto()
    IsService = auto()
    ServiceText = auto()
    SenderName = auto()
    SenderKey = auto()
    Time = auto()
    Edited = auto()
    Status = auto()
    ReplyToId = auto()
    ReplySender = auto()
    ReplyText = auto()
    ForwardedFrom = auto()
    Body = auto()  # tgclient: Html. Here the formattedText; text.py makes text and spans of it
    Content = auto()  # the TDLib content, for the row's kind and detail (rows.py)


class _Adapter:
    """Binds history callbacks to the model only while that history is the current one, so a
    late page from a previously opened chat can't corrupt the model."""

    def __init__(self, model: MessageListModel, history_ref: list[ChatHistory]) -> None:
        self._model = model
        self._ref = history_ref

    def _current(self) -> bool:
        return bool(self._ref) and self._model._history is self._ref[0]

    def history_insert(self, row: int, count: int, commit: Commit) -> None:
        if not self._current():
            commit()
            return
        self._model.beginInsertRows(QModelIndex(), row, row + count - 1)
        commit()
        self._model.endInsertRows()
        self._model._refresh_albums(row - 1, row + count)

    def history_remove(self, row: int, count: int, commit: Commit) -> None:
        if not self._current():
            commit()
            return
        self._model.beginRemoveRows(QModelIndex(), row, row + count - 1)
        message_id = self._ref[0].messages[row]["id"]
        commit()
        self._model._html_cache.pop(message_id, None)
        self._model.endRemoveRows()
        self._model._refresh_albums(row - 1, row)
        if message_id in self._model._selected:
            self._model._selected.discard(message_id)
            self._model.selectionChanged.emit()

    def history_changed(self, row: int) -> None:
        if self._current():
            self._model._row_changed(row)

    def history_reset(self, commit: Commit) -> None:
        if not self._current():
            commit()
            return
        model = self._model
        model.beginResetModel()
        commit()
        model._html_cache = {}
        model._media_cache = {}
        model.endResetModel()
        model._sync_latest()


class MessageListModel(QAbstractListModel):
    chatChanged = Signal()
    loadingChanged = Signal()
    unreadReady = Signal("QVariant")  # first unread message id, once loaded after opening
    statusChanged = Signal()
    latestChanged = Signal()
    countersChanged = Signal()
    selectionChanged = Signal()
    pinnedChanged = Signal()
    chatSearchChanged = Signal()
    replyKeyboardChanged = Signal()
    viewCheckNeeded = Signal()  # e.g. a reaction to a message that may be on screen

    def __init__(
        self, client: TdClient, chats: ChatStore, users: UserStore, parent: Any = None,
    ) -> None:
        super().__init__(parent)
        self._client = client
        self._chats = chats
        self._users = users
        self._history: ChatHistory | None = None
        self._loading = False
        self._viewed: set[int] = set()
        self._html_cache: dict[int, str] = {}
        self._revealed: set[int] = set()
        self._media_cache: dict[int, Any] = {}
        self._file_messages: dict[int, set[int]] = {}
        self._unread_after = 0  # last read incoming message id when the chat was opened
        self._first_unread = 0  # message showing the "Unread messages" separator
        self._at_latest = True
        self._pinned: PinnedMessages | None = None
        self._thread_return: tuple[int, int] | None = None
        self._search_query = ""
        self._search_ids: list[int] = []
        self._search_next = 0
        self._search_index = -1
        self._search_sender = ""
        self._search_sender_name = ""
        self._selected: set[int] = set()
        self._select_anchor = 0
        self._reactors: dict[tuple[int, str], list[str]] = {}
        self._reactors_asked: set[tuple[int, str]] = set()
        self._forums: Any = None  # tgclient's ForumStore: forums come later
        chats.subscribe(self._on_chat_store)

    # --- properties -------------------------------------------------------------------------

    @Property("QVariant", notify=chatChanged)
    def chatId(self) -> int:  # noqa: N802
        return self._history.chat_id if self._history else 0

    @Property(bool, notify=loadingChanged)
    def loading(self) -> bool:
        return self._loading

    @Property(bool, notify=latestChanged)
    def atLatest(self) -> bool:  # noqa: N802
        return self._at_latest

    @property
    def history(self) -> ChatHistory | None:
        """tgd: the protocol server reads rows and paging state from it."""
        return self._history

    # --- QML API ----------------------------------------------------------------------------

    @Slot("QVariant")
    def open(self, chat_id: Any) -> None:
        chat_id = int(chat_id or 0)
        if self._history and self._history.chat_id == chat_id:
            return
        self.close()
        if not chat_id:
            return
        # A forum opens on its list of topics: no history until one is picked.
        self._start(chat_id, TOPIC_LIST if self._is_forum_chat(chat_id) else 0)
        self._spawn(self._client.send({"@type": "openChat", "chat_id": chat_id}))
        if self._forums is not None and self.topicsMode:
            self._spawn(self._forums.load(chat_id))

    def _start(self, chat_id: int, topic_id: int, thread_id: int = 0,
               thread_unread_after: int = 0) -> None:
        ref: list[ChatHistory] = []
        history = ChatHistory(self._client, chat_id, _Adapter(self, ref),
                              last_message_id=(lambda: 0) if thread_id
                              else lambda: self._last_message_id(chat_id, topic_id),
                              topic_id=topic_id, thread_id=thread_id)
        history.on_unread_reaction = self.viewCheckNeeded.emit
        ref.append(history)
        self.beginResetModel()
        self._history = history
        self._viewed = set()
        self._revealed = set()
        self._html_cache = {}
        self._media_cache = {}
        self._file_messages = {}
        chat = self._chats.chats.get(chat_id)
        topic = self._topic()
        if topic is not None:
            self._unread_after = (topic.last_read_inbox_message_id
                                  if topic.unread_count > 0 else 0)
        elif thread_id:
            self._unread_after = thread_unread_after
        else:
            self._unread_after = (chat.last_read_inbox_message_id
                                  if chat and chat.unread_count > 0
                                  and topic_id != TOPIC_LIST else 0)
        self._first_unread = 0
        self._selected = set()
        self._select_anchor = 0
        self._search_query, self._search_ids, self._search_next = "", [], 0
        self._search_index = -1
        self._search_sender = self._search_sender_name = ""
        self.endResetModel()
        self.chatSearchChanged.emit()
        self.chatChanged.emit()
        self.statusChanged.emit()
        self.countersChanged.emit()
        self.selectionChanged.emit()
        self.replyKeyboardChanged.emit()
        self._sync_latest()
        if topic_id == TOPIC_LIST:
            return
        self._spawn(self._run_load(history, self._load_initial(history)))
        if thread_id:
            return  # comments have no pinned bar
        self._pinned = PinnedMessages(self._client, chat_id, self.pinnedChanged.emit,
                                      topic_obj(topic_id))
        self.pinnedChanged.emit()
        self._spawn(self._pinned.load())

    def _stop(self) -> None:
        """Drop the current history (chat or topic) without leaving the chat."""
        if self._history is not None:
            self._history.dispose()
        if self._pinned is not None:
            self._pinned.dispose()
            self._pinned = None
            self.pinnedChanged.emit()

    @Slot()
    def close(self) -> None:
        if self._history is None:
            return
        history = self._history
        self._thread_return = None
        self._stop()
        self.beginResetModel()
        self._history = None
        self._html_cache = {}
        self._media_cache = {}
        self._file_messages = {}
        self.endResetModel()
        self.chatChanged.emit()
        self._spawn(self._client.send({"@type": "closeChat", "chat_id": history.chat_id}))

    @Property(bool, notify=chatChanged)
    def threadMode(self) -> bool:  # noqa: N802
        """The comments of a channel post are shown (its discussion group's thread)."""
        return self._history is not None and self._history.thread_id != 0

    @Property(bool, notify=chatChanged)
    def topicsMode(self) -> bool:  # noqa: N802
        """A forum's list of topics is shown instead of messages."""
        return self._history is not None and self._history.topic_id == TOPIC_LIST

    @Property("QVariant", notify=chatChanged)
    def topicId(self) -> int:  # noqa: N802
        history = self._history
        return history.topic_id if history and history.topic_id != TOPIC_LIST else 0

    def current_draft(self) -> dict[str, Any] | None:
        """TDLib draftMessage of the open chat, or of the open forum topic."""
        topic = self._topic()
        if topic is not None:
            return topic.draft
        chat = self._chat()
        return (chat.draft if chat is not None and not self.topicsMode and not self.threadMode
                else None)

    def topic_obj(self) -> dict[str, Any] | None:
        """MessageTopic of the open topic (or comment thread) for requests: sending, drafts,
        typing."""
        history = self._history
        if history is not None and history.thread_id:
            return {"@type": "messageTopicThread", "message_thread_id": history.thread_id}
        return topic_obj(self.topicId)

    def _topic(self) -> Topic | None:
        history = self._history
        if history is None or self._forums is None or history.topic_id in (0, TOPIC_LIST):
            return None
        return self._forums.get(history.chat_id, history.topic_id)

    def _is_forum_chat(self, chat_id: int) -> bool:
        chat = self._chats.chats.get(chat_id)
        return (self._forums is not None and chat is not None and chat.type == "supergroup"
                and self._forums.is_forum(chat.peer_id))

    @Slot()
    def loadOlder(self) -> None:  # noqa: N802
        history = self._history
        if history and not history.loading and not history.reached_start:
            self._spawn(self._run_load(history, history.load_older()))

    @Slot()
    def loadNewer(self) -> None:  # noqa: N802
        history = self._history
        if history and not history.loading and not history.reached_end:
            self._spawn(self._run_load(history, self._load_newer(history)))

    async def _load_newer(self, history: ChatHistory) -> None:
        await history.load_newer()
        if history is self._history:
            self._sync_latest()

    @Slot()
    def jumpToLatest(self) -> None:  # noqa: N802
        """"Down" when the window stops short of the newest message: reload the newest page."""
        history = self._history
        if history is not None and not history.reached_end:
            self._spawn(self._run_load(history, self._load_latest(history)))

    async def _load_latest(self, history: ChatHistory) -> None:
        await history.load_latest()
        if history is self._history:
            self._sync_latest()

    @Slot(str, "QVariant")
    def send(self, text: str, reply_to: Any = 0) -> None:
        self.sendMessage(text, reply_to, {})

    @Slot(str, "QVariant", "QVariantMap")
    def sendMessage(self, text: str, reply_to: Any, options: dict[str, Any]) -> None:  # noqa: N802
        """Text from the composer; `options` from ComposerModel.sendOptions()."""
        history = self._history
        if history is None or not text.strip():
            return
        self._clear_unread_separator()
        self._spawn(self._send(history.chat_id, text, int(reply_to or 0), dict(options or {})))

    @Slot(int, int)
    def markViewed(self, first: int, last: int) -> None:  # noqa: N802
        history = self._history
        if history is None or first < 0 or last < 0:
            return
        lo, hi = sorted((first, last))
        shown = history.messages[lo : hi + 1]
        # Incoming messages are read once; own messages with new reactions are viewed until
        # TDLib clears their unread_reactions (that is what reads the reactions).
        incoming = [m["id"] for m in shown
                    if m["id"] not in self._viewed and not m.get("is_outgoing")]
        reacted = [m["id"] for m in shown
                   if m.get("is_outgoing") and m.get("unread_reactions")]
        if not incoming and not reacted:
            return
        self._viewed.update(incoming)
        ids = incoming + [i for i in reacted if i not in incoming]
        self._spawn(self._client.send({
            "@type": "viewMessages", "chat_id": history.chat_id, "message_ids": ids,
            "source": None, "force_read": True,
        }))

    # --- QAbstractListModel -----------------------------------------------------------------

    def rowCount(self, parent: Any = None) -> int:  # noqa: N802
        if (parent is not None and parent.isValid()) or self._history is None:
            return 0
        return len(self._history.messages)

    def data(self, index: Any, role: int = Qt.ItemDataRole.DisplayRole) -> Any:
        history = self._history
        if history is None or not index.isValid():
            return None
        row = index.row()
        if not 0 <= row < len(history.messages):
            return None
        message = history.messages[row]
        content = message.get("content", {})

        match role:
            case Role.MessageId:
                return message["id"]
            case Role.IsOutgoing:
                return bool(message.get("is_outgoing"))
            case Role.IsService:
                return is_service(content)
            case Role.ServiceText:
                if not is_service(content):
                    return ""
                return f"{self._sender_name(message)} {content_preview(content)}".strip()
            case Role.SenderName:
                return self._sender_name(message)
            case Role.SenderKey:
                return sender_key(message)
            case Role.Time:
                return int(message.get("date", 0))  # tgclient: clock(); the window writes it
            case Role.Edited:
                return bool(message.get("edit_date"))
            case Role.Status:
                return self._status(message)
            case Role.ReplyToId:
                return history.reply_target_id(message)
            case Role.ReplySender:
                reply = history.reply_message(message)
                if reply is None:
                    return ""
                if reply is DELETED_REPLY:
                    return "Deleted message"
                return self._sender_name(reply)
            case Role.ReplyText:
                reply = history.reply_message(message)
                if reply is None or reply is DELETED_REPLY:
                    return ""
                return self._short_text(reply)
            case Role.ForwardedFrom:
                return self._forwarded_from(message)
            case Role.Body:
                return message_body(content)
            case Role.Content:
                return content
        return None

    # --- internals --------------------------------------------------------------------------

    def _chat(self):  # noqa: ANN202
        return self._chats.chats.get(self._history.chat_id) if self._history else None

    def _sender_name(self, message: Message) -> str:
        sender = message.get("sender_id", {})
        if sender.get("@type") == "messageSenderUser":
            user = self._users.users.get(sender.get("user_id", 0))
            return user.full_name if user else ""
        if sender.get("@type") == "messageSenderChat":
            chat = self._chats.chats.get(sender.get("chat_id", 0))
            return chat.title if chat else ""
        return ""

    def _forwarded_from(self, message: Message) -> str:
        origin = (message.get("forward_info") or {}).get("origin") or {}
        match origin.get("@type"):
            case "messageOriginUser" | "messageForwardOriginUser":
                user = self._users.users.get(origin.get("sender_user_id", 0))
                return user.full_name if user else "Unknown"
            case "messageOriginHiddenUser" | "messageForwardOriginHiddenUser":
                return origin.get("sender_name", "")
            case "messageOriginChat" | "messageOriginChannel" | "messageForwardOriginChat" | \
                    "messageForwardOriginChannel":
                chat = self._chats.chats.get(origin.get("sender_chat_id") or origin.get("chat_id")
                                             or 0)
                title = chat.title if chat else "Unknown"
                signature = origin.get("author_signature", "")
                return f"{title} ({signature})" if signature else title
        return ""

    def _album_range(self, row: int) -> tuple[int, int] | None:
        """(newest row, oldest row) of the loaded album containing `row`, if 2+ are loaded."""
        messages = self._history.messages if self._history else []
        if not 0 <= row < len(messages):
            return None
        group = album_id(messages[row])
        if not group:
            return None
        low = high = row
        while low > 0 and album_id(messages[low - 1]) == group:
            low -= 1
        while high + 1 < len(messages) and album_id(messages[high + 1]) == group:
            high += 1
        return (low, high) if high > low else None

    def _refresh_albums(self, first: int, last: int) -> None:
        """An album gained or lost a member: all its rows change (hidden/shown, grid)."""
        history = self._history
        if history is None:
            return
        done: set[tuple[int, int]] = set()
        for row in range(max(0, first), min(len(history.messages), last + 1)):
            album = self._album_range(row)
            if album is None or album in done:
                continue
            done.add(album)
            for member in history.messages[album[0]:album[1] + 1]:
                self._html_cache.pop(member["id"], None)
            self.dataChanged.emit(self.index(album[0]), self.index(album[1]))
            if album[1] + 1 < len(history.messages):  # the row above may regain its name
                self.dataChanged.emit(self.index(album[1] + 1), self.index(album[1] + 1))

    def _clear_unread_separator(self) -> None:
        """Sending something: the separator goes and the view returns to the newest message."""
        if self._first_unread and self._history:
            row = self._history.row_of(self._first_unread)
            self._first_unread = 0
            self._row_changed(row)
        self.jumpToLatest()

    def _last_message_id(self, chat_id: int, topic_id: int = 0) -> int:
        if topic_id and self._forums is not None:
            topic = self._forums.get(chat_id, topic_id)
            return int(((topic.last_message if topic else None) or {}).get("id") or 0)
        chat = self._chats.chats.get(chat_id)
        return int(((chat.last_message if chat else None) or {}).get("id") or 0)

    def _topic_for(self, chat_id: int) -> dict[str, Any] | None:
        """The open topic, if a message goes to the chat that is open."""
        history = self._history
        return self.topic_obj() if history is not None and history.chat_id == chat_id else None

    def _sync_latest(self) -> None:
        latest = self._history.reached_end if self._history else True
        if latest != self._at_latest:
            self._at_latest = latest
            self.latestChanged.emit()

    def _short_text(self, message: Message) -> str:
        return " ".join(content_preview(message.get("content", {})).split())[:200]

    def _status(self, message: Message) -> str:
        if not message.get("is_outgoing"):
            return ""
        state = (message.get("sending_state") or {}).get("@type", "")
        if state == "messageSendingStatePending":
            return "pending"
        if state == "messageSendingStateFailed":
            return "failed"
        chat = self._chat()
        if chat and message["id"] <= chat.last_read_outbox_message_id:
            return "read"
        return "sent"

    def _row_changed(self, row: int) -> None:
        if self._history and 0 <= row < len(self._history.messages):
            message_id = self._history.messages[row]["id"]
            for stale in [k for k in self._reactors_asked if k[0] == message_id]:
                self._reactors_asked.discard(stale)  # reactions may have changed
                self._reactors.pop(stale, None)
            self._html_cache.pop(message_id, None)
            self._media_cache.pop(message_id, None)
            index = self.index(row)
            self.dataChanged.emit(index, index)
            album = self._album_range(row)
            if album and row != album[0]:  # the grid on the newest member shows this one
                self._row_changed_plain(album[0])

    def _row_changed_plain(self, row: int) -> None:
        if self._history and 0 <= row < len(self._history.messages):
            self._html_cache.pop(self._history.messages[row]["id"], None)
            index = self.index(row)
            self.dataChanged.emit(index, index)

    def _on_chat_store(self, kind: str, payload: Any) -> None:
        if kind == "chat" and self._history and payload == self._history.chat_id:
            self.chatChanged.emit()  # title etc.
            self.countersChanged.emit()
            self.replyKeyboardChanged.emit()
            if self.rowCount() > 0:  # read receipts
                self.dataChanged.emit(
                    self.index(0), self.index(self.rowCount() - 1), [Role.Status]
                )

    async def _load_initial(self, history: ChatHistory) -> None:
        """The newest page, or (with unread messages) the history around the last read one,
        however far back: newer pages load as the user scrolls down."""
        boundary = self._unread_after
        if not boundary:
            await history.load_initial()
            return
        await history.load_around(boundary)
        if history is not self._history:
            return
        self._sync_latest()
        unread = [m for m in history.messages if m["id"] > boundary and not m.get("is_outgoing")]
        if not unread:
            await self._load_latest(history)
            return
        self._first_unread = unread[-1]["id"]
        self._row_changed(history.row_of(self._first_unread))
        self.unreadReady.emit(self._first_unread)

    async def _formatted(self, text: str) -> dict[str, Any]:
        formatted: dict[str, Any] = {"@type": "formattedText", "text": text, "entities": []}
        if not text:
            return formatted
        try:  # **bold**, __italic__, `code`, ```pre```, ~~strike~~, ||spoiler||, [text](url)
            parsed = await self._client.send({"@type": "parseMarkdown", "text": formatted})
        except TdError as e:
            log.debug("parseMarkdown failed, sending plain text: %s", e)
            return formatted
        return mention_names(parsed)

    async def _run_load(self, history: ChatHistory, load: Any) -> None:
        self._set_loading(True)
        try:
            await load
        finally:
            if history is self._history:
                self._set_loading(False)

    def _set_loading(self, value: bool) -> None:
        if value != self._loading:
            self._loading = value
            self.loadingChanged.emit()

    async def _send(self, chat_id: int, text: str, reply_to: int,
                    options: dict[str, Any] | None = None) -> None:
        options = options or {}
        formatted = await self._formatted(text)
        send_options = _send_options(options)
        preview_options = ({"@type": "linkPreviewOptions", "is_disabled": True, "url": "",
                            "force_small_media": False, "force_large_media": False,
                            "show_above_text": False} if options.get("noPreview") else None)
        request: dict[str, Any] = {
            "@type": "sendMessage",
            "chat_id": chat_id,
            "topic_id": self._topic_for(chat_id),
            "input_message_content": {"@type": "inputMessageText", "text": formatted,
                                      "link_preview_options": preview_options,
                                      "clear_draft": True},
        }
        if reply_to:
            request["reply_to"] = {"@type": "inputMessageReplyToMessage", "message_id": reply_to}
        if send_options is not None:
            request["options"] = send_options
        try:
            message = await self._client.send(request)
        except TdError as e:
            log.warning("sendMessage failed: %s", e)
            return
        if send_options is not None and send_options.get("scheduling_state"):
            return  # tgclient: _load_scheduled (scheduled messages come later)
        history = self._history
        if (history and history.chat_id == chat_id and message.get("id")
                and history.get(message["id"]) is None):
            history.add(message)  # usually already added by updateNewMessage

    def _spawn(self, awaitable: Any) -> None:
        task = asyncio.ensure_future(awaitable)
        task.add_done_callback(_log_failure)


def _send_options(options: dict[str, Any]) -> dict[str, Any] | None:
    """messageSendOptions from the composer's {silent, scheduleAt}; None for the defaults."""
    silent, at = bool(options.get("silent")), int(options.get("scheduleAt") or 0)
    if not silent and not at:
        return None
    return {"@type": "messageSendOptions", "disable_notification": silent,
            "from_background": False, "protect_content": False,
            "scheduling_state": {"@type": "messageSchedulingStateSendAtDate", "send_date": at,
                                 "repeat_period": 0} if at else None}


def _log_failure(task: asyncio.Task[Any]) -> None:
    if not task.cancelled() and task.exception() is not None:
        log.error("Message model task failed", exc_info=task.exception())
