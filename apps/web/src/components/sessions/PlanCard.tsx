import { useId, useState, type ReactNode } from 'react';
import {
  MAX_APPROVAL_NOTE_CHARS,
  type Approval,
  type ApprovalDecision,
} from '@animaOS-SWARM/sdk';

import { revealHiddenCharacters, type ApprovalDecide } from '../../lib/approvals';
import type { Plan } from '../../lib/plan';

type Sent = 'start' | 'change' | 'skip' | 'always';

const SENT: Record<Sent, string> = {
  start: 'Plan started. It will still ask before the steps marked “asks first”.',
  change: 'Sent your changes. Expect a revised plan.',
  skip: 'Not now. Nothing in the plan was started.',
  always: 'Plans start on their own from now on. Change this in Permissions.',
};

/** The agent's plan for a bigger job, waiting for the owner: its steps, the
 *  ones that will ask again, and Start, Change or Not now. Changing it is a
 *  denial with the owner's note, which the agent reads and revises from. */
export function PlanCard({
  approval,
  plan,
  onDecide,
  canPersist,
  context,
}: {
  approval: Approval;
  plan: Plan;
  onDecide?: ApprovalDecide;
  canPersist: boolean;
  context?: ReactNode;
}) {
  const [changing, setChanging] = useState(false);
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState<Sent | null>(null);
  const [error, setError] = useState<string | null>(null);
  const noteId = useId();

  const send = async (
    outcome: Sent,
    decision: ApprovalDecision,
    noteText?: string,
  ) => {
    if (!onDecide || busy) return;
    setBusy(true);
    setError(null);
    const failure = await onDecide(approval, {
      decision,
      ...(noteText ? { note: noteText } : {}),
      ...(decision === 'allow_always'
        ? { matcher: { kind: 'any', value: '' } }
        : {}),
    });
    setBusy(false);
    if (failure) setError(failure);
    else setSent(outcome);
  };

  // The model wrote the plan: text nodes only, hidden characters shown.
  const text = (value: string) => revealHiddenCharacters(value);

  return (
    <section className="plan-card" aria-label={`Plan: ${plan.title}`}>
      <header className="plan-card-head">
        <strong>Plan · {text(plan.title)}</strong>
        <span>
          {plan.steps.length} {plan.steps.length === 1 ? 'step' : 'steps'}
        </span>
        {context}
      </header>
      <ol className="plan-card-steps">
        {plan.steps.map((step, index) => (
          <li key={index}>
            <span className="plan-card-number">{index + 1}</span>
            <span className="plan-card-action">{text(step.action)}</span>
            {step.asksFirst && <span className="plan-card-asks">Asks first</span>}
          </li>
        ))}
      </ol>
      {plan.estimate && (
        <p className="plan-card-estimate">{text(plan.estimate)}</p>
      )}
      {sent ? (
        <p className="plan-card-sent" role="status">
          {SENT[sent]}
        </p>
      ) : changing ? (
        <div className="plan-card-change">
          <label htmlFor={noteId}>What should change?</label>
          <textarea
            id={noteId}
            rows={2}
            maxLength={MAX_APPROVAL_NOTE_CHARS}
            value={note}
            placeholder="Text Mimi on Telegram instead of email"
            onChange={(event) => setNote(event.target.value)}
          />
          <div className="plan-card-actions">
            <button
              type="button"
              className="plan-card-primary"
              disabled={!onDecide || busy || note.trim() === ''}
              onClick={() =>
                void send('change', 'deny', `Change the plan: ${note.trim()}`)
              }
            >
              Send changes
            </button>
            <button type="button" disabled={busy} onClick={() => setChanging(false)}>
              Back
            </button>
          </div>
        </div>
      ) : (
        <div className="plan-card-actions">
          <button
            type="button"
            className="plan-card-primary"
            disabled={!onDecide || busy}
            onClick={() => void send('start', 'allow_once')}
          >
            Start plan
          </button>
          <button
            type="button"
            disabled={!onDecide || busy}
            onClick={() => setChanging(true)}
          >
            Change it
          </button>
          <button
            type="button"
            disabled={!onDecide || busy}
            onClick={() => void send('skip', 'deny', 'Not now.')}
          >
            Not now
          </button>
          {canPersist && (
            <button
              type="button"
              className="plan-card-quiet"
              disabled={!onDecide || busy}
              onClick={() => void send('always', 'allow_always')}
            >
              Don’t ask for plans
            </button>
          )}
        </div>
      )}
      {error && (
        <p className="plan-card-error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}

/** A plan the agent showed and started on its own (Free autonomy): the
 *  same card without approval buttons, and Stop while the run works. */
export function PlanNoteCard({
  plan,
  working,
  onStop,
}: {
  plan: Plan;
  /** The run that made it is still working. */
  working: boolean;
  /** Stops that run; offered only while it works. */
  onStop?: () => void;
}) {
  const text = (value: string) => revealHiddenCharacters(value);
  return (
    <section
      className="plan-card plan-card-note"
      aria-label={`Plan: ${plan.title}`}
    >
      <header className="plan-card-head">
        <strong>Plan · {text(plan.title)}</strong>
        <span>
          {plan.steps.length} {plan.steps.length === 1 ? 'step' : 'steps'}
        </span>
      </header>
      <ol className="plan-card-steps">
        {plan.steps.map((step, index) => (
          <li key={index}>
            <span className="plan-card-number">{index + 1}</span>
            <span className="plan-card-action">{text(step.action)}</span>
            {step.asksFirst && <span className="plan-card-asks">Asks first</span>}
          </li>
        ))}
      </ol>
      {plan.estimate && (
        <p className="plan-card-estimate">{text(plan.estimate)}</p>
      )}
      {working && (
        <div className="plan-card-status">
          <span className="plan-card-dot" aria-hidden="true" />
          <span>Working on it</span>
          {onStop && (
            <button type="button" onClick={onStop}>
              Stop
            </button>
          )}
        </div>
      )}
    </section>
  );
}
