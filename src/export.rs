use crate::models::ScanNode;
use crate::paths::{dirs_home, expand_user_path, is_under_scan_root};
use crate::scanner::format_bytes;
use crate::util::truncate_chars;
use chrono::Local;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const SENSITIVE_PATH_MARKERS: &[&str] = &[
    "/Library/Mail",
    "/Library/Safari",
    "/Library/Application Support/com.apple.TCC",
    "/Library/Containers/",
];

pub const SENSITIVE_HOME_SEGMENTS: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".aws",
    ".config/gcloud",
    "authorized_keys",
    ".netrc",
    ".docker",
];

pub fn has_sensitive_paths(root: &ScanNode) -> bool {
    for node in root.iter_descendants() {
        let path = node.path.to_string_lossy();
        if SENSITIVE_PATH_MARKERS.iter().any(|m| path.contains(m)) {
            return true;
        }
        if path.contains("/Library/") && path.contains("/Containers/") {
            return true;
        }
    }
    let root_path = root.path.to_string_lossy();
    SENSITIVE_PATH_MARKERS.iter().any(|m| root_path.contains(m))
}

pub fn is_sensitive_export_path(path: &Path) -> bool {
    let expanded = expand_user_path(path);
    let home = dirs_home();
    if expanded.starts_with(&home) {
        if let Ok(rel) = expanded.strip_prefix(&home) {
            let rel_str = rel.to_string_lossy();
            for segment in SENSITIVE_HOME_SEGMENTS {
                if rel_str == *segment || rel_str.starts_with(&format!("{segment}/")) {
                    return true;
                }
            }
        }
        if let Some(name) = expanded.file_name() {
            if name.to_string_lossy().starts_with('.') {
                return true;
            }
        }
    }
    false
}

pub fn export_warning(root: &ScanNode) -> Option<String> {
    if has_sensitive_paths(root) {
        Some(
            "This export may contain sensitive paths, owners, and timestamps from \
             protected macOS locations. Share exports carefully."
                .to_string(),
        )
    } else {
        None
    }
}

fn iter_export_nodes<'a>(root: &'a ScanNode, scan_root: &Path) -> Vec<&'a ScanNode> {
    let mut nodes = vec![root];
    fn walk<'a>(node: &'a ScanNode, scan_root: &Path, out: &mut Vec<&'a ScanNode>) {
        for child in &node.children {
            if !scan_root.as_os_str().is_empty() && !is_under_scan_root(&child.path, scan_root) {
                continue;
            }
            out.push(child);
            walk(child, scan_root, out);
        }
    }
    walk(root, scan_root, &mut nodes);
    nodes
}

fn depth_of(node: &ScanNode, cache: &mut std::collections::HashMap<PathBuf, usize>) -> usize {
    if let Some(&d) = cache.get(&node.path) {
        return d;
    }
    // Walk up via path parent since we lack parent refs
    let mut depth = 0;
    let mut current = node.path.clone();
    while let Some(parent) = current.parent() {
        if parent == current {
            break;
        }
        depth += 1;
        current = parent.to_path_buf();
    }
    cache.insert(node.path.clone(), depth);
    depth
}

pub fn export_text(
    root: &ScanNode,
    include_header: bool,
    scan_root: &Path,
    redact: bool,
) -> String {
    let scan_root = if scan_root.as_os_str().is_empty() {
        &root.path
    } else {
        scan_root
    };
    let mut lines = Vec::new();

    if include_header {
        lines.push("filetree Scan Report".to_string());
        lines.push(format!("Path: {}", root.path.display()));
        lines.push(format!(
            "Date: {}",
            Local::now().format("%Y-%m-%dT%H:%M:%S")
        ));
        lines.push(format!("Total size: {}", format_bytes(root.size as i64)));
        lines.push(format!(
            "Files: {:>8}  Folders: {:>8}",
            root.file_count, root.folder_count
        ));
        if redact {
            lines.push("Mode: redacted (relative paths, no owners)".to_string());
        }
        if let Some(w) = export_warning(root) {
            lines.push(format!("Warning: {w}"));
        }
        lines.push(String::new());
        lines.push(format!(
            "{:<40} {:>12} {:>12} {:>7} {:>8} {:>8}",
            "Name", "Size", "Allocated", "%", "Files", "Folders"
        ));
        lines.push("-".repeat(95));
    }

    let nodes = iter_export_nodes(root, scan_root);
    let mut cache = std::collections::HashMap::new();
    let mut sorted: Vec<&ScanNode> = nodes.to_vec();
    sorted.sort_by(|a, b| {
        depth_of(a, &mut cache)
            .cmp(&depth_of(b, &mut cache))
            .then_with(|| a.path.cmp(&b.path))
    });

    for node in sorted {
        let indent = "  ".repeat(depth_of(node, &mut cache));
        let name = truncate_chars(&node.name, 38);
        let pct = if node.path == root.path {
            "100.0".to_string()
        } else {
            // approximate parent percent via path depth walk — use 0 for simplicity in export
            format!("{:.1}", node.percent_of_parent(None))
        };
        lines.push(format!(
            "{indent}{name:<40} {:>12} {:>12} {:>6}% {:>8} {:>8}",
            format_bytes(node.size as i64),
            format_bytes(node.allocated as i64),
            pct,
            node.file_count,
            node.folder_count,
        ));
    }

    lines.join("\n")
}

pub fn export_csv(root: &ScanNode, scan_root: &Path, redact: bool) -> String {
    let scan_root = if scan_root.as_os_str().is_empty() {
        &root.path
    } else {
        scan_root
    };
    let mut output = String::new();
    output.push_str(
        "path,name,type,size,allocated,percent_of_parent,file_count,folder_count,owner,extension,mtime,is_symlink\n",
    );

    for node in iter_export_nodes(root, scan_root) {
        let path_value = if redact {
            node.path
                .strip_prefix(scan_root)
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| node.path.to_string_lossy().to_string())
        } else {
            node.path.to_string_lossy().to_string()
        };
        let owner_value = if redact {
            String::new()
        } else if node.owner.is_empty() {
            crate::scanner::get_owner(&node.path)
        } else {
            node.owner.clone()
        };
        let extension_value = if node.extension.is_empty() && !node.is_dir {
            crate::scanner::get_file_extension(&node.name)
        } else {
            node.extension.clone()
        };
        let mtime = if node.mtime > 0.0 {
            chrono::DateTime::from_timestamp(node.mtime as i64, 0)
                .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S").to_string())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let node_type = if node.is_dir { "directory" } else { "file" };
        output.push_str(&format!(
            "{},{},{},{},{},{:.2},{},{},{},{},{},{}\n",
            csv_escape(&path_value),
            csv_escape(&node.name),
            node_type,
            node.size,
            node.allocated,
            node.percent_of_parent(None),
            node.file_count,
            node.folder_count,
            csv_escape(&owner_value),
            csv_escape(&extension_value),
            mtime,
            node.is_symlink
        ));
    }

    output
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn save_report(
    root: &ScanNode,
    path: &Path,
    fmt: &str,
    overwrite: bool,
    scan_root: &Path,
    redact: bool,
) -> io::Result<PathBuf> {
    let fmt = if fmt == "auto" {
        if path.extension().and_then(|e| e.to_str()) == Some("csv") {
            "csv"
        } else {
            "text"
        }
    } else {
        fmt
    };

    let expanded = expand_user_path(path);
    if let Some(parent) = expanded.parent() {
        fs::create_dir_all(parent)?;
    }

    if expanded.exists() && !overwrite {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("File already exists: {}", expanded.display()),
        ));
    }

    let content = if fmt == "csv" {
        export_csv(root, scan_root, redact)
    } else {
        export_text(root, true, scan_root, redact)
    };

    fs::write(&expanded, content)?;
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csv_escape_quotes() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("say \"hi\""), "\"say \"\"hi\"\"\"");
    }

    #[test]
    fn test_is_sensitive_export_path_ssh() {
        assert!(is_sensitive_export_path(Path::new(
            "~/.ssh/authorized_keys"
        )));
    }
}
