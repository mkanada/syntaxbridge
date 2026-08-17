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

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use crate::ir::{
    BinaryOp, Constructor, Enum, Expr, Function, Method, Module, Origin, Param, Record, Stmt, Type,
    UnaryOp,
};

const INDENT: &str = "  ";
const UNSUPPORTED_HELPER_NAME: &str = "_syntaxBridgeUnsupported";

/// Groups `module`'s records and functions by the C++ source file they came
/// from (one `.dart` file per `.cpp`/`.hpp`) and emits each, records before
/// functions — including, since E11, the `import`s a file needs for
/// whichever other files it references (a header shared by more than one
/// `.cpp`, E11's own armadilha, means a type or free function's home file
/// often isn't the file using it). Keys are package-relative paths
/// (`lib/<stem>.dart`); a `BTreeMap` keeps the result — and therefore every
/// consumer that iterates it — deterministic (§5 restriction 5).
pub fn emit_module(module: &Module) -> BTreeMap<String, String> {
    emit_module_with_externals(module, &HashSet::new())
}

/// Like [`emit_module`], but every callable (free function, method,
/// constructor) whose usr is in `external_usrs`
/// (`docs/plans/lista-de-externos.md` — the effective external set
/// `project_service::build_transpiled_package` computes from
/// `externals::effective_external_set`) gets a mock body instead of its
/// real one, or the `Unsupported` bailout it would otherwise fall back to.
/// `external_usrs` holds borrowed `&str`s rather than owned `String`s so a
/// caller that already has `Vec<ExternalStatus>` (usrs as `String`) can
/// build the set with one pass of `.iter().map(String::as_str)`, without an
/// intermediate clone of every usr.
pub fn emit_module_with_externals(
    module: &Module,
    external_usrs: &HashSet<&str>,
) -> BTreeMap<String, String> {
    // E09: gathered across the *whole* module, not per-file — a mixin and
    // the class that uses it could in principle land in different files
    // (multi-TU dedup is E11's own armadilha, not reopened here), and
    // `emit_record` needs to know "is this record used as a mixin
    // somewhere" before it decides whether to emit `class` or `mixin` and
    // whether its fields need a default value (a `mixin` can't have any
    // constructor at all, unlike the ordinary synthetic positional one E03
    // gives every other record with fields — see `Record::mixins`'s doc
    // comment).
    let mixin_usrs = mixin_usrs(module);

    // Needed by `expand_mixin_chain`: a leaf class's `with` clause must
    // transitively pull in the `on` dependencies of every mixin it lists
    // (see that function's doc comment), which means looking up each listed
    // mixin's own `base_class`/`mixins` by usr — module-wide, since (like
    // `mixin_usrs` above) the mixin and the class applying it can land in
    // different files.
    let records_by_usr: HashMap<&str, &Record> = module
        .records
        .iter()
        .map(|record| (record.usr.as_str(), record))
        .collect();

    // E11: which file *declares* each top-level record/function — the other
    // half of what a file needs to know before it can print its own
    // `import`s. Method/constructor usrs aren't included (only reachable
    // through the record that owns them, always emitted in the same file as
    // that record — see `emit_file`'s doc comment on the one gap this
    // leaves: a cross-file *method* call, which no fixture exercises yet).
    let mut usr_to_stem: HashMap<&str, String> = HashMap::new();
    for record in &module.records {
        usr_to_stem.insert(record.usr.as_str(), file_stem(&record.origin.file));
    }
    for function in &module.functions {
        usr_to_stem.insert(function.usr.as_str(), file_stem(&function.origin.file));
    }
    for enum_decl in &module.enums {
        usr_to_stem.insert(enum_decl.usr.as_str(), file_stem(&enum_decl.origin.file));
    }

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

    let mut enums_by_stem: BTreeMap<String, Vec<&Enum>> = BTreeMap::new();
    for enum_decl in &module.enums {
        enums_by_stem
            .entry(file_stem(&enum_decl.origin.file))
            .or_default()
            .push(enum_decl);
    }

    // Module-wide, not per file: a field's enum type is routinely declared
    // in a different file from the record that holds it, and
    // `field_default_literal` needs that enum's first constant to give the
    // field a valid default.
    let enums_by_usr: HashMap<&str, &Enum> = module
        .enums
        .iter()
        .map(|enum_decl| (enum_decl.usr.as_str(), enum_decl))
        .collect();

    let mock = MockContext {
        external_usrs,
        enums_by_usr: &enums_by_usr,
        records_by_usr: &records_by_usr,
    };

    let stems: BTreeSet<String> = functions_by_stem
        .keys()
        .chain(records_by_stem.keys())
        .chain(enums_by_stem.keys())
        .cloned()
        .collect();

    stems
        .into_iter()
        .map(|stem| {
            let mut enums = enums_by_stem.remove(&stem).unwrap_or_default();
            enums.sort_by(|a, b| {
                a.origin
                    .line
                    .cmp(&b.origin.line)
                    .then_with(|| a.name.cmp(&b.name))
            });

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

            (
                format!("lib/{stem}.dart"),
                emit_file(
                    &stem,
                    &enums,
                    &records,
                    &functions,
                    &mixin_usrs,
                    &records_by_usr,
                    &usr_to_stem,
                    &enums_by_usr,
                    &mock,
                ),
            )
        })
        .collect()
}

/// Every record `usr` used as a mixin (E09) somewhere in `module`, computed
/// module-wide rather than per-file — a mixin and the class that uses it
/// could land in different files. Shared with `crate::validate::dart`, which
/// needs the same `class`-vs-`mixin` keyword this module already derives
/// from it, to build a matching declaration marker without recomputing this
/// itself and risking the two disagreeing.
///
/// Transitive, not just direct `record.mixins` targets: `expand_mixin_chain`
/// pulls a listed mixin's own `base_class`/`mixins` dependencies into
/// whichever leaf class actually applies it via `with` — a dependency that's
/// only ever reached that way (e.g. `AttAltSym`'s own single `base_class`,
/// `Att`, never named directly in any record's `mixins` list) still ends up
/// in a `with` clause somewhere, and Dart's `mixin_of_non_class` fires the
/// moment that clause names something declared `class` instead of `mixin`.
/// Closing over the same `base_class`/`mixins` edges `expand_mixin_chain`
/// itself walks keeps the two from disagreeing about what needs the `mixin`
/// keyword.
pub(crate) fn mixin_usrs(module: &Module) -> HashSet<&str> {
    let records_by_usr: HashMap<&str, &Record> = module
        .records
        .iter()
        .map(|record| (record.usr.as_str(), record))
        .collect();

    let mut result: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = module
        .records
        .iter()
        .flat_map(|record| record.mixins.iter().map(|base| base.usr.as_str()))
        .collect();
    while let Some(usr) = stack.pop() {
        if !result.insert(usr) {
            continue;
        }
        if let Some(record) = records_by_usr.get(usr) {
            stack.extend(
                record
                    .base_class
                    .iter()
                    .chain(record.mixins.iter())
                    .map(|base| base.usr.as_str()),
            );
        }
    }
    result
}

/// The full `with`-clause a `class` needs to actually apply `bases`
/// (`record.mixins`) once every one of Dart's `on`-constrained mixins in the
/// chain (see `emit_record`'s `is_mixin` branch) is accounted for. A mixin
/// declared `mixin M on A, B {}` only type-checks when applied to a class
/// hierarchy that *already* has `A` and `B` — `mixin_usrs` was never enough
/// to see that on its own, since a listed mixin like `AltSymInterface` can
/// itself be built from further bases (`Interface`, `AttAltSym`) that the
/// class applying it must list *first*, even though nothing in the C++
/// source ever names them together (`ControlElement` inherits
/// `AltSymInterface` directly, never `Interface`/`AttAltSym`). Recurses
/// depth-first and pushes each base only after its own dependencies, so the
/// result is already in valid application order; `seen` dedups by usr,
/// keeping the first (i.e. earliest-required) occurrence — Dart tolerates a
/// repeated mixin in one `with` clause, but a duplicate never adds anything
/// a single occurrence didn't already satisfy. A base whose usr isn't a
/// record in this module (an unresolved/external type, same as any other
/// `BaseClass` this emitter meets) is kept as a leaf: printed by name, with
/// no dependencies of its own to expand.
///
/// Returns the full `BaseClass` (usr *and* name), not just the printed
/// name — `collect_referenced_usrs_in_record` needs the usr half to resolve
/// each expanded dependency to the file it has to `import`, the same
/// expansion `emit_record`'s own `with` clause already relies on to know
/// what it needs printed by name.
fn expand_mixin_chain<'a>(
    bases: &'a [crate::ir::BaseClass],
    records_by_usr: &HashMap<&str, &'a Record>,
) -> Vec<&'a crate::ir::BaseClass> {
    fn visit<'a>(
        base: &'a crate::ir::BaseClass,
        records_by_usr: &HashMap<&str, &'a Record>,
        seen: &mut HashSet<&'a str>,
        result: &mut Vec<&'a crate::ir::BaseClass>,
    ) {
        if !seen.insert(base.usr.as_str()) {
            return;
        }
        if let Some(record) = records_by_usr.get(base.usr.as_str()) {
            for dependency in record.base_class.iter().chain(record.mixins.iter()) {
                visit(dependency, records_by_usr, seen, result);
            }
        }
        result.push(base);
    }

    let mut seen = HashSet::new();
    let mut result = Vec::new();
    for base in bases {
        visit(base, records_by_usr, &mut seen, &mut result);
    }
    result
}

pub(crate) fn file_stem(path: &str) -> String {
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

/// One `.dart` file's worth of records and functions, plus the two
/// cross-file/cross-record lookups it needs to emit correctly: `mixin_usrs`
/// (E09 — whether a record it declares needs to come out as `mixin`, not
/// `class`) and `usr_to_stem` (E11 — every top-level record/function's
/// *declaring* file, so this file can `import` whichever other files it
/// actually references — a header shared by more than one `.cpp` (E11's own
/// armadilha) means a type or free function's home file is often not this
/// one). Doesn't resolve a cross-file *method* call (a method's own usr
/// isn't in `usr_to_stem` — only its owning record's is, and no fixture
/// yet calls a method whose receiver's static type lives in another file)
/// — deliberately narrower than the free-function/record-type case, not a
/// silent gap: `dart analyze`'s `undefined_method` would surface it loudly
/// if it ever mattered. `records_by_usr` is the third such lookup, added for
/// `expand_mixin_chain`'s own module-wide `with`-clause expansion.
#[allow(clippy::too_many_arguments)]
fn emit_file(
    stem: &str,
    enums: &[&Enum],
    records: &[&Record],
    functions: &[&Function],
    mixin_usrs: &HashSet<&str>,
    records_by_usr: &HashMap<&str, &Record>,
    usr_to_stem: &HashMap<&str, String>,
    enums_by_usr: &HashMap<&str, &Enum>,
    mock: &MockContext<'_>,
) -> String {
    let mut used_expr_helper = false;
    // Set by `Expr::StringByteLength` (E05's UTF-8-byte-length bridge for
    // `std::string::size()` — see that variant's doc comment): needs
    // `dart:convert`'s `utf8`, which nothing else in this emitter uses, so
    // the import is added only when a file actually needs it, the same
    // opt-in shape `used_expr_helper` already uses for its own helper
    // function.
    let mut used_utf8_encode = false;
    let mut sections: Vec<String> = Vec::new();
    for enum_decl in enums {
        sections.push(emit_enum(enum_decl));
    }
    for record in records {
        sections.push(emit_record(
            record,
            mixin_usrs.contains(record.usr.as_str()),
            records_by_usr,
            &mut used_expr_helper,
            &mut used_utf8_encode,
            enums_by_usr,
            mock,
        ));
    }
    for function in functions {
        sections.push(emit_function(
            function,
            mock,
            &mut used_expr_helper,
            &mut used_utf8_encode,
        ));
    }

    let mut source = sections.join("\n");
    if used_expr_helper {
        if !source.is_empty() {
            source.push('\n');
        }
        source.push_str(&emit_unsupported_helper());
    }

    let mut referenced_usrs: HashSet<&str> = HashSet::new();
    for record in records {
        collect_referenced_usrs_in_record(record, records_by_usr, &mut referenced_usrs);
    }
    for function in functions {
        collect_referenced_usrs_in_type(&function.return_type, &mut referenced_usrs);
        for param in &function.params {
            collect_referenced_usrs_in_type(&param.ty, &mut referenced_usrs);
        }
        collect_referenced_usrs_in_stmts(&function.body, &mut referenced_usrs);
    }
    let needed_imports: BTreeSet<&str> = referenced_usrs
        .into_iter()
        .filter_map(|usr| usr_to_stem.get(usr))
        .map(String::as_str)
        .filter(|other_stem| *other_stem != stem)
        .collect();

    let mut import_lines: Vec<String> = Vec::new();
    if used_utf8_encode {
        import_lines.push("import 'dart:convert';".to_owned());
    }
    for other_stem in &needed_imports {
        import_lines.push(format!("import '{other_stem}.dart';"));
    }
    if !import_lines.is_empty() {
        source = format!("{}\n\n{source}", import_lines.join("\n"));
    }
    source
}

fn emit_unsupported_helper() -> String {
    format!(
        "Never {UNSUPPORTED_HELPER_NAME}(String reason) {{\n{INDENT}throw UnimplementedError(reason);\n}}\n"
    )
}

/// E11: every `usr` a record's own shape and members reach — field/static
/// field types, base class and mixins, and every constructor's/method's own
/// params/return type/body — the input `emit_file` needs to decide which
/// other files this one has to `import`.
///
/// `mixins` goes through the same `expand_mixin_chain` `emit_record` prints
/// into the `with` clause, not just the direct list: a leaf class's own
/// `with` names every transitive `on` dependency by name (Dart requires
/// each spelled out, not resolved through imports the way `extends`' single
/// target is), so this file needs an `import` for every one of them, not
/// only the record's *direct* C++ bases — a real gap in the real Verovio
/// 6.2.0 corpus (`Abbr`'s two direct bases pull in eight more names
/// transitively, none imported without this).
fn collect_referenced_usrs_in_record<'a>(
    record: &'a Record,
    records_by_usr: &HashMap<&str, &'a Record>,
    out: &mut HashSet<&'a str>,
) {
    for field in record.fields.iter().chain(&record.static_fields) {
        collect_referenced_usrs_in_type(&field.ty, out);
    }
    if let Some(base) = &record.base_class {
        out.insert(base.usr.as_str());
    }
    for mixin in expand_mixin_chain(&record.mixins, records_by_usr) {
        out.insert(mixin.usr.as_str());
    }
    for constructor in &record.constructors {
        for param in &constructor.params {
            collect_referenced_usrs_in_type(&param.ty, out);
        }
        collect_referenced_usrs_in_stmts(&constructor.body, out);
    }
    for method in &record.methods {
        collect_referenced_usrs_in_type(&method.return_type, out);
        for param in &method.params {
            collect_referenced_usrs_in_type(&param.ty, out);
        }
        if let Some(body) = &method.body {
            collect_referenced_usrs_in_stmts(body, out);
        }
    }
}

fn collect_referenced_usrs_in_type<'a>(ty: &'a Type, out: &mut HashSet<&'a str>) {
    match ty {
        Type::Record { usr, .. } | Type::Enum { usr, .. } => {
            out.insert(usr.as_str());
        }
        Type::List(element) | Type::Set(element) => collect_referenced_usrs_in_type(element, out),
        Type::Map(key, value) => {
            collect_referenced_usrs_in_type(key, out);
            collect_referenced_usrs_in_type(value, out);
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect_referenced_usrs_in_type(element, out);
            }
        }
        Type::Nullable(inner) => collect_referenced_usrs_in_type(inner, out),
        Type::Int | Type::Bool | Type::Double | Type::Void | Type::Str | Type::Unsupported(_) => {}
    }
}

fn collect_referenced_usrs_in_stmts<'a>(stmts: &'a [Stmt], out: &mut HashSet<&'a str>) {
    for stmt in stmts {
        collect_referenced_usrs_in_stmt(stmt, out);
    }
}

fn collect_referenced_usrs_in_stmt<'a>(stmt: &'a Stmt, out: &mut HashSet<&'a str>) {
    match stmt {
        Stmt::Return { value, .. } => {
            if let Some(expr) = value {
                collect_referenced_usrs_in_expr(expr, out);
            }
        }
        Stmt::VarDecl { ty, init, .. } => {
            collect_referenced_usrs_in_type(ty, out);
            if let Some(expr) = init {
                collect_referenced_usrs_in_expr(expr, out);
            }
        }
        Stmt::Assign { value, .. } => collect_referenced_usrs_in_expr(value, out),
        Stmt::FieldAssign { target, value, .. } => {
            collect_referenced_usrs_in_expr(target, out);
            collect_referenced_usrs_in_expr(value, out);
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            collect_referenced_usrs_in_expr(condition, out);
            collect_referenced_usrs_in_stmts(then_branch, out);
            collect_referenced_usrs_in_stmts(else_branch, out);
        }
        Stmt::While {
            condition, body, ..
        } => {
            collect_referenced_usrs_in_expr(condition, out);
            collect_referenced_usrs_in_stmts(body, out);
        }
        Stmt::For {
            init,
            condition,
            increment,
            body,
            ..
        } => {
            if let Some(init) = init {
                collect_referenced_usrs_in_stmt(init, out);
            }
            if let Some(condition) = condition {
                collect_referenced_usrs_in_expr(condition, out);
            }
            if let Some(increment) = increment {
                collect_referenced_usrs_in_stmt(increment, out);
            }
            collect_referenced_usrs_in_stmts(body, out);
        }
        Stmt::ExprStmt { expr, .. } => collect_referenced_usrs_in_expr(expr, out),
        Stmt::Throw { value, .. } => collect_referenced_usrs_in_expr(value, out),
        Stmt::TryCatch {
            try_body,
            catch_type,
            catch_body,
            ..
        } => {
            collect_referenced_usrs_in_stmts(try_body, out);
            collect_referenced_usrs_in_type(catch_type, out);
            collect_referenced_usrs_in_stmts(catch_body, out);
        }
        Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            collect_referenced_usrs_in_stmts(try_body, out);
            collect_referenced_usrs_in_stmts(finally_body, out);
        }
        Stmt::TupleAssign { targets, value, .. } => {
            for target in targets {
                collect_referenced_usrs_in_expr(target, out);
            }
            collect_referenced_usrs_in_expr(value, out);
        }
        Stmt::Unsupported { .. } => {}
    }
}

fn collect_referenced_usrs_in_expr<'a>(expr: &'a Expr, out: &mut HashSet<&'a str>) {
    match expr {
        Expr::IntLiteral { .. }
        | Expr::DoubleLiteral { .. }
        | Expr::BoolLiteral { .. }
        | Expr::StringLiteral { .. }
        | Expr::Unsupported { .. } => {}
        Expr::Ref { ty, .. } | Expr::This { ty, .. } => collect_referenced_usrs_in_type(ty, out),
        Expr::Binary { lhs, rhs, ty, .. } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(lhs, out);
            collect_referenced_usrs_in_expr(rhs, out);
        }
        Expr::Unary { operand, ty, .. } | Expr::Convert { operand, ty, .. } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(operand, out);
        }
        Expr::Call {
            target,
            callee_usr,
            args,
            ty,
            ..
        } => {
            out.insert(callee_usr.as_str());
            collect_referenced_usrs_in_type(ty, out);
            if let Some(target) = target {
                collect_referenced_usrs_in_expr(target, out);
            }
            for arg in args {
                collect_referenced_usrs_in_expr(arg, out);
            }
        }
        Expr::FieldAccess { target, ty, .. } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(target, out);
        }
        Expr::RecordConstruct {
            type_usr, fields, ..
        } => {
            out.insert(type_usr.as_str());
            for (_name, value) in fields {
                collect_referenced_usrs_in_expr(value, out);
            }
        }
        Expr::ConstructorCall { type_usr, args, .. } => {
            out.insert(type_usr.as_str());
            for arg in args {
                collect_referenced_usrs_in_expr(arg, out);
            }
        }
        Expr::Index {
            target, index, ty, ..
        } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(target, out);
            collect_referenced_usrs_in_expr(index, out);
        }
        Expr::StringByteLength { target, .. } => collect_referenced_usrs_in_expr(target, out),
        Expr::Tuple { values, .. } => {
            for value in values {
                collect_referenced_usrs_in_expr(value, out);
            }
        }
    }
}

/// `enum Foo { a, b, c }` — caso 4 of
/// `docs/plans/verovio-6.2-pointer-types.md`. No fixture has forced a
/// variant with associated data (C++'s enum has none to carry over
/// either), so this stays the plain, no-argument Dart enum form; the real
/// transpile pipeline reformats every file through `dart format` anyway
/// (`transpile::transpile`), so the exact line-wrapping here isn't load
/// bearing.
///
/// Dart requires an enum to declare at least one constant, so a variantless
/// `ir::Enum` gets an explicit placeholder rather than the `enum Foo { }`
/// that Dart rejects outright. `lower::cpp::lower_enum` already refuses to
/// build one (`enum Vazio {};` is legal C++ but has no Dart form), so this
/// only guards IR that didn't come from the C++ lowering pass — visible in
/// the output if it ever happens, never a file that won't parse.
fn emit_enum(enum_decl: &Enum) -> String {
    if enum_decl.variants.is_empty() {
        return format!(
            "// TODO(syntax-bridge): `{}` declares no constants; Dart has no empty enum.\nenum {} {{ unsupportedEmptyEnum }}\n",
            enum_decl.name, enum_decl.name
        );
    }

    format!(
        "enum {} {{ {} }}\n",
        enum_decl.name,
        enum_decl.variants.join(", ")
    )
}

/// A POD `struct`/`class` (`record.constructors.is_empty()` — no
/// user-declared constructor of its own) becomes a Dart class with every
/// field declared `final`... except E03's own armadilha rules that out:
/// `mover` mutates its (by-value-copied) parameter's fields in place, so
/// fields need to stay mutable. A positional constructor
/// (`Ponto(this.x, this.y);`) doubles as the copy constructor `lower::cpp`
/// needs for by-value parameter semantics (`RecordConstruct` emits a call to
/// this same constructor).
///
/// A record with its own declared constructor(s) (E04) instead emits each
/// one for real (`emit_constructor`, sorted by `constructor_index` — see
/// that field's docs on why sorting, not push order, decides which one is
/// primary), plus every static field and method. The two shapes don't mix on
/// the same record: a class with a hand-written constructor also owns its
/// own field initialization, so the E03 synthetic positional constructor
/// would either be redundant or, worse, a second and inconsistent way to
/// construct the same class.
#[allow(clippy::too_many_arguments)]
fn emit_record(
    record: &Record,
    is_mixin: bool,
    records_by_usr: &HashMap<&str, &Record>,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    enums_by_usr: &HashMap<&str, &Enum>,
    mock: &MockContext<'_>,
) -> String {
    // `abstract` is required the moment a class has any unimplemented
    // member — derived, not stored: a separate `Record.is_abstract` flag
    // could disagree with the method list it's supposed to summarize, so
    // this is the one source of truth for both this keyword and
    // `emit_method`'s own bodyless-signature branch. Meaningless for a
    // `mixin` declaration (Dart's `mixin` keyword has no `abstract` variant
    // — a mixin can't be instantiated at all, so nothing to mark abstract),
    // so skipped entirely for that case.
    let abstract_keyword = if !is_mixin && record.methods.iter().any(|method| method.body.is_none())
    {
        "abstract "
    } else {
        ""
    };
    // Dart forbids both `extends` and `with` on a `mixin` declaration — only
    // `on` (a superclass *constraint*, not composition) is legal there. A
    // record that's itself used as a mixin elsewhere (`is_mixin`) therefore
    // never gets `extends_clause`/`with_clause` below; its own base(s) —
    // whichever field is populated, `base_class` or `mixins`, the two are
    // mutually exclusive (`lower::cpp::base_classes_of`'s doc comment) —
    // become a single `on` clause instead. Composing the actual
    // implementation back together is pushed down to whichever concrete
    // `class` ends up applying the whole chain via `with` (below).
    let bases_clause = if is_mixin {
        let on_bases: Vec<&str> = record
            .base_class
            .iter()
            .chain(record.mixins.iter())
            .map(|base| base.name.as_str())
            .collect();
        if on_bases.is_empty() {
            String::new()
        } else {
            format!(" on {}", on_bases.join(", "))
        }
    } else {
        let extends_clause = match &record.base_class {
            Some(base) => format!(" extends {}", base.name),
            None => String::new(),
        };
        // E09: every base beyond a single `extends` becomes a Dart mixin
        // (`Record::mixins`'s own doc comment on why `mapping::options_for`'s
        // multiple-inheritance decision always shapes out this way) — never
        // combined with `extends_clause` by any fixture yet, but concatenated
        // rather than treated as mutually exclusive, since Dart itself allows
        // `class X extends A with B, C {}`. Expanded transitively
        // (`expand_mixin_chain`) rather than printed as-is: a listed mixin
        // that's itself built from further bases only got an `on` clause
        // above, which is a constraint the *applying* class must satisfy,
        // not composition the mixin does on its own.
        let with_clause = if record.mixins.is_empty() {
            String::new()
        } else {
            format!(
                " with {}",
                expand_mixin_chain(&record.mixins, records_by_usr)
                    .into_iter()
                    .map(|base| base.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        format!("{extends_clause}{with_clause}")
    };
    let keyword = if is_mixin { "mixin" } else { "class" };
    let mut source = format!(
        "{abstract_keyword}{keyword} {}{bases_clause} {{\n",
        record.name
    );
    for field in &record.fields {
        // Dart's flow analysis for non-nullable field initialization only
        // recognizes initializing formals (`this.field`) and initializer
        // lists, not plain assignment inside a constructor body — which is
        // how E04+ constructors set fields (`AGENTS.md`: "não implemente em
        // largura", no need for initializer-list emission yet). A real
        // constructor (`record.constructors` non-empty) therefore needs a
        // zero-value default at declaration so `dart analyze` doesn't flag
        // the field as possibly-unset; the synthetic positional constructor
        // path below already initializes every field itself and must stay
        // byte-identical to its pre-E04 output, so it's left untouched. A
        // `mixin` (E09) always needs the same zero-value default regardless
        // — it can't have *any* constructor (Dart forbids one on a `mixin`
        // declaration), so nothing else would ever initialize its fields.
        if !is_mixin && record.constructors.is_empty() {
            source.push_str(&format!(
                "{INDENT}{} {};\n",
                emit_type(&field.ty),
                field.name
            ));
        } else {
            source.push_str(&emit_field_declaration(
                "",
                &field.ty,
                &field.name,
                enums_by_usr,
            ));
        }
    }
    for field in &record.static_fields {
        source.push_str(&emit_field_declaration(
            "static ",
            &field.ty,
            &field.name,
            enums_by_usr,
        ));
    }

    if is_mixin {
        // A `mixin` declaration can't have a constructor at all (Dart
        // rejects one outright) — every field already got its zero-value
        // default above, which is the only initialization a mixin's own
        // fields ever get.
    } else if record.constructors.is_empty() {
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
                    "\n{INDENT}{}({ctor_params}) {{\n{INDENT}{INDENT}throw UnimplementedError({message});\n{INDENT}}}\n",
                    record.name
                ));
            }
            None => {
                source.push_str(&format!("\n{INDENT}{}({ctor_params});\n", record.name));
            }
        }
    } else {
        let mut constructors: Vec<&Constructor> = record.constructors.iter().collect();
        constructors.sort_by_key(|constructor| constructor.constructor_index);
        for constructor in constructors {
            source.push('\n');
            source.push_str(&emit_constructor(
                &record.name,
                constructor,
                mock,
                used_expr_helper,
                used_utf8_encode,
            ));
        }
    }

    for method in &record.methods {
        source.push('\n');
        source.push_str(&emit_method(
            &record.name,
            method,
            mock,
            used_expr_helper,
            used_utf8_encode,
        ));
    }

    source.push_str("}\n");
    source
}

/// One field declaration, with an initializer when the type has a sound
/// default and `late` when it doesn't.
///
/// Dart's flow analysis needs a non-nullable field to be definitely
/// assigned; `late` is how Dart itself says "assigned before first use,
/// just not here". Reaching for it in the no-sound-default case is what
/// keeps this honest — the alternative this replaced fabricated `0` for
/// *every* type, which for anything but a number is not merely a wrong
/// default but not even valid Dart (`Cor c = 0`, `Ponto p = 0`), so the
/// package stopped compiling. A `late` field that nothing assigns throws
/// `LateInitializationError` naming the field, at the moment it's actually
/// read: a loud, located failure instead of a silent fake zero.
fn emit_field_declaration(
    prefix: &str,
    ty: &Type,
    name: &str,
    enums_by_usr: &HashMap<&str, &Enum>,
) -> String {
    match field_default_literal(ty, enums_by_usr) {
        Some(literal) => format!("{INDENT}{prefix}{} {name} = {literal};\n", emit_type(ty)),
        None => format!("{INDENT}{prefix}late {} {name};\n", emit_type(ty)),
    }
}

/// The Dart literal a field of this type can be default-initialized to, or
/// `None` when the type has no sound default to write.
///
/// Where C++ default-construction has an obvious Dart equivalent, this
/// matches it: `std::string` → `''`, `std::vector` → `[]`, `std::set`/
/// `std::map` → `{}`. `0` covers `int` and `double` alike (numeric literal
/// coercion — the same trick `lower::cpp::default_scalar_value` uses). An
/// enum takes its first constant, which is what C++ value-initialization
/// (`Cor c{}`, zero) selects for the overwhelmingly common enum whose
/// first enumerator is 0 — and the enum must be looked up by `usr` rather
/// than assumed, since an enum declared in another file, or one this
/// module never declared at all, has no first constant to name.
///
/// `Record`/`Tuple`/`Void`/`Unsupported` return `None`: there is no
/// literal for them that would be both valid Dart and honest about the
/// value being absent.
fn field_default_literal(ty: &Type, enums_by_usr: &HashMap<&str, &Enum>) -> Option<String> {
    match ty {
        Type::Bool => Some("false".to_owned()),
        // A nullable field's own honest "no value yet" is `null`, not a
        // fabricated zero of the pointee type.
        Type::Nullable(_) => Some("null".to_owned()),
        Type::Int | Type::Double => Some("0".to_owned()),
        Type::Str => Some("''".to_owned()),
        Type::List(_) => Some("[]".to_owned()),
        // Dart's `{}` is an empty set or an empty map depending on the
        // context type, which is exactly the declared type here.
        Type::Set(_) | Type::Map(_, _) => Some("{}".to_owned()),
        Type::Enum { usr, name } => {
            let first = enums_by_usr.get(usr.as_str())?.variants.first()?;
            Some(format!("{name}.{first}"))
        }
        Type::Record { .. } | Type::Tuple(_) | Type::Void | Type::Unsupported(_) => None,
    }
}

/// What `emit_function`/`emit_method`/`emit_equality_operator`/
/// `emit_constructor` need to render a mock body instead of a real one when
/// the callable's own usr is in the effective external set
/// (`docs/plans/lista-de-externos.md`) — bundled into one struct rather than
/// three loose parameters threaded through every one of those call sites.
/// `enums_by_usr`/`records_by_usr` are the exact same module-wide lookups
/// `emit_record`/`field_default_literal` already build once in
/// `emit_module` — reused here, not recomputed.
struct MockContext<'a> {
    external_usrs: &'a HashSet<&'a str>,
    enums_by_usr: &'a HashMap<&'a str, &'a Enum>,
    records_by_usr: &'a HashMap<&'a str, &'a Record>,
}

impl MockContext<'_> {
    fn is_external(&self, usr: &str) -> bool {
        self.external_usrs.contains(usr)
    }
}

/// Recursion guard for [`mock_value_for_type`]'s `Record` case — generous
/// enough that no real C++ struct nesting depth should ever reach it (a
/// record can't contain itself *by value*, only by pointer/reference, which
/// already maps to `Type::Nullable`/`null` before recursion ever starts), so
/// this only exists as a defensive backstop, never expected to trigger.
const MOCK_VALUE_MAX_DEPTH: usize = 16;

/// Decision 1 (`docs/plans/lista-de-externos.md`): "mock = valor plausível,
/// execução segue" — the value an external callable's mocked body returns.
/// `None` when no plausible value exists for `ty` at all (an `Unsupported`
/// type, or a `Record` this module never lowered) — the one case where a
/// mock still has to fall back to the honest `Unsupported` bailout, because
/// there is no Dart value of that type to construct, mocked or not.
///
/// Every scalar/collection/`Enum` case matches `field_default_literal`
/// exactly (this function exists because that one intentionally stops at
/// `Record`/`Tuple`/`Void`, which a *field* declaration can leave `late`
/// but a mocked function's `return` statement can't). `Record` is resolved
/// by calling the record's own constructor — the synthetic positional one
/// (`emit_record`'s own "no constructors" branch, one arg per field, in
/// field order) when it has none of its own, otherwise its lowest-index
/// (primary) real constructor — with each argument itself built
/// recursively, so a record nesting another record by value still gets a
/// real, instantiable value instead of bottoming out early.
fn mock_value_for_type(
    ty: &Type,
    enums_by_usr: &HashMap<&str, &Enum>,
    records_by_usr: &HashMap<&str, &Record>,
    depth: usize,
) -> Option<String> {
    if let Some(literal) = field_default_literal(ty, enums_by_usr) {
        return Some(literal);
    }

    if depth >= MOCK_VALUE_MAX_DEPTH {
        return None;
    }

    match ty {
        Type::Record { usr, name } => {
            let record = records_by_usr.get(usr.as_str())?;
            let arg_types: Vec<&Type> = if record.constructors.is_empty() {
                record.fields.iter().map(|field| &field.ty).collect()
            } else {
                let mut constructors: Vec<&Constructor> = record.constructors.iter().collect();
                constructors.sort_by_key(|constructor| constructor.constructor_index);
                constructors[0]
                    .params
                    .iter()
                    .map(|param| &param.ty)
                    .collect()
            };
            let args: Option<Vec<String>> = arg_types
                .into_iter()
                .map(|arg_ty| mock_value_for_type(arg_ty, enums_by_usr, records_by_usr, depth + 1))
                .collect();
            Some(format!("{name}({})", args?.join(", ")))
        }
        Type::Tuple(types) => {
            let values: Option<Vec<String>> = types
                .iter()
                .map(|slot_ty| {
                    mock_value_for_type(slot_ty, enums_by_usr, records_by_usr, depth + 1)
                })
                .collect();
            Some(format!("({})", values?.join(", ")))
        }
        Type::Void | Type::Unsupported(_) => None,
        // Every other variant is handled by `field_default_literal` above.
        _ => None,
    }
}

/// The mock body itself (decision 1): `Void` needs no `return` at all; every
/// other representable type returns [`mock_value_for_type`]'s plausible
/// value, with an honest marker comment above it (never a `throw` — that's
/// exactly the `Unsupported` idiom this path exists to *not* use). Only
/// falls back to the real `Stmt::Unsupported` bailout when `ty` itself has
/// no constructible Dart value at all — a pre-existing gap in what the
/// product can represent, not something mocking could fix either way.
fn emit_mock_body(
    return_type: &Type,
    origin: &Origin,
    mock: &MockContext<'_>,
    depth: usize,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    let pad = INDENT.repeat(depth);
    if matches!(return_type, Type::Void) {
        return format!("{pad}// syntax-bridge: externo, corpo mockado\n");
    }

    match mock_value_for_type(return_type, mock.enums_by_usr, mock.records_by_usr, 0) {
        Some(value) => {
            format!("{pad}// syntax-bridge: externo, corpo mockado\n{pad}return {value};\n")
        }
        None => {
            let bailout = Stmt::Unsupported {
                reason: format!(
                    "externo, mas o tipo de retorno {} não tem valor plausível para mock",
                    emit_type(return_type)
                ),
                origin: origin.clone(),
            };
            emit_stmt(&bailout, depth, used_expr_helper, used_utf8_encode)
        }
    }
}

fn emit_constructor(
    record_name: &str,
    constructor: &Constructor,
    mock: &MockContext<'_>,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    let dart_name = dart_constructor_name(record_name, constructor.constructor_index);
    let params = format_params(&constructor.params, used_expr_helper, used_utf8_encode);
    // A constructor has no return type to mock a value for — every field
    // already got a sound default at its own declaration
    // (`emit_field_declaration`, always consulted once a record has any
    // real constructor at all), so an external constructor's mock body is
    // simply empty: the object still comes out fully, validly initialized.
    let body = if mock.is_external(&constructor.usr) {
        format!(
            "{}// syntax-bridge: externo, corpo mockado\n",
            INDENT.repeat(2)
        )
    } else {
        emit_body(
            &constructor.params,
            None,
            &constructor.body,
            &constructor.origin,
            used_expr_helper,
            used_utf8_encode,
            2,
        )
    };
    format!("{INDENT}{dart_name}({params}) {{\n{body}{INDENT}}}\n")
}

/// `constructor_index == 0` is always the primary, unnamed constructor;
/// every other index is a named constructor `ClassName.ctorN`, `N` being
/// `constructor_index + 1` — E04's armadilha, since Dart has no
/// signature-based constructor overloading and a C++ constructor has no
/// name of its own to reuse for the rest (see `Expr::ConstructorCall`'s
/// docs and `examples/E04-classe-com-encapsulamento/NOTES.md`).
fn dart_constructor_name(record_name: &str, constructor_index: usize) -> String {
    if constructor_index == 0 {
        record_name.to_owned()
    } else {
        format!("{record_name}.ctor{}", constructor_index + 1)
    }
}

fn emit_method(
    record_name: &str,
    method: &Method,
    mock: &MockContext<'_>,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    // Dart requires `operator ==` to override `Object.==` exactly —
    // `bool operator ==(Object other)`, never the receiver's own C++
    // parameter type (`dart analyze`: `invalid_override`, confirmed
    // empirically — E13's sixth finding). `lower::cpp` still lowers the
    // body faithfully, comparing against a same-type `other` (e.g.
    // `other._m_denominator`); wrapping it in an `is`-check both satisfies
    // the signature and lets Dart's own flow analysis promote `other` back
    // to `record_name` for the wrapped body, unchanged.
    if method.name == "operator==" {
        return emit_equality_operator(
            record_name,
            method,
            mock,
            used_expr_helper,
            used_utf8_encode,
        );
    }

    // Every other C++ operator overload printed under its own literal name
    // is invalid Dart syntax — `bool operator()(...)`, `bool operator<(...)`
    // (there's no bare `<name>` identifier in Dart at all; `dart format`
    // rejects it, confirmed empirically on Verovio 6.2.0's real `accid.cpp`,
    // the diagnostic finding that drove this fix). Three cases, in order of
    // how directly Dart can represent them:
    if method.name == "operator()" {
        // Dart's own callable-object idiom: a plain method named `call`
        // makes `obj(args)` dispatch to it automatically — this bridge
        // preserves the call-site syntax too, not just the declaration.
        return emit_method_named("call", method, mock, used_expr_helper, used_utf8_encode);
    }
    if let Some(symbol) = direct_dart_operator_symbol(&method.name, method.params.len()) {
        // Same arity, same meaning, Dart's own `operator <symbol>` syntax —
        // no special body handling needed, unlike `==`.
        return emit_method_named(
            &format!("operator {symbol}"),
            method,
            mock,
            used_expr_helper,
            used_utf8_encode,
        );
    }
    if method.name.starts_with("operator") {
        // No Dart equivalent exists (`operator<<`, `operator++`, a
        // conversion operator, ...). Declaring it under a synthesized,
        // always-valid name keeps the file parseable; the body still bails
        // out loudly instead of pretending the translation succeeded
        // ("silêncio é proibido"). Deliberately **not** consulting `mock`
        // here even if this usr is externally marked: the operator's own
        // *name* has no Dart equivalent regardless of whose body fills it,
        // so a bailout is the honest answer either way — a rare enough
        // combination (external *and* an unrepresentable operator) not to
        // be worth a second mock path for.
        return emit_unsupported_operator(method, used_expr_helper, used_utf8_encode);
    }

    emit_method_named(
        &method.name,
        method,
        mock,
        used_expr_helper,
        used_utf8_encode,
    )
}

/// The generic method-emission shape every ordinary method, and every
/// operator bridge (`call`, `operator <symbol>`), share — `dart_name` is
/// whatever `emit_method` decided to print instead of `method.name`.
fn emit_method_named(
    dart_name: &str,
    method: &Method,
    mock: &MockContext<'_>,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    let params = format_params(&method.params, used_expr_helper, used_utf8_encode);
    let override_prefix = if method.is_override {
        format!("{INDENT}@override\n")
    } else {
        String::new()
    };
    let return_type = emit_type(&method.return_type);

    // A pure virtual method (`body: None`, E06's abstract-method case) has
    // no implementation to print — Dart's own abstract-member syntax is a
    // signature with no body at all, not empty braces (`{}` would mean "does
    // nothing", not "not implemented"). A pure virtual method is never
    // itself externally marked in practice (nothing calls
    // `FunctionDeclarationKind::Method` cataloging for a body-less cursor
    // except the pure-virtual carve-out, whose `has_definition` is still
    // `true` — see that field's doc comment), so `mock` is irrelevant here.
    let Some(body_stmts) = &method.body else {
        return format!("{override_prefix}{INDENT}{return_type} {dart_name}({params});\n");
    };

    let body = if mock.is_external(&method.usr) {
        emit_mock_body(
            &method.return_type,
            &method.origin,
            mock,
            2,
            used_expr_helper,
            used_utf8_encode,
        )
    } else {
        emit_body(
            &method.params,
            Some(&method.return_type),
            body_stmts,
            &method.origin,
            used_expr_helper,
            used_utf8_encode,
            2,
        )
    };
    let static_keyword = if method.is_static { "static " } else { "" };
    format!(
        "{override_prefix}{INDENT}{static_keyword}{return_type} {dart_name}({params}) {{\n{body}{INDENT}}}\n"
    )
}

/// Dart's fixed set of user-overloadable operators that also have a C++
/// operator of the same spelling and the same arity — `unary-`'s arity (0
/// params) disambiguates it from binary `-` (1 param) exactly the way C++
/// itself does, so no separate "unary" spelling is needed here, just the
/// arity check.
const DIRECT_DART_OPERATOR_ARITIES: &[(&str, &[usize])] = &[
    ("+", &[1]),
    ("-", &[0, 1]),
    ("*", &[1]),
    ("/", &[1]),
    ("<", &[1]),
    ("<=", &[1]),
    (">", &[1]),
    (">=", &[1]),
    ("[]", &[1]),
    ("[]=", &[2]),
];

fn direct_dart_operator_symbol(method_name: &str, arity: usize) -> Option<&'static str> {
    let symbol = method_name.strip_prefix("operator")?;
    DIRECT_DART_OPERATOR_ARITIES
        .iter()
        .find(|(candidate, arities)| *candidate == symbol && arities.contains(&arity))
        .map(|(candidate, _)| *candidate)
}

/// A C++ operator overload with no Dart equivalent at all — `operator<<`,
/// `operator++`/`--` (Dart never lets a type customize those), a compound
/// assignment (`operator+=`), a conversion operator, or anything else not in
/// `DIRECT_DART_OPERATOR_ARITIES`. The declaration still needs a valid Dart
/// identifier in its place (a small, fixed, C++-wide vocabulary — not a
/// per-project or per-fixture table), and the body bails out with the same
/// `Stmt::Unsupported` rendering every other unrepresentable construct uses,
/// instead of either guessing a translation or dropping the member silently.
fn emit_unsupported_operator(
    method: &Method,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    let bridge_name = bridge_name_for_unsupported_operator(&method.name, method.params.len());
    let params = format_params(&method.params, used_expr_helper, used_utf8_encode);
    let return_type = emit_type(&method.return_type);

    if method.body.is_none() {
        return format!("{INDENT}{return_type} {bridge_name}({params});\n");
    }

    let bailout = Stmt::Unsupported {
        reason: format!(
            "`{}` has no Dart operator equivalent; bridged to a named method (`{bridge_name}`)",
            method.name
        ),
        origin: method.origin.clone(),
    };
    let body = emit_stmt(&bailout, 2, used_expr_helper, used_utf8_encode);
    format!("{INDENT}{return_type} {bridge_name}({params}) {{\n{body}{INDENT}}}\n")
}

fn bridge_name_for_unsupported_operator(method_name: &str, arity: usize) -> &'static str {
    let symbol = method_name.strip_prefix("operator").unwrap_or(method_name);
    match symbol {
        "<<" => "streamInsert",
        ">>" => "streamExtract",
        "->" => "arrow",
        "!" => "logicalNot",
        "~" => "bitwiseNot",
        "++" if arity == 0 => "increment",
        "++" => "incrementPostfix",
        "--" if arity == 0 => "decrement",
        "--" => "decrementPostfix",
        "+=" => "addAssign",
        "-=" => "subtractAssign",
        "*=" => "multiplyAssign",
        "/=" => "divideAssign",
        "%=" => "moduloAssign",
        "%" => "modulo",
        "&" => "bitwiseAnd",
        "|" => "bitwiseOr",
        "^" => "bitwiseXor",
        "&&" => "logicalAnd",
        "||" => "logicalOr",
        "," => "comma",
        _ => "unsupportedOperator",
    }
}

/// An operator== member always has exactly one parameter in valid C++ — see
/// `emit_method`'s own doc comment on why this needs different emission
/// from every other method.
fn emit_equality_operator(
    record_name: &str,
    method: &Method,
    mock: &MockContext<'_>,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    let override_prefix = if method.is_override {
        format!("{INDENT}@override\n")
    } else {
        String::new()
    };
    let Some(body_stmts) = &method.body else {
        return format!("{override_prefix}{INDENT}bool operator ==(Object other);\n");
    };
    let other_name = method
        .params
        .first()
        .map(|param| param.name.as_str())
        .expect("operator== always has exactly one parameter in valid C++");

    // `bool`, not `method.return_type`: the printed signature above is
    // always `bool operator ==`, regardless of what C++ declared — same
    // reasoning `emit_mock_body`'s caller elsewhere always mocks the
    // *printed* return type, not a value that wouldn't match the
    // signature's own declared type.
    let inner_body = if mock.is_external(&method.usr) {
        emit_mock_body(
            &Type::Bool,
            &method.origin,
            mock,
            3,
            used_expr_helper,
            used_utf8_encode,
        )
    } else {
        emit_body(
            &method.params,
            Some(&method.return_type),
            body_stmts,
            &method.origin,
            used_expr_helper,
            used_utf8_encode,
            3,
        )
    };
    format!(
        "{override_prefix}{INDENT}bool operator ==(Object {other_name}) {{\n\
         {INDENT}{INDENT}if ({other_name} is {record_name}) {{\n\
         {inner_body}\
         {INDENT}{INDENT}}}\n\
         {INDENT}{INDENT}return false;\n\
         {INDENT}}}\n"
    )
}

/// Every parameter with a `default_value` (E07's C++ default arguments)
/// becomes a trailing Dart *optional positional* parameter (`[T name =
/// value]`) — not named (`{T name = value}`), which would force every call
/// site to name the argument explicitly; C++'s own call sites never do, and
/// positional keeps the call syntax unchanged. Safe to partition this way
/// without checking that defaulted parameters are already trailing: C++
/// itself requires that (a parameter with a default can't precede one
/// without), so `lower::cpp` never produces an IR parameter list where
/// they aren't.
fn format_params(
    params: &[Param],
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    let mut parts: Vec<String> = params
        .iter()
        .filter(|param| param.default_value.is_none())
        .map(|param| format!("{} {}", emit_type(&param.ty), param.name))
        .collect();

    let optional: Vec<&Param> = params
        .iter()
        .filter(|param| param.default_value.is_some())
        .collect();
    if !optional.is_empty() {
        let optional_text = optional
            .iter()
            .map(|param| {
                format!(
                    "{} {} = {}",
                    emit_type(&param.ty),
                    param.name,
                    emit_expr(
                        param
                            .default_value
                            .as_ref()
                            .expect("filtered by is_some above"),
                        used_expr_helper,
                        used_utf8_encode,
                    )
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("[{optional_text}]"));
    }

    parts.join(", ")
}

/// `record.fields`' own unsupported check, extended to `static_fields`: a
/// static field with a type this IR can't represent is exactly as much of an
/// incomplete shape as an instance field would be, and bails the class out
/// the same way (see `emit_record`).
fn first_unsupported_field_reason(record: &Record) -> Option<String> {
    record
        .fields
        .iter()
        .chain(&record.static_fields)
        .find_map(|field| match &field.ty {
            Type::Unsupported(spelling) => Some(format!(
                "unsupported field type: {spelling} (field `{}`)",
                field.name
            )),
            _ => None,
        })
}

fn emit_function(
    function: &Function,
    mock: &MockContext<'_>,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    let params = format_params(&function.params, used_expr_helper, used_utf8_encode);
    let body = if mock.is_external(&function.usr) {
        emit_mock_body(
            &function.return_type,
            &function.origin,
            mock,
            1,
            used_expr_helper,
            used_utf8_encode,
        )
    } else {
        emit_body(
            &function.params,
            Some(&function.return_type),
            &function.body,
            &function.origin,
            used_expr_helper,
            used_utf8_encode,
            1,
        )
    };

    // Dart has no free-standing operators at all — every operator is an
    // instance method — so a *free* C++ operator function (the conventional
    // home for a class's `operator<<` stream-insertion overload) can never
    // become a real Dart `operator` declaration the way a method sometimes
    // can (`emit_method`'s `direct_dart_operator_symbol`). Its body is
    // ordinary, translatable code, though — only the name is the problem —
    // so, unlike the method-side bridge, this is a plain rename, not a body
    // bailout: the same small, C++-wide symbol table
    // (`bridge_name_for_unsupported_operator`), not a per-project one.
    let name = if function.name.starts_with("operator") {
        bridge_name_for_unsupported_operator(&function.name, function.params.len())
    } else {
        &function.name
    };

    format!(
        "{return_type} {name}({params}) {{\n{body}}}\n",
        return_type = emit_type(&function.return_type),
    )
}

/// Shared by `emit_function`/`emit_method`/`emit_constructor`: computes the
/// same bail-out-or-emit-normally body a free function always has, at
/// whichever indentation `depth` the caller's own body sits at (`1` for a
/// top-level function, `2` for a method or constructor nested inside a
/// class). `return_type` is `None` for a constructor, which has no return
/// type of its own to check.
///
/// E03's armadilha (`docs/plans/primeiro-corte-e01-e03.md` §7 PR5, see
/// `examples/E03-struct-pod/NOTES.md`): C++ copies a by-value `struct`
/// parameter; Dart passes the reference. `lower::cpp` already inserts an
/// explicit self-reassignment (`p = Ponto(p.x, p.y);`) as the first
/// statement of the body for every such parameter — nothing special is
/// needed here, the clone is just an ordinary `Stmt::Assign` by the time it
/// reaches the emitter. Kept as a comment here (not code) because the
/// interesting decision lives in the lowering step, and duplicating the
/// reasoning risks the two drifting apart.
///
/// A parameter or return type the IR can't represent is emitted as
/// `dynamic` — if the body then ran as normal Dart on top of that, it would
/// silently compute on untyped values (e.g. arithmetic on a `long`
/// parameter) instead of ever signaling the translation is incomplete.
/// Checked before the body itself: a signature-level failure takes priority
/// and makes the body's own contents irrelevant.
///
/// A *body-local* `Type::Unsupported` (a local variable's declared type, or
/// an expression's own inferred type — e.g. `int / long` promoting to `long`
/// under C++'s usual arithmetic conversions) needs the same treatment:
/// `emit_binary_op`'s truncating-division rule, for one, reads exactly that
/// type to decide `/` vs `~/`, and a type it doesn't recognize means that
/// decision can't be trusted either way.
fn emit_body(
    params: &[Param],
    return_type: Option<&Type>,
    body: &[Stmt],
    origin: &Origin,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    depth: usize,
) -> String {
    let bailout_reason = return_type
        .and_then(unsupported_spelling)
        .map(|spelling| format!("unsupported return type: {spelling}"))
        .or_else(|| first_unsupported_signature_param(params))
        .or_else(|| {
            first_unsupported_type_in_list(body)
                .map(|spelling| format!("unsupported type in expression: {spelling}"))
        });
    let signature_bailout = bailout_reason.map(|reason| Stmt::Unsupported {
        reason,
        origin: origin.clone(),
    });

    match signature_bailout
        .as_ref()
        .or_else(|| first_unsupported_in_list(body))
    {
        // A statement the IR can't represent may have declared a variable
        // (or otherwise established state) that a *later* statement in this
        // same body depends on — emitting only that one statement as a throw
        // and the rest as normal Dart would reference names that were never
        // declared, which is exactly the "compiles and is wrong" failure
        // mode §5's "silêncio é proibido" rule exists to prevent (confirmed
        // empirically: `dart analyze` reports `undefined_identifier` for
        // this, not just a stray warning). So the whole body bails out
        // instead of just the one statement — same shape as a single
        // `Stmt::Unsupported`, using the first one's reason/origin. Searched
        // recursively (nested inside `if`/`while`/`for` bodies too): a
        // conservative rule, not a scope analysis, but a simple one.
        Some(unsupported) => emit_stmt(unsupported, depth, used_expr_helper, used_utf8_encode),
        None => {
            let mut text = String::new();
            for stmt in body {
                text.push_str(&emit_stmt(stmt, depth, used_expr_helper, used_utf8_encode));
            }
            text
        }
    }
}

fn first_unsupported_signature_param(params: &[Param]) -> Option<String> {
    params.iter().find_map(|param| match &param.ty {
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
        Stmt::Throw { value, .. } => expr_unsupported_type_spelling(value),
        Stmt::TryCatch {
            try_body,
            catch_type,
            catch_body,
            ..
        } => unsupported_spelling(catch_type)
            .or_else(|| first_unsupported_type_in_list(try_body))
            .or_else(|| first_unsupported_type_in_list(catch_body)),
        Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => first_unsupported_type_in_list(try_body)
            .or_else(|| first_unsupported_type_in_list(finally_body)),
        Stmt::TupleAssign { targets, value, .. } => targets
            .iter()
            .find_map(expr_unsupported_type_spelling)
            .or_else(|| expr_unsupported_type_spelling(value)),
        Stmt::Unsupported { .. } => None,
    }
}

fn expr_unsupported_type_spelling(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::IntLiteral { .. }
        | Expr::DoubleLiteral { .. }
        | Expr::BoolLiteral { .. }
        | Expr::StringLiteral { .. }
        | Expr::Unsupported { .. } => None,
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
        Expr::Call {
            ty, target, args, ..
        } => unsupported_spelling(ty)
            .or_else(|| target.as_deref().and_then(expr_unsupported_type_spelling))
            .or_else(|| args.iter().find_map(expr_unsupported_type_spelling)),
        Expr::FieldAccess { ty, target, .. } => {
            unsupported_spelling(ty).or_else(|| expr_unsupported_type_spelling(target))
        }
        Expr::RecordConstruct { fields, .. } => fields
            .iter()
            .find_map(|(_name, value)| expr_unsupported_type_spelling(value)),
        // A `ConstructorCall`'s own type is always its (already-checked)
        // owning record's type, never itself `Unsupported` — only its
        // arguments can be. `This` carries a placeholder `Void` type (see
        // its doc comment) that's never meant to be checked here.
        Expr::ConstructorCall { args, .. } => args.iter().find_map(expr_unsupported_type_spelling),
        Expr::This { .. } => None,
        Expr::Index {
            target, index, ty, ..
        } => unsupported_spelling(ty)
            .or_else(|| expr_unsupported_type_spelling(target))
            .or_else(|| expr_unsupported_type_spelling(index)),
        Expr::StringByteLength { target, .. } => expr_unsupported_type_spelling(target),
        Expr::Tuple { values, .. } => values.iter().find_map(expr_unsupported_type_spelling),
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
        Stmt::TryCatch {
            try_body,
            catch_body,
            ..
        } => first_unsupported_in_list(try_body).or_else(|| first_unsupported_in_list(catch_body)),
        Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            first_unsupported_in_list(try_body).or_else(|| first_unsupported_in_list(finally_body))
        }
        Stmt::Return { .. }
        | Stmt::VarDecl { .. }
        | Stmt::Assign { .. }
        | Stmt::FieldAssign { .. }
        | Stmt::ExprStmt { .. }
        | Stmt::TupleAssign { .. }
        | Stmt::Throw { .. } => None,
    }
}

fn emit_type(ty: &Type) -> String {
    match ty {
        Type::Int => "int".to_owned(),
        Type::Bool => "bool".to_owned(),
        Type::Double => "double".to_owned(),
        Type::Void => "void".to_owned(),
        Type::Record { name, .. } | Type::Enum { name, .. } => name.clone(),
        Type::Str => "String".to_owned(),
        Type::List(element) => format!("List<{}>", emit_type(element)),
        Type::Set(element) => format!("Set<{}>", emit_type(element)),
        Type::Map(key, value) => format!("Map<{}, {}>", emit_type(key), emit_type(value)),
        // A single-element Dart record needs a trailing comma
        // (`(int,)`) to disambiguate it from a plain parenthesized
        // expression — `lower::cpp`'s out-param bridge (`Type::Tuple`)
        // can produce exactly one element when a function has only one
        // by-reference out-param.
        Type::Tuple(elements) if elements.len() == 1 => {
            format!("({},)", emit_type(&elements[0]))
        }
        Type::Tuple(elements) => format!(
            "({})",
            elements
                .iter()
                .map(emit_type)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Nullable(inner) => format!("{}?", emit_type(inner)),
        Type::Unsupported(spelling) => format!("dynamic /* unsupported: {spelling} */"),
    }
}

fn emit_stmt(
    stmt: &Stmt,
    depth: usize,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    let pad = INDENT.repeat(depth);
    match stmt {
        Stmt::Return { value, .. } => match value {
            Some(expr) => format!(
                "{pad}return {};\n",
                emit_expr(expr, used_expr_helper, used_utf8_encode)
            ),
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
                emit_expr(expr, used_expr_helper, used_utf8_encode)
            ),
            None => format!("{pad}late {} {name};\n", emit_type(ty)),
        },
        Stmt::Assign { name, value, .. } => {
            format!(
                "{pad}{name} = {};\n",
                emit_expr(value, used_expr_helper, used_utf8_encode)
            )
        }
        Stmt::FieldAssign {
            target,
            field,
            value,
            ..
        } => match target {
            // Bare `field = value;` for an implicit `this.field = value;` —
            // same omission `emit_expr`'s `FieldAccess` arm applies for a
            // read (see its comment).
            Expr::This { .. } => {
                format!(
                    "{pad}{field} = {};\n",
                    emit_expr(value, used_expr_helper, used_utf8_encode)
                )
            }
            _ => format!(
                "{pad}{}{}.{field} = {};\n",
                emit_expr(target, used_expr_helper, used_utf8_encode),
                receiver_bang(target),
                emit_expr(value, used_expr_helper, used_utf8_encode)
            ),
        },
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            let mut source = format!(
                "{pad}if ({}) {{\n",
                emit_expr(condition, used_expr_helper, used_utf8_encode)
            );
            for inner in then_branch {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                ));
            }
            if else_branch.is_empty() {
                source.push_str(&format!("{pad}}}\n"));
            } else {
                source.push_str(&format!("{pad}}} else {{\n"));
                for inner in else_branch {
                    source.push_str(&emit_stmt(
                        inner,
                        depth + 1,
                        used_expr_helper,
                        used_utf8_encode,
                    ));
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
                emit_expr(condition, used_expr_helper, used_utf8_encode)
            );
            for inner in body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                ));
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
                .map(|stmt| emit_for_clause(stmt, used_expr_helper, used_utf8_encode))
                .unwrap_or_default();
            let condition_text = condition
                .as_ref()
                .map(|expr| emit_expr(expr, used_expr_helper, used_utf8_encode))
                .unwrap_or_default();
            let increment_text = increment
                .as_deref()
                .map(|stmt| emit_for_clause(stmt, used_expr_helper, used_utf8_encode))
                .unwrap_or_default();
            let mut source =
                format!("{pad}for ({init_text}; {condition_text}; {increment_text}) {{\n");
            for inner in body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                ));
            }
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::ExprStmt { expr, .. } => format!(
            "{pad}{};\n",
            emit_expr(expr, used_expr_helper, used_utf8_encode)
        ),
        Stmt::Throw { value, .. } => format!(
            "{pad}throw {};\n",
            emit_expr(value, used_expr_helper, used_utf8_encode)
        ),
        Stmt::TryCatch {
            try_body,
            catch_type,
            catch_var,
            catch_body,
            ..
        } => {
            let mut source = format!("{pad}try {{\n");
            for inner in try_body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                ));
            }
            source.push_str(&format!(
                "{pad}}} on {} catch ({catch_var}) {{\n",
                emit_type(catch_type)
            ));
            for inner in catch_body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                ));
            }
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            let mut source = format!("{pad}try {{\n");
            for inner in try_body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                ));
            }
            source.push_str(&format!("{pad}}} finally {{\n"));
            for inner in finally_body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                ));
            }
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        // Item 9 of `docs/plans/diagnostico-verovio-6.2.0.md` (real repro:
        // Verovio's `Fraction::ReduceStatic` called with a nullable-pointer
        // field as an out-param argument): a target reached through a
        // nullable receiver needs `receiver!.field` (achado 5's own
        // null-safety fix, `receiver_bang`) — but Dart's pattern-assignment
        // grammar doesn't accept a postfix `!` inside a pattern element
        // (`dart format`: "Expected to find ')'" right after the `!`,
        // confirmed empirically). Ordinary (non-pattern) assignment has no
        // such restriction, so a target needing a bang routes around the
        // pattern grammar entirely: a bare block scopes a temporary holding
        // the call's result, then each target is assigned individually with
        // ordinary assignment syntax. The block (not just consecutive
        // statements) keeps the temporary's name from ever colliding with
        // another local — including a second bridged call in the same
        // function — without needing a counter to keep every temporary
        // name unique.
        Stmt::TupleAssign { targets, value, .. } if tuple_assign_needs_temp_block(targets) => {
            let mut source = format!("{pad}{{\n");
            source.push_str(&format!(
                "{pad}{INDENT}final {TUPLE_ASSIGN_TEMP} = {};\n",
                emit_expr(value, used_expr_helper, used_utf8_encode)
            ));
            for (index, target) in targets.iter().enumerate() {
                source.push_str(&format!(
                    "{pad}{INDENT}{} = {TUPLE_ASSIGN_TEMP}.${};\n",
                    emit_expr(target, used_expr_helper, used_utf8_encode),
                    index + 1
                ));
            }
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::TupleAssign { targets, value, .. } => {
            // A single-element Dart record pattern needs a trailing comma
            // (`(a,) = expr;`) — see `emit_type`'s own `Type::Tuple` arm for
            // why: without it, `(a)` parses as a parenthesized expression,
            // not a destructuring assignment target.
            let targets_text = if targets.len() == 1 {
                format!(
                    "{},",
                    emit_expr(&targets[0], used_expr_helper, used_utf8_encode)
                )
            } else {
                targets
                    .iter()
                    .map(|target| emit_expr(target, used_expr_helper, used_utf8_encode))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            format!(
                "{pad}({targets_text}) = {};\n",
                emit_expr(value, used_expr_helper, used_utf8_encode)
            )
        }
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
fn emit_for_clause(
    stmt: &Stmt,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
) -> String {
    match stmt {
        Stmt::VarDecl {
            name,
            ty,
            init: Some(expr),
            ..
        } => format!(
            "{} {name} = {}",
            emit_type(ty),
            emit_expr(expr, used_expr_helper, used_utf8_encode)
        ),
        Stmt::VarDecl {
            name,
            ty,
            init: None,
            ..
        } => format!("late {} {name}", emit_type(ty)),
        Stmt::Assign { name, value, .. } => {
            format!(
                "{name} = {}",
                emit_expr(value, used_expr_helper, used_utf8_encode)
            )
        }
        Stmt::ExprStmt { expr, .. } => emit_expr(expr, used_expr_helper, used_utf8_encode),
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

/// `expr`'s own static type, where it carries one directly — `None` for a
/// shape whose type is implied elsewhere (a literal, a construct/call
/// naming its own type by `type_name`, `Tuple`, `Unsupported`) rather than
/// stored as an `ir::Type` on the node. Used by `receiver_bang` to decide
/// whether a receiver needs Dart's `!` — every one of those "no type"
/// shapes is also never `Type::Nullable` (a construct call always yields a
/// real value, a literal is never a pointer), so `None` is exactly "not
/// nullable, no `!` needed" for that purpose without needing its own case.
fn expr_ty(expr: &Expr) -> Option<&Type> {
    match expr {
        Expr::Ref { ty, .. }
        | Expr::Binary { ty, .. }
        | Expr::Unary { ty, .. }
        | Expr::Convert { ty, .. }
        | Expr::Call { ty, .. }
        | Expr::FieldAccess { ty, .. }
        | Expr::This { ty, .. }
        | Expr::Index { ty, .. } => Some(ty),
        Expr::IntLiteral { .. }
        | Expr::DoubleLiteral { .. }
        | Expr::BoolLiteral { .. }
        | Expr::StringLiteral { .. }
        | Expr::RecordConstruct { .. }
        | Expr::ConstructorCall { .. }
        | Expr::StringByteLength { .. }
        | Expr::Tuple { .. }
        | Expr::Unsupported { .. } => None,
    }
}

/// `"!"` when `receiver`'s own static type is `Type::Nullable` (E10/E13's
/// pointer solver, `mapping::pointer_options_for` case A10 —
/// `docs/mapping-solver-cases.md`), `""` otherwise. C++ itself never
/// requires (or even offers) a null check to dereference a pointer — `p->x`
/// compiles whether or not `p` was actually null, undefined behavior at
/// worst — so a lowered `T*` field/call/index receiver is asserted
/// non-null here rather than propagating Dart's null-safety requirement
/// into a check `lower::cpp` has no C++ source construct to derive: the
/// same "trust the source, surface a real crash instead of silently
/// corrupting state" trade-off C++ itself already made for every one of
/// these call sites.
/// The temporary local `Stmt::TupleAssign`'s block-scoped fallback
/// (`tuple_assign_needs_temp_block`) declares — a bare block statement is
/// its own lexical scope in Dart, so this fixed name never collides with a
/// same-named local outside it, or with another bridged call's own block
/// elsewhere in the same function.
const TUPLE_ASSIGN_TEMP: &str = "_syntaxBridgeTupleAssign";

/// Whether `Stmt::TupleAssign`'s ordinary record-pattern syntax
/// (`(targets...) = value;`) is unusable for this `targets` list — true
/// when any target is reached through a nullable receiver
/// (`FieldAccess`/`Index`, the only two `Expr` shapes with a receiver
/// `receiver_bang` can apply `!` to) and so would need that `!` printed
/// *inside* a pattern element, which Dart's pattern-assignment grammar
/// rejects (`dart format`: "Expected to find ')'" right at the `!`,
/// confirmed empirically against a real Verovio file — see this variant's
/// own doc comment on `emit_stmt`'s `Stmt::TupleAssign` arm).
fn tuple_assign_needs_temp_block(targets: &[Expr]) -> bool {
    targets.iter().any(|target| match target {
        Expr::FieldAccess {
            target: receiver, ..
        }
        | Expr::Index {
            target: receiver, ..
        } => !receiver_bang(receiver).is_empty(),
        _ => false,
    })
}

fn receiver_bang(receiver: &Expr) -> &'static str {
    if matches!(expr_ty(receiver), Some(Type::Nullable(_))) {
        "!"
    } else {
        ""
    }
}

fn emit_expr(expr: &Expr, used_expr_helper: &mut bool, used_utf8_encode: &mut bool) -> String {
    match expr {
        Expr::IntLiteral { value, .. } => value.to_string(),
        Expr::DoubleLiteral { value, .. } => value.to_string(),
        Expr::BoolLiteral { value, .. } => value.to_string(),
        Expr::StringLiteral { value, .. } => dart_string_literal(value),
        Expr::Ref { name, .. } => name.clone(),
        Expr::Binary {
            op, lhs, rhs, ty, ..
        } => format!(
            "{} {} {}",
            emit_expr(lhs, used_expr_helper, used_utf8_encode),
            emit_binary_op(*op, ty),
            emit_expr(rhs, used_expr_helper, used_utf8_encode)
        ),
        Expr::Unary { op, operand, .. } => {
            let op_text = emit_unary_op(*op);
            let operand_text = emit_expr(operand, used_expr_helper, used_utf8_encode);
            // A nested unary minus (Verovio's `-VRV_UNSET`, a macro expanding
            // to `(-2147483647)`) would otherwise print as `--2147483647` —
            // two adjacent `-` characters with nothing between them merge
            // into Dart's prefix-decrement token, which can't apply to a
            // literal (`dart format`: "Missing selector", confirmed
            // empirically). Parenthesizing whenever the operand's own text
            // starts with the same character keeps the two tokens apart
            // regardless of how deep the nesting goes.
            if operand_text.starts_with(op_text) {
                format!("{op_text}({operand_text})")
            } else {
                format!("{op_text}{operand_text}")
            }
        }
        // The only promotion `lower::cpp` currently constructs a `Convert`
        // node for is int → double (see the IR's own doc comment) — Dart
        // never implicitly widens an `int` expression to `double`.
        Expr::Convert { operand, .. } => {
            format!(
                "{}.toDouble()",
                emit_expr(operand, used_expr_helper, used_utf8_encode)
            )
        }
        Expr::Call {
            target,
            callee_name,
            args,
            ..
        } => {
            let args_text = args
                .iter()
                .map(|arg| emit_expr(arg, used_expr_helper, used_utf8_encode))
                .collect::<Vec<_>>()
                .join(", ");
            // `None` (a free function) and `Some(This)` (a method called on
            // an implicit receiver, from inside another method) both emit
            // with no receiver at all — Dart, like C++, never requires
            // `this.` to reach a class's own members. Only a call on some
            // other, explicit object prints its receiver.
            match target.as_deref() {
                None | Some(Expr::This { .. }) => format!("{callee_name}({args_text})"),
                Some(receiver) => format!(
                    "{}{}.{callee_name}({args_text})",
                    emit_expr(receiver, used_expr_helper, used_utf8_encode),
                    receiver_bang(receiver)
                ),
            }
        }
        Expr::FieldAccess { target, field, .. } => match target.as_ref() {
            Expr::This { .. } => field.clone(),
            _ => format!(
                "{}{}.{field}",
                emit_expr(target, used_expr_helper, used_utf8_encode),
                receiver_bang(target)
            ),
        },
        Expr::RecordConstruct {
            type_name, fields, ..
        } => {
            let args_text = fields
                .iter()
                .map(|(_name, value)| emit_expr(value, used_expr_helper, used_utf8_encode))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{type_name}({args_text})")
        }
        Expr::ConstructorCall {
            type_name,
            constructor_index,
            args,
            ..
        } => {
            let args_text = args
                .iter()
                .map(|arg| emit_expr(arg, used_expr_helper, used_utf8_encode))
                .collect::<Vec<_>>()
                .join(", ");
            let dart_name = dart_constructor_name(type_name, *constructor_index);
            format!("{dart_name}({args_text})")
        }
        // Only ever reached if `This` somehow ends up as its own top-level
        // expression rather than (as `lower::cpp` always produces it) the
        // target of a `FieldAccess`/`Call` — both of which already special-
        // case it above and never call `emit_expr` on it directly. Kept
        // total (not `unreachable!()`) so a future lowering change that
        // *does* produce a bare `This` fails loudly in `dart analyze`
        // (`this` outside a method) rather than panicking the emitter.
        Expr::This { .. } => "this".to_owned(),
        Expr::Index { target, index, .. } => format!(
            "{}{}[{}]",
            emit_expr(target, used_expr_helper, used_utf8_encode),
            receiver_bang(target),
            emit_expr(index, used_expr_helper, used_utf8_encode)
        ),
        Expr::StringByteLength { target, .. } => {
            *used_utf8_encode = true;
            format!(
                "utf8.encode({}).length",
                emit_expr(target, used_expr_helper, used_utf8_encode)
            )
        }
        // A single-element Dart record needs a trailing comma
        // (`(a,)`) — see `emit_type`'s own `Type::Tuple` arm.
        Expr::Tuple { values, .. } if values.len() == 1 => {
            format!(
                "({},)",
                emit_expr(&values[0], used_expr_helper, used_utf8_encode)
            )
        }
        Expr::Tuple { values, .. } => format!(
            "({})",
            values
                .iter()
                .map(|value| emit_expr(value, used_expr_helper, used_utf8_encode))
                .collect::<Vec<_>>()
                .join(", ")
        ),
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
