# animaOS Rust Core

Reusable Rust runtime crates for animaOS. This package owns the reusable execution, memory, and swarm libraries that runnable hosts build on top of.

It does not own the runnable HTTP daemon. The current Rust host remains at [`hosts/rust-daemon`](../../hosts/rust-daemon).

## Crates

| Crate                                                     | What it does                                                                                                                                                                   |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| [`anima-core`](crates/anima-core)                         | Agent execution loop, trait interfaces, and runtime primitives. No HTTP or database implementation details.                                                                    |
| [`anima-service-adapters`](crates/anima-service-adapters) | Reusable browser, connector, bundle, speech/audio and document contracts and validation, including derived-content context costs. Depends on core; core does not depend on it. |
| [`anima-memory`](crates/anima-memory)                     | BM25-backed memory storage and retrieval helpers.                                                                                                                              |
| [`anima-swarm`](crates/anima-swarm)                       | Multi-agent coordination strategies built on top of `anima-core`.                                                                                                              |

## Build

From the repo root:

```bash
bun run --filter core-rust build
```

## Test

From the repo root:

```bash
bun run --filter core-rust test
```

## Architecture

See [ARCHITECTURE.md](./ARCHITECTURE.md) for detailed contributor documentation on the runtime, memory system, swarm coordination, and design rules.
