use std::{fs, path::Path, sync::Once};

use nvim_oxi::{
    Result,
    api::{self, Buffer, Window},
};
use time_tracking_cli::Config;

use crate::log_error;

/// Guards the data-directory warning so the per-keystroke `TextChanged` path
/// cannot spam `:messages` with the same line on every keypress.
static DATA_DIR_WARNED: Once = Once::new();

/// Check if the current buffer is a time tracking file (markdown file in data directory)
pub fn is_time_tracking_file(config: &Config) -> Result<bool> {
    let current_buffer = api::get_current_buf();

    is_buf_time_tracking_file(current_buffer, config)
}

/// Check if the provided window's buffer is a time tracking file (markdown file in data directory)
pub fn is_win_time_tracking_file(win: Window, config: &Config) -> Result<bool> {
    is_buf_time_tracking_file(win.get_buf()?, config)
}

/// Checks if the provided buffer is a time tracking file (markdown file in data directory)
pub fn is_buf_time_tracking_file(current_buffer: Buffer, config: &Config) -> Result<bool> {
    let buffer_name = current_buffer.get_name()?;
    let buffer_name_str = match buffer_name.to_str() {
        Ok(s) => s,
        Err(_) => return Ok(false),
    };

    if buffer_name_str.is_empty() {
        return Ok(false);
    }

    let buffer_path = Path::new(buffer_name_str);

    // The file may not exist yet — opening today's not-yet-written daily note
    // is the primary workflow — so resolve the parent directory instead and
    // rejoin the file name. Falls back to the raw path when the parent does
    // not resolve either.
    let buffer_path = match (buffer_path.parent(), buffer_path.file_name()) {
        (Some(parent), Some(file_name)) => fs::canonicalize(parent)
            .map(|dir| dir.join(file_name))
            .unwrap_or_else(|_| buffer_path.to_path_buf()),
        _ => buffer_path.to_path_buf(),
    };

    // TODO: Need to canonicalize in case the data directory is a symlink, should be done upstream
    // probably
    let data_dir = match fs::canonicalize(config.get_data_directory().unwrap_or("")) {
        Ok(dir) => dir,
        Err(e) => {
            DATA_DIR_WARNED.call_once(|| {
                let configured = config.get_data_directory().unwrap_or("<unset>").to_owned();
                let error = e.to_string();
                // Deferred via `schedule`: this branch can run from the
                // startup auto-open callback, which fires on a nvim main-loop
                // tick where calling the API synchronously is unsafe.
                nvim_oxi::schedule(move |_| {
                    log_error!(
                        "[time-tracking-nvim] could not resolve data directory {:?}: {}. \
                         The preview will not open for any file until this is fixed.",
                        configured,
                        error
                    );
                });
            });
            return Ok(false);
        }
    };

    // Check if file is in data directory and has .md extension
    let is_time_tracking_file = buffer_path.starts_with(data_dir)
        && matches!(buffer_path.extension(), Some(ext) if ext == "md");

    Ok(is_time_tracking_file)
}

/// Get the content of the current buffer
pub fn get_buffer_content() -> Result<String> {
    let current_buffer = api::get_current_buf();
    let line_count = current_buffer.line_count()?;
    let lines = current_buffer.get_lines(0..line_count, false)?;
    Ok(lines
        .into_iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>()
        .join("\n"))
}

pub fn any_tracking_visible(config: &Config) -> Result<bool> {
    for win in api::list_wins() {
        let buf = win.get_buf()?;
        let name = buf.get_name()?;

        // Skip the preview itself
        if name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            continue;
        }

        // Decide if THIS buffer is a time-tracking one.
        // If your existing utils::is_time_tracking_file(config) only checks
        // the *current* buffer, add a sibling helper that inspects `name`.
        if is_win_time_tracking_file(win, config)? {
            return Ok(true);
        }
    }
    Ok(false)
}
