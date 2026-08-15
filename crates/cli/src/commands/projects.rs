//! `sb projects` / `sb projects forget <path>` / `sb open <path>` — the
//! global, directory-independent project registry
//! (`GET/POST/DELETE /projects`, `POST /projects/open`), used from outside
//! a project directory.

use std::path::Path;

use serde_json::{Value, json};

use crate::http::Client;
use crate::output::{format_json, format_table};

use super::{CommandError, body_or_error, path_json};

pub fn request_list(client: &Client) -> Result<Value, CommandError> {
    body_or_error(client.get("/projects")?)
}

pub fn render_list(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(body);
    }

    let empty = Vec::new();
    let projects = body["projects"].as_array().unwrap_or(&empty);
    if projects.is_empty() {
        return "Nenhum projeto conhecido. Use `syntax-bridge init` ou `syntax-bridge open <path>`.\n"
            .to_owned();
    }

    let rows = projects
        .iter()
        .map(|project| {
            vec![
                text(project, "name"),
                text(project, "project_dir"),
                text(project, "last_ingest_status"),
                if project["available"].as_bool().unwrap_or(false) {
                    "yes".to_owned()
                } else {
                    "no".to_owned()
                },
            ]
        })
        .collect::<Vec<_>>();

    format_table(&["NAME", "PROJECT_DIR", "STATUS", "AVAILABLE"], &rows)
}

pub fn request_open(client: &Client, project_dir: &Path) -> Result<Value, CommandError> {
    body_or_error(client.post_json(
        "/projects/open",
        &json!({ "project_dir": path_json(project_dir) }),
    )?)
}

pub fn render_open(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(body);
    }

    let compilation_units = body["compilation_units"].as_array().map_or(0, Vec::len);
    let source_files = body["source_files"].as_array().map_or(0, Vec::len);
    format!(
        "{} ({})\n{} unidades de compilação, {} arquivos-fonte\n",
        text(body, "name"),
        text(body, "project_dir"),
        compilation_units,
        source_files,
    )
}

pub fn request_forget(client: &Client, project_dir: &Path) -> Result<Value, CommandError> {
    body_or_error(client.delete_json(
        "/projects",
        &json!({ "project_dir": path_json(project_dir) }),
    )?)
}

pub fn render_forget(json_mode: bool, body: &Value, project_dir: &Path) -> String {
    if json_mode {
        return format_json(body);
    }

    if body["forgotten"].as_bool().unwrap_or(false) {
        format!(
            "Removido dos projetos recentes: {}\n",
            project_dir.display()
        )
    } else {
        format!(
            "{} não estava nos projetos recentes.\n",
            project_dir.display()
        )
    }
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_list_prints_a_table_with_one_row_per_project() {
        let body = json!({
            "projects": [
                {
                    "name": "verovio",
                    "project_dir": "/home/user/verovio",
                    "source_language": "cpp",
                    "target_language": "dart",
                    "last_ingest_status": "succeeded",
                    "available": true
                },
                {
                    "name": "moved",
                    "project_dir": "/tmp/gone",
                    "source_language": "cpp",
                    "target_language": "dart",
                    "last_ingest_status": "succeeded",
                    "available": false
                }
            ]
        });

        let table = render_list(false, &body);
        assert!(table.contains("verovio"));
        assert!(table.contains("/home/user/verovio"));
        assert!(table.contains("yes"));
        assert!(table.contains("moved"));
        assert!(table.contains("no"));
    }

    #[test]
    fn render_list_reports_when_there_are_no_projects() {
        let body = json!({ "projects": [] });
        assert!(render_list(false, &body).contains("Nenhum projeto"));
    }

    #[test]
    fn render_list_in_json_mode_passes_the_body_through() {
        let body = json!({ "projects": [] });
        let rendered = render_list(true, &body);
        let reparsed: Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(reparsed, body);
    }

    #[test]
    fn render_open_summarizes_the_loaded_project() {
        let body = json!({
            "name": "verovio",
            "project_dir": "/home/user/verovio",
            "input_source_dir": "/home/user/verovio/input-source",
            "compilation_units": [{"file": "a.cpp"}, {"file": "b.cpp"}],
            "source_files": [{"path": "a.cpp", "kind": "translation_unit"}]
        });

        let rendered = render_open(false, &body);
        assert!(rendered.contains("verovio"));
        assert!(rendered.contains("2 unidades de compilação"));
        assert!(rendered.contains("1 arquivos-fonte"));
    }

    #[test]
    fn render_forget_reports_removal() {
        let body = json!({ "forgotten": true });
        let rendered = render_forget(false, &body, Path::new("/tmp/x"));
        assert!(rendered.contains("Removido"));
        assert!(rendered.contains("/tmp/x"));
    }

    #[test]
    fn render_forget_reports_when_the_project_was_not_known() {
        let body = json!({ "forgotten": false });
        let rendered = render_forget(false, &body, Path::new("/tmp/x"));
        assert!(rendered.contains("não estava"));
    }
}
