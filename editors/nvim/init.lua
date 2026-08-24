-- Headless Neovim host for native-lsp vs node-lsp RSS comparison.
-- nvim --headless --clean -u editors/nvim/init.lua

vim.opt.swapfile = false
vim.opt.hidden = true
vim.opt.loadplugins = false

local root = vim.env.NATIVE_LSP_ROOT or vim.fn.getcwd()
local kind = vim.env.NATIVE_LSP_KIND or "native"
local report_path = vim.env.NATIVE_LSP_REPORT or (root .. "/.nvim-lsp-report.json")
local done_path = vim.env.NATIVE_LSP_DONE
local mixed = vim.fn.glob(root .. "/testdata/mixed/*", false, true)
table.sort(mixed)

local cmd
if kind == "node" then
  cmd = { vim.env.NODE_BIN or "node", root .. "/compare/node-lsp.mjs" }
else
  cmd = { vim.env.NATIVE_LSP_BIN or (root .. "/target/release/native-lsp") }
end

local client_id
local bufs = {}

for _, path in ipairs(mixed) do
  vim.cmd.edit(vim.fn.fnameescape(path))
  local buf = vim.api.nvim_get_current_buf()
  table.insert(bufs, buf)
  local id = vim.lsp.start({
    name = "native-lsp",
    cmd = cmd,
    root_dir = root,
  })
  if id then
    client_id = id
  end
end

vim.wait(4000, function()
  if not client_id then
    return false
  end
  local client = vim.lsp.get_client_by_id(client_id)
  return client ~= nil and client.initialized == true
end, 50)

local probes = {}
for _, buf in ipairs(bufs) do
  local uri = vim.uri_from_bufnr(buf)
  local name = vim.fn.fnamemodify(vim.api.nvim_buf_get_name(buf), ":t")
  local ft = vim.bo[buf].filetype
  local sym_params = { textDocument = { uri = uri } }
  local syms = vim.lsp.buf_request_sync(buf, "textDocument/documentSymbol", sym_params, 3000) or {}
  local symbol_count = 0
  local line = 0
  for _, reply in pairs(syms) do
    local result = reply.result or {}
    symbol_count = #result
    if result[1] and result[1].location and result[1].location.range then
      line = result[1].location.range.start.line or 0
    end
  end
  local hover_params = {
    textDocument = { uri = uri },
    position = { line = line, character = 0 },
  }
  local hovers = vim.lsp.buf_request_sync(buf, "textDocument/hover", hover_params, 3000) or {}
  local hover = ""
  for _, reply in pairs(hovers) do
    local result = reply.result
    if result and result.contents then
      if type(result.contents) == "table" and result.contents.value then
        hover = vim.split(result.contents.value, "\n")[1] or ""
      end
    end
  end
  table.insert(probes, {
    name = name,
    language_id = ft,
    symbol_count = symbol_count,
    hover = hover,
  })
end

local report = {
  host = "neovim",
  server = kind,
  host_pid = vim.uv.os_getpid(),
  client_id = client_id,
  probes = probes,
}
vim.fn.writefile({ vim.json.encode(report) }, report_path)

if done_path and done_path ~= "" then
  vim.wait(30000, function()
    return vim.uv.fs_stat(done_path) ~= nil
  end, 50)
end

vim.cmd.quitall({ bang = true })
