import { useEffect, useRef, useState } from 'react';
import type { WorkflowsClient } from '@animaos/sdk';
import { RecoveryEvidence } from '../sessions/RecoveryEvidence';

type Inspect = WorkflowsClient['inspectAttempt'];
type Attempt = Awaited<ReturnType<Inspect>>;

export function WorkflowAttemptInspector({
  inspect,
  agentId,
  workflowId,
  workflowRevision,
  stepId,
  stepTitle,
  attempts,
  disabled,
  onFreshness,
}: {
  inspect: Inspect;
  agentId: string;
  workflowId: string;
  workflowRevision: number;
  stepId: string;
  stepTitle: string;
  attempts: string[];
  disabled: boolean;
  onFreshness: (stale: boolean) => void;
}) {
  const [result, setResult] = useState<Attempt | null>(null);
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const active = useRef<AbortController | null>(null);
  useEffect(
    () => () => {
      active.current?.abort();
    },
    [],
  );

  const load = async (jobId: string) => {
    active.current?.abort();
    const controller = new AbortController();
    active.current = controller;
    onFreshness(true);
    setLoading(true);
    setError('');
    setResult(null);
    try {
      const view = await inspect(agentId, workflowId, stepId, jobId, {
        signal: controller.signal,
      });
      if (controller.signal.aborted || active.current !== controller) return;
      setResult(view);
      onFreshness(view.workflowRevision !== workflowRevision);
    } catch (caught) {
      if (!controller.signal.aborted && active.current === controller)
        setError(caught instanceof Error ? caught.message : String(caught));
    } finally {
      if (!controller.signal.aborted && active.current === controller)
        setLoading(false);
    }
  };

  return (
    <section
      aria-label={`Saved attempts for ${stepTitle}`}
      className="space-y-2"
    >
      {attempts.map((jobId, index) => (
        <button
          key={jobId}
          type="button"
          className="studio-tool-button"
          disabled={disabled || loading}
          onClick={() => void load(jobId)}
        >
          Inspect saved attempt {index + 1}
        </button>
      ))}
      {loading && <p role="status">Loading saved attempt…</p>}
      {error && <p role="alert">Could not load saved evidence: {error}</p>}
      {result && result.workflowRevision !== workflowRevision && (
        <p role="alert">The workflow changed. Refresh before deciding.</p>
      )}
      {result && result.workflowRevision === workflowRevision && (
        <div>
          <p>
            Attempt {result.stepAttempt}: {result.status.replaceAll('_', ' ')}
          </p>
          {result.error && <p>{result.error}</p>}
          {result.result !== null && (
            <p>
              {result.result}
              {result.resultTruncated && ' (shortened)'}
            </p>
          )}
          {result.evidenceState === 'unavailable' && (
            <p>Detailed run evidence is unavailable.</p>
          )}
          {result.evidenceState === 'not_started' && (
            <p>This attempt has not started.</p>
          )}
          {result.run && <RecoveryEvidence evidence={result.run.evidence} />}
        </div>
      )}
    </section>
  );
}
