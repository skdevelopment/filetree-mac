use filetree::app_logic::{
    chart_bar_width, clamp_table_selection, confirm_dialog_executes, export_modal_selection,
    progress_bar_width, tick_needs_redraw, typed_confirm_executes,
};

#[test]
fn test_confirm_dialog_executes_yes_is_index_zero() {
    assert!(confirm_dialog_executes(0));
    assert!(!confirm_dialog_executes(1));
}

#[test]
fn test_typed_confirm_executes_confirm_index_zero() {
    assert!(typed_confirm_executes(0, "folder", "folder"));
    assert!(!typed_confirm_executes(1, "folder", "folder"));
    assert!(!typed_confirm_executes(0, "wrong", "folder"));
}

#[test]
fn test_export_modal_selection_cancel_is_none() {
    assert_eq!(export_modal_selection(0), Some(("text", false)));
    assert_eq!(export_modal_selection(1), Some(("csv", false)));
    assert_eq!(export_modal_selection(2), Some(("csv", true)));
    assert_eq!(export_modal_selection(3), None);
}

#[test]
fn test_chart_bar_width_scales_to_pane() {
    assert!(chart_bar_width(80, 18) >= 4);
    assert!(chart_bar_width(40, 18) < chart_bar_width(120, 18));
}

#[test]
fn test_progress_bar_width_scales() {
    assert!(progress_bar_width(30) >= 10);
    assert!(progress_bar_width(200) <= 50);
}

#[test]
fn test_tick_needs_redraw() {
    assert!(tick_needs_redraw(true, false, false, false));
    assert!(tick_needs_redraw(false, true, true, false));
    assert!(!tick_needs_redraw(false, true, false, false));
    assert!(tick_needs_redraw(false, true, false, true));
}

#[test]
fn test_clamp_table_selection() {
    assert_eq!(clamp_table_selection(Some(5), 3), Some(2));
    assert_eq!(clamp_table_selection(None, 0), None);
    assert_eq!(clamp_table_selection(None, 4), Some(0));
    assert_eq!(clamp_table_selection(Some(1), 5), Some(1));
}
