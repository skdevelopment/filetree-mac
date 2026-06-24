use filetree::models::{PatchKind, ProgressSnapshot, ScanNode, SortKey, TreePatch};
use filetree::scanner::{
    ascii_bar_chart, collect_extension_stats, collect_largest_files, format_bytes,
    get_allocated_size, get_file_extension, get_owner, labeled_children_chart, labeled_pie_legend,
    list_volumes, parse_df_line, parse_volumes_from_df, volume_bytes_for_path,
    volume_total_for_full_scan, DirectoryScanner,
};
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

fn sample_tree(tmp: &Path) -> PathBuf {
    fs::create_dir_all(tmp.join("subdir")).unwrap();
    fs::write(tmp.join("subdir/nested.txt"), "hello world").unwrap();
    fs::write(tmp.join("readme.md"), "# readme").unwrap();
    let mut png = vec![0x89, b'P', b'N', b'G'];
    png.extend(std::iter::repeat_n(0u8, 100));
    fs::write(tmp.join("image.png"), png).unwrap();
    fs::write(tmp.join(".hidden"), "secret").unwrap();
    tmp.to_path_buf()
}

fn scan_node(scanner: &mut DirectoryScanner, path: &Path) -> ScanNode {
    scanner.scan(path, None, None).0
}

fn scan_with_progress(
    scanner: &mut DirectoryScanner,
    path: &Path,
    progress_log: Arc<Mutex<Vec<ProgressSnapshot>>>,
) -> ScanNode {
    let log = progress_log.clone();
    let progress_cb = Arc::new(move |p: ProgressSnapshot| {
        log.lock().unwrap().push(p);
    });
    scanner.scan(path, Some(progress_cb), None).0
}

#[test]
fn test_format_bytes() {
    assert_eq!(format_bytes(0), "0 B");
    assert_eq!(format_bytes(-100), "0 B");
    assert_eq!(format_bytes(512), "512 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(2 * 1024 * 1024), "2.0 MB");
}

#[test]
fn test_get_file_extension_edge_cases() {
    let cases = [
        ("file.txt", "txt"),
        ("archive.tar.gz", "gz"),
        (".gitignore", "gitignore"),
        ("noext", ""),
        ("", ""),
        ("..", ""),
        ("file.", ""),
    ];
    for (name, expected) in cases {
        assert_eq!(get_file_extension(name), expected, "input={name}");
    }
}

#[test]
fn test_scan_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let root = scan_node(&mut scanner, &tree);
    assert!(root.is_dir);
    assert!(root.scan_complete);
    assert_eq!(root.file_count, 3);
    assert_eq!(root.folder_count, 1);
    assert!(root.size > 0);
    let names: std::collections::HashSet<_> =
        root.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["subdir", "readme.md", "image.png"].into_iter().collect()
    );
}

#[test]
fn test_scan_show_hidden() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    scanner.show_hidden = true;
    let root = scan_node(&mut scanner, &tree);
    let names: std::collections::HashSet<_> =
        root.children.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(".hidden"));
}

#[test]
fn test_scan_single_file() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let root = scan_node(&mut scanner, &tree.join("readme.md"));
    assert!(!root.is_dir);
    assert!(root.size > 0);
    assert_eq!(root.file_count, 1);
}

#[test]
fn test_sorted_children() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let root = scan_node(&mut scanner, &tree);
    let by_size: Vec<_> = root
        .sorted_children(SortKey::Size, true)
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(by_size[0], "image.png");
    assert_eq!(by_size.last().unwrap(), &"readme.md");

    let by_name: Vec<_> = root
        .sorted_children(SortKey::Name, false)
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(by_name, vec!["image.png", "readme.md", "subdir"]);
}

#[test]
fn test_collect_largest_files() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let root = scan_node(&mut scanner, &tree);
    let largest = collect_largest_files(&root, 2);
    assert_eq!(largest.len(), 2);
    assert_eq!(largest[0].name, "image.png");
    assert!(largest[0].size >= largest[1].size);
}

#[test]
fn test_collect_extension_stats() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let root = scan_node(&mut scanner, &tree);
    let stats: std::collections::HashMap<_, _> = collect_extension_stats(&root)
        .into_iter()
        .map(|s| (s.extension.clone(), s))
        .collect();
    assert_eq!(stats["png"].file_count, 1);
    assert!(stats["png"].total_size > 100);
    assert_eq!(stats["md"].file_count, 1);
    assert_eq!(stats["txt"].file_count, 1);
}

#[test]
fn test_ascii_bar_chart() {
    assert_eq!(
        ascii_bar_chart(&[], 10, None),
        vec!["(no data)".to_string()]
    );
    let items = vec![("txt".to_string(), 100), ("png".to_string(), 50)];
    let lines = ascii_bar_chart(&items, 10, None);
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("txt"));
    assert!(lines[0].contains('█'));
}

#[test]
fn test_ascii_bar_chart_zero_size() {
    let items = vec![("empty".to_string(), 0), ("full".to_string(), 100)];
    let lines = ascii_bar_chart(&items, 10, Some(100));
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("empty"));
    assert!(lines[0].contains("0 B"));
    assert!(lines[1].contains('█'));
}

#[test]
fn test_labeled_children_chart() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let root = scan_node(&mut scanner, &tree);
    let lines = labeled_children_chart(&root, 28, 16);
    assert!(lines.iter().any(|l| l.contains("Total:")));
    assert!(lines.iter().any(|l| l.contains('█')));
}

#[test]
fn test_labeled_pie_legend() {
    let items = vec![("txt".to_string(), 100), ("png".to_string(), 50)];
    let lines = labeled_pie_legend(&items, 10, 12);
    assert!(lines.len() >= 2);
    assert!(lines[1].contains("txt"));
    assert!(lines[1].contains('%'));
}

#[test]
fn test_parse_df_line_spaced_mount() {
    let line = "/dev/disk1 1000000 500000 500000 50% /Volumes/My Drive";
    let parsed = parse_df_line(line).unwrap();
    assert_eq!(parsed.4, "/Volumes/My Drive");
    assert_eq!(parsed.1, 1000000);
}

#[test]
fn test_parse_volumes_from_df_mocked() {
    let stdout = "Filesystem 1024-blocks Used Available Capacity Mounted on\n\
                  /dev/disk3s1 1000000 400000 600000 40% /System/Volumes/Data\n\
                  map auto_home 0 0 0 100% /System/Volumes/Data/home\n";
    let volumes = parse_volumes_from_df(stdout);
    assert_eq!(volumes.len(), 1);
    assert_eq!(volumes[0].total_bytes, 1000000 * 1024);
    assert_eq!(volumes[0].used_bytes, 400000 * 1024);
}

#[test]
fn test_list_volumes() {
    let volumes = list_volumes();
    assert!(!volumes.is_empty());
    assert!(volumes.iter().all(|v| v.mount_point.is_absolute()));
}

#[test]
fn test_cancel_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut deep = tree.clone();
    for i in 0..30 {
        deep = deep.join(format!("level{i}"));
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("file.txt"), "x".repeat(100)).unwrap();
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let progress_log = Arc::new(Mutex::new(Vec::new()));
    let log = progress_log.clone();

    let handle = thread::spawn(move || {
        let mut scanner = DirectoryScanner {
            cancel: cancel_flag.clone(),
            max_workers: 4,
            ..DirectoryScanner::new()
        };
        let progress_cb = Arc::new(move |p: ProgressSnapshot| {
            let dirs = p.scanned_dirs;
            log.lock().unwrap().push(p);
            if dirs > 2 {
                cancel_flag.store(true, Ordering::SeqCst);
            }
        });
        scanner.scan(&tree, Some(progress_cb), None)
    });

    let (root, progress) = handle.join().unwrap();
    assert!(!root.children.is_empty());

    let events = progress_log.lock().unwrap();
    assert!(!events.is_empty());
    let last = events.last().unwrap();
    assert!(last.cancelled);
    assert!(last.is_complete);
    assert!(progress.cancelled);
}

#[test]
fn test_symlink_dir_cycle() {
    let tmp = tempfile::tempdir().unwrap();
    let sub = tmp.path().join("sub");
    fs::create_dir(&sub).unwrap();
    symlink(&sub, sub.join("loop")).unwrap();

    let mut scanner = DirectoryScanner::new();
    scanner.follow_symlinks = true;
    let progress_log = Arc::new(Mutex::new(Vec::new()));
    let root = scan_with_progress(&mut scanner, tmp.path(), progress_log.clone());
    assert!(root.scan_complete);
    let events = progress_log.lock().unwrap();
    assert!(events
        .iter()
        .filter_map(|p| p.first_error.as_ref())
        .any(|e| e.to_lowercase().contains("cycle")));
}

#[test]
fn test_dir_reached_twice_counted_once() {
    // A real directory reachable via two paths (here: two symlinks to the same
    // target) must be traversed once, keyed by (dev, ino). This is the same
    // mechanism that stops macOS firmlink/mount loops (e.g. /System/Volumes/Data)
    // from recursing forever during a `/` scan.
    let tmp = tempfile::tempdir().unwrap();
    let real = tmp.path().join("real");
    fs::create_dir(&real).unwrap();
    fs::write(real.join("a.txt"), "hello").unwrap();
    symlink(&real, tmp.path().join("link1")).unwrap();
    symlink(&real, tmp.path().join("link2")).unwrap();

    let mut scanner = DirectoryScanner::new();
    scanner.follow_symlinks = true;
    let root = scanner.scan(tmp.path(), None, None).0;
    assert!(root.scan_complete);
    assert_eq!(
        root.file_count, 1,
        "the same directory reached via multiple paths must be counted once"
    );
}

#[test]
fn test_permission_denied_subdirectory() {
    let tmp = tempfile::tempdir().unwrap();
    let denied = tmp.path().join("denied");
    fs::create_dir(&denied).unwrap();
    fs::write(tmp.path().join("visible.txt"), "ok").unwrap();

    let mut perms = fs::metadata(&denied).unwrap().permissions();
    perms.set_mode(0o000);
    fs::set_permissions(&denied, perms).unwrap();

    let mut scanner = DirectoryScanner::new();
    let progress_log = Arc::new(Mutex::new(Vec::new()));
    let root = scan_with_progress(&mut scanner, tmp.path(), progress_log.clone());
    let denied_node = root.children.iter().find(|c| c.name == "denied").unwrap();
    assert!(denied_node.is_dir);
    assert!(denied_node.children.is_empty());
    assert_eq!(denied_node.size, 0);

    let events = progress_log.lock().unwrap();
    assert!(events
        .iter()
        .filter_map(|p| p.first_error.as_ref())
        .any(|e| e.to_lowercase().contains("permission")));
}

#[test]
fn test_symlink_not_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target.txt");
    fs::write(&target, "data").unwrap();
    let link = tmp.path().join("link.txt");
    symlink(&target, &link).unwrap();

    let mut scanner = DirectoryScanner::new();
    scanner.follow_symlinks = false;
    let root = scan_node(&mut scanner, tmp.path());
    let link_node = root.children.iter().find(|c| c.name == "link.txt").unwrap();
    assert_eq!(link_node.size, 0);
}

#[test]
fn test_symlink_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("target.txt");
    fs::write(&target, "hello").unwrap();
    let link = tmp.path().join("link.txt");
    symlink(&target, &link).unwrap();

    let mut scanner = DirectoryScanner::new();
    scanner.follow_symlinks = true;
    let root = scan_node(&mut scanner, tmp.path());
    let link_node = root.children.iter().find(|c| c.name == "link.txt").unwrap();
    assert!(link_node.size > 0);
}

#[test]
fn test_symlink_outside_scan_root_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tmp.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "secret").unwrap();
    let scan_root = tmp.path().join("scan");
    fs::create_dir(&scan_root).unwrap();
    let link = scan_root.join("escape");
    symlink(&outside, &link).unwrap();

    let mut scanner = DirectoryScanner::new();
    scanner.follow_symlinks = true;
    let progress_log = Arc::new(Mutex::new(Vec::new()));
    let root = scan_with_progress(&mut scanner, &scan_root, progress_log.clone());
    assert!(root.scan_complete);
    assert!(!root
        .children
        .iter()
        .any(|c| c.name == "escape" && c.size > 0));
    let events = progress_log.lock().unwrap();
    assert!(events
        .iter()
        .filter_map(|p| p.first_error.as_ref())
        .any(|e| e.to_lowercase().contains("outside scan root")));
}

#[test]
fn test_nonexistent_path() {
    let mut scanner = DirectoryScanner::new();
    let path = PathBuf::from("/nonexistent/filetree-test-path-xyzzy");
    let (root, progress) = scanner.scan(&path, None, None);
    assert!(root.children.is_empty());
    assert!(progress.is_complete);
    assert!(progress.error.is_some());
    assert!(progress
        .error
        .unwrap()
        .contains("/nonexistent/filetree-test-path-xyzzy"));
}

#[test]
fn test_rescan_subtree() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let mut root = scan_node(&mut scanner, &tree);
    let sub_path = tree.join("subdir");
    fs::write(sub_path.join("new.txt"), "added").unwrap();

    let sub = root.find_by_path_mut(&sub_path).unwrap();
    let (ok, progress) = scanner.rescan_subtree(sub, None, None);
    assert!(ok);
    assert!(progress.is_complete);
    assert!(!progress.cancelled);
    let updated = root.find_by_path(&sub_path).unwrap();
    assert!(updated.file_count >= 2);
}

#[test]
fn test_rescan_subtree_file_unchanged() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let mut root = scan_node(&mut scanner, &tree);
    let before = root.find_by_path(&tree.join("readme.md")).unwrap().size;
    let file = root.find_by_path_mut(&tree.join("readme.md")).unwrap();
    let (ok, _) = scanner.rescan_subtree(file, None, None);
    assert!(!ok);
    let after = root.find_by_path(&tree.join("readme.md")).unwrap().size;
    assert_eq!(before, after);
}

#[test]
fn test_rescan_cancel_emits_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let mut root = scan_node(&mut scanner, &tree);
    let sub_path = tree.join("subdir");
    for i in 0..200 {
        fs::write(sub_path.join(format!("file_{i}.dat")), "x".repeat(500)).unwrap();
    }

    let progress_log = Arc::new(Mutex::new(Vec::new()));
    let log = progress_log.clone();
    let cancel = scanner.cancel.clone();
    let done = Arc::new(AtomicBool::new(false));
    let done_flag = done.clone();
    let handle = thread::spawn(move || {
        let progress_cb = Arc::new(move |p: ProgressSnapshot| {
            let items = p.scanned_items;
            log.lock().unwrap().push(p);
            if items > 5 {
                cancel.store(true, Ordering::SeqCst);
            }
        });
        let sub = root.find_by_path_mut(&sub_path).unwrap();
        let (ok, _) = scanner.rescan_subtree(sub, Some(progress_cb), None);
        let _ = ok;
        done_flag.store(true, Ordering::SeqCst);
    });

    let start = std::time::Instant::now();
    while !done.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(5) {
        thread::sleep(Duration::from_millis(10));
    }
    handle.join().unwrap();
    assert!(done.load(Ordering::SeqCst));

    let events = progress_log.lock().unwrap();
    assert!(!events.is_empty());
    assert!(events.last().unwrap().cancelled);
}

#[test]
fn test_progress_callbacks() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let progress_log = Arc::new(Mutex::new(Vec::new()));
    scan_with_progress(&mut scanner, &tree, progress_log.clone());
    let events = progress_log.lock().unwrap();
    assert!(!events.is_empty());
    assert!(events.last().unwrap().is_complete);
}

#[test]
fn test_progress_scanned_items_monotonic() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let progress_log = Arc::new(Mutex::new(Vec::new()));
    scan_with_progress(&mut scanner, &tree, progress_log.clone());
    let events = progress_log.lock().unwrap();
    let mut prev = 0;
    for p in events.iter() {
        assert!(p.scanned_items >= prev);
        prev = p.scanned_items;
    }
}

#[test]
fn test_get_owner_and_allocated() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("owned.txt");
    fs::write(&file, "data").unwrap();
    assert!(!get_owner(&file).is_empty());
    assert!(get_allocated_size(&file, false) > 0);
}

#[test]
fn test_wide_parallel_scan_no_deadlock() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..40 {
        let dir = tmp.path().join(format!("dir{i}"));
        fs::create_dir(&dir).unwrap();
        for j in 0..8 {
            let sub = dir.join(format!("sub{j}"));
            fs::create_dir(&sub).unwrap();
            fs::write(sub.join("file.txt"), "x").unwrap();
        }
    }

    let path = tmp.path().to_path_buf();
    let started = std::time::Instant::now();
    let handle = thread::spawn(move || {
        let mut scanner = DirectoryScanner {
            max_workers: 4,
            ..DirectoryScanner::new()
        };
        scanner.scan(&path, None, None)
    });
    let (root, progress) = handle.join().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(root.scan_complete);
    assert!(progress.is_complete);
    assert_eq!(root.file_count, 40 * 8);
}

#[test]
fn test_parallel_scan_with_cancel() {
    let tmp = tempfile::tempdir().unwrap();
    for i in 0..12 {
        let dir = tmp.path().join(format!("dir{i}"));
        fs::create_dir(&dir).unwrap();
        for j in 0..20 {
            fs::write(dir.join(format!("f{j}.txt")), "x".repeat(50)).unwrap();
        }
    }

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    let handle = thread::spawn(move || {
        let mut scanner = DirectoryScanner {
            cancel: cancel_flag,
            max_workers: 8,
            ..DirectoryScanner::new()
        };
        let progress_cb = Arc::new(move |p: ProgressSnapshot| {
            if p.scanned_dirs > 3 {
                cancel.store(true, Ordering::SeqCst);
            }
        });
        scanner.scan(tmp.path(), Some(progress_cb), None)
    });

    let (_, progress) = handle.join().unwrap();
    assert!(progress.cancelled);
}

#[test]
fn test_volume_bytes_for_path_tempdir() {
    let tmp = tempfile::tempdir().unwrap();
    let total = volume_bytes_for_path(tmp.path());
    assert!(total.is_some());
    assert!(total.unwrap() > 0);
}

#[test]
fn test_volume_bytes_for_path_nonexistent() {
    let missing = PathBuf::from("/nonexistent-filetree-volume-path-xyz");
    assert!(volume_bytes_for_path(&missing).is_none());
}

#[test]
fn test_volume_total_for_full_scan_subdirectory() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(volume_total_for_full_scan(tmp.path()).is_none());
}

#[test]
fn test_scan_emits_tree_patches_during_scan() {
    let tmp = tempfile::tempdir().unwrap();
    let tree = sample_tree(tmp.path());
    let mut scanner = DirectoryScanner::new();
    let updates = Arc::new(Mutex::new(Vec::<(PathBuf, usize, bool, PatchKind)>::new()));
    let log = updates.clone();
    let patch_cb = Arc::new(move |patch: TreePatch| {
        log.lock().unwrap().push((
            patch.node.path.clone(),
            patch.node.children.len(),
            patch.node.scan_complete,
            patch.kind,
        ));
    });
    let root = scanner.scan(&tree, None, Some(patch_cb)).0;
    assert!(root.scan_complete);

    let log = updates.lock().unwrap();
    assert!(!log.is_empty());
    let early = log.iter().any(|(path, child_count, complete, kind)| {
        path == &tree && *child_count >= 3 && !*complete && *kind == PatchKind::Listed
    });
    assert!(
        early,
        "expected intermediate root listing patch before scan completed: {log:?}"
    );
}

/// Live diagnostic: scan `/` briefly and print error breakdown (run with `--ignored`).
#[test]
#[ignore = "manual live diagnostic for full-disk scan errors"]
fn diag_root_scan_errors_live() {
    use std::collections::HashMap;

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = cancel.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(25));
        cancel_flag.store(true, Ordering::SeqCst);
    });

    let mut scanner = DirectoryScanner {
        cancel,
        ..DirectoryScanner::new()
    };

    eprintln!("diag: scanning / for 25s...");
    let (root, progress) = scanner.scan(Path::new("/"), None, None);

    let mut kinds: HashMap<&str, usize> = HashMap::new();
    for e in &progress.errors {
        let kind = if e.to_lowercase().contains("permission") {
            "permission denied"
        } else if e.to_lowercase().contains("outside scan root") {
            "outside scan root"
        } else if e.to_lowercase().contains("cycle") {
            "symlink cycle"
        } else if e.to_lowercase().contains("cannot read") {
            "cannot read"
        } else {
            "other"
        };
        *kinds.entry(kind).or_insert(0) += 1;
    }

    eprintln!("=== live / scan diagnostic ===");
    eprintln!("cancelled: {}", progress.cancelled);
    eprintln!(
        "dirs: {}, items: {}",
        progress.scanned_dirs, progress.scanned_items
    );
    eprintln!("stored errors: {}", progress.errors.len());
    eprintln!("root children: {}", root.children.len());
    eprintln!("error categories: {kinds:?}");
    for e in progress.errors.iter().take(30) {
        eprintln!("  - {e}");
    }
}
