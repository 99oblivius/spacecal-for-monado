#!/bin/bash
set -e

cd "$(dirname "$0")/flatpak"

echo "Building flatpak..."
flatpak-builder --repo=repo --force-clean build dev.oblivius.spacecal-for-monado.yml

echo "Creating bundle..."
flatpak build-bundle repo spacecal-for-monado.flatpak dev.oblivius.spacecal-for-monado \
    --runtime-repo=https://flathub.org/repo/flathub.flatpakrepo

mv spacecal-for-monado.flatpak ..
echo "Done: spacecal-for-monado.flatpak"
