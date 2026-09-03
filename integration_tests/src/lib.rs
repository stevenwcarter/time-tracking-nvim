use nvim_oxi::api;
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;
use time_tracking_cli::Config;
use time_tracking_nvim::utils::*;

// Helper function to create a test config with a temporary directory
fn create_test_config_with_temp_dir() -> (Config, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temporary directory");
    // Only the fields these tests actually depend on are spelled out; the rest
    // come from Config::default() so that new upstream fields don't break the
    // build here.
    let config = Config {
        data_directory: Some(temp_dir.path().to_str().unwrap().to_string()),
        date: time::Date::from_calendar_date(2024, time::Month::January, 1).unwrap(),
        ..Default::default()
    };
    (config, temp_dir)
}

// Helper function to create a test file in a directory
fn create_test_file(dir: &std::path::Path, filename: &str, content: &str) -> std::path::PathBuf {
    let file_path = dir.join(filename);
    if let Some(parent) = file_path.parent() {
        fs::create_dir_all(parent).expect("Failed to create parent directories");
    }
    let mut file = File::create(&file_path).expect("Failed to create test file");
    write!(file, "{}", content).expect("Failed to write to test file");
    file_path
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_with_md_in_data_dir() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    
    // Create a markdown file in the data directory
    let md_file = create_test_file(temp_dir.path(), "test.md", "# Test Content");
    
    // Create a buffer with this file
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();
    
    // Test the function
    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(result, "Markdown file in data directory should be identified as time tracking file");
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_with_txt_in_data_dir() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    
    // Create a text file in the data directory
    let txt_file = create_test_file(temp_dir.path(), "test.txt", "Test Content");
    
    // Create a buffer with this file
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&txt_file).unwrap();
    
    // Test the function
    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(!result, "Text file in data directory should not be identified as time tracking file");
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_with_md_outside_data_dir() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    
    // Create another temp dir outside the data directory
    let other_temp_dir = TempDir::new().expect("Failed to create second temp directory");
    let md_file = create_test_file(other_temp_dir.path(), "test.md", "# Test Content");
    
    // Create a buffer with this file
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();
    
    // Test the function
    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(!result, "Markdown file outside data directory should not be identified as time tracking file");
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_with_empty_buffer_name() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    
    // Create a buffer with no name (empty buffer)
    let buf = api::create_buf(false, false).unwrap();
    
    // Test the function
    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(!result, "Buffer with empty name should not be identified as time tracking file");
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_in_subdirectory() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    
    // Create a markdown file in a subdirectory of the data directory
    let md_file = create_test_file(temp_dir.path(), "2024/january/project.md", "# Project Notes");
    
    // Create a buffer with this file
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();
    
    // Test the function
    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(result, "Markdown file in subdirectory of data directory should be identified as time tracking file");
}

#[nvim_oxi::test]
fn test_is_time_tracking_file_current_buffer() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    
    // Create a markdown file in the data directory
    let md_file = create_test_file(temp_dir.path(), "current.md", "# Current Buffer Test");
    
    // Set the current buffer to this file
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();
    api::set_current_buf(&buf).unwrap();
    
    // Test the function
    let result = is_time_tracking_file(&config).unwrap();
    assert!(result, "Current buffer with markdown file in data directory should be identified as time tracking file");
}

#[nvim_oxi::test]
fn test_is_win_time_tracking_file() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    
    // Create a markdown file in the data directory
    let md_file = create_test_file(temp_dir.path(), "window.md", "# Window Test");
    
    // Create a buffer and set it in the current window
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();
    
    let mut win = api::get_current_win();
    win.set_buf(&buf).unwrap();
    
    // Test the function
    let result = is_win_time_tracking_file(win, &config).unwrap();
    assert!(result, "Window with markdown buffer in data directory should be identified as time tracking window");
}

#[nvim_oxi::test]
fn test_get_buffer_content() {
    // Create a buffer with some content
    let mut buf = api::create_buf(false, false).unwrap();
    let test_lines = vec!["# Test Header", "Some content", "More content"];
    buf.set_lines(.., false, test_lines.iter().cloned()).unwrap();
    
    // Set it as current buffer
    api::set_current_buf(&buf).unwrap();
    
    // Test the function
    let result = get_buffer_content().unwrap();
    let expected = test_lines.join("\n");
    assert_eq!(result, expected, "Buffer content should match the set lines joined by newlines");
}

#[nvim_oxi::test]
fn test_get_buffer_content_empty() {
    // Create an empty buffer
    let buf = api::create_buf(false, false).unwrap();
    api::set_current_buf(&buf).unwrap();
    
    // Test the function
    let result = get_buffer_content().unwrap();
    assert_eq!(result, "", "Empty buffer should return empty string");
}

#[nvim_oxi::test]
fn test_any_tracking_visible_with_tracking_window() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    
    // Create a markdown file in the data directory
    let md_file = create_test_file(temp_dir.path(), "visible.md", "# Visible Test");
    
    // Create a buffer and set it in a window
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();
    
    let mut win = api::get_current_win();
    win.set_buf(&buf).unwrap();
    
    // Test the function
    let result = any_tracking_visible(&config).unwrap();
    assert!(result, "Should detect time tracking file in visible window");
}

#[nvim_oxi::test]
fn test_any_tracking_visible_with_preview_window() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    
    // Create a buffer that looks like a preview window
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name("some/path/[Time Tracking Preview]").unwrap();
    
    let mut win = api::get_current_win();
    win.set_buf(&buf).unwrap();
    
    // Test the function - should return false because preview windows are ignored
    let result = any_tracking_visible(&config).unwrap();
    assert!(!result, "Should ignore preview windows when checking for visible tracking files");
}

#[nvim_oxi::test]
fn test_any_tracking_visible_no_tracking_files() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    
    // Create another temp dir outside the data directory
    let other_temp_dir = TempDir::new().expect("Failed to create temp directory");
    let txt_file = create_test_file(other_temp_dir.path(), "normal.txt", "Normal file");
    
    // Create a buffer with a non-tracking file
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&txt_file).unwrap();
    
    let mut win = api::get_current_win();
    win.set_buf(&buf).unwrap();
    
    // Test the function
    let result = any_tracking_visible(&config).unwrap();
    assert!(!result, "Should return false when no time tracking files are visible");
}

// Tests for lib.rs functions
use time_tracking_nvim::{create_or_update_preview, time_tracking_with_config};

#[nvim_oxi::test]
fn test_create_or_update_preview_creates_new_buffer() {
    let test_output = "# Time Tracking Summary\n\n## Today\n- Task 1: 2h\n- Task 2: 1.5h";
    
    // Ensure we start with no preview buffer
    let mut initial_buffers = api::list_bufs();
    let has_preview_initially = initial_buffers.any(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    });
    assert!(!has_preview_initially, "Should start without preview buffer");
    
    // Create preview
    let result = create_or_update_preview(test_output);
    assert!(result.is_ok(), "Should successfully create preview: {:?}", result);
    
    // Verify preview buffer was created
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers.find(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    });
    assert!(preview_buffer.is_some(), "Preview buffer should be created");
    
    // Verify buffer content
    let buf = preview_buffer.unwrap();
    let lines: Vec<String> = buf.get_lines(.., false).unwrap()
        .map(|s| s.to_string())
        .collect();
    let content = lines.join("\n");
    assert_eq!(content, test_output, "Buffer content should match input");
}

#[nvim_oxi::test]
fn test_create_or_update_preview_updates_existing_buffer() {
    let initial_output = "# Initial Content\n- Item 1";
    let updated_output = "# Updated Content\n- Item 1\n- Item 2";
    
    // Create initial preview
    create_or_update_preview(initial_output).unwrap();
    
    // Verify initial content
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers.find(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    }).expect("Preview buffer should exist");
    
    // Update preview
    let result = create_or_update_preview(updated_output);
    assert!(result.is_ok(), "Should successfully update preview: {:?}", result);
    
    // Verify updated content
    let lines: Vec<String> = preview_buffer.get_lines(.., false).unwrap()
        .map(|s| s.to_string())
        .collect();
    let content = lines.join("\n");
    assert_eq!(content, updated_output, "Buffer content should be updated");
}

#[nvim_oxi::test]
fn test_create_or_update_preview_with_empty_output() {
    let empty_output = "";
    
    let result = create_or_update_preview(empty_output);
    assert!(result.is_ok(), "Should handle empty output: {:?}", result);
    
    // Verify buffer was created with empty content
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers.find(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    });
    assert!(preview_buffer.is_some(), "Preview buffer should be created even with empty content");
}

#[nvim_oxi::test]
fn test_create_or_update_preview_buffer_options() {
    let test_output = "# Test Content";
    
    create_or_update_preview(test_output).unwrap();
    
    // Find the preview buffer
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers.find(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    }).expect("Preview buffer should exist");
    
    // Check buffer options
    let bopts = nvim_oxi::api::opts::OptionOptsBuilder::default()
        .buf(preview_buffer.clone())
        .build();
    
    let buflisted: bool = api::get_option_value("buflisted", &bopts).unwrap();
    assert!(!buflisted, "Preview buffer should not be listed");
    
    let modifiable: bool = api::get_option_value("modifiable", &bopts).unwrap();
    assert!(!modifiable, "Preview buffer should not be modifiable after creation");
    
    let bufhidden: String = api::get_option_value("bufhidden", &bopts).unwrap();
    assert_eq!(bufhidden, "wipe", "Preview buffer should be wiped when hidden");
    
    let swapfile: bool = api::get_option_value("swapfile", &bopts).unwrap();
    assert!(!swapfile, "Preview buffer should not use swapfile");
}

#[nvim_oxi::test]
fn test_time_tracking_with_config_creates_commands() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    
    // Use Box::leak to create a static reference for the lifetime requirement
    let config_static: &'static Config = Box::leak(Box::new(config));
    
    // Call the function
    let result = time_tracking_with_config(config_static);
    assert!(result.is_ok(), "Should successfully create commands: {:?}", result);
    
    // Verify commands were created by trying to execute them
    // Note: We can't easily test the command functionality without more complex setup,
    // but we can verify they exist by checking if they're callable
    
    let commands_to_test = vec![
        "TimeTrackingToggle",
        "TimeTrackingUpdate", 
        "TimeTrackingAutoOpen",
        "TimeTrackingAutoClose",
        "TimeTrackingClose",
        "TimeTrackingMaybeCloseIfInvisible",
    ];
    
    for cmd in commands_to_test {
        // Try to get information about the command - this will fail if command doesn't exist
        let cmd_info_result = api::exec2(&format!("command {}", cmd), &Default::default());
        assert!(cmd_info_result.is_ok(), "Command {} should exist", cmd);
    }
}

#[nvim_oxi::test]
fn test_time_tracking_with_config_creates_autocommands() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    
    // Use Box::leak to create a static reference for the lifetime requirement
    let config_static: &'static Config = Box::leak(Box::new(config));
    
    // Call the function
    let result = time_tracking_with_config(config_static);
    assert!(result.is_ok(), "Should successfully create autocommands: {:?}", result);
    
    // We can't easily verify specific autocommands were created without complex introspection,
    // but we can verify the function completes successfully, which means all autocommands
    // were created without errors
    assert!(result.is_ok());
}

#[nvim_oxi::test]  
fn test_create_or_update_preview_with_multiline_content() {
    let multiline_output = "# Time Summary\n\n## Morning\n- Meeting: 1h\n- Code: 2h\n\n## Afternoon\n- Review: 30m\n- Documentation: 1.5h";
    
    create_or_update_preview(multiline_output).unwrap();
    
    // Verify content is preserved correctly
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers.find(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    }).expect("Preview buffer should exist");
    
    let lines: Vec<String> = preview_buffer.get_lines(.., false).unwrap()
        .map(|s| s.to_string())
        .collect();
    let content = lines.join("\n");
    assert_eq!(content, multiline_output, "Multiline content should be preserved");
    
    // Verify we have the expected number of lines
    let expected_lines: Vec<&str> = multiline_output.lines().collect();
    assert_eq!(lines.len(), expected_lines.len(), "Should have correct number of lines");
}

#[nvim_oxi::test]
fn test_create_or_update_preview_handles_special_characters() {
    let special_content = "# Test with special chars\n\n- Task with émojis: 🚀 ✅\n- Unicode: áéíóú\n- Symbols: @#$%^&*()";
    
    let result = create_or_update_preview(special_content);
    assert!(result.is_ok(), "Should handle special characters: {:?}", result);
    
    // Verify content is preserved
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers.find(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    }).expect("Preview buffer should exist");

    let lines: Vec<String> = preview_buffer.get_lines(.., false).unwrap()
        .map(|s| s.to_string())
        .collect();
    let content = lines.join("\n");
    assert_eq!(content, special_content, "Special characters should be preserved");
}

// Helper function to clean up preview buffers between tests
fn cleanup_preview_buffers() {
    let buffers = api::list_bufs();
    for buf in buffers {
        if let Ok(name) = buf.get_name() {
            if name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]")) {
                let _ = buf.delete(&nvim_oxi::api::opts::BufDeleteOpts::builder().force(true).build());
            }
        }
    }
}

#[nvim_oxi::test]
fn test_multiple_preview_creation_updates_same_buffer() {
    cleanup_preview_buffers();
    
    let content1 = "First content";
    let content2 = "Second content";
    let content3 = "Third content";
    
    // Create first preview
    create_or_update_preview(content1).unwrap();
    
    let buffers_after_first = api::list_bufs();
    let preview_count_1 = buffers_after_first.filter(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    }).count();
    assert_eq!(preview_count_1, 1, "Should have exactly one preview buffer after first creation");
    
    // Update preview
    create_or_update_preview(content2).unwrap();
    
    let buffers_after_second = api::list_bufs();
    let preview_count_2 = buffers_after_second.filter(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    }).count();
    assert_eq!(preview_count_2, 1, "Should still have exactly one preview buffer after update");
    
    // Update again
    create_or_update_preview(content3).unwrap();
    
    let buffers_after_third = api::list_bufs();
    let preview_count_3 = buffers_after_third.filter(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    }).count();
    assert_eq!(preview_count_3, 1, "Should still have exactly one preview buffer after second update");
    
    // Verify final content - need to get buffers again since we consumed the iterator
    let mut buffers_final = api::list_bufs();
    let preview_buffer = buffers_final.find(|buf| {
        buf.get_name().map(|name| name.to_str().is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))).unwrap_or(false)
    }).expect("Preview buffer should exist");
    
    let lines: Vec<String> = preview_buffer.get_lines(.., false).unwrap()
        .map(|s| s.to_string())
        .collect();
    let content = lines.join("\n");
    assert_eq!(content, content3, "Should have the latest content");
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_for_file_not_yet_written() {
    let (config, temp_dir) = create_test_config_with_temp_dir();

    // The primary workflow: `nvim ~/timetracking/2026-09-03.md` for today's
    // date, where the file does not exist on disk yet.
    let unwritten = temp_dir.path().join("2026-09-03.md");
    assert!(!unwritten.exists(), "precondition: file must not exist");

    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&unwritten).unwrap();

    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(
        result,
        "a .md file in the data directory that has not been written yet \
         should still be recognised as a time tracking file"
    );
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_unwritten_file_outside_data_dir() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let other_dir = TempDir::new().unwrap();

    let unwritten = other_dir.path().join("2026-09-03.md");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&unwritten).unwrap();

    let result = is_buf_time_tracking_file(buf, &config).unwrap();
    assert!(
        !result,
        "tolerating an unwritten file must not also stop enforcing the \
         data-directory boundary"
    );
}

#[nvim_oxi::test]
fn test_missing_data_directory_returns_false_and_does_not_panic() {
    // A data_directory that does not exist — the "misconfigured time-tracking-cli"
    // case that currently turns the whole plugin into a silent no-op.
    let config = Config {
        data_directory: Some("/nonexistent/time/tracking/dir".to_string()),
        date: time::Date::from_calendar_date(2024, time::Month::January, 1).unwrap(),
        ..Default::default()
    };

    let scratch = TempDir::new().unwrap();
    let md_file = create_test_file(scratch.path(), "test.md", "# Test");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();

    // Repeated calls model the per-keystroke TextChanged path: the warning
    // must be emitted at most once, and no call may panic or error.
    for _ in 0..5 {
        let buf = buf.clone();
        let result = is_buf_time_tracking_file(buf, &config);
        assert!(
            result.is_ok(),
            "a missing data directory must not produce an Err: {:?}",
            result
        );
        assert!(!result.unwrap(), "nothing is a tracking file without a data dir");
    }
}

#[nvim_oxi::test]
fn test_data_dir_memo_does_not_leak_between_configs() {
    // Two configs with different data directories, used alternately. A
    // process-global memo keyed only on "have I run yet" would answer with the
    // first config's directory for the second config's buffer.
    let (config_a, dir_a) = create_test_config_with_temp_dir();
    let (config_b, dir_b) = create_test_config_with_temp_dir();

    let file_a = create_test_file(dir_a.path(), "a.md", "# A");
    let file_b = create_test_file(dir_b.path(), "b.md", "# B");

    for _ in 0..3 {
        let mut buf_a = api::create_buf(false, false).unwrap();
        buf_a.set_name(&file_a).unwrap();
        assert!(
            is_buf_time_tracking_file(buf_a.clone(), &config_a).unwrap(),
            "file A must resolve against config A"
        );

        let mut buf_b = api::create_buf(false, false).unwrap();
        buf_b.set_name(&file_b).unwrap();
        assert!(
            is_buf_time_tracking_file(buf_b.clone(), &config_b).unwrap(),
            "file B must resolve against config B"
        );

        // Cross pairs must stay false. Reuses buf_a (rather than a second
        // buffer also named file_a) because Neovim does not allow two
        // buffers to share a name at once.
        assert!(
            !is_buf_time_tracking_file(buf_a.clone(), &config_b).unwrap(),
            "file A must not resolve against config B"
        );

        // Free both names so the next iteration's create_buf + set_name
        // does not collide with this iteration's buffers.
        let delete_opts = nvim_oxi::api::opts::BufDeleteOpts::builder()
            .force(true)
            .build();
        buf_a.delete(&delete_opts).unwrap();
        buf_b.delete(&delete_opts).unwrap();
    }
}

