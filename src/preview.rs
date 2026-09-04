use super::*;

use std::cell::RefCell;
#[cfg(not(windows))]
use std::time::Duration;

#[cfg(not(windows))]
use nvim_oxi::libuv::TimerHandle;

thread_local! {
    /// Cached handle to the preview buffer.
    ///
    /// The preview is created with `bufhidden=wipe`, so this handle can become
    /// invalid at any time — every read revalidates with `is_valid()` and falls
    /// back to a full scan. Without it, refreshing the preview cost one FFI
    /// round-trip per open buffer, on every keystroke.
    static PREVIEW_BUF: RefCell<Option<Buffer>> = const { RefCell::new(None) };
}

thread_local! {
    /// The last output successfully written to the preview buffer.
    ///
    /// Cleared whenever the preview buffer is created or destroyed, so a
    /// wiped-and-recreated preview always gets a full write.
    ///
    /// Invariant this cache depends on: it tracks what was last *written*,
    /// not what the buffer currently *contains*, so it is only correct as
    /// long as [`write_preview_contents_with`] is the sole writer to the
    /// preview buffer's contents. If another write path is ever added, this
    /// cache goes stale silently and the dirty-check there will skip writes
    /// the buffer actually needs.
    static LAST_OUTPUT: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_last_output(output: Option<String>) {
    LAST_OUTPUT.with(|cell| *cell.borrow_mut() = output);
}

fn last_output_matches(output: &str) -> bool {
    LAST_OUTPUT.with(|cell| cell.borrow().as_deref() == Some(output))
}

fn cached_preview_buf() -> Option<Buffer> {
    PREVIEW_BUF.with(|cell| {
        let mut slot = cell.borrow_mut();
        match slot.as_ref() {
            Some(buf) if buf.is_valid() => Some(buf.clone()),
            Some(_) => {
                *slot = None;
                None
            }
            None => None,
        }
    })
}

fn set_cached_preview_buf(buf: Option<Buffer>) {
    PREVIEW_BUF.with(|cell| *cell.borrow_mut() = buf);
}

/// The window in the *current tabpage* showing `buf`, if any.
///
/// Deliberately not `api::list_wins()`: that enumerates every tabpage, so a
/// preview open in tab 1 would count as "already open" for tab 2 and the second
/// tab would never get its own preview split.
fn preview_win_in_current_tab(buf: &Buffer) -> Result<Option<Window>> {
    for w in api::get_current_tabpage().list_wins()? {
        if &w.get_buf()? == buf {
            return Ok(Some(w));
        }
    }
    Ok(None)
}

/// Resolve the preview buffer and the window showing it, in one pass.
///
/// Returns `None` when no preview buffer exists; `Some((buf, None))` when the
/// buffer exists but is not displayed. Consolidates the six copies of this
/// lookup and gives the handle cache a single invalidation point.
fn find_preview() -> Result<Option<(Buffer, Option<Window>)>> {
    let buf = match cached_preview_buf() {
        Some(buf) => Some(buf),
        None => {
            let mut found = None;
            for b in api::list_bufs() {
                if b.get_name()?
                    .to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                {
                    found = Some(b);
                    break;
                }
            }
            if let Some(ref b) = found {
                set_cached_preview_buf(Some(b.clone()));
            }
            found
        }
    };

    let Some(buf) = buf else {
        return Ok(None);
    };

    let window = preview_win_in_current_tab(&buf)?;

    Ok(Some((buf, window)))
}

/// Trailing-edge debounce interval for autocommand-driven updates.
#[cfg(not(windows))]
const DEBOUNCE: Duration = Duration::from_millis(150);

/// Below this width a vertical split fails outright with E36 and damages the
/// layout on the way out, so no preview is the better outcome.
const MIN_SPLIT_COLUMNS: u32 = 40;

/// The preview aims for this fraction of the total screen width.
const PREVIEW_SCREEN_FRACTION: i64 = 3;

/// Floor for the preview, and the minimum width left to the window it split from.
const MIN_PREVIEW_COLUMNS: u32 = 20;

#[cfg(not(windows))]
thread_local! {
    /// In-flight debounce timer, if any.
    ///
    /// Re-armed on each keystroke, so a burst of typing costs one render at
    /// the end of the burst rather than one per character.
    ///
    /// **Every re-arm leaks ~200 bytes.** nvim-oxi's libuv binding allocates
    /// the `uv_timer_t` with `alloc::alloc` and boxes the callback with
    /// `Box::into_raw`, but `libuv::Handle` has no `Drop` impl, so dropping a
    /// `TimerHandle` frees nothing. There is no local fix: `TimerHandle`
    /// exposes `start`/`once` only as associated constructors that allocate a
    /// fresh handle, with no `&mut self` way to re-arm an existing one. The
    /// real fix is `impl Drop for Handle` upstream.
    ///
    /// If that lands, move the `= None` below **out** of the timer callback:
    /// clearing the cell there would drop — and then free — the `uv_timer_t`
    /// while `TimerHandle::once`'s own wrapper still holds the same pointer and
    /// is about to call `timer.stop()` on it (`crates/libuv/src/timer.rs:78-82`),
    /// which is a use-after-free. Clear it after the callback returns instead.
    ///
    /// What bounds the leak is the tracking-file guard at the top of
    /// [`update_preview_debounced`]: the autocommand fires for *every* `*.md`
    /// buffer, so without that guard editing a README would leak on every
    /// keystroke too. Keep the guard.
    static PENDING_UPDATE: RefCell<Option<TimerHandle>> = const { RefCell::new(None) };
}

/// Autocommand entry point: coalesce a burst of keystrokes into one render.
///
/// `TextChanged`/`TextChangedI` fire once per keystroke on Neovim's single UI
/// thread, and each render pays canonicalize syscalls, a window scan, a
/// full-buffer read, and a re-parse. Arming a one-shot timer instead keeps the
/// per-keystroke cost to cancelling and re-arming it.
///
/// `:TimeTrackingUpdate` deliberately still calls [`update_preview_fn`]
/// directly: a user who types the command expects to see the result, not to
/// wait out the debounce window.
#[cfg(not(windows))]
pub fn update_preview_debounced(config: &'static Config) -> Result<()> {
    // Arm nothing for a buffer that can never render a preview. The
    // autocommand fires for every `*.md` buffer, not just tracking notes, and
    // every armed timer is an allocation the libuv binding never frees (see
    // `PENDING_UPDATE`). `update_preview_fn` makes this same check when the
    // timer fires, so skipping the arm changes no behaviour — it only avoids
    // the leak, the timer, and the `schedule` round-trip for a buffer whose
    // render would have been a no-op.
    if !is_time_tracking_file(config)? {
        return Ok(());
    }

    // Cancel the render armed by the previous keystroke, so the burst renders
    // once, at its end.
    PENDING_UPDATE.with(|cell| {
        if let Some(timer) = cell.borrow_mut().as_mut() {
            let _ = timer.stop();
        }
    });

    // The libuv callback runs in Neovim's fast event context, where the API is
    // off limits (`E5560: nvim_buf_set_lines must not be called in a fast
    // event context`), so hand the render back to the main loop.
    //
    // The render must be wrapped in `catch_nvim_panic`, as the other
    // `schedule` body in `lib.rs` is. Before the debounce, the autocommand ran
    // `TimeTrackingUpdate`, whose `Function::from_fn` caught panics for us; now
    // the command's wrapper has long returned by the time this runs. The
    // formatter parses half-typed markdown on every pause in typing, and a
    // panic escaping here would unwind out of nvim-oxi's `extern "C"` Lua
    // trampoline — aborting Neovim with unsaved buffers rather than printing a
    // message.
    let timer = TimerHandle::once(DEBOUNCE, move || {
        PENDING_UPDATE.with(|cell| *cell.borrow_mut() = None);
        schedule(move |()| {
            if let Err(e) = catch_nvim_panic(|| update_preview_fn(config)) {
                log_error!("[time-tracking-nvim] debounced update failed: {}", e);
            }
        });
    })?;

    PENDING_UPDATE.with(|cell| *cell.borrow_mut() = Some(timer));
    Ok(())
}

/// Windows counterpart to [`update_preview_debounced`]: renders immediately.
///
/// The debounce needs nvim-oxi's `libuv` feature, which cannot work on Windows.
/// Its `uv_*` externs carry no `raw-dylib` link attribute — the mechanism every
/// other nvim-oxi FFI module uses to import from the host — so an MSVC build has
/// nothing to resolve them against and fails with `LNK2019`. Annotating them
/// would only move the failure to load time: the official v0.12.5 distribution
/// exports 5710 symbols from `nvim.exe` and zero `uv_*`, and `lua51.dll` exports
/// none either, so the symbols are simply absent.
///
/// Windows therefore keeps the pre-debounce behaviour — one render per keystroke.
/// That is what every platform did before B3, so this is a missing optimisation,
/// not a regression. The tracking-file guard still applies, so non-tracking `*.md`
/// buffers cost nothing beyond the check itself.
#[cfg(windows)]
pub fn update_preview_debounced(config: &'static Config) -> Result<()> {
    update_preview_fn(config)
}

/// Is a window in the current tabpage showing the preview?
fn preview_is_open() -> Result<bool> {
    Ok(matches!(find_preview()?, Some((_, Some(_)))))
}

/// Render the current buffer's day summary into the preview.
///
/// The single read-format-write path: every entry point that shows tracking
/// data goes through here, so the formatter arguments are specified once.
fn render_current_buffer(config: &Config) -> Result<()> {
    let buffer_content = get_buffer_content()?;
    let formatted_output = config.get_formatter().day_summary(
        &buffer_content,
        "",
        config.get_prefix(),
        config.get_suffix(),
    );
    create_or_update_preview(&formatted_output)
}

/// `:TimeTrackingToggle`: closes the preview when a window is showing it,
/// otherwise renders the current buffer's day summary into a new one.
///
/// Unlike the autocommand-driven paths, this says why nothing happened when the
/// current buffer is not a tracking file — the user asked for it by name.
pub fn toggle_preview_fn(config: &'static Config) -> Result<()> {
    if !is_time_tracking_file(config)? {
        // The user typed :TimeTrackingToggle explicitly, and README names this
        // as the first troubleshooting step — so unlike the autocommand-driven
        // paths, say why nothing happened.
        let buffer_name = api::get_current_buf()
            .get_name()
            .map(|n| n.to_string())
            .unwrap_or_else(|_| String::new());
        log_error!(
            "[time-tracking-nvim] {} is not a tracking file (data directory: {:?}). \
             Tracking files are .md files inside the data directory.",
            if buffer_name.is_empty() {
                "[No Name]"
            } else {
                &buffer_name
            },
            config.get_data_directory().unwrap_or("<unset>")
        );
        return Ok(());
    }

    if preview_is_open()? {
        close_preview()?;
    } else {
        render_current_buffer(config)?;
    }

    Ok(())
}

/// `:TimeTrackingUpdate`, and the render the debounce timer schedules: rebuilds
/// the day summary in the preview.
///
/// Does nothing unless the current buffer is a tracking file *and* a preview
/// window is already open — it never opens one.
pub fn update_preview_fn(config: &'static Config) -> Result<()> {
    if !is_time_tracking_file(config)? {
        return Ok(());
    }

    if preview_is_open()? {
        render_current_buffer(config)?;
    }

    Ok(())
}

/// Create the scratch buffer that backs the preview, and prime both caches.
fn create_preview_buffer() -> Result<Buffer> {
    let mut b = api::create_buf(false, true)?; // listed=false, scratch=true
    b.set_name("[Time Tracking Preview]")?;

    // Keep it unlisted and non-modifiable by default (DO NOT set 'readonly')
    let bopts = OptionOptsBuilder::default().buf(b.clone()).build();
    api::set_option_value("buflisted", false, &bopts)?;
    api::set_option_value("modifiable", false, &bopts)?;
    api::set_option_value("bufhidden", "wipe", &bopts)?;
    api::set_option_value("swapfile", false, &bopts)?;
    set_cached_preview_buf(Some(b.clone()));
    set_last_output(None);
    Ok(b)
}

/// The real line write behind [`write_preview_contents`].
fn set_preview_lines(buf: &mut Buffer, lines: Vec<String>) -> Result<()> {
    buf.set_lines(0..buf.line_count()?, false, lines)?;
    Ok(())
}

/// Write `output` into the preview buffer, skipping the rewrite when nothing
/// changed.
///
/// The rendered day summary is unchanged for most keystrokes, and rewriting
/// yanks the preview's scroll position and repaints the whole split.
///
/// No `buf.is_valid()` check: the caller passes either a buffer just created by
/// [`create_preview_buffer`] or one resolved by `find_preview`, whose cache
/// already revalidates before returning it (see `cached_preview_buf`) — so it
/// is always valid here, and checking again would only cost an FFI call while
/// suggesting a trust boundary that isn't there.
fn write_preview_contents(buf: &mut Buffer, output: &str) -> Result<()> {
    write_preview_contents_with(buf, output, set_preview_lines)
}

/// [`write_preview_contents`] with the line write injected, so that a *failing*
/// write can be provoked from a test.
///
/// It cannot be provoked any other way. `nvim_set_option_value` fires no
/// `OptionSet` event, so no autocommand can interleave between making the
/// buffer modifiable and writing it, and `nvim_buf_set_lines` into a buffer
/// just made modifiable does not fail of its own accord. Without this
/// parameter the restore-before-propagate ordering below — the whole point of
/// the function — would be guarded by code review alone; with it,
/// `test_a_failed_preview_write_restores_nomodifiable_and_leaves_the_cache_clean`
/// in `integration_tests` pins both halves of it.
///
/// Not part of the plugin's interface: `#[doc(hidden)]`, and production code
/// reaches it only through [`write_preview_contents`].
#[doc(hidden)]
pub fn write_preview_contents_with(
    buf: &mut Buffer,
    output: &str,
    write_lines: fn(&mut Buffer, Vec<String>) -> Result<()>,
) -> Result<()> {
    if last_output_matches(output) {
        return Ok(());
    }

    let bopts = OptionOptsBuilder::default().buf(buf.clone()).build();
    api::set_option_value("modifiable", true, &bopts)?;
    let lines: Vec<String> = output.lines().map(|s| s.to_string()).collect();
    let write = write_lines(buf, lines);
    // Restore before propagating: an early `?` here would leave the preview
    // permanently modifiable, so the user could type into it and lose the
    // edits on the next render.
    api::set_option_value("modifiable", false, &bopts)?;
    write?;
    set_last_output(Some(output.to_owned()));
    Ok(())
}

/// Apply the preview's window-local options and width.
///
/// A vsplit copies the source window's local options, so an ordinary
/// `set number relativenumber list signcolumn=yes` config eats 6-8 of the
/// preview's ~26 columns. Style it as the scratch preview it is.
///
/// Returns nothing: every call here logs its own failure, and none are fatal.
fn style_preview_window(win: &mut Window, source_width: u32) {
    // Keep the split’s width fixed
    let wopts = OptionOptsBuilder::default().win(win.clone()).build();
    if let Err(e) = api::set_option_value("winfixwidth", true, &wopts) {
        debug_log!("[ttnvim] could not pin preview width: {}\n", e);
    }

    // Cosmetic only — a failure costs the user some visual noise in the
    // preview, never correctness, so one debug line for the group is enough.
    for (name, value) in [
        ("number", false.into()),
        ("relativenumber", false.into()),
        ("wrap", false.into()),
        ("cursorline", false.into()),
        ("spell", false.into()),
        ("list", false.into()),
    ] {
        if let Err(e) = api::set_option_value::<nvim_oxi::Object>(name, value, &wopts) {
            debug_log!("[ttnvim] could not style preview ({}): {}\n", name, e);
        }
    }
    if let Err(e) = api::set_option_value("signcolumn", "no", &wopts) {
        debug_log!("[ttnvim] could not style preview (signcolumn): {}\n", e);
    }
    if let Err(e) = api::set_option_value("foldcolumn", "0", &wopts) {
        debug_log!("[ttnvim] could not style preview (foldcolumn): {}\n", e);
    }

    // ~1/3 of the screen, but never more than the window we split from can
    // spare: `columns` is global, and applying it to a window that is
    // itself only a third of the screen squeezes the user's edit window to
    // a couple of columns.
    if let Ok(total_cols) =
        api::get_option_value::<i64>("columns", &OptionOptsBuilder::default().build())
    {
        let one_third = u32::try_from(total_cols / PREVIEW_SCREEN_FRACTION).unwrap_or(u32::MAX);
        let width = one_third
            .min(source_width.saturating_sub(MIN_PREVIEW_COLUMNS))
            .max(MIN_PREVIEW_COLUMNS);
        if let Err(e) = win.set_width(width) {
            debug_log!("[ttnvim] could not set preview width: {}\n", e);
        }
    }
}

/// Open a vertical split to the right and attach the preview buffer to it.
///
/// Returns `Ok(())` without splitting when the window is too narrow or a window
/// operation is already in progress — a missing preview beats a broken layout.
fn open_preview_split(buf: &Buffer) -> Result<()> {
    // Capture the window we are about to split, before the split halves it.
    let origin = api::get_current_win();
    let source_width = origin.get_width().unwrap_or(u32::MAX);

    if source_width < MIN_SPLIT_COLUMNS {
        debug_log!(
            "[ttnvim] skipping preview split: source window is {} columns\n",
            source_width
        );
        return Ok(());
    }

    // Use a plain command for portability; it's fine here.
    if let Err(e) = api::command("rightbelow vsplit") {
        let msg = e.to_string();
        if msg.contains("E242") || msg.contains("Can't split a window while closing another") {
            // Window operation in progress; skip this update.
            debug_log!("[ttnvim] skipping split during window close: {}\n", msg);
            return Ok(());
        }
        log_error!("[time-tracking-nvim] failed to split: {}", msg);
        return Ok(());
    }

    // Current window is the new split
    let mut win: Window = api::get_current_win();

    // Attach our preview buffer
    if let Err(e) = win.set_buf(buf) {
        log_error!("[time-tracking-nvim] failed to set preview buffer: {}", e);
        if let Err(close_err) = win.close(false) {
            debug_log!("[ttnvim] failed to close orphan split: {}\n", close_err);
        }
        return Ok(());
    }

    style_preview_window(&mut win, source_width);

    // Restore focus by handle rather than `wincmd p`: the split has already
    // repointed Vim's previous-window pointer, so `wincmd p` only lands
    // correctly by accident — and it overwrites the user's own previous-window
    // target on the way. This changes where the cursor ends up, so a failure
    // is user-visible and warrants more than a debug line.
    if let Err(e) = api::set_current_win(&origin) {
        log_error!(
            "[time-tracking-nvim] could not return focus after opening the preview: {}",
            e
        );
    }

    Ok(())
}

/// Create or update the preview window with formatted time tracking data
pub fn create_or_update_preview(output: &str) -> Result<()> {
    // Bail if Neovim has no windows yet (during early startup churn)
    if api::list_wins().next().is_none() {
        return Ok(());
    }

    // Resolve the preview buffer and the window showing it in a single pass.
    let (preview, preview_win) = match find_preview()? {
        Some((buf, win)) => (Some(buf), win),
        None => (None, None),
    };

    let mut buf: Buffer = match preview {
        Some(b) => b,
        None => create_preview_buffer()?,
    };

    write_preview_contents(&mut buf, output)?;

    // `find_preview` resolved this above; a buffer created just now is by
    // definition displayed nowhere.
    if preview_win.is_none() {
        open_preview_split(&buf)?;
    }

    Ok(())
}

/// Closes the preview window and clears both preview caches.
///
/// When the preview is the only window left it is not closed at all:
/// `nvim_win_close` refuses the last window with E444, so a fresh listed buffer
/// is swapped into it instead. The caches are cleared on every path, including
/// the early return taken when no preview is open.
pub fn close_preview() -> Result<()> {
    let Some((_, Some(mut win))) = find_preview()? else {
        set_cached_preview_buf(None);
        set_last_output(None);
        return Ok(());
    };

    let window_count = api::list_wins().count();

    if window_count == 1 {
        // nvim_win_close behaves like :close and refuses the last window
        // (E444). Swap in a normal buffer instead, so the user lands somewhere
        // usable rather than stuck in the unlisted, nomodifiable preview with
        // no way back but :b#.
        match api::create_buf(true, false) {
            Ok(replacement) => {
                if let Err(e) = win.set_buf(&replacement) {
                    log_error!(
                        "[time-tracking-nvim] could not replace the preview buffer: {}",
                        e
                    );
                }
            }
            Err(e) => {
                log_error!(
                    "[time-tracking-nvim] could not create a replacement buffer: {}",
                    e
                );
            }
        }
    } else if let Err(e) = win.close(false) {
        // Non-fatal: propagating here turns a single close failure into an
        // error re-echoed on every subsequent BufEnter/WinClosed.
        log_error!(
            "[time-tracking-nvim] could not close the preview window: {}",
            e
        );
    }

    set_cached_preview_buf(None);
    set_last_output(None);
    Ok(())
}

/// Auto-open preview window if this is a time tracking file and preview isn't open
pub fn auto_open_preview(config: &'static Config) -> Result<()> {
    // Log and swallow the error rather than surfacing it at the command: this
    // runs from the VimEnter/BufWinEnter autocommand, so propagating would
    // re-echo the same failure on every buffer switch. Nothing here catches
    // unwinds — panics are caught a level up, by `catch_nvim_panic` in `lib.rs`.
    match auto_open_preview_impl(config) {
        Ok(_) => Ok(()),
        Err(e) => {
            log_error!("Auto-open failed: {}", e);
            Ok(())
        }
    }
}

/// Fallible body behind [`auto_open_preview`]: renders and opens the preview for
/// a tracking buffer that no preview window is showing yet.
pub fn auto_open_preview_impl(config: &'static Config) -> Result<()> {
    // No delay here: this runs on Neovim's single event-loop thread, so
    // sleeping cannot let a pending window operation complete — it is exactly
    // what prevents it. The split-during-close race is handled by the E242
    // guard in `open_preview_split` and by `create_or_update_preview`'s
    // empty-window-list bail.
    let is_tracking = is_time_tracking_file(config)?;
    if !is_tracking {
        log_info!("[TimeTracking] Auto-open: Not a tracking file");
        return Ok(());
    }

    if !preview_is_open()? {
        render_current_buffer(config)?;
    }

    Ok(())
}

/// Auto-close preview window if we're not in a time tracking file
pub fn auto_close_preview(config: &'static Config) -> Result<()> {
    // Log and swallow the error rather than surfacing it at the command, as
    // `auto_open_preview` does: closing the preview is best-effort. Nothing here
    // catches unwinds — panics are caught a level up, by `catch_nvim_panic` in
    // `lib.rs`.
    match auto_close_preview_impl(config) {
        Ok(_) => Ok(()),
        Err(e) => {
            log_error!("Auto-close failed: {}", e);
            Ok(())
        }
    }
}

pub fn auto_close_preview_impl(_config: &'static Config) -> Result<()> {
    // Always close the preview when BufLeave is triggered for a markdown file.
    // The autocommand pattern ensures we only get called for .md files.
    log_info!("Auto-closing preview (leaving markdown file)\n");
    close_preview()
}
