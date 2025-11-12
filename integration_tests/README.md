# Time Tracking Nvim Tests

This directory contains integration tests for the time-tracking-nvim plugin using the nvim-oxi test framework.

## Test Structure

The tests are organized in a separate cdylib crate as required by nvim-oxi. This allows the tests to interact with a real Neovim instance and test the actual plugin functionality.

## Running Tests

### Local Development

To run the tests locally:

```bash
cd tests
cargo test
```

Or use the test script:

```bash
cd tests
./run_tests.sh
```

### Continuous Integration

The tests are automatically run in GitHub Actions on every push and pull request. The integration tests (nvim-oxi tests) are run only on Ubuntu with a headless Neovim setup using Xvfb.

**Requirements for CI:**
- Neovim (stable version)
- Xvfb for headless display
- Rust toolchain

## Test Coverage

The test suite covers all utility functions and main library functions:

### Utils Functions (12 tests)
- **`is_buf_time_tracking_file`**: Tests buffer identification logic
  - Markdown files in data directory (should return true)
  - Non-markdown files in data directory (should return false)  
  - Markdown files outside data directory (should return false)
  - Empty buffer names (should return false)
  - Files in subdirectories of data directory (should return true)

- **`is_time_tracking_file`**: Tests current buffer identification

- **`is_win_time_tracking_file`**: Tests window buffer identification

- **`get_buffer_content`**: Tests buffer content retrieval
  - Populated buffers
  - Empty buffers

- **`any_tracking_visible`**: Tests visibility detection
  - Windows with time tracking files (should return true)
  - Preview windows (should be ignored)
  - Windows with non-tracking files (should return false)

### Main Library Functions (9 tests)
- **`create_or_update_preview`**: Tests preview buffer creation and management
  - Creates new buffer when none exists
  - Updates existing buffer content
  - Handles empty output correctly
  - Sets proper buffer options (not listed, not modifiable, etc.)
  - Preserves multiline content correctly
  - Handles special characters and Unicode
  - Ensures only one preview buffer exists (no duplicates)

- **`time_tracking_with_config`**: Tests command and autocommand registration
  - Creates all expected commands (Toggle, Update, AutoOpen, AutoClose, etc.)
  - Successfully registers autocommands without errors

## Test Architecture

The tests use:
- **nvim-oxi test framework**: Provides real Neovim API access
- **tempfile**: Creates temporary directories for test files
- **Manual Config creation**: Avoids environment-dependent configuration loading

Each test creates its own isolated environment with temporary directories and test files to ensure no interference between tests.