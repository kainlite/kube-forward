#!/bin/bash
set -e

# Define variables
GITHUB_REPO="kainlite/kube-forward"
BINARY_NAME="kube-forward"
INSTALL_DIR="/usr/local/bin"

# Function to detect OS and architecture
detect_platform() {
    local ARCH=$(uname -m)
    case $ARCH in
        x86_64)
            ARCH="amd64"
            ;;
        aarch64)
            ARCH="arm64"
            ;;
        *)
            echo "Unsupported architecture: $ARCH"
            exit 1
            ;;
    esac

    local OS=$(uname -s | tr '[:upper:]' '[:lower:]')
    case $OS in
        linux)
            ;;
        darwin)
            ;;
        *)
            echo "Unsupported operating system: $OS"
            exit 1
            ;;
    esac

    echo "${OS}_${ARCH}"
}

# Function to get the latest release version
get_latest_release() {
    curl --silent "https://api.github.com/repos/$GITHUB_REPO/releases/latest" |
    grep '"tag_name":' |
    sed -E 's/.*"([^"]+)".*/\1/'
}

# Install the binary
install_binary() {
    local PLATFORM=$1
    local VERSION=$2
    local TEMP_DIR=$(mktemp -d)
    local BINARY_URL="https://github.com/$GITHUB_REPO/releases/download/$VERSION/$BINARY_NAME-$PLATFORM"

    echo "⬇️  Downloading $BINARY_NAME $VERSION for $PLATFORM..."
    curl -sL "$BINARY_URL" -o "$TEMP_DIR/$BINARY_NAME"
    chmod +x "$TEMP_DIR/$BINARY_NAME"

    echo "📦 Installing to $INSTALL_DIR..."
    if [ -w "$INSTALL_DIR" ]; then
        mv "$TEMP_DIR/$BINARY_NAME" "$INSTALL_DIR/"
    else
        sudo mv "$TEMP_DIR/$BINARY_NAME" "$INSTALL_DIR/"
    fi

    rm -rf "$TEMP_DIR"
}

# Main installation process
main() {
    if ! command -v curl >/dev/null; then
        echo "Error: curl is required for installation"
        exit 1
    }

    local PLATFORM=$(detect_platform)
    local VERSION=$(get_latest_release)

    if [ -z "$VERSION" ]; then
        echo "Error: Unable to determine latest version"
        exit 1
    }

    install_binary "$PLATFORM" "$VERSION"
    
    if command -v $BINARY_NAME >/dev/null; then
        echo "✅ Installation successful!"
        echo "🚀 Run '$BINARY_NAME --help' to get started"
    else
        echo "❌ Installation failed"
        exit 1
    fi
}

main
