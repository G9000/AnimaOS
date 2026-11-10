import { useCallback, useEffect, useId, useRef, useState } from 'react';
import type { SaveWorkspaceContext, WorkspaceContextView } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';

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
  return (
    <section
      className="settings-group"
      aria-labelledby={titleId}
      aria-busy={busy || (!view && !error)}
    >
      <h3 className="settings-group-title" id={titleId}>
        M.O.T.H.E.R
      </h3>
      {!view && !error && <p role="status">Loading workspace connection…</p>}
      {error && (
        <p role="alert" className="personality-error">
          {error}
        </p>
      )}
      {view && (
        <>
          <p>
            <strong>
              {invalidBinding
                ? 'Connection needs attention'
                : binding?.mode === 'companion'
                  ? 'M.O.T.H.E.R companion'
                  : binding?.mode === 'shared'
                    ? 'Shared context only'
                    : 'Independent agent'}
            </strong>
          </p>
          {invalidBinding ? (
            <p className="settings-group-help">
              This saved connection could not be read. Clear it here or
              reconnect from M.O.T.H.E.R. The same chat keeps its saved local
              personality.
            </p>
          ) : binding?.mode === 'companion' ? (
            <>
              <p className="settings-group-help">
                Uses your M.O.T.H.E.R personality. Primary memory stays in
                M.O.T.H.E.R.
              </p>
              <p className="settings-group-help">
                When unavailable, continue the same chat with this agent’s saved
                personality. M.O.T.H.E.R memory stays offline until the
                connection returns.
              </p>
            </>
          ) : binding?.mode === 'shared' ? (
            <p className="settings-group-help">
              Keeps its own personality and reads only the records allowed by
              its M.O.T.H.E.R key.
            </p>
          ) : (
            <p className="settings-group-help">
              Uses its own personality and memory. In M.O.T.H.E.R, choose
              Connect AnimaOS to give this agent a companion or shared-context
              key.
            </p>
          )}
          {(binding || invalidBinding) && (
            <>
              <p className="settings-group-help">
                {connection
                  ? `${connection.label} · ${connection.state.replaceAll('_', ' ')}`
                  : 'Connection unavailable'}{' '}
                · Access is checked before each reply.
              </p>
              <button
                type="button"
                className="personality-secondary"
                disabled={disabled || busy || needsReload}
                onClick={() => void disconnect()}
              >
                Use this agent independently
              </button>
              <p className="settings-group-help">
                Change its role or shared records in M.O.T.H.E.R. Other agents
                keep their own keys.
              </p>
            </>
          )}
        </>
      )}
      <button
        type="button"
        className="personality-secondary"
        disabled={disabled || busy}
        onClick={onReload}
      >
        Reload connection
      </button>
    </section>
  );
}
