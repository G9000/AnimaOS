import { useEffect, useRef, useState } from 'react';
import type { RunRecoverySummary } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';

export function RunRecovery(props: {
  agentId: string;
  runId: string;
  continuationRunId?: string;
}) {
  return <Recovery key={`${props.agentId}:${props.runId}`} {...props} />;
}

function Recovery({
  agentId,
  runId,
  continuationRunId,
}: {
  agentId: string;
  runId: string;
  continuationRunId?: string;
}) {
  const [view, setView] = useState<RunRecoverySummary | null>(null);
  const [continuation, setContinuation] = useState(continuationRunId);
  const [reviewed, setReviewed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const alive = useRef(true);
  const locked = useRef(false);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    if (continuationRunId) setContinuation(continuationRunId);
  }, [continuationRunId]);
  async function act(continueWork: boolean) {
    if (locked.current) return;
    locked.current = true;
    setBusy(true);
    setError('');
    try {
      if (continueWork && view) {
        const next = await daemon.runs.continueRecovery(agentId, runId, {
          fingerprint: view.fingerprint,
          reviewed,
        });
        if (alive.current) setContinuation(next.id);
      } else {
        const result = await daemon.runs.recovery(agentId, runId);
        if (!alive.current) return;
        setView(result);
        setReviewed(false);
        if (result.continuationRunId) setContinuation(result.continuationRunId);
      }
    } catch (failure) {
      if (alive.current)
        setError(failure instanceof Error ? failure.message : String(failure));
    } finally {
      locked.current = false;
      if (alive.current) setBusy(false);
    }
  }
  if (continuation)
    return (
      <p role="status">
        Continuation saved. Follow the new attempt in this chat.
      </p>
    );
  return (
    <section aria-label="Recover interrupted work">
      {!view ? (
        <button
          className="studio-tool-button"
          disabled={busy}
          onClick={() => void act(false)}
        >
          Review interrupted work
        </button>
      ) : (
        <>
          <p>
            The new attempt will inspect saved progress and use your current
            permissions. Completed actions must be preserved.
          </p>
          {view.unknownActionCount > 0 && (
            <p>
              {view.unknownActionCount} recorded action outcome(s) are unknown.
              Check their effects before continuing.
            </p>
          )}
          {view.reviewRequired && (
            <label>
              <input
                type="checkbox"
                checked={reviewed}
                onChange={(event) => setReviewed(event.target.checked)}
              />{' '}
              I have reviewed the recorded actions and any unknown effects.
            </label>
          )}
          {view.eligible ? (
            <button
              className="studio-tool-button"
              disabled={busy || (view.reviewRequired && !reviewed)}
              onClick={() => void act(true)}
            >
              Continue remaining work
            </button>
          ) : (
            <p>
              This run cannot be continued. Recovery attempts may be exhausted
              or its session is no longer eligible.
            </p>
          )}
          <button
            className="studio-tool-button"
            disabled={busy}
            onClick={() => void act(false)}
          >
            Refresh recovery
          </button>
        </>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
