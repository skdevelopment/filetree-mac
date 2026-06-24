//! Pure helpers extracted from the TUI for unit testing.

/// Confirm dialog: index 0 = Yes, 1 = No.
pub fn confirm_dialog_executes(selected: usize) -> bool {
    selected == 0
}

/// Typed confirm: index 0 = Confirm, 1 = Cancel.
pub fn typed_confirm_executes(selected: usize, input: &str, confirm_text: &str) -> bool {
    selected == 0 && input.trim() == confirm_text
}

/// Export modal selection. Returns None for Cancel (index 3).
pub fn export_modal_selection(sel: usize) -> Option<(&'static str, bool)> {
    match sel {
        0 => Some(("text", false)),
        1 => Some(("csv", false)),
        2 => Some(("csv", true)),
        _ => None,
    }
}

/// Scale chart bar width to fit a pane.
pub fn chart_bar_width(pane_width: u16, label_reserve: usize) -> usize {
    ((pane_width as usize).saturating_sub(label_reserve + 14)).clamp(4, 40)
}

/// Whether the event loop should redraw on this tick.
pub fn tick_needs_redraw(
    dirty: bool,
    tick_elapsed: bool,
    scan_in_progress: bool,
    notification_active: bool,
) -> bool {
    dirty || (tick_elapsed && (scan_in_progress || notification_active))
}

/// Clamp table selection after rebuild.
pub fn clamp_table_selection(selected: Option<usize>, row_count: usize) -> Option<usize> {
    match selected {
        None => {
            if row_count > 0 {
                Some(0)
            } else {
                None
            }
        }
        Some(idx) if idx >= row_count && row_count > 0 => Some(row_count - 1),
        Some(_) if row_count == 0 => None,
        other => other,
    }
}
