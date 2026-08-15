//! One module per command family from
//! `docs/plans/interface-de-linha-de-comando.md`'s command table. Every
//! command follows the same shape: a thin function that does the HTTP call
//! (`request_*`), and a pure rendering function (`render_*`) that turns the
//! JSON body into what gets printed — the render functions are what the
//! unit tests exercise directly, without a server; a smaller set of
//! integration tests (in `tests/`) spawn a real server to prove the wiring
//! end to end.

pub mod files;
pub mod functions;
pub mod init;
pub mod pointers;
pub mod projects;
pub mod transpile;
pub mod types;

use serde_json::Value;

use crate::http::{Client, ClientError, HttpResponse};

#[derive(Debug)]
pub enum CommandError {
    Client(ClientError),
    /// A non-2xx response the server sent deliberately (see the
    /// `{"error": ..., "message": ...}` shape every error route in
    /// `crates/server/src/server.rs` returns), as opposed to a transport
    /// failure.
    Server {
        status: u16,
        message: String,
    },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Client(error) => write!(f, "{error}"),
            CommandError::Server { status, message } => write!(f, "{status}: {message}"),
        }
    }
}

impl From<ClientError> for CommandError {
    fn from(error: ClientError) -> Self {
        CommandError::Client(error)
    }
}

/// Unwraps an `HttpResponse` into its JSON body, turning a non-2xx status
/// into a `CommandError::Server` — every command's HTTP call goes through
/// this instead of checking `response.status` by hand.
pub fn body_or_error(response: HttpResponse) -> Result<Value, CommandError> {
    if (200..300).contains(&response.status) {
        Ok(response.body)
    } else {
        let message = response.body["message"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| response.body.to_string());
        Err(CommandError::Server {
            status: response.status,
            message,
        })
    }
}

/// Shared by every "inside a project" command: `project_dir` must already
/// be resolved (via `project::find_project_dir` or `--project`) before
/// reaching here — commands never re-derive it themselves.
pub struct ProjectContext<'a> {
    pub client: &'a Client,
    pub project_dir: std::path::PathBuf,
    pub json: bool,
}

/// JSON-encodes a filesystem path as the server's `PathBuf`-typed request
/// fields expect (a plain UTF-8 string).
pub fn path_json(path: &std::path::Path) -> Value {
    Value::String(path.to_string_lossy().into_owned())
}

/// Resolves a human-typed `needle` (e.g. `"Triangulo::area"` from `sb
/// functions callers Triangulo::area`) against a list of `{name,
/// namespace, usr, ...}` objects — types and functions share this shape,
/// so both `commands::types` and `commands::functions` use this instead of
/// requiring the caller to already know the `usr` clang assigns.
///
/// Tries an exact `usr` match first (so scripts/agents that already have a
/// `usr` from a previous response can pass it straight through), then
/// falls back to matching `namespace::name` or bare `name`.
pub fn resolve_usr<'a>(items: &'a [Value], needle: &str) -> Result<&'a str, CommandError> {
    if let Some(item) = items.iter().find(|item| item["usr"] == needle) {
        return Ok(item["usr"].as_str().unwrap_or_default());
    }

    let matches: Vec<&Value> = items
        .iter()
        .filter(|item| qualified_name(item) == needle || item["name"] == needle)
        .collect();

    match matches.as_slice() {
        [] => Err(CommandError::Server {
            status: 404,
            message: format!("nenhum resultado para {needle:?}"),
        }),
        [only] => Ok(only["usr"].as_str().unwrap_or_default()),
        many => {
            let candidates = many
                .iter()
                .map(|item| format!("{} ({})", qualified_name(item), item["usr"]))
                .collect::<Vec<_>>()
                .join(", ");
            Err(CommandError::Server {
                status: 409,
                message: format!("{needle:?} é ambíguo, candidatos: {candidates}"),
            })
        }
    }
}

fn qualified_name(item: &Value) -> String {
    let namespace = item["namespace"].as_str().unwrap_or_default();
    let name = item["name"].as_str().unwrap_or_default();
    if namespace.is_empty() {
        name.to_owned()
    } else {
        format!("{namespace}::{name}")
    }
}

#[cfg(test)]
mod resolve_usr_tests {
    use super::*;

    fn item(name: &str, namespace: &str, usr: &str) -> Value {
        serde_json::json!({ "name": name, "namespace": namespace, "usr": usr })
    }

    #[test]
    fn resolves_an_exact_usr_straight_through() {
        let items = vec![item("area", "geometry", "c:@F@area")];
        assert_eq!(resolve_usr(&items, "c:@F@area").unwrap(), "c:@F@area");
    }

    #[test]
    fn resolves_a_qualified_name() {
        let items = vec![item("area", "Triangulo", "c:@N@Triangulo@F@area")];
        assert_eq!(
            resolve_usr(&items, "Triangulo::area").unwrap(),
            "c:@N@Triangulo@F@area"
        );
    }

    #[test]
    fn resolves_a_bare_name_at_global_scope() {
        let items = vec![item("add", "", "c:@F@add")];
        assert_eq!(resolve_usr(&items, "add").unwrap(), "c:@F@add");
    }

    #[test]
    fn reports_not_found() {
        let items: Vec<Value> = vec![];
        let error = resolve_usr(&items, "missing").unwrap_err();
        assert!(matches!(error, CommandError::Server { status: 404, .. }));
    }

    #[test]
    fn reports_ambiguous_matches_by_bare_name() {
        let items = vec![
            item("area", "Triangulo", "c:@N@Triangulo@F@area"),
            item("area", "Quadrado", "c:@N@Quadrado@F@area"),
        ];
        let error = resolve_usr(&items, "area").unwrap_err();
        match error {
            CommandError::Server {
                status: 409,
                message,
            } => {
                assert!(message.contains("Triangulo::area"));
                assert!(message.contains("Quadrado::area"));
            }
            other => panic!("expected an ambiguous-match error, got {other:?}"),
        }
    }
}
