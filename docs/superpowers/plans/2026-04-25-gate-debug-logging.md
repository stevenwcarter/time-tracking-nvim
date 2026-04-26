# Gate Debug Logging Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Gate the 6 diagnostic `stderr` messages in `time_tracking_nvim()` behind a `TIME_TRACKING_DEBUG` env var so they are silent by default, while keeping the 3 genuine error messages unconditional.

**Architecture:** Add a single `debug_log!` macro to `src/lib.rs` alongside the existing macros. Replace each diagnostic `stderr.write_all` block with a `debug_log!` call. Genuine error-path blocks are left untouched. No other files change.

**Tech Stack:** Rust, nvim-oxi, `std::env::var`, `std::io::stderr`

---

### Task 1: Add `debug_log!` macro

**Files:**
- Modify: `src/lib.rs:38` (insert after the `log_error!` macro)

- [ ] **Step 1: Add the macro after `log_error!` in `src/lib.rs`**

Replace the block ending at line 38:

```rust
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        nvim_oxi::api::err_writeln(&format!($($arg)*));
    };
}
```

with:

```rust
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        nvim_oxi::api::err_writeln(&format!($($arg)*));
    };
}

#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("TIME_TRACKING_DEBUG").is_ok() {
            use std::io::Write;
            let _ = std::io::stderr().write_all(format!($($arg)*).as_bytes());
        }
    };
}
```

- [ ] **Step 2: Verify macro compiles**

```bash
cargo check 2>&1
```

Expected: no errors.

---

### Task 2: Convert diagnostic blocks to `debug_log!`

**Files:**
- Modify: `src/lib.rs:65-132` (the `time_tracking_nvim` function body)

- [ ] **Step 1: Replace the entire `time_tracking_nvim` function body**

Replace the current function (lines 63–132) with:

```rust
/// Plugin to provide time tracking previews while editing in Neovim.
#[nvim_oxi::plugin]
fn time_tracking_nvim() -> Result<Dictionary> {
    debug_log!("[ttnvim] entered time_tracking_nvim\n");

    // Install diagnostic hook to capture the real panic source.
    panic::set_hook(Box::new(|info| {
        let msg = format!("[ttnvim] PANIC: {info}\n");
        use std::io::Write;
        let _ = std::io::stderr().write_all(msg.as_bytes());
    }));

    debug_log!("[ttkvim] hook installed, starting catch_unwind\n");

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        debug_log!("[ttkvim] inside catch_unwind closure\n");
        let config = Config::try_get_no_args()
            .map_err(|e| nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(e.to_string())))?;
        debug_log!("[ttkvim] config loaded, calling time_tracking_with_config\n");
        let r = time_tracking_with_config(config);
        match &r {
            Ok(_) => {
                debug_log!("[ttkvim] time_tracking_with_config succeeded\n");
            }
            Err(e) => {
                use std::io::Write;
                let _ = std::io::stderr().write_all(
                    format!("[ttkvim] time_tracking_with_config FAILED: {e}\n").as_bytes(),
                );
            }
        }
        r
    }));

    debug_log!("[ttkvim] catch_unwind returned\n");

    let _ = panic::take_hook();

    // Never return Err: push_error → lua_error throws a C++ exception on macOS
    // (LUAJIT_UNWIND_EXTERNAL) which hits the nounwind terminate block → panic_cannot_unwind.
    match result {
        Ok(Ok(dict)) => Ok(dict),
        Ok(Err(e)) => {
            use std::io::Write;
            let _ = std::io::stderr().write_all(format!("[ttnvim] error: {e}\n").as_bytes());
            Ok(Dictionary::new())
        }
        Err(payload) => {
            let msg = panic_message(payload);
            use std::io::Write;
            let _ =
                std::io::stderr().write_all(format!("[ttnvim] panic caught: {msg}\n").as_bytes());
            Ok(Dictionary::new())
        }
    }
}
```

- [ ] **Step 2: Build and lint**

```bash
cargo build 2>&1
```

Expected: compiles without errors or warnings.

```bash
cargo clippy -- -D warnings 2>&1
```

Expected: no warnings.

- [ ] **Step 3: Run unit tests**

```bash
cargo test 2>&1
```

Expected: all tests pass.

- [ ] **Step 4: Smoke test — silent by default**

Build the plugin and launch Neovim normally (opening a non-tracking file). Confirm no diagnostic lines appear in the terminal.

```bash
./build.sh && nvim somefile.txt
```

Expected: no `[ttnvim]` or `[ttkvim]` lines in terminal output.

- [ ] **Step 5: Smoke test — verbose with env var**

```bash
TIME_TRACKING_DEBUG=1 nvim somefile.txt
```

Expected: `[ttnvim] entered time_tracking_nvim`, `[ttkvim] hook installed, starting catch_unwind`, etc. appear in the terminal.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs
git commit -m "fix: gate diagnostic stderr logging behind TIME_TRACKING_DEBUG env var"
```
