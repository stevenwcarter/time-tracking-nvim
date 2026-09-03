local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal

H.describe("is_version_newer", function()
  H.it("reports a higher patch as newer", function()
    H.eq(internal.is_version_newer("0.1.4", "0.1.7"), true)
  end)

  H.it("reports equal versions as not newer", function()
    H.eq(internal.is_version_newer("0.1.7", "0.1.7"), false)
  end)

  H.it("reports a lower version as not newer", function()
    H.eq(internal.is_version_newer("0.1.7", "0.1.4"), false)
  end)

  H.it("tolerates a leading v on either side", function()
    H.eq(internal.is_version_newer("v0.1.4", "v0.1.7"), true)
  end)

  H.it("pads a shorter version with zeros", function()
    H.eq(internal.is_version_newer("0.1", "0.1.1"), true)
    H.eq(internal.is_version_newer("0.1.0", "0.1"), false)
  end)

  H.it("assumes newer when either side is nil", function()
    H.eq(internal.is_version_newer(nil, "0.1.7"), true)
    H.eq(internal.is_version_newer("0.1.7", nil), true)
  end)
end)

return H
