use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, params};
use serde::Serialize;

use super::PersistenceError;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectRecord {
    pub name: String,
    pub project_dir: PathBuf,
    pub source_language: String,
    pub target_language: String,
    pub last_ingest_status: String,
}

/// Registry of every project ever created by syntax-bridge, shared across
/// all projects and stored once per user (see `default_global_db_path`).
pub struct GlobalStore {
    connection: Connection,
}

impl GlobalStore {
    pub fn open(path: &Path) -> Result<Self, PersistenceError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                project_dir TEXT NOT NULL UNIQUE,
                source_language TEXT NOT NULL,
                target_language TEXT NOT NULL,
                created_at_millis INTEGER NOT NULL,
                last_opened_at_millis INTEGER NOT NULL,
                last_ingest_status TEXT NOT NULL,
                last_ingest_at_millis INTEGER NOT NULL
            );",
        )?;

        Ok(Self { connection })
    }

    /// Inserts a new project, or updates it in place when `project_dir`
    /// already exists (keyed by directory, since a project's directory is
    /// stable for its lifetime while its name or ingest status can change).
    pub fn register_project(
        &self,
        name: &str,
        project_dir: &Path,
        source_language: &str,
        target_language: &str,
        ingest_status: &str,
    ) -> Result<(), PersistenceError> {
        let now = now_millis();
        let project_dir = project_dir.to_string_lossy();

        self.connection.execute(
            "INSERT INTO projects (
                name, project_dir, source_language, target_language,
                created_at_millis, last_opened_at_millis, last_ingest_status, last_ingest_at_millis
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?5)
             ON CONFLICT(project_dir) DO UPDATE SET
                name = excluded.name,
                source_language = excluded.source_language,
                target_language = excluded.target_language,
                last_opened_at_millis = excluded.last_opened_at_millis,
                last_ingest_status = excluded.last_ingest_status,
                last_ingest_at_millis = excluded.last_ingest_at_millis",
            params![
                name,
                project_dir,
                source_language,
                target_language,
                now,
                ingest_status
            ],
        )?;

        Ok(())
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectRecord>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT name, project_dir, source_language, target_language, last_ingest_status
             FROM projects ORDER BY last_opened_at_millis DESC",
        )?;

        let records = statement
            .query_map([], |row| {
                Ok(ProjectRecord {
                    name: row.get(0)?,
                    project_dir: PathBuf::from(row.get::<_, String>(1)?),
                    source_language: row.get(2)?,
                    target_language: row.get(3)?,
                    last_ingest_status: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }

    /// Lists the `limit` most recently opened projects, for the app's entry
    /// screen: unlike `list_projects`, this only ever pulls `limit` rows out
    /// of the database rather than every project ever created.
    pub fn recent_projects(&self, limit: i64) -> Result<Vec<ProjectRecord>, PersistenceError> {
        let mut statement = self.connection.prepare(
            "SELECT name, project_dir, source_language, target_language, last_ingest_status
             FROM projects ORDER BY last_opened_at_millis DESC LIMIT ?1",
        )?;

        let records = statement
            .query_map(params![limit], |row| {
                Ok(ProjectRecord {
                    name: row.get(0)?,
                    project_dir: PathBuf::from(row.get::<_, String>(1)?),
                    source_language: row.get(2)?,
                    target_language: row.get(3)?,
                    last_ingest_status: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(records)
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use std::thread::sleep;
    use std::time::Duration;

    use super::*;
    use crate::persistence::test_support::temp_db_path;

    #[test]
    fn registers_and_lists_a_new_project() {
        let db_path = temp_db_path("global-register");
        let store = GlobalStore::open(&db_path).expect("open global store");

        store
            .register_project(
                "counter",
                Path::new("/tmp/syntax-bridge-projects/counter"),
                "cpp",
                "dart",
                "success",
            )
            .expect("register project");

        let projects = store.list_projects().expect("list projects");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "counter");
        assert_eq!(
            projects[0].project_dir,
            Path::new("/tmp/syntax-bridge-projects/counter")
        );
        assert_eq!(projects[0].source_language, "cpp");
        assert_eq!(projects[0].target_language, "dart");
        assert_eq!(projects[0].last_ingest_status, "success");

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn re_registering_the_same_project_dir_updates_instead_of_duplicating() {
        let db_path = temp_db_path("global-upsert");
        let store = GlobalStore::open(&db_path).expect("open global store");
        let project_dir = Path::new("/tmp/syntax-bridge-projects/counter");

        store
            .register_project("counter", project_dir, "cpp", "dart", "success")
            .expect("register project");
        store
            .register_project("counter-renamed", project_dir, "cpp", "dart", "failed")
            .expect("re-register project");

        let projects = store.list_projects().expect("list projects");
        assert_eq!(projects.len(), 1, "expected upsert, not duplicate row");
        assert_eq!(projects[0].name, "counter-renamed");
        assert_eq!(projects[0].last_ingest_status, "failed");

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn lists_projects_most_recently_opened_first() {
        let db_path = temp_db_path("global-order");
        let store = GlobalStore::open(&db_path).expect("open global store");

        store
            .register_project(
                "first",
                Path::new("/tmp/syntax-bridge-projects/first"),
                "cpp",
                "dart",
                "success",
            )
            .expect("register first project");
        sleep(Duration::from_millis(5));
        store
            .register_project(
                "second",
                Path::new("/tmp/syntax-bridge-projects/second"),
                "cpp",
                "dart",
                "success",
            )
            .expect("register second project");
        sleep(Duration::from_millis(5));
        store
            .register_project(
                "first",
                Path::new("/tmp/syntax-bridge-projects/first"),
                "cpp",
                "dart",
                "success",
            )
            .expect("re-open first project");

        let projects = store.list_projects().expect("list projects");
        let names: Vec<_> = projects
            .iter()
            .map(|project| project.name.as_str())
            .collect();
        assert_eq!(names, vec!["first", "second"]);

        let _ = fs::remove_file(&db_path);
    }

    #[test]
    fn recent_projects_limits_to_the_most_recently_opened() {
        let db_path = temp_db_path("global-recent-limit");
        let store = GlobalStore::open(&db_path).expect("open global store");

        for index in 0..7 {
            store
                .register_project(
                    &format!("project-{index}"),
                    Path::new(&format!("/tmp/syntax-bridge-projects/project-{index}")),
                    "cpp",
                    "dart",
                    "success",
                )
                .expect("register project");
            sleep(Duration::from_millis(5));
        }

        let recent = store.recent_projects(5).expect("list recent projects");
        let names: Vec<_> = recent.iter().map(|project| project.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["project-6", "project-5", "project-4", "project-3", "project-2"]
        );

        let _ = fs::remove_file(&db_path);
    }
}
