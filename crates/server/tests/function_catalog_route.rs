//! Exercises `GET /projects/functions` and `GET /projects/functions/callers`,
//! which serve the function/method/macro catalog and its call graph (US-5)
//! already persisted in `project.db`, without reparsing. Mirrors
//! `type_catalog_route.rs`: the store is populated directly rather than
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
use syntax_bridge_server::persistence::ProjectStore;
use syntax_bridge_server::server::SyntaxBridgeServer;

fn sample_declarations() -> Vec<FunctionDeclaration> {
    vec![
        FunctionDeclaration {
            name: "area".to_owned(),
            kind: FunctionDeclarationKind::Method,
            namespace: "geometry".to_owned(),
            owning_class_usr: Some("c:@N@geometry@S@Shape".to_owned()),
            signature: "double geometry::Shape::area() const".to_owned(),
            file: "/workspace/src/shapes.h".to_owned(),
            line: 4,
            column: 19,
            end_line: 4,
            end_column: 40,
            usr: "c:@N@geometry@S@Shape@F@area#1#".to_owned(),
            is_virtual: true,
            overridden_usrs: Vec::new(),
        },
        FunctionDeclaration {
            name: "add".to_owned(),
            kind: FunctionDeclarationKind::FreeFunction,
            namespace: String::new(),
            owning_class_usr: None,
            signature: "int add(int a, int b)".to_owned(),
            file: "/workspace/src/math.cpp".to_owned(),
            line: 1,
            column: 5,
            end_line: 3,
            end_column: 1,
            usr: "c:@F@add#I#I#".to_owned(),
            is_virtual: false,
            overridden_usrs: Vec::new(),
        },
    ]
}

fn sample_calls() -> Vec<CallEdge> {
    vec![
        CallEdge {
            caller_usr: "c:@F@compute#".to_owned(),
            resolution: CallResolution::Resolved {
                callee_usr: "c:@F@add#I#I#".to_owned(),
                is_dynamic_dispatch: false,
            },
            file: "/workspace/src/math.cpp".to_owned(),
            line: 20,
            column: 18,
        },
        CallEdge {
            caller_usr: "c:@F@describe#".to_owned(),
            resolution: CallResolution::Resolved {
                callee_usr: "c:@N@geometry@S@Shape@F@area#1#".to_owned(),
                is_dynamic_dispatch: true,
            },
            file: "/workspace/src/shapes.cpp".to_owned(),
            line: 10,
            column: 19,
        },
    ]
}

#[test]
fn returns_the_persisted_function_catalog_without_reparsing() {
    let workspace = TempWorkspace::new("functions-route").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_function_declarations(&sample_declarations())
        .expect("persist function declarations");
    store
        .replace_call_edges(&sample_calls())
        .expect("persist call edges");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/functions?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_get(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    let functions = json
        .get("functions")
        .and_then(Value::as_array)
        .expect("response includes functions array");
    assert_eq!(functions.len(), 2, "unexpected response body: {body}");

    let area = functions
        .iter()
        .find(|entry| entry["name"] == "area")
        .expect("area entry");
    assert_eq!(area["kind"], "method");
    assert_eq!(area["is_virtual"], true);

    let caller_counts = json
        .get("caller_counts")
        .and_then(Value::as_object)
        .expect("response includes caller_counts object");
    assert_eq!(
        caller_counts.get("c:@F@add#I#I#"),
        Some(&Value::from(1)),
        "unexpected caller_counts: {caller_counts:?}"
    );
}

#[test]
fn callers_route_returns_the_persisted_callers_for_a_function() {
    let workspace =
        TempWorkspace::new("functions-callers-route").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_call_edges(&sample_calls())
        .expect("persist call edges");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/functions/callers?project_dir={}&usr={}",
        percent_encode(&project_dir.display().to_string()),
        percent_encode("c:@N@geometry@S@Shape@F@area#1#"),
    );
    let (status, body) = http_get(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    let callers = json
        .get("callers")
        .and_then(Value::as_array)
        .expect("response includes callers array");
    assert_eq!(callers.len(), 1, "unexpected response body: {body}");
    assert_eq!(callers[0]["caller_usr"], "c:@F@describe#");
    assert_eq!(callers[0]["resolution"]["is_dynamic_dispatch"], true);
}

#[test]
fn calls_in_file_route_returns_the_persisted_calls_for_a_file() {
    let workspace =
        TempWorkspace::new("functions-calls-in-file-route").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/counter");
    fs::create_dir_all(&project_dir).expect("create project dir");

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_call_edges(&sample_calls())
        .expect("persist call edges");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/functions/calls-in-file?project_dir={}&file={}",
        percent_encode(&project_dir.display().to_string()),
        percent_encode("/workspace/src/math.cpp"),
    );
    let (status, body) = http_get(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    let calls = json
        .get("calls")
        .and_then(Value::as_array)
        .expect("response includes calls array");
    assert_eq!(calls.len(), 1, "unexpected response body: {body}");
    assert_eq!(calls[0]["caller_usr"], "c:@F@compute#");
    assert_eq!(calls[0]["file"], "/workspace/src/math.cpp");
}

#[test]
fn functions_route_returns_not_found_for_a_project_without_a_database() {
    let workspace =
        TempWorkspace::new("functions-route-missing").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/missing");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/functions?project_dir={}",
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
