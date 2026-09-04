#!/bin/bash
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "${HERE}/../.." && pwd)"

echo "Running Lua loader tests (headless Neovim)..."

nvim --headless -u NONE --noplugin \
  --cmd "set runtimepath^=${REPO_ROOT}" \
  --cmd "lua package.path = package.path .. ';${HERE}/?.lua'" \
  -c "lua
    local failures = 0
    for _, spec in ipairs({ 'spec_version', 'spec_platform', 'spec_download_url', 'spec_install', 'spec_setup', 'spec_download' }) do
      package.loaded['harness'] = nil
      package.loaded[spec] = nil
      local H = require(spec)
      failures = failures + H.run()
    end
    if failures > 0 then vim.cmd('cquit 1') end
    vim.cmd('qall!')
  "

echo "Lua loader tests passed."
