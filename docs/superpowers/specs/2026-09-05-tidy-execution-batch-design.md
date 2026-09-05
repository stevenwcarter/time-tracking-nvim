# Tidy execution — 17 findings across Rust, Lua, and CI

Date: 2026-09-05
Branch: to be created off `main` @ 4066745
Source: `TIDY.md`, 17 items marked `[x] execute` (of 23 active; 1 skipped to the archive, 5 left unmarked)

## Scope

Every finding below was re-verified against current code on 2026-09-05 — the refresh pass removed two
resolved items (T24, T39), narrowed four to their surviving halves (T27, T34, T35, T36), promoted one
(T30), and corrected line numbers throughout. **The line numbers here were correct at 4066745 and will
drift as tasks land.** Locate by symbol and by the quoted code, and re-derive line numbers before each
edit; never apply a hunk at a remembered offset.

Two decisions were confirmed with the user before writing this spec:

- **T42 is approved** despite being a public-API signature change, which this skill otherwise never
  auto-applies. Rationale accepted: the blast radius is entirely in-repo (3 `src/` callers, ~20
  integration-test call sites), and a cdylib Neovim plugin has no external Rust consumers of
  `pub mod utils`. Its task must end with a full integration-test run.
- **T7 gets characterization tests first**, per the per-task contract for `risk: high`.

### Selected findings

| ID | Risk | File(s) | Summary |
|---|---|---|---|
| T31 | low | src/lib.rs, src/preview.rs | Five names imported into lib.rs but used only by preview.rs |
| T35 | low | src/preview.rs | `use super::*` is the crate's last wildcard import |
| T34 | low | src/lib.rs | `use std::io::Write;` repeated as a mid-function item twice |
| T36 | low | src/preview.rs | `match Option` where the file elsewhere uses let-else |
| T40 | low | src/preview.rs | Two identical match-log-swallow wrappers |
| T41 | low | src/preview.rs | `auto_open_preview_impl` / `auto_close_preview_impl` needlessly `pub` |
| T37 | medium | src/preview.rs | Every render resolves the preview twice |
| T38 | low | src/preview.rs | Preview write allocates a Vec plus one String per line |
| T42 | medium | src/utils.rs | Predicates take `Window`/`Buffer` by value (**public-API change**) |
| T43 | low | src/utils.rs | `get_buffer_content` allocates one String per line |
| T21 | low | lua/…/init.lua | `default_config` carries two never-implemented commented-out keys |
| T22 | low | lua/…/init.lua | `get_platform_info` rebuilds a table and calls `uv.os_uname()` twice |
| T27 | low | lua/…/init.lua | The message-chunk prefix is hand-written at 21 call sites |
| T7 | **high** | lua/…/health.lua | `M.check` is 119 lines of seven sequential probe sections |
| T23 | low | init.lua, build.sh, release.yml | Target-triple mapping encoded three times |
| T6 | medium | ci.yml, release.yml, build.sh | Version-extraction snippet copied three times; two CI jobs near-clones |
| T30 | medium | plugin/*.vim, README.md | Loader guard and README claim 0.11; the build floor is 0.12 |

## Sequencing

Eight chains. Ordering within a chain is load-bearing; the chains themselves run in the listed order.

### Chain 1 — Rust imports (T31 → T35 → T34)

T31 and T35 both rewrite the same two import blocks and the finding text of T31 says they pair. Doing
them in either order works only if done back to back; interleaving anything else between them leaves
the crate in a state where `use super::*` is the only thing keeping preview.rs compiling.

1. **T31** — delete `use nvim_oxi::api::opts::OptionOptsBuilder;` (src/lib.rs:11) and
   `use nvim_oxi::api::{Buffer, Window};` (src/lib.rs:13); narrow src/lib.rs:21 to
   `use crate::utils::any_tracking_visible;`. Extend the existing explicit import at src/preview.rs:3
   with `get_buffer_content, is_time_tracking_file`, and add
   `use nvim_oxi::api::{Buffer, Window, opts::OptionOptsBuilder};` alongside it. This is a *move*:
   deleting the lib.rs side without adding the preview.rs side breaks the build.
2. **T35** — replace `use super::*;` at src/preview.rs:1 with the explicit list. After T31 most of what
   the glob supplied is already imported explicitly, so this reduces to whatever remains (macros
   `log_error!`/`debug_log!`/`log_info!` may need `crate::` paths instead). Let clippy name the set:
   `cargo clippy --all-targets -- -W clippy::wildcard_imports`.
3. **T34** — hoist `use std::io::Write;` to the module header beside
   `use std::panic::{self, AssertUnwindSafe};`; delete the copies at src/lib.rs:101 and src/lib.rs:118.
   Leave the copy inside the `debug_log!` macro body (src/lib.rs:64) — a macro needs its own import to
   stay hygienic at arbitrary expansion sites.

### Chain 2 — preview.rs body (T36 → T40 → T41)

4. **T36** — `let mut buf: Buffer = if let Some(b) = preview { b } else { create_preview_buffer()? };`
   at src/preview.rs:575-578. Pure readability; `create_preview_buffer` already exists.
5. **T40** — add `fn log_and_swallow(label: &str, r: Result<()>) -> Result<()>`; delegate both
   `auto_open_preview` (src/preview.rs:664-676) and `auto_close_preview` (src/preview.rs:700-712) to it.
   Clears `clippy::ignored_unit_patterns` at :670 and :706. **Leave the third hit at src/lib.rs:165
   (`schedule(|_| ...)`) alone** — unrelated to this finding.
6. **T41** — drop `pub` from `auto_open_preview_impl` (src/preview.rs:680) and `auto_close_preview_impl`
   (src/preview.rs:714). T40 may fold `auto_close_preview_impl` away entirely; if it does, T41 reduces
   to the one remaining function. Do T40 first so T41 sees the final shape.

### Chain 3 — preview.rs render path (T37 → T38)

7. **T37** — split `create_or_update_preview_with(found, output)` out of `create_or_update_preview`
   (src/preview.rs:563-589, its own `find_preview()?` at :570), leaving the old name as a one-line
   delegate. Have the three callers bind `found` once and pass it through: `update_preview_fn`
   (probe at :366), `toggle_preview_fn` (:347), `auto_open_preview_impl` (:692). Medium risk — it moves
   a lookup that the throttle's leading-edge path depends on; run the integration suite after.
8. **T38** — the finding offers two routes and the implementer must take **(b)**: widen the test seam
   rather than working around it. `write_preview_contents_with(buf, output, write_lines)`
   (src/preview.rs:425-429) pins its callback to `fn(&mut Buffer, Vec<String>) -> Result<()>`; change
   that to `fn(&mut Buffer, Vec<&str>) -> Result<()>`, pass `output.lines().collect()`, and update
   `set_preview_lines` (src/preview.rs:390-393) to match. That removes the per-line `String` allocation
   while keeping the injectable-failure seam the tests rely on. Do **not** collapse the seam into a
   direct `buf.set_lines(..., output.lines())` — that deletes the seam and the test that uses it.

### Chain 4 — utils.rs (T42 → T43)

9. **T42** — take `&Window` / `&Buffer`; update callers at src/utils.rs:132, :137, :226 and every
   integration-test call site (~20, by value today). **Public-API signature change, user-approved.**
   This task ends with a full `cd integration_tests && cargo test` run, not just a build.
10. **T43** — `content.push_str(&line.to_string_lossy());` at src/utils.rs:189. `to_string_lossy`
    returns `Cow<'_, str>` and delegates to the same `self.inner` as `Display::fmt`, so behaviour is
    preserved. Optionally `reserve` capacity up front.

### Chain 5 — Lua init.lua (T21 → T22 → T27)

11. **T21** — delete the three lines at lua/time-tracking-nvim/init.lua:30-32 (the
    `-- Add any configuration options here` placeholder, `-- auto_start = true,`,
    `-- preview_width = nil, ...`). The live keys below them stay.
12. **T22** — hoist `local uname = uv.os_uname()` so `sysname` and `machine` come from one struct
    (currently two calls, init.lua:66 and :67); hoist the `platform_mappings` literal (init.lua:69-81)
    to a module-level `PLATFORM_MAPPINGS` beside `CURL_HARDENING` (init.lua:209); extract
    `local function normalize_arch(os_name, arch)` for the amd64 / darwin-aarch64 remaps
    (init.lua:84-97), keeping their explanatory comments on the helper.
13. **T27** — add `local function notify(kind, chunks)` beside `echo` (init.lua:16) that prepends the
    prefix chunk; route the 21 call sites through it (init.lua:640, 656, 812, 819, 829, 857, 884, 905,
    922, 932, 939, 944, 1004, 1014, 1030, 1039, 1072, 1079, 1086, 1091, 1103). **No prefix-override
    parameter** — the `test:` variant that would have needed one no longer exists.

### Chain 6 — health.lua (T7, two commits)

14. **T7a — characterization tests first.** `M.check` (health.lua:20, 119 lines) has `risk: high`, so
    per the per-task contract: add Lua characterization tests to `integration_tests/lua/` covering the
    seven probe sections and, critically, the early-return-on-failure behaviour that the decomposition
    must preserve. Confirm they pass against the **unchanged** function. Commit as
    `test: characterize M.check before tidy [T7]` — separate from the refactor.
15. **T7b — decompose.** Promote each existing section comment to a helper, using `nil` returns as the
    abort signal the current early returns express: `check_platform(internal)` → platform_info or nil;
    `check_binary(plugin_root, platform_info)` → binary_path or nil (covering both the filereadable and
    fs_stat sections); `check_versions(binary_path, internal)`; `check_cpath(plugin_root)`;
    `check_native_module()`; `check_commands()`; `check_external_tools()`. `M.check` becomes ~15 lines
    of `if not X then return end`. Note the helpers no longer need to recompute from
    `plugin_root`/`platform_info` — they can call `internal.*` directly — so signatures may simplify
    further than the list above. The characterization tests from T7a must stay green.

Runs before Chain 7 because T23 edits a pointer comment in health.lua, and doing that after the
decomposition avoids editing a line that is about to move.

### Chain 7 — cross-language mapping and CI dedup (T23 → T6)

16. **T23** — do **not** try to unify the three-language mapping; the finding is explicit that no
    single source exists without a codegen step this project does not use. Remove only the derivable
    redundancy: drop release.yml's `nvim_name` matrix column and derive it at release.yml:89 via
    `basename` plus `sed 's/^lib//'`; in build.sh:16-33 set `LIB_EXT` and a `LIB_PREFIX`, deriving
    `LIB_NAME` as `${LIB_PREFIX}time_tracking_nvim.${LIB_EXT}`. Then comment each of the three sites
    pointing at the other two, and update the supported-platforms hint in health.lua (was :24, now :33,
    and will have moved again after Chain 6) as the fourth site any new target must touch.
17. **T6** — add `scripts/versions.sh` exporting `cargo_version` and `lua_version` via the two greps;
    source it from build.sh:47 and both workflow jobs. Then collapse ci.yml's version-sync job
    (ci.yml:106-122) and release.yml's version-check job (release.yml:18-39) — the former is the latter
    minus the tag comparison — by giving version-sync a `workflow_call` trigger, or a shared composite
    action under `.github/actions/version-check`, taking an optional expected tag. `scripts/` already
    exists (it holds the pre-commit hook), so this adds no new top-level directory.

### Chain 8 — version floor (T30)

18. **T30** — guard on `has('nvim-0.12')` at plugin/time-tracking-nvim.vim:10 and update the message at
    :11; correct README.md:122 (`- Neovim 0.11+`). Prefer
    `echohl WarningMsg | echomsg ... | echohl None` over `echoerr` at plugin-sourcing scope. The real
    floor is Cargo.toml:27's `features = ["neovim-0-12"]`; today the guard and the README agree with
    each other and both disagree with the build, so a 0.11 user is told twice they are supported and
    then fails at `dlopen`.

## Invariants this batch depends on

1. **`write_preview_contents_with`'s callback is a test seam**, not incidental structure — a test
   injects a failing writer through it. T38 widens the seam; it must not remove it.
2. **`preview_is_open()` reflects the current tabpage only** (bughunt B45, landed). T37 moves that
   lookup around; the per-tabpage semantics must survive, or a second tabpage stops getting its own
   preview.
3. **The throttle's leading edge renders synchronously** and its trailing render arrives via a
   `timer_start` callback on the main loop. T37 and T38 both touch that render path; the 52 existing
   integration tests, including the insert-mode one, are the guard.
4. **`integration_tests` is edition 2024 and format-checked in CI** as of 4066745. Any Rust touched here
   must survive `cargo fmt --all -- --check` plus both clippy steps.

## Out of scope

- T20 (release archive contents) — user-skipped to the archive this pass.
- The 5 items left unmarked in `TIDY.md`.
- `bughunt.md` B61/B62 and anything in `TYPECHECK.md`.

## Verification

Per-finding: `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check` at the root,
plus the same two with `working-directory: integration_tests`. At every milestone (5 findings, or the
end of a chain): `cd integration_tests && cargo test` — 52 tests, all must stay green. Lua changes also
run `./integration_tests/lua/run_lua_tests.sh`.

**Environment note:** this machine sets a global `CARGO_TARGET_DIR` that collides with the
workspace-excluded `integration_tests` crate. Prefix cargo invocations with `env -u CARGO_TARGET_DIR`,
or the integration crate fails to compile with ~56 spurious `E0308` errors.
