import type {
  ActionReceipt,
  Approval,
  LearningReview,
  Run,
} from '@animaOS-SWARM/sdk';

import { CLASS_LABELS, revealHiddenCharacters } from './approvals';
import type { TelegramConnector } from './daemon-api';
import { PLAN_TOOL, parsePlan } from './plan';

/** Something waiting on the owner, in one queue (the "Needs you" inbox). */
export type NeedsYouItem =
  | { kind: 'approve'; id: string; atMs: number; approval: Approval }
  | {
      kind: 'pair';
      id: string;
      atMs: number;
      connector: TelegramConnector;
      chatId: string;
      chatName: string;
    }
  | {
      kind: 'fix';
      id: string;
      atMs: number;
      title: string;
      detail: string;
    }
  | {
      kind: 'review';
      id: string;
      atMs: number;
      review: LearningReview;
      suggestion: string;
    };

const BROKEN_CONNECTOR: Partial<Record<TelegramConnector['status'], string>> =
  {
    credentialRequired: 'The bot token needs replacing.',
    error: 'The bot stopped working. Restart it from Connections.',
    degraded: 'Messages are getting through slowly or not at all.',
  };

/** The queue, newest first: approvals, pairing requests, broken chat apps
 *  and skills waiting for a decision. */
export function needsYouItems({
  approvals,
  telegram,
  reviews,
}: {
  approvals: readonly Approval[];
  telegram: readonly TelegramConnector[];
  reviews: readonly LearningReview[];
}): NeedsYouItem[] {
  const items: NeedsYouItem[] = approvals.map((approval) => ({
    kind: 'approve',
    id: `approval:${approval.id}`,
    atMs: approval.createdAtMs,
    approval,
  }));
  for (const connector of telegram) {
    const pending = connector.pendingPairing;
    if (pending)
      items.push({
        kind: 'pair',
        id: `pair:${connector.id}`,
        atMs: pending.requestedAtMs,
        connector,
        chatId: pending.chat.id,
        chatName:
          pending.chat.title ?? pending.chat.username ?? pending.chat.id,
      });
    const broken = BROKEN_CONNECTOR[connector.status];
    if (broken)
      items.push({
        kind: 'fix',
        id: `connector:${connector.id}`,
        atMs: connector.updatedAtMs,
        title: `Telegram${connector.bot.username ? ` · @${connector.bot.username}` : ''} needs attention`,
        detail: broken,
      });
  }
  for (const review of reviews) {
    if (review.state !== 'completed') continue;
    for (const proposal of review.proposals) {
      if (proposal.decision !== 'pending') continue;
      items.push({
        kind: 'review',
        id: `proposal:${review.id}:${proposal.id}`,
        atMs: review.finishedAtMs ?? review.reservedAtMs,
        review,
        suggestion: proposal.suggestion,
      });
    }
  }
  return items.sort((left, right) => right.atMs - left.atMs);
}

/** An approval in a few words: the plan's title, or what the call does and
 *  its main argument. */
export function approvalHeadline(approval: Approval): {
  title: string;
  detail: string;
} {
  if (approval.tool === PLAN_TOOL) {
    const plan = parsePlan(approval.arguments);
    return {
      title: plan ? `Plan: ${plan.title}` : 'A plan to review',
      detail: plan
        ? `${plan.steps.length} ${plan.steps.length === 1 ? 'step' : 'steps'} · nothing starts until you say so`
        : 'Open the chat to see it.',
    };
  }
  const args = parseArguments(approval.arguments);
  const main = mainArgument(args);
  return {
    title: `${CLASS_LABELS[approval.class]} · ${revealHiddenCharacters(approval.tool)}`,
    detail: main ? revealHiddenCharacters(main) : 'Open the chat to see it.',
  };
}

/** Tools that only look: never listed among what the agent did. Task-list
 *  bookkeeping is left out too; it is noise in a list of effects. */
const QUIET_TOOLS: ReadonlySet<string> = new Set([
  'read_file',
  'read_document',
  'list_dir',
  'glob',
  'grep',
  'todo_read',
  'todo_write',
  'todo_list',
  'memory_search',
  'recent_memories',
  'get_current_time',
  'calculate',
  'bg_list',
  'bg_output',
  'list_workspace_agents',
  'list_helper_runs',
  'calendar_list_events',
  'mail_list_messages',
  'drive_search',
  'drive_read',
  'search_conversations',
  'read_conversation_window',
  'load_skill',
  'list_automations',
  PLAN_TOOL,
]);

const ACTION_LABELS: Record<string, string> = {
  write_file: 'Wrote a file',
  edit_file: 'Changed a file',
  multi_edit: 'Changed a file',
  bash: 'Ran a command',
  bg_start: 'Started a background command',
  bg_stop: 'Stopped a background command',
  execute_code: 'Ran code',
  web_fetch: 'Opened a web page',
  web_search: 'Searched the web',
  exa_search: 'Searched the web',
  memory_add: 'Saved a memory',
  memory_update: 'Updated a memory',
  memory_remove: 'Forgot a memory',
  memory_consolidate: 'Tidied its memory',
  propose_skill: 'Proposed a skill',
  create_automation: 'Created an automation',
  update_automation: 'Changed an automation',
  pause_automation: 'Paused an automation',
  resume_automation: 'Resumed an automation',
  delete_automation: 'Deleted an automation',
  trigger_automation: 'Ran an automation',
  mail_create_draft: 'Drafted an email',
  calendar_create_event: 'Added a calendar event',
  calendar_update_event: 'Moved a calendar event',
  calendar_delete_event: 'Removed a calendar event',
  send_message: 'Sent a message',
  broadcast_message: 'Sent a message to several agents',
  delegate_to_agent: 'Asked another agent',
  spawn_helper: 'Brought in a helper',
  spawn_helper_async: 'Brought in a helper',
  steer_helper: 'Steered a helper',
  stop_helper: 'Stopped a helper',
  browser_navigate: 'Opened a page in the browser',
  browser_act: 'Used the browser',
  browser_observe: 'Looked at the browser',
  browser_transfer: 'Moved a file through the browser',
};

/** One thing the agent did, from a run's action receipt. */
export interface DoneAction {
  id: string;
  runId: string;
  sessionId: string;
  atMs: number;
  title: string;
  detail: string | null;
  failed: boolean;
}

/** What the agent did, newest first: effects only, never plain reads. */
export function doneActions(runs: readonly Run[]): DoneAction[] {
  const actions: DoneAction[] = [];
  for (const run of runs) {
    for (const receipt of run.actionReceipts ?? []) {
      if (QUIET_TOOLS.has(receipt.name) || receipt.status === 'started')
        continue;
      actions.push({
        id: `${run.id}:${receipt.toolCallId}`,
        runId: run.id,
        sessionId: run.sessionId,
        atMs: receipt.finishedAtMs ?? receipt.startedAtMs,
        title:
          ACTION_LABELS[receipt.name] ?? revealHiddenCharacters(receipt.name),
        detail: receiptDetail(receipt),
        failed: receipt.status === 'failed',
      });
    }
  }
  return actions.sort((left, right) => right.atMs - left.atMs);
}

function receiptDetail(receipt: ActionReceipt): string | null {
  const main = mainArgument(parseArguments(receipt.argumentsPreview));
  return main ? revealHiddenCharacters(main) : null;
}

/** The argument that says what a call was about, in the order a person
 *  would look for it. */
const MAIN_KEYS = [
  'path',
  'file_path',
  'command',
  'url',
  'query',
  'title',
  'name',
  'content',
  'prompt',
  'message',
  'text',
  'to',
];

function parseArguments(text: string): Record<string, unknown> | null {
  try {
    const value: unknown = JSON.parse(text);
    return typeof value === 'object' && value !== null && !Array.isArray(value)
      ? (value as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

function mainArgument(args: Record<string, unknown> | null): string | null {
  if (!args) return null;
  for (const key of MAIN_KEYS) {
    const value = args[key];
    if (typeof value === 'string' && value.trim() !== '')
      return value.length > 120 ? `${value.slice(0, 119)}…` : value;
  }
  return null;
}
