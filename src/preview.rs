use super::*;

pub fn toggle_preview_fn(config: &'static Config) -> Result<()> {
    // Check if this is a time tracking file
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

    // Check if preview window exists
    let windows = api::list_wins();
    let mut has_preview = false;

    for win in windows {
        let buf = win.get_buf()?;
        let buf_name = buf.get_name()?;
        if buf_name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            has_preview = true;
            break;
        }
    }

    if has_preview {
        close_preview()?;
    } else {
        let buffer_content = get_buffer_content()?;
        let formatted_output = config.get_formatter().day_summary(
            &buffer_content,
            "",
            config.get_prefix(),
            config.get_suffix(),
        );
        create_or_update_preview(&formatted_output)?;
    }

    Ok(())
}

pub fn update_preview_fn(config: &'static Config) -> Result<()> {
    // Only update if it's a time tracking file and preview is open
    if !is_time_tracking_file(config)? {
        return Ok(());
    }

    // Check if preview window exists
    let windows = api::list_wins();
    let mut has_preview = false;

    for win in windows {
        let buf = win.get_buf()?;
        let buf_name = buf.get_name()?;
        if buf_name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            has_preview = true;
            break;
        }
    }

    if has_preview {
        let buffer_content = get_buffer_content()?;
        let formatted_output = config.get_formatter().day_summary(
            &buffer_content,
            "",
            config.get_prefix(),
            config.get_suffix(),
        );
        create_or_update_preview(&formatted_output)?;
    }

    Ok(())
}

/// Create or update the preview window with formatted time tracking data
pub fn create_or_update_preview(output: &str) -> Result<()> {
    // Bail if Neovim has no windows yet (during early startup churn)
    if api::list_wins().len() == 0 {
        return Ok(());
    }

    // Find an existing preview buffer
    let mut preview: Option<Buffer> = None;
    for b in api::list_bufs() {
        if b.get_name()?
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            preview = Some(b);
            break;
        }
    }

    // Create a scratch buffer if missing
    let mut buf: Buffer = match preview {
        Some(b) => b,
        None => {
            let mut b = api::create_buf(false, true)?; // listed=false, scratch=true
            b.set_name("[Time Tracking Preview]")?;

            // Keep it unlisted and non-modifiable by default (DO NOT set 'readonly')
            let bopts = OptionOptsBuilder::default().buf(b.clone()).build();
            api::set_option_value("buflisted", false, &bopts)?;
            api::set_option_value("modifiable", false, &bopts)?;
            api::set_option_value("bufhidden", "wipe", &bopts)?;
            api::set_option_value("swapfile", false, &bopts)?;
            b
        }
    };

    // Update buffer contents safely by toggling only 'modifiable'
    {
        let bopts = OptionOptsBuilder::default().buf(buf.clone()).build();
        api::set_option_value("modifiable", true, &bopts)?;
        let lines: Vec<String> = output.lines().map(|s| s.to_string()).collect();
        buf.set_lines(0..buf.line_count()?, false, lines)?;
        api::set_option_value("modifiable", false, &bopts)?;
    }

    // Is the preview buffer already shown?
    let mut is_open = false;
    for w in api::list_wins() {
        if w.get_buf()? == buf {
            is_open = true;
            break;
        }
    }

    // If not, create a vertical split and attach the preview buffer to it
    if !is_open {
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
        if let Err(e) = win.set_buf(&buf) {
            log_error!("[time-tracking-nvim] failed to set preview buffer: {}", e);
            let _ = win.close(false);
            return Ok(());
        }

        // Keep the split’s width fixed
        let wopts = OptionOptsBuilder::default().win(win.clone()).build();
        let _ = api::set_option_value("winfixwidth", true, &wopts);

        // Make it ~1/3 of the screen (columns is global; default opts OK)
        if let Ok(total_cols) =
            api::get_option_value::<i64>("columns", &OptionOptsBuilder::default().build())
        {
            let width = (total_cols / 3).max(20) as u32;
            let _ = win.set_width(width);
        }

        // Return to the previous window
        let _ = api::command("wincmd p");
    }

    Ok(())
}

/// Close the preview window if it exists
pub fn close_preview() -> Result<()> {
    let windows: Vec<Window> = api::list_wins().collect();
    let window_count = windows.len();

    for mut win in windows {
        let buf = win.get_buf()?;
        let buf_name = buf.get_name()?;
        if buf_name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            if window_count == 1 {
                // nvim_win_close behaves like :close and refuses the last
                // window (E444). Swap in a normal buffer instead, so the user
                // lands somewhere usable rather than stuck in the unlisted,
                // nomodifiable preview with no way back but :b#.
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
                // Non-fatal: propagating here turns a single close failure into
                // an error re-echoed on every subsequent BufEnter/WinClosed.
                log_error!(
                    "[time-tracking-nvim] could not close the preview window: {}",
                    e
                );
            }
            break;
        }
    }

    Ok(())
}

/// Auto-open preview window if this is a time tracking file and preview isn't open
pub fn auto_open_preview(config: &'static Config) -> Result<()> {
    // Add error handling wrapper to prevent panics
    match auto_open_preview_impl(config) {
        Ok(_) => Ok(()),
        Err(e) => {
            log_error!("Auto-open failed: {}", e);
            Ok(()) // Don't propagate error to prevent crash
        }
    }
}

pub fn auto_open_preview_impl(config: &'static Config) -> Result<()> {
    // No delay here: this runs on Neovim's single event-loop thread, so
    // sleeping cannot let a pending window operation complete — it is exactly
    // what prevents it. The split-during-close race is handled by the E242
    // guard in create_or_update_preview and the empty-window-list bail.
    let is_tracking = is_time_tracking_file(config)?;
    if !is_tracking {
        log_info!("[TimeTracking] Auto-open: Not a tracking file");
        return Ok(());
    }

    // Check if preview window already exists
    let windows = api::list_wins();
    let mut has_preview = false;

    for win in windows {
        let buf = win.get_buf()?;
        let buf_name = buf.get_name()?;
        if buf_name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            has_preview = true;
            break;
        }
    }

    // Only open if preview doesn't already exist
    if !has_preview {
        let buffer_content = get_buffer_content()?;
        let formatted_output = config.get_formatter().day_summary(
            &buffer_content,
            "",
            config.get_prefix(),
            config.get_suffix(),
        );
        create_or_update_preview(&formatted_output)?;
    }

    Ok(())
}

/// Auto-close preview window if we're not in a time tracking file
pub fn auto_close_preview(config: &'static Config) -> Result<()> {
    // Add error handling wrapper to prevent panics
    match auto_close_preview_impl(config) {
        Ok(_) => Ok(()),
        Err(e) => {
            log_error!("Auto-close failed: {}", e);
            Ok(()) // Don't propagate error to prevent crash
        }
    }
}

pub fn auto_close_preview_impl(_config: &'static Config) -> Result<()> {
    // Always close the preview when BufLeave is triggered for a markdown file
    // The autocommand pattern ensures we only get called for .md files
    // Check if preview window exists and close it
    let windows = api::list_wins();
    for win in windows {
        let buf = win.get_buf()?;
        let buf_name = buf.get_name()?;
        if buf_name
            .to_str()
            .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            log_info!("Auto-closing preview (leaving markdown file)\n");
            win.close(false)?;
            break;
        }
    }

    Ok(())
}
