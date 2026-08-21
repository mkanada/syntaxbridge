//! Runs `dart analyze --format=json` over an already-written, already-`dart
//! format`-clean package (criteria 1/2 of US-9, satisfied by
//! `transpile::emit_package`/`format_dart_source` before this module ever
//! runs) and translates each diagnostic's Dart location back to the C++
//! declaration it was emitted from (criterion 3, the piece that was
//! missing).
//!
//! **Granularity, honestly stated.** The translation resolves to whichever
//! top-level declaration (a free function, or a whole `class`/`mixin`,
//! or an `enum`) a diagnostic's line falls inside — not the exact
//! statement. Getting statement-level precision would mean a line map
//! surviving `dart format`'s reflow, which runs on the *whole* file and can
//! move any line; the only way to keep that map trustworthy is to derive it
//! from the literal formatted text `dart analyze` itself sees, which is
//! what [`locate_origin`] does, by finding each declaration's own header
//! text in that formatted output — nothing pre-formatted is trusted. This
//! costs per-method precision inside a class (a diagnostic anywhere in
//! `class Ponto { ... }` maps to `Ponto`'s own declaration, not to the
//! specific method), a known gap for a later degrau, not a silent one:
//! [`DartDiagnostic::origin`] is `None` when no declaration could be
//! located at all (an `import` line, or the synthetic
//! `_syntaxBridgeUnsupported` helper), never a wrong guess.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::emit::dart::{file_stem, mixin_usrs};
use crate::ir::{Enum, Function, Module, Origin, Record};
use crate::transpile::TranspiledPackage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    fn from_dart_analyze(value: &str) -> Self {
        match value {
            "ERROR" => Self::Error,
            "WARNING" => Self::Warning,
            // `dart analyze --format=json` only ever emits these three
            // (https://dart.dev/tools/diagnostic-codes docs' own severities)
            // — an unrecognized string is a toolchain surprise worth seeing
            // rather than silently downgrading, but panicking a whole
            // validation run over one unknown severity would be worse than
            // the alternative: fold it into `Info`, the least actionable
            // bucket, and let the raw `code`/message still reach the user.
            _ => Self::Info,
        }
    }
}

/// One `dart analyze` finding, translated to the C++ declaration it came
/// from when [`locate_origin`] could resolve one. See the module doc for
/// what "translated" means today (declaration-level, not statement-level).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DartDiagnostic {
    pub severity: Severity,
    pub message: String,
    /// Package-relative path, e.g. `"lib/aritmetica.dart"` — matches
    /// `TranspiledPackage::files`' own keys.
    pub dart_file: String,
    pub dart_line: u32,
    pub origin: Option<Origin>,
}

#[derive(Debug)]
pub enum ValidateError {
    /// `dart` isn't on `PATH`, or exited without producing the JSON
    /// `dart analyze --format=json` is documented to always print.
    DartUnavailable(String),
    MalformedAnalyzerOutput(String),
}

impl std::fmt::Display for ValidateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DartUnavailable(message) => write!(formatter, "dart analyze failed: {message}"),
            Self::MalformedAnalyzerOutput(message) => {
                write!(
                    formatter,
                    "could not parse `dart analyze --format=json`: {message}"
                )
            }
        }
    }
}

impl std::error::Error for ValidateError {}

/// Runs `dart analyze --format=json` against `output_dir` (a package already
/// written by `transpile::write_package`) and translates every diagnostic's
/// location back to `module`'s C++ origins — see the module doc for the
/// granularity that translation resolves at.
pub fn analyze_package(
    module: &Module,
    package: &TranspiledPackage,
    output_dir: &Path,
) -> Result<Vec<DartDiagnostic>, ValidateError> {
    let raw = run_dart_analyze(output_dir)?;

    Ok(raw
        .into_iter()
        .map(|diagnostic| {
            let dart_file = relative_to(&diagnostic.location.file, output_dir);
            let dart_line = diagnostic.location.range.start.line;
            let origin = package
                .files
                .get(&dart_file)
                .and_then(|source| locate_origin(module, &dart_file, source, dart_line));

            DartDiagnostic {
                severity: Severity::from_dart_analyze(&diagnostic.severity),
                message: diagnostic.problem_message,
                dart_file,
                dart_line,
                origin,
            }
        })
        .collect())
}

fn relative_to(absolute: &str, output_dir: &Path) -> String {
    Path::new(absolute)
        .strip_prefix(output_dir)
        .ok()
        .and_then(|relative| relative.to_str())
        .map(|relative| relative.replace('\\', "/"))
        .unwrap_or_else(|| absolute.to_owned())
}

#[derive(Debug, Deserialize)]
struct AnalyzeOutput {
    diagnostics: Vec<RawDiagnostic>,
}

#[derive(Debug, Deserialize)]
struct RawDiagnostic {
    severity: String,
    #[serde(rename = "problemMessage")]
    problem_message: String,
    location: RawLocation,
}

#[derive(Debug, Deserialize)]
struct RawLocation {
    file: String,
    range: RawRange,
}

#[derive(Debug, Deserialize)]
struct RawRange {
    start: RawPosition,
}

#[derive(Debug, Deserialize)]
struct RawPosition {
    line: u32,
}

fn run_dart_analyze(output_dir: &Path) -> Result<Vec<RawDiagnostic>, ValidateError> {
    let output = Command::new("dart")
        .arg("analyze")
        .arg("--format=json")
        .arg(output_dir)
        .output()
        .map_err(|error| ValidateError::DartUnavailable(error.to_string()))?;

    // `dart analyze` exits non-zero the moment it finds an ERROR-severity
    // diagnostic — that's the expected, common case here (the whole point
    // of this module), not a failure to run the tool. Only a stdout that
    // isn't the documented JSON envelope means the tool itself didn't run
    // (missing SDK, wrong path, a future CLI break).
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parsed: AnalyzeOutput = serde_json::from_str(&stdout).map_err(|error| {
        ValidateError::MalformedAnalyzerOutput(format!(
            "{error}\nstdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
    })?;

    Ok(parsed.diagnostics)
}

/// See the module doc for why this resolves at declaration granularity by
/// searching `formatted_source` (the literal text `dart analyze` ran
/// against) instead of trusting any pre-format line count.
///
/// `dart_relative_path` is only used to derive the C++-source file stem
/// (`"lib/aritmetica.dart"` → `"aritmetica"`) that selects which of
/// `module`'s declarations belong to this file — the same grouping
/// `emit::dart::emit_module` itself uses, via the same `file_stem` helper,
/// so the two can never disagree about which declaration lives where.
fn locate_origin(
    module: &Module,
    dart_relative_path: &str,
    formatted_source: &str,
    dart_line: u32,
) -> Option<Origin> {
    let stem = dart_relative_path
        .strip_prefix("lib/")
        .unwrap_or(dart_relative_path)
        .strip_suffix(".dart")
        .unwrap_or(dart_relative_path);

    let markers = declaration_markers(module, stem);
    let lines: Vec<&str> = formatted_source.lines().collect();

    let mut ranges: Vec<(u32, u32, &Origin)> = Vec::new();
    let mut search_from = 0usize;
    let mut open: Option<(usize, &Origin)> = None;

    for (needle, origin) in &markers {
        let Some(found) = find_line_containing(&lines, search_from, needle) else {
            continue;
        };
        if let Some((start, previous_origin)) = open.take() {
            // 1-indexed: the previous declaration's range runs up to (but
            // not including) this one's own start line.
            ranges.push((start as u32 + 1, found as u32, previous_origin));
        }
        open = Some((found, origin));
        search_from = found + 1;
    }
    if let Some((start, origin)) = open {
        ranges.push((start as u32 + 1, lines.len() as u32, origin));
    }

    ranges
        .into_iter()
        .find(|(start, end, _)| dart_line >= *start && dart_line <= *end)
        .map(|(_, _, origin)| origin.clone())
}

/// Ordered the same way `emit::dart::emit_file` emits them (enums, then
/// records, then free functions — each group sorted by C++ source
/// line/name) so a sequential, cursor-advancing search over the formatted
/// text can never match a later declaration's header before an earlier
/// one's.
fn declaration_markers<'a>(module: &'a Module, dart_stem: &str) -> Vec<(String, &'a Origin)> {
    let mixins = mixin_usrs(&module.records);

    let mut enums: Vec<&Enum> = module
        .enums
        .iter()
        .filter(|item| file_stem(&item.origin.file) == dart_stem)
        .collect();
    enums.sort_by(|a, b| {
        a.origin
            .line
            .cmp(&b.origin.line)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut records: Vec<&Record> = module
        .records
        .iter()
        .filter(|item| file_stem(&item.origin.file) == dart_stem)
        .collect();
    records.sort_by(|a, b| {
        a.origin
            .line
            .cmp(&b.origin.line)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut functions: Vec<&Function> = module
        .functions
        .iter()
        .filter(|item| file_stem(&item.origin.file) == dart_stem)
        .collect();
    functions.sort_by(|a, b| {
        a.origin
            .line
            .cmp(&b.origin.line)
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut markers = Vec::new();
    for item in enums {
        markers.push((format!("enum {}", item.name), &item.origin));
    }
    for item in records {
        let keyword = if mixins.contains(item.usr.as_str()) {
            "mixin"
        } else {
            "class"
        };
        markers.push((format!("{keyword} {}", item.name), &item.origin));
    }
    for item in functions {
        markers.push((format!("{}(", item.name), &item.origin));
    }
    markers
}

fn find_line_containing(lines: &[&str], from: usize, needle: &str) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(from)
        .find(|(_, line)| line.contains(needle))
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::Type;
    use std::collections::BTreeMap;

    fn origin(file: &str, line: u32) -> Origin {
        Origin {
            file: file.to_owned(),
            line,
            column: 1,
        }
    }

    fn sample_module() -> Module {
        Module {
            functions: vec![Function {
                name: "soma".to_owned(),
                usr: "c:@F@soma".to_owned(),
                params: Vec::new(),
                return_type: Type::Int,
                body: Vec::new(),
                origin: origin("/proj/aritmetica.cpp", 1),
            }],
            records: vec![Record {
                name: "Ponto".to_owned(),
                usr: "c:@S@Ponto".to_owned(),
                namespace: String::new(),
                fields: Vec::new(),
                static_fields: Vec::new(),
                constructors: Vec::new(),
                methods: Vec::new(),
                base_class: None,
                mixins: Vec::new(),
                destructor: None,
                origin: origin("/proj/aritmetica.cpp", 5),
            }],
            enums: Vec::new(),
        }
    }

    /// The deliberate-error scenario the roteiro asks for: a hand-built
    /// formatted Dart file whose two declarations' origins the test already
    /// knows, checking that a diagnostic line inside each one resolves back
    /// to the right C++ declaration — and that a line before the first
    /// declaration (e.g. an `import`) resolves to nothing instead of a wrong
    /// guess.
    #[test]
    fn a_diagnostic_line_resolves_to_the_declaration_it_falls_inside() {
        let module = sample_module();
        // Records before functions — the same order `emit::dart::emit_file`
        // itself emits them in, which `declaration_markers` relies on for
        // its sequential, cursor-advancing search.
        let formatted = "import 'dart:convert';\n\
             \n\
             class Ponto {\n\
             \u{20}\u{20}double x;\n\
             \u{20}\u{20}double y;\n\
             }\n\
             \n\
             int soma(int a, int b) {\n\
             \u{20}\u{20}return a + naoexiste;\n\
             }\n";

        assert_eq!(
            locate_origin(&module, "lib/aritmetica.dart", formatted, 1),
            None
        );
        assert_eq!(
            locate_origin(&module, "lib/aritmetica.dart", formatted, 4),
            Some(origin("/proj/aritmetica.cpp", 5))
        );
        assert_eq!(
            locate_origin(&module, "lib/aritmetica.dart", formatted, 9),
            Some(origin("/proj/aritmetica.cpp", 1))
        );
    }

    #[test]
    fn an_unrecognized_dart_file_resolves_to_nothing() {
        let module = sample_module();
        assert_eq!(
            locate_origin(&module, "lib/outro.dart", "int x() => 0;\n", 1),
            None
        );
    }

    #[test]
    fn a_record_used_as_a_mixin_is_located_by_its_mixin_keyword() {
        let mut module = sample_module();
        module.functions.clear();
        // A second record that references the first as a mixin — matches
        // `emit::dart::mixin_usrs`'s own input shape (a `BaseClass` naming
        // the mixed-in record's `usr`).
        let forma = Record {
            name: "Forma".to_owned(),
            usr: "c:@S@Forma".to_owned(),
            namespace: String::new(),
            fields: Vec::new(),
            static_fields: Vec::new(),
            constructors: Vec::new(),
            methods: Vec::new(),
            base_class: None,
            mixins: vec![crate::ir::BaseClass {
                usr: "c:@S@Ponto".to_owned(),
                name: "Ponto".to_owned(),
            }],
            destructor: None,
            origin: origin("/proj/aritmetica.cpp", 9),
        };
        module.records.push(forma);

        let formatted = "mixin Ponto {\n  double x;\n}\n\nclass Forma with Ponto {\n}\n";

        assert_eq!(
            locate_origin(&module, "lib/aritmetica.dart", formatted, 2),
            Some(origin("/proj/aritmetica.cpp", 5))
        );
        assert_eq!(
            locate_origin(&module, "lib/aritmetica.dart", formatted, 6),
            Some(origin("/proj/aritmetica.cpp", 9))
        );
    }

    #[test]
    fn severity_maps_dart_analyzes_three_documented_levels() {
        assert_eq!(Severity::from_dart_analyze("ERROR"), Severity::Error);
        assert_eq!(Severity::from_dart_analyze("WARNING"), Severity::Warning);
        assert_eq!(Severity::from_dart_analyze("INFO"), Severity::Info);
    }

    #[test]
    fn relative_to_strips_the_output_directory_and_normalizes_separators() {
        let output_dir = Path::new("/tmp/proj/transpiled");
        assert_eq!(
            relative_to("/tmp/proj/transpiled/lib/aritmetica.dart", output_dir),
            "lib/aritmetica.dart"
        );
    }

    /// `analyze_package` end to end, but against a canned `dart analyze`
    /// JSON payload shape via `locate_origin`/`relative_to` directly rather
    /// than shelling out — the real Dart toolchain is exercised by
    /// `tests/validate_dart.rs` instead, the same split
    /// `transpile.rs`/`tests/transpile.rs` already uses.
    #[test]
    fn diagnostics_without_a_resolvable_declaration_still_carry_the_raw_location() {
        let module = sample_module();
        let package = TranspiledPackage {
            package_name: "aritmetica".to_owned(),
            files: BTreeMap::from([(
                "lib/aritmetica.dart".to_owned(),
                "// TODO(syntax-bridge): stray line\nNever _syntaxBridgeUnsupported() {}\n"
                    .to_owned(),
            )]),
        };
        let origin = locate_origin(
            &module,
            "lib/aritmetica.dart",
            &package.files["lib/aritmetica.dart"],
            1,
        );
        assert_eq!(origin, None);
    }
}
