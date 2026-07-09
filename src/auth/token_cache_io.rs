//! On-disk I/O for the token store: an XDG-compliant location via
//! `etcetera::choose_app_strategy` — the state dir (`~/.local/state/xteams` on
//! Linux/macOS, honoring `$XDG_STATE_HOME`; the data dir on Windows, which has no state
//! dir). Provides a permission-guarded atomic write (sibling temp + fsync + rename,
//! `0600` file / `0700` dir), a tolerant load, and delete. The pure model + validity
//! rules live in `token_cache`; mutation is serialized by `lock::CacheLock`.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt as _;

use etcetera::app_strategy::{AppStrategy, AppStrategyArgs, choose_app_strategy};

use super::token_cache::TokenCache;
use crate::error::TokenStoreError;

const STORE_FILE: &str = "token-cache.json";

/// The per-app token-store directory. `choose_app_strategy` yields the `Xdg` strategy on
/// Linux/macOS (`state_dir` = `~/.local/state/xteams`, honoring `$XDG_STATE_HOME`) and
/// the `Windows` strategy on Windows (no state dir → fall back to its data dir). The FRT
/// is durable auth state, so a *state* dir — not a cache dir a cleaner might wipe — is the
/// right home.
pub fn store_dir() -> Result<PathBuf, TokenStoreError> {
    let strategy = choose_app_strategy(AppStrategyArgs {
        top_level_domain: "com".to_owned(),
        author: "xteams".to_owned(),
        app_name: "xteams".to_owned(),
    })
    .map_err(|_| TokenStoreError::StoreDir)?;
    Ok(strategy.state_dir().unwrap_or_else(|| strategy.data_dir()))
}

pub fn store_file() -> Result<PathBuf, TokenStoreError> {
    Ok(store_dir()?.join(STORE_FILE))
}

/// Load the store; `Ok(None)` when the file does not exist. A corrupt file is a hard
/// error (do not silently discard a possibly-valid FRT on a transient parse issue).
pub fn load() -> Result<Option<TokenCache>, TokenStoreError> {
    let path = store_file()?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(TokenStoreError::Read { path: show(&path), detail: e.to_string() }),
    };
    let cache = serde_json::from_slice(&bytes)
        .map_err(|e| TokenStoreError::Corrupt { path: show(&path), detail: e.to_string() })?;
    Ok(Some(cache))
}

/// Atomically persist the store: ensure a `0700` dir, write a sibling temp at `0600`,
/// fsync it, then rename over the target (rename is atomic on the same filesystem).
pub fn save(cache: &TokenCache) -> Result<(), TokenStoreError> {
    let dir = store_dir()?;
    ensure_dir(&dir)?;
    let path = dir.join(STORE_FILE);
    let json = serde_json::to_vec_pretty(cache)
        .map_err(|e| TokenStoreError::Write { path: show(&path), detail: e.to_string() })?;

    let tmp = dir.join(format!(".{STORE_FILE}.{}.tmp", uuid::Uuid::new_v4()));
    if let Err(detail) = write_private(&tmp, &json) {
        let _ = fs::remove_file(&tmp);
        return Err(TokenStoreError::Write { path: show(&tmp), detail });
    }
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        TokenStoreError::Write { path: show(&path), detail: e.to_string() }
    })
}

/// Delete the whole store (logout, or an FRT rejected as `invalid_grant`).
pub fn delete() -> Result<(), TokenStoreError> {
    let path = store_file()?;
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(TokenStoreError::Write { path: show(&path), detail: e.to_string() }),
    }
}

fn ensure_dir(dir: &Path) -> Result<(), TokenStoreError> {
    fs::create_dir_all(dir)
        .map_err(|e| TokenStoreError::Write { path: show(dir), detail: e.to_string() })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))
            .map_err(|e| TokenStoreError::Write { path: show(dir), detail: e.to_string() })?;
    }
    Ok(())
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())
}

fn show(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Point `$XDG_STATE_HOME` at a throwaway dir — `choose_app_strategy` uses the `Xdg`
    /// strategy on Linux/macOS, which honors it — so the round-trip never touches the real
    /// store. Serialized because it mutates a process-global env var.
    #[test]
    fn save_load_delete_round_trip_and_mode() {
        let base = std::env::temp_dir().join(format!("xteams-state-test-{}", uuid::Uuid::new_v4()));
        // SAFETY: single-threaded test; restored below.
        unsafe { std::env::set_var("XDG_STATE_HOME", &base) };

        assert!(load().expect("empty load ok").is_none());

        let cache = TokenCache::new("frt-xyz".to_owned());
        save(&cache).expect("save ok");

        let loaded = load().expect("load ok").expect("some");
        assert_eq!(loaded.refresh_token, "frt-xyz");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(store_file().unwrap()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "token store file must be owner-only");
        }

        delete().expect("delete ok");
        assert!(load().expect("post-delete load ok").is_none());

        unsafe { std::env::remove_var("XDG_STATE_HOME") };
        let _ = fs::remove_dir_all(&base);
    }
}
