//! Diagnostic test: how far is the transpiler (US-8/E01-E13) from handling a
//! real, unmodified C++ project end to end? Runs `function_catalog`'s IR
//! extraction and `emit::dart`'s emission over *every* compilation unit of
//! the real `test-resources/verovio-version-6.2.0.tar.gz` archive (298 TUs),
//! then reports coverage: how much of the emitted Dart is real translated
//! logic versus an `Unsupported` stub, and whether the result is even
//! syntactically valid Dart (`dart format`) or analyzable (`dart analyze`).
//!
//! Not run by default: `#[ignore]` (this is a research/assessment tool, not
//! a pass/fail correctness test — E01-E13's own corpus already covers that).
//! Deliberately skips `dart format`'s per-file formatting step
//! (`transpile::transpile` aborts the whole batch on the first file that
//! doesn't parse — exactly the kind of all-or-nothing signal this test
//! exists to avoid) and calls `emit::dart::emit_module` directly instead, so
//! every file's raw output is captured even if some don't parse.
//!
//! Run explicitly with:
//!   cargo test -p syntax-bridge-server --test verovio_6_2_0_transpile_diagnosis -- --ignored --nocapture

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::Value;
use syntax_bridge_server::emit::dart;
use syntax_bridge_server::function_catalog;
use syntax_bridge_server::ingest::{self, CreateProjectRequest};
use syntax_bridge_server::ir::{Expr, Function, Module, Origin, Param, Stmt, Type};

#[test]
#[ignore = "slow/diagnostic: transpiles every real Verovio 6.2.0 compilation unit and reports coverage"]
fn transpiling_the_real_verovio_6_2_0_project_reports_coverage() {
    let archive_path = repo_root().join("test-resources/verovio-version-6.2.0.tar.gz");
    assert!(
        archive_path.is_file(),
        "expected fixture at {archive_path:?}"
    );

    let workspace =
        TempWorkspace::new("verovio-6-2-0-transpile-diagnosis").expect("create temp workspace");

    eprintln!("[diagnosis] ingesting (cmake configure)...");
    let ingest_start = Instant::now();
    let project = ingest::create_project(CreateProjectRequest {
        name: "verovio-6-2-0".to_owned(),
        workspace_dir: workspace.path().join("projects"),
        archive_path,
    })
    .expect("ingest real Verovio 6.2.0 project");
    eprintln!(
        "[diagnosis] ingested {} compilation units in {:.1}s",
        project.compilation_units.len(),
        ingest_start.elapsed().as_secs_f64()
    );
    assert!(
        !project.compilation_units.is_empty(),
        "expected at least one compilation unit"
    );

    eprintln!("[diagnosis] extracting IR (libclang, real bodies)...");
    let extract_start = Instant::now();
    let catalog = function_catalog::extract_function_catalog(
        &project.compilation_units,
        &project.input_source_dir,
        None,
    )
    .expect("extract function catalog over the real project");
    eprintln!(
        "[diagnosis] extracted in {:.1}s: {} declarations, {} calls, {} ir functions, {} ir records",
        extract_start.elapsed().as_secs_f64(),
        catalog.declarations.len(),
        catalog.calls.len(),
        catalog.ir_functions.len(),
        catalog.ir_records.len()
    );

    let module = Module {
        functions: catalog.ir_functions,
        records: catalog.ir_records,
        enums: catalog.ir_enums,
    };
    let bailouts = collect_bailouts(&module);
    eprintln!(
        "[diagnosis] IR bailouts: {} unsupported type occurrences ({} distinct spellings), \
         {} unsupported expressions ({} distinct causes), {} unsupported statements ({} distinct causes)",
        bailouts.type_total(),
        bailouts.unsupported_types.len(),
        bailouts.expression_total(),
        bailouts.unsupported_expressions.len(),
        bailouts.statement_total(),
        bailouts.unsupported_statements.len(),
    );
    let files = dart::emit_module(&module);
    eprintln!("[diagnosis] emitted {} Dart files", files.len());

    // `dynamic` hides a missing source-to-target mapping from both users and
    // the Dart analyzer. Keep the real-project diagnostic as the broad
    // regression gate: every generated token must now be a concrete Dart
    // type or a named bridge such as `SyntaxBridgeOpaque`. String literals
    // are stripped first — a transpiled C++ string can legally contain the
    // plain English word "dynamic" (Verovio's `options.cpp` does, in a
    // `SetInfo(...)` help string), which isn't the Dart type this gate
    // exists to catch.
    let dynamic_files: Vec<&str> = files
        .iter()
        .filter_map(|(path, contents)| {
            contains_dart_token(&strip_dart_string_literals(contents), "dynamic")
                .then_some(path.as_str())
        })
        .collect();
    assert!(
        dynamic_files.is_empty(),
        "generated Dart must not contain `dynamic`; found it in {dynamic_files:?}"
    );

    // Coverage proxy: how much of the emitted text is an honest stub
    // (`_syntaxBridgeUnsupported<T>(...)` in expression position, `// TODO
    // (syntax-bridge):` in statement position — see emit::dart's module
    // docs) versus real translated code. Line-based, not a real parse — a
    // cheap, order-of-magnitude signal, not a precise metric.
    let mut total_lines: usize = 0;
    let mut unsupported_expr_lines: usize = 0;
    let mut unsupported_stmt_lines: usize = 0;
    for contents in files.values() {
        for line in contents.lines() {
            total_lines += 1;
            if line.contains("_syntaxBridgeUnsupported<") {
                unsupported_expr_lines += 1;
            }
            if line.contains("// TODO(syntax-bridge):") {
                unsupported_stmt_lines += 1;
            }
        }
    }
    eprintln!(
        "[diagnosis] {total_lines} lines emitted; {unsupported_expr_lines} lines contain an \
         unsupported *expression* stub; {unsupported_stmt_lines} lines are an unsupported \
         *statement* TODO"
    );

    // Write the raw (never `dart format`-piped) output to a real package
    // directory so `dart format`/`dart analyze` can be run on it as
    // external processes, exactly like a user's own toolchain would see it.
    let package_dir = workspace.path().join("dart-package");
    fs::create_dir_all(package_dir.join("lib")).expect("create package dir");
    fs::write(
        package_dir.join("pubspec.yaml"),
        "name: verovio\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n",
    )
    .expect("write pubspec.yaml");
    for (relative_path, contents) in &files {
        let path = package_dir.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create file's parent dir");
        }
        fs::write(&path, contents).expect("write emitted Dart file");
    }

    // Persistent copy for post-mortem inspection: `package_dir` lives under
    // `TempWorkspace`, deleted on drop at the end of this function, so once
    // the test finishes there is no way to grep/read the emitted Dart short
    // of rerunning the whole ~5min pipeline. `.diagnosis/` is already
    // gitignored and already used for the JSON/MD snapshot below.
    let persistent_package_dir = repo_root().join(".diagnosis/dart-package");
    let _ = fs::remove_dir_all(&persistent_package_dir);
    for (relative_path, contents) in &files {
        let path = persistent_package_dir.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create file's parent dir (persistent copy)");
        }
        fs::write(&path, contents).expect("write emitted Dart file (persistent copy)");
    }

    eprintln!("[diagnosis] checking which files are even syntactically valid Dart...");
    let mut unparseable_files = Vec::new();
    for relative_path in files.keys() {
        let path = package_dir.join(relative_path);
        let output = Command::new("dart")
            .arg("format")
            .arg("--output=none")
            .arg(&path)
            .output();
        match output {
            Ok(result) if !result.status.success() => {
                unparseable_files.push((
                    relative_path.clone(),
                    String::from_utf8_lossy(&result.stderr).into_owned(),
                ));
            }
            Ok(_) => {}
            Err(error) => {
                eprintln!("[diagnosis] could not run `dart format` at all: {error}");
                break;
            }
        }
    }
    eprintln!(
        "[diagnosis] {}/{} files are NOT syntactically valid Dart",
        unparseable_files.len(),
        files.len()
    );
    for (path, reason) in unparseable_files.iter().take(15) {
        eprintln!("  - {path}: {}", reason.lines().next().unwrap_or(reason));
    }
    eprintln!("[diagnosis] full stderr for every unparseable file:");
    for (path, reason) in unparseable_files.iter() {
        eprintln!("=== {path} ===\n{reason}");
    }
    eprintln!("[diagnosis] raw source of the first unparseable file (first 120 lines):");
    if let Some((path, _)) = unparseable_files.first() {
        let contents = files.get(path).expect("path came from files.keys()");
        for (index, line) in contents.lines().take(120).enumerate() {
            eprintln!("{:>4} | {line}", index + 1);
        }
    }

    eprintln!("[diagnosis] running `dart analyze --format=json` over the whole package...");
    let analyze_output = Command::new("dart")
        .arg("analyze")
        .arg("--format=json")
        .arg(&package_dir)
        .output();
    let (analyze_text, diagnostics) = match analyze_output {
        Ok(output) => {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let diags = parse_dart_analyze_json(&text).expect("parse dart analyze JSON");
            (text, diags)
        }
        Err(error) => {
            eprintln!("[diagnosis] could not run `dart analyze` at all: {error}");
            (String::new(), Vec::new())
        }
    };
    let error_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == "ERROR")
        .count();
    let warning_count = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.severity == "WARNING")
        .count();
    eprintln!(
        "[diagnosis] dart analyze: {error_count} errors, {warning_count} warnings ({} total \
         lines of output)",
        diagnostics.len()
    );

    // Taxonomy and concrete first occurrences of *why* `dart analyze`
    // rejects the package. JSON gives stable rule/path/line data, unlike the
    // human-oriented default output whose wording is not an interface.
    use std::collections::BTreeMap;
    let mut rule_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut rule_occurrences: BTreeMap<String, Vec<AnalyzerDiagnostic>> = BTreeMap::new();
    for diagnostic in &diagnostics {
        *rule_counts.entry(diagnostic.rule.clone()).or_insert(0) += 1;
        let occurrences = rule_occurrences.entry(diagnostic.rule.clone()).or_default();
        if occurrences.len() < 5 {
            occurrences.push(diagnostic.clone());
        }
    }
    let mut rule_counts: Vec<(String, usize)> = rule_counts.into_iter().collect();
    rule_counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    eprintln!("[diagnosis] top diagnostic rules:");
    for (rule, count) in rule_counts.iter().take(20) {
        eprintln!("  - {count:>5}  {rule}");
    }
    eprintln!("[diagnosis] first occurrences for top diagnostic rules:");
    for (rule, _) in rule_counts.iter().take(20) {
        for occurrence in rule_occurrences.get(rule).into_iter().flatten().take(5) {
            eprintln!(
                "  - [{rule}] {}:{}:{}: {}",
                occurrence.path, occurrence.line, occurrence.column, occurrence.message
            );
        }
    }

    eprintln!(
        "[diagnosis] summary: {} TUs, {} functions, {} records, {} Dart files, \
         {unparseable_files_len}/{files_len} files unparseable, {error_count} analyze errors, \
         {warning_count} analyze warnings",
        project.compilation_units.len(),
        module.functions.len(),
        module.records.len(),
        files.len(),
        unparseable_files_len = unparseable_files.len(),
        files_len = files.len(),
    );

    let report = DiagnosisReport {
        timestamp: current_timestamp_iso8601(),
        git_commit: current_git_commit(),
        compilation_units: project.compilation_units.len(),
        functions_lowered: module.functions.len(),
        records_lowered: module.records.len(),
        dart_files_emitted: files.len(),
        lines_emitted: total_lines,
        stub_expression_lines: unsupported_expr_lines,
        stub_statement_lines: unsupported_stmt_lines,
        unparseable_files_count: unparseable_files.len(),
        total_files: files.len(),
        dart_analyze_errors: error_count,
        dart_analyze_warnings: warning_count,
        extraction_time_seconds: extract_start.elapsed().as_secs_f64(),
        top_rules: rule_counts
            .iter()
            .take(20)
            .map(|(rule, count)| RuleCount {
                rule: rule.clone(),
                count: *count,
            })
            .collect(),
        top_rule_occurrences: rule_counts
            .iter()
            .take(20)
            .flat_map(|(rule, _)| rule_occurrences.get(rule).into_iter().flatten().cloned())
            .collect(),
        unparseable_files: unparseable_files
            .iter()
            .map(|(path, reason)| UnparseableFile {
                path: path.clone(),
                reason_first_line: reason.lines().next().unwrap_or(reason).to_owned(),
            })
            .collect(),
        bailouts,
    };
    write_diagnosis_report(&report, &repo_root().join(".diagnosis"), "verovio-6.2.0")
        .expect("write diagnosis report snapshot");
    fs::write(
        repo_root().join(".diagnosis/verovio-6.2.0.analyze.json"),
        &analyze_text,
    )
    .expect("write raw dart analyze JSON snapshot");
    eprintln!(
        "[diagnosis] wrote latest-run snapshot to {:?}",
        repo_root().join(".diagnosis")
    );

    // Not a pass/fail assertion on content — this test's value is the
    // eprintln! report above and the `.diagnosis/` snapshot written just
    // above, read with --nocapture. It only fails if the pipeline itself
    // couldn't run at all.
}

/// Checks whole Dart identifier tokens rather than a substring: `dynamic_`
/// is the safe renaming of a C++ variable called `dynamic`, not the Dart
/// escape-hatch type this diagnostic forbids.
fn contains_dart_token(source: &str, sought: &str) -> bool {
    source
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .any(|token| token == sought)
}

/// Blanks out the contents of every Dart string literal in `source`, so
/// `contains_dart_token` only ever sees real code tokens. Needed because a
/// transpiled C++ string literal can legally contain the plain English word
/// "dynamic" (Verovio's `options.cpp` has one: `SetInfo("Dynam dist", "...
/// for dynamic marks")`) — without this, that quoted prose would trip the
/// same `dynamic`-type regression gate a real `dynamic` token should. Only
/// needs to understand `emit::dart::dart_string_literal`'s own output shape
/// (single-quoted, `\`-escaped, never triple-quoted or raw) since this test
/// only ever scans text this project's own emitter produced.
fn strip_dart_string_literals(source: &str) -> String {
    let mut result = String::with_capacity(source.len());
    let mut chars = source.chars();
    while let Some(ch) = chars.next() {
        if ch != '\'' && ch != '"' {
            result.push(ch);
            continue;
        }
        let quote = ch;
        result.push(' ');
        while let Some(next) = chars.next() {
            if next == '\\' {
                chars.next();
                continue;
            }
            if next == quote {
                break;
            }
        }
    }
    result
}

#[test]
fn dynamic_regression_check_ignores_a_safe_identifier_suffix() {
    assert!(contains_dart_token(
        "int value = 0; dynamic value2;",
        "dynamic"
    ));
    assert!(!contains_dart_token("int dynamic_ = 0;", "dynamic"));
}

#[test]
fn dynamic_regression_check_ignores_the_word_inside_a_string_literal() {
    let source = "m.setInfo('Dynam dist', 'The default distance for dynamic marks');";
    assert!(contains_dart_token(source, "dynamic"));
    assert!(!contains_dart_token(
        &strip_dart_string_literals(source),
        "dynamic"
    ));
    assert!(contains_dart_token(
        &strip_dart_string_literals("dynamic value2;"),
        "dynamic"
    ));
}

#[test]
fn bailout_inventory_collects_types_expressions_and_statements_recursively() {
    let origin = Origin {
        file: "fixture.cpp".to_owned(),
        line: 7,
        column: 3,
    };
    let module = Module {
        functions: vec![Function {
            name: "fixture".to_owned(),
            usr: "fixture".to_owned(),
            params: vec![Param {
                name: "input".to_owned(),
                ty: Type::Unsupported("const void *".to_owned()),
                default_value: Some(Expr::Unsupported {
                    reason: "could not lower default value".to_owned(),
                    origin: origin.clone(),
                }),
            }],
            return_type: Type::Unsupported("std::ostream".to_owned()),
            body: vec![
                Stmt::VarDecl {
                    name: "buffer".to_owned(),
                    ty: Type::List(Box::new(Type::Unsupported("char_t".to_owned()))),
                    init: Some(Expr::UnsupportedTyped {
                        reason: "unsupported implicit conversion".to_owned(),
                        ty: Type::Unsupported("char_t".to_owned()),
                        origin: origin.clone(),
                    }),
                    origin: origin.clone(),
                },
                Stmt::Unsupported {
                    reason: "unsupported goto".to_owned(),
                    origin,
                },
            ],
            origin: Origin {
                file: "fixture.cpp".to_owned(),
                line: 1,
                column: 1,
            },
        }],
        records: vec![],
        enums: vec![],
    };

    let inventory = collect_bailouts(&module);

    assert_eq!(inventory.unsupported_types["char_t"], 2);
    assert_eq!(inventory.unsupported_types["const void *"], 1);
    assert_eq!(inventory.unsupported_types["std::ostream"], 1);
    assert_eq!(
        inventory.unsupported_expressions["could not lower default value"],
        1
    );
    assert_eq!(
        inventory.unsupported_expressions["unsupported implicit conversion"],
        1
    );
    assert_eq!(inventory.unsupported_statements["unsupported goto"], 1);
}

/// Complete accounting of every IR node that causes the Dart emitter to
/// produce a bailout. Keeping the source spelling/reason as the map key
/// makes the diagnosis actionable: it says what the lowerer could not model,
/// rather than merely counting generated Dart lines after formatting.
#[derive(Debug, Default, Serialize)]
struct BailoutInventory {
    unsupported_types: std::collections::BTreeMap<String, usize>,
    unsupported_expressions: std::collections::BTreeMap<String, usize>,
    unsupported_statements: std::collections::BTreeMap<String, usize>,
}

impl BailoutInventory {
    fn type_total(&self) -> usize {
        self.unsupported_types.values().sum()
    }

    fn expression_total(&self) -> usize {
        self.unsupported_expressions.values().sum()
    }

    fn statement_total(&self) -> usize {
        self.unsupported_statements.values().sum()
    }
}

fn collect_bailouts(module: &Module) -> BailoutInventory {
    let mut inventory = BailoutInventory::default();

    for function in &module.functions {
        collect_type_bailouts(&function.return_type, &mut inventory);
        for parameter in &function.params {
            collect_parameter_bailouts(parameter, &mut inventory);
        }
        collect_statement_bailouts(&function.body, &mut inventory);
    }

    for record in &module.records {
        for field in record.fields.iter().chain(&record.static_fields) {
            collect_type_bailouts(&field.ty, &mut inventory);
        }
        for constructor in &record.constructors {
            for parameter in &constructor.params {
                collect_parameter_bailouts(parameter, &mut inventory);
            }
            collect_statement_bailouts(&constructor.body, &mut inventory);
        }
        for method in &record.methods {
            collect_type_bailouts(&method.return_type, &mut inventory);
            for parameter in &method.params {
                collect_parameter_bailouts(parameter, &mut inventory);
            }
            if let Some(body) = &method.body {
                collect_statement_bailouts(body, &mut inventory);
            }
        }
        if let Some(destructor) = &record.destructor {
            collect_statement_bailouts(destructor, &mut inventory);
        }
    }

    inventory
}

fn collect_parameter_bailouts(parameter: &Param, inventory: &mut BailoutInventory) {
    collect_type_bailouts(&parameter.ty, inventory);
    if let Some(default_value) = &parameter.default_value {
        collect_expression_bailouts(default_value, inventory);
    }
}

fn collect_type_bailouts(ty: &Type, inventory: &mut BailoutInventory) {
    match ty {
        Type::List(inner) | Type::Set(inner) | Type::Nullable(inner) | Type::ListCursor(inner) => {
            collect_type_bailouts(inner, inventory);
        }
        Type::Map(key, value) => {
            collect_type_bailouts(key, inventory);
            collect_type_bailouts(value, inventory);
        }
        Type::Pair(first, second) => {
            collect_type_bailouts(first, inventory);
            collect_type_bailouts(second, inventory);
        }
        Type::Callback {
            return_type,
            params,
        } => {
            collect_type_bailouts(return_type, inventory);
            for parameter in params {
                collect_type_bailouts(parameter, inventory);
            }
        }
        Type::Tuple(values) => {
            for value in values {
                collect_type_bailouts(value, inventory);
            }
        }
        Type::Unsupported(spelling) => {
            *inventory
                .unsupported_types
                .entry(spelling.clone())
                .or_insert(0) += 1;
        }
        Type::Int
        | Type::Bool
        | Type::Double
        | Type::Void
        | Type::Record { .. }
        | Type::Enum { .. }
        | Type::Str
        | Type::Object
        | Type::Bytes
        | Type::ByteCursor => {}
    }
}

fn collect_expression_bailouts(expression: &Expr, inventory: &mut BailoutInventory) {
    match expression {
        Expr::Ref { ty, .. } | Expr::This { ty, .. } => collect_type_bailouts(ty, inventory),
        Expr::Binary { lhs, rhs, ty, .. } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(lhs, inventory);
            collect_expression_bailouts(rhs, inventory);
        }
        Expr::Conditional {
            condition,
            then_expr,
            else_expr,
            ty,
            ..
        } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(condition, inventory);
            collect_expression_bailouts(then_expr, inventory);
            collect_expression_bailouts(else_expr, inventory);
        }
        Expr::Unary { operand, ty, .. } | Expr::Convert { operand, ty, .. } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(operand, inventory);
        }
        Expr::Call {
            target, args, ty, ..
        } => {
            collect_type_bailouts(ty, inventory);
            if let Some(target) = target {
                collect_expression_bailouts(target, inventory);
            }
            for argument in args {
                collect_expression_bailouts(argument, inventory);
            }
        }
        Expr::FieldAccess { target, ty, .. } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(target, inventory);
        }
        Expr::RecordConstruct { fields, .. } => {
            for (_, value) in fields {
                collect_expression_bailouts(value, inventory);
            }
        }
        Expr::RecordCopy { target, .. } => {
            collect_expression_bailouts(target, inventory);
        }
        Expr::ConstructorCall { args, .. } => {
            for argument in args {
                collect_expression_bailouts(argument, inventory);
            }
        }
        Expr::Index {
            target, index, ty, ..
        } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(target, inventory);
            collect_expression_bailouts(index, inventory);
        }
        Expr::MapIndexOrInsert {
            target,
            index,
            default_value,
            ty,
            ..
        } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(target, inventory);
            collect_expression_bailouts(index, inventory);
            collect_expression_bailouts(default_value, inventory);
        }
        Expr::StringByteLength { target, .. } => collect_expression_bailouts(target, inventory),
        Expr::StringByteIndexOf { target, needle, .. } => {
            collect_expression_bailouts(target, inventory);
            collect_expression_bailouts(needle, inventory);
        }
        Expr::StringByteAt {
            target, index, ty, ..
        } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(target, inventory);
            collect_expression_bailouts(index, inventory);
        }
        Expr::Tuple { values, .. } => {
            for value in values {
                collect_expression_bailouts(value, inventory);
            }
        }
        Expr::ListLiteral { items, ty, .. } => {
            collect_type_bailouts(ty, inventory);
            for item in items {
                collect_expression_bailouts(item, inventory);
            }
        }
        Expr::MapLiteral { entries, ty, .. } => {
            collect_type_bailouts(ty, inventory);
            for (key, value) in entries {
                collect_expression_bailouts(key, inventory);
                collect_expression_bailouts(value, inventory);
            }
        }
        Expr::Is {
            operand,
            target_type,
            ..
        } => {
            collect_type_bailouts(target_type, inventory);
            collect_expression_bailouts(operand, inventory);
        }
        Expr::As { operand, ty, .. } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(operand, inventory);
        }
        Expr::Assign {
            target, value, ty, ..
        } => {
            collect_type_bailouts(ty, inventory);
            collect_expression_bailouts(target, inventory);
            collect_expression_bailouts(value, inventory);
        }
        Expr::Unsupported { reason, .. } | Expr::UnsupportedTyped { reason, .. } => {
            *inventory
                .unsupported_expressions
                .entry(reason.clone())
                .or_insert(0) += 1;
            if let Expr::UnsupportedTyped { ty, .. } = expression {
                collect_type_bailouts(ty, inventory);
            }
        }
        Expr::NamedArg { value, .. } => {
            collect_expression_bailouts(value, inventory);
        }
        Expr::IntLiteral { .. }
        | Expr::DoubleLiteral { .. }
        | Expr::BoolLiteral { .. }
        | Expr::NullLiteral { .. }
        | Expr::StringLiteral { .. } => {}
    }
}

fn collect_statement_bailouts(statements: &[Stmt], inventory: &mut BailoutInventory) {
    for statement in statements {
        match statement {
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    collect_expression_bailouts(value, inventory);
                }
            }
            Stmt::VarDecl { ty, init, .. } => {
                collect_type_bailouts(ty, inventory);
                if let Some(init) = init {
                    collect_expression_bailouts(init, inventory);
                }
            }
            Stmt::Assign { value, .. }
            | Stmt::ExprStmt { expr: value, .. }
            | Stmt::Throw { value, .. } => {
                collect_expression_bailouts(value, inventory);
            }
            Stmt::FieldAssign { target, value, .. } => {
                collect_expression_bailouts(target, inventory);
                collect_expression_bailouts(value, inventory);
            }
            Stmt::ExprAssign { target, value, .. } => {
                collect_expression_bailouts(target, inventory);
                collect_expression_bailouts(value, inventory);
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                collect_expression_bailouts(condition, inventory);
                collect_statement_bailouts(then_branch, inventory);
                collect_statement_bailouts(else_branch, inventory);
            }
            Stmt::While {
                condition, body, ..
            } => {
                collect_expression_bailouts(condition, inventory);
                collect_statement_bailouts(body, inventory);
            }
            Stmt::DoWhile {
                body, condition, ..
            } => {
                collect_statement_bailouts(body, inventory);
                collect_expression_bailouts(condition, inventory);
            }
            Stmt::For {
                init,
                condition,
                increment,
                body,
                ..
            } => {
                if let Some(init) = init {
                    collect_statement_bailouts(std::slice::from_ref(init), inventory);
                }
                if let Some(condition) = condition {
                    collect_expression_bailouts(condition, inventory);
                }
                if let Some(increment) = increment {
                    collect_statement_bailouts(std::slice::from_ref(increment), inventory);
                }
                collect_statement_bailouts(body, inventory);
            }
            Stmt::ForEach {
                ty, iterable, body, ..
            } => {
                collect_type_bailouts(ty, inventory);
                collect_expression_bailouts(iterable, inventory);
                collect_statement_bailouts(body, inventory);
            }
            Stmt::Break { .. } | Stmt::Continue { .. } | Stmt::ContinueLabel { .. } => {}
            Stmt::TryCatch {
                try_body,
                catch_type,
                catch_body,
                ..
            } => {
                collect_type_bailouts(catch_type, inventory);
                collect_statement_bailouts(try_body, inventory);
                collect_statement_bailouts(catch_body, inventory);
            }
            Stmt::TryFinally {
                try_body,
                finally_body,
                ..
            } => {
                collect_statement_bailouts(try_body, inventory);
                collect_statement_bailouts(finally_body, inventory);
            }
            Stmt::TupleAssign { targets, value, .. } => {
                for target in targets {
                    collect_expression_bailouts(target, inventory);
                }
                collect_expression_bailouts(value, inventory);
            }
            Stmt::Switch {
                scrutinee,
                cases,
                default,
                ..
            } => {
                collect_expression_bailouts(scrutinee, inventory);
                for case in cases {
                    for value in &case.values {
                        collect_expression_bailouts(value, inventory);
                    }
                    collect_statement_bailouts(&case.body, inventory);
                }
                if let Some(default) = default {
                    collect_statement_bailouts(default, inventory);
                }
            }
            Stmt::Unsupported { reason, .. } => {
                *inventory
                    .unsupported_statements
                    .entry(reason.clone())
                    .or_insert(0) += 1;
            }
        }
    }
}

fn current_timestamp_iso8601() -> String {
    Command::new("date")
        .arg("-u")
        .arg("+%Y-%m-%dT%H:%M:%SZ")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

fn current_git_commit() -> String {
    Command::new("git")
        .arg("rev-parse")
        .arg("--short")
        .arg("HEAD")
        .current_dir(repo_root())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

/// Raw metrics from one diagnosis run, gravadas em `.diagnosis/` (gitignored,
/// sempre sobrescritas) para que "qual a próxima direção" seja consultável a
/// qualquer momento sem reler histórico de terminal nem rodar de novo — a
/// interpretação (achados, causa-raiz, recomendação) continua vivendo à mão
/// em `docs/plans/diagnostico-verovio-6.2.0.md`, não é gerada aqui.
#[derive(Debug, Serialize)]
struct DiagnosisReport {
    timestamp: String,
    git_commit: String,
    compilation_units: usize,
    functions_lowered: usize,
    records_lowered: usize,
    dart_files_emitted: usize,
    lines_emitted: usize,
    stub_expression_lines: usize,
    stub_statement_lines: usize,
    unparseable_files_count: usize,
    total_files: usize,
    dart_analyze_errors: usize,
    dart_analyze_warnings: usize,
    extraction_time_seconds: f64,
    top_rules: Vec<RuleCount>,
    top_rule_occurrences: Vec<AnalyzerDiagnostic>,
    unparseable_files: Vec<UnparseableFile>,
    bailouts: BailoutInventory,
}

#[derive(Debug, Serialize)]
struct RuleCount {
    rule: String,
    count: usize,
}

/// One concrete `dart analyze --format=json` result, kept with the report so
/// a dominant rule can be reduced to a minimal C++ reproduction without
/// rerunning the whole Verovio corpus.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct AnalyzerDiagnostic {
    rule: String,
    severity: String,
    path: String,
    line: u64,
    column: u64,
    message: String,
}

/// Reads both JSON shapes Dart has used for analyzer output: the normal
/// object with a `diagnostics` array and a bare diagnostics array. Unknown or
/// incomplete entries are ignored rather than making a diagnostic-only run
/// fail because a future SDK adds a non-location event.
fn parse_dart_analyze_json(text: &str) -> Result<Vec<AnalyzerDiagnostic>, serde_json::Error> {
    let value: Value = serde_json::from_str(text)?;
    let entries = match &value {
        Value::Object(object) => object
            .get("diagnostics")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default(),
        Value::Array(entries) => entries.as_slice(),
        _ => &[],
    };

    Ok(entries
        .iter()
        .filter_map(|entry| {
            let object = entry.as_object()?;
            let rule = object.get("code")?.as_str()?.to_owned();
            let severity = object.get("severity")?.as_str()?.to_owned();
            let message = object
                .get("problemMessage")
                .or_else(|| object.get("message"))?
                .as_str()?
                .to_owned();
            let start = object.get("location")?.get("range")?.get("start")?;
            Some(AnalyzerDiagnostic {
                rule,
                severity,
                path: object.get("location")?.get("file")?.as_str()?.to_owned(),
                line: start.get("line")?.as_u64()?,
                column: start.get("column")?.as_u64()?,
                message,
            })
        })
        .collect())
}

#[derive(Debug, Serialize)]
struct UnparseableFile {
    path: String,
    reason_first_line: String,
}

impl DiagnosisReport {
    fn stub_percentage(&self) -> f64 {
        if self.lines_emitted == 0 {
            return 0.0;
        }
        100.0 * (self.stub_expression_lines + self.stub_statement_lines) as f64
            / self.lines_emitted as f64
    }

    fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# Diagnóstico Verovio 6.2.0 — última rodada\n\n");
        out.push_str(&format!(
            "Gerado em {} (commit {}). Métricas brutas, sem interpretação — achados e \
             recomendação continuam em `docs/plans/diagnostico-verovio-6.2.0.md`.\n\n",
            self.timestamp, self.git_commit
        ));
        out.push_str("| Métrica | Valor |\n| --- | --- |\n");
        out.push_str(&format!(
            "| Unidades de compilação | {} |\n",
            self.compilation_units
        ));
        out.push_str(&format!(
            "| Funções livres lowered | {} |\n",
            self.functions_lowered
        ));
        out.push_str(&format!(
            "| Classes/structs lowered | {} |\n",
            self.records_lowered
        ));
        out.push_str(&format!(
            "| Arquivos `.dart` emitidos | {} |\n",
            self.dart_files_emitted
        ));
        out.push_str(&format!("| Linhas emitidas | {} |\n", self.lines_emitted));
        out.push_str(&format!(
            "| Linhas stub (expressão) | {} |\n",
            self.stub_expression_lines
        ));
        out.push_str(&format!(
            "| Linhas stub (statement) | {} |\n",
            self.stub_statement_lines
        ));
        out.push_str(&format!("| Stub (%) | {:.1}% |\n", self.stub_percentage()));
        out.push_str(&format!(
            "| Arquivos que não parseiam | {}/{} |\n",
            self.unparseable_files_count, self.total_files
        ));
        out.push_str(&format!(
            "| Erros `dart analyze` | {} |\n",
            self.dart_analyze_errors
        ));
        out.push_str(&format!(
            "| Avisos `dart analyze` | {} |\n",
            self.dart_analyze_warnings
        ));
        out.push_str(&format!(
            "| Tempo de extração (s) | {:.1} |\n",
            self.extraction_time_seconds
        ));

        out.push_str("\n## Bailouts por origem no IR\n\n");
        out.push_str("| Origem | Ocorrências | Causas distintas |\n| --- | ---: | ---: |\n");
        out.push_str(&format!(
            "| Tipo C++ sem mapeamento | {} | {} |\n",
            self.bailouts.type_total(),
            self.bailouts.unsupported_types.len()
        ));
        out.push_str(&format!(
            "| Expressão sem lowering | {} | {} |\n",
            self.bailouts.expression_total(),
            self.bailouts.unsupported_expressions.len()
        ));
        out.push_str(&format!(
            "| Statement sem lowering | {} | {} |\n",
            self.bailouts.statement_total(),
            self.bailouts.unsupported_statements.len()
        ));
        out.push_str(
            "\nAs tabelas completas (spelling/razão e contagem) estão no campo `bailouts` \
             do snapshot JSON correspondente.\n",
        );

        out.push_str("\n## Top 20 regras do `dart analyze`\n\n");
        for (index, rule) in self.top_rules.iter().enumerate() {
            out.push_str(&format!(
                "{}. {:>6}  {}\n",
                index + 1,
                rule.count,
                rule.rule
            ));
        }

        out.push_str("\n## Primeiras ocorrências das regras dominantes\n\n");
        for occurrence in &self.top_rule_occurrences {
            out.push_str(&format!(
                "- `{}` — {}:{}:{} — {}\n",
                occurrence.rule,
                occurrence.path,
                occurrence.line,
                occurrence.column,
                occurrence.message
            ));
        }

        out.push_str(&format!(
            "\n## Arquivos que não parseiam ({})\n\n",
            self.unparseable_files.len()
        ));
        for file in &self.unparseable_files {
            out.push_str(&format!("- {}: {}\n", file.path, file.reason_first_line));
        }

        out
    }
}

/// Grava `{dir}/{name}.json` e `{dir}/{name}.md`, sobrescrevendo qualquer
/// rodada anterior — só o snapshot mais recente é mantido.
fn write_diagnosis_report(report: &DiagnosisReport, dir: &Path, name: &str) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let json = serde_json::to_string_pretty(report).expect("serialize DiagnosisReport");
    fs::write(dir.join(format!("{name}.json")), json)?;
    fs::write(dir.join(format!("{name}.md")), report.to_markdown())?;
    Ok(())
}

#[test]
fn write_diagnosis_report_writes_json_and_markdown_snapshots() {
    let workspace = TempWorkspace::new("write-diagnosis-report").expect("create temp workspace");
    let report = DiagnosisReport {
        timestamp: "2026-08-17T12:00:00Z".to_owned(),
        git_commit: "abc1234".to_owned(),
        compilation_units: 298,
        functions_lowered: 513,
        records_lowered: 1345,
        dart_files_emitted: 296,
        lines_emitted: 677_708,
        stub_expression_lines: 20_800,
        stub_statement_lines: 70_850,
        unparseable_files_count: 67,
        total_files: 296,
        dart_analyze_errors: 154_636,
        dart_analyze_warnings: 19_754,
        extraction_time_seconds: 300.0,
        top_rules: vec![
            RuleCount {
                rule: "duplicate_definition".to_owned(),
                count: 132_023,
            },
            RuleCount {
                rule: "mixin_of_non_class".to_owned(),
                count: 929,
            },
        ],
        top_rule_occurrences: vec![AnalyzerDiagnostic {
            rule: "duplicate_definition".to_owned(),
            severity: "ERROR".to_owned(),
            path: "lib/accid.dart".to_owned(),
            line: 42,
            column: 7,
            message: "The name is already defined.".to_owned(),
        }],
        unparseable_files: vec![UnparseableFile {
            path: "lib/accid.dart".to_owned(),
            reason_first_line: "Error: Expected ';' after this.".to_owned(),
        }],
        bailouts: BailoutInventory {
            unsupported_types: std::collections::BTreeMap::from([(
                "std::pair<int, int>".to_owned(),
                2,
            )]),
            unsupported_expressions: std::collections::BTreeMap::from([(
                "unsupported std::vector::resize call".to_owned(),
                3,
            )]),
            unsupported_statements: std::collections::BTreeMap::from([(
                "unsupported goto".to_owned(),
                1,
            )]),
        },
    };

    write_diagnosis_report(&report, workspace.path(), "verovio-6.2.0")
        .expect("write diagnosis report");

    let json_path = workspace.path().join("verovio-6.2.0.json");
    let md_path = workspace.path().join("verovio-6.2.0.md");
    assert!(json_path.is_file(), "expected {json_path:?} to exist");
    assert!(md_path.is_file(), "expected {md_path:?} to exist");

    let json_contents = fs::read_to_string(&json_path).expect("read json snapshot");
    let parsed: serde_json::Value =
        serde_json::from_str(&json_contents).expect("parse json snapshot");
    assert_eq!(parsed["compilation_units"], 298);
    assert_eq!(parsed["top_rules"][0]["rule"], "duplicate_definition");
    assert_eq!(
        parsed["top_rule_occurrences"][0]["line"], 42,
        "expected a concrete analyzer location"
    );
    assert_eq!(parsed["unparseable_files"][0]["path"], "lib/accid.dart");
    assert_eq!(
        parsed["bailouts"]["unsupported_types"]["std::pair<int, int>"],
        2
    );

    let md_contents = fs::read_to_string(&md_path).expect("read markdown snapshot");
    assert!(md_contents.contains("| Unidades de compilação | 298 |"));
    assert!(md_contents.contains("duplicate_definition"));
    assert!(md_contents.contains("Primeiras ocorrências"));
    assert!(md_contents.contains("lib/accid.dart"));
    assert!(md_contents.contains("Bailouts por origem no IR"));

    // Uma segunda rodada sobrescreve, não acumula.
    let mut second = report;
    second.compilation_units = 1;
    write_diagnosis_report(&second, workspace.path(), "verovio-6.2.0")
        .expect("write diagnosis report a second time");
    let overwritten = fs::read_to_string(&md_path).expect("read overwritten markdown snapshot");
    assert!(overwritten.contains("| Unidades de compilação | 1 |"));
    assert!(!overwritten.contains("| Unidades de compilação | 298 |"));
}

#[test]
fn analyzer_json_occurrences_keep_rule_location_and_message_for_triage() {
    let output = r#"{
      "diagnostics": [
        {
          "code": "receiver_of_type_never",
          "severity": "ERROR",
          "problemMessage": "The method 'run' can't be called on 'Never'.",
          "location": {
            "file": "/tmp/verovio/lib/vrv.dart",
            "range": { "start": { "line": 42, "column": 17 } }
          }
        },
        {
          "code": "unused_field",
          "severity": "WARNING",
          "problemMessage": "The value of the field isn't used.",
          "location": {
            "file": "/tmp/verovio/lib/vrv.dart",
            "range": { "start": { "line": 8, "column": 3 } }
          }
        }
      ]
    }"#;

    let diagnostics = parse_dart_analyze_json(output).expect("parse analyzer JSON");

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].rule, "receiver_of_type_never");
    assert_eq!(diagnostics[0].severity, "ERROR");
    assert_eq!(diagnostics[0].path, "/tmp/verovio/lib/vrv.dart");
    assert_eq!(diagnostics[0].line, 42);
    assert_eq!(diagnostics[0].column, 17);
    assert!(diagnostics[0].message.contains("can't be called"));
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("resolve repo root from CARGO_MANIFEST_DIR")
}

#[derive(Debug)]
struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(name: &str) -> std::io::Result<Self> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "syntax-bridge-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path)?;

        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
