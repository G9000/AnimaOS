import { useResetOnChange } from '../../hooks/useResetOnChange';
import { useEffect, useRef, useState } from 'react';
import type { RunRecoverySummary } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { RecoveryEvidence } from './RecoveryEvidence';
import { ContinuationStatus } from './ContinuationStatus';

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
  useResetOnChange([continuationRunId], () => {
    if (continuationRunId) setContinuation(continuationRunId);
  });
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
        setReviewed(false);
        const result = await daemon.runs.recovery(agentId, runId);
        if (!alive.current) return;
        setView(result);
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
      <ContinuationStatus
        key={`${agentId}:${continuation}`}
        agentId={agentId}
        runId={continuation}
      />
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
            permissions and the remaining original limits. It starts a new
            attempt, rather than resuming the exact interrupted operation.
            Actions may be repeated; check their effects before continuing.
          </p>
          <p>{view.remainingAttempts} recovery attempts remaining.</p>
          {view.evidence && <RecoveryEvidence evidence={view.evidence} />}
          {view.unknownActionCount > 0 && (
            <p>
              {view.unknownActionCount} recorded action outcome(s) are unknown.
              Check their effects before continuing.
            </p>
          )}
          {view.projectContextNotice && <p>{view.projectContextNotice}</p>}
          {view.reviewRequired && (
            <label>
              <input
                type="checkbox"
                checked={reviewed}
                onChange={(event) => setReviewed(event.target.checked)}
              />{' '}
              I have reviewed the recorded actions, any unknown effects, and any
              changes to the shared project context.
            </label>
          )}
          {view.eligible &&
          view.recoveryState !== 'invalid_or_stale' &&
          view.recoveryState !== 'stopped' &&
          view.recoveryState !== 'already_completed' ? (
            <button
              className="studio-tool-button"
              disabled={busy || (view.reviewRequired && !reviewed)}
              onClick={() => void act(true)}
            >
              Continue remaining work
            </button>
          ) : (
            <p>
              This run cannot be continued. Its saved state or permissions may
              have changed, or its recovery attempts may be exhausted.
            </p>
          )}
          <button
            className="studio-tool-button"
            disabled={busy}
            onClick={() => void act(false)}
          >
            Refresh recovery
          </button>
          <button
            className="studio-tool-button"
            disabled={busy}
            onClick={() => {
              setView(null);
              setReviewed(false);
              setError('');
            }}
          >
            Leave interrupted
          </button>
        </>
      )}
      {error && <p role="alert">{error}</p>}
    </section>
  );
}
