local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal
local health_mod = require("time-tracking-nvim.health")

-- Characterization tests for M.check, ahead of the decomposition that carves
-- its seven sequential probe sections (Platform, Binary, Versions, cpath,
-- Load, Commands, External tools) into helpers.
--
-- What is pinned, and how:
--
--   * The *sequence* of vim.health calls -- which function (start/ok/warn/
--     error/info) fired and in what order -- never their message text. The
--     decomposition moves every message verbatim, so wording assertions would
--     add no protection; ordering is exactly what a decomposition can get
--     wrong (a helper called from the wrong place, an early return that
--     stops sequencing a section too soon or too late).
--
--   * Which underlying probes actually ran, in order, recorded independently
--     of vim.health. This is what makes "no later section reports" (the
--     early-return cases) an observed fact -- the probe for that section
--     simply never fired -- rather than an inference from a health-call count
--     that could coincidentally match.
--
-- health.lua captures vim.health and vim.uv (or vim.loop) once, as its module
-- -level `health` and `uv` locals. Stubbing fields on those same table
-- objects, rather than replacing the tables, is what spec_platform and
-- spec_setup already do for uv.os_uname -- it works here for the same reason.
--
-- Every dependency M.check reaches out to is stubbed for every case, not just
-- the one under test, so these tests are deterministic on any contributor's
-- machine regardless of what platform, binaries, or tools actually exist
-- there.

-- A world in which every section reports success. Each test starts from this
-- and overrides only the fields its scenario cares about.
local function default_world()
  return {
    platform_info = { target = "x86_64-unknown-linux-gnu", ext = "so" },
    platform_err = nil,
    binary_path = "/fake/root/lua/time_tracking_nvim.so",
    binary_readable = true,
    stat = { size = 4096 },
    binary_version = "1.2.3",
    plugin_version = "1.2.3",
    plugin_root = "/fake/root",
    cpath_has_root = true,
    load_status = "ok",
    load_value = {},
    commands_registered = true,
    executables = { curl = true, tar = true, unzip = true },
  }
end

-- Runs health.check() with every dependency it reaches out to stubbed per
-- `world`, and returns { probes = "...", health = "..." }: comma-joined,
-- in-order records of which probe functions ran and which vim.health
-- functions fired, so a case can assert on both with a single H.eq. Anything
-- health.check() throws is re-raised after teardown.
local function run_check(world)
  local uv = vim.uv or vim.loop
  local probes, healthcalls = {}, {}

  local saved = {
    start = vim.health.start,
    ok = vim.health.ok,
    warn = vim.health.warn,
    error = vim.health.error,
    info = vim.health.info,
    get_platform_info = internal.get_platform_info,
    get_binary_path = internal.get_binary_path,
    read_binary_version = internal.read_binary_version,
    plugin_version = internal.PLUGIN_VERSION,
    plugin_root = internal.plugin_root,
    load_native = internal.load_native,
    filereadable = vim.fn.filereadable,
    fs_stat = uv.fs_stat,
    exists = vim.fn.exists,
    executable = vim.fn.executable,
    cpath = package.cpath,
  }

  local function record_health(kind)
    return function()
      table.insert(healthcalls, kind)
    end
  end
  vim.health.start = record_health("start")
  vim.health.ok = record_health("ok")
  vim.health.warn = record_health("warn")
  vim.health.error = record_health("error")
  vim.health.info = record_health("info")

  internal.get_platform_info = function()
    table.insert(probes, "get_platform_info")
    return world.platform_info, world.platform_err
  end

  internal.get_binary_path = function()
    table.insert(probes, "get_binary_path")
    return world.binary_path
  end

  internal.read_binary_version = function()
    table.insert(probes, "read_binary_version")
    return world.binary_version
  end

  internal.PLUGIN_VERSION = world.plugin_version

  internal.plugin_root = function()
    table.insert(probes, "plugin_root")
    return world.plugin_root
  end

  internal.load_native = function()
    table.insert(probes, "load_native")
    return world.load_status, world.load_value
  end

  vim.fn.filereadable = function(path)
    if path == world.binary_path then
      table.insert(probes, "filereadable")
      return world.binary_readable and 1 or 0
    end
    return saved.filereadable(path)
  end

  uv.fs_stat = function(path)
    if path == world.binary_path then
      table.insert(probes, "fs_stat")
      return world.stat
    end
    return saved.fs_stat(path)
  end

  -- The cpath section does a literal substring search for
  -- joinpath(plugin_root(), "lua"), so it is driven by setting package.cpath
  -- to contain (or omit) that exact substring, rather than by stubbing find().
  if world.plugin_root then
    local needle = vim.fs.joinpath(world.plugin_root, "lua")
    if world.cpath_has_root then
      package.cpath = needle .. "/?.so;" .. saved.cpath
    else
      package.cpath = saved.cpath
    end
  end

  vim.fn.exists = function(name)
    if name == ":TimeTrackingToggle" then
      table.insert(probes, "exists")
      return world.commands_registered and 2 or 0
    end
    return saved.exists(name)
  end

  vim.fn.executable = function(name)
    local execs = world.executables or {}
    if execs[name] ~= nil then
      table.insert(probes, "executable:" .. name)
      return execs[name] and 1 or 0
    end
    return saved.executable(name)
  end

  local ok, err = pcall(health_mod.check)

  vim.health.start = saved.start
  vim.health.ok = saved.ok
  vim.health.warn = saved.warn
  vim.health.error = saved.error
  vim.health.info = saved.info
  internal.get_platform_info = saved.get_platform_info
  internal.get_binary_path = saved.get_binary_path
  internal.read_binary_version = saved.read_binary_version
  internal.PLUGIN_VERSION = saved.plugin_version
  internal.plugin_root = saved.plugin_root
  internal.load_native = saved.load_native
  vim.fn.filereadable = saved.filereadable
  uv.fs_stat = saved.fs_stat
  vim.fn.exists = saved.exists
  vim.fn.executable = saved.executable
  package.cpath = saved.cpath

  if not ok then
    error(err, 0)
  end

  return {
    probes = table.concat(probes, ","),
    health = table.concat(healthcalls, ","),
  }
end

-- The probe sequence for a run that never returns early: every section is
-- reached and each of the three external tools is checked in turn.
local FULL_PROBES = table.concat({
  "get_platform_info",
  "get_binary_path",
  "filereadable",
  "fs_stat",
  "read_binary_version",
  "plugin_root",
  "load_native",
  "exists",
  "executable:curl",
  "executable:tar",
  "executable:unzip",
}, ",")

H.describe("M.check", function()
  H.it("happy path: every section reports and health.ok fires for each", function()
    local rec = run_check(default_world())
    H.eq(rec.probes, FULL_PROBES, "probes")
    H.eq(rec.health, "start,ok,ok,ok,ok,ok,ok,ok,ok,ok", "health calls")
  end)

  H.it("no platform: errors once and returns before any later section", function()
    local world = default_world()
    world.platform_info = nil
    world.platform_err = "Unsupported platform: plan9-mips"
    local rec = run_check(world)
    H.eq(rec.probes, "get_platform_info", "probes: only the platform check ran")
    H.eq(rec.health, "start,error", "health calls")
  end)

  H.it("no binary: reports and returns before version/cpath/load/commands/tools", function()
    local world = default_world()
    world.binary_readable = false
    local rec = run_check(world)
    H.eq(rec.probes, "get_platform_info,get_binary_path,filereadable", "probes: stops before fs_stat")
    H.eq(rec.health, "start,ok,error", "health calls")
  end)

  -- check_binary has three abort branches sharing one `return nil` --
  -- binary_path nil, filereadable failing (pinned above), and fs_stat
  -- failing (pinned below). All three collapse into a single helper, which
  -- is exactly the consolidation that made this finding risk: high, so each
  -- gets its own case rather than relying on one to stand in for the other
  -- two.
  H.it("get_binary_path returns nil: reports and returns before filereadable/fs_stat", function()
    local world = default_world()
    world.binary_path = nil
    local rec = run_check(world)
    H.eq(rec.probes, "get_platform_info,get_binary_path", "probes: stops before filereadable")
    H.eq(rec.health, "start,ok,error", "health calls")
  end)

  H.it("fs_stat fails: reports and returns before version/cpath/load/commands/tools", function()
    local world = default_world()
    world.stat = nil
    local rec = run_check(world)
    H.eq(rec.probes, "get_platform_info,get_binary_path,filereadable,fs_stat", "probes: stops after fs_stat")
    H.eq(rec.health, "start,ok,error", "health calls")
  end)

  H.it("version mismatch: warns but continues to the later sections", function()
    local world = default_world()
    world.plugin_version = "2.0.0"
    world.binary_version = "1.0.0"
    local rec = run_check(world)
    H.eq(rec.probes, FULL_PROBES, "probes: no early return on a mismatch")
    H.eq(rec.health, "start,ok,ok,warn,ok,ok,ok,ok,ok,ok", "health calls")
  end)

  H.it("native module fails to load: reports and continues to commands/tools", function()
    local world = default_world()
    world.load_status = "load_failed"
    world.load_value = "stub: cannot open shared object"
    local rec = run_check(world)
    H.eq(rec.probes, FULL_PROBES, "probes: no early return on a load failure")
    H.eq(rec.health, "start,ok,ok,ok,ok,error,ok,ok,ok,ok", "health calls")
  end)
end)

return H
