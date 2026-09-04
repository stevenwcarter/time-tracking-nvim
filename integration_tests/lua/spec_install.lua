local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal
local uv = vim.uv or vim.loop

local function write(path, content)
  local f = assert(io.open(path, "wb"))
  f:write(content)
  f:close()
end

local function read(path)
  local f = assert(io.open(path, "rb"))
  local s = f:read("*a")
  f:close()
  return s
end

local function with_tmpdir(fn)
  local dir = vim.fn.tempname() .. "_install_spec"
  vim.fn.mkdir(dir, "p")
  local ok, err = pcall(fn, dir)
  vim.fn.delete(dir, "rf")
  if not ok then
    error(err, 0)
  end
end

H.describe("install_binary", function()
  H.it("replaces an existing library on a fresh inode", function()
    -- Regression: the updater used `cp src dest`, which truncates and rewrites
    -- the existing file in place, keeping its inode. macOS caches a file's
    -- code-signature blob against the vnode, so once the old library had been
    -- loaded since boot, every page of the new bytes failed validation and
    -- Neovim was SIGKILLed (exit 137, reason CODESIGNING "Invalid Page") on
    -- every launch until the file got a new inode.
    with_tmpdir(function(dir)
      local src = vim.fs.joinpath(dir, "new.dylib")
      local dest = vim.fs.joinpath(dir, "time_tracking_nvim.dylib")
      write(src, "new bytes")
      write(dest, "old bytes that were mapped by a previous nvim")
      local old_ino = uv.fs_stat(dest).ino

      local ok, err = internal.install_binary(src, dest)

      H.ok(ok, "install failed: " .. tostring(err))
      H.eq(read(dest), "new bytes")
      H.ok(uv.fs_stat(dest).ino ~= old_ino, "dest kept its old inode")
    end)
  end)

  H.it("preserves the source file's mode bits", function()
    with_tmpdir(function(dir)
      local src = vim.fs.joinpath(dir, "new.so")
      local dest = vim.fs.joinpath(dir, "time_tracking_nvim.so")
      write(src, "x")
      uv.fs_chmod(src, 493) -- 0755
      H.ok(internal.install_binary(src, dest))
      H.eq(bit.band(uv.fs_stat(dest).mode, 511), 493, "mode")
    end)
  end)

  H.it("leaves no temp file behind and creates dest when absent", function()
    with_tmpdir(function(dir)
      local src = vim.fs.joinpath(dir, "new.so")
      local dest = vim.fs.joinpath(dir, "time_tracking_nvim.so")
      write(src, "fresh")
      H.ok(internal.install_binary(src, dest))
      H.eq(read(dest), "fresh")
      local names = vim.fn.readdir(dir)
      table.sort(names)
      H.eq(table.concat(names, ","), "new.so,time_tracking_nvim.so")
    end)
  end)

  H.it("fails cleanly and leaves dest untouched when src is missing", function()
    with_tmpdir(function(dir)
      local dest = vim.fs.joinpath(dir, "time_tracking_nvim.so")
      write(dest, "old")
      local ok, err = internal.install_binary(vim.fs.joinpath(dir, "nope"), dest)
      H.eq(ok, false)
      H.ok(type(err) == "string" and #err > 0, "expected an error message")
      H.eq(read(dest), "old")
      H.eq(#vim.fn.readdir(dir), 1, "temp file left behind")
    end)
  end)
end)

return H
