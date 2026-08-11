//! Exercises source-file discovery (compilation units plus the project-local
//! headers they include) end to end, through `project_service::create_project`,
//! and the HTTP endpoint that serves a single file's content on demand.
//!
//! Like `type_catalog.rs`, this depends on a real `libclang` being loadable
//! (see `crates/server/src/source_catalog.rs`), so it should be run inside
//! the Flatpak sandbox via `scripts/test-in-flatpak.sh`.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use syntax_bridge_server::ingest::{CompilationUnit, CreateProjectRequest};
use syntax_bridge_server::progress::{Cancellation, ExtractionProgress};
use syntax_bridge_server::project_service;
use syntax_bridge_server::server::SyntaxBridgeServer;
use syntax_bridge_server::source_catalog::{self, SourceCatalogError, SourceFileKind};

const POINT_H: &str = r#"
#pragma once

namespace shapes {

struct Point {
    int x;
    int y;
};

Point make_point(int x, int y);

}
"#;

const POINT_CPP: &str = r#"
#include "shapes/point.h"

namespace shapes {

Point make_point(int x, int y) {
    return Point{x, y};
}

}
"#;

const SHAPE_H: &str = r#"
#pragma once

#include "shapes/point.h"

namespace shapes {

struct Shape {
    Point origin;
    int width;
    int height;
};

int shape_area(const Shape& shape);

}
"#;

const SHAPE_CPP: &str = r#"
#include "shapes/shape.h"

namespace shapes {

int shape_area(const Shape& shape) {
    return shape.width * shape.height;
}

}
"#;

const LOGGER_H: &str = r#"
#pragma once

namespace util {

void log_message(const char* message);

}
"#;

const LOGGER_CPP: &str = r#"
#include "util/logger.h"

#include <cstdio>

namespace util {

void log_message(const char* message) {
    std::printf("%s\n", message);
}

}
"#;

const MAIN_CPP: &str = r#"
#include "shapes/point.h"
#include "shapes/shape.h"
#include "util/logger.h"

int main() {
    shapes::Point origin = shapes::make_point(0, 0);
    shapes::Shape shape{origin, 4, 5};
    int area = shapes::shape_area(shape);
    util::log_message(area == 20 ? "ok" : "fail");
    return area == 20 ? 0 : 1;
}
"#;

const CMAKE_LISTS: &str = r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_source_files_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
include_directories(include)
add_executable(syntax_bridge_source_files_fixture
    src/main.cpp
    src/shapes/point.cpp
    src/shapes/shape.cpp
    src/util/logger.cpp
)
"#;

#[test]
fn create_project_lists_translation_units_and_headers_as_source_files() {
    let workspace = TempWorkspace::new("source-files").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "shapes".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest project and extract source files");

    let files = &project.source_files;

    let translation_units: Vec<&str> = files
        .iter()
        .filter(|file| file.kind == SourceFileKind::TranslationUnit)
        .map(|file| file.path.as_str())
        .collect();
    let headers: Vec<&str> = files
        .iter()
        .filter(|file| file.kind == SourceFileKind::Header)
        .map(|file| file.path.as_str())
        .collect();

    for expected in ["main.cpp", "point.cpp", "shape.cpp", "logger.cpp"] {
        assert!(
            translation_units
                .iter()
                .any(|path| path.ends_with(expected)),
            "expected {expected} among translation units: {translation_units:#?}"
        );
    }
    for expected in ["point.h", "shape.h", "logger.h"] {
        assert!(
            headers.iter().any(|path| path.ends_with(expected)),
            "expected {expected} among headers: {headers:#?}"
        );
    }

    assert_eq!(
        files.len(),
        7,
        "expected 4 translation units + 3 headers with no duplicates: {files:#?}"
    );

    let project_store = syntax_bridge_server::persistence::ProjectStore::open(
        &project.project_dir.join("project.db"),
    )
    .expect("open project store");
    let persisted = project_store
        .list_source_files()
        .expect("list persisted source files");
    assert_eq!(persisted.len(), files.len());
    for file in files {
        assert!(
            persisted.contains(file),
            "expected persisted source files to contain {file:?}"
        );
    }
}

/// US-4 criterion 7, mirroring `type_catalog`'s
/// `extract_type_catalog_stops_early_when_cancelled`: a pre-cancelled token
/// is the deterministic way to prove this pass actually stops instead of
/// merely accepting the parameter.
#[test]
fn extract_source_files_stops_early_when_cancelled() {
    let workspace = TempWorkspace::new("source-files-cancel").expect("create temporary workspace");
    let project_root = workspace.path().join("project");
    fs::create_dir_all(&project_root).expect("create project dir");

    let file_a = project_root.join("a.cpp");
    fs::write(&file_a, "int a;").expect("write a.cpp");

    let units = vec![CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_a.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    }];

    let progress = ExtractionProgress::new();
    let cancellation = Cancellation::new();
    cancellation.cancel();

    let result = source_catalog::extract_source_files_cancellable(
        &units,
        &project_root,
        Some(&progress),
        Some(&cancellation),
    );

    assert!(
        matches!(result, Err(SourceCatalogError::Cancelled)),
        "expected a cancelled result, got {result:?}"
    );
    assert_eq!(
        progress.completed(),
        0,
        "no unit should have been marked done once cancellation was already requested"
    );
}

#[test]
fn source_file_endpoint_returns_content_and_rejects_paths_outside_project() {
    let workspace = TempWorkspace::new("source-file-endpoint").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let create_body = serde_json::json!({
        "name": "shapes",
        "workspace_dir": workspace.path().join("projects"),
        "archive_path": archive_path,
    })
    .to_string();
    let (status, body) = http_post(addr, "/projects", &create_body);
    assert!(
        status.starts_with("HTTP/1.1 202"),
        "unexpected project creation response: {status}"
    );

    let start: Value = serde_json::from_str(&body).expect("parse job-start response body");
    let job_id = start["job_id"].as_str().expect("response includes job_id");
    let job = poll_job_until_done(addr, job_id);
    assert_eq!(job["status"], "succeeded", "unexpected job status: {job}");
    let created = &job["project"];
    let project_dir = created["project_dir"]
        .as_str()
        .expect("response includes project_dir");
    let source_files = created["source_files"]
        .as_array()
        .expect("response includes source_files");
    let shape_header = source_files
        .iter()
        .find(|file| {
            file["path"]
                .as_str()
                .is_some_and(|path| path.ends_with("shape.h"))
        })
        .expect("expected shape.h among source_files")["path"]
        .as_str()
        .expect("shape.h path is a string");

    let query = format!(
        "/projects/source-file?project_dir={}&path={}",
        percent_encode(project_dir),
        percent_encode(shape_header)
    );
    let (status, body) = http_get(addr, &query);
    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected source-file response: {status} body={body}"
    );
    let json: Value = serde_json::from_str(&body).expect("parse source-file response body");
    assert!(
        json["content"]
            .as_str()
            .is_some_and(|content| content.contains("shape_area")),
        "unexpected source-file content: {body}"
    );

    let outside_query = format!(
        "/projects/source-file?project_dir={}&path={}",
        percent_encode(project_dir),
        percent_encode("/etc/passwd")
    );
    let (status, _body) = http_get(addr, &outside_query);
    assert!(
        status.starts_with("HTTP/1.1 400"),
        "expected a path outside the project to be rejected: {status}"
    );

    handle.shutdown().expect("stop test server");
}

fn write_fixture_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    let source_dir = workspace.join("fixture");
    fs::create_dir_all(source_dir.join("include/shapes"))?;
    fs::create_dir_all(source_dir.join("src/shapes"))?;
    fs::create_dir_all(source_dir.join("src/util"))?;

    fs::write(source_dir.join("include/shapes/point.h"), POINT_H)?;
    fs::write(source_dir.join("include/shapes/shape.h"), SHAPE_H)?;
    fs::write(source_dir.join("src/shapes/point.cpp"), POINT_CPP)?;
    fs::write(source_dir.join("src/shapes/shape.cpp"), SHAPE_CPP)?;
    fs::write(source_dir.join("src/util/logger.h"), LOGGER_H)?;
    fs::write(source_dir.join("src/util/logger.cpp"), LOGGER_CPP)?;
    fs::write(source_dir.join("src/main.cpp"), MAIN_CPP)?;
    fs::write(source_dir.join("CMakeLists.txt"), CMAKE_LISTS)?;

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

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    encoded
}

fn http_post(addr: SocketAddr, path: &str, body: &str) -> (String, String) {
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

fn http_get(addr: SocketAddr, path_and_query: &str) -> (String, String) {
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "GET {path_and_query} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write GET request");

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

/// Polls `GET /projects/jobs/{job_id}` until the job leaves the `running`
/// state, for the fixture's small, effectively-instant creation. Bounded so
/// a real regression fails the test instead of hanging the suite.
fn poll_job_until_done(addr: SocketAddr, job_id: &str) -> Value {
    for _ in 0..200 {
        let (_, body) = http_get(addr, &format!("/projects/jobs/{job_id}"));
        let json: Value = serde_json::from_str(&body).expect("parse job status response body");

        if json["status"] != "running" {
            return json;
        }

        std::thread::sleep(Duration::from_millis(20));
    }

    panic!("job {job_id} did not finish within the polling budget");
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
