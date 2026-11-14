# TG-01: tgclient on Zoraya — analysis

Sources: `~/workspace/tgclient` at `ccba758` (2026-10-10; read-only, working-tree edits of the
architect not read beyond code), its `CLAUDE.md` (architecture notes, in Russian) and
`docs/FEATURE_GAPS.md`; TDLib source in `tgclient/vendor/td` (pinned commit
`42e6a525…`, TDLib 1.8.67); Zoraya `origin/main` at `0c3ada9` (2026-11-14).

**Not read (secrets rules):** `tgclient/.env`, `tgclient/url.txt`, `.env.example` (modified,
may carry values), anything under `vendor/tdlib*/`, `dist/`, `build/`, `.flatpak-builder/`,
the user data directory (`td_db`, `td_files`, `*.sqlite3`, `vault.json`, `accounts.json`,
`prefs.json`), the screenshot in the repository root, logs. Configuration *values* in
`config.py` (model ids, app id) were skipped; only its structure was read.

---

## 1. tgclient's structure

### 1.1 Size

| Part | Files | Lines | Qt? |
|---|---:|---:|---|
| `td/` — TDLib binding, async client, login flow | 4 | 473 | no |
| `store/` — in-memory state fed by TDLib updates | 21 | 3 551 | no |
| `services/` — AI, search, update checker | 10 | 4 549 | no |
| `accounts.py`, `config.py`, `prefs.py`, `vault.py`, `cli.py` | 5 | 735 | no |
| **Qt-free Python** | **40** | **9 199** (46 %) | |
| `models/` — `QAbstractListModel`/`QObject` view-models for QML | 15 | 5 672 | yes |
| `ui/*.py` — controllers, image providers, media, notifications | 20 | 4 230 | yes |
| `app.py`, `selftest.py`, `devbundle.py` | 3 | 867 | yes |
| **Qt Python** | **38** | **10 769** (54 %) | |
| **All Python** | 78 | **19 968** | |
| QML (`ui/qml/TgClient/*.qml`) | 46 | 11 849 | — |
| Tests (`tests/`, on a fake TDLib) | 42 | 9 121 | partly |

The split is not accidental: `CLAUDE.md` makes it an invariant that `td/`, `store/` and
`services/` never import Qt, that all TDLib traffic goes through `TdClient.send()`/`.on()`,
and that update handlers run on the asyncio loop with no blocking I/O. That invariant holds
in the code (checked by grep: no `PySide6`/`qasync` import in any of those 40 files).

### 1.2 Layers

- **Logic, Qt-free (moves as is).**
  - `td/client.py` — `TdHub` owns the one `td_receive` thread and routes events by
    `@client_id` (multi-account) to `TdClient`s; requests are matched to responses by
    `@extra` into asyncio futures (`loop.call_soon_threadsafe`).
  - `td/auth.py` — `AuthFlow` driving `authorizationState*` through an `AuthUI` protocol
    (console implementation in `cli.py`, QML one in `ui/auth_controller.py`).
  - `store/` — `chats.py` (chat list and positions), `history.py` (the open chat's window of
    messages, paging, live updates; it reports changes through a `HistoryListener` with
    `commit()`), `files.py` (`FileManager`, single source of truth for TDLib file state),
    `media.py`, `presence.py`, `reactions.py`, `forums.py`, `pinned.py`, `polls.py`,
    `keyboards.py`, `link_preview.py`, `custom_emoji.py`, `stickers.py`, `emoji.py`,
    `album.py` (grid layout of albums), `users.py`, `notifications.py` (TDLib notification
    groups → `Notice` → a `NotificationSink`), `format.py`, `links.py`.
  - `store/richtext.py`, `store/markdown.py` — `formattedText` / LLM markdown → **HTML for Qt
    rich text**. The UTF-16 offset mapping and entity handling are logic; the HTML output is
    Qt-shaped (see §1.3).
  - `services/` — `ai.py` (`AiService`, the access rules: per-chat switch, never secret
    chats), `openrouter.py` (httpx client, streaming), `ai_store.py` (SQLite), `summary.py`,
    `assist.py` (prompts and parsers: translation, questions, events/.ics, replies, digest),
    `person_search.py`, `search.py` + `search_index.py` (hybrid search: SQLite FTS5 +
    vectors in numpy), `embeddings.py` (fastembed/ONNX, optional), `updates.py` (GitHub
    Releases self-update of the AppImage).
  - `vault.py` — keys: the TDLib database key in the system keyring or a file; a passcode
    wraps them (`hashlib.scrypt` → AES-GCM from `cryptography`); sealed AI files.
- **View-models, Qt-shaped logic (`models/`, most of `ui/*.py`).** Each is a `QObject` or
  `QAbstractListModel` with `Signal`s, `@Slot`s and `@Property`s. The method bodies are
  logic (which TDLib request to send, what to cache, selection, search-in-chat stepping,
  jump-to-message, the unread separator, composer drafts and mentions); the Qt part is the
  decorators, `begin/endInsertRows`-style row notifications, `QVariant` conversions and a
  few Qt utilities. `models/messages.py` (2 511 lines, one class, 129 Qt decorators/signals,
  ~190 methods) is the heart of the chat view. `ui/ai_controller.py` (734) is the AI
  panel's controller.
- **Glue and Qt-only code.** `app.py` (`Session`: one TDLib client + stores + models; QML
  engine; `qasync` loop), `selftest.py`, `devbundle.py` (macOS dev bundle); image providers
  (`ui/images.py`, `ui/icons.py` SVG, `ui/qr.py`); media (`ui/animation.py` TGS/WebM frames,
  `ui/recorder.py` microphone → Ogg/Opus, `ui/voice_player.py`, `ui/video_note.py`);
  `ui/notifications.py` (macOS UserNotifications, Linux D-Bus); `ui/updates.py`. And all of
  the QML.

### 1.3 Where Qt reaches into the logic

| Qt feature | Where | What replaces it in `tgd` |
|---|---|---|
| `Signal`/`@Slot`/`@Property` | all 15 `models/`, 13 controllers in `ui/` | an event bus: `emit(name, payload)` → JSON message to the front end; slots become request handlers; properties become a state snapshot + change events |
| `QAbstractListModel` row signals | chat list, messages, search, stickers, gifs, topics, folders, emoji, chat/sender pickers, person messages | list deltas (`insert`/`remove`/`move`/`change` with ids), driven by the existing `HistoryListener`/`ChatStore.subscribe()` callbacks |
| `QThread` | **none** — the TDLib thread is a plain `threading.Thread`; blocking work uses `asyncio.to_thread` | unchanged |
| `qasync` (Qt event loop as asyncio loop) | `app.py`, `selftest.py` | plain `asyncio.run()` |
| `QTimer` | `models/search.py` (debounce), `ui/lock.py` (auto-lock), `ui/updates.py`, `ui/recorder.py`, `ui/animation.py` | `loop.call_later` / `asyncio.sleep` |
| `QSettings` | **none** — preferences are `prefs.json`, accounts `accounts.json`, user settings a `.env` via `python-dotenv`, paths from `platformdirs` | unchanged (paths from the program's own directory) |
| `QNetworkAccessManager` | **none** — HTTP is `httpx` | unchanged (needs `ssl`) |
| `QImage`/`QImageReader`/`QPainter` | `ui/images.py` (avatars cropped round, thumbnails), `ui/icons.py`, `ui/qr.py`, `ui/animation.py`, `ui/video_note.py`, `devbundle.py` | front end: image decoding is a screen job; `tgd` only hands out file paths |
| `QFontDatabase` | `models/messages.py` (monospace family for `code`), `ui/ai_controller.py` | a constant / front-end side |
| `QGuiApplication` clipboard, `QDesktopServices.openUrl`, `QStandardPaths.DownloadLocation`, `QTranslator`, `QLocale` | composer paste, links, "Save as", i18n | front end (clipboard, open link, file picker, translations) |
| Qt rich-text HTML | `store/richtext.py`, `store/markdown.py`, `models/search.py` snippets, spoilers as `tgc://spoiler/<id>` links, invisible time-stamp tail | `tgd` sends **text + spans** (`bold`, `italic`, `code`, `pre`, `link`, `mention`, `spoiler`, `custom_emoji`, `highlight`, colour role) in code points; the existing UTF-16 map and entity logic stays; only the HTML emitter is replaced |
| QtMultimedia (`QMediaPlayer`, `QAudioOutput`, `QAudioSource`, QML `MediaPlayer`/`VideoOutput`) | voice playback, recording, video and GIF playback | front end, later (Zoraya has no audio or video yet; see §3) |

### 1.4 TDLib binding

- `td/tdjson.py` (88 lines) loads `libtdjson` with **`ctypes`** and uses only the four
  C functions of the JSON interface: `td_create_client_id`, `td_send(int, char*)`,
  `td_receive(double) → char*`, `td_execute(char*) → char*`. JSON is encoded/decoded in
  Python (`json`).
- One receiver thread (`td-receive`, daemon) loops `td_receive(1.0)` and posts each event to
  the asyncio loop with `call_soon_threadsafe`. Requests go out from the loop thread with
  `td_send` (thread-safe in TDLib). `td_execute` is used for `setLogVerbosityLevel` (and in
  the self-test for `parseMarkdown`/`getOption`).
- `TdLib` is already a `Protocol`, and the tests use a fake; a built-in `_tdjson` module
  exposing the same four functions (`create_client_id() → int`, `send(int, bytes)`,
  `receive(float) → bytes|None` releasing the GIL, `execute(bytes) → bytes|None`) replaces
  `tdjson.py` with a ~30-line wrapper and touches nothing else.
- TDLib parameters: `setTdlibParameters` with file, chat-info and message databases on,
  secret chats on, `database_directory = <account>/td_db`, `files_directory =
  <account>/td_files`, a database encryption key from `vault.py`.

**TDLib methods used** (≈ 115, collected from string literals; the full set):
`addChatMembers addChatToList addContact addMessageReaction addProxy addSavedAnimation
banChatMember cancelDownloadFile checkAuthenticationCode checkAuthenticationEmailCode
checkAuthenticationPassword checkChatInviteLink close closeChat createChatFolder
createNewBasicGroupChat createNewSupergroupChat createPrivateChat deleteChat deleteChatFolder
deleteChatHistory deleteMessages disableProxy downloadFile editChatFolder editMessageCaption
editMessageSchedulingState editMessageText enableProxy forwardMessages getActiveSessions
getBasicGroupFullInfo getCallbackQueryAnswer getChatFolder getChatHistory
getChatPinnedMessage getChatScheduledMessages getContacts getCustomEmojiStickers
getForumTopic getForumTopicHistory getForumTopics getGroupsInCommon getInlineQueryResults
getInstalledStickerSets getInternalLinkType getLinkPreview getMarkdownText getMe getMessage
getMessageAddedReactions getMessageAvailableReactions getMessageLink getMessageLinkInfo
getMessageProperties getMessageThread getMessageThreadHistory getMessages getOption
getProxies getRecentStickers getRepliedMessage getSavedAnimations
getScopeNotificationSettings getStickerSet getStorageStatisticsFast getSupergroupFullInfo
getUserFullInfo getUserPrivacySettingRules importContacts joinChat joinChatByInviteLink
leaveChat loadChats logOut markChecklistTasksAsDone openChat openMessageContent
optimizeStorage parseMarkdown pinChatMessage pingProxy readAllChatMentions
readAllChatReactions readAllForumTopicMentions readAllForumTopicReactions recognizeSpeech
removeContacts removeMessageReaction removeProxy reorderChatFolders
replacePrimaryChatInviteLink requestQrCodeAuthentication searchChatMembers
searchChatMessages searchContacts searchMessages searchPublicChat searchUserByPhoneNumber
sendChatAction sendMessage setAuthenticationEmailAddress setAuthenticationPhoneNumber
setChatDescription setChatDraftMessage setChatMemberStatus setChatNotificationSettings
setChatTitle setDatabaseEncryptionKey setLogVerbosityLevel setOption setPollAnswer
setScopeNotificationSettings setTdlibParameters setUserPrivacySettingRules
terminateAllOtherSessions terminateSession toggleChatIsMarkedAsUnread toggleChatIsPinned
unpinAllChatMessages unpinChatMessage viewMessages`.

**Updates handled** (43): `updateAuthorizationState updateBasicGroup updateChatAction
updateChatDraftMessage updateChatFolders updateChatHasScheduledMessages
updateChatIsMarkedAsUnread updateChatLastMessage updateChatNotificationSettings
updateChatOnlineMemberCount updateChatPhoto updateChatPosition updateChatReadInbox
updateChatReadOutbox updateChatReplyMarkup updateChatTitle updateChatUnreadMentionCount
updateChatUnreadReactionCount updateDeleteMessages updateFile updateForumTopic
updateForumTopicInfo updateInstalledStickerSets updateMessageContent updateMessageEdited
updateMessageInteractionInfo updateMessageIsPinned updateMessageMentionRead
updateMessageSendFailed updateMessageSendSucceeded updateMessageUnreadReactions
updateNewChat updateNewMessage updateNotificationGroup updateOption updatePoll
updateRecentStickers updateScopeNotificationSettings updateSupergroup updateUnreadChatCount
updateUnreadMessageCount updateUser updateUserStatus`.

All of these are handled in `store/` or `services/` (Qt-free), except that the models
subscribe to the stores. **None of the TDLib traffic changes in the port.**

### 1.5 Dependencies besides Qt

Versions from `uv.lock`.

| Package | Version | Pure Python? | Used for | On Zoraya |
|---|---|---|---|---|
| httpx (+ httpcore 1.0.9, h11 0.16.0, anyio 4.15.1, sniffio, idna 3.20, certifi 2026.7.22) | 0.28.1 | yes | OpenRouter, update check | keep; needs `ssl` and a CA bundle (certifi's file or Zoraya's trust store) |
| platformdirs | 4.12.2 | yes | data/config dirs | replace by the program's own directory (two call sites) |
| python-dotenv | 1.2.4 | yes | user settings file (`.env` in the config dir) | keep, or fold into `prefs.json` |
| numpy | 2.5.3 | **no** (C) | vectors for semantic search; PCM levels in recorder/video note | drop for the first version: keyword search (FTS5) works without vectors; make the import optional (5 files) |
| fastembed (+ onnxruntime 1.30.0, tokenizers 0.23.2, huggingface-hub) | 0.8.1 | **no** (C++/Rust) | local embeddings (optional extra) | drop (already optional) |
| cryptography (+ cffi 2.1.1) | 50.0.2 | **no** (Rust + C) | AES-GCM in `vault.py` (2 calls) | replace with a few lines over OpenSSL, which is linked into `python3` anyway for TDLib and `ssl` (built-in `_aesgcm`, or an `hmac`/`hashlib` construction) |
| keyring (+ jaraco.*) | 25.7.0 | yes, but needs an OS backend | DB key storage | no backend on Zoraya → the existing `TGC_KEY_STORE=file` path (key file in the program's own directory, 0600), optionally passcode-wrapped |
| av (PyAV, FFmpeg + libopus + libvpx) | 19.0.0 | **no** | Opus encode, WebM/VP9 decode, MP4 encode | front end, later |
| rlottie-python | 1.3.8 | **no** (C++) | TGS stickers | front end, later |
| segno | 1.6.6 | yes | QR code for QR login | keep in `tgd` (produce the matrix, send it as a bit grid) or do it in Rust |
| jeepney / pyobjc-framework-UserNotifications | 0.9.0 / 12.2.2 | — | system notifications | not applicable |
| qasync | 0.28.0 | yes | Qt-asyncio bridge | not needed |
| PySide6 | 6.11.2 | no | UI | removed |

Standard library needed in `tgd`: `asyncio`, `threading`, `json`, `sqlite3` (with **FTS5**),
`ssl`, `hashlib` (with `scrypt`, i.e. built against OpenSSL), `hmac`, `secrets`, `base64`,
`zlib` (httpx content decoding), `re`, `datetime`, `dataclasses`, `enum`, `pathlib`,
`tempfile`, `shutil`, `html`, `urllib.parse`, `logging`, `bisect`, `weakref`, `importlib`.
Not needed after the port: `ctypes`, `subprocess` (only `devbundle.py`), `plistlib`.

### 1.6 Files on disk (by kind)

Under the data directory (`TG_DATA_DIR`, else `platformdirs.user_data_dir`):
- per account and per DC kind (`<account>/prod` or `/test`):
  - `td_db/` — TDLib's databases (SQLite + binlog, encrypted with the database key);
  - `td_files/` — downloaded media and thumbnails (TDLib manages names and eviction);
  - `ai.sqlite3` — per-chat AI switches, transcripts, summaries (sealed with a passcode);
  - `search.sqlite3` — search tokens and vectors (no message text; disposable);
- `accounts.json` (account registry), `prefs.json` (UI preferences), `vault.json` (wrapped
  keys), `recent-emoji.json`, `models/` (embedding model, ~220 MB, optional), a `pasted/`
  directory for clipboard images, a `voice/` directory for recordings.
- Config directory: one `.env` with user settings (mode 0600).
- TDLib's log verbosity is set from `TD_LOG_LEVEL` (default 1) through `td_execute`;
  Python logs go to stderr.

Write pattern: TDLib holds an `fcntl(F_SETLK)` lock on its database files
(`tdutils/td/utils/port/FileFd.cpp`), writes a binlog with frequent `fsync`, and opens its
bundled SQLite (with FTS5) in **WAL mode** (`tddb/td/db/SqliteConnectionSafe.cpp`:
`PRAGMA journal_mode=WAL`). tgclient's own `ai.sqlite3` and `search.sqlite3` also use WAL,
and the search index uses FTS5 `contentless_delete=1` (SQLite ≥ 3.43). WAL keeps its index
in a `-shm` file mapped with `mmap(MAP_SHARED)` — unless the connection is in
`locking_mode=EXCLUSIVE`, where SQLite keeps it on the heap (see §3). Media under
`td_files/` grows without bound until `optimizeStorage` trims it.

### 1.7 Network peers

| Peer | Host / address | Port | Who connects |
|---|---|---|---|
| Telegram data centres (MTProto over TCP; TLS-like obfuscation only through proxies) | **IP addresses**: TDLib ships fixed addresses for DC 1–5 and then receives the current list from the server (`updateOption`/`config`); the published ranges are `149.154.160.0/20`, `91.108.4.0/22`, `91.108.8.0/22`, `91.108.12.0/22`, `91.108.16.0/22`, `91.108.56.0/22`, `95.161.64.0/20`, `185.76.151.0/24` and IPv6 `2001:b28:f23d::/48`, `2001:b28:f23f::/48`, `2001:67c:4e8::/48`, `2a0a:f280::/32` (media DCs and CDN DCs come inside these or are announced by the server) | 443, 80, 5222 (TDLib tries all three) | `tgd` (TDLib) |
| TDLib's fallback config (only when the DCs are unreachable) | `dns.google` / `mozilla.cloudflare-dns.com` (DNS-over-HTTPS), `firebaseremoteconfig.googleapis.com`, `firestore.googleapis.com`, `reserve-5a846.firebaseio.com`, `tcdnb.azureedge.net`, `*.stel.com` | 443 | `tgd` (TDLib); optional — can be left out of the grant |
| User-configured proxies (SOCKS5, HTTP, MTProto) | any | any | `tgd`; only if the user adds one |
| OpenRouter (LLM, transcription) | `openrouter.ai` | 443 (HTTPS) | `tgd` (httpx), only with AI turned on |
| Embedding model download | Hugging Face hosts | 443 | dropped with fastembed |
| Self-update | `api.github.com` (+ release asset hosts) | 443 | dropped on Zoraya |

No UDP (no calls), no listening sockets. Name resolution is needed only for OpenRouter
(and proxies given by name); Telegram itself is reached by address.

---

## 2. What the UI does — the list the native front end must cover

Taken from the 46 QML files, the controllers they bind to and `CLAUDE.md` (milestones
M1–M9). Marked **[core]** for a first usable client, **[later]** otherwise.

### 2.1 Login and accounts
- [core] Phone number → code (SMS/app) → 2FA password → email code / email address →
  registration (first and last name) (`AuthView.qml`, `td/auth.py`).
- [later] QR login: render a QR of the `tg://login?token=` link, wait for confirmation on
  another device (`ui/qr.py`, segno).
- [later] Several accounts: switcher in the sidebar, add / cancel / log out; badges and
  notifications from all (`AccountSwitcher.qml`, `ui/accounts.py`).
- [later] Passcode lock screen and auto-lock after idle (`LockScreen.qml`, `ui/lock.py`).

### 2.2 Chat list (sidebar)
- [core] Chats sorted by TDLib position, with avatar (round, from a small JPEG; letter tile
  until loaded), title, last message preview ("Draft:" prefix, sender prefix in groups),
  time, unread counter, muted style, mention "@" and reaction badges, pinned marker, online
  dot, "typing…" in place of the preview, marked-as-unread dot.
- [core] Folder tabs (TDLib folders + "All chats") with unread counts; archive as a header
  row, opening into the archive list.
- [later] Folder editor dialog (name, chat types, included chats, reorder).
- [core] Right-click chat menu: pin, mute (1 h / 8 h / 2 days / forever), archive, mark
  read/unread, clear history, leave/delete (with "also for …").
- [later] New chat / new group / new channel (`NewChatDialog.qml`), contacts dialog (list,
  search, add by phone).
- [core] Smooth moves when chats reorder are nice-to-have; correct order is required.

### 2.3 Chat view
- [core] Header: title, status (online / last seen / members / "typing…"), search button,
  AI toggle, profile on click.
- [core] Message feed, newest at the bottom, paging older history upward without jumps;
  "jump to bottom" button with unread count; "Unread messages" separator and opening at the
  first unread; day separators; grouping of consecutive messages of one sender with avatar
  on the last; sender names coloured; outgoing state (pending / sent / read ticks, failed).
- [core] Jump to a message (reply quote click, search result, link) with highlight flash,
  loading history around it.
- [core] Pinned bar under the header (cycles through pinned messages).
- [later] Forums: the chat opens as a list of topics, then a topic's history
  (`TopicList.qml`); channel comments open the discussion thread.
- [core] Text selection and copy within a bubble; multi-select of messages
  (Ctrl/Shift-click) with Copy / Forward / Delete bar.
- [core] Message context menu: reply, edit (own), delete (for me / for all), forward (chat
  picker), copy text, copy link, pin/unpin, reactions; double-click to reply.
- [later] Scheduled messages list (`ScheduledList.qml`), send silently / schedule dialog.
- [later] Bot keyboards: inline buttons under a message (URL, callback with toast/alert,
  copy text), reply keyboard above the composer.

### 2.4 Message kinds
- [core] **Text** with entities: bold, italic, underline, strikethrough, spoiler (hidden
  until clicked), code, pre / pre with language, block quote and expandable block quote,
  URL / text URL, mention / mention-name, hashtag, cashtag, bot command, e-mail, phone.
  Offsets are UTF-16 (handled in `store/richtext.py`).
- [core] **Replies** (quoted sender + snippet, click to jump) and **forwards** ("Forwarded
  from" user / chat / channel / hidden user).
- [core] **Photos**: JPEG; minithumbnail (tiny JPEG, blurred) as a placeholder, then the
  thumbnail, then the full size; albums laid out as a grid (`store/album.py`); captions.
- [core] **Documents**: icon, name, size, download progress, open / "Save as…".
- [core] **Service messages**: chat created, members added/removed, joined by link/request,
  title/photo changed, pinned, screenshot, contact registered, forum topic events,
  premium gift, custom service text, calls (as a line), expired photo/video.
- [core] **Stickers** (static): WebP. [later] **animated**: TGS (gzip-compressed Lottie
  JSON, rendered by rlottie at ≤30 fps) and **video stickers**: WebM VP9 with alpha.
  Custom emoji in text and reactions use the same three formats.
- [later] **Animated emoji**, **dice** (shown as the emoji first).
- [later] **GIFs / animations**: MP4 (H.264), muted, looping; saved GIFs picker.
- [later] **Video**: MP4 (H.264, sometimes H.265); first a poster (thumbnail) + duration,
  click opens in the viewer. **Video notes** (round MP4) likewise; recording them is out of
  scope.
- [later] **Voice notes**: Ogg/Opus; waveform from TDLib, play/pause; transcription
  (AI or TDLib `recognizeSpeech`). Recording: microphone → Opus.
- [later] **Audio** (music files): title/performer, play.
- [core] **Polls / quizzes / checklists**: options, vote, results bars; [later] creating
  them (`PollEditor.qml`).
- [core] **Contact**, **location**, **venue** as cards (location without a map first).
- [core] **Link previews**: site, title, description, image (small or large).
- [core] **Reactions** under messages (emoji, custom emoji, paid), counts, who reacted.
- [core] Edited mark, views/comments counter on channel posts.

### 2.5 Composer (input)
- [core] Multi-line input, Enter sends, Shift+Enter newline; markdown-ish input parsed by
  TDLib `parseMarkdown` (`**bold**`, `__italic__`, `` `code` ``, ```` ```pre``` ````,
  `~~strike~~`, `||spoiler||`, `[text](url)`).
- [core] Reply / edit bar above the input; Up in an empty field edits the last own message;
  Esc cancels.
- [core] Cloud drafts (save on switching chats, restore).
- [core] Own "typing…" chat action.
- [core] Emoji picker (Unicode emoji with recent) — [later] stickers, GIFs and custom emoji
  tabs (`EmojiStickerPicker.qml`).
- [core] @-mentions popup in groups (`searchChatMembers`).
- [core] Attach files (file picker), send as document or as photo; [later] paste image from
  clipboard, drag and drop, "send files" dialog with caption and grouping
  (`SendFilesDialog.qml`).
- [later] Link preview in the composer (with "×" to disable).
- [later] Voice recording button.

### 2.6 Search
- [core] Global search in the sidebar (Ctrl+F without a chat): chats by title, public
  chats, then messages (server `searchMessages` + local FTS5 index), sections, snippets with
  highlighted matches; click jumps to the message.
- [core] In-chat search (Ctrl+F in a chat): step through results older/newer, "From:"
  sender filter (`SenderChip.qml`, `models/sender_picker.py`).
- [later] "Found by meaning" (semantic search) — needs embeddings, dropped for now.
- [later] Messages of one person (in a chat / in all common chats) panel
  (`PersonMessagesPanel.qml`).

### 2.7 AI screens (all through `AiService`; OpenRouter, zero-data-retention providers only)
- [core-AI] Per-chat AI on/off in the header with a confirmation explaining what is sent
  where; never for secret chats.
- [core-AI] Summary side panel (`SummaryPanel.qml`): unread / 24 h / 7 days, streaming text
  with markdown, time links back to source messages (click jumps), cost shown, per-chat
  monthly limit.
- [core-AI] Questions to the chat history, "dates and meetings" with "Add to calendar"
  (.ics), collected answers to a message (table + "No answer"), questions to a PDF/text
  document.
- [later-AI] Summary of one person; translation of incoming messages under the text and of
  own text before sending; reply suggestions (4 tones) in the composer; "Digest / What did I
  promise" window across chats (`DigestDialog.qml`); smart notifications.
- [core-AI] Voice transcription on click (sends that one audio file).

### 2.8 Profile, settings, other dialogs
- [core] Profile panel (`ProfilePanel.qml`): photo, name, username, bio, phone if visible;
  groups in common; for groups: description, members (owner/admins first), invite link;
  tabs Media / Files / Links / Voice.
- [later] Group admin: create, add members, edit title/description, promote/demote/remove,
  delete (`ui/group_admin.py`).
- [core] Media viewer (`MediaViewer.qml`): full-window photo with zoom and pan, ←/→
  through the chat's media, caption, save; [later] video playback.
- [core] Settings (`SettingsDialog.qml`, 1 225 lines) sections: Appearance (theme
  system/light/dark, language en/ru/uk/cs), Notifications (on/off, preview, sound, test),
  AI (OpenRouter key, models, language of answers, monthly limit, chats with AI on, "what
  is sent, and where", delete AI data, search by meaning), Privacy (last seen, phone, etc.:
  All / Contacts / Nobody keeping exceptions), Passcode and auto-lock, Devices (active
  sessions, end one / all others), Proxy (add, enable, ping, remove; `tg://proxy` links),
  Storage (cache size, clear media cache), Updates (dropped on Zoraya), version.
- [core] Confirm dialogs, toasts, tooltips; invite-link and proxy-link confirmations;
  `t.me`/`tg://` internal links opened in-app (`getInternalLinkType`).

### 2.9 Notifications
- System notifications from TDLib's notification groups (respecting mute and scope
  settings), with sender and text or hidden text, click opens the chat, removed when read;
  an unread badge on the app icon; [later-AI] "smart" notifications filtered by relevance.
  On Zoraya there is no notification service yet (§3): the first version shows the unread
  total in the window title and an in-app toast; system notifications wait for a Zoraya
  notification interface.

### 2.10 Translations
UI strings are English with RU/UK/CS translations in JSON (`ui/i18n/*.json`, source text →
translation). The Rust front end can load the same JSON files; Python-side strings
(statuses, member counts) are English today.

---
