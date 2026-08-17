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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::ffi::CString;
use std::fmt;
use std::os::raw::{c_int, c_uint, c_void};
use std::path::Path;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::ingest::CompilationUnit;
use crate::ir;
use crate::lower;
use crate::mapping;
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
    /// Whether this is a `static` member (`Method` only — always `false`
    /// for every other `kind`). E13: C++ lets a `static` and a non-`static`
    /// member share a name (resolved by call-site syntax, not a real
    /// overload); Dart forbids it outright (`conflicting_static_and_instance`)
    /// — `mapping::overload_options_for` needs this to tell that collision
    /// apart from an ordinary same-arity overload, which E07's
    /// `"parametro-opcional"` decision would otherwise (wrongly) apply to it.
    pub is_static: bool,
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
    /// Whether `visit_cursor` found a real `libclang` definition for this
    /// usr in *any* parsed compilation unit — `true` for everything before
    /// this field existed (a definition is still required to reach
    /// `push_declaration` at all, except for the one new case below).
    /// `false` only for a free-function prototype cataloged with no
    /// definition anywhere in the project (`docs/plans/lista-de-externos.md`):
    /// the auto-detection signal `externals::effective_external_set` uses to
    /// tell "this project never compiles a body for this symbol" apart from
    /// an ordinary declared-and-defined function. Always `true` for
    /// `Method`/`Constructor`/`Destructor`/`FunctionTemplate`/
    /// `FunctionMacro` — those kinds are still only cataloged from a
    /// definition (or a pure-virtual declaration) cursor, unchanged.
    pub has_definition: bool,
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

/// One worker's local (not yet cross-chunk deduplicated) results — shared
/// between `parse_chunk`'s return type, `finish_function_catalog`'s
/// parameter, and `extraction::WorkerPartials`, which all merge the same
/// tuple shape. The `Vec<bool>` is `ir_function_is_prototype` — parallel to
/// the `Vec<ir::Function>` right before it (see `VisitorState`'s field of
/// the same name for why this can't just be folded into `ir::Function`
/// itself).
pub(crate) type FunctionCatalogPartial = (
    Vec<FunctionDeclaration>,
    Vec<CallEdge>,
    Vec<ir::Function>,
    Vec<bool>,
    Vec<ir::Record>,
    Vec<ir::Enum>,
);

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
    /// IR of every `enum`/`enum class` *definition* — caso 4 of
    /// `docs/plans/verovio-6.2-pointer-types.md`, mirrors `ir_records`.
    pub ir_enums: Vec<ir::Enum>,
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

    let catalog = finish_function_catalog(partials);

    log_function_catalog(format_args!(
        "extract_function_catalog: done in {:.2}s, {} declarations, {} calls, {} ir functions, {} ir records",
        extraction_started.elapsed().as_secs_f64(),
        catalog.declarations.len(),
        catalog.calls.len(),
        catalog.ir_functions.len(),
        catalog.ir_records.len()
    ));

    Ok(catalog)
}

/// Merges every worker's local partials into the final `FunctionCatalog`,
/// including the post-merge passes (overload renaming, record-name
/// disambiguation, RAII scope guards) that only make sense once every
/// translation unit's declarations/calls/IR are all in one place — factored
/// out of `extract_function_catalog_cancellable` so
/// `extraction::extract_project_catalogs_cancellable` (which drives its own
/// workers directly, sharing a parse with `pointer_catalog`) can reuse the
/// exact same merge and post-processing behavior.
pub(crate) fn finish_function_catalog(partials: Vec<FunctionCatalogPartial>) -> FunctionCatalog {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut declarations = Vec::new();
    let mut call_seen = HashSet::new();
    let mut calls = Vec::new();
    // Index (not just membership), and a parallel "is this stored entry a
    // prototype" map — same upgrade-in-place rationale as `seen`/
    // `declarations` above: two different workers can each see only one
    // side (one the prototype, one the real definition) of the same usr,
    // and whichever arrives second in this loop must be able to replace an
    // already-accepted prototype stand-in, never the reverse. See
    // `VisitorState.ir_function_is_prototype`'s doc comment for why an
    // `ir::Function` value alone can't answer "is this a prototype" on its
    // own.
    let mut ir_function_index: HashMap<String, usize> = HashMap::new();
    let mut ir_function_is_prototype: HashMap<String, bool> = HashMap::new();
    let mut ir_functions = Vec::new();
    let mut ir_record_seen = HashSet::new();
    let mut ir_records = Vec::new();
    let mut ir_enum_seen = HashSet::new();
    let mut ir_enums = Vec::new();

    for (
        partial_declarations,
        partial_calls,
        partial_ir_functions,
        partial_ir_function_is_prototype,
        partial_ir_records,
        partial_ir_enums,
    ) in partials
    {
        for declaration in partial_declarations {
            let identity = declaration_identity(&declaration);
            match seen.get(&identity) {
                None => {
                    seen.insert(identity, declarations.len());
                    declarations.push(declaration);
                }
                Some(&index) => {
                    if !declarations[index].has_definition && declaration.has_definition {
                        declarations[index] = declaration;
                    }
                }
            }
        }

        for call in partial_calls {
            if call_seen.insert(call_identity(&call)) {
                calls.push(call);
            }
        }

        for (function, is_prototype) in partial_ir_functions
            .into_iter()
            .zip(partial_ir_function_is_prototype)
        {
            match ir_function_index.get(&function.usr) {
                None => {
                    ir_function_index.insert(function.usr.clone(), ir_functions.len());
                    ir_function_is_prototype.insert(function.usr.clone(), is_prototype);
                    ir_functions.push(function);
                }
                Some(&index) => {
                    if ir_function_is_prototype[&function.usr] && !is_prototype {
                        ir_functions[index] = function;
                        ir_function_is_prototype.insert(ir_functions[index].usr.clone(), false);
                    }
                }
            }
        }

        for record in partial_ir_records {
            if ir_record_seen.insert(record.usr.clone()) {
                ir_records.push(record);
            }
        }

        for enum_decl in partial_ir_enums {
            if ir_enum_seen.insert(enum_decl.usr.clone()) {
                ir_enums.push(enum_decl);
            }
        }
    }

    // US-7/E07: the first real consultation of `mapping::overload_options_for`
    // by the generation pipeline itself, not just its own unit tests — see
    // that function's doc comment for what it decides and why only a subset
    // of its decisions are acted on here.
    apply_overload_renames(&mut ir_functions, &mut ir_records, &declarations, &calls);

    // Verovio 6.2.0 diagnosis (`docs/plans/diagnostico-verovio-6.2.0.md`
    // achado 2): two distinct C++ classes with the same short name in
    // different namespaces both lower correctly, but `emit::dart` names the
    // Dart class from the bare spelling alone — real occurrence in that
    // corpus (two unrelated `Object`s). Runs before RAII (order doesn't
    // matter between the two, but matches declaration order above).
    apply_record_name_disambiguation(&mut ir_functions, &mut ir_records);

    // Has to run after the loop above, when `ir_enums` is finally complete:
    // only then is "no declaration for this usr" a settled fact rather than
    // an enum that a later translation unit still might contribute.
    reject_undeclared_enum_refs(&mut ir_functions, &mut ir_records, &ir_enums);

    // E12: RAII — see the function's own doc comment for exactly what this
    // rewrites and why it has to run after every record (with its
    // destructor, if any) is already fully lowered.
    apply_raii_scope_guards(&mut ir_functions, &ir_records);

    FunctionCatalog {
        declarations,
        calls,
        ir_functions,
        ir_records,
        ir_enums,
    }
}

/// After every function/method is lowered with its original C++ name,
/// consults `mapping::overload_options_for` for each group of
/// same-(owning-class, name) declarations (US-7; E07 is the first degrau
/// where the generation pipeline itself calls this solver, not just its own
/// unit tests) and, when the decision requires a rename
/// (`"renomear-por-tipo"`/`"renomear-const-nao-const"`), renames the
/// corresponding `ir::Function`/`ir::Method` *and* every call site
/// referencing it by USR — computed once per group and applied everywhere,
/// the same "can never disagree" discipline `lower::cpp::constructor_ordinal`
/// already uses for E04's multiple constructors, so a call site can never
/// end up pointing at a name that no longer exists.
///
/// A decision that *doesn't* require a rename (`"assinatura-unica"`,
/// `"parametro-opcional"`) leaves the group untouched. `"parametro-opcional"`
/// (overloads differing only in arity) is deliberately not acted on here —
/// unlike a rename, folding it into Dart would mean *merging* two separate
/// `Function`/`Method` IR entries into one with an optional trailing
/// parameter, a different kind of change, and no fixture in this corpus
/// forces it yet (see `examples/E07-sobrecarga-e-parametros-default/NOTES.md`).
/// Leaving such a group's declarations with their shared original name is
/// not silent, either: two same-named top-level Dart declarations fail to
/// compile, so `dart analyze` surfaces the gap loudly if it's ever reached.
fn apply_overload_renames(
    ir_functions: &mut [ir::Function],
    ir_records: &mut [ir::Record],
    declarations: &[FunctionDeclaration],
    calls: &[CallEdge],
) {
    let facts = mapping::ProjectFacts::new_full(&[], &[], declarations, calls);

    let mut groups: BTreeMap<(Option<String>, String), Vec<&FunctionDeclaration>> = BTreeMap::new();
    for declaration in declarations {
        if !matches!(
            declaration.kind,
            FunctionDeclarationKind::FreeFunction | FunctionDeclarationKind::Method
        ) {
            continue;
        }
        groups
            .entry((
                declaration.owning_class_usr.clone(),
                declaration.name.clone(),
            ))
            .or_default()
            .push(declaration);
    }

    // Indexed by `usr` once, up front — `find_ir_params` used to rescan all
    // of `ir_functions`/every record's `methods` per renamed declaration,
    // which adds up across every multi-member overload group. Built (and
    // dropped) before `ir_functions`/`ir_records` are mutated below, so it
    // doesn't conflict with the `&mut` borrows the rename-application loop
    // needs.
    let mut ir_functions_by_usr: HashMap<&str, &[ir::Param]> =
        HashMap::with_capacity(ir_functions.len());
    for function in ir_functions.iter() {
        ir_functions_by_usr.insert(function.usr.as_str(), function.params.as_slice());
    }
    let mut ir_methods_by_usr: HashMap<&str, &[ir::Param]> = HashMap::new();
    for record in ir_records.iter() {
        for method in &record.methods {
            ir_methods_by_usr.insert(method.usr.as_str(), method.params.as_slice());
        }
    }

    let mut renames: HashMap<String, String> = HashMap::new();
    for ((owning_class_usr, name), group) in &groups {
        if group.len() <= 1 {
            continue;
        }
        let Some(option) = mapping::overload_options_for(owning_class_usr.as_deref(), name, &facts)
            .into_iter()
            .next()
        else {
            continue;
        };
        // E13: a static/instance name collision can't be told apart by
        // parameter *type* the way `dart_overload_name` does for the other
        // two ids — the instance side may take zero parameters (`Reduce()`),
        // leaving nothing to build a distinguishing suffix from at all.
        // Only the `static` declaration(s) get a suffix; the instance one
        // keeps its original name (already unambiguous among *other*
        // instance members) and is left out of `renames` entirely, matching
        // every other id's "only insert what actually changes" shape.
        if option.id == "renomear-estatico-instancia" {
            for declaration in group {
                if declaration.is_static {
                    renames.insert(declaration.usr.clone(), format!("{name}Static"));
                }
            }
            continue;
        }
        // Achado 1 (`docs/plans/diagnostico-verovio-6.2.0.md`): this id
        // means the two declarations have the *same parameter list*
        // (`mapping::overload_options_for`'s own `same_params` check) —
        // `dart_overload_name` below computes its suffix purely from
        // parameter types, so handing it both sides here would compute the
        // same (usually empty) suffix twice and rename them to the same
        // name, the exact bug this id exists to catch. Constness never
        // survives into the IR (`ir::Method` has no such field — Dart
        // doesn't dispatch on it), so it's read directly off each
        // declaration's own signature text via `mapping::signature_is_const`
        // instead. Only the `const` side is renamed, the same "leave the
        // other one be" shape `"renomear-estatico-instancia"` already uses
        // above — the non-`const` side keeps its original name, already
        // unambiguous among the record's *other* members.
        if option.id == "renomear-const-nao-const" {
            for declaration in group {
                if mapping::signature_is_const(&declaration.signature) {
                    renames.insert(declaration.usr.clone(), format!("{name}Const"));
                }
            }
            continue;
        }
        if option.id != "renomear-por-tipo" {
            continue;
        }
        for declaration in group {
            let params = ir_functions_by_usr
                .get(declaration.usr.as_str())
                .or_else(|| ir_methods_by_usr.get(declaration.usr.as_str()))
                .copied();
            if let Some(params) = params {
                renames.insert(declaration.usr.clone(), dart_overload_name(name, params));
            }
        }
    }

    if renames.is_empty() {
        return;
    }

    for function in ir_functions.iter_mut() {
        if let Some(new_name) = renames.get(&function.usr) {
            function.name = new_name.clone();
        }
        rename_calls_in_params(&mut function.params, &renames);
        rename_calls_in_stmts(&mut function.body, &renames);
    }
    for record in ir_records.iter_mut() {
        for method in &mut record.methods {
            if let Some(new_name) = renames.get(&method.usr) {
                method.name = new_name.clone();
            }
            rename_calls_in_params(&mut method.params, &renames);
            if let Some(body) = &mut method.body {
                rename_calls_in_stmts(body, &renames);
            }
        }
        for constructor in &mut record.constructors {
            rename_calls_in_params(&mut constructor.params, &renames);
            rename_calls_in_stmts(&mut constructor.body, &renames);
        }
    }
}

/// Verovio 6.2.0 diagnosis (`docs/plans/diagnostico-verovio-6.2.0.md`
/// achado 2): two distinct records can share a bare `name` — the corpus's
/// own case is two unrelated `Object`s in different namespaces — and
/// `emit::dart` names the Dart class from that bare spelling alone, so both
/// would print as `class Object`, an outright `duplicate_definition` when
/// they land in the same file. A record whose name is unique in the whole
/// module (the overwhelming common case) is never touched — `renames` stays
/// empty and every walk below is a no-op.
///
/// The pattern (deliberately the simplest one that's still always correct,
/// not a final design): qualify each colliding record with its own C++
/// namespace, PascalCased and prefixed onto the original name
/// (`ns1::Ponto` → `Ns1Ponto`). A record with no namespace keeps its bare
/// name as its first candidate. Whatever still collides after that (two
/// colliding records in the *same* namespace, or namespace-less on both
/// sides) gets a stable numeric suffix in `usr` order — not pretty, but
/// always unique and fully deterministic, which is what actually matters
/// here: two runs of the same project must always rename the same way.
fn apply_record_name_disambiguation(
    ir_functions: &mut [ir::Function],
    ir_records: &mut [ir::Record],
) {
    let mut groups: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, record) in ir_records.iter().enumerate() {
        groups.entry(record.name.clone()).or_default().push(index);
    }

    let mut renames: HashMap<String, String> = HashMap::new();
    for (name, indices) in &groups {
        if indices.len() <= 1 {
            continue;
        }

        let mut candidates: Vec<String> = indices
            .iter()
            .map(|&index| {
                let record = &ir_records[index];
                if record.namespace.is_empty() {
                    record.name.clone()
                } else {
                    format!(
                        "{}{}",
                        pascal_case_namespace(&record.namespace),
                        record.name
                    )
                }
            })
            .collect();

        let mut seen_candidates: HashMap<String, usize> = HashMap::new();
        for candidate in &mut candidates {
            let count = seen_candidates.entry(candidate.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                candidate.push_str(&count.to_string());
            }
        }

        for (&index, candidate) in indices.iter().zip(candidates.iter()) {
            if candidate != name {
                renames.insert(ir_records[index].usr.clone(), candidate.clone());
            }
        }
    }

    if renames.is_empty() {
        return;
    }

    for function in ir_functions.iter_mut() {
        rename_record_refs_in_type(&mut function.return_type, &renames);
        rename_record_refs_in_params(&mut function.params, &renames);
        rename_record_refs_in_stmts(&mut function.body, &renames);
    }
    for record in ir_records.iter_mut() {
        if let Some(new_name) = renames.get(&record.usr) {
            record.name = new_name.clone();
        }
        for field in record
            .fields
            .iter_mut()
            .chain(record.static_fields.iter_mut())
        {
            rename_record_refs_in_type(&mut field.ty, &renames);
        }
        if let Some(base) = &mut record.base_class {
            rename_record_refs_in_base(base, &renames);
        }
        for mixin in &mut record.mixins {
            rename_record_refs_in_base(mixin, &renames);
        }
        for constructor in &mut record.constructors {
            rename_record_refs_in_params(&mut constructor.params, &renames);
            rename_record_refs_in_stmts(&mut constructor.body, &renames);
        }
        for method in &mut record.methods {
            rename_record_refs_in_type(&mut method.return_type, &renames);
            rename_record_refs_in_params(&mut method.params, &renames);
            if let Some(body) = &mut method.body {
                rename_record_refs_in_stmts(body, &renames);
            }
        }
        if let Some(destructor) = &mut record.destructor {
            rename_record_refs_in_stmts(destructor, &renames);
        }
    }
}

/// `ns1::detail::Foo` → `Ns1Detail` — the namespace-qualifying prefix
/// `apply_record_name_disambiguation` sticks onto a colliding record's own
/// name. Each `::`-separated segment gets its first letter uppercased; the
/// segments are then joined with no separator, matching the capitalized,
/// no-underscore shape every other Dart identifier in this emitter already
/// uses (`ClassName`, not `class_name`).
fn pascal_case_namespace(namespace: &str) -> String {
    namespace
        .split("::")
        .map(|segment| {
            let mut chars = segment.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn rename_record_refs_in_type(ty: &mut ir::Type, renames: &HashMap<String, String>) {
    match ty {
        ir::Type::Record { usr, name } => {
            if let Some(new_name) = renames.get(usr) {
                *name = new_name.clone();
            }
        }
        ir::Type::List(element) | ir::Type::Set(element) | ir::Type::Nullable(element) => {
            rename_record_refs_in_type(element, renames)
        }
        ir::Type::Map(key, value) => {
            rename_record_refs_in_type(key, renames);
            rename_record_refs_in_type(value, renames);
        }
        ir::Type::Tuple(elements) => {
            for element in elements {
                rename_record_refs_in_type(element, renames);
            }
        }
        ir::Type::Int
        | ir::Type::Bool
        | ir::Type::Double
        | ir::Type::Void
        | ir::Type::Str
        | ir::Type::Enum { .. }
        | ir::Type::Unsupported(_) => {}
    }
}

fn rename_record_refs_in_base(base: &mut ir::BaseClass, renames: &HashMap<String, String>) {
    if let Some(new_name) = renames.get(&base.usr) {
        base.name = new_name.clone();
    }
}

fn rename_record_refs_in_params(params: &mut [ir::Param], renames: &HashMap<String, String>) {
    for param in params {
        rename_record_refs_in_type(&mut param.ty, renames);
        if let Some(default_value) = &mut param.default_value {
            rename_record_refs_in_expr(default_value, renames);
        }
    }
}

fn rename_record_refs_in_expr(expr: &mut ir::Expr, renames: &HashMap<String, String>) {
    IrRefVisitor {
        on_type: &mut |ty: &mut ir::Type| {
            if let ir::Type::Record { usr, name } = ty
                && let Some(new_name) = renames.get(usr)
            {
                *name = new_name.clone();
            }
        },
        on_record_construct: &mut |type_usr: &str, type_name: &mut String| {
            if let Some(new_name) = renames.get(type_usr) {
                *type_name = new_name.clone();
            }
        },
    }
    .visit_expr(expr);
}

/// Rewrites every `Type::Enum` whose `usr` no `ir::Enum` in this catalog
/// declares back into `Type::Unsupported`.
///
/// `lower::cpp::lower_type` mints a `Type::Enum` from nothing but the
/// cursor in front of it — it has no `project_root` to test against, and
/// an enum is routinely used as a type long before its own declaration is
/// visited, so it can't tell a project enum from `std::memory_order` or
/// from an empty `enum Vazio {};` that Dart can't represent at all.
/// `lower::cpp::lower_enum` applies all of those tests and simply doesn't
/// emit a declaration for an enum that fails one. Reconciling the two here,
/// against the finished list, is what keeps the pair honest: a `Type::Enum`
/// naming a type no file declares emits Dart that references an undefined
/// class, which `dart analyze` rejects — the silently-broken output that
/// `Unsupported` (a loud `UnimplementedError` at the use site) exists to
/// replace.
fn reject_undeclared_enum_refs(
    ir_functions: &mut [ir::Function],
    ir_records: &mut [ir::Record],
    ir_enums: &[ir::Enum],
) {
    let declared: HashSet<&str> = ir_enums.iter().map(|decl| decl.usr.as_str()).collect();

    let mut reject = |ty: &mut ir::Type| {
        if let ir::Type::Enum { usr, name } = ty
            && !declared.contains(usr.as_str())
        {
            *ty = ir::Type::Unsupported(name.clone());
        }
    };

    for function in ir_functions.iter_mut() {
        reject(&mut function.return_type);
        reject_in_params(&mut function.params, &mut reject);
        reject_in_stmts(&mut function.body, &mut reject);
    }

    for record in ir_records.iter_mut() {
        for field in record
            .fields
            .iter_mut()
            .chain(record.static_fields.iter_mut())
        {
            reject(&mut field.ty);
        }
        for constructor in &mut record.constructors {
            reject_in_params(&mut constructor.params, &mut reject);
            reject_in_stmts(&mut constructor.body, &mut reject);
        }
        for method in &mut record.methods {
            reject(&mut method.return_type);
            reject_in_params(&mut method.params, &mut reject);
            if let Some(body) = &mut method.body {
                reject_in_stmts(body, &mut reject);
            }
        }
        if let Some(destructor) = &mut record.destructor {
            reject_in_stmts(destructor, &mut reject);
        }
    }
}

fn reject_in_params(params: &mut [ir::Param], reject: &mut dyn FnMut(&mut ir::Type)) {
    for param in params {
        reject(&mut param.ty);
        if let Some(default_value) = &mut param.default_value {
            IrRefVisitor {
                on_type: reject,
                on_record_construct: &mut |_usr: &str, _name: &mut String| {},
            }
            .visit_expr(default_value);
        }
    }
}

fn reject_in_stmts(stmts: &mut [ir::Stmt], reject: &mut dyn FnMut(&mut ir::Type)) {
    IrRefVisitor {
        on_type: reject,
        on_record_construct: &mut |_usr: &str, _name: &mut String| {},
    }
    .visit_stmts(stmts);
}

/// The two things a post-pass can want to rewrite as it walks a lowered
/// body: every `ir::Type` that appears anywhere in it, and the
/// `type_usr`/`type_name` pair a `RecordConstruct`/`ConstructorCall`
/// carries out-of-band (a construction site names its Dart class in a
/// `String`, not in an `ir::Type`, so a type-only visitor would miss it).
///
/// One traversal serves every such pass. Each one that spelled out its own
/// walk would be a second place that has to stay exhaustive as `ir::Stmt`
/// and `ir::Expr` grow — and a pass that silently skips a variant produces
/// exactly the half-rewritten IR these passes exist to prevent.
struct IrRefVisitor<'a> {
    on_type: &'a mut dyn FnMut(&mut ir::Type),
    on_record_construct: &'a mut dyn FnMut(&str, &mut String),
}

impl IrRefVisitor<'_> {
    fn visit_stmts(&mut self, stmts: &mut [ir::Stmt]) {
        for stmt in stmts {
            self.visit_stmt(stmt);
        }
    }

    fn visit_stmt(&mut self, stmt: &mut ir::Stmt) {
        match stmt {
            ir::Stmt::Return { value, .. } => {
                if let Some(expr) = value {
                    self.visit_expr(expr);
                }
            }
            ir::Stmt::VarDecl { ty, init, .. } => {
                self.visit_type(ty);
                if let Some(expr) = init {
                    self.visit_expr(expr);
                }
            }
            ir::Stmt::Assign { value, .. } => self.visit_expr(value),
            ir::Stmt::FieldAssign { target, value, .. } => {
                self.visit_expr(target);
                self.visit_expr(value);
            }
            ir::Stmt::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.visit_expr(condition);
                self.visit_stmts(then_branch);
                self.visit_stmts(else_branch);
            }
            ir::Stmt::While {
                condition, body, ..
            } => {
                self.visit_expr(condition);
                self.visit_stmts(body);
            }
            ir::Stmt::For {
                init,
                condition,
                increment,
                body,
                ..
            } => {
                if let Some(init) = init {
                    self.visit_stmt(init);
                }
                if let Some(condition) = condition {
                    self.visit_expr(condition);
                }
                if let Some(increment) = increment {
                    self.visit_stmt(increment);
                }
                self.visit_stmts(body);
            }
            ir::Stmt::ExprStmt { expr, .. } => self.visit_expr(expr),
            ir::Stmt::Throw { value, .. } => self.visit_expr(value),
            ir::Stmt::TryCatch {
                try_body,
                catch_type,
                catch_body,
                ..
            } => {
                self.visit_stmts(try_body);
                self.visit_type(catch_type);
                self.visit_stmts(catch_body);
            }
            ir::Stmt::TryFinally {
                try_body,
                finally_body,
                ..
            } => {
                self.visit_stmts(try_body);
                self.visit_stmts(finally_body);
            }
            ir::Stmt::TupleAssign { targets, value, .. } => {
                for target in targets {
                    self.visit_expr(target);
                }
                self.visit_expr(value);
            }
            ir::Stmt::Unsupported { .. } => {}
        }
    }

    fn visit_expr(&mut self, expr: &mut ir::Expr) {
        match expr {
            ir::Expr::IntLiteral { .. }
            | ir::Expr::DoubleLiteral { .. }
            | ir::Expr::BoolLiteral { .. }
            | ir::Expr::StringLiteral { .. }
            | ir::Expr::Unsupported { .. } => {}
            ir::Expr::Ref { ty, .. } | ir::Expr::This { ty, .. } => {
                self.visit_type(ty);
            }
            ir::Expr::Binary { lhs, rhs, ty, .. } => {
                self.visit_type(ty);
                self.visit_expr(lhs);
                self.visit_expr(rhs);
            }
            ir::Expr::Unary { operand, ty, .. } | ir::Expr::Convert { operand, ty, .. } => {
                self.visit_type(ty);
                self.visit_expr(operand);
            }
            ir::Expr::Call {
                target, ty, args, ..
            } => {
                self.visit_type(ty);
                if let Some(target) = target {
                    self.visit_expr(target);
                }
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            ir::Expr::FieldAccess { target, ty, .. } => {
                self.visit_type(ty);
                self.visit_expr(target);
            }
            ir::Expr::RecordConstruct {
                type_usr,
                type_name,
                fields,
                ..
            } => {
                (self.on_record_construct)(type_usr, type_name);
                for (_name, value) in fields {
                    self.visit_expr(value);
                }
            }
            ir::Expr::ConstructorCall {
                type_usr,
                type_name,
                args,
                ..
            } => {
                (self.on_record_construct)(type_usr, type_name);
                for arg in args {
                    self.visit_expr(arg);
                }
            }
            ir::Expr::Index {
                target, index, ty, ..
            } => {
                self.visit_type(ty);
                self.visit_expr(target);
                self.visit_expr(index);
            }
            ir::Expr::StringByteLength { target, .. } => self.visit_expr(target),
            ir::Expr::Tuple { values, .. } => {
                for value in values {
                    self.visit_expr(value);
                }
            }
        }
    }

    /// Composite element types are visited before the node that holds
    /// them, so a visitor that *replaces* a node (`reject_undeclared_enum_refs`
    /// turning an `Enum` into an `Unsupported`) can't drop children that
    /// were never offered to it.
    fn visit_type(&mut self, ty: &mut ir::Type) {
        match ty {
            ir::Type::List(element) | ir::Type::Set(element) | ir::Type::Nullable(element) => {
                self.visit_type(element)
            }
            ir::Type::Map(key, value) => {
                self.visit_type(key);
                self.visit_type(value);
            }
            ir::Type::Tuple(elements) => {
                for element in elements {
                    self.visit_type(element);
                }
            }
            ir::Type::Int
            | ir::Type::Bool
            | ir::Type::Double
            | ir::Type::Void
            | ir::Type::Str
            | ir::Type::Record { .. }
            | ir::Type::Enum { .. }
            | ir::Type::Unsupported(_) => {}
        }
        (self.on_type)(ty);
    }
}

fn rename_record_refs_in_stmts(stmts: &mut [ir::Stmt], renames: &HashMap<String, String>) {
    IrRefVisitor {
        on_type: &mut |ty: &mut ir::Type| {
            if let ir::Type::Record { usr, name } = ty
                && let Some(new_name) = renames.get(usr)
            {
                *name = new_name.clone();
            }
        },
        on_record_construct: &mut |type_usr: &str, type_name: &mut String| {
            if let Some(new_name) = renames.get(type_usr) {
                *type_name = new_name.clone();
            }
        },
    }
    .visit_stmts(stmts);
}

fn rename_calls_in_params(params: &mut [ir::Param], renames: &HashMap<String, String>) {
    for param in params {
        if let Some(default_value) = &mut param.default_value {
            rename_calls_in_expr(default_value, renames);
        }
    }
}

/// E12: RAII. C++ runs a local's destructor deterministically the moment it
/// leaves scope; Dart has no such hook, so the only construct that runs
/// code unconditionally at block exit — `try`/`finally` — has to stand in
/// for it. For every free function (methods/constructors aren't scanned —
/// no fixture yet declares an RAII local *inside* one, and scoping this to
/// what's actually forced avoids guessing at the interaction with `this`),
/// finds the *first* top-level `VarDecl` whose type is a record with a real
/// destructor (`Record::destructor`, only `Some` for one with actual
/// teardown logic — see that field's doc comment) and wraps everything
/// after it in a `Stmt::TryFinally` whose `finally_body` is that
/// destructor's own statements, with every `Expr::This` replaced by a
/// reference to the local itself (`replace_this_with_ref_in_stmts` —
/// correct because the destructor's body was lowered exactly like any
/// other method's, receiver-implicit, and is now being spliced somewhere
/// that has no `this` of its own to be implicit about).
///
/// Only the *first* qualifying local in a function is wrapped, not every
/// one — two RAII locals in the same function would need *nested*
/// `try`/`finally` (each one's guard active only from its own declaration
/// onward), which no fixture forces yet; a second such local today keeps
/// its plain `VarDecl` and never gets its destructor called, a known gap
/// rather than a silently wrong nesting.
fn apply_raii_scope_guards(ir_functions: &mut [ir::Function], ir_records: &[ir::Record]) {
    let destructors: HashMap<&str, (&str, &[ir::Stmt])> = ir_records
        .iter()
        .filter_map(|record| {
            record
                .destructor
                .as_ref()
                .map(|body| (record.usr.as_str(), (record.name.as_str(), body.as_slice())))
        })
        .collect();
    if destructors.is_empty() {
        return;
    }

    for function in ir_functions.iter_mut() {
        apply_raii_scope_guard_to_stmts(&mut function.body, &destructors);
    }
}

fn apply_raii_scope_guard_to_stmts(
    stmts: &mut Vec<ir::Stmt>,
    destructors: &HashMap<&str, (&str, &[ir::Stmt])>,
) {
    let guard = stmts.iter().enumerate().find_map(|(index, stmt)| {
        let ir::Stmt::VarDecl {
            name,
            ty: ir::Type::Record { usr, .. },
            ..
        } = stmt
        else {
            return None;
        };
        destructors
            .get(usr.as_str())
            .map(|(_, body)| (index, name.clone(), *body))
    });
    let Some((index, var_name, destructor_body)) = guard else {
        return;
    };

    let mut finally_body = destructor_body.to_vec();
    replace_this_with_ref_in_stmts(&mut finally_body, &var_name);

    let try_body = stmts.split_off(index + 1);

    // The destructor's own body only ever touches state reachable without
    // `this` — e.g. a static field — the `This`→`Ref` substitution above has
    // nothing to replace, and the guard local ends up declared but never
    // read anywhere in the emitted Dart. Dart's analyzer treats that as an
    // `unused_local_variable` warning (the test harness's `dart analyze`
    // step fails on any warning), so when neither `try_body` nor
    // `finally_body` actually names the local, drop the binding and keep
    // only the constructor call for its side effect — still runs the same
    // construction, just as a bare expression statement instead of a named
    // declaration.
    let guard_decl = stmts.pop().expect("guard index was just read from stmts");
    let ir::Stmt::VarDecl {
        name,
        ty,
        init,
        origin,
    } = guard_decl
    else {
        unreachable!("guard_decl was matched as a VarDecl above");
    };
    let referenced = stmts_reference_name(&try_body, &var_name)
        || stmts_reference_name(&finally_body, &var_name);
    if referenced {
        stmts.push(ir::Stmt::VarDecl {
            name,
            ty,
            init,
            origin: origin.clone(),
        });
    } else if let Some(init) = init {
        stmts.push(ir::Stmt::ExprStmt {
            expr: init,
            origin: origin.clone(),
        });
    }

    stmts.push(ir::Stmt::TryFinally {
        try_body,
        finally_body,
        origin,
    });
}

fn stmts_reference_name(stmts: &[ir::Stmt], name: &str) -> bool {
    stmts.iter().any(|stmt| stmt_references_name(stmt, name))
}

fn stmt_references_name(stmt: &ir::Stmt, name: &str) -> bool {
    match stmt {
        ir::Stmt::Return { value, .. } => value
            .as_ref()
            .is_some_and(|expr| expr_references_name(expr, name)),
        ir::Stmt::VarDecl { init, .. } => init
            .as_ref()
            .is_some_and(|expr| expr_references_name(expr, name)),
        ir::Stmt::Assign { value, .. } => expr_references_name(value, name),
        ir::Stmt::FieldAssign { target, value, .. } => {
            expr_references_name(target, name) || expr_references_name(value, name)
        }
        ir::Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            expr_references_name(condition, name)
                || stmts_reference_name(then_branch, name)
                || stmts_reference_name(else_branch, name)
        }
        ir::Stmt::While {
            condition, body, ..
        } => expr_references_name(condition, name) || stmts_reference_name(body, name),
        ir::Stmt::For {
            init,
            condition,
            increment,
            body,
            ..
        } => {
            init.as_ref()
                .is_some_and(|stmt| stmt_references_name(stmt, name))
                || condition
                    .as_ref()
                    .is_some_and(|expr| expr_references_name(expr, name))
                || increment
                    .as_ref()
                    .is_some_and(|stmt| stmt_references_name(stmt, name))
                || stmts_reference_name(body, name)
        }
        ir::Stmt::ExprStmt { expr, .. } | ir::Stmt::Throw { value: expr, .. } => {
            expr_references_name(expr, name)
        }
        ir::Stmt::TryCatch {
            try_body,
            catch_body,
            ..
        } => stmts_reference_name(try_body, name) || stmts_reference_name(catch_body, name),
        ir::Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => stmts_reference_name(try_body, name) || stmts_reference_name(finally_body, name),
        ir::Stmt::TupleAssign { targets, value, .. } => {
            targets
                .iter()
                .any(|target| expr_references_name(target, name))
                || expr_references_name(value, name)
        }
        ir::Stmt::Unsupported { .. } => false,
    }
}

fn expr_references_name(expr: &ir::Expr, name: &str) -> bool {
    match expr {
        ir::Expr::Ref { name: ref_name, .. } => ref_name == name,
        ir::Expr::IntLiteral { .. }
        | ir::Expr::DoubleLiteral { .. }
        | ir::Expr::BoolLiteral { .. }
        | ir::Expr::StringLiteral { .. }
        | ir::Expr::This { .. }
        | ir::Expr::Unsupported { .. } => false,
        ir::Expr::Binary { lhs, rhs, .. } => {
            expr_references_name(lhs, name) || expr_references_name(rhs, name)
        }
        ir::Expr::Unary { operand, .. } | ir::Expr::Convert { operand, .. } => {
            expr_references_name(operand, name)
        }
        ir::Expr::Call { target, args, .. } => {
            target
                .as_ref()
                .is_some_and(|target| expr_references_name(target, name))
                || args.iter().any(|arg| expr_references_name(arg, name))
        }
        ir::Expr::FieldAccess { target, .. } | ir::Expr::StringByteLength { target, .. } => {
            expr_references_name(target, name)
        }
        ir::Expr::RecordConstruct { fields, .. } => fields
            .iter()
            .any(|(_name, value)| expr_references_name(value, name)),
        ir::Expr::ConstructorCall { args, .. } => {
            args.iter().any(|arg| expr_references_name(arg, name))
        }
        ir::Expr::Index { target, index, .. } => {
            expr_references_name(target, name) || expr_references_name(index, name)
        }
        ir::Expr::Tuple { values, .. } => {
            values.iter().any(|value| expr_references_name(value, name))
        }
    }
}

fn replace_this_with_ref_in_stmts(stmts: &mut [ir::Stmt], var_name: &str) {
    for stmt in stmts {
        replace_this_with_ref_in_stmt(stmt, var_name);
    }
}

fn replace_this_with_ref_in_stmt(stmt: &mut ir::Stmt, var_name: &str) {
    match stmt {
        ir::Stmt::Return { value, .. } => {
            if let Some(expr) = value {
                replace_this_with_ref_in_expr(expr, var_name);
            }
        }
        ir::Stmt::VarDecl { init, .. } => {
            if let Some(expr) = init {
                replace_this_with_ref_in_expr(expr, var_name);
            }
        }
        ir::Stmt::Assign { value, .. } => replace_this_with_ref_in_expr(value, var_name),
        ir::Stmt::FieldAssign { target, value, .. } => {
            replace_this_with_ref_in_expr(target, var_name);
            replace_this_with_ref_in_expr(value, var_name);
        }
        ir::Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            replace_this_with_ref_in_expr(condition, var_name);
            replace_this_with_ref_in_stmts(then_branch, var_name);
            replace_this_with_ref_in_stmts(else_branch, var_name);
        }
        ir::Stmt::While {
            condition, body, ..
        } => {
            replace_this_with_ref_in_expr(condition, var_name);
            replace_this_with_ref_in_stmts(body, var_name);
        }
        ir::Stmt::For {
            init,
            condition,
            increment,
            body,
            ..
        } => {
            if let Some(init) = init {
                replace_this_with_ref_in_stmt(init, var_name);
            }
            if let Some(condition) = condition {
                replace_this_with_ref_in_expr(condition, var_name);
            }
            if let Some(increment) = increment {
                replace_this_with_ref_in_stmt(increment, var_name);
            }
            replace_this_with_ref_in_stmts(body, var_name);
        }
        ir::Stmt::ExprStmt { expr, .. } => replace_this_with_ref_in_expr(expr, var_name),
        ir::Stmt::Throw { value, .. } => replace_this_with_ref_in_expr(value, var_name),
        ir::Stmt::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            replace_this_with_ref_in_stmts(try_body, var_name);
            replace_this_with_ref_in_stmts(catch_body, var_name);
        }
        ir::Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            replace_this_with_ref_in_stmts(try_body, var_name);
            replace_this_with_ref_in_stmts(finally_body, var_name);
        }
        ir::Stmt::TupleAssign { targets, value, .. } => {
            for target in targets {
                replace_this_with_ref_in_expr(target, var_name);
            }
            replace_this_with_ref_in_expr(value, var_name);
        }
        ir::Stmt::Unsupported { .. } => {}
    }
}

fn replace_this_with_ref_in_expr(expr: &mut ir::Expr, var_name: &str) {
    match expr {
        ir::Expr::This { ty, origin } => {
            *expr = ir::Expr::Ref {
                name: var_name.to_owned(),
                ty: ty.clone(),
                origin: origin.clone(),
            };
        }
        ir::Expr::IntLiteral { .. }
        | ir::Expr::DoubleLiteral { .. }
        | ir::Expr::BoolLiteral { .. }
        | ir::Expr::StringLiteral { .. }
        | ir::Expr::Ref { .. }
        | ir::Expr::Unsupported { .. } => {}
        ir::Expr::Binary { lhs, rhs, .. } => {
            replace_this_with_ref_in_expr(lhs, var_name);
            replace_this_with_ref_in_expr(rhs, var_name);
        }
        ir::Expr::Unary { operand, .. } | ir::Expr::Convert { operand, .. } => {
            replace_this_with_ref_in_expr(operand, var_name);
        }
        ir::Expr::Call { target, args, .. } => {
            if let Some(target) = target {
                replace_this_with_ref_in_expr(target, var_name);
            }
            for arg in args {
                replace_this_with_ref_in_expr(arg, var_name);
            }
        }
        ir::Expr::FieldAccess { target, .. } | ir::Expr::StringByteLength { target, .. } => {
            replace_this_with_ref_in_expr(target, var_name);
        }
        ir::Expr::RecordConstruct { fields, .. } => {
            for (_name, value) in fields {
                replace_this_with_ref_in_expr(value, var_name);
            }
        }
        ir::Expr::ConstructorCall { args, .. } => {
            for arg in args {
                replace_this_with_ref_in_expr(arg, var_name);
            }
        }
        ir::Expr::Index { target, index, .. } => {
            replace_this_with_ref_in_expr(target, var_name);
            replace_this_with_ref_in_expr(index, var_name);
        }
        ir::Expr::Tuple { values, .. } => {
            for value in values {
                replace_this_with_ref_in_expr(value, var_name);
            }
        }
    }
}

/// The deterministic name a renamed overload gets — appends every
/// parameter's Dart type name, capitalized, to the original C++ name
/// (`formatarValor` + `[Int]` params → `formatarValorInt`). Computed the
/// same way regardless of which specific overload in the group is asking,
/// so two overloads with different parameter types can never collide.
/// Shares `lower::cpp::overload_type_suffix` with E08's monomorphization
/// naming (`lower::cpp::monomorphized_template_name`) — same suffix scheme,
/// same reason to have only one implementation of it.
fn dart_overload_name(base_name: &str, params: &[ir::Param]) -> String {
    let mut name = base_name.to_owned();
    for param in params {
        name.push_str(&lower::cpp::overload_type_suffix(&param.ty));
    }
    name
}

fn rename_calls_in_stmts(stmts: &mut [ir::Stmt], renames: &HashMap<String, String>) {
    for stmt in stmts {
        rename_calls_in_stmt(stmt, renames);
    }
}

fn rename_calls_in_stmt(stmt: &mut ir::Stmt, renames: &HashMap<String, String>) {
    match stmt {
        ir::Stmt::Return { value, .. } => {
            if let Some(expr) = value {
                rename_calls_in_expr(expr, renames);
            }
        }
        ir::Stmt::VarDecl { init, .. } => {
            if let Some(expr) = init {
                rename_calls_in_expr(expr, renames);
            }
        }
        ir::Stmt::Assign { value, .. } => rename_calls_in_expr(value, renames),
        ir::Stmt::FieldAssign { target, value, .. } => {
            rename_calls_in_expr(target, renames);
            rename_calls_in_expr(value, renames);
        }
        ir::Stmt::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            rename_calls_in_expr(condition, renames);
            rename_calls_in_stmts(then_branch, renames);
            rename_calls_in_stmts(else_branch, renames);
        }
        ir::Stmt::While {
            condition, body, ..
        } => {
            rename_calls_in_expr(condition, renames);
            rename_calls_in_stmts(body, renames);
        }
        ir::Stmt::For {
            init,
            condition,
            increment,
            body,
            ..
        } => {
            if let Some(init) = init {
                rename_calls_in_stmt(init, renames);
            }
            if let Some(condition) = condition {
                rename_calls_in_expr(condition, renames);
            }
            if let Some(increment) = increment {
                rename_calls_in_stmt(increment, renames);
            }
            rename_calls_in_stmts(body, renames);
        }
        ir::Stmt::ExprStmt { expr, .. } => rename_calls_in_expr(expr, renames),
        ir::Stmt::Throw { value, .. } => rename_calls_in_expr(value, renames),
        ir::Stmt::TryCatch {
            try_body,
            catch_body,
            ..
        } => {
            rename_calls_in_stmts(try_body, renames);
            rename_calls_in_stmts(catch_body, renames);
        }
        ir::Stmt::TryFinally {
            try_body,
            finally_body,
            ..
        } => {
            rename_calls_in_stmts(try_body, renames);
            rename_calls_in_stmts(finally_body, renames);
        }
        ir::Stmt::TupleAssign { targets, value, .. } => {
            for target in targets {
                rename_calls_in_expr(target, renames);
            }
            rename_calls_in_expr(value, renames);
        }
        ir::Stmt::Unsupported { .. } => {}
    }
}

fn rename_calls_in_expr(expr: &mut ir::Expr, renames: &HashMap<String, String>) {
    match expr {
        ir::Expr::IntLiteral { .. }
        | ir::Expr::DoubleLiteral { .. }
        | ir::Expr::BoolLiteral { .. }
        | ir::Expr::StringLiteral { .. }
        | ir::Expr::Ref { .. }
        | ir::Expr::This { .. }
        | ir::Expr::Unsupported { .. } => {}
        ir::Expr::Binary { lhs, rhs, .. } => {
            rename_calls_in_expr(lhs, renames);
            rename_calls_in_expr(rhs, renames);
        }
        ir::Expr::Unary { operand, .. } | ir::Expr::Convert { operand, .. } => {
            rename_calls_in_expr(operand, renames);
        }
        ir::Expr::Call {
            target,
            callee_usr,
            callee_name,
            args,
            ..
        } => {
            if let Some(target) = target {
                rename_calls_in_expr(target, renames);
            }
            if let Some(new_name) = renames.get(callee_usr) {
                *callee_name = new_name.clone();
            }
            for arg in args {
                rename_calls_in_expr(arg, renames);
            }
        }
        ir::Expr::FieldAccess { target, .. } | ir::Expr::StringByteLength { target, .. } => {
            rename_calls_in_expr(target, renames);
        }
        ir::Expr::RecordConstruct { fields, .. } => {
            for (_name, value) in fields {
                rename_calls_in_expr(value, renames);
            }
        }
        ir::Expr::ConstructorCall { args, .. } => {
            for arg in args {
                rename_calls_in_expr(arg, renames);
            }
        }
        ir::Expr::Index { target, index, .. } => {
            rename_calls_in_expr(target, renames);
            rename_calls_in_expr(index, renames);
        }
        ir::Expr::Tuple { values, .. } => {
            for value in values {
                rename_calls_in_expr(value, renames);
            }
        }
    }
}

fn parse_chunk(
    worker_index: usize,
    chunk: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> FunctionCatalogPartial {
    // Each worker thread needs its own load: see
    // `type_catalog::parse_chunk` for why the calling thread's
    // `load_libclang()` doesn't cover this one.
    type_catalog::load_libclang().expect(
        "libclang already loaded successfully on the calling thread; \
         per-thread load is not expected to fail",
    );

    let mut seen = HashMap::new();
    let mut declarations = Vec::new();
    let mut call_seen = HashSet::new();
    let mut calls = Vec::new();
    let mut ir_functions = Vec::new();
    let mut ir_function_is_prototype = Vec::new();
    let mut ir_records = Vec::new();
    let mut ir_record_seen = HashSet::new();
    let mut ir_member_seen = HashSet::new();
    let mut ir_seen = HashSet::new();
    let mut ir_enums = Vec::new();
    let mut ir_enum_seen = HashSet::new();

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
                ir_function_is_prototype: &mut ir_function_is_prototype,
                ir_records: &mut ir_records,
                ir_record_seen: &mut ir_record_seen,
                ir_member_seen: &mut ir_member_seen,
                ir_seen: &mut ir_seen,
                ir_enums: &mut ir_enums,
                ir_enum_seen: &mut ir_enum_seen,
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

    (
        declarations,
        calls,
        ir_functions,
        ir_function_is_prototype,
        ir_records,
        ir_enums,
    )
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

pub(crate) struct VisitorState<'a> {
    project_root: &'a Path,
    declarations: &'a mut Vec<FunctionDeclaration>,
    /// Declaration identity (`declaration_identity`) → index into
    /// `declarations`, not just a membership set — `push_declaration` needs
    /// the index to *upgrade* an already-pushed prototype-only entry
    /// in place when a real definition for the same usr shows up later in
    /// this same worker's traversal (a free function declared in a header,
    /// defined in a `.cpp` the same worker also parses), without ever
    /// losing the definition to a "first seen wins" rule.
    seen: &'a mut HashMap<String, usize>,
    calls: &'a mut Vec<CallEdge>,
    call_seen: &'a mut HashSet<(String, CallResolution, String, u32, u32)>,
    ir_functions: &'a mut Vec<ir::Function>,
    /// Parallel to `ir_functions` (same length, same order): `true` for an
    /// entry synthesized from a free-function prototype with no definition
    /// anywhere in this project (`docs/plans/lista-de-externos.md`),
    /// `false` for a real definition. Needed because an `ir::Function`
    /// value alone can't tell the two apart (an empty `body` isn't a
    /// reliable signal — a real function can legitimately have one) — the
    /// cross-worker merge in `finish_function_catalog` uses this to prefer
    /// a real definition over a prototype stand-in for the same usr,
    /// exactly like `seen` does for `declarations`.
    ir_function_is_prototype: &'a mut Vec<bool>,
    ir_records: &'a mut Vec<ir::Record>,
    /// Dedupes record *creation* within one worker's traversal — a class
    /// fully defined in a header is reparsed once per translation unit that
    /// includes it (include guards only prevent re-inclusion within a
    /// single TU), so without this, `visit_cursor`'s class-definition
    /// branch would push a fresh, empty `ir::Record` for every one of those
    /// re-parses, and every later `.iter_mut().find(|r| &r.usr ==
    /// owner_usr)` attach (method/constructor/destructor) would keep
    /// hitting the *first* one — multiplying its `methods` list once per
    /// re-parse instead of leaving the duplicates empty. Mirrors
    /// `ir_enum_seen` just below. The cross-worker case (the same class
    /// reached by TUs in two different workers' chunks) is handled
    /// separately, by the whole-record dedup in `finish_function_catalog`.
    ir_record_seen: &'a mut HashSet<String>,
    /// Dedupes method/constructor *attachment* within one worker's
    /// traversal, keyed by the member's own `usr` — the same re-parse that
    /// makes `ir_record_seen` necessary (a class fully defined in a header,
    /// reached from more than one translation unit in this worker's chunk)
    /// also re-visits every inline method's definition cursor once per
    /// re-parse. `ir_record_seen` alone only stops the record itself from
    /// being duplicated; without this, the one surviving record would still
    /// receive the same method/constructor appended once per translation
    /// unit that included the header.
    ir_member_seen: &'a mut HashSet<String>,
    /// E08: dedupes monomorphized-function synthesis within one worker's
    /// traversal (`CallVisitorState.ir_functions`/`ir_seen` borrow these
    /// same two) — the cross-worker/explicit-specialization duplicate case
    /// is handled separately, by the final merge in
    /// `extract_function_catalog_cancellable`.
    ir_seen: &'a mut HashSet<String>,
    ir_enums: &'a mut Vec<ir::Enum>,
    ir_enum_seen: &'a mut HashSet<String>,
}

impl<'a> VisitorState<'a> {
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        project_root: &'a Path,
        declarations: &'a mut Vec<FunctionDeclaration>,
        seen: &'a mut HashMap<String, usize>,
        calls: &'a mut Vec<CallEdge>,
        call_seen: &'a mut HashSet<(String, CallResolution, String, u32, u32)>,
        ir_functions: &'a mut Vec<ir::Function>,
        ir_function_is_prototype: &'a mut Vec<bool>,
        ir_records: &'a mut Vec<ir::Record>,
        ir_record_seen: &'a mut HashSet<String>,
        ir_member_seen: &'a mut HashSet<String>,
        ir_seen: &'a mut HashSet<String>,
        ir_enums: &'a mut Vec<ir::Enum>,
        ir_enum_seen: &'a mut HashSet<String>,
    ) -> Self {
        Self {
            project_root,
            declarations,
            seen,
            calls,
            call_seen,
            ir_functions,
            ir_function_is_prototype,
            ir_records,
            ir_record_seen,
            ir_member_seen,
            ir_seen,
            ir_enums,
            ir_enum_seen,
        }
    }
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

/// Pushes `declaration`, or — if this worker already pushed a
/// prototype-only entry (`has_definition: false`) for the same identity —
/// upgrades it in place to the real definition now found. Never downgrades
/// an existing definition, and never duplicates: see `VisitorState.seen`'s
/// doc comment for why a plain membership set isn't enough here.
fn push_declaration(state: &mut VisitorState<'_>, declaration: FunctionDeclaration) {
    let identity = declaration_identity(&declaration);
    match state.seen.get(&identity) {
        None => {
            state.seen.insert(identity, state.declarations.len());
            state.declarations.push(declaration);
        }
        Some(&index) => {
            if !state.declarations[index].has_definition && declaration.has_definition {
                state.declarations[index] = declaration;
            }
        }
    }
}

unsafe fn visit_translation_unit(
    index: clang_sys::CXIndex,
    unit: &CompilationUnit,
    state: &mut VisitorState<'_>,
) {
    unsafe {
        let Some(translation_unit) = parse_translation_unit(index, unit) else {
            return;
        };
        visit_parsed_translation_unit(translation_unit, state);
        clang_sys::clang_disposeTranslationUnit(translation_unit);
    }
}

/// The flags this module needs — no `CXTranslationUnit_SkipFunctionBodies`
/// (see module docs): the call graph lives inside bodies.
/// `DetailedPreprocessingRecord` is kept so function-like macros are still
/// visited. Shared as-is with `pointer_catalog::visit_parsed_translation_unit`
/// by `extraction::extract_project_catalogs_cancellable`, which parses each
/// translation unit once per body-visibility requirement rather than once
/// per catalog; `pointer_catalog`'s own standalone parse uses `None` (no
/// flags), a subset this superset is safe for (see that module's own
/// `visit_parsed_translation_unit` doc comment).
pub(crate) const PARSE_FLAGS: clang_sys::CXTranslationUnit_Flags =
    clang_sys::CXTranslationUnit_DetailedPreprocessingRecord;

/// Parses `unit` with `PARSE_FLAGS`, returning `None` on failure (mirrors
/// `type_catalog::parse_translation_unit`).
pub(crate) unsafe fn parse_translation_unit(
    index: clang_sys::CXIndex,
    unit: &CompilationUnit,
) -> Option<clang_sys::CXTranslationUnit> {
    let file = CString::new(unit.file.as_str()).ok()?;

    let args = type_catalog::build_clang_args(unit);
    let arg_cstrings: Vec<CString> = args
        .iter()
        .filter_map(|arg| CString::new(arg.as_str()).ok())
        .collect();
    let arg_ptrs: Vec<*const std::os::raw::c_char> =
        arg_cstrings.iter().map(|arg| arg.as_ptr()).collect();

    unsafe {
        let translation_unit = clang_sys::clang_parseTranslationUnit(
            index,
            file.as_ptr(),
            arg_ptrs.as_ptr(),
            arg_ptrs.len() as c_int,
            std::ptr::null_mut(),
            0,
            PARSE_FLAGS,
        );

        if translation_unit.is_null() {
            None
        } else {
            Some(translation_unit)
        }
    }
}

/// Walks an already-parsed translation unit (mirrors
/// `type_catalog::visit_parsed_translation_unit`) — lets a caller sharing
/// one parse across catalogs call this without also disposing it.
pub(crate) unsafe fn visit_parsed_translation_unit(
    translation_unit: clang_sys::CXTranslationUnit,
    state: &mut VisitorState<'_>,
) {
    unsafe {
        let root_cursor = clang_sys::clang_getTranslationUnitCursor(translation_unit);
        clang_sys::clang_visitChildren(
            root_cursor,
            visit_cursor,
            state as *mut VisitorState<'_> as *mut c_void,
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
    // (not returned early, and regardless of `ir_record_seen` below) — the
    // generic walk still needs to descend into the record's own body to
    // reach any inline method definitions the existing
    // `function_declaration_kind_for` handling already covers, so a later
    // method/constructor/destructor cursor can still find this record (via
    // `ir_record_seen`'s first push, possibly from an earlier translation
    // unit in this same worker's chunk) to attach itself to. `ir_record_seen`
    // guards only the `push` itself: without it, a class fully defined in a
    // header gets pushed once per translation unit that includes it, and
    // every later attach keeps hitting the *first* (`.find`'s first match),
    // multiplying its `methods` once per re-parse instead of leaving the
    // duplicates empty.
    if (kind == clang_sys::CXCursor_StructDecl || kind == clang_sys::CXCursor_ClassDecl)
        && unsafe { clang_sys::clang_isCursorDefinition(cursor) } != 0
        && let Some(record) = lower::cpp::lower_record(cursor, state.project_root)
        && state.ir_record_seen.insert(record.usr.clone())
    {
        state.ir_records.push(record);
    }

    // Caso 4 of `docs/plans/verovio-6.2-pointer-types.md`: an `enum`/`enum
    // class` definition, lowered the same way a `struct`/`class` is just
    // above. `ir_enum_seen` dedupes within this one worker's traversal
    // (the same header commonly reparsed by every TU that includes it) —
    // the cross-worker case is handled by `finish_function_catalog`'s own
    // merge, same as `ir_records`.
    if kind == clang_sys::CXCursor_EnumDecl
        && unsafe { clang_sys::clang_isCursorDefinition(cursor) } != 0
        && let Some(enum_decl) = lower::cpp::lower_enum(cursor, state.project_root)
        && state.ir_enum_seen.insert(enum_decl.usr.clone())
    {
        state.ir_enums.push(enum_decl);
    }

    // A pure virtual method (`= 0`) never has a body, so
    // `clang_isCursorDefinition` is never true for it — its declaration is
    // still the one and only cursor for that virtual slot, and skipping it
    // would make `mapping::options_for`'s interface-vs-mixin rule (US-7,
    // `docs/mapping-solver-cases.md` case B03) blind to every pure
    // interface method. Unlike an ordinary in-header prototype of a
    // method/constructor/destructor (still correctly skipped here, to avoid
    // double-counting it alongside its out-of-line definition), a pure
    // virtual method can never gain a separate defining cursor elsewhere,
    // so this can't introduce a duplicate.
    let is_pure_virtual_declaration =
        unsafe { clang_sys::clang_CXXMethod_isPureVirtual(cursor) } != 0;
    let is_definition = unsafe { clang_sys::clang_isCursorDefinition(cursor) } != 0;
    let declaration_kind = function_declaration_kind_for(kind);

    // Unlike a method/constructor/destructor prototype (skipped, per the
    // comment above), a **free function** prototype is deliberately
    // cataloged even without a definition anywhere in this project —
    // `docs/plans/lista-de-externos.md`'s auto-detection needs a real,
    // structured signature (not just a name) for a symbol this project
    // declares but never defines, so a mock can be emitted with the right
    // return type. Scoped to `!is_system_header`: every libc/toolchain
    // prototype reachable through an `#include` would otherwise flood the
    // catalog with thousands of entries nothing ever calls.
    let is_uncatalogued_free_prototype = declaration_kind
        == Some(FunctionDeclarationKind::FreeFunction)
        && !is_definition
        && unsafe {
            clang_sys::clang_Location_isInSystemHeader(clang_sys::clang_getCursorLocation(cursor))
        } == 0;

    if let Some(declaration_kind) = declaration_kind
        && (is_definition || is_pure_virtual_declaration || is_uncatalogued_free_prototype)
        && let Some(mut declaration) =
            describe_function(cursor, declaration_kind, state.project_root)
    {
        declaration.has_definition = is_definition;
        let caller_usr = declaration.usr.clone();
        let declared_kind = declaration.kind;
        let owning_class_usr = declaration.owning_class_usr.clone();
        push_declaration(state, declaration);

        // IR lowering (E01–E03 scope: free functions only, see
        // `lower::cpp`'s docs) walks this same already-parsed cursor with
        // its own `clang_visitChildren` call, the same way the call-graph
        // visitor just below does — not a second `libclang` parse.
        if declared_kind == FunctionDeclarationKind::FreeFunction
            && !caller_usr.is_empty()
            && let Some(mut function_ir) =
                lower::cpp::lower_function(cursor, &caller_usr, state.project_root)
        {
            if !is_definition {
                // A prototype-only cursor: `lower_function` already
                // tolerates the missing `CompoundStmt` (empty `body`), but
                // an empty body is silently wrong Dart for a non-`void`
                // return type (no `return` statement) if this usr is ever
                // *not* in the effective external set (`externals.rs`) —
                // e.g. the user manually excluded an auto-detected
                // candidate. A single `Unsupported` statement makes the
                // ordinary (non-mock) emission path bail out honestly
                // instead, the same "silêncio é proibido" idiom every other
                // unrepresentable construct already uses.
                function_ir.body = vec![ir::Stmt::Unsupported {
                    reason: "declared but never defined in any compilation \
                             unit of this project"
                        .to_owned(),
                    origin: function_ir.origin.clone(),
                }];
                state.ir_functions.push(function_ir);
                state.ir_function_is_prototype.push(true);
            } else {
                // E08: this cursor is a full explicit specialization
                // (`template<> std::string dobro<std::string>(...)`) when
                // its own `clang_getSpecializedCursorTemplate` is non-null —
                // a real, user-written `FreeFunction` declaration by every
                // other measure, which is exactly why
                // `function_declaration_kind_for` never needed a separate
                // case for it. Renamed here via the same
                // `monomorphized_template_name` every call site referencing
                // it independently recomputes (`lower_call_expr`), so a
                // specialization's own declaration and its call sites can
                // never end up naming it differently.
                let specialized_template =
                    unsafe { clang_sys::clang_getSpecializedCursorTemplate(cursor) };
                if unsafe { clang_sys::clang_Cursor_isNull(specialized_template) } == 0 {
                    let base_name = unsafe {
                        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(
                            specialized_template,
                        ))
                    };
                    function_ir.name = lower::cpp::monomorphized_template_name(&base_name, cursor);
                }
                state.ir_functions.push(function_ir);
                state.ir_function_is_prototype.push(false);
            }
        }

        // E04: a method's or constructor's *definition* cursor — inline
        // (a child of the class cursor, reached via the `Recurse` below) or
        // out-of-line (a separate top-level cursor elsewhere in the same
        // translation unit) — is lowered here the same way a free function
        // is, then attached to the record its `owning_class_usr` names.
        // That record is guaranteed to already be in `state.ir_records` by
        // this point: C++ requires a complete class definition before any
        // out-of-line member of it can be defined, and an inline member is
        // only reached by recursing into the class cursor's children, which
        // happens strictly after this same function already pushed the
        // record for that cursor (see the `StructDecl`/`ClassDecl` branch
        // above). If the record can't be found, the definition is silently
        // skipped rather than lowered into nothing — should only happen for
        // a class outside `project_root` (filtered out of the catalog
        // entirely), never for one of the project's own.
        if let Some(owner_usr) = &owning_class_usr {
            if declared_kind == FunctionDeclarationKind::Method
                && state.ir_member_seen.insert(caller_usr.clone())
                && let Some(method_ir) =
                    lower::cpp::lower_method(cursor, &caller_usr, state.project_root)
                && let Some(record) = state.ir_records.iter_mut().find(|r| &r.usr == owner_usr)
            {
                record.methods.push(method_ir);
            }
            if declared_kind == FunctionDeclarationKind::Constructor
                && state.ir_member_seen.insert(caller_usr.clone())
                && let Some(constructor_ir) =
                    lower::cpp::lower_constructor(cursor, state.project_root)
                && let Some(record) = state.ir_records.iter_mut().find(|r| &r.usr == owner_usr)
            {
                record.constructors.push(constructor_ir);
            }
            // E12: unlike a method/constructor, a destructor is never
            // stored as a callable member — only its body (if it does real
            // teardown work) matters, for `apply_raii_scope_guards` to
            // splice in later.
            if declared_kind == FunctionDeclarationKind::Destructor
                && let Some(destructor_body) =
                    lower::cpp::lower_destructor(cursor, state.project_root)
                && let Some(record) = state.ir_records.iter_mut().find(|r| &r.usr == owner_usr)
            {
                record.destructor = Some(destructor_body);
            }
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
                ir_functions: &mut *state.ir_functions,
                ir_function_is_prototype: &mut *state.ir_function_is_prototype,
                ir_seen: &mut *state.ir_seen,
                pending_overload: None,
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
    /// E08: monomorphized functions synthesized from a call site's resolved
    /// *implicit* template instantiation (never independently visited at
    /// top level, unlike a full explicit specialization — see
    /// `record_call`). Shares its accumulator with the enclosing
    /// `VisitorState.ir_functions` so a duplicate (an explicit
    /// specialization *and* a call to it both reaching this usr) collapses
    /// the same way any other cross-source duplicate already does, in
    /// `extract_function_catalog_cancellable`'s final merge.
    ir_functions: &'a mut Vec<ir::Function>,
    /// Parallel to `ir_functions` (same length, same order) — see
    /// `VisitorState.ir_function_is_prototype`'s doc comment. Always pushed
    /// `false` here: a monomorphized function is synthesized from a real
    /// template instantiation, never a prototype-only stand-in.
    ir_function_is_prototype: &'a mut Vec<bool>,
    ir_seen: &'a mut HashSet<String>,
    /// Set by `visit_call_site` when the immediately preceding sibling
    /// cursor was an unresolved `CXCursor_OverloadedDeclRef` (case B04) —
    /// consumed by the very next callback, whichever cursor that turns out
    /// to be, to disambiguate which overload it actually calls.
    pending_overload: Option<PendingOverload>,
}

extern "C" fn visit_call_site(
    cursor: clang_sys::CXCursor,
    _parent: clang_sys::CXCursor,
    data: clang_sys::CXClientData,
) -> clang_sys::CXChildVisitResult {
    let state = unsafe { &mut *(data as *mut CallVisitorState<'_>) };
    let kind = unsafe { clang_sys::clang_getCursorKind(cursor) };

    // B04: a call to an overloaded *free* function used as an operand of
    // another call (here, `std::string`'s `operator+`, in
    // `formatar(contagem) + " / " + formatar(media)`) doesn't reach this
    // visitor as its own `CXCursor_CallExpr` at all — `libclang`'s cursor
    // API exposes only the bare, still-unresolved callee reference
    // (`CXCursor_OverloadedDeclRef`) as a direct child of the surrounding
    // expression, with `clang_getCursorReferenced` on it resolving to
    // nothing useful (itself, empty `usr`). The actual selected overload
    // only exists in full Clang's `Sema`, not in this simplified cursor
    // view — so it has to be re-derived here from the one thing `libclang`
    // does expose per candidate: each candidate's own parameter types
    // (`try_record_overloaded_call`, using the very next sibling cursor —
    // reliably the call's own single argument, confirmed empirically for
    // this pattern — as the disambiguating evidence).
    if kind == clang_sys::CXCursor_CallExpr {
        record_call(cursor, state);
    } else if kind == clang_sys::CXCursor_OverloadedDeclRef {
        state.pending_overload = pending_overload_from(cursor, state.project_root);
        return clang_sys::CXChildVisit_Recurse;
    } else if let Some(pending) = state.pending_overload.take() {
        record_overloaded_call(cursor, pending, state);
    }

    clang_sys::CXChildVisit_Recurse
}

/// What `try_record_overloaded_call` needs about an `OverloadedDeclRef`
/// cursor, captured immediately (rather than re-derived later) since the
/// cursor itself is only valid for the duration of this callback.
struct PendingOverload {
    file: String,
    line: u32,
    column: u32,
    /// One entry per candidate found by name lookup at this call site —
    /// its own `usr` and, when it has exactly one parameter (the only
    /// shape `try_record_overloaded_call` currently disambiguates), that
    /// parameter's type spelling to compare against the next sibling
    /// cursor's own type.
    candidates: Vec<(String, Option<String>)>,
}

fn pending_overload_from(
    cursor: clang_sys::CXCursor,
    project_root: &Path,
) -> Option<PendingOverload> {
    let (file, line, column) = type_catalog::cursor_site(cursor, project_root)?;

    let num_overloaded = unsafe { clang_sys::clang_getNumOverloadedDecls(cursor) };
    if num_overloaded == 0 {
        return None;
    }

    let mut candidates = Vec::with_capacity(num_overloaded as usize);
    for index in 0..num_overloaded {
        let candidate = unsafe { clang_sys::clang_getOverloadedDecl(cursor, index) };
        let candidate_usr =
            unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(candidate)) };
        if candidate_usr.is_empty() {
            continue;
        }

        let function_type = unsafe { clang_sys::clang_getCursorType(candidate) };
        let arg_count = unsafe { clang_sys::clang_getNumArgTypes(function_type) };
        let single_param_type = if arg_count == 1 {
            let arg_type = unsafe { clang_sys::clang_getArgType(function_type, 0) };
            Some(unsafe {
                type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(arg_type))
            })
        } else {
            None
        };
        candidates.push((candidate_usr, single_param_type));
    }

    Some(PendingOverload {
        file,
        line,
        column,
        candidates,
    })
}

/// Resolves a pending overloaded call (case B04) using `argument_cursor` —
/// the cursor immediately following the `OverloadedDeclRef` in traversal
/// order, which for the one shape this handles (a single-argument call to
/// an overloaded free function) is reliably that argument's own expression.
/// Disambiguates by comparing `argument_cursor`'s own type spelling against
/// each single-parameter candidate's parameter type spelling: resolves only
/// when *exactly one* candidate matches, and otherwise records the call as
/// unresolved rather than guessing — the same soundness rule every other
/// heuristic in this codebase follows (never a wrong-but-confident answer).
fn record_overloaded_call(
    argument_cursor: clang_sys::CXCursor,
    pending: PendingOverload,
    state: &mut CallVisitorState<'_>,
) {
    let argument_type = unsafe { clang_sys::clang_getCursorType(argument_cursor) };
    let argument_spelling = unsafe {
        type_catalog::cxstring_to_string(clang_sys::clang_getTypeSpelling(argument_type))
    };

    let mut matches = pending
        .candidates
        .iter()
        .filter(|(_, param_type)| param_type.as_deref() == Some(argument_spelling.as_str()));
    let resolution = match (matches.next(), matches.next()) {
        (Some((usr, _)), None) => CallResolution::Resolved {
            callee_usr: usr.clone(),
            is_dynamic_dispatch: false,
        },
        _ => CallResolution::Unresolved {
            reason: format!(
                "overloaded call with {} candidates could not be disambiguated from the \
                 argument type alone",
                pending.candidates.len()
            ),
        },
    };

    let edge = CallEdge {
        caller_usr: state.caller_usr.to_owned(),
        resolution,
        file: pending.file,
        line: pending.line,
        column: pending.column,
    };
    if state.call_seen.insert(call_identity(&edge)) {
        state.calls.push(edge);
    }
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
            let is_template_instantiation =
                unsafe { clang_sys::clang_Cursor_isNull(template) } == 0;

            // E08: `lower::cpp::lower_call_expr` names this call's callee
            // via `monomorphized_template_name(base_name, referenced)` —
            // that name is only backed by a real Dart declaration if one
            // exists. A full explicit specialization already gets one via
            // the ordinary top-level `FreeFunction` path above (renamed the
            // same way); an *implicit* instantiation never independently
            // reaches that path at all (see
            // `FunctionDeclarationKind::FunctionTemplate`'s docs), so it's
            // synthesized here instead, from `referenced` itself — its
            // instantiation already carries concrete (non-template-
            // dependent) parameter/return types, confirmed empirically, so
            // lowering it directly (rather than the abstract primary
            // template) produces a correct, already-substituted body.
            // Scoped to `FunctionDecl` (a free function/its instantiation)
            // to match `lower_call_expr`'s own scope — a template *method*
            // is E08+E04 territory no fixture forces yet.
            if is_template_instantiation && referenced_kind == clang_sys::CXCursor_FunctionDecl {
                let instantiation_usr = unsafe {
                    type_catalog::cxstring_to_string(clang_sys::clang_getCursorUSR(referenced))
                };
                if !instantiation_usr.is_empty()
                    && state.ir_seen.insert(instantiation_usr.clone())
                    && let Some(mut function_ir) = lower::cpp::lower_function(
                        referenced,
                        &instantiation_usr,
                        state.project_root,
                    )
                {
                    let base_name = unsafe {
                        type_catalog::cxstring_to_string(clang_sys::clang_getCursorSpelling(
                            template,
                        ))
                    };
                    function_ir.name =
                        lower::cpp::monomorphized_template_name(&base_name, referenced);
                    state.ir_functions.push(function_ir);
                    state.ir_function_is_prototype.push(false);
                }
            }

            let usr_source = if !is_template_instantiation {
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
        is_static: false,
        is_virtual: false,
        is_pure_virtual: false,
        is_defaulted: false,
        overridden_usrs: Vec::new(),
        has_definition: true,
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

    let is_static = kind == FunctionDeclarationKind::Method
        && unsafe { clang_sys::clang_CXXMethod_isStatic(cursor) } != 0;
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
        is_static,
        is_virtual,
        is_pure_virtual,
        is_defaulted,
        overridden_usrs,
        // Overwritten by the caller (`visit_cursor` is the only call site),
        // which knows `clang_isCursorDefinition` for this exact cursor —
        // this function has no opinion on it either way.
        has_definition: true,
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
