# Architecture

## Overview

filetree is a single-process macOS terminal application:

```
CLI (main.rs / clap)
    └── ratatui App (app.rs) — event loop, session lifecycle, dispatch_action
            ├── ui/ (modal, views, render, input) — TUI presentation + input routing
            ├── Menu/toolbar + Action dispatch (menu.rs) — keyboard + mouse → one action vocabulary
            ├── ActiveJob (session.rs)         — unified scan or delete background worker
            ├── ScanBridge (scan_bridge.rs)    — coalesce worker messages per frame
            ├── TreeState (tree_state.rs)      — live merge + orphan buffer
            ├── DirectoryScanner (scanner.rs)  — background scan thread + rayon work-stealing pool
            ├── Delete worker (delete.rs)      — cancellable recursive delete + progress
            ├── FDA check (fda.rs)             — startup banner (non-blocking)
            ├── Export (export.rs)             — on-demand reports
            └── Path safety (paths.rs)         — delete / reveal guards
```

Shared data models live in `models.rs`. Chart helpers in `charts.rs`.

## Input model

Every user intent — whether typed, clicked in the menu bar, clicked on the toolbar,
or clicked on a row — is expressed as a single [`menu::Action`] and handled in one
place, `App::dispatch_action`. `handle_key` maps key events via `menu::key_to_action`;
`handle_mouse`/`handle_left_click` hit-test the menu/toolbar/table click rectangles
(rebuilt each frame from the same `menu::render_row` geometry used to paint them).
This keeps keyboard and mouse capabilities in lockstep: an action added for one is
automatically reachable from the other. The event loop drains the entire pending
input backlog each iteration (bounded by `MAX_EVENTS_PER_TICK`) before redrawing
once, so held keys and bursts of mouse-move events stay responsive rather than
being processed one per 100ms tick.

## Scan pipeline

1. **Start** — User launches `filetree [path]`. `main.rs` calls `run_app(start_path)`.
2. **FDA** — On startup, `App` calls `check_full_disk_access()` and **starts the scan immediately**. If access is missing or inconclusive, a dismissible banner is shown; keyboard input is not captured.
3. **Normalize root** — Scan path is expanded and canonicalized to `scan_root`.
4. **Parallel scan** — `DirectoryScanner::scan()` runs the traversal inside a dedicated **rayon** thread pool sized to `max_workers` (`install_scan`). Each directory scans its child directories in place with `par_iter_mut` (`scan_traverse.rs`); because rayon is work-stealing, this nests safely at every level (no coordinator blocking, no deadlock) and a deep or lopsided subtree is spread across idle workers rather than pinning one thread. Traversal is size-first via `read_dir_fast()` / `getattrlistbulk` on macOS; owner/mtime load lazily when the user expands tree rows. When symlinks are not followed, the per-directory scan-root check is a lexical comparison (`paths::is_under_root_lexical`) with no `realpath()` syscall. **Cycle/duplicate guard:** before descending, `should_visit_dir` records each directory's filesystem identity `(dev, ino)` in a shared set and skips any already seen — for *all* scans, not just symlink-following ones, because macOS firmlinks/mount points (`/System/Volumes/Data`, `/Volumes/<bootdisk>` → `/`) are real directories that otherwise loop forever; a `MAX_SCAN_DEPTH` backstop guards against any missed cycle. **Cloud guard:** broad scans skip macOS File Provider roots (`~/Library/CloudStorage`, `~/Library/Mobile Documents`) whose dataless items would block on the network / trigger downloads — unless the scan root is itself inside one (explicit opt-in). Progress counters are lock-free (`ProgressThrottle` atomics + a CAS-acquired emit slot); `ErrorPolicy` (`scan_cache.rs`) throttles and aggregates permission-denied noise on full-disk scans.
5. **Live updates** — Scanner emits lightweight `ProgressSnapshot` values and `PatchKind::Listed` `TreePatch` messages as directories are listed (top two levels immediately; deeper levels time-throttled to 300ms so the unbounded channel can't outrun the UI drain). The finished tree is **not** streamed as a giant `Subtree` patch — it arrives once via the terminal `Complete`/`RescanComplete` message and is installed with `set_root` (a move). `ScanBridge::poll()` coalesces patches by path (max 64 messages per frame). `TreeState::apply_patch()` merges into the live tree using syscall-free lexical path keys (`paths::lexical_key`), an O(1) name index for child matching, O(depth) path descent, and orphan retry; the view rebuilds every ~200ms while `tree_state.dirty`. This merge runs on the UI thread, so its cost is deliberately kept proportional to the patch size, not the whole tree.
6. **Volume total** — For full scans of a volume mount root only, `volume_total_for_full_scan(scan_root)` via `statvfs` supplies the denominator for progress % and ETA. Subdirectory scans and subtree rescans use indeterminate progress.
7. **Completion** — `ScanProgress.is_complete` is set; errors and cancellation surface in the progress panel (title **Cancelling…** while stopping), status bar, and optional scan-errors modal. `end_scan_session()` clears progress state on complete, cancel, or error.

## Rescan

- **Subtree** (`r`) — Rebuilds a single branch, swaps children, rolls up ancestors via `rollup_chain()`.
- **Full** (`R`) — Re-runs `scan()` from `scan_root`.

## Views

| View | Source | Notes |
|------|--------|-------|
| Tree | `ScanNode` hierarchy | Split pane: table tree + `labeled_children_chart()` panel |
| Top-N | `collect_largest_files()` | Flat list + `ascii_bar_chart()` |
| Extensions | `collect_extension_stats()` + `labeled_pie_legend()` | Labeled segment breakdown |
| Volumes | `list_volumes()` | `df -Pk` parsing; table + chart; Enter scans mount point |

## Threading model

- **Main thread** — crossterm event loop (keyboard + mouse), ratatui render, `Option<ActiveJob>` (at most one background job).
- **Scan thread** — `std::thread::spawn` runs `DirectoryScanner::scan()` / `rescan_subtree_in_tree()`, which in turn drives a rayon pool.
- **Rayon worker pool** — sized to `max_workers` (≈2× cores, capped at 32); workers steal directory subtrees via `par_iter_mut`. `ScanProgress` updates only when a worker wins the CAS emit slot; per-file/dir counters are atomics; `seen_paths` is locked only when following symlinks (cycle detection).
- **Cancel** — `Action::Cancel` (`c`) calls `cancel_active_job()`: shared `Arc<AtomicBool>` for scans (checked at the top of each `scan_directory`), or `DeleteProgress::request_cancel()` for deletes.
- **Delete thread** — `std::thread::spawn` runs `delete::run_delete()`, recursively removing the target one entry at a time and publishing `Arc<DeleteProgress>`. `poll_active_job()` repaints the progress panel and, on completion, refreshes the affected subtree — mirroring the scan pipeline so a large delete never blocks the UI.

## External dependencies

- **ratatui 0.29** + **crossterm 0.28** — TUI framework and terminal I/O (including mouse capture).
- **rayon** — work-stealing thread pool for parallel directory traversal.
- **clap** — CLI parsing.
- **nix** / **libc** — Unix metadata, FDA errno checks.
- **macOS** — `open -R` (Finder), `open x-apple.systempreferences:…` (FDA settings), `df`, `statvfs`.

## Legacy

- **Swift prototype** (`TreeSize/`) — not wired into install.