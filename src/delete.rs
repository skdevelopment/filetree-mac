//! Background recursive delete with live progress reporting.
//!
//! `std::fs::remove_dir_all` deletes a whole tree in one opaque, blocking call —
//! on the large directories that trigger the typed-delete confirmation that
//! freezes the UI for seconds with no feedback. Instead we walk the tree
//! ourselves on a worker thread, removing one entry at a time and publishing the
//! current path and a running count into a shared [`DeleteProgress`] that the UI
//! thread polls each tick.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Shared progress for an in-flight delete. The worker thread updates it; the UI
/// thread reads it once per tick. Counters are atomics; the current path is a
/// short-held mutex (updated once per entry, never in a tight inner loop).
#[derive(Debug)]
pub struct DeleteProgress {
    items_deleted: AtomicU64,
    /// Best-effort estimate of the total entries to remove (files + folders +
    /// the target itself), taken from the scan tree. Used only for the bar; the
    /// real count can differ if the tree is stale, so the bar is clamped.
    pub total_items: u64,
    done: AtomicBool,
    cancel: AtomicBool,
    current: Mutex<PathBuf>,
    error: Mutex<Option<String>>,
}

impl DeleteProgress {
    pub fn new(total_items: u64, first: PathBuf) -> Self {
        Self {
            items_deleted: AtomicU64::new(0),
            total_items: total_items.max(1),
            done: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            current: Mutex::new(first),
            error: Mutex::new(None),
        }
    }

    pub fn items_deleted(&self) -> u64 {
        self.items_deleted.load(Ordering::Relaxed)
    }
    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::Relaxed)
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn current_path(&self) -> PathBuf {
        self.current
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }
    pub fn take_error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|e| e.into_inner()).take()
    }

    fn set_current(&self, path: &Path) {
        if let Ok(mut cur) = self.current.lock() {
            *cur = path.to_path_buf();
        }
    }
    fn record_removed(&self) {
        self.items_deleted.fetch_add(1, Ordering::Relaxed);
    }
    fn set_error(&self, msg: String) {
        if let Ok(mut slot) = self.error.lock() {
            if slot.is_none() {
                *slot = Some(msg);
            }
        }
    }
}

/// Entry point for the worker thread: delete `target`, then flag completion.
pub fn run_delete(target: PathBuf, is_dir: bool, progress: Arc<DeleteProgress>) {
    let result = if is_dir {
        delete_dir_recursive(&target, &progress)
    } else {
        delete_one(&target, &progress)
    };
    if let Err(e) = result {
        progress.set_error(e);
    }
    progress.done.store(true, Ordering::Relaxed);
}

fn delete_one(path: &Path, progress: &Arc<DeleteProgress>) -> Result<(), String> {
    progress.set_current(path);
    std::fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
    progress.record_removed();
    Ok(())
}

fn delete_dir_recursive(dir: &Path, progress: &Arc<DeleteProgress>) -> Result<(), String> {
    if progress.is_cancelled() {
        return Err("cancelled".to_string());
    }
    progress.set_current(dir);

    let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries {
        if progress.is_cancelled() {
            return Err("cancelled".to_string());
        }
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = entry.path();
        // `symlink_metadata` does not follow links, so a symlinked directory is
        // removed as a single entry (the link) rather than descended into — we
        // never delete outside the target tree.
        let meta =
            std::fs::symlink_metadata(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        if meta.is_dir() {
            delete_dir_recursive(&path, progress)?;
        } else {
            delete_one(&path, progress)?;
        }
    }

    // Children gone — remove the now-empty directory itself (post-order).
    progress.set_current(dir);
    std::fs::remove_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    progress.record_removed();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn unique_tmp(tag: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!("filetree_del_{tag}_{}", std::process::id()));
        p
    }

    #[test]
    fn deletes_tree_and_counts_every_entry() {
        let root = unique_tmp("tree");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub/inner")).unwrap();
        fs::write(root.join("a.txt"), b"a").unwrap();
        fs::write(root.join("sub/b.txt"), b"bb").unwrap();
        fs::write(root.join("sub/inner/c.txt"), b"ccc").unwrap();
        // entries: a.txt, sub, inner, b.txt, c.txt, root = 6
        let progress = Arc::new(DeleteProgress::new(6, root.clone()));
        run_delete(root.clone(), true, progress.clone());

        assert!(progress.is_done());
        assert!(progress.take_error().is_none());
        assert_eq!(progress.items_deleted(), 6);
        assert!(!root.exists());
    }

    #[test]
    fn deletes_single_file() {
        let dir = unique_tmp("file");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("solo.bin");
        fs::write(&file, b"x").unwrap();

        let progress = Arc::new(DeleteProgress::new(1, file.clone()));
        run_delete(file.clone(), false, progress.clone());

        assert!(progress.is_done());
        assert_eq!(progress.items_deleted(), 1);
        assert!(!file.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_target_reports_error_and_finishes() {
        let missing = unique_tmp("missing").join("nope");
        let progress = Arc::new(DeleteProgress::new(1, missing.clone()));
        run_delete(missing, false, progress.clone());
        assert!(progress.is_done());
        assert!(progress.take_error().is_some());
    }
}
