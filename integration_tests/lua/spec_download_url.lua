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

H.describe("checksum_verdict", function()
  -- The gate's whole decision, isolated from the network so all four
  -- combinations can be pinned. nil means "install"; a string is the refusal.

  H.it("allows installation when the digest matches", function()
    H.eq(internal.checksum_verdict("abc", "abc", false), nil)
  end)

  H.it("refuses a digest mismatch", function()
    local reason = internal.checksum_verdict("abc", "def", false)
    H.ok(reason, "a mismatch must be refused")
    H.ok(reason:match("mismatch"), "the reason must say why: " .. tostring(reason))
  end)

  H.it("refuses a digest mismatch even when unverified downloads are allowed", function()
    -- The asymmetry that makes the option safe: allow_unverified_download
    -- waives a *missing* digest only. A mismatch means the bytes are not the
    -- bytes that were published, which no opt-in may override.
    local reason = internal.checksum_verdict("abc", "def", true)
    H.ok(reason, "allow_unverified must not suppress a mismatch")
    H.ok(reason:match("mismatch"), "the reason must say why: " .. tostring(reason))
  end)

  H.it("refuses a missing digest by default", function()
    local reason = internal.checksum_verdict(nil, nil, false)
    H.ok(reason, "a missing SHA256SUMS must be refused when not opted in")
    H.ok(reason:match("SHA256SUMS"), "the reason must name the missing asset")
  end)

  H.it("allows a missing digest only when explicitly opted in", function()
    H.eq(internal.checksum_verdict(nil, nil, true), nil)
  end)

  H.it("refuses when the digest could not be computed", function()
    -- Defensive: a caller that reaches the verdict without an actual digest
    -- refuses rather than installs, in the same spirit as `opts and
    -- opts.allow_unverified`.
    H.ok(internal.checksum_verdict("abc", nil, true), "no actual digest must be refused")
  end)
end)

return H
