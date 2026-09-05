-- Time Tracking Neovim Plugin
-- Main initialization module

local M = {}
local uv = vim.uv or vim.loop

-- All user-facing messages go through here.
--
-- Every call site previously passed `history = false`, so nothing was
-- retrievable with :messages — and these all fire during setup() at startup,
-- where they scroll off within milliseconds. Worse, a message taller than one
-- line triggers Neovim's hit-enter prompt, so the multi-line failure blocks
-- stopped every launch with a wall of text the user then could not recall.
--
-- opts.transient = true keeps a progress notice out of the history.
local function echo(chunks, opts)
	opts = opts or {}
	vim.api.nvim_echo(chunks, not opts.transient, {})
end

-- Plugin version (should match Cargo.toml)
local PLUGIN_VERSION = "0.2.1"

local REPO = "stevenwcarter/time-tracking-nvim"
local RELEASES_URL = "https://github.com/" .. REPO .. "/releases"
local API_BASE = "https://api.github.com/repos/" .. REPO .. "/releases"

-- Default configuration
local default_config = {
	auto_download = true, -- Automatically download binaries if missing
	auto_update = true, -- Automatically update binary when plugin version changes
	-- Escape hatch for releases published before SHA256SUMS existed (<= v0.1.7)
	-- and for air-gapped mirrors. Leaving this false means a downloaded native
	-- library is never dlopen'd without matching a published digest.
	allow_unverified_download = false,
}

-- Add the binary directory to Lua's cpath
local function add_to_cpath(binary_path)
	local binary_dir = vim.fs.dirname(binary_path)
	local ext = vim.fn.fnamemodify(binary_path, ":e")
	local pattern = string.format("%s/?.%s", binary_dir, ext)

	-- Check if already in cpath (escape special characters for pattern matching)
	local escaped_pattern = pattern:gsub("([%.%-%+%[%]%(%)%^%$])", "%%%1")
	if not package.cpath:find(escaped_pattern, 1, true) then
		package.cpath = package.cpath .. ";" .. pattern
	end
end

-- Target triple and library extension per OS/arch. One of four places this
-- mapping lives; see the comment on `normalize_arch` and T23's pointers.
local PLATFORM_MAPPINGS = {
	linux = {
		x86_64 = { target = "x86_64-unknown-linux-gnu", ext = "so" },
		aarch64 = { target = "aarch64-unknown-linux-gnu", ext = "so" },
	},
	darwin = {
		x86_64 = { target = "x86_64-apple-darwin", ext = "dylib" },
		arm64 = { target = "aarch64-apple-darwin", ext = "dylib" },
	},
	windows = {
		x86_64 = { target = "x86_64-pc-windows-msvc", ext = "dll" },
	},
}

-- Normalize libuv's sysname to the keys used in PLATFORM_MAPPINGS.
-- uv.os_uname() mimics uname, so Windows reports "Windows_NT" (and MSYS/MinGW
-- shells report "MINGW64_NT-…"/"MSYS_NT-…"), none of which is "windows".
local function normalize_os_name(os_name)
	if os_name:match("^windows") or os_name:match("^mingw") or os_name:match("^msys") then
		return "windows"
	end
	return os_name
end

-- Fold alternative architecture spellings onto the keys PLATFORM_MAPPINGS uses.
--
-- macOS's own `uname -m` already reports "arm64", so the darwin remap is a
-- no-op there in practice; it exists only to tolerate a uname variant that
-- reports "aarch64" instead. It is scoped to darwin because
-- PLATFORM_MAPPINGS.linux is keyed "aarch64" (Linux's own uname -m spelling) —
-- applying it unconditionally made Linux aarch64 unreachable: it got remapped
-- to "arm64", which is not a key in the linux table, and the lookup failed with
-- "Unsupported platform: linux-arm64".
local function normalize_arch(os_name, arch)
	if arch == "amd64" then
		arch = "x86_64"
	end
	if os_name == "darwin" and arch == "aarch64" then
		arch = "arm64"
	end
	return arch
end

-- Get platform-specific information
local function get_platform_info()
	local uname = uv.os_uname()
	local os_name = normalize_os_name(uname.sysname:lower())
	local arch = normalize_arch(os_name, uname.machine:lower())

	local platform = PLATFORM_MAPPINGS[os_name]
	if not platform or not platform[arch] then
		return nil, string.format("Unsupported platform: %s-%s", os_name, arch)
	end

	return platform[arch], nil
end

-- The plugin's root directory: three levels up from this file
-- (lua/time-tracking-nvim/init.lua). debug.getinfo(1, "S") describes the
-- currently *running* function, i.e. this one — its source is always
-- init.lua regardless of which module calls plugin_root(), so exposing it
-- through M._internal for health.lua to call is safe.
local function plugin_root()
	local info = debug.getinfo(1, "S")
	return vim.fn.fnamemodify(info.source:sub(2), ":h:h:h")
end

-- Get the path where the binary should be located
local function get_binary_path()
	local root = plugin_root()
	local platform_info, err = get_platform_info()

	if not platform_info then
		return nil, err
	end

	local binary_name = "time_tracking_nvim." .. platform_info.ext
	return vim.fs.joinpath(root, "lua", binary_name), platform_info.target
end

-- Get version file path (stores the version of the downloaded binary)
local function get_version_file_path()
	local binary_path = get_binary_path()
	if not binary_path then
		return nil
	end
	return binary_path .. ".version"
end

-- Read version from version file
local function read_binary_version()
	local version_file = get_version_file_path()
	if not version_file or vim.fn.filereadable(version_file) ~= 1 then
		return nil
	end

	local content = vim.fn.readfile(version_file)
	if #content > 0 then
		return vim.trim(content[1])
	end
	return nil
end

-- Write version to version file
local function write_binary_version(version)
	local version_file = get_version_file_path()
	if not version_file then
		return false
	end

	-- Ensure directory exists
	local dir = vim.fs.dirname(version_file)
	vim.fn.mkdir(dir, "p")

	local success = pcall(vim.fn.writefile, { version }, version_file)
	return success
end

-- Parse a semantic version into numeric parts, tolerating a leading "v".
local function parse_semver(s)
	s = s:gsub("^v", "")
	local parts = {}
	for part in s:gmatch("([^%.]+)") do
		table.insert(parts, tonumber(part) or 0)
	end
	return parts
end

-- Compare version strings (basic semver comparison)
local function is_version_newer(current, new)
	if not current or not new then
		return true -- Assume newer if we can't compare
	end

	local current_parts = parse_semver(current)
	local new_parts = parse_semver(new)

	for i = 1, math.max(#current_parts, #new_parts) do
		local a, b = current_parts[i] or 0, new_parts[i] or 0
		if a ~= b then
			return b > a
		end
	end

	return false -- Versions are equal
end

-- Shared curl hardening for both the API call and the archive download.
--   --proto/--proto-redir =https  : curl's default redirect protocol set
--                                   includes plain HTTP, so a 302 to http://
--                                   would fetch the library we are about to
--                                   dlopen in cleartext.
--   <fail flag>                   : without -f/--fail-with-body curl exits 0
--                                   on an HTTP error, so a 403 rate-limit body
--                                   parsed as JSON and surfaced as "unsupported
--                                   platform". See `curl_fail_flag` below for
--                                   which of the two this resolves to.
--   --max-time/--connect-timeout  : a black-holed connection otherwise left
--                                   the callback pending forever.
local CURL_HARDENING = {
	"--proto",
	"=https",
	"--proto-redir",
	"=https",
	"--tlsv1.2",
	"--max-redirs",
	"5",
	"--connect-timeout",
	"10",
	"--max-time",
	"60",
	"--retry",
	"2",
}

-- Whether this curl build understands `--fail-with-body` (added in curl
-- 7.76.0, April 2021). RHEL 8 ships 7.61 and Ubuntu 20.04 ships 7.68 — on
-- those, curl treats it as an unrecognized option and exits 2 during option
-- parsing, before a single byte is sent, so auto-download would stop working
-- outright rather than degrading. Probed once, as a local subprocess spawn
-- with no network access, and cached: `-f` is decades old and gives the same
-- fail-on-error behaviour, at the cost of discarding the error body that
-- `--fail-with-body` keeps on stdout (see the exit-code handling in
-- `download_binary`, which accounts for that difference).
local curl_fail_flag_cache = nil
local function curl_fail_flag()
	if curl_fail_flag_cache == nil then
		local probe = vim.system({ "curl", "--fail-with-body", "--version" }, {}):wait(2000)
		curl_fail_flag_cache = (probe.code == 0) and "--fail-with-body" or "-f"
	end
	return curl_fail_flag_cache
end

-- Build a curl argv: {"curl", <hardening>, <fail flag>, unpack(extra)}
local function curl_cmd(extra)
	local cmd = { "curl" }
	vim.list_extend(cmd, CURL_HARDENING)
	table.insert(cmd, curl_fail_flag())
	vim.list_extend(cmd, extra)
	return cmd
end

-- Constrain where a release asset may be fetched from.
--
-- asset.browser_download_url is taken verbatim out of the API response and
-- handed to curl, so without this the only containment is that asset.name
-- string-matched — a tampered response pointing at any host would be fetched
-- and dlopen'd. Anchored patterns, so a host merely *containing* a trusted
-- name (github.com.evil.example) does not pass.
--
-- The two branches differ in how tight that containment is: the github.com
-- branch is anchored to this repo's own path, but the *.githubusercontent.com
-- branch accepts *any* host under that suffix with *any* path — e.g.
-- raw.githubusercontent.com/<anyone>/<anything> passes, not just assets for
-- this repo. That is acceptable only because this URL is never
-- attacker-supplied directly: it comes out of GitHub's own release API
-- response for this repo, and the only way to smuggle a different host past
-- the github.com branch is to already control that API response — at which
-- point the attacker could equally point browser_download_url at
-- objects.githubusercontent.com for their own malicious asset. This is not a
-- general-purpose "is this URL safe" check and must not be reused as one
-- without tightening the second branch.
local function is_trusted_download_url(url)
	if type(url) ~= "string" then
		return false
	end

	local host = url:match("^https://([%w%.%-]+)/")
	if not host then
		return false
	end

	if host == "github.com" then
		return url:match("^https://github%.com/stevenwcarter/time%-tracking%-nvim/") ~= nil
	end

	return host:match("%.githubusercontent%.com$") ~= nil
end

-- A complete SHA-256, bounded so it cannot latch onto a longer or shorter
-- hex run: 64 hex characters with a non-hex character (or a string edge) on
-- either side.
local SHA256_HEX_PATTERN = "%f[%x]" .. string.rep("%x", 64) .. "%f[%X]"

-- `:wait()` below blocks Neovim's event loop, so a wedged checksum binary
-- would hang the editor unrecoverably rather than just this download. Hashing
-- even a large archive takes milliseconds; this is generous headroom, not a
-- realistic expected duration. `SystemObj:wait(timeout)` force-kills the
-- process and returns code 124 on timeout, which the `out.code ~= 0` check
-- below already treats as a checksum failure — a fail-closed refusal, same as
-- any other command failure here.
local SHA256_TIMEOUT_MS = 5000

-- Compute the SHA-256 of a file.
--
-- Prefers a subprocess over reading the file into Lua: readfile/writefile
-- round-trips are lossy for binary content, and the digest has to match
-- byte-for-byte what sha256sum computed in CI.
local function file_sha256(path)
	local out
	if vim.fn.executable("sha256sum") == 1 then
		out = vim.system({ "sha256sum", "--", path }, { text = true }):wait(SHA256_TIMEOUT_MS)
	elseif vim.fn.executable("shasum") == 1 then
		out = vim.system({ "shasum", "-a", "256", "--", path }, { text = true }):wait(SHA256_TIMEOUT_MS)
	elseif vim.fn.executable("certutil") == 1 then
		out = vim.system({ "certutil", "-hashfile", path, "SHA256" }, { text = true }):wait(SHA256_TIMEOUT_MS)
	else
		return nil, "no SHA-256 implementation available (need sha256sum, shasum or certutil)"
	end

	if not out or out.code ~= 0 then
		return nil, "checksum command failed: " .. tostring(out and out.stderr or "?")
	end

	-- Anchor to a full 64-character SHA-256 rather than "nine or more hex
	-- characters": certutil echoes the file path in its transcript, so a path
	-- like C:\Users\deadbeef1234\x.tmp contains a hex run that would otherwise
	-- be read as the digest. Both outcomes are fail-closed refusals, never a
	-- false accept, but the anchor removes a spurious-refusal class.
	-- sha256sum/shasum are unaffected: their digest is first on the line.
	local digest = tostring(out.stdout):match(SHA256_HEX_PATTERN)
	if not digest then
		return nil, "could not parse checksum output"
	end
	return digest:lower()
end

-- Parse `sha256sum` output into { [basename] = digest }.
local function parse_sha256sums(text)
	local sums = {}
	for line in tostring(text):gmatch("[^\r\n]+") do
		local digest, name = line:match("^(%x+)%s+%*?(%S+)$")
		if digest and name then
			sums[vim.fs.basename(name)] = digest:lower()
		end
	end
	return sums
end

-- Decide whether a downloaded archive may be installed.
--
-- Pure, so the security decision is testable without a network: returns nil
-- when installation is allowed, or a reason string explaining the refusal.
--
-- allow_unverified waives a *missing* digest only. A mismatch is refused
-- unconditionally — it means the bytes on disk are not the bytes that were
-- published, which no opt-in may override.
local function checksum_verdict(expected_digest, actual_digest, allow_unverified)
	if expected_digest then
		if not actual_digest then
			return "Could not compute the archive's checksum - refusing to install"
		end
		if actual_digest ~= expected_digest then
			return string.format(
				"Checksum mismatch (expected %s, got %s) - refusing to install",
				expected_digest,
				actual_digest
			)
		end
		return nil
	end

	if allow_unverified then
		return nil
	end

	return "No SHA256SUMS published for this release, so the binary cannot be "
		.. "verified. Releases up to v0.1.7 predate checksums. To install "
		.. "anyway, use setup({ allow_unverified_download = true })."
end

-- Install a downloaded library at `dest` on a fresh inode.
--
-- This used to be `cp extracted dest`, which truncates and rewrites the
-- existing file in place, keeping its inode. macOS caches a file's
-- code-signature blob against the vnode. Once the previous library had been
-- loaded by any Neovim since boot, overwriting its bytes left that stale
-- blob in place, every page of the new library failed validation, and each
-- Neovim launch was SIGKILLed with termination reason CODESIGNING "Invalid
-- Page": exit 137, nothing on screen, nothing in nvim.log, on every launch
-- until the vnode was evicted. Copying to a sibling temp name and renaming
-- over `dest` gives the new bytes a new inode, so the kernel reads the new
-- signature. rename(2) replaces atomically on POSIX; libuv uses
-- MOVEFILE_REPLACE_EXISTING on Windows. fs_copyfile preserves mode bits.
local function install_binary(src, dest)
	local tmp = string.format("%s.%d.tmp", dest, uv.os_getpid())
	local ok, err = uv.fs_copyfile(src, tmp)
	if not ok then
		return false, "copy failed: " .. tostring(err)
	end
	ok, err = uv.fs_rename(tmp, dest)
	if not ok then
		uv.fs_unlink(tmp)
		return false, "rename failed: " .. tostring(err)
	end
	return true
end

-- The download is a chain of asynchronous phases -- fetch the release, pick its
-- assets, fetch the archive, resolve the published digest, verify, extract and
-- install, record the version -- and each one gets its own function below so
-- that `download_binary` at the end of the chain reads as the order they run
-- in, rather than as the nesting of the callbacks that sequence them.
--
-- Every phase that can fail once the scratch directory exists reports through
-- `fail`, so cleanup is one line at each site instead of a repeated triple.

-- Abandon a download in progress: remove the scratch directory and report the
-- failure. `callback` is download_binary's own (ok, message) callback, or the
-- one a phase was handed.
local function fail(temp_dir, callback, msg)
	vim.fn.delete(temp_dir, "rf")
	callback(false, msg)
end

-- Interpret the release-metadata response: the decoded release table, or nil
-- and a reason to give up. Pure.
local function decode_release(result)
	local ok, release_info = pcall(vim.json.decode, result.stdout)

	-- What the guards below quote back when they have nothing better to report.
	local body = tostring(result.stdout):sub(1, 200)

	-- curl's fail flag (`--fail-with-body`, or the old-curl fallback `-f` — see
	-- `curl_fail_flag`) makes curl exit non-zero on an HTTP error, but
	-- `--fail-with-body` still writes the response body to stdout. So a 403/404
	-- both fails this exit-code check *and* decodes to a table — treat that case
	-- as "keep going below", where the guards produce a far better message
	-- ("GitHub API error: API rate limit exceeded…") than the bare exit code
	-- would. Only bail out here when there is truly nothing to decode: a
	-- network/TLS failure, or the `-f` fallback, which discards the body along
	-- with the exit code.
	if result.code ~= 0 and not (ok and type(release_info) == "table") then
		local reason = result.stderr
		if not reason or reason == "" then
			reason = body
		end
		if not reason or reason == "" then
			reason = "curl exited with code " .. tostring(result.code)
		end
		return nil, "Failed to fetch release info: " .. reason
	end

	if not ok then
		return nil, "Failed to parse release info"
	end

	-- A rate-limited or errored API response decodes to valid JSON with no
	-- `assets` field, which used to fall through to "No binary found for
	-- target: …" — telling the user their platform is unsupported when they
	-- were merely rate-limited (60 req/hr per IP, routine on NAT).
	if type(release_info) ~= "table" then
		return nil, "Unexpected GitHub API response: " .. body
	end
	if release_info.message then
		return nil, "GitHub API error: " .. tostring(release_info.message)
	end
	if type(release_info.assets) ~= "table" then
		return nil, "GitHub API response had no assets (rate limited or malformed): " .. body
	end

	return release_info
end

-- Pick the assets to fetch for `target` out of a release: the platform archive
-- and, when the release published one, its SHA256SUMS.
--
-- Pure, so the trust check on the archive URL — the only thing standing between
-- a tampered API response and a dlopen of whatever it names — is decided
-- without a network. Expects a release already validated by decode_release.
--
-- Returns download_url, sums_url, asset_name, or three nils and a refusal.
local function select_assets(release_info, target)
	local asset_name = string.format("time-tracking-nvim-%s.tar.gz", target)
	if target:match("windows") then
		asset_name = string.format("time-tracking-nvim-%s.zip", target)
	end

	local download_url, sums_url
	for _, asset in ipairs(release_info.assets) do
		if asset.name == asset_name then
			download_url = asset.browser_download_url
		elseif asset.name == "SHA256SUMS" then
			sums_url = asset.browser_download_url
		end
	end

	if not download_url then
		return nil, nil, nil, "No binary found for target: " .. target
	end
	if not is_trusted_download_url(download_url) then
		return nil, nil, nil, "Refusing untrusted download URL: " .. tostring(download_url)
	end

	return download_url, sums_url, asset_name
end

-- Fetch a release's metadata from the GitHub API.
-- cb(release_info) on success, cb(nil, reason) on failure.
local function fetch_release(release_url, cb)
	-- -S (in addition to -s) so a hard failure below still has a real curl
	-- error on stderr instead of an empty string; -s alone suppresses it.
	local cmd = curl_cmd({ "-L", "-s", "-S", release_url })

	vim.system(cmd, {}, function(result)
		vim.schedule(function()
			local release_info, err = decode_release(result)
			cb(release_info, err)
		end)
	end)
end

-- Download `url` to `path`. cb(nil) on success, cb(stderr) on failure.
--
-- curl can fail with nothing on stderr, so that failure string may be empty;
-- Lua's only false values are nil and false, so `if err then` at the call sites
-- still reads an empty stderr as the failure it is.
local function fetch_file(url, path, cb)
	local cmd = curl_cmd({ "-L", "-o", path, "--", url })

	vim.system(cmd, {}, function(result)
		vim.schedule(function()
			if result.code ~= 0 then
				cb(result.stderr or "")
			else
				cb(nil)
			end
		end)
	end)
end

-- Fetch the release's SHA256SUMS into `temp_dir` and pick out the digest
-- published for `asset_name`. cb(digest) on success, cb(nil, reason) on failure.
local function fetch_sums(sums_url, temp_dir, asset_name, cb)
	local sums_file = vim.fs.joinpath(temp_dir, "SHA256SUMS")

	fetch_file(sums_url, sums_file, function(err)
		-- curl claiming success with no file on disk is refused exactly as an
		-- outright failure is: there is nothing to verify against either way.
		if err or vim.fn.filereadable(sums_file) ~= 1 then
			return cb(nil, "Could not download SHA256SUMS: " .. (err or ""))
		end

		local sums = parse_sha256sums(table.concat(vim.fn.readfile(sums_file), "\n"))
		local digest = sums[asset_name]
		if not digest then
			-- Decided here, before checksum_verdict is ever consulted, and so
			-- outside allow_unverified's reach: that opt-in waives a release
			-- that published no checksums at all, never an asset missing from
			-- the ones it did publish.
			return cb(nil, "SHA256SUMS has no entry for " .. asset_name)
		end

		cb(digest)
	end)
end

-- Decide whether the downloaded archive may be installed, hashing it only when
-- the release published a digest to compare against. Returns nil to allow the
-- install, or the reason for refusing it.
local function verify_archive(temp_file, asset_name, expected_digest, allow_unverified)
	local actual
	if expected_digest then
		local digest_err
		actual, digest_err = file_sha256(temp_file)
		if not actual then
			return "Could not compute checksum: " .. tostring(digest_err)
		end
	end

	local refusal = checksum_verdict(expected_digest, actual, allow_unverified)
	if refusal then
		return asset_name .. ": " .. refusal
	end

	return nil
end

-- Unpack the verified archive and move the library into place. cb(true) on
-- success, cb(false, reason) on failure; the scratch directory is gone by the
-- time cb runs either way, so callers must not clean up after it.
--
-- The archive is unpacked exactly as published: no path-containment or symlink
-- check, and no proof that tar or unzip exist at all (bughunt B56 and B28).
-- Both are preserved here as they stand.
local function extract_and_install(temp_file, temp_dir, binary_path, asset_name, cb)
	local extract_cmd
	if asset_name:match("%.zip$") then
		extract_cmd = { "unzip", "-q", "-o", temp_file, "-d", temp_dir }
	else
		extract_cmd = { "tar", "-xzf", temp_file, "-C", temp_dir }
	end

	vim.system(extract_cmd, {}, function(extract_result)
		vim.schedule(function()
			if extract_result.code ~= 0 then
				return fail(temp_dir, cb, "Failed to extract binary: " .. (extract_result.stderr or ""))
			end

			local extracted_binary = vim.fs.joinpath(temp_dir, "target", "release", vim.fs.basename(binary_path))
			if vim.fn.filereadable(extracted_binary) ~= 1 then
				return fail(temp_dir, cb, "Extracted binary not found at: " .. extracted_binary)
			end

			local installed, install_err = install_binary(extracted_binary, binary_path)
			-- Unconditional, and before the result is checked: the scratch
			-- directory is finished with whether or not the install worked.
			vim.fn.delete(temp_dir, "rf")

			if not installed then
				return cb(false, "Failed to install binary to target location: " .. tostring(install_err))
			end
			cb(true)
		end)
	end)
end

-- Record the tag the release actually resolved to, not the one we asked for:
-- with a pinned plugin tag these can differ, and recording the request made
-- every later version comparison a no-op.
--
-- Takes no path, because write_binary_version accepts none: it recomputes the
-- destination from plugin_root() rather than from the binary_path this download
-- installed to. In production the two agree.
local function record_version(release_info, expected_version)
	local resolved_tag = release_info.tag_name
	local version_to_store = resolved_tag and (resolved_tag:gsub("^v", "")) or "unknown"

	if expected_version and version_to_store ~= expected_version then
		echo({
			{ "time-tracking-nvim: ", "WarningMsg" },
			{
				string.format(
					"requested v%s but the release resolved to %s; recording %s",
					expected_version,
					tostring(resolved_tag),
					version_to_store
				),
				"Normal",
			},
		})
	end

	if not write_binary_version(version_to_store) then
		-- Not a fatal error, just warn
		echo({
			{ "time-tracking-nvim: ", "WarningMsg" },
			{ "Warning: Could not save version info", "Normal" },
		})
	end
end

-- Download the native library for `target` from GitHub releases and install it
-- at `binary_path`.
--
-- `expected_version` pins which release to ask for, and `opts.allow_unverified`
-- waives a release that published no checksums. `callback(ok, message)` fires
-- exactly once, from whichever phase decides the outcome.
local function download_binary(target, binary_path, callback, expected_version, opts)
	-- Ask for the release we actually want. Falling back to /latest only when
	-- no version was requested: previously this always fetched /latest and then
	-- recorded expected_version, so the .version file was an assertion about
	-- what we wanted rather than an observation of what we got.
	local release_url = expected_version and (API_BASE .. "/tags/v" .. expected_version) or (API_BASE .. "/latest")

	fetch_release(release_url, function(release_info, release_err)
		if release_err then
			return callback(false, release_err)
		end

		local download_url, sums_url, asset_name, asset_err = select_assets(release_info, target)
		if asset_err then
			return callback(false, asset_err)
		end

		-- Nothing is written to disk on behalf of a release we have refused, so
		-- both directories are created only once the archive URL is trusted.
		-- Safe to do here: fetch_release scheduled this continuation onto the
		-- main loop, where vim.fn is allowed.
		vim.fn.mkdir(vim.fs.dirname(binary_path), "p")
		local temp_dir = vim.fn.tempname() .. "_time_tracking"
		vim.fn.mkdir(temp_dir, "p")
		local temp_file = vim.fs.joinpath(temp_dir, asset_name)
		local allow_unverified = opts and opts.allow_unverified

		-- Both routes to a digest converge here: fetch_sums resolved this
		-- asset's line in a published SHA256SUMS, or the release published none
		-- and there is nothing to compare against.
		--
		-- Verify BEFORE extracting: everything downstream — extract, copy into
		-- lua/, and the pcall(require, …) that dlopens it — treats these bytes
		-- as trusted native code.
		local function verify_and_install(expected_digest, sums_err)
			if sums_err then
				return fail(temp_dir, callback, sums_err)
			end

			local refusal = verify_archive(temp_file, asset_name, expected_digest, allow_unverified)
			if refusal then
				return fail(temp_dir, callback, refusal)
			end

			extract_and_install(temp_file, temp_dir, binary_path, asset_name, function(ok, install_err)
				if not ok then
					-- Reported straight through rather than through fail():
					-- extract_and_install has already cleaned up.
					return callback(false, install_err)
				end

				record_version(release_info, expected_version)
				callback(true, "Binary downloaded successfully")
			end)
		end

		fetch_file(download_url, temp_file, function(download_err)
			if download_err then
				return fail(temp_dir, callback, "Failed to download binary: " .. download_err)
			end

			if not sums_url then
				return verify_and_install(nil)
			end

			if not is_trusted_download_url(sums_url) then
				-- A SHA256SUMS URL that fails the host allowlist is a tampering
				-- signal, not a missing asset. Reporting it as "no checksums
				-- published" would nudge the user to set
				-- allow_unverified_download in response to an attack, so refuse
				-- outright and do not mention the escape hatch.
				return fail(temp_dir, callback, "Refusing untrusted SHA256SUMS URL: " .. tostring(sums_url))
			end

			fetch_sums(sums_url, temp_dir, asset_name, verify_and_install)
		end)
	end)
end

-- Decide what setup() must do about the library on disk.
--
-- A binary with no .version file beside it predates version tracking, so it is
-- classified exactly like a genuine mismatch: both need an update, and both
-- carry a reason string that setup() puts in front of the user.
--
-- Returns:
--   binary_exists  the library is readable at binary_path
--   needs_update   its recorded version and PLUGIN_VERSION disagree
--   update_reason  why, for the message; "" when no update is needed
local function classify_binary_state(binary_path)
	if vim.fn.filereadable(binary_path) ~= 1 then
		return false, false, ""
	end

	local binary_version = read_binary_version()
	if not binary_version then
		return true, true, "no version information found (updating to track versions)"
	end

	if binary_version == PLUGIN_VERSION then
		return true, false, ""
	end

	local reason = string.format("version mismatch (plugin: %s, binary: %s)", PLUGIN_VERSION, binary_version)
	return true, true, reason
end

-- Load the native module and classify the outcome.
--
-- The module can fail in two distinct ways that callers must report
-- differently: the shared library may not load at all, or it may load and
-- then report an initialization failure through its `error` key.
--
-- Returns status ("ok" | "load_failed" | "init_failed") and a second value:
-- the module on "ok", otherwise the error value.
local function load_native()
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		return "load_failed", native
	end
	if type(native) == "table" and native.error then
		return "init_failed", native.error
	end
	return "ok", native
end

-- Are the external tools auto-download needs present?
--
-- `fatal` separates the two callers. A binary that is missing cannot be
-- recovered without curl, so that caller refuses and points at a manual
-- install; a binary that is merely out of date can still be loaded, so that
-- caller only warns.
--
-- `fatal` also gates the extractor check, because the update path has never
-- looked at tar or unzip: it starts downloads it may be unable to unpack.
-- That is bughunt B28, preserved here as it stands rather than fixed.
--
-- Returns true when the download may go ahead, or false and the echo chunks
-- describing what is missing.
local function have_download_tools(fatal)
	if vim.fn.executable("curl") ~= 1 then
		if not fatal then
			return false,
				{
					{ "time-tracking-nvim: ", "ErrorMsg" },
					{ "curl is required for auto-update but not found", "Normal" },
					{ "\nUsing existing binary, but it may be incompatible", "WarningMsg" },
				}
		end
		return false,
			{
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "curl is required for auto-download but not found", "Normal" },
				{ "\nPlease install curl or download manually from: ", "Normal" },
				{ RELEASES_URL, "Underlined" },
			}
	end

	if fatal and vim.fn.executable("tar") ~= 1 and vim.fn.executable("unzip") ~= 1 then
		return false,
			{
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "tar or unzip is required for auto-download but not found", "Normal" },
				{ "\nPlease install tar/unzip or download manually from: ", "Normal" },
				{ RELEASES_URL, "Underlined" },
			}
	end

	return true
end

-- The two label tables below hold everything that differs between fetching a
-- binary that is missing and replacing one that is out of date: the wording of
-- every message, and `fatal`, the one behavioural difference. Keeping the
-- severity in the same table as the wording is what stops the pair drifting
-- apart again.

-- First install. There is no binary to fall back to, so missing tooling is
-- fatal and a failed download ends with manual installation instructions.
local function install_labels(target, binary_path)
	return {
		fatal = true,
		progress = "Binary not found, downloading for " .. target .. "...",
		downloaded = "Binary downloaded successfully!",
		loaded = "Plugin loaded successfully!",
		load_failed = "Failed to load native module after download: ",
		load_hint = "\nPlease check the binary permissions and try restarting Neovim",
		failed = function(message)
			return {
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "Auto-download failed: ", "Normal" },
				{ message, "ErrorMsg" },
				{ "\n\nManual installation instructions:", "Normal" },
				{ "\n1. Go to: ", "Normal" },
				{ RELEASES_URL, "Underlined" },
				{ "\n2. Download: ", "Normal" },
				{ "time-tracking-nvim-" .. target .. (target:match("windows") and ".zip" or ".tar.gz"), "String" },
				{ "\n3. Extract to: ", "Normal" },
				{ vim.fs.dirname(binary_path), "Directory" },
			}
		end,
	}
end

-- Update. The binary on disk still works well enough to load, so nothing on
-- this path is fatal and every failure ends by saying so.
local function update_labels(update_reason)
	return {
		fatal = false,
		progress = "Binary update needed (" .. update_reason .. "), downloading...",
		downloaded = "Binary updated successfully!",
		loaded = "Plugin updated and loaded successfully!",
		load_failed = "Failed to load native module after update: ",
		load_hint = "\nPlease restart Neovim",
		failed = function(message)
			return {
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "Auto-update failed: ", "Normal" },
				{ message, "ErrorMsg" },
				{ "\nUsing existing binary, but it may be incompatible", "WarningMsg" },
			}
		end,
	}
end

-- Download the native library and, once it arrives, add it to cpath and load
-- it. `labels` is one of the two tables above, and is the only thing that
-- separates the install path from the update path.
--
-- Returns true when a download was actually started. The update caller reads
-- that to decide whether to fall through to the binary it already has; the
-- install caller has nothing to fall through to and ignores it.
--
-- The download is asynchronous: this returns as soon as one is under way, and
-- everything inside the callback runs long after setup() has returned.
local function download_then_load(target, binary_path, config, labels)
	echo({
		{ "time-tracking-nvim: ", "Title" },
		{ labels.progress, "Normal" },
	}, { transient = true })

	local ok, missing = have_download_tools(labels.fatal)
	if not ok then
		echo(missing)
		return false
	end

	download_binary(target, binary_path, function(success, message)
		if not success then
			echo(labels.failed(message))
			return
		end

		echo({
			{ "time-tracking-nvim: ", "MoreMsg" },
			{ labels.downloaded, "Normal" },
		})

		-- cpath first: load_native cannot find a library that is not on it.
		add_to_cpath(binary_path)

		local status, value = load_native()
		if status == "load_failed" then
			echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ labels.load_failed, "Normal" },
				{ value, "ErrorMsg" },
				{ labels.load_hint, "Normal" },
			})
		elseif status == "init_failed" then
			echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "Loaded but failed to initialize: " .. tostring(value), "Normal" },
			})
		else
			echo({
				{ "time-tracking-nvim: ", "MoreMsg" },
				{ labels.loaded, "Normal" },
			})
		end
	end, PLUGIN_VERSION, { allow_unverified = config.allow_unverified_download })

	return true
end

-- Entry point: require("time-tracking-nvim").setup(opts).
--
-- opts, all optional and defaulted in default_config above:
--   auto_download              fetch the native library when it is missing
--   auto_update                re-fetch it when its version and the plugin's disagree
--   allow_unverified_download  install a release that publishes no SHA256SUMS
--                              entry for the asset (a digest *mismatch* is still
--                              refused)
--
-- Resolves the platform and the library path, downloads if one of the above says
-- to, adds the library's directory to package.cpath, then requires the native
-- module. A download is asynchronous, so on that path the cpath and require steps
-- run from the download callback and setup() returns before the plugin is loaded.
-- The native module reports an initialization failure as `native.error` rather
-- than as a Lua error; this echoes it either way.
function M.setup(opts)
	opts = opts or {}

	local config = vim.tbl_extend("force", default_config, opts)

	-- Published, not merely kept: M.download() reads allow_unverified_download
	-- back off M.config at call time. This has to stay above the platform guard
	-- and the whole ladder, so that the branches which return early publish it too.
	M.config = config

	local binary_path, target = get_binary_path()
	if not binary_path then
		echo({
			{ "Error: ", "ErrorMsg" },
			{ target, "Normal" },
		})
		return
	end

	local binary_exists, needs_update, update_reason = classify_binary_state(binary_path)

	-- Handle missing binary
	if not binary_exists and config.auto_download then
		-- Returns either way. Without a binary there is nothing to fall back
		-- to, so a download that could not even be started ends setup here.
		download_then_load(target, binary_path, config, install_labels(target, binary_path))
		return
		-- Handle version updates for existing binaries
	elseif needs_update and config.auto_download and config.auto_update then
		if download_then_load(target, binary_path, config, update_labels(update_reason)) then
			return
		end
		-- Deliberately no return. The tooling check failed, but an out-of-date
		-- binary is still a binary, so the tail below loads the one on disk.
	elseif needs_update and not config.auto_update then
		echo({
			{ "time-tracking-nvim: ", "WarningMsg" },
			{
				"binary version mismatch ("
					.. update_reason
					.. "); auto-update is disabled. Run :lua require('time-tracking-nvim').version_info() for detail.",
				"Normal",
			},
		})
	elseif not binary_exists then
		echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{
				"binary not found at " .. binary_path .. ". Run :checkhealth time-tracking-nvim for detail.",
				"Normal",
			},
		})
		return
	end

	-- Add binary directory to cpath before trying to load
	add_to_cpath(binary_path)

	-- Load the native module
	local status, value = load_native()
	if status == "load_failed" then
		echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Failed to load native module: " .. value, "Normal" },
			{ "\nMake sure the plugin is properly installed and the dynamic library is available", "Normal" },
		})
		return
	end

	if status == "init_failed" then
		echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Native module loaded but failed to initialize: " .. tostring(value), "Normal" },
			{ "\nNo commands were registered. Check your time-tracking-cli configuration.", "Normal" },
		})
		return
	end
end

-- Expose commonly used functions

-- Opens or closes the preview, via `:TimeTrackingToggle`. Like the two below it,
-- this needs setup() to have loaded the native module — that is what registers
-- the command.
function M.toggle()
	vim.cmd("TimeTrackingToggle")
end

-- Re-renders the preview immediately, via `:TimeTrackingUpdate`, skipping the
-- TextChanged throttle.
function M.update()
	vim.cmd("TimeTrackingUpdate")
end

-- Closes the preview window, via `:TimeTrackingClose`.
function M.close()
	vim.cmd("TimeTrackingClose")
end

-- Manual download function for troubleshooting
function M.download()
	local binary_path, target = get_binary_path()
	if not binary_path then
		echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ target, "Normal" },
		})
		return
	end

	echo({
		{ "time-tracking-nvim: ", "Title" },
		{ "Manually downloading binary for " .. target .. "...", "Normal" },
	})

	download_binary(target, binary_path, function(success, message)
		if success then
			echo({
				{ "time-tracking-nvim: ", "MoreMsg" },
				{ "Binary downloaded successfully to " .. binary_path, "Normal" },
			})
		else
			echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "Download failed: " .. message, "Normal" },
			})
		end
	end, PLUGIN_VERSION, { allow_unverified = (M.config or {}).allow_unverified_download })
end

-- Check version information
function M.version_info()
	local binary_path = get_binary_path()
	if not binary_path then
		echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Cannot determine binary path", "Normal" },
		})
		return
	end

	local binary_exists = vim.fn.filereadable(binary_path) == 1
	local binary_version = read_binary_version() or "unknown"
	local version_match = binary_version == PLUGIN_VERSION

	echo({
		{ "time-tracking-nvim version info:", "Title" },
		{ "\n  Plugin version: ", "Normal" },
		{ PLUGIN_VERSION, "String" },
		{ "\n  Binary version: ", "Normal" },
		{ binary_version, version_match and "String" or "WarningMsg" },
		{ "\n  Binary exists: ", "Normal" },
		{ tostring(binary_exists), binary_exists and "String" or "ErrorMsg" },
		{ "\n  Versions match: ", "Normal" },
		{ tostring(version_match), version_match and "String" or "WarningMsg" },
	})

	if not binary_exists then
		echo({
			{ "\n\nBinary not found. Run setup with auto_download enabled.", "WarningMsg" },
		})
	elseif not version_match then
		echo({
			{ "\n\nVersion mismatch detected!", "WarningMsg" },
			{ "\nRun :lua require('time-tracking-nvim').download() to update", "Normal" },
		})
	end
end

-- Test seam. Not part of the public API; contents may change without notice.
-- Only pure helpers, or ones whose side effects stay inside paths they are
-- given, belong here.
M._internal = {
	PLUGIN_VERSION = PLUGIN_VERSION,
	is_version_newer = is_version_newer,
	get_platform_info = get_platform_info,
	normalize_os_name = normalize_os_name,
	is_trusted_download_url = is_trusted_download_url,
	file_sha256 = file_sha256,
	parse_sha256sums = parse_sha256sums,
	checksum_verdict = checksum_verdict,
	install_binary = install_binary,
	select_assets = select_assets,
	load_native = load_native,
	plugin_root = plugin_root,
	get_binary_path = get_binary_path,
	get_version_file_path = get_version_file_path,
	read_binary_version = read_binary_version,
}

return M
