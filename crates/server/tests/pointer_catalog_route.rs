//! Exercises `GET /projects/pointers`, which serves the pointer catalog
//! (Parte 1 of `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`) already
//! persisted in `project.db`, without reparsing.
//!
//! Like `type_catalog_route.rs`, this doesn't need a real `libclang`: the
//! store is populated directly.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use syntax_bridge_server::persistence::ProjectStore;
use syntax_bridge_server::pointer_catalog::{
    PointerDeclaration, PointerDeclarationKind, PointerShape,
};
use syntax_bridge_server::server::SyntaxBridgeServer;

fn sample_declarations() -> Vec<PointerDeclaration> {
    vec![
        PointerDeclaration {
            kind: PointerDeclarationKind::Parameter,
            shape: PointerShape::Scalar,
            name: "forma".to_owned(),
            pointee_type_name: "Forma".to_owned(),
            pointee_usr: "c:@S@Forma".to_owned(),
            file: "/workspace/src/desenha.cpp".to_owned(),
            line: 5,
            column: 20,
            usr: "c:@F@desenha#*$@S@Forma#@forma".to_owned(),
        },
        PointerDeclaration {
            kind: PointerDeclarationKind::Local,
            shape: PointerShape::FunctionPointer,
            name: "callback".to_owned(),
            pointee_type_name: "void (int)".to_owned(),
            pointee_usr: String::new(),
            file: "/workspace/src/desenha.cpp".to_owned(),
            line: 6,
            column: 10,
            usr: String::new(),
        },
    ]
}

#[test]
fn returns_the_persisted_catalog_without_reparsing() {
    let workspace = TempWorkspace::new("pointers-route").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_pointer_declarations(&sample_declarations())
        .expect("persist pointer declarations");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/pointers?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_get(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    let pointers = json
        .get("pointers")
        .and_then(Value::as_array)
        .expect("response includes pointers array");
    assert_eq!(pointers.len(), 2, "unexpected response body: {body}");

    let parameter = pointers
        .iter()
        .find(|entry| entry["name"] == "forma")
        .expect("forma entry");
    assert_eq!(parameter["kind"], "parameter");
    assert_eq!(parameter["shape"], "scalar");
    assert_eq!(parameter["pointee_usr"], "c:@S@Forma");

    let function_pointer = pointers
        .iter()
        .find(|entry| entry["name"] == "callback")
        .expect("callback entry");
    assert_eq!(function_pointer["shape"], "function_pointer");
    assert_eq!(function_pointer["pointee_usr"], "");

    // `possible_types` rides alongside `pointers` in the same response —
    // where the solver's narrowing (B07/B08) becomes reachable outside a
    // test, not a separate round trip. Empty here (this fixture never
    // persisted a function catalog to narrow against), but the key itself
    // is part of the route's contract.
    let possible_types = json
        .get("possible_types")
        .and_then(Value::as_object)
        .expect("response includes possible_types object");
    assert!(
        possible_types.is_empty(),
        "no function catalog was persisted, so nothing should have narrowed: {possible_types:?}"
    );
}

#[test]
fn returns_not_found_for_a_project_without_a_database() {
    let workspace =
        TempWorkspace::new("pointers-route-missing").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/missing");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/pointers?project_dir={}",
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
