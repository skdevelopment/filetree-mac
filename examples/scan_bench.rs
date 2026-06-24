//! Standalone scan benchmark (not shipped). Run with:
//!   cargo run --release --example scan_bench -- <path> [runs]
//!
//! Measures wall-clock time for a full `DirectoryScanner::scan` with no UI,
//! so scanner performance can be compared across changes.

use filetree::scanner::DirectoryScanner;
use std::path::PathBuf;
use std::time::Instant;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let runs: usize = args.next().and_then(|s| s.parse().ok()).unwrap_or(3);

    println!("Benchmarking scan of {} ({runs} runs)", path.display());
    let mut best = f64::MAX;
    let mut last_files = 0u64;
    let mut last_dirs = 0u64;
    let mut last_bytes = 0u64;
    for i in 0..runs {
        let mut scanner = DirectoryScanner::new();
        let start = Instant::now();
        let (root, progress) = scanner.scan(&path, None, None);
        let elapsed = start.elapsed().as_secs_f64();
        best = best.min(elapsed);
        last_files = root.file_count;
        last_dirs = root.folder_count;
        last_bytes = root.size;
        println!(
            "  run {}: {elapsed:.3}s  files={}  dirs={}  bytes={}  errors={}",
            i + 1,
            root.file_count,
            root.folder_count,
            root.size,
            progress.errors.len(),
        );
    }
    let items = last_files + last_dirs;
    println!(
        "best: {best:.3}s  ({:.0} items/s, {:.1} MB/s)  files={last_files} dirs={last_dirs}",
        items as f64 / best,
        last_bytes as f64 / best / 1_000_000.0,
    );
}
