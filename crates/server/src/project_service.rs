use std::fmt;
use std::path::Path;

use crate::ingest::{self, CreateProjectRequest, CreatedProject, IngestError};
use crate::persistence::{GlobalStore, PersistenceError, ProjectStore};
use crate::type_catalog::{self, TypeCatalogError};

pub const DEFAULT_SOURCE_LANGUAGE: &str = "cpp";
pub const DEFAULT_TARGET_LANGUAGE: &str = "dart";

#[derive(Debug)]
pub enum ProjectCreationError {
    Ingest(IngestError),
    Persistence(PersistenceError),
    TypeCatalog(TypeCatalogError),
}

impl ProjectCreationError {
    pub fn is_client_error(&self) -> bool {
        match self {
            Self::Ingest(error) => error.is_client_error(),
            Self::Persistence(_) => false,
            Self::TypeCatalog(_) => false,
        }
    }
}

impl fmt::Display for ProjectCreationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ingest(error) => write!(formatter, "{error}"),
            Self::Persistence(error) => write!(formatter, "{error}"),
            Self::TypeCatalog(error) => write!(formatter, "{error}"),
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

    let project_db_path = project.project_dir.join("project.db");
    let mut project_store = ProjectStore::open(&project_db_path)?;
    project_store.replace_compilation_units(&project.compilation_units)?;
    project_store.replace_type_declarations(&project.type_catalog)?;
    project_store.replace_type_dependencies(&project.type_dependencies)?;

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
