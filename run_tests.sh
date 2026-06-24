#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
XCD="/Applications/Xcode.app/Contents/Developer"
SWIFTC="$XCD/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc"
SDKROOT="$XCD/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"

mkdir -p test_output

echo "[test] Compiling scanner + test driver using direct toolchain..."
$SWIFTC \
  -sdk "$SDKROOT" \
  -target arm64-apple-macosx13.0 \
  -o test_output/scanner_test \
  TreeSize/Sources/Scanner.swift \
  test_scanner.swift \
  -framework Foundation

echo "[test] Running tests..."
test_output/scanner_test
echo "[test] Scanner tests completed successfully."
