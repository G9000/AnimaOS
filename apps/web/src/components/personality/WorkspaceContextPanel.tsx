import { useCallback, useEffect, useId, useRef, useState } from 'react';
import type {
  MemoryProviderBinding,
  SaveWorkspaceContext,
  WorkspaceContextView,
} from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { PenIcon } from '../PenIcon';
import { GardenPublicationsPanel } from './GardenPublicationsPanel';

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
  const [reviewConversion, setReviewConversion] = useState(false);
  const [personaDraft, setPersonaDraft] = useState<
    MemoryProviderBinding['personaSource'] | null
  >(null);

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

  async function update(input: Omit<SaveWorkspaceContext, 'expectedRevision'>) {
    if (!view || saving.current || needsReload || disabled) return;
    const signal = activeRequest.current?.signal;
    if (!signal || signal.aborted) return;
    saving.current = true;
    setBusy(true);
    setError('');
    try {
      const next = await save(agentId, {
        expectedRevision: view.revision,
        ...input,
      });
      if (!signal.aborted) {
        setView(next);
        setReviewConversion(false);
        setPersonaDraft(null);
      }
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
  const provider = view?.providerBinding;
  const invalidBinding = view?.status === 'configured' && !binding && !provider;
  const connection = view?.connections.find(
    (candidate) =>
      candidate.id === (provider ?? binding)?.connectionId &&
      candidate.tool === (provider ? 'mother_get_context' : binding?.tool),
  );
  const status = invalidBinding
    ? 'Connection needs attention'
    : provider
      ? 'M.O.T.H.E.R garden enabled'
      : binding?.mode === 'companion'
        ? 'M.O.T.H.E.R companion'
        : binding?.mode === 'shared'
          ? 'Shared context only'
          : 'Independent agent';
  const linked = Boolean(binding || provider || invalidBinding);
  const locked = disabled || busy || needsReload;
  const conversion: MemoryProviderBinding | null =
    binding &&
    !provider &&
    binding.subject === agentId &&
    binding.tool === 'mother_get_context' &&
    binding.workspaceId &&
    binding.generation
      ? {
          version: 1,
          provider: 'mother',
          connectionId: binding.connectionId,
          workspaceId: binding.workspaceId,
          generation: binding.generation,
          subject: binding.subject,
          publication: 'selected',
          accessMode: binding.mode,
          personaSource: binding.mode === 'companion' ? 'workspace' : 'local',
        }
      : null;
  const persona = personaDraft ?? provider?.personaSource ?? 'local';
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
                : provider
                  ? 'Anima keeps its own memory, capture and learning. M.O.T.H.E.R adds the shared context its key allows, when available.'
                  : binding?.mode === 'companion'
                    ? 'Uses your M.O.T.H.E.R personality. Primary memory stays there; if it is unreachable, the same chat continues on this agent’s saved personality.'
                    : binding?.mode === 'shared'
                      ? 'Keeps its own personality and reads only the records its M.O.T.H.E.R key allows.'
                      : 'Uses its own personality and memory. To link it, choose Connect AnimaOS in M.O.T.H.E.R.'}
            </p>
            {provider && (
              <>
                <p>
                  <strong>Native memory active</strong>
                </p>
                <p>
                  Connection: {connection?.label ?? provider.connectionId} (
                  {provider.connectionId}). Role: {provider.accessMode}.
                </p>
                <label>
                  Personality source
                  <select
                    value={persona}
                    disabled={locked}
                    onChange={(event) =>
                      setPersonaDraft(
                        event.target.value === 'workspace'
                          ? 'workspace'
                          : 'local',
                      )
                    }
                  >
                    <option value="local">
                      This agent’s local personality
                    </option>
                    {provider.accessMode === 'companion' && (
                      <option value="workspace">M.O.T.H.E.R workspace</option>
                    )}
                  </select>
                </label>
                <button
                  type="button"
                  className="settings-item-action"
                  disabled={locked || persona === provider.personaSource}
                  onClick={() =>
                    void update({
                      binding: null,
                      providerBinding: { ...provider, personaSource: persona },
                    })
                  }
                >
                  Save personality source
                </button>
                <p>
                  Disabling M.O.T.H.E.R stops automatic shared recall and
                  workspace personality, and uses the saved local personality.
                  It does not revoke its key or delete remote records.
                  Separately granted MCP tools remain available; revoke the key
                  separately to remove access. No remote write is made.
                </p>
              </>
            )}
            {conversion &&
              (reviewConversion ? (
                <section aria-label="Review additive connection">
                  <h4>Review additive connection</h4>
                  <p>
                    Connection: {connection?.label ?? conversion.connectionId} (
                    {conversion.connectionId}). Role: {conversion.accessMode}.
                  </p>
                  <p>
                    Workspace: {conversion.workspaceId}. Generation:{' '}
                    {conversion.generation}. Agent: {conversion.subject}.
                  </p>
                  <p>
                    {conversion.personaSource === 'workspace'
                      ? 'Keep the M.O.T.H.E.R personality, with the saved local personality as fallback.'
                      : 'Keep this agent’s local personality.'}
                  </p>
                  <p>
                    Native memory, capture and learning resume. M.O.T.H.E.R
                    becomes an additional source of shared context.
                  </p>
                  <p>
                    Existing chats, local data and grants stay unchanged. This
                    conversion does not publish memories or make remote writes.
                  </p>
                  <button
                    type="button"
                    className="settings-item-action"
                    disabled={locked}
                    onClick={() =>
                      void update({
                        binding: null,
                        providerBinding: conversion,
                      })
                    }
                  >
                    Confirm additive connection
                  </button>
                  <button
                    type="button"
                    className="settings-item-action"
                    disabled={locked}
                    onClick={() => setReviewConversion(false)}
                  >
                    Cancel
                  </button>
                </section>
              ) : (
                <>
                  <p>
                    Existing connection behavior remains active until you
                    confirm conversion.
                  </p>
                  <button
                    type="button"
                    className="settings-item-action"
                    disabled={locked}
                    onClick={() => setReviewConversion(true)}
                  >
                    Review additive connection
                  </button>
                </>
              ))}
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
            onClick={() =>
              void update(
                provider || invalidBinding
                  ? { binding: null, providerBinding: null }
                  : { binding: null },
              )
            }
          >
            {provider ? 'Disable M.O.T.H.E.R' : 'Unlink'}
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
      {view && <GardenPublicationsPanel agentId={agentId} disabled={disabled || busy} />}
    </section>
  );
}
