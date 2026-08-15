//! In-memory registry of in-flight/completed project-creation jobs, so
//! `POST /projects` can start the (potentially minutes-long) ingest plus
//! `libclang` extraction in the background and return immediately with a job
//! id, while `GET /projects/jobs/{id}` reports real progress by reading the
//! same [`crate::progress::ExtractionProgress`] atomics
//! `project_service::create_project` updates as it runs — see
//! `crates/server/tests/verovio_5_7_0_import_diagnosis.rs` for why blocking
//! the request on it is not acceptable for a real-world-sized project.
//!
//! Jobs are kept in memory only and never evicted — a long-running server
//! accumulates one entry per project created during its lifetime. Acceptable
//! for now (see `docs/plans/User Steps.md`'s open item on long-running
//! work); revisit if that ever matters in practice.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::ingest::CreatedProject;
use crate::project_service::{CreationProgress, ProjectCreationError};

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

pub struct ProjectCreationJob {
    pub progress: CreationProgress,
    outcome: Mutex<Option<Result<CreatedProject, ProjectCreationError>>>,
}

impl ProjectCreationJob {
    pub fn new() -> Self {
        Self {
            progress: CreationProgress::default(),
            outcome: Mutex::new(None),
        }
    }

    /// Records the terminal result. Called once, from the background thread
    /// that ran `project_service::create_project`.
    pub fn finish(&self, outcome: Result<CreatedProject, ProjectCreationError>) {
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
    /// requiring `CreatedProject`/`ProjectCreationError` to be `Clone` (the
    /// latter wraps non-`Clone` I/O and SQLite errors) — the caller builds
    /// whatever it needs (e.g. a JSON response) inside the closure, while
    /// the lock is held.
    pub fn with_outcome<R>(
        &self,
        f: impl FnOnce(Option<&Result<CreatedProject, ProjectCreationError>>) -> R,
    ) -> R {
        f(self.outcome.lock().unwrap().as_ref())
    }
}

impl Default for ProjectCreationJob {
    fn default() -> Self {
        Self::new()
    }
}

/// Generates a process-unique job id. Doesn't need to survive a restart —
/// jobs themselves don't (see the module doc) — so a simple in-process
/// counter is enough, no need for a UUID dependency.
fn generate_job_id() -> String {
    static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    format!("job-{}", NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed))
}

#[derive(Clone, Default)]
pub struct JobRegistry {
    jobs: Arc<Mutex<HashMap<String, Arc<ProjectCreationJob>>>>,
}

impl JobRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a new job, registers it, and returns both its id and a handle
    /// to it for the caller to run the work against.
    pub fn start(&self) -> (String, Arc<ProjectCreationJob>) {
        let id = generate_job_id();
        let job = Arc::new(ProjectCreationJob::new());
        self.jobs.lock().unwrap().insert(id.clone(), job.clone());
        (id, job)
    }

    pub fn get(&self, id: &str) -> Option<Arc<ProjectCreationJob>> {
        self.jobs.lock().unwrap().get(id).cloned()
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
        let registry = JobRegistry::new();
        let (id, job) = registry.start();

        assert!(Arc::ptr_eq(
            &job,
            &registry.get(&id).expect("job should be registered")
        ));
    }

    #[test]
    fn unknown_job_id_is_not_found() {
        let registry = JobRegistry::new();
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
        };

        job.finish(Ok(project));

        job.with_outcome(|outcome| match outcome {
            Some(Ok(project)) => assert_eq!(project.name, "counter"),
            other => panic!("expected a successful outcome, got {other:?}"),
        });
    }
}
