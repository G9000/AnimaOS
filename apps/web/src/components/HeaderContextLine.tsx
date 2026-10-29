import { useResetOnChange } from '../hooks/useResetOnChange';
import { useEffect, useRef, useState } from 'react';
import { useHeaderContextState } from '../lib/header-context';
import { MarkdownMessage } from './MarkdownMessage';

/** The top bar's summary opens in a panel under the bar. */
export function HeaderContextLine() {
  const state = useHeaderContextState();
  const summary = state?.summaryInline ? null : (state?.summary ?? null);
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);

  useResetOnChange([summary], () => {
    if (!summary) setOpen(false);
  });

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

  if (!summary) return null;
  const count = summary?.sourceMessageCount ?? 0;
  return (
    <div className="header-context" ref={root}>
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
