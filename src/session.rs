use crate::delete::DeleteProgress;
use crate::scan_bridge::ScanBridge;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

pub(crate) enum ActiveJob {
    Scan {
        bridge: ScanBridge,
        cancel: Arc<AtomicBool>,
        cancel_requested: bool,
        started_at: Instant,
        volume_total_bytes: Option<u64>,
    },
    Delete {
        progress: Arc<DeleteProgress>,
        started_at: Instant,
        label: String,
        target: PathBuf,
    },
}