---
name: xteams
description: >-
  Read and send Microsoft Teams messages from the command line with the
  `xteams` CLI — list chats and channels, read and post messages, reply in
  threads, react with emoji, search teams and people, and view your calendar,
  all without a Microsoft Graph app registration. Use whenever a task involves
  Microsoft Teams (or MS Teams) from a terminal or script — reading a chat or
  channel, posting or editing a message, replying to a thread, reacting,
  finding a person or team, or checking upcoming meetings.
license: MIT
compatibility: >-
  Requires the `xteams` binary on PATH. macOS reads the signed-in New Teams
  desktop app's cookies with no extra setup; any platform can instead run
  `xteams auth login` (device-code). `team`, `user`, and `calendar` always
  require `xteams auth login`.
metadata:
  author: boazy
  project: https://github.com/boazy/xteams-cli
---

# xteams — Microsoft Teams from the CLI

`xteams` drives Teams' private HTTP APIs using the credentials the New Teams
desktop app already holds (macOS) or a device-code sign-in (any platform). It
only ever acts as **you**, with **your** permissions. No Azure/Entra app, no
admin approval.

## Before anything: confirm it works

```sh
xteams auth status -j
```

- Success prints your account and token status. Proceed.
- `command not found` → the binary is not installed. Install it (see
  [Install](#install)) or stop and tell the user; do not fabricate output.
- On macOS the first run per rebuild may raise a Keychain prompt for the Teams
  "Safe Storage" key — that is expected; the user clicks **Always Allow**.

## Golden rules

1. **Always pass `-j` (`--json`) when you will parse the output.** It is a
   global flag valid on every command and anywhere in the argv. Human text is
   lossy; JSON carries every field. Pipe through `jq`.
2. **Quote every conversation id and every Teams link.** Ids contain `:`, `@`,
   and `;`; links contain `&`. Unquoted, the shell mangles them.
3. **Writes need consent.** `message new`, `message edit`, and `message react`
   send or mutate real Teams communications. Only run them once the user has
   given you the target conversation and the final content, and report back the
   server message id the command returns. `list`/`read`/`search` are safe to
   run freely.
4. **`team`, `user`, and `calendar` require `xteams auth login` first.** If not
   signed in they fail with a "run `xteams auth login`" error. Chat, message,
   thread, and channel commands work from the desktop cookies on macOS without
   login.
5. **Discover ids, don't guess them.** Get conversation ids from `chat list` /
   `channel list`; get message ids from `message list` / `message read`.

## Conversation ids

A `<conversation>` argument is one of:

- a **channel** id — contains `@thread.tacv2` (e.g. `19:abc...@thread.tacv2`)
- a **chat** id (1:1 or group) — contains `@unq.gbl.spaces`
- a **Teams deep link** you paste verbatim (see [Deep links](#deep-links))

`48:notes` is your private self-notes space — nobody else sees it. Use it as a
scratch target when you need to test a write safely.

## Command reference

Every command accepts the global `-j/--json`. `-n` limits counts.

### Auth

```sh
xteams auth status            # who am I? per-token audience + expiry
xteams auth login             # device-code sign-in; prints a code on stderr
xteams auth logout            # forget the device-code sign-in
xteams auth seed m365 [-t refresh|access]   # let the m365 CLI reuse your Graph token
```

### Chats & channels

```sh
xteams chat list [-n N]                 # recent 1:1 / group chats (default 20)
xteams channel list [TEAM]              # channels you follow; TEAM filters by id/name
xteams channel search <QUERY>           # channels by name substring
```

### Messages

```sh
xteams message list <CONV> [-n N] [-O FMT]        # last N messages (default 20)
xteams message read <CONV> [ID] [-O FMT]          # one message (ID optional if a link supplies it)
xteams message new  <CONV> [--content TEXT] [-I FMT] [--reply-to ROOT_ID]
xteams message edit <CONV> [ID] [--content TEXT] [-I FMT]
xteams message react <CONV> [ID] <EMOJI>          # EMOJI is required: like, heart, laugh, surprised, sad, angry, ...
```

- `message new`/`edit` take the body from `--content`, or from **stdin** when
  `--content` is omitted: `echo "hi" | xteams message new "$CONV"`.
- Reply into a thread with `--reply-to <root-message-id>`.
- The **server message id** to use for a later `edit`/`react` comes back from
  `message list`/`read` (JSON field `id`) — it is not the text you posted.

### Threads

```sh
xteams thread list <CONV> [-n N] [-a] [-O FMT]    # each thread's root; -a also fetches replies
xteams thread read <CONV> [ROOT_ID] [-O FMT]      # one thread: root + replies, chronological
```

### Teams, people, calendar (require `xteams auth login`)

```sh
xteams team list                        # teams you belong to
xteams team search <QUERY>              # teams by name
xteams user search <QUERY>              # people by name / email
xteams calendar upcoming [-d DAYS]      # upcoming events (default 7 days)
```

> The command is `calendar upcoming`, **not** `calendar list`. `team join`
> exists in the CLI but is deferred and not functional — do not rely on it.

## Message content & formats

Bodies are converted to/from the HTML Teams stores on the wire.

- **Input** (`message new`/`edit`) — `-I/--content-input-format`, default
  `markdown`:
  - `markdown` — CommonMark + tables, strikethrough, task lists
  - `plain` — literal text (HTML-escaped, newlines → `<br>`)
  - `html` — raw RichText/Html sent verbatim
  - `pandoc:<fmt>` — `pandoc --from <fmt> --to html` (needs `pandoc` on PATH)
- **Output** (`message`/`thread` `read`/`list`) — `-O/--content-output-format`:
  `markdown`, `plain`, `html`, `keep` (raw Teams HTML), or `pandoc:<fmt>`
  (`pandoc --from html --to <fmt>`).
  - Default output is **`markdown` in text mode** but **`keep` in `-j` JSON
    mode**. So when reading JSON, pass `-O markdown` if you want the body as
    Markdown rather than raw HTML.
- `-f/--content-format` sets input and output at once (mutually exclusive with
  `-I`/`-O`). Extra pandoc flags: `--pandoc-standalone`,
  `--pandoc-metadata title=Doc`, etc.

## Deep links

Anywhere `<CONV>` is accepted you can paste a link the Teams app generates
(`https://teams.microsoft.com/l/…`, `https://teams.cloud.microsoft/l/…`, or
`msteams:/l/…`) — for a channel, chat, team, or a specific message. The
conversation id comes from the link. When the link points at a **message**, the
message id is filled too, so you can drop the `[ID]` argument for
`read`/`edit`/`react`/`thread read`. **Quote the URL** (it contains `&`).

```sh
xteams message read 'https://teams.microsoft.com/l/message/19:abc@thread.tacv2/1699999999999?parentMessageId=1699999990000'
xteams message react 'https://teams.microsoft.com/l/message/19:abc@thread.tacv2/1699999999999' heart
```

## Parsing JSON output (`jq`)

Field names as emitted by `-j`:

- **chat list / channel list** → **array** of conversations: `.[].id`,
  `.[].threadProperties.topic` (channel title; may be null),
  `.[].lastMessage.imdisplayname`, `.[].lastMessage.content`.
- **message list** → **array** of messages: `.[].id`, `.[].imdisplayname`
  (sender), `.[].content`, `.[].composetime`, `.[].rootMessageId` (a message
  is a thread **root** when `id == rootMessageId`).
- **message read** → a **single** message object: `.id`, `.imdisplayname`,
  `.content`, `.rootMessageId`.
- **message new / edit / react** → a **single** action object: `.action`,
  `.conversation`, `.message_id` (the server id — reuse it for a later
  edit/react), `.emoji`.
- **thread list** → **array** of `{ root, replies }`: `.[].root.id`,
  `.[].root.content`, `.[].replies[].content`.
- **thread read** → a **flat array** of messages (root + replies,
  chronological), same shape as `message list`: `.[].id`, `.[].imdisplayname`,
  `.[].content`, `.[].rootMessageId`.
- **team list/search** → `.[].id`, `.[].displayName`, `.[].channels[].id`,
  `.[].channels[].displayName`.
- **user search** → people: `.[].DisplayName`, `.[].EmailAddresses[]`,
  `.[].JobTitle`, `.[].MRI` (their Teams id).
- **calendar upcoming** → `.[].subject`, `.[].start.dateTime`, `.[].end.dateTime`,
  `.[].location.displayName`, `.[].isOnlineMeeting`, `.[].webLink`.

```sh
# Find a chat by the other person's name, then read its last 20 messages as markdown
CONV=$(xteams -j chat list -n 50 | jq -r '.[] | select(.lastMessage.imdisplayname // "" | test("Alice";"i")) | .id' | head -1)
xteams -j message list "$CONV" -n 20 -O markdown | jq -r '.[] | "\(.imdisplayname): \(.content)"'
```

## Common workflows

**Post a markdown message to a channel** (after the user approves target + text):

```sh
xteams message new "$CONV" --content "**Deploying now** — see thread for status"
```

**Reply inside a thread:**

```sh
xteams message new "$CONV" --content "on it" --reply-to "$ROOT_ID"
```

**React to a message:**

```sh
xteams message react "$CONV" "$MSG_ID" heart
```

**Find a person's Teams id / email:**

```sh
xteams -j user search "Jane Doe" | jq -r '.[] | "\(.DisplayName) <\(.EmailAddresses[0] // "?")> \(.MRI // "")"'
```

**Check the next 3 days of meetings:**

```sh
xteams -j calendar upcoming -d 3 | jq -r '.[] | "\(.start.dateTime) \(.subject)"'
```

## Gotchas

- The calendar verb is `upcoming`, not `list`.
- `message react` requires the emoji argument — there is no default.
- In `-j` mode, message bodies default to raw Teams **HTML** (`keep`); add
  `-O markdown` for Markdown.
- `team`/`user`/`calendar` return a "run `xteams login`" error until you have
  run `xteams auth login`. Offer to run it (it prints a device code on stderr
  the user completes in a browser); do not silently skip the command.
- A cached token can be rejected (401); `xteams` clears exactly that entry and
  asks you to re-run — just run the command again.
- These are undocumented APIs and may break; only your own account and
  permissions are available. Respect the org's policies.

## Install

`xteams` is a separate Rust binary, not bundled with this skill.

```sh
# With mise (recommended)
mise use -g github:boazy/xteams-cli

# Or build from source
git clone https://github.com/boazy/xteams-cli
cd xteams-cli && cargo build --release   # binary at ./target/release/xteams
```

macOS is fully supported from the desktop cookies. On Windows/Linux (or where
Teams isn't installed) run `xteams auth login` once. Full details:
<https://github.com/boazy/xteams-cli>.
