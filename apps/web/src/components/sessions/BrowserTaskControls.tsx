import { useEffect, useRef, useState } from 'react';
import type {
  BrowserPolicy,
  BrowserReadOptions,
  BrowserScope,
  BrowserSelection,
  StopBrowserScopeInput,
  UploadedAttachment,
} from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { MessageAttachments } from '../attachments/MessageAttachments';
import { useLatest } from '../../hooks/useLatest';
import { useRenderScope } from '../../hooks/useRenderScope';

export interface BrowserControlsApi {
  policies(
    agentId: string,
    options?: BrowserReadOptions,
  ): Promise<BrowserPolicy[]>;
  get(
    agentId: string,
    runId: string,
    options?: BrowserReadOptions,
  ): Promise<BrowserScope | null>;
  stop(
    agentId: string,
    runId: string,
    input: StopBrowserScopeInput,
  ): Promise<BrowserScope>;
}

export interface BrowserTaskControlsProps {
  agentId: string;
  runId: string | null;
  disabled: boolean;
  offline?: boolean;
  attachments: readonly UploadedAttachment[];
  selection: BrowserSelection | null;
  onSelection(selection: BrowserSelection | null): void;
  api?: BrowserControlsApi;
}

export function BrowserTaskControls({
  agentId,
  runId,
  disabled,
  offline = false,
  attachments,
  selection,
  onSelection,
  api = daemon.browserScopes,
}: BrowserTaskControlsProps) {
  const key = `${agentId}\u0000${runId ?? ''}`;
  const { generation } = useRenderScope(key);
  const boundary = useLatest({ key, generation });
  const mounted = useRef(true);
  const details = useRef<HTMLDetailsElement>(null);
  const sequence = useRef(0);
  const controller = useRef<AbortController | null>(null);
  const [loaded, setLoaded] = useState<{
    key: string;
    policies: BrowserPolicy[];
    scope: BrowserScope | null;
  } | null>(null);
  const [pending, setPending] = useState<string | null>(null);
  const [failure, setFailure] = useState<{ key: string; text: string } | null>(
    null,
  );
  const [mutating, setMutating] = useState<string | null>(null);
  const mutation = useRef<string | null>(null);
  const current = loaded?.key === key ? loaded : null;
  const scope = current?.scope ?? null;
  const reading = pending === key;
  const busy = mutating === key;

  const read = async () => {
    if (offline || mutation.current === key) return;
    controller.current?.abort();
    const request = new AbortController();
    controller.current = request;
    const attempt = ++sequence.current;
    const active = () =>
      mounted.current &&
      boundary.current.generation === generation &&
      sequence.current === attempt &&
      !request.signal.aborted;
    setPending(key);
    setFailure(null);
    try {
      const [policies, selectedScope] = await Promise.all([
        api.policies(agentId, { signal: request.signal }),
        runId
          ? api.get(agentId, runId, { signal: request.signal })
          : Promise.resolve(null),
      ]);
      if (active()) setLoaded({ key, policies, scope: selectedScope });
    } catch {
      if (active()) {
        setLoaded(null);
        setFailure({
          key,
          text: 'Browser status could not be read. Refresh to check it.',
        });
      }
    } finally {
      if (active()) setPending(null);
    }
  };

  useEffect(() => {
    mounted.current = true;
    if (details.current?.open) void read();
    return () => {
      mounted.current = false;
      controller.current?.abort();
    };
    // The owner/run identity owns the read; changing a draft does not reload it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, api]);

  const stop = async () => {
    if (!scope || !runId || offline || mutation.current === key) return;
    mutation.current = key;
    setMutating(key);
    setFailure(null);
    controller.current?.abort();
    ++sequence.current;
    setPending(null);
    try {
      const stopped = await api.stop(agentId, runId, {
        expectedVersion: scope.version,
      });
      if (mounted.current && boundary.current.generation === generation)
        setLoaded({ key, policies: current?.policies ?? [], scope: stopped });
    } catch {
      if (mounted.current && boundary.current.generation === generation)
        setFailure({
          key,
          text: 'Browser Stop was not confirmed. Refresh its status before trying again.',
        });
    } finally {
      if (mutation.current === key) mutation.current = null;
      if (mounted.current && boundary.current.generation === generation)
        setMutating(null);
    }
  };
  const policy = current?.policies.find(
    (item) => item.id === selection?.policyId,
  );
  const selectedIds = selection?.selectedInputIds ?? [];
  const selectedBytes = attachments
    .filter((item) => selectedIds.includes(item.id))
    .reduce((sum, item) => sum + item.size, 0);
  return (
    <details
      ref={details}
      className="border-t border-line px-4 py-2 text-xs"
      onToggle={(event) => {
        if (
          event.currentTarget.open &&
          !current &&
          !reading &&
          (!controller.current || controller.current.signal.aborted)
        )
          void read();
      }}
    >
      <summary className="cursor-pointer text-ink-2">Browser task</summary>
      <div
        className="mt-2 space-y-2"
        role="region"
        aria-label="Browser task controls"
      >
        <p>
          Choose a browser policy for your next message. Tool access and action
          approvals are managed separately.
        </p>
        <button
          type="button"
          disabled={offline || reading || busy}
          onClick={() => void read()}
        >
          Refresh browser status
        </button>
        {reading && <p role="status">Reading browser status…</p>}
        {failure?.key === key && <p role="alert">{failure.text}</p>}
        {current && (
          <>
            <label>
              Browser policy for next message{' '}
              <select
                aria-label="Browser policy for next message"
                disabled={disabled || offline || busy || reading}
                value={selection?.policyId ?? ''}
                onChange={(event) => {
                  const chosen = current.policies.find(
                    (item) =>
                      item.id === event.target.value && item.workerAvailable,
                  );
                  onSelection(
                    chosen
                      ? { policyId: chosen.id, selectedInputIds: [] }
                      : null,
                  );
                }}
              >
                <option value="">No browser task</option>
                {current.policies.map((item) => (
                  <option
                    key={item.id}
                    value={item.id}
                    disabled={!item.workerAvailable}
                  >
                    {item.id} — {item.origin}
                    {item.workerAvailable ? '' : ' (unavailable)'}
                  </option>
                ))}
              </select>
            </label>
            {current.policies.length === 0 && (
              <p>No browser policy is installed on this daemon.</p>
            )}
            {policy && (
              <>
                <p>
                  {policy.origin} · {policy.limits.actions} actions ·{' '}
                  {policy.limits.scopeMs / 1000} seconds
                </p>
                <ul aria-label="Allowed browser routes">
                  {policy.routes.map((route) => (
                    <li key={route.id}>
                      {route.method} {route.path}
                    </li>
                  ))}
                </ul>
                {policy.origin.startsWith('https://') ? (
                  <p>Uploaded files aren’t shared with public pages.</p>
                ) : (
                  attachments.map((item) => (
                    <label key={item.id} className="block">
                      <input
                        type="checkbox"
                        checked={selectedIds.includes(item.id)}
                        disabled={
                          disabled ||
                          offline ||
                          busy ||
                          item.kind !== 'text' ||
                          item.size > policy.limits.inputBytes ||
                          (!selectedIds.includes(item.id) &&
                            selectedBytes + item.size >
                              policy.limits.inputBytes)
                        }
                        onChange={(event) =>
                          onSelection({
                            policyId: policy.id,
                            selectedInputIds: event.target.checked
                              ? [
                                  ...selectedIds.filter((id) => id !== item.id),
                                  item.id,
                                ]
                              : selectedIds.filter((id) => id !== item.id),
                          })
                        }
                      />{' '}
                      Share {item.name} ({item.size} bytes)
                    </label>
                  ))
                )}
                <p>
                  Selection applies once. Reloading or recovering a message
                  requires choosing browser access again.
                </p>
              </>
            )}
            {scope && (
              <div aria-label="Current browser scope">
                <p>
                  Browser scope: {scope.state} · {scope.actionsUsed} actions ·{' '}
                  {scope.requestsUsed} requests
                </p>
                <p>
                  Expires {new Date(scope.expiresAtMs).toLocaleTimeString()}
                </p>
                {scope.physicalLeaseHeld && (
                  <p>
                    Physical browser cleanup is pending until the daemon
                    confirms closure.
                  </p>
                )}
                {scope.error && (
                  <p role="status">
                    The browser scope needs attention. Refresh its status.
                  </p>
                )}
                <button
                  type="button"
                  disabled={
                    offline ||
                    busy ||
                    reading ||
                    (scope.state === 'closed' && !scope.physicalLeaseHeld)
                  }
                  onClick={() => void stop()}
                >
                  Stop browser
                </button>
                <MessageAttachments
                  agentId={agentId}
                  attachments={scope.artifacts.map((artifact) => ({
                    id: artifact.id,
                    type: artifact.kind === 'screenshot' ? 'image' : 'file',
                    name:
                      artifact.kind === 'csv'
                        ? 'browser-output.csv'
                        : 'browser-capture.jpg',
                    mime: artifact.kind === 'csv' ? 'text/csv' : 'image/jpeg',
                    size: artifact.bytes,
                  }))}
                />
                {scope.artifacts.map((artifact) => (
                  <p key={artifact.id}>
                    {artifact.kind}: {artifact.bytes} bytes · SHA-256{' '}
                    {artifact.sha256}
                  </p>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </details>
  );
}
