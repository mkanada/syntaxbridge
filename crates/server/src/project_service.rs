use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::function_catalog::{self, CallEdge, FunctionCatalogError, FunctionDeclaration};
use crate::ingest::{self, CompilationUnit, CreateProjectRequest, CreatedProject, IngestError};
use crate::persistence::{GlobalStore, PersistenceError, ProjectRecord, ProjectStore};
use crate::progress::{Cancellation, ExtractionProgress};
use crate::source_catalog::{self, SourceCatalogError, SourceFile};
use crate::type_catalog::{self, TypeCatalogError, TypeDeclaration, TypeUsage};

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
    pub cancellation: Cancellation,
}

pub const DEFAULT_SOURCE_LANGUAGE: &str = "cpp";
pub const DEFAULT_TARGET_LANGUAGE: &str = "dart";

#[derive(Debug)]
pub enum ProjectCreationError {
    Ingest(IngestError),
    Persistence(PersistenceError),
    TypeCatalog(TypeCatalogError),
    SourceCatalog(SourceCatalogError),
    FunctionCatalog(FunctionCatalogError),
}

impl ProjectCreationError {
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::Ingest(error) => error.is_client_error(),
            Self::Persistence(_) => false,
            Self::TypeCatalog(_) => false,
            Self::SourceCatalog(_) => false,
            Self::FunctionCatalog(_) => false,
        }
    }

    /// Distinguishes a user-requested cancellation (US-4 criterion 7) from a
    /// real failure, so `jobs.rs`/the HTTP layer can report `"cancelled"`
    /// instead of `"failed"` — cancelling isn't an error the user needs a
    /// message about.
    pub fn is_cancelled(&self) -> bool {
        matches!(
            self,
            Self::TypeCatalog(TypeCatalogError::Cancelled)
                | Self::SourceCatalog(SourceCatalogError::Cancelled)
                | Self::FunctionCatalog(FunctionCatalogError::Cancelled)
        )
    }
}

impl fmt::Display for ProjectCreationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ingest(error) => write!(formatter, "{error}"),
            Self::Persistence(error) => write!(formatter, "{error}"),
            Self::TypeCatalog(error) => write!(formatter, "{error}"),
            Self::SourceCatalog(error) => write!(formatter, "{error}"),
            Self::FunctionCatalog(error) => write!(formatter, "{error}"),
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

impl From<TypeCatalogError> for ProjectCreationError {
    fn from(error: TypeCatalogError) -> Self {
        Self::TypeCatalog(error)
    }
}

impl From<SourceCatalogError> for ProjectCreationError {
    fn from(error: SourceCatalogError) -> Self {
        Self::SourceCatalog(error)
    }
}

impl From<FunctionCatalogError> for ProjectCreationError {
    fn from(error: FunctionCatalogError) -> Self {
        Self::FunctionCatalog(error)
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

    let catalog = type_catalog::extract_type_catalog_cancellable(
        &project.compilation_units,
        &project.input_source_dir,
        progress.map(|progress| &progress.type_catalog),
        progress.map(|progress| &progress.cancellation),
    )?;
    project.type_catalog = catalog.declarations;
    project.type_dependencies = catalog.dependencies;
    let type_usages = catalog.usages;

    project.source_files = source_catalog::extract_source_files_cancellable(
        &project.compilation_units,
        &project.input_source_dir,
        progress.map(|progress| &progress.source_catalog),
        progress.map(|progress| &progress.cancellation),
    )?;

    let function_catalog = function_catalog::extract_function_catalog_cancellable(
        &project.compilation_units,
        &project.input_source_dir,
        progress.map(|progress| &progress.function_catalog),
        progress.map(|progress| &progress.cancellation),
    )?;

    let project_db_path = project.project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    project_store.replace_compilation_units(&project.compilation_units)?;
    project_store.replace_type_declarations(&project.type_catalog)?;
    project_store.replace_type_dependencies(&project.type_dependencies)?;
    project_store.replace_type_usages(&type_usages)?;
    project_store.replace_source_files(&project.source_files)?;
    project_store.replace_function_declarations(&function_catalog.declarations)?;
    project_store.replace_call_edges(&function_catalog.calls)?;

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
