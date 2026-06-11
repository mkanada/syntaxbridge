#[derive(Debug, Clone)]
pub struct DiagnosticCheck {
    pub tool: String,
    pub status: DiagnosticStatus,
    pub path: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DiagnosticStatus {
    Ok,
    Failed,
}

impl DiagnosticCheck {
    fn ok(tool: &str, path: Option<String>, message: Option<String>) -> Self {
        Self {
            tool: tool.to_owned(),
            status: DiagnosticStatus::Ok,
            path,
            message,
        }
    }

    fn failed(tool: &str, message: String) -> Self {
        Self {
            tool: tool.to_owned(),
            status: DiagnosticStatus::Failed,
            path: None,
            message: Some(message),
        }
    }

    pub fn line(&self) -> String {
        match (&self.status, &self.message) {
            (DiagnosticStatus::Ok, _) => format!("Checking {}...ok", self.tool),
            (DiagnosticStatus::Failed, Some(message)) => {
                format!("Checking {}...failed: {message}", self.tool)
            }
            (DiagnosticStatus::Failed, None) => format!("Checking {}...failed", self.tool),
        }
    }
}

#[flutter_rust_bridge::frb(sync)]
pub fn run_startup_diagnostics() -> Vec<DiagnosticCheck> {
    vec![
        diagnostics_pipeline_check(),
        sqlite_check(),
        tree_sitter_cpp_check(),
    ]
}

fn diagnostics_pipeline_check() -> DiagnosticCheck {
    DiagnosticCheck::ok("diagnostics pipeline", None, None)
}

fn sqlite_check() -> DiagnosticCheck {
    match run_sqlite_probe() {
        Ok(version) => DiagnosticCheck::ok("SQLite", None, Some(version)),
        Err(error) => DiagnosticCheck::failed("SQLite", error),
    }
}

fn run_sqlite_probe() -> Result<String, String> {
    let connection = rusqlite::Connection::open_in_memory().map_err(|error| error.to_string())?;
    connection
        .execute(
            "CREATE TABLE diagnostic_probe (id INTEGER PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO diagnostic_probe (value) VALUES (?1)",
            ["ok"],
        )
        .map_err(|error| error.to_string())?;

    let value: String = connection
        .query_row("SELECT value FROM diagnostic_probe WHERE id = 1", [], |row| row.get(0))
        .map_err(|error| error.to_string())?;

    if value == "ok" {
        Ok(format!("SQLite {}", rusqlite::version()))
    } else {
        Err(format!("unexpected probe value: {value}"))
    }
}

fn tree_sitter_cpp_check() -> DiagnosticCheck {
    match run_tree_sitter_cpp_probe() {
        Ok(message) => DiagnosticCheck::ok("Tree-sitter C++", None, Some(message)),
        Err(error) => DiagnosticCheck::failed("Tree-sitter C++", error),
    }
}

fn run_tree_sitter_cpp_probe() -> Result<String, String> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_cpp::LANGUAGE.into())
        .map_err(|error| error.to_string())?;

    let tree = parser
        .parse("int main() { return 0; }", None)
        .ok_or_else(|| "parser returned no tree".to_owned())?;
    let root = tree.root_node();

    if root.kind() == "translation_unit" && !root.has_error() {
        Ok("parsed translation_unit".to_owned())
    } else {
        Err(format!(
            "unexpected root node: kind={}, has_error={}",
            root.kind(),
            root.has_error()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    #[test]
    fn diagnostic_line_formats_success() {
        let check = DiagnosticCheck::ok("SQLite", None, None);

        assert_eq!(check.line(), "Checking SQLite...ok");
    }

    #[test]
    fn diagnostic_line_formats_failure() {
        let check = DiagnosticCheck::failed("SQLite", "probe failed".to_owned());

        assert_eq!(check.line(), "Checking SQLite...failed: probe failed");
    }

    #[test]
    fn sqlite_probe_uses_embedded_library() {
        let message = run_sqlite_probe().expect("SQLite probe should pass");

        assert!(message.starts_with("SQLite "));
    }

    #[test]
    fn sqlite_probe_ignores_fake_host_sqlite3() {
        with_fake_host_bin("sqlite3", || {
            let message = run_sqlite_probe().expect("SQLite probe should pass");

            assert!(message.starts_with("SQLite "));
        });
    }

    #[test]
    fn tree_sitter_cpp_probe_parses_minimal_cpp() {
        let message = run_tree_sitter_cpp_probe().expect("Tree-sitter C++ probe should pass");

        assert_eq!(message, "parsed translation_unit");
    }

    #[test]
    fn tree_sitter_cpp_probe_ignores_fake_host_tree_sitter() {
        with_fake_host_bin("tree-sitter", || {
            let message = run_tree_sitter_cpp_probe().expect("Tree-sitter C++ probe should pass");

            assert_eq!(message, "parsed translation_unit");
        });
    }

    #[test]
    fn startup_diagnostics_include_t2_1_to_t2_3_checks() {
        let checks = run_startup_diagnostics();
        let lines: Vec<String> = checks.iter().map(DiagnosticCheck::line).collect();

        assert!(lines.contains(&"Checking diagnostics pipeline...ok".to_owned()));
        assert!(lines.contains(&"Checking SQLite...ok".to_owned()));
        assert!(lines.contains(&"Checking Tree-sitter C++...ok".to_owned()));
    }

    fn with_fake_host_bin(name: &str, test: impl FnOnce()) {
        let _guard = path_lock().lock().expect("PATH test lock should be available");
        let fake_dir = std::env::temp_dir().join(format!(
            "syntax-bridge-fake-host-bin-{}-{}",
            name,
            std::process::id()
        ));
        std::fs::create_dir_all(&fake_dir).expect("fake host bin dir should be created");

        let fake_tool = fake_dir.join(name);
        std::fs::write(&fake_tool, "#!/bin/sh\necho HOST_TOOL_USED\nexit 97\n")
            .expect("fake host tool should be written");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(&fake_tool)
                .expect("fake host tool metadata should be readable")
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&fake_tool, permissions)
                .expect("fake host tool should be executable");
        }

        let original_path = std::env::var_os("PATH");
        let contaminated_path = match &original_path {
            Some(path) => {
                let mut paths = vec![fake_dir.clone()];
                paths.extend(std::env::split_paths(path));
                std::env::join_paths(paths).expect("contaminated PATH should be valid")
            }
            None => fake_dir.clone().into_os_string(),
        };

        std::env::set_var("PATH", contaminated_path);
        test();

        match original_path {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }

    fn path_lock() -> &'static Mutex<()> {
        static PATH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        PATH_LOCK.get_or_init(|| Mutex::new(()))
    }
}
