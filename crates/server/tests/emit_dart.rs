//! Pure IR → Dart text tests for `emit::dart` — no `libclang` involved, so
//! these run everywhere `lower_cpp.rs`'s tests can't (no toolchain
//! required).

use std::collections::BTreeMap;

use syntax_bridge_server::emit::dart::emit_module;
use syntax_bridge_server::ir::{
    BinaryOp, Expr, Field, Function, Module, Origin, Param, Record, Stmt, Type,
};

fn origin(line: u32) -> Origin {
    Origin {
        file: "/project/input-source/src/aritmetica.cpp".to_owned(),
        line,
        column: 1,
    }
}

fn soma_function() -> Function {
    Function {
        name: "soma".to_owned(),
        usr: "c:@F@soma#I#I#".to_owned(),
        params: vec![
            Param {
                name: "a".to_owned(),
                ty: Type::Int,
            },
            Param {
                name: "b".to_owned(),
                ty: Type::Int,
            },
        ],
        return_type: Type::Int,
        body: vec![Stmt::Return {
            value: Some(Expr::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(Expr::Ref {
                    name: "a".to_owned(),
                    ty: Type::Int,
                    origin: origin(3),
                }),
                rhs: Box::new(Expr::Ref {
                    name: "b".to_owned(),
                    ty: Type::Int,
                    origin: origin(3),
                }),
                ty: Type::Int,
                origin: origin(3),
            }),
            origin: origin(3),
        }],
        origin: origin(2),
    }
}

#[test]
fn emits_a_free_function_returning_a_binary_expression() {
    let module = Module {
        records: Vec::new(),
        functions: vec![soma_function()],
    };

    let files = emit_module(&module);
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        vec!["lib/aritmetica.dart"]
    );

    let expected = "int soma(int a, int b) {\n  return a + b;\n}\n";
    assert_eq!(files["lib/aritmetica.dart"], expected);
}

#[test]
fn transpiling_twice_produces_byte_identical_output() {
    let module = Module {
        records: Vec::new(),
        functions: vec![soma_function()],
    };

    let first = emit_module(&module);
    let second = emit_module(&module);
    assert_eq!(first, second);
}

#[test]
fn an_unsupported_statement_becomes_a_todo_comment_and_a_throw() {
    let function = Function {
        name: "nao_suportada".to_owned(),
        usr: "c:@F@nao_suportada#".to_owned(),
        params: vec![],
        return_type: Type::Void,
        body: vec![Stmt::Unsupported {
            reason: "unsupported statement cursor kind 207".to_owned(),
            origin: Origin {
                file: "/project/input-source/src/controle.cpp".to_owned(),
                line: 5,
                column: 5,
            },
        }],
        origin: Origin {
            file: "/project/input-source/src/controle.cpp".to_owned(),
            line: 4,
            column: 1,
        },
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
    };

    let files = emit_module(&module);
    let source = &files["lib/controle.dart"];

    assert!(
        source.contains("// TODO(syntax-bridge): unsupported statement cursor kind 207"),
        "missing TODO comment, got:\n{source}"
    );
    assert!(
        source.contains(
            "throw UnimplementedError('/project/input-source/src/controle.cpp:5: unsupported statement cursor kind 207');"
        ),
        "missing throw with origin, got:\n{source}"
    );
}

/// Regression test: `dart_string_literal` escaped `\` and `'` but not `$` —
/// a Dart single-quoted string still interpolates `$identifier`/`${expr}`,
/// so any text embedded in an `Unsupported` message (a project path can
/// legally contain `$` on Linux) could turn into broken/misinterpreted Dart
/// instead of the intended literal text.
#[test]
fn an_unsupported_message_escapes_dollar_signs_so_dart_never_interpolates_it() {
    let function = Function {
        name: "nao_suportada".to_owned(),
        usr: "c:@F@nao_suportada#".to_owned(),
        params: vec![],
        return_type: Type::Void,
        body: vec![Stmt::Unsupported {
            reason: "unsupported statement cursor kind 207".to_owned(),
            origin: Origin {
                file: "/home/user/$HOME/project/src/controle.cpp".to_owned(),
                line: 5,
                column: 5,
            },
        }],
        origin: Origin {
            file: "/project/input-source/src/controle.cpp".to_owned(),
            line: 4,
            column: 1,
        },
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
    };

    let files = emit_module(&module);
    let source = &files["lib/controle.dart"];

    assert!(
        source.contains("\\$HOME"),
        "expected the $ to be escaped so Dart doesn't try to interpolate it, got:\n{source}"
    );
    assert!(
        !source.contains("'/home/user/$HOME"),
        "an unescaped $ right after the opening quote attempts interpolation in Dart, got:\n{source}"
    );
}

#[test]
fn an_unsupported_expression_calls_a_never_returning_helper_that_still_type_checks() {
    let function = Function {
        name: "retorna_desconhecido".to_owned(),
        usr: "c:@F@retorna_desconhecido#".to_owned(),
        params: vec![],
        return_type: Type::Int,
        body: vec![Stmt::Return {
            value: Some(Expr::Unsupported {
                reason: "unsupported expression cursor kind 999".to_owned(),
                origin: origin(6),
            }),
            origin: origin(6),
        }],
        origin: origin(4),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains(
            "return _syntaxBridgeUnsupported('/project/input-source/src/aritmetica.cpp:6: unsupported expression cursor kind 999');"
        ),
        "missing helper call, got:\n{source}"
    );
    assert!(
        source.contains("Never _syntaxBridgeUnsupported(String reason) {"),
        "helper function should be defined when used, got:\n{source}"
    );
}

#[test]
fn the_unsupported_helper_is_omitted_when_nothing_needs_it() {
    let module = Module {
        records: Vec::new(),
        functions: vec![soma_function()],
    };

    let files = emit_module(&module);
    assert!(
        !files["lib/aritmetica.dart"].contains("_syntaxBridgeUnsupported"),
        "helper should not appear in a file that never uses it"
    );
}

#[test]
fn functions_from_different_source_files_land_in_different_dart_files() {
    let mut second = soma_function();
    second.name = "outra".to_owned();
    second.origin.file = "/project/input-source/src/outra.cpp".to_owned();
    for stmt in &mut second.body {
        if let Stmt::Return {
            value: Some(Expr::Binary { origin, .. }),
            ..
        } = stmt
        {
            origin.file = "/project/input-source/src/outra.cpp".to_owned();
        }
    }

    let module = Module {
        records: Vec::new(),
        functions: vec![soma_function(), second],
    };

    let files: BTreeMap<String, String> = emit_module(&module);
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        vec!["lib/aritmetica.dart", "lib/outra.dart"]
    );
}

/// Regression test: a function whose parameter type isn't representable
/// (e.g. C++ `long`) used to still emit its body as normal, live Dart —
/// `dynamic` params feeding real arithmetic, running and silently producing
/// wrong values instead of throwing. The whole function must bail out, the
/// same way a `Stmt::Unsupported` anywhere in the body already does.
#[test]
fn a_function_with_an_unsupported_parameter_type_throws_instead_of_running_its_body() {
    let function = Function {
        name: "dividir".to_owned(),
        usr: "c:@F@dividir#l#l#".to_owned(),
        params: vec![
            Param {
                name: "a".to_owned(),
                ty: Type::Unsupported("long".to_owned()),
            },
            Param {
                name: "b".to_owned(),
                ty: Type::Int,
            },
        ],
        return_type: Type::Int,
        body: vec![Stmt::Return {
            value: Some(Expr::Ref {
                name: "a".to_owned(),
                ty: Type::Unsupported("long".to_owned()),
                origin: origin(3),
            }),
            origin: origin(3),
        }],
        origin: origin(2),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("throw UnimplementedError("),
        "expected the whole function to bail out, got:\n{source}"
    );
    assert!(
        source.contains("long"),
        "expected the unsupported spelling in the message, got:\n{source}"
    );
    assert!(
        !source.contains("return a;"),
        "the original (wrong) body must not be emitted, got:\n{source}"
    );
}

/// Same failure mode as the parameter case above, but for the return type.
#[test]
fn a_function_with_an_unsupported_return_type_throws_instead_of_running_its_body() {
    let function = Function {
        name: "cria_id".to_owned(),
        usr: "c:@F@cria_id#".to_owned(),
        params: vec![],
        return_type: Type::Unsupported("unsigned long".to_owned()),
        body: vec![Stmt::Return {
            value: Some(Expr::IntLiteral {
                value: 1,
                origin: origin(3),
            }),
            origin: origin(3),
        }],
        origin: origin(2),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("throw UnimplementedError("),
        "expected the whole function to bail out, got:\n{source}"
    );
    assert!(
        !source.contains("return 1;"),
        "the original body must not be emitted, got:\n{source}"
    );
}

/// Regression test: a `struct`/`class` with a field type that isn't
/// representable used to still emit a normal, constructible Dart class —
/// silently accepting any value into an untyped (`dynamic`) field with no
/// signal the shape is incomplete. The constructor must throw instead.
#[test]
fn a_record_with_an_unsupported_field_type_has_a_throwing_constructor() {
    let record = Record {
        name: "Ponto3D".to_owned(),
        usr: "c:@S@Ponto3D".to_owned(),
        fields: vec![
            Field {
                name: "x".to_owned(),
                ty: Type::Double,
            },
            Field {
                name: "peso".to_owned(),
                ty: Type::Unsupported("long double".to_owned()),
            },
        ],
        origin: origin(1),
    };
    let module = Module {
        records: vec![record],
        functions: vec![],
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("class Ponto3D {"),
        "expected the class itself to still be declared, got:\n{source}"
    );
    assert!(
        source.contains("Ponto3D(this.x, this.peso) {"),
        "expected the constructor to still accept both fields, got:\n{source}"
    );
    assert!(
        source.contains("throw UnimplementedError("),
        "expected the constructor to throw instead of silently constructing, got:\n{source}"
    );
    assert!(
        source.contains("long double"),
        "expected the unsupported spelling in the message, got:\n{source}"
    );
}

/// Regression test: `emit_binary_op`'s truncating-division rule only checks
/// whether the node's own `ty` is `Type::Int`; when it's `Unsupported`
/// instead (e.g. C++'s usual arithmetic conversions promoted `int / long`
/// to `long`), the old code fell through to plain `/` — silently wrong,
/// since C++ integer division truncates regardless of width. The function's
/// signature here is fully representable (`int` in, `int` out), so this
/// specifically exercises the *body* scan, not the signature bail-out
/// `a_function_with_an_unsupported_parameter_type_throws_instead_of_running_its_body`
/// already covers.
#[test]
fn a_binary_expression_with_an_unsupported_result_type_bails_out_the_whole_function() {
    let function = Function {
        name: "media_estranha".to_owned(),
        usr: "c:@F@media_estranha#I#".to_owned(),
        params: vec![Param {
            name: "a".to_owned(),
            ty: Type::Int,
        }],
        return_type: Type::Int,
        body: vec![Stmt::Return {
            value: Some(Expr::Binary {
                op: BinaryOp::Div,
                lhs: Box::new(Expr::Ref {
                    name: "a".to_owned(),
                    ty: Type::Int,
                    origin: origin(3),
                }),
                rhs: Box::new(Expr::IntLiteral {
                    value: 2,
                    origin: origin(3),
                }),
                ty: Type::Unsupported("long".to_owned()),
                origin: origin(3),
            }),
            origin: origin(3),
        }],
        origin: origin(2),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        !source.contains("return a / 2;"),
        "the plain-`/` body must not be emitted for an unsupported result type, got:\n{source}"
    );
    assert!(
        source.contains("throw UnimplementedError("),
        "expected the whole function to bail out, got:\n{source}"
    );
}

/// Regression test: a local variable declared with an unsupported type
/// (e.g. `long b = 10;`) used to be emitted as `dynamic b = 10;` with the
/// rest of the body running as normal, live Dart on top of it.
#[test]
fn a_local_variable_with_an_unsupported_type_bails_out_the_whole_function() {
    let function = Function {
        name: "com_local_estranho".to_owned(),
        usr: "c:@F@com_local_estranho#I#".to_owned(),
        params: vec![Param {
            name: "a".to_owned(),
            ty: Type::Int,
        }],
        return_type: Type::Int,
        body: vec![
            Stmt::VarDecl {
                name: "b".to_owned(),
                ty: Type::Unsupported("long".to_owned()),
                init: Some(Expr::IntLiteral {
                    value: 10,
                    origin: origin(3),
                }),
                origin: origin(3),
            },
            Stmt::Return {
                value: Some(Expr::Ref {
                    name: "a".to_owned(),
                    ty: Type::Int,
                    origin: origin(4),
                }),
                origin: origin(4),
            },
        ],
        origin: origin(2),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        !source.contains("dynamic /* unsupported: long */ b = 10;"),
        "the untyped local declaration must not be emitted as live code, got:\n{source}"
    );
    assert!(
        source.contains("throw UnimplementedError("),
        "expected the whole function to bail out, got:\n{source}"
    );
}
