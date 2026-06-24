#!/usr/bin/env bash
# filetree — one-line macOS installer (Rust)
#
#   curl -fsSL https://raw.githubusercontent.com/skdevelopment/filetree-mac/main/install.sh | bash
#
# Local: ./install.sh

set -euo pipefail

FILETREE_OFFICIAL_GIT_URL="${FILETREE_OFFICIAL_GIT_URL:-https://github.com/skdevelopment/filetree-mac.git}"
FILETREE_OFFICIAL_RAW_BASE="${FILETREE_OFFICIAL_RAW_BASE:-https://raw.githubusercontent.com/skdevelopment/filetree-mac/main}"

FILETREE_GIT_URL="${FILETREE_GIT_URL:-}"
FILETREE_GIT_REF="${FILETREE_GIT_REF:-main}"
FILETREE_ALLOW_CUSTOM_REPO="${FILETREE_ALLOW_CUSTOM_REPO:-0}"
FILETREE_MODIFY_PATH="${FILETREE_MODIFY_PATH:-1}"
FILETREE_AUTO_INSTALL_RUST="${FILETREE_AUTO_INSTALL_RUST:-1}"
# Full Disk Access prompt after install: "" = ask if a terminal is attached,
# 1 = open System Settings without asking, 0 = print instructions only.
FILETREE_OPEN_FDA="${FILETREE_OPEN_FDA:-}"

# macOS deep link to System Settings → Privacy & Security → Full Disk Access
# (kept in sync with FDA_SETTINGS_URL in src/fda.rs).
FDA_SETTINGS_URL="x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles"

INSTALL_DIR="${FILETREE_INSTALL_DIR:-$HOME/.local}"
BIN_DIR="$INSTALL_DIR/bin"
SRC_CACHE="$INSTALL_DIR/share/filetree/src"

FILETREE_REPO="${FILETREE_REPO:-}"
if [[ -z "$FILETREE_REPO" && -n "${BASH_SOURCE[0]:-}" && -f "${BASH_SOURCE[0]}" ]]; then
  _script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  if [[ -f "$_script_dir/Cargo.toml" ]]; then
    FILETREE_REPO="$_script_dir"
  fi
fi

validate_install_dir() {
  local dir="$1"
  if [[ "$dir" == *$'\n'* || "$dir" == *$'\r'* ]]; then
    echo "Error: FILETREE_INSTALL_DIR must not contain newlines." >&2
    exit 1
  fi
  if [[ ! "$dir" =~ ^[A-Za-z0-9_./~@-]+$ ]]; then
    echo "Error: FILETREE_INSTALL_DIR contains invalid characters." >&2
    exit 1
  fi
  INSTALL_DIR="$(cd "$(dirname "$dir")" 2>/dev/null && pwd)/$(basename "$dir")" || INSTALL_DIR="$dir"
  BIN_DIR="$INSTALL_DIR/bin"
}

find_cargo() {
  if command -v cargo &>/dev/null; then
    return 0
  fi
  if [[ -x "$HOME/.cargo/bin/cargo" ]]; then
    export PATH="$HOME/.cargo/bin:$PATH"
    return 0
  fi
  return 1
}

install_rust_via_rustup() {
  echo "==> Installing Rust toolchain via rustup (one-time)..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
}

# Friendly name for the terminal app that needs Full Disk Access. Mirrors
# friendly_terminal_name() in src/fda.rs.
friendly_terminal_name() {
  case "${TERM_PROGRAM:-}" in
    Apple_Terminal)    echo "Terminal" ;;
    iTerm.app)         echo "iTerm" ;;
    WarpTerminal|Warp) echo "Warp" ;;
    vscode)            echo "VS Code" ;;
    Cursor)            echo "Cursor" ;;
    "")                echo "your terminal app" ;;
    *)                 echo "${TERM_PROGRAM//_/ }" ;;
  esac
}

# Guide the user through granting Full Disk Access so a first `filetree /` scan
# can read the whole disk. FDA is granted to the *terminal app*, not the binary.
prompt_full_disk_access() {
  [[ "$OS" == "Darwin" ]] || return 0
  local term; term="$(friendly_terminal_name)"
  echo ""
  echo "==> Full Disk Access (recommended)"
  echo "    A full-disk scan (filetree /) needs macOS Full Disk Access, granted to"
  echo "    your terminal app: $term"
  echo "    System Settings → Privacy & Security → Full Disk Access → add & enable"
  echo "    $term, then quit and reopen it. (filetree also reminds you on first launch.)"

  local do_open="$FILETREE_OPEN_FDA"
  if [[ -z "$do_open" ]]; then
    if [[ -r /dev/tty ]]; then
      printf "    Open Full Disk Access settings now? [Y/n] " > /dev/tty
      local ans=""
      read -r ans < /dev/tty || ans=""
      case "$ans" in
        [Nn]*) do_open=0 ;;
        *)     do_open=1 ;;
      esac
    else
      do_open=0
    fi
  fi

  if [[ "$do_open" == "1" ]]; then
    if open "$FDA_SETTINGS_URL" 2>/dev/null; then
      echo "    Opened System Settings. Add & enable $term, then restart it."
    else
      echo "    Could not open System Settings automatically — open it manually:"
      echo "      open \"$FDA_SETTINGS_URL\""
    fi
  else
    echo "    Skipped. Open it later with:  open \"$FDA_SETTINGS_URL\""
  fi
}

validate_install_dir "$INSTALL_DIR"

echo "==> filetree installer (Rust)"
echo "    Install to: $INSTALL_DIR"

OS="$(uname -s)"
if [[ "$OS" != "Darwin" ]]; then
  echo "Error: filetree requires macOS 12+." >&2
  exit 1
fi

if [[ -z "$FILETREE_REPO" || ! -f "$FILETREE_REPO/Cargo.toml" ]]; then
  if [[ -z "$FILETREE_GIT_URL" ]]; then
    FILETREE_GIT_URL="$FILETREE_OFFICIAL_GIT_URL"
  elif [[ "$FILETREE_GIT_URL" != "$FILETREE_OFFICIAL_GIT_URL" && "$FILETREE_ALLOW_CUSTOM_REPO" != "1" ]]; then
    echo "Error: custom FILETREE_GIT_URL requires FILETREE_ALLOW_CUSTOM_REPO=1." >&2
    exit 1
  fi
  if ! command -v git &>/dev/null; then
    echo "==> git not found — installing Xcode Command Line Tools (one-time)..."
    xcode-select --install 2>/dev/null || true
    echo "    Complete the popup installer, then run this command again." >&2
    exit 1
  fi
  echo "==> Fetching filetree from $FILETREE_GIT_URL @ $FILETREE_GIT_REF"
  if [[ -d "$SRC_CACHE/.git" ]]; then
    git -C "$SRC_CACHE" fetch --depth 1 origin "$FILETREE_GIT_REF" 2>/dev/null || \
      git -C "$SRC_CACHE" fetch --depth 1 origin "refs/tags/$FILETREE_GIT_REF" 2>/dev/null || \
      git -C "$SRC_CACHE" fetch --depth 1 origin
    git -C "$SRC_CACHE" checkout FETCH_HEAD 2>/dev/null || \
      git -C "$SRC_CACHE" checkout "$FILETREE_GIT_REF" 2>/dev/null || true
  else
    rm -rf "$SRC_CACHE"
    if ! git clone --depth 1 --branch "$FILETREE_GIT_REF" "$FILETREE_GIT_URL" "$SRC_CACHE" 2>/dev/null; then
      rm -rf "$SRC_CACHE"
      git clone --depth 1 "$FILETREE_GIT_URL" "$SRC_CACHE"
      git -C "$SRC_CACHE" checkout "$FILETREE_GIT_REF" 2>/dev/null || true
    fi
  fi
  FILETREE_REPO="$SRC_CACHE"
fi

echo "    Source: $FILETREE_REPO"

if ! find_cargo; then
  if [[ "$FILETREE_AUTO_INSTALL_RUST" == "1" ]]; then
    install_rust_via_rustup || exit 1
  else
    echo "Error: Rust (cargo) required. Install from https://rustup.rs" >&2
    exit 1
  fi
fi

RUST_VERSION="$(rustc --version 2>/dev/null || echo unknown)"
echo "    Rust: $RUST_VERSION"

mkdir -p "$BIN_DIR"

echo "==> Building filetree (release)"
(
  cd "$FILETREE_REPO"
  cargo build --release --quiet
)

# Install as filetree-mac: macOS endpoint security SIGKILLs Mach-O binaries whose
# on-disk name is exactly "filetree". The `filetree` command is a shell wrapper
# that execs filetree-mac (argv[0] stays filetree-mac).
rm -f "$BIN_DIR/filetree-mac" "$BIN_DIR/filetree"
cp "$FILETREE_REPO/target/release/filetree-mac" "$BIN_DIR/filetree-mac"
chmod +x "$BIN_DIR/filetree-mac"
if command -v codesign &>/dev/null; then
  codesign -s - --force "$BIN_DIR/filetree-mac" 2>/dev/null || true
fi
cat > "$BIN_DIR/filetree" << 'EOF'
#!/usr/bin/env sh
exec "$(dirname "$0")/filetree-mac" "$@"
EOF
chmod +x "$BIN_DIR/filetree"

append_path_to_rc() {
  local rc_file="$1"
  if [[ -f "$rc_file" ]] && grep -q 'filetree installer PATH' "$rc_file" 2>/dev/null; then
    return 0
  fi
  {
    echo ""
    echo "# filetree installer PATH"
    printf 'export PATH="%s:$PATH"\n' "$BIN_DIR"
  } >> "$rc_file"
  echo "==> Added $BIN_DIR to PATH in $rc_file"
}

if [[ "$FILETREE_MODIFY_PATH" == "1" ]]; then
  case "${SHELL##*/}" in
    zsh)
      append_path_to_rc "$HOME/.zshrc"
      append_path_to_rc "$HOME/.zprofile"
      ;;
    bash) append_path_to_rc "$HOME/.bashrc" ;;
  esac
fi

export PATH="$BIN_DIR:$PATH"

prompt_full_disk_access

echo ""
echo "==> Done! Run:"
echo ""
echo "    filetree          # scan whole system disk (/)"
echo "    filetree ~        # scan home folder only"
echo "    filetree /        # scan entire disk (grant Full Disk Access when asked)"
echo ""
if [[ "$FILETREE_MODIFY_PATH" == "1" ]]; then
  echo "    Open a new terminal tab, or run:  source ~/.zshrc"
  echo ""
fi
if command -v filetree &>/dev/null; then
  filetree --version 2>/dev/null || true
fi