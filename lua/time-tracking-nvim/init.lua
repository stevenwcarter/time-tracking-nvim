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
local PLUGIN_VERSION = "0.1.7"

-- Default configuration
local default_config = {
	-- Add any configuration options here
	-- auto_start = true,
	-- preview_width = nil, -- Will use 1/3 of screen width
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

-- Normalize libuv's sysname to the keys used in platform_mappings.
-- uv.os_uname() mimics uname, so Windows reports "Windows_NT" (and MSYS/MinGW
-- shells report "MINGW64_NT-…"/"MSYS_NT-…"), none of which is "windows".
local function normalize_os_name(os_name)
	if os_name:match("^windows") or os_name:match("^mingw") or os_name:match("^msys") then
		return "windows"
	end
	return os_name
end

-- Get platform-specific information
local function get_platform_info()
	local os_name = normalize_os_name(uv.os_uname().sysname:lower())
	local arch = uv.os_uname().machine:lower()

	local platform_mappings = {
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

	-- Handle alternative arch names
	if arch == "amd64" then
		arch = "x86_64"
	end
	if arch == "aarch64" then
		arch = "arm64"
	end

	local platform = platform_mappings[os_name]
	if not platform or not platform[arch] then
		return nil, string.format("Unsupported platform: %s-%s", os_name, arch)
	end

	return platform[arch], nil
end

-- Get the path where the binary should be located
local function get_binary_path()
	local info = debug.getinfo(1, "S")
	local plugin_root = vim.fn.fnamemodify(info.source:sub(2), ":h:h:h")
	local platform_info, err = get_platform_info()

	if not platform_info then
		return nil, err
	end

	local binary_name = "time_tracking_nvim." .. platform_info.ext
	return vim.fs.joinpath(plugin_root, "lua", binary_name), platform_info.target
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
	
	local success = pcall(vim.fn.writefile, {version}, version_file)
	return success
end

-- Compare version strings (basic semver comparison)
local function is_version_newer(current, new)
	if not current or not new then
		return true -- Assume newer if we can't compare
	end
	
	-- Remove 'v' prefix if present
	current = current:gsub("^v", "")
	new = new:gsub("^v", "")
	
	if current == new then
		return false
	end
	
	-- Simple string comparison for now (works for semver)
	-- This handles cases like "0.1.2" vs "0.1.3" correctly
	local current_parts = {}
	local new_parts = {}
	
	for part in current:gmatch("([^%.]+)") do
		table.insert(current_parts, tonumber(part) or 0)
	end
	
	for part in new:gmatch("([^%.]+)") do
		table.insert(new_parts, tonumber(part) or 0)
	end
	
	-- Pad shorter version with zeros
	local max_len = math.max(#current_parts, #new_parts)
	for i = #current_parts + 1, max_len do
		current_parts[i] = 0
	end
	for i = #new_parts + 1, max_len do
		new_parts[i] = 0
	end
	
	-- Compare each part
	for i = 1, max_len do
		if new_parts[i] > current_parts[i] then
			return true
		elseif new_parts[i] < current_parts[i] then
			return false
		end
	end
	
	return false -- Versions are equal
end

-- Shared curl hardening for both the API call and the archive download.
--   --proto/--proto-redir =https  : curl's default redirect protocol set
--                                   includes plain HTTP, so a 302 to http://
--                                   would fetch the library we are about to
--                                   dlopen in cleartext.
--   --fail-with-body              : without -f curl exits 0 on an HTTP error,
--                                   so a 403 rate-limit body parsed as JSON
--                                   and surfaced as "unsupported platform".
--   --max-time/--connect-timeout  : a black-holed connection otherwise left
--                                   the callback pending forever.
local CURL_HARDENING = {
	"--proto",
	"=https",
	"--proto-redir",
	"=https",
	"--tlsv1.2",
	"--fail-with-body",
	"--max-redirs",
	"5",
	"--connect-timeout",
	"10",
	"--max-time",
	"60",
	"--retry",
	"2",
}

-- Build a curl argv: {"curl", <hardening>, unpack(extra)}
local function curl_cmd(extra)
	local cmd = { "curl" }
	vim.list_extend(cmd, CURL_HARDENING)
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

-- Compute the SHA-256 of a file.
--
-- Prefers a subprocess over reading the file into Lua: readfile/writefile
-- round-trips are lossy for binary content, and the digest has to match
-- byte-for-byte what sha256sum computed in CI.
local function file_sha256(path)
	local out
	if vim.fn.executable("sha256sum") == 1 then
		out = vim.system({ "sha256sum", "--", path }, { text = true }):wait()
	elseif vim.fn.executable("shasum") == 1 then
		out = vim.system({ "shasum", "-a", "256", "--", path }, { text = true }):wait()
	elseif vim.fn.executable("certutil") == 1 then
		out = vim.system({ "certutil", "-hashfile", path, "SHA256" }, { text = true }):wait()
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

-- Download and extract binary from GitHub releases
local function download_binary(target, binary_path, callback, expected_version, opts)
	-- Ask for the release we actually want. Falling back to /latest only when
	-- no version was requested: previously this always fetched /latest and then
	-- recorded expected_version, so the .version file was an assertion about
	-- what we wanted rather than an observation of what we got.
	local api_base = "https://api.github.com/repos/stevenwcarter/time-tracking-nvim/releases"
	local release_url = expected_version and (api_base .. "/tags/v" .. expected_version)
		or (api_base .. "/latest")

	local cmd = curl_cmd({ "-L", "-s", release_url })

	vim.system(cmd, {}, function(result)
		vim.schedule(function()
			if result.code ~= 0 then
				callback(false, "Failed to fetch release info: " .. (result.stderr or ""))
				return
			end

			local ok, release_info = pcall(vim.json.decode, result.stdout)
			if not ok then
				callback(false, "Failed to parse release info")
				return
			end

			-- A rate-limited or errored API response decodes to valid JSON with
			-- no `assets` field, which used to fall through to "No binary found
			-- for target: …" — telling the user their platform is unsupported
			-- when they were merely rate-limited (60 req/hr per IP, routine on NAT).
			if type(release_info) ~= "table" then
				callback(false, "Unexpected GitHub API response: " .. tostring(result.stdout):sub(1, 200))
				return
			end
			if release_info.message then
				callback(false, "GitHub API error: " .. tostring(release_info.message))
				return
			end
			if type(release_info.assets) ~= "table" then
				callback(
					false,
					"GitHub API response had no assets (rate limited or malformed): "
						.. tostring(result.stdout):sub(1, 200)
				)
				return
			end

			-- Find the appropriate asset
			local asset_name = string.format("time-tracking-nvim-%s.tar.gz", target)
			if target:match("windows") then
				asset_name = string.format("time-tracking-nvim-%s.zip", target)
			end

			local download_url = nil
			local sums_url = nil
			for _, asset in ipairs(release_info.assets) do
				if asset.name == asset_name then
					download_url = asset.browser_download_url
				elseif asset.name == "SHA256SUMS" then
					sums_url = asset.browser_download_url
				end
			end

			if not download_url then
				callback(false, "No binary found for target: " .. target)
				return
			end

			if not is_trusted_download_url(download_url) then
				callback(false, "Refusing untrusted download URL: " .. tostring(download_url))
				return
			end

			-- Create target directory (safe to call in scheduled context)
			local target_dir = vim.fs.dirname(binary_path)
			vim.fn.mkdir(target_dir, "p")

			-- Create temp directory for download
			local temp_dir = vim.fn.tempname() .. "_time_tracking"
			vim.fn.mkdir(temp_dir, "p")
			local temp_file = vim.fs.joinpath(temp_dir, asset_name)

			local download_cmd = curl_cmd({ "-L", "-o", temp_file, "--", download_url })
			vim.system(download_cmd, {}, function(download_result)
				vim.schedule(function()
					if download_result.code ~= 0 then
						-- Clean up on error
						vim.fn.delete(temp_dir, "rf")
						callback(false, "Failed to download binary: " .. (download_result.stderr or ""))
						return
					end

					-- Verify BEFORE extracting: everything downstream — extract, copy
					-- into lua/, and the pcall(require, …) that dlopens it — treats
					-- these bytes as trusted native code.
					local allow_unverified = opts and opts.allow_unverified

					local function verify_then_extract(expected_digest)
						local actual, digest_err
						if expected_digest then
							actual, digest_err = file_sha256(temp_file)
							if not actual then
								vim.fn.delete(temp_dir, "rf")
								callback(false, "Could not compute checksum: " .. tostring(digest_err))
								return
							end
						end

						local refusal = checksum_verdict(expected_digest, actual, allow_unverified)
						if refusal then
							vim.fn.delete(temp_dir, "rf")
							callback(false, asset_name .. ": " .. refusal)
							return
						end

						-- Extract the archive
						local extract_cmd
						if asset_name:match("%.zip$") then
							extract_cmd = { "unzip", "-q", "-o", temp_file, "-d", temp_dir }
						else
							extract_cmd = { "tar", "-xzf", temp_file, "-C", temp_dir }
						end

						vim.system(extract_cmd, {}, function(extract_result)
							vim.schedule(function()
								if extract_result.code ~= 0 then
									-- Clean up on error
									vim.fn.delete(temp_dir, "rf")
									callback(false, "Failed to extract binary: " .. (extract_result.stderr or ""))
									return
								end

								-- Move the binary to the correct location
								local extracted_binary =
									vim.fs.joinpath(temp_dir, "target", "release", vim.fs.basename(binary_path))

								-- Check if extracted binary exists
								if vim.fn.filereadable(extracted_binary) ~= 1 then
									-- Clean up on error
									vim.fn.delete(temp_dir, "rf")
									callback(false, "Extracted binary not found at: " .. extracted_binary)
									return
								end

								local move_cmd = { "cp", extracted_binary, binary_path }
								vim.system(move_cmd, {}, function(move_result)
									vim.schedule(function()
										-- Clean up temp files
										vim.fn.delete(temp_dir, "rf")

										if move_result.code ~= 0 then
											callback(
												false,
												"Failed to copy binary to target location: " .. (move_result.stderr or "")
											)
											return
										end

										-- Record the tag we actually downloaded, not the one we asked
										-- for: with a pinned plugin tag these can differ, and recording
										-- the request made every later version comparison a no-op.
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

										callback(true, "Binary downloaded successfully")
									end)
								end)
							end)
						end)
					end

					if sums_url and not is_trusted_download_url(sums_url) then
						-- A SHA256SUMS URL that fails the host allowlist is a tampering signal,
						-- not a missing asset. Reporting it as "no checksums published" would
						-- nudge the user to set allow_unverified_download in response to an
						-- attack, so refuse outright and do not mention the escape hatch.
						vim.fn.delete(temp_dir, "rf")
						callback(false, "Refusing untrusted SHA256SUMS URL: " .. tostring(sums_url))
					elseif sums_url then
						local sums_file = vim.fs.joinpath(temp_dir, "SHA256SUMS")
						vim.system(curl_cmd({ "-L", "-o", sums_file, "--", sums_url }), {}, function(sums_result)
							vim.schedule(function()
								if sums_result.code ~= 0 or vim.fn.filereadable(sums_file) ~= 1 then
									vim.fn.delete(temp_dir, "rf")
									callback(false, "Could not download SHA256SUMS: " .. (sums_result.stderr or ""))
									return
								end
								local sums = parse_sha256sums(table.concat(vim.fn.readfile(sums_file), "\n"))
								local expected = sums[asset_name]
								if not expected then
									vim.fn.delete(temp_dir, "rf")
									callback(false, "SHA256SUMS has no entry for " .. asset_name)
									return
								end
								verify_then_extract(expected)
							end)
						end)
					else
						verify_then_extract(nil)
					end
				end)
			end)
		end)
	end)
end

function M.setup(opts)
	opts = opts or {}

	-- Merge user config with defaults
	local config = vim.tbl_extend("force", default_config, opts)

	-- Store config for other functions
	M.config = config

	-- Get binary path
	local binary_path, target = get_binary_path()
	if not binary_path then
		echo({
			{ "Error: ", "ErrorMsg" },
			{ target, "Normal" },
		})
		return
	end

	-- Check if binary exists
	local binary_exists = vim.fn.filereadable(binary_path) == 1
	
	-- Check version compatibility
	local needs_update = false
	local update_reason = ""
	
	if binary_exists then
		local current_binary_version = read_binary_version()
		if not current_binary_version then
			-- No version file found, assume it's an old binary
			needs_update = true
			update_reason = "no version information found (updating to track versions)"
		elseif current_binary_version ~= PLUGIN_VERSION then
			-- Version mismatch between plugin and binary
			needs_update = true
			update_reason = string.format(
				"version mismatch (plugin: %s, binary: %s)",
				PLUGIN_VERSION,
				current_binary_version
			)
		end
	end
	
	-- Handle missing binary
	if not binary_exists and config.auto_download then
		echo({
			{ "time-tracking-nvim: ", "Title" },
			{ "Binary not found, downloading for " .. target .. "...", "Normal" },
		}, { transient = true })

		-- Check if we have the required tools
		local has_curl = vim.fn.executable("curl") == 1
		local has_tar = vim.fn.executable("tar") == 1
		local has_unzip = vim.fn.executable("unzip") == 1

		if not has_curl then
			echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "curl is required for auto-download but not found", "Normal" },
				{ "\nPlease install curl or download manually from: ", "Normal" },
				{ "https://github.com/stevenwcarter/time-tracking-nvim/releases", "Underlined" },
			})
			return
		end

		if not has_tar and not has_unzip then
			echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "tar or unzip is required for auto-download but not found", "Normal" },
				{ "\nPlease install tar/unzip or download manually from: ", "Normal" },
				{ "https://github.com/stevenwcarter/time-tracking-nvim/releases", "Underlined" },
			})
			return
		end

		download_binary(target, binary_path, function(success, message)
			if success then
				echo({
					{ "time-tracking-nvim: ", "MoreMsg" },
					{ "Binary downloaded successfully!", "Normal" },
				})

				-- Add binary directory to cpath before trying to load
				add_to_cpath(binary_path)

				-- Try to load the native module now
				local ok, native = pcall(require, "time_tracking_nvim")
				if not ok then
					echo({
						{ "time-tracking-nvim: ", "ErrorMsg" },
						{ "Failed to load native module after download: ", "Normal" },
						{ native, "ErrorMsg" },
						{ "\nPlease check the binary permissions and try restarting Neovim", "Normal" },
					})
				else
					if type(native) == "table" and native.error then
						echo({
							{ "time-tracking-nvim: ", "ErrorMsg" },
							{ "Loaded but failed to initialize: " .. tostring(native.error), "Normal" },
						})
					else
						echo({
							{ "time-tracking-nvim: ", "MoreMsg" },
							{ "Plugin loaded successfully!", "Normal" },
						})
					end
				end
			else
				echo({
					{
						"time-tracking-nvim: ",
						"ErrorMsg",
					},
					{ "Auto-download failed: ", "Normal" },
					{
						message,
						"ErrorMsg",
					},
					{ "\n\nManual installation instructions:", "Normal" },
					{ "\n1. Go to: ", "Normal" },
					{
						"https://github.com/stevenwcarter/time-tracking-nvim/releases",
						"Underlined",
					},
					{ "\n2. Download: ", "Normal" },
					{ "time-tracking-nvim-" .. target .. (target:match("windows") and ".zip" or ".tar.gz"), "String" },
					{ "\n3. Extract to: ", "Normal" },
					{
						vim.fs.dirname(binary_path),
						"Directory",
					},
				})
			end
		end, PLUGIN_VERSION, { allow_unverified = config.allow_unverified_download })
		return
	-- Handle version updates for existing binaries
	elseif needs_update and config.auto_download and config.auto_update then
		echo({
			{ "time-tracking-nvim: ", "Title" },
			{ "Binary update needed (" .. update_reason .. "), downloading...", "Normal" },
		}, { transient = true })

		-- Check if we have the required tools
		local has_curl = vim.fn.executable("curl") == 1
		local has_tar = vim.fn.executable("tar") == 1
		local has_unzip = vim.fn.executable("unzip") == 1

		if not has_curl then
			echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "curl is required for auto-update but not found", "Normal" },
				{ "\nUsing existing binary, but it may be incompatible", "WarningMsg" },
			})
		else
			download_binary(target, binary_path, function(success, message)
				if success then
					echo({
						{ "time-tracking-nvim: ", "MoreMsg" },
						{ "Binary updated successfully!", "Normal" },
					})

					-- Add binary directory to cpath before trying to load
					add_to_cpath(binary_path)

					-- Try to load the native module now
					local ok, native = pcall(require, "time_tracking_nvim")
					if not ok then
						echo({
							{ "time-tracking-nvim: ", "ErrorMsg" },
							{ "Failed to load native module after update: ", "Normal" },
							{ native, "ErrorMsg" },
							{ "\nPlease restart Neovim", "Normal" },
						})
					else
						if type(native) == "table" and native.error then
							echo({
								{ "time-tracking-nvim: ", "ErrorMsg" },
								{ "Loaded but failed to initialize: " .. tostring(native.error), "Normal" },
							})
						else
							echo({
								{ "time-tracking-nvim: ", "MoreMsg" },
								{ "Plugin updated and loaded successfully!", "Normal" },
							})
						end
					end
				else
					echo({
						{ "time-tracking-nvim: ", "ErrorMsg" },
						{ "Auto-update failed: ", "Normal" },
						{ message, "ErrorMsg" },
						{ "\nUsing existing binary, but it may be incompatible", "WarningMsg" },
					})
				end
			end, PLUGIN_VERSION, { allow_unverified = config.allow_unverified_download })
			return
		end
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
				"binary not found at "
					.. binary_path
					.. ". Run :checkhealth time-tracking-nvim for detail.",
				"Normal",
			},
		})
		return
	end

	-- Add binary directory to cpath before trying to load
	add_to_cpath(binary_path)

	-- Load the native module
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Failed to load native module: " .. native, "Normal" },
			{ "\nMake sure the plugin is properly installed and the dynamic library is available", "Normal" },
		})
		return
	end

	if type(native) == "table" and native.error then
		echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Native module loaded but failed to initialize: " .. tostring(native.error), "Normal" },
			{ "\nNo commands were registered. Check your time-tracking-cli configuration.", "Normal" },
		})
		return
	end
end

-- Expose commonly used functions
function M.toggle()
	vim.cmd("TimeTrackingToggle")
end

function M.update()
	vim.cmd("TimeTrackingUpdate")
end

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

-- Test function to verify the plugin is working
function M.test()
	local binary_path, target = get_binary_path()
	if not binary_path then
		echo({
			{ "time-tracking-nvim test: ", "ErrorMsg" },
			{ target, "Normal" },
		})
		return false
	end

	local binary_exists = vim.fn.filereadable(binary_path) == 1
	if not binary_exists then
		echo({
			{ "time-tracking-nvim test: ", "ErrorMsg" },
			{ "Binary not found at ", "Normal" },
			{ binary_path, "Directory" },
		})
		return false
	end

	-- Add binary directory to cpath before trying to load
	add_to_cpath(binary_path)

	-- Check if binary has correct permissions
	local stat = uv.fs_stat(binary_path)
	if not stat then
		echo({
			{ "time-tracking-nvim test: ", "ErrorMsg" },
			{ "Cannot stat binary file: ", "Normal" },
			{ binary_path, "Directory" },
		})
		return false
	end

	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		echo({
			{ "time-tracking-nvim test: ", "ErrorMsg" },
			{ "Failed to load native module: ", "Normal" },
			{ native, "ErrorMsg" },
			{ "\n\nDebugging info:", "Normal" },
			{ "\n  Binary path: ", "Normal" },
			{ binary_path, "Directory" },
			{ "\n  Binary exists: ", "Normal" },
			{ tostring(binary_exists), "String" },
			{ "\n  Binary size: ", "Normal" },
			{ tostring(stat.size), "Number" },
			{ "\n  Current cpath: ", "Normal" },
			{ package.cpath, "Comment" },
		})
		return false
	end

	local binary_version = read_binary_version() or "unknown"
	local version_match = binary_version == PLUGIN_VERSION
	
	echo({
		{ "time-tracking-nvim test: ", "MoreMsg" },
		{ "✓ Plugin is working correctly!", "Normal" },
		{ "\n  Binary: ", "Normal" },
		{ binary_path, "Directory" },
		{ "\n  Target: ", "Normal" },
		{ target, "String" },
		{ "\n  Plugin version: ", "Normal" },
		{ PLUGIN_VERSION, "String" },
		{ "\n  Binary version: ", "Normal" },
		{ binary_version, version_match and "String" or "WarningMsg" },
		{ "\n  Versions match: ", "Normal" },
		{ tostring(version_match), version_match and "String" or "WarningMsg" },
	})
	
	if not version_match then
		echo({
			{ "\n\nWarning: Version mismatch detected!", "WarningMsg" },
			{ "\nRun :lua require('time-tracking-nvim').download() to update", "Normal" },
		})
	end
	
	return true
end

-- Test seam. Not part of the public API; contents may change without notice.
-- Only pure, side-effect-free helpers belong here.
M._internal = {
	is_version_newer = is_version_newer,
	get_platform_info = get_platform_info,
	normalize_os_name = normalize_os_name,
	is_trusted_download_url = is_trusted_download_url,
	file_sha256 = file_sha256,
	parse_sha256sums = parse_sha256sums,
	checksum_verdict = checksum_verdict,
}

return M
