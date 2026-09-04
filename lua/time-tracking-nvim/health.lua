-- :checkhealth time-tracking-nvim
--
-- Neovim resolves `:checkhealth <name>` to `lua/<name>/health.lua`, so this
-- file's location is what makes the idiomatic command work.

local M = {}

local health = vim.health
local uv = vim.uv or vim.loop

-- Entry point for `:checkhealth time-tracking-nvim`.
--
-- Reports, in order: platform support, the native library's presence and size,
-- whether the plugin and binary versions agree, whether the binary directory is
-- on package.cpath, whether the native module loads and initializes, whether the
-- commands are registered, and which of curl/tar/unzip auto-download can use.
--
-- The first three checks return early on failure: with no supported platform or
-- no readable library, every later check would only restate the same problem.
function M.check()
	health.start("time-tracking-nvim")

	local tt = require("time-tracking-nvim")
	local internal = tt._internal or {}

	-- Platform
	local platform_info, platform_err
	if internal.get_platform_info then
		platform_info, platform_err = internal.get_platform_info()
	end
	if not platform_info then
		health.error(tostring(platform_err), {
			"Supported: Linux x86_64/aarch64, macOS x86_64/arm64, Windows x86_64",
		})
		return
	end
	health.ok(string.format("Platform: %s (.%s)", platform_info.target, platform_info.ext))

	-- Binary
	local binary_path
	if internal.get_binary_path then
		binary_path = internal.get_binary_path()
	end

	if not binary_path then
		health.error("Native library not found at " .. tostring(binary_path), {
			"Run :lua require('time-tracking-nvim').download()",
			"Or build locally with ./build.sh",
		})
		return
	end

	if vim.fn.filereadable(binary_path) ~= 1 then
		health.error("Native library not found at " .. binary_path, {
			"Run :lua require('time-tracking-nvim').download()",
			"Or build locally with ./build.sh",
		})
		return
	end

	local stat = uv.fs_stat(binary_path)
	if not stat then
		health.error("Cannot stat " .. binary_path, {
			"Check the file's permissions",
			"Re-run :lua require('time-tracking-nvim').download()",
		})
		return
	end
	health.ok(string.format("Native library: %s (%d bytes)", binary_path, stat.size))

	-- Versions
	local binary_version = "unknown"
	if internal.read_binary_version then
		binary_version = internal.read_binary_version() or "unknown"
	end

	local plugin_version = internal.PLUGIN_VERSION
	if plugin_version and binary_version == plugin_version then
		health.ok("Version: plugin and binary both " .. plugin_version)
	elseif plugin_version then
		health.warn(
			string.format("Version mismatch: plugin %s, binary %s", plugin_version, binary_version),
			{ "Run :lua require('time-tracking-nvim').download()" }
		)
	else
		health.info("Binary version: " .. binary_version)
	end

	-- cpath
	local root
	if internal.plugin_root then
		root = internal.plugin_root()
	end
	if root and package.cpath:find(vim.fs.joinpath(root, "lua"), 1, true) then
		health.ok("Binary directory is on package.cpath")
	else
		health.warn("Binary directory is not on package.cpath", {
			"setup() adds it; make sure require('time-tracking-nvim').setup() has run",
		})
	end

	-- Load
	local status, value
	if internal.load_native then
		status, value = internal.load_native()
	end
	if status == "ok" then
		health.ok("Native module loads and initializes")
	elseif status == "init_failed" then
		health.error("Native module loaded but failed to initialize: " .. tostring(value), {
			"Check your time-tracking-cli configuration",
		})
	else
		health.error("Failed to load the native module: " .. tostring(value), {
			"Check the library's permissions and architecture",
			"cpath: " .. package.cpath,
		})
	end

	-- Commands
	if vim.fn.exists(":TimeTrackingToggle") == 2 then
		health.ok("Commands are registered")
	else
		health.error("Commands are not registered (:TimeTrackingToggle is missing)", {
			"Make sure require('time-tracking-nvim').setup() has run",
			"Check the native module load result above for a failed load",
		})
	end

	-- External tools used by auto-download
	for _, tool in ipairs({ "curl", "tar", "unzip" }) do
		if vim.fn.executable(tool) == 1 then
			health.ok(tool .. " is available")
		else
			health.warn(tool .. " is not available", { "Needed for auto-download/auto-update" })
		end
	end
end

return M
