import { useState } from 'react';
import { MarkdownMessage } from '../MarkdownMessage';
import type { SessionSummary } from '@animaos/sdk';
import { CheckIcon, ChevronIcon, SparkIcon } from '../icons';

/** The persisted compaction result, available even outside the loaded page. */
export function KeptSummary({
  summary,
  onShowOriginals,
  initiallyOpen = false,
}: {
  summary: SessionSummary;
  onShowOriginals?: () => void;
  initiallyOpen?: boolean;
}) {
  const [open, setOpen] = useState(initiallyOpen);
  return (
    <section className="session-kept-summary" aria-label="Conversation summary">
      <header className="session-kept-summary-head">
        <SparkIcon size={14} aria-hidden />
        <span>
          <span>What anima kept</span>
          {' · '}
          {summary.sourceMessageCount.toLocaleString()}{' '}
          earlier {summary.sourceMessageCount === 1 ? 'message' : 'messages'}
          {' summarized'}
        </span>
        <button
          type="button"
          className="session-kept-summary-toggle"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          {open ? 'Hide' : 'Show'}
          <ChevronIcon size={12} />
        </button>
      </header>
      {open && (
        <>
          <div className="session-kept-summary-body">
            <MarkdownMessage>{summary.text}</MarkdownMessage>
          </div>
          {onShowOriginals && (
            <footer className="session-kept-summary-foot">
              <button type="button" onClick={onShowOriginals}>
                Show the {summary.sourceMessageCount.toLocaleString()} original{' '}
                {summary.sourceMessageCount === 1 ? 'message' : 'messages'}
              </button>
              <span>
                <CheckIcon size={11} aria-hidden /> Originals stay searchable
              </span>
            </footer>
          )}
        </>
      )}
    </section>
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
