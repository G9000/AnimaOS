import { useEffect, useRef, useState } from 'react';
import type { WorkspaceContextView } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';

interface Props {
  agentId: string;
  name: string;
  onDone(): void;
  load?: (
    id: string,
    options: { signal?: AbortSignal },
  ) => Promise<WorkspaceContextView>;
}

/** A new companion owns this pairing screen; old replies cannot cross agents. */
export function MotherOnboarding(props: Props) {
  return <MotherPairing key={props.agentId} {...props} />;
}

function MotherPairing({
  agentId,
  name,
  onDone,
  load = daemon.workspaceContext,
}: Props) {
  const [attempt, setAttempt] = useState(0);
  const [view, setView] = useState<WorkspaceContextView | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(true);
  const pending = useRef(true);
  const completed = useRef(false);
  const heading = useRef<HTMLHeadingElement>(null);

  useEffect(() => heading.current?.focus(), []);

  useEffect(() => {
    const controller = new AbortController();
    pending.current = true;
    void load(agentId, { signal: controller.signal }).then(
      (next) => {
        if (controller.signal.aborted) return;
        setView(next);
        setBusy(false);
        pending.current = false;
      },
      (caught: unknown) => {
        if (controller.signal.aborted) return;
        setError(
          caught instanceof Error
            ? caught.message
            : 'Could not check the connection.',
        );
        setBusy(false);
        pending.current = false;
      },
    );
    return () => controller.abort();
  }, [agentId, attempt, load]);

  function check() {
    if (pending.current || completed.current) return;
    pending.current = true;
    setView(null);
    setError('');
    setBusy(true);
    setAttempt((value) => value + 1);
  }

  function finish() {
    if (completed.current) return;
    completed.current = true;
    onDone();
  }

  const binding = view?.binding;
  const provider = view?.providerBinding;
  const additive =
    view?.status === 'configured' &&
    !binding &&
    provider?.version === 1 &&
    provider.provider === 'mother' &&
    provider.subject === agentId &&
    !!provider.workspaceId &&
    !!provider.generation &&
    provider.publication === 'selected' &&
    (provider.accessMode === 'companion' ||
      (provider.accessMode === 'shared' && provider.personaSource === 'local'));
  const companion =
    view?.status === 'configured' &&
    !provider &&
    binding?.mode === 'companion' &&
    binding.subject === agentId &&
    binding.tool === 'mother_get_context' &&
    !!binding.workspaceId &&
    !!binding.generation;
  const connection = additive
    ? view?.connections.find(
        (row) =>
          row.id === provider.connectionId && row.tool === 'mother_get_context',
      )
    : companion
      ? view?.connections.find(
          (row) => row.id === binding.connectionId && row.tool === binding.tool,
        )
      : undefined;
  const ready =
    !busy && !error && (companion || additive) && connection?.state === 'ready';
  const independent = view?.status === 'independent' && !binding && !provider;

  return (
    <section
      className="onboarding__section"
      aria-labelledby="onboarding-step-heading"
      aria-busy={busy}
    >
      <h2
        id="onboarding-step-heading"
        className="onboarding__heading"
        ref={heading}
        tabIndex={-1}
      >
        Connect M.O.T.H.E.R
      </h2>
      <p className="onboarding__hint">
        {name} has been created. Pair only this companion with your independent
        M.O.T.H.E.R workspace.
      </p>
      <ol className="onboarding__hint space-y-3">
        <li>
          Open a terminal on this computer and run{' '}
          <code>mother connect animaos</code>.
        </li>
        <li>
          Sign in with your AnimaOS owner token in the masked setup field. No
          model-provider API key is needed for this local connection.
        </li>
        <li>
          Select {name} and choose <strong>Companion</strong> for its
          personality, primary memory and context. Review and confirm in
          M.O.T.H.E.R.
        </li>
        <li>
          Current pairing creates a legacy connection. After pairing, choose
          Review additive connection in Profile → M.O.T.H.E.R to keep Anima’s
          native memory active alongside the shared garden. Conversion keeps the
          existing personality source; you can change it separately later.
        </li>
      </ol>
      <p className="onboarding__mono">Agent ID: {agentId}</p>
      <p className="onboarding__hint">
        Other agents keep their own settings. AnimaOS checks access again before
        each reply.
      </p>
      {busy ? (
        <p role="status">Checking saved connection…</p>
      ) : error ? (
        <p role="alert" className="onboarding__alert">
          {error}
        </p>
      ) : ready ? (
        <div role="status">
          <strong>Saved connection ready</strong>
          <p className="onboarding__hint">
            {additive
              ? 'Native memory, capture and learning remain active. M.O.T.H.E.R adds shared context; personality is a separate choice in Profile.'
              : 'This is a legacy primary-memory connection. M.O.T.H.E.R is configured as this companion’s primary memory and context provider until you confirm conversion.'}
          </p>
        </div>
      ) : additive ? (
        <div role="status">
          <strong>
            {connection?.state === 'disconnected' ||
            connection?.state === 'degraded'
              ? 'M.O.T.H.E.R is offline'
              : 'Connection needs attention'}
          </strong>
          <p className="onboarding__hint">
            Native memory, capture and learning remain active. Shared context is
            unavailable until the connection is ready.
          </p>
        </div>
      ) : companion ? (
        <div role="status">
          <strong>
            {connection?.state === 'disconnected' ||
            connection?.state === 'degraded'
              ? 'M.O.T.H.E.R is offline'
              : 'Connection needs attention'}
          </strong>
          <p className="onboarding__hint">
            The same chat can use your saved local personality. M.O.T.H.E.R
            memory stays offline until the connection returns.
          </p>
        </div>
      ) : independent ? (
        <div role="status">
          <strong>Not paired yet</strong>
          <p className="onboarding__hint">
            Until paired, this companion uses its local personality and memory.
            You can finish later from Profile → M.O.T.H.E.R.
          </p>
        </div>
      ) : view ? (
        <p role="status" className="onboarding__hint">
          This saved setup is not a M.O.T.H.E.R companion connection for {name}.
          Review its role and agent in M.O.T.H.E.R, then check again.
        </p>
      ) : null}
      <div className="onboarding__footer">
        <button
          type="button"
          className="onboarding__link"
          disabled={busy}
          onClick={check}
        >
          Check connection
        </button>
        <button
          type="button"
          className="onboarding__button onboarding__button--primary"
          disabled={!ready}
          onClick={finish}
        >
          Continue
        </button>
      </div>
      {!ready && (
        <button type="button" className="onboarding__link" onClick={finish}>
          {companion || additive
            ? 'Finish setup with offline fallback'
            : 'Finish pairing later'}
        </button>
      )}
      {!view && !independent && !companion && (
        <p className="onboarding__hint">
          The connection has not been verified. Finishing later does not
          activate M.O.T.H.E.R.
        </p>
      )}
    </section>
  );
}
