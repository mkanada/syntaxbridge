use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::externals::{self, ExternalMark, NameRegexRule, PathRegexRule};
use crate::extraction::{self, ExtractionError};
use crate::function_catalog::{self, CallEdge, FunctionDeclaration};
use crate::ingest::{self, CompilationUnit, CreateProjectRequest, CreatedProject, IngestError};
use crate::ir;
use crate::mapping;
use crate::persistence::{
    GlobalStore, PersistenceError, ProjectCatalogs, ProjectRecord, ProjectStore,
};
use crate::pointer_catalog::{self, PointerDeclaration};
use crate::progress::{Cancellation, ExtractionProgress};
use crate::source_catalog::SourceFile;
use crate::transpile::{self, TranspileError, TranspiledPackage};
use crate::type_catalog::{TypeDeclaration, TypeUsage};
use crate::validate::dart::{self as validate_dart, DartDiagnostic};

/// Live progress trackers for `create_project`'s three `libclang` passes, so
/// a caller running it in the background (`jobs.rs`) can report real
/// progress to a poller instead of the caller having to wait for the whole
/// thing. `cancellation` is shared between all three (US-4 criterion 7,
/// extended to US-5's pass): stopping a job stops indexing regardless of
/// which pass is currently running, rather than requiring a separate flag
/// per pass.
#[derive(Default)]
pub struct CreationProgress {
    pub type_catalog: ExtractionProgress,
    pub source_catalog: ExtractionProgress,
    pub function_catalog: ExtractionProgress,
    pub pointer_catalog: ExtractionProgress,
    pub cancellation: Cancellation,
}

pub const DEFAULT_SOURCE_LANGUAGE: &str = "cpp";
pub const DEFAULT_TARGET_LANGUAGE: &str = "dart";

#[derive(Debug)]
pub enum ProjectCreationError {
    Ingest(IngestError),
    Persistence(PersistenceError),
    /// Covers all four `libclang` catalogs (types, source files, functions,
    /// pointers) — extracted together by `extraction::extract_project_catalogs_cancellable`
    /// rather than as four independent passes, so a failure in any of them
    /// surfaces through this one variant instead of one per catalog.
    Extraction(ExtractionError),
}

impl ProjectCreationError {
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::Ingest(error) => error.is_client_error(),
            Self::Persistence(_) => false,
            Self::Extraction(_) => false,
        }
    }

    /// Distinguishes a user-requested cancellation (US-4 criterion 7) from a
    /// real failure, so `jobs.rs`/the HTTP layer can report `"cancelled"`
    /// instead of `"failed"` — cancelling isn't an error the user needs a
    /// message about.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Extraction(ExtractionError::Cancelled))
    }
}

impl fmt::Display for ProjectCreationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ingest(error) => write!(formatter, "{error}"),
            Self::Persistence(error) => write!(formatter, "{error}"),
            Self::Extraction(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ProjectCreationError {}

impl From<IngestError> for ProjectCreationError {
    fn from(error: IngestError) -> Self {
        Self::Ingest(error)
    }
}

impl From<PersistenceError> for ProjectCreationError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

impl From<ExtractionError> for ProjectCreationError {
    fn from(error: ExtractionError) -> Self {
        Self::Extraction(error)
    }
}

/// Ingests a project from an archive and persists it: the project's own
/// compilation units go into a database inside its project directory, and
/// the project is registered into the shared global database so it can be
/// listed and reopened later.
pub fn create_project(
    request: CreateProjectRequest,
    global_db_path: &Path,
    progress: Option<&CreationProgress>,
) -> Result<CreatedProject, ProjectCreationError> {
    let mut project = ingest::create_project(request)?;

    let extracted = extraction::extract_project_catalogs_cancellable(
        &project.compilation_units,
        &project.input_source_dir,
        progress.map(|progress| &progress.type_catalog),
        progress.map(|progress| &progress.source_catalog),
        progress.map(|progress| &progress.function_catalog),
        progress.map(|progress| &progress.pointer_catalog),
        progress.map(|progress| &progress.cancellation),
    )?;
    project.type_catalog = extracted.type_catalog.declarations;
    project.type_dependencies = extracted.type_catalog.dependencies;
    let type_usages = extracted.type_catalog.usages;
    project.source_files = extracted.source_files;
    let function_catalog = extracted.function_catalog;
    project.pointer_catalog = extracted.pointer_catalog;

    let project_db_path = project.project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    project_store.replace_all(&ProjectCatalogs {
        compilation_units: &project.compilation_units,
        type_declarations: &project.type_catalog,
        type_dependencies: &project.type_dependencies,
        type_usages: &type_usages,
        source_files: &project.source_files,
        function_declarations: &function_catalog.declarations,
        call_edges: &function_catalog.calls,
        ir_functions: &function_catalog.ir_functions,
        ir_records: &function_catalog.ir_records,
        ir_enums: &function_catalog.ir_enums,
        pointer_declarations: &project.pointer_catalog,
    })?;

    let global_store = GlobalStore::open(global_db_path)?;
    global_store.register_project(
        &project.name,
        &project.project_dir,
        DEFAULT_SOURCE_LANGUAGE,
        DEFAULT_TARGET_LANGUAGE,
        "success",
    )?;

    Ok(project)
}

#[derive(Debug)]
pub enum ReadSourceFileError {
    OutsideProject,
    Io(io::Error),
}

impl ReadSourceFileError {
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::OutsideProject => true,
            Self::Io(error) => error.kind() == io::ErrorKind::NotFound,
        }
    }
}

impl fmt::Display for ReadSourceFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutsideProject => {
                write!(
                    formatter,
                    "requested path is outside the project's source tree"
                )
            }
            Self::Io(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ReadSourceFileError {}

/// A project reloaded from its own persisted data (`ProjectStore`), without
/// re-running ingest: used both to reopen one of the last-5 recent projects
/// and to import a project that already exists on disk from a prior ingest.
#[derive(Debug, Serialize)]
pub struct LoadedProject {
    pub name: String,
    pub project_dir: PathBuf,
    pub input_source_dir: PathBuf,
    pub compilation_units: Vec<CompilationUnit>,
    pub source_files: Vec<SourceFile>,
}

#[derive(Debug)]
pub enum OpenProjectError {
    NotFound(PathBuf),
    Persistence(PersistenceError),
}

impl OpenProjectError {
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::NotFound(_) => true,
            Self::Persistence(_) => false,
        }
    }
}

impl fmt::Display for OpenProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => {
                write!(
                    formatter,
                    "no syntax-bridge project found at {}",
                    path.display()
                )
            }
            Self::Persistence(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for OpenProjectError {}

impl From<PersistenceError> for OpenProjectError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

/// A recent project as offered on the entry screen.
///
/// `available` is resolved when listing rather than stored, because the user
/// can delete a project directory at any time without the app knowing: the
/// registry alone cannot say whether a project can still be opened.
#[derive(Debug, Serialize)]
pub struct RecentProject {
    pub name: String,
    pub project_dir: PathBuf,
    pub source_language: String,
    pub target_language: String,
    pub last_ingest_status: String,
    pub available: bool,
}

impl RecentProject {
    fn from_record(record: ProjectRecord) -> Self {
        Self {
            available: is_openable_project(&record.project_dir),
            name: record.name,
            project_dir: record.project_dir,
            source_language: record.source_language,
            target_language: record.target_language,
            last_ingest_status: record.last_ingest_status,
        }
    }
}

/// A project can be reopened when its own database is still on disk. This is
/// exactly what `open_project` checks, so the entry screen never presents a
/// project that opening would then reject.
fn is_openable_project(project_dir: &Path) -> bool {
    project_dir.join("project.db").is_file()
}

/// Lists the last 5 projects the app was used with, most recently opened
/// first, for the entry screen shown on startup. Each one is checked against
/// the filesystem so the screen can tell which are still openable.
pub fn list_recent_projects(global_db_path: &Path) -> Result<Vec<RecentProject>, PersistenceError> {
    let global_store = GlobalStore::open(global_db_path)?;
    let records = global_store.recent_projects(5)?;

    Ok(records
        .into_iter()
        .map(RecentProject::from_record)
        .collect())
}

/// Removes a project from the recent-projects registry. Nothing on disk is
/// touched: this only makes the app stop offering the project, which is what
/// a user wants after deleting (or moving) the project directory themselves.
pub fn forget_project(project_dir: &Path, global_db_path: &Path) -> Result<bool, PersistenceError> {
    let global_store = GlobalStore::open(global_db_path)?;
    global_store.forget_project(project_dir)
}

/// Reloads a project directly from its own `project.db`, without running
/// ingest again, and registers it (or touches `last_opened_at`) in the
/// global project registry. Used both to reopen a recent project and to
/// import a project that already exists on disk.
pub fn open_project(
    project_dir: &Path,
    global_db_path: &Path,
) -> Result<LoadedProject, OpenProjectError> {
    if !is_openable_project(project_dir) {
        return Err(OpenProjectError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    let compilation_units = project_store.list_compilation_units()?;
    let source_files = project_store.list_source_files()?;

    let name = project_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_owned();
    let input_source_dir = project_dir.join("input-source");

    let global_store = GlobalStore::open(global_db_path)?;
    global_store.register_project(
        &name,
        project_dir,
        DEFAULT_SOURCE_LANGUAGE,
        DEFAULT_TARGET_LANGUAGE,
        "success",
    )?;

    Ok(LoadedProject {
        name,
        project_dir: project_dir.to_path_buf(),
        input_source_dir,
        compilation_units,
        source_files,
    })
}

#[derive(Debug)]
pub enum ListTypesError {
    NotFound(PathBuf),
    Persistence(PersistenceError),
}

impl ListTypesError {
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::NotFound(_) => true,
            Self::Persistence(_) => false,
        }
    }
}

impl fmt::Display for ListTypesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => {
                write!(
                    formatter,
                    "no syntax-bridge project found at {}",
                    path.display()
                )
            }
            Self::Persistence(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ListTypesError {}

impl From<PersistenceError> for ListTypesError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

/// Serves the type catalog already persisted for a project (US-3), without
/// reparsing it — the gap `docs/plans/User Steps.md` calls out for
/// `LoadedProject`, closed here with a dedicated route instead of growing
/// that struct.
///
/// `usage_counts` (US-4) rides along keyed by `usr` rather than being
/// embedded per-`TypeDeclaration`, so the type-catalog model itself stays
/// free of usage-index concerns — the type list renders the count by looking
/// up each row's own `usr`.
#[derive(Debug, Serialize)]
pub struct TypeCatalogListing {
    pub types: Vec<TypeDeclaration>,
    pub usage_counts: HashMap<String, usize>,
}

pub fn list_types(project_dir: &Path) -> Result<TypeCatalogListing, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    Ok(TypeCatalogListing {
        types: project_store.list_type_declarations()?,
        usage_counts: project_store.type_usage_counts()?,
    })
}

/// Serves every recorded usage of one type (identified by its stable `usr`,
/// US-3), from the persisted index (US-4) without reparsing — what backs
/// "click a type, see every place it's used".
pub fn list_type_usages(
    project_dir: &Path,
    type_usr: &str,
) -> Result<Vec<TypeUsage>, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    Ok(project_store.list_type_usages_for(type_usr)?)
}

/// Serves the function/method/macro catalog already persisted for a project
/// (US-5), without reparsing — mirrors `TypeCatalogListing`/`list_types`.
/// `caller_counts` rides along keyed by `usr`, same reasoning as
/// `TypeCatalogListing::usage_counts`.
#[derive(Debug, Serialize)]
pub struct FunctionCatalogListing {
    pub functions: Vec<FunctionDeclaration>,
    pub caller_counts: HashMap<String, usize>,
}

pub fn list_functions(project_dir: &Path) -> Result<FunctionCatalogListing, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    Ok(FunctionCatalogListing {
        functions: project_store.list_function_declarations()?,
        caller_counts: project_store.call_counts()?,
    })
}

/// Serves every recorded caller of one function (identified by its stable
/// `usr`), from the persisted call graph (US-5) without reparsing — what
/// backs "click a function, see every place it's called" (criterion 5).
pub fn list_callers(project_dir: &Path, callee_usr: &str) -> Result<Vec<CallEdge>, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    Ok(project_store.list_callers_for(callee_usr)?)
}

/// Serves every recorded call site within one file, from the persisted call
/// graph (US-5) without reparsing — the flip side of `list_callers`, what
/// backs "click a call already on screen in the source viewer, jump to its
/// definition" (criterion 5's other direction).
pub fn list_calls_in_file(project_dir: &Path, file: &str) -> Result<Vec<CallEdge>, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    Ok(project_store.list_calls_in_file(file)?)
}

/// One concrete type a `return_type` pointer in [`PointerCatalogListing`]
/// can hold, per `mapping::pointer_options_for`'s narrowed answer.
#[derive(Debug, Serialize)]
pub struct PossibleType {
    pub usr: String,
    pub name: String,
}

/// Serves the pointer catalog already persisted for a project (Parte 1 of
/// `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`), without reparsing —
/// mirrors `TypeCatalogListing`/`list_types`.
///
/// `possible_types` is where the solver's narrowing (B07/B08,
/// `docs/mapping-solver-cases.md`) actually gets used outside a test: for
/// every `return_type` pointer whose pointee resolves to a project type,
/// this rebuilds a full `mapping::ProjectFacts` from what's already
/// persisted (declarations, usages, functions, calls — no reparsing) and
/// calls `mapping::pointer_options_for` with that pointer's own owning
/// function, keyed by the pointer's `usr` (which, for `return_type`, *is*
/// the owning function's own `usr` — see `pointer_catalog`'s module docs).
/// Parameter/field/local pointers are left out of `possible_types` for now:
/// nothing in `pointer_catalog` records which function a parameter/field/
/// local belongs to (only `return_type`'s `usr` doubles as that), so there
/// is no owning function to narrow against yet — see the "ligar
/// `pointer_catalog` à narrowing" item in
/// `docs/plans/catalogo-de-ponteiros-e-solver-tfa.md`.
#[derive(Debug, Serialize)]
pub struct PointerCatalogListing {
    pub pointers: Vec<PointerDeclaration>,
    pub possible_types: HashMap<String, Vec<PossibleType>>,
}

pub fn list_pointers(project_dir: &Path) -> Result<PointerCatalogListing, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    let pointers = project_store.list_pointer_declarations()?;
    let declarations = project_store.list_type_declarations()?;
    let usages = project_store.list_type_usages()?;
    let functions = project_store.list_function_declarations()?;
    let calls = project_store.list_call_edges()?;

    let facts = mapping::ProjectFacts::new_full(&declarations, &usages, &functions, &calls);

    // Indexed by `usr` once, up front, instead of the `.iter().find()` scans
    // this loop used to run per pointer (and per consequence within it) —
    // `.entry(...).or_insert(...)` keeps the same "first match wins" result
    // a linear `.find()` over possibly-duplicate `usr`s would have returned.
    let mut functions_by_usr = HashMap::with_capacity(functions.len());
    for function in &functions {
        functions_by_usr
            .entry(function.usr.as_str())
            .or_insert(function);
    }
    let mut declarations_by_usr = HashMap::with_capacity(declarations.len());
    for declaration in &declarations {
        declarations_by_usr
            .entry(declaration.usr.as_str())
            .or_insert(declaration);
    }

    let mut possible_types = HashMap::new();
    for pointer in &pointers {
        // Deterministic, so it applies to every pointer kind (not just
        // `ReturnType`) and needs no `ProjectFacts` at all — see
        // `scalar_pointee_dart_type`'s doc comment for why this is
        // narrower than "any scalar pointee".
        if let Some(dart_type) = mapping::scalar_pointee_dart_type(&pointer.pointee_type_name)
            && !pointer.usr.is_empty()
        {
            possible_types.insert(
                pointer.usr.clone(),
                vec![PossibleType {
                    usr: String::new(),
                    name: dart_type.to_owned(),
                }],
            );
            continue;
        }

        if pointer.kind != pointer_catalog::PointerDeclarationKind::ReturnType
            || pointer.pointee_usr.is_empty()
        {
            continue;
        }
        let Some(&owning_function) = functions_by_usr.get(pointer.usr.as_str()) else {
            continue;
        };
        let Some(&pointee) = declarations_by_usr.get(pointer.pointee_usr.as_str()) else {
            continue;
        };

        let options = mapping::pointer_options_for(
            mapping::PointeeShape::Known {
                usr: pointee.usr.clone(),
                name: pointee.name.clone(),
            },
            Some(&facts),
            Some(owning_function),
        );
        let types = options[0]
            .consequences
            .iter()
            .filter_map(|consequence| {
                declarations_by_usr
                    .get(consequence.affected_type_usr.as_str())
                    .map(|declaration| PossibleType {
                        usr: declaration.usr.clone(),
                        name: declaration.name.clone(),
                    })
            })
            .collect();
        possible_types.insert(pointer.usr.clone(), types);
    }

    Ok(PointerCatalogListing {
        pointers,
        possible_types,
    })
}

/// Serves the effective "extern" set (`docs/plans/lista-de-externos.md`)
/// plus both live regex rule lists, computed fresh from the already-
/// persisted catalogs on every request — mirrors `list_types`/`list_pointers`
/// in never reparsing, and mirrors `externals::effective_external_set`'s own
/// doc comment in never materializing the result: this function's whole job
/// is to gather that pure function's five inputs from the store.
#[derive(Debug, Serialize)]
pub struct ExternalListing {
    pub statuses: Vec<externals::ExternalStatus>,
    pub name_regexes: Vec<NameRegexRule>,
    pub path_regexes: Vec<PathRegexRule>,
}

pub fn list_externals(project_dir: &Path) -> Result<ExternalListing, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    let type_declarations = project_store.list_type_declarations()?;
    let function_declarations = project_store.list_function_declarations()?;
    let call_edges = project_store.list_call_edges()?;
    let name_regexes = project_store.list_name_regexes()?;
    let path_regexes = project_store.list_path_regexes()?;
    let manual_marks = project_store.list_external_marks()?;

    let statuses = externals::effective_external_set(
        &type_declarations,
        &function_declarations,
        &call_edges,
        &name_regexes,
        &path_regexes,
        &manual_marks,
    );

    Ok(ExternalListing {
        statuses,
        name_regexes,
        path_regexes,
    })
}

/// Records a manual external/not-external decision for one usr (decision 5,
/// `docs/plans/lista-de-externos.md`) — the direct-marking path used from
/// the Types/Functions views (a single usr, no expansion needed).
pub fn mark_external(project_dir: &Path, usr: &str, external: bool) -> Result<(), ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    project_store.set_external_mark(&ExternalMark {
        usr: usr.to_owned(),
        external,
        decided_at: now_timestamp(),
    })?;
    Ok(())
}

/// Marks every type/function declared in `file` external — decision 3's
/// "cascata é foto": expands to the file's usrs *now*
/// (`externals::expand_file_mark`) and writes one independent mark per usr;
/// nothing links them back to the file afterward. Returns the usrs marked,
/// so the caller can show the user what just happened.
pub fn mark_file_external(project_dir: &Path, file: &str) -> Result<Vec<String>, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    let type_declarations = project_store.list_type_declarations()?;
    let function_declarations = project_store.list_function_declarations()?;
    let usrs = externals::expand_file_mark(file, &type_declarations, &function_declarations);

    let decided_at = now_timestamp();
    for usr in &usrs {
        project_store.set_external_mark(&ExternalMark {
            usr: usr.clone(),
            external: true,
            decided_at: decided_at.clone(),
        })?;
    }
    Ok(usrs)
}

/// Same shape as [`mark_file_external`], expanding a type to itself plus
/// every method it owns (`externals::expand_type_mark`) instead of a file
/// to its contents.
pub fn mark_type_external(
    project_dir: &Path,
    type_usr: &str,
) -> Result<Vec<String>, ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    let function_declarations = project_store.list_function_declarations()?;
    let usrs = externals::expand_type_mark(type_usr, &function_declarations);

    let decided_at = now_timestamp();
    for usr in &usrs {
        project_store.set_external_mark(&ExternalMark {
            usr: usr.clone(),
            external: true,
            decided_at: decided_at.clone(),
        })?;
    }
    Ok(usrs)
}

#[derive(Debug)]
pub enum AddRegexError {
    NotFound(PathBuf),
    InvalidPattern(String),
    Persistence(PersistenceError),
}

impl AddRegexError {
    pub fn is_client_error(&self) -> bool {
        matches!(self, Self::NotFound(_) | Self::InvalidPattern(_))
    }
}

impl fmt::Display for AddRegexError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(formatter, "no project found at {}", path.display()),
            Self::InvalidPattern(message) => write!(formatter, "invalid regex pattern: {message}"),
            Self::Persistence(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for AddRegexError {}

impl From<PersistenceError> for AddRegexError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

/// Adds a name-regex rule (decision 6, `docs/plans/lista-de-externos.md`) —
/// refuses an invalid pattern (`externals::validate_regex_pattern`) instead
/// of persisting it and letting `effective_external_set` silently skip it
/// on every future read, per that function's own doc comment on the
/// invariant this call site is responsible for upholding.
pub fn add_name_regex(project_dir: &Path, pattern: &str) -> Result<NameRegexRule, AddRegexError> {
    if !is_openable_project(project_dir) {
        return Err(AddRegexError::NotFound(project_dir.to_path_buf()));
    }
    externals::validate_regex_pattern(pattern).map_err(AddRegexError::InvalidPattern)?;

    let project_db_path = project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    let created_at = now_timestamp();
    let id = project_store.add_name_regex(pattern, &created_at)?;
    Ok(NameRegexRule {
        id,
        pattern: pattern.to_owned(),
        created_at,
    })
}

pub fn remove_name_regex(project_dir: &Path, id: i64) -> Result<(), ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }
    let project_db_path = project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    project_store.remove_name_regex(id)?;
    Ok(())
}

/// Same shape as [`add_name_regex`], for path-regex rules.
pub fn add_path_regex(project_dir: &Path, pattern: &str) -> Result<PathRegexRule, AddRegexError> {
    if !is_openable_project(project_dir) {
        return Err(AddRegexError::NotFound(project_dir.to_path_buf()));
    }
    externals::validate_regex_pattern(pattern).map_err(AddRegexError::InvalidPattern)?;

    let project_db_path = project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    let created_at = now_timestamp();
    let id = project_store.add_path_regex(pattern, &created_at)?;
    Ok(PathRegexRule {
        id,
        pattern: pattern.to_owned(),
        created_at,
    })
}

pub fn remove_path_regex(project_dir: &Path, id: i64) -> Result<(), ListTypesError> {
    if !is_openable_project(project_dir) {
        return Err(ListTypesError::NotFound(project_dir.to_path_buf()));
    }
    let project_db_path = project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    project_store.remove_path_regex(id)?;
    Ok(())
}

/// Milliseconds since the Unix epoch, as a decimal string — not true RFC
/// 3339 despite `ExternalMark`/`NameRegexRule`/`PathRegexRule`'s `TEXT`
/// columns being able to hold either: no date/time crate is a dependency of
/// this workspace, and `global_store`'s `created_at_millis`/
/// `last_opened_at_millis` already establish "epoch millis, stored as a
/// plain number" as this codebase's actual precedent for a server-generated
/// timestamp, not the aspirational RFC 3339 `mapping::MappingDecision`'s
/// doc comment describes but that no production code has ever actually
/// generated (only hand-written test literals).
fn now_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().to_string())
        .unwrap_or_else(|_| "0".to_owned())
}

/// Reads a single source file's content for display, refusing to read
/// anything outside `project_dir`'s `input-source` subtree even if the
/// caller supplies an absolute path elsewhere on disk.
pub fn read_source_file(
    project_dir: &Path,
    requested_path: &Path,
) -> Result<String, ReadSourceFileError> {
    let canonical_root = project_dir
        .join("input-source")
        .canonicalize()
        .map_err(ReadSourceFileError::Io)?;
    let canonical_path = requested_path
        .canonicalize()
        .map_err(ReadSourceFileError::Io)?;

    if !canonical_path.starts_with(&canonical_root) {
        return Err(ReadSourceFileError::OutsideProject);
    }

    fs::read_to_string(&canonical_path).map_err(ReadSourceFileError::Io)
}

#[derive(Debug)]
pub enum TranspileProjectError {
    NotFound(PathBuf),
    Persistence(PersistenceError),
    Transpile(TranspileError),
}

impl TranspileProjectError {
    pub fn is_client_error(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }
}

impl fmt::Display for TranspileProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => {
                write!(
                    formatter,
                    "no syntax-bridge project found at {}",
                    path.display()
                )
            }
            Self::Persistence(error) => write!(formatter, "{error}"),
            Self::Transpile(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for TranspileProjectError {}

impl From<PersistenceError> for TranspileProjectError {
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error)
    }
}

impl From<TranspileError> for TranspileProjectError {
    fn from(error: TranspileError) -> Self {
        Self::Transpile(error)
    }
}

/// Transpiles a project's free functions to Dart (E01–E03 scope, US-8) and
/// writes the resulting package under `<project_dir>/transpiled/` — nothing
/// is written outside the project directory. Synchronous by design (PR2
/// decision, `docs/plans/primeiro-corte-e01-e03.md` §7): these examples
/// transpile in milliseconds, so this doesn't yet reuse `jobs.rs` the way
/// project creation (US-1) does — that's for when the cost actually shows up
/// (E11/E13), same call `list_types`'s docs already make for reparsing.
///
/// Reads the IR `create_project` already persisted (`ProjectStore::list_ir`)
/// instead of reparsing every compilation unit with `libclang` again here —
/// the same "serve from the store, don't reparse" rule `list_types`/
/// `list_functions`/`list_callers` already follow for their own catalogs.
/// Falls back to a full parse only for a project whose database predates IR
/// persistence (an empty `list_ir` result despite the project having
/// compilation units) so an old project doesn't silently transpile into an
/// empty package.
pub fn transpile_project(project_dir: &Path) -> Result<TranspiledPackage, TranspileProjectError> {
    Ok(build_transpiled_package(project_dir)?.1)
}

/// Shared by [`transpile_project`] and [`validate_project`]: builds the same
/// [`ir::Module`]/[`TranspiledPackage`] pair and writes the package under
/// `<project_dir>/transpiled/` — `validate_project` needs the `Module` too
/// (to translate `dart analyze` diagnostics back to C++ origins,
/// `validate::dart::analyze_package`), which plain `transpile_project`
/// discards, but both must build it exactly the same way.
fn build_transpiled_package(
    project_dir: &Path,
) -> Result<(ir::Module, TranspiledPackage), TranspileProjectError> {
    if !is_openable_project(project_dir) {
        return Err(TranspileProjectError::NotFound(project_dir.to_path_buf()));
    }

    let project_db_path = project_dir.join("project.db");
    let project_store = ProjectStore::open(&project_db_path)?;
    let compilation_units = project_store.list_compilation_units()?;
    let type_catalog = project_store.list_type_declarations()?;
    let type_mappings = project_store.list_type_mappings()?;
    // `docs/plans/lista-de-externos.md`: the effective external set is
    // always recomputed from the persisted catalogs, never itself
    // persisted — same "decision persisted, effect computed" shape
    // `type_mappings` above already uses. Only `effective: true` usrs are
    // passed to the emitter; `MockContext::is_external` is only ever
    // consulted with a function/method/constructor usr, so the type usrs
    // also present here are harmless, never looked up.
    let external_function_declarations = project_store.list_function_declarations()?;
    let external_call_edges = project_store.list_call_edges()?;
    let name_regexes = project_store.list_name_regexes()?;
    let path_regexes = project_store.list_path_regexes()?;
    let external_marks = project_store.list_external_marks()?;
    let external_usrs: std::collections::HashSet<String> = externals::effective_external_set(
        &type_catalog,
        &external_function_declarations,
        &external_call_edges,
        &name_regexes,
        &path_regexes,
        &external_marks,
    )
    .into_iter()
    .filter(|status| status.effective)
    .map(|status| status.usr)
    .collect();
    let input_source_dir = project_dir.join("input-source");
    let package_name = project_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("syntax_bridge_output");

    let (ir_functions, ir_records, ir_enums) = project_store.list_ir()?;
    let module = if ir_functions.is_empty()
        && ir_records.is_empty()
        && !compilation_units.is_empty()
    {
        let catalog =
            function_catalog::extract_function_catalog(&compilation_units, &input_source_dir, None)
                .map_err(TranspileError::from)?;
        ir::Module {
            functions: catalog.ir_functions,
            records: catalog.ir_records,
            enums: catalog.ir_enums,
        }
    } else {
        ir::Module {
            functions: ir_functions,
            records: ir_records,
            enums: ir_enums,
        }
    };
    let package = transpile::emit_package_with_externals(
        &module,
        package_name,
        &type_catalog,
        &type_mappings,
        &external_usrs,
    )?;
    transpile::write_package(&package, &project_dir.join("transpiled"))?;

    Ok((module, package))
}

#[derive(Debug)]
pub enum ValidateProjectError {
    Transpile(TranspileProjectError),
    Validate(validate_dart::ValidateError),
}

impl ValidateProjectError {
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::Transpile(error) => error.is_client_error(),
            Self::Validate(_) => false,
        }
    }
}

impl fmt::Display for ValidateProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transpile(error) => write!(formatter, "{error}"),
            Self::Validate(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for ValidateProjectError {}

impl From<TranspileProjectError> for ValidateProjectError {
    fn from(error: TranspileProjectError) -> Self {
        Self::Transpile(error)
    }
}

/// Transpiles a project (same package `POST /projects/transpile` would
/// write) and runs `dart analyze` against it, translating every diagnostic
/// back to the C++ declaration it came from (US-9, criterion 3) —
/// `validate::dart::analyze_package`'s own doc comment has the granularity
/// that translation resolves at.
pub fn validate_project(project_dir: &Path) -> Result<Vec<DartDiagnostic>, ValidateProjectError> {
    let (module, package) = build_transpiled_package(project_dir)?;
    validate_dart::analyze_package(&module, &package, &project_dir.join("transpiled"))
        .map_err(ValidateProjectError::Validate)
}
