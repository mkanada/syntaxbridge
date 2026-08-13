//! Lowers a `libclang`-parsed free function body into [`crate::ir`].
//!
//! Called from `function_catalog::visit_cursor`, on the *same* parsed
//! translation unit that pass already holds — this is not a fourth
//! `clang_parseTranslationUnit` pass (`docs/plans/primeiro-corte-e01-e03.md`
//! §4 decision 7). It walks the already-parsed function body cursor with its
//! own `clang_visitChildren` calls, exactly the way
//! `function_catalog::visit_call_site` already walks the same cursor to
//! build the call graph.
//!
//! Scope grows one degrau of `examples/` at a time. E01
//! (`docs/plans/primeiro-corte-e01-e03.md` §7 PR2) needed a `CompoundStmt`
//! body containing a single `ReturnStmt` over `DeclRefExpr`/integer
//! literal/`BinaryOperator`. E02 (PR4) added `if`/`while`/`for`, local
//! variable declaration and assignment, calls (including recursive ones),
//! unary negation, and the comparison/arithmetic operators those need. E03
//! (PR5) added `struct` records (`lower_record`), field access/assignment,
//! and — its armadilha — an implicit self-clone `lower_function` inserts for
//! every by-value `Record` parameter, so mutating it inside the body can't
//! leak back to the caller the way it would if Dart's pass-by-reference were
//! left to show through (see `examples/E03-struct-pod/NOTES.md`).
//! Anything else still becomes `ir::Stmt::Unsupported` /
//! `ir::Expr::Unsupported` rather than being silently dropped or panicking.

use std::ffi::c_void;
use std::os::raw::c_uint;
use std::path::Path;

use crate::ir;
use crate::type_catalog;

/// Lowers one free function's definition cursor into IR. `usr` is passed in
/// rather than re-derived, since the caller (`function_catalog::visit_cursor`)
/// already computed it as the catalog's join key for this same cursor.
/// Returns `None` only when the cursor has no usable name or site — mirrors
/// `function_catalog::describe_function`'s own bail-out conditions.
pub fn lower_function(
    cursor: clang_sys::CXCursor,
    usr: &str,
    project_root: &Path,
) -> Option<ir::Function> {
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    if name.is_empty() {
        return None;
    }

    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;
    let origin = ir::Origin { file, line, column };

    let return_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(cursor) });
    let (params, clone_prelude) = unsafe { collect_params_with_clone_prelude(cursor, &origin) };
    let body_cursor = unsafe { find_compound_stmt_child(cursor) };
    let mut body = match body_cursor {
        Some(compound) => unsafe { lower_compound_stmt(compound, project_root) },
        None => Vec::new(),
    };
    body.splice(0..0, clone_prelude);

    Some(ir::Function {
        name,
        usr: usr.to_owned(),
        params,
        return_type,
        body,
        origin,
    })
}

/// Lowers a `struct`/`class` *definition* cursor into IR — called from
/// `function_catalog::visit_cursor` alongside its free-function handling,
/// on the same already-parsed cursor (see this module's docs).
pub fn lower_record(cursor: clang_sys::CXCursor, project_root: &Path) -> Option<ir::Record> {
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    if name.is_empty() {
        return None;
    }
    let usr = unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(cursor)) };
    if usr.is_empty() {
        return None;
    }
    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;
    let origin = ir::Origin { file, line, column };
    let fields = unsafe { record_fields_of(cursor) };

    Some(ir::Record {
        name,
        usr,
        fields,
        origin,
    })
}

/// `struct`/`class` fields, in declaration order — filters `cursor`'s
/// children down to `CXCursor_FieldDecl` (skipping methods, access
/// specifiers, etc. that a non-POD-but-still-in-scope record might have).
unsafe fn record_fields_of(cursor: clang_sys::CXCursor) -> Vec<ir::Field> {
    unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| unsafe { clang_sys::clang_getCursorKind(*child) } == clang_sys::CXCursor_FieldDecl)
        .map(|field_cursor| {
            let name = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(field_cursor))
            };
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(field_cursor) });
            ir::Field { name, ty }
        })
        .collect()
}

fn lower_type(cx_type: clang_sys::CXType) -> ir::Type {
    // `Ponto` (bare, unqualified) resolves to `CXType_Elaborated`, not
    // `CXType_Record`, directly — confirmed empirically on a parameter type
    // (`clang_getTypeSpelling` prints the bare name, `.kind` is 119). The
    // record definition itself (`lower_record`) never goes through this
    // function, so it never hit the mismatch; every *use* of the type name
    // (as a parameter, field, or local) does. `clang_Type_getNamedType`
    // unwraps to the real underlying type.
    if cx_type.kind == clang_sys::CXType_Elaborated {
        return lower_type(unsafe { clang_sys::clang_Type_getNamedType(cx_type) });
    }

    match cx_type.kind {
        clang_sys::CXType_Int => ir::Type::Int,
        clang_sys::CXType_Bool => ir::Type::Bool,
        clang_sys::CXType_Double => ir::Type::Double,
        clang_sys::CXType_Void => ir::Type::Void,
        clang_sys::CXType_Record => {
            let decl = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
            let usr =
                unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(decl)) };
            let name = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(decl))
            };
            if usr.is_empty() || name.is_empty() {
                let spelling = unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
                };
                ir::Type::Unsupported(spelling)
            } else {
                ir::Type::Record { usr, name }
            }
        }
        _ => {
            let spelling = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
            };
            ir::Type::Unsupported(spelling)
        }
    }
}

unsafe fn collect_children(cursor: clang_sys::CXCursor) -> Vec<clang_sys::CXCursor> {
    let mut children: Vec<clang_sys::CXCursor> = Vec::new();

    extern "C" fn visit(
        cursor: clang_sys::CXCursor,
        _parent: clang_sys::CXCursor,
        data: clang_sys::CXClientData,
    ) -> clang_sys::CXChildVisitResult {
        let children = unsafe { &mut *(data as *mut Vec<clang_sys::CXCursor>) };
        children.push(cursor);
        clang_sys::CXChildVisit_Continue
    }

    unsafe {
        clang_sys::clang_visitChildren(
            cursor,
            visit,
            &mut children as *mut Vec<clang_sys::CXCursor> as *mut c_void,
        );
    }

    children
}

/// Collects parameters and, for every by-value `Record`-typed one, an
/// implicit self-clone statement (`p = Ponto(p.x, p.y);`) — E03's armadilha
/// (see this module's docs and `examples/E03-struct-pod/NOTES.md`). Works
/// straight off each `ParmDecl`'s `CXType` (not the already-lowered
/// `ir::Type`), since building the clone's field list needs the record's
/// declaration cursor, which `clang_getTypeDeclaration` only gives from a
/// `CXType`.
unsafe fn collect_params_with_clone_prelude(
    cursor: clang_sys::CXCursor,
    origin: &ir::Origin,
) -> (Vec<ir::Param>, Vec<ir::Stmt>) {
    let mut params = Vec::new();
    let mut prelude = Vec::new();

    for param_cursor in unsafe { collect_children(cursor) } {
        if unsafe { clang_sys::clang_getCursorKind(param_cursor) } != clang_sys::CXCursor_ParmDecl {
            continue;
        }

        let param_name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(param_cursor))
        };
        let cx_type = unsafe { clang_sys::clang_getCursorType(param_cursor) };
        let ty = lower_type(cx_type);

        if let ir::Type::Record { usr, name } = &ty {
            let decl = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
            let fields = unsafe { record_fields_of(decl) };
            let field_values = fields
                .into_iter()
                .map(|field| {
                    let access = ir::Expr::FieldAccess {
                        target: Box::new(ir::Expr::Ref {
                            name: param_name.clone(),
                            ty: ty.clone(),
                            origin: origin.clone(),
                        }),
                        field: field.name.clone(),
                        ty: field.ty,
                        origin: origin.clone(),
                    };
                    (field.name, access)
                })
                .collect();
            prelude.push(ir::Stmt::Assign {
                name: param_name.clone(),
                value: ir::Expr::RecordConstruct {
                    type_usr: usr.clone(),
                    type_name: name.clone(),
                    fields: field_values,
                    origin: origin.clone(),
                },
                origin: origin.clone(),
            });
        }

        params.push(ir::Param {
            name: param_name,
            ty,
        });
    }

    (params, prelude)
}

unsafe fn find_compound_stmt_child(cursor: clang_sys::CXCursor) -> Option<clang_sys::CXCursor> {
    unsafe { collect_children(cursor) }
        .into_iter()
        .find(|child| unsafe { clang_sys::clang_getCursorKind(*child) } == clang_sys::CXCursor_CompoundStmt)
}

unsafe fn lower_compound_stmt(cursor: clang_sys::CXCursor, project_root: &Path) -> Vec<ir::Stmt> {
    unsafe { collect_children(cursor) }
        .into_iter()
        .map(|child| unsafe { lower_stmt(child, project_root) })
        .collect()
}

/// Lowers an `if`/`while`/`for` branch that may or may not be a braced
/// block (`if (x) return;` has no `CompoundStmt` child at all — its single
/// statement cursor stands in directly).
unsafe fn lower_branch(cursor: clang_sys::CXCursor, project_root: &Path) -> Vec<ir::Stmt> {
    if unsafe { clang_sys::clang_getCursorKind(cursor) } == clang_sys::CXCursor_CompoundStmt {
        return unsafe { lower_compound_stmt(cursor, project_root) };
    }
    vec![unsafe { lower_stmt(cursor, project_root) }]
}

fn stmt_origin(cursor: clang_sys::CXCursor, project_root: &Path) -> ir::Origin {
    match type_catalog::cursor_site(cursor, project_root) {
        Some((file, line, column)) => ir::Origin { file, line, column },
        None => ir::Origin {
            file: String::new(),
            line: 0,
            column: 0,
        },
    }
}

/// Cursor kinds `lower_expr` can turn into a real [`ir::Expr`] (as opposed
/// to falling back to `Unsupported`) — used by `lower_stmt` to tell "this is
/// an expression used as a statement" (`Stmt::ExprStmt`) apart from "this is
/// a statement kind nothing here recognizes at all" (`Stmt::Unsupported`,
/// which is what keeps the whole-function bail-out in `emit::dart` honest).
fn is_known_expression_kind(kind: clang_sys::CXCursorKind) -> bool {
    matches!(
        kind,
        clang_sys::CXCursor_UnexposedExpr
            | clang_sys::CXCursor_ParenExpr
            | clang_sys::CXCursor_DeclRefExpr
            | clang_sys::CXCursor_IntegerLiteral
            | clang_sys::CXCursor_CXXBoolLiteralExpr
            | clang_sys::CXCursor_BinaryOperator
            | clang_sys::CXCursor_UnaryOperator
            | clang_sys::CXCursor_CallExpr
            | clang_sys::CXCursor_MemberRefExpr
    )
}

unsafe fn lower_stmt(cursor: clang_sys::CXCursor, project_root: &Path) -> ir::Stmt {
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
    let origin = stmt_origin(cursor, project_root);

    if kind == clang_sys::CXCursor_ReturnStmt {
        let value = unsafe { collect_children(cursor) }
            .into_iter()
            .next()
            .map(|child| unsafe { lower_expr(child, project_root) });
        return ir::Stmt::Return { value, origin };
    }

    if kind == clang_sys::CXCursor_DeclStmt {
        return unsafe { lower_decl_stmt(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_IfStmt {
        let children = unsafe { collect_children(cursor) };
        return match children.as_slice() {
            [condition_cursor, then_cursor] => ir::Stmt::If {
                condition: unsafe { lower_expr(*condition_cursor, project_root) },
                then_branch: unsafe { lower_branch(*then_cursor, project_root) },
                else_branch: Vec::new(),
                origin,
            },
            [condition_cursor, then_cursor, else_cursor] => ir::Stmt::If {
                condition: unsafe { lower_expr(*condition_cursor, project_root) },
                then_branch: unsafe { lower_branch(*then_cursor, project_root) },
                else_branch: unsafe { lower_branch(*else_cursor, project_root) },
                origin,
            },
            _ => ir::Stmt::Unsupported {
                reason: format!("IfStmt had {} children, expected 2 or 3", children.len()),
                origin,
            },
        };
    }

    if kind == clang_sys::CXCursor_WhileStmt {
        let children = unsafe { collect_children(cursor) };
        let [condition_cursor, body_cursor] = children.as_slice() else {
            return ir::Stmt::Unsupported {
                reason: format!("WhileStmt had {} children, expected 2", children.len()),
                origin,
            };
        };
        return ir::Stmt::While {
            condition: unsafe { lower_expr(*condition_cursor, project_root) },
            body: unsafe { lower_branch(*body_cursor, project_root) },
            origin,
        };
    }

    if kind == clang_sys::CXCursor_ForStmt {
        let children = unsafe { collect_children(cursor) };
        let [init_cursor, condition_cursor, increment_cursor, body_cursor] = children.as_slice()
        else {
            return ir::Stmt::Unsupported {
                reason: format!(
                    "ForStmt had {} children, expected init+condition+increment+body \
                     (a for-loop missing one of these clauses isn't supported yet)",
                    children.len()
                ),
                origin,
            };
        };
        return ir::Stmt::For {
            init: Some(Box::new(unsafe { lower_stmt(*init_cursor, project_root) })),
            condition: Some(unsafe { lower_expr(*condition_cursor, project_root) }),
            increment: Some(Box::new(unsafe {
                lower_stmt(*increment_cursor, project_root)
            })),
            body: unsafe { lower_branch(*body_cursor, project_root) },
            origin,
        };
    }

    if kind == clang_sys::CXCursor_BinaryOperator
        && unsafe { clang_sys::clang_getCursorBinaryOperatorKind(cursor) }
            == clang_sys::CXBinaryOperator_Assign
    {
        return unsafe { lower_assign_stmt(cursor, project_root, origin) };
    }

    if is_known_expression_kind(kind) {
        return ir::Stmt::ExprStmt {
            expr: unsafe { lower_expr(cursor, project_root) },
            origin,
        };
    }

    ir::Stmt::Unsupported {
        reason: format!("unsupported statement cursor kind {kind}"),
        origin,
    }
}

/// `int total = 0;` — a `DeclStmt` wrapping exactly one `VarDecl`. Multiple
/// declarators in one statement (`int a = 0, b = 1;`) aren't in any E01–E03
/// fixture; reported as `Unsupported` rather than guessing which one to
/// lower.
unsafe fn lower_decl_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Stmt {
    let children = unsafe { collect_children(cursor) };
    let [var_decl_cursor] = children.as_slice() else {
        return ir::Stmt::Unsupported {
            reason: format!(
                "DeclStmt had {} declarators, expected exactly 1",
                children.len()
            ),
            origin,
        };
    };

    if unsafe { clang_sys::clang_getCursorKind(*var_decl_cursor) } != clang_sys::CXCursor_VarDecl {
        return ir::Stmt::Unsupported {
            reason: "DeclStmt's declarator is not a VarDecl".to_owned(),
            origin,
        };
    }

    let name = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(*var_decl_cursor))
    };
    let cx_type = unsafe { clang_sys::clang_getCursorType(*var_decl_cursor) };
    let ty = lower_type(cx_type);

    // A record-typed `VarDecl` always has at least one child that isn't a
    // real initializer: `libclang` emits a leading `TypeRef` (pointing at
    // the record type) purely for navigation, present even for a builtin
    // type but only *matched here* since `int`/`double`/`bool` locals never
    // hit this path with a spurious extra child in E01/E02's fixtures.
    // `Ponto p;` with no written initializer *also* gets a child: an
    // implicit default-constructor `CallExpr` `libclang` synthesizes —
    // confirmed via `clang -Xclang -ast-dump` before writing this, not
    // guessed. Both need filtering out before "first remaining child, if
    // any" is the real initializer.
    let init_candidates: Vec<clang_sys::CXCursor> = unsafe { collect_children(*var_decl_cursor) }
        .into_iter()
        .filter(|child| unsafe { clang_sys::clang_getCursorKind(*child) } != clang_sys::CXCursor_TypeRef)
        .filter(|child| !unsafe { is_default_construct_with_no_args(*child) })
        .collect();
    let init = match init_candidates.as_slice() {
        // `late Ponto p;` alone isn't enough (checked with real `dart
        // analyze`, not assumed): `late` defers *whole-object*
        // assignment, but `p.x = x;` right after needs `p` to already
        // *hold* an object to set a field on — "definitely unassigned
        // late local variable". C++'s `Ponto p;` default-constructs in
        // place, so a genuinely equivalent Dart local needs a real
        // (zero-valued) object from the start, not a deferred one.
        [] => default_record_construct(&ty, cx_type, &origin),
        [only_child] => Some(unsafe { lower_expr(*only_child, project_root) }),
        _ => Some(ir::Expr::Unsupported {
            reason: format!(
                "VarDecl had {} initializer-shaped children, expected at most 1",
                init_candidates.len()
            ),
            origin: origin.clone(),
        }),
    };

    ir::Stmt::VarDecl {
        name,
        ty,
        init,
        origin,
    }
}

/// Builds a zero-valued `RecordConstruct` for a `Record`-typed local with no
/// written initializer (see `lower_decl_stmt`) — `None` for any other type,
/// where Dart's `late` is already sufficient (`emit::dart` still emits
/// `late` when `init` comes back `None`).
fn default_record_construct(
    ty: &ir::Type,
    cx_type: clang_sys::CXType,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    let ir::Type::Record { usr, name } = ty else {
        return None;
    };

    let decl = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
    let fields = unsafe { record_fields_of(decl) }
        .into_iter()
        .map(|field| {
            let value = default_scalar_value(&field.ty, origin);
            (field.name, value)
        })
        .collect();

    Some(ir::Expr::RecordConstruct {
        type_usr: usr.clone(),
        type_name: name.clone(),
        fields,
        origin: origin.clone(),
    })
}

/// Zero-value for a field's type — `IntLiteral(0)` for both `Int` and
/// `Double` (a bare `0` is a valid Dart `double` argument via numeric
/// literal coercion, confirmed with real `dart analyze`; no `Expr` for a
/// float literal exists yet — E01/E02 never needed one — and adding it just
/// for a default value nothing else uses would be exactly the premature
/// abstraction AGENTS.md rules out). Nested records / anything else stay
/// `Unsupported`, honestly, rather than guessing — not exercised by any
/// E01–E03 fixture.
fn default_scalar_value(ty: &ir::Type, origin: &ir::Origin) -> ir::Expr {
    match ty {
        ir::Type::Int | ir::Type::Double => ir::Expr::IntLiteral {
            value: 0,
            origin: origin.clone(),
        },
        ir::Type::Bool => ir::Expr::BoolLiteral {
            value: false,
            origin: origin.clone(),
        },
        ir::Type::Record { .. } | ir::Type::Void | ir::Type::Unsupported(_) => {
            ir::Expr::Unsupported {
                reason: "no default value available for this field's type yet".to_owned(),
                origin: origin.clone(),
            }
        }
    }
}

/// `total = total + i;` — a top-level `BinaryOperator` with `=`. Only a
/// simple variable target is supported (field/index assignment is E03/E10
/// scope); anything else is `Unsupported` rather than silently mistranslated.
unsafe fn lower_assign_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Stmt {
    let children = unsafe { collect_children(cursor) };
    let [lhs_cursor, rhs_cursor] = children.as_slice() else {
        return ir::Stmt::Unsupported {
            reason: format!(
                "assignment cursor had {} children, expected 2",
                children.len()
            ),
            origin,
        };
    };

    let lhs_kind = unsafe { clang_sys::clang_getCursorKind(*lhs_cursor) };
    let value = unsafe { lower_expr(*rhs_cursor, project_root) };

    if lhs_kind == clang_sys::CXCursor_DeclRefExpr {
        let name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(*lhs_cursor))
        };
        return ir::Stmt::Assign {
            name,
            value,
            origin,
        };
    }

    if lhs_kind == clang_sys::CXCursor_MemberRefExpr {
        let field = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(*lhs_cursor))
        };
        let target_children = unsafe { collect_children(*lhs_cursor) };
        let [target_cursor] = target_children.as_slice() else {
            return ir::Stmt::Unsupported {
                reason: format!(
                    "assignment target field access had {} children, expected 1",
                    target_children.len()
                ),
                origin,
            };
        };
        let target = unsafe { lower_expr(*target_cursor, project_root) };
        return ir::Stmt::FieldAssign {
            target,
            field,
            value,
            origin,
        };
    }

    ir::Stmt::Unsupported {
        reason: "assignment target is not a simple local variable or a field \
                  (index assignment not supported yet)"
            .to_owned(),
        origin,
    }
}

/// Cursor kinds `libclang` uses purely as sugar around another expression
/// (implicit conversions, parentheses) — lowering unwraps them transparently
/// by recursing into their single child, rather than treating them as their
/// own construct.
fn is_transparent_wrapper(kind: clang_sys::CXCursorKind) -> bool {
    matches!(
        kind,
        clang_sys::CXCursor_UnexposedExpr | clang_sys::CXCursor_ParenExpr
    )
}

unsafe fn lower_expr(cursor: clang_sys::CXCursor, project_root: &Path) -> ir::Expr {
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
    let origin = stmt_origin(cursor, project_root);

    if is_transparent_wrapper(kind) {
        let mut children = unsafe { collect_children(cursor) };
        if children.len() == 1 {
            let child_cursor = children.remove(0);
            let inner = unsafe { lower_expr(child_cursor, project_root) };

            // Most wrapper cursors really are pure sugar (parentheses, or an
            // lvalue-to-rvalue "load" that doesn't change the C++ type) —
            // the outer and child cursor types agree, and unwrapping
            // transparently is correct. But `libclang` reports an implicit
            // *numeric* conversion (C++'s usual arithmetic conversions, or
            // an `int` initializing a `double`) as this same
            // `UnexposedExpr` kind, wrapping a child of a genuinely
            // different type — confirmed via `clang -Xclang -ast-dump`
            // (`ImplicitCastExpr 'double' <IntegralToFloating>` wrapping an
            // `'int'` child). Discarding that mismatch instead of
            // preserving it as `Expr::Convert` is exactly the "compiles and
            // is wrong" failure `Type::Unsupported`/§5's "silêncio é
            // proibido" rule exists to prevent elsewhere in this module.
            let outer_ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
            let child_ty = lower_type(unsafe { clang_sys::clang_getCursorType(child_cursor) });
            return if outer_ty == child_ty {
                inner
            } else if child_ty == ir::Type::Int && outer_ty == ir::Type::Double {
                ir::Expr::Convert {
                    operand: Box::new(inner),
                    ty: ir::Type::Double,
                    origin,
                }
            } else {
                ir::Expr::Unsupported {
                    reason: format!(
                        "unsupported implicit conversion from {child_ty:?} to {outer_ty:?}"
                    ),
                    origin,
                }
            };
        }
        return ir::Expr::Unsupported {
            reason: format!("wrapper cursor kind {kind} did not have exactly one child"),
            origin,
        };
    }

    if kind == clang_sys::CXCursor_DeclRefExpr {
        let name =
            unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        return ir::Expr::Ref { name, ty, origin };
    }

    if kind == clang_sys::CXCursor_MemberRefExpr {
        let field =
            unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
        let children = unsafe { collect_children(cursor) };
        let [target_cursor] = children.as_slice() else {
            return ir::Expr::Unsupported {
                reason: format!(
                    "MemberRefExpr cursor had {} children, expected 1",
                    children.len()
                ),
                origin,
            };
        };
        let target = unsafe { lower_expr(*target_cursor, project_root) };
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        return ir::Expr::FieldAccess {
            target: Box::new(target),
            field,
            ty,
            origin,
        };
    }

    if kind == clang_sys::CXCursor_IntegerLiteral {
        return match unsafe { evaluate_int_eval_result(cursor) } {
            Some(value) => ir::Expr::IntLiteral { value, origin },
            None => ir::Expr::Unsupported {
                reason: "could not evaluate integer literal".to_owned(),
                origin,
            },
        };
    }

    if kind == clang_sys::CXCursor_CXXBoolLiteralExpr {
        return match unsafe { evaluate_int_eval_result(cursor) } {
            Some(value) => ir::Expr::BoolLiteral {
                value: value != 0,
                origin,
            },
            None => ir::Expr::Unsupported {
                reason: "could not evaluate bool literal".to_owned(),
                origin,
            },
        };
    }

    if kind == clang_sys::CXCursor_BinaryOperator {
        return unsafe { lower_binary_expr(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_UnaryOperator {
        return unsafe { lower_unary_expr(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_CallExpr {
        return unsafe { lower_call_expr(cursor, project_root, origin) };
    }

    ir::Expr::Unsupported {
        reason: format!("unsupported expression cursor kind {kind}"),
        origin,
    }
}

unsafe fn lower_binary_expr(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let operator_kind = unsafe { clang_sys::clang_getCursorBinaryOperatorKind(cursor) };
    let Some(op) = lower_binary_op(operator_kind) else {
        return ir::Expr::Unsupported {
            reason: format!("unsupported binary operator kind {operator_kind}"),
            origin,
        };
    };

    let children = unsafe { collect_children(cursor) };
    let [lhs_cursor, rhs_cursor] = children.as_slice() else {
        return ir::Expr::Unsupported {
            reason: format!(
                "binary operator cursor had {} children, expected 2",
                children.len()
            ),
            origin,
        };
    };

    let lhs = unsafe { lower_expr(*lhs_cursor, project_root) };
    let rhs = unsafe { lower_expr(*rhs_cursor, project_root) };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    ir::Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        ty,
        origin,
    }
}

unsafe fn lower_unary_expr(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let operator_kind = unsafe { clang_sys::clang_getCursorUnaryOperatorKind(cursor) };
    let Some(op) = lower_unary_op(operator_kind) else {
        return ir::Expr::Unsupported {
            reason: format!("unsupported unary operator kind {operator_kind}"),
            origin,
        };
    };

    let children = unsafe { collect_children(cursor) };
    let [operand_cursor] = children.as_slice() else {
        return ir::Expr::Unsupported {
            reason: format!(
                "unary operator cursor had {} children, expected 1",
                children.len()
            ),
            origin,
        };
    };

    let operand = unsafe { lower_expr(*operand_cursor, project_root) };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    ir::Expr::Unary {
        op,
        operand: Box::new(operand),
        ty,
        origin,
    }
}

/// Whether `cursor` is `libclang`'s implicit default-constructor call — the
/// synthetic initializer a record-typed `VarDecl` gets even when C++ source
/// writes no initializer at all (`Ponto p;`). Confirmed via
/// `clang -Xclang -ast-dump`, not guessed. `lower_decl_stmt` uses this to
/// tell "genuinely no initializer" apart from a real one, so it can emit
/// Dart's `late` instead of trying to lower a call to a constructor Dart
/// doesn't have.
unsafe fn is_default_construct_with_no_args(cursor: clang_sys::CXCursor) -> bool {
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_CallExpr {
        return false;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0
        || unsafe { clang_sys::clang_getCursorKind(referenced) } != clang_sys::CXCursor_Constructor
    {
        return false;
    }
    let is_default =
        unsafe { clang_sys::clang_CXXConstructor_isDefaultConstructor(referenced) != 0 };
    let has_no_args = unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) == 0 };
    is_default && has_no_args
}

/// `soma(2, 3)` / `fatorial(n - 1)` — resolves the callee the same way
/// `function_catalog::record_call` does (via `clang_getCursorReferenced`),
/// but only free functions are lowered as a `Call` node so far; methods and
/// templates are E04+/E08 scope. Arguments come from
/// `clang_Cursor_getNumArguments`/`clang_Cursor_getArgument` — the dedicated
/// call-argument API, which (unlike walking children by position) already
/// excludes the callee reference itself.
///
/// A copy/move constructor call is a second shape reachable here besides a
/// real function call: passing a `struct` by value (`mover(p, ...)`) or
/// returning one (`return p;`) wraps the real expression in a compiler
/// -inserted `CXXConstructExpr` — confirmed via `clang -Xclang -ast-dump`.
/// `libclang` reports its cursor kind as the same `CXCursor_CallExpr`, whose
/// `clang_getCursorReferenced` resolves to the constructor *declaration*
/// (`CXCursor_Constructor`) rather than a `CXCursor_FunctionDecl`. Treated
/// as transparent sugar (like `is_transparent_wrapper`, recursing into the
/// single real argument) rather than as a call: it isn't user-visible C++,
/// just bookkeeping, and `lower::cpp`'s own by-value-parameter clone
/// (`collect_params_with_clone_prelude`) is what actually implements the
/// copy semantics this bookkeeping exists to request.
unsafe fn lower_call_expr(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return ir::Expr::Unsupported {
            reason: "call target could not be resolved".to_owned(),
            origin,
        };
    }

    let referenced_kind = unsafe { clang_sys::clang_getCursorKind(referenced) };

    if referenced_kind == clang_sys::CXCursor_Constructor {
        let is_copy_or_move =
            unsafe { clang_sys::clang_CXXConstructor_isCopyConstructor(referenced) } != 0
                || unsafe { clang_sys::clang_CXXConstructor_isMoveConstructor(referenced) } != 0;
        let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) };
        if is_copy_or_move && arg_count == 1 {
            let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
            return unsafe { lower_expr(arg_cursor, project_root) };
        }
        return ir::Expr::Unsupported {
            reason: "unsupported constructor call (only implicit copy/move construction of a \
                      single value is supported so far)"
                .to_owned(),
            origin,
        };
    }

    if referenced_kind != clang_sys::CXCursor_FunctionDecl {
        return ir::Expr::Unsupported {
            reason: format!(
                "unsupported call target cursor kind {referenced_kind} \
                 (only free functions are lowered as calls so far)"
            ),
            origin,
        };
    }

    let callee_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced)) };
    if callee_usr.is_empty() {
        return ir::Expr::Unsupported {
            reason: "resolved call target has no stable identity".to_owned(),
            origin,
        };
    }
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };

    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) };
    if arg_count < 0 {
        return ir::Expr::Unsupported {
            reason: "could not enumerate call arguments".to_owned(),
            origin,
        };
    }
    let args = (0..arg_count)
        .map(|index| {
            let arg_cursor =
                unsafe { clang_sys::clang_Cursor_getArgument(cursor, index as c_uint) };
            unsafe { lower_expr(arg_cursor, project_root) }
        })
        .collect();

    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    ir::Expr::Call {
        callee_usr,
        callee_name,
        args,
        ty,
        origin,
    }
}

fn lower_binary_op(kind: clang_sys::CXBinaryOperatorKind) -> Option<ir::BinaryOp> {
    match kind {
        clang_sys::CXBinaryOperator_Add => Some(ir::BinaryOp::Add),
        clang_sys::CXBinaryOperator_Sub => Some(ir::BinaryOp::Sub),
        clang_sys::CXBinaryOperator_Mul => Some(ir::BinaryOp::Mul),
        clang_sys::CXBinaryOperator_Div => Some(ir::BinaryOp::Div),
        clang_sys::CXBinaryOperator_Rem => Some(ir::BinaryOp::Mod),
        clang_sys::CXBinaryOperator_LT => Some(ir::BinaryOp::Lt),
        clang_sys::CXBinaryOperator_LE => Some(ir::BinaryOp::Le),
        clang_sys::CXBinaryOperator_GT => Some(ir::BinaryOp::Gt),
        clang_sys::CXBinaryOperator_GE => Some(ir::BinaryOp::Ge),
        clang_sys::CXBinaryOperator_EQ => Some(ir::BinaryOp::Eq),
        clang_sys::CXBinaryOperator_NE => Some(ir::BinaryOp::Ne),
        clang_sys::CXBinaryOperator_LAnd => Some(ir::BinaryOp::And),
        _ => None,
    }
}

fn lower_unary_op(kind: clang_sys::CXUnaryOperatorKind) -> Option<ir::UnaryOp> {
    match kind {
        clang_sys::CXUnaryOperator_Minus => Some(ir::UnaryOp::Neg),
        _ => None,
    }
}

/// Shared by integer and boolean literals: both evaluate to `CXEval_Int`
/// via `clang_Cursor_Evaluate` (confirmed empirically — `libclang` has no
/// separate "evaluate as bool" entry point).
unsafe fn evaluate_int_eval_result(cursor: clang_sys::CXCursor) -> Option<i64> {
    let result = unsafe { clang_sys::clang_Cursor_Evaluate(cursor) };
    if result.is_null() {
        return None;
    }

    let kind = unsafe { clang_sys::clang_EvalResult_getKind(result) };
    let value = if kind == clang_sys::CXEval_Int {
        Some(unsafe { clang_sys::clang_EvalResult_getAsLongLong(result) })
    } else {
        None
    };

    unsafe {
        clang_sys::clang_EvalResult_dispose(result);
    }

    value
}
