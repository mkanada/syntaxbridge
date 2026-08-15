//! Exercises the hand-rolled HTTP/1.1 JSON client against a real,
//! ephemeral `syntax-bridge-server` instance — the same "spawn a real
//! server on `127.0.0.1:0`" pattern `crates/server/tests/server_health.rs`
//! uses, so this proves the client against the actual wire format instead
//! of a mock.

use syntax_bridge_cli::http::Client;
use syntax_bridge_server::server::SyntaxBridgeServer;

#[test]
fn get_reaches_the_health_endpoint_and_parses_the_json_body() {
    let server = SyntaxBridgeServer::bind("127.0.0.1:0").expect("bind test server");
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let client = Client::new(format!("http://{addr}"));
    let response = client.get("/health").expect("GET /health");

    assert_eq!(response.status, 200);
    assert_eq!(response.body["service"], "syntax-bridge-server");
    assert_eq!(response.body["status"], "ok");

    handle.shutdown().expect("stop test server");
}

#[test]
fn get_reports_a_connection_error_when_nothing_is_listening() {
    let client = Client::new("http://127.0.0.1:1");
    let error = client.get("/health").expect_err("connect should fail");

    assert!(
        error.to_string().to_lowercase().contains("connect"),
        "unexpected error: {error}"
    );
}

#[test]
fn post_json_reaches_the_open_project_route_and_reports_a_client_error_as_json() {
    let server = SyntaxBridgeServer::bind("127.0.0.1:0").expect("bind test server");
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let client = Client::new(format!("http://{addr}"));
    let response = client
        .post_json(
            "/projects/open",
            &serde_json::json!({ "project_dir": "/does/not/exist" }),
        )
        .expect("POST /projects/open");

    assert_eq!(response.status, 404);
    assert!(
        response.body["error"].is_string(),
        "expected an error field: {}",
        response.body
    );

    handle.shutdown().expect("stop test server");
}
