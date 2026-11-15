# `tg`: the Telegram window on Zoraya — design (TG-04, stage A)

`tg` is a native Zoraya program: Rust, `no_std`, over `zoraya-ui` and `zoraya-app`, as Files and Edit are. It holds
the compositor's channel and nothing else: no network, no file. Everything it shows comes from a **backend** behind
one trait: in stage A a sample backend built into the program (an invented account), in stage B `tgd` over a Unix
socket. What works and what was measured is in `docs/tg-window-report.md`; this file is the design.

- Code: `tg/window` (the crate `tg-window`: protocol types, model, sample backend, widgets; tested on the host),
  `tg/tg` (the program: the loop around it).
- The look follows Zoraya's tokens (`zoraya-tokens`, the direction `clear-field`) and its four rules. The one that
  shapes a chat client most: **colour means a room.** The compositor alone draws saturated colour, so `tg` has no
  coloured avatars, no blue bubbles and no green ticks. Its own colours are the neutral palette and `accent`.

## The window

One toplevel titled `Telegram`, 840 × 520 logical pixels at first, resizable. The compositor draws its frame, the
title and the grant pills (only its data directory: the window holds no network, so there is no network pill).

```
┌ Telegram ──────────────────────────────────────────────────────────────────────────────┐
│ [ Search            ] [Settings] │ Оксана Коваль                            [Summary]  │  48: bars
│──────────────────────────────────│ last seen recently                                  │
│ (ZР) Zoraya — розробка    13:48  │─────────────────────────────────────────────────────│
│      You: Журнал чистий…      ⚲  │                    ( Yesterday )                    │
│ (SM) Saved Messages       10:50  │ ╭──────────────────────────────╮                    │
│      File: plan.md · 12 KB    ⚲  │ │ Привіт! Ти вже бачив нову    │                    │
│ (ОК) Оксана Коваль        13:00  │ │ збірку?                11:20 │                    │
│      Не запізнюйся :)       (2)  │ ╰──────────────────────────────╯                    │
│ (ИС) Иван Соколов         12:25  │                    ╭───────────────────────────────╮│
│      В каталоге данных, рядом…   │                    │ Привіт! Ще ні, зараз          ││
│ (Р)  Родина ⊘             12:10  │                    │ подивлюсь.          11:22 ✓✓  ││
│      Мама: Не забудь подзв… (5)  │                    ╰───────────────────────────────╯│
│ (ZN) Zoraya News ⊘        11:10  │ ╭──────────────────────────────╮                    │
│      Tonight's build is…   (12)  │ │ ▏You                         │                    │
│  …                               │ │ ▏Виглядає добре. А кирилиця… │                    │
│                                  │ │ Так, усі літери є.     11:42 │                    │
│                                  │ ╰──────────────────────────────╯                    │
│                                  │ [ Message                                 ] [ Send ]│
└──────────────────────────────────┴─────────────────────────────────────────────────────┘
  300: the chat list                 the open chat
```

Under 620 logical pixels of width one pane is shown at a time: the list, or the open chat (Escape goes back).
With no chat open the right side says `Choose a chat to read it`.

### The chat list

A row is 62 high: the avatar (a disc of 44 with the title's initials, `bg.view` with a little `fg.text`: neutral),
the title in Inter Medium at `text.body`, the time at the right in `text.caption` and `fg.text.secondary` (the clock
today, the weekday within a week, else the date), and under them the last message as one line (in a group with its
sender's first name; a placeholder's kind for a photo or a file), cut with `…`.

| What | How it is drawn |
|---|---|
| Unread count | a pill at the right of the second line: `accent` with `fg.text.on-accent`; `border.strong` when the chat is muted; `999+` past that |
| Pinned | a pin (a head and a needle) at the right of the second line, when nothing is unread; pinned chats come first |
| Muted | a struck circle after the title, in `fg.text.disabled` |
| The open chat | the row in `bg.selection` |
| The keyboard's row | a ring of `accent`, 2 pixels, inside the row (only while the list has the keyboard) |
| A row not here yet | its disc alone (the window asked for rows it does not hold yet) |
| No chats, or none match | `No chats`, `No chats match “…”` |

The list holds only the rows it shows (the backend's **window**, below), so an account with thousands of chats costs
the window what a screenful costs.

### The chat view

- **Header** (48): the title in Inter Medium at `text.heading`, under it what the backend says of the chat
  (`last seen recently`, `5 members`, `channel · 1 204 subscribers`), and the `Summary` button at the right.
- **Bubbles.** Incoming at the left in `bg.control` with a `border` edge; outgoing at the right in `bg.selection`;
  corners of `radius.popup`; at most 70 % of the pane and 520 wide; the text at `text.body`, a line 19 high. Messages
  of one sender stand 3 apart, others 8.
- **In a bubble**, top to bottom: the sender's name (incoming, in a group; Inter Medium in `accent`); `Forwarded from
  NAME` (`text.caption`, secondary); the message answered (a bar of `accent`, who, its first line); a placeholder's
  tile; the text; the time (with `edited` before it when it was), beside the last line when it fits, else on a row of
  its own; and on outgoing messages a ring while it is not confirmed, one tick when sent, a second in `accent` when
  read.
- **Text spans**: bold in Inter Medium, code in JetBrains Mono, links and mentions in `accent` and underlined. Italic
  is drawn upright (the system has no italic face). Emoji have no font yet and draw as boxes (request R7).
- **Placeholders** until Zoraya has decoders: a tile of `bg.control.pressed` that says what it is and what the backend
  knows of it: `Photo` with its size (220 × 124), `Sticker` with its description (124 × 124), `Voice message` with its
  length, `File` with its name and size (220 × 44). A caption is the text under it.
- **Service lines** (`Ірина Бойко joined the group`) and the **dates** between days (`Today`, `Yesterday`, `11
  November`) are centred pills in `text.caption`.
- The view is anchored at the newest message. Scrolled up, it stays where it is when a message arrives; near the top
  it asks for the page before the oldest message it holds.

### The composer

A field of `bg.view` with a `border.strong` edge under the messages, and `Send` (the default button, in `accent`).
It grows with its text from one line to six, then scrolls to keep the cursor's line in view. **Enter sends;
Shift+Enter starts a line.** While a message is being answered a bar above the field names it (`Reply to NAME` and its
first line); Escape drops it. The placeholder is `Message`. A message is at most 4096 characters.

### Search

The field at the top of the list filters the chat list by title as it is typed (without case, in any script); the
list's window is asked again with the query. Enter or Down goes to the first chat found, Enter opens it, Escape
clears the search. Typing while the list has the keyboard goes to the search field. (Search inside messages is the
backend's `SearchModel` and comes with stage B.)

### The AI summary

`Summary` opens a card over the messages, the application's own (`bg.view`, `border.strong`, `radius.popup`; no glow:
that material is the compositor's):

```
        ╭──────────────────────────────────────────────╮
        │ Summary · Оксана Коваль                      │
        │ [Unread] [Last day] [Last week]              │
        │                                              │
        │ Оксана Коваль: 2 unread messages. The main   │
        │ points: …                                    │
        │                                              │
        │ Summarising…                        [Close]  │
        ╰──────────────────────────────────────────────╯
```

The scope's button in use is the accent one; another scope starts again. The text is added to as its parts come. A
refusal (`AI summaries are turned off in Settings.`) is written in `error`. Escape or `Close` closes it and cancels a
summary still running.

### The login

Three steps of one form in the middle of the window: the heading, what is asked, the backend's hint, one field, and
`Next` (Enter does the same):

```
        Telegram
        Your phone number, with the country's code
        (hint)
        [ +…                                   ]
        Telegram refused it: PHONE_NUMBER_INVALID        (in `error`, only after a refusal)
        [ Next ]
```

then `The code Telegram sent you`, then `Your password (two-step verification)`. **The password is never drawn:** the
field shows a dot a character, whatever was typed, and no login value is kept after it is sent or printed anywhere.
In the window this needs nothing from Zoraya (the terminal's missing echo control, R18, does not apply).

### Settings

A form of the same shape: a switch for each setting the backend keeps (in the sample: AI summaries, the last message
in the chat list, unread counts of muted chats), `Log out` and `Back`. Light and dark are not here: they follow
Zoraya's own setting.

### Light and dark, scale 1 and 2

Every colour is a token of the palette in use, so the window follows `Configure` (the compositor says light or dark)
with no code of its own; `zoraya-app` repaints it. Sizes are logical; every measure above is multiplied by the
scale's factor, text is rasterised at the physical size, and one-pixel lines are one logical pixel.

## The keyboard

Every action has a key. Tab and Shift+Tab go round the controls in this order: search, `Settings`, the list, the
messages, `Summary`, the composer, `Send` (and the card's buttons while it is open).

| Where | Keys |
|---|---|
| Search | text; Enter or Down: to the list, on the first chat found; Escape: clear |
| The list | Up, Down, Page Up, Page Down, Home, End: the keyboard's row (the list follows); Enter: open; text: search |
| The messages | Up, Down: the keyboard's message (the view follows); Shift+Up, Shift+Down: extend the selection; Page Up, Page Down, Home, End: scroll; Enter or R: answer it; Escape: drop the selection |
| The composer | text; Enter: send; Shift+Enter: a line; arrows, Home, End, Backspace, Delete; Up on the first line: to the messages; Page Up, Page Down: scroll the messages; Ctrl+F: to the search |
| Buttons, switches | Enter or Space |
| Anywhere | Escape closes the nearest thing open: settings, the summary, the reply, the selection, the search, and in a narrow window the chat |
| The system's | System+C copies the selected messages; System+V pastes into the field that has the keyboard; System+W closes the window (the compositor's chords) |

## The wheel

As in Zoraya's applications (ADR-0329): the wheel goes to what is under the pointer and moves no focus. Over the
list a notch is three rows, and the window of rows follows; over the messages a notch is 72 logical pixels (three
steps of 24, a little more than a line each); a view at its end takes no more turns that way. The composer and the
card do not scroll with the wheel.

## Selection and copy

A press on a message selects it; Shift with a press, or Shift with Up and Down, extends the selection over whole
messages (a band of `bg.control.pressed` behind them, a `border.strong` edge on their bubbles). System+C answers the
compositor's `Copy` with their text: one message as it is, several each with its sender (`Name: text`), a placeholder
as `[Photo] caption`. A press outside a bubble drops the selection. Selecting part of a message's text is not built.

## The protocol: what the window needs of `tgd`

`docs/tgd-protocol.md` (protocol 1) is a command line's protocol: `chats.list` gives twenty chats once, `chat.open`
a page and then only `message.new`. A window needs **models**: lists it holds a part of, kept true by the backend.
What follows is protocol 2's addition; the transport is the same (one JSON object a line, `{"op": …, "id": …}` in,
`{"re": …}` and `{"ev": …}` out; ids of chats and messages as strings). In the code it is `tg_window::proto`: each
`Request` is an `op`, each `Event` an `ev`, with these names and fields, so `SocketBackend` is a serialiser and
nothing more.

### Text

`{"text": "…", "spans": [[start, end, kind], …]}`: `start` and `end` in code points, `kind` one of `bold`, `italic`,
`code`, `link`, `mention` (tgclient's `store/richtext.py` has the entities and the UTF-16 map; it gains this emitter
beside the HTML one).

### The chat list model: a window of rows

| | |
|---|---|
| `chats.window` `{first, count, query}` | The window wants rows `[first, first + count)` of the list filtered by `query`, and from now on their changes. One window at a time: a new one replaces the old. |
| → `chats.rows` `{first, total, rows}` | The window's rows, whole. `total` is the filtered list's length. |
| → `chats.splice` `{at, remove, rows, total}` | From absolute index `at`, `remove` rows of the window are replaced by `rows`. |

A row is `{id, title, kind (private, group, channel, saved), last, date, unread, pinned, muted}`.

**One diff for every change.** After anything that changes the list (a message, a read, a pin, a move, a chat
gone), the backend computes the window's rows again, drops what is equal at the start and at the end against what it
last sent, and sends the middle as one `chats.splice`. A changed row is a splice of one; a chat that moves up from
row 7 to row 2 is a splice of six; a chat that enters or leaves moves the rest and the row at the window's end comes
or goes with it. The window applies it blindly (replace a range), so the two sides cannot disagree about a move's
meaning, and the backend needs no more than the rows it sent (`tests.rs` checks 600 random steps: the window's rows
equal the backend's list). The cost is at most one screenful a change.

### The message list model: pages and diffs by id

| | |
|---|---|
| `chat.open` `{chat, count}` | Opens the chat, reads it, and subscribes to it. |
| → `messages.reset` `{chat, title, subtitle, kind, rows, more}` | The newest `count` messages, oldest first; `more`: older ones exist. |
| `messages.older` `{chat, before, count}` → `messages.page` `{chat, rows, more}` | The page before message `before`. |
| → `message.new` `{chat, row}` | A message at the end. |
| → `message.update` `{chat, old_id, row}` | The message `old_id` is now `row`: edited, confirmed (a sent message's temporary id becomes its final one), read. |
| → `message.delete` `{chat, ids}` | |
| `messages.send` `{chat, text, reply_to}` | Answered by `message.new` with a temporary id and `state: pending`, then `message.update`. |
| `chat.close` | |

A row is `{id, date, out, sender, kind (text, service, photo, sticker, voice, file), text, detail, reply {sender,
text}, forward, edited, state (pending, sent, read)}`; `detail` is what a placeholder shows (`1280 × 800`, `0:42`,
`report.pdf · 248 KB`).

### The rest

| | |
|---|---|
| `hello` `{now, utc_offset, synthetic, …}` | Adds the wall clock and the zone to protocol 1's `hello`: the window has no clock or zone of its own. `synthetic` lets the window print what it shows on the console (tests); with a real account it prints nothing of it. |
| `auth.phone`, `auth.code`, `auth.password` `{value}`; `auth.logout` → `auth.state` `{state, hint, error}` | Protocol 1's, with the refusal as a field of the state that asks again. |
| `ai.summarize` `{chat, scope}`, `ai.cancel` → `ai.summary` `{chat, state (running, done, failed), text}` | `text` is added to what is shown: the answer streams. |
| `settings.set` `{key, on}` → `settings` `{items: [{key, label, on}]}` | The backend's own switches. |

### How tgclient's Qt view-models map onto it

The 40 % of tgclient that is Qt-shaped logic (TG-01, section 1) keeps its method bodies; its Qt notifications go to
an event bus that speaks the protocol. For stage A's scope:

| tgclient | Becomes | Notes |
|---|---|---|
| `models/chat_list.py` `ChatListModel` (`setList`, `loadMore`, roles `ChatId, Title, Preview, Time, UnreadCount, Muted, Pinned, ChatType, OutStatus, …`) | the chat list model | `rowCount`/`data` serve a window's rows; `dataChanged`, `rowsInserted`, `rowsRemoved`, `rowsMoved` and `modelReset` all become "the window may have changed": one `chats.splice`. `loadMore` is the backend's own business (it loads what a window asks for). `Initials` and `ColorIndex` are the view's here; `Time` is sent as `date` and written by the window. Later fields: `MentionCount`, `Draft`, `Typing`, `Online`, `Verified`, `MarkedUnread`. |
| `models/search.py` `SearchModel` (chat rows) | `chats.window`'s `query` | Its message hits and sections are a second list model of the same shape (stage B). |
| `models/messages.py` `MessageListModel` (`open`, `close`, `loadOlder`, `send`; `history_insert`, `history_remove`, `history_changed`, `history_reset`; roles `MessageId, IsOutgoing, IsService, ServiceText, SenderName, Html, Time, Edited, Status, ReplySender, ReplyText, MediaLabel, MediaKind, DayLabel, …`) | the message list model | `history_reset` → `messages.reset`; `history_insert` at the end → `message.new`, at the start (after `loadOlder`) → `messages.page`; `history_changed` → `message.update`; `history_remove` → `message.delete`. `Html` becomes text with spans; `DayLabel`, `ShowSender` and grouping are the view's. Later: `loadNewer`, `jumpTo`, reactions, selection's actions (`deleteSelected`, `forwardSelected`), edit. |
| `models/composer.py` `ComposerModel` (`setDraft`, `sent`) | `messages.send` | Drafts, mentions, link previews and staged files come later (`draft.set`, …). |
| `ui/auth_controller.py` `AuthController` (`step`, `hint`, `error`, `submit`) | `auth.*` and `auth.state` | One to one. QR login stays out (TG-01, decision 5). |
| `ui/ai_controller.py` `AiController` (`openPanel`, `ask`, `summaryChanged`, `setEnabled`) | `ai.summarize`, `ai.summary`, `settings` | The panel's other subjects (a person, a digest, translation) later. |

## What the window asks of Zoraya

Nothing at run time beyond what every application gets. At build time, two things Zoraya does not have yet, kept as
an overlay on a local clone and written as generic requests (`docs/requests.md`, R20 to R22): native programs built
outside the repository (the heap and the platform of an application as crates; extra programs in the ramdisk), and a
dock tile and launch record for such a program.
