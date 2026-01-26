#!/bin/bash
set -e

cd "$(dirname "$0")/flatpak"

echo "Building flatpak..."
flatpak-builder --repo=repo --force-clean build dev.oblivius.motoc-gui.yml

echo "Creating bundle..."
flatpak build-bundle repo motoc-gui.flatpak dev.oblivius.motoc-gui \
    --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo

mv motoc-gui.flatpak ..
echo "Done: motoc-gui.flatpak"
