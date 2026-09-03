-- Minimal zero-dependency assert harness for the Lua loader's pure helpers.
-- Runs under `nvim --headless -u NONE`.
local H = { failures = {}, passes = 0, current = "" }

function H.describe(name, fn)
  H.current = name
  fn()
end

function H.it(name, fn)
  local label = H.current .. " > " .. name
  local ok, err = pcall(fn)
  if ok then
    H.passes = H.passes + 1
  else
    table.insert(H.failures, label .. ": " .. tostring(err))
  end
end

function H.eq(actual, expected, msg)
  if actual ~= expected then
    error(string.format("%s: expected %s, got %s",
      msg or "eq", vim.inspect(expected), vim.inspect(actual)), 2)
  end
end

function H.ok(value, msg)
  if not value then
    error((msg or "ok") .. ": expected truthy, got " .. vim.inspect(value), 2)
  end
end

function H.run()
  for _, f in ipairs(H.failures) do
    io.stderr:write("FAIL  " .. f .. "\n")
  end
  io.stdout:write(string.format("%d passed, %d failed\n", H.passes, #H.failures))
  return #H.failures
end

return H
