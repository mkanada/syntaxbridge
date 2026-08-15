//! `sb types [--kind] [--namespace]` and `sb types usages <type>` —
//! `GET /projects/types` and `GET /projects/types/usages`. Filtering by
//! kind/namespace is client-side: the route always returns the full
//! catalog (`project_service::list_types`), so the CLI is what keeps
//! `--json`-less output from being a raw dump.

use std::path::Path;

use serde_json::Value;

use crate::http::{Client, build_query};
use crate::output::{format_json, format_table};

use super::{CommandError, body_or_error, resolve_usr};

pub fn request_types(client: &Client, project_dir: &Path) -> Result<Value, CommandError> {
    let query = build_query(&[("project_dir", &project_dir.to_string_lossy())]);
    body_or_error(client.get(&format!("/projects/types?{query}"))?)
}

pub struct TypeFilters<'a> {
    pub kind: Option<&'a str>,
    pub namespace: Option<&'a str>,
}

pub fn render_types(json_mode: bool, body: &Value, filters: &TypeFilters) -> String {
    let empty = Vec::new();
    let all_types = body["types"].as_array().unwrap_or(&empty);
    let filtered: Vec<&Value> = all_types
        .iter()
        .filter(|t| {
            filters
                .kind
                .is_none_or(|kind| t["kind"].as_str() == Some(kind))
        })
        .filter(|t| {
            filters
                .namespace
                .is_none_or(|namespace| t["namespace"].as_str() == Some(namespace))
        })
        .collect();

    if json_mode {
        return format_json(&Value::Array(filtered.into_iter().cloned().collect()));
    }

    let usage_counts = &body["usage_counts"];
    let rows = filtered
        .iter()
        .map(|t| {
            let usr = t["usr"].as_str().unwrap_or_default();
            vec![
                t["name"].as_str().unwrap_or_default().to_owned(),
                t["kind"].as_str().unwrap_or_default().to_owned(),
                t["namespace"].as_str().unwrap_or_default().to_owned(),
                t["file"].as_str().unwrap_or_default().to_owned(),
                usage_counts[usr].as_u64().unwrap_or(0).to_string(),
            ]
        })
        .collect::<Vec<_>>();

    format_table(&["NAME", "KIND", "NAMESPACE", "FILE", "USAGES"], &rows)
}

pub fn request_usages(
    client: &Client,
    project_dir: &Path,
    needle: &str,
) -> Result<Value, CommandError> {
    let types_body = request_types(client, project_dir)?;
    let empty = Vec::new();
    let all_types = types_body["types"].as_array().unwrap_or(&empty);
    let usr = resolve_usr(all_types, needle)?.to_owned();

    let query = build_query(&[
        ("project_dir", &project_dir.to_string_lossy()),
        ("usr", &usr),
    ]);
    body_or_error(client.get(&format!("/projects/types/usages?{query}"))?)
}

pub fn render_usages(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(body);
    }

    let empty = Vec::new();
    let usages = body["usages"].as_array().unwrap_or(&empty);
    let rows = usages
        .iter()
        .map(|u| {
            vec![
                u["kind"].as_str().unwrap_or_default().to_owned(),
                u["file"].as_str().unwrap_or_default().to_owned(),
                format!(
                    "{}:{}",
                    u["line"].as_u64().unwrap_or(0),
                    u["column"].as_u64().unwrap_or(0)
                ),
            ]
        })
        .collect::<Vec<_>>();

    format_table(&["KIND", "FILE", "LINE:COLUMN"], &rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_types() -> Value {
        json!({
            "types": [
                {"name": "Forma", "kind": "class", "namespace": "geometry", "file": "forma.h", "usr": "c:@N@geometry@S@Forma"},
                {"name": "Triangulo", "kind": "class", "namespace": "geometry", "file": "triangulo.h", "usr": "c:@N@geometry@S@Triangulo"},
                {"name": "MAX_SIZE", "kind": "constant_macro", "namespace": "", "file": "limits.h", "usr": "c:limits.h@MAX_SIZE"}
            ],
            "usage_counts": {
                "c:@N@geometry@S@Forma": 3,
                "c:@N@geometry@S@Triangulo": 1
            }
        })
    }

    #[test]
    fn render_types_lists_everything_with_no_filters() {
        let body = sample_types();
        let table = render_types(
            false,
            &body,
            &TypeFilters {
                kind: None,
                namespace: None,
            },
        );
        assert!(table.contains("Forma"));
        assert!(table.contains("Triangulo"));
        assert!(table.contains("MAX_SIZE"));
        assert!(table.contains("3"));
    }

    #[test]
    fn render_types_filters_by_kind() {
        let body = sample_types();
        let table = render_types(
            false,
            &body,
            &TypeFilters {
                kind: Some("constant_macro"),
                namespace: None,
            },
        );
        assert!(table.contains("MAX_SIZE"));
        assert!(!table.contains("Forma"));
    }

    #[test]
    fn render_types_filters_by_namespace() {
        let body = sample_types();
        let table = render_types(
            false,
            &body,
            &TypeFilters {
                kind: None,
                namespace: Some("geometry"),
            },
        );
        assert!(table.contains("Forma"));
        assert!(!table.contains("MAX_SIZE"));
    }

    #[test]
    fn render_usages_lists_kind_file_and_position() {
        let body = json!({
            "usages": [
                {"type_usr": "c:@N@geometry@S@Forma", "kind": "field", "file": "casa.h", "line": 10, "column": 3}
            ]
        });
        let table = render_usages(false, &body);
        assert!(table.contains("field"));
        assert!(table.contains("casa.h"));
        assert!(table.contains("10:3"));
    }
}
