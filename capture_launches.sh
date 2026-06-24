#!/bin/zsh
set -euo pipefail

# capture_launches.sh
# Produces clean launch1.log and launch2.log in {SCRATCH} (or ./scratch if unset)
# by running the real GUI entry point (no EXIT bypass, full population path) under debug.
# Uses a small temporary fixture and TREESIZE_SCAN_DIR so the run is fast and deterministic.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
APP_BIN="$SCRIPT_DIR/build/TreeSize.app/Contents/MacOS/TreeSize"

SCRATCH="${SCRATCH:-/var/folders/z7/5cg0y7l16kq0hfsbym48fkfw0000gq/T/grok-goal-ebc193b086ea/implementer}"
mkdir -p "$SCRATCH"

if [ ! -x "$APP_BIN" ]; then
  echo "[capture] Building..."
  "$SCRIPT_DIR/build.sh"
fi

FIX="$SCRIPT_DIR/test_output/capture-fixture"
rm -rf "$FIX"
mkdir -p "$FIX/a/b" "$FIX/c"
dd if=/dev/zero of="$FIX/a/big.bin" bs=1024 count=3 2>/dev/null || true
dd if=/dev/zero of="$FIX/a/b/med.bin" bs=1024 count=1 2>/dev/null || true
dd if=/dev/zero of="$FIX/c/s.bin" bs=256 count=1 2>/dev/null || true

echo "[capture] Capturing launch1.log (debug, full GUI path, small fixture)..."
( env TREESIZE_DEBUG=1 TREESIZE_SCAN_DIR="$FIX" "$APP_BIN" > "$SCRATCH/launch1.log" 2>&1 & PID=$! ; sleep 2.5 ; kill $PID 2>/dev/null || true ; wait $PID 2>/dev/null || true ) || true
echo "LAUNCH1_CAPTURED" >> "$SCRATCH/launch1.log"

echo "[capture] Capturing launch2.log (debug, full GUI path)..."
( env TREESIZE_DEBUG=1 TREESIZE_SCAN_DIR="$FIX" "$APP_BIN" > "$SCRATCH/launch2.log" 2>&1 & PID=$! ; sleep 2.5 ; kill $PID 2>/dev/null || true ; wait $PID 2>/dev/null || true ) || true
echo "LAUNCH2_CAPTURED" >> "$SCRATCH/launch2.log"

echo "[capture] Done. Logs:"
ls -l "$SCRATCH/launch1.log" "$SCRATCH/launch2.log"

# Optional self-check (does not fail the script for harness; verification will assert)
if grep -q 'APPLY_SCAN_RESULT\|CHART_DRAW\|DETAILS_VIEWFOR\|OUTLINE_VIEWFOR\|SELECTION_NAVIGATED\|OUTLINE_EXPANDED' "$SCRATCH/launch1.log" && \
   grep -q 'APPLY_SCAN_RESULT\|CHART_DRAW\|DETAILS_VIEWFOR\|OUTLINE_VIEWFOR\|SELECTION_NAVIGATED\|OUTLINE_EXPANDED' "$SCRATCH/launch2.log"; then
  echo "[capture] Key GUI population events present in both logs."
else
  echo "[capture] Warning: some expected debug events not found (may be timing); verification will check."
fi

rm -rf "$FIX"
