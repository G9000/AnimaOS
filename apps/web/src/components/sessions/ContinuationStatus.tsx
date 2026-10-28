import { useEffect, useState } from 'react';
import type { RunStatus } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';

const labels: Record<RunStatus, string> = {
  queued: 'Queued',
  running: 'Running',
  awaiting_approval: 'Waiting for approval',
  completed: 'Finished',
  failed: 'Needs attention',
  cancelled: 'Stopped',
  interrupted: 'Needs attention',
};

export function ContinuationStatus({
  agentId,
  runId,
}: {
  agentId: string;
  runId: string;
}) {
  const [refresh, setRefresh] = useState(0);
  const requestKey = JSON.stringify([agentId, runId, refresh]);
  const [saved, setSaved] = useState<{
    key: string;
    status: RunStatus | null;
    error: string;
    pollingPaused: boolean;
  } | null>(null);
  const current = saved?.key === requestKey ? saved : null;

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let count = 0;
    const controller = new AbortController();
    const load = async () => {
      try {
        const run = await daemon.runs.get(agentId, runId, {
          signal: controller.signal,
        });
        if (cancelled) return;
        count += 1;
        const active =
          run.status === 'queued' ||
          run.status === 'running' ||
          run.status === 'awaiting_approval';
        setSaved({
          key: requestKey,
          status: run.status,
          error: '',
          pollingPaused: active && count >= 60,
        });
        if (active) {
          if (count < 60) timer = setTimeout(() => void load(), 5000);
        }
      } catch (caught) {
        if (!cancelled)
          setSaved({
            key: requestKey,
            status: null,
            error: caught instanceof Error ? caught.message : String(caught),
            pollingPaused: false,
          });
      }
    };
    void load();
    return () => {
      cancelled = true;
      controller.abort();
      if (timer) clearTimeout(timer);
    };
  }, [agentId, runId, requestKey]);

  return (
    <section aria-label="Continuation status">
      <p role="status">
        Continuation saved. Follow the new attempt in this chat.
      </p>
      {current?.status && <p>{labels[current.status]}</p>}
      {!current && <p>Checking continuation…</p>}
      {current?.error && (
        <p role="alert">Could not check the continuation: {current.error}</p>
      )}
      {current?.pollingPaused && (
        <p>Automatic status checks paused. Refresh to check again.</p>
      )}
      <button
        className="studio-tool-button"
        onClick={() => setRefresh((value) => value + 1)}
      >
        Refresh continuation
      </button>
    </section>
  );
}
