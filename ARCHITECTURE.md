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

**Two entry paths, unified into one `Session`:**

1. **FRT-first (`xteams login`)** — when a FOCI family refresh token (FRT) is cached on
   disk, mint an `api.spaces.skype.com` token and POST it to `authz` to derive the
   skypetoken + regional service map. **No cookies, no Teams app** required (world 2).
2. **Cookie fallback** — otherwise **creds** reads the Chromium `Cookies` SQLite from the
   signed-in WebView profile (key derived from the macOS Keychain), extracts the AAD
   bearer from `authtoken`, and posts it to the same `authz` endpoint. Silent, but limited
   to the chat service (world 1).

Both paths yield the same `Session` (skypetoken + region + `regionGtms`). **api** then
calls the regional chat service with the skypetoken; bearer features
(`team`/`user`/`calendar`) mint per-audience tokens from the FRT. **output** — commands
return typed values; a single renderer prints human text or JSON.

## 2. Module map

| File | Responsibility |
|------|----------------|
| `src/main.rs` | Entry; `#[tokio::main]`; parse CLI; dispatch. |
| `src/cli.rs` | clap derive: two-tier `<noun> <verb>` tree + global `--cookies`, `-j/--json`. |
| `src/link.rs` | Parse Teams deep links (`/l/…` URLs) into `TeamsDeepLinkFields`; resolve a conversation argument that may be a link. |
| `src/creds.rs` | Cookie-extraction root: shared `TeamsCookies`, SQLite read + value post-processing; dispatches to a per-OS `imp`. |
| `src/creds/macos.rs` | macOS (tested): Keychain secret → PBKDF2-HMAC-SHA1 → AES-128-CBC. |
| `src/creds/windows.rs` | Windows (**untested**): `Local State` key → DPAPI unwrap → AES-256-GCM. |
| `src/creds/unsupported.rs` | Other platforms: cookie extraction unavailable → `CredsError::UnsupportedPlatform` (use `xteams auth login`). |
| `src/auth.rs` | Orchestration root: FRT-first `connect` (else cookie fallback); `build_client`; `SPACES_RESOURCE`; re-exports + `load/login_authenticator` helpers. |
| `src/auth/session.rs` | `Session` (skypetoken + region + gtms + identity + credential tag); cookie `establish`; FRT `from_skype_session`; shared `authz` POST + response parsing. |
| `src/auth/authenticator.rs` | `Authenticator`: FRT → per-audience bearer tokens + skype session, backed by the on-disk cache (lock-free reads; mutations under `CacheLock`); `invalidate_credential`; `logout`. |
| `src/auth/token_cache.rs` | Pure cache model (`TokenCache`, `StoredAccessToken`, `StoredSkypeSession`) + validity/invalidation (unit-tested; no I/O, no clock). |
| `src/auth/token_cache_io.rs` | Store path via `etcetera::choose_app_strategy` (XDG state dir — `~/.local/state/xteams/token-cache.json`, honoring `$XDG_STATE_HOME`; data dir on Windows), tolerant load, atomic `0600` save (temp+fsync+rename), delete. |
| `src/auth/lock.rs` | `AuthInteraction` (may-prompt) + `CacheLock`: single-writer lock (`create_new`); stale (>60s) → stderr prompt or `LockHeld`; self-only `Drop` release. |
| `src/auth/credential.rs` | `CachedCredential`/`SessionCredential` tags + `credential_to_invalidate` (scans the `eyre` chain for a rejected token). |
| `src/auth/jwt.rs` | Pure JWT claim decoding: identity (upn/name/tid), audience/TTL, `jwt_expiry`, shared `now_unix` (unit-tested). |
| `src/auth/oauth.rs` | Pure device-code/refresh logic: response parsing + poll-error classification (unit-tested). |
| `src/auth/device_code.rs` | Interactive device-code sign-in loop (prompt on stderr; polls the token endpoint). |
| `src/api.rs` | `ApiClient` (skypetoken chat) + shared `send_ok` non-2xx→error mapping. |
| `src/api/chat.rs` | Chat-service (IC3) ops: conversations, messages, threads, post/edit/react. |
| `src/api/csa.rs` | chatsvcagg (CSA) team/channel roster (`Authorization: Bearer`). |
| `src/api/substrate.rs` | substrate.office.com people search. |
| `src/api/calendar.rs` | Microsoft Graph calendar view. |
| `src/seed.rs` | `auth seed <target>`: mint a Graph token + identity, orchestrate the writers (`TokenType` = refresh/access). |
| `src/seed/connection.rs` | Pure builders for the m365 CLI `connection.json` (+ all-connections upsert); unit-tested. |
| `src/seed/msal_cache.rs` | Pure MSAL-cache key derivation + `build_cache` (family RT for silent renewal); unit-tested. |
| `src/seed/store.rs` | m365 store file I/O — connection + MSAL-cache writes/merge (home-dir paths) → `SeedError`. |
| `src/model.rs` | serde response types + result/status types. |
| `src/output.rs` | `DisplayOutput` trait, `render(value, json)`, list/message formatting. |
| `src/error.rs` | Typed `thiserror` errors: `CredsError`, `AuthError`, `ApiError`, `OAuthError`, `TokenStoreError`, `SeedError`. |
| `src/commands/*.rs` | One module per noun; handlers return data values, never print. |
| `poc/` | Throwaway Python discovery scripts (credential PoC, endpoint/audience probes). |

## 3. Credential extraction (`creds/`)

`creds.rs` is a thin, platform-dispatching root. Reading the Chromium `Cookies`
SQLite (copy-to-temp → `SELECT` the two rows) and post-processing a decrypted value
(strip the optional 32-byte `SHA256(host)` M127 prefix, keep the printable-UTF-8
candidate) are **shared**; deriving the key and the cipher are per-OS, in an `imp`
submodule chosen by `#[cfg]`:

- **`creds/macos.rs`** (tested) — detailed below.
- **`creds/windows.rs`** (**untested**) — DPAPI + AES-256-GCM; see §11.
- **`creds/unsupported.rs`** — neither macOS nor Windows: every entry point returns
  `CredsError::UnsupportedPlatform`, so the crate still **builds** and `xteams auth
  login` (no cookies needed) works everywhere — only the cookie fallback is
  unavailable.

**Cookies used** (both paths): `authtoken` (wraps the AAD bearer) and `skypetoken_asm`
(Skype token).

### macOS (`creds/macos.rs`)

- **Client**: New Teams (`com.microsoft.teams2`) runs an Edge WebView2 (`EBWebView`)
  → standard Chromium storage.
- **Cookie DB (default)**:
  `~/Library/Containers/com.microsoft.teams2/Data/Library/Application Support/Microsoft/MSTeams/EBWebView/WV2Profile_tfw/Cookies`
  (signed-in work profile; `--cookies` overrides).
- **Decryption**: Chromium `v10`/`v11` values, **AES-128-CBC**, IV = 16 spaces.
  - Key = `PBKDF2-HMAC-SHA1(secret, salt="saltysalt", iterations=1003, len=16)`.
  - `secret` = Keychain generic password, service `"Microsoft Teams Safe Storage"`,
    account `"Microsoft Teams"`, read **in-process** via the `security-framework` crate
    (a **macOS-only dependency**; `passwords::get_generic_password`; GUI prompt on first
    access by the `xteams` binary itself; falls back to a service-only
    `ItemSearchOptions` search).
- On macOS `Local State` has **no** `os_crypt.encrypted_key` (that is the Windows
  DPAPI path — see §11).

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
- `Session { skype_token, region, chat_service, gtms, identity, credential }` — `credential`
  tags the source (FRT skype session vs cookie) so a chat 401 evicts the right cache entry.
- `identity` (upn/name/tid) is decoded from the backing AAD JWT (cookie `authtoken` or the
  FRT-minted spaces token) with **no signature verification** (display/metadata only).
- Both paths share the `authz` POST in `auth/session.rs`; a 401 on a **cached** spaces
  token (FRT path) becomes `AuthError::AuthzUnauthorized` (evict + re-mint), while a
  cookie-path 401 stays a plain `AuthError::Authz`.

### FRT / FOCI token path (`auth/`) — primary, persistent

`xteams login` runs an OAuth 2.0 **device-code** grant against the Teams **FOCI** public
client `1fec8e78-bce4-4aaf-ab1b-5451cc387264` (`auth/device_code.rs`: prints the code on
**stderr**, polls `…/oauth2/v2.0/token`). The resulting **family refresh token (FRT)** can
mint a token for *any* audience — including `api.spaces.skype.com`, which drives the chat
service via `authz` — so once signed in, xteams needs **no cookies and no Teams app**
(world 2). Without an FRT it falls back to cookies (world 1: silent, chat-only).

**Persistent token cache** (`auth/token_cache.rs` + `auth/token_cache_io.rs`) — a single
JSON file in the XDG **state** dir via `etcetera` (`~/.local/state/xteams/token-cache.json`, honoring `$XDG_STATE_HOME`; the data dir on Windows), `0600`,
holding the FRT, every per-audience access token (with absolute expiry), the derived skype
session (skypetoken + region + `regionGtms` + expiry), and identity. A valid cached token
is used directly with **no network and no FRT refresh**; the FRT is redeemed (and possibly
rotated) only when a token is missing/expired. `xteams logout` deletes the file.

- **`Authenticator`** (`auth/authenticator.rs`) is disk-backed. `token_for(resource)`:
  read cache lock-free → if valid, return; else acquire the cache lock, **reload +
  double-check** (a sibling process may have just minted it), redeem
  (`grant_type=refresh_token`, `scope=<resource>/.default offline_access`), persist the new
  token + any rotated FRT, release. `skype_session()` derives + caches the skypetoken via
  spaces→`authz`; its expiry = `min(skypeToken.exp, spaces.exp)`, else the spaces exp, else
  a 45-min fallback. `region()` reuses it. Tenant defaults to `organizations`.
- **Concurrency (`auth/lock.rs`)** — every cache *mutation* runs under `CacheLock`, a
  `refresh.lock` file created with `create_new` (contents `{pid, started_at}`). A contender
  polls (~200ms); a **stale** lock (>60s) prompts on stderr to delete-and-continue when
  interactive (TTY & not `-j`), else returns `TokenStoreError::LockHeld`. `Drop` removes the
  lock only if it still holds our marker (never another process's). The all-valid read path
  takes no lock; atomic save (temp+fsync+rename) + the lock give lost-update-free
  read-modify-write.
- **401 invalidation** — a rejected cached token surfaces as `ApiError::Unauthorized`
  (any API) or `AuthError::AuthzUnauthorized` (authz on a cached spaces token), tagged with
  a `CachedCredential`. `commands::dispatch` wraps the run, scans the `eyre` chain
  (`credential_to_invalidate`), evicts exactly that entry under the lock, and asks the user
  to re-run — the next run re-mints it. An `invalid_grant` on the FRT itself clears the
  whole cache (sign in again).
- **Storage location:** the FRT is durable auth state, so it lives in the XDG **state**
  dir (via `etcetera::choose_app_strategy`), not a cache dir — deliberately, so a cache
  cleaner won't sign you out. Secure cross-platform storage (`keyring-core`) is a planned
  future option.
- The OneAuth broker's own refresh-token cache is **not** readable by an unsigned CLI
  (Keychain access-group scoped — confirmed via `SecItemCopyMatching`: `errSecItemNotFound`),
  which is why device-code is used instead of extracting it.

## 5. API layer (`api.rs`, `api/chat.rs`, bearer modules)

- `ApiClient::chat(method, path)` builds `{chat_service}/v1/users/ME/{path}` with the
  header **`Authentication: skypetoken=<token>`** (note: `Authentication`, not
  `Authorization`).
- `ApiClient::exec` sends and maps any non-2xx to `ApiError::Http { endpoint, status,
  body }` (via the shared `send_ok`).
- Conversation ids are percent-encoded into the path (they contain `:` `@` `;`).
- **Bearer features** (`api/csa.rs`, `api/substrate.rs`, `api/calendar.rs`) build
  `Authorization: Bearer <token>` requests via `Authenticator::authed(resource, method,
  url)` and reuse the same `send_ok` mapping.

### Endpoint reference (bearer services)

| Op | Method + path | Audience | Notes |
|----|---------------|----------|-------|
| Teams/channel roster | `GET https://teams.microsoft.com/api/csa/{region}/api/v1/teams/users/me/updates` | `chatsvcagg.teams.microsoft.com` | Response `teams[]`, each with `channels[]`; `{region}` from `authz` (never hardcoded). |
| People search | `POST https://substrate.office.com/search/api/v1/suggestions?scenario=powerbar` | `substrate.office.com` | Body `EntityRequests:[{Query:{QueryString,DisplayQueryString},EntityType:"People",Size,Fields}]` + `cvid`/`logicalId` UUIDs; headers `Origin`/`Referer` = `teams.microsoft.com` (**400** without these). Response `Groups[].Suggestions[]`. |
| Calendar | `GET https://graph.microsoft.com/v1.0/me/calendarView?startDateTime=&endDateTime=` | `graph.microsoft.com` (`Calendars.ReadWrite`) | `Prefer: outlook.timezone="UTC"`; response `value[]`. |

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
- `login` / `logout` → device-code sign-in / clear the stored refresh token (`AuthAction`)
- `auth seed m365 [-t refresh|access]` → seed the m365 CLI (default `refresh` injects the FOCI RT for silent renewal; `access` writes only a ~1 h token) (`SeedResult`); see §10
- `team list` / `team search <q>` → teams via CSA (`Vec<Team>`); `team join` still
  deferred (a write op; endpoint unverified)
- `user search <q>` → people via substrate (`Vec<Person>`)
- `calendar list [-d days]` → upcoming Graph events (`Vec<CalendarEvent>`)

The bearer commands (`team`/`user`/`calendar`) take `(verb, json)` — no cookies — and
build an `Authenticator` from the stored refresh token, erroring with
`OAuthError::NotLoggedIn` ("run `xteams login`") if absent.

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

## 9. Multi-audience tokens (device-code) — implemented

Desktop cookies yield only two audiences (`skypetoken`, `api.spaces.skype.com`).
Features needing other audiences now obtain them via the device-code/FOCI path
(§4, `auth/`):

| Feature | Command | Service host + audience | Status |
|---------|---------|-------------------------|--------|
| Teams / channel roster | `team list` / `team search` | `teams.microsoft.com/api/csa/<region>` — `chatsvcagg.teams.microsoft.com` | ✅ |
| Team join | `team join` | (CSA join endpoint) | ⏳ deferred — write op, endpoint unverified |
| People search | `user search` | `substrate.office.com/search/api/v1/suggestions` — `substrate.office.com` | ✅ |
| Calendar | `calendar list` | `graph.microsoft.com/v1.0/me/calendarView` — `graph.microsoft.com` (`Calendars.ReadWrite`) | ✅ |

Token / audience matrix:

| Token | Source | Audience(s) | Unlocks |
|-------|--------|-------------|---------|
| FOCI **family refresh token** | `xteams login` (device-code); cached in `token-cache.json` | any, redeemed per-audience | chatsvcagg, substrate, graph, **spaces → authz → skypetoken** |
| `skypetoken` (+ region + gtms) | `authz` (FRT spaces token, else cookie AAD); cached | Skype/IC3 | chat service |
| `authtoken` (AAD) | cookie fallback only | `api.spaces.skype.com` | `authz` |

- **Not extractable:** the OneAuth broker's refresh-token cache is Keychain items scoped
  to the app's entitlement (`com.microsoft.oneauth.<oid>`) — an unsigned CLI gets
  `errSecItemNotFound` even via the native `SecItemCopyMatching`, so device-code is used
  instead. The WebView storage holds no extractable refresh token either.
- Full investigation brief: **[docs/oneauth-handoff.md](docs/oneauth-handoff.md)**;
  discovery probes: `poc/mint_tokens.py`, `poc/probe_*.py`, `poc/extract_teams_creds.py`.

## 10. Credential seeding (`auth seed`)

`auth seed <target>` writes xteams' FOCI-minted tokens into *another* CLI's on-disk
credential store, so that tool can call Microsoft Graph without its own sign-in. Logic
lives in `src/seed/`; only `output::render` prints (returns a `SeedResult`, `-j` aware).

Current target: **m365** (pnp/cli-microsoft365). Both modes point m365 at the **same
client our tokens are issued by** — the Teams FOCI client `1fec8e78-…`
(`auth::FOCI_CLIENT`), `tenant = organizations`, `cloudType = Public`. This is mandatory
for `refresh`: AAD refuses to redeem the refresh token for any *other* client id
(`AADSTS700007`), even within the FOCI family, so a different client silently breaks
renewal.

`xteams auth seed m365 [-t|--token-type refresh|access]` (default **refresh**):

- Both modes mint a Graph access token (`Authenticator::token_for`, resource
  `https://graph.microsoft.com`), decode `oid`/`upn`/`tid` from the JWT
  (`auth::graph_identity`), and write an **active** `Connection` to
  `~/.cli-m365-connection.json` (mirrored into `~/.cli-m365-all-connections.json`) with the
  token under `accessTokens["https://graph.microsoft.com"]` (ISO-8601 `expiresOn`). m365's
  `ensureAccessToken` returns that token directly while unexpired — MSAL is not invoked —
  so the first call is instant.
- **`refresh` (default)** additionally injects the FOCI refresh token into m365's MSAL
  cache `~/.cli-m365-msal.json` (`src/seed/msal_cache.rs`): an `Account`, a family
  `RefreshToken` (`client_id = 1fec8e78`, `family_id = "1"`), and `AppMetadata`
  (`family_id = "1"`). When the bootstrap token expires, m365's `acquireTokenSilent` finds
  the family RT and redeems it at `login.microsoftonline.com/organizations/oauth2/v2.0/token`
  — **silent renewal for the refresh token's lifetime (~90 days), no re-seeding.** Cache
  key derivation mirrors `@azure/msal-node` (lowercased, family key uses `1`).
- **`access`** writes only the connection store (no MSAL cache); when the ~1 h token
  expires m365 cannot renew, so re-run `xteams auth seed m365 -t access` before then.
- **Scope:** the token carries the Teams first-party client's delegated Graph scopes;
  m365 commands needing a scope Teams lacks return HTTP 403. Only the Graph resource is
  seeded — m365 keys tokens per resource, so SharePoint/PowerApps/etc. are not covered.
- **Shared refresh token:** `refresh` mode places the *same* RT in both xteams'
  `token-cache.json` and m365's MSAL cache. If AAD rotates it on one side the other's copy
  may go stale; the FOCI family RT is long-lived, and re-running `auth seed m365` re-syncs
  both. (`Authenticator::refresh_token` reads the RT from the on-disk cache.)

Modules: `src/seed.rs` (orchestrator + token mint/identity + `TokenType` branch),
`src/seed/connection.rs` (pure `connection.json` builders), `src/seed/msal_cache.rs`
(pure MSAL-cache key derivation + `build_cache`), `src/seed/store.rs` (home-dir paths +
connection and MSAL-cache writes/merge → `SeedError`). All pure builders are unit-tested.

## 11. Windows (`creds/windows.rs`) — implemented, **untested**

Same EBWebView layout with Chromium's Windows crypto. Implemented from public
references but never run on a Windows host, so treat it as unverified.

- **Cookie DB**: `%LOCALAPPDATA%\Packages\MSTeams_8wekyb3d8bbwe\LocalCache\Microsoft\
  MSTeams\EBWebView\WV2Profile_tfw\Network\Cookies` (modern WebView2 ≥ v96; falls back
  to `WV2Profile_tfw\Cookies` on older installs). `--cookies` overrides.
- **Key**: base64 `os_crypt.encrypted_key` from `…\EBWebView\Local State`, minus the
  5-byte `DPAPI` prefix, unwrapped with `CryptUnprotectData` (via `windows-sys`) → a
  32-byte AES-256 key.
- **Decryption**: `v10`/`v11` values = 12-byte nonce ++ ciphertext ++ 16-byte GCM tag,
  **AES-256-GCM** (`aes-gcm`); then the shared M127 `SHA256(host)` handling.
- **Deps**: `windows-sys` + `aes-gcm`, gated to `cfg(windows)` in `Cargo.toml`
  (`security-framework` is likewise gated to `cfg(target_os = "macos")`), so other
  targets pull neither and still build.
- **Known gaps** (why it stays untested/limited):
  - **App-Bound Encryption (`v20`)**: Chromium ≥ 127 may store an
    `app_bound_encrypted_key` (extra SYSTEM/app-bound wrap) instead of `encrypted_key`;
    that path is **not** implemented — extraction then fails with a clear error that
    points at `xteams auth login`.
  - **File lock**: `ms-teams.exe`/`msedgewebview2.exe` hold the `Cookies` DB open, so
    the copy-to-temp read may fail unless Teams is closed.

Verified only to **compile** against real `windows-sys`/`aes-gcm` for
`x86_64-pc-windows-msvc` (isolated cross-check from macOS); runtime behavior is
unconfirmed. The rest of the pipeline (authz → skypetoken → chat service) is unchanged.

## 12. Build / QA

```sh
cargo build
cargo clippy            # must be clean; unwrap/expect/panic are hard-denied
./target/debug/xteams auth   # smoke test against the live account
```

There is **no mock backend** — QA is done by running the binary against a real,
signed-in account. Test write operations against the private self-notes space
(`48:notes`), which is not visible to anyone else.

- Signed-in (FRT) runs use `~/.local/state/xteams/token-cache.json`; `xteams logout` (or
  deleting that file) resets to a clean state, and the cookie fallback needs a signed-in
  New Teams.
- Routing/lock logic is testable offline without live creds: point `$XDG_STATE_HOME` at a
  temp dir with a `token-cache.json` (a bogus FRT exercises the FRT-first path and
  `invalid_grant` cleanup; a pre-seeded stale `refresh.lock` + `-j` exercises `LockHeld`).
