# Module reference

Canonical Rust crate at repository root (`src/`).

## `main.rs`

| Symbol | Role |
|--------|------|
| `Args` | clap CLI: `filetree [path]`, `--version` |
| `main()` | Entry; calls `app::run_app()` |

Default path: `/` (macOS). Use `~` explicitly for home.

## `app.rs`

ratatui TUI orchestrator (~950 lines). Owns `App` state, the event loop, scan/delete session lifecycle, and `dispatch_action`. Rendering, modals, views, and input live under `ui/`.

| Symbol | Role |
|--------|------|
| `App` | Main state: views, `Option<Modal>`, `Option<ActiveJob>`, menu/toolbar/mouse hit regions |
| `App::dispatch_action(Action)` | Single implementation point for every user intent (keyboard, mouse, menu, toolbar) |
| `poll_active_job()` | Poll scan bridge or delete progress each loop iteration |
| `cancel_active_job()` | Cancel whichever background job is running |
| `run_app(start_path)` | Enables raw mode + mouse capture, runs event loop |

Key behaviors: filter precompute, live tree merge via `TreeState` + `tree_state.dirty` refresh (~200ms), `ScanBridge` message coalescing, multi-line scan/delete progress panels, export overwrite/redact.

## `session.rs`

| Symbol | Role |
|--------|------|
| `ActiveJob` | `Scan { bridge, cancel, … }` or `Delete { progress, target, … }` — at most one background job at a time |

## `ui/modal.rs` / `ui/views.rs` / `ui/render.rs` / `ui/input.rs`

| Module | Role |
|--------|------|
| `ui/modal.rs` | `Modal`, `PendingAction`, `handle_modal_key`, `render_modal`, `execute_pending_action` |
| `ui/views.rs` | `TreeRow`, view builders (`build_tree_view`, …), navigation (`scroll_main`, `get_selected_node`, …) |
| `ui/render.rs` | `render`, menu/toolbar/table/chart/progress panels; width-adaptive tree columns (`Name` via `Fill`) |
| `ui/input.rs` | `handle_key` / `handle_mouse` / `handle_left_click`, `delete_selected`, `TerminalGuard` |

Modals: Help, Confirm, TypedConfirm, PathInput, Export, ScanErrors. FDA uses a non-modal top banner. The top two rows are the clickable menu bar and toolbar; an open dropdown floats above content and below modals.

The Tree view splits the content area 64/36 between the table and the chart panel. The **Top-N files** view also renders through `render_table` (its own `#`/`Size`/`%Disk`/`Path` columns, `Path` via `Fill`) so it is a full-width selectable table sharing the tree's `table_state`, click handling, and `get_selected_node` — delete/reveal work there too.

Delete runs on a background worker (`delete.rs`); `poll_active_job()` repaints the delete progress panel and, on completion, reports the result and refreshes the affected subtree. `Action::Cancel` (`c`) cancels an active delete or scan.

## `delete.rs`

| Symbol | Role |
|--------|------|
| `DeleteProgress` | Shared progress for a background delete: atomic item count, `total_items` estimate, done/cancel flags, current path, error slot |
| `run_delete()` | Worker entry point: recursively delete a target, updating `DeleteProgress`, then flag completion |

Recursion uses `symlink_metadata` (never follows links — a symlinked directory is removed as a single entry), removes files then their containing directory (post-order), and checks the cancel flag between entries.

## `menu.rs`

| Symbol | Role |
|--------|------|
| `ViewMode` | Tree, TopFiles, Extensions, Volumes |
| `Action` | Closed enum of every user intent; keyboard/mouse/menu/toolbar all map to it (`Cancel` stops scan or delete) |
| `MENUS` / `Menu` / `MenuItem` | Static menu-bar dropdown definitions (each item carries its key hint) |
| `TOOLBAR` / `ToolButton` | Static toolbar button definitions |
| `key_to_action()` | Pure key-event → `Action` mapping |
| `render_row()` | Lay out a row of labeled cells, returning text + per-cell click spans (shared by render and hit-test) |
| `dropdown_width()` / `dropdown_item_text()` | Pure dropdown geometry/formatting helpers |

## `scan_cache.rs` / `scan_progress.rs` / `scan_traverse.rs` / `macos_dir.rs`

| Symbol | Role |
|--------|------|
| `OwnerCache` | UID→username cache (avoids per-file `getpwuid`) |
| `ProgressThrottle` / `PatchThrottle` / `ErrorPolicy` | Lock-free progress counters (atomics + CAS emit slot); `PatchThrottle` emits the top two tree levels immediately and time-throttles deeper directories (300ms) so the patch channel can't outrun the UI drain; `ErrorPolicy` aggregates permission denials |
| `ScanContext` | Shared scan session (throttles, callbacks, cancel flag, `seen_dirs`, `cloud_skip_roots`); `max_workers` sizes the rayon pool |
| `scan_directory()` | Recursive traversal (`scan_traverse.rs`); child directories scanned in place via rayon `par_iter_mut`; bounded by `MAX_SCAN_DEPTH` |
| `should_visit_dir()` | Per-directory gate: scan-root containment, `(dev, ino)` cycle/duplicate guard (all scan modes — stops firmlink/mount loops), and cloud File Provider skip |
| `cloud_skip_roots_for()` | macOS cloud roots (`~/Library/CloudStorage`, `~/Library/Mobile Documents`) to skip for a broad scan; empty when explicitly scanning cloud |
| `is_under_root_lexical()` via `paths` | Per-directory containment check without `realpath()` (non-symlink scans) |
| `read_dir_fast()` | macOS `getattrlistbulk` with `read_dir` + single-stat fallback |

## `scan_bridge.rs`

| Symbol | Role |
|--------|------|
| `ScanMessage` | Worker → UI: `Progress`, `TreePatch`, `Complete`, `RescanComplete`, `Error` |
| `ScanBridge` | `poll(budget)` coalesces progress + patches; terminal events stop the batch |
| `MAX_MESSAGES_PER_POLL` | Per-frame message budget (64) |

## `tree_state.rs`

| Symbol | Role |
|--------|------|
| `TreeState` | Holds live `root`, `dirty` flag, orphan patch buffer |
| `apply_patch()` | Merge a `Listed` patch into the live tree using syscall-free lexical keys (`paths::lexical_key`); buffers orphans until parent exists |
| `set_root()` | Replace root (move, not merge) with the finished tree from the terminal scan message and retry pending orphans |

## `progress_ui.rs`

| Symbol | Role |
|--------|------|
| `ProgressDisplay` | UI-facing scan counters derived from `ProgressSnapshot` |
| `snapshot_to_scan_progress()` | Rebuild `ScanProgress` for finalize/error modals |

## `progress.rs`

| Symbol | Role |
|--------|------|
| `RateTracker` | Recent-sample byte/item rate tracking |
| `build_scan_progress_panel()` | Format 6-line scan progress panel |
| `scan_progress_percent()` | Byte/volume progress percentage |
| `compute_eta()` / `format_duration_hms()` | ETA and elapsed formatting |
| `truncate_progress_path()` | UTF-8-safe path truncation for progress row |

## `scanner.rs`

| Symbol | Role |
|--------|------|
| `DirectoryScanner` | Public scan API; runs traversal on a sized rayon work-stealing pool (`install_scan`); delegates per-directory work to `scan_traverse.rs` |
| `rollup_chain()` | Recompute size/count rollups for a path and its ancestors bottom-up via O(depth) directed descent (not a whole-tree walk) |
| `fill_node_metadata()` | Lazy metadata fill for visible tree rows (`scan_cache.rs`) |
| `list_volumes()` | Parse `df -Pk`; filter `devfs`/`map`; `statvfs` fallback |
| `volume_bytes_for_path()` | `statvfs` total bytes for a path's volume |
| `volume_total_for_full_scan()` | Volume total only when `scan_root` is a mount root |
| `is_volume_mount_root()` | True for `/` and known `df` mount points |
| `format_bytes()` | Human-readable sizes |
| `collect_largest_files()` | Top-N flat file list |
| `collect_extension_stats()` | Aggregate by extension |
| `parse_df_line()` | POSIX `df -Pk` line parser |

Re-exports chart functions from `charts.rs`.

## `charts.rs`

| Symbol | Role |
|--------|------|
| `ascii_bar_chart()` | ASCII horizontal bars with size and % labels |
| `labeled_children_chart()` | TreeSize-style bar chart of a folder's children |
| `labeled_pie_legend()` | Labeled segment legend for extension breakdown |

## `models.rs`

| Symbol | Role |
|--------|------|
| `ScanNode` | Tree node: path, sizes, counts, children; `apply_patch()` (O(N) via per-merge name index), `find_by_path[_mut]()` (O(depth) directed descent, lexical compare), `listing_patch()`, `subtree_patch()` |
| `TreePatch` / `PatchKind` | Live update payload: listed children vs subtree |
| `ProgressSnapshot` | Lightweight progress for UI thread (no error-vector clones) |
| `SortKey` | Name, Size, Allocated, Date, Extension, Owner, Percent |
| `ScanProgress` | Full scan counters and capped error list (`MAX_STORED_SCAN_ERRORS`) |
| `VolumeInfo` / `ExtensionStats` | Alternate view aggregates |

## `paths.rs`

| Symbol | Role |
|--------|------|
| `normalize_path()` | Canonical absolute path (`realpath`) — used for security boundary checks only |
| `expand_user_path()` | Expand `~` and make relative paths absolute (no canonicalize) — shared by export and modals |
| `dirs_home()` | `$HOME` or `/` fallback |
| `lexical_key()` | Syscall-free comparison key (expand `~`, absolutize, resolve `.`/`..`, collapse `/private` firmlink); used for tree-node identity on the live-merge hot path |
| `is_under_scan_root()` | Symlink-safe boundary check (canonicalizes both paths) |
| `is_under_root_lexical()` | Pure lexical containment check (no syscalls) for the scan hot path |
| `is_delete_protected()` | Denylist + scan-root guards |
| `safe_delete_target()` | TOCTOU re-validation before delete |

## `fda.rs`

| Symbol | Role |
|--------|------|
| `check_full_disk_access()` | Probe protected paths |
| `open_fda_settings()` | `open` System Settings FDA pane |
| `get_terminal_app_name()` | Friendly name for FDA instructions |

## `export.rs`

| Symbol | Role |
|--------|------|
| `export_text()` / `export_csv()` | Report generation |
| `save_report()` | Write file with overwrite guard |
| `export_warning()` / `is_sensitive_export_path()` | Sensitive path warnings |

## `platform.rs`

| Symbol | Role |
|--------|------|
| `default_scan_path()` | `/` on macOS, `~` elsewhere |

