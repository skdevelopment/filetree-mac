use crate::app::App;
use crate::fda::open_fda_settings;
use crate::menu::{self, Action};
use crate::paths::is_delete_protected;
use crate::scanner::format_bytes;
use crate::app::LARGE_DELETE_BYTES;
use crate::ui::modal::{Modal, PendingAction};
use crate::ui::render::{hit_action, hit_index, rect_contains};
use crossterm::event::{
    DisableMouseCapture, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind,
};
use std::io::stdout;

pub(crate) struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::execute!(stdout(), DisableMouseCapture);
        let _ = crossterm::execute!(stdout(), crossterm::terminal::LeaveAlternateScreen);
        let _ = crossterm::terminal::disable_raw_mode();
    }
}

impl App {
    pub(crate) fn handle_key(&mut self, key: KeyEvent) {
        if self.handle_modal_key(key) {
            return;
        }

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

    pub(crate) fn handle_mouse(&mut self, me: MouseEvent) {
        if self.modal.is_some() {
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

    pub(crate) fn handle_left_click(&mut self, col: u16, row: u16) {
        let point = (col, row);

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
                if already_selected {
                    self.toggle_expand_or_scan();
                }
                self.mark_dirty();
            }
        }
    }

    pub(crate) fn toggle_menu(&mut self, idx: usize) {
        self.open_menu = if self.open_menu == Some(idx) {
            None
        } else {
            Some(idx)
        };
        self.mark_dirty();
    }

    pub(crate) fn reveal_finder(&mut self) {
        let path = self
            .get_selected_node()
            .map(|n| n.path.clone())
            .or_else(|| self.tree_state.root.as_ref().map(|r| r.path.clone()));
        let Some(path) = path else {
            return;
        };
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

    pub(crate) fn path_allowed(&self, path: &std::path::Path) -> bool {
        if self.scan_root.as_os_str().is_empty() {
            return true;
        }
        crate::paths::is_under_scan_root(path, &self.scan_root)
    }

    pub(crate) fn delete_selected(&mut self) {
        if self.scan_in_progress() {
            self.notify("Cannot delete while scan is in progress".to_string(), true);
            return;
        }
        if self.delete_in_progress() {
            self.notify("A delete is already in progress".to_string(), true);
            return;
        }
        let Some(node) = self.get_selected_node() else {
            return;
        };
        let (path, is_dir, size) = (node.path.clone(), node.is_dir, node.size);
        let scan_root = if self.scan_root.as_os_str().is_empty() {
            path.clone()
        } else {
            self.scan_root.clone()
        };
        let (protected, reason) = is_delete_protected(&path, &scan_root);
        if protected {
            self.notify(reason, true);
            return;
        }

        let message = format!(
            "Permanently delete?\n\n{}\n\nSize: {}",
            path.display(),
            format_bytes(size as i64)
        );
        self.pending_action = Some(PendingAction::Delete {
            path: path.clone(),
            is_dir,
        });

        if is_dir && size >= LARGE_DELETE_BYTES {
            let confirm_name = path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "delete".to_string());
            self.modal = Some(Modal::TypedConfirm {
                message: format!("{message}\n\nLarge directory — type the folder name to confirm."),
                confirm_text: confirm_name,
                input: String::new(),
                selected: 0,
            });
        } else {
            self.modal = Some(Modal::Confirm {
                title: "Delete Item".to_string(),
                message,
                selected: 0,
            });
        }
    }
}