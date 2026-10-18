# Coding agent bundles: license notes

## This repository

All five skills in this folder were written for this repository. Its root
`package.json` and Cargo workspace metadata declare `MIT`. There is no root
`LICENSE` file, so no separate license text for the Anima parts is bundled here.
The manifests record that metadata declaration as-is.

## Third-party CLIs

The skills describe how to run third-party programs: Claude Code, Codex,
OpenCode, Gemini CLI and the ACP adapters `@agentclientprotocol/claude-agent-acp` and
`@agentclientprotocol/codex-acp`. None of them is redistributed here. Each is
installed by the owner under its own license.
