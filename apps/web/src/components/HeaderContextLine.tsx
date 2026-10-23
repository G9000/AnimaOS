import { useEffect, useRef, useState } from 'react';
import { useHeaderContextState } from '../lib/header-context';
import { MarkdownMessage } from './MarkdownMessage';

/** The top bar's middle: the day at the top of the screen and what anima kept
 *  from earlier. Its summary opens in a panel under the bar. */
export function HeaderContextLine() {
  const state = useHeaderContextState();
  const summary = state?.summary ?? null;
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!summary) setOpen(false);
  }, [summary]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') setOpen(false);
    };
    document.addEventListener('pointerdown', outside);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', outside);
      document.removeEventListener('keydown', escape);
    };
  }, [open]);

  const day = state?.day ?? null;
  if (!day && !summary) return null;
  const count = summary?.sourceMessageCount ?? 0;
  return (
    <div className="header-context" ref={root}>
      {day && <span className="header-context-day">{day}</span>}
      {day && summary && <i className="header-context-rule" aria-hidden />}
      {summary && (
        <>
          <span className="header-context-note">
            {count.toLocaleString()} earlier{' '}
            {count === 1 ? 'message' : 'messages'} summarized
          </span>
          <button
            type="button"
            className="header-context-link"
            aria-expanded={open}
            aria-haspopup="dialog"
            onClick={() => setOpen((value) => !value)}
          >
            {open ? 'Hide summary' : 'Show summary'}
          </button>
        </>
      )}
      {summary && open && (
        <div
          className="header-summary-panel"
          role="dialog"
          aria-label="What anima kept"
        >
          <p className="header-summary-meta">
            What anima kept · from {count.toLocaleString()} messages ·{' '}
            <time dateTime={new Date(summary.createdAtMs).toISOString()}>
              {new Date(summary.createdAtMs).toLocaleString()}
            </time>
          </p>
          <div className="header-summary-body">
            <MarkdownMessage>{summary.text}</MarkdownMessage>
          </div>
        </div>
      )}
    </div>
  );
}
