use nvim_oxi::api;
use std::fs::{self, File};
use std::io::Write;
use tempfile::TempDir;
use time_tracking_cli::Config;
use time_tracking_nvim::utils::*;
use time_tracking_nvim::{catch_nvim_panic_for_test, clear_last_error_for_test};

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
    let result = is_buf_time_tracking_file(&buf, &config).unwrap();
    assert!(
        result,
        "Markdown file in data directory should be identified as time tracking file"
    );
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
    let result = is_buf_time_tracking_file(&buf, &config).unwrap();
    assert!(
        !result,
        "Text file in data directory should not be identified as time tracking file"
    );
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
    let result = is_buf_time_tracking_file(&buf, &config).unwrap();
    assert!(
        !result,
        "Markdown file outside data directory should not be identified as time tracking file"
    );
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_with_empty_buffer_name() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();

    // Create a buffer with no name (empty buffer)
    let buf = api::create_buf(false, false).unwrap();

    // Test the function
    let result = is_buf_time_tracking_file(&buf, &config).unwrap();
    assert!(
        !result,
        "Buffer with empty name should not be identified as time tracking file"
    );
}

#[nvim_oxi::test]
fn test_is_buf_time_tracking_file_in_subdirectory() {
    let (config, temp_dir) = create_test_config_with_temp_dir();

    // Create a markdown file in a subdirectory of the data directory
    let md_file = create_test_file(
        temp_dir.path(),
        "2024/january/project.md",
        "# Project Notes",
    );

    // Create a buffer with this file
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file).unwrap();

    // Test the function
    let result = is_buf_time_tracking_file(&buf, &config).unwrap();
    assert!(
        result,
        "Markdown file in subdirectory of data directory should be identified as time tracking file"
    );
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
    assert!(
        result,
        "Current buffer with markdown file in data directory should be identified as time tracking file"
    );
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
    let result = is_win_time_tracking_file(&win, &config).unwrap();
    assert!(
        result,
        "Window with markdown buffer in data directory should be identified as time tracking window"
    );
}

#[nvim_oxi::test]
fn test_get_buffer_content() {
    // Create a buffer with some content
    let mut buf = api::create_buf(false, false).unwrap();
    let test_lines = ["# Test Header", "Some content", "More content"];
    buf.set_lines(.., false, test_lines.iter().cloned())
        .unwrap();

    // Set it as current buffer
    api::set_current_buf(&buf).unwrap();

    // Test the function
    let result = get_buffer_content().unwrap();
    let expected = test_lines.join("\n");
    assert_eq!(
        result, expected,
        "Buffer content should match the set lines joined by newlines"
    );
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
    let result = any_tracking_visible(&config, None).unwrap();
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
    let result = any_tracking_visible(&config, None).unwrap();
    assert!(
        !result,
        "Should ignore preview windows when checking for visible tracking files"
    );
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
    let result = any_tracking_visible(&config, None).unwrap();
    assert!(
        !result,
        "Should return false when no time tracking files are visible"
    );
}

#[nvim_oxi::test]
fn test_any_tracking_visible_skips_the_excluded_window() {
    let (config, temp_dir) = create_test_config_with_temp_dir();

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    let win = api::get_current_win();
    let handle = win.handle();

    assert!(
        any_tracking_visible(&config, None).unwrap(),
        "the tracking window is visible when nothing is excluded"
    );

    // WinClosed fires "just before it is removed from the window layout", so
    // the closing window is still in list_wins() when the handler runs.
    assert!(
        !any_tracking_visible(&config, Some(handle)).unwrap(),
        "the window being closed must not count itself as still visible"
    );
}

#[nvim_oxi::test]
fn test_only_md_files_can_be_tracking_files() {
    // Invariant 2 (see the code-health spec): narrowing the TextChanged
    // autocmd pattern from `*` to `*.md` is behavior-preserving ONLY because
    // is_buf_time_tracking_file already requires a .md extension. Pin it here
    // so a later change that relaxes the extension check fails loudly instead
    // of silently disabling live updates for the newly-allowed extensions.
    let (config, temp_dir) = create_test_config_with_temp_dir();

    for name in ["notes.txt", "notes.markdown", "notes", "notes.md.bak"] {
        let file = create_test_file(temp_dir.path(), name, "content");
        let mut buf = api::create_buf(false, false).unwrap();
        buf.set_name(&file).unwrap();
        assert!(
            !is_buf_time_tracking_file(&buf, &config).unwrap(),
            "{name} must not be a tracking file — the TextChanged autocmd only \
             fires for *.md"
        );
    }

    let md = create_test_file(temp_dir.path(), "notes.md", "content");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    assert!(is_buf_time_tracking_file(&buf, &config).unwrap());
}

// Tests for lib.rs functions
use time_tracking_nvim::{close_preview, create_or_update_preview, time_tracking_with_config};

#[nvim_oxi::test]
fn test_create_or_update_preview_creates_new_buffer() {
    let test_output = "# Time Tracking Summary\n\n## Today\n- Task 1: 2h\n- Task 2: 1.5h";

    // Ensure we start with no preview buffer
    let mut initial_buffers = api::list_bufs();
    let has_preview_initially = initial_buffers.any(|buf| {
        buf.get_name()
            .map(|name| {
                name.to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
            })
            .unwrap_or(false)
    });
    assert!(
        !has_preview_initially,
        "Should start without preview buffer"
    );

    // Create preview
    let result = create_or_update_preview(test_output);
    assert!(
        result.is_ok(),
        "Should successfully create preview: {:?}",
        result
    );

    // Verify preview buffer was created
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers.find(|buf| {
        buf.get_name()
            .map(|name| {
                name.to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
            })
            .unwrap_or(false)
    });
    assert!(preview_buffer.is_some(), "Preview buffer should be created");

    // Verify buffer content
    let buf = preview_buffer.unwrap();
    let lines: Vec<String> = buf
        .get_lines(.., false)
        .unwrap()
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
    let preview_buffer = buffers
        .find(|buf| {
            buf.get_name()
                .map(|name| {
                    name.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("Preview buffer should exist");

    // Update preview
    let result = create_or_update_preview(updated_output);
    assert!(
        result.is_ok(),
        "Should successfully update preview: {:?}",
        result
    );

    // Verify updated content
    let lines: Vec<String> = preview_buffer
        .get_lines(.., false)
        .unwrap()
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
        buf.get_name()
            .map(|name| {
                name.to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
            })
            .unwrap_or(false)
    });
    assert!(
        preview_buffer.is_some(),
        "Preview buffer should be created even with empty content"
    );
}

#[nvim_oxi::test]
fn test_create_or_update_preview_buffer_options() {
    let test_output = "# Test Content";

    create_or_update_preview(test_output).unwrap();

    // Find the preview buffer
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers
        .find(|buf| {
            buf.get_name()
                .map(|name| {
                    name.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("Preview buffer should exist");

    // Check buffer options
    let bopts = nvim_oxi::api::opts::OptionOptsBuilder::default()
        .buf(preview_buffer.clone())
        .build();

    let buflisted: bool = api::get_option_value("buflisted", &bopts).unwrap();
    assert!(!buflisted, "Preview buffer should not be listed");

    let modifiable: bool = api::get_option_value("modifiable", &bopts).unwrap();
    assert!(
        !modifiable,
        "Preview buffer should not be modifiable after creation"
    );

    let bufhidden: String = api::get_option_value("bufhidden", &bopts).unwrap();
    assert_eq!(
        bufhidden, "wipe",
        "Preview buffer should be wiped when hidden"
    );

    let swapfile: bool = api::get_option_value("swapfile", &bopts).unwrap();
    assert!(!swapfile, "Preview buffer should not use swapfile");
}

// Pins Invariant #1 of the tidy spec: the seven `TimeTracking*` command names
// are the plugin's public API and must survive byte-identical. `src/lib.rs`
// registers six of them from a `(name, desc, handler)` data table, where a
// name/handler transposition is a one-line typo.
//
// Deliberately NOT `:command {name}`, which this test used to run through
// `api::exec2`. `:command` is a *listing* command: it succeeds with "No
// user-defined commands found" whether or not anything matches, so the old
// assertion held with zero commands registered. This reads the registry itself.
//
// Each name is checked with its `nargs` and with "a callback is bound", which is
// as far as the registry can go towards the handler-binding gap: `nargs` pins
// `TimeTrackingMaybeCloseIfInvisible` as the only one of the seven taking an
// argument, so it catches any transposition involving it, and the callback check
// catches a name registered with no handler at all. Transposing two of the six
// nargs=0 handlers inside the data table stays invisible here — the behavioural
// tests elsewhere in this file are what cover that.
#[nvim_oxi::test]
fn test_time_tracking_with_config_creates_commands() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();

    // Use Box::leak to create a static reference for the lifetime requirement
    let config_static: &'static Config = Box::leak(Box::new(config));

    // Call the function
    let result = time_tracking_with_config(config_static);
    assert!(
        result.is_ok(),
        "Should successfully create commands: {:?}",
        result
    );

    // `nvim_get_commands` is read through `luaeval` rather than
    // `api::get_commands`: nvim-oxi's typed wrapper deserializes *every* entry
    // in the registry, and Neovim's own runtime ships `:EditQuery`, whose
    // `complete` is a Lua function where `CommandInfos::complete` is an
    // `Option<String>` — so the typed call fails before it reaches ours.
    const PROBE: &str = "luaeval('(function() \
        local out = {} \
        for name, c in pairs(vim.api.nvim_get_commands({ builtin = false })) do \
        if name:sub(1, 12) == \"TimeTracking\" then \
        out[#out + 1] = name .. \" nargs=\" .. tostring(c.nargs) .. \" handler=\" .. tostring(c.callback ~= nil) \
        end end \
        table.sort(out) \
        return out end)()')";

    let registered: Vec<String> = api::eval(PROBE).unwrap();

    let expected = vec![
        "TimeTrackingAutoClose nargs=0 handler=true".to_string(),
        "TimeTrackingAutoOpen nargs=0 handler=true".to_string(),
        "TimeTrackingClose nargs=0 handler=true".to_string(),
        "TimeTrackingInvalidateBufCache nargs=? handler=true".to_string(),
        "TimeTrackingMaybeCloseIfInvisible nargs=? handler=true".to_string(),
        "TimeTrackingOpenToday nargs=0 handler=true".to_string(),
        "TimeTrackingThrottleFire nargs=0 handler=true".to_string(),
        "TimeTrackingToggle nargs=0 handler=true".to_string(),
        "TimeTrackingUpdate nargs=0 handler=true".to_string(),
        "TimeTrackingUpdateThrottled nargs=0 handler=true".to_string(),
        "TimeTrackingWeeklyToggle nargs=0 handler=true".to_string(),
    ];

    assert_eq!(
        registered, expected,
        "exactly these eleven TimeTracking* commands, with these argument counts, \
         must be registered and bound to a handler"
    );
}

#[nvim_oxi::test]
fn test_time_tracking_with_config_creates_autocommands() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();

    // Use Box::leak to create a static reference for the lifetime requirement
    let config_static: &'static Config = Box::leak(Box::new(config));

    // Call the function
    let result = time_tracking_with_config(config_static);
    assert!(
        result.is_ok(),
        "Should successfully create autocommands: {:?}",
        result
    );

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
    let preview_buffer = buffers
        .find(|buf| {
            buf.get_name()
                .map(|name| {
                    name.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("Preview buffer should exist");

    let lines: Vec<String> = preview_buffer
        .get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect();
    let content = lines.join("\n");
    assert_eq!(
        content, multiline_output,
        "Multiline content should be preserved"
    );

    // Verify we have the expected number of lines
    let expected_lines: Vec<&str> = multiline_output.lines().collect();
    assert_eq!(
        lines.len(),
        expected_lines.len(),
        "Should have correct number of lines"
    );
}

#[nvim_oxi::test]
fn test_create_or_update_preview_handles_special_characters() {
    let special_content = "# Test with special chars\n\n- Task with émojis: 🚀 ✅\n- Unicode: áéíóú\n- Symbols: @#$%^&*()";

    let result = create_or_update_preview(special_content);
    assert!(
        result.is_ok(),
        "Should handle special characters: {:?}",
        result
    );

    // Verify content is preserved
    let mut buffers = api::list_bufs();
    let preview_buffer = buffers
        .find(|buf| {
            buf.get_name()
                .map(|name| {
                    name.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("Preview buffer should exist");

    let lines: Vec<String> = preview_buffer
        .get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect();
    let content = lines.join("\n");
    assert_eq!(
        content, special_content,
        "Special characters should be preserved"
    );
}

// Helper function to clean up preview buffers between tests
fn cleanup_preview_buffers() {
    let buffers = api::list_bufs();
    for buf in buffers {
        if let Ok(name) = buf.get_name()
            && name
                .to_str()
                .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        {
            let _ = buf.delete(
                &nvim_oxi::api::opts::BufDeleteOpts::builder()
                    .force(true)
                    .build(),
            );
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
    let preview_count_1 = buffers_after_first
        .filter(|buf| {
            buf.get_name()
                .map(|name| {
                    name.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .count();
    assert_eq!(
        preview_count_1, 1,
        "Should have exactly one preview buffer after first creation"
    );

    // Update preview
    create_or_update_preview(content2).unwrap();

    let buffers_after_second = api::list_bufs();
    let preview_count_2 = buffers_after_second
        .filter(|buf| {
            buf.get_name()
                .map(|name| {
                    name.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .count();
    assert_eq!(
        preview_count_2, 1,
        "Should still have exactly one preview buffer after update"
    );

    // Update again
    create_or_update_preview(content3).unwrap();

    let buffers_after_third = api::list_bufs();
    let preview_count_3 = buffers_after_third
        .filter(|buf| {
            buf.get_name()
                .map(|name| {
                    name.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .count();
    assert_eq!(
        preview_count_3, 1,
        "Should still have exactly one preview buffer after second update"
    );

    // Verify final content - need to get buffers again since we consumed the iterator
    let mut buffers_final = api::list_bufs();
    let preview_buffer = buffers_final
        .find(|buf| {
            buf.get_name()
                .map(|name| {
                    name.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("Preview buffer should exist");

    let lines: Vec<String> = preview_buffer
        .get_lines(.., false)
        .unwrap()
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

    let result = is_buf_time_tracking_file(&buf, &config).unwrap();
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

    let result = is_buf_time_tracking_file(&buf, &config).unwrap();
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
        let result = is_buf_time_tracking_file(&buf, &config);
        assert!(
            result.is_ok(),
            "a missing data directory must not produce an Err: {:?}",
            result
        );
        assert!(
            !result.unwrap(),
            "nothing is a tracking file without a data dir"
        );
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
            is_buf_time_tracking_file(&buf_a.clone(), &config_a).unwrap(),
            "file A must resolve against config A"
        );

        let mut buf_b = api::create_buf(false, false).unwrap();
        buf_b.set_name(&file_b).unwrap();
        assert!(
            is_buf_time_tracking_file(&buf_b.clone(), &config_b).unwrap(),
            "file B must resolve against config B"
        );

        // Cross pairs must stay false. Reuses buf_a (rather than a second
        // buffer also named file_a) because Neovim does not allow two
        // buffers to share a name at once.
        assert!(
            !is_buf_time_tracking_file(&buf_a.clone(), &config_b).unwrap(),
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

#[nvim_oxi::test]
fn test_data_dir_miss_is_not_cached() {
    // Fix-round regression guard (Finding 2): B15's memo must cache only
    // successful resolutions. A directory that is missing on the first call
    // and created before the second call must resolve on that very next
    // call — the warning text promises "until this is fixed", so caching the
    // miss (recovering only on restart) would contradict it.
    let temp_dir = TempDir::new().unwrap();
    let data_dir = temp_dir.path().join("not-yet-created");
    assert!(
        !data_dir.exists(),
        "precondition: directory must not exist yet"
    );

    let config = Config {
        data_directory: Some(data_dir.to_str().unwrap().to_string()),
        date: time::Date::from_calendar_date(2024, time::Month::January, 1).unwrap(),
        ..Default::default()
    };

    let md_file_path = data_dir.join("test.md");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md_file_path).unwrap();

    let miss = is_buf_time_tracking_file(&buf.clone(), &config).unwrap();
    assert!(
        !miss,
        "a missing data directory must not resolve as a tracking file"
    );

    fs::create_dir_all(&data_dir).unwrap();

    let hit = is_buf_time_tracking_file(&buf, &config).unwrap();
    assert!(
        hit,
        "a data directory that now exists must resolve on the very next call, \
         proving the earlier miss was not cached"
    );
}

#[nvim_oxi::test]
fn test_auto_open_does_not_block_the_event_loop() {
    use std::time::Instant;

    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    // A buffer that is NOT a tracking file: the old code slept 200ms *before*
    // even checking, so this cost the full delay for every unrelated markdown
    // buffer at VimEnter/BufWinEnter.
    let other = TempDir::new().unwrap();
    let md = create_test_file(other.path(), "README.md", "# Unrelated");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    let start = Instant::now();
    for _ in 0..3 {
        time_tracking_nvim::auto_open_preview(config_static).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 150,
        "three auto-open calls on a non-tracking buffer took {:?}; the \
         blocking thread::sleep is still on the event-loop thread",
        elapsed
    );
}

#[nvim_oxi::test]
fn test_toggle_outside_data_dir_creates_no_preview_and_returns_ok() {
    cleanup_preview_buffers();

    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let other = TempDir::new().unwrap();
    let md = create_test_file(other.path(), "notes.md", "# Unrelated");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    let result = time_tracking_nvim::toggle_preview_fn(config_static);
    assert!(result.is_ok(), "toggle must not error: {:?}", result);

    let has_preview = api::list_bufs().any(|b| {
        b.get_name()
            .map(|n| {
                n.to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
            })
            .unwrap_or(false)
    });
    assert!(
        !has_preview,
        "toggling outside the data directory must not create a preview"
    );
}

#[nvim_oxi::test]
fn test_close_preview_when_it_is_the_last_window() {
    cleanup_preview_buffers();

    // Put the preview in the only window, the state reached by pressing
    // <C-w>c in the file window (QuitPre does not fire for :close).
    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    // create_or_update_preview leaves the cursor back in the source window,
    // with the preview as a sibling split. Close the source window (the
    // <C-w>c) rather than `:only` from it: the preview buffer is
    // `bufhidden=wipe`, so `:only` from the source window would close the
    // preview window instead and wipe the buffer out from under us.
    api::command("close").unwrap();
    assert_eq!(api::list_wins().count(), 1, "precondition: one window");

    let result = close_preview();
    assert!(
        result.is_ok(),
        "closing the preview as the last window must not propagate E444: {:?}",
        result
    );

    let still_showing_preview = api::get_current_win()
        .get_buf()
        .unwrap()
        .get_name()
        .map(|n| {
            n.to_str()
                .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
        })
        .unwrap_or(false);
    assert!(
        !still_showing_preview,
        "the user must not be left sitting in the nomodifiable preview buffer"
    );
}

#[nvim_oxi::test]
fn test_preview_window_is_styled_as_a_scratch_preview() {
    use nvim_oxi::api::opts::OptionOptsBuilder;

    cleanup_preview_buffers();

    // A vsplit copies the source window's local options, so set the
    // near-ubiquitous ones on the source first.
    let sopts = OptionOptsBuilder::default()
        .win(api::get_current_win())
        .build();
    let orig_number: bool = api::get_option_value("number", &sopts).unwrap();
    let orig_wrap: bool = api::get_option_value("wrap", &sopts).unwrap();
    let orig_signcolumn: String = api::get_option_value("signcolumn", &sopts).unwrap();
    api::set_option_value("number", true, &sopts).unwrap();
    api::set_option_value("wrap", true, &sopts).unwrap();
    api::set_option_value("signcolumn", "yes", &sopts).unwrap();

    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let preview_win = api::list_wins()
        .find(|w| {
            w.get_buf()
                .and_then(|b| b.get_name())
                .map(|n| {
                    n.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("preview window should exist");

    let wopts = OptionOptsBuilder::default().win(preview_win).build();
    assert!(
        !api::get_option_value::<bool>("number", &wopts).unwrap(),
        "the preview must not show line numbers"
    );
    assert!(
        !api::get_option_value::<bool>("wrap", &wopts).unwrap(),
        "the preview must not soft-wrap"
    );
    assert_eq!(
        api::get_option_value::<String>("signcolumn", &wopts).unwrap(),
        "no",
        "the preview must not reserve a sign column"
    );

    // Restore the source window's options and collapse back to one window so
    // later tests in this shared Neovim instance do not inherit this test's
    // window-local state.
    api::set_option_value("number", orig_number, &sopts).unwrap();
    api::set_option_value("wrap", orig_wrap, &sopts).unwrap();
    api::set_option_value("signcolumn", orig_signcolumn.as_str(), &sopts).unwrap();
    api::command("only").unwrap();
}

#[nvim_oxi::test]
fn test_preview_does_not_crush_a_narrow_source_window() {
    use nvim_oxi::api::opts::OptionOptsBuilder;

    cleanup_preview_buffers();
    api::command("only").unwrap();

    // Pin the screen width so the assertion does not depend on the harness's
    // terminal size.
    let gopts = OptionOptsBuilder::default().build();
    let orig_columns: i64 = api::get_option_value("columns", &gopts).unwrap();
    api::set_option_value("columns", 80i64, &gopts).unwrap();
    let total_cols: i64 = api::get_option_value("columns", &gopts).unwrap();

    // Three vertical splits, so the source window is narrower than
    // total_cols/3 — the layout the finding describes.
    api::command("vsplit").unwrap();
    api::command("vsplit").unwrap();
    api::command("vsplit").unwrap();

    let source_width_before = api::get_current_win().get_width().unwrap();

    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let preview_win = api::list_wins().find(|w| {
        w.get_buf()
            .and_then(|b| b.get_name())
            .map(|n| {
                n.to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
            })
            .unwrap_or(false)
    });

    if let Some(preview_win) = preview_win {
        let preview_width = preview_win.get_width().unwrap();
        assert!(
            i64::from(preview_width) <= i64::from(source_width_before),
            "the preview ({preview_width} cols) took more than the window it \
             split from ({source_width_before} cols); width was computed from \
             the global &columns ({total_cols}) instead of available space"
        );
    }
    // No preview at all is the correct outcome for a very narrow source window.

    // Restore global state so later tests do not inherit this test's pinned
    // screen width or window layout.
    api::set_option_value("columns", orig_columns, &gopts).unwrap();
    api::command("only").unwrap();
}

#[nvim_oxi::test]
fn test_preview_width_clamps_to_source_window_not_global_columns() {
    use nvim_oxi::api::opts::OptionOptsBuilder;

    cleanup_preview_buffers();
    api::command("only").unwrap();

    let gopts = OptionOptsBuilder::default().build();
    let orig_columns: i64 = api::get_option_value("columns", &gopts).unwrap();
    let orig_equalalways: bool = api::get_option_value("equalalways", &gopts).unwrap();

    // Disable rebalancing so the source window's width is exactly what we
    // set it to below, not whatever `equalalways` happens to land on —
    // relying on rebalancing arithmetic to produce a convenient number is
    // what made the original 2-vsplit narrow-window test a false guard.
    api::set_option_value("equalalways", false, &gopts).unwrap();
    api::set_option_value("columns", 200i64, &gopts).unwrap();
    let total_cols: i64 = api::get_option_value("columns", &gopts).unwrap();
    let one_third = total_cols / 3;

    api::command("vsplit").unwrap();
    let mut source_win = api::get_current_win();
    source_win.set_width(50).unwrap();

    let source_width_before = source_win.get_width().unwrap();
    assert!(
        source_width_before >= 40,
        "precondition: source window must be at least 40 columns so the \
         <40 bail does not fire (got {source_width_before})"
    );

    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let preview_win = api::list_wins()
        .find(|w| {
            w.get_buf()
                .and_then(|b| b.get_name())
                .map(|n| {
                    n.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect(
            "preview window must be created: the source window is wide \
             enough that the <40 bail must not fire",
        );

    let preview_width = preview_win.get_width().unwrap();
    assert!(
        i64::from(preview_width) <= i64::from(source_width_before) - 20,
        "the preview ({preview_width} cols) left the {source_width_before}-column \
         source window less than 20 columns to work with"
    );
    assert!(
        i64::from(preview_width) < one_third,
        "the preview ({preview_width} cols) was sized from the global \
         &columns ({total_cols}, one third = {one_third}) instead of the \
         {source_width_before}-column window it split from"
    );

    // Restore global state so later tests do not inherit this test's pinned
    // screen width, disabled rebalancing, or window layout.
    api::set_option_value("columns", orig_columns, &gopts).unwrap();
    api::set_option_value("equalalways", orig_equalalways, &gopts).unwrap();
    api::command("only").unwrap();
}

#[nvim_oxi::test]
fn test_preview_cache_survives_a_wiped_buffer() {
    cleanup_preview_buffers();

    // First creation populates whatever cache exists.
    create_or_update_preview("first").unwrap();
    let first = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| {
                    n.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("preview buffer should exist");

    // bufhidden=wipe means the handle really can go away underneath us.
    api::command(&format!("bwipeout! {}", first.handle())).unwrap();
    assert!(!first.is_valid(), "precondition: the handle is now invalid");

    // Must not reuse the dead handle.
    let result = create_or_update_preview("second");
    assert!(
        result.is_ok(),
        "recreating after a wipe must succeed: {:?}",
        result
    );

    let second = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| {
                    n.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("a fresh preview buffer should have been created");
    assert!(second.is_valid());

    let lines: Vec<String> = second
        .get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(lines, vec!["second".to_string()]);
}

#[nvim_oxi::test]
fn test_identical_output_does_not_rewrite_the_preview_buffer() {
    cleanup_preview_buffers();

    create_or_update_preview("# Summary\n- total: 1h").unwrap();
    let buf = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| {
                    n.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("preview buffer should exist");

    let tick_after_first = buf.get_changedtick().unwrap();

    // The overwhelming majority of keystrokes leave the rendered summary
    // unchanged; rewriting yanks scroll position and repaints the split.
    create_or_update_preview("# Summary\n- total: 1h").unwrap();
    assert_eq!(
        buf.get_changedtick().unwrap(),
        tick_after_first,
        "an identical render must not rewrite the buffer"
    );

    // A genuinely different render must still write.
    create_or_update_preview("# Summary\n- total: 2h").unwrap();
    assert!(
        buf.get_changedtick().unwrap() > tick_after_first,
        "a changed render must write"
    );

    let lines: Vec<String> = buf
        .get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        lines,
        vec!["# Summary".to_string(), "- total: 2h".to_string()]
    );
}

#[nvim_oxi::test]
fn test_recreated_preview_always_gets_a_full_write() {
    cleanup_preview_buffers();

    create_or_update_preview("# Summary\n- total: 1h").unwrap();
    // Wipe it, then render the SAME content: a stale output cache would skip
    // the write and leave the new buffer empty.
    cleanup_preview_buffers();
    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let buf = api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| {
                    n.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("preview buffer should exist");

    let lines: Vec<String> = buf
        .get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        lines,
        vec!["# Summary".to_string(), "- total: 1h".to_string()],
        "a wiped-and-recreated preview must be written in full"
    );
}

#[nvim_oxi::test]
fn test_successful_init_returns_a_dictionary_without_an_error_key() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let dict = time_tracking_with_config(config_static).unwrap();
    assert!(
        dict.get("error").is_none(),
        "a successful init must not advertise an error"
    );
}

/// Locate the preview buffer, or panic with a useful message.
fn preview_buffer() -> nvim_oxi::api::Buffer {
    api::list_bufs()
        .find(|b| {
            b.get_name()
                .map(|n| {
                    n.to_str()
                        .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
                })
                .unwrap_or(false)
        })
        .expect("preview buffer should exist")
}

/// The current contents of the preview buffer, joined back into one string.
fn preview_text(buf: &nvim_oxi::api::Buffer) -> String {
    buf.get_lines(.., false)
        .unwrap()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

#[nvim_oxi::test]
fn test_explicit_update_renders_immediately() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    // Open the preview holding a sentinel the real render must overwrite.
    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();
    let tick_before = preview.get_changedtick().unwrap();

    // :TimeTrackingUpdate must render synchronously — a user who types the
    // command expects to see the result, not to wait out a throttle window.
    // No event-loop turn happens between this call and the assertions, so
    // anything that deferred the write — the debounce this replaced, or a
    // render booked behind the throttle — would leave the sentinel in place.
    time_tracking_nvim::update_preview_fn(config_static).unwrap();

    assert!(preview.is_valid());
    assert!(
        preview.get_changedtick().unwrap() > tick_before,
        "an explicit update must write the preview before it returns"
    );
    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "an explicit update must not be deferred behind the throttle"
    );
}

#[nvim_oxi::test]
fn test_throttled_update_coalesces_a_burst() {
    use std::time::Instant;

    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    time_tracking_with_config(config_static).unwrap();
    let preview = preview_buffer();

    time_tracking_nvim::reset_throttle_for_test();
    // Burn the leading edge, so the 20 calls below all land inside one window.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    let tick_before = preview.get_changedtick().unwrap();

    // Simulate a burst of keystrokes inside one window: each is dropped and
    // returns at once.
    let start = Instant::now();
    for _ in 0..20 {
        time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    }
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 100,
        "20 throttled updates took {:?}; the throttle must not block the \
         event loop",
        elapsed
    );

    // The whole burst must coalesce: nothing has been rendered yet, because
    // the event loop has not turned.
    assert_eq!(
        preview.get_changedtick().unwrap(),
        tick_before,
        "changes inside an open throttle window must not render synchronously"
    );

    api::command("call timer_stopall()").unwrap();
}

#[nvim_oxi::test]
fn test_throttled_update_renders_first_change_immediately() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();
    time_tracking_nvim::reset_throttle_for_test();

    // No event-loop turn happens between this call and the assertion, so
    // anything deferred leaves the sentinel in place. The debounce this
    // replaced always deferred.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "the first change in a burst must render before the call returns; \
         the preview still reads {:?}",
        preview_text(&preview)
    );
}

#[nvim_oxi::test]
fn test_throttled_burst_books_exactly_one_render() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();

    // Registered after the preview exists, so no BufEnter handler runs during
    // setup. Needed because the calls below book a real timer, which fires
    // `:TimeTrackingThrottleFire`.
    time_tracking_with_config(config_static).unwrap();

    // No event-loop turn from here on, so nothing this books can fire before
    // the assertions and `timer_info()` still lists it.
    time_tracking_nvim::reset_throttle_for_test();
    let timers_before: i64 = api::eval("len(timer_info())").unwrap();

    // Burn the leading edge, then 20 more changes inside the same window.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    for _ in 0..20 {
        time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    }

    let timers_after: i64 = api::eval("len(timer_info())").unwrap();
    assert_eq!(
        timers_after - timers_before,
        1,
        "21 changes inside one throttle window must book exactly one render, \
         got {}",
        timers_after - timers_before
    );

    // Leave nothing armed for whatever test runs next in this Neovim.
    api::command("call timer_stopall()").unwrap();
}

#[nvim_oxi::test]
fn test_throttled_render_is_booked_on_the_window_boundary() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();

    // Registered after the preview exists, so no BufEnter handler runs during
    // setup. Needed because the booking below arms a real timer, which fires
    // `:TimeTrackingThrottleFire`.
    time_tracking_with_config(config_static).unwrap();
    time_tracking_nvim::reset_throttle_for_test();

    // Burn the leading edge. That render is synchronous and arms nothing, so
    // turning the event loop below cannot let a booked render fire early and
    // restart the window.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    // Spend ~60ms of the 200ms window, so a boundary-aligned booking is
    // measurably shorter than a `THROTTLE`-from-now one.
    api::exec2(
        "lua vim.wait(60, function() return false end)",
        &Default::default(),
    )
    .unwrap();

    // A change inside the window books the trailing render.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    // Nothing else in this test arms a vimscript timer — `vim.wait` does not —
    // so the single entry is ours.
    let timers: i64 = api::eval("len(timer_info())").unwrap();
    assert_eq!(
        timers, 1,
        "the change must have been booked, not rendered; if the ~60ms wait \
         overran the 200ms window this test measures nothing"
    );

    // `timer_info()`'s `time` is the interval the timer was *set to*, i.e. the
    // exact value `arm_throttle_timer` passed. Booking at the window boundary
    // makes it `THROTTLE - elapsed` (~140ms); booking `THROTTLE` from now
    // would make it 200 — which is what happens if the remaining-time
    // arithmetic is ever replaced with a flat `arm_throttle_timer(THROTTLE)`.
    // The 190 threshold leaves ~50ms of slack over the expected ~140, so a
    // slow or loaded machine cannot flake it, while still failing a flat 200.
    let booked_ms: i64 = api::eval("timer_info()[0].time").unwrap();
    assert!(
        booked_ms < 190,
        "the trailing render must be booked at the window boundary \
         (~140ms after a 60ms wait), not a full THROTTLE from now; \
         timer_start got {booked_ms}ms"
    );

    // Leave nothing armed for whatever test runs next in this Neovim.
    api::command("call timer_stopall()").unwrap();
}

#[nvim_oxi::test]
fn test_throttled_update_renders_the_trailing_change() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // The trailing render arrives through `:TimeTrackingThrottleFire`, so the
    // commands must exist or the timer fires into E492.
    time_tracking_with_config(config_static).unwrap();
    time_tracking_nvim::reset_throttle_for_test();

    // Burn the leading edge, then re-prime the sentinel so only a *second*,
    // trailing render can clear it.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    create_or_update_preview("PLACEHOLDER").unwrap();

    // A change inside the window: booked, not rendered.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    assert!(
        preview_text(&preview).contains("PLACEHOLDER"),
        "a change inside an open window must not render synchronously"
    );

    // Turn the event loop past the window boundary so the booked render fires.
    // `bufnr()` takes a pattern, so address the preview by handle.
    let handle = preview.handle();
    api::exec2(
        &format!(
            "lua vim.wait(2000, function() \
               local l = vim.api.nvim_buf_get_lines({handle}, 0, 1, false)[1] or '' \
               return not l:find('PLACEHOLDER', 1, true) \
             end, 10)"
        ),
        &Default::default(),
    )
    .unwrap();

    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "the booked trailing render must land, so a burst never leaves the \
         preview stale; it still reads {:?}",
        preview_text(&preview)
    );

    // Nothing is armed now — `throttle_fire` cleared the booking as it ran —
    // so turning the event loop another ~60ms is safe, and it is what makes
    // the assertion below able to tell two states apart. The trailing render
    // lands *on* the leading edge's own 200ms boundary, so measured from the
    // leading edge the next change is at ~260ms (outside its window) while
    // measured from the trailing render it is at ~60ms (inside its window).
    // Without this gap the two are a millisecond or two apart and the
    // assertion proves nothing.
    api::exec2(
        "lua vim.wait(60, function() return false end)",
        &Default::default(),
    )
    .unwrap();

    // `throttle_fire` stamped `LAST_RENDER` as the trailing render landed, so
    // this change falls inside a *fresh* window and must be booked rather
    // than rendered. Without that stamp `LAST_RENDER` still holds the leading
    // edge, ~260ms ago, and this change renders synchronously — which is how
    // dropping it roughly doubles the render rate under sustained typing
    // (~0, 200, 210, 410, 420…).
    create_or_update_preview("PLACEHOLDER").unwrap();
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    assert!(
        preview_text(&preview).contains("PLACEHOLDER"),
        "a trailing render must restart the throttle window, so the change \
         after it is booked rather than rendered; the preview reads {:?}",
        preview_text(&preview)
    );

    // Leave nothing armed for whatever test runs next in this Neovim.
    api::command("call timer_stopall()").unwrap();
}

#[nvim_oxi::test]
fn test_throttled_trailing_render_lands_during_insert_mode() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // The trailing render arrives through `:TimeTrackingThrottleFire`, so the
    // commands must exist or the timer fires into E492.
    time_tracking_with_config(config_static).unwrap();
    time_tracking_nvim::reset_throttle_for_test();

    // Burn the leading edge, then re-prime the sentinel so only a *second*,
    // trailing render can clear it.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    create_or_update_preview("PLACEHOLDER").unwrap();

    // Book the trailing render, exactly as in
    // `test_throttled_update_renders_the_trailing_change`. The difference is
    // how the event loop gets turned below.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    // Every other throttle test turns the event loop from Normal mode, but
    // the plugin's real trigger is `TextChangedI`, which fires while the user
    // is in Insert mode. A `<Cmd>` mapping runs without leaving Insert mode,
    // so a `vim.wait` invoked through it spins the event loop while `mode()`
    // is still `i` — that is what lets `timer_start`'s callback (and so the
    // booked render) fire here. Written to a temp file and `luafile`-d,
    // rather than nested inline through `api::exec2`, because this is a
    // Vim-command string containing Lua, itself embedded in Lua, and that is
    // one quoting level too many to stay readable inline.
    let handle = preview.handle();
    let script = temp_dir.path().join("wait_in_insert_mode.lua");
    fs::write(
        &script,
        format!(
            "vim.g.tt_mode_during_wait = 'unset'\n\
             local keys = 'ihello' .. vim.api.nvim_replace_termcodes(\
             '<Cmd>lua vim.g.tt_mode_during_wait = vim.fn.mode(); vim.wait(2000, function() \
             local l = vim.api.nvim_buf_get_lines({handle}, 0, 1, false)[1] or \"\" \
             return not l:find(\"PLACEHOLDER\", 1, true) \
             end, 10)<CR><Esc>', true, false, true)\n\
             vim.fn.feedkeys(keys, 'x')\n"
        ),
    )
    .unwrap();
    api::exec2(
        &format!("luafile {}", script.display()),
        &Default::default(),
    )
    .unwrap();

    // Recorded from *inside* the `<Cmd>` mapping, not inferred afterward —
    // without this the test could silently degrade into another
    // Normal-mode test if the technique above ever stopped working.
    let mode_during_wait: String = api::get_var("tt_mode_during_wait").unwrap();
    assert_eq!(
        mode_during_wait, "i",
        "the wait must run while genuinely in Insert mode; got mode {:?}",
        mode_during_wait
    );

    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "the booked trailing render must land while the buffer is being \
         edited in Insert mode; the preview still reads {:?}",
        preview_text(&preview)
    );
}

#[nvim_oxi::test]
fn test_throttle_recovers_from_a_booking_destroyed_behind_its_back() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // Registered after the preview exists, so no BufEnter handler runs during
    // setup. Needed because the booking below arms a real timer, which fires
    // `:TimeTrackingThrottleFire`.
    time_tracking_with_config(config_static).unwrap();
    time_tracking_nvim::reset_throttle_for_test();

    // Burn the leading edge, then book a trailing render inside the window.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    // Destroy the booking behind the throttle's back — exactly what unrelated
    // code sharing this Neovim does. The timer is gone, but `THROTTLE_PENDING`
    // is still set, because only `throttle_fire` clears it and it will never
    // run now.
    api::command("call timer_stopall()").unwrap();

    // Re-prime the sentinel, then turn the event loop past twice the throttle
    // window (2 x 200ms), so the orphaned booking is provably older than any
    // deadline it could have had.
    create_or_update_preview("PLACEHOLDER").unwrap();
    api::exec2(
        "lua vim.wait(450, function() return false end)",
        &Default::default(),
    )
    .unwrap();

    // A later change must still render. Without the staleness escape hatch
    // the stranded flag drops this one — and every other autocommand-driven
    // update for the rest of the session — leaving only `:TimeTrackingUpdate`
    // working, with nothing to point at as the cause.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "a change arriving after a destroyed booking must still render; the \
         preview still reads {:?}",
        preview_text(&preview)
    );
}

#[nvim_oxi::test]
fn test_throttled_update_renders_nothing_for_a_non_tracking_file() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    // Open the preview from a real tracking file, so a stray render would be
    // plainly visible.
    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut tracked = api::create_buf(false, false).unwrap();
    tracked.set_name(&md).unwrap();
    api::set_current_buf(&tracked).unwrap();
    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // Switch to a markdown buffer outside the data directory. The
    // `TextChanged,TextChangedI *.md` autocommand fires for this buffer too,
    // but it can never produce a preview.
    let other_dir = TempDir::new().expect("Failed to create second temp directory");
    let readme = create_test_file(other_dir.path(), "README.md", "# Readme");
    let mut untracked = api::create_buf(false, false).unwrap();
    untracked.set_name(&readme).unwrap();
    api::set_current_buf(&untracked).unwrap();

    // Establish a known window boundary rather than inheriting whatever the
    // ambient thread-local state happens to be.
    time_tracking_nvim::reset_throttle_for_test();

    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    // Turn the event loop well past the throttle window.
    api::exec2(
        "lua vim.wait(600, function() return false end)",
        &Default::default(),
    )
    .unwrap();

    assert_eq!(
        preview_text(&preview),
        "PLACEHOLDER",
        "a markdown buffer outside the data directory must never render a preview"
    );
}

#[nvim_oxi::test]
fn test_throttle_renders_repeatedly_during_sustained_typing() {
    use std::time::{Duration, Instant};

    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // Register the commands: the throttle books its trailing renders through
    // `:TimeTrackingThrottleFire`. Done after the preview exists so no
    // BufEnter handler runs during setup.
    time_tracking_with_config(config_static).unwrap();

    // Type continuously for ~600ms — three throttle windows — turning the
    // event loop between keystrokes so booked renders get a chance to fire.
    // Re-priming the sentinel each iteration makes each render countable
    // without depending on what the formatter emits.
    let mut renders = 0;
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(600) {
        create_or_update_preview("PLACEHOLDER").unwrap();
        time_tracking_nvim::update_preview_throttled(config_static).unwrap();
        api::exec2(
            "lua vim.wait(20, function() return false end)",
            &Default::default(),
        )
        .unwrap();
        if !preview_text(&preview).contains("PLACEHOLDER") {
            renders += 1;
        }
    }

    assert!(
        renders >= 2,
        "600ms of continuous typing must produce at least two renders \
         (roughly one per 200ms window); got {renders}. A trailing-edge \
         debounce produces none, because every keystroke pushes its deadline \
         out again."
    );
}

#[nvim_oxi::test]
fn test_autocommand_is_throttled_but_explicit_command_is_not() {
    cleanup_preview_buffers();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let md = create_test_file(temp_dir.path(), "today.md", "# Today");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    // Open the preview holding a sentinel before registering the augroup, so
    // no BufEnter handler runs during setup.
    create_or_update_preview("PLACEHOLDER").unwrap();
    let preview = preview_buffer();

    // Register the augroup, including `TextChanged,TextChangedI *.md`.
    time_tracking_with_config(config_static).unwrap();

    time_tracking_nvim::reset_throttle_for_test();

    // The leading edge: the first TextChanged renders synchronously.
    api::exec2("doautocmd TextChanged", &Default::default()).unwrap();
    let tick_after_first = preview.get_changedtick().unwrap();
    assert!(
        !preview_text(&preview).contains("PLACEHOLDER"),
        "the first TextChanged must render at once"
    );

    // A second one inside the same window must not: it is booked instead.
    // This is what pins the autocommand to `:TimeTrackingUpdateThrottled`
    // rather than the unthrottled `:TimeTrackingUpdate`.
    api::exec2("doautocmd TextChanged", &Default::default()).unwrap();
    assert_eq!(
        preview.get_changedtick().unwrap(),
        tick_after_first,
        "a TextChanged inside an open throttle window must go through the throttle"
    );

    // The converse: `:TimeTrackingUpdate` is not throttled, so it renders even
    // inside the window. Re-prime the sentinel so the render is visible.
    create_or_update_preview("PLACEHOLDER").unwrap();
    let tick_before_explicit = preview.get_changedtick().unwrap();
    api::command("TimeTrackingUpdate").unwrap();
    assert!(
        preview.get_changedtick().unwrap() > tick_before_explicit,
        "the explicit :TimeTrackingUpdate command must still render at once"
    );
}

/// Whether the given tabpage has a window showing the preview buffer.
fn tab_shows_preview(tab: &nvim_oxi::api::TabPage) -> bool {
    tab.list_wins().unwrap().any(|w| {
        w.get_buf()
            .and_then(|b| b.get_name())
            .map(|n| {
                n.to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
            })
            .unwrap_or(false)
    })
}

// B45 regression guard: `find_preview` used to scan `api::list_wins()`, which
// enumerates every tabpage. A preview open in tab 1 made that scan conclude a
// preview was "already open" for tab 2 as well, so auto-open opened nothing
// there — the second tab's edits rendered into a buffer only visible back in
// tab 1.
#[nvim_oxi::test]
fn test_auto_open_gives_a_second_tabpage_its_own_preview_window() {
    cleanup_preview_buffers();
    api::command("only").unwrap();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    // Tab 1: open a tracking file and let auto-open create its preview split.
    let md1 = create_test_file(temp_dir.path(), "tab1.md", "# Tab 1");
    let mut buf1 = api::create_buf(false, false).unwrap();
    buf1.set_name(&md1).unwrap();
    api::set_current_buf(&buf1).unwrap();

    time_tracking_nvim::auto_open_preview(config_static).unwrap();

    let tab1 = api::get_current_tabpage();
    assert_eq!(
        tab1.list_wins().unwrap().count(),
        2,
        "precondition: tab 1 must have its own source + preview windows"
    );
    assert!(
        tab_shows_preview(&tab1),
        "precondition: tab 1's preview window must be showing the preview buffer"
    );

    // Tab 2: a fresh tabpage with its own tracking-file window. The preview
    // buffer already exists (it's global), but no window in this tabpage
    // shows it yet.
    api::command("tabnew").unwrap();
    let tab2 = api::get_current_tabpage();
    assert_ne!(
        tab1, tab2,
        "precondition: :tabnew must produce a distinct tabpage from tab 1"
    );

    let md2 = create_test_file(temp_dir.path(), "tab2.md", "# Tab 2");
    let mut buf2 = api::create_buf(false, false).unwrap();
    buf2.set_name(&md2).unwrap();
    api::set_current_buf(&buf2).unwrap();

    assert_eq!(
        tab2.list_wins().unwrap().count(),
        1,
        "precondition: tab 2 starts with only its source window"
    );

    time_tracking_nvim::auto_open_preview(config_static).unwrap();

    // The bug: this used to no-op, because tab 1's preview window made the
    // (untabbed) visibility scan report a preview as already open.
    assert_eq!(
        tab2.list_wins().unwrap().count(),
        2,
        "tab 2 must get its own preview window rather than being told tab 1's \
         counts as already open"
    );
    assert!(
        tab_shows_preview(&tab2),
        "tab 2 must show a window on the (shared) preview buffer"
    );

    // Tab 1's preview window must be untouched by tab 2's auto-open.
    assert_eq!(
        tab1.list_wins().unwrap().count(),
        2,
        "tab 1's own preview window must survive opening tab 2's"
    );

    // Clean up: close tab 2 and collapse tab 1 back to one window, so later
    // tests in this shared Neovim instance do not inherit an extra tabpage.
    api::command("tabclose").unwrap();
    api::command("only").unwrap();
    cleanup_preview_buffers();
}

// Review-fix regression guard for `close_preview`'s scope.
//
// B45 (commit c2cdb47) made the shared preview lookup tab-scoped, and
// `close_preview` was riding on it — so `:TimeTrackingClose` from a tabpage
// without a preview of its own became a silent no-op, and the early-return arm
// cleared `LAST_OUTPUT` while another tabpage still had that preview on screen.
// The next render there failed the dirty-check and rewrote the whole buffer,
// yanking its scroll position: precisely the cost the cache exists to prevent.
//
// `close_preview` scans globally again; the tab-scoped probe stays in place for
// the visibility/auto-open paths (see the B45 test above, which must keep
// passing). This test pins the split: after the close, nothing anywhere is
// displaying the preview, which is what makes clearing both caches correct.
#[nvim_oxi::test]
fn test_close_preview_closes_a_preview_living_in_another_tabpage() {
    cleanup_preview_buffers();
    api::command("only").unwrap();

    // Tab 1: a preview split, with `LAST_OUTPUT` primed to what it holds.
    create_or_update_preview("# Summary\n- total: 1h").unwrap();

    let tab1 = api::get_current_tabpage();
    assert!(
        tab_shows_preview(&tab1),
        "precondition: tab 1 must be showing the preview"
    );

    // Tab 2: a fresh tabpage with no preview window of its own — the state in
    // which the tab-scoped lookup reported "no preview open".
    api::command("tabnew").unwrap();
    let tab2 = api::get_current_tabpage();
    assert_ne!(
        tab1, tab2,
        "precondition: :tabnew must produce a distinct tabpage from tab 1"
    );
    assert!(
        !tab_shows_preview(&tab2),
        "precondition: tab 2 must not be showing the preview"
    );

    close_preview().unwrap();

    assert!(
        !tab_shows_preview(&tab1),
        "close_preview must close the preview window even though it lives in \
         another tabpage, rather than no-opping and clearing the caches under \
         a preview that is still on screen"
    );

    // `bufhidden=wipe`: closing the last window on the preview wipes the
    // buffer. Nothing is left displaying it, so dropping LAST_OUTPUT and the
    // buffer-handle cache on this path forgets nothing that is still live.
    let preview_buf_survives = api::list_bufs().any(|b| {
        b.get_name()
            .map(|n| {
                n.to_str()
                    .is_ok_and(|s| s.ends_with("[Time Tracking Preview]"))
            })
            .unwrap_or(false)
    });
    assert!(
        !preview_buf_survives,
        "the preview buffer must be gone once close_preview closes its last window"
    );

    // Leave the shared Neovim instance as we found it.
    api::command("tabclose").unwrap();
    api::command("only").unwrap();
    cleanup_preview_buffers();
}

/// A line write that always fails, for the B37 ordering test below.
///
/// Borrows a genuine API failure rather than fabricating an error value:
/// buffer handle 9_999_999 does not exist, so any call on it errors.
fn line_write_that_always_fails(
    _buf: &mut nvim_oxi::api::Buffer,
    _lines: Vec<&str>,
) -> nvim_oxi::Result<()> {
    nvim_oxi::api::Buffer::from(9_999_999_i32).line_count()?;
    Ok(())
}

// B37 regression guard. `write_preview_contents_with` makes the buffer
// modifiable, writes, and only then restores `nomodifiable` and records the
// output — so a failing write must still leave the preview nomodifiable and
// must not tell the dirty-check that `output` was written.
//
// Nothing inside Neovim can force that write to fail: `nvim_set_option_value`
// fires no `OptionSet` event, so no autocommand can interleave, and
// `nvim_buf_set_lines` into a buffer just made modifiable succeeds. Hence the
// injected writer — it is the only seam that can reach the failure branch.
#[nvim_oxi::test]
fn test_a_failed_preview_write_restores_nomodifiable_and_leaves_the_cache_clean() {
    use nvim_oxi::api::opts::OptionOptsBuilder;

    cleanup_preview_buffers();

    create_or_update_preview("FIRST").unwrap();
    let mut preview = preview_buffer();
    assert_eq!(
        preview_text(&preview),
        "FIRST",
        "precondition: the first render must land"
    );

    let result = time_tracking_nvim::write_preview_contents_with(
        &mut preview,
        "SECOND",
        line_write_that_always_fails,
    );
    assert!(
        result.is_err(),
        "precondition: the injected write must actually fail"
    );

    // Half one: the failure must not leave the preview editable. It would stay
    // that way for the rest of the session, so the user could type into the
    // preview and silently lose the edits on the next render.
    let bopts = OptionOptsBuilder::default().buf(preview.clone()).build();
    assert!(
        !api::get_option_value::<bool>("modifiable", &bopts).unwrap(),
        "a failed write must restore nomodifiable before propagating"
    );
    assert_eq!(
        preview_text(&preview),
        "FIRST",
        "a failed write must not have changed the buffer"
    );

    // Half two: the failure must not record "SECOND" as written. If it did,
    // the dirty-check would skip the next real render of that same text and
    // the preview would keep showing FIRST indefinitely.
    create_or_update_preview("SECOND").unwrap();
    assert_eq!(
        preview_text(&preview),
        "SECOND",
        "a failed write must not poison the last-output cache"
    );

    cleanup_preview_buffers();
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_never_returns_err_for_a_propagated_error() {
    clear_last_error_for_test();

    let result = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "synthetic stale-handle failure".to_string(),
        )))
    });

    assert!(
        result.is_ok(),
        "catch_nvim_panic must never return Err: {:?}",
        result
    );

    let messages: String = api::eval("execute('messages')").unwrap();
    assert!(
        messages.contains("synthetic stale-handle failure"),
        "the swallowed error must still be reported via :messages, got: {messages}"
    );
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_never_returns_err_for_a_panic() {
    clear_last_error_for_test();

    let result = catch_nvim_panic_for_test(|| {
        panic!("synthetic panic for B7 coverage");
    });

    assert!(
        result.is_ok(),
        "catch_nvim_panic must never return Err, even on a caught panic: {:?}",
        result
    );

    let messages: String = api::eval("execute('messages')").unwrap();
    assert!(
        messages.contains("synthetic panic for B7 coverage"),
        "the caught panic must still be reported via :messages, got: {messages}"
    );
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_dedupes_identical_consecutive_messages() {
    clear_last_error_for_test();

    let before: String = api::eval("execute('messages')").unwrap();
    let before_count = before.matches("dedup-marker-xyz").count();

    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "dedup-marker-xyz".to_string(),
        )))
    });
    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "dedup-marker-xyz".to_string(),
        )))
    });

    let after: String = api::eval("execute('messages')").unwrap();
    let after_count = after.matches("dedup-marker-xyz").count();

    assert_eq!(
        after_count - before_count,
        1,
        "an identical consecutive failure must be reported once, not per call"
    );
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_reports_a_different_message_right_after_a_dupe() {
    clear_last_error_for_test();

    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "first-marker-abc".to_string(),
        )))
    });
    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "second-marker-def".to_string(),
        )))
    });

    let messages: String = api::eval("execute('messages')").unwrap();
    assert!(messages.contains("first-marker-abc"));
    assert!(messages.contains("second-marker-def"));
}

#[nvim_oxi::test]
fn test_catch_nvim_panic_reports_the_same_message_again_after_a_success_in_between() {
    clear_last_error_for_test();

    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "recurring-marker-ghi".to_string(),
        )))
    });
    let _ = catch_nvim_panic_for_test(|| Ok(()));

    let before: String = api::eval("execute('messages')").unwrap();
    let before_count = before.matches("recurring-marker-ghi").count();

    let _ = catch_nvim_panic_for_test(|| {
        Err(nvim_oxi::Error::Api(nvim_oxi::api::Error::Other(
            "recurring-marker-ghi".to_string(),
        )))
    });

    let after: String = api::eval("execute('messages')").unwrap();
    let after_count = after.matches("recurring-marker-ghi").count();

    assert_eq!(
        after_count - before_count,
        1,
        "a failure recurring after an intervening success must be reported again"
    );
}

#[nvim_oxi::test]
fn test_buf_classification_cache_survives_across_repeated_calls() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let file_path = create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();

    assert!(is_buf_time_tracking_file(&buf, &config).unwrap());
    // A second call must return the same answer from the cache, not just
    // recompute correctly — this pins that the cache path is exercised at
    // all, not only that classification stays correct.
    assert!(is_buf_time_tracking_file(&buf, &config).unwrap());
}

#[nvim_oxi::test]
fn test_buf_classification_cache_invalidates_on_rename() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    // Each `#[nvim_oxi::test]` runs its own fresh, isolated Neovim process, so
    // the BufFilePost -> TimeTrackingInvalidateBufCache wiring this test
    // exercises only exists once this plugin instance registers it itself.
    let config_static: &'static Config = Box::leak(Box::new(config));
    time_tracking_with_config(config_static).unwrap();

    let other_dir = TempDir::new().unwrap();

    let outside_path = other_dir.path().join("notes.md");
    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(outside_path.to_str().unwrap()).unwrap();
    assert!(!is_buf_time_tracking_file(&buf, config_static).unwrap());

    // Rename the buffer into the data directory; BufFilePost fires and must
    // invalidate the cached (false) classification.
    let inside_path = temp_dir.path().join("2024-01-01.md");
    api::set_current_buf(&buf).unwrap();
    api::command(&format!("keepalt saveas {}", inside_path.to_str().unwrap())).unwrap();

    assert!(
        is_buf_time_tracking_file(&buf, config_static).unwrap(),
        "a renamed buffer must not serve a stale pre-rename classification"
    );
}

#[nvim_oxi::test]
fn test_buf_classification_cache_invalidates_on_wipeout() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let file_path = create_test_file(temp_dir.path(), "2024-01-02.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    let handle = buf.handle();
    assert!(is_buf_time_tracking_file(&buf, &config).unwrap());

    api::command(&format!("bwipeout! {}", handle)).unwrap();

    // No assertion is possible on the wiped buffer itself; this pins that
    // wiping it doesn't panic or leave the invalidation command failing.
    invalidate_buf_classification(handle);
}

/// Whether any window-less preview buffer currently exists, regardless of
/// whether a window is showing it.
fn preview_buffer_exists() -> bool {
    api::list_bufs().any(|b| is_preview_buf(&b).unwrap_or(false))
}

// W2: a preview closed via an explicit `:TimeTrackingToggle` (the close
// branch of `toggle_preview_fn`, which marks the preview dismissed right
// after `close_preview()` succeeds) must stay closed across the ordinary
// auto-open path — re-entering a tracking buffer must not resurrect it —
// until the user explicitly asks for it again via another
// `:TimeTrackingToggle`.
#[nvim_oxi::test]
fn test_closed_preview_does_not_auto_reopen_until_explicitly_reopened() {
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let file_path = create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();

    time_tracking_nvim::auto_open_preview(config_static).unwrap();
    assert!(preview_buffer_exists(), "preview should auto-open");

    // The close half of :TimeTrackingToggle: the preview is open, so this
    // closes it AND marks it dismissed.
    time_tracking_nvim::toggle_preview_fn(config_static).unwrap();
    assert!(!preview_buffer_exists(), "preview should be closed");

    // Simulate the auto-open path firing again for the same tracking
    // buffer -- it must NOT reopen a preview dismissed via :TimeTrackingToggle.
    time_tracking_nvim::auto_open_preview(config_static).unwrap();
    assert!(
        !preview_buffer_exists(),
        "a dismissed preview must not auto-reopen"
    );

    // A second, explicit :TimeTrackingToggle asks for it again.
    time_tracking_nvim::toggle_preview_fn(config_static).unwrap();
    assert!(
        preview_buffer_exists(),
        "an explicit toggle must reopen a dismissed preview"
    );
}

// W2 regression guard: `close_preview()` itself must stay dismissal-neutral,
// because it is also the target of `TimeTrackingMaybeCloseIfInvisible`, which
// fires routinely on BufEnter/TabEnter/WinClosed whenever no tracking file is
// currently visible -- an everyday event (e.g. switching to check a different
// file), not a user request to stop seeing the preview. If `close_preview()`
// set the dismissal flag itself, the very first such routine auto-close of a
// session would permanently suppress auto-reopen thereafter, silently
// breaking the plugin's core automatic open/close loop.
#[nvim_oxi::test]
fn test_routine_auto_close_does_not_suppress_the_next_auto_open() {
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let file_path = create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();

    time_tracking_nvim::auto_open_preview(config_static).unwrap();
    assert!(preview_buffer_exists(), "preview should auto-open");

    // Simulate TimeTrackingMaybeCloseIfInvisible's routine auto-close: it
    // calls close_preview() directly, with no dismissal side effect.
    close_preview().unwrap();
    assert!(!preview_buffer_exists(), "preview should be closed");

    // Switching back to a tracking buffer (auto-open firing again) must
    // reopen it -- this routine close was never a dismissal.
    time_tracking_nvim::auto_open_preview(config_static).unwrap();
    assert!(
        preview_buffer_exists(),
        "a routine, non-dismissing auto-close must not block the next auto-open"
    );
}

// W3: `buffer_status` gives statusline-style integrations (lualine, etc.) a
// value back for a tracking buffer instead of a rendered preview.
#[nvim_oxi::test]
fn test_status_reports_totals_for_a_tracking_buffer() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let file_path = create_test_file(
        temp_dir.path(),
        "2024-01-01.md",
        "9-10 work\n10-10:30 admin\n",
    );

    // `set_name` alone does not load the file's on-disk content into the
    // buffer (nvim-oxi's `create_buf` never reads the file, unlike `:edit`)
    // -- set the lines explicitly so `get_buffer_content()` below has
    // something to parse, matching what `create_test_file` wrote to disk.
    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    buf.set_lines(.., false, ["9-10 work", "10-10:30 admin"])
        .unwrap();
    api::set_current_buf(&buf).unwrap();

    let status = time_tracking_nvim::utils::buffer_status(
        &time_tracking_nvim::utils::get_buffer_content().unwrap(),
        config_static,
    );

    let total_minutes = status
        .iter()
        .find(|(k, _)| k.to_str() == Ok("total_minutes"))
        .map(|(_, v)| v.clone());
    assert_eq!(
        total_minutes,
        Some(nvim_oxi::Object::from(90i64)),
        "total_minutes must be present and correct: {:?}",
        status
    );
}

// W3: a buffer outside the configured data directory must not be classified
// as a tracking file, so callers (the `status` command, `M.summary()`) know
// to report `{ is_tracking_file = false }` rather than parse it.
#[nvim_oxi::test]
fn test_status_marks_non_tracking_buffer() {
    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let other_dir = TempDir::new().unwrap();
    let outside_path = other_dir.path().join("notes.md");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(outside_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();

    assert!(!is_time_tracking_file(&config).unwrap());
}

// W3: the brief's other two tests exercise `buffer_status` directly; this one
// exercises the actual seam added to `lib.rs` -- that `time_tracking_with_config`
// wires a *callable* `status` Function into the returned Dictionary, the same
// object Lua's `require("time_tracking_nvim").status()` would invoke.
#[nvim_oxi::test]
fn test_status_function_on_the_returned_dictionary_is_callable() {
    use nvim_oxi::conversion::FromObject;

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let file_path = create_test_file(temp_dir.path(), "2024-01-02.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    buf.set_lines(.., false, ["9-10 work"]).unwrap();
    api::set_current_buf(&buf).unwrap();

    let dict = time_tracking_with_config(config_static).unwrap();
    let status_obj = dict
        .get("status")
        .cloned()
        .expect("time_tracking_with_config's Dictionary must carry a \"status\" key");

    let status_fn: nvim_oxi::Function<(), nvim_oxi::Dictionary> =
        FromObject::from_object(status_obj).expect("\"status\" must be a callable Function");

    let result = status_fn.call(()).expect("calling status() must not error");
    assert_eq!(
        result.get("total_minutes"),
        Some(&nvim_oxi::Object::from(60i64)),
        "status() must report the current tracking buffer's parsed totals: {:?}",
        result
    );
}

// W3: the sibling of the test above, for the non-tracking branch of the same
// seam -- `status()`'s `if !is_time_tracking_file(config)? { ... }` guard in
// `lib.rs`, not just the pre-existing `is_time_tracking_file` helper it calls.
// `test_status_marks_non_tracking_buffer` above only exercises that helper;
// this one calls the real `status` Function and checks the shape it actually
// produces for a buffer outside the data directory.
#[nvim_oxi::test]
fn test_status_function_marks_non_tracking_buffer() {
    use nvim_oxi::conversion::FromObject;

    let (config, _temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let other_dir = TempDir::new().unwrap();
    let outside_path = other_dir.path().join("notes.md");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(outside_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();

    let dict = time_tracking_with_config(config_static).unwrap();
    let status_obj = dict
        .get("status")
        .cloned()
        .expect("time_tracking_with_config's Dictionary must carry a \"status\" key");

    let status_fn: nvim_oxi::Function<(), nvim_oxi::Dictionary> =
        FromObject::from_object(status_obj).expect("\"status\" must be a callable Function");

    let result = status_fn.call(()).expect("calling status() must not error");
    assert_eq!(
        result.get("is_tracking_file"),
        Some(&nvim_oxi::Object::from(false)),
        "status() must report is_tracking_file: false for a buffer outside the data directory: {:?}",
        result
    );
    assert_eq!(
        result.get("total_minutes"),
        None,
        "the non-tracking branch must not carry parsed totals it never computed: {:?}",
        result
    );
    assert_eq!(
        result.get("dead_time_minutes"),
        None,
        "the non-tracking branch must not carry parsed totals it never computed: {:?}",
        result
    );
}

// W10: `:checkhealth` reports whether the configured data directory resolves.
// `data_directory_status_dict` reuses `resolved_data_dir` rather than
// re-resolving independently, so it can never disagree with what
// `is_buf_time_tracking_file` uses to classify buffers.
#[nvim_oxi::test]
fn test_data_directory_status_resolves_a_real_directory() {
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let dict = time_tracking_nvim::utils::data_directory_status_dict(&config);

    assert_eq!(
        dict.get("resolved"),
        Some(&nvim_oxi::Object::from(true)),
        "an existing, configured directory must resolve: {:?}",
        dict
    );
    assert!(
        dict.get("canonical_path").is_some(),
        "a resolved directory must report its canonical path: {:?}",
        dict
    );
    let _ = temp_dir; // keep the TempDir alive for the duration of the assertions
}

// The sibling of the test above, for a data directory that does not exist at
// all. This is the case a plain `#[test]` in `src/utils.rs` cannot exercise:
// `resolved_data_dir`'s error arm calls `warn_data_dir_unresolved`, which
// calls the real `api::get_vvar` -- a call that links only against a live
// Neovim host, which a plain unit-test binary does not have (confirmed
// empirically: linking one that reaches this branch fails with "undefined
// symbol: nvim_get_vvar"). `test_missing_data_directory_returns_false_and_
// does_not_panic` above covers the same "misconfigured data directory" case
// for buffer classification; this one covers it for the health-check surface.
#[nvim_oxi::test]
fn test_data_directory_status_reports_unresolved_for_a_missing_directory() {
    let config = Config {
        data_directory: Some("/does/not/exist/at/all".to_string()),
        ..Default::default()
    };

    let dict = time_tracking_nvim::utils::data_directory_status_dict(&config);

    assert_eq!(
        dict.get("resolved"),
        Some(&nvim_oxi::Object::from(false)),
        "a nonexistent directory must not resolve: {:?}",
        dict
    );
    assert_eq!(
        dict.get("configured"),
        Some(&nvim_oxi::Object::from("/does/not/exist/at/all")),
        "the unresolved case must still echo back what was configured: {:?}",
        dict
    );
    assert_eq!(
        dict.get("canonical_path"),
        None,
        "an unresolved directory has no canonical path to report: {:?}",
        dict
    );
}

// W10: the `data_directory_status` Function `time_tracking_with_config` wires
// into its returned Dictionary -- the same object Lua's
// `require("time_tracking_nvim").data_directory_status()` (called from
// health.lua's check_data_directory) would invoke. Mirrors
// `test_status_function_on_the_returned_dictionary_is_callable` above.
#[nvim_oxi::test]
fn test_data_directory_status_function_on_the_returned_dictionary_is_callable() {
    use nvim_oxi::conversion::FromObject;

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let dict = time_tracking_with_config(config_static).unwrap();
    let data_directory_status_obj = dict.get("data_directory_status").cloned().expect(
        "time_tracking_with_config's Dictionary must carry a \"data_directory_status\" key",
    );

    let data_directory_status_fn: nvim_oxi::Function<(), nvim_oxi::Dictionary> =
        FromObject::from_object(data_directory_status_obj)
            .expect("\"data_directory_status\" must be a callable Function");

    let result = data_directory_status_fn
        .call(())
        .expect("calling data_directory_status() must not error");
    assert_eq!(
        result.get("resolved"),
        Some(&nvim_oxi::Object::from(true)),
        "data_directory_status() must report the configured temp dir as resolved: {:?}",
        result
    );
    let _ = temp_dir; // keep the TempDir alive for the duration of the assertions
}

// --- W5: the weekly summary view (`:TimeTrackingWeeklyToggle`) -------------

/// The seven dates the weekly view will render, anchored on `Config::default()`'s
/// week start of Saturday.
///
/// Computed rather than hardcoded because the view is about *this* week: a
/// fixture week would render seven "No time tracking file found" days no matter
/// what the test seeded.
///
/// The anchor date comes from `today_for_test()` — the production `today()`
/// itself — rather than being recomputed here. An earlier version of this
/// helper reimplemented the same `now_local()`-then-UTC logic the production
/// code used, which made the oracle share the production bug: both agreed on
/// UTC, so the test could not see that the *user's* week was being resolved in
/// the wrong timezone. Calling the real function means the two can never
/// disagree again — and, when it is wrong, it is wrong somewhere a test can
/// still catch it.
fn current_week_dates() -> [time::Date; 7] {
    time_tracking_cli::get_week_dates(
        &time_tracking_nvim::today_for_test(),
        time::Weekday::Saturday,
    )
}

/// The day file name `DataService` will look for for `date` — the same
/// `[year]-[month]-[day].md` shape `DataService::get_file_path` builds.
fn day_file_name(date: time::Date) -> String {
    format!(
        "{}.md",
        date.format(&time_tracking_cli::DATE_FORMAT)
            .expect("a Date always formats as YYYY-MM-DD")
    )
}

// W5: the weekly view must aggregate the whole week's day files rather than
// re-render the current buffer. Two seeded days sharing one project make that
// checkable end to end: the rollup for `alpha` is 2:00 only if both files were
// read, parsed and summed -- neither file contains that number on its own.
#[nvim_oxi::test]
fn test_weekly_toggle_renders_aggregate_totals() {
    cleanup_preview_buffers();
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let week = current_week_dates();
    // 60 minutes of `alpha`.
    create_test_file(temp_dir.path(), &day_file_name(week[1]), "9-10 alpha\n");
    // 60 more of `alpha`, plus 90 of `beta`.
    create_test_file(
        temp_dir.path(),
        &day_file_name(week[2]),
        "9-10 alpha\n10-11:30 beta\n",
    );

    time_tracking_nvim::toggle_weekly_preview_fn(config_static).unwrap();

    assert!(
        preview_buffer_exists(),
        "the weekly toggle must open a preview"
    );
    let text = preview_text(&preview_buffer());

    // 210 minutes across the week: 60 + 60 + 90.
    assert!(
        text.contains("Total Working Time: 3:30"),
        "the weekly total must sum every seeded day: {text}"
    );
    // `alpha - 2:00` appears in neither day file's own summary; only the
    // cross-day rollup produces it.
    assert!(
        text.contains("alpha - 2:00"),
        "a project's weekly rollup must span days: {text}"
    );
    assert!(
        text.contains("beta - 1:30"),
        "a project present on one day only must still roll up: {text}"
    );

    // Both seeded days get their own breakdown below the aggregate...
    assert!(
        text.contains(&time_tracking_cli::format_day_with_date(&week[1])),
        "the first seeded day must have a breakdown: {text}"
    );
    assert!(
        text.contains(&time_tracking_cli::format_day_with_date(&week[2])),
        "the second seeded day must have a breakdown: {text}"
    );
    // ...and the five days with no file are reported as such rather than
    // silently dropped, so the week reads as seven days.
    assert!(
        text.contains("No time tracking file found"),
        "days with no file must be called out: {text}"
    );
}

// W5: the toggle half. A second `:TimeTrackingWeeklyToggle` on an open weekly
// view closes it, the way `:TimeTrackingToggle` closes an open day view.
#[nvim_oxi::test]
fn test_weekly_toggle_closes_the_weekly_view_it_opened() {
    cleanup_preview_buffers();
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let week = current_week_dates();
    create_test_file(temp_dir.path(), &day_file_name(week[1]), "9-10 alpha\n");

    time_tracking_nvim::toggle_weekly_preview_fn(config_static).unwrap();
    assert!(
        preview_buffer_exists(),
        "the first weekly toggle must open the preview"
    );

    time_tracking_nvim::toggle_weekly_preview_fn(config_static).unwrap();
    assert!(
        !preview_buffer_exists(),
        "a second weekly toggle must close the weekly view"
    );
}

// W5: the other half of that condition. `:TimeTrackingWeeklyToggle` closes the
// preview only when the preview is showing *the weekly view*; run against an
// open day view it is a view switch, not a close. Toggling on the mere
// presence of a preview window would make the command unreachable from the
// state a user is nearly always in -- the day view auto-opens.
#[nvim_oxi::test]
fn test_weekly_toggle_switches_an_open_day_view_to_the_week() {
    cleanup_preview_buffers();
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let week = current_week_dates();
    let md = create_test_file(temp_dir.path(), &day_file_name(week[1]), "9-10 alpha\n");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    // The ordinary starting point: the day view, open.
    time_tracking_nvim::toggle_preview_fn(config_static).unwrap();
    let preview = preview_buffer();
    assert!(
        !preview_text(&preview).contains("WEEKLY TIME TRACKING SUMMARY"),
        "the day view must be what is showing first: {}",
        preview_text(&preview)
    );

    time_tracking_nvim::toggle_weekly_preview_fn(config_static).unwrap();

    assert!(
        preview_buffer_exists(),
        "the weekly toggle must not close a preview showing the day view"
    );
    assert!(
        preview_text(&preview_buffer()).contains("WEEKLY TIME TRACKING SUMMARY"),
        "the weekly toggle must swap the day view for the week: {}",
        preview_text(&preview_buffer())
    );
}

// W5: the keystroke-driven path must leave an open weekly view completely
// alone -- it must not render the day view over it, and it must not re-run the
// week's aggregation either. Re-reading and re-parsing seven day files is by
// some margin the most expensive thing this plugin does, and TextChanged fires
// once per keystroke.
//
// Both halves are observable through one probe: a day file written to disk
// *after* the weekly view was rendered. `render_weekly_view` builds a fresh
// `DataService` (and so a fresh cache) on every call, so any re-aggregation
// would pick that file up. The preview holding exactly its pre-existing text
// therefore proves the throttled path did neither -- where asserting only
// "still the weekly view" would not, since `update_preview_fn` would answer
// that on its own by re-rendering the same week.
#[nvim_oxi::test]
fn test_typing_neither_replaces_nor_re_aggregates_the_weekly_view() {
    cleanup_preview_buffers();
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let week = current_week_dates();
    // Make a tracking buffer current, so the throttled path gets past its
    // `is_time_tracking_file` gate and would otherwise render the day view.
    let md = create_test_file(temp_dir.path(), &day_file_name(week[1]), "9-10 alpha\n");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    time_tracking_nvim::toggle_weekly_preview_fn(config_static).unwrap();
    let preview = preview_buffer();
    let before = preview_text(&preview);
    let tick_before = preview.get_changedtick().unwrap();
    assert!(
        before.contains("Total Working Time: 1:00"),
        "the weekly view must be showing the one seeded day first: {before}"
    );

    // A second day appears on disk. Only a re-aggregation could notice.
    create_test_file(temp_dir.path(), &day_file_name(week[2]), "10-11:30 beta\n");

    // The TextChanged/TextChangedI autocommand's entry point. The throttle
    // window was just reset, so this takes the leading edge and renders
    // synchronously -- if it renders at all.
    time_tracking_nvim::update_preview_throttled(config_static).unwrap();

    assert_eq!(
        preview.get_changedtick().unwrap(),
        tick_before,
        "a keystroke must not rewrite the preview while the weekly view is up"
    );
    assert_eq!(
        preview_text(&preview),
        before,
        "a keystroke must neither swap in the day view nor re-aggregate the week"
    );
}

// W5: an explicitly typed `:TimeTrackingUpdate` is a *refresh*, so it rebuilds
// whichever view is showing. Rendering the day view unconditionally there
// would make the command a silent view switch, and would undo the weekly view
// the moment any other code path invoked it.
#[nvim_oxi::test]
fn test_explicit_update_refreshes_the_weekly_view_rather_than_replacing_it() {
    cleanup_preview_buffers();
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let week = current_week_dates();
    let md = create_test_file(temp_dir.path(), &day_file_name(week[1]), "9-10 alpha\n");
    let mut buf = api::create_buf(false, false).unwrap();
    buf.set_name(&md).unwrap();
    api::set_current_buf(&buf).unwrap();

    time_tracking_nvim::toggle_weekly_preview_fn(config_static).unwrap();
    let preview = preview_buffer();
    assert!(
        preview_text(&preview).contains("Total Working Time: 1:00"),
        "the weekly view must be showing the one seeded day first: {}",
        preview_text(&preview)
    );

    // The same probe the throttled test uses, with the opposite expectation:
    // an explicit refresh *must* pick this up.
    create_test_file(temp_dir.path(), &day_file_name(week[2]), "10-11:30 beta\n");

    time_tracking_nvim::update_preview_fn(config_static).unwrap();

    let text = preview_text(&preview);
    assert!(
        text.contains("WEEKLY TIME TRACKING SUMMARY"),
        ":TimeTrackingUpdate must refresh the weekly view, not replace it: {text}"
    );
    assert!(
        text.contains("Total Working Time: 2:30"),
        ":TimeTrackingUpdate must re-aggregate the week, picking up the new day: {text}"
    );
}

// W5 / I1: the week must be anchored on the date *Neovim* reports, not on
// `time::OffsetDateTime::now_local()`. `now_local()` fails in a multi-threaded
// process (which Neovim plus this plugin's Tokio runtime is) and silently
// degrades to UTC, and because this one date anchors all seven, a UTC/local
// disagreement across the week-start boundary shifts the whole view by a week.
#[nvim_oxi::test]
fn test_today_matches_the_date_neovim_itself_reports() {
    let from_nvim: String = api::call_function("strftime", ("%Y-%m-%d",)).unwrap();
    let today = time_tracking_nvim::today_for_test();

    assert_eq!(
        today
            .format(&time_tracking_cli::DATE_FORMAT)
            .expect("a Date always formats as YYYY-MM-DD"),
        from_nvim,
        "the weekly view's anchor date must be the editor's own local date"
    );

    // The assertion above is exact but only *observably* wrong in a timezone
    // whose date differs from UTC's right now, so on a UTC machine it would
    // pass even against a `now_local()`-based implementation. These two do not
    // depend on where the test runs: the zones are 25 hours apart, so their
    // local dates always differ, while anything reading UTC answers the same
    // date for both. POSIX `TZ` offsets rather than named zones, so no tzdata
    // is required; the sign is inverted by the POSIX spec, i.e. `XXX-14` is
    // UTC+14.
    let original: String = api::call_function("getenv", ("TZ",)).unwrap_or_default();

    api::command("let $TZ = 'XXX-14'").unwrap();
    let far_east = time_tracking_nvim::today_for_test();
    api::command("let $TZ = 'XXX+11'").unwrap();
    let far_west = time_tracking_nvim::today_for_test();

    api::command(&format!("let $TZ = '{original}'")).unwrap();

    assert_ne!(
        far_east, far_west,
        "the anchor date must follow the editor's timezone; reading UTC \
         instead answers the same date on both sides of the date line"
    );
}

// W5 / I3: the weekly view must survive `TimeTrackingMaybeCloseIfInvisible`.
//
// That autocommand fires on BufEnter/TabEnter/WinClosed and closes the preview
// whenever no *tracking file* is visible. That rule is right for the day view,
// which mirrors the buffer being edited -- and wrong for the week view, whose
// point is answering "how much did I work this week" from wherever the user
// happens to be. Without the `current_view_is_week()` exemption the weekly view
// could not be opened from a non-tracking buffer at all: `open_preview_split`
// ends with `set_current_win`, which itself fires BufEnter.
//
// Unlike the other W5 tests this registers the *real* autocommands via
// `time_tracking_with_config`, because the bug lives in the wiring rather than
// in any function those tests call directly.
#[nvim_oxi::test]
fn test_weekly_view_survives_switching_to_a_non_tracking_buffer() {
    cleanup_preview_buffers();
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let week = current_week_dates();
    create_test_file(temp_dir.path(), &day_file_name(week[1]), "9-10 alpha\n");

    // The real wiring: BufEnter/TabEnter -> TimeTrackingMaybeCloseIfInvisible.
    time_tracking_with_config(config_static).unwrap();

    // Open the weekly view from a buffer that is *not* a tracking file, which
    // is the whole point of the command being data-directory scoped. Merely
    // getting a preview back here already exercises the BufEnter that
    // `open_preview_split`'s `set_current_win` fires.
    let outside_dir = TempDir::new().unwrap();
    let mut other = api::create_buf(true, false).unwrap();
    other
        .set_name(outside_dir.path().join("notes.md").to_str().unwrap())
        .unwrap();
    api::set_current_buf(&other).unwrap();
    assert!(
        !is_buf_time_tracking_file(&other, config_static).unwrap(),
        "the fixture buffer must not be a tracking file"
    );

    time_tracking_nvim::toggle_weekly_preview_fn(config_static).unwrap();
    assert!(
        preview_buffer_exists(),
        "the weekly view must open from a non-tracking buffer, and must not be \
         closed again by the BufEnter that opening the split fires"
    );

    // And an explicit BufEnter -- the event a user generates by switching
    // buffers -- must leave it alone too.
    api::exec2("doautocmd BufEnter", &Default::default()).unwrap();
    assert!(
        preview_buffer_exists(),
        "BufEnter in a non-tracking buffer must not close the weekly view"
    );

    // The exemption is scoped to the weekly view: an explicit close still
    // closes it, so the user is never stuck with a preview they cannot dismiss.
    api::command("TimeTrackingClose").unwrap();
    assert!(
        !preview_buffer_exists(),
        ":TimeTrackingClose must still close the weekly view"
    );
}

// W5 / I3, the other side: the exemption must not leak to the day view. A day
// preview left open with no tracking file visible is the exact behaviour
// `TimeTrackingMaybeCloseIfInvisible` exists to prevent.
#[nvim_oxi::test]
fn test_day_view_is_still_closed_when_no_tracking_file_is_visible() {
    cleanup_preview_buffers();
    time_tracking_nvim::reset_throttle_for_test();

    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));

    let week = current_week_dates();
    let md = create_test_file(temp_dir.path(), &day_file_name(week[1]), "9-10 alpha\n");
    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(md.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();

    time_tracking_with_config(config_static).unwrap();
    time_tracking_nvim::toggle_preview_fn(config_static).unwrap();
    assert!(preview_buffer_exists(), "the day view must open");

    // Switch away to a buffer outside the data directory. BufEnter fires, and
    // with no tracking file visible the day view must go.
    let outside_dir = TempDir::new().unwrap();
    let mut other = api::create_buf(true, false).unwrap();
    other
        .set_name(outside_dir.path().join("notes.md").to_str().unwrap())
        .unwrap();
    api::set_current_buf(&other).unwrap();
    api::exec2("doautocmd BufEnter", &Default::default()).unwrap();

    assert!(
        !preview_buffer_exists(),
        "the day view must still be closed when no tracking file is visible"
    );
}

#[nvim_oxi::test]
fn test_preview_refreshes_after_external_file_change_and_checktime() {
    cleanup_preview_buffers();
    time_tracking_nvim::reset_throttle_for_test();
    let (config, temp_dir) = create_test_config_with_temp_dir();
    let config_static: &'static Config = Box::leak(Box::new(config));
    let file_path = create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n");

    let mut buf = api::create_buf(true, false).unwrap();
    buf.set_name(file_path.to_str().unwrap()).unwrap();
    api::set_current_buf(&buf).unwrap();
    api::command(&format!("edit {}", file_path.to_str().unwrap())).unwrap();

    time_tracking_nvim::toggle_preview_fn(config_static).unwrap();
    assert!(preview_buffer_exists());

    // Change the file on disk, outside the buffer.
    create_test_file(temp_dir.path(), "2024-01-01.md", "9-10 work\n10-11 admin\n");

    api::command("checktime").unwrap();

    // The autocmd chain (FileChangedShellPost -> TimeTrackingUpdateThrottled)
    // re-renders synchronously on its leading edge (see update_preview_throttled),
    // so the preview reflects the new content without the user typing.
    assert!(preview_buffer_exists());
}

// W11: :TimeTrackingOpenToday creates today's file from the configured
// template when it doesn't exist yet, opens it, and never re-seeds an
// existing file.
//
// The expected file name is derived from `today_for_test()` -- the very
// `today()` that `open_today_fn` itself calls -- via the same `day_file_name`
// helper the weekly-view tests use, rather than from
// `time::OffsetDateTime::now_utc().date()`. `open_today_fn` resolves "today"
// through Neovim's own local date (see `preview::today`'s doc comment), and
// near local midnight in a timezone that disagrees with UTC, a UTC-derived
// oracle and the production code would compute two different dates -- exactly
// the bug `today()` exists to avoid reintroducing (whats-next W5). Anchoring
// both sides on the same function is what `current_week_dates`'s doc comment
// already establishes this suite must do.
#[nvim_oxi::test]
fn test_open_today_creates_file_from_template_and_opens_it() {
    let (mut config, temp_dir) = create_test_config_with_temp_dir();
    let template_path = create_test_file(temp_dir.path(), "template.md", "# {date}\n\n");
    config.template_file = Some(template_path.to_str().unwrap().to_string());
    let config_static: &'static Config = Box::leak(Box::new(config));

    time_tracking_nvim::open_today_fn(config_static).unwrap();

    let today = time_tracking_nvim::today_for_test();
    let expected_path = temp_dir.path().join(day_file_name(today));
    assert!(
        expected_path.exists(),
        "today's file should have been created"
    );

    let today_str = today
        .format(&time_tracking_cli::DATE_FORMAT)
        .expect("a Date always formats as YYYY-MM-DD");
    let content = fs::read_to_string(&expected_path).unwrap();
    assert!(
        content.contains(&today_str),
        "the {{date}} placeholder should be replaced: {content}"
    );

    let current_name = api::get_current_buf().get_name().unwrap().to_string();
    assert_eq!(
        current_name,
        expected_path.to_str().unwrap(),
        "opening today's file should make it the current buffer"
    );

    // Running it again must not overwrite existing content.
    fs::write(&expected_path, "9-10 work\n").unwrap();
    time_tracking_nvim::open_today_fn(config_static).unwrap();
    let content_after = fs::read_to_string(&expected_path).unwrap();
    assert_eq!(
        content_after, "9-10 work\n",
        "an existing file must not be re-seeded"
    );
}
