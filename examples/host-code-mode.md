# The host's Code Mode and this project's runner

This page lists what the host's documentation says about its experimental Code Mode and how that
relates to `mcp-execution-cli run`. It contains documented facts only. Nothing here was measured
on a running host.

- **Source:** <https://code.claude.com/docs/en/code-mode.md>
- **Fetched:** 2026-11-24
- **Status of the mode:** the page says "Code Mode is experimental and off by default. It may change
  or be removed in any release." Re-read the page before relying on any line below.

## What the documentation states

| Topic | Statement (paraphrased from the page) |
|-------|----------------------------------------|
| Opt-in | Set `CLAUDE_CODE_EXPERIMENTAL_CODE_MODE` to `1`, in the shell or in the `env` block of user settings. |
| Project settings | The variable is ignored in a project's settings files. |
| Organisations | Setting the variable to `0` in managed settings turns the mode off for everyone in the organisation. |
| Tool | Code runs through a tool named `REPL`. |
| Permissions and hooks | `deny` and `ask` rules and `PreToolUse` hooks apply to nested calls under the tool's usual name. In auto mode the classifier reviews each piece of code before it runs, not each tool call. |
| Limits | Code Claude waits on stops after ten minutes; background code can run up to two hours. |
| Sandbox | Sandbox settings apply to the shell commands the code runs. The JavaScript and its file, web and MCP tool calls run outside the sandbox. |
| Where it works | Interactive terminal sessions only: not the VS Code panel, the Desktop app, cloud sessions or Remote Control. Not through a gateway or with Claude for Government. |
| Models | The page lists Opus 5.5, Sonnet 5.5, Haiku 5.5 and Fable 5.2. |
| Benefits | One piece of code can run commands in parallel, use one result in the next call, and return only the part of the output that is needed. The page publishes no token or cost figures. |

## What this means for the runner

- `run` executes a script outside the host's mode and has its own output budget. Whatever the host
  does to long output is separate and is not described by this page, except for the `uncutOutputNote`
  field, which the page mentions for v2.1.331 or later.
- Because the JavaScript of Code Mode runs outside the host's sandbox, treat a script that starts
  the runner from it as unsandboxed. Nested calls stay subject to the permission rules above.
- This project makes no claim that using Code Mode reduces tokens; the host publishes no figures.

## Not covered

The page does not document how MCP tools are called from code, the shape of their results, local
module imports, headless or Agent SDK behaviour, or guidance for MCP server authors. This project
does not describe them, and nothing it generates depends on them.
