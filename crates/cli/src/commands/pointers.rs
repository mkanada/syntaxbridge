//! `sb pointers` — `GET /projects/pointers`, the pointer catalog plus its
//! narrowing (`possible_types`, `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`).

use std::path::Path;

use serde_json::Value;

use crate::http::{Client, build_query};
use crate::output::{format_json, format_table};

use super::{CommandError, body_or_error};

pub fn request_pointers(client: &Client, project_dir: &Path) -> Result<Value, CommandError> {
    let query = build_query(&[("project_dir", &project_dir.to_string_lossy())]);
    body_or_error(client.get(&format!("/projects/pointers?{query}"))?)
}

pub fn render_pointers(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(body);
    }

    let empty = Vec::new();
    let pointers = body["pointers"].as_array().unwrap_or(&empty);
    let possible_types = &body["possible_types"];
    let rows = pointers
        .iter()
        .map(|pointer| {
            let usr = pointer["usr"].as_str().unwrap_or_default();
            let narrowed = possible_types[usr]
                .as_array()
                .map(|options| {
                    options
                        .iter()
                        .filter_map(|option| option["name"].as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| {
                    pointer["pointee_type_name"]
                        .as_str()
                        .unwrap_or_default()
                        .to_owned()
                });

            vec![
                pointer["name"].as_str().unwrap_or_default().to_owned(),
                pointer["kind"].as_str().unwrap_or_default().to_owned(),
                pointer["shape"].as_str().unwrap_or_default().to_owned(),
                narrowed,
                pointer["file"].as_str().unwrap_or_default().to_owned(),
            ]
        })
        .collect::<Vec<_>>();

    format_table(&["NAME", "KIND", "SHAPE", "POSSIBLE_TYPES", "FILE"], &rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn render_pointers_shows_the_narrowed_types_when_present() {
        let body = json!({
            "pointers": [
                {"name": "forma", "kind": "return_type", "shape": "scalar", "pointee_type_name": "Forma", "usr": "p1", "file": "fabrica.cpp"}
            ],
            "possible_types": {
                "p1": [{"usr": "c:@S@Triangulo", "name": "Triangulo"}]
            }
        });
        let table = render_pointers(false, &body);
        assert!(table.contains("Triangulo"));
        assert!(!table.contains("Forma  "));
    }

    #[test]
    fn render_pointers_falls_back_to_the_raw_pointee_type_when_not_narrowed() {
        let body = json!({
            "pointers": [
                {"name": "forma", "kind": "field", "shape": "scalar", "pointee_type_name": "Forma", "usr": "p1", "file": "casa.h"}
            ],
            "possible_types": {}
        });
        let table = render_pointers(false, &body);
        assert!(table.contains("Forma"));
    }
}
