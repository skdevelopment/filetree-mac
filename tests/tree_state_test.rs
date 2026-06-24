use filetree::models::{PatchKind, ScanNode, TreePatch};
use filetree::scanner::DirectoryScanner;
use filetree::tree_state::TreeState;
use std::path::PathBuf;

fn listing_patch(path: &str, children: Vec<ScanNode>) -> TreePatch {
    TreePatch {
        kind: PatchKind::Listed,
        node: ScanNode {
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            path: PathBuf::from(path),
            is_dir: true,
            children,
            ..ScanNode::new("", PathBuf::new(), true)
        },
    }
}

#[test]
fn test_apply_patch_normalized_root_path() {
    let mut state = TreeState::default();
    let scanner = DirectoryScanner::default();

    state.set_root(ScanNode::new("var", "/var".into(), true));

    let mut patch_node = ScanNode::new("var", "/private/var".into(), true);
    patch_node.size = 42;
    let patch = TreePatch {
        kind: PatchKind::Listed,
        node: patch_node,
    };

    assert!(state.apply_patch(patch, &scanner));
    assert_eq!(state.root.as_ref().unwrap().size, 42);
    assert!(state.dirty);
}

#[test]
fn test_orphan_then_parent_ordering() {
    let mut state = TreeState::default();
    let scanner = DirectoryScanner::default();

    state.set_root(ScanNode::new("root", "/root".into(), true));

    let grand_patch = listing_patch("/root/child/grand", vec![]);
    assert!(!state.apply_patch(grand_patch, &scanner));

    let child_patch = listing_patch(
        "/root/child",
        vec![ScanNode::new("grand", "/root/child/grand".into(), false)],
    );
    assert!(state.apply_patch(child_patch, &scanner));

    let root = state.root.as_ref().unwrap();
    assert_eq!(root.children.len(), 1);
    assert_eq!(root.children[0].children.len(), 1);
    assert_eq!(root.children[0].children[0].name, "grand");
}

#[test]
fn test_apply_patch_preserves_existing_grandchildren() {
    let mut state = TreeState::default();
    let scanner = DirectoryScanner::default();

    let mut root = ScanNode::new("root", "/root".into(), true);
    let mut child = ScanNode::new("child", "/root/child".into(), true);
    child
        .children
        .push(ScanNode::new("grand", "/root/child/grand".into(), false));
    root.children.push(child);
    state.set_root(root);

    let mut listed_child = ScanNode::new("child", "/root/child".into(), true);
    listed_child.size = 99;
    let patch = listing_patch("/root", vec![listed_child]);

    assert!(state.apply_patch(patch, &scanner));
    let root = state.root.as_ref().unwrap();
    assert_eq!(root.children[0].size, 99);
    assert_eq!(root.children[0].children.len(), 1);
    assert_eq!(root.children[0].children[0].name, "grand");
}
