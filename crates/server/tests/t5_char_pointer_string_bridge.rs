use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::function_catalog;
use syntax_bridge_server::ingest::CompilationUnit;

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

fn lower_and_emit(name: &str, source: &str) -> String {
    let workspace = TempWorkspace::new(name).expect("create temporary workspace");
    fs::create_dir_all(workspace.path()).expect("create project dir");
    let file_path = workspace.path().join("probe.cpp");
    fs::write(&file_path, source).expect("write fixture source");
    let unit = CompilationUnit {
        directory: workspace.path().display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    };
    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");
    let module = syntax_bridge_server::ir::Module {
        functions: catalog.ir_functions.clone(),
        records: catalog.ir_records.clone(),
        enums: catalog.ir_enums.clone(),
    };
    syntax_bridge_server::emit::dart::emit_module(&module)
        .into_values()
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn const_char_pointer_passed_to_std_string_parameter_emits_bang() {
    let dart = lower_and_emit(
        "t5-call-arg",
        r#"
#include <string>

struct Fonte {
    const char *bruto() const { return "x"; }
};

std::string normaliza(const std::string &s) {
    return s;
}

std::string usa(const Fonte &f) {
    return normaliza(f.bruto());
}
"#,
    );

    assert!(
        dart.contains("return normaliza(f.bruto()!);"),
        "expected normaliza(f.bruto()!), got:\n{dart}"
    );
}

#[test]
fn const_char_pointer_initialized_or_returned_as_std_string_emits_bang() {
    let dart = lower_and_emit(
        "t5-init-return",
        r#"
#include <string>

struct Fonte {
    const char *bruto() const { return "x"; }
};

std::string direto(const Fonte &f) {
    std::string s = f.bruto();
    return f.bruto();
}
"#,
    );

    assert!(
        dart.contains("String s = f.bruto()!;"),
        "expected String s = f.bruto()!, got:\n{dart}"
    );
    assert!(
        dart.contains("return f.bruto()!;"),
        "expected return f.bruto()!, got:\n{dart}"
    );
}

#[test]
fn const_char_pointer_assigned_to_std_string_emits_bang() {
    let dart = lower_and_emit(
        "t5-assign",
        r#"
#include <string>

struct Fonte {
    const char *bruto() const { return "x"; }
};

void atribui(std::string &s, const Fonte &f) {
    s = f.bruto();
}
"#,
    );

    assert!(
        dart.contains("s = f.bruto()!;"),
        "expected s = f.bruto()!, got:\n{dart}"
    );
}

#[test]
fn string_literal_does_not_emit_redundant_bang() {
    let dart = lower_and_emit(
        "t5-literal",
        r#"
#include <string>

std::string literal() {
    std::string s = "oi";
    return "ola";
}
"#,
    );

    assert!(
        dart.contains("String s = 'oi';"),
        "expected String s = 'oi';, got:\n{dart}"
    );
    assert!(
        dart.contains("return 'ola';"),
        "expected return 'ola';, got:\n{dart}"
    );
    assert!(
        !dart.contains("'oi'!") && !dart.contains("'ola'!"),
        "string literals should never have '!', got:\n{dart}"
    );
}
