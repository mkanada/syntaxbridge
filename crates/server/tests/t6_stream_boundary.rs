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
fn free_operator_left_shift_with_ostream_and_record_lowers_to_stream_bridge() {
    let dart = lower_and_emit(
        "t6-free-op",
        r#"
#include <ostream>
#include <sstream>
#include <string>

class Num {
public:
    int valor = 3;
};

std::ostream &operator<<(std::ostream &out, const Num &n) {
    out << n.valor;
    return out;
}

std::string texto(const Num &n) {
    std::ostringstream ss;
    ss << "n=" << n;
    return ss.str();
}
"#,
    );

    assert!(
        !dart.contains("Unsupported"),
        "expected no Unsupported bailouts, got:\n{dart}"
    );
    assert!(
        dart.contains("SyntaxBridgeOutputStream"),
        "expected SyntaxBridgeOutputStream in signatures, got:\n{dart}"
    );
}

#[test]
fn ostream_parameter_method_called_with_stringstream_and_cout() {
    let dart = lower_and_emit(
        "t6-param-method",
        r#"
#include <iostream>
#include <sstream>
#include <string>

class Item {
public:
    int id = 1;
    void print(std::ostream &out) const {
        out << "id=" << id;
    }
};

void run(const Item &item) {
    std::ostringstream ss;
    item.print(ss);
    item.print(std::cout);
}
"#,
    );

    assert!(
        !dart.contains("Unsupported"),
        "expected no Unsupported bailouts, got:\n{dart}"
    );
    assert!(
        dart.contains("void print(SyntaxBridgeOutputStream out)"),
        "expected void print(SyntaxBridgeOutputStream out), got:\n{dart}"
    );
}

#[test]
fn ostream_chained_insertion_with_endl_and_flush() {
    let dart = lower_and_emit(
        "t6-endl-flush",
        r#"
#include <ostream>
#include <string>

void logMsg(std::ostream &out, const std::string &msg) {
    out << "MSG: " << msg << std::endl;
    out << std::flush;
}
"#,
    );

    assert!(
        !dart.contains("Unsupported"),
        "expected no Unsupported bailouts, got:\n{dart}"
    );
    assert!(
        dart.contains("writeln") || dart.contains("flush"),
        "expected writeln / flush call, got:\n{dart}"
    );
}

#[test]
fn istream_parameter_maps_to_syntax_bridge_input_stream() {
    let dart = lower_and_emit(
        "t6-istream",
        r#"
#include <istream>
#include <string>

void processInput(std::istream &in) {
}
"#,
    );

    assert!(
        !dart.contains("Unsupported"),
        "expected no Unsupported bailouts, got:\n{dart}"
    );
    assert!(
        dart.contains("SyntaxBridgeInputStream"),
        "expected SyntaxBridgeInputStream, got:\n{dart}"
    );
}

#[test]
fn ofstream_and_ifstream_instantiation_and_usage() {
    let dart = lower_and_emit(
        "t6-file-streams",
        r#"
#include <fstream>
#include <string>

void save(const std::string &path, const std::string &content) {
    std::ofstream out(path);
    out << content << "\n";
    out.flush();
}

void load(const std::string &path) {
    std::ifstream in(path);
    if (in.eof()) {
        return;
    }
    int c = in.get();
}
"#,
    );

    assert!(
        !dart.contains("Unsupported"),
        "expected no Unsupported bailouts, got:\n{dart}"
    );
    assert!(
        dart.contains("SyntaxBridgeFileOutputStream"),
        "expected SyntaxBridgeFileOutputStream, got:\n{dart}"
    );
    assert!(
        dart.contains("SyntaxBridgeFileInputStream"),
        "expected SyntaxBridgeFileInputStream, got:\n{dart}"
    );
    assert!(
        dart.contains("out.write(content).write('\n');") || dart.contains("write"),
        "expected write call, got:\n{dart}"
    );
    assert!(
        dart.contains("out.flush();"),
        "expected flush call, got:\n{dart}"
    );
    assert!(
        dart.contains("in_.eof"),
        "expected eof access, got:\n{dart}"
    );
    assert!(
        dart.contains("in_.readByte()"),
        "expected readByte call, got:\n{dart}"
    );
}

#[test]
fn istringstream_construction_and_get() {
    let dart = lower_and_emit(
        "t6-istringstream",
        r#"
#include <sstream>
#include <string>

int readFirst(const std::string &data) {
    std::istringstream in(data);
    return in.get();
}
"#,
    );

    assert!(
        !dart.contains("Unsupported"),
        "expected no Unsupported bailouts, got:\n{dart}"
    );
    assert!(
        dart.contains("SyntaxBridgeStringInputStream"),
        "expected SyntaxBridgeStringInputStream, got:\n{dart}"
    );
    assert!(
        dart.contains("in_.readByte()"),
        "expected in_.readByte(), got:\n{dart}"
    );
}
