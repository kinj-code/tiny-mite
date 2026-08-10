#!/bin/bash
# Tiny Mite — Linux packaging script
# Creates a distributable AppImage-compatible bundle from the Rust workspace.

set -euo pipefail

APP_NAME="tiny-mite"
VERSION="${1:-0.1.0}"
OUTPUT_DIR="target/release-package"
BUNDLE_DIR="$OUTPUT_DIR/$APP_NAME-$VERSION-linux-x64"
BIN_DIR="$BUNDLE_DIR/bin"
DOCS_DIR="$BUNDLE_DIR/docs"
MODELS_DIR="$BUNDLE_DIR/models"
DATA_DIR="$BUNDLE_DIR/data"

echo "=== Tiny Mite Linux Packaging v$VERSION ==="

# ── Build release binary ───────────────────────────────────
echo "[1/5] Building release binary..."
cargo build --release -p tiny-mite-core 2>/dev/null || {
    echo "  Core binary not yet configured; building all workspace..."
    cargo build --release --workspace 2>&1 | tail -3
}

# ── Create directory structure ──────────────────────────────
echo "[2/5] Creating package structure..."
mkdir -p "$BIN_DIR" "$DOCS_DIR" "$MODELS_DIR" "$DATA_DIR"

# ── Copy binaries ───────────────────────────────────────────
echo "[3/5] Copying binaries..."
if [ -f "target/release/tiny-mite" ]; then
    cp target/release/tiny-mite "$BIN_DIR/"
fi
# Copy all release bins as potential CLI tools
for bin in target/release/tiny-mite-*; do
    if [ -f "$bin" ] && [ -x "$bin" ]; then
        cp "$bin" "$BIN_DIR/"
    fi
done 2>/dev/null || true

# ── Copy documentation ─────────────────────────────────────
echo "[4/5] Copying documentation..."
cp README.md "$DOCS_DIR/" 2>/dev/null || true
cp AGENTS.md "$DOCS_DIR/" 2>/dev/null || true
cp BUILD_MANIFEST.md "$DOCS_DIR/" 2>/dev/null || true
cp -r docs/ "$DOCS_DIR/" 2>/dev/null || true

# ── Create launcher script ──────────────────────────────────
echo "[5/5] Creating launcher..."
cat > "$BUNDLE_DIR/tiny-mite.sh" << 'LAUNCHER'
#!/bin/bash
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
export TINY_MITE_HOME="$SCRIPT_DIR"
export TINY_MITE_MODELS_DIR="$SCRIPT_DIR/models"
export TINY_MITE_DATA_DIR="$SCRIPT_DIR/data"
exec "$SCRIPT_DIR/bin/tiny-mite" "$@"
LAUNCHER
chmod +x "$BUNDLE_DIR/tiny-mite.sh"

# ── Create tarball ──────────────────────────────────────────
echo "  Creating tarball..."
tar -czf "$OUTPUT_DIR/$APP_NAME-$VERSION-linux-x64.tar.gz" \
    -C "$OUTPUT_DIR" "$APP_NAME-$VERSION-linux-x64"

echo ""
echo "=== Package created ==="
echo "  Bundle: $BUNDLE_DIR"
echo "  Archive: $OUTPUT_DIR/$APP_NAME-$VERSION-linux-x64.tar.gz"
echo "  Launch: $BUNDLE_DIR/tiny-mite.sh"
echo ""
echo "  To install: cp -r $BUNDLE_DIR /opt/tiny-mite"
echo "  To run: /opt/tiny-mite/tiny-mite.sh"