use nvim_oxi::{
    Dictionary, Function, Result,
    api::{
        self,
        opts::{CreateAutocmdOpts, CreateCommandOpts},
        types::CommandArgs,
    },
};
use std::path::Path;
use time_tracking_cli::Config;

/// Check if the current buffer is a time tracking file (markdown file in data directory)
fn is_time_tracking_file(config: &Config) -> Result<bool> {
    let current_buffer = api::get_current_buf();
    let buffer_name = current_buffer.get_name()?;

    if buffer_name.as_os_str().is_empty() {
        return Ok(false);
    }

    let buffer_path = Path::new(&buffer_name);
    let data_dir_str = config.get_data_directory().unwrap_or("");
    let data_dir = Path::new(data_dir_str);

    // Check if file is in data directory and has .md extension
    Ok(buffer_path.starts_with(data_dir)
        && matches!(buffer_path.extension(), Some(ext) if ext == "md"))
}

/// Get the content of the current buffer
fn get_buffer_content() -> Result<String> {
    let current_buffer = api::get_current_buf();
    let line_count = current_buffer.line_count()?;
    let lines = current_buffer.get_lines(0..line_count, false)?;
    Ok(lines
        .into_iter()
        .map(|s| s.to_string())
        .collect::<Vec<String>>()
        .join("\n"))
}

/// Create or update the preview window with formatted time tracking data
fn create_or_update_preview(output: &str) -> Result<()> {
    // Check if preview buffer already exists
    let buffers = api::list_bufs();
    let mut preview_buf = None;

    for buf in buffers {
        let buf_name = buf.get_name()?;
        if buf_name.ends_with("[Time Tracking Preview]") {
            preview_buf = Some(buf);
            break;
        }
    }

    // Create buffer if it doesn't exist
    let mut buf = match preview_buf {
        Some(buf) => buf,
        None => {
            let mut buf = api::create_buf(false, true)?;
            buf.set_name("[Time Tracking Preview]")?;
            #[allow(deprecated)]
            {
                buf.set_option("buftype", "nofile")?;
                buf.set_option("bufhidden", "hide")?;
                buf.set_option("swapfile", false)?;
                buf.set_option("readonly", true)?;
                buf.set_option("modifiable", false)?;
            }
            buf
        }
    };

    // Update buffer content
    #[allow(deprecated)]
    {
        buf.set_option("modifiable", true)?;
    }
    let output_lines: Vec<String> = output.lines().map(|s| s.to_string()).collect();
    buf.set_lines(0..buf.line_count()?, false, output_lines)?;
    #[allow(deprecated)]
    {
        buf.set_option("modifiable", false)?;
    }

    // Check if preview window is already open
    let windows = api::list_wins();
    let mut preview_win_exists = false;

    for win in windows {
        let win_buf = win.get_buf()?;
        if win_buf == buf {
            preview_win_exists = true;
            break;
        }
    }

    // Create window if it doesn't exist
    if !preview_win_exists {
        // Create a vertical split on the right
        api::command("vnew")?;
        let mut win = api::get_current_win();
        win.set_buf(&buf)?;

        // Set window width to 1/3 of screen
        #[allow(deprecated)]
        let total_width = api::get_option::<i64>("columns")?;
        let preview_width = total_width / 3;
        win.set_width(preview_width as u32)?;

        // Go back to the original window
        api::command("wincmd p")?;
    }

    Ok(())
}

/// Close the preview window if it exists
fn close_preview() -> Result<()> {
    let windows = api::list_wins();

    for win in windows {
        let buf = win.get_buf()?;
        let buf_name = buf.get_name()?;
        if buf_name.ends_with("[Time Tracking Preview]") {
            win.close(false)?;
            break;
        }
    }

    Ok(())
}

/// Plugin to provide time tracking previews while editing in Neovim.
#[nvim_oxi::plugin]
fn time_tracking_nvim() -> Result<Dictionary> {
    // Get a reference to our configuration
    #[allow(unused_variables)]
    let config = Config::get_no_args();

    // The plugin will generate data on-demand when commands are executed

    // Create command to toggle preview
    let toggle_preview = Function::from_fn(move |_: CommandArgs| -> Result<()> {
        let config = Config::get_no_args();

        // Check if this is a time tracking file
        if !is_time_tracking_file(config)? {
            // Just return silently if not a time tracking file
            return Ok(());
        }

        // Check if preview window exists
        let windows = api::list_wins();
        let mut has_preview = false;

        for win in windows {
            let buf = win.get_buf()?;
            let buf_name = buf.get_name()?;
            if buf_name.ends_with("[Time Tracking Preview]") {
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
    });

    // Create command to update preview (for auto-updating)
    let update_preview = Function::from_fn(move |_: CommandArgs| -> Result<()> {
        let config = Config::get_no_args();

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
            if buf_name.ends_with("[Time Tracking Preview]") {
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
    });

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

    // Set up autocommands for live updates on markdown files
    api::create_autocmd(
        vec!["TextChanged", "TextChangedI"],
        &CreateAutocmdOpts::builder()
            .command("TimeTrackingUpdate")
            .build(),
    )?;

    let api = Dictionary::new();
    Ok(api)
}
