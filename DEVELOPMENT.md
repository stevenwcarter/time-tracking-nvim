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
   require("time-tracking-nvim").setup({ auto_download = false, auto_update = false })
   ```
   With the defaults, a missing binary at the expected path triggers a
   download of the published release, so leaving `auto_download`/`auto_update`
   enabled here would silently replace your local build with upstream's and
   you would end up testing the wrong binary.

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

## Testing

### Unit Testing (Main Crate)
```bash
# Run basic unit tests and doc tests for the main crate
cargo test
```

### Integration Testing (nvim-oxi Framework)
```bash
# Run comprehensive integration tests with Neovim
./integration_tests/run_tests.sh
```

The integration tests are located in the `integration_tests/` directory and use the nvim-oxi testing framework to test plugin functionality within a real Neovim instance. These tests cover:

- Buffer detection and content extraction
- Preview window creation and management  
- Command registration and autocommand setup
- Edge cases and error handling

**Note**: Integration tests require Neovim to be installed and available in PATH.

## Formatting

`rustfmt.toml` pins `edition = "2024"` at the repo root, and every crate's
`Cargo.toml` sets the same. Keep them equal: `cargo fmt` takes the edition from
each `Cargo.toml` while a bare `rustfmt` takes it from `rustfmt.toml`, so if
they drift apart the two tools format differently and you get stray diffs that
never settle.

An optional pre-commit hook formats staged Rust files and restages them:

```bash
git config core.hooksPath scripts/hooks
```

It only touches files whose changes are *entirely* staged. A file with both
staged and unstaged changes is left alone and named in the output — rustfmt
rewrites whole files, so restaging one would sweep the unstaged half into the
commit. Bypass with `git commit --no-verify`, or `SKIP_RUSTFMT=1 git commit`.

CI checks formatting for both crates separately, since `integration_tests` is
excluded from the workspace and the root `cargo fmt` never reaches it.

## Release Process

1. Bump the version in **both** `Cargo.toml` (`version = "X.Y.Z"`) and
   `lua/time-tracking-nvim/init.lua` (`PLUGIN_VERSION = "X.Y.Z"`). CI fails if
   they disagree, and the release workflow additionally requires the git tag to
   match.
2. Update CHANGELOG.md (if exists)
3. Commit changes
4. Create and push a tag:
   ```bash
   git tag v0.1.0
   git push origin v0.1.0
   ```
5. GitHub Actions will automatically build and create a release

