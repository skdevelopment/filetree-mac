//! Progress mutex helpers and throttled emission for scan workers.

use crate::models::{PatchKind, ProgressSnapshot, ScanNode, ScanProgress, TreePatch};
use crate::scan_cache::{ErrorPolicy, PatchThrottle, ProgressThrottle};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

pub type ProgressCallback = Arc<dyn Fn(ProgressSnapshot) + Send + Sync>;
pub type PatchCallback = Arc<dyn Fn(TreePatch) + Send + Sync>;

/// Shared scan session state passed through directory traversal.
#[derive(Clone)]
pub(crate) struct ScanContext {
    pub progress: Arc<Mutex<ScanProgress>>,
    pub scan_root: PathBuf,
    pub on_progress: Option<ProgressCallback>,
    pub on_patch: Option<PatchCallback>,
    /// Directories already entered, keyed by filesystem identity `(dev, ino)`.
    /// Guards against traversal cycles — symlink loops *and* macOS firmlink /
    /// mount loops, which are real directories that recur even when symlinks are
    /// not followed (e.g. `/System/Volumes/Data/System/Volumes/Data/...`).
    pub seen_dirs: Arc<Mutex<HashSet<(u64, u64)>>>,
    /// macOS cloud File Provider roots not to descend into for this scan (empty
    /// when the scan explicitly targets cloud storage). See `scan_traverse`.
    pub cloud_skip_roots: Vec<PathBuf>,
    pub cancel: Arc<AtomicBool>,
    pub follow_symlinks: bool,
    pub show_hidden: bool,
    pub max_workers: usize,
    pub progress_throttle: Arc<ProgressThrottle>,
    pub patch_throttle: Arc<PatchThrottle>,
    pub error_policy: Arc<ErrorPolicy>,
}

pub(crate) fn lock_progress_snapshot(m: &Arc<Mutex<ScanProgress>>) -> ProgressSnapshot {
    m.lock().unwrap_or_else(|e| e.into_inner()).snapshot()
}

pub(crate) fn lock_progress(m: &Arc<Mutex<ScanProgress>>) -> ScanProgress {
    m.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub(crate) fn lock_progress_mut<'a>(
    m: &'a Arc<Mutex<ScanProgress>>,
    cancel: &Arc<AtomicBool>,
) -> Option<MutexGuard<'a, ScanProgress>> {
    match m.lock() {
        Ok(guard) => Some(guard),
        Err(e) => {
            cancel.store(true, Ordering::SeqCst);
            let mut guard = e.into_inner();
            guard.push_error("Scan progress state corrupted (worker panic)".to_string());
            guard.cancelled = true;
            None
        }
    }
}

pub(crate) fn mark_cancelled(ctx: &ScanContext) {
    if let Some(mut p) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
        p.cancelled = true;
    }
    flush_progress_counters(ctx);
    emit_progress(ctx);
}

pub(crate) fn emit_progress(ctx: &ScanContext) {
    if ctx.cancel.load(Ordering::SeqCst) {
        if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
            progress.cancelled = true;
        }
    }
    if let Some(ref cb) = ctx.on_progress {
        cb(lock_progress_snapshot(&ctx.progress));
    }
}

pub(crate) fn flush_progress_counters(ctx: &ScanContext) {
    let (items, bytes, dirs) = ctx.progress_throttle.take_pending();
    if items > 0 || bytes > 0 || dirs > 0 {
        if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
            progress.scanned_items += items;
            progress.bytes_scanned += bytes;
            progress.scanned_dirs += dirs;
        }
    }
}

pub(crate) fn maybe_emit_progress(ctx: &ScanContext, force: bool) {
    if force || ctx.progress_throttle.acquire_emit_slot() {
        flush_progress_counters(ctx);
        emit_progress(ctx);
    }
}

/// Throttled progress emit that also updates the "currently scanning" path.
/// The shared `ScanProgress` lock is only touched when this caller wins the emit
/// slot (~10×/sec), instead of once per directory.
pub(crate) fn maybe_emit_progress_path(ctx: &ScanContext, path: &std::path::Path) {
    if ctx.progress_throttle.acquire_emit_slot() {
        flush_progress_counters(ctx);
        if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
            progress.current_path = path.to_path_buf();
        }
        emit_progress(ctx);
    }
}

pub(crate) fn record_error(ctx: &ScanContext, path: &std::path::Path, message: String) {
    let should_emit = if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
        ctx.error_policy.record(&mut progress, path, message)
    } else {
        false
    };
    if should_emit {
        emit_progress(ctx);
    }
}

pub(crate) fn emit_patch(node: &ScanNode, ctx: &ScanContext, kind: PatchKind) {
    if let Some(ref cb) = ctx.on_patch {
        let patch = match kind {
            PatchKind::Listed => node.listing_patch(),
            PatchKind::Subtree => node.subtree_patch(),
        };
        cb(patch);
    }
}

pub(crate) fn maybe_emit_listing_patch(node: &ScanNode, ctx: &ScanContext, depth: usize) {
    if ctx.patch_throttle.should_emit(depth, node.children.len()) {
        emit_patch(node, ctx, PatchKind::Listed);
        ctx.patch_throttle.mark_emitted();
    }
}
