# Design: code-health audit batch (2026-09-03)

Execution spec for the 27 findings the user marked `[x] execute` in `bughunt.md`.
Source of truth for each item's evidence and proposed fix is `bughunt.md`; this
document records the design decisions, ordering, and per-item acceptance
criteria that the implementation plan turns into tasks.

## Baseline (verified on `codehealth/2026-09-03` @ f54bab2)

| Check | Command | Result |
|---|---|---|
| build | `cargo build` | pass |
| format | `cargo fmt -- --check` | pass |
| lint | `cargo clippy -- -D warnings` | pass (0 warnings) |
| unit tests | `cargo test` | pass (0 tests — the crate has none) |
| integration tests | `cd integration_tests && cargo test` | pass, 21/21 |

Note: the integration suite initially failed to compile with duplicate
`nvim_oxi_api` / `time_tracking_cli` types. That was a stale rlib in the shared
`CARGO_TARGET_DIR`, not a source defect; a forced rebuild of the parent crate
cleared it. No source change was required and none is in scope here.

## Scope

27 findings. `decision-needed` markers in `bughunt.md` (notably the
nvim-0.11-vs-`neovim-0-12`-feature floor) are explicitly **out of scope** and
must not be auto-applied.

## Test strategy

The Rust side has a working harness: `integration_tests/` runs a real Neovim via
`#[nvim_oxi::test]`. Every `risk: high` Rust finding gets a RED characterization
test there before its fix.

The Lua side has **no harness at all**, and 9 of the 27 findings are Lua. Task
T0 therefore builds one before any Lua finding is touched:

- `lua/time-tracking-nvim/init.lua` gains an `M._internal` table exporting the
  pure helpers under test (`get_platform_info`, `is_version_newer`, and the new
  `normalize_os_name` / `is_trusted_download_url` helpers). `_internal` is a
  test seam, documented as unstable, not part of the public API.
- `integration_tests/lua/` holds plain-Lua spec files and a tiny assert harness;
  `integration_tests/lua/run_lua_tests.sh` runs them under
  `nvim --headless -u NONE`, exiting non-zero on failure.
- `.github/workflows/ci.yml` gains a step running that script on ubuntu-latest.

Only pure, side-effect-free helpers are covered. Network paths (`download_binary`)
are verified by construction — argv tables are built by a helper that the Lua
tests assert on — not by mocking `vim.system`.

### Invariants this feature depends on

Recorded so a later change touching these can grep for who relies on them:

1. **The preview buffer is identified by its name suffix `[Time Tracking Preview]`.**
   B20/B34's handle cache and B14's dirty-check are correct only while that name
   is unique and stable. `bufhidden=wipe` (preview.rs:108) means the handle can
   go invalid at any time, so every cached handle read must revalidate with
   `is_valid()`. Pinned by `test_multiple_preview_creation_updates_same_buffer`
   plus new cache-invalidation tests.
2. **`is_buf_time_tracking_file` requires a `.md` extension** (utils.rs:62).
   B13's autocmd narrowing to `*.md` is behavior-preserving only because of this.
   Pinned by `test_is_buf_time_tracking_file_with_txt_in_data_dir` and a new
   test asserting the extension gate directly.
3. **`Config` is loaded once at plugin init and never mutated** (lib.rs:86).
   B15's `OnceLock` memoization of the data directory is sound only under this.
   Pinned by a new test asserting the memoized value matches a fresh
   `canonicalize` of `config.get_data_directory()`.
4. **`PLUGIN_VERSION` (init.lua) equals `version` (Cargo.toml).** B11 restores
   this and B18 makes the download path depend on it. Pinned by a CI check, not
   only by prose.
5. **Release archives carry a `SHA256SUMS` asset.** B1's verification depends on
   release.yml publishing it. Pinned by the release workflow change landing in
   the same commit as the verifying code.

## Ordering

Grouped by file so each commit is small and revertable, with coupled findings
adjacent so a later one builds on an earlier refactor. Milestone full-suite runs
after every 5 findings.

`T0` → utils.rs (`B17, B6, B15, B21`) → preview.rs
(`B5, B9, B23, B33, B31, B38, B20, B34, B14`) → lib.rs (`B13, B22, B8, B3`) →
Lua (`B30, B11, B18, B12, B25, B1, B2, B10, B16`) → CI (`B4`).

`B20` must land before `B34` (the extracted helper is backed by B20's cache) and
both before `B14` (its dirty-check invalidates at the same points).

## Per-finding acceptance criteria

### utils.rs

**B17 — canonicalize rejects a not-yet-written file.** Canonicalize
`buffer_path.parent()` and rejoin the file name; fall back to the raw path when
the parent also does not resolve. RED test: a buffer named
`<data_dir>/2026-09-03.md` that was never written to disk is recognised as a
tracking file. Existing on-disk tests must stay green.

**B6 — swallowed data-directory error.** Keep the message; emit it once via
`log_error!` behind a `std::sync::Once` so the per-keystroke path cannot spam,
then return `Ok(false)` as before. The per-buffer canonicalize failure stays at
`debug_log!` (unsaved and scratch buffers hit it legitimately). RED test: a
config whose `data_directory` does not exist returns `Ok(false)` and does not
panic; assert the `Once` fires at most once across repeated calls.

**B15 — data dir re-canonicalized per call.** Memoize into a
`static DATA_DIR: OnceLock<Option<PathBuf>>`. The buffer-path canonicalize at
utils.rs:34 stays per-call. Depends on invariant 3. Because the memo is
process-global and the integration tests construct several configs in one
process, the memo must key off the config's data directory, or the tests must
be written to tolerate first-write-wins — resolve this in implementation and
state which was chosen.

**B21 — `get_buffer_content` allocations.** Build with `String::with_capacity`
and push `'\n'` between lines. Signature unchanged; covered by the existing
`test_get_buffer_content` / `test_get_buffer_content_empty`.

### preview.rs

**B5 — blocking `thread::sleep` on the UI thread.** Delete both sleeps
(preview.rs:207, 261). The race they papered over is already covered by the E242
guard (135-143) and the `list_wins().len() == 0` bail (81). RED test: assert
`auto_open_preview` on a non-tracking buffer returns in well under the former
200 ms — a timing assertion with a generous margin, and assert the E242/no-window
guards still hold.

**B9 — raw `eprintln!` garbling the screen.** Replace both with `log_error!`
(which routes through `err_writeln` and reaches `:messages`). Demote the E242
special case to `debug_log!` rather than dropping it silently.

**B23 — `TimeTrackingToggle` silent no-op.** In `toggle_preview_fn` **only**,
replace the bare early `return Ok(())` with a message naming the buffer name and
`config.get_data_directory()`. `update_preview_fn` and `auto_open_preview` stay
silent. RED test: calling `toggle_preview_fn` outside the data dir produces a
message and still creates no preview.

**B33 — `close_preview` propagates E444.** When the preview is the last window,
replace its buffer with a fresh normal buffer instead of closing it. Downgrade
`win.close(false)?` to a logged non-fatal so a close failure never becomes a
repeating error on every autocommand. RED test: preview as sole window →
`close_preview()` returns `Ok`, the user is left in a listed modifiable buffer,
and no preview buffer remains.

**B31 — preview split inherits `number`/`wrap`/`signcolumn`.** Inside the
existing `if !is_open` block set `number=false`, `relativenumber=false`,
`wrap=false`, `signcolumn="no"`, `foldcolumn="0"`, `cursorline=false`,
`spell=false` on the new window. Nothing changes for users who never open a
preview.

**B38 — width from global `&columns`.** Capture the source window width before
splitting; `width = (total_cols / 3).min(src_w.saturating_sub(20)).max(20)`, and
skip the split entirely (return `Ok(())`) when `src_w < 40` so a narrow terminal
gets no preview rather than an E36 and a wrecked layout.

**B20 — preview buffer re-discovered by scanning all buffers.** Add
`thread_local! { static PREVIEW_BUF: RefCell<Option<Buffer>> }`. Accept the
cached handle when `is_valid()`; otherwise fall back to the `list_bufs` scan and
repopulate. Store on creation (preview.rs:101), clear in `close_preview`.
Depends on invariant 1.

**B34 — preview lookup duplicated across six sites.** Extract one
`fn find_preview() -> Result<Option<(Buffer, Option<Window>)>>` resolving buffer
and containing window in a single pass, backed by B20's cache, and call it from
all six sites (14, 52, 87, 125, 178, 267).

**B14 — full rewrite of the preview buffer every keystroke.** Cache the last
written output in a `thread_local!`; early-return from the write block when the
new output equals the cache **and** the preview buffer is still `is_valid()`.
Clear the cache on scratch-buffer creation and in `close_preview`. RED test:
two identical `create_or_update_preview` calls perform one write (observe via
`b:changedtick`), a differing call writes, and a wipe-then-recreate always
writes.

### lib.rs

**B13 — `TextChanged` pattern `*`.** Narrow to `*.md`. Behavior-preserving under
invariant 2.

**B22 — `WinClosed` counts the closing window.** Read the closing window ID from
`<amatch>` in the `TimeTrackingMaybeCloseIfInvisible` command and pass it to
`any_tracking_visible` so that window is skipped, mirroring the existing
skip-the-preview branch (utils.rs:85-90). This changes `any_tracking_visible`'s
signature — an internal `pub` in `utils`, used by the integration tests, so
those call sites update with it. RED test: with the sole tracking window
supplied as the excluded ID, `any_tracking_visible` returns false.

**B8 — config load failure registers zero commands, silently.** In the
`Ok(Err(e))` and `Err(payload)` arms call
`api::err_writeln("[time-tracking-nvim] failed to initialize: {e}")` and
populate the returned `Dictionary` with an `error` key. `init.lua` checks
`native.error` after each `pcall(require, ...)` (378, 450, 503) and surfaces it
instead of reporting success. The never-return-`Err` choice at lib.rs:108 is
deliberate and stays.

**B3 — no debounce on the per-keystroke path.** Enable nvim-oxi's `libuv`
feature; hold a `thread_local! { static PENDING: RefCell<Option<TimerHandle>> }`;
in the autocmd path stop any in-flight timer and re-arm a ~150 ms one-shot that
does the render. `:TimeTrackingUpdate` invoked explicitly renders immediately —
so the command needs to distinguish an explicit call from the autocmd one
(separate command, or a `bang`/arg the autocmd passes). Verify `TimerHandle` is
actually available on the pinned nvim-oxi revision **before** committing to
this; if it is not, stop and convert B3 to a `decision-needed` marker rather
than inventing a substitute mechanism.

### Lua

**B30 — Windows detection.** Normalize the OS name alongside the existing arch
normalization: `windows_nt` / `mingw*` / `msys*` → `windows`, before the table
lookup. RED Lua test on `_internal.normalize_os_name` and `get_platform_info`.

**B11 — `PLUGIN_VERSION` drift.** Set init.lua:8 to `0.1.7`. Add a `version-sync`
CI step comparing `^version = ` in Cargo.toml against `PLUGIN_VERSION = ` in
init.lua, failing on mismatch; add the same check to release.yml before the
build matrix, additionally asserting `${GITHUB_REF#refs/tags/v}` equals the
Cargo.toml version. Update DEVELOPMENT.md's Release Process step 1 to name both
files.

**B18 — always fetches `/releases/latest`, stamps the requested version.** When
`expected_version` is set, request `/releases/tags/v<expected_version>`, falling
back to `/latest` only when nil. Record the actual `release_info.tag_name` in
the version file (drop the `expected_version or` precedence). Warn naming both
when the resolved tag differs from what was requested.

**B12 — curl hardening.** Add
`--proto =https --proto-redir =https --tlsv1.2 --fail-with-body --max-redirs 5
--max-time 60 --connect-timeout 10 --retry 2` to both invocations. Guard the
decoded API response: bail with a clear message when it is not a table or
`assets` is not a table, reporting `release_info.message` verbatim when present,
so a 403 rate-limit stops claiming "your platform is unsupported".

**B25 — unvalidated `browser_download_url`.** Reject unless the URL matches
`^https://[%w%.%-]+%.githubusercontent%.com/` or
`^https://github%.com/stevenwcarter/time%-tracking%-nvim/`. Insert `"--"` before
the URL in the argv so a leading dash can never be read as a curl flag. RED Lua
test on `_internal.is_trusted_download_url` covering a github.com asset URL, an
objects.githubusercontent.com URL, a foreign host, and a leading-dash value.

**B1 — downloaded native library dlopen'd unverified.** Two sides, one commit:

- `release.yml` computes and publishes a `SHA256SUMS` asset covering every
  archive.
- `download_binary` fetches `SHA256SUMS`, verifies the archive digest **before
  extracting**, and on mismatch deletes the temp dir and calls
  `callback(false, ...)` — never reaching the copy at line 257.

Verification is **fail-closed**: a missing or unparseable `SHA256SUMS`, or a
digest mismatch, aborts the download. Releases at or before v0.1.7 carry no such
asset, so pinning an old tag will now refuse to auto-download; the escape hatch
is an explicit `setup({ allow_unverified_download = true })`, documented in the
README next to `auto_download`. This is a deliberate, user-visible behavior
change — call it out in the commit message. Digest is computed with
`vim.fn.sha256` over the read bytes where available, falling back to a
`sha256sum` / `certutil -hashfile` subprocess.

**B2 — unrecoverable messages and startup hit-enter prompts.** Add one local
`echo(chunks, level)` helper routing through `vim.notify`, and send all 32
`nvim_api_echo` sites through it. Keep `history = false` only for the two
transient progress notices (337, 422). Collapse the two multi-line startup walls
(476-483, 485-495) to a single line each, pointing at
`:lua require('time-tracking-nvim').version_info()` for detail.

**B10 — no `:checkhealth`.** Add `lua/time-tracking-nvim/health.lua` with
`M.check()` on `vim.health.start/ok/warn/error`, reusing what `M.test()` already
does: platform target, binary path + readability + size, `.version` contents vs
`PLUGIN_VERSION`, `package.cpath`, `pcall(require, ...)`, and
`vim.fn.executable` for curl/tar/unzip. Rewrite the README Troubleshooting
section to point at `:checkhealth time-tracking-nvim` and
`TIME_TRACKING_DEBUG=1 nvim 2>/tmp/ttnvim.log`. Confirm `health.lua` ships — it
lives under `lua/`, which release.yml already copies.

**B16 — build.sh writes where `setup()` never looks.** Copy to
`lua/time_tracking_nvim.${LIB_EXT}` (creating `lua/` if needed) and write a
matching `.version` file containing the Cargo.toml version so auto-update does
not immediately overwrite a local build. Update build.sh's closing instructions
and DEVELOPMENT.md "Testing Locally" to use
`setup({ auto_download = false, auto_update = false })`. Add
`lua/*.so`, `lua/*.dylib`, `lua/*.dll`, `lua/*.version` to `.gitignore`.

### CI

**B4 — floating third-party actions in a `contents: write` job.** Pin every
third-party `uses:` to a full 40-char commit SHA with a trailing `# vX.Y.Z`
comment, across release.yml (47, 52, 141) and ci.yml (26, 29, 33, 76). Move
`softprops/action-gh-release` off the abandoned v1 line to a pinned current
release. Add a Dependabot config with `package-ecosystem: github-actions` to
keep the SHAs maintained. `actions/*` are first-party and may stay on major
tags, but pin them too if it costs nothing. The SHAs must be looked up for real
— do not invent them; if they cannot be resolved offline, say so rather than
guessing.

## Per-task contract

1. Read the finding in `bughunt.md` (`todo-parser bughunt.md --id B<n> --full`).
2. If `risk: high`, write the regression test first, confirm it FAILS, commit as
   `test: characterize <unit> before fix [B<n>]`.
3. Apply the fix; the test goes GREEN.
4. Run `cargo fmt -- --check`, `cargo clippy -- -D warnings`, `cargo test`, and
   `cd integration_tests && cargo test`. After T0, also run the Lua suite.
5. `todo-parser bughunt.md --strip B<n>`.
6. `git add -A && git commit -m 'fix(<category>): <summary> [B<n>]'` — the code
   change and the strip land in the **same** commit.

Never bypass a failing check with `--allow-dirty` or `--no-verify`. Do not
refactor existing tests; add new ones. If executing a finding turns out to
require a public-API signature break or an architectural change beyond the
above, convert it to a `decision-needed` marker and skip it.
