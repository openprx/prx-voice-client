#!/usr/bin/env bash
# Reproduce the bundled UI fonts in crates/desktop-ui/assets/fonts/.
#
# Sources (official only, pinned):
#   - Hanken Grotesk and Noto Sans SC: github.com/google/fonts at a fixed commit
#   - JetBrains Mono: github.com/JetBrains/JetBrainsMono release v2.304
# The fontTools package is installed into a local virtual environment under
# target/ (never system-wide). Downloads are verified against SHA-256 sums.
#
# Usage: scripts/build-fonts.sh
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/crates/desktop-ui/assets/fonts"
WORK="$ROOT/target/font-build"
VENV="$ROOT/target/font-venv"
FONTTOOLS_VERSION="4.55.3"

GF_COMMIT="869789e84da92782c144d83d69b64cec54905b5f"
GF_RAW="https://raw.githubusercontent.com/google/fonts/$GF_COMMIT/ofl"
JBM_URL="https://github.com/JetBrains/JetBrainsMono/releases/download/v2.304/JetBrainsMono-2.304.zip"

mkdir -p "$WORK" "$OUT"

if [ ! -x "$VENV/bin/python3" ]; then
    python3 -m venv "$VENV"
fi
"$VENV/bin/pip" install -q "fonttools==$FONTTOOLS_VERSION"

fetch() { # url dest sha256
    [ -f "$2" ] || curl -sfL -o "$2" "$1"
    echo "$3  $2" | shasum -a 256 -c - >/dev/null
}

fetch "$GF_RAW/hankengrotesk/HankenGrotesk%5Bwght%5D.ttf" "$WORK/HankenGrotesk-wght.ttf" \
    813b3f8fa0965405669a89b38e51bbefd95eef6b8e20d1cb2d8c10cce062662f
fetch "$GF_RAW/hankengrotesk/OFL.txt" "$WORK/hanken-OFL.txt" \
    e02ccb89a86839b22feff7872ff5cc355cc0f58318d29eee20e2cf83a612f16d
fetch "$GF_RAW/notosanssc/NotoSansSC%5Bwght%5D.ttf" "$WORK/NotoSansSC-wght.ttf" \
    a3041811a78c361b1de50f953c805e0244951c21c5bd412f7232ef0d899af0da
fetch "$GF_RAW/notosanssc/OFL.txt" "$WORK/noto-OFL.txt" \
    1c05c68c34f9708415aada51f17e1b0092d2cea709bf4a94cd38114f9e73d7d9
fetch "$JBM_URL" "$WORK/jbm.zip" \
    6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf
rm -rf "$WORK/jbm" && mkdir -p "$WORK/jbm" && unzip -q "$WORK/jbm.zip" -d "$WORK/jbm"

# Python runs in isolated mode (-I) so nothing in the download directory can
# be imported.
"$VENV/bin/python3" -I "$ROOT/scripts/build_fonts.py" "$WORK" "$OUT"

cp "$WORK/hanken-OFL.txt" "$OUT/OFL-HankenGrotesk.txt"
cp "$WORK/noto-OFL.txt" "$OUT/OFL-NotoSansSC.txt"
cp "$WORK/jbm/OFL.txt" "$OUT/OFL-JetBrainsMono.txt"
ls -l "$OUT"
