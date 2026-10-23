import { useState } from 'react';
import type {
  Run,
  Session,
  SessionQueuedRunPatch,
  SessionReviseInput,
} from '@animaos/sdk';
import type { ChatMessage } from '../../lib/types';
import { ghostBtnCls } from '../ui-bits';
import { isOwnerWritten } from '../../lib/transcript';

export interface ConversationControlProps {
  session: Session;
  messages: readonly ChatMessage[];
  runs: readonly Run[];
  enabled: boolean;
  revising?: boolean;
  onRevise?: (
    mode: SessionReviseInput['mode'],
    throughMessageId?: string,
  ) => void;
  onOpenSource?: (sessionId: string) => void;
  onQueuePatch?: (run: Run, patch: SessionQueuedRunPatch) => Promise<boolean>;
  onCancel?: (run: Run) => void;
}

/** Native mutations stay scoped to this ordinary web conversation. */
export function ConversationControls({
  session,
  messages,
  runs,
  enabled,
  revising,
  onRevise,
  onOpenSource,
  onQueuePatch,
  onCancel,
}: ConversationControlProps) {
  const [boundary, setBoundary] = useState('');
  const [editing, setEditing] = useState<string | null>(null);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState<string | null>(null);
  const waiting = runs
    .filter(
      (run) =>
        run.status === 'queued' &&
        run.startedAtMs === null &&
        !run.stop &&
        run.agentId === session.agentId &&
        run.sessionId === session.id &&
        ['web', 'api'].includes(run.source) &&
        isOwnerWritten(run) &&
        !run.parentRunId &&
        !run.helperCompletion,
    )
    .sort(
      (left, right) =>
        (left.queueOrder ?? left.createdAtMs) -
        (right.queueOrder ?? right.createdAtMs),
    );
  const blocked =
    !enabled ||
    revising ||
    session.activeRuns > 0 ||
    runs.some((run) =>
      ['queued', 'running', 'awaiting_approval'].includes(run.status),
    );
  if (
    session.kind !== 'chat' ||
    !['web', 'api'].includes(session.origin) ||
    session.parentRunId ||
    !session.capabilities.send
  )
    return null;
  const patch = async (run: Run, input: SessionQueuedRunPatch) => {
    if (!onQueuePatch || busy) return;
    setBusy(run.id);
    try {
      if (await onQueuePatch(run, input)) setEditing(null);
    } finally {
      setBusy(null);
    }
  };
  return (
    <div
      className="space-y-2 px-4 py-2 text-xs text-ink-3"
      aria-label="Chat controls"
    >
      {session.parentSessionId && onOpenSource && (
        <button
          type="button"
          className={ghostBtnCls}
          onClick={() => onOpenSource(session.parentSessionId!)}
        >
          Open original chat
        </button>
      )}
      {onRevise && (
        <details>
          <summary className="cursor-pointer">
            {revising ? 'Opening revision…' : 'Conversation controls'}
          </summary>
          <p className="my-2">
            Revisions preserve the original chat and cannot reverse completed
            actions. Retry restores a draft for you to review and send.
          </p>
          <div className="flex flex-wrap items-center gap-2">
            <label>
              Branch through{' '}
              <select
                aria-label="Branch through message"
                value={boundary}
                onChange={(event) => setBoundary(event.target.value)}
                className="rounded border border-line bg-surface p-1"
              >
                <option value="">Latest message (full saved history)</option>
                {messages
                  .filter(
                    (message) =>
                      message.role === 'User' || message.role === 'Assistant',
                  )
                  .map((message) => (
                    <option key={message.id} value={message.id}>
                      {message.role}: {message.content.text.slice(0, 70)}
                    </option>
                  ))}
              </select>
            </label>
            <button
              type="button"
              className={ghostBtnCls}
              disabled={blocked}
              onClick={() =>
                onRevise('branch', ...(boundary ? [boundary] : []))
              }
            >
              Branch chat
            </button>
            <button
              type="button"
              className={ghostBtnCls}
              disabled={blocked}
              onClick={() => onRevise('undo')}
            >
              Undo latest exchange
            </button>
            <button
              type="button"
              className={ghostBtnCls}
              disabled={blocked}
              onClick={() => onRevise('retry')}
            >
              Retry latest prompt
            </button>
          </div>
          {blocked && (
            <p>
              Wait for active and queued work to finish before opening a
              revision.
            </p>
          )}
        </details>
      )}
      {waiting.length > 0 && onQueuePatch && (
        <details open>
          <summary>Waiting messages ({waiting.length})</summary>
          <ol className="max-h-48 space-y-2 overflow-y-auto pt-2">
            {waiting.map((run, index) => (
              <li key={run.id} className="rounded border border-line p-2">
                {editing === run.id ? (
                  <form
                    onSubmit={(event) => {
                      event.preventDefault();
                      void patch(run, { text });
                    }}
                  >
                    <textarea
                      aria-label={`Edit waiting message ${index + 1}`}
                      value={text}
                      maxLength={200000}
                      onChange={(event) => setText(event.target.value)}
                      className="w-full rounded border border-line bg-surface p-2"
                    />
                    <button
                      type="submit"
                      className={ghostBtnCls}
                      disabled={!enabled || Boolean(busy) || !text.trim()}
                    >
                      Save waiting message
                    </button>
                    <button
                      type="button"
                      className={ghostBtnCls}
                      onClick={() => setEditing(null)}
                    >
                      Discard edit
                    </button>
                  </form>
                ) : (
                  <>
                    <p className="whitespace-pre-wrap">
                      {index + 1} · {run.input.text || 'Attached files'}
                    </p>
                    <div className="flex flex-wrap gap-2">
                      <button
                        type="button"
                        className={ghostBtnCls}
                        disabled={!enabled || Boolean(busy)}
                        aria-label={`Edit queued message ${index + 1}`}
                        onClick={() => {
                          setEditing(run.id);
                          setText(run.input.text);
                        }}
                      >
                        Edit
                      </button>
                      <button
                        type="button"
                        className={ghostBtnCls}
                        disabled={!enabled || Boolean(busy) || index === 0}
                        aria-label={`Move queued message ${index + 1} up`}
                        onClick={() =>
                          void patch(run, {
                            beforeRunId: waiting[index - 1].id,
                          })
                        }
                      >
                        Move up
                      </button>
                      <button
                        type="button"
                        className={ghostBtnCls}
                        disabled={
                          !enabled ||
                          Boolean(busy) ||
                          index === waiting.length - 1
                        }
                        aria-label={`Move queued message ${index + 1} down`}
                        onClick={() =>
                          void patch(run, {
                            beforeRunId: waiting[index + 2]?.id ?? null,
                          })
                        }
                      >
                        Move down
                      </button>
                      {onCancel && (
                        <button
                          type="button"
                          className={ghostBtnCls}
                          disabled={!enabled || Boolean(busy)}
                          onClick={() => onCancel(run)}
                        >
                          Cancel waiting message
                        </button>
                      )}
                    </div>
                  </>
                )}
              </li>
            ))}
          </ol>
        </details>
      )}
    </div>
  );
}
