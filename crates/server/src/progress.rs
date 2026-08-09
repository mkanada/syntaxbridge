//! Tracks how far a `libclang`-based extraction pass (`type_catalog` or
//! `source_catalog`) has gotten, so `GET /projects/jobs/{id}` (`jobs.rs`) can
//! report real progress while `POST /projects` runs the extraction in the
//! background instead of blocking the HTTP request on it.

use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug, Default)]
pub struct ExtractionProgress {
    completed: AtomicUsize,
    total: AtomicUsize,
}

impl ExtractionProgress {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set once the number of compilation units to process is known. A
    /// `total` of zero means the extraction hasn't started yet, which a
    /// poller distinguishes from "in progress" or "done".
    pub fn set_total(&self, total: usize) {
        self.total.store(total, Ordering::Relaxed);
    }

    pub fn mark_one_done(&self) {
        self.completed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn completed(&self) -> usize {
        self.completed.load(Ordering::Relaxed)
    }

    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero_of_zero() {
        let progress = ExtractionProgress::new();
        assert_eq!(progress.completed(), 0);
        assert_eq!(progress.total(), 0);
    }

    #[test]
    fn tracks_total_and_completed_independently() {
        let progress = ExtractionProgress::new();
        progress.set_total(5);
        progress.mark_one_done();
        progress.mark_one_done();

        assert_eq!(progress.total(), 5);
        assert_eq!(progress.completed(), 2);
    }
}
