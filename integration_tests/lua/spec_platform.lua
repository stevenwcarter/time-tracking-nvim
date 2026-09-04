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

return H
