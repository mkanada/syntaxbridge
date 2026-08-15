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
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use syntax_bridge_server::emit::dart;
use syntax_bridge_server::function_catalog;
use syntax_bridge_server::ingest::{self, CreateProjectRequest};
use syntax_bridge_server::ir::Module;

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
    };
    let files = dart::emit_module(&module);
    eprintln!("[diagnosis] emitted {} Dart files", files.len());

    // Coverage proxy: how much of the emitted text is an honest stub
    // (`_syntaxBridgeUnsupported(...)` in expression position, `// TODO
    // (syntax-bridge):` in statement position — see emit::dart's module
    // docs) versus real translated code. Line-based, not a real parse — a
    // cheap, order-of-magnitude signal, not a precise metric.
    let mut total_lines: usize = 0;
    let mut unsupported_expr_lines: usize = 0;
    let mut unsupported_stmt_lines: usize = 0;
    for contents in files.values() {
        for line in contents.lines() {
            total_lines += 1;
            if line.contains("_syntaxBridgeUnsupported(") {
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
    eprintln!("[diagnosis] full stderr for the first 2 unparseable files:");
    for (path, reason) in unparseable_files.iter().take(2) {
        eprintln!("=== {path} ===\n{reason}");
    }
    eprintln!("[diagnosis] raw source of the first unparseable file (first 120 lines):");
    if let Some((path, _)) = unparseable_files.first() {
        let contents = files.get(path).expect("path came from files.keys()");
        for (index, line) in contents.lines().take(120).enumerate() {
            eprintln!("{:>4} | {line}", index + 1);
        }
    }

    eprintln!("[diagnosis] running `dart analyze` over the whole package...");
    let analyze_output = Command::new("dart")
        .arg("analyze")
        .arg(&package_dir)
        .output()
        .expect("run dart analyze");
    let analyze_text = String::from_utf8_lossy(&analyze_output.stdout).into_owned();
    let error_count = analyze_text
        .lines()
        .filter(|line| line.trim_start().starts_with("error"))
        .count();
    let warning_count = analyze_text
        .lines()
        .filter(|line| line.trim_start().starts_with("warning"))
        .count();
    eprintln!(
        "[diagnosis] dart analyze: {error_count} errors, {warning_count} warnings ({} total \
         lines of output)",
        analyze_text.lines().count()
    );
    eprintln!("[diagnosis] first 20 duplicate_definition lines:");
    for line in analyze_text
        .lines()
        .filter(|line| line.contains("duplicate_definition"))
        .take(20)
    {
        eprintln!("  {line}");
    }

    // A rough taxonomy of *why* `dart analyze` rejects a file — the lint
    // rule name is the last bracketed token on each error line
    // (`- undefined_method` style). Cheap text scan, not a JSON-format
    // parse (`--format=json` exists but this is diagnostic-only).
    use std::collections::BTreeMap;
    let mut rule_counts: BTreeMap<String, usize> = BTreeMap::new();
    for line in analyze_text.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("error") && !trimmed.starts_with("warning") {
            continue;
        }
        if let Some(rule) = trimmed.rsplit(" - ").next()
            && rule != trimmed
        {
            *rule_counts.entry(rule.trim().to_owned()).or_insert(0) += 1;
        }
    }
    let mut rule_counts: Vec<(String, usize)> = rule_counts.into_iter().collect();
    rule_counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    eprintln!("[diagnosis] top diagnostic rules:");
    for (rule, count) in rule_counts.iter().take(20) {
        eprintln!("  - {count:>5}  {rule}");
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

    // Not a pass/fail assertion on content — this test's value is the
    // eprintln! report above, read with --nocapture. It only fails if the
    // pipeline itself couldn't run at all.
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
