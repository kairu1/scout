#!/usr/bin/env sh
# scout release installer: fetch a musl release tarball from GitHub,
# verify it against the checksum published beside it, install the binary
# and the shell snippet, and add the shell integration once. The same
# command installs and updates. Deliberately NEVER writes a config into
# the discovery chain: the trust prompt must see your first config, and
# an update must not silently replace yours.
#
#   curl -fsSL https://raw.githubusercontent.com/kairu1/scout/main/install-release.sh | sh
#   curl -fsSL .../install-release.sh | sh -s -- 0.4.3   # a specific version
#   PREFIX=/opt/tools sh install-release.sh              # install to $PREFIX/bin
#
# Environment: PREFIX (default ~/.local; the binary goes to $PREFIX/bin,
# the snippet and reference config to $PREFIX/share/scout-dist, apart
# from scout's own data dir), SCOUT_RC (rc file to add the integration
# to; default ~/.bashrc), SCOUT_NO_RC (set to any value: touch no rc
# file), SCOUT_REPO (owner/name), SCOUT_RELEASE_BASE (download base URL,
# for mirrors and tests).
set -eu

fail() { echo "install-release.sh: $*" >&2; exit 1; }

: "${HOME:?install-release.sh: HOME is not set; set it, or pass PREFIX and SCOUT_RC}"
repo="${SCOUT_REPO:-kairu1/scout}"
base="${SCOUT_RELEASE_BASE:-https://github.com/$repo/releases/download}"
prefix="${PREFIX:-$HOME/.local}"
bindir="$prefix/bin"
sharedir="$prefix/share/scout-dist"
rc="${SCOUT_RC:-$HOME/.bashrc}"
marker='# >>> scout shell integration >>>'
marker_end='# <<< scout shell integration <<<'

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

# https only (file:// for mirrors and tests), no downgrade on redirect,
# and a bound on a hung mirror.
fetch() {
    curl -fsSL --proto '=https,file' --proto-redir '=https' --max-redirs 5 --max-time 300 "$@"
}

[ "$(uname -s)" = "Linux" ] || fail "release tarballs are Linux (musl) only; use install.sh to build from source"
case "$(uname -m)" in
    x86_64) target=x86_64-unknown-linux-musl ;;
    aarch64|arm64) target=aarch64-unknown-linux-musl ;;
    *) fail "no release for this machine ($(uname -m)); use install.sh to build from source" ;;
esac

# Version: the argument, else the latest release tag.
version="${1:-}"
if [ -z "$version" ]; then
    version="$(fetch "https://api.github.com/repos/$repo/releases/latest" \
        | sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)"
    [ -n "$version" ] || fail "could not read the latest release tag from GitHub; pass a version"
fi
version="${version#v}"
case "$version" in
    ''|*[!0-9A-Za-z._-]*) fail "version '$version' is not a release tag (digits, letters, dots, dashes)" ;;
esac
name="scout-$version-$target"

previous=""
if [ -x "$bindir/scout" ]; then
    previous="$("$bindir/scout" --version 2>/dev/null || true)"
fi

tmp="$(mktemp -d "${TMPDIR:-/tmp}/scout-install.XXXXXX")"
trap 'rm -rf "$tmp"' EXIT INT TERM HUP

echo "fetching $name..."
fetch -o "$tmp/$name.tar.gz" "$base/v$version/$name.tar.gz" \
    || fail "download failed: $base/v$version/$name.tar.gz"
fetch -o "$tmp/$name.tar.gz.sha256" "$base/v$version/$name.tar.gz.sha256" \
    || fail "checksum download failed: $base/v$version/$name.tar.gz.sha256"
grep -q "$name.tar.gz" "$tmp/$name.tar.gz.sha256" \
    || fail "the checksum file does not name $name.tar.gz; nothing installed"
(cd "$tmp" && $checksum "$name.tar.gz.sha256" >/dev/null 2>&1) \
    || fail "checksum mismatch for $name.tar.gz; nothing installed"
tar xzf "$tmp/$name.tar.gz" -C "$tmp" || fail "could not extract $name.tar.gz; nothing installed"

# Every member is checked before anything is installed, so a bad tarball
# leaves the previous install whole.
for member in scout scout.bash config.toml; do
    f="$tmp/$name/$member"
    if [ ! -f "$f" ] || [ -L "$f" ]; then
        fail "tarball has no regular file $member; nothing installed"
    fi
done
chmod 755 "$tmp/$name/scout"
got="$("$tmp/$name/scout" --version 2>/dev/null)" \
    || fail "the downloaded binary does not run on this machine; nothing installed"
[ "$got" = "scout $version" ] \
    || fail "the download reports '$got', not 'scout $version' (mis-tagged release or wrong mirror); nothing installed"

mkdir -p "$bindir" "$sharedir" || fail "could not create $bindir or $sharedir"
install -m755 "$tmp/$name/scout" "$bindir/scout"
install -m644 "$tmp/$name/scout.bash" "$sharedir/scout.bash"
install -m644 "$tmp/$name/config.toml" "$sharedir/config.toml"
if [ -n "$previous" ] && [ "$previous" != "$got" ]; then
    echo "updated: $previous -> $got ($bindir/scout)"
else
    echo "installed: $got ($bindir/scout)"
fi
echo "reference config: $sharedir/config.toml (yours, if any, is untouched)"

# Shell integration, once. The marker is what `scout recon` scans for; a
# block that sources an older location is rewritten to this one.
if [ -z "${SCOUT_NO_RC:-}" ]; then
    source_line="source \"$sharedir/scout.bash\""
    if [ -e "$rc" ] && [ ! -r "$rc" ]; then
        fail "$rc exists but is not readable; fix its mode or set SCOUT_RC"
    fi
    if [ -f "$rc" ] && grep -Fq "$marker" "$rc"; then
        if grep -Fq "$source_line" "$rc"; then
            echo "shell integration: already in $rc"
        else
            awk -v m="$marker" -v e="$marker_end" '$0 == m { skip = 1 } !skip { print } $0 == e { skip = 0 }' "$rc" > "$tmp/rc" \
                || fail "could not rewrite $rc"
            cat "$tmp/rc" > "$rc"
            printf '\n%s\n%s\n%s\n' "$marker" "$source_line" "$marker_end" >> "$rc"
            echo "shell integration: updated in $rc (it sourced an older location)"
        fi
    else
        printf '\n%s\n%s\n%s\n' "$marker" "$source_line" "$marker_end" >> "$rc"
        echo "shell integration: added to $rc (open a new shell, or: source $rc)"
    fi
fi

case ":$PATH:" in
    *":$bindir:"*) ;;
    *) echo "note: $bindir is not on your PATH; add: export PATH=\"$bindir:\$PATH\"" ;;
esac

if [ ! -f "${XDG_CONFIG_HOME:-$HOME/.config}/scout/config.toml" ]; then
    cat <<MSG
no config at ${XDG_CONFIG_HOME:-$HOME/.config}/scout/config.toml (built-in defaults work without one). To start from the reference:
  mkdir -p ~/.config/scout && cp $sharedir/config.toml ~/.config/scout/config.toml
  first launch shows a trust prompt for its actions; answer y.
then: scout index ~/projects
MSG
fi
