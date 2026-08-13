//! Extracts the catalog of callables — free functions, methods,
//! constructors, destructors and function-like macros — declared across a
//! project's compilation units, and the static call graph between them
//! (US-5).
//!
//! Unlike `type_catalog`, this parses *with* function bodies: the call graph
//! only exists inside them, so `CXTranslationUnit_SkipFunctionBodies` (the
//! flag `type_catalog` and `source_catalog` both rely on for speed, see the
//! "Escala" note in `docs/plans/User Steps.md`) cannot be used here. This is
//! a known, deliberate trade-off — this pass re-pays the full parsing cost
//! those two sidestep — not an oversight; extracting a call graph without
//! parsing bodies isn't possible with `libclang`.

use std::collections::HashSet;
use std::ffi::CString;
use std::fmt;
use std::os::raw::{c_int, c_uint, c_void};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::ingest::CompilationUnit;
use crate::ir;
use crate::lower;
use crate::progress::{Cancellation, ExtractionProgress};
use crate::type_catalog::{self, TypeDeclarationKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionDeclarationKind {
    FreeFunction,
    Method,
    Constructor,
    Destructor,
    /// A function-like macro (`#define SQUARE(x) ...`) — the only macro
    /// shape that behaves like a callable. The other macro kinds
    /// `type_catalog::TypeDeclarationKind` distinguishes (`ConstantMacro`,
    /// `HeaderGuard`, `AnnotationMacro`) already have a home in US-3's
    /// catalog and aren't duplicated here.
    FunctionMacro,
    /// A function or method template's *primary* declaration (`template
    /// <typename T> T f(T a)`) — `CXCursor_FunctionTemplate`, a cursor kind
    /// `FreeFunction`/`Method` don't match, so templates were invisible to
    /// the catalog until this variant was added. Each instantiation is a
    /// separate question (monomorphization is an open C++→Dart mapping
    /// decision, see US-7) and isn't cataloged separately here — only the
    /// one generic declaration is.
    FunctionTemplate,
}

impl FunctionDeclarationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FreeFunction => "free_function",
            Self::Method => "method",
            Self::Constructor => "constructor",
            Self::Destructor => "destructor",
            Self::FunctionMacro => "function_macro",
            Self::FunctionTemplate => "function_template",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "free_function" => Some(Self::FreeFunction),
            "method" => Some(Self::Method),
            "constructor" => Some(Self::Constructor),
            "destructor" => Some(Self::Destructor),
            "function_macro" => Some(Self::FunctionMacro),
            "function_template" => Some(Self::FunctionTemplate),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct FunctionDeclaration {
    pub name: String,
    pub kind: FunctionDeclarationKind,
    /// The chain of enclosing namespaces, mirroring
    /// `type_catalog::TypeDeclaration::namespace` — empty for a
    /// `FunctionMacro`, which has no namespace.
    pub namespace: String,
    /// `usr` of the owning struct/class/union, for `Method`/`Constructor`/
    /// `Destructor` — `None` for `FreeFunction`/`FunctionMacro`.
    pub owning_class_usr: Option<String>,
    /// Full signature text (return type, qualified name, parameter types and
    /// names, `const` qualifier) — what tells two overloads apart (US-5
    /// criterion 2) beyond their `usr`.
    pub signature: String,
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub usr: String,
    pub is_virtual: bool,
    /// Whether this is a pure virtual method (`= 0`), i.e. carries no body of
    /// its own — the fact `mapping::options_for` needs to tell a genuine
    /// interface (every virtual member pure) from a class that only *looks*
    /// like one until a member with a real default body shows up (US-7,
    /// `docs/mapping-solver-cases.md` case B03). Always `false` for a
    /// non-virtual member.
    pub is_pure_virtual: bool,
    /// Whether this was declared `= default` (most commonly `virtual
    /// ~X() = default;`, written only to keep a base class safely
    /// polymorphic) — `mapping::options_for` treats a *defaulted* destructor
    /// as carrying no real teardown logic, unlike a destructor with a body,
    /// which is the actual RAII signal (US-7, `docs/mapping-solver-cases.md`
    /// cases C05/C06). Always `false` for a non-defaulted member.
    pub is_defaulted: bool,
    /// `usr` of every virtual method this one overrides — more than one
    /// under multiple inheritance, when a derived class overrides methods
    /// from more than one base with the same signature. Empty when the
    /// method overrides nothing (including for non-methods).
    pub overridden_usrs: Vec<String>,
}

/// Whether a call site's target could be determined statically, and if so,
/// whether that determination is itself only the *statically* known target
/// because the call actually goes through virtual dispatch (US-5 criterion
/// 3). `libclang` resolves a virtual call to the declaration found by name
/// lookup on the caller's static type — not the dynamically-dispatched
/// override that runs — so `callee_usr` here is that static target, flagged
/// rather than presented as the definitive callee.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum CallResolution {
    Resolved {
        callee_usr: String,
        is_dynamic_dispatch: bool,
    },
    /// The call's target isn't statically known at all (US-5 criterion 6) —
    /// e.g. a call through a function pointer.
    Unresolved { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct CallEdge {
    pub caller_usr: String,
    pub resolution: CallResolution,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Default)]
pub struct FunctionCatalog {
    pub declarations: Vec<FunctionDeclaration>,
    pub calls: Vec<CallEdge>,
    /// IR of every free function's body, lowered by `lower::cpp` during the
    /// same traversal that builds `declarations`/`calls` — see that module's
    /// docs for why this isn't a fourth `libclang` pass. Only free functions
    /// are lowered so far (E01–E03 scope, `docs/plans/primeiro-corte-e01-e03.md`);
    /// methods/constructors/destructors are not yet represented in IR.
    pub ir_functions: Vec<ir::Function>,
    /// IR of every `struct`/`class` *definition*, lowered the same way —
    /// E03 scope (`docs/plans/primeiro-corte-e01-e03.md` §7 PR5).
    pub ir_records: Vec<ir::Record>,
}

#[derive(Debug)]
pub enum FunctionCatalogError {
    LibclangUnavailable(String),
    /// Mirrors `TypeCatalogError::Cancelled` (US-4 criterion 7).
    Cancelled,
}

impl fmt::Display for FunctionCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibclangUnavailable(message) => {
                write!(formatter, "libclang is unavailable: {message}")
            }
            Self::Cancelled => write!(formatter, "function catalog extraction was cancelled"),
        }
    }
}

impl std::error::Error for FunctionCatalogError {}

/// Parses every compilation unit with `libclang` and returns the
/// deduplicated catalog of callables and call edges. Mirrors
/// `type_catalog::extract_type_catalog`'s structure (parallel workers, one
/// `CXIndex` each, local dedup then cross-worker dedup) — see that
/// function's doc comment for why.
pub fn extract_function_catalog(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
) -> Result<FunctionCatalog, FunctionCatalogError> {
    extract_function_catalog_cancellable(compilation_units, project_root, progress, None)
}

/// Same as [`extract_function_catalog`], but stops early once `cancellation`
/// is signalled (US-4 criterion 7, reused here — see US-5's "compartilha com
/// US-4 a mesma infraestrutura de índice").
pub fn extract_function_catalog_cancellable(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> Result<FunctionCatalog, FunctionCatalogError> {
    type_catalog::load_libclang().map_err(FunctionCatalogError::LibclangUnavailable)?;

    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let total = compilation_units.len();
    if let Some(progress) = progress {
        progress.set_total(total);
    }
    log_function_catalog(format_args!(
        "extract_function_catalog: start, {total} compilation units"
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
                .map(|handle| {
                    handle
                        .join()
                        .expect("function catalog worker thread panicked")
                })
                .collect::<Vec<_>>()
        })
    };

    if cancellation.is_some_and(Cancellation::is_cancelled) {
        log_function_catalog(format_args!(
            "extract_function_catalog: cancelled after {:.2}s",
            extraction_started.elapsed().as_secs_f64()
        ));
        return Err(FunctionCatalogError::Cancelled);
    }

    let mut seen = HashSet::new();
    let mut declarations = Vec::new();
    let mut call_seen = HashSet::new();
    let mut calls = Vec::new();
    let mut ir_seen = HashSet::new();
    let mut ir_functions = Vec::new();
    let mut ir_record_seen = HashSet::new();
    let mut ir_records = Vec::new();

    for (partial_declarations, partial_calls, partial_ir_functions, partial_ir_records) in partials
    {
        for declaration in partial_declarations {
            if seen.insert(declaration_identity(&declaration)) {
                declarations.push(declaration);
            }
        }

        for call in partial_calls {
            if call_seen.insert(call_identity(&call)) {
                calls.push(call);
            }
        }

        for function in partial_ir_functions {
            if ir_seen.insert(function.usr.clone()) {
                ir_functions.push(function);
            }
        }

        for record in partial_ir_records {
            if ir_record_seen.insert(record.usr.clone()) {
                ir_records.push(record);
            }
        }
    }

    log_function_catalog(format_args!(
        "extract_function_catalog: done in {:.2}s, {} declarations, {} calls, {} ir functions, {} ir records",
        extraction_started.elapsed().as_secs_f64(),
        declarations.len(),
        calls.len(),
        ir_functions.len(),
        ir_records.len()
    ));

    Ok(FunctionCatalog {
        declarations,
        calls,
        ir_functions,
        ir_records,
    })
}

fn parse_chunk(
    worker_index: usize,
    chunk: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> (
    Vec<FunctionDeclaration>,
    Vec<CallEdge>,
    Vec<ir::Function>,
    Vec<ir::Record>,
) {
    // Each worker thread needs its own load: see
    // `type_catalog::parse_chunk` for why the calling thread's
    // `load_libclang()` doesn't cover this one.
    type_catalog::load_libclang().expect(
        "libclang already loaded successfully on the calling thread; \
         per-thread load is not expected to fail",
    );

    let mut seen = HashSet::new();
    let mut declarations = Vec::new();
    let mut call_seen = HashSet::new();
    let mut calls = Vec::new();
    let mut ir_functions = Vec::new();
    let mut ir_records = Vec::new();

    unsafe {
        let index = clang_sys::clang_createIndex(0, 0);

        for unit in chunk {
            if cancellation.is_some_and(Cancellation::is_cancelled) {
                log_function_catalog(format_args!(
                    "worker {worker_index}: stopping early, cancellation requested"
                ));
                break;
            }

            let mut state = VisitorState {
                project_root,
                declarations: &mut declarations,
                seen: &mut seen,
                calls: &mut calls,
                call_seen: &mut call_seen,
                ir_functions: &mut ir_functions,
                ir_records: &mut ir_records,
            };

            log_function_catalog(format_args!(
                "parsing (worker {worker_index}): {}",
                unit.file
            ));
            let unit_started = Instant::now();

            visit_translation_unit(index, unit, &mut state);

            if let Some(progress) = progress {
                progress.mark_one_done();
            }

            log_function_catalog(format_args!(
                "parsed in {:.2}s (worker {worker_index}): {}",
                unit_started.elapsed().as_secs_f64(),
                unit.file
            ));
        }

        clang_sys::clang_disposeIndex(index);
    }

    (declarations, calls, ir_functions, ir_records)
}

fn log_function_catalog(args: fmt::Arguments<'_>) {
    eprintln!(
        "[syntax-bridge][function_catalog][{}] {args}",
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
    declarations: &'a mut Vec<FunctionDeclaration>,
    seen: &'a mut HashSet<String>,
    calls: &'a mut Vec<CallEdge>,
    call_seen: &'a mut HashSet<(String, CallResolution, String, u32, u32)>,
    ir_functions: &'a mut Vec<ir::Function>,
    ir_records: &'a mut Vec<ir::Record>,
}

fn call_identity(call: &CallEdge) -> (String, CallResolution, String, u32, u32) {
    (
        call.caller_usr.clone(),
        call.resolution.clone(),
        call.file.clone(),
        call.line,
        call.column,
    )
}

/// Mirrors `type_catalog::declaration_identity`: dedup by `usr` when present
/// (the common case), falling back to a positional key otherwise.
fn declaration_identity(declaration: &FunctionDeclaration) -> String {
    if !declaration.usr.is_empty() {
        return declaration.usr.clone();
    }

    format!(
        "pos:{:?}:{}:{}:{}:{}",
        declaration.kind, declaration.name, declaration.file, declaration.line, declaration.column
    )
}

fn push_declaration(state: &mut VisitorState<'_>, declaration: FunctionDeclaration) {
    if state.seen.insert(declaration_identity(&declaration)) {
        state.declarations.push(declaration);
    }
}

unsafe fn visit_translation_unit(
    index: clang_sys::CXIndex,
    unit: &CompilationUnit,
    state: &mut VisitorState<'_>,
) {
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

    // No `CXTranslationUnit_SkipFunctionBodies` here (see module docs): the
    // call graph lives inside bodies. `DetailedPreprocessingRecord` is kept
    // so function-like macros are still visited.
    let flags = clang_sys::CXTranslationUnit_DetailedPreprocessingRecord;

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

extern "C" fn visit_cursor(
    cursor: clang_sys::CXCursor,
    _parent: clang_sys::CXCursor,
    data: clang_sys::CXClientData,
) -> clang_sys::CXChildVisitResult {
    let state = unsafe { &mut *(data as *mut VisitorState<'_>) };
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };

    if kind == clang_sys::CXCursor_MacroDefinition {
        if let Some(TypeDeclarationKind::FunctionMacro) =
            unsafe { type_catalog::classify_macro(cursor) }
            && let Some(declaration) = describe_macro(cursor, state.project_root)
        {
            push_declaration(state, declaration);
        }
        return clang_sys::CXChildVisit_Recurse;
    }

    // E03 scope: struct/class *definitions* become IR records, lowered on
    // this same already-parsed cursor. Falls through to `Recurse` below
    // (not returned early) — the generic walk still needs to descend into
    // the record's own body to reach any inline method definitions the
    // existing `function_declaration_kind_for` handling already covers.
    if (kind == clang_sys::CXCursor_StructDecl || kind == clang_sys::CXCursor_ClassDecl)
        && unsafe { clang_sys::clang_isCursorDefinition(cursor) } != 0
        && let Some(record) = lower::cpp::lower_record(cursor, state.project_root)
    {
        state.ir_records.push(record);
    }

    // A pure virtual method (`= 0`) never has a body, so
    // `clang_isCursorDefinition` is never true for it — its declaration is
    // still the one and only cursor for that virtual slot, and skipping it
    // would make `mapping::options_for`'s interface-vs-mixin rule (US-7,
    // `docs/mapping-solver-cases.md` case B03) blind to every pure
    // interface method. Unlike an ordinary in-header prototype (which *is*
    // correctly skipped here, to avoid double-counting it alongside its
    // out-of-line definition), a pure virtual method can never gain a
    // separate defining cursor elsewhere, so this can't introduce a
    // duplicate.
    let is_pure_virtual_declaration =
        unsafe { clang_sys::clang_CXXMethod_isPureVirtual(cursor) } != 0;

    if let Some(declaration_kind) = function_declaration_kind_for(kind)
        && (unsafe { clang_sys::clang_isCursorDefinition(cursor) } != 0
            || is_pure_virtual_declaration)
        && let Some(declaration) = describe_function(cursor, declaration_kind, state.project_root)
    {
        let caller_usr = declaration.usr.clone();
        let declared_kind = declaration.kind;
        push_declaration(state, declaration);

        // IR lowering (E01–E03 scope: free functions only, see
        // `lower::cpp`'s docs) walks this same already-parsed cursor with
        // its own `clang_visitChildren` call, the same way the call-graph
        // visitor just below does — not a second `libclang` parse.
        if declared_kind == FunctionDeclarationKind::FreeFunction
            && !caller_usr.is_empty()
            && let Some(function_ir) =
                lower::cpp::lower_function(cursor, &caller_usr, state.project_root)
        {
            state.ir_functions.push(function_ir);
        }

        // The call graph only lives inside this cursor's own subtree, so
        // it's walked here with a dedicated visitor carrying `caller_usr` —
        // rather than tracked as mutable "current function" state threaded
        // through the generic walk below, which would need an explicit
        // push/pop this flat callback API has no hook for. `Continue` (not
        // `Recurse`) tells `libclang` not to also descend into this
        // cursor's children generically, since the nested call already
        // covered them — visiting a function body twice would double the
        // cost this pass already pays for parsing with bodies enabled.
        if !caller_usr.is_empty() {
            let mut call_state = CallVisitorState {
                project_root: state.project_root,
                caller_usr: &caller_usr,
                calls: &mut *state.calls,
                call_seen: &mut *state.call_seen,
            };
            unsafe {
                clang_sys::clang_visitChildren(
                    cursor,
                    visit_call_site,
                    &mut call_state as *mut CallVisitorState<'_> as *mut c_void,
                );
            }
        }

        return clang_sys::CXChildVisit_Continue;
    }

    clang_sys::CXChildVisit_Recurse
}

struct CallVisitorState<'a> {
    project_root: &'a Path,
    caller_usr: &'a str,
    calls: &'a mut Vec<CallEdge>,
    call_seen: &'a mut HashSet<(String, CallResolution, String, u32, u32)>,
}

extern "C" fn visit_call_site(
    cursor: clang_sys::CXCursor,
    _parent: clang_sys::CXCursor,
    data: clang_sys::CXClientData,
) -> clang_sys::CXChildVisitResult {
    let state = unsafe { &mut *(data as *mut CallVisitorState<'_>) };
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };

    if kind == clang_sys::CXCursor_CallExpr {
        record_call(cursor, state);
    }

    clang_sys::CXChildVisit_Recurse
}

/// Records one call site: whether its target is statically resolvable
/// (US-5 criterion 6) and, when it is, whether that resolution is only the
/// statically-known target of a virtual dispatch (criterion 3, via
/// `clang_Cursor_isDynamicCall`, `libclang`'s own answer to exactly this
/// question).
fn record_call(cursor: clang_sys::CXCursor, state: &mut CallVisitorState<'_>) {
    let Some((file, line, column)) = type_catalog::cursor_site(cursor, state.project_root) else {
        return;
    };

    let referenced = unsafe { clang_sys::clang_getCursorReferenced(cursor) };
    let is_dynamic_dispatch = unsafe { clang_sys::clang_Cursor_isDynamicCall(cursor) } != 0;

    let resolution = if unsafe { clang_sys::clang_Cursor_isNull(referenced) } != 0 {
        CallResolution::Unresolved {
            reason: "callee could not be resolved".to_owned(),
        }
    } else {
        let referenced_kind = unsafe { clang_sys::clang_getCursorKind(referenced) };
        if function_declaration_kind_for(referenced_kind).is_some() {
            // A call to a template function/method resolves `referenced` to
            // the *implicit instantiation*, whose usr differs from the
            // primary template declaration the catalog actually holds (see
            // `FunctionDeclarationKind::FunctionTemplate`'s docs). Mapping
            // back to the primary template via
            // `clang_getSpecializedCursorTemplate` is what lets a template's
            // callers show up under its own catalog entry — a no-op
            // (null cursor) for a non-template callee.
            let template = unsafe { clang_sys::clang_getSpecializedCursorTemplate(referenced) };
            let usr_source = if unsafe { clang_sys::clang_Cursor_isNull(template) } != 0 {
                referenced
            } else {
                template
            };
            let callee_usr = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(usr_source))
            };
            if callee_usr.is_empty() {
                CallResolution::Unresolved {
                    reason: "resolved callee has no stable identity".to_owned(),
                }
            } else {
                CallResolution::Resolved {
                    callee_usr,
                    is_dynamic_dispatch,
                }
            }
        } else {
            CallResolution::Unresolved {
                reason: "call target is not statically a function (e.g. a function pointer)"
                    .to_owned(),
            }
        }
    };

    let edge = CallEdge {
        caller_usr: state.caller_usr.to_owned(),
        resolution,
        file,
        line,
        column,
    };
    if state.call_seen.insert(call_identity(&edge)) {
        state.calls.push(edge);
    }
}

fn function_declaration_kind_for(kind: clang_sys::CXCursorKind) -> Option<FunctionDeclarationKind> {
    match kind {
        clang_sys::CXCursor_FunctionDecl => Some(FunctionDeclarationKind::FreeFunction),
        clang_sys::CXCursor_CXXMethod => Some(FunctionDeclarationKind::Method),
        clang_sys::CXCursor_Constructor => Some(FunctionDeclarationKind::Constructor),
        clang_sys::CXCursor_Destructor => Some(FunctionDeclarationKind::Destructor),
        clang_sys::CXCursor_FunctionTemplate => Some(FunctionDeclarationKind::FunctionTemplate),
        _ => None,
    }
}

fn describe_macro(cursor: clang_sys::CXCursor, project_root: &Path) -> Option<FunctionDeclaration> {
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    if name.is_empty() {
        return None;
    }

    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;
    let (end_line, end_column) = unsafe { type_catalog::extent_end(cursor) };
    let usr = unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(cursor)) };

    Some(FunctionDeclaration {
        signature: format!("{name}(...)"),
        name,
        kind: FunctionDeclarationKind::FunctionMacro,
        namespace: String::new(),
        owning_class_usr: None,
        file,
        line,
        column,
        end_line,
        end_column,
        usr,
        is_virtual: false,
        is_pure_virtual: false,
        is_defaulted: false,
        overridden_usrs: Vec::new(),
    })
}

fn describe_function(
    cursor: clang_sys::CXCursor,
    kind: FunctionDeclarationKind,
    project_root: &Path,
) -> Option<FunctionDeclaration> {
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor)) };
    if name.is_empty() {
        return None;
    }

    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;
    let namespace = unsafe { type_catalog::namespace_of(cursor) };
    let (end_line, end_column) = unsafe { type_catalog::extent_end(cursor) };
    let usr = unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(cursor)) };

    // `FunctionTemplate` is included because it covers both free-function
    // and method templates (same cursor kind either way, see the module's
    // policy notes) — `owning_class_of` itself already returns `None` when
    // the semantic parent isn't a record type, so this is safe for a free
    // function template too.
    let is_method_like = matches!(
        kind,
        FunctionDeclarationKind::Method
            | FunctionDeclarationKind::Constructor
            | FunctionDeclarationKind::Destructor
            | FunctionDeclarationKind::FunctionTemplate
    );
    let owning_class = if is_method_like {
        unsafe { owning_class_of(cursor) }
    } else {
        None
    };
    let owning_class_usr = owning_class.as_ref().map(|(usr, _name)| usr.clone());

    let is_virtual = unsafe { clang_sys::clang_CXXMethod_isVirtual(cursor) } != 0;
    let is_pure_virtual = unsafe { clang_sys::clang_CXXMethod_isPureVirtual(cursor) } != 0;
    let is_defaulted = unsafe { clang_sys::clang_CXXMethod_isDefaulted(cursor) } != 0;
    let overridden_usrs = unsafe { overridden_usrs_of(cursor) };
    let is_const = unsafe { clang_sys::clang_CXXMethod_isConst(cursor) } != 0;

    let mut qualified_segments: Vec<String> = Vec::new();
    if !namespace.is_empty() {
        qualified_segments.push(namespace.clone());
    }
    if let Some((_usr, class_name)) = &owning_class {
        qualified_segments.push(class_name.clone());
    }
    qualified_segments.push(name.clone());
    let qualified_name = qualified_segments.join("::");

    let signature = unsafe { build_signature(cursor, &qualified_name, kind, is_const) };

    Some(FunctionDeclaration {
        name,
        kind,
        namespace,
        owning_class_usr,
        signature,
        file,
        line,
        column,
        end_line,
        end_column,
        usr,
        is_virtual,
        is_pure_virtual,
        is_defaulted,
        overridden_usrs,
    })
}

/// The owning struct/class/union's `(usr, name)`, for a method/constructor/
/// destructor cursor — `None` for a free function, or the rare case where
/// `libclang` can't identify the semantic parent as a record type.
unsafe fn owning_class_of(cursor: clang_sys::CXCursor) -> Option<(String, String)> {
    let parent = unsafe { clang_sys::clang_getCursorSemanticParent(cursor) };
    if unsafe { clang_sys::clang_Cursor_isNull(parent) } != 0 {
        return None;
    }

    let parent_kind = unsafe { clang_sys::clang_getCursorKind(parent) };
    if !matches!(
        parent_kind,
        clang_sys::CXCursor_StructDecl
            | clang_sys::CXCursor_ClassDecl
            | clang_sys::CXCursor_UnionDecl
    ) {
        return None;
    }

    let usr = unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(parent)) };
    let name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(parent)) };
    if usr.is_empty() || name.is_empty() {
        return None;
    }

    Some((usr, name))
}

/// The `usr` of every immediate virtual method `cursor` overrides (US-5
/// criterion 4's flip side — this is how a redefinition finds the base(s) it
/// redefines). Under multiple inheritance, `clang_getOverriddenCursors`
/// already returns one cursor per overridden base method — all of them are
/// kept, not just the first, so a method overriding same-signature virtuals
/// from more than one base is attributed to every one of them.
unsafe fn overridden_usrs_of(cursor: clang_sys::CXCursor) -> Vec<String> {
    let mut cursors: *mut clang_sys::CXCursor = std::ptr::null_mut();
    let mut count: c_uint = 0;
    unsafe {
        clang_sys::clang_getOverriddenCursors(cursor, &mut cursors, &mut count);
    }

    if cursors.is_null() || count == 0 {
        return Vec::new();
    }

    let mut usrs = Vec::with_capacity(count as usize);
    for index in 0..count {
        let overridden = unsafe { *cursors.add(index as usize) };
        let usr =
            unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(overridden)) };
        if !usr.is_empty() {
            usrs.push(usr);
        }
    }

    unsafe {
        clang_sys::clang_disposeOverriddenCursors(cursors);
    }

    usrs
}

/// Builds a full signature string: return type (omitted for
/// constructors/destructors, which have none), qualified name, parameter
/// types and names, and a trailing `const` for const methods — enough detail
/// to tell overloads apart (US-5 criterion 2) beyond their `usr`.
unsafe fn build_signature(
    cursor: clang_sys::CXCursor,
    qualified_name: &str,
    kind: FunctionDeclarationKind,
    is_const: bool,
) -> String {
    let params = unsafe { parameter_list(cursor) };
    let const_suffix = if is_const { " const" } else { "" };

    let return_prefix = match kind {
        FunctionDeclarationKind::Constructor | FunctionDeclarationKind::Destructor => String::new(),
        _ => {
            let result_type = unsafe { clang_sys::clang_getCursorResultType(cursor) };
            let spelling = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(result_type))
            };
            format!("{spelling} ")
        }
    };

    format!("{return_prefix}{qualified_name}({params}){const_suffix}")
}

/// Walks `cursor`'s direct children collecting `ParmDecl`s, rather than
/// `clang_Cursor_getNumArguments`/`clang_Cursor_getArgument`: those two only
/// support a fixed set of cursor kinds that, per `libclang`, does not
/// include `CXCursor_FunctionTemplate` — confirmed empirically, they
/// silently report zero arguments for a method/function template, which
/// would make every template's signature claim an empty parameter list.
/// Child-visiting works uniformly across every callable kind this module
/// handles, so there's no need for a kind-specific fallback.
unsafe fn parameter_list(cursor: clang_sys::CXCursor) -> String {
    let mut parts: Vec<String> = Vec::new();

    extern "C" fn collect(
        cursor: clang_sys::CXCursor,
        _parent: clang_sys::CXCursor,
        data: clang_sys::CXClientData,
    ) -> clang_sys::CXChildVisitResult {
        let parts = unsafe { &mut *(data as *mut Vec<String>) };
        if unsafe { clang_sys::clang_getCursorKind(cursor) } == clang_sys::CXCursor_ParmDecl {
            let argument_type = unsafe { clang_sys::clang_getCursorType(cursor) };
            let type_spelling = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(argument_type))
            };
            let argument_name = unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(cursor))
            };
            parts.push(if argument_name.is_empty() {
                type_spelling
            } else {
                format!("{type_spelling} {argument_name}")
            });
        }
        clang_sys::CXChildVisit_Continue
    }

    unsafe {
        clang_sys::clang_visitChildren(
            cursor,
            collect,
            &mut parts as *mut Vec<String> as *mut c_void,
        );
    }

    parts.join(", ")
}
