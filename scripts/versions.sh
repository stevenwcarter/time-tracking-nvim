#!/usr/bin/env bash
#
# Single source for the two versions that must agree: Cargo.toml's package
# version and the PLUGIN_VERSION constant in the Lua loader.
#
# Sourced, not executed — it sets `cargo_version` and `lua_version` in the
# caller's shell. Callers run from the repo root.

cargo_version="$(grep -m1 '^version = ' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
lua_version="$(grep -m1 'PLUGIN_VERSION = ' lua/time-tracking-nvim/init.lua | sed -E 's/.*"([^"]+)".*/\1/')"
