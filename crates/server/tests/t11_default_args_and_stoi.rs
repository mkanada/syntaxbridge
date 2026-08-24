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

fn lower_and_emit_multi_tu(name: &str, files: &[(&str, &str)], extra_args: &[&str]) -> String {
    let workspace = TempWorkspace::new(name).expect("create temporary workspace");
    let mut units = Vec::new();

    for &(filename, source) in files {
        let file_path = workspace.path().join(filename);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&file_path, source).expect("write fixture source");

        if filename.ends_with(".cpp") {
            let mut arguments = vec![
                "clang++".to_owned(),
                "-std=c++17".to_owned(),
                format!("-I{}", workspace.path().display()),
            ];
            for extra in extra_args {
                arguments.push(extra.to_string());
            }
            units.push(CompilationUnit {
                directory: workspace.path().display().to_string(),
                file: file_path.display().to_string(),
                command: None,
                arguments,
            });
        }
    }

    let catalog = function_catalog::extract_function_catalog(&units, workspace.path(), None)
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

/// Metade A — O default de argumentos em método out-of-line é declarado no header
/// e omitido na definição no .cpp. O Dart emitido deve preservar o valor default.
#[test]
fn multi_tu_out_of_line_method_preserves_default_arguments() {
    let files = [
        (
            "calc.h",
            r#"
struct Calc {
    static int passo(int base, int fator = 2);
    int escala(int valor, int mult = 10);
};
"#,
        ),
        (
            "calc.cpp",
            r#"
#include "calc.h"

int Calc::passo(int base, int fator) {
    return base * fator;
}

int Calc::escala(int valor, int mult) {
    return valor * mult;
}
"#,
        ),
        (
            "uso.cpp",
            r#"
#include "calc.h"

int usa() {
    Calc c;
    return Calc::passo(21) + c.escala(5);
}
"#,
        ),
    ];

    let emitted = lower_and_emit_multi_tu("multi-tu-method-default-args", &files, &[]);

    assert!(
        emitted.contains("static int passo(int base, [int fator = 2])"),
        "expected static method with default argument [int fator = 2], got:\n{emitted}"
    );
    assert!(
        emitted.contains("int escala(int valor, [int mult = 10])"),
        "expected instance method with default argument [int mult = 10], got:\n{emitted}"
    );
    assert!(
        emitted.contains("Calc.passo(21)"),
        "expected call site Calc.passo(21), got:\n{emitted}"
    );
    assert!(
        emitted.contains("c.escala(5)"),
        "expected call site c.escala(5), got:\n{emitted}"
    );
}

/// Metade A — O default em função livre e em construtor out-of-line também deve ser preservado.
#[test]
fn multi_tu_out_of_line_free_function_and_constructor_preserve_default_arguments() {
    let files = [
        (
            "item.h",
            r#"
int dobra(int base, int fator = 2);

struct Item {
    int valor;
    Item(int v = 42);
};
"#,
        ),
        (
            "item.cpp",
            r#"
#include "item.h"

int dobra(int base, int fator) {
    return base * fator;
}

Item::Item(int v) : valor(v) {}
"#,
        ),
        (
            "uso.cpp",
            r#"
#include "item.h"

int usa_item() {
    Item it;
    return dobra(10) + it.valor;
}
"#,
        ),
    ];

    let emitted = lower_and_emit_multi_tu("multi-tu-free-fn-ctor-default-args", &files, &[]);

    assert!(
        emitted.contains("int dobra(int base, [int fator = 2])"),
        "expected free function with default argument [int fator = 2], got:\n{emitted}"
    );
    assert!(
        emitted.contains("Item([this.valor = 42])") || emitted.contains("Item([int v = 42])"),
        "expected constructor with default argument 42, got:\n{emitted}"
    );
}

/// Metade B — std::stoi, std::stol, std::stoll, std::stod, std::stof são traduzidos
/// para int.parse(...) e double.parse(...), sem gerar mocks externos no basic_string.dart.
#[test]
fn std_stoi_stol_stoll_stod_stof_bridge_to_int_and_double_parse() {
    let workspace = TempWorkspace::new("std-stoi-tests").expect("create temporary workspace");
    let sys_dir = workspace.path().join("sys_include");
    fs::create_dir_all(&sys_dir).expect("create sys dir");

    let string_header = r#"
namespace std {
    template <typename CharT>
    class basic_string {
    public:
        basic_string();
        basic_string(const char*);
        int size() const;
        int length() const;
    };
    typedef basic_string<char> string;

    int stoi(const string &__str, void *__idx = 0, int __base = 10);
    long stol(const string &__str, void *__idx = 0, int __base = 10);
    long long stoll(const string &__str, void *__idx = 0, int __base = 10);
    float stof(const string &__str, void *__idx = 0);
    double stod(const string &__str, void *__idx = 0);
}
"#;
    fs::write(sys_dir.join("string"), string_header).expect("write mock string header");

    let source = r#"
#include <string>

int parse_int_1(const std::string &s) {
    return std::stoi(s);
}

int parse_int_null_idx(const std::string &s) {
    return std::stoi(s, nullptr);
}

int parse_stol(const std::string &s) {
    return std::stol(s);
}

int parse_stoll(const std::string &s) {
    return std::stoll(s);
}

double parse_stof(const std::string &s) {
    return std::stof(s);
}

double parse_stod(const std::string &s) {
    return std::stod(s);
}

double parse_stod_null_idx(const std::string &s) {
    return std::stod(s, nullptr);
}

int parse_int_null_idx_base_10(const std::string &s) {
    return std::stoi(s, nullptr, 10);
}

int parse_int_with_real_idx(const std::string &s) {
    void *idx = nullptr;
    return std::stoi(s, &idx);
}
"#;

    let files = [("probe.cpp", source)];
    let extra_arg = format!("-isystem{}", sys_dir.display());
    let emitted = lower_and_emit_multi_tu("std-stoi-tests", &files, &[&extra_arg]);

    assert!(
        emitted.contains("int.parse(s)"),
        "expected int.parse(s) for std::stoi / std::stol / std::stoll, got:\n{emitted}"
    );
    assert!(
        emitted.contains("double.parse(s)"),
        "expected double.parse(s) for std::stof / std::stod, got:\n{emitted}"
    );
    assert!(
        emitted.contains("_syntaxBridgeUnsupported<int>"),
        "expected unsupported bailout for non-null idx, got:\n{emitted}"
    );
    assert!(
        !emitted.contains("return stoi(") && !emitted.contains("int stoi("),
        "stoi should be bridged, not called as a bare function or mock, got:\n{emitted}"
    );
    assert!(
        !emitted.contains("return stod(") && !emitted.contains("double stod("),
        "stod should be bridged, not called as a bare function or mock, got:\n{emitted}"
    );
}
