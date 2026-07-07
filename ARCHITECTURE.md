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
    account `"Microsoft Teams"`, read via `/usr/bin/security -w` (GUI prompt on first
    access; falls back to service-only lookup).
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
| Thread replies | messages of `{conv};messageid={rootId}` | Channel threads are addressed by appending `;messageid=<root>` to the conversation id. |
| Post | `POST conversations/{target}/messages` | Body: `content`, `messagetype:"RichText/Html"`, `contenttype:"text"`, `imdisplayname`, `clientmessageid`. Reply → `target = {conv};messageid={root}`. |
| Edit | `PUT conversations/{conv}/messages/{id}` | Body adds `skypeeditedid:"{id}"`. |
| React | `PUT conversations/{conv}/messages/{id}/properties?name=emotions` | Body: `{emotions:{key:<emoji>, value:<epoch-ms>}}`. |

- **Server message id**: on POST, read from the `Location` response header (last path
  segment), falling back to `OriginalArrivalTime` in the body. Use *that* id for
  edit/react — not the echoed `clientmessageid`.
- Plain text is HTML-escaped and `\n`→`<br>`; `--html` sends `text` verbatim.

## 6. Data & output

- `model.rs`: `Conversation` (+ `is_channel()` = id contains `@thread.tacv2`,
  `topic()`), `Message`, `AuthStatus`, `MessageAction`, and response wrappers. All
  are `Serialize` (JSON).
- `output.rs` is the **only** module that writes to stdout:
  - `trait DisplayOutput { fn display_output(&self) -> String; }`
  - `render<T: Serialize + DisplayOutput>(value, json)` → JSON (`serde_json`
    pretty) or human text.
  - `MessageList(Vec<Message>)` — filters empty/system messages and sorts
    chronologically for human display; `#[serde(transparent)]` so JSON keeps the full
    data.
  - Blanket `impl DisplayOutput for Vec<T>`.
- **Business logic never prints.** `commands/*` handlers return values; the dispatcher
  calls `render`. Future color/table modes extend `output.rs` only.

## 7. Command dispatch (`commands.rs`)

`main` → `commands::dispatch(cli)` → per-noun `dispatch(verb, cookies, json)`:

- `auth` → `AuthStatus`
- `chat list` → `Vec<Conversation>` (channels excluded via `is_channel`)
- `channel list [team]` / `channel search <q>` → channels derived from the
  conversation list, filtered by case-insensitive substring on topic/id
- `message new/list/read/edit/react`, `thread list` → chat-service ops
- `team`, `user` → deferred (§9)

## 8. Conventions / invariants

- No `unwrap`/`expect`/`panic` outside tests (clippy-denied in `Cargo.toml`). Use `?`
  + typed errors.
- `thiserror` at library boundaries; `anyhow` in the binary.
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
- **Path to unlock**: extract a FOCI refresh token from the OneAuth cache, then mint
  per-audience access tokens via the AAD `/token` endpoint with the Teams first-party
  client id. Reference material: `poc/probe_*.py` (endpoint + audience probes),
  `poc/extract_teams_creds.py` (the working credential chain).

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
