# Module reference

Canonical Rust crate at repository root (`src/`).

## `main.rs`

| Symbol | Role |
|--------|------|
| `Args` | clap CLI: `filetree [path]`, `--version` |
| `main()` | Entry; calls `app::run_app()` |

Default path: `/` (macOS). Use `~` explicitly for home.

## `app.rs`

ratatui TUI application.

| Symbol | Role |
|--------|------|
| `App` | Main state: views, modals, scan lifecycle, input handling, menu/toolbar/mouse hit regions |
| `ViewMode` | Tree, TopFiles, Extensions, Volumes |
| `App::dispatch_action(Action)` | Single implementation point for every user intent (keyboard, mouse, menu, toolbar) |
| `handle_key` / `handle_mouse` / `handle_left_click` | Translate input events into `Action`s and hit-test the menu/toolbar/table |
| `run_app(start_path)` | Enables raw mode + mouse capture, runs event loop |

Modals: Help, Confirm, TypedConfirm, PathInput, Export, ScanErrors. FDA uses a non-modal top banner. The top two rows are the clickable menu bar and toolbar; an open dropdown floats above content and below modals.

Key behaviors: filter precompute, live tree merge via `TreeState` + `tree_state.dirty` refresh (~200ms), `ScanBridge` message coalescing, multi-line scan progress panel, delete with TOCTOU guards, export overwrite/redact, mouse click/scroll, `PageUp`/`PageDown`/`Home`/`End` navigation.

`render_table` builds **width-adaptive** columns for the tree view: the `Name` column always takes the leftover width via `Constraint::Fill` (after reserving a proportional minimum) so it can never collapse to zero, and secondary metadata columns are admitted in priority order only while they fit the pane. The Tree view splits the content area 64/36 between the table and the chart panel.

## `menu.rs`

| Symbol | Role |
|--------|------|
| `Action` | Closed enum of every user intent; keyboard/mouse/menu/toolbar all map to it |
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

