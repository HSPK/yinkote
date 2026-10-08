#!/bin/sh
# Keep execution behind a complete function definition when piped into sh.
yinkote_install() (
    set -eu

    fail() { printf 'yinkote: %s\n' "$*" >&2; exit 1; }
    case "${1:-}" in
        -h|--help)
            printf '%s\n' \
                'Install the latest Yinkote release for macOS or glibc Linux.' \
                'Usage: sh install.sh' \
                'YINKOTE_INSTALL_DIR overrides the default $HOME/.local/bin.' \
                'No sudo, shell profile changes, or service restarts are performed.'
            exit 0
            ;;
        '') ;;
        *) fail "unknown argument: $1" ;;
    esac
    [ "$#" -le 1 ] || fail 'unexpected arguments'
    for tool in curl uname mktemp awk chmod mv grep tr mkdir rm rmdir; do
        command -v "$tool" >/dev/null 2>&1 || fail "required command not found: $tool"
    done
    if command -v sha256sum >/dev/null 2>&1; then
        checksum() { sha256sum "$1"; }
    elif command -v shasum >/dev/null 2>&1; then
        checksum() { shasum -a 256 "$1"; }
    else
        fail 'SHA-256 verification requires sha256sum or shasum'
    fi

    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Darwin)
            # Prefer the native binary even when the shell is running in Rosetta.
            if [ "$arch" = x86_64 ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || :)" = 1 ]; then
                arch=arm64
            fi
            case "$arch" in
                arm64|aarch64) target=aarch64-apple-darwin ;;
                x86_64|amd64) target=x86_64-apple-darwin ;;
                *) fail "unsupported macOS architecture: $arch" ;;
            esac
            ;;
        Linux)
            if command -v ldd >/dev/null 2>&1 && ldd --version 2>&1 | grep -qi musl; then
                fail 'Alpine/musl is not supported by the published glibc binaries'
            fi
            case "$arch" in
                aarch64|arm64) target=aarch64-unknown-linux-gnu ;;
                x86_64|amd64) target=x86_64-unknown-linux-gnu ;;
                *) fail "unsupported Linux architecture: $arch" ;;
            esac
            ;;
        *) fail "unsupported OS: $os (Windows users should use install.ps1)" ;;
    esac

    repo=https://github.com/HSPK/yinkote
    fetch() {
        curl --fail --silent --show-error --location \
            --proto '=https' --proto-redir '=https' --tlsv1.2 \
            --retry 3 --connect-timeout 15 --max-time 300 "$@"
    }
    latest=$(fetch --head --output /dev/null --write-out '%{url_effective}' "$repo/releases/latest") \
        || fail 'could not resolve the latest GitHub release'
    case "$latest" in
        "$repo"/releases/tag/*) tag=${latest##*/} ;;
        *) fail "unexpected release URL: $latest" ;;
    esac
    printf '%s\n' "$tag" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+$' \
        || fail "unsupported release tag: $tag"
    version=${tag#v}

    updating=${YINKOTE_UPDATE_TARGET:-}
    if [ -n "$updating" ]; then
        current=${YINKOTE_CURRENT_VERSION:-}
        printf '%s\n' "$current" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.+-]+)?$' \
            || fail 'the updater did not provide a valid current version'
        if ! awk -v latest="$version" -v current="$current" 'BEGIN {
            split(latest, n, "."); split(current, parts, /[-+]/); split(parts[1], o, ".");
            for (i = 1; i <= 3; i++) {
                if (n[i] + 0 > o[i] + 0) exit 0;
                if (n[i] + 0 < o[i] + 0) exit 1;
            }
            exit current !~ /^[0-9]+\.[0-9]+\.[0-9]+-/;
        }'; then
            printf 'Yinkote %s is up to date (latest release: %s).\n' "$current" "$tag"
            exit 0
        fi
        destination=$updating
    else
        if [ -n "${YINKOTE_INSTALL_DIR:-}" ]; then
            install_dir=$YINKOTE_INSTALL_DIR
        else
            [ -n "${HOME:-}" ] || fail 'HOME is unset; set YINKOTE_INSTALL_DIR'
            install_dir=$HOME/.local/bin
        fi
        destination=$install_dir/yinkote
    fi
    case "$destination" in
        /*) ;;
        *) destination=$(pwd)/$destination ;;
    esac
    directory=${destination%/*}
    [ -n "$directory" ] || directory=/
    [ ! -d "$destination" ] || fail "destination is a directory: $destination"
    mkdir -p "$directory" || fail "cannot create $directory; choose a writable YINKOTE_INSTALL_DIR"
    staging=$(mktemp -d "$directory/.yinkote-install.XXXXXX") \
        || fail "cannot write to $directory; no files were replaced"
    cleanup() {
        rm -f "$staging/yinkote" "$staging/checksum"
        rmdir "$staging" || printf 'yinkote: could not remove temporary directory %s\n' "$staging" >&2
    }
    trap cleanup 0
    trap 'exit 130' INT
    trap 'exit 143' HUP TERM

    asset=yinkote-$target
    printf 'Downloading Yinkote %s (%s)...\n' "$tag" "$target"
    fetch --output "$staging/yinkote" "$repo/releases/download/$tag/$asset" \
        || fail "download failed for $asset"
    fetch --output "$staging/checksum" "$repo/releases/download/$tag/$asset.sha256" \
        || fail 'could not download the SHA-256 checksum'
    expected=$(awk 'NR == 1 { print $1 }' "$staging/checksum" | tr 'A-F' 'a-f')
    [ "${#expected}" -eq 64 ] || fail 'invalid SHA-256 checksum'
    case "$expected" in *[!0-9a-f]*) fail 'invalid SHA-256 checksum' ;; esac
    actual=$(checksum "$staging/yinkote") || fail 'could not calculate SHA-256'
    actual=${actual%% *}
    [ "$actual" = "$expected" ] || fail 'SHA-256 mismatch; the existing installation was not changed'
    chmod 755 "$staging/yinkote"
    reported=$("$staging/yinkote" --version) \
        || fail 'the verified binary cannot run on this system; the existing installation was not changed'
    [ "$reported" = "yinkote $version" ] \
        || fail "unexpected binary version: $reported; the existing installation was not changed"

    # Same filesystem, so readers see either the old executable or the new one.
    mv -f "$staging/yinkote" "$destination" \
        || fail "could not replace $destination; check directory permissions"
    printf 'Installed Yinkote %s to %s\n' "$version" "$destination"
    case ":${PATH:-}:" in
        *":$directory:"*) ;;
        *) printf 'Add %s to your PATH to run yinkote by name.\n' "$directory" ;;
    esac
    printf '%s\n' 'Library data was not changed. Restart any running Yinkote server to use this version.'
)

yinkote_install "$@"
