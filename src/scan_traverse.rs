//! Directory traversal: list, child build, parallel child dispatch, rollup.

use crate::macos_dir::read_dir_fast;
use crate::models::ScanNode;
use crate::paths::is_under_root_lexical;
use crate::scan_cache::get_file_extension;
use crate::scan_progress::{
    lock_progress_mut, mark_cancelled, maybe_emit_listing_patch, maybe_emit_progress,
    maybe_emit_progress_path, record_error, ScanContext,
};
use rayon::prelude::*;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::Ordering;

/// Maximum directory depth to descend. Far beyond any real filesystem tree; a
/// safety net so a missed cycle can never overflow a worker thread's stack.
const MAX_SCAN_DEPTH: usize = 512;

/// macOS File Provider roots, relative to `$HOME`. Cloud files here can be
/// "dataless" (not downloaded): reading them blocks on the network and can
/// trigger large downloads, so a broad scan must not descend into them.
const CLOUD_SYNC_MARKERS: &[&str] = &["Library/CloudStorage", "Library/Mobile Documents"];

fn cloud_sync_roots() -> Vec<std::path::PathBuf> {
    match std::env::var_os("HOME") {
        Some(home) => {
            let home = std::path::PathBuf::from(home);
            CLOUD_SYNC_MARKERS.iter().map(|m| home.join(m)).collect()
        }
        None => Vec::new(),
    }
}

/// Cloud File Provider roots to skip for a scan of `scan_root`. Empty when the
/// scan is explicitly targeting cloud storage (at or under such a root), so a
/// user who runs `filetree ~/Library/CloudStorage/<provider>` still gets a full
/// scan of it.
pub(crate) fn cloud_skip_roots_for(scan_root: &Path) -> Vec<std::path::PathBuf> {
    let roots = cloud_sync_roots();
    if roots.iter().any(|r| scan_root.starts_with(r)) {
        Vec::new()
    } else {
        roots
    }
}

pub(crate) fn rollup_node(node: &mut ScanNode) {
    node.size = node.children.iter().map(|c| c.size).sum();
    node.allocated = node.children.iter().map(|c| c.allocated).sum();
    node.file_count = node.children.iter().map(|c| c.file_count).sum();
    node.folder_count = node.children.iter().map(|c| c.folder_count).sum::<u64>()
        + node.children.iter().filter(|c| c.is_dir).count() as u64;
}

fn finish_child(node: &mut ScanNode, ctx: &ScanContext, depth: usize) {
    rollup_node(node);
    maybe_emit_listing_patch(node, ctx, depth);
}

fn is_allowed_path(path: &Path, ctx: &ScanContext) -> bool {
    // When not following symlinks, every path we visit is built by joining child
    // names onto the already-canonicalized scan root, so it is a lexical
    // descendant by construction. Skip the per-directory `realpath()` syscalls
    // entirely — this is the single hottest check in the traversal.
    if !ctx.follow_symlinks {
        return true;
    }
    // Following symlinks can jump outside the tree, so resolve the real path and
    // compare lexically against the (already-canonical) scan root.
    let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    is_under_root_lexical(&real, &ctx.scan_root)
}

fn should_visit_dir(path: &Path, ctx: &ScanContext) -> bool {
    if !is_allowed_path(path, ctx) {
        record_error(
            ctx,
            path,
            format!("Skipping directory outside scan root: {}", path.display()),
        );
        return false;
    }
    // Don't descend into macOS cloud File Provider areas (iCloud Drive, Google
    // Drive, OneDrive, Dropbox, Nextcloud, ...). Their not-downloaded items are
    // dataless: reading them blocks on the network and can pull down gigabytes.
    // They occupy ~0 local disk; scan such a folder directly to include it.
    for root in &ctx.cloud_skip_roots {
        if path != root.as_path() && path.starts_with(root) {
            record_error(
                ctx,
                path,
                format!(
                    "Skipped cloud storage (dataless; not traversed — scan it directly to include): {}",
                    path.display()
                ),
            );
            return false;
        }
    }
    // Cycle/duplicate guard by filesystem identity. This runs for *every* scan,
    // not just symlink-following ones: macOS firmlinks and mount points are real
    // directories that create traversal loops (e.g. `/Volumes/<bootdisk>` → `/`
    // and `/System/Volumes/Data/System/Volumes/Data/...`) even with symlinks
    // disabled. `fs::metadata` follows the final component so a followed symlink
    // is keyed by its target. If the stat fails we let the normal directory read
    // report the error rather than silently skipping.
    let Ok(meta) = std::fs::metadata(path) else {
        return true;
    };
    let key = (meta.dev(), meta.ino());
    let mut seen = ctx.seen_dirs.lock().unwrap_or_else(|e| e.into_inner());
    if !seen.insert(key) {
        drop(seen);
        record_error(
            ctx,
            path,
            format!(
                "Skipping already-visited directory (cycle or mount loop): {}",
                path.display()
            ),
        );
        return false;
    }
    true
}

fn build_child_from_entry(entry: crate::macos_dir::BulkEntry, ctx: &ScanContext) -> ScanNode {
    // Consume the bulk entry by value so the name and path allocations move
    // straight into the node instead of being cloned once per file.
    let crate::macos_dir::BulkEntry {
        name,
        path,
        mut is_dir,
        is_symlink,
        size,
        allocated,
    } = entry;

    if is_symlink && ctx.follow_symlinks {
        is_dir = path.is_dir();
    }

    let extension = if is_dir {
        String::new()
    } else {
        get_file_extension(&name)
    };

    let mut child = ScanNode::new(name, path, is_dir);
    child.is_symlink = is_symlink;

    if !is_dir {
        if is_symlink && !ctx.follow_symlinks {
            child.size = 0;
            child.allocated = 0;
        } else if is_symlink && ctx.follow_symlinks {
            if !is_allowed_path(&child.path, ctx) {
                child.size = 0;
                child.allocated = 0;
            } else if let Ok(meta) = fs::metadata(&child.path) {
                child.size = meta.size();
                child.allocated = meta.blocks() * 512;
                child.file_count = 1;
            }
        } else {
            child.size = size;
            child.allocated = allocated;
            child.file_count = 1;
        }
        child.extension = extension;
        ctx.progress_throttle.record_file(child.size);
    }

    child
}

pub(crate) fn scan_file(node: &mut ScanNode, ctx: &ScanContext) {
    if ctx.cancel.load(Ordering::SeqCst) {
        mark_cancelled(ctx);
        return;
    }
    match fs::symlink_metadata(&node.path) {
        Ok(meta) => {
            node.is_symlink = meta.file_type().is_symlink();
            if node.is_symlink {
                node.symlink_target = fs::read_link(&node.path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default();
                if ctx.follow_symlinks {
                    if !is_allowed_path(&node.path, ctx) {
                        record_error(
                            ctx,
                            &node.path,
                            format!(
                                "Skipping symlink target outside scan root: {}",
                                node.path.display()
                            ),
                        );
                        node.size = 0;
                        node.allocated = 0;
                    } else {
                        match fs::metadata(&node.path) {
                            Ok(m) => {
                                node.size = m.size();
                                node.allocated = m.blocks() * 512;
                            }
                            Err(_) => {
                                node.size = 0;
                                node.allocated = 0;
                            }
                        }
                    }
                } else {
                    node.size = 0;
                    node.allocated = 0;
                }
            } else {
                node.size = meta.size();
                node.allocated = meta.blocks() * 512;
            }
            node.extension = get_file_extension(&node.name);
            node.file_count = 1;
        }
        Err(e) => {
            record_error(
                ctx,
                &node.path,
                format!("Cannot read file {}: {e}", node.path.display()),
            );
        }
    }
    ctx.progress_throttle.record_file(node.size);
    maybe_emit_progress(ctx, false);
}

fn scan_child_directories(node: &mut ScanNode, ctx: &ScanContext, depth: usize, dir_count: usize) {
    let child_depth = depth + 1;

    if ctx.max_workers > 1 && dir_count > 1 {
        // Scan child directories in place on the shared rayon pool. Each subtree
        // emits its own completion patch (see end of `scan_directory`), so the UI
        // still streams per-directory updates; the parent is rolled up once by the
        // caller after this returns. Work-stealing means a deep, lopsided subtree
        // is split across idle workers instead of pinning one thread.
        node.children
            .par_iter_mut()
            .filter(|child| child.is_dir)
            .for_each(|child| scan_directory(child, ctx, child_depth));
    } else {
        // Sequential fallback (single worker, or a lone child directory). Iterate
        // by index so the per-child rollup can borrow the parent between children.
        let mut idx = 0;
        while idx < node.children.len() {
            if node.children[idx].is_dir {
                if ctx.cancel.load(Ordering::SeqCst) {
                    mark_cancelled(ctx);
                    return;
                }
                scan_directory(&mut node.children[idx], ctx, child_depth);
                finish_child(node, ctx, depth);
            }
            idx += 1;
        }
    }
}

pub(crate) fn scan_directory(node: &mut ScanNode, ctx: &ScanContext, depth: usize) {
    if ctx.cancel.load(Ordering::SeqCst) {
        mark_cancelled(ctx);
        return;
    }

    // Hard backstop against runaway recursion (e.g. an unforeseen directory
    // cycle) so a worker thread can never blow its stack. Real directory trees
    // are nowhere near this deep; cycles are normally caught by `should_visit_dir`.
    if depth > MAX_SCAN_DEPTH {
        record_error(
            ctx,
            &node.path,
            format!(
                "Skipping directory past max depth {MAX_SCAN_DEPTH}: {}",
                node.path.display()
            ),
        );
        node.scan_complete = true;
        return;
    }

    if node.is_dir && !should_visit_dir(&node.path, ctx) {
        node.scan_complete = true;
        return;
    }

    // Throttled: only locks the shared progress state (and clones the path) when
    // this worker wins the ~100ms emit slot, not on every directory.
    maybe_emit_progress_path(ctx, &node.path);

    let entries = match read_dir_fast(&node.path, ctx.show_hidden) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            let should_emit =
                if let Some(mut progress) = lock_progress_mut(&ctx.progress, &ctx.cancel) {
                    ctx.error_policy
                        .record_io(&mut progress, &node.path, "Permission denied", &e)
                } else {
                    false
                };
            if should_emit {
                maybe_emit_progress(ctx, true);
            }
            node.scan_complete = true;
            return;
        }
        Err(e) => {
            let should_emit = if let Some(mut progress) =
                lock_progress_mut(&ctx.progress, &ctx.cancel)
            {
                ctx.error_policy
                    .record_io(&mut progress, &node.path, "Cannot read directory", &e)
            } else {
                false
            };
            if should_emit {
                maybe_emit_progress(ctx, true);
            }
            node.scan_complete = true;
            return;
        }
    };

    // Lock-free: accumulated into an atomic and flushed on the next emit.
    ctx.progress_throttle.record_dir();
    maybe_emit_progress(ctx, false);

    node.children.reserve(entries.len());
    for entry in entries {
        if ctx.cancel.load(Ordering::SeqCst) {
            mark_cancelled(ctx);
            return;
        }
        if ctx.follow_symlinks && !is_allowed_path(&entry.path, ctx) {
            record_error(
                ctx,
                &entry.path,
                format!("Skipping entry outside scan root: {}", entry.path.display()),
            );
            continue;
        }
        node.add_child(build_child_from_entry(entry, ctx));
    }
    maybe_emit_progress(ctx, false);
    maybe_emit_listing_patch(node, ctx, depth);

    let dir_count = node.children.iter().filter(|c| c.is_dir).count();

    if dir_count == 0 {
        rollup_node(node);
        node.scan_complete = true;
        return;
    }

    scan_child_directories(node, ctx, depth, dir_count);

    if ctx.cancel.load(Ordering::SeqCst) {
        mark_cancelled(ctx);
        return;
    }

    rollup_node(node);
    node.scan_complete = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_roots_skipped_for_broad_scan_not_explicit() {
        let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else {
            return;
        };
        // A broad scan (e.g. `/` or `~`) skips the cloud File Provider roots so
        // it can't stall on dataless cloud items or trigger downloads.
        let broad = cloud_skip_roots_for(Path::new("/"));
        assert!(broad.iter().any(|p| p.ends_with("Library/CloudStorage")));
        assert!(broad
            .iter()
            .any(|p| p.ends_with("Library/Mobile Documents")));

        // An explicit scan at/under a cloud root opts in: nothing is skipped.
        let explicit = home.join("Library/CloudStorage/SomeProvider-account");
        assert!(cloud_skip_roots_for(&explicit).is_empty());
        assert!(cloud_skip_roots_for(&home.join("Library/CloudStorage")).is_empty());
    }
}
