#!/usr/bin/env sh
# scout release installer: fetch a musl release tarball from GitHub,
# verify it, install the binary and the shell snippet, and add the shell
# integration once. The same command installs and updates. Deliberately
# NEVER writes a config into the discovery chain: the trust prompt must
# see your first config, and an update must not silently replace yours.
#
#   curl -fsSL https://raw.githubusercontent.com/kairu1/scout/main/install-release.sh | sh
#   curl -fsSL .../install-release.sh | sh -s -- 0.4.3   # a specific version
#   PREFIX=/opt/tools sh install-release.sh              # install to $PREFIX/bin
#
# Environment: PREFIX (default ~/.local), SCOUT_RC (rc file to add the
# integration to; default ~/.bashrc), SCOUT_NO_RC=1 (do not touch any rc
# file), SCOUT_REPO (owner/name), SCOUT_RELEASE_BASE (download base URL,
# for mirrors and tests).
set -eu

repo="${SCOUT_REPO:-kairu1/scout}"
base="${SCOUT_RELEASE_BASE:-https://github.com/$repo/releases/download}"
prefix="${PREFIX:-$HOME/.local}"
bindir="$prefix/bin"
sharedir="$prefix/share/scout"
rc="${SCOUT_RC:-$HOME/.bashrc}"
marker='# >>> scout shell integration >>>'

fail() { echo "install-release.sh: $*" >&2; exit 1; }

for tool in curl tar; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool not found"
done
if command -v sha256sum >/dev/null 2>&1; then
    checksum="sha256sum -c"
elif command -v shasum >/dev/null 2>&1; then
    checksum="shasum -a 256 -c"
else
    fail "neither sha256sum nor shasum found; refusing to install an unverified binary"
fi

[ "$(uname -s)" = "Linux" ] || fail "release tarballs are Linux (musl) only; use install.sh to build from source"
case "$(uname -m)" in
    x86_64) target=x86_64-unknown-linux-musl ;;
    aarch64|arm64) target=aarch64-unknown-linux-musl ;;
    *) fail "no release for this machine ($(uname -m)); use install.sh to build from source" ;;
esac

# Version: the argument, else the latest release tag.
version="${1:-}"
if [ -z "$version" ]; then
    version="$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" \
        | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)"
    [ -n "$version" ] || fail "could not read the latest release tag from GitHub; pass a version"
fi
version="${version#v}"
name="scout-$version-$target"

previous=""
if [ -x "$bindir/scout" ]; then
    previous="$("$bindir/scout" --version 2>/dev/null || true)"
fi

tmp="$(mktemp -d "${TMPDIR:-/tmp}/scout-install.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT

echo "fetching $name..."
curl -fsSL -o "$tmp/$name.tar.gz" "$base/v$version/$name.tar.gz" \
    || fail "download failed: $base/v$version/$name.tar.gz"
curl -fsSL -o "$tmp/$name.tar.gz.sha256" "$base/v$version/$name.tar.gz.sha256" \
    || fail "checksum download failed"
(cd "$tmp" && $checksum "$name.tar.gz.sha256" >/dev/null) || fail "checksum mismatch for $name.tar.gz; nothing installed"
tar xzf "$tmp/$name.tar.gz" -C "$tmp"
[ -f "$tmp/$name/scout" ] || fail "tarball has no scout binary"

mkdir -p "$bindir" "$sharedir"
install -m755 "$tmp/$name/scout" "$bindir/scout"
install -m644 "$tmp/$name/scout.bash" "$sharedir/scout.bash"
install -m644 "$tmp/$name/config.toml" "$sharedir/config.toml"
installed="$("$bindir/scout" --version)"
if [ -n "$previous" ] && [ "$previous" != "$installed" ]; then
    echo "updated: $previous -> $installed ($bindir/scout)"
else
    echo "installed: $installed ($bindir/scout)"
fi
echo "reference config: $sharedir/config.toml (yours, if any, is untouched)"

# Shell integration, once. The marker is what `scout recon` scans for.
if [ -z "${SCOUT_NO_RC:-}" ]; then
    if [ -f "$rc" ] && grep -Fq "$marker" "$rc"; then
        echo "shell integration: already in $rc"
    else
        printf '\n%s\nsource %s\n# <<< scout shell integration <<<\n' "$marker" "$sharedir/scout.bash" >> "$rc"
        echo "shell integration: added to $rc (open a new shell, or: source $rc)"
    fi
fi

case ":$PATH:" in
    *":$bindir:"*) ;;
    *) echo "note: $bindir is not on your PATH; add: export PATH=\"$bindir:\$PATH\"" ;;
esac

if [ ! -f "${XDG_CONFIG_HOME:-$HOME/.config}/scout/config.toml" ]; then
    cat <<MSG
no config yet (built-in defaults work without one). To start from the reference:
  mkdir -p ~/.config/scout && cp $sharedir/config.toml ~/.config/scout/config.toml
  first launch shows a trust prompt for its actions; answer y.
then: scout index ~/projects
MSG
fi
