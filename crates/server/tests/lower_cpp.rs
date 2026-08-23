//! Exercises `lower::cpp`'s IR lowering, hooked into the real `libclang`
//! function-catalog pass (`function_catalog::extract_function_catalog`) —
//! mirrors `tests/function_catalog.rs`'s style: a small fixture written to a
//! temp workspace, parsed for real, no mocking of `libclang`.
//!
//! Needs a real `libclang` loadable in the environment — same condition as
//! `tests/function_catalog.rs` and `tests/type_catalog.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use syntax_bridge_server::function_catalog;
use syntax_bridge_server::ingest::CompilationUnit;
use syntax_bridge_server::ir::{BinaryOp, Expr, Stmt, Type};

/// Mirrors `examples/E01-funcao-aritmetica/input/src/aritmetica.cpp`.
const ARITMETICA_CPP: &str = r#"
int soma(int a, int b) {
    return a + b;
}
"#;

fn write_fixture(project_root: &Path, source: &str, file_name: &str) -> CompilationUnit {
    fs::create_dir_all(project_root).expect("create project dir");
    let file_path = project_root.join(file_name);
    fs::write(&file_path, source).expect("write fixture source");

    CompilationUnit {
        directory: project_root.display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), "-std=c++17".to_owned()],
    }
}

#[test]
fn lowers_a_free_function_returning_a_binary_expression() {
    let workspace = TempWorkspace::new("lower-cpp-e01").expect("create temporary workspace");
    let unit = write_fixture(workspace.path(), ARITMETICA_CPP, "aritmetica.cpp");

    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");

    assert_eq!(
        catalog.ir_functions.len(),
        1,
        "expected exactly one lowered function, got {:?}",
        catalog.ir_functions
    );
    let function = &catalog.ir_functions[0];

    assert_eq!(function.name, "soma");
    assert!(!function.usr.is_empty(), "usr should be populated");
    assert_eq!(function.return_type, Type::Int);
    assert_eq!(
        function.params,
        vec![
            syntax_bridge_server::ir::Param {
                name: "a".to_owned(),
                ty: Type::Int,
                default_value: None,
            },
            syntax_bridge_server::ir::Param {
                name: "b".to_owned(),
                ty: Type::Int,
                default_value: None,
            },
        ]
    );

    assert_eq!(function.body.len(), 1, "expected a single return statement");
    let Stmt::Return {
        value: Some(value), ..
    } = &function.body[0]
    else {
        panic!(
            "expected Stmt::Return with a value, got {:?}",
            function.body[0]
        );
    };

    let Expr::Binary {
        op, lhs, rhs, ty, ..
    } = value
    else {
        panic!("expected Expr::Binary, got {value:?}");
    };
    assert_eq!(*op, BinaryOp::Add);
    assert_eq!(*ty, Type::Int);

    let Expr::Ref { name: lhs_name, .. } = lhs.as_ref() else {
        panic!("expected lhs to be a Ref, got {lhs:?}");
    };
    assert_eq!(lhs_name, "a");

    let Expr::Ref { name: rhs_name, .. } = rhs.as_ref() else {
        panic!("expected rhs to be a Ref, got {rhs:?}");
    };
    assert_eq!(rhs_name, "b");
}

/// Regression test: `libclang` wraps an implicit numeric promotion (C++'s
/// usual arithmetic conversions, or an `int` initializing a `double` local)
/// in an `ImplicitCastExpr`, which `lower_expr` reports as
/// `CXCursor_UnexposedExpr` — the same cursor kind used for pure sugar like
/// parentheses. The old code always unwrapped it transparently, discarding
/// the promotion: `double resultado = total;` (an `int` parameter) lowered
/// `total` at its original `Type::Int`, and `emit::dart` printed
/// `double resultado = total;` verbatim, which `dart analyze` rejects
/// (Dart only coerces `int` to `double` for literal constants, not
/// expressions). The promotion must survive lowering as an explicit
/// `Expr::Convert` node.
#[test]
fn lowers_an_implicit_int_to_double_promotion_into_an_explicit_convert_node() {
    const MEDIA_CPP: &str = r#"
double media(int total) {
    double resultado = total;
    return resultado;
}
"#;
    let workspace =
        TempWorkspace::new("lower-cpp-int-to-double").expect("create temporary workspace");
    let unit = write_fixture(workspace.path(), MEDIA_CPP, "media.cpp");

    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");
    assert_eq!(catalog.ir_functions.len(), 1);
    let function = &catalog.ir_functions[0];

    let Stmt::VarDecl {
        ty,
        init: Some(init),
        ..
    } = &function.body[0]
    else {
        panic!(
            "expected the first statement to be an initialized VarDecl, got {:?}",
            function.body[0]
        );
    };
    assert_eq!(*ty, Type::Double);

    let Expr::Convert {
        ty: cast_ty,
        operand,
        ..
    } = init
    else {
        panic!("expected an explicit Convert node preserving the promotion, got {init:?}");
    };
    assert_eq!(*cast_ty, Type::Double);
    let Expr::Ref {
        name,
        ty: operand_ty,
        ..
    } = operand.as_ref()
    else {
        panic!("expected the converted operand to be a Ref, got {operand:?}");
    };
    assert_eq!(name, "total");
    assert_eq!(*operand_ty, Type::Int);
}

/// C++ implicitly converts `bool` to `int` (`1`/`0`) wherever an integer is
/// expected — the mirror image of the already-handled `int` truthiness
/// (`!integer`) and `int` → `double` promotion above. Dart has no such
/// conversion; the ternary is the direct, honest translation of what the
/// implicit conversion actually computes.
#[test]
fn lowers_an_implicit_bool_to_int_conversion_into_an_explicit_ternary() {
    let source = lower_and_emit(
        "lower-cpp-bool-to-int",
        r#"
int as_int(bool flag) {
    int total = flag;
    return total;
}
"#,
    );

    assert!(
        source.contains("flag ? 1 : 0"),
        "expected an explicit bool-to-int ternary, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported implicit conversion from Bool to Int"),
        "got:\n{source}"
    );
}

/// The narrowing counterpart of the already-handled `int` → `double`
/// promotion: C++ implicitly truncates a `double` toward zero wherever an
/// `int` is expected, the same direction Dart's `.toInt()` truncates.
#[test]
fn lowers_an_implicit_double_to_int_conversion_into_a_to_int_call() {
    let source = lower_and_emit(
        "lower-cpp-double-to-int",
        r#"
int truncated(double value) {
    int whole = value;
    return whole;
}
"#,
    );

    assert!(
        source.contains("value.toInt()"),
        "expected an explicit .toInt() truncation, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported implicit conversion from Double to Int"),
        "got:\n{source}"
    );
}

/// Round 24 (Tarefa 11): Narrowing `double` → `int` conversion belongs at the
/// assignment / boundary, not per operand inside an arithmetic expression.
/// In Dart, `a * 0.5 + b` is valid without converting `a` or `b` to double,
/// and `.toInt()` is applied to the whole expression.
#[test]
fn mixed_arithmetic_double_to_int_converts_at_boundary_not_operands() {
    let source = lower_and_emit(
        "lower-cpp-arithmetic-int-double",
        r#"
int f(int a, int b) {
    int x = a * 0.5 + b;
    return x;
}
"#,
    );

    assert!(
        source.contains("int x = (a * 0.5 + b).toInt();"),
        "expected exactly one .toInt() conversion on the whole expression, got:\n{source}"
    );
    assert!(
        !source.contains(".toDouble()"),
        "expected no redundant .toDouble() on operands, got:\n{source}"
    );
    assert!(
        !source.contains("toDouble().toInt()"),
        "expected no chained toDouble().toInt(), got:\n{source}"
    );
}

#[test]
fn mixed_arithmetic_negative_factors_converts_at_boundary() {
    let source = lower_and_emit(
        "lower-cpp-arithmetic-negative",
        r#"
int calc(int a, int b) {
    int x = a * -0.5 + b;
    return x;
}
"#,
    );

    assert!(
        source.contains("int x = (a * -0.5 + b).toInt();"),
        "expected .toInt() on whole expression with negative float factor, got:\n{source}"
    );
}

#[test]
fn explicit_static_cast_preserves_to_double_in_mixed_arithmetic() {
    let source = lower_and_emit(
        "lower-cpp-explicit-cast-arithmetic",
        r#"
double f(int a, int b) {
    double x = static_cast<double>(a) / b;
    return x;
}
"#,
    );

    assert!(
        source.contains("a.toDouble() / b"),
        "expected explicit static_cast to preserve .toDouble(), got:\n{source}"
    );
}

#[test]
fn integer_division_remains_truncating_division_operator() {
    let source = lower_and_emit(
        "lower-cpp-int-div",
        r#"
int div(int a, int b) {
    int x = a / b;
    return x;
}
"#,
    );

    assert!(
        source.contains("a ~/ b"),
        "expected int division to use ~/, got:\n{source}"
    );
}

#[test]
fn a_break_statement_keeps_its_control_flow_node_and_origin() {
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

    let workspace = TempWorkspace::new("lower-cpp-unsupported").expect("create temp workspace");
    let unit = write_fixture(workspace.path(), BREAK_CPP, "controle.cpp");

    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");

    assert_eq!(catalog.ir_functions.len(), 1);
    let function = &catalog.ir_functions[0];

    // `int i = 0;`, the `while`, and `return i;` — three top-level
    // statements, none of them `Unsupported` on their own; the `break;`
    // lives nested inside the `while`'s `if`.
    assert_eq!(
        function.body.len(),
        3,
        "expected 3 statements (decl, while, return), got {:?}",
        function.body
    );

    let Stmt::While {
        body: while_body, ..
    } = &function.body[1]
    else {
        panic!(
            "expected the second statement to be a While, got {:?}",
            function.body[1]
        );
    };
    assert_eq!(
        while_body.len(),
        2,
        "expected the if and the increment inside the loop"
    );

    let Stmt::If { then_branch, .. } = &while_body[0] else {
        panic!(
            "expected the first statement in the loop body to be an If, got {:?}",
            while_body[0]
        );
    };
    assert_eq!(
        then_branch.len(),
        1,
        "expected exactly the break inside the if"
    );

    let Stmt::Break { origin } = &then_branch[0] else {
        panic!(
            "expected `break;` to be its dedicated statement, got {:?}",
            then_branch[0]
        );
    };
    assert!(origin.file.ends_with("controle.cpp"));
    assert_eq!(origin.line, 6);
}

/// Real-world regression (`docs/plans/diagnostico-verovio-6.2.0.md` achado
/// 2): two distinct C++ classes with the same short name in different
/// namespaces both lower correctly on their own, but the emitter drops the
/// namespace when naming the Dart class — so both would print as `class
/// Ponto`, `duplicate_definition` (or worse, a name collision no one
/// notices until `dart analyze` runs). `usa`'s two fields exist so the test
/// can also confirm every *reference* to a renamed record — not just its
/// own declaration — gets rewritten to match.
const NAMESPACE_COLLISION_CPP: &str = r#"
namespace ns1 {
struct Ponto {
    int x;
};
}

namespace ns2 {
struct Ponto {
    double y;
};
}

struct Usa {
    ns1::Ponto a;
    ns2::Ponto b;
};
"#;

#[test]
fn two_records_with_the_same_name_in_different_namespaces_are_disambiguated() {
    let workspace =
        TempWorkspace::new("lower-cpp-namespace-collision").expect("create temp workspace");
    let unit = write_fixture(workspace.path(), NAMESPACE_COLLISION_CPP, "colisao.cpp");

    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");

    assert_eq!(
        catalog.ir_records.len(),
        3,
        "expected Ponto (ns1), Ponto (ns2), Usa, got {:?}",
        catalog
            .ir_records
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let named_ponto: Vec<&str> = catalog
        .ir_records
        .iter()
        .filter(|r| r.name == "Ponto")
        .map(|r| r.name.as_str())
        .collect();
    assert!(
        named_ponto.is_empty(),
        "no record should still be bare-named `Ponto` after disambiguation, got {:?}",
        catalog
            .ir_records
            .iter()
            .map(|r| &r.name)
            .collect::<Vec<_>>()
    );

    let ns1_ponto = catalog
        .ir_records
        .iter()
        .find(|r| r.namespace == "ns1")
        .expect("ns1::Ponto should still be findable by its namespace");
    let ns2_ponto = catalog
        .ir_records
        .iter()
        .find(|r| r.namespace == "ns2")
        .expect("ns2::Ponto should still be findable by its namespace");
    assert_ne!(
        ns1_ponto.name, ns2_ponto.name,
        "the two Pontos must end up with distinct Dart names"
    );
    assert_ne!(ns1_ponto.usr, ns2_ponto.usr);

    let usa = catalog
        .ir_records
        .iter()
        .find(|r| r.name == "Usa")
        .expect("Usa record");
    assert_eq!(usa.fields.len(), 2);
    let Type::Record {
        usr: a_usr,
        name: a_name,
    } = &usa.fields[0].ty
    else {
        panic!(
            "expected Usa.a to be a Record type, got {:?}",
            usa.fields[0].ty
        );
    };
    let Type::Record {
        usr: b_usr,
        name: b_name,
    } = &usa.fields[1].ty
    else {
        panic!(
            "expected Usa.b to be a Record type, got {:?}",
            usa.fields[1].ty
        );
    };
    assert_eq!(
        a_usr, &ns1_ponto.usr,
        "Usa.a's type usr must still point at ns1::Ponto"
    );
    assert_eq!(
        a_name, &ns1_ponto.name,
        "Usa.a's type name must match ns1::Ponto's renamed Dart name, not the stale bare `Ponto`"
    );
    assert_eq!(b_usr, &ns2_ponto.usr);
    assert_eq!(b_name, &ns2_ponto.name);
}

/// `mapping::pointer_options_for`'s case A10
/// (`docs/mapping-solver-cases.md`): a pointer to a type this IR already
/// represents (a project class, here `Nota`) has a statically finite set of
/// possible runtime values (null, or `Nota`/a subtype of it) — the same
/// guarantee that already makes Dart's own single-reference polymorphism
/// sound — so it maps directly to a nullable reference (`Nota?`), not
/// `Unsupported`. A pointer to a scalar (`int*`) has no such guarantee (it
/// could be a single value or a buffer) and must stay `Unsupported` —
/// case C01's own bridge answer, unchanged.
const POINTER_TO_CLASS_CPP: &str = r#"
class Nota {
public:
    int altura;
};

class Editor {
public:
    void Definir(Nota* nota) { m_atual = nota; }
    Nota* Atual() { return m_atual; }
    int* ContadorBruto() { return m_contador; }
private:
    Nota* m_atual;
    int* m_contador;
};
"#;

#[test]
fn a_pointer_to_a_known_class_becomes_a_nullable_reference_but_a_pointer_to_a_scalar_stays_unsupported()
 {
    let workspace = TempWorkspace::new("lower-cpp-pointer-solver").expect("create temp workspace");
    let unit = write_fixture(workspace.path(), POINTER_TO_CLASS_CPP, "editor.cpp");

    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");

    let editor = catalog
        .ir_records
        .iter()
        .find(|r| r.name == "Editor")
        .expect("Editor record");

    let m_atual = editor
        .fields
        .iter()
        .find(|f| f.name == "_m_atual")
        .expect("m_atual field");
    let Type::Nullable(inner) = &m_atual.ty else {
        panic!(
            "expected Nota* to lower to Type::Nullable, got {:?}",
            m_atual.ty
        );
    };
    assert!(
        matches!(inner.as_ref(), Type::Record { name, .. } if name == "Nota"),
        "expected the nullable's inner type to be the Nota record, got {inner:?}"
    );

    let m_contador = editor
        .fields
        .iter()
        .find(|f| f.name == "_m_contador")
        .expect("m_contador field");
    assert!(
        matches!(m_contador.ty, Type::Unsupported(_)),
        "a pointer to a scalar has no finite-pointee guarantee and must stay Unsupported, got {:?}",
        m_contador.ty
    );

    let definir = editor
        .methods
        .iter()
        .find(|m| m.name == "Definir")
        .expect("Definir method");
    let Type::Nullable(param_inner) = &definir.params[0].ty else {
        panic!(
            "expected Definir's Nota* parameter to be Type::Nullable, got {:?}",
            definir.params[0].ty
        );
    };
    assert!(matches!(param_inner.as_ref(), Type::Record { name, .. } if name == "Nota"));

    let atual = editor
        .methods
        .iter()
        .find(|m| m.name == "Atual")
        .expect("Atual method");
    assert!(
        matches!(atual.return_type, Type::Nullable(_)),
        "expected Atual's Nota* return type to be Type::Nullable, got {:?}",
        atual.return_type
    );

    let contador_bruto = editor
        .methods
        .iter()
        .find(|m| m.name == "ContadorBruto")
        .expect("ContadorBruto method");
    assert!(
        matches!(contador_bruto.return_type, Type::Unsupported(_)),
        "int* has no finite-pointee guarantee and must stay Unsupported, got {:?}",
        contador_bruto.return_type
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

/// Lowers `source` and emits it, returning the single Dart file — the
/// declaration and every reference to it end up in the same text, which is
/// the only place the two can be checked against each other.
fn lower_and_emit(name: &str, source: &str) -> String {
    lower_and_emit_with_std(name, source, "c++17")
}

/// `write_fixture`'s hardcoded `-std=c++17` doesn't match every real
/// project's own configuration — Verovio 6.2.0 itself builds as C++20
/// (`cmake/CMakeLists.txt`'s `set(CMAKE_CXX_STANDARD 20)`), and C++20's
/// rewritten-candidates rule changes the AST shape of every overloaded
/// comparison (`it != end()` compiles through `operator==` plus a
/// `CXXRewrittenBinaryOperator`/`UnaryOperator '!'` wrapper, not a direct
/// `operator!=` call — confirmed with a real `clang++ -Xclang -ast-dump
/// -std=c++20`, the exact gap that let the general-iterator-loop feature
/// pass every `-std=c++17` fixture yet match zero real Verovio occurrences
/// the first time it landed). Use this directly whenever a fixture needs to
/// pin a specific standard instead of trusting the default.
fn lower_and_emit_with_std(name: &str, source: &str, std: &str) -> String {
    let workspace = TempWorkspace::new(name).expect("create temporary workspace");
    fs::create_dir_all(workspace.path()).expect("create project dir");
    let file_path = workspace.path().join("probe.cpp");
    fs::write(&file_path, source).expect("write fixture source");
    let unit = CompilationUnit {
        directory: workspace.path().display().to_string(),
        file: file_path.display().to_string(),
        command: None,
        arguments: vec!["clang++".to_owned(), format!("-std={std}")],
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

/// An enum declared outside the project (`std::memory_order` here) is
/// named by `lower_type` but never declared by `lower_enum`, which only
/// emits declarations for enums inside `project_root`. Emitting the name
/// anyway produced `void f(memory_order m)` referencing a class no file in
/// the package defines — `dart analyze`'s `undefined_class`. It has to come
/// back out as `Unsupported`, which bails at the use site instead.
#[test]
fn an_enum_declared_outside_the_project_does_not_become_an_undeclared_dart_type() {
    let source = lower_and_emit(
        "lower-cpp-external-enum",
        r#"
#include <memory>
void f(std::memory_order m) { }
"#,
    );

    assert!(
        !source.contains("memory_order m"),
        "an external enum must not be named as a Dart parameter type, got:\n{source}"
    );
    assert!(
        source.contains("UnimplementedError"),
        "the function should bail out loudly instead, got:\n{source}"
    );
}

/// `std::array<T, N>` has Dart's `List<T>` value shape. Libclang exposes the
/// non-type template argument `N` as a child of the `ParmVarDecl`, so this
/// also guards against mistaking its bound for a C++ default argument and
/// emitting invalid Dart such as `List<int> a = 3`.
#[test]
fn a_std_array_lowers_to_a_list_without_a_spurious_default_argument() {
    let source = lower_and_emit(
        "lower-cpp-stdlib-array",
        r#"
#include <array>
void f(std::array<int, 3> a) { }
"#,
    );

    assert!(
        source.contains("void f(List<int> a)"),
        "std::array must lower to a required Dart List parameter, got:\n{source}"
    );
    assert!(
        !source.contains("a = 3") && !source.contains("UnimplementedError"),
        "the template bound is not a default argument or a bailout, got:\n{source}"
    );
}

/// C++ permits a default argument to live on a prior declaration while the
/// lowered function body comes from a later definition. The inherited default
/// is a real API contract, unlike the non-type `3` child that libclang exposes
/// for `std::array<int, 3>`.
#[test]
fn a_default_argument_declared_in_a_header_is_preserved_on_its_definition() {
    let workspace = TempWorkspace::new("lower-cpp-header-default-argument")
        .expect("create temporary workspace");
    fs::write(
        workspace.path().join("api.hpp"),
        "int increment(int value, int step = 1);\n",
    )
    .expect("write fixture header");
    let unit = write_fixture(
        workspace.path(),
        r#"
#include "api.hpp"

int increment(int value, int step) {
    return value + step;
}
"#,
        "api.cpp",
    );
    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");
    let module = syntax_bridge_server::ir::Module {
        functions: catalog.ir_functions,
        records: catalog.ir_records,
        enums: catalog.ir_enums,
    };
    let source = syntax_bridge_server::emit::dart::emit_module(&module)
        .into_values()
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        source.contains("int increment(int value, [int step = 1])"),
        "a header-declared default must survive on the lowered definition, got:\n{source}"
    );
}

/// These standard-library wrappers preserve a value shape Dart already has:
/// fixed-size and deque containers are lists, the unordered variants use the
/// same `Set`/`Map` interface, `optional` is nullable, smart pointers are
/// nullable references, and an initializer list is a list.  Their method
/// translation is deliberately a separate concern; at the type boundary none
/// of them needs an opaque placeholder.
#[test]
fn standard_library_value_wrappers_lower_to_existing_dart_core_types() {
    let workspace =
        TempWorkspace::new("lower-cpp-stdlib-value-wrappers").expect("create temporary workspace");
    let unit = write_fixture(
        workspace.path(),
        r#"
#include <array>
#include <deque>
#include <initializer_list>
#include <memory>
#include <optional>
#include <string>
#include <unordered_map>
#include <unordered_set>

void bridge(
    std::array<int, 3> fixed,
    std::deque<double> queue,
    std::unordered_set<int> ids,
    std::unordered_map<int, std::string> labels,
    std::initializer_list<int> initial,
    std::optional<int> maybe_count,
    std::unique_ptr<std::string> owned,
    std::shared_ptr<int> shared) {}
"#,
        "value_wrappers.cpp",
    );

    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");
    let function = catalog
        .ir_functions
        .iter()
        .find(|function| function.name == "bridge")
        .expect("bridge function");

    assert_eq!(
        function
            .params
            .iter()
            .map(|param| &param.ty)
            .collect::<Vec<_>>(),
        vec![
            &Type::List(Box::new(Type::Int)),
            &Type::List(Box::new(Type::Double)),
            &Type::Set(Box::new(Type::Int)),
            &Type::Map(Box::new(Type::Int), Box::new(Type::Str)),
            &Type::List(Box::new(Type::Int)),
            &Type::Nullable(Box::new(Type::Int)),
            &Type::Nullable(Box::new(Type::Str)),
            &Type::Nullable(Box::new(Type::Int)),
        ],
        "value-shaped standard library types must not become Type::Unsupported: {function:?}"
    );
}

/// A character typedef keeps its source spelling on a pointer even though
/// its canonical pointee type is `char`. The pointer must therefore follow
/// the existing C-string bridge (`String?`), rather than becoming opaque just
/// because a library chose an alias such as `char_t`.
#[test]
fn a_pointer_to_a_character_typedef_lowers_like_a_c_string() {
    let source = lower_and_emit(
        "lower-cpp-character-typedef-pointer",
        r#"
typedef char char_t;

void set_label(const char_t* label) {}
"#,
    );

    assert!(
        source.contains("void set_label(String? label)"),
        "a character typedef pointer must retain C-string semantics, got:\n{source}"
    );
    assert!(
        !source.contains("SyntaxBridgeOpaque") && !source.contains("UnimplementedError"),
        "a character typedef pointer must not force a type bailout, got:\n{source}"
    );
}

/// A native fixed-size array is a value container, not an FFI pointer. Its
/// bound is not encoded in Dart's type system, but its element type and list
/// shape are, so a record field can remain typed while array operations gain
/// dedicated lowering rules incrementally.
#[test]
fn native_fixed_size_arrays_lower_to_typed_dart_lists() {
    let source = lower_and_emit(
        "lower-cpp-native-arrays",
        r#"
struct Buffer {
    char bytes[32];
    int samples[3];
};
"#,
    );

    assert!(
        source.contains("List<int> bytes") && source.contains("List<int> samples"),
        "native arrays must preserve their element types as Dart Lists, got:\n{source}"
    );
    assert!(
        !source.contains("SyntaxBridgeOpaque") && !source.contains("UnimplementedError"),
        "native array fields must not force a type bailout, got:\n{source}"
    );
}

/// Verovio's miniz dependency exposes binary payloads through its named
/// `mz_uint8*` alias. It is neither a C string nor an arbitrary `void*`: the
/// Dart boundary can retain its byte-buffer contract as `Uint8List?`.
#[test]
fn a_known_byte_buffer_pointer_lowers_to_a_nullable_uint8_list() {
    let source = lower_and_emit(
        "lower-cpp-byte-buffer-pointer",
        r#"
typedef unsigned char mz_uint8;

void inflate(const mz_uint8* input, mz_uint8* output) {}
"#,
    );

    assert!(
        source.contains("import 'dart:typed_data';"),
        "a byte-buffer bridge must import Uint8List, got:\n{source}"
    );
    assert!(
        source.contains("void inflate(Uint8List? input, Uint8List? output)"),
        "known byte pointers must retain their Uint8List contract, got:\n{source}"
    );
    assert!(
        !source.contains("SyntaxBridgeOpaque") && !source.contains("UnimplementedError"),
        "known byte pointers must not force a type bailout, got:\n{source}"
    );
}

/// A `std::pair` is not lowered to a positional Dart record: C++ programs
/// access its stable `first` and `second` members, and generated files need
/// one shared nominal type when a pair crosses a file boundary.
#[test]
fn a_std_pair_lowers_to_the_shared_named_pair_adapter() {
    let files = {
        let workspace =
            TempWorkspace::new("lower-cpp-std-pair").expect("create temporary workspace");
        let unit = write_fixture(
            workspace.path(),
            r#"
#include <string>
#include <utility>

void consume(std::pair<int, std::string> value) {}
"#,
            "pair.cpp",
        );
        let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
            .expect("extract function catalog");
        let module = syntax_bridge_server::ir::Module {
            functions: catalog.ir_functions,
            records: catalog.ir_records,
            enums: catalog.ir_enums,
        };
        syntax_bridge_server::emit::dart::emit_module(&module)
    };

    assert!(
        files["lib/pair.dart"].contains("import 'syntax_bridge_support.dart';")
            && files["lib/pair.dart"].contains("SyntaxBridgePair<int, String> value"),
        "pair use must import and name the shared adapter, got:\n{}",
        files["lib/pair.dart"]
    );
    assert!(
        files["lib/syntax_bridge_support.dart"].contains("final class SyntaxBridgePair<A, B>")
            && files["lib/syntax_bridge_support.dart"].contains("final A first;")
            && files["lib/syntax_bridge_support.dart"].contains("final B second;"),
        "pair adapter must preserve C++ member names, got:\n{}",
        files["lib/syntax_bridge_support.dart"]
    );
}

/// A `void*` only becomes a Dart byte buffer when its surrounding signature
/// proves the buffer contract: a named payload plus its matching scalar
/// length. An unrelated `void*` still gets a precise type — the named
/// identity-only bridge (`SyntaxBridgeNativeHandle?`, see the
/// `a_void_pointer_lowers_to_a_named_native_handle_bridge_instead_of_a_bailout`
/// test below) — rather than being silently guessed as bytes.
#[test]
fn a_void_pointer_with_a_matching_length_parameter_lowers_to_bytes() {
    let source = lower_and_emit(
        "lower-cpp-void-buffer",
        r#"
#include <cstddef>

void digest(const void* data, size_t data_size) {}
void keep_opaque(void* context) {}
"#,
    );

    assert!(
        source.contains("import 'dart:typed_data';")
            && source.contains("void digest(Uint8List? data, int data_size)"),
        "a void buffer with an explicit matching length must become Uint8List?, got:\n{source}"
    );
    assert!(
        source.contains("void keep_opaque(SyntaxBridgeNativeHandle? context)"),
        "an unclassified void pointer must still get a precise, named bridge type, got:\n{source}"
    );
}

/// A C++ function pointer with a fully representable signature is a Dart
/// closure type at the API boundary. Its invocation lowering is separate;
/// preserving this type must not require an opaque pointer or `dynamic`.
#[test]
fn a_typed_function_pointer_lowers_to_a_dart_callback() {
    let source = lower_and_emit(
        "lower-cpp-function-pointer",
        r#"
int apply(int (*transform)(int), int value) {
    return transform(value);
}
"#,
    );

    assert!(
        source.contains("int apply(int Function(int) transform, int value)"),
        "a representable function pointer must become a typed Dart callback, got:\n{source}"
    );
    assert!(
        !source.contains("SyntaxBridgeOpaque /* unsupported: int (*)(int) */"),
        "callback type must not remain an opaque pointer, got:\n{source}"
    );
}

/// Container-dependent scalar aliases such as `std::vector<T>::size_type`
/// retain their canonical integer value domain in Dart. They must not become
/// an opaque declaration merely because libclang exposes the spelling as an
/// unexposed dependent alias at a use site.
#[test]
fn a_standard_container_size_type_lowers_to_an_int() {
    let source = lower_and_emit(
        "lower-cpp-container-size-type",
        r#"
#include <vector>

int advance(std::vector<int>::size_type offset) {
    std::vector<int>::size_type next = offset + 1;
    return next;
}
"#,
    );

    assert!(
        source.contains("int advance(int offset)") && source.contains("int next = offset + 1;"),
        "a standard container size_type must lower to int in both signature and local use, got:\n{source}"
    );
    assert!(
        !source.contains("size_type") && !source.contains("SyntaxBridgeOpaque"),
        "a canonical scalar alias must not remain an opaque type bailout, got:\n{source}"
    );
}

/// An `auto` local inferred from a standard-library operation can arrive as
/// `CXType_Auto` with the dependent spelling `size_type`. Its canonical type
/// is still an integer; the type bailout must disappear even while the
/// operation that produced the value awaits its own expression lowering.
#[test]
fn an_auto_local_inferred_as_size_type_lowers_to_an_int() {
    let source = lower_and_emit(
        "lower-cpp-auto-size-type",
        r#"
#include <string>

int find_index(std::string text) {
    auto index = text.find("x");
    return index;
}
"#,
    );

    assert!(
        !source.contains("unsupported type in expression: size_type"),
        "the canonical type of an auto size_type local must not cause a type bailout, got:\n{source}"
    );
    assert!(
        !source.contains("SyntaxBridgeOpaque /* unsupported: size_type */"),
        "an auto size_type local must not emit an opaque Dart type, got:\n{source}"
    );
}

/// C++ accepts an integer both as a boolean return value and as the operand
/// of logical negation. Dart requires a real `bool`, so lowering must retain
/// the truth conversion explicitly instead of emitting an invalid `!value`
/// or bailing out the enclosing function.
#[test]
fn integer_truthiness_and_logical_not_lower_to_typed_dart_booleans() {
    let source = lower_and_emit(
        "lower-cpp-integer-truthiness",
        r#"
bool is_zero(int value) {
    return !value;
}

bool is_present(int value) {
    return value;
}

bool has_value(int value) {
    if (value) {
        return true;
    }
    return false;
}
"#,
    );

    assert!(
        source.contains("bool is_zero(int value)")
            && source.contains("return !(value != 0);")
            && source.contains("bool is_present(int value)")
            && source.contains("return value != 0;")
            && source.contains("bool has_value(int value)")
            && source.contains("if (value != 0)"),
        "integer truthiness must become an explicit typed Dart boolean conversion, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported unary operator kind 10")
            && !source.contains("unsupported implicit conversion from Int to Bool"),
        "logical-not and int-to-bool must not remain expression bailouts, got:\n{source}"
    );
}

/// A standard-library member call can have a semantic wrapper around its
/// callee when its receiver is itself a call expression. The dispatcher must
/// locate the member reference structurally, rather than assuming it is the
/// first direct child of the call cursor.
#[test]
fn a_stdlib_method_call_with_a_temporary_receiver_keeps_its_receiver() {
    let source = lower_and_emit(
        "lower-cpp-stdlib-temporary-receiver",
        r#"
#include <vector>

std::vector<int> make_values();

int count_values() {
    return make_values().size();
}
"#,
    );

    assert!(
        source.contains("int count_values()")
            && source.contains("return make_values().length;")
            && !source.contains("standard-library method call's first child was not"),
        "a wrapped stdlib receiver must still reach the stdlib dispatcher, got:\n{source}"
    );
}

/// Libclang represents an overloaded assignment as an operator call: its
/// receiver is the first call argument, not a `MemberRefExpr`. The stdlib
/// dispatcher must normalize that shape before deciding whether it supports
/// the method itself, so its diagnostic remains specific and actionable.
#[test]
fn a_std_vector_assignment_copies_its_elements_instead_of_aliasing() {
    let source = lower_and_emit(
        "lower-cpp-stdlib-operator-receiver",
        r#"
#include <vector>

void copy_values(std::vector<int>& destination, const std::vector<int>& source) {
    destination = source;
}
"#,
    );

    assert!(
        source.contains("destination = List.of(source);")
            && !source.contains("unsupported std::vector::operator= call"),
        "a vector assignment must preserve C++ copy semantics, got:\n{source}"
    );
}

/// An implicitly generated C++ record assignment copies values. Dart record
/// instances are references, so the lowering must construct a new record from
/// the source fields instead of assigning the source object directly.
#[test]
fn a_defaulted_record_assignment_constructs_a_value_copy() {
    let source = lower_and_emit(
        "lower-cpp-defaulted-record-assignment",
        r#"
struct Coordinate {
    int x;
    int y;
};

void copy_coordinate(Coordinate& destination, const Coordinate& source) {
    destination = source;
}
"#,
    );

    assert!(
        source.contains("destination = Coordinate(source.x, source.y);"),
        "defaulted record assignment must construct a distinct Dart value, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported operator method call: operator="),
        "a defaulted record copy assignment must not remain an operator bailout, got:\n{source}"
    );
}

/// A defaulted record assignment must recursively preserve container value
/// semantics too: a vector field needs List.of rather than a shared List.
#[test]
fn a_defaulted_record_assignment_copies_vector_fields() {
    let source = lower_and_emit(
        "lower-cpp-defaulted-record-vector-copy",
        r#"
#include <vector>

struct Bucket {
    std::vector<int> values;
};

void copy_bucket(Bucket& destination, const Bucket& source) {
    destination = source;
}
"#,
    );

    assert!(
        source.contains("destination = Bucket(List.of(source.values));"),
        "record vector fields must keep C++ copy semantics, got:\n{source}"
    );
}

/// A trivial record with text and vector fields is default-constructed in C++
/// before its fields are used. Dart needs real typed values at construction,
/// not unsupported placeholders inside the synthetic record constructor.
#[test]
fn a_default_constructed_record_gets_typed_text_and_collection_values() {
    let source = lower_and_emit(
        "lower-cpp-default-record-text-collection",
        r#"
#include <string>
#include <vector>

struct State {
    std::string name;
    std::vector<int> values;
};

int state_size() {
    State state;
    state.name = "ready";
    return state.values.size();
}
"#,
    );

    assert!(
        source.contains("State state = State('', List.empty());")
            && source.contains("state.name = 'ready';")
            && source.contains("return state.values.length;"),
        "default record construction must use typed text and collection values, got:\n{source}"
    );
    assert!(
        !source.contains("no default value available for this field's type yet"),
        "representable text and collection fields must not become default-value bailouts, got:\n{source}"
    );
}

/// Default construction of a record nested by value must recursively create
/// the inner record. A late outer local cannot support immediate field writes.
#[test]
fn a_default_constructed_record_recursively_constructs_nested_records() {
    let source = lower_and_emit(
        "lower-cpp-default-record-nested",
        r#"
struct Point {
    int x;
};

struct Holder {
    Point point;
};

int set_point() {
    Holder holder;
    holder.point.x = 7;
    return holder.point.x;
}
"#,
    );

    assert!(
        source.contains("Holder holder = Holder(Point(0));")
            && source.contains("holder.point.x = 7;"),
        "nested trivial records must be constructed recursively, got:\n{source}"
    );
    assert!(
        !source.contains("no default value available for this field's type yet"),
        "a nested representable record must not become a default-value bailout, got:\n{source}"
    );
}

/// A nullable C++ value has the sound Dart default null. It must not turn a
/// trivial containing record into a default-value bailout.
#[test]
fn a_default_constructed_record_uses_null_for_nullable_fields() {
    let source = lower_and_emit(
        "lower-cpp-default-record-nullable",
        r#"
#include <optional>

struct Options {
    std::optional<int> limit;
};

void configure() {
    Options options;
    options.limit = 3;
}
"#,
    );

    assert!(
        source.contains("Options options = Options(null);")
            && source.contains("options.limit = 3;"),
        "nullable fields must receive a sound null default, got:\n{source}"
    );
    assert!(
        !source.contains("no default value available for this field's type yet"),
        "nullable fields must not remain default-value bailouts, got:\n{source}"
    );
}

/// A Dart enum has no implicit default, unlike the zero-initialized storage a
/// trivial C++ record can expose. Its first declared value is a deterministic
/// typed default that keeps the containing record constructible.
#[test]
fn a_default_constructed_record_uses_its_first_enum_variant() {
    let source = lower_and_emit(
        "lower-cpp-default-record-enum",
        r#"
enum Mode { idle, running };

struct Options {
    Mode mode;
};

void configure() {
    Options options;
}
"#,
    );

    assert!(
        source.contains("Options options = Options(Mode.idle);"),
        "enum fields must get the first declared typed value, got:\n{source}"
    );
    assert!(
        !source.contains("no default value available for this field's type yet"),
        "enum fields must not remain default-value bailouts, got:\n{source}"
    );
}

/// Assignment operators on Dart core-backed C++ library values are effects,
/// not opaque method calls. The receiver normalized from argument zero must
/// become a real Dart assignment.
#[test]
fn a_std_string_assignment_lowers_to_a_typed_dart_assignment() {
    let source = lower_and_emit(
        "lower-cpp-std-string-assignment",
        r#"
#include <string>

void copy_text(std::string& destination, const std::string& source) {
    destination = source;
}
"#,
    );

    assert!(
        source.contains("destination = source;")
            && !source.contains("unsupported std::basic_string::operator= call"),
        "std::string assignment must become a Dart assignment, got:\n{source}"
    );
}

/// Assigning *through* a `std::string*` out-param (Verovio's real idiom —
/// `editortoolkit_neume.cpp`'s `ParseAddSylAction`: `(*elementId) =
/// param.get<jsonxx::String>("elementId");`) must reassign the underlying
/// nullable local, not read-dereference it. `lower_stdlib_assignment_stmt`
/// lowers the assignment target the same way it lowers any other operand —
/// through `lower_expr`, which represents `*out` as `Expr::Convert` (a
/// dereference read, `out!`) — and the Dart emitter's `Stmt::ExprAssign`
/// fallback then renders that read-oriented node verbatim as the assignment
/// target, producing `out! = value;`, which `dart format` rejects outright
/// ("Illegal assignment to non-assignable expression"): `!` is a read-only
/// null-assertion operator, never a valid part of an lvalue. Two real
/// Verovio files fail to parse for exactly this reason as of the
/// 2026-08-20 diagnosis run.
#[test]
fn assigning_through_a_string_out_param_reassigns_the_nullable_local_without_a_bang() {
    let source = lower_and_emit(
        "lower-cpp-out-param-deref-assign",
        r#"
#include <string>

void fill(std::string value, std::string* out) {
    (*out) = value;
}
"#,
    );

    assert!(
        source.contains("out = value;"),
        "dereference-assignment through an out-param must reassign the nullable \
         local directly, with no `!`, got:\n{source}"
    );
    assert!(
        !source.contains("out! ="),
        "`!` is never valid on an assignment target, got:\n{source}"
    );
}

/// Indexed writes are ordinary Dart assignments once the vector receiver and
/// its element type are known.
#[test]
fn a_vector_index_assignment_lowers_to_a_dart_index_write() {
    let source = lower_and_emit(
        "lower-cpp-vector-index-assignment",
        r#"
#include <vector>

void copy_first(std::vector<int>& values) {
    values[0] = values[1];
}
"#,
    );

    assert!(
        source.contains("values[0] = values[1];")
            && !source.contains("index assignment not supported yet"),
        "vector index assignment must preserve the target and value, got:\n{source}"
    );
}

/// The front and back accessors of a non-empty vector have direct indexed
/// List counterparts. Dart's range error on an empty list remains the same
/// observable failure shape as the C++ precondition violation.
#[test]
fn vector_front_and_back_lower_to_typed_dart_indexes() {
    let source = lower_and_emit(
        "lower-cpp-vector-front-back",
        r#"
#include <vector>

int first(const std::vector<int>& values) {
    return values.front();
}

int last(const std::vector<int>& values) {
    return values.back();
}
"#,
    );

    assert!(
        source.contains("return values[0];")
            && source.contains("return values[values.length - 1];"),
        "vector front and back must become typed List indexes, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::vector::front call")
            && !source.contains("unsupported std::vector::back call"),
        "supported vector endpoint access must not remain bailouts, got:\n{source}"
    );
}

/// list and deque already lower to List<T> at the type boundary, so their
/// common endpoint, mutation and query operations use the same Dart List
/// adapters as vector without inventing an opaque iterator layer.
#[test]
fn list_and_deque_common_operations_lower_to_dart_list_operations() {
    let source = lower_and_emit(
        "lower-cpp-list-deque-common-operations",
        r#"
#include <deque>
#include <list>

int mutate_list(std::list<int>& values) {
    values.push_back(3);
    int result = values.front() + values.back() + values.size();
    values.clear();
    return result;
}

bool inspect_deque(std::deque<int>& values) {
    values.push_back(3);
    return !values.empty() && values.at(0) == values.front();
}
"#,
    );

    assert!(
        source.contains("values.add(3);")
            && source.contains("values[0]")
            && source.contains("values[values.length - 1]")
            && source.contains("values.length")
            && source.contains("values.clear();")
            && source.contains("values.isEmpty"),
        "list and deque methods must use Dart List operations, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::list::") && !source.contains("unsupported std::deque::"),
        "supported list and deque methods must not remain bailouts, got:\n{source}"
    );
}

/// A map subscript used as an assignment target has the same direct write
/// operation in Dart. This deliberately does not claim that a standalone
/// C++ map read, with its insertion-on-miss rule, is equivalent to Dart.
#[test]
fn map_index_assignment_lowers_to_a_typed_dart_map_write() {
    let source = lower_and_emit(
        "lower-cpp-map-index-assignment",
        r#"
#include <map>
#include <string>

void set_label(std::map<int, std::string>& labels) {
    labels[4] = "four";
}
"#,
    );

    assert!(
        source.contains("labels[4] = 'four';"),
        "map index assignment must become a Dart map write, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::map::operator[] call")
            && !source.contains("index assignment not supported yet"),
        "map write must not remain a bailout, got:\n{source}"
    );
}

/// Reading through C++ map subscript inserts the value-initialized mapped
/// value when absent. Dart putIfAbsent is the corresponding typed operation;
/// a plain nullable lookup would change both the result and the map state.
#[test]
fn map_index_read_preserves_cpp_default_insertion() {
    let source = lower_and_emit(
        "lower-cpp-map-index-read",
        r#"
#include <map>
#include <string>

std::string label(std::map<int, std::string>& labels) {
    return labels[4];
}
"#,
    );

    assert!(
        source.contains("return labels.putIfAbsent(4, () => '');"),
        "map index reads must retain C++ insertion-on-miss semantics, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::map::operator[] call"),
        "map reads with a representable default must not remain bailouts, got:\n{source}"
    );
}

/// A compound write reads a C++ map subscript before storing back. The read
/// must retain `operator[]`'s insertion-on-miss behavior while the outer
/// assignment remains Dart's direct map write.
#[test]
fn map_index_compound_assignment_preserves_default_insertion() {
    let source = lower_and_emit(
        "lower-cpp-map-index-compound-assignment",
        r#"
#include <map>

int increment(std::map<int, int>& values, int key) {
    values[key] += 2;
    return values[key];
}
"#,
    );

    assert!(
        source.contains("values[key] = values.putIfAbsent(key, () => 0) + 2;")
            && source.contains("return values.putIfAbsent(key, () => 0);")
            && !source
                .contains("compound assignment target is not a simple local variable or a field"),
        "compound map writes must retain C++ insertion semantics, got:\n{source}"
    );
}

/// Membership and cardinality queries on map and set have direct Dart core
/// operations and should remain typed instead of becoming opaque calls.
#[test]
fn map_and_set_queries_lower_to_typed_dart_core_operations() {
    let source = lower_and_emit(
        "lower-cpp-map-set-queries",
        r#"
#include <map>
#include <set>

bool map_contains(const std::map<int, int>& values, int key) {
    return values.count(key) > 0;
}

bool map_empty(const std::map<int, int>& values) {
    return values.empty();
}

int map_size(const std::map<int, int>& values) {
    return values.size();
}

bool set_contains(const std::set<int>& values, int key) {
    return values.count(key) > 0;
}

bool set_empty(const std::set<int>& values) {
    return values.empty();
}

int set_size(const std::set<int>& values) {
    return values.size();
}
"#,
    );

    assert!(
        source.contains("values.containsKey(key) ? 1 : 0")
            && source.contains("values.contains(key) ? 1 : 0")
            && source.contains("values.isEmpty")
            && source.contains("return values.length;"),
        "map and set queries must use typed Dart core operations, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::map::count call")
            && !source.contains("unsupported std::set::count call"),
        "query adapters must not remain bailouts, got:\n{source}"
    );
}

/// Native arrays already lower to typed Dart lists. Their subscript uses a
/// distinct ArraySubscriptExpr cursor from vector's operator call, but is the
/// same safe Dart index read/write once the receiver is known to be a list.
#[test]
fn a_native_array_index_assignment_lowers_to_a_typed_dart_index_write() {
    let source = lower_and_emit(
        "lower-cpp-native-array-index-assignment",
        r#"
int replace_first() {
    int values[2] = {1, 2};
    values[0] = values[1];
    return values[0];
}
"#,
    );

    assert!(
        source.contains("values[0] = values[1];")
            && source.contains("return values[0];")
            && !source.contains("unsupported expression cursor kind 113"),
        "native array subscript must lower as a typed Dart index, got:\n{source}"
    );
}

/// Allocating a project record through C++ new is a managed Dart object when
/// its pointee is already a representable record. The nullable return type
/// remains valid, while the constructor call carries the actual value.
#[test]
fn new_of_a_known_record_lowers_to_a_dart_constructor_call() {
    let source = lower_and_emit(
        "lower-cpp-new-known-record",
        r#"
struct Point {
    int value;
    Point(int input) : value(input) {}
};

Point* make_point() {
    return new Point(4);
}
"#,
    );

    assert!(
        source.contains("Point? make_point()") && source.contains("return Point(4);"),
        "new of a known record must become its Dart construction, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported expression cursor kind 134"),
        "CXXNewExpr must not remain a bailout for a known record, got:\n{source}"
    );
}

/// A pointer to an IR-known record already maps to a nullable Dart reference.
/// Taking the address of a by-reference value is therefore an identity at the
/// Dart boundary, while dereferencing needs the explicit non-null assertion.
#[test]
fn record_pointer_address_and_dereference_lower_to_nullable_dart_references() {
    let source = lower_and_emit(
        "lower-cpp-record-pointer-address-and-dereference",
        r#"
struct Node {
    int value;
};

Node* expose_address(Node& node) {
    return &node;
}

int read_node(Node* node) {
    return (*node).value;
}

bool has_node(Node* node) {
    return node;
}

bool has_no_node(Node* node) {
    return !node;
}
"#,
    );

    assert!(
        source.contains("Node? expose_address(Node node)")
            && source.contains("return node;")
            && source.contains("return node!.value;")
            && source.contains("return node != null;")
            && source.contains("return !(node != null);"),
        "known record pointers must use nullable Dart references, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported unary operator kind"),
        "address and dereference must not remain unary bailouts, got:\n{source}"
    );
}

/// Dart's class inheritance preserves C++'s upcast from `Derived*` to
/// `Base*`; both sides are nullable references after pointer lowering, so the
/// implicit conversion must not turn into an opaque placeholder.
#[test]
fn nullable_record_upcasts_follow_dart_class_inheritance() {
    let source = lower_and_emit(
        "lower-cpp-nullable-record-upcast",
        r#"
struct Base {};
struct Derived : Base {};

Base* as_base(Derived* value) {
    return value;
}
"#,
    );

    assert!(
        source.contains("class Derived extends Base")
            && source.contains("Base? as_base(Derived? value)")
            && source.contains("return value;")
            && !source.contains("unsupported implicit conversion"),
        "nullable upcasts must stay represented in Dart, got:\n{source}"
    );
}

/// F7 (`docs/prompts/2026-08-21-05-downcast-de-hierarquia-preservado.md`):
/// `static_cast`/C-style cast down a class hierarchy (`Base*` → `Derived*`)
/// must not be unwrapped as if it were sugar — `is_transparent_wrapper`
/// already treats `CXXStaticCastExpr`/`CStyleCastExpr` as pure sugar for the
/// numeric-conversion case, but the same unwrap silently dropped a real
/// pointer downcast, passing the operand along still typed as its base. A
/// downcast is emitted as Dart's checked `as T?`, which throws a
/// `TypeError` at runtime if the source wasn't really a `T` — the honest
/// translation of C++'s own unchecked (undefined-behavior-on-mismatch)
/// `static_cast`, never a silent `null`.
#[test]
fn static_cast_downcast_preserves_the_narrower_type() {
    let source = lower_and_emit(
        "lower-cpp-static-cast-downcast",
        r#"
struct Base {};
struct Derivada : Base {};

void f(Derivada* d) {}

void g(Base* b) {
    f(static_cast<Derivada*>(b));
}
"#,
    );

    assert!(
        source.contains("f((b as Derivada?))"),
        "expected the downcast to survive as a checked Dart cast, got:\n{source}"
    );
}

/// The same F7 downcast as above, but immediately dereferenced through the
/// cast result (`vrv_cast<Doc *>(object)->GetSomething()`-shaped —
/// `iomei.cpp`'s real trigger reaches a field, not just a call argument).
/// `emit::dart`'s `receiver_bang` appends `!` straight after whatever text
/// `emit_expr` renders for the receiver, with no parens of its own —
/// unparenthesized, `x as T?!.field` isn't valid Dart (`!` binds to the
/// cast's own `T?` type, not to the cast expression as a whole), confirmed
/// by `dart format` failing on 14 real Verovio files before `Expr::As`
/// started parenthesizing its own output.
#[test]
fn static_cast_downcast_used_as_a_field_receiver_stays_parseable() {
    let source = lower_and_emit(
        "lower-cpp-static-cast-downcast-receiver",
        r#"
struct Base {};
struct Derivada : Base { int valor; };

int g(Base* b) {
    return static_cast<Derivada*>(b)->valor;
}
"#,
    );

    assert!(
        source.contains("(b as Derivada?)!.valor"),
        "expected the parenthesized cast to survive the receiver's `!`, got:\n{source}"
    );
}

/// A user-defined C++ assignment operator may carry invariants beyond a
/// field-for-field copy. Dart has no overloadable assignment, so preserve its
/// body in a named instance method and route `destination = source` through
/// that same method instead of discarding it as an opaque operator call.
#[test]
fn user_defined_assignment_operator_becomes_a_named_dart_method() {
    let source = lower_and_emit(
        "lower-cpp-user-defined-assignment-operator",
        r#"
struct Counter {
    int value;

    Counter& operator=(const Counter& other) {
        value = other.value;
        return *this;
    }
};

void copy(Counter& destination, const Counter& source) {
    destination = source;
}
"#,
    );

    assert!(
        source.contains("Counter assignFrom(Counter other)")
            && source.contains("value = other.value;")
            && source.contains("return this;")
            && source.contains("destination.assignFrom(source);")
            && !source.contains("unsupported operator method call: operator="),
        "user-defined assignment must retain its method body, got:\n{source}"
    );
}

/// Tarefa 08 (F5), Caso 1: `operator=` implicit, right-hand side a freshly
/// constructed temporary — the real Verovio 6.2.0 trigger
/// (`alignfunctor.dart:67`'s `assignFrom(_m_time, Fraction(0));`, always
/// inside a member function assigning to one of its own fields). Overload
/// resolution picks the implicit *move* assignment for an rvalue right-hand
/// side, never the copy overload `lower_defaulted_record_assignment_stmt`
/// already handled — nothing else references the temporary, so a plain Dart
/// assignment is a sound translation, and the declaration-less `assignFrom`
/// bridge must not appear at all.
#[test]
fn implicit_assignment_from_a_freshly_constructed_temporary_is_a_plain_assignment() {
    let source = lower_and_emit(
        "lower-cpp-assign-from-temporary",
        r#"
struct Ponto {
    int x;
    int y;

    Ponto(int x, int y) : x(x), y(y) {}
};

struct Holder {
    Ponto m_point;

    Holder() : m_point(0, 0) {
        m_point = Ponto(1, 2);
    }
};
"#,
    );

    assert!(
        source.contains("m_point = Ponto(1, 2);"),
        "assignment from a temporary must stay a plain Dart assignment, got:\n{source}"
    );
    assert!(
        !source.contains("assignFrom"),
        "a temporary right-hand side must never route through the assignFrom bridge, got:\n{source}"
    );
}

/// Tarefa 08 (F5), Caso 2: `operator=` explicit, called on the receiver's own
/// field through implicit `this` — the exact receiver shape
/// `lower_method_call` used to misread as a normal `obj.method()` call
/// (`collect_children`'s first child is a `MemberRefExpr` in this shape too,
/// indistinguishable by kind alone from `field.assignFrom(...)`'s real
/// receiver), producing the same free two-argument `assignFrom(field, value)`
/// the implicit case did. The declaration must exist on `Contador` and the
/// call site must reach it as a real method call on the field.
#[test]
fn explicit_assignment_operator_reached_through_a_field_receiver_becomes_a_named_method_call() {
    let source = lower_and_emit(
        "lower-cpp-assign-explicit-field-receiver",
        r#"
struct Contador {
    int valor;

    Contador& operator=(const Contador& other) {
        valor = other.valor;
        return *this;
    }
};

struct Caixa {
    Contador m_contador;

    void reset(const Contador& origem) {
        m_contador = origem;
    }
};
"#,
    );

    assert!(
        source.contains("Contador assignFrom(Contador other)"),
        "the explicit assignment operator must still be declared as a named method, got:\n{source}"
    );
    assert!(
        source.contains("m_contador.assignFrom(origem);"),
        "the call site must reach the field through a real method call, not a free two-argument \
         call, got:\n{source}"
    );
    assert!(
        !source.contains("assignFrom(m_contador,"),
        "the field receiver must never be misread as the assignFrom call's first argument, \
         got:\n{source}"
    );
}

/// Tarefa 08 (F5), the dangerous case the prompt itself calls out: `operator=`
/// implicit, right-hand side a *live* object (`Ponto a, b; a = b;`). Overload
/// resolution picks the implicit copy assignment here — already handled by
/// `lower_defaulted_record_assignment_stmt`'s field-by-field
/// `RecordConstruct` — but this pins the exact shape the prompt warns a fix
/// must never regress into: whatever is emitted, it cannot be a call to an
/// `assignFrom` that doesn't exist anywhere in the package.
#[test]
fn implicit_assignment_from_a_live_object_never_calls_a_nonexistent_assign_from() {
    let source = lower_and_emit(
        "lower-cpp-assign-from-live-object",
        r#"
struct Ponto {
    int x;
    int y;
};

void sincroniza(Ponto& a, const Ponto& b) {
    a = b;
}
"#,
    );

    assert!(
        !source.contains("assignFrom"),
        "a live-object right-hand side must never route through the assignFrom bridge, got:\n{source}"
    );
    assert!(
        source.contains("a = Ponto(b.x, b.y);"),
        "a live-object copy assignment must construct a distinct Dart value, got:\n{source}"
    );
}

/// C++ loop control maps directly to Dart. A range-for's collection traversal
/// must remain typed while `continue` and `break` keep their control-flow
/// meaning.
#[test]
fn range_for_continue_and_break_lower_to_dart_control_flow() {
    let source = lower_and_emit(
        "lower-cpp-range-for-control-flow",
        r#"
#include <vector>

int first_nonzero(const std::vector<int>& values) {
    for (int value : values) {
        if (!value) {
            continue;
        }
        break;
    }
    return 0;
}
"#,
    );

    assert!(
        source.contains("for (int value in values)")
            && source.contains("continue;")
            && source.contains("break;")
            && !source.contains("unsupported statement cursor kind 225"),
        "range-for control flow must lower without statement bailouts, got:\n{source}"
    );
}

/// A do-while body runs once before its condition is checked; Dart has the
/// identical construct, so it must not become a plain while-loop bailout.
#[test]
fn do_while_lowers_to_darts_equivalent_control_flow() {
    let source = lower_and_emit(
        "lower-cpp-do-while",
        r#"
int count_once(int limit) {
    int value = 0;
    do {
        value++;
    } while (value < limit);
    return value;
}
"#,
    );

    assert!(
        source.contains("do {")
            && source.contains("value++;")
            && source.contains("} while (value < limit);")
            && !source.contains("unsupported statement cursor kind 208"),
        "do-while must retain its execution order, got:\n{source}"
    );
}

/// A mutable C++ range binding aliases the collection element. Dart's foreach
/// binding is a local value, so a list-backed lowering must write it back even
/// when the body exits through continue or break.
#[test]
fn mutable_vector_range_for_writes_the_binding_back_on_control_flow_exit() {
    let source = lower_and_emit(
        "lower-cpp-mutable-range-for",
        r#"
#include <vector>

void increment_all(std::vector<int>& values) {
    for (int& value : values) {
        ++value;
        if (value == 2) {
            continue;
        }
        if (value == 4) {
            break;
        }
    }
}
"#,
    );

    assert!(
        source.contains("try {")
            && source.contains("finally {")
            && source.contains("_syntaxBridgeIterable[_syntaxBridgeIndex] = value;")
            && !source
                .contains("mutable range-for reference needs a collection write-through adapter"),
        "mutable vector range-for must write each changed binding back, got:\n{source}"
    );
}

/// `for (auto it = list.begin(); it != list.end(); ++it) { ...*it... }` —
/// the classic manual-iterator idiom, common in the real Verovio corpus for
/// `std::list` (round 18, `docs/prompts/2026-08-20-loop-bailout.md`). Must
/// lower to the same `for (final ... in ...)` shape range-for already uses,
/// with `*it` reading as the plain element.
#[test]
fn manual_list_iterator_loop_lowers_to_dart_for_each() {
    let source = lower_and_emit(
        "lower-cpp-manual-list-iterator-loop",
        r#"
#include <list>

int sum_all(const std::list<int>& values) {
    int total = 0;
    for (auto it = values.begin(); it != values.end(); ++it) {
        total += *it;
    }
    return total;
}
"#,
    );

    assert!(
        source.contains("for (final int it in values)") && source.contains("total = total + it;"),
        "expected the manual iterator loop to lower to a Dart for-each, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "manual list iterator loop must not bail out, got:\n{source}"
    );
}

/// `it++` (postfix) is a distinct overload from `++it` (prefix) — same
/// spelling, different call-cursor argument count (a dummy `int` parameter
/// disambiguates it) — and just as common in the real corpus (confirmed by
/// grepping the bundled Verovio source directly). Since this idiom discards
/// the whole `for` header and rebuilds it as a Dart `for`-each, the
/// increment's own value never matters — prefix and postfix must lower
/// identically.
#[test]
fn manual_list_iterator_loop_supports_postfix_increment() {
    let source = lower_and_emit(
        "lower-cpp-manual-list-iterator-postfix",
        r#"
#include <list>

int sum_all(const std::list<int>& values) {
    int total = 0;
    for (auto it = values.begin(); it != values.end(); it++) {
        total += *it;
    }
    return total;
}
"#,
    );

    assert!(
        source.contains("for (final int it in values)") && source.contains("total = total + it;"),
        "expected postfix increment to lower to the same Dart for-each as prefix, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "manual list iterator loop with postfix increment must not bail out, got:\n{source}"
    );
}

/// `it->field` — arrow member access through a `std::list<T>::iterator`
/// where `T` is a project struct, the other half of the idiom
/// (`std::_List_iterator::operator->`, the single largest residual
/// expression cause in the round-17 snapshot).
#[test]
fn manual_list_iterator_loop_supports_arrow_field_access() {
    let source = lower_and_emit(
        "lower-cpp-manual-list-iterator-arrow",
        r#"
#include <list>

struct Item {
    int value;
};

int sum_values(const std::list<Item>& items) {
    int total = 0;
    for (auto it = items.begin(); it != items.end(); ++it) {
        total += it->value;
    }
    return total;
}
"#,
    );

    assert!(
        source.contains("for (final Item it in items)")
            && source.contains("total = total + it.value;"),
        "expected arrow member access through the loop iterator to read the element's field, \
         got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "manual list iterator loop with arrow access must not bail out, got:\n{source}"
    );
}

/// `std::set<T>` uses a different iterator template
/// (`_Rb_tree_const_iterator`) than `std::list`
/// (`_List_iterator`) — confirming the loop recognizer and the
/// `operator*`/`operator->` registry lookup aren't accidentally scoped to
/// only one container family.
#[test]
fn manual_set_iterator_loop_lowers_to_dart_for_each() {
    let source = lower_and_emit(
        "lower-cpp-manual-set-iterator-loop",
        r#"
#include <set>

int sum_all(const std::set<int>& values) {
    int total = 0;
    for (auto it = values.begin(); it != values.end(); ++it) {
        total += *it;
    }
    return total;
}
"#,
    );

    assert!(
        source.contains("for (final int it in values)") && source.contains("total = total + it;"),
        "expected the manual set-iterator loop to lower to a Dart for-each, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "manual set iterator loop must not bail out, got:\n{source}"
    );
}

/// A fixed-size `uint8_t[N]` array decays to `List(Int)` (`lower_type`'s
/// `CXType_ConstantArray` branch has no byte-buffer special case, unlike a
/// `uint8_t*` parameter's own type); passing it where a `const uint8_t*`
/// parameter expects `Uint8List?` needs the implicit-conversion wrapper to
/// bridge the two, the same way `List(Int) → Str` is already bridged for a
/// wide/UTF character-array literal.
#[test]
fn a_fixed_size_byte_array_bridges_to_uint8_list_at_a_buffer_parameter() {
    let source = lower_and_emit(
        "lower-cpp-byte-array-to-bytes",
        r#"
#include <cstdint>

void consume(const uint8_t* data);

void test() {
    uint8_t buffer[] = {1, 2, 3};
    consume(buffer);
}
"#,
    );

    assert!(
        source.contains("consume(Uint8List.fromList(buffer));"),
        "expected the fixed-size byte array to bridge to Uint8List.fromList, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "byte array to buffer parameter must not bail out, got:\n{source}"
    );
}

/// Prefix and postfix increments/decrements are valid Dart update expressions
/// and must not be left as unsupported unary operators in a loop body.
#[test]
fn prefix_and_postfix_updates_lower_to_dart_updates() {
    let source = lower_and_emit(
        "lower-cpp-increment-updates",
        r#"
int count_to(int limit) {
    int total = 0;
    for (int index = 0; index < limit; ++index) {
        total++;
    }
    while (total > 0) {
        --total;
        total--;
    }
    return total;
}
"#,
    );

    assert!(
        source.contains("for (int index = 0; index < limit; ++index)")
            && source.contains("total++;")
            && source.contains("--total;")
            && source.contains("total--;")
            && !source.contains("unsupported unary operator kind 1")
            && !source.contains("unsupported unary operator kind 2")
            && !source.contains("unsupported unary operator kind 3")
            && !source.contains("unsupported unary operator kind 4"),
        "prefix and postfix updates must emit Dart update syntax, got:\n{source}"
    );
}

/// Logical-or and integer bitwise operators have direct Dart syntax and must
/// not become opaque expression bailouts.
#[test]
fn logical_or_and_bitwise_compound_assignments_lower_to_dart_operators() {
    let source = lower_and_emit(
        "lower-cpp-logical-or-and-bitwise",
        r#"
#include <vector>

bool either(bool left, bool right) {
    return left || right;
}

int combine(int value, int mask) {
    value |= mask;
    value <<= 1;
    return value;
}

void increment_first(std::vector<int>& values) {
    values[0] += 1;
}
"#,
    );

    assert!(
        source.contains("return left || right;")
            && source.contains("value = value | mask;")
            && source.contains("value = value << 1;")
            && source.contains("values[0] = values[0] + 1;"),
        "logical-or and bitwise compound assignments must use Dart operators, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported binary operator kind 21")
            && !source.contains("unsupported compound assignment operator kind 32")
            && !source.contains("unsupported compound assignment operator kind 28")
            && !source
                .contains("compound assignment target is not a simple local variable or a field"),
        "direct Dart operators must not remain bailouts, got:\n{source}"
    );
}

/// The C++ conditional operator has the same lazy branch semantics as Dart's
/// ternary expression when all three expressions are already typed.
#[test]
fn a_conditional_operator_lowers_to_a_typed_dart_ternary() {
    let source = lower_and_emit(
        "lower-cpp-conditional-operator",
        r#"
int choose(bool condition, int yes, int no) {
    return condition ? yes : no;
}
"#,
    );

    assert!(
        source.contains("return condition ? yes : no;")
            && !source.contains("unsupported expression cursor kind 116"),
        "the C++ conditional operator must become a Dart ternary, got:\n{source}"
    );
}

/// C++ character literals have integer code-unit type. They must therefore
/// lower to an integer expression, not a one-character Dart String.
#[test]
fn a_character_literal_lowers_to_its_integer_code_unit() {
    let source = lower_and_emit(
        "lower-cpp-character-literal",
        r#"
int letter_a() {
    return 'A';
}
"#,
    );

    assert!(
        source.contains("return 65;") && !source.contains("unsupported expression cursor kind 110"),
        "a character literal must lower to its code unit, got:\n{source}"
    );
}

/// Core string and vector methods have direct, typed Dart counterparts. String
/// search is deliberately byte-based, matching C++ `basic_string` positions
/// instead of Dart's UTF-16 code-unit offsets.
#[test]
fn common_string_and_vector_methods_lower_to_typed_dart_adapters() {
    let source = lower_and_emit(
        "lower-cpp-common-stdlib-methods",
        r#"
#include <string>
#include <vector>

bool blank(const std::string& text) {
    return text.empty();
}

int find_byte(const std::string& text) {
    return text.find("x");
}

void append(std::vector<int>& values, int value) {
    values.push_back(value);
    values.clear();
}

int read(const std::vector<int>& values) {
    return values.at(0);
}
"#,
    );

    assert!(
        source.contains("return text.isEmpty;")
            && source.contains("return utf8.encode(text).indexOf(utf8.encode('x'));")
            && source.contains("values.add(value);")
            && source.contains("values.clear();")
            && source.contains("return values[0];"),
        "common stdlib methods must use typed Dart adapters, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::basic_string::empty call")
            && !source.contains("unsupported std::basic_string::find call")
            && !source.contains("unsupported std::vector::push_back call")
            && !source.contains("unsupported std::vector::clear call")
            && !source.contains("unsupported std::vector::at call"),
        "the mapped stdlib methods must not remain bailouts, got:\n{source}"
    );
}

/// String indexing and c_str are byte-oriented C++ APIs. The former must use
/// UTF-8 bytes rather than Dart UTF-16 code units; the latter can stay a Dart
/// String only in the already-string-typed return boundary.
#[test]
fn string_byte_index_and_c_str_lower_without_stdlib_bailouts() {
    let source = lower_and_emit(
        "lower-cpp-string-byte-operations",
        r#"
#include <string>

int first_byte(const std::string& text) {
    return text[0];
}

int byte_at(const std::string& text) {
    return text.at(1);
}

const char* text_pointer(const std::string& text) {
    return text.c_str();
}
"#,
    );

    assert!(
        source.contains("return utf8.encode(text)[0];")
            && source.contains("return utf8.encode(text)[1];")
            && source.contains("String? text_pointer(String text)")
            && source.contains("return text;"),
        "string byte operations must use typed Dart bridges, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::basic_string::operator[] call")
            && !source.contains("unsupported std::basic_string::at call")
            && !source.contains("unsupported std::basic_string::c_str call"),
        "the supported string byte operations must not remain bailouts, got:\n{source}"
    );
}

/// String concatenation assignment preserves immutable String value semantics
/// in Dart by expanding to one assignment over the existing value.
#[test]
fn std_string_append_assignment_lowers_to_dart_plus_equals() {
    let source = lower_and_emit(
        "lower-cpp-std-string-append-assignment",
        r#"
#include <string>

void append_marker(std::string& text) {
    text += "!";
}
"#,
    );

    assert!(
        source.contains("text = text + '!';")
            && !source.contains("unsupported std::basic_string::operator+= call"),
        "std::string operator+= must become a typed Dart reassignment, got:\n{source}"
    );
}

/// Mutating basic_string calls have to become a reassignment because Dart
/// String is immutable. clear and one-argument append have that exact value
/// semantics without needing a byte-buffer bridge.
#[test]
fn std_string_mutating_calls_lower_to_typed_reassignments() {
    let source = lower_and_emit(
        "lower-cpp-std-string-mutating-calls",
        r#"
#include <string>

void reset_and_append(std::string& text) {
    text.clear();
    text.append("ok");
}
"#,
    );

    assert!(
        source.contains("text = '';") && source.contains("text = text + 'ok';"),
        "mutating string calls must become String reassignments, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::basic_string::clear call")
            && !source.contains("unsupported std::basic_string::append call"),
        "supported mutating string calls must not remain bailouts, got:\n{source}"
    );
}

/// (c) — `docs/prompts/2026-08-21-06-bailout-tipado-e-opaque-compartilhado.md`:
/// real corpus trigger (`iomei.dart`/`jsonxx.dart`/`humlib.dart` in the
/// Verovio 6.2.0 diagnosis) — chaining a one-argument `append` (which
/// reassigns its receiver) onto a receiver that itself failed to lower
/// (here, a two-argument `append(s, n)` overload this bridge doesn't
/// support) used to build `Stmt::ExprAssign` with the *bailout itself* as
/// the assignment target: `_syntaxBridgeUnsupported<...>(...) = ... ;`.
/// That's not just semantically wrong, it's not valid Dart syntax at
/// all — Dart reads the callable-looking left side as an attempted
/// destructuring pattern and rejects it with unrelated pattern errors
/// (`not_a_type`, `positional_field_in_object_pattern`,
/// `refutable_pattern_in_irrefutable_context`, all confirmed on the same
/// line in the real diagnosis). The whole statement must escalate to an
/// honest `Stmt::Unsupported` instead.
#[test]
fn an_append_chained_onto_an_unrepresentable_receiver_bails_out_the_whole_statement_not_just_the_target()
 {
    let source = lower_and_emit(
        "lower-cpp-append-chained-onto-unsupported-receiver",
        r#"
#include <string>

void build(std::string& text, const char* extra, int extra_len, char terminator) {
    text.append(extra, extra_len).push_back(terminator);
}
"#,
    );

    assert!(
        !source.contains(") = _syntaxBridgeUnsupported"),
        "a bailout must never reach an assignment target position, got:\n{source}"
    );
    assert!(
        source.contains("throw UnimplementedError("),
        "expected the whole statement to bail out honestly instead, got:\n{source}"
    );
}

/// `std::basic_string::push_back(char)` — a real Verovio idiom, confirmed
/// by grepping the extracted source directly: `toolkit.cpp`'s
/// `option_str.push_back(option->GetShortOption())` and `iopae.cpp`'s
/// `paeStr.push_back(token.m_char)`. The single-`char` argument is this
/// IR's `Type::Int` (a code unit), so the reassignment goes through
/// `String.fromCharCode` rather than `append`'s direct `Str + Str`.
#[test]
fn std_string_push_back_appends_a_char_code_via_string_from_char_code() {
    let source = lower_and_emit(
        "lower-cpp-std-string-push-back",
        r#"
#include <string>

void append_char(std::string& text, char c) {
    text.push_back(c);
}
"#,
    );

    assert!(
        source.contains("text = text + String.fromCharCode(c);"),
        "push_back must become a typed Dart reassignment, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::basic_string::push_back call"),
        "got:\n{source}"
    );
}

/// `std::vector<T>::resize` — real Verovio idiom, confirmed by grepping
/// the extracted source directly: `staff.cpp`'s `lines.resize(count)` and
/// `iohumdrum.cpp`'s repeated `m_placement.resize(1000)`. Dart's own
/// `List.length` setter only shrinks safely (growing pads with `null`,
/// which throws at runtime for a non-nullable element type like `int`), so
/// growth goes through `addAll(List.filled(...))` instead — both real
/// branches are exercised here (shrink and grow) against the same target.
#[test]
fn vector_resize_shrinks_via_length_and_grows_via_list_filled() {
    let source = lower_and_emit(
        "lower-cpp-vector-resize",
        r#"
#include <vector>

void shrink(std::vector<int>& values, int count) {
    values.resize(count);
}
"#,
    );

    assert!(
        source.contains("if (count < values.length) {")
            && source.contains("values.length = count;")
            && source.contains("} else {")
            && source.contains("values.addAll(List.filled(count - values.length, 0));"),
        "expected an explicit shrink/grow split with a typed default fill, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::vector::resize call"),
        "got:\n{source}"
    );
}

/// Round 22: growing a `std::vector<Record>` needs the *record's* zero
/// value, not just a scalar's — real trigger `humlib.h`'s `MyCoord { int
/// x; int y; }`, used as `std::vector<MyCoord> sclef;` then
/// `sclef.resize(0)` (`MeasureInfo::clear`). The one-argument overload's
/// default fill used to fall straight to `default_scalar_value`, which
/// bails on *any* `Type::Record` element unconditionally — even one this
/// bridge can trivially zero-construct field by field.
#[test]
fn vector_resize_grows_a_record_element_with_a_field_by_field_default() {
    let source = lower_and_emit(
        "lower-cpp-vector-resize-record-element",
        r#"
#include <vector>

struct Coord {
    int x;
    int y;
};

void grow(std::vector<Coord>& values, int count) {
    values.resize(count);
}
"#,
    );

    assert!(
        source.contains("values.addAll(List.filled(count - values.length, Coord(0, 0)));"),
        "expected the record element's own field-by-field zero value, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "growing a vector of a trivially-defaultable record must not bail out, got:\n{source}"
    );
}

/// compare and substr have direct, typed Dart String counterparts. The C++
/// length overload of substr is translated to an exclusive Dart end index.
#[test]
fn std_string_compare_and_substr_lower_to_dart_string_operations() {
    let source = lower_and_emit(
        "lower-cpp-std-string-compare-substr",
        r#"
#include <string>

int compare_text(const std::string& left, const std::string& right) {
    return left.compare(right);
}

std::string tail(const std::string& text) {
    return text.substr(2);
}

std::string middle(const std::string& text) {
    return text.substr(2, 3);
}
"#,
    );

    assert!(
        source.contains("return left.compareTo(right);")
            && source.contains("return text.substring(2);")
            && source.contains("return text.substring(2, 2 + 3);"),
        "compare and substr must use typed Dart String operations, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported std::basic_string::compare call")
            && !source.contains("unsupported std::basic_string::substr call"),
        "supported String operations must not remain bailouts, got:\n{source}"
    );
}

/// `std::tuple` has a direct structural counterpart in Dart's record types.
/// Keeping it typed at an API boundary does not claim that every `std::get`
/// expression is already lowered; those operations retain their own explicit
/// expression-level coverage.
#[test]
fn a_std_tuple_lowers_to_a_typed_dart_record() {
    let source = lower_and_emit(
        "lower-cpp-std-tuple",
        r#"
#include <string>
#include <tuple>

void consume(std::tuple<int, std::string, bool> value) {}
"#,
    );

    assert!(
        source.contains("void consume((int, String, bool) value)"),
        "a std::tuple must preserve every typed slot as a Dart record, got:\n{source}"
    );
    assert!(
        !source.contains("std::tuple") && !source.contains("SyntaxBridgeOpaque"),
        "a std::tuple signature must not remain an opaque type bailout, got:\n{source}"
    );
}

/// C++ has many scalar spellings but Dart only needs `int` and `double` for
/// their value domains. None of these ordinary scalar types may cross the
/// lowering boundary as `Type::Unsupported`, which would make the emitter
/// fall back to `dynamic` instead of producing a Dart scalar signature.
#[test]
fn c_and_cpp_scalar_types_lower_to_the_closest_dart_scalars() {
    let source = lower_and_emit(
        "lower-cpp-all-scalars",
        r#"
double scalar_bridge(
    unsigned int unsigned_count,
    unsigned short short_count,
    signed char signed_byte,
    char byte,
    wchar_t wide_char,
    char16_t utf16_unit,
    char32_t unicode_code_point,
    short signed_short,
    long signed_long,
    long long signed_long_long,
    unsigned long long unsigned_long_long,
    float ratio,
    long double precise) {
  return ratio;
}
"#,
    );

    assert!(
        source.contains(
            "double scalar_bridge(int unsigned_count, int short_count, int signed_byte, int byte, int wide_char, int utf16_unit, int unicode_code_point, int signed_short, int signed_long, int signed_long_long, int unsigned_long_long, double ratio, double precise)"
        ),
        "ordinary C/C++ scalars must use Dart scalar types, got:\n{source}"
    );
    assert!(
        !source.contains("dynamic"),
        "scalar lowering must not leave a dynamic placeholder, got:\n{source}"
    );
}

/// Clang propagates a nested enum's access specifier onto each of its
/// `EnumConstantDecl`s, so a default-private `enum` inside a class had its
/// *references* run through `dart_member_name` (which prefixes `_`) while
/// its *declaration* used the raw spelling: `enum Cor { Vermelho }` declared,
/// `Cor._Vermelho` referenced — an undefined enum constant.
#[test]
fn a_private_nested_enums_constant_is_referenced_by_the_name_it_was_declared_with() {
    let source = lower_and_emit(
        "lower-cpp-private-nested-enum",
        r#"
class C {
    enum Cor { Vermelho, Azul };
public:
    Cor f() { return Vermelho; }
};
"#,
    );

    assert!(
        !source.contains("._Vermelho"),
        "the reference must not gain a privacy underscore the declaration lacks, got:\n{source}"
    );
    assert!(
        source.contains("Vermelho"),
        "expected the enumerator to survive lowering, got:\n{source}"
    );
}

/// Tarefa 03 (`docs/prompts/2026-08-21-03-*.md`, família F2): `protected` in
/// C++ means "visible to subclasses", but Dart's `_` means library-private,
/// and `emit::dart::emit_file` puts every record in its own file/library —
/// so a `protected` field prefixed with `_` becomes invisible to the very
/// subclasses C++ granted access to. A subclass reading its base's
/// `protected` field must see the same name the base declared it with, with
/// no leading `_`.
#[test]
fn a_protected_fields_name_is_visible_without_a_privacy_underscore_to_a_subclass() {
    let source = lower_and_emit(
        "lower-cpp-protected-field-visible-to-subclass",
        r#"
class Base {
protected:
    int m_value;
};
class Derived : public Base {
public:
    int read() { return m_value; }
};
"#,
    );

    assert!(
        !source.contains("_m_value"),
        "a protected field must not gain a library-privacy underscore, got:\n{source}"
    );
    assert!(
        source.contains("m_value"),
        "expected the protected field to survive lowering under its C++ name, got:\n{source}"
    );
}

/// A sibling bug to the one above, same root cause: `pugixml.hpp`'s
/// `xml_node::_root` is `protected` — the previous test's fix already
/// applies — but its *C++ spelling itself* starts with `_` (a leading-
/// underscore-for-"internal" convention some C++ codebases use regardless
/// of access specifier). Dart's privacy rule looks at the literal
/// identifier text, not at any access-specifier side channel, so a bare
/// pass-through of that spelling still reads as library-private and is
/// still invisible to a subclass in another file — the exact symptom the
/// test above fixed, wearing a different hat. Verified against the real
/// diagnosis run (1049 identical `undefined_getter` occurrences before and
/// after the access-specifier fix alone).
#[test]
fn a_protected_fields_leading_underscore_is_stripped_so_it_does_not_trip_darts_own_privacy_convention()
 {
    let source = lower_and_emit(
        "lower-cpp-protected-leading-underscore-field",
        r#"
class Base {
protected:
    int _root;
};
class Derived : public Base {
public:
    int read() { return _root; }
};
"#,
    );

    assert!(
        !source.contains("_root"),
        "a protected member whose C++ spelling already starts with `_` must not keep it, got:\n{source}"
    );
    assert!(
        source.contains("int root;") && source.contains("return root;"),
        "expected the leading underscore stripped consistently at declaration and reference, got:\n{source}"
    );
}

/// The companion to the two tests above: `private` (unlike `protected`) really is
/// invisible outside the declaring class in C++ too, so it must keep the
/// leading `_` — the fix for `protected` must not become "drop the
/// underscore from everything".
#[test]
fn a_private_field_still_gets_a_privacy_underscore() {
    let source = lower_and_emit(
        "lower-cpp-private-field-keeps-underscore",
        r#"
class C {
public:
    int read() { return m_value; }
private:
    int m_value;
};
"#,
    );

    assert!(
        source.contains("_m_value"),
        "expected a private field to keep its privacy underscore, got:\n{source}"
    );
}

/// A bare `;` (`CXCursor_NullStmt`) — a stray empty statement, C++'s idiom
/// for "nothing happens here" (a loop with all its work in the clauses, or
/// simply a redundant semicolon) — has no Dart form worth bailing out on: it
/// contributes nothing to any statement list, so it should just vanish
/// rather than becoming a `Stmt::Unsupported` TODO+throw that fails the
/// *whole enclosing function*.
#[test]
fn a_null_statement_is_omitted_rather_than_becoming_a_bailout() {
    let source = lower_and_emit(
        "lower-cpp-null-stmt",
        r#"
void tick(int n) {
    for (int i = 0; i < n; i++)
        ;
}
"#,
    );

    assert!(
        !source.contains("unsupported statement cursor kind"),
        "a bare `;` must never become a bailout, got:\n{source}"
    );
}

/// A bare `{ ... }` scoping block used as an ordinary statement (not an
/// `if`/`while`/`for` body, which `lower_branch` already unwraps) — C++
/// allows one anywhere a statement is expected, usually just to scope a
/// local's lifetime. Dart accepts a bare block too, but this IR has no
/// nested-block statement node, so the block's own statements are inlined
/// directly into the enclosing list instead — flattening loses C++'s block
/// scoping, but every name this IR lowers already has a scope no narrower
/// than its enclosing function in practice (no fixture shadows a name across
/// a nested block), so the flatten is observationally the same program.
#[test]
fn a_nested_bare_block_is_flattened_into_its_enclosing_statement_list() {
    let source = lower_and_emit(
        "lower-cpp-nested-block",
        r#"
int scoped() {
    {
        int x = 1;
        return x;
    }
}
"#,
    );

    assert!(
        !source.contains("unsupported statement cursor kind"),
        "a nested bare block must never become a bailout, got:\n{source}"
    );
    assert!(
        source.contains("int x = 1;") && source.contains("return x;"),
        "expected the block's statements to survive, flattened, got:\n{source}"
    );
}

/// `int a = 1, b = 2;` — one `DeclStmt` cursor with multiple `VarDecl`
/// children, C++'s comma-separated multi-declarator form. `DeclStmt had N
/// declarators, expected exactly 1` was the single largest statement-level
/// bailout family (136+ occurrences of exactly 2 declarators, plus smaller
/// counts for 3–22) in the 2026-08-20 real-Verovio diagnosis run. Each
/// declarator becomes its own ordinary `VarDecl` statement, in source
/// order, with the exact same per-declarator lowering a single-declarator
/// `DeclStmt` already gets — nothing new to prove about *how* one variable
/// lowers, only that N of them in one C++ statement produce N Dart
/// statements instead of one combined bailout.
#[test]
fn a_multi_declarator_decl_statement_splits_into_one_var_decl_per_declarator() {
    let source = lower_and_emit(
        "lower-cpp-multi-declarator",
        r#"
int sum_of_two() {
    int a = 1, b = 2;
    return a + b;
}
"#,
    );

    assert!(
        !source.contains("unsupported statement cursor kind") && !source.contains("DeclStmt had"),
        "a multi-declarator DeclStmt must never become a bailout, got:\n{source}"
    );
    assert!(
        source.contains("int a = 1;") && source.contains("int b = 2;"),
        "expected one VarDecl statement per declarator, got:\n{source}"
    );
}

/// `switch`/`case`/`default` — the largest single unmapped statement cursor
/// kind in the 2026-08-20 real-Verovio diagnosis run (148 occurrences).
/// Covers a stacked label sharing one body (`case 2: case 3: ...`, C++'s
/// idiom for "these values do the same thing") and `default`, each
/// break-terminated the way Dart requires.
#[test]
fn a_switch_statement_lowers_to_darts_switch_including_a_stacked_case_label() {
    let source = lower_and_emit(
        "lower-cpp-switch",
        r#"
int classify(int level) {
    int result = 0;
    switch (level) {
        case 1:
            result = 10;
            break;
        case 2:
        case 3:
            result = 20;
            break;
        default:
            result = -1;
            break;
    }
    return result;
}
"#,
    );

    assert!(
        !source.contains("unsupported statement cursor kind")
            && !source.contains("SwitchStmt")
            && !source.contains("falls through"),
        "a well-formed switch must never become a bailout, got:\n{source}"
    );
    assert!(
        source.contains("switch (level) {"),
        "expected a Dart switch statement, got:\n{source}"
    );
    assert!(
        source.contains("case 1:") && source.contains("case 2:") && source.contains("case 3:"),
        "expected every case label to survive, including the stacked one, got:\n{source}"
    );
    assert!(
        source.contains("default:"),
        "expected the default label to survive, got:\n{source}"
    );
}

/// A case whose body doesn't end in `break`/`continue`/`return`/`throw`
/// really does fall through into the next one in C++ — Dart has no
/// implicit-fallthrough form to lower that into (yet), so the honest
/// outcome is a bailout, not Dart that `dart format`/`dart analyze` would
/// reject for exactly the reason the C++ itself is unusual.
#[test]
fn a_switch_with_genuine_fallthrough_stays_an_honest_bailout() {
    // Case-to-case fallthrough (`case 1` into `case 2`) is now supported
    // via Dart's own `continue <label>;` syntax — see
    // `a_case_that_falls_through_into_the_next_one_uses_darts_own_continue_label_syntax`.
    // Falling through *into* `default` specifically stays unsupported:
    // `default` has no label slot to target (`emit::dart` always prints it
    // last, and `Stmt::Switch.default` is a bare `Vec<Stmt>`, not a
    // `SwitchCase`) — a narrower, real gap.
    let source = lower_and_emit(
        "lower-cpp-switch-fallthrough-into-default",
        r#"
int log_level(int level) {
    int result = 0;
    switch (level) {
        case 1:
            result = 1;
        default:
            result = 2;
            break;
    }
    return result;
}
"#,
    );

    assert!(
        source.contains("falls through"),
        "expected an honest bailout naming the fallthrough, got:\n{source}"
    );
    assert!(
        !source.contains("dynamic"),
        "a bailout must never use `dynamic`, got:\n{source}"
    );
}

/// C++ implicitly converts an enumerator to its underlying integer value
/// wherever an `int` is expected — one of the highest-volume bailout
/// families in the real Verovio 6.2.0 corpus (`data_DURATION`,
/// `data_BEAMPLACE`, `data_STEMDIRECTION`, 453 occurrences across just
/// those three enums as of the 2026-08-20 diagnosis run). The naive Dart
/// translation, `.index`, is *only* correct when the C++ enumerators are
/// declared with no explicit values, in source order, starting at 0 — never
/// guaranteed, and Verovio itself declares gapped/non-sequential values.
/// This fixture's `Segundo = 5` (not `1`) is exactly the case a `.index`-based
/// conversion would get silently wrong: `dart format`-clean, `dart
/// analyze`-clean, and a different number than the C++ program computes —
/// precisely the "compiles and is wrong" failure `Type::Unsupported`'s
/// design exists to rule out. The Dart enum must therefore carry its real
/// C++ value explicitly, not rely on declaration order.
#[test]
fn lowers_an_implicit_enum_to_int_conversion_using_the_enums_real_cpp_value_not_its_dart_index() {
    let source = lower_and_emit(
        "lower-cpp-enum-to-int",
        r#"
enum Nivel { Primeiro = 0, Segundo = 5, Terceiro = 6 };

int as_int(Nivel nivel) {
    int codigo = nivel;
    return codigo;
}
"#,
    );

    assert!(
        source.contains("nivel.value"),
        "expected the conversion to read the enum's explicit backing value, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported implicit conversion from Enum"),
        "got:\n{source}"
    );
    assert!(
        source.contains("Segundo(5)"),
        "expected the enum declaration to carry its real C++ value (5), not \
         its Dart declaration index (1), got:\n{source}"
    );
}

/// A qualified member call (`Base::foo()`, `this->Base::foo()`,
/// `ns::Base::foo()`) disambiguates *which* base implementation to call —
/// common in Verovio wherever a derived class explicitly re-invokes a base
/// method instead of relying on virtual dispatch. Confirmed via `clang++
/// -Xclang -ast-dump`: the qualifier attaches to the `MemberExpr`/
/// `MemberRefExpr` as a `NestedNameSpecifier`, which `libclang`'s cursor API
/// surfaces as a `TypeRef` (and, when namespace-qualified, a `NamespaceRef`
/// too) sibling cursor of the (usually implicit, so invisible to
/// `clang_visitChildren`) receiver. For an implicit-`this` qualified call,
/// that `TypeRef` is the *only* child `member_ref_receiver` sees — it took
/// the one-child branch and tried to `lower_expr` the `TypeRef` cursor
/// itself as if it were the receiver, landing on "unsupported expression
/// cursor kind 43" (206 occurrences in the 2026-08-20 Verovio diagnosis)
/// instead of resolving to `this`. The qualifier is disambiguation
/// information already resolved by `clang_getCursorReferenced` on the call
/// itself; it should be filtered out like the `TypeRef`/`NamespaceRef`/
/// `TemplateRef` noise already filtered elsewhere in this file (E03/E07/
/// `is_transparent_wrapper`), leaving 0 children (implicit `this`) or 1
/// (the real receiver).
#[test]
fn a_qualified_base_member_call_ignores_the_disambiguating_namespace_and_type_refs() {
    let source = lower_and_emit(
        "lower-cpp-qualified-member-call",
        r#"
class Base {
public:
    int Foo() { return 1; }
};

class Derived : public Base {
public:
    int Bar() {
        return Base::Foo();
    }
};
"#,
    );

    assert!(
        !source.contains("member reference had") && !source.contains("cursor kind 43"),
        "expected the qualified base call to resolve to `this`/the receiver, got:\n{source}"
    );
    assert!(
        source.contains("Foo()"),
        "expected the qualified call itself to still lower, got:\n{source}"
    );
}

/// F12/tarefa 09 (`docs/prompts/2026-08-21-09-chamada-a-base-qualificada.md`):
/// `B::f()` calling `A::f();` (the base implementation, by qualified name)
/// used to lower with the qualifier discarded — the emitted Dart called
/// `f()` bare, which resolves to `B`'s own override, infinite recursion.
/// Single inheritance is the simple case: `B extends A` directly, no mixin
/// linearization to disambiguate, so the qualified call always has exactly
/// one place `super.f()` can resolve to.
#[test]
fn a_qualified_base_call_in_single_inheritance_emits_super_not_a_self_recursive_call() {
    let source = lower_and_emit(
        "lower-cpp-qualified-base-call-single-inheritance",
        r#"
class A {
public:
    virtual void f() {}
};

class B : public A {
public:
    void f() override {
        A::f();
    }
};
"#,
    );

    assert!(
        source.contains("super.f()"),
        "expected the qualified base call to emit `super.f()`, got:\n{source}"
    );
    assert!(
        !source.contains("void f() {\n    f();"),
        "the qualified base call must not lower to a bare self-recursive call, got:\n{source}"
    );
}

/// F12/tarefa 09's dangerous case: multiple inheritance flattens into a
/// mixin list (`with M1, M2`), and Dart's own `super` resolves to the *last*
/// mixin in that list that declares the member — here `M2`, since both
/// declare `reset`. The C++ source names `M1` explicitly, which is *not*
/// the one Dart's `super.reset()` would actually reach — emitting `super.
/// reset()` anyway would silently call `M2`'s implementation instead of
/// `M1`'s, which is worse than the original recursion bug (it looks like it
/// works). The correct output is an honest bailout, never a guessed
/// `super.`.
#[test]
fn a_qualified_base_call_that_mismatches_dart_mixin_linearization_bails_out() {
    let source = lower_and_emit(
        "lower-cpp-qualified-base-call-linearization-mismatch",
        r#"
class M1 {
public:
    virtual void reset() {}
};

class M2 {
public:
    virtual void reset() {}
};

class D : public M1, public M2 {
public:
    void reset() override {
        M1::reset();
    }
};
"#,
    );

    assert!(
        !source.contains("super.reset()"),
        "expected an honest bailout, not a `super.reset()` that would silently \
         resolve to the wrong mixin, got:\n{source}"
    );
    assert!(
        !source.contains("void reset() {\n    reset();"),
        "the mismatched qualified base call must not fall back to a bare \
         self-recursive call either, got:\n{source}"
    );
    assert!(
        source.contains("does not resolve"),
        "expected the mismatch to surface as an explicit bailout reason, got:\n{source}"
    );
}

/// F12/tarefa 09's non-regression case: an ordinary virtual call (no C++
/// qualifier at all, whether spelled bare or through `this->`) must keep
/// emitting a plain, unqualified call — never `super.`, which would change
/// its dispatch (skip the most-derived override at every real virtual call
/// site, not just the qualified ones this family targets).
#[test]
fn an_unqualified_virtual_call_from_an_override_still_emits_a_bare_call() {
    let source = lower_and_emit(
        "lower-cpp-unqualified-virtual-call-non-regression",
        r#"
class A {
public:
    virtual void f() {}
    virtual void g() { f(); }
};

class B : public A {
public:
    void f() override { this->g(); }
};
"#,
    );

    assert!(
        !source.contains("super."),
        "an unqualified/virtual call must never emit `super.`, got:\n{source}"
    );
    assert!(
        source.contains("g()"),
        "expected the unqualified call to still lower, got:\n{source}"
    );
}

/// `std::vector<int> v = {1, 2, 3};` — a brace initializer for a
/// `vector`/`array`/`deque`, invoking their `initializer_list` constructor.
/// `lower::cpp` had no `Expr` shape for a brace-enclosed initializer list at
/// all (`unsupported expression cursor kind 119`, 181 occurrences in the
/// 2026-08-20 Verovio diagnosis), and the *outer* constructor call — reached
/// even after the list itself lowers — named a Dart function/class that
/// never exists (`vector(<int>[1, 2, 3])`, since `std::vector` maps to
/// `Type::List` and was deliberately never `lower_record`'d, the same
/// reasoning `Type::Str`/`basic_string` already documents). Both had to be
/// fixed together: the list becomes a Dart list literal
/// (`Expr::ListLiteral`, only ever produced when `clang_getCursorType` on
/// the `InitListExpr` cursor itself already resolves to `List<T>`), and the
/// `initializer_list` constructor call recognizes that shape and returns the
/// literal directly instead of wrapping it in a call to a nonexistent
/// `vector`/`array`/`deque` function.
#[test]
fn a_vector_initializer_list_lowers_to_a_dart_list_literal_without_a_bogus_constructor_call() {
    let source = lower_and_emit(
        "lower-cpp-vector-init-list",
        r#"
#include <vector>
std::vector<int> f() {
    std::vector<int> v = {1, 2, 3};
    return v;
}
"#,
    );

    assert!(
        source.contains("<int>[1, 2, 3]"),
        "expected a real Dart list literal, got:\n{source}"
    );
    assert!(
        !source.contains("vector("),
        "the initializer-list constructor call must not name a nonexistent `vector` function, got:\n{source}"
    );
    assert!(!source.contains("cursor kind 119"), "got:\n{source}");
}

/// Real trigger: `midifunctor.cpp`/`iocmme.cpp`'s static const lookup
/// tables, `static const std::map<int, data_DURATION> durationEq{ { a, b },
/// ... };`. Unlike `std::vector`'s flat initializer list, each of
/// `std::map`'s entries is itself a nested 2-element `{ key, value }`
/// list — a shape `lower_expr`'s generic `InitListExpr` handling (scoped
/// to `Type::List`'s flat elements) never reaches.
#[test]
fn a_map_initializer_list_lowers_to_a_dart_map_literal() {
    let source = lower_and_emit(
        "lower-cpp-map-init-list",
        r#"
#include <map>
std::map<int, int> f() {
    static const std::map<int, int> lookup{
        { 1, 10 },
        { 2, 20 },
    };
    return lookup;
}
"#,
    );

    assert!(
        source.contains("<int, int>{1: 10, 2: 20}"),
        "expected a real Dart map literal, got:\n{source}"
    );
    assert!(
        !source.contains("map("),
        "the initializer-list constructor call must not name a nonexistent `map` function, got:\n{source}"
    );
    assert!(!source.contains("cursor kind 119"), "got:\n{source}");
}

/// Same real-corpus shape, but with `unordered_map` (the other container
/// this applies to) and an enum-constant value, matching
/// `iocmme.cpp`'s `stemDirMap`/`accidMap` exactly (string/int key,
/// `data_*` enum value).
#[test]
fn an_unordered_map_initializer_list_with_enum_values_lowers_to_a_dart_map_literal() {
    let source = lower_and_emit(
        "lower-cpp-unordered-map-init-list-enum",
        r#"
#include <unordered_map>
enum Color { RED, GREEN, BLUE };
Color f(int i) {
    static const std::unordered_map<int, Color> m{
        { 1, RED },
        { 2, GREEN },
    };
    return m.at(i);
}
"#,
    );

    assert!(
        source.contains("Color.RED") && source.contains("Color.GREEN"),
        "expected the enum-constant values to survive as real Dart enum references, got:\n{source}"
    );
    assert!(!source.contains("cursor kind 119"), "got:\n{source}");
    assert!(!source.contains("unordered_map("), "got:\n{source}");
}

/// Real trigger: `adjustaccidxfunctor.cpp:25`'s `m_currentMeasure = NULL;`
/// (a constructor field-init assignment), plus the same idiom in a
/// comparison and a local-variable initializer — the pre-`nullptr`
/// null-pointer-constant style used throughout the corpus. `<cstddef>`
/// defines `NULL` as GNU's `__null` builtin (confirmed via `clang -E`, not
/// assumed) — a distinct `CXCursor_GNUNullExpr` cursor, reported with type
/// `long` rather than folding to a plain integer literal the way a bare
/// `0` already does.
#[test]
fn a_null_pointer_constant_lowers_to_a_dart_null_literal() {
    let source = lower_and_emit(
        "lower-cpp-null-pointer-constant",
        r#"
#include <cstddef>
struct Measure {};

struct Functor {
    Measure* m_currentMeasure;
    Functor() {
        m_currentMeasure = NULL;
    }
};

Measure* find_last(Measure* measure) {
    if (measure != NULL) {
        return measure;
    }
    Measure* none = NULL;
    return none;
}
"#,
    );

    assert!(
        source.contains("m_currentMeasure = null;")
            && source.contains("measure != null")
            && source.contains("Measure? none = null;"),
        "expected every NULL idiom to lower to Dart's own null literal, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported implicit conversion from Int to Nullable")
            && !source.contains("unsupported expression cursor kind 123"),
        "got:\n{source}"
    );
}

/// `m_data[i]` where `m_data` is a fixed-size array *field*
/// (`int m_data[10];`) — very common in Verovio's own C-style buffers.
/// `lower_array_subscript_expr` already recovered the true declared type
/// (bypassing C++'s array-to-pointer decay, which would otherwise make the
/// subscript's target look like a raw pointer instead of the `List<int>`
/// the field itself lowers to) for a bare local/global variable
/// (`DeclRefExpr`), but not for a field accessed via `MemberRefExpr` —
/// implicit `this` (`m_data[i]`, inside a method) or explicit
/// (`this->m_data[i]`) alike. Both hit "array subscript receiver is not a
/// lowered Dart collection" (359 occurrences in the 2026-08-20 Verovio
/// diagnosis) even though the field declaration right above it already
/// prints as `List<int> m_data`.
#[test]
fn a_fixed_array_field_indexed_through_implicit_or_explicit_this_is_indexable() {
    let source = lower_and_emit(
        "lower-cpp-array-field-index",
        r#"
class Holder {
public:
    int m_data[10];
    int First() { return m_data[0]; }
    int Second(int i) { return this->m_data[i]; }
};
"#,
    );

    assert!(
        !source.contains("array subscript receiver"),
        "expected both forms to index the field directly, got:\n{source}"
    );
    assert!(
        source.contains("return m_data[0];") && source.contains("return m_data[i];"),
        "expected a plain Dart index expression (implicit `this` needs no prefix), got:\n{source}"
    );
}

/// `m_rows[i][j]` — a multidimensional fixed array field, indexed twice.
/// Even after a single-level field index works (see the sibling test
/// above), the outer subscript's target is the *already-lowered* inner
/// `m_rows[i]` result — but C++ wraps it in an implicit array-to-pointer
/// decay conversion first, since the built-in `E1[E2]` requires `E1` to be
/// a pointer. Lowering that wrapper the generic way (comparing its outer
/// pointer-decayed type against the inner `List` type) doesn't know the
/// decay is moot once the inner subscript is already a Dart `List` — it
/// only recognizes real scalar/pointer/enum conversions, so it bailed.
#[test]
fn a_nested_array_subscript_on_a_multidimensional_fixed_array_field_lowers_directly() {
    let source = lower_and_emit(
        "lower-cpp-nested-array-field-index",
        r#"
class Grid {
public:
    int m_rows[4][4];
    int At(int i, int j) { return m_rows[i][j]; }
};
"#,
    );

    assert!(
        !source.contains("array subscript receiver")
            && !source.contains("unsupported implicit conversion"),
        "expected the nested index to lower directly, got:\n{source}"
    );
    assert!(source.contains("return m_rows[i][j];"), "got:\n{source}");
}

/// Calling a value *held* by a field/parameter, rather than a named
/// function — `m_callback(value)`/`this->m_callback(value)` (a callback
/// field, common for observer/visitor hooks in Verovio) and `cb(value)` (a
/// callback parameter). `clang_getCursorReferenced` on the call resolves to
/// the `FieldDecl`/`ParmDecl` holding the value, not a `FunctionDecl` — the
/// only call-target shape `lower_call_expr` recognized before this fix
/// ("unsupported call target cursor kind 6/10", 96+5 occurrences in the
/// 2026-08-20 Verovio diagnosis). When that declaration's own type already
/// lowers to a representable `Type::Callback` (a real C function pointer),
/// Dart's own call syntax needs no adapter at all.
#[test]
fn calling_a_callback_held_in_a_field_or_parameter_needs_no_adapter() {
    let source = lower_and_emit(
        "lower-cpp-callable-value-call",
        r#"
struct Holder {
    void (*m_callback)(int);
    void Fire(int value) {
        m_callback(value);
        this->m_callback(value);
    }
};
void call_param(void (*cb)(int), int value) {
    cb(value);
}
"#,
    );

    assert!(
        !source.contains("unsupported call target cursor kind"),
        "got:\n{source}"
    );
    assert!(
        source.matches("m_callback(value);").count() == 2,
        "expected both the implicit- and explicit-`this` calls to lower identically, got:\n{source}"
    );
    assert!(source.contains("cb(value);"), "got:\n{source}");
}

/// A user-defined conversion operator to `std::string` (`operator
/// std::string() const`) — the dominant shape behind Verovio's
/// `HumdrumToken`, whose implicit conversion to `std::string` was the
/// single largest bailout family in the 2026-08-20 diagnosis (~750 combined
/// occurrences of "unsupported implicit conversion from/to
/// Record{HumdrumToken}" and "call target cursor kind 26"). Confirmed via
/// `clang++ -Xclang -ast-dump` before writing the fix (not assumed): an
/// *implicit* conversion (`std::string s = t;`) already lowers to a real
/// `CXXMemberCallExpr` referencing the same `CXXConversionDecl` an
/// *explicit* call (`t.operator std::string()`) does — both reach
/// `lower_call_expr` the same way, so one fix (naming the operator
/// `toStr`, since its real C++ spelling has characters no Dart identifier
/// can) covers both call forms.
#[test]
fn a_conversion_operator_to_string_lowers_both_implicitly_and_explicitly_to_a_named_dart_method() {
    let source = lower_and_emit(
        "lower-cpp-conversion-operator-to-str",
        r#"
#include <string>
class Token {
public:
    Token(const std::string& s): m_s(s) {}
    operator std::string() const { return m_s; }
private:
    std::string m_s;
};
std::string implicit_use(Token t) {
    std::string s = t;
    return s;
}
std::string explicit_use(Token t) {
    return t.operator std::string();
}
"#,
    );

    assert!(
        !source.contains("unsupported")
            && !source.contains("cursor kind 26")
            && !source.contains("dynamic"),
        "got:\n{source}"
    );
    assert!(
        source.contains("String toStr() {"),
        "expected the conversion operator to be declared as a named Dart method, got:\n{source}"
    );
    assert!(
        source.matches("t.toStr()").count() == 2,
        "expected both the implicit and explicit call forms to lower identically, got:\n{source}"
    );
}

/// A user-defined conversion operator to `bool` (`operator bool() const`) —
/// the "unsupported conversion operator target: Bool" family (52
/// occurrences in the 2026-08-20 diagnosis), same mechanism as the `toStr`
/// test above (`conversion_operator_dart_method_name` is the shared source
/// of truth for both), just a different target type and synthesized name.
#[test]
fn a_conversion_operator_to_bool_lowers_both_implicitly_and_explicitly_to_a_named_dart_method() {
    let source = lower_and_emit(
        "lower-cpp-conversion-operator-to-bool",
        r#"
class OptionalFlag {
public:
    OptionalFlag(bool set): m_set(set) {}
    operator bool() const { return m_set; }
private:
    bool m_set;
};
bool implicit_use(OptionalFlag flag) {
    if (flag) {
        return true;
    }
    return false;
}
bool explicit_use(OptionalFlag flag) {
    return flag.operator bool();
}
"#,
    );

    assert!(
        !source.contains("unsupported") && !source.contains("dynamic"),
        "got:\n{source}"
    );
    assert!(
        source.contains("bool toBool() {"),
        "expected the conversion operator to be declared as a named Dart method, got:\n{source}"
    );
    assert!(
        source.matches("flag.toBool()").count() == 2,
        "expected both the implicit (`if (flag)`) and explicit call forms to lower identically, got:\n{source}"
    );
}

/// `sizeof(T)` for a well-known, fixed-width type — the real, common shape
/// found by grepping the Verovio source directly (`test-resources/verovio-
/// version-6.2.0.tar.gz`, unpacked locally to check the exact triggering
/// line instead of guessing): `crc.cpp`'s `8 * sizeof(crc)` (CRC bit width)
/// and `zip_file.hpp`'s `p + sizeof(mz_uint32)` (pointer-offset by a known
/// type's byte width). `sizeof`/`alignof`/other C++ type-trait unary
/// expressions all share one `libclang` cursor kind (136,
/// `CXCursor_UnaryExpr`) with no sub-kind exposed on the cursor API to
/// distinguish which — but `clang_Cursor_Evaluate` (already used by
/// `evaluate_int_eval_result` for integer/bool literals) already
/// constant-folds a `sizeof`/`alignof` whose *operand type* is complete and
/// has a known layout, exactly the "map only when the size is well-defined"
/// scope the backlog calls for — so evaluating first and only falling back
/// to a bailout when that fails covers `sizeof(T)` without needing to name
/// every possible type-trait shape up front.
#[test]
fn a_sizeof_expression_on_a_known_fixed_width_type_evaluates_to_a_constant() {
    let source = lower_and_emit(
        "lower-cpp-sizeof-known-type",
        r#"
typedef unsigned int crc;
unsigned int width() {
    return 8 * sizeof(crc);
}
"#,
    );

    assert!(
        source.contains("8 * 4"),
        "expected sizeof(unsigned int) to fold to the constant 4, got:\n{source}"
    );
    assert!(!source.contains("cursor kind 136"), "got:\n{source}");
}

/// `str.compare(pos, len, other)` — the real shape found by grepping the
/// Verovio source directly (`test-resources/verovio-version-6.2.0.tar.gz`):
/// `iohumdrum.cpp`'s `current->compare(0, 4, "*fs:")`, comparing a substring
/// starting at `pos` of length `len` against `other`. `lower_stdlib_method_
/// call`'s `("basic_string", "compare")` arm only accepted the 1-argument
/// overload (`compare(other)`); the 3-argument overload has a direct Dart
/// equivalent using the same `substring(start, start + count)` shape
/// `("basic_string", "substr")` right below it already establishes.
#[test]
fn a_three_argument_string_compare_lowers_to_a_substring_comparison() {
    let source = lower_and_emit(
        "lower-cpp-string-compare-3-arg",
        r#"
#include <string>
bool starts_with_fs(std::string s) {
    return s.compare(0, 4, "*fs:") == 0;
}
"#,
    );

    assert!(
        source.contains("s.substring(0, 0 + 4).compareTo('*fs:')"),
        "expected the 3-argument compare to become a substring comparison, got:\n{source}"
    );
    assert!(!source.contains("compare had"), "got:\n{source}");
}

/// `std::cout << "text" << value << std::endl;` — the classic chained
/// insertion idiom, confirmed as the real trigger by grepping the Verovio
/// source directly (`tools/main.cpp`'s `DisplayVersion`:
/// `std::cout << "Verovio " << vrv::GetVersion() << std::endl;`).
/// `std::cout` is a genuine 1:1 external boundary (real process stdout),
/// not a stand-in for an arbitrary `std::ostream` — narrowly bridged to
/// Dart's `print`, which needs no import and already appends the trailing
/// newline `std::endl` asks for. Scoped to chains that visibly end in
/// `std::endl` (the only case where `print`'s automatic newline is
/// semantically correct) and to `Str`/`Int`/`Double` operands (`Bool` is
/// excluded: C++'s default `operator<<(bool)` prints `0`/`1`, not Dart's
/// `"true"`/`"false"`, and this bridge can't know whether `std::boolalpha`
/// was set). `std::cerr` gets the same bridge (see the sibling test below,
/// to `dart:io`'s `stderr.writeln`); a chain without a trailing `std::endl`
/// stays bailout — that's the only case where the automatic newline
/// `print`/`stderr.writeln` both append is a semantic mismatch.
#[test]
fn a_cout_insertion_chain_ending_in_endl_lowers_to_print() {
    let source = lower_and_emit(
        "lower-cpp-cout-chain",
        r#"
#include <iostream>
void display_version(int major) {
    std::cout << "Verovio " << major << std::endl;
}
"#,
    );

    assert!(
        source.contains("print('Verovio ' + major.toString());"),
        "expected the chain to fold into one `print` call, got:\n{source}"
    );
    assert!(!source.contains("operator<<"), "got:\n{source}");
}

/// `std::cerr << "text" << std::endl;` — grepping the Verovio source
/// directly shows `std::cerr` is actually **more common** than `std::cout`
/// in this idiom (231 vs. 68 occurrences), almost always for the same
/// warning/error-message-then-newline shape — so it earns the same bridge,
/// to Dart's `stderr.writeln` (`dart:io`) rather than `print` (which only
/// ever reaches stdout). The import itself is added by a post-hoc scan of
/// the emitted source (`source.contains("stderr.")`), the same mechanism
/// already used for `Uint8List` → `dart:typed_data`, not a new threaded
/// flag through every `emit_expr`/`emit_stmt` call site.
#[test]
fn a_cerr_insertion_chain_ending_in_endl_lowers_to_stderr_writeln() {
    let source = lower_and_emit(
        "lower-cpp-cerr-chain",
        r#"
#include <iostream>
void warn(int code) {
    std::cerr << "Warning " << code << std::endl;
}
"#,
    );

    assert!(
        source.contains("stderr.writeln('Warning ' + code.toString());"),
        "expected the chain to fold into one `stderr.writeln` call, got:\n{source}"
    );
    assert!(
        source.contains("import 'dart:io';"),
        "expected the dart:io import to be added, got:\n{source}"
    );
    assert!(!source.contains("operator<<"), "got:\n{source}");
}

/// `std::find(X.begin(), X.end(), v) != X.end()` — "does `X` contain
/// `v`?", confirmed as a real, common shape by grepping the Verovio source
/// directly: `adjustbeamsfunctor.cpp:326`'s `std::find(dotLocs.cbegin(),
/// dotLocs.cend(), dotLoc) != dotLocs.cend()`. `std::find`'s own iterator
/// return value has no representation this bridge gives it on its own —
/// the *whole comparison* is recognized as one idiom instead, since every
/// `begin`/`end` mention has to agree on the exact same receiver for the
/// rewrite to be sound. Also covers the negated form (`==`, "is absent").
#[test]
fn a_find_against_end_comparison_lowers_to_a_dart_contains_call() {
    let source = lower_and_emit(
        "lower-cpp-find-contains",
        r#"
#include <set>
#include <algorithm>
bool has_dot(std::set<int>& dotLocs, int dotLoc) {
    if (std::find(dotLocs.cbegin(), dotLocs.cend(), dotLoc) != dotLocs.cend()) {
        return true;
    }
    return false;
}
bool missing_dot(std::set<int>& dotLocs, int dotLoc) {
    return std::find(dotLocs.begin(), dotLocs.end(), dotLoc) == dotLocs.end();
}
"#,
    );

    assert!(
        source.contains("if (dotLocs.contains(dotLoc)) {"),
        "expected the != form to become a plain contains call, got:\n{source}"
    );
    assert!(
        source.contains("return !(dotLocs.contains(dotLoc));"),
        "expected the == form to become a negated contains call, got:\n{source}"
    );
    assert!(
        !source.contains("operator!=") && !source.contains("operator=="),
        "got:\n{source}"
    );
}

/// `dynamic_cast<T*>(operand)` — a checked downcast, confirmed as a real,
/// common Verovio shape by grepping the source directly
/// (`options.cpp:184`'s `dynamic_cast<OptionBool *>(option)`,
/// `options.cpp:115`'s `dynamic_cast<const OptionDbl *>(this)`). Scoped to
/// a *simple* operand (`this` or a bare local/parameter): `operand is T`
/// evaluates `operand` twice by construction (once as the ternary
/// condition, once implicitly via Dart's own flow-sensitive promotion
/// inside the `then` branch), so anything with a side effect (a call, or a
/// field access reached through one) has to stay a bailout rather than
/// risk duplicating it — this bridge has no way to hoist a temporary from
/// pure-expression lowering yet (the same gap that already defers
/// `binary operator kind 22`).
#[test]
fn a_dynamic_cast_on_a_simple_operand_lowers_to_a_type_check_ternary() {
    let source = lower_and_emit(
        "lower-cpp-dynamic-cast",
        r#"
class Base {
public:
    virtual ~Base() {}
    Base* Self() { return this; }
};
class OptionBool : public Base {
public:
    bool m_value = false;
};
OptionBool* from_param(Base* option) {
    return dynamic_cast<OptionBool*>(option);
}
class Derived : public Base {
public:
    OptionBool* from_this() {
        return dynamic_cast<OptionBool*>(this);
    }
    OptionBool* from_call() {
        return dynamic_cast<OptionBool*>(Self());
    }
};
"#,
    );

    assert!(
        source.contains("return option is OptionBool ? option : null;"),
        "expected a simple parameter operand to lower to a type-check ternary, got:\n{source}"
    );
    assert!(
        source.contains("return this is OptionBool ? this : null;"),
        "expected `this` to lower the same way, got:\n{source}"
    );
    assert!(
        source.contains("dynamic_cast operand is not a simple reference"),
        "expected a call operand (side-effect risk from double evaluation) to stay a bailout, got:\n{source}"
    );
}

/// (b) — `docs/prompts/2026-08-21-06-bailout-tipado-e-opaque-compartilhado.md`:
/// the `dynamic_cast` bailout just above still has a known static type — the
/// cast's own nullable target record, `OptionBool?` — even though its
/// operand is unrepresentable. It must carry that type instead of the
/// generic opaque bridge, the same real corpus family (~126 occurrences of
/// "dynamic_cast operand is not a simple reference" in the Verovio 6.2.0
/// diagnosis) that produced `unchecked_use_of_nullable_value`/
/// `argument_type_not_assignable` whenever the bailout's declared context
/// (a variable, a field, a return type) expected the real record type.
#[test]
fn a_dynamic_cast_bailout_on_a_call_operand_still_carries_its_target_type() {
    let source = lower_and_emit(
        "lower-cpp-dynamic-cast-typed-bailout",
        r#"
class Base {
public:
    virtual ~Base() {}
    Base* Self() { return this; }
};
class OptionBool : public Base {
public:
    bool m_value = false;
};
class Derived : public Base {
public:
    OptionBool* from_call() {
        return dynamic_cast<OptionBool*>(Self());
    }
};
"#,
    );

    assert!(
        source.contains("_syntaxBridgeUnsupported<OptionBool?>("),
        "expected the bailout to carry the dynamic_cast's own nullable target \
         type instead of the generic opaque bridge, got:\n{source}"
    );
    assert!(
        !source.contains("_syntaxBridgeUnsupported<SyntaxBridgeOpaque>("),
        "a statically known bailout type must not fall back to the opaque bridge, got:\n{source}"
    );
}

/// `return new Abbr(*this);` — Verovio's own `Clone()` idiom
/// (`include/vrv/abbr.h`'s `Object *Clone() const override { return new
/// Abbr(*this); }`, confirmed as the real trigger by grepping the source
/// directly). `lower_call_expr` already treats a copy-constructor call as
/// transparent sugar (E03), recursing straight into `*this`/`other`
/// itself — so the construction never lowers to a `ConstructorCall`/
/// `RecordConstruct` the way `lower_new_expr` originally expected, even
/// though it's a completely representable allocation. Rebuilt as a
/// field-by-field `RecordConstruct` off the copy source instead — the same
/// construction `collect_params_with_clone_prelude` already builds for a
/// by-value parameter's own copy-on-entry clone, just keyed to an
/// arbitrary receiver expression.
#[test]
fn a_new_expression_copy_constructing_from_this_clones_every_field() {
    let source = lower_and_emit(
        "lower-cpp-new-copy-construct",
        r#"
class Abbr {
public:
    Abbr() {}
    Abbr(const Abbr& other) {}
    int m_x = 0;
    Abbr* Clone() const { return new Abbr(*this); }
};
"#,
    );

    assert!(
        source.contains("return Abbr(this.m_x);"),
        "expected a field-by-field clone from `this`, got:\n{source}"
    );
    assert!(!source.contains("CXX new child"), "got:\n{source}");
}

/// `U"x"` (a `char32_t[N]` string literal) — Verovio's own `Dynam::
/// IsSymbolOnly` (`return U"x";`/`m_symbolStr = U"";`, confirmed real via
/// grepping the source). `std::u32string` already lowers to `Type::Str`
/// (`stdlib_template_name` keys on the primary template name
/// `"basic_string"` regardless of its character-type argument, already
/// correct), but `string_literal_text` read the token spelling by
/// stripping a *bare* `"` prefix/suffix — for `U"x"` the token spelling is
/// literally `U"x"`, so `strip_prefix('"')` failed on the leading `U` and
/// the whole literal came back `None`. The resulting `Expr::Unsupported`
/// then got silently discarded by the implicit-conversion wrapper's own
/// type-mismatch fallback (the array-to-pointer-decayed `char32_t[N]`
/// lowers to `List(Int)`, disagreeing with the `Str` context), surfacing
/// as "unsupported implicit conversion from List(Int) to Nullable(Str)"
/// instead of the real, more specific failure. Also covers `u"..."`
/// (UTF-16), `L"..."` (wide) and `u8"..."` (explicit UTF-8) — every C++
/// string-literal encoding prefix, not just `U`.
#[test]
fn a_prefixed_string_literal_of_every_encoding_evaluates_its_text() {
    let source = lower_and_emit(
        "lower-cpp-prefixed-string-literal",
        r#"
#include <string>
class Dynam {
public:
    std::u32string GetText() const { return U"x"; }
    bool IsSymbolOnly() const {
        m_symbolStr = U"";
        std::u32string str = this->GetText();
        m_symbolStr = str;
        return true;
    }
    mutable std::u32string m_symbolStr;
};
std::u16string utf16() { return u"y"; }
std::wstring wide() { return L"z"; }
std::string explicit_utf8() { return u8"w"; }
"#,
    );

    assert!(
        source.contains("String GetText() {\n    return 'x';\n  }"),
        "expected the U-prefixed literal to evaluate to 'x', got:\n{source}"
    );
    assert!(
        source.contains("m_symbolStr = '';"),
        "expected the empty U-prefixed literal to evaluate too, got:\n{source}"
    );
    assert!(
        source.contains("return 'y';"),
        "expected the u-prefixed (UTF-16) literal to evaluate, got:\n{source}"
    );
    assert!(
        source.contains("return 'z';"),
        "expected the L-prefixed (wide) literal to evaluate, got:\n{source}"
    );
    assert!(
        source.contains("return 'w';"),
        "expected the u8-prefixed literal to evaluate, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported implicit conversion")
            && !source.contains("could not evaluate string literal"),
        "got:\n{source}"
    );
}

/// An anonymous top-level `enum { NAME = value, ... }` — a common C idiom
/// for a group of named integer constants, not a real type. Confirmed as
/// the real Verovio shape by grepping the source directly
/// (`include/vrv/smufl.h`'s `enum { SMUFL_0020_space = 0x0020,
/// SMUFL_266D_musicFlatSign = 0x266D, ... }`). `lower_enum`/`enum_identity`
/// already refuse to declare a Dart type for an anonymous enum (correctly
/// — there's no usable name), so a reference to one of its enumerators has
/// no stable binding to name either; inlining the enumerator's own known
/// compile-time value instead is exact, not a guess.
#[test]
fn an_anonymous_enum_constant_inlines_to_its_literal_value() {
    let source = lower_and_emit(
        "lower-cpp-anonymous-enum-constant",
        r#"
enum {
    SMUFL_0020_space = 0x0020,
    SMUFL_266D_musicFlatSign = 0x266D,
};
int glyph_code() {
    return SMUFL_266D_musicFlatSign;
}
"#,
    );

    assert!(
        source.contains("return 9837;"),
        "expected the anonymous enum constant to inline to its real value (0x266D = 9837), got:\n{source}"
    );
    assert!(
        !source.contains("unsupported implicit conversion"),
        "got:\n{source}"
    );
}

/// `*(field = new T()) = value;` — a real crash found in the Verovio
/// corpus itself (`include/json/jsonxx.h:275`'s bundled JSON library,
/// `*( array_value_ = new Array() ) = a;`), not just a bailout: the
/// dereference's operand is `field = new T()`, an assignment used as an
/// expression. At the time this test was written, that operand was itself
/// an unconditional `Expr::Unsupported` (`binary operator kind 22` had no
/// lowering yet) that the *dereference* wrapped in `Expr::Convert`
/// unconditionally, without checking whether the wrapped operand was
/// representable — `emit::dart`'s `Expr::Convert` renderer had no case for
/// an operand with no statically-known type, so it hit its own
/// `unreachable!()`, a real emitter panic confirmed via a fresh `just
/// verovio-diagnosis` run against the real corpus. Assignment-as-expression
/// is representable now (`Expr::Assign`, confirmed against real `dart
/// analyze`/`dart run` — Dart's own `=` is a real expression too), so the
/// *inner* assignment succeeds — but the outer dereference's `Convert` now
/// wraps an `Expr::Assign`, which is a valid Dart *value* but never a valid
/// Dart assignment *target* (`(x = y) = z;` doesn't compile); this stays an
/// honest bailout for that reason instead, never reaching emission.
#[test]
fn a_dereference_of_an_unassignable_operand_stays_an_honest_bailout() {
    let source = lower_and_emit(
        "lower-cpp-deref-of-unsupported-operand",
        r#"
struct Array {};
struct Holder {
    Array* array_value_;
    void import(const Array& a) {
        *(array_value_ = new Array()) = a;
    }
};
"#,
    );

    assert!(
        !source.contains("panicked")
            && source
                .contains("assignment target is not representable as a Dart assignment target"),
        "expected an honest bailout, not a panic, got:\n{source}"
    );
}

/// Unary `+` (`CXUnaryOperator_Plus`) — confirmed as the real Verovio
/// trigger by grepping the source directly (`iohumdrum.cpp:915`'s
/// `m_fbstates[staffindex] = +1;`, an explicit-positive-sign idiom). Unlike
/// every other unary operator, `+x` is a true no-op for an arithmetic
/// value in both C++ and Dart (no promotion Dart's own arbitrary-precision
/// `int`/`double` needs modeling for) — the operand lowers directly,
/// exactly as transparent as a parenthesized wrapper.
#[test]
fn unary_plus_lowers_to_its_bare_operand() {
    let source = lower_and_emit(
        "lower-cpp-unary-plus",
        r#"
int positive_one() {
    return +1;
}
"#,
    );

    assert!(
        source.contains("return 1;"),
        "expected unary plus to lower to its bare operand, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported unary operator"),
        "got:\n{source}"
    );
}

/// Assignment used as an *expression*, not a whole statement — confirmed
/// as a real Verovio trigger by grepping the source directly
/// (`adjustarticfunctor.cpp:47`'s `yIn = std::max(yAboveStem,
/// -staffHeight);`, a *plain-looking statement* that nonetheless reaches
/// `lower_binary_expr` — not `lower_stmt`'s own statement-level assignment
/// recognition — because `std::max`'s template instantiation wraps the
/// whole statement in an intervening `libclang` cursor first). Dart's own
/// `=` is a real expression too, evaluating to the assigned value — proven
/// against real `dart analyze`/`dart run` before implementing, not
/// assumed — so this needs no hoisted temporary statement, contrary to
/// this bailout's original (incorrect) deferral reasoning.
#[test]
fn assignment_used_as_an_expression_lowers_to_darts_own_assignment_expression() {
    let source = lower_and_emit(
        "lower-cpp-assignment-as-expression",
        r#"
#include <algorithm>
int via_wrapped_statement(int yAboveStem, int staffHeight) {
    int yIn;
    yIn = std::max(yAboveStem, -staffHeight);
    return yIn;
}
"#,
    );

    assert!(
        source.contains("(yIn = math.max(yAboveStem, -staffHeight));"),
        "expected the wrapped-statement assignment to lower to a parenthesized assignment expression, got:\n{source}"
    );
    assert!(
        !source.contains("unsupported binary operator kind 22"),
        "got:\n{source}"
    );
}

/// Real trigger: `jsonxx.h`'s `import(const String&)`,
/// `*( string_value_ = new String() ) = s;` — the assignment-as-expression
/// used as a dereference target wraps an inner `new` allocation that itself
/// bails out (`new String()`'s pointee isn't a known project record). The
/// inner `Expr::Assign` must propagate that bailout instead of silently
/// building a well-formed-looking assignment expression around a broken
/// value, which previously reached `dart format` as
/// `(value_ = _syntaxBridgeUnsupported<int?>(...))! = v;` — invalid Dart
/// ("Illegal assignment to non-assignable expression").
#[test]
fn an_assignment_expression_with_an_unsupported_right_hand_side_stays_an_honest_bailout() {
    let source = lower_and_emit(
        "lower-cpp-assignment-expression-unsupported-rhs",
        r#"
struct Holder {
    int* value_;
    void import(int v) {
        *( value_ = new int() ) = v;
    }
};
"#,
    );

    assert!(
        !source.contains("panicked"),
        "expected an honest bailout, not a panic, got:\n{source}"
    );
    assert!(
        !source.contains("= _syntaxBridgeUnsupported"),
        "an unsupported assignment-expression rhs must not reach emission as a live \
         assignment target, got:\n{source}"
    );
}

/// Real trigger: `svgdevicecontext.cpp`'s `GetColor`. Dart's switch-case
/// patterns only accept a literal or a named-constant reference, and
/// reject an inline operator expression outright (`dart analyze`: "The
/// binary operator << is not supported as a constant pattern"), even
/// though C++ accepts any integer-constant-expression as a case label.
/// Each label here must fold to its compile-time integer value instead of
/// being emitted as the (Dart-illegal) source expression.
#[test]
fn a_case_label_that_is_a_constant_expression_folds_to_its_integer_value() {
    let source = lower_and_emit(
        "lower-cpp-case-label-constant-expression",
        r##"
const char* get_color(int color) {
    switch (color) {
        case 255 << 16 | 255 << 8 | 255:
            return "#FFFFFF";
        case 255 << 16:
            return "#FF0000";
        default:
            return "#000000";
    }
}
"##,
    );
    assert!(source.contains("case 16777215:"), "got:\n{source}");
    assert!(source.contains("case 16711680:"), "got:\n{source}");
    assert!(
        !source.contains("<<") && !source.contains("|"),
        "case label should be folded to a literal, not emitted as an operator expression, \
         got:\n{source}"
    );
}

/// `emit::dart` groups declarations by file and emits them all at top
/// level, so two classes in one header each declaring `enum Type` (Verovio
/// does exactly this) both arrived as a bare top-level `enum Type` in the
/// same `.dart` file — a duplicate definition that doesn't compile.
#[test]
fn two_nested_enums_with_the_same_name_in_one_file_get_distinct_dart_names() {
    let source = lower_and_emit(
        "lower-cpp-nested-enum-collision",
        r#"
class A {
public:
    enum Type { Um, Dois };
    Type f() { return Um; }
};

class B {
public:
    enum Type { Tres, Quatro };
    Type g() { return Tres; }
};
"#,
    );

    assert!(
        !source.contains("enum Type {"),
        "a nested enum must not be emitted under its bare name, got:\n{source}"
    );
    assert!(
        source.contains("enum AType {") && source.contains("enum BType {"),
        "expected both nested enums under owner-qualified names, got:\n{source}"
    );
    assert!(
        source.contains("AType.Um") && source.contains("BType.Tres"),
        "references must use the same qualified name the declaration got, got:\n{source}"
    );
}

/// Diagnostic finding (`verovio_6_2_0_transpile_diagnosis`, achado 8,
/// Verovio's own `beam.h`): an anonymous top-level `enum { ... };` was
/// declared under libclang's own debug spelling for the anonymous cursor
/// (`"(unnamed enum at <file>:<line>:<col>)"`), which is not a valid Dart
/// identifier — `dart format` rejected the whole file at that line.
#[test]
fn an_anonymous_enum_is_never_declared_under_libclangs_debug_spelling() {
    let source = lower_and_emit(
        "lower-cpp-anonymous-enum",
        r#"
enum { PARTIAL_NONE, PARTIAL_THROUGH, PARTIAL_RIGHT, PARTIAL_LEFT };

void f() {}
"#,
    );

    assert!(
        !source.to_lowercase().contains("unnamed enum")
            && !source.to_lowercase().contains("anonymous enum"),
        "an anonymous enum must never leak libclang's debug spelling into a \
         Dart identifier, got:\n{source}"
    );
    assert!(
        !source.contains("enum ("),
        "an anonymous enum has no valid Dart name and must not be declared \
         at all, got:\n{source}"
    );
}

/// Diagnostic finding (`verovio_6_2_0_transpile_diagnosis`, achado 9,
/// Verovio's own `FloatingObject::IsCloserToStaffThan`): a C++ parameter
/// with no name (legal in a declaration, common in an interface signature
/// that never uses it) was emitted with no Dart identifier at all —
/// `dart format` rejected the file at the empty parameter slot.
#[test]
fn an_unnamed_parameter_gets_a_synthesized_positional_dart_name() {
    let source = lower_and_emit(
        "lower-cpp-unnamed-parameter",
        r#"
class C {
public:
    bool F(int, bool named) { return named; }
};
"#,
    );

    assert!(
        !source.contains("int , bool named"),
        "an unnamed parameter must not be emitted with no Dart identifier \
         at all, got:\n{source}"
    );
    assert!(
        source.contains("int arg0, bool named"),
        "an unnamed parameter should get a synthesized positional Dart \
         name, got:\n{source}"
    );
}

/// `index` and `values` are members every Dart enum already has, and `in`
/// is a Dart reserved word — all three are perfectly ordinary C++
/// enumerators, and emitting them verbatim produces an enum body Dart
/// rejects.
#[test]
fn enumerators_colliding_with_dart_reserved_names_are_renamed_consistently() {
    let source = lower_and_emit(
        "lower-cpp-reserved-enumerators",
        r#"
enum Chave { index, in, normal };

Chave escolher() { return index; }
"#,
    );

    assert!(
        source.contains("index_") && source.contains("in_"),
        "expected the colliding enumerators to be renamed, got:\n{source}"
    );
    assert!(
        source.contains("normal"),
        "a non-colliding enumerator should keep its name, got:\n{source}"
    );
    assert!(
        source.contains("Chave.index_"),
        "the reference must use the same renamed constant, got:\n{source}"
    );
}

/// Diagnostic finding (`verovio_6_2_0_transpile_diagnosis`, Verovio's own
/// `accid.cpp`): a functor's `operator()` was emitted with its literal C++
/// name — `bool operator()(...)` — which `dart format` rejects outright
/// (`Expected an identifier`, confirmed empirically). Dart's own idiom for a
/// callable object is a plain method named `call`; `obj(args)` already
/// dispatches to it automatically, so this bridge preserves call-site syntax
/// too, not just the declaration.
#[test]
fn operator_call_declared_on_a_record_bridges_to_darts_call_method() {
    let source = lower_and_emit(
        "lower-cpp-operator-call",
        r#"
class Comparador {
public:
    bool operator()(int a, int b) {
        return a < b;
    }
};
"#,
    );

    assert!(
        !source.contains("operator()"),
        "the raw C++ spelling must never reach Dart, got:\n{source}"
    );
    assert!(
        source.contains("bool call(int a, int b) {"),
        "expected the functor to bridge to Dart's own `call` method, got:\n{source}"
    );
}

/// `operator<` (and the rest of Dart's own overloadable comparison/
/// arithmetic set) has a direct, same-arity Dart equivalent — unlike
/// `operator==`, which needs a coerced `Object` parameter, this needs no
/// special body handling at all, just the `operator <name>` declaration
/// syntax instead of `<name>` printed as a bare (invalid) identifier.
#[test]
fn an_operator_in_darts_overloadable_set_declares_as_a_real_dart_operator() {
    let source = lower_and_emit(
        "lower-cpp-operator-lt",
        r#"
class Ponto {
public:
    int x;
    bool operator<(int limite) const {
        return x < limite;
    }
};
"#,
    );

    assert!(
        !source.contains("bool operator<(int limite)"),
        "the raw C++ spelling must never be printed as a bare identifier, got:\n{source}"
    );
    assert!(
        source.contains("bool operator <(int limite) {"),
        "expected Dart's own operator-declaration syntax, got:\n{source}"
    );
}

/// `operator++` has no Dart equivalent at all (Dart never lets a type
/// customize `++`/`--`). Emitting it under its literal C++ name is invalid
/// Dart syntax the same way `operator()` was — the fix bridges it to a
/// synthesized, always-valid method name while preserving its ordinary
/// method body.
#[test]
fn an_operator_with_no_dart_equivalent_bridges_to_a_named_method_instead_of_breaking_syntax() {
    let source = lower_and_emit(
        "lower-cpp-operator-increment",
        r#"
class Contador {
public:
    int valor;
    void operator++() {
        valor = valor + 1;
    }
};
"#,
    );

    assert!(
        !source.contains("operator++("),
        "the raw C++ spelling must never be printed as a Dart declaration, got:\n{source}"
    );
    assert!(
        source.contains("void increment() {"),
        "expected a synthesized, always-valid bridge name, got:\n{source}"
    );
    assert!(
        source.contains("valor = valor + 1;")
            && !source.contains("TODO(syntax-bridge)")
            && !source.contains("UnimplementedError"),
        "the named bridge must preserve the operator body, got:\n{source}"
    );
}

/// Dart has no free-standing operators. A free C++ `operator<<` therefore
/// becomes a consistently named helper both at its declaration and call
/// sites, retaining its ordinary function body instead of becoming opaque.
#[test]
fn a_free_operator_overload_with_no_dart_equivalent_becomes_unsupported_instead_of_an_invalid_call()
{
    let source = lower_and_emit(
        "lower-cpp-free-operator-shl",
        r#"
struct Foo {
    int x;
};

Foo operator<<(Foo a, int deslocamento) {
    Foo resultado;
    resultado.x = a.x << deslocamento;
    return resultado;
}

Foo usa(Foo a) {
    return a << 2;
}
"#,
    );

    assert!(
        !source.contains("operator<<("),
        "the raw C++ operator spelling must never be printed as a call target, got:\n{source}"
    );
    assert!(
        source.contains("Foo streamInsert(Foo a, int deslocamento)")
            && source.contains("return streamInsert(a, 2);")
            && !source.contains("unsupported free operator overload: operator<<"),
        "a free operator must use its named Dart bridge at declaration and call sites, got:\n{source}"
    );
}

/// Verovio's `-VRV_UNSET` (a macro expanding to `(-2147483647)`, so the
/// surface expression is a double unary minus) was emitted as `--2147483647`
/// — `dart format` reads `--` as the prefix-decrement token and rejects
/// decrementing a literal (`Missing selector such as '.identifier'`,
/// confirmed empirically). Two adjacent `-` characters with no separator
/// between them always merge into that token, regardless of how deeply
/// nested the two `Expr::Unary` nodes are — parenthesizing the inner one is
/// the general fix, not a special case for this one macro.
#[test]
fn nested_unary_minus_is_parenthesized_so_it_never_merges_into_a_decrement_token() {
    let source = lower_and_emit(
        "lower-cpp-nested-unary-minus",
        r#"
int f() {
    return -(-2147483647);
}
"#,
    );

    assert!(
        !source.contains("--2147483647"),
        "the two unary minuses must never merge into a decrement token, got:\n{source}"
    );
    assert!(
        source.contains("-(-2147483647)"),
        "expected the inner negation parenthesized, got:\n{source}"
    );
}

/// Diagnostic finding (`verovio_6_2_0_transpile_diagnosis`, item 9,
/// `jsonxx.dart`/`humlib.dart`): a C++ method or free function named after a
/// Dart reserved word (`is`, `finally`, ...) — legal in C++, none of these
/// are C++ keywords — was emitted with its literal C++ name, which
/// `dart format` rejects at the declaration (`'is' can't be used as an
/// identifier because it's a keyword`). The declaration and every call site
/// have to be renamed together (`function_catalog::apply_reserved_word_renames`,
/// the same usr-keyed `renames` map `apply_overload_renames` already
/// established for US-7's overload renaming), or the call site would keep
/// invoking a name the declaration no longer has.
#[test]
fn a_method_named_after_a_dart_reserved_word_is_renamed_at_declaration_and_call_site() {
    let source = lower_and_emit(
        "lower-cpp-reserved-method-name",
        r#"
class Consulta {
public:
    bool is() {
        return true;
    }

    bool checar() {
        return is();
    }
};
"#,
    );

    assert!(
        !source.contains("bool is()"),
        "the raw C++ spelling must never be declared as a Dart method, got:\n{source}"
    );
    assert!(
        source.contains("bool is_()"),
        "expected the method renamed with a trailing underscore, got:\n{source}"
    );
    assert!(
        source.contains("is_()") && !source.contains("return is();"),
        "expected the call site renamed to match the declaration, got:\n{source}"
    );
}

/// Same finding as the method case above, for a *free* function
/// (`vrv.dart`'s own call shape, though the collision there was a
/// parameter, not the function name itself — this is the function-name
/// sibling case, exercised directly since it goes through a different IR
/// path, `ir_functions` rather than a record's `methods`).
#[test]
fn a_free_function_named_after_a_dart_reserved_word_is_renamed_at_declaration_and_call_site() {
    let source = lower_and_emit(
        "lower-cpp-reserved-function-name",
        r#"
bool is() {
    return true;
}

bool checar() {
    return is();
}
"#,
    );

    assert!(
        !source.contains("bool is()"),
        "the raw C++ spelling must never be declared as a Dart function, got:\n{source}"
    );
    assert!(
        source.contains("bool is_()"),
        "expected the function renamed with a trailing underscore, got:\n{source}"
    );
    assert!(
        source.contains("is_()") && !source.contains("return is();"),
        "expected the call site renamed to match the declaration, got:\n{source}"
    );
}

/// Diagnostic finding (`verovio_6_2_0_transpile_diagnosis`, item 9,
/// `tuningsimpl.dart`/`vrv.dart`/`pugixml.dart`): a C++ parameter named
/// after a Dart reserved word (`is`, `in`, `var`, ...) — legal in C++, none
/// of these are C++ keywords — was emitted with its literal name, which
/// `dart format` rejects (`'in' can't be used as an identifier because it's
/// a keyword`). Unlike a method/function name, a parameter is lexically
/// scoped, not usr-keyed, so the fix has to live in `lower::cpp` itself
/// (`dart_safe_identifier`, applied at both the parameter declaration and
/// every reference inside the body, so the two can never disagree).
#[test]
fn a_parameter_named_after_a_dart_reserved_word_gets_a_safe_dart_name() {
    let source = lower_and_emit(
        "lower-cpp-reserved-parameter-name",
        r#"
int f(int in) {
    return in + 1;
}
"#,
    );

    assert!(
        !source.contains("(int in)"),
        "the raw C++ spelling must never be declared as a Dart parameter, got:\n{source}"
    );
    assert!(
        source.contains("int in_"),
        "expected the parameter renamed with a trailing underscore, got:\n{source}"
    );
    assert!(
        source.contains("in_ + 1"),
        "expected the reference inside the body renamed to match, got:\n{source}"
    );
}

/// Same finding as the parameter case above, for a *local variable*
/// (`jsonxx.dart`'s own repro: `basic_istringstream is = ...;`) — a
/// `DeclStmt`'s `VarDecl`, a different lowering path from a parameter's
/// `ParmDecl`, exercised separately since nothing guarantees the two share
/// code.
#[test]
fn a_local_variable_named_after_a_dart_reserved_word_gets_a_safe_dart_name() {
    let source = lower_and_emit(
        "lower-cpp-reserved-local-variable-name",
        r#"
int f() {
    int is = 1;
    return is + 1;
}
"#,
    );

    assert!(
        !source.contains("int is =") && !source.contains("int is;"),
        "the raw C++ spelling must never be declared as a Dart local variable, got:\n{source}"
    );
    assert!(
        source.contains("int is_ ="),
        "expected the local variable renamed with a trailing underscore, got:\n{source}"
    );
    assert!(
        source.contains("is_ + 1"),
        "expected the reference inside the body renamed to match, got:\n{source}"
    );
}

/// `dynamic` is accepted by Dart in a few identifier positions, but it must
/// never survive as a generated identifier: the package-level guarantee is
/// that searching generated Dart for `dynamic` finds no type escape hatch.
/// C++ permits this perfectly ordinary local name, so normalize it together
/// with the strictly reserved words.
#[test]
fn a_local_variable_named_dynamic_is_renamed() {
    let source = lower_and_emit(
        "lower-cpp-dynamic-local-name",
        r#"
int f() {
    int dynamic = 1;
    return dynamic + 1;
}
"#,
    );

    assert!(
        !source.contains("int dynamic =") && !source.contains("return dynamic +"),
        "the generated Dart must not retain dynamic as an identifier, got:\n{source}"
    );
    assert!(
        source.contains("int dynamic_ = 1") && source.contains("dynamic_ + 1"),
        "expected declaration and reference to be renamed consistently, got:\n{source}"
    );
}

/// Diagnostic finding (`verovio_6_2_0_transpile_diagnosis`, item 9,
/// `zip_file.dart`): an anonymous `struct { ... } campo;` (unlike an
/// anonymous `enum`, achado 8, no fixture in E01–E13 has one) hits the same
/// libclang quirk achado 8 already documented for enums —
/// `clang_getCursorSpelling` on an anonymous struct/class returns the
/// descriptive debug text `"(unnamed struct at <file>:<line>:<col>)"`, not
/// an empty string — and that text leaked straight into both a Dart `class`
/// declaration (a parse error) and a field's type reference. Neither has a
/// usable Dart name, so both must come back as an honest `Unsupported`
/// stub instead — the record is simply never declared (mirroring
/// `enum_identity`'s "anonymous — no usable Dart type name" early-out), and
/// any field of that anonymous type becomes `Type::Unsupported`, `emit::dart`'s
/// already-generic bailout for an unrepresentable field type.
#[test]
fn an_anonymous_struct_is_never_declared_under_libclangs_debug_spelling() {
    let source = lower_and_emit(
        "lower-cpp-anonymous-struct",
        r#"
struct Contêiner {
    struct {
        int ano;
        int mes;
    } data;
};
"#,
    );

    assert!(
        !source.contains("class ("),
        "an anonymous struct has no valid Dart name and must not be declared \
         at all, got:\n{source}"
    );
    assert!(
        !source.contains(") data;"),
        "a field of an anonymous struct type has no valid Dart type — it \
         must never be printed as a raw (invalid) type reference, got:\n{source}"
    );
    // libclang's debug spelling is still fine to *quote* inside the honest
    // `Unsupported` bailout's comment/exception message — that's a
    // diagnostic string, not a Dart identifier, and is represented by the
    // named opaque bridge rather than a dynamic type escape hatch.
    assert!(
        source.contains("SyntaxBridgeOpaque /* unsupported:")
            && source.contains("(unnamed struct at"),
        "expected the anonymous-struct field to become an honest Unsupported \
         bailout, got:\n{source}"
    );
}

/// Diagnostic finding (`verovio_6_2_0_transpile_diagnosis`, item 9,
/// `iocmme.dart`/`pugixml.dart`, real repro `Fraction::ReduceStatic` called
/// with a nullable-pointer field as an out-param argument): the out-param
/// bridge (E10/`achado 5`, `docs/plans/diagnostico-verovio-6.2.0.md`) emits
/// a Dart destructuring assignment, `(targets...) = call;` — but when a
/// target is a field reached through a nullable receiver, the field access
/// needs `receiver!.field` (achado 5's own null-safety fix), and Dart's
/// pattern-assignment grammar doesn't accept a postfix `!` inside a pattern
/// element (`dart format`: "Expected to find ')'" right after the `!`,
/// confirmed empirically against the real Verovio file). Ordinary
/// (non-pattern) assignment has no such restriction — `receiver!.field =
/// value;` is perfectly legal Dart — so the fix routes around the pattern
/// grammar entirely: a scoped block holds the call's result in a temporary,
/// then each target is assigned individually with ordinary assignment
/// syntax.
#[test]
fn a_tuple_assign_target_reached_through_a_nullable_receiver_avoids_pattern_assignment_syntax() {
    let source = lower_and_emit(
        "lower-cpp-tuple-assign-nullable-target",
        r#"
class Fraction {
public:
    static void ReduceStatic(int &num, int &den) {
        num = num / 2;
        den = den / 2;
    }
};

class Info {
public:
    int proportNum;
    int proportDen;
};

class Holder {
public:
    Info *info;

    void Normalize() {
        Fraction::ReduceStatic(info->proportNum, info->proportDen);
    }
};
"#,
    );

    assert!(
        !source.contains("(info!.proportNum, info!.proportDen) ="),
        "a nullable-receiver field access must never appear inside a Dart \
         pattern-assignment target, got:\n{source}"
    );
    assert!(
        source.contains("info!.proportNum =") && source.contains("info!.proportDen ="),
        "expected each target assigned individually, with ordinary (non-pattern) \
         null-assertion syntax, got:\n{source}"
    );
}

/// Round 19: the out-param bridge (E10/achado 5) only recognized C++'s
/// *reference* out-param idiom (`void f(int &out)`); the older, equally
/// common *pointer* spelling (`void f(int *out)`, real trigger:
/// `editortoolkit_neume.h`'s `ParseDragAction(..., int *x, int *y)`,
/// `win_getopt.h`'s `int *idx`) fell through to `Type::Unsupported`
/// entirely, since `lower_type`'s pointer branch has no `Known` shape for
/// a bare scalar pointee. `int *`/`size_t *`/etc. were consistently the
/// single largest residual family in `unsupported_types` after the void*
/// bridge (round 17) — grepping the real Verovio source confirmed every
/// sampled real occurrence is a single-value write-back, never an indexed
/// buffer, the same idiom the reference form already covers.
#[test]
fn a_pointer_out_param_bridges_to_a_dart_tuple_return() {
    let source = lower_and_emit(
        "lower-cpp-pointer-out-param",
        r#"
void GetPoint(int *x, int *y) {
    *x = 10;
    *y = 20;
}

int sum() {
    int a = 0;
    int b = 0;
    GetPoint(&a, &b);
    return a + b;
}
"#,
    );

    assert!(
        source.contains("(int, int) GetPoint(int x, int y)"),
        "expected the pointer out-params to become a Dart tuple return, got:\n{source}"
    );
    assert!(
        source.contains("(a, b) = GetPoint(a, b);"),
        "expected the call site to pass the caller's own variables as input and destructure \
         the tuple back into them, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "pointer out-param bridge must not bail out, got:\n{source}"
    );
}

/// A pointer out-arg that isn't a plain `&lvalue` (`nullptr`, opting out of
/// that particular output — a real, valid C++ call a caller can make even
/// though this bridge has no Dart target to write it into) must become an
/// honest statement-level bailout, not a silently-wrong `ExprStmt` that
/// evaluates the call and discards the very tuple the out-params were
/// bridged into.
#[test]
fn a_pointer_out_param_call_with_a_null_argument_bails_out_honestly() {
    let source = lower_and_emit(
        "lower-cpp-pointer-out-param-null-arg",
        r#"
#include <cstddef>

void GetPoint(int *x, int *y) {
    if (x) *x = 10;
    if (y) *y = 20;
}

void only_x(int *a) {
    GetPoint(a, nullptr);
}
"#,
    );

    assert!(
        source.contains("Unsupported"),
        "expected an honest bailout for the nullptr out-arg, got:\n{source}"
    );
    assert!(
        !source.contains("GetPoint(a, nullptr)") && !source.contains("GetPoint(a, null)"),
        "the call must not survive as a bare (value-discarding) expression statement, \
         got:\n{source}"
    );
}

/// Round 20: a `bool`-returning out-param function (the *dominant* real
/// shape — real trigger `editortoolkit_neume.h`'s `bool
/// ParseDragAction(jsonxx::Object param, std::string *elementId, int *x,
/// int *y)`, whose real body is `if (!param.has<...>("x")) return false;
/// (*x) = param.get<...>("x"); ...`, an early `return false;` *before* any
/// out-param write, and a parenthesized deref-assign — both reproduced
/// here faithfully, not simplified away), called inside an `if` condition
/// — the real corpus's own call site (`editortoolkit_neume.cpp:92`).
#[test]
fn a_bool_returning_out_param_call_used_in_an_if_condition_bridges_correctly() {
    let source = lower_and_emit(
        "lower-cpp-bool-out-param-if-condition",
        r#"
bool GetPoint(bool available, int *x, int *y) {
    if (!available) return false;
    (*x) = 10;
    (*y) = 20;
    return true;
}

int use_it(bool available) {
    int a = 0;
    int b = 0;
    if (GetPoint(available, &a, &b)) {
        return a + b;
    }
    return -1;
}
"#,
    );

    assert!(
        source.contains("(bool, int, int) GetPoint(bool available, int x, int y)"),
        "expected the bool return plus pointer out-params to become a Dart tuple, got:\n{source}"
    );
    assert!(
        source.contains("return (false, x, y);"),
        "expected the early return's implicit out-param values to fill the tuple's other \
         slots, got:\n{source}"
    );
    assert!(
        source.contains("return (true, x, y);"),
        "expected the original bool return value to be part of the tuple, got:\n{source}"
    );
    assert!(
        source.contains("_syntaxBridgeIfCallTemp = GetPoint(available, a, b);")
            && source.contains("a = _syntaxBridgeIfCallTemp.$2;")
            && source.contains("b = _syntaxBridgeIfCallTemp.$3;")
            && source.contains("if (_syntaxBridgeIfCallTemp.$1) {"),
        "expected the if-condition call to destructure into a temp before testing the bool \
         slot, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "bool-returning out-param call in an if condition must not bail out, got:\n{source}"
    );
}

/// The negated form, `if (!chamada(...))` — equally common in the real
/// corpus's own guard-clause style (`if (!param.has<...>("x")) return
/// false;`, the exact shape `ParseDragAction`'s own body uses, just with
/// the out-param call itself as the negated condition instead).
#[test]
fn a_negated_bool_returning_out_param_call_in_an_if_condition_bridges_correctly() {
    let source = lower_and_emit(
        "lower-cpp-negated-bool-out-param-if-condition",
        r#"
bool GetPoint(int *x, int *y) {
    (*x) = 10;
    (*y) = 20;
    return true;
}

int use_it() {
    int a = 0;
    int b = 0;
    if (!GetPoint(&a, &b)) {
        return -1;
    }
    return a + b;
}
"#,
    );

    assert!(
        source.contains("if (!(_syntaxBridgeIfCallTemp.$1)) {"),
        "expected the negated condition to test the negation of the tuple's bool slot, \
         got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "negated bool-returning out-param call in an if condition must not bail out, \
         got:\n{source}"
    );
}

/// F8/tarefa 10 (`docs/prompts/2026-08-21-10-parametros-de-saida-por-referencia.md`):
/// every out-param call site recognized so far (`call_out_param_arg_indices`)
/// was a free function or a `static` method, whose raw call-cursor arguments
/// line up 1:1 with the callee's own declared parameters. An *ordinary*
/// (non-static, non-operator) instance method called through plain
/// `obj.method(...)` syntax has that exact same alignment — `lower_method_call`'s
/// own `arg_skip = 0` for that shape already establishes it — but
/// `call_out_param_arg_indices` never recognized the callee as bridged at
/// all, so the call site fell through to the ordinary (unbridged) call
/// lowering: the callee's own signature had already been rewritten to return
/// a tuple (`apply_out_param_bridge` runs unconditionally from `lower_method`),
/// but the caller kept passing its `int`s by value into a callee that no
/// longer writes through them, leaving the caller's own out-param locals a
/// `late` that's never assigned (`definitely_unassigned_late_local_variable`).
/// Real corpus repro: `StaffAlignment::GetLeftRight(int, int&, int&) const`
/// called as `topNote->GetAlignment()->GetLeftRight(staffN, minLeft, maxRight)`
/// from `AdjustArpegFunctor::VisitArpeg`.
#[test]
fn a_reference_out_param_on_an_ordinary_instance_method_bridges_to_a_dart_tuple_return() {
    let source = lower_and_emit(
        "lower-cpp-instance-method-reference-out-param",
        r#"
class StaffAlignment {
public:
    void GetLeftRight(int staffN, int &minLeft, int &maxRight) const {
        minLeft = staffN - 1;
        maxRight = staffN + 1;
    }
};

int use_it(StaffAlignment &align, int staffN) {
    int minLeft = 0;
    int maxRight = 0;
    align.GetLeftRight(staffN, minLeft, maxRight);
    return minLeft + maxRight;
}
"#,
    );

    assert!(
        source.contains("(int, int) GetLeftRight(int staffN, int minLeft, int maxRight)"),
        "expected the instance method's reference out-params to become a Dart tuple \
         return, got:\n{source}"
    );
    assert!(
        source.contains("(minLeft, maxRight) = align.GetLeftRight(staffN, minLeft, maxRight);"),
        "expected the call site to destructure the callee's tuple back into the \
         caller's own variables, got:\n{source}"
    );
    assert!(
        !source.contains("late int"),
        "an out-param local must never be left as an unassigned `late`, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "an instance-method out-param call must not bail out, got:\n{source}"
    );
}

/// F8/tarefa 10, second gap the previous test's fixture didn't reach: once
/// an ordinary instance method's out-param call is recognized at all, a
/// target reached through an array index (`points[0].y`, real trigger
/// `View::CalcOffsetBezier`'s `CalcOffsetSpanningStartY(dc, points[0].y,
/// spanningType)`) can't sit inside `Stmt::TupleAssign`'s ordinary
/// record-pattern syntax either — confirmed empirically with a real `dart
/// analyze`/`dart format` run against exactly this shape:
/// `(points[0].y,) = call();` both fails to parse ("Expected to find ')'")
/// and, before that syntax error, mis-typechecks as
/// `pattern_type_mismatch_in_irrefutable_context` (the analyzer reads
/// `points[0].y` as `points` itself being destructured). Same "route around
/// the pattern grammar with a temp-block" fix `tuple_assign_needs_temp_block`
/// already applies to a nullable-receiver field target, extended to any
/// target with an `Index` anywhere in its chain.
#[test]
fn a_tuple_assign_target_reached_through_an_array_index_avoids_pattern_assignment_syntax() {
    let source = lower_and_emit(
        "lower-cpp-tuple-assign-index-target",
        r#"
struct Point {
    int y;
};

class View {
public:
    void CalcOffsetY(int &y, int spanningType) const {
        y = y + spanningType;
    }

    void CalcOffsetBezier(Point points[4], int spanningType) {
        CalcOffsetY(points[0].y, spanningType);
    }
};
"#,
    );

    assert!(
        !source.contains("(points[0].y,) ="),
        "an array-indexed target must never appear inside a Dart pattern-assignment \
         target, got:\n{source}"
    );
    assert!(
        source.contains("points[0].y = _syntaxBridgeTupleAssign.$1;"),
        "expected the target assigned individually, via ordinary assignment syntax, \
         got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "an array-indexed out-param target must not bail out, got:\n{source}"
    );
}

/// F8/tarefa 10, third gap: `apply_out_param_bridge` deliberately never
/// bridges a non-`void`-returning function/method whose out-param is the
/// *reference* form (its own doc comment: only the *pointer* form is
/// eligible there) — real trigger `Verse::AdjustPosition(int &overlap, int
/// freeSpace, const Doc *doc)`, called as `_m_previousVerse->AdjustPosition(
/// overlap, m_freeSpace, m_doc)`. Before `out_param_indices_are_all_pointer_
/// form` existed, `call_out_param_arg_indices` recognized this call as
/// bridged anyway (the reference-form out-param alone was enough), emitting
/// `(overlap,) = ...AdjustPosition(...);` against a callee whose declaration
/// `apply_out_param_bridge` correctly left as a plain `int`-returning
/// method — a real `dart analyze` run against exactly this shape confirmed
/// `pattern_type_mismatch_in_irrefutable_context` ("matched value of type
/// 'int' isn't assignable to the required type '(Object?,)'"). The call
/// site must agree with its callee's own (unbridged) declaration: an
/// ordinary call whose own (int) return value the caller uses however the
/// C++ source did.
#[test]
fn a_non_void_returning_instance_method_with_a_reference_out_param_is_never_treated_as_bridged() {
    let source = lower_and_emit(
        "lower-cpp-non-void-reference-out-param-instance-method",
        r#"
class Verse {
public:
    int AdjustPosition(int &overlap, int freeSpace) {
        overlap = overlap - freeSpace;
        return overlap;
    }
};

int use_it(Verse &verse, int overlap, int freeSpace) {
    verse.AdjustPosition(overlap, freeSpace);
    return overlap;
}
"#,
    );

    assert!(
        source.contains("int AdjustPosition(int overlap, int freeSpace)"),
        "a non-void-returning method with a reference out-param must keep its plain, \
         unbridged declaration, got:\n{source}"
    );
    assert!(
        !source.contains("(overlap,)") && !source.contains("(int,) AdjustPosition"),
        "the callee was never bridged into a tuple return, so the call site must never \
         treat it as one, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "an unbridged non-void out-param call must not bail out, got:\n{source}"
    );
}

/// F8/tarefa 10, fourth gap: a bare-statement out-param call that *omits* a
/// trailing default argument needing non-trivial destruction (real trigger
/// `Alignment::GetLeftRight(int, int&, int&, const std::vector<ClassId>
/// &excludes = {})`, called as `leftAlignment->GetLeftRight(staffN, minLeft,
/// maxRight);` from `HorizontalAligner::SetOverflowAboveTuning`/`
/// SetOverflowBelowTuning`) sits inside an `ExprWithCleanups` wrapper —
/// libclang exposes it as the same `CXCursor_UnexposedExpr` sugar
/// `is_transparent_wrapper` already unwraps everywhere else, since the
/// default-constructed `std::vector` temporary needs a destructor call.
/// `lower_stmt`'s own bare-`CallExpr` special case checked `kind ==
/// CXCursor_CallExpr` against the *wrapped* cursor directly, so it silently
/// never matched here, and the call fell through to the ordinary (unbridged)
/// path even though the callee's own declaration was genuinely bridged —
/// confirmed only by comparing this exact call, with vs. without an
/// explicit trailing argument, against a real Verovio file
/// (`horizontalaligner.cpp`'s own two call shapes at lines 304 vs. 441).
#[test]
fn a_bare_out_param_call_omitting_a_trailing_default_argument_still_bridges() {
    let source = lower_and_emit(
        "lower-cpp-out-param-omitted-default-arg",
        r#"
#include <vector>

class Alignment {
public:
    void GetLeftRight(int staffN, int &minLeft, int &maxRight, const std::vector<int> &excludes = {}) const {
        minLeft = staffN - 1;
        maxRight = staffN + 1;
    }
};

int with_default_omitted(Alignment &align, int staffN) {
    int minLeft = 0;
    int maxRight = 0;
    align.GetLeftRight(staffN, minLeft, maxRight);
    return minLeft + maxRight;
}
"#,
    );

    assert!(
        source.contains("(minLeft, maxRight) = align.GetLeftRight(staffN, minLeft, maxRight);"),
        "expected the omitted-default-argument call to still destructure the callee's \
         tuple back into the caller's own variables, got:\n{source}"
    );
    assert!(
        !source.contains("late int"),
        "an out-param local must never be left as an unassigned `late`, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "an omitted-default-argument out-param call must not bail out, got:\n{source}"
    );
}

/// F8/tarefa 10, fifth (and last) gap: a caller-side local declared with no
/// C++ initializer, immediately followed by an out-param-bridged call that
/// reuses it as *both* an input argument and a destructuring target — real
/// trigger `AdjustArpegFunctor::VisitArpeg`'s `int minTopLeft; int
/// maxTopRight; ...GetLeftRight(staffN, minTopLeft, maxTopRight);`. Even
/// once every earlier gap in this family is fixed, the call site correctly
/// becomes `(minTopLeft, maxTopRight) = ...GetLeftRight(staffN, minTopLeft,
/// maxTopRight);` — but the two locals were still emitted as bare `late
/// int`, deferring initialization to "first use", and the *very first* use
/// is this same statement's own call reading them as arguments *before*
/// its own destructuring assignment ever runs — a genuine
/// `definitely_unassigned_late_local_variable`, confirmed against the real
/// Verovio corpus. A neutral default value stands in for whatever the
/// caller never actually set, exactly as safe as C++'s own indeterminate
/// initial value.
#[test]
fn a_caller_local_reused_as_both_out_param_call_input_and_target_gets_a_neutral_default_not_late() {
    let source = lower_and_emit(
        "lower-cpp-out-param-neutral-default-input",
        r#"
class StaffAlignment {
public:
    void GetLeftRight(int staffN, int &minLeft, int &maxRight) const {
        minLeft = staffN - 1;
        maxRight = staffN + 1;
    }
};

int use_it(StaffAlignment &align, int staffN) {
    int minLeft;
    int maxRight;
    align.GetLeftRight(staffN, minLeft, maxRight);
    return minLeft + maxRight;
}
"#,
    );

    assert!(
        !source.contains("late int"),
        "a local reused as a bridged call's own input argument must never be left as an \
         unassigned `late`, got:\n{source}"
    );
    assert!(
        source.contains("int minLeft = 0;") && source.contains("int maxRight = 0;"),
        "expected a neutral default value instead of `late`, got:\n{source}"
    );
    assert!(
        source.contains("(minLeft, maxRight) = align.GetLeftRight(staffN, minLeft, maxRight);"),
        "expected the call site to still destructure the callee's tuple back into the \
         caller's own variables, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "must not bail out, got:\n{source}"
    );
}

/// F8/tarefa 10, the same neutral-default gap but with unrelated
/// declarations sitting *between* the out-param locals and the bridged
/// call — real trigger `Doc::GetGlyphHeight`'s `int x; int y; int w; int
/// h; Resources resources = GetResources(); Glyph *glyph =
/// resources.GetGlyph(code); ...GetBoundingBox(x, y, w, h);`. An
/// adjacency-only backward scan (this family's first version) stops at the
/// very first non-matching statement and never reaches `x`/`y`/`w`/`h` at
/// all; the fix has to skip over statements unrelated to the out-param
/// locals instead of stopping at them.
#[test]
fn a_neutral_default_still_applies_across_unrelated_statements_before_the_bridged_call() {
    let source = lower_and_emit(
        "lower-cpp-out-param-neutral-default-with-gap",
        r#"
class Glyph {
public:
    void GetBoundingBox(int &x, int &y, int &w, int &h) const {
        x = 0;
        y = 0;
        w = 10;
        h = 10;
    }
};

class Resources {
public:
    Glyph GetGlyph(int code) const {
        return Glyph();
    }
};

int use_it(Resources &resources, int code) {
    int x;
    int y;
    int w;
    int h;
    Glyph glyph = resources.GetGlyph(code);
    glyph.GetBoundingBox(x, y, w, h);
    return x + y + w + h;
}
"#,
    );

    assert!(
        !source.contains("late int"),
        "an out-param local separated from its bridged call by unrelated statements must \
         still never be left as an unassigned `late`, got:\n{source}"
    );
    assert!(
        source.contains("(x, y, w, h) = glyph.GetBoundingBox(x, y, w, h);"),
        "expected the call site to still destructure the callee's tuple back into the \
         caller's own variables, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "must not bail out, got:\n{source}"
    );
}

/// `std::stringstream`/`std::ostringstream` accumulation (round 19, real
/// trigger `options.cpp`'s `OptionArray::GetStr`): `ss << a << b;` used as
/// its own statement, across several separate insertions (including inside
/// a loop), then read back with `.str()`. Modeled directly as `Type::Str`
/// end to end — the declaration, every `<<` statement, and the final
/// `.str()` read.
#[test]
fn a_stringstream_accumulates_across_statements_and_reads_back_with_str() {
    let source = lower_and_emit(
        "lower-cpp-stringstream-accumulator",
        r#"
#include <sstream>
#include <string>
#include <vector>

std::string join(const std::vector<std::string>& values) {
    std::stringstream ss;
    int i = 0;
    for (std::string const& value : values) {
        if (i != 0) {
            ss << ", ";
        }
        ss << "\"" << value << "\"";
        ++i;
    }
    return ss.str();
}
"#,
    );

    assert!(
        source.contains("String ss = '';"),
        "expected the stringstream to start as an empty Dart String, got:\n{source}"
    );
    assert!(
        source.contains("ss = ss + ', ';") && source.contains("ss = ss + '\"' + value + '\"';"),
        "expected each insertion chain to reassign ss by concatenation, got:\n{source}"
    );
    assert!(
        source.contains("return ss;"),
        "expected ss.str() to read back the accumulated string directly, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "stringstream accumulation must not bail out, got:\n{source}"
    );
}

/// A numeric insertion into a stringstream needs `.toString()`, the same
/// way the `std::cout` chain already converts a non-`Str` operand.
#[test]
fn a_stringstream_converts_non_string_operands_with_to_string() {
    let source = lower_and_emit(
        "lower-cpp-stringstream-numeric",
        r#"
#include <sstream>
#include <string>

std::string describe(int count) {
    std::stringstream ss;
    ss << "count=" << count;
    return ss.str();
}
"#,
    );

    assert!(
        source.contains("ss = ss + 'count=' + count.toString();"),
        "expected the int operand to be converted with toString(), got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "stringstream numeric insertion must not bail out, got:\n{source}"
    );
}

/// A real invalid-Dart bug found in the Verovio corpus itself (round 19,
/// `zip_file.hpp`'s `tdefl_compress_normal`, confirmed via `dart format`
/// against the real emitted package: "Expected to find ';'."): `*pSrc++`
/// (C's dereference-then-advance idiom, `pSrc` a known byte-buffer
/// pointer) lowers `Expr::Convert{ operand: Unary{PostIncrement, pSrc},
/// ty: Int }`, which used to render bare as `pSrc++.toInt()` — a postfix
/// `++` can't have a suffix chained directly onto it in Dart.
#[test]
fn a_postfix_increment_converted_to_int_is_parenthesized() {
    let source = lower_and_emit(
        "lower-cpp-postfix-increment-convert",
        r#"
#include <cstdint>

int read_and_advance(const uint8_t* pSrc) {
    int c = *pSrc++;
    return c;
}
"#,
    );

    assert!(
        source.contains("(pSrc++).toInt()"),
        "a postfix increment converted with .toInt() must be parenthesized, got:\n{source}"
    );
}

/// Round 21: a byte-indexed *write* into a `std::string`
/// (`keyString[i] = tolower(keyString[i]);`, real trigger — grepped
/// directly, `ioabc.cpp:624` and `json/jsonxx.cc:637`'s `input[size - 2] =
/// ' ';`). Dart's `String` is immutable — there is no in-place indexed
/// assignment — so this reassigns the whole variable: encode to UTF-8
/// bytes (the same byte model `Expr::StringByteAt`'s *read* side already
/// uses), write the one byte, decode back.
#[test]
fn a_string_byte_index_write_reassigns_the_whole_string() {
    let source = lower_and_emit(
        "lower-cpp-string-byte-index-write",
        r#"
#include <string>

void lowercase_first(std::string& s) {
    s[0] = 'a';
}
"#,
    );

    assert!(
        source.contains("List<int> _syntaxBridgeStringBytes = utf8.encode(s);")
            && source.contains("_syntaxBridgeStringBytes[0] = ")
            && source.contains("s = utf8.decode(_syntaxBridgeStringBytes);"),
        "expected the string byte write to encode/mutate/decode, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "string byte-index write must not bail out, got:\n{source}"
    );
}

/// The same rewrite, but the target is a `std::string` *field* reached
/// through a plain (non-nullable) receiver — `keyString[i] = ...;` where
/// `keyString` might itself be a field, not always a bare local.
#[test]
fn a_string_byte_index_write_through_a_field_target_reassigns_the_field() {
    let source = lower_and_emit(
        "lower-cpp-string-byte-index-write-field",
        r#"
#include <string>

struct Holder {
    std::string keyString;
    void LowerFirst() { keyString[0] = 'a'; }
};
"#,
    );

    assert!(
        source.contains("keyString = utf8.decode(_syntaxBridgeStringBytes);"),
        "expected the field target to be reassigned after the byte write, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "string byte-index write through a field must not bail out, got:\n{source}"
    );
}

/// A real invalid-Dart bug found in the Verovio corpus itself (round 22,
/// `humlib.h`'s `class HumdrumToken : public std::string, public
/// HumHash`, confirmed against the real emitted package:
/// `class HumdrumToken with string, HumHash {`, referencing an undeclared
/// Dart class `string` — a `dart analyze` `undefined_class`).
/// `base_classes_of` included *any* base with a resolvable usr/name,
/// including one that maps to a library adapter (`Type::Str` for
/// `std::string`) this bridge never backs with a real declared Dart
/// class. Scoped to exactly `Type::Record`/`Type::Enum` bases now — the
/// only shapes this bridge ever emits a class declaration for.
#[test]
fn a_base_class_that_is_a_library_adapter_is_never_emitted_as_a_dart_mixin_or_extends() {
    let source = lower_and_emit(
        "lower-cpp-string-base-class",
        r#"
#include <string>

class Hashable {
public:
    int Hash() { return 0; }
};

class Comparable {
public:
    int Compare() { return 0; }
};

class Token : public std::string, public Hashable, public Comparable {
public:
    int m_x = 0;
};
"#,
    );

    assert!(
        !source.contains("with string") && !source.contains("extends string"),
        "must never reference an undeclared Dart class \"string\" as a base/mixin, \
         got:\n{source}"
    );
    assert!(
        source.contains("class Token with Hashable, Comparable"),
        "the genuine project base classes must still be kept as mixins, got:\n{source}"
    );
}

/// The single-base form (E06's "extends", not E09's multi-base mixin
/// list) needs the same exclusion: a class inheriting *only* from a
/// library adapter must declare no base at all, not `extends string`.
#[test]
fn a_single_library_adapter_base_is_never_emitted_as_a_dart_extends_clause() {
    let source = lower_and_emit(
        "lower-cpp-string-only-base-class",
        r#"
#include <string>

class Token : public std::string {
public:
    int m_x = 0;
};
"#,
    );

    assert!(
        !source.contains("extends string") && !source.contains("with string"),
        "must never reference an undeclared Dart class \"string\" as a base, got:\n{source}"
    );
    assert!(
        source.contains("class Token {"),
        "a class whose only base is a library adapter must declare no Dart base at all, \
         got:\n{source}"
    );
}

/// `void*`/`const void*` — the single largest type bailout in the
/// 2026-08-20 Verovio diagnosis (896 + 253 occurrences), real shapes
/// confirmed by grepping the extracted Verovio source directly
/// (`test-resources/verovio-version-6.2.0.tar.gz`):
/// `include/vrv/floatingobject.h`'s `SetDrawingGrpObject(void
/// *drawingGrpObject)` (a parameter), `include/pugi/pugixml.hpp`'s `void*
/// _impl;` (a field). `mapping::pointer_options_for` already answers
/// `"ponte-dart-ffi"` for an opaque pointee (`void`, a scalar, or an
/// already-unrepresentable pointee) — this only finishes that option's own
/// Dart realization for the `void` case specifically: a named, documented
/// bridge (`SyntaxBridgeNativeHandle`, identity-only, never dereferenced or
/// arithmetic'd) instead of the generic `Type::Unsupported` bailout. Scoped
/// to a `void` pointee only — a pointer to an unrepresentable scalar/record
/// pointee still needs its own future decision and stays `Unsupported`.
#[test]
fn a_void_pointer_lowers_to_a_named_native_handle_bridge_instead_of_a_bailout() {
    let source = lower_and_emit(
        "lower-cpp-void-pointer-native-handle",
        r#"
struct Holder {
    void *_impl;
    int SetDrawingGrpObject(void *drawingGrpObject) {
        _impl = drawingGrpObject;
        return _impl == nullptr ? 0 : 1;
    }
};
"#,
    );

    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "got:\n{source}"
    );
    assert!(
        source.contains("SyntaxBridgeNativeHandle? impl;"),
        "expected the field to keep a precise, named nullable type — and, since this public field's \
         C++ spelling starts with `_`, that leading underscore stripped (tarefa 03's leading-\
         underscore fix: a non-private member's bare name must not accidentally trip Dart's own \
         privacy convention), got:\n{source}"
    );
    assert!(
        source.contains("int SetDrawingGrpObject(SyntaxBridgeNativeHandle? drawingGrpObject)"),
        "expected the parameter to keep the same named type, got:\n{source}"
    );
    assert!(
        source.contains("impl = drawingGrpObject;") && source.contains("impl == null"),
        "expected assignment and null-comparison to keep working without any adapter, got:\n{source}"
    );
}

/// A `const void*` uses the exact same bridge as a plain `void*` — C++'s
/// `const` here only restricts writes through the pointer, which this
/// bridge (identity-only, no dereference) never does anyway.
#[test]
fn a_const_void_pointer_uses_the_same_native_handle_bridge_as_a_plain_void_pointer() {
    let source = lower_and_emit(
        "lower-cpp-const-void-pointer-native-handle",
        r#"
struct Writer {
    virtual void Write(const void *data, unsigned long size) = 0;
};
"#,
    );

    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "got:\n{source}"
    );
    assert!(
        source.contains("SyntaxBridgeNativeHandle? data"),
        "got:\n{source}"
    );
}

/// A `case` that falls through into the next one without an explicit
/// `break`/`continue`/`return`/`throw` — genuine C++ fallthrough, which
/// used to bail out the whole `switch` (`docs/plans/bailouts-verovio-6.2.0.md`'s
/// "a case falls through..." family, 36 occurrences in the 2026-08-20
/// diagnosis). Dart has its own explicit fallthrough syntax
/// (`continue <label>;` into a labeled sibling `case`), confirmed real Dart
/// syntax, not guessed. Exercises a chain of two falls-through (`1` into
/// `2`, `2` into `3`) so a fixture only covering one hop can't hide a
/// bug that only shows up in a longer chain.
#[test]
fn a_case_that_falls_through_into_the_next_one_uses_darts_own_continue_label_syntax() {
    let source = lower_and_emit(
        "lower-cpp-switch-fallthrough",
        r#"
#include <string>
std::string describe(int level) {
    std::string result = "";
    switch (level) {
        case 1:
            result += "low ";
        case 2:
            result += "mid ";
        case 3:
            result += "high";
            break;
        default:
            result = "unknown";
            break;
    }
    return result;
}
"#,
    );

    assert!(
        !source.contains("Unsupported")
            && !source.contains("a case falls through")
            && !source.contains("dynamic"),
        "got:\n{source}"
    );
    assert!(
        source.contains("continue _syntaxBridgeCase1;")
            && source.contains("continue _syntaxBridgeCase2;"),
        "expected each falling-through case to jump via Dart's own continue-label syntax, got:\n{source}"
    );
    assert!(
        source.contains("_syntaxBridgeCase1:\n    case 2:")
            && source.contains("_syntaxBridgeCase2:\n    case 3:"),
        "expected each label printed right before the case it targets, got:\n{source}"
    );
}

/// A `for` header missing one or more of its three clauses
/// (`for (;;)`-shaped) — `clang_visitChildren` silently skips an absent
/// clause with no positional marker, ambiguous from cursor kinds alone.
/// Real family: "ForStmt had 3/1 children" (28+20 occurrences in the
/// 2026-08-20 diagnosis). Exercises every combination a single fixture
/// reasonably can: missing increment, missing init, missing both init and
/// condition, and the fully-empty `for (;;)`.
#[test]
fn a_for_loop_missing_one_or_more_clauses_still_lowers_correctly() {
    let source = lower_and_emit(
        "lower-cpp-for-missing-clauses",
        r#"
void f() {
    int i;
    for (i = 0; i < 10; ) {
        i++;
    }
    for (; i < 10; i++) {
    }
    for (i = 0; ; i++) {
        if (i >= 10) break;
    }
    for (;;) {
        break;
    }
}
"#,
    );

    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "got:\n{source}"
    );
    assert!(
        source.contains("for (i = 0; i < 10; ) {"),
        "expected the missing-increment loop to keep its init/condition, got:\n{source}"
    );
    assert!(
        source.contains("for (; i < 10; i++) {"),
        "expected the missing-init loop to keep its condition/increment, got:\n{source}"
    );
    assert!(
        source.contains("for (i = 0; ; i++) {"),
        "expected the missing-condition loop to keep its init/increment, got:\n{source}"
    );
    assert!(
        source.contains("for (; ; ) {"),
        "expected the fully-empty header to lower with no clauses at all, got:\n{source}"
    );
}

/// `std::multiset<T>` — allows duplicates, unlike `std::set`/
/// `std::unordered_set`, so it lowers to `List<T>` (which preserves
/// duplicates) rather than `Set<T>` (which would silently drop them). Real
/// trigger family: "std::multiset (spelling: multiset<int>)" (9 occurrences
/// in the 2026-08-20 diagnosis).
#[test]
fn a_multiset_lowers_to_a_list_to_preserve_duplicate_elements() {
    let source = lower_and_emit(
        "lower-cpp-multiset",
        r#"
#include <set>
void f(std::multiset<int> values) { }
"#,
    );

    assert!(
        source.contains("void f(List<int> values)"),
        "expected multiset to lower to List<int>, got:\n{source}"
    );
    assert!(!source.contains("Unsupported"), "got:\n{source}");
}

/// `struct { ... } s, *ps;` — an inline struct definition followed by one
/// or more variable declarators using it, in the same `DeclStmt`. Confirmed
/// via `clang++ -Xclang -ast-dump` (not guessed): the `DeclStmt`'s direct
/// children are `[CXXRecordDecl, VarDecl(s), VarDecl(ps)]` — the type
/// declaration is a sibling of the real declarators, not their parent. Real
/// family: "DeclStmt's declarator is not a VarDecl" (44 occurrences in the
/// 2026-08-20 diagnosis).
#[test]
fn an_inline_struct_definition_alongside_its_declarators_lowers_only_the_variables() {
    let source = lower_and_emit(
        "lower-cpp-inline-struct-declarators",
        r#"
void f() {
    struct Point { int x; int y; } p, *pp;
    p.x = 1;
}
"#,
    );

    assert!(
        !source.contains("Unsupported")
            && !source.contains("dynamic")
            && !source.contains("declarator is not a VarDecl"),
        "got:\n{source}"
    );
    assert!(
        source.contains("Point p = Point(0, 0);") || source.contains("Point p"),
        "expected the first declarator to still lower, got:\n{source}"
    );
    assert!(source.contains("Point? pp"), "got:\n{source}");
}

/// `delete ptr;` — real triggers found by grepping the extracted Verovio
/// source directly (`layer.cpp`'s `delete m_staffDefClef;`, `toolkit.cpp`'s
/// `delete m_editorToolkit;`). This IR's pointers are plain GC-managed
/// Dart references with no ownership tracking yet, so manual deletion is a
/// no-op — omitted the same way a bare `;` already is.
#[test]
fn a_delete_statement_is_omitted_as_a_no_op() {
    let source = lower_and_emit(
        "lower-cpp-delete-statement",
        r#"
struct Clef {};
struct Layer {
    Clef *m_staffDefClef = nullptr;
    void ResetStaffDefObjects() {
        delete m_staffDefClef;
        m_staffDefClef = nullptr;
    }
};
"#,
    );

    assert!(
        !source.contains("Unsupported")
            && !source.contains("dynamic")
            && !source.contains("delete"),
        "expected the delete statement to vanish as a no-op, got:\n{source}"
    );
    assert!(
        source.contains("m_staffDefClef = null;"),
        "expected the surrounding statement to still lower normally, got:\n{source}"
    );
}

/// The textually last clause of a `switch` (here, the last `case` since
/// there is no `default`) needs no terminator at all — falling out the
/// bottom of a `switch` is already valid in both C++ and Dart, unlike
/// falling into the *next* case.
#[test]
fn the_last_case_of_a_switch_needs_no_terminator() {
    let source = lower_and_emit(
        "lower-cpp-switch-last-case-no-terminator",
        r#"
int classify(int level) {
    int result = 0;
    switch (level) {
        case 1:
            result = 10;
            break;
        case 2:
            result = 20;
    }
    return result;
}
"#,
    );

    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "got:\n{source}"
    );
    assert!(
        source.contains("case 2:\n      result = 20;\n  }"),
        "expected the last case to close the switch with no forced terminator, got:\n{source}"
    );
}

/// Under `-std=c++20` — the *real* standard Verovio 6.2.0 itself builds
/// with (`cmake/CMakeLists.txt`'s `set(CMAKE_CXX_STANDARD 20)`), not the
/// `-std=c++17` `lower_and_emit`'s fixtures default to — the manual
/// iterator idiom's `it != X.end()` condition compiles through C++20's
/// rewritten-candidates rule: `libstdc++`'s iterator classes define
/// `operator==` but no separate `operator!=`, so the call is to `==`, negated
/// by a real `UnaryOperator '!'` wrapped in a `CXXRewrittenBinaryOperator`
/// (confirmed with a real `clang++ -Xclang -ast-dump -std=c++20`). The first
/// version of this loop recognizer only matched a direct `operator!=` call
/// and so passed every one of this file's own `-std=c++17` fixtures while
/// matching *zero* real occurrences in the actual Verovio corpus — this
/// fixture pins `-std=c++20` specifically to catch that regression again.
#[test]
fn manual_iterator_loop_matches_the_cxx20_rewritten_not_equal_condition() {
    let source = lower_and_emit_with_std(
        "lower-cpp-manual-iterator-cxx20-rewritten",
        r#"
#include <list>

class Object {};

typedef std::list<const Object*> ListOfConstObjects;

bool has_multiple(ListOfConstObjects staves) {
    int count = 0;
    for (auto it = staves.begin(); it != staves.end(); ++it) {
        const Object *staff = *it;
        if (staff) count++;
        if (count > 1) return true;
    }
    return false;
}
"#,
        "c++20",
    );

    assert!(
        source.contains("for (final Object? it in staves)"),
        "expected the c++20-rewritten condition to still be recognized as the manual \
         iterator idiom, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "manual iterator loop under c++20 must not bail out, got:\n{source}"
    );
}

/// `container[i].field.begin()` — a real trigger from `iohumdrum.cpp`
/// (`ss[staffindex].tieends.begin()`). `same_receiver_ignoring_origin` only
/// compared `Ref`/`FieldAccess`/`This` shapes; an `Expr::Index` receiver
/// reached through a `FieldAccess` fell to its final `_ => false` arm even
/// when the three mentions (`begin`, `end` inside the loop check, `end`
/// implicitly re-checked) are syntactically identical, so the whole loop
/// idiom silently failed to match for any indexed receiver.
#[test]
fn manual_iterator_loop_recognizes_an_indexed_field_receiver() {
    let source = lower_and_emit(
        "lower-cpp-manual-iterator-indexed-receiver",
        r#"
#include <list>
#include <vector>

struct Bag {
    std::list<int> items;
};

int sum_all(std::vector<Bag>& bags, int idx) {
    int total = 0;
    for (auto it = bags[idx].items.begin(); it != bags[idx].items.end(); ++it) {
        total += *it;
    }
    return total;
}
"#,
    );

    assert!(
        source.contains("for (final int it in bags[idx].items)")
            && source.contains("total = total + it;"),
        "expected the indexed-field receiver to be recognized as the same container across \
         begin/end, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "manual iterator loop over an indexed field must not bail out, got:\n{source}"
    );
}

/// `container_begin_or_end_receiver`'s scope only listed
/// `vector`/`list`/`set`/`deque`/`multiset` — `unordered_set` maps to the
/// same `Type::Set` elsewhere in this module (`lower_type`'s
/// `CXType_Record` branch), so excluding it here was an oversight, not a
/// deliberate scope decision the way `map`/`unordered_map` are.
#[test]
fn manual_iterator_loop_supports_unordered_set() {
    let source = lower_and_emit(
        "lower-cpp-manual-iterator-unordered-set",
        r#"
#include <unordered_set>

int sum_all(const std::unordered_set<int>& values) {
    int total = 0;
    for (auto it = values.begin(); it != values.end(); ++it) {
        total += *it;
    }
    return total;
}
"#,
    );

    assert!(
        source.contains("for (final int it in values)") && source.contains("total = total + it;"),
        "expected unordered_set to be recognized by the manual iterator idiom, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "manual unordered_set iterator loop must not bail out, got:\n{source}"
    );
}

/// A pre-existing bug found while building round 19's stringstream support
/// (not specific to it): `std::string s;` (default-constructed, no written
/// initializer) lowered to `String s = basic_string();` — a call to a Dart
/// function that is never generated, invalid Dart. C++ guarantees a
/// default-constructed `std::string` is empty, so `default_scalar_value`'s
/// `''` is the exact right value, not a guess.
#[test]
fn a_bare_default_constructed_string_starts_as_an_empty_dart_string() {
    let source = lower_and_emit(
        "lower-cpp-bare-string-default-construct",
        r#"
#include <string>
std::string f() {
    std::string s;
    return s;
}
"#,
    );

    assert!(
        source.contains("String s = '';"),
        "expected a default-constructed std::string to start as an empty Dart String, \
         got:\n{source}"
    );
    assert!(
        !source.contains("basic_string()"),
        "must never call the nonexistent Dart function basic_string(), got:\n{source}"
    );
}

/// Round 23: `std::stack<T>` is LIFO-only but element-typed exactly like
/// `vector`/`list`/`deque` — real trigger `view_page.cpp`'s
/// `stack<Brush>`/`stack<Pen>` (drawing-context save/restore). `.top()` is
/// the last element, `.push`/`.pop` are `.add`/`.removeLast`.
#[test]
fn stack_push_pop_top_map_to_list_add_removelast_last() {
    let source = lower_and_emit(
        "lower-cpp-stack",
        r#"
#include <stack>

int f(std::stack<int>& s, int v) {
    s.push(v);
    int t = s.top();
    s.pop();
    return t;
}
"#,
    );

    assert!(
        source.contains("s.add(v);"),
        "expected push to become add, got:\n{source}"
    );
    assert!(
        source.contains("s[s.length - 1]"),
        "expected top to index the last element, got:\n{source}"
    );
    assert!(
        source.contains("s.removeLast();"),
        "expected pop to become removeLast, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "stack push/top/pop must not bail out, got:\n{source}"
    );
}

/// Round 23: `std::map::at(key)` must fetch a value that is required to
/// exist (C++ throws `out_of_range` otherwise) — `map[key]!` preserves that
/// "must exist" intent with a real Dart failure on a missing key.
#[test]
fn map_at_force_unwraps_the_looked_up_value() {
    let source = lower_and_emit(
        "lower-cpp-map-at",
        r#"
#include <map>
#include <string>

int f(std::map<std::string, int>& m, const std::string& key) {
    return m.at(key);
}
"#,
    );

    assert!(
        source.contains("m[key]!"),
        "expected map::at to force-unwrap the indexed value, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "map::at must not bail out, got:\n{source}"
    );
}

/// Round 23: `unique_ptr<T>`/`shared_ptr<T>`/`optional<T>` already lower to
/// `T?` at the type level — reading the wrapped value back (`.get()`,
/// `operator->`) is identity. Real trigger: `Object`'s
/// `std::unique_ptr<ListOfConstObjects> m_plistReferences`, read via
/// `.get()` in a getter and `->push_back(...)` in `object.cpp`.
#[test]
fn unique_ptr_get_and_arrow_are_identity_on_the_nullable_value() {
    let source = lower_and_emit(
        "lower-cpp-unique-ptr-get-arrow",
        r#"
#include <memory>
#include <vector>

struct Holder {
    std::unique_ptr<std::vector<int>> m_items;

    const std::vector<int> *Get() const { return m_items.get(); }
    void Add(int v) { m_items->push_back(v); }
};
"#,
    );

    assert!(
        source.contains("return m_items;"),
        "expected .get() to be identity on the nullable field, got:\n{source}"
    );
    assert!(
        source.contains("m_items!.add(v);"),
        "expected operator-> to force-unwrap then dispatch the method, got:\n{source}"
    );
    assert!(
        !source.contains("Unsupported") && !source.contains("dynamic"),
        "unique_ptr get/operator-> must not bail out, got:\n{source}"
    );
}

/// Regression: `.begin()`/`.end()` (and any other call whose own static
/// type is an unrecognized, system-header-only implementation type — here
/// libstdc++'s `__gnu_cxx::__normal_iterator`) used outside the narrow
/// idioms this bridge lowers specially still hits `lower_stdlib_method_call`'s
/// generic fallback, which types the bailout from the call's own
/// `lower_type`. `lower_type`'s `CXType_Record`/`CXType_Unexposed` branch
/// used to fall through to `Type::Record { usr, name }` for *any* named,
/// non-anonymous declaration with a real USR — including one from a system
/// header that this project never declares a Dart class for. The typed
/// bailout then printed that bare name as a generic type argument
/// (`_syntaxBridgeUnsupported<__normal_iterator>(...)`), which doesn't
/// parse: `__normal_iterator` names no Dart type. A record type is only
/// ever real here when it's declared in the project's own source, never a
/// system header.
#[test]
fn a_bailout_typed_from_an_unrecognized_system_header_type_never_prints_a_bare_undeclared_name() {
    let source = lower_and_emit(
        "lower-cpp-iterator-typed-bailout",
        r#"
#include <vector>

struct Coord {
    int m_x;
};

struct Thing {
    std::vector<Coord> m_refs;
    int f() {
        return m_refs.begin()->m_x;
    }
};
"#,
    );

    assert!(
        !source.contains("<__normal_iterator>"),
        "a system-header-only type must never print as a bare, undeclared Dart type \
         argument (appearing inside a bailout's own message string is fine), got:\n{source}"
    );
    assert!(
        source.contains("throw UnimplementedError("),
        "expected an honest bailout instead, got:\n{source}"
    );
}

/// F6/tarefa 07, Metade A: `std::max`/`std::min`/`std::abs`/`std::to_string`
/// as free functions — the causa raiz the prompt names is that these all
/// fell through `lower_call_expr`'s generic path (accepted because
/// `is_plain_dart_identifier("max")` is true) and printed as a bare,
/// undefined-in-Dart identifier, read as `this.max(...)` when called from
/// inside a method. `lower_stdlib_free_function_call` now recognizes each
/// one before that fallback.
#[test]
fn std_max_min_abs_to_string_lower_to_their_dart_equivalents() {
    let source = lower_and_emit(
        "lower-cpp-std-free-functions",
        r#"
#include <algorithm>
#include <cmath>
#include <string>

int clamp_ish(int a, int b, int c) {
    int hi = std::max(a, b);
    int lo = std::min(a, b);
    int magnitude = std::abs(c);
    std::string text = std::to_string(c);
    return hi + lo + magnitude + (int)text.size();
}
"#,
    );

    assert!(
        source.contains("math.max(a, b)"),
        "expected std::max to lower to math.max, got:\n{source}"
    );
    assert!(
        source.contains("math.min(a, b)"),
        "expected std::min to lower to math.min, got:\n{source}"
    );
    assert!(
        source.contains("c.abs()"),
        "expected std::abs(c) to lower to c.abs(), got:\n{source}"
    );
    assert!(
        source.contains("c.toString()"),
        "expected std::to_string(c) to lower to c.toString(), got:\n{source}"
    );
    assert!(
        source.contains("import 'dart:math' as math;"),
        "expected a namespaced dart:math import, got:\n{source}"
    );
    assert!(
        !source.contains("throw UnimplementedError("),
        "got:\n{source}"
    );
}

/// F6/tarefa 07, Metade A: `std::make_pair(a, b)` and the equivalent direct
/// `std::pair<A, B>(a, b)` construction both target `SyntaxBridgePair` —
/// `Type::Pair`'s own Dart representation, already used by
/// `mock_value_for_type`/pointee-shape lookups but never, before this,
/// actually constructed from live values.
#[test]
fn std_make_pair_and_pair_construction_lower_to_syntax_bridge_pair() {
    let source = lower_and_emit(
        "lower-cpp-std-pair",
        r#"
#include <utility>

std::pair<int, int> via_make_pair(int a, int b) {
    return std::make_pair(a, b);
}

std::pair<int, int> via_direct_construction(int a, int b) {
    return std::pair<int, int>(a, b);
}
"#,
    );

    assert!(
        source.contains("SyntaxBridgePair(a, b)"),
        "expected both std::make_pair and std::pair(...) to build a SyntaxBridgePair, got:\n{source}"
    );
    assert!(
        source.contains("import 'syntax_bridge_support.dart';"),
        "expected the support-file import for SyntaxBridgePair, got:\n{source}"
    );
    assert!(
        !source.contains("throw UnimplementedError("),
        "got:\n{source}"
    );
}

/// F6/tarefa 07, Metade A: `std::swap(a, b);` mutates both operands, so it
/// can't rewrite to a single expression the way `std::max`/`std::abs` do —
/// it expands into a hoisted temporary plus two assignments.
#[test]
fn std_swap_lowers_to_a_temp_variable_and_two_assignments() {
    let source = lower_and_emit(
        "lower-cpp-std-swap",
        r#"
#include <algorithm>

void trade(int& a, int& b) {
    std::swap(a, b);
}
"#,
    );

    assert!(
        source.contains("int _syntaxBridgeSwapTemp = a;"),
        "expected a hoisted temp holding a's original value, got:\n{source}"
    );
    assert!(
        source.contains("a = b;"),
        "expected a to take b's value, got:\n{source}"
    );
    assert!(
        source.contains("b = _syntaxBridgeSwapTemp;"),
        "expected b to take a's original value back from the temp, got:\n{source}"
    );
    assert!(!source.contains("swap(a, b)"), "got:\n{source}");
    assert!(
        !source.contains("throw UnimplementedError("),
        "got:\n{source}"
    );
}

/// F6/tarefa 07, Metade B: a call to `memset` (libc, declared in a system
/// header, never defined by this project) must become a real, imported,
/// named Dart adapter — visible as its own function, its call site
/// importing it — never a bare, undefined-in-Dart identifier printed
/// literally (the doc's own real trigger: `zip_file.cpp`'s `free(pComp);`).
/// Exercises the full pipeline `project_service::build_transpiled_package`
/// itself uses: catalog → `externals::effective_external_set` → `emit::
/// dart::emit_module_with_externals` — a plain `emit_module` (what
/// `lower_and_emit` calls) never mocks anything, external or not.
#[test]
fn a_libc_free_function_call_becomes_a_named_external_adapter() {
    let workspace =
        TempWorkspace::new("lower-cpp-libc-external-memset").expect("create temporary workspace");
    let unit = write_fixture(
        workspace.path(),
        r#"
#include <cstring>

void clear(void* p, int n) {
    memset(p, 0, n);
}
"#,
        "probe.cpp",
    );
    let catalog = function_catalog::extract_function_catalog(&[unit], workspace.path(), None)
        .expect("extract function catalog");

    let memset_declaration = catalog
        .declarations
        .iter()
        .find(|declaration| declaration.name == "memset")
        .expect("expected memset to be cataloged from its call site");
    assert!(
        !memset_declaration.has_definition,
        "expected memset to be cataloged as undefined by this project"
    );

    let external_usr_owned: Vec<String> = syntax_bridge_server::externals::effective_external_set(
        &[],
        &catalog.declarations,
        &catalog.calls,
        &[],
        &[],
        &[],
        &[],
    )
    .into_iter()
    .filter(|status| status.effective)
    .map(|status| status.usr)
    .collect();
    let external_usrs: std::collections::HashSet<&str> =
        external_usr_owned.iter().map(String::as_str).collect();
    assert!(
        external_usrs.contains(memset_declaration.usr.as_str()),
        "expected memset's usr to be auto-detected as external, got: {external_usrs:?}"
    );

    let module = syntax_bridge_server::ir::Module {
        functions: catalog.ir_functions.clone(),
        records: catalog.ir_records.clone(),
        enums: catalog.ir_enums.clone(),
    };
    let source =
        syntax_bridge_server::emit::dart::emit_module_with_externals(&module, &external_usrs)
            .into_values()
            .collect::<Vec<_>>()
            .join("\n");

    assert!(
        source.contains("memset(p, 0, n)"),
        "expected the call site to still reference memset by name, got:\n{source}"
    );
    assert!(
        source.contains("memset("),
        "expected a real, declared Dart adapter for memset, got:\n{source}"
    );
    assert!(
        !source.contains("throw UnimplementedError("),
        "expected memset's own body to be a plausible mock, not a throw, got:\n{source}"
    );
}

/// F6/tarefa 07, Metade B: `va_list`'s own libc/glibc shape (`typedef struct
/// __va_list_tag __builtin_va_list[1];`) reaches `lower_type`'s array
/// branch, whose element is `__va_list_tag` — a *compiler builtin*, not a
/// declaration from any real header, so `clang_Location_isInSystemHeader`
/// on it is false and it used to fall through as a bare, undeclared
/// `Type::Record`, printing `List<__va_list_tag>` (`dart analyze`'s
/// `non_type_as_type_argument`, 17 of the 19 real Verovio occurrences —
/// `vrv.cpp`'s varargs helpers).
#[test]
fn a_va_list_parameter_never_prints_the_bare_undeclared_va_list_tag_type() {
    let source = lower_and_emit(
        "lower-cpp-va-list-tag",
        r#"
#include <cstdarg>

void useVaList(va_list args) {
}
"#,
    );

    assert!(
        !source.contains("<__va_list_tag>"),
        "__va_list_tag must never print as a bare, undeclared Dart type argument \
         (appearing inside a bailout's own comment/message is fine), got:\n{source}"
    );
    assert!(
        source.contains("SyntaxBridgeOpaque"),
        "expected an honest opaque bailout for __va_list_tag instead, got:\n{source}"
    );
}

/// F6/tarefa 07: `std::string()`'s default constructor — real Verovio
/// trigger, `jsonxx.cc`'s `const std::string &attr = std::string()` default
/// parameter value and `return std::string();` — must lower to Dart's empty
/// string literal, not fall through to a literal `basic_string()` call
/// (`dart analyze`'s `undefined_function`; `basic_string` was never
/// `lower_record`'d, so no such Dart function exists).
#[test]
fn a_default_constructed_std_string_lowers_to_an_empty_string_literal() {
    let source = lower_and_emit(
        "lower-cpp-default-constructed-std-string",
        r#"
#include <string>

std::string withDefault(const std::string& attr = std::string()) {
    return attr;
}

std::string returnsEmpty() {
    return std::string();
}
"#,
    );

    assert!(
        !source.contains("basic_string()"),
        "expected no literal basic_string() call, got:\n{source}"
    );
    assert!(
        source.contains("String attr = ''"),
        "expected the default parameter value to lower to an empty string literal, got:\n{source}"
    );
    assert!(
        source.contains("return '';"),
        "expected the return statement to lower to an empty string literal, got:\n{source}"
    );
}
