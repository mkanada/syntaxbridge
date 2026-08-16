//! Discovers every source file that belongs to a project: each compilation
//! unit's own translation-unit file, plus every project-local header it
//! `#include`s (transitively), using `libclang`'s inclusion-stack API.
//!
//! Like `type_catalog`, `libclang` is loaded dynamically at runtime and is
//! only ever expected to be found inside the toolchain environment the
//! server actually runs in (see `type_catalog`'s module docs).

use std::collections::BTreeSet;
use std::ffi::CString;
use std::fmt;
use std::os::raw::{c_int, c_uint, c_void};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ingest::CompilationUnit;
use crate::progress::{Cancellation, ExtractionProgress};
use crate::type_catalog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFileKind {
    TranslationUnit,
    Header,
}

impl SourceFileKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TranslationUnit => "translation_unit",
            Self::Header => "header",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "translation_unit" => Some(Self::TranslationUnit),
            "header" => Some(Self::Header),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct SourceFile {
    pub path: String,
    pub kind: SourceFileKind,
}

#[derive(Debug)]
pub enum SourceCatalogError {
    LibclangUnavailable(String),
    /// Mirrors `TypeCatalogError::Cancelled` (US-4 criterion 7): extraction
    /// stopped early because `Cancellation::cancel` was called.
    Cancelled,
}

impl fmt::Display for SourceCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibclangUnavailable(message) => {
                write!(formatter, "libclang is unavailable: {message}")
            }
            Self::Cancelled => write!(formatter, "source catalog extraction was cancelled"),
        }
    }
}

impl std::error::Error for SourceCatalogError {}

/// Lists every compilation unit's own file, plus the project-local headers
/// `libclang` reports it including, deduplicated and sorted by path.
///
/// Compilation units that `libclang` fails to parse are skipped rather than
/// failing the whole extraction, mirroring `type_catalog::extract_type_catalog`.
///
/// This is a second full `libclang` parse of every compilation unit,
/// independent of `type_catalog::extract_type_catalog`'s own parse — `libclang`
/// doesn't expose a way to share the already-parsed AST between the two, so
/// it re-pays that cost. Parallelized the same way and for the same reason
/// (see that function's doc comment).
pub fn extract_source_files(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
) -> Result<Vec<SourceFile>, SourceCatalogError> {
    extract_source_files_cancellable(compilation_units, project_root, progress, None)
}

/// Same as [`extract_source_files`], but stops early once `cancellation` is
/// signalled (US-4 criterion 7), mirroring
/// `type_catalog::extract_type_catalog_cancellable`.
pub fn extract_source_files_cancellable(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> Result<Vec<SourceFile>, SourceCatalogError> {
    type_catalog::load_libclang().map_err(SourceCatalogError::LibclangUnavailable)?;

    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    if let Some(progress) = progress {
        progress.set_total(compilation_units.len());
    }

    let mut translation_units = BTreeSet::new();
    let mut headers = BTreeSet::new();

    if !compilation_units.is_empty() {
        let worker_count = std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
            .min(compilation_units.len());
        let chunk_size = compilation_units.len().div_ceil(worker_count);

        let partials = std::thread::scope(|scope| {
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
                        .expect("source catalog worker thread panicked")
                })
                .collect::<Vec<_>>()
        });

        for (partial_translation_units, partial_headers) in partials {
            translation_units.extend(partial_translation_units);
            headers.extend(partial_headers);
        }
    }

    if cancellation.is_some_and(Cancellation::is_cancelled) {
        return Err(SourceCatalogError::Cancelled);
    }

    Ok(finish_source_files(translation_units, headers))
}

/// Merges the translation-unit paths and discovered headers into the final
/// sorted `SourceFile` list — factored out of `extract_source_files_cancellable`
/// so `extraction::extract_project_catalogs_cancellable` (which drives its
/// own workers directly, sharing a parse with `type_catalog`) can reuse the
/// exact same merge behavior.
pub(crate) fn finish_source_files(
    translation_units: BTreeSet<String>,
    mut headers: BTreeSet<String>,
) -> Vec<SourceFile> {
    for translation_unit in &translation_units {
        headers.remove(translation_unit);
    }

    let mut files: Vec<SourceFile> = translation_units
        .into_iter()
        .map(|path| SourceFile {
            path,
            kind: SourceFileKind::TranslationUnit,
        })
        .chain(headers.into_iter().map(|path| SourceFile {
            path,
            kind: SourceFileKind::Header,
        }))
        .collect();
    files.sort_by(|left, right| left.path.cmp(&right.path));

    files
}

/// Parses `chunk`'s compilation units with a `CXIndex` private to this
/// worker, returning its own local translation units and headers for the
/// caller to merge (see `type_catalog::parse_chunk`, the same pattern).
fn parse_chunk(
    chunk: &[CompilationUnit],
    project_root: &Path,
    progress: Option<&ExtractionProgress>,
    cancellation: Option<&Cancellation>,
) -> (BTreeSet<String>, BTreeSet<String>) {
    // Each worker thread needs its own load: see `type_catalog::parse_chunk`
    // for why the calling thread's `load_libclang()` doesn't cover this one.
    type_catalog::load_libclang().expect(
        "libclang already loaded successfully on the calling thread; \
         per-thread load is not expected to fail",
    );

    let mut translation_units = BTreeSet::new();
    let mut headers = BTreeSet::new();

    unsafe {
        let index = clang_sys::clang_createIndex(0, 0);

        for unit in chunk {
            if cancellation.is_some_and(Cancellation::is_cancelled) {
                break;
            }

            if let Some(canonical) = canonicalize_within(&unit.file, project_root) {
                translation_units.insert(canonical);
            }

            collect_inclusions(index, unit, project_root, &mut headers);

            if let Some(progress) = progress {
                progress.mark_one_done();
            }
        }

        clang_sys::clang_disposeIndex(index);
    }

    (translation_units, headers)
}

struct InclusionState<'a> {
    project_root: &'a Path,
    headers: &'a mut BTreeSet<String>,
}

unsafe fn collect_inclusions(
    index: clang_sys::CXIndex,
    unit: &CompilationUnit,
    project_root: &Path,
    headers: &mut BTreeSet<String>,
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

    unsafe {
        let translation_unit = clang_sys::clang_parseTranslationUnit(
            index,
            file.as_ptr(),
            arg_ptrs.as_ptr(),
            arg_ptrs.len() as c_int,
            std::ptr::null_mut(),
            0,
            clang_sys::CXTranslationUnit_SkipFunctionBodies,
        );

        if translation_unit.is_null() {
            return;
        }

        collect_inclusions_from_parsed(translation_unit, project_root, headers);

        clang_sys::clang_disposeTranslationUnit(translation_unit);
    }
}

/// The half of `collect_inclusions` that doesn't own the translation unit's
/// lifetime, so `extraction::extract_project_catalogs_cancellable` can call
/// it against a translation unit `type_catalog` already parsed (with
/// `type_catalog::PARSE_FLAGS`, a superset of this module's own
/// `SkipFunctionBodies`-only flags) instead of reparsing.
/// `clang_getInclusions` reads a translation unit's inclusion table
/// directly, unaffected by the extra `DetailedPreprocessingRecord` flag
/// that superset adds.
pub(crate) unsafe fn collect_inclusions_from_parsed(
    translation_unit: clang_sys::CXTranslationUnit,
    project_root: &Path,
    headers: &mut BTreeSet<String>,
) {
    let mut state = InclusionState {
        project_root,
        headers,
    };
    unsafe {
        clang_sys::clang_getInclusions(
            translation_unit,
            inclusion_visitor,
            &mut state as *mut InclusionState<'_> as *mut c_void,
        );
    }
}

extern "C" fn inclusion_visitor(
    included_file: clang_sys::CXFile,
    _inclusion_stack: *mut clang_sys::CXSourceLocation,
    _include_length: c_uint,
    client_data: clang_sys::CXClientData,
) {
    let state = unsafe { &mut *(client_data as *mut InclusionState<'_>) };

    let file_name =
        unsafe { type_catalog::cxstring_to_string(clang_sys::clang_getFileName(included_file)) };
    if file_name.is_empty() {
        return;
    }

    if let Some(canonical) = canonicalize_within(&file_name, state.project_root) {
        state.headers.insert(canonical);
    }
}

pub(crate) fn canonicalize_within(file_name: &str, project_root: &Path) -> Option<String> {
    let path = PathBuf::from(file_name);
    let canonical = path.canonicalize().unwrap_or(path);
    if canonical.starts_with(project_root) {
        Some(canonical.display().to_string())
    } else {
        None
    }
}
