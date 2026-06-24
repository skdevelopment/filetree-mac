use crate::app::App;
use crate::app_logic::clamp_table_selection;
use crate::menu::ViewMode;
use crate::models::{ScanNode, SortKey, VolumeInfo};
use crate::scan_cache::fill_node_metadata;
use crate::scanner::{
    collect_extension_stats, collect_largest_files, format_bytes, labeled_pie_legend,
    list_volumes,
};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub(crate) const VIEW_CYCLE: [ViewMode; 4] = [
    ViewMode::Tree,
    ViewMode::TopFiles,
    ViewMode::Extensions,
    ViewMode::Volumes,
];

pub(crate) struct TreeRow {
    pub display_name: String,
    pub path: PathBuf,
    pub size: String,
    pub allocated: String,
    pub pct_parent: String,
    pub pct_disk: String,
    pub bar: String,
    pub files: String,
    pub folders: String,
    pub ext: String,
    pub modified: String,
    pub owner: String,
}

impl App {
    pub(crate) fn scroll_alt_view(&mut self, delta: i32) {
        let visible = self.alt_viewport_rows.max(1) as usize;
        let max_scroll = self.alt_lines.len().saturating_sub(visible) as i64;
        let next = (self.alt_scroll as i64 + delta as i64).clamp(0, max_scroll);
        self.alt_scroll = next as u16;
    }

    pub(crate) fn maybe_refresh_tree(&mut self) {
        if !self.tree_state.dirty {
            return;
        }
        let now = Instant::now();
        let interval = if self.scan_in_progress() {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(0)
        };
        if now.duration_since(self.last_tree_refresh) >= interval {
            self.last_tree_refresh = now;
            self.tree_state.dirty = false;
            self.rebuild_view();
            self.mark_dirty();
        }
    }

    pub(crate) fn recompute_filter_matches(&mut self) {
        if self.filter_text.is_empty() {
            self.filter_match_paths.clear();
            return;
        }
        let Some(ref root) = self.tree_state.root else {
            return;
        };
        let matches = root.filter_by_name(&self.filter_text, false);
        let mut paths = HashSet::new();
        for m in matches {
            paths.insert(m.path.clone());
            let mut current = m.path.parent();
            while let Some(p) = current {
                paths.insert(p.to_path_buf());
                current = p.parent();
            }
        }
        self.filter_match_paths = paths;
    }

    pub(crate) fn auto_expand_tree(&mut self) {
        if let Some(ref root) = self.tree_state.root {
            self.expanded_paths.insert(root.path.clone());
            for child in &root.children {
                if child.is_dir {
                    self.expanded_paths.insert(child.path.clone());
                }
            }
        }
    }

    pub(crate) fn rebuild_view(&mut self) {
        if let Some(path) = self.selected_path() {
            self.saved_cursor_path = Some(path);
        }
        match self.view_mode {
            ViewMode::Tree => self.build_tree_view(),
            ViewMode::TopFiles => self.build_top_files_view(),
            ViewMode::Extensions => self.build_extensions_view(),
            ViewMode::Volumes => self.build_volumes_view(),
        }
        let selected = self
            .saved_cursor_path
            .as_ref()
            .and_then(|p| self.tree_rows.iter().position(|r| &r.path == p))
            .or(self.table_state.selected());
        self.table_state
            .select(clamp_table_selection(selected, self.tree_rows.len()));
    }

    pub(crate) fn fill_visible_metadata(&mut self) {
        let Some(root) = self.tree_state.root.as_mut() else {
            return;
        };
        fn fill_expanded(node: &mut ScanNode, expanded: &HashSet<PathBuf>) {
            if node.needs_metadata() {
                fill_node_metadata(node);
            }
            if expanded.contains(&node.path) {
                for child in &mut node.children {
                    fill_expanded(child, expanded);
                }
            }
        }
        fill_expanded(root, &self.expanded_paths);
        if let Some(path) = self.saved_cursor_path.clone() {
            if let Some(node) = root.find_by_path_mut(&path) {
                fill_node_metadata(node);
            }
        }
    }

    pub(crate) fn build_tree_view(&mut self) {
        self.tree_rows.clear();
        self.fill_visible_metadata();
        let Some(ref root) = self.tree_state.root else {
            return;
        };

        #[allow(clippy::too_many_arguments)]
        fn walk(
            node: &ScanNode,
            parent: Option<&ScanNode>,
            root_size: u64,
            prefix: &str,
            is_last: bool,
            is_root: bool,
            expanded: &HashSet<PathBuf>,
            filter: &str,
            filter_paths: &HashSet<PathBuf>,
            sort_key: SortKey,
            sort_reverse: bool,
            rows: &mut Vec<TreeRow>,
        ) {
            if !filter.is_empty() && !filter_paths.contains(&node.path) {
                return;
            }

            let (branch, line_prefix) = if is_root {
                ("", "")
            } else if is_last {
                ("└── ", prefix)
            } else {
                ("├── ", prefix)
            };

            let (expand_marker, icon) = if node.is_dir {
                if expanded.contains(&node.path) {
                    ("v ", "[D]")
                } else {
                    ("> ", "[D]")
                }
            } else {
                ("  ", "[F]")
            };

            let mut display_name =
                format!("{line_prefix}{branch}{expand_marker}{icon}{}", node.name);
            if node.is_symlink {
                display_name.push_str(" →");
            }
            if node.is_dir && !node.scan_complete {
                display_name.push_str(" …");
            }

            let has_partial_size = node.size > 0 || node.allocated > 0;
            let show_metrics = node.scan_complete || !node.is_dir || has_partial_size;
            let size_display = if show_metrics {
                format_bytes(node.size as i64)
            } else {
                "…".to_string()
            };
            let alloc_display = if show_metrics {
                format_bytes(node.allocated as i64)
            } else {
                "…".to_string()
            };
            let pct_parent = if show_metrics {
                format!("{:.1}%", node.percent_of_parent(parent))
            } else {
                "…".to_string()
            };
            let pct_disk = if root_size == 0 {
                "—".to_string()
            } else if show_metrics {
                format!("{:.1}%", node.size as f64 / root_size as f64 * 100.0)
            } else {
                "…".to_string()
            };
            let bar = if root_size == 0 || node.size == 0 {
                "░".repeat(10)
            } else if !show_metrics {
                "…".to_string()
            } else {
                let pct = (node.size as f64 / root_size as f64).min(1.0);
                let filled = if pct > 0.0 {
                    (pct * 10.0).max(1.0) as usize
                } else {
                    0
                };
                format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled))
            };

            rows.push(TreeRow {
                display_name,
                path: node.path.clone(),
                size: size_display,
                allocated: alloc_display,
                pct_parent,
                pct_disk,
                bar,
                files: format!("{}", node.file_count),
                folders: format!("{}", node.folder_count),
                ext: if node.is_dir {
                    String::new()
                } else {
                    node.extension.clone()
                },
                modified: node
                    .mtime_dt()
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_default(),
                owner: node.owner.clone(),
            });

            if node.is_dir && expanded.contains(&node.path) {
                let children = node.sorted_children(sort_key, sort_reverse);
                let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
                for (i, child) in children.iter().enumerate() {
                    walk(
                        child,
                        Some(node),
                        root_size,
                        &child_prefix,
                        i == children.len() - 1,
                        false,
                        expanded,
                        filter,
                        filter_paths,
                        sort_key,
                        sort_reverse,
                        rows,
                    );
                }
            }
        }

        walk(
            root,
            None,
            root.size,
            "",
            true,
            true,
            &self.expanded_paths,
            &self.filter_text,
            &self.filter_match_paths,
            self.sort_key,
            self.sort_reverse,
            &mut self.tree_rows,
        );
    }

    pub(crate) fn build_top_files_view(&mut self) {
        self.alt_lines.clear();
        let rows: Vec<TreeRow> = {
            let Some(ref root) = self.tree_state.root else {
                self.tree_rows.clear();
                return;
            };
            let files = collect_largest_files(root, 100);
            let root_size = root.size.max(1);
            files
                .iter()
                .map(|f| TreeRow {
                    display_name: f.path.display().to_string(),
                    path: f.path.clone(),
                    size: format_bytes(f.size as i64),
                    allocated: String::new(),
                    pct_parent: String::new(),
                    pct_disk: format!("{:.1}%", f.size as f64 / root_size as f64 * 100.0),
                    bar: String::new(),
                    files: String::new(),
                    folders: String::new(),
                    ext: String::new(),
                    modified: String::new(),
                    owner: String::new(),
                })
                .collect()
        };
        self.tree_rows = rows;
    }

    pub(crate) fn build_extensions_view(&mut self) {
        self.alt_lines.clear();
        let Some(ref root) = self.tree_state.root else {
            self.alt_lines.push("No scan data".to_string());
            return;
        };
        let stats = collect_extension_stats(root);
        let total: u64 = stats.iter().map(|s| s.total_size).sum::<u64>().max(1);
        let items: Vec<(String, u64)> = stats
            .iter()
            .map(|s| (s.display_name(), s.total_size))
            .collect();

        self.alt_lines
            .push("Extensions — pie chart (labeled)".to_string());
        self.alt_lines.push(String::new());
        for line in labeled_pie_legend(&items, 32, 12) {
            self.alt_lines.push(line);
        }
        self.alt_lines.push(String::new());
        self.alt_lines.push("Details".to_string());
        for item in stats.iter().take(30) {
            let pct = item.total_size as f64 / total as f64 * 100.0;
            let label = if item.extension.is_empty() {
                item.display_name()
            } else {
                format!(".{}", item.extension)
            };
            self.alt_lines.push(format!(
                "  {label:12} {:>10}  {:>6} files  ({pct:.1}%)",
                format_bytes(item.total_size as i64),
                item.file_count
            ));
        }
    }

    pub(crate) fn build_volumes_view(&mut self) {
        self.volumes = list_volumes();
        self.tree_rows.clear();
        for vol in &self.volumes {
            let used_pct = vol.used_percent() / 100.0;
            let filled = (used_pct * 12.0) as usize;
            let bar = format!("{}{}", "█".repeat(filled), "░".repeat(12 - filled));
            self.tree_rows.push(TreeRow {
                display_name: vol.name.clone(),
                path: vol.mount_point.clone(),
                size: format_bytes(vol.total_bytes as i64),
                allocated: format_bytes(vol.used_bytes as i64),
                pct_parent: format_bytes(vol.free_bytes as i64),
                pct_disk: format!("{:.1}%", vol.used_percent()),
                bar,
                files: vol.mount_point.to_string_lossy().to_string(),
                folders: String::new(),
                ext: String::new(),
                modified: String::new(),
                owner: String::new(),
            });
        }
    }

    pub(crate) fn get_selected_node(&self) -> Option<&ScanNode> {
        if !matches!(self.view_mode, ViewMode::Tree | ViewMode::TopFiles) {
            return None;
        }
        let idx = self.table_state.selected()?;
        let path = self.tree_rows.get(idx)?.path.clone();
        self.tree_state.root.as_ref()?.find_by_path(&path)
    }

    pub(crate) fn get_selected_volume(&self) -> Option<&VolumeInfo> {
        if self.view_mode != ViewMode::Volumes {
            return None;
        }
        let idx = self.table_state.selected()?;
        let path = self.tree_rows.get(idx)?.path.clone();
        self.volumes.iter().find(|v| v.mount_point == path)
    }

    pub(crate) fn page_step(&self) -> i32 {
        if matches!(self.view_mode, ViewMode::Extensions) {
            self.alt_viewport_rows.max(1) as i32
        } else {
            self.table_viewport_rows.max(1) as i32
        }
    }

    pub(crate) fn scroll_main(&mut self, delta: i32) {
        if matches!(self.view_mode, ViewMode::Extensions) {
            self.scroll_alt_view(delta);
        } else {
            self.move_cursor(delta);
        }
        self.mark_dirty();
    }

    pub(crate) fn jump_to_edge(&mut self, to_start: bool) {
        if matches!(self.view_mode, ViewMode::Extensions) {
            if to_start {
                self.alt_scroll = 0;
            } else {
                self.scroll_alt_view(self.alt_lines.len() as i32);
            }
        } else if !self.tree_rows.is_empty() {
            let target = if to_start {
                0
            } else {
                self.tree_rows.len() - 1
            };
            self.table_state.select(Some(target));
        }
        self.mark_dirty();
    }

    pub(crate) fn move_cursor(&mut self, delta: i32) {
        if self.tree_rows.is_empty() {
            return;
        }
        let selected = self.table_state.selected().unwrap_or(0) as i64;
        let max = self.tree_rows.len() as i64 - 1;
        let new = (selected + delta as i64).clamp(0, max);
        self.table_state.select(Some(new as usize));
    }

    pub(crate) fn expand_selected(&mut self) {
        if let Some(path) = self.selected_path() {
            if self
                .tree_state
                .root
                .as_ref()
                .and_then(|r| r.find_by_path(&path))
                .map(|n| n.is_dir)
                .unwrap_or(false)
            {
                self.expanded_paths.insert(path);
                self.rebuild_view();
            }
        }
    }

    pub(crate) fn collapse_selected(&mut self) {
        let path = self.selected_path();
        if let Some(path) = path {
            self.expanded_paths.remove(&path);
            self.rebuild_view();
        }
    }

    pub(crate) fn toggle_expand_or_scan(&mut self) {
        if self.view_mode == ViewMode::Volumes {
            if let Some(vol) = self.get_selected_volume() {
                self.start_scan(vol.mount_point.clone());
            }
            return;
        }
        let path = self.selected_path();
        if let Some(path) = path {
            if self.expanded_paths.contains(&path) {
                self.expanded_paths.remove(&path);
            } else {
                self.expanded_paths.insert(path);
            }
            self.rebuild_view();
        }
    }

    pub(crate) fn selected_path(&self) -> Option<PathBuf> {
        let idx = self.table_state.selected()?;
        self.tree_rows.get(idx).map(|r| r.path.clone())
    }
}