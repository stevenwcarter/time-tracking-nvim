#!/bin/bash

# Build and Test Script for time-tracking-nvim

set -e

echo "🔨 Building time-tracking-nvim..."

# Build the project
cargo build --release

echo "✅ Build completed successfully!"

# Check if we're on a supported platform and copy the library to the expected location
OS="$(uname -s)"
case "${OS}" in
    Linux*)     
        LIB_EXT="so"
        LIB_NAME="libtime_tracking_nvim.so"
        ;;
    Darwin*)    
        LIB_EXT="dylib"
        LIB_NAME="libtime_tracking_nvim.dylib"
        ;;
    CYGWIN*|MINGW32*|MSYS*|MINGW*)
        LIB_EXT="dll"
        LIB_NAME="time_tracking_nvim.dll"
        ;;
    *)          
        echo "❌ Unsupported platform: ${OS}"
        exit 1
        ;;
esac

# Copy and rename the library to what Neovim expects.
#
# setup() loads from <plugin_root>/lua/ — add_to_cpath only ever adds
# `<plugin_root>/lua/?.<ext>`, so a build left in target/release is invisible
# to it, and auto_download (on by default) would silently fetch the *published*
# release over the top of a local build.
if [ -f "target/release/${LIB_NAME}" ]; then
    mkdir -p lua
    cp "target/release/${LIB_NAME}" "lua/time_tracking_nvim.${LIB_EXT}"
    echo "📦 Installed: lua/time_tracking_nvim.${LIB_EXT}"

    # Stamp the version so auto-update does not immediately replace this build.
    CARGO_VERSION="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
    printf '%s\n' "${CARGO_VERSION}" > "lua/time_tracking_nvim.${LIB_EXT}.version"
    echo "🏷  Stamped version: ${CARGO_VERSION}"
else
    echo "❌ Library not found: target/release/${LIB_NAME}"
    exit 1
fi

echo "🎉 Build completed! You can now test the plugin in Neovim."
echo ""
echo "To test locally, make sure this directory is in your Neovim runtimepath:"
echo "  set runtimepath+=$(pwd)"
echo ""
echo "Then in Neovim (disable downloads so your local build is not replaced):"
echo "  :lua require('time-tracking-nvim').setup({ auto_download = false, auto_update = false })"
echo "  :TimeTrackingToggle"