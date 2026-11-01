import type { Run } from '@animaos/sdk';

import { isOwnerWritten } from '../../lib/transcript';
import { ActionReceiptList } from './ActionReceiptList';
import { RunRecovery } from './RunRecovery';

/** Why an interrupted run did not reply: a restart in the web's words,
 *  otherwise the daemon's own reason, such as a stop or a full queue that
 *  left a steer unread. */
function interruptedReason(run: Run): string {
  switch (run.error?.code) {
    case 'restart_during_run':
      return 'The daemon restarted while this reply was running.';
    case 'restart_before_start':
      return 'The daemon restarted before this message was sent.';
    default:
      return run.error?.message.trim() || 'This message wasn’t sent.';
  }
}

function actionReceipts(run: Run) {
  if (!run.actionReceipts?.length) return null;

  return (
    <section className="outcome-section" aria-label="Recorded actions">
      <p className="outcome-section-title">Recorded actions</p>
      <ActionReceiptList receipts={run.actionReceipts} />
    </section>
  );
}

/** How a run ended when it did not simply reply (spec §15.2). */
export function RunOutcomeCard({
  run,
  onSendAgain,
  resent = false,
}: {
  run: Run;
  onSendAgain?: (run: Run) => void;
  /** Already sent again from this page: its button is used up. */
  resent?: boolean;
}) {
  // Outcome tasks use their own report controls; a new run cannot continue them.
  const sendAgain =
    !run.outcome && isOwnerWritten(run) ? onSendAgain : undefined;
  const recovery =
    !run.outcome &&
    onSendAgain &&
    (sendAgain || run.recoveryOf) &&
    run.recoveryAttempt !== undefined &&
    ['web', 'api'].includes(run.source) ? (
      <RunRecovery
        agentId={run.agentId}
        runId={run.id}
        continuationRunId={run.recovery?.continuationRunId}
        shownReceiptIds={run.actionReceipts?.map(
          (receipt) => receipt.toolCallId,
        )}
      />
    ) : null;
  if (run.status === 'cancelled')
    return (
      <div className="run-outcome" data-outcome="stopped">
        <p>Stopped</p>
        {actionReceipts(run)}
      </div>
    );
  if (run.status === 'failed')
    return (
      <div className="run-outcome" data-outcome="failed">
        <p>
          {run.error
            ? `This reply failed: ${run.error.message}`
            : 'This reply failed.'}
        </p>
        {actionReceipts(run)}
        {run.outcome && (
          <p>
            Use this run’s success criteria report to review its evidence and
            available actions.
          </p>
        )}
        {recovery}
        {sendAgain && !recovery && (
          <button
            type="button"
            className="studio-tool-button"
            disabled={resent}
            onClick={() => sendAgain(run)}
          >
            Retry
          </button>
        )}
      </div>
    );
  return (
    <div className="run-outcome" data-outcome="interrupted">
      <p>{interruptedReason(run)}</p>
      {run.toolsStarted.length > 0 && (
        <p className="run-outcome-warning">
          Tools were requested ({run.toolsStarted.join(', ')}). Check their
          effects before sending again.
        </p>
      )}
      {actionReceipts(run)}
      {run.outcome && (
        <p>
          Use this run’s success criteria report to review its evidence and
          available actions.
        </p>
      )}
      {recovery}
      {sendAgain && !recovery && (
        <button
          type="button"
          className="studio-tool-button"
          disabled={resent}
          onClick={() => sendAgain(run)}
        >
          Send again
        </button>
      )}
    </div>
  );
}
