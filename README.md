# xteams

An unofficial Microsoft Teams command-line client that talks to Teams using the
**credentials already stored by the New Teams desktop app** — no Microsoft Graph
API, no Azure/Entra app registration, no admin approval required.

> [!WARNING]
> `xteams` uses Teams' private, undocumented HTTP
> APIs. It works only with *your own* account and it only has the permissions you
> have. It is not (entirely) based on the offical Graph API documented by Microsoft
> and may break at any time, without prior warning. Use at your own risk.

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
| `xteams login` + all commands, on macOS / Windows / Linux | ✅ cross-platform (device-code; no Teams app) |
| Windows cookie fallback (no login) | 🧪 implemented, **untested** |

## Requirements

- **`xteams login` (device-code) works on macOS, Windows, and Linux** — it needs only
  a browser to complete sign-in, no Teams app.
- **Cookie fallback (no login)** reads the local **New Teams** ("Teams 2.0") install:
  - **macOS** — fully supported (bundle `com.microsoft.teams2`), signed in.
  - **Windows** — implemented but **untested** (`MSTeams_8wekyb3d8bbwe`); may require
    Teams closed, and does not cover App-Bound-Encryption (`v20`) cookies.
  - **Other platforms** — not supported; use `xteams login`.
- **Rust** (2024-edition toolchain) to build.

## Install

Future `v*` tags publish platform archives to [GitHub Releases](https://github.com/boazy/xteams-cli/releases)
with a `checksums.txt` manifest. macOS and Linux use `tar.gz`;
Windows uses `zip`.

### Install with mise

If `mise` is not installed, follow the [official mise installation instructions](https://mise.jdx.dev/getting-started.html).
Then install the latest GitHub Release globally:

```sh
mise use -g github:boazy/xteams-cli
```

`-g` records `xteams` in mise's global configuration, making it available from any directory.

### Build from source

```sh
git clone <this repo>
cd xteams-cli
cargo build --release
# binary at ./target/release/xteams
```

## Usage

`xteams` uses a two-tier `<category> <verb/subcommand>` layout. Add `-j` / `--json` to any command
for machine-readable output.

```
xteams auth status                                # who am I? token status
xteams auth login                                 # device-code sign-in (unlocks team/user/calendar)
xteams auth logout                                # forget the device-code sign-in
xteams auth seed m365                             # let the m365 CLI use your Graph token (see below)
xteams chat list [-n N]                           # recent 1:1 / group chats
xteams channel list [team]                        # channels you follow (optional name filter)
xteams channel search <query>                     # find channels by name
xteams message list <conversation> [-n N]         # last N messages
xteams message read <conversation> [id]           # a single message
xteams message new  <conversation> <text> [--reply-to <id>] [--html]
xteams message edit <conversation> [id] <text> [--html]
xteams message react <conversation> [id] <emoji>  # e.g. like, heart, laugh
xteams thread list <conversation> [-n N] [-a]     # threads in a conversation (top-level msg each; -a adds replies)
xteams thread read <conversation> [root-id]       # one thread: root + replies, chronological
xteams team list                                  # teams you belong to (needs `xteams login`)
xteams team search <query>                        # find teams by name
xteams user search <query>                        # find people by name / email
xteams calendar upcoming [-d DAYS]                # upcoming calendar events (default 7 days)
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

On first run (and after each rebuild) macOS may ask permission to read the Teams
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
complete in your browser — and `xteams` caches a FOCI **family refresh token** in
`~/.local/state/xteams/token-cache.json` (the XDG state dir, owner-only `0600`), then mints the per-audience
tokens on demand (silent for ~90 days). `xteams logout` forgets it.

Once signed in, that refresh token also drives the **chat** commands (it can mint the
skypetoken itself), so a logged-in `xteams` needs **no cookies and no running Teams
app** — handy where Teams isn't installed. Without login, `xteams` still works silently
from the desktop cookies, limited to the chat/message/thread/channel commands.

`team join` is still deferred: it is a write operation and its endpoint is unverified.

## Use your tokens with the m365 CLI

`xteams auth seed m365` seeds the [m365 CLI](https://github.com/pnp/cli-microsoft365)'s
credential store from your xteams sign-in, so `m365` can call Microsoft Graph without its
own login:

```sh
xteams auth seed m365            # default: refresh — m365 self-renews (~90 days)
xteams auth seed m365 -t access  # access-only — a ~1 h token; re-run before it expires
```

Select the mode with `-t`/`--token-type` (default `refresh`):

- **`refresh`** (default) injects a refresh token into m365's MSAL cache, so m365 silently
  renews Graph tokens for the token's lifetime — seed once.
- **`access`** writes only a ~1-hour Graph access token; re-run before it expires (m365 has
  no refresh token to renew from in this mode).

The token carries the Teams client's Graph scopes (m365 commands needing a scope Teams
doesn't hold return HTTP 403), and only Microsoft Graph is seeded. Requires `xteams login`
first — then use m365 as usual, e.g. `m365 status` or `m365 entra user get --id <guid>`.
You don't need to use `m365 setup` or `m365 login` if you seed m365 using this method.

## How it works (short version)

1. Read and decrypt the Teams cookies from the local WebView profile using the macOS
   Keychain key.
2. Exchange the AAD token for a Skype token + regional endpoints via Teams' `authz`.
3. Call the internal chat service with the Skype token.
4. `xteams login` runs an OAuth device-code sign-in to obtain a FOCI family refresh
   token, cached (with the minted access tokens + skype session) in
   `~/.local/state/xteams/token-cache.json` (the XDG state dir). It mints per-audience tokens for `chatsvcagg`,
   `substrate`, and Microsoft Graph on demand — and can derive the skypetoken too, so
   when signed in even the chat commands run without cookies or the Teams app.

Full technical detail: [ARCHITECTURE.md](ARCHITECTURE.md).

## Security notes

- After `xteams login`, `xteams` caches your FOCI refresh token, the minted access
  tokens, and the derived skype session as plaintext JSON in
  `~/.local/state/xteams/token-cache.json` (the XDG state dir, owner-only `0600`) so later commands don't
  re-authenticate every time. Treat it like any other on-disk credential; `xteams logout`
  deletes it. (Cookie-only, not-logged-in usage keeps tokens in memory for the command.)
- Due to Apple constantly breaking how application signatures work in undocumented ways
  and making user experience miserable to open source apps not signed with Apple Develper IDs,
  we're not using the Keychain to store the referesh token. This has negative impact on security,
  but `xteams` is not behaving any differently from Microsoft CLI tools, which also store all their
  tokens in file. It gives you the same level of security as Microsoft's own official CLI tools.
- `auth seed m365` additionally writes a Graph token (and, in `refresh` mode, the refresh
  token) into the **m365 CLI's own credential store** (`~/.cli-m365-*.json`), also as
  plaintext JSON.
- It uses undocumented APIs and may stop working when Microsoft changes them.
- Respect your organization's policies.

## License

MIT
