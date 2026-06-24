use crate::app::App;
use crate::app_logic::{
    confirm_dialog_executes, export_modal_selection, typed_confirm_executes,
};
use crate::delete::{run_delete, DeleteProgress};
use crate::export::{is_sensitive_export_path, save_report};
use crate::paths::{expand_user_path, safe_delete_target};
use crate::ui::render::centered_rect;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub(crate) const HELP_TEXT: &str = r#"filetree — keyboard shortcuts

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
pub(crate) enum PathInputPurpose {
    Goto,
    Filter,
}

#[derive(Debug, Clone)]
pub(crate) enum PendingAction {
    Delete {
        path: PathBuf,
        is_dir: bool,
    },
    Export {
        fmt: String,
        path: PathBuf,
        redact: bool,
        overwrite: bool,
    },
}

#[derive(Debug, Clone)]
pub(crate) enum Modal {
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
    },
    Export {
        path_input: String,
        selected: usize,
    },
    ScanErrors {
        errors: Vec<String>,
        scroll: u16,
    },
    ThemePicker {
        selected: usize,
        /// Theme active when the picker opened, restored if the user cancels.
        original: crate::theme::Theme,
    },
}

impl App {
    #[allow(clippy::needless_return)]
    pub(crate) fn handle_modal_key(&mut self, key: KeyEvent) -> bool {
        let Some(modal) = self.modal.as_mut() else {
            return false;
        };

        match modal {
            Modal::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?')) {
                    self.modal = None;
                }
            }
            Modal::Confirm { .. } => {
                match key.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        if let Modal::Confirm { selected, .. } = modal {
                            *selected = 0;
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        if let Modal::Confirm { selected, .. } = modal {
                            *selected = 1;
                        }
                    }
                    KeyCode::Enter => {
                        let selected = if let Modal::Confirm { selected, .. } = modal {
                            *selected
                        } else {
                            1
                        };
                        self.modal = None;
                        if confirm_dialog_executes(selected) {
                            self.execute_pending_action();
                        } else {
                            self.pending_action = None;
                        }
                    }
                    KeyCode::Esc => {
                        self.modal = None;
                        self.pending_action = None;
                    }
                    _ => {}
                }
            }
            Modal::TypedConfirm { .. } => {
                match key.code {
                    KeyCode::Left => {
                        if let Modal::TypedConfirm { selected, .. } = modal {
                            *selected = 0;
                        }
                    }
                    KeyCode::Right => {
                        if let Modal::TypedConfirm { selected, .. } = modal {
                            *selected = 1;
                        }
                    }
                    KeyCode::Enter => {
                        let (confirm_text, input_val, selected) =
                            if let Modal::TypedConfirm {
                                confirm_text,
                                input,
                                selected,
                                ..
                            } = modal
                            {
                                (confirm_text.clone(), input.clone(), *selected)
                            } else {
                                return true;
                            };
                        if typed_confirm_executes(selected, &input_val, &confirm_text) {
                            self.modal = None;
                            self.execute_pending_action();
                        } else if selected == 1 {
                            self.modal = None;
                            self.pending_action = None;
                        }
                    }
                    KeyCode::Esc => {
                        self.modal = None;
                        self.pending_action = None;
                    }
                    KeyCode::Backspace => {
                        if let Modal::TypedConfirm { input, .. } = modal {
                            input.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Modal::TypedConfirm { input, .. } = modal {
                            input.push(c);
                        }
                    }
                    _ => {}
                }
            }
            Modal::PathInput { .. } => {
                match key.code {
                    KeyCode::Enter => {
                        let (purpose, val) = if let Modal::PathInput {
                            input,
                            purpose,
                            ..
                        } = modal
                        {
                            (*purpose, input.clone())
                        } else {
                            return true;
                        };
                        self.modal = None;
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
                    KeyCode::Esc => self.modal = None,
                    KeyCode::Backspace => {
                        if let Modal::PathInput { input, .. } = modal {
                            input.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Modal::PathInput { input, .. } = modal {
                            input.push(c);
                        }
                    }
                    _ => {}
                }
            }
            Modal::Export { .. } => {
                match key.code {
                    KeyCode::Left => {
                        if let Modal::Export { selected, .. } = modal {
                            *selected = selected.saturating_sub(1);
                        }
                    }
                    KeyCode::Right => {
                        if let Modal::Export { selected, .. } = modal {
                            *selected = (*selected + 1).min(3);
                        }
                    }
                    KeyCode::Enter => {
                        let (path, sel) = if let Modal::Export {
                            path_input,
                            selected,
                        } = modal
                        {
                            (path_input.clone(), *selected)
                        } else {
                            return true;
                        };
                        let path = expand_user_path(Path::new(&path));
                        self.modal = None;
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
                    KeyCode::Esc => self.modal = None,
                    KeyCode::Backspace => {
                        if let Modal::Export { path_input, .. } = modal {
                            path_input.pop();
                        }
                    }
                    KeyCode::Char(c) => {
                        if let Modal::Export { path_input, .. } = modal {
                            path_input.push(c);
                        }
                    }
                    _ => {}
                }
            }
            Modal::ScanErrors { .. } => {
                match key.code {
                    KeyCode::Enter | KeyCode::Esc => self.modal = None,
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Modal::ScanErrors { scroll, .. } = modal {
                            *scroll += 1;
                        }
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Modal::ScanErrors { scroll, .. } = modal {
                            *scroll = scroll.saturating_sub(1);
                        }
                    }
                    _ => {}
                }
            }
            Modal::ThemePicker { .. } => {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if let Modal::ThemePicker { selected, .. } = modal {
                            *selected = selected.saturating_sub(1);
                            self.theme = crate::theme::THEMES[*selected];
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if let Modal::ThemePicker { selected, .. } = modal {
                            let len = crate::theme::THEMES.len();
                            *selected = (*selected + 1).min(len.saturating_sub(1));
                            self.theme = crate::theme::THEMES[*selected];
                        }
                    }
                    KeyCode::Enter => {
                        self.modal = None;
                    }
                    KeyCode::Esc => {
                        if let Modal::ThemePicker { original, .. } = modal {
                            self.theme = *original;
                        }
                        self.modal = None;
                    }
                    _ => {}
                }
            }
        }
        true
    }

    pub(crate) fn execute_pending_action(&mut self) {
        let action = self.pending_action.take();
        let Some(action) = action else {
            return;
        };

        match action {
            PendingAction::Delete { path, is_dir } => {
                let (target, error) = safe_delete_target(&path, &self.scan_root, is_dir);
                if let Some(err) = error {
                    self.notify(format!("Delete blocked: {err}"), true);
                    return;
                }
                let total_items = self
                    .tree_state
                    .root
                    .as_ref()
                    .and_then(|r| r.find_by_path(&target))
                    .map(|n| n.file_count + n.folder_count + 1)
                    .unwrap_or(1);
                let label = target
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| target.to_string_lossy().to_string());
                let progress = Arc::new(DeleteProgress::new(total_items, target.clone()));
                let started_at = Instant::now();
                self.status_message = format!("Deleting {label} … (press c to cancel)");
                self.active_job = Some(crate::session::ActiveJob::Delete {
                    progress: progress.clone(),
                    started_at,
                    label,
                    target: target.clone(),
                });
                std::thread::spawn(move || run_delete(target, is_dir, progress));
                self.mark_dirty();
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
                    self.modal = Some(Modal::Confirm {
                        title: "Export Warning".to_string(),
                        message: format!("Export to a sensitive location?\n\n{}", path.display()),
                        selected: 0,
                    });
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
                        self.modal = Some(Modal::Confirm {
                            title: "Export Overwrite".to_string(),
                            message: format!("Overwrite existing file?\n\n{display}"),
                            selected: 0,
                        });
                    }
                    Err(e) => self.notify(format!("Export failed: {e}"), true),
                }
            }
        }
    }

    pub(crate) fn render_modal(&self, f: &mut ratatui::Frame, area: Rect) {
        let Some(modal) = &self.modal else {
            return;
        };

        f.render_widget(Clear, area);
        let popup_area = centered_rect(70, 70, area);

        match modal {
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
            Modal::PathInput { prompt, input, .. } => {
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
        }
    }
}