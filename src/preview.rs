use crate::utils::{PREVIEW_BUF_NAME, get_buffer_content, is_preview_buf, is_time_tracking_file};
use crate::{debug_log, log_error, log_info};
use nvim_oxi::api::{Buffer, Window, opts::OptionOptsBuilder};
use nvim_oxi::{Result, api};
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use time_tracking_cli::Config;

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

/// The window showing `buf` in *any* tabpage, if any.
///
/// The global counterpart to [`preview_win_in_current_tab`], and deliberately
/// used by [`close_preview`] alone: closing is the one operation that has to
/// reach the preview wherever it lives. See that function for why.
fn preview_win_anywhere(buf: &Buffer) -> Result<Option<Window>> {
    for w in api::list_wins() {
        if &w.get_buf()? == buf {
            return Ok(Some(w));
        }
    }
    Ok(None)
}

/// The preview buffer, from the handle cache or — on a miss — a buffer scan.
///
/// Primes the cache on a scan hit. Buffers are global in Neovim, so there is at
/// most one of these no matter how many tabpages display it.
fn find_preview_buf() -> Result<Option<Buffer>> {
    if let Some(buf) = cached_preview_buf() {
        return Ok(Some(buf));
    }

    for b in api::list_bufs() {
        if is_preview_buf(&b)? {
            set_cached_preview_buf(Some(b.clone()));
            return Ok(Some(b));
        }
    }

    Ok(None)
}

/// Resolve the preview buffer and the window *in the current tabpage* showing
/// it, in one pass.
///
/// Returns `None` when no preview buffer exists; `Some((buf, None))` when the
/// buffer exists but no window **in the current tabpage** is displaying it —
/// which does not mean it is off screen, since another tabpage may still be
/// showing it. That qualifier is the whole point of this lookup (B45): it is
/// what lets a second tabpage open a preview of its own. It is also why
/// [`close_preview`] does not use it.
///
/// Consolidates the visibility lookups and gives the handle cache a single
/// invalidation point.
fn find_preview() -> Result<Option<(Buffer, Option<Window>)>> {
    let Some(buf) = find_preview_buf()? else {
        return Ok(None);
    };

    let window = preview_win_in_current_tab(&buf)?;

    Ok(Some((buf, window)))
}

/// Minimum interval between autocommand-driven renders.
///
/// A *throttle*, not a debounce: the first change in a burst renders at once,
/// and the rest render on this cadence, so the preview keeps up with
/// continuous typing instead of waiting for the user to stop.
const THROTTLE: Duration = Duration::from_millis(200);

/// Below this width a vertical split fails outright with E36 and damages the
/// layout on the way out, so no preview is the better outcome.
const MIN_SPLIT_COLUMNS: u32 = 40;

/// The preview aims for this fraction of the total screen width.
const PREVIEW_SCREEN_FRACTION: i64 = 3;

/// Floor for the preview, and the minimum width left to the window it split from.
const MIN_PREVIEW_COLUMNS: u32 = 20;

thread_local! {
    /// When the last throttle-path render happened.
    ///
    /// `None` until the first one, which is what lets the first change of a
    /// session render immediately.
    static LAST_RENDER: Cell<Option<Instant>> = const { Cell::new(None) };

    /// Whether a render is already booked for the current throttle window.
    ///
    /// This flag is the entire difference between this and the debounce it
    /// replaced. The debounce cancelled and re-armed its timer on every
    /// keystroke, pushing the render out for as long as the user kept typing.
    /// Here a booked render stays booked: later changes in the same window see
    /// this set and return, and the booked render fires on the window
    /// boundary.
    ///
    /// Cleared by [`throttle_fire`], which the timer reaches through
    /// `:TimeTrackingThrottleFire`. This plugin never cancels a booking of its
    /// own, but "a booked timer always fires exactly once, and always clears
    /// this" is an *assumption about the whole Neovim process*, not something
    /// this code can enforce: a `timer_stopall()` from unrelated code sharing
    /// the session destroys the booking without ever reaching
    /// [`throttle_fire`]. A flag left set that way would drop every
    /// autocommand-driven render for the rest of the session, so
    /// [`update_preview_throttled`] defends against it twice — it rolls the
    /// flag back explicitly when arming fails, and it treats a booking older
    /// than any deadline it could have had as lost.
    static THROTTLE_PENDING: Cell<bool> = const { Cell::new(false) };
}

/// Autocommand entry point: hold autocommand-driven renders to at most one per
/// [`THROTTLE`].
///
/// `TextChanged`/`TextChangedI` fire once per keystroke on Neovim's single UI
/// thread, and each render pays canonicalize syscalls, a window scan, a
/// full-buffer read and a re-parse — too much to do per keystroke. Rendering
/// only once the user stops, which is what the debounce this replaced did,
/// costs the opposite thing: the preview sits frozen for as long as they keep
/// typing. A leading-edge throttle does neither. The first change renders at
/// once and the rest land on a steady cadence, so the summary visibly
/// accumulates while the notes are being written.
///
/// `:TimeTrackingUpdate` deliberately still calls [`update_preview_fn`]
/// directly: a user who types the command expects to see the result now, not
/// at the next window boundary.
pub fn update_preview_throttled(config: &'static Config) -> Result<()> {
    // Render nothing for a buffer that can never show a preview. The
    // autocommand fires for every `*.md` buffer, not just tracking notes, so
    // without this every README keystroke would pay for a window scan and a
    // timer. `update_preview_fn` re-checks this when the timer fires, against
    // whatever buffer is current by then.
    if !is_time_tracking_file(config)? {
        return Ok(());
    }

    if THROTTLE_PENDING.get() {
        // A booked render is always due within `THROTTLE` of `LAST_RENDER`, so
        // twice that with no render having happened means no timer is coming.
        // (`THROTTLE_PENDING` implies `LAST_RENDER` is `Some` — the flag is
        // only set on a path that required it — so the `None` arm is
        // defensive, and treating it as stale is the safe direction.)
        let stale = LAST_RENDER
            .get()
            .is_none_or(|last| last.elapsed() >= THROTTLE * 2);
        if !stale {
            // A genuine booking: leave its deadline alone — moving it is
            // exactly what would turn this back into a debounce.
            return Ok(());
        }
        // The booking outlived any deadline it could have had, so its timer is
        // gone — `timer_stopall()` from unrelated code sharing this Neovim,
        // say. Drop it and re-arm below, rather than staying dead for the rest
        // of the session.
        THROTTLE_PENDING.set(false);
    }

    let remaining = LAST_RENDER.get().and_then(|last| {
        let elapsed = last.elapsed();
        (elapsed < THROTTLE).then(|| THROTTLE - elapsed)
    });

    let Some(remaining) = remaining else {
        // Leading edge: no window is open, so render now, synchronously.
        LAST_RENDER.set(Some(Instant::now()));
        return update_preview_fn(config);
    };

    // Inside an open window: book the render for the window *boundary* rather
    // than for `THROTTLE` from now, so the cadence stays even under continuous
    // typing instead of drifting later with each keystroke.
    THROTTLE_PENDING.set(true);
    if let Err(e) = arm_throttle_timer(remaining) {
        // Nothing else clears the flag if arming failed, and a stuck flag
        // would freeze the preview for the rest of the session.
        THROTTLE_PENDING.set(false);
        return Err(e);
    }

    Ok(())
}

/// Ask Neovim to run `:TimeTrackingThrottleFire` in `remaining`.
///
/// Deliberately Neovim's own `timer_start()` rather than nvim-oxi's
/// `libuv::TimerHandle`, which backed the debounce this replaced.
/// `TimerHandle` cannot be built on Windows — nvim-oxi's `uv_*` externs carry
/// no `raw-dylib` attribute and `nvim.exe` exports no such symbols — and it
/// leaks its `uv_timer_t` on every arm, because `libuv::Handle` has no `Drop`
/// impl and `TimerHandle` offers no `&mut self` re-arm. `timer_start` has
/// neither problem, and its callback runs on the main loop rather than in
/// libuv's fast event context, so the render it triggers needs no `schedule()`
/// hop to reach somewhere the API is legal.
///
/// The zero-argument lambda is Vim's own documented timer idiom (`:help
/// timer_start`): Neovim passes the timer id and the lambda ignores it.
fn arm_throttle_timer(remaining: Duration) -> Result<()> {
    // Floor of 1ms: `timer_start(0, ...)` is legal but says "next loop turn",
    // which is not what a sub-millisecond remainder means.
    let ms = remaining.as_millis().max(1);
    api::command(&format!(
        "call timer_start({ms}, {{-> execute('TimeTrackingThrottleFire')}})"
    ))?;
    Ok(())
}

/// `:TimeTrackingThrottleFire`: the render [`update_preview_throttled`] booked
/// for the end of the current window.
///
/// Internal — the timer is its only caller.
///
/// Returns `Ok(())` even when the render fails. This runs from a timer
/// callback with no user action attached, so an `Err` would surface as a bare
/// "Error executing vim function callback" with nothing to connect it to. The
/// logged message is more use than the error.
pub fn throttle_fire(config: &'static Config) -> Result<()> {
    THROTTLE_PENDING.set(false);
    LAST_RENDER.set(Some(Instant::now()));

    if let Err(e) = update_preview_fn(config) {
        log_error!("[time-tracking-nvim] throttled update failed: {}", e);
    }

    Ok(())
}

/// Clear the throttle window, so the next [`update_preview_throttled`] takes
/// the leading edge.
///
/// Test seam, not interface: it lets the integration tests establish a known
/// window boundary without sleeping.
#[doc(hidden)]
pub fn reset_throttle_for_test() {
    THROTTLE_PENDING.set(false);
    LAST_RENDER.set(None);
}

/// Is a window in the current tabpage showing the preview, per an already
/// resolved [`find_preview`] result?
fn preview_is_open_in(found: &Option<(Buffer, Option<Window>)>) -> bool {
    matches!(found, Some((_, Some(_))))
}

/// Render the current buffer's day summary into the preview.
///
/// The single read-format-write path: every entry point that shows tracking
/// data goes through here, so the formatter arguments are specified once.
fn render_current_buffer(config: &Config, found: Option<(Buffer, Option<Window>)>) -> Result<()> {
    let buffer_content = get_buffer_content()?;
    let formatted_output = config.get_formatter().day_summary(
        &buffer_content,
        "",
        config.get_prefix(),
        config.get_suffix(),
    );
    create_or_update_preview_with(found, &formatted_output)
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

    let found = find_preview()?;
    if preview_is_open_in(&found) {
        close_preview()?;
    } else {
        render_current_buffer(config, found)?;
    }

    Ok(())
}

/// `:TimeTrackingUpdate`, and the render the throttle books: rebuilds the day
/// summary in the preview.
///
/// Does nothing unless the current buffer is a tracking file *and* a preview
/// window is already open — it never opens one.
pub fn update_preview_fn(config: &'static Config) -> Result<()> {
    if !is_time_tracking_file(config)? {
        return Ok(());
    }

    let found = find_preview()?;
    if preview_is_open_in(&found) {
        render_current_buffer(config, found)?;
    }

    Ok(())
}

/// Create the scratch buffer that backs the preview, and prime both caches.
fn create_preview_buffer() -> Result<Buffer> {
    let mut b = api::create_buf(false, true)?; // listed=false, scratch=true
    b.set_name(PREVIEW_BUF_NAME)?;

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
fn set_preview_lines(buf: &mut Buffer, lines: Vec<&str>) -> Result<()> {
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
    write_lines: fn(&mut Buffer, Vec<&str>) -> Result<()>,
) -> Result<()> {
    if last_output_matches(output) {
        return Ok(());
    }

    let bopts = OptionOptsBuilder::default().buf(buf.clone()).build();
    api::set_option_value("modifiable", true, &bopts)?;
    let lines: Vec<&str> = output.lines().collect();
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
        // Saturate to 0, not `u32::MAX`: a negative `columns` is unreachable
        // (Neovim's minimum is 12), but if it ever were, clamping low leaves
        // the preview at `MIN_PREVIEW_COLUMNS`, while clamping high would hand
        // it everything the source window can spare. Low is the direction the
        // pre-`try_from` `(total_cols / 3).max(0) as u32` failed in.
        let one_third = u32::try_from(total_cols / PREVIEW_SCREEN_FRACTION).unwrap_or(0);
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
    // Bail before `find_preview` too: it calls `list_wins` on the current
    // tabpage, which can error in the same window-less startup state the
    // guard inside `create_or_update_preview_with` exists to absorb.
    if api::list_wins().next().is_none() {
        return Ok(());
    }

    create_or_update_preview_with(find_preview()?, output)
}

/// [`create_or_update_preview`] with the lookup already done.
///
/// Callers that had to probe for an open preview before deciding to render
/// pass their own `find_preview` result straight through, instead of throwing
/// it away and making this function repeat the scan.
///
/// Every path that renders — direct or through [`render_current_buffer`] —
/// goes through here, so the early-startup bail below has to live here too,
/// not in [`create_or_update_preview`] alone. The bail runs after `found` is
/// resolved: `find_preview` only enumerates buffers and windows, which is
/// safe with no windows open, so moving the check past that lookup costs
/// nothing.
fn create_or_update_preview_with(
    found: Option<(Buffer, Option<Window>)>,
    output: &str,
) -> Result<()> {
    // Bail if Neovim has no windows yet (during early startup churn)
    if api::list_wins().next().is_none() {
        return Ok(());
    }

    let (preview, preview_win) = match found {
        Some((buf, win)) => (Some(buf), win),
        None => (None, None),
    };

    let mut buf: Buffer = if let Some(b) = preview {
        b
    } else {
        create_preview_buffer()?
    };

    write_preview_contents(&mut buf, output)?;

    // `find_preview` resolved this above; a buffer created just now is by
    // definition displayed nowhere.
    if preview_win.is_none() {
        open_preview_split(&buf)?;
    }

    Ok(())
}

/// Closes the preview window — wherever it lives — and clears both preview
/// caches.
///
/// The window scan here is [`preview_win_anywhere`], not the tab-scoped probe
/// [`find_preview`] uses, for two reasons.
///
/// It is driven by `any_tracking_visible` (`src/utils.rs`), which enumerates
/// every tabpage: a tab-local close would let the guard and the action disagree
/// about scope, and would make `:TimeTrackingClose` a silent no-op whenever the
/// user is in a tabpage that has no preview of its own.
///
/// And it is what makes clearing the caches below correct. With a global scan,
/// "no window" means the preview is displayed nowhere at all, so forgetting it
/// costs nothing. A tab-local scan would also take that path while another
/// tabpage still had the preview on screen, dropping `LAST_OUTPUT` under a live
/// preview — and the next render there would fail the dirty-check and rewrite
/// the whole buffer, which is exactly the scroll-yanking repaint that cache
/// exists to prevent (see [`write_preview_contents`]).
///
/// When the preview is the only window left it is not closed at all:
/// `nvim_win_close` refuses the last window with E444, so a fresh listed buffer
/// is swapped into it instead. The caches are cleared on every path, including
/// the early return taken when no preview is open.
pub fn close_preview() -> Result<()> {
    let preview_win = match find_preview_buf()? {
        Some(buf) => preview_win_anywhere(&buf)?,
        None => None,
    };

    let Some(mut win) = preview_win else {
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

/// Run `r`, reporting any error under `label` and swallowing it.
///
/// Both autocommand-driven wrappers want the same thing: a failure reported
/// once, not propagated. Propagating would re-echo the same message on every
/// buffer switch. Panics are caught a level up, by `catch_nvim_panic` in
/// `lib.rs`.
fn log_and_swallow(label: &str, r: Result<()>) -> Result<()> {
    if let Err(e) = r {
        log_error!("{} failed: {}", label, e);
    }
    Ok(())
}

/// Auto-open preview window if this is a time tracking file and preview isn't open
pub fn auto_open_preview(config: &'static Config) -> Result<()> {
    log_and_swallow("Auto-open", auto_open_preview_impl(config))
}

/// Fallible body behind [`auto_open_preview`]: renders and opens the preview for
/// a tracking buffer that no preview window is showing yet.
fn auto_open_preview_impl(config: &'static Config) -> Result<()> {
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

    let found = find_preview()?;
    if !preview_is_open_in(&found) {
        render_current_buffer(config, found)?;
    }

    Ok(())
}

/// Auto-close preview window if we're not in a time tracking file
pub fn auto_close_preview(config: &'static Config) -> Result<()> {
    log_and_swallow("Auto-close", auto_close_preview_impl(config))
}

fn auto_close_preview_impl(_config: &'static Config) -> Result<()> {
    // Always close the preview when BufLeave is triggered for a markdown file.
    // The autocommand pattern ensures we only get called for .md files.
    log_info!("Auto-closing preview (leaving markdown file)\n");
    close_preview()
}
