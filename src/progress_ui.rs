//! Scan progress display state for the TUI status bar and progress panel.

use crate::models::{ProgressSnapshot, ScanProgress};
use crate::progress::scan_panel_cancelled;
use crate::scanner::format_bytes;

#[derive(Debug, Clone, Default)]
pub struct ProgressDisplay {
    pub bytes_scanned: u64,
    pub scanned_items: u64,
    pub scanned_dirs: u64,
    pub error_count: usize,
    pub worker_cancelled: bool,
    pub current_path: String,
}

impl ProgressDisplay {
    pub fn update_from_snapshot(&mut self, snapshot: &ProgressSnapshot) {
        self.bytes_scanned = snapshot.bytes_scanned;
        self.scanned_items = snapshot.scanned_items;
        self.scanned_dirs = snapshot.scanned_dirs;
        self.error_count = snapshot.error_count;
        self.worker_cancelled = snapshot.cancelled;
        self.current_path = snapshot.current_path.display().to_string();
    }

    pub fn status_line(&self, cancel_requested: bool) -> String {
        if scan_panel_cancelled(cancel_requested, self.worker_cancelled) {
            "Cancelling scan…".to_string()
        } else {
            let mut status = format!(
                "Scanning… {} items | {} | {} dirs",
                self.scanned_items,
                format_bytes(self.bytes_scanned as i64),
                self.scanned_dirs
            );
            if self.error_count > 0 {
                status.push_str(&format!(" | {} errors", self.error_count));
            }
            status
        }
    }
}

/// Reconstruct full progress for finalize/error modals from a lightweight snapshot.
pub fn snapshot_to_scan_progress(snapshot: &ProgressSnapshot) -> ScanProgress {
    let mut progress = ScanProgress {
        scanned_items: snapshot.scanned_items,
        scanned_dirs: snapshot.scanned_dirs,
        current_path: snapshot.current_path.clone(),
        bytes_scanned: snapshot.bytes_scanned,
        is_complete: snapshot.is_complete,
        cancelled: snapshot.cancelled,
        error: snapshot.first_error.clone(),
        errors: Vec::new(),
    };
    if let Some(err) = snapshot.first_error.clone() {
        progress.errors.push(err);
    }
    progress
}
