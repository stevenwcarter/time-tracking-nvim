# Time Tracking Neovim Plugin

A high-performance Neovim plugin written in Rust that provides live time tracking previews while editing markdown files. Built with [nvim-oxi](https://github.com/noib3/nvim-oxi) for optimal performance and reliability.

## Features

- 🚀 **High Performance**: Written in Rust for minimal overhead
- 📊 **Live Preview**: Real-time updates as you edit your time tracking files  
- 🪟 **Smart Window Management**: Automatic opening/closing of preview windows
- 📁 **Directory Aware**: Only activates for files in your configured time tracking directory
- ⌨️ **Keyboard Shortcuts**: Easy toggle commands and keybindings
- 🔧 **Zero Configuration**: Works out of the box with sensible defaults

## Installation

### Using [lazy.nvim](https://github.com/folke/lazy.nvim) (Recommended)

```lua
{
  "stevenwcarter/time-tracking-nvim",
  config = function()
    require("time-tracking-nvim").setup()
  end,
}
```

### Using [packer.nvim](https://github.com/wbthomason/packer.nvim)

```lua
use {
  "stevenwcarter/time-tracking-nvim",
  config = function()
    require("time-tracking-nvim").setup()
  end,
}
```

### Using [vim-plug](https://github.com/junegunn/vim-plug)

```vim
Plug 'stevenwcarter/time-tracking-nvim'

lua << EOF
require("time-tracking-nvim").setup()
EOF
```

## Configuration

The plugin itself works with zero configuration, but does utilize the configuration for
the [time-tracking-cli utility](https://github.com/stevenwcarter/time-tracking-cli)


## Usage

### Commands

The plugin provides several commands:

- `:TimeTrackingToggle` - Toggle the preview window on/off
- `:TimeTrackingPreview` - Show preview window (alias for toggle)
- `:TimeTrackingUpdate` - Manually update the preview content
- `:TimeTrackingClose` - Close the preview window

### Default Keybindings

- `<leader>tt` - Toggle time tracking preview

### Automatic Behavior

The plugin automatically:

1. **Opens preview** when you enter a markdown file in your time tracking directory
2. **Updates preview** in real-time as you type
3. **Closes preview** when you leave time tracking files or quit Neovim
4. **Manages window layout** to keep preview at 1/3 screen width

## How It Works

This plugin integrates with [time-tracking-cli](https://github.com/stevenwcarter/time-tracking-cli) to:

1. **Detect time tracking files** based on your configured data directory
2. **Parse markdown content** to extract time tracking information
3. **Format and display** summaries in a live preview window
4. **Update automatically** as you edit your time tracking files

## Requirements

- Neovim 0.11+ 
- The plugin includes pre-compiled binaries for:
  - Linux x86_64
  - macOS (Intel and Apple Silicon)
  - Windows x86_64

## Troubleshooting

### Plugin Not Loading

If you see an error about loading the native module:

1. Ensure you're using a supported platform (Linux, macOS, Windows x86_64)
2. Check that the plugin was installed correctly by your plugin manager
3. Try restarting Neovim

### Preview Not Showing

If the preview window doesn't appear:

1. Make sure you're editing a `.md` file in your time tracking directory
2. Check that your time-tracking-cli configuration is set up correctly
3. Try manually running `:TimeTrackingToggle`

### Performance Issues

The plugin is designed to be lightweight, but if you experience issues:

1. The preview updates on every text change - this is normal
2. Large files might cause slower updates
3. Report performance issues on GitHub

## Development

### Building from Source

```bash
git clone https://github.com/stevenwcarter/time-tracking-nvim
cd time-tracking-nvim
cargo build --release
```

### Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Related Projects

- [time-tracking-cli](https://github.com/stevenwcarter/time-tracking-cli) - The core time tracking functionality
- [time-tracking-parser](https://github.com/stevenwcarter/time-tracking-parser) - The parser for the time tracking format
- [nvim-oxi](https://github.com/noib3/nvim-oxi) - Rust bindings for Neovim plugins
