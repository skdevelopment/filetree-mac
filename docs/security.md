# Security

filetree is a **local, single-user** tool with no network service. Risk is concentrated in filesystem access, install script behavior, and export/delete actions.

## Threat model

- **Operator** — Legitimate user scanning/deleting their own files.
- **Malicious content** — Symlinks or race conditions inside a scanned directory (e.g. shared folder, cloned repo).
- **Install** — Remote `curl | bash` or tampered git source.

## Path boundaries (`paths.rs`)

| Control | Purpose |
|---------|---------|
| `is_under_scan_root()` | `realpath` (canonicalize both) + component compare vs normalized scan root |
| `is_under_root_lexical()` | Pure component compare, **no** `realpath` — scan hot path only (see below) |
| `lexical_key()` | Syscall-free path identity key (firmlink-aware) for **in-tree node matching only**; **not** a security boundary — see note below |
| `is_delete_protected()` | Block `/`, `~`, system prefixes, scan root, ancestors |
| `safe_delete_target()` | Pre-delete `lstat`, reject symlinks, TOCTOU re-check |

**Scanner:** When `follow_symlinks` is on, entries resolving outside `scan_root` are still checked with the canonicalizing `is_under_scan_root()` and skipped if they escape. When `follow_symlinks` is **off** (the default), the scanner never descends into a symlink (it is recorded as a zero-byte file, never traversed), so every directory it visits is a lexical descendant of the canonical scan root by construction; the per-directory check uses `is_under_root_lexical()` (no syscall) for speed without weakening the boundary. Delete, reveal, and export continue to use the canonicalizing checks (`is_under_scan_root` / `safe_delete_target`) regardless of scan mode.

**Traversal safety (availability):** `should_visit_dir` records each directory's `(dev, ino)` and refuses to re-enter one, for *every* scan mode — without this, macOS firmlink/mount loops (real directories, not symlinks) recurse until a worker overflows its stack or exhausts memory (a local denial-of-service / "crashes the machine" on a default `/` scan). A `MAX_SCAN_DEPTH` backstop bounds depth regardless. Broad scans also do not descend into cloud File Provider roots (`~/Library/CloudStorage`, `~/Library/Mobile Documents`): reading dataless cloud items blocks on the network and can trigger large unintended downloads; scanning such a folder is opt-in (make it the scan root).

**macOS `/private`:** Paths under `/tmp` resolve via `/private/var/...`. Delete denylist must not block in-tree deletes under an active scan root.

**`lexical_key()` is identity, not authorization:** the live-merge hot path (`tree_state`, `ScanNode::find_by_path*`, `child_index_by_path`) compares scan-tree node paths with `lexical_key()` to decide *which existing node a patch refers to*. It performs no `realpath()` and does not resolve symlinks — it only expands `~`, makes the path absolute, resolves `.`/`..`, and collapses a leading `/private` so macOS firmlinks (`/var` ⇄ `/private/var`) match. This is safe because all tree-node paths are built by joining child names onto the (already-canonical) scan root, so they are canonical by construction; the merge never trusts an externally supplied path to grant access. Every action that *touches the filesystem* — delete, reveal, export — still goes through the canonicalizing `is_under_scan_root()` / `safe_delete_target()` guards.

## Delete policy

- Confirmation dialog; typed confirmation for paths ≥ 1 GB
- No Trash integration — `std::fs::remove_file` / `std::fs::remove_dir_all`
- Disabled while `scan_in_progress` (background scan worker active)
- Post-delete subtree refresh uses background `start_rescan`, not a blocking rescan on the UI thread
- Reveal in Finder uses same boundary checks as delete

## Export policy

- Overwrite requires confirmation if file exists
- Extra warning for sensitive home paths (`~/.ssh/*`, dotfiles, etc.)
- Optional **redacted** export for sharing
- Export rows filtered to scan root; sensitive-path banner when FDA areas were scanned

## Install script (`install.sh`)

| Control | Purpose |
|---------|---------|
| `validate_install_dir()` | Reject newlines and unsafe chars in `FILETREE_INSTALL_DIR` |
| `FILETREE_MODIFY_PATH` | Defaults to `1` (PATH added to shell rc); set `0` to skip |
| `FILETREE_AUTO_INSTALL_RUST` | Defaults to `1`; installs Rust via rustup if `cargo` is missing |
| `printf` for PATH lines | Avoid injection in `~/.zshrc` / `~/.bashrc` |
| `FILETREE_ALLOW_CUSTOM_REPO=1` | Required to use non-default `FILETREE_GIT_URL` |
| `FILETREE_GIT_REF` | Default branch `main` for checkout (`install.sh`) |
| `FILETREE_OPEN_FDA` | FDA prompt mode: `""` ask if a terminal is attached, `1` open Settings, `0` instructions only |
| `checksums.txt` | Documented SHA256 verify for `install.sh` |

External commands use explicit argument vectors via `std::process::Command::new` (no shell interpolation), e.g. `df -Pk` in `scanner.rs` and `open` in `fda.rs`. The installer's FDA prompt reads its yes/no answer from `/dev/tty` (so it works under `curl | bash`) and opens only the fixed Apple settings URL via `open` — no interpolation of user input.

## Full Disk Access

FDA probing reads known protected paths to detect missing permissions. This does not bypass TCC; it only informs the user. Instructions name the detected terminal app. FDA is granted to the **terminal app**, not the `filetree` binary (which runs inside it). `install.sh` prompts for FDA after install — detecting the terminal and offering to open the Settings pane (`FILETREE_OPEN_FDA`) — and the app re-detects missing access on first launch.

## Install binary naming

`install.sh` copies the release build to `~/.local/bin/filetree-mac`. The `filetree` command is a small shell wrapper that `exec`s `filetree-mac`. macOS endpoint security SIGKILLs Mach-O binaries whose on-disk filename is exactly `filetree`; the wrapper keeps the familiar command while the running process is `filetree-mac`.

## Dependencies

Runtime deps are pinned in `Cargo.toml` (ratatui, crossterm, rayon, nix, etc.). Audit with `cargo audit` in dev environments.

## When changing security behavior

Update this file, [changelog.md](changelog.md), and tests under `tests/paths_test.rs`, `tests/export_test.rs`, `tests/fda_test.rs`, and install-related docs in [development.md](development.md).