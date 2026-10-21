# Operator-configured execution

`bash` keeps its existing local behavior. `execution_run` requires an exact
`targetId` from `execution_targets`; unavailable or unknown targets fail closed.
The ordinary execution grants, approval rules, stop signals, and native action
receipts apply. The model can supply only `targetId`, `command`, relative `cwd`,
and `timeoutMs`. It cannot supply transport arguments or add targets.

Set `ANIMAOS_EXECUTION_PROFILES_FILE` to an existing absolute JSON file outside
the agent workspace. The daemon reads one immutable snapshot at startup. An
absent file setting disables these targets; invalid configured profiles refuse
activation. Executables, SSH keys, and host-pin files must also be outside the
model-writable workspace. Configure these through the operating system rather
than model tools. There is no target discovery from PATH or automatic fallback.

The file has this shape (replace every placeholder with an operator-owned
resource):

```json
{
  "version": 1,
  "targets": [
    {
      "id": "local-workspace",
      "label": "Local workspace",
      "maxTimeoutMs": 120000,
      "backend": {
        "kind": "local",
        "executable": "C:\\Windows\\System32\\cmd.exe",
        "arguments": ["/d", "/s", "/c"],
        "workspaceRoot": "C:\\operator-selected-workspace"
      }
    },
    {
      "id": "isolated-build",
      "label": "Isolated build",
      "maxTimeoutMs": 120000,
      "backend": {
        "kind": "docker",
        "executable": "C:\\Program Files\\Docker\\Docker\\resources\\bin\\docker.exe",
        "image": "operator/image@sha256:REPLACE_WITH_64_HEX_INSTALLED_DIGEST",
        "memoryMb": 256,
        "cpus": 1,
        "pids": 64
      }
    },
    {
      "id": "remote-worker",
      "label": "Remote worker",
      "maxTimeoutMs": 120000,
      "backend": {
        "kind": "ssh",
        "executable": "C:\\Windows\\System32\\OpenSSH\\ssh.exe",
        "host": "operator-selected-host.example",
        "user": "worker",
        "port": 22,
        "identityFile": "C:\\operator-state\\worker-key",
        "knownHostsFile": "C:\\operator-state\\worker-known-hosts",
        "workerEntrypoint": "/opt/anima/bin/anima"
      }
    }
  ]
}
```

Local execution runs with daemon-user operating-system access and confines cwd
selection to its configured workspace. It is not a filesystem sandbox.
The explicit Local backend, like the SSH worker, requires Windows Job Object or
Linux delegated cgroup v2 custody; it refuses a weaker process-group-only launch.

Docker v1 uses an already installed digest-pinned image with `/bin/sh`,
`--pull=never`, no host mounts, no Docker socket, no network, UID/GID 65534,
read-only root, temporary `/workspace` and `/tmp`, dropped capabilities,
`no-new-privileges`, and bounded memory/CPU/process counts. The isolated workspace
starts empty and is removed after execution. The image must declare no persistent
volumes; otherwise dispatch is refused. A successful engine listing must prove
the exact owned container ID absent before target capacity is reused.
Killing the Docker CLI does not satisfy that proof. Existing containers bearing
the target label block new dispatch after daemon restart until an operator
reconciles them.

SSH runs only the configured fixed `anima --execution-worker-v1` entrypoint.
Command text travels in bounded length-prefixed JSON on stdin. Authentication is
noninteractive, selects only the configured identity, strictly checks the
configured known-hosts file, disables agent forwarding, and ignores implicit SSH
configuration. Provision the real Rust binary and pins on the selected endpoint;
this feature does not provision hosts or generate credentials.
Commands have the remote identity's operating-system permissions. The worker
provides process custody and deadlines; use the selected host/account's own
permissions for filesystem and network isolation.

The remote worker requires existing absolute `ANIMAOS_EXECUTION_WORKER_ROOT` and
`ANIMAOS_EXECUTION_WORKER_STATE_DIR` directories. Keep state outside the workspace
and protect it with host permissions. The worker requires Windows Job Object
or Linux delegated cgroup v2 custody before it will start a command. It owns the
child, stops on stdin EOF, enforces an independent deadline, and syncs a signed
terminal receipt after cleanup. A durable active marker refuses overlapping or
unreconciled work. Never clear a stale marker until the exact remote work has
been authoritatively checked.

Connection loss after dispatch reports `outcomeUnknown`. A target remains
occupied while the service queries the fixed worker for a terminal receipt;
untrusted command stdout cannot substitute for that receipt. `execution_status`
reads the latest receipt. If no proof arrives, capacity remains unavailable.
Daemon-memory receipt keys are not recovered after daemon restart; the original
native action therefore stays unresolved unless separately reconciled by an
operator. Captured output is bounded to 8 KiB per stream, and command data to
16 KiB. Each profile permits one physical execution at a time.

Focused tests use controlled subprocess peers for deterministic cleanup and
failure cases. The ignored `execution_live_docker_operator_profile_verifies_isolation_timeout_and_cancel`
test additionally requires explicit `ANIMAOS_EXECUTION_LIVE_DOCKER_PATH` and
`ANIMAOS_EXECUTION_LIVE_DOCKER_IMAGE` settings and access to a real Docker engine.
Controlled peers are not live Docker or SSH acceptance.
