//! Emits [`crate::ir`] as Dart source, formatted to match `dart format`'s
//! output exactly (2-space indent, no trailing whitespace) — criterion 5.2 of
//! `docs/plans/primeiro-corte-e01-e03.md` PR2 requires
//! `dart format --output=none --set-exit-if-changed` to report no diff.
//!
//! `Unsupported` nodes (§4 decision 8 of that plan) are never dropped:
//! - In statement position, they become a `// TODO(syntax-bridge): <reason>`
//!   comment followed by `throw UnimplementedError(...)`.
//! - In expression position, a bare `throw` isn't valid Dart syntax, so they
//!   call a private `Never`-returning helper instead — `Never` unifies with
//!   any expected type, so the surrounding expression still type-checks.
//!   The helper is only emitted into a file that actually needs it.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::ir::{BinaryOp, Expr, Function, Module, Origin, Record, Stmt, Type, UnaryOp};

const INDENT: &str = "  ";
const UNSUPPORTED_HELPER_NAME: &str = "_syntaxBridgeUnsupported";

/// Groups `module`'s records and functions by the C++ source file they came
/// from (one `.dart` file per `.cpp`/`.hpp` — the multi-file/dedup story is
/// E11's armadilha, out of scope here) and emits each, records before
/// functions. Keys are package-relative paths (`lib/<stem>.dart`); a
/// `BTreeMap` keeps the result — and therefore every consumer that iterates
/// it — deterministic (§5 restriction 5).
pub fn emit_module(module: &Module) -> BTreeMap<String, String> {
    let mut functions_by_stem: BTreeMap<String, Vec<&Function>> = BTreeMap::new();
    for function in &module.functions {
        functions_by_stem
            .entry(file_stem(&function.origin.file))
            .or_default()
            .push(function);
    }

    let mut records_by_stem: BTreeMap<String, Vec<&Record>> = BTreeMap::new();
    for record in &module.records {
        records_by_stem
            .entry(file_stem(&record.origin.file))
            .or_default()
            .push(record);
    }

    let stems: BTreeSet<String> = functions_by_stem
        .keys()
        .chain(records_by_stem.keys())
        .cloned()
        .collect();

    stems
        .into_iter()
        .map(|stem| {
            let mut records = records_by_stem.remove(&stem).unwrap_or_default();
            records.sort_by(|a, b| {
                a.origin
                    .line
                    .cmp(&b.origin.line)
                    .then_with(|| a.name.cmp(&b.name))
            });

            let mut functions = functions_by_stem.remove(&stem).unwrap_or_default();
            functions.sort_by(|a, b| {
                a.origin
                    .line
                    .cmp(&b.origin.line)
                    .then_with(|| a.name.cmp(&b.name))
            });

            (format!("lib/{stem}.dart"), emit_file(&records, &functions))
        })
        .collect()
}

fn file_stem(path: &str) -> String {
    let stem = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(sanitize_identifier)
        .unwrap_or_default();

    if stem.is_empty() {
        "output".to_owned()
    } else {
        stem
    }
}

/// Lowercases, folds common Latin diacritics to their base letter (the
/// project and its own example corpus are Portuguese-named — "Função" should
/// become `funcao`, not `fun_o`), and replaces every remaining character
/// outside `[a-z0-9_]` with `_`, collapsing repeats. Shared by file stems and
/// (from PR5 onward) package names, so a project's own naming quirks never
/// produce invalid Dart identifiers.
pub(crate) fn sanitize_identifier(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut previous_was_underscore = false;
    for ch in input
        .chars()
        .flat_map(char::to_lowercase)
        .map(fold_diacritic)
    {
        let normalized = if ch.is_ascii_alphanumeric() { ch } else { '_' };
        if normalized == '_' && previous_was_underscore {
            continue;
        }
        previous_was_underscore = normalized == '_';
        result.push(normalized);
    }

    let trimmed = result.trim_matches('_');
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        format!("_{trimmed}")
    } else {
        trimmed.to_owned()
    }
}

/// Folds a lowercase Latin-1/Latin Extended-A letter with a diacritic to its
/// base ASCII letter. Not general Unicode normalization (no
/// `unicode-normalization` crate is vendored — see
/// `tests/conversion_examples.rs`'s module docs for the same
/// no-new-dependency reasoning) — just the accents that actually show up in
/// this project's own Portuguese names.
fn fold_diacritic(ch: char) -> char {
    match ch {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        'ñ' => 'n',
        'ç' => 'c',
        other => other,
    }
}

fn emit_file(records: &[&Record], functions: &[&Function]) -> String {
    let mut used_expr_helper = false;
    let mut sections: Vec<String> = Vec::new();
    for record in records {
        sections.push(emit_record(record));
    }
    for function in functions {
        sections.push(emit_function(function, &mut used_expr_helper));
    }

    let mut source = sections.join("\n");
    if used_expr_helper {
        if !source.is_empty() {
            source.push('\n');
        }
        source.push_str(&emit_unsupported_helper());
    }
    source
}

fn emit_unsupported_helper() -> String {
    format!(
        "Never {UNSUPPORTED_HELPER_NAME}(String reason) {{\n{INDENT}throw UnimplementedError(reason);\n}}\n"
    )
}

/// A POD `struct`/`class` becomes a Dart class with every field declared
/// `final`... except E03's own armadilha rules that out: `mover` mutates its
/// (by-value-copied) parameter's fields in place, so fields need to stay
/// mutable. A positional constructor (`Ponto(this.x, this.y);`) doubles as
/// the copy constructor `lower::cpp` needs for by-value parameter semantics
/// (`RecordConstruct` emits a call to this same constructor).
fn emit_record(record: &Record) -> String {
    let mut source = format!("class {} {{\n", record.name);
    for field in &record.fields {
        source.push_str(&format!(
            "{INDENT}{} {};\n",
            emit_type(&field.ty),
            field.name
        ));
    }
    let ctor_params = record
        .fields
        .iter()
        .map(|field| format!("this.{}", field.name))
        .collect::<Vec<_>>()
        .join(", ");

    // A field type the IR can't represent (`dynamic /* unsupported: ... */`
    // above) means this class's shape is incomplete — silently allowing
    // construction would accept any value into that field with no signal
    // anything is wrong. The constructor still declares (and assigns, via
    // the `this.field` initializing formals) every field, then throws
    // before returning, the same "compiles, then bails out at the last
    // possible moment" shape `emit_function` uses for its own bail-out.
    match first_unsupported_field_reason(record) {
        Some(reason) => {
            let message = dart_string_literal(&unsupported_message(&reason, &record.origin));
            source.push_str(&format!(
                "\n{INDENT}{}({ctor_params}) {{\n{INDENT}{INDENT}throw UnimplementedError({message});\n{INDENT}}}\n}}\n",
                record.name
            ));
        }
        None => {
            source.push_str(&format!("\n{INDENT}{}({ctor_params});\n}}\n", record.name));
        }
    }
    source
}

fn first_unsupported_field_reason(record: &Record) -> Option<String> {
    record.fields.iter().find_map(|field| match &field.ty {
        Type::Unsupported(spelling) => Some(format!(
            "unsupported field type: {spelling} (field `{}`)",
            field.name
        )),
        _ => None,
    })
}

fn emit_function(function: &Function, used_expr_helper: &mut bool) -> String {
    let params = function
        .params
        .iter()
        .map(|param| format!("{} {}", emit_type(&param.ty), param.name))
        .collect::<Vec<_>>()
        .join(", ");

    // E03's armadilha (`docs/plans/primeiro-corte-e01-e03.md` §7 PR5, see
    // `examples/E03-struct-pod/NOTES.md`): C++ copies a by-value `struct`
    // parameter; Dart passes the reference. `lower::cpp` already inserts an
    // explicit self-reassignment (`p = Ponto(p.x, p.y);`) as the first
    // statement of the body for every such parameter — nothing special is
    // needed here, the clone is just an ordinary `Stmt::Assign` by the time
    // it reaches the emitter. Kept as a comment here (not code) because the
    // interesting decision lives in the lowering step, and duplicating the
    // reasoning risks the two drifting apart.
    // A parameter or return type the IR can't represent is emitted as
    // `dynamic` above — if the body then ran as normal Dart on top of that,
    // it would silently compute on untyped values (e.g. arithmetic on a
    // `long` parameter) instead of ever signaling the translation is
    // incomplete. Checked before the body itself: a signature-level failure
    // takes priority and makes the body's own contents irrelevant.
    //
    // A *body-local* `Type::Unsupported` (a local variable's declared type,
    // or an expression's own inferred type — e.g. `int / long` promoting to
    // `long` under C++'s usual arithmetic conversions) needs the same
    // treatment: `emit_binary_op`'s truncating-division rule, for one, reads
    // exactly that type to decide `/` vs `~/`, and a type it doesn't
    // recognize means that decision can't be trusted either way.
    let bailout_reason = first_unsupported_signature_type(function).or_else(|| {
        first_unsupported_type_in_list(&function.body)
            .map(|spelling| format!("unsupported type in expression: {spelling}"))
    });
    let signature_bailout = bailout_reason.map(|reason| Stmt::Unsupported {
        reason,
        origin: function.origin.clone(),
    });

    let body = match signature_bailout
        .as_ref()
        .or_else(|| first_unsupported_in_list(&function.body))
    {
        // A statement the IR can't represent may have declared a variable
        // (or otherwise established state) that a *later* statement in this
        // same body depends on — emitting only that one statement as a throw
        // and the rest as normal Dart would reference names that were never
        // declared, which is exactly the "compiles and is wrong" failure
        // mode §5's "silêncio é proibido" rule exists to prevent (confirmed
        // empirically: `dart analyze` reports `undefined_identifier` for
        // this, not just a stray warning). So the whole function bails out
        // instead of just the one statement — same shape as a single
        // `Stmt::Unsupported`, using the first one's reason/origin. Searched
        // recursively (nested inside `if`/`while`/`for` bodies too): a
        // conservative rule, not a scope analysis, but a simple one.
        Some(unsupported) => emit_stmt(unsupported, 1, used_expr_helper),
        None => {
            let mut body = String::new();
            for stmt in &function.body {
                body.push_str(&emit_stmt(stmt, 1, used_expr_helper));
            }
            body
        }
    };

    format!(
        "{return_type} {name}({params}) {{\n{body}}}\n",
        return_type = emit_type(&function.return_type),
        name = function.name,
    )
}

fn first_unsupported_signature_type(function: &Function) -> Option<String> {
    if let Type::Unsupported(spelling) = &function.return_type {
        return Some(format!("unsupported return type: {spelling}"));
    }
    function.params.iter().find_map(|param| match &param.ty {
        Type::Unsupported(spelling) => Some(format!(
            "unsupported parameter type: {spelling} (parameter `{}`)",
            param.name
        )),
        _ => None,
    })
}

/// Finds the first `Type::Unsupported` reachable from `body`'s expressions
/// and local declarations — as opposed to `first_unsupported_in_list`, which
/// only finds literal `Stmt::Unsupported`/`Expr::Unsupported` nodes (an
/// unrecognized *shape*). This instead catches a shape the lowering fully
/// understood whose *type* it couldn't represent, which matters because
/// `emit_binary_op` and friends make emission decisions based on that type.
fn first_unsupported_type_in_list(body: &[Stmt]) -> Option<&str> {
    body.iter().find_map(stmt_unsupported_type_spelling)
}

fn stmt_unsupported_type_spelling(stmt: &Stmt) -> Option<&str> {
    match stmt {
        Stmt::Return { value, .. } => value.as_ref().and_then(expr_unsupported_type_spelling),
        Stmt::VarDecl { ty, init, .. } => unsupported_spelling(ty)
            .or_else(|| init.as_ref().and_then(expr_unsupported_type_spelling)),
        Stmt::Assign { value, .. } => expr_unsupported_type_spelling(value),
        Stmt::FieldAssign { target, value, .. } => {
            expr_unsupported_type_spelling(target).or_else(|| expr_unsupported_type_spelling(value))
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => expr_unsupported_type_spelling(condition)
            .or_else(|| first_unsupported_type_in_list(then_branch))
            .or_else(|| first_unsupported_type_in_list(else_branch)),
        Stmt::While {
            condition, body, ..
        } => expr_unsupported_type_spelling(condition)
            .or_else(|| first_unsupported_type_in_list(body)),
        Stmt::For {
            init,
            condition,
            increment,
            body,
            ..
        } => init
            .as_deref()
            .and_then(stmt_unsupported_type_spelling)
            .or_else(|| condition.as_ref().and_then(expr_unsupported_type_spelling))
            .or_else(|| {
                increment
                    .as_deref()
                    .and_then(stmt_unsupported_type_spelling)
            })
            .or_else(|| first_unsupported_type_in_list(body)),
        Stmt::ExprStmt { expr, .. } => expr_unsupported_type_spelling(expr),
        Stmt::Unsupported { .. } => None,
    }
}

fn expr_unsupported_type_spelling(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::IntLiteral { .. } | Expr::BoolLiteral { .. } | Expr::Unsupported { .. } => None,
        Expr::Ref { ty, .. } => unsupported_spelling(ty),
        Expr::Binary { ty, lhs, rhs, .. } => unsupported_spelling(ty)
            .or_else(|| expr_unsupported_type_spelling(lhs))
            .or_else(|| expr_unsupported_type_spelling(rhs)),
        Expr::Unary { ty, operand, .. } => {
            unsupported_spelling(ty).or_else(|| expr_unsupported_type_spelling(operand))
        }
        Expr::Convert { ty, operand, .. } => {
            unsupported_spelling(ty).or_else(|| expr_unsupported_type_spelling(operand))
        }
        Expr::Call { ty, args, .. } => unsupported_spelling(ty)
            .or_else(|| args.iter().find_map(expr_unsupported_type_spelling)),
        Expr::FieldAccess { ty, target, .. } => {
            unsupported_spelling(ty).or_else(|| expr_unsupported_type_spelling(target))
        }
        Expr::RecordConstruct { fields, .. } => fields
            .iter()
            .find_map(|(_name, value)| expr_unsupported_type_spelling(value)),
    }
}

fn unsupported_spelling(ty: &Type) -> Option<&str> {
    match ty {
        Type::Unsupported(spelling) => Some(spelling.as_str()),
        _ => None,
    }
}

fn first_unsupported_in_list(body: &[Stmt]) -> Option<&Stmt> {
    body.iter().find_map(first_unsupported_in_stmt)
}

fn first_unsupported_in_stmt(stmt: &Stmt) -> Option<&Stmt> {
    match stmt {
        Stmt::Unsupported { .. } => Some(stmt),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => first_unsupported_in_list(then_branch)
            .or_else(|| first_unsupported_in_list(else_branch)),
        Stmt::While { body, .. } => first_unsupported_in_list(body),
        Stmt::For {
            init,
            increment,
            body,
            ..
        } => init
            .as_deref()
            .and_then(first_unsupported_in_stmt)
            .or_else(|| increment.as_deref().and_then(first_unsupported_in_stmt))
            .or_else(|| first_unsupported_in_list(body)),
        Stmt::Return { .. }
        | Stmt::VarDecl { .. }
        | Stmt::Assign { .. }
        | Stmt::FieldAssign { .. }
        | Stmt::ExprStmt { .. } => None,
    }
}

fn emit_type(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_owned(),
        Type::Bool => "bool".to_owned(),
        Type::Double => "double".to_owned(),
        Type::Void => "void".to_owned(),
        Type::Record { name, .. } => name.clone(),
        Type::Unsupported(spelling) => format!("dynamic /* unsupported: {spelling} */"),
    }
}

fn emit_stmt(stmt: &Stmt, depth: usize, used_expr_helper: &mut bool) -> String {
    let pad = INDENT.repeat(depth);
    match stmt {
        Stmt::Return { value, .. } => match value {
            Some(expr) => format!("{pad}return {};\n", emit_expr(expr, used_expr_helper)),
            None => format!("{pad}return;\n"),
        },
        // Dart requires every non-nullable local to be initialized where
        // it's declared — `int i;` is as much a compile error as
        // `Ponto p;` would be. `late` defers that requirement to first use,
        // the closest match to C++ letting a local sit default-constructed
        // (or, for a POD's scalar fields, indeterminate) until assigned.
        Stmt::VarDecl { name, ty, init, .. } => match init {
            Some(expr) => format!(
                "{pad}{} {name} = {};\n",
                emit_type(ty),
                emit_expr(expr, used_expr_helper)
            ),
            None => format!("{pad}late {} {name};\n", emit_type(ty)),
        },
        Stmt::Assign { name, value, .. } => {
            format!("{pad}{name} = {};\n", emit_expr(value, used_expr_helper))
        }
        Stmt::FieldAssign {
            target,
            field,
            value,
            ..
        } => format!(
            "{pad}{}.{field} = {};\n",
            emit_expr(target, used_expr_helper),
            emit_expr(value, used_expr_helper)
        ),
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let mut source = format!("{pad}if ({}) {{\n", emit_expr(condition, used_expr_helper));
            for inner in then_branch {
                source.push_str(&emit_stmt(inner, depth + 1, used_expr_helper));
            }
            if else_branch.is_empty() {
                source.push_str(&format!("{pad}}}\n"));
            } else {
                source.push_str(&format!("{pad}}} else {{\n"));
                for inner in else_branch {
                    source.push_str(&emit_stmt(inner, depth + 1, used_expr_helper));
                }
                source.push_str(&format!("{pad}}}\n"));
            }
            source
        }
        Stmt::While {
            condition, body, ..
        } => {
            let mut source = format!(
                "{pad}while ({}) {{\n",
                emit_expr(condition, used_expr_helper)
            );
            for inner in body {
                source.push_str(&emit_stmt(inner, depth + 1, used_expr_helper));
            }
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::For {
            init,
            condition,
            increment,
            body,
            ..
        } => {
            let init_text = init
                .as_deref()
                .map(|stmt| emit_for_clause(stmt, used_expr_helper))
                .unwrap_or_default();
            let condition_text = condition
                .as_ref()
                .map(|expr| emit_expr(expr, used_expr_helper))
                .unwrap_or_default();
            let increment_text = increment
                .as_deref()
                .map(|stmt| emit_for_clause(stmt, used_expr_helper))
                .unwrap_or_default();
            let mut source =
                format!("{pad}for ({init_text}; {condition_text}; {increment_text}) {{\n");
            for inner in body {
                source.push_str(&emit_stmt(inner, depth + 1, used_expr_helper));
            }
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::ExprStmt { expr, .. } => format!("{pad}{};\n", emit_expr(expr, used_expr_helper)),
        Stmt::Unsupported { reason, origin } => format!(
            "{pad}// TODO(syntax-bridge): {reason}\n{pad}throw UnimplementedError({message});\n",
            message = dart_string_literal(&unsupported_message(reason, origin)),
        ),
    }
}

/// A `for`-clause slot (init/increment) wants inline text with no trailing
/// `;`/newline of its own — `emit_stmt`'s shape doesn't fit. Only
/// `VarDecl`/`Assign`/`ExprStmt` are real for-clause shapes from a C++
/// `ForStmt`; anything else falls back to the same `Never`-returning helper
/// `Expr::Unsupported` uses, kept syntactically valid rather than emitting
/// non-expression text into an expression slot.
fn emit_for_clause(stmt: &Stmt, used_expr_helper: &mut bool) -> String {
    match stmt {
        Stmt::VarDecl {
            name,
            ty,
            init: Some(expr),
            ..
        } => format!(
            "{} {name} = {}",
            emit_type(ty),
            emit_expr(expr, used_expr_helper)
        ),
        Stmt::VarDecl {
            name,
            ty,
            init: None,
            ..
        } => format!("late {} {name}", emit_type(ty)),
        Stmt::Assign { name, value, .. } => {
            format!("{name} = {}", emit_expr(value, used_expr_helper))
        }
        Stmt::ExprStmt { expr, .. } => emit_expr(expr, used_expr_helper),
        other => {
            *used_expr_helper = true;
            format!(
                "{UNSUPPORTED_HELPER_NAME}({message})",
                message = dart_string_literal(&unsupported_message(
                    "unexpected statement shape in a for-loop clause",
                    other.origin()
                ))
            )
        }
    }
}

fn emit_expr(expr: &Expr, used_expr_helper: &mut bool) -> String {
    match expr {
        Expr::IntLiteral { value, .. } => value.to_string(),
        Expr::BoolLiteral { value, .. } => value.to_string(),
        Expr::Ref { name, .. } => name.clone(),
        Expr::Binary {
            op, lhs, rhs, ty, ..
        } => format!(
            "{} {} {}",
            emit_expr(lhs, used_expr_helper),
            emit_binary_op(*op, ty),
            emit_expr(rhs, used_expr_helper)
        ),
        Expr::Unary { op, operand, .. } => {
            format!(
                "{}{}",
                emit_unary_op(*op),
                emit_expr(operand, used_expr_helper)
            )
        }
        // The only promotion `lower::cpp` currently constructs a `Convert`
        // node for is int → double (see the IR's own doc comment) — Dart
        // never implicitly widens an `int` expression to `double`.
        Expr::Convert { operand, .. } => {
            format!("{}.toDouble()", emit_expr(operand, used_expr_helper))
        }
        Expr::Call {
            callee_name, args, ..
        } => {
            let args_text = args
                .iter()
                .map(|arg| emit_expr(arg, used_expr_helper))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{callee_name}({args_text})")
        }
        Expr::FieldAccess { target, field, .. } => {
            format!("{}.{field}", emit_expr(target, used_expr_helper))
        }
        Expr::RecordConstruct {
            type_name, fields, ..
        } => {
            let args_text = fields
                .iter()
                .map(|(_name, value)| emit_expr(value, used_expr_helper))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{type_name}({args_text})")
        }
        Expr::Unsupported { reason, origin } => {
            *used_expr_helper = true;
            format!(
                "{UNSUPPORTED_HELPER_NAME}({message})",
                message = dart_string_literal(&unsupported_message(reason, origin))
            )
        }
    }
}

fn emit_binary_op(op: BinaryOp, ty: &Type) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        // C++ `int / int` truncates; Dart `/` always produces a `double`.
        // `~/` is Dart's truncating-division operator — the E02 armadilha
        // (`docs/plans/primeiro-corte-e01-e03.md` §7 PR4). Both truncate
        // toward zero (confirmed empirically: `-7 ~/ 2 == -3` in Dart,
        // matching C++'s `-7 / 2`), so the mapping is exact for `Int`.
        BinaryOp::Div => {
            if matches!(ty, Type::Int) {
                "~/"
            } else {
                "/"
            }
        }
        BinaryOp::Mod => "%",
        BinaryOp::Lt => "<",
        BinaryOp::Le => "<=",
        BinaryOp::Gt => ">",
        BinaryOp::Ge => ">=",
        BinaryOp::Eq => "==",
        BinaryOp::Ne => "!=",
        BinaryOp::And => "&&",
    }
}

fn emit_unary_op(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
    }
}

fn unsupported_message(reason: &str, origin: &Origin) -> String {
    format!("{}:{}: {reason}", origin.file, origin.line)
}

fn dart_string_literal(text: &str) -> String {
    // Order matters: escaping `\` first means the backslashes this
    // introduces for `'` and `$` below are never themselves re-escaped.
    // `$` needs its own pass because Dart interpolates `$identifier`/`${..}`
    // inside single-quoted strings same as double-quoted ones — unescaped,
    // embedded text (e.g. a project path, which can legally contain `$` on
    // Linux) could turn into broken or misinterpreted Dart.
    let escaped = text
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('$', "\\$");
    format!("'{escaped}'")
}
