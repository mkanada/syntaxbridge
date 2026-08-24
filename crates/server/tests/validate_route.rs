//! Exercises `POST /projects/validate` (US-9) end to end over real HTTP —
//! mirrors `transpile_route.rs`'s shape and its persisted-IR shortcut.
//!
//! Like `transpile_route.rs`, `validate_route_reuses_persisted_ir_and_reports_origins`
//! seeds `ir_functions`/`ir_records` directly and points the compilation
//! unit at a file that doesn't exist on disk, so a reparse would fail
//! outright — a passing response with origin-translated diagnostics proves
//! both that the persisted IR was reused and that the C++ origin survived
//! the round trip through a real `dart analyze` subprocess.

use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use syntax_bridge_server::ingest::CompilationUnit;
use syntax_bridge_server::ir::{Field, Origin, Record, Type};
use syntax_bridge_server::persistence::ProjectStore;
use syntax_bridge_server::server::SyntaxBridgeServer;

#[test]
fn validate_route_reuses_persisted_ir_and_reports_origins() {
    let workspace =
        TempWorkspace::new("validate-route-persisted-ir").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/e03");
    let input_source_dir = project_dir.join("input-source");
    fs::create_dir_all(&input_source_dir).expect("create input-source dir");

    let missing_source_path = input_source_dir.join("ponto.cpp");
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

    // A record whose only field is `Type::Enum` referencing a usr this
    // `Module` never declares an `Enum` for (only `records` is persisted
    // below, no `enums`) — `emit::dart::emit_type` still prints the enum's
    // bare name (`Cor`), so the emitted class references an undeclared Dart
    // type. A real, deliberate `dart analyze` error, the same "package with
    // a real toolchain failure" shape `tests/validate_dart.rs` already
    // exercises without the HTTP route in front of it.
    let origin = Origin {
        file: missing_source_path.display().to_string(),
        line: 2,
        column: 1,
    };
    let persisted_record = Record {
        name: "Ponto".to_owned(),
        usr: "c:@S@Ponto".to_owned(),
        namespace: String::new(),
        fields: vec![Field {
            name: "cor".to_owned(),
            ty: Type::Enum {
                usr: "c:@E@Cor@not-persisted".to_owned(),
                name: "Cor".to_owned(),
            },
        }],
        static_fields: Vec::new(),
        constructors: Vec::new(),
        methods: Vec::new(),
        base_class: None,
        mixins: Vec::new(),
        library_base: None,
        destructor: None,
        origin: origin.clone(),
    };
    store
        .replace_ir(&[], &[persisted_record], &[])
        .expect("persist ir");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/validate?project_dir={}",
        percent_encode(&project_dir.display().to_string())
    );
    let (status, body) = http_post(addr, &query);
    handle.shutdown().expect("stop test server");

    assert!(
        status.starts_with("HTTP/1.1 200"),
        "unexpected response: {status} body={body}"
    );

    let json: Value = serde_json::from_str(&body).expect("parse response body");
    let diagnostics = json["diagnostics"]
        .as_array()
        .expect("diagnostics is an array");
    assert!(
        !diagnostics.is_empty(),
        "expected at least one dart analyze diagnostic, got: {body}"
    );

    // `Cor` was never persisted as its own `Enum` (only referenced by usr
    // from the field), so the analyzer's actual complaint is that the type
    // itself is unresolvable — still a real ERROR on `Ponto`'s own line,
    // which is what this test cares about proving: the diagnostic survives
    // the round trip to a real `dart analyze` subprocess and back to the
    // right C++ origin.
    let field_diagnostic = diagnostics
        .iter()
        .find(|d| d["message"].as_str().unwrap_or_default().contains("Cor"))
        .unwrap_or_else(|| panic!("expected a diagnostic about `Cor`, got: {body}"));
    assert_eq!(field_diagnostic["severity"], "error");
    assert_eq!(
        field_diagnostic["origin"]["file"],
        missing_source_path.display().to_string()
    );
    assert_eq!(field_diagnostic["origin"]["line"], 2);
}

#[test]
fn validate_route_returns_not_found_for_a_project_without_a_database() {
    let workspace =
        TempWorkspace::new("validate-route-missing").expect("create temporary workspace");
    let project_dir = workspace.path().join("projects/missing");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let query = format!(
        "/projects/validate?project_dir={}",
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
