# Architecture

Technical reference for `xteams`, an unofficial Microsoft Teams CLI that drives
Teams' private HTTP APIs using credentials extracted from the local New Teams
(Teams 2.0) desktop install.

**Audience:** engineers/agents extending this codebase. Keep this file in sync with
the code — see [AGENTS.md](AGENTS.md).

## 1. High-level pipeline

```
Keychain key ─┐
              ▼
cookies (v10) ── decrypt ──► authtoken (AAD)  ──authz──►  skypetoken + regionGtms
                             skypetoken_asm                        │
                                                                   ▼
                                                      chat service (IC3) ──► render
```

1. **creds** — read the Chromium `Cookies` SQLite from the signed-in WebView
   profile; decrypt values with a key derived from the macOS Keychain.
2. **auth** — extract the AAD bearer from the `authtoken` cookie; POST it to Teams'
   `authz` endpoint to get a fresh Skype token + the regional service map.
3. **api** — call the regional chat service with the Skype token.
4. **output** — commands return typed values; a single renderer prints human text or
   JSON.

## 2. Module map

| File | Responsibility |
|------|----------------|
| `src/main.rs` | Entry; `#[tokio::main]`; parse CLI; dispatch. |
| `src/cli.rs` | clap derive: two-tier `<noun> <verb>` tree + global `--cookies`, `-j/--json`. |
| `src/link.rs` | Parse Teams deep links (`/l/…` URLs) into `TeamsDeepLinkFields`; resolve a conversation argument that may be a link. |
| `src/creds.rs` | macOS credential extraction (Keychain → PBKDF2 → AES-128-CBC; SQLite cookie read). |
| `src/auth.rs` | `Session`: bearer extraction, `authz` exchange, region map, JWT identity claims. |
| `src/api.rs` | `ApiClient`: shared reqwest client, chat request builder, error mapping. |
| `src/api/chat.rs` | Chat-service (IC3) ops: conversations, messages, threads, post/edit/react. |
| `src/model.rs` | serde response types + result/status types. |
| `src/output.rs` | `DisplayOutput` trait, `render(value, json)`, list/message formatting. |
| `src/error.rs` | Typed `thiserror` errors: `CredsError`, `AuthError`, `ApiError`. |
| `src/commands/*.rs` | One module per noun; handlers return data values, never print. |
| `poc/` | Throwaway Python discovery scripts (credential PoC, endpoint/audience probes). |

## 3. Credential extraction (`creds.rs`, macOS)

- **Client**: New Teams (`com.microsoft.teams2`) runs an Edge WebView2 (`EBWebView`)
  → standard Chromium storage.
- **Cookie DB (default)**:
  `~/Library/Containers/com.microsoft.teams2/Data/Library/Application Support/Microsoft/MSTeams/EBWebView/WV2Profile_tfw/Cookies`
  (signed-in work profile; `--cookies` overrides). Copied to a temp file before
  reading (the app holds a lock) and deleted after.
- **Cookies used**: `authtoken` (wraps the AAD bearer) and `skypetoken_asm` (Skype
  token).
- **Decryption**: Chromium `v10`/`v11` values, **AES-128-CBC**, IV = 16 spaces.
  - Key = `PBKDF2-HMAC-SHA1(secret, salt="saltysalt", iterations=1003, len=16)`.
  - `secret` = Keychain generic password, service `"Microsoft Teams Safe Storage"`,
    account `"Microsoft Teams"`, read **in-process** via the `security-framework` crate
    (`passwords::get_generic_password`; GUI prompt on first access by the `xteams`
    binary itself; falls back to a service-only `ItemSearchOptions` search).
  - After PKCS7 unpad, Chromium ≥ M127 prepends a **32-byte `SHA256(host)`** to the
    plaintext; we return whichever candidate (with/without the 32-byte prefix) is
    valid printable UTF-8.
- On macOS `Local State` has **no** `os_crypt.encrypted_key` (that is the Windows
  DPAPI path — see §10).

## 4. Token model (`auth.rs`)

- The `authtoken` cookie value is URL-encoded `Bearer=<JWT>&Origin=...`; the JWT is
  extracted. Its audience is **`https://api.spaces.skype.com`**.
- **authz exchange**:
  `POST https://authsvc.teams.microsoft.com/v1.0/authz`
  with `Authorization: Bearer <aad>` and an empty JSON body →
  `{ tokens.skypeToken, region, regionGtms{...} }`.
  - `regionGtms` maps ~100 service names to hosts (`chatService`, `middleTier`,
    `chatServiceAggregator`, `substrate*`, …). String entries are retained in
    `Session.gtms`.
  - `chatService` = `https://<region>.ng.msg.teams.microsoft.com`. **Region is
    auto-discovered — never hardcode it.**
- `Session { skype_token, region, chat_service, gtms, identity }`.
- `identity` (upn/name/tenant/audience/exp) is decoded from the AAD JWT claims with
  **no signature verification** (display/metadata only).

## 5. API layer (`api.rs`, `api/chat.rs`)

- `ApiClient::chat(method, path)` builds `{chat_service}/v1/users/ME/{path}` with the
  header **`Authentication: skypetoken=<token>`** (note: `Authentication`, not
  `Authorization`).
- `ApiClient::exec` sends and maps any non-2xx to `ApiError::Http { endpoint, status,
  body }`.
- Conversation ids are percent-encoded into the path (they contain `:` `@` `;`).

### Endpoint reference (chat service / IC3)

| Op | Method + path | Notes |
|----|---------------|-------|
| List conversations | `GET conversations?view=msnp24Equivalent&pageSize=N&startTime=1` | Includes both chats and channels. |
| List messages | `GET conversations/{conv}/messages?pageSize=N&startTime=1` | |
| Read one message | `GET conversations/{conv}/messages/{id}` | |
| Read a thread | `GET` messages of `{conv};messageid={rootId}` | Root + replies for one thread (`thread read`). Channel threads are addressed by appending `;messageid=<root>` to the conversation id. |
| Post | `POST conversations/{target}/messages` | Body: `content`, `messagetype:"RichText/Html"`, `contenttype:"text"`, `imdisplayname`, `clientmessageid`. Reply → `target = {conv};messageid={root}`. |
| Edit | `PUT conversations/{conv}/messages/{id}` | Body adds `skypeeditedid:"{id}"`. |
| React | `PUT conversations/{conv}/messages/{id}/properties?name=emotions` | Body: `{emotions:{key:<emoji>, value:<epoch-ms>}}`. |

- **Server message id**: on POST, read from the `Location` response header (last path
  segment), falling back to `OriginalArrivalTime` in the body. Use *that* id for
  edit/react — not the echoed `clientmessageid`.
- **Threads**: every message carries `rootMessageId`; it is a thread **root** iff
  `id == rootMessageId`, otherwise a reply pointing at that root. `thread list` scans
  the flat message stream, selects the most-recent `-n` roots then orders them
  chronologically (earliest-first), and with `-a` fetches each root's replies via the
  `;messageid=` endpoint. `thread read <root>` returns one thread (root + replies)
  sorted chronologically.
- Plain text is HTML-escaped and `\n`→`<br>`; `--html` sends `text` verbatim.

## 6. Data & output

- `model.rs`: `Conversation` (+ `is_channel()` = id contains `@thread.tacv2`,
  `topic()`), `Message` (+ `root_message_id`/`sequence_id`, `is_thread_root()`,
  `time_key()`), `Thread` (`{ root, replies }`), `AuthStatus`, `MessageAction`, and
  response wrappers. All are `Serialize` (JSON).
- `output.rs` is the **only** module that writes to stdout:
  - `trait DisplayOutput { fn display_output(&self) -> String; }`
  - `render<T: Serialize + DisplayOutput>(value, json)` → JSON (`serde_json`
    pretty) or human text.
  - `MessageList` — built via `MessageList::new`, which stores messages
    **chronologically (earliest-first, latest-last)** so JSON (`#[serde(transparent)]`,
    full data) and human text share one order. `display_output` only *filters* empty/
    system messages — it never reorders. (Ordering lives in the data, not the renderer,
    so `-j` and text always agree.)
  - `ThreadList(Vec<Thread>)` — renders each thread's root, with replies indented
    beneath (when `-a`); transparent JSON is an array of `{ root, replies }`.
  - Blanket `impl DisplayOutput for Vec<T>`.
- **Business logic never prints.** `commands/*` handlers return values; the dispatcher
  calls `render`. Future color/table modes extend `output.rs` only.

## 7. Command dispatch (`commands.rs`)

`main` → `commands::dispatch(cli)` → per-noun `dispatch(verb, cookies, json)`:

- `auth` → `AuthStatus`
- `chat list` → `Vec<Conversation>` (channels excluded via `is_channel`)
- `channel list [team]` / `channel search <q>` → channels derived from the
  conversation list, filtered by case-insensitive substring on topic/id
- `message new/list/read/edit/react` → chat-service ops
- `thread list <conv> [-n] [-a]` → threads (roots via `list_threads`; `-a` adds each
  root's replies); `thread read <conv> <root>` → one thread chronologically
- `team`, `user` → deferred (§9)

### Deep-link resolution (`link.rs`)

Every `<conversation>` argument may instead be a Teams deep link (the
`https://teams.microsoft.com/l/…`, `https://teams.cloud.microsoft/l/…` or
`msteams:/l/…` URLs the desktop/web apps generate). `extract_teams_link_data` parses
one into `TeamsDeepLinkFields` — a **flat bag of optionals** (`kind`,
`conversation_id`, `message_id`, `parent_message_id`, `tenant_id`, …) so a caller
takes only what it needs regardless of the link kind; `resolve_conversation` returns
the conversation id (from the link, or the argument verbatim) plus the parsed fields.

- Ids are **not validated** — they are opaque strings handed to Teams. Both
  percent-encoded (`19%3A…%40thread.tacv2`) and literal (`19:…@thread.tacv2`) forms
  are decoded; `/l/chat/0/0` (new chat) yields no conversation id.
- When a command also takes a message id and the link carries one, the link fills it
  (an explicitly typed id still wins): a **specific message** uses the path id
  (`message_ref`); a **thread root** prefers `parentMessageId`, else the path id
  (`thread_ref`, used by `thread read` and `message new --reply-to`).
- Because a message link can supply the id, `read`/`edit`/`react`/`thread read` take
  the message-id positional as **optional**; `edit`/`react` reinterpret their trailing
  positional (`text`/`emoji`) accordingly. `message react`'s emoji is **mandatory**
  (no default). `link.rs` is pure and unit-tested (the one place with tests, since it
  needs no live backend).

## 8. Conventions / invariants

- No `unwrap`/`expect`/`panic` outside tests (clippy-denied in `Cargo.toml`). Use `?`
  + typed errors.
- `thiserror` at library boundaries; `eyre` (+ `color-eyre` reports) in the binary.
- ≤ 250 pure LOC per file; split by responsibility.
- Region/hosts come from `regionGtms` — never hardcode.
- Parse untrusted JSON into typed structs at the boundary (serde).

## 9. Deferred work (token-audience wall)

Only two audiences are available from the desktop cookies:

| Token | Audience | Unlocks |
|-------|----------|---------|
| `skypetoken` | (Skype/IC3) | chat service ✅ |
| `authtoken` (AAD) | `api.spaces.skype.com` | `authz` ✅, middle-tier (paths TBD) |

Blocked features need audiences we cannot currently obtain:

| Feature | Service host(s) | Needed audience | Finding |
|---------|-----------------|-----------------|---------|
| Team/channel roster, join | `chatsvcagg.teams.microsoft.com`, `teams.microsoft.com/api/csa/<region>` | `chatsvcagg.teams.microsoft.com` | **401** with skypetoken *and* AAD bearer. |
| User search | `substrate.office.com/search/api/v1/suggestions` | `substrate.office.com` | **401**. |
| Calendar | middle-tier / substrate | `api.spaces.skype.com` / substrate | middle-tier `.../api/mt/part/<region>-03` accepts our token, but teams/calendar paths returned **404** on guessed paths. |

- **No refresh token in WebView storage** (scanned Local Storage + IndexedDB: 0
  extractable JWTs). New Teams uses the native **OneAuth** broker; the refresh token
  lives in an encrypted OneAuth cache (Keychain/file), not found under common service
  names.
- **Path to unlock**: the OneAuth refresh-token cache is Keychain items scoped to the
  app's entitlement (`com.microsoft.oneauth.<oid>` — an unsigned CLI cannot read them),
  so direct extraction is blocked. The viable route is a **device-code + FOCI** login
  to obtain a family refresh token, then mint per-audience access tokens via the AAD
  `/token` endpoint. Full investigation brief + PoC/integration plan:
  **[docs/oneauth-handoff.md](docs/oneauth-handoff.md)**. Reference material:
  `poc/probe_*.py`, `poc/extract_teams_creds.py`.

## 10. Windows (future)

Same EBWebView layout, but cookies are **AES-256-GCM** with the key stored in
`Local State` → `os_crypt.encrypted_key`, DPAPI-unwrapped via `CryptUnprotectData`.
Add a `#[cfg(windows)]` path in `creds.rs`; the rest of the pipeline is unchanged.

## 11. Build / QA

```sh
cargo build
cargo clippy            # must be clean; unwrap/expect/panic are hard-denied
./target/debug/xteams auth   # smoke test against the live account
```

There is **no mock backend** — QA is done by running the binary against a real,
signed-in account. Test write operations against the private self-notes space
(`48:notes`), which is not visible to anyone else.
