import type { RunRecoverySummary } from '@animaos/sdk';

type Evidence = NonNullable<RunRecoverySummary['evidence']>;

/** Saved previews are evidence, not proof of the wider task or its effects. */
export function RecoveryEvidence({ evidence }: { evidence: Evidence }) {
  return (
    <section aria-label="Saved evidence">
      <p>Saved evidence may be incomplete. Check effects before continuing.</p>
      {evidence.actionReceipts.length > 0 && (
        <ul>
          {evidence.actionReceipts.map((receipt) => (
            <li key={receipt.toolCallId}>
              <p>
                {receipt.name} ·{' '}
                {receipt.status === 'succeeded'
                  ? 'Reported success'
                  : receipt.status === 'failed'
                    ? 'Reported failure'
                    : 'Outcome unknown'}
              </p>
              {receipt.durability === 'memory' && <p>Temporary record</p>}
              {receipt.argumentsPreview && (
                <p>
                  {receipt.argumentsPreview}
                  {receipt.argumentsTruncated && ' (shortened)'}
                </p>
              )}
              {receipt.resultPreview !== null && (
                <p>
                  {receipt.resultPreview}
                  {receipt.resultTruncated && ' (shortened)'}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}
      {evidence.unreceiptedTools.length > 0 && (
        <p>
          Earlier requests without recorded outcomes:{' '}
          {evidence.unreceiptedTools.join(', ')}
          {evidence.omittedUnreceiptedToolCount > 0 &&
            ` and ${evidence.omittedUnreceiptedToolCount} more`}
        </p>
      )}
      {evidence.checkpoint && (
        <div>
          {evidence.checkpoint.excerpts.length > 0 && <p>Saved excerpts:</p>}
          {evidence.checkpoint.excerpts.map((excerpt, index) => (
            <p
              key={`${excerpt.role}:${index}`}
              className="whitespace-pre-wrap break-words"
            >
              {excerpt.text}
            </p>
          ))}
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
