import { MarkdownMessage } from '../MarkdownMessage';
import type { SessionSummary } from '@animaOS-SWARM/sdk';

/** The persisted compaction result, available even outside the loaded page. */
export function KeptSummary({ summary }: { summary: SessionSummary }) {
  return (
    <details className="session-kept-summary">
      <summary>What anima kept</summary>
      <div className="session-kept-summary-body">
        <p className="session-kept-summary-meta">
          From {summary.sourceMessageCount.toLocaleString()} messages ·{' '}
          <time dateTime={new Date(summary.createdAtMs).toISOString()}>
            {new Date(summary.createdAtMs).toLocaleString()}
          </time>
        </p>
        <MarkdownMessage>{summary.text}</MarkdownMessage>
      </div>
    </details>
  );
}

/** Where the companion's view of the session begins (spec §5.3). Manual
 *  Compact disables and shows progress while its request is in flight
 *  (S3b-C). */
export function TrimmedDivider({
  onCompact,
  compacting = false,
}: {
  onCompact?: () => void;
  compacting?: boolean;
}) {
  return (
    <div className="context-trimmed">
      {/* The separator role belongs on a decorative element, never one
       *  wrapping the interactive Compact button (ARIA forbids focusable
       *  descendants of role="separator"). */}
      <span className="context-trimmed-rule" role="separator" />
      <span className="context-trimmed-text">
        Older messages are outside anima’s working memory · they stay here in
        the chat
      </span>
      {onCompact && (
        <button
          type="button"
          className="studio-tool-button"
          onClick={onCompact}
          disabled={compacting}
        >
          {compacting ? 'Compacting…' : 'Compact'}
        </button>
      )}
    </div>
  );
}

/** A helper session's user turn, credited to the agent that wrote it
 *  (the delegating companion, or the peer that sent it). */
export function DelegatedTurn({ from, text }: { from: string; text: string }) {
  return (
    <div className="delegated-turn">
      <p className="delegated-turn-from">From {from}</p>
      <MarkdownMessage>{text}</MarkdownMessage>
    </div>
  );
}
