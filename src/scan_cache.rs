//! Shared scan caches and throttled progress emission.

use nix::unistd::Uid;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const PATCH_EMIT_INTERVAL: Duration = Duration::from_millis(300);
const ERROR_EMIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub struct OwnerCache {
    inner: Mutex<HashMap<u32, String>>,
}

impl Default for OwnerCache {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

impl OwnerCache {
    pub fn lookup(&self, uid: u32) -> String {
        if let Ok(guard) = self.inner.lock() {
            if let Some(name) = guard.get(&uid) {
                return name.clone();
            }
        }
        let name = nix::unistd::User::from_uid(Uid::from_raw(uid))
            .ok()
            .flatten()
            .map(|u| u.name)
            .unwrap_or_else(|| "unknown".to_string());
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(uid, name.clone());
        }
        name
    }

    pub fn owner_from_metadata(meta: &fs::Metadata, cache: &Self) -> String {
        cache.lookup(meta.uid())
    }
}

/// Lock-free throttle for scan progress emission.
///
/// Worker threads accumulate item/byte/dir counts into atomics with no locking,
/// and a single thread per ~100ms window wins an emit slot via compare-and-swap.
/// This keeps the hot path (one call per file and per directory) off the shared
/// `Mutex<ScanProgress>`, which is the difference between scaling with cores and
/// serializing on a global lock once many rayon workers are scanning at once.
#[derive(Debug)]
pub struct ProgressThrottle {
    base: Instant,
    last_emit_ms: AtomicU64,
    emits_done: AtomicU64,
    pending_items: AtomicU64,
    pending_bytes: AtomicU64,
    pending_dirs: AtomicU64,
}

const PROGRESS_EMIT_INTERVAL_MS: u64 = 100;

/// Number of emits allowed through immediately at the start of a scan, before
/// time-based throttling kicks in. This makes progress appear the instant a scan
/// starts (rather than after a 100ms blackout) and guarantees that scans which
/// finish in under one interval still report intermediate state.
const BOOTSTRAP_EMITS: u64 = 64;

impl Default for ProgressThrottle {
    fn default() -> Self {
        Self {
            base: Instant::now(),
            last_emit_ms: AtomicU64::new(0),
            emits_done: AtomicU64::new(0),
            pending_items: AtomicU64::new(0),
            pending_bytes: AtomicU64::new(0),
            pending_dirs: AtomicU64::new(0),
        }
    }
}

impl ProgressThrottle {
    pub fn record_file(&self, size: u64) {
        self.pending_items.fetch_add(1, Ordering::Relaxed);
        self.pending_bytes.fetch_add(size, Ordering::Relaxed);
    }

    pub fn record_dir(&self) {
        self.pending_dirs.fetch_add(1, Ordering::Relaxed);
    }

    /// Returns `true` for exactly one caller per emit interval (or per bootstrap
    /// slot). The winner takes the slot via CAS so a burst of workers crossing
    /// the boundary together produces a single emit rather than a thundering
    /// herd.
    pub fn acquire_emit_slot(&self) -> bool {
        let now_ms = self.base.elapsed().as_millis() as u64;
        let last = self.last_emit_ms.load(Ordering::Relaxed);
        let time_due = now_ms.saturating_sub(last) >= PROGRESS_EMIT_INTERVAL_MS;
        if !time_due && self.emits_done.load(Ordering::Relaxed) >= BOOTSTRAP_EMITS {
            return false;
        }
        if self
            .last_emit_ms
            .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            self.emits_done.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    pub fn take_pending(&self) -> (u64, u64, u64) {
        let items = self.pending_items.swap(0, Ordering::Relaxed);
        let bytes = self.pending_bytes.swap(0, Ordering::Relaxed);
        let dirs = self.pending_dirs.swap(0, Ordering::Relaxed);
        (items, bytes, dirs)
    }
}

#[derive(Debug)]
pub struct PatchThrottle {
    last_emit: Mutex<Instant>,
}

impl Default for PatchThrottle {
    fn default() -> Self {
        Self {
            last_emit: Mutex::new(Instant::now() - PATCH_EMIT_INTERVAL),
        }
    }
}

impl PatchThrottle {
    pub fn should_emit(&self, depth: usize, _child_count: usize) -> bool {
        // Always surface the top two levels immediately so the tree paints fast.
        // Deeper directories are time-throttled regardless of size: emitting a
        // patch per directory (most have few children) floods the unbounded
        // channel far faster than the UI can drain and merge it, which is what
        // makes a large scan feel frozen.
        if depth <= 1 {
            return true;
        }
        let Ok(guard) = self.last_emit.lock() else {
            return true;
        };
        guard.elapsed() >= PATCH_EMIT_INTERVAL
    }

    pub fn mark_emitted(&self) {
        if let Ok(mut guard) = self.last_emit.lock() {
            *guard = Instant::now();
        }
    }
}

/// Aggregates noisy scan errors (permission denials on `/`, pseudo-fs paths).
#[derive(Debug)]
pub struct ErrorPolicy {
    last_emit: Mutex<Instant>,
    permission_denied: AtomicUsize,
}

impl Default for ErrorPolicy {
    fn default() -> Self {
        Self {
            last_emit: Mutex::new(Instant::now() - ERROR_EMIT_INTERVAL),
            permission_denied: AtomicUsize::new(0),
        }
    }
}

impl ErrorPolicy {
    pub fn should_skip_path(path: &Path) -> bool {
        let s = path.to_string_lossy();
        s.starts_with("/dev/") || s == "/dev"
    }

    pub fn record(
        &self,
        progress: &mut crate::models::ScanProgress,
        path: &Path,
        message: String,
    ) -> bool {
        if Self::should_skip_path(path) {
            return false;
        }
        let is_permission = message.to_lowercase().contains("permission denied");
        if is_permission {
            let n = self.permission_denied.fetch_add(1, Ordering::Relaxed) + 1;
            if n == 1 {
                progress.push_error(message);
            } else if n == 2 {
                progress.push_error(
                    "Permission denied on additional protected paths (further denials suppressed)"
                        .to_string(),
                );
            }
        } else {
            progress.push_error(message);
        }
        let Ok(guard) = self.last_emit.lock() else {
            return true;
        };
        if guard.elapsed() >= ERROR_EMIT_INTERVAL {
            drop(guard);
            self.mark_emitted();
            true
        } else {
            false
        }
    }

    pub fn record_io(
        &self,
        progress: &mut crate::models::ScanProgress,
        path: &Path,
        context: &str,
        err: &io::Error,
    ) -> bool {
        if Self::should_skip_path(path) {
            return false;
        }
        let message = if err.kind() == io::ErrorKind::PermissionDenied {
            format!("Permission denied: {} ({err})", path.display())
        } else {
            format!("{context} {}: {err}", path.display())
        };
        self.record(progress, path, message)
    }

    fn mark_emitted(&self) {
        if let Ok(mut guard) = self.last_emit.lock() {
            *guard = Instant::now();
        }
    }
}

/// Apply one `symlink_metadata` result to a node (size, allocated, symlink fields).
pub fn apply_file_metadata(node: &mut crate::models::ScanNode, meta: &fs::Metadata) {
    node.is_symlink = meta.file_type().is_symlink();
    if node.is_symlink {
        node.symlink_target = fs::read_link(&node.path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        node.size = 0;
        node.allocated = 0;
    } else {
        node.size = meta.size();
        node.allocated = meta.blocks() * 512;
        node.file_count = 1;
    }
}

/// Fill deferred owner/mtime/extension (single stat).
pub fn fill_node_metadata(node: &mut crate::models::ScanNode) {
    if node.metadata_loaded {
        return;
    }
    let Ok(meta) = fs::symlink_metadata(&node.path) else {
        return;
    };
    let cache = OwnerCache::default();
    node.owner = OwnerCache::owner_from_metadata(&meta, &cache);
    node.mtime = meta.mtime() as f64;
    if !node.is_dir {
        node.extension = extension_from_name(&node.name);
        if !node.is_symlink {
            node.size = meta.size();
            node.allocated = meta.blocks() * 512;
        }
    }
    node.metadata_loaded = true;
}

/// File extension for display/export (no extra I/O).
pub fn get_file_extension(name: &str) -> String {
    extension_from_name(name)
}

fn extension_from_name(name: &str) -> String {
    if name.starts_with('.') && name.matches('.').count() == 1 {
        return name[1..].to_lowercase();
    }
    std::path::Path::new(name)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_cache_reuses_lookup() {
        let cache = OwnerCache::default();
        let first = cache.lookup(0);
        let second = cache.lookup(0);
        assert_eq!(first, second);
    }

    #[test]
    fn error_policy_suppresses_repeated_permission_denials() {
        use crate::models::ScanProgress;

        let policy = ErrorPolicy::default();
        let mut progress = ScanProgress::default();
        let path = Path::new("/private/var/root");
        policy.record(
            &mut progress,
            path,
            "Permission denied: /private/var/root (os error 13)".to_string(),
        );
        policy.record(
            &mut progress,
            path,
            "Permission denied: /private/var/spool (os error 13)".to_string(),
        );
        policy.record(
            &mut progress,
            path,
            "Permission denied: /private/var/audit (os error 13)".to_string(),
        );
        assert_eq!(progress.errors.len(), 2);
        assert!(progress.errors[1].contains("suppressed"));
    }

    #[test]
    fn error_policy_skips_dev_paths() {
        use crate::models::ScanProgress;

        let policy = ErrorPolicy::default();
        let mut progress = ScanProgress::default();
        let emit = policy.record_io(
            &mut progress,
            Path::new("/dev/fd"),
            "Cannot read directory",
            &io::Error::from_raw_os_error(9),
        );
        assert!(!emit);
        assert!(progress.errors.is_empty());
    }

    #[test]
    fn progress_throttle_batches_counters() {
        let throttle = ProgressThrottle::default();
        throttle.record_file(100);
        throttle.record_file(50);
        throttle.record_dir();
        let (items, bytes, dirs) = throttle.take_pending();
        assert_eq!(items, 2);
        assert_eq!(bytes, 150);
        assert_eq!(dirs, 1);
    }
}
