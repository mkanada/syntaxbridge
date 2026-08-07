use std::fmt;
use std::io;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::runtime;
use tokio::sync::oneshot;

use crate::ingest::CreateProjectRequest;
use crate::persistence;
use crate::project_service::{self, OpenProjectError, ProjectCreationError, ReadSourceFileError};

pub const DEFAULT_ADDR: &str = "127.0.0.1:37651";

pub struct SyntaxBridgeServer {
    listener: TcpListener,
    global_db_path: PathBuf,
}

impl SyntaxBridgeServer {
    pub fn bind(addr: impl ToSocketAddrs) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        Ok(Self {
            listener,
            global_db_path: persistence::default_global_db_path(),
        })
    }

    /// Overrides where the global project registry database lives. Tests
    /// use this to avoid touching the real user's data directory.
    pub fn with_global_db_path(mut self, global_db_path: PathBuf) -> Self {
        self.global_db_path = global_db_path;
        self
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub fn spawn(self) -> io::Result<ServerHandle> {
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let thread = thread::Builder::new()
            .name("syntax-bridge-server".to_owned())
            .spawn(move || self.serve(Some(shutdown_rx)))?;

        Ok(ServerHandle {
            shutdown_tx: Some(shutdown_tx),
            thread: Some(thread),
        })
    }

    pub fn run(self) -> io::Result<()> {
        self.serve(None)
    }

    fn serve(self, shutdown_rx: Option<oneshot::Receiver<()>>) -> io::Result<()> {
        let runtime = runtime::Builder::new_multi_thread()
            .enable_io()
            .build()
            .map_err(io::Error::other)?;

        runtime.block_on(serve_with_axum(
            self.listener,
            self.global_db_path,
            shutdown_rx,
        ))
    }
}

pub struct ServerHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl ServerHandle {
    pub fn shutdown(mut self) -> io::Result<()> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        let Some(thread) = self.thread.take() else {
            return Ok(());
        };

        thread
            .join()
            .map_err(|_| io::Error::other("server thread panicked"))?
    }
}

pub fn run_blocking(addr: &str) -> io::Result<()> {
    let server = SyntaxBridgeServer::bind(addr)?;
    eprintln!("syntax-bridge-server listening on {}", server.local_addr()?);
    log_server(format_args!("bound address: {}", server.local_addr()?));
    server.run()
}

async fn serve_with_axum(
    listener: TcpListener,
    global_db_path: PathBuf,
    shutdown_rx: Option<oneshot::Receiver<()>>,
) -> io::Result<()> {
    listener.set_nonblocking(true)?;
    let listener = tokio::net::TcpListener::from_std(listener)?;
    log_server("serve loop started");

    let serve = axum::serve(listener, app(global_db_path));
    match shutdown_rx {
        Some(shutdown_rx) => {
            serve
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                    log_server("shutdown requested");
                })
                .await
        }
        None => serve.await,
    }
}

#[derive(Clone)]
struct AppState {
    global_db_path: Arc<PathBuf>,
}

fn app(global_db_path: PathBuf) -> Router {
    let state = AppState {
        global_db_path: Arc::new(global_db_path),
    };

    Router::new()
        .route("/health", get(health))
        .route(
            "/projects",
            get(list_recent_projects_from_http)
                .post(create_project_from_http)
                .delete(forget_project_from_http),
        )
        .route("/projects/open", post(open_project_from_http))
        .route("/projects/source-file", get(read_source_file_from_http))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    log_server("handling health request");
    Json(HealthResponse {
        service: "syntax-bridge-server",
        status: "ok",
    })
}

async fn create_project_from_http(
    State(state): State<AppState>,
    payload: Result<Json<CreateProjectRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => {
            log_server(format_args!("invalid project creation JSON: {error}"));
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"error":"invalid_json","message":error.body_text()}),
            );
        }
    };

    log_server(format_args!(
        "decoded CreateProjectRequest: name={:?} workspace_dir={} archive_path={}",
        request.name,
        request.workspace_dir.display(),
        request.archive_path.display()
    ));

    let global_db_path = (*state.global_db_path).clone();
    match tokio::task::spawn_blocking(move || {
        project_service::create_project(request, &global_db_path)
    })
    .await
    {
        Ok(Ok(project)) => {
            log_server(format_args!(
                "project created: name={} project_dir={} units={}",
                project.name,
                project.project_dir.display(),
                project.compilation_units.len()
            ));
            json_response(StatusCode::CREATED, project)
        }
        Ok(Err(error)) => project_creation_error_response(error),
        Err(error) => {
            log_server(format_args!("project ingest task failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"project_ingest_failed","message":error.to_string()}),
            )
        }
    }
}

fn project_creation_error_response(error: ProjectCreationError) -> Response {
    let status = if error.is_client_error() {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    log_server(format_args!(
        "project creation failed: status={status} error={error:?}"
    ));
    json_response(
        status,
        json!({"error":"project_creation_failed","message":error.to_string()}),
    )
}

async fn list_recent_projects_from_http(State(state): State<AppState>) -> Response {
    log_server("handling list recent projects request");

    let global_db_path = (*state.global_db_path).clone();
    match tokio::task::spawn_blocking(move || {
        project_service::list_recent_projects(&global_db_path)
    })
    .await
    {
        Ok(Ok(projects)) => json_response(StatusCode::OK, json!({ "projects": projects })),
        Ok(Err(error)) => {
            log_server(format_args!("list recent projects failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"list_recent_projects_failed","message":error.to_string()}),
            )
        }
        Err(error) => {
            log_server(format_args!("list recent projects task failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"list_recent_projects_failed","message":error.to_string()}),
            )
        }
    }
}

#[derive(Deserialize)]
struct OpenProjectRequest {
    project_dir: PathBuf,
}

async fn open_project_from_http(
    State(state): State<AppState>,
    payload: Result<Json<OpenProjectRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => {
            log_server(format_args!("invalid open project JSON: {error}"));
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"error":"invalid_json","message":error.body_text()}),
            );
        }
    };

    log_server(format_args!(
        "opening project: project_dir={}",
        request.project_dir.display()
    ));

    let global_db_path = (*state.global_db_path).clone();
    match tokio::task::spawn_blocking(move || {
        project_service::open_project(&request.project_dir, &global_db_path)
    })
    .await
    {
        Ok(Ok(project)) => {
            log_server(format_args!(
                "project opened: name={} project_dir={}",
                project.name,
                project.project_dir.display()
            ));
            json_response(StatusCode::OK, project)
        }
        Ok(Err(error)) => open_project_error_response(error),
        Err(error) => {
            log_server(format_args!("open project task failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"open_project_failed","message":error.to_string()}),
            )
        }
    }
}

/// Drops a project from the recent-projects registry. Deleting a registry
/// entry is idempotent from the caller's point of view, so forgetting an
/// unknown project is reported as success with `forgotten: false` rather than
/// as an error.
async fn forget_project_from_http(
    State(state): State<AppState>,
    payload: Result<Json<OpenProjectRequest>, JsonRejection>,
) -> Response {
    let Json(request) = match payload {
        Ok(request) => request,
        Err(error) => {
            log_server(format_args!("invalid forget project JSON: {error}"));
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"error":"invalid_json","message":error.body_text()}),
            );
        }
    };

    log_server(format_args!(
        "forgetting project: project_dir={}",
        request.project_dir.display()
    ));

    let global_db_path = (*state.global_db_path).clone();
    match tokio::task::spawn_blocking(move || {
        project_service::forget_project(&request.project_dir, &global_db_path)
    })
    .await
    {
        Ok(Ok(forgotten)) => {
            log_server(format_args!("project forgotten: removed={forgotten}"));
            json_response(StatusCode::OK, json!({ "forgotten": forgotten }))
        }
        Ok(Err(error)) => {
            log_server(format_args!("forget project failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"forget_project_failed","message":error.to_string()}),
            )
        }
        Err(error) => {
            log_server(format_args!("forget project task failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"forget_project_failed","message":error.to_string()}),
            )
        }
    }
}

fn open_project_error_response(error: OpenProjectError) -> Response {
    let status = if error.is_client_error() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    log_server(format_args!(
        "open project failed: status={status} error={error:?}"
    ));
    json_response(
        status,
        json!({"error":"open_project_failed","message":error.to_string()}),
    )
}

#[derive(Deserialize)]
struct SourceFileQuery {
    project_dir: PathBuf,
    path: PathBuf,
}

async fn read_source_file_from_http(Query(query): Query<SourceFileQuery>) -> Response {
    log_server(format_args!(
        "reading source file: project_dir={} path={}",
        query.project_dir.display(),
        query.path.display()
    ));

    match tokio::task::spawn_blocking(move || {
        project_service::read_source_file(&query.project_dir, &query.path)
    })
    .await
    {
        Ok(Ok(content)) => json_response(StatusCode::OK, json!({"content": content})),
        Ok(Err(error)) => read_source_file_error_response(error),
        Err(error) => {
            log_server(format_args!("read source file task failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"read_source_file_failed","message":error.to_string()}),
            )
        }
    }
}

fn read_source_file_error_response(error: ReadSourceFileError) -> Response {
    let status = match &error {
        ReadSourceFileError::OutsideProject => StatusCode::BAD_REQUEST,
        ReadSourceFileError::Io(io_error) if io_error.kind() == io::ErrorKind::NotFound => {
            StatusCode::NOT_FOUND
        }
        ReadSourceFileError::Io(_) => StatusCode::INTERNAL_SERVER_ERROR,
    };
    log_server(format_args!(
        "read source file failed: status={status} error={error:?}"
    ));
    json_response(
        status,
        json!({"error":"read_source_file_failed","message":error.to_string()}),
    )
}

fn json_response(status: StatusCode, body: impl Serialize) -> Response {
    (status, Json(body)).into_response()
}

#[derive(Serialize)]
struct HealthResponse {
    service: &'static str,
    status: &'static str,
}

fn log_server(message: impl fmt::Display) {
    eprintln!("[syntax-bridge][server][{}] {message}", timestamp_millis());
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
