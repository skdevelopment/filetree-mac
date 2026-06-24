use crate::models::{ExtensionStats, ScanNode, ScanProgress, VolumeInfo};
use crate::scan_cache::{ErrorPolicy, OwnerCache, PatchThrottle, ProgressThrottle};
use crate::scan_progress::{
    emit_progress, flush_progress_counters, lock_progress, lock_progress_mut, maybe_emit_progress,
    ScanContext,
};
use crate::scan_traverse::{rollup_node, scan_directory, scan_file};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

pub use crate::scan_cache::{fill_node_metadata, get_file_extension};
pub use crate::scan_progress::{PatchCallback, ProgressCallback};

pub fn format_bytes(size: i64) -> String {
    let size = if size < 0 { 0 } else { size as u64 };
    if size == 0 {
        return "0 B".to_string();
    }
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = size as f64;
    for (i, unit) in UNITS.iter().enumerate() {
        if value < 1024.0 || i == UNITS.len() - 1 {
            if *unit == "B" {
                return format!("{size} B");
            }
            return format!("{value:.1} {unit}");
        }
        value /= 1024.0;
    }
    format!("{size} B")
}

pub fn get_owner(path: &Path) -> String {
    match fs::symlink_metadata(path) {
        Ok(meta) => OwnerCache::owner_from_metadata(&meta, &OwnerCache::default()),
        Err(_) => "unknown".to_string(),
    }
}

pub fn get_allocated_size(path: &Path, follow_symlinks: bool) -> u64 {
    let meta = if follow_symlinks {
        fs::metadata(path)
    } else {
        fs::symlink_metadata(path)
    };
    match meta {
        Ok(m) => m.blocks() * 512,
        Err(_) => 0,
    }
}

pub fn parse_df_line(line: &str) -> Option<(String, u64, u64, u64, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 6 {
        return None;
    }
    let filesystem = parts[0].to_string();
    let blocks: u64 = parts[1].parse().ok()?;
    let used: u64 = parts[2].parse().ok()?;
    let available: u64 = parts[3].parse().ok()?;
    let mount = parts[5..].join(" ");
    Some((filesystem, blocks, used, available, mount))
}

pub fn volume_bytes_for_path(path: &Path) -> Option<u64> {
    nix::sys::statvfs::statvfs(path)
        .ok()
        .map(|usage| usage.blocks() as u64 * usage.fragment_size())
}

pub fn is_volume_mount_root(path: &Path) -> bool {
    let normalized = crate::paths::normalize_path(path);
    if normalized == Path::new("/") {
        return true;
    }
    list_volumes()
        .iter()
        .any(|v| crate::paths::normalize_path(&v.mount_point) == normalized)
}

pub fn volume_total_for_full_scan(scan_root: &Path) -> Option<u64> {
    if is_volume_mount_root(scan_root) {
        volume_bytes_for_path(scan_root)
    } else {
        None
    }
}

pub fn list_volumes() -> Vec<VolumeInfo> {
    let mut volumes = Vec::new();
    match Command::new("df").args(["-Pk"]).output() {
        Ok(output) if output.status.success() => {
            volumes = parse_volumes_from_df(&String::from_utf8_lossy(&output.stdout));
        }
        _ => {
            if let Ok(usage) = nix::sys::statvfs::statvfs("/") {
                let total = usage.blocks() as u64 * usage.fragment_size();
                let free = usage.blocks_available() as u64 * usage.fragment_size();
                let used = total.saturating_sub(free);
                volumes.push(VolumeInfo {
                    name: "Macintosh HD".to_string(),
                    mount_point: PathBuf::from("/"),
                    total_bytes: total,
                    used_bytes: used,
                    free_bytes: free,
                });
            }
        }
    }
    volumes
}

pub struct DirectoryScanner {
    pub follow_symlinks: bool,
    pub show_hidden: bool,
    pub max_workers: usize,
    pub scan_root: PathBuf,
    pub cancel: Arc<AtomicBool>,
}

impl Default for DirectoryScanner {
    fn default() -> Self {
        let cpus = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self {
            follow_symlinks: false,
            show_hidden: false,
            max_workers: (cpus * 2).min(32),
            scan_root: PathBuf::new(),
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl DirectoryScanner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn reset_cancel(&self) {
        self.cancel.store(false, Ordering::SeqCst);
    }

    fn make_context(
        &self,
        current_path: PathBuf,
        scan_root: PathBuf,
        on_progress: Option<ProgressCallback>,
        on_patch: Option<PatchCallback>,
    ) -> ScanContext {
        let cloud_skip_roots = crate::scan_traverse::cloud_skip_roots_for(&scan_root);
        ScanContext {
            progress: Arc::new(Mutex::new(ScanProgress {
                current_path,
                ..Default::default()
            })),
            scan_root,
            on_progress,
            on_patch,
            seen_dirs: Arc::new(Mutex::new(HashSet::new())),
            cloud_skip_roots,
            cancel: self.cancel.clone(),
            follow_symlinks: self.follow_symlinks,
            show_hidden: self.show_hidden,
            max_workers: self.max_workers,
            progress_throttle: Arc::new(ProgressThrottle::default()),
            patch_throttle: Arc::new(PatchThrottle::default()),
            error_policy: Arc::new(ErrorPolicy::default()),
        }
    }

    /// Run a traversal closure on a dedicated rayon work-stealing pool sized to
    /// `max_workers`. Nested `par_iter_mut` calls inside the traversal steal from
    /// this same pool, so the entire tree is scanned in parallel without the
    /// coordinator-blocking deadlock a fixed pool would hit. Falls back to the
    /// caller thread (rayon's global pool) if a dedicated pool cannot be built.
    fn install_scan<F: FnOnce() + Send>(&self, op: F) {
        match rayon::ThreadPoolBuilder::new()
            .num_threads(self.max_workers.max(1))
            .thread_name(|i| format!("filetree-scan-{i}"))
            .build()
        {
            Ok(pool) => pool.install(op),
            Err(_) => op(),
        }
    }

    pub fn scan(
        &mut self,
        root_path: &Path,
        on_progress: Option<ProgressCallback>,
        on_patch: Option<PatchCallback>,
    ) -> (ScanNode, ScanProgress) {
        self.cancel.store(false, Ordering::SeqCst);
        let root_path = expand_abs(root_path);
        self.scan_root = if root_path.exists() {
            std::fs::canonicalize(&root_path).unwrap_or(root_path.clone())
        } else {
            root_path.clone()
        };

        let name = root_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| root_path.to_string_lossy().to_string());

        let mut root = ScanNode::new(name, root_path.clone(), true);
        let ctx = self.make_context(
            root_path.clone(),
            self.scan_root.clone(),
            on_progress,
            on_patch,
        );

        if !root_path.exists() {
            if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
                progress.push_error(format!("Path does not exist: {}", root_path.display()));
                progress.is_complete = true;
            }
            root.scan_complete = true;
            emit_progress(&ctx);
            return (root, lock_progress(&ctx.progress));
        }

        if !root_path.is_dir() {
            root.is_dir = false;
            scan_file(&mut root, &ctx);
            root.scan_complete = true;
            if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
                progress.is_complete = true;
            }
            emit_progress(&ctx);
            return (root, lock_progress(&ctx.progress));
        }

        self.install_scan(|| scan_directory(&mut root, &ctx, 0));

        flush_progress_counters(&ctx);
        if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
            if ctx.cancel.load(Ordering::SeqCst) {
                progress.cancelled = true;
            }
            root.scan_complete = !progress.cancelled;
            progress.is_complete = true;
            progress.current_path = root_path.clone();
        }
        emit_progress(&ctx);
        // No terminal whole-tree patch: the caller delivers the finished tree via
        // the Complete message (installed with `set_root`, a move), so streaming a
        // full-tree Subtree patch here would just force a redundant — and, for a
        // large tree, very expensive — merge on the UI thread right before it.
        (root, lock_progress(&ctx.progress))
    }

    pub fn rescan_subtree(
        &mut self,
        node: &mut ScanNode,
        on_progress: Option<ProgressCallback>,
        on_patch: Option<PatchCallback>,
    ) -> (bool, ScanProgress) {
        if !node.is_dir {
            return (false, ScanProgress::default());
        }
        self.cancel.store(false, Ordering::SeqCst);

        let scan_root = if self.scan_root.as_os_str().is_empty() {
            std::fs::canonicalize(&node.path).unwrap_or_else(|_| node.path.clone())
        } else {
            self.scan_root.clone()
        };

        let ctx = self.make_context(node.path.clone(), scan_root, on_progress, on_patch);

        let mut temp = ScanNode::new(node.name.clone(), node.path.clone(), true);
        temp.owner = node.owner.clone();
        temp.mtime = node.mtime;
        temp.metadata_loaded = node.metadata_loaded;

        self.install_scan(|| scan_directory(&mut temp, &ctx, 0));
        flush_progress_counters(&ctx);
        maybe_emit_progress(&ctx, true);

        node.children = temp.children;
        node.size = temp.size;
        node.allocated = temp.allocated;
        node.file_count = temp.file_count;
        node.folder_count = temp.folder_count;
        node.scan_complete = true;

        if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
            progress.is_complete = true;
        }
        emit_progress(&ctx);
        // As with full scans, the rescanned subtree is delivered via the terminal
        // RescanComplete message (`set_root`); skip the redundant Subtree patch.
        (true, lock_progress(&ctx.progress))
    }

    /// Recompute size/count rollups for `target_path` and every ancestor up to
    /// `root`, bottom-up. Descends directly along the path (one component per
    /// level) rather than scanning the whole tree, so it stays O(depth) per
    /// call instead of O(nodes) — critical on the live-merge hot path where it
    /// runs once per incoming patch.
    pub fn rollup_chain(&self, root: &mut ScanNode, target_path: &Path) {
        fn descend(node: &mut ScanNode, rel: &[std::path::Component<'_>]) -> bool {
            match rel.split_first() {
                None => {
                    rollup_node(node);
                    true
                }
                Some((first, rest)) => {
                    let name = first.as_os_str();
                    let Some(idx) = node
                        .children
                        .iter()
                        .position(|c| c.path.file_name() == Some(name))
                    else {
                        return false;
                    };
                    if descend(&mut node.children[idx], rest) {
                        rollup_node(node);
                        true
                    } else {
                        false
                    }
                }
            }
        }

        let root_key = crate::paths::lexical_key(&root.path);
        let target_key = crate::paths::lexical_key(target_path);
        if root_key == target_key {
            rollup_node(root);
            return;
        }
        let Ok(rel) = target_key.strip_prefix(&root_key) else {
            return;
        };
        let comps: Vec<_> = rel.components().collect();
        descend(root, &comps);
    }

    pub fn rescan_subtree_in_tree(
        &mut self,
        root: &mut ScanNode,
        path: &Path,
        on_progress: Option<ProgressCallback>,
        on_patch: Option<PatchCallback>,
    ) -> (bool, ScanProgress) {
        if let Some(node) = root.find_by_path_mut(path) {
            let node_path = node.path.clone();
            let (ok, progress) = self.rescan_subtree(node, on_progress, on_patch);
            if ok {
                self.rollup_chain(root, &node_path);
                if let Some(parent_path) = node_path.parent() {
                    if parent_path != node_path.as_path() {
                        self.rollup_chain(root, parent_path);
                    }
                }
            }
            (ok, progress)
        } else {
            (false, ScanProgress::default())
        }
    }
}

pub fn parse_volumes_from_df(stdout: &str) -> Vec<VolumeInfo> {
    let mut volumes = Vec::new();
    for line in stdout.lines().skip(1) {
        let Some((filesystem, blocks, used, available, mount)) = parse_df_line(line) else {
            continue;
        };
        if !mount.starts_with('/') {
            continue;
        }
        if filesystem.starts_with("devfs") || filesystem.starts_with("map") {
            continue;
        }
        let name = Path::new(&mount)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| mount.clone());
        volumes.push(VolumeInfo {
            name,
            mount_point: PathBuf::from(mount),
            total_bytes: blocks * 1024,
            used_bytes: used * 1024,
            free_bytes: available * 1024,
        });
    }
    volumes
}

fn expand_abs(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    let expanded = if s == "~" {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"))
    } else if let Some(rest) = s.strip_prefix("~/") {
        std::env::var_os("HOME")
            .map(|h| PathBuf::from(h).join(rest))
            .unwrap_or_else(|| PathBuf::from(s.as_ref()))
    } else {
        path.to_path_buf()
    };
    if expanded.is_absolute() {
        expanded
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(expanded)
    }
}

pub fn collect_largest_files(root: &ScanNode, n: usize) -> Vec<&ScanNode> {
    let mut files: Vec<&ScanNode> = root
        .iter_descendants()
        .filter(|node| !node.is_dir && node.size > 0)
        .collect();
    files.sort_by_key(|b| std::cmp::Reverse(b.size));
    files.truncate(n);
    files
}

pub fn collect_extension_stats(root: &ScanNode) -> Vec<ExtensionStats> {
    use std::collections::HashMap;
    let mut stats: HashMap<String, ExtensionStats> = HashMap::new();
    for node in root.iter_descendants() {
        if node.is_dir {
            continue;
        }
        let ext = if node.extension.is_empty() {
            get_file_extension(&node.name)
        } else {
            node.extension.clone()
        };
        let entry = stats.entry(ext.clone()).or_insert(ExtensionStats {
            extension: ext,
            total_size: 0,
            file_count: 0,
        });
        entry.total_size += node.size;
        entry.file_count += 1;
    }
    let mut result: Vec<ExtensionStats> = stats.into_values().collect();
    result.sort_by_key(|b| std::cmp::Reverse(b.total_size));
    result
}

pub use crate::charts::{ascii_bar_chart, labeled_children_chart, labeled_pie_legend};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_df_line_rejects_short() {
        assert!(parse_df_line("too few").is_none());
    }

    #[test]
    fn test_parse_volumes_from_df_filters_devfs() {
        let stdout = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                      devfs 1 1 0 100% /dev\n\
                      /dev/disk1 1000 500 500 50% /\n";
        let volumes = parse_volumes_from_df(stdout);
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].mount_point, PathBuf::from("/"));
    }
}
