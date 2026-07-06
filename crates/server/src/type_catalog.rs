//! Extracts the catalog of named types (structs, classes, unions, enums,
//! typedefs, type aliases and macro `#define`s) declared across a project's
//! compilation units, using `libclang`.
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

use serde::Serialize;

use crate::ingest::CompilationUnit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TypeDeclarationKind {
    Struct,
    Class,
    Union,
    Enum,
    Typedef,
    TypeAlias,
    Macro,
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
            Self::Macro => "macro",
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
            "macro" => Some(Self::Macro),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TypeDeclaration {
    pub name: String,
    pub kind: TypeDeclarationKind,
    pub file: String,
    pub line: u32,
    pub column: u32,
}

/// An edge in the type dependency graph: `caller` references `callee` in its
/// own definition (a struct/class/union field, a base class, or the
/// underlying type of a typedef/type alias).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct TypeDependency {
    pub caller: TypeDeclaration,
    pub callee: TypeDeclaration,
}

/// The catalog of named types declared across a project, together with the
/// dependency edges between them.
#[derive(Debug, Clone, Default)]
pub struct TypeCatalog {
    pub declarations: Vec<TypeDeclaration>,
    pub dependencies: Vec<TypeDependency>,
}

#[derive(Debug)]
pub enum TypeCatalogError {
    LibclangUnavailable(String),
}

impl fmt::Display for TypeCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibclangUnavailable(message) => {
                write!(formatter, "libclang is unavailable: {message}")
            }
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
pub fn extract_type_catalog(
    compilation_units: &[CompilationUnit],
    project_root: &Path,
) -> Result<TypeCatalog, TypeCatalogError> {
    ensure_libclang_loaded()?;

    let project_root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());

    let mut seen = HashSet::new();
    let mut declarations = Vec::new();
    let mut dependency_seen = HashSet::new();
    let mut dependencies = Vec::new();

    unsafe {
        let index = clang_sys::clang_createIndex(0, 0);

        for unit in compilation_units {
            let mut visitor_state = VisitorState {
                project_root: &project_root,
                declarations: &mut declarations,
                seen: &mut seen,
                dependencies: &mut dependencies,
                dependency_seen: &mut dependency_seen,
            };

            visit_translation_unit(index, unit, &mut visitor_state);
        }

        clang_sys::clang_disposeIndex(index);
    }

    Ok(TypeCatalog {
        declarations,
        dependencies,
    })
}

struct VisitorState<'a> {
    project_root: &'a Path,
    declarations: &'a mut Vec<TypeDeclaration>,
    seen: &'a mut HashSet<(TypeDeclarationKind, String, String, u32, u32)>,
    dependencies: &'a mut Vec<TypeDependency>,
    dependency_seen: &'a mut HashSet<(TypeDeclaration, TypeDeclaration)>,
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
fn build_clang_args(unit: &CompilationUnit) -> Vec<String> {
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
    let is_relevant = declaration_kind_for(kind).filter(|declaration_kind| {
        *declaration_kind == TypeDeclarationKind::Macro
            || unsafe { clang_sys::clang_isCursorDefinition(cursor) } != 0
    });

    if let Some(declaration_kind) = is_relevant
        && let Some(declaration) = describe_cursor(cursor, declaration_kind, state.project_root)
    {
        let key = (
            declaration.kind,
            declaration.name.clone(),
            declaration.file.clone(),
            declaration.line,
            declaration.column,
        );

        if state.seen.insert(key) {
            state.declarations.push(declaration.clone());
        }

        if matches!(
            declaration_kind,
            TypeDeclarationKind::Typedef | TypeDeclarationKind::TypeAlias
        ) && let underlying = unsafe { clang_sys::clang_getTypedefDeclUnderlyingType(cursor) }
            && let Some(callee) = resolve_named_declaration(underlying, state.project_root)
        {
            push_dependency(state.dependencies, state.dependency_seen, declaration, callee);
        }
    }

    if matches!(
        kind,
        clang_sys::CXCursor_FieldDecl | clang_sys::CXCursor_CXXBaseSpecifier
    ) {
        record_member_dependency(cursor, parent, state);
    }

    clang_sys::CXChildVisit_Recurse
}

/// Records a dependency edge from the struct/class/union `parent` cursor to
/// the named type declared by a field or base-class-specifier `cursor`,
/// skipping silently when either side isn't a type this catalog tracks
/// (builtins, or types outside `project_root`).
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

    push_dependency(state.dependencies, state.dependency_seen, caller, callee);
}

/// Strips pointers, references and array indirections off `cxtype`, then
/// resolves what remains to the `TypeDeclaration` of its defining cursor, if
/// any is known to this catalog.
fn resolve_named_declaration(
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
        clang_sys::CXCursor_MacroDefinition => Some(TypeDeclarationKind::Macro),
        _ => None,
    }
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

    Some(TypeDeclaration {
        name,
        kind,
        file: canonical_file_path.display().to_string(),
        line,
        column,
    })
}

unsafe fn cxstring_to_string(string: clang_sys::CXString) -> String {
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
    if clang_sys::is_loaded() {
        return Ok(());
    }

    clang_sys::load().map_err(TypeCatalogError::LibclangUnavailable)
}
