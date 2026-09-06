local H = require("harness")
local tt = require("time-tracking-nvim")

-- Tests for :TimeTrackingDownload and :TimeTrackingVersion (whats-next W7):
-- the first two Lua-registered `vim.api.nvim_create_user_command` calls in
-- this plugin. Every other `:TimeTracking*` command is registered from the
-- Rust side once the native module loads and initializes; these two are
-- registered directly in M.setup(), unconditionally and before the
-- binary-exists/load-native ladder, precisely so they still exist when that
-- ladder fails to load the native module -- which is exactly the
-- troubleshooting scenario they exist to help with.
--
-- WHY THE NATIVE MODULE IS STUBBED
--
-- This repo's checked-out lua/time_tracking_nvim.so is a real compiled
-- library, and its .version file need not match PLUGIN_VERSION on any given
-- machine -- calling the real M.setup() completely unstubbed can select the
-- "needs_update, auto_update disabled" branch, which does not return early
-- and falls through to require() the real native module for real.
-- spec_setup.lua avoids that (see its run_setup doc comment: "the real
-- library on this machine is never dlopen'd"); this file follows the same
-- policy. Every case here stubs package.preload/package.loaded for the
-- native module name so requiring it always fails in a controlled way, and
-- stubs vim.fn.filereadable/readfile so classify_binary_state sees a binary
-- already at the plugin's own version -- which sends setup() straight to the
-- load attempt without ever starting a download (so vim.system need not be
-- stubbed at all: no case here touches the network).

local NATIVE = "time_tracking_nvim"

-- Calls tt.setup(opts) with the native module and the binary/version files
-- stubbed, runs `fn`, then restores every stub -- including package.cpath
-- (add_to_cpath mutates it) and tt.config -- before returning. This suite's
-- other spec files share this same Neovim process, so nothing here may leak.
--
-- The stubbed require() always fails (simulating "native module fails to
-- load"), which is the scenario these two commands specifically must survive.
local function with_native_load_failing(opts, fn)
  local internal = tt._internal
  local binary_path = internal.get_binary_path()
  local version_path = internal.get_version_file_path()

  local saved = {
    filereadable = vim.fn.filereadable,
    readfile = vim.fn.readfile,
    echo = vim.api.nvim_echo,
    cpath = package.cpath,
    config = tt.config,
    preload = package.preload[NATIVE],
    loaded = package.loaded[NATIVE],
  }

  -- Binary present, version already matching PLUGIN_VERSION: classify_binary_state
  -- reports no update needed, so setup()'s ladder falls straight through to
  -- the load attempt below rather than starting a download.
  vim.fn.filereadable = function(path)
    if path == binary_path or path == version_path then
      return 1
    end
    return saved.filereadable(path)
  end

  vim.fn.readfile = function(path, ...)
    if path == version_path then
      return { internal.PLUGIN_VERSION }
    end
    return saved.readfile(path, ...)
  end

  -- Swallowed: setup()'s own messages are not under test here and would
  -- otherwise scroll through the test output.
  vim.api.nvim_echo = function() end

  -- Beats the cpath searcher, so the real library on this machine is never
  -- dlopen'd (same technique spec_setup.lua uses). The stub always raises,
  -- standing in for a dlopen failure -- load_native's pcall is what turns
  -- that into the "load_failed" status setup() reports and returns on.
  package.loaded[NATIVE] = nil
  package.preload[NATIVE] = function()
    error("stub: cannot open shared object")
  end

  local ok, err = pcall(function()
    tt.setup(opts)
    fn()
  end)

  vim.fn.filereadable = saved.filereadable
  vim.fn.readfile = saved.readfile
  vim.api.nvim_echo = saved.echo
  package.cpath = saved.cpath
  tt.config = saved.config
  package.preload[NATIVE] = saved.preload
  package.loaded[NATIVE] = saved.loaded

  if not ok then
    error(err, 0)
  end
end

H.describe("TimeTracking* Lua-registered commands", function()
  H.it("registers TimeTrackingDownload and TimeTrackingVersion even when the native module fails to load", function()
    with_native_load_failing({ auto_download = false, auto_update = false }, function()
      H.eq(vim.fn.exists(":TimeTrackingDownload"), 2, "TimeTrackingDownload must be registered")
      H.eq(vim.fn.exists(":TimeTrackingVersion"), 2, "TimeTrackingVersion must be registered")
    end)
  end)

  H.it("TimeTrackingDownload calls through to M.download()", function()
    with_native_load_failing({ auto_download = false, auto_update = false }, function()
      local called = false
      local orig = tt.download
      tt.download = function()
        called = true
      end

      local ok, err = pcall(vim.cmd, "TimeTrackingDownload")
      tt.download = orig

      H.ok(ok, "TimeTrackingDownload errored: " .. tostring(err))
      H.ok(called, "TimeTrackingDownload must call M.download()")
    end)
  end)

  H.it("TimeTrackingVersion calls through to M.version_info()", function()
    with_native_load_failing({ auto_download = false, auto_update = false }, function()
      local called = false
      local orig = tt.version_info
      tt.version_info = function()
        called = true
      end

      local ok, err = pcall(vim.cmd, "TimeTrackingVersion")
      tt.version_info = orig

      H.ok(ok, "TimeTrackingVersion errored: " .. tostring(err))
      H.ok(called, "TimeTrackingVersion must call M.version_info()")
    end)
  end)
end)

return H
