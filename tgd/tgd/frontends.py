"""tgd's two front ends over one dispatcher.

- `JsonFrontEnd` (`tgd --json`): the line protocol, one JSON object per line on stdin and
  stdout (docs/tgd-protocol.md). Requests run concurrently; each gets one reply.
- `TextFrontEnd` (`tgd`): commands typed in a terminal, for testing and for a person:
  login, chats [N], open CHAT, close, history CHAT [N], send CHAT TEXT, ai CHAT on|off,
  aikey, summary CHAT [unread|day|week], status, help, quit.

Standard input is read with os.read in a thread of its own, not with input(): on Zoraya a
program's stdin is the shell's stream (not a tty), where os.read returns each typed line and
CPython's buffered input() did not (TG-03's probe).
"""

from __future__ import annotations

import asyncio
import contextlib
import json
import os
import sys
import threading
import time
from collections.abc import Awaitable, Callable
from typing import Any

from .core import Core, CoreError

MAX_LINE = 64 * 1024


class LineReader:
    """Lines of fd 0, as str without the newline; None at end of file."""

    def __init__(self, loop: asyncio.AbstractEventLoop) -> None:
        self._loop = loop
        self._queue: asyncio.Queue[str | None] = asyncio.Queue()
        threading.Thread(target=self._run, name="tgd-stdin", daemon=True).start()

    def _run(self) -> None:
        pending = b""
        while True:
            try:
                chunk = os.read(0, 4096)
            except InterruptedError:
                continue
            except OSError:
                chunk = b""
            if not chunk:
                if pending:
                    self._put(pending)
                self._put(None)
                return
            pending += chunk.replace(b"\r\n", b"\n").replace(b"\r", b"\n")
            while b"\n" in pending:
                line, pending = pending.split(b"\n", 1)
                self._put(line)
            if len(pending) > MAX_LINE:
                self._put(pending)
                pending = b""

    def _put(self, raw: bytes | None) -> None:
        line = None if raw is None else raw.decode("utf-8", errors="replace")
        with contextlib.suppress(RuntimeError):  # the loop has closed
            self._loop.call_soon_threadsafe(self._queue.put_nowait, line)

    async def line(self) -> str | None:
        return await self._queue.get()


class Dispatcher:
    """The protocol's requests: {"op": NAME, ...} -> a reply's fields (or CoreError)."""

    def __init__(self, core: Core, quit: Callable[[], None]) -> None:
        self.core = core
        self._quit = quit
        self.ops: dict[str, Callable[[dict[str, Any]], Awaitable[dict[str, Any]]]] = {
            "version": self._version, "status": self._status,
            "auth.credentials": self._credentials,
            **{f"auth.{kind}": self._answerer(kind)
               for kind in ("phone", "code", "password", "email", "email_code")},
            "chats.list": self._chats, "chat.open": self._open, "chat.close": self._close,
            "messages.history": self._history, "messages.send": self._send,
            "ai.key": self._ai_key, "ai.enable": self._ai_enable,
            "ai.summarize": self._summarize, "quit": self._quit_op,
        }

    async def handle(self, request: dict[str, Any]) -> dict[str, Any]:
        op = request.get("op")
        handler = self.ops.get(op) if isinstance(op, str) else None
        if handler is None:
            raise CoreError(f"unknown op {op!r}")
        return await handler(request)

    async def _version(self, r: dict[str, Any]) -> dict[str, Any]:
        from . import PROTOCOL, VERSION  # noqa: PLC0415

        return {"tgd": VERSION, "protocol": PROTOCOL, "tdlib": self.core.tdlib_version}

    async def _status(self, r: dict[str, Any]) -> dict[str, Any]:
        return self.core.status()

    async def _credentials(self, r: dict[str, Any]) -> dict[str, Any]:
        self.core.set_credentials(r.get("api_id", ""), str(r.get("api_hash", "")))
        return {}

    def _answerer(self, kind: str) -> Callable[[dict[str, Any]], Awaitable[dict[str, Any]]]:
        async def answer(r: dict[str, Any]) -> dict[str, Any]:
            self.core.answer(kind, str(r.get("value", "")))
            return {}

        return answer

    async def _chats(self, r: dict[str, Any]) -> dict[str, Any]:
        chats = await self.core.list_chats(int(r.get("limit", 20)))
        return {"chats": [self.core.chat_out(c) for c in chats]}

    async def _open(self, r: dict[str, Any]) -> dict[str, Any]:
        chat_id = await self.core.resolve(r.get("chat", ""))
        messages = await self.core.open_chat(chat_id, int(r.get("limit", 20)))
        return {"chat": self.core.chat_out(self.core.chat(chat_id)), "messages": messages}

    async def _close(self, r: dict[str, Any]) -> dict[str, Any]:
        await self.core.close_chat()
        return {}

    async def _history(self, r: dict[str, Any]) -> dict[str, Any]:
        chat_id = await self.core.resolve(r.get("chat", ""))
        return {"chat_id": chat_id,
                "messages": await self.core.history(chat_id, int(r.get("limit", 20)))}

    async def _send(self, r: dict[str, Any]) -> dict[str, Any]:
        chat_id = await self.core.resolve(r.get("chat", ""))
        return {"chat_id": chat_id, **await self.core.send_text(chat_id, str(r.get("text", "")))}

    async def _ai_key(self, r: dict[str, Any]) -> dict[str, Any]:
        self.core.set_ai_key(str(r.get("key", "")))
        return {}

    async def _ai_enable(self, r: dict[str, Any]) -> dict[str, Any]:
        chat_id = await self.core.resolve(r.get("chat", ""))
        on = bool(r.get("on", True))
        if not self.core.ai_enable(chat_id, on):
            raise CoreError("AI is never used in secret chats")
        return {"chat_id": chat_id, "on": on}

    async def _summarize(self, r: dict[str, Any]) -> dict[str, Any]:
        chat_id = await self.core.resolve(r.get("chat", ""))
        return {"chat_id": chat_id,
                **await self.core.summarize(chat_id, str(r.get("scope", "day")))}

    async def _quit_op(self, r: dict[str, Any]) -> dict[str, Any]:
        self._quit()
        return {}


def _write(text: str) -> None:
    sys.stdout.write(text)
    sys.stdout.flush()


class JsonFrontEnd:
    def __init__(self) -> None:
        self._done = asyncio.Event()
        self._tasks: set[asyncio.Task[None]] = set()

    def emit(self, event: dict[str, Any]) -> None:
        _write(json.dumps(event, ensure_ascii=False) + "\n")

    def quit(self) -> None:
        self._done.set()

    async def run(self, core: Core, reader: LineReader) -> None:
        dispatcher = Dispatcher(core, self.quit)
        reading = asyncio.ensure_future(self._read(dispatcher, reader))
        await self._done.wait()
        reading.cancel()
        for task in list(self._tasks):
            if not task.done():
                await asyncio.wait([task], timeout=2)

    async def _read(self, dispatcher: Dispatcher, reader: LineReader) -> None:
        while True:
            line = await reader.line()
            if line is None:
                self.quit()
                return
            if not line.strip():
                continue
            try:
                request = json.loads(line)
                if not isinstance(request, dict):
                    raise ValueError("not an object")
            except ValueError as e:
                self.emit({"re": None, "ok": False, "error": f"not a JSON object: {e}"})
                continue
            task = asyncio.ensure_future(self._one(dispatcher, request))
            self._tasks.add(task)
            task.add_done_callback(self._tasks.discard)

    async def _one(self, dispatcher: Dispatcher, request: dict[str, Any]) -> None:
        rid = request.get("id")
        try:
            result = await dispatcher.handle(request)
            self.emit({"re": rid, "ok": True, **result})
        except CoreError as e:
            self.emit({"re": rid, "ok": False, "error": str(e)})
        except Exception as e:  # noqa: BLE001 - a reply for every request
            import logging  # noqa: PLC0415

            logging.getLogger("tgd").exception("op %s failed", request.get("op"))
            self.emit({"re": rid, "ok": False, "error": f"internal error ({type(e).__name__})"})


HELP = """\
commands:
  login                    log in (asks for what Telegram needs, one thing at a time)
  chats [N]                the first N chats of the main list (20)
  open CHAT                the chat's last messages, then its new ones as they come
  close                    stop showing the open chat's new messages
  history CHAT [N]         the chat's last N messages (20)
  send CHAT TEXT           send TEXT to the chat
  ai CHAT on|off           allow (or stop) AI for this chat (off for every chat at first)
  aikey                    type an OpenRouter key (kept in the data directory)
  summary CHAT [SCOPE]     AI summary of the chat: unread, day (default) or week
  status                   the login state and what tgd uses
  quit                     close TDLib cleanly and end
CHAT is the number from the last `chats`, a chat id, or a part of the title."""

_STATE_TEXT = {
    "need_credentials": "no api id and hash yet: type login",
    "starting": "starting TDLib",
    "wait_phone": "Telegram asks for the phone number: type login",
    "wait_code": "Telegram sent a login code: type login",
    "wait_password": "Telegram asks for the 2FA password: type login",
    "wait_email": "Telegram asks for an email address: type login",
    "wait_email_code": "Telegram sent a code by email: type login",
    "closed": "TDLib closed",
}


class TextFrontEnd:
    def __init__(self) -> None:
        self._done = False
        self._in_login = False
        self._tty = os.isatty(0)

    def emit(self, event: dict[str, Any]) -> None:
        kind = event.get("ev")
        if kind == "hello":
            mode = "test DC" if event.get("test_dc") else "production DC"
            _write(f"tgd {event['tgd']}, TDLib {event['tdlib']} ({event['binding']}), "
                   f"{mode}, data in {event['data']}\n")
        elif kind == "auth.state":
            state = event["state"]
            if state == "ready":
                me = event.get("me") or {}
                _write(f"logged in{' as ' + me['name'] if me.get('name') else ''}\n")
            elif state == "failed":
                _write(f"login failed: {event.get('message', '')}\n")
            elif not self._in_login and state in _STATE_TEXT:
                _write(f"[{state}, TDLib {event.get('td') or '-'}] {_STATE_TEXT[state]}\n")
        elif kind == "auth.error":
            _write(f"Telegram: {event.get('message', '')}\n")
        elif kind == "bye":
            _write(f"bye (TDLib {event.get('td')})\n")
        elif kind == "message.new":
            _write(_line(event["message"], live=True) + "\n")

    async def run(self, core: Core, reader: LineReader) -> None:
        self.core, self.reader = core, reader
        _write("type help for the commands\n")
        while not self._done:
            line = await self._ask("tgd> ")
            if line is None:
                break
            words = line.strip().split(maxsplit=1)
            if not words:
                continue
            name, rest = words[0].lower(), words[1] if len(words) > 1 else ""
            try:
                await self._command(name, rest)
            except CoreError as e:
                _write(f"{e}\n")
            except Exception as e:  # noqa: BLE001
                import logging  # noqa: PLC0415

                logging.getLogger("tgd").exception("command %s failed", name)
                _write(f"internal error ({type(e).__name__}); see tgd.log\n")

    async def _ask(self, prompt: str, secret: bool = False) -> str | None:
        _write(prompt)
        restore = self._echo_off() if secret else None
        try:
            line = await self.reader.line()
        finally:
            if restore is not None:
                restore()
                _write("\n")
        return line

    def _echo_off(self) -> Callable[[], None] | None:
        if not self._tty:
            return None
        try:
            import termios  # noqa: PLC0415

            old = termios.tcgetattr(0)
            new = list(old)
            new[3] &= ~termios.ECHO
            termios.tcsetattr(0, termios.TCSADRAIN, new)
        except Exception:  # noqa: BLE001 - no terminal control: the input is shown
            return None
        return lambda: termios.tcsetattr(0, termios.TCSADRAIN, old)

    async def _command(self, name: str, rest: str) -> None:
        core = self.core
        match name:
            case "help" | "?":
                _write(HELP + "\n")
            case "quit" | "exit":
                self._done = True
            case "status":
                s = core.status()
                me = s["me"].get("name", "") if s["me"] else ""
                _write(f"state {s['auth']} (TDLib {s['td'] or '-'}){', ' + me if me else ''}; "
                       f"TDLib {s['tdlib']} through {s['binding']}; "
                       f"{'test' if s['test_dc'] else 'production'} DC; "
                       f"AI key {'set' if s['ai_key'] else 'not set'}\n")
            case "login":
                await self._login()
            case "chats":
                limit = int(rest) if rest.strip().isdigit() else 20
                for n, chat in enumerate(await core.list_chats(limit), 1):
                    unread = f"  [{chat.unread_count}]" if chat.unread_count else ""
                    _write(f"{n:3}  {chat.title}{unread}  ({chat.type}, {chat.id})\n")
            case "open":
                chat_id = await core.resolve(rest)
                messages = await core.open_chat(chat_id)
                chat = core.chat(chat_id)
                _write(f"-- {chat.title if chat else chat_id} --\n")
                for m in messages:
                    _write(_line(m) + "\n")
                _write("-- new messages will show here; `close` to stop --\n")
            case "close":
                await core.close_chat()
            case "history":
                ref, limit = _ref_and_count(rest)
                chat_id = await core.resolve(ref)
                for m in await core.history(chat_id, limit):
                    _write(_line(m) + "\n")
            case "send":
                ref, _, text = rest.partition(" ")
                if not text.strip():
                    raise CoreError("send CHAT TEXT")
                chat_id = await core.resolve(ref)
                result = await core.send_text(chat_id, text)
                _write(f"{result['state']} (message {result['id']})\n")
            case "ai":
                ref, _, switch = rest.rpartition(" ")
                if switch not in ("on", "off") or not ref:
                    raise CoreError("ai CHAT on|off")
                chat_id = await core.resolve(ref)
                if not core.ai_enable(chat_id, switch == "on"):
                    raise CoreError("AI is never used in secret chats")
                _write(f"AI {switch} for this chat\n")
            case "aikey":
                key = await self._ask("OpenRouter key: ", secret=True)
                if key:
                    core.set_ai_key(key)
                    _write("key kept in the data directory\n")
            case "summary":
                parts = rest.split()
                scope = "day"
                if parts and parts[-1] in ("unread", "day", "week"):
                    scope = parts.pop()
                chat_id = await core.resolve(" ".join(parts))
                _write(f"asking for a summary ({scope})…\n")
                result = await core.summarize(chat_id, scope)
                _write(result["text"].rstrip() + "\n")
                _write(f"({result['count']} messages, ${result['cost']:.4f})\n")
            case _:
                raise CoreError(f"unknown command {name!r}; type help")

    async def _login(self) -> None:
        core = self.core
        self._in_login = True
        try:
            if core.client is None:
                _write("api id and hash from my.telegram.org (kept in the data directory)\n")
                api_id = await self._ask("api id: ")
                if api_id is None:
                    return
                api_hash = await self._ask("api hash: ", secret=True)
                if api_hash is None:
                    return
                core.set_credentials(api_id, api_hash)
            seen = -1
            while True:
                state = core.auth_state
                kind, info = core.asking
                if state == "ready":
                    _write("already logged in\n" if seen == -1 else "")
                    return
                if state in ("failed", "closed", "closing", "logging_out"):
                    _write(f"cannot log in now ({state})\n")
                    return
                if kind:
                    value = await self._ask(_prompt(kind, info), secret=kind == "password")
                    if value is None:
                        return
                    try:
                        core.answer(kind, value)
                    except CoreError as e:
                        _write(f"{e}\n")
                    seen = core.auth_version
                    with contextlib.suppress(TimeoutError):
                        await core.wait_auth_change(seen, timeout=30)
                    continue
                if seen == -1:
                    seen = core.auth_version
                try:
                    seen = await core.wait_auth_change(seen, timeout=60)
                except TimeoutError:
                    _write(f"still {state} (TDLib {core.td_state or '-'}); waiting\n")
        finally:
            self._in_login = False


def _prompt(kind: str, info: dict[str, Any]) -> str:
    match kind:
        case "phone":
            return "phone number (international, +…): "
        case "code":
            via = info.get("via", "")
            return f"login code{' (' + via + ')' if via else ''}: "
        case "password":
            hint = info.get("hint", "")
            return f"2FA password{' (hint: ' + hint + ')' if hint else ''}: "
        case "email":
            return "email address: "
        case "email_code":
            return "code sent by email: "
    return f"{kind}: "


def _ref_and_count(rest: str) -> tuple[str, int]:
    ref, _, count = rest.rpartition(" ")
    if ref and count.isdigit():
        return ref, int(count)
    return rest, 20


def _line(m: dict[str, Any], live: bool = False) -> str:
    stamp = time.strftime("%Y-%m-%d %H:%M", time.localtime(m.get("date", 0)))
    who = "me" if m.get("out") else m.get("sender", "?")
    text = m.get("text", "")
    if m.get("kind") != "text":
        text = f"[{text}]"
    return f"{'* ' if live else ''}{stamp}  {who}: {text}"
