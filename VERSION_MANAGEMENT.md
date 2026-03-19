# Version Management System

The time-tracking-nvim plugin now includes an intelligent version management system that ensures the binary stays in sync with the plugin version.

## How It Works

### Version Tracking
- The plugin version is defined in both `Cargo.toml` and `lua/time-tracking-nvim/init.lua`
- When a binary is downloaded, a `.version` file is created alongside it containing the version information
- On plugin startup, the system compares the plugin version against the stored binary version

### Automatic Updates
When enabled (`auto_update = true`, which is the default), the plugin will automatically:

1. **Check Version Compatibility**: Compare plugin version vs binary version on startup
2. **Download Updates**: If versions don't match, download the appropriate binary for the current plugin version
3. **Store Version Info**: Save version information for future comparisons

### Configuration Options

```lua
require("time-tracking-nvim").setup({
  auto_download = true,  -- Download binary if missing (default: true)
  auto_update = true,    -- Auto-update binary when plugin version changes (default: true)
})
```

### Manual Management

#### Check Version Status
```lua
:lua require('time-tracking-nvim').version_info()
```

This will display:
- Current plugin version
- Current binary version  
- Whether they match
- Binary existence status

#### Force Download/Update
```lua
:lua require('time-tracking-nvim').download()
```

Forces a fresh download of the binary matching the current plugin version.

### Version Scenarios

1. **Fresh Installation**: Plugin detects no binary and downloads the latest matching the plugin version
2. **Plugin Update**: Plugin detects version mismatch and automatically updates the binary
3. **Manual Override**: User can disable auto-updates and manage versions manually
4. **Fallback**: If auto-update fails, plugin warns but continues with existing binary

### File Locations

- **Binary**: `lua/time_tracking_nvim.{ext}` (where `{ext}` is `so`/`dylib`/`dll`)
- **Version File**: `lua/time_tracking_nvim.{ext}.version`

### Troubleshooting

#### Version Mismatch Warnings
If you see version mismatch warnings:
```
Warning: Version mismatch detected!
Run :lua require('time-tracking-nvim').download() to update
```

You can:
1. Enable auto-updates: `require('time-tracking-nvim').setup({ auto_update = true })`
2. Manually update: `:lua require('time-tracking-nvim').download()`

#### Disable Auto-Updates
```lua
require("time-tracking-nvim").setup({
  auto_update = false,  -- Disable automatic binary updates
})
```

This is useful if you want to use a custom binary or manage versions manually.

### Version Comparison Logic

The system uses semantic version comparison:
- `0.1.3` vs `0.1.4` → Update needed
- `0.1.4` vs `0.1.4` → No update needed  
- Missing version file → Update needed (assumes old binary)

This ensures that whenever you update the plugin (via your package manager), the binary will automatically stay in sync.