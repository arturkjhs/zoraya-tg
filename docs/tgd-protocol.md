# tgd's line protocol (protocol 1)

`tgd --json` reads one JSON object per line on standard input and writes one per line on standard output. Lines
are UTF-8, at most 64 KiB. Requests run concurrently: each gets exactly one reply, matched by `id`, and events
come in between at any time. This is what a front end (`tg-term`, a GUI) speaks; `tgd` without `--json` is the
text front end over the same dispatcher.

## Requests and replies

A request is `{"op": NAME, "id": ANY, ...fields}`. The reply is `{"re": ID, "ok": true, ...fields}`, or
`{"re": ID, "ok": false, "error": TEXT}`. A line that is not a JSON object gets `{"re": null, "ok": false, ...}`.

| op | fields | reply fields |
|---|---|---|
| `version` | | `tgd`, `protocol`, `tdlib` |
| `status` | | `auth`, `td` (TDLib's state), `me`, `tdlib`, `binding`, `test_dc`, `credentials`, `ai_key`, `open_chat`, `max_rss_kib` (0 where the system does not say) |
| `auth.credentials` | `api_id`, `api_hash` | (kept in the data directory, mode 0600; TDLib starts) |
| `auth.phone`, `auth.code`, `auth.password`, `auth.email`, `auth.email_code` | `value` | (refused unless that is what is asked now) |
| `chats.list` | `limit` (20) | `chats`: `[{id, title, type, unread, muted}]` |
| `chat.open` | `chat`, `limit` (20) | `chat`, `messages`; then `message.new` events for that chat |
| `chat.close` | | |
| `messages.history` | `chat`, `limit` (20, at most 1000) | `chat_id`, `messages` (the newest `limit`, oldest first) |
| `messages.send` | `chat`, `text` | `chat_id`, `id`, `state` (`sent`, or `pending` if Telegram has not confirmed in time) |
| `ai.key` | `key` | (the OpenRouter key, kept in the data directory) |
| `ai.enable` | `chat`, `on` | `chat_id`, `on` (never for secret chats) |
| `ai.summarize` | `chat`, `scope` (`unread`, `day`, `week`) | `chat_id`, `scope`, `text`, `count`, ... |
| `quit` | | then `bye`, and tgd exits 0 |

`chat` is a number from the last `chats.list` (1, 2, ...), a chat id, part of a title, or `me` / `saved`
(Saved Messages). A message is `{id, chat_id, date, out, sender, kind, text}`. For a kind other than `text`,
`text` is tgclient's one-line preview.

## Events

| ev | fields |
|---|---|
| `hello` | `tgd`, `protocol`, `tdlib`, `binding` (`ctypes` or `builtin`), `test_dc`, `data`; always first |
| `auth.state` | `state`, `td`, and for the `wait_*` states what is asked: `wait_code` has `via`, `length`, `next`; `wait_password` has `hint`; `ready` has `me` |
| `auth.error` | `message`: Telegram's code, such as `PHONE_CODE_INVALID` |
| `auth.link` | a QR login waits for another device |
| `message.new` | `message`, in the open chat |
| `ai.change` | `kind` |
| `refused` | `reason` (`network`, `data`, `lock`), `detail`, and for `network` the `grant` line that starts tgd; then tgd exits 3, 4 or 5 |
| `bye` | `td`; last |

`state` is one of `need_credentials`, `starting`, `wait_phone`, `wait_code`, `wait_password`, `wait_email`,
`wait_email_code`, `wait_other_device`, `wait_registration`, `ready`, `failed` (with `message`), `logging_out`, `closing`, `closed`.

## What never crosses the protocol outward

A credential, a code, a password or a key is never echoed in a reply or an event, and none of them, nor any
message text, is written to `tgd.log`.
