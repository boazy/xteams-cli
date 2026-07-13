//! Single-writer lock guarding every token-cache mutation. A refresh may rotate the
//! FRT, and the read-modify-write of the JSON must not interleave across concurrent
//! `xteams` processes. The lock is a sibling file created with `create_new`; a `Drop`
//! guard removes only our own marker. A stale lock (>60s) prompts on stderr when
//! interactive, else errors. `AuthInteraction` decides whether prompting is allowed.

use std::io::{IsTerminal as _, Write as _};
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tokio::time::sleep;

use super::jwt::now_unix;
use super::token_cache_io;
use crate::error::TokenStoreError;

const LOCK_FILE: &str = "refresh.lock";
const POLL: Duration = Duration::from_millis(200);
const STALE_AFTER_SECS: i64 = 60;

/// Whether auth flows may prompt the user on stderr. Off for `-j`/JSON output and
/// whenever stdin/stderr are not both a terminal (piped/non-interactive).
#[derive(Debug, Clone, Copy)]
pub struct AuthInteraction {
    pub allow_prompt: bool,
}

impl AuthInteraction {
    pub fn from_json(json: bool) -> Self {
        let tty = std::io::stderr().is_terminal() && std::io::stdin().is_terminal();
        Self {
            allow_prompt: !json && tty,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LockInfo {
    pid: u32,
    started_at: i64,
}

/// An acquired cache lock. Dropping it removes the lock file iff it still holds our
/// marker, so we never delete another process's freshly-created lock.
pub struct CacheLock {
    path: std::path::PathBuf,
    marker: String,
}

impl CacheLock {
    /// Block until the cache lock is held by us. Polls every 200ms while another
    /// process holds a fresh lock; a stale lock (>60s) prompts (when interactive) to
    /// delete-and-continue, otherwise returns `TokenStoreError::LockHeld`.
    pub async fn acquire(interaction: AuthInteraction) -> Result<Self, TokenStoreError> {
        let dir = token_cache_io::store_dir()?;
        std::fs::create_dir_all(&dir).map_err(|e| TokenStoreError::Lock {
            path: show(&dir),
            detail: e.to_string(),
        })?;
        let path = dir.join(LOCK_FILE);
        loop {
            match try_create(&path) {
                Ok(marker) => return Ok(Self { path, marker }),
                Err(TryCreate::Io(detail)) => {
                    return Err(TokenStoreError::Lock {
                        path: show(&path),
                        detail,
                    });
                }
                Err(TryCreate::Held) => {
                    if handle_contention(&path, interaction)? {
                        continue; // stale lock cleared — retry create immediately
                    }
                    sleep(POLL).await;
                }
            }
        }
    }
}

impl Drop for CacheLock {
    fn drop(&mut self) {
        if std::fs::read_to_string(&self.path).ok().as_deref() == Some(self.marker.as_str()) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

enum TryCreate {
    Held,
    Io(String),
}

fn try_create(path: &Path) -> Result<String, TryCreate> {
    let info = LockInfo {
        pid: std::process::id(),
        started_at: now_unix(),
    };
    let marker = serde_json::to_string(&info).unwrap_or_else(|_| info.pid.to_string());
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut file) => {
            file.write_all(marker.as_bytes())
                .map_err(|e| TryCreate::Io(e.to_string()))?;
            Ok(marker)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(TryCreate::Held),
        Err(e) => Err(TryCreate::Io(e.to_string())),
    }
}

/// Returns `Ok(true)` when a stale lock was removed (retry immediately), `Ok(false)`
/// when the lock is fresh (caller should poll), or an error when stale and we may not
/// (or were told not to) remove it.
fn handle_contention(path: &Path, interaction: AuthInteraction) -> Result<bool, TokenStoreError> {
    let Some(started_at) = lock_started_at(path) else {
        return Ok(true); // vanished between create attempt and stat — retry
    };
    let age = now_unix().saturating_sub(started_at);
    if age < STALE_AFTER_SECS {
        return Ok(false);
    }
    let since = format_unix(started_at);
    let age_text = humanize(age);
    if interaction.allow_prompt && prompt_delete(&show(path), &since, &age_text)? {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(e) => Err(TokenStoreError::Lock {
                path: show(path),
                detail: e.to_string(),
            }),
        };
    }
    Err(TokenStoreError::LockHeld {
        path: show(path),
        since,
        age: age_text,
    })
}

/// The lock's start time, preferring its JSON `started_at`, falling back to the file
/// mtime so an unparseable lock can still age out instead of deadlocking.
fn lock_started_at(path: &Path) -> Option<i64> {
    if let Ok(content) = std::fs::read_to_string(path)
        && let Ok(info) = serde_json::from_str::<LockInfo>(&content)
    {
        return Some(info.started_at);
    }
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    i64::try_from(secs).ok()
}

/// Blocking stderr prompt (the user is present in the rare stale-lock case). Mirrors
/// the stderr-only convention of `device_code`, keeping stdout clean for `-j`.
fn prompt_delete(path: &str, since: &str, age: &str) -> Result<bool, TokenStoreError> {
    let mut err = std::io::stderr();
    let _ = writeln!(
        err,
        "\nA credential-refresh lock at {path} has been held since {since} ({age})."
    );
    let _ = write!(
        err,
        "Another xteams may have crashed mid-refresh. Delete the lock and continue? [y/N] "
    );
    let _ = err.flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| TokenStoreError::Lock {
            path: path.to_owned(),
            detail: e.to_string(),
        })?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn humanize(age_secs: i64) -> String {
    let mins = age_secs / 60;
    if mins < 60 {
        format!("{mins}m old")
    } else {
        format!("{}h{}m old", mins / 60, mins % 60)
    }
}

fn format_unix(ts: i64) -> String {
    OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .and_then(|dt| dt.format(&Rfc3339).ok())
        .unwrap_or_else(|| format!("unix:{ts}"))
}

fn show(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn from_json_disables_prompt_for_json_output() {
        assert!(!AuthInteraction::from_json(true).allow_prompt);
    }

    #[test]
    fn humanize_switches_from_minutes_to_hours() {
        assert_eq!(humanize(0), "0m old");
        assert_eq!(humanize(59), "0m old");
        assert_eq!(humanize(65 * 60), "1h5m old");
        assert_eq!(humanize(3 * 3600), "3h0m old");
    }

    #[test]
    fn format_unix_is_rfc3339() {
        // 2021-01-01T00:00:00Z
        assert_eq!(format_unix(1_609_459_200), "2021-01-01T00:00:00Z");
    }
}
