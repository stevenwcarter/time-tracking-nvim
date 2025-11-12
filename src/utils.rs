use std::{fs, path::Path};

use nvim_oxi::{
    Result,
    api::{self, Buffer, Error, Window},
};
use time_tracking_cli::Config;

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

    if buffer_name.as_os_str().is_empty() {
        return Ok(false);
    }

    let buffer_path = Path::new(&buffer_name);
    let buffer_path = fs::canonicalize(buffer_path)
        .map_err(|e| {
            Error::Other(format!(
                "Could not convert {} to a path: {}",
                buffer_name.display(),
                e
            ))
        })
        .ok();

    if buffer_path.is_none() {
        return Ok(false);
    }

    // TODO: Need to canonicalize in case the data directory is a symlink, should be done upstream
    // probably
    let data_dir = fs::canonicalize(config.get_data_directory().unwrap_or(""))
        .map_err(|_| Error::Other("could not find path for data directory".to_owned()))
        .ok();

    if buffer_path.is_none() || data_dir.is_none() {
        return Ok(false);
    }

    let buffer_path = buffer_path.unwrap();
    let data_dir = data_dir.unwrap();

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
        if name.ends_with("[Time Tracking Preview]") {
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
