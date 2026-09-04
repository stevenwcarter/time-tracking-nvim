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

H.describe("parse_sha256sums", function()
  H.it("parses sha256sum output keyed by basename", function()
    local sums = internal.parse_sha256sums(
      "abc123  time-tracking-nvim-x86_64-unknown-linux-gnu.tar.gz\n"
        .. "def456 *release-assets/time-tracking-nvim-x86_64-pc-windows-msvc.zip\n"
    )
    H.eq(sums["time-tracking-nvim-x86_64-unknown-linux-gnu.tar.gz"], "abc123")
    H.eq(sums["time-tracking-nvim-x86_64-pc-windows-msvc.zip"], "def456")
  end)

  H.it("ignores blank and malformed lines", function()
    local sums = internal.parse_sha256sums("\nnot a checksum line\n\n")
    H.eq(next(sums), nil)
  end)
end)

H.describe("file_sha256", function()
  H.it("matches the digest sha256sum computes", function()
    local path = vim.fn.tempname()
    vim.fn.writefile({ "hello" }, path)
    local digest = internal.file_sha256(path)
    vim.fn.delete(path)
    -- Independently verified with: printf 'hello\n' | sha256sum
    H.eq(digest, "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03")
  end)

  H.it("matches sha256sum for binary content", function()
    -- The "hello" fixture above round-trips through readfile/writefile
    -- unchanged, so it cannot catch an implementation that reads the archive
    -- into Lua instead of shelling out. Binary content can: NUL bytes, a
    -- high byte and no trailing newline all survive sha256sum and do not
    -- survive a readfile/writefile round-trip.
    local path = vim.fn.tempname()
    local f = assert(io.open(path, "wb"))
    f:write("\000\001\002\255binary\000no-trailing-newline")
    f:close()
    local reference = vim.system({ "sha256sum", "--", path }, { text = true }):wait()
    local expected = tostring(reference.stdout):match("%x%x%x%x%x%x%x%x%x+")
    H.ok(expected, "sha256sum must be available to establish the reference digest")
    local digest = internal.file_sha256(path)
    vim.fn.delete(path)
    H.eq(digest, expected)
  end)
end)

return H
