--- Defaults, and the one place they get merged with your options.
---
--- `require("lazymd").setup(opts)` hands its table here; everything else calls
--- `config.get()`. Going through a function rather than exporting the table
--- directly matters: a module that `require`s this when Neovim starts would
--- otherwise capture the defaults before `setup()` ever runs.

local M = {}

---@class lazymd.Config
---@field cmd string|nil Path to the lazymd binary. nil auto-detects.
---@field scale number Fraction of the editor the float covers, 0 to 1.
---@field border string|string[] Any value `nvim_open_win` accepts for `border`.
---@field on_unsaved "warn"|"write"|"refuse" What to do about a modified buffer.
---@field log_level string|nil Passed through to lazymd as --log-level.
local defaults = {
    cmd = nil,
    -- lazygit.nvim uses 0.9 and it reads well: large enough to be the thing
    -- you're looking at, small enough that you can see you're still in Neovim.
    scale = 0.9,
    -- lazymd draws its own rounded border and ` lazymd | file.md ` title bar,
    -- so a border here would be a second frame around the first one.
    border = "none",
    on_unsaved = "warn",
    log_level = nil,
}

local options = vim.deepcopy(defaults)

--- Merges `opts` over the defaults. lazy.nvim calls this for you when the
--- plugin spec has an `opts` table.
---@param opts lazymd.Config|nil
function M.setup(opts)
    options = vim.tbl_deep_extend("force", defaults, opts or {})
    return options
end

---@return lazymd.Config
function M.get()
    return options
end

return M
