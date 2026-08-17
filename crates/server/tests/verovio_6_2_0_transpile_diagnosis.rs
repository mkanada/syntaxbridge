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
        enums: catalog.ir_enums,
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
    eprintln!("[diagnosis] first 10 mixin_of_non_class lines:");
    for line in analyze_text
        .lines()
        .filter(|line| line.contains("mixin_of_non_class"))
        .take(10)
    {
        eprintln!("  {line}");
    }
    eprintln!("[diagnosis] first 10 extends_non_class lines:");
    for line in analyze_text
        .lines()
        .filter(|line| line.contains("extends_non_class"))
        .take(10)
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
        unparseable_files: unparseable_files
            .iter()
            .map(|(path, reason)| UnparseableFile {
                path: path.clone(),
                reason_first_line: reason.lines().next().unwrap_or(reason).to_owned(),
            })
            .collect(),
    };
    write_diagnosis_report(&report, &repo_root().join(".diagnosis"), "verovio-6.2.0")
        .expect("write diagnosis report snapshot");
    eprintln!(
        "[diagnosis] wrote latest-run snapshot to {:?}",
        repo_root().join(".diagnosis")
    );

    // Not a pass/fail assertion on content — this test's value is the
    // eprintln! report above and the `.diagnosis/` snapshot written just
    // above, read with --nocapture. It only fails if the pipeline itself
    // couldn't run at all.
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
    unparseable_files: Vec<UnparseableFile>,
}

#[derive(Debug, Serialize)]
struct RuleCount {
    rule: String,
    count: usize,
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

        out.push_str("\n## Top 20 regras do `dart analyze`\n\n");
        for (index, rule) in self.top_rules.iter().enumerate() {
            out.push_str(&format!(
                "{}. {:>6}  {}\n",
                index + 1,
                rule.count,
                rule.rule
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
        unparseable_files: vec![UnparseableFile {
            path: "lib/accid.dart".to_owned(),
            reason_first_line: "Error: Expected ';' after this.".to_owned(),
        }],
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
    assert_eq!(parsed["unparseable_files"][0]["path"], "lib/accid.dart");

    let md_contents = fs::read_to_string(&md_path).expect("read markdown snapshot");
    assert!(md_contents.contains("| Unidades de compilação | 298 |"));
    assert!(md_contents.contains("duplicate_definition"));
    assert!(md_contents.contains("lib/accid.dart"));

    // Uma segunda rodada sobrescreve, não acumula.
    let mut second = report;
    second.compilation_units = 1;
    write_diagnosis_report(&second, workspace.path(), "verovio-6.2.0")
        .expect("write diagnosis report a second time");
    let overwritten = fs::read_to_string(&md_path).expect("read overwritten markdown snapshot");
    assert!(overwritten.contains("| Unidades de compilação | 1 |"));
    assert!(!overwritten.contains("| Unidades de compilação | 298 |"));
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
