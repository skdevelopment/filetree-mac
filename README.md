# filetree

**A free, open-source alternative to TreeSize and DaisyDisk for macOS.** filetree is a fast, interactive terminal disk-usage analyzer — find what's eating your disk in seconds, with full keyboard **and mouse** navigation. No subscription, no app to buy, no telemetry.

![Rust](https://img.shields.io/badge/rust-1.75%2B-orange)
![macOS 12+](https://img.shields.io/badge/macOS-12%2B-lightgrey)
![License: MIT](https://img.shields.io/badge/license-MIT-green)

<p align="center">
  <img src="demo.png" alt="filetree scanning a full disk — an interactive terminal disk-usage analyzer for macOS" width="900">
</p>

## Why filetree?

| | filetree | TreeSize | DaisyDisk |
|---|:---:|:---:|:---:|
| Price | **Free & open source** | Paid / freemium | Paid |
| Native macOS | ✅ | ⚠️ (Windows-first) | ✅ |
| Runs in the terminal (SSH-friendly) | ✅ | ❌ | ❌ |
| Parallel multi-core scan | ✅ | ✅ | ✅ |
| Keyboard **and** mouse | ✅ | ✅ | mouse-first |
| Top-N files / extension breakdown / volumes | ✅ | ✅ | ✅ |
| No telemetry, no account | ✅ | — | — |

Point it at a folder (or your whole disk) and it shows a TreeSize-style sortable tree of folder sizes, a DaisyDisk-style breakdown of what's largest, and lets you reveal or delete the offenders — all without leaving the terminal.

## Install

### Homebrew (recommended if you use brew)

```bash
brew install skdevelopment/filetree-mac/filetree
```

Homebrew builds from the tagged release (requires the `rust` formula at build time). Upgrades: `brew upgrade filetree`.

To install the latest `main` branch instead:

```bash
brew install --HEAD skdevelopment/filetree-mac/filetree
```

### One-line install (any Mac)

Paste this in Terminal:

```bash
curl -fsSL https://raw.githubusercontent.com/skdevelopment/filetree-mac/main/install.sh | bash
```

The script will:

- Install Rust via rustup if needed (one-time)
- Build and install `filetree` into `~/.local/bin`
- Add `~/.local/bin` to your PATH

Open a **new terminal tab** (or run `source ~/.zshrc`), then:

```bash
filetree
```

### Already have the repo?

```bash
./install.sh
```

### Override defaults (optional)

| Variable | Default | Purpose |
|----------|---------|---------|
| `FILETREE_MODIFY_PATH` | `1` | Add `~/.local/bin` to shell config |
| `FILETREE_AUTO_INSTALL_RUST` | `1` | Install Rust via rustup if `cargo` is missing |
| `FILETREE_GIT_REF` | `main` | Git branch or tag to install |

### Manual install

```bash
cargo build --release
cp target/release/filetree-mac ~/.local/bin/
# Optional: shell wrapper so `filetree` works (see install.sh)
filetree-mac ~
```

## Quick start

```bash
filetree              # scan whole system disk (/) on macOS
filetree ~            # scan home directory
filetree ~/Downloads  # scan specific path
filetree --version
```

## Features

| Feature | Description |
|---------|-------------|
| **Fast parallel scan** | Work-stealing thread pool (rayon) scans every level in parallel; `getattrlistbulk` on macOS |
| **Menu & toolbar** | Clickable `File/View/Sort/Actions/Help` menus and a quick-action toolbar |
| **Mouse support** | Click menus, toolbar, and rows; scroll wheel to navigate |
| **Directory tree** | TreeSize-style expandable tree with per-folder sizes |
| **Chart panel** | Labeled bar chart of selected folder's children (sizes + %) |
| **Size columns** | Allocated size, actual size, % of parent, % of disk, inline bar |
| **Sort** | By name, size, allocated, date, extension, owner, percent |
| **Filter** | Find files/folders by name substring (`/`) |
| **Multiple views** | Tree, Top-100 files (selectable table with full paths), Extension breakdown (ASCII chart), Volumes |
| **Drive list** | All mounted volumes with used/free space bars |
| **Progress** | 6-line scan panel: bytes/items, ETA, current path; brief status bar summary |
| **Refresh** | Rescan selected folder (`r`) or entire tree (`R`) |
| **Delete** | Remove selected item with confirmation (`d`); live progress panel, cancellable (`c`); works from the tree or Top-100 view |
| **Reveal in Finder** | Open selected path in Finder (`f`) |
| **Export** | Save report as `.txt` or `.csv` (`e`) |
| **Symlinks** | Don't follow symlinks by default; toggle with `v` |
| **Hidden files** | Toggle with `H` |
| **Help** | Full shortcut list (`?`) |
| **Status bar** | Total size, item counts, scan path, settings |

## Full Disk Access (required for full scans)

macOS restricts access to protected folders (Mail, Safari data, TCC database, etc.) unless your **terminal app** has **Full Disk Access**.

`install.sh` offers to open the Full Disk Access settings for you right after installing (set `FILETREE_OPEN_FDA=1` to skip the prompt and open it automatically, or `0` to just print instructions). filetree also detects missing access on startup and shows instructions. To grant it manually:

1. Open **System Settings → Privacy & Security → Full Disk Access**
2. Click **+** and add your terminal (**Terminal**, **iTerm**, **Warp**, etc.)
3. Enable the toggle
4. **Quit and restart** the terminal
5. Run `filetree` again

Or press **o** in the FDA dialog to open System Settings directly.

Without FDA, scans still work but some system directories appear empty or inaccessible.

## Cloud storage

Broad scans (`filetree /` or `filetree ~`) skip macOS cloud folders — `~/Library/CloudStorage/*` (iCloud Drive, Google Drive, OneDrive, Dropbox, Nextcloud, …) and `~/Library/Mobile Documents/*`. Their not-downloaded files use ~0 local disk, and scanning them would stall on the network or trigger large downloads. To measure one, scan it directly:

```bash
filetree ~/Library/CloudStorage/GoogleDrive-you@example.com
```

## Keyboard shortcuts

### Navigation
| Key | Action |
|-----|--------|
| `↑`/`↓` or `j`/`k` | Move selection / scroll |
| `←`/`→` or `h`/`l` | Collapse/expand folder |
| `PgUp`/`PgDn` | Scroll one page |
| `Home`/`End` | Jump to first/last row |
| `Enter` | Toggle expand/collapse |
| `g` / `o` | Go to / open path |
| `1`–`4` | Switch views (Tree / Top-N / Extensions / Volumes) |
| `Tab` / `Shift+Tab` | Cycle views |

### Actions
| Key | Action |
|-----|--------|
| `r` | Refresh/rescan selected folder |
| `R` | Rescan entire tree |
| `/` | Filter by name |
| `Esc` | Clear filter / close menu |
| `s` | Cycle sort column |
| `S` | Toggle sort direction |
| `d` | Delete selected (confirmation) |
| `f` | Reveal in Finder |
| `e` | Export report |
| `v` | Toggle follow symlinks |
| `H` | Toggle hidden files |
| `t` | Color theme picker (11 themes) |
| `?` | Help screen |
| `q` | Quit |

### Mouse
| Gesture | Action |
|---------|--------|
| Click menu title | Open `File`/`View`/`Sort`/`Actions`/`Help` dropdown |
| Click toolbar button | Run that action (view tabs, rescan, filter, sort, hidden, theme, export, help, quit) |
| Click a row | Select it; click again to expand/collapse (or scan a volume) |
| Scroll wheel | Scroll the list / chart pane |

## Development

```bash
./install.sh              # or: cargo build --release
cargo test                # Rust integration + unit tests
cargo fmt && cargo clippy -- -D warnings
filetree ~                # run TUI
```

## Project structure

```
filetree-mac/
├── Formula/filetree.rb   # Homebrew (brew install skdevelopment/filetree-mac/filetree)
├── install.sh
├── Cargo.toml
├── README.md
├── LICENSE
├── src/                  # Rust TUI
│   ├── main.rs
│   ├── app.rs
│   ├── session.rs
│   ├── ui/               (modal, views, render, input)
│   ├── scanner.rs
│   └── …
└── tests/                # cargo test
```

## Requirements

- macOS 12+ (Intel or Apple Silicon)
- Rust 1.75+ (`install.sh` installs via rustup if needed; Homebrew pulls in `rust` as a build dependency)
- Terminal with UTF-8 support (for tree/box drawing characters)

## License

MIT — see [LICENSE](LICENSE).