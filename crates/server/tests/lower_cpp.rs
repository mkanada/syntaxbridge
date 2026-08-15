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
