# Gate Debug Logging Behind Environment Variable

**Date:** 2026-04-25

## Problem

Debug `stderr.write_all` calls added during troubleshooting in `src/lib.rs` fire unconditionally on every plugin startup. Because Neovim writes stderr output during terminal initialization, these messages appear diagonally across the screen before the editor is fully rendered — visible noise on every launch.

## Goal

Silence diagnostic startup messages by default while keeping them accessible via an environment variable for future debugging sessions. Genuine error messages (config load failure, panic caught, `time_tracking_with_config` failure) must remain unconditional.

## Design

### New macro: `debug_log!`

Add to `src/lib.rs` alongside the existing `log_info!` and `log_error!` macros:

```rust
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("TIME_TRACKING_DEBUG").is_ok() {
            use std::io::Write;
            let _ = std::io::stderr().write_all(format!($($arg)*).as_bytes());
        }
    };
}
```

The env var name `TIME_TRACKING_DEBUG` follows the conventional `<PROJECT>_DEBUG` pattern. Any non-empty value enables it (presence check only).

### Changes in `time_tracking_nvim()` — `src/lib.rs`

Six diagnostic blocks are converted from raw `stderr.write_all` to `debug_log!`:

| Message | Action |
|---|---|
| `[ttnvim] entered time_tracking_nvim` | → `debug_log!` |
| `[ttkvim] hook installed, starting catch_unwind` | → `debug_log!` |
| `[ttkvim] inside catch_unwind closure` | → `debug_log!` |
| `[ttkvim] config loaded, calling time_tracking_with_config` | → `debug_log!` |
| `[ttkvim] time_tracking_with_config succeeded` | → `debug_log!` |
| `[ttkvim] catch_unwind returned` | → `debug_log!` |

Three genuine error paths remain as raw `stderr.write_all`:

| Message | Reason kept |
|---|---|
| `[ttkvim] time_tracking_with_config FAILED: {e}` | Real failure, always relevant |
| `[ttnvim] error: {e}` | `Ok(Err(e))` arm — real failure |
| `[ttnvim] panic caught: {msg}` | Panic recovery — always relevant |

The panic hook (`panic::set_hook`) is kept as-is — it only fires during an actual panic, not on normal startup.

### Out of scope

- `src/preview.rs` — `log_error!` calls there use `nvim_oxi::api::err_writeln` (Neovim's message area), not stderr. They are not causing the startup noise and are not changed.
- `log_info!` and `log_error!` macros — no changes to these.

## Usage

To re-enable diagnostics when debugging:

```sh
TIME_TRACKING_DEBUG=1 nvim
```
