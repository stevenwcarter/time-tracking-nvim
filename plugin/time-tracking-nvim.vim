" Time Tracking Neovim Plugin
" Basic plugin initialization for Vim compatibility

if exists('g:loaded_time_tracking_nvim')
  finish
endif
let g:loaded_time_tracking_nvim = 1

" Ensure we're running on a compatible Neovim version
if !has('nvim-0.11')
  echoerr 'time-tracking-nvim requires Neovim 0.11 or later'
  finish
endif

" The actual plugin initialization is handled by the Lua module
" This file just ensures proper loading order and compatibility
