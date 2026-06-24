use filetree::models::{ExtensionStats, PatchKind, ScanNode, SortKey, TreePatch, VolumeInfo};

fn make_tree() -> ScanNode {
    let mut root = ScanNode::new("root", "/root".into(), true);
    root.scan_complete = true;
    root.size = 300;
    root.allocated = 350;
    let mut a = ScanNode::new("a.bin", "/root/a.bin".into(), false);
    a.size = 200;
    a.allocated = 220;
    a.extension = "bin".into();
    a.owner = "alice".into();
    let mut b = ScanNode::new("b.txt", "/root/b.txt".into(), false);
    b.size = 100;
    b.allocated = 120;
    b.extension = "txt".into();
    b.owner = "bob".into();
    b.mtime = 2.0;
    let mut c = ScanNode::new("c.txt", "/root/c.txt".into(), false);
    c.size = 50;
    c.allocated = 60;
    c.extension = "txt".into();
    c.owner = "carol".into();
    c.mtime = 1.0;
    root.add_child(a);
    root.add_child(b);
    root.add_child(c);
    root
}

#[test]
fn test_sorted_children_by_size() {
    let root = make_tree();
    let names: Vec<_> = root
        .sorted_children(SortKey::Size, true)
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["a.bin", "b.txt", "c.txt"]);
}

#[test]
fn test_sorted_children_by_name() {
    let root = make_tree();
    let names: Vec<_> = root
        .sorted_children(SortKey::Name, false)
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names, vec!["a.bin", "b.txt", "c.txt"]);
}

#[test]
fn test_sorted_children_by_extension() {
    let root = make_tree();
    let names: Vec<_> = root
        .sorted_children(SortKey::Extension, false)
        .iter()
        .map(|c| c.name.as_str())
        .collect();
    assert_eq!(names[0], "a.bin");
}

#[test]
fn test_filter_includes_root_when_name_matches() {
    let root = make_tree();
    let matches = root.filter_by_name("root", false);
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "root");
}

#[test]
fn test_find_by_path() {
    let root = make_tree();
    let found = root.find_by_path(std::path::Path::new("/root/b.txt"));
    assert!(found.is_some());
    assert_eq!(found.unwrap().name, "b.txt");
}

#[test]
fn test_percent_of_parent_partial_rollup() {
    let mut parent = ScanNode::new("parent", "/p".into(), true);
    parent.size = 100;
    parent.scan_complete = false;
    let mut child = ScanNode::new("child", "/p/child".into(), false);
    child.size = 25;
    assert_eq!(child.percent_of_parent(Some(&parent)), 25.0);
}

#[test]
fn test_percent_of_parent_incomplete_parent_zero_size() {
    let mut parent = ScanNode::new("parent", "/p".into(), true);
    parent.scan_complete = false;
    let mut child = ScanNode::new("child", "/p/child".into(), false);
    child.size = 25;
    assert_eq!(child.percent_of_parent(Some(&parent)), 0.0);
}

#[test]
fn test_percent_of_parent_normal() {
    let mut parent = ScanNode::new("parent", "/p".into(), true);
    parent.size = 200;
    parent.scan_complete = true;
    let mut child = ScanNode::new("child", "/p/child".into(), false);
    child.size = 50;
    assert_eq!(child.percent_of_parent(Some(&parent)), 25.0);
}

#[test]
fn test_apply_patch_preserves_deeper_children() {
    let mut root = ScanNode::new("root", "/root".into(), true);
    let mut child = ScanNode::new("child", "/root/child".into(), true);
    child
        .children
        .push(ScanNode::new("grand", "/root/child/grand".into(), false));
    root.children.push(child);

    let mut partial = ScanNode::new("root", "/root".into(), true);
    partial.size = 100;
    partial.scan_complete = false;
    partial
        .children
        .push(ScanNode::new("child", "/root/child".into(), true));
    partial.children[0].size = 100;
    partial.children[0].scan_complete = false;

    root.apply_patch(&TreePatch {
        kind: PatchKind::Listed,
        node: partial,
    });
    assert_eq!(root.size, 100);
    assert_eq!(root.children[0].children.len(), 1);
    assert_eq!(root.children[0].children[0].name, "grand");
}

#[test]
fn test_apply_patch_subtree_recurses() {
    let mut root = ScanNode::new("root", "/root".into(), true);
    root.children
        .push(ScanNode::new("child", "/root/child".into(), true));

    let mut partial_child = ScanNode::new("child", "/root/child".into(), true);
    partial_child.size = 50;
    partial_child
        .children
        .push(ScanNode::new("leaf", "/root/child/leaf".into(), false));

    let mut partial = ScanNode::new("root", "/root".into(), true);
    partial.children.push(partial_child);

    root.apply_patch(&TreePatch {
        kind: PatchKind::Subtree,
        node: partial,
    });
    assert_eq!(root.children[0].size, 50);
    assert_eq!(root.children[0].children.len(), 1);
    assert_eq!(root.children[0].children[0].name, "leaf");
}

#[test]
fn test_apply_listed_patch_dedupes_and_is_idempotent() {
    // Guards the O(N) name-indexed child match: a re-emitted listing for a wide
    // directory must update children in place, never duplicate them (and must
    // stay fast — the old linear-scan-per-child made this O(N²)).
    let mut root = ScanNode::new("root", "/root".into(), true);
    let make_patch = |size: u64| {
        let mut node = ScanNode::new("root", "/root".into(), true);
        for i in 0..500 {
            let mut c = ScanNode::new(format!("f{i}"), format!("/root/f{i}").into(), false);
            c.size = size;
            node.children.push(c);
        }
        TreePatch {
            kind: PatchKind::Listed,
            node,
        }
    };

    root.apply_patch(&make_patch(1));
    assert_eq!(root.children.len(), 500);

    root.apply_patch(&make_patch(7));
    assert_eq!(
        root.children.len(),
        500,
        "re-applied listing must not duplicate"
    );
    assert!(
        root.children.iter().all(|c| c.size == 7),
        "stats updated in place"
    );
}

#[test]
fn test_find_by_path_directed_descent_and_miss() {
    let mut root = ScanNode::new("root", "/root".into(), true);
    let mut a = ScanNode::new("a", "/root/a".into(), true);
    let mut b = ScanNode::new("b", "/root/a/b".into(), true);
    b.children
        .push(ScanNode::new("c.txt", "/root/a/b/c.txt".into(), false));
    a.children.push(b);
    root.children.push(a);

    assert_eq!(
        root.find_by_path(std::path::Path::new("/root/a/b/c.txt"))
            .unwrap()
            .name,
        "c.txt"
    );
    assert!(root
        .find_by_path(std::path::Path::new("/root/a/x/c.txt"))
        .is_none());
    assert!(root.find_by_path(std::path::Path::new("/other")).is_none());
}

#[test]
fn test_find_by_path_collapses_private_firmlink() {
    // Lookups must treat macOS firmlinks (`/var` == `/private/var`) as equal
    // without a `realpath()` syscall, in both directions.
    let mut var_root = ScanNode::new("var", "/var".into(), true);
    var_root
        .children
        .push(ScanNode::new("log", "/var/log".into(), true));
    assert_eq!(
        var_root
            .find_by_path(std::path::Path::new("/private/var/log"))
            .unwrap()
            .name,
        "log"
    );

    let mut private_root = ScanNode::new("var", "/private/var".into(), true);
    private_root
        .children
        .push(ScanNode::new("log", "/private/var/log".into(), true));
    assert_eq!(
        private_root
            .find_by_path(std::path::Path::new("/var/log"))
            .unwrap()
            .name,
        "log"
    );
}

#[test]
fn test_volume_info_used_percent() {
    let volume = VolumeInfo {
        name: "Macintosh HD".into(),
        mount_point: "/".into(),
        total_bytes: 1000,
        used_bytes: 250,
        free_bytes: 750,
    };
    assert_eq!(volume.used_percent(), 25.0);
}

#[test]
fn test_extension_stats_display_name() {
    assert_eq!(
        ExtensionStats {
            extension: "txt".into(),
            total_size: 1,
            file_count: 1,
        }
        .display_name(),
        "txt"
    );
    assert_eq!(
        ExtensionStats {
            extension: String::new(),
            total_size: 1,
            file_count: 1,
        }
        .display_name(),
        "(no extension)"
    );
}
