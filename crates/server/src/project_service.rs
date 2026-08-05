use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::ingest::{self, CompilationUnit, CreateProjectRequest, CreatedProject, IngestError};
use crate::persistence::{GlobalStore, PersistenceError, ProjectRecord, ProjectStore};
use crate::source_catalog::{self, SourceCatalogError, SourceFile};
use crate::type_catalog::{self, TypeCatalogError};

pub const DEFAULT_SOURCE_LANGUAGE: &str = "cpp";
pub const DEFAULT_TARGET_LANGUAGE: &str = "dart";

#[derive(Debug)]
pub enum ProjectCreationError {
    Ingest(IngestError),
    Persistence(PersistenceError),
    TypeCatalog(TypeCatalogError),
    SourceCatalog(SourceCatalogError),
}

impl ProjectCreationError {
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::Ingest(error) => error.is_client_error(),
            Self::Persistence(_) => false,
            Self::TypeCatalog(_) => false,
            Self::SourceCatalog(_) => false,
        }
    }
}

impl fmt::Display for ProjectCreationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ingest(error) => write!(formatter, "{error}"),
            Self::Persistence(error) => write!(formatter, "{error}"),
            Self::TypeCatalog(error) => write!(formatter, "{error}"),
            Self::SourceCatalog(error) => write!(formatter, "{error}"),
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

/// Ingests a project from an archive and persists it: the project's own
/// compilation units go into a database inside its project directory, and
/// the project is registered into the shared global database so it can be
/// listed and reopened later.
pub fn create_project(
    request: CreateProjectRequest,
    global_db_path: &Path,
) -> Result<CreatedProject, ProjectCreationError> {
    let mut project = ingest::create_project(request)?;

    let catalog =
        type_catalog::extract_type_catalog(&project.compilation_units, &project.input_source_dir)?;
    project.type_catalog = catalog.declarations;
    project.type_dependencies = catalog.dependencies;

    project.source_files = source_catalog::extract_source_files(
        &project.compilation_units,
        &project.input_source_dir,
    )?;

    let project_db_path = project.project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    project_store.replace_compilation_units(&project.compilation_units)?;
    project_store.replace_type_declarations(&project.type_catalog)?;
    project_store.replace_type_dependencies(&project.type_dependencies)?;
    project_store.replace_source_files(&project.source_files)?;

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

/// Lists the last 5 projects the app was used with, most recently opened
/// first, for the entry screen shown on startup.
pub fn list_recent_projects(global_db_path: &Path) -> Result<Vec<ProjectRecord>, PersistenceError> {
    let global_store = GlobalStore::open(global_db_path)?;
    global_store.recent_projects(5)
}

/// Reloads a project directly from its own `project.db`, without running
/// ingest again, and registers it (or touches `last_opened_at`) in the
/// global project registry. Used both to reopen a recent project and to
/// import a project that already exists on disk.
pub fn open_project(
    project_dir: &Path,
    global_db_path: &Path,
) -> Result<LoadedProject, OpenProjectError> {
    let project_db_path = project_dir.join("project.db");
    if !project_db_path.is_file() {
        return Err(OpenProjectError::NotFound(project_dir.to_path_buf()));
    }

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
