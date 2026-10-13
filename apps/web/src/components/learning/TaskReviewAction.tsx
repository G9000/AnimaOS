import { useState } from 'react';
import type { LearningReview, Run } from '@animaOS-SWARM/sdk';
import { daemon } from '../../lib/daemon-api';

export function TaskReviewAction({
  run,
  agentId,
  requestReview,
}: {
  run: Run;
  agentId: string;
  requestReview?: (run: Run) => Promise<LearningReview>;
}) {
  const [busy, setBusy] = useState(false);
  const [requested, setRequested] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const request = async () => {
    if (busy || requested) return;
    setBusy(true);
    setError(null);
    try {
      await (
        requestReview ??
        ((source) => daemon.requestLearningReview(agentId, source.id))
      )(run);
      setRequested(true);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : 'Task review failed.');
    } finally {
      setBusy(false);
    }
  };

  return (
    <section aria-label="Task learning review" className="ml-auto max-w-2xl">
      <div className="rounded-xl border border-line bg-white/[0.02] px-3.5 py-3">
        <p className="text-sm text-ink-2">
          Review this completed task and prepare suggestions for you to inspect.
        </p>
        <button
          type="button"
          className="studio-tool-button mt-2"
          onClick={() => void request()}
          disabled={busy || requested}
        >
          {busy
            ? 'Requesting review…'
            : requested
              ? 'Review requested'
              : 'Review this task'}
        </button>
        {error ? (
          <p role="alert" className="mt-2 text-sm text-danger">
            {error}
          </p>
        ) : null}
      </div>
    </section>
  );
}
