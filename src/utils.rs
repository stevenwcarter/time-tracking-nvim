//! Buffer and window helpers shared by the command and autocommand entry
//! points.
//!
//! Mostly predicates for "is this a tracking file?" — a `.md` file under the
//! configured data directory — plus the memoized directory resolution they all
//! share and the buffer read the renderers use.

use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, Once},
};

use nvim_oxi::{
    Dictionary, Result,
    api::{self, Buffer, Window},
};
use time_tracking_cli::Config;

use crate::log_error;

thread_local! {
    /// Per-buffer memoization of `is_buf_time_tracking_file`'s result.
    ///
    /// Keyed on `(buffer handle, configured data directory)` rather than the
    /// handle alone: production runs against a single `'static Config` for
    /// the plugin's whole lifetime (see `DATA_DIR_MEMO`), but the integration
    /// tests build several `Config`s in one process and, in
    /// `test_data_dir_memo_does_not_leak_between_configs`, deliberately
    /// re-check the *same* buffer against two of them — keying on the handle
    /// alone would let the first config's answer leak into the second's.
    /// Invalidated by `invalidate_buf_classification`, wired to
    /// `BufFilePost`/`BufDelete`/`BufWipeout` in `lib.rs` — a buffer's
    /// classification depends only on its name/extension and the configured
    /// directory, and only the former two change via those events.
    static BUF_CLASSIFICATION: RefCell<HashMap<(i32, String), bool>> =
        RefCell::new(HashMap::new());
}

/// Drop the cached classification for one buffer, under any configured
/// directory it was cached against.
///
/// Called from the `TimeTrackingInvalidateBufCache` command, itself wired to
/// `BufFilePost`/`BufDelete`/`BufWipeout` in `lib.rs`.
pub fn invalidate_buf_classification(handle: i32) {
    BUF_CLASSIFICATION.with(|cache| {
        cache.borrow_mut().retain(|(h, _), _| *h != handle);
    });
}

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
    let configured = config.get_data_directory().unwrap_or("");

    // Scope the guard: everything below this block does a syscall or calls into
    // Neovim, and holding a process-wide mutex across either is what
    // clippy::significant_drop_tightening is warning about.
    let cached = {
        let memo = match DATA_DIR_MEMO.lock() {
            Ok(memo) => memo,
            // A poisoned lock must not disable file detection; fall back to an
            // uncached resolve.
            Err(poisoned) => poisoned.into_inner(),
        };
        memo.as_ref()
            .filter(|(key, _)| key.as_str() == configured)
            .map(|(_, value)| value.clone())
    };

    if let Some(value) = cached {
        return value;
    }

    match fs::canonicalize(configured) {
        Ok(dir) => {
            let mut memo = match DATA_DIR_MEMO.lock() {
                Ok(memo) => memo,
                Err(poisoned) => poisoned.into_inner(),
            };
            *memo = Some((configured.to_owned(), Some(dir.clone())));
            drop(memo);
            Some(dir)
        }
        Err(e) => {
            // Leave `memo` untouched: a miss is not cached (see doc comment
            // above), and must not evict a previously cached successful
            // resolution for a different key either.
            warn_data_dir_unresolved(configured, &e);
            None
        }
    }
}

/// Warn once that the configured data directory could not be resolved.
///
/// `v:exiting` is non-nil once Neovim has begun quitting. Two
/// pre-existing integration tests (test_time_tracking_with_config_
/// creates_{commands,autocommands}) drop their Config's backing
/// TempDir when the Rust test function returns, and separately
/// register a `schedule()`-deferred TimeTrackingAutoOpen callback
/// (see lib.rs) at startup that is still pending at that point.
/// That callback then runs during the harness's `:qall!`-driven
/// shutdown — confirmed empirically via `v:exiting` — observing a
/// directory the *test itself* just deleted, not a real
/// misconfiguration. Calling the nvim API at that point crashes
/// the process, so skip the warning outright: nvim is about to
/// exit anyway, so nothing would show it to a user.
///
/// `DATA_DIR_WARNED.call_once` is called from right here, at the
/// point the message is actually written, not from an outer gate
/// that decides whether to attempt it — so a call suppressed by
/// `is_exiting` above never marks the warning "done", and a later
/// call (this process is not, in fact, exiting) still gets to try.
fn warn_data_dir_unresolved(configured: &str, e: &std::io::Error) {
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
}

/// Check if the current buffer is a time tracking file (markdown file in data directory)
pub fn is_time_tracking_file(config: &Config) -> Result<bool> {
    let current_buffer = api::get_current_buf();

    is_buf_time_tracking_file(&current_buffer, config)
}

/// Check if the provided window's buffer is a time tracking file (markdown file in data directory)
pub fn is_win_time_tracking_file(win: &Window, config: &Config) -> Result<bool> {
    is_buf_time_tracking_file(&win.get_buf()?, config)
}

/// Checks if the provided buffer is a time tracking file (markdown file in data directory)
pub fn is_buf_time_tracking_file(current_buffer: &Buffer, config: &Config) -> Result<bool> {
    let handle = current_buffer.handle();
    let key = (handle, config.get_data_directory().unwrap_or("").to_owned());

    if let Some(cached) = BUF_CLASSIFICATION.with(|cache| cache.borrow().get(&key).copied()) {
        return Ok(cached);
    }

    let result = is_buf_time_tracking_file_uncached(current_buffer, config)?;

    // A `false` caused by a currently-unresolvable data directory must not be
    // cached: `resolved_data_dir` deliberately doesn't cache that miss either
    // (see its doc comment) so the plugin recovers on the very next call once
    // the directory is created/mounted, without a restart. A `true` result
    // implies the directory did resolve, so this only ever skips caching a
    // negative.
    if result || resolved_data_dir(config).is_some() {
        BUF_CLASSIFICATION.with(|cache| {
            cache.borrow_mut().insert(key, result);
        });
    }

    Ok(result)
}

/// Checks if the provided buffer is a time tracking file (markdown file in data directory)
fn is_buf_time_tracking_file_uncached(current_buffer: &Buffer, config: &Config) -> Result<bool> {
    let buffer_name = current_buffer.get_name()?;
    let Ok(buffer_name_str) = buffer_name.to_str() else {
        return Ok(false);
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
        content.push_str(&line.to_string_lossy());
    }
    Ok(content)
}

/// Parsed totals for `buffer_content`, for statusline-style integrations
/// that want a value back rather than a rendered preview.
///
/// Always reports `is_tracking_file: true` — callers that already know the
/// buffer is a tracking file (this plugin's own `status` command) check
/// `is_time_tracking_file` first and only call this when it's already
/// `true`; a caller unsure of that should check first rather than rely on
/// this function to judge it.
pub fn buffer_status(buffer_content: &str, config: &Config) -> Dictionary {
    let data = time_tracking_parser::parse_time_tracking_data(
        buffer_content,
        config.get_prefix(),
        config.get_suffix(),
    );

    Dictionary::from_iter([
        ("is_tracking_file", nvim_oxi::Object::from(true)),
        (
            "total_minutes",
            nvim_oxi::Object::from(data.total_minutes as i64),
        ),
        (
            "dead_time_minutes",
            nvim_oxi::Object::from(data.dead_time_minutes as i64),
        ),
        (
            "warning_count",
            nvim_oxi::Object::from(data.warnings.len() as i64),
        ),
    ])
}

/// Name given to the preview scratch buffer.
///
/// Neovim reports buffer names as absolute paths, so every consumer matches on
/// the *suffix*, never equality.
pub const PREVIEW_BUF_NAME: &str = "[Time Tracking Preview]";

/// Is `buf` the preview buffer?
pub fn is_preview_buf(buf: &Buffer) -> Result<bool> {
    Ok(buf
        .get_name()?
        .to_str()
        .is_ok_and(|s| s.ends_with(PREVIEW_BUF_NAME)))
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

        // Skip the preview itself
        if is_preview_buf(&buf)? {
            continue;
        }

        if is_win_time_tracking_file(&win, config)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nvim_oxi::Object;

    #[test]
    fn buffer_status_parses_totals_from_content() {
        let config = Config {
            data_directory: Some("/tmp/does-not-matter-for-this-test".to_string()),
            ..Default::default()
        };
        let dict = buffer_status("9-10 work\n10-10:30 admin\n", &config);

        assert_eq!(dict.get("total_minutes"), Some(&Object::from(90i64)));
        assert_eq!(dict.get("is_tracking_file"), Some(&Object::from(true)));
        assert_eq!(dict.get("dead_time_minutes"), Some(&Object::from(0i64)));
        assert_eq!(dict.get("warning_count"), Some(&Object::from(0i64)));
    }

    #[test]
    fn buffer_status_reports_zero_totals_for_content_with_no_time_entries() {
        let config = Config {
            data_directory: Some("/tmp/does-not-matter-for-this-test".to_string()),
            ..Default::default()
        };

        let dict = buffer_status("", &config);
        assert_eq!(dict.get("total_minutes"), Some(&Object::from(0i64)));

        let dict = buffer_status("# just a heading, no time entries\n", &config);
        assert_eq!(dict.get("total_minutes"), Some(&Object::from(0i64)));
    }
}
