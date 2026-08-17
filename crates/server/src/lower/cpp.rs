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
        ir::Type::List(element) => format!("List{}", overload_type_suffix(element)),
        ir::Type::Set(element) => format!("Set{}", overload_type_suffix(element)),
        ir::Type::Map(key, value) => format!(
            "Map{}{}",
            overload_type_suffix(key),
            overload_type_suffix(value)
        ),
        ir::Type::Record { name, .. } | ir::Type::Enum { name, .. } => name.clone(),
        ir::Type::Tuple(elements) => elements.iter().map(overload_type_suffix).collect(),
        ir::Type::Nullable(inner) => format!("Nullable{}", overload_type_suffix(inner)),
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
/// reference — C++'s "out parameter" idiom (`void f(int &out)`), never
/// generalized past this (a reference to a `Record`/`Str`/`List` here is
/// left alone; no fixture needs it). `cursor` may be a function/method
/// *declaration* (`clang_Cursor_getNumArguments`/`getArgument` work on a
/// declaration exactly like on a call, per `lower_call_arguments`'s own
/// doc comment) — used both by `apply_out_param_bridge`, from the
/// declaration being lowered, and independently by `call_out_param_arg_indices`,
/// from a *call*'s resolved callee — so a call site and its callee can
/// never disagree about which parameters were bridged. Empty (the
/// overwhelmingly common case) for anything else, including a `const`
/// reference (E05's own by-reference `std::string`/`std::vector`
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
            unsafe { is_non_const_scalar_reference(cx_type) }
        })
        .map(|index| index as usize)
        .collect()
}

unsafe fn is_non_const_scalar_reference(cx_type: clang_sys::CXType) -> bool {
    if cx_type.kind != clang_sys::CXType_LValueReference {
        return false;
    }
    let pointee = unsafe { clang_sys::clang_getPointeeType(cx_type) };
    if unsafe { clang_sys::clang_isConstQualifiedType(pointee) } != 0 {
        return false;
    }
    matches!(
        lower_type(pointee),
        ir::Type::Int | ir::Type::Double | ir::Type::Bool
    )
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
    params: &[ir::Param],
    return_type: &mut ir::Type,
    body: &mut Vec<ir::Stmt>,
    origin: &ir::Origin,
) {
    if *return_type != ir::Type::Void {
        return;
    }
    let indices = unsafe { out_param_indices(cursor) };
    if indices.is_empty() {
        return;
    }

    replace_void_returns_with_tuple(body, &indices, params, origin);
    body.push(ir::Stmt::Return {
        value: Some(ir::Expr::Tuple {
            values: out_param_tuple_values(&indices, params, origin),
            origin: origin.clone(),
        }),
        origin: origin.clone(),
    });

    *return_type = ir::Type::Tuple(
        indices
            .iter()
            .map(|&index| params[index].ty.clone())
            .collect(),
    );
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

/// A bare `return;` inside a bridged `void` function/method also needs to
/// return the out-param tuple — a fall-through past the last statement
/// isn't the only way such a function can end. Walks every nested
/// `if`/`while`/`for`/`try` block; every other statement shape is left
/// alone (a `return` with a real C++ value can't appear in a function this
/// module already confirmed returns `void`).
fn replace_void_returns_with_tuple(
    stmts: &mut [ir::Stmt],
    indices: &[usize],
    params: &[ir::Param],
    origin: &ir::Origin,
) {
    for stmt in stmts {
        replace_void_return_with_tuple(stmt, indices, params, origin);
    }
}

fn replace_void_return_with_tuple(
    stmt: &mut ir::Stmt,
    indices: &[usize],
    params: &[ir::Param],
    origin: &ir::Origin,
) {
    match stmt {
        ir::Stmt::Return {
            value: value @ None,
            ..
        } => {
            *value = Some(ir::Expr::Tuple {
                values: out_param_tuple_values(indices, params, origin),
                origin: origin.clone(),
            });
        }
        ir::Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            replace_void_returns_with_tuple(then_branch, indices, params, origin);
            replace_void_returns_with_tuple(else_branch, indices, params, origin);
        }
        ir::Stmt::While { body, .. } => {
            replace_void_returns_with_tuple(body, indices, params, origin)
        }
        ir::Stmt::For {
            init,
            increment,
            body,
            ..
        } => {
            if let Some(init) = init {
                replace_void_return_with_tuple(init, indices, params, origin);
            }
            if let Some(increment) = increment {
                replace_void_return_with_tuple(increment, indices, params, origin);
            }
            replace_void_returns_with_tuple(body, indices, params, origin);
        }
        ir::Stmt::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            replace_void_returns_with_tuple(try_body, indices, params, origin);
            replace_void_returns_with_tuple(catch_body, indices, params, origin);
        }
        ir::Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            replace_void_returns_with_tuple(try_body, indices, params, origin);
            replace_void_returns_with_tuple(finally_body, indices, params, origin);
        }
        ir::Stmt::Return { .. }
        | ir::Stmt::VarDecl { .. }
        | ir::Stmt::Assign { .. }
        | ir::Stmt::FieldAssign { .. }
        | ir::Stmt::ExprStmt { .. }
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
    let (params, clone_prelude) =
        unsafe { collect_params_with_clone_prelude(cursor, &origin, project_root) };
    let body_cursor = unsafe { find_compound_stmt_child(cursor) };
    let mut body = match body_cursor {
        Some(compound) => unsafe { lower_compound_stmt(compound, project_root) },
        None => Vec::new(),
    };
    body.splice(0..0, clone_prelude);
    unsafe { apply_out_param_bridge(cursor, &params, &mut return_type, &mut body, &origin) };

    Some(ir::Function {
        name,
        usr: usr.to_owned(),
        params,
        return_type,
        body,
        origin,
    })
}

/// Lowers a method's *definition* cursor into IR — called from
/// `function_catalog::visit_cursor` for a `CXXMethodDecl` with a body
/// (inline or out-of-line), the same way `lower_function` handles a free
/// function. `is_static` is read straight off the cursor rather than passed
/// in — the caller already needed it to route here in the first place, but
/// re-reading is one cheap `libclang` call, not worth threading as a
/// parameter.
pub fn lower_method(
    cursor: clang_sys::CXCursor,
    usr: &str,
    project_root: &Path,
) -> Option<ir::Method> {
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    if name.is_empty() {
        return None;
    }

    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;
    let origin = ir::Origin { file, line, column };

    let mut return_type = lower_type(unsafe { clang_sys::clang_getCursorResultType(cursor) });
    let (params, clone_prelude) =
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
        let mut body = match body_cursor {
            Some(compound) => unsafe { lower_compound_stmt(compound, project_root) },
            None => Vec::new(),
        };
        body.splice(0..0, clone_prelude);
        unsafe { apply_out_param_bridge(cursor, &params, &mut return_type, &mut body, &origin) };
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
    Some(unsafe { lower_compound_stmt(body_cursor, project_root) })
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
    let mut body = match body_cursor {
        Some(compound) => unsafe { lower_compound_stmt(compound, project_root) },
        None => Vec::new(),
    };
    body.splice(0..0, clone_prelude);

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

    let variants = unsafe { enum_variants(cursor) };

    Some(ir::Enum {
        name,
        usr,
        variants,
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
    if unsafe { enum_variants(decl) }.is_empty() {
        return None;
    }

    Some((usr, name))
}

/// Every enumerator of `decl`, in source order, already under the Dart
/// name each one is referenced by (`dart_enum_constant_name`).
unsafe fn enum_variants(decl: clang_sys::CXCursor) -> Vec<String> {
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
            dart_enum_constant_name(&cpp_name)
        })
        .collect()
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
/// renaming them here would churn names for nothing. None of these is a
/// C++ keyword either, so an ordinary C++ identifier can land on any of
/// them (Verovio 6.2.0 diagnosis, item 9: `bool is()`, `void f(int in)`,
/// `int is = 1;`, `void finally()` all appear in the real corpus).
const RESERVED_WORDS: &[&str] = &[
    "assert", "break", "case", "catch", "class", "const", "continue", "default", "do", "else",
    "enum", "extends", "false", "final", "finally", "for", "if", "in", "is", "new", "null",
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
                None
            } else {
                Some(ir::BaseClass { usr, name })
            }
        })
        .collect()
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
/// route through, so they can never disagree): a private/protected C++
/// member (E04's visibility requirement — Dart only distinguishes
/// library-private, so `protected` collapses into the same leading-`_`
/// treatment as `private`) gets a leading `_`, trimming one trailing `_` off
/// the C++ name first so a conventionally-named `saldo_` becomes `_saldo`,
/// not `_saldo_`. A `public` (or unspecified-in-a-`struct`, which defaults
/// to public) member is untouched.
unsafe fn dart_member_name(cursor: clang_sys::CXCursor) -> String {
    let cpp_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    let access = unsafe { clang_sys::clang_getCXXAccessSpecifier(cursor) };
    let is_private = matches!(
        access,
        clang_sys::CX_CXXPrivate | clang_sys::CX_CXXProtected
    );
    if is_private {
        format!("_{}", cpp_name.trim_end_matches('_'))
    } else {
        dart_safe_identifier(&cpp_name)
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
    let children = unsafe { collect_children(member_ref_cursor) };
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
        let mut pointee_ty = lower_type(unsafe { clang_sys::clang_getPointeeType(cx_type) });
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
        if let ir::Type::Unsupported(spelling) = &pointee_ty
            && mapping::scalar_pointee_dart_type(spelling).is_some()
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
            _ => mapping::PointeeShape::Opaque,
        };
        let options = mapping::pointer_options_for(shape, None, None);
        return if options[0].id == "referencia-anulavel" {
            ir::Type::Nullable(Box::new(pointee_ty))
        } else {
            let spelling = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(cx_type))
            };
            ir::Type::Unsupported(spelling)
        };
    }

    match cx_type.kind {
        clang_sys::CXType_Int => ir::Type::Int,
        // `std::string::size()`/`std::vector::size()` both return
        // `size_type` — `size_t`, `unsigned long` on this project's
        // toolchain (confirmed empirically, not assumed: this is exactly
        // the type E05's `mensagem.size()` return-value conversion hit).
        // Mapped to the same `Type::Int` every other integer width in this
        // corpus uses — Dart's `int` is 64-bit already, and cross-language
        // integer-width divergence is an accepted, already-precedented gap
        // here (E01's `int` overflow, `examples/E01-funcao-aritmetica/NOTES.md`),
        // not a new one.
        clang_sys::CXType_ULong => ir::Type::Int,
        clang_sys::CXType_Bool => ir::Type::Bool,
        clang_sys::CXType_Double => ir::Type::Double,
        clang_sys::CXType_Void => ir::Type::Void,
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
            let stdlib_name = unsafe { stdlib_template_name(decl) };
            match stdlib_name.as_deref() {
                Some("basic_string") => return ir::Type::Str,
                // `std::list<T>` shares `Type::List`'s shape with
                // `std::vector<T>` — see that variant's doc comment for why
                // this is a deliberate reuse (caso 5,
                // `docs/plans/verovio-6.2-pointer-types.md`), not an
                // oversight.
                Some("vector") | Some("list") => {
                    let element =
                        if unsafe { clang_sys::clang_Type_getNumTemplateArguments(cx_type) } >= 1 {
                            lower_type(unsafe {
                                clang_sys::clang_Type_getTemplateArgumentAsType(cx_type, 0)
                            })
                        } else {
                            ir::Type::Unsupported(
                                "std::vector/list with no element type argument".to_owned(),
                            )
                        };
                    return ir::Type::List(Box::new(element));
                }
                Some("set") => {
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
                Some("map") => {
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
    project_root: &Path,
) -> (Vec<ir::Param>, Vec<ir::Stmt>) {
    let mut params = Vec::new();
    let mut prelude = Vec::new();

    for param_cursor in unsafe { collect_children(cursor) } {
        if unsafe { clang_sys::clang_getCursorKind(param_cursor) } != clang_sys::CXCursor_ParmDecl {
            continue;
        }

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
        let ty = lower_type(cx_type);

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
        // initializer. Only looked up for a scalar/`Str` parameter: a
        // `Record`-typed default would interact with the by-value clone
        // prelude above in a way no fixture forces yet, so it stays
        // unimplemented rather than guessed at.
        let default_value = if matches!(ty, ir::Type::Record { .. }) {
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
                .map(|default_cursor| unsafe { lower_expr(default_cursor, project_root) })
        };

        params.push(ir::Param {
            name: param_name,
            ty,
            default_value,
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

    // `vrv::Fraction::Reduce(numerador, denominador);` (E13) — a bare call
    // to a function/method `apply_out_param_bridge` rewrote to return a
    // Dart record instead of `void`: the statement itself needs to become
    // a destructuring assignment (`(numerador, denominador) = ...;`), not a
    // plain `ExprStmt` that discards the record. Checked before the
    // `is_known_expression_kind` fallback below, which would otherwise
    // treat this exactly like any other bare call.
    if kind == clang_sys::CXCursor_CallExpr {
        let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
        if unsafe { clang_sys::clang_Cursor_isNull(referenced) } == 0 {
            let out_indices = unsafe { call_out_param_arg_indices(referenced) };
            if !out_indices.is_empty() {
                let targets: Vec<ir::Expr> = out_indices
                    .iter()
                    .map(|&index| {
                        let arg_cursor =
                            unsafe { clang_sys::clang_Cursor_getArgument(cursor, index as c_uint) };
                        unsafe { lower_expr(arg_cursor, project_root) }
                    })
                    .collect();
                let value = unsafe { lower_expr(cursor, project_root) };
                return ir::Stmt::TupleAssign {
                    targets,
                    value,
                    origin,
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

/// The 0-based positions, among a `CallExpr`'s own raw arguments, of every
/// argument passed to a bridged out-param (see `apply_out_param_bridge`) —
/// recomputed independently from `referenced` (the call's resolved
/// callee), the same "never disagree" discipline `out_param_indices`'s own
/// doc comment describes. Only recognizes a call to a free function or a
/// `static` method: `lower_call_arguments`'s own doc comment already
/// establishes that shape's raw argument index lines up 1:1 with the
/// callee's declared parameter index (no receiver consuming argument 0 the
/// way an instance-method or operator call's does) — an instance-method
/// out-param isn't supported here, since no fixture yet needs one.
unsafe fn call_out_param_arg_indices(referenced: clang_sys::CXCursor) -> Vec<usize> {
    let referenced_kind = unsafe { clang_sys::clang_getCursorKind(referenced) };
    let is_free_or_static = referenced_kind == clang_sys::CXCursor_FunctionDecl
        || (referenced_kind == clang_sys::CXCursor_CXXMethod
            && unsafe { clang_sys::clang_CXXMethod_isStatic(referenced) } != 0);
    if !is_free_or_static {
        return Vec::new();
    }
    unsafe { out_param_indices(referenced) }
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

    // Item 9 of `docs/plans/diagnostico-verovio-6.2.0.md`
    // (`verovio_6_2_0_transpile_diagnosis`): a local variable legally named
    // after a Dart reserved word (`jsonxx.dart`'s own
    // `basic_istringstream is = ...;`) hits the same parse error a
    // reserved-word parameter/method name does — `dart_safe_identifier`
    // covers it the same way. Every reference to this local inside the
    // body resolves through the same function (`qualified_static_member_name`
    // → `dart_member_name`'s public branch), so the two can never disagree.
    let name = dart_safe_identifier(&unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(*var_decl_cursor))
    });
    let cx_type = unsafe { clang_sys::clang_getCursorType(*var_decl_cursor) };
    let ty = lower_type(cx_type);

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
    // this, not guessed. All three need filtering out before "first
    // remaining child, if any" is the real initializer.
    let init_candidates: Vec<clang_sys::CXCursor> = unsafe { collect_children(*var_decl_cursor) }
        .into_iter()
        .filter(|child| {
            !matches!(
                unsafe { clang_sys::clang_getCursorKind(*child) },
                clang_sys::CXCursor_TypeRef | clang_sys::CXCursor_NamespaceRef
            )
        })
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
        ir::Type::Record { .. }
        | ir::Type::Enum { .. }
        | ir::Type::Str
        | ir::Type::List(_)
        | ir::Type::Set(_)
        | ir::Type::Map(_, _)
        | ir::Type::Tuple(_)
        | ir::Type::Nullable(_)
        | ir::Type::Void
        | ir::Type::Unsupported(_) => ir::Expr::Unsupported {
            reason: "no default value available for this field's type yet".to_owned(),
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
        rhs: Box::new(unsafe { lower_expr(*rhs_cursor, project_root) }),
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

    ir::Stmt::Unsupported {
        reason: "compound assignment target is not a simple local variable or a field".to_owned(),
        origin,
    }
}

fn compound_assign_op(kind: clang_sys::CXBinaryOperatorKind) -> Option<ir::BinaryOp> {
    match kind {
        clang_sys::CXBinaryOperator_AddAssign => Some(ir::BinaryOp::Add),
        clang_sys::CXBinaryOperator_SubAssign => Some(ir::BinaryOp::Sub),
        clang_sys::CXBinaryOperator_MulAssign => Some(ir::BinaryOp::Mul),
        clang_sys::CXBinaryOperator_DivAssign => Some(ir::BinaryOp::Div),
        clang_sys::CXBinaryOperator_RemAssign => Some(ir::BinaryOp::Mod),
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
            reason: format!(
                "wrapper cursor kind {kind} had {} children after filtering type \
                 references, expected exactly one",
                children.len()
            ),
            origin,
        };
    }

    if kind == clang_sys::CXCursor_DeclRefExpr {
        let name =
            unsafe { qualified_static_member_name(clang_sys::clang_getCursorReferenced(cursor)) };
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
        return ir::Expr::This {
            ty: ir::Type::Void,
            origin,
        };
    }

    if kind == clang_sys::CXCursor_StringLiteral {
        return match unsafe { string_literal_text(cursor) } {
            Some(value) => ir::Expr::StringLiteral { value, origin },
            None => ir::Expr::Unsupported {
                reason: "could not evaluate string literal".to_owned(),
                origin,
            },
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

    if kind == clang_sys::CXCursor_FloatingLiteral {
        return match unsafe { evaluate_float_eval_result(cursor) } {
            Some(value) => ir::Expr::DoubleLiteral { value, origin },
            None => ir::Expr::Unsupported {
                reason: "could not evaluate floating-point literal".to_owned(),
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
        let owner_template_name = unsafe { stdlib_template_name(owner) };
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

    if referenced_kind != clang_sys::CXCursor_FunctionDecl {
        return ir::Expr::Unsupported {
            reason: format!(
                "unsupported call target cursor kind {referenced_kind} \
                 (only free functions, methods and constructors are lowered as calls so far)"
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
        return ir::Expr::Unsupported {
            reason: format!("unsupported free operator overload: {callee_name}"),
            origin,
        };
    }

    let args = match unsafe { lower_call_arguments(cursor, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::Unsupported {
                reason: "could not enumerate call arguments".to_owned(),
                origin,
            };
        }
    };

    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(cursor) });
    ir::Expr::Call {
        target: None,
        callee_usr,
        callee_name,
        args,
        ty,
        origin,
    }
}

/// Whether `name` could ever be printed as a bare Dart call target
/// (`{name}(args)`) — a real identifier, not an operator token like
/// `operator<<` or `operator<=>`. Used as the last-resort guard on every
/// generic `Call`-construction fallback in this module: a call target this
/// rejects has no valid literal spelling in Dart, so it must become
/// `Expr::Unsupported` instead of a `Call` `emit::dart` would print verbatim.
fn is_plain_dart_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
unsafe fn lower_stdlib_method_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let template_name = unsafe { stdlib_template_name(owner) }?;
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };

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
        if template_name != "vector" {
            return Some(ir::Expr::Unsupported {
                reason: format!("unsupported std::{template_name}::operator[] call"),
                origin: origin.clone(),
            });
        }
        let children = unsafe { collect_children(call_cursor) };
        let [receiver_cursor, _operator_ref_cursor, index_cursor] = children.as_slice() else {
            return Some(ir::Expr::Unsupported {
                reason: format!(
                    "std::vector::operator[] call had {} children, expected 3",
                    children.len()
                ),
                origin: origin.clone(),
            });
        };
        let target = unsafe { lower_expr(*receiver_cursor, project_root) };
        let index = unsafe { lower_expr(*index_cursor, project_root) };
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
        let owner_type = unsafe { clang_sys::clang_getCursorType(owner) };
        let ty = if unsafe { clang_sys::clang_Type_getNumTemplateArguments(owner_type) } >= 1 {
            lower_type(unsafe { clang_sys::clang_Type_getTemplateArgumentAsType(owner_type, 0) })
        } else {
            ir::Type::Unsupported("std::vector with no element type argument".to_owned())
        };
        return Some(ir::Expr::Index {
            target: Box::new(target),
            index: Box::new(index),
            ty,
            origin: origin.clone(),
        });
    }

    let receiver_children = unsafe { collect_children(call_cursor) };
    let member_ref_cursor = *receiver_children.first()?;
    if unsafe { clang_sys::clang_getCursorKind(member_ref_cursor) }
        != clang_sys::CXCursor_MemberRefExpr
    {
        return Some(ir::Expr::Unsupported {
            reason: "standard-library method call's first child was not the expected \
                      member-reference cursor"
                .to_owned(),
            origin: origin.clone(),
        });
    }
    let target = unsafe { member_ref_receiver(member_ref_cursor, project_root, origin) };

    match (template_name.as_str(), callee_name.as_str()) {
        ("basic_string", "size") | ("basic_string", "length") => Some(ir::Expr::StringByteLength {
            target: Box::new(target),
            origin: origin.clone(),
        }),
        ("vector", "size") => Some(ir::Expr::FieldAccess {
            target: Box::new(target),
            field: "length".to_owned(),
            ty: ir::Type::Int,
            origin: origin.clone(),
        }),
        _ => Some(ir::Expr::Unsupported {
            reason: format!("unsupported std::{template_name}::{callee_name} call"),
            origin: origin.clone(),
        }),
    }
}

/// `std::gcd(a, b)` (`<numeric>`, E13's `Fraction::Reduce`) — the one
/// non-operator, non-method `std` free function this corpus needs a bridge
/// for. Dart's `int` already has this exact method natively
/// (`5.gcd(6) == 1`, confirmed with real `dart analyze`/`dart run` — no
/// helper function needed at all, unlike `Expr::StringByteLength`'s UTF-8
/// bridge), so this maps the two-argument free call directly onto a method
/// call on the first argument (`a.gcd(b)`) rather than emitting a
/// `Call` to a top-level `gcd` that doesn't exist in Dart. Gated on
/// `clang_Location_isInSystemHeader` and the owning namespace being `std`,
/// the same two-part guard `lower_stdlib_operator_call` uses, so a
/// project's own free function named `gcd` is never mistaken for this one.
unsafe fn lower_stdlib_free_function_call(
    call_cursor: clang_sys::CXCursor,
    referenced: clang_sys::CXCursor,
    callee_name: &str,
    project_root: &Path,
    origin: &ir::Origin,
) -> Option<ir::Expr> {
    if callee_name != "gcd" {
        return None;
    }
    if unsafe {
        clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(referenced))
    } == 0
    {
        return None;
    }
    let owner = unsafe { clang_sys::clang_getCursorSemanticParent(referenced) };
    let owner_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(owner)) };
    if owner_name != "std" {
        return None;
    }

    let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
    if arg_count != 2 {
        return None;
    }
    let lhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
    let rhs_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 1) };
    let target = unsafe { lower_expr(lhs_cursor, project_root) };
    let arg = unsafe { lower_expr(rhs_cursor, project_root) };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });
    Some(ir::Expr::Call {
        target: Some(Box::new(target)),
        callee_usr: unsafe {
            type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced))
        },
        callee_name: "gcd".to_owned(),
        args: vec![arg],
        ty,
        origin: origin.clone(),
    })
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
    let receiver_children = unsafe { collect_children(call_cursor) };
    let Some(first_child) = receiver_children.first() else {
        return ir::Expr::Unsupported {
            reason: "method call had no receiver expression".to_owned(),
            origin,
        };
    };

    let (target, arg_skip) = if unsafe { clang_sys::clang_getCursorKind(*first_child) }
        == clang_sys::CXCursor_MemberRefExpr
    {
        (
            unsafe { member_ref_receiver(*first_child, project_root, &origin) },
            0,
        )
    } else {
        let arg_count = unsafe { clang_sys::clang_Cursor_getNumArguments(call_cursor) };
        if arg_count < 1 {
            return ir::Expr::Unsupported {
                reason: "operator call had no receiver argument".to_owned(),
                origin,
            };
        }
        let receiver_cursor = unsafe { clang_sys::clang_Cursor_getArgument(call_cursor, 0) };
        (unsafe { lower_expr(receiver_cursor, project_root) }, 1)
    };

    let callee_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced)) };
    if callee_usr.is_empty() {
        return ir::Expr::Unsupported {
            reason: "resolved method call target has no stable identity".to_owned(),
            origin,
        };
    }
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    // A functor call (`pred(a, b)`) reaches this same operator-syntax
    // branch as `a == b` (E13) does — `emit::dart`'s own bridge for the
    // *declaration* of `operator()` renames it to Dart's `call` method
    // (`emit::dart::emit_method`), so the call site has to agree, or the
    // two would name different methods.
    let callee_name = if callee_name == "operator()" {
        "call".to_owned()
    } else {
        callee_name
    };
    // Any other operator-syntax call this module doesn't specifically
    // recognize (`lower_record_operator_call` already intercepted the ones
    // Dart maps directly) has no bare-identifier spelling `emit::dart` could
    // print as a call target — same guard as the free-function fallback
    // above, and for the same reason.
    if !is_plain_dart_identifier(&callee_name) {
        return ir::Expr::Unsupported {
            reason: format!("unsupported operator method call: {callee_name}"),
            origin,
        };
    }
    let args = match unsafe { lower_call_arguments_skipping(call_cursor, arg_skip, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::Unsupported {
                reason: "could not enumerate method call arguments".to_owned(),
                origin,
            };
        }
    };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });

    ir::Expr::Call {
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
        return ir::Expr::Unsupported {
            reason: "static method's owning class has no stable identity".to_owned(),
            origin,
        };
    }

    let callee_usr =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced)) };
    if callee_usr.is_empty() {
        return ir::Expr::Unsupported {
            reason: "resolved static method call target has no stable identity".to_owned(),
            origin,
        };
    }
    let callee_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(referenced)) };
    let args = match unsafe { lower_call_arguments(call_cursor, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::Unsupported {
                reason: "could not enumerate static method call arguments".to_owned(),
                origin,
            };
        }
    };
    let ty = lower_type(unsafe { clang_sys::clang_getCursorType(call_cursor) });

    ir::Expr::Call {
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
        return ir::Expr::Unsupported {
            reason: "constructor's owning class has no stable identity".to_owned(),
            origin,
        };
    }

    let constructor_index = unsafe { constructor_ordinal(owner, referenced) };
    let args = match unsafe { lower_call_arguments(call_cursor, project_root) } {
        Some(args) => args,
        None => {
            return ir::Expr::Unsupported {
                reason: "could not enumerate constructor call arguments".to_owned(),
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

    let inner = spelling.strip_prefix('"')?.strip_suffix('"')?;
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
