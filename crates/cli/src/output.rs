//! Rendering shared by every command: a compact aligned table for humans
//! (the default) and an indented tree for the call graph — the two shapes
//! `docs/plans/interface-de-linha-de-comando.md` proposes as the answer to
//! "text can't carry as much as a graphical view can": granular, navigable
//! output instead of a raw dump. `--json` bypasses both and prints the
//! server's response body as-is, for scripts and agents.

use serde::Serialize;
use serde_json::Value;

/// Renders a left-aligned table: each column as wide as its widest cell
/// (header included), columns separated by two spaces — the same shape as
/// `git log --oneline` / `kubectl get`, not a boxed/ruled table.
pub fn format_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let column_count = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|header| header.len()).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate().take(column_count) {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }

    let mut output = String::new();
    push_row(&mut output, headers.iter().map(|h| h.to_string()), &widths);
    for row in rows {
        push_row(&mut output, row.iter().cloned(), &widths);
    }
    output
}

fn push_row(output: &mut String, cells: impl Iterator<Item = String>, widths: &[usize]) {
    let rendered: Vec<String> = cells
        .enumerate()
        .map(|(index, cell)| {
            let width = widths.get(index).copied().unwrap_or(0);
            if index + 1 == widths.len() {
                // Last column isn't padded — avoids trailing whitespace on
                // every line.
                cell
            } else {
                format!("{cell:width$}")
            }
        })
        .collect();
    output.push_str(&rendered.join("  "));
    output.push('\n');
}

pub fn format_json(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// A node in the textual call-graph tree `functions callers` renders —
/// see `commands::functions::build_callers_tree`. `Serialize` so `--json`
/// can emit the same tree it prints as text, instead of a differently
/// shaped payload for agents.
#[derive(Serialize)]
pub struct TreeNode {
    pub label: String,
    pub children: Vec<TreeNode>,
}

/// Renders `root` as an indented tree using the same box-drawing
/// connectors `tree`/most IDE "find callers" panels use, so a call graph
/// stays legible without pixels:
///
/// ```text
/// Triangulo::area
/// ├─ Forma::calcularTotal
/// │  └─ main
/// └─ Relatorio::resumir
/// ```
pub fn render_tree(root: &TreeNode) -> String {
    let mut output = root.label.clone();
    output.push('\n');
    render_children(&mut output, &root.children, "");
    output
}

fn render_children(output: &mut String, children: &[TreeNode], prefix: &str) {
    let last_index = children.len().checked_sub(1);
    for (index, child) in children.iter().enumerate() {
        let is_last = Some(index) == last_index;
        let connector = if is_last { "└─ " } else { "├─ " };
        output.push_str(prefix);
        output.push_str(connector);
        output.push_str(&child.label);
        output.push('\n');

        let child_prefix = format!("{prefix}{}", if is_last { "   " } else { "│  " });
        render_children(output, &child.children, &child_prefix);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_table_aligns_columns_to_their_widest_cell() {
        let table = format_table(
            &["Name", "Kind"],
            &[
                vec!["Forma".to_owned(), "Struct".to_owned()],
                vec!["X".to_owned(), "Enum".to_owned()],
            ],
        );

        assert_eq!(table, "Name   Kind\nForma  Struct\nX      Enum\n");
    }

    #[test]
    fn format_table_handles_no_rows() {
        assert_eq!(format_table(&["Name"], &[]), "Name\n");
    }

    #[test]
    fn render_tree_matches_the_proposal_example() {
        let tree = TreeNode {
            label: "Triangulo::area".to_owned(),
            children: vec![
                TreeNode {
                    label: "Forma::calcularTotal".to_owned(),
                    children: vec![TreeNode {
                        label: "main".to_owned(),
                        children: vec![],
                    }],
                },
                TreeNode {
                    label: "Relatorio::resumir".to_owned(),
                    children: vec![],
                },
            ],
        };

        assert_eq!(
            render_tree(&tree),
            "Triangulo::area\n\
             ├─ Forma::calcularTotal\n\
             │  └─ main\n\
             └─ Relatorio::resumir\n"
        );
    }

    #[test]
    fn render_tree_handles_a_leaf_root() {
        let tree = TreeNode {
            label: "lonely".to_owned(),
            children: vec![],
        };
        assert_eq!(render_tree(&tree), "lonely\n");
    }
}
