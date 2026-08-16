mod global_store;
mod project_store;

pub use global_store::{GlobalStore, ProjectRecord};
pub use project_store::{ProjectCatalogs, ProjectStore};

use std::env;
use std::fmt;
use std::io;
use std::path::PathBuf;

/// Root directory for all syntax-bridge data, following the XDG base
/// directory spec. Under Flatpak this resolves under the sandboxed
/// `~/.var/app/<app-id>/data` automatically, since the sandbox sets
/// `XDG_DATA_HOME` accordingly.
pub fn default_data_dir() -> PathBuf {
    resolve_data_dir(env::var("XDG_DATA_HOME").ok(), env::var("HOME").ok())
}

pub fn default_global_db_path() -> PathBuf {
    default_data_dir().join("global.db")
}

fn resolve_data_dir(xdg_data_home: Option<String>, home: Option<String>) -> PathBuf {
    match xdg_data_home.filter(|value| !value.is_empty()) {
        Some(xdg_data_home) => PathBuf::from(xdg_data_home).join("syntax-bridge"),
        None => {
            let home = home.unwrap_or_else(|| ".".to_owned());
            PathBuf::from(home).join(".local/share/syntax-bridge")
        }
    }
}

#[derive(Debug)]
pub enum PersistenceError {
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Sqlite(error) => write!(formatter, "{error}"),
            Self::Json(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<io::Error> for PersistenceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for PersistenceError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for PersistenceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    pub(crate) fn temp_db_path(label: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "syntax-bridge-{label}-{}-{}.db",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_data_dir_from_xdg_data_home_when_set() {
        let resolved = resolve_data_dir(
            Some("/custom/data".to_owned()),
            Some("/home/user".to_owned()),
        );
        assert_eq!(resolved, PathBuf::from("/custom/data/syntax-bridge"));
    }

    #[test]
    fn falls_back_to_home_local_share_when_xdg_data_home_is_unset() {
        let resolved = resolve_data_dir(None, Some("/home/user".to_owned()));
        assert_eq!(
            resolved,
            PathBuf::from("/home/user/.local/share/syntax-bridge")
        );
    }

    #[test]
    fn ignores_empty_xdg_data_home() {
        let resolved = resolve_data_dir(Some(String::new()), Some("/home/user".to_owned()));
        assert_eq!(
            resolved,
            PathBuf::from("/home/user/.local/share/syntax-bridge")
        );
    }
}
