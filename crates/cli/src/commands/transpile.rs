//! `sb transpile` — `POST /projects/transpile`.

use std::path::Path;

use serde_json::Value;

use crate::http::{Client, build_query};
use crate::output::format_json;

use super::{CommandError, body_or_error};

pub fn request_transpile(client: &Client, project_dir: &Path) -> Result<Value, CommandError> {
    let query = build_query(&[("project_dir", &project_dir.to_string_lossy())]);
    body_or_error(client.post_json(&format!("/projects/transpile?{query}"), &Value::Null)?)
}

pub fn render_outcome(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(body);
    }

    let empty = serde_json::Map::new();
    let files = body["files"].as_object().unwrap_or(&empty);
    let mut paths: Vec<&String> = files.keys().collect();
    paths.sort();

    let mut output = format!(
        "Pacote {} gerado, {} arquivo(s):\n",
        body["package_name"].as_str().unwrap_or_default(),
        paths.len(),
    );
    for path in paths {
        output.push_str("  ");
        output.push_str(path);
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_outcome_lists_generated_files_sorted() {
        let body = json!({
            "package_name": "counter",
            "files": {
                "lib/counter.dart": "...",
                "pubspec.yaml": "..."
            }
        });
        let rendered = render_outcome(false, &body);
        assert!(rendered.contains("counter"));
        assert!(rendered.contains("2 arquivo"));
        let dart_pos = rendered.find("lib/counter.dart").unwrap();
        let pubspec_pos = rendered.find("pubspec.yaml").unwrap();
        assert!(dart_pos < pubspec_pos, "expected sorted output: {rendered}");
    }
}
