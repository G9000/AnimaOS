# MCP setup from owner chat

The owner can ask whether Anima supports MCP before supplying a server. The model must see a real native setup tool and accurately invite a URL or configuration. Once supplied, it can save, enable, connect, inspect discovered tools, review exact descriptors, grant selected aliases, reconnect, disconnect, or remove its own connections through the existing durable Rust MCP manager.

Use a host-derived `manage_mcp` tool on direct owner web/API chat runs. Helpers, peers, connector messages, jobs, and follow-ups cannot administer connections. Revalidate the current run before each operation. Management uses the existing Exec approval policy and durable action receipts. Discovery remains separate from review and grants; no remote tool is invoked during setup. Existing UI and chat operate on the same records and versions.

Accept a single server definition as JSON, optionally wrapped in `mcpServers`, Hermes `mcp_servers`, or OpenClaw `mcp.servers`. Reuse supported Streamable HTTP URLs and trusted absolute Windows executable/argument launches. Reject unsupported fields explicitly instead of silently dropping authentication, environment, working-directory, or transport requirements. Credentials remain in the existing owner password control and OS vault, outside model tool arguments. Do not add OAuth, package installation, arbitrary headers, automatic reconnect, or a second MCP runtime in this change.

Saved configuration, reviews, and grants survive chats and restarts. Existing restart behavior restores disconnected; the agent can explicitly reconnect through chat. Newly granted tools become available on the next owner message, preserving the run's immutable tool catalog. Tool results must explain this boundary rather than claiming immediate invocation capability.

Verify model-visible setup availability before any server is configured, real lifecycle operations through the native tool, exact version conflicts, owner isolation, denied approval, durable restore, and availability/invocation in a subsequent chat. Use synthetic servers and credentials only; a real project connection requires the owner's eventual server details.
