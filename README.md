![AnimaOS: I'm at your command.](docs/images/animaos-banner.png)

# AnimaOS

**An agentic operating system**

AnimaOS brings persistent AI agents, memory, tools, and automations into one personal workspace. It runs on your computer or your own server, on top of your existing operating system. You choose the models, connect the tools, and decide how independently your agents can work.

Talk to anima in the web console or through a connected chat app. Give it work to do, let it bring in helpers, and follow its plans, actions, and results in the same place. Its context carries across conversations through saved memory and reusable skills.

This repository contains the system: a reusable Rust agent runtime, the daemon that hosts it, a TypeScript SDK, CLI and terminal UI, and the web console.

## What you can do

- **Keep agents for different parts of your life.** Each agent has its own main conversation and memory. Switch between them in the console and return to their saved context.
- **Work with files, code, and documents.** Agents can use workspace tools, run commands and background processes, research the web, and use configured services such as Google Drive, Calendar, and mail. Reviewed MCP tools and community bundles extend what they can use.
- **Delegate bigger jobs.** An agent can bring in helpers for research, coding, or review, supervise their work, and bring the results back to your conversation.
- **Build useful memory and skills.** Save preferences and facts, recall earlier work, and turn successful tasks into reusable skills. Review and manage what your agents keep.
- **Schedule recurring work.** Automations and background tasks run in the daemon while the console is closed. The daemon must stay running for that work to continue.
- **Use chat and voice.** Type, dictate, or listen to replies. Paired private Telegram messages join the main conversation; Telegram groups and Discord channels have separate threads. Recorded-audio transcription and selected-reply speech are available as server opt-ins.
- **See what happened.** Plans, helper progress, action records, a Needs-you inbox, and weekly reports make work, approvals, failures, and recorded usage visible.

## Choose how independently agents work

Set autonomy per agent in Settings:

- **Free:** carries out ordinary work and reports what it did, with additional checks for sensitive actions.
- **Strict:** asks before changes, command execution, or delegation.
- **Custom:** uses the individual approval rules you configure.

Blocked commands and protected-action checks still apply. Plans and approvals follow the selected policy; granting tool access does not make every action unconditional.

## Current scope

The working stack is the Rust daemon and web console, with persistent agents, memory, tools, delegation, schedules, and embedded Telegram and Discord connectors. Local models are supported through Ollama; hosted providers are also available.

AnimaOS is under active development. Tool and connector capabilities have specific setup and scope limits, documented in the [daily-use guide](docs/companion/daily-use.md). Browser work is currently scoped; see [browser support and limits](docs/companion/browser-work.md). Interrupted work may require review after a daemon restart. Full mission resume, universal undo, and wake-word voice are not complete.

The [VPS deployment package](deploy/vps/README.md) targets one trusted owner. Public deployment, real account integrations, and microphone behavior still have manual acceptance checks; see the guide for the current verification boundary.

## Install the desktop application

Native packages are built for Windows x64 (setup EXE), Linux x64 (Debian and
AppImage), and macOS Apple Silicon and Intel (app and DMG). Open **Anima** after
installation to start browser onboarding; terminal setup is not required.
See the [installation guide](docs/companion/installing.md) for platform steps,
updates, and the current validation and signing status. Build packages from source
with the [release guide](tools/release/README.md#native-desktop-installers).

## Development quick start

Install Bun and the Rust toolchain, then run from the repository root:

```bash
bun install --frozen-lockfile
bun run --filter @animaos/sdk build
bun dev --host rust
```

The launcher automatically opens an approved Anima page once the web console and daemon are ready. Create your companion, select its workspace, and connect a model provider. Onboarding covers the model, introductions, access, and voice.

By default, the launcher starts the Rust daemon/API and the web console. Run `bun dev --console` to reopen an approved page while the services are running. Use `bun dev --no-open` to keep browser opening manual; the terminal prints a fresh owner link. The console connects to the daemon at `127.0.0.1:8080`. Restored Telegram and Discord connectors run inside the daemon.

Run the optional surfaces explicitly from the repository root:

| Command | Starts |
| --- | --- |
| `bun dev` | Daemon/API and web console |
| `bun run dev:docs` | User and contributor documentation sites |
| `bun run dev:game` | Village game host and browser |
| `bun run dev:playground` | Daemon/API and playground |
| `bun run dev:all` | Daemon/API, web, playground, both docs sites, and game |

Docs and game commands run independently of the assistant daemon. The game keeps its own ports, saves, model-connection resolution and explicit durable-engine configuration; see the [Village instructions](poc/village/README.md). Install its dependencies once from `poc/village` with `bun install`.

Rust build outputs share a 32 GiB budget under `target/cargo`. The wrapped build and test commands clean inactive output when needed and check running builds every 15 seconds, stopping their own command if it exceeds the budget. Running Anima and Village instances are protected. Validation reuses `target/cargo/validation`; avoid creating a new target folder for each feature. Development and test profiles disable incremental state and keep only limited debug information.

Use `bun run rust:cache:status` to inspect cache size. For an explicit full reset, stop programs/builds using the cache, then run `bun run rust:cache:clean`; later builds will recompile. The maximum managed cache budget is 32 GiB. `ANIMA_RUST_CACHE_BUDGET_GIB` can lower it; higher values, including inherited overrides, are capped at 32 GiB, and child commands receive the effective limit. Checks permit temporary overshoot between scans, and raw Cargo commands or explicitly selected external target directories are outside the wrapper's live monitoring.

- **Run it on a server:** the [VPS deployment guide](deploy/vps/README.md) covers HTTPS, owner authentication, credentials, backups and current limits.
- **Day-to-day setup:** [Anima daily-use setup](docs/companion/daily-use.md) covers identity files, memory, approvals, schedules and connectors.

## Architecture

Reusable engine code lives in `packages/`; runnable backend processes live in `hosts/`; user-facing surfaces live in `apps/`. Host selection stays in `tools/workspace-dev`.

| Path                                                    | What lives there                                                                                                                                                                              |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `packages/core-rust`                                    | The reusable Rust runtime: `anima-core`, `anima-memory`, `anima-swarm`, `anima-schedule`, `anima-harness`, model adapters, and document support. The core stays independent of backend hosts. |
| `hosts/rust-daemon`                                     | `anima-daemon`, the runnable Axum host. It exposes HTTP and SSE, and embeds the Telegram and Discord connectors.                                                                              |
| `packages/sdk`                                          | The public TypeScript client for the daemon. It owns the shared TypeScript types.                                                                                                             |
| `packages/cli`, `packages/tui`, `packages/ui`           | The `animaos` command line, its terminal UI, and shared web components. All are thin clients of the daemon.                                                                                   |
| `apps/web`                                              | The companion web console (React and Vite).                                                                                                                                                   |
| `apps/playground`, `apps/docs`, `apps/contributor-docs` | The playground and the two Astro documentation sites.                                                                                                                                         |
| `tools/workspace-dev`                                   | The `bun dev` launcher, which picks the host and starts the browser apps.                                                                                                                     |
| `deploy/vps`                                            | Docker Compose, Caddy and the daemon image for a one-owner server.                                                                                                                            |
| `design/`                                               | The pen.dev design file and the orb shader.                                                                                                                                                   |

More detail: [Rust daemon architecture](docs/rust-daemon-architecture.md), [swarm architecture](docs/swarm-architecture.md).

Workspace and npm package names use `animaos` / `@animaos`; use those names in the commands below.

## Design

The interface is designed in [pen.dev](https://pen.dev) before it is built. [`design/design.pen`](design/design.pen) holds every screen:

- the end-to-end journey and onboarding
- the main chat, its states (working, offline, errors, plans, receipts) and the agent switcher
- the Needs-you inbox and the weekly report
- chat moments, helpers, and every Settings page
- the components sheet, mascot explorations and the landing page

The orb is a shader, [`design/shaders/presence.glsl`](design/shaders/presence.glsl).

UI changes start in the design file and are built from it.

## Development

See [docs/testing.md](docs/testing.md) for focused validation commands and the [contributor playbook](apps/contributor-docs/src/content/docs/workflow/playbook.mdx) for a step-by-step bring-up.

```bash
bun run build              # build all packages
bun run test               # run all tests
bun run typecheck          # check all workspace types
bun run lint               # lint workspace code

bun dev                    # daemon/API + web console (same as bun dev --host rust)
bun run dev:docs           # user and contributor docs only
bun run dev:game           # Village game host + browser
bun run dev:playground     # daemon/API + playground
bun run dev:all            # all development services, including the game
bun run daemon             # the Rust daemon only
bun run web                # the web console only (daemon must be running)
```

Use focused workspace scripts, for example `bun run --filter rust-daemon test` or `bun run --filter @animaos/web test`. Run that workspace's `typecheck` and `build` scripts as needed.

## CLI and SDK

The CLI and terminal UI remain available for agency and SDK workflows. `create` runs locally; `launch`, `run` and `chat` talk to the daemon.

```bash
bun run build:cli-sdk
export OPENAI_API_KEY="sk-..."
bun run animaos create my-agency --provider openai --model gpt-4o-mini

bun run daemon                                   # in another terminal
cd my-agency
bun run animaos launch "Write a blog post about AI" # TUI
bun run animaos launch --no-tui "your task"         # plain text, one task
bun run animaos launch --no-tui                     # plain-text prompt (/help, /health, exit)
```

Use the repo-local CLI while developing: Bun runs this workspace's `animaos` build. See [docs/SDK_USAGE.md](docs/SDK_USAGE.md) for the SDK.

For task progress, background helpers, local Python, and selected-file recovery, see the [Rust autonomous runtime guide](hosts/rust-daemon/README.md#autonomous-runtime).

### Model providers

| Provider      | Accepted names     | Key env vars                                                                                      | Base URL env var      | Notes                    |
| ------------- | ------------------ | ------------------------------------------------------------------------------------------------- | --------------------- | ------------------------ |
| OpenAI        | `openai`           | `OPENAI_API_KEY`, `OPENAI_KEY`, `OPENAI_TOKEN`                                                    | `OPENAI_BASE_URL`     | Native chat completions  |
| Anthropic     | `anthropic`        | `ANTHROPIC_API_KEY`, `ANTHROPIC_KEY`, `ANTHROPIC_TOKEN`, `CLAUDE_API_KEY`                         | `ANTHROPIC_BASE_URL`  | Native Messages API      |
| Google Gemini | `google`, `gemini` | `GOOGLE_API_KEY`, `GOOGLE_KEY`, `GOOGLE_AI_KEY`, `GEMINI_API_KEY`, `GOOGLE_GENERATIVE_AI_API_KEY` | `GOOGLE_BASE_URL`     | Native `generateContent` |
| Ollama        | `ollama`           | `OLLAMA_API_KEY`                                                                                  | `OLLAMA_BASE_URL`     | Local; OpenAI-compatible |
| Groq          | `groq`             | `GROQ_API_KEY`, `GROQ_KEY`, `GROQ_TOKEN`                                                          | `GROQ_BASE_URL`       | OpenAI-compatible        |
| xAI           | `xai`, `grok`      | `XAI_API_KEY`, `XAI_KEY`, `GROK_API_KEY`                                                          | `XAI_BASE_URL`        | OpenAI-compatible        |
| OpenRouter    | `openrouter`       | `OPENROUTER_API_KEY`, `OPENROUTER_KEY`, `OPENROUTER_TOKEN`                                        | `OPENROUTER_BASE_URL` | OpenAI-compatible        |
| Mistral       | `mistral`          | `MISTRAL_API_KEY`, `MISTRAL_KEY`, `MISTRAL_TOKEN`                                                 | `MISTRAL_BASE_URL`    | OpenAI-compatible        |
| Together      | `together`         | `TOGETHER_API_KEY`, `TOGETHER_KEY`, `TOGETHER_TOKEN`                                              | `TOGETHER_BASE_URL`   | OpenAI-compatible        |
| DeepSeek      | `deepseek`         | `DEEPSEEK_API_KEY`                                                                                | `DEEPSEEK_BASE_URL`   | OpenAI-compatible        |
| Fireworks     | `fireworks`        | `FIREWORKS_API_KEY`                                                                               | `FIREWORKS_BASE_URL`  | OpenAI-compatible        |
| Perplexity    | `perplexity`       | `PERPLEXITY_API_KEY`                                                                              | `PERPLEXITY_BASE_URL` | OpenAI-compatible        |

`--api-key` overrides the environment for one CLI call. For daemon-backed commands, credentials are forwarded per request, so changing keys doesn't need a daemon restart. In the web console, the ChatGPT subscription and server-side keys are set up during onboarding or later in Settings → Model.

## Tech stack

Rust (Axum, SQLite), Bun, TypeScript, React, Vite, Tailwind, Vitest, Playwright, and pen.dev for design.

## License

MIT
