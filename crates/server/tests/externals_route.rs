//! Exercises the "extern" HTTP surface (`docs/plans/lista-de-externos.md`):
//! `GET /projects/externals`, `POST /projects/externals/mark`,
//! `POST /projects/externals/mark-file`, `POST /projects/externals/mark-type`,
//! and the name/path regex add+remove routes. Mirrors
//! `function_catalog_route.rs`: the store is populated directly rather than
//! through a real `libclang` extraction.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use syntax_bridge_server::function_catalog::{
    CallEdge, CallResolution, FunctionDeclaration, FunctionDeclarationKind,
};
use syntax_bridge_server::persistence::{ExternalsStore, ProjectStore};
use syntax_bridge_server::server::SyntaxBridgeServer;
use syntax_bridge_server::type_catalog::{TypeDeclaration, TypeDeclarationKind};

fn sample_type() -> TypeDeclaration {
    TypeDeclaration {
        name: "Shape".to_owned(),
        kind: TypeDeclarationKind::Class,
        namespace: String::new(),
        file: "/workspace/src/shapes.h".to_owned(),
        line: 3,
        column: 7,
        end_line: 10,
        end_column: 1,
        usr: "c:@S@Shape".to_owned(),
    }
}

fn sample_undefined_function() -> FunctionDeclaration {
    FunctionDeclaration {
        name: "undef".to_owned(),
        kind: FunctionDeclarationKind::FreeFunction,
        namespace: String::new(),
        owning_class_usr: None,
        signature: "void undef()".to_owned(),
        file: "/workspace/src/third_party/vendor.h".to_owned(),
        line: 1,
        column: 6,
        end_line: 1,
        end_column: 13,
        usr: "c:@F@undef#".to_owned(),
        is_static: false,
        is_virtual: false,
        is_pure_virtual: false,
        is_defaulted: false,
        overridden_usrs: Vec::new(),
        has_definition: false,
    }
}

fn sample_call_to_undefined() -> CallEdge {
    CallEdge {
        caller_usr: "c:@F@caller#".to_owned(),
        resolution: CallResolution::Resolved {
            callee_usr: "c:@F@undef#".to_owned(),
            is_dynamic_dispatch: false,
        },
        file: "/workspace/src/caller.cpp".to_owned(),
        line: 5,
        column: 3,
    }
}

#[test]
fn list_externals_route_reports_manual_regex_and_auto_detected_sources() {
    let workspace = TempWorkspace::new("externals-route-list").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_type_declarations(&[sample_type()])
        .expect("persist type declarations");
    store
        .replace_function_declarations(&[sample_undefined_function()])
        .expect("persist function declarations");
    store
        .replace_call_edges(&[sample_call_to_undefined()])
        .expect("persist call edges");
    ExternalsStore::open(&project_dir)
        .add_path_regex("^/workspace/src/third_party/", "0")
        .expect("persist path regex");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/externals?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_get(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    let statuses = json
        .get("statuses")
        .and_then(Value::as_array)
        .expect("response includes statuses array");
    assert_eq!(statuses.len(), 1, "unexpected response body: {body}");
    assert_eq!(statuses[0]["usr"], "c:@F@undef#");
    assert_eq!(statuses[0]["effective"], true);
    let sources = statuses[0]["sources"]
        .as_array()
        .expect("sources is an array");
    let has_kind = |kind: &str| sources.iter().any(|source| source["kind"] == kind);
    assert!(has_kind("auto_undefined_function"), "{sources:?}");
    assert!(has_kind("path_regex"), "{sources:?}");

    let path_regexes = json
        .get("path_regexes")
        .and_then(Value::as_array)
        .expect("response includes path_regexes array");
    assert_eq!(path_regexes.len(), 1, "unexpected response body: {body}");
}

#[test]
fn mark_external_route_persists_a_manual_decision() {
    let workspace = TempWorkspace::new("externals-route-mark").expect("create temp workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");
    ProjectStore::open(&project_dir.join("project.db")).expect("open project store");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({
        "project_dir": project_dir,
        "usr": "c:@F@f#",
        "external": true,
    });
    let (status, _body) = http_post(addr, "/projects/externals/mark", &body);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status}"
    );

    let marks = ExternalsStore::open(&project_dir)
        .list_marks()
        .expect("list external marks");
    assert_eq!(marks.len(), 1);
    assert_eq!(marks[0].usr, "c:@F@f#");
    assert!(marks[0].external);
}

/// Item 3 (`docs/prompts/2026-08-19-mudanca-interacao.md`) reversed decision
/// 3's "cascata é foto" for files: marking a file external now creates a
/// persistent [`FileMark`] rather than expanding into per-usr marks, so the
/// declaration it covers (`Shape`) shows up as effective via the
/// `file_mark` source, and unmarking the file removes it again in one call.
#[test]
fn mark_file_external_route_persists_and_clears_a_file_mark() {
    let workspace = TempWorkspace::new("externals-route-mark-file").expect("create temp workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_type_declarations(&[sample_type()])
        .expect("persist type declarations");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let mark_body = serde_json::json!({
        "project_dir": project_dir,
        "file": "/workspace/src/shapes.h",
        "external": true,
    });
    let (mark_status, mark_response_body) =
        http_post(addr, "/projects/externals/mark-file", &mark_body);
    assert!(
        mark_status.starts_with("HTTP/1.1 200"),
        "unexpected response: {mark_status} body={mark_response_body}"
    );

    let file_marks = ExternalsStore::open(&project_dir)
        .list_file_marks()
        .expect("list file marks");
    assert_eq!(file_marks.len(), 1);
    assert_eq!(file_marks[0].file, "/workspace/src/shapes.h");

    let query = format!(
        "/projects/externals?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (list_status, list_body) = http_get(addr, &query);
    assert!(list_status.starts_with("HTTP/1.1 200"), "{list_status}");
    let json: Value = serde_json::from_str(&list_body).expect("parse response body");
    let statuses = json["statuses"].as_array().expect("statuses array");
    let shape = statuses
        .iter()
        .find(|status| status["usr"] == "c:@S@Shape")
        .expect("Shape should be in the effective set via its file mark");
    assert_eq!(shape["effective"], true);
    let sources = shape["sources"].as_array().expect("sources array");
    assert!(
        sources.iter().any(|source| source["kind"] == "file_mark"),
        "{sources:?}"
    );

    let unmark_body = serde_json::json!({
        "project_dir": project_dir,
        "file": "/workspace/src/shapes.h",
        "external": false,
    });
    let (unmark_status, _) = http_post(addr, "/projects/externals/mark-file", &unmark_body);
    handle.shutdown().expect("stop test server");
    assert!(unmark_status.starts_with("HTTP/1.1 200"), "{unmark_status}");

    assert!(
        ExternalsStore::open(&project_dir)
            .list_file_marks()
            .expect("list file marks")
            .is_empty(),
        "unmarking the file should remove its file mark"
    );
}

#[test]
fn mark_type_external_route_expands_and_marks_the_type_and_its_methods() {
    let workspace = TempWorkspace::new("externals-route-mark-type").expect("create temp workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let area = FunctionDeclaration {
        owning_class_usr: Some("c:@S@Shape".to_owned()),
        kind: FunctionDeclarationKind::Method,
        ..sample_undefined_function()
    };
    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_function_declarations(&[area])
        .expect("persist function declarations");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({
        "project_dir": project_dir,
        "type_usr": "c:@S@Shape",
    });
    let (status, response_body) = http_post(addr, "/projects/externals/mark-type", &body);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={response_body}"
    );
    let json: Value = serde_json::from_str(&response_body).expect("parse response body");
    let marked = json["marked_usrs"].as_array().expect("marked_usrs array");
    assert_eq!(marked.len(), 2, "{marked:?}");
    let marked_strings: Vec<&str> = marked.iter().filter_map(Value::as_str).collect();
    assert!(marked_strings.contains(&"c:@S@Shape"));
    assert!(marked_strings.contains(&"c:@F@undef#"));
}

#[test]
fn name_regex_route_adds_and_then_removes_a_rule() {
    let workspace = TempWorkspace::new("externals-route-regex").expect("create temp workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");
    ProjectStore::open(&project_dir.join("project.db")).expect("open project store");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let add_body = serde_json::json!({
        "project_dir": project_dir,
        "pattern": "^humlib::",
    });
    let (add_status, add_response) = http_post(addr, "/projects/externals/name-regex", &add_body);
    assert!(
        add_status.starts_with("HTTP/1.1 200"),
        "unexpected response: {add_status} body={add_response}"
    );
    let rule: Value = serde_json::from_str(&add_response).expect("parse response body");
    let id = rule["id"].as_i64().expect("response includes id");
    assert_eq!(rule["pattern"], "^humlib::");

    let remove_body = serde_json::json!({
        "project_dir": project_dir,
        "id": id,
    });
    let (remove_status, _) = http_post(addr, "/projects/externals/name-regex/remove", &remove_body);
    handle.shutdown().expect("stop test server");
    assert!(
        remove_status.starts_with("HTTP/1.1 200"),
        "unexpected response: {remove_status}"
    );

    assert!(
        ExternalsStore::open(&project_dir)
            .list_name_regexes()
            .expect("list name regexes")
            .is_empty()
    );
}

#[test]
fn add_name_regex_route_rejects_an_invalid_pattern() {
    let workspace =
        TempWorkspace::new("externals-route-invalid-regex").expect("create temp workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");
    ProjectStore::open(&project_dir.join("project.db")).expect("open project store");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({
        "project_dir": project_dir,
        "pattern": "(unclosed",
    });
    let (status, response_body) = http_post(addr, "/projects/externals/name-regex", &body);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 400"),
        "unexpected response: {status} body={response_body}"
    );

    assert!(
        ExternalsStore::open(&project_dir)
            .list_name_regexes()
            .expect("list name regexes")
            .is_empty(),
        "an invalid pattern must never be persisted"
    );
}

#[test]
fn externals_route_returns_not_found_for_a_project_without_a_database() {
    let workspace = TempWorkspace::new("externals-route-missing").expect("create temp workspace");
    let project_dir = workspace.path().join("projects/missing");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/externals?project_dir={}",
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
