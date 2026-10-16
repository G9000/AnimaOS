import { useId, useState } from 'react';

import { formatElapsed, type ToolStep } from '../../lib/transcript';
import { toolWebSources } from '../../lib/web-sources';

const STATUS_LABELS: Record<ToolStep['status'], string> = {
  running: 'running',
  success: 'done',
  error: 'failed',
};

/** One tool call (spec §15.2): name, short arguments, a spinner then ✓ or ✗,
 *  its duration, and an expandable result. */
export function ToolStepCard({ step }: { step: ToolStep }) {
  const [open, setOpen] = useState(false);
  const resultId = useId();
  const sources = toolWebSources(step.name, step.status, step.sources);
  return (
    <div className="tool-step" data-status={step.status}>
      <button
        type="button"
        className="tool-step-toggle"
        aria-expanded={open}
        aria-controls={open ? resultId : undefined}
        onClick={() => setOpen((value) => !value)}
      >
        <span className="tool-step-icon" aria-hidden>
          {step.status === 'running' ? (
            <span className="tool-step-spinner" />
          ) : step.status === 'success' ? (
            '✓'
          ) : (
            '✗'
          )}
        </span>
        <span className="tool-step-name">{step.name}</span>
        {step.argumentsPreview && (
          <span className="tool-step-args">{step.argumentsPreview}</span>
        )}
        <span className="sr-only">, {STATUS_LABELS[step.status]}</span>
        {step.durationMs !== null && (
          <span className="tool-step-duration">
            {formatElapsed(step.durationMs)}
          </span>
        )}
      </button>
      {/* Mounted only while open (a long result is never kept in the DOM),
       *  so aria-controls above names the region only when it exists
       *  (S3b-E). */}
      {open && (
        <div id={resultId} className="tool-step-result">
          {step.result === null ? (
            <p>
              {step.status === 'running'
                ? 'Still running…'
                : 'No result was recorded.'}
            </p>
          ) : (
            <pre>{step.result}</pre>
          )}
          {step.truncated && (
            <p className="tool-step-note">Result shortened to 2 KiB.</p>
          )}
          {sources.length > 0 && (
            <div className="mt-3 space-y-2 text-sm">
              <p>Search sources</p>
              <ul aria-label="Search sources" className="space-y-1">
                {sources.map((source) => (
                  <li key={source.url}>
                    <a
                      href={source.url}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="break-words text-accent underline underline-offset-2"
                    >
                      {source.title}
                    </a>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
