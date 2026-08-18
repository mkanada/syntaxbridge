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

#[test]
fn an_unsupported_statement_becomes_an_unsupported_node_with_origin_and_reason() {
    // `break;` (E02 already lowers `while`/`for` themselves, but not the
    // loop-control statements inside them) — still genuinely unsupported,
    // unlike the `DeclStmt`/`WhileStmt` this test used before E02 (PR4)
    // implemented both.
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

    let Stmt::Unsupported { reason, origin } = &then_branch[0] else {
        panic!(
            "expected `break;` to be Unsupported, got {:?}",
            then_branch[0]
        );
    };
    assert!(!reason.is_empty());
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
    let workspace = TempWorkspace::new(name).expect("create temporary workspace");
    let unit = write_fixture(workspace.path(), source, "probe.cpp");
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
/// length. An unrelated `void*` must remain an explicit bailout instead of
/// being silently guessed as bytes.
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
        source.contains("void keep_opaque(SyntaxBridgeOpaque"),
        "an unclassified void pointer must stay an explicit bailout, got:\n{source}"
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
fn a_stdlib_operator_call_uses_its_first_argument_as_the_receiver() {
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
        source.contains("unsupported std::vector::operator= call")
            && !source.contains("standard-library method call's first child was not"),
        "an operator-style stdlib call must expose its receiver before dispatch, got:\n{source}"
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
/// synthesized, always-valid method name and bails the body out loudly
/// (`// TODO(syntax-bridge)` + `throw UnimplementedError`, "silêncio é
/// proibido") instead of pretending the translation succeeded.
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
        source.contains("TODO(syntax-bridge)") && source.contains("UnimplementedError"),
        "the body must bail out loudly instead of silently dropping the semantics, got:\n{source}"
    );
}

/// A *free* operator overload with no Dart equivalent (`operator<<`, C++'s
/// idiomatic stream-insertion overload) reaches the same call-lowering path
/// as `std::string`'s `operator+`/`operator==` (E13) but isn't one of the
/// ones that path recognizes — before this fix, the fallback built an
/// ordinary `Expr::Call` naming the callee `operator<<`, and `emit::dart`
/// printed it verbatim: `operator<<(a, 2)`, a bare invalid Dart identifier
/// used as a call target.
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
