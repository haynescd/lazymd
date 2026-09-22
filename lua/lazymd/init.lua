--- lazymd.nvim — preview the current Markdown buffer with lazymd, in a float.
---
--- The command lives in plugin/lazymd.lua; everything it does is here. Phase 1
--- of the plan: open a float, run the binary in it, and clean up properly in
--- both directions. docs/NVIM-PLUGIN.md walks through the APIs involved.

local config = require("lazymd.config")
local window = require("lazymd.window")

local M = {}

--- Everything about the preview that's currently open, or nils.
---
--- Module-local on purpose. lazygit.nvim keeps the equivalent in true globals
--- (`LAZYGIT_BUFFER`), which is a pattern from older Lua plugins: anything in
--- the editor can overwrite them, and they survive a plugin reload as stale
--- values. A local table is no harder and has neither problem.
local state = {
    buf = nil, ---@type integer|nil
    win = nil, ---@type integer|nil
    job = nil, ---@type integer|nil
    from_win = nil, ---@type integer|nil Window to put the cursor back in.
    images = {}, ---@type table<string, true> Kitty image ids relayed to the host terminal.
}

-- `clear = true` means re-sourcing this file replaces its autocmds instead of
-- adding a second copy of each — the thing that makes plugin reloads safe.
local augroup = vim.api.nvim_create_augroup("lazymd", { clear = true })

---@param msg string
---@param level integer|nil
local function notify(msg, level)
    vim.notify("lazymd: " .. msg, level or vim.log.levels.INFO)
end

local function is_open()
    return state.win ~= nil and vim.api.nvim_win_is_valid(state.win)
end

--- Tells the host terminal to drop every image this preview relayed to it.
---
--- The terminal keeps image data until told otherwise (or until its memory
--- quota evicts it), and it never learns that the lazymd process went away.
--- `d=I` deletes by id *and* frees the data; `q=2` asks for no reply.
local function release_images()
    for id in pairs(state.images) do
        vim.api.nvim_ui_send("\27_Ga=d,d=I,i=" .. id .. ",q=2\27\\")
    end
    state.images = {}
end

--- Forgets the preview without touching the window.
---
--- Used on a failed exit, where the window stays up so you can read the error
--- but a following `:Lazymd` should still open a fresh one.
local function forget()
    release_images()
    state.buf, state.win, state.job, state.from_win = nil, nil, nil, nil
end

--- Closes the preview and puts the cursor back where it came from.
function M.close()
    local win, from, job = state.win, state.from_win, state.job
    forget()

    -- Stop the job here rather than leaving it to BufWipeout below: we've just
    -- cleared the state that handler reads, so it would find nothing to stop.
    -- `jobstop` throws on an id that has already finished, which is the common
    -- case — a clean quit reaches us through on_exit.
    if job then
        pcall(vim.fn.jobstop, job)
    end

    if win and vim.api.nvim_win_is_valid(win) then
        -- `true` is force: the buffer has a running job attached, and Neovim
        -- would otherwise refuse to close the last window showing it.
        vim.api.nvim_win_close(win, true)
    end
    -- The window we came from may be long gone (`:Lazymd`, then close the
    -- split behind it), so the preview closing must not depend on it.
    if from and vim.api.nvim_win_is_valid(from) then
        vim.api.nvim_set_current_win(from)
    end
end

--- The file to preview: an explicit argument, else the current buffer.
---@param arg string|nil
---@return string|nil path, string|nil reason
local function resolve_file(arg)
    if arg then
        local path = vim.fn.fnamemodify(vim.fn.expand(arg), ":p")
        if not vim.uv.fs_stat(path) then
            return nil, arg .. " doesn't exist"
        end
        return path
    end

    -- Terminal, help and quickfix buffers all carry a 'buftype'; only a normal
    -- buffer ("") is backed by a file on disk.
    if vim.bo.buftype ~= "" then
        return nil, "this isn't a file buffer"
    end

    local path = vim.api.nvim_buf_get_name(0)
    if path == "" then
        return nil, "this buffer has no file yet — save it first"
    end
    if not vim.uv.fs_stat(path) then
        return nil, vim.fn.fnamemodify(path, ":t") .. " hasn't been written to disk yet"
    end
    return path
end

--- Finds the lazymd binary: this repo's release build first, then $PATH.
---
--- `nvim_get_runtime_file` asks Neovim where it actually loaded us from, which
--- beats hardcoding a path or digging it out of `debug.getinfo`. This file is
--- at <root>/lua/lazymd/init.lua, so the repo root is three directories up.
---@param cfg lazymd.Config
---@return string|nil
local function find_binary(cfg)
    -- `executable()` is happy with an absolute path as well as a bare name, so
    -- one check covers a configured path, a built binary, and $PATH alike.
    if cfg.cmd then
        return vim.fn.executable(cfg.cmd) == 1 and cfg.cmd or nil
    end

    local this = vim.api.nvim_get_runtime_file("lua/lazymd/init.lua", false)[1]
    if this then
        local root = vim.fs.dirname(vim.fs.dirname(vim.fs.dirname(this)))
        local built = vim.fs.joinpath(root, "target", "release", "lazymd")
        if vim.fn.executable(built) == 1 then
            return built
        end
    end

    if vim.fn.executable("lazymd") == 1 then
        return "lazymd"
    end
end

--- Whether a terminal UI is attached that `nvim_ui_send` can write to. Not the
--- case headless, or for a GUI like Neovide, which draws its own grid.
local function has_tty_ui()
    for _, ui in ipairs(vim.api.nvim_list_uis()) do
        if ui.stdout_tty then
            return true
        end
    end
    return false
end

--- Whether the terminal Neovim is running in speaks the kitty graphics protocol.
---
--- There's no reliable way to ask from inside Neovim, so this goes by the
--- environment the terminal sets. Anything unrecognised gets halfblocks, which
--- are blurry but always correct.
local function host_speaks_kitty()
    local program = vim.env.TERM_PROGRAM
    return program == "ghostty" or program == "WezTerm" or vim.env.KITTY_WINDOW_ID ~= nil
end

--- Resolves the `images` option to what lazymd is actually told to do.
---@param cfg lazymd.Config
---@return "kitty"|"halfblocks"
local function image_protocol(cfg)
    -- Relaying needs `nvim_ui_send` (0.12+) and a terminal on the other end
    -- of it. Without both, kitty images would arrive as blank cells.
    local can_relay = vim.api.nvim_ui_send ~= nil and has_tty_ui()

    if cfg.images == "kitty" and not can_relay then
        notify('images = "kitty" needs Neovim 0.12+ in a terminal, using halfblocks', vim.log.levels.WARN)
        return "halfblocks"
    end
    if cfg.images == "auto" then
        return (can_relay and host_speaks_kitty()) and "kitty" or "halfblocks"
    end
    return cfg.images
end

--- Relays one kitty graphics sequence from lazymd to the host terminal.
---
--- lazymd draws images as Unicode placeholder characters, which are ordinary
--- text and pass through Neovim's terminal fine. The image data itself travels
--- in an APC escape (`ESC _G ...`) that Neovim's terminal emulator swallows.
--- `TermRequest` hands it to us instead, and `nvim_ui_send` writes it straight
--- to the real terminal, which then draws the image wherever Neovim puts the
--- placeholders.
---@param ev { data: { sequence: string, terminator: string } }
local function relay_graphics(ev)
    local seq = ev.data.sequence
    -- An allowlist, not a filter: only kitty graphics get through. Forwarding
    -- a child's escapes wholesale would let anything shown in the preview
    -- drive the real terminal — set the clipboard (OSC 52), retitle it, and so on.
    if seq:sub(1, 3) ~= "\27_G" then
        return
    end
    vim.api.nvim_ui_send(seq .. ev.data.terminator)

    -- Remember the id so release_images() can free it. Only the control part,
    -- before the `;`, is key=value; the payload after it is base64.
    local control = seq:match("^\27_G([^;]*)")
    local id = control and ("," .. control):match(",i=(%d+)")
    if id then
        state.images[id] = true
    end
end

--- What to do when the buffer you're previewing has unsaved changes.
---
--- lazymd renders the file on disk, so a modified buffer means the preview is
--- behind. It catches up by itself on your next `:w` — the watcher sees it —
--- which is why the default only warns.
---@param cfg lazymd.Config
---@param path string
---@return boolean ok
local function handle_unsaved(cfg, path)
    if not vim.bo.modified then
        return true
    end

    if cfg.on_unsaved == "refuse" then
        notify("buffer has unsaved changes — save it first", vim.log.levels.ERROR)
        return false
    elseif cfg.on_unsaved == "write" then
        vim.cmd.write()
        return true
    end

    notify(
        vim.fn.fnamemodify(path, ":t") .. " has unsaved changes, previewing the version on disk",
        vim.log.levels.WARN
    )
    return true
end

--- Runs when lazymd exits, however it exited.
---@param code integer
local function on_exit(_, code)
    -- Job callbacks can arrive while Neovim is somewhere that forbids changing
    -- windows, so hand the teardown back to the main loop.
    vim.schedule(function()
        if code == 0 then
            M.close()
            return
        end

        -- lazymd prints why it failed (a bad path, say) and exits 1. Leave the
        -- window up so the message is readable, but forget the preview either
        -- way, or the next `:Lazymd` would think one is still running.
        forget()
        notify("exited with code " .. code .. " — press q to close", vim.log.levels.WARN)
    end)
end

--- Opens the preview, or focuses it if one is already up.
---@param arg string|nil Optional path; defaults to the current buffer's file.
function M.open(arg)
    if is_open() then
        vim.api.nvim_set_current_win(state.win)
        return
    end

    local cfg = config.get()

    local path, reason = resolve_file(arg)
    if not path then
        return notify(reason, vim.log.levels.ERROR)
    end

    local bin = find_binary(cfg)
    if not bin then
        return notify("binary not found — run `cargo build --release`, or put lazymd on $PATH", vim.log.levels.ERROR)
    end

    -- Only meaningful when previewing the buffer you're sitting in.
    if not arg and not handle_unsaved(cfg, path) then
        return
    end

    local protocol = image_protocol(cfg)
    local argv = { bin, "--image-protocol", protocol }
    if protocol == "kitty" and cfg.cell_size then
        vim.list_extend(argv, { "--cell-size", cfg.cell_size })
    end
    if cfg.log_level then
        vim.list_extend(argv, { "--log-level", cfg.log_level })
    end
    table.insert(argv, path)

    state.from_win = vim.api.nvim_get_current_win()
    state.buf, state.win = window.open(cfg)
    state.job = window.start(argv, on_exit)

    if state.job <= 0 then
        notify("couldn't start " .. bin, vim.log.levels.ERROR)
        M.close()
        return
    end

    -- Terminal mode, so your keys reach lazymd instead of Neovim. `q`, `Esc`
    -- and `Ctrl-c` all quit it, which lands in on_exit above.
    vim.cmd.startinsert()

    -- Once the process has exited the buffer drops back to normal mode, where
    -- this gives you the same `q` to dismiss it. While lazymd is running it
    -- never fires: terminal mode sends `q` straight through.
    vim.keymap.set("n", "q", M.close, { buffer = state.buf, desc = "Close lazymd" })

    if protocol == "kitty" then
        vim.api.nvim_create_autocmd("TermRequest", {
            group = augroup,
            buffer = state.buf,
            callback = relay_graphics,
        })
    end

    -- Closing the window with `:q` wipes the buffer, which lands here. Without
    -- it the lazymd process would outlive the window showing it.
    vim.api.nvim_create_autocmd("BufWipeout", {
        group = augroup,
        buffer = state.buf,
        callback = function()
            -- `jobstop` throws on an id that has already finished, and a clean
            -- exit gets here by way of on_exit, so this is a normal path.
            if state.job then
                pcall(vim.fn.jobstop, state.job)
            end
            forget()
        end,
    })
end

---@param opts lazymd.Config|nil
function M.setup(opts)
    config.setup(opts)
end

-- Registered once, rather than per-open: the callback is cheap when nothing is
-- open, and this way there's exactly one of it no matter how often you toggle.
vim.api.nvim_create_autocmd("VimResized", {
    group = augroup,
    desc = "Re-centre the lazymd float",
    callback = function()
        if is_open() then
            window.resize(state.win, config.get())
        end
    end,
})

return M
