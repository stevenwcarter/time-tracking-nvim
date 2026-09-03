use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, Once},
};

use nvim_oxi::{
    Result,
    api::{self, Buffer, Window},
};
use time_tracking_cli::Config;

use crate::log_error;

/// Guards the data-directory warning so the per-keystroke `TextChanged` path
/// cannot spam `:messages` with the same line on every keypress.
static DATA_DIR_WARNED: Once = Once::new();

/// Memoized resolution of the configured data directory.
///
/// `Config` is loaded once at plugin init and never mutated (see `lib.rs`), so
/// in production this resolves exactly once instead of paying a `realpath(2)`
/// on every keystroke. Keyed on the raw configured string so that tests — which
/// build several `Config`s in one process — still get the right answer.
static DATA_DIR_MEMO: Mutex<Option<(String, Option<PathBuf>)>> = Mutex::new(None);

/// Resolves and caches the canonicalized data directory for `config`.
///
/// Returns `None` when the configured directory does not exist or cannot be
/// canonicalized, warning once via [`DATA_DIR_WARNED`] — unless Neovim is
/// already quitting, in which case the warning is skipped rather than risking
/// a crash (see the comment on the `Err` arm below).
///
/// A miss is deliberately **not** cached. Before memoization the plugin
/// re-resolved on every call, so a directory that started missing and later
/// appeared (mounted, created, typo fixed) started working on the very next
/// keystroke — and the warning this function prints says "until this is
/// fixed", promising exactly that recovery. Caching a miss would answer
/// `None` forever once cached, contradicting the message and requiring a
/// restart to recover; only a successful resolution is stable enough to
/// memoize; a failure is retried every call, same as before B15.
fn resolved_data_dir(config: &Config) -> Option<PathBuf> {
    let configured = config.get_data_directory().unwrap_or("").to_owned();

    let mut memo = match DATA_DIR_MEMO.lock() {
        Ok(memo) => memo,
        // A poisoned lock must not disable file detection; fall back to an
        // uncached resolve.
        Err(poisoned) => poisoned.into_inner(),
    };

    if let Some((key, value)) = memo.as_ref()
        && key == &configured
    {
        return value.clone();
    }

    match fs::canonicalize(&configured) {
        Ok(dir) => {
            *memo = Some((configured, Some(dir.clone())));
            Some(dir)
        }
        Err(e) => {
            // Leave `memo` untouched: a miss is not cached (see doc comment
            // above), and must not evict a previously cached successful
            // resolution for a different key either.
            //
            // `v:exiting` is non-nil once Neovim has begun quitting. Two
            // pre-existing integration tests (test_time_tracking_with_config_
            // creates_{commands,autocommands}) drop their Config's backing
            // TempDir when the Rust test function returns, and separately
            // register a `schedule()`-deferred TimeTrackingAutoOpen callback
            // (see lib.rs) at startup that is still pending at that point.
            // That callback then runs during the harness's `:qall!`-driven
            // shutdown — confirmed empirically via `v:exiting` — observing a
            // directory the *test itself* just deleted, not a real
            // misconfiguration. Calling the nvim API at that point crashes
            // the process, so skip the warning outright: nvim is about to
            // exit anyway, so nothing would show it to a user.
            //
            // `DATA_DIR_WARNED.call_once` is called from right here, at the
            // point the message is actually written, not from an outer gate
            // that decides whether to attempt it — so a call suppressed by
            // `is_exiting` above never marks the warning "done", and a later
            // call (this process is not, in fact, exiting) still gets to try.
            let is_exiting = api::get_vvar::<Option<i64>>("exiting")
                .ok()
                .flatten()
                .is_some();
            if !is_exiting {
                DATA_DIR_WARNED.call_once(|| {
                    log_error!(
                        "[time-tracking-nvim] could not resolve data directory {:?}: {}. \
                         The preview will not open for any file until this is fixed.",
                        configured,
                        e
                    );
                });
            }
            None
        }
    }
}

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

    let Some(data_dir) = resolved_data_dir(config) else {
        return Ok(false);
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

    // Build the joined string directly: the previous
    // `.map(to_string).collect::<Vec<_>>().join()` allocated one String per
    // line plus a Vec, then threw them all away.
    let mut content = String::new();
    for (i, line) in lines.enumerate() {
        if i > 0 {
            content.push('\n');
        }
        content.push_str(&line.to_string());
    }
    Ok(content)
}

/// Is any window showing a time-tracking file?
///
/// `exclude_win` skips one window by handle. `WinClosed` fires *before* the
/// window leaves the layout, so the handler must not let the window being
/// closed vote for keeping the preview open.
pub fn any_tracking_visible(config: &Config, exclude_win: Option<i32>) -> Result<bool> {
    for win in api::list_wins() {
        if Some(win.handle()) == exclude_win {
            continue;
        }

        let buf = win.get_buf()?;
        let name = buf.get_name()?;

        // Skip the preview itself
        if name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            continue;
        }

        if is_win_time_tracking_file(win, config)? {
            return Ok(true);
        }
    }
    Ok(false)
}
