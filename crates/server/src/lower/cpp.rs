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

use std::cell::RefCell;
use std::ffi::c_void;
use std::os::raw::c_uint;
use std::path::Path;

use crate::ir;
use crate::mapping;
use crate::type_catalog;

/// The Dart type name a monomorphization suffix uses — appended,
/// capitalized, to a template's base name (`dobro` + `[Int]` params →
/// `dobroInt`, E08) or an overload's base name (`formatarValor` + `[Int]`
/// params → `formatarValorInt`, E07). One implementation shared by both
/// naming schemes (`monomorphized_template_name` here,
/// `function_catalog::dart_overload_name`) rather than two that could drift
/// out of agreement.
pub fn overload_type_suffix(ty: &ir::Type) -> String {
    match ty {
        ir::Type::Int => "Int".to_owned(),
        ir::Type::Bool => "Bool".to_owned(),
        ir::Type::Double => "Double".to_owned(),
        ir::Type::Void => "Void".to_owned(),
        ir::Type::Str => "String".to_owned(),
        ir::Type::Bytes => "Bytes".to_owned(),
        ir::Type::List(element) => format!("List{}", overload_type_suffix(element)),
        ir::Type::Set(element) => format!("Set{}", overload_type_suffix(element)),
        ir::Type::Map(key, value) => format!(
            "Map{}{}",
            overload_type_suffix(key),
            overload_type_suffix(value)
        ),
        ir::Type::Pair(first, second) => format!(
            "Pair{}{}",
            overload_type_suffix(first),
            overload_type_suffix(second)
        ),
        ir::Type::ListCursor(element) => format!("ListCursor{}", overload_type_suffix(element)),
        ir::Type::Callback {
            return_type,
            params,
        } => format!(
            "Callback{}{}",
            overload_type_suffix(return_type),
            params.iter().map(overload_type_suffix).collect::<String>()
        ),
        ir::Type::Record { name, .. } | ir::Type::Enum { name, .. } => name.clone(),
        ir::Type::Tuple(elements) => elements.iter().map(overload_type_suffix).collect(),
        ir::Type::Nullable(inner) => format!("Nullable{}", overload_type_suffix(inner)),
        ir::Type::Object => "Object".to_owned(),
        // Achado 1 restante (`docs/plans/diagnostico-verovio-6.2.0.md`): a
        // fixed `"Unsupported"` suffix here made every unrepresentable
        // parameter type indistinguishable from every other one — two
        // overloads whose only difference was, say, `int*` vs. `double*`
        // (both `Unsupported`, case C01, never bridged) computed the exact
        // same renamed Dart name and collided (`duplicate_definition`),
        // confirmed live in the Verovio 6.2.0 corpus after the
        // arity-and-return-type fix above (`CalculateDotLocations`,
        // `GetCrossStaffExtremes`). The C++ spelling itself is real,
        // distinguishing information the suffix was discarding — folded in
        // here via `pascal_case_alnum_segments` (alphanumeric runs only, so
        // `*`/`<`/`>`/`,`/whitespace in the spelling can't produce an
        // invalid Dart identifier fragment).
        ir::Type::Unsupported(spelling) => {
            let sanitized = pascal_case_alnum_segments(spelling);
            if sanitized.is_empty() {
                "Unsupported".to_owned()
            } else {
                format!("Unsupported{sanitized}")
            }
        }
    }
}

/// `"int *"` → `"IntPointer"`-style fragments (here, just `"Int"` — the
/// caller adds context); any run of non-alphanumeric characters is a
/// separator, dropped, and each alphanumeric run gets its first letter
/// uppercased and is joined with no separator — the same PascalCase-join
/// shape `function_catalog::pascal_case_namespace` uses for a `::`-only
/// separator, generalized here to arbitrary C++ type-spelling punctuation
/// (`*`, `<>`, `,`, whitespace) so the result is always a valid fragment of
/// a Dart identifier.
fn pascal_case_alnum_segments(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// The deterministic Dart-side name for a call/declaration that resolves to
/// *any* instantiation of a function template (E08) — `cursor` is the
/// concrete instantiation itself (an implicit instantiation's synthesized
/// decl, or a full explicit specialization's own written decl; both report
/// concrete, non-template-dependent parameter types, confirmed empirically
/// against `clang_getCursorType` on each). Appends `overload_type_suffix`
/// of every parameter's *concrete* type to `base_name` — the same scheme
/// E07 uses for a renamed overload, and for the same reason: Dart has
/// neither C++ template instantiation nor (usable, for this project's
/// scope) generic type parameters with the operator-based constraints a
/// template body like `valor + valor` implicitly relies on, so each
/// concrete instantiation becomes its own named Dart function instead.
/// Called independently at both the declaration this resolves to
/// (`function_catalog::record_call`'s synthesis, or the top-level
/// `FreeFunction` path for an explicit specialization) and every call site
/// referencing it (`lower_call_expr`) — deterministic from `cursor` alone,
/// so the two can never disagree about the name.
pub fn monomorphized_template_name(base_name: &str, cursor: clang_sys::CXCursor) -> String {
    let mut name = base_name.to_owned();
    for param_cursor in unsafe { collect_children(cursor) } {
        if unsafe { clang_sys::clang_getCursorKind(param_cursor) } != clang_sys::CXCursor_ParmDecl {
            continue;
        }
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(param_cursor) });
        name.push_str(&overload_type_suffix(&ty));
    }
    name
}

/// The 0-based parameter positions, among `cursor`'s own declared
/// parameters, of every non-`const` scalar (`Int`/`Double`/`Bool`)
/// reference or pointer — C++'s "out parameter" idiom, both its reference
/// form (`void f(int &out)`) and its pointer form (`void f(int *out)`,
/// the older/C-compatible spelling — confirmed the *dominant* real shape
/// of a bare mutable scalar pointer *parameter* by grepping the actual
/// Verovio source directly, round 19: `editortoolkit_neume.h`'s
/// `ParseDragAction(..., int *x, int *y)`, `win_getopt.h`'s `int *idx`,
/// `zip_file.hpp`'s `mz_uint32 *pIndex` — every real example found was a
/// single scalar write-back, never an indexed buffer — the same
/// "non-const scalar reference is an out-param, not aliased state" bar
/// `is_non_const_scalar_out_param_type` already applied unconditionally
/// to the reference form. Never generalized past a bare scalar (a
/// reference/pointer to `Record`/`Str`/`List`/`Bytes` is left alone; those
/// already have their own precise representations that a blind pointer
/// buffer would misrepresent). `cursor` may be a function/method
/// *declaration* (`clang_Cursor_getNumArguments`/`getArgument` work on a
/// declaration exactly like on a call, per `lower_call_arguments`'s own
/// doc comment) — used both by `apply_out_param_bridge`, from the
/// declaration being lowered, and independently by `call_out_param_arg_indices`,
/// from a *call*'s resolved callee — so a call site and its callee can
/// never disagree about which parameters were bridged. Empty (the
/// overwhelmingly common case) for anything else, including a `const`
/// reference/pointer (E05's own by-reference `std::string`/`std::vector`
/// parameters, read-only, correctly untouched by this).
unsafe fn out_param_indices(cursor: clang_sys::CXCursor) -> Vec<usize> {
    let count = unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) };
    if count <= 0 {
        return Vec::new();
    }
    (0..count as c_uint)
        .filter(|&index| {
            let param_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, index) };
            let cx_type = unsafe { clang_sys::clang_getCursorType(param_cursor) };
            unsafe { is_non_const_scalar_out_param_type(cx_type) }
        })
        .map(|index| index as usize)
        .collect()
}

/// Whether every one of `cursor`'s own out-param `indices` is the *pointer*
/// form (`int *out`), never the reference form (`int &out`) — the exact
/// eligibility bar `apply_out_param_bridge` applies before bridging a
/// non-`void`-returning function/method (see its own doc comment on why:
/// a reference out-param's call-site argument is a bare name, so an
/// unrecognized call to a non-`void` reference-bridged function would
/// silently keep assuming the callee's *original* scalar return type
/// instead of the tuple it was never actually rewritten to return —
/// `call_out_param_arg_indices` needs this exact same bar for a call
/// site to never disagree with whether its callee's *declaration* was
/// actually bridged (F8/tarefa 10, real trigger `Verse::AdjustPosition(int
/// &overlap, int freeSpace, const Doc *doc)`: non-`void` return, reference-
/// form out-param — `apply_out_param_bridge` correctly leaves its
/// declaration alone, but before this check existed, `call_out_param_arg_
/// indices` recognized the call anyway, producing a `(overlap,) = ...`
/// destructure against a callee that still just returns a bare `int`).
/// `cursor` may be either the declaration being bridged or a call's
/// resolved callee, exactly like `out_param_indices` itself.
unsafe fn out_param_indices_are_all_pointer_form(
    cursor: clang_sys::CXCursor,
    indices: &[usize],
) -> bool {
    indices.iter().all(|&index| {
        let param_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, index as c_uint) };
        unsafe { clang_sys::clang_getCursorType(param_cursor) }.kind == clang_sys::CXType_Pointer
    })
}

unsafe fn is_non_const_scalar_out_param_type(cx_type: clang_sys::CXType) -> bool {
    if cx_type.kind != clang_sys::CXType_LValueReference
        && cx_type.kind != clang_sys::CXType_Pointer
    {
        return false;
    }
    let pointee = unsafe { clang_sys::clang_getPointeeType(cx_type) };
    if unsafe { clang_sys::clang_isConstQualifiedType(pointee) } != 0 {
        return false;
    }
    // A pointer (never a reference — `mz_uint8&`/`char&` isn't a shape
    // this corpus uses) whose pointee already has a *more specific*
    // representation of its own — a named byte-buffer alias
    // (`mz_uint8`/`uint8_t`, real trigger:
    // `a_known_byte_buffer_pointer_lowers_to_a_nullable_uint8_list`'s own
    // `mz_uint8* output`) or a text character type (`char`/`wchar_t`/...)
    // — is that representation (`Bytes`/`Str` via `lower_type`'s own
    // pointer branch), never a bare-scalar out-param: both alternatives
    // lower `pointee` to `Type::Int` on its own (a byte/character *is* a
    // small integer), which would otherwise make this indistinguishable
    // from a genuine `int*`/`size_t*` out-param.
    if cx_type.kind == clang_sys::CXType_Pointer {
        let pointee_spelling =
            unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(pointee)) };
        if is_known_byte_buffer_type(&pointee_spelling)
            || unsafe { is_text_character_type(pointee) }
        {
            return false;
        }
    }
    matches!(
        lower_type(pointee),
        ir::Type::Int | ir::Type::Double | ir::Type::Bool
    )
}

// Scoped registry of *pointer*-shaped out-param names currently standing
// for their pointee's own type, while a function/method body that has one
// is being lowered — round 19's pointer form of the out-param bridge.
// Unlike the reference form (`int &out`, which C++ itself never lets the
// body write through explicit `*`/`&` syntax — `out` already reads exactly
// like an ordinary local), the pointer form (`int *out`) is written and
// read through explicit `*out`/`&out` in the body, and `lower_unary_expr`'s
// `Deref`/`AddrOf` handling normally keys entirely off `lower_type` on the
// operand's own *declared* C++ type (`int *`, still `Type::Unsupported` —
// `out_param_indices`'s parameter-type rewrite in `apply_out_param_bridge`
// only touches the `ir::Param` list, built *before* body lowering, not the
// raw clang type `lower_unary_expr` re-derives independently). Same
// thread-local-stack shape as `ACTIVE_ITERATOR_LOOPS` (round 18) and the
// same reasoning for why: `lower_stmt`/`lower_expr` have no context
// parameter to carry this through, and each compilation unit lowers on its
// own worker thread.
thread_local! {
    static ACTIVE_POINTER_OUT_PARAMS: RefCell<Vec<(String, ir::Type)>> =
        const { RefCell::new(Vec::new()) };
}

/// The pointer-shaped (not reference-shaped — those need no interception,
/// see the registry's own doc comment) out-params of `cursor`, as
/// `(dart_name, pointee_type)` pairs ready to push onto
/// `ACTIVE_POINTER_OUT_PARAMS` before lowering its body.
unsafe fn pointer_out_param_bindings(cursor: clang_sys::CXCursor) -> Vec<(String, ir::Type)> {
    unsafe { out_param_indices(cursor) }
        .into_iter()
        .filter_map(|index| {
            let param_cursor =
                unsafe { clang_sys::clang_Cursor_getArgument(cursor, index as c_uint) };
            let cx_type = unsafe { clang_sys::clang_getCursorType(param_cursor) };
            if cx_type.kind != clang_sys::CXType_Pointer {
                return None;
            }
            let name = dart_safe_identifier(&unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(param_cursor))
            });
            let pointee_ty = lower_type(unsafe { clang_sys::clang_getPointeeType(cx_type) });
            Some((name, pointee_ty))
        })
        .collect()
}

fn push_active_pointer_out_params(bindings: &[(String, ir::Type)]) {
    ACTIVE_POINTER_OUT_PARAMS.with(|stack| {
        stack.borrow_mut().extend(bindings.iter().cloned());
    });
}

fn pop_active_pointer_out_params(count: usize) {
    ACTIVE_POINTER_OUT_PARAMS.with(|stack| {
        let mut stack = stack.borrow_mut();
        let new_len = stack.len().saturating_sub(count);
        stack.truncate(new_len);
    });
}

// The `usr` of the record whose method/constructor/destructor body is
// currently being lowered (F12/tarefa 09) — same thread-local-stack shape and
// same reasoning as `ACTIVE_POINTER_OUT_PARAMS` just above: `lower_expr` has
// no context parameter to carry this through, and `lower_method_call` needs
// it to tell a genuinely self-qualified call (`Foo::f()` written inside
// `Foo::f()` itself — still recursive in Dart exactly as it is in C++, left
// alone) apart from a qualified call to a *different*, ancestor record
// (`Base::f()` from inside a derived override — the shape that recurses
// forever if lowered the same way, `docs/prompts/
// 2026-08-21-09-chamada-a-base-qualificada.md`'s whole reason to exist).
thread_local! {
    static ACTIVE_METHOD_OWNER_USRS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

fn push_active_method_owner_usr(usr: String) {
    ACTIVE_METHOD_OWNER_USRS.with(|stack| stack.borrow_mut().push(usr));
}

fn pop_active_method_owner_usr() {
    ACTIVE_METHOD_OWNER_USRS.with(|stack| {
        stack.borrow_mut().pop();
    });
}

fn active_method_owner_usr() -> Option<String> {
    ACTIVE_METHOD_OWNER_USRS.with(|stack| stack.borrow().last().cloned())
}

fn active_pointer_out_param_type(name: &str) -> Option<ir::Type> {
    ACTIVE_POINTER_OUT_PARAMS.with(|stack| {
        stack
            .borrow()
            .iter()
            .rev()
            .find(|(active_name, _)| active_name == name)
            .map(|(_, ty)| ty.clone())
    })
}

/// Rewrites `return_type`/`body` in place to bridge C++'s "out parameter"
/// idiom to a Dart record return (`ir::Type::Tuple`) — E10 flagged the idea
/// and deliberately didn't build it; E13's `Fraction::Reduce(int&, int&)`
/// forces it. Only triggers for a `void`-returning function/method with at
/// least one qualifying reference parameter (`out_param_indices`); every
/// other function/method is untouched, and this is called unconditionally
/// from both `lower_function` and `lower_method` for exactly that reason.
unsafe fn apply_out_param_bridge(
    cursor: clang_sys::CXCursor,
    params: &mut [ir::Param],
    return_type: &mut ir::Type,
    body: &mut Vec<ir::Stmt>,
    origin: &ir::Origin,
) {
    let indices = unsafe { out_param_indices(cursor) };
    if indices.is_empty() {
        return;
    }
    // A real out-param function very often isn't `void` — a `bool`/status
    // return alongside the out-params is the *dominant* shape in the real
    // Verovio corpus (round 19/20, real trigger: `editortoolkit_neume.h`'s
    // `bool ParseDragAction(..., int *x, int *y)`), not an edge case the
    // original `void`-only scope correctly deferred. Bridging a non-`void`
    // function is scoped to the *pointer* form of out-param specifically
    // (`unsafe { is_pointer_out_param(cursor, index) }` below), never the
    // reference form: a reference out-param's call-site argument is a bare
    // name (`Reduce(numerador, denominador)`, no `&`) that lowers cleanly
    // through the *ordinary* expression path in any context this module
    // doesn't specifically recognize as a bridged call (an `if` condition,
    // a nested boolean expression, ...) — which would silently assume the
    // callee still returns its original scalar type instead of the new
    // tuple, a real type mismatch this module has no way to catch from a
    // single function's own lowering. A *pointer* out-arg has no such
    // risk: `&x` for a bare scalar always lowers to an honest
    // `Unsupported` in any context other than the ones this module
    // specifically unwraps it in (`lower_unary_expr`'s `AddrOf` case has
    // no `Known` shape for a scalar pointee) — so an unrecognized call
    // site to a non-`void` pointer-bridged function fails safely (an
    // honest bailout on its own arguments), never a clean-looking call to
    // the wrong Dart signature.
    let had_void_return = *return_type == ir::Type::Void;
    if !had_void_return && !unsafe { out_param_indices_are_all_pointer_form(cursor, &indices) } {
        return;
    }

    // The reference form of an out-param already gets the right Dart
    // parameter type for free: `lower_type`'s `CXType_LValueReference`
    // branch unwraps straight to the pointee's own lowered type (a
    // reference can't be null, so no `Nullable` wrapper). The pointer
    // form doesn't: `lower_type`'s `CXType_Pointer` branch has no `Known`
    // shape for a bare scalar pointee (only object/collection pointees
    // do), so it fell through to `Type::Unsupported` before
    // `out_param_indices` above ever got a say — this is the one place
    // that already knows "this specific parameter is a recognized
    // out-param", so it's the right place to correct it, by re-deriving
    // the type directly from the pointee rather than trusting whatever
    // `collect_params_with_clone_prelude` already put in `params[index]`.
    for &index in &indices {
        let param_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, index as c_uint) };
        let cx_type = unsafe { clang_sys::clang_getCursorType(param_cursor) };
        if cx_type.kind == clang_sys::CXType_Pointer {
            params[index].ty = lower_type(unsafe { clang_sys::clang_getPointeeType(cx_type) });
        }
    }

    replace_returns_with_out_param_tuple(body, &indices, params, origin);
    // A bare `return;`/fall-through only needs synthesizing for a `void`
    // function — every path of a well-formed non-`void` C++ function
    // already has an explicit `return value;` (falling off the end
    // without one is undefined behavior a real compiler would already
    // reject/warn on), which `replace_returns_with_out_param_tuple` above
    // already rewrote in place; adding another one here would return a
    // tuple missing its own first (original-return-value) slot.
    if had_void_return {
        body.push(ir::Stmt::Return {
            value: Some(ir::Expr::Tuple {
                values: out_param_tuple_values(&indices, params, origin),
                origin: origin.clone(),
            }),
            origin: origin.clone(),
        });
    }

    let mut tuple_types: Vec<ir::Type> = if had_void_return {
        Vec::new()
    } else {
        vec![return_type.clone()]
    };
    tuple_types.extend(indices.iter().map(|&index| params[index].ty.clone()));
    *return_type = ir::Type::Tuple(tuple_types);
}

fn out_param_tuple_values(
    indices: &[usize],
    params: &[ir::Param],
    origin: &ir::Origin,
) -> Vec<ir::Expr> {
    indices
        .iter()
        .map(|&index| ir::Expr::Ref {
            name: params[index].name.clone(),
            ty: params[index].ty.clone(),
            origin: origin.clone(),
        })
        .collect()
}

/// Every `return`, in a bridged function/method, needs its value (if any)
/// folded into the out-param tuple, in addition to the out-param values
/// themselves — a bare `return;` (the `void` case: the out-param values
/// alone become the tuple) or `return value;` (the non-`void` case, real
/// trigger `editortoolkit_neume.h`'s `bool ParseDragAction(...)`: `value`
/// becomes the tuple's own first slot, out-param values following it, the
/// same order `apply_out_param_bridge` builds the return *type* tuple in).
/// Walks every nested `if`/`while`/`for`/`try` block; every other
/// statement shape is left alone.
fn replace_returns_with_out_param_tuple(
    stmts: &mut [ir::Stmt],
    indices: &[usize],
    params: &[ir::Param],
    origin: &ir::Origin,
) {
    for stmt in stmts {
        replace_return_with_out_param_tuple(stmt, indices, params, origin);
    }
}

fn replace_return_with_out_param_tuple(
    stmt: &mut ir::Stmt,
    indices: &[usize],
    params: &[ir::Param],
    origin: &ir::Origin,
) {
    match stmt {
        ir::Stmt::Return { value, .. } => {
            let mut tuple_values = out_param_tuple_values(indices, params, origin);
            if let Some(original_value) = value.take() {
                tuple_values.insert(0, original_value);
            }
            *value = Some(ir::Expr::Tuple {
                values: tuple_values,
                origin: origin.clone(),
            });
        }
        ir::Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            replace_returns_with_out_param_tuple(then_branch, indices, params, origin);
            replace_returns_with_out_param_tuple(else_branch, indices, params, origin);
        }
        ir::Stmt::While { body, .. } => {
            replace_returns_with_out_param_tuple(body, indices, params, origin)
        }
        ir::Stmt::DoWhile { body, .. } => {
            replace_returns_with_out_param_tuple(body, indices, params, origin)
        }
        ir::Stmt::For {
            init,
            increment,
            body,
            ..
        } => {
            if let Some(init) = init {
                replace_return_with_out_param_tuple(init, indices, params, origin);
            }
            if let Some(increment) = increment {
                replace_return_with_out_param_tuple(increment, indices, params, origin);
            }
            replace_returns_with_out_param_tuple(body, indices, params, origin);
        }
        ir::Stmt::ForEach { body, .. } => {
            replace_returns_with_out_param_tuple(body, indices, params, origin)
        }
        ir::Stmt::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            replace_returns_with_out_param_tuple(try_body, indices, params, origin);
            replace_returns_with_out_param_tuple(catch_body, indices, params, origin);
        }
        ir::Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            replace_returns_with_out_param_tuple(try_body, indices, params, origin);
            replace_returns_with_out_param_tuple(finally_body, indices, params, origin);
        }
        ir::Stmt::Switch { cases, default, .. } => {
            for case in cases {
                replace_returns_with_out_param_tuple(&mut case.body, indices, params, origin);
            }
            if let Some(default) = default {
                replace_returns_with_out_param_tuple(default, indices, params, origin);
            }
        }
        ir::Stmt::VarDecl { .. }
        | ir::Stmt::Assign { .. }
        | ir::Stmt::FieldAssign { .. }
        | ir::Stmt::ExprAssign { .. }
        | ir::Stmt::ExprStmt { .. }
        | ir::Stmt::Break { .. }
        | ir::Stmt::Continue { .. }
        | ir::Stmt::ContinueLabel { .. }
        | ir::Stmt::Throw { .. }
        | ir::Stmt::TupleAssign { .. }
        | ir::Stmt::Unsupported { .. } => {}
    }
}

/// F8/tarefa 10's last gap: a caller-side local declared with no C++
/// initializer (`int x;`), later reused by an out-param-bridged call as
/// *both* an input argument and a destructuring target (`GetBoundingBox(x,
/// y, w, h)` / `(x, y, w, h) = ...` — the same reuse `lower_stmt`'s own
/// `Stmt::TupleAssign` construction always does, correct for a callee that
/// genuinely reads-and-modifies its out-param, like `Fraction::
/// ReduceStatic`'s `num = num / 2`). `emit::dart` turns a no-initializer
/// `VarDecl` into `late T name;`, deferring initialization to first use —
/// correct for a local truly untouched until a later statement, but wrong
/// here: the call reads each local's value (as an argument) before its own
/// destructuring assignment ever writes to it, a real read of a still-
/// unassigned `late` local (`definitely_unassigned_late_local_variable`,
/// real trigger `Doc::GetGlyphHeight`'s `int x; int y; int w; int h;
/// Resources resources = GetResources(); Glyph *glyph =
/// resources.GetGlyph(code); ...GetBoundingBox(x, y, w, h);` — note the
/// unrelated declarations sitting *between* the out-param locals and the
/// call, ruling out an adjacency-only scan). A neutral default value
/// (`default_scalar_value`, the same stand-in `default_field_value`
/// already gives an uninitialized field) is exactly as safe as whatever
/// indeterminate value C++ itself would have left there. Scans forward
/// from each no-initializer declaration for the first later statement, at
/// the same nesting level, that either bridges it (patch and stop) or
/// plainly reassigns it first (`Stmt::Assign`/`Stmt::ExprAssign` naming it
/// — already gets a real value before any risky read, so `late` was
/// already fine; stop without patching) — anything else in between (an
/// unrelated declaration, an unrelated call, ...) is simply skipped over.
fn neutralize_out_param_call_input_locals(stmts: &mut [ir::Stmt]) {
    for decl_index in 0..stmts.len() {
        let ir::Stmt::VarDecl {
            name, init: None, ..
        } = &stmts[decl_index]
        else {
            continue;
        };
        let name = name.clone();
        let mut qualifies = false;
        for peek in &stmts[decl_index + 1..] {
            match peek {
                ir::Stmt::Assign {
                    name: assigned_name,
                    ..
                } if *assigned_name == name => break,
                ir::Stmt::ExprAssign {
                    target:
                        ir::Expr::Ref {
                            name: target_name, ..
                        },
                    ..
                } if *target_name == name => break,
                ir::Stmt::TupleAssign {
                    targets,
                    value: ir::Expr::Call { args, .. },
                    ..
                } => {
                    let is_call_input = args.iter().any(
                        |arg| matches!(arg, ir::Expr::Ref { name: arg_name, .. } if *arg_name == name),
                    );
                    let is_destructure_target = targets.iter().any(
                        |target| matches!(target, ir::Expr::Ref { name: target_name, .. } if *target_name == name),
                    );
                    if is_call_input && is_destructure_target {
                        qualifies = true;
                        break;
                    }
                }
                _ => {}
            }
        }
        if qualifies {
            let ir::Stmt::VarDecl {
                ty, init, origin, ..
            } = &mut stmts[decl_index]
            else {
                unreachable!("re-matched the same VarDecl just inspected above")
            };
            *init = Some(default_scalar_value(ty, origin));
        }
    }
    for stmt in stmts.iter_mut() {
        recurse_neutralize_out_param_call_input_locals(stmt);
    }
}

fn recurse_neutralize_out_param_call_input_locals(stmt: &mut ir::Stmt) {
    match stmt {
        ir::Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            neutralize_out_param_call_input_locals(then_branch);
            neutralize_out_param_call_input_locals(else_branch);
        }
        ir::Stmt::While { body, .. } | ir::Stmt::DoWhile { body, .. } => {
            neutralize_out_param_call_input_locals(body)
        }
        ir::Stmt::For { body, .. } => neutralize_out_param_call_input_locals(body),
        ir::Stmt::ForEach { body, .. } => neutralize_out_param_call_input_locals(body),
        ir::Stmt::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            neutralize_out_param_call_input_locals(try_body);
            neutralize_out_param_call_input_locals(catch_body);
        }
        ir::Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            neutralize_out_param_call_input_locals(try_body);
            neutralize_out_param_call_input_locals(finally_body);
        }
        ir::Stmt::Switch { cases, default, .. } => {
            for case in cases {
                neutralize_out_param_call_input_locals(&mut case.body);
            }
            if let Some(default) = default {
                neutralize_out_param_call_input_locals(default);
            }
        }
        ir::Stmt::Return { .. }
        | ir::Stmt::VarDecl { .. }
        | ir::Stmt::Assign { .. }
        | ir::Stmt::FieldAssign { .. }
        | ir::Stmt::ExprAssign { .. }
        | ir::Stmt::ExprStmt { .. }
        | ir::Stmt::Break { .. }
        | ir::Stmt::Continue { .. }
        | ir::Stmt::ContinueLabel { .. }
        | ir::Stmt::Throw { .. }
        | ir::Stmt::TupleAssign { .. }
        | ir::Stmt::Unsupported { .. } => {}
    }
}

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

    let mut return_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(cursor) });
    let (mut params, clone_prelude) =
        unsafe { collect_params_with_clone_prelude(cursor, &origin, project_root) };
    let body_cursor = unsafe { find_compound_stmt_child(cursor) };
    let pointer_out_params = unsafe { pointer_out_param_bindings(cursor) };
    push_active_pointer_out_params(&pointer_out_params);
    let mut body = match body_cursor {
        Some(compound) => unsafe { lower_compound_stmt(compound, project_root) },
        None => Vec::new(),
    };
    pop_active_pointer_out_params(pointer_out_params.len());
    body.splice(0..0, clone_prelude);
    neutralize_out_param_call_input_locals(&mut body);
    unsafe { apply_out_param_bridge(cursor, &mut params, &mut return_type, &mut body, &origin) };

    // F15/tarefa 15.9: real C++ requires *some* free function literally
    // named `main` with one of exactly two shapes (`main()`/`main(int,
    // char **)`, C's third form `char *argv[]` decaying to the same
    // pointer-to-pointer type) to link at all — matching on name plus this
    // arity/first-param-type shape is precise enough that a false positive
    // would need an unrelated free function coincidentally sharing both,
    // cheaper than proving global scope (which this function has no ready
    // access to).
    if name == "main" && (params.is_empty() || (params.len() == 2 && params[0].ty == ir::Type::Int))
    {
        apply_main_entry_point_signature(&mut params, &mut return_type, &mut body, &origin);
    }

    Some(ir::Function {
        name,
        usr: usr.to_owned(),
        params,
        return_type,
        body,
        origin,
    })
}

/// Rewrites a C `main`'s signature to Dart's own entry-point shape (F15/
/// tarefa 15.9: `void main()`/`void main(List<String> args)`, the only two
/// Dart accepts — `main_first_positional_parameter_type` otherwise). When
/// the original had parameters, prepends a prologue binding the *original*
/// `argc`/`argv` names to `args.length`/`args`, so every reference already
/// lowered inside `body` (resolved against the real C++ parameter cursors
/// during the ordinary body-lowering pass above, unaware of this rewrite)
/// keeps working unchanged under its own original name. Also rewrites every
/// `return <value>;` in `body` to a bare `return;`
/// (`strip_return_values`): a C exit code has no Dart `void main()`
/// equivalent to carry it to (that would need `dart:io`'s `exit()`, a
/// different, bigger feature this task doesn't ask for) — dropping it is
/// the accepted cost of choosing `void main()`, not a silent type-mapping
/// gap.
fn apply_main_entry_point_signature(
    params: &mut Vec<ir::Param>,
    return_type: &mut ir::Type,
    body: &mut Vec<ir::Stmt>,
    origin: &ir::Origin,
) {
    *return_type = ir::Type::Void;
    strip_return_values(body);
    if params.is_empty() {
        return;
    }
    let argc_name = params[0].name.clone();
    let argv_name = params.get(1).map(|param| param.name.clone());
    params.clear();
    params.push(ir::Param {
        name: "args".to_owned(),
        ty: ir::Type::List(Box::new(ir::Type::Str)),
        default_value: None,
    });
    let args_ref = || ir::Expr::Ref {
        name: "args".to_owned(),
        ty: ir::Type::List(Box::new(ir::Type::Str)),
        origin: origin.clone(),
    };
    let mut prologue = vec![ir::Stmt::VarDecl {
        name: argc_name,
        ty: ir::Type::Int,
        init: Some(ir::Expr::FieldAccess {
            target: Box::new(args_ref()),
            field: "length".to_owned(),
            ty: ir::Type::Int,
            origin: origin.clone(),
        }),
        origin: origin.clone(),
    }];
    if let Some(argv_name) = argv_name {
        prologue.push(ir::Stmt::VarDecl {
            name: argv_name,
            ty: ir::Type::List(Box::new(ir::Type::Str)),
            init: Some(args_ref()),
            origin: origin.clone(),
        });
    }
    body.splice(0..0, prologue);
}

/// Recursively rewrites every `return <value>;` in `stmts` (and any nested
/// branch/loop/switch/try body) to a bare `return;` —
/// `apply_main_entry_point_signature`'s own doc comment on why.
fn strip_return_values(stmts: &mut [ir::Stmt]) {
    for stmt in stmts {
        match stmt {
            ir::Stmt::Return { value, .. } => *value = None,
            ir::Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                strip_return_values(then_branch);
                strip_return_values(else_branch);
            }
            ir::Stmt::While { body, .. } | ir::Stmt::DoWhile { body, .. } => {
                strip_return_values(body);
            }
            ir::Stmt::For { body, .. } | ir::Stmt::ForEach { body, .. } => {
                strip_return_values(body);
            }
            ir::Stmt::TryCatch {
                try_body,
                catch_body,
                ..
            } => {
                strip_return_values(try_body);
                strip_return_values(catch_body);
            }
            ir::Stmt::TryFinally {
                try_body,
                finally_body,
                ..
            } => {
                strip_return_values(try_body);
                strip_return_values(finally_body);
            }
            ir::Stmt::Switch { cases, default, .. } => {
                for case in cases.iter_mut() {
                    strip_return_values(&mut case.body);
                }
                if let Some(default) = default {
                    strip_return_values(default);
                }
            }
            ir::Stmt::VarDecl { .. }
            | ir::Stmt::Assign { .. }
            | ir::Stmt::FieldAssign { .. }
            | ir::Stmt::ExprAssign { .. }
            | ir::Stmt::Break { .. }
            | ir::Stmt::Continue { .. }
            | ir::Stmt::ContinueLabel { .. }
            | ir::Stmt::ExprStmt { .. }
            | ir::Stmt::Throw { .. }
            | ir::Stmt::TupleAssign { .. }
            | ir::Stmt::Unsupported { .. } => {}
        }
    }
}

/// A minimal `ir::Function` for a system-header free function (libc/POSIX
/// — `memset`, `fclose`, ...) that F6/tarefa 07's Metade B just cataloged
/// as an external boundary (`function_catalog::
/// catalog_system_header_free_function_call`). `lower_function` itself
/// can't be reused here: it requires `type_catalog::cursor_site`, which
/// unconditionally refuses every system-header location (by design — see
/// that function's own doc comment on why the ordinary top-level
/// declaration walk never catalogs one at all). `origin` is built by the
/// caller from `type_catalog::system_header_cursor_site` instead.
///
/// The body is always the same honest "declared but never defined in any
/// compilation unit of this project" bailout the in-project
/// uncataloged-prototype path already gives (`function_catalog::
/// visit_cursor`'s own `is_uncatalogued_free_prototype` branch) — never
/// actually reached when this usr ends up in the effective external set
/// (`emit::dart`'s `MockContext` derives a mock body straight from
/// `return_type`, ignoring `body` entirely), but kept honest for the case
/// the user manually excludes this auto-detected candidate
/// (`docs/plans/lista-de-externos.md` decision 4/5).
pub(crate) unsafe fn lower_system_header_free_function_mock(
    referenced: clang_sys::CXCursor,
    usr: &str,
    origin: ir::Origin,
) -> ir::Function {
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    let return_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(referenced) });
    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(referenced) };
    let params = if arg_count < 0 {
        Vec::new()
    } else {
        (0..arg_count as c_uint)
            .map(|index| {
                let param_cursor =
                    unsafe { clang_sys::clang_Cursor_getArgument(referenced, index) };
                let raw_name = unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(
                        param_cursor,
                    ))
                };
                let name = if raw_name.is_empty() {
                    format!("arg{index}")
                } else {
                    dart_safe_identifier(&raw_name)
                };
                ir::Param {
                    name,
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(param_cursor) }),
                    default_value: None,
                }
            })
            .collect()
    };
    ir::Function {
        name,
        usr: usr.to_owned(),
        params,
        return_type,
        body: vec![ir::Stmt::Unsupported {
            reason: "declared but never defined in any compilation unit of this project".to_owned(),
            origin: origin.clone(),
        }],
        origin,
    }
}

/// Lowers a method's *definition* cursor into IR — called from
/// `function_catalog::visit_cursor` for a `CXXMethodDecl` with a body
/// (inline or out-of-line), the same way `lower_function` handles a free
/// function. `is_static` is read straight off the cursor rather than passed
/// in — the caller already needed it to route here in the first place, but
/// re-reading is one cheap `libclang` call, not worth threading as a
/// parameter.
/// The Dart method name given to a C++ conversion operator whose target is
/// `std::string` (`operator std::string() const`) — its real C++ spelling
/// ("operator basic_string"/"operator std::string", confirmed empirically)
/// has characters no Dart identifier can. `lower_method` (the declaration)
/// and `lower_method_call` (every call site) both read this same constant,
/// so they can never disagree about the name.
const CONVERSION_TO_STR_METHOD_NAME: &str = "toStr";
/// The Dart method name given to a C++ conversion operator whose target is
/// `bool` (`operator bool() const`) — real Verovio trigger: option/handle
/// wrapper classes with an explicit-truthiness idiom (`if (option) {...}`),
/// the "unsupported conversion operator target: Bool" family in the
/// 2026-08-20 diagnosis. Same reasoning and same three call sites as
/// `CONVERSION_TO_STR_METHOD_NAME` (`conversion_operator_dart_method_name`
/// is the single source of truth for both).
const CONVERSION_TO_BOOL_METHOD_NAME: &str = "toBool";

/// The Dart method name a C++ conversion operator's target type earns, if
/// any — `None` for any target this module hasn't verified a name/semantics
/// for yet, which callers must treat as an explicit bailout rather than a
/// guessed name. `lower_method` (the declaration), `lower_call_expr`
/// (deciding whether to route a call through `lower_method_call` at all) and
/// `lower_method_call` (the call site's own callee name) all read this same
/// function, so the three can never disagree about which target types are
/// supported or what they're named.
fn conversion_operator_dart_method_name(target_type: &ir::Type) -> Option<&'static str> {
    match target_type {
        ir::Type::Str => Some(CONVERSION_TO_STR_METHOD_NAME),
        ir::Type::Bool => Some(CONVERSION_TO_BOOL_METHOD_NAME),
        _ => None,
    }
}

/// The `usr`/`name` `lower_type` gives a `void*`/`const void*` pointer's
/// synthesized bridge record — never a real `lower_record`'d class (no
/// `libclang` cursor declares it), so its `usr` is a syntax-bridge-owned
/// namespace rather than a real USR, matching the same pattern
/// `lower_type`'s `Pair`/`Str`/`Bytes` pointee shapes already use just above.
/// `emit::dart::NATIVE_HANDLE_TYPE_NAME` must read the same literal name.
const NATIVE_HANDLE_USR: &str = "syntax-bridge:native-handle";
const NATIVE_HANDLE_TYPE_NAME: &str = "SyntaxBridgeNativeHandle";

pub fn lower_method(
    cursor: clang_sys::CXCursor,
    usr: &str,
    project_root: &Path,
) -> Option<ir::Method> {
    // A conversion operator has no ordinary spelling `emit::dart` could
    // print as a Dart method name — only a target type
    // `conversion_operator_dart_method_name` names is understood; any other
    // target is skipped entirely rather than collected under a guessed
    // name, the same "explicit bailout over silent wrong output" rule
    // every other unrepresentable construct in this module follows. Every
    // other property (params — always empty for a conversion operator,
    // body, `return_type`) is computed identically to an ordinary method
    // below; only the name needs its own path.
    let is_conversion_operator =
        unsafe { clang_sys::clang_getCursorKind(cursor) } == clang_sys::CXCursor_ConversionFunction;
    let conversion_name = is_conversion_operator
        .then(|| {
            conversion_operator_dart_method_name(&lower_type(unsafe {
                clang_sys::clang_getCursorResultType(cursor)
            }))
        })
        .flatten();
    if is_conversion_operator && conversion_name.is_none() {
        return None;
    }

    let name = if let Some(conversion_name) = conversion_name {
        conversion_name.to_owned()
    } else {
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) }
    };
    if name.is_empty() {
        return None;
    }

    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;
    let origin = ir::Origin { file, line, column };

    let mut return_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(cursor) });
    let (mut params, clone_prelude) =
        unsafe { collect_params_with_clone_prelude(cursor, &origin, project_root) };
    // A pure virtual method (`virtual T f() = 0;`) has no body cursor at
    // all — `body: None` models that directly (E06's abstract-method case)
    // rather than treating "no `CompoundStmt` found" as an error the way
    // it would be for any other method.
    let is_pure_virtual = unsafe { clang_sys::clang_CXXMethod_isPureVirtual(cursor) } != 0;
    let body = if is_pure_virtual {
        None
    } else {
        let body_cursor = unsafe { find_compound_stmt_child(cursor) };
        let pointer_out_params = unsafe { pointer_out_param_bindings(cursor) };
        push_active_pointer_out_params(&pointer_out_params);
        let owner = unsafe { clang_sys::clang_getCursorSemanticParent(cursor) };
        let owner_usr =
            unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(owner)) };
        push_active_method_owner_usr(owner_usr);
        let mut body = match body_cursor {
            Some(compound) => unsafe { lower_compound_stmt(compound, project_root) },
            None => Vec::new(),
        };
        pop_active_method_owner_usr();
        pop_active_pointer_out_params(pointer_out_params.len());
        body.splice(0..0, clone_prelude);
        neutralize_out_param_call_input_locals(&mut body);
        unsafe {
            apply_out_param_bridge(cursor, &mut params, &mut return_type, &mut body, &origin)
        };
        Some(body)
    };
    let is_static = unsafe { clang_sys::clang_CXXMethod_isStatic(cursor) } != 0;
    let is_override = unsafe { method_overrides_a_base(cursor) };

    Some(ir::Method {
        name,
        usr: usr.to_owned(),
        params,
        return_type,
        body,
        is_static,
        is_override,
        origin,
    })
}

/// Lowers a (non-copy, non-move) constructor's *definition* cursor into IR —
/// `None` for a copy/move constructor (`lower_call_expr` already treats a
/// call to one of those as transparent sugar around its single argument, per
/// E03; there is never a `Record::constructors` entry to make for one) as
/// well as the usual name/site bail-outs. See `constructor_ordinal` for what
/// `constructor_index` means and why it, not declaration position, is the
/// identity `emit::dart` sorts and names by.
/// A destructor's body (E12's RAII) — `None` for one with no real teardown
/// logic (implicit, empty, or `= default`; `constructor_has_real_body`'s
/// same empty-`CompoundStmt` check applies unchanged here — the "libclang
/// synthesizes an empty body for a trivial member" quirk it documents isn't
/// constructor-specific), which E06 already established is the right,
/// honest translation for "does nothing but participate in a `virtual`
/// hierarchy" (`examples/E06-heranca-simples/NOTES.md`). Never stored on
/// `ir::Method`/`Record::methods` — Dart has no destructor concept to
/// declare it as — only ever consumed by
/// `function_catalog::apply_raii_scope_guards`, which splices these
/// statements into a `Stmt::TryFinally` at each local declaration of this
/// type instead.
pub fn lower_destructor(cursor: clang_sys::CXCursor, project_root: &Path) -> Option<Vec<ir::Stmt>> {
    if !unsafe { constructor_has_real_body(cursor) } {
        return None;
    }
    let body_cursor = unsafe { find_compound_stmt_child(cursor) }?;
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(cursor) };
    let owner_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(owner)) };
    push_active_method_owner_usr(owner_usr);
    let body = unsafe { lower_compound_stmt(body_cursor, project_root) };
    pop_active_method_owner_usr();
    Some(body)
}

pub fn lower_constructor(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
) -> Option<ir::Constructor> {
    if unsafe { is_copy_or_move_constructor(cursor) } {
        return None;
    }

    let usr = unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(cursor)) };
    if usr.is_empty() {
        return None;
    }
    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;
    let origin = ir::Origin { file, line, column };

    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(cursor) };
    let constructor_index = unsafe { constructor_ordinal(owner, cursor) };

    let (params, clone_prelude) =
        unsafe { collect_params_with_clone_prelude(cursor, &origin, project_root) };
    let body_cursor = unsafe { find_compound_stmt_child(cursor) };
    let owner_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(owner)) };
    push_active_method_owner_usr(owner_usr);
    let mut body = match body_cursor {
        Some(compound) => unsafe { lower_compound_stmt(compound, project_root) },
        None => Vec::new(),
    };
    pop_active_method_owner_usr();
    body.splice(0..0, clone_prelude);
    neutralize_out_param_call_input_locals(&mut body);

    Some(ir::Constructor {
        usr,
        constructor_index,
        params,
        body,
        origin,
    })
}

/// Lowers a `struct`/`class` *definition* cursor into IR — called from
/// `function_catalog::visit_cursor` alongside its free-function handling,
/// on the same already-parsed cursor (see this module's docs).
///
/// `None` for an anonymous struct/class (`struct { ... } field;`, item 9 of
/// `docs/plans/diagnostico-verovio-6.2.0.md` — Verovio's own
/// `zip_file.hpp`): the same libclang quirk achado 8 already documented for
/// an anonymous *enum* applies here too — `clang_getCursorSpelling` returns
/// the descriptive debug text `"(unnamed struct at <file>:<line>:<col>)"`,
/// not an empty string, so the old `name.is_empty()` guard alone never
/// caught it, and that text leaked straight into a Dart `class` declaration
/// (a parse error). `clang_Cursor_isAnonymous` is the version-independent
/// way to ask the question this function actually needs answered, mirroring
/// `dart_enum_type_name`'s own fix for the enum case — no usable Dart type
/// name, so there is nothing to declare here at all.
pub fn lower_record(cursor: clang_sys::CXCursor, project_root: &Path) -> Option<ir::Record> {
    if unsafe { clang_sys::clang_Cursor_isAnonymous(cursor) } != 0 {
        return None;
    }
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
    let namespace = unsafe { type_catalog::namespace_of(cursor) };
    let fields = unsafe { record_fields_of(cursor) };
    let static_fields = unsafe { record_static_fields_of(cursor) };
    let mut bases = unsafe { base_classes_of(cursor) };
    // Exactly one base is E06's "extends" (`base_class`); two or more is
    // E09's "herança múltipla" (`mixins`) — never both at once. Zero bases
    // leaves both empty.
    let (base_class, mixins) = if bases.len() == 1 {
        (bases.pop(), Vec::new())
    } else {
        (None, bases)
    };

    Some(ir::Record {
        name,
        usr,
        namespace,
        fields,
        static_fields,
        // Filled in later, by `function_catalog::visit_cursor`, as it visits
        // each constructor's/method's own *definition* cursor — which for an
        // out-of-line member is a separate top-level cursor elsewhere in the
        // translation unit, not a child of this one (see that module's doc
        // comment on why this record is mutated in place rather than built
        // whole here).
        constructors: Vec::new(),
        methods: Vec::new(),
        base_class,
        mixins,
        // Filled in later, the same way constructors/methods are — see the
        // comment just above.
        destructor: None,
        origin,
    })
}

/// Lowers an `enum`/`enum class` *definition* cursor to `ir::Enum` — the
/// same shape as `lower_record`, one level simpler (no fields/methods/base
/// classes, just an ordered list of enumerator names). Caso 4 of
/// `docs/plans/verovio-6.2-pointer-types.md`. `None` for an anonymous enum
/// (no usable Dart type name) or one whose declaration site can't be
/// resolved, mirroring `lower_record`'s own two early-outs.
pub fn lower_enum(cursor: clang_sys::CXCursor, project_root: &Path) -> Option<ir::Enum> {
    let (usr, name) = unsafe { enum_identity(cursor, project_root) }?;
    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;
    let origin = ir::Origin { file, line, column };

    let (variants, values) = unsafe { enum_variants(cursor) };

    Some(ir::Enum {
        name,
        usr,
        variants,
        values,
        origin,
    })
}

/// The one acceptance test every site that can produce a `Type::Enum`
/// shares with `lower_enum`, returning the enum's `usr` and its *Dart*
/// type name. Keeping it in a single function is the whole point: a
/// `Type::Enum` that `lower_enum` would have rejected is a reference to a
/// Dart type nothing ever declares, which `dart analyze` reports as
/// `undefined_class` — the silent-wrong-output failure this module's
/// "silêncio é proibido" rule exists to prevent. `None` when the enum is:
///
/// - anonymous, or without a `usr` — no usable Dart type name (the same
///   two early-outs `lower_record` has);
/// - declared outside `project_root` — `std::memory_order`,
///   `std::launch`, a third-party header's enum. `lower_enum` only ever
///   emits declarations for enums inside the project, so an external one
///   can be *named* here but never *declared* anywhere;
/// - empty (`enum Vazio {};`, legal C++) — Dart requires an enum to
///   declare at least one constant, so there is no valid Dart enum to
///   emit for it.
unsafe fn enum_identity(
    decl: clang_sys::CXCursor,
    project_root: &Path,
) -> Option<(String, String)> {
    let name = unsafe { dart_enum_type_name(decl) };
    if name.is_empty() {
        return None;
    }
    let usr = unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(decl)) };
    if usr.is_empty() {
        return None;
    }
    type_catalog::cursor_site(decl, project_root)?;
    if unsafe { enum_variants(decl) }.0.is_empty() {
        return None;
    }

    Some((usr, name))
}

/// Every enumerator of `decl`, in source order: its Dart name
/// (`dart_enum_constant_name`) and its real C++ value
/// (`clang_getEnumConstantDeclValue`) — see `ir::Enum::values`'s doc
/// comment for why the value travels alongside the name instead of being
/// derived from declaration position later.
unsafe fn enum_variants(decl: clang_sys::CXCursor) -> (Vec<String>, Vec<i64>) {
    unsafe { collect_children(decl) }
        .into_iter()
        .filter(|child| {
            (unsafe { clang_sys::clang_getCursorKind(*child) })
                == clang_sys::CXCursor_EnumConstantDecl
        })
        .map(|constant| {
            let cpp_name = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(constant))
            };
            let value = unsafe { clang_sys::clang_getEnumConstantDeclValue(constant) };
            (dart_enum_constant_name(&cpp_name), value)
        })
        .unzip()
}

/// The Dart type name for an enum declaration — its own spelling, prefixed
/// with the owning record's when the enum is nested inside one.
///
/// `emit::dart` has no nesting to put an enum back into: it groups every
/// declaration by *file* and emits them all at top level, so C++'s two
/// distinct `A::Type` and `B::Type` (two classes in one header each
/// declaring `enum Type`, which Verovio does) would both arrive as a bare
/// top-level `enum Type` in the same `.dart` file — a duplicate
/// definition. Qualifying here, rather than renaming after the fact only
/// when a collision is observed, is what lets the *reference* sites
/// (`lower_type`'s `CXType_Enum` branch and
/// `qualified_static_member_name`) reach the identical name from the same
/// cursor without having to know what else the file contains.
unsafe fn dart_enum_type_name(decl: clang_sys::CXCursor) -> String {
    // `clang_getCursorSpelling` on an anonymous `enum { ... };` is *not*
    // reliably empty — this project's libclang returns the descriptive
    // debug text `"(unnamed enum at <file>:<line>:<col>)"` instead (achado
    // 8, `verovio_6_2_0_transpile_diagnosis`: that text leaked straight
    // into a Dart `enum` declaration, which `dart format` rejects).
    // `clang_Cursor_isAnonymous` is the version-independent way to ask the
    // question this function actually needs answered, so every caller's
    // existing `name.is_empty()` check (both here and in `lower_type`'s
    // `CXType_Enum` branch) keeps working without knowing about the quirk.
    if unsafe { clang_sys::clang_Cursor_isAnonymous(decl) } != 0 {
        return String::new();
    }
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(decl)) };
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(decl) };
    let owner_kind = unsafe { clang_sys::clang_getCursorKind(owner) };
    if owner_kind != clang_sys::CXCursor_ClassDecl && owner_kind != clang_sys::CXCursor_StructDecl {
        return name;
    }
    let owner_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(owner)) };
    if owner_name.is_empty() {
        name
    } else {
        // No separator, matching `function_catalog::pascal_case_namespace`'s
        // own `Ns1Detail` shape — the house style for every other
        // qualified Dart identifier this pipeline builds.
        format!("{owner_name}{name}")
    }
}

/// Dart's true reserved words — illegal in *any* identifier position, not
/// just `dart_enum_constant_name`'s enum-constant one. Dart's *built-in
/// identifiers* (`as`, `mixin`, `static`, ...) are deliberately absent:
/// they're barred from type-name position only, not from an ordinary
/// value-level identifier like a parameter/local/method/function name, so
/// renaming them here would churn names for nothing. `dynamic` is the one
/// deliberate exception: it is normalized even where Dart permits it so the
/// generated package has no `dynamic` escape-hatch spelling at all. None of
/// the remaining words is a C++ keyword either, so an ordinary C++ identifier
/// can land on any of them (Verovio 6.2.0 diagnosis, item 9: `bool is()`,
/// `void f(int in)`, `int is = 1;`, `void finally()` all appear in the real
/// corpus).
const RESERVED_WORDS: &[&str] = &[
    "assert", "break", "case", "catch", "class", "const", "continue", "default", "do", "dynamic",
    "else", "enum", "extends", "false", "final", "finally", "for", "if", "in", "is", "new", "null",
    "rethrow", "return", "super", "switch", "this", "throw", "true", "try", "var", "void", "while",
    "with",
];

/// A C++ value-level identifier (parameter, local variable, method, free
/// function) as a Dart one — every valid C++ identifier is already a valid
/// Dart one, except for the handful landing on one of Dart's own reserved
/// words (`RESERVED_WORDS`), which get a trailing `_` appended. Two
/// distinct callers apply this at the *usr*-based level for methods/free
/// functions (`function_catalog::apply_reserved_word_renames`, since a call
/// site is resolved by usr, not by re-deriving the C++ spelling) and at the
/// lexical level for parameters/locals (this module, both the declaration
/// and every `DeclRefExpr`/`dart_member_name` reference — see that
/// function's own doc comment for why a *pure* function of the string,
/// with no symbol table, is enough to keep the two from ever disagreeing).
pub(crate) fn dart_safe_identifier(name: &str) -> String {
    if RESERVED_WORDS.contains(&name) {
        format!("{name}_")
    } else {
        name.to_owned()
    }
}

/// `dart_safe_identifier`, extended for the one further collision unique to
/// a local variable (never a parameter, field, or free function/method
/// name): C++ keeps a type's name and a local variable's name in separate
/// namespaces (`tm tm;` is legal, common C style — real trigger:
/// `zip_file.dart:1390`), but Dart has one shared namespace, so a local
/// shadows its own type inside its own initializer — most concretely, the
/// synthesized default-value constructor call a record-typed local with no
/// written initializer gets (`lower_one_var_decl`, right below), which
/// would otherwise resolve to the not-yet-initialized local instead of the
/// type, `referenced_before_declaration`. `dart_name` is `name` already run
/// through `dart_safe_identifier`, so the two checks compose in either
/// order. Only a `Record`/`Enum` type can collide this way — Dart's own
/// scalar type names (`int`, `String`, ...) are never valid C++ identifiers
/// in the first place, so no local can ever be spelled to match one.
fn dart_safe_local_name(dart_name: &str, ty: &ir::Type) -> String {
    let shadows_own_type = matches!(
        ty,
        ir::Type::Record { name, .. } | ir::Type::Enum { name, .. } if name == dart_name
    );
    if shadows_own_type {
        format!("{dart_name}_")
    } else {
        dart_name.to_owned()
    }
}

/// A C++ enumerator's name as a Dart enum constant. Every valid C++
/// identifier is already a valid Dart one, so the only rewriting needed is
/// for names Dart won't accept *in this position*: `dart_safe_identifier`'s
/// reserved words, plus the members every Dart enum inherits
/// (`index`/`values` from the enum itself, the rest from `Object`) — a
/// constant can't shadow any of them either.
///
/// Both the declaration (`enum_variants`) and every reference
/// (`qualified_static_member_name`) resolve names through here, so the two
/// can't disagree about whether a given enumerator was rewritten. A C++
/// enum declaring *both* `index` and `index_` collides under this mapping —
/// Dart then rejects the duplicate constant outright, which is the loud
/// failure, not a silently mis-emitted program.
fn dart_enum_constant_name(cpp_name: &str) -> String {
    const ENUM_INHERITED_MEMBERS: &[&str] = &[
        "index",
        "values",
        "hashCode",
        "runtimeType",
        "toString",
        "noSuchMethod",
    ];

    if ENUM_INHERITED_MEMBERS.contains(&cpp_name) {
        format!("{cpp_name}_")
    } else {
        dart_safe_identifier(cpp_name)
    }
}

/// Every one of `cursor`'s base classes (`class Cachorro : public Animal`,
/// E06; `class PatoDaguaVoador : public Voador, public Nadador`, E09), in
/// declaration order, resolved to each base's own USR/name. The caller
/// decides what count means (one is `extends`, two or more are mixins —
/// `lower_record`'s own doc comment on `Record::mixins`); a base whose
/// declaration can't be resolved (empty usr/name) is silently dropped from
/// the list rather than guessed at, same as any other USR/name lookup in
/// this module.
unsafe fn base_classes_of(cursor: clang_sys::CXCursor) -> Vec<ir::BaseClass> {
    unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| {
            (unsafe { clang_sys::clang_getCursorKind(*child) })
                == clang_sys::CXCursor_CXXBaseSpecifier
        })
        .filter_map(|base_specifier| {
            let base_type = unsafe { clang_sys::clang_getCursorType(base_specifier) };
            let decl = unsafe { clang_sys::clang_getTypeDeclaration(base_type) };
            let usr =
                unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(decl)) };
            let name = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(decl))
            };
            if usr.is_empty() || name.is_empty() {
                return None;
            }
            // A base that isn't itself a `Type::Record` — `class
            // HumdrumToken : public std::string, public HumHash` (real,
            // `humlib.h`) is the corpus trigger: `std::string` resolves to
            // `Type::Str` here, a library adapter this bridge represents
            // as a Dart `String`, never as a real declared class. Left
            // unfiltered, `emit::dart` printed it as a base/mixin name
            // anyway — `class HumdrumToken with string, HumHash` —
            // referencing an undeclared Dart class `string`, a real
            // `dart analyze` `undefined_class` error, not just a
            // stylistic wart. `Type::Record`/`Type::Enum` are the only
            // shapes this bridge ever backs with a real Dart class
            // declaration; anything else this same base type would
            // otherwise map to (`Str`/`List`/`Bytes`/`Unsupported`/...)
            // is excluded the same way. This does *not* pretend the
            // inheritance never happened — `HumdrumToken`'s own use as a
            // `Str` (assignment, comparison, `.find`/`.substr`, ...) stays
            // its own honest bailout (`unsupported implicit conversion
            // from Record{HumdrumToken} to Str`), since fully modeling a
            // string-backed base needs the base's own constructor
            // arguments — a C++ member-initializer-list entry
            // (`: std::string(s)`), which `clang_visitChildren` never
            // exposes as a cursor at all (`CXCursor_CXXCtorInitializer`
            // doesn't exist in `libclang`'s public C API — confirmed
            // directly against `clang-sys`'s own generated bindings, not
            // assumed) — a real, separate blocker this filter doesn't
            // attempt to work around.
            match lower_type(base_type) {
                ir::Type::Record { .. } | ir::Type::Enum { .. } => {
                    Some(ir::BaseClass { usr, name })
                }
                _ => None,
            }
        })
        .collect()
}

/// Whether the record declared at `record_decl` transitively derives from
/// `ancestor_usr` — used to tell a pointer upcast (safe, transparent) from
/// a downcast (needs a checked Dart `as T?`) when lowering an explicit
/// `static_cast`/C-style cast between two different `Type::Record`s (F7,
/// `docs/prompts/2026-08-21-05-downcast-de-hierarquia-preservado.md`).
/// Walks `CXXBaseSpecifier` cursors directly, the same way
/// `base_classes_of` does for a record's *declared* bases, rather than
/// consulting a `Module`/type catalog: none exists yet at this point in
/// lowering (`lower_expr` runs during a single libclang cursor visitation,
/// well before `function_catalog::extract_function_catalog` assembles the
/// `Module` its callers use). `clang_getTypeDeclaration` alone can return a
/// forward-declaration cursor with no base-specifier children, so this
/// follows it to `clang_getCursorDefinition` wherever one is available —
/// at every recursion level, since a base's own declaration cursor can be
/// a forward declaration just as easily as the starting one.
unsafe fn record_derives_from(record_decl: clang_sys::CXCursor, ancestor_usr: &str) -> bool {
    let definition = unsafe { clang_sys::clang_getCursorDefinition(record_decl) };
    let record_cursor = if unsafe { clang_sys::clang_Cursor_isNull(definition) } != 0 {
        record_decl
    } else {
        definition
    };

    unsafe { collect_children(record_cursor) }
        .into_iter()
        .filter(|child| {
            (unsafe { clang_sys::clang_getCursorKind(*child) })
                == clang_sys::CXCursor_CXXBaseSpecifier
        })
        .any(|base_specifier| {
            let base_type = unsafe { clang_sys::clang_getCursorType(base_specifier) };
            let base_decl = unsafe { clang_sys::clang_getTypeDeclaration(base_type) };
            let base_usr = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(base_decl))
            };
            base_usr == ancestor_usr || unsafe { record_derives_from(base_decl, ancestor_usr) }
        })
}

/// Whether `cursor` (a `CXXMethod` definition) overrides a base class's
/// virtual method — `clang_getOverriddenCursors`, not name-matching against
/// the base's own member list, so an unrelated method that happens to share
/// a name with a base member is never mistaken for an override.
unsafe fn method_overrides_a_base(cursor: clang_sys::CXCursor) -> bool {
    let mut overridden: *mut clang_sys::CXCursor = std::ptr::null_mut();
    let mut count: c_uint = 0;
    unsafe {
        clang_sys::clang_getOverriddenCursors(cursor, &mut overridden, &mut count);
    }
    let has_override = count > 0;
    if !overridden.is_null() {
        unsafe {
            clang_sys::clang_disposeOverriddenCursors(overridden);
        }
    }
    has_override
}

/// `struct`/`class` fields, in declaration order — filters `cursor`'s
/// children down to `CXCursor_FieldDecl` (skipping methods, access
/// specifiers, etc. that a non-POD-but-still-in-scope record might have).
unsafe fn record_fields_of(cursor: clang_sys::CXCursor) -> Vec<ir::Field> {
    unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| unsafe { clang_sys::clang_getCursorKind(*child) } == clang_sys::CXCursor_FieldDecl)
        .map(|field_cursor| {
            let name = unsafe { dart_member_name(field_cursor) };
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(field_cursor) });
            ir::Field { name, ty }
        })
        .collect()
}

/// `static` data members, in declaration order — a static member is a
/// `CXCursor_VarDecl` child of the record (confirmed with
/// `clang -Xclang -ast-dump`: distinct from an instance field's
/// `CXCursor_FieldDecl`), so it's invisible to `record_fields_of`'s filter
/// and needs its own pass.
unsafe fn record_static_fields_of(cursor: clang_sys::CXCursor) -> Vec<ir::Field> {
    unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| unsafe { clang_sys::clang_getCursorKind(*child) } == clang_sys::CXCursor_VarDecl)
        .map(|field_cursor| {
            let name = unsafe { dart_member_name(field_cursor) };
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(field_cursor) });
            ir::Field { name, ty }
        })
        .collect()
}

/// The Dart-side name for a field/static-field cursor (or the cursor a
/// `MemberRefExpr` resolves to, via `clang_getCursorReferenced` — the same
/// call site the field's own declaration and every reference to it both
/// route through, so they can never disagree): a `private` C++ member gets
/// a leading `_`, trimming one trailing `_` off the C++ name first so a
/// conventionally-named `saldo_` becomes `_saldo`, not `_saldo_`.
///
/// `protected` does *not* get the same treatment (tarefa 03,
/// `docs/prompts/2026-08-21-03-*.md`, família F2 — decisão de produto
/// confirmada pelo usuário: opção A). `protected` in C++ means "visible to
/// subclasses", but Dart's `_` means library-private, and `emit::dart`
/// gives every record its own file/library — so prefixing `_` on a
/// `protected` member hides it from exactly the subclasses C++ granted
/// access to (`Undefined name` in every subclass file, `unused_field` in
/// the declaring one). Collapsing `protected` into `private` was
/// `dart_member_name`'s original approach, matching Dart's own two-level
/// model, but that model has no library-scoped-but-subclass-visible tier to
/// map `protected` onto, so it's treated as public instead: a `public` (or
/// unspecified-in-a-`struct`, which defaults to public) member is
/// untouched *except* for a leading `_` already in its C++ spelling
/// (`pugixml.hpp`'s `protected xml_node_struct* _root;` — some C++
/// codebases use a leading underscore as their own "internal" convention,
/// independent of the access specifier). Dart's privacy rule reads the
/// literal identifier text, not the access specifier, so passing that
/// spelling through unchanged reproduces the exact bug this function exists
/// to avoid: `_root` still can't be read from any file but the declaring
/// one. Stripped here rather than left to `dart_safe_identifier` since it's
/// a privacy concern (this function's whole reason to exist), not a
/// reserved-word one; guarded against an all-underscore name (`_`, `__`)
/// stripping to empty, which no real C++ member spelling is.
unsafe fn dart_member_name(cursor: clang_sys::CXCursor) -> String {
    let cpp_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    let access = unsafe { clang_sys::clang_getCXXAccessSpecifier(cursor) };
    if access == clang_sys::CX_CXXPrivate {
        format!("_{}", cpp_name.trim_end_matches('_'))
    } else {
        let public_name = match cpp_name.trim_start_matches('_') {
            "" => cpp_name.as_str(),
            stripped => stripped,
        };
        dart_safe_identifier(public_name)
    }
}

/// `dart_member_name`, qualified with `ClassName.`/`EnumName.` when
/// `referenced` is a `static` data member (E12 — reading a class's static
/// counter from a free function, `Guarda::contadorAberto`/bare
/// `contadorAberto` from inside a method both resolve here) or an
/// enumerator (caso 4 of `docs/plans/verovio-6.2-pointer-types.md` —
/// `data_STAFFREL::STAFFREL_before`/bare `STAFFREL_before` from an
/// unscoped enum both resolve here too, same reasoning: C++ allows the
/// qualified form for both scoped and unscoped enums even where the bare
/// form also compiles, so always qualifying is correct regardless of which
/// spelling the source used). Bare access to a static member/enumerator
/// only compiles in Dart *inside* the declaring type's own body (Dart enum
/// values always need `EnumName.` qualification, full stop) — always
/// qualifying is correct in every context, so this doesn't need to know
/// where the reference sits to decide — `lower_expr`'s own `DeclRefExpr`
/// case has no such context to give it anyway. A static data member is a
/// `CXCursor_VarDecl` child of the class (confirmed already, by the same
/// distinction `record_static_fields_of` uses against `CXCursor_FieldDecl`);
/// anything else (a non-static field is always reached through a
/// `MemberRefExpr`, not `DeclRefExpr`, so never reaches here — a free
/// function, a local, a parameter) is returned unqualified.
unsafe fn qualified_static_member_name(referenced: clang_sys::CXCursor) -> String {
    let referenced_kind = unsafe { clang_sys::clang_getCursorKind(referenced) };
    // An enumerator resolves its name through `dart_enum_constant_name`,
    // never `dart_member_name`. Clang propagates the *enum's* access
    // specifier onto every one of its `EnumConstantDecl`s, so a nested
    // `enum` that is private by default (`class C { enum Cor { Vermelho }; }`
    // — ordinary C++) would otherwise have `dart_member_name` prefix an
    // underscore here and nowhere else: the declaration, built by
    // `enum_variants`, says `Vermelho` while this reference says
    // `Cor._Vermelho`. Dart privacy for an enum lives on the enum *type*
    // anyway, not on its individual constants, so there is nothing for the
    // prefix to express in this position.
    let name = if referenced_kind == clang_sys::CXCursor_EnumConstantDecl {
        let cpp_name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced))
        };
        dart_enum_constant_name(&cpp_name)
    } else {
        unsafe { dart_member_name(referenced) }
    };

    if referenced_kind != clang_sys::CXCursor_VarDecl
        && referenced_kind != clang_sys::CXCursor_EnumConstantDecl
    {
        return name;
    }
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let owner_kind = unsafe { clang_sys::clang_getCursorKind(owner) };
    if owner_kind != clang_sys::CXCursor_ClassDecl
        && owner_kind != clang_sys::CXCursor_StructDecl
        && owner_kind != clang_sys::CXCursor_EnumDecl
    {
        // A genuine block-scope local (never a static member — those all
        // have a Class/Struct/Enum owner, just excluded above) — the other
        // half of `lower_one_var_decl`'s own `dart_safe_local_name` rename,
        // applied here so every later *read* of the same local agrees with
        // its declaration (F15/tarefa 15.8).
        if referenced_kind == clang_sys::CXCursor_VarDecl {
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(referenced) });
            return dart_safe_local_name(&name, &ty);
        }
        return name;
    }
    // For an enum owner this has to be the *same* `dart_enum_type_name` the
    // declaration was emitted under, or a nested enum's qualifier here
    // (`Type.`) won't match the name it was declared as (`AType`).
    let owner_name = if owner_kind == clang_sys::CXCursor_EnumDecl {
        unsafe { dart_enum_type_name(owner) }
    } else {
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(owner)) }
    };
    if owner_name.is_empty() {
        name
    } else {
        format!("{owner_name}.{name}")
    }
}

/// The receiver of a `MemberRefExpr` cursor (a field access or, seen from
/// `lower_method_call`, a method reference used as a call's callee) —
/// `this` for an implicit access (`saldo_`, not `this->saldo_`), or the
/// explicit object for one written out (`conta.saldo_`, `p.x`).
///
/// Confirmed empirically, not assumed: `clang_visitChildren` reports **zero**
/// children for a `MemberRefExpr` whose receiver is an implicit `this` —
/// `clang -Xclang -ast-dump` shows a `CXXThisExpr "implicit this"` node
/// there, but that's Clang's own AST dump operating on the full internal
/// tree, not `libclang`'s cursor-visitation API, which doesn't surface
/// implicit nodes as visitable child cursors. An *explicit* `this->x` (not
/// exercised by any current fixture, since it's semantically identical to
/// the implicit case) would show up as the one-child shape below, resolved
/// the normal way through `lower_expr`'s own `CXXThisExpr` handling.
unsafe fn member_ref_receiver(
    member_ref_cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> ir::Expr {
    // A qualified access (`Base::foo()`, `ns::Base::foo()`) attaches its
    // `NestedNameSpecifier` as a `TypeRef`/`NamespaceRef` sibling of the
    // receiver — disambiguation already resolved by `clang_getCursorReferenced`
    // on the call itself, not a value. Left unfiltered, a qualified call on
    // an implicit `this` (whose `CXXThisExpr` `libclang` never visits) has
    // that `TypeRef` as its *only* child, and it would be mistaken for the
    // receiver itself. See this function's caller-side test,
    // `a_qualified_base_member_call_ignores_the_disambiguating_namespace_and_type_refs`.
    let children: Vec<clang_sys::CXCursor> = unsafe { collect_children(member_ref_cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
            )
        })
        .collect();
    match children.as_slice() {
        [] => ir::Expr::This {
            ty: ir::Type::Void,
            origin: origin.clone(),
        },
        [target_cursor] => unsafe { lower_expr(*target_cursor, project_root) },
        _ => ir::Expr::Unsupported {
            reason: format!(
                "member reference had {} children, expected 0 (implicit `this`) or 1",
                children.len()
            ),
            origin: origin.clone(),
        },
    }
}

/// The base named by a qualified member reference (`Base::foo()`,
/// `this->Base::foo()`), if any — the disambiguating `TypeRef` sibling
/// `member_ref_receiver` (just above) already filters out of the receiver
/// walk. `None` for an ordinary unqualified/virtual access (`foo()`,
/// `obj.foo()`), whose `MemberRefExpr` has no such sibling at all — this is
/// exactly how `lower_method_call` (F12/tarefa 09) tells a qualified base
/// call apart from ordinary virtual dispatch, the distinction
/// `EditorialElement::Reset();` vs `ResetSource();` loses today
/// (`docs/prompts/2026-08-21-09-chamada-a-base-qualificada.md`).
unsafe fn member_ref_qualifier_base(
    member_ref_cursor: clang_sys::CXCursor,
) -> Option<ir::BaseClass> {
    let type_ref = unsafe { collect_children(member_ref_cursor) }
        .into_iter()
        .find(|child| {
            let kind = unsafe { clang_sys::clang_getCursorKind(*child) };
            kind == clang_sys::CXCursor_TypeRef
        })?;
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(type_ref) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    let usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced)) };
    if usr.is_empty() {
        return None;
    }
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    Some(ir::BaseClass { usr, name })
}

/// Whether `cursor`'s own spelling location resolves to no real file at
/// all — true for a compiler builtin (`__va_list_tag`, confirmed
/// empirically: `clang_Location_isInSystemHeader` on it is *false*, since
/// there's no header it's "in"), never for an ordinary declaration from
/// project source or a real system header, both of which always have a
/// genuine file. Deliberately independent of `project_root` (unlike
/// `type_catalog::cursor_site`) — `lower_type` has no path context to give
/// it, and doesn't need one: this only ever needs to tell "a real
/// declaration somewhere" apart from "no declaration at all", not which
/// project a real one belongs to.
unsafe fn cursor_has_no_real_file_location(cursor: clang_sys::CXCursor) -> bool {
    let location = unsafe { clang_sys::clang_getCursorLocation(cursor) };
    let mut file = std::ptr::null_mut();
    unsafe {
        clang_sys::clang_getSpellingLocation(
            location,
            &mut file,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }
    file.is_null()
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

    // `std::string` is itself a `typedef` for `basic_string<char, ...>` —
    // unwrapping through it is what lets the `CXType_Record` branch below
    // (and its `stdlib_template_name` check) ever see the real
    // specialization at all. Confirmed via `clang -Xclang -ast-dump`: a
    // `const std::string&` parameter's outer kind is `LValueReference`,
    // whose pointee is `Elaborated` (namespace-qualified, same as any
    // `std::`-prefixed name), which unwraps to this `Typedef`, spelled
    // `"std::string"` — exactly the text this used to report as
    // `Unsupported` before this branch existed.
    if cx_type.kind == clang_sys::CXType_Typedef {
        let decl = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
        return lower_type(unsafe { clang_sys::clang_getTypedefDeclUnderlyingType(decl) });
    }

    // `const std::string&`/`const std::vector<int>&` — E05's fixture takes
    // every library-adapted parameter by const reference specifically to
    // dodge the by-value-copy armadilha E03 already solved for `Record`
    // (`examples/E03-struct-pod/NOTES.md`) rather than reopening it for
    // `Str`/`List` too; that's a real fix, not a gap, but it does mean
    // `lower_type` has to see through the reference to reach the pointee at
    // all, or every by-reference parameter/argument in E05 would report as
    // `Unsupported` before ever reaching the `CXType_Record` branch below.
    // Dart has no reference types, so unwrapping and discarding the
    // reference itself (not just `const`) is correct — every parameter and
    // temporary in Dart already behaves like a reference to its object.
    if cx_type.kind == clang_sys::CXType_LValueReference {
        return lower_type(unsafe { clang_sys::clang_getPointeeType(cx_type) });
    }

    // A bare (undecayed) function type — real trigger: a free function's
    // *name* used directly as a value (F10/tarefa 13's `std::sort(b, e,
    // descending)`/`std::for_each(b, e, print_one)`, where the comparator/
    // callback argument is a plain `DeclRefExpr` to a top-level function,
    // never wrapped in an explicit `&`). C++'s own AST keeps that
    // `DeclRefExpr`'s static type as the function type itself
    // (`CXType_FunctionProto`), not the pointer it decays to at the actual
    // call — confirmed empirically: only an *explicit* function-pointer
    // *variable*/parameter type reaches the `CXType_Pointer` branch below.
    // Dart draws no such distinction — referencing a top-level function by
    // name already produces a first-class function value with the exact
    // call shape `lower_callback_type` already extracts from a function
    // pointer's pointee type, so this reuses it directly rather than
    // requiring the pointer wrapper this AST shape never has.
    if cx_type.kind == clang_sys::CXType_FunctionProto {
        return unsafe { lower_callback_type(cx_type) };
    }

    // `T*` — E10 recognized honestly refusing every raw pointer as
    // `dart:ffi` territory ("talvez a resposta certa seja recusar"), and
    // that stayed the whole story until the Verovio 6.2.0 diagnosis
    // (`docs/plans/diagnostico-verovio-6.2.0.md`, achado 5) showed how much
    // of a real object-graph codebase's surface area that costs: most raw
    // pointers in idiomatic C++ OOP are a single, possibly-null reference
    // to one object, not a buffer. `mapping::pointer_options_for` (case
    // A10, `docs/mapping-solver-cases.md`) is consulted for real here, not
    // just to describe a decision: when the pointee itself already lowers
    // to a type this IR fully represents (a project `Record`, or the E05
    // library adapters `Str`/`List`), C++'s own static type system already
    // guarantees the pointer is either null or an object of that type (or
    // a subtype) — a nullable reference, `Type::Nullable`. Anything else
    // (`void`, a scalar, or a pointee this module itself can't represent)
    // keeps case C01's own answer, unchanged: `Type::Unsupported`, honest
    // about still needing `dart:ffi`.
    if cx_type.kind == clang_sys::CXType_Pointer {
        let pointee_cx_type = unsafe { clang_sys::clang_getPointeeType(cx_type) };
        if pointee_cx_type.kind == clang_sys::CXType_FunctionProto {
            return unsafe { lower_callback_type(pointee_cx_type) };
        }
        let pointee_spelling = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(pointee_cx_type))
        };
        let mut pointee_ty = lower_type(pointee_cx_type);
        // `char`/`const char` — the raw-pointer sibling of E05's
        // `std::string` adapter. `mapping::scalar_pointee_dart_type`
        // already decided these two spellings map to Dart `String`
        // (`docs/plans/verovio-6.2-pointer-types.md` caso 3), but that
        // decision only reached the pointer *catalog*'s display
        // (`project_service::list_pointers`), never `lower_type` itself —
        // a raw C string fell through the generic scalar catch-all to
        // `Unsupported` like `int`/`bool`/`void`. A C string has the same
        // finite-and-nullable guarantee as a pointer to any other
        // IR-known pointee (it's either null or a real byte string, never
        // legitimately something else), so folding it into `Type::Str` —
        // the same representation `std::string` already uses — lets it
        // fall through the ordinary `Known` branch just below instead of
        // needing a parallel case.
        if is_known_byte_buffer_type(&pointee_spelling) {
            pointee_ty = ir::Type::Bytes;
        } else if mapping::scalar_pointee_dart_type(&pointee_spelling).is_some()
            || unsafe { is_text_character_type(pointee_cx_type) }
        {
            pointee_ty = ir::Type::Str;
        }
        // `lower_type` has no project-wide catalog in hand (only the
        // pointee it just lowered), so `facts: None` — `pointer_options_for`
        // still answers correctly with the unenriched singleton set `[T]`;
        // see that function's own doc comment on why that's honest, not a
        // shortcut. `Str`/`List` have no project `usr` of their own (E05's
        // library adapters, never `lower_record`'d) — named by their C++
        // origin instead, which is exactly as much identity as
        // `possible_pointee_types` needs to report "just this one, no
        // subclasses to find" for them.
        let shape = match &pointee_ty {
            ir::Type::Record { usr, name } | ir::Type::Enum { usr, name } => {
                mapping::PointeeShape::Known {
                    usr: usr.clone(),
                    name: name.clone(),
                }
            }
            ir::Type::Str => mapping::PointeeShape::Known {
                usr: "std::string".to_owned(),
                name: "String".to_owned(),
            },
            ir::Type::Bytes => mapping::PointeeShape::Known {
                usr: "syntax-bridge:bytes".to_owned(),
                name: "Uint8List".to_owned(),
            },
            ir::Type::List(_) => mapping::PointeeShape::Known {
                usr: "std::vector".to_owned(),
                name: "List".to_owned(),
            },
            ir::Type::Set(_) => mapping::PointeeShape::Known {
                usr: "std::set".to_owned(),
                name: "Set".to_owned(),
            },
            ir::Type::Map(_, _) => mapping::PointeeShape::Known {
                usr: "std::map".to_owned(),
                name: "Map".to_owned(),
            },
            ir::Type::Pair(_, _) => mapping::PointeeShape::Known {
                usr: "std::pair".to_owned(),
                name: "SyntaxBridgePair".to_owned(),
            },
            _ => mapping::PointeeShape::Opaque,
        };
        let options = mapping::pointer_options_for(shape, None, None);
        return if options[0].id == "referencia-anulavel" {
            ir::Type::Nullable(Box::new(pointee_ty))
        } else if pointee_ty == ir::Type::Void {
            // `void*`/`const void*` — the single largest type bailout in the
            // 2026-08-20 Verovio diagnosis (896 + 253 occurrences, real
            // shapes confirmed by grepping the extracted Verovio source:
            // `include/vrv/floatingobject.h`'s `SetDrawingGrpObject(void
            // *drawingGrpObject)`, `include/pugi/pugixml.hpp`'s `void*
            // _impl;`). `pointer_options_for` already answers
            // `"ponte-dart-ffi"` for this shape — this finishes that
            // option's Dart realization for the `void` pointee
            // specifically: a named, documented bridge
            // (`NATIVE_HANDLE_TYPE_NAME`, identity-only, never dereferenced
            // or arithmetic'd), rather than the generic `Unsupported`
            // bailout. A pointer to an unrepresentable scalar/record
            // pointee (not `void`) still falls through to `Unsupported`
            // below — that shape can still be a buffer or ABI callback,
            // which this identity-only bridge would misrepresent.
            ir::Type::Nullable(Box::new(ir::Type::Record {
                usr: NATIVE_HANDLE_USR.to_owned(),
                name: NATIVE_HANDLE_TYPE_NAME.to_owned(),
            }))
        } else {
            let spelling = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
            };
            ir::Type::Unsupported(spelling)
        };
    }

    match cx_type.kind {
        // Dart has one arbitrary-precision integral scalar. C/C++'s signed,
        // unsigned, character, fixed-width and wide-character scalar kinds
        // all preserve their value-domain shape as `int`; signedness and
        // fixed-width constraints belong to a boundary validator when a
        // particular API needs them, not to `dynamic` placeholders in the
        // generated program. `size_t` reaches this arm as `unsigned long` on
        // the Flatpak toolchain.
        clang_sys::CXType_Char_U
        | clang_sys::CXType_UChar
        | clang_sys::CXType_Char16
        | clang_sys::CXType_Char32
        | clang_sys::CXType_UShort
        | clang_sys::CXType_UInt
        | clang_sys::CXType_ULong
        | clang_sys::CXType_ULongLong
        | clang_sys::CXType_UInt128
        | clang_sys::CXType_Char_S
        | clang_sys::CXType_SChar
        | clang_sys::CXType_WChar
        | clang_sys::CXType_Short
        | clang_sys::CXType_Int
        | clang_sys::CXType_Long
        | clang_sys::CXType_LongLong
        | clang_sys::CXType_Int128 => ir::Type::Int,
        clang_sys::CXType_Bool => ir::Type::Bool,
        // Dart's `double` is IEEE-754 binary64. It is the closest Dart
        // scalar for every C/C++ floating kind; precision narrower/wider than
        // binary64 is tracked by the source type catalog rather than erased
        // into `dynamic` in the emitted API.
        clang_sys::CXType_Half
        | clang_sys::CXType_Float
        | clang_sys::CXType_Double
        | clang_sys::CXType_LongDouble
        | clang_sys::CXType_Float128
        | clang_sys::CXType_Float16 => ir::Type::Double,
        clang_sys::CXType_Void => ir::Type::Void,
        // A native `T[N]` has the same value-container shape as Dart's
        // `List<T>`. The bound is deliberately not erased into a fake type:
        // it remains available from the source catalog for a future boundary
        // validator, while the generated program keeps the element type and
        // does not need a `SyntaxBridgeOpaque` field just because a fixed
        // array appeared in a C++ record.
        clang_sys::CXType_ConstantArray => ir::Type::List(Box::new(lower_type(unsafe {
            clang_sys::clang_getArrayElementType(cx_type)
        }))),
        // Caso 4 of `docs/plans/verovio-6.2-pointer-types.md`: an
        // `enum`/`enum class` use, mirroring the `CXType_Record` branch
        // below but simpler — `clang_getTypeDeclaration` on an enum type
        // always resolves directly to its own `EnumDecl` (no
        // stdlib-template-name/union special-casing needed, and no
        // `Unexposed` sharing, unlike `Record`).
        clang_sys::CXType_Enum => {
            let decl = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
            let usr =
                unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(decl)) };
            let name = unsafe { dart_enum_type_name(decl) };
            // Whether this enum is one the package actually *declares* is
            // not decided here — `lower_type` has no `project_root` to
            // test against, and an enum reachable as a type is routinely
            // lowered long before its own declaration is visited.
            // `function_catalog::reject_undeclared_enum_refs` settles it
            // afterwards, against the finished `ir_enums` list, and
            // rewrites whatever `lower_enum` never declared (external,
            // anonymous, or empty) back to `Unsupported`.
            if usr.is_empty() || name.is_empty() {
                let spelling = unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
                };
                ir::Type::Unsupported(spelling)
            } else {
                ir::Type::Enum { usr, name }
            }
        }
        // `libclang` reports a template specialization like
        // `basic_string<char>`/`vector<int>` as `CXType_Unexposed`
        // (confirmed empirically, not assumed — every stdlib type in E05's
        // fixture came back this way, never as `CXType_Record`), a known
        // limitation for types it can't fully model. `clang_getTypeDeclaration`
        // still resolves to the real class-template-specialization decl
        // regardless, so the two kinds share this branch rather than
        // `Unexposed` falling to the `Unsupported` catch-all below.
        clang_sys::CXType_Record | clang_sys::CXType_Unexposed => {
            let decl = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
            // A `union` (E10) shares `CXType_Record` with `struct`/`class` —
            // Clang doesn't give it its own type kind — but `lower_record`
            // is only ever dispatched for `CXCursor_StructDecl`/
            // `CXCursor_ClassDecl` (`function_catalog::visit_cursor`), never
            // `CXCursor_UnionDecl`, so a union's `ir::Record` never actually
            // gets built. Falling through to the ordinary
            // usr/name-resolution path below would still find a valid
            // usr/name (the union decl itself resolves fine) and return
            // `Type::Record { usr, name }` pointing at a class that doesn't
            // exist anywhere in the emitted Dart — confirmed the hard way,
            // as `dart analyze`'s `undefined_class` on a fixture that uses
            // one as a parameter type, not caught by any earlier degrau
            // because none of them ever had a union to expose it.
            // Overlapping memory for two differently-typed fields has no
            // Dart equivalent worth guessing at (`dart:ffi` territory, and
            // even then only for a `Struct`, which reads each field at a
            // byte offset — a real bridge, not attempted until a fixture
            // forces it) — refusing explicitly, before the dangling
            // reference can happen, is the honest answer this degrau's own
            // armadilha names directly.
            if unsafe { clang_sys::clang_getCursorKind(decl) } == clang_sys::CXCursor_UnionDecl {
                let spelling = unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
                };
                return ir::Type::Unsupported(format!("union {spelling}"));
            }
            // `__gnu_cxx::__normal_iterator<Ptr, Container>` — libstdc++'s
            // real implementation of `std::vector<T>::iterator`/
            // `std::string::iterator` (a `typedef` down to this, confirmed
            // via `clang++ -Xclang -ast-dump`). Checked before
            // `stdlib_template_name` (whose own ancestor walk only accepts
            // `std`, not `__gnu_cxx` — see `is_normal_iterator_decl`'s doc
            // comment for why that's deliberate, not an oversight) via its
            // own narrow, dedicated check. A *long-lived* one (a field,
            // F10/tarefa 13's own `convertfunctor.dart:354` trigger — `late
            // __normal_iterator _m_currentMeasure;`) has no single
            // recognized idiom to erase it the way
            // `lower_find_iterator_guard_idiom`/`lower_iterator_for_loop` do
            // for one scoped to a single statement/loop, so it needs its own
            // standing representation — `Type::ListCursor`,
            // `SyntaxBridgeListCursor<T>`'s Dart shape — rather than falling
            // to `Type::Unsupported` and printing the internal libstdc++
            // name as a bare, undeclared Dart type (`dart analyze`'s
            // `undefined_class`/`non_type_as_type_argument`, this same
            // family's whole premise). The iterator's own second template
            // argument is the container it iterates (confirmed via
            // `-ast-dump`); `lower_type` on that container already gives the
            // exact element type this project already computes for it
            // (`Type::List`/`Type::Set`) — scoped to those two shapes only:
            // a `basic_string::iterator`'s "element" is a code unit, not a
            // value this cursor's `List<T>`-backed shape fits, so that
            // (rarer) case stays the honest bailout below rather than a
            // wrong translation.
            if unsafe { is_normal_iterator_decl(decl) } {
                let container_ty =
                    if unsafe { clang_sys::clang_Type_getNumTemplateArguments(cx_type) } >= 2 {
                        Some(lower_type(unsafe {
                            clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 1)
                        }))
                    } else {
                        None
                    };
                if let Some(ir::Type::List(element) | ir::Type::Set(element)) = container_ty {
                    return ir::Type::ListCursor(element);
                }
                let spelling = unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
                };
                return ir::Type::Unsupported(format!(
                    "std::__normal_iterator (spelling: {spelling})"
                ));
            }
            let stdlib_name = unsafe { stdlib_template_name(decl) };
            match stdlib_name.as_deref() {
                Some("basic_string") => return ir::Type::Str,
                // `std::stringstream`/`std::ostringstream` — the read+write
                // and write-only accumulator streams (round 19, real
                // trigger `options.cpp`'s `OptionArray::GetStr`: `ss <<
                // "\"" << value << "\""; ... return ss.str();`). Modeled
                // directly as `Type::Str` rather than a distinct type:
                // every operation this bridge supports on one (`<<`
                // insertion as a statement, `.str()`) reduces to plain
                // string concatenation/identity, so giving the variable
                // itself `Type::Str` reuses the entire existing string
                // machinery (default value, assignment, emission) instead
                // of inventing a parallel one. `basic_istringstream`
                // (read/extraction via `>>`, a different idiom entirely)
                // deliberately excluded — no fixture needs it yet.
                Some("basic_stringstream") | Some("basic_ostringstream") => return ir::Type::Str,
                // `std::list<T>`, `std::deque<T>`, `std::array<T, N>` and
                // `std::initializer_list<T>` preserve the same value shape
                // Dart exposes as `List<T>`. Their iteration and allocation
                // characteristics differ, and fixed array bounds need a
                // boundary validator when they are observable, but none of
                // that warrants an opaque type in an otherwise typed API.
                // Methods with semantics that Dart's List does not share
                // still take their own explicit expression bailout until
                // they gain a lowering rule.
                // `std::multiset<T>` allows duplicate elements — `Set<T>`
                // (used for `set`/`unordered_set` below, which reject
                // duplicates) would silently drop them, a real semantic
                // loss `List<T>` doesn't have. `List<T>` doesn't preserve
                // `multiset`'s automatic sort-by-value either, the same
                // documented, deliberate approximation this module already
                // accepts for `unordered_set`'s/`unordered_map`'s iteration
                // order (see their own comment just below) — order is a
                // separate behavioral concern from the type boundary
                // itself, not a reason to erase it.
                // `std::stack<T>` (default `std::deque<T>`-backed adapter,
                // LIFO-only) shares the same first-template-argument
                // element type and `List<T>` Dart shape as the sequence
                // containers below; the LIFO-only access pattern is a
                // method-dispatch concern (`lower_stdlib_method_call`'s
                // `"stack"` arms: `top`→`.last`, `push`→`.add`,
                // `pop`→`.removeLast`), not a type-mapping one.
                Some("vector")
                | Some("list")
                | Some("deque")
                | Some("array")
                | Some("initializer_list")
                | Some("multiset")
                | Some("stack") => {
                    let element =
                        if unsafe { clang_sys::clang_Type_getNumTemplateArguments(cx_type) } >= 1 {
                            lower_type(unsafe {
                                clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 0)
                            })
                        } else {
                            ir::Type::Unsupported(
                                "std::sequence with no element type argument".to_owned(),
                            )
                        };
                    return ir::Type::List(Box::new(element));
                }
                // `unordered_set` preserves Set's membership semantics; the
                // iteration-order difference is a separate behavioral
                // concern, not a reason to erase the type boundary.
                Some("set") | Some("unordered_set") => {
                    let element = if unsafe {
                        clang_sys::clang_Type_getNumTemplateArguments(cx_type)
                    } >= 1
                    {
                        lower_type(unsafe {
                            clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 0)
                        })
                    } else {
                        ir::Type::Unsupported("std::set with no element type argument".to_owned())
                    };
                    return ir::Type::Set(Box::new(element));
                }
                // Just like `unordered_set`, `unordered_map` has Dart's
                // `Map<K, V>` value shape even though its ordering and
                // performance characteristics differ from `std::map`.
                Some("map") | Some("unordered_map") => {
                    let arg_count =
                        unsafe { clang_sys::clang_Type_getNumTemplateArguments(cx_type) };
                    let key = if arg_count >= 1 {
                        lower_type(unsafe {
                            clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 0)
                        })
                    } else {
                        ir::Type::Unsupported("std::map with no key type argument".to_owned())
                    };
                    let value = if arg_count >= 2 {
                        lower_type(unsafe {
                            clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 1)
                        })
                    } else {
                        ir::Type::Unsupported("std::map with no value type argument".to_owned())
                    };
                    return ir::Type::Map(Box::new(key), Box::new(value));
                }
                Some("pair") => {
                    let arg_count =
                        unsafe { clang_sys::clang_Type_getNumTemplateArguments(cx_type) };
                    let first = if arg_count >= 1 {
                        lower_type(unsafe {
                            clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 0)
                        })
                    } else {
                        ir::Type::Unsupported("std::pair with no first type argument".to_owned())
                    };
                    let second = if arg_count >= 2 {
                        lower_type(unsafe {
                            clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 1)
                        })
                    } else {
                        ir::Type::Unsupported("std::pair with no second type argument".to_owned())
                    };
                    return ir::Type::Pair(Box::new(first), Box::new(second));
                }
                // A `std::tuple` is a positional product just like Dart's
                // record type. Unlike `std::pair`, it has no stable
                // `first`/`second` field names to preserve, so the IR's
                // existing `Tuple` variant (introduced for out parameters)
                // is the direct representation. This maps only the type
                // boundary: `std::get` and tuple-specific operations remain
                // independently lowered expressions rather than being
                // silently treated as Dart record access.
                Some("tuple") => {
                    let argument_count =
                        unsafe { clang_sys::clang_Type_getNumTemplateArguments(cx_type) };
                    if argument_count < 0 {
                        return ir::Type::Unsupported(
                            "std::tuple with unavailable type arguments".to_owned(),
                        );
                    }
                    return ir::Type::Tuple(
                        (0..argument_count)
                            .map(|index| unsafe {
                                lower_type(clang_sys::clang_Type_getTemplateArgumentAsType(
                                    cx_type,
                                    index as c_uint,
                                ))
                            })
                            .collect(),
                    );
                }
                // `optional<T>` and the standard smart pointers are typed
                // wrappers around the presence or absence of a known value.
                // Dart's `T?` represents that shape directly.  This does not
                // claim to preserve ownership/control-block mechanics — any
                // operation that observes those mechanics remains an
                // expression-level bailout until it has a deliberate Dart
                // adapter — but it keeps signatures and fields statically
                // typed instead of turning them into SyntaxBridgeOpaque.
                Some("optional") | Some("unique_ptr") | Some("shared_ptr") | Some("weak_ptr") => {
                    let element =
                        if unsafe { clang_sys::clang_Type_getNumTemplateArguments(cx_type) } >= 1 {
                            lower_type(unsafe {
                                clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 0)
                            })
                        } else {
                            ir::Type::Unsupported(
                                "std::optional/smart pointer with no element type argument"
                                    .to_owned(),
                            )
                        };
                    return ir::Type::Nullable(Box::new(element));
                }
                // Achado 4 (`docs/plans/diagnostico-verovio-6.2.0.md`): a
                // stdlib template with no E05/E10 adapter (`std::array`,
                // `std::unordered_map`, `std::pair`, `std::optional`, ...)
                // still resolves to a real `usr`/name via
                // `clang_getTypeDeclaration` below — falling through would
                // return `Type::Record { usr, name }` pointing at a class
                // this project never declares, which prints as a bare,
                // silently-undefined type reference in the emitted Dart
                // (`dart analyze`'s `undefined_class`) instead of the honest
                // bailout every other unrepresentable type already gets.
                // Only a *named* primary template (this branch only reaches
                // `Some` when `stdlib_template_name` found one under `std`)
                // triggers this — a project-defined class is never
                // `stdlib_template_name`'d, so this can't misfire on the
                // user's own types.
                Some(other) => {
                    let spelling = unsafe {
                        type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
                    };
                    return ir::Type::Unsupported(format!("std::{other} (spelling: {spelling})"));
                }
                None => {}
            }
            let usr =
                unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(decl)) };
            // Item 9 of `docs/plans/diagnostico-verovio-6.2.0.md`: an
            // anonymous struct/class's `clang_getCursorSpelling` is
            // libclang's descriptive debug text, not empty (same quirk
            // `lower_record`'s own doc comment documents) — forcing `name`
            // empty here for that case, same as `lower_record`, routes an
            // anonymous struct field into the `Unsupported` branch just
            // below instead of a `Type::Record` naming a class that was
            // never (and, per `lower_record`, never will be) declared.
            let name = if unsafe { clang_sys::clang_Cursor_isAnonymous(decl) } != 0 {
                String::new()
            } else {
                unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(decl))
                }
            };
            if usr.is_empty() || name.is_empty() {
                // `Unexposed` doesn't always mean "maybe a stdlib class" —
                // confirmed empirically (E08): the *type of a call
                // expression* invoking an implicit instantiation of a
                // function template (`dobro(5)`, `T = int`) reports as
                // `CXType_Unexposed` too, spelled `"int"`, even though it's
                // an ordinary scalar with no declaration at all
                // (`clang_getTypeDeclaration` on it is empty, hence
                // reaching this branch). `clang_getCanonicalType` resolves
                // straight past the template-parameter-dependence that
                // produced the `Unexposed` wrapper in the first place,
                // landing on the concrete `CXType_Int` — tried only as a
                // fallback, after the stdlib/`Record` checks above, so it
                // can't change behavior for a genuine class type that
                // legitimately has no usable declaration.
                if cx_type.kind == clang_sys::CXType_Unexposed {
                    let canonical = unsafe { clang_sys::clang_getCanonicalType(cx_type) };
                    if canonical.kind != clang_sys::CXType_Unexposed {
                        return lower_type(canonical);
                    }
                }
                let spelling = unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
                };
                ir::Type::Unsupported(spelling)
            } else if unsafe {
                clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(decl))
            } != 0
                // `__va_list_tag` (F6/tarefa 07, Metade B: `va_list`'s own
                // element type, `typedef struct __va_list_tag
                // __builtin_va_list[1];`) is a *compiler builtin*, not a
                // declaration `#include`d from any real header — confirmed
                // empirically: its own `clang_Location_isInSystemHeader` is
                // false (there's no header to be "in"), so without this it
                // fell through to the ordinary `Type::Record { usr, name }`
                // branch below and printed as a bare, undeclared
                // `__va_list_tag` (inside `List<__va_list_tag>`, since
                // `va_list`'s array-of-1 shape reaches here through the
                // `CXType_ConstantArray` branch above) — `dart analyze`'s
                // `non_type_as_type_argument`. A real project declaration
                // always has a genuine file location, so this can't misfire
                // on one.
                || unsafe { cursor_has_no_real_file_location(decl) }
            {
                // A named, non-anonymous declaration with a real usr isn't
                // automatically one of *this project's* records — libstdc++
                // internals the stdlib-adapter match above doesn't name
                // (`__gnu_cxx::__normal_iterator`, the real type `.begin()`/
                // `.end()` return outside the narrow idioms this bridge
                // lowers specially) have both too, and reaching here with
                // one built `Type::Record { usr, name }` naming a class this
                // project never declares — confirmed the hard way: a typed
                // bailout then printed it as a bare, undeclared Dart type
                // argument (`_syntaxBridgeUnsupported<__normal_iterator>`),
                // which doesn't parse at all (`dart analyze`'s
                // `non_type_as_type_argument`/`undefined_class`), the exact
                // "silêncio é proibido" failure mode `Type::Record`'s own
                // fallback exists to avoid one guard up (the union check).
                // A project record is always declared in the project's own
                // source, never a system header, so this can't misfire on a
                // real one.
                let spelling = unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
                };
                ir::Type::Unsupported(spelling)
            } else {
                ir::Type::Record { usr, name }
            }
        }
        _ => {
            // `auto` locals inferred from dependent standard-library calls
            // (for example, `std::string::find`) are reported as
            // `CXType_Auto`, spelled `size_type`, rather than as the
            // `CXType_Unexposed` aliases handled above. At this fallback
            // point every structured type/template adapter has already had
            // a chance to preserve its own shape, so following a genuinely
            // different canonical kind is safe and turns the inferred scalar
            // into its normal Dart type (`int` here) instead of an opaque
            // bailout.
            let canonical = unsafe { clang_sys::clang_getCanonicalType(cx_type) };
            if canonical.kind != cx_type.kind {
                return lower_type(canonical);
            }
            let spelling = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
            };
            ir::Type::Unsupported(spelling)
        }
    }
}

/// Whether `cx_type` is one character code unit that a null-terminated C++
/// pointer conventionally uses as text.  The canonical kind, rather than the
/// spelling, keeps typedefs such as PugiXML's `char_t` on the same C-string
/// bridge as a plain `char*`; a binary `unsigned char*` still remains an
/// explicit pointer/buffer bailout.
unsafe fn is_text_character_type(cx_type: clang_sys::CXType) -> bool {
    let canonical = unsafe { clang_sys::clang_getCanonicalType(cx_type) };
    matches!(
        canonical.kind,
        clang_sys::CXType_Char_S
            | clang_sys::CXType_Char_U
            | clang_sys::CXType_WChar
            | clang_sys::CXType_Char16
            | clang_sys::CXType_Char32
    )
}

/// Lowers a non-ABI C++ function-pointer type to a typed Dart closure. The
/// enclosing pointer is intentionally discarded only after inspecting the
/// `FunctionProto` it points to: a Dart closure has the same call shape, but
/// it is not claimed to be an FFI `NativeFunction`.
unsafe fn lower_callback_type(function_type: clang_sys::CXType) -> ir::Type {
    let argument_count = unsafe { clang_sys::clang_getNumArgTypes(function_type) };
    if argument_count < 0 {
        let spelling = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(function_type))
        };
        return ir::Type::Unsupported(format!("function pointer with no prototype: {spelling}"));
    }
    let params = (0..argument_count as c_uint)
        .map(|index| lower_type(unsafe { clang_sys::clang_getArgType(function_type, index) }))
        .collect();
    let return_type = lower_type(unsafe { clang_sys::clang_getResultType(function_type) });
    ir::Type::Callback {
        return_type: Box::new(return_type),
        params,
    }
}

/// The byte aliases that have an explicit binary-buffer contract in the
/// source surface this transpiler supports. Deliberately narrower than every
/// `unsigned char*`: that raw spelling can still mean one scalar or an ABI
/// object, whereas these named aliases conventionally denote a byte span.
fn is_known_byte_buffer_type(spelling: &str) -> bool {
    matches!(
        spelling
            .trim()
            .strip_prefix("const ")
            .unwrap_or(spelling.trim())
            .trim_end_matches(" const")
            .trim(),
        "uint8_t" | "std::uint8_t" | "mz_uint8"
    )
}

/// The primary template's name (`"basic_string"`, `"vector"`) for a
/// specialization declared (transitively) in namespace `std` — `None` for
/// anything else, including a user's own template (no false positives: a
/// project-defined `std::vector`-shaped class is never confused with the
/// real one, since this checks the primary template's *enclosing
/// namespace*, not just the specialization's own spelling). `std::string`
/// itself never reaches here as a name — it's a `typedef` for
/// `basic_string<char, ...>`, and `lower_type`'s own `CXType_Typedef`
/// branch resolves through it before this is ever called.
///
/// The walk up the namespace chain (rather than checking the immediate
/// parent) is a real fix, not defensive padding: `libstdc++` (confirmed
/// live against this project's own toolchain, not assumed from
/// documentation) declares `basic_string` inside `namespace std {
/// inline namespace __cxx11 { ... } }` for its dual-ABI story — checking
/// only the direct parent finds `__cxx11`, never `std`, and every
/// `std::string` in the corpus silently fails to match. `vector` has no
/// such wrapper, so this walk is a no-op for it — one function has to
/// handle both regardless, since a future standard-library type this
/// project adapts could just as easily gain (or lose) an inline namespace
/// of its own.
unsafe fn stdlib_template_name(decl: clang_sys::CXCursor) -> Option<String> {
    let template = unsafe { clang_sys::clang_getSpecializedCursorTemplate(decl) };
    if unsafe { clang_sys::clang_Cursor_isNull(template) } != 0 {
        return None;
    }

    let mut ancestor = unsafe { clang_sys::clang_getCursorSemanticParent(template) };
    loop {
        if unsafe { clang_sys::clang_Cursor_isNull(ancestor) } != 0
            || unsafe { clang_sys::clang_getCursorKind(ancestor) }
                == clang_sys::CXCursor_TranslationUnit
        {
            return None;
        }
        let name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(ancestor))
        };
        if name == "std" {
            break;
        }
        ancestor = unsafe { clang_sys::clang_getCursorSemanticParent(ancestor) };
    }

    Some(unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(template)) })
}

/// Whether `decl` is (transitively) a specialization of `__gnu_cxx::
/// __normal_iterator` — libstdc++'s real (out-of-`std`) implementation of
/// `std::vector<T>::iterator`/`std::string::iterator`. Deliberately a
/// separate, narrow check rather than widening `stdlib_template_name`'s own
/// ancestor walk to also accept `__gnu_cxx`: that was tried first and
/// reached much further than intended — `stdlib_template_name` is also
/// `lower_stdlib_method_call`'s *general* per-call-site template-name
/// resolver, so accepting `__gnu_cxx` there changed the bailout reason (and,
/// worse, sometimes the *shape*) of every other `__normal_iterator` method
/// call in the whole codebase (`operator=`, `operator++`, ...), not just the
/// `operator*`/`operator->` dereference this bridge actually recognizes —
/// confirmed the hard way on the real Verovio 6.2.0 corpus: it unmasked a
/// pre-existing, unrelated bug in the generic manual-`for`-loop lowering
/// (previously hidden behind a whole-function bailout that this widening
/// incidentally stopped triggering) and produced genuinely unparseable Dart
/// in three real files. Used only by `lower_type`'s own `Type::ListCursor`
/// mapping and by `lower_stdlib_method_call`'s `operator*`/`operator->` arm
/// — both narrowly scoped to the one idiom each actually recognizes, so
/// nothing else in the corpus can be affected in shape or spelling.
unsafe fn is_normal_iterator_decl(decl: clang_sys::CXCursor) -> bool {
    let template = unsafe { clang_sys::clang_getSpecializedCursorTemplate(decl) };
    if unsafe { clang_sys::clang_Cursor_isNull(template) } != 0 {
        return false;
    }
    if unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(template)) }
        != "__normal_iterator"
    {
        return false;
    }
    let mut ancestor = unsafe { clang_sys::clang_getCursorSemanticParent(template) };
    loop {
        if unsafe { clang_sys::clang_Cursor_isNull(ancestor) } != 0
            || unsafe { clang_sys::clang_getCursorKind(ancestor) }
                == clang_sys::CXCursor_TranslationUnit
        {
            return false;
        }
        let name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(ancestor))
        };
        if name == "__gnu_cxx" {
            return true;
        }
        ancestor = unsafe { clang_sys::clang_getCursorSemanticParent(ancestor) };
    }
}

/// Resolves a standard-library template through a type declaration. Member
/// declarations of some specializations do not themselves expose a
/// specialized-template cursor, while the receiver type still does.
unsafe fn stdlib_template_name_of_type(cx_type: clang_sys::CXType) -> Option<String> {
    let declaration = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
    if unsafe { clang_sys::clang_Cursor_isNull(declaration) } != 0 {
        return None;
    }
    unsafe { stdlib_template_name(declaration) }
}

/// Whether a parameter declaration spells an actual C++ default argument.
///
/// A `ParmVarDecl`'s child cursors are not sufficient: libclang also exposes
/// non-type template arguments from its type (`std::array<int, 3> values`)
/// as children. Tokenizing the parameter's own extent settles that ambiguity
/// for a default written on the same declaration. A default inherited from a
/// preceding header declaration has its expression child outside this extent,
/// so it is identified by source range instead.
unsafe fn parameter_has_explicit_default(cursor: clang_sys::CXCursor) -> bool {
    let translation_unit = unsafe { clang_sys::clang_Cursor_getTranslationUnit(cursor) };
    let extent = unsafe { clang_sys::clang_getCursorExtent(cursor) };
    let mut tokens: *mut clang_sys::CXToken = std::ptr::null_mut();
    let mut token_count: c_uint = 0;
    unsafe {
        clang_sys::clang_tokenize(translation_unit, extent, &mut tokens, &mut token_count);
    }

    let has_default = if tokens.is_null() {
        false
    } else {
        (0..token_count).any(|index| {
            let token = unsafe { *tokens.add(index as usize) };
            unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTokenSpelling(
                    translation_unit,
                    token,
                )) == "="
            }
        })
    };

    if !tokens.is_null() {
        unsafe {
            clang_sys::clang_disposeTokens(translation_unit, tokens, token_count);
        }
    }

    if has_default {
        return true;
    }

    unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
            )
        })
        .any(|child| unsafe { !cursor_extent_contains_child_location(cursor, child) })
}

/// Whether `child` originates in `cursor`'s own source extent. This is the
/// distinction libclang preserves for a default argument inherited by a
/// definition: the `ParmVarDecl` belongs to the definition, but its default
/// expression still points back to the prior declaration (often a header).
/// A non-type template argument is spelled inside the parameter's own range.
unsafe fn cursor_extent_contains_child_location(
    cursor: clang_sys::CXCursor,
    child: clang_sys::CXCursor,
) -> bool {
    let extent = unsafe { clang_sys::clang_getCursorExtent(cursor) };
    let Some((start_file, start_offset)) =
        (unsafe { source_file_and_offset(clang_sys::clang_getRangeStart(extent)) })
    else {
        return false;
    };
    let Some((end_file, end_offset)) =
        (unsafe { source_file_and_offset(clang_sys::clang_getRangeEnd(extent)) })
    else {
        return false;
    };
    let Some((child_file, child_offset)) =
        (unsafe { source_file_and_offset(clang_sys::clang_getCursorLocation(child)) })
    else {
        return false;
    };

    start_file == end_file
        && start_file == child_file
        && start_offset <= child_offset
        && child_offset <= end_offset
}

unsafe fn source_file_and_offset(
    location: clang_sys::CXSourceLocation,
) -> Option<(clang_sys::CXFile, c_uint)> {
    let mut file = std::ptr::null_mut();
    let mut line = 0;
    let mut column = 0;
    let mut offset = 0;
    unsafe {
        clang_sys::clang_getSpellingLocation(
            location,
            &mut file,
            &mut line,
            &mut column,
            &mut offset,
        );
    }
    (!file.is_null()).then_some((file, offset))
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

/// Lowers a parameter with the small amount of signature context that a raw
/// type spelling alone cannot carry. A `void*` is only a byte buffer when a
/// sibling scalar parameter names *that same* pointer's length; otherwise it
/// retains `lower_type`'s explicit unsupported result.
unsafe fn lower_parameter_type(
    parameter: clang_sys::CXCursor,
    siblings: &[clang_sys::CXCursor],
) -> ir::Type {
    let cx_type = unsafe { clang_sys::clang_getCursorType(parameter) };
    if unsafe { is_void_pointer_type(cx_type) }
        && unsafe { has_matching_scalar_length_parameter(parameter, siblings) }
    {
        ir::Type::Nullable(Box::new(ir::Type::Bytes))
    } else {
        lower_type(cx_type)
    }
}

unsafe fn is_void_pointer_type(cx_type: clang_sys::CXType) -> bool {
    if cx_type.kind != clang_sys::CXType_Pointer {
        return false;
    }
    let pointee = unsafe { clang_sys::clang_getPointeeType(cx_type) };
    let canonical = unsafe { clang_sys::clang_getCanonicalType(pointee) };
    canonical.kind == clang_sys::CXType_Void
}

/// Finds a scalar length whose source name proves it belongs to `pointer`:
/// `data` + `data_size`, `pIn_buf` + `pIn_buf_size`, and their camel-case
/// equivalents. The condition deliberately does not accept a merely nearby
/// `size` parameter for an arbitrary `void*`; without the name relationship
/// that would be a guess about an ABI handle rather than a buffer contract.
unsafe fn has_matching_scalar_length_parameter(
    pointer: clang_sys::CXCursor,
    siblings: &[clang_sys::CXCursor],
) -> bool {
    let pointer_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(pointer)) };
    let pointer_key = normalized_parameter_name(&pointer_name);
    if pointer_key.is_empty() {
        return false;
    }

    siblings.iter().copied().any(|candidate| {
        if unsafe { clang_sys::clang_equalCursors(pointer, candidate) } != 0 {
            return false;
        }
        let candidate_type = unsafe { clang_sys::clang_getCursorType(candidate) };
        if lower_type(candidate_type) != ir::Type::Int {
            return false;
        }
        let candidate_name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(candidate))
        };
        let candidate_key = normalized_parameter_name(&candidate_name);
        let Some(suffix) = candidate_key.strip_prefix(&pointer_key) else {
            return false;
        };
        matches!(suffix, "size" | "length" | "len" | "count" | "bytes")
    })
}

fn normalized_parameter_name(name: &str) -> String {
    name.chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
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
    project_root: &Path,
) -> (Vec<ir::Param>, Vec<ir::Stmt>) {
    let mut params = Vec::new();
    let mut prelude = Vec::new();
    let param_cursors: Vec<clang_sys::CXCursor> = unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| unsafe { clang_sys::clang_getCursorKind(*child) } == clang_sys::CXCursor_ParmDecl)
        .collect();

    for param_cursor in param_cursors.iter().copied() {
        // A C++ parameter may legally have no name (common in an interface
        // signature that never uses it, e.g. `bool F(int, bool named)`) —
        // Dart requires every positional parameter to have one. Achado 9,
        // `verovio_6_2_0_transpile_diagnosis`: left blank, this produced a
        // parameter with no identifier at all, which `dart format` rejects.
        // Item 9 of the same diagnosis: a C++ parameter legally named after
        // a Dart reserved word (`void f(int in)`, none of Dart's reserved
        // words are reserved in C++) hits the same class of parse error —
        // `dart_safe_identifier` covers both the synthesized `arg{n}` name
        // (never reserved, so a no-op there) and a real reserved-word
        // spelling in one pass. Every reference to this parameter inside
        // the body resolves through the same function
        // (`qualified_static_member_name` → `dart_member_name`'s public
        // branch), so the two can never disagree.
        let spelling = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(param_cursor))
        };
        let param_name = dart_safe_identifier(&if spelling.is_empty() {
            format!("arg{}", params.len())
        } else {
            spelling
        });
        let cx_type = unsafe { clang_sys::clang_getCursorType(param_cursor) };
        let ty = unsafe { lower_parameter_type(param_cursor, &param_cursors) };

        // `lower_type` unwraps `const Animal&` down to the same `Type::Record`
        // a by-value `Animal` parameter resolves to (E06 — `lower_type`'s own
        // `CXType_LValueReference` branch, added for E05's `const
        // std::string&`/`const std::vector<int>&`) — so this can't tell a
        // by-value `Record` from a by-reference one just by looking at `ty`
        // anymore. Only a genuinely by-value parameter needs E03's
        // copy-on-entry clone (`examples/E03-struct-pod/NOTES.md`); a
        // reference parameter is never copied in C++ in the first place, so
        // cloning it here would silently invent a copy the original code
        // never made — checked directly against `cx_type.kind`, the type as
        // written, before `lower_type` erases the distinction.
        let is_by_value = cx_type.kind != clang_sys::CXType_LValueReference;
        if is_by_value && let ir::Type::Record { usr, name } = &ty {
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

        // A default argument (`int passo = 1`, E07) is the `ParmVarDecl`'s
        // own child expression cursor — the same "declaration's child is
        // its initializer" shape `lower_decl_stmt` already reads for a
        // local variable, just on a parameter instead — but a
        // namespace-qualified parameter type (`const std::string&`, E05)
        // *also* has its own `TypeRef`/`NamespaceRef` child cursors for the
        // type itself, with no default value involved at all. Confirmed
        // empirically (not assumed): without filtering these out, every
        // `std::string`/`std::vector` parameter picked up a bogus "default
        // value" lowered from its own type reference, breaking every E05
        // function whose parameter list has one — the same TypeRef trap
        // `lower_decl_stmt` already filters for a local variable's
        // initializer. An additional source-token check matters for a
        // parameter such as `std::array<int, 3> values`: libclang exposes
        // the non-type template argument `3` as another ParmVarDecl child,
        // indistinguishable by cursor kind from a real default expression.
        // A default argument is the only one of the two whose parameter
        // spelling contains `=`. Only looked up for a scalar/`Str`
        // parameter: a `Record`-typed default would interact with the
        // by-value clone prelude above in a way no fixture forces yet, so it
        // stays unimplemented rather than guessed at.
        let default_value = if matches!(ty, ir::Type::Record { .. })
            || !unsafe { parameter_has_explicit_default(param_cursor) }
        {
            None
        } else {
            unsafe { collect_children(param_cursor) }
                .into_iter()
                .find(|child| {
                    !matches!(
                        unsafe { clang_sys::clang_getCursorKind(*child) },
                        clang_sys::CXCursor_TypeRef
                            | clang_sys::CXCursor_NamespaceRef
                            | clang_sys::CXCursor_TemplateRef
                    )
                })
                .map(|default_cursor| unsafe {
                    default_argument_value(default_cursor, &ty, project_root, origin)
                })
        };

        params.push(ir::Param {
            name: param_name,
            ty,
            default_value,
        });
    }

    // A C++ variadic parameter (`, ...`) has no direct Dart equivalent —
    // Dart has no unlimited-arity parameter list at all — but per-argument
    // type erasure has an honest one: an *explicit*, nameable boundary,
    // never `dynamic` (AGENTS.md). A trailing optional `List<Object?> args
    // = const []` collects every argument beyond the fixed ones; the call
    // site's own lowering (`regroup_variadic_call_args`) packages them into
    // exactly that list, so `LogError('%s', str)` becomes
    // `LogError('%s', <Object?>[str])` rather than a call `dart analyze`
    // rejects as `extra_positional_arguments` (F15/tarefa 15.7 — the
    // "fronteira nomeada e explícita" this prompt asked to decide on before
    // fixing).
    if unsafe { clang_sys::clang_Cursor_isVariadic(cursor) } != 0 {
        params.push(ir::Param {
            name: "args".to_owned(),
            ty: variadic_args_type(),
            default_value: Some(ir::Expr::ListLiteral {
                items: Vec::new(),
                ty: variadic_args_type(),
                origin: origin.clone(),
            }),
        });
    }

    (params, prelude)
}

/// The Dart type a C++ variadic parameter's collected trailing arguments
/// get — see `collect_params_with_clone_prelude`'s own doc comment on why
/// this, not `dynamic`.
fn variadic_args_type() -> ir::Type {
    ir::Type::List(Box::new(ir::Type::Nullable(Box::new(ir::Type::Object))))
}

/// Repackages a variadic C++ call's trailing arguments into the single
/// `List<Object?>` the Dart signature exposes for them
/// (`collect_params_with_clone_prelude`'s own trailing `args` parameter,
/// F15/tarefa 15.7). `referenced` is the callee's *declaration* cursor —
/// `clang_Cursor_getNumArguments` on it reports the fixed parameter count
/// only, never counting the `...` itself (confirmed empirically, the same
/// API this module already reads a call's own argument count from). A
/// non-variadic callee, or a variadic call that supplies none of its
/// trailing arguments (Dart's own default `const []` already covers that
/// case), comes back unchanged.
unsafe fn regroup_variadic_call_args(
    mut args: Vec<ir::Expr>,
    referenced: clang_sys::CXCursor,
    origin: &ir::Origin,
) -> Vec<ir::Expr> {
    if unsafe { clang_sys::clang_Cursor_isVariadic(referenced) } == 0 {
        return args;
    }
    let fixed_count = unsafe { clang_sys::clang_Cursor_getNumArguments(referenced) };
    let Ok(fixed_count) = usize::try_from(fixed_count) else {
        return args;
    };
    if args.len() <= fixed_count {
        return args;
    }
    let variadic_tail = args.split_off(fixed_count);
    args.push(ir::Expr::ListLiteral {
        items: variadic_tail,
        ty: variadic_args_type(),
        origin: origin.clone(),
    });
    args
}

/// One `case`/`default` label found while unwrapping a `CaseStmt`/
/// `DefaultStmt` chain (`unwrap_case_labels`) — `Value` doesn't lower its
/// expression eagerly, since a label found this way might turn out to be
/// unrepresentable (`lower_switch_stmt` still needs to decide whether the
/// *whole switch* stays a bailout before committing to any of its parts).
enum SwitchLabel {
    Value(clang_sys::CXCursor),
    Default,
}

/// `switch`/`case`/`default` (`docs/plans/bailouts-verovio-6.2.0.md`'s
/// `switch` family, 148 occurrences in the 2026-08-20 real-Verovio
/// diagnosis run): `CXCursor_SwitchStmt` → [condition, `CompoundStmt`].
/// Confirmed via `clang -Xclang -ast-dump` (not guessed) that the body's
/// `CompoundStmt` children are a *flat* list where a `CaseStmt`/
/// `DefaultStmt` cursor sits inline at its label's position, but each one's
/// own single child (`Stmt::getSubStmt()`) is *only* the statement
/// immediately following the label — which is itself another
/// `CaseStmt`/`DefaultStmt` for a stacked label (`case 2: case 3: ...`), not
/// the whole rest of that case's body. Every statement after that first one
/// is a *sibling* of the label cursor in the same flat list, not a
/// descendant of it. `unwrap_case_labels` untangles the stacked-label
/// nesting; the loop below re-attaches each subsequent sibling statement to
/// whichever case/default is currently open.
unsafe fn lower_switch_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Stmt {
    let children = unsafe { collect_children(cursor) };
    let [scrutinee_cursor, body_cursor] = children.as_slice() else {
        return ir::Stmt::Unsupported {
            reason: format!(
                "SwitchStmt had {} children, expected condition+body",
                children.len()
            ),
            origin,
        };
    };
    if unsafe { clang_sys::clang_getCursorKind(*body_cursor) } != clang_sys::CXCursor_CompoundStmt {
        return ir::Stmt::Unsupported {
            reason: "SwitchStmt's body was not a CompoundStmt".to_owned(),
            origin,
        };
    }

    let scrutinee = unsafe { lower_expr(*scrutinee_cursor, project_root) };
    let body_children = unsafe { collect_children(*body_cursor) };

    let mut cases: Vec<ir::SwitchCase> = Vec::new();
    let mut default: Option<Vec<ir::Stmt>> = None;
    // `None` before the first label is seen (a statement there would be
    // unreachable dead code with nothing to attach it to — malformed
    // enough in practice that bailing the whole switch out is fine);
    // `Some(true)` while `default:`'s body is open, `Some(false)` while a
    // `cases[cases.len() - 1]`'s body is open.
    let mut target_is_default: Option<bool> = None;

    for child in body_children {
        let child_kind = unsafe { clang_sys::clang_getCursorKind(child) };
        if child_kind == clang_sys::CXCursor_CaseStmt
            || child_kind == clang_sys::CXCursor_DefaultStmt
        {
            let Some((labels, first_stmt)) = (unsafe { unwrap_case_labels(child) }) else {
                return ir::Stmt::Unsupported {
                    reason: "CaseStmt/DefaultStmt had an unexpected shape (a GNU case range, \
                              `case a ... b:`, isn't supported yet)"
                        .to_owned(),
                    origin,
                };
            };
            let mut values = Vec::new();
            let mut is_default = false;
            for label in labels {
                match label {
                    SwitchLabel::Value(value_cursor) => {
                        let Some(value) = (unsafe {
                            switch_case_label_value(value_cursor, project_root, &origin)
                        }) else {
                            return ir::Stmt::Unsupported {
                                reason: "a case label is not a Dart-representable constant \
                                         pattern (a literal, or an enum/const reference)"
                                    .to_owned(),
                                origin,
                            };
                        };
                        values.push(value);
                    }
                    SwitchLabel::Default => is_default = true,
                }
            }
            let body = match first_stmt {
                Some(first_stmt) => unsafe { lower_stmt_into(first_stmt, project_root) },
                None => Vec::new(),
            };
            if is_default {
                default = Some(body);
                target_is_default = Some(true);
            } else {
                cases.push(ir::SwitchCase {
                    values,
                    body,
                    label: None,
                });
                target_is_default = Some(false);
            }
            continue;
        }

        let lowered = unsafe { lower_stmt_into(child, project_root) };
        match target_is_default {
            Some(true) => default
                .as_mut()
                .expect("target_is_default is only Some(true) once default is Some")
                .extend(lowered),
            Some(false) => cases
                .last_mut()
                .expect("target_is_default is only Some(false) once cases is non-empty")
                .body
                .extend(lowered),
            None => {
                return ir::Stmt::Unsupported {
                    reason: "statement in a switch body before any case/default label isn't \
                              supported yet"
                        .to_owned(),
                    origin,
                };
            }
        }
    }

    // A C++ `case` body that already ends in `return`/`throw`/`continue`/
    // `break` often carries a further `break;` right after it — many style
    // guides require a terminator on every case regardless, and it's
    // harmless in C++. Dart flags that trailing statement `dead_code` (real
    // trigger: `accid.dart`'s `GetAccidGlyph`, family F15/tarefa 15.1), so
    // anything lowered after the case's first terminator is truncated here,
    // before the fallthrough pass below inspects each body's last statement.
    for case in &mut cases {
        truncate_after_case_terminator(&mut case.body);
    }
    if let Some(default) = &mut default {
        truncate_after_case_terminator(default);
    }

    // Dart, unlike C++, rejects implicit fallthrough out of a non-empty
    // `case` as a compile error — but unlike C++'s error, Dart *does* have
    // an explicit fallthrough form: `continue <label>;`, jumping into a
    // labeled sibling `case`/`default` (confirmed real Dart syntax, not
    // guessed). Every non-empty case whose body doesn't already end in a
    // jump gets that continue appended, targeting the very next case in
    // source order — which in turn gets that label attached
    // (`SwitchCase::label`, printed by `emit::dart` right before its own
    // `case` line(s)). The textually last clause (the last case when there
    // is no `default`, or `default` itself, since `emit::dart` always
    // prints `default:` last regardless of its true source position) needs
    // no terminator at all — falling out the bottom of a `switch` is
    // already valid in both C++ and Dart. Falling out of a case *into*
    // `default` specifically stays a bailout for now: `default` has no
    // label slot of its own to target (`Stmt::Switch.default` is a bare
    // `Vec<Stmt>`), a real but narrower gap than general fallthrough was.
    let mut fallthrough_label_count = 0u32;
    for index in 0..cases.len() {
        let body_ends_safely = cases[index].body.is_empty()
            || matches!(cases[index].body.last(), Some(last) if is_case_terminator(last));
        if body_ends_safely {
            continue;
        }
        let is_textually_last_clause = index + 1 == cases.len() && default.is_none();
        if is_textually_last_clause {
            continue;
        }
        if index + 1 >= cases.len() {
            // Falls through past the last case into `default` — not
            // representable yet (see doc comment above).
            return ir::Stmt::Unsupported {
                reason: "a case falls through into the next one without an explicit break/\
                          continue/return/throw — Dart has no implicit fallthrough"
                    .to_owned(),
                origin,
            };
        }
        fallthrough_label_count += 1;
        let label = format!("_syntaxBridgeCase{fallthrough_label_count}");
        cases[index].body.push(ir::Stmt::ContinueLabel {
            label: label.clone(),
            origin: origin.clone(),
        });
        cases[index + 1].label = Some(label);
    }
    // `default`'s own body needs no such check: `emit::dart` always prints
    // it last, so it never needs to fall through anywhere — the same
    // "textually last clause" exemption the loop above already gives the
    // last `case` when there is no `default`.

    ir::Stmt::Switch {
        scrutinee,
        cases,
        default,
        origin,
    }
}

/// Drops every statement after a case body's first terminator
/// (`break`/`continue`/`return`/`throw`) — anything past it is unreachable,
/// most commonly a redundant `break;` right after a `return` (see the doc
/// comment at this function's call site).
fn truncate_after_case_terminator(body: &mut Vec<ir::Stmt>) {
    if let Some(index) = body.iter().position(is_case_terminator) {
        body.truncate(index + 1);
    }
}

fn is_case_terminator(stmt: &ir::Stmt) -> bool {
    matches!(
        stmt,
        ir::Stmt::Break { .. }
            | ir::Stmt::Continue { .. }
            | ir::Stmt::ContinueLabel { .. }
            | ir::Stmt::Return { .. }
            | ir::Stmt::Throw { .. }
    )
}

/// A `case` label's value, made Dart-safe. Dart's switch-case patterns only
/// accept a narrow set of "constant pattern" shapes — a literal or a
/// reference to a named constant (e.g. an enum value) — and reject an
/// inline operator expression outright (confirmed via real `dart analyze`:
/// "The binary operator << is not supported as a constant pattern"), even
/// though C++ accepts *any* integer-constant-expression as a case label
/// (confirmed as a real Verovio trigger: `svgdevicecontext.cpp`'s
/// `GetColor`, `case 255 << 16 | 255 << 8 | 255:`). `lower_expr` on such a
/// cursor already produces a correct-but-Dart-illegal `Expr::Binary` chain,
/// so this only special-cases the shapes that are already Dart-safe as-is
/// and otherwise folds the whole expression to its compile-time integer
/// value via `clang_Cursor_Evaluate` — every C++ case label is guaranteed
/// to already be an integer constant expression, so this fold only fails
/// for a label shape this function doesn't yet recognize as safe (`None`,
/// which `lower_switch_stmt` turns into a whole-switch bailout).
/// A parameter default's value, made Dart-const-safe (F15/tarefa 15.4).
/// Dart requires a parameter default to be a compile-time constant
/// expression, but `lower_expr`'s ordinary lowering can turn a genuine C++
/// constant into a Dart *runtime* expression: an integer literal implicitly
/// converted to the parameter's `double` type becomes a `Expr::Convert`,
/// printed as `0.toDouble()` (a method call — real trigger:
/// `editortoolkit_neume.dart:2695`'s `distanceToBB`), and an unscoped enum
/// constant implicitly converted to `int` becomes `PenStyle.PEN_SOLID.value`
/// (a getter call, never constant even on a const receiver — real trigger:
/// `devicecontext.dart:146`'s `SetBackground`). Both are real C++ constant
/// expressions (`clang_Cursor_Evaluate` succeeds on both), so — mirroring
/// `switch_case_label_value`'s own already-safe-shapes-pass-through-else-fold
/// pattern — this only reaches for the evaluator when the naive lowering
/// isn't already one of Dart's own constant-pattern shapes.
unsafe fn default_argument_value(
    default_cursor: clang_sys::CXCursor,
    param_ty: &ir::Type,
    project_root: &Path,
    origin: &ir::Origin,
) -> ir::Expr {
    let lowered = unsafe { lower_expr(default_cursor, project_root) };
    if matches!(
        lowered,
        ir::Expr::Ref { .. }
            | ir::Expr::IntLiteral { .. }
            | ir::Expr::DoubleLiteral { .. }
            | ir::Expr::BoolLiteral { .. }
            | ir::Expr::StringLiteral { .. }
    ) {
        return lowered;
    }

    match param_ty {
        ir::Type::Double => {
            // `clang_Cursor_Evaluate` folds a cursor by its own type, not
            // the parameter's — an integer-literal default implicitly
            // converted to `double` (`double rotate = 0`) evaluates as
            // `CXEval_Int`, not `CXEval_Float`, so both are tried.
            if let Some(value) = unsafe { evaluate_float_eval_result(default_cursor) } {
                return ir::Expr::DoubleLiteral {
                    value,
                    origin: origin.clone(),
                };
            }
            if let Some(value) = unsafe { evaluate_int_eval_result(default_cursor) } {
                return ir::Expr::DoubleLiteral {
                    value: value as f64,
                    origin: origin.clone(),
                };
            }
        }
        ir::Type::Int => {
            if let Some(value) = unsafe { evaluate_int_eval_result(default_cursor) } {
                return ir::Expr::IntLiteral {
                    value,
                    origin: origin.clone(),
                };
            }
        }
        _ => {}
    }
    lowered
}

unsafe fn switch_case_label_value(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    let lowered = unsafe { lower_expr(cursor, project_root) };
    if matches!(
        lowered,
        ir::Expr::Ref { .. }
            | ir::Expr::IntLiteral { .. }
            | ir::Expr::BoolLiteral { .. }
            | ir::Expr::StringLiteral { .. }
    ) {
        return Some(lowered);
    }

    unsafe { evaluate_int_eval_result(cursor) }.map(|value| ir::Expr::IntLiteral {
        value,
        origin: origin.clone(),
    })
}

/// Untangles a `CaseStmt`/`DefaultStmt` chain into every label it stacks
/// (`case 2: case 3: baz();` → `[Value(2), Value(3)]`) and the cursor of the
/// first real (non-label) statement the chain bottoms out at, if any —
/// `lower_switch_stmt`'s own doc comment has the empirically-confirmed AST
/// shape this unwinds. `None` for a shape this doesn't recognize (a GNU
/// case range, `CaseStmt` with other than exactly 1 or 2 children).
unsafe fn unwrap_case_labels(
    cursor: clang_sys::CXCursor,
) -> Option<(Vec<SwitchLabel>, Option<clang_sys::CXCursor>)> {
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
    if kind == clang_sys::CXCursor_CaseStmt {
        let children = unsafe { collect_children(cursor) };
        let (label, sub) = match children.as_slice() {
            [value_cursor, sub_cursor] => (SwitchLabel::Value(*value_cursor), Some(*sub_cursor)),
            [value_cursor] => (SwitchLabel::Value(*value_cursor), None),
            _ => return None,
        };
        let (mut labels, first_stmt) = match sub {
            Some(sub_cursor)
                if unsafe { clang_sys::clang_getCursorKind(sub_cursor) }
                    == clang_sys::CXCursor_CaseStmt
                    || unsafe { clang_sys::clang_getCursorKind(sub_cursor) }
                        == clang_sys::CXCursor_DefaultStmt =>
            {
                unsafe { unwrap_case_labels(sub_cursor) }?
            }
            other => (Vec::new(), other),
        };
        labels.insert(0, label);
        Some((labels, first_stmt))
    } else if kind == clang_sys::CXCursor_DefaultStmt {
        let children = unsafe { collect_children(cursor) };
        let sub = match children.as_slice() {
            [sub_cursor] => Some(*sub_cursor),
            [] => None,
            _ => return None,
        };
        let (mut labels, first_stmt) = match sub {
            Some(sub_cursor)
                if unsafe { clang_sys::clang_getCursorKind(sub_cursor) }
                    == clang_sys::CXCursor_CaseStmt
                    || unsafe { clang_sys::clang_getCursorKind(sub_cursor) }
                        == clang_sys::CXCursor_DefaultStmt =>
            {
                unsafe { unwrap_case_labels(sub_cursor) }?
            }
            other => (Vec::new(), other),
        };
        labels.insert(0, SwitchLabel::Default);
        Some((labels, first_stmt))
    } else {
        None
    }
}

unsafe fn find_compound_stmt_child(cursor: clang_sys::CXCursor) -> Option<clang_sys::CXCursor> {
    unsafe { collect_children(cursor) }
        .into_iter()
        .find(|child| unsafe { clang_sys::clang_getCursorKind(*child) } == clang_sys::CXCursor_CompoundStmt)
}

unsafe fn lower_compound_stmt(cursor: clang_sys::CXCursor, project_root: &Path) -> Vec<ir::Stmt> {
    let children = unsafe { collect_children(cursor) };
    let mut result = Vec::new();
    let mut index = 0;
    while index < children.len() {
        // F10/tarefa 13's "declare, guard, dereference" idiom
        // (`lower_find_iterator_guard_idiom`) spans two adjacent sibling
        // statements — a shape `lower_stmt_into`'s per-child dispatch can
        // never see on its own, since it only ever looks at one statement
        // cursor at a time. Checked here, one lookahead pair at a time,
        // before falling back to the ordinary single-statement path.
        if index + 1 < children.len()
            && let Some(fused) = unsafe {
                lower_find_iterator_guard_idiom(children[index], children[index + 1], project_root)
            }
        {
            result.extend(fused);
            index += 2;
            continue;
        }
        result.extend(unsafe { lower_stmt_into(children[index], project_root) });
        index += 1;
    }
    result
}

/// Lowers one statement cursor into zero, one, or many `ir::Stmt`s — the
/// building block both `lower_compound_stmt` (one call per child) and
/// `lower_branch` (one call on the branch cursor itself) share:
///
/// - a bare `;` (`CXCursor_NullStmt`, C++'s "nothing happens here")
///   contributes nothing — never a `Stmt::Unsupported` bailout for a
///   construct that has no effect to lose in the first place;
/// - a bare `{ ... }` scoping block used as an ordinary statement (not an
///   `if`/`while`/`for` body, already unwrapped before this is ever called
///   on one) inlines its own statements directly into the caller's list.
///   This IR has no nested-block statement node to hold one otherwise, and
///   flattening loses only C++'s block-local scoping — every name this IR
///   lowers already has a scope no narrower than its enclosing function in
///   practice, so the flatten is observationally the same program;
/// - anything else lowers through the ordinary single-statement path.
unsafe fn lower_stmt_into(cursor: clang_sys::CXCursor, project_root: &Path) -> Vec<ir::Stmt> {
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
    if kind == clang_sys::CXCursor_NullStmt {
        return Vec::new();
    }
    // `delete ptr;` — real triggers found by grepping the extracted
    // Verovio source directly (`layer.cpp`'s `delete m_staffDefClef;`,
    // `toolkit.cpp`'s `delete m_editorToolkit;`, among many others), every
    // one a plain field/variable operand. This IR's current pointer
    // representation (`Nullable(Record)`, `mapping::pointer_options_for`'s
    // `"referencia-anulavel"` case) never tracks ownership — there is no
    // `Owned<T>`/`dispose()` this could route to yet (`docs/plans/
    // bailouts-verovio-6.2.0.md`'s own pointer-and-ownership table still
    // lists that as future work) — so the only representation this project
    // has *is* a GC-managed Dart reference, for which manual deletion is
    // simply a no-op: omitted the same way `NullStmt` already is, not
    // routed through a bailout for a construct whose current representation
    // has no runtime effect to lose.
    if kind == clang_sys::CXCursor_CXXDeleteExpr {
        return Vec::new();
    }
    if kind == clang_sys::CXCursor_CompoundStmt {
        return unsafe { lower_compound_stmt(cursor, project_root) };
    }
    if kind == clang_sys::CXCursor_DeclStmt {
        let origin = stmt_origin(cursor, project_root);
        if let Some(statements) = unsafe { lower_multi_decl_stmt(cursor, project_root, &origin) } {
            return statements;
        }
    }
    if kind == clang_sys::CXCursor_IfStmt {
        let origin = stmt_origin(cursor, project_root);
        if let Some(statements) =
            unsafe { lower_if_with_out_param_call(cursor, project_root, &origin) }
        {
            return statements;
        }
    }
    if kind == clang_sys::CXCursor_BinaryOperator
        && unsafe { clang_sys::clang_getCursorBinaryOperatorKind(cursor) }
            == clang_sys::CXBinaryOperator_Assign
    {
        let origin = stmt_origin(cursor, project_root);
        if let Some(statements) =
            unsafe { lower_string_byte_assign_stmt(cursor, project_root, &origin) }
        {
            return statements;
        }
    }
    if kind == clang_sys::CXCursor_CallExpr {
        let origin = stmt_origin(cursor, project_root);
        if let Some(statements) = unsafe { lower_std_swap_stmt(cursor, project_root, &origin) } {
            return statements;
        }
    }
    vec![unsafe { lower_stmt(cursor, project_root) }]
}

/// `std::swap(a, b);` (F6/tarefa 07, Metade A's "outros" bucket) — Dart has
/// no free `swap` function, and unlike `std::max`/`std::abs` this isn't a
/// pure-value call that can rewrite to a single expression: it mutates both
/// operands, so it has to expand into three statements the same way
/// `lower_string_byte_assign_stmt` already expands a byte-indexed string
/// write — exactly why this lives in `lower_stmt_into`, not `lower_stmt`.
/// `None` for anything that isn't a two-argument call to `std::swap` on two
/// plain assignable lvalues; once that shape is confirmed, an
/// unassignable-target pair still commits to an honest
/// `Stmt::Unsupported` rather than falling through to `lower_stmt`'s
/// generic `ExprStmt` path, which would print a literal, undefined `swap(a,
/// b);` in Dart.
unsafe fn lower_std_swap_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<Vec<ir::Stmt>> {
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0
        || unsafe { clang_sys::clang_getCursorKind(referenced) } != clang_sys::CXCursor_FunctionDecl
    {
        return None;
    }
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    if name != "swap"
        || unsafe {
            clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
                referenced,
            ))
        } == 0
        || !unsafe { free_function_reachable_from_std(referenced) }
    {
        return None;
    }

    if unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) } != 2 {
        return Some(vec![ir::Stmt::Unsupported {
            reason: "unsupported argument shape for std::swap".to_owned(),
            origin: origin.clone(),
        }]);
    }
    let lhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
    let rhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 1) };
    let lhs = unsafe { lower_expr(lhs_cursor, project_root) };
    let rhs = unsafe { lower_expr(rhs_cursor, project_root) };
    if unassignable_target_reason(&lhs).is_some() || unassignable_target_reason(&rhs).is_some() {
        return Some(vec![ir::Stmt::Unsupported {
            reason: "std::swap operand is not representable as a Dart assignment target".to_owned(),
            origin: origin.clone(),
        }]);
    }

    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(lhs_cursor) });
    let temp_name = "_syntaxBridgeSwapTemp".to_owned();
    Some(vec![
        ir::Stmt::VarDecl {
            name: temp_name.clone(),
            ty: ty.clone(),
            init: Some(lhs.clone()),
            origin: origin.clone(),
        },
        ir::Stmt::ExprAssign {
            target: lhs,
            value: rhs.clone(),
            origin: origin.clone(),
        },
        ir::Stmt::ExprAssign {
            target: rhs,
            value: ir::Expr::Ref {
                name: temp_name,
                ty,
                origin: origin.clone(),
            },
            origin: origin.clone(),
        },
    ])
}

/// `target[index] = value;` where `target[index]` reads as a byte-indexed
/// `std::string` access (round 21, real trigger — grepped directly,
/// `ioabc.cpp`'s `keyString[i] = tolower(keyString[i]);`,
/// `json/jsonxx.cc`'s `input[size - 2] = ' ';`). Dart's `String` has no
/// in-place indexed assignment (it's immutable) — the whole target
/// variable/field is reassigned instead, from a byte buffer round-tripped
/// through the same UTF-8 encoding `Expr::StringByteAt`'s *read* side
/// already uses (`utf8.encode`/`.indexOf` — this bridge's own byte model
/// for a string, not UTF-16 code units): encode to bytes, write the one
/// byte, decode back. Three statements from what was one C++ statement —
/// exactly why this lives in `lower_stmt_into` (already returns
/// `Vec<Stmt>` for `DeclStmt`/`CompoundStmt`/the out-param `if` pattern),
/// not `lower_stmt` itself. `None` (falling through to the ordinary
/// `lower_assign_stmt`, unchanged) for any assignment whose lowered LHS
/// isn't exactly `Expr::StringByteAt`, or whose own target isn't a simple
/// assignable lvalue (`is_assignable_lvalue`) — the same conservative
/// bar every other rewrite in this module holds itself to.
unsafe fn lower_string_byte_assign_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<Vec<ir::Stmt>> {
    let children = unsafe { collect_children(cursor) };
    let [lhs_cursor, rhs_cursor] = children.as_slice() else {
        return None;
    };
    let lhs = unsafe { lower_expr(*lhs_cursor, project_root) };
    let ir::Expr::StringByteAt {
        target: string_target,
        index,
        ..
    } = lhs
    else {
        return None;
    };
    if !string_target.is_assignable_lvalue() {
        return None;
    }
    let value = unsafe { lower_expr(*rhs_cursor, project_root) };

    let bytes_name = "_syntaxBridgeStringBytes".to_owned();
    let bytes_ref = ir::Expr::Ref {
        name: bytes_name.clone(),
        ty: ir::Type::List(Box::new(ir::Type::Int)),
        origin: origin.clone(),
    };
    Some(vec![
        ir::Stmt::VarDecl {
            name: bytes_name,
            ty: ir::Type::List(Box::new(ir::Type::Int)),
            init: Some(ir::Expr::Call {
                base_qualifier: None,
                target: None,
                callee_usr: String::new(),
                callee_name: "utf8.encode".to_owned(),
                args: vec![(*string_target).clone()],
                ty: ir::Type::List(Box::new(ir::Type::Int)),
                origin: origin.clone(),
            }),
            origin: origin.clone(),
        },
        ir::Stmt::ExprAssign {
            target: ir::Expr::Index {
                target: Box::new(bytes_ref.clone()),
                index,
                ty: ir::Type::Int,
                origin: origin.clone(),
            },
            value,
            origin: origin.clone(),
        },
        ir::Stmt::ExprAssign {
            target: *string_target,
            value: ir::Expr::Call {
                base_qualifier: None,
                target: None,
                callee_usr: String::new(),
                callee_name: "utf8.decode".to_owned(),
                args: vec![bytes_ref],
                ty: ir::Type::Str,
                origin: origin.clone(),
            },
            origin: origin.clone(),
        },
    ])
}

/// `if (chamada(...))`/`if (!chamada(...))` where `chamada` resolves to a
/// non-`void` out-param-bridged function (round 20 — real trigger
/// `editortoolkit_neume.cpp:92`'s `if (this->ParseDragAction(json.get<
/// jsonxx::Object>("param"), &elementId, &x, &y))`). The bridged callee
/// now returns `(bool, ...)`, not `bool` — using it directly as an `if`
/// condition, the way this idiom's *value* is actually consumed, needs a
/// temporary holding the whole tuple, one assignment per out-param target,
/// and the `if` itself testing only the tuple's own first slot. This is
/// exactly why `Stmt::TupleAssign`'s own bare-statement form (used by a
/// *discarded*-return call, `lower_stmt`'s `CXCursor_CallExpr` branch)
/// isn't reused here: this call's return value *is* consumed, just not by
/// assignment. Returns `None` (never a guess) for anything not exactly
/// this shape — the call is still `lower_stmt`'s ordinary `IfStmt` path
/// otherwise, and its condition still lowers through the *generic*
/// expression path, whose own `&x`-is-`Unsupported` rule for a bare scalar
/// pointer (`is_non_const_scalar_out_param_type`'s own doc comment) is what
/// keeps every *other*, unrecognized use of a non-`void` bridged call
/// (nested in a larger boolean expression, a `while` condition, assigned
/// to a variable, ...) an honest bailout instead of a silent type mismatch
/// against the callee's real (tuple) Dart signature.
unsafe fn lower_if_with_out_param_call(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<Vec<ir::Stmt>> {
    let children = unsafe { collect_children(cursor) };
    let (condition_cursor, then_cursor, else_cursor) = match children.as_slice() {
        [condition_cursor, then_cursor] => (*condition_cursor, *then_cursor, None),
        [condition_cursor, then_cursor, else_cursor] => {
            (*condition_cursor, *then_cursor, Some(*else_cursor))
        }
        _ => return None,
    };

    let condition_cursor = unsafe { unwrap_transparent_value_cursor(condition_cursor) };
    // `CXUnaryOperator_Not` (9) is bitwise `~`; logical `!` is
    // `CXUnaryOperator_LNot` (10) — confirmed directly against clang-sys's
    // own constants after this comparison silently never matched with the
    // wrong one.
    let is_negated = unsafe { clang_sys::clang_getCursorKind(condition_cursor) }
        == clang_sys::CXCursor_UnaryOperator
        && unsafe { clang_sys::clang_getCursorUnaryOperatorKind(condition_cursor) }
            == clang_sys::CXUnaryOperator_LNot;
    let call_cursor = if is_negated {
        let not_children = unsafe { collect_children(condition_cursor) };
        let [operand_cursor] = not_children.as_slice() else {
            return None;
        };
        unsafe { unwrap_transparent_value_cursor(*operand_cursor) }
    } else {
        condition_cursor
    };
    if unsafe { clang_sys::clang_getCursorKind(call_cursor) } != clang_sys::CXCursor_CallExpr {
        return None;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(call_cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    let out_indices = unsafe { call_out_param_arg_indices(referenced) };
    if out_indices.is_empty() {
        return None;
    }
    let leading_ty = unsafe { out_param_bridge_leading_return_type(referenced, &out_indices) }?;
    if !matches!(leading_ty, ir::Type::Bool | ir::Type::Int) {
        // A status type this bridge doesn't yet know how to turn into a
        // Dart boolean condition (no fixture forces one) — bail rather
        // than guess.
        return None;
    }

    let target_cursors: Option<Vec<clang_sys::CXCursor>> = out_indices
        .iter()
        .map(|&index| unsafe { out_arg_target_cursor(referenced, call_cursor, index) })
        .collect();
    let target_cursors = target_cursors?;
    let out_param_targets: Vec<ir::Expr> = target_cursors
        .into_iter()
        .map(|target_cursor| unsafe { lower_expr(target_cursor, project_root) })
        .collect();
    // An out-param target that itself failed to lower to a plain
    // assignable lvalue can't be placed on the left of the `Stmt::ExprAssign`
    // this function builds below (`unassignable_target_reason`'s own doc
    // comment). Bailing out to `None` here — rather than trying to build a
    // half-formed `Stmt::Unsupported` mid-construction — falls back to this
    // condition's ordinary (non-out-param-bridged) `IfStmt` lowering, whose
    // own generic expression path already produces an honest bailout for
    // exactly this failure, per this function's own doc comment.
    if out_param_targets
        .iter()
        .any(|target| unassignable_target_reason(target).is_some())
    {
        return None;
    }

    let mut call_value = unsafe { lower_expr(call_cursor, project_root) };
    if let ir::Expr::Call { args, .. } = &mut call_value {
        for (&index, target) in out_indices.iter().zip(&out_param_targets) {
            if let Some(arg) = args.get_mut(index) {
                *arg = target.clone();
            }
        }
    }

    // Recomputed straight from `referenced`'s own parameter cursors, the
    // same way `apply_out_param_bridge` derives each out-param's pointee
    // type for its own tuple — not read back off `out_param_targets`,
    // whose lowered `Expr`s don't uniformly expose their own static type
    // through one accessor this module has.
    let out_param_types: Vec<ir::Type> = out_indices
        .iter()
        .map(|&index| {
            let param_cursor =
                unsafe { clang_sys::clang_Cursor_getArgument(referenced, index as c_uint) };
            lower_type(unsafe {
                clang_sys::clang_getPointeeType(clang_sys::clang_getCursorType(param_cursor))
            })
        })
        .collect();
    let temp_ty = ir::Type::Tuple(
        std::iter::once(leading_ty.clone())
            .chain(out_param_types.iter().cloned())
            .collect(),
    );
    let temp_name = "_syntaxBridgeIfCallTemp".to_owned();
    let mut statements = vec![ir::Stmt::VarDecl {
        name: temp_name.clone(),
        ty: temp_ty.clone(),
        init: Some(call_value),
        origin: origin.clone(),
    }];
    for (index, (target, ty)) in out_param_targets
        .into_iter()
        .zip(out_param_types)
        .enumerate()
    {
        statements.push(ir::Stmt::ExprAssign {
            target,
            value: ir::Expr::FieldAccess {
                target: Box::new(ir::Expr::Ref {
                    name: temp_name.clone(),
                    ty: temp_ty.clone(),
                    origin: origin.clone(),
                }),
                field: format!("${}", index + 2),
                ty,
                origin: origin.clone(),
            },
            origin: origin.clone(),
        });
    }

    let temp_first_field = ir::Expr::FieldAccess {
        target: Box::new(ir::Expr::Ref {
            name: temp_name,
            ty: temp_ty,
            origin: origin.clone(),
        }),
        field: "$1".to_owned(),
        ty: leading_ty.clone(),
        origin: origin.clone(),
    };
    let condition = match leading_ty {
        ir::Type::Int => ir::Expr::Convert {
            operand: Box::new(temp_first_field),
            ty: ir::Type::Bool,
            origin: origin.clone(),
        },
        _ => temp_first_field,
    };
    let condition = if is_negated {
        ir::Expr::Unary {
            op: ir::UnaryOp::Not,
            operand: Box::new(condition),
            ty: ir::Type::Bool,
            origin: origin.clone(),
        }
    } else {
        condition
    };

    statements.push(ir::Stmt::If {
        condition,
        then_branch: unsafe { lower_branch(then_cursor, project_root) },
        else_branch: match else_cursor {
            Some(else_cursor) => unsafe { lower_branch(else_cursor, project_root) },
            None => Vec::new(),
        },
        origin: origin.clone(),
    });
    Some(statements)
}

/// Lowers an `if`/`while`/`for` branch that may or may not be a braced
/// block (`if (x) return;` has no `CompoundStmt` child at all — its single
/// statement cursor stands in directly).
unsafe fn lower_branch(cursor: clang_sys::CXCursor, project_root: &Path) -> Vec<ir::Stmt> {
    unsafe { lower_stmt_into(cursor, project_root) }
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
            | clang_sys::CXCursor_CharacterLiteral
            | clang_sys::CXCursor_FloatingLiteral
            | clang_sys::CXCursor_StringLiteral
            | clang_sys::CXCursor_CXXBoolLiteralExpr
            | clang_sys::CXCursor_BinaryOperator
            | clang_sys::CXCursor_UnaryOperator
            | clang_sys::CXCursor_CallExpr
            | clang_sys::CXCursor_MemberRefExpr
            | clang_sys::CXCursor_CXXThisExpr
            // `(void)fmt;` (E13's `LogDebug` stub, the idiomatic C++ way to
            // silence an unused-parameter warning) — a `CStyleCastExpr` used
            // directly as a statement. Routing it through `Stmt::ExprStmt`
            // instead of `Stmt::Unsupported` matters regardless of whether
            // `lower_expr` can represent the cast's operand: an `ExprStmt`
            // wrapping an expression `lower_expr` can't handle only throws
            // at that one call site if actually reached, while a bare
            // `Stmt::Unsupported` bails the *whole enclosing function* out
            // (`emit::dart::first_unsupported_in_list`) on the theory that a
            // statement shape it can't even recognize might have declared
            // state later code depends on — not a concern here, since the
            // statement shape itself (a cast) is perfectly well understood.
            // (`lower_expr`'s own cast-to-`void` branch discards the operand
            // outright when it *can* represent it — caso 3 of
            // `docs/plans/verovio-6.2-pointer-types.md` made that the common
            // case for `(void)fmt;` once `const char*` stopped being
            // `Unsupported`.)
            | clang_sys::CXCursor_CXXStaticCastExpr
            | clang_sys::CXCursor_CStyleCastExpr
    )
}

/// Whether each of a `ForStmt`'s init/condition/increment clauses is
/// actually written in the source, resolved by tokenizing the statement's
/// own extent rather than guessing from `clang_visitChildren`'s child count
/// (which silently skips an absent clause with no positional marker —
/// `lower_stmt`'s own `CXCursor_ForStmt` branch doc comment has the real
/// trigger). `None` when the token shape isn't the plain `for ( ... ; ... ;
/// ... )` this looks for (defensive: an unexpected shape falls back to the
/// existing bailout rather than risking a wrong clause assignment).
///
/// Scans at paren-depth 1 (immediately inside the header's own `(...)`) for
/// exactly two `;` tokens and the matching closing `)` — a `;` can only
/// appear inside a *nested* statement in a for-header via a lambda body,
/// astronomically rare in practice and, if it ever occurs, only produces
/// this function returning `None` (wrong segment counts) rather than a
/// wrong result, since the caller cross-checks the derived clause count
/// against the cursor children's actual count before trusting either.
unsafe fn for_stmt_clause_presence(cursor: clang_sys::CXCursor) -> Option<[bool; 3]> {
    let translation_unit = unsafe { clang_sys::clang_Cursor_getTranslationUnit(cursor) };
    let extent = unsafe { clang_sys::clang_getCursorExtent(cursor) };
    let mut tokens: *mut clang_sys::CXToken = std::ptr::null_mut();
    let mut token_count: c_uint = 0;
    unsafe {
        clang_sys::clang_tokenize(translation_unit, extent, &mut tokens, &mut token_count);
    }
    if tokens.is_null() {
        return None;
    }
    let spellings: Vec<String> = (0..token_count)
        .map(|index| {
            let token = unsafe { *tokens.add(index as usize) };
            unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTokenSpelling(
                    translation_unit,
                    token,
                ))
            }
        })
        .collect();
    unsafe {
        clang_sys::clang_disposeTokens(translation_unit, tokens, token_count);
    }

    if spellings.first().map(String::as_str) != Some("for")
        || spellings.get(1).map(String::as_str) != Some("(")
    {
        return None;
    }
    let mut depth = 1i32;
    let mut semicolon_indices: Vec<usize> = Vec::new();
    let mut close_paren_index: Option<usize> = None;
    for (index, spelling) in spellings.iter().enumerate().skip(2) {
        match spelling.as_str() {
            "(" => depth += 1,
            ")" => {
                depth -= 1;
                if depth == 0 {
                    close_paren_index = Some(index);
                    break;
                }
            }
            ";" if depth == 1 => semicolon_indices.push(index),
            _ => {}
        }
    }
    let [semi1, semi2] = semicolon_indices.as_slice() else {
        return None;
    };
    let close_paren = close_paren_index?;
    if !(*semi1 < *semi2 && *semi2 < close_paren) {
        return None;
    }
    Some([*semi1 > 2, semi2 - semi1 > 1, close_paren - semi2 > 1])
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

    if kind == clang_sys::CXCursor_DoStmt {
        let children = unsafe { collect_children(cursor) };
        let [body_cursor, condition_cursor] = children.as_slice() else {
            return ir::Stmt::Unsupported {
                reason: format!(
                    "DoStmt had {} children, expected body+condition",
                    children.len()
                ),
                origin,
            };
        };
        return ir::Stmt::DoWhile {
            body: unsafe { lower_branch(*body_cursor, project_root) },
            condition: unsafe { lower_expr(*condition_cursor, project_root) },
            origin,
        };
    }

    if kind == clang_sys::CXCursor_ForStmt {
        if let Some(stmt) = unsafe { lower_iterator_for_loop(cursor, project_root, &origin) } {
            return stmt;
        }
        let children = unsafe { collect_children(cursor) };
        if let [init_cursor, condition_cursor, increment_cursor, body_cursor] = children.as_slice()
        {
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
        // Fewer than 4 children means at least one of init/condition/
        // increment was omitted in the source (`for (;;)`, `for (i = 0;;
        // i++)`, ...) — `clang_visitChildren` simply skips an absent
        // clause, with no positional marker left behind to say *which*
        // one, ambiguous from cursor kinds alone (real trigger: "ForStmt
        // had 3/1 children", 28+20 occurrences in the 2026-08-20
        // diagnosis). `for_stmt_clause_presence` resolves the ambiguity by
        // tokenizing the statement's own source text instead.
        if let Some([has_init, has_condition, has_increment]) =
            unsafe { for_stmt_clause_presence(cursor) }
        {
            let expected_len = [has_init, has_condition, has_increment]
                .iter()
                .filter(|present| **present)
                .count()
                + 1;
            if children.len() == expected_len {
                let mut remaining = children.iter();
                let init =
                    has_init.then(|| remaining.next().expect("counted by expected_len above"));
                let condition =
                    has_condition.then(|| remaining.next().expect("counted by expected_len above"));
                let increment =
                    has_increment.then(|| remaining.next().expect("counted by expected_len above"));
                let body_cursor = remaining
                    .next()
                    .expect("counted by expected_len above, body is always last");
                return ir::Stmt::For {
                    init: init.map(|cursor| Box::new(unsafe { lower_stmt(*cursor, project_root) })),
                    condition: condition.map(|cursor| unsafe { lower_expr(*cursor, project_root) }),
                    increment: increment
                        .map(|cursor| Box::new(unsafe { lower_stmt(*cursor, project_root) })),
                    body: unsafe { lower_branch(*body_cursor, project_root) },
                    origin,
                };
            }
        }
        return ir::Stmt::Unsupported {
            reason: format!(
                "ForStmt had {} children, expected init+condition+increment+body \
                 (a for-loop missing one of these clauses isn't supported yet)",
                children.len()
            ),
            origin,
        };
    }

    if kind == clang_sys::CXCursor_CXXForRangeStmt {
        // Libclang exposes precisely the source-level trio here: the range
        // binding declaration, the iterable expression and the body
        // (`[VarDecl, DeclRefExpr, CompoundStmt]` for `for (int x : xs)`).
        // It does not surface Clang's compiler-synthesized begin/end locals.
        let children = unsafe { collect_children(cursor) };
        let [binding_cursor, iterable_cursor, body_cursor] = children.as_slice() else {
            return ir::Stmt::Unsupported {
                reason: format!(
                    "CXXForRangeStmt had {} children, expected binding+iterable+body",
                    children.len()
                ),
                origin,
            };
        };
        if unsafe { clang_sys::clang_getCursorKind(*binding_cursor) } != clang_sys::CXCursor_VarDecl
        {
            return ir::Stmt::Unsupported {
                reason: "CXXForRangeStmt first child was not a range binding declaration"
                    .to_owned(),
                origin,
            };
        }
        let binding_type = unsafe { clang_sys::clang_getCursorType(*binding_cursor) };
        let is_mutable_reference = binding_type.kind == clang_sys::CXType_LValueReference
            && unsafe {
                clang_sys::clang_isConstQualifiedType(clang_sys::clang_getPointeeType(binding_type))
                    == 0
            };
        let iterable_type = lower_type(unsafe { clang_sys::clang_getCursorType(*iterable_cursor) });
        if is_mutable_reference && !matches!(iterable_type, ir::Type::List(_)) {
            return ir::Stmt::Unsupported {
                reason: "mutable range-for reference needs a list write-through adapter".to_owned(),
                origin,
            };
        }
        let is_final = unsafe {
            clang_sys::clang_isConstQualifiedType(binding_type) != 0
                || (binding_type.kind == clang_sys::CXType_LValueReference
                    && clang_sys::clang_isConstQualifiedType(clang_sys::clang_getPointeeType(
                        binding_type,
                    )) != 0)
        };
        let iterable_expr = unsafe { lower_expr(*iterable_cursor, project_root) };
        // F15/tarefa 15.5: Dart's `for`-`in` requires an `Iterable` — neither
        // `String` nor `Map` is one, so a straight-through `for (auto c :
        // str)`/`for (auto &kv : mapa)` fails `for_in_of_invalid_type`. Both
        // get an adapter getter on the iterable itself instead: `char`
        // always lowers to `Type::Int` (`lower_type`'s own doc comment), so
        // a `String`'s binding is always an int reading each UTF-16 code
        // unit (`.codeUnits`); a `Map<K, V>`'s `.entries` gives
        // `Iterable<MapEntry<K, V>>` — `emit::dart`'s `Stmt::ForEach` arm
        // recognizes this exact shape (a `Type::Pair` binding fed by a
        // `.entries` access) and prints the binding as Dart's own
        // `MapEntry<K, V>`, whose `first`/`second` extension (the shared
        // support file) lets the body's own `kv.first`/`kv.second`
        // (`std::pair`'s member names) survive unchanged.
        let iterable = match &iterable_type {
            ir::Type::Str => ir::Expr::FieldAccess {
                target: Box::new(iterable_expr),
                field: "codeUnits".to_owned(),
                ty: ir::Type::List(Box::new(ir::Type::Int)),
                origin: origin.clone(),
            },
            ir::Type::Map(key_ty, value_ty) => ir::Expr::FieldAccess {
                target: Box::new(iterable_expr),
                field: "entries".to_owned(),
                ty: ir::Type::List(Box::new(ir::Type::Pair(key_ty.clone(), value_ty.clone()))),
                origin: origin.clone(),
            },
            _ => iterable_expr,
        };
        return ir::Stmt::ForEach {
            name: unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(
                    *binding_cursor,
                ))
            },
            ty: lower_type(binding_type),
            is_final,
            write_back: is_mutable_reference,
            iterable,
            body: unsafe { lower_branch(*body_cursor, project_root) },
            origin,
        };
    }

    if kind == clang_sys::CXCursor_SwitchStmt {
        return unsafe { lower_switch_stmt(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_BreakStmt {
        return ir::Stmt::Break { origin };
    }

    if kind == clang_sys::CXCursor_ContinueStmt {
        return ir::Stmt::Continue { origin };
    }

    if kind == clang_sys::CXCursor_BinaryOperator
        && unsafe { clang_sys::clang_getCursorBinaryOperatorKind(cursor) }
            == clang_sys::CXBinaryOperator_Assign
    {
        return unsafe { lower_assign_stmt(cursor, project_root, origin) };
    }

    // `m_numerator /= gcdVal;` (E13's `Fraction::Reduce`) — a
    // `CompoundAssignOperator`, a different cursor kind from plain `=`
    // (confirmed via `clang -Xclang -ast-dump`, not conflated with it):
    // desugars to a plain assignment of a `Binary` expression, the same
    // "compiles and is right" shape C++ itself defines `x op= y` as
    // (`x = x op y`), evaluating the target only once (E13's own targets
    // are always a simple field or local, never anything with a
    // side-effecting receiver, so lowering the target expression twice here
    // never double-evaluates one).
    if kind == clang_sys::CXCursor_CompoundAssignOperator {
        return unsafe { lower_compound_assign_stmt(cursor, project_root, origin) };
    }

    // `throw value;` (E12) — a `CXXThrowExpr` is syntactically an
    // expression (confirmed via `clang -Xclang -ast-dump`: it can appear
    // anywhere an expression can in C++), but every use in this corpus is
    // as its own statement, so it gets a dedicated `Stmt::Throw` rather
    // than folding into `is_known_expression_kind`/`ExprStmt` — `ir::Expr`
    // has no "throw" variant to hold one either.
    if kind == clang_sys::CXCursor_CXXThrowExpr {
        let Some(value_cursor) = unsafe { collect_children(cursor) }.into_iter().next() else {
            return ir::Stmt::Unsupported {
                reason: "throw expression had no thrown value (rethrow, `throw;`, isn't \
                          supported yet)"
                    .to_owned(),
                origin,
            };
        };
        return ir::Stmt::Throw {
            value: unsafe { lower_expr(value_cursor, project_root) },
            origin,
        };
    }

    // `try { ... } catch (T name) { ... }` (E12) — scoped to exactly one
    // `CXXCatchStmt` child; multiple `catch` clauses or a catch-all
    // (`catch (...)`, no `VarDecl` child) aren't lowered yet, since no
    // fixture forces either.
    if kind == clang_sys::CXCursor_CXXTryStmt {
        let children = unsafe { collect_children(cursor) };
        let [try_cursor, catch_cursor] = children.as_slice() else {
            return ir::Stmt::Unsupported {
                reason: format!(
                    "CXXTryStmt had {} children, expected exactly 2 (try body + one catch \
                     clause — multiple catches/catch-all aren't supported yet)",
                    children.len()
                ),
                origin,
            };
        };
        if unsafe { clang_sys::clang_getCursorKind(*catch_cursor) }
            != clang_sys::CXCursor_CXXCatchStmt
        {
            return ir::Stmt::Unsupported {
                reason: "CXXTryStmt's second child was not the expected CXXCatchStmt".to_owned(),
                origin,
            };
        }
        let catch_children = unsafe { collect_children(*catch_cursor) };
        let [catch_var_cursor, catch_body_cursor] = catch_children.as_slice() else {
            return ir::Stmt::Unsupported {
                reason: format!(
                    "CXXCatchStmt had {} children, expected 2 (catch-all `catch (...)`, with \
                     no typed variable, isn't supported yet)",
                    catch_children.len()
                ),
                origin,
            };
        };
        return ir::Stmt::TryCatch {
            try_body: unsafe { lower_branch(*try_cursor, project_root) },
            catch_type: lower_type(unsafe { clang_sys::clang_getCursorType(*catch_var_cursor) }),
            catch_var: unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(
                    *catch_var_cursor,
                ))
            },
            catch_body: unsafe { lower_branch(*catch_body_cursor, project_root) },
            origin,
        };
    }

    // `ss << a << b;` (round 19) — a `std::stringstream` accumulation used
    // as its own statement. Checked directly on `cursor`, not through
    // `method_call_cursor_under_wrappers`: the chain's outermost
    // `operator<<` can resolve to a free function (a literal insertion,
    // same as the `std::cout` chain's own note on this), not only a
    // `CXXMethod`, and `lower_stringstream_insertion_stmt` already does its
    // own cursor-kind/receiver-chain validation.
    if let Some(statement) =
        unsafe { lower_stringstream_insertion_stmt(cursor, project_root, &origin) }
    {
        return statement;
    }

    // An overloaded assignment is represented as a CallExpr, not the
    // BinaryOperator shape handled above. For immutable Dart-backed values
    // such as `String`, C++ copy assignment has the same value semantics as
    // a Dart assignment; normalize its operator-call receiver into a real
    // assignment statement before the generic expression path can turn it
    // into an opaque method call.
    if let Some(call_cursor) = unsafe { method_call_cursor_under_wrappers(cursor) } {
        if let Some(statement) =
            unsafe { lower_stdlib_mutating_string_stmt(call_cursor, project_root, &origin) }
        {
            return statement;
        }
        if let Some(statement) =
            unsafe { lower_stdlib_mutating_sequence_stmt(call_cursor, project_root, &origin) }
        {
            return statement;
        }
    }

    if let Some(assignment_cursor) = unsafe { assignment_operator_call_cursor(cursor) } {
        if let Some(assign) =
            unsafe { lower_stdlib_assignment_stmt(assignment_cursor, project_root, &origin) }
        {
            return assign;
        }
        if let Some(assign) = unsafe {
            lower_defaulted_record_assignment_stmt(assignment_cursor, project_root, &origin)
        } {
            return assign;
        }
    }

    // `vrv::Fraction::Reduce(numerador, denominador);` (E13) — a bare call
    // to a function/method `apply_out_param_bridge` rewrote to return a
    // Dart record instead of `void`: the statement itself needs to become
    // a destructuring assignment (`(numerador, denominador) = ...;`), not a
    // plain `ExprStmt` that discards the record. Checked before the
    // `is_known_expression_kind` fallback below, which would otherwise
    // treat this exactly like any other bare call. Unwrapped through
    // `unwrap_transparent_value_cursor` first (F8/tarefa 10, real trigger
    // `Alignment::GetLeftRight`'s own trailing default argument, `const
    // std::vector<ClassId> &excludes = {}`): a bare-statement call that
    // *omits* a trailing default argument needing non-trivial destruction —
    // Verovio's own default-constructed `std::vector` — sits inside an
    // `ExprWithCleanups` wrapper (libclang exposes it as
    // `CXCursor_UnexposedExpr`, the same sugar `is_transparent_wrapper`
    // already unwraps everywhere else), so the bare `kind ==
    // CXCursor_CallExpr` check below never matched at all and this whole
    // out-param bridge was silently skipped — the same shape
    // `lower_if_with_out_param_call`'s own condition cursor already had to
    // unwrap for exactly this reason.
    let call_expr_cursor = unsafe { unwrap_transparent_value_cursor(cursor) };
    if unsafe { clang_sys::clang_getCursorKind(call_expr_cursor) } == clang_sys::CXCursor_CallExpr {
        let referenced = unsafe { clang_sys::clang_getCursorReferenced(call_expr_cursor) };
        if unsafe { clang_sys::clang_Cursor_isNull(referenced) } == 0 {
            let out_indices = unsafe { call_out_param_arg_indices(referenced) };
            if !out_indices.is_empty() {
                let target_cursors: Option<Vec<clang_sys::CXCursor>> = out_indices
                    .iter()
                    .map(|&index| unsafe {
                        out_arg_target_cursor(referenced, call_expr_cursor, index)
                    })
                    .collect();
                return match target_cursors {
                    Some(target_cursors) => {
                        let out_param_targets: Vec<ir::Expr> = target_cursors
                            .into_iter()
                            .map(|target_cursor| unsafe { lower_expr(target_cursor, project_root) })
                            .collect();
                        // Same reasoning as `lower_if_with_out_param_call`'s
                        // own check: an out-param target that didn't lower
                        // to a plain assignable lvalue can't sit in
                        // `Stmt::TupleAssign`'s destructuring-pattern
                        // position below (Dart doesn't parse a bailout
                        // helper call there at all, let alone assign to
                        // it) — the whole statement has to become an
                        // honest bailout instead.
                        if let Some(reason) = out_param_targets
                            .iter()
                            .find_map(unassignable_target_reason)
                        {
                            return ir::Stmt::Unsupported { reason, origin };
                        }
                        let mut value = unsafe { lower_expr(call_expr_cursor, project_root) };
                        // The pointer form's raw C++ argument is `&a`, not a
                        // Dart-representable value on its own — `lower_expr`
                        // above just lowered it generically (an
                        // `address-of`, `Unsupported` for a bare scalar
                        // pointee). The bridged Dart *call*, though, takes
                        // `a` itself as a plain input parameter (mirroring
                        // exactly how the reference form already calls
                        // `Reduce(numerador, denominador)` with no `&` at
                        // all) — so each out-arg slot in the already-lowered
                        // `Expr::Call`'s own argument list is overwritten
                        // with the same target expression `out_param_targets`
                        // already resolved, rather than trusting the generic
                        // address-of lowering neither needs nor can produce
                        // here.
                        if let ir::Expr::Call { args, .. } = &mut value {
                            for (&index, target) in out_indices.iter().zip(&out_param_targets) {
                                if let Some(arg) = args.get_mut(index) {
                                    *arg = target.clone();
                                }
                            }
                        }
                        // A non-`void` bridged callee's tuple has the
                        // original return value as its own leading slot
                        // (round 20) — a bare-statement call
                        // (`ParseDragAction(...);`) discards it exactly
                        // the way C++ itself allows discarding any
                        // return value, so the destructuring target for
                        // that slot is a wildcard, not a real assignment.
                        // Prepended only *after* the arg-patching above,
                        // which indexes by out-param position and would
                        // misalign against this extra leading slot.
                        let mut targets = out_param_targets;
                        if let Some(leading_ty) = unsafe {
                            out_param_bridge_leading_return_type(referenced, &out_indices)
                        } {
                            targets.insert(
                                0,
                                ir::Expr::Ref {
                                    name: "_".to_owned(),
                                    ty: leading_ty,
                                    origin: origin.clone(),
                                },
                            );
                        }
                        ir::Stmt::TupleAssign {
                            targets,
                            value,
                            origin,
                        }
                    }
                    // A pointer-shaped out-arg that isn't a plain `&lvalue`
                    // (`nullptr`, opting out of that output; a temporary;
                    // any other shape) has no Dart target to assign into.
                    // An honest statement-level bailout here, not a silent
                    // fall-through to `is_known_expression_kind` below: that
                    // path would lower this same cursor as a bare
                    // `ExprStmt`, evaluating the call and *discarding* its
                    // return value — which is exactly the out-param tuple
                    // this call's callee was rewritten to return. Discarding
                    // it would silently drop whatever the call was
                    // called for, the "compiles and is wrong" failure this
                    // module's own bailout discipline exists to prevent.
                    None => ir::Stmt::Unsupported {
                        reason: "call to an out-param-bridged function had an argument this \
                                 module couldn't resolve back to an assignable target"
                            .to_owned(),
                        origin,
                    },
                };
            }
        }
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

/// Finds a resolved method call under compiler-only statement wrappers. This
/// keeps statement-level adapters for immutable Dart values independent from
/// the expression lowering that would otherwise turn their C++ mutation into
/// a call to a nonexistent Dart method.
unsafe fn method_call_cursor_under_wrappers(
    cursor: clang_sys::CXCursor,
) -> Option<clang_sys::CXCursor> {
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } == 0
        && unsafe { clang_sys::clang_getCursorKind(referenced) } == clang_sys::CXCursor_CXXMethod
    {
        return Some(cursor);
    }
    let children: Vec<clang_sys::CXCursor> = unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
            )
        })
        .collect();
    let calls: Vec<clang_sys::CXCursor> = children
        .into_iter()
        .filter_map(|child| unsafe { method_call_cursor_under_wrappers(child) })
        .collect();
    let [call] = calls.as_slice() else {
        return None;
    };
    Some(*call)
}

/// Converts the mutating basic_string operations whose effect is exactly an
/// immutable Dart String reassignment. More involved byte-oriented edits stay
/// explicit bailouts until their bounds and encoding semantics have adapters.
unsafe fn lower_stdlib_mutating_string_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Stmt> {
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    if unsafe { stdlib_template_name(owner) }.as_deref() != Some("basic_string") {
        return None;
    }
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    let target = unsafe { stdlib_method_receiver(cursor, project_root, origin) }.ok()?;
    let args = unsafe { lower_call_arguments(cursor, project_root) }?;
    let value = match callee_name.as_str() {
        "clear" if args.is_empty() => ir::Expr::StringLiteral {
            value: String::new(),
            origin: origin.clone(),
        },
        "append" if args.len() == 1 => ir::Expr::Binary {
            op: ir::BinaryOp::Add,
            lhs: Box::new(target.clone()),
            rhs: Box::new(args.into_iter().next().expect("one append argument")),
            ty: ir::Type::Str,
            origin: origin.clone(),
        },
        // `std::basic_string::push_back(char)` — real triggers found by
        // grepping the extracted Verovio source directly
        // (`toolkit.cpp`'s `option_str.push_back(option->GetShortOption())`,
        // `iopae.cpp`'s `paeStr.push_back(token.m_char)`). The argument is a
        // single `char`, which this IR already represents as `Type::Int`
        // (a code unit, per the `CharacterLiteral` lowering just below) —
        // `String.fromCharCode` is Dart's own precise inverse, so the same
        // reassignment shape `append` uses above just needs its rhs wrapped
        // through that one static call first.
        "push_back" if args.len() == 1 => ir::Expr::Binary {
            op: ir::BinaryOp::Add,
            lhs: Box::new(target.clone()),
            rhs: Box::new(ir::Expr::Call {
                base_qualifier: None,
                target: None,
                callee_usr: String::new(),
                callee_name: "String.fromCharCode".to_owned(),
                args: vec![args.into_iter().next().expect("one push_back argument")],
                ty: ir::Type::Str,
                origin: origin.clone(),
            }),
            ty: ir::Type::Str,
            origin: origin.clone(),
        },
        _ => return None,
    };
    if let Some(reason) = unassignable_target_reason(&target) {
        return Some(ir::Stmt::Unsupported {
            reason,
            origin: origin.clone(),
        });
    }
    Some(ir::Stmt::ExprAssign {
        target,
        value,
        origin: origin.clone(),
    })
}

/// Converts `std::vector`/`std::list`/`std::deque::resize` into an explicit
/// shrink/grow `if`/`else`. Dart's `List.length` setter only shrinks safely
/// on its own — growing it pads with `null`, which throws at runtime for a
/// non-nullable element type — so growth instead pads through
/// `list.addAll(List.filled(...))` with the element type's own default
/// value (`default_scalar_value`, the same helper `MapIndexOrInsert` already
/// uses), or the caller's explicit fill for the two-argument overload.
unsafe fn lower_stdlib_mutating_sequence_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Stmt> {
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let template_name = unsafe { stdlib_template_name(owner) }?;
    if !matches!(template_name.as_str(), "vector" | "list" | "deque") {
        return None;
    }
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    if callee_name != "resize" {
        return None;
    }
    let target = unsafe { stdlib_method_receiver(cursor, project_root, origin) }.ok()?;
    let args = unsafe { lower_call_arguments(cursor, project_root) }?;
    let element_ty = unsafe { stdlib_sequence_element_type(owner, &template_name) };
    let (new_length, fill) = match args.as_slice() {
        [new_length] => (
            new_length.clone(),
            // `default_scalar_value` alone falls straight to
            // `Unsupported` for *any* `Type::Record` element — even one
            // this bridge could trivially zero-construct field by field
            // (real trigger: `humlib.h`'s `MyCoord { int x; int y; }`,
            // used as `std::vector<MyCoord>`, round 22). `default_field_
            // value` tries the recursive `default_record_construct_at_
            // depth` construction first, falling back to
            // `default_scalar_value` (still honestly `Unsupported`) only
            // when that itself can't resolve — the same helper a nested
            // `Record`-typed *field*'s own default already goes through,
            // just reached here from a container's element type instead
            // of a field type. Needs the element's own `CXType`, not just
            // its already-lowered `ir::Type` (`element_ty`): recomputed
            // directly from `owner`'s first template argument, the exact
            // same derivation `stdlib_sequence_element_type` itself uses
            // internally, since only the `ir::Type` half of that reaches
            // this call site today.
            unsafe {
                default_field_value(
                    &element_ty,
                    clang_sys::clang_Type_getTemplateArgumentAsType(
                        clang_sys::clang_getCursorType(owner),
                        0,
                    ),
                    origin,
                    0,
                )
            },
        ),
        [new_length, fill] => (new_length.clone(), fill.clone()),
        _ => {
            return Some(ir::Stmt::Unsupported {
                reason: format!(
                    "std::{template_name}::resize had {} arguments, expected 1 or 2",
                    args.len()
                ),
                origin: origin.clone(),
            });
        }
    };
    let current_length = ir::Expr::FieldAccess {
        target: Box::new(target.clone()),
        field: "length".to_owned(),
        ty: ir::Type::Int,
        origin: origin.clone(),
    };
    Some(ir::Stmt::If {
        condition: ir::Expr::Binary {
            op: ir::BinaryOp::Lt,
            lhs: Box::new(new_length.clone()),
            rhs: Box::new(current_length.clone()),
            ty: ir::Type::Bool,
            origin: origin.clone(),
        },
        then_branch: vec![ir::Stmt::FieldAssign {
            target: target.clone(),
            field: "length".to_owned(),
            value: new_length.clone(),
            origin: origin.clone(),
        }],
        else_branch: vec![ir::Stmt::ExprStmt {
            expr: ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr: String::new(),
                callee_name: "addAll".to_owned(),
                args: vec![ir::Expr::Call {
                    base_qualifier: None,
                    target: None,
                    callee_usr: String::new(),
                    callee_name: "List.filled".to_owned(),
                    args: vec![
                        ir::Expr::Binary {
                            op: ir::BinaryOp::Sub,
                            lhs: Box::new(new_length),
                            rhs: Box::new(current_length),
                            ty: ir::Type::Int,
                            origin: origin.clone(),
                        },
                        fill,
                    ],
                    ty: ir::Type::List(Box::new(element_ty)),
                    origin: origin.clone(),
                }],
                ty: ir::Type::Void,
                origin: origin.clone(),
            },
            origin: origin.clone(),
        }],
        origin: origin.clone(),
    })
}

/// Finds an overloaded assignment underneath compiler-only expression
/// wrappers. libclang can place ExprWithCleanups and implicit conversion
/// cursors around a statement such as an optional assignment; only the inner
/// call carries the receiver and right-hand-side arguments needed by the
/// explicit Dart assignment adapter.
unsafe fn assignment_operator_call_cursor(
    cursor: clang_sys::CXCursor,
) -> Option<clang_sys::CXCursor> {
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } == 0 {
        let callee_name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced))
        };
        if callee_name == "operator=" || callee_name == "operator+=" {
            return Some(cursor);
        }
    }

    let children: Vec<clang_sys::CXCursor> = unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
            )
        })
        .collect();
    let [child] = children.as_slice() else {
        return None;
    };
    unsafe { assignment_operator_call_cursor(*child) }
}

/// Lowers the stdlib `operator=` calls whose Dart representation has the same
/// value-copy semantics. Mutable collections deliberately stay out of this
/// helper: `List` assignment aliases storage whereas `std::vector` assignment
/// copies elements, so it needs its own `List.of` adapter instead of a silent
/// direct assignment.
///
/// Handles *both* implicitly-defaulted special members C++ generates for a
/// class with value semantics: copy assignment (`clang_CXXMethod_
/// isCopyAssignmentOperator`) and move assignment (`clang_CXXMethod_
/// isMoveAssignmentOperator`) — never a user-written body for either (that's
/// `lower_method_call`'s `assignFrom` bridge, F5's Caso 2), only the
/// compiler-synthesized member-for-member copy. Overload resolution already
/// did the hard part for us: it only ever selects the move overload for an
/// rvalue right-hand side (F5's Caso 1's "temporário recém-construído"), so
/// which overload `referenced` names *is* the signal for whether Dart's
/// aliasing assignment is sound here — confirmed empirically (not assumed):
/// `p = Ponto(1, 2);` and `this->m_point = Ponto(1, 2);` both resolve to the
/// implicit `Ponto &operator=(Ponto&&)`, never the copy overload, even though
/// neither source line ever writes "move".
unsafe fn lower_defaulted_record_assignment_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Stmt> {
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0
        || unsafe { clang_sys::clang_getCursorKind(referenced) } != clang_sys::CXCursor_CXXMethod
        || unsafe { clang_sys::clang_CXXMethod_isDefaulted(referenced) } == 0
        || unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) } != 2
    {
        return None;
    }
    let is_copy = unsafe { clang_sys::clang_CXXMethod_isCopyAssignmentOperator(referenced) } != 0;
    let is_move = unsafe { clang_sys::clang_CXXMethod_isMoveAssignmentOperator(referenced) } != 0;
    if !(is_copy || is_move) {
        return None;
    }

    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let ir::Type::Record { usr, name } =
        lower_type(unsafe { clang_sys::clang_getCursorType(owner) })
    else {
        return None;
    };
    let target_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
    let source_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 1) };
    let target = unsafe { lower_expr(target_cursor, project_root) };
    // `*(field = new T()) = value;` — the target dereference's own operand
    // (`field = new T()`) can now be a representable `Expr::Assign` (an
    // assignment used as an expression) instead of a bailout, but
    // `Expr::Assign` is a valid Dart *value*, never a valid Dart assignment
    // *target* (`(x = y) = z;` doesn't compile) — `emit::dart`'s own
    // `Stmt::ExprAssign` unwrap only fires for the shapes Dart actually
    // accepts there (`unassignable_target_reason`), so anything else has to
    // stay an honest bailout here rather than reach emission and produce
    // invalid Dart. Also catches `target` itself already being an
    // `Expr::Unsupported`/`UnsupportedTyped` bailout (e.g. the dereference's
    // own operand failed to lower at all) — that shape isn't wrapped in
    // `Convert`, so the narrower check this replaced only ever saw it once
    // it *was* wrapped.
    if let Some(reason) = unassignable_target_reason(&target) {
        return Some(ir::Stmt::Unsupported {
            reason,
            origin: origin.clone(),
        });
    }
    // Move assignment only fires for an rvalue right-hand side, but an
    // rvalue isn't automatically an *unaliased* one: `a = std::move(b);`
    // resolves to the same move overload a genuine temporary does, yet `b`
    // stays a live, named object a reader can still reach afterward — C++
    // leaves it in a valid, independent (if unspecified) state, which a
    // plain Dart `a = b;` would violate by aliasing the two forever after.
    // `unwrap_transparent_value_cursor` peels the `MaterializeTemporaryExpr`/
    // `ImplicitCastExpr` sugar every rvalue argument is wrapped in (both
    // `libclang`'s `CXCursor_UnexposedExpr` catch-all); what's left is either
    // a `DeclRefExpr`/`MemberRefExpr` naming the live object being moved
    // from, or a real construction/call with no name behind it at all — the
    // exact "temporário recém-construído" the task's Caso 1 describes.
    let source_root_kind =
        unsafe { clang_sys::clang_getCursorKind(unwrap_transparent_value_cursor(source_cursor)) };
    let source_is_provably_fresh = !matches!(
        source_root_kind,
        clang_sys::CXCursor_DeclRefExpr | clang_sys::CXCursor_MemberRefExpr
    );
    if is_move && source_is_provably_fresh {
        return Some(ir::Stmt::ExprAssign {
            target,
            value: unsafe { lower_expr(source_cursor, project_root) },
            origin: origin.clone(),
        });
    }
    let source = unsafe { lower_expr(source_cursor, project_root) };
    let fields = unsafe { record_fields_of(owner) }
        .into_iter()
        .map(|field| {
            let value = ir::Expr::FieldAccess {
                target: Box::new(source.clone()),
                field: field.name.clone(),
                ty: field.ty.clone(),
                origin: origin.clone(),
            };
            (field.name, clone_value_expr(value, &field.ty, origin))
        })
        .collect();

    Some(ir::Stmt::ExprAssign {
        target,
        value: ir::Expr::RecordConstruct {
            type_usr: usr,
            type_name: name,
            fields,
            origin: origin.clone(),
        },
        origin: origin.clone(),
    })
}

fn clone_value_expr(value: ir::Expr, ty: &ir::Type, origin: &ir::Origin) -> ir::Expr {
    let callee_name = match ty {
        ir::Type::List(_) => Some("List.of"),
        ir::Type::Set(_) => Some("Set.of"),
        ir::Type::Map(_, _) => Some("Map.of"),
        ir::Type::Bytes => Some("Uint8List.fromList"),
        _ => None,
    };
    match callee_name {
        Some(callee_name) => ir::Expr::Call {
            base_qualifier: None,
            target: None,
            callee_usr: String::new(),
            callee_name: callee_name.to_owned(),
            args: vec![value],
            ty: ty.clone(),
            origin: origin.clone(),
        },
        None => value,
    }
}

unsafe fn lower_stdlib_assignment_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Stmt> {
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    if callee_name != "operator=" && callee_name != "operator+=" {
        return None;
    }
    if unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) } != 2 {
        return None;
    }
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let target_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
    let template_name = unsafe { stdlib_template_name(owner) }
        .or_else(|| unsafe {
            stdlib_template_name_of_type(clang_sys::clang_getCursorType(target_cursor))
        })
        .or_else(|| unsafe {
            let is_system_method = clang_sys::clang_Location_isInSystemHeader(
                clang_sys::clang_getCursorLocation(referenced),
            ) != 0;
            let owner_name =
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(owner));
            (is_system_method && owner_name == "optional").then_some(owner_name)
        })?;
    let target = target_cursor;
    let value_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 1) };
    let target = unsafe { lower_expr(target, project_root) };
    let value = unsafe { lower_expr(value_cursor, project_root) };
    let value = match (template_name.as_str(), callee_name.as_str()) {
        ("basic_string", "operator=") => value,
        ("basic_string", "operator+=") => ir::Expr::Binary {
            op: ir::BinaryOp::Add,
            lhs: Box::new(target.clone()),
            rhs: Box::new(value),
            ty: ir::Type::Str,
            origin: origin.clone(),
        },
        ("optional", "operator=") => value,
        ("vector" | "list" | "deque", "operator=") => ir::Expr::Call {
            base_qualifier: None,
            target: None,
            callee_usr: String::new(),
            callee_name: "List.of".to_owned(),
            args: vec![value],
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(value_cursor) }),
            origin: origin.clone(),
        },
        _ => return None,
    };
    if let Some(reason) = unassignable_target_reason(&target) {
        return Some(ir::Stmt::Unsupported {
            reason,
            origin: origin.clone(),
        });
    }
    Some(ir::Stmt::ExprAssign {
        target,
        value,
        origin: origin.clone(),
    })
}

/// The 0-based positions, among a `CallExpr`'s own raw arguments, of every
/// argument passed to a bridged out-param (see `apply_out_param_bridge`) —
/// recomputed independently from `referenced` (the call's resolved
/// callee), the same "never disagree" discipline `out_param_indices`'s own
/// doc comment describes. Recognizes a call to a free function, a `static`
/// method, or an *ordinary* (non-`static`, non-operator-syntax) instance
/// method: `lower_call_arguments`'s own doc comment already establishes that
/// a free function's raw argument index lines up 1:1 with the callee's
/// declared parameter index (no receiver consuming argument 0), and
/// `lower_method_call`'s own `arg_skip` derivation establishes the exact
/// same 1:1 alignment for a plain `obj.method(args)` call — only an
/// operator-syntax call (`a == b`, `pred(a, b)` through `operator()`, ...)
/// has the receiver folded into argument 0 instead, which is why that shape
/// stays excluded (checked the same way `lower_method_call` itself
/// disambiguates it: the callee's raw spelling starts with `"operator"`).
/// F8/tarefa 10's real trigger (`docs/prompts/
/// 2026-08-21-10-parametros-de-saida-por-referencia.md`): `StaffAlignment::
/// GetLeftRight(int, int&, int&) const`, an ordinary instance method, called
/// as `topNote->GetAlignment()->GetLeftRight(staffN, minLeft, maxRight)` —
/// `apply_out_param_bridge` already rewrote its *declaration* to return a
/// tuple (it runs unconditionally from `lower_method`), but every call site
/// fell through to the unbridged call path, leaving the caller's own
/// out-param locals a `late` that's never assigned.
unsafe fn call_out_param_arg_indices(referenced: clang_sys::CXCursor) -> Vec<usize> {
    let referenced_kind = unsafe { clang_sys::clang_getCursorKind(referenced) };
    let is_free_function = referenced_kind == clang_sys::CXCursor_FunctionDecl;
    let is_ordinary_method = referenced_kind == clang_sys::CXCursor_CXXMethod && {
        let raw_name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced))
        };
        let is_static = unsafe { clang_sys::clang_CXXMethod_isStatic(referenced) } != 0;
        is_static || !raw_name.starts_with("operator")
    };
    if !is_free_function && !is_ordinary_method {
        return Vec::new();
    }
    let indices = unsafe { out_param_indices(referenced) };
    if indices.is_empty() {
        return indices;
    }
    // `apply_out_param_bridge` never rewrites a non-`void`-returning
    // function/method whose out-params are the *reference* form — only the
    // *pointer* form is eligible there (see its own doc comment). A callee
    // this shape (real trigger: `Verse::AdjustPosition(int &overlap, int
    // freeSpace, const Doc *doc)`) keeps its original, unbridged
    // declaration untouched, so a call site must never treat it as bridged
    // either — `out_param_indices_are_all_pointer_form`'s own doc comment
    // has the full story.
    let return_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(referenced) });
    if return_type != ir::Type::Void
        && !unsafe { out_param_indices_are_all_pointer_form(referenced, &indices) }
    {
        return Vec::new();
    }
    indices
}

/// Whether `referenced` (a callee already confirmed out-param-bridged —
/// `indices` non-empty) was bridged as *non*-`void` (round 20) — and if so,
/// the original return type that becomes the out-param tuple's own
/// leading slot. Mirrors `apply_out_param_bridge`'s own eligibility check
/// exactly (non-`void` *and* every out-param is the pointer form) so a
/// call site and its callee's definition can never disagree about whether
/// the leading slot exists — the same "never disagree" discipline
/// `out_param_indices`'s own doc comment already establishes for the
/// indices themselves.
unsafe fn out_param_bridge_leading_return_type(
    referenced: clang_sys::CXCursor,
    indices: &[usize],
) -> Option<ir::Type> {
    let return_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(referenced) });
    if return_type == ir::Type::Void {
        return None;
    }
    unsafe { out_param_indices_are_all_pointer_form(referenced, indices) }.then_some(return_type)
}

/// The cursor a bridged out-arg (position `index` in `call_cursor`'s raw
/// arguments) should be lowered as, to become one `Stmt::TupleAssign`
/// target — `None` when it can't be resolved to a plain assignable lvalue.
/// The reference form of an out-param binds implicitly at the call site
/// (`Reduce(numerador, denominador)`, no `&`): the raw argument cursor is
/// already the target. The pointer form needs `&numerador`/`&x` written
/// explicitly in the source — the argument cursor is a `UnaryOperator`
/// address-of wrapping the real target, which needs unwrapping first, and
/// `None` (not a guess) for anything that isn't exactly that shape (a
/// `nullptr` opt-out, a temporary, ...). `referenced`'s own declared
/// parameter type at `index`, not the *lowered* `ir::Param` (unavailable
/// here, and irrelevant: only the C++ reference-vs-pointer *shape* decides
/// which unwrapping applies).
unsafe fn out_arg_target_cursor(
    referenced: clang_sys::CXCursor,
    call_cursor: clang_sys::CXCursor,
    index: usize,
) -> Option<clang_sys::CXCursor> {
    let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, index as c_uint) };
    let param_cursor = unsafe { clang_sys::clang_Cursor_getArgument(referenced, index as c_uint) };
    let param_type = unsafe { clang_sys::clang_getCursorType(param_cursor) };
    if param_type.kind != clang_sys::CXType_Pointer {
        return Some(arg_cursor);
    }
    let unwrapped = unsafe { unwrap_transparent_value_cursor(arg_cursor) };
    if unsafe { clang_sys::clang_getCursorKind(unwrapped) } != clang_sys::CXCursor_UnaryOperator
        || unsafe { clang_sys::clang_getCursorUnaryOperatorKind(unwrapped) }
            != clang_sys::CXUnaryOperator_AddrOf
    {
        return None;
    }
    let children = unsafe { collect_children(unwrapped) };
    let [operand_cursor] = children.as_slice() else {
        return None;
    };
    Some(*operand_cursor)
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
    unsafe { lower_one_var_decl(*var_decl_cursor, project_root, origin) }
}

/// `int a, b;` (or `int a = 1, b = 2;`) — one `DeclStmt` cursor with more
/// than one `VarDecl` child, C++'s comma-separated multi-declarator form.
/// `lower_decl_stmt` only ever handles exactly one declarator (a for-loop's
/// `init` clause — `ir::Stmt::For.init` — is a single `Box<Stmt>`, with no
/// room for more than one); a `DeclStmt` reached as an ordinary block
/// statement (via `lower_stmt_into`, which already flattens a nested
/// `CompoundStmt` the same way) has no such limit, so each declarator
/// becomes its own `VarDecl` statement, in source order — the exact same
/// per-declarator lowering `lower_decl_stmt` already gives a single one.
/// `None` when `cursor` isn't a multi-declarator `DeclStmt` at all, so the
/// caller falls through to the ordinary single-statement path unchanged.
unsafe fn lower_multi_decl_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<Vec<ir::Stmt>> {
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_DeclStmt {
        return None;
    }
    let children = unsafe { collect_children(cursor) };
    if children.len() < 2 {
        return None;
    }
    Some(
        children
            .into_iter()
            // `struct { ... } s, *ps;` — an inline struct/enum/union/typedef
            // definition followed by one or more variable declarators using
            // it, a real C idiom (real trigger: the "DeclStmt's declarator
            // is not a VarDecl" family, 44 occurrences in the 2026-08-20
            // diagnosis). The type-declaration cursor is `DeclStmt`'s first
            // child alongside the real `VarDecl`s — not itself a runtime
            // action, so it's a type declaration to skip, not a statement
            // to bail out on (leaving it in would have produced a spurious
            // `Stmt::Unsupported` — an unconditional `throw` — spliced
            // between the real declarators, which is worse than dropping
            // it: this project's own `lower_record`/`lower_enum` passes
            // over the type declaration separately, wherever it needs to).
            .filter(|child| {
                !matches!(
                    unsafe { clang_sys::clang_getCursorKind(*child) },
                    clang_sys::CXCursor_StructDecl
                        | clang_sys::CXCursor_ClassDecl
                        | clang_sys::CXCursor_UnionDecl
                        | clang_sys::CXCursor_EnumDecl
                        | clang_sys::CXCursor_TypedefDecl
                )
            })
            .map(|var_decl_cursor| unsafe {
                lower_one_var_decl(var_decl_cursor, project_root, origin.clone())
            })
            .collect(),
    )
}

/// The per-declarator lowering both `lower_decl_stmt` (exactly one) and
/// `lower_multi_decl_stmt` (more than one) share.
unsafe fn lower_one_var_decl(
    var_decl_cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Stmt {
    if unsafe { clang_sys::clang_getCursorKind(var_decl_cursor) } != clang_sys::CXCursor_VarDecl {
        return ir::Stmt::Unsupported {
            reason: "DeclStmt's declarator is not a VarDecl".to_owned(),
            origin,
        };
    }
    let var_decl_cursor = &var_decl_cursor;

    // Item 9 of `docs/plans/diagnostico-verovio-6.2.0.md`
    // (`verovio_6_2_0_transpile_diagnosis`): a local variable legally named
    // after a Dart reserved word (`jsonxx.dart`'s own
    // `basic_istringstream is = ...;`) hits the same parse error a
    // reserved-word parameter/method name does — `dart_safe_identifier`
    // covers it the same way. Every reference to this local inside the
    // body resolves through the same function (`qualified_static_member_name`
    // → `dart_member_name`'s public branch), so the two can never disagree.
    let cx_type = unsafe { clang_sys::clang_getCursorType(*var_decl_cursor) };
    let ty = lower_type(cx_type);
    // F15/tarefa 15.8: `struct tm tm;` (real trigger: `zip_file.dart:1390`)
    // keeps `tm` the type and `tm` the local in separate C++ namespaces —
    // Dart has one shared namespace, so the local shadows the type the
    // moment both share a spelling, and this record's own synthesized
    // default-value constructor call (`tm(0, 0, ...)`, right below) would
    // resolve to the not-yet-initialized local instead of the type,
    // `referenced_before_declaration`. `qualified_static_member_name`
    // applies this identical rename to every later *read* of the same
    // local, so the two can't disagree either.
    let name = dart_safe_local_name(
        &dart_safe_identifier(&unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(*var_decl_cursor))
        }),
        &ty,
    );

    // A record-typed `VarDecl` always has at least one child that isn't a
    // real initializer: `libclang` emits a leading `TypeRef` (pointing at
    // the record type) purely for navigation, present even for a builtin
    // type but only *matched here* since `int`/`double`/`bool` locals never
    // hit this path with a spurious extra child in E01/E02's fixtures. A
    // record type named through a namespace (`vrv::Fraction a(1, 2);`, E13)
    // adds a second, sibling non-initializer child instead — `NamespaceRef`
    // (spelling `"vrv"`) — confirmed empirically (not by guessing from
    // `-ast-dump`, whose tree doesn't show this: it's a `libclang` cursor
    // quirk, not an AST node) with a real `Fraction a(1, 2);`, which without
    // this filter reported "VarDecl had 2 initializer-shaped children"
    // instead of finding the single real one, a `CallExpr` wrapping the
    // resolved constructor. `Ponto p;` with no written initializer *also*
    // gets a child: an implicit default-constructor `CallExpr` `libclang`
    // synthesizes — confirmed via `clang -Xclang -ast-dump` before writing
    // this, not guessed. A dependent standard-library alias such as
    // `std::vector<int>::size_type` adds a `TemplateRef` alongside its real
    // initializer for the same navigational purpose. `struct Point { ... }
    // p, *pp;` (real trigger: "VarDecl had 2 initializer-shaped children",
    // 44 occurrences in the 2026-08-20 diagnosis) adds the struct's own
    // `CXCursor_StructDecl` as a child of *each* declarator's own `VarDecl`
    // cursor too — confirmed empirically, a further libclang navigation
    // quirk beyond the `TypeRef`-shaped one `lower_multi_decl_stmt`'s own
    // doc comment already covers for the `DeclStmt`-level sibling. All
    // five need filtering out before "first remaining child, if any" is
    // the real initializer.
    let init_candidates: Vec<clang_sys::CXCursor> = unsafe { collect_children(*var_decl_cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
                    | clang_sys::CXCursor_StructDecl
                    | clang_sys::CXCursor_ClassDecl
                    | clang_sys::CXCursor_UnionDecl
                    | clang_sys::CXCursor_EnumDecl
            )
        })
        .filter(|child| !unsafe { is_default_construct_with_no_args(*child) })
        .filter(|child| !unsafe { is_default_construct_of_a_known_adapter_type(*child) })
        .collect();
    let init = match init_candidates.as_slice() {
        // `late Ponto p;` alone isn't enough (checked with real `dart
        // analyze`, not assumed): `late` defers *whole-object*
        // assignment, but `p.x = x;` right after needs `p` to already
        // *hold* an object to set a field on — "definitely unassigned
        // late local variable". C++'s `Ponto p;` default-constructs in
        // place, so a genuinely equivalent Dart local needs a real
        // (zero-valued) object from the start, not a deferred one.
        // `default_record_construct` only ever handles `Type::Record`
        // (field-by-field). A default-constructed *library-adapter* class
        // — `std::string s;`, and since round 19 `std::stringstream ss;`
        // — reaches this same "no written initializer, but a real
        // implicit default-constructor call was filtered out above" case
        // with a `ty` that isn't a `Record`, and `default_scalar_value`
        // already has the exact zero value each of these types
        // constructs to in real C++ (`""` for a string, `{}`/empty for a
        // collection). Never applied to a bare scalar (`Int`/`Double`/
        // `Bool`): those reach this arm only for a genuinely uninitialized
        // C++ local (`int a;`, no constructor involved at all), where
        // C++ itself makes no promise about the value — giving it a real
        // `0` would be an actual semantic invention, not a translation.
        [] => default_record_construct(&ty, cx_type, &origin).or_else(|| {
            matches!(
                ty,
                ir::Type::Str
                    | ir::Type::List(_)
                    | ir::Type::Set(_)
                    | ir::Type::Map(_, _)
                    | ir::Type::Bytes
            )
            .then(|| default_scalar_value(&ty, &origin))
        }),
        [only_child] => Some(unsafe { lower_expr(*only_child, project_root) }),
        _ => Some(ir::Expr::UnsupportedTyped {
            reason: format!(
                "VarDecl had {} initializer-shaped children, expected at most 1",
                init_candidates.len()
            ),
            ty: ty.clone(),
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
    unsafe { default_record_construct_at_depth(ty, cx_type, origin, 0) }
}

const DEFAULT_RECORD_CONSTRUCT_MAX_DEPTH: usize = 16;

unsafe fn default_record_construct_at_depth(
    ty: &ir::Type,
    cx_type: clang_sys::CXType,
    origin: &ir::Origin,
    depth: usize,
) -> Option<ir::Expr> {
    let ir::Type::Record { usr, name } = ty else {
        return None;
    };
    if depth >= DEFAULT_RECORD_CONSTRUCT_MAX_DEPTH {
        return None;
    }

    let decl = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
    let fields = unsafe { collect_children(decl) }
        .into_iter()
        .filter(|field| unsafe {
            clang_sys::clang_getCursorKind(*field) == clang_sys::CXCursor_FieldDecl
        })
        .map(|field_cursor| {
            let field_type = unsafe { clang_sys::clang_getCursorType(field_cursor) };
            let ty = lower_type(field_type);
            let value = unsafe { default_field_value(&ty, field_type, origin, depth + 1) };
            let name = unsafe { dart_member_name(field_cursor) };
            (name, value)
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
unsafe fn default_field_value(
    ty: &ir::Type,
    cx_type: clang_sys::CXType,
    origin: &ir::Origin,
    depth: usize,
) -> ir::Expr {
    if matches!(ty, ir::Type::Record { .. })
        && let Some(value) =
            unsafe { default_record_construct_at_depth(ty, cx_type, origin, depth) }
    {
        return value;
    }
    if let ir::Type::Enum { name, .. } = ty {
        let declaration = unsafe { clang_sys::clang_getTypeDeclaration(cx_type) };
        let first_variant =
            unsafe { collect_children(declaration) }
                .into_iter()
                .find(|child| unsafe {
                    clang_sys::clang_getCursorKind(*child) == clang_sys::CXCursor_EnumConstantDecl
                });
        if let Some(variant) = first_variant {
            let cpp_name = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(variant))
            };
            return ir::Expr::Ref {
                name: format!("{name}.{}", dart_enum_constant_name(&cpp_name)),
                ty: ty.clone(),
                origin: origin.clone(),
            };
        }
    }
    default_scalar_value(ty, origin)
}

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
        ir::Type::Str => ir::Expr::StringLiteral {
            value: String::new(),
            origin: origin.clone(),
        },
        ir::Type::List(_) => ir::Expr::Call {
            base_qualifier: None,
            target: None,
            callee_usr: String::new(),
            callee_name: "List.empty".to_owned(),
            args: Vec::new(),
            ty: ty.clone(),
            origin: origin.clone(),
        },
        ir::Type::Set(_) => ir::Expr::Call {
            base_qualifier: None,
            target: None,
            callee_usr: String::new(),
            callee_name: "Set.empty".to_owned(),
            args: Vec::new(),
            ty: ty.clone(),
            origin: origin.clone(),
        },
        ir::Type::Map(_, _) => ir::Expr::Call {
            base_qualifier: None,
            target: None,
            callee_usr: String::new(),
            callee_name: "Map".to_owned(),
            args: Vec::new(),
            ty: ty.clone(),
            origin: origin.clone(),
        },
        ir::Type::Bytes => ir::Expr::Call {
            base_qualifier: None,
            target: None,
            callee_usr: String::new(),
            callee_name: "Uint8List".to_owned(),
            args: vec![ir::Expr::IntLiteral {
                value: 0,
                origin: origin.clone(),
            }],
            ty: ty.clone(),
            origin: origin.clone(),
        },
        ir::Type::Nullable(_) => ir::Expr::NullLiteral {
            origin: origin.clone(),
        },
        ir::Type::Record { .. }
        | ir::Type::Enum { .. }
        | ir::Type::Pair(_, _)
        | ir::Type::ListCursor(_)
        | ir::Type::Callback { .. }
        | ir::Type::Tuple(_)
        | ir::Type::Void
        | ir::Type::Object
        | ir::Type::Unsupported(_) => ir::Expr::UnsupportedTyped {
            reason: "no default value available for this field's type yet".to_owned(),
            ty: ty.clone(),
            origin: origin.clone(),
        },
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
            qualified_static_member_name(clang_sys::clang_getCursorReferenced(*lhs_cursor))
        };
        return ir::Stmt::Assign {
            name,
            value,
            origin,
        };
    }

    if lhs_kind == clang_sys::CXCursor_MemberRefExpr {
        let field = unsafe { dart_member_name(clang_sys::clang_getCursorReferenced(*lhs_cursor)) };
        let target = unsafe { member_ref_receiver(*lhs_cursor, project_root, &origin) };
        return ir::Stmt::FieldAssign {
            target,
            field,
            value,
            origin,
        };
    }

    // `values[index] = value` reaches this branch as the `operator[]` call
    // expression used as the left side of an ordinary BinaryOperator. Its
    // stdlib lowering gives either a typed `Expr::Index` or, for a map, a
    // `MapIndexOrInsert`; the Dart emitter recognizes the latter as a write
    // and emits a direct map assignment without evaluating its read-default.
    let target = unsafe { lower_expr(*lhs_cursor, project_root) };
    if matches!(
        target,
        ir::Expr::Index { .. } | ir::Expr::MapIndexOrInsert { .. }
    ) {
        return ir::Stmt::ExprAssign {
            target,
            value,
            origin,
        };
    }
    // `*out = value` where `out` is a pointer-shaped out-param (round 19):
    // `lhs_kind` here is `CXCursor_UnaryOperator` (a dereference), not
    // `CXCursor_DeclRefExpr`, so the plain-variable branch above never
    // fires — but `lower_unary_expr`'s own out-param check already
    // resolved this exact `target` down to the same `Expr::Ref` shape that
    // branch produces (the parameter *is* the pointee, per the bridge), so
    // it gets the identical `Stmt::Assign` treatment here.
    if let ir::Expr::Ref { name, .. } = target {
        return ir::Stmt::Assign {
            name,
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

/// `m_numerator /= gcdVal;` — desugars to `m_numerator = m_numerator / gcdVal;`
/// (`lower_assign_stmt`'s own two target shapes, wrapping the synthesized
/// `Binary`). Only the arithmetic compound operators are mapped
/// (`compound_assign_op`); a bitwise/shift one (never seen in this corpus)
/// stays `Unsupported` rather than guessing.
unsafe fn lower_compound_assign_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Stmt {
    let operator_kind = unsafe { clang_sys::clang_getCursorBinaryOperatorKind(cursor) };
    let Some(op) = compound_assign_op(operator_kind) else {
        return ir::Stmt::Unsupported {
            reason: format!("unsupported compound assignment operator kind {operator_kind}"),
            origin,
        };
    };

    let children = unsafe { collect_children(cursor) };
    let [lhs_cursor, rhs_cursor] = children.as_slice() else {
        return ir::Stmt::Unsupported {
            reason: format!(
                "compound assignment cursor had {} children, expected 2",
                children.len()
            ),
            origin,
        };
    };

    let lhs_kind = unsafe { clang_sys::clang_getCursorKind(*lhs_cursor) };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    let value = ir::Expr::Binary {
        op,
        lhs: Box::new(unsafe { lower_expr(*lhs_cursor, project_root) }),
        rhs: Box::new(unsafe { lower_binary_operand(*rhs_cursor, project_root) }),
        ty,
        origin: origin.clone(),
    };

    if lhs_kind == clang_sys::CXCursor_DeclRefExpr {
        let name = unsafe {
            qualified_static_member_name(clang_sys::clang_getCursorReferenced(*lhs_cursor))
        };
        return ir::Stmt::Assign {
            name,
            value,
            origin,
        };
    }

    if lhs_kind == clang_sys::CXCursor_MemberRefExpr {
        let field = unsafe { dart_member_name(clang_sys::clang_getCursorReferenced(*lhs_cursor)) };
        let target = unsafe { member_ref_receiver(*lhs_cursor, project_root, &origin) };
        return ir::Stmt::FieldAssign {
            target,
            field,
            value,
            origin,
        };
    }

    let target = unsafe { lower_expr(*lhs_cursor, project_root) };
    if matches!(
        target,
        ir::Expr::Index { .. } | ir::Expr::MapIndexOrInsert { .. }
    ) && is_repeatable_expr(&target)
    {
        return ir::Stmt::ExprAssign {
            target,
            value,
            origin,
        };
    }

    ir::Stmt::Unsupported {
        reason: "compound assignment target is not a simple local variable or a field".to_owned(),
        origin,
    }
}

/// Whether `target` is representable as a Dart assignment target: one of
/// `Expr::is_assignable_lvalue`'s own shapes, or (the bridged
/// pointer-dereference shape `lower_unary_expr`'s address-of/dereference
/// handling produces) a `Convert` wrapping one. Anything else — most often
/// a nested `Expr::Unsupported`/`UnsupportedTyped` bailout surfacing where
/// the target itself failed to lower, e.g. `stdlib_method_receiver`
/// returning the bailout it got from `lower_expr` unchanged — is not just
/// semantically wrong but syntactically invalid Dart when placed on the
/// left of `=`: `SomeCall<T>(...) = value;` doesn't parse as a plain
/// assignment. Dart's grammar reads a callable-looking LHS as an attempted
/// destructuring pattern instead, and rejects it with unrelated-looking
/// pattern errors (confirmed on the real Verovio 6.2.0 corpus —
/// `not_a_type`/`positional_field_in_object_pattern`/
/// `refutable_pattern_in_irrefutable_context` all firing on the same line).
/// Returns the bailout's own reason when `target` already carries one, so
/// the statement-level `Stmt::Unsupported` this escalates to stays as
/// specific as the value-level bailout it replaces.
fn unassignable_target_reason(target: &ir::Expr) -> Option<String> {
    // `Expr::is_assignable_lvalue`'s own whitelist doesn't include
    // `MapIndexOrInsert` — a `std::map`/`unordered_map` index write is
    // still a legitimate assignment target `emit::dart` renders specially
    // (`target[index] = value;`, never through the generic lvalue path),
    // the same second shape `lower_assign_stmt`'s own pre-existing
    // `Index`/`MapIndexOrInsert` check already recognizes.
    if target.is_assignable_lvalue() || matches!(target, ir::Expr::MapIndexOrInsert { .. }) {
        return None;
    }
    if let ir::Expr::Convert { operand, .. } = target
        && operand.is_assignable_lvalue()
    {
        return None;
    }
    Some(match target {
        ir::Expr::Unsupported { reason, .. } | ir::Expr::UnsupportedTyped { reason, .. } => {
            reason.clone()
        }
        _ => "assignment target is not representable as a Dart assignment target".to_owned(),
    })
}

/// A compound assignment reads and writes its target. Dart's expanded
/// assignment therefore repeats the target expression; only lower targets
/// without observable evaluation effects are safe to use until the IR grows a
/// temporary-binding expression form.
fn is_repeatable_expr(expr: &ir::Expr) -> bool {
    match expr {
        ir::Expr::IntLiteral { .. }
        | ir::Expr::DoubleLiteral { .. }
        | ir::Expr::BoolLiteral { .. }
        | ir::Expr::NullLiteral { .. }
        | ir::Expr::StringLiteral { .. }
        | ir::Expr::Ref { .. }
        | ir::Expr::This { .. } => true,
        ir::Expr::FieldAccess { target, .. } => is_repeatable_expr(target),
        ir::Expr::Index { target, index, .. } => {
            is_repeatable_expr(target) && is_repeatable_expr(index)
        }
        ir::Expr::MapIndexOrInsert {
            target,
            index,
            default_value,
            ..
        } => {
            is_repeatable_expr(target)
                && is_repeatable_expr(index)
                && is_repeatable_expr(default_value)
        }
        _ => false,
    }
}

fn compound_assign_op(kind: clang_sys::CXBinaryOperatorKind) -> Option<ir::BinaryOp> {
    match kind {
        clang_sys::CXBinaryOperator_AddAssign => Some(ir::BinaryOp::Add),
        clang_sys::CXBinaryOperator_SubAssign => Some(ir::BinaryOp::Sub),
        clang_sys::CXBinaryOperator_MulAssign => Some(ir::BinaryOp::Mul),
        clang_sys::CXBinaryOperator_DivAssign => Some(ir::BinaryOp::Div),
        clang_sys::CXBinaryOperator_RemAssign => Some(ir::BinaryOp::Mod),
        clang_sys::CXBinaryOperator_ShlAssign => Some(ir::BinaryOp::ShiftLeft),
        clang_sys::CXBinaryOperator_ShrAssign => Some(ir::BinaryOp::ShiftRight),
        clang_sys::CXBinaryOperator_AndAssign => Some(ir::BinaryOp::BitAnd),
        clang_sys::CXBinaryOperator_XorAssign => Some(ir::BinaryOp::BitXor),
        clang_sys::CXBinaryOperator_OrAssign => Some(ir::BinaryOp::BitOr),
        _ => None,
    }
}

/// Cursor kinds `libclang` uses purely as sugar around another expression
/// (implicit conversions, parentheses) — lowering unwraps them transparently
/// by recursing into their single child, rather than treating them as their
/// own construct.
///
/// `static_cast<double>(m_numerator)` and `(void)fmt;` (both E13) join this
/// set too: an *explicit* cast has exactly the same single-child shape as an
/// implicit conversion (confirmed via `clang -Xclang -ast-dump`: `static_cast`
/// wraps the same `int`→`double` `ImplicitCastExpr` chain a plain implicit
/// promotion does, just with an extra `CXXStaticCastExpr`/`CStyleCastExpr`
/// layer on top whose own type already matches that chain's outer type) —
/// the outer/child type comparison below already turns a real numeric
/// mismatch into `Expr::Convert`, a cast to `void` into an outright discard
/// of the operand (any representable value can be safely thrown away; C++
/// only reaches a `void` target through an explicit cast, never an implicit
/// conversion), and leaves anything else this module can't represent exactly
/// as honest as any other unrecognized conversion, `Expr::Unsupported` rather
/// than silently dropped.
fn is_transparent_wrapper(kind: clang_sys::CXCursorKind) -> bool {
    matches!(
        kind,
        clang_sys::CXCursor_UnexposedExpr
            | clang_sys::CXCursor_ParenExpr
            // `std::string("oi")` (E08's `dobro(std::string("oi"))` call
            // argument) — C++'s functional-cast construction syntax
            // (`Type(args)`, as opposed to a declaration or `new`). Its
            // single child is the same `CXXConstructExpr` a plain
            // declaration's initializer already goes through, so it
            // unwraps exactly like any other sugar wrapper here.
            | clang_sys::CXCursor_CXXFunctionalCastExpr
            | clang_sys::CXCursor_CXXStaticCastExpr
            | clang_sys::CXCursor_CStyleCastExpr
    )
}

unsafe fn lower_expr(cursor: clang_sys::CXCursor, project_root: &Path) -> ir::Expr {
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
    let origin = stmt_origin(cursor, project_root);

    if is_transparent_wrapper(kind) {
        // `std::string("oi")` (E08's `dobro(std::string("oi"))` argument) —
        // a `CXXFunctionalCastExpr`'s children aren't just the wrapped
        // expression: the type it casts to comes along as its own
        // `NamespaceRef`/`TypeRef` children first (confirmed empirically,
        // not assumed: `[NamespaceRef, TypeRef, UnexposedExpr]` for this
        // exact call), the same trap already filtered for a `ParmVarDecl`'s
        // default-value child (E07) and a local variable's initializer
        // (E03) — same fix, third site.
        let mut children: Vec<clang_sys::CXCursor> = unsafe { collect_children(cursor) }
            .into_iter()
            .filter(|child| {
                !matches!(
                    unsafe { clang_sys::clang_getCursorKind(*child) },
                    clang_sys::CXCursor_TypeRef
                        | clang_sys::CXCursor_NamespaceRef
                        | clang_sys::CXCursor_TemplateRef
                )
            })
            .collect();
        if children.len() == 1 {
            let child_cursor = children.remove(0);
            let inner = unsafe { lower_expr(child_cursor, project_root) };

            // A `std::string`-typed literal ("Ola, ") lowers to
            // `Expr::StringLiteral` (see that variant's doc comment) at the
            // *inner* `StringLiteral` cursor — but the outer wrapper here is
            // `libclang`'s `ArrayToPointerDecay` (`const char[N]` →
            // `const char *`, confirmed via `clang -Xclang -ast-dump`), a
            // real C++ type change the `outer_ty`/`child_ty` comparison
            // below would reject: `lower_type` has no array/pointer
            // handling, so both sides come back as differently-spelled
            // `Type::Unsupported`, which don't compare equal to each other
            // or to the `Int`/`Double` case just below. The string literal
            // is already fully and correctly lowered at this point — its
            // C-string decay is irrelevant sugar, exactly as sugary as the
            // cases the comparison below exists to let through — so it
            // returns immediately instead of falling into a comparison that
            // was never meant to evaluate `Type::Unsupported`/`Type::Unsupported`
            // pairs as "different types" in the first place.
            if matches!(inner, ir::Expr::StringLiteral { .. }) {
                return inner;
            }

            // An anonymous enum's constant (`lower_expr`'s own
            // `DeclRefExpr` case, just above, already inlines it as a
            // plain `IntLiteral` — there's no Dart type to name it
            // through) hits the exact same trap: `clang_getCursorType` on
            // the *cursor* still reports the enum's own (unnamed,
            // `Type::Unsupported`) type, disagreeing with `outer_ty`
            // (`Int`) below even though `inner` is already the correct,
            // final value. Scoped to `outer_ty == Int` specifically — the
            // one context this shortcut is proven correct for; anything
            // else (an anonymous-enum constant used where a `double` is
            // expected, unseen in this corpus) still falls through to the
            // ordinary comparison below rather than being guessed at.
            if matches!(inner, ir::Expr::IntLiteral { .. })
                && lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }) == ir::Type::Int
            {
                return inner;
            }

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
            } else if child_ty == ir::Type::Int && outer_ty == ir::Type::Bool {
                // C++ truthiness is an implicit conversion, not a Dart
                // property of integers. Keep it as a typed conversion so a
                // returned/conditioned integer becomes `value != 0` instead
                // of an invalid Dart boolean context.
                ir::Expr::Convert {
                    operand: Box::new(inner),
                    ty: ir::Type::Bool,
                    origin,
                }
            } else if child_ty == ir::Type::Bool && outer_ty == ir::Type::Int {
                // The mirror image of the truthiness case just above: C++
                // implicitly converts `bool` to `1`/`0` wherever an integer
                // is expected (arithmetic, an `int`-typed initializer, a
                // return). `emit::dart` turns this into `operand ? 1 : 0`.
                ir::Expr::Convert {
                    operand: Box::new(inner),
                    ty: ir::Type::Int,
                    origin,
                }
            } else if matches!(child_ty, ir::Type::Enum { .. }) && outer_ty == ir::Type::Int {
                // C++ implicitly reads an enumerator as its underlying
                // integer value wherever an `int` is expected — one of the
                // highest-volume bailout families in the real Verovio 6.2.0
                // corpus. `emit::dart` reads the enum's own `.value` field
                // (`ir::Enum::values`), never Dart's `.index`: C++
                // enumerators aren't guaranteed 0-based/sequential/gapless,
                // so `.index` would silently compute a different number for
                // any enum that isn't.
                ir::Expr::Convert {
                    operand: Box::new(inner),
                    ty: ir::Type::Int,
                    origin,
                }
            } else if child_ty == ir::Type::Double && outer_ty == ir::Type::Int {
                // C++'s narrowing `double` → `int` truncates toward zero in
                // exactly the same direction `.toInt()` does — safe to
                // represent directly, the same way the widening `int` →
                // `double` case above is.
                //
                // Chained `int` → `double` → `int` conversions cancel out to
                // identity on the original `int` expression: converting an `int`
                // to IEEE-754 double is lossless for 32-bit integers, and
                // converting back to `int` has no fractional part to truncate,
                // so `.toDouble().toInt()` is a pure no-op (Tarefa 11).
                if let ir::Expr::Convert {
                    operand: inner_op,
                    ty: ir::Type::Double,
                    ..
                } = &inner
                    && (matches!(inner_op.ty(), Some(ir::Type::Int))
                        || matches!(inner_op.as_ref(), ir::Expr::IntLiteral { .. }))
                {
                    return *inner_op.clone();
                }
                ir::Expr::Convert {
                    operand: Box::new(inner),
                    ty: ir::Type::Int,
                    origin,
                }
            } else if (child_ty == ir::Type::Int
                || child_ty == ir::Type::Unsupported("std::nullptr_t".to_owned()))
                && matches!(outer_ty, ir::Type::Nullable(_))
                && matches!(
                    inner,
                    ir::Expr::IntLiteral { value: 0, .. } | ir::Expr::NullLiteral { .. }
                )
            {
                // `Measure *measure = NULL;` / `if (measure != NULL)` — a
                // real Verovio idiom throughout the corpus predating
                // `nullptr` (`adjustaccidxfunctor.cpp:25`, among many
                // others). C++ only ever inserts an implicit `int` →
                // pointer conversion for the null-pointer constant (a bare
                // `0`, or `NULL`'s `__null` — already `Expr::NullLiteral`
                // by the time it reaches here, see
                // `CXCursor_GNUNullExpr`'s own handling above). `nullptr`
                // itself lowers directly to `Expr::NullLiteral` too
                // (`CXCursor_CXXNullPtrLiteralExpr`'s own handling above),
                // but its *type* is `std::nullptr_t`, which `lower_type` has
                // no arm for — reaches this wrapper as
                // `Unsupported("std::nullptr_t")`, not `Int`, so needs its
                // own branch of this same condition; the `inner` shape check
                // still guards it exactly as tightly as the `Int` case
                // above. Matching on exactly these null-constant shapes (not
                // just "any `Int`"/"any `Unsupported`") is exactly as safe
                // as trusting the compiler's own acceptance elsewhere in
                // this function.
                ir::Expr::NullLiteral { origin }
            } else if matches!(child_ty, ir::Type::Nullable(_)) && outer_ty == ir::Type::Bool {
                // C++ accepts a pointer in every boolean context. Known
                // object pointers lower to `T?`, for which Dart requires the
                // explicit null test rather than treating the reference as a
                // boolean value.
                ir::Expr::Convert {
                    operand: Box::new(inner),
                    ty: ir::Type::Bool,
                    origin,
                }
            } else if outer_ty == ir::Type::Void {
                // `(void)fmt;` (E13's `LogDebug` stub) — C++'s idiom for
                // "evaluate this and deliberately discard the result",
                // almost always used to silence an unused-parameter/
                // unused-variable warning. This used to be unreachable in
                // practice: every pointee this module could lower always
                // hit `Type::Unsupported` before caso 3
                // (`docs/plans/verovio-6.2-pointer-types.md`) taught
                // `char`/`const char*` to become `Type::Nullable(Type::Str)`,
                // so `outer_ty`/`child_ty` were never both representable at
                // once here. Now that a cast-to-`void` operand genuinely
                // can be representable, discarding is exactly as sound as
                // the same-type unwrap just above — the value is legitimate,
                // C++ itself never lets you implicitly convert *to* `void`
                // (only an explicit cast reaches this branch), and Dart is
                // happy to evaluate `inner` as a bare expression statement.
                inner
            } else if matches!(child_ty, ir::Type::Record { .. })
                && matches!(outer_ty, ir::Type::Record { .. })
            {
                // A derived-to-base implicit conversion (E06 —
                // `apresentarAnimal(c)` passing a `Cachorro` where an
                // `Animal` is expected): C++ only ever inserts this specific
                // wrapper when `child_ty` really does derive from `outer_ty`
                // (the compiler already checked that to accept the source in
                // the first place), and Dart needs no cast at all for a
                // subtype value used where its supertype is expected — the
                // object already satisfies both types. Not narrowed to
                // "`child_ty`'s `Record` is actually a subtype of
                // `outer_ty`'s" because nothing at this call site has the
                // whole `Module` in scope to check that against; trusting
                // the C++ compiler's own acceptance of the source is exactly
                // as sound.
                inner
            } else if let (ir::Type::Nullable(child_record), ir::Type::Nullable(outer_record)) =
                (&child_ty, &outer_ty)
                && let (
                    ir::Type::Record { usr: child_usr, .. },
                    ir::Type::Record { usr: outer_usr, .. },
                ) = (child_record.as_ref(), outer_record.as_ref())
            {
                // The nullable counterpart of the record upcast above. A
                // `Derived*` becomes `Derived?` and a `Base*` becomes
                // `Base?`; Dart preserves the same subtype relation, so a
                // compiler-accepted *implicit* C++ conversion (only ever
                // inserted for a derived-to-base widening) needs no
                // generated cast.
                //
                // An *explicit* `static_cast`/C-style cast reaching this
                // same shape (`kind` one of the two, rather than the
                // implicit-conversion `UnexposedExpr`) isn't guaranteed to
                // be that same safe widening — F7
                // (`docs/prompts/2026-08-21-05-downcast-de-hierarquia-preservado.md`):
                // Verovio's `vrv_cast<Doc *>(object)` (`vrv_cast` is
                // `#define vrv_cast static_cast`,
                // `include/vrv/vrvdef.h:65`) narrows `Object*` down to
                // `Doc*`, and unwrapping it the same way as the implicit
                // case silently drops the cast, leaving the operand typed
                // as its own base — confirmed as the real corpus trigger
                // for ~1523 `dart analyze` diagnostics
                // (`argument_type_not_assignable`/`invalid_assignment`/
                // `return_of_invalid_type`) in the Verovio 6.2.0 diagnosis.
                // `record_derives_from` walks the *child*'s own base
                // specifiers (transitively, straight from libclang cursors
                // — no `Module`/catalog exists yet at this point in
                // lowering) to tell a genuine upcast, still transparent,
                // from a downcast or unrelated cast, which needs a real
                // Dart cast to survive.
                if child_usr == outer_usr {
                    inner
                } else if matches!(
                    kind,
                    clang_sys::CXCursor_CXXStaticCastExpr | clang_sys::CXCursor_CStyleCastExpr
                ) && !unsafe {
                    let child_pointer_ty = clang_sys::clang_getCursorType(child_cursor);
                    let child_pointee_ty = clang_sys::clang_getPointeeType(child_pointer_ty);
                    let child_decl = clang_sys::clang_getTypeDeclaration(child_pointee_ty);
                    record_derives_from(child_decl, outer_usr)
                } {
                    // A narrowing (or unrelated) explicit cast: C++'s own
                    // `static_cast` is unchecked here (undefined behavior on
                    // a real mismatch, not a null result), so the honest
                    // translation is Dart's checked `as T?` — it throws a
                    // `TypeError` if the source wasn't really a `T`, rather
                    // than silently turning a program bug into `null` the
                    // way `dynamic_cast`'s ternary form would.
                    ir::Expr::As {
                        operand: Box::new(inner),
                        ty: outer_ty.clone(),
                        origin,
                    }
                } else {
                    inner
                }
            } else if child_ty == ir::Type::List(Box::new(ir::Type::Int))
                && (outer_ty == ir::Type::Bytes
                    || outer_ty == ir::Type::Nullable(Box::new(ir::Type::Bytes)))
            {
                // A fixed-size `uint8_t[N]` array decays to `List(Int)`
                // (`lower_type`'s `CXType_ConstantArray` branch has no
                // byte-buffer special case, unlike a `uint8_t*` parameter's
                // own type) — passing it where a `const uint8_t*`/`Uint8List?`
                // parameter is expected needs this bridge, the same
                // "compiles and is right" reasoning as `List.of`/
                // `Uint8List.fromList` already applied to copy-construction
                // in `clone_value_expr` just above.
                ir::Expr::Call {
                    base_qualifier: None,
                    target: None,
                    callee_usr: String::new(),
                    callee_name: "Uint8List.fromList".to_owned(),
                    args: vec![inner],
                    ty: outer_ty,
                    origin,
                }
            } else {
                ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "unsupported implicit conversion from {child_ty:?} to {outer_ty:?}"
                    ),
                    ty: outer_ty,
                    origin,
                }
            };
        }
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "wrapper cursor kind {kind} had {} children after filtering type \
                 references, expected exactly one",
                children.len()
            ),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    }

    if kind == clang_sys::CXCursor_DeclRefExpr {
        let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
        // An anonymous `enum { SMUFL_0020_space = 0x0020, ... }` (a common
        // C idiom for a group of named integer constants, not a real type
        // — confirmed as the real Verovio shape by grepping the source,
        // `include/vrv/smufl.h`) never gets a Dart type declared for it
        // (`lower_enum`/`enum_identity` already refuse an anonymous enum,
        // correctly — there is no usable Dart type name). A *reference* to
        // one of its enumerators has no stable Dart binding to name either,
        // then: `Type::Unsupported("(unnamed enum at ...)")`. But the
        // enumerator's own value is a real, known compile-time constant
        // (`clang_getEnumConstantDeclValue`, the same accessor
        // `enum_variants` already uses for a *named* enum's declaration) —
        // inlining it directly is exact, not a guess, and needs no type
        // identity at all.
        if unsafe { clang_sys::clang_getCursorKind(referenced) }
            == clang_sys::CXCursor_EnumConstantDecl
        {
            let parent_enum = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
            if unsafe { clang_sys::clang_Cursor_isAnonymous(parent_enum) } != 0 {
                let value = unsafe { clang_sys::clang_getEnumConstantDeclValue(referenced) };
                return ir::Expr::IntLiteral { value, origin };
            }
        }
        let name = unsafe { qualified_static_member_name(referenced) };
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        return ir::Expr::Ref { name, ty, origin };
    }

    if kind == clang_sys::CXCursor_MemberRefExpr {
        let field = unsafe { dart_member_name(clang_sys::clang_getCursorReferenced(cursor)) };
        let target = unsafe { member_ref_receiver(cursor, project_root, &origin) };
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        return ir::Expr::FieldAccess {
            target: Box::new(target),
            field,
            ty,
            origin,
        };
    }

    if kind == clang_sys::CXCursor_CXXThisExpr {
        // `clang_getCursorType` on a `this` cursor is the pointer type
        // (`ContaBancaria *`, or `const ContaBancaria *` inside a `const`
        // method) — `lower_type` has no pointer handling (E10 scope), so it
        // would report this as `Unsupported`. `This` never needs its own
        // type for anything `emit::dart` does with it today (it only ever
        // appears as an omitted receiver), so a placeholder `Void` is
        // honest about "not represented" without spuriously bailing out the
        // whole enclosing method the way a real `Type::Unsupported` would.
        let this_ty = match lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }) {
            ir::Type::Nullable(inner) => *inner,
            _ => ir::Type::Void,
        };
        return ir::Expr::This {
            ty: this_ty,
            origin,
        };
    }

    if kind == clang_sys::CXCursor_StringLiteral {
        return match unsafe { string_literal_text(cursor) } {
            Some(value) => ir::Expr::StringLiteral { value, origin },
            None => ir::Expr::UnsupportedTyped {
                reason: "could not evaluate string literal".to_owned(),
                ty: ir::Type::Str,
                origin,
            },
        };
    }

    if kind == clang_sys::CXCursor_CharacterLiteral {
        return match unsafe { evaluate_int_eval_result(cursor) } {
            Some(value) => ir::Expr::IntLiteral { value, origin },
            None => ir::Expr::UnsupportedTyped {
                reason: "could not evaluate character literal".to_owned(),
                ty: ir::Type::Int,
                origin,
            },
        };
    }

    if kind == clang_sys::CXCursor_IntegerLiteral {
        return match unsafe { evaluate_int_eval_result(cursor) } {
            Some(value) => ir::Expr::IntLiteral { value, origin },
            None => ir::Expr::UnsupportedTyped {
                reason: "could not evaluate integer literal".to_owned(),
                ty: ir::Type::Int,
                origin,
            },
        };
    }

    // `NULL` — glibc/libstdc++'s `<cstddef>` defines it as GNU's `__null`
    // builtin (confirmed via `clang -E`, not assumed), a distinct cursor
    // kind libclang reports with type `long` rather than folding it to a
    // plain `0` the way `CXCursor_IntegerLiteral` already does. Real
    // Verovio trigger throughout the corpus, predating `nullptr`
    // (`adjustaccidxfunctor.cpp:25`'s `m_currentMeasure = NULL;`, among
    // many others). `__null` only ever means "the null pointer constant"
    // wherever C++ allows it to appear, so this lowers directly to
    // `Expr::NullLiteral` rather than routing through the generic
    // `Int` → `Nullable` implicit-conversion wrapper below (which only
    // catches this when `libclang` happens to wrap it in an explicit
    // conversion cursor — not guaranteed for every context `__null` can
    // appear in, e.g. a call argument already matching the parameter's
    // nullable type).
    if kind == clang_sys::CXCursor_GNUNullExpr {
        return ir::Expr::NullLiteral { origin };
    }

    // `nullptr` — its own type is `std::nullptr_t`, which `lower_type` has
    // no arm for (falls to the generic `Unsupported("std::nullptr_t")`
    // catch-all), so without this direct case the implicit-conversion
    // wrapper below only ever sees an already-`Unsupported` child type and
    // bails, even though `nullptr` unconditionally means "the null pointer
    // constant" everywhere C++ allows it — real trigger:
    // `include/vrv/floatingobject.h`-shaped `void *drawingGrpObject`
    // fields/params compared with `field == nullptr`, once `void*` itself
    // stopped being a bailout (`NATIVE_HANDLE_TYPE_NAME`). Same reasoning as
    // `CXCursor_GNUNullExpr` just above, kept as its own `if` rather than
    // folded into that one so each cursor kind's own doc comment stays next
    // to its own trigger.
    if kind == clang_sys::CXCursor_CXXNullPtrLiteralExpr {
        return ir::Expr::NullLiteral { origin };
    }

    if kind == clang_sys::CXCursor_FloatingLiteral {
        return match unsafe { evaluate_float_eval_result(cursor) } {
            Some(value) => ir::Expr::DoubleLiteral { value, origin },
            None => ir::Expr::UnsupportedTyped {
                reason: "could not evaluate floating-point literal".to_owned(),
                ty: ir::Type::Double,
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
            None => ir::Expr::UnsupportedTyped {
                reason: "could not evaluate bool literal".to_owned(),
                ty: ir::Type::Bool,
                origin,
            },
        };
    }

    if kind == clang_sys::CXCursor_CXXNewExpr {
        return unsafe { lower_new_expr(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_ArraySubscriptExpr {
        return unsafe { lower_array_subscript_expr(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_BinaryOperator {
        return unsafe { lower_binary_expr(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_ConditionalOperator {
        return unsafe { lower_conditional_expr(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_UnaryOperator {
        return unsafe { lower_unary_expr(cursor, project_root, origin) };
    }

    if kind == clang_sys::CXCursor_CallExpr {
        return unsafe { lower_call_expr(cursor, project_root, origin) };
    }

    // `sizeof(T)`/`alignof(T)`/other C++ type-trait unary expressions all
    // share this one `libclang` cursor kind (`UnaryExpr`), with no sub-kind
    // exposed on the cursor API to tell them apart. Rather than naming each
    // one, evaluate first: `clang_Cursor_Evaluate` (`evaluate_int_eval_
    // result`, already used for integer/bool literals) already
    // constant-folds a `sizeof`/`alignof` whose operand type is complete
    // and has a known layout — precisely the "map only when the size is
    // well-defined" scope the backlog calls for. A shape it can't fold
    // (`sizeof` of an incomplete/dependent type, or some other type-trait
    // extension) falls straight through to the generic bailout below,
    // unchanged.
    if kind == clang_sys::CXCursor_UnaryExpr
        && let Some(value) = unsafe { evaluate_int_eval_result(cursor) }
    {
        return ir::Expr::IntLiteral { value, origin };
    }

    if kind == clang_sys::CXCursor_InitListExpr {
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        // Only the `List<T>` destination is handled here (a brace initializer
        // for a `std::vector`/`std::array`/`std::initializer_list`, the
        // shape libclang's own `clang_getCursorType` on the `InitListExpr`
        // cursor already resolves to `List<T>` for). Aggregate structs,
        // `Set`/`Map` and fixed C arrays resolve to a different `ty` here
        // and stay an explicit bailout below rather than guessing a Dart
        // literal shape from an unverified type.
        if let ir::Type::List(_) = &ty {
            let items = unsafe { collect_children(cursor) }
                .into_iter()
                .map(|child| unsafe { lower_expr(child, project_root) })
                .collect();
            return ir::Expr::ListLiteral { items, ty, origin };
        }
    }

    if kind == clang_sys::CXCursor_CXXDynamicCastExpr {
        return unsafe { lower_dynamic_cast_expr(cursor, project_root, origin) };
    }

    ir::Expr::UnsupportedTyped {
        reason: format!("unsupported expression cursor kind {kind}"),
        ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
        origin,
    }
}

/// `dynamic_cast<T*>(operand)` — a checked downcast, common in Verovio for
/// walking a class hierarchy from a base pointer (`options.cpp`'s
/// `dynamic_cast<const OptionDbl *>(this)`, `dynamic_cast<OptionBool
/// *>(option)`, etc. — confirmed real occurrences, not `vrv_cast`, the
/// project's own macro: that one expands to `static_cast` unless a debug
/// build flag is set, `include/vrv/vrvdef.h:59`, so it's already handled
/// transparently elsewhere as an ordinary cast wrapper). Scoped to a
/// *simple* operand (`this` or a bare local/parameter reference): grepping
/// the Verovio source directly shows this covers the clear majority of
/// real occurrences (`dynamic_cast<T*>(this)`/`dynamic_cast<T*>(option)`-
/// shaped, 254 of 435 raw textual matches) — because emitting `operand is
/// T ? operand : null`
/// evaluates `operand` twice, and a call or field access as operand would
/// risk duplicating a side effect or repeating real work; there is no way
/// to hoist a temporary from pure-expression lowering here (the same
/// architectural gap that already defers `unsupported binary operator kind
/// 22`, assignment used as an expression). Dart's flow-sensitive type
/// promotion inside a ternary's condition→then branch (guaranteed safe by
/// this same scoping, since a plain local/parameter/`this` is exactly what
/// it promotes) means the `then` branch needs no separate cast at all.
unsafe fn lower_dynamic_cast_expr(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    // `libclang` emits a leading `TypeRef` for the cast's target type
    // (`OptionBool`) alongside the real operand child — the same
    // navigation-only noise already filtered at every other site in this
    // module that walks a cursor's children looking for the one real
    // value.
    let children: Vec<clang_sys::CXCursor> = unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
            )
        })
        .collect();
    let [operand_cursor] = children.as_slice() else {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "dynamic_cast had {} children after filtering type references, expected exactly 1",
                children.len()
            ),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    };
    let operand_value_cursor = unsafe { unwrap_transparent_value_cursor(*operand_cursor) };
    if !matches!(
        unsafe { clang_sys::clang_getCursorKind(operand_value_cursor) },
        clang_sys::CXCursor_DeclRefExpr | clang_sys::CXCursor_CXXThisExpr
    ) {
        return ir::Expr::UnsupportedTyped {
            reason: "dynamic_cast operand is not a simple reference — re-evaluating a call \
                     or field access twice risks duplicating a side effect"
                .to_owned(),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    }

    let target_type = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    let ir::Type::Nullable(target_record) = &target_type else {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "dynamic_cast target type does not resolve to a nullable record: {target_type:?}"
            ),
            ty: target_type.clone(),
            origin,
        };
    };
    if !matches!(target_record.as_ref(), ir::Type::Record { .. }) {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "dynamic_cast target type does not resolve to a representable record: {target_type:?}"
            ),
            ty: target_type.clone(),
            origin,
        };
    }

    let operand = unsafe { lower_expr(operand_value_cursor, project_root) };
    ir::Expr::Conditional {
        condition: Box::new(ir::Expr::Is {
            operand: Box::new(operand.clone()),
            target_type: (**target_record).clone(),
            origin: origin.clone(),
        }),
        then_expr: Box::new(operand.clone()),
        else_expr: Box::new(ir::Expr::NullLiteral {
            origin: origin.clone(),
        }),
        ty: target_type,
        origin,
    }
}

unsafe fn lower_conditional_expr(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let children = unsafe { collect_children(cursor) };
    let [condition_cursor, then_cursor, else_cursor] = children.as_slice() else {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "conditional operator had {} children, expected condition+then+else",
                children.len()
            ),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    };
    ir::Expr::Conditional {
        condition: Box::new(unsafe { lower_expr(*condition_cursor, project_root) }),
        then_expr: Box::new(unsafe { lower_expr(*then_cursor, project_root) }),
        else_expr: Box::new(unsafe { lower_expr(*else_cursor, project_root) }),
        ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
        origin,
    }
}

/// A C++ allocation of a project record has the same object construction
/// shape as Dart's managed runtime. Restrict this to an already-lowered
/// record pointer: allocation of scalar storage, arrays, and FFI-facing
/// objects remains an explicit unsupported boundary.
unsafe fn lower_new_expr(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let allocation_type = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    if !matches!(
        allocation_type,
        ir::Type::Nullable(ref inner) if matches!(inner.as_ref(), ir::Type::Record { .. })
    ) {
        return ir::Expr::UnsupportedTyped {
            reason: "CXX new needs a known record pointee or an explicit ownership adapter"
                .to_owned(),
            ty: allocation_type,
            origin,
        };
    }

    let children: Vec<clang_sys::CXCursor> = unsafe { collect_children(cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
            )
        })
        .collect();
    let [construction_cursor] = children.as_slice() else {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "CXX new had {} construction children, expected exactly 1",
                children.len()
            ),
            ty: allocation_type,
            origin,
        };
    };
    let construction = unsafe { lower_expr(*construction_cursor, project_root) };
    if matches!(
        construction,
        ir::Expr::ConstructorCall { .. } | ir::Expr::RecordConstruct { .. }
    ) {
        return construction;
    }
    // `new T(*this)` / `new T(other)` — a copy-construction allocation, the
    // real shape behind Verovio's own `Clone()` idiom (`abbr.h`'s `Object
    // *Clone() const override { return new Abbr(*this); }`, confirmed via
    // grep against the real source). `lower_call_expr`'s own copy/move
    // handling already treats the constructor call as transparent sugar
    // (E03) and recurses straight into the single real argument, so
    // `construction` here is already whatever `*this`/`other` itself lowers
    // to — never a `ConstructorCall`/`RecordConstruct`. Recognized via
    // `clang_CXXConstructor_isCopyConstructor`/`isMoveConstructor` on the
    // *resolved* constructor rather than guessed from `construction`'s own
    // shape, then rebuilt as a field-by-field `RecordConstruct` reading
    // every field off the copy source — the same construction
    // `collect_params_with_clone_prelude` already builds for a by-value
    // parameter's own copy-on-entry clone (E03), just keyed to an arbitrary
    // receiver expression instead of a parameter name.
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(*construction_cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } == 0
        && unsafe { clang_sys::clang_getCursorKind(referenced) } == clang_sys::CXCursor_Constructor
        && unsafe { is_copy_or_move_constructor(referenced) }
        && let ir::Type::Nullable(record_type) = &allocation_type
        && let ir::Type::Record { usr, name } = record_type.as_ref()
    {
        let pointee_cx_type =
            unsafe { clang_sys::clang_getPointeeType(clang_sys::clang_getCursorType(cursor)) };
        let decl = unsafe { clang_sys::clang_getTypeDeclaration(pointee_cx_type) };
        let fields = unsafe { record_fields_of(decl) };
        let field_values = fields
            .into_iter()
            .map(|field| {
                let access = ir::Expr::FieldAccess {
                    target: Box::new(construction.clone()),
                    field: field.name.clone(),
                    ty: field.ty,
                    origin: origin.clone(),
                };
                (field.name, access)
            })
            .collect();
        return ir::Expr::RecordConstruct {
            type_usr: usr.clone(),
            type_name: name.clone(),
            fields: field_values,
            origin,
        };
    }
    ir::Expr::UnsupportedTyped {
        reason: "CXX new child was not a representable record construction".to_owned(),
        ty: allocation_type,
        origin,
    }
}

/// Lowers a native C/C++ array subscript only when its receiver's declaration
/// is already represented as a Dart collection. Array-to-pointer decay makes
/// the expression cursor itself look like a pointer; recovering the referenced
/// declaration's type distinguishes a fixed array from raw pointer arithmetic.
unsafe fn lower_array_subscript_expr(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let children = unsafe { collect_children(cursor) };
    let [target_cursor, index_cursor] = children.as_slice() else {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "array subscript cursor had {} children, expected 2",
                children.len()
            ),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    };

    let target_value_cursor = unsafe { unwrap_transparent_value_cursor(*target_cursor) };
    let target_kind = unsafe { clang_sys::clang_getCursorKind(target_value_cursor) };
    let target = if target_kind == clang_sys::CXCursor_DeclRefExpr {
        let referenced = unsafe { clang_sys::clang_getCursorReferenced(target_value_cursor) };
        let declared_type = lower_type(unsafe { clang_sys::clang_getCursorType(referenced) });
        if matches!(declared_type, ir::Type::List(_) | ir::Type::Bytes) {
            ir::Expr::Ref {
                name: unsafe { qualified_static_member_name(referenced) },
                ty: declared_type,
                origin: stmt_origin(target_value_cursor, project_root),
            }
        } else {
            unsafe { lower_expr(*target_cursor, project_root) }
        }
    } else if target_kind == clang_sys::CXCursor_MemberRefExpr {
        // The same array-to-pointer decay the `DeclRefExpr` branch above
        // works around, but for a fixed-size array *field*
        // (`m_data[i]`/`this->m_data[i]`) instead of a local/global
        // variable — `lower_type(clang_getCursorType(target_value_cursor))`
        // would report the decayed pointer type, not the field's real
        // declared `T[N]`, so the referenced `FieldDecl`'s own type is
        // recovered directly instead, same as the `DeclRefExpr` case.
        let referenced = unsafe { clang_sys::clang_getCursorReferenced(target_value_cursor) };
        let declared_type = lower_type(unsafe { clang_sys::clang_getCursorType(referenced) });
        if matches!(declared_type, ir::Type::List(_) | ir::Type::Bytes) {
            ir::Expr::FieldAccess {
                target: Box::new(unsafe {
                    member_ref_receiver(target_value_cursor, project_root, &origin)
                }),
                field: unsafe { dart_member_name(referenced) },
                ty: declared_type,
                origin: stmt_origin(target_value_cursor, project_root),
            }
        } else {
            unsafe { lower_expr(*target_cursor, project_root) }
        }
    } else if target_kind == clang_sys::CXCursor_ArraySubscriptExpr {
        // A nested subscript (`m_rows[i][j]`, a multidimensional fixed
        // array field): the same array-to-pointer decay wraps `m_rows[i]`
        // in an implicit conversion cursor when it's used as *this*
        // subscript's own target, since the built-in `E1[E2]` requires `E1`
        // to decay to a pointer first. Recursing through `lower_expr` on
        // the *original* (still-wrapped) `*target_cursor` would hit that
        // wrapper's generic conversion handling, which only knows real
        // scalar/pointer/enum conversions — not "this decay is moot, the
        // inner subscript is already a `List`" — and bail. Recursing on
        // `target_value_cursor` (already unwrapped by
        // `unwrap_transparent_value_cursor` above) skips straight to
        // `lower_array_subscript_expr` again for the inner subscript,
        // which needs no decay at all: a `List` is already indexable.
        unsafe { lower_expr(target_value_cursor, project_root) }
    } else {
        unsafe { lower_expr(*target_cursor, project_root) }
    };

    // `Expr::Index` covers a nested subscript (`m_rows[i][j]`, a
    // multidimensional fixed array field): the outer subscript's target is
    // itself the already-lowered `Expr::Index` for `m_rows[i]`, whose `ty`
    // is the inner `List<T>`'s own element type — one level removed from
    // the field's declared `List<List<T>>`, but still `List`/`Bytes` and
    // just as indexable.
    let is_indexable = matches!(
        target,
        ir::Expr::Ref {
            ty: ir::Type::List(_) | ir::Type::Bytes,
            ..
        } | ir::Expr::FieldAccess {
            ty: ir::Type::List(_) | ir::Type::Bytes,
            ..
        } | ir::Expr::Index {
            ty: ir::Type::List(_) | ir::Type::Bytes,
            ..
        }
    );
    if !is_indexable {
        return ir::Expr::UnsupportedTyped {
            reason: "array subscript receiver is not a lowered Dart collection".to_owned(),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    }

    ir::Expr::Index {
        target: Box::new(target),
        index: Box::new(unsafe { lower_expr(*index_cursor, project_root) }),
        ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
        origin,
    }
}

/// Removes the purely syntactic wrappers that hide an array declaration behind
/// lvalue-to-rvalue or array-to-pointer decay. If a wrapper has an unexpected
/// value shape, preserve it for normal lowering instead of guessing.
unsafe fn unwrap_transparent_value_cursor(mut cursor: clang_sys::CXCursor) -> clang_sys::CXCursor {
    loop {
        let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
        if !is_transparent_wrapper(kind) {
            return cursor;
        }
        let children: Vec<clang_sys::CXCursor> = unsafe { collect_children(cursor) }
            .into_iter()
            .filter(|child| {
                !matches!(
                    unsafe { clang_sys::clang_getCursorKind(*child) },
                    clang_sys::CXCursor_TypeRef
                        | clang_sys::CXCursor_NamespaceRef
                        | clang_sys::CXCursor_TemplateRef
                )
            })
            .collect();
        let [only_child] = children.as_slice() else {
            return cursor;
        };
        cursor = *only_child;
    }
}

/// Each child of a `std::map`/`unordered_map` initializer list's underlying
/// `InitListExpr` is `{ key, value }` — confirmed empirically (not
/// guessed from the full internal AST, which wraps each entry in an
/// implicit `CXXConstructExpr` for `std::pair` that `libclang`'s coarser
/// cursor API never surfaces): each pair cursor's own kind is *also*
/// `CXCursor_InitListExpr`, with exactly 2 children — the key and value
/// expressions directly, no `std::pair` construction machinery to unwrap.
/// `None` for any child that isn't exactly this shape — the caller falls
/// back to the ordinary bailout rather than guessing.
unsafe fn map_literal_entries(
    init_list_cursor: clang_sys::CXCursor,
    key_ty: &ir::Type,
    project_root: &Path,
) -> Option<Vec<(ir::Expr, ir::Expr)>> {
    unsafe { collect_children(init_list_cursor) }
        .into_iter()
        .map(|pair_cursor| {
            if unsafe { clang_sys::clang_getCursorKind(pair_cursor) }
                != clang_sys::CXCursor_InitListExpr
            {
                return None;
            }
            let pair_children = unsafe { collect_children(pair_cursor) };
            let [key_cursor, value_cursor] = pair_children.as_slice() else {
                return None;
            };
            let key =
                coerce_map_literal_key(unsafe { lower_expr(*key_cursor, project_root) }, key_ty);
            let value = unsafe { lower_expr(*value_cursor, project_root) };
            Some((key, value))
        })
        .collect()
}

/// A map literal's declared key type (`ir::Type::Map`'s first component)
/// can differ from one entry's own key expression type when C++'s
/// unscoped-enum-to-`int`-implicit-conversion is in play — real trigger:
/// `alignfunctor.cpp:44`'s `durationEq`, a `std::map<int, data_DURATION>`
/// keyed by `option_DURATION_EQ` constants (F15/tarefa 15.6).
/// `map_literal_entries` lowers each pair's key straight off its own
/// cursor, with no implicit-cast wrapper cursor libclang exposes inside a
/// braced-init-list entry the way `lower_binary_operand`/
/// `default_argument_value` catch this for other implicit-conversion
/// contexts — so a bare enum member reached a `Map<int, ...>` literal
/// unconverted, `map_key_type_not_assignable`. Folds it down to
/// `Expr::Convert { ty: Int, .. }`, the same node any other
/// unscoped-enum-to-`int` conversion in this emitter already becomes,
/// which `emit::dart` renders as `.value`.
fn coerce_map_literal_key(key: ir::Expr, declared_key_ty: &ir::Type) -> ir::Expr {
    if *declared_key_ty == ir::Type::Int && matches!(key.ty(), Some(ir::Type::Enum { .. })) {
        let origin = key.origin().clone();
        return ir::Expr::Convert {
            operand: Box::new(key),
            ty: ir::Type::Int,
            origin,
        };
    }
    key
}

/// Lowers an operand of a binary arithmetic or relational operator.
///
/// In C++, binary operators (e.g. `a * 0.5`) insert an `ImplicitCastExpr`
/// (<IntegralToFloating>) on integer operands to widen them to `double`.
/// In Dart, binary arithmetic and comparison operators accept `int` and
/// `double` operands interchangeably and evaluate to `double` (or `bool`)
/// without requiring `.toDouble()`.
///
/// When `cursor` is an *implicit* promotion (`CXCursor_UnexposedExpr`) from
/// `Int` to `Double`, this function strips that redundant wrapper and lowers
/// the underlying expression directly. Explicit casts (`static_cast<double>`,
/// C-style casts, functional casts) are preserved.
unsafe fn lower_binary_operand(cursor: clang_sys::CXCursor, project_root: &Path) -> ir::Expr {
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
    if kind == clang_sys::CXCursor_UnexposedExpr {
        let outer_ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        let children = unsafe { collect_children(cursor) };
        let non_nav_children: Vec<_> = children
            .into_iter()
            .filter(|c| {
                !matches!(
                    unsafe { clang_sys::clang_getCursorKind(*c) },
                    clang_sys::CXCursor_TypeRef
                        | clang_sys::CXCursor_NamespaceRef
                        | clang_sys::CXCursor_TemplateRef
                )
            })
            .collect();
        if non_nav_children.len() == 1 {
            let child_cursor = non_nav_children[0];
            let child_ty = lower_type(unsafe { clang_sys::clang_getCursorType(child_cursor) });
            if child_ty == ir::Type::Int && outer_ty == ir::Type::Double {
                return unsafe { lower_expr(child_cursor, project_root) };
            }
        }
    }
    unsafe { lower_expr(cursor, project_root) }
}

unsafe fn lower_binary_expr(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let operator_kind = unsafe { clang_sys::clang_getCursorBinaryOperatorKind(cursor) };

    // Assignment used as an *expression* (`while ((x = foo()) != nullptr)`,
    // or the same shape reached indirectly when an intervening `libclang`
    // wrapper — e.g. a template-instantiated call's cleanup node — keeps a
    // plain-looking `x = foo();` *statement* from ever being recognized as
    // `CXCursor_BinaryOperator` at the statement level in the first place,
    // confirmed as the real Verovio trigger:
    // `adjustarticfunctor.cpp`'s `yIn = std::max(yAboveStem,
    // -staffHeight);`, which never reaches `lower_stmt`'s own
    // statement-level assignment recognition at all). `lower_binary_op`
    // has no case for `CXBinaryOperator_Assign` — it isn't a value-
    // producing operator in the same sense `+`/`==` are — so this is
    // handled on its own, mirroring `lower_assign_stmt`'s own two simple
    // target shapes (a bare local/field) exactly, before the generic
    // operator-mapping fallback below.
    if operator_kind == clang_sys::CXBinaryOperator_Assign {
        let children = unsafe { collect_children(cursor) };
        let [lhs_cursor, rhs_cursor] = children.as_slice() else {
            return ir::Expr::UnsupportedTyped {
                reason: format!(
                    "assignment expression cursor had {} children, expected 2",
                    children.len()
                ),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
                origin,
            };
        };
        let lhs_kind = unsafe { clang_sys::clang_getCursorKind(*lhs_cursor) };
        let target = if lhs_kind == clang_sys::CXCursor_DeclRefExpr {
            let name = unsafe {
                qualified_static_member_name(clang_sys::clang_getCursorReferenced(*lhs_cursor))
            };
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(*lhs_cursor) });
            Some(ir::Expr::Ref {
                name,
                ty,
                origin: origin.clone(),
            })
        } else if lhs_kind == clang_sys::CXCursor_MemberRefExpr {
            let field =
                unsafe { dart_member_name(clang_sys::clang_getCursorReferenced(*lhs_cursor)) };
            let receiver = unsafe { member_ref_receiver(*lhs_cursor, project_root, &origin) };
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(*lhs_cursor) });
            Some(ir::Expr::FieldAccess {
                target: Box::new(receiver),
                field,
                ty,
                origin: origin.clone(),
            })
        } else {
            None
        };
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        let Some(target) = target else {
            return ir::Expr::UnsupportedTyped {
                reason: "assignment-as-expression target is not a simple local variable \
                         or a field"
                    .to_owned(),
                ty,
                origin,
            };
        };
        let value = unsafe { lower_expr(*rhs_cursor, project_root) };
        // An unsupported right-hand side (e.g. `new String()` where `String`
        // isn't a known record — the real Verovio trigger, `jsonxx.h`'s
        // `*( string_value_ = new String() ) = s;`) must not be wrapped in a
        // well-formed-looking `Expr::Assign`: nothing downstream (this
        // switch/assignment-target logic, `emit::dart`) expects an
        // otherwise-valid node to hide a broken value, and the surrounding
        // dereference-as-target logic specifically relies on an
        // `Expr::Unsupported`/`UnsupportedTyped` operand being visible one
        // level up to trigger its own bailout.
        if let ir::Expr::Unsupported { reason, .. } | ir::Expr::UnsupportedTyped { reason, .. } =
            &value
        {
            return ir::Expr::UnsupportedTyped {
                reason: reason.clone(),
                ty,
                origin,
            };
        }
        return ir::Expr::Assign {
            target: Box::new(target),
            value: Box::new(value),
            ty,
            origin,
        };
    }

    let Some(op) = lower_binary_op(operator_kind) else {
        return ir::Expr::UnsupportedTyped {
            reason: format!("unsupported binary operator kind {operator_kind}"),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    };

    let children = unsafe { collect_children(cursor) };
    let [lhs_cursor, rhs_cursor] = children.as_slice() else {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "binary operator cursor had {} children, expected 2",
                children.len()
            ),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    };

    let lhs = unsafe { lower_binary_operand(*lhs_cursor, project_root) };
    let rhs = unsafe { lower_binary_operand(*rhs_cursor, project_root) };
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
    let children = unsafe { collect_children(cursor) };
    let [operand_cursor] = children.as_slice() else {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "unary operator cursor had {} children, expected 1",
                children.len()
            ),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    };

    // Unary `+` (`m_fbstates[staffindex] = +1;`, confirmed the real
    // Verovio trigger by grepping `iohumdrum.cpp` directly — an explicit-
    // positive-sign idiom) is a true no-op on an arithmetic value in both
    // C++ and Dart, unlike every other unary operator here: no promotion
    // to preserve, no sign to apply. The operand lowers directly, exactly
    // as transparent as a parenthesized wrapper.
    if operator_kind == clang_sys::CXUnaryOperator_Plus {
        return unsafe { lower_expr(*operand_cursor, project_root) };
    }

    // `*out` where `out` is a pointer-shaped out-param (round 19,
    // `pointer_out_param_bindings`/`ACTIVE_POINTER_OUT_PARAMS`): this
    // bridge already treats the parameter itself as directly holding the
    // pointee's value (its own `ir::Param.ty` is the pointee type, not
    // `Nullable`/`Unsupported`), so `*out` needs no conversion at all —
    // it *is* `out`. Checked before the general
    // `CXUnaryOperator_Deref`/`AddrOf` handling below, which keys off
    // `lower_type` on the operand's raw declared type (`int *`, still
    // `Type::Unsupported` — that rewrite only ever touched the `ir::Param`
    // list, not what this function independently re-derives from the
    // cursor) and would otherwise report the same honest-but-now-wrong
    // "dereference requires a representable nullable reference" bailout
    // for a case this module already has a precise answer for.
    let unwrapped_operand_cursor = unsafe { unwrap_transparent_value_cursor(*operand_cursor) };
    if operator_kind == clang_sys::CXUnaryOperator_Deref
        && unsafe { clang_sys::clang_getCursorKind(unwrapped_operand_cursor) }
            == clang_sys::CXCursor_DeclRefExpr
    {
        let operand_name = dart_safe_identifier(&unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(
                unwrapped_operand_cursor,
            ))
        });
        if let Some(pointee_ty) = active_pointer_out_param_type(&operand_name) {
            return ir::Expr::Ref {
                name: operand_name,
                ty: pointee_ty,
                origin,
            };
        }
    }

    // A raw pointer is represented only when its pointee is already a Dart
    // reference type (`T?`). In that narrow, statically known case C++'s
    // address-of/dereference pair has an exact Dart counterpart: taking an
    // address widens `T` to `T?`, and dereferencing is the non-null assertion
    // that C++ itself assumes at the same source location. Scalar/void
    // pointers still carry `Type::Unsupported` and remain visible bailouts.
    if operator_kind == clang_sys::CXUnaryOperator_AddrOf
        || operator_kind == clang_sys::CXUnaryOperator_Deref
    {
        let operand = unsafe { lower_expr(*operand_cursor, project_root) };
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        let operand_ty = lower_type(unsafe { clang_sys::clang_getCursorType(*operand_cursor) });
        let represented_address = operator_kind == clang_sys::CXUnaryOperator_AddrOf
            && matches!(ty, ir::Type::Nullable(_));
        let represented_dereference = operator_kind == clang_sys::CXUnaryOperator_Deref
            && matches!(operand_ty, ir::Type::Nullable(_));
        // A real crash found in the Verovio corpus itself (`json/jsonxx.h`'s
        // bundled JSON library, `*( array_value_ = new Array() ) = a;`,
        // confirmed via a full-corpus diagnosis run — not just a bailout):
        // `operand` here can already be `Expr::Unsupported`/`UnsupportedTyped`
        // (the address-of/dereference's operand is itself an unrepresentable
        // expression, e.g. an assignment used as an expression — `unsupported
        // binary operator kind 22`). Wrapping an already-unsupported operand
        // in `Expr::Convert` regardless produced a node `emit::dart`'s own
        // `Expr::Convert` renderer has no case for (its operand has no
        // statically-known type to dispatch on) — hitting emit's own
        // `unreachable!()`. Propagating the existing bailout directly, with
        // this expression's own `ty`, keeps it an honest, typed bailout
        // instead — the same "never wrap a bailout in a further
        // transformation" rule this module already follows everywhere else.
        if let ir::Expr::Unsupported { reason, .. } | ir::Expr::UnsupportedTyped { reason, .. } =
            &operand
        {
            return ir::Expr::UnsupportedTyped {
                reason: reason.clone(),
                ty,
                origin,
            };
        }
        if represented_address || represented_dereference {
            return ir::Expr::Convert {
                operand: Box::new(operand),
                ty,
                origin,
            };
        }
        return ir::Expr::UnsupportedTyped {
            reason: if operator_kind == clang_sys::CXUnaryOperator_AddrOf {
                "address-of requires a representable nullable reference".to_owned()
            } else {
                "dereference requires a representable nullable reference".to_owned()
            },
            ty,
            origin,
        };
    }

    let Some(op) = lower_unary_op(operator_kind) else {
        return ir::Expr::UnsupportedTyped {
            reason: format!("unsupported unary operator kind {operator_kind}"),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    };

    let operand = unsafe { lower_expr(*operand_cursor, project_root) };
    let operand_ty = lower_type(unsafe { clang_sys::clang_getCursorType(*operand_cursor) });
    let operand = if op == ir::UnaryOp::Not && operand_ty == ir::Type::Int {
        // `!integer` is legal C++ truthiness but invalid Dart. The unary
        // operation itself still returns `bool`; only its operand needs the
        // explicit conversion.
        ir::Expr::Convert {
            operand: Box::new(operand),
            ty: ir::Type::Bool,
            origin: origin.clone(),
        }
    } else if op == ir::UnaryOp::Not && matches!(operand_ty, ir::Type::Nullable(_)) {
        ir::Expr::Convert {
            operand: Box::new(operand),
            ty: ir::Type::Bool,
            origin: origin.clone(),
        }
    } else {
        operand
    };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    ir::Expr::Unary {
        op,
        operand: Box::new(operand),
        ty,
        origin,
    }
}

/// Whether `cursor` is `libclang`'s implicit *trivial* default-constructor
/// call — the synthetic initializer a record-typed `VarDecl` gets even when
/// C++ source writes no initializer at all (`Ponto p;`). Confirmed via
/// `clang -Xclang -ast-dump`, not guessed. `lower_decl_stmt` uses this to
/// tell "genuinely no initializer" apart from a real one, so it can emit
/// Dart's `late` instead of trying to lower a call to a constructor Dart
/// doesn't have.
///
/// E04 sharpens "implicit" to mean *has no body*, not just "zero
/// arguments": `ContaBancaria conta;` also reaches this same shape
/// (`is_default_constructor && zero args`), but `ContaBancaria`'s
/// zero-argument constructor is user-written and increments a counter — it
/// must be lowered as a real `ConstructorCall`, not silently short-circuited
/// into the "no real initializer" path the way `Ponto`'s truly-implicit one
/// should be. A trivial compiler-generated default constructor never gets a
/// `CompoundStmt` child; a user-written one, however empty, always does.
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
    let has_real_body = unsafe { constructor_has_real_body(referenced) };
    is_default && has_no_args && !has_real_body
}

/// A real, pre-existing bug found while building round 19's stringstream
/// support (not new to it — confirmed with a bare `std::string s;`
/// fixture too): `is_default_construct_with_no_args`'s `!has_real_body`
/// condition doesn't filter out a *library* type's implicit default
/// constructor (`std::string s;`, `std::stringstream ss;`) — its body is
/// never instantiated/visible to this translation unit at all, so
/// `constructor_has_real_body` can't tell "nothing to do" apart from
/// "unknown" and (conservatively, correctly for a project type) treats it
/// as if it might do real work. Left unfiltered, the constructor call
/// lowers through the generic path as `basic_string()`/
/// `basic_stringstream()` — a call to a Dart function that is never
/// generated, invalid Dart, confirmed via `dart analyze` on the resulting
/// package. For these specific known adapters, `default_scalar_value`
/// already has the *exact* real C++ zero value independent of body
/// inspection (`""` for a string — C++ guarantees a default-constructed
/// `std::string` is empty, not "unspecified" the way a bare scalar is) —
/// so unlike the general case, no body inspection is needed at all here.
unsafe fn is_default_construct_of_a_known_adapter_type(cursor: clang_sys::CXCursor) -> bool {
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_CallExpr {
        return false;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0
        || unsafe { clang_sys::clang_getCursorKind(referenced) } != clang_sys::CXCursor_Constructor
        || unsafe { clang_sys::clang_CXXConstructor_isDefaultConstructor(referenced) } == 0
        || unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) } != 0
    {
        return false;
    }
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    matches!(
        unsafe { stdlib_template_name(owner) }.as_deref(),
        Some("basic_string") | Some("basic_stringstream") | Some("basic_ostringstream")
    )
}

/// Whether `constructor`'s `CompoundStmt` actually contains a statement.
/// `find_compound_stmt_child(cursor).is_some()` alone isn't enough to tell a
/// user-written body apart from a compiler-implicit one: confirmed
/// empirically (not assumed) that `libclang` still synthesizes an *empty*
/// `CompoundStmt` child for a purely-implicit trivial default constructor —
/// `clang -Xclang -ast-dump`'s pretty-printer just doesn't bother rendering
/// it, which is what made this look body-less at first. An explicitly
/// user-written `Ponto() {}` would (correctly) also count as having no real
/// body here — semantically it does nothing either, so treating it the same
/// as the fully-implicit case is the right answer, not a coincidence of the
/// test.
unsafe fn constructor_has_real_body(constructor: clang_sys::CXCursor) -> bool {
    match unsafe { find_compound_stmt_child(constructor) } {
        Some(compound) => !unsafe { collect_children(compound) }.is_empty(),
        None => false,
    }
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
        return ir::Expr::UnsupportedTyped {
            reason: "call target could not be resolved".to_owned(),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
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
        // A `__gnu_cxx::__normal_iterator` value materializes through one of
        // *several* constructors, not just copy/move (real trigger:
        // `floatingobject.cpp:160`'s `auto it = std::find(...)`, whose
        // implicit materialization resolves to the templated
        // pointer-to-iterator converting constructor, which
        // `clang_CXXConstructor_isCopyConstructor`/`isMoveConstructor` both
        // report `false` for) — falling through to the generic
        // `lower_constructor_call` path below, like `basic_string`'s own
        // converting constructor two checks down, would build an
        // `Expr::ConstructorCall` naming `__normal_iterator`, a class this
        // project never `lower_record`'d, printing a literal call to a
        // Dart-undefined identifier (`__normal_iterator(...)`, confirmed the
        // hard way on the real Verovio 6.2.0 corpus — F10/tarefa 13's own
        // `undefined_method` regression, exactly the "silêncio é proibido"
        // failure this whole family exists to prevent). Every one of this
        // class's constructors takes exactly one argument (the value being
        // wrapped/converted), so the same transparent unwrap `is_copy_or_move`
        // uses above is exact here too, regardless of which specific
        // overload resolved; a shape with a different argument count (none
        // observed, but not proven impossible) falls to an honest typed
        // bailout instead of guessing.
        if unsafe { is_normal_iterator_decl(clang_sys::clang_getCursorSemanticParent(referenced)) }
        {
            if arg_count == 1 {
                let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
                return unsafe { lower_expr(arg_cursor, project_root) };
            }
            return ir::Expr::UnsupportedTyped {
                reason: "unsupported __normal_iterator construction shape".to_owned(),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
                origin,
            };
        }
        // `return "Au au";` from a function returning `std::string` (E06)
        // implicitly invokes `basic_string`'s converting constructor from
        // `const char*` — a `CXXConstructExpr`, surfacing here exactly like
        // any other constructor call. Without this check it fell into the
        // real-constructor path below and tried to build an
        // `Expr::ConstructorCall` naming a `basic_string` that was never
        // `lower_record`'d (E05 deliberately never does — see
        // `Type::Str`'s doc comment), producing `basic_string(...)` — Dart
        // has no such function. The C string literal argument already
        // lowers to `Expr::StringLiteral` on its own (`Type::Str` is what
        // that literal always was, in Dart terms); this just recurses into
        // it directly, the same transparent treatment `Type::Str`'s literal
        // already gets when the compiler wraps it via `UnexposedExpr`
        // instead of a full constructor call.
        let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
        let owner_template_name = unsafe { stdlib_template_name(owner) }.or_else(|| unsafe {
            let is_system_constructor = clang_sys::clang_Location_isInSystemHeader(
                clang_sys::clang_getCursorLocation(referenced),
            ) != 0;
            let owner_name =
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(owner));
            (is_system_constructor && owner_name == "optional").then_some(owner_name)
        });
        // `arg_count` is 2, not 1 (confirmed empirically) — the compiler
        // materializes the constructor's defaulted `allocator` parameter as
        // an explicit second argument instead of omitting it. Only the
        // first argument (the actual string content) matters for the
        // transparent passthrough; the allocator argument carries no
        // information `Type::Str` needs to represent.
        if arg_count >= 1 && owner_template_name.as_deref() == Some("basic_string") {
            let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
            return unsafe { lower_expr(arg_cursor, project_root) };
        }
        // `std::string()` (F6/tarefa 07: real Verovio trigger —
        // `jsonxx.cc`'s `const std::string &attr = std::string()` default
        // parameter value) — the default constructor, zero real arguments
        // (confirmed empirically: unlike the converting-constructor case
        // just above, there's no defaulted-allocator argument here at all).
        // `basic_string` was never `lower_record`'d (`Type::Str`'s own doc
        // comment), so falling through to the generic constructor-call path
        // below would name a Dart function that doesn't exist —
        // `basic_string()`, `dart analyze`'s `undefined_function`. An empty
        // Dart string is the exact same value a default-constructed
        // `std::string` already has.
        if arg_count == 0 && owner_template_name.as_deref() == Some("basic_string") {
            return ir::Expr::StringLiteral {
                value: String::new(),
                origin,
            };
        }
        // A Dart nullable value directly models an engaged optional. The
        // constructor has no remaining runtime identity after the type
        // boundary has become T?, so retain just its payload.
        if owner_template_name.as_deref() == Some("optional") {
            if arg_count == 0 {
                return ir::Expr::NullLiteral { origin };
            }
            let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
            return unsafe { lower_expr(arg_cursor, project_root) };
        }
        // `std::vector<int> v = {1, 2, 3}` invokes `vector`'s
        // `initializer_list` constructor — a real `CXXConstructExpr`, same
        // as any other constructor call, but `vector` was never
        // `lower_record`'d (it maps to `Type::List`, the same reasoning as
        // `basic_string`/`optional` above), so falling into the generic
        // path below would name a Dart function that doesn't exist,
        // `vector(<int>[1, 2, 3])`. When the single argument is itself an
        // initializer list, the constructor call carries no information
        // beyond that list's own contents — recurse directly into it
        // (`lower_expr` already turns the `InitListExpr` into
        // `Expr::ListLiteral` when its type resolves to `List<T>`).
        if arg_count >= 1
            && matches!(
                owner_template_name.as_deref(),
                Some("vector") | Some("array") | Some("deque") | Some("initializer_list")
            )
        {
            let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
            // `clang_Cursor_getArgument`'s cursor is an `UnexposedExpr`
            // wrapping the real `InitListExpr` (confirmed empirically, not
            // assumed — arg count 2 for `vector`'s `initializer_list`
            // constructor, the same defaulted-allocator surprise already
            // documented above for `basic_string`), so this can't check the
            // argument's own cursor kind directly the way the
            // `basic_string`/`optional` cases above do. Lowering it and
            // checking the *result* shape instead reuses `lower_expr`'s own
            // wrapper-unwrapping (`is_transparent_wrapper`) rather than
            // duplicating it, and stays safe for every constructor shape
            // this doesn't apply to (size+fill, iterator-range, copy): those
            // lower to something other than `Expr::ListLiteral`, so they
            // fall through to the generic (still-explicit) path below
            // unchanged.
            if let list_literal @ ir::Expr::ListLiteral { .. } =
                unsafe { lower_expr(arg_cursor, project_root) }
            {
                return list_literal;
            }
        }
        // `std::map<K, V> m{ {k1, v1}, {k2, v2} };` (also `unordered_map`) —
        // real Verovio trigger: static const lookup tables declared this
        // way throughout the corpus (`midifunctor.cpp`, `iocmme.cpp`, ...).
        // Unlike `vector`'s flat element list, `map`'s `initializer_list`
        // wraps a `std::pair<const K, V>[N]` array whose own elements are
        // each `{ key, value }` — libclang reports each pair entry's own
        // cursor kind as `CXCursor_InitListExpr` too (confirmed empirically:
        // its coarser cursor API never surfaces the implicit `std::pair`
        // `CXXConstructExpr` the full internal AST wraps each entry in), so
        // this can't reuse the `ListLiteral`-shaped check above (whose items
        // are plain values, not nested 2-element lists) and instead walks
        // the pair array directly — see `map_literal_entries`.
        if arg_count >= 1
            && matches!(
                owner_template_name.as_deref(),
                Some("map") | Some("unordered_map")
            )
            && let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) })
            && let ir::Type::Map(key_ty, _) = &ty
        {
            let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
            let init_list_cursor = unsafe { unwrap_transparent_value_cursor(arg_cursor) };
            if unsafe { clang_sys::clang_getCursorKind(init_list_cursor) }
                == clang_sys::CXCursor_InitListExpr
                && let Some(entries) =
                    unsafe { map_literal_entries(init_list_cursor, key_ty, project_root) }
            {
                return ir::Expr::MapLiteral {
                    entries,
                    ty,
                    origin,
                };
            }
        }
        // `std::pair<A, B> p(a, b);` / `std::pair<A, B>(a, b)` — the
        // constructor counterpart to `lower_stdlib_free_function_call`'s
        // `make_pair` case above: same Dart target (`SyntaxBridgePair`,
        // `Type::Pair`'s own representation — see the `PointeeShape::Known`
        // table above), just reached via direct construction syntax instead
        // of the free function. `pair` has no defaulted-allocator argument
        // the way `basic_string`/`vector` do (confirmed empirically), so
        // `arg_count == 2` is the exact real-argument count, not merely a
        // floor the way it is for those two.
        if arg_count == 2
            && owner_template_name.as_deref() == Some("pair")
            && let ir::Type::Pair(_, _) =
                lower_type(unsafe { clang_sys::clang_getCursorType(cursor) })
        {
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
            let first_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
            let second_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 1) };
            let first = unsafe { lower_expr(first_cursor, project_root) };
            let second = unsafe { lower_expr(second_cursor, project_root) };
            return ir::Expr::Call {
                base_qualifier: None,
                target: None,
                callee_usr: unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced))
                },
                // Must read the same literal name as `emit::dart::PAIR_TYPE_NAME`.
                callee_name: "SyntaxBridgePair".to_owned(),
                args: vec![first, second],
                ty,
                origin,
            };
        }
        // A real (non-copy/move) constructor call — E04. `lower_decl_stmt`
        // already routes the *trivial implicit* default constructor (no
        // user body) to `default_record_construct` before this function is
        // ever reached (`is_default_construct_with_no_args`), so getting
        // here with a real constructor means it has a body, and belongs in
        // `Record::constructors` — see `Expr::ConstructorCall`'s docs for
        // why identity is the ordinal, not the name.
        return unsafe { lower_constructor_call(cursor, referenced, project_root, origin) };
    }

    if referenced_kind == clang_sys::CXCursor_CXXMethod {
        if let Some(special) =
            unsafe { lower_stdlib_method_call(cursor, referenced, project_root, &origin) }
        {
            return special;
        }
        // `vrv::Fraction::Reduce(a, b);` (E13) — a static method called by
        // qualified name from outside its class has no receiver at all:
        // `libclang` shapes the call exactly like a free function call
        // (callee reference, then the real arguments, confirmed via
        // `clang -Xclang -ast-dump`), so it's lowered the same way rather
        // than through `lower_method_call`, which always expects a
        // receiver.
        if unsafe { clang_sys::clang_CXXMethod_isStatic(referenced) } != 0 {
            return unsafe { lower_static_method_call(cursor, referenced, project_root, origin) };
        }
        // `a + b`/`a == b` (E13) reaching a *user* Record's own operator
        // method — US-7's `mapping::operator_option` already decides a
        // singleton operator on Dart's native symbol list maps *directly*
        // (case `"operador-direto"`); this is where that decision is
        // actually carried out. Checked before `lower_method_call`, which
        // would otherwise emit `a.operator+(b)` — syntactically invalid
        // Dart (`dart analyze`: `undefined_getter` on `operator`, confirmed
        // empirically): an overloaded operator can only be invoked with
        // the operator's own syntax, never as an ordinary method call.
        let callee_name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced))
        };
        if let Some(operator_expr) =
            unsafe { lower_record_operator_call(cursor, &callee_name, project_root, &origin) }
        {
            return operator_expr;
        }
        return unsafe { lower_method_call(cursor, referenced, project_root, origin) };
    }

    // `std::string s = token;` / `t.operator std::string()` — a C++
    // user-defined conversion operator, called explicitly or (far more
    // often in the real Verovio corpus, via `HumdrumToken`) implicitly
    // wherever the target type is expected. Confirmed via `clang++ -Xclang
    // -ast-dump` (not assumed): the implicit form already inserts a real
    // `CXXMemberCallExpr` referencing the `CXXConversionDecl` — the exact
    // same shape an explicit method call has, just wrapped in an outer
    // `ImplicitCastExpr <UserDefinedConversion>` that `lower_expr`'s
    // transparent-wrapper unwrapping (`is_transparent_wrapper`) already
    // passes through unchanged whenever the wrapper and its child agree on
    // type — which they always do here, since the call's own type already
    // *is* the conversion's target type. So fixing the call itself, below,
    // fixes both call forms with the same code. Scoped to the target types
    // `conversion_operator_dart_method_name` understands (`Str` —
    // `HumdrumToken`'s conversion to `std::string`, ~750 combined
    // occurrences in the 2026-08-20 diagnosis; `Bool` — an explicit-
    // truthiness wrapper idiom): any other conversion target (a numeric
    // type, another record) stays an honest, explicit bailout rather than
    // guessing a Dart name or semantics this hasn't been verified for.
    if referenced_kind == clang_sys::CXCursor_ConversionFunction {
        let target_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(referenced) });
        if conversion_operator_dart_method_name(&target_type).is_some() {
            return unsafe { lower_method_call(cursor, referenced, project_root, origin) };
        }
        return ir::Expr::UnsupportedTyped {
            reason: format!("unsupported conversion operator target: {target_type:?}"),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    }

    // `m_callback(value)` / `this->m_callback(value)` (a callback *field*),
    // `cb(value)` (a callback *parameter*), or a callback held in a local
    // *variable* — all three resolve their call target to the declaration
    // holding the value, not a `FunctionDecl`, since the value itself (not
    // some named function) is what's being invoked. When that declaration's
    // own type already lowers to a representable `Type::Callback` (a real
    // C function pointer, not an opaque `void*`/ABI callback), Dart's own
    // "call whatever this identifier resolves to" syntax needs no adapter:
    // `field(args)`/`cb(args)` is already valid Dart, the same as any other
    // implicit-`this` field access already emits with no `this.` prefix.
    if matches!(
        referenced_kind,
        clang_sys::CXCursor_FieldDecl | clang_sys::CXCursor_VarDecl | clang_sys::CXCursor_ParmDecl
    ) {
        let declared_type = lower_type(unsafe { clang_sys::clang_getCursorType(referenced) });
        if let ir::Type::Callback { return_type, .. } = declared_type {
            return unsafe {
                lower_callable_value_call(
                    cursor,
                    referenced,
                    referenced_kind,
                    *return_type,
                    project_root,
                    origin,
                )
            };
        }
    }

    if referenced_kind != clang_sys::CXCursor_FunctionDecl {
        return ir::Expr::UnsupportedTyped {
            reason: format!(
                "unsupported call target cursor kind {referenced_kind} \
                 (only free functions, methods and constructors are lowered as calls so far)"
            ),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    }

    let callee_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced)) };
    if callee_usr.is_empty() {
        return ir::Expr::UnsupportedTyped {
            reason: "resolved call target has no stable identity".to_owned(),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    }
    // A call resolving to *any* instantiation of a function template — full
    // explicit specialization or implicit — reports a real (non-null)
    // `clang_getSpecializedCursorTemplate` (confirmed empirically, kind
    // `CXCursor_FunctionTemplate`; an ordinary function's is null, kind
    // `CXCursor_InvalidFile`). E08's monomorphization name
    // (`monomorphized_template_name`) is applied here too, not just at the
    // declaration this resolves to — see that function's doc comment for
    // why computing it independently, the same deterministic way, at every
    // site that needs it is what keeps them from ever disagreeing.
    //
    // Gated to a *user* template (`!is_in_system_header`): `std::string`'s
    // own `operator+`/`operator==` are themselves function templates in
    // `libstdc++`, so an unguarded check here would rename them before
    // `lower_stdlib_operator_call` below ever gets a chance to recognize
    // `callee_name == "operator+"` — confirmed the hard way, as a
    // regression across every E05 fixture, not anticipated up front.
    let specialized_template = unsafe { clang_sys::clang_getSpecializedCursorTemplate(referenced) };
    let is_user_template_instantiation = unsafe {
        clang_sys::clang_Cursor_isNull(specialized_template) == 0
            && clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
                referenced,
            )) == 0
    };
    let callee_name = if is_user_template_instantiation {
        let base_name = unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(
                specialized_template,
            ))
        };
        monomorphized_template_name(&base_name, referenced)
    } else {
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) }
    };

    if let Some(special) = unsafe {
        lower_stdlib_operator_call(cursor, referenced, &callee_name, project_root, &origin)
    } {
        return special;
    }
    if let Some(special) = unsafe {
        lower_stdlib_free_function_call(cursor, referenced, &callee_name, project_root, &origin)
    } {
        return special;
    }
    if let Some(special) = unsafe {
        lower_stdlib_algorithm_call(cursor, referenced, &callee_name, project_root, &origin)
    } {
        return special;
    }
    if matches!(callee_name.as_str(), "operator!=" | "operator==")
        && unsafe {
            clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
                referenced,
            ))
        } != 0
        && unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) } == 2
        && let lhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) }
        && let rhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 1) }
        && let Some(contains_expr) = unsafe {
            lower_find_contains_idiom(
                callee_name == "operator!=",
                lhs_cursor,
                rhs_cursor,
                project_root,
                &origin,
            )
        }
    {
        return contains_expr;
    }

    // Dart only permits operators as instance methods, whereas C++ commonly
    // declares them as free functions. The declaration emitter gives those
    // functions a stable ordinary helper name; preserve the call graph by
    // using precisely the same name here instead of replacing a valid body
    // with an opaque expression. System operators remain the responsibility
    // of their own library adapters above — emitting an unimported helper for
    // one would only hide a missing mapping.
    if callee_name.starts_with("operator")
        && unsafe {
            clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
                referenced,
            ))
        } == 0
    {
        let args = match unsafe { lower_call_arguments(cursor, project_root) } {
            Some(args) => args,
            None => {
                return ir::Expr::UnsupportedTyped {
                    reason: "could not enumerate free operator arguments".to_owned(),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
                    origin,
                };
            }
        };
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
        return ir::Expr::Call {
            base_qualifier: None,
            target: None,
            callee_usr,
            callee_name: dart_operator_bridge_name(&callee_name, args.len()).to_owned(),
            args,
            ty,
            origin,
        };
    }

    // Every free operator overload this function actually knows how to
    // translate is handled by one of the two `lower_stdlib_*` calls above;
    // reaching here with a `callee_name` that isn't a plain identifier means
    // an operator neither one recognizes (`operator<<`, or C++20's `<=>`
    // rewritten-candidate machinery calling `std::__cmp_cat`'s `operator<`,
    // both confirmed on the real Verovio 6.2.0 corpus). `emit::dart` prints
    // a `Call`'s `callee_name` as a bare identifier — `operator<<(a, 2)` —
    // which `dart format` rejects outright, so this must bail out here
    // rather than build a `Call` no emitter step downstream could catch.
    if !is_plain_dart_identifier(&callee_name) {
        return ir::Expr::UnsupportedTyped {
            reason: format!("unsupported free operator overload: {callee_name}"),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
            origin,
        };
    }

    let args = match unsafe { lower_call_arguments(cursor, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::UnsupportedTyped {
                reason: "could not enumerate call arguments".to_owned(),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(cursor) }),
                origin,
            };
        }
    };
    let args = unsafe { regroup_variadic_call_args(args, referenced, &origin) };

    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    ir::Expr::Call {
        base_qualifier: None,
        target: None,
        callee_usr,
        callee_name,
        args,
        ty,
        origin,
    }
}

/// `m_callback(value)` / `this->m_callback(value)` / `cb(value)` — a call
/// whose target is a field/variable/parameter *holding* a callback value,
/// not a named function. `callee_usr` has no real meaning here (there's no
/// function declaration to cross-reference — the callable is a value, like
/// any other expression), so it stays empty, the same convention already
/// used for the synthetic `List.empty`/`Set.empty` default-value calls.
unsafe fn lower_callable_value_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    referenced_kind: clang_sys::CXCursorKind,
    return_type: ir::Type,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let args = match unsafe { lower_call_arguments(call_cursor, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::UnsupportedTyped {
                reason: "could not enumerate callback call arguments".to_owned(),
                ty: return_type,
                origin,
            };
        }
    };

    let target = if referenced_kind == clang_sys::CXCursor_FieldDecl {
        let receiver_children = unsafe { collect_children(call_cursor) };
        // The call's own callee sub-expression (`m_callback`, before the
        // trailing `(...)`) is itself wrapped in an `UnexposedExpr`
        // (`libclang`'s lvalue-to-rvalue "load" of the field's `Function`
        // value) — the same wrapper `unwrap_transparent_value_cursor`
        // already exists to see through, confirmed empirically here rather
        // than assumed like every other such site in this module.
        let first_child_value = receiver_children
            .first()
            .map(|first_child| unsafe { unwrap_transparent_value_cursor(*first_child) });
        match first_child_value {
            Some(value_cursor)
                if unsafe { clang_sys::clang_getCursorKind(value_cursor) }
                    == clang_sys::CXCursor_MemberRefExpr =>
            {
                Some(Box::new(unsafe {
                    member_ref_receiver(value_cursor, project_root, &origin)
                }))
            }
            _ => {
                return ir::Expr::UnsupportedTyped {
                    reason: "callback field call had no member-reference receiver".to_owned(),
                    ty: return_type,
                    origin,
                };
            }
        }
    } else {
        None
    };

    ir::Expr::Call {
        base_qualifier: None,
        target,
        callee_usr: String::new(),
        callee_name: unsafe { dart_member_name(referenced) },
        args,
        ty: return_type,
        origin,
    }
}

/// Whether `name` could ever be printed as a bare Dart call target
/// (`{name}(args)`) — a real identifier, not an operator token like
/// `operator<<` or `operator<=>`. Used as the last-resort guard on every
/// generic `Call`-construction fallback in this module: a call target this
/// rejects has no valid literal spelling in Dart, so it must become
/// `Expr::Unsupported` instead of a `Call` `emit::dart` would print verbatim.
pub(crate) fn is_plain_dart_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Stable Dart helper name for a C++ operator that cannot be declared as a
/// free-standing Dart operator. Shared with the emitter so every declaration
/// and call site keeps the same target without an ad-hoc per-project mapping.
pub(crate) fn dart_operator_bridge_name(operator_name: &str, arity: usize) -> &'static str {
    let symbol = operator_name
        .strip_prefix("operator")
        .unwrap_or(operator_name);
    match symbol {
        "<<" => "streamInsert",
        ">>" => "streamExtract",
        "->" => "arrow",
        "!" => "logicalNot",
        "~" => "bitwiseNot",
        "=" => "assignFrom",
        "++" if arity == 0 => "increment",
        "++" => "incrementPostfix",
        "--" if arity == 0 => "decrement",
        "--" => "decrementPostfix",
        "+=" => "addAssign",
        "-=" => "subtractAssign",
        "*=" => "multiplyAssign",
        "/=" => "divideAssign",
        "%=" => "moduloAssign",
        "%" => "modulo",
        "&" => "bitwiseAnd",
        "|" => "bitwiseOr",
        "^" => "bitwiseXor",
        "&&" => "logicalAnd",
        "||" => "logicalOr",
        "," => "comma",
        _ => "unsupportedOperator",
    }
}

/// Lowers every argument of a call cursor (`clang_Cursor_getArgument`
/// already excludes the callee reference itself, whether that's a bare
/// function name or, for a method call, the `MemberRefExpr` — see
/// `lower_call_expr`'s own doc comment on that API). Shared by free
/// function, method and constructor calls; `None` only when `libclang`
/// can't enumerate the arguments at all (negative count), never for zero
/// arguments.
///
/// `incrementar(10)` (E07, one argument omitted, using `passo`'s C++
/// default) still reports `clang_Cursor_getNumArguments == 2` — the omitted
/// argument is materialized as its own cursor at the call site rather than
/// simply not existing. Confirmed empirically (not assumed): that cursor is
/// `CXCursor_UnexposedExpr` with **zero** children, unlike every other
/// `UnexposedExpr` sugar this module unwraps, which always has exactly one
/// (`is_transparent_wrapper`'s own doc comment). Filtered out here, before
/// `lower_expr` ever sees it and reports "did not have exactly one child" —
/// the Dart-side call simply omits the same trailing argument, which is
/// correct on its own: the parameter is already emitted as an *optional*
/// Dart parameter with the identical default value (`ir::Param::default_value`),
/// so Dart supplies it exactly as C++ did.
unsafe fn lower_call_arguments(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
) -> Option<Vec<ir::Expr>> {
    unsafe { lower_call_arguments_skipping(cursor, 0, project_root) }
}

/// `lower_call_arguments`, skipping the first `skip` raw arguments before
/// lowering the rest — `lower_method_call` needs `skip = 1` for a
/// `CXXOperatorCallExpr` (`a == b`), whose own first argument is the
/// receiver (`a`), not a real call argument (see that function's doc
/// comment).
unsafe fn lower_call_arguments_skipping(
    cursor: clang_sys::CXCursor,
    skip: c_uint,
    project_root: &Path,
) -> Option<Vec<ir::Expr>> {
    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) };
    if arg_count < 0 || (arg_count as c_uint) < skip {
        return None;
    }
    Some(
        (skip..arg_count as c_uint)
            .map(|index| unsafe { clang_sys::clang_Cursor_getArgument(cursor, index) })
            .filter(|arg_cursor| {
                !(unsafe { clang_sys::clang_getCursorKind(*arg_cursor) }
                    == clang_sys::CXCursor_UnexposedExpr
                    && unsafe { collect_children(*arg_cursor) }.is_empty())
            })
            .map(|arg_cursor| unsafe { lower_expr(arg_cursor, project_root) })
            .collect(),
    )
}

/// A call to a method whose owning class is a recognized C++ standard-
/// library type (`std::basic_string`/`std::vector` so far — see
/// `stdlib_template_name`) — `.size()`, `operator[]`, and any other member
/// these Dart adapters expose need their own translation instead of the
/// generic `obj.method(args)` shape `lower_method_call` builds (Dart's
/// `String`/`List` don't have a `.size()` method, and `String.length`
/// counts UTF-16 code units where C++ counts UTF-8 bytes — see
/// `Expr::StringByteLength`). `None` only when the receiver *isn't* one of
/// these library types at all — anything recognized as one but not
/// otherwise handled here comes back `Some(Expr::Unsupported)` rather than
/// falling through to the generic method-call path, which would silently
/// emit Dart that compiles but calls a method that doesn't exist ("silêncio
/// é proibido").
/// The two global streams this narrow bridge understands as a genuine 1:1
/// mapping to a real Dart output sink — `std::cout` to `print` (real
/// process stdout, no import needed) and `std::cerr` to `dart:io`'s
/// `stderr` (grepping the Verovio source directly shows `std::cerr` is
/// actually the *more* common of the two here, 231 vs. 68 occurrences —
/// almost always the same warning/error-then-newline shape `std::cout`'s
/// own idiom has). `std::clog` and any other `std::ostream` stay bailout —
/// this bridge only names the two *global* streams, never a stand-in for
/// an arbitrary stream value.
#[derive(Clone, Copy, PartialEq)]
enum KnownOstream {
    Cout,
    Cerr,
}

unsafe fn known_ostream_global(cursor: clang_sys::CXCursor) -> Option<KnownOstream> {
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_DeclRefExpr {
        return None;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    let is_std = unsafe {
        clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(referenced))
    } != 0;
    if !is_std {
        return None;
    }
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    match name.as_str() {
        "cout" => Some(KnownOstream::Cout),
        "cerr" => Some(KnownOstream::Cerr),
        _ => None,
    }
}

/// Whether `cursor` (after unwrapping the function-to-pointer decay
/// `std::endl`'s manipulator-function reference goes through) is
/// `std::endl` — the only manipulator `lower_cout_insertion_chain`
/// recognizes, since it's the one case where Dart's `print` (which always
/// appends a newline) is an exact semantic match.
unsafe fn is_std_endl(cursor: clang_sys::CXCursor) -> bool {
    let value_cursor = unsafe { unwrap_transparent_value_cursor(cursor) };
    if unsafe { clang_sys::clang_getCursorKind(value_cursor) } != clang_sys::CXCursor_DeclRefExpr {
        return false;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(value_cursor) };
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    name == "endl"
        && unsafe {
            clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
                referenced,
            ))
        } != 0
}

/// Walks a chain of `std::cout << a << b << ...` insertions — left-
/// associative in the AST (`((cout << a) << b) << ...`, confirmed via
/// `clang -Xclang -ast-dump`, the same operator-syntax shape `operator[]`'s
/// own comment on this function's caller documents: receiver as argument
/// 0, value as argument 1, no `MemberRefExpr`) — collecting each inserted
/// value as a Dart string-producing expression, in left-to-right order,
/// alongside which of the two known streams the chain bottoms out at.
/// `None` when the chain doesn't bottom out at `std::cout`/`std::cerr`
/// (some other/unknown stream) or when any operand's type isn't one
/// `.toString()` is trusted to print the same way `operator<<`'s built-in
/// overloads do (`Str`/`Int`/`Double` — `Bool` is deliberately excluded:
/// C++'s default `operator<<(bool)` prints `0`/`1`, not Dart's
/// `"true"`/`"false"`, and this bridge has no way to know whether
/// `std::boolalpha` was set at this call site; a `Record`/`Enum` operand
/// might have its own custom `operator<<` overload this bridge can't
/// replicate).
unsafe fn lower_ostream_insertion_chain(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<(KnownOstream, Vec<ir::Expr>)> {
    if let Some(stream) = unsafe { known_ostream_global(cursor) } {
        return Some((stream, Vec::new()));
    }
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_CallExpr {
        return None;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    let spelling =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    if spelling != "operator<<" {
        return None;
    }
    // `owner << const char*` (a plain string literal insertion) resolves to
    // the *free* template `std::operator<<(basic_ostream&, const char*)`,
    // not a `basic_ostream` member — confirmed empirically (not assumed):
    // checking `referenced`'s owning class the way `operator[]` above does
    // fails for exactly this case, since there is no owning class to find.
    // The call's own *result type*, though, is always `basic_ostream<char>`
    // for a valid insertion regardless of which overload resolved it — but
    // it spells as libstdc++'s internal `__ostream_type` typedef alias, not
    // the record type directly (also confirmed empirically), and
    // `clang_getTypeDeclaration` on a typedef resolves to the `TypedefDecl`
    // itself, which `stdlib_template_name` correctly reports as "not a
    // template specialization" (it isn't one). `clang_getCanonicalType`
    // desugars through the typedef to the real `basic_ostream<char>`
    // specialization first, the same way this module already desugars
    // elsewhere before checking a type's identity — and checking the
    // *result* type instead of the *receiver*'s owning class covers both
    // the member and free-function overload forms with one condition.
    let call_type =
        unsafe { clang_sys::clang_getCanonicalType(clang_sys::clang_getCursorType(cursor)) };
    if unsafe { stdlib_template_name_of_type(call_type) }.as_deref() != Some("basic_ostream") {
        return None;
    }
    if unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) } != 2 {
        return None;
    }
    let receiver_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
    let value_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 1) };
    let (stream, mut pieces) =
        unsafe { lower_ostream_insertion_chain(receiver_cursor, project_root, origin) }?;
    let value = unsafe { lower_expr(value_cursor, project_root) };
    // A `"literal"` argument's own `libclang` type is `const char[N]`, not
    // `Type::Str` — `lower_expr` already special-cases the cursor kind
    // directly to produce `Expr::StringLiteral` (confirmed empirically, the
    // same trap `Expr::StringLiteral`'s own doc comment documents for the
    // C-string surface generally), so a lowered string literal is checked
    // by its own shape here instead of re-deriving the type from the
    // cursor; every other operand still goes through the cursor's type.
    let piece = if matches!(value, ir::Expr::StringLiteral { .. }) {
        value
    } else {
        match lower_type(unsafe { clang_sys::clang_getCursorType(value_cursor) }) {
            ir::Type::Str => value,
            ir::Type::Int | ir::Type::Double => ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(value)),
                callee_usr: String::new(),
                callee_name: "toString".to_owned(),
                args: Vec::new(),
                ty: ir::Type::Str,
                origin: origin.clone(),
            },
            _ => return None,
        }
    };
    pieces.push(piece);
    Some((stream, pieces))
}

/// Whether `cursor` is a `DeclRefExpr` to a local `std::stringstream`/
/// `std::ostringstream` variable — the base case
/// `lower_stringstream_insertion_chain` bottoms out at, mirroring
/// `known_ostream_global`'s role for the `std::cout`/`std::cerr` chain.
/// Checked against the variable's own *declared* clang type
/// (`stdlib_template_name_of_type`), not `lower_type`'s result: a
/// stringstream variable already lowers to `Type::Str` (this bridge models
/// it *as* the accumulated string directly), which would be
/// indistinguishable from an ordinary `std::string` local otherwise.
unsafe fn stringstream_variable_name(cursor: clang_sys::CXCursor) -> Option<String> {
    // `ss`, as the receiver of `operator<<`, is always reached through an
    // implicit `DerivedToBase` cast (`basic_iostream` → `basic_ostream`,
    // confirmed via `-ast-dump`: `operator<<` takes `basic_ostream&`, and
    // `stringstream`'s own inheritance chain is `basic_stringstream` →
    // `basic_iostream` → `basic_ostream`) — unlike `std::cout`/`std::cerr`,
    // whose global objects genuinely *are* `basic_ostream` already, no cast
    // needed. `unwrap_transparent_value_cursor` strips it before the
    // `DeclRefExpr` check below, the same wrapper class this module
    // already unwraps in half a dozen other spots.
    let cursor = unsafe { unwrap_transparent_value_cursor(cursor) };
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_DeclRefExpr {
        return None;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    // `std::stringstream` is itself a typedef for `basic_stringstream<char>`
    // (the same shape `std::string`/`basic_string<char>` has, and the same
    // trap `lower_ostream_insertion_chain`'s own `call_type` already
    // canonicalizes for): `clang_getTypeDeclaration` on the typedef itself
    // resolves to the `TypedefDecl`, which `stdlib_template_name` correctly
    // reports as "not a template specialization" — so this needs the
    // canonical type first, or every real `std::stringstream` variable
    // would silently never match.
    let declared_type =
        unsafe { clang_sys::clang_getCanonicalType(clang_sys::clang_getCursorType(referenced)) };
    match unsafe { stdlib_template_name_of_type(declared_type) }.as_deref() {
        Some("basic_stringstream") | Some("basic_ostringstream") => {}
        _ => return None,
    }
    Some(dart_safe_identifier(&unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced))
    }))
}

/// `ss << a << b << ...;` — the `std::stringstream`/`std::ostringstream`
/// accumulator idiom (round 19, real trigger `options.cpp`'s
/// `OptionArray::GetStr`: `ss << "\"" << value << "\"";` inside a loop,
/// then `return ss.str();`). Structurally the *same* left-associative
/// chain `lower_ostream_insertion_chain` already walks for `std::cout`/
/// `std::cerr` (same operator-syntax shape, same
/// `stdlib_template_name_of_type(call_type) == "basic_ostream"` check —
/// `basic_stringstream` inherits `basic_ostream`'s `operator<<`, so a
/// chain ending at either resolves through the exact same overloads) —
/// duplicated rather than sharing one function with
/// `lower_ostream_insertion_chain`, since the two differ in exactly the
/// one thing a shared helper would need a parameter for anyway (the base
/// case: a known global stream object vs. a local variable's own name) and
/// in what they do with the result (`print`/`stderr.writeln` vs. a
/// self-reassignment). No `std::endl`/manipulator requirement here, unlike
/// the `std::cout` chain: a stringstream accumulates across many separate
/// statements, not one terminal flush, so this fires for *every* insertion
/// chain into a recognized stringstream variable, used as its own
/// statement.
unsafe fn lower_stringstream_insertion_chain(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<(String, Vec<ir::Expr>)> {
    if let Some(name) = unsafe { stringstream_variable_name(cursor) } {
        return Some((name, Vec::new()));
    }
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_CallExpr {
        return None;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    let spelling =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    if spelling != "operator<<" {
        return None;
    }
    let call_type =
        unsafe { clang_sys::clang_getCanonicalType(clang_sys::clang_getCursorType(cursor)) };
    if unsafe { stdlib_template_name_of_type(call_type) }.as_deref() != Some("basic_ostream") {
        return None;
    }
    if unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) } != 2 {
        return None;
    }
    let receiver_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
    let value_cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 1) };
    let (name, mut pieces) =
        unsafe { lower_stringstream_insertion_chain(receiver_cursor, project_root, origin) }?;
    let value = unsafe { lower_expr(value_cursor, project_root) };
    let piece = if matches!(value, ir::Expr::StringLiteral { .. }) {
        value
    } else {
        match lower_type(unsafe { clang_sys::clang_getCursorType(value_cursor) }) {
            ir::Type::Str => value,
            ir::Type::Int | ir::Type::Double => ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(value)),
                callee_usr: String::new(),
                callee_name: "toString".to_owned(),
                args: Vec::new(),
                ty: ir::Type::Str,
                origin: origin.clone(),
            },
            _ => return None,
        }
    };
    pieces.push(piece);
    Some((name, pieces))
}

/// `ss << a << b;` used as its own statement — builds
/// `ss = ss + a.toString() + b.toString();`, preserving whatever `ss`
/// already held (the same reduce-with-`Add` `lower_ostream_insertion_chain`
/// already uses for `print`'s message, just starting the fold from `ss`
/// itself instead of the first piece, since this appends rather than
/// replaces). `None` for anything `lower_stringstream_insertion_chain`
/// itself doesn't recognize, so the caller falls through to the ordinary
/// bailout unchanged.
unsafe fn lower_stringstream_insertion_stmt(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Stmt> {
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_CallExpr {
        return None;
    }
    let (name, pieces) =
        unsafe { lower_stringstream_insertion_chain(cursor, project_root, origin) }?;
    if pieces.is_empty() {
        return None;
    }
    let value = pieces.into_iter().fold(
        ir::Expr::Ref {
            name: name.clone(),
            ty: ir::Type::Str,
            origin: origin.clone(),
        },
        |acc, piece| ir::Expr::Binary {
            op: ir::BinaryOp::Add,
            lhs: Box::new(acc),
            rhs: Box::new(piece),
            ty: ir::Type::Str,
            origin: origin.clone(),
        },
    );
    Some(ir::Stmt::Assign {
        name,
        value,
        origin: origin.clone(),
    })
}

/// `*it` / `it->field` inside a loop `lower_iterator_for_loop` recognized,
/// or inside the guarded `then` branch `lower_find_iterator_guard_idiom`
/// recognized — both register the iterator variable's Dart binding in
/// `ACTIVE_ITERATOR_LOOPS` before lowering the body/branch that can
/// dereference it. Deliberately *not* using a generic receiver lowering:
/// `it`'s own declared type is the never-representable iterator, so
/// `lower_expr` on the (`ImplicitCastExpr`-wrapped, confirmed via
/// `-ast-dump`) receiver cursor runs straight into the generic
/// implicit-conversion wrapper and produces a compound
/// `Expr::UnsupportedTyped`, not the plain `Expr::Ref` this needs — the
/// receiver's *identity* (which declaration it names) is all that's needed
/// here, not a full expression lowering of a type this bridge can't
/// represent. `clang_getCursorReferenced` resolves straight through the
/// cast to the `VarDecl`, exactly like `lower_iterator_for_loop`'s own
/// condition/increment checks do. If that name is currently bound in
/// `ACTIVE_ITERATOR_LOOPS`, it *is* the loop's/branch's own Dart element
/// binding — `Expr::Ref` to that same name, now correctly typed as the
/// element rather than the iterator. `operator->` returns the identical
/// `Ref`, not a `FieldAccess` itself: the field name isn't available at
/// this call site (this function only ever sees the `operator->` call in
/// isolation), so the surrounding member-access lowering that invoked this
/// — the same one that already wraps a plain `.field`/`->field` around any
/// other receiver — wraps the field around this corrected `Ref` the same
/// way. An iterator variable used *outside* a recognized idiom (stored in a
/// field, produced by a bare `std::find`, reassigned mid-loop via
/// `operator=`) has nothing registered for its name and falls through to
/// the honest bailout below, unchanged. Not scoped to a specific iterator
/// template name (`_List_iterator`, `_Rb_tree_const_iterator`/
/// `_Rb_tree_iterator` for `set`, `__normal_iterator` for `vector`, ...):
/// the registry lookup below is what actually gates correctness — it can
/// only match a name one of those two idioms itself pushed, which already
/// restricted the container to `Type::List`/`Type::Set` — so a real
/// receiver neither recognizer ever produced (a smart pointer's own
/// `operator*`, for instance) would just fail the lookup and fall through
/// unchanged.
unsafe fn lower_stdlib_dereference_call(
    call_cursor: clang_sys::CXCursor,
    template_name: &str,
    callee_name: &str,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    if unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) } < 1 {
        return Some(ir::Expr::UnsupportedTyped {
            reason: format!("unsupported std::{template_name}::{callee_name} call"),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
            origin: origin.clone(),
        });
    }
    let receiver_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
    let receiver_referenced = unsafe { clang_sys::clang_getCursorReferenced(receiver_cursor) };
    let receiver_name = if unsafe { clang_sys::clang_Cursor_isNull(receiver_referenced) } == 0 {
        Some(dart_safe_identifier(&unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(
                receiver_referenced,
            ))
        }))
    } else {
        None
    };
    match receiver_name
        .and_then(|name| active_iterator_loop_element_type(&name).map(|ty| (name, ty)))
    {
        Some((name, elem_ty)) => Some(ir::Expr::Ref {
            name,
            ty: elem_ty,
            origin: origin.clone(),
        }),
        None => Some(ir::Expr::UnsupportedTyped {
            reason: format!("unsupported std::{template_name}::{callee_name} call"),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
            origin: origin.clone(),
        }),
    }
}

unsafe fn lower_stdlib_method_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };

    // `*it`/`it->field` on a `__gnu_cxx::__normal_iterator` (`std::vector`'s/
    // `std::string`'s real iterator implementation, outside `namespace
    // std`) — checked before `stdlib_template_name(owner)?` below, whose own
    // ancestor walk only accepts `std` (deliberately: see
    // `is_normal_iterator_decl`'s doc comment for why a shared, general
    // template-name resolver is the wrong place to widen this). Delegates to
    // the exact same generic-template dispatch below by simply treating this
    // as `template_name == "__normal_iterator"` — the dereference arm inside
    // that big match doesn't care which specific stdlib template it is
    // (matches on `(_, "operator*" | "operator->")`), so this only needs to
    // get there, not duplicate its logic.
    if matches!(callee_name.as_str(), "operator*" | "operator->")
        && unsafe { is_normal_iterator_decl(owner) }
    {
        return unsafe {
            lower_stdlib_dereference_call(call_cursor, "__normal_iterator", &callee_name, origin)
        };
    }

    let template_name = unsafe { stdlib_template_name(owner) }?;

    // `operator[]` doesn't share `.size()`'s AST shape, even though both are
    // `CXXMethod`s of the same class: a dot-call's receiver is wrapped in a
    // `MemberRefExpr` (`member_ref_receiver`'s usual territory), but an
    // operator-syntax call's receiver is just a bare expression cursor,
    // sitting alongside the operator function reference and the index as
    // one of three plain children — confirmed empirically (`collect_children`
    // on `valores[i]` gives `[DeclRefExpr, UnexposedExpr, UnexposedExpr]`,
    // never a `MemberRefExpr`, unlike `valores.size()`'s single
    // `MemberRefExpr` child). Handled first and separately rather than
    // forcing one receiver-extraction shape to fit both.
    if callee_name == "operator[]" {
        let children = unsafe { collect_children(call_cursor) };
        let [receiver_cursor, _operator_ref_cursor, index_cursor] = children.as_slice() else {
            return Some(ir::Expr::UnsupportedTyped {
                reason: format!(
                    "std::{template_name}::operator[] call had {} children, expected 3",
                    children.len()
                ),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin: origin.clone(),
            });
        };
        let target = unsafe { lower_expr(*receiver_cursor, project_root) };
        let index = unsafe { lower_expr(*index_cursor, project_root) };
        if template_name == "basic_string" {
            return Some(ir::Expr::StringByteAt {
                target: Box::new(target),
                index: Box::new(index),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin: origin.clone(),
            });
        }
        if template_name == "map" {
            let value_type = match &target {
                ir::Expr::Ref { ty, .. } | ir::Expr::FieldAccess { ty, .. } => match ty {
                    ir::Type::Map(_, value_type) => (**value_type).clone(),
                    _ => {
                        return Some(ir::Expr::UnsupportedTyped {
                            reason: "std::map::operator[] receiver did not lower to a Dart Map"
                                .to_owned(),
                            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                            origin: origin.clone(),
                        });
                    }
                },
                _ => {
                    return Some(ir::Expr::UnsupportedTyped {
                        reason: "std::map::operator[] receiver has no recoverable map type"
                            .to_owned(),
                        ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                        origin: origin.clone(),
                    });
                }
            };
            return Some(ir::Expr::MapIndexOrInsert {
                target: Box::new(target),
                index: Box::new(index),
                default_value: Box::new(default_scalar_value(&value_type, origin)),
                ty: value_type,
                origin: origin.clone(),
            });
        }
        if !matches!(template_name.as_str(), "vector" | "deque") {
            return Some(ir::Expr::UnsupportedTyped {
                reason: format!("unsupported std::{template_name}::operator[] call"),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin: origin.clone(),
            });
        }
        // Deliberately not `lower_type(clang_getCursorType(call_cursor))` —
        // `operator[]`'s own return type is `const_reference`, a
        // template-dependent alias that `libclang` reports as
        // `CXType_Unexposed` with no usable declaration behind it (confirmed
        // empirically: it produced `Type::Unsupported("int")` for
        // `valores[i]`, which would have bailed out every function that
        // indexes a `vector<int>` even though the index itself lowers
        // fine). `owner` is the `vector<int>` specialization decl itself —
        // asking *its own* type for template argument 0 reuses the exact
        // same element-type resolution `lower_type`'s
        // `CXType_Record`/`CXType_Unexposed` branch already does for a
        // `vector<int>`-typed value, and is reliable for the same reason
        // that one is.
        let ty = unsafe { stdlib_sequence_element_type(owner, &template_name) };
        return Some(ir::Expr::Index {
            target: Box::new(target),
            index: Box::new(index),
            ty,
            origin: origin.clone(),
        });
    }

    // `std::cout << a << b << std::endl;` / `std::cerr << a << std::endl;`
    // — see `lower_ostream_insertion_chain` for the narrow scope (only
    // `std::cout`/`std::cerr`, only when the chain visibly ends in
    // `std::endl`, only `Str`/`Int`/`Double` operands). Anything outside
    // that scope (another stream, no trailing `endl`, an unrecognized
    // operand type) returns `None` here and falls through to this
    // function's own generic bailout below, unchanged.
    if template_name == "basic_ostream"
        && callee_name == "operator<<"
        && unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) } == 2
    {
        let receiver_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
        let value_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 1) };
        if unsafe { is_std_endl(value_cursor) }
            && let Some((stream, pieces)) =
                unsafe { lower_ostream_insertion_chain(receiver_cursor, project_root, origin) }
            && !pieces.is_empty()
        {
            let message = pieces
                .into_iter()
                .reduce(|acc, piece| ir::Expr::Binary {
                    op: ir::BinaryOp::Add,
                    lhs: Box::new(acc),
                    rhs: Box::new(piece),
                    ty: ir::Type::Str,
                    origin: origin.clone(),
                })
                .expect("checked non-empty above");
            // `print` (stdout, no import) and `stderr.writeln` (`dart:io`,
            // added by `emit::dart`'s post-hoc `source.contains("stderr.")`
            // scan — the same mechanism already used for `Uint8List` →
            // `dart:typed_data`) both append the trailing newline
            // `std::endl` asks for.
            return Some(match stream {
                KnownOstream::Cout => ir::Expr::Call {
                    base_qualifier: None,
                    target: None,
                    callee_usr: String::new(),
                    callee_name: "print".to_owned(),
                    args: vec![message],
                    ty: ir::Type::Void,
                    origin: origin.clone(),
                },
                KnownOstream::Cerr => ir::Expr::Call {
                    base_qualifier: None,
                    target: Some(Box::new(ir::Expr::Ref {
                        name: "stderr".to_owned(),
                        ty: ir::Type::Void,
                        origin: origin.clone(),
                    })),
                    callee_usr: String::new(),
                    callee_name: "writeln".to_owned(),
                    args: vec![message],
                    ty: ir::Type::Void,
                    origin: origin.clone(),
                },
            });
        }
    }

    // A normal dot call owns a `MemberRefExpr` for its callee. Overloaded
    // operators, however, use the same call cursor but expose the receiver
    // as argument zero (`destination = source` is the common case in the
    // Verovio corpus). Normalize both shapes *before* method dispatch: a
    // method that we do not yet bridge must still report its own name, not a
    // fragile incidental detail of libclang's child ordering.
    let target = match unsafe { stdlib_method_receiver(call_cursor, project_root, origin) } {
        Ok(target) => target,
        Err(reason) => {
            return Some(ir::Expr::UnsupportedTyped {
                reason,
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin: origin.clone(),
            });
        }
    };

    match (template_name.as_str(), callee_name.as_str()) {
        // `ss.str()` — the stringstream variable already *is* `Type::Str`
        // (round 19: `lower_type`'s `basic_stringstream`/
        // `basic_ostringstream` case), so reading it back is identity, the
        // zero-argument form only (`ss.str(newValue)`, a rarer resetting
        // overload no fixture needs, falls through to the generic bailout
        // below unchanged).
        ("basic_stringstream" | "basic_ostringstream", "str")
            if unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) } == 0 =>
        {
            Some(target)
        }
        ("basic_string", "size") | ("basic_string", "length") => Some(ir::Expr::StringByteLength {
            target: Box::new(target),
            origin: origin.clone(),
        }),
        ("vector" | "list" | "deque" | "stack", "size") => Some(ir::Expr::FieldAccess {
            target: Box::new(target),
            field: "length".to_owned(),
            ty: ir::Type::Int,
            origin: origin.clone(),
        }),
        ("map", "size") | ("set", "size") => Some(ir::Expr::FieldAccess {
            target: Box::new(target),
            field: "length".to_owned(),
            ty: ir::Type::Int,
            origin: origin.clone(),
        }),
        ("basic_string", "empty")
        | ("vector", "empty")
        | ("list", "empty")
        | ("deque", "empty")
        | ("stack", "empty")
        | ("map", "empty")
        | ("set", "empty") => Some(ir::Expr::FieldAccess {
            target: Box::new(target),
            field: "isEmpty".to_owned(),
            ty: ir::Type::Bool,
            origin: origin.clone(),
        }),
        // `v.erase(std::remove_if(v.begin(), v.end(), pred), v.end());` — the
        // classic erase-remove idiom, one C++ statement (no separate
        // temporary iterator), F10/tarefa 13
        // (`docs/prompts/2026-08-21-13-iteradores-stl.md`). `remove_if`'s own
        // return value (an iterator to the new logical end) has no
        // representation this bridge gives it on its own, same reasoning as
        // `lower_find_contains_idiom`'s `std::find`: the *whole* two-call
        // idiom is recognized together, requiring the erase receiver and
        // every `begin`/`end` mention inside `remove_if`'s own arguments and
        // the outer `end()` argument to all agree
        // (`container_begin_or_end_receiver`/`same_receiver_ignoring_origin`,
        // the same machinery `lower_find_contains_idiom` already uses).
        // `remove_if` reached any other way (no enclosing `.erase(...)`, a
        // mismatched receiver) is handled by `lower_stdlib_algorithm_call`'s
        // own honest bailout instead — it never reaches here at all, since
        // this arm only fires once `.erase(...)`'s own first argument is
        // already known to *be* a `remove_if` call.
        ("vector" | "list" | "deque", "erase") => {
            match unsafe { lower_erase_remove_if_idiom(&target, call_cursor, project_root, origin) }
            {
                Some(removed) => Some(removed),
                None => Some(ir::Expr::UnsupportedTyped {
                    reason: format!("unsupported std::{template_name}::erase call"),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                }),
            }
        }
        ("map", "contains") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if args.len() != 1 {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::map::contains had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr: String::new(),
                callee_name: "containsKey".to_owned(),
                args,
                ty: ir::Type::Bool,
                origin: origin.clone(),
            })
        }
        ("set", "contains") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if args.len() != 1 {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::set::contains had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr: String::new(),
                callee_name: "contains".to_owned(),
                args,
                ty: ir::Type::Bool,
                origin: origin.clone(),
            })
        }
        ("map", "count") | ("set", "count") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if args.len() != 1 {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::{template_name}::count had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            let contains_name = if template_name == "map" {
                "containsKey"
            } else {
                "contains"
            };
            Some(ir::Expr::Conditional {
                condition: Box::new(ir::Expr::Call {
                    base_qualifier: None,
                    target: Some(Box::new(target)),
                    callee_usr: String::new(),
                    callee_name: contains_name.to_owned(),
                    args,
                    ty: ir::Type::Bool,
                    origin: origin.clone(),
                }),
                then_expr: Box::new(ir::Expr::IntLiteral {
                    value: 1,
                    origin: origin.clone(),
                }),
                else_expr: Box::new(ir::Expr::IntLiteral {
                    value: 0,
                    origin: origin.clone(),
                }),
                ty: ir::Type::Int,
                origin: origin.clone(),
            })
        }
        ("basic_string", "c_str") => Some(target),
        ("basic_string", "find") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            let [needle] = args.as_slice() else {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::basic_string::find had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            };
            Some(ir::Expr::StringByteIndexOf {
                target: Box::new(target),
                needle: Box::new(needle.clone()),
                origin: origin.clone(),
            })
        }
        ("basic_string", "compare") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            // `compare(other)` compares the whole receiver; `compare(pos,
            // count, other)` compares only the substring starting at `pos`
            // of length `count` (`current->compare(0, 4, "*fs:")`, the real
            // shape found in Verovio's `iohumdrum.cpp`) — the receiver of
            // the comparison becomes `target.substring(pos, pos + count)`,
            // the same `start, start + count` shape the `substr` arm right
            // below already establishes for the same two-argument slice.
            // The 5-argument overload (also slicing `other`) isn't
            // evidenced in this corpus and stays unsupported.
            let (compare_target, other) = match args.as_slice() {
                [other] => (target, other.clone()),
                [pos, count, other] => (
                    ir::Expr::Call {
                        base_qualifier: None,
                        target: Some(Box::new(target)),
                        callee_usr: String::new(),
                        callee_name: "substring".to_owned(),
                        args: vec![
                            pos.clone(),
                            ir::Expr::Binary {
                                op: ir::BinaryOp::Add,
                                lhs: Box::new(pos.clone()),
                                rhs: Box::new(count.clone()),
                                ty: ir::Type::Int,
                                origin: origin.clone(),
                            },
                        ],
                        ty: ir::Type::Str,
                        origin: origin.clone(),
                    },
                    other.clone(),
                ),
                _ => {
                    return Some(ir::Expr::UnsupportedTyped {
                        reason: format!(
                            "std::basic_string::compare had {} arguments, expected 1 or 3",
                            args.len()
                        ),
                        ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                        origin: origin.clone(),
                    });
                }
            };
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(compare_target)),
                callee_usr: String::new(),
                callee_name: "compareTo".to_owned(),
                args: vec![other],
                ty: ir::Type::Int,
                origin: origin.clone(),
            })
        }
        ("basic_string", "substr") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            let substring_args = match args.as_slice() {
                [start] => vec![start.clone()],
                [start, count] => vec![
                    start.clone(),
                    ir::Expr::Binary {
                        op: ir::BinaryOp::Add,
                        lhs: Box::new(start.clone()),
                        rhs: Box::new(count.clone()),
                        ty: ir::Type::Int,
                        origin: origin.clone(),
                    },
                ],
                _ => {
                    return Some(ir::Expr::UnsupportedTyped {
                        reason: format!(
                            "std::basic_string::substr had {} arguments, expected 1 or 2",
                            args.len()
                        ),
                        ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                        origin: origin.clone(),
                    });
                }
            };
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr: String::new(),
                callee_name: "substring".to_owned(),
                args: substring_args,
                ty: ir::Type::Str,
                origin: origin.clone(),
            })
        }
        ("basic_string", "at") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            let [index] = args.as_slice() else {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::basic_string::at had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            };
            Some(ir::Expr::StringByteAt {
                target: Box::new(target),
                index: Box::new(index.clone()),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin: origin.clone(),
            })
        }
        ("vector" | "list" | "deque", "push_back") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if args.len() != 1 {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::{template_name}::push_back had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr: String::new(),
                callee_name: "add".to_owned(),
                args,
                ty: ir::Type::Void,
                origin: origin.clone(),
            })
        }
        ("vector" | "list" | "deque", "clear") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if !args.is_empty() {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::{template_name}::clear had {} arguments, expected none",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr: String::new(),
                callee_name: "clear".to_owned(),
                args,
                ty: ir::Type::Void,
                origin: origin.clone(),
            })
        }
        ("vector" | "deque", "at") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            let [index] = args.as_slice() else {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::{template_name}::at had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            };
            Some(ir::Expr::Index {
                target: Box::new(target),
                index: Box::new(index.clone()),
                ty: unsafe { stdlib_sequence_element_type(owner, &template_name) },
                origin: origin.clone(),
            })
        }
        ("vector" | "list" | "deque", "front") | ("vector" | "list" | "deque", "back") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if !args.is_empty() {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::{template_name}::{callee_name} had {} arguments, expected none",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            let index = if callee_name == "front" {
                ir::Expr::IntLiteral {
                    value: 0,
                    origin: origin.clone(),
                }
            } else {
                ir::Expr::Binary {
                    op: ir::BinaryOp::Sub,
                    lhs: Box::new(ir::Expr::FieldAccess {
                        target: Box::new(target.clone()),
                        field: "length".to_owned(),
                        ty: ir::Type::Int,
                        origin: origin.clone(),
                    }),
                    rhs: Box::new(ir::Expr::IntLiteral {
                        value: 1,
                        origin: origin.clone(),
                    }),
                    ty: ir::Type::Int,
                    origin: origin.clone(),
                }
            };
            Some(ir::Expr::Index {
                target: Box::new(target),
                index: Box::new(index),
                ty: unsafe { stdlib_sequence_element_type(owner, &template_name) },
                origin: origin.clone(),
            })
        }
        // `std::stack<T>` (default `std::deque<T>`-backed) is LIFO-only,
        // but its element-type resolution and Dart target are identical to
        // `vector`/`list`/`deque`'s (`lower_type`'s stdlib-template branch
        // maps it to the same `List<T>`) — `.top()` is the last element,
        // `.push`/`.pop` are `.add`/`.removeLast`, `.empty`/`.size` mirror
        // the vector arms just below. Real corpus triggers (`view_page.cpp`
        // etc.): `stack<Brush|Pen>` (drawing-context save/restore) and
        // `stack<FontInfo *|Object *>` (both project-pointer element
        // types, already representable).
        ("stack", "top") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if !args.is_empty() {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::stack::top had {} arguments, expected none",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            Some(ir::Expr::Index {
                target: Box::new(target.clone()),
                index: Box::new(ir::Expr::Binary {
                    op: ir::BinaryOp::Sub,
                    lhs: Box::new(ir::Expr::FieldAccess {
                        target: Box::new(target),
                        field: "length".to_owned(),
                        ty: ir::Type::Int,
                        origin: origin.clone(),
                    }),
                    rhs: Box::new(ir::Expr::IntLiteral {
                        value: 1,
                        origin: origin.clone(),
                    }),
                    ty: ir::Type::Int,
                    origin: origin.clone(),
                }),
                ty: unsafe { stdlib_sequence_element_type(owner, &template_name) },
                origin: origin.clone(),
            })
        }
        ("stack", "push") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if args.len() != 1 {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::stack::push had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr: String::new(),
                callee_name: "add".to_owned(),
                args,
                ty: ir::Type::Void,
                origin: origin.clone(),
            })
        }
        ("stack", "pop") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            if !args.is_empty() {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::stack::pop had {} arguments, expected none",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            }
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr: String::new(),
                callee_name: "removeLast".to_owned(),
                args,
                ty: ir::Type::Void,
                origin: origin.clone(),
            })
        }
        // `map`/`unordered_map` already lower to `Map<K, V>`. `.at(key)`
        // asks for a value that must exist (C++ throws `out_of_range`
        // otherwise) — `map[key]!` preserves that "must exist" intent with
        // a real Dart runtime failure on a missing key, even though the
        // thrown type differs from `std::out_of_range` (the same documented
        // trade-off already accepted for other STL methods this module
        // maps to a near-equivalent rather than a byte-for-byte behavioral
        // twin, e.g. `dynamic_cast`'s round 9).
        ("map" | "unordered_map", "at") => {
            let args = unsafe { lower_call_arguments(call_cursor, project_root) }?;
            let [key] = args.as_slice() else {
                return Some(ir::Expr::UnsupportedTyped {
                    reason: format!(
                        "std::{template_name}::at had {} arguments, expected exactly 1",
                        args.len()
                    ),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin: origin.clone(),
                });
            };
            let value_type = match &target {
                ir::Expr::Ref { ty, .. } | ir::Expr::FieldAccess { ty, .. } => match ty {
                    ir::Type::Map(_, value_type) => (**value_type).clone(),
                    _ => {
                        return Some(ir::Expr::UnsupportedTyped {
                            reason: format!(
                                "std::{template_name}::at receiver did not lower to a Dart Map"
                            ),
                            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                            origin: origin.clone(),
                        });
                    }
                },
                _ => {
                    return Some(ir::Expr::UnsupportedTyped {
                        reason: format!(
                            "std::{template_name}::at receiver has no recoverable map type"
                        ),
                        ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                        origin: origin.clone(),
                    });
                }
            };
            // `Expr::Convert` to a non-`Nullable` target from a
            // `Nullable`-typed operand already renders as the `!` force-
            // unwrap (`emit::dart`'s `Expr::Convert` arm, the same one that
            // renders a raw-pointer dereference) — reused here instead of a
            // new IR node.
            Some(ir::Expr::Convert {
                operand: Box::new(ir::Expr::Index {
                    target: Box::new(target),
                    index: Box::new(key.clone()),
                    ty: ir::Type::Nullable(Box::new(value_type.clone())),
                    origin: origin.clone(),
                }),
                ty: value_type,
                origin: origin.clone(),
            })
        }
        // `optional`/smart pointers already lower to `T?` at the type level
        // (`lower_type`'s `optional`/`unique_ptr`/`shared_ptr`/`weak_ptr`
        // branch) — reading the wrapped value back is identity, the same
        // value the receiver already denotes. This does not claim to
        // preserve C++'s throw-on-empty-access behavior of
        // `optional::value()`/`unique_ptr::operator*` — the same documented
        // limitation `lower_type`'s own comment already states for this
        // family (ownership/control-block mechanics aren't modeled, only
        // presence/absence). Real corpus trigger: `Object::m_plistReferences`
        // (`std::unique_ptr<ListOfConstObjects>`), read via `.get()` and
        // `->` (`push_back`).
        (
            "optional" | "unique_ptr" | "shared_ptr" | "weak_ptr",
            "get" | "value" | "operator*" | "operator->",
        ) => Some(target),
        ("optional" | "unique_ptr" | "shared_ptr" | "weak_ptr", "has_value") => {
            Some(ir::Expr::Binary {
                op: ir::BinaryOp::Ne,
                lhs: Box::new(target),
                rhs: Box::new(ir::Expr::NullLiteral {
                    origin: origin.clone(),
                }),
                ty: ir::Type::Bool,
                origin: origin.clone(),
            })
        }
        // `*it` / `it->field` inside a loop `lower_iterator_for_loop`
        // recognized. Deliberately *not* using `target` (the generic
        // receiver lowering computed above, shared by every other arm):
        // `it`'s own declared type is the never-representable iterator, so
        // `lower_expr` on the (`ImplicitCastExpr`-wrapped, confirmed via
        // `-ast-dump`) receiver cursor runs straight into the generic
        // implicit-conversion wrapper and produces a compound
        // `Expr::UnsupportedTyped`, not the plain `Expr::Ref` this needs —
        // the receiver's *identity* (which declaration it names) is all
        // that's needed here, not a full expression lowering of a type this
        // bridge can't represent. `clang_getCursorReferenced` resolves
        // straight through the cast to the `VarDecl`, exactly like
        // `lower_iterator_for_loop`'s own condition/increment checks do.
        // If that name is currently bound in `ACTIVE_ITERATOR_LOOPS`, it
        // *is* the loop's own Dart element binding — `Expr::Ref` to that
        // same name, now correctly typed as the element rather than the
        // iterator. `operator->` returns the identical `Ref`, not a
        // `FieldAccess` itself: the field name isn't available at this call
        // site (this function only ever sees the `operator->` call in
        // isolation), so the surrounding member-access lowering that
        // invoked this — the same one that already wraps a plain
        // `.field`/`->field` around any other receiver — wraps the field
        // around this corrected `Ref` the same way. An iterator variable
        // used *outside* a recognized loop (stored in a field, produced by
        // `std::find`, reassigned mid-loop via `operator=`) has nothing
        // registered for its name and falls through to the honest bailout
        // below, unchanged. Not scoped to a specific iterator template name
        // (`_List_iterator`, `_Rb_tree_const_iterator`/`_Rb_tree_iterator`
        // for `set`, ...): the registry lookup below is what actually gates
        // correctness — it can only match a name `lower_iterator_for_loop`
        // itself pushed, which already restricted the container to
        // `Type::List`/`Type::Set` — so a real receiver this loop
        // recognizer never produced (a smart pointer's own `operator*`, for
        // instance) would just fail the lookup and fall through unchanged.
        (_, "operator*" | "operator->")
            if unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) } >= 1 =>
        unsafe { lower_stdlib_dereference_call(call_cursor, &template_name, &callee_name, origin) },
        _ => Some(ir::Expr::UnsupportedTyped {
            reason: format!("unsupported std::{template_name}::{callee_name} call"),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
            origin: origin.clone(),
        }),
    }
}

/// The element type of a recognized sequence specialization. `operator[]`
/// and `at` return a dependent `reference` alias, so their own cursor type is
/// less useful than the template argument on the owning class.
unsafe fn stdlib_sequence_element_type(
    owner: clang_sys::CXCursor,
    template_name: &str,
) -> ir::Type {
    let owner_type = unsafe { clang_sys::clang_getCursorType(owner) };
    if unsafe { clang_sys::clang_Type_getNumTemplateArguments(owner_type) } >= 1 {
        lower_type(unsafe { clang_sys::clang_Type_getTemplateArgumentAsType(owner_type, 0) })
    } else {
        ir::Type::Unsupported(format!(
            "std::{template_name} with no element type argument"
        ))
    }
}

/// Resolves the object receiving a standard-library method call while
/// normalizing libclang's two call shapes: a regular dot call has a direct
/// `MemberRefExpr` child, while an overloaded operator passes its receiver as
/// argument zero. Keeping this structural normalization separate from the
/// stdlib method table lets an unsupported method retain a precise diagnostic.
unsafe fn stdlib_method_receiver(
    call_cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Result<ir::Expr, String> {
    let member_ref = unsafe { collect_children(call_cursor) }
        .into_iter()
        .find(|child| unsafe {
            clang_sys::clang_getCursorKind(*child) == clang_sys::CXCursor_MemberRefExpr
        });

    if let Some(member_ref) = member_ref {
        return Ok(unsafe { member_ref_receiver(member_ref, project_root, origin) });
    }

    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
    if arg_count < 1 {
        return Err("standard-library operator call had no receiver argument".to_owned());
    }
    let receiver_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
    Ok(unsafe { lower_expr(receiver_cursor, project_root) })
}

/// `X.begin()`/`X.cbegin()` (`want_begin`) or `X.end()`/`X.cend()`
/// (`!want_begin`) — a zero-argument iterator-producing member call on a
/// known sequence/set container. Returns the *lowered* receiver `X`, so
/// `lower_find_contains_idiom` can require every `begin`/`end` receiver in
/// the whole comparison to be the exact same value via `Expr`'s own
/// `PartialEq`, rather than comparing raw cursor identity. `None` for
/// anything else: a different member name, an owner this bridge doesn't
/// map to a Dart collection, or a non-call expression.
unsafe fn container_begin_or_end_receiver(
    cursor: clang_sys::CXCursor,
    want_begin: bool,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_CallExpr {
        return None;
    }
    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        return None;
    }
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    let matches_name = if want_begin {
        name == "begin" || name == "cbegin"
    } else {
        name == "end" || name == "cend"
    };
    if !matches_name {
        return None;
    }
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let template_name = unsafe { stdlib_template_name(owner) }?;
    // Every template name `lower_type`'s own `CXType_Record`/
    // `CXType_Unexposed` branch already maps to `Type::List`/`Type::Set`
    // (`"array"`/`"initializer_list"` → `List`, `"unordered_set"` → `Set`,
    // confirmed by grepping that branch directly) belongs here too — this
    // function only decides *which* containers are safe to treat as one
    // receiver across `begin`/`end`, not how they're represented. `map`/
    // `unordered_map` deliberately excluded: their iterator's `first`/
    // `second` needs a `key`/`value` translation neither this idiom nor
    // `lower_find_contains_idiom` (this function's other caller) attempts.
    if !matches!(
        template_name.as_str(),
        "vector"
            | "list"
            | "set"
            | "deque"
            | "multiset"
            | "array"
            | "initializer_list"
            | "unordered_set"
    ) {
        return None;
    }
    unsafe { stdlib_method_receiver(cursor, project_root, origin) }.ok()
}

/// Whether `a` and `b` are the same *receiver* expression — the same
/// lvalue shape and identity, ignoring where in the source each mention
/// sits. `Expr`'s derived `PartialEq` compares `Origin` too, so it always
/// reports two mentions of the same variable at different source
/// locations as unequal; only the two lvalue shapes a `begin`/`end`
/// receiver can actually take (a bare reference, or a field reached
/// through one) are compared here, so this stays conservative rather than
/// guessing for a shape it hasn't seen.
fn same_receiver_ignoring_origin(a: &ir::Expr, b: &ir::Expr) -> bool {
    match (a, b) {
        (
            ir::Expr::Ref {
                name: name_a,
                ty: ty_a,
                ..
            },
            ir::Expr::Ref {
                name: name_b,
                ty: ty_b,
                ..
            },
        ) => name_a == name_b && ty_a == ty_b,
        (
            ir::Expr::FieldAccess {
                target: target_a,
                field: field_a,
                ty: ty_a,
                ..
            },
            ir::Expr::FieldAccess {
                target: target_b,
                field: field_b,
                ty: ty_b,
                ..
            },
        ) => {
            field_a == field_b && ty_a == ty_b && same_receiver_ignoring_origin(target_a, target_b)
        }
        // `ss[staffindex].tieends.begin()` (real trigger, `iohumdrum.cpp`):
        // an array/map-subscript receiver reached through `FieldAccess`
        // above. `ty` intentionally not compared here (unlike `Ref`/
        // `FieldAccess`): `Index`'s own `ty` is the *element* type, already
        // covered once by the outer `FieldAccess`/`Ref` comparison that
        // wraps this — comparing it again would just repeat the same check.
        (
            ir::Expr::Index {
                target: target_a,
                index: index_a,
                ..
            },
            ir::Expr::Index {
                target: target_b,
                index: index_b,
                ..
            },
        ) => {
            same_receiver_ignoring_origin(target_a, target_b)
                && same_receiver_ignoring_origin(index_a, index_b)
        }
        (ir::Expr::This { .. }, ir::Expr::This { .. }) => true,
        _ => false,
    }
}

/// `std::find(X.begin(), X.end(), v) != X.end()` (or `==`, negated) — "does
/// `X` contain `v`?", confirmed as a real, common shape by grepping the
/// Verovio source directly (`adjustbeamsfunctor.cpp:326`'s
/// `std::find(dotLocs.cbegin(), dotLocs.cend(), dotLoc) !=
/// dotLocs.cend()`). Recognized as one whole comparison, not pieced
/// together from independently-lowered halves: `std::find`'s own iterator
/// return value has no representation this bridge gives it on its own, so
/// this only fires when every `begin`/`end` receiver across both sides of
/// the comparison provably agrees (`Expr`'s own `PartialEq`) — three
/// independent extractions that all have to land on the same lowered
/// receiver expression. `is_negated` is `true` for `!=` (the common "is
/// present" form), `false` for `==` ("is absent", wrapped in `!`).
///
/// `std::find_if(X.begin(), X.end(), pred) != X.end()` (F10/tarefa 13,
/// `docs/prompts/2026-08-21-13-iteradores-stl.md`) is the exact same idiom
/// with a predicate standing in for a value — "does some element of `X`
/// satisfy `pred`?" — so it shares every structural check below and only
/// differs in which Dart method the match becomes (`any(pred)` instead of
/// `contains(v)`) and in what its one distinguishing argument lowers to (a
/// predicate expression — usually a functor construction, already lowered
/// generically since `operator()` bridges to Dart's own `call` — rather
/// than a plain value).
unsafe fn lower_find_contains_idiom(
    is_negated: bool,
    find_call_cursor: clang_sys::CXCursor,
    outer_end_cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    let find_call_cursor = unsafe { unwrap_transparent_value_cursor(find_call_cursor) };
    let outer_end_cursor = unsafe { unwrap_transparent_value_cursor(outer_end_cursor) };
    if unsafe { clang_sys::clang_getCursorKind(find_call_cursor) } != clang_sys::CXCursor_CallExpr {
        return None;
    }
    let find_referenced = unsafe { clang_sys::clang_getCursorReferenced(find_call_cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(find_referenced) } != 0 {
        return None;
    }
    let find_name = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(find_referenced))
    };
    let dart_method_name = match find_name.as_str() {
        "find" => "contains",
        "find_if" => "any",
        _ => return None,
    };
    if unsafe {
        clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
            find_referenced,
        ))
    } == 0
    {
        return None;
    }
    if unsafe { clang_sys::clang_Cursor_getNumArguments(find_call_cursor) } != 3 {
        return None;
    }
    let begin_cursor = unsafe { clang_sys::clang_Cursor_getArgument(find_call_cursor, 0) };
    let end_cursor_in_find = unsafe { clang_sys::clang_Cursor_getArgument(find_call_cursor, 1) };
    let value_cursor = unsafe { clang_sys::clang_Cursor_getArgument(find_call_cursor, 2) };

    let begin_receiver =
        unsafe { container_begin_or_end_receiver(begin_cursor, true, project_root, origin) }?;
    let end_receiver_in_find = unsafe {
        container_begin_or_end_receiver(end_cursor_in_find, false, project_root, origin)
    }?;
    let end_receiver_outer =
        unsafe { container_begin_or_end_receiver(outer_end_cursor, false, project_root, origin) }?;
    // Structural equality *ignoring `Origin`*: the same three-in-source-code
    // receiver (`dotLocs` appearing at `.cbegin()`, `.cend()`, and the outer
    // `.cend()` again) lowers to three `Expr`s that agree on everything
    // except *where in the source* each mention sits — `Expr`'s derived
    // `PartialEq` compares `Origin` too, so it always reports these as
    // different. Only the lvalue shapes a receiver can actually take here
    // are compared; anything else is conservatively "not equal" rather than
    // guessed.
    if !same_receiver_ignoring_origin(&begin_receiver, &end_receiver_in_find)
        || !same_receiver_ignoring_origin(&begin_receiver, &end_receiver_outer)
    {
        return None;
    }

    let value_or_pred = unsafe { lower_expr(value_cursor, project_root) };
    let contains = ir::Expr::Call {
        base_qualifier: None,
        target: Some(Box::new(begin_receiver)),
        callee_usr: String::new(),
        callee_name: dart_method_name.to_owned(),
        args: vec![value_or_pred],
        ty: ir::Type::Bool,
        origin: origin.clone(),
    };
    Some(if is_negated {
        contains
    } else {
        ir::Expr::Unary {
            op: ir::UnaryOp::Not,
            operand: Box::new(contains),
            ty: ir::Type::Bool,
            origin: origin.clone(),
        }
    })
}

/// Strips an implicit copy/move-construction wrapper materializing a
/// by-value class-type temporary (`remove_if`'s/`end()`'s own iterator
/// return value, passed as a real function argument) — confirmed
/// empirically as a real, distinct shape from anything
/// `unwrap_transparent_value_cursor` already strips: it's a genuine
/// `CXCursor_CallExpr` whose own `clang_getCursorReferenced` resolves to
/// the constructor (`CXCursor_Constructor`) rather than the ordinary
/// `CXCursor_FunctionDecl` a real call resolves to, with the actual wrapped
/// call as its single argument. Loops (not a single strip) since
/// `unwrap_transparent_value_cursor` may need another pass once the
/// materialization is gone (a plain sugar wrapper, e.g. `ExprWithCleanups`,
/// can sit on either side of it). A cursor this doesn't apply to (an
/// ordinary call, with a real `FunctionDecl` target) passes through
/// unchanged after at most one failed check.
unsafe fn unwrap_value_materialization(mut cursor: clang_sys::CXCursor) -> clang_sys::CXCursor {
    loop {
        cursor = unsafe { unwrap_transparent_value_cursor(cursor) };
        if unsafe { clang_sys::clang_getCursorKind(cursor) } != clang_sys::CXCursor_CallExpr {
            return cursor;
        }
        let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
        if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0
            || unsafe { clang_sys::clang_getCursorKind(referenced) }
                != clang_sys::CXCursor_Constructor
            || unsafe { clang_sys::clang_Cursor_getNumArguments(cursor) } != 1
        {
            return cursor;
        }
        cursor = unsafe { clang_sys::clang_Cursor_getArgument(cursor, 0) };
    }
}

/// `erase_call_cursor` is a `.erase(a, b)` dot-call already known to be on
/// one of the sequence containers `lower_stdlib_method_call`'s own `"erase"`
/// arm recognizes, with `erase_receiver` its already-lowered receiver
/// (`target`, in that function's own naming) — `X` in `X.erase(std::
/// remove_if(X.begin(), X.end(), pred), X.end())`. `None` for anything that
/// isn't exactly that shape (a different first argument, a mismatched
/// receiver anywhere in the chain); `lower_stdlib_method_call`'s own caller
/// turns that into an honest bailout rather than guessing.
unsafe fn lower_erase_remove_if_idiom(
    erase_receiver: &ir::Expr,
    erase_call_cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    if unsafe { clang_sys::clang_Cursor_getNumArguments(erase_call_cursor) } != 2 {
        return None;
    }
    let remove_if_cursor = unsafe {
        unwrap_value_materialization(clang_sys::clang_Cursor_getArgument(erase_call_cursor, 0))
    };
    let outer_end_cursor = unsafe {
        unwrap_value_materialization(clang_sys::clang_Cursor_getArgument(erase_call_cursor, 1))
    };

    if unsafe { clang_sys::clang_getCursorKind(remove_if_cursor) } != clang_sys::CXCursor_CallExpr {
        return None;
    }
    let remove_if_referenced = unsafe { clang_sys::clang_getCursorReferenced(remove_if_cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(remove_if_referenced) } != 0 {
        return None;
    }
    let remove_if_name = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(remove_if_referenced))
    };
    if remove_if_name != "remove_if"
        || unsafe {
            clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
                remove_if_referenced,
            ))
        } == 0
        || !unsafe { free_function_reachable_from_std(remove_if_referenced) }
        || unsafe { clang_sys::clang_Cursor_getNumArguments(remove_if_cursor) } != 3
    {
        return None;
    }

    let begin_cursor = unsafe { clang_sys::clang_Cursor_getArgument(remove_if_cursor, 0) };
    let end_cursor_in_remove_if =
        unsafe { clang_sys::clang_Cursor_getArgument(remove_if_cursor, 1) };
    let pred_cursor = unsafe { clang_sys::clang_Cursor_getArgument(remove_if_cursor, 2) };

    let begin_receiver =
        unsafe { container_begin_or_end_receiver(begin_cursor, true, project_root, origin) }?;
    let end_receiver_in_remove_if = unsafe {
        container_begin_or_end_receiver(end_cursor_in_remove_if, false, project_root, origin)
    }?;
    let end_receiver_outer =
        unsafe { container_begin_or_end_receiver(outer_end_cursor, false, project_root, origin) }?;
    if !same_receiver_ignoring_origin(&begin_receiver, &end_receiver_in_remove_if)
        || !same_receiver_ignoring_origin(&begin_receiver, &end_receiver_outer)
        || !same_receiver_ignoring_origin(&begin_receiver, erase_receiver)
    {
        return None;
    }

    let pred = unsafe { lower_expr(pred_cursor, project_root) };
    Some(ir::Expr::Call {
        base_qualifier: None,
        target: Some(Box::new(begin_receiver)),
        callee_usr: String::new(),
        callee_name: "removeWhere".to_owned(),
        args: vec![pred],
        ty: ir::Type::Void,
        origin: origin.clone(),
    })
}

// Scoped registry of C++ iterator variables currently standing for a Dart
// `for`-each element inside `lower_iterator_for_loop`'s recognized idiom
// (round 18, `docs/prompts/2026-08-20-loop-bailout.md`): while the loop
// body is being lowered, `*it`/`it->field` need to resolve to the loop's
// own Dart binding (`it`, already typed as the element) instead of the raw
// `std::_List_iterator`/`std::_Rb_tree_const_iterator` bailout a plain
// `DeclRefExpr` lookup would produce. Keyed by Dart variable name rather
// than a clang cursor: every consumer (`lower_stdlib_method_call`'s
// `operator*`/`operator->` arms) only ever has the already-lowered
// receiver `Expr` in hand, which keeps the name but discards cursor
// identity — the same class-name-based approximation
// `same_receiver_ignoring_origin` already accepts elsewhere in this file.
// Thread-local, not a threaded parameter: `lower_stmt`/`lower_expr` have no
// context parameter to carry this through, and each compilation unit lowers
// on its own worker thread (confirmed by the diagnosis tool's own
// per-worker log lines), so no cross-unit interference is possible. A
// `Vec` (stack), not a single slot, so nested iterator loops with distinct
// variable names both resolve correctly.
thread_local! {
    static ACTIVE_ITERATOR_LOOPS: RefCell<Vec<(String, ir::Type)>> = const { RefCell::new(Vec::new()) };
}

fn push_active_iterator_loop(name: String, elem_ty: ir::Type) {
    ACTIVE_ITERATOR_LOOPS.with(|stack| stack.borrow_mut().push((name, elem_ty)));
}

fn pop_active_iterator_loop() {
    ACTIVE_ITERATOR_LOOPS.with(|stack| {
        stack.borrow_mut().pop();
    });
}

/// The element type bound to `name` by the innermost currently-lowering
/// `lower_iterator_for_loop`, if any — `None` for an iterator variable used
/// outside that recognized idiom (mid-body reassignment, an iterator stored
/// in a field, `std::find` result, ...), which correctly leaves those
/// (rarer, harder) shapes as the honest bailout they already were.
fn active_iterator_loop_element_type(name: &str) -> Option<ir::Type> {
    ACTIVE_ITERATOR_LOOPS.with(|stack| {
        stack
            .borrow()
            .iter()
            .rev()
            .find(|(active_name, _)| active_name == name)
            .map(|(_, ty)| ty.clone())
    })
}

/// `for (auto it = X.begin(); it != X.end(); ++it) { ...*it...it->f... }` —
/// the general-iterator idiom explicitly deferred by round 8 (2026-08-20) as
/// "bem maior que um fix de expressão isolada", the single largest residual
/// family after that round (`std::vector::begin`/`end`,
/// `std::list::begin`/`end`, `std::_List_iterator::operator*`/`operator->`/
/// `operator++`, combined ~700 occurrences in the round-17 snapshot).
/// Recognized as one whole statement, exactly like
/// `lower_find_contains_idiom` recognizes `std::find(...) != end()` as one
/// whole comparison: `std::_List_iterator`/`std::_Rb_tree_const_iterator`
/// have no standalone representation in this IR (an iterator is not a
/// value Dart has), so this only fires when the *entire* C++ idiom is
/// present and every `begin`/`end` receiver agrees
/// (`container_begin_or_end_receiver`, `same_receiver_ignoring_origin`,
/// both already built for the `find`/`contains` idiom and reused verbatim
/// here). When it matches, the loop lowers to the *same* `Stmt::ForEach`
/// node `CXXForRangeStmt` already produces — `for (final it in x) { ... }`
/// in Dart — so every existing consumer of that node (emission, the
/// function catalog) needs no change at all. Scoped to `vector`/`list`/
/// `set`/`deque`/`multiset` (`container_begin_or_end_receiver`'s own
/// scope); `map`/`unordered_map` iteration needs a `first`/`second` →
/// `key`/`value` translation this function deliberately does not attempt,
/// so a map's `begin`/`end` pair simply fails to match here and falls
/// through to the ordinary bailout, unchanged. Mutating the sequence
/// through the iterator mid-loop (`it = list.erase(it);`,
/// `_List_iterator::operator=`) is equally out of scope — the *shape*
/// matches (this function still recognizes the loop), but that expression
/// inside the body stays its own honest bailout, same as any other
/// unrecognized construct in a loop body already lowered today.
unsafe fn lower_iterator_for_loop(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Stmt> {
    let children = unsafe { collect_children(cursor) };
    let [init_cursor, condition_cursor, increment_cursor, body_cursor] = children.as_slice() else {
        return None;
    };
    // The condition's `CXXOperatorCallExpr` sits inside an `ExprWithCleanups`
    // wrapper when the comparison materializes a temporary (`it !=
    // X.end()`'s `X.end()` result) — confirmed via `-ast-dump`: at the
    // *cursor* level this reports as a generic `CXCursor_UnexposedExpr`,
    // already `unwrap_transparent_value_cursor`'s exact territory.
    let condition_cursor = &unsafe { unwrap_transparent_value_cursor(*condition_cursor) };

    // `init`: `auto it = X.begin();` — a `DeclStmt` wrapping exactly one
    // `VarDecl`, initialized by a recognized `begin`/`cbegin` call.
    if unsafe { clang_sys::clang_getCursorKind(*init_cursor) } != clang_sys::CXCursor_DeclStmt {
        return None;
    }
    let init_children = unsafe { collect_children(*init_cursor) };
    let [it_decl_cursor] = init_children.as_slice() else {
        return None;
    };
    if unsafe { clang_sys::clang_getCursorKind(*it_decl_cursor) } != clang_sys::CXCursor_VarDecl {
        return None;
    }
    let it_name = dart_safe_identifier(&unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(*it_decl_cursor))
    });
    let it_init_candidates: Vec<clang_sys::CXCursor> = unsafe { collect_children(*it_decl_cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
            )
        })
        .collect();
    let [it_init_cursor] = it_init_candidates.as_slice() else {
        return None;
    };
    let begin_receiver =
        unsafe { container_begin_or_end_receiver(*it_init_cursor, true, project_root, origin) }?;
    let elem_ty = match &begin_receiver {
        ir::Expr::Ref { ty, .. } | ir::Expr::FieldAccess { ty, .. } => match ty {
            ir::Type::List(elem) | ir::Type::Set(elem) => (**elem).clone(),
            _ => return None,
        },
        _ => return None,
    };

    // `condition`: `it != X.end()`, the same receiver as `begin` above.
    // `it`'s iterator class overloads `operator!=` (confirmed with a real
    // `clang++ -Xclang -ast-dump`, not assumed: it surfaces as a
    // `CXXOperatorCallExpr` — often resolved via ADL to a hidden-friend free
    // function, `bool operator!=(const _Self&, const _Self&)` in
    // libstdc++ — which the *cursor-level* API reports as an ordinary
    // `CXCursor_CallExpr`, the same shape `container_begin_or_end_receiver`
    // and `stdlib_method_receiver` already treat every overloaded operator
    // call as). Never `CXCursor_BinaryOperator`, which only real built-in
    // `!=` (scalar/pointer operands) uses.
    //
    // Under `-std=c++20` (confirmed the *actual* flag Verovio's own
    // `cmake/CMakeLists.txt` sets — `set(CMAKE_CXX_STANDARD 20)` — not the
    // `-std=c++17` this repo's own unit-test fixtures default to, which is
    // why this loop matched every synthetic fixture but *zero* real
    // occurrences the first time this landed): C++20's rewritten-candidates
    // rule means `it != X.end()` compiles to a call to `operator==`
    // (confirmed real: `libstdc++`'s iterator classes define `==`, not a
    // separate `!=`, and C++20 synthesizes the negation), wrapped in a real
    // `CXXRewrittenBinaryOperator` (itself reported as a transparent
    // `CXCursor_UnexposedExpr`, already unwrapped above) around a genuine
    // `UnaryOperator '!'`. That `!` is *not* sugar — `is_transparent_wrapper`
    // correctly leaves it alone — so it needs its own explicit branch here,
    // structurally equivalent to `lower_find_contains_idiom`'s own
    // `is_negated` handling for the exact same rewritten-`!=`-as-`==` shape.
    let (condition_operator_cursor, expected_name) =
        if unsafe { clang_sys::clang_getCursorKind(*condition_cursor) }
            == clang_sys::CXCursor_UnaryOperator
            && unsafe { clang_sys::clang_getCursorUnaryOperatorKind(*condition_cursor) }
                == clang_sys::CXUnaryOperator_LNot
        {
            let not_children = unsafe { collect_children(*condition_cursor) };
            let [operand_cursor] = not_children.as_slice() else {
                return None;
            };
            (*operand_cursor, "operator==")
        } else {
            (*condition_cursor, "operator!=")
        };
    if unsafe { clang_sys::clang_getCursorKind(condition_operator_cursor) }
        != clang_sys::CXCursor_CallExpr
    {
        return None;
    }
    let condition_referenced =
        unsafe { clang_sys::clang_getCursorReferenced(condition_operator_cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(condition_referenced) } != 0 {
        return None;
    }
    let condition_name = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(condition_referenced))
    };
    if condition_name != expected_name
        || unsafe { clang_sys::clang_Cursor_getNumArguments(condition_operator_cursor) } != 2
    {
        return None;
    }
    let condition_lhs =
        unsafe { clang_sys::clang_Cursor_getArgument(condition_operator_cursor, 0) };
    let condition_rhs =
        unsafe { clang_sys::clang_Cursor_getArgument(condition_operator_cursor, 1) };
    let condition_lhs_referenced = unsafe { clang_sys::clang_getCursorReferenced(condition_lhs) };
    if unsafe { clang_sys::clang_Cursor_isNull(condition_lhs_referenced) } != 0
        || unsafe { clang_sys::clang_equalCursors(condition_lhs_referenced, *it_decl_cursor) } == 0
    {
        return None;
    }
    let condition_rhs = unsafe { unwrap_transparent_value_cursor(condition_rhs) };
    let end_receiver =
        unsafe { container_begin_or_end_receiver(condition_rhs, false, project_root, origin) }?;
    if !same_receiver_ignoring_origin(&begin_receiver, &end_receiver) {
        return None;
    }

    // `increment`: `++it` or `it++` — same overloaded-operator shape as the
    // condition above (`CXCursor_CallExpr`, receiver as argument 0, the
    // `stdlib_method_receiver`/`operator[]` convention), confirmed via the
    // same `-ast-dump`: prefix `operator++` here is a real `CXXMethod`
    // (unlike `!=`'s ADL free function), reached the same way through
    // `clang_getCursorReferenced` on the call cursor.
    if unsafe { clang_sys::clang_getCursorKind(*increment_cursor) } != clang_sys::CXCursor_CallExpr
    {
        return None;
    }
    let increment_referenced = unsafe { clang_sys::clang_getCursorReferenced(*increment_cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(increment_referenced) } != 0 {
        return None;
    }
    let increment_name = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(increment_referenced))
    };
    // Postfix `it++` is a *distinct* overload from prefix `++it`
    // (`operator++(int)`, a dummy `int` parameter purely to disambiguate
    // overload resolution — confirmed via `-ast-dump`) but shares the exact
    // same spelling, `"operator++"`; its call cursor reports 2 arguments
    // (receiver, then the compiler-synthesized dummy `0`), not 1. Since
    // this whole loop shape is discarded and rebuilt as a Dart `for`-each,
    // prefix and postfix are equally fine here — neither's *value* is ever
    // used, only its side effect — so the receiver (always argument 0) is
    // all that matters, not the total argument count.
    if increment_name != "operator++"
        || unsafe { clang_sys::clang_Cursor_getNumArguments(*increment_cursor) } < 1
    {
        return None;
    }
    let increment_receiver = unsafe { clang_sys::clang_Cursor_getArgument(*increment_cursor, 0) };
    let increment_referenced_receiver =
        unsafe { clang_sys::clang_getCursorReferenced(increment_receiver) };
    if unsafe { clang_sys::clang_Cursor_isNull(increment_referenced_receiver) } != 0
        || unsafe { clang_sys::clang_equalCursors(increment_referenced_receiver, *it_decl_cursor) }
            == 0
    {
        return None;
    }

    push_active_iterator_loop(it_name.clone(), elem_ty.clone());
    let body = unsafe { lower_branch(*body_cursor, project_root) };
    pop_active_iterator_loop();

    Some(ir::Stmt::ForEach {
        name: it_name,
        ty: elem_ty,
        is_final: true,
        write_back: false,
        iterable: begin_receiver,
        body,
        origin: origin.clone(),
    })
}

/// `T it = std::find[_if](X.begin(), X.end(), value_or_pred); if (it !=
/// X.end()) { ...*it...it->... } [else { ... }]` — F10/tarefa 13's headline
/// three-statement idiom
/// (`docs/prompts/2026-08-21-13-iteradores-stl.md`): declare, guard,
/// dereference. Unlike `lower_find_contains_idiom` (the same `find`/
/// `find_if` call compared against `end()` *inline*, a pure boolean
/// question), here the iterator is *bound to a name* and dereferenced in a
/// later statement — the found value has to survive across two statements,
/// which Dart's own null safety already models directly: `it` becomes a
/// nullable binding holding the found element (or `null`), the guard
/// becomes a null check, and every `*it`/`it->field` inside the guarded
/// branch resolves through the same `ACTIVE_ITERATOR_LOOPS` registry
/// `lower_iterator_for_loop` already populates for its own loop variable —
/// this idiom just populates it for the guarded `then` branch instead of a
/// loop body.
///
/// Recognized as one whole unit here, not as two independently-dispatched
/// statements, because a partial match would be worse than none (the
/// prompt's own words): a `find`/`find_if` call this doesn't recognize as
/// part of a larger idiom reaches `lower_call_expr`'s free-function
/// dispatch on its own, where `lower_stdlib_algorithm_call`'s honest
/// bailout catches it — never a silent half-translation.
///
/// `decl_cursor`/`if_cursor` must be two adjacent children of the same
/// `CompoundStmt`, in source order (`lower_compound_stmt`'s own lookahead
/// window). `None` for anything that isn't exactly this shape — the caller
/// falls back to lowering each independently.
unsafe fn lower_find_iterator_guard_idiom(
    decl_cursor: clang_sys::CXCursor,
    if_cursor: clang_sys::CXCursor,
    project_root: &Path,
) -> Option<Vec<ir::Stmt>> {
    if unsafe { clang_sys::clang_getCursorKind(decl_cursor) } != clang_sys::CXCursor_DeclStmt {
        return None;
    }
    let decl_children = unsafe { collect_children(decl_cursor) };
    let [it_decl_cursor] = decl_children.as_slice() else {
        return None;
    };
    if unsafe { clang_sys::clang_getCursorKind(*it_decl_cursor) } != clang_sys::CXCursor_VarDecl {
        return None;
    }
    let it_name = dart_safe_identifier(&unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(*it_decl_cursor))
    });
    let it_init_candidates: Vec<clang_sys::CXCursor> = unsafe { collect_children(*it_decl_cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef
                    | clang_sys::CXCursor_NamespaceRef
                    | clang_sys::CXCursor_TemplateRef
            )
        })
        .collect();
    let [it_init_cursor] = it_init_candidates.as_slice() else {
        return None;
    };
    let find_call_cursor = unsafe { unwrap_value_materialization(*it_init_cursor) };
    if unsafe { clang_sys::clang_getCursorKind(find_call_cursor) } != clang_sys::CXCursor_CallExpr {
        return None;
    }
    let find_referenced = unsafe { clang_sys::clang_getCursorReferenced(find_call_cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(find_referenced) } != 0 {
        return None;
    }
    let find_name = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(find_referenced))
    };
    if !matches!(find_name.as_str(), "find" | "find_if")
        || unsafe {
            clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
                find_referenced,
            ))
        } == 0
        || !unsafe { free_function_reachable_from_std(find_referenced) }
        || unsafe { clang_sys::clang_Cursor_getNumArguments(find_call_cursor) } != 3
    {
        return None;
    }

    let origin = stmt_origin(decl_cursor, project_root);
    let begin_cursor = unsafe { clang_sys::clang_Cursor_getArgument(find_call_cursor, 0) };
    let end_cursor_in_find = unsafe { clang_sys::clang_Cursor_getArgument(find_call_cursor, 1) };
    let value_or_pred_cursor = unsafe { clang_sys::clang_Cursor_getArgument(find_call_cursor, 2) };

    let begin_receiver =
        unsafe { container_begin_or_end_receiver(begin_cursor, true, project_root, &origin) }?;
    let end_receiver_in_find = unsafe {
        container_begin_or_end_receiver(end_cursor_in_find, false, project_root, &origin)
    }?;
    if !same_receiver_ignoring_origin(&begin_receiver, &end_receiver_in_find) {
        return None;
    }
    let elem_ty = container_element_type(&begin_receiver)?;

    if unsafe { clang_sys::clang_getCursorKind(if_cursor) } != clang_sys::CXCursor_IfStmt {
        return None;
    }
    let if_children = unsafe { collect_children(if_cursor) };
    let (condition_cursor, then_cursor, else_cursor) = match if_children.as_slice() {
        [condition_cursor, then_cursor] => (*condition_cursor, *then_cursor, None),
        [condition_cursor, then_cursor, else_cursor] => {
            (*condition_cursor, *then_cursor, Some(*else_cursor))
        }
        _ => return None,
    };
    // `it != X.end()` — same C++20 rewritten-`!=`-as-`!(operator==)` shape
    // `lower_iterator_for_loop`'s own condition check already handles (see
    // that function's doc comment for why: a real `CXXRewrittenBinaryOperator`
    // reported as a transparent `CXCursor_UnexposedExpr` wrapping a genuine
    // `UnaryOperator '!'`, which `is_transparent_wrapper` correctly leaves
    // alone).
    let condition_cursor = unsafe { unwrap_transparent_value_cursor(condition_cursor) };
    let (condition_operator_cursor, expected_name) = if unsafe {
        clang_sys::clang_getCursorKind(condition_cursor) == clang_sys::CXCursor_UnaryOperator
    } && unsafe {
        clang_sys::clang_getCursorUnaryOperatorKind(condition_cursor)
    } == clang_sys::CXUnaryOperator_LNot
    {
        let not_children = unsafe { collect_children(condition_cursor) };
        let [operand_cursor] = not_children.as_slice() else {
            return None;
        };
        (*operand_cursor, "operator==")
    } else {
        (condition_cursor, "operator!=")
    };
    if unsafe { clang_sys::clang_getCursorKind(condition_operator_cursor) }
        != clang_sys::CXCursor_CallExpr
    {
        return None;
    }
    let condition_referenced =
        unsafe { clang_sys::clang_getCursorReferenced(condition_operator_cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(condition_referenced) } != 0 {
        return None;
    }
    let condition_name = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(condition_referenced))
    };
    if condition_name != expected_name
        || unsafe { clang_sys::clang_Cursor_getNumArguments(condition_operator_cursor) } != 2
    {
        return None;
    }
    let condition_lhs =
        unsafe { clang_sys::clang_Cursor_getArgument(condition_operator_cursor, 0) };
    let condition_rhs =
        unsafe { clang_sys::clang_Cursor_getArgument(condition_operator_cursor, 1) };
    let condition_lhs_referenced = unsafe { clang_sys::clang_getCursorReferenced(condition_lhs) };
    if unsafe { clang_sys::clang_Cursor_isNull(condition_lhs_referenced) } != 0
        || unsafe { clang_sys::clang_equalCursors(condition_lhs_referenced, *it_decl_cursor) } == 0
    {
        return None;
    }
    let condition_rhs = unsafe { unwrap_transparent_value_cursor(condition_rhs) };
    let end_receiver_condition =
        unsafe { container_begin_or_end_receiver(condition_rhs, false, project_root, &origin) }?;
    if !same_receiver_ignoring_origin(&begin_receiver, &end_receiver_condition) {
        return None;
    }

    // Every structural check passed — build the fused replacement: a
    // nullable binding, then a null-check `If` in place of the original
    // two statements. `elem_ty` can already be `Nullable` on its own (a
    // `vector<Object*>`'s element, real Verovio trigger:
    // `alignfunctor.cpp`'s `std::find_if(... m_timeSpanningElements ...)`
    // over a vector of pointers) — wrapping it in another `Nullable` would
    // double up ("not found" and "found a null pointer" collapse to the
    // same value anyway, so there's no information lost by reusing it) and,
    // worse, print as Dart's invalid `T??` syntax (confirmed the hard way on
    // the real Verovio 6.2.0 corpus: `dart format` refused to parse the
    // result).
    let it_ty = match &elem_ty {
        ir::Type::Nullable(_) => elem_ty.clone(),
        _ => ir::Type::Nullable(Box::new(elem_ty.clone())),
    };
    let init_expr = if find_name == "find" {
        // `X.contains(v) ? v : null` — when found, `*it` and `v` are the
        // same value by construction (`std::find` found `v` itself), so no
        // helper is needed the way `find_if` needs one to locate *which*
        // element matched.
        let value = unsafe { lower_expr(value_or_pred_cursor, project_root) };
        ir::Expr::Conditional {
            condition: Box::new(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(begin_receiver.clone())),
                callee_usr: String::new(),
                callee_name: "contains".to_owned(),
                args: vec![value.clone()],
                ty: ir::Type::Bool,
                origin: origin.clone(),
            }),
            then_expr: Box::new(value),
            else_expr: Box::new(ir::Expr::NullLiteral {
                origin: origin.clone(),
            }),
            ty: it_ty.clone(),
            origin: origin.clone(),
        }
    } else {
        let pred = unsafe { lower_expr(value_or_pred_cursor, project_root) };
        ir::Expr::Call {
            base_qualifier: None,
            target: None,
            callee_usr: String::new(),
            // Must read the same literal name as
            // `emit::dart::FIRST_WHERE_HELPER_NAME`.
            callee_name: "syntaxBridgeFirstWhere".to_owned(),
            args: vec![begin_receiver.clone(), pred],
            ty: it_ty.clone(),
            origin: origin.clone(),
        }
    };

    push_active_iterator_loop(it_name.clone(), elem_ty);
    let then_branch = unsafe { lower_branch(then_cursor, project_root) };
    pop_active_iterator_loop();
    let else_branch = match else_cursor {
        Some(else_cursor) => unsafe { lower_branch(else_cursor, project_root) },
        None => Vec::new(),
    };

    Some(vec![
        ir::Stmt::VarDecl {
            name: it_name.clone(),
            ty: it_ty.clone(),
            init: Some(init_expr),
            origin: origin.clone(),
        },
        ir::Stmt::If {
            condition: ir::Expr::Binary {
                op: ir::BinaryOp::Ne,
                lhs: Box::new(ir::Expr::Ref {
                    name: it_name,
                    ty: it_ty,
                    origin: origin.clone(),
                }),
                rhs: Box::new(ir::Expr::NullLiteral {
                    origin: origin.clone(),
                }),
                ty: ir::Type::Bool,
                origin: origin.clone(),
            },
            then_branch,
            else_branch,
            origin,
        },
    ])
}

/// Whether `decl` (a `FunctionDecl` already confirmed to live in a system
/// header) is reachable from `std` — either declared directly inside
/// `namespace std` (or one of its inline namespaces, e.g. libstdc++'s
/// `std::__cxx11` for `to_string`, confirmed via `clang++ -Xclang
/// -ast-dump`: walked the same way `stdlib_template_name` already walks a
/// template's ancestors) or declared directly in the *global* namespace and
/// reached only through a `using` declaration `std` itself resolves through
/// (confirmed for `std::abs(int)`: libstdc++'s `<cstdlib>` does `using
/// ::abs;`, so `clang_getCursorReferenced` on the call already resolves
/// straight past the `UsingShadowDecl` to glibc's real, global-scope `::abs`
/// — there is no libclang API left, by that point, to ask "was this reached
/// via `std::`", so a global-scope declaration is accepted outright here).
/// The false-positive this can't rule out — a project's own vendored
/// third-party header happening to declare an unrelated global `abs`/`max`/…
/// and *also* being clang-flagged as a system header (`-isystem`) — is the
/// same pre-existing risk `gcd`'s own guard already carries; not new.
unsafe fn free_function_reachable_from_std(decl: clang_sys::CXCursor) -> bool {
    let mut ancestor = unsafe { clang_sys::clang_getCursorSemanticParent(decl) };
    // Whether the walk has crossed a *named, non-`std`* namespace before
    // either finding `std` or running out of ancestors. `extern "C" { ... }`
    // blocks (`CXCursor_LinkageSpec`, glibc's real wrapping for `::abs`)
    // carry no name of their own and are skipped transparently rather than
    // tripping this — a bare global-scope declaration (no namespace at all)
    // still counts as reachable.
    let mut saw_other_namespace = false;
    loop {
        if unsafe { clang_sys::clang_Cursor_isNull(ancestor) } != 0
            || unsafe { clang_sys::clang_getCursorKind(ancestor) }
                == clang_sys::CXCursor_TranslationUnit
        {
            return !saw_other_namespace;
        }
        if unsafe { clang_sys::clang_getCursorKind(ancestor) } == clang_sys::CXCursor_Namespace {
            let name = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(ancestor))
            };
            if name == "std" {
                return true;
            }
            saw_other_namespace = true;
        }
        ancestor = unsafe { clang_sys::clang_getCursorSemanticParent(ancestor) };
    }
}

/// Every free-function name F6/tarefa 07's Metade A gives a real Dart
/// translation to — `lower_stdlib_free_function_call`'s own curated list,
/// plus `swap` (bridged separately, at the statement level, by
/// `lower_std_swap_stmt`, since it mutates both operands rather than
/// producing a single value). Shared with
/// `function_catalog::record_call` (via `is_bridged_stdlib_free_function`
/// below) so Metade B's external-boundary auto-detection never *also*
/// mocks a symbol Metade A already translates for real — that would emit
/// an unused, dead mock function alongside the real call and risk `dart
/// analyze`'s `unused_element`.
const BRIDGED_STDLIB_FREE_FUNCTION_NAMES: &[&str] =
    &["gcd", "max", "min", "abs", "to_string", "make_pair", "swap"];

/// Whether `referenced` (a resolved call target already confirmed to be a
/// `CXCursor_FunctionDecl`) is one of Metade A's curated `std::` adapters —
/// see `BRIDGED_STDLIB_FREE_FUNCTION_NAMES`'s own doc comment for why
/// `function_catalog::record_call` needs this exclusion.
pub(crate) unsafe fn is_bridged_stdlib_free_function(referenced: clang_sys::CXCursor) -> bool {
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    BRIDGED_STDLIB_FREE_FUNCTION_NAMES.contains(&name.as_str())
        && unsafe {
            clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(
                referenced,
            ))
        } != 0
        && unsafe { free_function_reachable_from_std(referenced) }
}

/// `std` free functions with a direct, unconditional Dart equivalent — no
/// per-project mapping decision needed, the same way `lower_stdlib_operator_call`
/// below needs none: every one of these names means the exact same thing in
/// every C++ program that spells it, so there is no "which Dart target did
/// the user mean" question to ask (unlike, say, a record's own method,
/// where US-7's `mapping::MappingDecision` exists precisely because the
/// answer depends on the project). Gated on `clang_Location_isInSystemHeader`
/// and the owning namespace being `std`, the same two-part guard
/// `lower_stdlib_operator_call` uses, so a project's own free function named
/// `max`/`abs`/`swap`/… is never mistaken for the standard library's.
///
/// Once a name matches and the guard passes, this is committed to being
/// `std::<name>` — an argument shape this doesn't recognize falls to
/// `Expr::UnsupportedTyped` rather than `None`, which would let the call
/// fall through to the generic path below and print a literal, undefined
/// Dart identifier (F6/tarefa 07's whole premise: "silêncio é proibido"
/// applies here exactly as it does to `lower_stdlib_method_call`).
unsafe fn lower_stdlib_free_function_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    callee_name: &str,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    if !BRIDGED_STDLIB_FREE_FUNCTION_NAMES.contains(&callee_name) || callee_name == "swap" {
        return None;
    }
    if unsafe {
        clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(referenced))
    } == 0
    {
        return None;
    }
    if !unsafe { free_function_reachable_from_std(referenced) } {
        return None;
    }

    let callee_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced)) };
    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
    let unsupported = || ir::Expr::UnsupportedTyped {
        reason: format!("unsupported argument shape for std::{callee_name}"),
        ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
        origin: origin.clone(),
    };

    match callee_name {
        "gcd" => {
            if arg_count != 2 {
                return None;
            }
            let lhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
            let rhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 1) };
            let target = unsafe { lower_expr(lhs_cursor, project_root) };
            let arg = unsafe { lower_expr(rhs_cursor, project_root) };
            // Not `lower_type(clang_getCursorType(call_cursor))`: `std::gcd`'s
            // real C++ return type is `std::common_type_t<M, N>`, a
            // library-internal alias `lower_type` can't resolve to anything
            // meaningful (confirmed the hard way — E13's own
            // `Fraction::Reduce` regressed to a whole-body bailout once
            // `lower_type` stopped mis-resolving an unrecognized
            // system-header type as a bogus `Type::Record`). This bridge
            // already asserts the semantic fact that Dart's `int.gcd()` is
            // the exact match — for the two-`int`-argument shape it's gated
            // to, that method always returns `int`, so the result is typed
            // directly rather than through the alien C++ type expression.
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr,
                callee_name: "gcd".to_owned(),
                args: vec![arg],
                ty: ir::Type::Int,
                origin: origin.clone(),
            })
        }
        // `std::max(a, b)`/`std::min(a, b)` — Dart 3's `dart:math` exposes
        // the exact same two-argument free function under the same name
        // (`math.max`/`math.min`, confirmed against real `dart analyze`),
        // so this is a straight rename rather than a method-call rewrite
        // the way `gcd` needs. `std::max`/`min`'s real return type is
        // `const T&`; `lower_type` already strips `CXType_LValueReference`
        // down to `T` (see its own `CXType_LValueReference` branch), so the
        // call cursor's own type is safe to use directly here, unlike
        // `gcd`'s alien `common_type_t`.
        "max" | "min" => {
            if arg_count != 2 {
                return Some(unsupported());
            }
            let lhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
            let rhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 1) };
            let lhs = unsafe { lower_expr(lhs_cursor, project_root) };
            let rhs = unsafe { lower_expr(rhs_cursor, project_root) };
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: None,
                callee_usr,
                callee_name: format!("math.{callee_name}"),
                args: vec![lhs, rhs],
                ty,
                origin: origin.clone(),
            })
        }
        // `std::abs(x)` — Dart's numeric types carry this as an instance
        // method (`x.abs()`), not a top-level function, the same shape
        // `gcd` already bridges to `a.gcd(b)`.
        "abs" => {
            if arg_count != 1 {
                return Some(unsupported());
            }
            let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
            let target = unsafe { lower_expr(arg_cursor, project_root) };
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr,
                callee_name: "abs".to_owned(),
                args: Vec::new(),
                ty,
                origin: origin.clone(),
            })
        }
        // `std::to_string(x)` — Dart's `.toString()` is the same instance
        // method every value already carries, no `dart:core` import needed.
        "to_string" => {
            if arg_count != 1 {
                return Some(unsupported());
            }
            let arg_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
            let target = unsafe { lower_expr(arg_cursor, project_root) };
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(target)),
                callee_usr,
                callee_name: "toString".to_owned(),
                args: Vec::new(),
                ty: ir::Type::Str,
                origin: origin.clone(),
            })
        }
        // `std::make_pair(a, b)` — `Type::Pair`'s own Dart representation,
        // `SyntaxBridgePair` (`emit::dart::PAIR_TYPE_NAME`), already exists
        // for the *type* (used by `mock_value_for_type`/pointee-shape
        // lookups); this is the first place that actually *constructs* one
        // from a live pair of values, so `make_pair`'s two arguments become
        // its two constructor arguments directly.
        "make_pair" => {
            if arg_count != 2 {
                return Some(unsupported());
            }
            let lhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
            let rhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 1) };
            let lhs = unsafe { lower_expr(lhs_cursor, project_root) };
            let rhs = unsafe { lower_expr(rhs_cursor, project_root) };
            let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });
            let ir::Type::Pair(first_ty, second_ty) = ty else {
                return Some(unsupported());
            };
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: None,
                callee_usr,
                // Must read the same literal name as
                // `emit::dart::PAIR_TYPE_NAME`.
                callee_name: "SyntaxBridgePair".to_owned(),
                args: vec![lhs, rhs],
                ty: ir::Type::Pair(first_ty, second_ty),
                origin: origin.clone(),
            })
        }
        _ => None,
    }
}

/// The element type a `begin`/`end`-recognized container receiver
/// (`container_begin_or_end_receiver`'s own output shape) carries — `None`
/// for a receiver whose lowered `Expr`/`Type` combination isn't one of the
/// two shapes that function ever actually produces. Factored out of
/// `lower_iterator_for_loop`'s own inline match (kept there unchanged) since
/// `lower_stdlib_algorithm_call` below needs the exact same extraction for
/// `count_if`'s intermediate `.where(...)` node.
fn container_element_type(receiver: &ir::Expr) -> Option<ir::Type> {
    match receiver {
        ir::Expr::Ref { ty, .. } | ir::Expr::FieldAccess { ty, .. } => match ty {
            ir::Type::List(elem) | ir::Type::Set(elem) => Some((**elem).clone()),
            _ => None,
        },
        _ => None,
    }
}

/// `std` algorithm free functions whose first two arguments are a
/// `begin()`/`end()` pair on the exact same container — the STL "whole
/// idiom" family F10/tarefa 13 targets
/// (`docs/prompts/2026-08-21-13-iteradores-stl.md`). Every one of these has
/// a direct, single-expression Dart equivalent once the begin/end pair is
/// recognized as "the whole container", the same
/// `container_begin_or_end_receiver`/`same_receiver_ignoring_origin`
/// machinery `lower_find_contains_idiom`/`lower_iterator_for_loop` already
/// use for the same reason.
///
/// `find`/`find_if`/`remove_if` are deliberately *not* given a real
/// translation here: `find`/`find_if` only have a sound one as part of a
/// larger idiom (compared against `end()` — `lower_find_contains_idiom` — or
/// declared, then guarded, then dereferenced — the compound-statement
/// fusion in `lower_compound_stmt`), both of which intercept the call
/// *before* it ever reaches here as a bare `FunctionDecl` call, structurally
/// walking the relevant cursors themselves rather than going through
/// `lower_call_expr`. `remove_if` only has one, paired with `.erase(...)`
/// (`lower_stdlib_method_call`'s own `"erase"` arm). Reaching this function
/// with one of those three names bare means neither recognized shape
/// matched — an honest, typed bailout, never a fall-through to the generic
/// path below that would print a literal, undefined-in-Dart call (this
/// whole family's own root cause, per the prompt's own evidence: `find_if`
/// printed as a literal call because `is_plain_dart_identifier("find_if")`
/// is true).
unsafe fn lower_stdlib_algorithm_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    callee_name: &str,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    if !matches!(
        callee_name,
        "find" | "find_if" | "count_if" | "sort" | "for_each" | "remove_if"
    ) {
        return None;
    }
    if unsafe {
        clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(referenced))
    } == 0
    {
        return None;
    }
    if !unsafe { free_function_reachable_from_std(referenced) } {
        return None;
    }

    let unsupported = || ir::Expr::UnsupportedTyped {
        reason: format!("unsupported use of std::{callee_name} — not part of a recognized idiom"),
        ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
        origin: origin.clone(),
    };

    if matches!(callee_name, "find" | "find_if" | "remove_if") {
        return Some(unsupported());
    }

    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
    if arg_count < 2 {
        return Some(unsupported());
    }
    let begin_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
    let end_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 1) };
    let (Some(begin_receiver), Some(end_receiver)) = (
        unsafe { container_begin_or_end_receiver(begin_cursor, true, project_root, origin) },
        unsafe { container_begin_or_end_receiver(end_cursor, false, project_root, origin) },
    ) else {
        return Some(unsupported());
    };
    if !same_receiver_ignoring_origin(&begin_receiver, &end_receiver) {
        return Some(unsupported());
    }

    match callee_name {
        "count_if" => {
            if arg_count != 3 {
                return Some(unsupported());
            }
            let Some(elem_ty) = container_element_type(&begin_receiver) else {
                return Some(unsupported());
            };
            let pred_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 2) };
            let pred = unsafe { lower_expr(pred_cursor, project_root) };
            Some(ir::Expr::FieldAccess {
                target: Box::new(ir::Expr::Call {
                    base_qualifier: None,
                    target: Some(Box::new(begin_receiver)),
                    callee_usr: String::new(),
                    callee_name: "where".to_owned(),
                    args: vec![pred],
                    ty: ir::Type::List(Box::new(elem_ty)),
                    origin: origin.clone(),
                }),
                field: "length".to_owned(),
                ty: ir::Type::Int,
                origin: origin.clone(),
            })
        }
        "sort" if arg_count == 2 => Some(ir::Expr::Call {
            base_qualifier: None,
            target: Some(Box::new(begin_receiver)),
            callee_usr: String::new(),
            callee_name: "sort".to_owned(),
            args: Vec::new(),
            ty: ir::Type::Void,
            origin: origin.clone(),
        }),
        "sort" if arg_count == 3 => {
            let cmp_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 2) };
            let cmp = unsafe { lower_expr(cmp_cursor, project_root) };
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(begin_receiver)),
                callee_usr: String::new(),
                callee_name: "sort".to_owned(),
                args: vec![cmp],
                ty: ir::Type::Void,
                origin: origin.clone(),
            })
        }
        "for_each" if arg_count == 3 => {
            let fn_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 2) };
            let f = unsafe { lower_expr(fn_cursor, project_root) };
            Some(ir::Expr::Call {
                base_qualifier: None,
                target: Some(Box::new(begin_receiver)),
                callee_usr: String::new(),
                callee_name: "forEach".to_owned(),
                args: vec![f],
                ty: ir::Type::Void,
                origin: origin.clone(),
            })
        }
        _ => Some(unsupported()),
    }
}

/// A call to a *free* operator overload (`"a" + b`, `a == b`) whose operand
/// types make it C++'s `std::string` `operator+`/`operator==`/`operator!=`
/// — these resolve to ordinary library functions (`referenced_kind ==
/// FunctionDecl`, confirmed via `clang -Xclang -ast-dump`: `Function
/// 'operator+' ...`), not methods, so `lower_stdlib_method_call` never sees
/// them. Dart's `String` already overloads `+`/`==` with the same meaning
/// (`==` needs no help at all; `+` needs no bridge either — only
/// `.size()`'s *count* differs, not concatenation), so these translate
/// directly to `Expr::Binary` instead of a `Call` to a function named
/// "operator+" that doesn't exist in Dart. `None` for any operator this
/// doesn't recognize, or a user-defined overload (gated on
/// `clang_Location_isInSystemHeader` so a project's own `operator+` is never
/// mistaken for the standard library's) — including a non-string use of
/// `+`/`==`/`!=` that some future degrau might introduce, since only one
/// operand needs to resolve to `Type::Str` for this to trigger.
unsafe fn lower_stdlib_operator_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    callee_name: &str,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    let op = match callee_name {
        "operator+" => ir::BinaryOp::Add,
        "operator==" => ir::BinaryOp::Eq,
        "operator!=" => ir::BinaryOp::Ne,
        _ => return None,
    };
    if unsafe {
        clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(referenced))
    } == 0
    {
        return None;
    }

    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
    if arg_count != 2 {
        return None;
    }
    let lhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
    let rhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 1) };
    let lhs_ty = lower_type(unsafe { clang_sys::clang_getCursorType(lhs_cursor) });
    let rhs_ty = lower_type(unsafe { clang_sys::clang_getCursorType(rhs_cursor) });
    if lhs_ty != ir::Type::Str && rhs_ty != ir::Type::Str {
        return None;
    }

    let lhs = unsafe { lower_expr(lhs_cursor, project_root) };
    let rhs = unsafe { lower_expr(rhs_cursor, project_root) };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });
    Some(ir::Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        ty,
        origin: origin.clone(),
    })
}

/// A user `Record`'s own operator method, called with C++'s infix syntax
/// (`a + b`) — `lower_stdlib_operator_call`'s counterpart for a `CXXMethod`
/// operator instead of a free-function one (`std::string`'s `operator+`
/// there is non-member/ADL, so it's never reached via this path). `None`
/// for any operator outside Dart's own overloadable set (`mapping::
/// DART_OPERATOR_SYMBOLS`) — matching `lower_method_call`'s existing
/// bail-out for those (`operador-sem-equivalente-direto`, not built yet).
unsafe fn lower_record_operator_call(
    call_cursor: clang_sys::CXCursor,
    callee_name: &str,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    let op = match callee_name {
        "operator+" => ir::BinaryOp::Add,
        "operator-" => ir::BinaryOp::Sub,
        "operator*" => ir::BinaryOp::Mul,
        "operator/" => ir::BinaryOp::Div,
        "operator==" => ir::BinaryOp::Eq,
        "operator!=" => ir::BinaryOp::Ne,
        "operator<" => ir::BinaryOp::Lt,
        "operator<=" => ir::BinaryOp::Le,
        "operator>" => ir::BinaryOp::Gt,
        "operator>=" => ir::BinaryOp::Ge,
        _ => return None,
    };

    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
    if arg_count != 2 {
        return None;
    }
    let lhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
    let rhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 1) };
    let lhs = unsafe { lower_expr(lhs_cursor, project_root) };
    let rhs = unsafe { lower_expr(rhs_cursor, project_root) };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });
    Some(ir::Expr::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        ty,
        origin: origin.clone(),
    })
}

/// `obj.method(args)` or, from inside another method, a bare `method(args)`
/// referring to `this` implicitly — both shapes are a `CXCursor_CallExpr`
/// whose first child is a `CXCursor_MemberRefExpr` (confirmed with
/// `clang -Xclang -ast-dump`, not guessed: a member call is
/// `CXXMemberCallExpr` in Clang's own AST, but `libclang` normalizes it to
/// the same simplified `CallExpr` cursor kind free calls get).
///
/// `a == b` (E13, a user-defined operator called with infix syntax from
/// outside its class) is a *different* shape sharing the same `CallExpr`
/// cursor kind: Clang's `CXXOperatorCallExpr` never folds the receiver into
/// a `MemberRefExpr` the way `a.op(b)` does — the receiver is the call's own
/// first *argument* instead, confirmed empirically (not guessed): for
/// `a == b`, `clang_Cursor_getNumArguments` reports `2`, with
/// `clang_Cursor_getArgument(0)` resolving to `a` and `(1)` to `b`, while
/// `collect_children` reports three `UnexposedExpr` children (the callee
/// reference, then the same two arguments) with no `MemberRefExpr` at all.
/// This confirms a hypothesis this function's own text already carried
/// before E13 existed.
unsafe fn lower_method_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    // `this->m_point = Ponto(1, 2);` (F5/tarefa 08) breaks the assumption
    // this function's own doc comment above states as fact: a `CXXOperatorCallExpr`
    // does *not* always spell its receiver as an `UnexposedExpr`-wrapped
    // argument, distinct in kind from `MemberRefExpr`. When the receiver
    // itself is a field accessed through implicit `this`, `collect_children`
    // reports that `MemberRefExpr` directly as `call_cursor`'s first child —
    // confirmed empirically (not assumed) by instrumenting this exact call
    // with the real Verovio-shaped fixture below: `first_child`'s kind was
    // `102` (`CXCursor_MemberRefExpr`) for `m_point = Ponto(1, 2)` inside a
    // constructor, identical to the kind a genuine `obj.method()` receiver
    // reports. The old kind-only check therefore misread the assignment's
    // *first argument* as a method-call *receiver*, producing a free
    // two-argument `assignFrom(m_point, Ponto(1, 2))` instead of
    // `m_point.assignFrom(Ponto(1, 2))` — the real corpus's exact `assignFrom`
    // shape (`docs/plans/dart-analyze-verovio-6.2.0.md`'s F5).
    //
    // The operator's own name resolves the ambiguity `first_child`'s kind
    // alone can't: `referenced`'s spelling starting with "operator" already
    // means (per this function's own comment on `a == b`) the receiver is
    // always `clang_Cursor_getArgument(call_cursor, 0)`, regardless of what
    // shape that argument's expression happens to have. `operator()`
    // (functor calls) shares this shape too. A user-defined conversion
    // operator is the one exception whose raw spelling also starts with
    // "operator" (`"operator std::string"`) yet is invoked with genuine
    // `obj.method()` call syntax, receiver truly folded into a
    // `MemberRefExpr` — excluded by its own distinct cursor kind, checked
    // directly rather than inferred from the spelling.
    let raw_callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    let is_operator_syntax_call = unsafe { clang_sys::clang_getCursorKind(referenced) }
        != clang_sys::CXCursor_ConversionFunction
        && raw_callee_name.starts_with("operator");

    let (target, arg_skip, base_qualifier) = if is_operator_syntax_call {
        let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
        if arg_count < 1 {
            return ir::Expr::UnsupportedTyped {
                reason: "operator call had no receiver argument".to_owned(),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin,
            };
        }
        let receiver_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
        (
            unsafe { lower_expr(receiver_cursor, project_root) },
            1,
            None,
        )
    } else {
        let receiver_children = unsafe { collect_children(call_cursor) };
        let Some(first_child) = receiver_children.first() else {
            return ir::Expr::UnsupportedTyped {
                reason: "method call had no receiver expression".to_owned(),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin,
            };
        };
        if unsafe { clang_sys::clang_getCursorKind(*first_child) }
            == clang_sys::CXCursor_MemberRefExpr
        {
            // `Base::foo()`/`this->Base::foo()` (F12/tarefa 09) — only a
            // qualified call on an *implicit* `this` receiver has a Dart
            // `super.` equivalent at all; `obj.Base::foo()` on an explicit,
            // different object has none (Dart has no way to pick a specific
            // ancestor implementation through an arbitrary reference), so
            // this stays disqualified even though `member_ref_qualifier_base`
            // still reports the same `TypeRef`. A qualifier naming *this
            // very record* (a non-virtual self-qualified call, still
            // genuinely recursive the same way in Dart as in C++) is left
            // alone exactly like an unqualified call; only a qualifier
            // naming a *different* record, on the implicit receiver, is a
            // real base-qualified call, whose dispatch `super.`/a bailout
            // has to capture — see `Expr::Call::base_qualifier`'s own doc
            // comment.
            let receiver = unsafe { member_ref_receiver(*first_child, project_root, &origin) };
            let qualifier = unsafe { member_ref_qualifier_base(*first_child) };
            let base_qualifier = qualifier.filter(|base| {
                matches!(receiver, ir::Expr::This { .. })
                    && active_method_owner_usr().as_deref() != Some(base.usr.as_str())
            });
            (receiver, 0, base_qualifier)
        } else {
            let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
            if arg_count < 1 {
                return ir::Expr::UnsupportedTyped {
                    reason: "operator call had no receiver argument".to_owned(),
                    ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                    origin,
                };
            }
            let receiver_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
            (
                unsafe { lower_expr(receiver_cursor, project_root) },
                1,
                None,
            )
        }
    };

    let callee_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced)) };
    if callee_usr.is_empty() {
        return ir::Expr::UnsupportedTyped {
            reason: "resolved method call target has no stable identity".to_owned(),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
            origin,
        };
    }
    // A functor call (`pred(a, b)`) reaches this same operator-syntax
    // branch as `a == b` (E13) does — `emit::dart`'s own bridge for the
    // *declaration* of `operator()` renames it to Dart's `call` method
    // (`emit::dart::emit_method`), so the call site has to agree, or the
    // two would name different methods.
    //
    // A conversion operator (`t.operator std::string()`/`t.operator bool()`)
    // reaches here too: its real spelling ("operator basic_string"/
    // "operator std::string", confirmed empirically) contains characters no
    // Dart identifier can — `conversion_operator_dart_method_name` gives the
    // exact same synthesized name `lower_method` gives the *declaration*, so
    // a call always finds it. `lower_call_expr` already refused to dispatch
    // here at all for a target type this doesn't name (see its own
    // `CXCursor_ConversionFunction` branch); the raw (unusable-as-an-
    // identifier) spelling is kept as a fallback rather than panicking, in
    // the defensive case some other future call path reaches this function
    // directly with a target type this module still doesn't name.
    let callee_name = if raw_callee_name == "operator()" {
        "call".to_owned()
    } else if unsafe { clang_sys::clang_getCursorKind(referenced) }
        == clang_sys::CXCursor_ConversionFunction
    {
        let target_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(referenced) });
        conversion_operator_dart_method_name(&target_type)
            .map(str::to_owned)
            .unwrap_or(raw_callee_name)
    } else {
        raw_callee_name
    };
    if callee_name.starts_with("operator") {
        // F13/tarefa 12 (`docs/prompts/2026-08-21-12-overloads-const-e-colisoes-de-nome.md`):
        // a call to a member *operator template* instantiation
        // (`template <class T> Object& operator<<(const T&)`, jsonxx's real
        // shape) reaches here with `referenced` already resolved to the
        // instantiation, but this project has no member-template
        // monomorphization (only the free-function path does, via
        // `monomorphized_template_name` above `lower_stdlib_operator_call`)
        // — the template itself is never lowered into a real `ir::Method`,
        // so `dart_operator_bridge_name(&callee_name, ...)` below would
        // print a bridge name (`"streamInsert"`, purely from the symbol and
        // arity) that names no declaration at all. Confirmed live on the
        // real Verovio 6.2.0 corpus: unrelated *other* overloads of the same
        // operator symbol on the same class used to share that exact bridge
        // name (`duplicate_definition`, this family's own achado), which
        // coincidentally gave this call *something* to resolve against;
        // disambiguating those by type (this task) removes that accidental
        // stand-in and turns the latent gap into a loud `undefined_method` —
        // "reaching the declaration but not every call site", the failure
        // mode this task's own method explicitly tests for. Bailing here
        // explicitly, the same honest-failure shape every other
        // not-yet-lowered construct in this module already uses, is the fix
        // that generalizes: it holds regardless of whether a same-named
        // sibling happens to exist to accidentally mask it.
        if unsafe {
            clang_sys::clang_Cursor_isNull(clang_sys::clang_getSpecializedCursorTemplate(
                referenced,
            ))
        } == 0
        {
            return ir::Expr::UnsupportedTyped {
                reason: "call to a member operator template instantiation — not yet monomorphized"
                    .to_owned(),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin,
            };
        }
        let args =
            match unsafe { lower_call_arguments_skipping(call_cursor, arg_skip, project_root) } {
                Some(args) => args,
                None => {
                    return ir::Expr::UnsupportedTyped {
                        reason: "could not enumerate named operator bridge arguments".to_owned(),
                        ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                        origin,
                    };
                }
            };
        let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });
        return ir::Expr::Call {
            base_qualifier,
            target: Some(Box::new(target)),
            callee_usr,
            callee_name: dart_operator_bridge_name(&callee_name, args.len()).to_owned(),
            args,
            ty,
            origin,
        };
    }
    // Any other operator-syntax call this module doesn't specifically
    // recognize (`lower_record_operator_call` already intercepted the ones
    // Dart maps directly) has no bare-identifier spelling `emit::dart` could
    // print as a call target — same guard as the free-function fallback
    // above, and for the same reason.
    if !is_plain_dart_identifier(&callee_name) {
        return ir::Expr::UnsupportedTyped {
            reason: format!("unsupported operator method call: {callee_name}"),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
            origin,
        };
    }
    let args = match unsafe { lower_call_arguments_skipping(call_cursor, arg_skip, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::UnsupportedTyped {
                reason: "could not enumerate method call arguments".to_owned(),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin,
            };
        }
    };
    let args = unsafe { regroup_variadic_call_args(args, referenced, &origin) };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });

    ir::Expr::Call {
        base_qualifier,
        target: Some(Box::new(target)),
        callee_usr,
        callee_name,
        args,
        ty,
        origin,
    }
}

/// `vrv::Fraction::Reduce(a, b);` (E13) — a static method called from
/// outside its class, shaped exactly like a free-function call (no receiver
/// argument to skip, unlike `lower_method_call`'s operator-call branch).
/// `target` is a synthetic `Expr::Ref` naming the owning class: `emit::dart`
/// already renders `Some(receiver).method(args)` for any `Expr::Call` target
/// by printing `{receiver}.{callee}(args)`, and a `Ref` emits as its bare
/// `name` — so a `Ref` naming the class itself produces exactly Dart's own
/// `ClassName.method(args)` static-call syntax, and registers the class as a
/// real dependency for `emit::dart`'s own import collection (E11), which is
/// correct: the call site genuinely needs that class in scope.
unsafe fn lower_static_method_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let owner_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(owner)) };
    let owner_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(owner)) };
    if owner_usr.is_empty() || owner_name.is_empty() {
        return ir::Expr::UnsupportedTyped {
            reason: "static method's owning class has no stable identity".to_owned(),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
            origin,
        };
    }

    let callee_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced)) };
    if callee_usr.is_empty() {
        return ir::Expr::UnsupportedTyped {
            reason: "resolved static method call target has no stable identity".to_owned(),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
            origin,
        };
    }
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    let args = match unsafe { lower_call_arguments(call_cursor, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::UnsupportedTyped {
                reason: "could not enumerate static method call arguments".to_owned(),
                ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
                origin,
            };
        }
    };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });

    ir::Expr::Call {
        base_qualifier: None,
        target: Some(Box::new(ir::Expr::Ref {
            name: owner_name.clone(),
            ty: ir::Type::Record {
                usr: owner_usr,
                name: owner_name,
            },
            origin: origin.clone(),
        })),
        callee_usr,
        callee_name,
        args,
        ty,
        origin,
    }
}

/// `ClassName(args)` / `ClassName varName(args)` reaching a real,
/// user-bodied constructor — see `Expr::ConstructorCall`'s docs on why the
/// ordinal (not the constructor's own, nonexistent, name) is what identifies
/// it, and `constructor_ordinal` for how that ordinal is computed the exact
/// same way both here and in `lower_record`'s own constructor collection, so
/// the two can never disagree about which constructor is "the primary one".
unsafe fn lower_constructor_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    project_root: &Path,
    origin: ir::Origin,
) -> ir::Expr {
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let type_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(owner)) };
    let type_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(owner)) };
    if type_usr.is_empty() || type_name.is_empty() {
        return ir::Expr::UnsupportedTyped {
            reason: "constructor's owning class has no stable identity".to_owned(),
            ty: lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) }),
            origin,
        };
    }

    let constructor_index = unsafe { constructor_ordinal(owner, referenced) };
    let args = match unsafe { lower_call_arguments(call_cursor, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::UnsupportedTyped {
                reason: "could not enumerate constructor call arguments".to_owned(),
                ty: ir::Type::Record {
                    usr: type_usr,
                    name: type_name,
                },
                origin,
            };
        }
    };

    ir::Expr::ConstructorCall {
        type_usr,
        type_name,
        constructor_index,
        args,
        origin,
    }
}

/// Where `target` sits, in declaration order, among `owner`'s own
/// non-copy/non-move constructors — `0` for the first one declared, `1` for
/// the second, and so on. Walks *declarations* (`collect_children(owner)`,
/// which only sees in-class prototypes), not *definitions*: an out-of-line
/// constructor definition is a separate top-level cursor sharing the same
/// `usr` as its in-class prototype (matched by `usr`, `libclang`'s
/// position-independent identity), so counting declarations is what stays
/// correct regardless of whether a constructor is defined inline or out of
/// line. Compiler-generated copy/move constructors are skipped — E04 has no
/// fixture that declares its own, and counting the implicit ones would shift
/// every real constructor's ordinal by however many of those `libclang`
/// happens to synthesize.
unsafe fn constructor_ordinal(owner: clang_sys::CXCursor, target: clang_sys::CXCursor) -> usize {
    let target_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(target)) };

    let mut index = 0;
    for child in unsafe { collect_children(owner) } {
        if unsafe { clang_sys::clang_getCursorKind(child) } != clang_sys::CXCursor_Constructor {
            continue;
        }
        if unsafe { is_copy_or_move_constructor(child) } {
            continue;
        }
        let child_usr =
            unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(child)) };
        if child_usr == target_usr {
            return index;
        }
        index += 1;
    }
    0
}

unsafe fn is_copy_or_move_constructor(cursor: clang_sys::CXCursor) -> bool {
    unsafe {
        clang_sys::clang_CXXConstructor_isCopyConstructor(cursor) != 0
            || clang_sys::clang_CXXConstructor_isMoveConstructor(cursor) != 0
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
        clang_sys::CXBinaryOperator_LOr => Some(ir::BinaryOp::Or),
        clang_sys::CXBinaryOperator_Shl => Some(ir::BinaryOp::ShiftLeft),
        clang_sys::CXBinaryOperator_Shr => Some(ir::BinaryOp::ShiftRight),
        clang_sys::CXBinaryOperator_And => Some(ir::BinaryOp::BitAnd),
        clang_sys::CXBinaryOperator_Xor => Some(ir::BinaryOp::BitXor),
        clang_sys::CXBinaryOperator_Or => Some(ir::BinaryOp::BitOr),
        _ => None,
    }
}

fn lower_unary_op(kind: clang_sys::CXUnaryOperatorKind) -> Option<ir::UnaryOp> {
    match kind {
        clang_sys::CXUnaryOperator_Minus => Some(ir::UnaryOp::Neg),
        clang_sys::CXUnaryOperator_LNot => Some(ir::UnaryOp::Not),
        clang_sys::CXUnaryOperator_PreInc => Some(ir::UnaryOp::PreIncrement),
        clang_sys::CXUnaryOperator_PreDec => Some(ir::UnaryOp::PreDecrement),
        clang_sys::CXUnaryOperator_PostInc => Some(ir::UnaryOp::PostIncrement),
        clang_sys::CXUnaryOperator_PostDec => Some(ir::UnaryOp::PostDecrement),
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

unsafe fn evaluate_float_eval_result(cursor: clang_sys::CXCursor) -> Option<f64> {
    let result = unsafe { clang_sys::clang_Cursor_Evaluate(cursor) };
    if result.is_null() {
        return None;
    }

    let kind = unsafe { clang_sys::clang_EvalResult_getKind(result) };
    let value = if kind == clang_sys::CXEval_Float {
        Some(unsafe { clang_sys::clang_EvalResult_getAsDouble(result) })
    } else {
        None
    };

    unsafe {
        clang_sys::clang_EvalResult_dispose(result);
    }

    value
}

/// The literal's own source text, unescaped — unlike int/float literals,
/// `clang_Cursor_Evaluate` returns null for a bare `CXCursor_StringLiteral`
/// cursor (confirmed empirically: it doesn't reach `CXEval_StrLiteral` for
/// any string in this corpus, so `evaluate_int_eval_result`'s/
/// `evaluate_float_eval_result`'s shared `clang_Cursor_Evaluate` pattern
/// doesn't extend to strings the way it first seemed it would). Falls back
/// to tokenizing the cursor's own extent and reading the first token's
/// spelling instead — reliable because a `StringLiteral` cursor's extent is
/// exactly the quoted literal, never more.
unsafe fn string_literal_text(cursor: clang_sys::CXCursor) -> Option<String> {
    let tu = unsafe { clang_sys::clang_Cursor_getTranslationUnit(cursor) };
    let range = unsafe { clang_sys::clang_getCursorExtent(cursor) };
    let mut tokens: *mut clang_sys::CXToken = std::ptr::null_mut();
    let mut num_tokens: c_uint = 0;
    unsafe { clang_sys::clang_tokenize(tu, range, &mut tokens, &mut num_tokens) };
    if tokens.is_null() || num_tokens == 0 {
        return None;
    }

    let first_token = unsafe { *tokens };
    let spelling = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getTokenSpelling(tu, first_token))
    };
    unsafe {
        clang_sys::clang_disposeTokens(tu, tokens, num_tokens);
    }

    // The token spelling carries the encoding prefix C++ allows before the
    // opening quote (`u8"..."`, `u"..."`, `U"..."`, `L"..."`) — stripping a
    // bare `"` alone only matched an un-prefixed literal; `U"x"` failed at
    // the very first character and this returned `None`, silently
    // discarded by the implicit-conversion wrapper's own fallback (proved
    // by the real Verovio corpus: `Dynam::IsSymbolOnly`'s `return U"x";`).
    // Every encoding narrows to the same Dart `String` a plain literal
    // already does — `std::u32string`/`u16string`/`wstring` already lower
    // to `Type::Str` themselves (`stdlib_template_name` keys on the
    // primary template name, not the character-type argument), so this is
    // completing that existing decision, not making a new one.
    let first_quote = spelling.find('"')?;
    let last_quote = spelling.rfind('"')?;
    if last_quote <= first_quote {
        return None;
    }
    let inner = &spelling[first_quote + 1..last_quote];
    Some(unescape_c_string_literal(inner))
}

/// The handful of escape sequences this corpus's own fixtures actually use
/// (or could plausibly need next) — `\\`, `\"`, `\n`, `\t`, `\r` — not a
/// complete C++ escape-sequence grammar (no `\xNN`/`\uNNNN`/octal). Anything
/// else passes through unescaped rather than being silently dropped, honest
/// about the gap instead of guessing at it.
fn unescape_c_string_literal(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            result.push(ch);
            continue;
        }
        match chars.next() {
            Some('n') => result.push('\n'),
            Some('t') => result.push('\t'),
            Some('r') => result.push('\r'),
            Some('\\') => result.push('\\'),
            Some('"') => result.push('"'),
            Some(other) => {
                result.push('\\');
                result.push(other);
            }
            None => result.push('\\'),
        }
    }
    result
}
