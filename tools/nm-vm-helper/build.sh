#!/bin/bash
# Build and sign nm-vm-helper
# Requires: Xcode Command Line Tools, macOS 13+

set -e

echo "Building nm-vm-helper..."
swiftc -O \
  -target arm64-apple-macos13 \
  -framework Virtualization \
  main.swift \
  -o nm-vm-helper

echo "Signing with Virtualization entitlements..."
codesign \
  --entitlements entitlements.plist \
  --sign - \
  --force \
  nm-vm-helper

echo "Installing to /usr/local/bin/..."
sudo install -m 755 nm-vm-helper /usr/local/bin/nm-vm-helper

echo "Done. nm-vm-helper installed."
