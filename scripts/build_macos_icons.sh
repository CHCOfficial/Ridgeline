#!/bin/zsh
set -euo pipefail

ROOT_DIR="${0:A:h:h}"
ICON_DIR="$ROOT_DIR/assets/icons/macos"
PYTHON_BIN="${CODEX_PYTHON:-python3}"

if ! "$PYTHON_BIN" -c 'from PIL import Image' >/dev/null 2>&1; then
  print -u2 "Pillow is required. Set CODEX_PYTHON to a Python installation containing Pillow."
  exit 1
fi

"$PYTHON_BIN" "$ROOT_DIR/scripts/prepare_macos_icons.py"
print "Built Classic, Party, and Contour macOS icon families."
