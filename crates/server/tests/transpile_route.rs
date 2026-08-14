//! Exercises `POST /projects/transpile` (PR2 of
//! `docs/plans/primeiro-corte-e01-e03.md`) end to end over real HTTP.
//!
//! Like `type_catalog_route.rs`/`function_catalog_route.rs`, this route now
//! has a persisted-IR shortcut (`ProjectStore::list_ir`, populated by
//! `project_service::create_project`): `transpile_route_returns_the_emitted_dart_package`
//! below seeds only compilation units (no persisted IR), so it exercises the
//! legacy-project fallback path and still needs a real C++ file under
//! `input-source` plus a real `libclang`.
//! `transpile_route_reuses_persisted_ir_instead_of_reparsing` instead seeds
//! `ir_functions`/`ir_records` directly and points the compilation unit at a
//! file that doesn't exist on disk — a reparse would fail outright, so a
//! passing response proves the persisted IR was actually reused. The
//! not-found case doesn't touch `libclang` at all, so it stays cheap like
//! the other routes' missing-database tests.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use syntax_bridge_server::ingest::CompilationUnit;
use syntax_bridge_server::ir::{BinaryOp, Expr, Function, Origin, Param, Stmt, Type};
use syntax_bridge_server::persistence::ProjectStore;
use syntax_bridge_server::server::SyntaxBridgeServer;

/// Identical to `examples/E01-funcao-aritmetica/input/src/aritmetica.cpp`.
const ARITMETICA_CPP: &str = r#"
int soma(int a, int b) {
    return a + b;
}
"#;

#[test]
fn transpile_route_returns_the_emitted_dart_package() {
    let workspace = TempWorkspace::new("transpile-route").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/e01");
    let input_source_dir = project_dir.join("input-source");
    fs::create_dir_all(&input_source_dir).expect("create input-source dir");

    let source_path = input_source_dir.join("aritmetica.cpp");
    fs::write(&source_path, ARITMETICA_CPP).expect("write aritmetica.cpp");

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_compilation_units(&[CompilationUnit {
            directory: input_source_dir.display().to_string(),
            file: source_path.display().to_string(),
            command: None,
            arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
        }])
        .expect("persist compilation units");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/transpile?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_post(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    assert_eq!(json["package_name"], "e01");
    assert_eq!(
        json["files"]["lib/aritmetica.dart"],
        "int soma(int a, int b) {\n  return a + b;\n}\n"
    );
    assert!(
        json["files"]["pubspec.yaml"]
            .as_str()
            .expect("pubspec.yaml is a string")
            .starts_with("name: e01\n")
    );

    // Also written to disk, not just returned inline.
    let written = fs::read_to_string(project_dir.join("transpiled/lib/aritmetica.dart"))
        .expect("read written lib/aritmetica.dart");
    assert_eq!(written, "int soma(int a, int b) {\n  return a + b;\n}\n");
}

/// Regression test: `transpile_project` used to always reparse every
/// compilation unit with `libclang`, discarding the IR project creation had
/// already computed — the same waste `list_types`/`list_functions` already
/// avoid for their own catalogs. Points the (only) compilation unit at a
/// `.cpp` file that was never written to disk, so a reparse attempt would
/// fail outright (`libclang` can't parse a file that doesn't exist); the
/// route succeeding, with output matching the *persisted* IR rather than
/// anything a reparse could have produced, proves the persisted IR was
/// actually what got used.
#[test]
fn transpile_route_reuses_persisted_ir_instead_of_reparsing() {
    let workspace =
        TempWorkspace::new("transpile-route-persisted-ir").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/e01");
    let input_source_dir = project_dir.join("input-source");
    fs::create_dir_all(&input_source_dir).expect("create input-source dir");

    let missing_source_path = input_source_dir.join("aritmetica.cpp");
    assert!(
        !missing_source_path.exists(),
        "the fixture must not write this file — a reparse must be impossible"
    );

    let mut store =
        ProjectStore::open(&project_dir.join("project.db")).expect("open project store");
    store
        .replace_compilation_units(&[CompilationUnit {
            directory: input_source_dir.display().to_string(),
            file: missing_source_path.display().to_string(),
            command: None,
            arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
        }])
        .expect("persist compilation units");

    let origin = Origin {
        file: missing_source_path.display().to_string(),
        line: 2,
        column: 1,
    };
    let persisted_function = Function {
        name: "soma".to_owned(),
        usr: "c:@F@soma#I#I#".to_owned(),
        params: vec![
            Param {
                name: "a".to_owned(),
                ty: Type::Int,
                default_value: None,
            },
            Param {
                name: "b".to_owned(),
                ty: Type::Int,
                default_value: None,
            },
        ],
        return_type: Type::Int,
        body: vec![Stmt::Return {
            value: Some(Expr::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(Expr::Ref {
                    name: "a".to_owned(),
                    ty: Type::Int,
                    origin: origin.clone(),
                }),
                rhs: Box::new(Expr::Ref {
                    name: "b".to_owned(),
                    ty: Type::Int,
                    origin: origin.clone(),
                }),
                ty: Type::Int,
                origin: origin.clone(),
            }),
            origin: origin.clone(),
        }],
        origin,
    };
    store
        .replace_ir(&[persisted_function], &[])
        .expect("persist ir");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/transpile?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_post(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    assert_eq!(
        json["files"]["lib/aritmetica.dart"],
        "int soma(int a, int b) {\n  return a + b;\n}\n"
    );
}

#[test]
fn transpile_route_returns_not_found_for_a_project_without_a_database() {
    let workspace =
        TempWorkspace::new("transpile-route-missing").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/missing");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/transpile?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_post(addr, &query);
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

fn http_post(addr: SocketAddr, path_and_query: &str) -> (String, String) {
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "POST {path_and_query} HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
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
