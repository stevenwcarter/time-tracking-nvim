use std::panic::{self, AssertUnwindSafe};

use nvim_oxi::api::opts::OptionOptsBuilder;
use nvim_oxi::api::types::CommandArgs;
use nvim_oxi::api::{Buffer, Window};
use nvim_oxi::schedule;
use nvim_oxi::{
    Dictionary, Function, Result,
    api::{
        self,
        opts::{CreateAutocmdOpts, CreateCommandOpts},
    },
};
use time_tracking_cli::Config;

use crate::utils::{any_tracking_visible, get_buffer_content, is_time_tracking_file};

mod preview;
pub mod utils;

use preview::*;

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
    panic::catch_unwind(AssertUnwindSafe(|| {
        let config = Config::try_get_no_args()
            .map_err(|e| nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(e.to_string())))?;
        time_tracking_with_config(config)
    }))
    .map_err(|payload| nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(panic_message(payload))))
    .flatten()
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

    let maybe_close_if_invisible = Function::from_fn(move |_: CommandArgs| {
        catch_nvim_panic(|| {
            if !any_tracking_visible(config)? {
                close_preview()?;
            }
            Ok(())
        })
    });

    api::create_user_command(
        "TimeTrackingMaybeCloseIfInvisible",
        maybe_close_if_invisible,
        &CreateCommandOpts::builder().build(),
    )?;

    // Fire when views/layouts tend to change
    api::create_autocmd(
        vec!["BufEnter", "WinClosed", "TabEnter"],
        &CreateAutocmdOpts::builder()
            .command("TimeTrackingMaybeCloseIfInvisible")
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

    // Set up autocommands for live updates on markdown files
    api::create_autocmd(
        vec!["TextChanged", "TextChangedI"],
        &CreateAutocmdOpts::builder()
            .command("TimeTrackingUpdate")
            .build(),
    )?;

    // Set up autocommand to auto-open preview after Neovim fully starts
    api::create_autocmd(
        vec!["VimEnter", "BufWinEnter"],
        &CreateAutocmdOpts::builder()
            .patterns(vec!["*.md"])
            .command("TimeTrackingAutoOpen")
            .build(),
    )?;

    // Set up autocommand to close preview window when quitting Neovim
    api::create_autocmd(
        vec!["VimLeavePre"],
        &CreateAutocmdOpts::builder()
            .command("silent! bwipeout [Time Tracking Preview]")
            .build(),
    )?;

    api::create_autocmd(
        vec!["QuitPre"],
        &CreateAutocmdOpts::builder()
            .command("TimeTrackingClose")
            .build(),
    )?;

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
