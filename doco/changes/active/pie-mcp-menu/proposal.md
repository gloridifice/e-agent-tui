<!-- doco:lifecycle v=1 created-at=2026-10-09T10:12:28Z completed-at=- archived-at=- -->
# Pi-native MCP configuration menu

## Purpose
Bring the accepted offline MCP modal into pie without replacing Pi's MCP client or its native MCP command. Pi RPC supports native status/login/logout/reconnect but not a structured live server directory or immediate enabled/exposure setters.

## Scope and acceptance
- The native MCP command opens a centered, responsive frontend modal over the chat; closing preserves the composer draft. `q` closes browsing views and remains ordinary text in filter and masked callback editors.
- Inspect file-configured and extension-registered servers and currently registered tools. Native text status is explicitly an on-demand observation, not a subscribed live snapshot. Tool selection never invokes a tool.
- Confirm enabled-state and server-exposure changes, preserving unrelated configuration and credentials. Respect project trust and defining-file/project-override precedence; extension registrations without a file remain read-only.
- Save while the agent is running without reloading or interrupting it, and show a message-area reminder that the reload command is required. Do not automatically reload at subsequent settlement.
- Save while idle, then perform native resource reload and refresh catalogs. Recheck native idle admission; if it becomes busy, keep the saved configuration and report the need for manual reload. Reload failure never rolls back a successful save or claims the change is active.
- Native authentication and reconnect stay in the existing Pi child. Mask and discard callback values; no management prompts or secrets enter agent history or transcript.
- Preserve legacy extension ownership of the MCP command, older Pi behavior, DSH behavior, and existing manual reload semantics. No second MCP client, connection probe, credential-store implementation, new dependencies, or new/modified tests.
- Synchronize delivered architecture, configuration, presentation and interaction contracts; verify builds, existing scoped tests, lint and isolated real-terminal behavior.

## Result
Pending — not completed.
