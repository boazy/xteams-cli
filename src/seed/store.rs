//! File I/O for the m365 CLI credential store. Owns the on-disk paths and writes,
//! mapping failures to `SeedError`; business logic elsewhere never touches the disk.

use std::path::{Path, PathBuf};

use super::connection::{self, Connection};
use crate::error::SeedError;

const CONNECTION_FILE: &str = ".cli-m365-connection.json";
const ALL_CONNECTIONS_FILE: &str = ".cli-m365-all-connections.json";
const MSAL_FILE: &str = ".cli-m365-msal.json";

fn home() -> Result<PathBuf, SeedError> {
    etcetera::home_dir().map_err(|_| SeedError::HomeDir)
}

pub fn write_connection(conn: &Connection) -> Result<Vec<PathBuf>, SeedError> {
    write_connection_in(&home()?, conn)
}

fn write_connection_in(base: &Path, conn: &Connection) -> Result<Vec<PathBuf>, SeedError> {
    let name = conn
        .name
        .clone()
        .unwrap_or_else(|| conn.identity_id.clone());

    let conn_json = serde_json::to_string_pretty(conn).map_err(|e| SeedError::Serialize {
        what: "connection",
        detail: e.to_string(),
    })?;
    let conn_path = base.join(CONNECTION_FILE);
    write_file(&conn_path, &conn_json)?;

    let conn_value = serde_json::to_value(conn).map_err(|e| SeedError::Serialize {
        what: "connection",
        detail: e.to_string(),
    })?;
    let all_path = base.join(ALL_CONNECTIONS_FILE);
    let all = connection::all_connections_upsert(read_json(&all_path), conn_value, &name);
    let all_json = serde_json::to_string_pretty(&all).map_err(|e| SeedError::Serialize {
        what: "all-connections",
        detail: e.to_string(),
    })?;
    write_file(&all_path, &all_json)?;

    Ok(vec![conn_path, all_path])
}

pub fn write_msal_cache(additions: &serde_json::Value) -> Result<PathBuf, SeedError> {
    write_msal_cache_in(&home()?, additions)
}

fn write_msal_cache_in(base: &Path, additions: &serde_json::Value) -> Result<PathBuf, SeedError> {
    let path = base.join(MSAL_FILE);
    let mut cache = read_json(&path).unwrap_or_else(|| serde_json::json!({}));
    merge_sections(&mut cache, additions);
    let json = serde_json::to_string_pretty(&cache).map_err(|e| SeedError::Serialize {
        what: "msal-cache",
        detail: e.to_string(),
    })?;
    write_file(&path, &json)?;
    Ok(path)
}

fn merge_sections(cache: &mut serde_json::Value, additions: &serde_json::Value) {
    let (Some(dst), Some(src)) = (cache.as_object_mut(), additions.as_object()) else {
        return;
    };
    for (section, entries) in src {
        let Some(entries) = entries.as_object() else {
            continue;
        };
        let dst_section = dst
            .entry(section.clone())
            .or_insert_with(|| serde_json::json!({}));
        if let Some(dst_map) = dst_section.as_object_mut() {
            for (key, value) in entries {
                dst_map.insert(key.clone(), value.clone());
            }
        }
    }
}

fn write_file(path: &Path, contents: &str) -> Result<(), SeedError> {
    std::fs::write(path, contents).map_err(|e| SeedError::Write {
        path: path.display().to_string(),
        detail: e.to_string(),
    })
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn temp_base() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xteams-seed-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_connection_and_all_connections() {
        let base = temp_base();
        let conn = connection::build_connection(
            "tok",
            "exp",
            Some("u@c.com"),
            "oidZ",
            "tidZ",
            "app",
            "common",
        );
        let paths = write_connection_in(&base, &conn).unwrap();
        assert_eq!(paths.len(), 2);

        let conn_txt = std::fs::read_to_string(base.join(CONNECTION_FILE)).unwrap();
        let cv: serde_json::Value = serde_json::from_str(&conn_txt).unwrap();
        assert_eq!(cv["active"], serde_json::json!(true));
        assert_eq!(
            cv["accessTokens"]["https://graph.microsoft.com"]["accessToken"],
            serde_json::json!("tok")
        );

        let all_txt = std::fs::read_to_string(base.join(ALL_CONNECTIONS_FILE)).unwrap();
        let av: serde_json::Value = serde_json::from_str(&all_txt).unwrap();
        assert_eq!(av.as_array().map(|a| a.len()), Some(1));

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn all_connections_merge_preserves_others() {
        let base = temp_base();
        std::fs::write(
            base.join(ALL_CONNECTIONS_FILE),
            serde_json::json!([{ "name": "someone-else" }]).to_string(),
        )
        .unwrap();
        let conn =
            connection::build_connection("tok", "exp", None, "oidM", "tidM", "app", "common");
        write_connection_in(&base, &conn).unwrap();

        let all_txt = std::fs::read_to_string(base.join(ALL_CONNECTIONS_FILE)).unwrap();
        let av: serde_json::Value = serde_json::from_str(&all_txt).unwrap();
        let arr = av.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert!(
            arr.iter()
                .any(|c| c["name"] == serde_json::json!("someone-else"))
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn msal_merge_preserves_existing_and_adds_new() {
        let base = temp_base();
        std::fs::write(
            base.join(MSAL_FILE),
            serde_json::json!({
                "RefreshToken": { "existing-key": { "secret": "old" } },
                "Account": {},
            })
            .to_string(),
        )
        .unwrap();
        let additions = serde_json::json!({
            "RefreshToken": { "new-key": { "secret": "new", "family_id": "1" } },
            "AppMetadata": { "am-key": { "family_id": "1" } },
        });
        write_msal_cache_in(&base, &additions).unwrap();

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(base.join(MSAL_FILE)).unwrap()).unwrap();
        assert_eq!(
            v["RefreshToken"]["existing-key"]["secret"],
            serde_json::json!("old")
        );
        assert_eq!(
            v["RefreshToken"]["new-key"]["secret"],
            serde_json::json!("new")
        );
        assert_eq!(
            v["AppMetadata"]["am-key"]["family_id"],
            serde_json::json!("1")
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn msal_create_from_absent_is_valid() {
        let base = temp_base();
        let additions = serde_json::json!({ "RefreshToken": { "k": { "secret": "s" } } });
        write_msal_cache_in(&base, &additions).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(base.join(MSAL_FILE)).unwrap()).unwrap();
        assert_eq!(v["RefreshToken"]["k"]["secret"], serde_json::json!("s"));
        std::fs::remove_dir_all(&base).ok();
    }
}
