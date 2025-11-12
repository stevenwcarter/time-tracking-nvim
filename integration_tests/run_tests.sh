#!/bin/bash
set -e

echo "Running nvim-oxi integration tests..."
echo "Neovim version:"
nvim --version

echo "Starting tests..."
cd "$(dirname "$0")"
cargo test --verbose

echo "Integration tests completed successfully!"