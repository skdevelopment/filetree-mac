# Features

User-facing behavior of the **filetree** TUI. Update this file when shortcuts, views, or workflows change.

## CLI

```bash
filetree [path]     # default path: / (whole system disk on macOS)
filetree --version
```

## Menu bar, toolbar & mouse

The top two rows are an always-visible **menu bar** and **toolbar**, both fully clickable:

- **Menu bar** — `File`, `View`, `Sort`, `Actions`, `Help`. Click a title to open its dropdown; every item lists the equivalent keyboard shortcut, so the full shortcut set is discoverable without opening Help. Click an item to run it; click the title again (or `Esc`) to close.
- **Toolbar** — quick buttons for the four views (active view highlighted) plus `Rescan`, `Filter`, `Sort`, `Hidden`, `Theme`, `Export`, `Help`, `Quit`.
- **Mouse** — click a table row to select it; click the selected row again to expand/collapse it (or scan a volume in the Volumes view); use the **scroll wheel** to scroll the list or chart panes.

Every action is reachable by **both** keyboard and mouse — the two share one dispatcher, so they never diverge.

## Views

| Key | View | Description |
|-----|------|-------------|
| `1` | Tree | Split-pane: expandable tree (left) + labeled bar chart (right) |
| `2` | Top-N | Largest files (default top 100) with labeled size chart |
| `3` | Extensions | Breakdown by file extension with labeled pie-style legend |
| `4` | Volumes | Mounted disks with usage bars; **Enter** on a row starts scan of that mount |
| `Tab` / `Shift+Tab` | — | Cycle views forward / backward |

## Tree columns

- Name (TreeSize-style tree glyphs: `├──`/`└──`, expand `▸`/`▾`, folder/file icons)
- Size (logical)
- Allocated (disk blocks)
- % of parent
- % of disk (relative to scan root)
- Inline size bar (relative to scan root)
- File / folder counts
- Extension, modified date, owner

The columns are **width-adaptive**. The **Name** column (the file list) is always shown and reserves a generous share of the pane; the secondary metadata columns (Size, %, bar, counts, allocated, modified, extension, owner) are filled in by priority only as far as the terminal width allows, so the file list never gets squeezed out on a narrow window. Widen the terminal to reveal more columns.

While a scan is running, the tree **updates live**: the scan root and its top levels appear immediately, deeper folders fill in on a throttled refresh, folders show `…` until their subtree finishes, and sizes roll up as directories complete. Live tree refreshes are deliberately throttled so the merge never competes with the scan for the UI thread; the authoritative complete tree is installed the instant the scan finishes, and the 6-line progress panel gives fine-grained byte/rate/ETA feedback throughout.

On scan complete, the root and first-level folders auto-expand (TreeSize default).

## Chart panel (tree view)

The right pane shows a **labeled bar chart** of the **selected folder's immediate children**: name, bar, size, and percent of parent. Updates when you move the cursor or expand/collapse folders.

## Sort & filter

| Key | Action |
|-----|--------|
| `s` | Cycle sort column |
| `S` | Toggle ascending / descending |
| `/` | Open filter dialog (substring match) |
| `Esc` | Clear filter |

Filter includes the scan root when its name matches; cancel clears the filter bar.

## Scan controls

| Key | Action |
|-----|--------|
| `r` | Rescan selected folder |
| `R` | Rescan entire tree from scan root |
| `c` | Cancel in-progress scan (partial results kept) |
| `g` | Go to path (modal) |
| `v` | Toggle follow symlinks (default: off) |
| `H` | Toggle hidden files |

## File actions

| Key | Action |
|-----|--------|
| `d` | Delete selected item (confirmation; typed confirm for large paths) |
| `f` | Reveal in Finder (`open -R`) |
| `e` | Export report (text or CSV; optional redacted mode) |

Deletes are **permanent** (not moved to Trash). Protected paths (system prefixes, scan root, home, ancestors) are blocked.

## Navigation

| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Move selection / scroll |
| `←`/`→` or `h`/`l` | Collapse / expand |
| `PgUp`/`PgDn` | Scroll one page |
| `Home`/`End` | Jump to first / last row |
| `Enter` | Toggle expand/collapse (tree view) |
| `t` | Color theme picker |
| `?` | Help overlay |
| `q` | Quit |

The mouse scroll wheel and clickable menu/toolbar provide the same navigation and actions (see **Menu bar, toolbar & mouse** above).

## Full Disk Access

`install.sh` prompts for Full Disk Access at install time — it names your terminal app and offers to open **System Settings → Privacy & Security → Full Disk Access** (FDA is granted to the *terminal app*, not the binary). Set `FILETREE_OPEN_FDA=1` to open it without asking, or `0` to only print instructions.

On startup, filetree probes protected macOS locations. The scan **starts immediately**; if FDA is missing or inconclusive, a **top banner** explains how to grant **Full Disk Access** while the scan continues. The banner does not block keyboard input.

- Press **`o`** while the banner is visible to open System Settings
- Press **`Esc`** to dismiss the banner (scan keeps running)

Scans without FDA may show empty or incomplete system directories.

## Cloud storage (skipped by default)

A broad scan (`/`, `~`) does **not** descend into macOS cloud File Provider folders — `~/Library/CloudStorage/*` (iCloud Drive, Google Drive, OneDrive, Dropbox, Nextcloud, …) and `~/Library/Mobile Documents/*` (iCloud). Their not-downloaded ("dataless") files occupy ~0 local disk, and reading them would block on the network and could pull down gigabytes. To include one, **scan it directly**, e.g. `filetree ~/Library/CloudStorage/GoogleDrive-you@example.com`.

## Export

- Formats: plain text, CSV
- Options: redacted export (relative paths, omit owner)
- Warnings for sensitive paths and existing file overwrite
- Rows limited to paths under the scan root

## Fast scan

Scanning speed is the priority. filetree:

- Walks the tree on a **work-stealing thread pool** (rayon), scanning sibling directories in parallel at *every* level — a single huge subtree (e.g. `node_modules`) is split across idle cores instead of stalling on one thread.
- Is **size-first**: it collects sizes and counts in one pass and loads owner/modified metadata only for **expanded** folders in the tree view. Export resolves missing owners on demand.
- On macOS, lists directories with `getattrlistbulk` for fewer kernel round-trips per folder.
- Keeps the hot path lean: no `realpath()` syscall per directory (lexical containment check), lock-free progress counters, and per-file allocations moved rather than copied.

These changes make warm-cache scans roughly **30% faster** than the previous release, with larger gains on cold caches and many-core machines.

## Scan progress panel

While a scan or rescan is active, a **6-line panel** appears above the status bar:

| Row | Content |
|-----|---------|
| 1 | Progress bar + percentage (byte-based when volume total is known) |
| 2 | Data scanned vs volume total, or scanned-only |
| 3 | File count, directory count, items/sec |
| 4 | Elapsed time and ETA (byte-rate based; `calculating…` until rate stabilizes) |
| 5 | Current path (UTF-8-safe truncation) |
| 6 | Error count badge when scan errors occurred |

**Determinate progress** (%, data ratio, ETA) applies only when scanning an entire **volume mount root** (e.g. `filetree /` or a volume listed in the Volumes view). Subdirectory scans (`filetree ~/Documents`) and subtree rescans (`r`) use **indeterminate** mode: bar shows activity, data row shows bytes scanned only, ETA shows `—`.

Press **`c`** to cancel; the panel title changes to **Cancelling…** and partial results are kept.

## Status bar

When idle: scan path, totals, settings (hidden/symlinks), and sort. During scans: brief summary (items, bytes, dirs, errors). Notifications and cancel/error state also appear here.