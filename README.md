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
| Team list / join / search, user search, calendar | ⏳ deferred (see below) |
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
xteams chat list [-n N]                        # recent 1:1 / group chats
xteams channel list [team]                     # channels you follow (optional name filter)
xteams channel search <query>                  # find channels by name
xteams message list <conversation> [-n N]      # last N messages
xteams message read <conversation> <id>        # a single message
xteams message new  <conversation> <text> [--reply-to <id>] [--html]
xteams message edit <conversation> <id> <text> [--html]
xteams message react <conversation> <id> [emoji]      # emoji defaults to "like"
xteams thread list <conversation> [-n N] [-a]  # threads in a conversation (top-level msg each; -a adds replies)
xteams thread read <conversation> <root-id>    # one thread: root + replies, chronological
```

`<conversation>` is a Teams conversation id — a channel (`19:...@thread.tacv2`) or a
chat (`19:...@unq.gbl.spaces`). Discover ids with `chat list` / `channel list`.

### Examples

```sh
# Post a rich message to a channel
xteams message new 19:abc@thread.tacv2 "<b>Deploying now</b>" --html

# Reply inside a thread
xteams message new 19:abc@thread.tacv2 "on it" --reply-to 1699999999999

# Machine-readable output for scripting
xteams -j message list 19:abc@thread.tacv2 -n 20 | jq '.[].imdisplayname'
```

### Keychain prompt

On first run (and after each rebuild) macOS asks permission to read the Teams
"Safe Storage" key from your login Keychain. Click **Always Allow** to stop the
prompt recurring.

## Output

- **Default** — concise, human-readable text.
- **`-j` / `--json`** — full structured JSON (all fields, no lossy formatting),
  intended for scripts and tooling.

## Deferred features

`team list/join/search`, `user search`, and calendar require Teams tokens for
*different service audiences* (`chatsvcagg`, `substrate.office.com`) than the ones
the desktop app leaves in cookies. Getting them means minting tokens from the native
OneAuth cache — a larger reverse-engineering effort that is not yet done. Those
commands exit with a clear "deferred" message.

## How it works (short version)

1. Read and decrypt the Teams cookies from the local WebView profile using the macOS
   Keychain key.
2. Exchange the AAD token for a Skype token + regional endpoints via Teams' `authz`.
3. Call the internal chat service with the Skype token.

Full technical detail: [ARCHITECTURE.md](ARCHITECTURE.md).

## Security notes

- `xteams` reads live access tokens for your account. It never writes them to disk;
  tokens live only in memory for the duration of a command.
- It uses undocumented APIs and may stop working when Microsoft changes them.
- Respect your organization's policies.

## License

MIT OR Apache-2.0.
