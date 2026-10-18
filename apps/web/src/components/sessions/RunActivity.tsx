import { useEffect, useState, type ReactNode } from 'react';
import { isTerminalRunStatus, type Automation } from '@animaOS-SWARM/sdk';

import { isActiveRun, type LiveRun } from '../../lib/session-events';
import {
  formatElapsed,
  hasWrittenInput,
  liveToolSteps,
  type PendingBubble,
  type ToolStep,
  type TranscriptActions,
} from '../../lib/transcript';
import type { ChatMessage } from '../../lib/types';
import { ApprovalCard } from './ApprovalCard';
import { AutomationNoticeCard } from './AutomationNoticeCard';
import { HelperCard } from './HelperCard';
import { PlanNoteCard } from './PlanCard';
import { ToolStepCard } from './ToolStepCard';
import { CodingSessionCard } from '../coding/CodingSessionCard';
import {
  mergeAcpSnapshots,
  parseCodingSession,
  type CodingSessionView,
} from '../../lib/coding-sessions';
import { AttachmentReferences } from '../attachments/AttachmentReferences';
import { messageAttachment } from '../../lib/attachments';
import {
  attachmentsForRun,
  EMPTY_RUN_ATTACHMENTS,
  type KnownRunAttachments,
} from '../../lib/run-attachments';

type RenderMessage = (message: ChatMessage) => ReactNode;

function useElapsed(
  startedAtMs: number | null,
  finishedAtMs: number | null,
): number {
  const [now, setNow] = useState(() => Date.now());
  const running = startedAtMs !== null && finishedAtMs === null;
  useEffect(() => {
    if (!running) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(timer);
  }, [running]);
  if (startedAtMs === null) return 0;
  return Math.max(0, (finishedAtMs ?? now) - startedAtMs);
}

/**
 * Each card's key: its step and call id, which the daemon's live registry
 * keys cards by too, since a provider may reuse a call id in every step of
 * a run (the adapters' synthesized ids are unique within a response). A
 * pair seen again (a recovered result beside its call's card) takes its
 * occurrence, so no two cards share a key.
 */
function stepKeys(steps: readonly ToolStep[]): string[] {
  const seen = new Map<string, number>();
  return steps.map((step) => {
    const base = `${step.stepId ?? ''}\u0000${step.toolCallId}`;
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    return count === 0 ? base : `${base}\u0000${count}`;
  });
}

interface SessionSnapshot {
  session: CodingSessionView;
  /** Where its latest snapshot is among the steps. */
  index: number;
  typed: number;
}

/** The latest snapshot of each app or coding-agent session the steps
 *  touched, in the order they first appear. */
function sessionSnapshots(steps: readonly ToolStep[]): SessionSnapshot[] {
  const byId = new Map<string, SessionSnapshot>();
  steps.forEach((step, index) => {
    if (step.status !== 'success') return;
    const parsed = parseCodingSession(step.result);
    if (!parsed) return;
    const key = `${parsed.kind}:${parsed.id}`;
    const known = byId.get(key);
    const session =
      parsed.kind === 'acp_session' && known?.session.kind === 'acp_session'
        ? mergeAcpSnapshots(known.session, parsed)
        : parsed;
    const typed =
      (known?.typed ?? 0) + (step.name === 'app_send' ? 1 : 0);
    byId.set(key, { session, index, typed });
  });
  // A later call on the same session that is still running (a prompt the
  // coding agent is working on, maybe waiting on an approval) outranks the
  // last finished snapshot's status.
  steps.forEach((step, index) => {
    if (step.status !== 'running') return;
    const kind = step.name.startsWith('acp_')
      ? 'acp_session'
      : step.name.startsWith('app_')
        ? 'app_session'
        : null;
    const id = /(?:^|,\s*)id:\s*([a-z]+-\d+)/.exec(step.argumentsPreview)?.[1];
    if (!kind || !id) return;
    const known = byId.get(`${kind}:${id}`);
    if (!known || known.index > index) return;
    const status = known.session.status;
    if (status === 'stopped' || status === 'exited') return;
    byId.set(`${kind}:${id}`, {
      ...known,
      session: { ...known.session, status: 'running' } as CodingSessionView,
    });
  });
  return [...byId.values()];
}

function typedNote(typed: number): string | undefined {
  if (typed === 0) return undefined;
  return `Typed ${typed} ${typed === 1 ? 'message' : 'messages'}`;
}

/** Tool requests as cards (spec §15.2): open while the run works, then
 *  collapsed to "N tool requests · Ns". */
export function ToolBlock({
  steps,
  active,
  elapsedMs,
  actions,
  onStop,
}: {
  steps: readonly ToolStep[];
  active: boolean;
  elapsedMs?: number;
  actions?: TranscriptActions;
  /** Stops the run these steps belong to (a plan card's Stop). */
  onStop?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const count = steps.length;
  const total =
    elapsedMs ?? steps.reduce((sum, step) => sum + (step.durationMs ?? 0), 0);
  const label = active
    ? count === 0
      ? `Working · ${formatElapsed(total)}`
      : `Working · ${count} ${count === 1 ? 'step' : 'steps'} · ${formatElapsed(total)}`
    : `${count} ${count === 1 ? 'tool request' : 'tool requests'} · ${formatElapsed(total)}`;
  const expanded = active || open;
  const keys = stepKeys(steps);
  const agentId = actions?.sessionAgentId;
  const snapshots = sessionSnapshots(steps);
  const pinned = new Set(snapshots.map((snapshot) => snapshot.index));
  // Spec §15.2: an automation the companion created stays visible with
  // Undo, even when the block is collapsed.
  const notices: Automation[] = [];
  for (const candidate of steps) {
    const automation = actions?.automationNotice?.(candidate) ?? null;
    if (automation && !notices.some((known) => known.id === automation.id))
      notices.push(automation);
  }
  // A plan the agent showed and started on its own (Free autonomy); one
  // waiting for the owner is an approval card instead.
  const plans = steps.flatMap((step, index) =>
    step.status === 'success' && step.plan
      ? [{ plan: step.plan, key: keys[index] }]
      : [],
  );
  return (
    <div className="tool-block" data-active={active || undefined}>
      {plans.map(({ plan, key }) => (
        <PlanNoteCard
          key={key}
          plan={plan}
          working={active}
          onStop={active ? onStop : undefined}
        />
      ))}
      {active ? (
        <p className="tool-block-label">{label}</p>
      ) : (
        <button
          type="button"
          className="tool-block-toggle"
          aria-expanded={open}
          onClick={() => setOpen((value) => !value)}
        >
          {label}
        </button>
      )}
      {expanded && count > 0 && (
        <ul className="tool-block-steps">
          {steps.map((step, index) => (
            <li key={keys[index]}>
              {step.helper ? (
                <HelperCard
                  step={step}
                  target={actions?.helperSession?.(step) ?? null}
                  onOpen={actions?.onOpenSession}
                />
              ) : (
                <ToolStepCard
                  step={step}
                  agentId={agentId}
                  pinned={pinned.has(index)}
                />
              )}
            </li>
          ))}
        </ul>
      )}
      {snapshots.map(({ session, typed, index }) => (
        // A newer snapshot mounts afresh: a local Stop or Cancel no longer
        // stands in for what the daemon reported.
        <CodingSessionCard
          key={`${session.kind}:${session.id}:${keys[index]}`}
          session={session}
          agentId={agentId}
          active={active}
          footer={typedNote(typed)}
        />
      ))}
      {notices.map((automation) => (
        <AutomationNoticeCard
          key={automation.id}
          automation={automation}
          onUndo={actions?.onUndoAutomation}
        />
      ))}
    </div>
  );
}

/** A run not yet in the session's history (spec §15.2): a queued message,
 *  the reply in progress, or a finished one waiting for its messages. */
export function RunActivity({
  live,
  agentName,
  actions,
  renderMessage,
  knownAttachments = EMPTY_RUN_ATTACHMENTS,
}: {
  live: LiveRun;
  agentName: string;
  actions?: TranscriptActions;
  renderMessage: RenderMessage;
  knownAttachments?: KnownRunAttachments;
}) {
  const { run } = live;
  const attachments = attachmentsForRun(knownAttachments, run);
  const unknownIds = run.input.attachmentIds.filter(
    (id) => !attachments.some((item) => item.id === id),
  );
  const active = isActiveRun(run);
  const elapsed = useElapsed(
    run.startedAtMs,
    isTerminalRunStatus(run.status)
      ? (run.finishedAtMs ?? run.startedAtMs)
      : null,
  );
  const input = hasWrittenInput(run)
    ? renderMessage({
        id: `${run.id}:input`,
        role: 'User',
        content: {
          text: run.input.text,
          ...(attachments.length
            ? { attachments: attachments.map(messageAttachment) }
            : {}),
        },
        created_at_ms: run.createdAtMs,
      })
    : null;

  if (run.status === 'queued') {
    return (
      <div className="run-queued">
        {input}
        <AttachmentReferences ids={unknownIds} />
        <div className="run-queued-meta">
          <span>Queued</span>
          {actions?.onCancelQueued && (
            <button
              type="button"
              className="studio-tool-button"
              onClick={() => actions.onCancelQueued?.(run)}
            >
              Cancel
            </button>
          )}
        </div>
      </div>
    );
  }

  const steps = liveToolSteps(live);
  const at = run.startedAtMs ?? run.createdAtMs;
  return (
    <section
      className="run-activity"
      aria-label={active ? `${agentName} is replying` : `${agentName}’s reply`}
    >
      {input}
      <AttachmentReferences ids={unknownIds} />
      {live.steers.map((steer) => (
        <div key={steer.messageId}>
          {renderMessage({
            id: steer.messageId,
            role: 'User',
            content: {
              text: steer.text,
              metadata: { steer: true },
              ...(steer.attachments ? { attachments: steer.attachments } : {}),
            },
            created_at_ms: at,
          })}
        </div>
      ))}
      {(active || steps.length > 0) && (
        <ToolBlock
          steps={steps}
          active={active}
          elapsedMs={active ? elapsed : undefined}
          actions={actions}
          onStop={
            active && actions?.onStopRun
              ? () => actions.onStopRun?.(run)
              : undefined
          }
        />
      )}
      {live.approvals.map((approval) => (
        <ApprovalCard
          key={approval.id}
          approval={approval}
          onDecide={actions?.onDecideApproval}
          canPersist={approval.agentId === actions?.companionAgentId}
        />
      ))}
      {/* The status region stays mounted and only its text changes, so a
       *  screen reader announces the phase when it starts (S3b-E). */}
      <div role="status">
        {run.status === 'awaiting_approval' && (
          <p className="run-phase">Waiting for your approval…</p>
        )}
        {live.phase === 'compacting' && (
          <p className="run-phase">Compacting earlier messages…</p>
        )}
      </div>
      {live.steps
        .filter((step) => step.text)
        .map((step) => (
          <div key={step.stepId}>
            {renderMessage({
              id: step.stepId,
              role: 'Assistant',
              content: {
                text: step.textOffset > 0 ? `…${step.text}` : step.text,
              },
              created_at_ms: at,
            })}
          </div>
        ))}
    </section>
  );
}

const PENDING_LABELS: Record<PendingBubble['status'], string> = {
  sending: 'Sending…',
  retrying: 'Not delivered yet · retrying…',
  steering: 'Joining the reply in progress…',
};

/** A message on its way to the daemon (spec §15.5). */
export function PendingMessage({
  pending,
  onCancel,
  renderMessage,
}: {
  pending: PendingBubble;
  /** Offered until the daemon accepts it: a steer it took is not (S3b-I). */
  onCancel?: (key: string) => void;
  renderMessage: RenderMessage;
}) {
  return (
    <div className="pending-message" data-status={pending.status}>
      {renderMessage({
        id: `pending:${pending.key}`,
        role: 'User',
        content: {
          text: pending.text,
          ...(pending.attachments ? { attachments: pending.attachments } : {}),
        },
        created_at_ms: pending.createdAtMs,
      })}
      <p className="pending-message-label">
        {PENDING_LABELS[pending.status]}
        {onCancel && pending.status !== 'steering' ? (
          <>
            {' '}
            <button
              type="button"
              className="studio-tool-button"
              aria-label="Cancel sending"
              onClick={() => onCancel(pending.key)}
            >
              Cancel
            </button>
          </>
        ) : null}
      </p>
    </div>
  );
}
