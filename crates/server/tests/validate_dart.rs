//! End-to-end test of `validate::dart::analyze_package` against the real
//! Dart toolchain (`dart analyze --format=json`) — mirrors
//! `tests/transpile.rs`'s split between unit tests (pure, in
//! `validate/dart.rs` itself) and this file (the real subprocess).
//!
//! Unlike `tests/transpile.rs`, this doesn't need `libclang` at all: the
//! whole point of `emit::dart` is that it never produces invalid Dart (the
//! `Unsupported` escape hatch), so there is no real C++ fixture in this
//! corpus that would make `dart analyze` fail today. Per the roteiro in
//! `docs/plans/User Steps.md` (US-9): "um pacote Dart com erro deliberado,
//! cuja origem o teste conhece" — a hand-built `Module` (the same shape a
//! real transpile would produce) paired with a hand-written, deliberately
//! broken `TranspiledPackage` is what actually exercises criterion 3
//! honestly, without inventing a C++ construct this product doesn't yet
//! mistranslate.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::ir::{Function, Module, Origin, Param, Record, Type};
use syntax_bridge_server::transpile::{self, TranspiledPackage};
use syntax_bridge_server::validate::dart::{self, Severity};

fn origin(file: &str, line: u32) -> Origin {
    Origin {
        file: file.to_owned(),
        line,
        column: 1,
    }
}

#[test]
fn a_deliberate_analyzer_error_resolves_back_to_the_c_plus_plus_declaration_it_came_from() {
    let workspace = TempWorkspace::new("validate-dart").expect("create temp workspace");

    let module = Module {
        functions: vec![Function {
            name: "soma".to_owned(),
            usr: "c:@F@soma".to_owned(),
            params: vec![
                Param {
                    name: "a".to_owned(),
                    ty: Type::Int,
                    default_value: None,
                },
                Param {
                    name: "b".to_owned(),
                    ty: Type::Int,
                    default_value: None,
                },
            ],
            return_type: Type::Int,
            body: Vec::new(),
            origin: origin("/workspace/src/aritmetica.cpp", 1),
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
            origin: origin("/workspace/src/aritmetica.cpp", 5),
        }],
        enums: Vec::new(),
    };

    // Records before functions, matching `emit::dart::emit_file`'s own
    // order — `analyze_package`'s translation relies on it.
    let broken_source = "class Ponto {\n\
         \u{20}\u{20}double x;\n\
         \u{20}\u{20}double y;\n\
         }\n\
         \n\
         int soma(int a, int b) {\n\
         \u{20}\u{20}return a + naoexiste;\n\
         }\n";
    let package = TranspiledPackage {
        package_name: "aritmetica".to_owned(),
        files: BTreeMap::from([
            ("lib/aritmetica.dart".to_owned(), broken_source.to_owned()),
            (
                "pubspec.yaml".to_owned(),
                "name: aritmetica\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n".to_owned(),
            ),
        ]),
    };

    let output_dir = workspace.path().join("transpiled");
    transpile::write_package(&package, &output_dir).expect("write package");

    let diagnostics =
        dart::analyze_package(&module, &package, &output_dir).expect("run dart analyze");

    assert!(
        diagnostics.iter().any(|d| d.severity == Severity::Error),
        "expected at least one ERROR diagnostic, got: {diagnostics:?}"
    );

    let undefined_identifier = diagnostics
        .iter()
        .find(|d| d.message.contains("naoexiste"))
        .unwrap_or_else(|| panic!("expected a diagnostic about `naoexiste`, got: {diagnostics:?}"));
    assert_eq!(
        undefined_identifier.origin,
        Some(origin("/workspace/src/aritmetica.cpp", 1)),
        "the undefined-identifier error is inside `soma`'s body and must resolve there"
    );

    let uninitialized_field = diagnostics
        .iter()
        .find(|d| d.message.contains("'x'") || d.message.contains("'y'"))
        .unwrap_or_else(|| {
            panic!("expected a diagnostic about an uninitialized field, got: {diagnostics:?}")
        });
    assert_eq!(
        uninitialized_field.origin,
        Some(origin("/workspace/src/aritmetica.cpp", 5)),
        "an uninitialized-field error inside `Ponto` must resolve to `Ponto`'s own declaration"
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
