-- Time Tracking Neovim Plugin
-- Main initialization module

local M = {}
local uv = vim.uv or vim.loop

-- Default configuration
local default_config = {
	-- Add any configuration options here
	-- auto_start = true,
	-- preview_width = nil, -- Will use 1/3 of screen width
	auto_download = true, -- Automatically download binaries if missing
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

-- Get platform-specific information
local function get_platform_info()
	local os_name = uv.os_uname().sysname:lower()
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

-- Download and extract binary from GitHub releases
local function download_binary(target, binary_path, callback)
	-- Get the latest release info
	local cmd = {
		"curl",
		"-L",
		"-s",
		"https://api.github.com/repos/stevenwcarter/time-tracking-nvim/releases/latest",
	}

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

			-- Find the appropriate asset
			local asset_name = string.format("time-tracking-nvim-%s.tar.gz", target)
			if target:match("windows") then
				asset_name = string.format("time-tracking-nvim-%s.zip", target)
			end

			local download_url = nil
			for _, asset in ipairs(release_info.assets or {}) do
				if asset.name == asset_name then
					download_url = asset.browser_download_url
					break
				end
			end

			if not download_url then
				callback(false, "No binary found for target: " .. target)
				return
			end

			-- Create target directory (safe to call in scheduled context)
			local target_dir = vim.fs.dirname(binary_path)
			vim.fn.mkdir(target_dir, "p")

			-- Create temp directory for download
			local temp_dir = vim.fn.tempname() .. "_time_tracking"
			vim.fn.mkdir(temp_dir, "p")
			local temp_file = vim.fs.joinpath(temp_dir, asset_name)

			local download_cmd = { "curl", "-L", "-o", temp_file, download_url }
			vim.system(download_cmd, {}, function(download_result)
				vim.schedule(function()
					if download_result.code ~= 0 then
						-- Clean up on error
						vim.fn.delete(temp_dir, "rf")
						callback(false, "Failed to download binary: " .. (download_result.stderr or ""))
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

									callback(true, "Binary downloaded successfully")
								end)
							end)
						end)
					end)
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
		vim.api.nvim_echo({
			{ "Error: ", "ErrorMsg" },
			{ target, "Normal" },
		}, false, {})
		return
	end

	-- Check if binary exists
	local binary_exists = vim.fn.filereadable(binary_path) == 1

	if not binary_exists and config.auto_download then
		vim.api.nvim_echo({
			{ "time-tracking-nvim: ", "Title" },
			{ "Binary not found, downloading for " .. target .. "...", "Normal" },
		}, false, {})

		-- Check if we have the required tools
		local has_curl = vim.fn.executable("curl") == 1
		local has_tar = vim.fn.executable("tar") == 1
		local has_unzip = vim.fn.executable("unzip") == 1

		if not has_curl then
			vim.api.nvim_echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "curl is required for auto-download but not found", "Normal" },
				{ "\nPlease install curl or download manually from: ", "Normal" },
				{ "https://github.com/stevenwcarter/time-tracking-nvim/releases", "Underlined" },
			}, false, {})
			return
		end

		if not has_tar and not has_unzip then
			vim.api.nvim_echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "tar or unzip is required for auto-download but not found", "Normal" },
				{ "\nPlease install tar/unzip or download manually from: ", "Normal" },
				{ "https://github.com/stevenwcarter/time-tracking-nvim/releases", "Underlined" },
			}, false, {})
			return
		end

		download_binary(target, binary_path, function(success, message)
			if success then
				vim.api.nvim_echo({
					{ "time-tracking-nvim: ", "MoreMsg" },
					{ "Binary downloaded successfully!", "Normal" },
				}, false, {})

				-- Add binary directory to cpath before trying to load
				add_to_cpath(binary_path)

				-- Try to load the native module now
				local ok, native = pcall(require, "time_tracking_nvim")
				if not ok then
					vim.api.nvim_echo({
						{ "time-tracking-nvim: ", "ErrorMsg" },
						{ "Failed to load native module after download: ", "Normal" },
						{ native, "ErrorMsg" },
						{ "\nPlease check the binary permissions and try restarting Neovim", "Normal" },
					}, false, {})
				else
					vim.api.nvim_echo({
						{ "time-tracking-nvim: ", "MoreMsg" },
						{ "Plugin loaded successfully!", "Normal" },
					}, false, {})
				end
			else
				vim.api.nvim_echo({
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
				}, false, {})
			end
		end)
		return
	elseif not binary_exists then
		vim.api.nvim_echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Binary not found at ", "Normal" },
			{ binary_path, "Directory" },
			{ "\n\nTo enable auto-download, use:", "Normal" },
			{ "\nrequire('time-tracking-nvim').setup({ auto_download = true })", "String" },
			{ "\n\nOr download manually from: ", "Normal" },
			{ "https://github.com/stevenwcarter/time-tracking-nvim/releases", "Underlined" },
			{ "\nDownload: ", "Normal" },
			{ "time-tracking-nvim-" .. target .. (target:match("windows") and ".zip" or ".tar.gz"), "String" },
		}, false, {})
		return
	end

	-- Add binary directory to cpath before trying to load
	add_to_cpath(binary_path)

	-- Load the native module
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		vim.api.nvim_echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ "Failed to load native module: " .. native, "Normal" },
			{ "\nMake sure the plugin is properly installed and the dynamic library is available", "Normal" },
		}, false, {})
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
		vim.api.nvim_echo({
			{ "time-tracking-nvim: ", "ErrorMsg" },
			{ target, "Normal" },
		}, false, {})
		return
	end

	vim.api.nvim_echo({
		{ "time-tracking-nvim: ", "Title" },
		{ "Manually downloading binary for " .. target .. "...", "Normal" },
	}, false, {})

	download_binary(target, binary_path, function(success, message)
		if success then
			vim.api.nvim_echo({
				{ "time-tracking-nvim: ", "MoreMsg" },
				{ "Binary downloaded successfully to " .. binary_path, "Normal" },
			}, false, {})
		else
			vim.api.nvim_echo({
				{ "time-tracking-nvim: ", "ErrorMsg" },
				{ "Download failed: " .. message, "Normal" },
			}, false, {})
		end
	end)
end

-- Test function to verify the plugin is working
function M.test()
	local binary_path, target = get_binary_path()
	if not binary_path then
		vim.api.nvim_echo({
			{ "time-tracking-nvim test: ", "ErrorMsg" },
			{ target, "Normal" },
		}, false, {})
		return false
	end

	local binary_exists = vim.fn.filereadable(binary_path) == 1
	if not binary_exists then
		vim.api.nvim_echo({
			{ "time-tracking-nvim test: ", "ErrorMsg" },
			{ "Binary not found at ", "Normal" },
			{ binary_path, "Directory" },
		}, false, {})
		return false
	end

	-- Add binary directory to cpath before trying to load
	add_to_cpath(binary_path)

	-- Check if binary has correct permissions
	local stat = uv.fs_stat(binary_path)
	if not stat then
		vim.api.nvim_echo({
			{ "time-tracking-nvim test: ", "ErrorMsg" },
			{ "Cannot stat binary file: ", "Normal" },
			{ binary_path, "Directory" },
		}, false, {})
		return false
	end

	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		vim.api.nvim_echo({
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
		}, false, {})
		return false
	end

	vim.api.nvim_echo({
		{ "time-tracking-nvim test: ", "MoreMsg" },
		{ "✓ Plugin is working correctly!", "Normal" },
		{ "\n  Binary: ", "Normal" },
		{ binary_path, "Directory" },
		{ "\n  Target: ", "Normal" },
		{ target, "String" },
	}, false, {})
	return true
end

return M
