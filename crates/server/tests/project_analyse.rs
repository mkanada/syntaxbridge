//! Exercises item 2 (`docs/prompts/2026-08-19-mudanca-interacao.md`):
//! ingestion persists only declarations, and the "Analyse" step
//! (`project_service::analyse_project`, `POST /projects/analyse`) persists
//! everything else — usages, dependencies, the call graph, IR, and the
//! pointer catalog.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use syntax_bridge_server::ingest::CreateProjectRequest;
use syntax_bridge_server::persistence::ProjectStore;
use syntax_bridge_server::project_service;
use syntax_bridge_server::server::SyntaxBridgeServer;

const CPP_SOURCE: &str = r#"
int answer() {
    return 42;
}

int main() {
    return answer() == 42 ? 0 : 1;
}
"#;

fn open_project_store(project_dir: &Path) -> ProjectStore {
    ProjectStore::open(&project_dir.join("project.db")).expect("open project store")
}

#[test]
fn create_project_persists_only_declarations() {
    let workspace = TempWorkspace::new("analyse-ingest-only").expect("create temp workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project");

    assert!(
        !project.is_analysed,
        "a freshly ingested project must not report itself as analysed"
    );

    let store = open_project_store(&project.project_dir);
    assert!(
        !store
            .list_function_declarations()
            .expect("list function declarations")
            .is_empty(),
        "ingestion must still persist function declarations"
    );
    assert!(
        !store
            .list_source_files()
            .expect("list source files")
            .is_empty(),
        "ingestion must still persist source files"
    );

    assert!(
        store.list_call_edges().expect("list call edges").is_empty(),
        "ingestion must not persist the call graph — that's Analyse's job"
    );
    assert!(
        store
            .list_type_dependencies()
            .expect("list type dependencies")
            .is_empty(),
        "ingestion must not persist type dependencies — that's Analyse's job"
    );
    assert!(
        store
            .list_pointer_declarations()
            .expect("list pointer declarations")
            .is_empty(),
        "ingestion must not persist the pointer catalog — that's Analyse's job"
    );
}

#[test]
fn analyse_project_persists_the_call_graph_and_pointer_catalog() {
    let workspace = TempWorkspace::new("analyse-full").expect("create temp workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project");

    project_service::analyse_project(&project.project_dir, None).expect("analyse project");

    assert!(
        project_service::is_project_analysed(&project.project_dir),
        "the marker file should exist once analysis succeeds"
    );

    let store = open_project_store(&project.project_dir);
    let call_edges = store.list_call_edges().expect("list call edges");
    assert!(
        call_edges
            .iter()
            .any(|edge| edge.caller_usr.contains("main")),
        "expected a call edge from main to answer: {call_edges:#?}"
    );
}

#[test]
fn analyse_project_reports_not_found_for_a_missing_project() {
    let workspace = TempWorkspace::new("analyse-missing").expect("create temp workspace");
    let missing_dir = workspace.path().join("does-not-exist");

    let result = project_service::analyse_project(&missing_dir, None);

    assert!(
        matches!(
            result,
            Err(project_service::AnalyseProjectError::NotFound(_))
        ),
        "expected NotFound, got {result:?}"
    );
}

#[test]
fn analyse_project_route_runs_as_a_background_job() {
    let workspace = TempWorkspace::new("analyse-route").expect("create temp workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(global_db_path);
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({ "project_dir": project.project_dir });
    let (start_status, start_body) = http_post(addr, "/projects/analyse", &body);
    assert!(
        start_status.starts_with("HTTP/1.1 202"),
        "unexpected response: {start_status} body={start_body}"
    );
    let start_json: Value = serde_json::from_str(&start_body).expect("parse start response");
    let job_id = start_json["job_id"].as_str().expect("job_id in response");

    let mut final_status = Value::Null;
    for _ in 0..200 {
        let query = format!("/projects/analyse-jobs/{job_id}");
        let (poll_status, poll_body) = http_get(addr, &query);
        assert!(poll_status.starts_with("HTTP/1.1 200"), "{poll_status}");
        let poll_json: Value = serde_json::from_str(&poll_body).expect("parse poll response");
        let status = poll_json["status"].as_str().unwrap_or_default().to_owned();
        if status != "running" && status != "cancelling" {
            final_status = poll_json;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    handle.shutdown().expect("stop test server");

    assert_eq!(
        final_status["status"], "succeeded",
        "unexpected final status: {final_status:?}"
    );
    assert!(project_service::is_project_analysed(&project.project_dir));
}

fn write_cmake_fixture_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    let source_dir = workspace.join("fixture");
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("main.cpp"), CPP_SOURCE)?;
    fs::write(
        source_dir.join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_analyse_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_analyse_fixture main.cpp)
"#,
    )?;

    let output = Command::new("tar")
        .arg("-czf")
        .arg(archive_path)
        .arg("-C")
        .arg(workspace)
        .arg("fixture")
        .output()?;
    assert_success(output);

    Ok(())
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn http_get(addr: SocketAddr, path_and_query: &str) -> (String, String) {
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "GET {path_and_query} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write GET request");

    read_http_response(stream)
}

fn http_post(addr: SocketAddr, path: &str, body: &Value) -> (String, String) {
    let body = body.to_string();
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "POST {path} HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write POST request");

    read_http_response(stream)
}

fn read_http_response(mut stream: TcpStream) -> (String, String) {
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");

    let status_line = response.lines().next().unwrap_or_default().to_owned();
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_owned();

    (format!("{status_line}\r\n"), body)
}

#[derive(Debug)]
struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(name: &str) -> io::Result<Self> {
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
