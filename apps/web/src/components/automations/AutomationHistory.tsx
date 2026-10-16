import { useEffect, useState } from 'react';
import {
  DaemonHttpError,
  type Automation,
  type AutomationRun,
} from '@animaOS-SWARM/sdk';

import { COMPANION_UNREACHABLE, formatWhen } from '../../lib/approvals';
import { RUN_OUTCOME_LABELS } from '../../lib/automations';
import { daemon } from '../../lib/daemon-api';
import { revealInvisible } from '../../lib/skills';

/** An automation's latest runs, newest first (spec §9.1, §15.4). */
export function AutomationHistory({
  agentId,
  automation,
  version,
  online,
  onClose,
  onOpenSession,
}: {
  agentId: string;
  automation: Automation;
  /** `LiveState.automationsVersion`: a new run reads the list again. */
  version: number;
  online: boolean;
  onClose: () => void;
  onOpenSession: (sessionId: string) => void;
}) {
  const contentKey = JSON.stringify([agentId, automation.id, version, online]);

  return (
    <aside
      className="automation-history"
      aria-label={`History of ${revealInvisible(automation.name).text}`}
    >
      <div className="automation-history-header">
        <h3>History</h3>
        <button type="button" className="studio-tool-button" onClick={onClose}>
          Close history
        </button>
      </div>
      <AutomationHistoryContent
        key={contentKey}
        agentId={agentId}
        automation={automation}
        online={online}
        onOpenSession={onOpenSession}
      />
    </aside>
  );
}

function AutomationHistoryContent({
  agentId,
  automation,
  online,
  onOpenSession,
}: {
  agentId: string;
  automation: Automation;
  online: boolean;
  onOpenSession: (sessionId: string) => void;
}) {
  const [runs, setRuns] = useState<AutomationRun[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!online) return;
    const controller = new AbortController();
    daemon
      .automationHistory(agentId, automation.id, { signal: controller.signal })
      .then((next) => {
        if (controller.signal.aborted) return;
        setRuns(next);
        setError(null);
      })
      .catch((caught: unknown) => {
        if (controller.signal.aborted) return;
        setError(
          caught instanceof DaemonHttpError
            ? caught.message
            : COMPANION_UNREACHABLE,
        );
      });
    return () => controller.abort();
  }, [agentId, automation.id, online]);

  if (!online)
    return <p className="automations-empty">History is unavailable offline.</p>;

  return (
    <>
      {error && (
        <p className="automations-error" role="alert">
          {error}
        </p>
      )}
      {runs === null && !error && <p className="automations-empty">Loading…</p>}
      {runs !== null && runs.length === 0 && (
        <p className="automations-empty">It has not run yet.</p>
      )}
      {runs !== null && runs.length > 0 && (
        <ul className="automation-history-list">
          {runs.map((run) => (
            <li key={run.id} className="automation-history-run">
              <span>{formatWhen(run.firedAtMs)}</span>
              {run.outcome === 'failed' &&
              run.errorCode === 'schedule_run_interrupted' ? (
                <>
                  <span data-outcome="failed">Needs review</span>
                  <p>
                    Some actions may have already happened. Review the prior
                    thread and its effects before using Run now or Resume.
                  </p>
                </>
              ) : (
                <span data-outcome={run.outcome}>
                  {RUN_OUTCOME_LABELS[run.outcome]}
                </span>
              )}
              {run.manual && <span className="automation-badge">Run now</span>}
              {run.errorCode && <code>{run.errorCode}</code>}
              {run.outcome === 'failed' &&
              run.errorCode === 'schedule_run_interrupted' ? (
                run.sessionId ? (
                  <button
                    type="button"
                    className="studio-tool-button"
                    onClick={() => onOpenSession(run.sessionId as string)}
                  >
                    Review thread
                  </button>
                ) : (
                  <p>No saved thread is available.</p>
                )
              ) : (
                run.sessionId && (
                  <button
                    type="button"
                    className="studio-tool-button"
                    onClick={() => onOpenSession(run.sessionId as string)}
                  >
                    Open
                  </button>
                )
              )}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}
