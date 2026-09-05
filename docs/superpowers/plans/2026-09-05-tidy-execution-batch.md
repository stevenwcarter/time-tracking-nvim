# Tidy Execution Batch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply the 17 `TIDY.md` findings the user marked for execution — import hygiene, dead code, duplication, and small perf wins across Rust, Lua, and CI — one commit per finding.

**Architecture:** Eight chains, ordered so that each edit lands on code the previous edit already settled: Rust imports before Rust bodies, `M.check`'s decomposition before the CI task edits a `health.lua` line that would otherwise move under it, and `T22`'s hoisted constant before `T23`'s comments point at it. Every finding gets its own commit so any single fix stays independently revertable.

**Tech Stack:** Rust (edition 2024, nvim-oxi `neovim-0-12`), Lua (Neovim runtime), bash, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-05-tidy-execution-batch-design.md`

## Global Constraints

- **Line numbers in this plan were correct at `4066745` and drift as tasks land.** Locate by symbol name and by the quoted code. Never apply a hunk at a remembered offset — re-grep first.
- **`env -u CARGO_TARGET_DIR` on every cargo invocation.** This machine sets a global `CARGO_TARGET_DIR` that collides with the workspace-excluded `integration_tests` crate; without the prefix that crate fails to compile with ~56 spurious `E0308` errors that have nothing to do with your change.
- Verification per finding, all four must pass:
  `env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`,
  and the same two with `cd integration_tests`.
- Full suite at each chain boundary: `cd integration_tests && env -u CARGO_TARGET_DIR cargo test` — **52 tests, all must stay green.** Lua changes also run `./integration_tests/lua/run_lua_tests.sh`.
- **One commit per finding**, message form `tidy(<lens>): <summary> [T<n>]`. Never batch two findings into one commit.
- **`TIDY.md` is git-ignored** via `.git/info/exclude`, so `todo-parser TIDY.md --strip T<n>` changes nothing git can see. Run the strip anyway — it is the findings-file bookkeeping — but do **not** `git add -f` it, and do not be alarmed when the commit contains only code.
- **A pre-commit hook is active** (`core.hooksPath scripts/hooks`). It runs rustfmt over fully-staged `.rs` files and restages them. It deliberately skips files that have *both* staged and unstaged changes, naming them in its output — if you see that message, stage the rest or stash it.
- **Do not refactor existing tests.** Update call sites when a signature changes (T38, T42 require it), and add new tests only where a task says to (T7).
- Never use `--no-verify`, `--allow-dirty`, or `SKIP_RUSTFMT=1`.

---

### Task 1: Rust import hygiene (T31 → T35 → T34)

**Files:**
- Modify: `src/lib.rs:9-21` (import block), `src/lib.rs:101`, `src/lib.rs:118`
- Modify: `src/preview.rs:1-5` (import block)

**Interfaces:**
- Consumes: nothing.
- Produces: `src/preview.rs` owns its own imports of `Buffer`, `Window`, `OptionOptsBuilder`, `get_buffer_content`, `is_time_tracking_file`. Later Rust tasks edit bodies in this file and must not reintroduce `use super::*`.

T31 and T35 must land back to back. T31 alone leaves preview.rs compiling only because the glob is still there; do not stop between them.

- [ ] **Step 1: T31 — move the five names to where they are used**

In `src/lib.rs`, delete these two lines entirely:

```rust
use nvim_oxi::api::opts::OptionOptsBuilder;
use nvim_oxi::api::{Buffer, Window};
```

and narrow this line:

```rust
use crate::utils::{any_tracking_visible, get_buffer_content, is_time_tracking_file};
```

to:

```rust
use crate::utils::any_tracking_visible;
```

In `src/preview.rs`, extend the existing explicit import (currently line 3) and add the nvim_oxi one beside it:

```rust
use crate::utils::{
    PREVIEW_BUF_NAME, get_buffer_content, is_preview_buf, is_time_tracking_file,
};
use nvim_oxi::api::{Buffer, Window, opts::OptionOptsBuilder};
```

Leave `use super::*;` on line 1 for now — Step 3 removes it.

- [ ] **Step 2: Verify T31 builds**

Run: `env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings`
Expected: clean. If `unused_imports` fires in preview.rs, the glob was already supplying a name you added twice — remove your duplicate, not the glob.

- [ ] **Step 3: Commit T31**

```bash
todo-parser TIDY.md --strip T31
git add src/lib.rs src/preview.rs
git commit -m 'tidy(dead-code): move lib.rs-only imports to preview.rs, their sole user [T31]'
```

- [ ] **Step 4: T35 — replace the wildcard import**

In `src/preview.rs`, replace line 1 (`use super::*;`) with direct imports from the real crates rather than re-exports through `super`:

```rust
use crate::{debug_log, log_error, log_info};
use nvim_oxi::{Result, api};
use time_tracking_cli::Config;
```

The three macros are `#[macro_export]`, so they live at the crate root and `use crate::{...}` is the right path for them. Ask the compiler for the exact remaining set rather than guessing:

```bash
env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -W clippy::wildcard_imports
```

If a name you removed turns out to be needed (for example `catch_nvim_panic`), add `use super::catch_nvim_panic;` — a `super::` import of one named item is fine; only the glob is the finding.

- [ ] **Step 5: Verify T35**

Run: `env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings -W clippy::wildcard_imports`
Expected: clean, and zero `wildcard_imports` warnings crate-wide.

- [ ] **Step 6: Commit T35**

```bash
todo-parser TIDY.md --strip T35
git add src/preview.rs
git commit -m 'tidy(idioms): replace preview.rs wildcard import with explicit names [T35]'
```

- [ ] **Step 7: T34 — hoist the duplicated `use std::io::Write;`**

In `src/lib.rs`, add it to the module header so it sits beside the existing std import:

```rust
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
```

Then delete the two in-function copies. The first is inside the `panic::set_hook` closure:

```rust
    panic::set_hook(Box::new(|info| {
        let msg = format!("[ttnvim] PANIC: {info}\n");
        use std::io::Write;                    // <-- delete this line
        let _ = std::io::stderr().write_all(msg.as_bytes());
    }));
```

The second is inside the `Err(e)` arm of the `match &r` block:

```rust
            Err(e) => {
                use std::io::Write;            // <-- delete this line
                let _ = std::io::stderr().write_all(
```

**Leave the copy inside the `debug_log!` macro body alone** (the one near `if std::env::var("TIME_TRACKING_DEBUG").is_ok()`). A macro needs its own import to stay hygienic at arbitrary expansion sites, and that one is not what clippy flags.

- [ ] **Step 8: Verify T34**

Run: `env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings -W clippy::items_after_statements`
Expected: clean, and zero `items_after_statements` warnings.

- [ ] **Step 9: Commit T34 and run the suite**

```bash
todo-parser TIDY.md --strip T34
git add src/lib.rs
git commit -m 'tidy(idioms): hoist the duplicated std::io::Write import [T34]'
cd integration_tests && env -u CARGO_TARGET_DIR cargo test
```

Expected: 52 passed.

---

### Task 2: preview.rs body cleanups (T36 → T40 → T41)

**Files:**
- Modify: `src/preview.rs` — `create_or_update_preview` (~:575), `auto_open_preview` (~:664), `auto_close_preview` (~:700), `auto_open_preview_impl` (~:680), `auto_close_preview_impl` (~:714)

**Interfaces:**
- Consumes: Task 1's import block in preview.rs.
- Produces: `fn log_and_swallow(label: &str, r: Result<()>) -> Result<()>` (private). `auto_open_preview_impl` and `auto_close_preview_impl` become private; their public wrappers `auto_open_preview` / `auto_close_preview` keep their existing signatures and stay exported from lib.rs.

T40 before T41 so T41 sees the final shape of both wrappers.

- [ ] **Step 1: T36 — match to let-else**

In `create_or_update_preview`, replace:

```rust
    let mut buf: Buffer = match preview {
        Some(b) => b,
        None => create_preview_buffer()?,
    };
```

with:

```rust
    let mut buf: Buffer = if let Some(b) = preview {
        b
    } else {
        create_preview_buffer()?
    };
```

- [ ] **Step 2: Verify and commit T36**

```bash
env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
todo-parser TIDY.md --strip T36
git add src/preview.rs
git commit -m 'tidy(idioms): let-else the preview buffer lookup [T36]'
```

- [ ] **Step 3: T40 — extract the shared log-and-swallow wrapper**

Add this private helper immediately above `auto_open_preview`:

```rust
/// Run `r`, reporting any error under `label` and swallowing it.
///
/// Both autocommand-driven wrappers want the same thing: a failure reported
/// once, not propagated. Propagating would re-echo the same message on every
/// buffer switch. Panics are caught a level up, by `catch_nvim_panic` in
/// `lib.rs`.
fn log_and_swallow(label: &str, r: Result<()>) -> Result<()> {
    if let Err(e) = r {
        log_error!("{} failed: {}", label, e);
    }
    Ok(())
}
```

Then replace both wrapper bodies. `auto_open_preview` becomes:

```rust
/// Auto-open preview window if this is a time tracking file and preview isn't open
pub fn auto_open_preview(config: &'static Config) -> Result<()> {
    log_and_swallow("Auto-open", auto_open_preview_impl(config))
}
```

and `auto_close_preview` becomes:

```rust
/// Auto-close preview window if we're not in a time tracking file
pub fn auto_close_preview(config: &'static Config) -> Result<()> {
    log_and_swallow("Auto-close", auto_close_preview_impl(config))
}
```

The helper reproduces the existing messages exactly — `"Auto-open failed: {e}"` and `"Auto-close failed: {e}"`. **Keep both `*_impl` functions**; do not fold `auto_close_preview_impl` into its wrapper, because T41 (next) narrows its visibility and expects it to still exist.

- [ ] **Step 4: Verify T40**

Run: `env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings -W clippy::ignored_unit_patterns`
Expected: the two `Ok(_) => Ok(())` hits in preview.rs are gone. One hit remains at `src/lib.rs:165` (`schedule(|_| ...)`) — **leave it**, it is not part of this finding.

- [ ] **Step 5: Commit T40**

```bash
todo-parser TIDY.md --strip T40
git add src/preview.rs
git commit -m 'tidy(duplication): extract log_and_swallow for the auto-open/close wrappers [T40]'
```

- [ ] **Step 6: T41 — narrow visibility**

Drop `pub` from both:

```rust
fn auto_open_preview_impl(config: &'static Config) -> Result<()> {
```

```rust
fn auto_close_preview_impl(_config: &'static Config) -> Result<()> {
```

Neither appears in lib.rs's `pub use preview::{...}` lists, and `mod preview;` is private, so neither was reachable outside the crate anyway. Integration tests call only the wrappers.

- [ ] **Step 7: Verify and commit T41**

```bash
env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings
cd integration_tests && env -u CARGO_TARGET_DIR cargo test && cd ..
todo-parser TIDY.md --strip T41
git add src/preview.rs
git commit -m 'tidy(dead-code): make the auto-open/close impls private [T41]'
```

Expected: 52 passed.

---

### Task 3: preview.rs render path (T37 → T38)

**Files:**
- Modify: `src/preview.rs` — `preview_is_open` (~:301), `render_current_buffer` (~:309), `toggle_preview_fn` (~:347), `update_preview_fn` (~:361), `set_preview_lines` (~:390), `write_preview_contents_with` (~:425), `create_or_update_preview` (~:563), `auto_open_preview_impl` (~:680)
- Modify: `integration_tests/src/lib.rs` — the injected-writer test

**Interfaces:**
- Consumes: Task 2's `log_and_swallow`, private `*_impl` functions.
- Produces:
  - `fn create_or_update_preview_with(found: Option<(Buffer, Option<Window>)>, output: &str) -> Result<()>` (private)
  - `pub fn create_or_update_preview(output: &str) -> Result<()>` — **signature unchanged**, integration tests call it directly in many places
  - `fn render_current_buffer(config: &Config, found: Option<(Buffer, Option<Window>)>) -> Result<()>`
  - `fn preview_is_open_in(found: &Option<(Buffer, Option<Window>)>) -> bool`
  - `write_preview_contents_with`'s callback type becomes `fn(&mut Buffer, Vec<&str>) -> Result<()>`

This is the riskiest task in the batch: it moves a lookup the throttle's leading-edge path depends on. Run the full suite at the end of each finding, not just at the end of the task.

- [ ] **Step 1: T37 — split the render entry point**

Replace `create_or_update_preview` with a delegate plus the new inner function:

```rust
/// Create or update the preview window with formatted time tracking data
pub fn create_or_update_preview(output: &str) -> Result<()> {
    // Bail if Neovim has no windows yet (during early startup churn)
    if api::list_wins().next().is_none() {
        return Ok(());
    }

    create_or_update_preview_with(find_preview()?, output)
}

/// [`create_or_update_preview`] with the lookup already done.
///
/// Callers that had to probe for an open preview before deciding to render
/// pass their own `find_preview` result straight through, instead of throwing
/// it away and making this function repeat the scan.
fn create_or_update_preview_with(
    found: Option<(Buffer, Option<Window>)>,
    output: &str,
) -> Result<()> {
    let (preview, preview_win) = match found {
        Some((buf, win)) => (Some(buf), win),
        None => (None, None),
    };

    let mut buf: Buffer = if let Some(b) = preview {
        b
    } else {
        create_preview_buffer()?
    };

    write_preview_contents(&mut buf, output)?;

    // `find_preview` resolved this above; a buffer created just now is by
    // definition displayed nowhere.
    if preview_win.is_none() {
        open_preview_split(&buf)?;
    }

    Ok(())
}
```

- [ ] **Step 2: T37 — thread the lookup through `render_current_buffer`**

```rust
fn render_current_buffer(config: &Config, found: Option<(Buffer, Option<Window>)>) -> Result<()> {
    let buffer_content = get_buffer_content()?;
    let formatted_output = config.get_formatter().day_summary(
        &buffer_content,
        "",
        config.get_prefix(),
        config.get_suffix(),
    );
    create_or_update_preview_with(found, &formatted_output)
}
```

Replace `preview_is_open` with a version that reads an already-resolved lookup:

```rust
/// Is a window in the current tabpage showing the preview, per an already
/// resolved [`find_preview`] result?
fn preview_is_open_in(found: &Option<(Buffer, Option<Window>)>) -> bool {
    matches!(found, Some((_, Some(_))))
}
```

Delete the old no-argument `preview_is_open` once its last caller is gone — `-D warnings` will flag it as dead code if you leave it.

- [ ] **Step 3: T37 — update the three callers**

`update_preview_fn`:

```rust
pub fn update_preview_fn(config: &'static Config) -> Result<()> {
    if !is_time_tracking_file(config)? {
        return Ok(());
    }

    let found = find_preview()?;
    if preview_is_open_in(&found) {
        render_current_buffer(config, found)?;
    }

    Ok(())
}
```

`toggle_preview_fn` — replace only its trailing open/close decision, leaving the not-a-tracking-file guard above it untouched:

```rust
    let found = find_preview()?;
    if preview_is_open_in(&found) {
        close_preview()?;
    } else {
        render_current_buffer(config, found)?;
    }

    Ok(())
}
```

`auto_open_preview_impl` — replace its trailing block:

```rust
    let found = find_preview()?;
    if !preview_is_open_in(&found) {
        render_current_buffer(config, found)?;
    }

    Ok(())
}
```

- [ ] **Step 4: Verify T37**

```bash
env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings
cd integration_tests && env -u CARGO_TARGET_DIR cargo test
```

Expected: 52 passed. This finding touches the throttle's render path, so a failure here is real — do not proceed past a red suite. Pay particular attention to the tabpage tests: `find_preview` is deliberately current-tabpage-scoped and that must survive.

- [ ] **Step 5: Commit T37**

```bash
todo-parser TIDY.md --strip T37
git add src/preview.rs
git commit -m 'tidy(opportunistic): pass the resolved preview lookup into the render path [T37]'
```

- [ ] **Step 6: T38 — widen the test seam to borrow instead of allocate**

Change `set_preview_lines` to take borrowed lines:

```rust
/// The real line write behind [`write_preview_contents`].
fn set_preview_lines(buf: &mut Buffer, lines: Vec<&str>) -> Result<()> {
    buf.set_lines(0..buf.line_count()?, false, lines)?;
    Ok(())
}
```

Change the seam's callback type and drop the per-line allocation in `write_preview_contents_with`:

```rust
pub fn write_preview_contents_with(
    buf: &mut Buffer,
    output: &str,
    write_lines: fn(&mut Buffer, Vec<&str>) -> Result<()>,
) -> Result<()> {
```

```rust
    let lines: Vec<&str> = output.lines().collect();
```

**Do not delete the seam.** The parameter exists so a test can inject a failing writer; that is the only way to provoke the failure path this function's restore-before-propagate ordering guards. Keeping it is the whole point of choosing this route over a direct `buf.set_lines(..., output.lines())`.

- [ ] **Step 7: T38 — update the injecting test**

In `integration_tests/src/lib.rs`, the test is `test_a_failed_preview_write_restores_nomodifiable_and_leaves_the_cache_clean` (~:2090, the B37 regression guard). The failing writer it passes to `write_preview_contents_with` must change its parameter from `Vec<String>` to `Vec<&str>` to match the new callback type. Change only that signature — not the assertions, and not the comment above the test explaining why the seam exists.

- [ ] **Step 8: Verify and commit T38**

```bash
env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings
cargo fmt --all -- --check
cd integration_tests && env -u CARGO_TARGET_DIR cargo test && cd ..
todo-parser TIDY.md --strip T38
git add src/preview.rs integration_tests/src/lib.rs
git commit -m 'tidy(opportunistic): borrow preview lines instead of allocating per line [T38]'
```

Expected: 52 passed. If `set_lines` rejects `Vec<&str>`, report it rather than reverting to `Vec<String>` silently — the fallback is `Vec<Cow<'_, str>>`, not abandoning the finding.

---

### Task 4: utils.rs predicates and buffer read (T42 → T43)

**Files:**
- Modify: `src/utils.rs` — `is_time_tracking_file` (~:128), `is_win_time_tracking_file` (~:136), `is_buf_time_tracking_file` (~:141), `get_buffer_content` (~:176), `any_tracking_visible` (~:213)
- Modify: `integration_tests/src/lib.rs` — ~20 call sites

**Interfaces:**
- Consumes: nothing from Tasks 1-3.
- Produces: `pub fn is_win_time_tracking_file(win: &Window, config: &Config) -> Result<bool>` and `pub fn is_buf_time_tracking_file(current_buffer: &Buffer, config: &Config) -> Result<bool>`.

**T42 is a public-API signature change.** It is normally never auto-applied; the user approved it explicitly for this batch. That approval does not extend to any other signature.

- [ ] **Step 1: T42 — take the handles by reference**

```rust
/// Check if the current buffer is a time tracking file (markdown file in data directory)
pub fn is_time_tracking_file(config: &Config) -> Result<bool> {
    let current_buffer = api::get_current_buf();

    is_buf_time_tracking_file(&current_buffer, config)
}

/// Check if the provided window's buffer is a time tracking file (markdown file in data directory)
pub fn is_win_time_tracking_file(win: &Window, config: &Config) -> Result<bool> {
    is_buf_time_tracking_file(&win.get_buf()?, config)
}

/// Checks if the provided buffer is a time tracking file (markdown file in data directory)
pub fn is_buf_time_tracking_file(current_buffer: &Buffer, config: &Config) -> Result<bool> {
```

In `any_tracking_visible`, pass the borrow:

```rust
        if is_win_time_tracking_file(&win, config)? {
            return Ok(true);
        }
```

- [ ] **Step 2: T42 — update every integration-test call site**

Run `env -u CARGO_TARGET_DIR cargo build --tests` from `integration_tests/` and let the compiler enumerate them; there are roughly 20. Each is a mechanical `f(x, config)` → `f(&x, config)`. Do not restructure the tests around the change — add the `&` and nothing else.

- [ ] **Step 3: Verify T42 with the full suite**

```bash
env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings -W clippy::needless_pass_by_value
cd integration_tests && env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings
cd integration_tests && env -u CARGO_TARGET_DIR cargo test
```

Expected: 52 passed, and no `needless_pass_by_value` on either predicate. One unrelated hit remains on `panic_message` in `src/lib.rs` — leave it.

- [ ] **Step 4: Commit T42**

```bash
todo-parser TIDY.md --strip T42
git add src/utils.rs integration_tests/src/lib.rs
git commit -m 'tidy(idioms): take Window/Buffer by reference in the tracking-file predicates [T42]'
```

- [ ] **Step 5: T43 — stop allocating a String per line**

In `get_buffer_content`, replace:

```rust
        content.push_str(&line.to_string());
```

with:

```rust
        content.push_str(&line.to_string_lossy());
```

`nvim_oxi::String::to_string_lossy` returns `Cow<'_, str>` and delegates to the same `self.inner` as `Display::fmt`, so behaviour is unchanged. The comment two lines above already explains that this loop exists to avoid allocations — this makes it true for the last one.

- [ ] **Step 6: Verify and commit T43**

```bash
env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings
cd integration_tests && env -u CARGO_TARGET_DIR cargo test && cd ..
todo-parser TIDY.md --strip T43
git add src/utils.rs
git commit -m 'tidy(opportunistic): borrow lines in get_buffer_content [T43]'
```

Expected: 52 passed, including `test_get_buffer_content` and `test_get_buffer_content_empty`.

---

### Task 5: Lua init.lua (T21 → T22 → T27)

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` — `default_config` (~:29), `get_platform_info` (~:65), `echo` (~:16) and 21 call sites

**Interfaces:**
- Consumes: nothing.
- Produces: module-level `PLATFORM_MAPPINGS`, `normalize_arch(os_name, arch)`, and `notify(kind, chunks)`. Task 7 references `PLATFORM_MAPPINGS` in a comment.

- [ ] **Step 1: T21 — delete the dead config keys**

In `default_config`, delete exactly these three lines:

```lua
	-- Add any configuration options here
	-- auto_start = true,
	-- preview_width = nil, -- Will use 1/3 of screen width
```

Keep `auto_download`, `auto_update`, `allow_unverified_download` and the comment block above `allow_unverified_download`. Neither deleted key is read anywhere in the repo; the preview width is computed unconditionally in `src/preview.rs`.

- [ ] **Step 2: Verify and commit T21**

```bash
./integration_tests/lua/run_lua_tests.sh
todo-parser TIDY.md --strip T21
git add lua/time-tracking-nvim/init.lua
git commit -m 'tidy(dead-code): drop the two never-implemented default_config keys [T21]'
```

- [ ] **Step 3: T22 — one uname call, hoisted mappings, extracted arch normalizer**

Add the mappings table at module level, immediately above `local CURL_HARDENING = {`:

```lua
-- Target triple and library extension per OS/arch. One of four places this
-- mapping lives; see the comment on `normalize_arch` and T23's pointers.
local PLATFORM_MAPPINGS = {
	linux = {
		x86_64 = { target = "x86_64-unknown-linux-gnu", ext = "so" },
		aarch64 = { target = "aarch64-unknown-linux-gnu", ext = "so" },
	},
	darwin = {
		x86_64 = { target = "x86_64-apple-darwin", ext = "dylib" },
		arm64 = { target = "aarch64-apple-darwin", ext = "dylib" },
	},
	windows = {
		x86_64 = { target = "x86_64-pc-windows-msvc", ext = "dll" },
	},
}
```

Add the arch normalizer above `get_platform_info`, carrying the existing explanatory comment verbatim:

```lua
-- Fold alternative architecture spellings onto the keys PLATFORM_MAPPINGS uses.
--
-- macOS's own `uname -m` already reports "arm64", so the darwin remap is a
-- no-op there in practice; it exists only to tolerate a uname variant that
-- reports "aarch64" instead. It is scoped to darwin because
-- PLATFORM_MAPPINGS.linux is keyed "aarch64" (Linux's own uname -m spelling) —
-- applying it unconditionally made Linux aarch64 unreachable: it got remapped
-- to "arm64", which is not a key in the linux table, and the lookup failed with
-- "Unsupported platform: linux-arm64".
local function normalize_arch(os_name, arch)
	if arch == "amd64" then
		arch = "x86_64"
	end
	if os_name == "darwin" and arch == "aarch64" then
		arch = "arm64"
	end
	return arch
end
```

Then rewrite the head of `get_platform_info` so one `uv.os_uname()` call serves both fields, and delete the inline table and the two remap blocks it replaces:

```lua
local function get_platform_info()
	local uname = uv.os_uname()
	local os_name = normalize_os_name(uname.sysname:lower())
	local arch = normalize_arch(os_name, uname.machine:lower())

	local platform = PLATFORM_MAPPINGS[os_name]
	if not platform or not platform[arch] then
```

Leave the rest of the function (the error return and the successful return) untouched.

- [ ] **Step 4: Verify T22**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: green, including `spec_platform.lua`. Its `with_uname` stub returns a fresh table per call, so it passes under either call count — but the count is now one per `get_platform_info`, four fewer per `setup()`.

- [ ] **Step 5: Commit T22**

```bash
todo-parser TIDY.md --strip T22
git add lua/time-tracking-nvim/init.lua
git commit -m 'tidy(opportunistic): one uname call and a module-level platform table [T22]'
```

- [ ] **Step 6: T27 — route the prefix through one helper**

Add beside `echo` (immediately below it):

```lua
-- Echo with the plugin's name prefix already attached.
--
-- The prefix chunk was hand-written at 21 call sites; centralising it keeps
-- them from drifting apart and makes the highlight group a single decision.
local function notify(hl, chunks, opts)
	local out = { { "time-tracking-nvim: ", hl } }
	for _, chunk in ipairs(chunks) do
		out[#out + 1] = chunk
	end
	echo(out, opts)
end
```

Then rewrite each of the 21 call sites. They currently read like:

```lua
	echo({
		{ "time-tracking-nvim: ", "WarningMsg" },
		{ "some message", "String" },
	})
```

and become:

```lua
	notify("WarningMsg", {
		{ "some message", "String" },
	})
```

Find them all with `grep -n '"time-tracking-nvim: "' lua/time-tracking-nvim/init.lua` — expect 21. Preserve each site's highlight group and any `opts` third argument (`{ transient = true }`) exactly. **Do not add a prefix-override parameter**: the `"time-tracking-nvim test: "` variant that would have needed one no longer exists.

- [ ] **Step 7: Verify and commit T27**

```bash
grep -c '"time-tracking-nvim: "' lua/time-tracking-nvim/init.lua   # expect 1, inside notify
./integration_tests/lua/run_lua_tests.sh
todo-parser TIDY.md --strip T27
git add lua/time-tracking-nvim/init.lua
git commit -m 'tidy(duplication): route the message prefix through one notify helper [T27]'
```

---

### Task 6: health.lua M.check (T7 — characterization tests, then decomposition)

**Files:**
- Create: `integration_tests/lua/spec_health.lua` (characterization tests)
- Modify: `integration_tests/lua/run_lua_tests.sh:14` (register the new spec in the explicit list)
- Modify: `lua/time-tracking-nvim/health.lua` — `M.check` (~:20, 119 lines)

**Interfaces:**
- Consumes: nothing.
- Produces: `M.check` decomposed into seven local helpers. Task 7 edits a comment in this file afterwards.

This is the only `risk: high` finding in the batch. **The tests come first and land in their own commit** — that is the per-task contract for high-risk findings, and it is what makes the refactor verifiable rather than hopeful.

- [ ] **Step 1: T7a — read the current function end to end**

Read `lua/time-tracking-nvim/health.lua` in full. Note the seven probe sections (Platform, Binary, Versions, cpath, Load, Commands, External tools) and — critically — which of them `return` early on failure. The decomposition must preserve exactly that early-abort behaviour: with no supported platform or no readable library, the later checks would only restate the same problem.

- [ ] **Step 2: T7a — write characterization tests**

Follow the existing harness style in `integration_tests/lua/` — see `spec_platform.lua` and `spec_setup.lua` for how they stub the `_internal` seam, and `harness.lua` for the assertion helpers. You will also need to stub `vim.health` itself, which no existing spec does; record the calls it receives so you can assert on their order.

**`run_lua_tests.sh` does not glob for specs — it iterates an explicit list at line 14.** Add `'spec_health'` to that list, or your new file will silently never run:

```bash
    for _, spec in ipairs({ 'spec_version', 'spec_platform', 'spec_download_url', 'spec_install', 'spec_setup', 'spec_download', 'spec_health' }) do
```

Cover, at minimum:

1. Happy path: every probe reports and `health.ok` is called for each section.
2. No platform: `health.error` is called once and **no later section reports** — the early return.
3. No binary: reports the binary error and again stops before the version/cpath/load sections.
4. Version mismatch: reports a warning but continues to the later sections.
5. Native module fails to load: reports and continues to commands/external tools.

Assert on the *sequence* of `health.*` calls, not just that one happened — the ordering is the behaviour the decomposition most easily breaks.

- [ ] **Step 3: T7a — confirm the tests pass against the UNCHANGED function**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: PASS. If any test fails here, the test encodes an assumption the current code does not hold — fix the test, not `health.lua`. These tests must be green *before* any refactor, or they characterize nothing.

- [ ] **Step 4: T7a — commit the tests alone**

```bash
git add integration_tests/lua/spec_health.lua integration_tests/lua/run_lua_tests.sh
git commit -m 'test: characterize M.check before tidy [T7]'
```

Do **not** strip T7 yet — the finding is not fixed until Step 6.

- [ ] **Step 5: T7b — decompose**

Promote each section comment to a local helper above `M.check`, using `nil` returns as the abort signal the current early returns express:

```lua
local function check_platform(internal) end      -- returns platform_info, or nil after reporting
local function check_binary(internal) end        -- returns binary_path, or nil after reporting
local function check_versions(internal, binary_path) end
local function check_cpath(internal) end
local function check_native_module() end
local function check_commands() end
local function check_external_tools() end
```

`check_binary` covers both the filereadable and the fs_stat sections — they are one concern. Note the helpers can call `internal.*` directly (`internal.get_binary_path()`, `internal.read_binary_version()`, `internal.plugin_root()`, `internal.load_native()`), so they need fewer parameters than the pre-`_internal` shape suggested; take only what each actually uses.

`M.check` then becomes roughly:

```lua
function M.check()
	health.start("time-tracking-nvim")

	local tt = require("time-tracking-nvim")
	local internal = tt._internal or {}

	local platform_info = check_platform(internal)
	if not platform_info then
		return
	end

	local binary_path = check_binary(internal)
	if not binary_path then
		return
	end

	check_versions(internal, binary_path)
	check_cpath(internal)
	check_native_module()
	check_commands()
	check_external_tools()
end
```

Move each section's existing reporting calls and comments into its helper verbatim. This is a reorganization, not a rewrite: no message text, highlight, or advice list should change.

- [ ] **Step 6: T7b — verify against the characterization tests and commit**

```bash
./integration_tests/lua/run_lua_tests.sh
todo-parser TIDY.md --strip T7
git add lua/time-tracking-nvim/health.lua
git commit -m 'tidy(long-methods): decompose M.check into seven probe helpers [T7]'
```

Expected: PASS — the same tests, unchanged, still green. A failure here means the decomposition changed behaviour; fix `health.lua`, never the tests.

---

### Task 7: cross-language mapping and CI dedup (T23 → T6)

**Files:**
- Create: `scripts/versions.sh`
- Modify: `build.sh:15-33` and `build.sh:47`, `.github/workflows/release.yml` (matrix ~:48-64, cp step ~:89, version-check ~:24-39), `.github/workflows/ci.yml` (version-sync ~:106-122), `lua/time-tracking-nvim/health.lua` (supported-platforms hint)

**Interfaces:**
- Consumes: Task 5's `PLATFORM_MAPPINGS`, Task 6's decomposed `health.lua`.
- Produces: `scripts/versions.sh` exporting `cargo_version` and `lua_version`.

- [ ] **Step 1: T23 — derive `nvim_name` instead of listing it**

In `.github/workflows/release.yml`, delete the `nvim_name:` line from all four matrix entries, leaving:

```yaml
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            lib_name: libtime_tracking_nvim.so
          - target: x86_64-apple-darwin
            os: macos-latest
            lib_name: libtime_tracking_nvim.dylib
          - target: aarch64-apple-darwin
            os: macos-latest
            lib_name: libtime_tracking_nvim.dylib
          - target: x86_64-pc-windows-msvc
            os: windows-latest
            lib_name: time_tracking_nvim.dll
```

Then derive it in the `Create plugin structure` step:

```yaml
      - name: Create plugin structure
        shell: bash
        run: |
          mkdir -p plugin-structure/target/release

          # Neovim requires the library without the `lib` prefix; the matrix
          # carries only the built name, and this strips it.
          nvim_name="$(basename '${{ matrix.lib_name }}' | sed 's/^lib//')"

          cp target/${{ matrix.target }}/release/${{ matrix.lib_name }} "plugin-structure/target/release/${nvim_name}"
```

- [ ] **Step 2: T23 — derive `LIB_NAME` in build.sh**

Replace the `case` block so each arm sets only what varies:

```bash
OS="$(uname -s)"
case "${OS}" in
    Linux*)
        LIB_EXT="so"
        LIB_PREFIX="lib"
        ;;
    Darwin*)
        LIB_EXT="dylib"
        LIB_PREFIX="lib"
        ;;
    CYGWIN*|MINGW32*|MSYS*|MINGW*)
        LIB_EXT="dll"
        LIB_PREFIX=""
        ;;
    *)
        echo "❌ Unsupported platform: ${OS}"
        exit 1
        ;;
esac
LIB_NAME="${LIB_PREFIX}time_tracking_nvim.${LIB_EXT}"
```

- [ ] **Step 3: T23 — add the cross-reference comments**

Add a comment at each of the four sites naming the other three, so a new target cannot be added to one and forgotten in the rest. The four sites are: `PLATFORM_MAPPINGS` in `lua/time-tracking-nvim/init.lua`, the `case` in `build.sh`, the matrix `include:` in `release.yml`, and the supported-platforms advice string in `health.lua` (re-grep for `"Supported: Linux x86_64"` — Task 6 moved it).

Do **not** attempt to unify the three mappings into one source. The Lua table must be readable inside Neovim with no shell, `build.sh` runs before any artifact exists, and the CI matrix must be static YAML; there is no shared source without a codegen step this project does not use.

- [ ] **Step 4: Verify and commit T23**

```bash
bash -n build.sh
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"
./build.sh
./integration_tests/lua/run_lua_tests.sh
todo-parser TIDY.md --strip T23
git add build.sh .github/workflows/release.yml lua/time-tracking-nvim/init.lua lua/time-tracking-nvim/health.lua
git commit -m 'tidy(duplication): derive the Neovim library name instead of listing it [T23]'
```

Expected: `./build.sh` still produces `lua/time_tracking_nvim.so` and stamps its `.version` file.

- [ ] **Step 5: T6 — extract the shared version reader**

Create `scripts/versions.sh`:

```bash
#!/usr/bin/env bash
#
# Single source for the two versions that must agree: Cargo.toml's package
# version and the PLUGIN_VERSION constant in the Lua loader.
#
# Sourced, not executed — it sets `cargo_version` and `lua_version` in the
# caller's shell. Callers run from the repo root.

cargo_version="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
lua_version="$(grep -m1 'PLUGIN_VERSION = ' lua/time-tracking-nvim/init.lua | sed -E 's/.*"([^"]+)".*/\1/')"
```

Make it executable (`chmod +x scripts/versions.sh`) and source it from `build.sh`, replacing the inline `CARGO_VERSION=` line:

```bash
    # shellcheck source=scripts/versions.sh
    . ./scripts/versions.sh
    printf '%s\n' "${cargo_version}" > "lua/time_tracking_nvim.${LIB_EXT}.version"
    echo "🏷  Stamped version: ${cargo_version}"
```

- [ ] **Step 6: T6 — collapse the two near-clone CI jobs**

Give `ci.yml`'s `version-sync` job a `workflow_call` trigger so `release.yml` can reuse it instead of restating it. In `ci.yml`, add to the top-level `on:`:

```yaml
  workflow_call:
    inputs:
      expected_tag:
        description: Tag version the sources must also match (release only)
        required: false
        type: string
```

Rewrite the job's script to source the shared file and apply the tag comparison only when the input is present:

```yaml
      - name: Check Cargo.toml and init.lua agree
        run: |
          . ./scripts/versions.sh
          echo "Cargo.toml: ${cargo_version}"
          echo "init.lua:   ${lua_version}"
          if [ "${cargo_version}" != "${lua_version}" ]; then
            echo "::error::PLUGIN_VERSION (${lua_version}) does not match Cargo.toml version (${cargo_version})"
            exit 1
          fi
          expected="${{ inputs.expected_tag }}"
          if [ -n "${expected}" ] && [ "${cargo_version}" != "${expected#v}" ]; then
            echo "::error::version mismatch — tag ${expected#v}, Cargo.toml ${cargo_version}, init.lua ${lua_version}"
            exit 1
          fi
```

In `release.yml`, replace the whole `version-check` job body with a call to it:

```yaml
  version-check:
    name: Version check
    uses: ./.github/workflows/ci.yml
    with:
      expected_tag: ${{ github.event_name == 'workflow_dispatch' && github.event.inputs.tag || github.ref_name }}
```

**Caution:** `uses:` on a job runs the *entire* called workflow, not one job of it. If reusing `ci.yml` wholesale would make every release also run the full test matrix and that is unwanted, stop and take the alternative the finding names instead — a composite action under `.github/actions/version-check` invoked as a step from both jobs. Either satisfies T6; pick one, and say in the commit body which and why.

- [ ] **Step 7: Verify and commit T6**

```bash
bash -n scripts/versions.sh build.sh
( . ./scripts/versions.sh && echo "cargo=${cargo_version} lua=${lua_version}" )   # both must be 0.2.1
./build.sh
python3 -c "import yaml; [yaml.safe_load(open(f)) for f in ('.github/workflows/ci.yml', '.github/workflows/release.yml')]"
todo-parser TIDY.md --strip T6
git add scripts/versions.sh build.sh .github/workflows/ci.yml .github/workflows/release.yml
git commit -m 'tidy(duplication): share the version-extraction snippet and collapse the twin CI jobs [T6]'
```

CI workflow changes cannot be fully verified locally. Say so plainly in the commit body and flag it in the task report.

---

### Task 8: Neovim version floor (T30)

**Files:**
- Modify: `plugin/time-tracking-nvim.vim:10-12`, `README.md:122`

**Interfaces:**
- Consumes: nothing.
- Produces: nothing.

- [ ] **Step 1: T30 — correct the guard**

In `plugin/time-tracking-nvim.vim`, replace:

```vim
if !has('nvim-0.11')
  echoerr 'time-tracking-nvim requires Neovim 0.11 or later'
  finish
endif
```

with:

```vim
if !has('nvim-0.12')
  echohl WarningMsg
  echomsg 'time-tracking-nvim requires Neovim 0.12 or later'
  echohl None
  finish
endif
```

`echoerr` at plugin-sourcing scope throws, which is heavier than a version notice warrants; `echomsg` under `WarningMsg` says the same thing without aborting the sourcing of unrelated plugins.

- [ ] **Step 2: T30 — correct the README**

In `README.md`, change the requirements bullet:

```markdown
- Neovim 0.11+
```

to:

```markdown
- Neovim 0.12+
```

Note the current line has a trailing space; drop it while you are there.

- [ ] **Step 3: Verify and commit T30**

```bash
grep -n "nvim-0\.12" plugin/time-tracking-nvim.vim
grep -n "Neovim 0\.12" README.md
grep -rn "0\.11" README.md plugin/ CLAUDE.md   # expect no surviving 0.11 claim
todo-parser TIDY.md --strip T30
git add plugin/time-tracking-nvim.vim README.md
git commit -m 'tidy(idioms): raise the declared Neovim floor to 0.12, matching the build [T30]'
```

The real floor is `Cargo.toml`'s `features = ["neovim-0-12"]`. Before this change the guard and the README agreed with each other and both disagreed with the build, so a 0.11 user was told twice they were supported and then failed at `dlopen`.

---

### Task 9: Final verification

**Files:** none modified.

- [ ] **Step 1: Confirm every finding was stripped**

```bash
todo-parser TIDY.md --summary
```

Expected: `marked execute: 0`, 5 active unmarked items remaining, 1 archived. If any of the 17 is still marked execute, its commit did not run the strip — find it and fix the bookkeeping.

- [ ] **Step 2: Run everything**

```bash
env -u CARGO_TARGET_DIR cargo build
cargo fmt --all -- --check
env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings
( cd integration_tests && env -u CARGO_TARGET_DIR cargo fmt -- --check )
( cd integration_tests && env -u CARGO_TARGET_DIR cargo clippy --all-targets -- -D warnings )
( cd integration_tests && env -u CARGO_TARGET_DIR cargo test )
./integration_tests/lua/run_lua_tests.sh
./build.sh
```

Expected: all green, 52 Rust integration tests plus the Lua specs (now including `spec_health.lua`).

- [ ] **Step 3: Confirm the commit shape**

```bash
git log --oneline main..HEAD
```

Expected: 18 commits after the spec commit — one per finding, plus the separate `test: characterize M.check before tidy [T7]`. No commit should carry two `[T<n>]` tags.

---

## Self-review

**Spec coverage.** All 17 selected findings have a task: T31/T35/T34 → Task 1; T36/T40/T41 → Task 2; T37/T38 → Task 3; T42/T43 → Task 4; T21/T22/T27 → Task 5; T7 → Task 6 (two commits, tests first per its `risk: high`); T23/T6 → Task 7; T30 → Task 8. The spec's four invariants are enforced: the test seam survives (Task 3 Step 6 forbids removing it), per-tabpage `find_preview` semantics are called out in Task 3 Step 4, the throttle render path is guarded by the 52-test suite at every step of Task 3, and `integration_tests`' fmt/clippy gates are in the Global Constraints.

**Placeholder scan.** Every code step carries the literal replacement text. The one deliberately open decision is Task 7 Step 6's `workflow_call`-vs-composite-action choice, which is a genuine fork the finding itself names; the step gives both routes, the criterion for choosing, and requires the choice be recorded in the commit body.

**Type consistency.** `create_or_update_preview(output: &str)` keeps its public one-argument signature throughout (integration tests call it directly); the new inner function is `create_or_update_preview_with(found, output)`. `render_current_buffer` gains a second parameter in Task 3 and is not referenced by any earlier task. `preview_is_open` is deleted and replaced by `preview_is_open_in(&found) -> bool` in the same step, so no task references the removed name. `write_preview_contents_with`'s callback is `fn(&mut Buffer, Vec<&str>) -> Result<()>` in both the production change (Task 3 Step 6) and the test update (Step 7). `is_buf_time_tracking_file(&Buffer, &Config)` and `is_win_time_tracking_file(&Window, &Config)` are used consistently in Task 4. `notify(hl, chunks, opts)` in Task 5 matches its call sites. `PLATFORM_MAPPINGS` is introduced in Task 5 and referenced by Task 7.
