# code-health Audit Batch Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the 27 findings the user selected in `bughunt.md` — one commit per finding, each with a regression test where the affected behavior is uncovered.

**Architecture:** Three layers change independently. The Rust core (`src/utils.rs`, `src/preview.rs`, `src/lib.rs`) is verified by the existing `#[nvim_oxi::test]` integration suite that drives a real Neovim. The Lua loader (`lua/time-tracking-nvim/init.lua`) gets a new headless-Neovim test harness (Task 0) before any Lua finding is touched. CI/workflow changes are verified by inspection plus a new version-sync check that fails loudly.

**Tech Stack:** Rust (edition 2024), nvim-oxi 0.6.0 (git `7ad27a7`, features `neovim-0-12`, gaining `libuv`), time-tracking-cli 0.9.0 (git `3157a4c`), Lua 5.1/LuaJIT via Neovim 0.12, GitHub Actions.

**Spec:** `docs/superpowers/specs/2026-09-03-codehealth-audit-batch-design.md`

## Global Constraints

- **Edition 2024** for the main crate. `integration_tests/` is edition 2021 and stays that way in this batch — changing it is out of scope.
- **nvim-oxi API floor:** the pinned revision has `Buffer::is_valid`, `Buffer::is_loaded`, `Buffer::get_changedtick`, `Window::get_width`, `Window::set_width`, `Window::is_valid`, `api::err_writeln`, `api::eval`, `api::create_buf`, `libuv::TimerHandle::once`/`stop`, `CommandNArgs::ZeroOrOne`. **`api::notify` does NOT exist** on this revision — that is why `log_info!` is commented out. Use `log_error!` (which wraps `err_writeln`) for every user-visible Rust message. Never call `api::notify`.
- **Never bypass a check:** no `--allow-dirty`, no `--no-verify`, no `#[ignore]` to make a suite pass.
- **One-way test rule:** do not refactor or delete existing tests. Add new ones. The only permitted edits to existing tests are mechanical call-site updates forced by a signature change (Task 15 only).
- **Per-finding commit:** the code change and its `todo-parser bughunt.md --strip B<n>` land in the SAME commit. A characterization test commits separately, BEFORE the fix.
- **Verification command set** (run all, from repo root, after every task):
  ```bash
  cargo fmt -- --check && cargo clippy -- -D warnings && cargo test \
    && (cd integration_tests && cargo test)
  ```
  From Task 0 onward also: `./integration_tests/lua/run_lua_tests.sh`
- **Commit message trailer:** every commit ends with a blank line then
  `Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs`
- **Out of scope — do not touch:** the `decision-needed` marker in `bughunt.md` about `has('nvim-0.11')` vs the `neovim-0-12` cargo feature. Leave the guard, the README requirement line, and the Cargo feature exactly as they are.
- **Stale-artifact gotcha:** `CARGO_TARGET_DIR` is shared (`/home/.build/cargo-target`). If `integration_tests` fails to compile with "multiple different versions of crate `nvim_oxi_api`", it is a stale parent rlib, not a source bug: `touch src/lib.rs` and rebuild. Do not "fix" it by editing types.

## File Structure

| File | Responsibility | Tasks |
|---|---|---|
| `src/utils.rs` | buffer/window/file detection, buffer content read | 1-4 |
| `src/preview.rs` | preview buffer + window lifecycle | 5-13 |
| `src/lib.rs` | plugin entry, commands, autocommands, log macros | 14-17 |
| `lua/time-tracking-nvim/init.lua` | platform detect, download, version, messages | 0, 18-24, 26 |
| `lua/time-tracking-nvim/health.lua` | **new** — `:checkhealth` provider | 25 |
| `integration_tests/src/lib.rs` | Rust regression tests (real Neovim) | 1-17 |
| `integration_tests/lua/harness.lua` | **new** — minimal assert/run harness | 0 |
| `integration_tests/lua/spec_*.lua` | **new** — pure-helper Lua specs | 0, 18, 22 |
| `integration_tests/lua/run_lua_tests.sh` | **new** — headless runner | 0 |
| `.github/workflows/ci.yml` | lint/test/lua-suite/version-sync | 0, 19, 27 |
| `.github/workflows/release.yml` | build matrix, SHA256SUMS, release | 19, 23, 27 |
| `.github/dependabot.yml` | **new** — keep action SHAs fresh | 27 |
| `build.sh` | local dev build → `lua/` | 26 |
| `README.md`, `DEVELOPMENT.md`, `.gitignore` | docs + ignores | 19, 23, 25, 26 |

---

## Task 0: Lua test harness (prerequisite — no finding ID)

**Files:**
- Create: `integration_tests/lua/harness.lua`
- Create: `integration_tests/lua/spec_version.lua`
- Create: `integration_tests/lua/run_lua_tests.sh`
- Modify: `lua/time-tracking-nvim/init.lua` (add `M._internal` near the end, before `return M`)
- Modify: `.github/workflows/ci.yml` (add a Lua-suite step after the integration-tests step)

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `M._internal` on the init.lua module — a table of pure helpers for tests. Initially `{ is_version_newer = is_version_newer, get_platform_info = get_platform_info }`. Later tasks ADD keys: `normalize_os_name` (Task 18), `is_trusted_download_url` (Task 22), `parse_sha256sums` / `file_sha256` (Task 23), `PLUGIN_VERSION` (Task 25).
  - `harness.lua` returns `{ describe, it, eq, ok, run }` where `run()` prints results and returns the failure count.
  - `run_lua_tests.sh` exits 0 on all-pass, 1 otherwise.

- [ ] **Step 1: Write the harness**

Create `integration_tests/lua/harness.lua`:

```lua
-- Minimal zero-dependency assert harness for the Lua loader's pure helpers.
-- Runs under `nvim --headless -u NONE`.
local H = { failures = {}, passes = 0, current = "" }

function H.describe(name, fn)
  H.current = name
  fn()
end

function H.it(name, fn)
  local label = H.current .. " > " .. name
  local ok, err = pcall(fn)
  if ok then
    H.passes = H.passes + 1
  else
    table.insert(H.failures, label .. ": " .. tostring(err))
  end
end

function H.eq(actual, expected, msg)
  if actual ~= expected then
    error(string.format("%s: expected %s, got %s",
      msg or "eq", vim.inspect(expected), vim.inspect(actual)), 2)
  end
end

function H.ok(value, msg)
  if not value then
    error((msg or "ok") .. ": expected truthy, got " .. vim.inspect(value), 2)
  end
end

function H.run()
  for _, f in ipairs(H.failures) do
    io.stderr:write("FAIL  " .. f .. "\n")
  end
  io.stdout:write(string.format("%d passed, %d failed\n", H.passes, #H.failures))
  return #H.failures
end

return H
```

- [ ] **Step 2: Expose the pure helpers for testing**

In `lua/time-tracking-nvim/init.lua`, immediately before the final `return M`, add:

```lua
-- Test seam. Not part of the public API; contents may change without notice.
-- Only pure, side-effect-free helpers belong here.
M._internal = {
	is_version_newer = is_version_newer,
	get_platform_info = get_platform_info,
}

return M
```

(Delete the old bare `return M` — do not leave two.)

- [ ] **Step 3: Write the first spec, exercising the harness itself**

Create `integration_tests/lua/spec_version.lua`:

```lua
local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal

H.describe("is_version_newer", function()
  H.it("reports a higher patch as newer", function()
    H.eq(internal.is_version_newer("0.1.4", "0.1.7"), true)
  end)

  H.it("reports equal versions as not newer", function()
    H.eq(internal.is_version_newer("0.1.7", "0.1.7"), false)
  end)

  H.it("reports a lower version as not newer", function()
    H.eq(internal.is_version_newer("0.1.7", "0.1.4"), false)
  end)

  H.it("tolerates a leading v on either side", function()
    H.eq(internal.is_version_newer("v0.1.4", "v0.1.7"), true)
  end)

  H.it("pads a shorter version with zeros", function()
    H.eq(internal.is_version_newer("0.1", "0.1.1"), true)
    H.eq(internal.is_version_newer("0.1.0", "0.1"), false)
  end)

  H.it("assumes newer when either side is nil", function()
    H.eq(internal.is_version_newer(nil, "0.1.7"), true)
    H.eq(internal.is_version_newer("0.1.7", nil), true)
  end)
end)

return H
```

- [ ] **Step 4: Write the runner**

Create `integration_tests/lua/run_lua_tests.sh` (and `chmod +x` it):

```bash
#!/bin/bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${HERE}/../.." && pwd)"

echo "Running Lua loader tests (headless Neovim)..."

nvim --headless -u NONE --noplugin \
  --cmd "set runtimepath^=${REPO_ROOT}" \
  --cmd "lua package.path = package.path .. ';${HERE}/?.lua'" \
  -c "lua
    local failures = 0
    for _, spec in ipairs({ 'spec_version' }) do
      package.loaded['harness'] = nil
      package.loaded[spec] = nil
      local H = require(spec)
      failures = failures + H.run()
    end
    if failures > 0 then vim.cmd('cquit 1') end
    vim.cmd('qall!')
  "

echo "Lua loader tests passed."
```

**Note for whoever extends this:** the spec list inside the `-c "lua ...` block is literal. Later tasks that add a spec file MUST add its name to that `{ 'spec_version' }` table.

- [ ] **Step 5: Run the suite and verify it passes**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: `6 passed, 0 failed` then `Lua loader tests passed.`

If `require("time-tracking-nvim")` fails, the `runtimepath^=` prepend is wrong — the module lives at `lua/time-tracking-nvim/init.lua` relative to the repo root.

- [ ] **Step 6: Verify the harness actually catches failures**

Temporarily change the first assertion to `H.eq(internal.is_version_newer("0.1.4", "0.1.7"), false)`, run the script, and confirm it prints a `FAIL` line and exits non-zero (`echo $?` → 1). Then revert the assertion.

This step is not optional: a harness that cannot fail is worse than no harness.

- [ ] **Step 7: Wire it into CI**

In `.github/workflows/ci.yml`, after the "Run integration tests" step, add:

```yaml
      - name: Run Lua loader tests
        if: matrix.os == 'ubuntu-latest'
        run: |
          chmod +x integration_tests/lua/run_lua_tests.sh
          ./integration_tests/lua/run_lua_tests.sh
```

- [ ] **Step 8: Full verification**

```bash
cargo fmt -- --check && cargo clippy -- -D warnings && cargo test \
  && (cd integration_tests && cargo test) \
  && ./integration_tests/lua/run_lua_tests.sh
```
Expected: all green, 21 Rust integration tests, 6 Lua assertions.

- [ ] **Step 9: Commit**

```bash
git add -A
git commit -m "$(cat <<'EOF'
test: add headless-Neovim harness for the Lua loader

The Lua layer had zero test coverage. Adds a minimal zero-dependency
assert harness, a first spec covering is_version_newer, and a CI step.
Pure helpers are reached through a new M._internal test seam.

Prerequisite for the Lua findings in the 2026-09-03 code-health batch.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

**No `--strip`:** Task 0 is not a `bughunt.md` finding.

---

## Task 1: B17 — canonicalize rejects a not-yet-written tracking file

**Files:**
- Modify: `src/utils.rs:33-45`
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `is_buf_time_tracking_file(Buffer, &Config) -> Result<bool>` — signature unchanged, now returns `true` for a path whose parent exists but whose file does not.

- [ ] **Step 1: Write the failing test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_for_file_not_yet_written() {
    let (config, temp_dir) = create_test_config_with_temp_dir();

    // The primary workflow: `nvim ~/timetracking/2026-09-03.md` for today's
    // date, where the file does not exist on disk yet.
    let unwritten = temp_dir.path().join("2026-09-03.md");
    assert!(!unwritten.exists(), "precondition: file must not exist");

    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&unwritten).unwrap();

    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(
        result,
        "a .md file in the data directory that has not been written yet \
         should still be recognised as a time tracking file"
    );
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_unwritten_file_outside_data_dir() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let other_dir = TempDir::new().unwrap();

    let unwritten = other_dir.path().join("2026-09-03.md");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&unwritten).unwrap();

    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(
        !result,
        "tolerating an unwritten file must not also stop enforcing the \
         data-directory boundary"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd integration_tests && cargo test test_is_buf_time_tracking_file_for_file_not_yet_written`
Expected: FAIL — `assertion failed: result`. (The second test passes already; it is the guard that the fix does not over-widen.)

- [ ] **Step 3: Implement**

In `src/utils.rs`, replace the block that canonicalizes `buffer_path` (currently lines 33-45, from `let buffer_path = Path::new(buffer_name_str);` down to the `if buffer_path.is_none() { return Ok(false); }`) with:

```rust
    let buffer_path = Path::new(buffer_name_str);

    // The file may not exist yet — opening today's not-yet-written daily note
    // is the primary workflow — so resolve the parent directory instead and
    // rejoin the file name. Falls back to the raw path when the parent does
    // not resolve either.
    let buffer_path = match (buffer_path.parent(), buffer_path.file_name()) {
        (Some(parent), Some(file_name)) => fs::canonicalize(parent)
            .map(|dir| dir.join(file_name))
            .unwrap_or_else(|_| buffer_path.to_path_buf()),
        _ => buffer_path.to_path_buf(),
    };
```

Then delete the now-dangling `if buffer_path.is_none() || data_dir.is_none()` check's `buffer_path` half and the `let buffer_path = buffer_path.unwrap();` line, leaving:

```rust
    let data_dir = fs::canonicalize(config.get_data_directory().unwrap_or(""))
        .map_err(|_| Error::Other("could not find path for data directory".to_owned()))
        .ok();

    let Some(data_dir) = data_dir else {
        return Ok(false);
    };
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 23 tests (21 existing + 2 new). All existing on-disk tests stay green, which is what proves the change is a widening, not a replacement.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B17
git add -A
git commit -m "$(cat <<'EOF'
fix(correctness): recognise a tracking file that is not yet on disk [B17]

fs::canonicalize requires the path to exist, so opening today's daily
note before the first :w made is_buf_time_tracking_file return false —
auto-open, :TimeTrackingToggle and live updates all silently did nothing
until the file was saved. Canonicalize the parent directory and rejoin
the file name instead, keeping the symlink-resolution intent.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 2: B6 — data-directory canonicalize failure is swallowed

**Files:**
- Modify: `src/utils.rs` (the `data_dir` block, and add a `use std::sync::Once`)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: Task 1's rewritten `buffer_path` block.
- Produces: no signature change. A module-level `static DATA_DIR_WARNED: Once` in `src/utils.rs`.

- [ ] **Step 1: Write the characterization test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_missing_data_directory_returns_false_and_does_not_panic() {
    // A data_directory that does not exist — the "misconfigured time-tracking-cli"
    // case that currently turns the whole plugin into a silent no-op.
    let config = Config {
        data_directory: Some("/nonexistent/time/tracking/dir".to_string()),
        date: time::Date::from_calendar_date(2024, time::Month::January, 1).unwrap(),
        ..Default::default()
    };

    let scratch = TempDir::new().unwrap();
    let md_file = create_test_file(scratch.path(), "test.md", "# Test");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();

    // Repeated calls model the per-keystroke TextChanged path: the warning
    // must be emitted at most once, and no call may panic or error.
    for _ in 0..5 {
        let buf = buf.clone();
        let result = is_buf_time_tracking_file(buf, &config);
        assert!(
            result.is_ok(),
            "a missing data directory must not produce an Err: {:?}",
            result
        );
        assert!(!result.unwrap(), "nothing is a tracking file without a data dir");
    }
}
```

- [ ] **Step 2: Run it**

Run: `cd integration_tests && cargo test test_missing_data_directory_returns_false_and_does_not_panic`
Expected: this test **passes on unchanged code** — the current behavior already returns `Ok(false)`. That is expected: it is a characterization test pinning the contract (no Err, no panic, idempotent) that the `Once` must not break. Record in the commit that it is characterization, not RED-first.

The genuinely observable part of B6 — that a message is emitted — cannot be asserted from inside `#[nvim_oxi::test]` because `err_writeln` output is not capturable there. Verify it manually in Step 5.

- [ ] **Step 3: Implement**

At the top of `src/utils.rs` add `Once` to the std import:

```rust
use std::{fs, path::Path, sync::Once};
```

Add a module-level static after the imports:

```rust
/// Guards the data-directory warning so the per-keystroke `TextChanged` path
/// cannot spam `:messages` with the same line on every keypress.
static DATA_DIR_WARNED: Once = Once::new();
```

Replace the `data_dir` block with:

```rust
    // TODO: Need to canonicalize in case the data directory is a symlink, should be done upstream
    // probably
    let data_dir = match fs::canonicalize(config.get_data_directory().unwrap_or("")) {
        Ok(dir) => dir,
        Err(e) => {
            DATA_DIR_WARNED.call_once(|| {
                log_error!(
                    "[time-tracking-nvim] could not resolve data directory {:?}: {}. \
                     The preview will not open for any file until this is fixed.",
                    config.get_data_directory().unwrap_or("<unset>"),
                    e
                );
            });
            return Ok(false);
        }
    };
```

`log_error!` is exported at the crate root via `#[macro_export]`, so it resolves as `crate::log_error!` — no `use` needed inside `utils.rs`.

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 24 tests.

- [ ] **Step 5: Verify the message manually**

```bash
./build.sh
```
(If `build.sh` has not yet been fixed by Task 26, copy the artifact by hand:
`cp target/release/libtime_tracking_nvim.so lua/time_tracking_nvim.so`.)

Then, with a `time-tracking-cli` config whose `data_directory` points at a
nonexistent path:

```bash
nvim -c 'lua require("time-tracking-nvim").setup({auto_download=false, auto_update=false})' \
     -c 'TimeTrackingToggle' -c 'messages'
```

Expected: one `could not resolve data directory` line in `:messages`. Type a few
characters and re-check `:messages` — the line must still appear exactly once.

If a working misconfigured setup cannot be produced locally, say so explicitly
in the task report rather than claiming the manual check passed.

- [ ] **Step 6: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 7: Strip and commit**

```bash
todo-parser bughunt.md --strip B6
git add -A
git commit -m "$(cat <<'EOF'
fix(observability): report an unresolvable data directory once [B6]

The descriptive Error::Other built for a missing or unreadable
data_directory was discarded with .ok(), turning a misconfigured
time-tracking-cli into a plugin that loads cleanly, registers every
command, and then does nothing forever with no message anywhere.

Emit it through log_error! behind a std::sync::Once so the
per-keystroke TextChanged path cannot spam :messages.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 3: B15 — data directory re-canonicalized on every call

**Files:**
- Modify: `src/utils.rs`
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: Task 2's `DATA_DIR_WARNED` and the `match`-based `data_dir` block.
- Produces: a private `fn resolved_data_dir(config: &Config) -> Option<PathBuf>` in `src/utils.rs`.

**Design decision (resolving the spec's open question):** the integration suite
builds several `Config`s with different `TempDir`s in one process, so a
process-global `OnceLock<Option<PathBuf>>` would memoize the first test's temp
dir and break every later test. Use a **keyed** memo instead: a
`Mutex<Option<(String, Option<PathBuf>)>>` holding the raw configured string
alongside its resolved value, recomputing only when the key changes. Under the
real invariant (one static Config per process) this resolves exactly once, and
it stays correct when the invariant does not hold. State this choice in the
commit message.

- [ ] **Step 1: Write the guard test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_data_dir_memo_does_not_leak_between_configs() {
    // Two configs with different data directories, used alternately. A
    // process-global memo keyed only on "have I run yet" would answer with the
    // first config's directory for the second config's buffer.
    let (config_a, dir_a) = create_test_config_with_temp_dir();
    let (config_b, dir_b) = create_test_config_with_temp_dir();

    let file_a = create_test_file(dir_a.path(), "a.md", "# A");
    let file_b = create_test_file(dir_b.path(), "b.md", "# B");

    for _ in 0..3 {
        let mut buf_a = api::create_buf(false, false).unwrap();
        buf_a.set_name(&file_a).unwrap();
        assert!(
            is_buf_time_tracking_file(buf_a, &config_a).unwrap(),
            "file A must resolve against config A"
        );

        let mut buf_b = api::create_buf(false, false).unwrap();
        buf_b.set_name(&file_b).unwrap();
        assert!(
            is_buf_time_tracking_file(buf_b, &config_b).unwrap(),
            "file B must resolve against config B"
        );

        // Cross pairs must stay false.
        let mut buf_a2 = api::create_buf(false, false).unwrap();
        buf_a2.set_name(&file_a).unwrap();
        assert!(
            !is_buf_time_tracking_file(buf_a2, &config_b).unwrap(),
            "file A must not resolve against config B"
        );
    }
}
```

- [ ] **Step 2: Run to verify it passes on unchanged code**

Run: `cd integration_tests && cargo test test_data_dir_memo_does_not_leak_between_configs`
Expected: PASS. This is the guard test written BEFORE the memo, so that the
memo cannot silently introduce the leak. Keep it — it is the load-bearing test
for this task.

- [ ] **Step 3: Implement**

Add to the imports in `src/utils.rs`:

```rust
use std::{fs, path::{Path, PathBuf}, sync::{Mutex, Once}};
```

Add after `DATA_DIR_WARNED`:

```rust
/// Memoized resolution of the configured data directory.
///
/// `Config` is loaded once at plugin init and never mutated (see `lib.rs`), so
/// in production this resolves exactly once instead of paying a `realpath(2)`
/// on every keystroke. Keyed on the raw configured string so that tests — which
/// build several `Config`s in one process — still get the right answer.
static DATA_DIR_MEMO: Mutex<Option<(String, Option<PathBuf>)>> = Mutex::new(None);

fn resolved_data_dir(config: &Config) -> Option<PathBuf> {
    let configured = config.get_data_directory().unwrap_or("").to_owned();

    let mut memo = match DATA_DIR_MEMO.lock() {
        Ok(memo) => memo,
        // A poisoned lock must not disable file detection; fall back to an
        // uncached resolve.
        Err(poisoned) => poisoned.into_inner(),
    };

    if let Some((key, value)) = memo.as_ref()
        && key == &configured
    {
        return value.clone();
    }

    let resolved = match fs::canonicalize(&configured) {
        Ok(dir) => Some(dir),
        Err(e) => {
            DATA_DIR_WARNED.call_once(|| {
                log_error!(
                    "[time-tracking-nvim] could not resolve data directory {:?}: {}. \
                     The preview will not open for any file until this is fixed.",
                    configured,
                    e
                );
            });
            None
        }
    };

    *memo = Some((configured, resolved.clone()));
    resolved
}
```

Replace the `match fs::canonicalize(...)` block added in Task 2 with:

```rust
    let Some(data_dir) = resolved_data_dir(config) else {
        return Ok(false);
    };
```

Note: `let ... && ...` let-chains are stable in edition 2024. If clippy or rustc
rejects the chain, rewrite as a nested `if let` — do not change the semantics.

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 25 tests, including `test_data_dir_memo_does_not_leak_between_configs`.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B15
git add -A
git commit -m "$(cat <<'EOF'
perf(caching): memoize the resolved data directory [B15]

fs::canonicalize on the configured data directory ran on every call —
once per keystroke via TextChanged, and once per open window via
any_tracking_visible — to recompute an invariant value.

Memoized keyed on the raw configured string rather than with a bare
OnceLock: the integration suite builds several Configs in one process,
and an unkeyed memo would answer with the first one's directory forever.
Under the real invariant (one static Config per process) this still
resolves exactly once.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 4: B21 — `get_buffer_content` allocation churn

**Files:**
- Modify: `src/utils.rs:68-77`
- Test: covered by existing `test_get_buffer_content` / `test_get_buffer_content_empty`

**Interfaces:**
- Consumes: nothing.
- Produces: `get_buffer_content() -> Result<String>` — signature and output unchanged.

- [ ] **Step 1: Confirm the existing tests cover this**

Run: `cd integration_tests && cargo test get_buffer_content`
Expected: PASS, 2 tests. `risk: low` and the behavior is already covered, so no new characterization test is required.

- [ ] **Step 2: Implement**

Replace the body of `get_buffer_content` in `src/utils.rs`:

```rust
/// Get the content of the current buffer
pub fn get_buffer_content() -> Result<String> {
    let current_buffer = api::get_current_buf();
    let line_count = current_buffer.line_count()?;
    let lines = current_buffer.get_lines(0..line_count, false)?;

    // Build the joined string directly: the previous
    // `.map(to_string).collect::<Vec<_>>().join()` allocated one String per
    // line plus a Vec, then threw them all away.
    let mut content = String::new();
    for (i, line) in lines.enumerate() {
        if i > 0 {
            content.push('\n');
        }
        content.push_str(&line.to_string());
    }
    Ok(content)
}
```

`line.to_string()` is kept deliberately: it is exactly what the old code did per
line, so the output is byte-identical. The win is dropping the `Vec` and the
second full-size allocation, not changing how a line is stringified.

- [ ] **Step 3: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 25 tests, `test_get_buffer_content` and `test_get_buffer_content_empty` both green.

- [ ] **Step 4: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser bughunt.md --strip B21
git add -A
git commit -m "$(cat <<'EOF'
perf(caching): build buffer content without the intermediate Vec [B21]

The old map/collect/join allocated a String per line plus a Vec and
discarded all of it for the joined result — ~502 allocations for a
500-line file, once per keystroke on the TextChangedI path.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

### MILESTONE A — after Task 4

Run the full suite from the repo root:
```bash
cargo fmt -- --check && cargo clippy -- -D warnings && cargo test \
  && (cd integration_tests && cargo test) \
  && ./integration_tests/lua/run_lua_tests.sh
```
On red: bisect within Tasks 1-4, revert the offender, surface the diagnosis. Do not continue past a red milestone.

---

## Task 5: B5 — blocking `thread::sleep` on the UI thread

**Files:**
- Modify: `src/preview.rs:205-208` and `src/preview.rs:259-262`
- Modify: `src/lib.rs` (widen the `pub use` re-exports)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `auto_open_preview_impl` / `auto_close_preview_impl` — signatures unchanged, no longer block.
  - `src/lib.rs` re-exports widened to
    `pub use preview::{auto_open_preview, close_preview, create_or_update_preview, toggle_preview_fn, update_preview_fn};`
    so the integration crate can drive them. Later tasks add `update_preview_debounced` (Task 17).

- [ ] **Step 1: Widen the re-exports**

In `src/lib.rs`, replace `pub use preview::create_or_update_preview;` with:

```rust
pub use preview::{
    auto_open_preview, close_preview, create_or_update_preview, toggle_preview_fn,
    update_preview_fn,
};
```

Keep the `use preview::*;` line below it — it is what lets `lib.rs` name the
rest of the module unqualified.

- [ ] **Step 2: Write the failing test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_auto_open_does_not_block_the_event_loop() {
    use std::time::Instant;

    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    // A buffer that is NOT a tracking file: the old code slept 200ms *before*
    // even checking, so this cost the full delay for every unrelated markdown
    // buffer at VimEnter/BufWinEnter.
    let other = TempDir::new().unwrap();
    let md = create_test_file(other.path(), "README.md", "# Unrelated");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    let start = Instant::now();
    for _ in 0..3 {
        time_tracking_nvim::auto_open_preview(config_static).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 150,
        "three auto-open calls on a non-tracking buffer took {:?}; the \
         blocking thread::sleep is still on the event-loop thread",
        elapsed
    );
}
```

Confirm `api::set_current_buf` exists on this nvim-oxi revision:
`grep -n "pub fn set_current_buf" /home/steve/.cargo/git/checkouts/nvim-oxi-*/7ad27a7/crates/api/src/vim.rs`.
If it does not, switch the current buffer with
`api::command(&format!("buffer {}", buf.handle()))` instead, and use that same
approach in every later task that needs it (Tasks 7, 15, 17).

- [ ] **Step 3: Run to verify it fails**

Run: `cd integration_tests && cargo test test_auto_open_does_not_block_the_event_loop`
Expected: FAIL — elapsed ≈ 600ms, well over the 150ms bound.

- [ ] **Step 4: Implement**

In `src/preview.rs`, delete the sleep from `auto_open_preview_impl`:

```rust
pub fn auto_open_preview_impl(config: &'static Config) -> Result<()> {
    // No delay here: this runs on Neovim's single event-loop thread, so
    // sleeping cannot let a pending window operation complete — it is exactly
    // what prevents it. The split-during-close race is handled by the E242
    // guard in create_or_update_preview and the empty-window-list bail.
    let is_tracking = is_time_tracking_file(config)?;
```

And from `auto_close_preview_impl`:

```rust
pub fn auto_close_preview_impl(_config: &'static Config) -> Result<()> {
    // Always close the preview when BufLeave is triggered for a markdown file
```

(Delete both `std::thread::sleep(...)` lines and their preceding
"Add a small delay to avoid race conditions" comments.)

- [ ] **Step 5: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 26 tests.

- [ ] **Step 6: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 7: Strip and commit**

```bash
todo-parser bughunt.md --strip B5
git add -A
git commit -m "$(cat <<'EOF'
fix(caching): remove blocking sleeps from the event-loop thread [B5]

auto_open_preview_impl slept 200ms on Neovim's single UI thread before
even checking whether the buffer was a tracking file, so `nvim README.md`
froze at VimEnter and again from the scheduled auto-open; auto-close
added 30ms more. The comment claimed the delay avoided races with window
operations, but blocking the loop is precisely what prevents pending
window work from completing.

The race is already covered by the E242 guard and the empty-window-list
bail in create_or_update_preview.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 6: B9 — raw `eprintln!` garbles the screen

**Files:**
- Modify: `src/preview.rs:135-153`
- Test: none (not independently assertable from `#[nvim_oxi::test]`; verified by inspection + the E242 path staying green)

**Interfaces:**
- Consumes: nothing.
- Produces: no signature change.

- [ ] **Step 1: Implement**

In `src/preview.rs`, replace the split-failure block:

```rust
    // If not, create a vertical split and attach the preview buffer to it
    if !is_open {
        // Use a plain command for portability; it's fine here.
        if let Err(e) = api::command("rightbelow vsplit") {
            let msg = e.to_string();
            if msg.contains("E242") || msg.contains("Can't split a window while closing another") {
                // Window operation in progress; skip this update.
                debug_log!("[ttnvim] skipping split during window close: {}\n", msg);
                return Ok(());
            }
            log_error!("[time-tracking-nvim] failed to split: {}", msg);
            return Ok(());
        }

        // Current window is the new split
        let mut win: Window = api::get_current_win();

        // Attach our preview buffer
        if let Err(e) = win.set_buf(&buf) {
            log_error!("[time-tracking-nvim] failed to set preview buffer: {}", e);
            let _ = win.close(false);
            return Ok(());
        }
```

`eprintln!` wrote raw bytes to stderr from inside a running Neovim, painting
over the editor grid; `log_error!` routes through `api::err_writeln`, which
reaches `:messages` and renders correctly in GUI clients.

- [ ] **Step 2: Verify no `eprintln!` survives in `src/`**

Run: `grep -rn 'eprintln!' src/`
Expected: no output. If any remain outside `preview.rs`, they are not part of
B9 — leave them and note it in the task report.

- [ ] **Step 3: Full verification**

Run the Global Constraints verification command set. Expected: all green, 26 integration tests.

- [ ] **Step 4: Strip and commit**

```bash
todo-parser bughunt.md --strip B9
git add -A
git commit -m "$(cat <<'EOF'
fix(observability): route split failures through err_writeln [B9]

The two split/attach failure paths used eprintln!, writing raw bytes to
stderr from inside a running Neovim — messages painted diagonally across
the editor grid, invisible in GUI clients, never in :messages. Reachable
from the per-keystroke path, so a repeating narrow-terminal E36 garbled
the screen on every keystroke.

This is the failure the 2026-04-25 gate-debug-logging spec was written to
fix; these two calls survived it. Also demotes the E242 special case to
debug_log! rather than dropping it silently.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 7: B23 — `TimeTrackingToggle` silent no-op

**Files:**
- Modify: `src/preview.rs:3-8`
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: `toggle_preview_fn` exported publicly in Task 5.
- Produces: `toggle_preview_fn(&'static Config) -> Result<()>` — unchanged signature, now emits a message on the not-a-tracking-file path.

- [ ] **Step 1: Write the characterization test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_toggle_outside_data_dir_creates_no_preview_and_returns_ok() {
    cleanup_preview_buffers();

    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let other = TempDir::new().unwrap();
    let md = create_test_file(other.path(), "notes.md", "# Unrelated");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    let result = time_tracking_nvim::toggle_preview_fn(config_static);
    assert!(result.is_ok(), "toggle must not error: {:?}", result);

    let has_preview = api::list_bufs().any(|b| {
        b.get_name()
            .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
            .unwrap_or(false)
    });
    assert!(
        !has_preview,
        "toggling outside the data directory must not create a preview"
    );
}
```

This is a characterization test: it pins that adding the message does not
accidentally start creating a preview. The message itself is not capturable
from `#[nvim_oxi::test]`, so verify it manually in Step 4.

- [ ] **Step 2: Run to verify it passes on unchanged code**

Run: `cd integration_tests && cargo test test_toggle_outside_data_dir_creates_no_preview_and_returns_ok`
Expected: PASS (characterization).

- [ ] **Step 3: Implement**

In `src/preview.rs`:

```rust
pub fn toggle_preview_fn(config: &'static Config) -> Result<()> {
    // Check if this is a time tracking file
    if !is_time_tracking_file(config)? {
        // The user typed :TimeTrackingToggle explicitly, and README names this
        // as the first troubleshooting step — so unlike the autocommand-driven
        // paths, say why nothing happened.
        let buffer_name = api::get_current_buf()
            .get_name()
            .map(|n| n.to_string())
            .unwrap_or_else(|_| String::new());
        log_error!(
            "[time-tracking-nvim] {} is not a tracking file (data directory: {:?}). \
             Tracking files are .md files inside the data directory.",
            if buffer_name.is_empty() { "[No Name]" } else { &buffer_name },
            config.get_data_directory().unwrap_or("<unset>")
        );
        return Ok(());
    }
```

Leave `update_preview_fn` and `auto_open_preview` silent — they run from
autocommands, where a message per keystroke would be far worse than silence.

- [ ] **Step 4: Verify the message manually**

Build and load the plugin (as in Task 2 Step 5), open a `.md` file outside the
data directory, run `:TimeTrackingToggle`, then `:messages`. Expected: one line
naming both the buffer and the configured data directory.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green, 27 integration tests.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B23
git add -A
git commit -m "$(cat <<'EOF'
fix(observability): explain why :TimeTrackingToggle did nothing [B23]

Silence is right for the autocommand-driven paths and wrong here: the
user typed the command, and README names it as the first thing to try
when the preview does not appear. Combined with the swallowed
data-directory error, the two most common failure modes — wrong directory
and broken config — produced byte-identical behavior: nothing.

Names both inputs (buffer and configured data directory). update_preview_fn
and auto_open_preview stay silent.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 8: B33 — `close_preview` strands the user on E444

**Files:**
- Modify: `src/preview.rs:174-191`
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: `close_preview` exported publicly in Task 5.
- Produces: `close_preview() -> Result<()>` — unchanged signature, never returns Err for a last-window close.

- [ ] **Step 1: Write the failing test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_close_preview_when_it_is_the_last_window() {
    cleanup_preview_buffers();

    // Put the preview in the only window, the state reached by pressing
    // <C-w>c in the file window (QuitPre does not fire for :close).
    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let preview_buf = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
                .unwrap_or(false)
        })
        .expect("preview buffer should exist");

    // Collapse to a single window showing the preview.
    api::command("only").unwrap();
    let mut win = api::get_current_win();
    win.set_buf(&preview_buf).unwrap();
    assert_eq!(api::list_wins().count(), 1, "precondition: one window");

    let result = close_preview();
    assert!(
        result.is_ok(),
        "closing the preview as the last window must not propagate E444: {:?}",
        result
    );

    let still_showing_preview = api::get_current_win()
        .get_buf()
        .unwrap()
        .get_name()
        .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
        .unwrap_or(false);
    assert!(
        !still_showing_preview,
        "the user must not be left sitting in the nomodifiable preview buffer"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd integration_tests && cargo test test_close_preview_when_it_is_the_last_window`
Expected: FAIL — `close_preview()` returns `Err` carrying E444, so the first assertion trips.

- [ ] **Step 3: Implement**

Replace `close_preview` in `src/preview.rs`:

```rust
/// Close the preview window if it exists
pub fn close_preview() -> Result<()> {
    let windows: Vec<Window> = api::list_wins().collect();
    let window_count = windows.len();

    for mut win in windows {
        let buf = win.get_buf()?;
        let buf_name = buf.get_name()?;
        if buf_name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            if window_count == 1 {
                // nvim_win_close behaves like :close and refuses the last
                // window (E444). Swap in a normal buffer instead, so the user
                // lands somewhere usable rather than stuck in the unlisted,
                // nomodifiable preview with no way back but :b#.
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
                // Non-fatal: propagating here turns a single close failure into
                // an error re-echoed on every subsequent BufEnter/WinClosed.
                log_error!("[time-tracking-nvim] could not close the preview window: {}", e);
            }
            break;
        }
    }

    Ok(())
}
```

`Window::close` takes `self` by value, so `win` must be owned — collecting
`list_wins()` into a `Vec` first is what makes both `set_buf(&mut self)` and
`close(self)` usable in the same loop.

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 28 tests. Pay attention to `test_multiple_preview_creation_updates_same_buffer` and the other preview tests: `close_preview` is shared, so a regression shows up there.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B33
git add -A
git commit -m "$(cat <<'EOF'
fix(frontend): do not strand the user when the preview is the last window [B33]

nvim_win_close behaves like :close and refuses the last window with
E444. Pressing <C-w>c in the file window (which does not fire QuitPre)
left the user sitting in the unlisted, nomodifiable preview scratch
buffer, with the error re-echoed on every subsequent BufEnter/WinClosed
and recovery requiring knowledge of :b#.

Swap in a normal buffer instead when it is the last window, and downgrade
a close failure to a logged non-fatal.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 9: B31 — preview split inherits `number`/`wrap`/`signcolumn`

**Files:**
- Modify: `src/preview.rs` (inside the `if !is_open` block, after the `winfixwidth` call)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: Task 6's rewritten `if !is_open` block.
- Produces: no signature change.

- [ ] **Step 1: Write the failing test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_preview_window_is_styled_as_a_scratch_preview() {
    use nvim_oxi::api::opts::OptionOptsBuilder;

    cleanup_preview_buffers();

    // A vsplit copies the source window's local options, so set the
    // near-ubiquitous ones on the source first.
    let sopts = OptionOptsBuilder::default().win(api::get_current_win()).build();
    api::set_option_value("number", true, &sopts).unwrap();
    api::set_option_value("wrap", true, &sopts).unwrap();
    api::set_option_value("signcolumn", "yes", &sopts).unwrap();

    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let preview_win = api::list_wins()
        .find(|w| {
            w.get_buf()
                .and_then(|b| b.get_name())
                .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
                .unwrap_or(false)
        })
        .expect("preview window should exist");

    let wopts = OptionOptsBuilder::default().win(preview_win).build();
    assert!(
        !api::get_option_value::<bool>("number", &wopts).unwrap(),
        "the preview must not show line numbers"
    );
    assert!(
        !api::get_option_value::<bool>("wrap", &wopts).unwrap(),
        "the preview must not soft-wrap"
    );
    assert_eq!(
        api::get_option_value::<String>("signcolumn", &wopts).unwrap(),
        "no",
        "the preview must not reserve a sign column"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd integration_tests && cargo test test_preview_window_is_styled_as_a_scratch_preview`
Expected: FAIL on the first assertion — the split inherited `number`.

- [ ] **Step 3: Implement**

In `src/preview.rs`, right after the `winfixwidth` line:

```rust
        // Keep the split's width fixed
        let wopts = OptionOptsBuilder::default().win(win.clone()).build();
        let _ = api::set_option_value("winfixwidth", true, &wopts);

        // A vsplit copies the source window's local options, so an ordinary
        // `set number relativenumber list signcolumn=yes` config eats 6-8 of
        // the preview's ~26 columns. Style it as the scratch preview it is.
        let _ = api::set_option_value("number", false, &wopts);
        let _ = api::set_option_value("relativenumber", false, &wopts);
        let _ = api::set_option_value("wrap", false, &wopts);
        let _ = api::set_option_value("signcolumn", "no", &wopts);
        let _ = api::set_option_value("foldcolumn", "0", &wopts);
        let _ = api::set_option_value("cursorline", false, &wopts);
        let _ = api::set_option_value("spell", false, &wopts);
        let _ = api::set_option_value("list", false, &wopts);
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 29 tests.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B31
git add -A
git commit -m "$(cat <<'EOF'
fix(frontend): style the preview split as a scratch preview [B31]

A vsplit copies the source window's local options, so with the common
`set number relativenumber list cursorline signcolumn=yes` the preview
rendered line numbers, listchars and a sign column beside the summary —
eating 6-8 of its ~26 columns — and long lines reflowed on every resize.

Only affects the branch that creates the split, so nothing changes for
users who never open a preview.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 10: B38 — preview width from global `&columns`

**Files:**
- Modify: `src/preview.rs` (the `if !is_open` block: capture source width before the split; the width computation)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: Task 9's option block.
- Produces: no signature change. `create_or_update_preview` now returns `Ok(())` without splitting when the source window is narrower than 40 columns.

- [ ] **Step 1: Write the failing test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_preview_does_not_crush_a_narrow_source_window() {
    use nvim_oxi::api::opts::OptionOptsBuilder;

    cleanup_preview_buffers();
    api::command("only").unwrap();

    // Pin the screen width so the assertion does not depend on the harness's
    // terminal size.
    let gopts = OptionOptsBuilder::default().build();
    api::set_option_value("columns", 80i64, &gopts).unwrap();
    let total_cols: i64 = api::get_option_value("columns", &gopts).unwrap();

    // Two vertical splits, so the source window is roughly a third of the
    // screen — the layout the finding describes.
    api::command("vsplit").unwrap();
    api::command("vsplit").unwrap();

    let source_width_before = api::get_current_win().get_width().unwrap();

    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let preview_win = api::list_wins().find(|w| {
        w.get_buf()
            .and_then(|b| b.get_name())
            .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
            .unwrap_or(false)
    });

    if let Some(preview_win) = preview_win {
        let preview_width = preview_win.get_width().unwrap();
        assert!(
            i64::from(preview_width) <= i64::from(source_width_before),
            "the preview ({preview_width} cols) took more than the window it \
             split from ({source_width_before} cols); width was computed from \
             the global &columns ({total_cols}) instead of available space"
        );
    }
    // No preview at all is the correct outcome for a very narrow source window.
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd integration_tests && cargo test test_preview_does_not_crush_a_narrow_source_window`
Expected: FAIL — the preview is forced to `columns / 3` (26), exceeding the ~19-column source window.

- [ ] **Step 3: Implement**

In `src/preview.rs`, inside `if !is_open`, capture the source width **before** the split and bail on a narrow window:

```rust
    if !is_open {
        // Capture the window we are about to split, before the split halves it.
        let source_width = api::get_current_win().get_width().unwrap_or(u32::MAX);

        // Below ~40 columns the vsplit fails outright with E36 and wrecks the
        // layout on the way. No preview is a better outcome than a broken one.
        if source_width < 40 {
            debug_log!(
                "[ttnvim] skipping preview split: source window is {} columns\n",
                source_width
            );
            return Ok(());
        }

        // Use a plain command for portability; it's fine here.
        if let Err(e) = api::command("rightbelow vsplit") {
```

Then replace the width computation:

```rust
        // ~1/3 of the screen, but never more than the window we split from can
        // spare: `columns` is global, and applying it to a window that is
        // itself only a third of the screen squeezes the user's edit window to
        // a couple of columns.
        if let Ok(total_cols) =
            api::get_option_value::<i64>("columns", &OptionOptsBuilder::default().build())
        {
            let one_third = (total_cols / 3).max(0) as u32;
            let width = one_third.min(source_width.saturating_sub(20)).max(20);
            let _ = win.set_width(width);
        }
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 30 tests.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B38
git add -A
git commit -m "$(cat <<'EOF'
fix(frontend): size the preview from available space, not &columns [B38]

The width came from the global `columns` but was applied to a window
just split off the *current* one. In an 80-column terminal with two
existing vsplits, opening a tracking file squeezed the edit window to a
couple of columns; below ~42 columns the vsplit failed outright with E36.

Clamp to what the source window can spare, and skip the split entirely
below 40 columns so a narrow terminal gets no preview rather than a
wrecked layout.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

### MILESTONE B — after Task 10

Run the full suite. On red: bisect within Tasks 5-10, revert the offender, surface the diagnosis.

---

## Task 11: B20 — preview buffer re-discovered by scanning every buffer

**Files:**
- Modify: `src/preview.rs` (imports, new thread-local, the `list_bufs` scan, `close_preview`)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: Task 8's `close_preview`.
- Produces:
  - `thread_local! { static PREVIEW_BUF: RefCell<Option<Buffer>> }` in `src/preview.rs`.
  - `fn cached_preview_buf() -> Option<Buffer>` — returns the cached handle only when `is_valid()`.
  - `fn set_cached_preview_buf(buf: Option<Buffer>)`.

- [ ] **Step 1: Write the guard test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_preview_cache_survives_a_wiped_buffer() {
    cleanup_preview_buffers();

    // First creation populates whatever cache exists.
    create_or_update_preview("first").unwrap();
    let first = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
                .unwrap_or(false)
        })
        .expect("preview buffer should exist");

    // bufhidden=wipe means the handle really can go away underneath us.
    api::command(&format!("bwipeout! {}", first.handle())).unwrap();
    assert!(!first.is_valid(), "precondition: the handle is now invalid");

    // Must not reuse the dead handle.
    let result = create_or_update_preview("second");
    assert!(result.is_ok(), "recreating after a wipe must succeed: {:?}", result);

    let second = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
                .unwrap_or(false)
        })
        .expect("a fresh preview buffer should have been created");
    assert!(second.is_valid());

    let lines: Vec<String> = second
        .get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines, vec!["second".to_string()]);
}
```

Check `Buffer::handle()` exists (`grep -n "pub fn handle" /home/steve/.cargo/git/checkouts/nvim-oxi-*/7ad27a7/crates/api/src/buffer.rs`); if not, wipe by name with `api::command("bwipeout! [Time\\ Tracking\\ Preview]")`.

- [ ] **Step 2: Run to verify it passes on unchanged code**

Run: `cd integration_tests && cargo test test_preview_cache_survives_a_wiped_buffer`
Expected: PASS. Written BEFORE the cache so the cache cannot introduce the stale-handle bug — this is the load-bearing test for invariant 1.

- [ ] **Step 3: Implement**

At the top of `src/preview.rs`, after `use super::*;`:

```rust
use std::cell::RefCell;

thread_local! {
    /// Cached handle to the preview buffer.
    ///
    /// The preview is created with `bufhidden=wipe`, so this handle can become
    /// invalid at any time — every read revalidates with `is_valid()` and falls
    /// back to a full scan. Without it, refreshing the preview cost one FFI
    /// round-trip per open buffer, on every keystroke.
    static PREVIEW_BUF: RefCell<Option<Buffer>> = const { RefCell::new(None) };
}

fn cached_preview_buf() -> Option<Buffer> {
    PREVIEW_BUF.with(|cell| {
        let mut slot = cell.borrow_mut();
        match slot.as_ref() {
            Some(buf) if buf.is_valid() => Some(buf.clone()),
            Some(_) => {
                *slot = None;
                None
            }
            None => None,
        }
    })
}

fn set_cached_preview_buf(buf: Option<Buffer>) {
    PREVIEW_BUF.with(|cell| *cell.borrow_mut() = buf);
}
```

Replace the buffer-discovery block in `create_or_update_preview`:

```rust
    // Find an existing preview buffer, preferring the cached handle.
    let preview: Option<Buffer> = cached_preview_buf().or_else(|| {
        let found = api::list_bufs().find(|b| {
            b.get_name()
                .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
                .unwrap_or(false)
        });
        if let Some(ref b) = found {
            set_cached_preview_buf(Some(b.clone()));
        }
        found
    });
```

In the `None =>` arm that creates the scratch buffer, cache it before returning:

```rust
            api::set_option_value("swapfile", false, &bopts)?;
            set_cached_preview_buf(Some(b.clone()));
            b
```

In `close_preview`, clear the cache once the preview is dealt with — add
`set_cached_preview_buf(None);` immediately before the `break;`.

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 31 tests, including `test_preview_cache_survives_a_wiped_buffer` and `test_multiple_preview_creation_updates_same_buffer`.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B20
git add -A
git commit -m "$(cat <<'EOF'
perf(caching): cache the preview buffer handle [B20]

Refreshing the preview scanned every open buffer — one FFI round-trip
plus an NvimString allocation each — to rediscover a handle that almost
never changes. With 30 buffers loaded that was 30 round-trips per
character typed.

bufhidden=wipe means the handle really can disappear, so every read
revalidates with is_valid() and falls back to the full scan.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 12: B34 — preview lookup duplicated across six call sites

**Files:**
- Modify: `src/preview.rs` (all six lookup sites)
- Test: existing preview tests, plus Task 11's cache test

**Interfaces:**
- Consumes: `cached_preview_buf` / `set_cached_preview_buf` from Task 11.
- Produces: `fn find_preview() -> Result<Option<(Buffer, Option<Window>)>>` in `src/preview.rs` — resolves the preview buffer and its containing window (if displayed) in a single pass over windows.

- [ ] **Step 1: Implement the helper**

Add to `src/preview.rs`, after `set_cached_preview_buf`:

```rust
/// Resolve the preview buffer and the window showing it, in one pass.
///
/// Returns `None` when no preview buffer exists; `Some((buf, None))` when the
/// buffer exists but is not displayed. Consolidates the six copies of this
/// lookup and gives the handle cache a single invalidation point.
fn find_preview() -> Result<Option<(Buffer, Option<Window>)>> {
    let buf = match cached_preview_buf() {
        Some(buf) => Some(buf),
        None => {
            let mut found = None;
            for b in api::list_bufs() {
                if b.get_name()?
                    .to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                {
                    found = Some(b);
                    break;
                }
            }
            if let Some(ref b) = found {
                set_cached_preview_buf(Some(b.clone()));
            }
            found
        }
    };

    let Some(buf) = buf else {
        return Ok(None);
    };

    let mut window = None;
    for w in api::list_wins() {
        if w.get_buf()? == buf {
            window = Some(w);
            break;
        }
    }

    Ok(Some((buf, window)))
}
```

- [ ] **Step 2: Route the six sites through it**

`toggle_preview_fn` — replace the window loop with:

```rust
    let has_preview = matches!(find_preview()?, Some((_, Some(_))));
```

`update_preview_fn` — same replacement.

`create_or_update_preview` — replace the Task 11 discovery block and the
separate `is_open` window loop with a single call:

```rust
    let (preview, preview_win) = match find_preview()? {
        Some((buf, win)) => (Some(buf), win),
        None => (None, None),
    };
```

then use `preview` for the `match` that creates the scratch buffer, and
`let is_open = preview_win.is_some();` for the split decision. When a fresh
scratch buffer is created, `is_open` is necessarily `false`.

`close_preview` — replace the loop with:

```rust
pub fn close_preview() -> Result<()> {
    let Some((_, Some(mut win))) = find_preview()? else {
        set_cached_preview_buf(None);
        return Ok(());
    };

    let window_count = api::list_wins().count();
    // ... the last-window handling from Task 8, operating on `win` ...

    set_cached_preview_buf(None);
    Ok(())
}
```

Preserve Task 8's exact last-window semantics — replace the buffer when
`window_count == 1`, otherwise `close(false)` with the failure logged, not
propagated.

`auto_open_preview_impl` — replace its window loop with
`let has_preview = matches!(find_preview()?, Some((_, Some(_))));`.

`auto_close_preview_impl` — delegate to `close_preview()`, keeping the existing
`log_info!("Auto-closing preview (leaving markdown file)\n");` line before the
call. The behavior is now identical, and delegating removes the sixth copy.

- [ ] **Step 3: Verify no copies remain**

Run: `grep -c 'Time Tracking Preview' src/preview.rs`
Expected: 2 — one in `find_preview`, one in `set_name`. Anything higher means a
copy survived.

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 31 tests. This is a pure refactor; every existing preview test is the regression net.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B34
git add -A
git commit -m "$(cat <<'EOF'
refactor(caching): single-pass preview lookup shared by all call sites [B34]

The "find a buffer/window named [Time Tracking Preview]" loop was
copy-pasted at six sites. On the hot path update_preview_fn walked all
windows, then create_or_update_preview walked all buffers and all windows
again — the same state fetched three times per keystroke — and because
each copy re-derived the answer there was nowhere to cache it.

find_preview() resolves buffer and window in one pass, backed by the
handle cache, and becomes the single invalidation point.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 13: B14 — preview rewritten in full on every keystroke

**Files:**
- Modify: `src/preview.rs` (new thread-local, the write block, cache clears)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: `find_preview`, `set_cached_preview_buf` from Tasks 11-12.
- Produces: `thread_local! { static LAST_OUTPUT: RefCell<Option<String>> }`, `fn set_last_output(Option<String>)` and `fn last_output_matches(&str) -> bool` in `src/preview.rs`.

- [ ] **Step 1: Write the failing test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_identical_output_does_not_rewrite_the_preview_buffer() {
    cleanup_preview_buffers();

    create_or_update_preview("# Summary\n- total: 1h").unwrap();
    let buf = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
                .unwrap_or(false)
        })
        .expect("preview buffer should exist");

    let tick_after_first = buf.get_changedtick().unwrap();

    // The overwhelming majority of keystrokes leave the rendered summary
    // unchanged; rewriting yanks scroll position and repaints the split.
    create_or_update_preview("# Summary\n- total: 1h").unwrap();
    assert_eq!(
        buf.get_changedtick().unwrap(),
        tick_after_first,
        "an identical render must not rewrite the buffer"
    );

    // A genuinely different render must still write.
    create_or_update_preview("# Summary\n- total: 2h").unwrap();
    assert!(
        buf.get_changedtick().unwrap() > tick_after_first,
        "a changed render must write"
    );

    let lines: Vec<String> = buf
        .get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines, vec!["# Summary".to_string(), "- total: 2h".to_string()]);
}

#[nvim_oxi::test]
fn test_recreated_preview_always_gets_a_full_write() {
    cleanup_preview_buffers();

    create_or_update_preview("# Summary\n- total: 1h").unwrap();
    // Wipe it, then render the SAME content: a stale output cache would skip
    // the write and leave the new buffer empty.
    cleanup_preview_buffers();
    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let buf = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
                .unwrap_or(false)
        })
        .expect("preview buffer should exist");

    let lines: Vec<String> = buf
        .get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        lines,
        vec!["# Summary".to_string(), "- total: 1h".to_string()],
        "a wiped-and-recreated preview must be written in full"
    );
}
```

- [ ] **Step 2: Run to verify the first one fails**

Run: `cd integration_tests && cargo test test_identical_output_does_not_rewrite_the_preview_buffer`
Expected: FAIL — the changedtick advances on the identical second render.

Run: `cd integration_tests && cargo test test_recreated_preview_always_gets_a_full_write`
Expected: PASS (characterization — the guard against the cache-invalidation bug the fix could introduce).

- [ ] **Step 3: Implement**

Add alongside `PREVIEW_BUF` in `src/preview.rs`:

```rust
thread_local! {
    /// The last output successfully written to the preview buffer.
    ///
    /// Cleared whenever the preview buffer is created or destroyed, so a
    /// wiped-and-recreated preview always gets a full write.
    static LAST_OUTPUT: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_last_output(output: Option<String>) {
    LAST_OUTPUT.with(|cell| *cell.borrow_mut() = output);
}

fn last_output_matches(output: &str) -> bool {
    LAST_OUTPUT.with(|cell| cell.borrow().as_deref() == Some(output))
}
```

Replace the write block in `create_or_update_preview`:

```rust
    // Update buffer contents, skipping the rewrite when nothing changed.
    // The rendered day summary is unchanged for most keystrokes, and rewriting
    // yanks the preview's scroll position and repaints the whole split.
    if !(last_output_matches(output) && buf.is_valid()) {
        let bopts = OptionOptsBuilder::default().buf(buf.clone()).build();
        api::set_option_value("modifiable", true, &bopts)?;
        let lines: Vec<String> = output.lines().map(|s| s.to_string()).collect();
        buf.set_lines(0..buf.line_count()?, false, lines)?;
        api::set_option_value("modifiable", false, &bopts)?;
        set_last_output(Some(output.to_owned()));
    }
```

Clear the output cache at both lifecycle points:
- in the `None =>` arm that creates the scratch buffer, next to
  `set_cached_preview_buf(Some(b.clone()));`, add `set_last_output(None);`
- in `close_preview`, next to each `set_cached_preview_buf(None);`, add
  `set_last_output(None);`

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 33 tests.

Watch `test_create_or_update_preview_updates_existing_buffer` and
`test_multiple_preview_creation_updates_same_buffer` closely: if either writes
the same content twice and asserts on the result, the skip is correct and the
assertion should still hold. If one goes red, the cache is not being invalidated
where it must be — fix the invalidation, do not weaken the test.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B14
git add -A
git commit -m "$(cat <<'EOF'
perf(caching): skip the preview rewrite when the render is unchanged [B14]

Every keystroke performed two set_option_value FFI calls, allocated a
String per output line, and replaced the entire preview buffer — plus the
redraw Neovim schedules for it — even though the rendered day summary is
unchanged for the overwhelming majority of edits. Visible as scroll
position yanked toward the top and the split repainting (obvious over
ssh/tmux).

Cache invalidates on scratch-buffer creation and in close_preview, so a
wiped-and-recreated preview always gets a full write.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

### MILESTONE C — after Task 13

Run the full suite. On red: bisect within Tasks 11-13, revert the offender, surface the diagnosis.

---

## Task 14: B13 — `TextChanged` autocmd pattern `*`

**Files:**
- Modify: `src/lib.rs:199`
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: no signature change.

- [ ] **Step 1: Pin the invariant the change relies on**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_only_md_files_can_be_tracking_files() {
    // Invariant 2 (see the code-health spec): narrowing the TextChanged
    // autocmd pattern from `*` to `*.md` is behavior-preserving ONLY because
    // is_buf_time_tracking_file already requires a .md extension. Pin it here
    // so a later change that relaxes the extension check fails loudly instead
    // of silently disabling live updates for the newly-allowed extensions.
    let (config, temp_dir) = create_test_config_with_temp_dir();

    for name in ["notes.txt", "notes.markdown", "notes", "notes.md.bak"] {
        let file = create_test_file(temp_dir.path(), name, "content");
        let mut buf = api::create_buf(false, false).unwrap();
        buf.set_name(&file).unwrap();
        assert!(
            !is_buf_time_tracking_file(buf, &config).unwrap(),
            "{name} must not be a tracking file — the TextChanged autocmd only \
             fires for *.md"
        );
    }

    let md = create_test_file(temp_dir.path(), "notes.md", "content");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    assert!(is_buf_time_tracking_file(buf, &config).unwrap());
}
```

- [ ] **Step 2: Run to verify it passes**

Run: `cd integration_tests && cargo test test_only_md_files_can_be_tracking_files`
Expected: PASS.

- [ ] **Step 3: Implement**

In `src/lib.rs`, change line 199:

```rust
    api::command("autocmd TextChanged,TextChangedI *.md TimeTrackingUpdate")?;
```

- [ ] **Step 4: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 34 tests.

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B13
git add -A
git commit -m "$(cat <<'EOF'
perf(caching): narrow the TextChanged autocmd to *.md [B13]

The pattern was `*`, so typing in any buffer — a Rust source file, a git
commit message, a help buffer — crossed the FFI boundary and paid
get_current_buf + get_name + up to two canonicalize syscalls before
bailing out.

Behavior-preserving: is_buf_time_tracking_file already requires a .md
extension, so no non-markdown buffer could ever produce a preview. That
invariant now has its own test.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 15: B22 — `WinClosed` counts the window being closed

**Files:**
- Modify: `src/utils.rs` (`any_tracking_visible` signature)
- Modify: `src/lib.rs` (the `maybe_close_if_invisible` command; the autocmd lines)
- Modify: `integration_tests/src/lib.rs` (mechanical call-site updates — the only permitted edits to existing tests in this batch)

**Interfaces:**
- Consumes: nothing.
- Produces: `any_tracking_visible(config: &Config, exclude_win: Option<i32>) -> Result<bool>` — **signature change**. The handle type must match `Window::handle()`'s return type; check it and use exactly that type everywhere, including in the tests.

- [ ] **Step 1: Write the failing test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_any_tracking_visible_skips_the_excluded_window() {
    let (config, temp_dir) = create_test_config_with_temp_dir();

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    let win = api::get_current_win();
    let handle = win.handle();

    assert!(
        any_tracking_visible(&config, None).unwrap(),
        "the tracking window is visible when nothing is excluded"
    );

    // WinClosed fires "just before it is removed from the window layout", so
    // the closing window is still in list_wins() when the handler runs.
    assert!(
        !any_tracking_visible(&config, Some(handle)).unwrap(),
        "the window being closed must not count itself as still visible"
    );
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cd integration_tests && cargo test test_any_tracking_visible_skips_the_excluded_window`
Expected: FAIL to COMPILE — `any_tracking_visible` takes one argument. That is the RED state for a signature change.

- [ ] **Step 3: Implement the signature change**

In `src/utils.rs`:

```rust
/// Is any window showing a time-tracking file?
///
/// `exclude_win` skips one window by handle. `WinClosed` fires *before* the
/// window leaves the layout, so the handler must not let the window being
/// closed vote for keeping the preview open.
pub fn any_tracking_visible(config: &Config, exclude_win: Option<i32>) -> Result<bool> {
    for win in api::list_wins() {
        if Some(win.handle()) == exclude_win {
            continue;
        }

        let buf = win.get_buf()?;
        let name = buf.get_name()?;

        // Skip the preview itself
        if name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            continue;
        }

        if is_win_time_tracking_file(win, config)? {
            return Ok(true);
        }
    }
    Ok(false)
}
```

Confirm `Window::handle()`'s return type
(`grep -n "pub fn handle" /home/steve/.cargo/git/checkouts/nvim-oxi-*/7ad27a7/crates/api/src/window.rs`)
and make `exclude_win: Option<T>` use exactly that type.

- [ ] **Step 4: Pass the closing window through from the autocommand**

In `src/lib.rs`, make the command accept an optional argument:

```rust
    let maybe_close_if_invisible = Function::from_fn(move |args: CommandArgs| {
        catch_nvim_panic(move || {
            // WinClosed sets <amatch> to the window-ID of the window that is
            // about to be removed. BufEnter/TabEnter set it to a buffer name,
            // so those fire the command with no argument.
            let exclude = args
                .args
                .as_deref()
                .and_then(|s| s.trim().parse().ok());

            if !any_tracking_visible(config, exclude)? {
                close_preview()?;
            }
            Ok(())
        })
    });

    api::create_user_command(
        "TimeTrackingMaybeCloseIfInvisible",
        maybe_close_if_invisible,
        &CreateCommandOpts::builder()
            .nargs(CommandNArgs::ZeroOrOne)
            .build(),
    )?;
```

Add the import: `use nvim_oxi::api::types::CommandNArgs;` alongside the existing
`CommandArgs` import.

Split the autocommand so only `WinClosed` passes `<amatch>`:

```rust
    api::command("autocmd BufEnter,TabEnter * TimeTrackingMaybeCloseIfInvisible")?;
    api::command("autocmd WinClosed * TimeTrackingMaybeCloseIfInvisible <amatch>")?;
```

(replacing the single `BufEnter,WinClosed,TabEnter` line).

- [ ] **Step 5: Update the existing call sites**

Three existing tests call `any_tracking_visible(&config)`:
`test_any_tracking_visible_with_tracking_window`,
`test_any_tracking_visible_with_preview_window`,
`test_any_tracking_visible_no_tracking_files`.

Change each to `any_tracking_visible(&config, None)`. This is a mechanical
call-site update forced by the signature change — do not alter what any of them
asserts.

- [ ] **Step 6: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 35 tests.

- [ ] **Step 7: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 8: Strip and commit**

```bash
todo-parser bughunt.md --strip B22
git add -A
git commit -m "$(cat <<'EOF'
fix(correctness): do not let the closing window keep the preview open [B22]

:help WinClosed says it fires "just before it is removed from the window
layout", so list_wins() still contained the closing window when the
handler ran. Closing the last tracking window with <C-w>c or :close
(neither fires QuitPre, so the fallback did not run either) left an
orphaned preview showing stale data for a file no longer on screen.

Pass <amatch> — the window-ID WinClosed sets — through to
any_tracking_visible so that window is skipped, mirroring the existing
skip-the-preview branch. BufEnter/TabEnter, whose <amatch> is a buffer
name, fire the command with no argument.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

### MILESTONE D — after Task 15

Run the full suite. On red: bisect within Tasks 14-15, revert the offender, surface the diagnosis.

---

## Task 16: B8 — config load failure registers zero commands, silently

**Files:**
- Modify: `src/lib.rs:110-124`
- Modify: `lua/time-tracking-nvim/init.lua` (the three `pcall(require, ...)` sites)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: the plugin entry returns a `Dictionary` carrying an `error` key (a string) when initialization failed; empty otherwise. `init.lua` reads `native.error`.

- [ ] **Step 1: Write the test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_successful_init_returns_a_dictionary_without_an_error_key() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let dict = time_tracking_with_config(config_static).unwrap();
    assert!(
        dict.get("error").is_none(),
        "a successful init must not advertise an error"
    );
}
```

The failure path cannot be reached from inside `#[nvim_oxi::test]` — it requires
`Config::try_get_no_args()` to fail, which happens before the test harness gets
control. This test pins the success contract so the new `error` key cannot leak
into it; the failure path is verified manually in Step 5.

- [ ] **Step 2: Run to verify it passes**

Run: `cd integration_tests && cargo test test_successful_init_returns_a_dictionary_without_an_error_key`
Expected: PASS. Confirm the accessor name with
`grep -n "pub fn get" /home/steve/.cargo/git/checkouts/nvim-oxi-*/7ad27a7/crates/types/src/dictionary.rs`
and adjust if it differs.

- [ ] **Step 3: Implement the Rust side**

In `src/lib.rs`, replace the final `match result` block:

```rust
    // Never return Err: push_error → lua_error throws a C++ exception on macOS
    // (LUAJIT_UNWIND_EXTERNAL) which hits the nounwind terminate block →
    // panic_cannot_unwind. Report the failure through the returned dictionary
    // and :messages instead, so the Lua layer can stop claiming success.
    match result {
        Ok(Ok(dict)) => Ok(dict),
        Ok(Err(e)) => Ok(init_failure_dict(&format!("{e}"))),
        Err(payload) => Ok(init_failure_dict(&panic_message(payload))),
    }
}

/// Build the dictionary returned when initialization failed, and make the
/// reason visible in `:messages` — without it the user gets a plugin that
/// loads cleanly, registers nothing, and answers `:TimeTrackingToggle` with
/// `E492: Not an editor command`.
fn init_failure_dict(msg: &str) -> Dictionary {
    api::err_writeln(&format!("[time-tracking-nvim] failed to initialize: {msg}"));
    debug_log!("[ttnvim] init failed: {}\n", msg);

    Dictionary::from_iter([("error", msg)])
}
```

If `Dictionary` has no `from_iter` for that shape, build it with
`let mut dict = Dictionary::new(); dict.insert("error", msg); dict` — check the
type's API and use whichever exists.

- [ ] **Step 4: Implement the Lua side**

In `lua/time-tracking-nvim/init.lua`, each of the three
`local ok, native = pcall(require, "time_tracking_nvim")` sites currently treats
`ok` alone as success. For the plain `M.setup` site near the end:

```lua
	-- Load the native module
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		vim.api.nvim_echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Failed to load native module: " .. native, "Normal" },
			{ "\nMake sure the plugin is properly installed and the dynamic library is available", "Normal" },
		}, false, {})
		return
	end

	if type(native) == "table" and native.error then
		vim.api.nvim_echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Native module loaded but failed to initialize: " .. tostring(native.error), "Normal" },
			{ "\nNo commands were registered. Check your time-tracking-cli configuration.", "Normal" },
		}, false, {})
		return
	end
end
```

For the two post-download sites (the `else` branches echoing "Plugin loaded
successfully!" and "Plugin updated and loaded successfully!"), guard the success
message:

```lua
				else
					if type(native) == "table" and native.error then
						vim.api.nvim_echo({
							{ "time-tracking-nvim: ", "ErrorMsg" },
							{ "Loaded but failed to initialize: " .. tostring(native.error), "Normal" },
						}, false, {})
					else
						vim.api.nvim_echo({
							{ "time-tracking-nvim: ", "MoreMsg" },
							{ "Plugin loaded successfully!", "Normal" },
						}, false, {})
					end
				end
```

(and the matching "Plugin updated and loaded successfully!" at the update site).

Task 24 rewrites all of these echo calls to go through a helper — leave the
`nvim_echo` shape as-is here and let Task 24 convert them.

- [ ] **Step 5: Verify manually**

Build, then run with a broken or absent time-tracking-cli config:

```bash
nvim -c 'lua require("time-tracking-nvim").setup({auto_download=false, auto_update=false})' \
     -c 'messages'
```

Expected: a `failed to initialize` line, and NOT "Plugin loaded successfully!".
`:TimeTrackingToggle` should still report `E492`, but now the reason is on
screen. If a broken config cannot be produced locally, say so in the report.

- [ ] **Step 6: Full verification**

Run the Global Constraints verification command set. Expected: all green, 36 integration tests.

- [ ] **Step 7: Strip and commit**

```bash
todo-parser bughunt.md --strip B8
git add -A
git commit -m "$(cat <<'EOF'
fix(api-surface): surface a config-load failure instead of swallowing it [B8]

When Config::try_get_no_args() returned Err the entry point wrote to
process stderr — invisible in a TUI, or screen-garbling — and returned
an empty Dictionary, so no commands were created. Lua's pcall succeeded,
init.lua printed "Plugin loaded successfully!", and :TimeTrackingToggle
answered E492 with nothing anywhere explaining why.

Report through err_writeln and an `error` key on the returned dictionary;
init.lua now checks it at all three load sites. The never-return-Err
choice is deliberate and unchanged.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 17: B3 — no debounce on the per-keystroke live-update path

**Files:**
- Modify: `Cargo.toml` (add the `libuv` feature)
- Modify: `src/preview.rs` (debounced entry point)
- Modify: `src/lib.rs` (a separate autocmd-driven command; widen `pub use`)
- Test: `integration_tests/src/lib.rs`

**Interfaces:**
- Consumes: `update_preview_fn` from Task 12.
- Produces:
  - `pub fn update_preview_debounced(config: &'static Config) -> Result<()>` in `src/preview.rs`, added to the `pub use` in `src/lib.rs`.
  - a new user command `TimeTrackingUpdateDebounced`, which the `TextChanged` autocmd calls. `TimeTrackingUpdate` keeps rendering immediately.

**Prerequisite already verified:** `libuv::TimerHandle::once(Duration, cb)` and
`TimerHandle::stop(&mut self)` both exist on the pinned nvim-oxi revision
(`crates/libuv/src/timer.rs:70` and `:86`). If enabling the feature breaks the
build for a reason not resolvable inside this task, STOP: convert B3 to a
`decision-needed` marker in `bughunt.md` and skip it rather than inventing a
substitute mechanism.

- [ ] **Step 1: Enable the feature and confirm it builds**

In `Cargo.toml`:

```toml
nvim-oxi = { git = "https://github.com/noib3/nvim-oxi", branch = "main", features = [
    "neovim-0-12",
    "libuv",
] }
```

Run: `cargo build`
Expected: success. If it fails, STOP and follow the decision-needed path above.

- [ ] **Step 2: Write the test**

Append to `integration_tests/src/lib.rs`:

```rust
#[nvim_oxi::test]
fn test_explicit_update_renders_immediately() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    // Open the preview so update_preview_fn has something to refresh.
    create_or_update_preview("initial").unwrap();

    // :TimeTrackingUpdate must render synchronously — a user who types the
    // command expects to see the result, not to wait out a debounce window.
    time_tracking_nvim::update_preview_fn(config_static).unwrap();

    let preview = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| n.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")))
                .unwrap_or(false)
        })
        .expect("preview buffer should exist");
    assert!(preview.is_valid());
}

#[nvim_oxi::test]
fn test_debounced_update_returns_without_blocking() {
    use std::time::Instant;

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    // Simulate a burst of keystrokes: each re-arms the timer and returns at
    // once; none of them may block for the debounce interval.
    let start = Instant::now();
    for _ in 0..20 {
        time_tracking_nvim::update_preview_debounced(config_static).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 100,
        "20 debounced updates took {:?}; the debounce must not block the \
         event loop",
        elapsed
    );
}
```

- [ ] **Step 3: Run to verify the second fails**

Run: `cd integration_tests && cargo test test_debounced_update_returns_without_blocking`
Expected: FAIL to COMPILE — `update_preview_debounced` does not exist yet.

- [ ] **Step 4: Implement the debounce**

In `src/preview.rs`:

```rust
use nvim_oxi::libuv::TimerHandle;
use std::time::Duration;

/// Trailing-edge debounce interval for autocommand-driven updates.
const DEBOUNCE: Duration = Duration::from_millis(150);

thread_local! {
    /// In-flight debounce timer, if any. Re-armed on each keystroke so the
    /// render happens once the user pauses rather than once per character.
    static PENDING_UPDATE: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
}

/// Autocommand entry point: coalesce a burst of keystrokes into one render.
///
/// `:TimeTrackingUpdate` still calls `update_preview_fn` directly, because a
/// user who types the command expects to see the result immediately.
pub fn update_preview_debounced(config: &'static Config) -> Result<()> {
    PENDING_UPDATE.with(|cell| {
        if let Some(timer) = cell.borrow_mut().as_mut() {
            let _ = timer.stop();
        }
    });

    let timer = TimerHandle::once(DEBOUNCE, move || {
        PENDING_UPDATE.with(|cell| *cell.borrow_mut() = None);
        if let Err(e) = update_preview_fn(config) {
            log_error!("[time-tracking-nvim] debounced update failed: {}", e);
        }
    })
    .map_err(|e| {
        nvim_oxi::Error::Api(api::Error::Other(format!(
            "could not arm the update timer: {e}"
        )))
    })?;

    PENDING_UPDATE.with(|cell| *cell.borrow_mut() = Some(timer));
    Ok(())
}
```

`TimerHandle::once`'s callback signature and error type must be matched exactly
— read `crates/libuv/src/timer.rs:70` and adapt the closure (it may take a
handle argument and/or require a specific return type). Do not guess. `config`
is already `&'static Config`, so it satisfies any `'static` capture bound.

- [ ] **Step 5: Wire the autocommand to the debounced command**

In `src/lib.rs`, add the re-export and a command alongside `TimeTrackingUpdate`:

```rust
    let update_preview_debounced_cmd = Function::from_fn(move |_: CommandArgs| {
        catch_nvim_panic(|| update_preview_debounced(config))
    });

    api::create_user_command(
        "TimeTrackingUpdateDebounced",
        update_preview_debounced_cmd,
        &CreateCommandOpts::builder().build(),
    )?;
```

and point the autocommand at it (keeping `TimeTrackingUpdate` for explicit use):

```rust
    api::command("autocmd TextChanged,TextChangedI *.md TimeTrackingUpdateDebounced")?;
```

Add `update_preview_debounced` to the `pub use preview::{...}` list.

- [ ] **Step 6: Run to verify it passes**

Run: `cd integration_tests && cargo test`
Expected: PASS — 38 tests.

`test_time_tracking_with_config_creates_commands` iterates a list of expected
commands. It must keep passing; if it asserts an exact command count, add
`TimeTrackingUpdateDebounced` to its list — a mechanical update forced by the
new command, not a weakening.

- [ ] **Step 7: Verify the debounce by hand**

Build, open a real tracking file, hold a key down, and confirm the preview
updates once you stop rather than flickering per character. Then run
`:TimeTrackingUpdate` and confirm it renders immediately.

- [ ] **Step 8: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 9: Strip and commit**

```bash
todo-parser bughunt.md --strip B3
git add -A
git commit -m "$(cat <<'EOF'
perf(caching): debounce autocommand-driven preview updates [B3]

TextChanged,TextChangedI fired once per keystroke, synchronously on
Neovim's single UI thread, with zero coalescing — each fire paying
canonicalize syscalls, an FFI window scan, a full-buffer read and join, a
boxed formatter, a full re-parse of the buffer, and a complete rewrite of
the preview. Holding a key down in a large tracking note lagged visibly,
against a README that advertises minimal overhead.

Adds a 150ms trailing-edge debounce on a libuv one-shot timer behind a
new TimeTrackingUpdateDebounced command, which the autocommand now calls.
:TimeTrackingUpdate still renders immediately.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

### MILESTONE E — after Task 17

Run the full suite. This is the end of the Rust work. On red: bisect within Tasks 16-17, revert the offender, surface the diagnosis.

---

## Task 18: B30 — Windows platform detection never matches

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` (`get_platform_info`, `M._internal`)
- Create: `integration_tests/lua/spec_platform.lua`
- Modify: `integration_tests/lua/run_lua_tests.sh` (add the spec to the list)

**Interfaces:**
- Consumes: Task 0's harness and `M._internal`.
- Produces: `normalize_os_name(os_name: string) -> string`, added to `M._internal`.

- [ ] **Step 1: Write the failing spec**

Create `integration_tests/lua/spec_platform.lua`:

```lua
local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal

H.describe("normalize_os_name", function()
  H.it("maps libuv's Windows_NT to windows", function()
    -- uv.os_uname().sysname returns "Windows_NT" on Windows; lowercased that
    -- is "windows_nt", which was never a key in platform_mappings, so the
    -- shipped x86_64-pc-windows-msvc build was unreachable.
    H.eq(internal.normalize_os_name("windows_nt"), "windows")
  end)

  H.it("maps mingw and msys variants to windows", function()
    H.eq(internal.normalize_os_name("mingw64_nt-10.0"), "windows")
    H.eq(internal.normalize_os_name("msys_nt-10.0"), "windows")
  end)

  H.it("leaves linux and darwin alone", function()
    H.eq(internal.normalize_os_name("linux"), "linux")
    H.eq(internal.normalize_os_name("darwin"), "darwin")
  end)
end)

return H
```

- [ ] **Step 2: Add the spec to the runner**

In `integration_tests/lua/run_lua_tests.sh`, change the spec list to:

```lua
    for _, spec in ipairs({ 'spec_version', 'spec_platform' }) do
```

- [ ] **Step 3: Run to verify it fails**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: FAIL — `attempt to call a nil value` for `normalize_os_name`, non-zero exit.

- [ ] **Step 4: Implement**

In `lua/time-tracking-nvim/init.lua`, add the helper above `get_platform_info`:

```lua
-- Normalize libuv's sysname to the keys used in platform_mappings.
-- uv.os_uname() mimics uname, so Windows reports "Windows_NT" (and MSYS/MinGW
-- shells report "MINGW64_NT-…"/"MSYS_NT-…"), none of which is "windows".
local function normalize_os_name(os_name)
	if os_name:match("^windows") or os_name:match("^mingw") or os_name:match("^msys") then
		return "windows"
	end
	return os_name
end
```

and use it inside `get_platform_info`:

```lua
local function get_platform_info()
	local os_name = normalize_os_name(uv.os_uname().sysname:lower())
	local arch = uv.os_uname().machine:lower()
```

Add it to `M._internal`:

```lua
M._internal = {
	is_version_newer = is_version_newer,
	get_platform_info = get_platform_info,
	normalize_os_name = normalize_os_name,
}
```

- [ ] **Step 5: Run to verify it passes**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: `10 passed, 0 failed`.

- [ ] **Step 6: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 7: Strip and commit**

```bash
todo-parser bughunt.md --strip B30
git add -A
git commit -m "$(cat <<'EOF'
fix(correctness): detect Windows, whose uname reports Windows_NT [B30]

uv.os_uname().sysname returns "Windows_NT"; lowercased that is
"windows_nt", which was never a key in platform_mappings. get_platform_info
returned "Unsupported platform: windows_nt-x86_64", get_binary_path
returned nil, and setup() aborted before adding anything to cpath — so
the entire `windows` branch of the mapping table was dead code and the
published x86_64-pc-windows-msvc asset was unreachable.

Normalize the OS name (windows/mingw/msys) alongside the existing arch
normalization.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 19: B11 — `PLUGIN_VERSION` drift, with nothing enforcing the sync

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua:8`
- Modify: `.github/workflows/ci.yml` (new `version-sync` job)
- Modify: `.github/workflows/release.yml` (version check before the build matrix)
- Modify: `DEVELOPMENT.md` (Release Process step 1)

**Interfaces:**
- Consumes: nothing.
- Produces: invariant 4 becomes CI-enforced.

- [ ] **Step 1: Write the check as a script first, and watch it fail**

Run this against the current tree:

```bash
cargo_version=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
lua_version=$(grep -m1 'PLUGIN_VERSION = ' lua/time-tracking-nvim/init.lua | sed -E 's/.*"([^"]+)".*/\1/')
echo "Cargo.toml: $cargo_version"
echo "init.lua:   $lua_version"
[ "$cargo_version" = "$lua_version" ]
echo "exit=$?"
```

Expected: `Cargo.toml: 0.1.7`, `init.lua: 0.1.4`, non-zero exit. That is the RED state — the drift is real and the check catches it.

- [ ] **Step 2: Fix the drift**

In `lua/time-tracking-nvim/init.lua` line 8:

```lua
local PLUGIN_VERSION = "0.1.7"
```

- [ ] **Step 3: Re-run the check**

Run the Step 1 script again. Expected: both `0.1.7`, exit 0.

- [ ] **Step 4: Enforce it in CI**

In `.github/workflows/ci.yml`, add a job (sibling to `test` and `security`):

```yaml
  version-sync:
    name: Version sync
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Check Cargo.toml and init.lua agree
        run: |
          cargo_version=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
          lua_version=$(grep -m1 'PLUGIN_VERSION = ' lua/time-tracking-nvim/init.lua | sed -E 's/.*"([^"]+)".*/\1/')
          echo "Cargo.toml: ${cargo_version}"
          echo "init.lua:   ${lua_version}"
          if [ "${cargo_version}" != "${lua_version}" ]; then
            echo "::error::PLUGIN_VERSION (${lua_version}) does not match Cargo.toml version (${cargo_version})"
            exit 1
          fi
```

Task 27 replaces `actions/checkout@v4` here with a pinned SHA — leave the tag
for now so the two changes stay separable.

- [ ] **Step 5: Enforce it at release time too**

In `.github/workflows/release.yml`, add a job that `build` depends on:

```yaml
  version-check:
    name: Version check
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Check tag, Cargo.toml and init.lua all agree
        run: |
          cargo_version=$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
          lua_version=$(grep -m1 'PLUGIN_VERSION = ' lua/time-tracking-nvim/init.lua | sed -E 's/.*"([^"]+)".*/\1/')
          if [ "${{ github.event_name }}" = "workflow_dispatch" ]; then
            tag="${{ github.event.inputs.tag }}"
          else
            tag="${GITHUB_REF#refs/tags/}"
          fi
          tag_version="${tag#v}"
          echo "tag: ${tag_version}  Cargo.toml: ${cargo_version}  init.lua: ${lua_version}"
          if [ "${cargo_version}" != "${lua_version}" ] || [ "${cargo_version}" != "${tag_version}" ]; then
            echo "::error::version mismatch — tag ${tag_version}, Cargo.toml ${cargo_version}, init.lua ${lua_version}"
            exit 1
          fi
```

and add `needs: version-check` to the existing `build` job.

- [ ] **Step 6: Update the release documentation**

In `DEVELOPMENT.md`, Release Process step 1 currently names only `Cargo.toml`.
Change it to name both files explicitly, e.g.:

```markdown
1. Bump the version in **both** `Cargo.toml` (`version = "X.Y.Z"`) and
   `lua/time-tracking-nvim/init.lua` (`PLUGIN_VERSION = "X.Y.Z"`). CI fails if
   they disagree, and the release workflow additionally requires the git tag to
   match.
```

Read the surrounding numbered list first and match its existing wording and formatting.

- [ ] **Step 7: Full verification**

Run the Global Constraints verification command set, plus the Step 1 script. Expected: all green, versions equal.

Also sanity-check the workflow YAML parses:
```bash
python3 -c "
import yaml
for f in ['.github/workflows/ci.yml', '.github/workflows/release.yml']:
    yaml.safe_load(open(f)); print('ok', f)
"
```

- [ ] **Step 8: Strip and commit**

```bash
todo-parser bughunt.md --strip B11
git add -A
git commit -m "$(cat <<'EOF'
fix(api-surface): resync PLUGIN_VERSION and enforce it in CI [B11]

init.lua said 0.1.4 while Cargo.toml said 0.1.7 — three releases of
drift, because DEVELOPMENT.md's release process named only Cargo.toml and
nothing checked. The consequence was worse than a wrong number: a fresh
install downloaded the 0.1.7 asset and stamped the sidecar 0.1.4, after
which read_binary_version() always matched, needs_update stayed false
forever, and the auto-update mechanism was permanently disarmed — while
version_info() reported the wrong version into every bug report.

Adds a version-sync CI job and a release-time check that also requires
the pushed tag to match, so the invariant is pinned by a check rather
than by prose.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 20: B18 — always fetches `/releases/latest`, stamps the requested version

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` (`download_binary`: the API URL and the version-file write)

**Interfaces:**
- Consumes: nothing.
- Produces: `download_binary(target, binary_path, callback, expected_version)` — unchanged signature; now requests a tag-specific endpoint when `expected_version` is set and records the resolved `tag_name`.

- [ ] **Step 1: Implement the tag-specific request**

In `download_binary`, replace the hardcoded URL:

```lua
local function download_binary(target, binary_path, callback, expected_version)
	-- Ask for the release we actually want. Falling back to /latest only when
	-- no version was requested: previously this always fetched /latest and then
	-- recorded expected_version, so the .version file was an assertion about
	-- what we wanted rather than an observation of what we got.
	local api_base = "https://api.github.com/repos/stevenwcarter/time-tracking-nvim/releases"
	local release_url = expected_version and (api_base .. "/tags/v" .. expected_version)
		or (api_base .. "/latest")

	local cmd = {
		"curl",
		"-L",
		"-s",
		release_url,
	}
```

Task 21 hardens this argv further — leave the flags as they are here.

- [ ] **Step 2: Record what was actually downloaded**

Replace the version-file write:

```lua
									-- Record the tag we actually downloaded, not the one we asked
									-- for: with a pinned plugin tag these can differ, and recording
									-- the request made every later version comparison a no-op.
									local resolved_tag = release_info.tag_name
									local version_to_store = resolved_tag and (resolved_tag:gsub("^v", "")) or "unknown"

									if expected_version and version_to_store ~= expected_version then
										vim.api.nvim_echo({
											{ "time-tracking-nvim: ", "WarningMsg" },
											{
												string.format(
													"requested v%s but the release resolved to %s; recording %s",
													expected_version,
													tostring(resolved_tag),
													version_to_store
												),
												"Normal",
											},
										}, false, {})
									end

									if not write_binary_version(version_to_store) then
										-- Not a fatal error, just warn
										vim.api.nvim_echo({
											{ "time-tracking-nvim: ", "WarningMsg" },
											{ "Warning: Could not save version info", "Normal" },
										}, false, {})
									end
```

Two details that matter:
- The `:gsub("^v", "")` is wrapped in parentheses so only the string is
  assigned — `gsub` returns two values, and without the parens
  `version_to_store` would silently pick up the replacement count in some
  assignment contexts.
- Stripping the `v` is required: `PLUGIN_VERSION` has no prefix but `tag_name`
  does, and `read_binary_version()` is compared directly against
  `PLUGIN_VERSION`. Without the strip, every start would see a mismatch and
  re-download.

- [ ] **Step 3: Verify the version round-trip by hand**

```bash
rm -f lua/time_tracking_nvim.so.version
nvim --headless -c 'lua require("time-tracking-nvim").download()' -c 'sleep 15' -c 'qall!'
cat lua/time_tracking_nvim.so.version
```

Expected: `0.1.7` (no `v` prefix), matching `PLUGIN_VERSION` after Task 19.

v0.1.7 exists as a release, so `/releases/tags/v0.1.7` resolves. If it does not
(network unavailable, or the tag is absent), say so explicitly rather than
reporting a pass — and confirm the fallback path by temporarily calling
`download_binary` with `expected_version = nil`.

- [ ] **Step 4: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 5: Strip and commit**

```bash
todo-parser bughunt.md --strip B18
git add -A
git commit -m "$(cat <<'EOF'
fix(correctness): download the requested release and record what arrived [B18]

The URL was hardcoded to /releases/latest while the .version file was
written from the caller-supplied expected_version, and tag_name was never
compared against it — so the recorded version was an assertion, never an
observation. Pinning the plugin to an old tag downloaded the latest
binary, stamped the pinned version, and reported "versions match" about a
different native module.

Request /releases/tags/v<expected_version> when a version is known,
falling back to /latest only when it is not; record the resolved tag
(with its v prefix stripped, since PLUGIN_VERSION carries none) and warn
naming both when they differ.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 21: B12 — curl invocations lack failure, protocol and timeout hardening

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` (both `vim.system` curl calls; the API-response guard)

**Interfaces:**
- Consumes: Task 20's `release_url`.
- Produces: a local `CURL_HARDENING` table and a `curl_cmd(extra)` helper, shared by both invocations and by Task 23's SHA256SUMS fetch.

- [ ] **Step 1: Implement the shared flag set**

Above `download_binary` in `lua/time-tracking-nvim/init.lua`:

```lua
-- Shared curl hardening for both the API call and the archive download.
--   --proto/--proto-redir =https  : curl's default redirect protocol set
--                                   includes plain HTTP, so a 302 to http://
--                                   would fetch the library we are about to
--                                   dlopen in cleartext.
--   --fail-with-body              : without -f curl exits 0 on an HTTP error,
--                                   so a 403 rate-limit body parsed as JSON
--                                   and surfaced as "unsupported platform".
--   --max-time/--connect-timeout  : a black-holed connection otherwise left
--                                   the callback pending forever.
local CURL_HARDENING = {
	"--proto",
	"=https",
	"--proto-redir",
	"=https",
	"--tlsv1.2",
	"--fail-with-body",
	"--max-redirs",
	"5",
	"--connect-timeout",
	"10",
	"--max-time",
	"60",
	"--retry",
	"2",
}

-- Build a curl argv: {"curl", <hardening>, unpack(extra)}
local function curl_cmd(extra)
	local cmd = { "curl" }
	vim.list_extend(cmd, CURL_HARDENING)
	vim.list_extend(cmd, extra)
	return cmd
end
```

- [ ] **Step 2: Use it for the API call**

```lua
	local cmd = curl_cmd({ "-L", "-s", release_url })
```

- [ ] **Step 3: Use it for the archive download**

```lua
			local download_cmd = curl_cmd({ "-L", "-o", temp_file, download_url })
```

Task 22 adds the `"--"` separator before `download_url`; leave it out here.

- [ ] **Step 4: Guard the decoded API response**

Immediately after the `pcall(vim.json.decode, ...)` block:

```lua
			local ok, release_info = pcall(vim.json.decode, result.stdout)
			if not ok then
				callback(false, "Failed to parse release info")
				return
			end

			-- A rate-limited or errored API response decodes to valid JSON with
			-- no `assets`, which used to fall through to "No binary found for
			-- target: …" — telling the user their platform is unsupported when
			-- they were merely rate-limited (60 req/hr per IP, routine on NAT).
			if type(release_info) ~= "table" then
				callback(false, "Unexpected GitHub API response: " .. tostring(result.stdout):sub(1, 200))
				return
			end
			if release_info.message then
				callback(false, "GitHub API error: " .. tostring(release_info.message))
				return
			end
			if type(release_info.assets) ~= "table" then
				callback(
					false,
					"GitHub API response had no assets (rate limited or malformed): "
						.. tostring(result.stdout):sub(1, 200)
				)
				return
			end
```

Then simplify the asset loop's `release_info.assets or {}` to
`release_info.assets`, since it is now guaranteed to be a table.

- [ ] **Step 5: Verify the hardening by hand**

```bash
# The happy path still works.
nvim --headless -c 'lua require("time-tracking-nvim").download()' -c 'sleep 20' -c 'qall!'

# The flags are accepted by the installed curl.
curl --proto '=https' --proto-redir '=https' --tlsv1.2 --fail-with-body \
     --max-redirs 5 --connect-timeout 10 --max-time 60 --retry 2 \
     -sL https://api.github.com/repos/stevenwcarter/time-tracking-nvim/releases/latest \
     -o /dev/null -w '%{http_code}\n'
```

Expected: the download succeeds, and the raw curl prints `200`. If curl rejects
a flag (older builds lack `--fail-with-body`), report which one and substitute
`-f` rather than dropping the hardening.

- [ ] **Step 6: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 7: Strip and commit**

```bash
todo-parser bughunt.md --strip B12
git add -A
git commit -m "$(cat <<'EOF'
fix(security): harden both curl invocations and guard the API response [B12]

Both calls used a bare -L with no --proto, --proto-redir, --max-redirs,
--tlsv1.2, -f or --max-time. curl's default redirect protocol set includes
plain HTTP, so a 302 to http:// fetched the native library we are about
to dlopen in cleartext. Without -f, curl exits 0 on any HTTP error: a 403
rate-limit body decoded as valid JSON with no assets and was reported as
"No binary found for target: …" — "your platform is unsupported" when the
user was merely rate-limited; a 404 wrote an HTML error page into the
temp file and blamed tar. With no --max-time a black-holed connection left
the callback pending forever after setup() returned.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 22: B25 — `browser_download_url` used without validation

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` (URL check, the `--` separator, `M._internal`)
- Create: `integration_tests/lua/spec_download_url.lua`
- Modify: `integration_tests/lua/run_lua_tests.sh` (add the spec)

**Interfaces:**
- Consumes: Task 21's `curl_cmd`.
- Produces: `is_trusted_download_url(url) -> boolean`, added to `M._internal`.

- [ ] **Step 1: Write the failing spec**

Create `integration_tests/lua/spec_download_url.lua`:

```lua
local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal

H.describe("is_trusted_download_url", function()
  H.it("accepts a github.com release asset for this repo", function()
    H.eq(internal.is_trusted_download_url(
      "https://github.com/stevenwcarter/time-tracking-nvim/releases/download/v0.1.7/time-tracking-nvim-x86_64-unknown-linux-gnu.tar.gz"
    ), true)
  end)

  H.it("accepts an objects.githubusercontent.com URL", function()
    H.eq(internal.is_trusted_download_url(
      "https://objects.githubusercontent.com/github-production-release-asset/12345/abcdef"
    ), true)
  end)

  H.it("rejects a foreign host", function()
    H.eq(internal.is_trusted_download_url("https://evil.example/x.tar.gz"), false)
  end)

  H.it("rejects a different GitHub repo", function()
    H.eq(internal.is_trusted_download_url(
      "https://github.com/attacker/evil/releases/download/v1/x.tar.gz"
    ), false)
  end)

  H.it("rejects plain http", function()
    H.eq(internal.is_trusted_download_url(
      "http://github.com/stevenwcarter/time-tracking-nvim/releases/download/v0.1.7/x.tar.gz"
    ), false)
  end)

  H.it("rejects a value that curl would read as an option", function()
    -- The URL is the trailing argv element, so a leading dash is parsed as a
    -- flag: -K/home/user/.netrc makes curl read an attacker-chosen config.
    H.eq(internal.is_trusted_download_url("-K/home/user/.netrc"), false)
    H.eq(internal.is_trusted_download_url("--output/tmp/pwned"), false)
  end)

  H.it("rejects a host that merely contains a trusted name", function()
    H.eq(internal.is_trusted_download_url("https://github.com.evil.example/x.tar.gz"), false)
    H.eq(internal.is_trusted_download_url("https://notgithubusercontent.com/x"), false)
  end)

  H.it("rejects nil and non-strings", function()
    H.eq(internal.is_trusted_download_url(nil), false)
    H.eq(internal.is_trusted_download_url(42), false)
  end)
end)

return H
```

- [ ] **Step 2: Add the spec to the runner**

```lua
    for _, spec in ipairs({ 'spec_version', 'spec_platform', 'spec_download_url' }) do
```

- [ ] **Step 3: Run to verify it fails**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: FAIL — `is_trusted_download_url` is nil.

- [ ] **Step 4: Implement**

Above `download_binary`:

```lua
-- Constrain where a release asset may be fetched from.
--
-- asset.browser_download_url is taken verbatim out of the API response and
-- handed to curl, so without this the only containment is that asset.name
-- string-matched — a tampered response pointing at any host would be fetched
-- and dlopen'd. Anchored patterns, so a host merely *containing* a trusted
-- name (github.com.evil.example) does not pass.
local function is_trusted_download_url(url)
	if type(url) ~= "string" then
		return false
	end

	local host = url:match("^https://([%w%.%-]+)/")
	if not host then
		return false
	end

	if host == "github.com" then
		return url:match("^https://github%.com/stevenwcarter/time%-tracking%-nvim/") ~= nil
	end

	return host:match("%.githubusercontent%.com$") ~= nil
end
```

Reject the URL in the asset loop, right after `download_url` is resolved:

```lua
			if not download_url then
				callback(false, "No binary found for target: " .. target)
				return
			end

			if not is_trusted_download_url(download_url) then
				callback(false, "Refusing untrusted download URL: " .. tostring(download_url))
				return
			end
```

Add the `--` separator so a leading dash can never be read as a flag, even if
the allowlist is later loosened:

```lua
			local download_cmd = curl_cmd({ "-L", "-o", temp_file, "--", download_url })
```

Add it to `M._internal`:

```lua
M._internal = {
	is_version_newer = is_version_newer,
	get_platform_info = get_platform_info,
	normalize_os_name = normalize_os_name,
	is_trusted_download_url = is_trusted_download_url,
}
```

- [ ] **Step 5: Run to verify it passes**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: `19 passed, 0 failed`.

- [ ] **Step 6: Verify a real download still works**

```bash
rm -f lua/time_tracking_nvim.so.version
nvim --headless -c 'lua require("time-tracking-nvim").download()' -c 'sleep 20' -c 'qall!'
ls -l lua/time_tracking_nvim.so lua/time_tracking_nvim.so.version
```

Expected: both files present. A real asset URL must pass the allowlist — if it
does not, the pattern is wrong, so widen it deliberately (and add a spec case)
rather than removing the check.

- [ ] **Step 7: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 8: Strip and commit**

```bash
todo-parser bughunt.md --strip B25
git add -A
git commit -m "$(cat <<'EOF'
fix(security): constrain the release asset URL before handing it to curl [B25]

browser_download_url came verbatim out of the decoded JSON and became the
trailing argv element of the curl call. The only containment was that
asset.name string-matched, so a tampered response pointing at any host was
fetched and dlopen'd; and because it was trailing, a value beginning with
a dash was parsed by curl as an option (-K reads an attacker-chosen
config, --output redirects the write).

Anchored host allowlist plus a "--" separator, with specs covering the
foreign-host, wrong-repo, plain-http, leading-dash and lookalike-host
cases.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

### MILESTONE F — after Task 22

Run the full suite including the Lua specs. On red: bisect within Tasks 18-22, revert the offender, surface the diagnosis.

---

## Task 23: B1 — downloaded native library is dlopen'd unverified

**Files:**
- Modify: `.github/workflows/release.yml` (publish `SHA256SUMS`)
- Modify: `lua/time-tracking-nvim/init.lua` (fetch + verify before extracting; new `allow_unverified_download` option)
- Modify: `integration_tests/lua/spec_download_url.lua` (digest + parser specs)
- Modify: `README.md` (document the option next to `auto_download`)

**Interfaces:**
- Consumes: Task 21's `curl_cmd`, Task 22's `is_trusted_download_url`.
- Produces:
  - a `SHA256SUMS` asset on every release, in `sha256sum` format (`<hex>  <filename>`).
  - `download_binary(target, binary_path, callback, expected_version, opts)` — **one new trailing parameter**, `opts = { allow_unverified = boolean }`. All three call sites pass it.
  - `default_config.allow_unverified_download = false`.
  - `file_sha256(path) -> digest|nil, err` and `parse_sha256sums(text) -> table`, both added to `M._internal`.

**This is a deliberate, user-visible behavior change.** Releases at or before
v0.1.7 carry no `SHA256SUMS`, so with `expected_version` pinned to such a
release the download now refuses. Say so in the commit message and the README.

- [ ] **Step 1: Publish the checksums**

In `.github/workflows/release.yml`, in the `release` job, extend the
"Prepare release assets" step:

```yaml
      - name: Prepare release assets
        run: |
          mkdir -p release-assets
          find artifacts -name "*.tar.gz" -o -name "*.zip" | while read file; do
            cp "$file" release-assets/
          done
          cd release-assets
          # Names only, no paths: the Lua verifier matches on basename.
          sha256sum *.tar.gz *.zip > SHA256SUMS
          cat SHA256SUMS
          ls -la
```

`files: release-assets/*` already globs, so `SHA256SUMS` is uploaded with no
further change.

- [ ] **Step 2: Add the config option**

```lua
local default_config = {
	auto_download = true, -- Automatically download binaries if missing
	auto_update = true, -- Automatically update binary when plugin version changes
	-- Escape hatch for releases published before SHA256SUMS existed (<= v0.1.7)
	-- and for air-gapped mirrors. Leaving this false means a downloaded native
	-- library is never dlopen'd without matching a published digest.
	allow_unverified_download = false,
}
```

- [ ] **Step 3: Implement the digest helpers**

Add above `download_binary`:

```lua
-- Compute the SHA-256 of a file.
--
-- Prefers a subprocess over reading the file into Lua: readfile/writefile
-- round-trips are lossy for binary content, and the digest has to match
-- byte-for-byte what sha256sum computed in CI.
local function file_sha256(path)
	local out
	if vim.fn.executable("sha256sum") == 1 then
		out = vim.system({ "sha256sum", "--", path }, { text = true }):wait()
	elseif vim.fn.executable("shasum") == 1 then
		out = vim.system({ "shasum", "-a", "256", "--", path }, { text = true }):wait()
	elseif vim.fn.executable("certutil") == 1 then
		out = vim.system({ "certutil", "-hashfile", path, "SHA256" }, { text = true }):wait()
	else
		return nil, "no SHA-256 implementation available (need sha256sum, shasum or certutil)"
	end

	if not out or out.code ~= 0 then
		return nil, "checksum command failed: " .. tostring(out and out.stderr or "?")
	end

	local digest = tostring(out.stdout):match("%x%x%x%x%x%x%x%x%x+")
	if not digest then
		return nil, "could not parse checksum output"
	end
	return digest:lower()
end

-- Parse `sha256sum` output into { [basename] = digest }.
local function parse_sha256sums(text)
	local sums = {}
	for line in tostring(text):gmatch("[^\r\n]+") do
		local digest, name = line:match("^(%x+)%s+%*?(%S+)$")
		if digest and name then
			sums[vim.fs.basename(name)] = digest:lower()
		end
	end
	return sums
end
```

- [ ] **Step 4: Locate the SHA256SUMS asset**

In `download_binary`, the asset loop must now find both assets, so it no longer
`break`s:

```lua
			local download_url = nil
			local sums_url = nil
			for _, asset in ipairs(release_info.assets) do
				if asset.name == asset_name then
					download_url = asset.browser_download_url
				elseif asset.name == "SHA256SUMS" then
					sums_url = asset.browser_download_url
				end
			end
```

- [ ] **Step 5: Verify before extracting**

Restructure the archive-download callback so the existing extract chain is
reached only through a verification gate. Wrap the existing extract logic in a
local function and gate it:

```lua
					-- Verify BEFORE extracting: everything downstream — extract,
					-- copy into lua/, and the pcall(require, …) that dlopens it —
					-- treats these bytes as trusted native code.
					local allow_unverified = opts and opts.allow_unverified

					local function verify_then_extract(expected_digest)
						if expected_digest then
							local actual, digest_err = file_sha256(temp_file)
							if not actual then
								vim.fn.delete(temp_dir, "rf")
								callback(false, "Could not compute checksum: " .. tostring(digest_err))
								return
							end
							if actual ~= expected_digest then
								vim.fn.delete(temp_dir, "rf")
								callback(
									false,
									string.format(
										"Checksum mismatch for %s (expected %s, got %s) - refusing to install",
										asset_name,
										expected_digest,
										actual
									)
								)
								return
							end
						elseif not allow_unverified then
							vim.fn.delete(temp_dir, "rf")
							callback(
								false,
								"No SHA256SUMS published for this release, so the binary cannot be "
									.. "verified. Releases up to v0.1.7 predate checksums. To install "
									.. "anyway, use setup({ allow_unverified_download = true })."
							)
							return
						end

						-- ... the existing extract_cmd / vim.system(extract_cmd, ...) chain,
						-- unchanged, moved inside this function ...
					end

					if sums_url and is_trusted_download_url(sums_url) then
						local sums_file = vim.fs.joinpath(temp_dir, "SHA256SUMS")
						vim.system(curl_cmd({ "-L", "-o", sums_file, "--", sums_url }), {}, function(sums_result)
							vim.schedule(function()
								if sums_result.code ~= 0 or vim.fn.filereadable(sums_file) ~= 1 then
									vim.fn.delete(temp_dir, "rf")
									callback(false, "Could not download SHA256SUMS: " .. (sums_result.stderr or ""))
									return
								end
								local sums = parse_sha256sums(table.concat(vim.fn.readfile(sums_file), "\n"))
								local expected = sums[asset_name]
								if not expected then
									vim.fn.delete(temp_dir, "rf")
									callback(false, "SHA256SUMS has no entry for " .. asset_name)
									return
								end
								verify_then_extract(expected)
							end)
						end)
					else
						verify_then_extract(nil)
					end
```

Move the existing extract chain into `verify_then_extract` rather than
duplicating it, and keep every existing `vim.fn.delete(temp_dir, "rf")` cleanup
on every error path.

- [ ] **Step 6: Thread the option through the call sites**

Change the signature to
`local function download_binary(target, binary_path, callback, expected_version, opts)`
and update all three call sites:
- the auto-download site: `end, PLUGIN_VERSION, { allow_unverified = config.allow_unverified_download })`
- the auto-update site: same
- `M.download()`: `end, PLUGIN_VERSION, { allow_unverified = (M.config or {}).allow_unverified_download })`

- [ ] **Step 7: Test the digest and the parser**

First establish the expected digest independently:

```bash
printf 'hello\n' | sha256sum
```

Then append to `integration_tests/lua/spec_download_url.lua`:

```lua
H.describe("parse_sha256sums", function()
  H.it("parses sha256sum output keyed by basename", function()
    local sums = internal.parse_sha256sums(
      "abc123  time-tracking-nvim-x86_64-unknown-linux-gnu.tar.gz\n"
        .. "def456 *release-assets/time-tracking-nvim-x86_64-pc-windows-msvc.zip\n"
    )
    H.eq(sums["time-tracking-nvim-x86_64-unknown-linux-gnu.tar.gz"], "abc123")
    H.eq(sums["time-tracking-nvim-x86_64-pc-windows-msvc.zip"], "def456")
  end)

  H.it("ignores blank and malformed lines", function()
    local sums = internal.parse_sha256sums("\nnot a checksum line\n\n")
    H.eq(next(sums), nil)
  end)
end)

H.describe("file_sha256", function()
  H.it("matches the digest sha256sum computes", function()
    local path = vim.fn.tempname()
    vim.fn.writefile({ "hello" }, path)
    local digest = internal.file_sha256(path)
    vim.fn.delete(path)
    -- Independently verified with: printf 'hello\n' | sha256sum
    H.eq(digest, "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03")
  end)
end)
```

If the `printf` in the shell disagrees with the literal above, trust the shell
and update the literal — the point is that `file_sha256` agrees with the tool
CI uses.

Add `parse_sha256sums` and `file_sha256` to `M._internal`.

- [ ] **Step 8: Run the Lua suite**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: all green, including the three new assertions.

- [ ] **Step 9: Document the option**

In `README.md`, next to where `auto_download` / `auto_update` are described, add
`allow_unverified_download` with its default (`false`) and a one-line
explanation that downloads are checksum-verified against the release's
`SHA256SUMS`, and that releases up to v0.1.7 predate it. If the README does not
currently document the setup options at all, add a short "Configuration" block
covering all three.

- [ ] **Step 10: Verify end-to-end**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml')); print('yaml ok')"
rm -f lua/time_tracking_nvim.so.version
nvim --headless -c 'lua require("time-tracking-nvim").download()' -c 'sleep 25' -c 'messages' -c 'qall!'
```

Expected: because v0.1.7 has no `SHA256SUMS`, this now **refuses** with the
"No SHA256SUMS published" message. That is the intended fail-closed behavior.
Confirm the escape hatch works:

```bash
nvim --headless -c 'lua require("time-tracking-nvim").setup({ allow_unverified_download = true })' \
     -c 'sleep 25' -c 'messages' -c 'qall!'
```

Report exactly what both runs printed.

- [ ] **Step 11: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 12: Strip and commit**

```bash
todo-parser bughunt.md --strip B1
git add -A
git commit -m "$(cat <<'EOF'
fix(security): verify the downloaded library against a published digest [B1]

download_binary fetched an archive from a URL out of the GitHub API JSON,
extracted it, copied it over lua/time_tracking_nvim.{so,dylib,dll}, and
the caller immediately dlopen'd it — with nothing between the network and
dlopen validating the bytes. Anyone able to serve that response got
arbitrary native code execution inside every user's editor on the next
setup(), silently, because auto_download and auto_update both default true.

release.yml now publishes SHA256SUMS alongside the archives, and the
loader fetches it and verifies the digest BEFORE extracting.

BEHAVIOR CHANGE: verification is fail-closed. Releases up to v0.1.7
predate SHA256SUMS, so pinning one of those tags now refuses to
auto-download. The escape hatch is an explicit
setup({ allow_unverified_download = true }), documented in the README.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 24: B2 — messages are unrecoverable and force a hit-enter prompt

**Files:**
- Modify: `lua/time-tracking-nvim/init.lua` (all `vim.api.nvim_echo` call sites)

**Interfaces:**
- Consumes: nothing.
- Produces: a local `echo(chunks, opts)` helper. `opts = { transient = boolean }`.

- [ ] **Step 1: Count the call sites first**

Run: `grep -c 'nvim_echo' lua/time-tracking-nvim/init.lua`
Record the number — every one must end up going through the helper.

- [ ] **Step 2: Implement the helper**

Near the top of `lua/time-tracking-nvim/init.lua`, after `local uv = ...`:

```lua
-- All user-facing messages go through here.
--
-- Every call site previously passed `history = false`, so nothing was
-- retrievable with :messages — and these all fire during setup() at startup,
-- where they scroll off within milliseconds. Worse, a message taller than one
-- line triggers Neovim's hit-enter prompt, so the multi-line failure blocks
-- stopped every launch with a wall of text the user then could not recall.
--
-- opts.transient = true keeps a progress notice out of the history.
local function echo(chunks, opts)
	opts = opts or {}
	vim.api.nvim_echo(chunks, not opts.transient, {})
end
```

- [ ] **Step 3: Route every site through it**

Mechanically replace each `vim.api.nvim_echo({ ... }, false, {})` with
`echo({ ... })`, except the two progress notices — "Binary not found,
downloading for …" and "Binary update needed (…), downloading…" — which become
`echo({ ... }, { transient = true })`.

- [ ] **Step 4: Collapse the two startup walls**

The version-mismatch branch (`elseif needs_update and not config.auto_update`)
echoes six lines and the missing-binary branch (`elseif not binary_exists`)
eight. Both fire at startup and both trip the hit-enter prompt. Replace with one
line each:

```lua
	elseif needs_update and not config.auto_update then
		echo({
			{ "time-tracking-nvim: ", "WarningMsg" },
			{
				"binary version mismatch ("
					.. update_reason
					.. "); auto-update is disabled. Run :lua require('time-tracking-nvim').version_info() for detail.",
				"Normal",
			},
		})
	elseif not binary_exists then
		echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{
				"binary not found at "
					.. binary_path
					.. ". Run :checkhealth time-tracking-nvim for detail.",
				"Normal",
			},
		})
		return
	end
```

The `:checkhealth` provider lands in Task 25, which is next; the message is
advice, not a code path, so the ordering is fine.

- [ ] **Step 5: Verify no bare call sites remain**

Run: `grep -n 'nvim_echo' lua/time-tracking-nvim/init.lua`
Expected: exactly one hit — the definition inside `echo`.

- [ ] **Step 6: Verify messages are recoverable**

```bash
nvim --headless --cmd "set runtimepath^=$(pwd)" \
     -c 'lua require("time-tracking-nvim").setup({auto_download=false, auto_update=false})' \
     -c 'messages' -c 'qall!'
```

Expected: the plugin's messages appear in the `:messages` output. Then open an
interactive `nvim` with the same setup and confirm no hit-enter prompt at
startup.

- [ ] **Step 7: Run the Lua suite**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: all green — `echo` is a local, so `_internal` is unaffected.

- [ ] **Step 8: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 9: Strip and commit**

```bash
todo-parser bughunt.md --strip B2
git add -A
git commit -m "$(cat <<'EOF'
fix(observability): make loader messages recoverable from :messages [B2]

All 32 nvim_echo calls passed history = false, so none were retrievable
with :messages — and they all fire during setup() at startup, scrolling
off within milliseconds. A failed auto-download flashed and was gone,
:messages showed nothing, and :TimeTrackingToggle then answered E492 with
no artifact anywhere explaining why.

Routes every site through one helper that records to history (transient
progress notices excepted) and collapses the six- and eight-line startup
failure blocks to one line each — taller than one line triggers Neovim's
hit-enter prompt, stopping every launch with a wall of text.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 25: B10 — no `:checkhealth` entry point

**Files:**
- Create: `lua/time-tracking-nvim/health.lua`
- Modify: `lua/time-tracking-nvim/init.lua` (`M._internal.PLUGIN_VERSION`)
- Modify: `README.md` (Troubleshooting section)

**Interfaces:**
- Consumes: `M._internal` (Tasks 0/18/22/23).
- Produces: `require("time-tracking-nvim.health").check()`, reached as `:checkhealth time-tracking-nvim`; `PLUGIN_VERSION` added to `M._internal`.

- [ ] **Step 1: Export the version for the health check**

In `lua/time-tracking-nvim/init.lua`:

```lua
M._internal = {
	PLUGIN_VERSION = PLUGIN_VERSION,
	is_version_newer = is_version_newer,
	get_platform_info = get_platform_info,
	normalize_os_name = normalize_os_name,
	is_trusted_download_url = is_trusted_download_url,
	parse_sha256sums = parse_sha256sums,
	file_sha256 = file_sha256,
}
```

- [ ] **Step 2: Write the health provider**

Create `lua/time-tracking-nvim/health.lua`:

```lua
-- :checkhealth time-tracking-nvim
--
-- Neovim resolves `:checkhealth <name>` to `lua/<name>/health.lua`, so this
-- file's location is what makes the idiomatic command work.

local M = {}

local health = vim.health
local uv = vim.uv or vim.loop

function M.check()
	health.start("time-tracking-nvim")

	local tt = require("time-tracking-nvim")
	local internal = tt._internal or {}

	-- Platform
	local platform_info, platform_err = internal.get_platform_info and internal.get_platform_info()
	if not platform_info then
		health.error("Unsupported platform: " .. tostring(platform_err), {
			"Supported: Linux x86_64/aarch64, macOS x86_64/arm64, Windows x86_64",
		})
		return
	end
	health.ok(string.format("Platform: %s (.%s)", platform_info.target, platform_info.ext))

	-- Binary
	local plugin_root = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h:h")
	local binary_path = vim.fs.joinpath(plugin_root, "lua", "time_tracking_nvim." .. platform_info.ext)

	if vim.fn.filereadable(binary_path) ~= 1 then
		health.error("Native library not found at " .. binary_path, {
			"Run :lua require('time-tracking-nvim').download()",
			"Or build locally with ./build.sh",
		})
		return
	end

	local stat = uv.fs_stat(binary_path)
	if not stat then
		health.error("Cannot stat " .. binary_path)
		return
	end
	health.ok(string.format("Native library: %s (%d bytes)", binary_path, stat.size))

	-- Versions
	local version_file = binary_path .. ".version"
	local binary_version = "unknown"
	if vim.fn.filereadable(version_file) == 1 then
		local content = vim.fn.readfile(version_file)
		if #content > 0 then
			binary_version = vim.trim(content[1])
		end
	end

	local plugin_version = internal.PLUGIN_VERSION
	if plugin_version and binary_version == plugin_version then
		health.ok("Version: plugin and binary both " .. plugin_version)
	elseif plugin_version then
		health.warn(
			string.format("Version mismatch: plugin %s, binary %s", plugin_version, binary_version),
			{ "Run :lua require('time-tracking-nvim').download()" }
		)
	else
		health.info("Binary version: " .. binary_version)
	end

	-- cpath
	if package.cpath:find(vim.fs.joinpath(plugin_root, "lua"), 1, true) then
		health.ok("Binary directory is on package.cpath")
	else
		health.warn("Binary directory is not on package.cpath", {
			"setup() adds it; make sure require('time-tracking-nvim').setup() has run",
		})
	end

	-- Load
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		health.error("Failed to load the native module: " .. tostring(native), {
			"Check the library's permissions and architecture",
			"cpath: " .. package.cpath,
		})
	elseif type(native) == "table" and native.error then
		health.error("Native module loaded but failed to initialize: " .. tostring(native.error), {
			"Check your time-tracking-cli configuration",
		})
	else
		health.ok("Native module loads and initializes")
	end

	-- Commands
	if vim.fn.exists(":TimeTrackingToggle") == 2 then
		health.ok("Commands are registered")
	else
		health.error("Commands are not registered (:TimeTrackingToggle is missing)")
	end

	-- External tools used by auto-download
	for _, tool in ipairs({ "curl", "tar", "unzip" }) do
		if vim.fn.executable(tool) == 1 then
			health.ok(tool .. " is available")
		else
			health.warn(tool .. " is not available", { "Needed for auto-download/auto-update" })
		end
	end
end

return M
```

- [ ] **Step 3: Run it**

```bash
nvim --headless \
  --cmd "set runtimepath^=$(pwd)" \
  -c 'lua require("time-tracking-nvim").setup({auto_download=false, auto_update=false})' \
  -c 'checkhealth time-tracking-nvim' -c 'qall!'
```

Expected: a `time-tracking-nvim` health section with per-check OK/WARN/ERROR
lines and no Lua error. Fix anything that errors out — a health provider that
crashes is worse than none.

- [ ] **Step 4: Rewrite the README Troubleshooting section**

Replace the current "Plugin Not Loading" / "Preview Not Showing" advice ("Try
restarting Neovim", "Check that your time-tracking-cli configuration is set up
correctly") with a section that leads with the two real diagnostics:

    ## Troubleshooting

    Start here:

    ```vim
    :checkhealth time-tracking-nvim
    ```

    It reports the detected platform, whether the native library is present and
    loadable, whether the plugin and binary versions agree, whether
    `package.cpath` is set up, whether the commands registered, and whether
    `curl`/`tar`/`unzip` are available for auto-download.

    For startup problems that happen before the plugin loads, capture the debug
    log:

    ```bash
    TIME_TRACKING_DEBUG=1 nvim 2>/tmp/ttnvim.log
    ```

    ### Preview Not Showing

    Run `:TimeTrackingToggle` — it now reports why when the current buffer is
    not a tracking file, naming both the buffer and the configured data
    directory. The preview only opens for `.md` files inside your
    time-tracking-cli `data_directory`.

    ### Version Information

    ```vim
    :lua require('time-tracking-nvim').version_info()
    ```

Read the surrounding README first and match its heading levels and tone. Keep
any existing subsection that is still accurate.

- [ ] **Step 5: Run the Lua suite**

Run: `./integration_tests/lua/run_lua_tests.sh`
Expected: all green.

- [ ] **Step 6: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 7: Strip and commit**

```bash
todo-parser bughunt.md --strip B10
git add -A
git commit -m "$(cat <<'EOF'
feat(observability): add :checkhealth time-tracking-nvim [B10]

The repo already had nearly everything a health check needs in M.test()
and M.version_info(), but neither was mentioned in the README, there was
no health.lua so the idiomatic :checkhealth reported the plugin as
unknown, and TIME_TRACKING_DEBUG was documented only under
docs/superpowers/specs/ — which ships in no release artifact. A "plugin
does nothing on macOS" report left the maintainer with no command to ask
the user to run.

Adds the health provider and rewrites README Troubleshooting to lead with
:checkhealth and the debug-log capture.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 26: B16 — `build.sh` writes where `setup()` never looks

**Files:**
- Modify: `build.sh`
- Modify: `DEVELOPMENT.md` ("Testing Locally")
- Modify: `.gitignore`

**Interfaces:**
- Consumes: nothing.
- Produces: `./build.sh` leaves `lua/time_tracking_nvim.<ext>` and `lua/time_tracking_nvim.<ext>.version` in place.

- [ ] **Step 1: Confirm the bug**

Run: `grep -n 'binary_name\|joinpath' lua/time-tracking-nvim/init.lua | head`
Confirm `get_binary_path` returns `<plugin_root>/lua/time_tracking_nvim.<ext>`
and that `add_to_cpath` only ever adds `<plugin_root>/lua/?.<ext>` — so
`target/release` is never on `package.cpath`.

Then:
```bash
rm -f lua/time_tracking_nvim.so
./build.sh
ls -l target/release/time_tracking_nvim.so lua/time_tracking_nvim.so 2>&1
```
Expected: the `target/release` copy exists, the `lua/` one does not.

- [ ] **Step 2: Fix the copy target**

Replace the whole copy block in `build.sh` (the `if [ -f ... ]` / `case` / `else`
structure) with:

```bash
# Copy and rename the library to what Neovim expects.
#
# setup() loads from <plugin_root>/lua/ — add_to_cpath only ever adds
# `<plugin_root>/lua/?.<ext>`, so a build left in target/release is invisible
# to it, and auto_download (on by default) would silently fetch the *published*
# release over the top of a local build.
if [ -f "target/release/${LIB_NAME}" ]; then
    mkdir -p lua
    cp "target/release/${LIB_NAME}" "lua/time_tracking_nvim.${LIB_EXT}"
    echo "📦 Installed: lua/time_tracking_nvim.${LIB_EXT}"

    # Stamp the version so auto-update does not immediately replace this build.
    CARGO_VERSION="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
    printf '%s\n' "${CARGO_VERSION}" > "lua/time_tracking_nvim.${LIB_EXT}.version"
    echo "🏷  Stamped version: ${CARGO_VERSION}"
else
    echo "❌ Library not found: target/release/${LIB_NAME}"
    exit 1
fi
```

The per-OS `case` inside the old block is redundant once `LIB_EXT` is used
directly. The `mkdir -p target/release` line above it can go too.

- [ ] **Step 3: Fix the closing instructions**

```bash
echo "🎉 Build completed! You can now test the plugin in Neovim."
echo ""
echo "To test locally, make sure this directory is in your Neovim runtimepath:"
echo "  set runtimepath+=$(pwd)"
echo ""
echo "Then in Neovim (disable downloads so your local build is not replaced):"
echo "  :lua require('time-tracking-nvim').setup({ auto_download = false, auto_update = false })"
echo "  :TimeTrackingToggle"
```

- [ ] **Step 4: Ignore the build products**

Append to `.gitignore`:

```gitignore
lua/*.so
lua/*.dylib
lua/*.dll
lua/*.version
```

- [ ] **Step 5: Update DEVELOPMENT.md**

In "Testing Locally", change the `setup()` invocation to
`require('time-tracking-nvim').setup({ auto_download = false, auto_update = false })`
and add a sentence explaining why: with the defaults, a missing binary at the
expected path triggers a download of the published release, so you would be
testing upstream's binary rather than your own build. Read the surrounding
section and match its formatting.

- [ ] **Step 6: Verify**

```bash
rm -f lua/time_tracking_nvim.so lua/time_tracking_nvim.so.version
./build.sh
ls -l lua/time_tracking_nvim.so lua/time_tracking_nvim.so.version
cat lua/time_tracking_nvim.so.version
git status --porcelain
```

Expected: both files exist, the version file matches `Cargo.toml` (0.1.7), and
`git status` shows **no** untracked `lua/*.so` — proving the `.gitignore`
entries work.

Then confirm the local build actually loads:

```bash
nvim --headless --cmd "set runtimepath^=$(pwd)" \
  -c 'lua require("time-tracking-nvim").setup({ auto_download = false, auto_update = false })' \
  -c 'echo exists(":TimeTrackingToggle")' -c 'qall!'
```

Expected: prints `2` (the command is defined) — the local build was found and loaded.

- [ ] **Step 7: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 8: Strip and commit**

```bash
todo-parser bughunt.md --strip B16
git add -A
git commit -m "$(cat <<'EOF'
fix(api-surface): build.sh installs where setup() actually looks [B16]

build.sh left the library in target/release, but get_binary_path returns
<plugin_root>/lua/time_tracking_nvim.<ext> and add_to_cpath only ever
adds <plugin_root>/lua/?.<ext> — target/release is never on package.cpath.
A contributor following DEVELOPMENT.md hit filereadable() == false and,
because auto_download defaults to true, silently downloaded the
*published* release and tested upstream's binary while believing they
were testing their own edit.

Installs into lua/ and stamps a matching .version file so auto-update
does not immediately overwrite the local build; docs now disable
downloads for local testing, and the build products are gitignored.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Task 27: B4 — floating third-party actions in a `contents: write` job

**Files:**
- Modify: `.github/workflows/release.yml` (every `uses:`)
- Modify: `.github/workflows/ci.yml` (every `uses:`)
- Create: `.github/dependabot.yml`

**Interfaces:**
- Consumes: nothing.
- Produces: every `uses:` pinned to a 40-char commit SHA.

**These SHAs were resolved against the live GitHub API on 2026-09-03. Do not invent or alter them.**

| Action | Pin | Version |
|---|---|---|
| `actions/checkout` | `11d5960a326750d5838078e36cf38b85af677262` | v4.4.0 |
| `actions/upload-artifact` | `ea165f8d65b6e75b540449e92b4886f43607fa02` | v4.6.2 |
| `actions/download-artifact` | `d3f86a106a0bac45b974a628896c90dbdf5c8093` | v4.3.0 |
| `dtolnay/rust-toolchain` | `6bed0761d98439e5a578e2877258200ad565ba87` | `stable` branch @ 2026-09-03 |
| `Swatinem/rust-cache` | `6323deb102c322ba6fcbdcafc7e3dddab59af2b6` | v2.9.2 |
| `rhysd/action-setup-vim` | `febef33995d6649302e9d88dda81e071b68f16a7` | v1.6.1 |
| `softprops/action-gh-release` | `efb35369e0ad2afab669f228072c1b0d510eae64` | v3.0.3 |

`actions/*` are pinned at the SHA of the `v4` tag they already use — this is a
pin, not a version bump. Dependabot will propose the upgrades.

- [ ] **Step 1: Pin every `uses:` in both workflows**

Apply these replacements everywhere they appear (including the jobs Task 19
added):

```yaml
        uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4.4.0
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2
        uses: actions/download-artifact@d3f86a106a0bac45b974a628896c90dbdf5c8093 # v4.3.0
        uses: dtolnay/rust-toolchain@6bed0761d98439e5a578e2877258200ad565ba87 # stable @ 2026-09-03
        uses: Swatinem/rust-cache@6323deb102c322ba6fcbdcafc7e3dddab59af2b6 # v2.9.2
        uses: rhysd/action-setup-vim@febef33995d6649302e9d88dda81e071b68f16a7 # v1.6.1
        uses: softprops/action-gh-release@efb35369e0ad2afab669f228072c1b0d510eae64 # v3.0.3
```

Keep each step's existing `name:`, `with:` and `env:` blocks — change only the
`uses:` line.

`dtolnay/rust-toolchain@stable` is the weakest of the lot: `stable` is a
*branch*, so every run pulled its current HEAD into a job that builds the
shipped artifact.

**`softprops/action-gh-release` moves v1 → v3.** Check its release notes for
breaking changes to the inputs this workflow uses (`tag_name`, `name`, `draft`,
`prerelease`, `files`, `body`, and the `GITHUB_TOKEN` env var). If v3 renamed or
removed any of them, update the `with:` block accordingly and say so in the
commit message. If you cannot verify input compatibility, pin v2's SHA
(`3bb12739c298aeb8a4eeaf626c5b8d85266b0e65`) instead and note why.

- [ ] **Step 2: Add Dependabot**

Create `.github/dependabot.yml`:

```yaml
version: 2
updates:
  - package-ecosystem: "github-actions"
    directory: "/"
    schedule:
      interval: "weekly"
    commit-message:
      prefix: "chore(ci)"

  - package-ecosystem: "cargo"
    directory: "/"
    schedule:
      interval: "weekly"
    commit-message:
      prefix: "chore(deps)"
```

Pinned SHAs are only as good as the process that refreshes them; without this
the pins rot into permanently stale actions.

- [ ] **Step 3: Verify no floating references remain**

```bash
grep -rn 'uses:' .github/workflows/ | grep -v '@[0-9a-f]\{40\}'
```
Expected: no output.

- [ ] **Step 4: Verify the YAML parses**

```bash
python3 -c "
import yaml
for f in ['.github/workflows/ci.yml', '.github/workflows/release.yml', '.github/dependabot.yml']:
    yaml.safe_load(open(f)); print('ok', f)
"
```

- [ ] **Step 5: Full verification**

Run the Global Constraints verification command set. Expected: all green.

- [ ] **Step 6: Strip and commit**

```bash
todo-parser bughunt.md --strip B4
git add -A
git commit -m "$(cat <<'EOF'
fix(security): pin every third-party action to a commit SHA [B4]

The release job grants contents: write and hands GITHUB_TOKEN to
softprops/action-gh-release@v1 — a mutable tag on an abandoned major.
dtolnay/rust-toolchain@stable was weaker still: `stable` is a branch, so
every run pulled its current HEAD into the job that builds the shipped
artifact. Swatinem/rust-cache@v2 and rhysd/action-setup-vim@v1 floated
too, and rust-cache restores a cache into that same build job.

Compromising any of those upstreams — or force-moving a tag — would
execute code in the job producing the archives every user's loader
downloads and dlopens.

All SHAs resolved against the GitHub API on 2026-09-03. Adds Dependabot
for github-actions and cargo so the pins do not rot.

Claude-Session: https://claude.ai/code/session_01NxDoU22rRXAhMnM1WRo3rs
EOF
)"
```

---

## Final verification

- [ ] **All 27 findings stripped**

```bash
todo-parser bughunt.md --summary
```
Expected: 30 active items remain (the 30 that were never marked), 0 marked execute, 0 marked skip.

- [ ] **Full suite green**

```bash
cargo fmt -- --check && cargo clippy -- -D warnings && cargo test \
  && (cd integration_tests && cargo test) \
  && ./integration_tests/lua/run_lua_tests.sh
```

- [ ] **One commit per finding**

```bash
git log --oneline main..HEAD
```
Expected: the Task 0 harness commit, plus one `fix:`/`perf:`/`refactor:`/`feat:`
commit per finding tagged `[B<n>]`, plus separate `test:` commits for any
characterization tests written before a high-risk fix.

- [ ] **No summary commit.** The per-finding commits are the audit trail.

## Self-review notes

- **Spec coverage:** all 27 findings map to Tasks 1-27; the spec's test-strategy section maps to Task 0; the five recorded invariants are pinned by tests in Tasks 11 (1), 14 (2), 3 (3), 19 (4) and 23 (5).
- **Known softness, stated rather than hidden:** B6, B9, B23 and B8's failure path emit messages that `#[nvim_oxi::test]` cannot capture, so those tasks pair a characterization test (pinning that the behavior does not otherwise change) with an explicit manual verification step. Each says to report honestly if the manual check could not be performed.
- **B3 has a documented stop condition** rather than a fallback invention: if the `libuv` feature does not build, convert to `decision-needed` and skip.
- **Task 15 is the only task permitted to edit existing tests**, and only for the mechanical `any_tracking_visible(&config)` → `(&config, None)` call-site update forced by the signature change.
- **Interface consistency check:** `cleanup_preview_buffers()` (existing helper) is used by Tasks 7, 8, 9, 10, 11, 13, 17. `set_cached_preview_buf` / `cached_preview_buf` (Task 11) are consumed by Tasks 12 and 13. `find_preview` (Task 12) is consumed by Task 13. `curl_cmd` (Task 21) is consumed by Tasks 22 and 23. `is_trusted_download_url` (Task 22) is consumed by Task 23. `M._internal` grows monotonically across Tasks 0, 18, 22, 23, 25 — each task shows the full table as it should read after that task.
