# Development guide

## Prerequisites

- macOS 12+ (Intel or Apple Silicon)
- Rust 1.75+ (`rustup` recommended)
- Accept Xcode license if build tools fail: `sudo xcodebuild -license accept`

## Setup

```bash
cd filetree-mac
cargo build
```

## Run locally

```bash
cargo run -- /tmp
cargo run -- ~
./target/debug/filetree /
```

## Test

```bash
cargo test
cargo test --test scanner_test
cargo test --test paths_test
cargo test --lib                 # unit tests, incl. menu/action + headless TUI render tests
```

Integration tests live in `tests/*.rs` (scanner, paths, export, FDA, models, CLI, app_logic, platform, scan_bridge, tree_state, progress). Unit tests live in `#[cfg(test)]` modules, including `src/menu.rs` (key→action mapping, row layout) and `src/app.rs` (menu/toolbar/dropdown rendering and click hit-testing via ratatui `TestBackend`).

### Scan benchmark

`examples/scan_bench.rs` measures raw scan throughput with no UI:

```bash
cargo run --release --example scan_bench -- <path> [runs]
# e.g. cargo run --release --example scan_bench -- ~/Development 5
```

It prints per-run and best wall-clock time plus items/sec and MB/sec — use it to check scanner performance changes (warm the cache with one throwaway run first).

`examples/merge_bench.rs` measures the **UI-thread live-merge** cost, which `scan_bench` does not exercise:

```bash
cargo run --release --example merge_bench -- <path>
# e.g. cargo run --release --example merge_bench -- ~/Development
```

It scans in a worker thread, feeds every streamed `TreePatch` through `TreeState::apply_patch` (exactly as `App::poll_scan_messages` does), and reports total merge CPU, `set_root` CPU, the slowest individual patches, and merge cost as a percentage of scan time. Merge overhead should be a small fraction of scan time; a large percentage means the merge has regressed to super-linear behavior (e.g. a quadratic child match or a stray `realpath()` on the hot path).

### Release binary signing

Release builds use `strip = true` in `Cargo.toml`. `install.sh` ad-hoc signs the binary after copy (`codesign -s - --force`) so local installs pass basic Gatekeeper checks. Re-sign after any manual rebuild if `spctl` rejects the binary.

## Lint / format

```bash
cargo fmt
cargo clippy -- -D warnings
```

## Packaging / install

```bash
cargo build --release
./install.sh    # installs ~/.local/bin/filetree-mac and symlinks filetree → filetree-mac
```

Version: `Cargo.toml` (`0.2.0`).

## Installer (`install.sh`)

- Detects local clone via `Cargo.toml`
- Clones official repo when piped via `curl | bash`
- Auto-installs Rust via rustup when `cargo` is missing (`FILETREE_AUTO_INSTALL_RUST=1`)
- Builds release binary and symlinks `filetree-mac`

