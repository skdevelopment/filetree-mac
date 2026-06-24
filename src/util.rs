/// Truncate a string to at most `max_chars` Unicode scalar values.
pub fn truncate_chars(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

/// Compute bar fill length for a percentage and total width.
pub fn bar_fill_len(pct: f64, width: usize, min_block_for_nonzero: bool) -> usize {
    if pct <= 0.0 {
        return 0;
    }
    let raw = (pct / 100.0 * width as f64).round() as usize;
    if min_block_for_nonzero {
        raw.max(1).min(width)
    } else if pct < 1.0 {
        0
    } else {
        raw.max(1).min(width)
    }
}
