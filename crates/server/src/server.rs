use std::fmt;
use std::io;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::Router;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Json, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::runtime;
use tokio::sync::oneshot;

use crate::ingest::CreateProjectRequest;
use crate::jobs::{JobPhase, JobRegistry};
use crate::persistence;
use crate::project_service::{self, ListTypesError, OpenProjectError, ReadSourceFileError};

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
    job_registry: JobRegistry,
}

fn app(global_db_path: PathBuf) -> Router {
    let state = AppState {
        global_db_path: Arc::new(global_db_path),
        job_registry: JobRegistry::new(),
    };

    Router::new()
        .route("/health", get(health))
        .route(
            "/projects",
            get(list_recent_projects_from_http)
                .post(create_project_from_http)
                .delete(forget_project_from_http),
        )
        .route(
            "/projects/jobs/{job_id}",
            get(poll_project_creation_job_from_http).delete(cancel_project_creation_job_from_http),
        )
        .route("/projects/open", post(open_project_from_http))
        .route("/projects/source-file", get(read_source_file_from_http))
        .route("/projects/types", get(list_types_from_http))
        .route("/projects/types/usages", get(list_type_usages_from_http))
        .with_state(state)
}

async fn health() -> Json<HealthResponse> {
    log_server("handling health request");
    Json(HealthResponse {
        service: "syntax-bridge-server",
        status: "ok",
    })
}

/// Starts project creation as a background job and returns immediately with
/// a job id to poll (`poll_project_creation_job_from_http`), instead of
/// blocking the request on the whole ingest + `libclang` extraction — which,
/// for a real-world-sized project, takes minutes (see
/// `crates/server/tests/verovio_5_7_0_import_diagnosis.rs`) and would
/// otherwise leave the client with nothing to show but a frozen request.
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
    let (job_id, job) = state.job_registry.start();
    log_server(format_args!(
        "project creation job started: job_id={job_id}"
    ));

    let log_job_id = job_id.clone();
    tokio::task::spawn_blocking(move || {
        let outcome =
            project_service::create_project(request, &global_db_path, Some(&job.progress));
        match &outcome {
            Ok(project) => log_server(format_args!(
                "project creation job succeeded: job_id={log_job_id} name={} project_dir={} units={}",
                project.name,
                project.project_dir.display(),
                project.compilation_units.len()
            )),
            Err(error) => log_server(format_args!(
                "project creation job failed: job_id={log_job_id} error={error:?}"
            )),
        }
        job.finish(outcome);
    });

    json_response(StatusCode::ACCEPTED, json!({"job_id": job_id}))
}

/// Reports a project-creation job's status: still `running` (with live
/// progress for each `libclang` pass), `succeeded` (with the finished
/// project), or `failed` (with the error) — always as a `200`, since the
/// poll request itself succeeded even when the job it describes did not.
async fn poll_project_creation_job_from_http(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Response {
    let Some(job) = state.job_registry.get(&job_id) else {
        return json_response(
            StatusCode::NOT_FOUND,
            json!({"error":"job_not_found","message": format!("no project creation job with id {job_id}")}),
        );
    };

    job.with_outcome(|outcome| match outcome {
        Some(Ok(project)) => json_response(
            StatusCode::OK,
            json!({"status": "succeeded", "project": project}),
        ),
        Some(Err(error)) if error.is_cancelled() => {
            json_response(StatusCode::OK, json!({"status": "cancelled"}))
        }
        Some(Err(error)) => json_response(
            StatusCode::OK,
            json!({
                "status": "failed",
                "message": error.to_string(),
                "is_client_error": error.is_client_error(),
            }),
        ),
        None => {
            let type_catalog_completed = job.progress.type_catalog.completed();
            let type_catalog_total = job.progress.type_catalog.total();
            let source_catalog_completed = job.progress.source_catalog.completed();
            let source_catalog_total = job.progress.source_catalog.total();
            let phase = crate::jobs::derive_phase(
                type_catalog_completed,
                type_catalog_total,
                source_catalog_completed,
                source_catalog_total,
            );
            // Cancellation was requested but the background thread hasn't
            // reported a terminal outcome yet — distinct from `"cancelled"`
            // (`finish`'s outcome was a cancellation) so a poller never sees
            // a stale `"running"` after asking to cancel, without lying
            // about the job having actually stopped.
            let status = if job.cancel_requested() {
                "cancelling"
            } else {
                "running"
            };

            json_response(
                StatusCode::OK,
                json!({
                    "status": status,
                    "phase": phase_str(phase),
                    "type_catalog": {
                        "completed": type_catalog_completed,
                        "total": type_catalog_total,
                    },
                    "source_catalog": {
                        "completed": source_catalog_completed,
                        "total": source_catalog_total,
                    },
                }),
            )
        }
    })
}

/// Requests cancellation of an in-flight project-creation job (US-4
/// criterion 7). Best-effort: the background thread stops on its own next
/// check between compilation units (see `progress::Cancellation`), so this
/// returns immediately with `202` rather than waiting for that to happen —
/// callers poll `GET /projects/jobs/{job_id}` to observe the eventual
/// `"cancelling"` → `"cancelled"` transition. Idempotent, and harmless to
/// call on a job that has already finished.
async fn cancel_project_creation_job_from_http(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Response {
    let Some(job) = state.job_registry.get(&job_id) else {
        return json_response(
            StatusCode::NOT_FOUND,
            json!({"error":"job_not_found","message": format!("no project creation job with id {job_id}")}),
        );
    };

    job.cancel();
    log_server(format_args!("cancellation requested: job_id={job_id}"));

    json_response(StatusCode::ACCEPTED, json!({"status": "cancel_requested"}))
}

fn phase_str(phase: JobPhase) -> &'static str {
    match phase {
        JobPhase::Ingesting => "ingesting",
        JobPhase::CatalogingTypes => "cataloging_types",
        JobPhase::DiscoveringSourceFiles => "discovering_source_files",
        JobPhase::Persisting => "persisting",
    }
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

#[derive(Deserialize)]
struct ProjectDirQuery {
    project_dir: PathBuf,
}

async fn list_types_from_http(Query(query): Query<ProjectDirQuery>) -> Response {
    log_server(format_args!(
        "listing types: project_dir={}",
        query.project_dir.display()
    ));

    match tokio::task::spawn_blocking(move || project_service::list_types(&query.project_dir)).await
    {
        Ok(Ok(listing)) => json_response(StatusCode::OK, listing),
        Ok(Err(error)) => list_types_error_response(error),
        Err(error) => {
            log_server(format_args!("list types task failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"list_types_failed","message":error.to_string()}),
            )
        }
    }
}

#[derive(Deserialize)]
struct TypeUsagesQuery {
    project_dir: PathBuf,
    usr: String,
}

/// Serves every usage of one type (US-4), identified by its stable `usr`
/// (US-3) rather than by position, from the persisted index — the request a
/// click on a type-list row makes to populate its "used at" panel.
async fn list_type_usages_from_http(Query(query): Query<TypeUsagesQuery>) -> Response {
    log_server(format_args!(
        "listing usages: project_dir={} usr={}",
        query.project_dir.display(),
        query.usr
    ));

    match tokio::task::spawn_blocking(move || {
        project_service::list_type_usages(&query.project_dir, &query.usr)
    })
    .await
    {
        Ok(Ok(usages)) => json_response(StatusCode::OK, json!({ "usages": usages })),
        Ok(Err(error)) => list_types_error_response(error),
        Err(error) => {
            log_server(format_args!("list usages task failed: {error}"));
            json_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error":"list_usages_failed","message":error.to_string()}),
            )
        }
    }
}

fn list_types_error_response(error: ListTypesError) -> Response {
    let status = if error.is_client_error() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    log_server(format_args!(
        "list types failed: status={status} error={error:?}"
    ));
    json_response(
        status,
        json!({"error":"list_types_failed","message":error.to_string()}),
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
