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
fn safe_bool_conversion_in_if_condition_and_logical_operators() {
    let dart = lower_and_emit(
        "t7-safe-bool",
        r#"
class Caixa {
public:
    typedef void (*safe_bool)(Caixa*);
    operator safe_bool() const {
        return valor != 0 ? (safe_bool)1 : (safe_bool)0;
    }
    int valor = 0;
};

int usa(const Caixa &c) {
    if (c) {
        return 1;
    }
    return 0;
}

bool nao_caixa(const Caixa &c) {
    return !c;
}

bool e_caixa(const Caixa &c1, const Caixa &c2) {
    return c1 && c2;
}

int ternario_caixa(const Caixa &c) {
    return c ? 10 : 20;
}

int laco_caixa(Caixa c) {
    while (c) {
        c.valor--;
    }
    return c.valor;
}
"#,
    );

    assert!(
        !dart.contains("unsupported"),
        "expected no unsupported bailouts, got:\n{dart}"
    );
    assert!(
        dart.contains("bool toBool()"),
        "expected Caixa to declare `bool toBool()`, got:\n{dart}"
    );
    assert!(
        dart.contains("if (c.toBool())"),
        "expected `if (c.toBool())`, got:\n{dart}"
    );
    assert!(
        dart.contains("return !(c.toBool())") || dart.contains("return !c.toBool()"),
        "expected `!c.toBool()`, got:\n{dart}"
    );
    assert!(
        dart.contains("c1.toBool() && c2.toBool()"),
        "expected `c1.toBool() && c2.toBool()`, got:\n{dart}"
    );
    assert!(
        dart.contains("c.toBool() ? 10 : 20"),
        "expected `c.toBool() ? 10 : 20`, got:\n{dart}"
    );
}

#[test]
fn conversion_operators_to_scalar_types_and_strings() {
    let dart = lower_and_emit(
        "t7-scalar-conversions",
        r#"
class Numero {
public:
    int valor = 42;
    operator int() const { return valor; }
    operator double() const { return (double)valor; }
    operator bool() const { return valor != 0; }
};

int pega_int(Numero n) {
    int x = n;
    return x;
}

double pega_double(Numero n) {
    double d = n;
    return d;
}

bool pega_bool(Numero n) {
    bool b = n;
    return b;
}
"#,
    );

    assert!(
        !dart.contains("unsupported"),
        "expected no unsupported bailouts, got:\n{dart}"
    );
    assert!(
        dart.contains("int toInt() {"),
        "expected `int toInt()`, got:\n{dart}"
    );
    assert!(
        dart.contains("double toDouble() {"),
        "expected `double toDouble()`, got:\n{dart}"
    );
    assert!(
        dart.contains("bool toBool() {"),
        "expected `bool toBool()`, got:\n{dart}"
    );
    assert!(
        dart.contains("n.toInt()"),
        "expected `n.toInt()`, got:\n{dart}"
    );
    assert!(
        dart.contains("n.toDouble()"),
        "expected `n.toDouble()`, got:\n{dart}"
    );
    assert!(
        dart.contains("n.toBool()"),
        "expected `n.toBool()`, got:\n{dart}"
    );
}

#[test]
fn record_inheriting_from_std_string_uses_composition() {
    let dart = lower_and_emit(
        "t7-string-base",
        r#"
namespace std {
    template <typename CharT>
    class basic_string {
    public:
        int size() const;
        int length() const;
        bool operator==(const char*) const;
    };
    typedef basic_string<char> string;
}

class Token : public std::string {
public:
    int linha = 0;
};

bool ehFim(const Token &t) {
    return t == "*-";
}

int tamanho(const Token &t) {
    return t.size();
}

int comprimento(const Token *t) {
    if (t != nullptr) {
        return t->length();
    }
    return 0;
}
"#,
    );

    assert!(
        !dart.contains("class Token with string"),
        "Token must not have `with string`, got:\n{dart}"
    );
    assert!(
        !dart.contains("class Token extends string"),
        "Token must not have `extends string`, got:\n{dart}"
    );
    assert!(
        dart.contains("String syntaxBridgeStringBase = '';"),
        "Token should declare syntaxBridgeStringBase field, got:\n{dart}"
    );
    assert!(
        !dart.contains("unsupported implicit conversion"),
        "no unsupported implicit conversion expected, got:\n{dart}"
    );
    assert!(
        dart.contains("t.syntaxBridgeStringBase == '*-'"),
        "expected `t.syntaxBridgeStringBase == '*-'`, got:\n{dart}"
    );
    assert!(
        dart.contains("t.syntaxBridgeStringBase.length")
            || dart.contains("t.syntaxBridgeStringBase"),
        "expected access to syntaxBridgeStringBase for size/length, got:\n{dart}"
    );
}

#[test]
fn record_inheriting_from_std_vector_uses_composition() {
    let dart = lower_and_emit(
        "t7-vector-base",
        r#"
namespace std {
    template <typename T>
    class vector {
    public:
        int size() const;
        T& operator[](int i);
    };
}

class Item {
public:
    int id = 0;
};

class GridStaff : public std::vector<Item*> {
public:
    int index = 0;
};

int conta(const GridStaff &staff) {
    return staff.size();
}

Item* primeiro(GridStaff &staff) {
    return staff[0];
}
"#,
    );

    assert!(
        !dart.contains("class GridStaff with"),
        "GridStaff must not have `with vector`, got:\n{dart}"
    );
    assert!(
        !dart.contains("class GridStaff extends vector"),
        "GridStaff must not have `extends vector`, got:\n{dart}"
    );
    assert!(
        dart.contains("syntaxBridgeListBase"),
        "GridStaff should declare syntaxBridgeListBase field, got:\n{dart}"
    );
    assert!(
        !dart.contains("unsupported"),
        "no unsupported bailouts expected, got:\n{dart}"
    );
}
