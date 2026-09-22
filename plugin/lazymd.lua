-- Defines the :Lazymd command, and nothing else.
--
-- Files under plugin/ are sourced by Neovim when the plugin loads, which with
-- `cmd = "Lazymd"` in the lazy.nvim spec is the first time you run the command.
-- Keeping this file to a command definition is what makes that deferral worth
-- having: the real code in lua/lazymd/ is only `require`d once you ask for it.

if vim.g.loaded_lazymd then
    return
end
vim.g.loaded_lazymd = true

vim.api.nvim_create_user_command("Lazymd", function(cmd)
    require("lazymd").open(cmd.args ~= "" and cmd.args or nil)
end, {
    nargs = "?",
    complete = "file",
    desc = "Preview Markdown with lazymd",
})
