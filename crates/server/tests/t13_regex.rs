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
}
"#;
    fs::write(sys_dir.join("string"), string_header).expect("write mock string header");

    let regex_header = r#"
#include <string>

namespace std {
    namespace regex_constants {
        typedef int syntax_option_type;
        typedef int match_flag_type;
        constexpr syntax_option_type ECMAScript = 0;
        constexpr syntax_option_type icase = 1;
        constexpr syntax_option_type nosubs = 2;
        constexpr syntax_option_type optimize = 4;
        constexpr syntax_option_type collate = 8;
        constexpr syntax_option_type multiline = 16;
    }

    template <typename CharT>
    class basic_regex {
    public:
        typedef regex_constants::syntax_option_type flag_type;
        basic_regex();
        basic_regex(const char* p, flag_type f = regex_constants::ECMAScript);
        basic_regex(const string& s, flag_type f = regex_constants::ECMAScript);
        basic_regex& assign(const char* p, flag_type f = regex_constants::ECMAScript);
        basic_regex& assign(const string& s, flag_type f = regex_constants::ECMAScript);
    };
    typedef basic_regex<char> regex;

    template <typename BidirIt>
    class sub_match {
    public:
        bool matched;
        string str() const;
        int length() const;
        operator string() const;
    };
    typedef sub_match<const char*> csub_match;
    typedef sub_match<const char*> ssub_match;

    template <typename BidirIt>
    class match_results {
    public:
        typedef sub_match<BidirIt> value_type;
        match_results();
        bool empty() const;
        bool ready() const;
        int size() const;
        int length(int n = 0) const;
        int position(int n = 0) const;
        string str(int n = 0) const;
        const sub_match<BidirIt>& operator[](int n) const;
    };
    typedef match_results<const char*> cmatch;
    typedef match_results<const char*> smatch;

    template <typename BidirIt>
    bool operator==(const sub_match<BidirIt>& lhs, const basic_string<char>& rhs);
    template <typename BidirIt>
    bool operator==(const sub_match<BidirIt>& lhs, const char* rhs);
    template <typename BidirIt>
    bool operator==(const basic_string<char>& lhs, const sub_match<BidirIt>& rhs);
    template <typename BidirIt>
    bool operator==(const char* lhs, const sub_match<BidirIt>& rhs);

    bool regex_search(const string& s, smatch& m, const regex& re, regex_constants::match_flag_type flags = 0);
    bool regex_search(const char* s, cmatch& m, const regex& re, regex_constants::match_flag_type flags = 0);
    bool regex_search(const string& s, const regex& re, regex_constants::match_flag_type flags = 0);
    bool regex_search(const char* s, const regex& re, regex_constants::match_flag_type flags = 0);

    string regex_replace(const string& s, const regex& re, const string& fmt, regex_constants::match_flag_type flags = 0);
    string regex_replace(const string& s, const regex& re, const char* fmt, regex_constants::match_flag_type flags = 0);

    bool regex_match(const string& s, smatch& m, const regex& re, regex_constants::match_flag_type flags = 0);
    bool regex_match(const string& s, const regex& re, regex_constants::match_flag_type flags = 0);
}
"#;
    fs::write(sys_dir.join("regex"), regex_header).expect("write mock regex header");

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
                format!("-isystem{}", sys_dir.display()),
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

/// Item 1: Idioma do humlib (temNota e limpa)
/// std::regex_search preenche std::smatch e retorna bool;
/// std::regex_replace substitui todas as ocorrências.
/// O Dart gerado deve usar RegExp, sem bailouts.
#[test]
fn humlib_regex_search_and_replace_idiom_emits_clean_dart_regexp() {
    let files = [
        (
            "nota.h",
            r#"
#pragma once
#include <string>

bool temNota(const std::string &s);
std::string limpa(const std::string &s);
"#,
        ),
        (
            "nota.cpp",
            r#"
#include "nota.h"
#include <regex>

bool temNota(const std::string &s) {
    std::regex re("([A-G])([#b]?)([0-9])");
    std::smatch m;
    if (std::regex_search(s, m, re)) {
        return m[1] == "C";
    }
    return false;
}

std::string limpa(const std::string &s) {
    std::regex espacos("\\s+");
    return std::regex_replace(s, espacos, " ");
}
"#,
        ),
    ];

    let emitted = lower_and_emit_multi_tu("t13-humlib-idiom", &files, &[]);

    assert!(
        !emitted.contains("_syntaxBridgeUnsupported"),
        "emitted Dart contains unsupported expressions:\n{emitted}"
    );
    assert!(
        !emitted.contains("TODO(syntax-bridge)"),
        "emitted Dart contains TODO statement bailouts:\n{emitted}"
    );
    assert!(
        emitted.contains("RegExp("),
        "emitted Dart must construct RegExp:\n{emitted}"
    );
    assert!(
        emitted.contains("firstMatch"),
        "emitted Dart must call firstMatch:\n{emitted}"
    );
    assert!(
        emitted.contains("replaceAll"),
        "emitted Dart must call replaceAll:\n{emitted}"
    );
    assert!(
        emitted.contains(".group(1)"),
        "emitted Dart must access group 1:\n{emitted}"
    );
}

/// Item 2: Flags suportadas
/// icase -> caseSensitive: false
/// multiline -> multiLine: true
/// combinação de icase e multiline
#[test]
fn supported_regex_flags_emit_named_arguments() {
    let files = [(
        "flags.cpp",
        r#"
#include <regex>
#include <string>

bool testaIcase(const std::string &s) {
    std::regex re("abc", std::regex_constants::icase);
    return std::regex_search(s, re);
}

bool testaMultiline(const std::string &s) {
    std::regex re("^abc", std::regex_constants::multiline);
    return std::regex_search(s, re);
}

bool testaAmbas(const std::string &s) {
    std::regex re("^abc", std::regex_constants::icase | std::regex_constants::multiline);
    return std::regex_search(s, re);
}
"#,
    )];

    let emitted = lower_and_emit_multi_tu("t13-supported-flags", &files, &[]);

    assert!(
        !emitted.contains("_syntaxBridgeUnsupported"),
        "emitted Dart contains unsupported expressions:\n{emitted}"
    );
    assert!(
        emitted.contains("caseSensitive: false"),
        "emitted Dart must have caseSensitive: false for icase:\n{emitted}"
    );
    assert!(
        emitted.contains("multiLine: true"),
        "emitted Dart must have multiLine: true for multiline:\n{emitted}"
    );
    assert!(
        emitted.contains(".hasMatch("),
        "emitted Dart must call hasMatch for 2-arg regex_search:\n{emitted}"
    );
}

/// Item 3: Flag não suportada (nosubs, optimize, etc.) gera bailout explícito com nome da flag
#[test]
fn unsupported_regex_flag_emits_explicit_bailout_naming_flag() {
    let files = [(
        "nosubs.cpp",
        r#"
#include <regex>
#include <string>

bool testaNosubs(const std::string &s) {
    std::regex re("a", std::regex_constants::nosubs);
    return std::regex_search(s, re);
}
"#,
    )];

    let emitted = lower_and_emit_multi_tu("t13-unsupported-flag", &files, &[]);

    assert!(
        emitted.contains("nosubs"),
        "bailout must mention 'nosubs' flag:\n{emitted}"
    );
    assert!(
        emitted.contains("_syntaxBridgeUnsupported"),
        "must emit unsupported helper call:\n{emitted}"
    );
}

/// Item 4: std::regex_match gera bailout explícito sobre ancoragem
#[test]
fn regex_match_emits_explicit_bailout() {
    let files = [(
        "match.cpp",
        r#"
#include <regex>
#include <string>

bool testaMatch(const std::string &s) {
    std::regex re("[0-9]+");
    return std::regex_match(s, re);
}
"#,
    )];

    let emitted = lower_and_emit_multi_tu("t13-regex-match", &files, &[]);

    assert!(
        emitted.contains("regex_match"),
        "bailout must mention 'regex_match':\n{emitted}"
    );
    assert!(
        emitted.contains("_syntaxBridgeUnsupported"),
        "must emit unsupported helper call:\n{emitted}"
    );
}

/// Item 5: Operações em std::smatch (size, empty, str, position, length)
#[test]
fn smatch_methods_emit_correct_dart_counterparts() {
    let files = [(
        "smatch_ops.cpp",
        r#"
#include <regex>
#include <string>

int inspecionaMatch(const std::string &s) {
    std::regex re("(\\w+):(\\d+)");
    std::smatch m;
    if (std::regex_search(s, m, re)) {
        if (!m.empty()) {
            int sz = m.size();
            std::string total = m.str(0);
            std::string chave = m[1];
            int pos = m.position(0);
            int len = m.length(0);
            return sz + pos + len;
        }
    }
    return 0;
}
"#,
    )];

    let emitted = lower_and_emit_multi_tu("t13-smatch-methods", &files, &[]);

    assert!(
        !emitted.contains("_syntaxBridgeUnsupported"),
        "emitted Dart contains unsupported expressions:\n{emitted}"
    );
    assert!(
        emitted.contains(".groupCount + 1"),
        "m.size() must emit groupCount + 1:\n{emitted}"
    );
    assert!(
        emitted.contains(".group(0)"),
        "m.str(0) or m[0] must emit .group(0):\n{emitted}"
    );
    assert!(
        emitted.contains(".group(1)"),
        "m[1] must emit .group(1):\n{emitted}"
    );
    assert!(
        emitted.contains(".start"),
        "m.position(0) must emit .start:\n{emitted}"
    );
}
