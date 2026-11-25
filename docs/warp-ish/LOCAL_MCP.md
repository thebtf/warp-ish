# Local MCP control on Windows

Enable **Settings → Scripting → Local MCP control** and open a new pane. Start the MCP client from that pane so it receives the endpoint and pane UUID. Configure the client to run the packaged `app/warp-ish-mcp.exe` over stdio. The adapter connects on its first tool call. Launch `app/warp-ish.exe` to open the terminal as usual.

The adapter includes optional usage examples in its MCP initialization response. Clients can pass these examples to the agent.

Use `where_am_i` to get the instance, window, tab group, tab, and pane IDs. A peer launched with `start_peer` receives its block ID while that block is active. For other launches, `block_id` is null. Agents inside the same terminal program manage their own conversation IDs. `list_surfaces` shows the addressable layout in the selected profile. Use the returned IDs verbatim and refresh them after restarting warp-ish or re-enabling local control.

To start a peer, call `create_tab` with a window ID or `split_pane` with a pane ID and direction. Use the returned pane ID for `start_peer` with the program, arguments, working directory, and optional prompt. The target needs a ready, idle PowerShell prompt with an empty buffer. Startup passes the prompt as the last argument. A busy response means the pane is still bootstrapping or has another command or draft.

`send_key` addresses a running terminal program by pane ID. Supported modifier names are `ctrl`, `alt`, and `shift`. Use the normal close confirmation for a running process. A close request reports acceptance of the UI action. `list_surfaces` confirms whether the pane or tab has closed.

The endpoint is local to a warp-ish process and accepts the same Windows user. Turning off local control closes it. Open a new pane after turning it back on so its agent inherits the current endpoint.
