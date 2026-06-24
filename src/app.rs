use crate::app_logic::{
    chart_bar_width, clamp_table_selection, confirm_dialog_executes, export_modal_selection,
    tick_needs_redraw, typed_confirm_executes,
};
use crate::export::{export_warning, is_sensitive_export_path, save_report};
use crate::fda::{check_full_disk_access, open_fda_settings};
use crate::menu::{self, Action};
use crate::models::{ProgressSnapshot, ScanNode, ScanProgress, SortKey, VolumeInfo};
use crate::paths::{is_delete_protected, is_under_scan_root, safe_delete_target};
use crate::platform::default_scan_path;
use crate::progress::{
    build_scan_progress_panel, scan_panel_cancelled, scan_progress_bar_width,
    scan_progress_inner_width, scan_progress_path_max_chars, RateTracker, ScanProgressPanelInput,
    SCAN_PROGRESS_PANEL_LINES,
};
use crate::progress_ui::{snapshot_to_scan_progress, ProgressDisplay};
use crate::scan_bridge::{ScanBridge, ScanMessage, MAX_MESSAGES_PER_POLL};
use crate::scan_cache::fill_node_metadata;
use crate::scanner::{
    ascii_bar_chart, collect_extension_stats, collect_largest_files, format_bytes,
    labeled_children_chart, labeled_pie_legend, list_volumes, volume_total_for_full_scan,
    DirectoryScanner, PatchCallback, ProgressCallback,
};
use crate::theme::Theme;
use crate::tree_state::TreeState;
use crate::util::truncate_chars;
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, MouseButton,
    MouseEvent, MouseEventKind,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Terminal;
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

const LARGE_DELETE_BYTES: u64 = 1_000_000_000;

/// Upper bound on input events drained in a single event-loop iteration, so a
/// flood of mouse-move events can never starve rendering.
const MAX_EVENTS_PER_TICK: usize = 256;

const HELP_TEXT: &str = r#"filetree — keyboard shortcuts

Navigation
  ↑/↓ or j/k     Move selection / scroll
  ←/→ or h/l     Collapse/expand folder
  PgUp/PgDn      Scroll one page
  Home/End       Jump to first/last row
  Enter          Toggle expand/collapse (or scan volume)
  Tab            Next view tab
  Shift+Tab      Previous view tab
  g / o          Go to / open scan path

Actions
  r              Refresh/rescan selected folder
  R              Rescan entire tree
  /              Filter/search by name (modal)
  Esc            Clear filter / close menu
  s              Cycle sort column
  S              Toggle sort direction
  d              Delete selected (with confirmation)
  f              Reveal in Finder
  e              Export report
  v              Toggle follow symlinks
  H              Toggle show hidden files
  t / T          Open color theme picker
  c              Cancel active scan

Views
  1              Tree view
  2              Top-N largest files
  3              Extension breakdown
  4              Drive/volume list

Mouse
  Click menu     Open a dropdown (File/View/Sort/Actions/Help)
  Click toolbar  Run the button's action
  Click row      Select; click again to expand/collapse
  Wheel          Scroll the list

Other
  ?              This help screen
  q              Quit"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Tree,
    TopFiles,
    Extensions,
    Volumes,
}

const VIEW_CYCLE: [ViewMode; 4] = [
    ViewMode::Tree,
    ViewMode::TopFiles,
    ViewMode::Extensions,
    ViewMode::Volumes,
];

#[derive(Debug, Clone)]
enum Modal {
    None,
    Help,
    Confirm {
        title: String,
        message: String,
        selected: usize,
    },
    TypedConfirm {
        message: String,
        confirm_text: String,
        input: String,
        selected: usize,
    },
    PathInput {
        prompt: String,
        input: String,
        purpose: PathInputPurpose,
        selected: usize,
    },
    Export {
        path_input: String,
        selected: usize,
    },
    ScanErrors {
        errors: Vec<String>,
        scroll: u16,
    },
    Message {
        title: String,
        message: String,
    },
    ThemePicker {
        selected: usize,
        /// Theme active when the picker opened, restored if the user cancels.
        original: Theme,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathInputPurpose {
    Goto,
    Filter,
}

#[derive(Debug, Clone)]
enum PendingAction {
    Delete {
        path: PathBuf,
        is_dir: bool,
        size: u64,
    },
    Export {
        fmt: String,
        path: PathBuf,
        redact: bool,
        overwrite: bool,
    },
    RescanSubtree {
        path: PathBuf,
    },
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(stdout(), DisableMouseCapture);
        let _ = crossterm::execute!(stdout(), crossterm::terminal::LeaveAlternateScreen);
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

struct TreeRow {
    display_name: String,
    path: PathBuf,
    size: String,
    allocated: String,
    pct_parent: String,
    pct_disk: String,
    bar: String,
    files: String,
    folders: String,
    ext: String,
    modified: String,
    owner: String,
}

pub struct App {
    start_path: PathBuf,
    scan_root: PathBuf,
    scanner: DirectoryScanner,
    tree_state: TreeState,
    sort_key: SortKey,
    sort_reverse: bool,
    filter_text: String,
    filter_match_paths: std::collections::HashSet<PathBuf>,
    view_mode: ViewMode,
    expanded_paths: std::collections::HashSet<PathBuf>,
    follow_symlinks: bool,
    show_hidden: bool,
    scan_in_progress: bool,
    scan_started_at: Option<Instant>,
    volume_total_bytes: Option<u64>,
    scan_rate_tracker: RateTracker,
    progress_display: ProgressDisplay,
    cancel_requested: bool,
    volumes: Vec<VolumeInfo>,
    table_state: TableState,
    status_message: String,
    notification: Option<(String, bool)>,
    modal: Modal,
    pending_action: Option<PendingAction>,
    last_tree_refresh: Instant,
    last_status: Instant,
    last_progress_snapshot: Option<ProgressSnapshot>,
    scan_bridge: Option<ScanBridge>,
    fda_banner: Option<String>,
    should_quit: bool,
    tree_rows: Vec<TreeRow>,
    alt_lines: Vec<String>,
    show_filter_bar: bool,
    fda_checked: bool,
    active_cancel: Option<Arc<std::sync::atomic::AtomicBool>>,
    dirty: bool,
    notification_until: Option<Instant>,
    saved_cursor_path: Option<PathBuf>,
    last_chart_width: usize,
    alt_scroll: u16,
    alt_viewport_rows: u16,
    theme: Theme,
    /// Index into [`menu::MENUS`] of the open dropdown, if any.
    open_menu: Option<usize>,
    /// Click rectangles recomputed each render: menu-bar titles → menu index.
    menu_hitboxes: Vec<(Rect, usize)>,
    /// Toolbar buttons → action.
    toolbar_hitboxes: Vec<(Rect, Action)>,
    /// Open dropdown items → action (empty when no menu is open).
    dropdown_hitboxes: Vec<(Rect, Action)>,
    /// Region where table data rows are drawn, for click-to-select.
    table_rows_area: Rect,
    /// Visible data-row count of the table, for PageUp/PageDown.
    table_viewport_rows: u16,
}

impl App {
    pub fn new(start_path: Option<PathBuf>, theme: Theme) -> Self {
        let start = start_path.unwrap_or_else(default_scan_path);
        Self {
            start_path: start.clone(),
            scan_root: PathBuf::new(),
            scanner: DirectoryScanner::new(),
            tree_state: TreeState::default(),
            sort_key: SortKey::Size,
            sort_reverse: true,
            filter_text: String::new(),
            filter_match_paths: std::collections::HashSet::new(),
            view_mode: ViewMode::Tree,
            expanded_paths: std::collections::HashSet::new(),
            follow_symlinks: false,
            show_hidden: false,
            scan_in_progress: false,
            scan_started_at: None,
            volume_total_bytes: None,
            scan_rate_tracker: RateTracker::new(),
            progress_display: ProgressDisplay::default(),
            cancel_requested: false,
            volumes: Vec::new(),
            table_state: TableState::default(),
            status_message: "Ready. Checking Full Disk Access…".to_string(),
            notification: None,
            modal: Modal::None,
            pending_action: None,
            last_tree_refresh: Instant::now(),
            last_status: Instant::now(),
            last_progress_snapshot: None,
            scan_bridge: None,
            fda_banner: None,
            should_quit: false,
            tree_rows: Vec::new(),
            alt_lines: Vec::new(),
            show_filter_bar: false,
            fda_checked: false,
            active_cancel: None,
            dirty: true,
            notification_until: None,
            saved_cursor_path: None,
            last_chart_width: 28,
            alt_scroll: 0,
            alt_viewport_rows: 24,
            theme,
            open_menu: None,
            menu_hitboxes: Vec::new(),
            toolbar_hitboxes: Vec::new(),
            dropdown_hitboxes: Vec::new(),
            table_rows_area: Rect::default(),
            table_viewport_rows: 24,
        }
    }

    fn scroll_alt_view(&mut self, delta: i32) {
        let visible = self.alt_viewport_rows.max(1) as usize;
        let max_scroll = self.alt_lines.len().saturating_sub(visible) as i64;
        let next = (self.alt_scroll as i64 + delta as i64).clamp(0, max_scroll);
        self.alt_scroll = next as u16;
    }

    pub fn run(&mut self) -> io::Result<()> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            stdout(),
            crossterm::terminal::EnterAlternateScreen,
            EnableMouseCapture
        )?;
        let _guard = TerminalGuard;
        let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

        self.check_fda_and_start();

        let tick_rate = Duration::from_millis(100);
        let mut last_tick = Instant::now();

        loop {
            self.poll_scan_messages();
            self.maybe_refresh_tree();
            self.expire_notification();

            let tick_elapsed = last_tick.elapsed() >= tick_rate;
            if tick_needs_redraw(
                self.dirty,
                tick_elapsed,
                self.scan_in_progress,
                self.notification_until.is_some(),
            ) {
                terminal.draw(|f| self.render(f))?;
                self.dirty = false;
            }

            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if event::poll(timeout)? {
                // Drain the whole input backlog in one pass, then redraw once.
                // Processing a single event per tick makes held arrow keys and
                // queued mouse events feel laggy when a tick is up to 100ms.
                let mut processed = 0;
                loop {
                    match event::read()? {
                        Event::Key(key) => {
                            self.handle_key(key);
                            self.dirty = true;
                        }
                        Event::Mouse(me) => {
                            self.handle_mouse(me);
                            self.dirty = true;
                        }
                        Event::Resize(_, _) => {
                            self.dirty = true;
                        }
                        _ => {}
                    }
                    processed += 1;
                    if self.should_quit
                        || processed >= MAX_EVENTS_PER_TICK
                        || !event::poll(Duration::from_secs(0))?
                    {
                        break;
                    }
                }
            }

            if tick_elapsed {
                last_tick = Instant::now();
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn expire_notification(&mut self) {
        if let Some(until) = self.notification_until {
            if Instant::now() >= until {
                self.notification = None;
                self.notification_until = None;
                self.mark_dirty();
            }
        }
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn maybe_refresh_tree(&mut self) {
        if !self.tree_state.dirty {
            return;
        }
        let now = Instant::now();
        let interval = if self.scan_in_progress {
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

    fn cancel_active_scan(&mut self) {
        if let Some(cancel) = &self.active_cancel {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn check_fda_and_start(&mut self) {
        let result = check_full_disk_access();
        self.fda_checked = true;
        self.start_scan(self.start_path.clone());
        if !result.has_access || result.inconclusive {
            self.fda_banner = Some(format!(
                "{} — Scan is running. [o] Open Settings  [Esc] Dismiss",
                result.message
            ));
        }
    }

    fn validate_scan_path(&self, path: &Path) -> Option<String> {
        if !path.exists() {
            return Some(format!("Path does not exist: {}", path.display()));
        }
        if path.is_dir() {
            if nix::unistd::access(path, nix::unistd::AccessFlags::X_OK).is_err() {
                return Some(format!("Path is not traversable: {}", path.display()));
            }
        } else if nix::unistd::access(path, nix::unistd::AccessFlags::R_OK).is_err() {
            return Some(format!("Path is not readable: {}", path.display()));
        }
        None
    }

    fn start_scan(&mut self, path: PathBuf) {
        if let Some(err) = self.validate_scan_path(&path) {
            self.notify(err.clone(), true);
            self.status_message = err;
            return;
        }

        self.cancel_active_scan();
        let path = expand_abs(&path);
        self.start_path = path.clone();
        self.scan_root = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        self.expanded_paths.clear();
        self.expanded_paths.insert(path.clone());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        let root = ScanNode::new(name, path.clone(), true);
        self.tree_state.set_root(root);
        self.begin_scan_session(format!("Scanning {}", path.display()), true);
        self.spawn_scan_worker(true, None);
        self.rebuild_view();
        self.mark_dirty();
    }

    fn start_rescan(&mut self, subtree_path: PathBuf) {
        self.cancel_active_scan();
        self.begin_scan_session(format!("Rescanning {}", subtree_path.display()), false);
        self.spawn_scan_worker(false, Some(subtree_path));
        self.mark_dirty();
    }

    fn begin_scan_session(&mut self, status: String, use_volume_progress: bool) {
        self.scan_in_progress = true;
        self.scan_started_at = Some(Instant::now());
        self.scan_rate_tracker.reset();
        self.last_progress_snapshot = None;
        self.progress_display = ProgressDisplay::default();
        self.cancel_requested = false;
        self.volume_total_bytes = if use_volume_progress {
            volume_total_for_full_scan(&self.scan_root)
        } else {
            None
        };
        self.status_message = format!("{status} (press c to cancel)");
    }

    fn end_scan_session(&mut self) {
        self.scan_started_at = None;
        self.volume_total_bytes = None;
        self.scan_rate_tracker.reset();
        self.last_progress_snapshot = None;
        self.progress_display = ProgressDisplay::default();
        self.cancel_requested = false;
    }

    fn finalize_scan_session(&mut self, progress: ScanProgress, full_scan: bool) {
        let progress = if progress.is_complete {
            progress
        } else {
            self.last_progress_snapshot
                .as_ref()
                .map(snapshot_to_scan_progress)
                .unwrap_or(progress)
        };
        self.end_scan_session();

        if progress.cancelled {
            self.status_message = if full_scan {
                "Scan cancelled (partial results)".to_string()
            } else {
                "Rescan cancelled (partial results)".to_string()
            };
            self.recompute_filter_matches();
            self.rebuild_view();
            self.maybe_show_scan_errors(&progress);
            return;
        }

        if full_scan {
            self.auto_expand_tree();
        }
        self.recompute_filter_matches();
        self.rebuild_view();
        self.update_status_bar();
        self.maybe_show_scan_errors(&progress);
    }

    fn spawn_scan_worker(&mut self, full: bool, subtree_path: Option<PathBuf>) {
        self.cancel_active_scan();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.active_cancel = Some(cancel.clone());
        self.scanner.cancel = cancel.clone();

        let (tx, rx) = mpsc::channel();
        self.scan_bridge = Some(ScanBridge::new(rx));

        let follow_symlinks = self.follow_symlinks;
        let show_hidden = self.show_hidden;
        let start_path = self.start_path.clone();
        let root_snapshot = self.tree_state.root.clone();
        let scan_root = self.scan_root.clone();

        std::thread::spawn(move || {
            let mut scanner = DirectoryScanner {
                follow_symlinks,
                show_hidden,
                cancel,
                scan_root,
                ..DirectoryScanner::new()
            };

            let progress_cb: ProgressCallback = Arc::new({
                let tx = tx.clone();
                move |p| {
                    let _ = tx.send(ScanMessage::Progress(p));
                }
            });

            let patch_cb: PatchCallback = Arc::new({
                let tx = tx.clone();
                move |patch| {
                    let _ = tx.send(ScanMessage::TreePatch(patch));
                }
            });

            if full {
                let (root, progress) =
                    scanner.scan(&start_path, Some(progress_cb.clone()), Some(patch_cb));
                let _ = tx.send(ScanMessage::Complete { root, progress });
            } else if let Some(path) = subtree_path {
                if let Some(mut root) = root_snapshot {
                    let (ok, progress) = scanner.rescan_subtree_in_tree(
                        &mut root,
                        &path,
                        Some(progress_cb.clone()),
                        Some(patch_cb),
                    );
                    if ok {
                        let _ = tx.send(ScanMessage::RescanComplete {
                            path,
                            root,
                            progress,
                        });
                    } else {
                        let _ = tx.send(ScanMessage::Complete { root, progress });
                    }
                } else {
                    let _ = tx.send(ScanMessage::Error(
                        "Rescan failed: no scan data loaded".to_string(),
                    ));
                }
            } else {
                let _ = tx.send(ScanMessage::Error("Scan worker: invalid mode".to_string()));
            }
        });
        self.mark_dirty();
    }

    fn poll_scan_messages(&mut self) {
        let Some(bridge) = &self.scan_bridge else {
            return;
        };
        let batch = bridge.poll(MAX_MESSAGES_PER_POLL);
        if batch.is_empty() {
            return;
        }

        if let Some(snapshot) = batch.progress {
            self.on_scan_progress(snapshot);
            self.mark_dirty();
        }

        for (_, patch) in batch.patches {
            self.tree_state.apply_patch(patch, &self.scanner);
            self.mark_dirty();
        }

        if let Some(terminal) = batch.terminal {
            match terminal {
                crate::scan_bridge::TerminalScanEvent::Complete {
                    root,
                    progress,
                    full_scan,
                } => {
                    self.tree_state.set_root(*root);
                    self.finalize_scan_session(progress, full_scan);
                    self.scan_in_progress = false;
                    self.scan_bridge = None;
                    self.active_cancel = None;
                    self.mark_dirty();
                }
                crate::scan_bridge::TerminalScanEvent::Error(e) => {
                    self.end_scan_session();
                    self.status_message = e.clone();
                    self.notify(e, true);
                    self.scan_in_progress = false;
                    self.scan_bridge = None;
                    self.active_cancel = None;
                    self.mark_dirty();
                }
            }
        }
    }

    fn on_scan_progress(&mut self, snapshot: ProgressSnapshot) {
        self.progress_display.update_from_snapshot(&snapshot);
        self.last_progress_snapshot = Some(snapshot);
        self.scan_rate_tracker.record(
            self.progress_display.bytes_scanned,
            self.progress_display.scanned_items,
        );

        let now = Instant::now();
        if now.duration_since(self.last_status) >= Duration::from_millis(500) {
            self.last_status = now;
            if self.cancel_requested && !self.progress_display.worker_cancelled {
                return;
            }
            self.status_message = self.progress_display.status_line(self.cancel_requested);
        }
    }

    fn maybe_show_scan_errors(&mut self, progress: &ScanProgress) {
        if !progress.errors.is_empty() {
            self.modal = Modal::ScanErrors {
                errors: progress.errors.clone(),
                scroll: 0,
            };
        }
    }

    fn notify(&mut self, msg: String, is_error: bool) {
        let duration = if is_error {
            Duration::from_secs(8)
        } else {
            Duration::from_secs(4)
        };
        self.notification = Some((msg, is_error));
        self.notification_until = Some(Instant::now() + duration);
        self.mark_dirty();
    }

    fn update_status_bar(&mut self) {
        let Some(ref root) = self.tree_state.root else {
            return;
        };
        let hidden = if self.show_hidden { "ON" } else { "OFF" };
        let symlinks = if self.follow_symlinks { "ON" } else { "OFF" };
        let dir = if self.sort_reverse { "↓" } else { "↑" };
        self.status_message = format!(
            "Path: {} | Total: {} | Files: {:>6} | Folders: {:>6} | Hidden: {hidden} | Follow symlinks: {symlinks} | Sort: {} {dir}",
            root.path.display(),
            format_bytes(root.size as i64),
            root.file_count,
            root.folder_count,
            self.sort_key.label()
        );
    }

    fn recompute_filter_matches(&mut self) {
        if self.filter_text.is_empty() {
            self.filter_match_paths.clear();
            return;
        }
        let Some(ref root) = self.tree_state.root else {
            return;
        };
        let matches = root.filter_by_name(&self.filter_text, false);
        let mut paths = std::collections::HashSet::new();
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

    fn auto_expand_tree(&mut self) {
        if let Some(ref root) = self.tree_state.root {
            self.expanded_paths.insert(root.path.clone());
            for child in &root.children {
                if child.is_dir {
                    self.expanded_paths.insert(child.path.clone());
                }
            }
        }
    }

    fn rebuild_view(&mut self) {
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

    fn fill_visible_metadata(&mut self) {
        let Some(root) = self.tree_state.root.as_mut() else {
            return;
        };
        fn fill_expanded(node: &mut ScanNode, expanded: &std::collections::HashSet<PathBuf>) {
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

    fn build_tree_view(&mut self) {
        self.tree_rows.clear();
        self.fill_visible_metadata();
        let Some(ref root) = self.tree_state.root else {
            return;
        };

        fn walk(
            node: &ScanNode,
            parent: Option<&ScanNode>,
            root_size: u64,
            prefix: &str,
            is_last: bool,
            is_root: bool,
            expanded: &std::collections::HashSet<PathBuf>,
            filter: &str,
            filter_paths: &std::collections::HashSet<PathBuf>,
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

    fn build_top_files_view(&mut self) {
        self.alt_lines.clear();
        let Some(ref root) = self.tree_state.root else {
            self.alt_lines.push("No scan data".to_string());
            return;
        };
        let files = collect_largest_files(root, 100);
        let total: u64 = files.iter().map(|f| f.size).sum::<u64>().max(1);
        let items: Vec<(String, u64)> = files
            .iter()
            .take(20)
            .map(|f| {
                (
                    f.path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default(),
                    f.size,
                )
            })
            .collect();

        self.alt_lines.push("Top 100 Largest Files".to_string());
        self.alt_lines
            .push("Labeled size chart (top 20)".to_string());
        self.alt_lines.push(String::new());
        for line in ascii_bar_chart(&items, 32, Some(total)) {
            self.alt_lines.push(line);
        }
        self.alt_lines.push(String::new());
        for (i, f) in files.iter().enumerate() {
            let path_display = truncate_chars(&f.path.display().to_string(), 56);
            self.alt_lines.push(format!(
                "{:>3}. {:>10}  ({:>5.1}%)  {path_display}",
                i + 1,
                format_bytes(f.size as i64),
                f.size as f64 / total as f64 * 100.0,
            ));
        }
        self.alt_lines
            .push(format!("Total files scanned: {}", root.file_count));
    }

    fn build_extensions_view(&mut self) {
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

    fn build_volumes_view(&mut self) {
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

    fn get_selected_node(&self) -> Option<&ScanNode> {
        if self.view_mode != ViewMode::Tree {
            return None;
        }
        let idx = self.table_state.selected()?;
        let path = self.tree_rows.get(idx)?.path.clone();
        self.tree_state.root.as_ref()?.find_by_path(&path)
    }

    fn get_selected_volume(&self) -> Option<&VolumeInfo> {
        if self.view_mode != ViewMode::Volumes {
            return None;
        }
        let idx = self.table_state.selected()?;
        let path = self.tree_rows.get(idx)?.path.clone();
        self.volumes.iter().find(|v| v.mount_point == path)
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.handle_modal_key(key) {
            return;
        }

        // The Full Disk Access banner intercepts a couple of keys before normal
        // dispatch: 'o' opens System Settings, Esc dismisses the banner.
        if self.fda_banner.is_some() {
            match key.code {
                KeyCode::Char('o') => {
                    let _ = open_fda_settings();
                    return;
                }
                KeyCode::Esc => {
                    self.fda_banner = None;
                    return;
                }
                _ => {}
            }
        }

        // Esc is contextual and not part of the shared action vocabulary: it
        // closes an open menu first, otherwise clears the active filter.
        if matches!(key.code, KeyCode::Esc) {
            if self.open_menu.is_some() {
                self.open_menu = None;
            } else {
                self.dispatch_action(Action::ClearFilter);
            }
            return;
        }

        if let Some(action) = menu::key_to_action(key) {
            self.open_menu = None;
            self.dispatch_action(action);
        }
    }

    /// Single entry point for every user intent — keyboard, mouse, menu, and
    /// toolbar all route here, so each action has exactly one implementation.
    fn dispatch_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Help => self.modal = Modal::Help,
            Action::OpenFilter => {
                self.modal = Modal::PathInput {
                    prompt: "Filter by name (substring):".to_string(),
                    input: self.filter_text.clone(),
                    purpose: PathInputPurpose::Filter,
                    selected: 0,
                };
            }
            Action::ClearFilter => {
                self.filter_text.clear();
                self.filter_match_paths.clear();
                self.show_filter_bar = false;
                self.rebuild_view();
            }
            Action::CycleSort => {
                let keys = SortKey::all();
                let idx = keys.iter().position(|&k| k == self.sort_key).unwrap_or(0);
                self.sort_key = keys[(idx + 1) % keys.len()];
                self.rebuild_view();
                self.update_status_bar();
            }
            Action::ToggleSortDir => {
                self.sort_reverse = !self.sort_reverse;
                self.rebuild_view();
                self.update_status_bar();
            }
            Action::SetSort(key) => {
                self.sort_key = key;
                self.rebuild_view();
                self.update_status_bar();
            }
            Action::RescanTree => {
                if let Some(ref root) = self.tree_state.root {
                    self.start_scan(root.path.clone());
                }
            }
            Action::RescanSelected => {
                if self.scan_in_progress {
                    self.notify(
                        "Scan in progress — press c to cancel first".to_string(),
                        false,
                    );
                    return;
                }
                if let Some(node) = self.get_selected_node() {
                    if node.is_dir {
                        self.start_rescan(node.path.clone());
                    }
                } else if let Some(ref root) = self.tree_state.root {
                    self.start_scan(root.path.clone());
                }
            }
            Action::CancelScan => {
                if self.scan_in_progress {
                    self.cancel_active_scan();
                    self.cancel_requested = true;
                    self.status_message = "Cancelling scan…".to_string();
                    self.mark_dirty();
                }
            }
            Action::GotoPath => {
                self.modal = Modal::PathInput {
                    prompt: "Scan path:".to_string(),
                    input: self.start_path.to_string_lossy().to_string(),
                    purpose: PathInputPurpose::Goto,
                    selected: 0,
                };
            }
            Action::ToggleSymlinks => {
                self.follow_symlinks = !self.follow_symlinks;
                self.update_status_bar();
                if let Some(ref root) = self.tree_state.root {
                    self.start_scan(root.path.clone());
                }
            }
            Action::ToggleHidden => {
                self.show_hidden = !self.show_hidden;
                self.update_status_bar();
                if let Some(ref root) = self.tree_state.root {
                    self.start_scan(root.path.clone());
                }
            }
            // Open the theme picker. Navigating it live-previews each theme;
            // Enter keeps the choice, Esc restores the theme shown here.
            Action::ThemePicker => {
                let selected = crate::theme::THEMES
                    .iter()
                    .position(|t| t.name == self.theme.name)
                    .unwrap_or(0);
                self.modal = Modal::ThemePicker {
                    selected,
                    original: self.theme,
                };
            }
            Action::NextView => self.cycle_view(1),
            Action::PrevView => self.cycle_view(-1),
            Action::SetView(view) => {
                self.view_mode = view;
                if matches!(self.view_mode, ViewMode::TopFiles | ViewMode::Extensions) {
                    self.alt_scroll = 0;
                }
                self.rebuild_view();
            }
            Action::Collapse => self.collapse_selected(),
            Action::Expand => self.expand_selected(),
            Action::ToggleExpandOrScan => self.toggle_expand_or_scan(),
            Action::MoveDown => self.scroll_main(1),
            Action::MoveUp => self.scroll_main(-1),
            Action::PageDown => self.scroll_main(self.page_step()),
            Action::PageUp => self.scroll_main(-self.page_step()),
            Action::Home => self.jump_to_edge(true),
            Action::End => self.jump_to_edge(false),
            Action::RevealFinder => self.reveal_finder(),
            Action::Delete => self.delete_selected(),
            Action::Export => {
                if self.tree_state.root.is_some() {
                    if let Some(ref root) = self.tree_state.root {
                        if let Some(w) = export_warning(root) {
                            self.notify(w, false);
                        }
                    }
                    self.modal = Modal::Export {
                        path_input: format!("{}/filetree-report.txt", dirs_home().display()),
                        selected: 0,
                    };
                } else {
                    self.notify("Nothing to export".to_string(), false);
                }
            }
        }
    }

    fn cycle_view(&mut self, delta: i32) {
        let len = VIEW_CYCLE.len() as i32;
        let idx = VIEW_CYCLE
            .iter()
            .position(|&v| v == self.view_mode)
            .unwrap_or(0) as i32;
        let next = ((idx + delta) % len + len) % len;
        self.view_mode = VIEW_CYCLE[next as usize];
        if matches!(self.view_mode, ViewMode::TopFiles | ViewMode::Extensions) {
            self.alt_scroll = 0;
        }
        self.rebuild_view();
    }

    /// One page of movement for PageUp/PageDown, based on the visible row count.
    fn page_step(&self) -> i32 {
        if matches!(self.view_mode, ViewMode::TopFiles | ViewMode::Extensions) {
            self.alt_viewport_rows.max(1) as i32
        } else {
            self.table_viewport_rows.max(1) as i32
        }
    }

    /// Move the cursor (table) or scroll (chart/list views) by `delta` rows.
    fn scroll_main(&mut self, delta: i32) {
        if matches!(self.view_mode, ViewMode::TopFiles | ViewMode::Extensions) {
            self.scroll_alt_view(delta);
        } else {
            self.move_cursor(delta);
        }
        self.mark_dirty();
    }

    fn jump_to_edge(&mut self, to_start: bool) {
        if matches!(self.view_mode, ViewMode::TopFiles | ViewMode::Extensions) {
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

    fn handle_mouse(&mut self, me: MouseEvent) {
        // Modals are keyboard-driven; ignore mouse while one is open.
        if !matches!(self.modal, Modal::None) {
            return;
        }
        match me.kind {
            MouseEventKind::ScrollDown => self.scroll_main(3),
            MouseEventKind::ScrollUp => self.scroll_main(-3),
            MouseEventKind::Down(MouseButton::Left) => {
                self.handle_left_click(me.column, me.row);
            }
            _ => {}
        }
    }

    fn handle_left_click(&mut self, col: u16, row: u16) {
        let point = (col, row);

        // An open dropdown captures clicks first: an item fires, a menu title
        // toggles, anything else dismisses the menu.
        if self.open_menu.is_some() {
            if let Some(action) = hit_action(&self.dropdown_hitboxes, point) {
                self.open_menu = None;
                self.dispatch_action(action);
                return;
            }
            if let Some(idx) = hit_index(&self.menu_hitboxes, point) {
                self.toggle_menu(idx);
                return;
            }
            self.open_menu = None;
            return;
        }

        if let Some(idx) = hit_index(&self.menu_hitboxes, point) {
            self.toggle_menu(idx);
            return;
        }

        if let Some(action) = hit_action(&self.toolbar_hitboxes, point) {
            self.dispatch_action(action);
            return;
        }

        if rect_contains(self.table_rows_area, point) {
            let rel = row.saturating_sub(self.table_rows_area.y) as usize;
            let idx = self.table_state.offset() + rel;
            if idx < self.tree_rows.len() {
                let already_selected = self.table_state.selected() == Some(idx);
                self.table_state.select(Some(idx));
                // A second click on the already-selected row acts like Enter:
                // expand/collapse the folder, or scan the volume.
                if already_selected {
                    self.toggle_expand_or_scan();
                }
                self.mark_dirty();
            }
        }
    }

    fn toggle_menu(&mut self, idx: usize) {
        self.open_menu = if self.open_menu == Some(idx) {
            None
        } else {
            Some(idx)
        };
        self.mark_dirty();
    }

    #[allow(clippy::needless_return)]
    fn handle_modal_key(&mut self, key: KeyEvent) -> bool {
        match &self.modal {
            Modal::None => return false,
            Modal::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?')) {
                    self.modal = Modal::None;
                }
                return true;
            }
            Modal::Confirm { .. } => {
                match key.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        if let Modal::Confirm { selected, .. } = &mut self.modal {
                            *selected = 0;
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if let Modal::Confirm { selected, .. } = &mut self.modal {
                            *selected = 1;
                        }
                    }
                    KeyCode::Enter => {
                        let selected = if let Modal::Confirm { selected, .. } = &self.modal {
                            *selected
                        } else {
                            1
                        };
                        self.modal = Modal::None;
                        if confirm_dialog_executes(selected) {
                            self.execute_pending_action();
                        } else {
                            self.pending_action = None;
                        }
                    }
                    KeyCode::Esc => {
                        self.modal = Modal::None;
                        self.pending_action = None;
                    }
                    _ => {}
                }
                return true;
            }
            Modal::TypedConfirm { .. } => {
                match key.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        if let Modal::TypedConfirm { selected, .. } = &mut self.modal {
                            *selected = 0;
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if let Modal::TypedConfirm { selected, .. } = &mut self.modal {
                            *selected = 1;
                        }
                    }
                    KeyCode::Enter => {
                        let (confirm_text, input_val, selected) = if let Modal::TypedConfirm {
                            confirm_text,
                            input,
                            selected,
                            ..
                        } = &self.modal
                        {
                            (confirm_text.clone(), input.clone(), *selected)
                        } else {
                            return true;
                        };
                        if typed_confirm_executes(selected, &input_val, &confirm_text) {
                            self.modal = Modal::None;
                            self.execute_pending_action();
                        } else if selected == 1 {
                            self.modal = Modal::None;
                            self.pending_action = None;
                        }
                    }
                    KeyCode::Esc => {
                        self.modal = Modal::None;
                        self.pending_action = None;
                    }
                    KeyCode::Backspace => {
                        if let Modal::TypedConfirm { input, .. } = &mut self.modal {
                            input.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Modal::TypedConfirm { input, .. } = &mut self.modal {
                            input.push(c);
                        }
                    }
                    _ => {}
                }
                return true;
            }
            Modal::PathInput { .. } => {
                match key.code {
                    KeyCode::Enter => {
                        let (purpose, val) =
                            if let Modal::PathInput { input, purpose, .. } = &self.modal {
                                (*purpose, input.clone())
                            } else {
                                return true;
                            };
                        self.modal = Modal::None;
                        match purpose {
                            PathInputPurpose::Goto if !val.is_empty() => {
                                self.start_scan(PathBuf::from(val));
                            }
                            PathInputPurpose::Filter => {
                                self.filter_text = val;
                                self.show_filter_bar = !self.filter_text.is_empty();
                                self.recompute_filter_matches();
                                self.rebuild_view();
                            }
                            _ => {}
                        }
                    }
                    KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Backspace => {
                        if let Modal::PathInput { input, .. } = &mut self.modal {
                            input.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Modal::PathInput { input, .. } = &mut self.modal {
                            input.push(c);
                        }
                    }
                    _ => {}
                }
                return true;
            }
            Modal::Export { .. } => {
                match key.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        if let Modal::Export { selected, .. } = &mut self.modal {
                            *selected = selected.saturating_sub(1);
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if let Modal::Export { selected, .. } = &mut self.modal {
                            *selected = (*selected + 1).min(3);
                        }
                    }
                    KeyCode::Enter => {
                        let (path, sel) = if let Modal::Export {
                            path_input,
                            selected,
                        } = &self.modal
                        {
                            (path_input.clone(), *selected)
                        } else {
                            return true;
                        };
                        let path = expand_user_path(Path::new(&path));
                        self.modal = Modal::None;
                        if let Some((fmt, redact)) = export_modal_selection(sel) {
                            self.pending_action = Some(PendingAction::Export {
                                fmt: fmt.to_string(),
                                path,
                                redact,
                                overwrite: false,
                            });
                            self.execute_pending_action();
                        }
                    }
                    KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Backspace => {
                        if let Modal::Export { path_input, .. } = &mut self.modal {
                            path_input.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Modal::Export { path_input, .. } = &mut self.modal {
                            path_input.push(c);
                        }
                    }
                    _ => {}
                }
                return true;
            }
            Modal::ScanErrors { .. } => {
                match key.code {
                    KeyCode::Enter | KeyCode::Esc => self.modal = Modal::None,
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Modal::ScanErrors { scroll, .. } = &mut self.modal {
                            *scroll += 1;
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Modal::ScanErrors { scroll, .. } = &mut self.modal {
                            *scroll = scroll.saturating_sub(1);
                        }
                    }
                    _ => {}
                }
                return true;
            }
            Modal::Message { .. } => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                    self.modal = Modal::None;
                }
                return true;
            }
            Modal::ThemePicker { .. } => {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Modal::ThemePicker { selected, .. } = &mut self.modal {
                            *selected = selected.saturating_sub(1);
                            self.theme = crate::theme::THEMES[*selected];
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Modal::ThemePicker { selected, .. } = &mut self.modal {
                            let len = crate::theme::THEMES.len();
                            *selected = (*selected + 1).min(len.saturating_sub(1));
                            self.theme = crate::theme::THEMES[*selected];
                        }
                    }
                    KeyCode::Enter => {
                        self.modal = Modal::None;
                    }
                    KeyCode::Esc => {
                        if let Modal::ThemePicker { original, .. } = &self.modal {
                            self.theme = *original;
                        }
                        self.modal = Modal::None;
                    }
                    _ => {}
                }
                return true;
            }
        }
    }

    fn execute_pending_action(&mut self) {
        let action = self.pending_action.take();
        let Some(action) = action else { return };

        match action {
            PendingAction::Delete {
                path,
                is_dir,
                size: _,
            } => {
                let (target, error) = safe_delete_target(&path, &self.scan_root, is_dir);
                if let Some(err) = error {
                    self.notify(format!("Delete blocked: {err}"), true);
                    return;
                }
                let result = if is_dir {
                    std::fs::remove_dir_all(&target)
                } else {
                    std::fs::remove_file(&target)
                };
                match result {
                    Ok(()) => {
                        self.notify(format!("Deleted: {}", target.display()), false);
                        if let Some(parent_path) = target.parent().map(|p| p.to_path_buf()) {
                            if self.expanded_paths.contains(&parent_path) {
                                self.start_rescan(parent_path);
                            } else if let Some(ref root) = self.tree_state.root {
                                self.start_scan(root.path.clone());
                            }
                        } else if let Some(ref root) = self.tree_state.root {
                            self.start_scan(root.path.clone());
                        }
                        self.rebuild_view();
                    }
                    Err(e) => self.notify(format!("Delete failed: {e}"), true),
                }
            }
            PendingAction::Export {
                fmt,
                path,
                redact,
                overwrite,
            } => {
                let Some(ref root) = self.tree_state.root else {
                    return;
                };
                if is_sensitive_export_path(&path) && !overwrite {
                    self.pending_action = Some(PendingAction::Export {
                        fmt,
                        path: path.clone(),
                        redact,
                        overwrite: false,
                    });
                    self.modal = Modal::Confirm {
                        title: "Export Warning".to_string(),
                        message: format!("Export to a sensitive location?\n\n{}", path.display()),
                        selected: 0,
                    };
                    return;
                }
                match save_report(root, &path, &fmt, overwrite, &self.scan_root, redact) {
                    Ok(saved) => self.notify(format!("Exported to {}", saved.display()), false),
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        let display = path.display().to_string();
                        self.pending_action = Some(PendingAction::Export {
                            fmt,
                            path,
                            redact,
                            overwrite: true,
                        });
                        self.modal = Modal::Confirm {
                            title: "Export Overwrite".to_string(),
                            message: format!("Overwrite existing file?\n\n{display}"),
                            selected: 0,
                        };
                    }
                    Err(e) => self.notify(format!("Export failed: {e}"), true),
                }
            }
            PendingAction::RescanSubtree { path } => self.start_rescan(path),
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        if self.tree_rows.is_empty() {
            return;
        }
        let selected = self.table_state.selected().unwrap_or(0) as i64;
        let max = self.tree_rows.len() as i64 - 1;
        let new = (selected + delta as i64).clamp(0, max);
        self.table_state.select(Some(new as usize));
    }

    fn expand_selected(&mut self) {
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

    fn collapse_selected(&mut self) {
        let path = self.selected_path();
        if let Some(path) = path {
            self.expanded_paths.remove(&path);
            self.rebuild_view();
        }
    }

    fn toggle_expand_or_scan(&mut self) {
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

    fn selected_path(&self) -> Option<PathBuf> {
        let idx = self.table_state.selected()?;
        self.tree_rows.get(idx).map(|r| r.path.clone())
    }

    fn reveal_finder(&mut self) {
        let path = self
            .get_selected_node()
            .map(|n| n.path.clone())
            .or_else(|| self.tree_state.root.as_ref().map(|r| r.path.clone()));
        let Some(path) = path else { return };
        if !self.scan_root.as_os_str().is_empty() && !self.path_allowed(&path) {
            self.notify("Cannot reveal path outside scan root".to_string(), true);
            return;
        }
        if path.exists() {
            let _ = std::process::Command::new("open")
                .args(["-R", &path.to_string_lossy()])
                .output();
        }
    }

    fn path_allowed(&self, path: &Path) -> bool {
        if self.scan_root.as_os_str().is_empty() {
            return true;
        }
        is_under_scan_root(path, &self.scan_root)
    }

    fn delete_selected(&mut self) {
        if self.scan_in_progress {
            self.notify("Cannot delete while scan is in progress".to_string(), true);
            return;
        }
        let Some(node) = self.get_selected_node().cloned() else {
            return;
        };
        let scan_root = if self.scan_root.as_os_str().is_empty() {
            node.path.clone()
        } else {
            self.scan_root.clone()
        };
        let (protected, reason) = is_delete_protected(&node.path, &scan_root);
        if protected {
            self.notify(reason, true);
            return;
        }

        let message = format!(
            "Permanently delete?\n\n{}\n\nSize: {}",
            node.path.display(),
            format_bytes(node.size as i64)
        );
        self.pending_action = Some(PendingAction::Delete {
            path: node.path.clone(),
            is_dir: node.is_dir,
            size: node.size,
        });

        if node.is_dir && node.size >= LARGE_DELETE_BYTES {
            let confirm_name = node
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "delete".to_string());
            self.modal = Modal::TypedConfirm {
                message: format!("{message}\n\nLarge directory — type the folder name to confirm."),
                confirm_text: confirm_name,
                input: String::new(),
                selected: 0,
            };
        } else {
            self.modal = Modal::Confirm {
                title: "Delete Item".to_string(),
                message,
                selected: 0,
            };
        }
    }

    fn render_scan_progress_panel(&self, f: &mut ratatui::Frame, area: Rect) {
        let display = &self.progress_display;
        let elapsed_secs = self
            .scan_started_at
            .map(|start| start.elapsed().as_secs_f64())
            .unwrap_or(0.0);
        let inner_width = scan_progress_inner_width(area.width);
        let pct_label = match self.volume_total_bytes {
            Some(total) if total > 0 => {
                let pct = crate::progress::scan_progress_percent(
                    display.bytes_scanned,
                    self.volume_total_bytes,
                    false,
                );
                format!("{pct}%")
            }
            _ => "scanning…".to_string(),
        };
        let panel = build_scan_progress_panel(ScanProgressPanelInput {
            bytes_scanned: display.bytes_scanned,
            scanned_items: display.scanned_items,
            scanned_dirs: display.scanned_dirs,
            current_path: &display.current_path,
            error_count: display.error_count,
            volume_total: self.volume_total_bytes,
            elapsed_secs,
            rates: self.scan_rate_tracker.snapshot(),
            bar_width: scan_progress_bar_width(inner_width, &pct_label),
            path_max_chars: scan_progress_path_max_chars(inner_width),
            max_line_width: inner_width,
            complete: false,
            cancelled: scan_panel_cancelled(self.cancel_requested, display.worker_cancelled),
        });

        let mut lines = vec![
            Line::from(panel.bar_line),
            Line::from(panel.data_line),
            Line::from(panel.items_line),
            Line::from(panel.time_line),
            Line::from(panel.path_line),
        ];
        if let Some(err_line) = panel.errors_line {
            lines.push(Line::from(err_line));
        } else {
            lines.push(Line::from(""));
        }

        let widget =
            Paragraph::new(lines).block(Block::default().borders(Borders::TOP).title(panel.title));
        f.render_widget(widget, area);
    }

    fn render(&mut self, f: &mut ratatui::Frame) {
        let size = f.area();
        // Hit rectangles are rebuilt every frame; clear last frame's first so a
        // stale region can never be clicked.
        self.menu_hitboxes.clear();
        self.toolbar_hitboxes.clear();
        self.dropdown_hitboxes.clear();
        self.table_rows_area = Rect::default();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // menu bar
                Constraint::Length(1), // toolbar
                Constraint::Length(if self.show_filter_bar { 1 } else { 0 }),
                Constraint::Min(0),
                Constraint::Length(if self.scan_in_progress {
                    SCAN_PROGRESS_PANEL_LINES
                } else {
                    0
                }),
                Constraint::Length(3),
            ])
            .split(size);

        self.render_menu_bar(f, chunks[0]);
        self.render_toolbar(f, chunks[1]);

        if self.show_filter_bar {
            let filter_text = if self.filter_text.is_empty() {
                "Filter: none".to_string()
            } else {
                format!("Filter: {}", self.filter_text)
            };
            f.render_widget(
                Paragraph::new(filter_text).style(self.theme.filter_style()),
                chunks[2],
            );
        }

        match self.view_mode {
            ViewMode::Tree | ViewMode::Volumes => {
                let h_chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
                    .split(chunks[3]);
                self.render_table(f, h_chunks[0]);
                self.render_chart_panel(f, h_chunks[1]);
            }
            ViewMode::TopFiles | ViewMode::Extensions => {
                self.render_alt_view(f, chunks[3]);
            }
        }

        if self.scan_in_progress {
            self.render_scan_progress_panel(f, chunks[4]);
        }

        let status = if let Some((ref msg, _)) = self.notification {
            msg.clone()
        } else {
            self.status_message.clone()
        };
        let status_widget = Paragraph::new(status).wrap(Wrap { trim: true }).block(
            Block::default()
                .borders(Borders::TOP)
                .title("filetree")
                .border_style(self.theme.border_style()),
        );
        f.render_widget(status_widget, chunks[5]);

        if let Some(ref banner) = self.fda_banner {
            self.render_fda_banner(f, size, banner);
        }

        // The open dropdown floats above the content but below modal dialogs.
        if self.open_menu.is_some() {
            self.render_dropdown(f, size);
        }

        if !matches!(self.modal, Modal::None) {
            self.render_modal(f, size);
        }
    }

    fn render_menu_bar(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let cells = menu::menu_bar_cells();
        let (_, spans) = menu::render_row(&cells, 1);
        self.menu_hitboxes = spans
            .iter()
            .enumerate()
            .map(|(i, &(x, w))| {
                (
                    Rect {
                        x: area.x.saturating_add(x),
                        y: area.y,
                        width: w,
                        height: 1,
                    },
                    i,
                )
            })
            .collect();

        let mut line_spans = Vec::with_capacity(cells.len() * 2);
        for (i, cell) in cells.iter().enumerate() {
            if i > 0 {
                line_spans.push(Span::raw(" "));
            }
            let style = if self.open_menu == Some(i) {
                self.theme.selection_style()
            } else {
                self.theme.header_style()
            };
            line_spans.push(Span::styled(cell.clone(), style));
        }
        f.render_widget(
            Paragraph::new(Line::from(line_spans)).style(self.theme.filter_style()),
            area,
        );
    }

    fn render_toolbar(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let cells = menu::toolbar_cells();
        let (_, spans) = menu::render_row(&cells, 1);
        self.toolbar_hitboxes = spans
            .iter()
            .enumerate()
            .map(|(i, &(x, w))| {
                (
                    Rect {
                        x: area.x.saturating_add(x),
                        y: area.y,
                        width: w,
                        height: 1,
                    },
                    menu::TOOLBAR[i].action,
                )
            })
            .collect();

        let mut line_spans = Vec::with_capacity(cells.len() * 2);
        for (i, cell) in cells.iter().enumerate() {
            if i > 0 {
                line_spans.push(Span::raw(" "));
            }
            let active = menu::TOOLBAR[i].action == Action::SetView(self.view_mode);
            let style = if active {
                self.theme.selection_style()
            } else {
                self.theme.accent_style()
            };
            line_spans.push(Span::styled(cell.clone(), style));
        }
        f.render_widget(Paragraph::new(Line::from(line_spans)), area);
    }

    fn render_dropdown(&mut self, f: &mut ratatui::Frame, screen: Rect) {
        let Some(idx) = self.open_menu else {
            return;
        };
        let menu = &menu::MENUS[idx];
        let anchor_x = self
            .menu_hitboxes
            .iter()
            .find(|(_, i)| *i == idx)
            .map(|(r, _)| r.x)
            .unwrap_or(0);

        let width = menu::dropdown_width(menu).min(screen.width.max(1));
        let x = anchor_x.min(screen.width.saturating_sub(width));
        let y = 1u16; // directly under the menu bar
        let height = (menu.items.len() as u16 + 2).min(screen.height.saturating_sub(y).max(1));
        let area = Rect {
            x,
            y,
            width,
            height,
        };

        f.render_widget(Clear, area);

        let inner_width = area.width.saturating_sub(2) as usize;
        let visible_items = area.height.saturating_sub(2) as usize;
        let mut lines = Vec::with_capacity(visible_items);
        for (i, item) in menu.items.iter().take(visible_items).enumerate() {
            lines.push(Line::from(menu::dropdown_item_text(item, inner_width)));
            self.dropdown_hitboxes.push((
                Rect {
                    x: area.x.saturating_add(1),
                    y: area.y.saturating_add(1).saturating_add(i as u16),
                    width: area.width.saturating_sub(2),
                    height: 1,
                },
                item.action,
            ));
        }

        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(menu.title)
                .border_style(self.theme.accent_style()),
        );
        f.render_widget(widget, area);
    }

    fn render_fda_banner(&self, f: &mut ratatui::Frame, area: Rect, message: &str) {
        let banner_area = Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 3.min(area.height),
        };
        let p = Paragraph::new(truncate_chars(message, banner_area.width as usize))
            .style(self.theme.warning_style())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Full Disk Access")
                    .border_style(self.theme.warning_style()),
            )
            .wrap(Wrap { trim: true });
        f.render_widget(p, banner_area);
    }

    fn render_table(&mut self, f: &mut ratatui::Frame, area: Rect) {
        // Record where data rows land (inside the border, below the 1-row header)
        // so left-clicks can be mapped back to a row index.
        self.table_rows_area = Rect {
            x: area.x.saturating_add(1),
            y: area.y.saturating_add(2),
            width: area.width.saturating_sub(2),
            height: area.height.saturating_sub(3),
        };
        self.table_viewport_rows = self.table_rows_area.height;

        let (header, rows, constraints) = if self.view_mode == ViewMode::Volumes {
            let header = Row::new(vec![
                "Volume", "Mount", "Total", "Used", "Free", "Used %", "Bar",
            ]);
            let rows: Vec<Row> = self
                .tree_rows
                .iter()
                .map(|r| {
                    Row::new(vec![
                        r.display_name.as_str(),
                        r.files.as_str(),
                        r.size.as_str(),
                        r.allocated.as_str(),
                        r.pct_parent.as_str(),
                        r.pct_disk.as_str(),
                        r.bar.as_str(),
                    ])
                })
                .collect();
            let constraints = vec![
                Constraint::Length(12),
                Constraint::Min(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(8),
                Constraint::Length(12),
            ];
            (header, rows, constraints)
        } else {
            // Adaptive columns. "Name" (the file list) is the one column that must
            // never disappear, so it takes the leftover width via `Fill`; the
            // secondary columns are added in priority order only while they still
            // fit the pane. Previously Name was a `Percentage`, which ratatui's
            // layout solver squeezes to zero when the fixed `Length` columns
            // already fill a narrow pane — leaving a list of sizes with no names.
            #[derive(Clone, Copy)]
            enum Field {
                Size,
                PctParent,
                Bar,
                Files,
                Folders,
                PctDisk,
                Alloc,
                Modified,
                Ext,
                Owner,
            }
            const OPTIONAL: &[(&str, u16, Field)] = &[
                ("Size", 10, Field::Size),
                ("%Par", 7, Field::PctParent),
                ("Bar", 12, Field::Bar),
                ("Files", 7, Field::Files),
                ("Folders", 8, Field::Folders),
                ("%Disk", 7, Field::PctDisk),
                ("Alloc", 10, Field::Alloc),
                ("Modified", 16, Field::Modified),
                ("Ext", 6, Field::Ext),
                ("Owner", 12, Field::Owner),
            ];
            const SPACING: u16 = 1;

            let inner_w = self.table_rows_area.width;
            // Reserve a generous share for the file names before admitting any
            // metadata column, so Name stays readable rather than being whittled
            // down to a few characters once many optional columns happen to fit.
            let name_reserve = (inner_w * 3 / 10).max(24);
            let mut used = name_reserve + SPACING;
            let mut chosen: Vec<usize> = Vec::new();
            for (i, (_, w, _)) in OPTIONAL.iter().enumerate() {
                let add = w + SPACING;
                if used + add <= inner_w {
                    used += add;
                    chosen.push(i);
                } else {
                    break;
                }
            }

            let mut header_cells: Vec<&str> = Vec::with_capacity(chosen.len() + 1);
            header_cells.push("Name");
            let mut constraints: Vec<Constraint> = Vec::with_capacity(chosen.len() + 1);
            constraints.push(Constraint::Fill(1));
            for &i in &chosen {
                header_cells.push(OPTIONAL[i].0);
                constraints.push(Constraint::Length(OPTIONAL[i].1));
            }
            let header = Row::new(header_cells);

            let rows: Vec<Row> = self
                .tree_rows
                .iter()
                .map(|r| {
                    let mut cells: Vec<&str> = Vec::with_capacity(chosen.len() + 1);
                    cells.push(r.display_name.as_str());
                    for &i in &chosen {
                        cells.push(match OPTIONAL[i].2 {
                            Field::Size => r.size.as_str(),
                            Field::PctParent => r.pct_parent.as_str(),
                            Field::Bar => r.bar.as_str(),
                            Field::Files => r.files.as_str(),
                            Field::Folders => r.folders.as_str(),
                            Field::PctDisk => r.pct_disk.as_str(),
                            Field::Alloc => r.allocated.as_str(),
                            Field::Modified => r.modified.as_str(),
                            Field::Ext => r.ext.as_str(),
                            Field::Owner => r.owner.as_str(),
                        });
                    }
                    Row::new(cells)
                })
                .collect();

            (header, rows, constraints)
        };

        let title = match self.view_mode {
            ViewMode::Tree => "Directory tree",
            ViewMode::Volumes => "Volumes",
            _ => "View",
        };

        let table = Table::new(rows, constraints)
            .header(header.style(self.theme.header_style()))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(self.theme.border_style()),
            )
            .row_highlight_style(self.theme.selection_style());

        f.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_chart_panel(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let bar_width = chart_bar_width(area.width, 18);
        self.last_chart_width = bar_width;
        let mut lines = Vec::new();
        let title = match self.view_mode {
            ViewMode::Volumes => "Drive usage".to_string(),
            ViewMode::TopFiles => "Top files chart".to_string(),
            ViewMode::Extensions => "Extensions chart".to_string(),
            _ => {
                if let Some(node) = self.get_selected_node().or(self.tree_state.root.as_ref()) {
                    format!("Bar chart — {}", node.name)
                } else {
                    "Folder breakdown".to_string()
                }
            }
        };

        match self.view_mode {
            ViewMode::Volumes => {
                lines.push(Line::from("Mounted volumes"));
                let sel = self.get_selected_volume().map(|v| v.name.clone());
                let items: Vec<(String, u64)> = self
                    .volumes
                    .iter()
                    .map(|v| {
                        let label = if Some(&v.name) == sel.as_ref() {
                            format!("> {}", v.name)
                        } else {
                            v.name.clone()
                        };
                        (label, v.used_bytes)
                    })
                    .collect();
                for l in ascii_bar_chart(&items, bar_width, None) {
                    lines.push(Line::from(l));
                }
                lines.push(Line::from(""));
                lines.push(Line::from("Press Enter on a volume to scan it"));
            }
            ViewMode::TopFiles => {
                if let Some(ref root) = self.tree_state.root {
                    let files = collect_largest_files(root, 20);
                    let items: Vec<(String, u64)> = files
                        .iter()
                        .map(|f| {
                            (
                                f.path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default(),
                                f.size,
                            )
                        })
                        .collect();
                    for l in ascii_bar_chart(&items, bar_width, None) {
                        lines.push(Line::from(l));
                    }
                } else {
                    lines.push(Line::from("No scan data"));
                }
            }
            ViewMode::Extensions => {
                if let Some(ref root) = self.tree_state.root {
                    let stats = collect_extension_stats(root);
                    let items: Vec<(String, u64)> = stats
                        .iter()
                        .take(12)
                        .map(|s| (s.display_name(), s.total_size))
                        .collect();
                    for l in labeled_pie_legend(&items, bar_width, 12) {
                        lines.push(Line::from(l));
                    }
                } else {
                    lines.push(Line::from("No scan data"));
                }
            }
            _ => {
                if let Some(node) = self.get_selected_node().or(self.tree_state.root.as_ref()) {
                    for l in labeled_children_chart(node, bar_width, 16) {
                        lines.push(Line::from(l));
                    }
                } else {
                    lines.push(Line::from("No chart for this view"));
                }
            }
        }

        let widget = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(self.theme.border_style()),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(widget, area);
    }

    fn render_alt_view(&mut self, f: &mut ratatui::Frame, area: Rect) {
        let title = match self.view_mode {
            ViewMode::TopFiles => "Top-N files",
            ViewMode::Extensions => "Extensions",
            _ => "View",
        };
        self.alt_viewport_rows = area.height.saturating_sub(2);
        let visible = self.alt_viewport_rows.max(1) as usize;
        let max_scroll = self.alt_lines.len().saturating_sub(visible);
        if self.alt_scroll as usize > max_scroll {
            self.alt_scroll = max_scroll as u16;
        }
        let scroll_hint = if max_scroll > 0 {
            format!(" (↑/↓ scroll {}/{})", self.alt_scroll, max_scroll)
        } else {
            String::new()
        };
        let max_chars = area.width.saturating_sub(4) as usize;
        let lines: Vec<Line> = self
            .alt_lines
            .iter()
            .skip(self.alt_scroll as usize)
            .take(visible)
            .map(|l| Line::from(truncate_chars(l, max_chars)))
            .collect();
        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("{title}{scroll_hint}"))
                .border_style(self.theme.border_style()),
        );
        f.render_widget(widget, area);
    }

    fn render_modal(&self, f: &mut ratatui::Frame, area: Rect) {
        f.render_widget(Clear, area);
        let popup_area = centered_rect(70, 70, area);

        match &self.modal {
            Modal::Help => {
                let p = Paragraph::new(HELP_TEXT)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Help (Esc to close)")
                            .border_style(self.theme.accent_style()),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(p, popup_area);
            }
            Modal::Confirm {
                title,
                message,
                selected,
            } => {
                let buttons = if *selected == 0 {
                    "[Yes]  No"
                } else {
                    " Yes  [No]"
                };
                let text = format!("{message}\n\n{buttons}");
                let p = Paragraph::new(text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(title.as_str())
                            .border_style(self.theme.danger_style()),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(p, popup_area);
            }
            Modal::TypedConfirm {
                message,
                confirm_text,
                input,
                selected,
            } => {
                let buttons = if *selected == 0 {
                    "[Confirm]  Cancel"
                } else {
                    " Confirm  [Cancel]"
                };
                let text = format!(
                    "{message}\n\nType [{confirm_text}] to confirm:\n>{input}\n\n{buttons}"
                );
                let p = Paragraph::new(text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Confirm delete")
                            .border_style(self.theme.danger_style()),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(p, popup_area);
            }
            Modal::PathInput {
                prompt,
                input,
                selected: _,
                ..
            } => {
                let text = format!("{prompt}\n\n>{input}\n\nEnter OK  Esc Cancel");
                let p = Paragraph::new(text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Input")
                            .border_style(self.theme.accent_style()),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(p, popup_area);
            }
            Modal::Export {
                path_input,
                selected,
            } => {
                let opts = ["Text (.txt)", "CSV (.csv)", "Redacted CSV", "Cancel"];
                let opt_line: String = opts
                    .iter()
                    .enumerate()
                    .map(|(i, o)| {
                        if i == *selected {
                            format!("[{o}]")
                        } else {
                            format!(" {o} ")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
                let text = format!("Export report to file:\n\n>{path_input}\n\n{opt_line}");
                let p = Paragraph::new(text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Export")
                            .border_style(self.theme.accent_style()),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(p, popup_area);
            }
            Modal::ScanErrors { errors, scroll } => {
                let shown: Vec<&str> = errors
                    .iter()
                    .skip(*scroll as usize)
                    .take(20)
                    .map(|s| s.as_str())
                    .collect();
                let text = format!(
                    "Scan completed with {} error(s)\n\n{}",
                    errors.len(),
                    shown.join("\n")
                );
                let p = Paragraph::new(text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Scan errors")
                            .border_style(self.theme.warning_style()),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(p, popup_area);
            }
            Modal::Message { title, message } => {
                let p = Paragraph::new(message.as_str())
                    .block(Block::default().borders(Borders::ALL).title(title.as_str()))
                    .wrap(Wrap { trim: false });
                f.render_widget(p, popup_area);
            }
            Modal::ThemePicker { selected, .. } => {
                let lines: Vec<String> = crate::theme::THEMES
                    .iter()
                    .enumerate()
                    .map(|(i, t)| {
                        if i == *selected {
                            format!("> {}  (preview)", t.name)
                        } else {
                            format!("  {}", t.name)
                        }
                    })
                    .collect();
                let text = format!(
                    "{}\n\n↑/↓ preview  Enter apply  Esc cancel",
                    lines.join("\n")
                );
                let p = Paragraph::new(text)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Color theme")
                            .border_style(self.theme.accent_style()),
                    )
                    .wrap(Wrap { trim: false });
                f.render_widget(p, popup_area);
            }
            Modal::None => {}
        }
    }
}

fn rect_contains(r: Rect, (x, y): (u16, u16)) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}

fn hit_index(boxes: &[(Rect, usize)], point: (u16, u16)) -> Option<usize> {
    boxes
        .iter()
        .find(|(r, _)| rect_contains(*r, point))
        .map(|(_, i)| *i)
}

fn hit_action(boxes: &[(Rect, Action)], point: (u16, u16)) -> Option<Action> {
    boxes
        .iter()
        .find(|(r, _)| rect_contains(*r, point))
        .map(|(_, a)| *a)
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let vert_pad = (100 - percent_y) / 2;
    let vert_rem = (100 - percent_y) % 2;
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(vert_pad),
            Constraint::Percentage(percent_y),
            Constraint::Percentage(vert_pad + vert_rem),
        ])
        .split(r);

    let horiz_pad = (100 - percent_x) / 2;
    let horiz_rem = (100 - percent_x) % 2;
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(horiz_pad),
            Constraint::Percentage(percent_x),
            Constraint::Percentage(horiz_pad + horiz_rem),
        ])
        .split(popup_layout[1])[1]
}

fn expand_abs(path: &Path) -> PathBuf {
    expand_user_path(path)
}

fn expand_user_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    let expanded = if s == "~" {
        dirs_home()
    } else if let Some(rest) = s.strip_prefix("~/") {
        dirs_home().join(rest)
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

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

pub fn run_app(start_path: Option<PathBuf>, theme: Theme) -> io::Result<()> {
    let mut app = App::new(start_path, theme);
    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    fn new_app() -> App {
        App::new(Some(PathBuf::from("/")), Theme::default())
    }

    fn render_once(app: &mut App, w: u16, h: u16) {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal.draw(|f| app.render(f)).expect("render");
    }

    #[test]
    fn render_populates_menu_and_toolbar_hitboxes() {
        let mut app = new_app();
        render_once(&mut app, 120, 40);
        assert_eq!(app.menu_hitboxes.len(), menu::MENUS.len());
        assert_eq!(app.toolbar_hitboxes.len(), menu::TOOLBAR.len());
    }

    #[test]
    fn clicking_menu_title_opens_then_closes_dropdown() {
        let mut app = new_app();
        render_once(&mut app, 120, 40);
        let (rect, idx) = app.menu_hitboxes[0];
        app.handle_left_click(rect.x, rect.y);
        assert_eq!(app.open_menu, Some(idx));

        render_once(&mut app, 120, 40);
        assert!(!app.dropdown_hitboxes.is_empty());

        // Clicking the same title again toggles the dropdown closed.
        app.handle_left_click(rect.x, rect.y);
        assert_eq!(app.open_menu, None);
    }

    #[test]
    fn clicking_dropdown_item_dispatches_its_action() {
        let mut app = new_app();
        render_once(&mut app, 120, 40);
        // Open the View menu and click its "Volumes" item.
        let view_idx = menu::MENUS.iter().position(|m| m.title == "View").unwrap();
        let (rect, _) = app.menu_hitboxes[view_idx];
        app.handle_left_click(rect.x, rect.y);
        render_once(&mut app, 120, 40);

        let (item_rect, _) = app
            .dropdown_hitboxes
            .iter()
            .find(|(_, a)| *a == Action::SetView(ViewMode::Volumes))
            .copied()
            .expect("volumes item present");
        app.handle_left_click(item_rect.x, item_rect.y);
        assert_eq!(app.view_mode, ViewMode::Volumes);
        assert_eq!(app.open_menu, None);
    }

    #[test]
    fn clicking_toolbar_view_button_switches_view() {
        let mut app = new_app();
        render_once(&mut app, 120, 40);
        let (rect, _) = app
            .toolbar_hitboxes
            .iter()
            .find(|(_, a)| *a == Action::SetView(ViewMode::Extensions))
            .copied()
            .expect("extensions toolbar button");
        app.handle_left_click(rect.x, rect.y);
        assert_eq!(app.view_mode, ViewMode::Extensions);
    }

    #[test]
    fn dispatch_set_sort_changes_sort_key() {
        let mut app = new_app();
        app.dispatch_action(Action::SetSort(SortKey::Name));
        assert_eq!(app.sort_key, SortKey::Name);
    }

    #[test]
    fn page_step_tracks_table_viewport() {
        let mut app = new_app();
        render_once(&mut app, 120, 40);
        // The table viewport must be a positive number of rows after rendering.
        assert!(app.page_step() >= 1);
    }

    /// Flatten a rendered frame into a single string for content assertions.
    fn rendered_text(app: &mut App, w: u16, h: u16) -> String {
        let backend = TestBackend::new(w, h);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| app.render(f)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn menu_bar_and_toolbar_text_is_visible() {
        let mut app = new_app();
        let text = rendered_text(&mut app, 120, 24);
        for menu in menu::MENUS {
            assert!(text.contains(menu.title), "menu '{}' missing", menu.title);
        }
        assert!(text.contains("[Tree]"));
        assert!(text.contains("[Quit]"));
    }

    #[test]
    fn open_dropdown_shows_item_key_hints() {
        let mut app = new_app();
        app.open_menu = Some(1); // View
        let text = rendered_text(&mut app, 120, 24);
        assert!(text.contains("Color theme"));
        assert!(text.contains("Next view"));
    }
}
