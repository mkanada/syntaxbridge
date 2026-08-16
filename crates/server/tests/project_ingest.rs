use std::fs;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use syntax_bridge_server::ingest::{CreateProjectRequest, IngestError, create_project};
use syntax_bridge_server::persistence::GlobalStore;
use syntax_bridge_server::project_service::{self, CreationProgress};
use syntax_bridge_server::server::SyntaxBridgeServer;

const CPP_SOURCE: &str = r#"
int answer() {
    return 42;
}

int main() {
    return answer() == 42 ? 0 : 1;
}
"#;

const VEROVIO_ARCHIVE: &[u8] = include_bytes!("fixtures/verovio/verovio-version-6.2.0.tar.gz");

#[test]
fn creates_project_from_tarball_and_lists_cmake_compilation_units() {
    let workspace = TempWorkspace::new("ingest-tarball").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");

    let project = create_project(CreateProjectRequest {
        name: "counter".to_owned(),
        workspace_dir: workspace.path().join("projects"),
        archive_path,
    })
    .expect("ingest project");

    assert_eq!(project.name, "counter");
    assert!(
        project
            .input_source_dir
            .join("fixture/CMakeLists.txt")
            .is_file()
    );
    assert!(project.compile_commands_path.is_file());

    assert_eq!(
        project.compilation_units.len(),
        1,
        "expected one CMake compilation unit: {project:#?}"
    );

    let unit = &project.compilation_units[0];
    assert!(
        unit.file.ends_with("main.cpp"),
        "expected main.cpp compilation unit: {unit:#?}"
    );
    assert!(
        unit.command
            .as_deref()
            .is_some_and(|command| !command.is_empty())
            || !unit.arguments.is_empty(),
        "compilation unit should include the compiler invocation: {unit:#?}"
    );
}

/// `extract_archive` validates every tar entry (`validate_tar_entries`)
/// before extracting anything, specifically to reject path-traversal
/// members like this one — a regression guard for the ingest performance
/// work that collapses the archive's separate listing and extraction passes
/// into one decompression, so that change can't accidentally start
/// extracting before validating.
#[test]
fn rejects_a_tarball_with_a_path_traversal_entry_without_extracting_it() {
    let workspace = TempWorkspace::new("ingest-unsafe-tar").expect("create temporary workspace");
    let archive_path = workspace.path().join("evil.tar.gz");
    write_path_traversal_tarball(workspace.path(), &archive_path)
        .expect("create malicious archive");

    let result = create_project(CreateProjectRequest {
        name: "counter".to_owned(),
        workspace_dir: workspace.path().join("projects"),
        archive_path,
    });

    assert!(
        matches!(result, Err(IngestError::UnsafeArchiveEntry(_))),
        "expected the path-traversal entry to be rejected: {result:?}"
    );

    assert!(
        !workspace.path().join("evil.txt").exists(),
        "a path-traversal entry must never be written outside the project directory"
    );
}

#[test]
fn creates_project_from_zip_and_lists_cmake_compilation_units() {
    let workspace = TempWorkspace::new("ingest-zip").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.zip");
    write_cmake_fixture_zip(workspace.path(), &archive_path).expect("create fixture archive");

    let project = create_project(CreateProjectRequest {
        name: "counter".to_owned(),
        workspace_dir: workspace.path().join("projects"),
        archive_path,
    })
    .expect("ingest project");

    assert!(
        project
            .input_source_dir
            .join("fixture/CMakeLists.txt")
            .is_file()
    );
    assert_eq!(project.compilation_units.len(), 1);
    assert!(project.compilation_units[0].file.ends_with("main.cpp"));
}

#[test]
fn creates_project_in_specific_directory_and_extracts_verovio_tarball() {
    let workspace = TempWorkspace::new("ingest-verovio").expect("create temporary workspace");
    let project_workspace_dir = workspace.path().join("syntax-bridge-projects");
    let archive_path = workspace.path().join("verovio-version-6.2.0.tar.gz");
    fs::write(&archive_path, VEROVIO_ARCHIVE).expect("write Verovio fixture archive");

    let project = create_project(CreateProjectRequest {
        name: "verovio".to_owned(),
        workspace_dir: project_workspace_dir.clone(),
        archive_path,
    })
    .expect("ingest Verovio fixture project");

    let expected_project_dir = project_workspace_dir.join("verovio");
    let expected_source_root = expected_project_dir
        .join("input-source")
        .join("verovio-version-6.2.0");

    assert_eq!(project.project_dir, expected_project_dir);
    assert_eq!(
        project.input_source_dir,
        expected_project_dir.join("input-source")
    );
    assert!(expected_source_root.join("cmake/CMakeLists.txt").is_file());
    assert!(expected_source_root.join("src/vrv.cpp").is_file());
    assert!(expected_source_root.join("include/vrv/vrv.h").is_file());
}

#[test]
fn create_project_persists_compilation_units_and_registers_project_globally() {
    let workspace = TempWorkspace::new("ingest-persistence").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let project = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest and persist project");

    let project_store = syntax_bridge_server::persistence::ProjectStore::open(
        &project.project_dir.join("project.db"),
    )
    .expect("open project store");
    let persisted_units = project_store
        .list_compilation_units()
        .expect("list persisted compilation units");
    assert_eq!(persisted_units, project.compilation_units);

    let global_store = GlobalStore::open(&global_db_path).expect("open global store");
    let projects = global_store.list_projects().expect("list projects");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].name, "counter");
    assert_eq!(projects[0].project_dir, project.project_dir);
    assert_eq!(projects[0].source_language, "cpp");
    assert_eq!(projects[0].target_language, "dart");
    assert_eq!(projects[0].last_ingest_status, "success");
}

/// US-4 criterion 7: cancelling before `create_project` even starts is the
/// deterministic edge of "cancellation works end to end" — the extraction
/// layers already prove they stop early (`type_catalog`'s and
/// `source_catalog`'s own tests); this proves `create_project` itself
/// surfaces that as a distinct, non-persisting outcome instead of the
/// generic `TypeCatalog`/`SourceCatalog` failure path.
#[test]
fn create_project_reports_cancellation_and_persists_nothing() {
    let workspace = TempWorkspace::new("ingest-cancel").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let progress = CreationProgress::default();
    progress.cancellation.cancel();

    let result = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        Some(&progress),
    );

    let error = result.expect_err("a pre-cancelled run should not succeed");
    assert!(
        error.is_cancelled(),
        "expected a cancellation error, got {error:?}"
    );

    assert!(
        !workspace
            .path()
            .join("projects")
            .join("counter")
            .join("project.db")
            .exists(),
        "a cancelled run should not persist a project database"
    );
    assert!(
        !global_db_path.exists()
            || GlobalStore::open(&global_db_path)
                .expect("open global store")
                .list_projects()
                .expect("list projects")
                .is_empty(),
        "a cancelled run should not register the project globally"
    );
}

/// US-4 criterion 7 at real scale, mirroring
/// `verovio_5_7_0_import_diagnosis.rs`'s precedent for large-fixture tests:
/// not run by default (`--ignored`) because a full `libclang` pass over the
/// Verovio fixture's ~290 translation units takes minutes even
/// uninterrupted (see "Escala" in `docs/plans/User Steps.md`). The other
/// cancellation tests all pre-cancel before starting, which is deterministic
/// but only proves the flag is *checked*, not that cancelling a run already
/// in flight actually cuts it short before finishing on its own — on the
/// small fixture used elsewhere, a genuine mid-flight cancel would race
/// against a run fast enough to complete before the cancel is even sent.
/// This fixture is slow enough that the 500ms head start before cancelling
/// can't plausibly overlap with completion.
#[test]
#[ignore = "slow: runs a real libclang extraction over the Verovio 6.2.0 fixture (~290 TUs) to prove cancellation actually interrupts a run in flight"]
fn create_project_stops_within_seconds_of_cancellation_on_a_real_project() {
    let workspace =
        TempWorkspace::new("ingest-cancel-verovio").expect("create temporary workspace");
    let archive_path = workspace.path().join("verovio-version-6.2.0.tar.gz");
    fs::write(&archive_path, VEROVIO_ARCHIVE).expect("write Verovio fixture archive");
    let global_db_path = workspace.path().join("global.db");
    let request = CreateProjectRequest {
        name: "verovio".to_owned(),
        workspace_dir: workspace.path().join("projects"),
        archive_path,
    };

    let progress = CreationProgress::default();
    let started = Instant::now();

    let result = std::thread::scope(|scope| {
        let handle = scope
            .spawn(|| project_service::create_project(request, &global_db_path, Some(&progress)));

        std::thread::sleep(Duration::from_millis(500));
        progress.cancellation.cancel();

        handle.join().expect("creation thread panicked")
    });

    let elapsed = started.elapsed();
    let error = result.expect_err("a cancelled run should not succeed");
    assert!(
        error.is_cancelled(),
        "expected a cancellation error, got {error:?}"
    );
    assert!(
        elapsed < Duration::from_secs(60),
        "cancellation should cut a multi-minute extraction short, took {elapsed:?}"
    );
}

#[test]
fn open_project_reloads_persisted_data_without_reingesting() {
    let workspace = TempWorkspace::new("open-project").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let created = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest and persist project");

    let opened = project_service::open_project(&created.project_dir, &global_db_path)
        .expect("reopen persisted project");

    assert_eq!(opened.name, "counter");
    assert_eq!(opened.project_dir, created.project_dir);
    assert_eq!(opened.input_source_dir, created.input_source_dir);
    assert_eq!(opened.compilation_units, created.compilation_units);
    assert_eq!(opened.source_files, created.source_files);

    let global_store = GlobalStore::open(&global_db_path).expect("open global store");
    let projects = global_store.list_projects().expect("list projects");
    assert_eq!(
        projects.len(),
        1,
        "reopening should touch the existing row, not duplicate it: {projects:#?}"
    );
}

#[test]
fn open_project_rejects_a_directory_without_a_project_database() {
    let workspace = TempWorkspace::new("open-project-missing").expect("create temporary workspace");
    let bogus_dir = workspace.path().join("not-a-project");
    fs::create_dir_all(&bogus_dir).expect("create bogus directory");
    let global_db_path = workspace.path().join("global.db");

    let error = project_service::open_project(&bogus_dir, &global_db_path)
        .expect_err("opening a directory without project.db should fail");

    assert!(
        error.is_client_error(),
        "missing project.db should be a client error: {error:?}"
    );
}

/// `POST /projects` starts a background job and returns immediately
/// (`202 Accepted` + `job_id`) rather than blocking on the whole ingest —
/// see `crates/server/src/server.rs`'s `create_project_from_http` doc
/// comment for why. This polls `GET /projects/jobs/{id}` for the fixture's
/// small, effectively-instant creation.
#[test]
fn project_endpoint_returns_created_project_and_compilation_units() {
    let workspace = TempWorkspace::new("ingest-http").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({
        "name": "counter",
        "workspace_dir": workspace.path().join("projects"),
        "archive_path": archive_path,
    })
    .to_string();

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "POST /projects HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");

    assert!(
        response.starts_with("HTTP/1.1 202 Accepted\r\n"),
        "unexpected response: {response}"
    );

    let response_body = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let start_json: Value = serde_json::from_str(response_body).expect("parse response body");
    let job_id = start_json["job_id"]
        .as_str()
        .expect("response includes job_id");

    let json = poll_job_until_done(addr, job_id);
    handle.shutdown().expect("stop test server");

    assert_eq!(json["status"], "succeeded", "unexpected job status: {json}");
    let project = &json["project"];
    let units = project
        .get("compilation_units")
        .and_then(Value::as_array)
        .expect("response includes compilation_units array");

    assert_eq!(project["name"], "counter");
    assert_eq!(units.len(), 1, "unexpected project body: {project}");
    assert!(
        units[0]["file"]
            .as_str()
            .is_some_and(|file| file.ends_with("main.cpp")),
        "response should include main.cpp unit: {project}"
    );

    let global_store =
        GlobalStore::open(&workspace.path().join("global.db")).expect("open global store");
    let projects = global_store.list_projects().expect("list projects");
    assert_eq!(
        projects.len(),
        1,
        "expected the created project to be registered globally"
    );
    assert_eq!(projects[0].name, "counter");
    assert_eq!(
        projects[0].project_dir,
        workspace.path().join("projects").join("counter")
    );
}

#[test]
fn project_creation_job_reports_failure_for_a_client_error() {
    let workspace =
        TempWorkspace::new("ingest-http-job-failure").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({
        "name": "../escape",
        "workspace_dir": workspace.path().join("projects"),
        "archive_path": archive_path,
    })
    .to_string();

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "POST /projects HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    assert!(
        response.starts_with("HTTP/1.1 202 Accepted\r\n"),
        "starting the job itself should still succeed: {response}"
    );

    let response_body = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let start_json: Value = serde_json::from_str(response_body).expect("parse response body");
    let job_id = start_json["job_id"]
        .as_str()
        .expect("response includes job_id");

    let json = poll_job_until_done(addr, job_id);
    handle.shutdown().expect("stop test server");

    assert_eq!(json["status"], "failed", "unexpected job status: {json}");
    assert_eq!(
        json["is_client_error"], true,
        "an invalid project name should be a client error: {json}"
    );
    assert!(
        json["message"].as_str().is_some_and(|m| !m.is_empty()),
        "expected a non-empty error message: {json}"
    );
}

#[test]
fn project_creation_job_endpoint_returns_not_found_for_an_unknown_job() {
    let workspace =
        TempWorkspace::new("ingest-http-job-missing").expect("create temporary workspace");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "GET /projects/jobs/does-not-exist HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    handle.shutdown().expect("stop test server");

    assert!(
        response.starts_with("HTTP/1.1 404 Not Found\r\n"),
        "unexpected response: {response}"
    );
}

/// US-4 criterion 7's HTTP surface: `DELETE /projects/jobs/{id}` on an id
/// nobody registered behaves like the poll route's own unknown-id case.
#[test]
fn cancel_job_endpoint_returns_not_found_for_an_unknown_job() {
    let workspace =
        TempWorkspace::new("ingest-http-cancel-missing").expect("create temporary workspace");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "DELETE /projects/jobs/does-not-exist HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    handle.shutdown().expect("stop test server");

    assert!(
        response.starts_with("HTTP/1.1 404 Not Found\r\n"),
        "unexpected response: {response}"
    );
}

/// Cancelling a job that has already finished must not retroactively change
/// its outcome — a real result the client may already be showing shouldn't
/// flip to "cancelled" because of a request that arrived too late to matter.
/// This is the deterministic slice of "cancel is best-effort": timing a
/// cancel to land *before* a job as small as this fixture finishes would be
/// racy (see `poll_job_until_done`'s own doc comment), so this test instead
/// proves the safe, always-true half of the contract.
#[test]
fn cancel_job_endpoint_does_not_alter_an_already_finished_job() {
    let workspace =
        TempWorkspace::new("ingest-http-cancel-finished").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({
        "name": "counter",
        "workspace_dir": workspace.path().join("projects"),
        "archive_path": archive_path,
    })
    .to_string();

    let mut start_stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        start_stream,
        "POST /projects HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write request");
    let mut start_response = String::new();
    start_stream
        .read_to_string(&mut start_response)
        .expect("read response");
    let start_response_body = start_response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let start_json: Value = serde_json::from_str(start_response_body).expect("parse response body");
    let job_id = start_json["job_id"]
        .as_str()
        .expect("response includes job_id");

    let finished = poll_job_until_done(addr, job_id);
    assert_eq!(
        finished["status"], "succeeded",
        "unexpected job status: {finished}"
    );

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "DELETE /projects/jobs/{job_id} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");
    let mut cancel_response = String::new();
    stream
        .read_to_string(&mut cancel_response)
        .expect("read response");
    assert!(
        cancel_response.starts_with("HTTP/1.1 202 Accepted\r\n"),
        "cancelling should still be accepted, even though the job is already done: {cancel_response}"
    );

    let after_cancel = poll_job_until_done(addr, job_id);
    handle.shutdown().expect("stop test server");

    assert_eq!(
        after_cancel["status"], "succeeded",
        "a late cancel must not overwrite a real outcome: {after_cancel}"
    );
    assert_eq!(after_cancel["project"]["name"], "counter");
}

#[test]
fn project_endpoint_accepts_chunked_json_requests() {
    let workspace = TempWorkspace::new("ingest-http-chunked").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({
        "name": "counter",
        "workspace_dir": workspace.path().join("projects"),
        "archive_path": archive_path,
    })
    .to_string();
    let split_at = body.len() / 2;
    let first_chunk = &body[..split_at];
    let second_chunk = &body[split_at..];

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "POST /projects HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{}\r\n{:x}\r\n{}\r\n0\r\n\r\n",
        first_chunk.len(),
        first_chunk,
        second_chunk.len(),
        second_chunk
    )
    .expect("write chunked request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");

    assert!(
        response.starts_with("HTTP/1.1 202 Accepted\r\n"),
        "unexpected response: {response}"
    );

    let response_body = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let start_json: Value = serde_json::from_str(response_body).expect("parse response body");
    let job_id = start_json["job_id"]
        .as_str()
        .expect("response includes job_id");

    let json = poll_job_until_done(addr, job_id);
    handle.shutdown().expect("stop test server");

    assert_eq!(json["status"], "succeeded", "unexpected job status: {json}");
}

/// Polls `GET /projects/jobs/{job_id}` until the job leaves the `running`
/// state, for tests exercising the fixture's small, effectively-instant
/// creation. Bounded so a real regression fails the test instead of hanging
/// the suite.
fn poll_job_until_done(addr: SocketAddr, job_id: &str) -> Value {
    for _ in 0..200 {
        let mut stream = TcpStream::connect(addr).expect("connect to test server");
        write!(
            stream,
            "GET /projects/jobs/{job_id} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
        )
        .expect("write poll request");

        let mut response = String::new();
        stream.read_to_string(&mut response).expect("read response");
        let response_body = response
            .split("\r\n\r\n")
            .nth(1)
            .expect("response has HTTP body");
        let json: Value = serde_json::from_str(response_body).expect("parse response body");

        if json["status"] != "running" {
            return json;
        }

        std::thread::sleep(Duration::from_millis(20));
    }

    panic!("job {job_id} did not finish within the polling budget");
}

#[test]
fn recent_projects_endpoint_lists_the_last_created_project() {
    let workspace = TempWorkspace::new("recent-http").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest and persist project");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(global_db_path);
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "GET /projects HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    handle.shutdown().expect("stop test server");

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected response: {response}"
    );

    let response_body = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let json: Value = serde_json::from_str(response_body).expect("parse response body");
    let projects = json
        .get("projects")
        .and_then(Value::as_array)
        .expect("response includes projects array");

    assert_eq!(
        projects.len(),
        1,
        "unexpected response body: {response_body}"
    );
    assert_eq!(projects[0]["name"], "counter");
}

#[test]
fn recent_projects_endpoint_flags_a_project_whose_directory_is_gone() {
    let workspace = TempWorkspace::new("recent-missing").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let created = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest and persist project");

    // The user deleted the project directory outside the app: the registry
    // still knows about it, but there is nothing left to open.
    fs::remove_dir_all(&created.project_dir).expect("delete the project directory");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(global_db_path);
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "GET /projects HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    handle.shutdown().expect("stop test server");

    let response_body = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let json: Value = serde_json::from_str(response_body).expect("parse response body");
    let projects = json
        .get("projects")
        .and_then(Value::as_array)
        .expect("response includes projects array");

    assert_eq!(
        projects.len(),
        1,
        "unexpected response body: {response_body}"
    );
    assert_eq!(projects[0]["name"], "counter");
    assert_eq!(
        projects[0]["available"], false,
        "a project whose directory is gone must be reported as unavailable: {response_body}"
    );
}

#[test]
fn recent_projects_endpoint_reports_an_existing_project_as_available() {
    let workspace = TempWorkspace::new("recent-available").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest and persist project");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(global_db_path);
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "GET /projects HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    handle.shutdown().expect("stop test server");

    let response_body = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let json: Value = serde_json::from_str(response_body).expect("parse response body");
    let projects = json
        .get("projects")
        .and_then(Value::as_array)
        .expect("response includes projects array");

    assert_eq!(projects[0]["available"], true, "body: {response_body}");
}

#[test]
fn forget_project_endpoint_drops_it_from_the_recent_projects_list() {
    let workspace = TempWorkspace::new("forget-http").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let created = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest and persist project");
    fs::remove_dir_all(&created.project_dir).expect("delete the project directory");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(global_db_path);
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({ "project_dir": created.project_dir }).to_string();
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "DELETE /projects HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected response: {response}"
    );

    let mut stream = TcpStream::connect(addr).expect("reconnect to test server");
    write!(
        stream,
        "GET /projects HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .expect("write request");

    let mut listing = String::new();
    stream.read_to_string(&mut listing).expect("read response");
    handle.shutdown().expect("stop test server");

    let listing_body = listing
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let json: Value = serde_json::from_str(listing_body).expect("parse response body");
    let projects = json
        .get("projects")
        .and_then(Value::as_array)
        .expect("response includes projects array");

    assert!(
        projects.is_empty(),
        "forgotten project must not be listed again: {listing_body}"
    );
}

#[test]
fn open_project_endpoint_reloads_a_previously_ingested_project() {
    let workspace = TempWorkspace::new("open-http").expect("create temporary workspace");
    let archive_path = workspace.path().join("fixture.tar.gz");
    write_cmake_fixture_tarball(workspace.path(), &archive_path).expect("create fixture archive");
    let global_db_path = workspace.path().join("global.db");

    let created = project_service::create_project(
        CreateProjectRequest {
            name: "counter".to_owned(),
            workspace_dir: workspace.path().join("projects"),
            archive_path,
        },
        &global_db_path,
        None,
    )
    .expect("ingest and persist project");

    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(global_db_path);
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body = serde_json::json!({ "project_dir": created.project_dir }).to_string();
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "POST /projects/open HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    handle.shutdown().expect("stop test server");

    assert!(
        response.starts_with("HTTP/1.1 200 OK\r\n"),
        "unexpected response: {response}"
    );

    let response_body = response
        .split("\r\n\r\n")
        .nth(1)
        .expect("response has HTTP body");
    let json: Value = serde_json::from_str(response_body).expect("parse response body");
    assert_eq!(json["name"], "counter");
    let units = json
        .get("compilation_units")
        .and_then(Value::as_array)
        .expect("response includes compilation_units array");
    assert_eq!(units.len(), 1, "unexpected response body: {response_body}");
}

#[test]
fn open_project_endpoint_returns_not_found_for_a_bogus_directory() {
    let workspace = TempWorkspace::new("open-http-missing").expect("create temporary workspace");
    let server = SyntaxBridgeServer::bind("127.0.0.1:0")
        .expect("bind test server")
        .with_global_db_path(workspace.path().join("global.db"));
    let addr = server.local_addr().expect("read server address");
    let handle = server.spawn().expect("spawn test server");

    let body =
        serde_json::json!({ "project_dir": workspace.path().join("does-not-exist") }).to_string();
    let mut stream = TcpStream::connect(addr).expect("connect to test server");
    write!(
        stream,
        "POST /projects/open HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("write request");

    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    handle.shutdown().expect("stop test server");

    assert!(
        response.starts_with("HTTP/1.1 404 Not Found\r\n"),
        "unexpected response: {response}"
    );
}

fn write_cmake_fixture_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    write_cmake_fixture(workspace)?;

    let output = Command::new("tar")
        .arg("-czf")
        .arg(archive_path)
        .arg("-C")
        .arg(workspace)
        .arg("fixture")
        .output()?;
    assert_success(output);

    Ok(())
}

fn write_path_traversal_tarball(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    let payload_dir = workspace.join("payload");
    fs::create_dir_all(&payload_dir)?;
    fs::write(payload_dir.join("evil.txt"), b"pwned")?;

    // `--transform` rewrites the stored member name to `../evil.txt` at
    // archive-creation time, since `tar -c` itself won't accept a `..`
    // path directly — this is how a hostile archive (crafted by hand, not
    // through this codebase's own `tar`) would encode a traversal entry.
    let output = Command::new("tar")
        .arg("-czf")
        .arg(archive_path)
        .arg("--transform")
        .arg("s,^,../,")
        .arg("-C")
        .arg(&payload_dir)
        .arg("evil.txt")
        .output()?;
    assert_success(output);

    Ok(())
}

fn write_cmake_fixture_zip(workspace: &Path, archive_path: &Path) -> io::Result<()> {
    write_cmake_fixture(workspace)?;

    let output = Command::new("zip")
        .arg("-qr")
        .arg(archive_path)
        .arg("fixture")
        .current_dir(workspace)
        .output()?;
    assert_success(output);

    Ok(())
}

fn write_cmake_fixture(workspace: &Path) -> io::Result<()> {
    let source_dir = workspace.join("fixture");
    fs::create_dir_all(&source_dir)?;
    fs::write(source_dir.join("main.cpp"), CPP_SOURCE)?;
    fs::write(
        source_dir.join("CMakeLists.txt"),
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_ingest_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_ingest_fixture main.cpp)
"#,
    )?;

    Ok(())
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
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
