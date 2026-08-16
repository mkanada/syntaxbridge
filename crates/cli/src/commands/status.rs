//! `sb status <job-id>` — a single, non-blocking query against `GET
//! /projects/jobs/{id}`, the same route `commands::init::run` already polls
//! in a loop while it blocks the terminal on project creation. This command
//! exists for the case that loop doesn't cover: checking on an ingestion
//! that's running from *another* terminal or process (`init` now prints its
//! job id up front — see `init::run` — precisely so it can be copied here).

use serde_json::Value;

use crate::http::Client;
use crate::output::format_json;

use super::{CommandError, body_or_error};

use super::init::render_outcome as render_terminal_outcome;

pub fn request_status(client: &Client, job_id: &str) -> Result<Value, CommandError> {
    body_or_error(client.get(&format!("/projects/jobs/{job_id}"))?)
}

/// Terminal statuses (`succeeded`/`cancelled`/`failed`) render exactly like
/// `sb init`'s outcome — same JSON shape, same message. `running`/
/// `cancelling` are status-specific: phase, per-pass counts, and a rough
/// overall fraction across whichever passes have already reported a total
/// (see `overall_progress`).
pub fn render_status(json: bool, body: &Value) -> String {
    if json {
        return format_json(body);
    }

    match body["status"].as_str().unwrap_or("") {
        "running" | "cancelling" => render_in_progress(body),
        _ => render_terminal_outcome(false, body),
    }
}

const PASSES: [&str; 4] = [
    "type_catalog",
    "source_catalog",
    "function_catalog",
    "pointer_catalog",
];

fn render_in_progress(body: &Value) -> String {
    let status = body["status"].as_str().unwrap_or("");
    let phase = body["phase"].as_str().unwrap_or("desconhecida");

    let mut out = if status == "cancelling" {
        format!("cancelando (fase: {phase})\n")
    } else {
        format!("fase: {phase}\n")
    };

    for pass in PASSES {
        let completed = body[pass]["completed"].as_u64().unwrap_or(0);
        let total = body[pass]["total"].as_u64().unwrap_or(0);
        out.push_str(&format!("  {pass}: {completed}/{total}\n"));
    }

    if let Some((completed, total)) = overall_progress(body) {
        let percent = (completed as f64 / total as f64 * 100.0).round() as u64;
        out.push_str(&format!(
            "progresso aproximado: {completed}/{total} ({percent}%)\n"
        ));
    } else {
        out.push_str("progresso aproximado: iniciando extração\n");
    }

    out
}

/// Sums `completed`/`total` across every pass that has already reported a
/// non-zero total (an untouched pass has nothing to add yet — see
/// `ExtractionProgress::set_total`'s doc comment on why zero means "not
/// started"). `None` while every pass is still at zero, i.e. before
/// `type_catalog` even begins — there's nothing to divide by yet. This is
/// deliberately approximate ("aproximado" in the rendered output): it grows
/// as later passes report their own totals, not just as work completes
/// within a single pass.
fn overall_progress(body: &Value) -> Option<(u64, u64)> {
    let mut completed_sum = 0u64;
    let mut total_sum = 0u64;
    for pass in PASSES {
        completed_sum += body[pass]["completed"].as_u64().unwrap_or(0);
        total_sum += body[pass]["total"].as_u64().unwrap_or(0);
    }
    if total_sum == 0 {
        None
    } else {
        Some((completed_sum, total_sum))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn running_body(phase: &str, type_completed: u64, type_total: u64) -> Value {
        json!({
            "status": "running",
            "phase": phase,
            "type_catalog": {"completed": type_completed, "total": type_total},
            "source_catalog": {"completed": 0, "total": 0},
            "function_catalog": {"completed": 0, "total": 0},
            "pointer_catalog": {"completed": 0, "total": 0},
        })
    }

    #[test]
    fn render_status_shows_the_phase_before_any_total_is_known() {
        let body = running_body("ingesting", 0, 0);
        let rendered = render_status(false, &body);
        assert!(rendered.contains("fase: ingesting"));
        assert!(rendered.contains("iniciando extração"));
    }

    #[test]
    fn render_status_shows_per_pass_counts_and_an_overall_percentage() {
        let body = running_body("cataloging_types", 45, 120);
        let rendered = render_status(false, &body);
        assert!(rendered.contains("type_catalog: 45/120"));
        assert!(rendered.contains("progresso aproximado: 45/120 (38%)"));
    }

    #[test]
    fn render_status_combines_totals_across_passes_that_have_started() {
        let mut body = running_body("cataloging_functions", 120, 120);
        body["source_catalog"] = json!({"completed": 10, "total": 10});
        body["function_catalog"] = json!({"completed": 3, "total": 20});
        let rendered = render_status(false, &body);
        assert!(rendered.contains("progresso aproximado: 133/150 (89%)"));
    }

    #[test]
    fn render_status_marks_a_cancelling_job() {
        let body = running_body("cataloging_pointers", 5, 5);
        let mut body = body;
        body["status"] = json!("cancelling");
        let rendered = render_status(false, &body);
        assert!(rendered.starts_with("cancelando (fase: cataloging_pointers)"));
    }

    #[test]
    fn render_status_delegates_terminal_statuses_to_init_render_outcome() {
        let body = json!({
            "status": "succeeded",
            "project": {"name": "counter", "project_dir": "/tmp/counter", "compilation_units": []}
        });
        let rendered = render_status(false, &body);
        assert!(rendered.contains("counter"));
    }

    #[test]
    fn render_status_json_mode_returns_the_raw_body() {
        let body = json!({"status": "running", "phase": "ingesting"});
        assert_eq!(render_status(true, &body), format_json(&body));
    }
}
