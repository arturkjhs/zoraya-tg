"""tgd's core: one TDLib client and tgclient's Qt-free layers, behind an async API that both
front ends (the JSON-line protocol and the text front end) call.

What is tgclient's, unchanged: `td/` (TdHub, TdClient, AuthFlow, TdlibParams), `store/`
(ChatStore, UserStore, FileManager, ChatHistory, format), `services/` (AiService, AiStore,
OpenRouter), `vault.py` (the database key, `TGC_KEY_STORE=file`: no keyring), `config.py`
(Settings), `prefs.py`. What is tgd's: the wiring that tgclient's `app.Session` does with Qt
(here without Qt), the authorization prompts as awaitable questions, and the shapes that go
to a front end.

Privacy: nothing here logs a credential, a code, a password, a key or message text. The
credentials are kept only in the data directory (`credentials.json`, mode 0600).
"""

from __future__ import annotations

import asyncio
import base64
import dataclasses
import json
import logging
import os
import sys
import time
from collections.abc import Callable
from pathlib import Path
from typing import Any

from . import VERSION

log = logging.getLogger("tgd")

Emit = Callable[[dict[str, Any]], None]

ACCOUNT = "0"  # tgclient's first account (accounts.FIRST)

# TDLib's authorization states -> the protocol's short names.
AUTH_STATES = {
    "authorizationStateWaitTdlibParameters": "starting",
    "authorizationStateWaitPhoneNumber": "wait_phone",
    "authorizationStateWaitEmailAddress": "wait_email",
    "authorizationStateWaitEmailCode": "wait_email_code",
    "authorizationStateWaitCode": "wait_code",
    "authorizationStateWaitOtherDeviceConfirmation": "wait_other_device",
    "authorizationStateWaitRegistration": "wait_registration",
    "authorizationStateWaitPassword": "wait_password",
    "authorizationStateReady": "ready",
    "authorizationStateLoggingOut": "logging_out",
    "authorizationStateClosing": "closing",
    "authorizationStateClosed": "closed",
}
ASK_KINDS = ("phone", "code", "password", "email", "email_code")


class CoreError(Exception):
    """A request a front end made that cannot be done; the message is for the person."""


def _write_private(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(fd, "w", encoding="utf-8") as f:
        f.write(text)
    os.replace(tmp, path)


class Credentials:
    """api id and hash, the OpenRouter key: from a file outside every repository (host), or
    typed at tgd's prompt and kept in the data directory. Never printed or logged."""

    def __init__(self, data_dir: Path, api_file: Path | None) -> None:
        self._path = data_dir / "credentials.json"
        self.api_id = 0
        self.api_hash = ""
        self.openrouter_key = ""
        self.source = ""
        stored: dict[str, Any] = {}
        if self._path.exists():
            try:
                stored = json.loads(self._path.read_text(encoding="utf-8"))
            except (OSError, ValueError):
                log.error("credentials.json is unreadable; ignoring it")
        if api_file is not None and api_file.exists():
            values = _read_env_file(api_file)
            stored = {**stored, **{k: v for k, v in {
                "api_id": values.get("TG_API_ID", ""),
                "api_hash": values.get("TG_API_HASH", ""),
                "openrouter_key": values.get("OPENROUTER_API_KEY", ""),
            }.items() if v}}
            self.source = "file"
        try:
            self.api_id = int(stored.get("api_id") or 0)
        except (TypeError, ValueError):
            self.api_id = 0
        self.api_hash = str(stored.get("api_hash") or "")
        self.openrouter_key = str(stored.get("openrouter_key") or "")
        if not self.source and self.api_id:
            self.source = "data"

    @property
    def complete(self) -> bool:
        return bool(self.api_id and self.api_hash)

    def save(self) -> None:
        _write_private(self._path, json.dumps({
            "api_id": self.api_id, "api_hash": self.api_hash,
            "openrouter_key": self.openrouter_key}))


def _read_env_file(path: Path) -> dict[str, str]:
    values: dict[str, str] = {}
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        name, value = line.split("=", 1)
        values[name.strip().removeprefix("export ").strip()] = value.strip().strip("'\"")
    return values


class PromptAuthUI:
    """tgclient's AuthUI as questions a front end answers: each ask_* waits for answer()."""

    def __init__(self, core: Core) -> None:
        self._core = core
        self.kind = ""
        self.info: dict[str, Any] = {}
        self._future: asyncio.Future[str] | None = None

    async def _ask(self, kind: str, info: dict[str, Any]) -> str:
        loop = asyncio.get_running_loop()
        self.kind, self.info = kind, info
        self._future = loop.create_future()
        self._core._auth_changed(f"wait_{kind}", info)
        try:
            return await self._future
        finally:
            self.kind, self.info, self._future = "", {}, None

    def answer(self, kind: str, value: str) -> None:
        if self._future is None or self._future.done():
            raise CoreError(f"nothing is asked now (the state is {self._core.auth_state})")
        if kind != self.kind:
            raise CoreError(f"the {self.kind.replace('_', ' ')} is asked now, not the "
                            f"{kind.replace('_', ' ')}")
        self._future.set_result(value)

    async def ask_phone(self) -> str:
        return await self._ask("phone", {})

    async def ask_code(self, code_info: dict[str, Any]) -> str:
        kind = code_info.get("type") or {}
        return await self._ask("code", {
            "via": kind.get("@type", "").removeprefix("authenticationCodeType"),
            "length": kind.get("length", 0),
            "next": (code_info.get("next_type") or {}).get("@type", "").removeprefix(
                "authenticationCodeType")})

    async def ask_password(self, hint: str) -> str:
        return await self._ask("password", {"hint": hint})

    async def ask_email(self) -> str:
        return await self._ask("email", {})

    async def ask_email_code(self, code_info: dict[str, Any]) -> str:
        return await self._ask("email_code", {})

    async def show_link(self, link: str) -> None:
        self._core.emit({"ev": "auth.link", "note": "confirm on another device"})

    async def show_error(self, message: str) -> None:
        # TDLib's messages here are codes such as PHONE_CODE_INVALID: no secret in them.
        self._core.emit({"ev": "auth.error", "message": message})


class Core:
    def __init__(self, *, data_dir: Path, test_dc: bool, binding: str, tdjson_path: str | None,
                 api_file: Path | None, emit: Emit, td_log_level: int = 1) -> None:
        self.data_dir = data_dir
        self.test_dc = test_dc
        self.binding = binding
        self._tdjson_path = tdjson_path
        self.emit = emit
        self.td_log_level = td_log_level
        self.credentials = Credentials(data_dir, api_file)
        self.tdlib_version = ""
        self.auth_state = "need_credentials"
        self.td_state = ""
        self.me: dict[str, Any] = {}
        self.listed: list[int] = []  # chat ids of the last chats.list, for "N" references
        self.open_chat_id = 0
        self._lib: Any = None
        self._hub: Any = None
        self.client: Any = None
        self._auth_task: asyncio.Task[None] | None = None
        self._auth_cond = asyncio.Condition()
        self._auth_version = 0
        self._ui = PromptAuthUI(self)
        self._history: Any = None
        self._rekey: bytes | None = None
        self.ai: Any = None
        self._vault: Any = None
        self._sent: dict[int, asyncio.Future[dict[str, Any]]] = {}

    # --- start and stop ---------------------------------------------------------------------

    async def start(self) -> None:
        from . import binding  # noqa: PLC0415

        self._lib = binding.load(self.binding, self._tdjson_path)
        self._lib.execute({"@type": "setLogVerbosityLevel",
                           "new_verbosity_level": self.td_log_level})
        self._lib.execute({"@type": "setLogStream", "log_stream": {
            "@type": "logStreamFile", "path": str(self.data_dir / "td.log"),
            "max_file_size": 4 << 20, "redirect_stderr": False}})
        version = self._lib.execute({"@type": "getOption", "name": "version"}) or {}
        self.tdlib_version = str(version.get("value", ""))
        self.emit({"ev": "hello", "tgd": VERSION, "protocol": 1, "tdlib": self.tdlib_version,
                   "binding": self.binding, "test_dc": self.test_dc,
                   "data": str(self.data_dir)})
        if self.credentials.complete:
            self._start_client()
        else:
            self._auth_changed("need_credentials", {})

    def _settings(self) -> Any:
        from tgclient.config import Settings  # noqa: PLC0415

        return Settings(
            api_id=self.credentials.api_id, api_hash=self.credentials.api_hash,
            data_dir=self.data_dir, use_test_dc=self.test_dc, td_log_level=self.td_log_level,
            log_level="WARNING", openrouter_api_key=self.credentials.openrouter_key)

    def _start_client(self) -> None:
        os.environ["TGC_KEY_STORE"] = "file"  # no keyring: the key file in the data directory
        from tgclient.prefs import Prefs  # noqa: PLC0415
        from tgclient.services.ai import AiService  # noqa: PLC0415
        from tgclient.services.ai_store import AiStore  # noqa: PLC0415
        from tgclient.services.openrouter import OpenRouter, mask_key  # noqa: PLC0415
        from tgclient.store.chats import ChatStore  # noqa: PLC0415
        from tgclient.store.files import FileManager  # noqa: PLC0415
        from tgclient.store.users import UserStore  # noqa: PLC0415
        from tgclient.td import AuthFlow, TdHub, TdlibParams  # noqa: PLC0415
        from tgclient.vault import Vault, database_exists  # noqa: PLC0415

        settings = self._settings()
        self.settings = settings
        settings.database_dir.mkdir(parents=True, exist_ok=True)
        settings.files_dir.mkdir(parents=True, exist_ok=True)
        self._hub = TdHub(self._lib)
        self.client = self._hub.create_client()
        # Subscribers before the first request (tgclient's rule: updateNewChat comes once).
        self.files = FileManager(self.client, ACCOUNT)
        self.chats = ChatStore(self.client, self.files)
        self.users = UserStore(self.client, self.files)
        self.client.on("updateAuthorizationState", self._on_td_state)
        self.client.on("updateMessageSendSucceeded", self._on_send_result)
        self.client.on("updateMessageSendFailed", self._on_send_result)
        self.prefs = Prefs(self.data_dir / "prefs.json")
        router = OpenRouter(settings.openrouter_api_key) if settings.openrouter_api_key else None
        self.ai = AiService(
            self.client, self.chats, self.users, AiStore(settings.ai_db_path), router,
            self.prefs.get("summary_model") or settings.summary_model,
            self.prefs.get("transcription_model") or settings.transcription_model,
            key_hint=mask_key(settings.openrouter_api_key) if router else "",
            key_source="settings" if router else "",
            cheap_model=self.prefs.get("cheap_model") or settings.cheap_model,
            monthly_limit=float(self.prefs.get("ai_monthly_limit") or 0.0),
            translate_to=self.prefs.get("ai_language") or "en")
        self.ai.subscribe(self._on_ai)
        params = TdlibParams(
            api_id=settings.api_id, api_hash=settings.api_hash,
            database_dir=settings.database_dir, files_dir=settings.files_dir,
            use_test_dc=settings.use_test_dc, application_version=f"tgd {VERSION}")
        # The database key (tgclient's Session._prepare_database_key).
        self._vault = Vault(self.data_dir, keyring=None)
        key = self._vault.database_key(ACCOUNT)
        if key is None:
            key = self._vault.new_key()
            if database_exists(settings.database_dir):
                self._rekey, key = key, b""
            else:
                self._vault.store_database_key(ACCOUNT, key)
        params = dataclasses.replace(params, database_encryption_key=key)
        self._flow = AuthFlow(self.client, self._ui, params)
        self._auth_task = asyncio.ensure_future(self._run_auth())

    async def _run_auth(self) -> None:
        from tgclient.td import AuthError, TdError  # noqa: PLC0415

        try:
            try:
                await self.client.send({"@type": "getOption", "name": "version"})
            except TdError:
                pass  # only starts the TDLib instance
            await self._flow.run()
        except AuthError as e:
            self._auth_changed("failed", {"message": str(e)})
            return
        if self._rekey is not None:
            new, self._rekey = self._rekey, None
            try:
                await self.client.send({"@type": "setDatabaseEncryptionKey",
                                        "new_encryption_key": base64.b64encode(new).decode()})
                self._vault.store_database_key(ACCOUNT, new)
            except TdError as e:
                log.error("encrypting the old database failed: %s", e.code)
        try:
            me = await self.client.send({"@type": "getMe"})
            self.me = {"id": me.get("id", 0),
                       "name": f"{me.get('first_name', '')} {me.get('last_name', '')}".strip()}
        except TdError:
            self.me = {}
        self._auth_changed("ready", {"me": self.me})
        log.info("authorized")

    def _on_td_state(self, event: dict[str, Any]) -> None:
        name = event["authorization_state"]["@type"]
        self.td_state = name
        log.info("TDLib state %s", name)
        short = AUTH_STATES.get(name, name)
        # The wait_* states are announced by the prompts (with what they need); the others here.
        if not short.startswith("wait_") and short != "ready":
            self._auth_changed(short, {})

    def _auth_changed(self, state: str, info: dict[str, Any]) -> None:
        self.auth_state = state
        self.emit({"ev": "auth.state", "state": state, "td": self.td_state, **info})
        self._auth_version += 1

        async def notify() -> None:
            async with self._auth_cond:
                self._auth_cond.notify_all()

        asyncio.ensure_future(notify())

    async def wait_auth_change(self, seen: int, timeout: float | None = None) -> int:
        """Waits until the authorization state changes after version `seen`."""
        async with self._auth_cond:
            await asyncio.wait_for(
                self._auth_cond.wait_for(lambda: self._auth_version != seen), timeout)
        return self._auth_version

    @property
    def auth_version(self) -> int:
        return self._auth_version

    @property
    def asking(self) -> tuple[str, dict[str, Any]]:
        return self._ui.kind, self._ui.info

    async def close(self) -> None:
        if self._history is not None:
            self._history.dispose()
            self._history = None
        if self._auth_task is not None and not self._auth_task.done():
            self._auth_task.cancel()
        if self.ai is not None:
            try:
                await self.ai.close()
            except Exception:  # noqa: BLE001 - never lose the close over it
                log.exception("closing the AI service")
        if self.client is not None:
            await self.client.close(timeout=15.0)
        if self._hub is not None:
            self._hub.stop()

    # --- authorization ----------------------------------------------------------------------

    def set_credentials(self, api_id: str | int, api_hash: str) -> None:
        if self.client is not None:
            raise CoreError("the credentials are set already (start tgd with a new data "
                            "directory to use others)")
        try:
            number = int(str(api_id).strip())
        except ValueError:
            raise CoreError("the api id is a number") from None
        api_hash = str(api_hash).strip()
        if number <= 0 or len(api_hash) < 16 or not all(
                c in "0123456789abcdef" for c in api_hash.lower()):
            raise CoreError("that is not an api id and hash from my.telegram.org")
        self.credentials.api_id, self.credentials.api_hash = number, api_hash
        self.credentials.save()
        self._start_client()

    def answer(self, kind: str, value: str) -> None:
        if kind not in ASK_KINDS:
            raise CoreError(f"unknown answer {kind!r}")
        self._ui.answer(kind, value)

    def require_ready(self) -> None:
        if self.auth_state != "ready":
            raise CoreError(f"not logged in (the state is {self.auth_state}); log in first")

    # --- chats ------------------------------------------------------------------------------

    def chat_out(self, chat: Any) -> dict[str, Any]:
        return {"id": chat.id, "title": chat.title, "type": chat.type,
                "unread": chat.unread_count, "muted": self.chats.is_muted(chat)}

    async def list_chats(self, limit: int = 20) -> list[Any]:
        from tgclient.store.chats import MAIN  # noqa: PLC0415

        self.require_ready()
        limit = max(1, min(int(limit), 500))
        for _ in range(10):
            if len(self.chats.chats_in(MAIN)) >= limit or self.chats.is_fully_loaded(MAIN):
                break
            before = len(self.chats.chats_in(MAIN))
            await self.chats.load_more(MAIN, limit=max(limit, 20))
            await asyncio.sleep(0.05)  # position updates may trail the answer
            if len(self.chats.chats_in(MAIN)) == before:
                break
        chats = self.chats.chats_in(MAIN)[:limit]
        self.listed = [c.id for c in chats]
        return chats

    async def resolve(self, ref: Any) -> int:
        """A chat from a front end: an id, a number from the last list ("3"), or a part of
        its title that only one loaded chat has."""
        from tgclient.td import TdError  # noqa: PLC0415

        self.require_ready()
        text = str(ref).strip()
        if not text:
            raise CoreError("which chat?")
        if text.casefold() in ("me", "saved") and self.me.get("id"):
            chat = await self.client.send({"@type": "createPrivateChat",
                                           "user_id": self.me["id"], "force": False})
            return int(chat["id"])
        try:
            number = int(text)
        except ValueError:
            number = None
        if number is not None:
            if 0 < number <= len(self.listed) and abs(number) < 10000:
                return self.listed[number - 1]
            if number in self.chats.chats:
                return number
            try:
                await self.client.send({"@type": "getChat", "chat_id": number})
                return number
            except TdError:
                raise CoreError(f"no chat {number}") from None
        if not self.chats.chats:
            await self.list_chats(100)
        needle = text.casefold()
        found = [c for c in self.chats.chats.values() if needle in c.title.casefold()]
        exact = [c for c in found if c.title.casefold() == needle]
        if len(exact) == 1 or len(found) == 1:
            return (exact or found)[0].id
        if not found:
            raise CoreError(f"no loaded chat's title has {text!r}")
        raise CoreError(f"{len(found)} chats match {text!r}; give its number or id")

    def chat(self, chat_id: int) -> Any:
        return self.chats.chats.get(chat_id)

    # --- messages ---------------------------------------------------------------------------

    def sender_name(self, message: dict[str, Any]) -> str:
        sender = message.get("sender_id") or {}
        if sender.get("@type") == "messageSenderUser":
            user = self.users.users.get(sender.get("user_id", 0))
            if user is not None:
                return user.full_name or "?"
            return str(sender.get("user_id", "?"))
        chat = self.chats.chats.get(sender.get("chat_id", 0))
        return chat.title if chat is not None else "?"

    def message_out(self, message: dict[str, Any]) -> dict[str, Any]:
        from tgclient.store.format import content_preview  # noqa: PLC0415

        content = message.get("content") or {}
        if content.get("@type") == "messageText":
            text = (content.get("text") or {}).get("text", "")
            kind = "text"
        else:
            text = content_preview(content)
            kind = content.get("@type", "").removeprefix("message").lower() or "unknown"
        return {"id": message.get("id", 0), "chat_id": message.get("chat_id", 0),
                "date": message.get("date", 0), "out": bool(message.get("is_outgoing")),
                "sender": self.sender_name(message), "kind": kind, "text": text}

    async def history(self, chat_id: int, limit: int = 20) -> list[dict[str, Any]]:
        from tgclient.store.history import ChatHistory  # noqa: PLC0415

        self.require_ready()
        limit = max(1, min(int(limit), 1000))
        history = ChatHistory(self.client, chat_id,
                              last_message_id=self._last_message_id(chat_id))
        try:
            await history.load_initial()
            while len(history.messages) < limit and not history.reached_start:
                before = len(history.messages)
                await history.load_older()
                if len(history.messages) == before:
                    break
            return [self.message_out(m) for m in reversed(history.messages[:limit])]
        finally:
            history.dispose()

    def _last_message_id(self, chat_id: int) -> Callable[[], int]:
        def last() -> int:
            chat = self.chats.chats.get(chat_id)
            return int(((chat.last_message if chat else None) or {}).get("id") or 0)

        return last

    async def open_chat(self, chat_id: int, limit: int = 20) -> list[dict[str, Any]]:
        """The chat whose new messages are sent as events (message.new) until another opens."""
        from tgclient.store.history import ChatHistory  # noqa: PLC0415

        self.require_ready()
        await self.close_chat()
        live = _LiveListener(self)
        history = ChatHistory(self.client, chat_id, listener=live,
                              last_message_id=self._last_message_id(chat_id))
        live.history = history
        self._history, self.open_chat_id = history, chat_id
        await self.client.send({"@type": "openChat", "chat_id": chat_id})
        await history.load_initial()
        live.live = True
        return [self.message_out(m) for m in reversed(history.messages[:max(1, limit)])]

    async def close_chat(self) -> None:
        if self._history is None:
            return
        history, chat_id = self._history, self.open_chat_id
        self._history, self.open_chat_id = None, 0
        history.dispose()
        try:
            await self.client.send({"@type": "closeChat", "chat_id": chat_id})
        except Exception:  # noqa: BLE001
            pass

    async def send_text(self, chat_id: int, text: str) -> dict[str, Any]:
        from tgclient.td import TdError  # noqa: PLC0415

        self.require_ready()
        if not text.strip():
            raise CoreError("nothing to send")
        try:
            pending = await self.client.send({
                "@type": "sendMessage", "chat_id": chat_id,
                "input_message_content": {
                    "@type": "inputMessageText",
                    "text": {"@type": "formattedText", "text": text, "entities": []}}})
        except TdError as e:
            raise CoreError(f"Telegram: {e.message}") from None
        future: asyncio.Future[dict[str, Any]] = asyncio.get_running_loop().create_future()
        self._sent[pending["id"]] = future
        if self._history is not None and self.open_chat_id == chat_id:
            self._history.add(pending)
        try:
            result = await asyncio.wait_for(future, 60)
        except TimeoutError:
            return {"id": pending["id"], "state": "pending"}
        finally:
            self._sent.pop(pending["id"], None)
        if result["@type"] == "updateMessageSendFailed":
            error = result.get("error") or {}
            raise CoreError(f"not sent: {error.get('message', result.get('error_message', '?'))}")
        return {"id": result["message"]["id"], "state": "sent"}

    def _on_send_result(self, event: dict[str, Any]) -> None:
        future = self._sent.get(event.get("old_message_id", 0))
        if future is not None and not future.done():
            future.set_result(event)

    # --- AI ---------------------------------------------------------------------------------

    def set_ai_key(self, key: str) -> None:
        from tgclient.services.openrouter import OpenRouter, mask_key  # noqa: PLC0415

        key = key.strip()
        if not key:
            raise CoreError("no key given")
        self.credentials.openrouter_key = key
        if self.credentials.complete:
            self.credentials.save()
        if self.ai is not None:
            self.ai.use_router(OpenRouter(key), mask_key(key), "settings")

    def ai_enable(self, chat_id: int, on: bool) -> bool:
        self.require_ready()
        return bool(self.ai.set_enabled(chat_id, on))

    async def summarize(self, chat_id: int, scope: str = "day",
                        timeout: float = 240.0) -> dict[str, Any]:
        from tgclient.services import summary as summaries  # noqa: PLC0415

        self.require_ready()
        if scope not in summaries.SCOPES:
            raise CoreError(f"the scope is one of {', '.join(summaries.SCOPES)}")
        if not self.ai.configured:
            raise CoreError("no OpenRouter key: type one with `aikey` first")
        if not self.ai.is_enabled(chat_id):
            raise CoreError("AI is off for this chat (tgclient's rule); turn it on with "
                            "`ai CHAT on`" if self.ai.allowed(chat_id)
                            else "AI is never used in secret chats")
        started = time.monotonic()
        self.ai.summarize(chat_id, scope)
        while True:
            state = self.ai.summary(chat_id)
            if state.state in ("done", "error"):
                break
            if time.monotonic() - started > timeout:
                raise CoreError("the summary did not arrive in time")
            await asyncio.sleep(0.25)
        if state.state == "error":
            raise CoreError(state.error)
        return {"scope": state.scope, "text": state.text, "count": state.count,
                "cost": state.cost, "truncated": state.truncated}

    def _on_ai(self, kind: str, payload: Any) -> None:
        if kind == "summary":
            self.emit({"ev": "ai.change", "kind": kind})

    # --- other ------------------------------------------------------------------------------

    def status(self) -> dict[str, Any]:
        return {"auth": self.auth_state, "td": self.td_state, "me": self.me,
                "tdlib": self.tdlib_version, "binding": self.binding, "test_dc": self.test_dc,
                "credentials": self.credentials.complete,
                "ai_key": bool(self.credentials.openrouter_key),
                "open_chat": self.open_chat_id, "max_rss_kib": max_rss_kib()}


def max_rss_kib() -> int:
    """The process's peak resident memory in KiB (0 when the platform does not say)."""
    try:
        import resource  # noqa: PLC0415

        rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
    except Exception:  # noqa: BLE001
        return 0
    return rss // 1024 if sys.platform == "darwin" else rss  # bytes on macOS, KiB elsewhere


class _LiveListener:
    """tgclient's HistoryListener for the open chat: new messages at the top become events."""

    def __init__(self, core: Core) -> None:
        self._core = core
        self.history: Any = None
        self.live = False

    def history_insert(self, row: int, count: int, commit: Callable[[], None]) -> None:
        commit()
        if self.live and row == 0 and self.history is not None:
            for message in reversed(self.history.messages[:count]):
                self._core.emit({"ev": "message.new", "message": self._core.message_out(message)})

    def history_remove(self, row: int, count: int, commit: Callable[[], None]) -> None:
        commit()

    def history_changed(self, row: int) -> None:
        pass

    def history_reset(self, commit: Callable[[], None]) -> None:
        commit()
