#!/bin/sh
# Install rok-sqlc from a GitHub release on Linux or macOS.
#
#   curl -fsSL https://raw.githubusercontent.com/ateeq1999/rok-sqlc/main/install.sh | sh
#   curl -fsSL https://raw.githubusercontent.com/ateeq1999/rok-sqlc/main/install.sh | sh -s -- --version v0.4.0
#
# Options (each also has an environment variable):
#   --version <tag>   ROK_SQLC_VERSION      release to install (default: latest)
#   --to <dir>        ROK_SQLC_INSTALL_DIR  install directory (default: ~/.local/bin)
#
# The script downloads the archive for this platform, checks it against the
# release's SHA-256 checksum, and copies the `rok-sqlc` binary into the
# install directory. It needs curl or wget, tar, and sha256sum or shasum.
# On Windows, use install.ps1.

set -eu

REPO="ateeq1999/rok-sqlc"
# Where release assets are downloaded from: <base>/<tag>/<file>. Overridable
# for mirrors and for testing the script against local archives.
BASE_URL="${ROK_SQLC_BASE_URL:-https://github.com/$REPO/releases/download}"
VERSION="${ROK_SQLC_VERSION:-latest}"
INSTALL_DIR="${ROK_SQLC_INSTALL_DIR:-${HOME:?HOME is not set}/.local/bin}"

say() { printf 'rok-sqlc-install: %s\n' "$*"; }
err() {
    printf 'rok-sqlc-install: error: %s\n' "$*" >&2
    exit 1
}

usage() {
    cat << 'EOF'
Install rok-sqlc from a GitHub release.

Usage: install.sh [--version <tag>] [--to <dir>]

  --version <tag>  release to install, e.g. v0.4.0 (default: latest)
                   [env: ROK_SQLC_VERSION]
  --to <dir>       install directory (default: ~/.local/bin)
                   [env: ROK_SQLC_INSTALL_DIR]
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || err "--version needs a value"
            VERSION="$2"
            shift 2
            ;;
        --to)
            [ $# -ge 2 ] || err "--to needs a value"
            INSTALL_DIR="$2"
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *) err "unknown option: $1 (see --help)" ;;
    esac
done

if command -v curl > /dev/null 2>&1; then
    download() { curl --proto '=https,file' -fsSL "$1" -o "$2"; }
elif command -v wget > /dev/null 2>&1; then
    download() { wget -q "$1" -O "$2"; }
else
    err "need curl or wget to download rok-sqlc"
fi

if command -v sha256sum > /dev/null 2>&1; then
    sha256() { sha256sum "$1" | cut -d ' ' -f 1; }
elif command -v shasum > /dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
else
    err "need sha256sum or shasum to verify the download"
fi

detect_target() {
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os" in
        Linux) os_part="unknown-linux-musl" ;;
        Darwin)
            os_part="apple-darwin"
            # A shell running under Rosetta reports x86_64 on Apple silicon;
            # install the native binary instead.
            if [ "$arch" = "x86_64" ] &&
                [ "$(sysctl -n hw.optional.arm64 2> /dev/null || echo 0)" = "1" ]; then
                arch="arm64"
            fi
            ;;
        MINGW* | MSYS* | CYGWIN* | Windows_NT)
            err "on Windows, install with install.ps1 (see the README)"
            ;;
        *) err "unsupported operating system: $os" ;;
    esac
    case "$arch" in
        x86_64 | amd64) arch_part="x86_64" ;;
        aarch64 | arm64) arch_part="aarch64" ;;
        *) err "unsupported architecture: $arch (build from source with cargo install rok-sqlc)" ;;
    esac
    echo "$arch_part-$os_part"
}

# Resolves "latest" to a tag without the GitHub API (and its rate limit):
# github.com/<repo>/releases/latest redirects to .../releases/tag/<tag>.
resolve_latest() {
    url="https://github.com/$REPO/releases/latest"
    if command -v curl > /dev/null 2>&1; then
        effective="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$url")" ||
            err "could not look up the latest release"
    else
        effective="$(wget -q --max-redirect=5 -S -O /dev/null "$url" 2>&1 |
            sed -n 's/^ *[Ll]ocation: *//p' | tail -n 1 | tr -d '\r')"
    fi
    tag="${effective##*/}"
    case "$tag" in
        v[0-9]*) echo "$tag" ;;
        *) err "could not find a published release of rok-sqlc" ;;
    esac
}

target="$(detect_target)"
if [ "$VERSION" = "latest" ]; then
    tag="$(resolve_latest)"
else
    case "$VERSION" in
        v*) tag="$VERSION" ;;
        *) tag="v$VERSION" ;;
    esac
fi
version="${tag#v}"
name="rok-sqlc-$version-$target"
archive="$name.tar.gz"

tmp="$(mktemp -d 2> /dev/null || mktemp -d -t rok-sqlc)"
trap 'rm -rf "$tmp"' EXIT INT TERM

say "downloading rok-sqlc $version for $target"
download "$BASE_URL/$tag/$archive" "$tmp/$archive" ||
    err "download failed: $BASE_URL/$tag/$archive (is $tag a release with a $target build?)"
download "$BASE_URL/$tag/$archive.sha256" "$tmp/$archive.sha256" ||
    err "download failed: $BASE_URL/$tag/$archive.sha256"

expected="$(cut -d ' ' -f 1 < "$tmp/$archive.sha256")"
actual="$(sha256 "$tmp/$archive")"
[ -n "$expected" ] && [ "$expected" = "$actual" ] ||
    err "checksum mismatch for $archive (expected $expected, got $actual)"

tar -xzf "$tmp/$archive" -C "$tmp"
[ -f "$tmp/$name/rok-sqlc" ] || err "$archive does not contain rok-sqlc"

mkdir -p "$INSTALL_DIR"
# Copy beside the destination, then rename, so a running rok-sqlc is never
# left half-written.
cp "$tmp/$name/rok-sqlc" "$INSTALL_DIR/.rok-sqlc.tmp"
chmod 755 "$INSTALL_DIR/.rok-sqlc.tmp"
mv -f "$INSTALL_DIR/.rok-sqlc.tmp" "$INSTALL_DIR/rok-sqlc"

say "installed $("$INSTALL_DIR/rok-sqlc" --version) to $INSTALL_DIR/rok-sqlc"

case ":${PATH:-}:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        say "$INSTALL_DIR is not on your PATH; add it with:"
        say "  export PATH=\"$INSTALL_DIR:\$PATH\""
        ;;
esac
