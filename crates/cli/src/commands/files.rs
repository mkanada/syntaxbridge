//! `sb files` and `sb cat <file>` — the file-listing gap noted in
//! `docs/plans/interface-de-linha-de-comando.md`'s "Decisões em aberto":
//! there is no dedicated "list files" route, only `source_files` embedded
//! in `POST /projects/open`'s response, and `GET /projects/source-file` to
//! read one file's content. `files` reopens the project (cheap — no
//! re-extraction, see `project_service::open_project`) to get that list.

use std::path::Path;

use serde_json::{Value, json};

use crate::http::{Client, build_query};
use crate::output::{format_json, format_table};

use super::{CommandError, body_or_error, path_json};

pub fn request_files(client: &Client, project_dir: &Path) -> Result<Value, CommandError> {
    body_or_error(client.post_json(
        "/projects/open",
        &json!({ "project_dir": path_json(project_dir) }),
    )?)
}

pub fn render_files(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(&body["source_files"]);
    }

    let project_dir = body["project_dir"].as_str().unwrap_or_default();
    let empty = Vec::new();
    let files = body["source_files"].as_array().unwrap_or(&empty);
    let rows = files
        .iter()
        .map(|file| {
            let path = file["path"].as_str().unwrap_or_default();
            vec![
                display_path(project_dir, path).into_owned(),
                file["kind"].as_str().unwrap_or_default().to_owned(),
            ]
        })
        .collect::<Vec<_>>();

    format_table(&["PATH", "KIND"], &rows)
}

/// Shortens a `source_files[].path` (always absolute, see
/// `source_catalog::SourceFile`) to be relative to `<project_dir>/input-source`
/// for display — `sb files` printing the full absolute path on every line
/// would just be noise a human has to visually strip back out.
fn display_path<'a>(project_dir: &str, absolute_path: &'a str) -> std::borrow::Cow<'a, str> {
    let prefix = format!("{}/input-source/", project_dir.trim_end_matches('/'));
    match absolute_path.strip_prefix(&prefix) {
        Some(relative) => std::borrow::Cow::Borrowed(relative),
        None => std::borrow::Cow::Borrowed(absolute_path),
    }
}

/// Resolves what `sb cat` was given into the absolute path
/// `GET /projects/source-file` requires (`project_service::read_source_file`
/// canonicalizes it and checks it falls under `<project_dir>/input-source`):
/// an already-absolute path is passed through untouched (so pasting a path
/// straight from `sb files --json` still works), anything else is resolved
/// against `input-source` so `sb cat fixture/main.cpp` — matching what
/// `sb files`' table prints — just works.
fn resolve_cat_path(project_dir: &Path, path: &str) -> String {
    if Path::new(path).is_absolute() {
        path.to_owned()
    } else {
        project_dir
            .join("input-source")
            .join(path)
            .to_string_lossy()
            .into_owned()
    }
}

pub fn request_cat(client: &Client, project_dir: &Path, path: &str) -> Result<Value, CommandError> {
    let resolved = resolve_cat_path(project_dir, path);
    let query = build_query(&[
        ("project_dir", &project_dir.to_string_lossy()),
        ("path", &resolved),
    ]);
    body_or_error(client.get(&format!("/projects/source-file?{query}"))?)
}

pub fn render_cat(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(body);
    }
    body["content"].as_str().unwrap_or_default().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_files_lists_path_and_kind() {
        let body = json!({
            "source_files": [
                {"path": "src/main.cpp", "kind": "translation_unit"},
                {"path": "src/main.h", "kind": "header"}
            ]
        });
        let rendered = render_files(false, &body);
        assert!(rendered.contains("src/main.cpp"));
        assert!(rendered.contains("translation_unit"));
        assert!(rendered.contains("src/main.h"));
        assert!(rendered.contains("header"));
    }

    #[test]
    fn render_files_shortens_paths_relative_to_input_source() {
        let body = json!({
            "project_dir": "/tmp/counter",
            "source_files": [
                {"path": "/tmp/counter/input-source/fixture/main.cpp", "kind": "translation_unit"}
            ]
        });
        let rendered = render_files(false, &body);
        assert!(rendered.contains("fixture/main.cpp"));
        assert!(!rendered.contains("/tmp/counter"));
    }

    #[test]
    fn render_files_in_json_mode_prints_only_the_source_files_array() {
        let body = json!({
            "name": "irrelevant",
            "source_files": [{"path": "a.cpp", "kind": "translation_unit"}]
        });
        let rendered = render_files(true, &body);
        let reparsed: Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(reparsed, body["source_files"]);
    }

    #[test]
    fn render_cat_prints_the_raw_content_without_quoting() {
        let body = json!({ "content": "int main() { return 0; }\n" });
        assert_eq!(render_cat(false, &body), "int main() { return 0; }\n");
    }

    #[test]
    fn resolve_cat_path_joins_a_relative_path_under_input_source() {
        assert_eq!(
            resolve_cat_path(Path::new("/tmp/counter"), "fixture/main.cpp"),
            "/tmp/counter/input-source/fixture/main.cpp"
        );
    }

    #[test]
    fn resolve_cat_path_passes_an_absolute_path_through_untouched() {
        assert_eq!(
            resolve_cat_path(
                Path::new("/tmp/counter"),
                "/tmp/counter/input-source/fixture/main.cpp"
            ),
            "/tmp/counter/input-source/fixture/main.cpp"
        );
    }
}
