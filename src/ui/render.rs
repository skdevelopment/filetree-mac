use crate::app::App;
use crate::app_logic::chart_bar_width;
use crate::menu::{self, Action, ViewMode};
use crate::progress::{
    build_scan_progress_panel, scan_panel_cancelled, scan_progress_bar_width,
    scan_progress_inner_width, scan_progress_path_max_chars, ScanProgressPanelInput,
    SCAN_PROGRESS_PANEL_LINES,
};
use crate::scanner::{
    ascii_bar_chart, collect_extension_stats, collect_largest_files, labeled_children_chart,
    labeled_pie_legend,
};
use crate::session::ActiveJob;
use crate::util::truncate_chars;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Row, Wrap};

pub(crate) fn rect_contains(r: Rect, (x, y): (u16, u16)) -> bool {
    x >= r.x && x < r.x.saturating_add(r.width) && y >= r.y && y < r.y.saturating_add(r.height)
}

pub(crate) fn hit_index(boxes: &[(Rect, usize)], point: (u16, u16)) -> Option<usize> {
    boxes
        .iter()
        .find(|(r, _)| rect_contains(*r, point))
        .map(|(_, i)| *i)
}

pub(crate) fn hit_action(boxes: &[(Rect, Action)], point: (u16, u16)) -> Option<Action> {
    boxes
        .iter()
        .find(|(r, _)| rect_contains(*r, point))
        .map(|(_, a)| *a)
}

pub(crate) fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
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

impl App {
    pub(crate) fn render_delete_progress_panel(&self, f: &mut ratatui::Frame, area: Rect) {
        let Some(ActiveJob::Delete {
            progress,
            started_at,
            ..
        }) = &self.active_job
        else {
            return;
        };
        let deleted = progress.items_deleted();
        let total = progress.total_items;
        let frac = (deleted as f64 / total as f64).clamp(0.0, 1.0);
        let pct = (frac * 100.0).round() as u64;
        let cancelling = progress.is_cancelled();
        let title = if cancelling {
            "Cancelling delete…"
        } else {
            "Deleting"
        };

        let inner_width = scan_progress_inner_width(area.width);
        let pct_label = format!("{pct}%");
        let bar_w = scan_progress_bar_width(inner_width, &pct_label);
        let filled = ((frac * bar_w as f64).round() as usize).min(bar_w);
        let bar_line = format!(
            "[{}{}] {pct_label}",
            "█".repeat(filled),
            "░".repeat(bar_w - filled)
        );

        let elapsed = started_at.elapsed().as_secs_f64();
        let items_line = format!(
            "Removed {deleted} / {total} items   Elapsed {}",
            crate::progress::format_duration_hms(elapsed)
        );

        let current = progress.current_path();
        let path_max = scan_progress_path_max_chars(inner_width);
        let path_line = format!(
            "Deleting: {}",
            crate::progress::truncate_progress_path(&current.to_string_lossy(), path_max)
        );

        let lines = vec![
            Line::from(bar_line),
            Line::from(items_line),
            Line::from(path_line),
        ];
        let widget = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::TOP)
                .title(title)
                .border_style(self.theme.danger_style()),
        );
        f.render_widget(widget, area);
    }

    pub(crate) fn render_scan_progress_panel(&self, f: &mut ratatui::Frame, area: Rect) {
        let Some(ActiveJob::Scan {
            started_at,
            volume_total_bytes,
            cancel_requested,
            ..
        }) = &self.active_job
        else {
            return;
        };
        let display = &self.progress_display;
        let elapsed_secs = started_at.elapsed().as_secs_f64();
        let inner_width = scan_progress_inner_width(area.width);
        let pct_label = match volume_total_bytes {
            Some(total) if *total > 0 => {
                let pct = crate::progress::scan_progress_percent(
                    display.bytes_scanned,
                    *volume_total_bytes,
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
            volume_total: *volume_total_bytes,
            elapsed_secs,
            rates: self.scan_rate_tracker.snapshot(),
            bar_width: scan_progress_bar_width(inner_width, &pct_label),
            path_max_chars: scan_progress_path_max_chars(inner_width),
            max_line_width: inner_width,
            complete: false,
            cancelled: scan_panel_cancelled(*cancel_requested, display.worker_cancelled),
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

    pub(crate) fn render(&mut self, f: &mut ratatui::Frame) {
        let size = f.area();
        self.menu_hitboxes.clear();
        self.toolbar_hitboxes.clear();
        self.dropdown_hitboxes.clear();
        self.table_rows_area = Rect::default();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(if self.show_filter_bar { 1 } else { 0 }),
                Constraint::Min(0),
                Constraint::Length(if self.job_in_progress() {
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
            ViewMode::TopFiles => {
                self.render_table(f, chunks[3]);
            }
            ViewMode::Extensions => {
                self.render_alt_view(f, chunks[3]);
            }
        }

        if self.delete_in_progress() {
            self.render_delete_progress_panel(f, chunks[4]);
        } else if self.scan_in_progress() {
            self.render_scan_progress_panel(f, chunks[4]);
        }

        let status = if let Some((ref msg, _)) = self.notification {
            msg.clone()
        } else if self.view_mode == ViewMode::TopFiles
            && !self.scan_in_progress()
            && !self.delete_in_progress()
        {
            self.selected_path()
                .map(|p| format!("Selected: {}", p.display()))
                .unwrap_or_else(|| self.status_message.clone())
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

        if self.open_menu.is_some() {
            self.render_dropdown(f, size);
        }

        if self.modal.is_some() {
            self.render_modal(f, size);
        }
    }

    pub(crate) fn render_menu_bar(&mut self, f: &mut ratatui::Frame, area: Rect) {
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

    pub(crate) fn render_toolbar(&mut self, f: &mut ratatui::Frame, area: Rect) {
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

    pub(crate) fn render_dropdown(&mut self, f: &mut ratatui::Frame, screen: Rect) {
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
        let y = 1u16;
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

    pub(crate) fn render_fda_banner(&self, f: &mut ratatui::Frame, area: Rect, message: &str) {
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

    pub(crate) fn render_table(&mut self, f: &mut ratatui::Frame, area: Rect) {
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
        } else if self.view_mode == ViewMode::TopFiles {
            let header = Row::new(vec![
                "#".to_string(),
                "Size".to_string(),
                "%Disk".to_string(),
                "Path".to_string(),
            ]);
            let rows: Vec<Row> = self
                .tree_rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    Row::new(vec![
                        format!("{}", i + 1),
                        r.size.clone(),
                        r.pct_disk.clone(),
                        r.display_name.clone(),
                    ])
                })
                .collect();
            let constraints = vec![
                Constraint::Length(5),
                Constraint::Length(11),
                Constraint::Length(8),
                Constraint::Fill(1),
            ];
            (header, rows, constraints)
        } else {
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
            ViewMode::TopFiles => "Top 100 largest files  —  d: delete  f: reveal",
            _ => "View",
        };

        let table = ratatui::widgets::Table::new(rows, constraints)
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

    pub(crate) fn render_chart_panel(&mut self, f: &mut ratatui::Frame, area: Rect) {
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

    pub(crate) fn render_alt_view(&mut self, f: &mut ratatui::Frame, area: Rect) {
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
}