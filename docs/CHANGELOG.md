# Documentation changelog

## 2026-10-23

- Document bound turn progress, conservative startup recovery, durable secondary-call usage and snapshot format 27. Add real child-process crash acceptance and a six-case Rust trace-evaluation battery; distinguish deterministic and provider evidence.

- Document opt-in skill evaluation before promotion, historical compaction checkpoints, request-copy output pruning, and completion evidence bound to file versions. Describe approval/Undo boundaries, provider and usage limits, and the SDK controls; update the Rust daemon API inventory to 290 operations.

## 2026-10-20 (reference cleanup)

- Removed optional comparison branding and contribution prose from documentation, design text and source comments. Renamed component license files to generic third-party notices while retaining their copyright and full MIT terms.

## 2026-10-20 (legacy removal)

- Removed the legacy TypeScript server (`apps/server`), the TypeScript core and its libraries (`packages/core-ts`, `swarm`, `tools`, `memory`, `mod-sdk`), the mod system, the agency generator, and the Elixir and Python placeholder hosts. The SDK, CLI and TUI are now thin clients of the Rust daemon.
- Updated the README, AGENTS.md, SDK usage guide, package READMEs, PRD architecture tables and the docs sites to match. Removed the stale package maturity scorecard and the April web UI design docs.

## 2026-10-20

- Refresh the root README around AnimaOS as an agentic operating system, with current autonomy controls, setup, architecture, and implementation limits.
- Replace the README screenshot gallery with the supplied AnimaOS banner.
- Prepare a smaller JPG of the banner for manual upload as the GitHub repository's social preview.
