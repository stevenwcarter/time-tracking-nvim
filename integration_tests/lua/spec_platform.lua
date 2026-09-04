local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal

H.describe("normalize_os_name", function()
  H.it("maps libuv's Windows_NT to windows", function()
    -- uv.os_uname().sysname returns "Windows_NT" on Windows; lowercased that
    -- is "windows_nt", which was never a key in platform_mappings, so the
    -- shipped x86_64-pc-windows-msvc build was unreachable.
    H.eq(internal.normalize_os_name("windows_nt"), "windows")
  end)

  H.it("maps mingw and msys variants to windows", function()
    H.eq(internal.normalize_os_name("mingw64_nt-10.0"), "windows")
    H.eq(internal.normalize_os_name("msys_nt-10.0"), "windows")
  end)

  H.it("leaves linux and darwin alone", function()
    H.eq(internal.normalize_os_name("linux"), "linux")
    H.eq(internal.normalize_os_name("darwin"), "darwin")
  end)
end)

H.describe("get_platform_info", function()
  -- get_platform_info reads uv.os_uname() directly rather than taking it as
  -- a parameter, so these stub it out for the duration of `fn`. `uv` here is
  -- the same table object `init.lua` holds as its own local `uv`, so
  -- patching this field is visible to it too.
  local function with_uname(sysname, machine, fn)
    local uv = vim.uv or vim.loop
    local orig = uv.os_uname
    uv.os_uname = function()
      return { sysname = sysname, machine = machine, release = "test", version = "test" }
    end
    local ok, err = pcall(fn)
    uv.os_uname = orig
    if not ok then
      error(err, 0)
    end
  end

  H.it("maps Linux aarch64 to the aarch64-unknown-linux-gnu target", function()
    -- Regression: an unconditional aarch64 -> arm64 remap (meant for macOS)
    -- was applied to every platform, and platform_mappings.linux is keyed
    -- "aarch64", so a real Linux ARM64 machine got "Unsupported platform:
    -- linux-arm64" even though CI publishes aarch64-unknown-linux-gnu.
    with_uname("Linux", "aarch64", function()
      local info, err = internal.get_platform_info()
      H.ok(info, "expected a platform match, got error: " .. tostring(err))
      H.eq(info.target, "aarch64-unknown-linux-gnu")
    end)
  end)

  H.it("maps macOS arm64 to the aarch64-apple-darwin target", function()
    with_uname("Darwin", "arm64", function()
      local info, err = internal.get_platform_info()
      H.ok(info, "expected a platform match, got error: " .. tostring(err))
      H.eq(info.target, "aarch64-apple-darwin")
    end)
  end)
end)

return H
