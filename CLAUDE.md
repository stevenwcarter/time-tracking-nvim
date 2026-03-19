# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

A Neovim plugin written in Rust using [nvim-oxi](https://github.com/noib3/nvim-oxi) bindings. It provides live preview updates of time tracking data while editing markdown files, integrating with the [time-tracking-cli](https://github.com/stevenwcarter/time-tracking-cli) utility.

## Commands

```bash
# Build (handles library renaming from libtime_tracking_nvim.* to time_tracking_nvim.*)
cargo build
./build.sh          # recommended for local dev — also renames the output artifact

# Release build
cargo build --release

# Lint and format
cargo fmt -- --check
cargo clippy -- -D warnings

# Unit tests
cargo test

# Integration tests (requires Neovim installed)
./integration_tests/run_tests.sh
# or: cd integration_tests && cargo test --verbose
```

## Architecture

The plugin has two layers:

**Lua layer** (`lua/time-tracking-nvim/init.lua`): Entry point for `require("time-tracking-nvim").setup()`. Handles platform detection, binary download/auto-update from GitHub releases, semantic version comparison, and adds the binary directory to Lua's cpath for module loading.

**Rust core** (`src/`):
- `lib.rs` — Plugin entry point (`#[nvim_oxi::plugin]`). Registers 5 user commands (`TimeTrackingToggle`, `TimeTrackingUpdate`, `TimeTrackingAutoOpen`, `TimeTrackingAutoClose`, `TimeTrackingClose`) and sets up autocommands for auto-open on VimEnter/BufWinEnter, live updates on TextChanged/TextChangedI, auto-close on QuitPre, and window layout preservation.
- `preview.rs` — Preview window/buffer lifecycle: create, update, toggle, close. Creates scratch buffers (unlisted, non-modifiable, no swap) in a vertical split at ~1/3 screen width. Toggles `modifiable` when writing content.
- `utils.rs` — File/buffer/window detection helpers to determine if the current buffer is a time tracking file.

**Data flow**: VimEnter/BufWinEnter triggers auto-open → plugin calls time-tracking-cli on the buffer content → preview window shows formatted output → TextChanged events update the preview in real-time → QuitPre closes the preview.

## Key Implementation Details

- **Library naming**: Rust builds as `libtime_tracking_nvim.{so,dylib,dll}` but Neovim requires `time_tracking_nvim.{so,dylib,dll}`. The `build.sh` script and CI workflows handle this rename.
- **macOS dynamic linking**: `.cargo/config.toml` sets platform-specific `rustflags` for both Intel and Apple Silicon targets.
- **Integration tests**: Use the nvim-oxi test framework running a real Neovim instance. CI runs them under Xvfb on Ubuntu only.
- **Releases**: CI/CD in `.github/workflows/release.yml` builds for 4 targets (Linux x86_64, macOS x86_64/aarch64, Windows x86_64) on git tags matching `v*`.
