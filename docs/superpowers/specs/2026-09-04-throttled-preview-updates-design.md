# Throttled preview updates — 200ms leading-edge cadence, all platforms

Date: 2026-09-04
Branch: `throttle/2026-09-04` (from `main` @ 77514de)
Source: user request via `/ship-it --ask`

## Problem

`update_preview_debounced` (`src/preview.rs:184`) is a **trailing-edge debounce** at 150ms. Every
`TextChanged`/`TextChangedI` cancels the in-flight timer and re-arms it, so *nothing renders until the
user stops typing*. A user typing continuously for ten seconds sees a frozen preview for ten seconds.

That is the wrong feel for this plugin. The preview is meant to be read *while* writing notes — the
user wants to watch the day summary accumulate as they type, not have it snap into place during
pauses.

Three secondary problems ride along with the current implementation:

1. **Windows is excluded.** The debounce is built on nvim-oxi's `libuv` bindings, whose `uv_*` externs
   carry no `raw-dylib` link attribute; the official Neovim Windows build exports none of those symbols
   either. So `src/preview.rs:244` carries a `#[cfg(windows)]` fork that renders on every keystroke,
   and `Cargo.toml` carries two nearly-identical `nvim-oxi` dependency blocks.
2. **Every re-arm leaks ~200 bytes.** Documented at length in the `PENDING_UPDATE` doc comment
   (`src/preview.rs:145-172`): nvim-oxi's `libuv::Handle` has no `Drop` impl, `TimerHandle` exposes no
   `&mut self` re-arm, so each keystroke allocates a `uv_timer_t` and a boxed callback that are never
   freed. There is no local fix.
3. **The libuv callback runs in a fast event context**, where the Neovim API is illegal
   (`E5560: nvim_buf_set_lines must not be called in a fast event context`), forcing a `schedule()`
   round-trip and a hand-rolled `catch_nvim_panic` wrapper inside it.

## Goal

Replace the debounce with a **leading-edge throttle at 200ms**:

- The first change in a burst renders **immediately and synchronously**.
- During sustained typing, renders land on a steady ~200ms cadence.
- The last change in a burst is always followed by a final render within 200ms, so the preview never
  ends stale.
- Identical behavior on Linux, macOS and Windows.

## Design

### Backend: Neovim's own `timer_start()`, not libuv

The trailing render needs a timer. Rather than nvim-oxi's `libuv::TimerHandle`, arm Neovim's own
vimscript `timer_start()`, which the plugin can reach through the `api::command` path it already uses
for every autocommand registration.

This is the decision that unlocks everything in "Problem" above:

| | libuv `TimerHandle` | Neovim `timer_start()` |
|---|---|---|
| Windows | unavailable — needs a `#[cfg]` fork | works |
| Leak | ~200B per arm, unfixable locally | none — no Rust-side handle |
| Callback context | fast event context; needs `schedule()` | main loop; API is legal directly |
| Cargo surface | `libuv` feature + duplicate target blocks | none |

The emitted vimscript is Vim's own documented timer idiom (`:help timer_start`):

```
call timer_start(83, {-> execute('TimeTrackingThrottleFire')})
```

built with `format!` (so `{{` / `}}` escape to the literal braces). A zero-argument lambda is a valid
timer callback — Vim's own help gives exactly this form — and Neovim passes the timer id, which the
lambda ignores.

If the implementer hits trouble with the vimscript lambda, the equivalent fallback is
`api::exec2("lua vim.defer_fn(function() vim.cmd('TimeTrackingThrottleFire') end, 83)", …)`.
`vim.defer_fn` is Neovim's own uv-timer wrapper and is likewise main-loop safe. Do not fall back to
libuv.

### State

Two thread-locals, replacing `PENDING_UPDATE`. No `#[cfg]` guards on either — they compile everywhere.

```rust
/// Minimum interval between autocommand-driven renders.
const THROTTLE: Duration = Duration::from_millis(200);

/// When the last throttle-path render happened.
static LAST_RENDER: Cell<Option<Instant>>;

/// Whether a render is already booked for the current window.
static THROTTLE_PENDING: Cell<bool>;
```

### Algorithm

`update_preview_throttled(config)`, the `TextChanged`/`TextChangedI` entry point:

1. **Guard:** `if !is_time_tracking_file(config)? { return Ok(()) }`. Unchanged from today, and still
   load-bearing — the autocommand fires for *every* `*.md` buffer, not just tracking notes.
2. **If `THROTTLE_PENDING` is set → return.** A render is already booked at this window's deadline.
   *This single line is the entire difference between a throttle and a debounce.* The debounce
   cancelled and re-armed here, pushing the deadline out for as long as the user kept typing; the
   throttle leaves the booked deadline alone. (With one exception, added later: a booking older than
   `2 × THROTTLE` is treated as lost rather than pending — see invariant 5.)
3. **If `LAST_RENDER` is `None`, or `≥ THROTTLE` has elapsed since it → render now**, synchronously,
   and stamp `LAST_RENDER = Instant::now()`. This is the leading edge.
4. **Otherwise → book the trailing render.** Set `THROTTLE_PENDING = true` and
   `timer_start(remaining_ms.max(1), …)` where `remaining = THROTTLE - elapsed`, so it lands exactly
   on the window boundary rather than 200ms after the current keystroke.

`throttle_fire(config)`, behind the internal `:TimeTrackingThrottleFire` command:

1. Clear `THROTTLE_PENDING`.
2. Stamp `LAST_RENDER = Instant::now()`.
3. `update_preview_fn(config)` — which keeps its own tracking-file and preview-open guards, re-checked
   against whatever buffer is current at fire time (same as the debounce did).
4. **Never return `Err`.** Log failures with `log_error!` and return `Ok(())`, mirroring the current
   debounce callback (`src/preview.rs:218-221`). An `Err` here surfaces as a bare
   "Error executing vim function callback" from the timer, detached from any user action.

No cancellation path is needed anywhere: the flag is what suppresses redundant arming, so a booked
timer always fires exactly once and always clears its own flag — within this plugin. That is a
property of *this* code, not of the process it shares; see invariant 5 for what recovers the flag when
something else destroys the timer.

### Naming

| Old | New | Why |
|---|---|---|
| `update_preview_debounced` | `update_preview_throttled` | the old name now describes the opposite algorithm |
| `:TimeTrackingUpdateDebounced` | `:TimeTrackingUpdateThrottled` | user-visible in `:TimeTracking<Tab>` completion |
| `DEBOUNCE` (150ms) | `THROTTLE` (200ms) | |
| `PENDING_UPDATE` | `THROTTLE_PENDING` + `LAST_RENDER` | |
| — | `:TimeTrackingThrottleFire` | new, `(internal)`-prefixed like `:TimeTrackingMaybeCloseIfInvisible` |

`:TimeTrackingUpdateThrottled` is a user-visible rename. Both it and the old name are autocommand
plumbing rather than documented API — neither appears in README's command list (`README.md:94-96`) nor
in the Lua wrappers — so the rename is safe, but it is called out here because it *is* visible in
command completion.

### Deletions

- The `#[cfg(windows)]` fork of `update_preview_debounced` (`src/preview.rs:229-246`) and its
  ~18-line explanatory doc comment.
- The `#[cfg(not(windows))]` attributes on the `Duration` import, the `TimerHandle` import, the
  interval const and the timer thread-local.
- `use nvim_oxi::libuv::TimerHandle;`.
- The `libuv` feature and both `[target.'cfg(…)'.dependencies]` blocks in `Cargo.toml`, collapsed back
  to one plain `[dependencies]` entry for `nvim-oxi` with just `neovim-0-12`. The 9-line comment block
  above them explaining the Windows link failure goes too.
- The `#[cfg(not(windows))]` gates on the two debounce integration tests.

### Explicitly unchanged

- `:TimeTrackingUpdate` → `update_preview_fn`, still fully synchronous. A user who types the command
  expects the result, not a wait. It does **not** stamp `LAST_RENDER`; an explicit update followed
  immediately by typing may render once more than strictly necessary, which is correct-but-eager and
  cheaper than coupling the two paths.
- `render_current_buffer`, `create_or_update_preview`, the `LAST_OUTPUT` dirty-check, and the
  `PREVIEW_BUF` handle cache.
- `M.update()` in `lua/time-tracking-nvim/init.lua:1057` still calls `:TimeTrackingUpdate`; only its
  comment's word "debounce" changes.

## Invariants this feature depends on

Recorded so a later change that breaks one can be traced back here.

1. **Neovim `timer_start` callbacks run on the main loop, not in a fast event context** — so
   `throttle_fire` may call `nvim_buf_set_lines` directly, with no `schedule()` hop. This is the
   load-bearing new invariant and the reason the libuv `schedule()` round-trip disappears. Pinned by
   the trailing-render test, which asserts the preview buffer actually changes after a timer fire.
2. **`write_preview_contents_with` remains the sole writer of preview buffer contents**, so the
   `LAST_OUTPUT` cache stays accurate. The throttle leans on this: a repeat render that produces
   identical output must be a cheap no-op, or the 200ms cadence would rewrite the buffer constantly.
3. **The `is_time_tracking_file` guard stays first in `update_preview_throttled`.** Without it, editing
   any `*.md` file arms timers. It no longer bounds a memory leak (there isn't one now), but it still
   bounds the work.
4. **Plugin state is thread-local on Neovim's single UI thread.** `LAST_RENDER`/`THROTTLE_PENDING` are
   `Cell`s with no synchronization, which is only sound under that assumption.
5. **No other code in the session calls `timer_stopall()`** — *was assumed, now recovered from.*
   `THROTTLE_PENDING` is cleared only by `throttle_fire`, so anything that destroys a booked timer
   without running it — most realistically a `timer_stopall()` from unrelated code sharing the Neovim
   process — used to strand the flag, and a stranded flag made `update_preview_throttled` return early
   forever: autocommand-driven updates dead for the rest of the session, `:TimeTrackingUpdate` still
   working, and nothing to point at as the cause. `update_preview_throttled` now treats a booking with
   no render inside `2 × THROTTLE` of `LAST_RENDER` as lost — a booked render is always due within one
   `THROTTLE` of it — clears the flag and re-arms. The cost of a false positive is one extra render;
   the cost of the old assumption failing was the feature. Pinned by
   `test_throttle_recovers_from_a_booking_destroyed_behind_its_back`, which destroys a booking with
   `timer_stopall()` and asserts a later change still renders.
6. **The tabpage and buffer current when a render is booked are still current when it fires** — *assumed,
   documented rather than fixed.* `throttle_fire` re-checks nothing itself; `update_preview_fn` applies
   its own `is_time_tracking_file` and `preview_is_open()` guards against whatever is current at fire
   time, and `preview_is_open()` is scoped to the **current tabpage** (see `preview_win_in_current_tab`).
   So switching tabpage — or switching to a non-tracking buffer — inside an open throttle window sends
   the booked fire straight into that guard, and it renders nothing: the previous buffer's preview stays
   stale until its next change. This is identical to the behaviour of the debounce this replaced, and it
   is self-healing — by the time that buffer is edited again more than `THROTTLE` has almost always
   elapsed, so the change takes the leading edge and renders at once. Recorded here rather than fixed:
   re-targeting a booked render at the buffer it was booked for would mean capturing and revalidating a
   buffer handle across the timer, for a window that closes on its own within 200ms.

## Test plan

The existing four `test_debounced_*` tests in `integration_tests/src/lib.rs:1141-1310` encode debounce
semantics and **must be rewritten, not merely renamed** — `test_debounced_update_returns_without_blocking`
asserts "the debounce must not render synchronously on each keystroke", which leading-edge now violates
by design.

| Test | Pins |
|---|---|
| `test_throttled_update_renders_first_change_immediately` | Leading edge. One call, no event-loop turn, sentinel already overwritten on return. |
| `test_throttled_update_coalesces_a_burst` | 20 rapid calls advance `changedtick` exactly once, and return in <100ms. |
| `test_throttled_update_renders_the_trailing_change` | Call; mutate the buffer; call again inside the window; turn the loop; preview shows the **final** content. Pins invariant 1 and the never-stale guarantee. |
| `test_throttle_renders_repeatedly_during_sustained_typing` | **The test that fails against the old debounce.** Drive changes continuously for ~500ms; assert ≥2 renders land. A debounce renders zero times under continuous input. |
| `test_throttled_update_renders_nothing_for_a_non_tracking_file` | Adapted from the existing test; guard still holds. |
| `test_autocommand_is_throttled_but_explicit_command_is_not` | Adapted: first `doautocmd TextChanged` renders (leading edge), a second inside the window does not, and `:TimeTrackingUpdate` always renders. |
| `test_explicit_update_renders_immediately` | Unchanged; only its "debounce" wording. |

All of these lose their `#[cfg(not(windows))]` gates — the behavior is now uniform.

**Test seam:** add `#[doc(hidden)] pub fn reset_throttle_for_test()` clearing both thread-locals, so
tests can establish a known window boundary without sleeping. This follows the existing
`write_preview_contents_with` precedent (`src/lib.rs:31-33`), which is documented as "Test seam, not
interface".

## Documentation

- `README.md:127-132` — the paragraph claiming Linux/macOS debounce with Windows divergence becomes
  false in every clause. Rewrite as: updates are throttled to at most one render per 200ms on all
  platforms, with the first change rendering immediately.
- `CLAUDE.md` — the "Data flow" line says "TextChanged events update the preview in real-time";
  sharpen to name the throttle.
- `lua/time-tracking-nvim/init.lua:1057` — comment says "skipping the TextChanged debounce".

## Out of scope

- Making the interval configurable through `setup()` (asked and declined — hardcoded const).
- `bughunt.md` B57 (`TimeTrackingAutoClose` wired to no autocommand) and B54 (`bwipeout` pattern),
  both adjacent in `src/lib.rs` but unrelated.
- The `Config::get_formatter` per-render allocation noted in `bughunt.md:232`. That entry says it is
  "largely subsumed if the debounce in B3 lands first" — with a throttle it is bounded at one
  allocation per 200ms, which is the same conclusion.

## Files touched

| File | Change |
|---|---|
| `src/preview.rs` | throttle state + `update_preview_throttled` + `throttle_fire`; delete cfg fork |
| `src/lib.rs` | rename exports, register `:TimeTrackingUpdateThrottled` and `:TimeTrackingThrottleFire`, repoint the autocommand |
| `Cargo.toml` | drop `libuv` and both target-specific dependency blocks |
| `integration_tests/src/lib.rs` | rewrite the four debounce tests, add the cadence test |
| `README.md`, `CLAUDE.md`, `lua/…/init.lua` | doc/comment updates |
