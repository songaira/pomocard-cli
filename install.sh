#!/usr/bin/env bash
set -euo pipefail

REPO="songaira/pomocard-cli"
DIR="$HOME/.local/bin"
ASSET="pomocard-linux"
DEST="$DIR/pomocard"

mkdir -p "$DIR"
echo "Installing pomocard-cli -> $DEST"

if command -v gh >/dev/null 2>&1; then
    gh release download -R "$REPO" -p "$ASSET" --output "$DEST" --clobber
else
    URL=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
        | grep -o "https://github.com/$REPO/releases/download/[^\"]*/$ASSET" | head -n1)
    if [ -z "$URL" ]; then
        echo "No '$ASSET' in the latest release. Build from source instead: cargo install --path ."
        exit 1
    fi
    curl -fsSL "$URL" -o "$DEST"
fi

chmod +x "$DEST"

case ":$PATH:" in
    *":$DIR:"*) ;;
    *) echo "Add $DIR to your PATH, e.g. export PATH=\"$DIR:\$PATH\" in ~/.bashrc or ~/.zshrc" ;;
esac

echo "Done. Run: pomocard"
