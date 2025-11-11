# Development Guide

## Prerequisites

- Rust 1.70+ with cargo
- Neovim 0.11+

## Building

```bash
# Build for development
cargo build

# Build for release
cargo build --release

# Or use the build script (recommended - handles library renaming)
./build.sh
```

**Important**: Rust builds libraries with a `lib` prefix (e.g., `libtime_tracking_nvim.so`), but Neovim expects the module name exactly (e.g., `time_tracking_nvim.so`). The build script and CI automatically handle this renaming.

## Testing Locally

1. Build the plugin:
   ```bash
   ./build.sh
   ```

2. Add to your Neovim config (temporarily):
   ```lua
   vim.opt.runtimepath:append("/path/to/time-tracking-nvim")
   require("time-tracking-nvim").setup()
   ```

3. Test the commands:
   ```
   :TimeTrackingToggle
   ```

## Project Structure

```
├── src/                  # Rust source code
│   ├── lib.rs           # Main plugin logic
│   └── utils.rs         # Utility functions
├── lua/                 # Lua interface
│   └── time-tracking-nvim/
│       └── init.lua     # Plugin setup and configuration
├── plugin/              # Vim plugin compatibility
│   └── time-tracking-nvim.vim
├── .github/workflows/   # CI/CD
│   ├── ci.yml          # Continuous integration
│   └── release.yml     # Release automation
└── target/             # Build artifacts (gitignored)
```

## Release Process

1. Update version in `Cargo.toml`
2. Update CHANGELOG.md (if exists)
3. Commit changes
4. Create and push a tag:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
5. GitHub Actions will automatically build and create a release

