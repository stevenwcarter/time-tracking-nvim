use std::panic::{self, AssertUnwindSafe};

use nvim_oxi::api::opts::OptionOptsBuilder;
use nvim_oxi::api::types::{CommandArgs, CommandNArgs};
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::schedule;
use nvim_oxi::{
    Dictionary, Function, Result,
    api::{self, opts::CreateCommandOpts},
};
use time_tracking_cli::Config;

use crate::utils::{any_tracking_visible, get_buffer_content, is_time_tracking_file};

mod preview;
pub mod utils;

use preview::*;
pub use preview::{
    auto_open_preview, close_preview, create_or_update_preview, toggle_preview_fn,
    update_preview_fn,
};

#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        #[allow(unused_imports)]
        use nvim_oxi::api::types::LogLevel;
        // let _ = nvim_oxi::api::notify(&format!($($arg)*), LogLevel::Info, &Default::default());
    };
}

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        nvim_oxi::api::err_writeln(&format!($($arg)*));
    };
}

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
        use std::io::Write;
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
                use std::io::Write;
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

/// inner function which accepts `config` for testing
pub fn time_tracking_with_config(config: &'static Config) -> Result<Dictionary> {
    // Create command to toggle preview
    let toggle_preview =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| toggle_preview_fn(config)));

    // Create command to update preview (for auto-updating)
    let update_preview =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| update_preview_fn(config)));

    // Create command to auto-open preview
    let auto_open =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| auto_open_preview(config)));

    // Create command to auto-close preview
    let auto_close =
        Function::from_fn(move |_: CommandArgs| catch_nvim_panic(|| auto_close_preview(config)));

    // Create command to manually close preview window
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
            .nargs(CommandNArgs::ZeroOrOne)
            .build(),
    )?;

    // Register commands
    api::create_user_command(
        "TimeTrackingToggle",
        toggle_preview,
        &CreateCommandOpts::builder().build(),
    )?;

    api::create_user_command(
        "TimeTrackingUpdate",
        update_preview,
        &CreateCommandOpts::builder().build(),
    )?;

    api::create_user_command(
        "TimeTrackingAutoOpen",
        auto_open,
        &CreateCommandOpts::builder().build(),
    )?;

    api::create_user_command(
        "TimeTrackingAutoClose",
        auto_close,
        &CreateCommandOpts::builder().build(),
    )?;

    api::create_user_command(
        "TimeTrackingClose",
        close_preview_cmd,
        &CreateCommandOpts::builder().build(),
    )?;

    // Register autocommands via Vimscript to avoid nvim-oxi keyset mask mismatch on 0.12.2+
    api::command("augroup TimeTrackingNvim")?;
    api::command("autocmd!")?;
    api::command("autocmd BufEnter,TabEnter * TimeTrackingMaybeCloseIfInvisible")?;
    api::command("autocmd WinClosed * TimeTrackingMaybeCloseIfInvisible <amatch>")?;
    api::command("autocmd TextChanged,TextChangedI *.md TimeTrackingUpdate")?;
    api::command("autocmd VimEnter,BufWinEnter *.md TimeTrackingAutoOpen")?;
    api::command("autocmd VimLeavePre * silent! bwipeout [Time Tracking Preview]")?;
    api::command("autocmd QuitPre * TimeTrackingClose")?;
    api::command("augroup END")?;

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
