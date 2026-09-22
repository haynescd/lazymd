# Learning Neovim Plugin Development by Building lazymd.nvim

A study guide, in the shape of [LEARNING.md](LEARNING.md). The plugin under
`plugin/` and `lua/lazymd/` is small on purpose — about 500 lines of Lua, more
than half of it comments — that opens a floating window, runs `lazymd` inside
it, relays its images to the real terminal, and cleans up. Every Neovim concept
it touches is below, in the order you need them.

## How to use this guide

1. **Read `:help` first, not a blog post.** Neovim's own docs are the reference,
   they match your version exactly, and every section here names the tag to
   `:help`. Blogs go stale — `termopen()` is all over the internet and has been
   deprecated since 0.11.
2. **Type it yourself.** The same rule as the Rust ladder.
3. **Reload as you go.** `:Lazy reload lazymd` re-sources the plugin without
   restarting Neovim. When something gets weird, restart anyway — stale state is
   the usual cause, and N3 explains why.
4. **`:messages` is your `println!`.** Together with `vim.print(x)` (which
   pretty-prints a table) and `:lua= expr`, that's most of Lua debugging in
   Neovim.

Written against **Neovim 0.12**. Check `:version` if something here disagrees
with your editor.

---

## N0 — How Neovim finds your plugin

**Goal:** `:Lazymd` exists, and running it calls your code.

**New concepts:** `runtimepath`, the `plugin/` and `lua/` directories, how
`require` resolves, user commands, lazy-loading.

A "plugin" is not a special kind of thing. It's a directory on Neovim's
**`runtimepath`** (`:help 'runtimepath'`, `:echo &rtp` to see yours) laid out by
convention:

| Directory | When it runs | What goes in it |
|---|---|---|
| `plugin/` | Sourced automatically at startup, every file | Command and mapping definitions — the entry points |
| `lua/` | Only when something `require`s it | Everything else |
| `doc/` | On `:helptags` | `:help` documentation |
| `after/` | Sourced last | Overrides of other plugins |

`require("lazymd.window")` looks for `lua/lazymd/window.lua` on every
`runtimepath` entry, and `require("lazymd")` finds `lua/lazymd/init.lua` — the
same `init.lua` convention as a directory module in plain Lua. The result is
cached in `package.loaded`, so the file runs **once** per session no matter how
often you require it. That cache is why a plugin reload needs care.

`plugin/lazymd.lua` is the whole entry point:

```lua
if vim.g.loaded_lazymd then return end
vim.g.loaded_lazymd = true

vim.api.nvim_create_user_command("Lazymd", function(cmd)
    require("lazymd").open(cmd.args ~= "" and cmd.args or nil)
end, { nargs = "?", complete = "file", desc = "Preview Markdown with lazymd" })
```

Three things are deliberate:

- **The `loaded_` guard.** A file on `runtimepath` can be sourced more than once
  (two copies on the path, a manual `:source`). The guard is the long-standing
  convention for making that harmless.
- **`require` *inside* the callback.** Starting Neovim shouldn't load code you
  might never use. This way `lua/lazymd/` is read the first time you actually run
  the command.
- **`nargs = "?"`** makes the argument optional and `complete = "file"` gives you
  tab-completion of paths for free (`:help command-attributes`).

**lazy.nvim's part** is only to put the directory on `runtimepath` — plus, with
`cmd = "Lazymd"`, to defer even that: it defines a stub `:Lazymd`, and the first
time you run it, the plugin loads for real and the command is replaced.

> **The gotcha.** lazy.nvim names a plugin after the directory it came from, and
> derives the module to call `setup()` on from that name. This repo's directory
> is still `codon`, so `opts = {}` would make it call `require("codon").setup({})`
> and fail. `name = "lazymd"` in the spec fixes both halves — the module it
> requires *and* how the plugin is listed in `:Lazy`. (`main = "lazymd"` fixes
> only the first, which leaves `:Lazy load lazymd` reporting no such plugin.)

**Read:** `:help 'runtimepath'`, `:help lua-require`,
`:help nvim_create_user_command()`.

**Checkpoint:** `:Lazymd` reports a lazymd error rather than `E492: Not an editor
command`. `:Lazy` shows the plugin as not-loaded until you run it.

**Hint if stuck:** `:lua= vim.api.nvim_get_runtime_file("lua/lazymd/init.lua", false)`
prints where Neovim thinks your plugin is. An empty table means `runtimepath` is
the problem, not your Lua.

---

## N1 — Buffers, windows, and the float

**Goal:** `:Lazymd` opens an empty float, centred, with no editor chrome.

**New concepts:** buffers vs. windows, scratch buffers, `bufhidden`,
`nvim_open_win`, the `vim.bo` / `vim.wo` / `vim.o` scopes.

The one idea to get straight: **a buffer is text, a window is a viewport onto a
buffer.** They're separate handles with separate lifetimes. One buffer can be
shown in three windows; closing a window doesn't delete the buffer it showed.
Most confusing plugin bugs come from forgetting which of the two you're holding.

`window.lua` creates both:

```lua
local buf = vim.api.nvim_create_buf(false, true)  -- listed? scratch?
vim.bo[buf].bufhidden = "wipe"
```

`nvim_create_buf(false, true)` is **unlisted** (doesn't show in `:ls`, doesn't
turn up in buffer-cycling) and **scratch** (throwaway: no file, no "save your
changes?" on exit). That pairing is the standard one for UI a plugin owns.

`bufhidden = "wipe"` says: when the last window showing this buffer closes,
delete the buffer entirely. It's what makes `:q` inside the float tear everything
down, and N3 builds on it.

Then the window:

```lua
local win = vim.api.nvim_open_win(buf, true, {
    relative = "editor",
    style = "minimal",
    border = cfg.border,
    width = w, height = h, row = r, col = c,
})
```

- **`relative = "editor"`** positions against the whole editor, which is why the
  geometry reads `vim.o.columns` and `vim.o.lines` rather than the current
  window's size.
- **`true`** (the second argument) focuses the new window. For us that's not
  cosmetic — N2 depends on it.
- **`style = "minimal"`** turns off `number`, `signcolumn`, `cursorline`,
  `foldcolumn`, `list` and the rest in one word. lazymd paints every cell itself;
  none of Neovim's chrome should show through.
- **`border = "none"`**, because lazymd already draws its own rounded border and
  ` lazymd │ file.md ` title bar. Two frames look like a mistake.

**The three option scopes** trip everyone up once:

| Scope | Means | Example |
|---|---|---|
| `vim.o.x` | Global / current, the `:set` you type | `vim.o.columns` |
| `vim.bo[buf].x` | Buffer-local (`:setlocal` on a buffer option) | `vim.bo[buf].filetype` |
| `vim.wo[win].x` | Window-local | `vim.wo[win].wrap` |

Indexing with an explicit handle — `vim.bo[buf]`, not `vim.bo` — sets the option
on *that* buffer rather than whichever happens to be current. Worth making a
habit: it's the difference between a plugin that works and one that works only
when you call it from the right window.

**Read:** `:help nvim_open_win()`, `:help api-buffer`, `:help lua-vim-options`.

**Checkpoint:** an empty, centred, chrome-free box. `:q` closes it. `:ls` doesn't
mention it.

**Hint if stuck:** `:lua= vim.api.nvim_win_get_config(0)` inside the float prints
the exact config Neovim is using, which is the fastest way to see which field you
got wrong.

---

## N2 — Jobs, pseudo-terminals, and `term = true`

**Goal:** lazymd is actually running in the float, and your keys reach it.

**New concepts:** jobs, what a pty is, `jobstart` vs `vim.system`, terminal mode.

Neovim has three ways to run a program, and picking the wrong one is the classic
way to lose an afternoon:

| API | Use it for | Gives you a terminal? |
|---|---|---|
| `vim.system()` | Almost everything — capture stdout, await a result | No |
| `jobstart(cmd, {})` | Streaming output line by line | No |
| `jobstart(cmd, { term = true })` | Running a **terminal program** | Yes |

lazymd is a full-screen TUI: it enters raw mode, switches to the alternate
screen, asks the terminal how large it is and how it draws images, and expects
keystrokes one at a time. None of that works over a plain pipe. It needs a
**pseudo-terminal** — a kernel device pair that looks exactly like a real
terminal to the program on the far end, which is how `tmux`, `ssh` and Neovim's
own `:terminal` all work.

```lua
vim.fn.jobstart({ bin, path }, { term = true, on_exit = on_exit })
vim.cmd.startinsert()
```

> **Order is load-bearing.** `term = true` attaches the pty to the **current**
> buffer. The float must already be open and focused, or lazymd renders into
> whatever buffer you were editing. This is the single most confusing failure in
> the whole plugin, and it looks like your file got eaten.

`startinsert` puts the window into **terminal mode**, where keystrokes go to the
program instead of Neovim (`:help terminal-mode`). Without it you're in normal
mode over a terminal buffer, and `j` scrolls Neovim's view of lazymd's output
rather than reaching lazymd at all. `Ctrl-\ Ctrl-n` is how you get back out —
though here you'd just press `q`.

Two consequences worth knowing:

- **`termopen()` is deprecated.** It was the old way to do exactly this, removed
  in favour of `jobstart(..., { term = true })` in 0.11. Most tutorials predate
  that.
- **`$TERM` becomes `xterm-256color`.** Neovim sets it for the job. Truecolor
  still works, so lazymd's syntax highlighting is fine — but Neovim's built-in
  terminal implements neither the kitty graphics protocol nor sixel, and it
  doesn't answer lazymd's "what can you draw?" query. Left alone, images fall
  back to blurry halfblocks. N5 is how the plugin gets them sharp anyway.

**Read:** `:help jobstart()`, `:help terminal-mode`, `:help job-control`.

**Checkpoint:** `:Lazymd` on `test/Sample.md` shows the rendered document. `j`
and `k` scroll it. `:w` in another window updates it a moment later — that's
lazymd's own file watcher, no plugin code involved.

**Hint if stuck:** if the float opens empty, the binary probably isn't where you
think. `:lua= vim.fn.executable("lazymd")` returns 1 or 0, and
`$XDG_STATE_HOME/lazymd/lazymd.log` has the other half of the story.

---

## N3 — Lifecycle: cleaning up in both directions

**Goal:** nothing is left behind, whichever way the preview ends.

**New concepts:** `on_exit`, autocommands, augroups, buffer-local autocommands,
`vim.schedule`.

A preview can end **two** ways, and a plugin that only handles one leaks:

1. **lazymd exits** (you pressed `q`) — the window is still open and must be
   closed.
2. **The window closes** (you typed `:q`, or closed a tab) — lazymd is still
   running and must be stopped.

Direction 1 is the `on_exit` callback you passed to `jobstart`:

```lua
local function on_exit(_, code)
    vim.schedule(function()
        if code == 0 then M.close() return end
        forget()
        notify("exited with code " .. code .. " — press q to close", vim.log.levels.WARN)
    end)
end
```

Two details carry real weight:

- **`vim.schedule`.** Callbacks can arrive in a "fast event context" where much
  of the API is forbidden (`:help api-fast`). `vim.schedule` defers the work to
  the main loop, where everything is legal. When an API call fails inside a
  callback with a complaint about not being allowed, this is why.
- **The non-zero branch.** lazymd prints `lazymd: couldn't read …` and exits 1 on
  a bad path. Closing the window on the way out would erase the message before
  you could read it — so the window stays, but the plugin still forgets it, or
  the next `:Lazymd` would think a preview was already running. (lazygit.nvim
  returns early here without clearing its state, which is exactly that bug.)

Direction 2 is an **autocommand** — Neovim's event hooks (`:help autocmd`). This
one is *buffer-local*, so it only fires for our scratch buffer:

```lua
vim.api.nvim_create_autocmd("BufWipeout", {
    group = augroup,
    buffer = state.buf,
    callback = function()
        if state.job then pcall(vim.fn.jobstop, state.job) end
        forget()
    end,
})
```

This is where `bufhidden = "wipe"` from N1 pays off: closing the window wipes the
buffer, wiping the buffer fires `BufWipeout`, and that stops the process. Without
it you get orphaned `lazymd` processes — check with `pgrep lazymd`.

`pcall` is there because `jobstop` raises on a job that has already finished, and
on the clean path it has.

**Augroups** (`:help autocmd-groups`) are how autocommands stay reloadable:

```lua
local augroup = vim.api.nvim_create_augroup("lazymd", { clear = true })
```

`clear = true` deletes every autocommand previously registered in this group. Ten
`:Lazy reload lazymd`s therefore leave you with one `VimResized` handler, not
ten. Without it, reloading a plugin during development quietly stacks duplicate
handlers until something misbehaves — and you blame the wrong code.

Speaking of which, `VimResized` is registered **once at module load**, not per
open, and checks whether a float exists when it fires. Per-open registration
would need matching deregistration; a single cheap handler doesn't.

**State** lives in a module-local table:

```lua
local state = { buf = nil, win = nil, job = nil, from_win = nil }
```

Not a global. lazygit.nvim uses `LAZYGIT_BUFFER` and friends, which is a pattern
from older Lua plugins: any code in the editor can clobber them, and they outlive
a reload as stale values pointing at buffers that no longer exist.

**Read:** `:help autocmd`, `:help nvim_create_autocmd()`, `:help api-fast`,
`:help vim.schedule()`.

**Checkpoint:** every row of the table in the phase-1 plan. Especially: `:q`
inside the float leaves no process behind, and `:Lazymd /nope.md` leaves the
error on screen.

**Hint if stuck:** `:autocmd lazymd` lists everything registered in the group —
the fastest way to spot duplicates.

---

## N4 — Where phase 2 picks up

Phase 1 is a float you look at and dismiss. The actual goal is a **split** beside
the buffer you're editing, and two things make it work:

- **`wincmd p`** after opening the split, to hand focus straight back to your
  source buffer. A preview you have to leave to type in isn't a preview.
- **`vim.api.nvim_chan_send(chan, "j")`** — writing to the terminal's channel is
  identical to typing into it. That's how you scroll a preview you aren't focused
  on: send `"j"`, `"\x04"` for Ctrl-d, and so on, from a mapping in your *source*
  buffer. `jobstart` returns the channel id, which is the same number as the job
  id.

Phase 3 debounces `TextChanged` to write the buffer to a temp file so the preview
updates as you type, and phase 4 adds a control socket to lazymd itself so the
preview can follow your cursor. Both are in
[the plan](https://claude.ai/artifact/VasnVhNy1A8BkGJQTkFxEo).

**Read:** `:help nvim_chan_send()`, `:help :wincmd`, `:help TextChanged`.

---

## N5 — Talking to the host terminal

**Goal:** images in the float are as sharp as lazymd standalone, in Ghostty,
kitty or WezTerm.

**New concepts:** the host terminal vs. `:terminal`, `TermRequest`,
`nvim_ui_send`, kitty Unicode placeholders, why relays are allowlists.

There are **two terminals** in play, and keeping them apart is the whole trick:

```
Ghostty  ◀── Neovim's TUI draws its grid here ──  Neovim
                                                    │
                                          :terminal (libvterm)
                                                    │
                                                  lazymd
```

lazymd talks to libvterm, Neovim's built-in terminal emulator. libvterm turns
lazymd's output into a grid of cells, and Neovim redraws those cells onto
Ghostty along with everything else on screen. Anything libvterm doesn't
understand never reaches Ghostty.

**Kitty's Unicode placeholders** are what make images survive that trip anyway.
ratatui-image draws a kitty image in two parts:

1. **The data**, sent once as an APC escape: `ESC _G i=<id>,a=T,U=1,… ; <base64> ESC \`.
   `U=1` makes it a *virtual placement*: Ghostty stores the image but draws
   nothing yet.
2. **The placeholders**: every cell the image covers gets the character
   `U+10EEEE`, with combining marks for its row and column, and the image id
   in its **foreground colour**.

Part 2 is just text in a colour, and libvterm stores it and Neovim redraws it
like any other text. Ghostty sees placeholder cells arrive and paints the image
over them, wherever they are. That's why scrolling, float position, and popups
covering the preview all just work: Neovim is moving text around, and the
image follows the text.

Part 1 is the problem. libvterm doesn't know kitty's APC and drops it, so
Ghostty never receives the image the placeholders point at. Neovim 0.12 has
exactly the two hooks needed to carry it across:

- **`TermRequest`** fires when a `:terminal` child emits an OSC, DCS or APC
  sequence. `ev.data.sequence` holds it, starting with the introducer
  (`"\27_G…"`), and `ev.data.terminator` holds the `ESC \` separately.
- **`nvim_ui_send(bytes)`** writes raw bytes to the terminal Neovim is running
  in.

So the relay in `init.lua` is a few lines:

```lua
local function relay_graphics(ev)
    local seq = ev.data.sequence
    if seq:sub(1, 3) ~= "\27_G" then
        return
    end
    vim.api.nvim_ui_send(seq .. ev.data.terminator)
    -- …remember the image id, for cleanup
end
```

> **Why the allowlist matters.** It's tempting to forward *everything* a
> `:terminal` child emits and let Ghostty sort it out. Don't. That hands
> whatever's running in the preview control of your real terminal: `OSC 52`
> writes your system clipboard, `OSC 0` retitles the window, and a Markdown
> file is exactly the kind of untrusted input that could carry either. Forward
> the one thing you need, by prefix, and drop the rest.

**Cleanup has a new direction too.** Ghostty holds image data until it's told
to drop it, and it never learns that lazymd exited. So the plugin records each
relayed `i=` id, and `forget()` sends `ESC _G a=d,d=I,i=<id> ESC \` for each one
(`d=I`: delete by id *and* free the data).

**Two limits worth knowing:**

- **Cell size.** Standalone, lazymd asks the terminal how big a cell is in
  pixels (`CSI 16 t`). Inside Neovim it can't: `TermResponse` relays DA1, OSC,
  DCS and APC replies, but not CSI, and Neovim leaves the `:terminal` pty's
  pixel size at zero. lazymd guesses 10×20. A wrong guess doesn't blur
  anything; ratatui-image sizes the image for the guessed grid, and Ghostty
  draws it pixel-for-pixel at the real one, so it comes out the wrong size in
  its space (blank margin, or cropped). Set `cell_size` in the plugin options.
  To find it, run `lazymd` standalone once and look for
  `images: Kitty, 12x26 px per cell` in the log.
- **No compression.** ratatui-image only enables zlib when a query says the
  terminal supports it, and there's no query here. An 800×400 image is about
  1.7 MB of base64 through Neovim. That's fine for a local terminal, and worth
  knowing over SSH.

`auto` picks kitty only when all three hold: `nvim_ui_send` exists, a terminal
UI is attached (`nvim_list_uis()` reports `stdout_tty`), and the environment
names a kitty-protocol terminal (`TERM_PROGRAM` is `ghostty` or `WezTerm`, or
`KITTY_WINDOW_ID` is set). Anything else gets halfblocks. They're blurry, but a
kitty image sent to a terminal that can't draw it is worse: blank cells.

**Read:** `:help TermRequest`, `:help nvim_ui_send()`, `:help TermResponse`,
and kitty's
[Unicode placeholders](https://sw.kovidgoyal.net/kitty/graphics-protocol/#unicode-placeholders).

**Checkpoint:** `:Lazymd test/images.md` in Ghostty shows sharp images that
scroll with `j`/`k`, and `q` leaves nothing behind on screen.

**Hint if stuck:** blank cells where an image should be means the placeholders
arrived but the data didn't. Check `:autocmd lazymd` lists a `TermRequest`
handler, and that `:lua= vim.o.termguicolors` is `true`. Without truecolor,
Neovim rounds the placeholder's foreground to the nearest palette colour, and
the image id it encodes is lost.

---

## Reference: the APIs this plugin uses

Everything above, in one table, for when you remember the concept but not the
name.

| Call | Does |
|---|---|
| `nvim_create_user_command` | Defines `:Lazymd` |
| `nvim_create_buf(false, true)` | Unlisted scratch buffer |
| `nvim_open_win(buf, true, cfg)` | Opens and focuses a float |
| `nvim_win_set_config` | Moves/resizes an open window |
| `nvim_win_close(win, true)` | Closes a window, force |
| `nvim_win_is_valid` / `nvim_buf_is_valid` | Guards against a handle that's gone |
| `nvim_set_current_win` | Moves the cursor to a window |
| `nvim_get_runtime_file` | Where Neovim loaded a file from |
| `jobstart(argv, { term = true })` | Runs a program in a pty |
| `jobstop` | Stops it |
| `nvim_create_autocmd` / `nvim_create_augroup` | Event hooks, and reloadable groups |
| `vim.schedule` | Defers work out of a fast context |
| `vim.fn.executable` | Is this a runnable command or path? |
| `vim.uv.fs_stat` | Does this file exist? |
| `vim.fs.dirname` / `vim.fs.joinpath` | Path manipulation |
| `vim.tbl_deep_extend("force", a, b)` | Merges user options over defaults |
| `vim.notify` | Messages, routed through whatever notifier you use |
| `TermRequest` autocmd | A `:terminal` child emitted an OSC, DCS or APC |
| `nvim_ui_send(bytes)` | Writes raw bytes to the host terminal (0.12+) |
| `nvim_list_uis()` | Which UIs are attached; `stdout_tty` means a real terminal |
