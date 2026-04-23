# Panic Fix Design — 2026-04-23

## Problem

With the nvim-oxi `neovim-0-12` branch, the plugin entry point (`luaopen_time_tracking_nvim`) is an `extern "C"` function with no `catch_unwind`. Any Rust panic inside the plugin initialization or callbacks crosses this FFI boundary and triggers a fatal "panic in a function that cannot unwind" abort.

Confirmed panic source: `Config::get_no_args()` in `time-tracking-cli` calls `Config::load(false).expect("Could not load configuration")`, which panics if the config file is missing or malformed. This same pattern affects all callbacks via nvim-oxi's `c_fun` wrapper (also `extern "C"`, no `catch_unwind`).

## Approach C: Fix Upstream + Add Defensive catch_unwind

### Part 1 — Fix `time-tracking-cli`

Change `Config::get_no_args()` (and `Config::get()`) to return `Result<&'static Config>` instead of panicking. Callers in the plugin must handle the `Result`.

### Part 2 — Add `catch_unwind` at the plugin entry point

Wrap the body of `time_tracking_nvim()` in `std::panic::catch_unwind(AssertUnwindSafe(...))`. If a panic still occurs (e.g., from a future dependency), convert it to a Neovim error instead of aborting.

### Part 3 — Add `catch_unwind` in each plugin callback

Wrap each closure passed to `Function::from_fn` in a `catch_unwind` helper so that any unexpected panic inside a callback is caught, logged via `err_writeln`, and surfaced as a recoverable error rather than aborting Neovim.

## Data Flow

```
luaopen_time_tracking_nvim  [extern "C"]
  └─ entrypoint::entrypoint  [inlined, no catch_unwind]
       └─ time_tracking_nvim()  [catch_unwind wraps everything here]
            └─ Config::get_no_args() → Result<&'static Config>  [no more panic]
            └─ time_tracking_with_config(config)
                 └─ Function::from_fn(catch_unwind_closure(...))  [per-callback wrap]
```

## Error Handling

- Entry point panic → returned as `nvim_oxi::Error::Other(...)` → Neovim shows Lua error
- Callback panic → `err_writeln` notification + returned as `nvim_oxi::Error::Other(...)`
- Config load failure → returned as `Result::Err`, plugin load fails with a clear message

## Testing

- `cargo build` must pass
- Existing unit and integration tests must remain green
