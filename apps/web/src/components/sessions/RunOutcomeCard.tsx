import type { Run } from '@animaOS-SWARM/sdk';

import { isOwnerWritten } from '../../lib/transcript';

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
    <section aria-label="Recorded actions">
      <p>Recorded actions</p>
      <ul>
        {run.actionReceipts.map((receipt) => {
          const statusLabel =
            receipt.status === 'succeeded'
              ? 'Reported success'
              : receipt.status === 'failed'
                ? 'Reported failure'
                : 'Outcome unknown';
          return (
            <li key={receipt.toolCallId}>
              <p>
                <span>{receipt.name}</span> · <span>{statusLabel}</span>
              </p>
              {receipt.durability === 'memory' && (
                <p>Temporary (memory only)</p>
              )}
              {receipt.argumentsPreview && (
                <p>
                  Arguments: <span>{receipt.argumentsPreview}</span>
                  {receipt.argumentsTruncated && ' (shortened)'}
                </p>
              )}
              {receipt.resultPreview !== null && (
                <p>
                  Result: <span>{receipt.resultPreview}</span>
                  {receipt.resultTruncated && ' (shortened)'}
                </p>
              )}
            </li>
          );
        })}
      </ul>
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
  // Only a message the owner wrote is theirs to send again.
  const sendAgain = isOwnerWritten(run) ? onSendAgain : undefined;
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
        {sendAgain && (
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
      {sendAgain && (
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
