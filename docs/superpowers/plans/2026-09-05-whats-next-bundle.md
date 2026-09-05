# Bundled whats-next execution (B7 + 10 items) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship 10 items selected from `WHATS-NEXT.md`'s 2026-09-05 triage, plus `bughunt.md` B7 as a prerequisite bug fix that W6 rides on, as one branch.

**Architecture:** Twelve independently-testable tasks against the existing two-layer plugin (Lua bootstrap in `lua/time-tracking-nvim/`, Rust core in `src/`). Two small pieces of new shared infrastructure are built early because later tasks depend on them: a lazily-initialized Tokio runtime (`src/async_rt.rs`, needed by W5 and W11 to call `time-tracking-cli`'s async API) and a hardened `catch_nvim_panic` (B7/W6, the shared wrapper every command closure in `lib.rs` already goes through).

**Tech Stack:** Rust (nvim-oxi bindings), Lua (Neovim config/bootstrap layer), `time-tracking-cli` (git dependency), `tokio` (new direct dependency), `time-tracking-parser` (new direct dependency).

**Spec:** `docs/superpowers/specs/2026-09-05-whats-next-bundle-design.md`

## Global Constraints

- Rust edition/toolchain: match the existing `Cargo.toml` (`edition = "2024"`) — do not change it.
- `cargo fmt -- --check` and `cargo clippy -- -D warnings` must pass after every task (per `CLAUDE.md`).
- `cargo test` (unit) and `./integration_tests/run_tests.sh` (integration, requires Neovim) must pass after every Rust task.
- `integration_tests/lua/run_lua_tests.sh` must pass after every Lua task.
- Every new `:TimeTracking*` command added by a task must also be added to `test_time_tracking_with_config_creates_commands`'s expected list in `integration_tests/src/lib.rs` (alphabetically sorted, per that test's `table.sort`) in the *same* task that adds the command — do not leave that test failing for a later task to fix.
- New Cargo dependencies use the exact git sources named in the spec's "New Cargo dependencies" section — no version pins beyond what's shown there (this project tracks `time-tracking-cli`'s own `main` branch).
- Never call `time_tracking_cli::Config::get()` or `time_tracking_cli::DataService::get()` (the argv-parsing/global-config singletons) from this plugin — always `Config::try_get_no_args()` (already used) and `DataService::new_with_dir(...)` (introduced in Task 1/7).

---

## Task 1: Shared Tokio runtime (`src/async_rt.rs`)

**Files:**
- Modify: `Cargo.toml`
- Create: `src/async_rt.rs`
- Modify: `src/lib.rs:22-23` (add `mod async_rt;`)

**Interfaces:**
- Produces: `pub fn block_on<F: std::future::Future>(fut: F) -> F::Output` in `crate::async_rt`, consumed by Task 7 (W5) and Task 11 (W11).

- [ ] **Step 1: Add the `tokio` dependency**

In `Cargo.toml`, under `[dependencies]`, add:

```toml
tokio = { version = "1", features = ["rt"] }
```

(`time-tracking-cli` already pulls in `tokio` with its `full` feature set as a hard, non-optional dependency — Cargo unifies features for one crate across the whole graph, so declaring only `rt` here is enough to name `tokio::runtime::Builder`/`Runtime` ourselves; the `fs`/`time`/etc. drivers `time-tracking-cli`'s async functions need are already compiled in.)

- [ ] **Step 2: Write the failing test**

Create `src/async_rt.rs`:

```rust
//! A single, lazily-initialized Tokio runtime shared by every part of this
//! plugin that needs to call into `time-tracking-cli`'s async API
//! (`DataService`, `create_template_content`, ...).
//!
//! `time-tracking-cli` is built with `default-features = false`, but `tokio`
//! is a hard, non-optional dependency of that crate regardless of features,
//! and so are the async functions this plugin calls. A single current-thread
//! runtime, reused via `block_on`, is enough: every caller here is
//! synchronous Neovim command-handler code, invoked on purpose by the user,
//! doing only local-disk work.

use std::sync::OnceLock;
use tokio::runtime::Runtime;

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

fn runtime() -> &'static Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build the time-tracking-nvim async runtime")
    })
}

/// Run `fut` to completion on the shared runtime, blocking the calling
/// thread until it finishes.
pub fn block_on<F: std::future::Future>(fut: F) -> F::Output {
    runtime().block_on(fut)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_on_runs_an_async_block_to_completion() {
        let result = block_on(async { 2 + 2 });
        assert_eq!(result, 4);
    }

    #[test]
    fn block_on_reuses_the_same_runtime_across_calls() {
        let a = block_on(async { 1 });
        let b = block_on(async { 2 });
        assert_eq!((a, b), (1, 2));
    }
}
```

- [ ] **Step 3: Register the module**

In `src/lib.rs`, next to the existing module declarations (`mod preview;` / `pub mod utils;`), add:

```rust
mod async_rt;
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib async_rt`
Expected: both tests PASS.

- [ ] **Step 5: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/async_rt.rs src/lib.rs
git commit -m "feat: add shared Tokio runtime for calling time-tracking-cli's async API"
```

---

## Task 2: bughunt B7 — `catch_nvim_panic` never returns `Err` (also delivers W6)

**Files:**
- Modify: `src/lib.rs:69-90` (`panic_message`, `catch_nvim_panic`)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Produces (doc-hidden test seams, same convention as `preview::reset_throttle_for_test`): `pub fn catch_nvim_panic_for_test<F: FnOnce() -> Result<()>>(f: F) -> Result<()>` and `pub fn clear_last_error_for_test()` in the crate root, re-exported the same way the existing `#[doc(hidden)] pub use preview::{reset_throttle_for_test, write_preview_contents_with};` block works.
- Consumes: nothing new.

- [ ] **Step 1: Write the failing tests**

In `integration_tests/src/lib.rs`, add to the top `use` block:

```rust
use time_tracking_nvim::{catch_nvim_panic_for_test, clear_last_error_for_test};
```

Then append these tests:

```rust
#[nvim_oxi::test]
fn test_catch_nvim_panic_never_returns_err_for_a_propagated_error() {
    clear_last_error_for_test();

    let result = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "synthetic stale-handle failure".to_string(),
        )))
    });

    assert!(
        result.is_ok(),
        "catch_nvim_panic must never return Err: {:?}",
        result
    );

    let messages: String = api::eval("execute('messages')").unwrap();
    assert!(
        messages.contains("synthetic stale-handle failure"),
        "the swallowed error must still be reported via :messages, got: {messages}"
    );
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_never_returns_err_for_a_panic() {
    clear_last_error_for_test();

    let result = catch_nvim_panic_for_test(|| {
        panic!("synthetic panic for B7 coverage");
    });

    assert!(
        result.is_ok(),
        "catch_nvim_panic must never return Err, even on a caught panic: {:?}",
        result
    );

    let messages: String = api::eval("execute('messages')").unwrap();
    assert!(
        messages.contains("synthetic panic for B7 coverage"),
        "the caught panic must still be reported via :messages, got: {messages}"
    );
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_dedupes_identical_consecutive_messages() {
    clear_last_error_for_test();

    let before: String = api::eval("execute('messages')").unwrap();
    let before_count = before.matches("dedup-marker-xyz").count();

    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "dedup-marker-xyz".to_string(),
        )))
    });
    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "dedup-marker-xyz".to_string(),
        )))
    });

    let after: String = api::eval("execute('messages')").unwrap();
    let after_count = after.matches("dedup-marker-xyz").count();

    assert_eq!(
        after_count - before_count,
        1,
        "an identical consecutive failure must be reported once, not per call"
    );
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_reports_a_different_message_right_after_a_dupe() {
    clear_last_error_for_test();

    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "first-marker-abc".to_string(),
        )))
    });
    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "second-marker-def".to_string(),
        )))
    });

    let messages: String = api::eval("execute('messages')").unwrap();
    assert!(messages.contains("first-marker-abc"));
    assert!(messages.contains("second-marker-def"));
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_reports_the_same_message_again_after_a_success_in_between() {
    clear_last_error_for_test();

    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "recurring-marker-ghi".to_string(),
        )))
    });
    let _ = catch_nvim_panic_for_test(|| Ok(()));

    let before: String = api::eval("execute('messages')").unwrap();
    let before_count = before.matches("recurring-marker-ghi").count();

    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "recurring-marker-ghi".to_string(),
        )))
    });

    let after: String = api::eval("execute('messages')").unwrap();
    let after_count = after.matches("recurring-marker-ghi").count();

    assert_eq!(
        after_count - before_count,
        1,
        "a failure recurring after an intervening success must be reported again"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `./integration_tests/run_tests.sh` (or `cd integration_tests && cargo test --verbose`)
Expected: FAIL — `catch_nvim_panic_for_test`/`clear_last_error_for_test` do not exist yet.

- [ ] **Step 3: Implement**

In `src/lib.rs`, replace the existing `catch_nvim_panic` function (and add the dedup state above it):

```rust
use std::cell::RefCell;

thread_local! {
    /// The last error message `catch_nvim_panic` reported.
    ///
    /// A failure here can recur on every keystroke (bughunt B7's repro:
    /// `TextChangedI` re-invoking a command against a stale window handle),
    /// so an unconditional `err_writeln` on every call would spam
    /// `:messages` with an identical line per keystroke. This dedupes
    /// *identical consecutive* messages only — a different failure, or the
    /// same one recurring after something else succeeded in between, is
    /// always reported. Mirrors `LAST_OUTPUT`/`last_output_matches` in
    /// `preview.rs`, applied to error text instead of preview content.
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Report `msg` via `api::err_writeln`, unless it is identical to the last
/// message this reported.
fn report_error_deduped(msg: &str) {
    let already_reported = LAST_ERROR.with(|cell| cell.borrow().as_deref() == Some(msg));
    if already_reported {
        return;
    }
    api::err_writeln(msg);
    LAST_ERROR.with(|cell| *cell.borrow_mut() = Some(msg.to_owned()));
}

/// Clear the dedup latch after a successful call, so a failure that recurs
/// *after* a success in between is reported again rather than staying
/// silenced by an unrelated earlier failure.
fn clear_last_error() {
    LAST_ERROR.with(|cell| *cell.borrow_mut() = None);
}

/// Run `f`, catching both a panic and a propagated `Err`, and report either
/// through `:messages` — but never return `Err` from this function itself.
///
/// Returning `Err` from a `Function::from_fn` callback hits
/// `push_error -> lua_error`, which under `LUAJIT_UNWIND_EXTERNAL`
/// (macOS/arm64) throws a C++ exception through a `nounwind` frame and
/// aborts Neovim — the exact failure mode `time_tracking_nvim`'s own entry
/// point was already fixed to avoid. Every command in `register_commands`
/// is wrapped in this function, so this is the one place that decision has
/// to hold for all of them (this also gives `:TimeTrackingToggle`/
/// `:TimeTrackingUpdate` a diagnostic message on failure, for the first
/// time — bughunt B7 / whats-next W6).
fn catch_nvim_panic<F>(f: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => {
            clear_last_error();
            Ok(())
        }
        Ok(Err(e)) => {
            report_error_deduped(&format!("[time-tracking-nvim] {}", e));
            Ok(())
        }
        Err(payload) => {
            let msg = panic_message(payload);
            report_error_deduped(&format!("[time-tracking-nvim] panic: {}", msg));
            Ok(())
        }
    }
}

// Test seams, not interface: let the integration tests exercise the
// panic/Err-swallowing behavior directly, the same way
// `write_preview_contents_with` and `reset_throttle_for_test` are exposed.
#[doc(hidden)]
pub fn catch_nvim_panic_for_test<F>(f: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    catch_nvim_panic(f)
}

#[doc(hidden)]
pub fn clear_last_error_for_test() {
    clear_last_error();
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `./integration_tests/run_tests.sh`
Expected: PASS (all five new tests, plus every pre-existing test still green).

- [ ] **Step 5: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs integration_tests/src/lib.rs
git commit -m "fix: catch_nvim_panic never returns Err, and reports failures via :messages

Fixes bughunt B7 (Err from a Function::from_fn callback risks a hard abort
under LUAJIT_UNWIND_EXTERNAL) and delivers whats-next W6 (direct command
failures now surface a diagnostic message) via the same code path."
```

---

## Task 3: W1 — cache per-buffer tracking-file classification

**Files:**
- Modify: `src/utils.rs`
- Modify: `src/lib.rs` (new internal command + autocommand)
- Modify: `integration_tests/src/lib.rs` (new tests + update the commands-list test)

**Interfaces:**
- Produces: `pub fn invalidate_buf_classification(handle: i32)` in `crate::utils`.
- Consumes: nothing new. `is_buf_time_tracking_file`'s public signature is unchanged.

- [ ] **Step 1: Write the failing tests**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_buf_classification_cache_survives_across_repeated_calls() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let file_path = create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();

    assert!(is_buf_time_tracking_file(&buf, &config).unwrap());
    // A second call must return the same answer from the cache, not just
    // recompute correctly — this pins that the cache path is exercised at
    // all, not only that classification stays correct.
    assert!(is_buf_time_tracking_file(&buf, &config).unwrap());
}

#[nvim_oxi::test]
fn test_buf_classification_cache_invalidates_on_rename() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let other_dir = TempDir::new().unwrap();

    let outside_path = other_dir.path().join("notes.md");
    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(outside_path.to_str().unwrap()).unwrap();
    assert!(!is_buf_time_tracking_file(&buf, &config).unwrap());

    // Rename the buffer into the data directory; BufFilePost fires and must
    // invalidate the cached (false) classification.
    let inside_path = temp_dir.path().join("2024-01-01.md");
    api::set_current_buf(&buf).unwrap();
    api::command(&format!("keepalt saveas {}", inside_path.to_str().unwrap())).unwrap();

    assert!(
        is_buf_time_tracking_file(&buf, &config).unwrap(),
        "a renamed buffer must not serve a stale pre-rename classification"
    );
}

#[nvim_oxi::test]
fn test_buf_classification_cache_invalidates_on_wipeout() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let file_path = create_test_file(temp_dir.path(), "2024-01-02.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    let handle = buf.handle();
    assert!(is_buf_time_tracking_file(&buf, &config).unwrap());

    api::command(&format!("bwipeout! {}", handle)).unwrap();

    // No assertion is possible on the wiped buffer itself; this pins that
    // wiping it doesn't panic or leave the invalidation command failing.
    invalidate_buf_classification(handle);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `./integration_tests/run_tests.sh`
Expected: FAIL — `invalidate_buf_classification` does not exist; the rename test currently passes today by accident (no cache yet) but will be kept as a regression pin once the cache exists.

- [ ] **Step 3: Implement the cache in `src/utils.rs`**

Add near the top, alongside the other `use`/`thread_local!` items:

```rust
use std::collections::HashMap;

thread_local! {
    /// Per-buffer memoization of `is_buf_time_tracking_file`'s result.
    ///
    /// Keyed on the buffer handle. Invalidated by
    /// `invalidate_buf_classification`, wired to
    /// `BufFilePost`/`BufDelete`/`BufWipeout` in `lib.rs` — a buffer's
    /// classification depends only on its name and extension, both of which
    /// change only via one of those three events.
    static BUF_CLASSIFICATION: RefCell<HashMap<i32, bool>> = RefCell::new(HashMap::new());
}

/// Drop the cached classification for one buffer.
///
/// Called from the `TimeTrackingInvalidateBufCache` command, itself wired to
/// `BufFilePost`/`BufDelete`/`BufWipeout` in `lib.rs`.
pub fn invalidate_buf_classification(handle: i32) {
    BUF_CLASSIFICATION.with(|cache| {
        cache.borrow_mut().remove(&handle);
    });
}
```

(`RefCell` must be imported — `use std::cell::RefCell;` if not already present in this file's `use std::{...}` block; add it there rather than a second `use` line.)

Rename the existing `is_buf_time_tracking_file` body to `is_buf_time_tracking_file_uncached` (identical body, just the name), and replace it with:

```rust
/// Checks if the provided buffer is a time tracking file (markdown file in data directory)
pub fn is_buf_time_tracking_file(current_buffer: &Buffer, config: &Config) -> Result<bool> {
    let handle = current_buffer.handle();
    if let Some(cached) = BUF_CLASSIFICATION.with(|cache| cache.borrow().get(&handle).copied()) {
        return Ok(cached);
    }

    let result = is_buf_time_tracking_file_uncached(current_buffer, config)?;
    BUF_CLASSIFICATION.with(|cache| {
        cache.borrow_mut().insert(handle, result);
    });
    Ok(result)
}
```

- [ ] **Step 4: Register the invalidation command in `src/lib.rs`**

In `register_commands`, alongside the other `Function::from_fn` closures:

```rust
let invalidate_buf_cache = Function::from_fn(move |args: CommandArgs| {
    catch_nvim_panic(move || {
        if let Some(handle) = args.args.as_deref().and_then(|s| s.trim().parse().ok()) {
            crate::utils::invalidate_buf_classification(handle);
        }
        Ok(())
    })
});

api::create_user_command(
    "TimeTrackingInvalidateBufCache",
    invalidate_buf_cache,
    &CreateCommandOpts::builder()
        .desc("(internal) Drop the cached tracking-file classification for one buffer")
        .nargs(CommandNArgs::ZeroOrOne)
        .build(),
)?;
```

In `register_autocommands`, add one line alongside the existing `autocmd BufEnter,TabEnter * ...` line:

```rust
api::command("autocmd BufFilePost,BufDelete,BufWipeout * TimeTrackingInvalidateBufCache <abuf>")?;
```

- [ ] **Step 5: Update the commands-list test**

In `integration_tests/src/lib.rs`, `test_time_tracking_with_config_creates_commands`'s `expected` vector gains one entry, in alphabetical position (between `TimeTrackingClose` and `TimeTrackingMaybeCloseIfInvisible`):

```rust
    let expected = vec![
        "TimeTrackingAutoClose nargs=0 handler=true".to_string(),
        "TimeTrackingAutoOpen nargs=0 handler=true".to_string(),
        "TimeTrackingClose nargs=0 handler=true".to_string(),
        "TimeTrackingInvalidateBufCache nargs=? handler=true".to_string(),
        "TimeTrackingMaybeCloseIfInvisible nargs=? handler=true".to_string(),
        "TimeTrackingThrottleFire nargs=0 handler=true".to_string(),
        "TimeTrackingToggle nargs=0 handler=true".to_string(),
        "TimeTrackingUpdate nargs=0 handler=true".to_string(),
        "TimeTrackingUpdateThrottled nargs=0 handler=true".to_string(),
    ];
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 7: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 8: Commit**

```bash
git add src/utils.rs src/lib.rs integration_tests/src/lib.rs
git commit -m "perf: cache per-buffer tracking-file classification (whats-next W1)"
```

---

## Task 4: W2 — preview dismissal persists until explicitly reopened

**Files:**
- Modify: `src/preview.rs`
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: `is_preview_buf` (already `pub fn` in `utils.rs`, already imported via `use time_tracking_nvim::utils::*;` in the test file).
- Produces: no new public API — `close_preview`, `toggle_preview_fn`, `auto_open_preview` keep their existing signatures and are already re-exported.

- [ ] **Step 1: Write the failing test**

Append to `integration_tests/src/lib.rs`:

```rust
fn preview_buffer_exists() -> bool {
    api::list_bufs().any(|b| is_preview_buf(&b).unwrap_or(false))
}

#[nvim_oxi::test]
fn test_closed_preview_does_not_auto_reopen_until_explicitly_reopened() {
    reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let file_path = create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();

    auto_open_preview(config_static).unwrap();
    assert!(preview_buffer_exists(), "preview should auto-open");

    close_preview().unwrap();
    assert!(!preview_buffer_exists(), "preview should be closed");

    // Simulate the auto-open path firing again for the same tracking
    // buffer -- it must NOT reopen a dismissed preview.
    auto_open_preview(config_static).unwrap();
    assert!(
        !preview_buffer_exists(),
        "a dismissed preview must not auto-reopen"
    );

    // An explicit :TimeTrackingToggle asks for it again.
    toggle_preview_fn(config_static).unwrap();
    assert!(
        preview_buffer_exists(),
        "an explicit toggle must reopen a dismissed preview"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `./integration_tests/run_tests.sh`
Expected: FAIL — the second `auto_open_preview` call currently reopens the preview.

- [ ] **Step 3: Implement**

In `src/preview.rs`, add near the other `thread_local!` blocks:

```rust
thread_local! {
    /// Whether the user explicitly dismissed the preview (`:TimeTrackingClose`,
    /// or the close half of `:TimeTrackingToggle`) since it was last opened.
    ///
    /// `auto_open_preview_impl` respects this: only an explicit
    /// `:TimeTrackingToggle`/`:TimeTrackingUpdate` clears it, so the preview
    /// stays closed across ordinary buffer/tab switches until the user asks
    /// for it again.
    static PREVIEW_DISMISSED: Cell<bool> = const { Cell::new(false) };
}
```

Replace the two `set_cached_preview_buf(None); set_last_output(None);` pairs inside `close_preview()` with a single helper, and add that helper:

```rust
/// Clear both preview caches and mark the preview dismissed.
///
/// Called from every path in `close_preview` that actually closes or swaps
/// out the preview window.
fn clear_preview_state_on_close() {
    set_cached_preview_buf(None);
    set_last_output(None);
    PREVIEW_DISMISSED.set(true);
}
```

so `close_preview` becomes (only the two call sites change; the rest of the function is unchanged):

```rust
pub fn close_preview() -> Result<()> {
    let preview_win = match find_preview_buf()? {
        Some(buf) => preview_win_anywhere(&buf)?,
        None => None,
    };

    let Some(mut win) = preview_win else {
        clear_preview_state_on_close();
        return Ok(());
    };

    let window_count = api::list_wins().count();

    if window_count == 1 {
        match api::create_buf(true, false) {
            Ok(replacement) => {
                if let Err(e) = win.set_buf(&replacement) {
                    log_error!(
                        "[time-tracking-nvim] could not replace the preview buffer: {}",
                        e
                    );
                }
            }
            Err(e) => {
                log_error!(
                    "[time-tracking-nvim] could not create a replacement buffer: {}",
                    e
                );
            }
        }
    } else if let Err(e) = win.close(false) {
        log_error!(
            "[time-tracking-nvim] could not close the preview window: {}",
            e
        );
    }

    clear_preview_state_on_close();
    Ok(())
}
```

In `toggle_preview_fn`, clear the dismissal flag right before rendering:

```rust
    let found = find_preview()?;
    if preview_is_open_in(&found) {
        close_preview()?;
    } else {
        PREVIEW_DISMISSED.set(false);
        render_current_buffer(config, found)?;
    }
```

In `auto_open_preview_impl`, bail out first when dismissed:

```rust
fn auto_open_preview_impl(config: &'static Config) -> Result<()> {
    if PREVIEW_DISMISSED.get() {
        return Ok(());
    }

    let is_tracking = is_time_tracking_file(config)?;
    if !is_tracking {
        log_info!("[TimeTracking] Auto-open: Not a tracking file");
        return Ok(());
    }

    let found = find_preview()?;
    if !preview_is_open_in(&found) {
        render_current_buffer(config, found)?;
    }

    Ok(())
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 5: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/preview.rs integration_tests/src/lib.rs
git commit -m "feat: preview dismissal persists until explicitly reopened (whats-next W2)"
```

---

## Task 5: W3 — lightweight status query for statusline integrations

**Files:**
- Modify: `Cargo.toml` (new `time-tracking-parser` dependency)
- Modify: `src/utils.rs` (parse-and-summarize helper)
- Modify: `src/lib.rs` (expose `status` on the returned `Dictionary`)
- Modify: `lua/time-tracking-nvim/init.lua` (`M.summary()` wrapper)
- Test: unit test in `src/utils.rs`, integration test in `integration_tests/src/lib.rs`

**Interfaces:**
- Produces: `pub fn buffer_status(buffer_content: &str, config: &Config) -> Dictionary` in `crate::utils` (returns `{is_tracking_file, total_minutes, dead_time_minutes, warning_count}` — used by `lib.rs`'s new `status` `Function` and reused, unmodified, by Task 10's `data_directory_status` neighbor for the same `Dictionary`-building convention). This task establishes the pattern of adding a `Function` key to the `Dictionary` `time_tracking_with_config` returns — **Task 10 (W10) is sequenced after this one** because both tasks edit the same `let mut api = Dictionary::new(); ...; Ok(api)` block in `lib.rs`.

- [ ] **Step 1: Add the new dependency**

In `Cargo.toml`, under `[dependencies]`:

```toml
time-tracking-parser = { git = "https://github.com/stevenwcarter/time-tracking-parser" }
```

- [ ] **Step 2: Write the failing unit test**

In `src/utils.rs`, add (near the bottom, alongside any existing `#[cfg(test)] mod tests` — create one if none exists yet):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_status_parses_totals_from_content() {
        let config = Config {
            data_directory: Some("/tmp/does-not-matter-for-this-test".to_string()),
            ..Default::default()
        };
        let dict = buffer_status("9-10 work\n10-10:30 admin\n", &config);

        assert_eq!(dict.get("total_minutes"), Some(&Object::from(90i64)));
    }
}
```

(Adjust the exact `Dictionary`/`Object` comparison to whatever nvim-oxi's `Dictionary` API actually exposes for reading a value back out by key — check `Dictionary::get` in nvim-oxi's docs/source at implementation time; if `Dictionary` has no direct `get`, iterate its entries instead: `dict.iter().find(|(k, _)| k == "total_minutes")`.)

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib buffer_status`
Expected: FAIL — `buffer_status` does not exist.

- [ ] **Step 4: Implement `buffer_status` in `src/utils.rs`**

```rust
use nvim_oxi::Dictionary;

/// Parsed totals for `buffer_content`, for statusline-style integrations
/// that want a value back rather than a rendered preview.
///
/// Returns `{is_tracking_file: false}` when `config` has no usable data
/// directory context to judge tracking-file-ness against — callers that
/// already know the buffer is a tracking file (this plugin's own `status`
/// command) skip that ambiguity by checking `is_time_tracking_file` first
/// and only calling this when it's already `true`.
pub fn buffer_status(buffer_content: &str, config: &Config) -> Dictionary {
    let data = time_tracking_parser::parse_time_tracking_data(
        buffer_content,
        config.get_prefix(),
        config.get_suffix(),
    );

    Dictionary::from_iter([
        ("is_tracking_file", true.into()),
        ("total_minutes", (data.total_minutes as i64).into()),
        ("dead_time_minutes", (data.dead_time_minutes as i64).into()),
        ("warning_count", (data.warnings.len() as i64).into()),
    ])
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib buffer_status`
Expected: PASS.

- [ ] **Step 6: Expose it on the native module's returned `Dictionary`**

In `src/lib.rs`, `time_tracking_with_config` currently ends with:

```rust
    let api = Dictionary::new();
    Ok(api)
```

Replace with:

```rust
    let status = Function::from_fn(move |_: ()| -> Result<Dictionary> {
        if !crate::utils::is_time_tracking_file(config)? {
            return Ok(Dictionary::from_iter([("is_tracking_file", false.into())]));
        }
        let content = crate::utils::get_buffer_content()?;
        Ok(crate::utils::buffer_status(&content, config))
    });

    let api = Dictionary::from_iter([("status", nvim_oxi::Object::from(status))]);
    Ok(api)
```

(`Function` values need to convert into `nvim_oxi::Object` to sit inside a `Dictionary` the same way plain values do — check the exact conversion nvim-oxi expects, e.g. `Object::from(status)` or `status.into()`; the six commands registered via `api::create_user_command` already prove `Function::from_fn` values work as callables, this just needs the right `Into`/`From` path to embed one as a `Dictionary` entry instead of a command.)

- [ ] **Step 7: Add the Lua-facing wrapper**

In `lua/time-tracking-nvim/init.lua`, alongside `M.toggle()`/`M.update()`/`M.close()`:

```lua
-- Returns the current tracking buffer's parsed totals (total_minutes,
-- dead_time_minutes, warning_count), or { is_tracking_file = false } when
-- the current buffer isn't one -- for statusline (lualine, etc.)
-- integrations that want a value back, not a rendered preview.
function M.summary()
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok or not native.status then
		return { is_tracking_file = false }
	end
	return native.status()
end
```

- [ ] **Step 8: Write the integration test**

In `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_status_reports_totals_for_a_tracking_buffer() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let file_path = create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n10-10:30 admin\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();

    let status = time_tracking_nvim::utils::buffer_status(
        &time_tracking_nvim::utils::get_buffer_content().unwrap(),
        config_static,
    );

    let total_minutes = status
        .iter()
        .find(|(k, _)| k.as_str() == Ok("total_minutes"))
        .map(|(_, v)| v.clone());
    assert!(total_minutes.is_some(), "total_minutes must be present: {:?}", status);
}

#[nvim_oxi::test]
fn test_status_marks_non_tracking_buffer() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let other_dir = TempDir::new().unwrap();
    let outside_path = other_dir.path().join("notes.md");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(outside_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();

    assert!(!is_time_tracking_file(&config).unwrap());
}
```

(Adjust the `Dictionary` iteration/key-lookup calls in Step 2 and here to match nvim-oxi's actual `Dictionary`/`Object` API surface, confirmed by the compiler at implementation time — the assertions' *intent* — "totals are present and correct for a tracking buffer; a non-tracking buffer is correctly classified" — must hold regardless of the exact accessor syntax.)

- [ ] **Step 9: Run all tests**

Run: `cargo test && ./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 10: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 11: Commit**

```bash
git add Cargo.toml Cargo.lock src/utils.rs src/lib.rs lua/time-tracking-nvim/init.lua integration_tests/src/lib.rs
git commit -m "feat: lightweight status query for statusline integrations (whats-next W3)"
```

---

## Task 6: W4 — optional GitHub token for API rate limits

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua`
- Test: new `integration_tests/lua/spec_github_token.lua`

**Interfaces:**
- Produces: `M.config.github_token` (read by `fetch_release`); no other module depends on this.

- [ ] **Step 1: Write the failing Lua spec**

Look at `integration_tests/lua/spec_download.lua`'s header comment for the exact `debug.getupvalue` technique before writing this file — it documents precisely how to reach a `local` function with no test seam through a public function that closes over it.

Create `integration_tests/lua/spec_github_token.lua`:

```lua
local H = require("harness")
local tt = require("time-tracking-nvim")

H.describe("github_token", function()
	H.it("adds an Authorization header to the release-API request when configured", function()
		local recorded_cmd
		local orig_system = vim.system
		vim.system = function(cmd, opts, cb)
			recorded_cmd = cmd
			-- Never let the real fetch happen; a non-zero-arg constructor is
			-- enough to satisfy fetch_release's callback contract without a
			-- network round trip.
			cb({ code = 1, stdout = "", stderr = "no network in test" })
		end

		tt.setup({ auto_download = false, auto_update = false, github_token = "test-token-123" })
		-- M.download() reaches fetch_release through download_binary, the
		-- same path spec_download.lua's debug.getupvalue technique reaches.
		tt.download()

		vim.system = orig_system

		local found_header = false
		for i, arg in ipairs(recorded_cmd) do
			if arg == "-H" and recorded_cmd[i + 1] == "Authorization: Bearer test-token-123" then
				found_header = true
			end
		end
		H.ok(found_header, "expected an Authorization header in: " .. vim.inspect(recorded_cmd))
	end)

	H.it("never adds the header to an asset download", function()
		-- fetch_file (asset/SHA256SUMS downloads) must not receive the token
		-- even when one is configured -- only the API call should.
		local recorded_cmds = {}
		local orig_system = vim.system
		vim.system = function(cmd, opts, cb)
			table.insert(recorded_cmds, cmd)
			cb({ code = 1, stdout = "", stderr = "no network in test" })
		end

		tt.setup({ auto_download = false, auto_update = false, github_token = "test-token-123" })
		tt.download()

		vim.system = orig_system

		for _, cmd in ipairs(recorded_cmds) do
			for i, arg in ipairs(cmd) do
				H.ok(
					not (arg == "-H" and cmd[i + 1] == "Authorization: Bearer test-token-123"
						and cmd[2] ~= nil and tostring(cmd[#cmd]):match("api%.github%.com") == nil),
					"an asset-download argv must never carry the Authorization header: " .. vim.inspect(cmd)
				)
			end
		end
	end)
end)

return H
```

(Model the exact stubbing mechanics on `spec_download.lua` and `spec_setup.lua`'s established conventions in this directory rather than the sketch above verbatim — those two files already show the precise `vim.system` stub shape and callback-argument order this codebase uses; match that shape exactly.)

- [ ] **Step 2: Run the spec to verify it fails**

Run: `cd integration_tests/lua && ./run_lua_tests.sh`
Expected: FAIL — no `github_token` support yet, so no `Authorization` header is ever sent.

- [ ] **Step 3: Implement**

In `lua/time-tracking-nvim/init.lua`:

Add to `default_config`:

```lua
local default_config = {
	auto_download = true,
	auto_update = true,
	allow_unverified_download = false,
	github_token = nil, -- optional token for the GitHub API call only (not asset downloads); falls back to $GITHUB_TOKEN / $GH_TOKEN
}
```

Add a small resolver near `curl_cmd`:

```lua
-- Resolve the GitHub token to use for the release-API call: an explicit
-- setup({ github_token = ... }) wins, falling back to the environment.
local function resolve_github_token(config)
	return (config and config.github_token) or os.getenv("GITHUB_TOKEN") or os.getenv("GH_TOKEN")
end
```

Give `fetch_release` a token parameter and thread it through its one call site:

```lua
-- Fetch a release's metadata from the GitHub API.
-- cb(release_info) on success, cb(nil, reason) on failure.
local function fetch_release(release_url, token, cb)
	local extra = { "-L", "-s", "-S", release_url }
	if token then
		table.insert(extra, 1, "Authorization: Bearer " .. token)
		table.insert(extra, 1, "-H")
	end
	local cmd = curl_cmd(extra)

	vim.system(cmd, {}, function(result)
		vim.schedule(function()
			local release_info, err = decode_release(result)
			cb(release_info, err)
		end)
	end)
end
```

Update `download_binary`'s call site (the only caller of `fetch_release`):

```lua
local function download_binary(target, binary_path, callback, expected_version, opts)
	local release_url = expected_version and (API_BASE .. "/tags/v" .. expected_version) or (API_BASE .. "/latest")
	local token = resolve_github_token(opts)

	fetch_release(release_url, token, function(release_info, release_err)
```

and thread `M.config` (or the `opts` already passed for `allow_unverified`) into that call — `download_binary`'s `opts` parameter already carries `allow_unverified`; add `github_token = (M.config or {}).github_token` alongside it at both call sites (`download_then_load` and `M.download`), the same way `allow_unverified` is already passed:

```lua
	end, PLUGIN_VERSION, { allow_unverified = config.allow_unverified_download, github_token = config.github_token })
```

(in `download_then_load`), and

```lua
	end, PLUGIN_VERSION, { allow_unverified = (M.config or {}).allow_unverified_download, github_token = (M.config or {}).github_token })
```

(in `M.download`). `fetch_file` (asset/SHA256SUMS downloads) is deliberately left untouched — it must never receive the token.

- [ ] **Step 4: Run the spec to verify it passes**

Run: `cd integration_tests/lua && ./run_lua_tests.sh`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add lua/time-tracking-nvim/init.lua integration_tests/lua/spec_github_token.lua
git commit -m "feat: optional github_token config to avoid API rate limits (whats-next W4)"
```

---

## Task 7: W5 — weekly summary view (`:TimeTrackingWeeklyToggle`)

**Depends on:** Task 1 (`crate::async_rt::block_on`).

**Files:**
- Modify: `Cargo.toml` (new `time`, confirm `tokio` already present from Task 1)
- Modify: `src/preview.rs` (weekly render path, `PreviewView` state)
- Modify: `src/lib.rs` (new command)
- Modify: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: `crate::async_rt::block_on` (Task 1).
- Produces: `pub fn render_weekly_view(config: &'static Config, found: Option<(Buffer, Option<Window>)>) -> Result<()>` in `crate::preview`, and a `PreviewView` enum tracked alongside `LAST_OUTPUT` (private — no other task consumes it directly, but Task 11 must not need it).

- [ ] **Step 1: Add dependencies**

In `Cargo.toml`, confirm `tokio` is present from Task 1, and add:

```toml
time = { version = "0.3", features = ["formatting", "local-offset", "macros"] }
```

- [ ] **Step 2: Write the failing unit test**

In `src/preview.rs`, add (or extend an existing `#[cfg(test)] mod tests`):

```rust
#[cfg(test)]
mod weekly_tests {
    use super::*;
    use time_tracking_cli::{DefaultDisplayFormatter, data_svc::WeeklyProject};
    use time_tracking_parser::TimeTrackingData;

    #[test]
    fn assemble_weekly_view_omits_empty_warnings_and_projects_sections() {
        let formatter = DefaultDisplayFormatter;
        let days: Vec<(time::Date, String, Option<TimeTrackingData>)> = vec![];
        let text = assemble_weekly_view(
            "Jan 1",
            "Jan 7",
            120,
            30,
            &[] as &[String],
            &[] as &[WeeklyProject],
            &days,
            &formatter,
        );

        assert!(!text.contains("Warnings"), "empty warnings must be omitted: {text}");
        assert!(text.contains("Jan 1"));
        assert!(text.contains("Jan 7"));
    }
}
```

(Confirm the exact public names — `DefaultDisplayFormatter`, `WeeklyProject`'s module path, and whether the formatter's warnings-section text literally contains "Warnings" — against `time-tracking-cli`'s actual `DisplayFormatter`/`weekly_warnings` implementation at `~/.cargo/git/checkouts/time-tracking-cli-*/*/src/display/default.rs` before writing the assertion; the *intent* — no warnings text appears when the list is empty — is what matters.)

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib assemble_weekly_view`
Expected: FAIL — `assemble_weekly_view` does not exist.

- [ ] **Step 4: Implement the weekly render path in `src/preview.rs`**

```rust
use time::{OffsetDateTime, Weekday};
use time_tracking_cli::{DataService, data_svc::ParseSettings, get_week_dates, parse_weekday};

thread_local! {
    /// Which view is currently rendered in the preview, so the throttled
    /// TextChanged path doesn't silently replace an open weekly view with
    /// the day view on the next keystroke, and doesn't re-aggregate the
    /// whole week on typing cadence either.
    static CURRENT_VIEW: Cell<PreviewView> = const { Cell::new(PreviewView::Day) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PreviewView {
    Day,
    Week,
}

/// Build the weekly view's text from already-fetched data. Pure — the
/// network/disk work happens in `render_weekly_view`, which calls this with
/// the results.
fn assemble_weekly_view(
    week_start_label: &str,
    week_end_label: &str,
    total_minutes: u32,
    dead_time_minutes: u32,
    warnings: &[String],
    projects: &[time_tracking_cli::data_svc::WeeklyProject],
    days: &[(time::Date, String, Option<time_tracking_parser::TimeTrackingData>)],
    formatter: &dyn time_tracking_cli::DisplayFormatter,
) -> String {
    let mut out = String::new();
    out.push_str(&formatter.weekly_header(week_start_label, week_end_label));
    out.push_str(&formatter.weekly_totals(total_minutes, dead_time_minutes));
    if !warnings.is_empty() {
        out.push_str(&formatter.weekly_warnings(warnings));
    }
    if !projects.is_empty() {
        out.push_str(&formatter.weekly_projects(projects));
    }
    for (date, content, data) in days {
        out.push_str(&formatter.day_header(&date.to_string()));
        match data {
            Some(d) if d.total_minutes > 0 => {
                out.push_str(&formatter.day_summary(content, "  ", None, None));
            }
            Some(_) => out.push_str("  (no time entries)\n"),
            None => out.push_str("  (no file for this day)\n"),
        }
    }
    out
}

/// Render the current week's summary into the preview.
///
/// Builds its own hermetic `DataService` via `new_with_dir` rather than the
/// global `DataService::get()` singleton, which reads `Config::get()` (real
/// argv) internally -- this plugin never parses argv (see
/// `Config::try_get_no_args()` in `lib.rs`).
pub fn render_weekly_view(config: &'static Config, found: Option<(Buffer, Option<Window>)>) -> Result<()> {
    let today = OffsetDateTime::now_local()
        .map(|dt| dt.date())
        .unwrap_or_else(|_| OffsetDateTime::now_utc().date());

    let week_start_day: Weekday = parse_weekday(config.get_week_start_day())
        .unwrap_or(Weekday::Saturday);
    let week_dates = get_week_dates(&today, week_start_day);

    let Some(data_dir) = config.get_data_directory() else {
        return create_or_update_preview_with(found, "No data directory configured.");
    };

    let parse_settings = ParseSettings {
        prefix: config.get_prefix().map(String::from),
        suffix: config.get_suffix().map(String::from),
        template_file: config.get_template_file().map(String::from),
    };
    let data_service = DataService::new_with_dir(
        DataService::DEFAULT_CACHE_TIMEOUT_SECONDS,
        std::path::PathBuf::from(data_dir),
        parse_settings,
    );

    let summary = crate::async_rt::block_on(data_service.get_weekly_summary(&week_dates))
        .map_err(|e| nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(e.to_string())))?;

    let text = assemble_weekly_view(
        &week_dates[0].to_string(),
        &week_dates[6].to_string(),
        summary.total_minutes,
        summary.dead_time_minutes,
        &summary.warnings,
        &summary.projects,
        &summary.days,
        config.get_formatter(),
    );

    CURRENT_VIEW.set(PreviewView::Week);
    create_or_update_preview_with(found, &text)
}
```

Modify `render_current_buffer` (the day view's render path) to set `CURRENT_VIEW::Day` when it runs:

```rust
fn render_current_buffer(config: &Config, found: Option<(Buffer, Option<Window>)>) -> Result<()> {
    let buffer_content = get_buffer_content()?;
    let formatted_output = config.get_formatter().day_summary(
        &buffer_content,
        "",
        config.get_prefix(),
        config.get_suffix(),
    );
    CURRENT_VIEW.set(PreviewView::Day);
    create_or_update_preview_with(found, &formatted_output)
}
```

Modify `update_preview_throttled` to skip re-rendering while the weekly view is current (add this check right after the existing `is_time_tracking_file` early-return):

```rust
    if !is_time_tracking_file(config)? {
        return Ok(());
    }
    if CURRENT_VIEW.get() == PreviewView::Week {
        // The keystroke-driven path must not re-aggregate the whole week on
        // typing cadence; :TimeTrackingUpdate (typed explicitly) still can.
        return Ok(());
    }
```

Add a public `toggle_weekly_preview_fn`, mirroring `toggle_preview_fn`:

```rust
/// `:TimeTrackingWeeklyToggle`: closes the preview when it is showing the
/// weekly view, otherwise renders the current week into it.
pub fn toggle_weekly_preview_fn(config: &'static Config) -> Result<()> {
    let found = find_preview()?;
    if preview_is_open_in(&found) && CURRENT_VIEW.get() == PreviewView::Week {
        close_preview()?;
    } else {
        PREVIEW_DISMISSED.set(false);
        render_weekly_view(config, found)?;
    }
    Ok(())
}
```

- [ ] **Step 5: Re-export and register the command in `src/lib.rs`**

Add `toggle_weekly_preview_fn` to the existing `pub use preview::{...}` list.

In `register_commands`:

```rust
let toggle_weekly_preview = Function::from_fn(move |_: CommandArgs| {
    catch_nvim_panic(|| toggle_weekly_preview_fn(config))
});
```

and add `("TimeTrackingWeeklyToggle", "Toggle the weekly time-tracking preview", toggle_weekly_preview)` to the command-registration loop's tuple list.

- [ ] **Step 6: Update the commands-list test**

In `integration_tests/src/lib.rs`, add `"TimeTrackingWeeklyToggle nargs=0 handler=true".to_string(),` to the `expected` vector, in alphabetical order (after `TimeTrackingUpdateThrottled`).

- [ ] **Step 7: Write the integration test**

```rust
#[nvim_oxi::test]
fn test_weekly_toggle_renders_aggregate_totals() {
    reset_throttle_for_test();
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n");
    create_test_file(temp_dir.path(), "2024-01-02.md", "9-11 work\n");

    time_tracking_nvim::toggle_weekly_preview_fn(config_static).unwrap();
    assert!(preview_buffer_exists(), "weekly preview should open");
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test && ./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 9: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock src/preview.rs src/lib.rs integration_tests/src/lib.rs
git commit -m "feat: weekly summary view via :TimeTrackingWeeklyToggle (whats-next W5)"
```

---

## Task 8: W7 — `:TimeTrackingDownload` / `:TimeTrackingVersion` commands

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua`
- Modify: `lua/time-tracking-nvim/health.lua` (update the hint text)
- Test: new `integration_tests/lua/spec_commands.lua`

- [ ] **Step 1: Write the failing spec**

Create `integration_tests/lua/spec_commands.lua`:

```lua
local H = require("harness")

H.describe("TimeTracking* Lua-registered commands", function()
	H.it("registers TimeTrackingDownload and TimeTrackingVersion even when the native module fails to load", function()
		local tt = require("time-tracking-nvim")
		tt.setup({ auto_download = false, auto_update = false })

		H.eq(vim.fn.exists(":TimeTrackingDownload"), 2, "TimeTrackingDownload must be registered")
		H.eq(vim.fn.exists(":TimeTrackingVersion"), 2, "TimeTrackingVersion must be registered")
	end)

	H.it("TimeTrackingDownload calls through to M.download()", function()
		local tt = require("time-tracking-nvim")
		local called = false
		local orig = tt.download
		tt.download = function() called = true end

		vim.cmd("TimeTrackingDownload")
		H.ok(called, "TimeTrackingDownload must call M.download()")

		tt.download = orig
	end)

	H.it("TimeTrackingVersion calls through to M.version_info()", function()
		local tt = require("time-tracking-nvim")
		local called = false
		local orig = tt.version_info
		tt.version_info = function() called = true end

		vim.cmd("TimeTrackingVersion")
		H.ok(called, "TimeTrackingVersion must call M.version_info()")

		tt.version_info = orig
	end)
end)

return H
```

- [ ] **Step 2: Run the spec to verify it fails**

Run: `cd integration_tests/lua && ./run_lua_tests.sh`
Expected: FAIL — the commands don't exist yet.

- [ ] **Step 3: Implement**

In `lua/time-tracking-nvim/init.lua`, at the very top of `M.setup(opts)` (before `opts = opts or {}` even runs is fine, but simplest is right after, before the platform/binary ladder — these must be registered unconditionally, so place them before any early `return`):

```lua
function M.setup(opts)
	opts = opts or {}

	-- Registered unconditionally, before the binary-exists/load-native
	-- ladder below: these are pure-Lua troubleshooting operations that must
	-- work even when the native module never loads.
	pcall(vim.api.nvim_create_user_command, "TimeTrackingDownload", function()
		M.download()
	end, { desc = "Download or re-download the native binary" })
	pcall(vim.api.nvim_create_user_command, "TimeTrackingVersion", function()
		M.version_info()
	end, { desc = "Show plugin/binary version info" })

	local config = vim.tbl_extend("force", default_config, opts)
	...
```

(`pcall` guards against `setup()` being called twice in the same session, which would otherwise error with "command already exists" on the second call — `nvim_create_user_command` overwrites by default in modern Neovim, but guarding costs nothing and matches this file's generally defensive style elsewhere.)

Update every troubleshooting message that says `:lua require('time-tracking-nvim').download()` in this file's `notify`/`echo` calls (the `not config.auto_update` branch's message, and `M.version_info()`'s own "Version mismatch detected!" message) to say `:TimeTrackingDownload` instead. Update `health.lua`'s two `"Run :lua require('time-tracking-nvim').download()"` hint strings (in `check_binary` and `check_versions`) the same way.

- [ ] **Step 4: Run the spec to verify it passes**

Run: `cd integration_tests/lua && ./run_lua_tests.sh`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add lua/time-tracking-nvim/init.lua lua/time-tracking-nvim/health.lua integration_tests/lua/spec_commands.lua
git commit -m "feat: :TimeTrackingDownload and :TimeTrackingVersion commands (whats-next W7)"
```

---

## Task 9: W9 — preview refreshes on external file changes

**Files:**
- Modify: `src/lib.rs` (`register_autocommands`)
- Test: `integration_tests/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[nvim_oxi::test]
fn test_preview_refreshes_after_external_file_change_and_checktime() {
    reset_throttle_for_test();
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let file_path = create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();
    api::command(&format!("edit {}", file_path.to_str().unwrap())).unwrap();

    time_tracking_nvim::toggle_preview_fn(config_static).unwrap();
    assert!(preview_buffer_exists());

    // Change the file on disk, outside the buffer.
    create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n10-11 admin\n");

    api::command("checktime").unwrap();

    // The autocmd chain (FileChangedShellPost -> TimeTrackingUpdateThrottled)
    // re-renders synchronously on its leading edge (see update_preview_throttled),
    // so the preview reflects the new content without the user typing.
    assert!(preview_buffer_exists());
}
```

(This test pins the *wiring* — that `:checktime` on an externally-changed tracking buffer does not error and the preview stays open/gets a chance to refresh. If the harness makes asserting on the preview's exact rendered text straightforward — e.g. via the same buffer-content read `write_preview_contents_with`'s test seam already supports — extend the assertion to check the preview's line count grew from one entry to two; otherwise the presence check above is the minimum bar and the "no new Rust logic" claim in the spec is exactly why: correctness here rests on Neovim's own `checktime`/`FileChangedShellPost` firing correctly, which is out of this plugin's control to unit-test further.)

- [ ] **Step 2: Run test to verify it fails**

Run: `./integration_tests/run_tests.sh`
Expected: FAIL, or already passing by accident if some other autocmd happens to catch it — confirm by temporarily reverting Step 3 and re-running; if it already passes, the two new autocmd lines are still correct and worth adding since the current behavior depends on incidental autocmds, not a documented contract.

- [ ] **Step 3: Implement**

In `src/lib.rs`, `register_autocommands`, add two lines alongside the existing ones:

```rust
api::command("autocmd BufReadPost,FileChangedShellPost *.md TimeTrackingUpdateThrottled")?;
api::command("autocmd FocusGained,BufEnter *.md checktime")?;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 5: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs integration_tests/src/lib.rs
git commit -m "feat: preview refreshes on external file changes (whats-next W9)"
```

---

## Task 10: W10 — `:checkhealth` checks whether the data directory resolves

**Depends on:** Task 5 (shares the same `Dictionary`-exposure block in `lib.rs`'s `time_tracking_with_config` — sequence this task after Task 5 lands to avoid a merge conflict on that block).

**Files:**
- Modify: `src/utils.rs` (expose the existing `resolved_data_dir` logic as a `Dictionary`-returning helper)
- Modify: `src/lib.rs` (add `data_directory_status` to the returned `Dictionary`)
- Modify: `lua/time-tracking-nvim/health.lua` (new check)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: the `Dictionary`-in-`api` pattern Task 5 established (`let api = Dictionary::from_iter([("status", ...)]); Ok(api)` becomes a two-entry dictionary here).
- Produces: `pub fn data_directory_status_dict(config: &Config) -> Dictionary` in `crate::utils`.

- [ ] **Step 1: Write the failing unit test**

In `src/utils.rs`'s test module:

```rust
#[test]
fn data_directory_status_reports_unresolved_for_a_missing_directory() {
    let config = Config {
        data_directory: Some("/does/not/exist/at/all".to_string()),
        ..Default::default()
    };
    let dict = data_directory_status_dict(&config);
    let resolved = dict
        .iter()
        .find(|(k, _)| k.as_str() == Ok("resolved"))
        .map(|(_, v)| v.clone());
    assert_eq!(resolved, Some(false.into()));
}
```

(Confirm the exact `Dictionary`/`Object` read-back syntax against nvim-oxi's actual API, same caveat as Task 5's test.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib data_directory_status`
Expected: FAIL — function does not exist.

- [ ] **Step 3: Implement in `src/utils.rs`**

```rust
/// The configured data directory's resolution status, as a `Dictionary` for
/// exposure to Lua (`:checkhealth`'s data-directory check).
///
/// Reuses `resolved_data_dir` rather than re-resolving independently, so
/// this can never disagree with what `is_buf_time_tracking_file` actually
/// uses to classify buffers.
pub fn data_directory_status_dict(config: &Config) -> Dictionary {
    let configured = config.get_data_directory().unwrap_or("<unset>").to_string();

    match resolved_data_dir(config) {
        Some(path) => Dictionary::from_iter([
            ("configured", configured.into()),
            ("resolved", true.into()),
            ("canonical_path", path.to_string_lossy().into_owned().into()),
        ]),
        None => Dictionary::from_iter([
            ("configured", configured.into()),
            ("resolved", false.into()),
        ]),
    }
}
```

(`resolved_data_dir` already exists in this file as a private `fn`; no change to it is needed beyond calling it here.)

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib data_directory_status`
Expected: PASS.

- [ ] **Step 5: Expose it in `src/lib.rs`**

Extend the `Dictionary` built in Task 5's `time_tracking_with_config` to add a second entry:

```rust
    let data_directory_status = Function::from_fn(move |_: ()| -> Result<Dictionary> {
        Ok(crate::utils::data_directory_status_dict(config))
    });

    let api = Dictionary::from_iter([
        ("status", nvim_oxi::Object::from(status)),
        ("data_directory_status", nvim_oxi::Object::from(data_directory_status)),
    ]);
    Ok(api)
```

- [ ] **Step 6: Add the health check**

In `lua/time-tracking-nvim/health.lua`, add:

```lua
-- Data directory. Needs the native module loaded to call this, so it runs
-- after check_native_module and reports nothing if that failed.
local function check_data_directory()
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok or type(native) ~= "table" or not native.data_directory_status then
		return
	end

	local status = native.data_directory_status()
	if status.resolved then
		health.ok("Data directory resolves: " .. tostring(status.canonical_path))
	else
		health.error("Data directory does not resolve: " .. tostring(status.configured), {
			"The preview will not open for any file until this is fixed",
		})
	end
end
```

and call it from `M.check()`, right after `check_native_module(internal)` and before `check_commands()`:

```lua
	check_native_module(internal)
	check_data_directory()
	check_commands()
```

- [ ] **Step 7: Write the integration test**

```rust
#[nvim_oxi::test]
fn test_data_directory_status_resolves_a_real_directory() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let dict = time_tracking_nvim::utils::data_directory_status_dict(&config);
    let resolved = dict
        .iter()
        .find(|(k, _)| k.as_str() == Ok("resolved"))
        .map(|(_, v)| v.clone());
    assert_eq!(resolved, Some(true.into()));
    let _ = temp_dir; // keep the TempDir alive for the duration of the assertion
}
```

- [ ] **Step 8: Run all tests**

Run: `cargo test && ./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 9: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 10: Commit**

```bash
git add src/utils.rs src/lib.rs lua/time-tracking-nvim/health.lua integration_tests/src/lib.rs
git commit -m "feat: :checkhealth reports whether the data directory resolves (whats-next W10)"
```

---

## Task 11: W11 — `:TimeTrackingOpenToday` command

**Depends on:** Task 1 (`crate::async_rt::block_on`).

**Files:**
- Modify: `src/lib.rs` (new command)
- Test: `integration_tests/src/lib.rs`

- [ ] **Step 1: Write the failing test**

```rust
#[nvim_oxi::test]
fn test_open_today_creates_file_from_template_and_opens_it() {
    let (mut config, temp_dir) = create_test_config_with_temp_dir();
    let template_path = create_test_file(temp_dir.path(), "template.md", "# {date}\n\n");
    config.template_file = Some(template_path.to_str().unwrap().to_string());
    config.date = time::OffsetDateTime::now_utc().date();
    let config_static: &'static Config = Box::leak(Box::new(config));

    time_tracking_nvim::open_today_fn(config_static).unwrap();

    let today_str = config_static.date.to_string();
    let expected_path = temp_dir.path().join(format!("{today_str}.md"));
    assert!(expected_path.exists(), "today's file should have been created");

    let content = std::fs::read_to_string(&expected_path).unwrap();
    assert!(content.contains(&today_str), "the {{date}} placeholder should be replaced: {content}");

    // Running it again must not overwrite existing content.
    std::fs::write(&expected_path, "9-10 work\n").unwrap();
    time_tracking_nvim::open_today_fn(config_static).unwrap();
    let content_after = std::fs::read_to_string(&expected_path).unwrap();
    assert_eq!(content_after, "9-10 work\n", "an existing file must not be re-seeded");
}
```

(Confirm `time_tracking_cli::DATE_FORMAT`'s exact string form matches `config_static.date.to_string()`'s default `Display` output before relying on that equality — if they differ, format `today` through `DATE_FORMAT` explicitly on both sides of the comparison instead of `Date`'s `Display` impl.)

- [ ] **Step 2: Run test to verify it fails**

Run: `./integration_tests/run_tests.sh`
Expected: FAIL — `open_today_fn` does not exist.

- [ ] **Step 3: Implement in `src/lib.rs`**

```rust
/// `:TimeTrackingOpenToday`: opens today's tracking file, creating it from
/// the configured template if it doesn't exist yet.
pub fn open_today_fn(config: &'static Config) -> Result<()> {
    let Some(data_dir) = config.get_data_directory() else {
        log_error!("[time-tracking-nvim] no data directory configured");
        return Ok(());
    };

    let today = time::OffsetDateTime::now_local()
        .map(|dt| dt.date())
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc().date());
    let date_str = today
        .format(&time_tracking_cli::DATE_FORMAT)
        .unwrap_or_else(|_| today.to_string());

    let dir = std::path::Path::new(data_dir);
    let file_path = dir.join(format!("{date_str}.md"));

    if !file_path.exists() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            log_error!("[time-tracking-nvim] could not create data directory: {}", e);
            return Ok(());
        }
        let content = crate::async_rt::block_on(time_tracking_cli::create_template_content(
            &today,
            config.get_template_file(),
        ))
        .unwrap_or_default();
        if let Err(e) = std::fs::write(&file_path, content) {
            log_error!("[time-tracking-nvim] could not create today's file: {}", e);
            return Ok(());
        }
    }

    let escaped: String = api::call_function("fnameescape", (file_path.to_string_lossy(),))
        .unwrap_or_else(|_| file_path.to_string_lossy().into_owned());
    api::command(&format!("edit {escaped}"))?;
    Ok(())
}
```

Add to `register_commands`:

```rust
let open_today = Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| open_today_fn(config)));
```

and add `("TimeTrackingOpenToday", "Open (creating if needed) today's tracking file", open_today)` to the command-registration loop's tuple list.

- [ ] **Step 4: Update the commands-list test**

Add `"TimeTrackingOpenToday nargs=0 handler=true".to_string(),` to the `expected` vector in `integration_tests/src/lib.rs`, alphabetically (after `TimeTrackingMaybeCloseIfInvisible`, before `TimeTrackingThrottleFire`).

- [ ] **Step 5: Run test to verify it passes**

Run: `./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 6: Lint and format**

Run: `cargo fmt -- --check && cargo clippy -- -D warnings`
Expected: clean.

- [ ] **Step 7: Commit**

```bash
git add src/lib.rs integration_tests/src/lib.rs
git commit -m "feat: :TimeTrackingOpenToday command (whats-next W11)"
```

---

## Task 12: Documentation updates

**Files:**
- Modify: `README.md`
- Modify: `CLAUDE.md`

**Interfaces:** none — this task changes no code.

- [ ] **Step 1: Update `README.md`'s Commands list**

In the Commands section (currently three bullets), add the four new commands:

```markdown
- `:TimeTrackingToggle` - Toggle the preview window on/off
- `:TimeTrackingUpdate` - Manually update the preview content
- `:TimeTrackingClose` - Close the preview window
- `:TimeTrackingWeeklyToggle` - Toggle a weekly summary view in the preview
- `:TimeTrackingOpenToday` - Open (creating from your template if needed) today's tracking file
- `:TimeTrackingDownload` - Download or re-download the native binary
- `:TimeTrackingVersion` - Show plugin/binary version info
```

- [ ] **Step 2: Document `github_token` in Setup options**

```markdown
require("time-tracking-nvim").setup({
  auto_download = true,
  auto_update = true,
  allow_unverified_download = false,
  github_token = nil,                -- optional GitHub token for the release-API call, to avoid rate limits
})
```

with a new bullet:

```markdown
- `github_token` (default `nil`) — sent as an `Authorization` header on the
  GitHub **API** call only (never on the asset download itself). Falls back
  to `$GITHUB_TOKEN`/`$GH_TOKEN` when unset. Useful behind a shared IP that
  hits GitHub's unauthenticated 60 requests/hour limit.
```

- [ ] **Step 3: Update the Version Information troubleshooting section**

Replace:

```markdown
### Version Information

\`\`\`vim
:lua require('time-tracking-nvim').version_info()
\`\`\`
```

with:

```markdown
### Version Information

\`\`\`vim
:TimeTrackingVersion
\`\`\`
```

- [ ] **Step 4: Add a statusline-integration mention**

Under Usage, add a short new subsection:

```markdown
### Statusline Integration

\`require('time-tracking-nvim').summary()\` returns the current tracking
buffer's parsed totals (\`total_minutes\`, \`dead_time_minutes\`,
\`warning_count\`), or \`{ is_tracking_file = false }\` otherwise — for
lualine/statusline components that want a value back rather than a
rendered preview.
```

- [ ] **Step 5: Update `CLAUDE.md`'s Architecture section**

In the `lib.rs` bullet's command list, add the four new commands to the enumeration, and add one sentence noting the returned `Dictionary` now exposes `status`/`data_directory_status` functions alongside the registered commands (today it is always empty). Add a one-line mention of `src/async_rt.rs` alongside the existing `preview.rs`/`utils.rs` bullets: "`async_rt.rs` — a single lazily-initialized Tokio runtime shared by everything that calls into `time-tracking-cli`'s async API (the weekly view, opening today's file)."

- [ ] **Step 6: Commit**

```bash
git add README.md CLAUDE.md
git commit -m "docs: document the new commands, github_token option, and summary() API"
```

---

## Self-Review

**Spec coverage:**
- B7/W6 → Task 2. W1 → Task 3. W2 → Task 4. W3 → Task 5. W4 → Task 6. W5 → Task 7. W7 → Task 8. W9 → Task 9. W10 → Task 10. W11 → Task 11. Documentation updates → Task 12. Async runtime cross-cutting piece → Task 1. All eleven spec items and the cross-cutting section are covered.

**Placeholder scan:** every step carries real, concrete code — no "TBD"/"add error handling"/"similar to Task N" placeholders. Three spots flag a compiler-verify-at-implementation-time detail rather than leaving a placeholder (`Dictionary`/`Object` read-back syntax in Tasks 5/10's tests, `Buffer::handle()`'s exact usage already confirmed against nvim-oxi's source, `DisplayFormatter`'s exact warnings-text in Task 7) — these are precise, falsifiable claims to confirm against the compiler/dependency source, not vague direction.

**Type/name consistency check:**
- `catch_nvim_panic_for_test` / `clear_last_error_for_test` (Task 2) — used consistently in Task 2's tests only; no other task calls them.
- `invalidate_buf_classification(handle: i32)` (Task 3) — matches the `<abuf>` autocmd's string-to-i32 parse in `lib.rs`.
- `PREVIEW_DISMISSED` (Task 4) is `preview.rs`-local; Task 7's `toggle_weekly_preview_fn` also references it (`PREVIEW_DISMISSED.set(false)`) — Task 7 is sequenced after Task 4 lands, so this name exists by the time Task 7 needs it. **Ordering note:** execute Task 4 before Task 7.
- `buffer_status` (Task 5) and `data_directory_status_dict` (Task 10) both build a `Dictionary` and both get wired into the same `let api = Dictionary::from_iter([...])` block in `lib.rs` — Task 10's step 5 shows the two-entry version, consuming Task 5's one-entry version. **Ordering note:** execute Task 5 before Task 10 (already called out in Task 10's header).
- `crate::async_rt::block_on` (Task 1) is consumed by Task 7 and Task 11 with identical usage (`crate::async_rt::block_on(fut)`). **Ordering note:** execute Task 1 before Task 7 and Task 11.
- `CURRENT_VIEW`/`PreviewView` (Task 7) is `preview.rs`-local and not referenced by any other task.

**Task ordering summary:** 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 → 12 satisfies every dependency above. Tasks 3, 6, 8, 9 have no dependencies beyond Task 2 landing first (all wrap command handlers in `catch_nvim_panic`, so building on the hardened version avoids re-touching `lib.rs`'s command-registration block twice) and could run in parallel with each other once Task 2 is done; Tasks 5→10 and 1→7/11 must stay sequential for the reasons above.
