"""tgclient's ComposerModel (models/composer.py), its drafts: `setDraft`, `sent`, `flush`,
`close`, the chat switch, the cloud draft coming back, the typing action. Method bodies
unchanged; `_on_chat_changed` drops the calls into what is not here yet (staged files, the
link preview, @mentions). The protocol's `messages.send` is `sent()` and then the message
model's `send`; `draft.set` (docs/tg-window.md, "later") will be `setDraft`.
"""

from __future__ import annotations

import asyncio
import logging
import time
from typing import Any

from tgclient.store.chats import ChatStore, draft_reply_to, draft_text
from tgclient.td.client import TdClient, TdError

from .messages import MessageListModel
from .qt import Property, QObject, Signal, Slot

log = logging.getLogger(__name__)

DRAFT_DELAY = 1.5  # seconds of no typing before the draft is saved
TYPING_INTERVAL = 5.0  # Telegram shows a typing action for ~6 s


class ComposerModel(QObject):
    remoteDraft = Signal(str, "QVariant")  # text, reply-to id: replace the input

    def __init__(self, client: TdClient, chats: ChatStore, messages: MessageListModel,
                 parent: Any = None) -> None:
        super().__init__(parent)
        self._client = client
        self._chats = chats
        self._messages = messages
        self._chat_id = 0
        self._topic: dict[str, Any] | None = None  # forum topic of the input, if any
        self._text = ""
        self._reply_to = 0
        self._synced: tuple[str, int] = ("", 0)  # what TDLib has for the current chat
        self._save_task: asyncio.Task[Any] | None = None
        self._last_typing = 0.0
        self._tasks: set[asyncio.Task[Any]] = set()
        messages.chatChanged.connect(self._on_chat_changed)
        chats.subscribe(self._on_chats)

    # --- drafts -----------------------------------------------------------------------------

    @Property(str, notify=remoteDraft)
    def draftText(self) -> str:  # noqa: N802
        """Saved draft of the open chat (or forum topic): QML puts it into the input."""
        return draft_text(self._messages.current_draft())

    @Property("QVariant", notify=remoteDraft)
    def draftReplyTo(self) -> int:  # noqa: N802
        return draft_reply_to(self._messages.current_draft())

    @Slot(str, "QVariant")
    def setDraft(self, text: str, reply_to: Any = 0) -> None:  # noqa: N802
        if not self._chat_id:
            return
        reply_to = int(reply_to or 0)
        typed = text != self._text and bool(text.strip())
        self._text, self._reply_to = text, reply_to
        if typed:
            self._send_typing()
        self._cancel_save()
        if (text.strip(), reply_to) != self._synced:
            self._save_task = self._spawn(self._save_later(self._chat_id, text, reply_to))

    @Slot()
    def sent(self) -> None:
        """The input was sent: TDLib clears the draft itself (clear_draft)."""
        self._cancel_save()
        self._text, self._reply_to = "", 0
        self._synced = ("", 0)

    def flush(self) -> asyncio.Future[Any] | None:
        """Save a pending draft now (switching chats, quitting)."""
        if self._save_task is None or self._save_task.done():
            return None
        self._save_task.cancel()
        self._save_task = None
        return self._spawn(self._save(self._chat_id, self._text, self._reply_to))

    async def close(self) -> None:
        pending = self.flush()
        if pending is not None:
            await asyncio.wait([pending], timeout=2)

    def _on_chat_changed(self) -> None:
        chat_id = int(self._messages.chatId or 0)
        topic = self._messages.topic_obj()
        if (chat_id, topic) == (self._chat_id, self._topic):
            return
        self.flush()
        self._chat_id, self._topic = chat_id, topic
        self._last_typing = 0.0
        draft = self._messages.current_draft()
        self._text = draft_text(draft)
        self._reply_to = draft_reply_to(draft)
        self._synced = (self._text.strip(), self._reply_to)

    def _on_chats(self, kind: str, payload: Any) -> None:
        if kind != "chat" or payload != self._chat_id or not self._chat_id or self._topic:
            return
        chat = self._chats.chats[self._chat_id]
        remote = (draft_text(chat.draft), draft_reply_to(chat.draft))
        if (remote[0].strip(), remote[1]) == self._synced:
            return  # our own save coming back, or nothing changed
        if (self._text.strip(), self._reply_to) != self._synced:
            return  # the user is typing: their text wins, it will overwrite the cloud draft
        self._text, self._reply_to = remote
        self._synced = (remote[0].strip(), remote[1])
        self.remoteDraft.emit(remote[0], remote[1])

    async def _save_later(self, chat_id: int, text: str, reply_to: int) -> None:
        await asyncio.sleep(DRAFT_DELAY)
        await self._save(chat_id, text, reply_to)

    async def _save(self, chat_id: int, text: str, reply_to: int) -> None:
        draft: dict[str, Any] | None = None
        if text.strip():
            draft = {
                "@type": "draftMessage",
                "reply_to": ({"@type": "inputMessageReplyToMessage", "message_id": reply_to}
                             if reply_to else None),
                "date": 0,
                "content": {"@type": "draftMessageContentText", "text": {
                    "@type": "formattedText", "text": text, "entities": []},
                    "link_preview_options": None},
                "effect_id": 0,
                "suggested_post_info": None,
            }
        if chat_id == self._chat_id:
            self._synced = (text.strip(), reply_to if draft else 0)
        try:
            await self._client.send({"@type": "setChatDraftMessage", "chat_id": chat_id,
                                     "topic_id": self._topic if chat_id == self._chat_id
                                     else None, "draft_message": draft})
        except TdError as e:
            log.warning("Saving the draft failed: %s", e)

    def _cancel_save(self) -> None:
        if self._save_task is not None:
            self._save_task.cancel()
            self._save_task = None

    def _send_typing(self) -> None:
        now = time.monotonic()
        if now - self._last_typing < TYPING_INTERVAL:
            return
        self._last_typing = now
        self._spawn(self._request({
            "@type": "sendChatAction", "chat_id": self._chat_id, "topic_id": self._topic,
            "business_connection_id": "", "action": {"@type": "chatActionTyping"}}))

    async def _request(self, request: dict[str, Any]) -> None:
        try:
            await self._client.send(request)
        except TdError as e:
            log.debug("%s failed: %s", request["@type"], e)

    def _spawn(self, coro: Any) -> asyncio.Task[Any]:
        task = asyncio.ensure_future(coro)
        self._tasks.add(task)
        task.add_done_callback(self._done)
        return task

    def _done(self, task: asyncio.Task[Any]) -> None:
        self._tasks.discard(task)
        if not task.cancelled() and task.exception() is not None:
            log.error("Composer task failed", exc_info=task.exception())
