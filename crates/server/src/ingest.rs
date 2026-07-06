use std::env;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::type_catalog::{TypeDeclaration, TypeDependency};

#[derive(Debug, Deserialize)]
pub struct CreateProjectRequest {
    #[serde(alias = "project_name")]
    pub name: String,
    pub workspace_dir: PathBuf,
    pub archive_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct CreatedProject {
    pub name: String,
    pub project_dir: PathBuf,
    pub input_source_dir: PathBuf,
    pub cmake_source_dir: PathBuf,
    pub build_dir: PathBuf,
    pub compile_commands_path: PathBuf,
    pub compilation_units: Vec<CompilationUnit>,
    /// Populated by `project_service::create_project` after ingest, once the
    /// compilation units are known: `ingest::create_project` itself has no
    /// use for `libclang` and leaves this empty.
    pub type_catalog: Vec<TypeDeclaration>,
    /// Dependency edges between cataloged types (fields, base classes,
    /// typedef/alias underlying types), populated alongside `type_catalog`.
    pub type_dependencies: Vec<TypeDependency>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CompilationUnit {
    pub directory: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<String>,
}

#[derive(Debug)]
pub enum IngestError {
    InvalidProjectName(String),
    UnsupportedArchive(PathBuf),
    UnsafeArchiveEntry(String),
    MissingCMakeProject(PathBuf),
    MissingCompileCommands(PathBuf),
    EmptyCompileCommands(PathBuf),
    Io(io::Error),
    ToolFailed {
        tool: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    Json(serde_json::Error),
}

impl IngestError {
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            Self::InvalidProjectName(_)
                | Self::UnsupportedArchive(_)
                | Self::UnsafeArchiveEntry(_)
                | Self::MissingCMakeProject(_)
                | Self::MissingCompileCommands(_)
                | Self::EmptyCompileCommands(_),
        )
    }
}

impl fmt::Display for IngestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProjectName(name) => {
                write!(formatter, "invalid project name: {name}")
            }
            Self::UnsupportedArchive(path) => {
                write!(formatter, "unsupported archive format: {}", path.display())
            }
            Self::UnsafeArchiveEntry(entry) => {
                write!(formatter, "archive contains an unsafe entry: {entry}")
            }
            Self::MissingCMakeProject(path) => {
                write!(
                    formatter,
                    "no CMakeLists.txt found under {}",
                    path.display()
                )
            }
            Self::MissingCompileCommands(path) => {
                write!(
                    formatter,
                    "compile_commands.json was not created at {}",
                    path.display()
                )
            }
            Self::EmptyCompileCommands(path) => {
                write!(
                    formatter,
                    "compile_commands.json has no entries: {}",
                    path.display()
                )
            }
            Self::Io(error) => write!(formatter, "{error}"),
            Self::ToolFailed {
                tool,
                status,
                stderr,
            } => {
                write!(
                    formatter,
                    "{tool} failed with status {:?}: {}",
                    status,
                    stderr.trim()
                )
            }
            Self::Json(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<io::Error> for IngestError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for IngestError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

pub fn create_project(request: CreateProjectRequest) -> Result<CreatedProject, IngestError> {
    log_ingest(format_args!("create_project: start"));
    log_ingest(format_args!("project name: {:?}", request.name));
    log_ingest(format_args!(
        "workspace_dir: {}",
        request.workspace_dir.display()
    ));
    log_ingest(format_args!(
        "archive_path: {}",
        request.archive_path.display()
    ));
    log_ingest(format_args!(
        "current_dir: {}",
        env::current_dir()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|error| format!("<unavailable: {error}>"))
    ));
    log_ingest(format_args!(
        "PATH: {}",
        env::var("PATH").unwrap_or_else(|_| "<unset>".to_owned())
    ));

    validate_project_name(&request.name)?;
    log_ingest(format_args!("project name validated"));

    let project_dir = request.workspace_dir.join(&request.name);
    let input_source_dir = project_dir.join("input-source");
    let build_dir = project_dir.join("build");

    log_path_metadata("workspace_dir before create", &request.workspace_dir);
    log_path_metadata("archive_path before extract", &request.archive_path);
    log_ingest(format_args!(
        "creating workspace_dir: {}",
        request.workspace_dir.display()
    ));
    fs::create_dir_all(&request.workspace_dir)?;
    log_path_metadata("workspace_dir after create", &request.workspace_dir);

    log_ingest(format_args!(
        "creating project_dir: {}",
        project_dir.display()
    ));
    fs::create_dir(&project_dir)?;
    log_path_metadata("project_dir after create", &project_dir);

    log_ingest(format_args!(
        "creating input_source_dir: {}",
        input_source_dir.display()
    ));
    fs::create_dir(&input_source_dir)?;
    log_path_metadata("input_source_dir after create", &input_source_dir);

    extract_archive(&request.archive_path, &input_source_dir)?;
    log_ingest(format_args!("archive extracted"));

    let cmake_source_dir = find_cmake_source_dir(&input_source_dir)?
        .ok_or_else(|| IngestError::MissingCMakeProject(input_source_dir.clone()))?;
    log_ingest(format_args!(
        "selected CMake source dir: {}",
        cmake_source_dir.display()
    ));

    run_cmake(&cmake_source_dir, &build_dir)?;

    let compile_commands_path = build_dir.join("compile_commands.json");
    log_path_metadata("compile_commands_path", &compile_commands_path);
    if !compile_commands_path.is_file() {
        return Err(IngestError::MissingCompileCommands(compile_commands_path));
    }

    let compilation_units = read_compilation_units(&compile_commands_path)?;
    log_ingest(format_args!(
        "compile_commands entries: {}",
        compilation_units.len()
    ));
    if compilation_units.is_empty() {
        return Err(IngestError::EmptyCompileCommands(compile_commands_path));
    }

    log_ingest(format_args!("create_project: success"));

    Ok(CreatedProject {
        name: request.name,
        project_dir,
        input_source_dir,
        cmake_source_dir,
        build_dir,
        compile_commands_path,
        compilation_units,
        type_catalog: Vec::new(),
        type_dependencies: Vec::new(),
    })
}

fn validate_project_name(name: &str) -> Result<(), IngestError> {
    log_ingest(format_args!("validating project name: {:?}", name));
    let valid = !name.trim().is_empty()
        && Path::new(name)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
        && !name.contains(['/', '\\']);

    if valid {
        Ok(())
    } else {
        Err(IngestError::InvalidProjectName(name.to_owned()))
    }
}

fn extract_archive(archive_path: &Path, destination: &Path) -> Result<(), IngestError> {
    log_ingest(format_args!(
        "extract_archive: archive={} destination={}",
        archive_path.display(),
        destination.display()
    ));
    match archive_kind(archive_path) {
        Some(ArchiveKind::TarGz) => {
            log_ingest(format_args!("archive kind: tar.gz"));
            validate_tar_entries(archive_path)?;
            assert_success(
                "tar",
                Command::new("tar")
                    .arg("-xzf")
                    .arg(archive_path)
                    .arg("-C")
                    .arg(destination)
                    .arg("--no-same-owner")
                    .arg("--no-same-permissions")
                    .output()?,
            )
            .map(|_| ())
        }
        Some(ArchiveKind::Zip) => {
            log_ingest(format_args!("archive kind: zip"));
            validate_zip_entries(archive_path)?;
            assert_success(
                "unzip",
                Command::new("unzip")
                    .arg("-q")
                    .arg(archive_path)
                    .arg("-d")
                    .arg(destination)
                    .output()?,
            )
            .map(|_| ())
        }
        None => {
            log_ingest(format_args!("archive kind: unsupported"));
            Err(IngestError::UnsupportedArchive(archive_path.to_path_buf()))
        }
    }
}

fn validate_tar_entries(archive_path: &Path) -> Result<(), IngestError> {
    log_ingest(format_args!(
        "validating tar entries: {}",
        archive_path.display()
    ));
    let output = Command::new("tar").arg("-tzf").arg(archive_path).output()?;
    assert_success("tar", output).and_then(validate_archive_listing)
}

fn validate_zip_entries(archive_path: &Path) -> Result<(), IngestError> {
    log_ingest(format_args!(
        "validating zip entries: {}",
        archive_path.display()
    ));
    let output = Command::new("unzip")
        .arg("-Z1")
        .arg(archive_path)
        .output()?;
    assert_success("unzip", output).and_then(validate_archive_listing)
}

fn validate_archive_listing(listing: String) -> Result<(), IngestError> {
    let mut entry_count = 0usize;
    let mut first_entries = Vec::new();
    for entry in listing.lines().filter(|entry| !entry.trim().is_empty()) {
        entry_count += 1;
        if first_entries.len() < 20 {
            first_entries.push(entry.to_owned());
        }

        let path = Path::new(entry);
        let safe = path
            .components()
            .all(|component| matches!(component, Component::CurDir | Component::Normal(_)));

        if !safe {
            log_ingest(format_args!("unsafe archive entry detected: {entry}"));
            return Err(IngestError::UnsafeArchiveEntry(entry.to_owned()));
        }
    }

    log_ingest(format_args!("archive entry count: {entry_count}"));
    for entry in first_entries {
        log_ingest(format_args!("archive entry sample: {entry}"));
    }

    Ok(())
}

fn run_cmake(source_dir: &Path, build_dir: &Path) -> Result<(), IngestError> {
    log_ingest(format_args!(
        "running cmake: source_dir={} build_dir={}",
        source_dir.display(),
        build_dir.display()
    ));
    assert_success(
        "cmake",
        Command::new("cmake")
            .arg("-S")
            .arg(source_dir)
            .arg("-B")
            .arg(build_dir)
            .arg("-DCMAKE_EXPORT_COMPILE_COMMANDS=ON")
            .output()?,
    )?;

    Ok(())
}

fn find_cmake_source_dir(input_source_dir: &Path) -> Result<Option<PathBuf>, IngestError> {
    log_ingest(format_args!(
        "searching CMakeLists.txt under {}",
        input_source_dir.display()
    ));
    let mut candidates = Vec::new();
    collect_cmake_source_dirs(input_source_dir, &mut candidates)?;
    candidates.sort_by(|left, right| {
        left.components()
            .count()
            .cmp(&right.components().count())
            .then_with(|| left.cmp(right))
    });

    log_ingest(format_args!(
        "CMake source candidates: {}",
        candidates.len()
    ));
    for candidate in &candidates {
        log_ingest(format_args!(
            "CMake source candidate: {}",
            candidate.display()
        ));
    }

    Ok(candidates.into_iter().next())
}

fn collect_cmake_source_dirs(
    directory: &Path,
    candidates: &mut Vec<PathBuf>,
) -> Result<(), IngestError> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    let has_cmake_lists = entries.iter().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name == "CMakeLists.txt")
    });

    if has_cmake_lists {
        log_ingest(format_args!(
            "found CMakeLists.txt in {}",
            directory.display()
        ));
        candidates.push(directory.to_path_buf());
    }

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_cmake_source_dirs(&path, candidates)?;
        }
    }

    Ok(())
}

fn read_compilation_units(path: &Path) -> Result<Vec<CompilationUnit>, IngestError> {
    log_ingest(format_args!("reading compile commands: {}", path.display()));
    #[derive(Deserialize)]
    struct RawCompilationUnit {
        directory: PathBuf,
        file: PathBuf,
        command: Option<String>,
        #[serde(default)]
        arguments: Vec<String>,
    }

    let contents = fs::read_to_string(path)?;
    let units: Vec<RawCompilationUnit> = serde_json::from_str(&contents)?;

    Ok(units
        .into_iter()
        .map(|unit| CompilationUnit {
            directory: unit.directory.display().to_string(),
            file: unit.file.display().to_string(),
            command: unit.command,
            arguments: unit.arguments,
        })
        .collect())
}

fn assert_success(tool: &'static str, output: Output) -> Result<String, IngestError> {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    log_ingest(format_args!(
        "{tool} exited with status {:?}, success={}",
        output.status.code(),
        output.status.success()
    ));
    log_tool_output(tool, "stdout", &stdout);
    log_tool_output(tool, "stderr", &stderr);

    if output.status.success() {
        Ok(stdout)
    } else {
        Err(IngestError::ToolFailed {
            tool,
            status: output.status.code(),
            stderr,
        })
    }
}

#[derive(Clone, Copy)]
enum ArchiveKind {
    TarGz,
    Zip,
}

fn archive_kind(path: &Path) -> Option<ArchiveKind> {
    let name = path.file_name()?.to_str()?.to_ascii_lowercase();

    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(ArchiveKind::TarGz)
    } else if name.ends_with(".zip") {
        Some(ArchiveKind::Zip)
    } else {
        None
    }
}

fn log_ingest(args: fmt::Arguments<'_>) {
    eprintln!("[syntax-bridge][ingest][{}] {args}", timestamp_millis());
}

fn log_tool_output(tool: &str, stream: &str, contents: &str) {
    if contents.is_empty() {
        log_ingest(format_args!("{tool} {stream}: <empty>"));
        return;
    }

    for line in contents.lines() {
        log_ingest(format_args!("{tool} {stream}: {line}"));
    }
}

fn log_path_metadata(label: &str, path: &Path) {
    match fs::metadata(path) {
        Ok(metadata) => {
            log_ingest(format_args!(
                "{label}: path={} exists=true is_file={} is_dir={} len={} readonly={}",
                path.display(),
                metadata.is_file(),
                metadata.is_dir(),
                metadata.len(),
                metadata.permissions().readonly()
            ));
        }
        Err(error) => {
            log_ingest(format_args!(
                "{label}: path={} exists=false metadata_error={error}",
                path.display()
            ));
        }
    }
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
