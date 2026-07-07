# AGENTS.md

`xteams` is an unofficial Microsoft Teams CLI (Rust) that drives Teams' private HTTP
APIs using credentials extracted from the local New Teams desktop app.

## Read first

- **[ARCHITECTURE.md](ARCHITECTURE.md)** — how everything works: credential chain,
  token model, endpoints, module map, deferred work. **Read before changing code.**
- [README.md](README.md) — user-facing overview and commands.

## Golden rule

**Keep [ARCHITECTURE.md](ARCHITECTURE.md) in sync with the code.** Any change to the
credential/token flow, endpoints, module layout, output layer, command surface,
conventions, or deferred-feature status MUST be reflected in `ARCHITECTURE.md` in the
same change. Updating it is part of the definition of done.

## Conventions

- No `unwrap`/`expect`/`panic` outside tests (clippy-denied). Use `?` + typed errors
  (`thiserror` in libraries, `eyre` + `color-eyre` in the binary).
- ≤ 250 pure LOC per file; split by responsibility.
- Parse untrusted JSON into typed structs at the boundary (serde).
- **Business logic never prints.** Commands return typed values; only
  `output::render` / `DisplayOutput` write output, and every command must support
  `-j`/`--json`.
- Never hardcode the region or service hosts — read them from `regionGtms`.

## Build & QA

```sh
cargo build
cargo clippy                 # must be clean
./target/debug/xteams auth   # smoke test (needs a signed-in New Teams; Keychain prompt)
```

There is no mock backend — verify against the live account. Test writes against
`48:notes` (private self-notes). First run per rebuild triggers a Keychain prompt;
choose "Always Allow".
