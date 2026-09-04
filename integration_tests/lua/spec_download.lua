local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal

-- Characterization tests for download_binary's callback pyramid.
--
-- These pin what the pyramid does *today*, ahead of the refactor that flattens
-- it. spec_setup.lua stops at download_binary's first line -- its vim.system
-- stub records the release-API argv and never invokes the callback -- so
-- everything below that line (HTTP-failure reconciliation, asset matching, the
-- trust check, the SHA256SUMS fetch, checksum verification, extraction,
-- install, and the success/failure dispatch back to the caller) had no
-- coverage at all. This file is that net.
--
-- HOW THE REAL FUNCTION IS REACHED
--
-- download_binary is a `local` in init.lua with no test seam, and this spec may
-- not touch lua/. It is reached instead through debug.getupvalue on the public
-- M.download, which closes over it: that hands back the genuine function
-- object, so what runs below is the production body, not a re-implementation.
-- Its five arguments (target, binary_path, callback, expected_version, opts)
-- are then supplied directly, which is what makes the callback's (ok, err)
-- observable rather than having to be inferred from an echo.
--
-- Everything the body reaches out to is stubbed at the same depth spec_setup
-- already uses -- vim.system -- and the continuation it hands that stub is
-- driven with a synthetic result. Filesystem work is left real but confined to
-- a per-case sandbox, so "the archive was installed", "the temp directory was
-- cleaned up" and "the version file was written" are checked by looking at
-- disk rather than at a message.
--
-- WHAT THE ASSERTIONS ARE ALLOWED TO TOUCH
--
-- The flatten ahead will deliberately reword messages, so no case asserts on
-- our own prose. Cases assert on: which commands were spawned and with what
-- argv, whether the callback fired and with what (ok, err), what exists on
-- disk, and whether cleanup ran. Where the only observable difference between
-- two branches is the error string -- the four release-fetch failures all end
-- in callback(false, ...) with nothing else to see -- the assertion is that a
-- value *we supplied* (curl's stderr, the API's own message, the offending
-- URL, the target name) is propagated into it. That is a contract about
-- information flow, not about wording, and survives a reword.
--
-- Known bugs in this area are pinned as they stand, never as they ought to be;
-- see the B28 and B56 notes at their cases.

local VERSION = internal.PLUGIN_VERSION
local LINUX = "x86_64-unknown-linux-gnu"
local WINDOWS = "x86_64-pc-windows-msvc"
local RELEASE_BASE = "https://github.com/stevenwcarter/time-tracking-nvim/releases/download/v" .. VERSION .. "/"
local API_BASE = "https://api.github.com/repos/stevenwcarter/time-tracking-nvim/releases"

-- 64 hex characters each, so both satisfy file_sha256's anchored pattern.
local DIGEST = string.rep("ab", 32)
local OTHER_DIGEST = string.rep("cd", 32)

-- The production function itself, not a stand-in. M.download closes over it.
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

local function asset_for(target)
  local ext = target:match("windows") and ".zip" or ".tar.gz"
  return "time-tracking-nvim-" .. target .. ext
end

local function write_file(path, content)
  vim.fn.mkdir(vim.fs.dirname(path), "p")
  local f = assert(io.open(path, "wb"))
  f:write(content)
  f:close()
end

local function read_file(path)
  local f = assert(io.open(path, "rb"))
  local s = f:read("*a")
  f:close()
  return s
end

local function contains(haystack, needle)
  return type(haystack) == "string" and haystack:find(needle, 1, true) ~= nil
end

-- A release payload with one platform asset and, optionally, a SHA256SUMS one.
--   o.asset_name   override the asset's name, to make it not match the target
--   o.asset_url    override its browser_download_url, for the trust check
--   o.sums_url     add a SHA256SUMS asset with this URL
--   o.tag          the release's tag_name
local function release_json(o)
  o = o or {}
  local assets = {
    {
      name = o.asset_name or asset_for(o.target or LINUX),
      browser_download_url = o.asset_url or (RELEASE_BASE .. asset_for(o.target or LINUX)),
    },
  }
  if o.sums_url then
    table.insert(assets, { name = "SHA256SUMS", browser_download_url = o.sums_url })
  end
  return vim.json.encode({ tag_name = o.tag or ("v" .. VERSION), assets = assets })
end

-- Starts a real download_binary call with the outside world stubbed, and hands
-- back a handle for driving its continuations and inspecting what happened.
--
-- o.target             defaults to the linux gnu triple
-- o.binary_name        basename of the install destination
-- o.expected_version   4th argument; false means "pass nil", i.e. ask /latest
-- o.allow_unverified   opts.allow_unverified
--
-- Handle fields:
--   calls      { {argv, cb}, ... } for every vim.system spawned WITH a callback
--   hashes     argv[1] of every checksum subprocess (vim.system + :wait())
--   deletes    { {path, flags}, ... } every vim.fn.delete
--   versions   { {lines, path}, ... } every vim.fn.writefile
--   done       { ok, err } once the callback fires, nil until then
--   sha        the synthetic result the stubbed checksum subprocess returns
local function start(o)
  o = o or {}
  local target = o.target or LINUX
  local asset_name = asset_for(target)

  -- Called before the tempname stub is installed, so this is a real temp path.
  local root = vim.fn.tempname() .. "_dl_spec"
  vim.fn.mkdir(root, "p")

  local run = {
    root = root,
    target = target,
    asset_name = asset_name,
    binary_path = vim.fs.joinpath(root, "lua", o.binary_name or "time_tracking_nvim.so"),
    -- download_binary builds this as tempname() .. "_time_tracking"; the
    -- tempname stub below is what makes it predictable.
    temp_dir = vim.fs.joinpath(root, "tmp") .. "_time_tracking",
    calls = {},
    hashes = {},
    deletes = {},
    versions = {},
    echoes = 0,
    done = nil,
    callbacks = 0,
    sha = { code = 0, stdout = DIGEST .. "  archive", stderr = "" },
  }
  run.temp_file = vim.fs.joinpath(run.temp_dir, asset_name)
  run.sums_file = vim.fs.joinpath(run.temp_dir, "SHA256SUMS")
  run.extracted = vim.fs.joinpath(run.temp_dir, "target", "release", vim.fs.basename(run.binary_path))

  local saved = {
    system = vim.system,
    schedule = vim.schedule,
    echo = vim.api.nvim_echo,
    tempname = vim.fn.tempname,
    delete = vim.fn.delete,
    writefile = vim.fn.writefile,
    executable = vim.fn.executable,
  }

  vim.system = function(cmd, _opts, cb)
    if not cb then
      -- The two synchronous callers. curl_fail_flag()'s capability probe is
      -- answered without spawning anything (its result is cached module-wide,
      -- so whether it even reaches here depends on spec ordering, and it must
      -- never be counted as work this function did). Everything else with no
      -- callback is file_sha256's checksum subprocess.
      if cmd[1] ~= "curl" then
        table.insert(run.hashes, cmd[1])
      end
      local result = (cmd[1] == "curl") and { code = 0, stdout = "", stderr = "" } or run.sha
      return {
        wait = function()
          return result
        end,
        kill = function() end,
      }
    end
    table.insert(run.calls, { argv = cmd, cb = cb })
    return { kill = function() end }
  end

  -- Inline, so driving a continuation runs the whole synchronous stretch that
  -- follows it before returning. Nothing in this path depends on actually
  -- yielding to the loop, and this keeps every case deterministic and free of
  -- sleeps. A flatten that switches to vim.schedule_wrap is still intercepted:
  -- schedule_wrap is built on vim.schedule.
  vim.schedule = function(fn)
    fn()
  end

  vim.api.nvim_echo = function()
    run.echoes = run.echoes + 1
  end

  vim.fn.tempname = function()
    return vim.fs.joinpath(root, "tmp")
  end

  vim.fn.delete = function(path, flags)
    table.insert(run.deletes, { path = path, flags = flags })
    return saved.delete(path, flags)
  end

  -- Intercepted rather than sandboxed, because it cannot be sandboxed:
  -- write_binary_version ignores the binary_path download_binary was given and
  -- recomputes the destination from plugin_root(), so letting this through
  -- would drop a .version file into the checked-out repo. Recording it also
  -- makes "which version was recorded" assertable. See the note on the
  -- resolved-tag case.
  vim.fn.writefile = function(lines, path)
    table.insert(run.versions, { lines = lines, path = path })
    return 0
  end

  -- file_sha256 picks its implementation off this; pinning it to sha256sum
  -- keeps the argv identical on every host.
  vim.fn.executable = function(name)
    return name == "sha256sum" and 1 or 0
  end

  function run:finish()
    vim.system = saved.system
    vim.schedule = saved.schedule
    vim.api.nvim_echo = saved.echo
    vim.fn.tempname = saved.tempname
    vim.fn.delete = saved.delete
    vim.fn.writefile = saved.writefile
    vim.fn.executable = saved.executable
    saved.delete(root, "rf")
  end

  -- Invokes continuation #i with `result`, after creating any files the
  -- command it stands in for would have produced.
  function run:drive(i, result, files)
    for path, content in pairs(files or {}) do
      write_file(path, content)
    end
    local call = self.calls[i]
    if not call then
      error(string.format("no vim.system call #%d (saw %d)", i, #self.calls), 2)
    end
    call.cb(result)
  end

  -- Spelled out rather than an `and`/`or` ternary: the false branch is nil,
  -- which such a ternary cannot express.
  local expected_version = o.expected_version or VERSION
  if o.expected_version == false then
    expected_version = nil
  end

  -- Separate from start() so with_run can run it under the same pcall that
  -- guarantees finish(): a throw out of download_binary before the stubs came
  -- back off would corrupt every case after this one.
  function run:begin()
    download_binary(target, self.binary_path, function(ok, err)
      self.callbacks = self.callbacks + 1
      self.done = { ok = ok, err = err }
    end, expected_version, { allow_unverified = o.allow_unverified })
  end

  return run
end

local function with_run(o, fn)
  local run = start(o)
  local ok, err = pcall(function()
    run:begin()
    fn(run)
  end)
  run:finish()
  if not ok then
    error(err, 0)
  end
end

-- argv[1] of each spawned-with-callback command, in order: the shape of the
-- pyramid's descent, which is exactly what a flatten must preserve.
local function spawned(run)
  local names = {}
  for _, call in ipairs(run.calls) do
    table.insert(names, call.argv[1])
  end
  return table.concat(names, ",")
end

local function argv(run, i)
  return table.concat(run.calls[i].argv, " ")
end

local function failed(run)
  H.eq(run.callbacks, 1, "callback should have fired exactly once")
  H.eq(run.done.ok, false, "expected a failure")
  return tostring(run.done.err)
end

local function succeeded(run)
  H.eq(run.callbacks, 1, "callback should have fired exactly once")
  H.ok(run.done.ok, "expected success, got: " .. tostring(run.done and run.done.err))
end

local function cleaned_up(run)
  H.eq(#run.deletes, 1, "expected exactly one cleanup delete")
  H.eq(run.deletes[1].path, run.temp_dir, "cleanup deleted the wrong path")
  H.eq(run.deletes[1].flags, "rf", "cleanup was not recursive+force")
  H.eq(vim.fn.isdirectory(run.temp_dir), 0, "temp directory survived cleanup")
end

-- Drives the release fetch and the archive download, parking the run at
-- whatever the body does next.
local function to_archive(run, release)
  run:drive(1, { code = 0, stdout = release, stderr = "" })
  run:drive(2, { code = 0, stdout = "", stderr = "" }, { [run.temp_file] = "archive bytes" })
end

H.describe("download_binary control flow", function()
  H.it("asks for the pinned release tag when given an expected version", function()
    with_run({}, function(run)
      H.eq(spawned(run), "curl", "one spawn, the release-metadata fetch")
      H.ok(contains(argv(run, 1), API_BASE .. "/tags/v" .. VERSION), "argv was " .. argv(run, 1))
      H.eq(run.callbacks, 0, "nothing has been decided yet")
    end)
  end)

  H.it("asks for the latest release when given no expected version", function()
    with_run({ expected_version = false }, function(run)
      H.ok(contains(argv(run, 1), API_BASE .. "/latest"), "argv was " .. argv(run, 1))
    end)
  end)

  H.it("gives up when the release fetch fails with nothing to decode", function()
    with_run({}, function(run)
      run:drive(1, { code = 6, stdout = "", stderr = "curl: (6) Could not resolve host" })
      H.eq(spawned(run), "curl", "no download was started")
      H.ok(contains(failed(run), "Could not resolve host"), "curl's stderr is not propagated")
      H.eq(#run.deletes, 0, "nothing to clean up; the temp dir was never made")
      H.eq(vim.fn.isdirectory(run.temp_dir), 0, "temp dir should not exist")
    end)
  end)

  H.it("keeps going past a non-zero exit when the body still decodes", function()
    -- The --fail-with-body reconciliation, and the single most invertible
    -- decision in the function: curl exits non-zero on an HTTP error but still
    -- writes the response body, so a 403 both fails the exit-code check and
    -- decodes to a table. Today that combination deliberately does NOT bail at
    -- the exit-code guard; it falls through to the API-error guard below,
    -- which can name the real reason. A flatten that reorders these two
    -- reports "curl exited with code 22" for every rate limit.
    with_run({}, function(run)
      run:drive(1, {
        code = 22,
        stdout = '{"message":"API rate limit exceeded for 203.0.113.7"}',
        stderr = "curl: (22) The requested URL returned error: 403",
      })
      H.eq(spawned(run), "curl", "no download was started")
      H.ok(contains(failed(run), "API rate limit exceeded for 203.0.113.7"), "the API's own reason is lost: " .. failed(run))
    end)
  end)

  H.it("reports a GitHub API error body on a successful fetch", function()
    with_run({}, function(run)
      run:drive(1, { code = 0, stdout = '{"message":"Not Found"}', stderr = "" })
      H.eq(spawned(run), "curl")
      H.ok(contains(failed(run), "Not Found"), "the API's own reason is lost")
    end)
  end)

  H.it("gives up when the release body is not JSON at all", function()
    with_run({}, function(run)
      run:drive(1, { code = 0, stdout = "<html>502 Bad Gateway</html>", stderr = "" })
      H.eq(spawned(run), "curl", "no download was started")
      failed(run)
    end)
  end)

  H.it("gives up when the release body decodes to a non-table", function()
    with_run({}, function(run)
      run:drive(1, { code = 0, stdout = "123", stderr = "" })
      H.eq(spawned(run), "curl")
      H.ok(contains(failed(run), "123"), "the raw body is not echoed back")
    end)
  end)

  H.it("gives up when the release has no assets array", function()
    with_run({}, function(run)
      run:drive(1, { code = 0, stdout = '{"tag_name":"v' .. VERSION .. '"}', stderr = "" })
      H.eq(spawned(run), "curl")
      H.ok(contains(failed(run), "tag_name"), "the raw body is not echoed back")
    end)
  end)

  H.it("reports when no asset matches the target", function()
    with_run({}, function(run)
      run:drive(1, {
        code = 0,
        stdout = release_json({ asset_name = "time-tracking-nvim-sparc-sun-solaris.tar.gz" }),
        stderr = "",
      })
      H.eq(spawned(run), "curl", "nothing was downloaded")
      H.ok(contains(failed(run), LINUX), "the target is not named in the failure")
    end)
  end)

  H.it("refuses an untrusted download URL and fetches nothing", function()
    -- The security boundary. browser_download_url is taken verbatim out of the
    -- API response and handed to curl, so this guard is the only thing between
    -- a tampered response and a dlopen of whatever it points at. The assertion
    -- that matters is not the message: it is that curl was never spawned a
    -- second time, that the untrusted host appears in no argv, and that the
    -- refusal happens before the temp directory is even created.
    local evil = "https://github.com.evil.example/stevenwcarter/time-tracking-nvim/x.tar.gz"
    with_run({}, function(run)
      run:drive(1, { code = 0, stdout = release_json({ asset_url = evil }), stderr = "" })
      H.eq(spawned(run), "curl", "the untrusted URL was fetched")
      for i = 1, #run.calls do
        H.ok(not contains(argv(run, i), "evil.example"), "argv #" .. i .. " reached the untrusted host")
      end
      H.ok(contains(failed(run), evil), "the refused URL is not named")
      H.eq(vim.fn.isdirectory(run.temp_dir), 0, "refused after making a temp dir")
      H.eq(#run.deletes, 0, "nothing was created, so nothing is cleaned up")
    end)
  end)

  H.it("refuses an untrusted SHA256SUMS URL after the archive is already down", function()
    -- Same boundary, one level in, and with a different shape: by the time
    -- this fires the archive has been fetched into the temp directory, so the
    -- refusal has cleanup attached to it. Pinned together because a flatten
    -- that centralises cleanup could keep the refusal and lose the delete.
    local evil = "https://sha.evil.example/SHA256SUMS"
    with_run({}, function(run)
      to_archive(run, release_json({ sums_url = evil }))
      H.eq(spawned(run), "curl,curl", "the untrusted SHA256SUMS URL was fetched")
      for i = 1, #run.calls do
        H.ok(not contains(argv(run, i), "evil.example"), "argv #" .. i .. " reached the untrusted host")
      end
      H.ok(contains(failed(run), evil), "the refused URL is not named")
      H.eq(#run.hashes, 0, "nothing was hashed")
      cleaned_up(run)
    end)
  end)

  H.it("cleans up and reports when the archive download fails", function()
    with_run({}, function(run)
      run:drive(1, { code = 0, stdout = release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }), stderr = "" })
      H.eq(spawned(run), "curl,curl")
      H.ok(contains(argv(run, 2), run.temp_file), "the archive is not fetched into the temp dir")
      run:drive(2, { code = 7, stdout = "", stderr = "curl: (7) connection refused" })
      H.ok(contains(failed(run), "connection refused"), "curl's stderr is not propagated")
      cleaned_up(run)
    end)
  end)

  H.it("cleans up when the SHA256SUMS download fails", function()
    with_run({}, function(run)
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }))
      H.eq(spawned(run), "curl,curl,curl")
      H.ok(contains(argv(run, 3), run.sums_file), "SHA256SUMS is not fetched into the temp dir")
      run:drive(3, { code = 28, stdout = "", stderr = "curl: (28) timed out" })
      H.ok(contains(failed(run), "timed out"), "curl's stderr is not propagated")
      H.eq(#run.hashes, 0, "nothing was hashed")
      cleaned_up(run)
    end)
  end)

  H.it("cleans up when SHA256SUMS reports success but no file lands", function()
    -- The second half of `sums_result.code ~= 0 or filereadable(...) ~= 1`.
    -- Distinct from the case above: curl claims success and the guard still
    -- refuses, because the file is not there.
    with_run({}, function(run)
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }))
      run:drive(3, { code = 0, stdout = "", stderr = "" })
      failed(run)
      H.eq(#run.hashes, 0, "nothing was hashed")
      cleaned_up(run)
    end)
  end)

  H.it("cleans up when SHA256SUMS has no entry for the asset", function()
    with_run({}, function(run)
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }))
      run:drive(3, { code = 0, stdout = "", stderr = "" }, {
        [run.sums_file] = DIGEST .. "  some-other-asset.tar.gz\n",
      })
      H.eq(spawned(run), "curl,curl,curl", "extraction was reached without a digest")
      H.ok(contains(failed(run), run.asset_name), "the asset is not named")
      H.eq(#run.hashes, 0, "hashed the archive despite having no digest to compare")
      cleaned_up(run)
    end)
  end)

  H.it("refuses and does not extract when the digest mismatches", function()
    with_run({}, function(run)
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }))
      run:drive(3, { code = 0, stdout = "", stderr = "" }, {
        [run.sums_file] = OTHER_DIGEST .. "  " .. run.asset_name .. "\n",
      })
      H.eq(spawned(run), "curl,curl,curl", "the archive was extracted despite a bad digest")
      H.eq(table.concat(run.hashes, ","), "sha256sum", "the archive was not hashed")
      local err = failed(run)
      H.ok(contains(err, OTHER_DIGEST), "the expected digest is not reported")
      H.ok(contains(err, DIGEST), "the actual digest is not reported")
      cleaned_up(run)
    end)
  end)

  H.it("refuses a digest mismatch even when unverified downloads are allowed", function()
    -- allow_unverified waives a *missing* digest only. checksum_verdict pins
    -- that as a unit in spec_download_url; this pins that the flag is plumbed
    -- into it from opts and cannot turn a mismatch into an install.
    with_run({ allow_unverified = true }, function(run)
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }))
      run:drive(3, { code = 0, stdout = "", stderr = "" }, {
        [run.sums_file] = OTHER_DIGEST .. "  " .. run.asset_name .. "\n",
      })
      H.eq(spawned(run), "curl,curl,curl", "a mismatched archive was extracted")
      failed(run)
      cleaned_up(run)
    end)
  end)

  H.it("refuses and does not extract when the checksum cannot be computed", function()
    with_run({}, function(run)
      run.sha = { code = 1, stdout = "", stderr = "sha256sum: no such file" }
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }))
      run:drive(3, { code = 0, stdout = "", stderr = "" }, {
        [run.sums_file] = DIGEST .. "  " .. run.asset_name .. "\n",
      })
      H.eq(spawned(run), "curl,curl,curl", "extraction ran without a verified digest")
      failed(run)
      cleaned_up(run)
    end)
  end)

  H.it("extracts only once the digest matches", function()
    -- Verification happens before extraction, not after: everything downstream
    -- treats these bytes as trusted native code.
    with_run({}, function(run)
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }))
      run:drive(3, { code = 0, stdout = "", stderr = "" }, {
        [run.sums_file] = DIGEST .. "  " .. run.asset_name .. "\n",
      })
      H.eq(spawned(run), "curl,curl,curl,tar", "extraction did not follow a good digest")
      H.eq(argv(run, 4), "tar -xzf " .. run.temp_file .. " -C " .. run.temp_dir)
      H.eq(run.callbacks, 0, "decided before extracting")
      -- bughunt B56, pinned as it stands: the archive is unpacked with no
      -- path-containment or symlink check, and download_binary never verifies
      -- that tar/unzip exist (spec_setup pins the caller-side half of that,
      -- bughunt B28). Neither is fixed here.
    end)
  end)

  H.it("refuses an unchecksummed release by default, fetching and hashing nothing", function()
    with_run({}, function(run)
      to_archive(run, release_json({}))
      H.eq(spawned(run), "curl,curl", "something was fetched after the archive")
      H.eq(#run.hashes, 0, "hashed the archive with no digest to compare against")
      H.ok(contains(failed(run), run.asset_name), "the asset is not named")
      cleaned_up(run)
    end)
  end)

  H.it("extracts an unchecksummed release when allow_unverified is set", function()
    -- The end-to-end plumbing of setup({ allow_unverified_download = true })
    -- into this decision, which nothing guarded before.
    with_run({ allow_unverified = true }, function(run)
      to_archive(run, release_json({}))
      H.eq(spawned(run), "curl,curl,tar", "the escape hatch did not reach extraction")
      H.eq(#run.hashes, 0, "hashed the archive with no digest to compare against")
      H.eq(run.callbacks, 0, "decided before extracting")
    end)
  end)

  H.it("cleans up and reports when extraction fails", function()
    with_run({ allow_unverified = true }, function(run)
      to_archive(run, release_json({}))
      run:drive(3, { code = 2, stdout = "", stderr = "tar: unexpected EOF" })
      H.ok(contains(failed(run), "unexpected EOF"), "tar's stderr is not propagated")
      cleaned_up(run)
    end)
  end)

  H.it("cleans up and reports when the extracted binary is missing", function()
    with_run({ allow_unverified = true }, function(run)
      to_archive(run, release_json({}))
      run:drive(3, { code = 0, stdout = "", stderr = "" })
      H.ok(contains(failed(run), run.extracted), "the path it looked at is not reported")
      H.eq(vim.fn.filereadable(run.binary_path), 0, "installed something anyway")
      cleaned_up(run)
    end)
  end)

  H.it("installs the extracted library, records the version and cleans up", function()
    with_run({}, function(run)
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS" }))
      run:drive(3, { code = 0, stdout = "", stderr = "" }, {
        [run.sums_file] = DIGEST .. "  " .. run.asset_name .. "\n",
      })
      run:drive(4, { code = 0, stdout = "", stderr = "" }, { [run.extracted] = "native library bytes" })

      succeeded(run)
      H.eq(read_file(run.binary_path), "native library bytes", "the library was not installed")
      H.eq(#run.versions, 1, "the version file was not written")
      H.eq(run.versions[1].lines[1], VERSION, "recorded the wrong version")
      -- Pinned as-is, and worth knowing before the flatten: the version file's
      -- path comes from get_version_file_path(), which recomputes the library
      -- location from plugin_root() and ignores the binary_path argument
      -- download_binary was handed. In production the two agree; nothing here
      -- makes them agree.
      H.eq(run.versions[1].path, internal.get_version_file_path(), "version file path")
      H.eq(run.echoes, 0, "quiet on the happy path")
      cleaned_up(run)
    end)
  end)

  H.it("records the tag the release resolved to, not the one requested", function()
    with_run({}, function(run)
      to_archive(run, release_json({ sums_url = RELEASE_BASE .. "SHA256SUMS", tag = "v9.9.9" }))
      run:drive(3, { code = 0, stdout = "", stderr = "" }, {
        [run.sums_file] = DIGEST .. "  " .. run.asset_name .. "\n",
      })
      run:drive(4, { code = 0, stdout = "", stderr = "" }, { [run.extracted] = "bytes" })

      succeeded(run)
      H.eq(run.versions[1].lines[1], "9.9.9", "recorded the requested tag, not the resolved one")
      H.ok(run.echoes > 0, "the mismatch between requested and resolved was not surfaced")
    end)
  end)

  H.it("uses the zip asset and unzip for a windows target", function()
    with_run({
      target = WINDOWS,
      binary_name = "time_tracking_nvim.dll",
      allow_unverified = true,
    }, function(run)
      H.eq(run.asset_name, "time-tracking-nvim-" .. WINDOWS .. ".zip")
      to_archive(run, release_json({ target = WINDOWS }))
      H.eq(spawned(run), "curl,curl,unzip")
      H.ok(contains(argv(run, 2), run.asset_name), "the zip asset was not the one fetched")
      H.eq(argv(run, 3), "unzip -q -o " .. run.temp_file .. " -d " .. run.temp_dir)
    end)
  end)
end)

return H
