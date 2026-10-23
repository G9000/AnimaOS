import { useId, useState } from 'react';

import { parseCodingSession } from '../../lib/coding-sessions';
import { formatElapsed, type ToolStep } from '../../lib/transcript';
import { CodingSessionCard } from '../coding/CodingSessionCard';
import { toolWebSources } from '../../lib/web-sources';
import { savedToolResultId, ToolResultArtifact } from './ToolResultArtifact';
import { ChatExportDownload, chatExportResult } from './ChatExportDownload';

const STATUS_LABELS: Record<ToolStep['status'], string> = {
  running: 'running',
  success: 'done',
  error: 'failed',
};

/** One tool call (spec §15.2): name, short arguments, a spinner then ✓ or ✗,
 *  its duration, and an expandable result. */
export function ToolStepCard({
  step,
  agentId,
  pinned = false,
}: {
  step: ToolStep;
  /** The agent that ran it, for an app session's Stop. */
  agentId?: string;
  /** Its session's latest snapshot is already shown under the steps: the
   *  result opens as plain text. */
  pinned?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const resultId = useId();
  const sources = toolWebSources(step.name, step.status, step.sources);
  const artifactId = savedToolResultId(step.result);
  const exported =
    step.status === 'success' ? chatExportResult(step.name, step.result) : null;
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
      {exported && (
        <div className="px-3 py-2">
          <ChatExportDownload {...exported} />
        </div>
      )}
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
            <StepResult
              result={step.result}
              agentId={agentId}
              pinned={pinned}
            />
          )}
          {artifactId && agentId && (
            <ToolResultArtifact
              key={`${agentId}:${artifactId}`}
              agentId={agentId}
              artifactId={artifactId}
            />
          )}
          {step.truncated && !artifactId && (
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

/** An app or coding-agent snapshot as its card; anything else, or one cut
 *  short, as the plain result. */
function StepResult({
  result,
  agentId,
  pinned,
}: {
  result: string;
  agentId?: string;
  pinned: boolean;
}) {
  const session = pinned ? null : parseCodingSession(result);
  return session ? (
    <CodingSessionCard session={session} agentId={agentId} />
  ) : (
    <pre>{result}</pre>
  );
}
