use filetree::export::{
    export_csv, export_text, export_warning, has_sensitive_paths, is_sensitive_export_path,
    save_report,
};
use filetree::models::ScanNode;
use filetree::scanner::DirectoryScanner;
use std::fs;
use std::io;
use std::path::Path;

fn scanned_root(tmp: &Path) -> ScanNode {
    fs::create_dir_all(tmp.join("subdir")).unwrap();
    fs::write(tmp.join("readme.md"), "# readme").unwrap();
    let mut scanner = DirectoryScanner::new();
    scanner.scan(tmp, None, None).0
}

fn parse_csv_row(line: &str) -> Vec<String> {
    line.split(',')
        .map(|s| s.trim_matches('"').to_string())
        .collect()
}

#[test]
fn test_export_text() {
    let tmp = tempfile::tempdir().unwrap();
    let root = scanned_root(tmp.path());
    let text = export_text(&root, true, tmp.path(), false);
    assert!(text.contains("filetree Scan Report"));
    assert!(text.contains("readme.md"));
}

#[test]
fn test_export_text_redacted() {
    let tmp = tempfile::tempdir().unwrap();
    let root = scanned_root(tmp.path());
    let text = export_text(&root, true, tmp.path(), true);
    assert!(text.to_lowercase().contains("redacted"));
}

#[test]
fn test_export_csv() {
    let tmp = tempfile::tempdir().unwrap();
    let root = scanned_root(tmp.path());
    let csv = export_csv(&root, tmp.path(), false);
    assert!(csv.starts_with("path,name"));
    let lines: Vec<_> = csv.lines().collect();
    let header = parse_csv_row(lines[0]);
    let readme_line = lines
        .iter()
        .find(|l| l.contains("readme.md"))
        .expect("readme row");
    let row = parse_csv_row(readme_line);
    let idx = |name: &str| header.iter().position(|h| h == name).unwrap();
    assert_eq!(row[idx("name")], "readme.md");
    assert_eq!(row[idx("type")], "file");
    assert!(row[idx("size")].parse::<u64>().unwrap() > 0);
    assert!(
        row[idx("allocated")].parse::<u64>().unwrap() >= row[idx("size")].parse::<u64>().unwrap()
    );
    assert!(!row[idx("owner")].is_empty());
    assert!(row[idx("percent_of_parent")].parse::<f64>().unwrap() > 0.0);
    assert_eq!(row[idx("is_symlink")], "false");
}

#[test]
fn test_export_csv_filters_out_of_root() {
    let tmp = tempfile::tempdir().unwrap();
    let mut root = scanned_root(tmp.path());
    let outside = ScanNode::new("outside.txt", "/outside/outside.txt".into(), false);
    root.add_child(outside);
    let csv = export_csv(&root, tmp.path(), false);
    assert!(!csv.contains("/outside/outside.txt"));
}

#[test]
fn test_export_csv_redacted_omits_owner() {
    let tmp = tempfile::tempdir().unwrap();
    let root = scanned_root(tmp.path());
    let csv = export_csv(&root, tmp.path(), true);
    for line in csv.lines().skip(1) {
        let row = parse_csv_row(line);
        assert_eq!(row[8], "");
    }
}

#[test]
fn test_save_report_text() {
    let tmp = tempfile::tempdir().unwrap();
    let root = scanned_root(tmp.path());
    let out = tmp.path().join("reports/nested/report.txt");
    let saved = save_report(&root, &out, "text", false, tmp.path(), false).unwrap();
    assert_eq!(saved, out);
    assert!(out.exists());
}

#[test]
fn test_save_report_overwrite_guard() {
    let tmp = tempfile::tempdir().unwrap();
    let root = scanned_root(tmp.path());
    let out = tmp.path().join("report.txt");
    fs::write(&out, "existing").unwrap();
    let err = save_report(&root, &out, "text", false, tmp.path(), false).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn test_export_warning_sensitive() {
    let mut root = ScanNode::new("root", "/Users/me".into(), true);
    let child = ScanNode::new("mbox", "/Users/me/Library/Mail/mbox".into(), false);
    root.add_child(child);
    let warning = export_warning(&root);
    assert!(warning.is_some());
    assert!(warning.unwrap().to_lowercase().contains("sensitive"));
    assert!(has_sensitive_paths(&root));
}

#[test]
fn test_is_sensitive_export_path_ssh() {
    assert!(is_sensitive_export_path(Path::new(
        "~/.ssh/authorized_keys"
    )));
}

#[test]
fn test_is_sensitive_export_path_normal() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("report.txt");
    assert!(!is_sensitive_export_path(&path));
}
