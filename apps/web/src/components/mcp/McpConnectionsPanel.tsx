import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useLatest } from '../../hooks/useLatest';
import { useLayoutEffect, useRef, useState, type ReactNode } from 'react';
import { McpOAuthAccount } from './McpOAuthAccount';
import './mcp.css';
import { useConnectorLifetime } from '../../hooks/useConnectorLifetime';
import {
  mcpOwnerApi,
  type McpConnection,
  type McpToolDescriptor,
  type McpGrantOutcome,
} from '../../lib/daemon-api';

export type McpOwnerApi = typeof mcpOwnerApi;
const button = 'ui-button';
const primaryButton = 'ui-button is-primary';
const quietButton = 'ui-button is-quiet';
const dangerButton = 'ui-button is-danger';
const inputClass = 'field';

function failure(caught: unknown): string {
  const status = (caught as { status?: number } | null)?.status;
  if (status === 409)
    return 'This connection changed. Refresh its status and confirm your action again.';
  if (status === 403) return 'This action is not available for this agent.';
  if (status === 429)
    return 'The connection is busy or its limit has been reached. Refresh before trying again.';
  return 'The result could not be confirmed. Refresh status before taking another action.';
}

type Tone = 'ok' | 'wait' | 'off' | 'bad';

function statusTone(connection: McpConnection): Tone {
  if (connection.state === 'stopping' || connection.state === 'connecting')
    return 'wait';
  if (!connection.enabled || connection.state === 'disconnected') return 'off';
  if (connection.state === 'degraded' || connection.toolAccess === 'denied')
    return 'bad';
  if (connection.state === 'needs_review') return 'wait';
  return connection.state === 'ready' ? 'ok' : 'off';
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
      return connection.toolAccess === 'automatic'
        ? 'Connected — tools available automatically'
        : connection.toolAccess === 'denied'
          ? 'Connected — tools blocked by policy'
          : 'Connected — reviewed tools ready';
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
  // eslint-disable-next-line no-control-regex -- Reject control bytes in connection secrets and endpoints.
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
  // The add form opens on request; with nothing saved it is the page.
  const [adding, setAdding] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [needsRefresh, setNeedsRefresh] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const generation = useRef(0);
  const readGeneration = useRef(0);
  const locked = useRef(false);
  const currentAgent = useLatest(agentId);

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

  useResetOnChange([agentId, api], () => {
    setConnections([]);
    setLabel('');
    setEndpoint('');
    setConnectionType('streamableHttp');
    setExecutable('');
    setArgumentsText('');
    setPassEmptyArgument(false);
    setTrustedLocalCode(false);
  });

  useResetOnChange([agentId, api, lifetime.token], () => {
    setToolNames(null);
    setRowsLifetime(null);
    setTrustedLocalCode(false);
    setBusy(false);
    setLoading(online);
    setNeedsRefresh(!online);
    setError(null);
  });
  useLayoutEffect(() => {
    generation.current += 1;
    locked.current = false;
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
  const showAddForm = adding || (!loading && !error && connections.length === 0);
  return (
    <section aria-label="External tools (MCP)" className="mcp-panel">
      <header className="mcp-head">
        <div>
          <h2 className="mcp-title">External tools (MCP)</h2>
          <p className="mcp-lede">
            Servers that give this agent more tools. A new server starts off;
            you connect it when you are ready. Whether its tools need your
            review follows the agent’s permission level.
          </p>
        </div>
        <div className="mcp-head-actions">
          <button
            className={quietButton}
            disabled={!online || busy || loading || permissionsBusy}
            onClick={() => void refresh(undefined, true)}
          >
            Refresh status
          </button>
          {!showAddForm && (
            <button
              className={primaryButton}
              disabled={blocked}
              onClick={() => setAdding(true)}
            >
              Add a server
            </button>
          )}
        </div>
      </header>
      {loading && (
        <p role="status" className="mcp-note">
          Reading saved connections and permissions…
        </p>
      )}
      {error && (
        <p role="alert" className="mcp-alert">
          {error}
        </p>
      )}
      {permissionsUncertain && (
        <p className="mcp-note">
          Refresh status after the current request finishes to confirm
          permissions before making further changes.
        </p>
      )}
      {disabled && (
        <p className="mcp-note">
          Connection changes are unavailable while this agent is busy or
          restricted.
        </p>
      )}
      {showAddForm && (
      <form
        className="mcp-add"
        aria-label="Add a server"
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
              setAdding(false);
            },
          );
        }}
      >
        <h3 className="mcp-add-title">Add a server</h3>
        <label className="mcp-field">
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
        <label className="mcp-field">
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
          <label className="mcp-field mcp-field-wide">
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
          <div className="mcp-field-wide mcp-local">
            <p className="mcp-note">
              A local process runs code on this computer. Use an installed
              executable you trust. No shell commands, package launchers,
              installs or environment settings are supported. Saving stays
              disabled and does not start it.
            </p>
            <label className="mcp-field">
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
            <label className="mcp-field">
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
              <label className="mcp-check">
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
            <p className="mcp-note">
              Each line is one argument. Spaces and quotes are passed literally;
              blank lines are empty arguments. Launch details cannot be edited
              after saving.
            </p>
            {executable && localIssue && (
              <p className="mcp-note">{localIssue}</p>
            )}
            <label className="mcp-check">
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
        <div className="mcp-add-actions">
          <p className="mcp-note">Saved servers start off.</p>
          {connections.length > 0 && (
            <button
              type="button"
              className={quietButton}
              onClick={() => setAdding(false)}
            >
              Cancel
            </button>
          )}
          <button
            className={primaryButton}
            disabled={
              blocked ||
              !label.trim() ||
              (connectionType === 'stdio'
                ? !trustedLocalCode || localIssue !== null
                : !endpoint.trim())
            }
            type="submit"
          >
            Save server
          </button>
        </div>
      </form>
      )}
      {!loading && !error && connections.length === 0 && (
        <p className="mcp-empty">No servers yet.</p>
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
          account={
            row.transport !== 'stdio' && (
              <McpOAuthAccount
                connection={row}
                api={api}
                lifetime={lifetime.token}
                blocked={
                  blocked ||
                  row.state === 'stopping' ||
                  row.state === 'connecting' ||
                  row.physicalCallsActive
                }
                stale={!online || loading || needsRefresh}
                runAction={(action) => mutate(action, () => {})}
                onSettled={() => {
                  setTimeout(() => void refresh(), 0);
                }}
              />
            )
          }
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
    <div className="mcp-tool-text">
      <p className="mcp-tool-name">{tool.remoteName}</p>
      <p className="mcp-tool-description">{tool.description}</p>
      <details className="mcp-tool-details">
        <summary>Inspect tool details</summary>
        <p>Descriptor: {tool.descriptorDigest}</p>
        <pre>
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
  account,
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
  account: ReactNode;
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
  const [dirty, setDirty] = useState({ label: false, endpoint: false });
  useResetOnChange(
    [row.label, row.endpoint, row.version, label, endpoint],
    () => {
      const resetLabel = !dirty.label || label === row.label;
      const resetEndpoint = !dirty.endpoint || endpoint === row.endpoint;
      if (resetLabel) setLabel(row.label);
      if (resetEndpoint) setEndpoint(row.endpoint);
      setDirty({
        label: resetLabel ? false : dirty.label,
        endpoint: resetEndpoint ? false : dirty.endpoint,
      });
    },
  );
  useResetOnChange([row.endpoint, row.version, lifetime], () => {
    // A refreshed version or endpoint requires deliberate re-entry.
    setBearerToken('');
  });
  const [confirmRemove, setConfirmRemove] = useState<number | null>(null);
  useResetOnChange([lifetime], () => {
    setConfirmRemove(null);
  });
  const local = row.transport === 'stdio';
  const bearerEligible =
    !local &&
    (row.endpoint.startsWith('https://') ||
      /^http:\/\/(?:localhost|127\.0\.0\.1|\[::1\])(?::\d+)?(?:\/|$)/.test(
        row.endpoint,
      ));
  const wireBusy =
    row.state === 'stopping' ||
    row.state === 'connecting' ||
    row.physicalCallsActive;
  const connected = row.state === 'ready' || row.state === 'needs_review';
  const tone = stale ? 'off' : statusTone(row);
  return (
    <article
      aria-label={`External server ${row.label}`}
      className="mcp-server"
      data-tone={tone}
    >
      <div className="mcp-server-head">
        <span className="mcp-server-dot" aria-hidden="true" />
        <div className="mcp-server-name">
          <h3>{row.label}</h3>
          <p>
            {local
              ? (row.stdio?.executable ?? 'Launch details unavailable')
              : row.endpoint}
          </p>
        </div>
        <span className="mcp-status" data-tone={tone}>
          {stale ? 'Status unavailable — refresh to verify' : statusText(row)}
        </span>
        <div className="mcp-server-actions">
          {!row.enabled ? (
            <button
              className={button}
              disabled={blocked || wireBusy}
              onClick={() => save({ enabled: true })}
            >
              Enable connection
            </button>
          ) : connected || row.state === 'stopping' ? (
            <button
              className={button}
              disabled={
                blocked ||
                row.state === 'stopping' ||
                row.state === 'disconnected'
              }
              onClick={disconnect}
            >
              {local ? 'Stop local server' : 'Disconnect server'}
            </button>
          ) : (
            <button
              className={primaryButton}
              disabled={blocked || wireBusy || (local && !row.stdio)}
              onClick={connect}
            >
              {local ? 'Start local server' : 'Connect server'}
            </button>
          )}
        </div>
      </div>
      {local && row.stdio && (
        <p className="mcp-server-args">
          {row.stdio.args.length > 0
            ? row.stdio.args.map((arg, index) => (
                <span key={index}>{JSON.stringify(arg)}</span>
              ))
            : 'No arguments'}
        </p>
      )}
      {row.physicalCallsActive && (
        <p className="mcp-note">
          Work or cleanup is still active. Disconnecting does not undo completed
          effects.
        </p>
      )}
      {row.error && (
        <p className="mcp-alert">
          The connection reported an issue ({row.error.code}). Refresh to check
          its current state.
        </p>
      )}
      {row.toolAccess === 'automatic' && (
        <p className="mcp-note">
          Tools from this connection become available automatically after it is
          enabled and connected. New tools are available on the next agent turn.
        </p>
      )}
      {row.toolAccess === 'denied' && (
        <p className="mcp-note">
          MCP tools are blocked by the agent’s execution policy.
        </p>
      )}
      {showSavedTools && row.candidates.length > 0 && (
        <div className="mcp-tools">
          <h4>Available tools</h4>
          {row.candidates.map((tool) => {
            const reviewed = row.reviewedTools.some(
              (saved) => saved.descriptorDigest === tool.descriptorDigest,
            );
            return (
              <div key={tool.descriptorDigest} className="mcp-tool">
                <Descriptor tool={tool} />
                <p className="mcp-tool-state">
                  {row.toolAccess === 'automatic'
                    ? !stale && row.enabled && connected
                      ? 'Available automatically under the agent’s permissions.'
                      : 'Currently unavailable — enable and connect to use automatically.'
                    : row.toolAccess === 'denied'
                      ? 'Blocked by the agent’s execution policy.'
                      : reviewed
                        ? 'Reviewed — permission is chosen separately.'
                        : 'Not reviewed — not available to the agent.'}
                </p>
                {(row.toolAccess ?? 'review') === 'review' && (
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
                )}
              </div>
            );
          })}
        </div>
      )}
      {showSavedTools &&
        (row.toolAccess ?? 'review') === 'review' &&
        row.reviewedTools.length > 0 && (
          <div className="mcp-tools">
            <h4>Reviewed permissions</h4>
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
                <div key={tool.alias} className="mcp-tool">
                  <div className="mcp-tool-text">
                    <Descriptor tool={tool} />
                    <p className="mcp-tool-alias">Saved tool: {tool.alias}</p>
                  </div>
                  <p className="mcp-tool-state">
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
                    className={granted ? button : primaryButton}
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
      <details className="mcp-manage">
        <summary>Manage</summary>
        <div className="mcp-manage-body">
          {account}
          {bearerEligible && !row.hasOAuthCredential && (
            <div className="mcp-token">
              <p>
                {row.hasBearerCredential ? 'Token saved' : 'No token saved'}
                <span className="mcp-note">
                  {' '}
                  · A saved token does not confirm that this server accepts it.
                </span>
              </p>
              <div className="mcp-inline">
                <label className="mcp-field">
                  API token
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
                <button
                  className={button}
                  disabled={blocked || wireBusy || bearerToken.length === 0}
                  onClick={() => {
                    const submitted = bearerToken;
                    setBearerToken('');
                    void saveBearerCredential(submitted);
                  }}
                >
                  {row.hasBearerCredential ? 'Replace token' : 'Save token'}
                </button>
                {row.hasBearerCredential && (
                  <button
                    className={quietButton}
                    disabled={blocked || wireBusy}
                    onClick={() => {
                      setBearerToken('');
                      void clearBearerCredential();
                    }}
                  >
                    Remove token
                  </button>
                )}
              </div>
            </div>
          )}
          <details className="mcp-edit">
            <summary>Edit saved connection</summary>
            <div className="mcp-inline">
              <label className="mcp-field">
                Saved label
                <input
                  className={inputClass}
                  value={label}
                  maxLength={80}
                  disabled={blocked || wireBusy}
                  onChange={(event) => {
                    setDirty((current) => ({ ...current, label: true }));
                    setLabel(event.target.value);
                  }}
                />
              </label>
              {!local && (
                <label className="mcp-field mcp-field-grow">
                  Saved server URL
                  <input
                    className={inputClass}
                    value={endpoint}
                    maxLength={2048}
                    disabled={blocked || wireBusy}
                    onChange={(event) => {
                      setDirty((current) => ({ ...current, endpoint: true }));
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
                    setDirty({ label: false, endpoint: false });
                    setLabel(saved.label);
                    setEndpoint(saved.endpoint);
                  })
                }
              >
                Save connection changes
              </button>
            </div>
            {local && (
              <p className="mcp-note">
                Only start code you trust. To change the program or arguments,
                remove this connection and save a new one.
              </p>
            )}
          </details>
          <div className="mcp-manage-actions">
            <button
              className={button}
              disabled={blocked || wireBusy || !connected}
              onClick={discover}
            >
              Refresh available tools
            </button>
            {row.enabled && (
              <button
                className={button}
                disabled={blocked || wireBusy}
                onClick={() => save({ enabled: false })}
              >
                Disable connection
              </button>
            )}
            <button
              className={dangerButton}
              disabled={blocked || wireBusy}
              onClick={() => setConfirmRemove(row.version)}
            >
              Remove saved connection
            </button>
          </div>
          {confirmRemove !== null && (
            <div className="mcp-confirm" role="group" aria-label="Confirm removal">
              <p>
                Remove this saved connection? Completed tool effects are kept.
                Only this connection will be removed.
              </p>
              {confirmRemove !== row.version && (
                <p>
                  The connection changed. Choose Remove saved connection again
                  to confirm its current state.
                </p>
              )}
              <div className="mcp-inline">
                <button
                  className={dangerButton}
                  disabled={
                    blocked || wireBusy || confirmRemove !== row.version
                  }
                  onClick={remove}
                >
                  Confirm removal
                </button>
                <button
                  className={quietButton}
                  onClick={() => setConfirmRemove(null)}
                >
                  Keep connection
                </button>
              </div>
            </div>
          )}
        </div>
      </details>
    </article>
  );
}
