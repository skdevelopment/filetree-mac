# Agent index — filetree-mac

This file is the **entry point for AI agents** (and human contributors) working in this repository. Read it first before making changes.

## Project summary

**filetree** is a TreeSize-style disk usage analyzer for macOS: an interactive ASCII terminal UI built with **Rust**, **ratatui 0.29**, and **crossterm**. Users install via `./install.sh` and run `filetree [path]`.

The **canonical application** is the Rust crate at the repository root (`Cargo.toml`, `src/`). Do not treat `TreeSize/` (Swift prototype) or `build/` as the primary product unless explicitly asked.

---

## Documentation index

| Document | Purpose |
|----------|---------|
| [docs/README.md](docs/README.md) | Docs folder overview and conventions |
| [docs/architecture.md](docs/architecture.md) | System design, data flow, module boundaries |
| [docs/modules.md](docs/modules.md) | Per-module API and responsibilities |
| [docs/development.md](docs/development.md) | Setup, test, lint, release workflow |
| [docs/features.md](docs/features.md) | User-facing feature list and keyboard shortcuts |
| [docs/security.md](docs/security.md) | Path safety, install hardening, FDA |
| [docs/changelog.md](docs/changelog.md) | Version history and notable changes |

User-facing install and quick start remain in [README.md](README.md).

---

## Mandatory rule: keep docs in sync

**Every agent MUST update documentation when changing the codebase.**

This applies to **all** change types:

- New features
- Bug fixes
- Refactors or module moves
- Performance improvements
- Security hardening
- Test-only changes that clarify intended behavior
- Install / packaging / CI changes

### What to update (checklist)

After each change, review this list and edit every doc that is now wrong or incomplete:

1. **[docs/changelog.md](docs/changelog.md)** — Add an entry under `[Unreleased]` (or the current version) describing what changed and why.
2. **[docs/modules.md](docs/modules.md)** — If you added, renamed, or removed modules, functions, classes, or public behavior.
3. **[docs/architecture.md](docs/architecture.md)** — If data flow, threading, or component boundaries changed.
4. **[docs/features.md](docs/features.md)** — If user-visible behavior, shortcuts, views, or CLI flags changed.
5. **[docs/security.md](docs/security.md)** — If delete guards, export rules, install script, or FDA logic changed.
6. **[docs/development.md](docs/development.md)** — If dev setup, test commands, coverage scope, or tooling changed.
7. **[README.md](README.md)** — If install steps, quick start, or high-level feature table changed.
8. **This file ([agent.md](agent.md))** — If new docs are added, the doc index changes, or agent workflow rules change.

### Changelog entry format

Use this structure in `docs/changelog.md`:

```markdown
### Added | Changed | Fixed | Security | Removed
- Short description of the change ([#issue] if applicable)
```

Group under `## [Unreleased]` until a version is released, then move to `## [x.y.z] - YYYY-MM-DD`.

### Definition of done

A task is **not complete** until:

- [ ] Code change is implemented
- [ ] Tests added or updated where appropriate (`cargo test`)
- [ ] `cargo fmt` / `cargo clippy` pass on touched paths
- [ ] Relevant docs from the checklist above are updated
- [ ] `docs/changelog.md` has an `[Unreleased]` entry

---

## Repository map

```
filetree-mac/
├── agent.md              ← you are here
├── README.md             ← user install & quick start
├── Cargo.toml            ← Rust crate (canonical)
├── install.sh            ← one-command installer (cargo build --release)
├── checksums.txt         ← SHA256 of install.sh
├── docs/                 ← agent & contributor reference
├── src/                  ← canonical Rust application
│   ├── main.rs           ← CLI entry (clap)
│   ├── app.rs            ← TUI orchestrator (event loop, session lifecycle, dispatch_action)
│   ├── session.rs        ← ActiveJob (unified scan/delete background worker)
│   ├── ui/               ← modal, views, render, input (TUI presentation)
│   ├── menu.rs           ← ViewMode, Action enum, menu/toolbar defs, key→action
│   ├── progress.rs       ← scan progress panel, rates, ETA helpers
│   ├── scanner.rs        ← parallel directory scanner (rayon work-stealing pool)
│   ├── models.rs         ← ScanNode, progress, volumes, extensions
│   ├── paths.rs          ← delete/path boundary safety
│   ├── platform.rs       ← default scan path (/ on macOS)
│   ├── fda.rs            ← Full Disk Access detection
│   ├── export.rs         ← text/CSV export
│   └── charts.rs         ← ASCII bar charts
└── tests/                ← cargo integration tests
```

Legacy (not canonical): `TreeSize/`, `build/`, `Makefile`, `build.sh`, `run_tests.sh`.

---

## Quick commands

```bash
# One-line install (any Mac)
curl -fsSL https://raw.githubusercontent.com/skdevelopment/filetree-mac/main/install.sh | bash

./install.sh                          # install from local clone
cargo build --release                 # build filetree-mac binary
cargo test                            # run tests
cargo fmt && cargo clippy -- -D warnings
filetree ~                            # run TUI
```

**macOS note:** Accept the Xcode license if build tools fail: `sudo xcodebuild -license accept`.

---

## Agent workflow (recommended)

1. Read **agent.md** (this file) and the docs relevant to your task.
2. Read the modules you will touch (`docs/modules.md` + source).
3. Implement the change with minimal scope.
4. Run tests and lint.
5. Update docs per the checklist above.
6. Summarize what changed and which doc files were updated.

---

## Key constraints

- **macOS-first** — FDA, Finder reveal (`open -R`), and volume listing assume Darwin.
- **Safety** — Deletes are permanent (no Trash). All delete/export/reveal paths must go through `paths.rs` guards.
- **Scan root boundary** — Traversal and export must respect `is_under_scan_root()` when following symlinks; the no-symlink scan hot path may use the lexical `is_under_root_lexical()` (paths stay inside the canonical root by construction).
- **Input parity** — Every user action lives in `menu::Action` and is handled once in `App::dispatch_action`, so keyboard and mouse stay in sync. Add new actions there, not as ad-hoc key handlers.
- **No silent doc drift** — If behavior changes and docs do not, the change is incomplete.