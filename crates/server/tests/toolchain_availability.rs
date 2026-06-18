use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tree_sitter::Parser;

const CPP_SOURCE: &str = r#"
#include <string>

namespace bridge_fixture {
struct Counter {
    int value;

    int add(int delta) const {
        return value + delta;
    }
};
}

int main() {
    bridge_fixture::Counter counter{7};
    return counter.add(5) == 12 ? 0 : 1;
}
"#;

#[test]
fn libclang_parses_a_small_cpp_translation_unit() {
    let workspace = TempWorkspace::new("libclang").expect("create temporary workspace");
    let source_path = workspace.path().join("sample.cpp");
    let probe_path = workspace.path().join("libclang_probe.c");
    let binary_path = workspace.path().join("libclang_probe");

    fs::write(&source_path, CPP_SOURCE).expect("write C++ fixture");
    fs::write(&probe_path, LIBCLANG_PROBE_SOURCE).expect("write libclang probe");

    let mut compile = Command::new("clang");
    compile
        .arg(&probe_path)
        .arg("-o")
        .arg(&binary_path)
        .arg("-lclang");

    add_llvm_toolchain_flags(&mut compile);
    assert_success(compile.output().expect("run clang for libclang probe"));

    let output = Command::new(&binary_path)
        .arg(&source_path)
        .output()
        .expect("run libclang probe");

    assert_success(output);
}

#[test]
fn clang_compiles_a_small_cpp_program() {
    let workspace = TempWorkspace::new("clang").expect("create temporary workspace");
    let source_path = workspace.path().join("sample.cpp");
    let binary_path = workspace.path().join("sample");

    fs::write(&source_path, CPP_SOURCE).expect("write C++ fixture");

    let output = Command::new("clang++")
        .arg("-std=c++17")
        .arg(&source_path)
        .arg("-o")
        .arg(&binary_path)
        .output()
        .expect("run clang++");

    assert_success(output);

    let run = Command::new(&binary_path)
        .output()
        .expect("run compiled C++ fixture");

    assert_success(run);
}

#[test]
fn cmake_exports_compile_commands_for_a_small_cpp_project() {
    let workspace = TempWorkspace::new("cmake").expect("create temporary workspace");
    let source_path = workspace.path().join("sample.cpp");
    let cmake_path = workspace.path().join("CMakeLists.txt");
    let build_path = workspace.path().join("build");

    fs::write(&source_path, CPP_SOURCE).expect("write C++ fixture");
    fs::write(
        &cmake_path,
        r#"
cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_fixture LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
add_executable(syntax_bridge_fixture sample.cpp)
"#,
    )
    .expect("write CMake fixture");

    let configure = Command::new("cmake")
        .arg("-S")
        .arg(workspace.path())
        .arg("-B")
        .arg(&build_path)
        .arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON")
        .output()
        .expect("run cmake configure");

    assert_success(configure);

    let compile_commands_path = build_path.join("compile_commands.json");
    let compile_commands =
        fs::read_to_string(&compile_commands_path).expect("read CMake compile_commands.json");
    let entries: Value =
        serde_json::from_str(&compile_commands).expect("parse CMake compile_commands.json");
    let entries = entries
        .as_array()
        .expect("CMake compile_commands.json is an array");

    assert!(
        entries.iter().any(|entry| {
            let file = entry
                .get("file")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let command = entry
                .get("command")
                .and_then(Value::as_str)
                .or_else(|| {
                    entry
                        .get("arguments")
                        .and_then(Value::as_array)
                        .map(|_| "arguments")
                })
                .unwrap_or_default();

            file.ends_with("sample.cpp") && !command.is_empty()
        }),
        "compile_commands.json should include a command for sample.cpp: {compile_commands}"
    );
}

#[test]
fn tree_sitter_generates_an_ast_for_a_small_cpp_program() {
    let mut parser = Parser::new();
    let language = tree_sitter_cpp::LANGUAGE.into();
    parser
        .set_language(&language)
        .expect("load tree-sitter C++ grammar");

    let tree = parser
        .parse(CPP_SOURCE, None)
        .expect("tree-sitter should parse the C++ fixture");
    let root = tree.root_node();

    assert_eq!(root.kind(), "translation_unit");
    assert!(
        has_descendant_kind(root, "function_definition"),
        "AST should include a function_definition, got: {}",
        root.to_sexp()
    );
}

const LIBCLANG_PROBE_SOURCE: &str = r#"
#include <clang-c/Index.h>
#include <stdio.h>

static enum CXChildVisitResult visitor(CXCursor cursor, CXCursor parent, CXClientData data) {
    (void)parent;

    unsigned *found = (unsigned *)data;
    enum CXCursorKind kind = clang_getCursorKind(cursor);

    if (kind == CXCursor_FunctionDecl || kind == CXCursor_CXXMethod || kind == CXCursor_StructDecl) {
        *found = 1;
    }

    return CXChildVisit_Recurse;
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "usage: %s <source.cpp>\n", argv[0]);
        return 64;
    }

    CXIndex index = clang_createIndex(0, 0);
    const char *args[] = {"-x", "c++", "-std=c++17"};
    CXTranslationUnit unit = clang_parseTranslationUnit(
        index,
        argv[1],
        args,
        3,
        0,
        0,
        CXTranslationUnit_None
    );

    if (!unit) {
        fprintf(stderr, "libclang failed to parse translation unit\n");
        clang_disposeIndex(index);
        return 2;
    }

    unsigned found = 0;
    clang_visitChildren(clang_getTranslationUnitCursor(unit), visitor, &found);
    clang_disposeTranslationUnit(unit);
    clang_disposeIndex(index);

    if (!found) {
        fprintf(stderr, "libclang parsed the file but did not expose expected declarations\n");
        return 3;
    }

    return 0;
}
"#;

fn add_llvm_toolchain_flags(command: &mut Command) {
    for includedir in llvm_include_dirs() {
        command.arg(format!("-I{}", includedir.display()));
    }

    for libdir in llvm_library_dirs() {
        command.arg(format!("-L{}", libdir.display()));
        append_library_path(command, &libdir);
    }
}

fn llvm_include_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(includedir) = llvm_config_value("--includedir") {
        let path = PathBuf::from(includedir);
        if path.join("clang-c/Index.h").exists() {
            dirs.push(path);
        }
    }

    dirs.extend(versioned_llvm_roots().into_iter().filter_map(|root| {
        let include = root.join("include");
        include.join("clang-c/Index.h").exists().then_some(include)
    }));

    dirs
}

fn llvm_library_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Ok(libdir) = llvm_config_value("--libdir") {
        let path = PathBuf::from(libdir);
        if path.exists() {
            dirs.push(path);
        }
    }

    dirs.extend(versioned_llvm_roots().into_iter().filter_map(|root| {
        let lib = root.join("lib");
        lib.exists().then_some(lib)
    }));

    dirs
}

fn versioned_llvm_roots() -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir("/usr/lib") else {
        return Vec::new();
    };

    let mut roots: Vec<_> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("llvm-"))
        })
        .collect();

    roots.sort();
    roots.reverse();
    roots
}

fn llvm_config_value(flag: &str) -> io::Result<String> {
    let output = Command::new("llvm-config").arg(flag).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "llvm-config {flag} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn append_library_path(command: &mut Command, libdir: &Path) {
    let variable = if cfg!(target_os = "macos") {
        "DYLD_LIBRARY_PATH"
    } else {
        "LD_LIBRARY_PATH"
    };

    let mut paths = vec![libdir.to_path_buf()];
    if let Some(existing) = std::env::var_os(variable) {
        paths.extend(std::env::split_paths(&existing));
    }

    if let Ok(joined) = std::env::join_paths(paths) {
        command.env(variable, joined);
    }
}

fn has_descendant_kind(node: tree_sitter::Node<'_>, expected_kind: &str) -> bool {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == expected_kind || has_descendant_kind(child, expected_kind) {
            return true;
        }
    }

    false
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
