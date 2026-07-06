use std::fmt;
use std::io;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::{Json, State};
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Serialize;
use serde_json::json;
use tokio::runtime;
use tokio::sync::oneshot;

use crate::ingest::CreateProjectRequest;
use crate::persistence;
use crate::project_service::{self, ProjectCreationError};

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

        runtime.block_on(serve_with_axum(self.listener, self.global_db_path, shutdown_rx))
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
        .route("/projects", post(create_project_from_http))
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
