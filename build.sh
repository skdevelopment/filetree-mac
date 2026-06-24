#!/bin/zsh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SRC_DIR="$SCRIPT_DIR/TreeSize/Sources"
RES_DIR="$SCRIPT_DIR/TreeSize/Resources"
OUT_APP="$SCRIPT_DIR/build/TreeSize.app"
OUT_MACOS="$OUT_APP/Contents/MacOS"
OUT_RES="$OUT_APP/Contents/Resources"

XCD="/Applications/Xcode.app/Contents/Developer"
SWIFTC="$XCD/Toolchains/XcodeDefault.xctoolchain/usr/bin/swiftc"
SDKROOT="$XCD/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk"
CODESIGN="/usr/bin/codesign"

mkdir -p "$OUT_MACOS" "$OUT_RES"

echo "[build] Compiling Swift sources..."
SWIFT_FILES=("$SRC_DIR/Scanner.swift" "$SRC_DIR/TreeSizeApp.swift")

$SWIFTC \
  -sdk "$SDKROOT" \
  -target arm64-apple-macosx13.0 \
  -o "$OUT_MACOS/TreeSize" \
  "${SWIFT_FILES[@]}" \
  -framework AppKit \
  -framework Foundation \
  -Xlinker -dead_strip

echo "[build] Assembling app bundle..."
cp "$RES_DIR/Info.plist" "$OUT_APP/Contents/Info.plist"
echo "APPL????" > "$OUT_APP/Contents/PkgInfo"

echo "[build] Ad-hoc code signing..."
$CODESIGN --force --deep -s - "$OUT_APP" 2>/dev/null || true

echo "[build] Success: $OUT_APP"
ls -l "$OUT_MACOS/TreeSize" "$OUT_APP/Contents/Info.plist"
