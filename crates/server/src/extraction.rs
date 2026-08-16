//! Drives `type_catalog`, `source_catalog`, `function_catalog` and
//! `pointer_catalog` together for `project_service::create_project`,
//! parsing each compilation unit **twice** instead of **four times**.
//!
//! Each of those four modules used to independently call
//! `clang_parseTranslationUnit` per compilation unit — a `libclang` parse is
//! the dominant cost of ingestion (see the modules' own doc comments), and
//! the same project header is commonly reparsed by every translation unit
//! that includes it, so paying that cost four times over was the single
//! largest identified opportunity in the ingestion performance review
//! ("Oportunidades priorizadas" item #1).
//!
//! The four passes don't all need the same parse, though:
//! `type_catalog`/`source_catalog` parse with
//! `CXTranslationUnit_SkipFunctionBodies` (they never look inside a function
//! body), while `function_catalog`/`pointer_catalog` need full bodies (the
//! call graph and local pointer declarations both live there) — two
//! genuinely different parses. So each translation unit is parsed once per
//! *group*, and both catalogs in that group are visited against the one
//! shared `CXTranslationUnit`, instead of a fifth "unify everything into one
//! visitor" pass that would have meant rewriting all four modules' cursor
//! visitors (correctness-critical, extensively tested logic) into one. Each
//! module's own visitor is untouched here — only its parse step is shared,
//! via the `parse_translation_unit`/`visit_parsed_translation_unit` split
//! each of the four modules exposes for this purpose. Each module's own
//! standalone `extract_*_cancellable` entry point is also untouched and
//! keeps doing its own independent parse — this module is strictly
//! additive, used only by `project_service::create_project`.

use std::collections::{BTreeSet, HashSet};
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::function_catalog::{self, FunctionCatalog, FunctionCatalogPartial};
use crate::ingest::CompilationUnit;
use crate::pointer_catalog::{self, PointerDeclaration};
use crate::progress::{Cancellation, ExtractionProgress};
use crate::source_catalog::{self, SourceFile};
use crate::type_catalog::{self, TypeCatalog, TypeDeclaration, TypeDependency, TypeUsage};

/// Every catalog `project_service::create_project` needs from `libclang`,
/// produced together by [`extract_project_catalogs_cancellable`].
pub struct Extracted {
    pub type_catalog: TypeCatalog,
    pub source_files: Vec<SourceFile>,
    pub function_catalog: FunctionCatalog,
    pub pointer_catalog: Vec<PointerDeclaration>,
}

#[derive(Debug)]
pub enum ExtractionError {
    LibclangUnavailable(String),
    /// Mirrors `TypeCatalogError::Cancelled` and its three siblings (US-4
    /// criterion 7): extraction stopped early because `Cancellation::cancel`
    /// was called.
    Cancelled,
}

impl fmt::Display for ExtractionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibclangUnavailable(message) => {
                write!(formatter, "libclang is unavailable: {message}")
            }
            Self::Cancelled => write!(formatter, "project extraction was cancelled"),
        }
    }
}

impl std::error::Error for ExtractionError {}

/// Progress trackers for each of the four catalogs, mirroring
/// `project_service::CreationProgress`'s four fields exactly (this module
/// deliberately doesn't depend on `project_service` — the dependency runs
/// the other way — so it takes them individually rather than that struct).
/// All four still advance together in practice, one `mark_one_done` per
/// compilation unit each, since the two parses now happen in the same loop
/// iteration; kept separate so `jobs.rs`/the HTTP poller's existing
/// four-progress-bar contract with the Flutter client needs no changes.
pub fn extract_project_catalogs_cancellable(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    type_progress: Option<&ExtractionProgress>,
    source_progress: Option<&ExtractionProgress>,
    function_progress: Option<&ExtractionProgress>,
    pointer_progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> Result<Extracted, ExtractionError> {
    type_catalog::load_libclang().map_err(ExtractionError::LibclangUnavailable)?;

    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let total = compilation_units.len();
    for progress in [
        type_progress,
        source_progress,
        function_progress,
        pointer_progress,
    ]
    .into_iter()
    .flatten()
    {
        progress.set_total(total);
    }
    log_extraction(format_args!(
        "extract_project_catalogs: start, {total} compilation units"
    ));
    let extraction_started = Instant::now();

    let partials = if compilation_units.is_empty() {
        Vec::new()
    } else {
        let worker_count = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
            .min(total);
        // A shared cursor claimed one compilation unit at a time, instead of
        // a static `chunks(chunk_size)` split: translation units vary
        // wildly in parse cost (a several-thousand-line file next to a
        // handful of one-liners is common in real C++), so a fixed
        // up-front split can leave one worker still grinding through a
        // large unit while the others, having exhausted their smaller
        // chunks, sit idle. Every worker instead pulls the next unclaimed
        // index until none are left, so a slow unit only stalls the worker
        // that drew it, not the whole batch.
        let next_index = AtomicUsize::new(0);

        std::thread::scope(|scope| {
            (0..worker_count)
                .map(|worker_index| {
                    let project_root = &project_root;
                    let next_index = &next_index;
                    scope.spawn(move || {
                        parse_units(
                            worker_index,
                            compilation_units,
                            next_index,
                            project_root,
                            type_progress,
                            source_progress,
                            function_progress,
                            pointer_progress,
                            cancellation,
                        )
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|handle| handle.join().expect("extraction worker thread panicked"))
                .collect::<Vec<_>>()
        })
    };

    if cancellation.is_some_and(Cancellation::is_cancelled) {
        log_extraction(format_args!(
            "extract_project_catalogs: cancelled after {:.2}s",
            extraction_started.elapsed().as_secs_f64()
        ));
        return Err(ExtractionError::Cancelled);
    }

    let mut type_partials = Vec::with_capacity(partials.len());
    let mut translation_units = BTreeSet::new();
    let mut headers = BTreeSet::new();
    let mut function_partials = Vec::with_capacity(partials.len());
    let mut pointer_partials = Vec::with_capacity(partials.len());

    for partial in partials {
        type_partials.push(partial.type_catalog);
        translation_units.extend(partial.translation_units);
        headers.extend(partial.headers);
        function_partials.push(partial.function_catalog);
        pointer_partials.push(partial.pointer_catalog);
    }

    let extracted = Extracted {
        type_catalog: type_catalog::finish_type_catalog(type_partials),
        source_files: source_catalog::finish_source_files(translation_units, headers),
        function_catalog: function_catalog::finish_function_catalog(function_partials),
        pointer_catalog: pointer_catalog::finish_pointer_catalog(pointer_partials),
    };

    log_extraction(format_args!(
        "extract_project_catalogs: done in {:.2}s, {} types, {} source files, {} functions, {} pointers",
        extraction_started.elapsed().as_secs_f64(),
        extracted.type_catalog.declarations.len(),
        extracted.source_files.len(),
        extracted.function_catalog.declarations.len(),
        extracted.pointer_catalog.len()
    ));

    Ok(extracted)
}

type TypeCatalogPartial = (Vec<TypeDeclaration>, Vec<TypeDependency>, Vec<TypeUsage>);

struct WorkerPartials {
    type_catalog: TypeCatalogPartial,
    translation_units: BTreeSet<String>,
    headers: BTreeSet<String>,
    function_catalog: FunctionCatalogPartial,
    pointer_catalog: Vec<PointerDeclaration>,
}

/// Parses compilation units claimed one at a time from `next_index` (a
/// cursor shared across every worker — see the dynamic-scheduling comment
/// at the call site) with a `CXIndex` private to this worker, parsing each
/// unit twice — once per body-visibility requirement — and visiting both of
/// that parse's catalogs against the shared result, instead of four
/// independent parses.
#[allow(clippy::too_many_arguments)]
fn parse_units(
    worker_index: usize,
    units: &[CompilationUnit],
    next_index: &AtomicUsize,
    project_root: &Path,
    type_progress: Option<&ExtractionProgress>,
    source_progress: Option<&ExtractionProgress>,
    function_progress: Option<&ExtractionProgress>,
    pointer_progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> WorkerPartials {
    // Each worker thread needs its own load: see `type_catalog::parse_chunk`
    // for why the calling thread's `load_libclang()` doesn't cover this one.
    type_catalog::load_libclang().expect(
        "libclang already loaded successfully on the calling thread; \
         per-thread load is not expected to fail",
    );

    let mut type_seen = HashSet::new();
    let mut type_declarations = Vec::new();
    let mut type_dependency_seen = HashSet::new();
    let mut type_dependencies = Vec::new();
    let mut type_usage_seen = HashSet::new();
    let mut type_usages = Vec::new();
    let mut translation_units = BTreeSet::new();
    let mut headers = BTreeSet::new();

    let mut function_seen = HashSet::new();
    let mut function_declarations = Vec::new();
    let mut call_seen = HashSet::new();
    let mut calls = Vec::new();
    let mut ir_functions = Vec::new();
    let mut ir_records = Vec::new();
    let mut ir_seen = HashSet::new();
    let mut pointer_declarations = Vec::new();

    unsafe {
        let index = clang_sys::clang_createIndex(0, 0);

        loop {
            if cancellation.is_some_and(Cancellation::is_cancelled) {
                log_extraction(format_args!(
                    "worker {worker_index}: stopping early, cancellation requested"
                ));
                break;
            }

            let claimed = next_index.fetch_add(1, Ordering::Relaxed);
            let Some(unit) = units.get(claimed) else {
                break;
            };

            log_extraction(format_args!(
                "parsing (worker {worker_index}): {}",
                unit.file
            ));
            let unit_started = Instant::now();

            // Group A: `type_catalog` + `source_catalog`, one
            // `SkipFunctionBodies` parse shared between them.
            if let Some(canonical) = source_catalog::canonicalize_within(&unit.file, project_root) {
                translation_units.insert(canonical);
            }
            if let Some(translation_unit) = type_catalog::parse_translation_unit(index, unit) {
                let mut type_state = type_catalog::VisitorState::new(
                    project_root,
                    &mut type_declarations,
                    &mut type_seen,
                    &mut type_dependencies,
                    &mut type_dependency_seen,
                    &mut type_usages,
                    &mut type_usage_seen,
                );
                type_catalog::visit_parsed_translation_unit(translation_unit, &mut type_state);
                source_catalog::collect_inclusions_from_parsed(
                    translation_unit,
                    project_root,
                    &mut headers,
                );
                clang_sys::clang_disposeTranslationUnit(translation_unit);
            }
            if let Some(progress) = type_progress {
                progress.mark_one_done();
            }
            if let Some(progress) = source_progress {
                progress.mark_one_done();
            }

            // Group B: `function_catalog` + `pointer_catalog`, one
            // full-body parse shared between them.
            if let Some(translation_unit) = function_catalog::parse_translation_unit(index, unit) {
                let mut function_state = function_catalog::VisitorState::new(
                    project_root,
                    &mut function_declarations,
                    &mut function_seen,
                    &mut calls,
                    &mut call_seen,
                    &mut ir_functions,
                    &mut ir_records,
                    &mut ir_seen,
                );
                function_catalog::visit_parsed_translation_unit(
                    translation_unit,
                    &mut function_state,
                );
                pointer_catalog::visit_parsed_translation_unit(
                    translation_unit,
                    project_root,
                    &mut pointer_declarations,
                );
                clang_sys::clang_disposeTranslationUnit(translation_unit);
            }
            if let Some(progress) = function_progress {
                progress.mark_one_done();
            }
            if let Some(progress) = pointer_progress {
                progress.mark_one_done();
            }

            log_extraction(format_args!(
                "parsed in {:.2}s (worker {worker_index}): {}",
                unit_started.elapsed().as_secs_f64(),
                unit.file
            ));
        }

        clang_sys::clang_disposeIndex(index);
    }

    WorkerPartials {
        type_catalog: (type_declarations, type_dependencies, type_usages),
        translation_units,
        headers,
        function_catalog: (function_declarations, calls, ir_functions, ir_records),
        pointer_catalog: pointer_declarations,
    }
}

fn log_extraction(args: fmt::Arguments<'_>) {
    eprintln!("[syntax-bridge][extraction][{}] {args}", timestamp_millis());
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
