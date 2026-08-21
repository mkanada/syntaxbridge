//! Pure IR → Dart text tests for `emit::dart` — no `libclang` involved, so
//! these run everywhere `lower_cpp.rs`'s tests can't (no toolchain
//! required).

use std::collections::BTreeMap;

use std::collections::HashSet;

use syntax_bridge_server::emit::dart::{emit_module, emit_module_with_externals};
use syntax_bridge_server::ir::{
    BaseClass, BinaryOp, Constructor, Enum, Expr, Field, Function, Method, Module, Origin, Param,
    Record, Stmt, Type,
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
                default_value: None,
            },
            Param {
                name: "b".to_owned(),
                ty: Type::Int,
                default_value: None,
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
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    assert_eq!(
        files.keys().collect::<Vec<_>>(),
        vec!["lib/aritmetica.dart"]
    );

    let expected = "int soma(int a, int b) {\n  return a + b;\n}\n";
    assert_eq!(files["lib/aritmetica.dart"], expected);
}

/// `Type::Nullable(Type::Str)` — `lower::cpp::lower_type`'s new case 3
/// answer (`docs/plans/verovio-6.2-pointer-types.md`) for a raw
/// `char*`/`const char*`. `emit_type`'s `Nullable` arm is already generic
/// (`format!("{}?", emit_type(inner))`), so this only needs to confirm
/// `Str` inside it prints `String?` rather than assume it from reading the
/// code.
#[test]
fn a_nullable_str_return_type_emits_as_a_nullable_dart_string() {
    let rotulo = Function {
        name: "Rotulo".to_owned(),
        usr: "c:@F@Rotulo#".to_owned(),
        params: Vec::new(),
        return_type: Type::Nullable(Box::new(Type::Str)),
        body: vec![Stmt::Return {
            value: None,
            origin: origin(2),
        }],
        origin: origin(2),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![rotulo],
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    assert!(
        files
            .values()
            .any(|source| source.contains("String? Rotulo(")),
        "expected a `String? Rotulo(...)` signature, got:\n{files:?}"
    );
}

/// `Type::Set`/`Type::Map` — caso 5 of
/// `docs/plans/verovio-6.2-pointer-types.md`, the same "prove the generic
/// arm actually does the right thing" reasoning as the `Nullable(Str)`
/// test just above.
#[test]
fn set_and_map_types_emit_as_their_dart_core_equivalents() {
    let membros = Function {
        name: "Membros".to_owned(),
        usr: "c:@F@Membros#".to_owned(),
        params: Vec::new(),
        return_type: Type::Set(Box::new(Type::Int)),
        body: vec![Stmt::Return {
            value: None,
            origin: origin(2),
        }],
        origin: origin(2),
    };
    let opcoes = Function {
        name: "Opcoes".to_owned(),
        usr: "c:@F@Opcoes#".to_owned(),
        params: Vec::new(),
        return_type: Type::Map(Box::new(Type::Str), Box::new(Type::Int)),
        body: vec![Stmt::Return {
            value: None,
            origin: origin(3),
        }],
        origin: origin(3),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![membros, opcoes],
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    assert!(
        files
            .values()
            .any(|source| source.contains("Set<int> Membros(")),
        "expected a `Set<int> Membros(...)` signature, got:\n{files:?}"
    );
    assert!(
        files
            .values()
            .any(|source| source.contains("Map<String, int> Opcoes(")),
        "expected a `Map<String, int> Opcoes(...)` signature, got:\n{files:?}"
    );
}

/// Caso 4 of `docs/plans/verovio-6.2-pointer-types.md`: `Module::enums`
/// actually reaches Dart source, not just `Type::Enum`'s type-annotation
/// text (already covered indirectly by every `Type::Enum` match arm) —
/// without this, a function returning/taking an enum would reference a
/// Dart type that's never declared anywhere in the emitted package,
/// `dart analyze`'s `undefined_class` territory.
///
/// Every enumerator carries its real C++ value explicitly (`(0)`, `(1)`,
/// `(2)` here) rather than relying on Dart's own `.index` — see
/// `ir::Enum::values`'s doc comment for why: C++ enumerators aren't
/// guaranteed 0-based/sequential/gapless, so `.index` alone would silently
/// disagree with the C++ program for any enum that isn't.
#[test]
fn an_enum_emits_as_a_dart_enum_declaration_with_its_real_cpp_values() {
    let cor = Enum {
        name: "Cor".to_owned(),
        usr: "c:@E@Cor".to_owned(),
        variants: vec!["Vermelho".to_owned(), "Verde".to_owned(), "Azul".to_owned()],
        values: vec![0, 1, 2],
        origin: origin(2),
    };
    let module = Module {
        records: Vec::new(),
        functions: Vec::new(),
        enums: vec![cor],
    };

    let files = emit_module(&module);
    assert!(
        files.values().any(|source| {
            source.contains("enum Cor")
                && source.contains("Vermelho(0)")
                && source.contains("Verde(1)")
                && source.contains("Azul(2)")
                && source.contains("const Cor(this.value)")
                && source.contains("final int value")
        }),
        "expected an enum declaration with explicit backing values, got:\n{files:?}"
    );
}

#[test]
fn transpiling_twice_produces_byte_identical_output() {
    let module = Module {
        records: Vec::new(),
        functions: vec![soma_function()],
        enums: Vec::new(),
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
        enums: Vec::new(),
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
        enums: Vec::new(),
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

/// C++ string literals may contain a physical newline through an escaped
/// source sequence. Dart single-quoted literals cannot: writing it literally
/// leaves the generated package unparsable. Keep every control character in
/// its escaped Dart spelling instead.
#[test]
fn a_string_literal_escapes_control_characters_so_the_dart_file_stays_parseable() {
    let function = Function {
        name: "mensagem".to_owned(),
        usr: "c:@F@mensagem#".to_owned(),
        params: vec![],
        return_type: Type::Str,
        body: vec![Stmt::Return {
            value: Some(Expr::StringLiteral {
                value: "primeira\nsegunda\r\tterceira".to_owned(),
                origin: origin(5),
            }),
            origin: origin(5),
        }],
        origin: origin(4),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
        enums: Vec::new(),
    };

    let source = &emit_module(&module)["lib/aritmetica.dart"];

    assert!(
        source.contains("return 'primeira\\nsegunda\\r\\tterceira';"),
        "control characters must be escaped in a Dart literal, got:\n{source}"
    );
    assert!(
        !source.contains("primeira\nsegunda"),
        "a physical newline must never occur inside the Dart literal, got:\n{source}"
    );
}

/// A bailout expression still always throws at runtime, but it must not emit
/// `dynamic`. Real generated code can keep traversing the syntactic expression
/// around that bailout (`unsupported().x` or `unsupported().method()`), so the
/// helper is generic and receives an explicit, named opaque bridge type until
/// lowering can preserve the source expression's exact type.
#[test]
fn an_unsupported_expression_calls_a_typed_helper_without_dynamic() {
    let function = Function {
        name: "retorna_desconhecido".to_owned(),
        usr: "c:@F@retorna_desconhecido#".to_owned(),
        params: vec![],
        return_type: Type::Int,
        body: vec![Stmt::Return {
            value: Some(Expr::FieldAccess {
                target: Box::new(Expr::Unsupported {
                    reason: "unsupported expression cursor kind 999".to_owned(),
                    origin: origin(6),
                }),
                field: "value".to_owned(),
                ty: Type::Int,
                origin: origin(6),
            }),
            origin: origin(6),
        }],
        origin: origin(4),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains(
            "return _syntaxBridgeUnsupported<SyntaxBridgeOpaque>('/project/input-source/src/aritmetica.cpp:6: unsupported expression cursor kind 999').value;"
        ),
        "missing helper call, got:\n{source}"
    );
    assert!(
        source.contains("T _syntaxBridgeUnsupported<T>(String reason) {"),
        "helper function should preserve an explicit static type, got:\n{source}"
    );
    assert!(
        source.contains("final class SyntaxBridgeOpaque {"),
        "unsupported types need a named bridge declaration, got:\n{source}"
    );
    assert!(
        !source.contains("dynamic"),
        "an unsupported expression must not reintroduce dynamic, got:\n{source}"
    );
}

#[test]
fn a_typed_unsupported_expression_preserves_its_static_dart_type() {
    let function = Function {
        name: "comprimento_desconhecido".to_owned(),
        usr: "c:@F@comprimento_desconhecido#".to_owned(),
        params: vec![],
        return_type: Type::Int,
        body: vec![Stmt::Return {
            value: Some(Expr::FieldAccess {
                target: Box::new(Expr::UnsupportedTyped {
                    reason: "unsupported expression cursor kind 998".to_owned(),
                    ty: Type::Str,
                    origin: origin(6),
                }),
                field: "length".to_owned(),
                ty: Type::Int,
                origin: origin(6),
            }),
            origin: origin(6),
        }],
        origin: origin(4),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function],
        enums: Vec::new(),
    };

    let source = &emit_module(&module)["lib/aritmetica.dart"];
    assert!(
        source.contains(
            "return _syntaxBridgeUnsupported<String>('/project/input-source/src/aritmetica.cpp:6: unsupported expression cursor kind 998').length;"
        ),
        "the bailout must retain String as its receiver type, got:\n{source}"
    );
    assert!(
        !source.contains("SyntaxBridgeOpaque") && !source.contains("dynamic"),
        "a known type must not fall back to the opaque bridge or dynamic, got:\n{source}"
    );
}

#[test]
fn the_unsupported_helper_is_omitted_when_nothing_needs_it() {
    let module = Module {
        records: Vec::new(),
        functions: vec![soma_function()],
        enums: Vec::new(),
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
        enums: Vec::new(),
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
                default_value: None,
            },
            Param {
                name: "b".to_owned(),
                ty: Type::Int,
                default_value: None,
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
        enums: Vec::new(),
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
    assert!(
        source.contains("SyntaxBridgeOpaque /* unsupported: long */ a"),
        "an unsupported source type needs the named opaque bridge, got:\n{source}"
    );
    assert!(
        source.contains("final class SyntaxBridgeOpaque {") && !source.contains("dynamic"),
        "the generated signature must not fall back to dynamic, got:\n{source}"
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
        enums: Vec::new(),
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
        namespace: String::new(),
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
        static_fields: Vec::new(),
        constructors: Vec::new(),
        methods: Vec::new(),
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(1),
    };
    let module = Module {
        records: vec![record],
        functions: vec![],
        enums: Vec::new(),
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
            default_value: None,
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
        enums: Vec::new(),
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
            default_value: None,
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
        enums: Vec::new(),
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

/// `mapping::pointer_options_for` case A10 (`docs/mapping-solver-cases.md`):
/// `lower::cpp::lower_type` maps a pointer-to-known-record to
/// `Type::Nullable`, C++'s own guarantee that it's either null or a real
/// object. But C++ itself never requires a null check to dereference a
/// pointer (`p->x` compiles either way), so a field/method access or a
/// field assignment through such a value must be asserted non-null (`!`)
/// in the emitted Dart — without it, `dart analyze` rejects the access
/// outright (`unchecked_use_of_nullable_value`), turning a type-safety
/// improvement into new breakage instead of less.
#[test]
fn field_and_method_access_through_a_nullable_pointer_gets_a_non_null_assertion() {
    use syntax_bridge_server::ir::{Constructor, Method};

    let nota_usr = "c:@S@Nota".to_owned();
    let nota = Record {
        name: "Nota".to_owned(),
        usr: nota_usr.clone(),
        namespace: String::new(),
        fields: vec![Field {
            name: "altura".to_owned(),
            ty: Type::Int,
        }],
        static_fields: Vec::new(),
        constructors: Vec::new(),
        methods: Vec::new(),
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(2),
    };
    let nota_ty = || Type::Record {
        usr: nota_usr.clone(),
        name: "Nota".to_owned(),
    };
    let this_atual = || Expr::FieldAccess {
        target: Box::new(Expr::This {
            ty: Type::Void,
            origin: origin(5),
        }),
        field: "_m_atual".to_owned(),
        ty: Type::Nullable(Box::new(nota_ty())),
        origin: origin(5),
    };

    let editor = Record {
        name: "Editor".to_owned(),
        usr: "c:@S@Editor".to_owned(),
        namespace: String::new(),
        fields: vec![Field {
            name: "_m_atual".to_owned(),
            ty: Type::Nullable(Box::new(nota_ty())),
        }],
        static_fields: Vec::new(),
        constructors: vec![Constructor {
            usr: "c:@S@Editor@F@Editor#".to_owned(),
            constructor_index: 0,
            params: Vec::new(),
            body: Vec::new(),
            origin: origin(4),
        }],
        methods: vec![
            // `altura = m_atual->altura;` — a field read through the
            // pointer, as a plain expression statement.
            Method {
                name: "AlturaAtual".to_owned(),
                usr: "c:@S@Editor@F@AlturaAtual#".to_owned(),
                params: Vec::new(),
                return_type: Type::Int,
                body: Some(vec![Stmt::Return {
                    value: Some(Expr::FieldAccess {
                        target: Box::new(this_atual()),
                        field: "altura".to_owned(),
                        ty: Type::Int,
                        origin: origin(6),
                    }),
                    origin: origin(6),
                }]),
                is_static: false,
                is_override: false,
                origin: origin(6),
            },
            // `m_atual->altura = valor;` — a field write through the
            // pointer.
            Method {
                name: "DefinirAltura".to_owned(),
                usr: "c:@S@Editor@F@DefinirAltura#I#".to_owned(),
                params: vec![Param {
                    name: "valor".to_owned(),
                    ty: Type::Int,
                    default_value: None,
                }],
                return_type: Type::Void,
                body: Some(vec![Stmt::FieldAssign {
                    target: this_atual(),
                    field: "altura".to_owned(),
                    value: Expr::Ref {
                        name: "valor".to_owned(),
                        ty: Type::Int,
                        origin: origin(7),
                    },
                    origin: origin(7),
                }]),
                is_static: false,
                is_override: false,
                origin: origin(7),
            },
        ],
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(3),
    };

    let module = Module {
        records: vec![nota, editor],
        functions: Vec::new(),
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("return _m_atual!.altura;"),
        "expected a non-null assertion on the field read, got:\n{source}"
    );
    assert!(
        source.contains("_m_atual!.altura = valor;"),
        "expected a non-null assertion on the field write, got:\n{source}"
    );
    assert!(
        source.contains("Nota? _m_atual"),
        "expected the field itself to keep its nullable type, got:\n{source}"
    );
}

fn nota_ref_ty() -> Type {
    Type::Nullable(Box::new(Type::Record {
        usr: "c:@S@Nota".to_owned(),
        name: "Nota".to_owned(),
    }))
}

fn nullable_ref(name: &str, line: u32) -> Expr {
    Expr::Ref {
        name: name.to_owned(),
        ty: nota_ref_ty(),
        origin: origin(line),
    }
}

fn altura_read(receiver: Expr, line: u32) -> Expr {
    Expr::FieldAccess {
        target: Box::new(receiver),
        field: "altura".to_owned(),
        ty: Type::Int,
        origin: origin(line),
    }
}

fn int_var_decl(name: &str, init: Expr, line: u32) -> Stmt {
    Stmt::VarDecl {
        name: name.to_owned(),
        ty: Type::Int,
        init: Some(init),
        origin: origin(line),
    }
}

/// Dart's flow-sensitive type promotion: once a local/parameter has been
/// forced non-null with `!`, Dart's own analyzer treats every later read of
/// that *same* local as already non-null and flags a repeated `!` as
/// `unnecessary_non_null_assertion` — `receiver_bang` used to decide the `!`
/// purely from the receiver's static type, with no notion of "already
/// checked this flow", so it repeated the `!` on every dereference (real
/// Verovio 6.2.0 diagnostic evidence: 6107 occurrences across 77 files, see
/// `docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`).
#[test]
fn a_promoted_pointer_parameter_skips_the_redundant_non_null_assertion_on_later_reads() {
    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#*$@S@Nota#*$@S@Nota#".to_owned(),
        params: vec![
            Param {
                name: "a".to_owned(),
                ty: nota_ref_ty(),
                default_value: None,
            },
            Param {
                name: "b".to_owned(),
                ty: nota_ref_ty(),
                default_value: None,
            },
        ],
        return_type: Type::Void,
        body: vec![
            int_var_decl("x1", altura_read(nullable_ref("a", 3), 3), 3),
            int_var_decl("x2", altura_read(nullable_ref("a", 4), 4), 4),
            int_var_decl("y1", altura_read(nullable_ref("b", 5), 5), 5),
            int_var_decl("y2", altura_read(nullable_ref("b", 6), 6), 6),
        ],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    let expected_body = "\
  int x1 = a!.altura;
  int x2 = a.altura;
  int y1 = b!.altura;
  int y2 = b.altura;
";
    assert!(
        source.contains(expected_body),
        "expected only the first read of each parameter to carry '!', got:\n{source}"
    );
}

/// The safety half of the same rule: an assignment invalidates the
/// promotion, since Dart's own analyzer can no longer prove the new value is
/// non-null (`docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`'s
/// own safety criterion — removing a `!` Dart would still require is a
/// compile error, not a warning).
#[test]
fn reassigning_a_promoted_pointer_brings_the_non_null_assertion_back() {
    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#*$@S@Nota#".to_owned(),
        params: vec![Param {
            name: "a".to_owned(),
            ty: nota_ref_ty(),
            default_value: None,
        }],
        return_type: Type::Void,
        body: vec![
            int_var_decl("x1", altura_read(nullable_ref("a", 3), 3), 3),
            Stmt::Assign {
                name: "a".to_owned(),
                value: Expr::NullLiteral { origin: origin(4) },
                origin: origin(4),
            },
            int_var_decl("x2", altura_read(nullable_ref("a", 5), 5), 5),
        ],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    let expected_body = "\
  int x1 = a!.altura;
  a = null;
  int x2 = a!.altura;
";
    assert!(
        source.contains(expected_body),
        "expected the '!' to return after `a` is reassigned, got:\n{source}"
    );
}

/// Dart never promotes a field access (`this._m_x`, `obj.field`) — unlike a
/// local/parameter, another piece of code could reassign it between a check
/// and a later use, so every field read keeps its own `!` regardless of how
/// many times the same field is read in the same body.
#[test]
fn a_field_access_never_promotes_even_when_read_twice_in_the_same_body() {
    use syntax_bridge_server::ir::{Constructor, Method};

    let nota = Record {
        name: "Nota".to_owned(),
        usr: "c:@S@Nota".to_owned(),
        namespace: String::new(),
        fields: vec![Field {
            name: "altura".to_owned(),
            ty: Type::Int,
        }],
        static_fields: Vec::new(),
        constructors: Vec::new(),
        methods: Vec::new(),
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(2),
    };

    let this_atual = |line: u32| Expr::FieldAccess {
        target: Box::new(Expr::This {
            ty: Type::Void,
            origin: origin(line),
        }),
        field: "_m_atual".to_owned(),
        ty: nota_ref_ty(),
        origin: origin(line),
    };

    let editor = Record {
        name: "Editor".to_owned(),
        usr: "c:@S@Editor".to_owned(),
        namespace: String::new(),
        fields: vec![Field {
            name: "_m_atual".to_owned(),
            ty: nota_ref_ty(),
        }],
        static_fields: Vec::new(),
        constructors: vec![Constructor {
            usr: "c:@S@Editor@F@Editor#".to_owned(),
            constructor_index: 0,
            params: Vec::new(),
            body: Vec::new(),
            origin: origin(4),
        }],
        methods: vec![Method {
            name: "SomaAltura".to_owned(),
            usr: "c:@S@Editor@F@SomaAltura#".to_owned(),
            params: Vec::new(),
            return_type: Type::Int,
            body: Some(vec![
                int_var_decl("x1", altura_read(this_atual(6), 6), 6),
                Stmt::Return {
                    value: Some(altura_read(this_atual(7), 7)),
                    origin: origin(7),
                },
            ]),
            is_static: false,
            is_override: false,
            origin: origin(6),
        }],
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(3),
    };

    let module = Module {
        records: vec![nota, editor],
        functions: Vec::new(),
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert_eq!(
        source.matches("_m_atual!.altura").count(),
        2,
        "expected both field reads to keep '!' since Dart never promotes a field, got:\n{source}"
    );
}

/// A `x != null` conjunct earlier in the same `&&` chain promotes `x` for
/// the rest of that chain *and* for the `if`'s `then` branch — real Verovio
/// evidence: `.diagnosis/dart-package/lib/accid.dart:154`,
/// `element!.IsClassId(…) && chord != null && chord!.HasAdjacentNotesInStaff(staff)`.
#[test]
fn a_null_check_conjunct_promotes_the_rest_of_the_and_chain_and_the_then_branch() {
    let has_notes = |line: u32| Expr::Call {
        target: Some(Box::new(nullable_ref("chord", line))),
        callee_usr: "c:@S@Nota@F@HasNotes#".to_owned(),
        callee_name: "HasNotes".to_owned(),
        args: Vec::new(),
        ty: Type::Bool,
        origin: origin(line),
    };
    let other_thing = |line: u32| Expr::Call {
        target: Some(Box::new(nullable_ref("chord", line))),
        callee_usr: "c:@S@Nota@F@OtherThing#".to_owned(),
        callee_name: "OtherThing".to_owned(),
        args: Vec::new(),
        ty: Type::Void,
        origin: origin(line),
    };

    let condicao = Expr::Binary {
        op: BinaryOp::And,
        lhs: Box::new(Expr::Binary {
            op: BinaryOp::Ne,
            lhs: Box::new(nullable_ref("chord", 4)),
            rhs: Box::new(Expr::NullLiteral { origin: origin(4) }),
            ty: Type::Bool,
            origin: origin(4),
        }),
        rhs: Box::new(has_notes(4)),
        ty: Type::Bool,
        origin: origin(4),
    };

    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#".to_owned(),
        params: Vec::new(),
        return_type: Type::Void,
        body: vec![
            Stmt::VarDecl {
                name: "chord".to_owned(),
                ty: nota_ref_ty(),
                init: Some(Expr::Call {
                    target: None,
                    callee_usr: "c:@F@GetFirstAncestor#".to_owned(),
                    callee_name: "GetFirstAncestor".to_owned(),
                    args: Vec::new(),
                    ty: nota_ref_ty(),
                    origin: origin(3),
                }),
                origin: origin(3),
            },
            Stmt::If {
                condition: condicao,
                then_branch: vec![Stmt::ExprStmt {
                    expr: other_thing(5),
                    origin: origin(5),
                }],
                else_branch: Vec::new(),
                origin: origin(4),
            },
        ],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("if (chord != null && chord.HasNotes())"),
        "expected the second conjunct's '!' to be dropped once `chord != null` already proved it, got:\n{source}"
    );
    assert!(
        source.contains("chord.OtherThing();"),
        "expected the then-branch to inherit the condition's promotion, got:\n{source}"
    );
}

/// A `&&`'s right operand only runs when the left operand is `true` — a bang
/// inside it must not promote anything for code *after* the whole `&&`,
/// since that code is also reached along the path where the left operand
/// was `false` and the right operand never ran at all. Real Verovio
/// regression: `if (!flag && !(pos!.Foo())) { continue; }` followed a few
/// lines later by an unguarded `pos.Bar()` — the first emitter draft merged
/// `pos`'s bang from inside the `&&`'s right operand back into the ambient
/// promoted set regardless of whether that operand ever ran, producing
/// `unchecked_use_of_nullable_value` (381 → 1080 occurrences over the real
/// Verovio 6.2.0 corpus, `just verovio-diagnosis`).
/// `&&` binds tighter than `||` in both C++ and Dart — printing a `||` node
/// as a bare, unparenthesized child of an `&&` node silently changes which
/// operand short-circuits which (`x && (a || b)` vs. the unparenthesized
/// `x && a || b`, which Dart reads as `(x && a) || b`, a different boolean
/// altogether). This is a real, pre-existing correctness bug independent of
/// nullability — but it also breaks the promotion tracking's own soundness:
/// `docs/prompts/2026-08-21-04-bang-redundante-e-promocao.md`'s own evidence
/// trail — Verovio's `view_page.cpp`, `reh && ((reh->HasTstamp() &&
/// reh->GetTstamp() == 0) || (reh->GetStart()->Is(BARLINE) && ...))` emitted
/// with the `||` unparenthesized let a bang inside the left `&&` chain
/// (legitimately promoting `reh` under the *real* tree, where the whole
/// `||` is the right-hand side of the outer `&&`) leak into the `||`'s own
/// right operand — correct under the real tree, but `dart analyze` parses
/// the malformed text with the *other* grouping, where that promotion
/// doesn't hold, and flags `unchecked_use_of_nullable_value`.
#[test]
fn an_or_nested_inside_an_and_is_parenthesized_so_dart_reads_the_same_tree_this_module_does() {
    let has_tstamp = Expr::Call {
        target: Some(Box::new(nullable_ref("reh", 4))),
        callee_usr: "c:@S@Nota@F@HasTstamp#".to_owned(),
        callee_name: "HasTstamp".to_owned(),
        args: Vec::new(),
        ty: Type::Bool,
        origin: origin(4),
    };
    let get_start = Expr::Call {
        target: Some(Box::new(nullable_ref("reh", 4))),
        callee_usr: "c:@S@Nota@F@GetStart#".to_owned(),
        callee_name: "GetStart".to_owned(),
        args: Vec::new(),
        ty: nota_ref_ty(),
        origin: origin(4),
    };
    let is_class_id = Expr::Call {
        target: Some(Box::new(get_start)),
        callee_usr: "c:@S@Nota@F@IsClassId#".to_owned(),
        callee_name: "IsClassId".to_owned(),
        args: Vec::new(),
        ty: Type::Bool,
        origin: origin(4),
    };

    let a = Expr::Binary {
        op: BinaryOp::And,
        lhs: Box::new(has_tstamp),
        rhs: Box::new(is_class_id.clone()),
        ty: Type::Bool,
        origin: origin(4),
    };
    // `reh` used as a bare truthy pointer check (`if (reh && ...)`), the
    // C++ shape that actually triggered the real regression — it lowers to
    // `Expr::Convert{ty: Bool}`, not `Expr::Binary{Ne}`, so it renders as
    // `reh != null` without ever registering as a structural null check for
    // `and_chain_null_check_names`.
    let reh_truthy = Expr::Convert {
        operand: Box::new(nullable_ref("reh", 4)),
        ty: Type::Bool,
        origin: origin(4),
    };
    let condicao = Expr::Binary {
        op: BinaryOp::And,
        lhs: Box::new(reh_truthy),
        rhs: Box::new(Expr::Binary {
            op: BinaryOp::Or,
            lhs: Box::new(a),
            rhs: Box::new(is_class_id),
            ty: Type::Bool,
            origin: origin(4),
        }),
        ty: Type::Bool,
        origin: origin(4),
    };
    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#".to_owned(),
        params: Vec::new(),
        return_type: Type::Bool,
        body: vec![
            Stmt::VarDecl {
                name: "reh".to_owned(),
                ty: nota_ref_ty(),
                init: Some(Expr::Call {
                    target: None,
                    callee_usr: "c:@F@Find#".to_owned(),
                    callee_name: "Find".to_owned(),
                    args: Vec::new(),
                    ty: nota_ref_ty(),
                    origin: origin(3),
                }),
                origin: origin(3),
            },
            Stmt::Return {
                value: Some(condicao),
                origin: origin(4),
            },
        ],
        origin: origin(2),
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains(
            "return reh != null && (reh!.HasTstamp() && reh.GetStart()!.IsClassId() \
             || reh.GetStart()!.IsClassId());"
        ),
        "expected the '||' to stay parenthesized inside the '&&' so Dart parses the \
         same grouping this module's own promotion tracking reasoned about, got:\n{source}"
    );
}

#[test]
fn a_bang_inside_a_short_circuited_and_operand_does_not_leak_past_the_whole_condition() {
    let foo = |line: u32| Expr::Call {
        target: Some(Box::new(nullable_ref("pos", line))),
        callee_usr: "c:@S@Nota@F@Foo#".to_owned(),
        callee_name: "Foo".to_owned(),
        args: Vec::new(),
        ty: Type::Bool,
        origin: origin(line),
    };
    let bar = |line: u32| Expr::Call {
        target: Some(Box::new(nullable_ref("pos", line))),
        callee_usr: "c:@S@Nota@F@Bar#".to_owned(),
        callee_name: "Bar".to_owned(),
        args: Vec::new(),
        ty: Type::Void,
        origin: origin(line),
    };

    let condicao = Expr::Binary {
        op: BinaryOp::And,
        lhs: Box::new(Expr::Ref {
            name: "flag".to_owned(),
            ty: Type::Bool,
            origin: origin(4),
        }),
        rhs: Box::new(foo(4)),
        ty: Type::Bool,
        origin: origin(4),
    };

    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#b#".to_owned(),
        params: vec![
            Param {
                name: "flag".to_owned(),
                ty: Type::Bool,
                default_value: None,
            },
            Param {
                name: "pos".to_owned(),
                ty: nota_ref_ty(),
                default_value: None,
            },
        ],
        return_type: Type::Void,
        body: vec![
            Stmt::If {
                condition: condicao,
                then_branch: vec![Stmt::Return {
                    value: None,
                    origin: origin(4),
                }],
                else_branch: Vec::new(),
                origin: origin(4),
            },
            Stmt::ExprStmt {
                expr: bar(5),
                origin: origin(5),
            },
        ],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("pos!.Bar();"),
        "expected `pos` to still need '!' after the if, since the '&&'s right \
         operand (where `pos` was checked) might never have run, got:\n{source}"
    );
}

/// A reassignment nested two `if` levels deep still invalidates a
/// promotion the *outer* scope was relying on — real Verovio regression:
/// `staff` promoted by an early `staff!.m_drawingStaffSize`, reassigned by
/// `staff = slur!.CalculatePrincipalStaff(...)` two `if` levels deeper, then
/// read again as a bare `staff.GetN()` right after that inner `if` —
/// `emit_scoped_block`'s "nothing leaks back" design correctly dropped the
/// inner scope's own copy of the promotion, but nothing invalidated the
/// *outer* scope's copy, which had promoted `staff` before ever seeing the
/// reassignment (`unchecked_use_of_nullable_value`, 381 → 434 over the real
/// corpus even after the other two fixes in this file's history).
#[test]
fn a_reassignment_nested_two_if_levels_deep_still_invalidates_the_outer_promotion() {
    let staff_ty = nota_ref_ty();
    let call = |callee: &str, line: u32| Expr::Call {
        target: None,
        callee_usr: format!("c:@F@{callee}#"),
        callee_name: callee.to_owned(),
        args: Vec::new(),
        ty: staff_ty.clone(),
        origin: origin(line),
    };
    let bool_ref = |name: &str, line: u32| Expr::Ref {
        name: name.to_owned(),
        ty: Type::Bool,
        origin: origin(line),
    };

    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#bb#".to_owned(),
        params: vec![
            Param {
                name: "cond1".to_owned(),
                ty: Type::Bool,
                default_value: None,
            },
            Param {
                name: "cond2".to_owned(),
                ty: Type::Bool,
                default_value: None,
            },
        ],
        return_type: Type::Void,
        body: vec![
            Stmt::VarDecl {
                name: "staff".to_owned(),
                ty: staff_ty.clone(),
                init: Some(call("GetStaff", 3)),
                origin: origin(3),
            },
            int_var_decl("x", altura_read(nullable_ref("staff", 4), 4), 4),
            Stmt::If {
                condition: bool_ref("cond1", 5),
                then_branch: vec![
                    Stmt::If {
                        condition: bool_ref("cond2", 6),
                        then_branch: vec![Stmt::Assign {
                            name: "staff".to_owned(),
                            value: call("OtherStaff", 7),
                            origin: origin(7),
                        }],
                        else_branch: Vec::new(),
                        origin: origin(6),
                    },
                    Stmt::ExprStmt {
                        expr: altura_read(nullable_ref("staff", 8), 8),
                        origin: origin(8),
                    },
                ],
                else_branch: Vec::new(),
                origin: origin(5),
            },
        ],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert_eq!(
        source.matches("staff!.altura").count(),
        2,
        "expected both the first promoting read and the read after the nested \
         reassignment to keep '!', got:\n{source}"
    );
}

/// A loop's back-edge means a name the body reassigns *anywhere* can't be
/// trusted as promoted at the top of the body either, even textually
/// *before* that reassignment — real Verovio regression:
/// `HumdrumToken? token = endtoken; int tcount = token!.getPreviousTokenCount();`
/// correctly promotes `token` before a `while` loop, but the loop body both
/// reads `token` early and reassigns it later
/// (`token = token.getPreviousToken(0);`) — `dart analyze` still flags that
/// early read as needing its own `!` (`unchecked_use_of_nullable_value`,
/// 381 → 421 over the real corpus even after every other fix in this file's
/// history).
#[test]
fn a_loops_own_reassignment_strips_the_promotion_for_the_whole_body_not_just_after_it() {
    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#*$@S@Nota#".to_owned(),
        params: vec![Param {
            name: "token".to_owned(),
            ty: nota_ref_ty(),
            default_value: None,
        }],
        return_type: Type::Void,
        body: vec![
            int_var_decl("tcount", altura_read(nullable_ref("token", 3), 3), 3),
            Stmt::While {
                condition: Expr::Binary {
                    op: BinaryOp::Gt,
                    lhs: Box::new(Expr::Ref {
                        name: "tcount".to_owned(),
                        ty: Type::Int,
                        origin: origin(4),
                    }),
                    rhs: Box::new(Expr::IntLiteral {
                        value: 0,
                        origin: origin(4),
                    }),
                    ty: Type::Bool,
                    origin: origin(4),
                },
                body: vec![
                    int_var_decl("early", altura_read(nullable_ref("token", 5), 5), 5),
                    Stmt::Assign {
                        name: "token".to_owned(),
                        value: Expr::Call {
                            target: None,
                            callee_usr: "c:@F@NextToken#".to_owned(),
                            callee_name: "NextToken".to_owned(),
                            args: Vec::new(),
                            ty: nota_ref_ty(),
                            origin: origin(6),
                        },
                        origin: origin(6),
                    },
                ],
                origin: origin(4),
            },
        ],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("int early = token!.altura;"),
        "expected the early read inside the loop body to keep '!', since the \
         same body reassigns `token` further down and the loop's back-edge \
         means that could just as well be the previous iteration's value, \
         got:\n{source}"
    );
}

/// A method call's receiver is evaluated — and dereferenced — before any of
/// its arguments, same as Dart's own left-to-right evaluation of `receiver.
/// method(args)`. A promotion an argument establishes must not reach back
/// to decide the receiver's own `!`: real regression found by
/// `just verovio-diagnosis` while implementing this promotion tracking
/// (`unchecked_use_of_nullable_value` rose from 381 to 1080 because the
/// first emitter draft rendered every call's `args_text` before its
/// receiver's own `receiver_bang` decision, letting a bang inside an
/// argument that happened to share the receiver's name silently swallow
/// the receiver's own required `!`).
#[test]
fn a_calls_receiver_is_dereferenced_before_its_arguments_not_after() {
    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#".to_owned(),
        params: Vec::new(),
        return_type: Type::Void,
        body: vec![
            Stmt::VarDecl {
                name: "chord".to_owned(),
                ty: nota_ref_ty(),
                init: Some(Expr::Call {
                    target: None,
                    callee_usr: "c:@F@GetFirstAncestor#".to_owned(),
                    callee_name: "GetFirstAncestor".to_owned(),
                    args: Vec::new(),
                    ty: nota_ref_ty(),
                    origin: origin(3),
                }),
                origin: origin(3),
            },
            Stmt::ExprStmt {
                expr: Expr::Call {
                    target: Some(Box::new(nullable_ref("chord", 4))),
                    callee_usr: "c:@S@Nota@F@Foo#I#".to_owned(),
                    callee_name: "Foo".to_owned(),
                    args: vec![altura_read(nullable_ref("chord", 4), 4)],
                    ty: Type::Void,
                    origin: origin(4),
                },
                origin: origin(4),
            },
        ],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("chord!.Foo(chord.altura);"),
        "expected the receiver's own '!' (evaluated first) rather than a bang \
         smuggled onto the argument that runs after it, got:\n{source}"
    );
}

/// `if (x == null) return;` (or any other unconditional exit) promotes `x`
/// for the rest of the enclosing block: falling past the `if` at all proves
/// the condition was false.
#[test]
fn a_null_guard_that_returns_promotes_the_pointer_for_the_rest_of_the_function() {
    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#*$@S@Nota#".to_owned(),
        params: vec![Param {
            name: "a".to_owned(),
            ty: nota_ref_ty(),
            default_value: None,
        }],
        return_type: Type::Void,
        body: vec![
            Stmt::If {
                condition: Expr::Binary {
                    op: BinaryOp::Eq,
                    lhs: Box::new(nullable_ref("a", 3)),
                    rhs: Box::new(Expr::NullLiteral { origin: origin(3) }),
                    ty: Type::Bool,
                    origin: origin(3),
                },
                then_branch: vec![Stmt::Return {
                    value: None,
                    origin: origin(3),
                }],
                else_branch: Vec::new(),
                origin: origin(3),
            },
            int_var_decl("x1", altura_read(nullable_ref("a", 4), 4), 4),
        ],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    let expected_body = "\
  if (a == null) {
    return;
  }
  int x1 = a.altura;
";
    assert!(
        source.contains(expected_body),
        "expected the guard to promote `a` for the rest of the function, got:\n{source}"
    );
}

/// `then_expr` and `else_expr` are mutually exclusive, exactly like an
/// `if`'s two branches — a bang inside one must not promote the other, nor
/// leak past the whole ternary to code that runs regardless of which side
/// was taken.
#[test]
fn a_ternarys_two_branches_neither_promote_each_other_nor_leak_past_the_ternary() {
    let processa = Function {
        name: "Processa".to_owned(),
        usr: "c:@F@Processa#*$@S@Nota#b#".to_owned(),
        params: vec![
            Param {
                name: "a".to_owned(),
                ty: nota_ref_ty(),
                default_value: None,
            },
            Param {
                name: "flag".to_owned(),
                ty: Type::Bool,
                default_value: None,
            },
        ],
        return_type: Type::Int,
        body: vec![Stmt::Return {
            value: Some(Expr::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(Expr::Conditional {
                    condition: Box::new(Expr::Ref {
                        name: "flag".to_owned(),
                        ty: Type::Bool,
                        origin: origin(3),
                    }),
                    then_expr: Box::new(altura_read(nullable_ref("a", 3), 3)),
                    else_expr: Box::new(altura_read(nullable_ref("a", 3), 3)),
                    ty: Type::Int,
                    origin: origin(3),
                }),
                rhs: Box::new(altura_read(nullable_ref("a", 3), 3)),
                ty: Type::Int,
                origin: origin(3),
            }),
            origin: origin(3),
        }],
        origin: origin(2),
    };

    let module = Module {
        records: Vec::new(),
        functions: vec![processa],
        enums: Vec::new(),
    };
    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("return (flag ? a!.altura : a!.altura) + a!.altura;"),
        "expected every one of the three mutually-exclusive-or-later reads of `a` \
         to keep its own '!', got:\n{source}"
    );
}

/// A field whose type has no sound zero literal used to be emitted as
/// `Cor c = 0;` / `Ponto p = 0;` — not a poor default but invalid Dart, so
/// the whole package stopped compiling. An enum field takes its first
/// constant; a record field, which has no literal at all, becomes `late`.
#[test]
fn a_field_without_a_sound_zero_literal_is_late_not_a_fabricated_zero() {
    let cor = Enum {
        name: "Cor".to_owned(),
        usr: "c:@E@Cor".to_owned(),
        variants: vec!["vermelho".to_owned(), "azul".to_owned()],
        values: vec![0, 1],
        origin: origin(1),
    };

    let ponto = Record {
        name: "Ponto".to_owned(),
        usr: "c:@S@Ponto".to_owned(),
        namespace: String::new(),
        fields: vec![],
        static_fields: vec![],
        constructors: vec![],
        methods: vec![],
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(2),
    };

    // Every field type that has to be initialized at declaration because
    // the record owns a real constructor (E04's shape).
    let alvo = Record {
        name: "Alvo".to_owned(),
        usr: "c:@S@Alvo".to_owned(),
        namespace: String::new(),
        fields: vec![
            Field {
                name: "cor".to_owned(),
                ty: Type::Enum {
                    name: "Cor".to_owned(),
                    usr: "c:@E@Cor".to_owned(),
                },
            },
            Field {
                name: "origem".to_owned(),
                ty: Type::Record {
                    name: "Ponto".to_owned(),
                    usr: "c:@S@Ponto".to_owned(),
                },
            },
            Field {
                name: "rotulo".to_owned(),
                ty: Type::Str,
            },
            Field {
                name: "pesos".to_owned(),
                ty: Type::List(Box::new(Type::Int)),
            },
        ],
        static_fields: vec![Field {
            name: "padrao".to_owned(),
            ty: Type::Enum {
                name: "Cor".to_owned(),
                usr: "c:@E@Cor".to_owned(),
            },
        }],
        constructors: vec![Constructor {
            usr: "c:@S@Alvo@F@Alvo#".to_owned(),
            constructor_index: 0,
            params: vec![],
            body: vec![],
            origin: origin(3),
        }],
        methods: vec![],
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(3),
    };

    let module = Module {
        records: vec![ponto, alvo],
        functions: Vec::new(),
        enums: vec![cor],
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("Cor cor = Cor.vermelho;"),
        "an enum field should default to its first constant, got:\n{source}"
    );
    assert!(
        source.contains("static Cor padrao = Cor.vermelho;"),
        "a static enum field should too, got:\n{source}"
    );
    assert!(
        source.contains("late Ponto origem;"),
        "a record field has no valid literal default, so it must be `late`, got:\n{source}"
    );
    assert!(
        source.contains("String rotulo = '';"),
        "a string field should default to the empty string, got:\n{source}"
    );
    assert!(
        source.contains("List<int> pesos = [];"),
        "a list field should default to the empty list, got:\n{source}"
    );
    assert!(
        !source.contains("= 0;"),
        "no field of a non-numeric type may be initialized to 0, got:\n{source}"
    );
}

/// Dart rejects an enum with no constants outright, so emitting
/// `enum Vazio {  }` would take the whole file down with it.
#[test]
fn an_enum_without_constants_still_emits_parseable_dart() {
    let module = Module {
        records: Vec::new(),
        functions: Vec::new(),
        enums: vec![Enum {
            name: "Vazio".to_owned(),
            usr: "c:@E@Vazio".to_owned(),
            variants: vec![],
            values: vec![],
            origin: origin(1),
        }],
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        !source.contains("enum Vazio {  }") && !source.contains("enum Vazio { }"),
        "an empty Dart enum doesn't parse, got:\n{source}"
    );
    assert!(
        source.contains("TODO(syntax-bridge)"),
        "the placeholder should say why it's there, got:\n{source}"
    );
}

fn base(usr: &str, name: &str) -> BaseClass {
    BaseClass {
        usr: usr.to_owned(),
        name: name.to_owned(),
    }
}

fn mixin_record(name: &str, base_class: Option<BaseClass>, mixins: Vec<BaseClass>) -> Record {
    Record {
        name: name.to_owned(),
        usr: format!("c:@S@{name}"),
        namespace: String::new(),
        fields: Vec::new(),
        static_fields: Vec::new(),
        constructors: Vec::new(),
        methods: Vec::new(),
        base_class,
        mixins,
        destructor: None,
        origin: origin(1),
    }
}

/// Regression test: a record with more than one base always becomes
/// `mixins`, and `emit_record` used to slap `mixin`'s keyword onto the same
/// `with_clause` a plain multi-base `class` gets — `mixin Composto with
/// Voador, Nadador {`, which `dart format` rejects ("A mixin can't have a
/// with clause"). This shape never showed up in E09's own fixture
/// (`PatoDaguaVoador`'s two bases, `Voador`/`Nadador`, are themselves
/// base-less), only in the real Verovio 6.2.0 corpus, where an interface
/// record used as a mixin elsewhere (`AltSymInterface`) itself has more than
/// one base (`Interface`, `AttAltSym`) — see
/// `docs/plans/diagnostico-verovio-6.2.0.md`. Dart's own equivalent for "a
/// mixin built out of other mixins" is `mixin M on A, B {}` (a superclass
/// *constraint*, not composition) — which pushes the actual composition
/// down to whichever concrete class ends up applying the whole chain via
/// `with`: it must list every transitive `on` dependency before the mixin
/// that requires it, not just the mixin itself.
#[test]
fn a_mixin_built_from_multiple_bases_uses_on_not_with_and_leaf_classes_expand_the_chain() {
    let voador = mixin_record("Voador", None, Vec::new());
    let nadador = mixin_record("Nadador", None, Vec::new());
    let composto = mixin_record(
        "Composto",
        None,
        vec![base(&voador.usr, "Voador"), base(&nadador.usr, "Nadador")],
    );
    // `Usuario` only lists `Composto`, never `Voador`/`Nadador` directly —
    // exactly how `ControlElement` lists `AltSymInterface` without also
    // separately listing `Interface`/`AttAltSym` in the real corpus.
    let usuario = mixin_record("Usuario", None, vec![base(&composto.usr, "Composto")]);
    let module = Module {
        records: vec![voador, nadador, composto, usuario],
        functions: Vec::new(),
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("mixin Composto on Voador, Nadador {"),
        "expected `Composto` to constrain via `on`, not compose via `with`, got:\n{source}"
    );
    assert!(
        !source.contains("mixin Composto with"),
        "a Dart `mixin` can't have a `with` clause, got:\n{source}"
    );
    assert!(
        source.contains("class Usuario with Voador, Nadador, Composto {"),
        "expected `Usuario` to expand `Composto`'s own `on` dependencies into its `with` \
         clause, in dependency-before-dependent order, got:\n{source}"
    );
}

/// Same bug, single-base shape: a record with exactly one base (E06's
/// `extends`) that's *also* used as a mixin elsewhere used to keep its
/// `extends` clause verbatim — `mixin AttAltSym extends Att {`, equally
/// invalid Dart (a `mixin` can't have `extends` either, only `on`).
#[test]
fn a_mixin_with_a_single_base_uses_on_not_extends() {
    let att_alt_sym = mixin_record("AttAltSym", Some(base("c:@S@Att", "Att")), Vec::new());
    let rotulavel = mixin_record("Rotulavel", None, Vec::new());
    let control_like = mixin_record(
        "ControlLike",
        None,
        vec![
            base(&att_alt_sym.usr, "AttAltSym"),
            base(&rotulavel.usr, "Rotulavel"),
        ],
    );
    let module = Module {
        records: vec![att_alt_sym, rotulavel, control_like],
        functions: Vec::new(),
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("mixin AttAltSym on Att {"),
        "expected `AttAltSym` to constrain via `on`, not `extends`, got:\n{source}"
    );
    assert!(
        !source.contains("mixin AttAltSym extends"),
        "a Dart `mixin` can't have an `extends` clause, got:\n{source}"
    );
    assert!(
        source.contains("class ControlLike with Att, AttAltSym, Rotulavel {"),
        "expected `ControlLike` to expand `AttAltSym`'s own `on` dependency (`Att`, unresolved \
         in this module but still needed) before it in the `with` clause, got:\n{source}"
    );
}

/// Regression test: `mixin_usrs` used to collect only *direct*
/// `record.mixins` targets, so a record reachable exclusively through
/// another mixin's own `base_class` — never named directly in anyone's
/// `mixins` list — kept its plain `class` keyword even though
/// `expand_mixin_chain` (previous test) puts it in a leaf's `with` clause
/// anyway. Dart's `mixin_of_non_class` fires the moment a `with` clause
/// names something that isn't declared `mixin`: this is `Att` from the
/// previous test, this time actually present as a record in the module (not
/// just an unresolved name) so its own declaration keyword can be checked.
#[test]
fn a_base_reachable_only_through_another_mixins_base_class_still_gets_the_mixin_keyword() {
    let att = mixin_record("Att", None, Vec::new());
    let att_alt_sym = mixin_record("AttAltSym", Some(base(&att.usr, "Att")), Vec::new());
    let rotulavel = mixin_record("Rotulavel", None, Vec::new());
    let control_like = mixin_record(
        "ControlLike",
        None,
        vec![
            base(&att_alt_sym.usr, "AttAltSym"),
            base(&rotulavel.usr, "Rotulavel"),
        ],
    );
    let module = Module {
        records: vec![att, att_alt_sym, rotulavel, control_like],
        functions: Vec::new(),
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("mixin Att {"),
        "expected `Att` to be declared `mixin`, since `ControlLike` applies it via `with`, \
         got:\n{source}"
    );
    assert!(
        !source.contains("class Att {"),
        "a `with Att` clause needs `Att` declared as `mixin`, not `class` \
         (`mixin_of_non_class`), got:\n{source}"
    );
    assert!(
        source.contains("class ControlLike with Att, AttAltSym, Rotulavel {"),
        "got:\n{source}"
    );
}

/// Regression test: `collect_referenced_usrs_in_record` (E11's `import`
/// computation) only ever looked at a record's *direct* `mixins` — but
/// `emit_record`'s own `with` clause (`expand_mixin_chain`) prints every
/// transitive `on` dependency by name too. A leaf class whose only *direct*
/// mixin lives in its own file, while that mixin's own further-up bases
/// live in a different one (the real Verovio 6.2.0 shape: `Abbr`'s two
/// direct bases pull in eight more names transitively, none of them
/// imported), used to emit a `with` clause referencing types this file
/// never imports — `undefined_class` at best, or (since an unresolved name
/// in a `with` clause reads as "not a mixin") `mixin_of_non_class`.
#[test]
fn a_leaf_class_imports_every_transitively_expanded_mixin_dependency() {
    fn origin_in(file: &str) -> Origin {
        Origin {
            file: file.to_owned(),
            line: 1,
            column: 1,
        }
    }

    // Three distinct files, so each import has exactly one possible cause:
    // `Usuario` (file A) only ever names `Composto` (file B) directly in its
    // own `record.mixins` — the existing, already-correct direct-usr
    // collection accounts for that import on its own. `Voador`/`Nadador`
    // (file C) are reachable only through *Composto's* own `mixins`, two
    // levels down from `Usuario` — nothing in `Usuario`'s own direct fields
    // ever names them, so file A's import for file C can only come from
    // walking the same expanded chain `emit_record` prints into `Usuario`'s
    // `with` clause.
    let voador = Record {
        origin: origin_in("/project/input-source/src/animais.cpp"),
        ..mixin_record("Voador", None, Vec::new())
    };
    let nadador = Record {
        origin: origin_in("/project/input-source/src/animais.cpp"),
        ..mixin_record("Nadador", None, Vec::new())
    };
    let composto = Record {
        origin: origin_in("/project/input-source/src/interfaces.cpp"),
        ..mixin_record(
            "Composto",
            None,
            vec![base(&voador.usr, "Voador"), base(&nadador.usr, "Nadador")],
        )
    };
    let usuario = mixin_record("Usuario", None, vec![base(&composto.usr, "Composto")]);
    let module = Module {
        records: vec![voador, nadador, composto, usuario],
        functions: Vec::new(),
        enums: Vec::new(),
    };

    let files = emit_module(&module);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("import 'interfaces.dart';"),
        "expected an import for `Composto`'s file (`Usuario`'s direct mixin), got:\n{source}"
    );
    assert!(
        source.contains("import 'animais.dart';"),
        "expected an import for `Voador`/`Nadador`'s file too — `Usuario`'s expanded `with` \
         clause names them directly, even though neither is `Usuario`'s own direct mixin, \
         got:\n{source}"
    );
}

/// `docs/plans/lista-de-externos.md` decision 1: "mock = valor plausível,
/// execução segue" — a free function whose usr is in the effective external
/// set gets a `return` of a plausible default for its type, never a
/// `throw`, even though its own body (never lowered, per
/// `function_catalog`'s prototype cataloging) is the `Stmt::Unsupported`
/// placeholder `lower::cpp` synthesizes for exactly this case.
#[test]
fn an_externally_marked_free_function_returns_a_plausible_default_instead_of_throwing() {
    let function = Function {
        body: vec![Stmt::Unsupported {
            reason: "declared but never defined in any compilation unit of this project".to_owned(),
            origin: origin(2),
        }],
        ..soma_function()
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function.clone()],
        enums: Vec::new(),
    };
    let external_usrs: HashSet<&str> = [function.usr.as_str()].into_iter().collect();

    let files = emit_module_with_externals(&module, &external_usrs);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("// syntax-bridge: externo, corpo mockado"),
        "expected an honest marker comment, got:\n{source}"
    );
    assert!(
        source.contains("return 0;"),
        "expected a plausible int default, got:\n{source}"
    );
    assert!(
        !source.contains("throw"),
        "an external usr must never throw — that's the Unsupported idiom this path \
         exists to replace, got:\n{source}"
    );
}

/// A `void`-returning external function has nothing to `return` a default
/// of — the mock body is just the marker comment, still never a `throw`.
#[test]
fn an_externally_marked_void_function_has_an_empty_mocked_body() {
    let function = Function {
        return_type: Type::Void,
        body: vec![Stmt::Unsupported {
            reason: "declared but never defined".to_owned(),
            origin: origin(2),
        }],
        ..soma_function()
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function.clone()],
        enums: Vec::new(),
    };
    let external_usrs: HashSet<&str> = [function.usr.as_str()].into_iter().collect();

    let files = emit_module_with_externals(&module, &external_usrs);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("// syntax-bridge: externo, corpo mockado"),
        "got:\n{source}"
    );
    assert!(!source.contains("throw"), "got:\n{source}");
    assert!(!source.contains("return "), "got:\n{source}");
}

/// Regression guard for `emit_module`'s own delegation: with no usr in the
/// external set, output must be byte-identical to before this feature
/// existed — `emit_module` is `emit_module_with_externals` with an empty
/// set, never a behavior change for a project that marks nothing external.
#[test]
fn emit_module_produces_the_same_output_as_an_empty_external_set() {
    let module = Module {
        records: Vec::new(),
        functions: vec![soma_function()],
        enums: Vec::new(),
    };

    let plain = emit_module(&module);
    let with_empty_set = emit_module_with_externals(&module, &HashSet::new());
    assert_eq!(plain, with_empty_set);
}

fn area_method(usr: &str, return_type: Type, body: Vec<Stmt>) -> Method {
    Method {
        name: "area".to_owned(),
        usr: usr.to_owned(),
        params: Vec::new(),
        return_type,
        body: Some(body),
        is_static: false,
        is_override: false,
        origin: origin(3),
    }
}

/// Same guarantee as the free-function case, for a method reached by
/// marking its owning type external (decision 3's cascade,
/// `docs/plans/lista-de-externos.md`) or the method's own usr directly.
#[test]
fn an_externally_marked_method_mocks_its_return_value() {
    let method = area_method(
        "c:@S@Shape@F@area#",
        Type::Double,
        vec![Stmt::Unsupported {
            reason: "declared but never defined".to_owned(),
            origin: origin(3),
        }],
    );
    let record = Record {
        name: "Shape".to_owned(),
        usr: "c:@S@Shape".to_owned(),
        namespace: String::new(),
        fields: Vec::new(),
        static_fields: Vec::new(),
        constructors: Vec::new(),
        methods: vec![method],
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(1),
    };
    let module = Module {
        records: vec![record],
        functions: Vec::new(),
        enums: Vec::new(),
    };
    let external_usrs: HashSet<&str> = ["c:@S@Shape@F@area#"].into_iter().collect();

    let files = emit_module_with_externals(&module, &external_usrs);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("// syntax-bridge: externo, corpo mockado"),
        "got:\n{source}"
    );
    assert!(source.contains("return 0;"), "got:\n{source}");
    assert!(!source.contains("throw"), "got:\n{source}");
}

/// A constructor has no return type of its own to mock — its fields already
/// get a sound default at declaration (`emit_field_declaration`), so an
/// external constructor's mocked body is simply empty, and the object still
/// comes out fully initialized.
#[test]
fn an_externally_marked_constructor_has_an_empty_mocked_body() {
    let constructor = Constructor {
        usr: "c:@S@Shape@F@Shape#".to_owned(),
        constructor_index: 0,
        params: Vec::new(),
        body: vec![Stmt::Unsupported {
            reason: "declared but never defined".to_owned(),
            origin: origin(2),
        }],
        origin: origin(2),
    };
    let record = Record {
        name: "Shape".to_owned(),
        usr: "c:@S@Shape".to_owned(),
        namespace: String::new(),
        fields: vec![Field {
            name: "radius".to_owned(),
            ty: Type::Double,
        }],
        static_fields: Vec::new(),
        constructors: vec![constructor],
        methods: Vec::new(),
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(1),
    };
    let module = Module {
        records: vec![record],
        functions: Vec::new(),
        enums: Vec::new(),
    };
    let external_usrs: HashSet<&str> = ["c:@S@Shape@F@Shape#"].into_iter().collect();

    let files = emit_module_with_externals(&module, &external_usrs);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("// syntax-bridge: externo, corpo mockado"),
        "got:\n{source}"
    );
    assert!(!source.contains("throw"), "got:\n{source}");
}

/// When the mocked type itself has no plausible value at all
/// (`Type::Unsupported` — the product doesn't know how to represent it,
/// mocked or not), the mock path falls back to the same honest
/// `Stmt::Unsupported` bailout every other unrepresentable construct uses —
/// the one case where "nada de frio" still yields to "silêncio é proibido",
/// because there is no non-throwing Dart value of that type to return.
#[test]
fn an_externally_marked_function_with_an_unmockable_return_type_still_bails_out_honestly() {
    let function = Function {
        return_type: Type::Unsupported("long".to_owned()),
        body: vec![Stmt::Unsupported {
            reason: "declared but never defined".to_owned(),
            origin: origin(2),
        }],
        ..soma_function()
    };
    let module = Module {
        records: Vec::new(),
        functions: vec![function.clone()],
        enums: Vec::new(),
    };
    let external_usrs: HashSet<&str> = [function.usr.as_str()].into_iter().collect();

    let files = emit_module_with_externals(&module, &external_usrs);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("throw UnimplementedError("),
        "expected the honest Unsupported bailout when the type itself has no \
         plausible value, got:\n{source}"
    );
    assert!(
        source.contains("não tem valor plausível para mock"),
        "got:\n{source}"
    );
}

/// A `Record`-returning external function mocks a real, instantiable value —
/// a call to the record's own synthetic positional constructor (no
/// `constructors` of its own), with each field's own default recursively.
#[test]
fn an_externally_marked_function_returning_a_record_mocks_a_constructor_call() {
    let ponto = Record {
        name: "Ponto".to_owned(),
        usr: "c:@S@Ponto".to_owned(),
        namespace: String::new(),
        fields: vec![
            Field {
                name: "x".to_owned(),
                ty: Type::Int,
            },
            Field {
                name: "y".to_owned(),
                ty: Type::Int,
            },
        ],
        static_fields: Vec::new(),
        constructors: Vec::new(),
        methods: Vec::new(),
        base_class: None,
        mixins: Vec::new(),
        destructor: None,
        origin: origin(1),
    };
    let function = Function {
        name: "origem".to_owned(),
        usr: "c:@F@origem#".to_owned(),
        params: Vec::new(),
        return_type: Type::Record {
            usr: ponto.usr.clone(),
            name: ponto.name.clone(),
        },
        body: vec![Stmt::Unsupported {
            reason: "declared but never defined".to_owned(),
            origin: origin(5),
        }],
        origin: origin(5),
    };
    let module = Module {
        records: vec![ponto],
        functions: vec![function.clone()],
        enums: Vec::new(),
    };
    let external_usrs: HashSet<&str> = [function.usr.as_str()].into_iter().collect();

    let files = emit_module_with_externals(&module, &external_usrs);
    let source = &files["lib/aritmetica.dart"];

    assert!(
        source.contains("return Ponto(0, 0);"),
        "expected a real, instantiable mock value, got:\n{source}"
    );
    assert!(!source.contains("throw"), "got:\n{source}");
}
