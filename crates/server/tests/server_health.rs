use std::io::{Read, Write};
use std::net::TcpStream;

use syntax_bridge_server::server::SyntaxBridgeServer;

#[test]
fn health_endpoint_reports_the_server_ready() {
    let server = SyntaxBridgeServer::bind("127.0.0.1:0").expect("bind test server");
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "GET /health HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected response: {response}"
    );
    assert!(
        response.contains(r#""service":"syntax-bridge-server""#),
        "response should identify the service: {response}"
    );
    assert!(
        response.contains(r#""status":"ok""#),
        "response should report readiness: {response}"
    );

    handle.shutdown().expect("stop test server");
}
