//! `sb init <archive> [--name] [--workspace]` — creates a project
//! (`POST /projects`), which the server runs as a background job because
//! the `libclang` extraction it triggers can take minutes
//! (`crates/server/src/server.rs`'s `create_project_from_http` doc
//! comment). This command polls `GET /projects/jobs/{id}` until the job
//! leaves `running`/`cancelling`, printing a line each time the reported
//! phase changes instead of a generic spinner — the phase is already
//! computed server-side (`jobs::derive_phase`), so surfacing it is free.

use std::path::Path;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::http::Client;
use crate::output::format_json;

use super::{CommandError, body_or_error, path_json};

const POLL_INTERVAL: Duration = Duration::from_millis(300);

pub fn request_create(
    client: &Client,
    name: &str,
    workspace_dir: &Path,
    archive_path: &Path,
) -> Result<String, CommandError> {
    let body = body_or_error(client.post_json(
        "/projects",
        &json!({
            "name": name,
            "workspace_dir": path_json(workspace_dir),
            "archive_path": path_json(archive_path),
        }),
    )?)?;

    body["job_id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| CommandError::Server {
            status: 200,
            message: "resposta de criação de projeto sem job_id".to_owned(),
        })
}

fn poll_once(client: &Client, job_id: &str) -> Result<Value, CommandError> {
    body_or_error(client.get(&format!("/projects/jobs/{job_id}"))?)
}

/// Runs the create-then-poll loop to completion, calling `on_progress` once
/// per phase change (not once per poll — polling is much more frequent
/// than the phase actually advances). Returns the job's terminal body
/// (`status` one of `succeeded`/`cancelled`/`failed`).
pub fn run(
    client: &Client,
    name: &str,
    workspace_dir: &Path,
    archive_path: &Path,
    mut on_progress: impl FnMut(&str),
) -> Result<Value, CommandError> {
    let job_id = request_create(client, name, workspace_dir, archive_path)?;

    let mut last_phase: Option<String> = None;
    loop {
        let poll = poll_once(client, &job_id)?;
        let status = poll["status"].as_str().unwrap_or("");
        if let Some(message) = describe_progress(last_phase.as_deref(), &poll) {
            on_progress(&message);
            last_phase = poll["phase"].as_str().map(str::to_owned);
        }
        if status != "running" && status != "cancelling" {
            return Ok(poll);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// Pure: given the previously-announced phase and a poll response, decides
/// whether there's something new worth printing. `None` means "nothing
/// changed since the last update" — keeps the loop from printing the same
/// phase every 300ms.
fn describe_progress(last_phase: Option<&str>, body: &Value) -> Option<String> {
    let status = body["status"].as_str().unwrap_or("");
    if status != "running" && status != "cancelling" {
        return None;
    }
    let phase = body["phase"].as_str().unwrap_or("");
    if Some(phase) == last_phase {
        return None;
    }
    let label = if status == "cancelling" {
        format!("cancelando ({phase})")
    } else {
        phase.to_owned()
    };
    Some(format!("[{label}]\n"))
}

pub fn render_outcome(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(body);
    }

    match body["status"].as_str().unwrap_or("") {
        "succeeded" => {
            let project = &body["project"];
            let units = project["compilation_units"].as_array().map_or(0, Vec::len);
            format!(
                "Projeto criado: {} ({})\n{} unidades de compilação\n",
                project["name"].as_str().unwrap_or_default(),
                project["project_dir"].as_str().unwrap_or_default(),
                units,
            )
        }
        "cancelled" => "Criação cancelada.\n".to_owned(),
        "failed" => format!(
            "Falha ao criar o projeto: {}\n",
            body["message"].as_str().unwrap_or("erro desconhecido")
        ),
        other => format!("Status inesperado do job: {other}\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_progress_reports_the_first_phase_seen() {
        let body = json!({ "status": "running", "phase": "ingesting" });
        assert_eq!(
            describe_progress(None, &body),
            Some("[ingesting]\n".to_owned())
        );
    }

    #[test]
    fn describe_progress_stays_quiet_when_the_phase_has_not_changed() {
        let body = json!({ "status": "running", "phase": "ingesting" });
        assert_eq!(describe_progress(Some("ingesting"), &body), None);
    }

    #[test]
    fn describe_progress_reports_a_phase_change() {
        let body = json!({ "status": "running", "phase": "cataloging_types" });
        assert_eq!(
            describe_progress(Some("ingesting"), &body),
            Some("[cataloging_types]\n".to_owned())
        );
    }

    #[test]
    fn describe_progress_is_none_once_the_job_is_terminal() {
        let body = json!({ "status": "succeeded", "project": {} });
        assert_eq!(describe_progress(Some("ingesting"), &body), None);
    }

    #[test]
    fn render_outcome_summarizes_a_successful_creation() {
        let body = json!({
            "status": "succeeded",
            "project": {
                "name": "counter",
                "project_dir": "/tmp/counter",
                "compilation_units": [{"file": "main.cpp"}]
            }
        });
        let rendered = render_outcome(false, &body);
        assert!(rendered.contains("counter"));
        assert!(rendered.contains("1 unidades de compilação"));
    }

    #[test]
    fn render_outcome_reports_failure_with_the_server_message() {
        let body = json!({ "status": "failed", "message": "archive not found" });
        assert!(render_outcome(false, &body).contains("archive not found"));
    }

    #[test]
    fn render_outcome_reports_cancellation() {
        let body = json!({ "status": "cancelled" });
        assert!(render_outcome(false, &body).contains("cancelada"));
    }
}
