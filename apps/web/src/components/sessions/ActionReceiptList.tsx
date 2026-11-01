import type { ActionReceipt } from '@animaos/sdk';

import { toolLabel } from '../../lib/tool-labels';

const STATUS_LABELS = {
  succeeded: 'Reported success',
  failed: 'Reported failure',
  started: 'Outcome unknown',
} as const;

/** Recorded actions as one line each: what ran and how it reported back.
 *  The raw arguments and result stay one click away, wrapped to the card. */
export function ActionReceiptList({
  receipts,
}: {
  receipts: readonly ActionReceipt[];
}) {
  return (
    <ul className="outcome-receipts">
      {receipts.map((receipt) => {
        const status = receipt.status;
        const hasDetails =
          Boolean(receipt.argumentsPreview) || receipt.resultPreview !== null;
        return (
          <li key={receipt.toolCallId} data-status={status}>
            <div className="outcome-receipt-head">
              <span className="outcome-receipt-mark" aria-hidden>
                {status === 'succeeded' ? '✓' : status === 'failed' ? '!' : '?'}
              </span>
              <span className="outcome-receipt-name" title={receipt.name}>
                {toolLabel(receipt.name)}
              </span>
              <span className="outcome-receipt-status">
                {STATUS_LABELS[status]}
              </span>
              {receipt.durability === 'memory' && (
                <span className="outcome-receipt-temp">
                  Temporary (memory only)
                </span>
              )}
            </div>
            {hasDetails && (
              <details className="outcome-receipt-details">
                <summary>Details</summary>
                <p className="outcome-receipt-tool">
                  Tool <code>{receipt.name}</code>
                </p>
                {receipt.argumentsPreview && (
                  <>
                    <p className="outcome-receipt-key">
                      Sent{receipt.argumentsTruncated && ' (shortened)'}
                    </p>
                    <pre>{receipt.argumentsPreview}</pre>
                  </>
                )}
                {receipt.resultPreview !== null && (
                  <>
                    <p className="outcome-receipt-key">
                      Got back{receipt.resultTruncated && ' (shortened)'}
                    </p>
                    <pre>{receipt.resultPreview}</pre>
                  </>
                )}
              </details>
            )}
          </li>
        );
      })}
    </ul>
  );
}
