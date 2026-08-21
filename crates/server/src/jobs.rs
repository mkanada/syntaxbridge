//! In-memory registry of in-flight/completed background jobs — project
//! creation (ingestion) and, since item 2
//! (`docs/prompts/2026-08-19-mudanca-interacao.md`), analysis — so
//! `POST /projects`/`POST /projects/analyse` can start the (potentially
//! minutes-long) `libclang` work in the background and return immediately
//! with a job id, while `GET /projects/jobs/{id}`/`GET /projects/analyse-jobs/{id}`
//! report real progress by reading the same [`crate::progress::ExtractionProgress`]
//! atomics the running job updates — see
//! `crates/server/tests/verovio_5_7_0_import_diagnosis.rs` for why blocking
//! the request on it is not acceptable for a real-world-sized project.
//!
//! [`Job`]/[`JobRegistry`] are generic over the outcome type so ingestion and
//! analysis share this machinery instead of duplicating it — the two kinds
//! never mix (each gets its own [`JobRegistry`] instance in `AppState`), the
//! generic parameter just avoids writing the same struct twice.
//!
//! Jobs are kept in memory only and never evicted — a long-running server
//! accumulates one entry per project created/analysed during its lifetime.
//! Acceptable for now (see `docs/plans/User Steps.md`'s open item on
//! long-running work); revisit if that ever matters in practice.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::ingest::CreatedProject;
use crate::project_service::{AnalyseProjectError, CreationProgress, ProjectCreationError};

/// Where a job stands, derived from live progress rather than stored
/// explicitly, so there's no separate state to keep in sync with the
/// progress counters themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobPhase {
    Ingesting,
    CatalogingTypes,
    DiscoveringSourceFiles,
    CatalogingFunctions,
    CatalogingPointers,
    Persisting,
}

/// Derives the current phase from the four extraction passes' live
/// counters. A `total` of zero means that pass hasn't started yet (see
/// `ExtractionProgress::set_total`).
#[allow(clippy::too_many_arguments)]
pub fn derive_phase(
    type_catalog_completed: usize,
    type_catalog_total: usize,
    source_catalog_completed: usize,
    source_catalog_total: usize,
    function_catalog_completed: usize,
    function_catalog_total: usize,
    pointer_catalog_completed: usize,
    pointer_catalog_total: usize,
) -> JobPhase {
    if type_catalog_total == 0 {
        JobPhase::Ingesting
    } else if type_catalog_completed < type_catalog_total {
        JobPhase::CatalogingTypes
    } else if source_catalog_total == 0 || source_catalog_completed < source_catalog_total {
        JobPhase::DiscoveringSourceFiles
    } else if function_catalog_total == 0 || function_catalog_completed < function_catalog_total {
        JobPhase::CatalogingFunctions
    } else if pointer_catalog_total == 0 || pointer_catalog_completed < pointer_catalog_total {
        JobPhase::CatalogingPointers
    } else {
        JobPhase::Persisting
    }
}

pub struct Job<T, E> {
    pub progress: CreationProgress,
    outcome: Mutex<Option<Result<T, E>>>,
}

impl<T, E> Job<T, E> {
    pub fn new() -> Self {
        Self {
            progress: CreationProgress::default(),
            outcome: Mutex::new(None),
        }
    }

    /// Records the terminal result. Called once, from the background thread
    /// that ran the job's work (`project_service::create_project` or
    /// `project_service::analyse_project`).
    pub fn finish(&self, outcome: Result<T, E>) {
        *self.outcome.lock().unwrap() = Some(outcome);
    }

    /// Requests cancellation (US-4 criterion 7): the background thread
    /// notices the shared flag between compilation units and stops on its
    /// own next check, best-effort rather than immediate. Safe to call after
    /// the job has already finished — it just has no effect, since nothing
    /// reads the flag anymore.
    pub fn cancel(&self) {
        self.progress.cancellation.cancel();
    }

    /// Whether cancellation was requested for this job, regardless of
    /// whether the background thread has actually stopped yet — the
    /// distinction a poller needs to report `"cancelling"` (requested, still
    /// running) separately from `"cancelled"` (`finish`'s outcome was a
    /// cancellation).
    pub fn cancel_requested(&self) -> bool {
        self.progress.cancellation.is_cancelled()
    }

    /// Gives the caller access to the terminal outcome, if any, without
    /// requiring `T`/`E` to be `Clone` (both wrap non-`Clone` I/O and SQLite
    /// errors in practice) — the caller builds whatever it needs (e.g. a
    /// JSON response) inside the closure, while the lock is held.
    pub fn with_outcome<R>(&self, f: impl FnOnce(Option<&Result<T, E>>) -> R) -> R {
        f(self.outcome.lock().unwrap().as_ref())
    }
}

impl<T, E> Default for Job<T, E> {
    fn default() -> Self {
        Self::new()
    }
}

/// A project-creation (ingestion) job — [`Job`] specialized the way
/// `POST /projects` has always used it.
pub type ProjectCreationJob = Job<CreatedProject, ProjectCreationError>;

/// An analysis job (item 2, `docs/prompts/2026-08-19-mudanca-interacao.md`)
/// — [`Job`] specialized for `POST /projects/analyse`. No payload to hand
/// back on success: the client already has the project's identity from
/// ingestion and re-fetches whatever listing it needs (types, functions,
/// pointers, externals) once analysis lands, the same way every other
/// mutation in this server works.
pub type AnalysisJob = Job<(), AnalyseProjectError>;

/// [`JobRegistry`] specialized for [`ProjectCreationJob`]s — what
/// `AppState` holds for `POST /projects`.
pub type ProjectCreationJobRegistry = JobRegistry<CreatedProject, ProjectCreationError>;

/// [`JobRegistry`] specialized for [`AnalysisJob`]s — what `AppState` holds
/// for `POST /projects/analyse`, kept separate from
/// [`ProjectCreationJobRegistry`] so an id from one kind is never confused
/// for the other's.
pub type AnalysisJobRegistry = JobRegistry<(), AnalyseProjectError>;

/// Generates a process-unique job id. Doesn't need to survive a restart —
/// jobs themselves don't (see the module doc) — so a simple in-process
/// counter is enough, no need for a UUID dependency. Shared by every
/// [`JobRegistry`] instance (one counter, not one per job kind), so ids
/// stay unique even though ingestion and analysis jobs live in separate
/// registries.
fn generate_job_id() -> String {
    static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    format!("job-{}", NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed))
}

/// The id-to-handle map a [`JobRegistry`] guards behind its `Mutex` —
/// named so the field below states its own shape instead of nesting four
/// generic layers inline.
type JobMap<T, E> = HashMap<String, Arc<Job<T, E>>>;

pub struct JobRegistry<T, E> {
    jobs: Arc<Mutex<JobMap<T, E>>>,
}

// Written by hand instead of `#[derive(Clone)]`: the derive would require
// `T: Clone, E: Clone` even though cloning this type only ever clones the
// outer `Arc` (a registry handle, shared by every request), never a `T`/`E`
// value itself — a real limitation of derived `Clone` on generic structs,
// not a rule this type actually needs to follow.
impl<T, E> Clone for JobRegistry<T, E> {
    fn clone(&self) -> Self {
        Self {
            jobs: self.jobs.clone(),
        }
    }
}

impl<T, E> JobRegistry<T, E> {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Creates a new job, registers it, and returns both its id and a handle
    /// to it for the caller to run the work against.
    pub fn start(&self) -> (String, Arc<Job<T, E>>) {
        let id = generate_job_id();
        let job = Arc::new(Job::new());
        self.jobs.lock().unwrap().insert(id.clone(), job.clone());
        (id, job)
    }

    pub fn get(&self, id: &str) -> Option<Arc<Job<T, E>>> {
        self.jobs.lock().unwrap().get(id).cloned()
    }
}

impl<T, E> Default for JobRegistry<T, E> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_is_ingesting_before_the_type_catalog_total_is_known() {
        assert_eq!(derive_phase(0, 0, 0, 0, 0, 0, 0, 0), JobPhase::Ingesting);
    }

    #[test]
    fn phase_is_cataloging_types_while_that_pass_is_incomplete() {
        assert_eq!(
            derive_phase(3, 10, 0, 0, 0, 0, 0, 0),
            JobPhase::CatalogingTypes
        );
    }

    #[test]
    fn phase_is_discovering_source_files_once_types_are_done() {
        assert_eq!(
            derive_phase(10, 10, 0, 0, 0, 0, 0, 0),
            JobPhase::DiscoveringSourceFiles
        );
        assert_eq!(
            derive_phase(10, 10, 4, 10, 0, 0, 0, 0),
            JobPhase::DiscoveringSourceFiles
        );
    }

    #[test]
    fn phase_is_cataloging_functions_once_source_files_are_done() {
        assert_eq!(
            derive_phase(10, 10, 10, 10, 0, 0, 0, 0),
            JobPhase::CatalogingFunctions
        );
        assert_eq!(
            derive_phase(10, 10, 10, 10, 4, 10, 0, 0),
            JobPhase::CatalogingFunctions
        );
    }

    #[test]
    fn phase_is_cataloging_pointers_once_functions_are_done() {
        assert_eq!(
            derive_phase(10, 10, 10, 10, 10, 10, 0, 0),
            JobPhase::CatalogingPointers
        );
        assert_eq!(
            derive_phase(10, 10, 10, 10, 10, 10, 4, 10),
            JobPhase::CatalogingPointers
        );
    }

    #[test]
    fn phase_is_persisting_once_all_passes_are_done() {
        assert_eq!(
            derive_phase(10, 10, 10, 10, 10, 10, 10, 10),
            JobPhase::Persisting
        );
    }

    #[test]
    fn registry_starts_a_job_and_finds_it_by_id() {
        let registry = ProjectCreationJobRegistry::new();
        let (id, job) = registry.start();

        assert!(Arc::ptr_eq(
            &job,
            &registry.get(&id).expect("job should be registered")
        ));
    }

    #[test]
    fn unknown_job_id_is_not_found() {
        let registry = ProjectCreationJobRegistry::new();
        assert!(registry.get("does-not-exist").is_none());
    }

    #[test]
    fn job_reports_no_outcome_until_finished() {
        let job = ProjectCreationJob::new();
        job.with_outcome(|outcome| assert!(outcome.is_none()));
    }

    #[test]
    fn job_starts_without_cancellation_requested() {
        let job = ProjectCreationJob::new();
        assert!(!job.cancel_requested());
    }

    #[test]
    fn cancelling_a_job_is_observed_through_cancel_requested() {
        let job = ProjectCreationJob::new();
        job.cancel();
        assert!(job.cancel_requested());
    }

    #[test]
    fn cancelling_an_already_finished_job_is_harmless() {
        let job = ProjectCreationJob::new();
        job.finish(Ok(CreatedProject {
            name: "counter".to_owned(),
            project_dir: "/tmp/counter".into(),
            input_source_dir: "/tmp/counter/input-source".into(),
            cmake_source_dir: "/tmp/counter/input-source".into(),
            build_dir: "/tmp/counter/build".into(),
            compile_commands_path: "/tmp/counter/build/compile_commands.json".into(),
            compilation_units: Vec::new(),
            type_catalog: Vec::new(),
            type_dependencies: Vec::new(),
            source_files: Vec::new(),
            pointer_catalog: Vec::new(),
            is_analysed: false,
        }));

        job.cancel();

        job.with_outcome(|outcome| match outcome {
            Some(Ok(project)) => assert_eq!(project.name, "counter"),
            other => panic!("expected the original successful outcome, got {other:?}"),
        });
    }

    #[test]
    fn job_reports_the_outcome_once_finished() {
        let job = ProjectCreationJob::new();
        let project = CreatedProject {
            name: "counter".to_owned(),
            project_dir: "/tmp/counter".into(),
            input_source_dir: "/tmp/counter/input-source".into(),
            cmake_source_dir: "/tmp/counter/input-source".into(),
            build_dir: "/tmp/counter/build".into(),
            compile_commands_path: "/tmp/counter/build/compile_commands.json".into(),
            compilation_units: Vec::new(),
            type_catalog: Vec::new(),
            type_dependencies: Vec::new(),
            source_files: Vec::new(),
            pointer_catalog: Vec::new(),
            is_analysed: false,
        };

        job.finish(Ok(project));

        job.with_outcome(|outcome| match outcome {
            Some(Ok(project)) => assert_eq!(project.name, "counter"),
            other => panic!("expected a successful outcome, got {other:?}"),
        });
    }
}
