//! End-to-end transpile test against a real `libclang` parse — mirrors
//! `tests/lower_cpp.rs`'s fixture style, but exercises the full
//! `transpile::transpile` → `transpile::write_package` path and validates
//! the result with the real Dart toolchain (`dart analyze` +
//! `dart format --set-exit-if-changed`), the way
//! `tests/toolchain_availability.rs::dart_sdk_analyzes_and_runs_a_small_dart_program`
//! already does for a hand-written fixture.
//!
//! This is PR2's proof for `docs/plans/primeiro-corte-e01-e03.md`'s E01
//! completion criteria 1–4 and 7 (criterion 6, the `Unsupported` escape
//! hatch, is covered by `tests/emit_dart.rs` instead — no Dart toolchain
//! needed there).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::ingest::CompilationUnit;
use syntax_bridge_server::mapping::MappingDecision;
use syntax_bridge_server::transpile::{self, TranspileError};
use syntax_bridge_server::type_catalog::{TypeDeclaration, TypeDeclarationKind};

/// Identical to `examples/E01-funcao-aritmetica/input/src/aritmetica.cpp`.
const ARITMETICA_CPP: &str = r#"
int soma(int a, int b) {
    return a + b;
}
"#;

fn write_fixture(project_root: &Path) -> CompilationUnit {
    fs::create_dir_all(project_root).expect("create project dir");
    let file_path = project_root.join("aritmetica.cpp");
    fs::write(&file_path, ARITMETICA_CPP).expect("write aritmetica.cpp");

    CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    }
}

#[test]
fn transpiles_a_free_function_into_a_dart_package() {
    let workspace = TempWorkspace::new("transpile-e01").expect("create temporary workspace");
    let unit = write_fixture(workspace.path());

    let package = transpile::transpile(&[unit], workspace.path(), "E01 Função Aritmética")
        .expect("transpile");

    assert_eq!(package.package_name, "e01_funcao_aritmetica");
    assert_eq!(
        package.files["lib/aritmetica.dart"],
        "int soma(int a, int b) {\n  return a + b;\n}\n"
    );
    assert!(package.files["pubspec.yaml"].starts_with("name: e01_funcao_aritmetica\n"));
}

/// Criterion 3 of PR2 (US-8 criterion 3): transpiling twice with the same
/// input produces byte-identical output.
#[test]
fn transpiling_twice_produces_byte_identical_output() {
    let workspace = TempWorkspace::new("transpile-determinism").expect("create temp workspace");
    let unit = write_fixture(workspace.path());

    let units = [unit];
    let first = transpile::transpile(&units, workspace.path(), "e01").expect("first transpile");
    let second = transpile::transpile(&units, workspace.path(), "e01").expect("second transpile");

    assert_eq!(first.files, second.files);
}

/// Criterion 4 of PR2 (US-8 criterion 4): every declaration is traceable to
/// its C++ origin — checked here at the IR boundary that `emit::dart`
/// consumes, via the origin embedded in the `Unsupported` escape hatch.
/// `tests/lower_cpp.rs` already proves origins are populated on the IR
/// itself; this test proves the written package is real, valid Dart on top
/// of that IR.
#[test]
fn the_written_package_passes_dart_analyze_and_is_already_dart_format_clean() {
    let workspace = TempWorkspace::new("transpile-dart-toolchain").expect("create temp workspace");
    let unit = write_fixture(workspace.path());

    let package = transpile::transpile(&[unit], workspace.path(), "e01").expect("transpile");

    let output_dir = workspace.path().join("dart-package");
    transpile::write_package(&package, &output_dir).expect("write package");

    let analyze = Command::new("dart")
        .arg("analyze")
        .arg(&output_dir)
        .output()
        .expect("run dart analyze");
    assert_success(analyze);

    let format_check = Command::new("dart")
        .arg("format")
        .arg("--output=none")
        .arg("--set-exit-if-changed")
        .arg(&output_dir)
        .output()
        .expect("run dart format --set-exit-if-changed");
    assert_success(format_check);
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Regression test: an `Unsupported` node's message embeds an absolute file
/// path plus a reason, easily over `dart format`'s 80-column page width —
/// long enough that `dart_style` wraps the call onto multiple lines. Caught
/// by hand (a temp-workspace path was long enough to trip it) before this
/// test existed; `transpile::transpile` piping every file through the real
/// `dart format` (see `format_dart_source`) is what fixed it, instead of
/// `emit::dart` trying to hand-replicate `dart_style`'s wrapping decision.
#[test]
fn a_long_unsupported_message_still_produces_dart_format_clean_output() {
    // `break;` — still genuinely unsupported after E02 (PR4), which lowers
    // `while`/`if`/`DeclStmt` themselves but not loop-control statements.
    // The while-loop this test used before E02 landed now fully lowers
    // (progress, not a bug), so it stopped exercising this regression.
    const BREAK_CPP: &str = r#"
int primeiro_maior_que(int limite) {
    int i = 0;
    while (true) {
        if (i > limite) {
            break;
        }
        i = i + 1;
    }
    return i;
}
"#;

    let workspace =
        TempWorkspace::new("transpile-unsupported-format").expect("create temp workspace");
    fs::create_dir_all(workspace.path()).expect("create project dir");
    let file_path = workspace.path().join("controle.cpp");
    fs::write(&file_path, BREAK_CPP).expect("write controle.cpp");
    let unit = CompilationUnit {
        directory: workspace.path().display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let package =
        transpile::transpile(&[unit], workspace.path(), "e02").expect("transpile with break");
    let source = &package.files["lib/controle.dart"];
    assert!(
        source.contains("UnimplementedError"),
        "expected an Unsupported escape hatch in the output, got:\n{source}"
    );

    let output_dir = workspace.path().join("dart-package");
    transpile::write_package(&package, &output_dir).expect("write package");

    let analyze = Command::new("dart")
        .arg("analyze")
        .arg(&output_dir)
        .output()
        .expect("run dart analyze");
    assert_success(analyze);

    let format_check = Command::new("dart")
        .arg("format")
        .arg("--output=none")
        .arg("--set-exit-if-changed")
        .arg(&output_dir)
        .output()
        .expect("run dart format --set-exit-if-changed");
    assert_success(format_check);
}

/// Regression test for the int→double promotion fix in `lower::cpp`
/// (`Expr::Convert`): confirms the emitted `.toDouble()` call is real,
/// `dart analyze`/`dart format --set-exit-if-changed`-clean Dart, not just
/// IR that looks right.
#[test]
fn an_int_to_double_promotion_transpiles_to_dart_analyze_clean_code() {
    const MEDIA_CPP: &str = r#"
double media(int total) {
    double resultado = total;
    return resultado;
}
"#;
    let workspace = TempWorkspace::new("transpile-int-to-double").expect("create temp workspace");
    fs::create_dir_all(workspace.path()).expect("create project dir");
    let file_path = workspace.path().join("media.cpp");
    fs::write(&file_path, MEDIA_CPP).expect("write media.cpp");
    let unit = CompilationUnit {
        directory: workspace.path().display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let package = transpile::transpile(&[unit], workspace.path(), "media").expect("transpile");
    let source = &package.files["lib/media.dart"];
    assert!(
        source.contains("total.toDouble()"),
        "expected the promotion to survive as an explicit conversion, got:\n{source}"
    );
    assert!(
        !source.contains("UnimplementedError"),
        "the promotion is fully representable and must not bail out, got:\n{source}"
    );

    let output_dir = workspace.path().join("dart-package");
    transpile::write_package(&package, &output_dir).expect("write package");

    let analyze = Command::new("dart")
        .arg("analyze")
        .arg(&output_dir)
        .output()
        .expect("run dart analyze");
    assert_success(analyze);

    let format_check = Command::new("dart")
        .arg("format")
        .arg("--output=none")
        .arg("--set-exit-if-changed")
        .arg(&output_dir)
        .output()
        .expect("run dart format --set-exit-if-changed");
    assert_success(format_check);
}

/// Regression test: `transpile`/`transpile_project` never consulted
/// `mapping::options_for` or the persisted `MappingDecision`s at all — a
/// decision recorded for a type (including a stale or typo'd one, e.g.
/// after hand-editing `decisions.toml`, or a type that got renamed after
/// the decision was recorded) had zero effect and was never even read.
/// `transpile_with_mappings` must at least notice when a recorded decision
/// doesn't match any of the type's real options, and refuse instead of
/// silently proceeding as if nothing had been decided.
#[test]
fn a_recorded_mapping_decision_with_an_unknown_option_id_is_rejected() {
    const PONTO_CPP: &str = r#"
struct Ponto {
    double x;
    double y;
};
"#;
    let workspace = TempWorkspace::new("transpile-bad-mapping").expect("create temp workspace");
    fs::create_dir_all(workspace.path()).expect("create project dir");
    let file_path = workspace.path().join("ponto.cpp");
    fs::write(&file_path, PONTO_CPP).expect("write ponto.cpp");
    let unit = CompilationUnit {
        directory: workspace.path().display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let type_catalog = vec![TypeDeclaration {
        name: "Ponto".to_owned(),
        kind: TypeDeclarationKind::Struct,
        namespace: String::new(),
        file: file_path.display().to_string(),
        line: 2,
        column: 8,
        end_line: 5,
        end_column: 2,
        usr: "c:@S@Ponto".to_owned(),
    }];
    let decisions = vec![MappingDecision {
        type_usr: "c:@S@Ponto".to_owned(),
        option_id: "opcao-que-nao-existe".to_owned(),
        decided_at: "2026-01-01T00:00:00Z".to_owned(),
    }];

    let result = transpile::transpile_with_mappings(
        &[unit],
        workspace.path(),
        "ponto",
        &type_catalog,
        &decisions,
    );

    match result {
        Err(TranspileError::UnknownMappingOption {
            type_usr,
            option_id,
            ..
        }) => {
            assert_eq!(type_usr, "c:@S@Ponto");
            assert_eq!(option_id, "opcao-que-nao-existe");
        }
        other => panic!("expected TranspileError::UnknownMappingOption, got {other:?}"),
    }
}

/// The same fixture with a *valid* decision (matching `mapping::options_for`'s
/// one real option for a struct) must transpile exactly as it would with no
/// decision at all — recording the (only) correct choice is not an error.
#[test]
fn a_recorded_mapping_decision_with_a_valid_option_id_transpiles_normally() {
    const PONTO_CPP: &str = r#"
struct Ponto {
    double x;
    double y;
};
"#;
    let workspace = TempWorkspace::new("transpile-good-mapping").expect("create temp workspace");
    fs::create_dir_all(workspace.path()).expect("create project dir");
    let file_path = workspace.path().join("ponto.cpp");
    fs::write(&file_path, PONTO_CPP).expect("write ponto.cpp");
    let unit = CompilationUnit {
        directory: workspace.path().display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };

    let type_catalog = vec![TypeDeclaration {
        name: "Ponto".to_owned(),
        kind: TypeDeclarationKind::Struct,
        namespace: String::new(),
        file: file_path.display().to_string(),
        line: 2,
        column: 8,
        end_line: 5,
        end_column: 2,
        usr: "c:@S@Ponto".to_owned(),
    }];
    let decisions = vec![MappingDecision {
        type_usr: "c:@S@Ponto".to_owned(),
        option_id: "classe-direta".to_owned(),
        decided_at: "2026-01-01T00:00:00Z".to_owned(),
    }];

    let package = transpile::transpile_with_mappings(
        &[unit],
        workspace.path(),
        "ponto",
        &type_catalog,
        &decisions,
    )
    .expect("transpile with a valid recorded decision");

    assert!(package.files["lib/ponto.dart"].contains("class Ponto {"));
}

#[test]
fn write_package_creates_every_file_under_the_output_directory() {
    let package = transpile::TranspiledPackage {
        package_name: "sample".to_owned(),
        files: BTreeMap::from([
            ("pubspec.yaml".to_owned(), "name: sample\n".to_owned()),
            (
                "lib/sample.dart".to_owned(),
                "int answer() => 42;\n".to_owned(),
            ),
        ]),
    };

    let workspace = TempWorkspace::new("write-package").expect("create temp workspace");
    let output_dir = workspace.path().join("out");
    transpile::write_package(&package, &output_dir).expect("write package");

    assert_eq!(
        fs::read_to_string(output_dir.join("pubspec.yaml")).expect("read pubspec.yaml"),
        "name: sample\n"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("lib/sample.dart")).expect("read lib/sample.dart"),
        "int answer() => 42;\n"
    );
}

/// Regression test: `write_package` only ever wrote the current package's
/// files, never removing anything already sitting in `output_dir` from a
/// previous transpile. If a C++ source file (or function) is later
/// removed/renamed, its old `lib/<stem>.dart` survived on disk, orphaned and
/// no longer matching the project's current source, alongside the new
/// output.
#[test]
fn write_package_removes_stale_files_from_a_previous_transpile() {
    let workspace = TempWorkspace::new("write-package-stale").expect("create temp workspace");
    let output_dir = workspace.path().join("out");

    let first_package = transpile::TranspiledPackage {
        package_name: "sample".to_owned(),
        files: BTreeMap::from([
            ("pubspec.yaml".to_owned(), "name: sample\n".to_owned()),
            (
                "lib/removida.dart".to_owned(),
                "int removida() => 1;\n".to_owned(),
            ),
        ]),
    };
    transpile::write_package(&first_package, &output_dir).expect("write first package");
    assert!(output_dir.join("lib/removida.dart").is_file());

    let second_package = transpile::TranspiledPackage {
        package_name: "sample".to_owned(),
        files: BTreeMap::from([
            ("pubspec.yaml".to_owned(), "name: sample\n".to_owned()),
            ("lib/nova.dart".to_owned(), "int nova() => 2;\n".to_owned()),
        ]),
    };
    transpile::write_package(&second_package, &output_dir).expect("write second package");

    assert!(
        !output_dir.join("lib/removida.dart").exists(),
        "the file from the removed C++ source must not survive a later transpile"
    );
    assert_eq!(
        fs::read_to_string(output_dir.join("lib/nova.dart")).expect("read lib/nova.dart"),
        "int nova() => 2;\n"
    );
}

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
