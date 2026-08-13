//! Harness for the `examples/` corpus described in
//! `docs/plans/conversao-guiada-por-exemplos.md` and scoped by
//! `docs/plans/primeiro-corte-e01-e03.md` (PR1: "infra do corpus").
//!
//! It varre `examples/`, parses each `example.toml`, and keeps the corpus
//! honest: an example marked `esperado-falhar` that starts passing fails the
//! suite (something started working by accident and nobody noticed), and an
//! example marked `passa` that regresses also fails the suite.
//!
//! `example.toml` is a flat, hand-written TOML subset (string/integer/array
//! of strings, no tables, no nesting) — deliberately not parsed with a
//! `toml` crate: the workspace vendors all dependencies
//! (`.cargo/config.toml` replaces crates.io with the local `vendor/`
//! directory, mirroring the Flatpak's no-network policy), and `toml` isn't
//! vendored. Adding it would mean vendoring a new dependency for four
//! well-known scalar fields and two string arrays; a ~60-line subset parser
//! is cheaper and is exactly AGENTS.md's "não introduza dependências
//! externas sem justificar a necessidade".

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::Value;

use syntax_bridge_server::function_catalog;
use syntax_bridge_server::ingest;
use syntax_bridge_server::ir;
use syntax_bridge_server::mapping::{self, MappingDecision};
use syntax_bridge_server::persistence::ProjectStore;
use syntax_bridge_server::transpile;

// ---------------------------------------------------------------------
// example.toml: minimal TOML-subset parser
// ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum TomlValue {
    String(String),
    Integer(i64),
    Array(Vec<String>),
}

/// Parses the flat `key = value` subset of TOML that `example.toml` and
/// (from US-7 onward) `decisions.toml` use: strings, integers, and arrays of
/// strings. No tables, no nesting, no multiline values — see the module doc
/// for why this isn't a real TOML parser.
fn parse_toml_subset(input: &str) -> Result<BTreeMap<String, TomlValue>, String> {
    let mut table = BTreeMap::new();

    for (line_number, raw_line) in input.lines().enumerate() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {}: expected `key = value`, got {raw_line:?}",
                line_number + 1
            ));
        };
        let key = key.trim().to_owned();
        let value = value.trim();

        let parsed = if let Some(inner) = value
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            let mut items = Vec::new();
            for item in inner.split(',') {
                let item = item.trim();
                if item.is_empty() {
                    continue;
                }
                items.push(parse_quoted_string(item).ok_or_else(|| {
                    format!(
                        "line {}: array item {item:?} is not a quoted string",
                        line_number + 1
                    )
                })?);
            }
            TomlValue::Array(items)
        } else if let Some(text) = parse_quoted_string(value) {
            TomlValue::String(text)
        } else {
            value
                .parse::<i64>()
                .map(TomlValue::Integer)
                .map_err(|_| format!("line {}: cannot parse value {value:?}", line_number + 1))?
        };

        table.insert(key, parsed);
    }

    Ok(table)
}

fn parse_quoted_string(text: &str) -> Option<String> {
    let inner = text.strip_prefix('"')?.strip_suffix('"')?;
    Some(inner.to_owned())
}

// ---------------------------------------------------------------------
// ExampleManifest
// ---------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExampleStatus {
    Passa,
    EsperadoFalhar,
}

#[derive(Debug, Clone)]
struct ExampleManifest {
    id: String,
    #[allow(dead_code)]
    nome: String,
    #[allow(dead_code)]
    nivel: i64,
    status: ExampleStatus,
    #[allow(dead_code)]
    motivo: String,
    #[allow(dead_code)]
    constroi: Vec<String>,
    #[allow(dead_code)]
    passos: Vec<String>,
}

impl ExampleManifest {
    fn parse(contents: &str) -> Result<Self, String> {
        let table = parse_toml_subset(contents)?;

        let id = expect_string(&table, "id")?;
        let nome = expect_string(&table, "nome")?;
        let nivel = expect_integer(&table, "nivel")?;
        let status_text = expect_string(&table, "status")?;
        let status = match status_text.as_str() {
            "passa" => ExampleStatus::Passa,
            "esperado-falhar" => ExampleStatus::EsperadoFalhar,
            other => return Err(format!("unknown status {other:?}")),
        };
        let motivo = expect_string(&table, "motivo")?;
        let constroi = expect_array(&table, "constroi")?;
        let passos = expect_array(&table, "passos")?;

        Ok(Self {
            id,
            nome,
            nivel,
            status,
            motivo,
            constroi,
            passos,
        })
    }
}

fn expect_string(table: &BTreeMap<String, TomlValue>, key: &str) -> Result<String, String> {
    match table.get(key) {
        Some(TomlValue::String(value)) => Ok(value.clone()),
        Some(_) => Err(format!("field {key:?} is not a string")),
        None => Err(format!("missing field {key:?}")),
    }
}

fn expect_integer(table: &BTreeMap<String, TomlValue>, key: &str) -> Result<i64, String> {
    match table.get(key) {
        Some(TomlValue::Integer(value)) => Ok(*value),
        Some(_) => Err(format!("field {key:?} is not an integer")),
        None => Err(format!("missing field {key:?}")),
    }
}

fn expect_array(table: &BTreeMap<String, TomlValue>, key: &str) -> Result<Vec<String>, String> {
    match table.get(key) {
        Some(TomlValue::Array(value)) => Ok(value.clone()),
        Some(_) => Err(format!("field {key:?} is not an array")),
        None => Err(format!("missing field {key:?}")),
    }
}

// ---------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------

#[derive(Debug)]
struct Example {
    dir: PathBuf,
    manifest: ExampleManifest,
}

impl Example {
    fn input_dir(&self) -> PathBuf {
        self.dir.join("input")
    }
}

#[derive(Debug)]
enum DiscoveryError {
    MissingManifest(String),
    InvalidManifest(String, String),
    Io(String, io::Error),
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingManifest(name) => {
                write!(formatter, "{name}: missing example.toml")
            }
            Self::InvalidManifest(name, reason) => {
                write!(formatter, "{name}: invalid example.toml: {reason}")
            }
            Self::Io(path, error) => write!(formatter, "{path}: {error}"),
        }
    }
}

/// Varre os subdiretórios imediatos de `root`, cada um exigindo seu próprio
/// `example.toml` — critério 3 de PR1: um diretório sem manifesto, ou com um
/// manifesto malformado, nomeia o exemplo no erro em vez de ser ignorado.
fn discover_examples(root: &Path) -> Result<Vec<Example>, DiscoveryError> {
    let read_dir = fs::read_dir(root)
        .map_err(|error| DiscoveryError::Io(root.display().to_string(), error))?;

    let mut entries: Vec<PathBuf> = read_dir
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    entries.sort();

    let mut examples = Vec::with_capacity(entries.len());
    for dir in entries {
        let name = dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        let manifest_path = dir.join("example.toml");
        if !manifest_path.is_file() {
            return Err(DiscoveryError::MissingManifest(name));
        }

        let contents = fs::read_to_string(&manifest_path)
            .map_err(|error| DiscoveryError::Io(manifest_path.display().to_string(), error))?;
        let manifest = ExampleManifest::parse(&contents)
            .map_err(|reason| DiscoveryError::InvalidManifest(name, reason))?;

        examples.push(Example { dir, manifest });
    }

    Ok(examples)
}

fn examples_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

// ---------------------------------------------------------------------
// Running an example
// ---------------------------------------------------------------------

// `Passed` isn't constructed until PR3 wires up the behavioral oracle (the
// third of the three criteria `run_example` checks) — kept here so the match
// in the test below doesn't change shape when that happens.
#[allow(dead_code)]
enum Outcome {
    Passed,
    Failed(String),
}

/// Runs the three criteria of `conversao-guiada-por-exemplos.md` §5 in
/// order — golden (5.1), `dart analyze`/`dart format` (5.2), oráculo
/// comportamental (5.3) — stopping at the first that fails. No branch here
/// may depend on `example.id`/name (§8 regra 1): every example, including
/// ones with no `expected/` yet, goes through the same path and fails for a
/// structural reason instead of being special-cased.
///
/// PR3 adds the oracle; until then, a golden+analyze pass still reports
/// `Failed` (with a reason saying so) rather than `Passed` — the corpus
/// can't call criterion 5.3 done before it exists.
fn run_example(example: &Example) -> Outcome {
    let workspace = match TempWorkspace::new(&format!("run-example-{}", example.manifest.id)) {
        Ok(workspace) => workspace,
        Err(error) => return Outcome::Failed(format!("could not create temp workspace: {error}")),
    };
    let build_dir = workspace.path().join("build");

    if let Err(error) = run_command(
        Command::new("cmake")
            .arg("-S")
            .arg(example.input_dir())
            .arg("-B")
            .arg(&build_dir)
            .arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"),
    ) {
        return Outcome::Failed(format!("cmake configure failed: {error}"));
    }

    let compile_commands_path = build_dir.join("compile_commands.json");
    let compilation_units = match ingest::read_compilation_units(&compile_commands_path) {
        Ok(units) => units,
        Err(error) => {
            return Outcome::Failed(format!("could not read compile_commands.json: {error}"));
        }
    };

    let project_root = example
        .input_dir()
        .canonicalize()
        .unwrap_or_else(|_| example.input_dir());
    let raw_package_name = example
        .dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| example.manifest.id.clone());

    let package = match transpile::transpile(&compilation_units, &project_root, &raw_package_name) {
        Ok(package) => package,
        Err(error) => return Outcome::Failed(format!("transpile failed: {error}")),
    };

    // Criterion 5.1: golden. `just examples-bless` (SYNTAX_BRIDGE_BLESS=1)
    // overwrites expected/ with the current output instead of comparing —
    // "golden é regravável" (§5.1 of conversao-guiada-por-exemplos.md), but
    // the rewrite always shows up in the PR diff and needs a normal run
    // afterwards to actually pass.
    let expected_dir = example.dir.join("expected");
    if std::env::var("SYNTAX_BRIDGE_BLESS").is_ok_and(|value| value == "1") {
        if let Err(error) = write_golden_files(&expected_dir, &package.files) {
            return Outcome::Failed(format!("could not bless expected/: {error}"));
        }
        eprintln!(
            "[examples] {} blessed: {} rewritten",
            example.manifest.id,
            expected_dir.display()
        );
    }
    let Some(golden_files) = read_golden_files(&expected_dir) else {
        return Outcome::Failed(format!(
            "no expected/ golden yet under {}",
            expected_dir.display()
        ));
    };
    if golden_files != package.files {
        return Outcome::Failed(format!(
            "generated Dart does not match expected/ (run `just examples-bless` to inspect/update)\n\
             generated: {:#?}\nexpected: {:#?}",
            package.files, golden_files
        ));
    }

    // Criterion 5.2: compiles (`dart analyze` + `dart format`).
    let package_dir = workspace.path().join("dart-package");
    if let Err(error) = transpile::write_package(&package, &package_dir) {
        return Outcome::Failed(format!("could not write Dart package: {error}"));
    }
    if let Err(error) = run_command(Command::new("dart").arg("analyze").arg(&package_dir)) {
        return Outcome::Failed(format!("dart analyze failed: {error}"));
    }
    if let Err(error) = run_command(
        Command::new("dart")
            .arg("format")
            .arg("--output=none")
            .arg("--set-exit-if-changed")
            .arg(&package_dir),
    ) {
        return Outcome::Failed(format!("dart format check failed: {error}"));
    }

    // Records (for aggregate-typed oracle arguments, E03+): re-derived here
    // rather than threaded out of `transpile::transpile` (which only needs
    // to expose the emitted files, not the IR) — a second, cheap parse in
    // test-only code, not the product's ingestion pipeline the "no fourth
    // pass" rule is about.
    let records =
        match function_catalog::extract_function_catalog(&compilation_units, &project_root, None) {
            Ok(catalog) => catalog.ir_records,
            Err(error) => return Outcome::Failed(format!("could not re-extract records: {error}")),
        };

    // Criterion 5.3: oráculo comportamental.
    run_oracle(example, &project_root, &package, &package_dir, &records)
}

// ---------------------------------------------------------------------
// Oracle (criterion 5.3): compile+run the real C++, run the transpiled
// Dart, compare canonical output per case in `oracle/cases.json`.
// ---------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct OracleFile {
    #[allow(dead_code)]
    schema_version: u32,
    casos: Vec<OracleCase>,
}

#[derive(Debug, Deserialize)]
struct OracleCase {
    funcao: String,
    args: Vec<Value>,
    espera: Value,
    divergencia_conhecida: Option<String>,
}

/// Every case's contract is: whatever `funcao(args)` returns in real C++
/// must equal what the transpiled Dart returns, unless the case declares a
/// known divergence — in which case they must actually differ (§4 decision
/// 3 of `primeiro-corte-e01-e03.md`: the premise is declared, not silently
/// assumed).
fn run_oracle(
    example: &Example,
    project_root: &Path,
    package: &transpile::TranspiledPackage,
    package_dir: &Path,
    records: &[ir::Record],
) -> Outcome {
    let cases_path = example.dir.join("oracle").join("cases.json");
    let cases = match read_oracle_cases(&cases_path) {
        Ok(cases) => cases,
        Err(error) => return Outcome::Failed(format!("could not read oracle/cases.json: {error}")),
    };
    if cases.is_empty() {
        return Outcome::Failed(format!(
            "oracle/cases.json at {} has no casos",
            cases_path.display()
        ));
    }

    let oracle_workspace = match TempWorkspace::new(&format!("oracle-{}", example.manifest.id)) {
        Ok(workspace) => workspace,
        Err(error) => {
            return Outcome::Failed(format!("could not create oracle workspace: {error}"));
        }
    };

    let cpp_lines = match run_cpp_oracle(project_root, &cases, records, oracle_workspace.path()) {
        Ok(lines) => lines,
        Err(error) => return Outcome::Failed(format!("C++ oracle run failed: {error}")),
    };
    let dart_lines = match run_dart_oracle(
        package,
        package_dir,
        &cases,
        records,
        oracle_workspace.path(),
    ) {
        Ok(lines) => lines,
        Err(error) => return Outcome::Failed(format!("Dart oracle run failed: {error}")),
    };

    match compare_oracle_outputs(
        &cases,
        &cpp_lines,
        &dart_lines,
        &project_root.display().to_string(),
    ) {
        Ok(()) => Outcome::Passed,
        Err(reason) => Outcome::Failed(reason),
    }
}

/// The actual assertion the oracle rests on (criterion 5.3), isolated from
/// process-spawning so the mutation test below can feed it a deliberately
/// sabotaged `dart_lines` and prove *this exact function* — not a copy of
/// its logic — rejects it, with origin and both values in the message
/// (§5.4 of `conversao-guiada-por-exemplos.md`: "exige que o oráculo falhe
/// com origem e valores esperado/obtido").
fn compare_oracle_outputs(
    cases: &[OracleCase],
    cpp_lines: &[String],
    dart_lines: &[String],
    origin: &str,
) -> Result<(), String> {
    if cpp_lines.len() != cases.len() || dart_lines.len() != cases.len() {
        return Err(format!(
            "expected {} lines from each oracle, got {} from C++ and {} from Dart",
            cases.len(),
            cpp_lines.len(),
            dart_lines.len()
        ));
    }

    for (index, case) in cases.iter().enumerate() {
        // C++'s default `<<` formatting drops the trailing `.0` off a
        // whole-number `double` ("7", not "7.0"); Dart's `double.toString()`
        // never does. Not full float equivalence (that's US-10's
        // `equivalence.rs`, bit-level — see
        // `examples/E02-controle-de-fluxo/NOTES.md`) — just enough that a
        // case whose `espera` is written with a decimal point (`7.0`, i.e.
        // `serde_json` parsed it as an `f64`, `is_f64()`) isn't rejected on
        // a formatting artifact instead of a real divergence.
        let cpp_actual = canonicalize_for_comparison(&cpp_lines[index], &case.espera);
        let dart_actual = canonicalize_for_comparison(&dart_lines[index], &case.espera);

        // Criterion 2: `espera` is sanity-checked against the *real C++*
        // output — a mismatch blames the example, not the product.
        if let Some(expected_text) = json_canonical(&case.espera)
            && expected_text != cpp_actual
        {
            return Err(format!(
                "example error in oracle/cases.json case {index} ({}({:?})): \
                 espera={expected_text:?} but the real C++ produced {cpp_actual:?} — \
                 the exemplo's `espera` is wrong, fix oracle/cases.json",
                case.funcao, case.args
            ));
        }

        match &case.divergencia_conhecida {
            Some(reason) => {
                if cpp_actual == dart_actual {
                    return Err(format!(
                        "case {index} ({}({:?})) is marked divergencia_conhecida={reason:?} \
                         but C++ and Dart agree ({cpp_actual:?}) — remove the marker or the \
                         premise it documents no longer holds",
                        case.funcao, case.args
                    ));
                }
            }
            None => {
                if cpp_actual != dart_actual {
                    return Err(format!(
                        "case {index} ({}({:?})) origin C++={origin} diverges: C++ produced \
                         {cpp_actual:?}, Dart produced {dart_actual:?}",
                        case.funcao, case.args
                    ));
                }
            }
        }
    }

    Ok(())
}

fn canonicalize_for_comparison(text: &str, espera: &Value) -> String {
    let is_double = matches!(espera, Value::Number(number) if number.is_f64());
    if is_double && !text.contains('.') {
        format!("{text}.0")
    } else {
        text.to_owned()
    }
}

fn read_oracle_cases(path: &Path) -> Result<Vec<OracleCase>, String> {
    let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let file: OracleFile = serde_json::from_str(&contents).map_err(|error| error.to_string())?;
    Ok(file.casos)
}

/// Every JSON value type the oracle knows how to turn into a source literal
/// / canonical printed form — ints and bools, the only shapes E01's cases
/// use. Anything else (doubles, objects — E02/E03 scope) is reported as an
/// error naming the case, never silently skipped.
/// A JSON object argument (`{"x": 3.0, "y": 4.0}`, E03+) is an aggregate
/// literal — `records` is how the harness knows the target type's *declared*
/// field order (`{"y": ..., "x": ...}` in `cases.json` would otherwise be
/// ambiguous), matched by field-name-set rather than threading a parameter
/// type through from the call site, since exactly one record type is
/// expected to match for any exemplo up through E03.
fn json_literal_for_cpp(value: &Value, records: &[ir::Record]) -> Option<String> {
    match value {
        // `Number::to_string()` preserves the JSON source's own form
        // (`7` stays `"7"`, `7.0` stays `"7.0"`) — both are valid C++/Dart
        // literal syntax, and int literals are usable directly where a
        // `double` parameter is expected in both languages, so this needs
        // no int/float branching.
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Object(fields) => {
            let record = find_matching_record(records, fields)?;
            let values = record
                .fields
                .iter()
                .map(|field| json_literal_for_cpp(fields.get(&field.name)?, records))
                .collect::<Option<Vec<_>>>()?
                .join(", ");
            // Valid as both a C++ aggregate-init (`Ponto{3.0, 4.0}`) and a
            // Dart positional-constructor call — `run_cpp_oracle` uses this
            // same literal text for C++ too, so it has to parse as both.
            Some(format!("{}{{{values}}}", record.name))
        }
        _ => None,
    }
}

fn json_literal_for_dart(value: &Value, records: &[ir::Record]) -> Option<String> {
    match value {
        Value::Object(fields) => {
            let record = find_matching_record(records, fields)?;
            let values = record
                .fields
                .iter()
                .map(|field| json_literal_for_dart(fields.get(&field.name)?, records))
                .collect::<Option<Vec<_>>>()?
                .join(", ");
            // Dart has no brace aggregate-init for a class — the
            // constructor `emit::dart::emit_record` generates
            // (`ClassName(this.field1, ...)`) takes the same
            // declared-order positional arguments though, so the call
            // syntax differs from C++'s but the value doesn't.
            Some(format!("{}({values})", record.name))
        }
        // Same shapes, same textual form as C++ for the ints/bools/doubles
        // E01–E02 use.
        _ => json_literal_for_cpp(value, records),
    }
}

fn find_matching_record<'a>(
    records: &'a [ir::Record],
    fields: &serde_json::Map<String, Value>,
) -> Option<&'a ir::Record> {
    records.iter().find(|record| {
        record.fields.len() == fields.len()
            && record
                .fields
                .iter()
                .all(|field| fields.contains_key(&field.name))
    })
}

fn json_canonical(value: &Value) -> Option<String> {
    match value {
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        _ => None,
    }
}

fn run_cpp_oracle(
    project_root: &Path,
    cases: &[OracleCase],
    records: &[ir::Record],
    scratch_dir: &Path,
) -> Result<Vec<String>, String> {
    let src_dir = project_root.join("src");
    let mut cpp_files = Vec::new();
    let mut header_names = Vec::new();
    collect_oracle_sources(&src_dir, &mut cpp_files, &mut header_names)?;
    cpp_files.sort();
    header_names.sort();

    let mut main_source = String::from("#include <iomanip>\n#include <iostream>\n");
    for header_name in &header_names {
        main_source.push_str(&format!("#include \"{header_name}\"\n"));
    }
    // `std::setprecision(15)` narrows (doesn't eliminate) the gap between
    // C++'s default 6-significant-digit `<<` formatting and Dart's
    // shortest-round-trip `double.toString()` — full cross-language float
    // equivalence is explicitly out of scope here (US-10's
    // `equivalence.rs`/bit-level comparison, not this harness); this just
    // keeps E02's one `double` case (`7.0 / 2.0`) from failing on a
    // formatting artifact that isn't a real divergence.
    main_source.push_str("\nint main() {\n    std::cout << std::setprecision(15);\n");
    for (index, case) in cases.iter().enumerate() {
        let args_literal = case
            .args
            .iter()
            .map(|arg| json_literal_for_cpp(arg, records))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                format!(
                    "case {index} ({}) has an argument type the oracle harness doesn't support yet",
                    case.funcao
                )
            })?
            .join(", ");
        match &case.espera {
            Value::Bool(_) => main_source.push_str(&format!(
                "    std::cout << ({}({args_literal}) ? \"true\" : \"false\") << \"\\n\";\n",
                case.funcao
            )),
            _ => main_source.push_str(&format!(
                "    std::cout << {}({args_literal}) << \"\\n\";\n",
                case.funcao
            )),
        }
    }
    main_source.push_str("    return 0;\n}\n");

    let main_path = scratch_dir.join("oracle_main.cpp");
    fs::write(&main_path, main_source).map_err(|error| error.to_string())?;

    let binary_path = scratch_dir.join("oracle_cpp_bin");
    let mut compile = Command::new("clang++");
    compile
        .arg("-std=c++17")
        .arg(format!("-I{}", src_dir.display()))
        .arg(&main_path);
    for cpp_file in &cpp_files {
        compile.arg(cpp_file);
    }
    compile.arg("-o").arg(&binary_path);
    run_command(&mut compile).map_err(|error| format!("compiling oracle C++ driver: {error}"))?;

    let output = run_command(&mut Command::new(&binary_path))
        .map_err(|error| format!("running oracle C++ binary: {error}"))?;
    Ok(stdout_lines(&output))
}

fn collect_oracle_sources(
    dir: &Path,
    cpp_files: &mut Vec<PathBuf>,
    header_names: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_oracle_sources(&path, cpp_files, header_names)?;
            continue;
        }
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("cpp") => cpp_files.push(path),
            Some("hpp") | Some("h") => {
                if let Some(name) = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                {
                    header_names.push(name);
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn run_dart_oracle(
    package: &transpile::TranspiledPackage,
    package_dir: &Path,
    cases: &[OracleCase],
    records: &[ir::Record],
    scratch_dir: &Path,
) -> Result<Vec<String>, String> {
    let dart_lib_files: Vec<&String> = package
        .files
        .keys()
        .filter(|path| path.starts_with("lib/") && path.ends_with(".dart"))
        .collect();

    let mut main_source = String::new();
    for lib_file in &dart_lib_files {
        let file_name = Path::new(lib_file)
            .file_name()
            .expect("lib/*.dart path always has a file name")
            .to_string_lossy();
        main_source.push_str(&format!("import '{file_name}';\n"));
    }
    main_source.push_str("\nvoid main() {\n");
    for (index, case) in cases.iter().enumerate() {
        let args_literal = case
            .args
            .iter()
            .map(|arg| json_literal_for_dart(arg, records))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| {
                format!(
                    "case {index} ({}) has an argument type the oracle harness doesn't support yet",
                    case.funcao
                )
            })?
            .join(", ");
        main_source.push_str(&format!("  print({}({args_literal}));\n", case.funcao));
    }
    main_source.push_str("}\n");

    let main_path = package_dir.join("lib").join("_oracle_main.dart");
    fs::write(&main_path, main_source).map_err(|error| error.to_string())?;
    let _ = scratch_dir; // reserved for future oracle scratch needs (E02+)

    let output = run_command(Command::new("dart").arg("run").arg(&main_path))
        .map_err(|error| format!("running oracle Dart driver: {error}"))?;
    Ok(stdout_lines(&output))
}

fn stdout_lines(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.to_owned())
        .collect()
}

/// `just examples-bless`: replaces `expected_dir`'s contents with `files`,
/// removing stale entries first so a renamed/removed generated file doesn't
/// leave an orphaned golden behind.
fn write_golden_files(expected_dir: &Path, files: &BTreeMap<String, String>) -> Result<(), String> {
    if expected_dir.is_dir() {
        fs::remove_dir_all(expected_dir).map_err(|error| error.to_string())?;
    }
    for (relative_path, contents) in files {
        let path = expected_dir.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&path, contents).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn read_golden_files(expected_dir: &Path) -> Option<BTreeMap<String, String>> {
    if !expected_dir.is_dir() {
        return None;
    }

    let mut files = BTreeMap::new();
    collect_golden_files(expected_dir, expected_dir, &mut files);
    Some(files)
}

fn collect_golden_files(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .collect();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            collect_golden_files(root, &path, files);
        } else if let Ok(contents) = fs::read_to_string(&path)
            && let Ok(relative) = path.strip_prefix(root)
        {
            files.insert(relative.to_string_lossy().replace('\\', "/"), contents);
        }
    }
}

#[test]
fn examples_corpus_reports_status_per_manifest() {
    let examples = discover_examples(&examples_root()).expect("discover examples/");
    assert!(
        !examples.is_empty(),
        "expected at least one example under examples/"
    );

    let mut regressions = Vec::new();
    for example in &examples {
        let outcome = run_example(example);
        match (&outcome, example.manifest.status) {
            (Outcome::Passed, ExampleStatus::EsperadoFalhar) => {
                regressions.push(format!(
                    "{}: marcado esperado-falhar mas passou — algo começou a \
                     funcionar por acidente; atualize example.toml para status = \"passa\"",
                    example.manifest.id
                ));
            }
            (Outcome::Failed(reason), ExampleStatus::Passa) => {
                regressions.push(format!(
                    "{}: marcado passa mas falhou: {reason}",
                    example.manifest.id
                ));
            }
            (Outcome::Failed(reason), ExampleStatus::EsperadoFalhar) => {
                eprintln!(
                    "[examples] {} não implementado (esperado): {reason}",
                    example.manifest.id
                );
            }
            (Outcome::Passed, ExampleStatus::Passa) => {
                eprintln!("[examples] {} passa", example.manifest.id);
            }
        }
    }

    assert!(
        regressions.is_empty(),
        "regressões no corpus de exemplos:\n{}",
        regressions.join("\n")
    );
}

/// Critério 5 de PR1: os três `input/` compilam com `cmake` + `clang++`,
/// ainda que nada seja transpilado. Prova que o toolchain está pronto para o
/// harness de oráculo que PR3 vai construir em cima do mesmo mecanismo.
#[test]
fn every_example_input_compiles_with_cmake_and_clang() {
    let examples = discover_examples(&examples_root()).expect("discover examples/");

    for example in &examples {
        let workspace = TempWorkspace::new(&format!("examples-cmake-{}", example.manifest.id))
            .expect("create temporary workspace");
        let build_dir = workspace.path().join("build");

        configure_and_build(&example.input_dir(), &build_dir).unwrap_or_else(|error| {
            panic!(
                "{} failed to compile with cmake + clang++: {error}",
                example.manifest.id
            )
        });
    }
}

#[test]
fn a_directory_without_example_toml_is_reported_not_silently_skipped() {
    let workspace =
        TempWorkspace::new("examples-missing-manifest").expect("create temporary workspace");
    fs::create_dir_all(workspace.path().join("EXX-sem-manifesto/input")).expect("create dir");

    let error = discover_examples(workspace.path()).expect_err("expected a discovery error");
    assert!(
        matches!(error, DiscoveryError::MissingManifest(ref name) if name == "EXX-sem-manifesto"),
        "unexpected error: {error}"
    );
}

#[test]
fn a_malformed_example_toml_names_the_example_in_the_error() {
    let workspace =
        TempWorkspace::new("examples-malformed-manifest").expect("create temporary workspace");
    let dir = workspace.path().join("EXX-malformado");
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(dir.join("example.toml"), "isto nao e uma linha valida\n").expect("write manifest");

    let error = discover_examples(workspace.path()).expect_err("expected a discovery error");
    assert!(
        format!("{error}").contains("EXX-malformado"),
        "error should name the offending example, got: {error}"
    );
}

#[test]
fn parses_a_well_formed_manifest() {
    let manifest = ExampleManifest::parse(
        r#"
id = "E01"
nome = "Função aritmética livre"
nivel = 1
status = "esperado-falhar"
motivo = "emissor Dart ainda não existe"
constroi = ["funcao-livre", "int", "expressao-binaria", "return"]
passos = ["US-7", "US-8", "US-9", "US-10"]
"#,
    )
    .expect("parse manifest");

    assert_eq!(manifest.id, "E01");
    assert_eq!(manifest.status, ExampleStatus::EsperadoFalhar);
    assert_eq!(
        manifest.constroi,
        vec!["funcao-livre", "int", "expressao-binaria", "return"]
    );
    assert_eq!(manifest.passos, vec!["US-7", "US-8", "US-9", "US-10"]);
}

/// `decisions.toml` reuses `example.toml`'s flat TOML subset — one decision
/// per file is all E03 needs (`Ponto` has exactly one viable option, so
/// there's no real choice to encode yet; multiple decisions per file, if a
/// later degrau needs them, is `decisions.toml`'s own problem to solve, not
/// retrofitted here speculatively).
fn read_decisions_toml(path: &Path) -> Result<Option<MappingDecision>, String> {
    if !path.is_file() {
        return Ok(None);
    }
    let contents = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let table = parse_toml_subset(&contents)?;
    Ok(Some(MappingDecision {
        type_usr: expect_string(&table, "type_usr")?,
        option_id: expect_string(&table, "option_id")?,
        decided_at: expect_string(&table, "decided_at")?,
    }))
}

/// PR5 criteria 5–7 of `docs/plans/primeiro-corte-e01-e03.md`: a decision
/// comes from `decisions.toml`, applied to `project.db` without going
/// through the UI, and survives the project being reopened. The persistence
/// half (round-trip, reopen) is already proven at the store layer
/// (`persistence::project_store::tests::round_trips_type_mappings` /
/// `reopening_the_project_preserves_the_recorded_type_mapping`); this test
/// closes the loop from the actual file to that layer, and cross-checks the
/// decision against `mapping::options_for`'s real output for the same type
/// (criterion 3), not a hand-written option id that could silently drift
/// from what the product would actually offer.
#[test]
fn e03_decisions_toml_applies_to_the_database_without_going_through_the_ui() {
    let examples = discover_examples(&examples_root()).expect("discover examples/");
    let e03 = examples
        .iter()
        .find(|example| example.manifest.id == "E03")
        .expect("E03 example should exist in examples/");

    let decision = read_decisions_toml(&e03.dir.join("decisions.toml"))
        .expect("read decisions.toml")
        .expect("E03 should have a decisions.toml");

    let workspace = TempWorkspace::new("e03-decisions-toml").expect("create temporary workspace");
    let db_path = workspace.path().join("project.db");
    let mut store = ProjectStore::open(&db_path).expect("open project store");
    store
        .set_type_mapping(&decision)
        .expect("apply decision to the database");
    drop(store);

    let reopened = ProjectStore::open(&db_path).expect("reopen project store");
    let persisted = reopened
        .list_type_mappings()
        .expect("list persisted type mappings");
    assert_eq!(
        persisted,
        vec![decision.clone()],
        "decision from decisions.toml should survive reopening the project"
    );

    // Criterion 3 cross-check: the recorded option must actually be one
    // `mapping::options_for` would offer for `Ponto` — not just any string.
    let ponto = ir_record_declaration_for(&e03.input_dir(), "Ponto");
    let options = mapping::options_for(&ponto, &[], &[]);
    assert_eq!(
        options.len(),
        1,
        "a struct without multiple inheritance should have exactly one option"
    );
    assert_eq!(options[0].id, decision.option_id);
}

/// Builds a minimal `TypeDeclaration` for `mapping::options_for` from a real
/// extraction — `mapping.rs` only needs `kind`/`name`/`usr` to decide, so
/// this doesn't need the whole `type_catalog` pass, just the one field it
/// reads off `ir::Record` (already extracted for the transpile step).
fn ir_record_declaration_for(
    input_dir: &Path,
    name: &str,
) -> syntax_bridge_server::type_catalog::TypeDeclaration {
    use syntax_bridge_server::type_catalog::{TypeDeclaration, TypeDeclarationKind};

    let record = find_ir_record(input_dir, name);
    TypeDeclaration {
        name: record.name,
        kind: TypeDeclarationKind::Struct,
        namespace: String::new(),
        file: record.origin.file,
        line: record.origin.line,
        column: record.origin.column,
        end_line: record.origin.line,
        end_column: record.origin.column,
        usr: record.usr,
    }
}

fn find_ir_record(input_dir: &Path, name: &str) -> ir::Record {
    let workspace = TempWorkspace::new("find-ir-record").expect("create temp workspace");
    let build_dir = workspace.path().join("build");
    run_command(
        Command::new("cmake")
            .arg("-S")
            .arg(input_dir)
            .arg("-B")
            .arg(&build_dir)
            .arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"),
    )
    .expect("cmake configure");
    let compilation_units =
        ingest::read_compilation_units(&build_dir.join("compile_commands.json"))
            .expect("read compile_commands.json");
    let project_root = input_dir
        .canonicalize()
        .unwrap_or_else(|_| input_dir.to_path_buf());
    let catalog =
        function_catalog::extract_function_catalog(&compilation_units, &project_root, None)
            .expect("extract function catalog");
    catalog
        .ir_records
        .into_iter()
        .find(|record| record.name == name)
        .unwrap_or_else(|| panic!("no record named {name} found"))
}

/// The mutation test §5.4 of `conversao-guiada-por-exemplos.md` requires:
/// "introduz uma divergência de propósito no emissor (trocar `+` por `-`) e
/// exige que o oráculo falhe com origem e valores esperado/obtido".
///
/// Literally patching `emit::dart::emit_binary_op` and recompiling mid-test
/// isn't practical inside one Rust test process, so this proves the same
/// observable outcome a different way: it runs E01's *real* C++ ground
/// truth (via `run_cpp_oracle`, unmodified) against a hand-sabotaged Dart
/// package whose `lib/aritmetica.dart` is byte-for-byte what
/// `emit::dart::emit_module` would produce if `BinaryOp::Add`'s `"+"`
/// mutated to `"-"` — then feeds both into `compare_oracle_outputs`, the
/// *actual* production comparison function (not a copy of its logic). A
/// suite that never sabotages anything doesn't prove its own oracle works;
/// this does.
#[test]
fn mutation_test_a_sabotaged_dart_emitter_is_caught_by_the_oracle() {
    let examples = discover_examples(&examples_root()).expect("discover examples/");
    let e01 = examples
        .iter()
        .find(|example| example.manifest.id == "E01")
        .expect("E01 example should exist in examples/");

    let cases = read_oracle_cases(&e01.dir.join("oracle").join("cases.json"))
        .expect("read E01's oracle/cases.json");

    let workspace = TempWorkspace::new("mutation-test-e01").expect("create temporary workspace");
    let project_root = e01
        .input_dir()
        .canonicalize()
        .unwrap_or_else(|_| e01.input_dir());

    let cpp_lines =
        run_cpp_oracle(&project_root, &cases, &[], workspace.path()).expect("run real C++ oracle");

    // Byte-for-byte what the emitter would produce with `+` mutated to `-`
    // in `emit_binary_op` — see this test's doc comment for why it's
    // hand-written instead of generated by a patched `emit::dart`.
    let sabotaged_package = transpile::TranspiledPackage {
        package_name: "e01_funcao_aritmetica".to_owned(),
        files: BTreeMap::from([(
            "lib/aritmetica.dart".to_owned(),
            "int soma(int a, int b) {\n  return a - b;\n}\n".to_owned(),
        )]),
    };
    let package_dir = workspace.path().join("sabotaged-dart-package");
    transpile::write_package(&sabotaged_package, &package_dir).expect("write sabotaged package");

    let dart_lines = run_dart_oracle(
        &sabotaged_package,
        &package_dir,
        &cases,
        &[],
        workspace.path(),
    )
    .expect("run sabotaged Dart oracle");

    let result = compare_oracle_outputs(&cases, &cpp_lines, &dart_lines, "mutation-test-origin");

    let Err(message) = result else {
        panic!(
            "expected the oracle to catch the sabotaged `+` -> `-` mutation, but it reported \
             success — cpp_lines={cpp_lines:?} dart_lines={dart_lines:?}"
        );
    };
    assert!(
        message.contains("mutation-test-origin"),
        "message should carry the origin, got: {message}"
    );
    // soma(2, 3): real C++ produces 5 (obtido esperado), sabotaged Dart
    // produces 2 - 3 = -1 (obtido) — both values must be visible.
    assert!(
        message.contains('5') && message.contains("-1"),
        "message should carry both the expected and obtained values, got: {message}"
    );
}

// ---------------------------------------------------------------------
// cmake + clang++ plumbing
// ---------------------------------------------------------------------

fn configure_and_build(source_dir: &Path, build_dir: &Path) -> Result<(), String> {
    run_command(
        Command::new("cmake")
            .arg("-S")
            .arg(source_dir)
            .arg("-B")
            .arg(build_dir)
            .arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"),
    )
    .map_err(|error| format!("cmake configure: {error}"))?;

    run_command(Command::new("cmake").arg("--build").arg(build_dir))
        .map_err(|error| format!("cmake --build: {error}"))?;

    Ok(())
}

fn run_command(command: &mut Command) -> Result<Output, String> {
    let output = command
        .output()
        .map_err(|error| format!("failed to spawn {command:?}: {error}"))?;

    if !output.status.success() {
        return Err(format!(
            "{command:?} exited with {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(output)
}

// ---------------------------------------------------------------------
// TempWorkspace — mirrors the pattern in tests/project_ingest.rs
// ---------------------------------------------------------------------

#[derive(Debug)]
struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(name: &str) -> io::Result<Self> {
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
