#!/bin/bash

set -e

# Determine OS and architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

# Set the appropriate binary name
if [ "$OS" = "darwin" ]; then
    BINARY_NAME="pgm-macos"
elif [ "$OS" = "linux" ]; then
    BINARY_NAME="pgm-linux"
else
    echo "Unsupported operating system: $OS"
    exit 1
fi

# GitHub release URL
RELEASE_URL="https://github.com/tellefsen/pgm/releases/latest/download/$BINARY_NAME"

# Download the binary
echo "Downloading pgm..."
curl -sSL "$RELEASE_URL" -o pgm

# Make it executable
chmod +x pgm

# Move the binary to /usr/local/bin/
if [ -f /.dockerenv ] || [ "$EUID" -eq 0 ]; then
    # In Docker or running as root, don't use sudo
    mv pgm /usr/local/bin/
else
    # Not in Docker and not root, use sudo
    sudo mv pgm /usr/local/bin/
fi

echo "pgm has been installed successfully!"