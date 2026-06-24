//! Pure helpers for scan progress display, rates, and ETA.

use crate::scanner::format_bytes;
use crate::util::truncate_chars;
use std::time::{Duration, Instant};

/// Content rows in the scan progress panel (excluding top border).
pub const SCAN_PROGRESS_CONTENT_LINES: u16 = 6;

/// Total layout height: top border row + content rows.
pub const SCAN_PROGRESS_PANEL_LINES: u16 = SCAN_PROGRESS_CONTENT_LINES + 1;

const MIN_RATE_ELAPSED_SECS: f64 = 0.5;
const MIN_ETA_ELAPSED_SECS: f64 = 2.0;
const MIN_ETA_RATE_BYTES: f64 = 4096.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EtaState {
    Unknown,
    Calculating,
    Remaining(f64),
    Done,
}

impl EtaState {
    pub fn label(self) -> &'static str {
        match self {
            EtaState::Unknown => "—",
            EtaState::Calculating => "calculating…",
            EtaState::Done => "00:00:00",
            EtaState::Remaining(_) => "",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RateSnapshot {
    pub bytes_per_sec: Option<f64>,
    pub items_per_sec: Option<f64>,
}

#[derive(Debug, Clone)]
struct RateSample {
    at: Instant,
    bytes: u64,
    items: u64,
}

#[derive(Debug, Clone, Default)]
pub struct RateTracker {
    samples: Vec<RateSample>,
}

impl RateTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.samples.clear();
    }

    pub fn record(&mut self, bytes: u64, items: u64) {
        self.push_sample(Instant::now(), bytes, items);
    }

    fn push_sample(&mut self, at: Instant, bytes: u64, items: u64) {
        if let Some(last) = self.samples.last() {
            if bytes < last.bytes || items < last.items {
                return;
            }
        }
        self.samples.push(RateSample { at, bytes, items });
        let cutoff = at.checked_sub(Duration::from_secs(5)).unwrap_or(at);
        self.samples.retain(|s| s.at >= cutoff);
        if self.samples.len() > 20 {
            let drain = self.samples.len() - 20;
            self.samples.drain(0..drain);
        }
    }

    pub fn snapshot(&self) -> RateSnapshot {
        let (first, last) = match (self.samples.first(), self.samples.last()) {
            (Some(f), Some(l)) if self.samples.len() >= 2 => (f, l),
            _ => return RateSnapshot::default(),
        };
        let elapsed = last.at.duration_since(first.at).as_secs_f64();
        compute_rates(first.bytes, first.items, last.bytes, last.items, elapsed)
    }

    #[cfg(test)]
    pub(crate) fn record_at(&mut self, at: Instant, bytes: u64, items: u64) {
        self.push_sample(at, bytes, items);
    }
}

/// Inner text width for the progress panel (top border only; no side borders).
pub fn scan_progress_inner_width(area_width: u16) -> usize {
    area_width as usize
}

/// Bar fill width accounting for brackets, space, and percentage label.
pub fn scan_progress_bar_width(inner_width: usize, pct_label: &str) -> usize {
    let reserve = 3 + pct_label.chars().count();
    inner_width.saturating_sub(reserve).clamp(4, 50)
}

/// Max path characters for the `Current:` row.
pub fn scan_progress_path_max_chars(inner_width: usize) -> usize {
    inner_width.saturating_sub(9).max(1)
}

/// Compute byte/item rates from deltas over an elapsed interval.
pub fn compute_rates(
    start_bytes: u64,
    start_items: u64,
    end_bytes: u64,
    end_items: u64,
    elapsed_secs: f64,
) -> RateSnapshot {
    if elapsed_secs < MIN_RATE_ELAPSED_SECS {
        return RateSnapshot::default();
    }
    let bytes_delta = end_bytes.saturating_sub(start_bytes) as f64;
    let items_delta = end_items.saturating_sub(start_items) as f64;
    RateSnapshot {
        bytes_per_sec: (bytes_delta > 0.0).then_some(bytes_delta / elapsed_secs),
        items_per_sec: (items_delta > 0.0).then_some(items_delta / elapsed_secs),
    }
}

/// Progress percentage from bytes scanned vs volume total (capped for in-progress scans).
pub fn scan_progress_percent(bytes_scanned: u64, volume_total: Option<u64>, complete: bool) -> u8 {
    if complete {
        return 100;
    }
    match volume_total {
        Some(total) if total > 0 => {
            let pct = (bytes_scanned as f64 / total as f64 * 100.0).clamp(0.0, 99.0);
            pct.round() as u8
        }
        _ => 0,
    }
}

/// Format a duration as `HH:MM:SS`.
pub fn format_duration_hms(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}

pub fn format_eta(state: EtaState) -> String {
    match state {
        EtaState::Remaining(secs) => format_duration_hms(secs),
        other => other.label().to_string(),
    }
}

/// ETA from remaining bytes and measured scan rate.
pub fn compute_eta(
    bytes_scanned: u64,
    volume_total: Option<u64>,
    bytes_per_sec: Option<f64>,
    elapsed_secs: f64,
    complete: bool,
) -> EtaState {
    if complete {
        return EtaState::Done;
    }
    let Some(total) = volume_total else {
        return EtaState::Unknown;
    };
    if elapsed_secs < MIN_ETA_ELAPSED_SECS {
        return EtaState::Calculating;
    }
    let Some(rate) = bytes_per_sec else {
        return EtaState::Calculating;
    };
    if rate < MIN_ETA_RATE_BYTES {
        return EtaState::Calculating;
    }
    let remaining = total.saturating_sub(bytes_scanned);
    if remaining == 0 {
        EtaState::Calculating
    } else {
        EtaState::Remaining(remaining as f64 / rate)
    }
}

pub fn format_item_rate(rate: Option<f64>) -> String {
    match rate {
        Some(r) if r >= 100.0 => format!("{:.0} items/s", r),
        Some(r) if r >= 1.0 => format!("{:.1} items/s", r),
        Some(r) if r > 0.0 => format!("{:.2} items/s", r),
        _ => "— items/s".to_string(),
    }
}

/// UTF-8-safe path truncation with ellipsis when over `max_chars`.
pub fn truncate_progress_path(path: &str, max_chars: usize) -> String {
    if path.chars().count() <= max_chars {
        return path.to_string();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", truncate_chars(path, keep))
}

pub fn format_data_line(bytes_scanned: u64, volume_total: Option<u64>) -> String {
    let scanned = format_bytes(bytes_scanned as i64);
    match volume_total {
        Some(total) if total > 0 => {
            let total_fmt = format_bytes(total as i64);
            format!("Data: {scanned} / {total_fmt}")
        }
        _ => format!("Data: {scanned} scanned"),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanProgressPanel {
    pub title: String,
    pub bar_line: String,
    pub data_line: String,
    pub items_line: String,
    pub time_line: String,
    pub path_line: String,
    pub errors_line: Option<String>,
}

pub struct ScanProgressPanelInput<'a> {
    pub bytes_scanned: u64,
    pub scanned_items: u64,
    pub scanned_dirs: u64,
    pub current_path: &'a str,
    pub error_count: usize,
    pub volume_total: Option<u64>,
    pub elapsed_secs: f64,
    pub rates: RateSnapshot,
    pub bar_width: usize,
    pub path_max_chars: usize,
    pub max_line_width: usize,
    pub complete: bool,
    pub cancelled: bool,
}

fn fit_line(line: String, max_chars: usize) -> String {
    if line.chars().count() <= max_chars {
        line
    } else {
        truncate_progress_path(&line, max_chars)
    }
}

pub fn build_scan_progress_panel(input: ScanProgressPanelInput<'_>) -> ScanProgressPanel {
    let pct = scan_progress_percent(input.bytes_scanned, input.volume_total, input.complete);
    let pct_label = match input.volume_total {
        Some(total) if total > 0 => format!("{pct}%"),
        _ => "scanning…".to_string(),
    };
    let bar_w = input.bar_width.max(4);
    let filled = if input.volume_total.is_some_and(|t| t > 0) {
        ((pct as usize) * bar_w / 100).min(bar_w)
    } else {
        let w =
            ((input.scanned_items as f64 + 1.0).log10() * (bar_w as f64 / 4.0)).round() as usize;
        w.clamp(1, bar_w.saturating_sub(1).max(1))
    };
    let bar_line = format!(
        "[{}{}] {pct_label}",
        "█".repeat(filled),
        "░".repeat(bar_w.saturating_sub(filled))
    );

    let data_line = format_data_line(input.bytes_scanned, input.volume_total);
    let items_line = format!(
        "Items: {} files | {} dirs | rate: {}",
        input.scanned_items,
        input.scanned_dirs,
        format_item_rate(input.rates.items_per_sec)
    );

    let elapsed = format_duration_hms(input.elapsed_secs);
    let eta = compute_eta(
        input.bytes_scanned,
        input.volume_total,
        input.rates.bytes_per_sec,
        input.elapsed_secs,
        input.complete,
    );
    let time_line = format!("Elapsed: {elapsed} | ETA: {}", format_eta(eta));

    let path = truncate_progress_path(input.current_path, input.path_max_chars);
    let path_line = format!("Current: {path}");

    let errors_line = if input.error_count > 0 {
        Some(format!("Errors: {}", input.error_count))
    } else {
        None
    };

    let title = if input.cancelled {
        "Cancelling…".to_string()
    } else {
        "Scanning".to_string()
    };

    let max_w = input.max_line_width.max(1);
    ScanProgressPanel {
        title,
        bar_line: fit_line(bar_line, max_w),
        data_line: fit_line(data_line, max_w),
        items_line: fit_line(items_line, max_w),
        time_line: fit_line(time_line, max_w),
        path_line: fit_line(path_line, max_w),
        errors_line: errors_line.map(|l| fit_line(l, max_w)),
    }
}

/// Whether the progress panel should show the cancelling state.
pub fn scan_panel_cancelled(cancel_requested: bool, progress_cancelled: bool) -> bool {
    cancel_requested || progress_cancelled
}

pub fn progress_bar_fill(pct: u8, bar_width: usize) -> usize {
    let bar_w = bar_width.max(4);
    ((pct as usize) * bar_w / 100).min(bar_w)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_line_fits(line: &str, max_width: usize) {
        assert!(
            line.chars().count() <= max_width,
            "line too wide ({} > {}): {line}",
            line.chars().count(),
            max_width
        );
    }

    fn panel_input<'a>(
        inner_width: usize,
        current_path: &'a str,
        volume_total: Option<u64>,
        elapsed_secs: f64,
        rates: RateSnapshot,
        complete: bool,
        cancelled: bool,
    ) -> ScanProgressPanelInput<'a> {
        let pct_label = match volume_total {
            Some(t) if t > 0 => "50%".to_string(),
            _ => "scanning…".to_string(),
        };
        ScanProgressPanelInput {
            bytes_scanned: 500_000_000,
            scanned_items: 1200,
            scanned_dirs: 40,
            current_path,
            error_count: 0,
            volume_total,
            elapsed_secs,
            rates,
            bar_width: scan_progress_bar_width(inner_width, &pct_label),
            path_max_chars: scan_progress_path_max_chars(inner_width),
            max_line_width: inner_width,
            complete,
            cancelled,
        }
    }

    #[test]
    fn test_scan_progress_percent_from_bytes() {
        assert_eq!(scan_progress_percent(0, Some(1_000_000_000), false), 0);
        assert_eq!(
            scan_progress_percent(500_000_000, Some(1_000_000_000), false),
            50
        );
        assert_eq!(
            scan_progress_percent(2_000_000_000, Some(1_000_000_000), false),
            99
        );
        assert_eq!(
            scan_progress_percent(500_000_000, Some(1_000_000_000), true),
            100
        );
        assert_eq!(scan_progress_percent(100, None, false), 0);
    }

    #[test]
    fn test_format_duration_hms() {
        assert_eq!(format_duration_hms(0.0), "00:00:00");
        assert_eq!(format_duration_hms(65.0), "00:01:05");
        assert_eq!(format_duration_hms(3661.0), "01:01:01");
    }

    #[test]
    fn test_compute_rates() {
        let rates = compute_rates(0, 0, 10_000, 50, 2.0);
        assert_eq!(rates.bytes_per_sec, Some(5000.0));
        assert_eq!(rates.items_per_sec, Some(25.0));

        let slow = compute_rates(0, 0, 100, 1, 0.1);
        assert_eq!(slow.bytes_per_sec, None);

        let zero_delta = compute_rates(100, 10, 100, 10, 2.0);
        assert_eq!(zero_delta.bytes_per_sec, None);
        assert_eq!(zero_delta.items_per_sec, None);
    }

    #[test]
    fn test_compute_eta_states() {
        assert_eq!(
            compute_eta(0, Some(1_000_000), Some(100_000.0), 0.5, false),
            EtaState::Calculating
        );
        assert_eq!(
            compute_eta(0, Some(1_000_000), Some(100_000.0), 3.0, false),
            EtaState::Remaining(10.0)
        );
        assert_eq!(
            compute_eta(0, None, Some(100_000.0), 5.0, false),
            EtaState::Unknown
        );
        assert_eq!(
            compute_eta(1_000_000, Some(1_000_000), Some(100_000.0), 5.0, false),
            EtaState::Calculating
        );
        assert_eq!(
            compute_eta(1_000_000, Some(1_000_000), Some(100_000.0), 5.0, true),
            EtaState::Done
        );
        assert_eq!(
            compute_eta(0, Some(1_000_000), Some(100.0), 5.0, false),
            EtaState::Calculating
        );
    }

    #[test]
    fn test_format_eta() {
        assert_eq!(format_eta(EtaState::Unknown), "—");
        assert_eq!(format_eta(EtaState::Calculating), "calculating…");
        assert_eq!(format_eta(EtaState::Remaining(125.0)), "00:02:05");
        assert_eq!(format_eta(EtaState::Done), "00:00:00");
    }

    #[test]
    fn test_truncate_progress_path_utf8() {
        assert_eq!(truncate_progress_path("short", 10), "short");
        assert_eq!(
            truncate_progress_path("café-path-very-long-name", 8),
            "café-pa…"
        );
    }

    #[test]
    fn test_rate_tracker_monotonic_and_snapshot() {
        let base = Instant::now();
        let mut tracker = RateTracker::new();
        tracker.record_at(base, 0, 0);
        tracker.record_at(base + Duration::from_secs(2), 10_000, 50);
        let snap = tracker.snapshot();
        assert_eq!(snap.bytes_per_sec, Some(5000.0));
        assert_eq!(snap.items_per_sec, Some(25.0));

        tracker.record_at(base + Duration::from_secs(3), 5_000, 60);
        let snap_after_regress = tracker.snapshot();
        assert_eq!(snap_after_regress.bytes_per_sec, Some(5000.0));

        tracker.reset();
        assert_eq!(tracker.snapshot(), RateSnapshot::default());
    }

    #[test]
    fn test_rate_tracker_window_trim() {
        let base = Instant::now();
        let mut tracker = RateTracker::new();
        tracker.record_at(base, 0, 0);
        tracker.record_at(base + Duration::from_secs(6), 60_000, 300);
        let snap = tracker.snapshot();
        assert_eq!(snap.bytes_per_sec, None);
    }

    #[test]
    fn test_build_scan_progress_panel_with_total() {
        let panel = build_scan_progress_panel(ScanProgressPanelInput {
            bytes_scanned: 500_000_000,
            scanned_items: 1200,
            scanned_dirs: 40,
            current_path: "/Users/me/Documents/report.pdf",
            error_count: 2,
            volume_total: Some(1_000_000_000),
            elapsed_secs: 120.0,
            rates: RateSnapshot {
                bytes_per_sec: Some(50_000.0),
                items_per_sec: Some(10.0),
            },
            bar_width: 20,
            path_max_chars: 40,
            max_line_width: 80,
            complete: false,
            cancelled: false,
        });
        assert_eq!(panel.title, "Scanning");
        assert!(panel.bar_line.contains("50%"));
        assert!(panel.data_line.contains(" / "));
        assert!(panel.items_line.contains("10.0 items/s"));
        assert!(panel.time_line.contains("Elapsed: 00:02:00"));
        assert!(panel.path_line.contains("Current:"));
        assert_eq!(panel.errors_line.as_deref(), Some("Errors: 2"));
    }

    #[test]
    fn test_build_scan_progress_panel_without_total() {
        let panel = build_scan_progress_panel(ScanProgressPanelInput {
            bytes_scanned: 1_500_000,
            scanned_items: 10,
            scanned_dirs: 2,
            current_path: "/tmp",
            error_count: 0,
            volume_total: None,
            elapsed_secs: 5.0,
            rates: RateSnapshot::default(),
            bar_width: 16,
            path_max_chars: 30,
            max_line_width: 60,
            complete: false,
            cancelled: false,
        });
        assert!(panel.bar_line.contains("scanning…"));
        assert!(panel.data_line.contains("scanned"));
        assert!(panel.errors_line.is_none());
    }

    #[test]
    fn test_build_scan_progress_panel_cancelled() {
        let panel = build_scan_progress_panel(ScanProgressPanelInput {
            bytes_scanned: 100,
            scanned_items: 5,
            scanned_dirs: 1,
            current_path: "/tmp",
            error_count: 0,
            volume_total: None,
            elapsed_secs: 3.0,
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
    fn test_build_scan_progress_panel_complete() {
        let panel = build_scan_progress_panel(ScanProgressPanelInput {
            bytes_scanned: 1_000_000_000,
            scanned_items: 1000,
            scanned_dirs: 10,
            current_path: "/",
            error_count: 0,
            volume_total: Some(1_000_000_000),
            elapsed_secs: 60.0,
            rates: RateSnapshot {
                bytes_per_sec: Some(1_000_000.0),
                items_per_sec: Some(20.0),
            },
            bar_width: 20,
            path_max_chars: 40,
            max_line_width: 80,
            complete: true,
            cancelled: false,
        });
        assert!(panel.bar_line.contains("100%"));
        assert!(panel.time_line.contains("ETA: 00:00:00"));
    }

    #[test]
    fn test_scan_progress_layout_narrow_widths() {
        let path = "/Users/me/Development/filetree-mac/src/app.rs";
        let rates = RateSnapshot {
            bytes_per_sec: Some(50_000.0),
            items_per_sec: Some(10.0),
        };
        for width in [20u16, 40, 80] {
            let inner = scan_progress_inner_width(width);
            let panel = build_scan_progress_panel(panel_input(
                inner,
                path,
                Some(1_000_000_000),
                120.0,
                rates,
                false,
                false,
            ));
            assert_line_fits(&panel.bar_line, inner);
            assert_line_fits(&panel.data_line, inner);
            assert_line_fits(&panel.items_line, inner);
            assert_line_fits(&panel.time_line, inner);
            assert_line_fits(&panel.path_line, inner);
        }
    }

    #[test]
    fn test_scan_panel_cancelled_immediate() {
        assert!(scan_panel_cancelled(true, false));
        assert!(scan_panel_cancelled(false, true));
        assert!(!scan_panel_cancelled(false, false));

        let panel = build_scan_progress_panel(ScanProgressPanelInput {
            bytes_scanned: 100,
            scanned_items: 5,
            scanned_dirs: 1,
            current_path: "/tmp",
            error_count: 0,
            volume_total: None,
            elapsed_secs: 1.0,
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
    fn test_progress_bar_fill() {
        assert_eq!(progress_bar_fill(50, 20), 10);
        assert_eq!(progress_bar_fill(100, 20), 20);
    }
}
