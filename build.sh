#!/bin/bash
# Build Work Desk, install the `desk` command to ~/.local/bin and, on a Mac,
# Work Desk.app to ~/Applications.
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
# Remove first: copying over a binary in place makes macOS kill it at launch (stale signature cache).
rm -f "$HOME/.local/bin/desk"
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

if [ "$(uname)" = "Darwin" ]; then
  APP="target/release/Work Desk.app"
  VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
  rm -rf "$APP"
  mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
  cp target/release/desk "$APP/Contents/MacOS/desk"
  cp macos/WorkDesk.icns "$APP/Contents/Resources/WorkDesk.icns"
  sed "s/__VERSION__/$VERSION/g" macos/Info.plist > "$APP/Contents/Info.plist"
  # Ad-hoc signature: enough for an app built on this Mac to open without warnings.
  # (Full path because conda/miniforge ship a different `codesign`.)
  /usr/bin/codesign --force --sign - "$APP" >/dev/null

  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/Work Desk.app"
  cp -R "$APP" "$HOME/Applications/"
  touch "$HOME/Applications/Work Desk.app"
  echo "Installed: $HOME/Applications/Work Desk.app"
  echo "(If Work Desk was open, quit it and open it again to get this version.)"
fi

echo
echo "Next:"
echo "  desk key sk-your-deepseek-key"
echo "  open \"\$HOME/Applications/Work Desk.app\"   # the desktop app (or find Work Desk in Spotlight)"
echo "  desk                                     # or the same thing in your browser"
