local H = require("harness")
local tt = require("time-tracking-nvim")

-- Characterization/behavior tests for the optional github_token config
-- option (whats-next W4): it must reach the release-API curl call
-- (fetch_release) as an Authorization header, and must never reach the
-- asset-download curl call (fetch_file, used for both the archive and
-- SHA256SUMS).
--
-- HOW THE REAL FUNCTION IS REACHED
--
-- download_binary is a `local` in init.lua with no test seam, and this spec
-- may not touch lua/. It is reached the same way spec_download.lua reaches
-- it: through debug.getupvalue on the public M.download, which closes over
-- it. That hands back the genuine function object, so what runs below is the
-- production body, not a re-implementation. Its opts table (the 5th
-- argument) is where github_token travels -- the same field
-- download_then_load and M.download thread `config.github_token` /
-- `(M.config or {}).github_token` into at their call sites -- so driving
-- download_binary directly with opts.github_token set exercises exactly what
-- a real setup({ github_token = ... }) + download() would produce.
--
-- vim.system is stubbed the same way spec_download.lua stubs it: calls made
-- WITH a callback (the real async curl invocations) are recorded rather than
-- answered, and calls made WITHOUT one (curl_fail_flag()'s synchronous
-- capability probe) are answered inline and never counted. vim.schedule is
-- stubbed to run its argument immediately, so driving continuation #1 (the
-- release-API response) runs the whole synchronous stretch that follows it
-- -- including the archive fetch's vim.system call -- before control returns
-- to the test.
--
-- os.getenv is also stubbed, scoped to GITHUB_TOKEN/GH_TOKEN: production
-- resolve_github_token() falls back to those two, so left alone a
-- contributor's own shell (GH_TOKEN is common for `gh` CLI scripting) would
-- leak an ambient token into the "no token configured" cases. Every other
-- environment variable still resolves through the real os.getenv. Each case
-- says what those two names answer with (with_run's third argument), so the
-- fallback is exercised in both directions -- present and absent -- rather
-- than only ever being stubbed away.

local function upvalue(fn, name)
  local i = 1
  while true do
    local n, v = debug.getupvalue(fn, i)
    if not n then
      return nil
    end
    if n == name then
      return v
    end
    i = i + 1
  end
end

local download_binary = upvalue(tt.download, "download_binary")

-- Same fallback spec_download.lua uses: report a single failing case rather
-- than let a nil upvalue turn every case below into "attempt to call a nil
-- value".
if not download_binary then
  H.describe("github_token control flow", function()
    H.it("is still reachable through M.download's upvalues", function()
      error("M.download no longer closes over download_binary -- update this seam")
    end)
  end)
  return H
end

local VERSION = tt._internal.PLUGIN_VERSION
local LINUX = "x86_64-unknown-linux-gnu"
local RELEASE_BASE = "https://github.com/stevenwcarter/time-tracking-nvim/releases/download/v" .. VERSION .. "/"

local function asset_for(target)
  local ext = target:match("windows") and ".zip" or ".tar.gz"
  return "time-tracking-nvim-" .. target .. ext
end

-- A release payload with exactly one asset matching `target` and no
-- SHA256SUMS, so driving the release fetch leads to exactly one further
-- vim.system call: the archive download.
local function release_json(target)
  return vim.json.encode({
    tag_name = "v" .. VERSION,
    assets = {
      { name = asset_for(target), browser_download_url = RELEASE_BASE .. asset_for(target) },
    },
  })
end

-- Starts a real download_binary call with the outside world stubbed, and
-- hands back a handle for driving its continuations and inspecting what was
-- spawned. Modeled on spec_download.lua's start()/drive() shape, trimmed to
-- what this file needs.
--
-- Handle fields:
--   calls   { {argv, cb}, ... } for every vim.system spawned WITH a callback
--   done    { ok, err } once the download_binary callback fires, nil until then
-- `env` maps GITHUB_TOKEN/GH_TOKEN to the values the stubbed os.getenv should
-- answer with; anything absent from it reads back as nil. Defaulting it to an
-- empty table is what makes "no token anywhere" the baseline, and passing a
-- populated one is what lets a case exercise resolve_github_token's
-- environment half.
local function start(env)
  env = env or {}
  local target = LINUX
  local root = vim.fn.tempname() .. "_token_spec"
  vim.fn.mkdir(root, "p")

  local run = {
    root = root,
    target = target,
    binary_path = vim.fs.joinpath(root, "lua", "time_tracking_nvim.so"),
    calls = {},
    done = nil,
  }

  local saved = {
    system = vim.system,
    schedule = vim.schedule,
    tempname = vim.fn.tempname,
    getenv = os.getenv,
  }

  -- resolve_github_token falls back to these two when opts.github_token is
  -- nil. Left unstubbed, a contributor's own shell (GH_TOKEN is common for
  -- `gh` CLI scripting) would leak into "no token configured" cases and
  -- make them fail outside this ambient state -- exactly the class of
  -- externality spec_download.lua/spec_setup.lua stub out for vim.system,
  -- vim.fn.tempname/executable, uv.os_uname, etc. Every other name falls
  -- through to the real os.getenv unchanged. Driving both names from `env`
  -- rather than hardcoding nil is what lets a case put a token in the
  -- environment deterministically, on any machine, either way round.
  os.getenv = function(name)
    if name == "GITHUB_TOKEN" or name == "GH_TOKEN" then
      return env[name]
    end
    return saved.getenv(name)
  end

  vim.system = function(cmd, _opts, cb)
    if not cb then
      -- The synchronous caller: curl_fail_flag()'s capability probe. Answered
      -- without spawning anything; its result is cached module-wide, so
      -- whether it even reaches here depends on spec ordering, and it must
      -- never be counted as a call this function made.
      return {
        wait = function()
          return { code = 0, stdout = "", stderr = "" }
        end,
        kill = function() end,
      }
    end
    table.insert(run.calls, { argv = cmd, cb = cb })
    return { kill = function() end }
  end

  -- Inline, so driving a continuation runs the whole synchronous stretch that
  -- follows it before returning -- including the archive fetch that follows a
  -- successful release-API response.
  vim.schedule = function(fn)
    fn()
  end

  vim.fn.tempname = function()
    return vim.fs.joinpath(root, "tmp")
  end

  function run:finish()
    vim.system = saved.system
    vim.schedule = saved.schedule
    vim.fn.tempname = saved.tempname
    os.getenv = saved.getenv
    vim.fn.delete(root, "rf")
  end

  function run:drive(i, result)
    local call = self.calls[i]
    if not call then
      error(string.format("no vim.system call #%d (saw %d)", i, #self.calls), 2)
    end
    call.cb(result)
  end

  function run:begin(opts)
    download_binary(self.target, self.binary_path, function(ok, err)
      self.done = { ok = ok, err = err }
    end, VERSION, opts)
  end

  return run
end

local function with_run(opts, fn, env)
  local run = start(env)
  local ok, err = pcall(function()
    run:begin(opts)
    fn(run)
  end)
  run:finish()
  if not ok then
    error(err, 0)
  end
end

-- The Authorization header in an argv, or nil if there is none. Looks for
-- the flag pair rather than a substring match, so this cannot be fooled by
-- the token happening to appear elsewhere in the command.
local function auth_header(argv)
  for i, arg in ipairs(argv) do
    if arg == "-H" and type(argv[i + 1]) == "string" and argv[i + 1]:match("^Authorization:") then
      return argv[i + 1]
    end
  end
  return nil
end

H.describe("github_token", function()
  H.it("adds an Authorization header to the release-API curl call when configured", function()
    with_run({ github_token = "test-token-123" }, function(run)
      H.eq(#run.calls, 1, "only the release fetch should have happened so far")
      H.eq(
        auth_header(run.calls[1].argv),
        "Authorization: Bearer test-token-123",
        "expected an Authorization header in: " .. vim.inspect(run.calls[1].argv)
      )
    end)
  end)

  H.it("omits the Authorization header from the release-API call when no token is configured", function()
    with_run({}, function(run)
      H.eq(#run.calls, 1, "only the release fetch should have happened so far")
      H.eq(auth_header(run.calls[1].argv), nil, "no token was configured, but a header was sent anyway")
    end)
  end)

  -- The environment half of resolve_github_token
  -- (`... or os.getenv("GITHUB_TOKEN") or os.getenv("GH_TOKEN")`). Every case
  -- above stubs both names to nil, so without this the fallback never
  -- executes under test at all and deleting it would break nothing.
  H.it("falls back to $GITHUB_TOKEN when setup() configured no github_token", function()
    with_run({}, function(run)
      H.eq(#run.calls, 1, "only the release fetch should have happened so far")
      H.eq(
        auth_header(run.calls[1].argv),
        "Authorization: Bearer env-token-abc",
        "expected the environment token in: " .. vim.inspect(run.calls[1].argv)
      )
    end, { GITHUB_TOKEN = "env-token-abc" })
  end)

  H.it("falls back to $GH_TOKEN when neither github_token nor $GITHUB_TOKEN is set", function()
    with_run({}, function(run)
      H.eq(
        auth_header(run.calls[1].argv),
        "Authorization: Bearer gh-token-xyz",
        "expected the GH_TOKEN fallback in: " .. vim.inspect(run.calls[1].argv)
      )
    end, { GH_TOKEN = "gh-token-xyz" })
  end)

  H.it("an explicit github_token wins over the environment", function()
    with_run({ github_token = "explicit-token" }, function(run)
      H.eq(
        auth_header(run.calls[1].argv),
        "Authorization: Bearer explicit-token",
        "setup()'s value must win over $GITHUB_TOKEN: " .. vim.inspect(run.calls[1].argv)
      )
    end, { GITHUB_TOKEN = "env-token-abc" })
  end)

  H.it("never adds the Authorization header to the asset-download curl call, even when a token is configured", function()
    with_run({ github_token = "test-token-123" }, function(run)
      -- Drive the release fetch to success so the archive download is
      -- reached. No SHA256SUMS asset is published in this fixture, so this is
      -- the second and last vim.system call this run makes.
      run:drive(1, { code = 0, stdout = release_json(LINUX), stderr = "" })

      H.eq(#run.calls, 2, "expected the release fetch, then the archive fetch")
      H.eq(
        auth_header(run.calls[1].argv),
        "Authorization: Bearer test-token-123",
        "the release-API call lost its header"
      )
      H.eq(
        auth_header(run.calls[2].argv),
        nil,
        "an asset-download argv must never carry the Authorization header: " .. vim.inspect(run.calls[2].argv)
      )
    end)
  end)
end)

return H
