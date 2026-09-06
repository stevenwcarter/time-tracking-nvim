//! Neovim plugin (loaded as a cdylib) that renders a live time-tracking day
//! summary for the buffer being edited into a side split.
//!
//! `time_tracking_nvim` is the entry point Neovim calls when the native module
//! is `require`d. It never returns `Err`: an initialization failure is reported
//! through the `error` key of the dictionary it returns, because throwing out of
//! the plugin entry point aborts Neovim on macOS (see the comment there).

use std::io::Write;
use std::panic::{self, AssertUnwindSafe};

use nvim_oxi::api::types::{CommandArgs, CommandNArgs};
use nvim_oxi::schedule;
use nvim_oxi::{
    Dictionary, Function, Result,
    api::{self, opts::CreateCommandOpts},
};
use time_tracking_cli::Config;

use crate::utils::any_tracking_visible;

mod async_rt;
mod preview;
pub mod utils;

pub use preview::{
    auto_close_preview, auto_open_preview, close_preview, create_or_update_preview, throttle_fire,
    toggle_preview_fn, update_preview_fn, update_preview_throttled,
};
// Test seams, not interface: see `preview::write_preview_contents_with` and
// `preview::reset_throttle_for_test`.
#[doc(hidden)]
pub use preview::{reset_throttle_for_test, write_preview_contents_with};

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

fn catch_nvim_panic<F>(f: F) -> Result<()>
where
    F: FnOnce() -> Result<()>,
{
    panic::catch_unwind(AssertUnwindSafe(f))
        .map_err(|payload| {
            let msg = panic_message(payload);
            api::err_writeln(&format!("[time-tracking-nvim] panic: {}", msg));
            nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(msg))
        })
        .flatten()
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

    let api = Dictionary::new();
    Ok(api)
}

/// Register the `TimeTracking*` user commands.
fn register_commands(config: &'static Config) -> Result<()> {
    let toggle_preview =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| toggle_preview_fn(config)));

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

    let auto_open =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| auto_open_preview(config)));

    let auto_close =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| auto_close_preview(config)));

    let close_preview_cmd =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(close_preview));

    let maybe_close_if_invisible = Function::from_fn(move |args: CommandArgs| {
        catch_nvim_panic(move || {
            // WinClosed sets <amatch> to the window-ID of the window that is
            // about to be removed. BufEnter/TabEnter set it to a buffer name,
            // so those fire the command with no argument.
            let exclude = args.args.as_deref().and_then(|s| s.trim().parse().ok());

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
            .desc("(internal) Close the preview when no tracking file is visible")
            .nargs(CommandNArgs::ZeroOrOne)
            .build(),
    )?;

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
            // this command is wired to no autocommand (bughunt B57) and its
            // description was otherwise indistinguishable from
            // `TimeTrackingClose`'s in the `:TimeTracking<Tab>` completion
            // list. Renaming or removing it is B57's job, not this one's.
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
    api::command("autocmd WinClosed * TimeTrackingMaybeCloseIfInvisible <amatch>")?;
    api::command("autocmd TextChanged,TextChangedI *.md TimeTrackingUpdateThrottled")?;
    api::command("autocmd VimEnter,BufWinEnter *.md TimeTrackingAutoOpen")?;
    // NOT interpolating PREVIEW_BUF_NAME here on purpose: `:bwipeout` splits its
    // argument on whitespace and matches each piece as a regexp, so this never
    // matches the preview buffer and errors under `silent!` (bughunt B54).
    // Substituting the constant would make the line read as correct while
    // staying inert. Fix it properly with B54 instead.
    api::command("autocmd VimLeavePre * silent! bwipeout [Time Tracking Preview]")?;
    api::command("autocmd QuitPre * TimeTrackingClose")?;
    api::command("augroup END")?;
    Ok(())
}
