//! Orchestrates C++ → Dart transpilation: extract IR (reusing
//! `function_catalog`'s existing body-parsing pass, see `lower::cpp`'s
//! docs), emit Dart (`emit::dart`), and produce a package.
//!
//! Deliberately does not read or write any `project.db` — it only needs
//! `CompilationUnit`s and a project root, the same shape
//! `function_catalog::extract_function_catalog` itself takes. That keeps it
//! usable both from the HTTP route (which reads compilation units from a
//! persisted project) and from the `examples/` oracle harness (PR3), which
//! only has a `compile_commands.json` from configuring+building a bare
//! fixture — no ingested project at all.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

use crate::emit::dart::{self, sanitize_identifier};
use crate::function_catalog::{self, FunctionCatalogError};
use crate::ingest::CompilationUnit;
use crate::ir::Module;
use crate::mapping::{self, MappingDecision};
use crate::type_catalog::TypeDeclaration;

#[derive(Debug)]
pub enum TranspileError {
    LibclangUnavailable(String),
    Io(PathBuf, io::Error),
    /// `dart format` failed or wasn't found. `emit::dart` doesn't try to
    /// replicate `dart_style`'s line-wrapping decisions by hand — e.g. an
    /// `Unsupported` node's message can be arbitrarily long, and dart_style
    /// wraps a call onto multiple lines past its page width in a way that's
    /// error-prone to hand-copy exactly. Piping the emitted source through
    /// the real formatter (`--output=show`, reading stdin) is what
    /// guarantees criterion 5.2 ("já está no formato de `dart format`") for
    /// every case, not just the ones this module happened to test by hand.
    DartFormatFailed(String),
    /// A `MappingDecision` was recorded for `type_usr`, but `option_id`
    /// isn't among that type's real options (`mapping::options_for`) — a
    /// stale decision (the type's shape changed since it was recorded) or a
    /// hand-edited `decisions.toml` typo. Refusing instead of silently
    /// treating the type as "undecided" is what makes a recorded decision
    /// (or a broken one) an observable effect on the output, per AGENTS.md's
    /// "resolver o mapeamento de tipos... é o objetivo principal do produto"
    /// and §5's "silêncio é proibido".
    UnknownMappingOption {
        type_usr: String,
        type_name: String,
        option_id: String,
    },
}

impl TranspileError {
    pub fn is_client_error(&self) -> bool {
        false
    }
}

impl fmt::Display for TranspileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibclangUnavailable(message) => {
                write!(formatter, "libclang is unavailable: {message}")
            }
            Self::Io(path, error) => write!(formatter, "{}: {error}", path.display()),
            Self::DartFormatFailed(message) => write!(formatter, "dart format failed: {message}"),
            Self::UnknownMappingOption {
                type_usr,
                type_name,
                option_id,
            } => write!(
                formatter,
                "recorded mapping decision for `{type_name}` ({type_usr}) references \
                 unknown option `{option_id}`"
            ),
        }
    }
}

impl std::error::Error for TranspileError {}

impl From<FunctionCatalogError> for TranspileError {
    fn from(error: FunctionCatalogError) -> Self {
        match error {
            FunctionCatalogError::LibclangUnavailable(message) => {
                Self::LibclangUnavailable(message)
            }
            // Transpilation never passes a `Cancellation` token in yet (the
            // route is synchronous — PR2 decision, see
            // docs/plans/primeiro-corte-e01-e03.md §7 PR2), so
            // `function_catalog` never actually produces this variant here.
            FunctionCatalogError::Cancelled => {
                Self::LibclangUnavailable("transpilation was cancelled".to_owned())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TranspiledPackage {
    pub package_name: String,
    /// Package-relative path (`"pubspec.yaml"`, `"lib/aritmetica.dart"`) →
    /// file contents. A `BTreeMap` so iteration order — and therefore
    /// `write_package`'s output — is deterministic (§5 restriction 5).
    pub files: BTreeMap<String, String>,
}

/// Extracts IR and emits a Dart package for `compilation_units`.
/// `package_name` is sanitized into a valid Dart package identifier — it
/// need not already be one (see `sanitize_identifier`). Equivalent to
/// [`transpile_with_mappings`] with no type catalog and no recorded
/// decisions — every current example fixture is a free function or a
/// `struct`/`class`, and `mapping::options_for` never offers more than one
/// option for either, so there is nothing yet for an empty catalog/decisions
/// to change here.
pub fn transpile(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    package_name: &str,
) -> Result<TranspiledPackage, TranspileError> {
    transpile_with_mappings(compilation_units, project_root, package_name, &[], &[])
}

/// Like [`transpile`], but also consults `mapping::options_for` for every
/// emitted `struct`/`class` against `type_catalog`/`decisions`: if a
/// `MappingDecision` was recorded for a type, its `option_id` must be among
/// that type's real options, or transpilation refuses
/// (`TranspileError::UnknownMappingOption`) instead of silently treating the
/// (invalid) decision as if it were never recorded.
pub fn transpile_with_mappings(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    package_name: &str,
    type_catalog: &[TypeDeclaration],
    decisions: &[MappingDecision],
) -> Result<TranspiledPackage, TranspileError> {
    let catalog =
        function_catalog::extract_function_catalog(compilation_units, project_root, None)?;
    let module = Module {
        functions: catalog.ir_functions,
        records: catalog.ir_records,
    };

    emit_package(&module, package_name, type_catalog, decisions)
}

/// Emits a Dart package from an already-built [`Module`] — the second half
/// of [`transpile_with_mappings`] (mapping validation, `emit::dart`,
/// `dart format`, `pubspec.yaml`), split out so a caller that already has
/// the IR (`project_service::transpile_project`, reading it back from
/// `ProjectStore` instead of reparsing every compilation unit with
/// `libclang` on every request) doesn't have to go through
/// `function_catalog::extract_function_catalog` a second time.
pub fn emit_package(
    module: &Module,
    package_name: &str,
    type_catalog: &[TypeDeclaration],
    decisions: &[MappingDecision],
) -> Result<TranspiledPackage, TranspileError> {
    for record in &module.records {
        let Some(declaration) = type_catalog.iter().find(|decl| decl.usr == record.usr) else {
            continue;
        };
        let Some(decision) = decisions.iter().find(|d| d.type_usr == record.usr) else {
            continue;
        };
        let options = mapping::options_for(declaration, type_catalog, decisions);
        if !options.iter().any(|option| option.id == decision.option_id) {
            return Err(TranspileError::UnknownMappingOption {
                type_usr: record.usr.clone(),
                type_name: record.name.clone(),
                option_id: decision.option_id.clone(),
            });
        }
    }

    let mut files = dart::emit_module(module);
    for contents in files.values_mut() {
        *contents = format_dart_source(contents)?;
    }

    let sanitized_name = sanitized_package_name(package_name);
    files.insert("pubspec.yaml".to_owned(), emit_pubspec(&sanitized_name));

    Ok(TranspiledPackage {
        package_name: sanitized_name,
        files,
    })
}

/// Formats one `.dart` file's source through the real `dart format`,
/// reading from stdin and writing to stdout (`--output=show` with no path
/// argument) rather than touching disk — see `TranspileError::DartFormatFailed`
/// for why this exists instead of a hand-rolled line-wrapping pass.
fn format_dart_source(source: &str) -> Result<String, TranspileError> {
    let mut child = Command::new("dart")
        .arg("format")
        .arg("--output=show")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            TranspileError::DartFormatFailed(format!("failed to spawn `dart format`: {error}"))
        })?;

    let mut stdin = child
        .stdin
        .take()
        .expect("stdin was requested as piped above");
    let source = source.to_owned();
    // Write on a separate thread, concurrently with `wait_with_output`
    // reading stdout/stderr below. Writing all of stdin first and only then
    // reading output would deadlock against a child that blocks writing
    // output (pipe buffer full) before it's done reading input — true of
    // `dart format` on today's behavior only because it happens to consume
    // all of stdin before producing any output; not a guarantee worth
    // depending on.
    let writer = std::thread::spawn(move || stdin.write_all(source.as_bytes()));

    let output = child.wait_with_output().map_err(|error| {
        TranspileError::DartFormatFailed(format!("failed to read `dart format`'s output: {error}"))
    })?;

    let write_result = writer
        .join()
        .map_err(|_| TranspileError::DartFormatFailed("stdin writer thread panicked".to_owned()))?;

    if !output.status.success() {
        return Err(TranspileError::DartFormatFailed(format!(
            "exited with {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    write_result.map_err(|error| {
        TranspileError::DartFormatFailed(format!(
            "failed to write to `dart format`'s stdin: {error}"
        ))
    })?;

    String::from_utf8(output.stdout)
        .map_err(|error| TranspileError::DartFormatFailed(format!("non-UTF-8 output: {error}")))
}

fn sanitized_package_name(name: &str) -> String {
    let sanitized = sanitize_identifier(name);
    if sanitized.is_empty() {
        "syntax_bridge_output".to_owned()
    } else {
        sanitized
    }
}

fn emit_pubspec(package_name: &str) -> String {
    format!("name: {package_name}\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n")
}

/// Writes every file in `package.files` under `output_dir`, creating parent
/// directories as needed. `output_dir` is cleared first — otherwise a file
/// left over from a previous transpile of the same project (e.g. a
/// `lib/<stem>.dart` for a C++ source file since renamed or removed) would
/// survive on disk alongside the new output, no longer matching the
/// project's current source.
pub fn write_package(package: &TranspiledPackage, output_dir: &Path) -> Result<(), TranspileError> {
    if output_dir.is_dir() {
        fs::remove_dir_all(output_dir)
            .map_err(|error| TranspileError::Io(output_dir.to_path_buf(), error))?;
    }

    for (relative_path, contents) in &package.files {
        let path = output_dir.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| TranspileError::Io(parent.to_path_buf(), error))?;
        }
        fs::write(&path, contents).map_err(|error| TranspileError::Io(path.clone(), error))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for a pipe deadlock: `format_dart_source` used to
    /// write all of `source` to `dart format`'s stdin, blocking, before ever
    /// reading its stdout/stderr. `dart format` itself blocks writing its
    /// output once the OS pipe buffer (commonly 64KiB on Linux) fills up,
    /// which happens for large-enough inputs — the two blocked writes
    /// deadlock each other forever. Bounded with a timeout so a regression
    /// fails loudly instead of hanging the test suite.
    #[test]
    fn formatting_a_source_larger_than_the_pipe_buffer_does_not_deadlock() {
        let mut source = String::new();
        for index in 0..80_000 {
            source.push_str(&format!("int f{index}() => 0;\n"));
        }
        assert!(
            source.len() > 128 * 1024,
            "fixture must exceed the OS pipe buffer size"
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = format_dart_source(&source);
            let _ = sender.send(result);
        });

        match receiver.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(result) => {
                result.expect("format_dart_source should succeed on valid Dart source");
            }
            Err(_) => {
                panic!("format_dart_source did not return within 30s — likely a pipe deadlock")
            }
        }
    }
}
