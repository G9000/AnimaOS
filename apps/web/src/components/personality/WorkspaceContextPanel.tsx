import { useCallback, useEffect, useId, useRef, useState } from 'react';
import type { SaveWorkspaceContext, WorkspaceContextView } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { PenIcon } from '../PenIcon';

type Props = {
  agentId: string;
  disabled?: boolean;
  load?: (
    id: string,
    options: { signal?: AbortSignal },
  ) => Promise<WorkspaceContextView>;
  save?: (
    id: string,
    input: SaveWorkspaceContext,
  ) => Promise<WorkspaceContextView>;
};

/** Changing agents unmounts all drafts and pending requests from the previous agent. */
export function WorkspaceContextPanel(props: Props) {
  const [attempt, setAttempt] = useState(0);
  const reload = useCallback(() => setAttempt((value) => value + 1), []);
  return (
    <WorkspaceContextSettings
      key={`${props.agentId}-${attempt}`}
      {...props}
      onReload={reload}
    />
  );
}

function WorkspaceContextSettings({
  agentId,
  disabled = false,
  load = daemon.workspaceContext,
  save = daemon.saveWorkspaceContext,
  onReload,
}: Props & { onReload: () => void }) {
  const titleId = useId();
  const activeRequest = useRef<AbortController | null>(null);
  const saving = useRef(false);
  const [view, setView] = useState<WorkspaceContextView | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [needsReload, setNeedsReload] = useState(false);

  useEffect(() => {
    const controller = new AbortController();
    activeRequest.current = controller;
    void load(agentId, { signal: controller.signal }).then(
      (next) => {
        if (!controller.signal.aborted) setView(next);
      },
      (caught: unknown) => {
        if (!controller.signal.aborted)
          setError(
            caught instanceof Error
              ? caught.message
              : 'Could not load workspace connection.',
          );
      },
    );
    return () => {
      controller.abort();
    };
  }, [agentId, load]);

  async function disconnect() {
    if (!view || saving.current || needsReload || disabled) return;
    const signal = activeRequest.current?.signal;
    if (!signal || signal.aborted) return;
    saving.current = true;
    setBusy(true);
    setError('');
    try {
      const next = await save(agentId, {
        expectedRevision: view.revision,
        binding: null,
      });
      if (!signal.aborted) setView(next);
    } catch (caught) {
      if (!signal.aborted) {
        setNeedsReload(true);
        setError(
          `${caught instanceof Error ? caught.message : 'Save could not be confirmed.'} Reload to check the saved connection before trying again.`,
        );
      }
    } finally {
      if (!signal.aborted) {
        setBusy(false);
        saving.current = false;
      }
    }
  }

  const binding = view?.binding;
  const invalidBinding = view?.status === 'configured' && !binding;
  const connection = view?.connections.find(
    (candidate) => candidate.id === binding?.connectionId,
  );
  const status = invalidBinding
    ? 'Connection needs attention'
    : binding?.mode === 'companion'
      ? 'M.O.T.H.E.R companion'
      : binding?.mode === 'shared'
        ? 'Shared context only'
        : 'Independent agent';
  const linked = Boolean(binding || invalidBinding);
  const linkState = !linked
    ? null
    : connection
      ? connection.state.replaceAll('_', ' ')
      : 'connection unavailable';
  return (
    <section
      className="settings-item workspace-context-row"
      aria-labelledby={titleId}
      aria-busy={busy || (!view && !error)}
    >
      <span className="settings-item-icon" aria-hidden="true">
        <PenIcon name="plugs-connected-fill" size={16} />
      </span>
      <div className="settings-item-text">
        <h3 id={titleId}>M.O.T.H.E.R</h3>
        {!view && !error && <p role="status">Loading workspace connection…</p>}
        {error && (
          <p role="alert" className="personality-error">
            {error}
          </p>
        )}
        {view && (
          <>
            <p className="workspace-context-summary">
              <strong className="workspace-context-status">{status}</strong>
              {linkState && (
                <span
                  className="workspace-context-state"
                  data-state={connection?.state ?? 'unavailable'}
                >
                  {linkState}
                </span>
              )}
            </p>
            <p>
              {invalidBinding
                ? 'This saved connection could not be read. Clear it here or reconnect from M.O.T.H.E.R; the same chat keeps its saved local personality.'
                : binding?.mode === 'companion'
                  ? 'Uses your M.O.T.H.E.R personality. Primary memory stays there; if it is unreachable, the same chat continues on this agent’s saved personality.'
                  : binding?.mode === 'shared'
                    ? 'Keeps its own personality and reads only the records its M.O.T.H.E.R key allows.'
                    : 'Uses its own personality and memory. To link it, choose Connect AnimaOS in M.O.T.H.E.R.'}
            </p>
          </>
        )}
      </div>
      <div className="workspace-context-actions">
        {view && linked && (
          <button
            type="button"
            className="settings-item-action"
            title="Use this agent independently"
            disabled={disabled || busy || needsReload}
            onClick={() => void disconnect()}
          >
            Unlink
          </button>
        )}
        <button
          type="button"
          className="settings-item-action"
          disabled={disabled || busy}
          onClick={onReload}
          aria-label="Reload connection"
        >
          Reload
        </button>
      </div>
    </section>
  );
}
