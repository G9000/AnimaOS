import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { useConnectorLifetime } from '../../hooks/useConnectorLifetime';
import {
  mcpOwnerApi,
  type McpConnection,
  type McpToolDescriptor,
  type McpGrantOutcome,
} from '../../lib/daemon-api';

export type McpOwnerApi = typeof mcpOwnerApi;
const button =
  'rounded-lg border border-line px-3 py-2 text-xs disabled:opacity-50';
const inputClass =
  'w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm';

function failure(caught: unknown): string {
  const status = (caught as { status?: number } | null)?.status;
  if (status === 409)
    return 'This connection changed. Refresh its status and confirm your action again.';
  if (status === 403) return 'This action is not available for this agent.';
  if (status === 429)
    return 'The connection is busy or its limit has been reached. Refresh before trying again.';
  return 'The result could not be confirmed. Refresh status before taking another action.';
}

function statusText(connection: McpConnection): string {
  if (connection.state === 'stopping') return 'Stopping — waiting for cleanup';
  if (connection.state === 'connecting') return 'Connecting';
  if (!connection.enabled) return 'Saved — disabled';
  switch (connection.state) {
    case 'disconnected':
      return 'Enabled — disconnected';
    case 'needs_review':
      return 'Connected — tools need review';
    case 'ready':
      return 'Connected — reviewed tools ready';
    case 'degraded':
      return 'Connection unavailable';
  }
  return 'Status unknown — refresh before taking action';
}

function launchIssue(
  label: string,
  executable: string,
  args: string[],
): string | null {
  const bytes = (value: string) => new TextEncoder().encode(value).length;
  const control = /[\u0000-\u001f\u007f]/;
  if (!/^(?:\/|[A-Za-z]:[\\/]|\\\\)/.test(executable))
    return 'Enter an absolute executable path.';
  if (control.test(executable) || bytes(executable) > 2048)
    return 'The executable path contains unsupported characters or is too long.';
  if (
    /^(?:[A-Za-z]:[\\/]|\\\\)/.test(executable) &&
    !executable.toLowerCase().endsWith('.exe')
  )
    return 'A Windows executable must end in .exe.';
  if (
    args.length > 32 ||
    args.some((arg) => control.test(arg) || bytes(arg) > 2048)
  )
    return 'Use at most 32 arguments, each within 2048 bytes and without control characters.';
  if (bytes(JSON.stringify({ label, stdio: { executable, args } })) > 16 * 1024)
    return 'The saved launch details exceed the size limit.';
  return null;
}

export function McpConnectionsPanel({
  agentId,
  disabled = false,
  online = true,
  epoch: connectionEpoch = 0,
  permissionsUncertain = false,
  permissionsBusy = false,
  onGrantsChanged,
  onBusyChanged,
  onPermissionsUncertain,
  api = mcpOwnerApi,
}: {
  agentId: string;
  disabled?: boolean;
  online?: boolean;
  epoch?: number;
  permissionsUncertain?: boolean;
  permissionsBusy?: boolean;
  onGrantsChanged?: (toolNames: string[]) => void | Promise<void>;
  onBusyChanged?: (busy: boolean) => void;
  onPermissionsUncertain?: () => void;
  api?: McpOwnerApi;
}) {
  const lifetime = useConnectorLifetime(agentId, online, connectionEpoch);
  const [connections, setConnections] = useState<McpConnection[]>([]);
  const [rowsLifetime, setRowsLifetime] = useState<object | null>(null);
  const [toolNames, setToolNames] = useState<string[] | null>(null);
  const [label, setLabel] = useState('');
  const [endpoint, setEndpoint] = useState('');
  const [connectionType, setConnectionType] = useState<
    'streamableHttp' | 'stdio'
  >('streamableHttp');
  const [executable, setExecutable] = useState('');
  const [argumentsText, setArgumentsText] = useState('');
  const [passEmptyArgument, setPassEmptyArgument] = useState(false);
  const [trustedLocalCode, setTrustedLocalCode] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [needsRefresh, setNeedsRefresh] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const previousLifetime = useRef(lifetime.token);
  if (previousLifetime.current !== lifetime.token) {
    previousLifetime.current = lifetime.token;
    generation.current++;
  }
  const readGeneration = useRef(0);
  const locked = useRef(false);
  const currentAgent = useRef(agentId);
  currentAgent.current = agentId;

  async function refresh(signal?: AbortSignal, explicit = false) {
    if (
      !lifetime.isCurrent(lifetime.token) ||
      locked.current ||
      (explicit && permissionsBusy)
    )
      return;
    const epoch = generation.current;
    const read = ++readGeneration.current;
    const owner = agentId;
    setLoading(true);
    try {
      const [rows, names] = await Promise.all([
        api.list(owner, { signal }),
        api.toolNames(owner),
      ]);
      if (
        epoch !== generation.current ||
        read !== readGeneration.current ||
        currentAgent.current !== owner
      )
        return;
      if (explicit) {
        onBusyChanged?.(true);
        try {
          await onGrantsChanged?.(names);
        } finally {
          if (lifetime.matches(lifetime.token)) onBusyChanged?.(false);
        }
        if (
          epoch !== generation.current ||
          read !== readGeneration.current ||
          currentAgent.current !== owner
        )
          return;
      }
      setConnections(rows);
      setRowsLifetime(lifetime.token);
      setToolNames(names);
      setNeedsRefresh(false);
      setError(null);
    } catch {
      if (
        epoch !== generation.current ||
        read !== readGeneration.current ||
        currentAgent.current !== owner
      )
        return;
      setNeedsRefresh(true);
      setError(
        'Status unavailable. Refresh to check saved connections and permissions.',
      );
    } finally {
      if (
        epoch === generation.current &&
        read === readGeneration.current &&
        currentAgent.current === owner
      )
        setLoading(false);
    }
  }

  useLayoutEffect(() => {
    setConnections([]);
    setLabel('');
    setEndpoint('');
    setConnectionType('streamableHttp');
    setExecutable('');
    setArgumentsText('');
    setPassEmptyArgument(false);
    setTrustedLocalCode(false);
  }, [agentId, api]);

  useLayoutEffect(() => {
    generation.current += 1;
    locked.current = false;
    setToolNames(null);
    setRowsLifetime(null);
    setTrustedLocalCode(false);
    setBusy(false);
    setLoading(online);
    setNeedsRefresh(!online);
    setError(null);
    const controller = new AbortController();
    if (online) void refresh(controller.signal);
    return () => {
      controller.abort();
      generation.current += 1;
      readGeneration.current += 1;
    };
    // An owner change invalidates every read and mutation from the previous view.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId, api, lifetime.token]);

  function replace(connection: McpConnection) {
    setRowsLifetime(lifetime.token);
    setConnections((current) => {
      const exists = current.some((row) => row.id === connection.id);
      return exists
        ? current.map((row) => (row.id === connection.id ? connection : row))
        : [...current, connection];
    });
  }

  async function mutate<T>(
    action: () => Promise<T>,
    accepted: (value: T) => void | Promise<void>,
    affectsPermissions = false,
  ) {
    if (
      !lifetime.isCurrent(lifetime.token) ||
      disabled ||
      loading ||
      needsRefresh ||
      locked.current
    )
      return;
    locked.current = true;
    const epoch = generation.current;
    const owner = agentId;
    readGeneration.current += 1;
    setBusy(true);
    onBusyChanged?.(true);
    // The parent retains this fence across navigation, including an uncertain
    // response or an acknowledgement arriving after this panel unmounts.
    if (affectsPermissions) onPermissionsUncertain?.();
    setError(null);
    try {
      const value = await action();
      if (epoch === generation.current && currentAgent.current === owner)
        await accepted(value);
    } catch (caught) {
      if (epoch === generation.current && currentAgent.current === owner) {
        setNeedsRefresh(true);
        setError(failure(caught));
      }
    } finally {
      if (epoch === generation.current && currentAgent.current === owner) {
        locked.current = false;
        setBusy(false);
      }
      // The parent owns the cross-panel lock until this request settles, even
      // if navigation unmounted this panel. Its callback fences the owner.
      if (lifetime.matches(lifetime.token)) onBusyChanged?.(false);
    }
  }

  const blocked =
    !online ||
    disabled ||
    loading ||
    busy ||
    needsRefresh ||
    permissionsUncertain;
  const launchArgs =
    argumentsText === ''
      ? passEmptyArgument
        ? ['']
        : []
      : argumentsText.split(/\r?\n/);
  const localIssue =
    connectionType === 'stdio'
      ? launchIssue(label, executable, launchArgs)
      : null;
  return (
    <section
      aria-label="External tools (MCP)"
      className="space-y-4 rounded-2xl border border-line bg-panel/60 p-4 sm:p-6"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-ink">
            External tools (MCP)
          </h2>
          <p className="mt-1 max-w-3xl text-xs text-ink-3">
            Save a server, connect, review its tools, then choose which tools
            your agent may use. Saving or reviewing never grants permission.
            These external tools use your existing approval rules.
          </p>
        </div>
        <button
          className={button}
          disabled={!online || busy || loading || permissionsBusy}
          onClick={() => void refresh(undefined, true)}
        >
          Refresh status
        </button>
      </div>
      {loading && (
        <p role="status" className="text-xs text-ink-3">
          Reading saved connections and permissions…
        </p>
      )}
      {error && (
        <p role="alert" className="text-sm text-ink">
          {error}
        </p>
      )}
      {permissionsUncertain && (
        <p className="text-xs text-ink-3">
          Refresh status after the current request finishes to confirm
          permissions before making further changes.
        </p>
      )}
      {disabled && (
        <p className="text-xs text-ink-3">
          Connection changes are unavailable while this agent is busy or
          restricted.
        </p>
      )}
      <form
        className="grid gap-3 sm:grid-cols-2"
        onSubmit={(event) => {
          event.preventDefault();
          const savedLabel = label;
          const savedEndpoint = endpoint;
          if (connectionType === 'stdio' && (!trustedLocalCode || localIssue))
            return;
          const savedInput: Parameters<McpOwnerApi['create']>[1] =
            connectionType === 'stdio'
              ? { label: savedLabel, stdio: { executable, args: launchArgs } }
              : { label: savedLabel, endpoint: savedEndpoint };
          void mutate(
            () => api.create(agentId, savedInput),
            (row) => {
              replace(row);
              setLabel('');
              setEndpoint('');
              setExecutable('');
              setArgumentsText('');
              setPassEmptyArgument(false);
              setTrustedLocalCode(false);
            },
          );
        }}
      >
        <label className="text-xs">
          Connection type
          <select
            className={inputClass}
            value={connectionType}
            disabled={blocked}
            onChange={(event) => {
              setConnectionType(
                event.target.value as 'streamableHttp' | 'stdio',
              );
              setPassEmptyArgument(false);
              setTrustedLocalCode(false);
            }}
          >
            <option value="streamableHttp">HTTP server</option>
            <option value="stdio">Local process</option>
          </select>
        </label>
        <label className="text-xs">
          Connection label
          <input
            className={inputClass}
            value={label}
            maxLength={80}
            disabled={blocked}
            onChange={(event) => setLabel(event.target.value)}
          />
        </label>
        {connectionType === 'streamableHttp' ? (
          <label className="text-xs">
            Server URL
            <input
              className={inputClass}
              value={endpoint}
              maxLength={2048}
              disabled={blocked}
              placeholder="https://server.example/mcp"
              onChange={(event) => setEndpoint(event.target.value)}
            />
          </label>
        ) : (
          <div className="space-y-3 sm:col-span-2">
            <p className="text-xs text-ink-3">
              A local process runs code on this computer. Use an installed
              executable you trust. No shell commands, package launchers,
              installs or environment settings are supported. Saving stays
              disabled and does not start it.
            </p>
            <label className="block text-xs">
              Absolute executable path
              <input
                className={inputClass}
                value={executable}
                maxLength={2048}
                disabled={blocked}
                onChange={(event) => {
                  setExecutable(event.target.value);
                  setTrustedLocalCode(false);
                }}
              />
            </label>
            <label className="block text-xs">
              Arguments — one per line
              <textarea
                className={inputClass}
                value={argumentsText}
                maxLength={16 * 1024}
                disabled={blocked}
                onChange={(event) => {
                  setArgumentsText(event.target.value);
                  setPassEmptyArgument(false);
                  setTrustedLocalCode(false);
                }}
              />
            </label>
            {argumentsText === '' && (
              <label className="flex items-start gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={passEmptyArgument}
                  disabled={blocked}
                  onChange={(event) => {
                    setPassEmptyArgument(event.target.checked);
                    setTrustedLocalCode(false);
                  }}
                />
                Pass one empty argument
              </label>
            )}
            <p className="text-xs text-ink-3">
              Each line is one argument. Spaces and quotes are passed literally;
              blank lines are empty arguments. Launch details cannot be edited
              after saving.
            </p>
            {executable && localIssue && (
              <p className="text-xs text-ink-3">{localIssue}</p>
            )}
            <label className="flex items-start gap-2 text-xs">
              <input
                type="checkbox"
                checked={trustedLocalCode}
                disabled={blocked}
                onChange={(event) => setTrustedLocalCode(event.target.checked)}
              />
              I trust this local program and its arguments
            </label>
          </div>
        )}
        <div className="sm:col-span-2">
          <button
            className={button}
            disabled={
              blocked ||
              !label.trim() ||
              (connectionType === 'stdio'
                ? !trustedLocalCode || localIssue !== null
                : !endpoint.trim())
            }
            type="submit"
          >
            Save disabled connection
          </button>
        </div>
      </form>
      {!loading && !error && connections.length === 0 && (
        <p className="text-sm text-ink-3">No external tool servers saved.</p>
      )}
      {connections.map((row) => (
        <ConnectionCard
          key={`${agentId}:${row.id}`}
          connection={row}
          stale={!online || loading || needsRefresh}
          showSavedTools={online && rowsLifetime === lifetime.token}
          lifetime={lifetime.token}
          toolNames={toolNames}
          blocked={blocked}
          save={(patch, saved) =>
            void mutate(
              () =>
                api.update(agentId, row.id, {
                  expectedVersion: row.version,
                  ...patch,
                }),
              (value) => {
                replace(value);
                saved?.(value);
              },
            )
          }
          connect={() =>
            void mutate(
              () =>
                api.connect(agentId, row.id, { expectedVersion: row.version }),
              replace,
            )
          }
          discover={() =>
            void mutate(
              () =>
                api.discover(agentId, row.id, { expectedVersion: row.version }),
              replace,
            )
          }
          disconnect={() =>
            void mutate(
              () =>
                api.disconnect(agentId, row.id, {
                  expectedVersion: row.version,
                }),
              replace,
            )
          }
          saveBearerCredential={(bearerToken) =>
            mutate(
              () =>
                api.saveBearerCredential(agentId, row.id, {
                  expectedVersion: row.version,
                  bearerToken,
                }),
              replace,
            )
          }
          clearBearerCredential={() =>
            mutate(
              () =>
                api.clearBearerCredential(agentId, row.id, {
                  expectedVersion: row.version,
                }),
              replace,
            )
          }
          review={(tool) => {
            if (!row.discoveryId) return;
            const click = {
              expectedVersion: row.version,
              discoveryId: row.discoveryId,
              descriptorDigest: tool.descriptorDigest,
            };
            void mutate(() => api.review(agentId, row.id, click), replace);
          }}
          grant={(alias, digest, granted) => {
            const click = {
              expectedVersion: row.version,
              descriptorDigest: digest,
              granted,
            };
            void mutate(
              () => api.setGrant(agentId, row.id, alias, click),
              async (outcome: McpGrantOutcome) => {
                replace(outcome.connection);
                setToolNames(outcome.toolNames);
                await onGrantsChanged?.(outcome.toolNames);
              },
              true,
            );
          }}
          remove={() =>
            void mutate(
              () =>
                api.remove(agentId, row.id, { expectedVersion: row.version }),
              async () => {
                const epoch = generation.current;
                setConnections((current) =>
                  current.filter((item) => item.id !== row.id),
                );
                const names = await api.toolNames(agentId);
                if (
                  epoch !== generation.current ||
                  currentAgent.current !== agentId
                )
                  return;
                setToolNames(names);
                await onGrantsChanged?.(names);
              },
              true,
            )
          }
        />
      ))}
    </section>
  );
}

function Descriptor({ tool }: { tool: McpToolDescriptor }) {
  return (
    <div className="min-w-0 space-y-1">
      <p className="break-words text-sm font-medium">{tool.remoteName}</p>
      <p className="whitespace-pre-wrap break-words text-xs text-ink-3">
        {tool.description}
      </p>
      <details className="text-xs text-ink-3">
        <summary>Inspect tool details</summary>
        <p className="mt-2 break-all">Descriptor: {tool.descriptorDigest}</p>
        <pre className="mt-2 max-h-64 overflow-auto whitespace-pre-wrap break-all">
          {JSON.stringify(
            { inputSchema: tool.inputSchema, outputSchema: tool.outputSchema },
            null,
            2,
          )}
        </pre>
      </details>
    </div>
  );
}

function ConnectionCard({
  connection: row,
  stale,
  showSavedTools,
  lifetime,
  toolNames,
  blocked,
  save,
  connect,
  discover,
  disconnect,
  saveBearerCredential,
  clearBearerCredential,
  review,
  grant,
  remove,
}: {
  connection: McpConnection;
  stale: boolean;
  showSavedTools: boolean;
  lifetime: object;
  toolNames: string[] | null;
  blocked: boolean;
  save: (
    patch: { label?: string; endpoint?: string; enabled?: boolean },
    saved?: (row: McpConnection) => void,
  ) => void;
  connect: () => void;
  discover: () => void;
  disconnect: () => void;
  saveBearerCredential: (bearerToken: string) => Promise<void>;
  clearBearerCredential: () => Promise<void>;
  review: (tool: McpToolDescriptor) => void;
  grant: (alias: string, digest: string, granted: boolean) => void;
  remove: () => void;
}) {
  const [label, setLabel] = useState(row.label);
  const [endpoint, setEndpoint] = useState(row.endpoint);
  const [bearerToken, setBearerToken] = useState('');
  const dirty = useRef({ label: false, endpoint: false });
  useEffect(() => {
    if (!dirty.current.label || label === row.label) {
      dirty.current.label = false;
      setLabel(row.label);
    }
    if (!dirty.current.endpoint || endpoint === row.endpoint) {
      dirty.current.endpoint = false;
      setEndpoint(row.endpoint);
    }
  }, [row.label, row.endpoint, row.version, label, endpoint]);
  useEffect(() => {
    // A refreshed version or endpoint requires deliberate re-entry.
    setBearerToken('');
  }, [row.endpoint, row.version, lifetime]);
  const [confirmRemove, setConfirmRemove] = useState<number | null>(null);
  useLayoutEffect(() => {
    setConfirmRemove(null);
  }, [lifetime]);
  const local = row.transport === 'stdio';
  const bearerEligible = !local && row.endpoint.startsWith('https://');
  const wireBusy =
    row.state === 'stopping' ||
    row.state === 'connecting' ||
    row.physicalCallsActive;
  const connected = row.state === 'ready' || row.state === 'needs_review';
  return (
    <article
      aria-label={`External server ${row.label}`}
      className="space-y-3 rounded-xl border border-line p-4"
    >
      <h3 className="break-words font-medium">{row.label}</h3>
      <p className="text-xs text-ink-3">
        {stale ? 'Status unavailable — refresh to verify' : statusText(row)}
      </p>
      {local ? (
        <div className="space-y-1 text-xs text-ink-3">
          <p>Local process — saved launch</p>
          <p className="whitespace-pre-wrap break-all">
            {row.stdio?.executable ?? 'Launch details unavailable'}
          </p>
          {row.stdio &&
            (row.stdio.args.length > 0 ? (
              <ol className="list-decimal space-y-1 pl-5">
                {row.stdio.args.map((arg, index) => (
                  <li key={index} className="whitespace-pre-wrap break-all">
                    {JSON.stringify(arg)}
                  </li>
                ))}
              </ol>
            ) : (
              <p>No arguments</p>
            ))}
          <p>
            Only start code you trust. To change the program or arguments,
            remove this connection and save a new one.
          </p>
        </div>
      ) : (
        <p className="break-all text-xs text-ink-3">{row.endpoint}</p>
      )}
      {row.physicalCallsActive && (
        <p className="text-xs">
          Work or cleanup is still active. Disconnecting does not undo completed
          effects.
        </p>
      )}
      {row.error && (
        <p className="text-xs">
          The connection reported an issue ({row.error.code}). Refresh to check
          its current state.
        </p>
      )}
      {bearerEligible && (
        <div className="space-y-2 rounded-lg border border-line p-3 text-xs">
          <p>
            {row.hasBearerCredential
              ? 'Bearer credential saved'
              : 'No bearer credential saved'}
          </p>
          <p className="text-ink-3">
            A saved credential does not confirm that this server accepts it.
          </p>
          <label className="block">
            Bearer token
            <input
              className={inputClass}
              type="password"
              autoComplete="off"
              spellCheck={false}
              value={bearerToken}
              maxLength={4096}
              disabled={blocked || wireBusy}
              onChange={(event) => setBearerToken(event.target.value)}
            />
          </label>
          <div className="flex flex-wrap gap-2">
            <button
              className={button}
              disabled={blocked || wireBusy || bearerToken.length === 0}
              onClick={() => {
                const submitted = bearerToken;
                setBearerToken('');
                void saveBearerCredential(submitted);
              }}
            >
              {row.hasBearerCredential
                ? 'Replace bearer credential'
                : 'Save bearer credential'}
            </button>
            {row.hasBearerCredential && (
              <button
                className={button}
                disabled={blocked || wireBusy}
                onClick={() => {
                  setBearerToken('');
                  void clearBearerCredential();
                }}
              >
                Clear bearer credential
              </button>
            )}
          </div>
        </div>
      )}
      <details>
        <summary className="text-xs">Edit saved connection</summary>
        <div className="mt-2 grid gap-2 sm:grid-cols-2">
          <label className="text-xs">
            Saved label
            <input
              className={inputClass}
              value={label}
              maxLength={80}
              disabled={blocked || wireBusy}
              onChange={(event) => {
                dirty.current.label = true;
                setLabel(event.target.value);
              }}
            />
          </label>
          {!local && (
            <label className="text-xs">
              Saved server URL
              <input
                className={inputClass}
                value={endpoint}
                maxLength={2048}
                disabled={blocked || wireBusy}
                onChange={(event) => {
                  dirty.current.endpoint = true;
                  setEndpoint(event.target.value);
                }}
              />
            </label>
          )}
          <button
            className={button}
            disabled={
              blocked ||
              wireBusy ||
              !label.trim() ||
              (!local && !endpoint.trim())
            }
            onClick={() =>
              save(local ? { label } : { label, endpoint }, (saved) => {
                dirty.current = { label: false, endpoint: false };
                setLabel(saved.label);
                setEndpoint(saved.endpoint);
              })
            }
          >
            Save connection changes
          </button>
        </div>
      </details>
      <div className="flex flex-wrap gap-2">
        <button
          className={button}
          disabled={blocked || wireBusy}
          onClick={() => save({ enabled: !row.enabled })}
        >
          {row.enabled ? 'Disable connection' : 'Enable connection'}
        </button>
        <button
          className={button}
          disabled={
            blocked ||
            wireBusy ||
            !row.enabled ||
            connected ||
            (local && !row.stdio)
          }
          onClick={connect}
        >
          {local ? 'Start local server' : 'Connect server'}
        </button>
        <button
          className={button}
          disabled={blocked || wireBusy || !connected}
          onClick={discover}
        >
          Refresh available tools
        </button>
        <button
          className={button}
          disabled={
            blocked || row.state === 'stopping' || row.state === 'disconnected'
          }
          onClick={disconnect}
        >
          {local ? 'Stop local server' : 'Disconnect server'}
        </button>
        <button
          className={button}
          disabled={blocked || wireBusy}
          onClick={() => setConfirmRemove(row.version)}
        >
          Remove saved connection
        </button>
      </div>
      {confirmRemove !== null && (
        <div className="space-y-2 text-xs">
          <p>
            Remove this saved connection? Completed tool effects are kept. Only
            this connection will be removed.
          </p>
          {confirmRemove !== row.version && (
            <p>
              The connection changed. Choose Remove saved connection again to
              confirm its current state.
            </p>
          )}
          <div className="flex gap-2">
            <button
              className={button}
              disabled={blocked || wireBusy || confirmRemove !== row.version}
              onClick={remove}
            >
              Confirm removal
            </button>
            <button className={button} onClick={() => setConfirmRemove(null)}>
              Keep connection
            </button>
          </div>
        </div>
      )}
      {showSavedTools && row.candidates.length > 0 && (
        <div className="space-y-3">
          <h4 className="text-sm font-medium">Available tools</h4>
          {row.candidates.map((tool) => {
            const reviewed = row.reviewedTools.some(
              (saved) => saved.descriptorDigest === tool.descriptorDigest,
            );
            return (
              <div
                key={tool.descriptorDigest}
                className="space-y-2 border-t border-line pt-3"
              >
                <Descriptor tool={tool} />
                <p className="text-xs">
                  {reviewed
                    ? 'Reviewed — permission is chosen separately.'
                    : 'Not reviewed — not available to the agent.'}
                </p>
                <button
                  className={button}
                  disabled={
                    blocked ||
                    wireBusy ||
                    !connected ||
                    !row.discoveryId ||
                    reviewed
                  }
                  onClick={() => review(tool)}
                >
                  Review {tool.remoteName}
                </button>
              </div>
            );
          })}
        </div>
      )}
      {showSavedTools && row.reviewedTools.length > 0 && (
        <div className="space-y-3">
          <h4 className="text-sm font-medium">Reviewed permissions</h4>
          {row.reviewedTools.map((tool) => {
            const granted = toolNames?.includes(tool.alias) ?? false;
            const usable =
              !stale &&
              row.enabled &&
              connected &&
              row.candidates.some(
                (candidate) =>
                  candidate.descriptorDigest === tool.descriptorDigest,
              );
            return (
              <div
                key={tool.alias}
                className="space-y-2 border-t border-line pt-3"
              >
                <Descriptor tool={tool} />
                <p className="break-all text-xs">Saved tool: {tool.alias}</p>
                <p className="text-xs">
                  {toolNames === null
                    ? 'Permission unknown'
                    : granted
                      ? 'Permission granted'
                      : 'Permission not granted'}{' '}
                  ·{' '}
                  {usable
                    ? 'Available on this connection'
                    : 'Currently unavailable; saved permission is retained'}
                </p>
                <button
                  className={button}
                  disabled={blocked || toolNames === null}
                  onClick={() =>
                    grant(tool.alias, tool.descriptorDigest, !granted)
                  }
                >
                  {granted ? 'Revoke' : 'Grant'} {tool.remoteName}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </article>
  );
}
