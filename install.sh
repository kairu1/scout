#!/usr/bin/env sh
# scout installer: build from source, install to ~/.local/bin, point at
# the shell snippet. Deliberately NEVER writes a config into the
# discovery chain (ADR-003/ADR-004: a shipped config would pre-trust
# itself; the trust prompt must see your first config).
#
#   ./install.sh                 # install to ~/.local/bin
#   PREFIX=/opt/tools ./install.sh   # install to $PREFIX/bin
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
prefix="${PREFIX:-$HOME/.local}"
bindir="$prefix/bin"

command -v cargo >/dev/null 2>&1 || {
    echo "install.sh: cargo not found — install Rust via https://rustup.rs first" >&2
    exit 1
}

echo "building scout (release)..."
(cd "$here" && cargo build --release)

mkdir -p "$bindir"
install -m755 "$here/target/release/scout" "$bindir/scout"
echo "installed: $bindir/scout ($("$bindir/scout" --version))"

case ":$PATH:" in
    *":$bindir:"*) ;;
    *) echo "note: $bindir is not on your PATH" ;;
esac

cat <<MSG

next steps:
  1. shell integration (makes Enter cd your shell):
       echo 'source $here/shell/scout.bash' >> ~/.bashrc
  2. config (optional — built-in defaults work without one):
       mkdir -p ~/.config/scout
       cp $here/examples/config.toml ~/.config/scout/config.toml
     first launch will show a trust prompt for these actions; answer y.
  3. index something:
       scout index ~/projects
MSG
