# Changelog

All notable changes to **filetree** are documented here.

Format based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).  
Agents: add entries under **[Unreleased]** for every change; move to a version section on release.

## [Unreleased]

## [0.3.0] - 2026-06-24

### Changed
- **Menu bar and toolbar text readable on every theme.** Several palettes (Tokyo Night, Catppuccin, One Dark, Nord) painted menu/toolbar labels with a foreground that matched the bar background — invisible text. Chrome rows now use `filter_fg` on `filter_bg` for inactive buttons and a distinct `selection_bg` highlight for the active view / open menu; Tokyo Night, Catppuccin, and One Dark table-header colors were retuned accordingly.
- **Refactored `src/app.rs` into smaller modules.** The ~3000-line monolith is now split into `session.rs` (`ActiveJob` unifying scan and delete workers), `ui/modal.rs`, `ui/views.rs`, `ui/render.rs`, and `ui/input.rs`. `App` uses `Option<Modal>` (no `Modal::None`) and `active_job: Option<ActiveJob>` instead of separate scan/delete fields. `ViewMode` moved to `menu.rs`; `Action::CancelScan` renamed to `Action::Cancel`. Path helpers `dirs_home` / `expand_user_path` consolidated in `paths.rs`.

### Added
- **Delete now shows live progress.** Deleting a folder previously called `remove_dir_all` synchronously on the UI thread, freezing the app for seconds on large directories with no feedback. Deletion now runs on a background worker that removes one entry at a time and reports the current path and a running item count; a progress panel (bar, `removed / total items`, elapsed, current path) appears while it runs, the UI stays responsive, and **`c` cancels** a delete in progress. New module `src/delete.rs`.
- **Top-N files view is now a selectable, deletable table.** It was static, scrolling text with paths truncated to 56 characters. It is now a full-width table (`#`, `Size`, `%Disk`, `Path`) where each file is **selectable by keyboard and mouse**, the **full path** is shown (wide `Path` column plus the selected file's complete path in the status bar), and you can **delete (`d`)** or **reveal (`f`)** the selected file directly from the view.
- **Five more color themes:** `tokyo-night`, `catppuccin` (Mocha), `one-dark`, `monokai`, and a `light` theme for light-background terminals — 11 built-in themes total, cycled with `t`.

### Fixed
- **Could not type `h` or `l` when confirming a delete or naming an export.** The typed-delete confirmation and the export filename field both reused the vim-style `h`/`l` keys as button navigation, which swallowed those letters before they reached the text field — so folder names / filenames containing `h` or `l` were impossible to type. Those fields now take all letters; only the arrow keys move between buttons.
- **Dracula theme: column headers were purple-on-dark and hard to read** — they now use the near-white Dracula foreground. Purple remains the filter-bar accent (behind dark text).

## [0.2.0] - 2026-06-24

First public release — a free, open-source alternative to TreeSize and DaisyDisk for macOS.

### Fixed
- **The directory-tree file list reappears (Name column no longer collapses).** The tree table's `Name` column was a `Constraint::Percentage`, which ratatui's layout solver squeezes to **zero width** once the fixed-width metadata columns (`Size`, `Alloc`, `%Par`, …) already fill a narrow pane — so the table showed a column of sizes with no file or folder names at all. The columns are now **width-adaptive**: `Name` always takes the leftover space via `Constraint::Fill` after reserving a generous proportional minimum (≥30% of the pane, ≥24 cols), and the secondary columns are added in priority order only while they actually fit. The tree/chart split also shifted from 58/42 to 64/36 so the file list (the primary surface) gets more room. The file list now renders correctly at any terminal width.
- **Full-disk (`/`) scans no longer hang or crash the system.** Two distinct causes:
  - **Directory traversal cycles:** `should_visit_dir` skipped cycle detection entirely when symlinks were not followed (the default). macOS firmlinks and mount points are *real* directories that form loops even without symlinks — e.g. `/System/Volumes/Data/System/Volumes/Data/...` and `/Volumes/<bootdisk>` → `/` — so a `/` scan recursed forever, overflowing a worker's stack / exhausting memory. Cycle detection now runs for **every** scan, keyed by filesystem identity `(dev, ino)`, so each real directory is traversed exactly once. A full `/` scan now completes in seconds (≈1.8M files in ~19s, ~1.2 GB peak) instead of crashing. Added a `MAX_SCAN_DEPTH` (512) backstop against any unforeseen runaway recursion.
  - **Cloud File Provider stalls:** scanning not-downloaded ("dataless") items under `~/Library/CloudStorage/` (iCloud Drive, Google Drive, OneDrive, Dropbox, Nextcloud, …) and `~/Library/Mobile Documents/` blocks on the network in `getdirentries` and can trigger large downloads. Broad scans now skip these cloud roots by default (they occupy ~0 local disk); scan such a folder directly to include it. Previously a `~`/`/` scan could stall indefinitely on cloud folders.

### Added
- **Public release packaging.** Added a top-level `.gitignore` (excludes `/target`, the legacy `/build` Swift `.app` artifact, `/test_output`, and local AI-tooling config) and repositioned the README to market filetree as a **free, open-source alternative to TreeSize and DaisyDisk for macOS**, with a comparison table and a one-line `curl … | bash` install.
- **Installer prompts for Full Disk Access.** `install.sh` now detects your terminal app, explains that FDA is granted to the terminal (not the binary), and offers to open **System Settings → Privacy & Security → Full Disk Access** directly. Controlled by `FILETREE_OPEN_FDA` (`""` = ask when a terminal is attached, `1` = open without asking, `0` = print instructions only). The app also still detects missing FDA on first launch.

### Performance
- **Live-merge no longer freezes the UI during a scan (merge cost cut from tens of seconds to well under a millisecond):** the per-patch tree merge that runs on the UI thread dropped from ~51 s to ~0.2 ms when scanning a 34k-file tree, and from huge stalls to ~0.6 ms on a 238k-file tree. The scanner itself was always fast (~0.5–2 s); the TUI felt slow because the merge was super-linear and syscall-heavy. Fixed by, in order of impact:
  - **No more per-directory `Subtree` patches:** the scanner streamed a full-subtree patch for *every* completed directory (post-order), so each node was re-merged once per ancestor — O(N·depth) work — and the post-order order produced large orphan cascades. The tree structure now flows through cheap pre-order `Listed` patches (direct children only) plus incremental `rollup_chain`, and the finished tree arrives once via the terminal `Complete`/`RescanComplete` message, installed with `set_root` (a move). Streamed patch count for a 238k-file scan fell from ~17,700 to ~15.
  - **O(N) child matching:** `ScanNode::apply_patch` now indexes existing children by name in a `HashMap` once per merge instead of a linear scan per patch child, so merging a directory with N children is O(N) rather than O(N²). A single `target/debug/deps` patch (26,903 entries) dropped from 12.6 s to ~11 ms.
  - **O(total) orphan resolution:** out-of-order patches are buffered keyed by the parent path they wait on and resolved (cascading) only when that parent merges, instead of re-scanning the entire orphan buffer after every patch (previously O(patches·orphans) — ~2.4 s on a deep `/usr` scan, now ~0.5 ms).
  - **No `realpath()` on the merge path:** tree-node lookups/merges (`find_by_path`, `find_by_path_mut`, `child_index_by_path`, `tree_state` merge) now compare paths with the new syscall-free `paths::lexical_key` instead of `normalize_path` (which called `std::fs::canonicalize` on *every node visited*). `lexical_key` collapses the macOS `/private` firmlink prefix lexically so `/var` and `/private/var` still compare equal, and fast-paths already-clean paths to a single allocation.
  - **O(depth) tree descent:** `find_by_path*` and `scanner::rollup_chain` now descend directly along the target path (one component per level) instead of walking the whole tree per patch; `rollup_chain` rolls up the whole ancestor chain in one pass, so the redundant per-parent rollup was removed.
  - **Patch flood bounded:** `PatchThrottle` no longer emits a patch for every small directory (the old `child_count <= 50` bypass); deep directories are time-throttled (300 ms) so the unbounded channel can't outrun the UI drain. Top two levels still emit immediately.
- **Snappier input:** the event loop drains the whole pending input backlog each iteration (bounded by `MAX_EVENTS_PER_TICK`) and redraws once, so held arrow keys and queued mouse events no longer lag behind the 100 ms tick.
- **Faster scans (~30% on warm caches, larger on cold/many-core systems):** directory traversal now runs on a bounded **rayon** work-stealing pool using `par_iter_mut` at every tree level, so deep or lopsided subtrees are split across idle workers instead of pinning a single thread (previously only the scan root's direct children were scanned in parallel)
- Removed two `realpath()` syscalls per directory: the scan-root containment check is now a lexical comparison (`paths::is_under_root_lexical`) when symlinks are not followed, since every visited path is a descendant of the canonical root by construction
- Lock-free progress accounting: per-file and per-directory counters plus the "current path" update use atomics and a CAS-acquired emit slot (`ProgressThrottle`) instead of locking the shared `Mutex<ScanProgress>` on every entry; a bootstrap burst keeps progress responsive on sub-100ms scans
- `BulkEntry` is consumed by value when building child nodes (moving the name/path allocations instead of cloning them per file); child vectors are pre-reserved to entry count

### Added
- **Mouse support:** click the menu bar to open dropdowns, click toolbar buttons and table rows, click a selected row again to expand/collapse (or scan a volume), and scroll with the wheel — via crossterm mouse capture
- **Menu bar + toolbar:** always-visible `File / View / Sort / Actions / Help` menus and a quick-action toolbar; each dropdown item shows its keyboard shortcut, making the full shortcut set discoverable by mouse
- `src/menu.rs` — the shared `Action` vocabulary, menu/toolbar definitions, `key_to_action` mapping, and pure layout/hit-test helpers (unit tested)
- Keyboard: `PageUp`/`PageDown` (scroll one page) and `Home`/`End` (jump to first/last row)
- `App::dispatch_action` — single entry point so keyboard, mouse, menu, and toolbar share one implementation per action
- Headless TUI render and click tests via ratatui `TestBackend`; `examples/scan_bench.rs` standalone scan-benchmark harness

### Fixed
- Full-disk `/` scans no longer exhaust threads (`Resource temporarily unavailable`) — the scanner runs on a bounded rayon work-stealing pool instead of unbounded nested thread fan-out
- Progress mutex deadlock when recording permission-denied errors during traversal
- Permission-denied spam on `/` scans: `ErrorPolicy` aggregates denials and skips `/dev/*` pseudo-fs paths
- Cleared pre-existing clippy lints (`items after a test module` in `paths.rs`/`fda.rs`, `cmp_owned` in `platform_test`, `non_octal_unix_permissions` in `fda_test`/`scanner_test`) so `cargo clippy --all-targets` is warning-free

### Changed
- `Esc` closes an open menu before clearing the active filter
- **Size-first scan (always on):** traversal collects size/allocated in one pass with deferred owner/mtime (filled when folders are expanded in the tree); extensions derived from filenames without extra I/O — no separate fast-scan flag
- Parallel child scans use rayon `par_iter_mut` in place at every tree level; nested parallelism is safe (work-stealing) and no node cloning is needed to hand work to workers
- `scanner.rs` split into `scan_traverse.rs` and `scan_progress.rs`
- Progress and tree-patch UI updates throttled (100ms / 300ms) instead of per-file channel traffic
- macOS directory reads use `getattrlistbulk` when available (`macos_dir.rs`), with single-stat fallback
- Default scan path on macOS remains **`/`** (whole system volume)
- Live scan pipeline rewritten: scanner emits `TreePatch` (listed) + `ProgressSnapshot`; `scan_bridge` coalesces per path with a 64-message poll budget; `tree_state` merges with syscall-free lexical path keys and orphan buffering
- FDA notice is a dismissible top banner (no input capture); scan always starts on launch; `[o]` opens Settings, `[Esc]` dismisses
- `app.rs` delegates scan→UI merge to `tree_state` and message coalescing to `scan_bridge`; progress display moved to `progress_ui.rs`
- Directory tree updates live during scans: children appear as folders are discovered, sizes roll up incrementally, and the view refreshes every ~200ms while scanning
- Scan progress UI: dedicated 6-line panel with byte-based progress bar, data scanned vs volume total, item/dir rates, elapsed time, ETA, current path, and error count; status bar shows a brief summary during scans
- Progress %/ETA only for full volume-mount scans; subdirectory and rescan use indeterminate mode; cancel shows in panel title; session state cleared on error/complete

### Added
- `src/scan_cache.rs` — UID→username cache, lock-free progress/patch/error throttles, `ErrorPolicy`, lazy `fill_node_metadata()`
- `src/scan_progress.rs` — `ScanContext`, progress mutex helpers, throttled error/progress emit
- `src/scan_traverse.rs` — directory listing, child build, unified child dispatch, rollup
- `src/macos_dir.rs` — bulk directory enumeration (`getattrlistbulk` on macOS)
- `ScanNode::metadata_loaded` — tracks deferred owner/mtime during size-first scan
- `src/scan_bridge.rs` — scan worker message types, per-poll coalescing, terminal event handling (`tests/scan_bridge_test.rs`)
- `src/tree_state.rs` — live tree merge with `paths::lexical_key` (syscall-free, firmlink-aware), orphan retry, rollup (`tests/tree_state_test.rs`)
- `examples/merge_bench.rs` — benchmarks the UI-thread live-merge cost (scan → stream patches → `apply_patch`), reporting merge CPU vs scan time and the slowest patches
- `src/progress_ui.rs` — `ProgressDisplay` and `ProgressSnapshot` → status bar mapping
- `ScanNode::apply_patch()`, `TreePatch`, `PatchKind`, `ProgressSnapshot`; capped scan errors (`MAX_STORED_SCAN_ERRORS`)
- `src/progress.rs` — pure helpers for scan rates, ETA, duration formatting, and progress panel layout (`tests/progress_test.rs`)
- `scanner::volume_bytes_for_path()` — `statvfs` volume capacity for progress denominator
- `app_logic::tick_needs_redraw()` for testable event-loop redraw policy

### Removed
- `src/scan_pool.rs` (`ScanScheduler` fixed worker pool / `run_parallel`) — replaced by rayon work-stealing; `crossbeam-channel` dependency dropped
- `DirectoryScanner::fast_scan` — size-first traversal is the only scan path
- Legacy Python/Textual package (`src/filetree/`), pytest suite (`tests/test_*.py`), and `pyproject.toml` — Rust is the only supported runtime

### Fixed
- Live tree path mismatch on macOS (`/var` vs `/private/var`): merges compare via `paths::normalize_path`
- `merge_from` stub misclassification replaced by explicit `PatchKind::Listed` vs `PatchKind::Subtree`
- Progress callbacks no longer clone full `ScanProgress` (including unbounded error vectors) on every emit
- Unbounded channel drain replaced by bounded per-poll batch (`MAX_MESSAGES_PER_POLL = 64`)
- Removed dead `shared_root` shadow tree and duplicate emit/throttle layers in scanner + app
- Poisoned progress mutex cancels scan and records a fatal error instead of silent corruption
- FDA dialog no longer blocks scanning: scan starts on launch; Esc dismisses the banner without cancelling the scan
- Live tree merges preserve deep children via `apply_patch` listing stubs without cloning full subtrees
- macOS launch crash (`SIGKILL`, Code Signature Invalid): Cargo binary renamed to `filetree-mac`; `install.sh` ad-hoc signs the binary and installs a shell wrapper for the `filetree` command
- Live tree update throttle (500ms) and depth-limited scanner emits to reduce memory pressure during large scans
- Live tree `tree_dirty` flag now set on all successful partial-root merge paths (early returns previously skipped UI refresh)
- Partial-root scan messages coalesced per path each poll to reduce clone/memory pressure
- Live tree updates no longer reset when a subdirectory completes (stale shared-root snapshot was replacing the in-progress tree)
- Cancel (`c`) shows **Cancelling…** in progress panel and status immediately via `cancel_requested`, without waiting for worker ack
- Scan worker errors set `status_message` in addition to the notification toast
- `install.sh` installs the release binary as `filetree-mac` and symlinks `filetree` → `filetree-mac` so endpoint security tools that SIGKILL an executable named `filetree` do not block the CLI

### Added
- `src/app_logic.rs` — testable pure helpers for confirm/export/chart-width/table-clamp logic
- `src/util.rs` — UTF-8-safe `truncate_chars` and `bar_fill_len` chart helpers
- Integration tests: `app_logic_test`, `cli_test`, `platform_test`; expanded scanner/paths/export/fda/models coverage
- `#[cfg(test)]` unit tests in `scanner.rs`, `paths.rs`, `export.rs`, `fda.rs`, `charts.rs`
- `fda::build_fda_result()` for deterministic FDA branch testing

### Fixed
- Confirm/TypedConfirm/Export modal selection aligned with rendered labels (Yes/Confirm at index 0; Cancel skips export)
- Per-scan `Arc<AtomicBool>` cancel token prevents overlapping scans from clearing each other's cancel flag
- Post-delete refresh uses background `start_rescan` instead of blocking the TUI thread
- Live scan updates via `PartialRoot` messages and `rollup_ancestors` on shared tree (fixed `shared_root` mutex deadlock)
- Notifications expire after 4s/8s; dirty-flag rendering; `Event::Resize` handling; `TerminalGuard` on panic
- `ScanMessage::RescanComplete` emitted from rescan worker; error when rescan has no loaded root
- `progress.cancelled` set during scan loops; Complete handler prefers final `is_complete` progress snapshot
- Chart/progress bar widths scale to pane/terminal width; TopFiles/Extensions retain split-pane charts
- Volumes chart highlights selected volume; ASCII tree icons; table uses mixed constraints
- Scanner worker `join` uses `unwrap_or_else` fallback; poisoned mutex locks use `into_inner()`
- CLI default path uses `platform::default_scan_path()` when no argument is given
- `export_text` uses char-safe truncation; CSV/text redaction and out-of-root filtering tested
- `safe_delete_target` returns canonical path for delete operations (TOCTOU re-check)

### Added
- **Rust rewrite** — canonical `filetree` binary using **ratatui 0.29** + **crossterm** (repo-root `Cargo.toml`)
- `cargo test` integration tests for scanner, paths, models, export, and FDA (`tests/`)
- `src/filetree/LEGACY.md` marking the Python/Textual package as legacy reference

### Changed
- **Canonical runtime is Rust** — `install.sh` builds `cargo build --release` and installs to `~/.local/bin/filetree`
- Installer auto-installs Rust via rustup when `cargo` is missing (`FILETREE_AUTO_INSTALL_RUST=1`)
- TUI event loop: crossterm input → app state → ratatui render; scan runs in background thread with progress channels
- Version bumped to **0.2.0** for the Rust release

### Removed
- Python virtualenv install path from `install.sh` (Python sources kept under `src/filetree/` for reference)

### Added
- `agent.md` agent index with mandatory documentation sync rules
- `docs/` reference folder: architecture, modules, development, features, security, changelog
- TreeSize-style split-pane UI: expandable directory tree + labeled bar chart panel
- `labeled_children_chart()` and `labeled_pie_legend()` for charts with name, size, and % labels
- `platform.default_scan_path()` — defaults to `/` on macOS (whole system disk)
- Tree columns: `% Disk` and inline size bar; auto-expand root and first-level folders after scan

### Changed
- **One-line install** — `curl -fsSL …/install.sh | bash` works without env vars; auto-installs Python via Homebrew when needed; PATH enabled by default (`FILETREE_MODIFY_PATH=1`)
- Default scan path is `/` on macOS (was `~`); use `filetree ~` to scan home only
- `install.sh` completion message reflects `/` default
- Extension and Top-N views use labeled ASCII charts

### Fixed
- Initialize `DirectoryScanner._pending_futures` in `__init__` (crashed all scans/tests)
- `DataTable.column_count` → `len(ordered_columns)` for current Textual API
- Row selection uses `ordered_rows[row].key` (Textual removed `get_row_key()`)
- Cancelled scans exit thread pool promptly (`shutdown(wait=False, cancel_futures=True)`)
- Thread-pool deadlock when scanning symlink cycles or deep trees (workers now recurse synchronously)
- Symlink cycle detection no longer bypasses `seen_paths` for scan-root revisits
- Lock re-entry deadlock when recording symlink cycle errors during scan
- `ScanErrorsDialog` populates error log after first refresh (Textual mount timing)
- FDA instructions show **Terminal** instead of **Apple_Terminal** for macOS Terminal.app

## [0.1.0] - 2026-06-23

### Added
- Interactive Textual TUI disk usage analyzer for macOS (TreeSize-style)
- Parallel directory scanner with live progress and cancel support
- Views: tree, top-N files, extension breakdown (ASCII chart), volumes
- Sort by name, size, allocated, date, extension, owner, percent
- Filter by name; Tab / Shift+Tab view cycling
- Delete with confirmation and typed confirm for large paths
- Reveal in Finder, text/CSV export with optional redaction
- Full Disk Access detection and System Settings shortcut
- `install.sh` one-command installer with venv and PATH setup
- `checksums.txt` for install script integrity verification
- Path safety module (`paths.py`): scan-root boundaries, TOCTOU delete checks
- Test suite (~75 tests): scanner, paths, FDA, export, models, CLI, TUI pilots

### Security
- Symlink boundary enforcement on scan, delete, reveal, and export
- Install path validation; opt-in shell rc modification
- `FILETREE_ALLOW_CUSTOM_REPO` guard for custom git URLs
- Export overwrite and sensitive-path warnings

### Changed
- README documents Python TUI as canonical implementation

### Removed
- Legacy Swift prototype documented as non-canonical (sources may remain in tree)