import type { RunRecoverySummary } from '@animaos/sdk';

import { ActionReceiptList } from './ActionReceiptList';

type Evidence = NonNullable<RunRecoverySummary['evidence']>;

/** Saved previews are evidence, not proof of the wider task or its effects. */
export function RecoveryEvidence({
  evidence,
  shownReceiptIds = [],
}: {
  evidence: Evidence;
  /** Actions the card above already lists; they are not repeated here. */
  shownReceiptIds?: readonly string[];
}) {
  const receipts = evidence.actionReceipts.filter(
    (receipt) => !shownReceiptIds.includes(receipt.toolCallId),
  );
  return (
    <section className="outcome-section" aria-label="Saved evidence">
      <p className="outcome-section-title">
        Saved evidence may be incomplete. Check effects before continuing.
      </p>
      {receipts.length > 0 && <ActionReceiptList receipts={receipts} />}
      {evidence.unreceiptedTools.length > 0 && (
        <p>
          Earlier requests without recorded outcomes:{' '}
          {evidence.unreceiptedTools.join(', ')}
          {evidence.omittedUnreceiptedToolCount > 0 &&
            ` and ${evidence.omittedUnreceiptedToolCount} more`}
        </p>
      )}
      {evidence.checkpoint && (
        <div className="outcome-evidence-checkpoint">
          {evidence.checkpoint.excerpts.length > 0 && (
            <details className="outcome-receipt-details">
              <summary>
                Saved excerpts ({evidence.checkpoint.excerpts.length})
              </summary>
              {evidence.checkpoint.excerpts.map((excerpt, index) => (
                <pre key={`${excerpt.role}:${index}`}>{excerpt.text}</pre>
              ))}
            </details>
          )}
          {evidence.checkpoint.intendedTools.length > 0 && (
            <p>
              Requested actions, with no outcome implied:{' '}
              {evidence.checkpoint.intendedTools
                .map((tool) => tool.name)
                .join(', ')}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
