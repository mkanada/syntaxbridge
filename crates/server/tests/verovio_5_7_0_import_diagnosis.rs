//! Diagnostic test for a hang reported when importing the real Verovio
//! 5.7.0 archive through the running app: "ele trava no meio da
//! importação" (it freezes partway through import), with nothing useful in
//! the logs.
//!
//! Not run by default: `#[ignore]`. It exercises
//! `project_service::create_project` — the exact function `POST /projects`
//! calls, including the full `libclang` type-catalog extraction over every
//! compilation unit — against the real
//! `test-resources/verovio-version-5.7.0.tar.gz` archive the user actually
//! used, on a background thread with a bounded timeout so a genuine hang
//! fails loudly with a clear message instead of blocking the test runner
//! forever.
//!
//! Run explicitly with:
//!   cargo test -p syntax-bridge-server --test verovio_5_7_0_import_diagnosis -- --ignored --nocapture

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use syntax_bridge_server::ingest::CreateProjectRequest;
use syntax_bridge_server::project_service;

#[test]
#[ignore = "slow/diagnostic: reproduces a reported hang importing the real Verovio 5.7.0 archive"]
fn create_project_completes_for_the_real_verovio_5_7_0_archive() {
    let archive_path = repo_root().join("test-resources/verovio-version-5.7.0.tar.gz");
    assert!(
        archive_path.is_file(),
        "expected fixture at {archive_path:?}"
    );

    let workspace =
        TempWorkspace::new("verovio-5-7-0-import-diagnosis").expect("create temporary workspace");
    let workspace_dir = workspace.path().join("projects");
    let global_db_path = workspace.path().join("global.db");

    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = project_service::create_project(
            CreateProjectRequest {
                name: "verovio-5-7-0".to_owned(),
                workspace_dir,
                archive_path,
            },
            &global_db_path,
            None,
        );
        let _ = sender.send(result);
    });

    match receiver.recv_timeout(Duration::from_secs(300)) {
        Ok(Ok(project)) => {
            eprintln!(
                "create_project succeeded: {} compilation units, {} types",
                project.compilation_units.len(),
                project.type_catalog.len()
            );
            assert!(
                !project.compilation_units.is_empty(),
                "expected compilation units: name={} project_dir={:?}",
                project.name,
                project.project_dir
            );
        }
        Ok(Err(error)) => panic!("create_project failed (not a hang): {error}"),
        Err(_) => panic!(
            "create_project did not finish within 300s — reproduces the reported hang \
             importing test-resources/verovio-version-5.7.0.tar.gz"
        ),
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve repo root from CARGO_MANIFEST_DIR")
}

#[derive(Debug)]
struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(name: &str) -> std::io::Result<Self> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "syntax-bridge-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path)?;

        Ok(Self { path })
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
