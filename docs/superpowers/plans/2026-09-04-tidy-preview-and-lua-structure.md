# Tidy: preview/lib structure, Lua setup decomposition, folded bug fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute the 17 `TIDY.md` items marked for this batch plus 5 `bughunt.md` findings folded in by user decision, one commit per finding, leaving the tree green at every step.

**Architecture:** Four independent chains. Within `src/preview.rs` the bug fixes land *first*, inside the monolithic `create_or_update_preview`, and the structural extraction happens afterwards on already-correct code — that ordering is the whole reason the bugs were folded in rather than deferred. `src/lib.rs` merges a tidy dedup with a bug fix that would otherwise conflict with it. The Lua chain deletes dead code first to shrink the surface the later extractions must carry.

**Tech Stack:** Rust (nvim-oxi, cdylib), Lua (Neovim runtime), GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-04-tidy-preview-and-lua-structure-design.md`

## Global Constraints

- Branch is `tidy/2026-09-04`. Never commit to `main`.
- **One commit per finding.** Code change + findings-file strip in the same commit. Never bulk-commit.
- Commit message: `tidy(<lens>): <summary> [T<n>]` for TIDY items, `fix(<area>): <summary> [B<n>]` for bughunt items.
- Every commit message ends with the trailer `Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs`
- Strip on fix: `todo-parser TIDY.md --strip T<n>` or `todo-parser bughunt.md --strip B<n>`, staged in the same commit.
- After every Rust task: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`. All must be clean.
- After every Lua task: re-read the edit; `luajit -bl <file> >/dev/null` if available.
- Never `--no-verify`, never `--allow-dirty`.
- **All seven `TimeTracking*` command names must survive byte-identical.** Pinned by `test_time_tracking_with_config_creates_commands`.
- **`create_or_update_preview`'s public signature must not change** — ~10 integration tests call it.
- Renames and public-API signature changes are **not** in scope. If a task appears to need one, stop and report.
- Milestone: after every 5 tasks, run the full suite — `cargo test`, `./integration_tests/run_tests.sh`, `integration_tests/lua/run_lua_tests.sh`.

---

# Chain A — src/preview.rs (bugs before structure)

Strict order: Task 1 → 7. Each depends on its predecessor's shape.

### Task 1: B45 — per-tabpage preview visibility

**Files:**
- Modify: `src/preview.rs` (add helper near `find_preview` at :66; call sites at :92, :237, :265, :483)
- Test: `integration_tests/src/lib.rs` (existing preview tests must stay green)

**Interfaces:**
- Produces: `fn preview_win_in_current_tab(buf: &Buffer) -> Result<Option<Window>>` — used by Task 2.

**Context:** `find_preview()` at `src/preview.rs:66` scans `api::list_wins()` (line 92), which enumerates windows in *all* tabpages. With the preview open in tab 1, opening a tracking file in tab 2 finds the tab-1 window, concludes the preview is open, and opens nothing.

- [ ] **Step 1: Add the helper**

In `src/preview.rs`, directly above `fn find_preview()`:

```rust
/// The window in the *current tabpage* showing `buf`, if any.
///
/// Deliberately not `api::list_wins()`: that enumerates every tabpage, so a
/// preview open in tab 1 would count as "already open" for tab 2 and the second
/// tab would never get its own preview split.
fn preview_win_in_current_tab(buf: &Buffer) -> Result<Option<Window>> {
    for w in api::get_current_tabpage().list_wins()? {
        if &w.get_buf()? == buf {
            return Ok(Some(w));
        }
    }
    Ok(None)
}
```

- [ ] **Step 2: Use it in `find_preview`**

Replace the window-scan loop in `find_preview` (currently `src/preview.rs:91-97`):

```rust
    let window = preview_win_in_current_tab(&buf)?;

    Ok(Some((buf, window)))
```

- [ ] **Step 3: Verify `close_preview` stays global**

`close_preview` at `src/preview.rs:413` and the `VimLeavePre` cleanup must keep closing the preview wherever it lives. `close_preview` calls `find_preview()`, which is now tab-scoped — that is correct: `:TimeTrackingClose` in tab 2 should not close tab 1's preview. Leave `api::list_wins().count()` at :420 alone; the last-window check is genuinely global. Confirm by reading, no edit.

- [ ] **Step 4: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean.

- [ ] **Step 5: Run the preview integration tests**

Run: `./integration_tests/run_tests.sh`
Expected: PASS. These run single-tabpage, so behaviour is unchanged for them.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B45
git add src/preview.rs bughunt.md
git commit -m "fix(preview): scope preview visibility to the current tabpage [B45]

nvim_list_wins enumerates every tabpage, so a preview open in tab 1 made
auto-open conclude the preview already existed for tab 2 and open nothing.
Scan the current tabpage instead. close_preview stays global.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 2: T13 — extract the render pipeline and the open-preview probe

**Files:**
- Modify: `src/preview.rs` (call sites at :237, :249, :265, :277, :483, :489)

**Interfaces:**
- Consumes: `preview_win_in_current_tab` (Task 1).
- Produces: `fn preview_is_open() -> Result<bool>`, `fn render_current_buffer(config: &Config) -> Result<()>` — used by Task 7.

**Context:** The four-line read/format/write pipeline appears verbatim in `toggle_preview_fn` (:243-250), `update_preview_fn` (:268-275) and `auto_open_preview_impl` (:484-491). The `matches!(find_preview()?, Some((_, Some(_))))` probe appears at :237, :265 and :483. After Task 1 that probe is tab-scoped, so wrapping it now captures the corrected behaviour in one place.

- [ ] **Step 1: Add both helpers**

In `src/preview.rs`, directly above `pub fn toggle_preview_fn` (:214):

```rust
/// Is a window in the current tabpage showing the preview?
fn preview_is_open() -> Result<bool> {
    Ok(matches!(find_preview()?, Some((_, Some(_)))))
}

/// Render the current buffer's day summary into the preview.
///
/// The single read-format-write path: every entry point that shows tracking
/// data goes through here, so the formatter arguments are specified once.
fn render_current_buffer(config: &Config) -> Result<()> {
    let buffer_content = get_buffer_content()?;
    let formatted_output = config.get_formatter().day_summary(
        &buffer_content,
        "",
        config.get_prefix(),
        config.get_suffix(),
    );
    create_or_update_preview(&formatted_output)
}
```

- [ ] **Step 2: Rewrite the three call sites**

In `toggle_preview_fn`, replace the `let has_preview = ...` line and the `if/else` body with:

```rust
    if preview_is_open()? {
        close_preview()?;
    } else {
        render_current_buffer(config)?;
    }

    Ok(())
```

In `update_preview_fn`, replace its body after the tracking-file guard with:

```rust
    if preview_is_open()? {
        render_current_buffer(config)?;
    }

    Ok(())
```

In `auto_open_preview_impl`, replace its `has_preview` block with:

```rust
    if !preview_is_open()? {
        render_current_buffer(config)?;
    }

    Ok(())
```

- [ ] **Step 3: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean.

- [ ] **Step 4: Run the integration tests**

Run: `./integration_tests/run_tests.sh`
Expected: PASS — specifically `test_explicit_update_renders_immediately`, `test_toggle_outside_data_dir_creates_no_preview_and_returns_ok`, `test_multiple_preview_creation_updates_same_buffer`.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser TIDY.md --strip T13
git add src/preview.rs TIDY.md
git commit -m "tidy(duplication): extract render_current_buffer and preview_is_open [T13]

The read-format-write pipeline was written out three times and the
open-preview probe three more. Both now have one definition.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 3: B37 — restore `modifiable` even when the write fails

**Files:**
- Modify: `src/preview.rs:322-329`

**Context:** The dirty-checked write sets `modifiable=true`, calls `set_lines(...)?`, then sets `modifiable=false`. The `?` on `set_lines` returns early on failure, so `modifiable` is left **true** permanently — the user can then type into the preview and their edits are silently discarded on the next render.

- [ ] **Step 1: Capture the result and restore unconditionally**

Replace the body of the `if !last_output_matches(output)` block (`src/preview.rs:322-329`) with:

```rust
    if !last_output_matches(output) {
        let bopts = OptionOptsBuilder::default().buf(buf.clone()).build();
        api::set_option_value("modifiable", true, &bopts)?;
        let lines: Vec<String> = output.lines().map(|s| s.to_string()).collect();
        let write = buf.set_lines(0..buf.line_count()?, false, lines);
        // Restore before propagating: an early `?` here would leave the preview
        // permanently modifiable, so the user could type into it and lose the
        // edits on the next render.
        api::set_option_value("modifiable", false, &bopts)?;
        write?;
        set_last_output(Some(output.to_owned()));
    }
```

- [ ] **Step 2: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean.

- [ ] **Step 3: Run the integration tests**

Run: `./integration_tests/run_tests.sh`
Expected: PASS. `set_last_output` is now only reached on a successful write, which is the correct invariant — `LAST_OUTPUT` documents itself as tracking what was last *written*.

- [ ] **Step 4: Strip and commit**

```bash
todo-parser bughunt.md --strip B37
git add src/preview.rs bughunt.md
git commit -m "fix(preview): restore nomodifiable when the line write fails [B37]

set_lines(...)? returned early with modifiable still true, leaving the
preview editable for the rest of the session. Restore the option before
propagating, and only record LAST_OUTPUT on a write that succeeded.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 4: B44 — stop discarding window-layout errors

**Files:**
- Modify: `src/preview.rs:368, 374, 379-386, 397`

**Context:** `let _ = win.close(false)` (:368), `let _ = api::set_option_value("winfixwidth", ...)` (:374), the eight styling `let _ =` calls (:379-386) and `let _ = win.set_width(width)` (:397) all discard failures. When the layout goes wrong the user sees a squeezed or unstyled split with nothing recorded anywhere.

Note: `wincmd p` at :401 is **not** touched here — Task 5 replaces that line entirely.

- [ ] **Step 1: Log the close failure**

Replace `src/preview.rs:368`:

```rust
            if let Err(close_err) = win.close(false) {
                debug_log!("[ttnvim] failed to close orphan split: {}\n", close_err);
            }
```

- [ ] **Step 2: Log the width failures**

Replace the `winfixwidth` line (:374):

```rust
        if let Err(e) = api::set_option_value("winfixwidth", true, &wopts) {
            debug_log!("[ttnvim] could not pin preview width: {}\n", e);
        }
```

Replace the `set_width` line (:397):

```rust
            if let Err(e) = win.set_width(width) {
                debug_log!("[ttnvim] could not set preview width: {}\n", e);
            }
```

- [ ] **Step 3: Log the styling failures as a group**

The eight styling calls are cosmetic and share one failure mode, so collapse them into a loop rather than eight `if let Err` blocks. Replace `src/preview.rs:379-386`:

```rust
        // Cosmetic only — a failure costs the user some visual noise in the
        // preview, never correctness, so one debug line for the group is enough.
        for (name, value) in [
            ("number", false.into()),
            ("relativenumber", false.into()),
            ("wrap", false.into()),
            ("cursorline", false.into()),
            ("spell", false.into()),
            ("list", false.into()),
        ] {
            if let Err(e) = api::set_option_value::<nvim_oxi::Object>(name, value, &wopts) {
                debug_log!("[ttnvim] could not style preview ({}): {}\n", name, e);
            }
        }
        if let Err(e) = api::set_option_value("signcolumn", "no", &wopts) {
            debug_log!("[ttnvim] could not style preview (signcolumn): {}\n", e);
        }
        if let Err(e) = api::set_option_value("foldcolumn", "0", &wopts) {
            debug_log!("[ttnvim] could not style preview (foldcolumn): {}\n", e);
        }
```

If the `Object` conversion above does not typecheck against the pinned nvim-oxi, fall back to eight explicit `if let Err(e) = ... { debug_log!(...) }` blocks — the loop is a readability preference, not a requirement. Do not leave any `let _ =` behind.

- [ ] **Step 4: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean.

- [ ] **Step 5: Run the integration tests**

Run: `./integration_tests/run_tests.sh`
Expected: PASS — all these paths were previously silent and remain non-fatal.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B44
git add src/preview.rs bughunt.md
git commit -m "fix(preview): record window-layout failures instead of discarding them [B44]

Eleven layout calls were dropped with let _, so a squeezed or unstyled
preview split had no diagnostic anywhere. Route them through debug_log!,
recoverable under TIME_TRACKING_DEBUG=1 without adding default noise.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 5: B39 — restore focus by handle, not `wincmd p`

**Files:**
- Modify: `src/preview.rs:336` (capture) and `:401` (restore)

**Context:** After creating the split the code runs `wincmd p` to return focus. That jumps to Vim's *previous-window* pointer, which the split itself has just repointed — so it lands correctly by accident, and clobbers the user's own previous-window target as a side effect. Saving the handle is deterministic and leaves the pointer alone.

- [ ] **Step 1: Capture the origin window before splitting**

`src/preview.rs:336` currently reads:

```rust
        let source_width = api::get_current_win().get_width().unwrap_or(u32::MAX);
```

Replace with:

```rust
        let origin = api::get_current_win();
        let source_width = origin.get_width().unwrap_or(u32::MAX);
```

- [ ] **Step 2: Restore by handle**

Replace `src/preview.rs:401` (`let _ = api::command("wincmd p");`) with:

```rust
        // Restore focus by handle rather than `wincmd p`: the split has already
        // repointed Vim's previous-window pointer, so `wincmd p` only lands
        // correctly by accident — and it overwrites the user's own previous-window
        // target on the way. This changes where the cursor ends up, so a failure
        // is user-visible and warrants more than a debug line.
        if let Err(e) = api::set_current_win(&origin) {
            log_error!(
                "[time-tracking-nvim] could not return focus after opening the preview: {}",
                e
            );
        }
```

- [ ] **Step 3: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean. If `api::set_current_win` takes the window by value in the pinned nvim-oxi, pass `origin.clone()` and keep `origin` for nothing else.

- [ ] **Step 4: Run the integration tests**

Run: `./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser bughunt.md --strip B39
git add src/preview.rs bughunt.md
git commit -m "fix(preview): return focus by window handle, not wincmd p [B39]

wincmd p follows the previous-window pointer the split just repointed, so
it landed correctly only by accident and clobbered the user's own
previous-window target. Save the handle before splitting and restore it.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 6: T15 — name the window-geometry constants

**Files:**
- Modify: `src/preview.rs` (const block near :104; uses at :342, :395-396)

**Interfaces:**
- Produces: `MIN_SPLIT_COLUMNS`, `PREVIEW_SCREEN_FRACTION`, `MIN_PREVIEW_COLUMNS` — used by Task 7.

- [ ] **Step 1: Add the constants**

In `src/preview.rs`, immediately after the `DEBOUNCE` const block (around :104), outside any `#[cfg]`:

```rust
/// Below this width a vertical split fails outright with E36 and damages the
/// layout on the way out, so no preview is the better outcome.
const MIN_SPLIT_COLUMNS: u32 = 40;

/// The preview aims for this fraction of the total screen width.
const PREVIEW_SCREEN_FRACTION: i64 = 3;

/// Floor for the preview, and the minimum width left to the window it split from.
const MIN_PREVIEW_COLUMNS: u32 = 20;
```

- [ ] **Step 2: Use them**

`src/preview.rs:342` becomes:

```rust
        if source_width < MIN_SPLIT_COLUMNS {
```

`src/preview.rs:395-396` becomes:

```rust
            let one_third =
                u32::try_from(total_cols / PREVIEW_SCREEN_FRACTION).unwrap_or(u32::MAX);
            let width = one_third
                .min(source_width.saturating_sub(MIN_PREVIEW_COLUMNS))
                .max(MIN_PREVIEW_COLUMNS);
```

The `u32::try_from` also removes the lossy `as u32` cast and subsumes the old `.max(0)` — a negative quotient becomes `Err` and falls back to `u32::MAX`, which the subsequent `.min(...)` clamps.

- [ ] **Step 3: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean.

- [ ] **Step 4: Strip and commit**

```bash
todo-parser TIDY.md --strip T15
git add src/preview.rs TIDY.md
git commit -m "tidy(idioms): name the preview window-geometry constants [T15]

40, 3 and 20 were inline while the sibling debounce interval was already a
named const. Also replaces the lossy 'as u32' with u32::try_from.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 7: T14 — split `create_or_update_preview`

**Files:**
- Modify: `src/preview.rs:281-406` (the function) and `:283`, `:420` (emptiness idiom)

**Interfaces:**
- Consumes: everything from Tasks 1-6. This task extracts code that is already correct.
- Produces: `create_preview_buffer`, `write_preview_contents`, `open_preview_split`, `style_preview_window`.

**Context:** `create_or_update_preview` does four jobs in 125 lines: startup bail, buffer resolve-or-create, dirty-checked write, split creation + styling. Its public signature is consumed by ~10 integration tests and **must not change**.

- [ ] **Step 1: Extract `create_preview_buffer`**

Add above `pub fn create_or_update_preview`:

```rust
/// Create the scratch buffer that backs the preview, and prime both caches.
fn create_preview_buffer() -> Result<Buffer> {
    let mut b = api::create_buf(false, true)?; // listed=false, scratch=true
    b.set_name("[Time Tracking Preview]")?;

    // Keep it unlisted and non-modifiable by default (DO NOT set 'readonly')
    let bopts = OptionOptsBuilder::default().buf(b.clone()).build();
    api::set_option_value("buflisted", false, &bopts)?;
    api::set_option_value("modifiable", false, &bopts)?;
    api::set_option_value("bufhidden", "wipe", &bopts)?;
    api::set_option_value("swapfile", false, &bopts)?;
    set_cached_preview_buf(Some(b.clone()));
    set_last_output(None);
    Ok(b)
}
```

- [ ] **Step 2: Extract `write_preview_contents`**

Carry the full `LAST_OUTPUT` invariant comment and the B37 restore-before-propagate comment onto the helper — they explain this code, not its caller:

```rust
/// Write `output` into the preview buffer, skipping the rewrite when nothing
/// changed.
///
/// The rendered day summary is unchanged for most keystrokes, and rewriting
/// yanks the preview's scroll position and repaints the whole split.
///
/// No `buf.is_valid()` check: the caller passes either a buffer just created by
/// [`create_preview_buffer`] or one from `find_preview`'s cache, which
/// revalidates before returning — so it is always valid here.
fn write_preview_contents(buf: &mut Buffer, output: &str) -> Result<()> {
    if last_output_matches(output) {
        return Ok(());
    }

    let bopts = OptionOptsBuilder::default().buf(buf.clone()).build();
    api::set_option_value("modifiable", true, &bopts)?;
    let lines: Vec<String> = output.lines().map(|s| s.to_string()).collect();
    let write = buf.set_lines(0..buf.line_count()?, false, lines);
    // Restore before propagating: an early `?` here would leave the preview
    // permanently modifiable, so the user could type into it and lose the edits
    // on the next render.
    api::set_option_value("modifiable", false, &bopts)?;
    write?;
    set_last_output(Some(output.to_owned()));
    Ok(())
}
```

- [ ] **Step 3: Extract `style_preview_window`**

Move the `winfixwidth` call, the styling loop from Task 4, and the width computation from Task 6 into:

```rust
/// Apply the preview's window-local options and width.
///
/// A vsplit copies the source window's local options, so an ordinary
/// `set number relativenumber list signcolumn=yes` config eats 6-8 of the
/// preview's ~26 columns. Style it as the scratch preview it is.
fn style_preview_window(win: &mut Window, source_width: u32) {
    // ... winfixwidth + styling loop from Task 4, verbatim ...
    // ... width computation from Task 6, verbatim ...
}
```

Keep every comment that travelled with those lines. This helper returns `()` — every call inside it already logs its own failure and none are fatal.

- [ ] **Step 4: Extract `open_preview_split`**

```rust
/// Open a vertical split to the right and attach the preview buffer to it.
///
/// Returns `Ok(())` without splitting when the window is too narrow or a window
/// operation is already in progress — a missing preview beats a broken layout.
fn open_preview_split(buf: &Buffer) -> Result<()> {
    // ... the whole current `if !is_open` body, Tasks 4-6 included ...
    // ends with the B39 focus restore
}
```

- [ ] **Step 5: Reduce the outer function**

`create_or_update_preview` becomes:

```rust
/// Create or update the preview window with formatted time tracking data
pub fn create_or_update_preview(output: &str) -> Result<()> {
    // Bail if Neovim has no windows yet (during early startup churn)
    if api::list_wins().next().is_none() {
        return Ok(());
    }

    let (preview, preview_win) = match find_preview()? {
        Some((buf, win)) => (Some(buf), win),
        None => (None, None),
    };

    let mut buf: Buffer = match preview {
        Some(b) => b,
        None => create_preview_buffer()?,
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

- [ ] **Step 6: Make the emptiness idiom consistent**

`src/preview.rs:420` in `close_preview` currently reads `let window_count = api::list_wins().count();` and is compared `== 1`. Leave the count — it genuinely needs the number, not emptiness. Only :283 changes, and Step 5 already did it. Confirm by reading; no second edit.

- [ ] **Step 7: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean.

- [ ] **Step 8: Run the full suite (milestone)**

Run: `cargo test && ./integration_tests/run_tests.sh && integration_tests/lua/run_lua_tests.sh`
Expected: PASS. This is a milestone boundary — 7 tasks in.

- [ ] **Step 9: Strip and commit**

```bash
todo-parser TIDY.md --strip T14
git add src/preview.rs TIDY.md
git commit -m "tidy(long-methods): split create_or_update_preview into four helpers [T14]

125 lines doing four jobs becomes bail, resolve-or-create, write, ensure
shown. The public signature is unchanged. Also switches the startup bail to
next().is_none(), which short-circuits and reads as the emptiness check it is.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

# Chain B — src/lib.rs

### Task 8: T12 + B41 — data-driven command registration with descriptions

**Files:**
- Modify: `src/lib.rs:150-246`
- Test: `integration_tests/src/lib.rs::test_time_tracking_with_config_creates_commands` (do not edit — it must pass unchanged)

**Context:** These two findings were flagged as conflicting: T12 wants one shared `CreateCommandOpts`, B41 wants a distinct `.desc()` and explicit `.nargs()` per command. Folding them resolves the conflict — B41's per-command data becomes T12's table column. Doing either alone would make the other harder.

Six commands are registered with `&CreateCommandOpts::builder().build()`, differing only in name and function. `TimeTrackingMaybeCloseIfInvisible` is separate: it needs `CommandNArgs::ZeroOrOne` and reads `args`.

**All seven names must survive byte-identical.**

- [ ] **Step 1: Confirm the test that pins the names**

Run: `grep -n 'TimeTracking' integration_tests/src/lib.rs | head -20`
Expected: a list of expected command names. Note it — every one must still be registered at the end.

- [ ] **Step 2: Replace the six registrations with a table-driven loop**

In `src/lib.rs`, after the `maybe_close_if_invisible` registration (which stays exactly as it is), replace the six `api::create_user_command(...)` blocks with:

```rust
    // Name, description, handler. The description is what `:command
    // TimeTracking<Tab>` and which-key/telescope pickers show; without it all
    // six rendered as a blank column and were indistinguishable.
    for (name, desc, func) in [
        (
            "TimeTrackingToggle",
            "Toggle the time-tracking preview split",
            toggle_preview,
        ),
        (
            "TimeTrackingUpdate",
            "Re-render the time-tracking preview now",
            update_preview,
        ),
        (
            "TimeTrackingUpdateDebounced",
            "Re-render the preview, coalescing a burst of keystrokes",
            update_preview_debounced_cmd,
        ),
        (
            "TimeTrackingAutoOpen",
            "Open the preview if the current buffer is a tracking file",
            auto_open,
        ),
        (
            "TimeTrackingAutoClose",
            "Close the time-tracking preview",
            auto_close,
        ),
        (
            "TimeTrackingClose",
            "Close the time-tracking preview split",
            close_preview_cmd,
        ),
    ] {
        api::create_user_command(
            name,
            func,
            &CreateCommandOpts::builder()
                .desc(desc)
                .nargs(CommandNArgs::Zero)
                .build(),
        )?;
    }
```

The six `Function<CommandArgs, Result<()>>` values are the same concrete type, so the array typechecks.

- [ ] **Step 3: Add a description to the internal command too**

The `TimeTrackingMaybeCloseIfInvisible` registration keeps its `CommandNArgs::ZeroOrOne` and gains a `.desc()` that marks it internal:

```rust
    api::create_user_command(
        "TimeTrackingMaybeCloseIfInvisible",
        maybe_close_if_invisible,
        &CreateCommandOpts::builder()
            .desc("(internal) Close the preview when no tracking file is visible")
            .nargs(CommandNArgs::ZeroOrOne)
            .build(),
    )?;
```

**Do not rename it.** B41 also suggests renaming it or dropping the command indirection; renames are a disabled category for this pass. The `(internal)` prefix is the in-scope half.

- [ ] **Step 4: Split the registration body**

Extract two private functions so `time_tracking_with_config` reads as register → schedule → return:

```rust
/// Register the `TimeTracking*` user commands.
fn register_commands(config: &'static Config) -> Result<()> {
    // ... every Function::from_fn binding and both registration blocks ...
}

/// Register the `TimeTrackingNvim` autocommand group.
///
/// Issued as Vimscript to avoid an nvim-oxi keyset mask mismatch on 0.12.2+.
fn register_autocommands() -> Result<()> {
    api::command("augroup TimeTrackingNvim")?;
    api::command("autocmd!")?;
    api::command("autocmd BufEnter,TabEnter * TimeTrackingMaybeCloseIfInvisible")?;
    api::command("autocmd WinClosed * TimeTrackingMaybeCloseIfInvisible <amatch>")?;
    api::command("autocmd TextChanged,TextChangedI *.md TimeTrackingUpdateDebounced")?;
    api::command("autocmd VimEnter,BufWinEnter *.md TimeTrackingAutoOpen")?;
    api::command("autocmd VimLeavePre * silent! bwipeout [Time Tracking Preview]")?;
    api::command("autocmd QuitPre * TimeTrackingClose")?;
    api::command("augroup END")?;
    Ok(())
}
```

`time_tracking_with_config` becomes:

```rust
pub fn time_tracking_with_config(config: &'static Config) -> Result<Dictionary> {
    register_commands(config)?;
    register_autocommands()?;

    // Scheduled to delay until startup is complete
    schedule(|_| {
        catch_nvim_panic(|| {
            api::command("TimeTrackingAutoOpen").map_err(|e| {
                log_error!("Issue running auto-open on start-up {:?}", e);
                nvim_oxi::Error::Api(e)
            })
        })
    });

    let api = Dictionary::new();
    Ok(api)
}
```

Leave the `VimLeavePre bwipeout` line exactly as it is — its argument is a broken pattern tracked as bughunt B54, which is not in this batch.

- [ ] **Step 5: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean. If `.desc()` is not available on the pinned nvim-oxi's builder, stop and report — B41 asserts it exists and was verified against the pinned checkout.

- [ ] **Step 6: Verify every command name survived**

Run: `./integration_tests/run_tests.sh`
Expected: PASS, `test_time_tracking_with_config_creates_commands` included.

- [ ] **Step 7: Strip both findings and commit**

Two findings, one code change — this is the one place the one-commit-per-finding rule bends, because splitting them would leave an intermediate commit that has T12's shared opts without B41's descriptions, i.e. the exact conflict the fold was meant to avoid. Strip both in the one commit and say so in the message.

```bash
todo-parser TIDY.md --strip T12
todo-parser bughunt.md --strip B41
git add src/lib.rs TIDY.md bughunt.md
git commit -m "tidy(duplication)!: table-driven command registration with descriptions [T12][B41]

Collapses six near-identical create_user_command calls into a (name, desc,
handler) table and splits the 110-line body into register_commands and
register_autocommands.

T12 and B41 land together on purpose: T12 alone would hoist a single shared
CreateCommandOpts, which B41 would then have to unwind to give each command
its own description. As a table column the two are the same change.

B41's rename half is deliberately not applied — renames are out of scope for
this pass. TimeTrackingMaybeCloseIfInvisible is marked '(internal)' in its
description instead. All seven command names are byte-identical.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

# Chain C — Lua

Strict order: Task 9 → 14. Tasks 15-16 are order-free within the chain.

### Task 9: T11 — delete `M.test`

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua:1001-1096`

**Context:** `M.test()` is ~96 lines with no caller anywhere in the repo, fully superseded by `health.lua`'s `M.check()` under `:checkhealth time-tracking-nvim`. Deleting it first removes one of the five `pcall(require, ...)` sites Task 10 must otherwise unify, and ~96 lines Tasks 12-13 would carry.

It is a public `M.*` function, so a user's own config could in principle call it — but it is undocumented (README's troubleshooting section names only `:checkhealth` and `version_info()`), which is why this is `risk: medium` rather than low.

- [ ] **Step 1: Re-confirm it has no caller**

Run: `git grep -n 'M\.test\|\.test()' -- ':!docs' ':!*.md'`
Expected: only the definition in `init.lua`. If anything else appears, stop and report.

- [ ] **Step 2: Delete the function**

Remove `function M.test()` through its closing `end` (`lua/time-tracking-nvim/init.lua:1001-1096`). Delete the whole block including its doc comment.

- [ ] **Step 3: Check the README and docs**

Run: `git grep -n 'M\.test\|\.test()' -- '*.md'`
If `README.md`, `DEVELOPMENT.md` or `CLAUDE.md` document it, remove those lines in this same commit.

- [ ] **Step 4: Verify the Lua still loads**

Run: `luajit -bl lua/time-tracking-nvim/init.lua >/dev/null && echo OK`
Expected: `OK`. Then `integration_tests/lua/run_lua_tests.sh` — expected PASS.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser TIDY.md --strip T11
git add lua/time-tracking-nvim/init.lua TIDY.md
git commit -m "tidy(dead-code): delete M.test, superseded by :checkhealth [T11]

96 lines with no caller in the repo. health.lua's M.check performs the same
platform/binary/version/cpath/load probes and is the documented entry point.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 10: T4 — one `load_native` for the four remaining call sites

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` (sites at :752, :831, :890; `M._internal` at :1099)

**Interfaces:**
- Produces: `load_native()` returning `status, native_or_err` where status is `"ok" | "load_failed" | "init_failed"`. Exported on `M._internal`. Consumed by Task 11 (health.lua) and Tasks 12-13.

**Context:** The `pcall(require, "time_tracking_nvim")` + `type(native) == "table" and native.error` two-stage check was written out five times; Task 9 removed one. The remaining four are `init.lua:752`, `:831`, `:890` and `health.lua:84`. The wording has already diverged across them.

- [ ] **Step 1: Add the helper**

Place it above `M.setup` (`init.lua:669`):

```lua
--- Load the native module and classify the outcome.
---
--- The module can fail in two distinct ways that callers must report
--- differently: the shared library may not load at all, or it may load and then
--- report an initialization failure through its `error` key.
---
---@return string status One of "ok", "load_failed", "init_failed"
---@return any value The module on "ok", otherwise the error value
local function load_native()
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		return "load_failed", native
	end
	if type(native) == "table" and native.error then
		return "init_failed", native.error
	end
	return "ok", native
end
```

- [ ] **Step 2: Rewrite the three `init.lua` call sites**

At each of `:752`, `:831`, `:890`, replace the `local ok, native = pcall(require, ...)` block and its two-branch check with a `local status, value = load_native()` and a branch on `status`. **Keep each site's own display text** — only the detection is shared, not the messages.

- [ ] **Step 3: Export on the test seam**

Add to the `M._internal` table (`init.lua:1099`), preserving the existing entries:

```lua
	load_native = load_native,
```

- [ ] **Step 4: Verify**

Run: `luajit -bl lua/time-tracking-nvim/init.lua >/dev/null && integration_tests/lua/run_lua_tests.sh`
Expected: PASS.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser TIDY.md --strip T4
git add lua/time-tracking-nvim/init.lua TIDY.md
git commit -m "tidy(duplication): one load_native for the native-module load check [T4]

The pcall-require plus native.error two-stage check was written out at four
sites with four hand-maintained message sets, and the 'loaded but failed to
init' case had already diverged in wording. Detection is now shared; each
site keeps its own display text.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 11: T8 — health.lua consumes init.lua's helpers

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` (`M._internal` at :1099)
- Modify: `lua/time-tracking-nvim/health.lua:39-40, 53-60, 75, 84-96`

**Interfaces:**
- Consumes: `load_native` (Task 10).
- Produces: `plugin_root`, `get_binary_path`, `get_version_file_path`, `read_binary_version` on `M._internal`.

**Context:** `health.lua` rebuilds the binary path itself (`:39-40`) and re-does the `.version` file read (`:53-60`) instead of importing `init.lua`'s versions (`:104-115`, `:127-139`). They agree today, so `:checkhealth` can report a path `setup()` would never load if either drifts.

- [ ] **Step 1: Export the helpers**

`init.lua` computes its plugin root inside `get_binary_path`. Extract it to a named local first if it is not already one, then add to `M._internal`:

```lua
	plugin_root = plugin_root,
	get_binary_path = get_binary_path,
	get_version_file_path = get_version_file_path,
	read_binary_version = read_binary_version,
```

- [ ] **Step 2: Replace health.lua's path computation**

`health.lua:39-40` currently does `debug.getinfo(1, "S")` and `vim.fs.joinpath`. Replace with `internal.get_binary_path()`, which returns `binary_path, target`. Guard for the seam being absent the way the file already guards `internal.get_platform_info` at `:27`.

- [ ] **Step 3: Replace health.lua's version read**

`health.lua:53-60` becomes `local binary_version = internal.read_binary_version() or "unknown"`.

- [ ] **Step 4: Replace health.lua's cpath check root**

`health.lua:75` uses the local `plugin_root`; point it at `internal.plugin_root()`.

- [ ] **Step 5: Replace health.lua's load check**

`health.lua:84-96` becomes a branch on `internal.load_native()`'s status, keeping health.lua's own `health.error(...)` text and advice lists.

- [ ] **Step 6: Verify**

Run: `luajit -bl lua/time-tracking-nvim/health.lua >/dev/null && integration_tests/lua/run_lua_tests.sh`
Expected: PASS. Then manually: `nvim -c 'checkhealth time-tracking-nvim' -c 'qa!'` should not error.

- [ ] **Step 7: Strip and commit**

```bash
todo-parser TIDY.md --strip T8
git add lua/time-tracking-nvim/init.lua lua/time-tracking-nvim/health.lua TIDY.md
git commit -m "tidy(duplication): health.lua imports init.lua's path and version helpers [T8]

health.lua rebuilt the binary path and re-did the .version read itself, so
:checkhealth could report a path setup() would never load. Both now come
through the M._internal seam.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 12: Characterization tests for `M.setup` and `download_binary`

**Files:**
- Create: `integration_tests/lua/spec_setup.lua`
- Modify: `integration_tests/lua/run_lua_tests.sh` (register the new spec)

**Context:** T1, T2 and T3 all carry `risk: high — needs characterization tests first`. This task writes those tests **against unchanged code** and confirms they pass, so the three refactors that follow have a safety net. Nothing in `lua/` changes here.

`spec_install.lua` (added by d5054df) is the template: it stubs `vim.system` and `vim.uv` and drives the real functions. `harness.lua` provides the assertion helpers.

- [ ] **Step 1: Read the existing harness and a template spec**

Run: `cat integration_tests/lua/harness.lua && cat integration_tests/lua/spec_install.lua`
Note the stubbing pattern and the assertion API before writing anything.

- [ ] **Step 2: Write the characterization spec**

`integration_tests/lua/spec_setup.lua` must pin the *current* observable behaviour of `M.setup`'s branch ladder, with `download_binary` stubbed out. At minimum, one case per ladder outcome:

- binary missing, `auto_download = true` → download is attempted
- binary missing, `auto_download = false` → no download, warning path
- binary present and version matches → no download, native load attempted
- binary present, version differs, `auto_download` and `auto_update` both true → update attempted
- binary present, version differs, `auto_download = false`, `auto_update = true` → **currently falls through and loads the stale binary silently** (this is bughunt B48; pin the behaviour as it is today, and add a comment naming B48 so the test is understood as characterization, not endorsement)

Assert on which stub was called and with what, not on echo text — the refactors in Tasks 13-14 deliberately rewrite the messages.

- [ ] **Step 3: Register the spec**

Add `spec_setup.lua` to `integration_tests/lua/run_lua_tests.sh` alongside the existing specs.

- [ ] **Step 4: Run against unchanged code**

Run: `integration_tests/lua/run_lua_tests.sh`
Expected: **PASS.** If any case fails, the test encodes an assumption the code does not hold — fix the test, not the code. Do not proceed to Task 13 until green.

- [ ] **Step 5: Commit**

```bash
git add integration_tests/lua/spec_setup.lua integration_tests/lua/run_lua_tests.sh
git commit -m "test: characterize M.setup's branch ladder before tidy [T1][T2][T3]

Pins the current observable behaviour of every setup() ladder outcome with
download_binary stubbed, so the three high-risk refactors that follow have a
net. One case pins bughunt B48's silent stale-binary fallthrough as it stands
today — characterization, not endorsement.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 13: T3 — collapse setup()'s twin download/update branches

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua:713-865` (approximate after Tasks 9-11; re-locate by symbol)

**Interfaces:**
- Consumes: `load_native` (Task 10), `spec_setup.lua` (Task 12).
- Produces: `have_download_tools(fatal)`, `download_then_load(target, binary_path, config, labels)`.

**Context:** setup()'s "binary missing" branch and its "needs update" branch are the same ~60-line sequence — executable probe, progress echo, `download_binary(...)`, then `add_to_cpath` + load check — differing only in wording and in whether a missing `curl` is fatal.

- [ ] **Step 1: Extract the tool probe**

```lua
--- Are the external tools auto-download needs present?
---
--- `fatal` distinguishes the two callers: a missing binary cannot be recovered
--- without curl, while a stale one can still be loaded.
---@return boolean ok
---@return table|nil err_chunks Echo chunks describing what is missing
local function have_download_tools(fatal)
	-- folds the has_curl/has_tar/has_unzip trio
end
```

- [ ] **Step 2: Extract the shared download-then-load sequence**

```lua
--- Download the binary, add it to cpath, and load it.
---
---@param labels table { progress, success, failure, load_hint }
local function download_then_load(target, binary_path, config, labels)
	-- the single copy of download_binary + add_to_cpath + load_native check
end
```

Keep the missing-curl severity difference as the `fatal` parameter, not a second copy of the body.

- [ ] **Step 3: Rewrite both branches as calls**

Each branch becomes a `have_download_tools(...)` guard plus one `download_then_load(...)` call with its own label table.

- [ ] **Step 4: Verify against the characterization tests**

Run: `integration_tests/lua/run_lua_tests.sh`
Expected: PASS — Task 12's spec asserts on stub calls, not echo text, so the reworded messages do not break it.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser TIDY.md --strip T3
git add lua/time-tracking-nvim/init.lua TIDY.md
git commit -m "tidy(duplication): fold setup()'s twin download/update branches [T3]

The binary-missing and needs-update branches were the same 60-line sequence
twice, differing only in wording and in whether a missing curl is fatal.
That difference is now a parameter.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 14: T2 — decompose `M.setup`

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` (`M.setup`, re-locate by symbol)

**Interfaces:**
- Consumes: `load_native` (Task 10), `have_download_tools` / `download_then_load` (Task 13).
- Produces: `classify_binary_state(binary_path)`.

**Context:** After Tasks 9, 10 and 13, `M.setup` is already much shorter. What remains is the classification step and the dispatch ladder. **`setup(opts)`'s signature is public and must not change.**

- [ ] **Step 1: Extract the classifier**

```lua
--- Decide what setup() must do about the installed binary.
---
---@return boolean binary_exists
---@return boolean needs_update
---@return string|nil update_reason
local function classify_binary_state(binary_path)
	-- the filereadable + read_binary_version + comparison block
end
```

- [ ] **Step 2: Reduce `M.setup` to merge → resolve → classify → dispatch**

The body should read as those four steps, with the ladder calling `download_then_load` for the two download outcomes and `load_native` for the straight-load outcome.

- [ ] **Step 3: Verify**

Run: `integration_tests/lua/run_lua_tests.sh && luajit -bl lua/time-tracking-nvim/init.lua >/dev/null`
Expected: PASS.

- [ ] **Step 4: Milestone — full suite**

Run: `cargo test && ./integration_tests/run_tests.sh && integration_tests/lua/run_lua_tests.sh`
Expected: PASS.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser TIDY.md --strip T2
git add lua/time-tracking-nvim/init.lua TIDY.md
git commit -m "tidy(long-methods): decompose M.setup into classify and dispatch [T2]

243 lines covering config merge, path resolution, version classification, two
download branches and native loading now reads as merge, resolve, classify,
dispatch. setup(opts)'s signature is unchanged.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 15: T1 — flatten `download_binary`'s callback pyramid

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua:415-667` (re-locate by symbol)

**Interfaces:**
- Consumes: `spec_setup.lua` (Task 12).
- Produces: `fetch_release`, `select_assets`, `fetch_sums`, `verify_archive`, `extract_and_install`, `record_version`, `fail`.

**Context:** 239 lines nested seven levels deep through `vim.system`/`vim.schedule` callbacks, with no seam between fetch, asset-select, checksum, extract and install. This is the largest single item in the batch.

- [ ] **Step 1: Extract the cleanup helper first**

The triple `vim.fn.delete(temp_dir, "rf"); callback(false, msg); return` occurs eight times. Extract it before anything else — it shrinks every subsequent step:

```lua
--- Abandon the download: remove the scratch directory and report the failure.
local function fail(temp_dir, callback, msg)
	vim.fn.delete(temp_dir, "rf")
	callback(false, msg)
end
```

Each site becomes `return fail(temp_dir, callback, msg)`.

- [ ] **Step 2: Extract the pure asset selector**

```lua
--- Pick the release assets for `target`.
---@return string|nil download_url
---@return string|nil sums_url
---@return string|nil asset_name
---@return string|nil err
local function select_assets(release_info, target)
```

This one is pure, so export it on `M._internal` alongside `is_trusted_download_url` and add a spec case for it.

- [ ] **Step 3: Extract the remaining phases**

`fetch_release(release_url, cb)`, `fetch_sums(sums_url, temp_dir, asset_name, cb)`, `verify_archive(temp_file, expected_digest, allow_unverified)` (returns a refusal string or nil, reusing `checksum_verdict`), `extract_and_install(temp_file, temp_dir, binary_path, asset_name, cb)`, `record_version(release_info, expected_version)`.

Replace the nested `verify_then_extract` closure with the top-level `verify_archive`.

- [ ] **Step 4: Verify**

Run: `integration_tests/lua/run_lua_tests.sh`
Expected: PASS — `spec_download_url.lua` and `spec_install.lua` cover parts of this path directly.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser TIDY.md --strip T1
git add lua/time-tracking-nvim/init.lua TIDY.md
git commit -m "tidy(long-methods): flatten download_binary into named phases [T1]

239 lines nested seven levels deep becomes fetch, select, verify, extract,
record — each a named function taking an explicit callback. The eight-times
repeated cleanup triple is now one helper. select_assets is pure and joins
the _internal test seam.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 16: T9 + T10 — semver parse extraction and URL constants

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua:156-197` (T9), `:420, :726, :736, :787` (T10)

**Context — read before starting T9:** `is_version_newer` has **no production caller**. The real update gate is a string inequality (bughunt B55). This task refactors it on the assumption B55 resolves by making it live rather than by deleting it. If B55 has since been resolved by deletion, **skip the T9 half and strip it as obsolete**.

- [ ] **Step 1 (T9): Extract the parser**

```lua
--- Parse a semantic version into numeric parts, tolerating a leading "v".
---@return table|nil parts
local function parse_semver(s)
```

Then fold the two zero-padding loops into the comparison by indexing with `or 0`:

```lua
	for i = 1, math.max(#current_parts, #new_parts) do
		local a, b = current_parts[i] or 0, new_parts[i] or 0
		if a ~= b then
			return b > a
		end
	end
	return false
```

- [ ] **Step 2 (T9): Verify against the existing spec**

Run: `integration_tests/lua/run_lua_tests.sh`
Expected: PASS — `spec_version.lua` has 8 assertion sites against this function.

- [ ] **Step 3 (T9): Strip and commit**

```bash
todo-parser TIDY.md --strip T9
git add lua/time-tracking-nvim/init.lua TIDY.md
git commit -m "tidy(long-methods): extract parse_semver from is_version_newer [T9]

46 lines running nil-guard, v-strip, split, zero-pad and compare becomes a
parser plus a comparison that indexes with 'or 0'. Both padding loops go away.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

- [ ] **Step 4 (T10): Hoist the URL constants**

Near `PLUGIN_VERSION` (`init.lua:22`):

```lua
local REPO = "stevenwcarter/time-tracking-nvim"
local RELEASES_URL = "https://github.com/" .. REPO .. "/releases"
local API_BASE = "https://api.github.com/repos/" .. REPO .. "/releases"
```

Use `RELEASES_URL` at `:726`, `:736`, `:787` and `API_BASE` at `:420`.

**Leave the escaped Lua pattern at `init.lua:289` as a literal.** It is the `is_trusted_download_url` allowlist; deriving it from a variable a future edit could widen would weaken a security check to save one line.

- [ ] **Step 5 (T10): Verify**

Run: `integration_tests/lua/run_lua_tests.sh`
Expected: PASS — `spec_download_url.lua` exercises the allowlist directly.

- [ ] **Step 6 (T10): Strip and commit**

```bash
todo-parser TIDY.md --strip T10
git add lua/time-tracking-nvim/init.lua TIDY.md
git commit -m "tidy(idioms): hoist the repo and releases URLs into constants [T10]

The releases URL was inlined three times and the same repo spelled again as
an API base. The is_trusted_download_url allowlist pattern stays a literal on
purpose — it should not be derivable from a variable a later edit could widen.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

# Chain D — independent

No ordering constraints; run in any order.

### Task 17: T5 — extract the preview buffer-name constant

**Files:**
- Modify: `src/utils.rs` (new const + helper), `src/preview.rs:74, 292`, `src/utils.rs:196`
- Do **not** modify: `src/lib.rs:244`

**Context:** `"[Time Tracking Preview]"` appears at four sites. The two suffix matches (`preview.rs:74`, `utils.rs:196`) are byte-identical expressions.

**Deliberate scope reduction:** the finding names a fourth site, `src/lib.rs:244`'s `autocmd VimLeavePre * silent! bwipeout [Time Tracking Preview]`. That argument is a broken unescaped pattern that never matches — `:bwipeout` splits on whitespace and treats each piece as a regexp, so `[Time` is an unterminated character class and the command errors under `silent!`. This is bughunt **B54**, not in this batch. Interpolating a named constant there would make the line *read* as correct while staying inert, which is worse for the next reader than leaving it visibly odd. Leave it, with a comment.

- [ ] **Step 1: Add the constant and helper to `src/utils.rs`**

```rust
/// Name given to the preview scratch buffer.
///
/// Neovim reports buffer names as absolute paths, so every consumer matches on
/// the *suffix*, never equality.
pub const PREVIEW_BUF_NAME: &str = "[Time Tracking Preview]";

/// Is `buf` the preview buffer?
pub fn is_preview_buf(buf: &Buffer) -> Result<bool> {
    Ok(buf
        .get_name()?
        .to_str()
        .is_ok_and(|s| s.ends_with(PREVIEW_BUF_NAME)))
}
```

- [ ] **Step 2: Use the helper at both suffix-match sites**

In `src/preview.rs:71-77`, the scan loop body becomes `if is_preview_buf(&b)? { found = Some(b); break; }`.

In `src/utils.rs:193-198` (inside `any_tracking_visible`), the skip check becomes `if is_preview_buf(&buf)? { continue; }`. Note this site already has `buf` in hand from the line above, so it does not re-fetch.

- [ ] **Step 3: Use the constant at the naming site**

`src/preview.rs:292` (inside `create_preview_buffer` after Task 7) becomes `b.set_name(PREVIEW_BUF_NAME)?;`.

- [ ] **Step 4: Leave a marker at the fourth site**

Above `src/lib.rs:244`, add:

```rust
    // NOT interpolating PREVIEW_BUF_NAME here on purpose: `:bwipeout` splits its
    // argument on whitespace and matches each piece as a regexp, so this never
    // matches the preview buffer and errors under `silent!` (bughunt B54).
    // Substituting the constant would make the line read as correct while
    // staying inert. Fix it properly with B54 instead.
```

- [ ] **Step 5: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean. `is_preview_buf` needs `Buffer` in scope in `utils.rs` — it is already imported there.

- [ ] **Step 6: Run the integration tests**

Run: `./integration_tests/run_tests.sh`
Expected: PASS.

- [ ] **Step 7: Strip and commit**

```bash
todo-parser TIDY.md --strip T5
git add src/utils.rs src/preview.rs src/lib.rs TIDY.md
git commit -m "tidy(duplication): extract PREVIEW_BUF_NAME and is_preview_buf [T5]

Three of the four sites now share a constant and a helper. The fourth, the
VimLeavePre bwipeout argument, is deliberately left alone: its pattern is
broken (bughunt B54) and interpolating a named constant would disguise that.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 18: T16 — tighten `resolved_data_dir`'s lock and stop allocating on hits

**Files:**
- Modify: `src/utils.rs:49-110`

**Context:** Three coupled problems in one function: the `DATA_DIR_MEMO` guard is held across a `canonicalize(2)` syscall and across two Neovim FFI calls in the `Err` arm; the memo key is `to_owned()`'d on every call including the cache-hit path the memo exists to make cheap; and a 25-line rationale sits in the middle of the `Err` arm.

This runs on every `BufEnter`/`TabEnter`/`WinClosed` and once per window inside `any_tracking_visible`.

- [ ] **Step 1: Borrow the key and scope the lock**

Replace the head of `resolved_data_dir` (`src/utils.rs:50-63`):

```rust
fn resolved_data_dir(config: &Config) -> Option<PathBuf> {
    let configured = config.get_data_directory().unwrap_or("");

    // Scope the guard: everything below this block does a syscall or calls into
    // Neovim, and holding a process-wide mutex across either is what
    // clippy::significant_drop_tightening is warning about.
    let cached = {
        let memo = match DATA_DIR_MEMO.lock() {
            Ok(memo) => memo,
            // A poisoned lock must not disable file detection; fall back to an
            // uncached resolve.
            Err(poisoned) => poisoned.into_inner(),
        };
        memo.as_ref()
            .filter(|(key, _)| key.as_str() == configured)
            .map(|(_, value)| value.clone())
    };

    if let Some(value) = cached {
        return value;
    }
```

- [ ] **Step 2: Re-lock only to store**

The `Ok` arm of the `canonicalize` match stores with the key allocated **only here**:

```rust
        Ok(dir) => {
            let mut memo = match DATA_DIR_MEMO.lock() {
                Ok(memo) => memo,
                Err(poisoned) => poisoned.into_inner(),
            };
            *memo = Some((configured.to_owned(), Some(dir.clone())));
            drop(memo);
            Some(dir)
        }
```

- [ ] **Step 3: Extract the warning path**

Move the `v:exiting` check, the `DATA_DIR_WARNED.call_once` and the entire explanatory comment block out of the `Err` arm into:

```rust
/// Warn once that the configured data directory could not be resolved.
///
/// (move the existing 25-line rationale comment here verbatim — it explains
/// this code, not its caller)
fn warn_data_dir_unresolved(configured: &str, e: &std::io::Error) {
```

The `Err` arm becomes `warn_data_dir_unresolved(configured, &e); None`. Note the `{:?}` in the `log_error!` formats a `&str` identically to a `String`, so no format change is needed.

**Preserve the miss-is-not-cached behaviour.** The doc comment at `:41-48` promises recovery on the next keystroke once the directory appears. Only the `Ok` arm writes to the memo.

- [ ] **Step 4: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean.

- [ ] **Step 5: Run the memo tests specifically**

Run: `./integration_tests/run_tests.sh`
Expected: PASS — `test_data_dir_memo_does_not_leak_between_configs` and `test_data_dir_miss_is_not_cached` are the two that matter here.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser TIDY.md --strip T16
git add src/utils.rs TIDY.md
git commit -m "tidy(idioms): tighten resolved_data_dir's lock and borrow its memo key [T16]

The guard was held across canonicalize(2) and two Neovim FFI calls, and the
key was allocated on every call including the cache hit the memo exists to
make cheap. The Err arm's 25-line rationale moves onto its own helper.
Miss-is-not-cached is preserved.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 19: T17 — let-else for consistency

**Files:**
- Modify: `src/utils.rs:127-130`

- [ ] **Step 1: Replace the match**

```rust
    let Ok(buffer_name_str) = buffer_name.to_str() else {
        return Ok(false);
    };
```

This matches the `let Some(data_dir) = resolved_data_dir(config) else { ... }` form already used 22 lines below in the same function.

- [ ] **Step 2: Build and lint**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt`
Expected: all clean.

- [ ] **Step 3: Strip and commit**

```bash
todo-parser TIDY.md --strip T17
git add src/utils.rs TIDY.md
git commit -m "tidy(idioms): let-else in is_buf_time_tracking_file [T17]

clippy::manual_let_else — a match-with-early-return sat 22 lines above an
idiomatic let-else on the same code path.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 20: T18 — gate clippy on all targets in CI

**Files:**
- Modify: `.github/workflows/ci.yml:48`

- [ ] **Step 1: Widen the clippy invocation**

```yaml
      - name: Run clippy
        run: cargo clippy --all-targets -- -D warnings
```

This matches the invocation `CLAUDE.md` documents for local use.

- [ ] **Step 2: Confirm it is a zero-diff tightening**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: exit 0, no output. The tree is already clean under the stronger form.

- [ ] **Step 3: Strip and commit**

```bash
todo-parser TIDY.md --strip T18
git add .github/workflows/ci.yml TIDY.md
git commit -m "tidy(idioms): gate CI clippy on --all-targets [T18]

CI ran the narrower form, so lints in the crate's own test code were never
gated. The tree is already clean under the stronger invocation.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

### Task 21: T19 — stop rebuilding cargo-audit from source

**Files:**
- Modify: `.github/workflows/ci.yml:74-86`

**Context:** The `test` job already uses `Swatinem/rust-cache@6323deb...`; the `security` job does not, so `cargo install cargo-audit` recompiles the tool and its whole dependency tree on every push and PR.

- [ ] **Step 1: Add the pinned cache step to the security job**

Copy the `Swatinem/rust-cache` step from the `test` job verbatim — **same pinned SHA** — into the `security` job, before the install step.

- [ ] **Step 2: Make the install reproducible**

```yaml
      - name: Install cargo-audit
        run: cargo install cargo-audit --locked
```

- [ ] **Step 3: Validate the workflow parses**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci.yml')); print('YAML OK')"`
Expected: `YAML OK`.

- [ ] **Step 4: Strip and commit**

```bash
todo-parser TIDY.md --strip T19
git add .github/workflows/ci.yml TIDY.md
git commit -m "tidy(opportunistic): cache the security job's toolchain [T19]

cargo-audit and its full dependency tree were recompiled on every push and
PR. Reuses the same pinned rust-cache step the test job already has, and
pins the install with --locked.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs"
```

---

## Final verification

- [ ] **Full suite**

Run: `cargo build && cargo clippy --all-targets -- -D warnings && cargo fmt -- --check && cargo test && ./integration_tests/run_tests.sh && integration_tests/lua/run_lua_tests.sh`
Expected: all green.

- [ ] **Findings files drained of this batch**

Run: `todo-parser TIDY.md --summary && todo-parser bughunt.md --summary`
Expected: `TIDY.md` shows **26 active, 0 marked execute** (the 17 stripped, the 26 unchecked untouched). `bughunt.md` shows **25 active** (30 minus the 5 folded in).

- [ ] **One commit per finding**

Run: `git log --oneline main..HEAD`
Expected: 23 commits — 1 doc-comment commit (`ed9c86e`, already landed), 1 characterization-test commit (Task 12), and 21 finding commits for the 22 findings (T12+B41 share one, by design; Task 16 produces two).

No summary commit. The per-finding commits are the audit trail.
