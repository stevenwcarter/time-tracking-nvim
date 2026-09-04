local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal

-- Characterization tests for M.setup's branch ladder.
--
-- These pin what setup() does *today*, ahead of the three refactors that fold
-- its twin download/update branches together, decompose it, and flatten
-- download_binary's callback pyramid. Where today's behaviour is wrong it is
-- still pinned as it stands; see the B48 case, and the two asymmetries
-- flagged further down. A characterization test that encodes the behaviour we
-- *wish* the code had is not a net, it is a trap: it fails the moment a
-- correct refactor preserves what the code actually does.
--
-- Two rules keep these assertions durable:
--
--   * Assert on which stub fired and with what, never on echo text. The
--     refactors ahead deliberately reword every message, so wording is churn.
--     Message *count* is still fair game where silence is the behaviour under
--     test.
--
--   * download_binary is a `local` in init.lua with no test seam, and this
--     spec may not touch lua/. So "a download was attempted" is pinned one
--     level deeper, at the curl argv download_binary hands to vim.system.
--     The stub never invokes the callback it is given, so download_binary's
--     asynchronous tail never runs and what these tests measure is exactly
--     setup()'s synchronous ladder.

local NATIVE = "time_tracking_nvim"
local API_PREFIX = "https://api.github.com/"

-- The release setup() asks for when it downloads: its own version, never
-- /latest. Built from PLUGIN_VERSION so a version bump does not break this.
local WANTED_RELEASE = "/releases/tags/v" .. internal.PLUGIN_VERSION

local function ends_with(s, suffix)
  return type(s) == "string" and s:sub(-#suffix) == suffix
end

-- The curl invocations that hit the release API, i.e. the ones that mean
-- download_binary was entered. curl_fail_flag()'s one-off
-- `curl --fail-with-body --version` capability probe is not one of them, and
-- whether it shows up at all depends on whether an earlier spec already
-- warmed its cache, so it must never be counted.
local function api_calls(rec)
  local urls = {}
  for _, argv in ipairs(rec.curl) do
    local url = argv[#argv]
    if type(url) == "string" and url:sub(1, #API_PREFIX) == API_PREFIX then
      table.insert(urls, url)
    end
  end
  return urls
end

-- Runs M.setup with every call it makes to the outside world stubbed, and
-- reports what those stubs saw.
--
-- world.binary_exists   the native library is on disk
-- world.binary_version  contents of its .version file (nil = no version file,
--                       which setup() reads as "old binary, needs updating")
-- world.executables     name -> boolean for vim.fn.executable; the default has
--                       curl, tar and unzip all present
-- world.uname           override uv.os_uname(), for the unsupported-platform
--                       guard only; a *different supported* platform would
--                       desynchronise the paths captured just below
--
-- Returns { curl = {argv, ...}, native_loads = n, echoes = n }.
local function run_setup(world, opts)
  -- Captured before any stub is installed, so they are the paths setup() will
  -- compute for this machine.
  local binary_path = internal.get_binary_path()
  local version_path = internal.get_version_file_path()

  local uv = vim.uv or vim.loop
  local execs = world.executables or { curl = true, tar = true, unzip = true }
  local rec = { curl = {}, native_loads = 0, echoes = 0 }

  local saved = {
    system = vim.system,
    echo = vim.api.nvim_echo,
    filereadable = vim.fn.filereadable,
    readfile = vim.fn.readfile,
    executable = vim.fn.executable,
    os_uname = uv.os_uname,
    cpath = package.cpath,
    config = tt.config,
    preload = package.preload[NATIVE],
    loaded = package.loaded[NATIVE],
  }

  vim.system = function(cmd, _opts, _cb)
    table.insert(rec.curl, cmd)
    -- curl_fail_flag() probes synchronously with :wait(); every other call
    -- site passes a callback, which this deliberately never invokes.
    return {
      wait = function()
        return { code = 0, stdout = "", stderr = "" }
      end,
      kill = function() end,
    }
  end

  -- Swallowed rather than counted-and-printed: these messages would otherwise
  -- scroll through the test output, and their text is not under test.
  vim.api.nvim_echo = function()
    rec.echoes = rec.echoes + 1
  end

  vim.fn.filereadable = function(path)
    if path == binary_path then
      return world.binary_exists and 1 or 0
    end
    if path == version_path then
      return world.binary_version and 1 or 0
    end
    return saved.filereadable(path)
  end

  vim.fn.readfile = function(path, ...)
    if path == version_path then
      return { world.binary_version }
    end
    return saved.readfile(path, ...)
  end

  -- Total, not a fallthrough: setup() only ever asks about these three, and a
  -- host that happens to lack one must not change the answer.
  vim.fn.executable = function(name)
    return execs[name] and 1 or 0
  end

  if world.uname then
    uv.os_uname = function()
      return world.uname
    end
  end

  -- Beats the cpath searcher, so the real library on this machine is never
  -- dlopen'd and a load is observable as a call rather than as a side effect.
  package.loaded[NATIVE] = nil
  package.preload[NATIVE] = function()
    rec.native_loads = rec.native_loads + 1
    return {}
  end

  local ok, err = pcall(tt.setup, opts)

  vim.system = saved.system
  vim.api.nvim_echo = saved.echo
  vim.fn.filereadable = saved.filereadable
  vim.fn.readfile = saved.readfile
  vim.fn.executable = saved.executable
  uv.os_uname = saved.os_uname
  package.cpath = saved.cpath
  tt.config = saved.config
  package.preload[NATIVE] = saved.preload
  package.loaded[NATIVE] = saved.loaded

  if not ok then
    error(err, 0)
  end
  return rec
end

H.describe("M.setup ladder", function()
  H.it("returns before anything else when the platform is unsupported", function()
    local rec = run_setup({ uname = { sysname = "Plan9", machine = "mips" } }, {})
    H.eq(#api_calls(rec), 0, "no download")
    H.eq(rec.native_loads, 0, "no load")
    H.ok(rec.echoes > 0, "said something about the platform")
  end)

  H.it("downloads when the binary is missing and auto_download is on", function()
    local rec = run_setup({ binary_exists = false }, { auto_download = true })
    local urls = api_calls(rec)
    H.eq(#urls, 1, "exactly one release-API fetch")
    H.ok(ends_with(urls[1], WANTED_RELEASE), "asked for " .. WANTED_RELEASE .. ", got " .. tostring(urls[1]))
    H.eq(rec.native_loads, 0, "the load is deferred to the download callback")
  end)

  H.it("neither downloads nor loads when the binary is missing and auto_download is off", function()
    local rec = run_setup({ binary_exists = false }, { auto_download = false })
    H.eq(#api_calls(rec), 0, "no download")
    H.eq(rec.native_loads, 0, "returns before the load")
    H.ok(rec.echoes > 0, "told the user the binary is missing")
  end)

  H.it("loads without downloading when the version file matches the plugin", function()
    local rec = run_setup({ binary_exists = true, binary_version = internal.PLUGIN_VERSION }, {})
    H.eq(#api_calls(rec), 0, "no download")
    H.eq(rec.native_loads, 1, "loaded the native module")
    H.eq(rec.echoes, 0, "quiet on the happy path")
  end)

  H.it("updates when the version differs and auto_download and auto_update are both on", function()
    local rec = run_setup(
      { binary_exists = true, binary_version = "0.0.1" },
      { auto_download = true, auto_update = true }
    )
    local urls = api_calls(rec)
    H.eq(#urls, 1, "exactly one release-API fetch")
    H.ok(ends_with(urls[1], WANTED_RELEASE), "asked for " .. WANTED_RELEASE .. ", got " .. tostring(urls[1]))
    H.eq(rec.native_loads, 0, "the load is deferred to the download callback")
  end)

  H.it("treats a missing .version file as needing an update", function()
    -- No version file at all is the pre-0.1.x-upgrade shape, and setup()
    -- funnels it into the same update branch as a genuine mismatch.
    local rec = run_setup({ binary_exists = true, binary_version = nil }, {})
    H.eq(#api_calls(rec), 1, "exactly one release-API fetch")
    H.eq(rec.native_loads, 0, "the load is deferred to the download callback")
  end)

  H.it("loads a stale binary silently when auto_download is off (bughunt B48)", function()
    -- CHARACTERIZATION, NOT ENDORSEMENT. B48 is open and is not fixed in this
    -- batch. With auto_download = false and auto_update left at its default of
    -- true, a version-mismatched binary matches no branch of the ladder: the
    -- update branch wants auto_download, and the "auto-update is disabled"
    -- warning wants `not auto_update`. So control reaches the tail and the
    -- stale library is loaded without one word to the user, who opted out of
    -- downloads and never learns their binary does not match the plugin.
    --
    -- `echoes == 0` is the assertion that pins the bug. When B48 is fixed this
    -- test SHOULD fail -- update it then, deliberately. Do not "fix" it now.
    local rec = run_setup({ binary_exists = true, binary_version = "0.0.1" }, { auto_download = false })
    H.eq(#api_calls(rec), 0, "no download, as asked")
    H.eq(rec.native_loads, 1, "the stale binary is loaded anyway")
    H.eq(rec.echoes, 0, "and not one word about it: this is B48")
  end)

  H.it("warns about a version mismatch but loads anyway when auto_update is off", function()
    local rec = run_setup({ binary_exists = true, binary_version = "0.0.1" }, { auto_update = false })
    H.eq(#api_calls(rec), 0, "no download")
    H.ok(rec.echoes > 0, "warned about the mismatch")
    H.eq(rec.native_loads, 1, "this branch does not return; the tail loads")
  end)

  H.it("gives up on a missing binary when curl is absent", function()
    local rec = run_setup({ binary_exists = false, executables = { tar = true, unzip = true } }, {})
    H.eq(#api_calls(rec), 0, "no download")
    H.eq(rec.native_loads, 0, "the missing-binary branch returns")
    H.ok(rec.echoes > 0, "explained why")
  end)

  H.it("gives up on a missing binary when neither tar nor unzip is present", function()
    local rec = run_setup({ binary_exists = false, executables = { curl = true } }, {})
    H.eq(#api_calls(rec), 0, "no download")
    H.eq(rec.native_loads, 0, "the missing-binary branch returns")
    H.ok(rec.echoes > 0, "explained why")
  end)

  H.it("falls through to the stale binary when an update needs curl and curl is absent", function()
    -- Asymmetry with the two cases above, pinned deliberately: the
    -- missing-binary branch `return`s when its tooling check fails, the update
    -- branch does not: it echoes and drops through to the tail, which loads
    -- the existing library. Folding the two branches into one parameterised
    -- path has to keep both outcomes.
    local rec = run_setup(
      { binary_exists = true, binary_version = "0.0.1", executables = { tar = true, unzip = true } },
      {}
    )
    H.eq(#api_calls(rec), 0, "no download")
    H.ok(rec.echoes > 0, "warned that curl is missing")
    H.eq(rec.native_loads, 1, "the update branch does not return; the tail loads")
  end)

  H.it("attempts an update even with no extractor available", function()
    -- Second asymmetry: the update branch computes has_tar and has_unzip and
    -- then never reads them, so unlike the missing-binary branch it starts a
    -- download it cannot unpack. Pinned as-is.
    local rec = run_setup(
      { binary_exists = true, binary_version = "0.0.1", executables = { curl = true } },
      {}
    )
    H.eq(#api_calls(rec), 1, "downloads regardless of tar/unzip")
    H.eq(rec.native_loads, 0, "the load is deferred to the download callback")
  end)
end)

return H
