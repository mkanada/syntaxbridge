//! Emits [`crate::ir`] as Dart source, formatted to match `dart format`'s
//! output exactly (2-space indent, no trailing whitespace) — criterion 5.2 of
//! `docs/plans/primeiro-corte-e01-e03.md` PR2 requires
//! `dart format --output=none --set-exit-if-changed` to report no diff.
//!
//! `Unsupported` nodes (§4 decision 8 of that plan) are never dropped:
//! - In statement position, they become a `// TODO(syntax-bridge): <reason>`
//!   comment followed by `throw UnimplementedError(...)`.
//! - In expression position, a bare `throw` isn't valid Dart syntax, so they
//!   call a private generic helper with an explicit opaque bridge type. It
//!   still always throws at runtime without introducing `dynamic` into the
//!   generated API. The helper and bridge are emitted only into a file that
//!   needs either one.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use crate::ir::{
    BinaryOp, Constructor, ConstructorInit, Enum, Expr, Function, Method, Module, Origin, Param,
    Record, Stmt, Type, UnaryOp,
};
use crate::lower::cpp::dart_operator_bridge_name;

const INDENT: &str = "  ";
const UNSUPPORTED_HELPER_NAME: &str = "_syntaxBridgeUnsupported";
const OPAQUE_TYPE_NAME: &str = "SyntaxBridgeOpaque";
const PAIR_TYPE_NAME: &str = "SyntaxBridgePair";
/// Must read the same literal name as `lower::cpp::NATIVE_HANDLE_TYPE_NAME`.
const NATIVE_HANDLE_TYPE_NAME: &str = "SyntaxBridgeNativeHandle";
/// `foreach_binding_type_text`'s own marker: nothing else in this emitter
/// ever spells out Dart's built-in `MapEntry<`, so its presence in a printed
/// file is a safe, unambiguous signal that the file needs
/// `emit_map_entry_pair_extension`'s `first`/`second` extension (F15/tarefa
/// 15.5) — the same "distinctive printed text, not a separate tracked flag"
/// pattern `OPAQUE_TYPE_NAME`/`PAIR_TYPE_NAME`/etc. already use.
const MAP_ENTRY_MARKER: &str = "MapEntry<";
/// F10/tarefa 13's `find_if` "declare, guard, dereference" idiom (see
/// `lower::cpp::lower_compound_stmt`'s find/find_if fusion) needs "the first
/// element satisfying a predicate, or null" — `dart:core`'s `Iterable` has
/// no such member (only `package:collection`'s `IterableExtension` does,
/// and AGENTS.md asks for a justified reason before adding any new
/// dependency), so this is a named, single-pass helper in the support file
/// instead, the same "adaptador nomeado" answer `SyntaxBridgePair` already
/// gives `std::make_pair`.
const FIRST_WHERE_HELPER_NAME: &str = "syntaxBridgeFirstWhere";
const INDEX_OF_BYTES_HELPER_NAME: &str = "syntaxBridgeIndexOfBytes";
const INDEX_OF_BYTE_HELPER_NAME: &str = "syntaxBridgeIndexOfByte";
/// `lower::cpp::Type::ListCursor`'s own Dart shape — a named adapter for a
/// long-lived `std::vector<T>::iterator`/`std::string::iterator` (a field,
/// or a local outliving the one recognized guard-and-deref/for-each idiom),
/// the same "named bridge, not an erased type" answer `SyntaxBridgePair`
/// already gives `std::pair`.
const LIST_CURSOR_TYPE_NAME: &str = "SyntaxBridgeListCursor";
const SUPPORT_FILE_NAME: &str = "syntax_bridge_support.dart";

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
    // T2 (`docs/prompts/2026-08-23-02-copia-por-valor-sem-construtor-posicional.md`):
    // a copy of a record that cannot declare the named copy constructor
    // (`T.syntaxBridgeCopyOf`) becomes an honest typed bailout before any
    // file is printed, so neither `emit_expr` nor the import walk can ever
    // see a copy with no constructor to call.
    let module = rewrite_non_copyable_record_copies(module);
    let module = &module;

    // E09: gathered across the *whole* module, not per-file — a mixin and
    // the class that uses it could in principle land in different files
    // (multi-TU dedup is E11's own armadilha, not reopened here), and
    // `emit_record` needs to know "is this record used as a mixin
    // somewhere" before it decides whether to emit `class` or `mixin` and
    // whether its fields need a default value (a `mixin` can't have any
    // constructor at all, unlike the ordinary synthetic positional one E03
    // gives every other record with fields — see `Record::mixins`'s doc
    // comment).
    let mixin_usrs = mixin_usrs(&module.records);

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

    // T2: each record's copy verdict (a blocker string, or `None` for
    // copyable), module-wide — the same scope `mixin_usrs` needs, for the
    // same reason: a record's base chain decides its copyability, and the
    // base can live in another file.
    let mut copy_reasons: HashMap<&str, Option<String>> = HashMap::new();
    {
        let mut visiting: HashSet<&str> = HashSet::new();
        for record in &module.records {
            let blocker = record_copy_blocker(record, &records_by_usr, &mixin_usrs, &mut visiting);
            copy_reasons.insert(record.usr.as_str(), blocker);
        }
    }

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

    let mut files: BTreeMap<String, String> = stems
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
                    &copy_reasons,
                    &mock,
                ),
            )
        })
        .collect();
    let needs_opaque_support = files
        .values()
        .any(|source| source.contains(OPAQUE_TYPE_NAME));
    let needs_pair_support = files.values().any(|source| source.contains(PAIR_TYPE_NAME));
    let needs_native_handle_support = files
        .values()
        .any(|source| source.contains(NATIVE_HANDLE_TYPE_NAME));
    let needs_first_where_support = files
        .values()
        .any(|source| source.contains(FIRST_WHERE_HELPER_NAME));
    let needs_list_cursor_support = files
        .values()
        .any(|source| source.contains(LIST_CURSOR_TYPE_NAME));
    let needs_map_entry_pair_extension = files
        .values()
        .any(|source| source.contains(MAP_ENTRY_MARKER));
    let needs_string_byte_index_support = files.values().any(|source| {
        source.contains(INDEX_OF_BYTES_HELPER_NAME) || source.contains(INDEX_OF_BYTE_HELPER_NAME)
    });
    if needs_opaque_support
        || needs_pair_support
        || needs_native_handle_support
        || needs_first_where_support
        || needs_list_cursor_support
        || needs_map_entry_pair_extension
        || needs_string_byte_index_support
    {
        let mut support = String::new();
        if needs_string_byte_index_support {
            support.push_str("import 'dart:convert';\n\n");
        }
        if needs_opaque_support {
            support.push_str(&emit_opaque_type());
        }
        if needs_pair_support {
            if !support.is_empty() && !support.ends_with("\n\n") {
                support.push('\n');
            }
            support.push_str(&emit_pair_support());
        }
        if needs_native_handle_support {
            if !support.is_empty() && !support.ends_with("\n\n") {
                support.push('\n');
            }
            support.push_str(&emit_native_handle_support());
        }
        if needs_first_where_support {
            if !support.is_empty() && !support.ends_with("\n\n") {
                support.push('\n');
            }
            support.push_str(&emit_first_where_support());
        }
        if needs_list_cursor_support {
            if !support.is_empty() && !support.ends_with("\n\n") {
                support.push('\n');
            }
            support.push_str(&emit_list_cursor_support());
        }
        if needs_map_entry_pair_extension {
            if !support.is_empty() && !support.ends_with("\n\n") {
                support.push('\n');
            }
            support.push_str(&emit_map_entry_pair_extension());
        }
        if needs_string_byte_index_support {
            if !support.is_empty() && !support.ends_with("\n\n") {
                support.push('\n');
            }
            support.push_str(&emit_string_byte_index_support());
        }
        files.insert(format!("lib/{SUPPORT_FILE_NAME}"), support);
    }
    files
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
pub(crate) fn mixin_usrs(records: &[Record]) -> HashSet<&str> {
    let records_by_usr: HashMap<&str, &Record> = records
        .iter()
        .map(|record| (record.usr.as_str(), record))
        .collect();

    let mut result: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = records
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
pub(crate) fn expand_mixin_chain<'a>(
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
    copy_reasons: &HashMap<&str, Option<String>>,
    mock: &MockContext<'_>,
) -> String {
    let mut used_expr_helper = false;
    // Set by the UTF-8 string bridge expressions (`StringByteLength` and
    // `StringByteIndexOf`), which need
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
            copy_reasons,
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
        // See `body_bails_out`'s doc comment: a type only named by a
        // statement the printed function never keeps must not count as an
        // import dependency.
        if !body_bails_out(
            &function.params,
            Some(&function.return_type),
            &function.body,
        ) {
            collect_referenced_usrs_in_stmts(&function.body, &mut referenced_usrs);
        }
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
    if source.contains("Uint8List") {
        import_lines.push("import 'dart:typed_data';".to_owned());
    }
    if source.contains("stderr.") {
        import_lines.push("import 'dart:io';".to_owned());
    }
    // `math.max`/`math.min` (F6/tarefa 07's `std::max`/`std::min` bridge,
    // `lower::cpp::lower_stdlib_free_function_call`) — the one construct in
    // this file that needs a *namespaced* import, since `dart:math`'s own
    // top-level `max`/`min` would otherwise collide with the many project
    // functions and record methods already named `max`/`min` in the real
    // Verovio corpus (grepped directly, e.g. `vrv::Point`'s own `Max`
    // family) — confirmed as a real ambiguity risk, not hypothetical.
    if source.contains("math.max(") || source.contains("math.min(") {
        import_lines.push("import 'dart:math' as math;".to_owned());
    }
    if source.contains(OPAQUE_TYPE_NAME)
        || source.contains(PAIR_TYPE_NAME)
        || source.contains(NATIVE_HANDLE_TYPE_NAME)
        || source.contains(FIRST_WHERE_HELPER_NAME)
        || source.contains(LIST_CURSOR_TYPE_NAME)
        || source.contains(MAP_ENTRY_MARKER)
        || source.contains(INDEX_OF_BYTES_HELPER_NAME)
        || source.contains(INDEX_OF_BYTE_HELPER_NAME)
    {
        import_lines.push(format!("import '{SUPPORT_FILE_NAME}';"));
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
        "T {UNSUPPORTED_HELPER_NAME}<T>(String reason) {{\n{INDENT}throw UnimplementedError(reason);\n}}\n"
    )
}

/// A named bridge for source types that have not acquired their own Dart
/// adapter yet. The original spelling remains in a trailing comment emitted
/// by `emit_type`, so this class never turns an unknown C++ type into an
/// untracked `dynamic` escape hatch.
fn emit_opaque_type() -> String {
    format!("final class {OPAQUE_TYPE_NAME} {{\n{INDENT}const {OPAQUE_TYPE_NAME}();\n}}\n")
}

/// `std::pair`'s members are mutable in C++ (`p.first = x;` is legal and
/// common — real trigger: `iohumdrum.dart:9192`'s `v.second = i;`,
/// F15/tarefa 15.3), so `first`/`second` can't be `final` the way the
/// class's own constructor initializes them — a straight-through member
/// assignment would otherwise become Dart's `assignment_to_final`. Mutable
/// fields mean the constructor itself can no longer be `const` (a `const`
/// constructor requires every field it initializes to be `final`); nothing
/// in this emitter ever constructs a `SyntaxBridgePair` with the `const`
/// keyword (`mock_value_for_type`/`lower::cpp`'s pair-construction lowering
/// both print a plain call), so losing it costs nothing.
fn emit_pair_support() -> String {
    format!(
        "final class {PAIR_TYPE_NAME}<A, B> {{\n{INDENT}{PAIR_TYPE_NAME}(this.first, this.second);\n\n{INDENT}A first;\n{INDENT}B second;\n}}\n"
    )
}

/// F15/tarefa 15.5: a `for (auto &kv : mapa)` range-for over a `std::map<K,
/// V>` lowers its iterable to `mapa.entries` (`lower::cpp`'s
/// `CXXForRangeStmt` case — Dart's `Map` isn't `Iterable`, `Map.entries` is)
/// while keeping the binding's own `Type::Pair(K, V)` — the same type a
/// real `std::pair` variable gets, so the body's own `kv.first`/`kv.second`
/// (`std::pair`'s member names) lower unchanged, as an ordinary
/// `Expr::FieldAccess`. Dart's own `MapEntry<K, V>` (what `.entries` really
/// yields, and what `foreach_binding_type_text` prints for this shape) only
/// exposes `.key`/`.value`, not `.first`/`.second` — this extension supplies
/// them, so lowering never needs to rewrite every `.first`/`.second`
/// occurrence inside the loop body into `.key`/`.value` itself.
fn emit_map_entry_pair_extension() -> String {
    format!(
        "extension SyntaxBridgeMapEntryPair<K, V> on MapEntry<K, V> {{\n{INDENT}K get first => key;\n{INDENT}V get second => value;\n}}\n"
    )
}

/// The Dart type text a `Stmt::ForEach` binding is declared with. Ordinarily
/// just `emit_type(ty)` — but when `ty` is `Type::Pair(K, V)` *and*
/// `iterable` is the `.entries` access `lower::cpp`'s `CXXForRangeStmt` case
/// synthesizes for a `std::map` range-for (see `emit_map_entry_pair_
/// extension`'s doc comment), the binding is really Dart's own
/// `MapEntry<K, V>` — printing `SyntaxBridgePair<K, V>` instead would be
/// simply wrong (that's not what `.entries` yields) even before
/// `.first`/`.second` enter into it.
fn foreach_binding_type_text(ty: &Type, iterable: &Expr) -> String {
    if let Type::Pair(key_ty, value_ty) = ty
        && let Expr::FieldAccess { field, .. } = iterable
        && field == "entries"
    {
        return format!("MapEntry<{}, {}>", emit_type(key_ty), emit_type(value_ty));
    }
    emit_type(ty)
}

/// `std::find_if(X.begin(), X.end(), pred)`'s "first match, or none" —
/// single-pass and side-effect-safe (`pred` runs at most once per element,
/// unlike a `where(pred).isEmpty ? null : where(pred).first` rewrite, which
/// would evaluate it twice over the same prefix).
fn emit_first_where_support() -> String {
    format!(
        "T? {FIRST_WHERE_HELPER_NAME}<T>(Iterable<T> iterable, bool Function(T) test) {{\n\
         {INDENT}for (final item in iterable) {{\n\
         {INDENT}{INDENT}if (test(item)) return item;\n\
         {INDENT}}}\n\
         {INDENT}return null;\n\
         }}\n"
    )
}

/// UTF-8 byte search helpers for `std::basic_string::find` (T4).
fn emit_string_byte_index_support() -> String {
    format!(
        "int {INDEX_OF_BYTES_HELPER_NAME}(String haystack, String needle, [int from = 0]) {{\n\
         {INDENT}if (from < 0) from = 0;\n\
         {INDENT}final hBytes = utf8.encode(haystack);\n\
         {INDENT}final nBytes = utf8.encode(needle);\n\
         {INDENT}if (nBytes.isEmpty) return from <= hBytes.length ? from : -1;\n\
         {INDENT}if (from + nBytes.length > hBytes.length) return -1;\n\
         {INDENT}outer:\n\
         {INDENT}for (int i = from; i <= hBytes.length - nBytes.length; i++) {{\n\
         {INDENT}{INDENT}for (int j = 0; j < nBytes.length; j++) {{\n\
         {INDENT}{INDENT}{INDENT}if (hBytes[i + j] != nBytes[j]) continue outer;\n\
         {INDENT}{INDENT}}}\n\
         {INDENT}{INDENT}return i;\n\
         {INDENT}}}\n\
         {INDENT}return -1;\n\
         }}\n\n\
         int {INDEX_OF_BYTE_HELPER_NAME}(String haystack, int byte, [int from = 0]) {{\n\
         {INDENT}if (from < 0) from = 0;\n\
         {INDENT}final hBytes = utf8.encode(haystack);\n\
         {INDENT}return hBytes.indexOf(byte, from);\n\
         }}\n"
    )
}

/// `lower::cpp::Type::ListCursor`'s Dart shape: a position over a `List<T>`
/// (`current`/`moveNext`/`isEnd`), the named adapter a long-lived
/// `std::vector<T>::iterator`/`std::string::iterator` (`__gnu_cxx::
/// __normal_iterator<...>`) needs when it survives past the one idiom
/// `lower::cpp::lower_find_iterator_guard_idiom`/`lower_iterator_for_loop`
/// can erase entirely — a field, or a local reassigned/held across more than
/// one statement.
fn emit_list_cursor_support() -> String {
    format!(
        "final class {LIST_CURSOR_TYPE_NAME}<T> {{\n\
         {INDENT}{LIST_CURSOR_TYPE_NAME}(this._items, [this._index = 0]);\n\n\
         {INDENT}final List<T> _items;\n\
         {INDENT}int _index;\n\n\
         {INDENT}bool get isEnd => _index >= _items.length;\n\
         {INDENT}T get current => _items[_index];\n\
         {INDENT}void moveNext() {{\n\
         {INDENT}{INDENT}_index++;\n\
         {INDENT}}}\n\
         }}\n"
    )
}

/// A named bridge for a C++ `void*`/`const void*` value — an opaque native
/// pointer this project never dereferences or does address arithmetic on,
/// only ever holds and passes along (`mapping::pointer_options_for`'s
/// `"ponte-dart-ffi"` option, `docs/plans/bailouts-verovio-6.2.0.md`'s
/// phase-4 "void* → handle de domínio nomeado"). Unlike `SyntaxBridgeOpaque`
/// (§ "Definições" of `docs/prompts/2026-08-20-loop-bailout.md` — a
/// placeholder with no connection to the C++ type it replaced), this class
/// *is* the honest Dart shape of a `void*`: identity-only, nothing more,
/// documented as such rather than hiding an unmapped type.
fn emit_native_handle_support() -> String {
    format!(
        "/// An opaque native pointer (`void*`/`const void*` in the original C++)\n\
         /// that Syntax Bridge never dereferences or does address arithmetic on —\n\
         /// only holds and forwards it. Two handles are the same handle only by\n\
         /// identity (`==` is the default `Object` identity check).\n\
         final class {NATIVE_HANDLE_TYPE_NAME} {{\n\
         {INDENT}const {NATIVE_HANDLE_TYPE_NAME}();\n\
         }}\n"
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
        for init in &constructor.inits {
            match init {
                ConstructorInit::Field { value, .. } => {
                    collect_referenced_usrs_in_expr(value, out);
                }
                ConstructorInit::Base { usr, args, .. } => {
                    out.insert(usr.as_str());
                    for arg in args {
                        collect_referenced_usrs_in_expr(arg, out);
                    }
                }
            }
        }
        // See `body_bails_out`'s doc comment: a type only named by a
        // statement the printed constructor never keeps must not count as
        // an import dependency.
        if !body_bails_out(&constructor.params, None, &constructor.body) {
            collect_referenced_usrs_in_stmts(&constructor.body, out);
        }
    }
    for method in &record.methods {
        collect_referenced_usrs_in_type(&method.return_type, out);
        for param in &method.params {
            collect_referenced_usrs_in_type(&param.ty, out);
        }
        if let Some(body) = &method.body
            && !body_bails_out(&method.params, Some(&method.return_type), body)
        {
            collect_referenced_usrs_in_stmts(body, out);
        }
    }
}

fn collect_referenced_usrs_in_type<'a>(ty: &'a Type, out: &mut HashSet<&'a str>) {
    match ty {
        Type::Record { usr, .. } | Type::Enum { usr, .. } => {
            out.insert(usr.as_str());
        }
        Type::List(element) | Type::Set(element) | Type::ListCursor(element) => {
            collect_referenced_usrs_in_type(element, out)
        }
        Type::Map(key, value) | Type::Pair(key, value) => {
            collect_referenced_usrs_in_type(key, out);
            collect_referenced_usrs_in_type(value, out);
        }
        Type::Callback {
            return_type,
            params,
        } => {
            collect_referenced_usrs_in_type(return_type, out);
            for parameter in params {
                collect_referenced_usrs_in_type(parameter, out);
            }
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect_referenced_usrs_in_type(element, out);
            }
        }
        Type::Nullable(inner) => collect_referenced_usrs_in_type(inner, out),
        Type::Int
        | Type::Bool
        | Type::Double
        | Type::Void
        | Type::Str
        | Type::Bytes
        | Type::Object
        | Type::Unsupported(_) => {}
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
        Stmt::ExprAssign {
            target: Expr::MapIndexOrInsert {
                target: map, index, ..
            },
            value,
            ..
        } => {
            collect_referenced_usrs_in_expr(map, out);
            collect_referenced_usrs_in_expr(index, out);
            collect_referenced_usrs_in_expr(value, out);
        }
        Stmt::ExprAssign { target, value, .. } => {
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
        Stmt::DoWhile {
            body, condition, ..
        } => {
            collect_referenced_usrs_in_stmts(body, out);
            collect_referenced_usrs_in_expr(condition, out);
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
        Stmt::ForEach {
            ty, iterable, body, ..
        } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(iterable, out);
            collect_referenced_usrs_in_stmts(body, out);
        }
        Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::ContinueLabel { .. } => {}
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
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            collect_referenced_usrs_in_expr(scrutinee, out);
            for case in cases {
                for value in &case.values {
                    collect_referenced_usrs_in_expr(value, out);
                }
                collect_referenced_usrs_in_stmts(&case.body, out);
            }
            if let Some(default) = default {
                collect_referenced_usrs_in_stmts(default, out);
            }
        }
        Stmt::Unsupported { .. } => {}
    }
}

fn collect_referenced_usrs_in_expr<'a>(expr: &'a Expr, out: &mut HashSet<&'a str>) {
    match expr {
        Expr::IntLiteral { .. }
        | Expr::DoubleLiteral { .. }
        | Expr::BoolLiteral { .. }
        | Expr::NullLiteral { .. }
        | Expr::StringLiteral { .. }
        | Expr::Unsupported { .. } => {}
        Expr::UnsupportedTyped { ty, .. } => collect_referenced_usrs_in_type(ty, out),
        Expr::Ref { ty, .. } | Expr::This { ty, .. } => collect_referenced_usrs_in_type(ty, out),
        Expr::Binary { lhs, rhs, ty, .. } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(lhs, out);
            collect_referenced_usrs_in_expr(rhs, out);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ty,
            ..
        } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(condition, out);
            collect_referenced_usrs_in_expr(then_expr, out);
            collect_referenced_usrs_in_expr(else_expr, out);
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
        Expr::RecordCopy {
            target, type_usr, ..
        } => {
            out.insert(type_usr.as_str());
            collect_referenced_usrs_in_expr(target, out);
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
        Expr::MapIndexOrInsert {
            target,
            index,
            default_value,
            ty,
            ..
        } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(target, out);
            collect_referenced_usrs_in_expr(index, out);
            collect_referenced_usrs_in_expr(default_value, out);
        }
        Expr::StringByteAt {
            target, index, ty, ..
        } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(target, out);
            collect_referenced_usrs_in_expr(index, out);
        }
        Expr::StringByteLength { target, .. } => collect_referenced_usrs_in_expr(target, out),
        Expr::StringByteIndexOf {
            target,
            needle,
            from,
            ..
        } => {
            collect_referenced_usrs_in_expr(target, out);
            collect_referenced_usrs_in_expr(needle, out);
            if let Some(from) = from {
                collect_referenced_usrs_in_expr(from, out);
            }
        }
        Expr::Tuple { values, .. } => {
            for value in values {
                collect_referenced_usrs_in_expr(value, out);
            }
        }
        Expr::ListLiteral { items, ty, .. } => {
            collect_referenced_usrs_in_type(ty, out);
            for item in items {
                collect_referenced_usrs_in_expr(item, out);
            }
        }
        Expr::MapLiteral { entries, ty, .. } => {
            collect_referenced_usrs_in_type(ty, out);
            for (key, value) in entries {
                collect_referenced_usrs_in_expr(key, out);
                collect_referenced_usrs_in_expr(value, out);
            }
        }
        Expr::Is {
            operand,
            target_type,
            ..
        } => {
            collect_referenced_usrs_in_type(target_type, out);
            collect_referenced_usrs_in_expr(operand, out);
        }
        Expr::As { operand, ty, .. } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(operand, out);
        }
        Expr::Assign {
            target, value, ty, ..
        } => {
            collect_referenced_usrs_in_type(ty, out);
            collect_referenced_usrs_in_expr(target, out);
            collect_referenced_usrs_in_expr(value, out);
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
///
/// Every enumerator carries its real C++ value explicitly
/// (`Vermelho(0), Verde(1), Azul(2)`, plus a `const` constructor and a
/// `value` field) rather than relying on Dart's own `.index` — see
/// `ir::Enum::values`'s doc comment for why: C++ enumerators aren't
/// guaranteed 0-based/sequential/gapless, so `.index` alone would silently
/// disagree with the C++ program for any enum that isn't (Verovio itself
/// declares several that aren't). `Expr::Convert` to `Type::Int` with an
/// `Enum`-typed operand (`emit_expr`) reads this same `.value`.
fn emit_enum(enum_decl: &Enum) -> String {
    if enum_decl.variants.is_empty() {
        return format!(
            "// TODO(syntax-bridge): `{}` declares no constants; Dart has no empty enum.\nenum {} {{ unsupportedEmptyEnum(0);\n  const {}(this.value);\n  final int value;\n}}\n",
            enum_decl.name, enum_decl.name, enum_decl.name
        );
    }

    let constants = enum_decl
        .variants
        .iter()
        .zip(&enum_decl.values)
        .map(|(name, value)| format!("{name}({value})"))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "enum {name} {{\n  {constants};\n\n  const {name}(this.value);\n  final int value;\n}}\n",
        name = enum_decl.name,
    )
}

/// A POD `struct`/`class` (`record.constructors.is_empty()` — no
/// user-declared constructor of its own) becomes a Dart class with every
/// field declared `final`... except E03's own armadilha rules that out:
/// `mover` mutates its (by-value-copied) parameter's fields in place, so
/// fields need to stay mutable.
///
/// A record with its own declared constructor(s) (E04) instead emits each
/// one for real (`emit_constructor`, sorted by `constructor_index` — see
/// that field's docs on why sorting, not push order, decides which one is
/// primary), plus every static field and method. The two shapes don't mix on
/// the same record: a class with a hand-written constructor also owns its
/// own field initialization, so the E03 synthetic positional constructor
/// would either be redundant or, worse, a second and inconsistent way to
/// construct the same class.
///
/// T2: every *copyable* record — with or without own constructors —
/// additionally declares the one stable copy form, the named copy
/// constructor `T.syntaxBridgeCopyOf(T other)`
/// (`emit_copy_constructor`), which `Expr::RecordCopy` call sites funnel
/// into. That constructor is exactly why the copy no longer rides the
/// synthetic positional constructor: the two roles used to share it, and a
/// record with own constructors had no positional constructor at all for
/// the copy to call (~2.000 Verovio `extra_positional_arguments`).
#[allow(clippy::too_many_arguments)]
fn emit_record(
    record: &Record,
    is_mixin: bool,
    records_by_usr: &HashMap<&str, &Record>,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    enums_by_usr: &HashMap<&str, &Enum>,
    copy_reasons: &HashMap<&str, Option<String>>,
    mock: &MockContext<'_>,
) -> String {
    // A record that is globally a `mixin` (`is_mixin`) but has a
    // constructor with a base initializer (`: Base(...)`) cannot remain a
    // `mixin` in Dart: a `mixin` may not declare a constructor and `super`
    // does not exist. The original T1 plan (prompt 2026-08-23-01) calls this
    // out explicitly as a product decision — either turn the initializer into
    // an explicit init-method call/bailout or force the record to `class`.
    // This emitter chooses the latter (honest, minimal churn): a mixin that
    // would need `super` is emitted as `class` so the initializer is not
    // silently discarded. The same fallback applies to any written
    // initializer (field or base) — a `mixin` with a constructor that would
    // otherwise be omitted entirely would discard observable initialization.
    let has_ctor_inits = record
        .constructors
        .iter()
        .any(|ctor| !ctor.inits.is_empty());
    let effective_is_mixin = is_mixin && !has_ctor_inits;
    // `abstract` is required the moment a class has any unimplemented
    // member — derived, not stored: a separate `Record.is_abstract` flag
    // could disagree with the method list it's supposed to summarize, so
    // this is the one source of truth for both this keyword and
    // `emit_method`'s own bodyless-signature branch. Meaningless for a
    // `mixin` declaration (Dart's `mixin` keyword has no `abstract` variant
    // — a mixin can't be instantiated at all, so nothing to mark abstract),
    // so skipped entirely for that case.
    let abstract_keyword =
        if !effective_is_mixin && record.methods.iter().any(|method| method.body.is_none()) {
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
    // `effective_is_mixin` above already forced a fallback to `class` when a
    // constructor would otherwise need `super`/initializer emission.
    let bases_clause = if effective_is_mixin {
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
    let keyword = if effective_is_mixin { "mixin" } else { "class" };
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
        // `effective_is_mixin` already accounts for the T1 fallback above.
        if !effective_is_mixin && record.constructors.is_empty() {
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

    if effective_is_mixin {
        // A `mixin` declaration can't have a constructor at all (Dart
        // rejects one outright) — every field already got its zero-value
        // default above, which is the only initialization a mixin's own
        // fields ever get. `effective_is_mixin` already forced a fallback
        // to `class` when inits exist, so this branch truly has none.
    } else if record.constructors.is_empty() {
        let ctor_params = record
            .fields
            .iter()
            .map(|field| format!("this.{}", field.name))
            .collect::<Vec<_>>()
            .join(", ");

        // A field type the IR can't represent (`SyntaxBridgeOpaque /*
        // unsupported: ... */` above) means this class's shape is incomplete — silently allowing
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

    // T2: the one stable copy form, declared by every record that can
    // declare constructors at all (`copy_reasons` carries the blocker for
    // the ones that can't — a `mixin`, an uncopiable base chain — whose
    // copy sites were already rewritten into honest bailouts before
    // emission). Same branch for records with and without own
    // constructors: the copy never rides a constructor the record happens
    // to have, it has one of its own.
    if !effective_is_mixin
        && copy_reasons
            .get(record.usr.as_str())
            .is_some_and(Option::is_none)
    {
        source.push('\n');
        source.push_str(&emit_copy_constructor(record, records_by_usr, copy_reasons));
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
        Type::Bytes => Some("Uint8List(0)".to_owned()),
        Type::List(_) => Some("[]".to_owned()),
        // Dart's `{}` is an empty set or an empty map depending on the
        // context type, which is exactly the declared type here.
        Type::Set(_) | Type::Map(_, _) => Some("{}".to_owned()),
        Type::Enum { usr, name } => {
            let first = enums_by_usr.get(usr.as_str())?.variants.first()?;
            Some(format!("{name}.{first}"))
        }
        Type::Record { .. }
        | Type::Pair(_, _)
        | Type::ListCursor(_)
        | Type::Callback { .. }
        | Type::Tuple(_)
        | Type::Void
        | Type::Object
        | Type::Unsupported(_) => None,
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
        Type::Pair(first, second) => Some(format!(
            "{PAIR_TYPE_NAME}({}, {})",
            mock_value_for_type(first, enums_by_usr, records_by_usr, depth + 1)?,
            mock_value_for_type(second, enums_by_usr, records_by_usr, depth + 1)?,
        )),
        Type::Callback { .. } => None,
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
            // A fresh, throwaway `Promoted`: this whole call renders exactly
            // one bailout statement, never a real function body with a flow
            // of its own to track.
            emit_stmt(
                &bailout,
                depth,
                used_expr_helper,
                used_utf8_encode,
                &mut Promoted::new(),
            )
        }
    }
}

fn expr_contains_this(expr: &Expr) -> bool {
    match expr {
        Expr::This { .. } => true,
        Expr::Ref { .. }
        | Expr::IntLiteral { .. }
        | Expr::DoubleLiteral { .. }
        | Expr::BoolLiteral { .. }
        | Expr::NullLiteral { .. }
        | Expr::StringLiteral { .. }
        | Expr::Unsupported { .. } => false,
        Expr::UnsupportedTyped { .. } => false,
        Expr::Binary { lhs, rhs, .. } => expr_contains_this(lhs) || expr_contains_this(rhs),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            expr_contains_this(condition)
                || expr_contains_this(then_expr)
                || expr_contains_this(else_expr)
        }
        Expr::Unary { operand, .. } => expr_contains_this(operand),
        Expr::Convert { operand, .. } => expr_contains_this(operand),
        Expr::Call { target, args, .. } => {
            target.as_ref().is_some_and(|t| expr_contains_this(t))
                || args.iter().any(expr_contains_this)
        }
        Expr::FieldAccess { target, .. } => expr_contains_this(target),
        Expr::RecordConstruct { fields, .. } => fields.iter().any(|(_, v)| expr_contains_this(v)),
        Expr::RecordCopy { target, .. } => expr_contains_this(target),
        Expr::ConstructorCall { args, .. } => args.iter().any(expr_contains_this),
        Expr::Index { target, index, .. } => {
            expr_contains_this(target) || expr_contains_this(index)
        }
        Expr::MapIndexOrInsert {
            target,
            index,
            default_value,
            ..
        } => {
            expr_contains_this(target)
                || expr_contains_this(index)
                || expr_contains_this(default_value)
        }
        Expr::StringByteLength { target, .. } => expr_contains_this(target),
        Expr::StringByteIndexOf {
            target,
            needle,
            from,
            ..
        } => {
            expr_contains_this(target)
                || expr_contains_this(needle)
                || from.as_ref().is_some_and(|f| expr_contains_this(f))
        }
        Expr::StringByteAt { target, index, .. } => {
            expr_contains_this(target) || expr_contains_this(index)
        }
        Expr::Tuple { values, .. } => values.iter().any(expr_contains_this),
        Expr::ListLiteral { items, .. } => items.iter().any(expr_contains_this),
        Expr::MapLiteral { entries, .. } => entries
            .iter()
            .any(|(k, v)| expr_contains_this(k) || expr_contains_this(v)),
        Expr::Is { operand, .. } => expr_contains_this(operand),
        Expr::As { operand, .. } => expr_contains_this(operand),
        Expr::Assign { target, value, .. } => {
            expr_contains_this(target) || expr_contains_this(value)
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
    if mock.is_external(&constructor.usr) {
        let body = format!(
            "{}// syntax-bridge: externo, corpo mockado\n",
            INDENT.repeat(2)
        );
        return format!("{INDENT}{dart_name}({params}) {{\n{body}{INDENT}}}\n");
    }

    // Split initializer list into Dart-legal initializer part and body
    // assignments. `super` must be last, only one exists, and no initializer
    // expression may read `this` (Dart rule) — the `Field` whose `value`
    // references another field of the same object is moved to the first line
    // of the body as a plain assignment. This is the only
    // order-observable transformation T1 permits, and it is safe because the
    // field already has a default at its declaration.
    let mut field_inits_for_list: Vec<(&String, &Expr)> = Vec::new();
    let mut field_inits_for_body: Vec<(&String, &Expr)> = Vec::new();
    let mut base_init: Option<(&String, &String, &Vec<Expr>)> = None;
    let mut extra_base_bailouts: Vec<String> = Vec::new();

    for init in &constructor.inits {
        match init {
            ConstructorInit::Field { name, value } => {
                if expr_contains_this(value) {
                    field_inits_for_body.push((name, value));
                } else {
                    field_inits_for_list.push((name, value));
                }
            }
            ConstructorInit::Base { usr, name, args } => {
                if base_init.is_none() {
                    // Super args also may not read `this`; if they do, the
                    // initializer is not Dart-legal and must become a bailout.
                    let reads_this = args.iter().any(expr_contains_this);
                    if reads_this {
                        extra_base_bailouts.push(format!(
                            "base initializer `{name}` reads `this` and cannot be in the initializer list"
                        ));
                    } else {
                        base_init = Some((usr, name, args));
                    }
                } else {
                    extra_base_bailouts.push(format!(
                        "extra base initializer `{name}` — Dart only allows one `super`"
                    ));
                }
            }
        }
    }

    let mut init_parts: Vec<String> = Vec::new();
    for (name, value) in &field_inits_for_list {
        let val_text = emit_expr(
            value,
            used_expr_helper,
            used_utf8_encode,
            &mut Promoted::new(),
        );
        init_parts.push(format!("{name} = {val_text}"));
    }
    if let Some((_, _, args)) = base_init {
        let args_text = args
            .iter()
            .map(|arg| {
                emit_expr(
                    arg,
                    used_expr_helper,
                    used_utf8_encode,
                    &mut Promoted::new(),
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        init_parts.push(format!("super({args_text})"));
    }
    let initializer = if init_parts.is_empty() {
        String::new()
    } else {
        format!(" : {}", init_parts.join(", "))
    };

    // Body: moved field inits first, then extra base bailouts, then original
    // body. Moved field inits become `field = value;` (implicit `this`).
    let mut body_stmts: Vec<Stmt> = Vec::new();
    for (name, value) in field_inits_for_body {
        body_stmts.push(Stmt::FieldAssign {
            target: Expr::This {
                ty: Type::Void,
                origin: constructor.origin.clone(),
            },
            field: (*name).clone(),
            value: (*value).clone(),
            origin: constructor.origin.clone(),
        });
    }
    for reason in extra_base_bailouts {
        body_stmts.push(Stmt::Unsupported {
            reason,
            origin: constructor.origin.clone(),
        });
    }
    body_stmts.extend(constructor.body.clone());

    let body = emit_body(
        &constructor.params,
        None,
        &body_stmts,
        &constructor.origin,
        used_expr_helper,
        used_utf8_encode,
        2,
    );
    format!("{INDENT}{dart_name}({params}){initializer} {{\n{body}{INDENT}}}\n")
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
        // Dart has no free assignment/increment/stream operators, but a
        // C++ member operator still has an ordinary method body whose state
        // changes must be retained. Give it the stable named bridge used by
        // call lowering and emit that body normally; any unsupported detail
        // inside remains visible at its actual source location.
        return emit_method_named(
            dart_operator_bridge_name(&method.name, method.params.len()),
            method,
            mock,
            used_expr_helper,
            used_utf8_encode,
        );
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
                let default = param
                    .default_value
                    .as_ref()
                    .expect("filtered by is_some above");
                // Dart requires a parameter default to be a compile-time
                // constant. Every other default this module builds already
                // is one (`emit_expr`'s own literal/`.value` shapes) except
                // the empty `List<Object?>` a variadic C++ parameter's
                // trailing-arguments collector defaults to
                // (`lower::cpp::collect_params_with_clone_prelude`,
                // F15/tarefa 15.7): `emit_expr`'s `Expr::ListLiteral` arm
                // prints a plain (non-`const`) literal, since a list
                // default is otherwise never produced. `const []` lets Dart
                // infer the element type from the parameter's own declared
                // type, so no explicit `<Object?>` is needed here either.
                let default_text = if matches!(default, Expr::ListLiteral { items, .. } if items.is_empty())
                {
                    "const []".to_owned()
                } else {
                    emit_expr(
                        default,
                        used_expr_helper,
                        used_utf8_encode,
                        // A default-value expression stands alone — never a
                        // dereference chained onto an earlier one in the
                        // same parameter list — so it needs no shared
                        // `Promoted` state with the rest of the signature.
                        &mut Promoted::new(),
                    )
                };
                format!("{} {} = {default_text}", emit_type(&param.ty), param.name)
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

/// T2 (`docs/prompts/2026-08-23-02-copia-por-valor-sem-construtor-posicional.md`):
/// why a record cannot be value-copied, or `None` when it can. A copyable
/// record declares the one stable copy form — the named copy constructor
/// `T.syntaxBridgeCopyOf(T other)` (`emit_record`) — that every copy site
/// (by-value parameter prelude, implicit `operator=`, copy-construction
/// `new T(src)`) funnels into. A record is *not* copyable when:
///
/// - it is emitted as a Dart `mixin` (`mixin_usrs` membership, minus the
///   `emit_record` fallback that forces a mixin-with-constructor-initializers
///   back to `class`) — a `mixin` declaration cannot have any constructor;
/// - its `extends` base is unknown to the module (nothing to copy its
///   fields from or through) or is itself not copyable — the copy
///   constructor's `super.syntaxBridgeCopyOf(other)` needs the base to own
///   one too;
/// - any mixin applied through its `with` clause (the transitively expanded
///   chain, minus members already covered by the `extends` chain, whose
///   fields `super` copies) is unknown to the module or declares a private
///   (`_`-prefixed) field: Dart privacy is per-library, so the applying
///   class — always another file — can neither read nor write it, and a
///   copy that silently skipped it would be exactly the partial copy T2
///   forbids.
///
/// Field *types* never block copying (unlike the zero-value path): the copy
/// constructor assigns from `other`, so a field with no sound default
/// (`late`) or an opaque bridge type is copied as soundly as any other.
pub(crate) fn record_copy_blocker<'a>(
    record: &'a Record,
    records_by_usr: &HashMap<&str, &'a Record>,
    mixin_usrs: &HashSet<&str>,
    visiting: &mut HashSet<&'a str>,
) -> Option<String> {
    if !visiting.insert(record.usr.as_str()) {
        // A base cycle (impossible in valid C++, this is just a defensive
        // stop) says nothing about copyability — treat it as copyable and
        // let the outer levels compose their own verdict.
        return None;
    }
    let verdict = record_copy_blocker_inner(record, records_by_usr, mixin_usrs, visiting);
    visiting.remove(record.usr.as_str());
    verdict
}

fn record_copy_blocker_inner<'a>(
    record: &'a Record,
    records_by_usr: &HashMap<&str, &'a Record>,
    mixin_usrs: &HashSet<&str>,
    visiting: &mut HashSet<&'a str>,
) -> Option<String> {
    // Same formula `emit_record` uses to pick `mixin` vs `class`: a mixin
    // whose constructor would need `super`/initializer emission falls back
    // to `class`, and only a record actually emitted as `mixin` is barred
    // from declaring constructors.
    let has_ctor_inits = record
        .constructors
        .iter()
        .any(|ctor| !ctor.inits.is_empty());
    if mixin_usrs.contains(record.usr.as_str()) && !has_ctor_inits {
        return Some("emitido como mixin, não pode declarar construtor de cópia".to_owned());
    }
    if let Some(base) = &record.base_class {
        let Some(base_record) = records_by_usr.get(base.usr.as_str()) else {
            return Some(format!(
                "a base `{}` não está no módulo, não há como copiar seus campos",
                base.name
            ));
        };
        if let Some(blocker) =
            record_copy_blocker(base_record, records_by_usr, mixin_usrs, visiting)
        {
            return Some(format!("a base `{}` não é copiável: {blocker}", base.name));
        }
    }
    // Fields reachable through the `with` clause but *not* through the
    // `extends` chain (those are `super.syntaxBridgeCopyOf`'s job — walking
    // them here would both double-copy them and, worse, wrongly block on
    // the base's private fields, which the super copy handles legally).
    let mut extends_chain: HashSet<&str> = HashSet::new();
    let mut cursor = record.base_class.as_ref();
    while let Some(base) = cursor {
        if !extends_chain.insert(base.usr.as_str()) {
            break;
        }
        cursor = records_by_usr
            .get(base.usr.as_str())
            .and_then(|base_record| base_record.base_class.as_ref());
    }
    for mixin in expand_mixin_chain(&record.mixins, records_by_usr) {
        if extends_chain.contains(mixin.usr.as_str()) {
            continue;
        }
        let Some(mixin_record) = records_by_usr.get(mixin.usr.as_str()) else {
            return Some(format!(
                "o mixin `{}` não está no módulo, seus campos não podem ser copiados",
                mixin.name
            ));
        };
        if let Some(field) = mixin_record
            .fields
            .iter()
            .find(|field| field.name.starts_with('_'))
        {
            return Some(format!(
                "o mixin `{}` declara o campo privado `{}`, que outra biblioteca Dart \
                 não pode ler",
                mixin.name, field.name
            ));
        }
    }
    None
}

/// The `T.syntaxBridgeCopyOf(T other)` declaration every copyable record
/// gets (T2): a generative named constructor that rebuilds the value field
/// by field — the record's *own* fields, the public fields of every `with`
/// mixin (their private ones made the record non-copyable already), and the
/// `extends` chain through `super.syntaxBridgeCopyOf(other)`, so a derived
/// copy copies its base subobject exactly like C++ does. Copy semantics per
/// field: a copyable record is deep-copied through its own copy
/// constructor; a mutable collection (`List`/`Set`/`Map`/`Bytes`) is copied
/// (`List.of`/`Set.of`/`Map.of`/`Uint8List.fromList`, the same bridges the
/// old implicit-`operator=` lowering spelled at every assignment site);
/// everything else — scalars, the immutable `String`, nullable pointers —
/// is assigned as-is, which is observably the same value C++'s own member
/// copy produces (a C++ pointer field aliases too).
fn emit_copy_constructor(
    record: &Record,
    records_by_usr: &HashMap<&str, &Record>,
    copy_reasons: &HashMap<&str, Option<String>>,
) -> String {
    // T2's own `not_initialized_non_nullable_instance_field` regression
    // (first hit on E11 `Comum.x` — `int x;` before `emit_record` added the
    // copy constructor): Dart's flow analysis needs a non-nullable field
    // either `late`/default-initialized or definitely assigned via the
    // constructor's initializer list — assignment in the body is not
    // enough. Own fields satisfy it here through a field initializer
    // (`x = other.x` — implicitly `this.x = ...`); mixin-inherited fields
    // cannot be initialized that way and stay in the body, which is fine
    // because every field a `mixin` actually declares is `late` or
    // default-initialized (`emit_record`'s `effective_is_mixin` path).
    let mut own_inits: Vec<String> = Vec::new();
    let mut mixin_body = String::new();
    let mut emit_own_field = |field: &crate::ir::Field| {
        let value = match &field.ty {
            Type::Record { usr, name }
                if copy_reasons.get(usr.as_str()).is_some_and(Option::is_none) =>
            {
                format!("{name}.syntaxBridgeCopyOf(other.{})", field.name)
            }
            Type::List(_) => format!("List.of(other.{})", field.name),
            Type::Set(_) => format!("Set.of(other.{})", field.name),
            Type::Map(_, _) => format!("Map.of(other.{})", field.name),
            Type::Bytes => format!("Uint8List.fromList(other.{})", field.name),
            _ => format!("other.{}", field.name),
        };
        own_inits.push(format!("{} = {value}", field.name));
    };
    for field in &record.fields {
        emit_own_field(field);
    }
    let mut emit_mixin_field = |field: &crate::ir::Field| {
        let value = match &field.ty {
            Type::Record { usr, name }
                if copy_reasons.get(usr.as_str()).is_some_and(Option::is_none) =>
            {
                format!("{name}.syntaxBridgeCopyOf(other.{})", field.name)
            }
            Type::List(_) => format!("List.of(other.{})", field.name),
            Type::Set(_) => format!("Set.of(other.{})", field.name),
            Type::Map(_, _) => format!("Map.of(other.{})", field.name),
            Type::Bytes => format!("Uint8List.fromList(other.{})", field.name),
            _ => format!("other.{}", field.name),
        };
        mixin_body.push_str(&format!("{INDENT}{INDENT}{} = {value};\n", field.name));
    };
    let mut extends_chain: HashSet<&str> = HashSet::new();
    let mut cursor = record.base_class.as_ref();
    while let Some(base) = cursor {
        if !extends_chain.insert(base.usr.as_str()) {
            break;
        }
        cursor = records_by_usr
            .get(base.usr.as_str())
            .and_then(|base_record| base_record.base_class.as_ref());
    }
    for mixin in expand_mixin_chain(&record.mixins, records_by_usr) {
        if extends_chain.contains(mixin.usr.as_str()) {
            continue;
        }
        if let Some(mixin_record) = records_by_usr.get(mixin.usr.as_str()) {
            for field in &mixin_record.fields {
                emit_mixin_field(field);
            }
        }
    }
    let mut init_parts: Vec<String> = Vec::new();
    init_parts.extend(own_inits);
    if record.base_class.is_some() {
        init_parts.push("super.syntaxBridgeCopyOf(other)".to_owned());
    }
    let initializer = if init_parts.is_empty() {
        String::new()
    } else {
        format!(" : {}", init_parts.join(", "))
    };
    format!(
        "{INDENT}{}.syntaxBridgeCopyOf({} other){initializer} {{\n{mixin_body}{INDENT}}}\n",
        record.name, record.name
    )
}

/// T2: replaces every `Expr::RecordCopy` of a record that cannot declare the
/// named copy constructor with an honest typed bailout at the copy's own
/// position — never a silent partial copy. Emission borrows the module it
/// prints, so the rewrite works on one clone (the same shape the extraction
/// pipeline already uses when it hands IR between stages) and returns it as
/// the module everything downstream prints. Statement-position copies (the
/// by-value parameter prelude, an implicit `operator=`) render as the typed
/// throwing expression they become — the body below them is unreachable
/// after the throw, and the parameter/local keeps its declared static type
/// (no `dynamic`).
fn rewrite_non_copyable_record_copies(module: &Module) -> Module {
    let records_by_usr: HashMap<&str, &Record> = module
        .records
        .iter()
        .map(|record| (record.usr.as_str(), record))
        .collect();
    let mixin_usrs = mixin_usrs(&module.records);
    let mut copy_reasons: HashMap<&str, Option<String>> = HashMap::new();
    let mut visiting: HashSet<&str> = HashSet::new();
    for record in &module.records {
        let blocker = record_copy_blocker(record, &records_by_usr, &mixin_usrs, &mut visiting);
        copy_reasons.insert(record.usr.as_str(), blocker);
    }

    let mut rewritten = module.clone();
    let rewrite_bodies = |record: &mut Record| {
        for constructor in &mut record.constructors {
            rewrite_stmts_for_blocked_copy(&mut constructor.body, &copy_reasons);
        }
        for method in &mut record.methods {
            if let Some(body) = &mut method.body {
                rewrite_stmts_for_blocked_copy(body, &copy_reasons);
            }
        }
    };
    for record in &mut rewritten.records {
        rewrite_bodies(record);
    }
    for function in &mut rewritten.functions {
        rewrite_stmts_for_blocked_copy(&mut function.body, &copy_reasons);
    }
    rewritten
}

fn rewrite_stmts_for_blocked_copy(
    stmts: &mut [Stmt],
    copy_reasons: &HashMap<&str, Option<String>>,
) {
    let mut visitor = crate::function_catalog::IrRefVisitor {
        on_type: &mut |_| {},
        on_record_construct: &mut |_, _, _| {},
        on_expr: &mut |expr: &mut Expr| {
            if let Expr::RecordCopy {
                type_usr,
                type_name,
                origin,
                ..
            } = expr
                && let Some(Some(blocker)) = copy_reasons.get(type_usr.as_str())
            {
                *expr = Expr::UnsupportedTyped {
                    reason: format!("cópia por valor de {type_name} não copiável: {blocker}"),
                    ty: Type::Record {
                        usr: type_usr.clone(),
                        name: type_name.clone(),
                    },
                    origin: origin.clone(),
                };
            }
        },
    };
    visitor.visit_stmts(stmts);
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
    // (`dart_operator_bridge_name`), not a per-project one.
    let name = if function.name.starts_with("operator") {
        dart_operator_bridge_name(&function.name, function.params.len())
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
/// A parameter or return type the IR can't represent is emitted through the
/// named `SyntaxBridgeOpaque` bridge. If the body then ran as normal Dart on
/// top of it, it would silently compute on a value with no faithful mapping
/// (e.g. arithmetic on a `long` parameter) instead of ever signaling the
/// translation is incomplete.
/// Checked before the body itself: a signature-level failure takes priority
/// and makes the body's own contents irrelevant.
///
/// A *body-local* `Type::Unsupported` (a local variable's declared type, or
/// an expression's own inferred type — e.g. `int / long` promoting to `long`
/// under C++'s usual arithmetic conversions) needs the same treatment:
/// `emit_binary_op`'s truncating-division rule, for one, reads exactly that
/// type to decide `/` vs `~/`, and a type it doesn't recognize means that
/// decision can't be trusted either way.
/// True exactly when `emit_body` collapses `body` into a single bailout
/// `throw` instead of printing it statement-by-statement — same condition
/// `emit_body` itself computes (kept here, not called from there, so the two
/// never need to agree by convention alone: see the doc comment on why this
/// exists). `collect_referenced_usrs_in_record` and the top-level
/// free-function loop in `emit_module_with_externals` both need it: a type
/// only ever named by a statement that gets discarded this way must not
/// count as a dependency the file needs to import (F15/tarefa 15.2) — real
/// trigger: `CalcAlignmentPitchPosFunctor::VisitLayerElement` in the real
/// Verovio 6.2.0 corpus, whose full body switches on several concrete
/// note-like types before hitting one unsupported `std::list` iterator use
/// partway through; the printed method is a single `throw`, but the old
/// import-usr walk still counted every one of those types.
fn body_bails_out(params: &[Param], return_type: Option<&Type>, body: &[Stmt]) -> bool {
    return_type.and_then(unsupported_spelling).is_some()
        || first_unsupported_signature_param(params).is_some()
        || first_unsupported_type_in_list(body).is_some()
        || first_unsupported_in_list(body).is_some()
}

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

    // Fresh per body: a function/method/constructor body is exactly the
    // scope Dart's own flow-sensitive promotion resets at — no parameter
    // starts promoted, and nothing from a caller's own body (there isn't
    // one visible here) could apply.
    let mut promoted = Promoted::new();
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
        Some(unsupported) => emit_stmt(
            unsupported,
            depth,
            used_expr_helper,
            used_utf8_encode,
            &mut promoted,
        ),
        None => {
            let mut text = String::new();
            for stmt in body {
                text.push_str(&emit_stmt(
                    stmt,
                    depth,
                    used_expr_helper,
                    used_utf8_encode,
                    &mut promoted,
                ));
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
        Stmt::ExprAssign {
            target: Expr::MapIndexOrInsert {
                target: map, index, ..
            },
            value,
            ..
        } => expr_unsupported_type_spelling(map)
            .or_else(|| expr_unsupported_type_spelling(index))
            .or_else(|| expr_unsupported_type_spelling(value)),
        Stmt::ExprAssign { target, value, .. } => {
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
        Stmt::DoWhile {
            body, condition, ..
        } => first_unsupported_type_in_list(body)
            .or_else(|| expr_unsupported_type_spelling(condition)),
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
        Stmt::ForEach {
            ty, iterable, body, ..
        } => unsupported_spelling(ty)
            .or_else(|| expr_unsupported_type_spelling(iterable))
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
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => expr_unsupported_type_spelling(scrutinee)
            .or_else(|| {
                cases.iter().find_map(|case| {
                    case.values
                        .iter()
                        .find_map(expr_unsupported_type_spelling)
                        .or_else(|| first_unsupported_type_in_list(&case.body))
                })
            })
            .or_else(|| default.as_deref().and_then(first_unsupported_type_in_list)),
        Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::ContinueLabel { .. } => None,
        Stmt::Unsupported { .. } => None,
    }
}

fn expr_unsupported_type_spelling(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::IntLiteral { .. }
        | Expr::DoubleLiteral { .. }
        | Expr::BoolLiteral { .. }
        | Expr::NullLiteral { .. }
        | Expr::StringLiteral { .. }
        | Expr::Unsupported { .. } => None,
        Expr::UnsupportedTyped { ty, .. } => unsupported_spelling(ty),
        Expr::Ref { ty, .. } => unsupported_spelling(ty),
        Expr::Binary { ty, lhs, rhs, .. } => unsupported_spelling(ty)
            .or_else(|| expr_unsupported_type_spelling(lhs))
            .or_else(|| expr_unsupported_type_spelling(rhs)),
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ty,
            ..
        } => unsupported_spelling(ty)
            .or_else(|| expr_unsupported_type_spelling(condition))
            .or_else(|| expr_unsupported_type_spelling(then_expr))
            .or_else(|| expr_unsupported_type_spelling(else_expr)),
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
        Expr::RecordCopy { target, .. } => expr_unsupported_type_spelling(target),
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
        Expr::MapIndexOrInsert {
            target,
            index,
            default_value,
            ty,
            ..
        } => unsupported_spelling(ty)
            .or_else(|| expr_unsupported_type_spelling(target))
            .or_else(|| expr_unsupported_type_spelling(index))
            .or_else(|| expr_unsupported_type_spelling(default_value)),
        Expr::StringByteLength { target, .. } => expr_unsupported_type_spelling(target),
        Expr::StringByteIndexOf {
            target,
            needle,
            from,
            ..
        } => expr_unsupported_type_spelling(target)
            .or_else(|| expr_unsupported_type_spelling(needle))
            .or_else(|| {
                from.as_ref()
                    .and_then(|f| expr_unsupported_type_spelling(f))
            }),
        Expr::StringByteAt {
            target, index, ty, ..
        } => unsupported_spelling(ty)
            .or_else(|| expr_unsupported_type_spelling(target))
            .or_else(|| expr_unsupported_type_spelling(index)),
        Expr::Tuple { values, .. } => values.iter().find_map(expr_unsupported_type_spelling),
        Expr::ListLiteral { items, ty, .. } => unsupported_spelling(ty)
            .or_else(|| items.iter().find_map(expr_unsupported_type_spelling)),
        Expr::MapLiteral { entries, ty, .. } => unsupported_spelling(ty).or_else(|| {
            entries.iter().find_map(|(key, value)| {
                expr_unsupported_type_spelling(key)
                    .or_else(|| expr_unsupported_type_spelling(value))
            })
        }),
        Expr::Is {
            operand,
            target_type,
            ..
        } => unsupported_spelling(target_type).or_else(|| expr_unsupported_type_spelling(operand)),
        Expr::As { operand, ty, .. } => {
            unsupported_spelling(ty).or_else(|| expr_unsupported_type_spelling(operand))
        }
        Expr::Assign {
            target, value, ty, ..
        } => unsupported_spelling(ty)
            .or_else(|| expr_unsupported_type_spelling(target))
            .or_else(|| expr_unsupported_type_spelling(value)),
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
        Stmt::ForEach { body, .. } => first_unsupported_in_list(body),
        Stmt::DoWhile { body, .. } => first_unsupported_in_list(body),
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
        Stmt::Switch { cases, default, .. } => cases
            .iter()
            .find_map(|case| first_unsupported_in_list(&case.body))
            .or_else(|| default.as_deref().and_then(first_unsupported_in_list)),
        Stmt::Return { .. }
        | Stmt::VarDecl { .. }
        | Stmt::Assign { .. }
        | Stmt::FieldAssign { .. }
        | Stmt::ExprAssign { .. }
        | Stmt::ExprStmt { .. }
        | Stmt::TupleAssign { .. }
        | Stmt::Break { .. }
        | Stmt::Continue { .. }
        | Stmt::ContinueLabel { .. }
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
        Type::Bytes => "Uint8List".to_owned(),
        Type::List(element) => format!("List<{}>", emit_type(element)),
        Type::Set(element) => format!("Set<{}>", emit_type(element)),
        Type::Map(key, value) => format!("Map<{}, {}>", emit_type(key), emit_type(value)),
        Type::Pair(first, second) => format!(
            "{PAIR_TYPE_NAME}<{}, {}>",
            emit_type(first),
            emit_type(second)
        ),
        Type::ListCursor(element) => format!("{LIST_CURSOR_TYPE_NAME}<{}>", emit_type(element)),
        Type::Callback {
            return_type,
            params,
        } => format!(
            "{} Function({})",
            emit_type(return_type),
            params.iter().map(emit_type).collect::<Vec<_>>().join(", ")
        ),
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
        Type::Object => "Object".to_owned(),
        Type::Unsupported(spelling) => {
            format!("{OPAQUE_TYPE_NAME} /* unsupported: {spelling} */")
        }
    }
}

fn emit_stmt(
    stmt: &Stmt,
    depth: usize,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    promoted: &mut Promoted,
) -> String {
    let pad = INDENT.repeat(depth);
    match stmt {
        Stmt::Return { value, .. } => match value {
            Some(expr) => format!(
                "{pad}return {};\n",
                emit_expr(expr, used_expr_helper, used_utf8_encode, promoted)
            ),
            None => format!("{pad}return;\n"),
        },
        // Dart requires every non-nullable local to be initialized where
        // it's declared — `int i;` is as much a compile error as
        // `Ponto p;` would be. `late` defers that requirement to first use,
        // the closest match to C++ letting a local sit default-constructed
        // (or, for a POD's scalar fields, indeterminate) until assigned.
        //
        // A declaration always removes `name` from `promoted` first: even
        // where a fixture reuses a name, the freshly declared local is a new
        // binding with no promotion history of its own yet — the first
        // dereference of it always needs its own `!`.
        Stmt::VarDecl { name, ty, init, .. } => match init {
            Some(expr) => {
                let init_text = emit_expr(expr, used_expr_helper, used_utf8_encode, promoted);
                promoted.remove(name);
                format!("{pad}{} {name} = {init_text};\n", emit_type(ty))
            }
            None => {
                promoted.remove(name);
                format!("{pad}late {} {name};\n", emit_type(ty))
            }
        },
        Stmt::Assign { name, value, .. } => {
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            // Reassignment invalidates any promotion `name` held — see
            // `receiver_bang`'s own doc comment.
            promoted.remove(name);
            format!("{pad}{name} = {value_text};\n")
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
                let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
                format!("{pad}{field} = {value_text};\n")
            }
            _ => {
                let target_text =
                    emit_receiver(target, used_expr_helper, used_utf8_encode, promoted);
                let bang = receiver_bang(target, promoted);
                let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
                format!("{pad}{target_text}{bang}.{field} = {value_text};\n")
            }
        },
        Stmt::ExprAssign {
            target: Expr::MapIndexOrInsert {
                target: map, index, ..
            },
            value,
            ..
        } => {
            let map_text = emit_receiver(map, used_expr_helper, used_utf8_encode, promoted);
            let bang = receiver_bang(map, promoted);
            let index_text = emit_expr(index, used_expr_helper, used_utf8_encode, promoted);
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            format!("{pad}{map_text}{bang}[{index_text}] = {value_text};\n")
        }
        // Assigning *through* a `T*` out-param (`(*out) = value;`, C++'s
        // idiom for a mutable output parameter — Verovio's real
        // `ParseAddSylAction`/`ParseDragAction`/`ParseInsertAction`, among
        // others) lowers its target through the same `lower_expr` every
        // *read* of `*out` goes through, which represents the dereference as
        // `Expr::Convert` and renders it with the `!` a read needs. An
        // assignment target is a write, not a read: this model has no
        // separate pointee storage to write into — the "pointer" *is* the
        // nullable slot — so the target is just `operand` reassigned
        // directly, never `operand!` (a bare `!` is a read-only null-assertion,
        // never valid syntax on an assignment's left side; two real Verovio
        // files fail to parse as Dart without this case, see
        // `assigning_through_a_string_out_param_reassigns_the_nullable_local_without_a_bang`
        // in `crates/server/tests/lower_cpp.rs`).
        Stmt::ExprAssign {
            target: Expr::Convert { operand, .. },
            value,
            ..
        } if operand.is_assignable_lvalue() => {
            let target_text = emit_expr(operand, used_expr_helper, used_utf8_encode, promoted);
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            if let Expr::Ref { name, .. } = operand.as_ref() {
                promoted.remove(name);
            }
            format!("{pad}{target_text} = {value_text};\n")
        }
        Stmt::ExprAssign { target, value, .. } => {
            let target_text = emit_expr(target, used_expr_helper, used_utf8_encode, promoted);
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            if let Expr::Ref { name, .. } = target {
                promoted.remove(name);
            }
            format!("{pad}{target_text} = {value_text};\n")
        }
        Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            // The condition always executes, in the enclosing flow — so it
            // renders (and any bang usage inside it promotes) against the
            // *ambient* `promoted`, not a scoped clone. `then_branch` and
            // `else_branch` might not, so each gets its own clone, extended
            // with whatever the condition itself proves for that branch,
            // and discarded once the branch is emitted (`emit_scoped_block`)
            // — except the one case handled below, after both branches:
            // `if (x == null) return;` (or any other unconditional exit)
            // proves `x` non-null for the rest of the enclosing block,
            // *because* falling past the whole `if` at all means the
            // condition was false.
            let condition_text = emit_expr(condition, used_expr_helper, used_utf8_encode, promoted);
            let mut source = format!("{pad}if ({condition_text}) {{\n");

            let mut then_extra = Vec::new();
            and_chain_null_check_names(condition, &mut then_extra);
            let mut then_promoted = promoted.clone();
            for name in then_extra {
                then_promoted.insert(name.to_owned());
            }
            source.push_str(&emit_scoped_block(
                then_branch,
                depth + 1,
                used_expr_helper,
                used_utf8_encode,
                then_promoted,
            ));

            if else_branch.is_empty() {
                source.push_str(&format!("{pad}}}\n"));
            } else {
                source.push_str(&format!("{pad}}} else {{\n"));
                let mut else_promoted = promoted.clone();
                if let Some(name) = ref_null_check_name(condition, BinaryOp::Eq) {
                    else_promoted.insert(name.to_owned());
                }
                source.push_str(&emit_scoped_block(
                    else_branch,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                    else_promoted,
                ));
                source.push_str(&format!("{pad}}}\n"));
            }

            // Anything either branch reassigns invalidates that name's
            // promotion for the rest of the enclosing block, regardless of
            // which branch actually ran — `collect_assigned_names`'s own
            // doc comment has the real Verovio regression this guards
            // against (an inner branch's reassignment silently surviving in
            // an outer scope's stale promotion). Order matters: the guard
            // case right after this re-establishes one specific name only
            // once the sweep has cleared it, since that reasoning holds
            // independent of what a *non-taken* `then_branch` might have
            // reassigned.
            let mut assigned = HashSet::new();
            collect_assigned_names(then_branch, &mut assigned);
            collect_assigned_names(else_branch, &mut assigned);
            for name in &assigned {
                promoted.remove(name);
            }

            if let Some(name) = ref_null_check_name(condition, BinaryOp::Eq)
                && branch_always_exits(then_branch)
            {
                promoted.insert(name.to_owned());
            }
            source
        }
        // A loop's own condition/increment run on every iteration, not just
        // the first — including whichever iteration is the last one before
        // falling out of the loop. Because of that back-edge, a promotion
        // established on one iteration (whether from a literal `!` or a
        // reassignment reachable earlier in the body) can't be trusted to
        // still hold on the next: the whole loop, condition and increment
        // included, renders against a single clone of `promoted` that's
        // simply dropped once the loop is emitted, never merged back into
        // the ambient set used after it. Some real reduction is left on the
        // table by being this conservative — an acceptable residual, not a
        // safety gap (see the plan doc's own criterion: never remove a `!`
        // Dart would still require). `loop_scoped_promoted` additionally
        // strips any name the body itself reassigns — needed even for the
        // very first, textually-before-the-reassignment use inside the body
        // (that function's own doc comment has the real regression).
        Stmt::DoWhile {
            body, condition, ..
        } => {
            let mut scoped = loop_scoped_promoted(promoted, body);
            let mut source = format!("{pad}do {{\n");
            for inner in body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                    &mut scoped,
                ));
            }
            let condition_text =
                emit_expr(condition, used_expr_helper, used_utf8_encode, &mut scoped);
            source.push_str(&format!("{pad}}} while ({condition_text});\n"));
            source
        }
        Stmt::ForEach {
            name,
            ty,
            is_final,
            write_back,
            iterable,
            body,
            ..
        } => {
            let iterable_text = emit_expr(iterable, used_expr_helper, used_utf8_encode, promoted);
            if *write_back {
                const ITERABLE_NAME: &str = "_syntaxBridgeIterable";
                const INDEX_NAME: &str = "_syntaxBridgeIndex";
                let mut source = format!("{pad}final {ITERABLE_NAME} = {iterable_text};\n");
                source.push_str(&format!(
                    "{pad}for (int {INDEX_NAME} = 0; {INDEX_NAME} < {ITERABLE_NAME}.length; ++{INDEX_NAME}) {{\n"
                ));
                source.push_str(&format!(
                    "{pad}{INDENT}{} {name} = {ITERABLE_NAME}[{INDEX_NAME}];\n",
                    emit_type(ty)
                ));
                source.push_str(&format!("{pad}{INDENT}try {{\n"));
                source.push_str(&emit_scoped_block(
                    body,
                    depth + 2,
                    used_expr_helper,
                    used_utf8_encode,
                    loop_scoped_promoted(promoted, body),
                ));
                source.push_str(&format!("{pad}{INDENT}}} finally {{\n"));
                source.push_str(&format!(
                    "{pad}{INDENT}{INDENT}{ITERABLE_NAME}[{INDEX_NAME}] = {name};\n"
                ));
                source.push_str(&format!("{pad}{INDENT}}}\n"));
                source.push_str(&format!("{pad}}}\n"));
                return source;
            }
            let binding = if *is_final { "final " } else { "" };
            let mut source = format!(
                "{pad}for ({binding}{} {name} in {iterable_text}) {{\n",
                foreach_binding_type_text(ty, iterable)
            );
            source.push_str(&emit_scoped_block(
                body,
                depth + 1,
                used_expr_helper,
                used_utf8_encode,
                loop_scoped_promoted(promoted, body),
            ));
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::While {
            condition, body, ..
        } => {
            let mut scoped = loop_scoped_promoted(promoted, body);
            let condition_text =
                emit_expr(condition, used_expr_helper, used_utf8_encode, &mut scoped);
            let mut source = format!("{pad}while ({condition_text}) {{\n");
            if let Some(name) = ref_null_check_name(condition, BinaryOp::Ne) {
                scoped.insert(name.to_owned());
            }
            for inner in body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                    &mut scoped,
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
            let mut scoped = loop_scoped_promoted(promoted, body);
            if let Some(stmt) = increment.as_deref() {
                let mut assigned = HashSet::new();
                collect_assigned_names(std::slice::from_ref(stmt), &mut assigned);
                for name in &assigned {
                    scoped.remove(name);
                }
            }
            let init_text = init
                .as_deref()
                .map(|stmt| emit_for_clause(stmt, used_expr_helper, used_utf8_encode, &mut scoped))
                .unwrap_or_default();
            let condition_text = condition
                .as_ref()
                .map(|expr| emit_expr(expr, used_expr_helper, used_utf8_encode, &mut scoped))
                .unwrap_or_default();
            let increment_text = increment
                .as_deref()
                .map(|stmt| emit_for_clause(stmt, used_expr_helper, used_utf8_encode, &mut scoped))
                .unwrap_or_default();
            let mut source =
                format!("{pad}for ({init_text}; {condition_text}; {increment_text}) {{\n");
            for inner in body {
                source.push_str(&emit_stmt(
                    inner,
                    depth + 1,
                    used_expr_helper,
                    used_utf8_encode,
                    &mut scoped,
                ));
            }
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::Break { .. } => format!("{pad}break;\n"),
        Stmt::Continue { .. } => format!("{pad}continue;\n"),
        Stmt::ContinueLabel { label, .. } => format!("{pad}continue {label};\n"),
        Stmt::ExprStmt { expr, .. } => format!(
            "{pad}{};\n",
            emit_expr(expr, used_expr_helper, used_utf8_encode, promoted)
        ),
        Stmt::Throw { value, .. } => format!(
            "{pad}throw {};\n",
            emit_expr(value, used_expr_helper, used_utf8_encode, promoted)
        ),
        // `try_body` might throw before finishing (that's the whole reason
        // it's a `try`), so anything it promotes can't be trusted in
        // `catch_body` — each gets its own clone of `promoted`, dropped once
        // rendered, same reasoning as an `if`'s branches.
        Stmt::TryCatch {
            try_body,
            catch_type,
            catch_var,
            catch_body,
            ..
        } => {
            let mut source = format!("{pad}try {{\n");
            source.push_str(&emit_scoped_block(
                try_body,
                depth + 1,
                used_expr_helper,
                used_utf8_encode,
                promoted.clone(),
            ));
            source.push_str(&format!(
                "{pad}}} on {} catch ({catch_var}) {{\n",
                emit_type(catch_type)
            ));
            source.push_str(&emit_scoped_block(
                catch_body,
                depth + 1,
                used_expr_helper,
                used_utf8_encode,
                promoted.clone(),
            ));
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            let mut source = format!("{pad}try {{\n");
            source.push_str(&emit_scoped_block(
                try_body,
                depth + 1,
                used_expr_helper,
                used_utf8_encode,
                promoted.clone(),
            ));
            source.push_str(&format!("{pad}}} finally {{\n"));
            source.push_str(&emit_scoped_block(
                finally_body,
                depth + 1,
                used_expr_helper,
                used_utf8_encode,
                promoted.clone(),
            ));
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
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            let mut source = format!("{pad}{{\n");
            source.push_str(&format!(
                "{pad}{INDENT}final {TUPLE_ASSIGN_TEMP} = {value_text};\n"
            ));
            for (index, target) in targets.iter().enumerate() {
                if is_tuple_assign_discard(target) {
                    continue;
                }
                let target_text = emit_expr(target, used_expr_helper, used_utf8_encode, promoted);
                source.push_str(&format!(
                    "{pad}{INDENT}{target_text} = {TUPLE_ASSIGN_TEMP}.${};\n",
                    index + 1
                ));
            }
            source.push_str(&format!("{pad}}}\n"));
            invalidate_ref_targets(targets, promoted);
            source
        }
        Stmt::TupleAssign { targets, value, .. } => {
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            // A single-element Dart record pattern needs a trailing comma
            // (`(a,) = expr;`) — see `emit_type`'s own `Type::Tuple` arm for
            // why: without it, `(a)` parses as a parenthesized expression,
            // not a destructuring assignment target.
            let targets_text = if targets.len() == 1 {
                format!(
                    "{},",
                    emit_expr(&targets[0], used_expr_helper, used_utf8_encode, promoted)
                )
            } else {
                targets
                    .iter()
                    .map(|target| emit_expr(target, used_expr_helper, used_utf8_encode, promoted))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            let source = format!("{pad}({targets_text}) = {value_text};\n");
            invalidate_ref_targets(targets, promoted);
            source
        }
        // `lower::cpp::lower_switch_stmt` already guarantees every case's
        // `body` ends in a jump (`break`/`continue`/`continue <label>;`/
        // `return`/`throw`) or is empty (pure label-stacking) — Dart's own
        // requirement for a non-empty `case`. A `case.label`, when present,
        // is what a preceding case's `Stmt::ContinueLabel` jumps into —
        // Dart's own explicit-fallthrough syntax — printed on its own line
        // right before the `case` line(s) it labels.
        Stmt::Switch {
            scrutinee,
            cases,
            default,
            ..
        } => {
            let scrutinee_text = emit_expr(scrutinee, used_expr_helper, used_utf8_encode, promoted);
            let mut source = format!("{pad}switch ({scrutinee_text}) {{\n");
            let case_pad = INDENT.repeat(depth + 1);
            for case in cases {
                if let Some(label) = &case.label {
                    source.push_str(&format!("{case_pad}{label}:\n"));
                }
                for value in &case.values {
                    let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
                    source.push_str(&format!("{case_pad}case {value_text}:\n"));
                }
                // A case that stacks labels shares a body with the case(s)
                // it falls through from (`SwitchCase::label`'s own doc
                // comment) — but each case still gets its own clone of
                // `promoted`, the same conservative-and-safe treatment every
                // other maybe-skipped block gets (`Stmt::TryCatch`'s doc
                // comment).
                source.push_str(&emit_scoped_block(
                    &case.body,
                    depth + 2,
                    used_expr_helper,
                    used_utf8_encode,
                    promoted.clone(),
                ));
            }
            if let Some(default) = default {
                source.push_str(&format!("{case_pad}default:\n"));
                source.push_str(&emit_scoped_block(
                    default,
                    depth + 2,
                    used_expr_helper,
                    used_utf8_encode,
                    promoted.clone(),
                ));
            }
            source.push_str(&format!("{pad}}}\n"));
            source
        }
        Stmt::Unsupported { reason, origin } => format!(
            "{pad}// TODO(syntax-bridge): {reason}\n{pad}throw UnimplementedError({message});\n",
            message = dart_string_literal(&unsupported_message(reason, origin)),
        ),
    }
}

/// A `for`-clause slot (init/increment) wants inline text with no trailing
/// `;`/newline of its own — `emit_stmt`'s shape doesn't fit. Only
/// `VarDecl`/`Assign`/`ExprAssign`/`ExprStmt` are real for-clause shapes from a C++
/// `ForStmt`; anything else falls back to the same `Never`-returning helper
/// `Expr::Unsupported` uses, kept syntactically valid rather than emitting
/// non-expression text into an expression slot.
fn emit_for_clause(
    stmt: &Stmt,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    promoted: &mut Promoted,
) -> String {
    match stmt {
        Stmt::VarDecl {
            name,
            ty,
            init: Some(expr),
            ..
        } => {
            let init_text = emit_expr(expr, used_expr_helper, used_utf8_encode, promoted);
            promoted.remove(name);
            format!("{} {name} = {init_text}", emit_type(ty))
        }
        Stmt::VarDecl {
            name,
            ty,
            init: None,
            ..
        } => {
            promoted.remove(name);
            format!("late {} {name}", emit_type(ty))
        }
        Stmt::Assign { name, value, .. } => {
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            promoted.remove(name);
            format!("{name} = {value_text}")
        }
        Stmt::ExprAssign {
            target: Expr::MapIndexOrInsert {
                target: map, index, ..
            },
            value,
            ..
        } => {
            let map_text = emit_receiver(map, used_expr_helper, used_utf8_encode, promoted);
            let bang = receiver_bang(map, promoted);
            let index_text = emit_expr(index, used_expr_helper, used_utf8_encode, promoted);
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            format!("{map_text}{bang}[{index_text}] = {value_text}")
        }
        // Same out-param dereference-assignment case `emit_stmt`'s
        // `Stmt::ExprAssign` handles — see its doc comment.
        Stmt::ExprAssign {
            target: Expr::Convert { operand, .. },
            value,
            ..
        } if operand.is_assignable_lvalue() => {
            let target_text = emit_expr(operand, used_expr_helper, used_utf8_encode, promoted);
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            if let Expr::Ref { name, .. } = operand.as_ref() {
                promoted.remove(name);
            }
            format!("{target_text} = {value_text}")
        }
        Stmt::ExprAssign { target, value, .. } => {
            let target_text = emit_expr(target, used_expr_helper, used_utf8_encode, promoted);
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            if let Expr::Ref { name, .. } = target {
                promoted.remove(name);
            }
            format!("{target_text} = {value_text}")
        }
        Stmt::ExprStmt { expr, .. } => {
            emit_expr(expr, used_expr_helper, used_utf8_encode, promoted)
        }
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
    expr.ty()
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

/// A `Stmt::TupleAssign` target that discards its own slot — round 20's
/// non-`void` out-param bridge (`lower::cpp::apply_out_param_bridge`):
/// when the original C++ return value itself is discarded at a call site
/// (`ParseDragAction(...);`, a bare statement ignoring the `bool` — legal
/// C++), the tuple's own leading slot has nothing to assign into. `_` in
/// Dart's *pattern*-destructuring position (`(_, x, y) = call();`) is a
/// real wildcard needing no declaration — but the temp-block fallback
/// below assigns each target with an *ordinary* statement
/// (`target = temp.$N;`), where a bare `_` would be an assignment to an
/// undeclared identifier, a real Dart error, not a wildcard. `lower::cpp`
/// marks a discard as `Expr::Ref { name: "_", .. }` (`Ref` rendering is
/// exactly its own name, so the pattern form needs nothing special); the
/// block form below checks for this marker explicitly and skips the line
/// entirely instead of assigning to it.
fn is_tuple_assign_discard(target: &Expr) -> bool {
    matches!(target, Expr::Ref { name, .. } if name == "_")
}

/// Invalidates the promotion of every bare-local `targets` entry — see
/// `receiver_bang`'s own doc comment on why reassignment always does.
/// `Stmt::TupleAssign`'s targets aren't necessarily locals (a `FieldAccess`/
/// `Index` target is never tracked in `promoted` to begin with, so removing
/// it here is a no-op), so this only has an effect on the `Expr::Ref` ones.
fn invalidate_ref_targets(targets: &[Expr], promoted: &mut Promoted) {
    for target in targets {
        if let Expr::Ref { name, .. } = target {
            promoted.remove(name);
        }
    }
}

/// Whether `Stmt::TupleAssign`'s ordinary record-pattern syntax
/// (`(targets...) = value;`) is unusable for this `targets` list — true
/// when any target is reached through a nullable receiver
/// (`FieldAccess`/`Index`, the only two `Expr` shapes with a receiver
/// `receiver_bang` can apply `!` to) and so would need that `!` printed
/// *inside* a pattern element, which Dart's pattern-assignment grammar
/// rejects (`dart format`: "Expected to find ')'" right at the `!`,
/// confirmed empirically against a real Verovio file — see this variant's
/// own doc comment on `emit_stmt`'s `Stmt::TupleAssign` arm) — or when any
/// target contains an `Index` anywhere in its chain at all, regardless of
/// nullability (F8/tarefa 10, real trigger `View::CalcOffsetBezier`'s
/// `CalcOffsetSpanningStartY(dc, points[0].y, spanningType)`, an
/// instance-method out-param call newly bridged once `lower::cpp::
/// call_out_param_arg_indices` stopped excluding ordinary instance methods):
/// Dart's pattern grammar has no production for a subscript expression as a
/// pattern element at all — `(points[0].y,) = call();` fails to parse
/// ("Expected to find ')'") *and* fails to type-check
/// (`pattern_type_mismatch_in_irrefutable_context`, misreading `points[0].y`
/// as `points` itself being destructured), confirmed empirically the same
/// way the nullable-receiver case above was. Every other `Expr` shape this
/// module can emit as an lvalue (`Ref`, a plain non-nullable `FieldAccess`
/// chain) parses fine as a pattern element on its own.
fn tuple_assign_needs_temp_block(targets: &[Expr]) -> bool {
    targets.iter().any(target_needs_tuple_assign_temp_block)
}

fn target_needs_tuple_assign_temp_block(target: &Expr) -> bool {
    match target {
        Expr::FieldAccess {
            target: receiver, ..
        } => !receiver_bang_by_type(receiver).is_empty() || target_contains_index(receiver),
        Expr::Index { .. } => true,
        _ => false,
    }
}

/// Whether `expr`'s own chain (following `FieldAccess`'s `target` field)
/// ever reaches an `Index` — see `target_needs_tuple_assign_temp_block`'s
/// own doc comment for why any `Index` anywhere in a `Stmt::TupleAssign`
/// target disqualifies the whole target from Dart's pattern-assignment
/// grammar, not just an `Index` at the target's own top level.
fn target_contains_index(expr: &Expr) -> bool {
    match expr {
        Expr::Index { .. } => true,
        Expr::FieldAccess { target, .. } => target_contains_index(target),
        _ => false,
    }
}

/// Names of local variables/parameters Dart's flow-sensitive type promotion
/// currently treats as non-null at the emission point reached so far — see
/// `receiver_bang`'s own doc comment for the conservative subset of Dart's
/// real promotion rules this tracks. Threaded the same way as
/// `used_expr_helper`/`used_utf8_encode`: sequentially, mutably, through
/// every statement and expression in a single straight-line scope. A nested,
/// possibly-not-taken scope (an `if` branch, a loop body, a `try`/`catch`
/// arm, a `switch` case) always gets a *clone*, extended if the scope's own
/// entry condition proves anything, and that clone is simply dropped once
/// the scope is done — nothing learned inside leaks back out, except the
/// one narrow case `Stmt::If`'s own handling merges back explicitly (an
/// unconditional-exit null guard, `if (x == null) return;`).
type Promoted = HashSet<String>;

/// Whether `receiver` needs a trailing `!` to read through, using only its
/// static type — ignores Dart's flow-sensitive promotion entirely. Used by
/// `tuple_assign_needs_temp_block`, which decides an emission *shape* before
/// the real receiver is emitted, so it has no `Promoted` state of its own to
/// consult or update. Conservatively assuming a statically-nullable receiver
/// always needs the temp-block shape — even where the real emission later
/// finds the `!` unnecessary — is safe on its own terms: it can only produce
/// more verbose Dart than strictly needed, never wrong Dart.
fn receiver_bang_by_type(receiver: &Expr) -> &'static str {
    if matches!(expr_ty(receiver), Some(Type::Nullable(_))) {
        "!"
    } else {
        ""
    }
}

/// Whether `receiver` needs a trailing `!` to read through, given both its
/// static type and Dart's flow-sensitive type promotion. A `T*`-derived
/// receiver is always `Type::Nullable(T)`, and the static type alone can't
/// tell whether the C++ source ever checked for null — asserting with `!` is
/// always *correct* there (see this function's module-level doc comment on
/// why C++ never required that check), but repeating it after Dart's own
/// analyzer has already proved the receiver non-null earns
/// `unnecessary_non_null_assertion` (real Verovio 6.2.0 evidence: 6107
/// occurrences across 77 files — `docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`).
/// Only `Expr::Ref` — a bare local or parameter — is ever tracked in
/// `promoted`: Dart never promotes a field access (`this._m_x`, `obj.field`)
/// or a call result, since other code could reassign a field between the
/// check and the use, so every other receiver shape keeps consulting only
/// the static type via `receiver_bang_by_type`.
fn receiver_bang(receiver: &Expr, promoted: &mut Promoted) -> &'static str {
    if let Expr::Ref { name, ty, .. } = receiver {
        if matches!(ty, Type::Nullable(_)) {
            return if promoted.insert(name.clone()) {
                "!"
            } else {
                ""
            };
        }
        return "";
    }
    receiver_bang_by_type(receiver)
}

/// Whether `receiver`, printed right before `.field`/`[index]`/`(args)`/
/// `.putIfAbsent(...)` glues on, needs parentheses first. `Expr::As`/
/// `Expr::Assign` already self-parenthesize unconditionally at their own
/// `emit_expr` arms (see each one's own doc comment), so they need nothing
/// extra here. Every other composite shape this checks prints with lower
/// precedence than postfix member access, so gluing that postfix syntax on
/// bare reassociates it onto only part of the receiver instead of its
/// whole result: `sparent is Layer ? sparent : null!.GetCurrentClef()`
/// calls a method on the bare `null` literal, not the ternary's own result
/// (F14/tarefa 14's caso 2, a real Verovio `null_check_always_fails` +
/// `receiver_of_type_never` regression — `editortoolkit_neume.dart:1835`,
/// from `lower::cpp::lower_dynamic_cast_expr`'s synthesized `x is T ? x :
/// null`). Postfix member access binds tighter than every one of these
/// shapes, `Comparison` (`!=`) included, so — unlike `binary_child_needs_
/// parens`'s own `Expr::Convert` arm — there's no precedence-relative case
/// to spare here: any non-`Simple` `ConvertShape` always needs wrapping
/// (`convert_shape`'s own doc comment on why `Convert` doesn't carry this
/// as its own IR tag for this `match` to see directly).
fn receiver_needs_parens(receiver: &Expr) -> bool {
    matches!(
        receiver,
        Expr::Binary { .. } | Expr::Unary { .. } | Expr::Conditional { .. } | Expr::Is { .. }
    ) || !matches!(convert_shape(receiver), ConvertShape::Simple)
}

/// Renders `receiver`, wrapped in parens first when `receiver_needs_parens`
/// says so — every call site that glues `.field`/`[index]`/`(args)`/
/// `.putIfAbsent(...)` onto a receiver's text uses this instead of calling
/// `emit_expr` on it directly, so none of them can individually forget the
/// check (F14/tarefa 14).
fn emit_receiver(
    receiver: &Expr,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    promoted: &mut Promoted,
) -> String {
    let text = emit_expr(receiver, used_expr_helper, used_utf8_encode, promoted);
    if receiver_needs_parens(receiver) {
        format!("({text})")
    } else {
        text
    }
}

/// If `expr` is `x != null`/`null != x` (when `op` is `BinaryOp::Ne`) or
/// `x == null`/`null == x` (when `op` is `BinaryOp::Eq`) for some
/// local/parameter `x`, its name — the shape Dart's flow analysis treats as
/// a promotion witness. Doesn't look inside `&&`/`||` itself; see
/// `and_chain_null_check_names` for that.
fn ref_null_check_name(expr: &Expr, op: BinaryOp) -> Option<&str> {
    let Expr::Binary {
        op: found_op,
        lhs,
        rhs,
        ..
    } = expr
    else {
        return None;
    };
    if *found_op != op {
        return None;
    }
    match (lhs.as_ref(), rhs.as_ref()) {
        (Expr::Ref { name, .. }, Expr::NullLiteral { .. }) => Some(name.as_str()),
        (Expr::NullLiteral { .. }, Expr::Ref { name, .. }) => Some(name.as_str()),
        _ => None,
    }
}

/// Every name proven non-null by `expr` being `true`, when `expr` is a
/// (possibly nested) `&&` chain — the conservative subset of Dart's
/// promotion this module tracks for `&&`: each `x != null` conjunct, at any
/// position in the chain, promotes `x` for every conjunct to its right
/// (`emit_and_rhs`) and for the branch the whole chain guards (`Stmt::If`'s
/// `then_branch`). A non-`&&`, non-null-check conjunct (a plain call, say)
/// contributes nothing but doesn't stop the walk — nor does `expr` not being
/// an `&&` chain at all, so a bare `if (x != null)` is handled by the same
/// call.
fn and_chain_null_check_names<'a>(expr: &'a Expr, out: &mut Vec<&'a str>) {
    if let Expr::Binary {
        op: BinaryOp::And,
        lhs,
        rhs,
        ..
    } = expr
    {
        and_chain_null_check_names(lhs, out);
        and_chain_null_check_names(rhs, out);
        return;
    }
    if let Some(name) = ref_null_check_name(expr, BinaryOp::Ne) {
        out.push(name);
    }
}

/// Whether every path through `stmts` (as far as this module's conservative
/// analysis goes: just its last statement) leaves the enclosing block
/// through a jump rather than falling off the end — the shape
/// `Stmt::If`'s handling needs to know a `x == null` guard's `then_branch`
/// unconditionally exits before it can promote `x` for the rest of the
/// enclosing block (`if (x == null) return;`, real Verovio 6.2.0 evidence in
/// `docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`).
fn branch_always_exits(stmts: &[Stmt]) -> bool {
    matches!(
        stmts.last(),
        Some(
            Stmt::Return { .. }
                | Stmt::Throw { .. }
                | Stmt::Break { .. }
                | Stmt::Continue { .. }
                | Stmt::ContinueLabel { .. }
        )
    )
}

/// Emits `stmts` as a nested scope that might not run (an `if` branch, a
/// loop body, a `try`/`catch` arm, a `switch` case): `promoted` is *owned*,
/// not borrowed, specifically so nothing this scope learns — a bang usage,
/// a reassignment — can leak back into the caller's own `Promoted` once this
/// function returns and the local copy is dropped. The caller clones (and,
/// for `Stmt::If`'s branches, extends) its own `Promoted` to build the value
/// passed in.
fn emit_scoped_block(
    stmts: &[Stmt],
    depth: usize,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    mut promoted: Promoted,
) -> String {
    let mut source = String::new();
    for stmt in stmts {
        source.push_str(&emit_stmt(
            stmt,
            depth,
            used_expr_helper,
            used_utf8_encode,
            &mut promoted,
        ));
    }
    source
}

/// `expr`'s name, if it's the bare-`Ref` shape an assignment target
/// invalidates a promotion for — see `receiver_bang`'s own doc comment on
/// why reassignment always does. Shared between `emit_stmt`'s own
/// invalidation (a same-level `Stmt::Assign`/`Stmt::ExprAssign`) and
/// `collect_assigned_names` below (an assignment nested inside a maybe-
/// skipped block).
fn assign_target_name(target: &Expr) -> Option<&str> {
    match target {
        Expr::Ref { name, .. } => Some(name),
        _ => None,
    }
}

/// A fresh clone of `promoted` for rendering a loop's condition/increment/
/// body, with any name `body` itself reassigns already stripped out — even
/// for a use textually *before* that reassignment. A loop's own back-edge
/// means the top of the body has two incoming paths: straight from before
/// the loop (where a name might be legitimately promoted) and looping back
/// from the bottom of the previous iteration (where that same name might
/// already have been reassigned to something not proven non-null). Dart's
/// own analyzer resolves that by never trusting a promotion for a name the
/// loop body assigns anywhere, for the whole body, not just from the
/// assignment onward — real Verovio 6.2.0 regression:
/// `docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`'s own evidence
/// trail — `HumdrumToken? token = endtoken; int tcount =
/// token!.getPreviousTokenCount();` correctly promotes `token` before a
/// `while` loop, but the loop body both uses `token` early (inside a nested
/// `for`) and reassigns it later (`token = token.getPreviousToken(0);`) —
/// `dart analyze` still flags that *early* use as needing its own `!`, even
/// on what textually looks like the first, not-yet-reassigned use.
fn loop_scoped_promoted(promoted: &Promoted, body: &[Stmt]) -> Promoted {
    let mut assigned = HashSet::new();
    collect_assigned_names(body, &mut assigned);
    let mut scoped = promoted.clone();
    for name in &assigned {
        scoped.remove(name);
    }
    scoped
}

/// Every local/parameter name `stmts` assigns to, anywhere inside it —
/// recursing into every nested block (`if`, loop, `try`, `switch`), not
/// just this list's own top level. `Stmt::If`'s handling uses this on both
/// of its branches to invalidate, in the *ambient* `Promoted` used for code
/// after the whole `if`, any name either branch might have reassigned: real
/// Verovio 6.2.0 regression (`docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`)
/// — `staff` promoted by an early `staff!.m_drawingStaffSize`, then
/// reassigned by `staff = slur!.CalculatePrincipalStaff(...)` two `if`
/// levels deeper, then read again as a bare `staff.GetN()` right after —
/// `emit_scoped_block`'s own "nothing leaks back" design correctly drops
/// that inner scope's *own* copy of the promotion, but nothing was
/// invalidating the *outer* scope's copy, which had promoted `staff` before
/// ever seeing the reassignment. A conservative over-approximation (an
/// assignment on a path that isn't actually reachable here) only costs an
/// unnecessary `!`, never an unsafely dropped one.
fn collect_assigned_names(stmts: &[Stmt], out: &mut HashSet<String>) {
    for stmt in stmts {
        match stmt {
            Stmt::Assign { name, .. } => {
                out.insert(name.clone());
            }
            Stmt::ExprAssign { target, .. } => {
                let name = assign_target_name(target).or_else(|| match target {
                    Expr::Convert { operand, .. } => assign_target_name(operand),
                    _ => None,
                });
                if let Some(name) = name {
                    out.insert(name.to_owned());
                }
            }
            Stmt::TupleAssign { targets, .. } => {
                for target in targets {
                    if let Some(name) = assign_target_name(target) {
                        out.insert(name.to_owned());
                    }
                }
            }
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                collect_assigned_names(then_branch, out);
                collect_assigned_names(else_branch, out);
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::ForEach { body, .. } => {
                collect_assigned_names(body, out);
            }
            Stmt::For {
                init,
                increment,
                body,
                ..
            } => {
                if let Some(stmt) = init.as_deref() {
                    collect_assigned_names(std::slice::from_ref(stmt), out);
                }
                if let Some(stmt) = increment.as_deref() {
                    collect_assigned_names(std::slice::from_ref(stmt), out);
                }
                collect_assigned_names(body, out);
            }
            Stmt::TryCatch {
                try_body,
                catch_body,
                ..
            } => {
                collect_assigned_names(try_body, out);
                collect_assigned_names(catch_body, out);
            }
            Stmt::TryFinally {
                try_body,
                finally_body,
                ..
            } => {
                collect_assigned_names(try_body, out);
                collect_assigned_names(finally_body, out);
            }
            Stmt::Switch { cases, default, .. } => {
                for case in cases {
                    collect_assigned_names(&case.body, out);
                }
                if let Some(default) = default {
                    collect_assigned_names(default, out);
                }
            }
            Stmt::Return { .. }
            | Stmt::VarDecl { .. }
            | Stmt::FieldAssign { .. }
            | Stmt::Break { .. }
            | Stmt::Continue { .. }
            | Stmt::ContinueLabel { .. }
            | Stmt::ExprStmt { .. }
            | Stmt::Throw { .. }
            | Stmt::Unsupported { .. } => {}
        }
    }
}

/// Whether `operand` needs wrapping parentheses before chaining a postfix
/// suffix (`.toInt()`, `.toDouble()`, `.value`, `!`).
///
/// In Dart, postfix call / member access binds tighter than binary operators,
/// conditionals, assignments, and unary operators. Any operand whose outermost
/// syntactic operator has lower precedence must be parenthesized so that the
/// postfix operator applies to the whole expression rather than its rightmost
/// token (e.g. `(a * 0.5 + b).toInt()` rather than `a * 0.5 + b.toInt()`).
fn convert_operand_needs_parens(operand: &Expr) -> bool {
    match operand {
        Expr::Binary { .. }
        | Expr::Conditional { .. }
        | Expr::Unary { .. }
        | Expr::Assign { .. }
        | Expr::Is { .. }
        | Expr::As { .. } => true,
        Expr::Convert { ty, operand, .. } => match ty {
            Type::Bool => true,
            Type::Int if matches!(expr_ty(operand), Some(Type::Bool)) => true,
            _ => false,
        },
        _ => false,
    }
}

/// The syntactic shape an `Expr::Convert` node actually renders as —
/// `Convert` doesn't carry this in its own IR tag, so callers that need to
/// know the shape without re-rendering the text (F14/tarefa 14:
/// `binary_child_needs_parens`, the `Expr::Unary` arm's operand check, and
/// `receiver_needs_parens`) call `convert_shape` instead of duplicating
/// `emit_expr`'s own `Expr::Convert` match. The two variants beyond
/// `Simple` need different treatment: a ternary has the lowest precedence
/// of anything this module ever prints, so it always needs wrapping the
/// same as a literal `Expr::Conditional`, unconditionally — but a `!=`
/// comparison is precedence-sensitive exactly like a literal
/// `Expr::Binary{op: Ne, ..}` child, so treating it as *always* needing
/// wrapping is itself a bug: it over-wraps a safe case like `x != null &&
/// y` (`!=` already binds tighter than `&&`) into `(x != null) && y` —
/// harmless there, but it broke `an_or_nested_inside_an_and_is_
/// parenthesized_so_dart_reads_the_same_tree_this_module_does`'s *exact*
/// expected grouping the first time this shipped, since over-wrapping
/// changes the printed text even where it doesn't change the parse.
enum ConvertShape {
    /// A plain postfix chain (`.toInt()`, `.toDouble()`, `.value`, `!`, or a
    /// pass-through) — always safe to embed unwrapped, in every position
    /// this module ever puts it.
    Simple,
    /// `x ? 1 : 0` (`Bool`→`Int`) — `docs/prompts/2026-08-21-14-
    /// parentizacao-e-precedencia.md`'s caso 1: glued bare as a binary
    /// operand, Dart's own low ternary precedence reassociates the whole
    /// expression around it.
    Ternary,
    /// `x != y` (`_`→`Bool`).
    Comparison,
}

fn convert_shape(expr: &Expr) -> ConvertShape {
    let Expr::Convert { operand, ty, .. } = expr else {
        return ConvertShape::Simple;
    };
    if matches!(expr_ty(operand), Some(Type::Nullable(inner)) if inner.as_ref() == ty)
        || (matches!(expr_ty(operand), Some(Type::Nullable(_)))
            && !matches!(
                ty,
                Type::Double | Type::Int | Type::Bool | Type::Nullable(_)
            ))
    {
        return ConvertShape::Simple;
    }
    match ty {
        Type::Int if matches!(expr_ty(operand), Some(Type::Bool)) => ConvertShape::Ternary,
        Type::Bool => ConvertShape::Comparison,
        Type::Nullable(_) => convert_shape(operand),
        _ => ConvertShape::Simple,
    }
}

/// Renders `operand` for an `Expr::Convert` branch that chains a postfix
/// suffix directly onto it (`.toInt()`, `.toDouble()`, `.value`, `!`) —
/// parenthesized when `operand` has lower precedence than postfix member
/// access (binary arithmetic, ternary, unary increment/negation, etc.).
fn emit_convert_operand(
    operand: &Expr,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    promoted: &mut Promoted,
) -> String {
    let text = emit_expr(operand, used_expr_helper, used_utf8_encode, promoted);
    if convert_operand_needs_parens(operand) {
        format!("({text})")
    } else {
        text
    }
}

/// Emits a short-circuit operator's (`&&`/`||`) right operand against a
/// *clone* of `promoted`, discarded once `rhs` is rendered — never the
/// caller's own `promoted` in place. Two separate reasons this has to be a
/// throwaway clone, not a widen-in-place:
///
/// - `rhs` might not run at all: `&&` skips it once `lhs` is `false`, `||`
///   skips it once `lhs` is `true`. A bang inside `rhs` only proves anything
///   in the world where `rhs` actually executed — real Verovio regression
///   (`docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`'s own
///   evidence trail): `if (!a && !(positioner!.GetObject()!.…)) { continue; }`
///   followed by an unguarded `positioner.GetDrawingPlace()` a few lines
///   later — the whole `&&` is reached, but its right operand, where
///   `positioner!` sits, only runs when `!a` is `true`; a first emitter
///   draft merged that operand's own bang usage back into the ambient set
///   regardless, so `positioner` looked promoted even along the path where
///   `rhs` never ran, producing `unchecked_use_of_nullable_value`.
/// - what *is* provably true from `lhs` alone (`and_chain_null_check_names`,
///   `&&` only) is exactly what `Stmt::If`'s own handling of `then_branch`
///   already seeds separately from the condition's structure — that's the
///   one legitimate way a `&&`'s left operand promotes anything beyond its
///   own right operand, and it doesn't need this function's help.
fn emit_short_circuit_rhs(
    lhs: &Expr,
    rhs: &Expr,
    op: BinaryOp,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    promoted: &Promoted,
) -> String {
    let mut scoped = promoted.clone();
    if op == BinaryOp::And {
        let mut extra = Vec::new();
        and_chain_null_check_names(lhs, &mut extra);
        for name in extra {
            scoped.insert(name.to_owned());
        }
    }
    emit_expr(rhs, used_expr_helper, used_utf8_encode, &mut scoped)
}

fn emit_expr(
    expr: &Expr,
    used_expr_helper: &mut bool,
    used_utf8_encode: &mut bool,
    promoted: &mut Promoted,
) -> String {
    match expr {
        Expr::IntLiteral { value, .. } => value.to_string(),
        Expr::DoubleLiteral { value, .. } => value.to_string(),
        Expr::BoolLiteral { value, .. } => value.to_string(),
        Expr::NullLiteral { .. } => "null".to_owned(),
        Expr::StringLiteral { value, .. } => dart_string_literal(value),
        Expr::Ref { name, .. } => name.clone(),
        Expr::Binary {
            op, lhs, rhs, ty, ..
        } => {
            let lhs_text = emit_expr(lhs, used_expr_helper, used_utf8_encode, promoted);
            let rhs_text = if matches!(op, BinaryOp::And | BinaryOp::Or) {
                emit_short_circuit_rhs(lhs, rhs, *op, used_expr_helper, used_utf8_encode, promoted)
            } else {
                emit_expr(rhs, used_expr_helper, used_utf8_encode, promoted)
            };
            // A lower- (or, on the right, equal-) precedence child printed
            // bare would silently reassociate — see `binary_child_needs_parens`'s
            // own doc comment for why this is a real correctness bug on its
            // own, not just a nullability concern.
            let lhs_text = if binary_child_needs_parens(*op, lhs, false) {
                format!("({lhs_text})")
            } else {
                lhs_text
            };
            let rhs_text = if binary_child_needs_parens(*op, rhs, true) {
                format!("({rhs_text})")
            } else {
                rhs_text
            };
            format!("{lhs_text} {} {rhs_text}", emit_binary_op(*op, ty))
        }
        // `then_expr`/`else_expr` are mutually exclusive, exactly like
        // `Stmt::If`'s two branches (see that arm's own doc comment) — each
        // renders against its own clone of `promoted`, extended with
        // whatever `condition` proves for that side, and neither clone is
        // merged back: a bang inside `then_expr` must not promote a name
        // for `else_expr`'s evaluation, nor for whatever follows the whole
        // ternary.
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ..
        } => {
            let condition_text = emit_expr(condition, used_expr_helper, used_utf8_encode, promoted);
            let mut then_extra = Vec::new();
            and_chain_null_check_names(condition, &mut then_extra);
            let mut then_promoted = promoted.clone();
            for name in then_extra {
                then_promoted.insert(name.to_owned());
            }
            let then_text = emit_expr(
                then_expr,
                used_expr_helper,
                used_utf8_encode,
                &mut then_promoted,
            );
            let mut else_promoted = promoted.clone();
            if let Some(name) = ref_null_check_name(condition, BinaryOp::Eq) {
                else_promoted.insert(name.to_owned());
            }
            let else_text = emit_expr(
                else_expr,
                used_expr_helper,
                used_utf8_encode,
                &mut else_promoted,
            );
            format!("{condition_text} ? {then_text} : {else_text}")
        }
        Expr::Unary { op, operand, .. } => {
            let op_text = emit_unary_op(*op);
            let operand_text = emit_expr(operand, used_expr_helper, used_utf8_encode, promoted);
            if matches!(op, UnaryOp::PostIncrement | UnaryOp::PostDecrement) {
                return format!("{operand_text}{op_text}");
            }
            // Two different reasons collapse to the same "wrap it in
            // parens" output here. `Not`'s operand can be an arbitrary
            // lower-precedence expression (`return !(node != null);`) —
            // `!` binds tighter than Dart's own `!=`/`&&`/`||`/ternary, so
            // an unparenthesized `!node != null` would parse as `(!node)
            // != null`, a different truth value entirely; always
            // parenthesizing sidesteps having to reason about every
            // operand shape's precedence individually. A nested unary
            // minus (Verovio's `-VRV_UNSET`, a macro expanding to
            // `(-2147483647)`) needs the same wrapping for an unrelated,
            // purely lexical reason: printed bare it would read
            // `--2147483647` — two adjacent `-` characters with nothing
            // between them merge into Dart's prefix-decrement token, which
            // can't apply to a literal (`dart format`: "Missing selector",
            // confirmed empirically). Checking whether the operand's own
            // text starts with the same character catches that regardless
            // of how deep the nesting goes. A third, independent reason
            // (F14/tarefa 14): a `Neg`/`PreIncrement`/`PreDecrement`
            // operand that's itself `Binary`/`Conditional`/a synthesized
            // ternary or comparison (`convert_shape`) needs the same
            // protection `Not` already always gets — prefix unary binds
            // tighter than every one of those shapes (`Comparison`
            // included, unlike `binary_child_needs_parens`'s own
            // precedence-relative treatment of it), so there's no
            // precedence-relative case to spare here either. Printed bare,
            // `-(a + b)` reads as `-a + b`, silently negating only `a`
            // (confirmed empirically: `dart analyze` never flags this,
            // since both expressions type-check).
            if *op == UnaryOp::Not
                || operand_text.starts_with(op_text)
                || matches!(
                    operand.as_ref(),
                    Expr::Binary { .. } | Expr::Conditional { .. }
                )
                || !matches!(convert_shape(operand), ConvertShape::Simple)
            {
                format!("{op_text}({operand_text})")
            } else {
                format!("{op_text}{operand_text}")
            }
        }
        Expr::Convert { operand, ty, .. } => {
            if matches!(expr_ty(operand), Some(Type::Nullable(inner)) if inner.as_ref() == ty)
                || (matches!(expr_ty(operand), Some(Type::Nullable(_)))
                    && !matches!(
                        ty,
                        Type::Double | Type::Int | Type::Bool | Type::Nullable(_)
                    ))
            {
                format!(
                    "{}!",
                    emit_convert_operand(operand, used_expr_helper, used_utf8_encode, promoted)
                )
            } else {
                match ty {
                    Type::Double => format!(
                        "{}.toDouble()",
                        emit_convert_operand(operand, used_expr_helper, used_utf8_encode, promoted)
                    ),
                    // `bool` → `int` (C++ implicitly reads a `bool` as `1`/`0`
                    // wherever an integer is expected), `enum` → `int` (the
                    // enumerator's real C++ value — `ir::Enum::values`'s doc
                    // comment on why this is `.value`, never Dart's `.index`), and
                    // the narrowing `double` → `int` (truncates toward zero, same
                    // direction as `.toInt()`) — `lower::cpp`'s three
                    // `child_ty`/`outer_ty` arms that construct this with
                    // `ty: Type::Int`.
                    // The synthesized ternary's own *condition* slot: Dart's
                    // grammar puts a bare `Expr::Conditional` operand there
                    // out of bounds entirely (a ternary's condition must be
                    // `ifNullExpression`, one grammar tier above another
                    // ternary) — same wrapping need as any other embedded
                    // ternary-shaped child (F14/tarefa 14), just caught as a
                    // hard syntax error here instead of a silent
                    // reassociation.
                    Type::Int if matches!(expr_ty(operand), Some(Type::Bool)) => {
                        let operand_text =
                            emit_expr(operand, used_expr_helper, used_utf8_encode, promoted);
                        let operand_text = if matches!(operand.as_ref(), Expr::Conditional { .. })
                            || matches!(convert_shape(operand), ConvertShape::Ternary)
                        {
                            format!("({operand_text})")
                        } else {
                            operand_text
                        };
                        format!("{operand_text} ? 1 : 0")
                    }
                    Type::Int if matches!(expr_ty(operand), Some(Type::Enum { .. })) => format!(
                        "{}.value",
                        emit_convert_operand(operand, used_expr_helper, used_utf8_encode, promoted)
                    ),
                    Type::Int => format!(
                        "{}.toInt()",
                        emit_convert_operand(operand, used_expr_helper, used_utf8_encode, promoted)
                    ),
                    // This synthesizes exactly the same `!=` shape a
                    // literal `Expr::Binary{op: Ne, ..}` node would, with
                    // `operand` standing in as its left operand — so it
                    // needs the identical left-operand wrapping decision
                    // `binary_child_needs_parens` already makes for a real
                    // one (F14/tarefa 14: a dynamic_cast ternary used as a
                    // bare truthy pointer check, `if (dynamic_cast<Chord*>
                    // (element))`, lowers to exactly this branch —
                    // unwrapped, `element is Chord ? element : null != 0`'s
                    // own ternary swallows the `!= 0` into its *else*
                    // branch instead of comparing the whole cast result,
                    // confirmed on the real Verovio 6.2.0 corpus:
                    // `view.dart`'s `non_bool_condition`).
                    Type::Bool => {
                        let operand_text =
                            emit_expr(operand, used_expr_helper, used_utf8_encode, promoted);
                        let operand_text =
                            if binary_child_needs_parens(BinaryOp::Ne, operand, false) {
                                format!("({operand_text})")
                            } else {
                                operand_text
                            };
                        format!(
                            "{operand_text} != {}",
                            if matches!(expr_ty(operand), Some(Type::Nullable(_))) {
                                "null"
                            } else {
                                "0"
                            }
                        )
                    }
                    // C++ address-of widens a known Dart reference `T` to its
                    // nullable pointer representation `T?`; Dart performs that
                    // widening implicitly. A dereference goes the other way and
                    // needs the explicit assertion that mirrors C++'s own unchecked
                    // pointer access.
                    Type::Nullable(_) => {
                        emit_expr(operand, used_expr_helper, used_utf8_encode, promoted)
                    }
                    _ if matches!(operand.as_ref(), Expr::This { .. }) => {
                        emit_expr(operand, used_expr_helper, used_utf8_encode, promoted)
                    }
                    _ if matches!(expr_ty(operand), Some(Type::Nullable(_))) => format!(
                        "{}!",
                        emit_convert_operand(operand, used_expr_helper, used_utf8_encode, promoted)
                    ),
                    _ => unreachable!(
                        "only represented scalar and nullable-reference conversions construct \
                         Expr::Convert, got ty={ty:?} operand={operand:?} at {:?}",
                        expr.origin()
                    ),
                }
            }
        }
        Expr::Call {
            target,
            base_qualifier,
            callee_name,
            args,
            ..
        } => {
            // `Expr::Call{target: None, callee_name: "utf8.encode"/"utf8.
            // decode", ..}` (round 21's string byte-index write —
            // `lower_string_byte_assign_stmt`) is the one place this
            // generic renderer emits a raw `dart:convert` reference
            // outside the dedicated `Expr::StringByteAt`/
            // `StringByteLength`/`find` renderers, which each already set
            // this flag directly at their own call sites — this call has
            // no such dedicated renderer, so it has to set it here
            // instead, or the `import 'dart:convert';` this call needs
            // would silently never get added.
            if callee_name.starts_with("utf8.") {
                *used_utf8_encode = true;
            }
            // The receiver (when there is one) is evaluated — and, for
            // promotion purposes, dereferenced — before any argument, same
            // as Dart's own left-to-right evaluation of `receiver.method
            // (args)`. Computing `args_text` first would let a bang inside
            // an argument promote a name the receiver itself still needs
            // its own `!` for at this point, since Dart's analyzer hasn't
            // reached that argument yet when it checks the receiver —
            // producing `unchecked_use_of_nullable_value` in exactly the
            // real Verovio corpus's most common expression shape (real
            // regression caught by `just verovio-diagnosis` while
            // implementing `receiver_bang`'s promotion tracking).
            // `base_qualifier` (F12/tarefa 09) is set only once
            // `function_catalog::resolve_qualified_base_calls` has already
            // confirmed the base named in C++ is exactly the one Dart's own
            // mixin linearization reaches through `super` for this member —
            // any call that didn't confirm was already downgraded to an
            // `Expr::UnsupportedTyped` bailout before emission ever sees it,
            // so reaching here with `Some` always means `super.` is correct.
            let receiver_prefix = if base_qualifier.is_some() {
                "super.".to_owned()
            } else {
                match target.as_deref() {
                    None | Some(Expr::This { .. }) => String::new(),
                    Some(receiver) => {
                        let receiver_text =
                            emit_receiver(receiver, used_expr_helper, used_utf8_encode, promoted);
                        let bang = receiver_bang(receiver, promoted);
                        format!("{receiver_text}{bang}.")
                    }
                }
            };
            let args_text = args
                .iter()
                .map(|arg| emit_expr(arg, used_expr_helper, used_utf8_encode, promoted))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{receiver_prefix}{callee_name}({args_text})")
        }
        Expr::FieldAccess { target, field, .. } => match target.as_ref() {
            Expr::This { .. } => field.clone(),
            _ => {
                let target_text =
                    emit_receiver(target, used_expr_helper, used_utf8_encode, promoted);
                let bang = receiver_bang(target, promoted);
                format!("{target_text}{bang}.{field}")
            }
        },
        Expr::RecordConstruct {
            type_name, fields, ..
        } => {
            let args_text = fields
                .iter()
                .map(|(_name, value)| {
                    emit_expr(value, used_expr_helper, used_utf8_encode, promoted)
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{type_name}({args_text})")
        }
        // T2 (`docs/prompts/2026-08-23-02-copia-por-valor-sem-construtor-posicional.md`):
        // the by-value copy role. Every copy site funnels into the one
        // stable copy form the record declares for itself —
        // `T.syntaxBridgeCopyOf` (see `emit_record`) — instead of
        // improvising a positional-constructor call that only exists for
        // records *without* own constructors. `rewrite_non_copyable_record_copies`
        // has already replaced every copy of a record that cannot declare
        // that constructor with a typed bailout, so reaching here always
        // means the named constructor exists.
        Expr::RecordCopy {
            target, type_name, ..
        } => {
            let target_text = emit_expr(target, used_expr_helper, used_utf8_encode, promoted);
            format!("{type_name}.syntaxBridgeCopyOf({target_text})")
        }
        Expr::ConstructorCall {
            type_name,
            constructor_index,
            args,
            ..
        } => {
            let args_text = args
                .iter()
                .map(|arg| emit_expr(arg, used_expr_helper, used_utf8_encode, promoted))
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
        Expr::Index { target, index, .. } => {
            let target_text = emit_receiver(target, used_expr_helper, used_utf8_encode, promoted);
            let bang = receiver_bang(target, promoted);
            let index_text = emit_expr(index, used_expr_helper, used_utf8_encode, promoted);
            format!("{target_text}{bang}[{index_text}]")
        }
        Expr::MapIndexOrInsert {
            target,
            index,
            default_value,
            ..
        } => {
            let target_text = emit_receiver(target, used_expr_helper, used_utf8_encode, promoted);
            let bang = receiver_bang(target, promoted);
            let index_text = emit_expr(index, used_expr_helper, used_utf8_encode, promoted);
            // `default_value` renders inside a `() => ...` closure literal
            // (below) — it may run zero times (an existing key) or later
            // than this point, never inline with the rest of this
            // expression, so anything it promotes must not leak into
            // `promoted` past this call, same reasoning as a loop body or
            // an `if` branch.
            let default_text = emit_expr(
                default_value,
                used_expr_helper,
                used_utf8_encode,
                &mut promoted.clone(),
            );
            format!("{target_text}{bang}.putIfAbsent({index_text}, () => {default_text})")
        }
        Expr::StringByteLength { target, .. } => {
            *used_utf8_encode = true;
            format!(
                "utf8.encode({}).length",
                emit_expr(target, used_expr_helper, used_utf8_encode, promoted)
            )
        }
        Expr::StringByteIndexOf {
            target,
            needle,
            from,
            ..
        } => {
            let target_text = emit_expr(target, used_expr_helper, used_utf8_encode, promoted);
            let needle_text = emit_expr(needle, used_expr_helper, used_utf8_encode, promoted);
            let from_suffix = match from {
                Some(from_expr) => {
                    let from_text =
                        emit_expr(from_expr, used_expr_helper, used_utf8_encode, promoted);
                    format!(", {from_text}")
                }
                None => String::new(),
            };
            let is_byte = matches!(expr_ty(needle), Some(Type::Int))
                || matches!(needle.as_ref(), Expr::IntLiteral { .. });
            if is_byte {
                format!("{INDEX_OF_BYTE_HELPER_NAME}({target_text}, {needle_text}{from_suffix})")
            } else {
                format!("{INDEX_OF_BYTES_HELPER_NAME}({target_text}, {needle_text}{from_suffix})")
            }
        }
        Expr::StringByteAt { target, index, .. } => {
            *used_utf8_encode = true;
            format!(
                "utf8.encode({})[{}]",
                emit_expr(target, used_expr_helper, used_utf8_encode, promoted),
                emit_expr(index, used_expr_helper, used_utf8_encode, promoted)
            )
        }
        // A single-element Dart record needs a trailing comma
        // (`(a,)`) — see `emit_type`'s own `Type::Tuple` arm.
        Expr::Tuple { values, .. } if values.len() == 1 => {
            format!(
                "({},)",
                emit_expr(&values[0], used_expr_helper, used_utf8_encode, promoted)
            )
        }
        Expr::ListLiteral { items, ty, .. } => format!(
            "<{}>[{}]",
            emit_type(match ty {
                Type::List(element) => element,
                _ => ty,
            }),
            items
                .iter()
                .map(|item| emit_expr(item, used_expr_helper, used_utf8_encode, promoted))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::MapLiteral { entries, ty, .. } => {
            let (key_ty, value_ty) = match ty {
                Type::Map(key, value) => (key.as_ref(), value.as_ref()),
                _ => (ty, ty),
            };
            format!(
                "<{}, {}>{{{}}}",
                emit_type(key_ty),
                emit_type(value_ty),
                entries
                    .iter()
                    .map(|(key, value)| {
                        let key_text = emit_expr(key, used_expr_helper, used_utf8_encode, promoted);
                        let value_text =
                            emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
                        format!("{key_text}: {value_text}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        Expr::Is {
            operand,
            target_type,
            ..
        } => format!(
            "{} is {}",
            emit_expr(operand, used_expr_helper, used_utf8_encode, promoted),
            emit_type(target_type)
        ),
        // Always parenthesized, the same reasoning as `Expr::Assign` just
        // below: `as`'s own type operand can itself end in `?`, and this
        // result is exactly the shape `receiver_bang` targets (the cast
        // narrows a pointer, so it's overwhelmingly used as a receiver
        // right after) — an unparenthesized `x as T?!.field` doesn't parse
        // as "force-unwrap the cast, then access `.field`" the way `(x as
        // T?)!.field` does; Dart reads the bare form as a syntax error
        // (confirmed empirically: it broke `dart format` on 14 real
        // Verovio files before this was parenthesized). Every other
        // embedding context (a call argument, a binary operand, ...)
        // accepts the extra parens just as safely.
        Expr::As { operand, ty, .. } => format!(
            "({} as {})",
            emit_expr(operand, used_expr_helper, used_utf8_encode, promoted),
            emit_type(ty)
        ),
        // Always parenthesized: Dart's `=` has the same low precedence
        // C++'s does, so an unparenthesized `x = y != null` would parse as
        // `x = (y != null)`, not the intended `(x = y) != null`.
        Expr::Assign { target, value, .. } => {
            let target_text = emit_expr(target, used_expr_helper, used_utf8_encode, promoted);
            let value_text = emit_expr(value, used_expr_helper, used_utf8_encode, promoted);
            // Reassignment invalidates any promotion the target held — Dart
            // can no longer prove the new value is non-null, so a later
            // dereference needs its own `!` again.
            if let Expr::Ref { name, .. } = target.as_ref() {
                promoted.remove(name);
            }
            format!("({target_text} = {value_text})")
        }
        Expr::Tuple { values, .. } => format!(
            "({})",
            values
                .iter()
                .map(|value| emit_expr(value, used_expr_helper, used_utf8_encode, promoted))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Expr::Unsupported { reason, origin } => {
            *used_expr_helper = true;
            format!(
                "{UNSUPPORTED_HELPER_NAME}<{OPAQUE_TYPE_NAME}>({message})",
                message = dart_string_literal(&unsupported_message(reason, origin))
            )
        }
        Expr::UnsupportedTyped { reason, ty, origin } => {
            *used_expr_helper = true;
            format!(
                "{UNSUPPORTED_HELPER_NAME}<{}>({message})",
                emit_type(ty),
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
        BinaryOp::Or => "||",
        BinaryOp::ShiftLeft => "<<",
        BinaryOp::ShiftRight => ">>",
        BinaryOp::BitAnd => "&",
        BinaryOp::BitXor => "^",
        BinaryOp::BitOr => "|",
    }
}

/// Dart's own binary-operator precedence (higher binds tighter) — the
/// subset this module ever constructs. Needed to decide when a nested
/// `Expr::Binary` must be parenthesized to print with the same grouping the
/// IR tree actually has: printing a lower-precedence child bare next to a
/// higher-precedence parent silently reassociates it (`x && a || b` parses
/// as `(x && a) || b`, not the intended `x && (a || b)`) — a real,
/// nullability-independent correctness bug that also breaks the promotion
/// tracking's own soundness once one operand promotes a name (see the
/// `Expr::Binary` arm's own doc comment and
/// `docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`'s evidence
/// trail for the real Verovio regression this caused).
fn binary_precedence(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Or => 1,
        BinaryOp::And => 2,
        BinaryOp::BitOr => 3,
        BinaryOp::BitXor => 4,
        BinaryOp::BitAnd => 5,
        BinaryOp::Eq | BinaryOp::Ne => 6,
        BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => 7,
        BinaryOp::ShiftLeft | BinaryOp::ShiftRight => 8,
        BinaryOp::Add | BinaryOp::Sub => 9,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => 10,
    }
}

/// Whether `child`, printed as the left or right operand of a `Binary` node
/// with operator `outer_op`, needs parentheses to keep the same grouping the
/// IR tree actually has. A ternary always does (lower precedence than any
/// binary operator). A nested `Binary` needs them whenever it binds looser
/// than `outer_op` — or, on the right side, whenever it binds *equally
/// tight*: every one of these operators is left-associative in both Dart
/// and C++, so a same-precedence child can only be on the right if the
/// source had explicit parentheses putting it there, which the printed text
/// must therefore preserve (`a - (b - c)` is not `a - b - c`).
fn binary_child_needs_parens(outer_op: BinaryOp, child: &Expr, child_is_rhs: bool) -> bool {
    match child {
        Expr::Conditional { .. } => true,
        Expr::Binary { op: child_op, .. } => {
            let outer = binary_precedence(outer_op);
            let inner = binary_precedence(*child_op);
            if child_is_rhs {
                inner <= outer
            } else {
                inner < outer
            }
        }
        // `Expr::Convert` doesn't carry its rendered shape as an IR tag for
        // this `match` to see directly (`convert_shape`'s own doc comment).
        // A `Bool`→`Int` conversion renders a ternary — Dart's lowest
        // precedence of anything this module prints — so it always needs
        // wrapping unconditionally, the same as a literal
        // `Expr::Conditional` child (F14/tarefa 14's caso 1: a synthesized
        // `solo ? 1 : 0` glued bare as `==`'s left operand reassociated the
        // whole condition around it). A `_`→`Bool` conversion renders a
        // `!=` comparison, precedence-sensitive exactly like a literal
        // `Expr::Binary{op: Ne, ..}` child — treating it as unconditional
        // too over-wraps a safe case like `x != null && y` into `(x !=
        // null) && y` (real regression the first time this shipped: see
        // `convert_shape`'s own doc comment), so this reuses the exact same
        // precedence check the `Expr::Binary` arm above does, standing in
        // `BinaryOp::Ne` for the comparison's own (fixed) operator.
        Expr::Convert { .. } => match convert_shape(child) {
            ConvertShape::Ternary => true,
            ConvertShape::Comparison => {
                let outer = binary_precedence(outer_op);
                let inner = binary_precedence(BinaryOp::Ne);
                if child_is_rhs {
                    inner <= outer
                } else {
                    inner < outer
                }
            }
            ConvertShape::Simple => false,
        },
        _ => false,
    }
}

fn emit_unary_op(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Neg => "-",
        UnaryOp::Not => "!",
        UnaryOp::PreIncrement | UnaryOp::PostIncrement => "++",
        UnaryOp::PreDecrement | UnaryOp::PostDecrement => "--",
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
        .replace('$', "\\$")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
        .replace('\u{08}', "\\b")
        .replace('\u{0C}', "\\f");
    format!("'{escaped}'")
}
