use std::path::{Path, PathBuf};

/// The marker file `syntax-bridge-server` writes at the root of every
/// project it creates (`project_service::is_openable_project`) — reused
/// here as the same kind of anchor `.git` is for git: walking up from `cwd`
/// until one is found tells the CLI which project a "no path given"
/// command should act on.
const PROJECT_MARKER: &str = "project.db";

/// Walks `start` and its ancestors looking for `PROJECT_MARKER`, the same
/// way `git` resolves a repository from any subdirectory of it. Returns the
/// first (innermost) directory that contains the marker, or `None` if no
/// ancestor does.
pub fn find_project_dir(start: &Path) -> Option<PathBuf> {
    let mut dir = start;
    loop {
        if dir.join(PROJECT_MARKER).is_file() {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempWorkspace {
        path: PathBuf,
    }

    impl TempWorkspace {
        fn new(name: &str) -> Self {
            let mut path = std::env::temp_dir();
            path.push(format!(
                "syntax-bridge-cli-{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).expect("create temp workspace");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempWorkspace {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn finds_the_project_when_the_marker_is_in_the_starting_directory() {
        let workspace = TempWorkspace::new("marker-here");
        fs::write(workspace.path().join("project.db"), b"").expect("write marker");

        assert_eq!(
            find_project_dir(workspace.path()),
            Some(workspace.path().to_path_buf())
        );
    }

    #[test]
    fn finds_the_project_by_walking_up_from_a_nested_subdirectory() {
        let workspace = TempWorkspace::new("nested");
        fs::write(workspace.path().join("project.db"), b"").expect("write marker");
        let nested = workspace.path().join("input-source").join("src");
        fs::create_dir_all(&nested).expect("create nested dir");

        assert_eq!(
            find_project_dir(&nested),
            Some(workspace.path().to_path_buf())
        );
    }

    #[test]
    fn finds_nothing_when_no_ancestor_has_the_marker() {
        let workspace = TempWorkspace::new("no-marker");
        let nested = workspace.path().join("a").join("b");
        fs::create_dir_all(&nested).expect("create nested dir");

        assert_eq!(find_project_dir(&nested), None);
    }
}
