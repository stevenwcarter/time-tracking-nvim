local H = require("harness")
local tt = require("time-tracking-nvim")
local internal = tt._internal

H.describe("is_trusted_download_url", function()
  H.it("accepts a github.com release asset for this repo", function()
    H.eq(internal.is_trusted_download_url(
      "https://github.com/stevenwcarter/time-tracking-nvim/releases/download/v0.1.7/time-tracking-nvim-x86_64-unknown-linux-gnu.tar.gz"
    ), true)
  end)

  H.it("accepts an objects.githubusercontent.com URL", function()
    H.eq(internal.is_trusted_download_url(
      "https://objects.githubusercontent.com/github-production-release-asset/12345/abcdef"
    ), true)
  end)

  H.it("rejects a foreign host", function()
    H.eq(internal.is_trusted_download_url("https://evil.example/x.tar.gz"), false)
  end)

  H.it("rejects a different GitHub repo", function()
    H.eq(internal.is_trusted_download_url(
      "https://github.com/attacker/evil/releases/download/v1/x.tar.gz"
    ), false)
  end)

  H.it("rejects plain http", function()
    H.eq(internal.is_trusted_download_url(
      "http://github.com/stevenwcarter/time-tracking-nvim/releases/download/v0.1.7/x.tar.gz"
    ), false)
  end)

  H.it("rejects a value that curl would read as an option", function()
    -- The URL is the trailing argv element, so a leading dash is parsed as a
    -- flag: -K/home/user/.netrc makes curl read an attacker-chosen config.
    H.eq(internal.is_trusted_download_url("-K/home/user/.netrc"), false)
    H.eq(internal.is_trusted_download_url("--output/tmp/pwned"), false)
  end)

  H.it("rejects a host that merely contains a trusted name", function()
    H.eq(internal.is_trusted_download_url("https://github.com.evil.example/x.tar.gz"), false)
    H.eq(internal.is_trusted_download_url("https://notgithubusercontent.com/x"), false)
  end)

  H.it("rejects nil and non-strings", function()
    H.eq(internal.is_trusted_download_url(nil), false)
    H.eq(internal.is_trusted_download_url(42), false)
  end)
end)

return H
