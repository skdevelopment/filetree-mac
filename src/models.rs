use crate::paths;
use chrono::{DateTime, Local};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SortKey {
    Name,
    Size,
    Allocated,
    Date,
    Extension,
    Owner,
    Percent,
}

impl SortKey {
    pub fn all() -> &'static [SortKey] {
        &[
            SortKey::Name,
            SortKey::Size,
            SortKey::Allocated,
            SortKey::Date,
            SortKey::Extension,
            SortKey::Owner,
            SortKey::Percent,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            SortKey::Name => "name",
            SortKey::Size => "size",
            SortKey::Allocated => "allocated",
            SortKey::Date => "date",
            SortKey::Extension => "extension",
            SortKey::Owner => "owner",
            SortKey::Percent => "percent",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScanNode {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub allocated: u64,
    pub file_count: u64,
    pub folder_count: u64,
    pub mtime: f64,
    pub owner: String,
    pub extension: String,
    pub is_symlink: bool,
    pub symlink_target: String,
    pub children: Vec<ScanNode>,
    pub scan_complete: bool,
    /// When false, owner/mtime/extension are filled on first UI access (fast scan).
    pub metadata_loaded: bool,
}

impl ScanNode {
    pub fn new(name: impl Into<String>, path: PathBuf, is_dir: bool) -> Self {
        Self {
            name: name.into(),
            path,
            is_dir,
            size: 0,
            allocated: 0,
            file_count: 0,
            folder_count: 0,
            mtime: 0.0,
            owner: String::new(),
            extension: String::new(),
            is_symlink: false,
            symlink_target: String::new(),
            children: Vec::new(),
            scan_complete: false,
            metadata_loaded: false,
        }
    }

    pub fn needs_metadata(&self) -> bool {
        !self.metadata_loaded
    }

    pub fn percent_of_parent(&self, parent: Option<&ScanNode>) -> f64 {
        match parent {
            None => {
                if self.size > 0 {
                    100.0
                } else {
                    0.0
                }
            }
            Some(p) if p.size == 0 => 0.0,
            Some(p) => (self.size as f64 / p.size as f64) * 100.0,
        }
    }

    pub fn mtime_dt(&self) -> Option<DateTime<Local>> {
        if self.mtime == 0.0 {
            None
        } else {
            DateTime::from_timestamp(self.mtime as i64, 0).map(|dt| dt.with_timezone(&Local))
        }
    }

    pub fn add_child(&mut self, child: ScanNode) {
        self.children.push(child);
    }

    fn copy_stats_from(&mut self, other: &ScanNode) {
        self.size = other.size;
        self.allocated = other.allocated;
        self.file_count = other.file_count;
        self.folder_count = other.folder_count;
        self.mtime = other.mtime;
        self.owner = other.owner.clone();
        self.extension = other.extension.clone();
        self.is_symlink = other.is_symlink;
        self.symlink_target = other.symlink_target.clone();
        self.scan_complete = other.scan_complete;
        self.metadata_loaded = other.metadata_loaded;
    }

    fn copy_child_stats(target: &mut ScanNode, source: &ScanNode) {
        target.size = source.size;
        target.allocated = source.allocated;
        target.file_count = source.file_count;
        target.folder_count = source.folder_count;
        target.mtime = source.mtime;
        target.owner = source.owner.clone();
        target.extension = source.extension.clone();
        target.is_symlink = source.is_symlink;
        target.symlink_target = source.symlink_target.clone();
        target.scan_complete = source.scan_complete;
        target.metadata_loaded = source.metadata_loaded;
    }

    pub(crate) fn child_index_by_path(children: &[ScanNode], path: &Path) -> Option<usize> {
        // `path` is always a direct child of these `children`'s parent, and names
        // are unique within a directory, so the final component identifies the
        // child unambiguously — no allocation or path normalization needed.
        let name = path.file_name()?;
        children
            .iter()
            .position(|c| c.path.file_name() == Some(name))
    }

    /// Index existing children by file name for O(1) lookup. Built once per
    /// merge so matching N patch children against M existing children is O(N+M)
    /// instead of O(N·M) — directories like `target/debug/deps` can hold tens of
    /// thousands of entries, where the quadratic scan cost seconds per patch.
    fn child_name_index(&self) -> HashMap<OsString, usize> {
        let mut index = HashMap::with_capacity(self.children.len());
        for (i, c) in self.children.iter().enumerate() {
            if let Some(name) = c.path.file_name() {
                index.insert(name.to_os_string(), i);
            }
        }
        index
    }

    /// Apply a live scan patch without discarding deeper cached children.
    pub fn apply_patch(&mut self, patch: &TreePatch) {
        self.copy_stats_from(&patch.node);
        let index = self.child_name_index();

        match patch.kind {
            PatchKind::Listed => {
                for p_child in &patch.node.children {
                    let existing = p_child.path.file_name().and_then(|n| index.get(n).copied());
                    if let Some(idx) = existing {
                        Self::copy_child_stats(&mut self.children[idx], p_child);
                    } else {
                        let mut stub = p_child.clone();
                        stub.children.clear();
                        self.add_child(stub);
                    }
                }
            }
            PatchKind::Subtree => {
                for p_child in &patch.node.children {
                    let existing = p_child.path.file_name().and_then(|n| index.get(n).copied());
                    if let Some(idx) = existing {
                        self.children[idx].merge_subtree(p_child);
                    } else {
                        self.add_child(p_child.clone());
                    }
                }
            }
        }
    }

    /// Merge a fully-scanned subtree from `source` into `self` in place, cloning
    /// a node only when it does not already exist. Recursing by reference keeps
    /// this O(nodes); the previous approach cloned the entire subtree at every
    /// level, making a deep merge O(nodes²).
    fn merge_subtree(&mut self, source: &ScanNode) {
        self.copy_stats_from(source);
        let index = self.child_name_index();
        for s_child in &source.children {
            let existing = s_child.path.file_name().and_then(|n| index.get(n).copied());
            if let Some(idx) = existing {
                self.children[idx].merge_subtree(s_child);
            } else {
                self.add_child(s_child.clone());
            }
        }
    }

    /// Build a listing patch (direct children only, no grandchildren).
    pub fn listing_patch(&self) -> TreePatch {
        TreePatch {
            kind: PatchKind::Listed,
            node: ScanNode {
                name: self.name.clone(),
                path: self.path.clone(),
                is_dir: self.is_dir,
                size: self.size,
                allocated: self.allocated,
                file_count: self.file_count,
                folder_count: self.folder_count,
                mtime: self.mtime,
                owner: self.owner.clone(),
                extension: self.extension.clone(),
                is_symlink: self.is_symlink,
                symlink_target: self.symlink_target.clone(),
                scan_complete: self.scan_complete,
                metadata_loaded: self.metadata_loaded,
                children: self
                    .children
                    .iter()
                    .map(|c| ScanNode {
                        name: c.name.clone(),
                        path: c.path.clone(),
                        is_dir: c.is_dir,
                        size: c.size,
                        allocated: c.allocated,
                        file_count: c.file_count,
                        folder_count: c.folder_count,
                        mtime: c.mtime,
                        owner: c.owner.clone(),
                        extension: c.extension.clone(),
                        is_symlink: c.is_symlink,
                        symlink_target: c.symlink_target.clone(),
                        children: Vec::new(),
                        scan_complete: c.scan_complete,
                        metadata_loaded: c.metadata_loaded,
                    })
                    .collect(),
            },
        }
    }

    /// Build a subtree patch for a completed directory.
    pub fn subtree_patch(&self) -> TreePatch {
        TreePatch {
            kind: PatchKind::Subtree,
            node: self.clone(),
        }
    }

    pub fn sorted_children(&self, key: SortKey, reverse: bool) -> Vec<&ScanNode> {
        let mut children: Vec<&ScanNode> = self.children.iter().collect();
        children.sort_by(|a, b| {
            let ord = match key {
                SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortKey::Size => a.size.cmp(&b.size),
                SortKey::Allocated => a.allocated.cmp(&b.allocated),
                SortKey::Date => a.mtime.partial_cmp(&b.mtime).unwrap_or(Ordering::Equal),
                SortKey::Extension => a.extension.to_lowercase().cmp(&b.extension.to_lowercase()),
                SortKey::Owner => a.owner.to_lowercase().cmp(&b.owner.to_lowercase()),
                SortKey::Percent => {
                    let pa = a.percent_of_parent(Some(self));
                    let pb = b.percent_of_parent(Some(self));
                    pa.partial_cmp(&pb).unwrap_or(Ordering::Equal)
                }
            };
            if reverse {
                ord.reverse()
            } else {
                ord
            }
        });
        children
    }

    pub fn iter_descendants(&self) -> impl Iterator<Item = &ScanNode> {
        ScanNodeIter {
            stack: self.children.iter().collect(),
        }
    }

    pub fn find_by_path(&self, path: &Path) -> Option<&ScanNode> {
        // Tree nodes nest strictly by path, so descend directly to the target by
        // matching one component per level instead of walking every node. Both
        // endpoints are keyed (syscall-free) so the search root and query agree
        // on the `/private` firmlink form before comparing prefixes.
        let key = paths::lexical_key(path);
        let self_key = paths::lexical_key(&self.path);
        if self_key == key {
            return Some(self);
        }
        let rel = key.strip_prefix(&self_key).ok()?;
        let mut node = self;
        for comp in rel.components() {
            let name = comp.as_os_str();
            let idx = node
                .children
                .iter()
                .position(|c| c.path.file_name() == Some(name))?;
            node = &node.children[idx];
        }
        Some(node)
    }

    pub fn find_by_path_mut(&mut self, path: &Path) -> Option<&mut ScanNode> {
        let key = paths::lexical_key(path);
        let self_key = paths::lexical_key(&self.path);
        if self_key == key {
            return Some(self);
        }
        let rel = key.strip_prefix(&self_key).ok()?;
        let names: Vec<_> = rel
            .components()
            .map(|c| c.as_os_str().to_os_string())
            .collect();
        let mut node = self;
        for name in &names {
            let idx = node
                .children
                .iter()
                .position(|c| c.path.file_name() == Some(name.as_os_str()))?;
            node = &mut node.children[idx];
        }
        Some(node)
    }

    pub fn filter_by_name(&self, pattern: &str, case_sensitive: bool) -> Vec<&ScanNode> {
        let pattern_cmp = if case_sensitive {
            pattern.to_string()
        } else {
            pattern.to_lowercase()
        };
        let mut results = Vec::new();
        let root_name = if case_sensitive {
            self.name.clone()
        } else {
            self.name.to_lowercase()
        };
        if root_name.contains(&pattern_cmp) {
            results.push(self);
        }
        for node in self.iter_descendants() {
            let name = if case_sensitive {
                node.name.clone()
            } else {
                node.name.to_lowercase()
            };
            if name.contains(&pattern_cmp) {
                results.push(node);
            }
        }
        results
    }
}

struct ScanNodeIter<'a> {
    stack: Vec<&'a ScanNode>,
}

impl<'a> Iterator for ScanNodeIter<'a> {
    type Item = &'a ScanNode;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.stack.pop()?;
        for child in node.children.iter().rev() {
            self.stack.push(child);
        }
        Some(node)
    }
}

/// Maximum scan errors retained in memory (additional errors bump `error_count` only).
pub const MAX_STORED_SCAN_ERRORS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchKind {
    /// Direct children listed; grandchildren are not materialized.
    Listed,
    /// Full or partial subtree under `node`.
    Subtree,
}

#[derive(Debug, Clone)]
pub struct TreePatch {
    pub kind: PatchKind,
    pub node: ScanNode,
}

/// Lightweight progress update for the UI thread (no error string clones).
#[derive(Debug, Clone)]
pub struct ProgressSnapshot {
    pub bytes_scanned: u64,
    pub scanned_items: u64,
    pub scanned_dirs: u64,
    pub error_count: usize,
    pub cancelled: bool,
    pub is_complete: bool,
    pub current_path: PathBuf,
    pub first_error: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ScanProgress {
    pub scanned_items: u64,
    pub scanned_dirs: u64,
    pub current_path: PathBuf,
    pub bytes_scanned: u64,
    pub is_complete: bool,
    pub cancelled: bool,
    pub error: Option<String>,
    pub errors: Vec<String>,
}

impl ScanProgress {
    pub fn push_error(&mut self, message: String) {
        if self.error.is_none() {
            self.error = Some(message.clone());
        }
        if self.errors.len() < MAX_STORED_SCAN_ERRORS {
            self.errors.push(message);
        }
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            bytes_scanned: self.bytes_scanned,
            scanned_items: self.scanned_items,
            scanned_dirs: self.scanned_dirs,
            error_count: self.errors.len(),
            cancelled: self.cancelled,
            is_complete: self.is_complete,
            current_path: self.current_path.clone(),
            first_error: self.error.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct VolumeInfo {
    pub name: String,
    pub mount_point: PathBuf,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub free_bytes: u64,
}

impl VolumeInfo {
    pub fn used_percent(&self) -> f64 {
        if self.total_bytes == 0 {
            0.0
        } else {
            (self.used_bytes as f64 / self.total_bytes as f64) * 100.0
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtensionStats {
    pub extension: String,
    pub total_size: u64,
    pub file_count: u64,
}

impl ExtensionStats {
    pub fn display_name(&self) -> String {
        if self.extension.is_empty() {
            "(no extension)".to_string()
        } else {
            self.extension.clone()
        }
    }
}
