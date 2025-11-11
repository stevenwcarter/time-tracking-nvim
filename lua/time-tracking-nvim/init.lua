-- Time Tracking Neovim Plugin
-- Main initialization module

local M = {}

-- Default configuration
local default_config = {
	-- Add any configuration options here
	-- auto_start = true,
	-- preview_width = nil, -- Will use 1/3 of screen width
}

function M.setup(opts)
	opts = opts or {}

	-- Merge user config with defaults
	local config = vim.tbl_extend("force", default_config, opts)

	-- Load the native module
	local ok, native = pcall(require, "time_tracking_nvim")
	if not ok then
		vim.api.nvim_echo({
			{ "Failed to load time_tracking_nvim native module: ", native },
			{ "Make sure the plugin ins properly installed and the dynamic library is available" },
		}, false, { err = true })
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

return M
