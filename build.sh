#!/bin/bash
# Build Work Desk and install the `desk` command to ~/.local/bin.
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null 2>&1; then
  if [ -x "$HOME/.cargo/bin/cargo" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
  else
    echo "Rust isn't installed yet. Install it (takes about a minute), then run this script again:"
    echo
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    echo
    exit 1
  fi
fi

echo "Building Work Desk (the first build downloads crates and takes a few minutes)…"
cargo build --release

mkdir -p "$HOME/.local/bin"
cp target/release/desk "$HOME/.local/bin/desk"
echo
echo "Installed: $HOME/.local/bin/desk"

case ":$PATH:" in
  *":$HOME/.local/bin:"*) ;;
  *)
    echo
    echo "One more step so your terminal can find it:"
    echo "  echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.zshrc && source ~/.zshrc"
    ;;
esac

echo
echo "Next:"
echo "  desk key sk-your-deepseek-key"
echo "  desk"
