//! Exercises `GET /projects/types`, which serves the type catalog already
//! persisted in `project.db` without reparsing.
//!
//! Unlike `type_catalog.rs`, this doesn't need a real `libclang`: the store
//! is populated directly, the way `docs/plans/User Steps.md` (US-4's
//! testability notes, which apply here too) asks read routes to be tested —
//! independently of the extraction that fills them.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use syntax_bridge_server::persistence::ProjectStore;
use syntax_bridge_server::server::SyntaxBridgeServer;
use syntax_bridge_server::type_catalog::{TypeDeclaration, TypeDeclarationKind};

fn sample_declarations() -> Vec<TypeDeclaration> {
    vec![
        TypeDeclaration {
            name: "Point".to_owned(),
            kind: TypeDeclarationKind::Struct,
            namespace: "geometry".to_owned(),
            file: "/workspace/src/types.h".to_owned(),
            line: 3,
            column: 8,
            end_line: 6,
            end_column: 1,
        },
        TypeDeclaration {
            name: "ANSWER".to_owned(),
            kind: TypeDeclarationKind::ConstantMacro,
            namespace: String::new(),
            file: "/workspace/src/types.h".to_owned(),
            line: 1,
            column: 9,
            end_line: 1,
            end_column: 20,
        },
    ]
}

#[test]
fn returns_the_persisted_catalog_without_reparsing() {
    let workspace = TempWorkspace::new("types-route").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_type_declarations(&sample_declarations())
        .expect("persist type declarations");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/types?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_get(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    let types = json
        .get("types")
        .and_then(Value::as_array)
        .expect("response includes types array");
    assert_eq!(types.len(), 2, "unexpected response body: {body}");

    let names: Vec<&str> = types
        .iter()
        .map(|entry| entry["name"].as_str().expect("name is a string"))
        .collect();
    assert!(names.contains(&"Point"), "missing Point: {body}");
    assert!(names.contains(&"ANSWER"), "missing ANSWER: {body}");

    let point = types
        .iter()
        .find(|entry| entry["name"] == "Point")
        .expect("Point entry");
    assert_eq!(point["kind"], "struct");
}

#[test]
fn returns_not_found_for_a_project_without_a_database() {
    let workspace = TempWorkspace::new("types-route-missing").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/missing");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/types?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_get(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 404"),
        "unexpected response: {status} body={body}"
    );
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
