//! Extracts the catalog of named types (structs, classes, unions, enums,
//! typedefs, type aliases) and `#define` macros declared across a project's
//! compilation units, using `libclang`. Macros are further classified into
//! constants, function-like macros, include guards and other annotations
//! (see `TypeDeclarationKind`) since most of them aren't types and some
//! (include guards) aren't meaningful to a user at all.
//!
//! `libclang` is loaded dynamically at runtime (see the `clang-sys`
//! `runtime` feature) and is only ever expected to be found inside the
//! toolchain environment the server actually runs in (the Flatpak sandbox in
//! production, which bundles LLVM through the `llvm21` SDK extension). No
//! path to a specific `libclang` is hardcoded here.

use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::fmt;
use std::os::raw::{c_int, c_uint, c_void};
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::ingest::CompilationUnit;
use crate::progress::{Cancellation, ExtractionProgress};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeDeclarationKind {
    Struct,
    Class,
    Union,
    Enum,
    Typedef,
    TypeAlias,
    /// An object-like macro with a value, e.g. `#define MAX_SIZE 100` — the
    /// closest thing C's preprocessor has to a named constant.
    ConstantMacro,
    /// A function-like macro, e.g. `#define SQUARE(x) ((x) * (x))`.
    FunctionMacro,
    /// The valueless `#define` half of an `#ifndef`/`#define` include guard
    /// (e.g. `#define FOO_H`). Pure build plumbing with nothing to show a
    /// user, so callers are expected to filter this kind out rather than
    /// display it.
    HeaderGuard,
    /// Any other valueless object-like macro (feature flags, export/import
    /// annotations like `#define MYLIB_API`). Not a type either, but kept
    /// distinct from `HeaderGuard` in case it becomes useful later.
    AnnotationMacro,
}

impl TypeDeclarationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Class => "class",
            Self::Union => "union",
            Self::Enum => "enum",
            Self::Typedef => "typedef",
            Self::TypeAlias => "type_alias",
            Self::ConstantMacro => "constant_macro",
            Self::FunctionMacro => "function_macro",
            Self::HeaderGuard => "header_guard",
            Self::AnnotationMacro => "annotation_macro",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "struct" => Some(Self::Struct),
            "class" => Some(Self::Class),
            "union" => Some(Self::Union),
            "enum" => Some(Self::Enum),
            "typedef" => Some(Self::Typedef),
            "type_alias" => Some(Self::TypeAlias),
            "constant_macro" => Some(Self::ConstantMacro),
            "function_macro" => Some(Self::FunctionMacro),
            "header_guard" => Some(Self::HeaderGuard),
            "annotation_macro" => Some(Self::AnnotationMacro),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TypeDeclaration {
    pub name: String,
    pub kind: TypeDeclarationKind,
    /// The chain of enclosing namespaces, innermost last, joined with `::`
    /// (e.g. `"geometry::detail"`), or empty for a type declared at global
    /// scope. Anonymous namespaces are skipped since they contribute no
    /// name to disambiguate with.
    pub namespace: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    /// The line and column of the end of the declaration's extent (e.g. the
    /// closing `}` of a struct/class/union/enum body), used to highlight the
    /// whole declaration rather than just its starting point.
    pub end_line: u32,
    pub end_column: u32,
    /// `libclang`'s Unified Symbol Resolution for this cursor
    /// (`clang_getCursorUSR`) — a semantic identity independent of source
    /// position, unlike `(kind, name, file, line, column)`, which breaks the
    /// instant an unrelated edit shifts line numbers. This is the stable
    /// identity US-4 onward reference types by (see `docs/plans/User
    /// Steps.md`, US-3).
    pub usr: String,
}

/// An edge in the type dependency graph: `caller` references `callee` in its
/// own definition (a struct/class/union field, a base class, or the
/// underlying type of a typedef/type alias).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TypeDependency {
    pub caller: TypeDeclaration,
    pub callee: TypeDeclaration,
}

/// The closed taxonomy of "use" this catalog tracks (US-4). Each kind is a
/// *signature-level* mention of a named type — visible without parsing
/// function bodies, since `extract_type_catalog` parses with
/// `CXTranslationUnit_SkipFunctionBodies` for performance (see the "Escala"
/// note in `docs/plans/User Steps.md`) and reuses that same AST walk rather
/// than reparsing with bodies enabled just for this. Expression-level kinds
/// that only occur inside a body — casts, `sizeof`, `new`, template
/// arguments — are deliberately out of scope for this pass; the doc's open
/// item on completing the taxonomy stays open for those.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeUsageKind {
    /// A file/namespace/static-member-scope variable declaration. Local
    /// variables inside function bodies aren't visible to this pass (see
    /// above) and so aren't counted.
    VariableDeclaration,
    /// A function or method parameter.
    Parameter,
    /// A struct/class/union field.
    Field,
    /// A function or method's return type.
    ReturnType,
    /// A base class in a `class Derived : Base` specifier.
    Inheritance,
    /// The underlying type of a `typedef`/`using` alias.
    TypedefMention,
}

impl TypeUsageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::VariableDeclaration => "variable_declaration",
            Self::Parameter => "parameter",
            Self::Field => "field",
            Self::ReturnType => "return_type",
            Self::Inheritance => "inheritance",
            Self::TypedefMention => "typedef_mention",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "variable_declaration" => Some(Self::VariableDeclaration),
            "parameter" => Some(Self::Parameter),
            "field" => Some(Self::Field),
            "return_type" => Some(Self::ReturnType),
            "inheritance" => Some(Self::Inheritance),
            "typedef_mention" => Some(Self::TypedefMention),
            _ => None,
        }
    }
}

/// One occurrence of a project type being used at a specific source
/// location, keyed by the used type's `usr` (US-3's stable identity) rather
/// than by embedding the whole `TypeDeclaration` — usages are looked up by
/// type identity, and a `TypeDeclaration` copy per occurrence would just be
/// redundant, position-derived data.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TypeUsage {
    pub type_usr: String,
    pub kind: TypeUsageKind,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

/// The catalog of named types declared across a project, the dependency
/// edges between them, and every place a type is used (US-4).
#[derive(Debug, Clone, Default)]
pub struct TypeCatalog {
    pub declarations: Vec<TypeDeclaration>,
    pub dependencies: Vec<TypeDependency>,
    pub usages: Vec<TypeUsage>,
}

#[derive(Debug)]
pub enum TypeCatalogError {
    LibclangUnavailable(String),
    /// Extraction stopped early because `Cancellation::cancel` was called
    /// (US-4 criterion 7) — not a real failure, so `create_project` treats it
    /// distinctly from the other variants (see `ProjectCreationError::
    /// is_cancelled`) rather than persisting a partial catalog.
    Cancelled,
}

impl fmt::Display for TypeCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibclangUnavailable(message) => {
                write!(formatter, "libclang is unavailable: {message}")
            }
            Self::Cancelled => write!(formatter, "type catalog extraction was cancelled"),
        }
    }
}

impl std::error::Error for TypeCatalogError {}

/// Parses every compilation unit with `libclang` and returns the
/// deduplicated catalog of named types declared within `project_root`.
///
/// Compilation units that `libclang` fails to parse are skipped rather than
/// failing the whole extraction, since a single misconfigured translation
/// unit shouldn't prevent cataloging the rest of the project.
///
/// Each `libclang` parse is a cold parse of the whole translation unit (no
/// precompiled headers), which is inherently slow one at a time — on a real
/// project like Verovio (~290 units) that's several minutes spent entirely
/// on one core, indistinguishable from a hang from the outside (see
/// `crates/server/tests/verovio_5_7_0_import_diagnosis.rs`). Units are
/// independent, so they're split across a worker per CPU core, each with its
/// own `CXIndex` — sharing one `CXIndex` across threads isn't safe, but one
/// index per thread is the documented-safe way to parallelize `libclang`.
/// Declarations and dependencies are deduplicated locally per worker, then
/// again across workers when merging, since the same project header is
/// commonly included — and therefore reparsed — by many translation units.
pub fn extract_type_catalog(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
) -> Result<TypeCatalog, TypeCatalogError> {
    extract_type_catalog_cancellable(compilation_units, project_root, progress, None)
}

/// Same as [`extract_type_catalog`], but stops early once `cancellation` is
/// signalled (US-4 criterion 7). Each worker checks it once per compilation
/// unit — best-effort, not preemptive — and a cancelled run reports
/// [`TypeCatalogError::Cancelled`] instead of a partial catalog, so callers
/// never mistake an interrupted run for a complete one.
pub fn extract_type_catalog_cancellable(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> Result<TypeCatalog, TypeCatalogError> {
    ensure_libclang_loaded()?;

    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let total = compilation_units.len();
    if let Some(progress) = progress {
        progress.set_total(total);
    }
    log_type_catalog(format_args!(
        "extract_type_catalog: start, {total} compilation units"
    ));
    let extraction_started = Instant::now();

    let partials = if compilation_units.is_empty() {
        Vec::new()
    } else {
        let worker_count = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
            .min(total);
        let chunk_size = total.div_ceil(worker_count);

        std::thread::scope(|scope| {
            compilation_units
                .chunks(chunk_size)
                .enumerate()
                .map(|(worker_index, chunk)| {
                    let project_root = &project_root;
                    scope.spawn(move || {
                        parse_chunk(worker_index, chunk, project_root, progress, cancellation)
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| handle.join().expect("type catalog worker thread panicked"))
                .collect::<Vec<_>>()
        })
    };

    if cancellation.is_some_and(Cancellation::is_cancelled) {
        log_type_catalog(format_args!(
            "extract_type_catalog: cancelled after {:.2}s",
            extraction_started.elapsed().as_secs_f64()
        ));
        return Err(TypeCatalogError::Cancelled);
    }

    let mut seen = HashSet::new();
    let mut declarations = Vec::new();
    let mut dependency_seen = HashSet::new();
    let mut dependencies = Vec::new();
    let mut usage_seen = HashSet::new();
    let mut usages = Vec::new();

    for (partial_declarations, partial_dependencies, partial_usages) in partials {
        for declaration in partial_declarations {
            if seen.insert(declaration_identity(&declaration)) {
                declarations.push(declaration);
            }
        }

        for dependency in partial_dependencies {
            push_dependency(
                &mut dependencies,
                &mut dependency_seen,
                dependency.caller,
                dependency.callee,
            );
        }

        for usage in partial_usages {
            if usage_seen.insert(usage_identity(&usage)) {
                usages.push(usage);
            }
        }
    }

    log_type_catalog(format_args!(
        "extract_type_catalog: done in {:.2}s, {} declarations, {} dependencies, {} usages",
        extraction_started.elapsed().as_secs_f64(),
        declarations.len(),
        dependencies.len(),
        usages.len()
    ));

    Ok(TypeCatalog {
        declarations,
        dependencies,
        usages,
    })
}

/// Parses `chunk`'s compilation units with a `CXIndex` private to this
/// worker, returning its own local (not yet cross-chunk deduplicated)
/// declarations and dependencies for the caller to merge.
fn parse_chunk(
    worker_index: usize,
    chunk: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> (Vec<TypeDeclaration>, Vec<TypeDependency>, Vec<TypeUsage>) {
    // `clang-sys`'s `runtime` feature loads the shared library into
    // thread-local storage, so the load done by `ensure_libclang_loaded` on
    // the calling thread doesn't cover this worker thread — each one has to
    // load it independently before making any `clang_sys` call.
    load_libclang().expect(
        "libclang already loaded successfully on the calling thread; \
         per-thread load is not expected to fail",
    );

    let mut seen = HashSet::new();
    let mut declarations = Vec::new();
    let mut dependency_seen = HashSet::new();
    let mut dependencies = Vec::new();
    let mut usage_seen = HashSet::new();
    let mut usages = Vec::new();

    unsafe {
        let index = clang_sys::clang_createIndex(0, 0);

        for unit in chunk {
            if cancellation.is_some_and(Cancellation::is_cancelled) {
                log_type_catalog(format_args!(
                    "worker {worker_index}: stopping early, cancellation requested"
                ));
                break;
            }

            let mut visitor_state = VisitorState {
                project_root,
                declarations: &mut declarations,
                seen: &mut seen,
                dependencies: &mut dependencies,
                dependency_seen: &mut dependency_seen,
                usages: &mut usages,
                usage_seen: &mut usage_seen,
            };

            log_type_catalog(format_args!(
                "parsing (worker {worker_index}): {}",
                unit.file
            ));
            let unit_started = Instant::now();

            visit_translation_unit(index, unit, &mut visitor_state);

            if let Some(progress) = progress {
                progress.mark_one_done();
            }

            log_type_catalog(format_args!(
                "parsed in {:.2}s (worker {worker_index}): {}",
                unit_started.elapsed().as_secs_f64(),
                unit.file
            ));
        }

        clang_sys::clang_disposeIndex(index);
    }

    (declarations, dependencies, usages)
}

fn log_type_catalog(args: fmt::Arguments<'_>) {
    eprintln!(
        "[syntax-bridge][type_catalog][{}] {args}",
        timestamp_millis()
    );
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

struct VisitorState<'a> {
    project_root: &'a Path,
    declarations: &'a mut Vec<TypeDeclaration>,
    seen: &'a mut HashSet<String>,
    dependencies: &'a mut Vec<TypeDependency>,
    dependency_seen: &'a mut HashSet<(TypeDeclaration, TypeDeclaration)>,
    usages: &'a mut Vec<TypeUsage>,
    usage_seen: &'a mut HashSet<(String, TypeUsageKind, String, u32, u32)>,
}

/// The key used to deduplicate a `TypeUsage` across translation units (the
/// same project header commonly gets reparsed by every TU that includes it):
/// the used type's `usr` plus the usage's own kind and site, since the same
/// type can legitimately be used more than once at different sites.
fn usage_identity(usage: &TypeUsage) -> (String, TypeUsageKind, String, u32, u32) {
    (
        usage.type_usr.clone(),
        usage.kind,
        usage.file.clone(),
        usage.line,
        usage.column,
    )
}

/// The key used to deduplicate a `TypeDeclaration` across translation units:
/// its `usr` when `libclang` provided one (the stable, position-independent
/// identity), falling back to the old positional key on the rare cursor kind
/// where it doesn't (e.g. some very old `libclang` versions for macros).
fn declaration_identity(declaration: &TypeDeclaration) -> String {
    if !declaration.usr.is_empty() {
        return declaration.usr.clone();
    }

    format!(
        "pos:{:?}:{}:{}:{}:{}",
        declaration.kind, declaration.name, declaration.file, declaration.line, declaration.column
    )
}

unsafe fn visit_translation_unit(
    index: clang_sys::CXIndex,
    unit: &CompilationUnit,
    state: &mut VisitorState<'_>,
) {
    let Ok(file) = CString::new(unit.file.as_str()) else {
        return;
    };

    let args = build_clang_args(unit);
    let arg_cstrings: Vec<CString> = args
        .iter()
        .filter_map(|arg| CString::new(arg.as_str()).ok())
        .collect();
    let arg_ptrs: Vec<*const std::os::raw::c_char> =
        arg_cstrings.iter().map(|arg| arg.as_ptr()).collect();

    let flags = clang_sys::CXTranslationUnit_DetailedPreprocessingRecord
        | clang_sys::CXTranslationUnit_SkipFunctionBodies;

    unsafe {
        let translation_unit = clang_sys::clang_parseTranslationUnit(
            index,
            file.as_ptr(),
            arg_ptrs.as_ptr(),
            arg_ptrs.len() as c_int,
            std::ptr::null_mut(),
            0,
            flags,
        );

        if translation_unit.is_null() {
            return;
        }

        let root_cursor = clang_sys::clang_getTranslationUnitCursor(translation_unit);
        clang_sys::clang_visitChildren(
            root_cursor,
            visit_cursor,
            state as *mut VisitorState<'_> as *mut c_void,
        );

        clang_sys::clang_disposeTranslationUnit(translation_unit);
    }
}

/// Strips the compiler executable, the output flag and the source file
/// itself from a compile command, leaving only the flags `libclang` needs
/// (include paths, defines, the `-std` flag, and so on).
///
/// Exposed to `source_catalog`, which needs the same flags to parse each
/// compilation unit's headers via `clang_getInclusions`.
pub(crate) fn build_clang_args(unit: &CompilationUnit) -> Vec<String> {
    let tokens: Vec<String> = if !unit.arguments.is_empty() {
        unit.arguments.clone()
    } else if let Some(command) = &unit.command {
        shlex::split(command).unwrap_or_default()
    } else {
        Vec::new()
    };

    let mut args = Vec::with_capacity(tokens.len());
    let mut tokens = tokens.into_iter().skip(1).peekable();

    while let Some(token) = tokens.next() {
        if token == "-c" {
            continue;
        }
        if token == "-o" {
            tokens.next();
            continue;
        }
        if token == unit.file {
            continue;
        }

        args.push(token);
    }

    args
}

extern "C" fn visit_cursor(
    cursor: clang_sys::CXCursor,
    parent: clang_sys::CXCursor,
    data: clang_sys::CXClientData,
) -> clang_sys::CXChildVisitResult {
    let state = unsafe { &mut *(data as *mut VisitorState<'_>) };

    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
    let is_relevant = if kind == clang_sys::CXCursor_MacroDefinition {
        unsafe { classify_macro(cursor) }
    } else {
        declaration_kind_for(kind)
            .filter(|_| unsafe { clang_sys::clang_isCursorDefinition(cursor) } != 0)
    };

    if let Some(declaration_kind) = is_relevant
        && let Some(declaration) = describe_cursor(cursor, declaration_kind, state.project_root)
    {
        if state.seen.insert(declaration_identity(&declaration)) {
            state.declarations.push(declaration.clone());
        }

        if matches!(
            declaration_kind,
            TypeDeclarationKind::Typedef | TypeDeclarationKind::TypeAlias
        ) && let underlying = unsafe { clang_sys::clang_getTypedefDeclUnderlyingType(cursor) }
            && let Some(callee) = resolve_named_declaration(underlying, state.project_root)
        {
            push_usage(state, TypeUsageKind::TypedefMention, cursor, &callee);
            push_dependency(
                state.dependencies,
                state.dependency_seen,
                declaration,
                callee,
            );
        }
    }

    if matches!(
        kind,
        clang_sys::CXCursor_FieldDecl | clang_sys::CXCursor_CXXBaseSpecifier
    ) {
        record_member_dependency(cursor, parent, state);
    }

    match kind {
        clang_sys::CXCursor_ParmDecl => {
            let parameter_type = unsafe { clang_sys::clang_getCursorType(cursor) };
            if let Some(target) = resolve_named_declaration(parameter_type, state.project_root) {
                push_usage(state, TypeUsageKind::Parameter, cursor, &target);
            }
        }
        clang_sys::CXCursor_VarDecl => {
            let variable_type = unsafe { clang_sys::clang_getCursorType(cursor) };
            if let Some(target) = resolve_named_declaration(variable_type, state.project_root) {
                push_usage(state, TypeUsageKind::VariableDeclaration, cursor, &target);
            }
        }
        clang_sys::CXCursor_FunctionDecl | clang_sys::CXCursor_CXXMethod => {
            let result_type = unsafe { clang_sys::clang_getCursorResultType(cursor) };
            if let Some(target) = resolve_named_declaration(result_type, state.project_root) {
                push_usage(state, TypeUsageKind::ReturnType, cursor, &target);
            }
        }
        _ => {}
    }

    clang_sys::CXChildVisit_Recurse
}

/// Records a dependency edge from the struct/class/union `parent` cursor to
/// the named type declared by a field or base-class-specifier `cursor`,
/// skipping silently when either side isn't a type this catalog tracks
/// (builtins, or types outside `project_root`). Also records the matching
/// `TypeUsage` (`Field` or `Inheritance`) at `cursor`'s own location, which
/// is the field's or base-specifier's site — not `parent`'s.
fn record_member_dependency(
    cursor: clang_sys::CXCursor,
    parent: clang_sys::CXCursor,
    state: &mut VisitorState<'_>,
) {
    let parent_kind = unsafe { clang_sys::clang_getCursorKind(parent) };
    let Some(parent_declaration_kind) = declaration_kind_for(parent_kind) else {
        return;
    };
    let Some(caller) = describe_cursor(parent, parent_declaration_kind, state.project_root) else {
        return;
    };

    let member_type = unsafe { clang_sys::clang_getCursorType(cursor) };
    let Some(callee) = resolve_named_declaration(member_type, state.project_root) else {
        return;
    };

    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };
    let usage_kind = if kind == clang_sys::CXCursor_CXXBaseSpecifier {
        TypeUsageKind::Inheritance
    } else {
        TypeUsageKind::Field
    };
    push_usage(state, usage_kind, cursor, &callee);

    push_dependency(state.dependencies, state.dependency_seen, caller, callee);
}

/// Records that `target` is used with kind `kind` at `site_cursor`'s own
/// location, deduplicating repeat occurrences (the same project header is
/// commonly reparsed by many translation units). Silently skips sites
/// outside `project_root` or in a system header, the same filter
/// `describe_cursor` applies to declarations.
fn push_usage(
    state: &mut VisitorState<'_>,
    kind: TypeUsageKind,
    site_cursor: clang_sys::CXCursor,
    target: &TypeDeclaration,
) {
    if target.usr.is_empty() {
        return;
    }
    let Some((file, line, column)) = cursor_site(site_cursor, state.project_root) else {
        return;
    };

    let usage = TypeUsage {
        type_usr: target.usr.clone(),
        kind,
        file,
        line,
        column,
    };
    if state.usage_seen.insert(usage_identity(&usage)) {
        state.usages.push(usage);
    }
}

/// Strips pointers, references and array indirections off `cxtype`, then
/// resolves what remains to the `TypeDeclaration` of its defining cursor, if
/// any is known to this catalog.
///
/// `pub(crate)` so `pointer_catalog` can reuse it to resolve a raw pointer's
/// pointee back to this same catalog's `usr` — the two modules disagree on
/// what to do with the indirection itself (this catalog strips it as noise;
/// `pointer_catalog` is the one place indirection *is* the point), but
/// "what named type is ultimately behind this type" is the same question
/// either way.
pub(crate) fn resolve_named_declaration(
    cxtype: clang_sys::CXType,
    project_root: &Path,
) -> Option<TypeDeclaration> {
    let stripped = strip_indirections(cxtype);
    let type_decl_cursor = unsafe { clang_sys::clang_getTypeDeclaration(stripped) };
    if unsafe { clang_sys::clang_Cursor_isNull(type_decl_cursor) } != 0 {
        return None;
    }

    let definition_cursor = unsafe { clang_sys::clang_getCursorDefinition(type_decl_cursor) };
    let target_cursor = if unsafe { clang_sys::clang_Cursor_isNull(definition_cursor) } != 0 {
        type_decl_cursor
    } else {
        definition_cursor
    };

    let target_kind = unsafe { clang_sys::clang_getCursorKind(target_cursor) };
    let declaration_kind = declaration_kind_for(target_kind)?;
    describe_cursor(target_cursor, declaration_kind, project_root)
}

fn strip_indirections(mut cxtype: clang_sys::CXType) -> clang_sys::CXType {
    loop {
        cxtype = match cxtype.kind {
            clang_sys::CXType_Pointer
            | clang_sys::CXType_LValueReference
            | clang_sys::CXType_RValueReference => unsafe {
                clang_sys::clang_getPointeeType(cxtype)
            },
            clang_sys::CXType_ConstantArray
            | clang_sys::CXType_IncompleteArray
            | clang_sys::CXType_VariableArray
            | clang_sys::CXType_DependentSizedArray => unsafe {
                clang_sys::clang_getArrayElementType(cxtype)
            },
            _ => break,
        };
    }
    cxtype
}

/// Inserts a `caller -> callee` edge, deduplicating repeat edges (e.g. two
/// fields of the same type) and dropping self-referential edges (e.g. a
/// linked-list node pointing to itself), which would be noise in a graph
/// meant to drive topological generation order.
fn push_dependency(
    dependencies: &mut Vec<TypeDependency>,
    seen: &mut HashSet<(TypeDeclaration, TypeDeclaration)>,
    caller: TypeDeclaration,
    callee: TypeDeclaration,
) {
    if caller == callee {
        return;
    }

    if seen.insert((caller.clone(), callee.clone())) {
        dependencies.push(TypeDependency { caller, callee });
    }
}

fn declaration_kind_for(kind: clang_sys::CXCursorKind) -> Option<TypeDeclarationKind> {
    match kind {
        clang_sys::CXCursor_StructDecl => Some(TypeDeclarationKind::Struct),
        clang_sys::CXCursor_ClassDecl => Some(TypeDeclarationKind::Class),
        clang_sys::CXCursor_UnionDecl => Some(TypeDeclarationKind::Union),
        clang_sys::CXCursor_EnumDecl => Some(TypeDeclarationKind::Enum),
        clang_sys::CXCursor_TypedefDecl => Some(TypeDeclarationKind::Typedef),
        clang_sys::CXCursor_TypeAliasDecl => Some(TypeDeclarationKind::TypeAlias),
        _ => None,
    }
}

/// Classifies a `CXCursor_MacroDefinition` cursor into the macro kind that
/// best describes it, or `None` for compiler-builtin macros (`__STDC__` and
/// the like), which carry no project information worth cataloging.
///
/// Function-like macros (`#define SQUARE(x) ...`) are unambiguous via
/// `clang_Cursor_isMacroFunctionLike`. Object-like macros are split by
/// whether they expand to anything: a macro with no replacement tokens is
/// either an include guard's `#define` half or some other valueless
/// annotation/flag, distinguished with `looks_like_header_guard`; a macro
/// with replacement tokens is treated as a constant.
pub(crate) unsafe fn classify_macro(cursor: clang_sys::CXCursor) -> Option<TypeDeclarationKind> {
    if unsafe { clang_sys::clang_Cursor_isMacroBuiltin(cursor) } != 0 {
        return None;
    }

    if unsafe { clang_sys::clang_Cursor_isMacroFunctionLike(cursor) } != 0 {
        return Some(TypeDeclarationKind::FunctionMacro);
    }

    if unsafe { macro_has_value_tokens(cursor) } {
        return Some(TypeDeclarationKind::ConstantMacro);
    }

    let name = unsafe { cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    let location = unsafe { clang_sys::clang_getCursorLocation(cursor) };
    let mut line: c_uint = 0;
    unsafe {
        clang_sys::clang_getSpellingLocation(
            location,
            std::ptr::null_mut(),
            &mut line,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
    }

    Some(if looks_like_header_guard(&name, line) {
        TypeDeclarationKind::HeaderGuard
    } else {
        TypeDeclarationKind::AnnotationMacro
    })
}

/// Tokenizes an object-like macro's definition and reports whether it
/// expands to anything beyond its own name (e.g. `MAX_SIZE` in `#define
/// MAX_SIZE 100` has one extra token; a valueless `#define MYLIB_API` has
/// none).
unsafe fn macro_has_value_tokens(cursor: clang_sys::CXCursor) -> bool {
    let translation_unit = unsafe { clang_sys::clang_Cursor_getTranslationUnit(cursor) };
    let extent = unsafe { clang_sys::clang_getCursorExtent(cursor) };

    let mut tokens: *mut clang_sys::CXToken = std::ptr::null_mut();
    let mut token_count: c_uint = 0;
    unsafe {
        clang_sys::clang_tokenize(translation_unit, extent, &mut tokens, &mut token_count);
    }

    let has_value = token_count > 1;

    if !tokens.is_null() {
        unsafe {
            clang_sys::clang_disposeTokens(translation_unit, tokens, token_count);
        }
    }

    has_value
}

/// Heuristic for whether a valueless object-like macro is the `#define`
/// half of an `#ifndef`/`#define` include guard, rather than some other
/// flag/annotation macro (e.g. `#define MYLIB_API`): `libclang` doesn't
/// expose `#ifndef`/`#endif` structure directly, but guards are
/// conventionally the first thing defined in a header and named after the
/// file they protect, so both the position and the name are checked.
fn looks_like_header_guard(name: &str, line: u32) -> bool {
    const NEAR_TOP_OF_FILE: u32 = 5;

    line <= NEAR_TOP_OF_FILE && guard_name_pattern(name)
}

fn guard_name_pattern(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    const SUFFIXES: &[&str] = &["_H", "_H_", "_HPP", "_HPP_", "_HXX", "_HXX_", "_INCLUDED"];

    SUFFIXES.iter().any(|suffix| upper.ends_with(suffix))
}

fn describe_cursor(
    cursor: clang_sys::CXCursor,
    kind: TypeDeclarationKind,
    project_root: &Path,
) -> Option<TypeDeclaration> {
    let name = unsafe { cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    if name.is_empty() {
        return None;
    }

    let (file, line, column) = cursor_site(cursor, project_root)?;

    let namespace = unsafe { namespace_of(cursor) };
    let (end_line, end_column) = unsafe { extent_end(cursor) };
    let usr = unsafe { cxstring_to_string(clang_sys::clang_getCursorUSR(cursor)) };

    Some(TypeDeclaration {
        name,
        kind,
        namespace,
        file,
        line,
        column,
        end_line,
        end_column,
        usr,
    })
}

/// Resolves `cursor`'s own spelling location to `(file, line, column)`,
/// filtering out system headers and anything outside `project_root` — the
/// same rule `describe_cursor` applies to declarations, shared here since
/// `push_usage` needs an identical filter for usage sites.
pub(crate) fn cursor_site(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
) -> Option<(String, u32, u32)> {
    let location = unsafe { clang_sys::clang_getCursorLocation(cursor) };
    if unsafe { clang_sys::clang_Location_isInSystemHeader(location) } != 0 {
        return None;
    }

    let mut file = std::ptr::null_mut();
    let mut line: c_uint = 0;
    let mut column: c_uint = 0;
    unsafe {
        clang_sys::clang_getSpellingLocation(
            location,
            &mut file,
            &mut line,
            &mut column,
            std::ptr::null_mut(),
        );
    }

    if file.is_null() {
        return None;
    }

    let file_name = unsafe { cxstring_to_string(clang_sys::clang_getFileName(file)) };
    if file_name.is_empty() {
        return None;
    }

    let file_path = PathBuf::from(&file_name);
    let canonical_file_path = file_path.canonicalize().unwrap_or(file_path);
    if !canonical_file_path.starts_with(project_root) {
        return None;
    }

    Some((canonical_file_path.display().to_string(), line, column))
}

/// Walks `cursor`'s semantic parents, collecting the spelling of every
/// enclosing `namespace` (innermost last), and joins them with `::`.
///
/// Non-namespace parents (a struct nesting another struct, for instance) are
/// skipped rather than stopping the walk, so a type nested inside a class
/// still picks up that class's enclosing namespace.
pub(crate) unsafe fn namespace_of(cursor: clang_sys::CXCursor) -> String {
    let mut segments = Vec::new();
    let mut parent = unsafe { clang_sys::clang_getCursorSemanticParent(cursor) };

    while unsafe { clang_sys::clang_Cursor_isNull(parent) } == 0 {
        let parent_kind = unsafe { clang_sys::clang_getCursorKind(parent) };
        if parent_kind == clang_sys::CXCursor_TranslationUnit {
            break;
        }

        if parent_kind == clang_sys::CXCursor_Namespace {
            let name = unsafe { cxstring_to_string(clang_sys::clang_getCursorSpelling(parent)) };
            if !name.is_empty() {
                segments.push(name);
            }
        }

        parent = unsafe { clang_sys::clang_getCursorSemanticParent(parent) };
    }

    segments.reverse();
    segments.join("::")
}

/// The line/column of the end of `cursor`'s extent (e.g. the closing `}` of
/// a struct/class/union/enum body), for highlighting the whole declaration.
pub(crate) unsafe fn extent_end(cursor: clang_sys::CXCursor) -> (u32, u32) {
    let extent = unsafe { clang_sys::clang_getCursorExtent(cursor) };
    let end = unsafe { clang_sys::clang_getRangeEnd(extent) };

    let mut line: c_uint = 0;
    let mut column: c_uint = 0;
    unsafe {
        clang_sys::clang_getSpellingLocation(
            end,
            std::ptr::null_mut(),
            &mut line,
            &mut column,
            std::ptr::null_mut(),
        );
    }

    (line, column)
}

pub(crate) unsafe fn cxstring_to_string(string: clang_sys::CXString) -> String {
    unsafe {
        let c_str_ptr = clang_sys::clang_getCString(string);
        let value = if c_str_ptr.is_null() {
            String::new()
        } else {
            CStr::from_ptr(c_str_ptr).to_string_lossy().into_owned()
        };

        clang_sys::clang_disposeString(string);
        value
    }
}

fn ensure_libclang_loaded() -> Result<(), TypeCatalogError> {
    load_libclang().map_err(TypeCatalogError::LibclangUnavailable)
}

/// Loads `libclang` if it isn't already, shared with `source_catalog` so the
/// dynamic-loading step (and its error message) stays consistent across the
/// two `libclang`-backed extractors.
pub(crate) fn load_libclang() -> Result<(), String> {
    if clang_sys::is_loaded() {
        return Ok(());
    }

    clang_sys::load()
}
