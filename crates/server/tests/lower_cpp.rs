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

/// Achado 4 (`docs/plans/diagnostico-verovio-6.2.0.md`): a stdlib container
/// with no E05 adapter (`basic_string`/`vector`/`list`/`set`/`map` are the
/// only ones — `std::array` here has none) used to fall through
/// `lower_type`'s `CXType_Record`/`CXType_Unexposed` branch the same way a
/// project-defined class does: `clang_getTypeDeclaration` resolves fine (a
/// real `array<int, 3>` decl, real usr, real name `array`), so the old code
/// returned `Type::Record { usr, name: "array" }` pointing at a class this
/// project never declares. That prints as a bare, undefined type reference
/// in the emitted Dart (`array a`, per the same failure mode already fixed
/// for an external enum just above) rather than an honest bailout — worse
/// than a stub, because nothing in the emitted line itself flags it as
/// untranslated.
#[test]
fn a_stdlib_container_without_an_adapter_becomes_unsupported_not_an_undeclared_record() {
    let source = lower_and_emit(
        "lower-cpp-stdlib-no-adapter",
        r#"
#include <array>
void f(std::array<int, 3> a) { }
"#,
    );

    assert!(
        !source.contains("array a"),
        "a stdlib container without an adapter must not be named as a bare Dart parameter type, got:\n{source}"
    );
    assert!(
        source.contains("UnimplementedError"),
        "the function should bail out loudly instead, got:\n{source}"
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
    // diagnostic string, not a Dart identifier, the same distinction
    // achado 5's `dynamic /* unsupported: T* */` already draws.
    assert!(
        source.contains("dynamic /* unsupported:") && source.contains("(unnamed struct at"),
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
