use crate::app_logic::tick_needs_redraw;
use crate::export::export_warning;
use crate::fda::check_full_disk_access;
use crate::menu::{Action, ViewMode};
use crate::models::{ProgressSnapshot, ScanNode, ScanProgress, SortKey};
use crate::paths::expand_user_path;
use crate::platform::default_scan_path;
use crate::progress::RateTracker;
use crate::progress_ui::{snapshot_to_scan_progress, ProgressDisplay};
use crate::scan_bridge::{ScanBridge, ScanMessage, MAX_MESSAGES_PER_POLL};
use crate::scanner::{format_bytes, volume_total_for_full_scan, DirectoryScanner};
use crate::session::ActiveJob;
use crate::theme::Theme;
use crate::tree_state::TreeState;
use crate::ui::input::TerminalGuard;
use crate::ui::modal::{Modal, PathInputPurpose, PendingAction};
use crate::ui::views::{TreeRow, VIEW_CYCLE};
use crossterm::event::{self, EnableMouseCapture, Event};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::widgets::TableState;
use ratatui::Terminal;
use std::collections::HashSet;
use std::io::{self, stdout};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub(crate) const LARGE_DELETE_BYTES: u64 = 1_000_000_000;

/// Upper bound on input events drained in a single event-loop iteration, so a
/// flood of mouse-move events can never starve rendering.
const MAX_EVENTS_PER_TICK: usize = 256;

pub struct App {
    pub(crate) start_path: PathBuf,
    pub(crate) scan_root: PathBuf,
    pub(crate) scanner: DirectoryScanner,
    pub(crate) tree_state: TreeState,
    pub(crate) sort_key: SortKey,
    pub(crate) sort_reverse: bool,
    pub(crate) filter_text: String,
    pub(crate) filter_match_paths: HashSet<PathBuf>,
    pub(crate) view_mode: ViewMode,
    pub(crate) expanded_paths: HashSet<PathBuf>,
    pub(crate) follow_symlinks: bool,
    pub(crate) show_hidden: bool,
    pub(crate) active_job: Option<ActiveJob>,
    pub(crate) scan_rate_tracker: RateTracker,
    pub(crate) progress_display: ProgressDisplay,
    pub(crate) volumes: Vec<crate::models::VolumeInfo>,
    pub(crate) table_state: TableState,
    pub(crate) status_message: String,
    pub(crate) notification: Option<(String, bool)>,
    pub(crate) modal: Option<Modal>,
    pub(crate) pending_action: Option<PendingAction>,
    pub(crate) last_tree_refresh: Instant,
    pub(crate) last_status: Instant,
    pub(crate) last_progress_snapshot: Option<ProgressSnapshot>,
    pub(crate) fda_banner: Option<String>,
    pub(crate) should_quit: bool,
    pub(crate) tree_rows: Vec<TreeRow>,
    pub(crate) alt_lines: Vec<String>,
    pub(crate) show_filter_bar: bool,
    pub(crate) fda_checked: bool,
    pub(crate) dirty: bool,
    pub(crate) notification_until: Option<Instant>,
    pub(crate) saved_cursor_path: Option<PathBuf>,
    pub(crate) last_chart_width: usize,
    pub(crate) alt_scroll: u16,
    pub(crate) alt_viewport_rows: u16,
    pub(crate) theme: Theme,
    pub(crate) open_menu: Option<usize>,
    pub(crate) menu_hitboxes: Vec<(Rect, usize)>,
    pub(crate) toolbar_hitboxes: Vec<(Rect, Action)>,
    pub(crate) dropdown_hitboxes: Vec<(Rect, Action)>,
    pub(crate) table_rows_area: Rect,
    pub(crate) table_viewport_rows: u16,
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
            filter_match_paths: HashSet::new(),
            view_mode: ViewMode::Tree,
            expanded_paths: HashSet::new(),
            follow_symlinks: false,
            show_hidden: false,
            active_job: None,
            scan_rate_tracker: RateTracker::new(),
            progress_display: ProgressDisplay::default(),
            volumes: Vec::new(),
            table_state: TableState::default(),
            status_message: "Ready. Checking Full Disk Access…".to_string(),
            notification: None,
            modal: None,
            pending_action: None,
            last_tree_refresh: Instant::now(),
            last_status: Instant::now(),
            last_progress_snapshot: None,
            fda_banner: None,
            should_quit: false,
            tree_rows: Vec::new(),
            alt_lines: Vec::new(),
            show_filter_bar: false,
            fda_checked: false,
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
            self.poll_active_job();
            self.maybe_refresh_tree();
            self.expire_notification();

            let tick_elapsed = last_tick.elapsed() >= tick_rate;
            if tick_needs_redraw(
                self.dirty,
                tick_elapsed,
                self.scan_in_progress(),
                self.notification_until.is_some(),
            ) {
                terminal.draw(|f| self.render(f))?;
                self.dirty = false;
            }

            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if event::poll(timeout)? {
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

    pub(crate) fn expire_notification(&mut self) {
        if let Some(until) = self.notification_until {
            if Instant::now() >= until {
                self.notification = None;
                self.notification_until = None;
                self.mark_dirty();
            }
        }
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub(crate) fn scan_in_progress(&self) -> bool {
        matches!(self.active_job, Some(ActiveJob::Scan { .. }))
    }

    pub(crate) fn delete_in_progress(&self) -> bool {
        matches!(self.active_job, Some(ActiveJob::Delete { .. }))
    }

    pub(crate) fn job_in_progress(&self) -> bool {
        self.active_job.is_some()
    }

    pub(crate) fn cancel_active_job(&mut self) {
        match &self.active_job {
            Some(ActiveJob::Scan { cancel, .. }) => {
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            Some(ActiveJob::Delete { progress, .. }) => {
                progress.request_cancel();
            }
            None => {}
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

    pub(crate) fn start_scan(&mut self, path: PathBuf) {
        if self.delete_in_progress() {
            self.notify(
                "Cannot scan while a delete is in progress".to_string(),
                true,
            );
            return;
        }
        if let Some(err) = self.validate_scan_path(&path) {
            self.notify(err.clone(), true);
            self.status_message = err;
            return;
        }

        self.cancel_active_job();
        let path = expand_user_path(&path);
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

    pub(crate) fn start_rescan(&mut self, subtree_path: PathBuf) {
        if self.delete_in_progress() {
            self.notify(
                "Cannot scan while a delete is in progress".to_string(),
                true,
            );
            return;
        }
        self.cancel_active_job();
        self.begin_scan_session(format!("Rescanning {}", subtree_path.display()), false);
        self.spawn_scan_worker(false, Some(subtree_path));
        self.mark_dirty();
    }

    fn begin_scan_session(&mut self, status: String, _use_volume_progress: bool) {
        self.scan_rate_tracker.reset();
        self.last_progress_snapshot = None;
        self.progress_display = ProgressDisplay::default();
        self.status_message = format!("{status} (press c to cancel)");
    }

    fn end_scan_session(&mut self) {
        self.scan_rate_tracker.reset();
        self.last_progress_snapshot = None;
        self.progress_display = ProgressDisplay::default();
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
        self.cancel_active_job();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.scanner.cancel = cancel.clone();

        let (tx, rx) = mpsc::channel();
        let volume_total_bytes = volume_total_for_full_scan(&self.scan_root);
        let use_volume = full;
        self.active_job = Some(ActiveJob::Scan {
            bridge: ScanBridge::new(rx),
            cancel: cancel.clone(),
            cancel_requested: false,
            started_at: Instant::now(),
            volume_total_bytes: if use_volume {
                volume_total_bytes
            } else {
                None
            },
        });

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

            let progress_cb = Arc::new({
                let tx = tx.clone();
                move |p| {
                    let _ = tx.send(ScanMessage::Progress(p));
                }
            });

            let patch_cb = Arc::new({
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

    fn poll_active_job(&mut self) {
        if let Some(ActiveJob::Delete { progress, .. }) = &self.active_job {
            let done = progress.is_done();
            self.mark_dirty();
            if done {
                self.finish_delete_job();
            }
            return;
        }

        let batch = match &self.active_job {
            Some(ActiveJob::Scan { bridge, .. }) => bridge.poll(MAX_MESSAGES_PER_POLL),
            Some(ActiveJob::Delete { .. }) | None => return,
        };

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
                    self.active_job = None;
                    self.mark_dirty();
                }
                crate::scan_bridge::TerminalScanEvent::Error(e) => {
                    self.end_scan_session();
                    self.status_message = e.clone();
                    self.notify(e, true);
                    self.active_job = None;
                    self.mark_dirty();
                }
            }
        }
    }

    fn finish_delete_job(&mut self) {
        let Some(ActiveJob::Delete {
            progress,
            label,
            target,
            ..
        }) = self.active_job.take()
        else {
            return;
        };

        let deleted = progress.items_deleted();

        if let Some(err) = progress.take_error() {
            if progress.is_cancelled() {
                self.notify(
                    format!("Delete cancelled — removed {deleted} item(s) before stopping"),
                    true,
                );
            } else {
                self.notify(format!("Delete failed: {err}"), true);
            }
        } else {
            self.notify(format!("Deleted {label} ({deleted} item(s))"), false);
        }

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
        self.mark_dirty();
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
            let cancel_requested = matches!(
                &self.active_job,
                Some(ActiveJob::Scan {
                    cancel_requested: true,
                    ..
                })
            );
            if cancel_requested && !self.progress_display.worker_cancelled {
                return;
            }
            self.status_message = self
                .progress_display
                .status_line(cancel_requested);
        }
    }

    fn maybe_show_scan_errors(&mut self, progress: &ScanProgress) {
        if !progress.errors.is_empty() {
            self.modal = Some(Modal::ScanErrors {
                errors: progress.errors.clone(),
                scroll: 0,
            });
        }
    }

    pub(crate) fn notify(&mut self, msg: String, is_error: bool) {
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

    pub(crate) fn dispatch_action(&mut self, action: Action) {
        match action {
            Action::Quit => self.should_quit = true,
            Action::Help => self.modal = Some(Modal::Help),
            Action::OpenFilter => {
                self.modal = Some(Modal::PathInput {
                    prompt: "Filter by name (substring):".to_string(),
                    input: self.filter_text.clone(),
                    purpose: PathInputPurpose::Filter,
                });
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
                if self.scan_in_progress() {
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
            Action::Cancel => {
                if self.delete_in_progress() {
                    self.cancel_active_job();
                    self.status_message = "Cancelling delete…".to_string();
                    self.mark_dirty();
                } else if self.scan_in_progress() {
                    self.cancel_active_job();
                    if let Some(ActiveJob::Scan {
                        cancel_requested, ..
                    }) = &mut self.active_job
                    {
                        *cancel_requested = true;
                    }
                    self.status_message = "Cancelling scan…".to_string();
                    self.mark_dirty();
                }
            }
            Action::GotoPath => {
                self.modal = Some(Modal::PathInput {
                    prompt: "Scan path:".to_string(),
                    input: self.start_path.to_string_lossy().to_string(),
                    purpose: PathInputPurpose::Goto,
                });
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
            Action::ThemePicker => {
                let selected = crate::theme::THEMES
                    .iter()
                    .position(|t| t.name == self.theme.name)
                    .unwrap_or(0);
                self.modal = Some(Modal::ThemePicker {
                    selected,
                    original: self.theme,
                });
            }
            Action::NextView => self.cycle_view(1),
            Action::PrevView => self.cycle_view(-1),
            Action::SetView(view) => {
                self.view_mode = view;
                match view {
                    ViewMode::Extensions => self.alt_scroll = 0,
                    ViewMode::TopFiles => self.table_state.select(Some(0)),
                    _ => {}
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
                    self.modal = Some(Modal::Export {
                        path_input: format!(
                            "{}/filetree-report.txt",
                            crate::paths::dirs_home().display()
                        ),
                        selected: 0,
                    });
                } else {
                    self.notify("Nothing to export".to_string(), false);
                }
            }
        }
    }

    pub(crate) fn cycle_view(&mut self, delta: i32) {
        let len = VIEW_CYCLE.len() as i32;
        let idx = VIEW_CYCLE
            .iter()
            .position(|&v| v == self.view_mode)
            .unwrap_or(0) as i32;
        let next = ((idx + delta) % len + len) % len;
        self.view_mode = VIEW_CYCLE[next as usize];
        match self.view_mode {
            ViewMode::Extensions => self.alt_scroll = 0,
            ViewMode::TopFiles => self.table_state.select(Some(0)),
            _ => {}
        }
        self.rebuild_view();
    }
}

pub fn run_app(start_path: Option<PathBuf>, theme: Theme) -> io::Result<()> {
    let mut app = App::new(start_path, theme);
    app.run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu;
    use crate::theme::Theme;
    use crate::ui::modal::Modal;
    use crossterm::event::{KeyCode, KeyEvent};
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

        app.handle_left_click(rect.x, rect.y);
        assert_eq!(app.open_menu, None);
    }

    #[test]
    fn clicking_dropdown_item_dispatches_its_action() {
        let mut app = new_app();
        render_once(&mut app, 120, 40);
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
        assert!(app.page_step() >= 1);
    }

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
        app.open_menu = Some(1);
        let text = rendered_text(&mut app, 120, 24);
        assert!(text.contains("Color theme"));
        assert!(text.contains("Next view"));
    }

    #[test]
    fn typed_confirm_accepts_h_and_l_letters() {
        let mut app = new_app();
        app.modal = Some(Modal::TypedConfirm {
            message: "confirm".into(),
            confirm_text: "html-libs".into(),
            input: String::new(),
            selected: 0,
        });
        for c in "html-libs".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
        match &app.modal {
            Some(Modal::TypedConfirm { input, .. }) => assert_eq!(input, "html-libs"),
            _ => panic!("modal should still be TypedConfirm"),
        }
    }

    #[test]
    fn top_files_view_is_selectable_and_resolves_nodes() {
        let mut app = new_app();
        let mut root = ScanNode::new("ftroot", PathBuf::from("/tmp/ftroot"), true);
        let mut big = ScanNode::new("big.bin", PathBuf::from("/tmp/ftroot/big.bin"), false);
        big.size = 1000;
        big.file_count = 1;
        let mut small = ScanNode::new("small.bin", PathBuf::from("/tmp/ftroot/small.bin"), false);
        small.size = 10;
        small.file_count = 1;
        root.size = 1010;
        root.add_child(big);
        root.add_child(small);
        app.tree_state.root = Some(root);
        app.scan_root = PathBuf::from("/tmp/ftroot");

        app.dispatch_action(Action::SetView(ViewMode::TopFiles));
        assert_eq!(app.tree_rows.len(), 2);
        assert_eq!(app.tree_rows[0].display_name, "/tmp/ftroot/big.bin");
        assert_eq!(
            app.get_selected_node().map(|n| n.name.as_str()),
            Some("big.bin")
        );
        app.dispatch_action(Action::MoveDown);
        assert_eq!(
            app.get_selected_node().map(|n| n.name.as_str()),
            Some("small.bin")
        );
    }

    #[test]
    fn export_filename_accepts_h_and_l_letters() {
        let mut app = new_app();
        app.modal = Some(Modal::Export {
            path_input: String::new(),
            selected: 0,
        });
        for c in "hello.html".chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
        match &app.modal {
            Some(Modal::Export { path_input, .. }) => assert_eq!(path_input, "hello.html"),
            _ => panic!("modal should still be Export"),
        }
    }
}