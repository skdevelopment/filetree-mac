use crate::models::ScanNode;
use crate::scanner::format_bytes;
use crate::util::{bar_fill_len, truncate_chars};

pub fn ascii_bar_chart(items: &[(String, u64)], width: usize, total: Option<u64>) -> Vec<String> {
    if items.is_empty() {
        return vec!["(no data)".to_string()];
    }
    let total = total.unwrap_or_else(|| items.iter().map(|(_, s)| s).sum());
    let total = if total == 0 { 1 } else { total };
    let label_width = items
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(8)
        .min(20);

    items
        .iter()
        .map(|(label, size)| {
            let pct = *size as f64 / total as f64 * 100.0;
            let bar_len = bar_fill_len(pct, width, true);
            let bar = format!(
                "{}{}",
                "█".repeat(bar_len),
                "░".repeat(width.saturating_sub(bar_len))
            );
            format!(
                "{:<width$} {bar} {:>10}  {:5.1}%",
                truncate_chars(label, label_width),
                format_bytes(*size as i64),
                pct,
                width = label_width
            )
        })
        .collect()
}

pub fn labeled_children_chart(node: &ScanNode, width: usize, max_items: usize) -> Vec<String> {
    if !node.is_dir {
        return vec![
            format!("{}  (file)", node.name),
            format!("Size: {}", format_bytes(node.size as i64)),
        ];
    }
    if node.children.is_empty() {
        return vec![node.name.clone(), "(empty folder)".to_string()];
    }

    let mut children: Vec<&ScanNode> = node.children.iter().collect();
    children.sort_by_key(|b| std::cmp::Reverse(b.size));
    children.truncate(max_items);

    let total = if node.size > 0 {
        node.size
    } else {
        node.children.iter().map(|c| c.size).sum()
    };
    let total = if total == 0 { 1 } else { total };
    let label_width = children
        .iter()
        .map(|c| c.name.chars().count() + 2)
        .max()
        .unwrap_or(8)
        .min(18);

    let mut lines = vec![
        node.name.clone(),
        format!(
            "Total: {}  |  {} item(s)",
            format_bytes(node.size as i64),
            node.children.len()
        ),
        String::new(),
        format!(
            "{:<width$} Size bar                      Size      %",
            "Name",
            width = label_width
        ),
    ];

    for child in children {
        let pct = child.size as f64 / total as f64 * 100.0;
        let bar_len = bar_fill_len(pct, width, false);
        let bar = format!(
            "{}{}",
            "█".repeat(bar_len),
            "░".repeat(width.saturating_sub(bar_len))
        );
        let icon = if child.is_dir { "[D]" } else { "[F]" };
        let name = truncate_chars(&format!("{icon} {}", child.name), label_width + 2);
        lines.push(format!(
            "{:<lw$} {bar} {:>10}  {:5.1}%",
            name,
            format_bytes(child.size as i64),
            pct,
            lw = label_width + 2
        ));
    }

    if node.children.len() > max_items {
        lines.push(format!("… and {} more", node.children.len() - max_items));
    }

    lines
}

pub fn labeled_pie_legend(items: &[(String, u64)], width: usize, max_items: usize) -> Vec<String> {
    if items.is_empty() {
        return vec!["(no data)".to_string()];
    }
    let shown = &items[..items.len().min(max_items)];
    let total: u64 = items.iter().map(|(_, s)| s).sum();
    let total = if total == 0 { 1 } else { total };
    let label_width = shown
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(8)
        .min(16);

    let mut lines = vec![format!(
        "{:<width$} Share                         Size      %",
        "Type",
        width = label_width
    )];

    for (label, size) in shown {
        let pct = *size as f64 / total as f64 * 100.0;
        let bar_len = bar_fill_len(pct, width, false);
        let bar = format!(
            "{}{}",
            "█".repeat(bar_len),
            "░".repeat(width.saturating_sub(bar_len))
        );
        lines.push(format!(
            "{:<width$} {bar} {:>10}  {:5.1}%",
            truncate_chars(label, label_width),
            format_bytes(*size as i64),
            pct,
            width = label_width
        ));
    }

    if items.len() > max_items {
        let other: u64 = items[max_items..].iter().map(|(_, s)| s).sum();
        let pct = other as f64 / total as f64 * 100.0;
        let bar_len = bar_fill_len(pct, width, false).max(if other > 0 { 1 } else { 0 });
        let bar = format!(
            "{}{}",
            "█".repeat(bar_len),
            "░".repeat(width.saturating_sub(bar_len))
        );
        lines.push(format!(
            "{:<width$} {bar} {:>10}  {:5.1}%",
            "(other)",
            format_bytes(other as i64),
            pct,
            width = label_width
        ));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_bar_chart_zero_size() {
        let items = vec![("empty".to_string(), 0), ("full".to_string(), 100)];
        let lines = ascii_bar_chart(&items, 10, Some(100));
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("empty"));
        assert!(lines[0].contains("0 B"));
        assert!(lines[1].contains('█'));
    }
}
