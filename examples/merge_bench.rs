//! Benchmark the live-merge hot path the TUI runs on its UI thread.
//!
//! `scan_bench` measures raw traversal only. This tool additionally feeds every
//! streamed `TreePatch` through `TreeState::apply_patch` (exactly what
//! `App::poll_scan_messages` does) and the final tree through `set_root`, then
//! reports how much CPU the merge actually costs. If the merge time is a small
//! fraction of the scan time, the UI stays responsive during a scan.
//!
//! Usage: `cargo run --release --example merge_bench -- <path>`

use filetree::models::TreePatch;
use filetree::scanner::DirectoryScanner;
use filetree::tree_state::TreeState;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"));

    let (tx, rx) = mpsc::channel::<TreePatch>();
    let scan_path = path.clone();
    let scan_thread = std::thread::spawn(move || {
        let mut scanner = DirectoryScanner::new();
        let patch_cb = Arc::new(move |patch: TreePatch| {
            let _ = tx.send(patch);
        });
        let start = Instant::now();
        let (root, progress) = scanner.scan(&scan_path, None, Some(patch_cb));
        (root, progress, start.elapsed())
    });

    // Drain + merge on this thread, mirroring the UI loop. Measure only the CPU
    // actually spent inside apply_patch (recv blocking is the scanner's time).
    let merge_scanner = DirectoryScanner::default();
    let mut state = TreeState::default();
    let mut patches = 0u64;
    let mut merge_cpu = Duration::ZERO;
    let mut slow: Vec<(Duration, PathBuf, usize)> = Vec::new();
    while let Ok(patch) = rx.recv() {
        let p = patch.node.path.clone();
        let n = patch.node.children.len();
        let s = Instant::now();
        state.apply_patch(patch, &merge_scanner);
        let dt = s.elapsed();
        merge_cpu += dt;
        patches += 1;
        slow.push((dt, p, n));
    }
    slow.sort_by_key(|s| std::cmp::Reverse(s.0));
    println!("--- slowest patches ---");
    for (dt, p, n) in slow.iter().take(8) {
        println!("  {dt:>12?}  children={n:<6}  {}", p.display());
    }
    println!("---");

    let (root, progress, scan_elapsed) = scan_thread.join().unwrap();
    let files = root.file_count;
    let bytes = root.size;

    let s = Instant::now();
    state.set_root(root);
    let set_root_cpu = s.elapsed();

    let total_merge = merge_cpu + set_root_cpu;
    println!("path:           {}", path.display());
    println!("files:          {files}");
    println!("bytes:          {:.2} MB", bytes as f64 / 1e6);
    println!("scan wall:      {scan_elapsed:?}");
    println!("patches merged: {patches}");
    println!("merge CPU:      {merge_cpu:?}  (apply_patch total)");
    println!("set_root CPU:   {set_root_cpu:?}");
    println!(
        "merge overhead: {:.1}% of scan time",
        total_merge.as_secs_f64() / scan_elapsed.as_secs_f64().max(1e-9) * 100.0
    );
    if !progress.errors.is_empty() {
        println!("(scan reported {} errors)", progress.errors.len());
    }
}
