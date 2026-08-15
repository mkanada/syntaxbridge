//! `sb functions`, `sb functions callers <name>` and
//! `sb functions calls-in-file --file`. `callers` is the CLI's answer to
//! "text can't show a graph" (`docs/plans/interface-de-linha-de-comando.md`):
//! it walks `GET /projects/functions/callers` recursively up to `--depth`
//! and renders the result as an indented tree (or, with `--format dot`, as
//! Graphviz for an agent or `dot -Tsvg` to actually draw).

use std::collections::{HashMap, HashSet};
use std::path::Path;

use serde_json::Value;

use crate::http::{Client, build_query};
use crate::output::{TreeNode, format_json, format_table, render_tree};

use super::{CommandError, body_or_error};

pub fn request_functions(client: &Client, project_dir: &Path) -> Result<Value, CommandError> {
    let query = build_query(&[("project_dir", &project_dir.to_string_lossy())]);
    body_or_error(client.get(&format!("/projects/functions?{query}"))?)
}

/// A method's `namespace` field (`type_catalog`/`function_catalog`) is the
/// *enclosing C++ namespace*, not its owning class — `owning_class_usr`
/// carries that separately, as a `usr` rather than a name. Two methods
/// named `area` on different classes both show up with the same bare
/// `namespace` (often empty), so without resolving `owning_class_usr`
/// against the type catalog, `Forma::area` and `Triangulo::area` are
/// indistinguishable to `sb functions callers area` — this map is what
/// makes that resolution possible. Built once per command from
/// `GET /projects/types`, keyed by `usr`.
pub type ClassNames = HashMap<String, String>;

pub fn class_names_from_types(types: &[Value]) -> ClassNames {
    types
        .iter()
        .filter_map(|t| {
            Some((
                t["usr"].as_str()?.to_owned(),
                t["name"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn qualified_name(item: &Value, class_names: &ClassNames) -> String {
    let name = item["name"].as_str().unwrap_or_default();
    let namespace = item["namespace"].as_str().unwrap_or_default();
    let owning_class = item["owning_class_usr"]
        .as_str()
        .and_then(|usr| class_names.get(usr));

    let scope = match (namespace.is_empty(), owning_class) {
        (true, None) => String::new(),
        (true, Some(class_name)) => class_name.clone(),
        (false, None) => namespace.to_owned(),
        (false, Some(class_name)) => format!("{namespace}::{class_name}"),
    };

    if scope.is_empty() {
        name.to_owned()
    } else {
        format!("{scope}::{name}")
    }
}

pub fn render_functions(
    json_mode: bool,
    body: &Value,
    filter: Option<&str>,
    class_names: &ClassNames,
) -> String {
    let empty = Vec::new();
    let all_functions = body["functions"].as_array().unwrap_or(&empty);
    let filtered: Vec<&Value> = all_functions
        .iter()
        .filter(|f| {
            filter.is_none_or(|needle| {
                qualified_name(f, class_names)
                    .to_lowercase()
                    .contains(&needle.to_lowercase())
            })
        })
        .collect();

    if json_mode {
        return format_json(&Value::Array(filtered.into_iter().cloned().collect()));
    }

    let caller_counts = &body["caller_counts"];
    let rows = filtered
        .iter()
        .map(|f| {
            let usr = f["usr"].as_str().unwrap_or_default();
            vec![
                qualified_name(f, class_names),
                f["kind"].as_str().unwrap_or_default().to_owned(),
                f["signature"].as_str().unwrap_or_default().to_owned(),
                caller_counts[usr].as_u64().unwrap_or(0).to_string(),
            ]
        })
        .collect::<Vec<_>>();

    format_table(&["NAME", "KIND", "SIGNATURE", "CALLERS"], &rows)
}

pub fn request_callers(
    client: &Client,
    project_dir: &Path,
    usr: &str,
) -> Result<Value, CommandError> {
    let query = build_query(&[
        ("project_dir", &project_dir.to_string_lossy()),
        ("usr", usr),
    ]);
    body_or_error(client.get(&format!("/projects/functions/callers?{query}"))?)
}

/// Resolves a human-typed function name/USR against `functions`, the same
/// way `resolve_usr` does for types — reimplemented locally (rather than
/// reusing `super::resolve_usr`) because matching and the ambiguity
/// message both need `class_names`-aware `qualified_name`, which a bare
/// `{name, namespace, usr}` match can't produce for methods.
pub fn resolve_function(
    functions: &[Value],
    needle: &str,
    class_names: &ClassNames,
) -> Result<String, CommandError> {
    if let Some(item) = functions.iter().find(|f| f["usr"] == needle) {
        return Ok(item["usr"].as_str().unwrap_or_default().to_owned());
    }

    let matches: Vec<&Value> = functions
        .iter()
        .filter(|f| qualified_name(f, class_names) == needle || f["name"] == needle)
        .collect();

    match matches.as_slice() {
        [] => Err(CommandError::Server {
            status: 404,
            message: format!("nenhuma função encontrada para {needle:?}"),
        }),
        [only] => Ok(only["usr"].as_str().unwrap_or_default().to_owned()),
        many => {
            let candidates = many
                .iter()
                .map(|f| format!("{} ({})", qualified_name(f, class_names), f["usr"]))
                .collect::<Vec<_>>()
                .join(", ");
            Err(CommandError::Server {
                status: 409,
                message: format!("{needle:?} é ambíguo, candidatos: {candidates}"),
            })
        }
    }
}

/// Builds the caller tree rooted at `root_usr`, up to `max_depth` levels,
/// by calling `GET /projects/functions/callers` once per node expanded.
/// `max_depth` of `0` means "root only" (no network calls beyond the
/// initial one implied by the caller already having `root_usr`).
///
/// Guards against cycles in the call graph (mutual recursion) by tracking
/// the current root-to-node path, not every node visited anywhere in the
/// tree — a function legitimately called from two different branches is
/// not a cycle, only a function calling back into its own ancestry is.
pub fn build_callers_tree(
    client: &Client,
    project_dir: &Path,
    functions: &[Value],
    class_names: &ClassNames,
    root_usr: &str,
    max_depth: u32,
) -> Result<TreeNode, CommandError> {
    let mut visited = HashSet::new();
    visited.insert(root_usr.to_owned());
    let children = collect_children(
        client,
        project_dir,
        functions,
        class_names,
        root_usr,
        max_depth,
        &mut visited,
    )?;
    Ok(TreeNode {
        label: label_for(functions, class_names, root_usr),
        children,
    })
}

#[allow(clippy::too_many_arguments)]
fn collect_children(
    client: &Client,
    project_dir: &Path,
    functions: &[Value],
    class_names: &ClassNames,
    usr: &str,
    remaining_depth: u32,
    visited: &mut HashSet<String>,
) -> Result<Vec<TreeNode>, CommandError> {
    if remaining_depth == 0 {
        return Ok(Vec::new());
    }

    let body = request_callers(client, project_dir, usr)?;
    let empty = Vec::new();
    let callers = body["callers"].as_array().unwrap_or(&empty);

    let mut nodes = Vec::new();
    for edge in callers {
        let caller_usr = edge["caller_usr"].as_str().unwrap_or_default().to_owned();
        let label = label_for(functions, class_names, &caller_usr);

        if !visited.insert(caller_usr.clone()) {
            nodes.push(TreeNode {
                label: format!("{label} (ciclo)"),
                children: Vec::new(),
            });
            continue;
        }

        let children = collect_children(
            client,
            project_dir,
            functions,
            class_names,
            &caller_usr,
            remaining_depth - 1,
            visited,
        )?;
        visited.remove(&caller_usr);

        nodes.push(TreeNode { label, children });
    }
    Ok(nodes)
}

fn label_for(functions: &[Value], class_names: &ClassNames, usr: &str) -> String {
    functions
        .iter()
        .find(|f| f["usr"] == usr)
        .map(|f| qualified_name(f, class_names))
        .unwrap_or_else(|| usr.to_owned())
}

pub fn render_callers_tree_text(tree: &TreeNode) -> String {
    render_tree(tree)
}

pub fn render_callers_tree_json(tree: &TreeNode) -> String {
    format_json(&serde_json::to_value(tree).unwrap_or(Value::Null))
}

/// Renders the tree as Graphviz DOT: an edge per caller → callee
/// relationship, `child -> parent` since a child node in this tree is a
/// caller *of* its parent. Meant for `--format dot`, consumed either by a
/// human piping into `dot -Tsvg` or by an agent that wants the graph shape
/// without walking nested JSON.
pub fn render_callers_tree_dot(tree: &TreeNode) -> String {
    let mut edges = String::new();
    collect_dot_edges(tree, &mut edges);
    format!("digraph calls {{\n{edges}}}\n")
}

fn collect_dot_edges(node: &TreeNode, out: &mut String) {
    for child in &node.children {
        out.push_str(&format!("  {:?} -> {:?};\n", child.label, node.label));
        collect_dot_edges(child, out);
    }
}

pub fn request_calls_in_file(
    client: &Client,
    project_dir: &Path,
    file: &str,
) -> Result<Value, CommandError> {
    let query = build_query(&[
        ("project_dir", &project_dir.to_string_lossy()),
        ("file", file),
    ]);
    body_or_error(client.get(&format!("/projects/functions/calls-in-file?{query}"))?)
}

pub fn render_calls_in_file(json_mode: bool, body: &Value) -> String {
    if json_mode {
        return format_json(body);
    }

    let empty = Vec::new();
    let calls = body["calls"].as_array().unwrap_or(&empty);
    let rows = calls
        .iter()
        .map(|call| {
            let resolution = &call["resolution"];
            let target = match resolution["status"].as_str() {
                Some("resolved") => resolution["callee_usr"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                Some("unresolved") => {
                    format!(
                        "(não resolvido: {})",
                        resolution["reason"].as_str().unwrap_or_default()
                    )
                }
                _ => String::new(),
            };
            vec![
                call["caller_usr"].as_str().unwrap_or_default().to_owned(),
                target,
                format!(
                    "{}:{}",
                    call["line"].as_u64().unwrap_or(0),
                    call["column"].as_u64().unwrap_or(0)
                ),
            ]
        })
        .collect::<Vec<_>>();

    format_table(&["CALLER", "CALLEE", "LINE:COLUMN"], &rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_functions() -> Vec<Value> {
        vec![
            json!({"name": "area", "namespace": "", "owning_class_usr": "c:@S@Triangulo", "kind": "method", "signature": "double Triangulo::area() const", "usr": "u_triangulo_area"}),
            json!({"name": "area", "namespace": "", "owning_class_usr": "c:@S@Forma", "kind": "method", "signature": "double Forma::area() const", "usr": "u_forma_area"}),
            json!({"name": "calcularTotal", "namespace": "", "owning_class_usr": "c:@S@Forma", "kind": "method", "signature": "double Forma::calcularTotal()", "usr": "u_total"}),
            json!({"name": "main", "namespace": "", "owning_class_usr": null, "kind": "free_function", "signature": "int main()", "usr": "u_main"}),
        ]
    }

    fn sample_class_names() -> ClassNames {
        [
            ("c:@S@Triangulo".to_owned(), "Triangulo".to_owned()),
            ("c:@S@Forma".to_owned(), "Forma".to_owned()),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn qualified_name_disambiguates_methods_from_different_classes() {
        let class_names = sample_class_names();
        let functions = sample_functions();
        assert_eq!(
            qualified_name(&functions[0], &class_names),
            "Triangulo::area"
        );
        assert_eq!(qualified_name(&functions[1], &class_names), "Forma::area");
    }

    #[test]
    fn render_functions_filters_case_insensitively_on_the_disambiguated_name() {
        let body = json!({
            "functions": sample_functions(),
            "caller_counts": {"u_triangulo_area": 2}
        });
        let table = render_functions(false, &body, Some("triangulo"), &sample_class_names());
        assert!(table.contains("Triangulo::area"));
        assert!(!table.contains("Forma::area"));
        assert!(!table.contains("main"));
    }

    #[test]
    fn resolve_function_disambiguates_a_bare_name_by_owning_class() {
        let class_names = sample_class_names();
        let functions = sample_functions();
        let usr = resolve_function(&functions, "Triangulo::area", &class_names).unwrap();
        assert_eq!(usr, "u_triangulo_area");
    }

    #[test]
    fn resolve_function_reports_ambiguity_with_class_qualified_candidates() {
        let class_names = sample_class_names();
        let functions = sample_functions();
        let error = resolve_function(&functions, "area", &class_names).unwrap_err();
        match error {
            CommandError::Server {
                status: 409,
                message,
            } => {
                assert!(message.contains("Triangulo::area"));
                assert!(message.contains("Forma::area"));
            }
            other => panic!("expected an ambiguous-match error, got {other:?}"),
        }
    }

    #[test]
    fn label_for_falls_back_to_the_usr_when_the_function_is_unknown() {
        let class_names = sample_class_names();
        assert_eq!(label_for(&[], &class_names, "u_mystery"), "u_mystery");
        assert_eq!(
            label_for(&sample_functions(), &class_names, "u_triangulo_area"),
            "Triangulo::area"
        );
    }

    #[test]
    fn render_callers_tree_text_matches_the_expected_indentation() {
        let tree = TreeNode {
            label: "Triangulo::area".to_owned(),
            children: vec![TreeNode {
                label: "Forma::calcularTotal".to_owned(),
                children: vec![TreeNode {
                    label: "main".to_owned(),
                    children: vec![],
                }],
            }],
        };
        let rendered = render_callers_tree_text(&tree);
        assert_eq!(
            rendered,
            "Triangulo::area\n└─ Forma::calcularTotal\n   └─ main\n"
        );
    }

    #[test]
    fn render_callers_tree_dot_emits_child_to_parent_edges() {
        let tree = TreeNode {
            label: "area".to_owned(),
            children: vec![TreeNode {
                label: "calcularTotal".to_owned(),
                children: vec![],
            }],
        };
        let dot = render_callers_tree_dot(&tree);
        assert!(dot.starts_with("digraph calls {\n"));
        assert!(dot.contains("\"calcularTotal\" -> \"area\";"));
    }

    #[test]
    fn render_callers_tree_json_round_trips_labels() {
        let tree = TreeNode {
            label: "root".to_owned(),
            children: vec![TreeNode {
                label: "child".to_owned(),
                children: vec![],
            }],
        };
        let rendered = render_callers_tree_json(&tree);
        let reparsed: Value = serde_json::from_str(&rendered).expect("valid JSON");
        assert_eq!(reparsed["label"], "root");
        assert_eq!(reparsed["children"][0]["label"], "child");
    }

    #[test]
    fn render_calls_in_file_shows_unresolved_calls_with_their_reason() {
        let body = json!({
            "calls": [
                {
                    "caller_usr": "u_main",
                    "resolution": {"status": "unresolved", "reason": "function pointer"},
                    "file": "main.cpp",
                    "line": 5,
                    "column": 3
                },
                {
                    "caller_usr": "u_main",
                    "resolution": {"status": "resolved", "callee_usr": "u_area", "is_dynamic_dispatch": false},
                    "file": "main.cpp",
                    "line": 6,
                    "column": 3
                }
            ]
        });
        let table = render_calls_in_file(false, &body);
        assert!(table.contains("não resolvido: function pointer"));
        assert!(table.contains("u_area"));
    }
}
