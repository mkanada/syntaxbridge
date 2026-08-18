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
#[test]
fn an_enum_emits_as_a_plain_dart_enum_declaration() {
    let cor = Enum {
        name: "Cor".to_owned(),
        usr: "c:@E@Cor".to_owned(),
        variants: vec!["Vermelho".to_owned(), "Verde".to_owned(), "Azul".to_owned()],
        origin: origin(2),
    };
    let module = Module {
        records: Vec::new(),
        functions: Vec::new(),
        enums: vec![cor],
    };

    let files = emit_module(&module);
    assert!(
        files
            .values()
            .any(|source| source.contains("enum Cor { Vermelho, Verde, Azul }")),
        "expected a plain `enum Cor {{ ... }}` declaration, got:\n{files:?}"
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

/// A bailout expression still always throws at runtime, but it must not
/// advertise `Never` to the Dart analyzer. Real generated code can keep
/// traversing the syntactic expression around that bailout (`unsupported().x`
/// or `unsupported().method()`), and a `Never` receiver turns each such
/// traversal into `receiver_of_type_never`. `dynamic` keeps the bailout
/// explicit while quarantining the unknown static type at its boundary.
#[test]
fn an_unsupported_expression_calls_a_dynamic_helper_to_quarantine_the_bailout_type() {
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
            "return _syntaxBridgeUnsupported('/project/input-source/src/aritmetica.cpp:6: unsupported expression cursor kind 999').value;"
        ),
        "missing helper call, got:\n{source}"
    );
    assert!(
        source.contains("dynamic _syntaxBridgeUnsupported(String reason) {"),
        "helper function should quarantine the unsupported expression as dynamic, got:\n{source}"
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
