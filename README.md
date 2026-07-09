# xteams

An unofficial Microsoft Teams command-line client that talks to Teams using the
**credentials already stored by the New Teams desktop app** — no Microsoft Graph
API, no Azure/Entra app registration, no admin approval required.

> ⚠️ **Unofficial & unsupported.** `xteams` uses Teams' private, undocumented HTTP
> APIs with tokens extracted from your local install. It works only with *your own*
> account on *your own* machine. It is not sanctioned by Microsoft, may break at any
> time, and may conflict with your organization's acceptable-use policy. Use at your
> own risk.

## Why

The official Graph API requires an approved Azure app, which many organizations lock
down. `xteams` sidesteps that by reusing the tokens the Teams desktop app already
holds — the same way the app itself talks to the backend.

## Status

| Area | Status |
|------|--------|
| Auth / credential extraction (macOS) | ✅ working |
| Chats, messages, threads, posting, editing, reactions | ✅ working |
| Channel list / search (channels you follow) | ✅ working |
| Team list / search, user search, calendar (via `xteams login`) | ✅ working |
| Seed the m365 CLI (`auth seed m365`) with a Graph token | ✅ working |
| Team join | ⏳ deferred (write op; endpoint unverified) |
| Windows | ⏳ not yet (design accounts for it) |

## Requirements

- **macOS** with **New Teams** ("Teams 2.0", bundle `com.microsoft.teams2`)
  installed and signed in.
- **Rust** (2024-edition toolchain) to build.

## Install

```sh
git clone <this repo>
cd xteams-cli
cargo build --release
# binary at ./target/release/xteams
```

## Usage

`xteams` uses a two-tier `<noun> <verb>` layout. Add `-j` / `--json` to any command
for machine-readable output.

```
xteams auth                                   # who am I? token status
xteams login                                   # device-code sign-in (unlocks team/user/calendar)
xteams logout                                  # forget the device-code sign-in
xteams auth seed m365                          # let the m365 CLI use your Graph token (see below)
xteams chat list [-n N]                        # recent 1:1 / group chats
xteams channel list [team]                     # channels you follow (optional name filter)
xteams channel search <query>                  # find channels by name
xteams message list <conversation> [-n N]      # last N messages
xteams message read <conversation> [id]        # a single message
xteams message new  <conversation> <text> [--reply-to <id>] [--html]
xteams message edit <conversation> [id] <text> [--html]
xteams message react <conversation> [id] <emoji>      # e.g. like, heart, laugh
xteams thread list <conversation> [-n N] [-a]  # threads in a conversation (top-level msg each; -a adds replies)
xteams thread read <conversation> [root-id]    # one thread: root + replies, chronological
xteams team list                               # teams you belong to (needs `xteams login`)
xteams team search <query>                     # find teams by name
xteams user search <query>                      # find people by name / email
xteams calendar list [-d DAYS]                 # upcoming calendar events (default 7 days)
```

`<conversation>` is a Teams conversation id — a channel (`19:...@thread.tacv2`) or a
chat (`19:...@unq.gbl.spaces`) — or a **Teams link** you copied from the app (see
below). Discover ids with `chat list` / `channel list`.

### Pasting Teams links

Anywhere a `<conversation>` is accepted you can paste a link the Teams app generates
(`https://teams.microsoft.com/l/…`, `https://teams.cloud.microsoft/l/…`, or
`msteams:/l/…`) — for a channel, chat, team, or a specific message. The conversation
id is taken from the link. When the link points at a **message**, the message id is
used too, so you can drop the separate `[id]` argument: `message read`/`edit`/`react`
target that message, while `thread read` and `message new --reply-to` target its
thread (preferring the link's `parentMessageId`). An explicitly typed id still wins.
Quote the URL so your shell doesn't split it on `&`.

### Examples

```sh
# Post a rich message to a channel
xteams message new 19:abc@thread.tacv2 "<b>Deploying now</b>" --html

# Reply inside a thread
xteams message new 19:abc@thread.tacv2 "on it" --reply-to 1699999999999

# Machine-readable output for scripting
xteams -j message list 19:abc@thread.tacv2 -n 20 | jq '.[].imdisplayname'

# Read the exact message a Teams link points to (conversation + id from the link)
xteams message read 'https://teams.microsoft.com/l/message/19:abc@thread.tacv2/1699999999999?parentMessageId=1699999990000'

# React to that message — id comes from the link, you just add the emoji
xteams message react 'https://teams.microsoft.com/l/message/19:abc@thread.tacv2/1699999999999' heart
```

### Keychain prompt

On first run (and after each rebuild) macOS asks permission to read the Teams
"Safe Storage" key from your login Keychain. Click **Always Allow** to stop the
prompt recurring.

## Output

- **Default** — concise, human-readable text.
- **`-j` / `--json`** — full structured JSON (all fields, no lossy formatting),
  intended for scripts and tooling.

## Extra features via `xteams login`

`team list/search`, `user search`, and `calendar` talk to service audiences
(`chatsvcagg.teams.microsoft.com`, `substrate.office.com`, Microsoft Graph) that the
desktop cookies don't cover. Run **`xteams login`** once — a device-code sign-in you
complete in your browser — and `xteams` stores a refresh token in your Keychain, then
mints the per-audience tokens on demand (silent for ~90 days). `xteams logout` forgets
it.

`team join` is still deferred: it is a write operation and its endpoint is unverified.

## Use your tokens with the m365 CLI

`xteams auth seed m365` writes a Microsoft Graph access token into the
[m365 CLI](https://github.com/pnp/cli-microsoft365)'s connection store
(`~/.cli-m365-connection.json`), so `m365` can call Graph without its own sign-in:

```sh
xteams auth seed m365      # then, e.g.: m365 status  •  m365 entra user get --id <guid>
```

The seeded token lasts about an hour and carries the Teams client's Graph scopes
(m365 commands needing a scope Teams doesn't hold return HTTP 403). Re-run
`xteams auth seed m365` before it expires — this path stores no refresh token for m365
to renew from. Requires `xteams login` first.

## How it works (short version)

1. Read and decrypt the Teams cookies from the local WebView profile using the macOS
   Keychain key.
2. Exchange the AAD token for a Skype token + regional endpoints via Teams' `authz`.
3. Call the internal chat service with the Skype token.
4. For `team` / `user` / `calendar`, `xteams login` runs an OAuth device-code sign-in
   to obtain a FOCI family refresh token (stored in the Keychain), then mints
   per-audience tokens for `chatsvcagg`, `substrate`, and Microsoft Graph on demand.

Full technical detail: [ARCHITECTURE.md](ARCHITECTURE.md).

## Security notes

- `xteams` reads live access tokens for your account. It never writes them to disk;
  tokens live only in memory for the duration of a command.
- It uses undocumented APIs and may stop working when Microsoft changes them.
- Respect your organization's policies.

## License

MIT OR Apache-2.0.
