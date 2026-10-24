# Runtime security

The daemon enforces owner identity, native tool grants, execution approvals and execution budgets separately. Tool discovery and a model's output never grant authority. These controls reduce the effect of an injected instruction; they do not establish task-quality parity with another agent.

## Owner access

Every private `/api/*` route requires owner credentials. An allowed `Origin`, referrer, loopback address or Fetch Metadata header alone is insufficient. Browsers send a matching signed HttpOnly cookie and `x-anima-console-session` header; native clients send the local admin bearer credential. Host, Origin and forwarding checks still apply. Public health/documentation routes, expiring signed launch-code exchange, state-authenticated OAuth callbacks and explicitly shared viewer routes retain their own narrow access rules.

The optional `ANIMAOS_RS_API_KEY` is a separate gate. Native SDK clients use `ownerToken` for owner identity and `apiKey` for that additional gate. Browser credentials are not put in image or stream URLs and authenticated fetches do not follow redirects.

The installed supervisor provides a signed launch link. `bun dev --host rust` provisions its owner-token file and prints a signed console URL after readiness. Use `bun dev --console` to obtain a fresh link, or `bun dev --console --playground` for the playground. Standalone daemon deployments must configure `ANIMA_LOCAL_ADMIN_TOKEN_FILE` or `ANIMA_LOCAL_ADMIN_TOKEN`; an unreadable configured token file fails closed. Keep token files outside agent workspaces.

The launcher verifies owner access before reusing an existing daemon. For a daemon started before owner-token provisioning, stop that daemon with its launcher and run `bun dev` again. To reuse a separately started daemon, configure its matching `ANIMA_LOCAL_ADMIN_TOKEN_FILE`.

Library hosts restoring persistence can use `app_with_configured_persistence_for_listener(config, &listener)`. It derives locality from the actual bound TCP listener and retains the same owner credentials and request guards. Serve that router only on the supplied listener. The original persistence constructor has no trusted listener and keeps private routes unavailable.

## Execution defaults

Host code execution is disabled by default, including shell, background jobs, Python programs and interactive/coding-agent input. Use an explicitly configured isolated execution target. An unavailable target never falls back to local execution.

An operator who deliberately wants host access can set `ANIMAOS_ALLOW_HOST_EXECUTION=1` before daemon startup, or set `allowHostExecution: true` in the operator-owned execution profiles file. The file must remain outside the model-writable workspace. This opt-in does not bypass tool grants, the approval floor, current owner authority, execution budgets or identity binding. Free autonomy does not enable it.

Host opt-in gives approved code the OS user's filesystem and network access. Environment filtering and output redaction do not replace an OS sandbox. The Docker target uses a digest-pinned installed image, no host mounts or network, an unprivileged user, a read-only root, dropped capabilities, no-new-privileges and bounded resources. An SSH target is an explicit capability on the configured remote machine and must be secured there.

To configure isolated execution, set `ANIMAOS_EXECUTION_PROFILES_FILE` to an existing absolute JSON file outside the agent workspace. Its top-level fields are `version: 1` and `targets`. Each target has an `id`, `label` and `backend`; a Docker backend has `kind: "docker"`, an absolute installed CLI `executable`, and an installed `image` reference ending in `@sha256:` plus its 64-character digest. The daemon does not pull missing images. `execution_targets` lists these fixed profiles; `execution_run` selects one by `targetId`. Executables, SSH identities and host-pin files must also remain outside the agent workspace.

## Approval and process identity

Before an approval wait, executable paths, executable bytes, the working directory, exact command arguments and identifiable entry scripts are bound. Resolution and bytes are checked again after authority waits and immediately before physical launch. Replacing a script or placing another executable earlier in lookup refuses execution and requires a new request.

The supported binding grammar is deliberately finite: static POSIX commands, supported builtins, explicit entry scripts and supported inline Python/Node programs. Ambiguous shell state changes, expansion, input loaders, unsupported shells and package-loader forms are refused. Binding covers entrypoints; it does not authenticate the entire imported module or subprocess graph.

Each binding reads at most 64 executable, script or launcher-configuration files and 256 MiB in total. On Windows, Git Bash's compatibility launcher and actual shell image are both checked. The default `fstab` mount configuration is checked too; custom `fstab` or `fstab.d` mounts, legacy MSYS symlink files and virtual `/tmp` operands are refused because their shell resolution cannot be safely inferred. Use native filesystem symlinks and explicit Windows absolute entry-script paths. Mount configuration files are limited to 4 KiB each.

Hardened host launches require complete descendant custody before starting. The currently supported mechanism is Windows Job Objects. Interactive ConPTY sessions cannot currently join a Job through the workspace's safe APIs and refuse hardened launch. Unix process groups and the existing same-identity delegated cgroups do not qualify: a child can detach or migrate to a writable parent. Use an isolated execution target on those paths. Generic embeddable process APIs retain their explicitly cooperative behavior; they do not establish an isolation guarantee.

## Network and credential handling

General web fetches accept only public HTTP(S) destinations without embedded credentials. The connector validates the actual DNS answer set and dials those exact addresses, preserving the hostname for HTTP Host and TLS verification. Empty/failed/mixed-private answers, private/special addresses, scoped IPv6 and unsafe redirects fail closed. Implicit environment/OS proxies are disabled for this path. Owner-scoped browser origins and operator-configured service endpoints use separate explicit policies.

Ordinary file tools refuse credential files and directories, including environment files, SSH/cloud stores, known credential JSON files and admin-token files. Recursive search checks discovered files too; existing ancestor aliases are checked before creating new files. These path checks are not a boundary against an already privileged same-user process racing filesystem changes.

Recognizable secrets and credential-labeled structured values are sanitized before tool outputs enter action receipts, retained result artifacts, observers or subsequent model context. Private-key blocks and nested JSON/metadata are included. Numeric token accounting is preserved. Sanitization omissions carry explicit incomplete-output evidence. Heuristic redaction cannot recognize every possible secret, which is why sensitive-file denial and isolated execution are enforced independently.

Background match and exit notifications pass through the same sanitizer before shortening and before follow-up admission, so that asynchronous notes do not bypass the tool-result boundary.

## Verification scope

Regression coverage targets credentialless owner requests, approval decisions, DNS rebinding/private redirects, sensitive-file reads/writes/search, nested secret outputs, executable/script drift, default host denial and unsupported containment refusal. Tests use isolated profiles, workspaces and loopback fixtures. They do not prove remote delivery, provider reasoning quality or a complete autonomous repair-and-restart cycle.

Run `bun run rust:test` for the full Rust verification. Focused daemon integration checks use `bun tools/workspace-dev/scripts/run-rust-daemon-tests.ts --test agent_api` (substitute the target name). The wrapper creates a fresh synthetic owner identity before Cargo starts and removes inherited owner credentials, execution profiles and host-execution opt-ins from the test child environment.

Local reference checkouts inspected for this work were Hermes `519657aa98d4969ec9e23c70c074d1982ef3ccf1` and OpenClaw `034856a7f365293c22adde64946c3f974c7b23f6`. Implementation behavior and verification in this tree remain authoritative.
