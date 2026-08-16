//! Extracts every raw C++ pointer (`T*`) declared across a project's
//! compilation units, using `libclang` — Parte 1 of
//! `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`. This is the fact base
//! the mapping solver's pointer rules (`mapping::pointer_options_for`,
//! `mapping::possible_pointee_types`) are meant to grow into consulting
//! instead of the textual `signature.contains('*')` heuristic
//! (`mapping.rs:905`) and the class-hierarchy-only enumeration they use
//! today.
//!
//! Four kinds of declared pointer are tracked (see [`PointerDeclarationKind`]):
//! parameter, field, local variable, function return type. This is a closed
//! taxonomy, not an oversight — a namespace/file-scope pointer *variable* is
//! deliberately out of scope for this first pass (rare in practice, and not
//! one of the four the driving prompt asked for); `std::unique_ptr`/
//! `std::shared_ptr` are out of scope too, since they aren't `T*` in the
//! AST — smart pointers would need their own pass, the same way
//! `std::vector`/`std::string` are their own library adapters rather than
//! part of the raw-pointer story.
//!
//! Unlike `type_catalog`/`source_catalog`, this pass does **not** parse with
//! `CXTranslationUnit_SkipFunctionBodies`: a local pointer variable only
//! exists inside a function body, so skipping bodies would make
//! [`PointerDeclarationKind::Local`] unreachable. This mirrors
//! `function_catalog`'s own reason for keeping bodies (see that module's
//! docs) and pays the same one-full-parse cost.
//!
//! `libclang` is loaded dynamically at runtime, same as `type_catalog` and
//! `source_catalog` (see `type_catalog`'s module docs for why).

use std::collections::HashSet;
use std::ffi::CString;
use std::os::raw::{c_int, c_void};
use std::path::Path;

use serde::Serialize;

use crate::ingest::CompilationUnit;
use crate::progress::{Cancellation, ExtractionProgress};
use crate::type_catalog;

/// Where a declared pointer sits in the source — a *site*, not a shape (see
/// [`PointerShape`] for that axis).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerDeclarationKind {
    /// A function or method parameter.
    Parameter,
    /// A struct/class/union field.
    Field,
    /// A variable local to a function or method body.
    Local,
    /// A function or method's return type. `name` on the declaration is the
    /// *function's* name in this case — the pointer itself has none.
    ReturnType,
}

impl PointerDeclarationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Parameter => "parameter",
            Self::Field => "field",
            Self::Local => "local",
            Self::ReturnType => "return_type",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "parameter" => Some(Self::Parameter),
            "field" => Some(Self::Field),
            "local" => Some(Self::Local),
            "return_type" => Some(Self::ReturnType),
            _ => None,
        }
    }
}

/// What a pointer points *at*, one level down — the axis the mapping solver
/// cares about (`mapping.rs`'s C01 already treats "aritmética de
/// ponteiros"/opaque pointees as needing a `dart:ffi` bridge unconditionally;
/// `FunctionPointer` and `DoublePointer` are exactly the two shapes that
/// can never be a plain nullable Dart reference, regardless of what the
/// pointee resolves to).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PointerShape {
    /// `T*`, where `T` itself isn't a pointer or a function type.
    Scalar,
    /// `T**` (or deeper) — a pointer to another pointer.
    DoublePointer,
    /// `T (*)(Args...)` — a pointer to a function.
    FunctionPointer,
}

impl PointerShape {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::DoublePointer => "double_pointer",
            Self::FunctionPointer => "function_pointer",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "scalar" => Some(Self::Scalar),
            "double_pointer" => Some(Self::DoublePointer),
            "function_pointer" => Some(Self::FunctionPointer),
            _ => None,
        }
    }
}

/// One pointer declared at a specific source location.
///
/// `pointee_usr` deliberately isn't a full `type_catalog::TypeDeclaration`
/// copy — same reasoning as `type_catalog::TypeUsage::type_usr`: this is
/// looked up by the pointee's identity, and a full copy per pointer would
/// just be redundant, position-derived data. Empty when the pointee isn't a
/// named type this project's own `type_catalog` tracks (`void`, a scalar,
/// or — always, for `PointerShape::FunctionPointer` — a function type,
/// which `type_catalog` has no declaration for at all).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct PointerDeclaration {
    pub kind: PointerDeclarationKind,
    pub shape: PointerShape,
    pub name: String,
    pub pointee_type_name: String,
    pub pointee_usr: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub usr: String,
}

#[derive(Debug)]
pub enum PointerCatalogError {
    LibclangUnavailable(String),
    /// Mirrors `TypeCatalogError::Cancelled`/`SourceCatalogError::Cancelled`:
    /// extraction stopped early because `Cancellation::cancel` was called.
    Cancelled,
}

impl std::fmt::Display for PointerCatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LibclangUnavailable(message) => {
                write!(formatter, "libclang is unavailable: {message}")
            }
            Self::Cancelled => write!(formatter, "pointer catalog extraction was cancelled"),
        }
    }
}

impl std::error::Error for PointerCatalogError {}

/// Parses every compilation unit with `libclang` and returns the
/// deduplicated catalog of raw pointers declared within `project_root`.
///
/// Parallelized across a worker per CPU core, one `CXIndex` per thread, the
/// same reasoning as `type_catalog::extract_type_catalog` (see that
/// function's doc comment).
pub fn extract_pointer_catalog(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
) -> Result<Vec<PointerDeclaration>, PointerCatalogError> {
    extract_pointer_catalog_cancellable(compilation_units, project_root, progress, None)
}

/// Same as [`extract_pointer_catalog`], but stops early once `cancellation`
/// is signalled, mirroring `type_catalog::extract_type_catalog_cancellable`.
pub fn extract_pointer_catalog_cancellable(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> Result<Vec<PointerDeclaration>, PointerCatalogError> {
    type_catalog::load_libclang().map_err(PointerCatalogError::LibclangUnavailable)?;

    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let total = compilation_units.len();
    if let Some(progress) = progress {
        progress.set_total(total);
    }

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
                .map(|chunk| {
                    let project_root = &project_root;
                    scope.spawn(move || parse_chunk(chunk, project_root, progress, cancellation))
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .expect("pointer catalog worker thread panicked")
                })
                .collect::<Vec<_>>()
        })
    };

    if cancellation.is_some_and(Cancellation::is_cancelled) {
        return Err(PointerCatalogError::Cancelled);
    }

    Ok(finish_pointer_catalog(partials))
}

/// Merges every worker's local partials into the final deduplicated
/// pointer catalog — factored out of `extract_pointer_catalog_cancellable`
/// so `extraction::extract_project_catalogs_cancellable` (which drives its
/// own workers directly, sharing a parse with `function_catalog`) can reuse
/// the exact same merge behavior.
pub(crate) fn finish_pointer_catalog(
    partials: Vec<Vec<PointerDeclaration>>,
) -> Vec<PointerDeclaration> {
    let mut seen = HashSet::new();
    let mut declarations = Vec::new();
    for partial in partials {
        for declaration in partial {
            if seen.insert(pointer_identity(&declaration)) {
                declarations.push(declaration);
            }
        }
    }

    declarations
}

fn parse_chunk(
    chunk: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> Vec<PointerDeclaration> {
    type_catalog::load_libclang().expect(
        "libclang already loaded successfully on the calling thread; \
         per-thread load is not expected to fail",
    );

    let mut declarations = Vec::new();

    unsafe {
        let index = clang_sys::clang_createIndex(0, 0);

        for unit in chunk {
            if cancellation.is_some_and(Cancellation::is_cancelled) {
                break;
            }

            visit_translation_unit(index, unit, project_root, &mut declarations);

            if let Some(progress) = progress {
                progress.mark_one_done();
            }
        }

        clang_sys::clang_disposeIndex(index);
    }

    declarations
}

struct VisitorState<'a> {
    project_root: &'a Path,
    declarations: &'a mut Vec<PointerDeclaration>,
}

unsafe fn visit_translation_unit(
    index: clang_sys::CXIndex,
    unit: &CompilationUnit,
    project_root: &Path,
    declarations: &mut Vec<PointerDeclaration>,
) {
    unsafe {
        let Ok(file) = CString::new(unit.file.as_str()) else {
            return;
        };

        let args = type_catalog::build_clang_args(unit);
        let arg_cstrings: Vec<CString> = args
            .iter()
            .filter_map(|arg| CString::new(arg.as_str()).ok())
            .collect();
        let arg_ptrs: Vec<*const std::os::raw::c_char> =
            arg_cstrings.iter().map(|arg| arg.as_ptr()).collect();

        let translation_unit = clang_sys::clang_parseTranslationUnit(
            index,
            file.as_ptr(),
            arg_ptrs.as_ptr(),
            arg_ptrs.len() as c_int,
            std::ptr::null_mut(),
            0,
            clang_sys::CXTranslationUnit_None,
        );

        if translation_unit.is_null() {
            return;
        }

        visit_parsed_translation_unit(translation_unit, project_root, declarations);

        clang_sys::clang_disposeTranslationUnit(translation_unit);
    }
}

/// Walks an already-parsed translation unit (mirrors
/// `type_catalog`/`function_catalog`'s own `visit_parsed_translation_unit`)
/// — lets `extraction::extract_project_catalogs_cancellable` call this
/// against the translation unit it already parsed for `function_catalog`
/// (with `function_catalog::PARSE_FLAGS`) instead of reparsing with this
/// module's own `CXTranslationUnit_None`. Visiting a cursor kind this
/// module's `visit_cursor` doesn't recognize (the handful
/// `DetailedPreprocessingRecord` adds — macro/inclusion-directive cursors,
/// never nested inside a function body) is a no-op for it, same as any
/// other cursor kind it already ignores.
pub(crate) unsafe fn visit_parsed_translation_unit(
    translation_unit: clang_sys::CXTranslationUnit,
    project_root: &Path,
    declarations: &mut Vec<PointerDeclaration>,
) {
    let mut state = VisitorState {
        project_root,
        declarations,
    };
    unsafe {
        let root_cursor = clang_sys::clang_getTranslationUnitCursor(translation_unit);
        clang_sys::clang_visitChildren(
            root_cursor,
            visit_cursor,
            &mut state as *mut VisitorState<'_> as *mut c_void,
        );
    }
}

extern "C" fn visit_cursor(
    cursor: clang_sys::CXCursor,
    _parent: clang_sys::CXCursor,
    data: clang_sys::CXClientData,
) -> clang_sys::CXChildVisitResult {
    let state = unsafe { &mut *(data as *mut VisitorState<'_>) };
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };

    match kind {
        clang_sys::CXCursor_ParmDecl => {
            let cxtype = unsafe { clang_sys::clang_getCursorType(cursor) };
            record_pointer(cursor, cxtype, PointerDeclarationKind::Parameter, state);
        }
        clang_sys::CXCursor_FieldDecl => {
            let cxtype = unsafe { clang_sys::clang_getCursorType(cursor) };
            record_pointer(cursor, cxtype, PointerDeclarationKind::Field, state);
        }
        clang_sys::CXCursor_VarDecl if is_local_to_a_function(cursor) => {
            let cxtype = unsafe { clang_sys::clang_getCursorType(cursor) };
            record_pointer(cursor, cxtype, PointerDeclarationKind::Local, state);
        }
        clang_sys::CXCursor_FunctionDecl | clang_sys::CXCursor_CXXMethod => {
            let cxtype = unsafe { clang_sys::clang_getCursorResultType(cursor) };
            record_pointer(cursor, cxtype, PointerDeclarationKind::ReturnType, state);
        }
        _ => {}
    }

    clang_sys::CXChildVisit_Recurse
}

/// A local variable's semantic parent (unlike its *lexical* parent, which
/// would be the nearest enclosing `{ }` block) is the enclosing function or
/// method itself, regardless of how many nested blocks it's declared inside
/// — exactly the distinction that separates `PointerDeclarationKind::Local`
/// from a namespace/file-scope pointer variable (out of scope, see module
/// docs), whose semantic parent is the translation unit or a namespace.
fn is_local_to_a_function(cursor: clang_sys::CXCursor) -> bool {
    let parent = unsafe { clang_sys::clang_getCursorSemanticParent(cursor) };
    let parent_kind = unsafe { clang_sys::clang_getCursorKind(parent) };

    matches!(
        parent_kind,
        clang_sys::CXCursor_FunctionDecl
            | clang_sys::CXCursor_CXXMethod
            | clang_sys::CXCursor_Constructor
            | clang_sys::CXCursor_Destructor
            | clang_sys::CXCursor_ConversionFunction
    )
}

/// Records `cursor` as a `PointerDeclaration` of kind `kind` if `cxtype` is
/// a raw pointer (`clang_sys::CXType_Pointer`) — references, values and
/// everything else are silently skipped, since only raw pointers are this
/// pass's scope (see module docs).
fn record_pointer(
    cursor: clang_sys::CXCursor,
    cxtype: clang_sys::CXType,
    kind: PointerDeclarationKind,
    state: &mut VisitorState<'_>,
) {
    if cxtype.kind != clang_sys::CXType_Pointer {
        return;
    }

    let Some((file, line, column)) = type_catalog::cursor_site(cursor, state.project_root) else {
        return;
    };

    let pointee = unsafe { clang_sys::clang_getPointeeType(cxtype) };
    let desugared_pointee = unsafe { desugar_typedefs(pointee) };
    // `shape` reads the *desugared* pointee, the same type `pointee_usr`
    // below resolves through. Taking it from the sugared one instead let a
    // typedef that hides a pointer (`typedef Objeto* ObjetoPtr`, then
    // `ObjetoPtr*`) report `Scalar` while `pointee_usr` — which unwinds
    // every indirection via `strip_indirections` — pointed at `Objeto`:
    // together, a description of an `Objeto?` that the declaration
    // (`Objeto**`) never had. `pointee_type_name` deliberately keeps the
    // sugared spelling: it's what the user wrote and what the UI shows.
    let shape = pointer_shape(desugared_pointee);
    let pointee_type_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(pointee)) };
    let pointee_usr =
        type_catalog::resolve_named_declaration(desugared_pointee, state.project_root)
            .map(|declaration| declaration.usr)
            .unwrap_or_default();

    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    let usr = unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(cursor)) };

    state.declarations.push(PointerDeclaration {
        kind,
        shape,
        name,
        pointee_type_name,
        pointee_usr,
        file,
        line,
        column,
        usr,
    });
}

/// Follows a chain of `typedef`s down to the type they ultimately name —
/// the same desugaring `lower::cpp::lower_type`'s `CXType_Typedef` branch
/// already performs for real code generation, applied here to whichever
/// type `resolve_named_declaration` is about to resolve. Without this, a
/// pointer to the classic C idiom `typedef struct { ... } Nome;` (Verovio
/// 6.2.0's embedded miniz vendor: `mz_zip_archive`, `tdefl_compressor`, ...)
/// resolves `pointee_usr` to the typedef's own declaration (kind
/// `Typedef`) instead of the anonymous struct it names — `type_catalog`
/// itself never catalogs a `usr` for that typedef identity as a struct, so
/// `project_service::list_pointers`'s `declarations_by_usr` lookup misses
/// it and the pointer is silently left out of the trivial/nullable
/// narrowing it actually qualifies for
/// (`docs/plans/verovio-6.2-pointer-types.md` caso 2). Bounded at 16 hops:
/// typedef chains can't cycle in valid C++, so this is defensive, not a
/// real limit — the same reasoning `mapping::possible_pointee_types` uses
/// for its own inheritance walk.
unsafe fn desugar_typedefs(mut cxtype: clang_sys::CXType) -> clang_sys::CXType {
    for _ in 0..16 {
        let decl = unsafe { clang_sys::clang_getTypeDeclaration(cxtype) };
        if unsafe { clang_sys::clang_Cursor_isNull(decl) } != 0
            || unsafe { clang_sys::clang_getCursorKind(decl) } != clang_sys::CXCursor_TypedefDecl
        {
            return cxtype;
        }
        cxtype = unsafe { clang_sys::clang_getTypedefDeclUnderlyingType(decl) };
    }
    cxtype
}

fn pointer_shape(pointee: clang_sys::CXType) -> PointerShape {
    match pointee.kind {
        clang_sys::CXType_Pointer => PointerShape::DoublePointer,
        clang_sys::CXType_FunctionProto | clang_sys::CXType_FunctionNoProto => {
            PointerShape::FunctionPointer
        }
        _ => PointerShape::Scalar,
    }
}

/// The key used to deduplicate a `PointerDeclaration` across translation
/// units (the same project header is commonly reparsed by every TU that
/// includes it): the pointer's own `usr` when `libclang` provided one,
/// falling back to its site otherwise — mirrors
/// `type_catalog::declaration_identity`.
fn pointer_identity(declaration: &PointerDeclaration) -> String {
    if !declaration.usr.is_empty() {
        return declaration.usr.clone();
    }

    format!(
        "pos:{:?}:{}:{}:{}:{}",
        declaration.kind, declaration.name, declaration.file, declaration.line, declaration.column
    )
}
