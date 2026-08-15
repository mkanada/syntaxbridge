//! A minimal blocking HTTP/1.1 JSON client, written by hand instead of
//! pulling in a crate like `reqwest`: every route this CLI talks to
//! (`crates/server/src/server.rs`) is a small, non-streamed JSON
//! request/response, the same shape `crates/server/tests` already exercise
//! over a raw `TcpStream` — so a client purpose-built for that shape is a
//! few dozen lines, not a new dependency to vendor.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use serde_json::Value;

/// How long to wait on the underlying socket before giving up — covers
/// both connecting and reading the response. Generous because project
/// creation can involve a `libclang` pass, but callers of that route poll a
/// job id (see `commands::init`) rather than block on a single request.
const IO_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Client {
    base_url: String,
}

#[derive(Debug)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug)]
pub enum ClientError {
    InvalidUrl(String),
    Connect(String, io::Error),
    Io(io::Error),
    MalformedResponse(String),
    InvalidJsonBody(serde_json::Error),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClientError::InvalidUrl(url) => write!(f, "invalid server URL: {url}"),
            ClientError::Connect(addr, error) => {
                write!(f, "could not connect to {addr}: {error}")
            }
            ClientError::Io(error) => write!(f, "I/O error talking to the server: {error}"),
            ClientError::MalformedResponse(detail) => {
                write!(f, "malformed response from the server: {detail}")
            }
            ClientError::InvalidJsonBody(error) => {
                write!(f, "server response was not valid JSON: {error}")
            }
        }
    }
}

impl std::error::Error for ClientError {}

impl Client {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
        }
    }

    pub fn get(&self, path_and_query: &str) -> Result<HttpResponse, ClientError> {
        self.request("GET", path_and_query, None)
    }

    pub fn post_json(&self, path: &str, body: &Value) -> Result<HttpResponse, ClientError> {
        self.request("POST", path, Some(body))
    }

    pub fn delete_json(&self, path: &str, body: &Value) -> Result<HttpResponse, ClientError> {
        self.request("DELETE", path, Some(body))
    }

    fn request(
        &self,
        method: &str,
        path_and_query: &str,
        body: Option<&Value>,
    ) -> Result<HttpResponse, ClientError> {
        let authority = self
            .base_url
            .strip_prefix("http://")
            .ok_or_else(|| ClientError::InvalidUrl(self.base_url.clone()))?;

        let mut stream = TcpStream::connect(authority)
            .map_err(|error| ClientError::Connect(authority.to_owned(), error))?;
        stream
            .set_read_timeout(Some(IO_TIMEOUT))
            .map_err(ClientError::Io)?;
        stream
            .set_write_timeout(Some(IO_TIMEOUT))
            .map_err(ClientError::Io)?;

        let body_bytes = body.map(|value| serde_json::to_vec(value).unwrap_or_default());

        let mut request = format!(
            "{method} {path_and_query} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n"
        );
        if let Some(bytes) = &body_bytes {
            request.push_str("Content-Type: application/json\r\n");
            request.push_str(&format!("Content-Length: {}\r\n", bytes.len()));
        }
        request.push_str("\r\n");

        stream
            .write_all(request.as_bytes())
            .map_err(ClientError::Io)?;
        if let Some(bytes) = &body_bytes {
            stream.write_all(bytes).map_err(ClientError::Io)?;
        }

        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).map_err(ClientError::Io)?;

        parse_response(&raw)
    }
}

fn parse_response(raw: &[u8]) -> Result<HttpResponse, ClientError> {
    let separator = b"\r\n\r\n";
    let split_at = raw
        .windows(separator.len())
        .position(|window| window == separator)
        .ok_or_else(|| {
            ClientError::MalformedResponse("no header/body separator found".to_owned())
        })?;

    let head = std::str::from_utf8(&raw[..split_at])
        .map_err(|error| ClientError::MalformedResponse(error.to_string()))?;
    let body_bytes = &raw[split_at + separator.len()..];

    let status_line = head
        .lines()
        .next()
        .ok_or_else(|| ClientError::MalformedResponse("empty response".to_owned()))?;
    let status = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| {
            ClientError::MalformedResponse(format!("unparseable status line: {status_line}"))
        })?;

    let body = if body_bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(body_bytes).map_err(ClientError::InvalidJsonBody)?
    };

    Ok(HttpResponse { status, body })
}

/// Percent-encodes a single query parameter value (RFC 3986 unreserved
/// characters kept as-is, everything else escaped) — needed because
/// `project_dir` and file paths routinely contain `/` and spaces, which a
/// raw query string can't carry.
pub fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char)
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub fn build_query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(key, value)| format!("{key}={}", percent_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_keeps_unreserved_characters_and_escapes_the_rest() {
        assert_eq!(percent_encode("abcXYZ019-_.~"), "abcXYZ019-_.~");
        assert_eq!(percent_encode("/tmp/my project"), "%2Ftmp%2Fmy%20project");
    }

    #[test]
    fn build_query_joins_encoded_pairs_with_ampersands() {
        assert_eq!(
            build_query(&[("project_dir", "/tmp/a b"), ("usr", "c:@F@add")]),
            "project_dir=%2Ftmp%2Fa%20b&usr=c%3A%40F%40add"
        );
    }

    #[test]
    fn parse_response_reads_status_and_json_body() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"a\":1}";
        let response = parse_response(raw).expect("parse response");
        assert_eq!(response.status, 200);
        assert_eq!(response.body["a"], 1);
    }

    #[test]
    fn parse_response_accepts_an_empty_body() {
        let raw = b"HTTP/1.1 202 Accepted\r\n\r\n";
        let response = parse_response(raw).expect("parse response");
        assert_eq!(response.status, 202);
        assert_eq!(response.body, Value::Null);
    }
}
