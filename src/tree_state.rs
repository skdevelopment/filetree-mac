//! Live scan tree merge state with lexical path keys and orphan buffering.

use crate::models::{PatchKind, ScanNode, TreePatch};
use crate::paths;
use crate::scanner::DirectoryScanner;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct TreeState {
    pub root: Option<ScanNode>,
    pub dirty: bool,
    /// Patches whose parent is not yet in the tree, keyed by the lexical key of
    /// the parent path they wait on. When that parent merges, only the patches
    /// keyed to it are retried (cascading to any they in turn unblock), so
    /// orphan handling stays O(total orphans) instead of re-scanning the whole
    /// buffer after every single patch.
    orphans: HashMap<PathBuf, Vec<TreePatch>>,
}

impl TreeState {
    pub fn set_root(&mut self, root: ScanNode) {
        // The terminal scan message delivers the authoritative full tree, which
        // supersedes any buffered live patches; replaying stale partial patches
        // over it could clobber finished stats, so drop them.
        self.root = Some(root);
        self.dirty = true;
        self.orphans.clear();
    }

    pub fn apply_patch(&mut self, patch: TreePatch, scanner: &DirectoryScanner) -> bool {
        let merged = match &mut self.root {
            None => {
                let path = paths::lexical_key(&patch.node.path);
                self.root = Some(patch.node);
                self.dirty = true;
                Some(path)
            }
            Some(full) => {
                if Self::try_merge(full, &patch, scanner) {
                    self.dirty = true;
                    Some(paths::lexical_key(&patch.node.path))
                } else {
                    self.buffer_orphan(patch);
                    None
                }
            }
        };
        match merged {
            Some(path) => {
                self.resolve_orphans_for(path, scanner);
                true
            }
            None => false,
        }
    }

    fn try_merge(full: &mut ScanNode, patch: &TreePatch, scanner: &DirectoryScanner) -> bool {
        let path = &patch.node.path;
        if paths::lexical_key(&full.path) == paths::lexical_key(path) {
            full.apply_patch(patch);
            return true;
        }
        if let Some(target) = full.find_by_path_mut(path) {
            target.apply_patch(patch);
            // `rollup_chain` rolls up `path` and every ancestor up to the root in
            // one directed pass, so no separate parent rollup is needed.
            scanner.rollup_chain(full, path);
            return true;
        }
        let Some(parent_path) = path.parent() else {
            return false;
        };
        if parent_path == path.as_path() || full.find_by_path(parent_path).is_none() {
            return false;
        }
        let Some(parent_node) = full.find_by_path_mut(parent_path) else {
            return false;
        };
        if let Some(idx) = ScanNode::child_index_by_path(&parent_node.children, path) {
            parent_node.children[idx].apply_patch(patch);
        } else {
            let mut child = patch.node.clone();
            if matches!(patch.kind, PatchKind::Listed) {
                for c in &mut child.children {
                    c.children.clear();
                }
            }
            parent_node.add_child(child);
        }
        scanner.rollup_chain(full, path);
        true
    }

    /// The path whose presence in the tree would let `patch` merge: its parent
    /// (or the path itself for a parent-less root patch).
    fn orphan_key(patch: &TreePatch) -> PathBuf {
        match patch.node.path.parent() {
            Some(parent) => paths::lexical_key(parent),
            None => paths::lexical_key(&patch.node.path),
        }
    }

    fn buffer_orphan(&mut self, patch: TreePatch) {
        let key = Self::orphan_key(&patch);
        self.orphans.entry(key).or_default().push(patch);
    }

    /// `merged_path` (and therefore its freshly-created child stubs) just became
    /// present, so retry every orphan that was waiting on it, cascading to each
    /// path those merges unblock. Touches only the relevant orphans, not the
    /// whole buffer.
    fn resolve_orphans_for(&mut self, merged_path: PathBuf, scanner: &DirectoryScanner) {
        let mut work = vec![merged_path];
        while let Some(path) = work.pop() {
            let Some(waiting) = self.orphans.remove(&path) else {
                continue;
            };
            let mut rebuffer: Vec<TreePatch> = Vec::new();
            {
                let Some(full) = self.root.as_mut() else {
                    return;
                };
                for patch in waiting {
                    if Self::try_merge(full, &patch, scanner) {
                        work.push(paths::lexical_key(&patch.node.path));
                    } else {
                        rebuffer.push(patch);
                    }
                }
            }
            for patch in rebuffer {
                self.buffer_orphan(patch);
            }
            self.dirty = true;
        }
    }
}
