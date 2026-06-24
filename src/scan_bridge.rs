//! Scan worker → UI message bridge with per-frame coalescing.

use crate::models::{ProgressSnapshot, ScanNode, ScanProgress, TreePatch};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;

pub const MAX_MESSAGES_PER_POLL: usize = 64;

pub enum ScanMessage {
    Progress(ProgressSnapshot),
    TreePatch(TreePatch),
    Complete {
        root: ScanNode,
        progress: ScanProgress,
    },
    RescanComplete {
        path: PathBuf,
        root: ScanNode,
        progress: ScanProgress,
    },
    Error(String),
}

pub enum TerminalScanEvent {
    Complete {
        root: Box<ScanNode>,
        progress: ScanProgress,
        full_scan: bool,
    },
    Error(String),
}

pub struct PollBatch {
    pub progress: Option<ProgressSnapshot>,
    pub patches: HashMap<PathBuf, TreePatch>,
    pub terminal: Option<TerminalScanEvent>,
}

impl PollBatch {
    pub fn is_empty(&self) -> bool {
        self.progress.is_none() && self.patches.is_empty() && self.terminal.is_none()
    }
}

pub struct ScanBridge {
    rx: Receiver<ScanMessage>,
}

impl ScanBridge {
    pub fn new(rx: Receiver<ScanMessage>) -> Self {
        Self { rx }
    }

    pub fn poll(&self, budget: usize) -> PollBatch {
        let mut batch = PollBatch {
            progress: None,
            patches: HashMap::new(),
            terminal: None,
        };
        let limit = budget.min(MAX_MESSAGES_PER_POLL);
        for _ in 0..limit {
            let Ok(msg) = self.rx.try_recv() else {
                break;
            };
            match msg {
                ScanMessage::Progress(p) => batch.progress = Some(p),
                ScanMessage::TreePatch(patch) => {
                    let key = patch.node.path.clone();
                    batch.patches.insert(key, patch);
                }
                ScanMessage::Complete { root, progress } => {
                    batch.terminal = Some(TerminalScanEvent::Complete {
                        root: Box::new(root),
                        progress,
                        full_scan: true,
                    });
                    break;
                }
                ScanMessage::RescanComplete { root, progress, .. } => {
                    batch.terminal = Some(TerminalScanEvent::Complete {
                        root: Box::new(root),
                        progress,
                        full_scan: false,
                    });
                    break;
                }
                ScanMessage::Error(e) => {
                    batch.terminal = Some(TerminalScanEvent::Error(e));
                    break;
                }
            }
        }
        batch
    }
}
