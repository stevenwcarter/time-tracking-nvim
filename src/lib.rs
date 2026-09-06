//! Neovim plugin (loaded as a cdylib) that renders a live time-tracking day
//! summary for the buffer being edited into a side split.
//!
//! `time_tracking_nvim` is the entry point Neovim calls when the native module
//! is `require`d. It never returns `Err`: an initialization failure is reported
//! through the `error` key of the dictionary it returns, because throwing out of
//! the plugin entry point aborts Neovim on macOS (see the comment there).

use std::cell::RefCell;
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;

use nvim_oxi::api::types::{CommandArgs, CommandNArgs};
use nvim_oxi::schedule;
use nvim_oxi::{
    Dictionary, Function, Result,
    api::{self, opts::CreateCommandOpts},
};
use time_tracking_cli::data_svc::ParseSettings;
use time_tracking_cli::{Config, DataService};

use crate::utils::any_tracking_visible;

mod async_rt;
mod preview;
pub mod utils;

pub use preview::{
    auto_close_preview, auto_open_preview, close_preview, create_or_update_preview, throttle_fire,
    toggle_preview_fn, toggle_weekly_preview_fn, update_preview_fn, update_preview_throttled,
};
// Test seams, not interface: see `preview::write_preview_contents_with`,
// `preview::reset_throttle_for_test` and `preview::today_for_test`.
#[doc(hidden)]
pub use preview::{reset_throttle_for_test, today_for_test, write_preview_contents_with};

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        #[allow(unused_imports)]
        use nvim_oxi::api::types::LogLevel;
        // let _ = nvim_oxi::api::notify(&format!($($arg)*), LogLevel::Info, &Default::default());
    };
}

/// Writes a formatted message to Neovim's error output, via `api::err_writeln`.
///
/// That is an API call, so this must not be reached from a fast event context,
/// where calling the Neovim API is illegal. Use `debug_log!` there instead.
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        nvim_oxi::api::err_writeln(&format!($($arg)*));
    };
}

/// Writes a formatted message to stderr, but only when `TIME_TRACKING_DEBUG` is
/// set in the environment.
///
/// Touches no Neovim API, which is why it is usable where `log_error!` is not:
/// during plugin load and from a fast event context, before the API is usable.
#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        if std::env::var("TIME_TRACKING_DEBUG").is_ok() {
            use std::io::Write;
            let _ = std::io::stderr().write_all(format!($($arg)*).as_bytes());
        }
    };
}

fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "time-tracking-nvim: unknown panic".to_owned()
    }
}

thread_local! {
    /// The last error message `catch_nvim_panic` reported.
    ///
    /// A failure here can recur on every keystroke (bughunt B7's repro:
    /// `TextChangedI` re-invoking a command against a stale window handle),
    /// so an unconditional `err_writeln` on every call would spam
    /// `:messages` with an identical line per keystroke. This dedupes
    /// *identical consecutive* messages only — a different failure, or the
    /// same one recurring after something else succeeded in between, is
    /// always reported. Mirrors `LAST_OUTPUT`/`last_output_matches` in
    /// `preview.rs`, applied to error text instead of preview content.
    static LAST_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Report `msg` via `api::err_writeln`, unless it is identical to the last
/// message this reported.
fn report_error_deduped(msg: &str) {
    let already_reported = LAST_ERROR.with(|cell| cell.borrow().as_deref() == Some(msg));
    if already_reported {
        return;
    }
    api::err_writeln(msg);
    LAST_ERROR.with(|cell| *cell.borrow_mut() = Some(msg.to_owned()));
}

/// Clear the dedup latch after a successful call, so a failure that recurs
/// *after* a success in between is reported again rather than staying
/// silenced by an unrelated earlier failure.
fn clear_last_error() {
    LAST_ERROR.with(|cell| *cell.borrow_mut() = None);
}

/// Run `f`, catching both a panic and a propagated `Err`, and report either
/// through `:messages` — but never return `Err` from this function itself.
///
/// Returning `Err` from a `Function::from_fn` callback hits
/// `push_error -> lua_error`, which under `LUAJIT_UNWIND_EXTERNAL`
/// (macOS/arm64) throws a C++ exception through a `nounwind` frame and
/// aborts Neovim — the exact failure mode `time_tracking_nvim`'s own entry
/// point was already fixed to avoid. Every command in `register_commands`
/// is wrapped in this function, so this is the one place that decision has
/// to hold for all of them (this also gives `:TimeTrackingToggle`/
/// `:TimeTrackingUpdate` a diagnostic message on failure, for the first
/// time — bughunt B7 / whats-next W6).
fn catch_nvim_panic<F>(f: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    match panic::catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => {
            clear_last_error();
            Ok(())
        }
        Ok(Err(e)) => {
            report_error_deduped(&format!("[time-tracking-nvim] {}", e));
            Ok(())
        }
        Err(payload) => {
            let msg = panic_message(payload);
            report_error_deduped(&format!("[time-tracking-nvim] panic: {}", msg));
            Ok(())
        }
    }
}

// Test seams, not interface: let the integration tests exercise the
// panic/Err-swallowing behavior directly, the same way
// `write_preview_contents_with` and `reset_throttle_for_test` are exposed.
#[doc(hidden)]
pub fn catch_nvim_panic_for_test<F>(f: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    catch_nvim_panic(f)
}

#[doc(hidden)]
pub fn clear_last_error_for_test() {
    clear_last_error();
}

/// Plugin to provide time tracking previews while editing in Neovim.
#[nvim_oxi::plugin]
fn time_tracking_nvim() -> Result<Dictionary> {
    debug_log!("[ttnvim] entered time_tracking_nvim\n");

    // Install diagnostic hook to capture the real panic source.
    panic::set_hook(Box::new(|info| {
        let msg = format!("[ttnvim] PANIC: {info}\n");
        let _ = std::io::stderr().write_all(msg.as_bytes());
    }));

    debug_log!("[ttkvim] hook installed, starting catch_unwind\n");

    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        debug_log!("[ttkvim] inside catch_unwind closure\n");
        let config = Config::try_get_no_args()
            .map_err(|e| nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(e.to_string())))?;
        debug_log!("[ttkvim] config loaded, calling time_tracking_with_config\n");
        let r = time_tracking_with_config(config);
        match &r {
            Ok(_) => {
                debug_log!("[ttkvim] time_tracking_with_config succeeded\n");
            }
            Err(e) => {
                let _ = std::io::stderr().write_all(
                    format!("[ttkvim] time_tracking_with_config FAILED: {e}\n").as_bytes(),
                );
            }
        }
        r
    }));

    debug_log!("[ttkvim] catch_unwind returned\n");

    let _ = panic::take_hook();

    // Never return Err: push_error → lua_error throws a C++ exception on macOS
    // (LUAJIT_UNWIND_EXTERNAL) which hits the nounwind terminate block → panic_cannot_unwind.
    // Report the failure through the returned dictionary and :messages instead,
    // so the Lua layer can stop claiming success.
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

/// Registers the `TimeTracking*` user commands and the `TimeTrackingNvim`
/// autocommand group, and schedules the startup auto-open.
///
/// This is the whole of initialization; `time_tracking_nvim` calls it on every
/// plugin load. It is `pub`, and takes `config` explicitly instead of loading
/// it, so the integration tests can drive it with a `Config` pointed at a
/// temporary directory.
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

    let status = Function::<(), Dictionary>::from_fn(move |_: ()| -> Result<Dictionary> {
        if !crate::utils::is_time_tracking_file(config)? {
            return Ok(Dictionary::from_iter([(
                "is_tracking_file",
                nvim_oxi::Object::from(false),
            )]));
        }
        let content = crate::utils::get_buffer_content()?;
        Ok(crate::utils::buffer_status(&content, config))
    });

    let data_directory_status =
        Function::<(), Dictionary>::from_fn(move |_: ()| -> Result<Dictionary> {
            Ok(crate::utils::data_directory_status_dict(config))
        });

    let api = Dictionary::from_iter([
        ("status", nvim_oxi::Object::from(status)),
        (
            "data_directory_status",
            nvim_oxi::Object::from(data_directory_status),
        ),
    ]);
    Ok(api)
}

/// `:TimeTrackingOpenToday`: opens today's tracking file, creating it from
/// the configured template if it doesn't exist yet.
///
/// "Today" is resolved via `preview::today()` — Neovim's own local date, read
/// through `strftime` — rather than `time::OffsetDateTime::now_local()`.
/// `now_local()` is effectively always an error in Neovim's multi-threaded
/// process, so a naive implementation would silently fall back to UTC on
/// nearly every real invocation, and near local midnight that can create (or
/// open) the wrong day's file. See `preview::today`'s doc comment for the
/// full story (whats-next W5's fix).
///
/// An existing file is opened as-is and never re-seeded from the template:
/// only the *absence* of the file triggers template expansion.
///
/// The file path and the data directory are resolved through a hermetic
/// [`DataService`] (`get_file_path`/`ensure_data_dir`) rather than hand-rolled
/// `dir.join(...)`/`create_dir_all(...)` calls, the same way
/// [`crate::preview::render_weekly_view`] builds its own — that is the
/// established idiom in this codebase for exactly this pairing of directory
/// resolution and file naming, and `time_tracking_cli` already owns it, so
/// duplicating it here would be a second, independent implementation that can
/// silently drift from upstream.
pub fn open_today_fn(config: &'static Config) -> Result<()> {
    let Some(data_dir) = config.get_data_directory() else {
        log_error!("[time-tracking-nvim] no data directory configured");
        return Ok(());
    };

    let today = crate::preview::today();

    let data_service = DataService::new_with_dir(
        DataService::DEFAULT_CACHE_TIMEOUT_SECONDS,
        PathBuf::from(data_dir),
        ParseSettings::from_config(config),
    );

    let file_path = match crate::async_rt::block_on(data_service.get_file_path(today)) {
        Ok(path) => path,
        Err(e) => {
            log_error!(
                "[time-tracking-nvim] could not resolve today's file path: {}",
                e
            );
            return Ok(());
        }
    };

    if !file_path.exists() {
        if let Err(e) = crate::async_rt::block_on(data_service.ensure_data_dir()) {
            log_error!(
                "[time-tracking-nvim] could not create data directory: {}",
                e
            );
            return Ok(());
        }
        // A template-read failure (e.g. a configured `template_file` that no
        // longer exists) falls back to an empty file rather than blocking the
        // command entirely — the day file is still worth creating and opening
        // — but is logged rather than swallowed, unlike the brief's sketch,
        // so a misconfigured template doesn't fail silently.
        let content = crate::async_rt::block_on(time_tracking_cli::create_template_content(
            &today,
            config.get_template_file(),
        ))
        .unwrap_or_else(|e| {
            log_error!(
                "[time-tracking-nvim] could not build today's file from the template: {}",
                e
            );
            String::new()
        });
        if let Err(e) = std::fs::write(&file_path, content) {
            log_error!("[time-tracking-nvim] could not create today's file: {}", e);
            return Ok(());
        }
    }

    let escaped: String = api::call_function("fnameescape", (file_path.to_string_lossy(),))
        .unwrap_or_else(|_| file_path.to_string_lossy().into_owned());
    api::command(&format!("edit {escaped}"))?;
    Ok(())
}

/// Register the `TimeTracking*` user commands.
fn register_commands(config: &'static Config) -> Result<()> {
    let toggle_preview =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| toggle_preview_fn(config)));

    // The week-at-a-glance counterpart to `:TimeTrackingToggle`. It aggregates
    // the data directory rather than the current buffer, so unlike the day
    // toggle it does not require a tracking buffer to be current.
    let toggle_weekly_preview = Function::from_fn(move |_: CommandArgs| {
        catch_nvim_panic(|| toggle_weekly_preview_fn(config))
    });

    let update_preview =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| update_preview_fn(config)));

    // Update the preview from the TextChanged autocommands, at most once per
    // throttle window.
    let update_preview_throttled_cmd = Function::from_fn(move |_: CommandArgs| {
        catch_nvim_panic(|| update_preview_throttled(config))
    });

    // The render the throttle books with `timer_start`. Internal: that timer
    // is its only caller.
    let throttle_fire_cmd =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| throttle_fire(config)));

    let open_today =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| open_today_fn(config)));

    let auto_open =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| auto_open_preview(config)));

    let auto_close =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| auto_close_preview(config)));

    // `:TimeTrackingClose` is an explicit user request to stop seeing the
    // preview, unlike `close_preview`'s other callers (the invisibility-driven
    // auto-close and the QuitPre autocommand), so it marks the preview
    // dismissed itself right after the close succeeds — see
    // `preview::mark_preview_dismissed`'s doc comment for why `close_preview`
    // does not do this on every caller's behalf.
    let close_preview_cmd = Function::from_fn(move |_: CommandArgs| {
        catch_nvim_panic(|| {
            close_preview()?;
            crate::preview::mark_preview_dismissed();
            Ok(())
        })
    });

    let maybe_close_if_invisible = Function::from_fn(move |args: CommandArgs| {
        catch_nvim_panic(move || {
            // WinClosed sets <amatch> to the window-ID of the window that is
            // about to be removed. BufEnter/TabEnter set it to a buffer name,
            // so those fire the command with no argument.
            let exclude = args.args.as_deref().and_then(|s| s.trim().parse().ok());

            // The visibility rule belongs to the *day* view, which mirrors the
            // buffer being edited: with no tracking file on screen there is
            // nothing for it to mirror. The weekly view is the opposite — it
            // aggregates the data directory precisely so the user can check
            // "how much did I work this week" from wherever they are — so it
            // is exempt. Without the exemption `:TimeTrackingWeeklyToggle`
            // could not be used from a non-tracking buffer at all, since
            // `open_preview_split`'s closing `set_current_win` fires `BufEnter`
            // and would close the split it had just opened.
            //
            // `:TimeTrackingClose`, `:TimeTrackingWeeklyToggle` and `QuitPre`
            // still close it: they call `close_preview` directly.
            if !any_tracking_visible(config, exclude)? && !crate::preview::current_view_is_week() {
                close_preview()?;
            }
            Ok(())
        })
    });

    api::create_user_command(
        "TimeTrackingMaybeCloseIfInvisible",
        maybe_close_if_invisible,
        &CreateCommandOpts::builder()
            .desc("(internal) Close the preview when no tracking file is visible")
            .nargs(CommandNArgs::ZeroOrOne)
            .build(),
    )?;

    let invalidate_buf_cache = Function::from_fn(move |args: CommandArgs| {
        catch_nvim_panic(move || {
            if let Some(handle) = args.args.as_deref().and_then(|s| s.trim().parse().ok()) {
                crate::utils::invalidate_buf_classification(handle);
            }
            Ok(())
        })
    });

    api::create_user_command(
        "TimeTrackingInvalidateBufCache",
        invalidate_buf_cache,
        &CreateCommandOpts::builder()
            .desc("(internal) Drop the cached tracking-file classification for one buffer")
            .nargs(CommandNArgs::ZeroOrOne)
            .build(),
    )?;

    // Name, description, handler. The description is what `:command
    // TimeTracking<Tab>` and which-key/telescope pickers show; without it
    // these all render as a blank column and are indistinguishable.
    for (name, desc, func) in [
        (
            "TimeTrackingToggle",
            "Toggle the time-tracking preview split",
            toggle_preview,
        ),
        (
            "TimeTrackingWeeklyToggle",
            "Toggle the weekly time-tracking summary in the preview split",
            toggle_weekly_preview,
        ),
        (
            "TimeTrackingOpenToday",
            "Open (creating if needed) today's tracking file",
            open_today,
        ),
        (
            "TimeTrackingUpdate",
            "Re-render the time-tracking preview now",
            update_preview,
        ),
        (
            "TimeTrackingUpdateThrottled",
            "(internal) Re-render the preview, at most once per throttle window",
            update_preview_throttled_cmd,
        ),
        (
            "TimeTrackingThrottleFire",
            "(internal) Run the render the throttle booked",
            throttle_fire_cmd,
        ),
        (
            "TimeTrackingAutoOpen",
            "Open the preview if the current buffer is a tracking file",
            auto_open,
        ),
        (
            // Marked "(internal)" like `TimeTrackingMaybeCloseIfInvisible`:
            // this is the target of the `QuitPre` autocommand (see
            // `register_autocommands`) rather than something to type, and its
            // description was otherwise indistinguishable from
            // `TimeTrackingClose`'s in the `:TimeTracking<Tab>` completion
            // list. It differs from `TimeTrackingClose` in exactly one way,
            // and that is the point of it: it closes without marking the
            // preview dismissed. Renaming it is bughunt B57's job, not this
            // one's.
            "TimeTrackingAutoClose",
            "(internal) Close the time-tracking preview",
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

    Ok(())
}

/// Register the `TimeTrackingNvim` autocommand group.
///
/// Issued as Vimscript to avoid an nvim-oxi keyset mask mismatch on 0.12.2+.
fn register_autocommands() -> Result<()> {
    api::command("augroup TimeTrackingNvim")?;
    api::command("autocmd!")?;
    api::command("autocmd BufEnter,TabEnter * TimeTrackingMaybeCloseIfInvisible")?;
    // `<abuf>` is not textually substituted into a Lua/Rust-callback user
    // command's arguments the way it is for a legacy Ex-command body (e.g.
    // `bwipeout! <abuf>`) — a Lua-backed command like this one receives the
    // literal string `"<abuf>"`. Route it through `expand()` instead, which
    // does perform the substitution regardless of callback vs. Ex-command.
    api::command(
        "autocmd BufFilePost,BufDelete,BufWipeout * execute 'TimeTrackingInvalidateBufCache ' . expand('<abuf>')",
    )?;
    api::command("autocmd WinClosed * TimeTrackingMaybeCloseIfInvisible <amatch>")?;
    api::command("autocmd TextChanged,TextChangedI *.md TimeTrackingUpdateThrottled")?;
    api::command("autocmd VimEnter,BufWinEnter *.md TimeTrackingAutoOpen")?;
    api::command("autocmd BufReadPost,FileChangedShellPost *.md TimeTrackingUpdateThrottled")?;
    api::command("autocmd FocusGained,BufEnter *.md checktime")?;
    // NOT interpolating PREVIEW_BUF_NAME here on purpose: `:bwipeout` splits its
    // argument on whitespace and matches each piece as a regexp, so this never
    // matches the preview buffer and errors under `silent!` (bughunt B54).
    // Substituting the constant would make the line read as correct while
    // staying inert. Fix it properly with B54 instead.
    api::command("autocmd VimLeavePre * silent! bwipeout [Time Tracking Preview]")?;
    // Deliberately `TimeTrackingAutoClose`, not `TimeTrackingClose`.
    //
    // `:TimeTrackingClose`'s handler marks the preview *dismissed* (see
    // `register_commands`), which is right for a user typing the command and
    // catastrophic here: `QuitPre` fires for every `:q` anywhere in the
    // session, including quitting an unrelated split, so routing it through
    // the dismissing path would leave `PREVIEW_DISMISSED` latched and stop the
    // preview auto-opening for *any* tracking file for the rest of the
    // session. `TimeTrackingAutoClose` closes and nothing else, which is
    // exactly what this autocommand did before dismissal existed — bughunt
    // B19 (this closes the preview on any `:q`, not just the one showing it)
    // is left exactly as it was, out of scope, rather than compounded.
    api::command("autocmd QuitPre * TimeTrackingAutoClose")?;
    api::command("augroup END")?;
    Ok(())
}
