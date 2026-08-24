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
fn string_find_and_npos_idiom_emits_bytes_helper_and_negative_one() {
    let source = r#"
#include <string>
std::string antes(const std::string &s) {
    if (s.find("-") != std::string::npos) {
        return s.substr(0, s.find("-"));
    }
    return s;
}
"#;
    let dart = lower_and_emit("t4-find-npos", source);

    assert!(
        !dart.contains("basic_string"),
        "Dart output should not contain basic_string, got:\n{dart}"
    );
    assert!(
        !dart.contains("utf8.encode(s).indexOf(utf8.encode(\"-\"))"),
        "Dart output should not emit broken nested utf8.encode, got:\n{dart}"
    );
    assert!(
        dart.contains("syntaxBridgeIndexOfBytes(s, '-') != -1")
            || dart.contains("syntaxBridgeIndexOfBytes(s, \"-\") != -1"),
        "expected syntaxBridgeIndexOfBytes with -1 comparison, got:\n{dart}"
    );
}

#[test]
fn string_find_char_and_from_offset_emits_byte_helper() {
    let source = r#"
#include <string>
bool temAspas(const std::string &s) {
    return s.find('"') != std::string::npos;
}
int buscaDepois(const std::string &s, int pos) {
    return s.find("abc", pos);
}
"#;
    let dart = lower_and_emit("t4-find-char", source);

    assert!(
        !dart.contains("basic_string"),
        "Dart output should not contain basic_string, got:\n{dart}"
    );
    assert!(
        dart.contains("syntaxBridgeIndexOfByte(s, 34) != -1")
            || dart.contains("syntaxBridgeIndexOfByte(s, 34)"),
        "expected syntaxBridgeIndexOfByte for char search, got:\n{dart}"
    );
    assert!(
        dart.contains("syntaxBridgeIndexOfBytes(s, 'abc', pos)")
            || dart.contains("syntaxBridgeIndexOfBytes(s, \"abc\", pos)"),
        "expected syntaxBridgeIndexOfBytes with pos argument, got:\n{dart}"
    );
}

#[test]
fn string_substr_with_npos_count_omits_second_argument() {
    let source = r#"
#include <string>
std::string resto(const std::string &s, int pos) {
    return s.substr(pos, std::string::npos);
}
"#;
    let dart = lower_and_emit("t4-substr-npos", source);

    assert!(
        !dart.contains("basic_string"),
        "Dart output should not contain basic_string, got:\n{dart}"
    );
    assert!(
        !dart.contains("pos + -1"),
        "substr with npos count should not emit pos + -1, got:\n{dart}"
    );
    assert!(
        dart.contains("s.substring(pos)"),
        "expected s.substring(pos), got:\n{dart}"
    );
}

#[test]
fn string_char_concatenations_wrap_in_from_char_code() {
    let source = r#"
#include <string>
std::string comQuebra(std::string s) {
    s += '\n';
    return s;
}
std::string prefixaChar(const std::string &s) {
    return 'X' + s;
}
std::string sufixaChar(const std::string &s) {
    return s + 'Y';
}
"#;
    let dart = lower_and_emit("t4-char-concat", source);

    assert!(
        dart.contains("String.fromCharCode(10)"),
        "expected String.fromCharCode(10) for s += '\\n', got:\n{dart}"
    );
    assert!(
        dart.contains("String.fromCharCode(88) + s"),
        "expected String.fromCharCode(88) + s for 'X' + s, got:\n{dart}"
    );
    assert!(
        dart.contains("s + String.fromCharCode(89)"),
        "expected s + String.fromCharCode(89) for s + 'Y', got:\n{dart}"
    );
}
