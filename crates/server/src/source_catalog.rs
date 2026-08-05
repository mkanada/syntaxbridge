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
}

impl fmt::Display for SourceCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibclangUnavailable(message) => {
                write!(formatter, "libclang is unavailable: {message}")
            }
        }
    }
}

impl std::error::Error for SourceCatalogError {}

/// Lists every compilation unit's own file, plus the project-local headers
/// `libclang` reports it including, deduplicated and sorted by path.
///
/// Compilation units that `libclang` fails to parse are skipped rather than
/// failing the whole extraction, mirroring `type_catalog::extract_type_catalog`.
pub fn extract_source_files(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
) -> Result<Vec<SourceFile>, SourceCatalogError> {
    type_catalog::load_libclang().map_err(SourceCatalogError::LibclangUnavailable)?;

    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let mut translation_units = BTreeSet::new();
    let mut headers = BTreeSet::new();

    unsafe {
        let index = clang_sys::clang_createIndex(0, 0);

        for unit in compilation_units {
            if let Some(canonical) = canonicalize_within(&unit.file, &project_root) {
                translation_units.insert(canonical);
            }

            collect_inclusions(index, unit, &project_root, &mut headers);
        }

        clang_sys::clang_disposeIndex(index);
    }

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

    Ok(files)
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

        let mut state = InclusionState {
            project_root,
            headers,
        };
        clang_sys::clang_getInclusions(
            translation_unit,
            inclusion_visitor,
            &mut state as *mut InclusionState<'_> as *mut c_void,
        );

        clang_sys::clang_disposeTranslationUnit(translation_unit);
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

fn canonicalize_within(file_name: &str, project_root: &Path) -> Option<String> {
    let path = PathBuf::from(file_name);
    let canonical = path.canonicalize().unwrap_or(path);
    if canonical.starts_with(project_root) {
        Some(canonical.display().to_string())
    } else {
        None
    }
}
