# Throttled Preview Updates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the 150ms trailing-edge debounce on preview updates with a 200ms leading-edge throttle, so the preview keeps up with continuous typing instead of only refreshing during pauses.

**Architecture:** The throttle keeps two thread-locals — when the last render happened, and whether one is already booked for the current window. The first change in a burst renders synchronously; a change inside an open window books a render on the window *boundary* (not `THROTTLE` from now) and further changes in that window are dropped. The trailing render is booked with Neovim's own `timer_start()` rather than nvim-oxi's `libuv::TimerHandle`, which is what lets the whole `#[cfg(windows)]` fork, the `libuv` Cargo feature, the per-arm memory leak, and the fast-event-context `schedule()` hop all disappear.

**Tech Stack:** Rust (edition 2024), nvim-oxi (`neovim-0-12`), Neovim vimscript `timer_start()`, `#[nvim_oxi::test]` integration harness.

**Spec:** `docs/superpowers/specs/2026-09-04-throttled-preview-updates-design.md`

## Global Constraints

- Edition 2024 for all crates; `rustfmt.toml` edition must stay equal to every `Cargo.toml` edition.
- `cargo clippy --all-targets -- -D warnings` must pass. `cargo fmt --all -- --check` must pass.
- Throttle interval is exactly `Duration::from_millis(200)`, a hardcoded `const`. It is **not** configurable through `setup()` — that was asked and declined.
- Exact new names, used verbatim everywhere: function `update_preview_throttled`, function `throttle_fire`, const `THROTTLE`, thread-locals `LAST_RENDER` and `THROTTLE_PENDING`, commands `:TimeTrackingUpdateThrottled` and `:TimeTrackingThrottleFire`.
- `:TimeTrackingUpdate` → `update_preview_fn` stays fully synchronous and unthrottled. Do not route it through the throttle.
- Do not reintroduce `nvim_oxi::libuv` anywhere. If the vimscript lambda gives trouble, the only sanctioned fallback is `vim.defer_fn` via `api::exec2`.
- Behaviour must be identical on Linux, macOS and Windows. No new `#[cfg]` guards.

---

### Task 1: Replace the debounce with the throttle

This task is atomic by necessity: renaming `update_preview_debounced` and dropping `TimerHandle` breaks compilation of `src/`, `Cargo.toml` and the integration test crate simultaneously, so they land together.

**Files:**
- Modify: `src/preview.rs:1-9` (imports), `src/preview.rs:131-246` (const, thread-local, both `update_preview_debounced` bodies)
- Modify: `src/lib.rs:26-33` (exports), `src/lib.rs:185-192` (command closures), `src/lib.rs:236-245` (command table), `src/lib.rs:285` (autocommand)
- Modify: `Cargo.toml:26-46` (dependency blocks)
- Test: `integration_tests/src/lib.rs` (new cadence test; rename call sites; fix the two debounce-semantics assertions and the command-registry list)

**Interfaces:**
- Produces, for Tasks 2 and 3:
  - `pub fn update_preview_throttled(config: &'static Config) -> nvim_oxi::Result<()>`
  - `pub fn throttle_fire(config: &'static Config) -> nvim_oxi::Result<()>`
  - `#[doc(hidden)] pub fn reset_throttle_for_test()`
  - Commands `:TimeTrackingUpdateThrottled` and `:TimeTrackingThrottleFire`, both `nargs=0`.
- Consumes: nothing.

- [ ] **Step 1: Write the failing cadence test**

This is the test that actually distinguishes a throttle from a debounce: under continuous typing a debounce renders *nothing*. Write it against the **current** name `update_preview_debounced` so it compiles and runs red right now; Step 7 renames the call.

It counts renders without depending on what the formatter emits: re-prime the preview with a sentinel each iteration and count how many times a render clears it.

Append to `integration_tests/src/lib.rs`, after `test_debounced_update_renders_nothing_for_a_non_tracking_file`:

```rust
#[nvim_oxi::test]
fn test_throttle_renders_repeatedly_during_sustained_typing() {
    use std::time::{Duration, Instant};

    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // Register the commands: the throttle books its trailing renders through
    // `:TimeTrackingThrottleFire`. Done after the preview exists so no
    // BufEnter handler runs during setup.
    time_tracking_with_config(config_static).unwrap();

    // Type continuously for ~600ms — three throttle windows — turning the
    // event loop between keystrokes so booked renders get a chance to fire.
    // Re-priming the sentinel each iteration makes each render countable
    // without depending on what the formatter emits.
    let mut renders = 0;
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(600) {
        create_or_update_preview("PLACEHOLDER").unwrap();
        time_tracking_nvim::update_preview_debounced(config_static).unwrap();
        api::exec2(
            "lua vim.wait(20, function() return false end)",
            &Default::default(),
        )
        .unwrap();
        if !preview_text(&preview).contains("PLACEHOLDER") {
            renders += 1;
        }
    }

    assert!(
        renders >= 2,
        "600ms of continuous typing must produce at least two renders \
         (roughly one per 200ms window); got {renders}. A trailing-edge \
         debounce produces none, because every keystroke pushes its deadline \
         out again."
    );
}
```

- [ ] **Step 2: Run it and confirm it fails**

```bash
cd integration_tests && cargo test test_throttle_renders_repeatedly_during_sustained_typing -- --nocapture
```

Expected: FAIL — `600ms of continuous typing must produce at least two renders ...; got 0`.

If it reports a number ≥ 2, stop: the premise is wrong and the rest of the plan needs rethinking. Report that rather than proceeding.

- [ ] **Step 3: Rewrite the throttle state in `src/preview.rs`**

Replace the import block at `src/preview.rs:1-9`:

```rust
use super::*;

use crate::utils::{PREVIEW_BUF_NAME, is_preview_buf};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
```

(`RefCell` is still used by `PREVIEW_BUF` and `LAST_OUTPUT`. The `#[cfg(not(windows))]` attributes and the `nvim_oxi::libuv::TimerHandle` import are gone.)

Replace the `DEBOUNCE` const at `src/preview.rs:131-133`:

```rust
/// Minimum interval between autocommand-driven renders.
///
/// A *throttle*, not a debounce: the first change in a burst renders at once,
/// and the rest render on this cadence, so the preview keeps up with
/// continuous typing instead of waiting for the user to stop.
const THROTTLE: Duration = Duration::from_millis(200);
```

Replace the whole `#[cfg(not(windows))] thread_local! { ... PENDING_UPDATE ... }` block at `src/preview.rs:145-172`:

```rust
thread_local! {
    /// When the last throttle-path render happened.
    ///
    /// `None` until the first one, which is what lets the first change of a
    /// session render immediately.
    static LAST_RENDER: Cell<Option<Instant>> = const { Cell::new(None) };

    /// Whether a render is already booked for the current throttle window.
    ///
    /// This flag is the entire difference between this and the debounce it
    /// replaced. The debounce cancelled and re-armed its timer on every
    /// keystroke, pushing the render out for as long as the user kept typing.
    /// Here a booked render stays booked: later changes in the same window see
    /// this set and return, and the booked render fires on the window
    /// boundary.
    ///
    /// Cleared by [`throttle_fire`], which the timer reaches through
    /// `:TimeTrackingThrottleFire`. There is no cancellation path — a booked
    /// timer always fires exactly once and always clears this — so the only
    /// way it can stick is an arming failure, which
    /// [`update_preview_throttled`] rolls back explicitly.
    static THROTTLE_PENDING: Cell<bool> = const { Cell::new(false) };
}
```

- [ ] **Step 4: Replace both `update_preview_debounced` bodies with the throttle**

Delete everything from the `/// Autocommand entry point:` doc comment at `src/preview.rs:174` through the end of the `#[cfg(windows)]` variant at `src/preview.rs:246` — both the `#[cfg(not(windows))]` and `#[cfg(windows)]` functions and their doc comments. Replace with:

```rust
/// Autocommand entry point: hold autocommand-driven renders to at most one per
/// [`THROTTLE`].
///
/// `TextChanged`/`TextChangedI` fire once per keystroke on Neovim's single UI
/// thread, and each render pays canonicalize syscalls, a window scan, a
/// full-buffer read and a re-parse — too much to do per keystroke. Rendering
/// only once the user stops, which is what the debounce this replaced did,
/// costs the opposite thing: the preview sits frozen for as long as they keep
/// typing. A leading-edge throttle does neither. The first change renders at
/// once and the rest land on a steady cadence, so the summary visibly
/// accumulates while the notes are being written.
///
/// `:TimeTrackingUpdate` deliberately still calls [`update_preview_fn`]
/// directly: a user who types the command expects to see the result now, not
/// at the next window boundary.
pub fn update_preview_throttled(config: &'static Config) -> Result<()> {
    // Render nothing for a buffer that can never show a preview. The
    // autocommand fires for every `*.md` buffer, not just tracking notes, so
    // without this every README keystroke would pay for a window scan and a
    // timer. `update_preview_fn` re-checks this when the timer fires, against
    // whatever buffer is current by then.
    if !is_time_tracking_file(config)? {
        return Ok(());
    }

    // A render is already booked for this window. Leave its deadline alone —
    // moving it is exactly what would turn this back into a debounce.
    if THROTTLE_PENDING.get() {
        return Ok(());
    }

    let remaining = LAST_RENDER.get().and_then(|last| {
        let elapsed = last.elapsed();
        (elapsed < THROTTLE).then(|| THROTTLE - elapsed)
    });

    let Some(remaining) = remaining else {
        // Leading edge: no window is open, so render now, synchronously.
        LAST_RENDER.set(Some(Instant::now()));
        return update_preview_fn(config);
    };

    // Inside an open window: book the render for the window *boundary* rather
    // than for `THROTTLE` from now, so the cadence stays even under continuous
    // typing instead of drifting later with each keystroke.
    THROTTLE_PENDING.set(true);
    if let Err(e) = arm_throttle_timer(remaining) {
        // Nothing else clears the flag if arming failed, and a stuck flag
        // would freeze the preview for the rest of the session.
        THROTTLE_PENDING.set(false);
        return Err(e);
    }

    Ok(())
}

/// Ask Neovim to run `:TimeTrackingThrottleFire` in `remaining`.
///
/// Deliberately Neovim's own `timer_start()` rather than nvim-oxi's
/// `libuv::TimerHandle`, which backed the debounce this replaced.
/// `TimerHandle` cannot be built on Windows — nvim-oxi's `uv_*` externs carry
/// no `raw-dylib` attribute and `nvim.exe` exports no such symbols — and it
/// leaks its `uv_timer_t` on every arm, because `libuv::Handle` has no `Drop`
/// impl and `TimerHandle` offers no `&mut self` re-arm. `timer_start` has
/// neither problem, and its callback runs on the main loop rather than in
/// libuv's fast event context, so the render it triggers needs no `schedule()`
/// hop to reach somewhere the API is legal.
///
/// The zero-argument lambda is Vim's own documented timer idiom (`:help
/// timer_start`): Neovim passes the timer id and the lambda ignores it.
fn arm_throttle_timer(remaining: Duration) -> Result<()> {
    // Floor of 1ms: `timer_start(0, ...)` is legal but says "next loop turn",
    // which is not what a sub-millisecond remainder means.
    let ms = remaining.as_millis().max(1);
    api::command(&format!(
        "call timer_start({ms}, {{-> execute('TimeTrackingThrottleFire')}})"
    ))?;
    Ok(())
}

/// `:TimeTrackingThrottleFire`: the render [`update_preview_throttled`] booked
/// for the end of the current window.
///
/// Internal — the timer is its only caller.
///
/// Returns `Ok(())` even when the render fails. This runs from a timer
/// callback with no user action attached, so an `Err` would surface as a bare
/// "Error executing vim function callback" with nothing to connect it to. The
/// logged message is more use than the error.
pub fn throttle_fire(config: &'static Config) -> Result<()> {
    THROTTLE_PENDING.set(false);
    LAST_RENDER.set(Some(Instant::now()));

    if let Err(e) = update_preview_fn(config) {
        log_error!("[time-tracking-nvim] throttled update failed: {}", e);
    }

    Ok(())
}

/// Clear the throttle window, so the next [`update_preview_throttled`] takes
/// the leading edge.
///
/// Test seam, not interface: it lets the integration tests establish a known
/// window boundary without sleeping.
#[doc(hidden)]
pub fn reset_throttle_for_test() {
    THROTTLE_PENDING.set(false);
    LAST_RENDER.set(None);
}
```

Also fix the one stale mention left behind: at `src/preview.rs:304` the doc comment on `update_preview_fn` reads "`:TimeTrackingUpdate`, and the render the debounce timer schedules" — change "debounce timer schedules" to "throttle books".

- [ ] **Step 5: Rewire `src/lib.rs`**

Replace the export block at `src/lib.rs:26-33`:

```rust
pub use preview::{
    auto_close_preview, auto_open_preview, close_preview, create_or_update_preview, throttle_fire,
    toggle_preview_fn, update_preview_fn, update_preview_throttled,
};
// Test seams, not interface: see `preview::write_preview_contents_with` and
// `preview::reset_throttle_for_test`.
#[doc(hidden)]
pub use preview::{reset_throttle_for_test, write_preview_contents_with};
```

Note this also drops the `use preview::*;` glob on `src/lib.rs:26` by naming `auto_close_preview` explicitly — that glob was the only thing supplying it. If removing the glob produces unresolved names, add them to this `pub use` list rather than putting the glob back.

Replace the debounced command closure at `src/lib.rs:186-190`:

```rust
    // Update the preview from the TextChanged autocommands, at most once per
    // throttle window.
    let update_preview_throttled_cmd = Function::from_fn(move |_: CommandArgs| {
        catch_nvim_panic(|| update_preview_throttled(config))
    });

    // The render the throttle books with `timer_start`. Internal: that timer
    // is its only caller.
    let throttle_fire_cmd =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| throttle_fire(config)));
```

Replace the `TimeTrackingUpdateDebounced` entry in the command table at `src/lib.rs:238-242` with two entries:

```rust
        (
            "TimeTrackingUpdateThrottled",
            "(internal) Re-render the preview, at most once per throttle window",
            update_preview_throttled_cmd,
        ),
        (
            "TimeTrackingThrottleFire",
            "(internal) Run the render the throttle booked",
            throttle_fire_cmd,
        ),
```

Replace the autocommand at `src/lib.rs:285`:

```rust
    api::command("autocmd TextChanged,TextChangedI *.md TimeTrackingUpdateThrottled")?;
```

- [ ] **Step 6: Collapse the `Cargo.toml` dependency blocks**

Delete the 9-line comment about the `libuv` feature and both `[target.'cfg(...)'.dependencies]` blocks (`Cargo.toml:27-46`), leaving `[dependencies]` as:

```toml
[dependencies]
time-tracking-cli = { git = "https://github.com/stevenwcarter/time-tracking-cli.git", branch = "main", default-features = false }
nvim-oxi = { git = "https://github.com/noib3/nvim-oxi", branch = "main", features = [
    "neovim-0-12",
] }

[workspace]
exclude = ["integration_tests"]
```

- [ ] **Step 7: Update every reference in the integration tests**

Four mechanical renames plus three assertion fixes.

Renames — `time_tracking_nvim::update_preview_debounced` → `time_tracking_nvim::update_preview_throttled` at every call site, including the one written in Step 1. Rename the test functions themselves:

| Old | New |
|---|---|
| `test_debounced_update_returns_without_blocking` | `test_throttled_update_coalesces_a_burst` |
| `test_debounced_update_eventually_renders` | `test_throttled_update_eventually_renders` |
| `test_debounced_update_renders_nothing_for_a_non_tracking_file` | `test_throttled_update_renders_nothing_for_a_non_tracking_file` |
| `test_autocommand_is_debounced_but_explicit_command_is_not` | `test_autocommand_is_throttled_but_explicit_command_is_not` |

Delete the `#[cfg(not(windows))]` attribute and the two-line `// Debounce-specific: Windows has no libuv timer...` comment above `test_throttled_update_coalesces_a_burst` and `test_autocommand_is_throttled_but_explicit_command_is_not`. Behaviour is uniform now.

Assertion fix A — in `test_throttled_update_coalesces_a_burst`, the loop's first iteration now takes the leading edge and renders. Insert a leading-edge burn plus a reset immediately after `let tick_before = preview.get_changedtick().unwrap();`, and re-read the tick:

```rust
    time_tracking_nvim::reset_throttle_for_test();
    // Burn the leading edge, so the 20 calls below all land inside one window.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    let tick_before = preview.get_changedtick().unwrap();
```

and change the final assertion's message from `"the debounce must not render synchronously on each keystroke"` to `"changes inside an open throttle window must not render synchronously"`. Also update the `20 debounced updates took` message to `20 throttled updates took`, and the `// Simulate a burst of keystrokes: each re-arms the timer` comment to `// Simulate a burst of keystrokes inside one window: each is dropped and returns at once`.

Assertion fix B — in `test_autocommand_is_throttled_but_explicit_command_is_not`, the first `doautocmd TextChanged` now renders (leading edge), so `assert_eq!(tick, tick_before)` is wrong. Replace the body from `let tick_before = preview.get_changedtick().unwrap();` to the end of the function with:

```rust
    time_tracking_nvim::reset_throttle_for_test();

    // The leading edge: the first TextChanged renders synchronously.
    api::exec2("doautocmd TextChanged", &Default::default()).unwrap();
    let tick_after_first = preview.get_changedtick().unwrap();
    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "the first TextChanged must render at once"
    );

    // A second one inside the same window must not: it is booked instead.
    // This is what pins the autocommand to `:TimeTrackingUpdateThrottled`
    // rather than the unthrottled `:TimeTrackingUpdate`.
    api::exec2("doautocmd TextChanged", &Default::default()).unwrap();
    assert_eq!(
        preview.get_changedtick().unwrap(),
        tick_after_first,
        "a TextChanged inside an open throttle window must go through the throttle"
    );

    // The converse: `:TimeTrackingUpdate` is not throttled, so it renders even
    // inside the window. Re-prime the sentinel so the render is visible.
    create_or_update_preview("PLACEHOLDER").unwrap();
    let tick_before_explicit = preview.get_changedtick().unwrap();
    api::command("TimeTrackingUpdate").unwrap();
    assert!(
        preview.get_changedtick().unwrap() > tick_before_explicit,
        "the explicit :TimeTrackingUpdate command must still render at once"
    );
```

Assertion fix C — `test_time_tracking_with_config_creates_commands` (`integration_tests/src/lib.rs:428-443`) pins the exact command registry. It grows from seven entries to eight. Replace the `expected` vector with (this is Lua `table.sort` byte order):

```rust
    let expected = vec![
        "TimeTrackingAutoClose nargs=0 handler=true".to_string(),
        "TimeTrackingAutoOpen nargs=0 handler=true".to_string(),
        "TimeTrackingClose nargs=0 handler=true".to_string(),
        "TimeTrackingMaybeCloseIfInvisible nargs=? handler=true".to_string(),
        "TimeTrackingThrottleFire nargs=0 handler=true".to_string(),
        "TimeTrackingToggle nargs=0 handler=true".to_string(),
        "TimeTrackingUpdate nargs=0 handler=true".to_string(),
        "TimeTrackingUpdateThrottled nargs=0 handler=true".to_string(),
    ];
```

and change `"exactly these seven TimeTracking* commands"` to `"exactly these eight TimeTracking* commands"`.

Finally, in `test_throttled_update_renders_nothing_for_a_non_tracking_file`, change the comment `// Turn the event loop well past the debounce window.` to `// Turn the event loop well past the throttle window.`, and in `test_explicit_update_renders_immediately` change `"wait out a debounce window"` → `"wait out a throttle window"` and `"must not be deferred behind the debounce"` → `"must not be deferred behind the throttle"`.

- [ ] **Step 8: Build, lint, and run the whole suite**

```bash
cargo build
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cd integration_tests && cargo test --verbose
```

Expected: all green, including `test_throttle_renders_repeatedly_during_sustained_typing`, which was red at Step 2.

If `cargo clippy` flags the `remaining` shadowing in the let-else, keep the shadowing and silence nothing — rename the binding instead.

- [ ] **Step 9: Commit**

```bash
git add src/preview.rs src/lib.rs Cargo.toml Cargo.lock integration_tests/src/lib.rs integration_tests/Cargo.lock
git commit -m "$(cat <<'EOF'
feat: throttle preview updates to 200ms instead of debouncing

The 150ms trailing-edge debounce re-armed its timer on every keystroke,
so the preview stayed frozen for as long as the user kept typing and
only caught up during pauses. Replace it with a leading-edge throttle:
the first change renders at once, the rest land on a 200ms cadence, and
a trailing render always closes the burst so the preview never ends
stale.

Books the trailing render with Neovim's own timer_start() rather than
nvim-oxi's libuv TimerHandle. That drops the #[cfg(windows)] fork (the
libuv externs cannot link there), the ~200-byte-per-arm leak (libuv
Handle has no Drop impl), the libuv Cargo feature with its duplicate
target-specific dependency blocks, and the schedule() hop that the
fast event context required.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

### Task 2: Regression tests for the throttle contract

Task 1's cadence test proves the behaviour changed. These three pin the specific rules a future edit could break silently. Each names the mutation that turns it red, so a reviewer can check it actually guards something.

**Files:**
- Test: `integration_tests/src/lib.rs` (add three tests, delete one now-subsumed test)

**Interfaces:**
- Consumes from Task 1: `update_preview_throttled`, `reset_throttle_for_test`, commands `:TimeTrackingUpdateThrottled` / `:TimeTrackingThrottleFire`.
- Produces: nothing.

- [ ] **Step 1: Add the leading-edge test**

Guards: deleting the `let Some(remaining) = remaining else { ... }` leading-edge branch in `update_preview_throttled` (i.e. always booking a timer) turns this red.

```rust
#[nvim_oxi::test]
fn test_throttled_update_renders_first_change_immediately() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();
    time_tracking_nvim::reset_throttle_for_test();

    // No event-loop turn happens between this call and the assertion, so
    // anything deferred leaves the sentinel in place. The debounce this
    // replaced always deferred.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "the first change in a burst must render before the call returns; \
         the preview still reads {:?}",
        preview_text(&preview)
    );
}
```

- [ ] **Step 2: Add the one-timer-per-window test**

Guards: deleting the `if THROTTLE_PENDING.get() { return Ok(()); }` early return. Without it every keystroke books its own timer — the debounce's re-arm-per-keystroke behaviour in new clothes — and this goes red while the burst test in Task 1 stays green.

```rust
#[nvim_oxi::test]
fn test_throttled_burst_books_exactly_one_render() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // Registered after the preview exists, so no BufEnter handler runs during
    // setup. Needed because the calls below book a real timer, which fires
    // `:TimeTrackingThrottleFire`.
    time_tracking_with_config(config_static).unwrap();

    // No event-loop turn from here on, so nothing this books can fire before
    // the assertions and `timer_info()` still lists it.
    time_tracking_nvim::reset_throttle_for_test();
    let timers_before: i64 = api::eval("len(timer_info())").unwrap();

    // Burn the leading edge, then 20 more changes inside the same window.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    for _ in 0..20 {
        time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    }

    let timers_after: i64 = api::eval("len(timer_info())").unwrap();
    assert_eq!(
        timers_after - timers_before,
        1,
        "21 changes inside one throttle window must book exactly one render, \
         got {}",
        timers_after - timers_before
    );

    // Leave nothing armed for whatever test runs next in this Neovim.
    api::command("call timer_stopall()").unwrap();
    let _ = preview;
}
```

- [ ] **Step 3: Add the trailing-render test**

Guards the never-stale guarantee, and is the only test that pins the spec's load-bearing invariant — that a `timer_start` callback runs on the main loop, where `nvim_buf_set_lines` is legal. If that were false the render would fail with `E5560` and the sentinel would survive.

```rust
#[nvim_oxi::test]
fn test_throttled_update_renders_the_trailing_change() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // The trailing render arrives through `:TimeTrackingThrottleFire`, so the
    // commands must exist or the timer fires into E492.
    time_tracking_with_config(config_static).unwrap();
    time_tracking_nvim::reset_throttle_for_test();

    // Burn the leading edge, then re-prime the sentinel so only a *second*,
    // trailing render can clear it.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    create_or_update_preview("PLACEHOLDER").unwrap();

    // A change inside the window: booked, not rendered.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    assert!(
        preview_text(&preview).contains("PLACEHOLDER"),
        "a change inside an open window must not render synchronously"
    );

    // Turn the event loop past the window boundary so the booked render fires.
    // `bufnr()` takes a pattern, so address the preview by handle.
    let handle = preview.handle();
    api::exec2(
        &format!(
            "lua vim.wait(2000, function() \
               local l = vim.api.nvim_buf_get_lines({handle}, 0, 1, false)[1] or '' \
               return not l:find('PLACEHOLDER', 1, true) \
             end, 10)"
        ),
        &Default::default(),
    )
    .unwrap();

    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "the booked trailing render must land, so a burst never leaves the \
         preview stale; it still reads {:?}",
        preview_text(&preview)
    );
}
```

- [ ] **Step 4: Delete the subsumed test**

`test_throttled_update_eventually_renders` (renamed in Task 1 Step 7) now only exercises the leading edge, which Step 1 covers directly and more precisely — and its name claims to test the deferred path, which it no longer reaches. Delete the whole function. Step 3 is its real successor.

- [ ] **Step 5: Run the suite**

```bash
cd integration_tests && cargo test --verbose
```

Expected: PASS, including all three new tests.

- [ ] **Step 6: Verify each new test actually guards something**

For each mutation below, apply it to `src/preview.rs`, run the named test, confirm it FAILS, then revert:

1. Delete the `let Some(remaining) = remaining else { ... };` block and always book a timer → `test_throttled_update_renders_first_change_immediately` fails.
2. Delete `if THROTTLE_PENDING.get() { return Ok(()); }` → `test_throttled_burst_books_exactly_one_render` fails (21 timers, not 1).
3. In `throttle_fire`, replace the `update_preview_fn(config)` call with `Ok(())` → `test_throttled_update_renders_the_trailing_change` fails.

Confirm the working tree is clean of all three mutations before committing.

- [ ] **Step 7: Commit**

```bash
git add integration_tests/src/lib.rs
git commit -m "$(cat <<'EOF'
test: pin the throttle contract

Three regression tests, each verified to fail against a specific
mutation: the leading edge renders synchronously, a burst inside one
window books exactly one render, and the booked trailing render lands.

The trailing test is also what pins the invariant the timer backend
rests on — that a timer_start callback runs on the main loop, where
nvim_buf_set_lines is legal. If that stopped holding the render would
die with E5560 and this test would catch it.

Drops test_throttled_update_eventually_renders, which the trailing
test subsumes.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

### Task 3: Documentation and final verification

**Files:**
- Modify: `README.md:128-132`
- Modify: `CLAUDE.md:38` (command list), `CLAUDE.md:42` (data flow)
- Modify: `lua/time-tracking-nvim/init.lua:1056-1057`

**Interfaces:**
- Consumes from Task 1: the command names and the throttle behaviour being documented.
- Produces: nothing.

- [ ] **Step 1: Fix the README's platform paragraph**

Every clause of `README.md:128-132` is now false — there is no debounce, no Linux/macOS restriction, and no libuv. Replace those five lines with:

```markdown
Live preview updates are throttled: the first change renders immediately, then
further changes render at most once every 200ms while you keep typing, with a
final render once you stop. Linux, macOS and Windows all behave identically.
```

- [ ] **Step 2: Fix `CLAUDE.md`**

`CLAUDE.md:38` says the plugin "Registers 5 user commands" and lists five. It has been wrong since `TimeTrackingMaybeCloseIfInvisible` and the debounce command were added; Task 1 makes it eight. Replace that sentence's command clause with:

```
Registers 8 user commands (`TimeTrackingToggle`, `TimeTrackingUpdate`,
`TimeTrackingUpdateThrottled`, `TimeTrackingThrottleFire`,
`TimeTrackingAutoOpen`, `TimeTrackingAutoClose`, `TimeTrackingClose`,
`TimeTrackingMaybeCloseIfInvisible` — the last four are internal, driven by
autocommands and the throttle timer)
```

`CLAUDE.md:42` says "TextChanged events update the preview in real-time". Replace that clause with:

```
TextChanged events update the preview, throttled to at most one render per 200ms (the first change in a burst renders immediately, and a trailing render closes it)
```

- [ ] **Step 3: Fix the Lua comment**

`lua/time-tracking-nvim/init.lua:1056-1057` reads "skipping the / TextChanged debounce." Change the word `debounce` to `throttle`. `M.update()` itself is unchanged — it still calls `:TimeTrackingUpdate`.

- [ ] **Step 4: Confirm no stale references survive**

```bash
grep -rniE "debounc" --include="*.rs" --include="*.lua" --include="*.md" --include="*.toml" --include="*.sh" --include="*.yml" . | grep -v "^./target" | grep -v "^./docs/superpowers"
```

Expected: no output. Hits under `docs/superpowers/` are historical specs and plans (including this one) and must **not** be rewritten — they are a record of what was decided when.

```bash
grep -rn "libuv\|TimerHandle" --include="*.rs" --include="*.toml" . | grep -v "^./target" | grep -v "^./docs/superpowers"
```

Expected: no output.

- [ ] **Step 5: Full verification**

```bash
cargo build
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
./build.sh
cd integration_tests && cargo test --verbose
```

Expected: all green. `./build.sh` must still produce `time_tracking_nvim.so` (the rename step), confirming the cdylib builds without the libuv feature.

Windows cannot be verified locally. The change removes the only Windows-specific code path and introduces no FFI, so CI on `windows-latest` (`.github/workflows/ci.yml:19`) is the check — note in the PR that it is the gate for the Windows claim.

- [ ] **Step 6: Commit**

```bash
git add README.md CLAUDE.md lua/time-tracking-nvim/init.lua
git commit -m "$(cat <<'EOF'
docs: describe the throttle, and correct the command count

The README's platform paragraph was false in every clause after the
throttle landed. CLAUDE.md's "registers 5 user commands" had been stale
since TimeTrackingMaybeCloseIfInvisible was added; it is eight now.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Self-review

**Spec coverage.** Every spec section maps to a task: backend choice and algorithm → Task 1 Steps 3-4; naming table → Task 1 Steps 4-5, 7; deletions → Task 1 Steps 3, 4, 6, 7; "explicitly unchanged" → Global Constraints plus Task 1 Step 7's Assertion fix B; invariants 1-4 → Task 2 Step 3 pins invariant 1, Task 1 Step 4's guard preserves invariant 3; test plan table → Task 1 Step 1 and Task 2 Steps 1-3, with `test_throttled_update_renders_nothing_for_a_non_tracking_file` and `test_explicit_update_renders_immediately` carried forward in Task 1 Step 7; documentation → Task 3.

Two items surfaced during planning that the spec did not name, both folded into Task 1 Step 7: `test_time_tracking_with_config_creates_commands` pins the exact command registry and grows from seven entries to eight, and `src/lib.rs:26`'s `use preview::*;` glob has to go when `auto_close_preview` is named explicitly. A third, in Task 3 Step 2: `CLAUDE.md`'s command count was already stale before this work.

**Type consistency.** `update_preview_throttled(&'static Config) -> Result<()>`, `throttle_fire(&'static Config) -> Result<()>`, `arm_throttle_timer(Duration) -> Result<()>` (private), `reset_throttle_for_test()`. `LAST_RENDER: Cell<Option<Instant>>` and `THROTTLE_PENDING: Cell<bool>` are `Cell`, so `.get()`/`.set()` throughout — never `.borrow_mut()`, which is what the `RefCell`-based `PREVIEW_BUF` and `LAST_OUTPUT` use. `THROTTLE: Duration`. Command names match the Global Constraints list verbatim in Task 1 Steps 4, 5 and 7 and in Task 3 Step 2.
