# Bundled feature/UX/perf/debt pass (whats-next 2026-09-05 execute)

## Goal

Ship the 10 items selected from `WHATS-NEXT.md`'s 2026-09-05 triage as one
branch, plus one prerequisite bug fix (`bughunt` B7) that W6 explicitly
depends on. This spec is the combined design for all 11 changes — decided
during `/ship-it --ask`'s Q&A:

- **B7 is fixed as a prerequisite** (user's explicit choice) rather than
  deferred, because it's small (effort S), already fully specified in
  `bughunt.md`, and W6 cannot be built without it.
- **W5 (weekly view) gets full multi-day aggregation**, not a cheap
  single-buffer stand-in — via **Approach B**: one persistent `tokio::Runtime`
  created lazily and used with `block_on` inline in the command handler
  (local-disk-only work, invoked on purpose by the user — the blocking is
  negligible and this avoids the thread/generation-counter machinery a
  background-thread design would need for no real benefit here).

| ID | Title | Lens |
|----|-------|------|
| B7 | `catch_nvim_panic` must never return `Err` (prerequisite for W6) | bughunt (Critical) |
| W1 | Cache per-buffer tracking-file classification | scale-perf |
| W2 | Preview dismissal persists until explicitly reopened | ux |
| W3 | Lightweight status query for statusline integrations | feature-gap |
| W4 | Optional GitHub token for API rate limits | binary-distribution |
| W5 | Weekly summary view (`:TimeTrackingWeeklyToggle`) | unblock-debt |
| W6 | Direct command failures get diagnostic messages | ux (rides on B7) |
| W7 | `:TimeTrackingDownload` / `:TimeTrackingVersion` commands | ux |
| W9 | Preview refreshes on external file changes | feature-gap |
| W10 | `:checkhealth` checks whether the data directory resolves | ux |
| W11 | `:TimeTrackingOpenToday` command | feature-gap |

## Cross-cutting: a shared Tokio runtime for W5 and W11

`time-tracking-cli` is built with `default-features = false` in our
`Cargo.toml`, but `tokio = { version = "1.0", features = ["full"] }` is a
**hard, non-optional** dependency of that crate regardless of features (it's
not gated under `[features]`), and so are the `data_svc`, `display`, and
`file_utils` modules W5/W11 need (`DataService`, `get_weekly_summary`,
`create_template_content`, `get_week_dates`, `parse_weekday` are all always
compiled). So no new Cargo feature flags are needed on the `time-tracking-cli`
dependency — but we do need `tokio` itself as an **explicit direct
dependency** in our own `Cargo.toml` (not just transitive) to name its types:

```toml
tokio = { version = "1", features = ["rt"] }
```

Add one lazily-initialized, process-wide `tokio::runtime::Runtime` built via
`tokio::runtime::Builder::new_current_thread().enable_all().build()` (a
current-thread runtime is enough — this plugin never needs multiple
concurrent async tasks, and `Builder::new_current_thread()` only needs
tokio's `rt` feature, not `rt-multi-thread`; the `fs`/`time`/etc. services
`DataService`/`create_template_content` use are already compiled in because
`time-tracking-cli` requests tokio's `full` feature set, and Cargo unifies
features for one crate across the whole dependency graph), e.g. a
`std::sync::OnceLock<tokio::runtime::Runtime>` in a new small module
(`src/async_rt.rs` or similar) with one helper:

```rust
pub fn block_on<F: std::future::Future>(fut: F) -> F::Output
```

Both W5 and W11 call through this one helper rather than building their own
runtime — it's the only place `tokio::runtime::Builder` is constructed.

**Pitfall already checked and avoided:** `DataService::get()` and
`Config::get()` are the *global-singleton* accessors, and `Config::get()`
parses **real process argv** as `time-tracking-cli`'s own CLI arguments —
wrong and potentially panicking inside Neovim's process. The plugin already
avoids this via `Config::try_get_no_args()` in `lib.rs`. W5 and W11 must do
the same: use `DataService::new_with_dir(cache_timeout, data_dir, parse_settings)`
(the same pattern the CLI's own TUI and tests use to stay hermetic — see
`data_svc.rs`'s doc comment on `new_with_dir`), built from the already-loaded
`&'static Config` this plugin holds, never `DataService::get()`.

## Per-item design

### 0. Prerequisite: bughunt B7 — `catch_nvim_panic` must never return `Err`

**File:** `src/lib.rs:79-90`.

Per `bughunt.md` B7: returning `Err` from a `Function::from_fn` callback hits
`push_error → lua_error`, which under `LUAJIT_UNWIND_EXTERNAL` (macOS/arm64)
throws a C++ exception through a `nounwind` frame and aborts Neovim — the
exact failure mode the plugin's entry point (`time_tracking_nvim`) was
already fixed to avoid, but `catch_nvim_panic` still does it for every one of
the six command closures it wraps.

**Fix:** change `catch_nvim_panic` so both its arms end in `Ok(())`:

- Panic arm: keep the existing `api::err_writeln` call, but return `Ok(())`
  instead of converting to `Err`.
- **New:** the inner `Result::Err(e)` arm (currently passed through via
  `.flatten()`) must also log via `api::err_writeln` with the
  `[time-tracking-nvim] ...` prefix (mirroring `log_and_swallow` in
  `preview.rs:713-718`) and return `Ok(())`.

**Message-spam guard:** the bughunt repro is a failure that recurs on *every
keystroke* (`TextChangedI` → `TimeTrackingUpdate` on a stale window handle).
Do **not** implement this as a permanent one-shot latch (that would silently
swallow every future distinct error for the rest of the session, hiding
unrelated later failures). Instead, dedupe **identical consecutive**
messages: a thread-local `RefCell<Option<String>>` holding the last message
logged, cleared whenever a *different* message (or `Ok`) is logged — same
shape as `LAST_OUTPUT`/`last_output_matches` in `preview.rs:31-40`, applied to
error text instead of preview content. A different failure, or the same
failure recurring after something else succeeded in between, is always
reported.

**This is also W6.** Every command in `register_commands` (`lib.rs:176-282`)
is wrapped in `catch_nvim_panic`, so fixing B7 at this one call site
automatically gives `:TimeTrackingToggle`/`:TimeTrackingUpdate` (and every
other direct command) a diagnostic message on failure — no separate code
change. `auto_open_preview`/`auto_close_preview` already pre-swallow via
`log_and_swallow` before `catch_nvim_panic` ever sees an `Err`, so this
doesn't double-log those paths. **W6's acceptance criterion becomes an
explicit test**, not a separate change: prove that a `toggle_preview_fn` /
`update_preview_fn` failure surfaces a message (previously it did not — it
aborted or was silently dropped depending on platform).

### W1. Cache per-buffer tracking-file classification

**File:** `src/utils.rs`.

Add a bounded thread-local cache:

```rust
thread_local! {
    static BUF_CLASSIFICATION: RefCell<HashMap<i32, bool>> = ...;
}
```

keyed on the buffer handle (`Buffer` exposes `.handle() -> i32`, same as
`Window::handle()` already used in `preview.rs`). `is_buf_time_tracking_file`
checks the cache first; on a miss it computes as today and stores the result.

**Invalidation:** register one new internal command (mirroring the
`TimeTrackingMaybeCloseIfInvisible <amatch>` pattern in `lib.rs`), e.g.
`TimeTrackingInvalidateBufCache <abuf>`, wired to:

```
autocmd BufFilePost,BufDelete,BufWipeout * TimeTrackingInvalidateBufCache <abuf>
```

The handler removes that one buffer number from the cache (a rename or
delete is rare compared to buffer switches, so no need to invalidate on
anything hotter than this).

**Invariant this depends on:** a buffer's *classification* (tracking file or
not) never changes without one of `BufFilePost`/`BufDelete`/`BufWipeout`
firing for it. This holds today (classification depends only on buffer name
+ extension, both of which only change via a rename), but is worth stating
explicitly since a future feature that reclassifies a buffer in place (e.g.
W16-style configurable detection rules, not in this bundle) would need to
invalidate here too.

**Test:** a new integration test (in `integration_tests/`) that opens a
buffer as a non-tracking name, asserts it is not treated as tracking, renames
it (`:saveas`/`BufFilePost`) into the data directory with a `.md` extension,
and asserts it now *is* treated as tracking — pinning that the cache doesn't
serve a stale classification across a rename. A second case: open a tracking
buffer, close/wipe it, and confirm no leftover cache entry affects a
different buffer later reusing the same handle number.

### W2. Preview dismissal persists until explicitly reopened

**Files:** `src/preview.rs`.

Add `thread_local! { static PREVIEW_DISMISSED: Cell<bool> = ...; }`.

- `close_preview()` (`preview.rs:658`): set `PREVIEW_DISMISSED.set(true)` on
  every path that actually closes or swaps out the preview (i.e. keep this
  in the single shared close function, not scattered across call sites).
- `toggle_preview_fn`'s render branch (`preview.rs:354`, the `else` that
  calls `render_current_buffer` because no preview is currently open): clear
  `PREVIEW_DISMISSED.set(false)` — this is the "ask for the preview again"
  the user does.
- `auto_open_preview_impl` (`preview.rs:727`): return `Ok(())` early when
  `PREVIEW_DISMISSED.get()` is true, before the existing tracking-file check
  or after it — either order is fine since both are cheap; put the dismissal
  check first since it's a plain flag read with no I/O.

**Design note — `close_preview()` is shared with `QuitPre`.** `:TimeTrackingClose`
is both the user-facing command and the target of the (separately broken,
**not fixed in this bundle**) `QuitPre * TimeTrackingClose` autocommand
(bughunt B19). Setting the dismissal flag unconditionally inside
`close_preview()` means a `QuitPre`-triggered close also counts as
"dismissed." This does not make B19 worse: B19's current behavior is already
"closes and nothing ever brings it back" (per bughunt's own repro), which is
observably identical to "dismissed until the user explicitly reopens." No
change in scope is needed here to accommodate B19 — this is a documented
consequence, not an oversight.

**Not in scope:** `update_preview_fn` never opens a new preview window (only
renders into one already open), so dismissal never needs to gate it.

**Test:** integration test — open a tracking buffer (preview auto-opens),
`:TimeTrackingClose`, then re-trigger the auto-open path (switch away and
back, or re-fire `VimEnter`/`BufWinEnter` per the existing test harness
conventions) and assert the preview stays closed; then `:TimeTrackingToggle`
and assert it reopens.

### W3. Lightweight status query for statusline integrations

**New dependency:** add `time-tracking-parser` as a **direct** Cargo
dependency (same git source `time-tracking-cli` already uses:
`https://github.com/stevenwcarter/time-tracking-parser`, no branch pin
needed — Cargo will unify to the same locked commit). It is not re-exported
by `time-tracking-cli`, and `day_summary` only returns pre-formatted display
text, not structured totals — so getting `{total_minutes, dead_time_minutes,
warnings}` requires calling `parse_time_tracking_data` directly:

```rust
time_tracking_parser::parse_time_tracking_data(&buffer_content, config.get_prefix(), config.get_suffix())
// -> TimeTrackingData { total_minutes, dead_time_minutes, warnings, projects, .. }
```

**Surface:** expose this as a Lua-callable, not just an Ex command echo,
since the point is statusline (lualine) integration, which needs a value
back, not a message. Add a new `Function` to the `Dictionary` returned by
`time_tracking_with_config` in `lib.rs` (currently `Dictionary::new()` at
line 171) — e.g. key `"status"` — that:

1. Checks `is_time_tracking_file(config)`; returns an empty/`{}` table (or a
   table with an explicit `is_tracking_file = false`) if not.
2. Otherwise reads the buffer, parses it, and returns a `Dictionary` with
   `total_minutes`, `dead_time_minutes`, and `warning_count` (the length of
   `warnings`, not the full text — keep the statusline payload small).

This becomes `require('time_tracking_nvim').status()` in Lua. Also expose it
through the plugin's own public Lua module for ergonomics:
`require('time-tracking-nvim').summary()` in `init.lua`, a thin wrapper that
calls the native module's `status()` (mirroring how `.toggle()`/`.update()`/
`.close()` wrap `vim.cmd(...)` today, except this wraps a native call instead
of a command since it needs a return value).

**Test:** unit test for the Rust-side parse-and-summarize helper (pure
function, easy to test without Neovim) plus one integration test confirming
`require('time-tracking-nvim').summary()` returns the right totals for a
buffer with known content, and an empty/marked result for a non-tracking
buffer.

### W4. Optional GitHub token for API rate limits

**File:** `lua/time-tracking-nvim/init.lua`.

- Add `github_token` to `default_config` (default `nil`) — read either from
  `setup({ github_token = "..." })` or fall back to `os.getenv("GITHUB_TOKEN")
  or os.getenv("GH_TOKEN")` when unset.
- In `curl_cmd` (`init.lua:264-270`) do **not** add the header
  unconditionally — only `fetch_release` (the GitHub **API** call,
  `init.lua:529-540`) should ever send it; `fetch_file` (asset/archive
  downloads, and the SHA256SUMS fetch) must not, since those go to
  `objects.githubusercontent.com`/similar and don't need or want a token
  attached. Thread a `headers` parameter through `curl_cmd`/`fetch_release`
  rather than changing the shared hardening table, or give `fetch_release`
  its own small wrapper that appends `-H "Authorization: Bearer <token>"`
  when `config.github_token` (or the env fallback) is set.
- Note for the implementer (confirmed during design, corrects the original
  finding's rationale): `needs_update` in `classify_binary_state`
  (`init.lua:775-791`) is a **purely local** comparison (`read_binary_version()`
  vs. the hardcoded `PLUGIN_VERSION` constant) — the API is not hit on every
  restart, only when the binary is missing or that comparison disagrees. The
  real exposure is a fleet of machines behind one shared IP each hitting the
  API once on a genuine version bump (or first install), not "every Neovim
  launch."

**Test:** a new `integration_tests/lua/spec_github_token.lua`, following the
established convention in this directory (`harness.lua`'s zero-dependency
`H.describe`/`H.it`/`H.eq`, run via `run_lua_tests.sh`): stub `vim.system` as
`spec_download.lua` does, reach `fetch_release`/`curl_cmd` the same way that
file reaches `download_binary` when there's no test seam (`debug.getupvalue`
off a public function that closes over it — `M.download` or `M.setup`),
and assert the recorded argv includes `-H "Authorization: Bearer <token>"`
when a token is configured/present in the environment and omits it when
absent, plus a separate case proving the asset-download argv (`fetch_file`)
never includes it either way.

### W5. Weekly summary view (`:TimeTrackingWeeklyToggle`)

**Files:** `src/preview.rs`, `src/lib.rs`, new `src/async_rt.rs` (see
cross-cutting section above).

**Pitfall already checked and avoided:** `show_weekly_summary_with` (the
function the original finding pointed at) calls the formatter's `display_*`
methods (`display_weekly_header`, `display_weekly_totals`, ...), which
`println!` directly to stdout — useless for building preview-buffer text. Do
**not** call `show_weekly_summary_with`. Instead, replicate its structure
using the **String-returning** trait methods and the raw aggregate data:

1. Resolve the week's dates: `parse_weekday(config.get_week_start_day())` →
   `time::Weekday`, then `get_week_dates(&today, week_start_day)` → `[Date; 7]`
   (today via `time::OffsetDateTime::now_local()`, falling back to
   `now_utc().date()` if local-offset lookup fails — matches the plugin's own
   process, no config flag for this exists upstream).
2. Build a `DataService` via `DataService::new_with_dir(DataService::DEFAULT_CACHE_TIMEOUT_SECONDS, data_dir, parse_settings)` where `parse_settings` is a
   `ParseSettings { prefix: config.get_prefix().map(String::from), suffix: config.get_suffix().map(String::from), template_file: config.get_template_file().map(String::from) }`
   built from the already-loaded `Config` — never `DataService::get()` (see
   cross-cutting pitfall above).
3. `block_on(data_service.get_weekly_summary(&week_dates))` → `WeeklySummary { total_minutes, dead_time_minutes, warnings, projects, days }`.
4. Concatenate a `String` using the formatter's plain-string methods in the
   same order `show_weekly_summary_with` uses: `weekly_header`,
   `weekly_totals`, `weekly_warnings` (if non-empty), `weekly_projects` (if
   non-empty), then per day in `summary.days`: `day_header`, and either
   `day_summary` (if that day has data) or a short "no data"/"no file" line
   (there's no String-returning equivalent of `display_no_data_found`/
   `display_no_file_found` — write the plugin's own one-line fallback text
   for those two cases rather than adding upstream API surface for it).
5. Write the result into the preview via the existing
   `create_or_update_preview_with` path, exactly like `render_current_buffer`
   does for the day view.

**Command:** `:TimeTrackingWeeklyToggle` — same toggle semantics as
`:TimeTrackingToggle` (closes if a preview showing the *weekly* view is
open; otherwise renders the weekly view), registered the same way as the
other six in `register_commands`.

**Scope boundary (explicitly deferred, per the earlier scope discussion):**
day view and week view share the single `PREVIEW_BUF` slot (W14, the
multi-pane preview refactor, is not in this bundle) — switching to the weekly
view replaces whatever the preview was showing, same as switching buffers
does today. Track *which* view is currently rendered in the preview (a
thread-local `enum PreviewView { Day, Week }`, alongside `LAST_OUTPUT`) so
`update_preview_fn`/the throttled TextChanged path re-render the *day* view
only when the day view is what's showing — don't let a keystroke in a
tracking buffer silently replace an open weekly view.

More precisely: the **throttled autocmd path** (`update_preview_throttled` /
`throttle_fire`, driven by every keystroke) must skip re-rendering entirely
while the weekly view is current — re-aggregating the whole week via
`block_on` on typing cadence is exactly the pathological case "block_on is
fine because it's user-invoked and infrequent" was justified against. The
**direct, explicitly-typed** `:TimeTrackingUpdate` command may re-render
whichever view is current (day or week) — an explicit, infrequent user
action re-aggregating the week is fine, and doing so keeps `:TimeTrackingUpdate`'s
existing meaning ("refresh what's showing now") consistent across both views.

**Test:** unit test for the week-string-assembly helper (pure, given a
`WeeklySummary` and a formatter) covering: a week with data every day, a week
with a missing day file, and empty warnings/projects (should omit those
sections, matching `show_weekly_summary_with`'s own `if !summary.warnings.is_empty()`
guards). Integration test: seed 2-3 day files in a temp data directory,
`:TimeTrackingWeeklyToggle`, assert the preview contains the expected
aggregate totals.

### W7. `:TimeTrackingDownload` / `:TimeTrackingVersion` commands

**File:** `lua/time-tracking-nvim/init.lua`.

Every existing `:TimeTracking*` command is registered from the **Rust** side
(`api::create_user_command` in `lib.rs`) — there is no Lua-side
`vim.api.nvim_create_user_command` call anywhere in this plugin today. That
matters here: `M.download()`/`M.version_info()` are pure-Lua operations that
must work **even when the native module fails to load** (that's exactly the
troubleshooting scenario they exist for) — so they cannot be registered from
the Rust side (which only runs on a successful native load) and must be
registered directly in `init.lua`, early in `M.setup()`, **before** the
binary-exists/load-native ladder, so they exist regardless of how that ladder
turns out:

```lua
vim.api.nvim_create_user_command("TimeTrackingDownload", function() M.download() end,
  { desc = "Download or re-download the native binary" })
vim.api.nvim_create_user_command("TimeTrackingVersion", function() M.version_info() end,
  { desc = "Show plugin/binary version info" })
```

Update every troubleshooting message that currently says
`:lua require('time-tracking-nvim').download()` (in `init.lua`'s own
notify/echo strings and in `health.lua`) to say `:TimeTrackingDownload`
instead — that's the actual point of this item (see W7's `why`).

**Test:** a new `integration_tests/lua/spec_commands.lua` (or added to
`spec_setup.lua`, whichever fits its existing scope better), using the same
`harness.lua` convention as the other `spec_*.lua` files: stub `load_native`
to force a failure the way `spec_setup.lua` already does for its own cases,
call `M.setup()`, and assert `vim.fn.exists(":TimeTrackingDownload") == 2`
and `vim.fn.exists(":TimeTrackingVersion") == 2` regardless of that forced
failure — then a second case invoking each command and asserting (via a
stubbed `M.download`/`M.version_info`, or by observing their side effects the
way `spec_download.lua` does) that they actually call through.

### W9. Preview refreshes on external file changes

**File:** `src/lib.rs`, `register_autocommands` (`lib.rs:287-303`).

Add:

```
autocmd BufReadPost,FileChangedShellPost *.md TimeTrackingUpdateThrottled
autocmd FocusGained,BufEnter *.md checktime
```

The `checktime` line is what actually detects the on-disk change and fires
`FileChangedShellPost` if the buffer needs reloading; without it Neovim never
notices the file changed until some other trigger runs `:checktime`
implicitly. `TimeTrackingUpdateThrottled` (not the unthrottled
`TimeTrackingUpdate`) is the right target — it already no-ops for non-tracking
buffers and re-renders through the same throttle path as everything else, so
this needs no new Rust logic, only the two autocmd lines.

**Test:** integration test — write new content directly to the day file on
disk (not through the buffer), trigger `:checktime` (or wait for
`FocusGained`), and assert the preview picks up the new content without the
user having typed anything in the buffer.

### W10. `:checkhealth` checks whether the tracking data directory resolves

**Files:** `src/lib.rs` (new exposed function), `lua/time-tracking-nvim/health.lua`.

Same shape as W3's exposure: add a second key to the `Dictionary` returned
from `time_tracking_with_config`, e.g. `"data_directory_status"`, a
`Function` with no arguments that returns a `Dictionary` with:

- `configured`: the raw configured string (`config.get_data_directory()`, or
  a marker for "unset").
- `resolved`: boolean — whether it canonicalizes.
- `canonical_path`: the resolved path as a string, when `resolved` is true.
- `reason`: the `io::Error` text, when `resolved` is false.

This should reuse the same resolution logic `resolved_data_dir` in
`utils.rs` already implements (make it `pub(crate)` if it isn't already
reachable from `lib.rs`, or add a thin `pub fn data_directory_status(config: &Config) -> Dictionary`
wrapper in `utils.rs` that calls it) — do not reimplement directory
resolution a second time; that's exactly the kind of drift the memoization
comment on `resolved_data_dir` warns about.

**`health.lua`:** add `check_data_directory(internal)`, called after
`check_native_module` (needs the native module loaded to call this) and
before `check_commands`:

```lua
local tt = require("time-tracking-nvim")
local native_ok, native = pcall(require, "time_tracking_nvim")
if native_ok and native.data_directory_status then
  local status = native.data_directory_status()
  if status.resolved then
    health.ok("Data directory resolves: " .. status.canonical_path)
  else
    health.error("Data directory does not resolve: " .. tostring(status.reason), {
      "Configured value: " .. tostring(status.configured),
      "The preview will not open for any file until this is fixed",
    })
  end
end
```

(Adjust to whatever calling convention the rest of `health.lua` uses for
`internal`/`native` — it currently only reads test-seam functions off
`tt._internal`, never the native module's own returned dictionary, so this is
a new pattern in this file; keep it consistent with how `check_native_module`
already calls `internal.load_native()`.)

**Test:** integration test — point `Config` at a directory that does not
exist, call the new native function, assert `resolved = false` and a
sensible `reason`; then point it at a real temp directory and assert
`resolved = true` with the expected canonical path.

### W11. `:TimeTrackingOpenToday` command

**Files:** `src/lib.rs` (new command), reuses the shared `block_on` helper
from the cross-cutting section.

```
:TimeTrackingOpenToday
```

- Resolve today's date the same way as W5 (`time::OffsetDateTime::now_local()`,
  falling back to UTC).
- Compute the day-file path directly:
  `data_dir.join(format!("{}.md", today.format(&time_tracking_cli::DATE_FORMAT)?))`
  — `DATE_FORMAT` is `pub` at the crate root (`time-tracking-cli/src/lib.rs:17-18`,
  `"[year]-[month]-[day]"`), so this doesn't need a `DataService` at all for
  the path itself.
- If the file doesn't exist yet, seed it via
  `block_on(time_tracking_cli::create_template_content(&today, config.get_template_file()))`
  and write the result with `std::fs::write` (creating the data directory
  first if needed — `std::fs::create_dir_all` on the parent, mirroring what
  `DataService::ensure_data_dir` does, but done directly since we're not
  otherwise building a `DataService` here).
- `api::command(&format!("edit {}", shellescape_or_fnameescape(path)))` (use
  Neovim's own `fnameescape()` via `api::call_function` or an equivalent
  nvim-oxi helper — do not hand-roll shell escaping for a `:edit` argument).

**Not in scope:** `:TimeTrackingOpenDate <date>` (the optional extension the
finding mentioned) — ship `OpenToday` only; a dated variant is easy to add
later and isn't needed to satisfy this item's `why`.

**Test:** integration test — point `Config` at an empty temp data directory
with a template file configured, run `:TimeTrackingOpenToday`, assert
today's file now exists with the template's `{date}` placeholder replaced
and the correct buffer is open; run it again and assert the existing file's
content is untouched (no second seed).

## New Cargo dependencies

```toml
tokio = { version = "1", features = ["rt"] }
time-tracking-parser = { git = "https://github.com/stevenwcarter/time-tracking-parser" }
time = { version = "0.3", features = ["formatting", "local-offset", "macros"] }
```

(`time` is already a transitive dependency via `time-tracking-cli`, but W5/W11
name `time::OffsetDateTime`/`time::Weekday` directly, so it needs to be
direct too, matching feature flags already enabled upstream.)

## Documentation updates

- **README.md**: add `:TimeTrackingWeeklyToggle`, `:TimeTrackingDownload`,
  `:TimeTrackingVersion`, `:TimeTrackingOpenToday` to the Commands list
  (`README.md:94-96`); add `github_token` to the Setup options block and its
  bullet description (`README.md:57-73`); update the "Version Information"
  troubleshooting section to show `:TimeTrackingVersion` instead of the
  `:lua require(...)` form (`README.md:159-163`); mention `require('time-tracking-nvim').summary()`
  as a statusline integration point (new subsection under Usage).
- **CLAUDE.md**: update the command list in "Architecture" (currently names
  exactly 8 commands and calls out which are internal) to include the four
  new ones, and add one line each for the new `src/async_rt.rs` module and
  the two new native-module-exposed functions (`status`, `data_directory_status`)
  under "Key Implementation Details" if that pattern (functions on the
  returned Dictionary beyond commands) is new enough to warrant a callout —
  it is: today that Dictionary is always empty.

## Invariants this feature depends on

- **W1** depends on buffer classification never changing without a
  `BufFilePost`/`BufDelete`/`BufWipeout` firing (stated above; test pins the
  rename case).
- **W2** depends on `close_preview()` remaining the single function that
  actually closes/swaps the preview window — if a future change adds a
  second path that closes it without going through `close_preview()`, that
  path would bypass the dismissal flag. Grep for direct `win.close()` calls
  on a preview window outside `close_preview()` before extending this area
  later.
- **W5** depends on the day-file naming convention (`YYYY-MM-DD.md` under
  `data_directory`) staying in sync between this plugin and
  `time-tracking-cli` — already an existing coupling (`is_buf_time_tracking_file`
  hardcodes the same convention independently; `WHATS-NEXT.md` W16
  [unblock-debt] tracks unifying this, not fixed in this bundle), not a new
  coupling introduced here.

## Out of scope (explicitly deferred, not part of this branch)

- bughunt B19 (QuitPre closing the preview unconditionally) — not selected
  for this bundle; W2's design above documents why it doesn't need to be
  fixed for W2 to be correct.
- bughunt B42 (double buffer/name fetch in `any_tracking_visible`) — adjacent
  to W1 but distinct; not selected.
- W14 (multi-pane preview refactor) — W5's day/week views share one preview
  slot; simultaneous day+week display needs W14 first.
- `:TimeTrackingOpenDate <date>` — only `OpenToday` ships.
