"""The protocol's rows (docs/tg-window.md, "The protocol") from the view-models' data:
what QML read by role, the window reads as JSON.

`rich()` is the emitter that sits beside tgclient's `formatted_to_html` (store/richtext.py):
the same entities and the same UTF-16 map, out as `{"text", "spans": [[start, end, kind]]}`
in code points. Spoilers stay hidden as in tgclient: their characters become U+2591, so no
offset moves and nothing in them is a span.
"""

from __future__ import annotations

from typing import Any

from tgclient.store.format import content_preview
from tgclient.store.media import duration_text, extract, human_size
from tgclient.store.richtext import utf16_index_map

from . import chat_list, messages
from .qt import QModelIndex

SPAN_KINDS = {
    "textEntityTypeBold": "bold",
    "textEntityTypeItalic": "italic",
    "textEntityTypeBlockQuote": "italic",  # tgclient draws a quote in italics too
    "textEntityTypeExpandableBlockQuote": "italic",
    "textEntityTypeCode": "code",
    "textEntityTypePre": "code",
    "textEntityTypePreCode": "code",
    "textEntityTypeTextUrl": "link",
    "textEntityTypeUrl": "link",
    "textEntityTypeEmailAddress": "link",
    "textEntityTypePhoneNumber": "link",
    "textEntityTypeHashtag": "link",
    "textEntityTypeCashtag": "link",
    "textEntityTypeBotCommand": "link",
    "textEntityTypeMention": "mention",
    "textEntityTypeMentionName": "mention",
}
HIDDEN = "░"


def rich(formatted: dict[str, Any] | None) -> dict[str, Any]:
    text = (formatted or {}).get("text", "") or ""
    entities = (formatted or {}).get("entities") or []
    if not text:
        return {"text": "", "spans": []}
    index = utf16_index_map(text)
    limit = len(index) - 1
    chars = list(text)
    hidden: list[tuple[int, int]] = []
    spans: list[list[Any]] = []
    for entity in entities:
        offset, length = entity.get("offset", 0), entity.get("length", 0)
        if length <= 0 or offset < 0 or offset >= limit:
            continue
        start, end = index[offset], index[min(offset + length, limit)]
        if start >= end:
            continue
        kind = (entity.get("type") or {}).get("@type", "")
        if kind == "textEntityTypeSpoiler":
            hidden.append((start, end))
            for i in range(start, end):
                if chars[i] not in "\n ":
                    chars[i] = HIDDEN
            continue
        if kind in SPAN_KINDS:
            spans.append([start, end, SPAN_KINDS[kind]])
    spans = [s for s in spans if not any(a < s[1] and s[0] < b for a, b in hidden)]
    spans.sort(key=lambda s: (s[0], -s[1]))
    return {"text": "".join(chars), "spans": spans}


def plain(text: str) -> dict[str, Any]:
    return {"text": text, "spans": []}


# --- chats ------------------------------------------------------------------------------------

def chat_kind(chat: Any, my_id: int | None) -> str:
    if chat.type in ("private", "secret"):
        return "saved" if my_id and chat.peer_id == my_id else "private"
    return "channel" if chat.type == "channel" else "group"


def chat_row(model: chat_list.ChatListModel, row: int) -> dict[str, Any]:
    """Row `row` of the chat list model as the protocol's chat row."""
    R = chat_list.Role
    i = QModelIndex(row)
    chat_id = model.data(i, R.ChatId)
    chat = model._store.chats[chat_id]
    return chat_out(chat, model.data(i, R.Title), model.data(i, R.Preview),
                    model.data(i, R.UnreadCount), model.data(i, R.Pinned),
                    model.data(i, R.Muted), model._users.my_id)


def chat_out(chat: Any, title: str, last: str, unread: int, pinned: bool, muted: bool,
             my_id: int | None) -> dict[str, Any]:
    message = chat.last_message or {}
    return {"id": str(chat.id), "title": title, "kind": chat_kind(chat, my_id),
            "last": last, "date": int(message.get("date") or 0), "unread": int(unread),
            "pinned": bool(pinned), "muted": bool(muted)}


# --- messages ---------------------------------------------------------------------------------

def kind_and_detail(content: dict[str, Any]) -> tuple[str, str]:
    """What a message is and what its placeholder says until Zoraya has decoders."""
    media = extract(content)
    match content.get("@type"):
        case "messagePhoto":
            return "photo", f"{media.width} × {media.height}" if media and media.width else ""
        case "messageVideo" | "messageAnimation" | "messageVideoNote":
            label = {"messageVideo": "Video", "messageAnimation": "GIF",
                     "messageVideoNote": "Video message"}[content["@type"]]
            seconds = media.duration if media else 0
            return "photo", f"{label} · {duration_text(seconds)}" if seconds else label
        case "messageSticker":
            return "sticker", media.emoji if media else ""
        case "messageAnimatedEmoji":
            return ("sticker", media.emoji) if media else ("text", "")
        case "messageVoiceNote":
            return "voice", duration_text(media.duration if media else 0)
        case "messageAudio" | "messageDocument":
            name = (media.title or media.file_name) if media else ""
            size = int(((media.file if media else None) or {}).get("size") or 0)
            parts = [name or ("Audio" if content["@type"] == "messageAudio" else "File")]
            if media and media.duration:
                parts.append(duration_text(media.duration))
            if size:
                parts.append(human_size(size))
            return "file", " · ".join(parts)
    return "text", ""


def message_row(model: messages.MessageListModel, row: int) -> dict[str, Any]:
    """Row `row` of the message list model as the protocol's message row."""
    R = messages.Role
    i = QModelIndex(row)
    content = model.data(i, R.Content) or {}
    reply_sender = model.data(i, R.ReplySender)
    reply_text = model.data(i, R.ReplyText)
    forward = model.data(i, R.ForwardedFrom)
    if model.data(i, R.IsService):
        kind, detail, text = "service", "", plain(model.data(i, R.ServiceText))
    else:
        kind, detail = kind_and_detail(content)
        body = model.data(i, R.Body)
        if body is None and kind == "text":
            body = {"text": content_preview(content), "entities": []}  # location, poll, ...
        text = rich(body)
    status = model.data(i, R.Status)
    return {
        "id": str(model.data(i, R.MessageId)),
        "date": model.data(i, R.Time),
        "out": model.data(i, R.IsOutgoing),
        "sender": model.data(i, R.SenderName),
        "kind": kind,
        "text": text,
        "detail": detail,
        "reply": ({"sender": reply_sender, "text": reply_text}
                  if model.data(i, R.ReplyToId) and (reply_sender or reply_text) else None),
        "forward": forward or None,
        "edited": model.data(i, R.Edited),
        # tgclient's "failed" has no state in protocol 2 yet: it stays pending.
        "state": {"pending": "pending", "failed": "pending", "read": "read"}.get(status, "sent"),
    }
