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

# Create target/release directory structure for plugin
mkdir -p target/release

# Copy and rename the library to what Neovim expects
if [ -f "target/release/${LIB_NAME}" ]; then
    case "${OS}" in
        Linux*)     
            cp target/release/${LIB_NAME} target/release/time_tracking_nvim.so
            echo "📦 Library renamed: target/release/time_tracking_nvim.so"
            ;;
        Darwin*)    
            cp target/release/${LIB_NAME} target/release/time_tracking_nvim.dylib
            echo "📦 Library renamed: target/release/time_tracking_nvim.dylib"
            ;;
        CYGWIN*|MINGW32*|MSYS*|MINGW*)
            cp target/release/${LIB_NAME} target/release/time_tracking_nvim.dll
            echo "📦 Library renamed: target/release/time_tracking_nvim.dll"
            ;;
    esac
else
    echo "❌ Library not found: target/release/${LIB_NAME}"
    exit 1
fi

echo "🎉 Build completed! You can now test the plugin in Neovim."
echo ""
echo "To test locally, make sure this directory is in your Neovim runtimepath:"
echo "  set runtimepath+=\$(pwd)"
echo ""
echo "Then in Neovim:"
echo "  :lua require('time-tracking-nvim').setup()"
echo "  :TimeTrackingToggle"