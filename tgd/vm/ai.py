"""tgclient's AiController (ui/ai_controller.py), the chat summary: the open chat
(`chatId`), whether AI may and does run there (`available`, `enabled`, `setEnabled`),
`summarize`, and what the panel shows (`summaryState`, `summaryText`, `summaryError`),
with `summaryChanged` and `chatChanged`. Method bodies unchanged. The key, the models, the
budget, questions, events, answers, documents, replies, translation and the digest come
later; tgd's own `ai.key` (protocol 1) sets the key meanwhile.

The service is tgclient's AiService (services/ai.py), unchanged.
"""

from __future__ import annotations

from typing import Any

from .qt import Property, QObject, Signal, Slot


class AiController(QObject):
    chatChanged = Signal()
    summaryChanged = Signal()
    enabledChatsChanged = Signal()
    configChanged = Signal()
    usageChanged = Signal()
    globalChanged = Signal()

    def __init__(self, service: Any, chats: Any, parent: Any = None) -> None:
        super().__init__(parent)
        self._service = service
        self._chats = chats
        self._chat_id = 0
        self._subject = ""  # what the summary panel shows: "" = the chat, "user:<id>" = a person
        self._topic_id = 0  # open forum topic
        # (chat id, topic id) -> (topic name, last read message id); set by the app
        self.topic_info: Any = lambda chat, topic: ("", 0)
        self._global = "digest"
        service.subscribe(self._on_service)
        chats.subscribe(self._on_chats)

    # --- the open chat ----------------------------------------------------------------------

    def _get_chat_id(self) -> int:
        return self._chat_id

    def _set_chat_id(self, chat_id: Any) -> None:
        chat_id = int(chat_id or 0)
        if chat_id != self._chat_id:
            self._chat_id = chat_id
            self._subject = ""
            self.chatChanged.emit()
            self.summaryChanged.emit()

    chatId = Property("QVariant", _get_chat_id, _set_chat_id, notify=chatChanged)  # noqa: N815

    def _summary_subject(self) -> str:
        return f"topic:{self._topic_id}" if self._topic_id else ""

    @Property(bool, notify=chatChanged)
    def available(self) -> bool:
        """False for secret chats: AI can't be turned on there."""
        return self._service.allowed(self._chat_id)

    @Property(bool, notify=chatChanged)
    def enabled(self) -> bool:
        return self._service.is_enabled(self._chat_id)

    @Slot(bool)
    def setEnabled(self, enabled: bool) -> None:  # noqa: N802
        self._service.set_enabled(self._chat_id, enabled)

    @Slot(str)
    def summarize(self, scope: str) -> None:
        if self._chat_id:
            self._show(self._summary_subject())
            name, last_read = (self.topic_info(self._chat_id, self._topic_id)
                               if self._topic_id else ("", 0))
            self._service.summarize(self._chat_id, scope, self._topic_id, name, last_read)

    def _show(self, subject: str) -> None:
        if subject != self._subject:
            self._subject = subject
            self.summaryChanged.emit()

    def _current(self):  # noqa: ANN202
        return self._service.summary(self._chat_id, self._subject)

    @Property(str, notify=summaryChanged)
    def summaryState(self) -> str:  # noqa: N802
        return self._current().state

    @Property(str, notify=summaryChanged)
    def summaryScope(self) -> str:  # noqa: N802
        return self._current().scope

    @Property(str, notify=summaryChanged)
    def summaryText(self) -> str:  # noqa: N802
        return self._current().text

    @Property(str, notify=summaryChanged)
    def summaryError(self) -> str:  # noqa: N802
        return self._current().error

    # --- internals --------------------------------------------------------------------------

    def _on_service(self, kind: str, payload: Any) -> None:
        if kind == "config":
            self.configChanged.emit()
        elif kind in ("enabled", "flags"):
            self.enabledChatsChanged.emit()
            if payload == self._chat_id:
                self.chatChanged.emit()
        elif kind == "summary" and payload == (self._chat_id, self._subject):
            self.summaryChanged.emit()
        elif kind == "summary" and payload == (0, self._global):
            self.globalChanged.emit()
        elif kind == "usage":
            self.usageChanged.emit()

    def _on_chats(self, kind: str, payload: Any) -> None:
        if kind == "chat" and self._service.is_enabled(payload):
            self.enabledChatsChanged.emit()  # title change
        if kind == "chat" and payload == self._chat_id:
            self.chatChanged.emit()  # the chat may have just arrived
