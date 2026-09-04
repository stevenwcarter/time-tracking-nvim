# Tidy execution — preview/lib structure, Lua setup decomposition, folded bug fixes

Date: 2026-09-04
Branch: `tidy/2026-09-04` (from `main` @ d5054df)
Source: `TIDY.md` (17 items marked execute) + `bughunt.md` (5 items folded in by user decision)

## Scope

22 items total. The user elected to **fold five open `bughunt.md` findings into this pass** rather than
defer the two tidy items that collide with them (T12, T14). That choice is what sets the sequencing
below: the bug fixes land *inside* the monolithic functions first, and the structural extractions
happen afterwards, on already-correct code.

### From TIDY.md (17)

| ID | File | Risk | Summary |
|---|---|---|---|
| T1 | lua/init.lua | high | Flatten `download_binary`'s 7-level callback pyramid |
| T2 | lua/init.lua | high | `M.setup` is a 243-line multi-phase entry point |
| T3 | lua/init.lua | high | setup()'s download/update branches are the same 60-line sequence twice |
| T4 | lua/init.lua | medium | pcall-require + `native.error` check written 5 times |
| T5 | src/preview.rs, utils.rs, lib.rs | low | Extract `[Time Tracking Preview]` literal |
| T8 | lua/health.lua | low | health.lua reimplements two init.lua helpers |
| T9 | lua/init.lua | low | `is_version_newer` runs four phases in 46 lines |
| T10 | lua/init.lua | low | Hoist repo/releases/API URL literals |
| T11 | lua/init.lua | medium | Delete `M.test` (no caller, superseded by `M.check`) |
| T12 | src/lib.rs | medium | Collapse six command registrations, split 110-line body |
| T13 | src/preview.rs | low | Render pipeline + open-preview probe each written 3x |
| T14 | src/preview.rs | low | `create_or_update_preview` does four jobs in 125 lines |
| T15 | src/preview.rs | low | Window-geometry magic numbers inline |
| T16 | src/utils.rs | low | `resolved_data_dir`: lock held across syscall + FFI; key alloc on hit |
| T17 | src/utils.rs | low | manual_let_else 22 lines above an idiomatic let-else |
| T18 | .github/workflows/ci.yml | low | clippy without `--all-targets` |
| T19 | .github/workflows/ci.yml | low | `cargo-audit` rebuilt from source, no cache |

### Folded in from bughunt.md (5)

| ID | File | Summary |
|---|---|---|
| B37 | src/preview.rs | Preview buffer left permanently modifiable if the line write fails |
| B39 | src/preview.rs | Focus returned with `wincmd p` instead of the saved window handle |
| B41 | src/lib.rs | All commands registered with empty options (no `.desc()` / `.nargs()`) |
| B44 | src/preview.rs | Window-layout errors discarded with `let _` |
| B45 | src/preview.rs | Preview visibility checked across all tabpages, so tab 2 never gets one |

## Sequencing

Ordering is load-bearing. Three chains, executable in parallel across files but strictly ordered within
each chain.

### Chain A — src/preview.rs (bugs before structure)

1. **B45** — add `fn preview_win_in_current_tab() -> Result<Option<Window>>` using
   `api::get_current_tabpage().list_wins()?`. Repoint the visibility checks in `toggle_preview_fn`,
   `update_preview_fn`, `auto_open_preview_impl` and the `is_open` loop in `create_or_update_preview`.
   `close_preview` and the VimLeavePre cleanup stay global.
2. **T13** — extract `fn preview_is_open() -> Result<bool>` and `fn render_current_buffer(config)`.
   `preview_is_open` wraps the *B45-corrected* probe, so these two are one coherent change:
   do B45 first, then T13 consumes it. Three call sites shrink to a guard plus one call.
3. **B37** — capture the `set_lines` result, restore `modifiable=false` unconditionally, then `?` the
   result. A scope-guard struct is acceptable and covers future early returns.
4. **B44** — convert the three `let _ =` layout drops to `if let Err(e) = ... { debug_log!(...) }`;
   promote the `wincmd p` case to `log_error!` since it moves the user's cursor.
5. **B39** — replace `wincmd p` with a saved handle: `let origin = api::get_current_win();` before the
   split, `api::set_current_win(&origin)` after. Supersedes B44's `wincmd p` bullet — after B39 there is
   no `wincmd p` left to log, so B44's third drop becomes the `set_current_win` result instead.
6. **T15** — introduce `MIN_SPLIT_COLUMNS = 40`, `PREVIEW_SCREEN_FRACTION = 3`, `MIN_PREVIEW_COLUMNS = 20`
   beside the existing `DEBOUNCE` const.
7. **T14** — only now extract `create_preview_buffer`, `write_preview_contents`, `open_preview_split`
   and `style_preview_window`. B37 lands inside `write_preview_contents`; B39/B44 inside
   `open_preview_split`; T15's consts inside `style_preview_window`. Also make the window-list emptiness
   idiom consistent (`api::list_wins().next().is_none()` at both preview.rs:277 and :409).

### Chain B — src/lib.rs

8. **T12 + B41 together.** These were flagged as conflicting; folding them resolves the conflict rather
   than sequencing around it. Drive the simple commands from a data table of
   `(name, desc, func)` triples, and give each `.desc(desc).nargs(CommandNArgs::Zero)` — B41's requirement
   becomes T12's table column instead of something T12 would have to unwind. Keep
   `TimeTrackingMaybeCloseIfInvisible` spelled out separately (it needs `CommandNArgs::ZeroOrOne`).
   Then split into `register_commands` / `register_autocommands`.
   Command names stay byte-identical; `test_time_tracking_with_config_creates_commands` pins them.

### Chain C — Lua

9. **T11** — delete `M.test`. Do this first: it removes one of the five `load_native` sites and ~96 lines
   that T2/T4 would otherwise have to carry.
10. **T4** — add `local function load_native()` returning `status, native_or_err`
    (`"ok"` | `"load_failed"` | `"init_failed"`), export on the `M._internal` seam.
11. **T8** — export `get_binary_path`, `get_version_file_path`, `read_binary_version` and `plugin_root`
    on `M._internal`; health.lua consumes them and drops its own `debug.getinfo(1, "S")`.
12. **T3** — extract `have_download_tools(fatal)` and `download_then_load(target, binary_path, config, labels)`.
13. **T2** — extract `classify_binary_state`, `check_download_tools`, `on_download_finished`;
    `setup()` becomes merge, resolve, classify, dispatch.
14. **T1** — flatten `download_binary` into named CPS steps (`fetch_release`, `select_assets`,
    `fetch_sums`, `extract_and_install`, `record_version`, `verify_archive`) plus a `fail(temp_dir, msg)`
    helper for the 8x repeated cleanup triple.
15. **T9**, **T10** — independent polish, any point in the chain.

### Chain D — independent

16. **T16**, **T17** (src/utils.rs), **T18**, **T19** (.github/workflows/ci.yml). No ordering constraints.

## Characterization tests required first

T1, T2 and T3 carry `risk: high — needs characterization tests first`. Before touching any of them,
write characterization tests for `download_binary` and `M.setup`, confirm they pass on unchanged code,
and commit as `test: characterize <unit> before tidy [T<n>]`.

The existing Lua spec harness is `integration_tests/lua/` (`harness.lua` plus `spec_*.lua`, run by
`run_lua_tests.sh`). New specs go there. Note `spec_install.lua` was added by d5054df and is a good
template for stubbing `vim.system` / `vim.uv`.

## Deliberate deviations from the findings as written

- **T5 is scoped to three of its four sites.** The finding proposes substituting `PREVIEW_BUF_NAME` into
  `lib.rs:227`'s `autocmd VimLeavePre * silent! bwipeout [Time Tracking Preview]`. That argument is a
  broken unescaped pattern that never matches (bughunt **B54**, not in this batch). Interpolating a
  named constant there would make the line *read* as correct while staying inert — worse for the next
  reader than leaving it visibly odd. Apply the constant at preview.rs:74, preview.rs:292 and
  utils.rs:189; leave lib.rs:227's literal in place with a comment pointing at B54.
- **B41's namespace-leak half is not auto-applied.** B41 also proposes renaming
  `TimeTrackingMaybeCloseIfInvisible` or dropping its command indirection. Renames are a
  disabled-by-default category under tidy. Apply only the `.desc()` / `.nargs()` half; record the
  rename as decision-needed.

## Invariants this work depends on

Stated explicitly so a later change that flips one can be traced back here.

1. **Command names are the public API.** All seven `TimeTracking*` names must survive byte-identical.
   Pinned by `test_time_tracking_with_config_creates_commands` (integration_tests/src/lib.rs:385).
   T12 and B41 both restructure the registration site; neither may alter a name.
2. **`M._internal` is the Lua test seam.** T4 and T8 widen it. `integration_tests/lua/spec_*.lua` reach
   in through it; anything added there is now load-bearing for tests, not private.
3. **`create_or_update_preview`'s public signature is consumed by ~10 integration tests.** T14 may
   extract helpers beneath it but must not change its signature.
4. **The preview buffer is identified by name suffix**, not by a handle, in three places. T5 centralises
   that; if the naming scheme ever changes, `PREVIEW_BUF_NAME` and `is_preview_buf` are the only sites.
5. **`is_version_newer` has no production caller** (bughunt B55 — the real update gate is a string
   inequality at init.lua:324). T9 refactors this function on the assumption B55 resolves by *making it
   live*, not by deleting it. If B55 is later resolved by deletion, T9's work is discarded — that is a
   known, accepted risk of running T9 before B55.

## Per-task contract

1. Read the finding.
2. If `risk: high` — characterization tests first, committed separately.
3. Apply the change.
4. Run `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt`. Lua has no linter;
   re-read edits and sanity-check with `luajit -bl <file> >/dev/null`.
5. Every 5 findings or at end of chain: full test suite (`cargo test`, `integration_tests/run_tests.sh`,
   `integration_tests/lua/run_lua_tests.sh`).
6. Strip the finding: `todo-parser TIDY.md --strip T<n>` (or `todo-parser bughunt.md --strip B<n>` for
   the five folded bugs — bughunt.md carries the same strip-on-fix standing instruction).
7. Commit code + strip together: `tidy(<lens>): <summary> [T<n>]`, or `fix(<area>): <summary> [B<n>]`
   for the bug fixes. One commit per finding; never bulk.

## Out of scope

- The 26 unchecked TIDY.md items stay for the next run.
- The 25 remaining bughunt.md findings stay open.
- Renames and public-API signature changes (T42 is already marked decision-needed).
