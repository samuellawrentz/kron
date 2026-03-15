#!/bin/sh
set -eu

REPO="samm/kron"
INSTALL_DIR="${KRON_INSTALL_DIR:-$HOME/.local/bin}"

main() {
    platform="$(detect_platform)"
    arch="$(detect_arch)"
    artifact="kron-${arch}-${platform}"

    version="$(latest_version)"
    if [ -z "$version" ]; then
        err "could not determine latest version"
    fi
    say "Installing kron $version ($arch-$platform)"

    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    url="https://github.com/${REPO}/releases/download/${version}/${artifact}.tar.gz"
    say "Downloading $url"
    download "$url" "$tmpdir/${artifact}.tar.gz"

    tar xzf "$tmpdir/${artifact}.tar.gz" -C "$tmpdir"

    mkdir -p "$INSTALL_DIR"
    mv "$tmpdir/kron" "$INSTALL_DIR/kron"
    chmod +x "$INSTALL_DIR/kron"

    say "Installed kron to $INSTALL_DIR/kron"

    # Create config directory
    config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/kron/jobs"
    mkdir -p "$config_dir"

    # Check PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        warn "$INSTALL_DIR is not in your PATH"
        add_to_path "$INSTALL_DIR"
    fi

    say ""
    say "Done! Run 'kron --help' to get started."
}

detect_platform() {
    case "$(uname -s)" in
        Linux*)  echo "linux" ;;
        Darwin*) echo "macos" ;;
        *)       err "unsupported platform: $(uname -s)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *)             err "unsupported architecture: $(uname -m)" ;;
    esac
}

latest_version() {
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
            | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
    elif command -v wget > /dev/null 2>&1; then
        wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
            | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
    else
        err "curl or wget is required"
    fi
}

download() {
    if command -v curl > /dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget > /dev/null 2>&1; then
        wget -qO "$2" "$1"
    else
        err "curl or wget is required"
    fi
}

add_to_path() {
    dir="$1"
    profile=""

    if [ -n "${ZSH_VERSION:-}" ] || [ -f "$HOME/.zshrc" ]; then
        profile="$HOME/.zshrc"
    elif [ -f "$HOME/.bashrc" ]; then
        profile="$HOME/.bashrc"
    elif [ -f "$HOME/.profile" ]; then
        profile="$HOME/.profile"
    fi

    if [ -n "$profile" ]; then
        line="export PATH=\"${dir}:\$PATH\""
        if ! grep -qF "$dir" "$profile" 2>/dev/null; then
            echo "" >> "$profile"
            echo "# Added by kron installer" >> "$profile"
            echo "$line" >> "$profile"
            say "Added $dir to PATH in $profile"
            say "Restart your shell or run: source $profile"
        fi
    else
        say "Add this to your shell profile:"
        say "  export PATH=\"${dir}:\$PATH\""
    fi
}

say() {
    printf "  \033[1;32mkron\033[0m: %s\n" "$*"
}

warn() {
    printf "  \033[1;33mkron\033[0m: %s\n" "$*"
}

err() {
    printf "  \033[1;31mkron\033[0m: %s\n" "$*" >&2
    exit 1
}

main
