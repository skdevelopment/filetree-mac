use filetree::models::{PatchKind, ProgressSnapshot, ScanNode, ScanProgress, TreePatch};
use filetree::scan_bridge::{ScanBridge, ScanMessage, MAX_MESSAGES_PER_POLL};
use std::path::PathBuf;
use std::sync::mpsc;

fn sample_patch(path: &str) -> TreePatch {
    TreePatch {
        kind: PatchKind::Listed,
        node: ScanNode::new("dir", PathBuf::from(path), true),
    }
}

#[test]
fn test_poll_coalesces_patches_by_path() {
    let (tx, rx) = mpsc::channel();
    let bridge = ScanBridge::new(rx);

    let mut node_a = ScanNode::new("a", "/a".into(), true);
    node_a.size = 1;
    let mut node_a2 = ScanNode::new("a", "/a".into(), true);
    node_a2.size = 2;

    tx.send(ScanMessage::TreePatch(TreePatch {
        kind: PatchKind::Listed,
        node: node_a,
    }))
    .unwrap();
    tx.send(ScanMessage::TreePatch(TreePatch {
        kind: PatchKind::Listed,
        node: node_a2,
    }))
    .unwrap();

    let batch = bridge.poll(MAX_MESSAGES_PER_POLL);
    assert_eq!(batch.patches.len(), 1);
    assert_eq!(
        batch.patches.get(&PathBuf::from("/a")).unwrap().node.size,
        2
    );
}

#[test]
fn test_poll_terminal_stops_processing() {
    let (tx, rx) = mpsc::channel();
    let bridge = ScanBridge::new(rx);

    tx.send(ScanMessage::TreePatch(sample_patch("/late")))
        .unwrap();
    tx.send(ScanMessage::Complete {
        root: ScanNode::new("root", "/".into(), true),
        progress: ScanProgress::default(),
    })
    .unwrap();
    tx.send(ScanMessage::TreePatch(sample_patch("/ignored")))
        .unwrap();

    let batch = bridge.poll(MAX_MESSAGES_PER_POLL);
    assert!(batch.terminal.is_some());
    assert!(!batch.patches.contains_key(&PathBuf::from("/ignored")));
}

#[test]
fn test_poll_respects_message_budget() {
    let (tx, rx) = mpsc::channel();
    let bridge = ScanBridge::new(rx);

    for i in 0..200 {
        tx.send(ScanMessage::Progress(ProgressSnapshot {
            bytes_scanned: i,
            scanned_items: i,
            scanned_dirs: 0,
            error_count: 0,
            cancelled: false,
            is_complete: false,
            current_path: PathBuf::from("/"),
            first_error: None,
        }))
        .unwrap();
    }

    let batch = bridge.poll(10);
    assert!(batch.progress.is_some());
    assert_eq!(batch.progress.unwrap().bytes_scanned, 9);
}
