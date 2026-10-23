import { useEffect, useMemo, useRef, useState } from 'react';
import type { Automation, Run, Session } from '@animaos/sdk';

import {
  decideApproval as sendApprovalDecision,
  type ApprovalDecide,
} from '../lib/approvals';
import { automationNoticeFor } from '../lib/automations';
import type { LiveState } from '../lib/session-events';
import type {
  HelperTarget,
  ToolStep,
  TranscriptActions,
} from '../lib/transcript';

/** The session a helper card opens: its live child run's, else the listed
 *  helper session the call's run started. */
export function helperSessionTarget(
  step: ToolStep,
  liveRuns: LiveState['runs'],
  sessions: readonly Session[],
): HelperTarget | null {
  return helperSessionFromChildren(
    step,
    Object.values(liveRuns).map((item) => item.run),
    sessions,
  );
}

function helperSessionFromChildren(
  step: ToolStep,
  children: readonly Pick<Run, 'parentRunId' | 'agentId' | 'sessionId'>[],
  sessions: readonly Session[],
): HelperTarget | null {
  const helperAgentId = step.helper?.agentId;
  if (!helperAgentId || !step.runId) return null;
  const child = children.find(
    (run) => run.parentRunId === step.runId && run.agentId === helperAgentId,
  );
  if (child) return { agentId: child.agentId, sessionId: child.sessionId };
  const session = sessions.find(
    (item) => item.agentId === helperAgentId && item.parentRunId === step.runId,
  );
  return session ? { agentId: session.agentId, sessionId: session.id } : null;
}

export interface TranscriptActionOptions {
  /** The open session, once its record is known. */
  session: Session | null;
  /** Its messages can be sent again (a Telegram session needs its connector). */
  resendable: boolean;
  /** The companion's live runs, where a helper card finds its child run. */
  liveRuns: LiveState['runs'];
  /** The listed sessions, where it finds a finished helper's session. */
  sessions: readonly Session[];
  stopRun: (run: Run) => void;
  /** Takes back a send the daemon has not accepted (S3b-I). */
  cancelPending: (key: string) => void;
  /** Sends a run's message again; false when it could not go. */
  sendAgain: (run: Run) => boolean;
  compact: (session: Session) => void;
  /** A manual Compact is in flight (S3b-C). */
  compacting: boolean;
  openSession: (target: HelperTarget) => void;
  /** Sends an approval decision; the daemon call by default. */
  decideApproval?: ApprovalDecide;
  /** The companion shown: only its approvals can become rules. */
  companionId?: string | null;
  /** The companion's automations, where a notice card finds its own. */
  automations?: readonly Automation[];
  /** Deletes an automation (the notice card's Undo). */
  undoAutomation?: (automation: Automation) => Promise<boolean>;
  /** The automation list was read at least once: reminder cards know their
   *  state from then on. */
  automationsLoaded?: boolean;
  online?: boolean;
  /** A reminder card's Done and Snooze. */
  completeAutomation?: (automation: Automation) => Promise<boolean>;
  snoozeAutomation?: (
    automation: Automation,
    minutes: number,
  ) => Promise<boolean>;
  /** A Discord note's "Open thread": a session, at one of its messages. */
  openSessionAt?: (sessionId: string, messageId?: string) => void;
}

const NO_AUTOMATIONS: readonly Automation[] = [];

/**
 * What the owner can do from the open session's transcript (spec §15.2):
 * cancel a queued message, send a failed or interrupted one again, compact
 * the session, and open a helper's session, each as the session's
 * capabilities allow. The actions keep one identity per capability and
 * sessions-list change, so neither a keystroke in the composer nor a
 * streamed delta renders every message again. A helper card reads the live
 * runs as it renders: a live run's cards render with each of its events,
 * and a history card again once the sessions list, read after every run's
 * lifecycle event, changes.
 */
export function useTranscriptActions({
  session,
  resendable,
  liveRuns,
  sessions,
  stopRun,
  cancelPending,
  sendAgain,
  compact,
  compacting,
  openSession,
  decideApproval,
  companionId,
  automations = NO_AUTOMATIONS,
  undoAutomation,
  automationsLoaded = false,
  online = true,
  completeAutomation,
  snoozeAutomation,
  openSessionAt,
}: TranscriptActionOptions): TranscriptActions {
  const latest = {
    stopRun,
    cancelPending,
    sendAgain,
    compact,
    openSession,
    decideApproval,
    undoAutomation,
    completeAutomation,
    snoozeAutomation,
    openSessionAt,
  };
  const latestRef = useRef(latest);
  useEffect(() => {
    latestRef.current = latest;
  });
  // A run sent again from this page is not sent a second time: its button
  // is used up for as long as the page is open.
  const [resent, setResent] = useState<ReadonlySet<string>>(() => new Set());
  const resentRef = useRef(resent);
  const cancellable = session?.capabilities.stop === true;
  const canResend = resendable && session?.capabilities.send === true;
  const compactable = session?.capabilities.compact ? session : null;
  const canUndo = undoAutomation !== undefined;
  const canAnswerReminders =
    completeAutomation !== undefined && snoozeAutomation !== undefined;
  const sessionAgentId = session?.agentId;
  // Tokens, steps and status do not affect helper navigation. Only changing a
  // parent/agent/session target should invalidate every transcript action.
  const helperSignature = JSON.stringify(
    Object.values(liveRuns)
      .filter(({ run }) => run.parentRunId)
      .map(({ run }) => ({
        parentRunId: run.parentRunId,
        agentId: run.agentId,
        sessionId: run.sessionId,
      })),
  );
  const helperChildren = useMemo<
    Pick<Run, 'parentRunId' | 'agentId' | 'sessionId'>[]
  >(() => JSON.parse(helperSignature), [helperSignature]);
  return useMemo<TranscriptActions>(
    () => ({
      ...(cancellable
        ? {
            onCancelQueued: (run: Run) => latestRef.current.stopRun(run),
            onStopRun: (run: Run) => latestRef.current.stopRun(run),
          }
        : {}),
      onCancelPending: (key) => latestRef.current.cancelPending(key),
      ...(canResend
        ? {
            onSendAgain: (run: Run) => {
              if (resentRef.current.has(run.id)) return;
              if (!latestRef.current.sendAgain(run)) return;
              resentRef.current = new Set(resentRef.current).add(run.id);
              setResent(resentRef.current);
            },
          }
        : {}),
      resentRunIds: resent,
      ...(compactable
        ? {
            onCompact: () => latestRef.current.compact(compactable),
            compacting,
          }
        : {}),
      helperSession: (step) =>
        helperSessionFromChildren(step, helperChildren, sessions),
      onOpenSession: (target) => latestRef.current.openSession(target),
      onDecideApproval: (approval, input) =>
        (latestRef.current.decideApproval ?? sendApprovalDecision)(
          approval,
          input,
        ),
      automationNotice: (step: ToolStep) =>
        automationNoticeFor(step, automations),
      ...(canUndo
        ? {
            onUndoAutomation: (automation: Automation) =>
              latestRef.current.undoAutomation?.(automation) ??
              Promise.resolve(false),
          }
        : {}),
      ...(canAnswerReminders
        ? {
            automationCards: {
              automations,
              loaded: automationsLoaded,
              online,
              onDone: (automation: Automation) =>
                latestRef.current.completeAutomation?.(automation) ??
                Promise.resolve(false),
              onSnooze: (automation: Automation, minutes: number) =>
                latestRef.current.snoozeAutomation?.(automation, minutes) ??
                Promise.resolve(false),
              onOpenSession: (sessionId: string, messageId?: string) =>
                latestRef.current.openSessionAt?.(sessionId, messageId),
            },
          }
        : {}),
      ...(companionId ? { companionAgentId: companionId } : {}),
      ...(sessionAgentId ? { sessionAgentId } : {}),
    }),
    [
      cancellable,
      canResend,
      compactable,
      sessions,
      helperChildren,
      resent,
      compacting,
      companionId,
      sessionAgentId,
      automations,
      canUndo,
      canAnswerReminders,
      automationsLoaded,
      online,
    ],
  );
}
