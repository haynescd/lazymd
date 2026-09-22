--- The floating window, and the lazymd process that lives inside it.
---
--- These are separated from init.lua because they're the two Neovim subsystems
--- worth understanding on their own: a window showing a throwaway buffer, and a
--- job attached to that buffer through a pseudo-terminal. init.lua decides
--- *when* to do these things; this file knows *how*. See docs/NVIM-PLUGIN.md.

local M = {}

--- Geometry for a float of `scale`, centred on the editor.
---
--- `vim.o.columns` and `vim.o.lines` describe the whole editor rather than the
--- current window, which is what `relative = "editor"` measures against. One
--- row comes off the height for the command line.
---@param scale number
local function geometry(scale)
    local width = math.ceil(vim.o.columns * scale)
    local height = math.ceil(vim.o.lines * scale) - 1
    return {
        relative = "editor",
        width = width,
        height = height,
        -- Integers: a fractional row would be rounded somewhere out of our
        -- hands, and the float would sit a row off from where we asked.
        row = math.floor((vim.o.lines - height) / 2),
        col = math.floor((vim.o.columns - width) / 2),
    }
end

--- Opens the float over a fresh scratch buffer and focuses it.
---@param cfg lazymd.Config
---@return integer buf, integer win
function M.open(cfg)
    -- `false, true` is "unlisted, scratch": it never appears in :ls, and Neovim
    -- never asks you to save it on the way out.
    local buf = vim.api.nvim_create_buf(false, true)

    -- Wiping the buffer when its last window closes is what makes `:q` inside
    -- the float tear the whole preview down — the BufWipeout autocmd in
    -- init.lua hangs off that, and stops the lazymd process with it.
    vim.bo[buf].bufhidden = "wipe"
    -- Gives you something to match on in your own autocmds and keymaps later.
    vim.bo[buf].filetype = "lazymd"

    local config = geometry(cfg.scale)
    config.style = "minimal"
    config.border = cfg.border

    -- `true` focuses the new window, and it has to be focused before the job
    -- starts: `term = true` attaches the pty to whichever buffer is current.
    --
    -- `style = "minimal"` turns off 'number', 'signcolumn', 'cursorline',
    -- 'foldcolumn', 'list' and friends in one go, instead of unsetting each by
    -- hand. lazymd is drawing every cell itself; none of that should show
    -- through.
    local win = vim.api.nvim_open_win(buf, true, config)

    return buf, win
end

--- Re-centres an open float after the editor changed size.
---
--- Neovim resizes the pty to match, the kernel sends SIGWINCH, and lazymd
--- re-renders at the new width on its own — so moving the window is all there
--- is to do here.
---@param win integer
---@param cfg lazymd.Config
function M.resize(win, cfg)
    vim.api.nvim_win_set_config(win, geometry(cfg.scale))
end

--- Starts lazymd in the current buffer, wired to a pseudo-terminal.
---
--- `term = true` is the 0.11+ replacement for the deprecated `termopen()`. The
--- usual advice is to prefer `vim.system()` for running a program, but it has
--- no pty — and a full-screen TUI needs one: lazymd switches to the alternate
--- screen, reads keys in raw mode, and asks the terminal how big it is.
---
--- Returns the job id, or 0/-1 if the command couldn't be run at all.
---@param argv string[]
---@param on_exit fun(job: integer, code: integer, event: string)
---@return integer job
function M.start(argv, on_exit)
    return vim.fn.jobstart(argv, { term = true, on_exit = on_exit })
end

return M
