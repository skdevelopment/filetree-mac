use filetree::progress::{
    build_scan_progress_panel, compute_eta, compute_rates, format_duration_hms, format_eta,
    scan_panel_cancelled, scan_progress_bar_width, scan_progress_inner_width,
    scan_progress_path_max_chars, scan_progress_percent, truncate_progress_path, EtaState,
    RateSnapshot, ScanProgressPanelInput,
};

#[test]
fn test_progress_percent_integration() {
    assert_eq!(
        scan_progress_percent(250_000_000, Some(1_000_000_000), false),
        25
    );
    assert_eq!(
        scan_progress_percent(900_000_000, Some(1_000_000_000), true),
        100
    );
}

#[test]
fn test_rate_and_eta_integration() {
    let rates = compute_rates(0, 0, 20_000, 100, 4.0);
    assert_eq!(rates.bytes_per_sec, Some(5000.0));
    assert_eq!(rates.items_per_sec, Some(25.0));

    let eta = compute_eta(100_000, Some(1_000_000), rates.bytes_per_sec, 5.0, false);
    assert_eq!(eta, EtaState::Remaining(180.0));
    assert_eq!(format_eta(eta), "00:03:00");
}

#[test]
fn test_eta_states_integration() {
    assert_eq!(
        compute_eta(0, None, Some(1000.0), 5.0, false),
        EtaState::Unknown
    );
    assert_eq!(
        compute_eta(0, Some(1_000_000), Some(1000.0), 1.0, false),
        EtaState::Calculating
    );
    assert_eq!(
        compute_eta(1_000_000, Some(1_000_000), Some(1000.0), 5.0, true),
        EtaState::Done
    );
}

#[test]
fn test_duration_formatting() {
    assert_eq!(format_duration_hms(3723.0), "01:02:03");
}

#[test]
fn test_path_truncation_integration() {
    let long = "/Users/someone/Development/very/deep/nested/project/filetree-mac";
    let truncated = truncate_progress_path(long, 30);
    assert!(truncated.ends_with('…'));
    assert!(truncated.chars().count() <= 30);
}

#[test]
fn test_panel_lines_include_key_fields() {
    let panel = build_scan_progress_panel(ScanProgressPanelInput {
        bytes_scanned: 2_000_000_000,
        scanned_items: 50_000,
        scanned_dirs: 1_200,
        current_path: "/System/Library",
        error_count: 0,
        volume_total: Some(500_000_000_000),
        elapsed_secs: 600.0,
        rates: RateSnapshot {
            bytes_per_sec: Some(1_000_000.0),
            items_per_sec: Some(80.0),
        },
        bar_width: 24,
        path_max_chars: 50,
        max_line_width: 80,
        complete: false,
        cancelled: false,
    });
    assert!(panel.data_line.starts_with("Data:"));
    assert!(panel.items_line.contains("items/s"));
    assert!(panel.time_line.contains("ETA:"));
    assert!(panel.path_line.contains("/System/Library"));
}

#[test]
fn test_cancel_requested_before_worker_ack() {
    assert!(scan_panel_cancelled(true, false));
    let panel = build_scan_progress_panel(ScanProgressPanelInput {
        bytes_scanned: 500,
        scanned_items: 10,
        scanned_dirs: 2,
        current_path: "/tmp",
        error_count: 0,
        volume_total: None,
        elapsed_secs: 2.0,
        rates: RateSnapshot::default(),
        bar_width: 10,
        path_max_chars: 20,
        max_line_width: 40,
        complete: false,
        cancelled: scan_panel_cancelled(true, false),
    });
    assert_eq!(panel.title, "Cancelling…");
}

#[test]
fn test_panel_cancelled_title() {
    let panel = build_scan_progress_panel(ScanProgressPanelInput {
        bytes_scanned: 100,
        scanned_items: 1,
        scanned_dirs: 0,
        current_path: "/tmp",
        error_count: 0,
        volume_total: None,
        elapsed_secs: 2.0,
        rates: RateSnapshot::default(),
        bar_width: 10,
        path_max_chars: 20,
        max_line_width: 40,
        complete: false,
        cancelled: true,
    });
    assert_eq!(panel.title, "Cancelling…");
}

#[test]
fn test_panel_complete_eta_done() {
    let panel = build_scan_progress_panel(ScanProgressPanelInput {
        bytes_scanned: 1_000,
        scanned_items: 10,
        scanned_dirs: 1,
        current_path: "/",
        error_count: 0,
        volume_total: Some(1_000),
        elapsed_secs: 5.0,
        rates: RateSnapshot {
            bytes_per_sec: Some(100.0),
            items_per_sec: Some(2.0),
        },
        bar_width: 12,
        path_max_chars: 30,
        max_line_width: 60,
        complete: true,
        cancelled: false,
    });
    assert!(panel.bar_line.contains("100%"));
    assert!(panel.time_line.contains("ETA: 00:00:00"));
}

#[test]
fn test_panel_narrow_bar_width() {
    let inner = scan_progress_inner_width(20);
    let bar_w = scan_progress_bar_width(inner, "scanning…");
    assert!(bar_w >= 4);
    assert!(bar_w <= inner);

    let panel = build_scan_progress_panel(ScanProgressPanelInput {
        bytes_scanned: 1000,
        scanned_items: 5,
        scanned_dirs: 1,
        current_path: "/tmp/foo",
        error_count: 0,
        volume_total: None,
        elapsed_secs: 3.0,
        rates: RateSnapshot::default(),
        bar_width: bar_w,
        path_max_chars: scan_progress_path_max_chars(inner),
        max_line_width: inner,
        complete: false,
        cancelled: false,
    });
    assert!(panel.bar_line.chars().count() <= inner);
    assert!(panel.path_line.chars().count() <= inner);
}
