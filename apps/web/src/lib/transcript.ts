import {
  isTerminalRunStatus,
  type Automation,
  type ModelFallback,
  type Run,
  type SessionMessageAttachment,
  type WebSource,
} from '@animaOS-SWARM/sdk';

import type { SaveOutcome, SavedState } from '../hooks/useSaveToMemory';
import type { ApprovalDecide } from './approvals';
import { PLAN_TOOL, parsePlan, type Plan } from './plan';
import { REVIEW_MIN_ACTIONS, type TaskReviewApi } from './task-reviews';
import { emptyLiveRun, stepRunId, type LiveRun } from './session-events';
import type { ChatMessage } from './types';
import { toolWebSources } from './web-sources';

/** Tools whose calls start a helper or delegated run (spec §15.2). */
export const HELPER_TOOLS: ReadonlySet<string> = new Set([
  'spawn_helper',
  'delegate_to_agent',
]);

/** Where a helper card's "Open session" goes. */
export interface HelperTarget {
  agentId: string;
  sessionId: string;
}

/** The helper a `spawn_helper` or `delegate_to_agent` call runs. */
export interface ToolHelper {
  /** The helper's name, or the specialist's id. */
  label: string;
  /** Known from the call (a specialist) or its result (a helper). */
  agentId: string | null;
}

/** One tool call as a card shows it (spec §15.2). */
export interface ToolStep {
  /** The model call that made it; with `toolCallId`, the card's identity
   *  (a provider may reuse a call id in every step). Null when unknown. */
  stepId: string | null;
  toolCallId: string;
  name: string;
  /** The call's arguments on one short line. */
  argumentsPreview: string;
  status: 'running' | 'success' | 'error';
  durationMs: number | null;
  /** The result, or the error of a failed call; null while it runs. */
  result: string | null;
  truncated: boolean;
  /** The run that made the call, when known. */
  runId: string | null;
  helper: ToolHelper | null;
  sources?: WebSource[];
  /** A `propose_plan` call's plan, when its arguments hold one. */
  plan?: Plan;
}

/** The plan in a `propose_plan` call's arguments (an object, or JSON text
 *  that may be cut short), as a step field. */
function planField(name: string, args: unknown): { plan?: Plan } {
  if (name !== PLAN_TOOL) return {};
  const plan = parsePlan(
    typeof args === 'string' ? args : JSON.stringify(args ?? null),
  );
  return plan ? { plan } : {};
}

/** A message the daemon has not accepted yet, or a steer not yet applied. */
export interface PendingBubble {
  key: string;
  text: string;
  attachments?: SessionMessageAttachment[];
  createdAtMs: number;
  status: 'sending' | 'retrying' | 'steering';
}

export type TranscriptItem =
  | { kind: 'message'; key: string; message: ChatMessage }
  | { kind: 'delegated'; key: string; message: ChatMessage; from: string }
  | { kind: 'revised'; key: string; message: ChatMessage }
  | { kind: 'tools'; key: string; steps: ToolStep[]; messageIds: string[] }
  | { kind: 'run'; key: string; live: LiveRun }
  | { kind: 'outcome'; key: string; run: Run }
  | { kind: 'learning-review'; key: string; run: Run }
  | { kind: 'model-note'; key: string; text: string }
  | { kind: 'pending'; key: string; pending: PendingBubble }
  | { kind: 'trimmed'; key: string };

type ToolsItem = Extract<TranscriptItem, { kind: 'tools' }>;

/** What the owner can do from the transcript. */
export interface TranscriptActions {
  onCancelQueued?: (run: Run) => void;
  /** Stops a run in progress (a plan card's Stop). */
  onStopRun?: (run: Run) => void;
  /** Takes back a message the daemon has not accepted (S3b-I). */
  onCancelPending?: (key: string) => void;
  onSendAgain?: (run: Run) => void;
  /** Reads and starts advice-only reviews of finished tasks; the daemon's
   *  learning routes when absent. */
  taskReviews?: TaskReviewApi;
  /** Runs already sent again from this page: their Retry or Send again is
   *  used up. */
  resentRunIds?: ReadonlySet<string>;
  onCompact?: () => void;
  /** A manual Compact is in flight (spec §15.2, S3b-C). */
  compacting?: boolean;
  helperSession?: (step: ToolStep) => HelperTarget | null;
  onOpenSession?: (target: HelperTarget) => void;
  /** Sends the owner's decision from an inline approval card (spec §7.3). */
  onDecideApproval?: ApprovalDecide;
  /** The companion whose transcript this is: only its approvals offer
   *  "Always allow", as rules for other agents are not managed here yet. */
  companionAgentId?: string;
  /** The agent whose session this is: it owns the app and coding-agent
   *  sessions its tool steps show, so their Stop goes to it. */
  sessionAgentId?: string;
  /** The automation a `create_automation` call made, for its notice card
   *  (spec §15.2). */
  automationNotice?: (step: ToolStep) => Automation | null;
  /** Undo (delete) an automation the companion made; true when it went. */
  onUndoAutomation?: (automation: Automation) => Promise<boolean>;
  /** Saves a chat message to the companion's memory (spec §10, §15.2). */
  onSaveToMemory?: (message: ChatMessage) => Promise<SaveOutcome>;
  /** Whether a message was saved from this page, for a bubble that mounts
   *  again; `shortened` when only its first characters were kept. */
  savedToMemory?: (messageId: string) => SavedState | null;
}

export interface TranscriptInput {
  /** The loaded history, oldest first. */
  messages: readonly ChatMessage[];
  /** The session's runs from its stream and ledger, oldest first. */
  runs?: readonly LiveRun[];
  pending?: readonly PendingBubble[];
  /** The newest message outside the companion's view (spec §5.3). */
  trimmedThrough?: string | null;
  /** Set for helper sessions: who wrote their user turns. */
  delegatedBy?: string | null;
}

type Fields = Record<string, unknown>;

function fieldsOf(value: unknown): Fields {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Fields)
    : {};
}

function stringField(fields: Fields, key: string): string | null {
  const value = fields[key];
  return typeof value === 'string' ? value : null;
}

function numberField(fields: Fields, key: string): number | null {
  const value = fields[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function parsedFields(text: string | null): Fields {
  if (!text) return {};
  try {
    return fieldsOf(JSON.parse(text));
  } catch {
    return {};
  }
}

function metadataOf(message: ChatMessage): Fields {
  return fieldsOf(message.content.metadata);
}

/** Shape check for a saved reply from committed session history. The caller
 *  supplies that history context; live and pending content is not eligible. */
export function isAcceptedBudgetWrapupMessage(message: ChatMessage): boolean {
  const metadata = metadataOf(message);
  const calls = metadata.toolCalls;
  return (
    message.role === 'Assistant' &&
    metadata.budgetWrapup === true &&
    metadata.revised !== true &&
    metadata.stopped !== true &&
    metadata.incomplete !== true &&
    (calls === undefined ||
      calls === null ||
      (Array.isArray(calls) && calls.length === 0))
  );
}

/** The run a committed message belongs to. */
export function messageRunId(message: ChatMessage): string | null {
  const metadata = metadataOf(message);
  const stepId = stringField(metadata, 'stepId');
  return stringField(metadata, 'runId') ?? (stepId ? stepRunId(stepId) : null);
}

function shorten(line: string, limit = 120): string {
  const flat = line.replace(/\s+/g, ' ').trim();
  return flat.length > limit ? `${flat.slice(0, limit - 1)}…` : flat;
}

/** A call's arguments on one short line: `key: value, …`. */
export function argumentsSummary(args: unknown): string {
  const entries = Object.entries(fieldsOf(args));
  if (entries.length === 0) return '';
  return shorten(
    entries
      .map(
        ([key, value]) =>
          `${key}: ${typeof value === 'string' ? value : JSON.stringify(value)}`,
      )
      .join(', '),
  );
}

/** A live card's JSON arguments preview (2 KiB at most, maybe cut). */
export function previewSummary(preview: string): string {
  try {
    return argumentsSummary(JSON.parse(preview));
  } catch {
    return shorten(preview);
  }
}

const DELEGATED_TASK_PREFIX = 'Task delegated by workspace manager ';

/** A delegated task without its routing preamble (the daemon's
 *  `delegated_task_text`). */
export function delegatedTaskText(text: string): string {
  if (!text.startsWith(DELEGATED_TASK_PREFIX)) return text;
  const split = text.indexOf('\n\n');
  return split < 0 ? text : text.slice(split + 2);
}

/** Totals such as "Used 3 tools · 4s". */
export function formatElapsed(ms: number): string {
  return ms < 1_000 ? '<1s' : `${Math.round(ms / 1_000)}s`;
}

function toolHelper(
  name: string,
  args: Fields,
  result: string | null,
): ToolHelper | null {
  if (!HELPER_TOOLS.has(name)) return null;
  const started = stringField(parsedFields(result), 'agentId');
  if (name === 'delegate_to_agent') {
    const agentId = stringField(args, 'agent_id');
    return { label: agentId ?? 'Specialist', agentId: agentId ?? started };
  }
  return { label: stringField(args, 'name') ?? 'Helper', agentId: started };
}

interface StoredCall {
  id: string;
  name: string;
  args: Fields;
}

function storedCalls(metadata: Fields): StoredCall[] {
  const calls = metadata.toolCalls;
  if (!Array.isArray(calls)) return [];
  return calls.flatMap((value): StoredCall[] => {
    const call = fieldsOf(value);
    const id = stringField(call, 'id');
    const name = stringField(call, 'name');
    return id && name ? [{ id, name, args: fieldsOf(call.args) }] : [];
  });
}

/** `"success"` or `"error"` from the daemon's own `toolStatus` marker
 *  (spec §3.3); `taskResult` is never exposed to the web (Ruling 3). A
 *  pre-M3 tool message carries no `toolStatus` marker at all, so its
 *  status falls back to the `{status,data,error}` JSON shape of its own
 *  text (the same shape a failed result's text uses). */
function resultStatus(
  message: ChatMessage,
  metadata: Fields,
): 'success' | 'error' {
  const marker = stringField(metadata, 'toolStatus');
  if (marker) return marker === 'error' ? 'error' : 'success';
  return stringField(parsedFields(message.content.text), 'status') === 'error'
    ? 'error'
    : 'success';
}

function resultDuration(metadata: Fields): number | null {
  return numberField(metadata, 'toolDurationMs');
}

/** The result text a card shows: on success, the message's own text; on
 *  failure, the error the daemon wrote into the message's JSON text
 *  (`content_from_tool_result` in anima-core's runtime — `taskResult`
 *  itself stays hidden from the web, Ruling 3). */
function resultText(message: ChatMessage, status: 'success' | 'error'): string {
  if (status !== 'error') return message.content.text;
  return (
    stringField(parsedFields(message.content.text), 'error') ??
    message.content.text
  );
}

/** The tool cards of a run the stream is showing. */
export function liveToolSteps(live: LiveRun): ToolStep[] {
  return live.tools.map((card) => {
    const sources = toolWebSources(card.name, card.status, card.sources);
    return {
      stepId: card.stepId,
      toolCallId: card.toolCallId,
      name: card.name,
      argumentsPreview: previewSummary(card.argumentsPreview),
      status: card.status,
      durationMs: card.durationMs,
      result: card.resultPreview,
      truncated: card.truncated,
      ...(sources.length ? { sources } : {}),
      ...planField(card.name, card.argumentsPreview),
      runId: live.run.id,
      helper: toolHelper(
        card.name,
        parsedFields(card.argumentsPreview),
        card.resultPreview,
      ),
    };
  });
}

/** Where a non-terminal status sits in the run lifecycle (spec §4.1).
 *  Running and awaiting approval are both in flight and rank the same: the
 *  stream's view of either wins (M4 Review Focus 5). */
const NON_TERMINAL_ORDER: Record<string, number> = {
  queued: 0,
  running: 1,
  awaiting_approval: 1,
};

/** Whether `next` is further along the (non-terminal) lifecycle than
 *  `current` — never true once either side is terminal (a finish is
 *  decided by the terminal-status branch above, not this one). */
function isMoreAdvanced(next: Run['status'], current: Run['status']): boolean {
  if (isTerminalRunStatus(next) || isTerminalRunStatus(current)) return false;
  return NON_TERMINAL_ORDER[next] > NON_TERMINAL_ORDER[current];
}

/** The session's runs: the stream's view of each, the ledger's record for
 *  the rest, and the ledger's finish when the stream missed it. When both
 *  are still mid-flight, the more advanced status wins, so a stale
 *  `queued` snapshot never outlives the ledger's `running`. */
export function mergeSessionRuns(
  live: readonly LiveRun[],
  ledger: readonly Run[],
): LiveRun[] {
  const byId = new Map(live.map((item) => [item.run.id, item]));
  for (const run of ledger) {
    const current = byId.get(run.id);
    if (!current) byId.set(run.id, emptyLiveRun(run));
    else if (
      isTerminalRunStatus(run.status) &&
      !isTerminalRunStatus(current.run.status)
    )
      byId.set(run.id, { ...current, run });
    else if (isMoreAdvanced(run.status, current.run.status))
      byId.set(run.id, { ...current, run });
  }
  return [...byId.values()].sort(
    (left, right) => left.run.createdAtMs - right.run.createdAtMs,
  );
}

/** Sources whose input is a message someone wrote into the session, which
 *  a run shows as its user turn. */
const WRITTEN_INPUT: ReadonlySet<Run['source']> = new Set([
  'web',
  'api',
  'telegram',
  'discord',
]);

export const RECOVERY_WORK_NOTICE = 'Continuing reviewed work';

export function hasWrittenInput(
  run: Pick<Run, 'source' | 'recoveryOf'>,
): boolean {
  return !run.recoveryOf && WRITTEN_INPUT.has(run.source);
}

/** The daemon's own note after a calendar write is applied: an `api` run
 *  that is not the owner's turn (the daemon's `is_calendar_write_followup`). */
const CALENDAR_WRITE_REF_PREFIX = 'calendar-write:';

/** A Telegram inbound turn's ref, `<connectorId>:<updateId>`, parsed the
 *  way the daemon's stop does (`stop_inbound`: split at the last colon, an
 *  integer after it). Connector ids (`telegram-<ms>-<n>`) have no colon; an
 *  owner turn sent from the console carries the bare connector id. */
const TELEGRAM_INBOUND_REF = /:[+-]?\d+$/;

/**
 * Whether the owner wrote the run's message, so Retry and Send again may
 * send it as theirs (spec §15.2): a web or API message, or a Telegram owner
 * turn sent from the console. Never a check-in's prompt, a job's or a
 * delegated or peer task, the daemon's calendar note, or a Telegram inbound
 * turn (someone else's message, which the daemon itself re-runs on boot).
 * A Telegram run without a ref cannot show it is an owner turn, so it is
 * not offered.
 */
export function isOwnerWritten(
  run: Pick<Run, 'source' | 'sourceRef' | 'recoveryOf'>,
): boolean {
  if (run.recoveryOf) return false;
  switch (run.source) {
    case 'web':
      return true;
    case 'api':
      return !run.sourceRef?.startsWith(CALENDAR_WRITE_REF_PREFIX);
    case 'telegram':
      return (
        run.sourceRef !== null && !TELEGRAM_INBOUND_REF.test(run.sourceRef)
      );
    case 'discord':
      // Native inbound refs append the provider message snowflake; owner
      // turns from the web carry the bare connector id.
      return run.sourceRef !== null && !/:[1-9]\d*$/.test(run.sourceRef);
    default:
      return false;
  }
}

/** A finished run whose outcome the owner should see (spec §15.2). */
function hasOutcome(run: Run): boolean {
  return (
    run.status === 'failed' ||
    run.status === 'interrupted' ||
    (run.status === 'cancelled' && run.startedAtMs !== null)
  );
}

function canReviewCompletedRun(run: Run, hasLinkedReply: boolean): boolean {
  const receipts = run.actionReceipts;
  return (
    run.status === 'completed' &&
    run.parentRunId === null &&
    run.error === null &&
    run.stop === null &&
    run.replyMessageId !== null &&
    hasLinkedReply &&
    isOwnerWritten(run) &&
    receipts !== undefined &&
    // Short tasks hold too little to learn from; their card is noise.
    receipts.length >= REVIEW_MIN_ACTIONS &&
    receipts.every(
      (receipt) =>
        receipt.status !== 'started' && receipt.durability === 'persistent',
    )
  );
}

function messageItem(
  message: ChatMessage,
  delegatedBy: string | null,
): TranscriptItem {
  const metadata = metadataOf(message);
  if (message.role === 'User' && stringField(metadata, 'recoveryOf'))
    return { kind: 'model-note', key: message.id, text: RECOVERY_WORK_NOTICE };
  if (message.role === 'Assistant' && metadata.revised === true)
    return { kind: 'revised', key: message.id, message };
  if (message.role === 'User' && delegatedBy)
    return {
      kind: 'delegated',
      key: message.id,
      from: delegatedBy,
      message: {
        ...message,
        content: {
          ...message.content,
          text: delegatedTaskText(message.content.text),
        },
      },
    };
  return { kind: 'message', key: message.id, message };
}

/** The loaded history as transcript items, with what placing the runs
 *  among them needs. */
export interface TranscriptHistory {
  readonly items: readonly TranscriptItem[];
  /** When each item's first message was created; null for the divider. */
  readonly startedAtMs: readonly (number | null)[];
  /** The index of each run's last item. */
  readonly lastOfRun: ReadonlyMap<string, number>;
  /** The index of the last item of each model call (a step id). */
  readonly lastOfStep: ReadonlyMap<string, number>;
  /** Runs whose committed partial reply is labelled "Stopped". */
  readonly stoppedRuns: ReadonlySet<string>;
  /** When the oldest loaded message was created; null without history. */
  readonly oldestLoadedMs: number | null;
}

export type HistoryInput = Pick<
  TranscriptInput,
  'messages' | 'trimmedThrough' | 'delegatedBy'
>;

/** The session's history with tool steps grouped (spec §15.2). Built once
 *  per history change, so its items keep their identity while the
 *  session's runs stream and the view skips rendering them again. */
export function buildHistory(input: HistoryInput): TranscriptHistory {
  const items: TranscriptItem[] = [];
  const startedAtMs: (number | null)[] = [];
  const push = (item: TranscriptItem, at: number | null) => {
    items.push(item);
    startedAtMs.push(at);
  };
  const lastOfRun = new Map<string, number>();
  const lastOfStep = new Map<string, number>();
  const stoppedRuns = new Set<string>();
  // Ruling 3: an orphan tool card (its call is not in the same block)
  // takes its name from the matching assistant message's `toolCalls`,
  // never a `toolName` metadata key the daemon does not write. This spans
  // the whole loaded page, not just the current block, so a delayed or
  // recovered result still gets the name its call used.
  const callNames = new Map<string, string>();
  const trimmed = input.trimmedThrough ?? null;
  const delegatedBy = input.delegatedBy ?? null;
  let block: ToolsItem | null = null;
  // The run `block` belongs to; a message from a different run (or a
  // known run following an unattributed one, or vice versa) starts a new
  // block instead of joining it, even with nothing textual in between.
  let blockRunId: string | null = null;

  if (trimmed && !input.messages.some((message) => message.id === trimmed))
    push({ kind: 'trimmed', key: 'trimmed' }, null);

  // The open block belonging to `runId`, or null if there is none (`block`
  // is read live: a same-iteration reset below must be seen by a later
  // check in that same iteration, not a value captured before it).
  const openBlockFor = (runId: string | null): ToolsItem | null =>
    block && blockRunId === runId ? block : null;

  for (const message of input.messages) {
    const runId = messageRunId(message);
    const metadata = metadataOf(message);
    // Set when a result answers a card placed earlier: its run's last item
    // stays where it was.
    let answeredEarlier = false;
    if (message.role === 'Tool') {
      const callId = stringField(metadata, 'toolCallId');
      const stepId = stringField(metadata, 'stepId');
      const status = resultStatus(message, metadata);
      const result = resultText(message, status);
      const open = openBlockFor(runId);
      // A result answers the call of its own step: a provider may reuse a
      // call id in every step of a run.
      const answers = (item: ToolStep) =>
        item.toolCallId === callId &&
        item.result === null &&
        (stepId === null || item.stepId === null || item.stepId === stepId);
      let owner: ToolsItem | null = open;
      let step = open?.steps.find(answers);
      // A recovered or delayed result, recorded after its run moved on,
      // answers its call's earlier card in the same run (T17).
      if (!step && callId !== null && runId !== null)
        for (let index = items.length - 1; index >= 0 && !step; index -= 1) {
          const item = items[index];
          if (item.kind !== 'tools' || item === open) continue;
          step = item.steps.find(
            (candidate) => candidate.runId === runId && answers(candidate),
          );
          if (step) {
            owner = item;
            answeredEarlier = true;
          }
        }
      if (owner && step) {
        step.status = status;
        step.durationMs = resultDuration(metadata);
        step.result = result;
        const ownsSourceResult =
          runId !== null &&
          stringField(metadata, 'runId') === runId &&
          stepId !== null &&
          callId !== null &&
          step.runId === runId &&
          step.stepId === stepId &&
          step.toolCallId === callId &&
          metadata.toolStatus === 'success';
        const sources = toolWebSources(
          step.name,
          ownsSourceResult ? 'success' : 'error',
          metadata.webSources,
        );
        if (sources.length) step.sources = sources;
        else delete step.sources;
        if (step.helper && !step.helper.agentId)
          step.helper = {
            ...step.helper,
            agentId: stringField(parsedFields(message.content.text), 'agentId'),
          };
        owner.messageIds.push(message.id);
      } else {
        // A result whose call is on an older page, or not in this block,
        // still gets its own card.
        const orphan: ToolStep = {
          stepId,
          toolCallId: callId ?? message.id,
          name: (callId && callNames.get(callId)) ?? 'tool',
          argumentsPreview: '',
          status,
          durationMs: resultDuration(metadata),
          result,
          truncated: false,
          runId,
          helper: null,
        };
        if (open) {
          open.steps.push(orphan);
          open.messageIds.push(message.id);
        } else {
          block = {
            kind: 'tools',
            key: `tools:${message.id}`,
            steps: [orphan],
            messageIds: [message.id],
          };
          blockRunId = runId;
          push(block, message.created_at_ms);
        }
      }
    } else {
      const calls = message.role === 'Assistant' ? storedCalls(metadata) : [];
      for (const call of calls) callNames.set(call.id, call.name);
      if (calls.length === 0 || message.content.text.trim()) {
        block = null;
        blockRunId = null;
        push(messageItem(message, delegatedBy), message.created_at_ms);
      }
      if (calls.length > 0) {
        const stepId = stringField(metadata, 'stepId');
        const steps = calls.map(
          (call): ToolStep => ({
            stepId,
            toolCallId: call.id,
            name: call.name,
            argumentsPreview: argumentsSummary(call.args),
            status: 'running',
            durationMs: null,
            result: null,
            truncated: false,
            runId,
            helper: toolHelper(call.name, call.args, null),
            ...planField(call.name, call.args),
          }),
        );
        // Re-checked after the reset just above: a message with both text
        // and calls always starts its tool block fresh, never merging
        // into the block its own leading bubble just closed.
        const current = openBlockFor(runId);
        if (current) {
          current.steps.push(...steps);
          current.messageIds.push(message.id);
        } else {
          block = {
            kind: 'tools',
            key: `tools:${message.id}`,
            steps,
            messageIds: [message.id],
          };
          blockRunId = runId;
          push(block, message.created_at_ms);
        }
      }
    }
    // The model call's own messages: its reply, or the calls it made. A
    // tool result belongs to the call before it, so it adds no anchor.
    const messageStepId = stringField(metadata, 'stepId');
    if (messageStepId && message.role === 'Assistant')
      lastOfStep.set(messageStepId, items.length - 1);
    if (runId) {
      if (!answeredEarlier) lastOfRun.set(runId, items.length - 1);
      if (metadata.stopped === true) stoppedRuns.add(runId);
    }
    if (message.id === trimmed) {
      block = null;
      blockRunId = null;
      push({ kind: 'trimmed', key: 'trimmed' }, null);
    }
  }

  // History is committed whole: a stored call with no result never got one.
  for (const item of items)
    if (item.kind === 'tools')
      for (const step of item.steps)
        if (step.result === null) step.status = 'error';

  return {
    items,
    startedAtMs,
    lastOfRun,
    lastOfStep,
    stoppedRuns,
    oldestLoadedMs:
      input.messages.length > 0 ? input.messages[0].created_at_ms : null,
  };
}

export interface LiveInput {
  runs?: readonly LiveRun[];
  pending?: readonly PendingBubble[];
  /** Older history may exist beyond the loaded page (assumed unless said
   *  otherwise): a run older than the page's oldest message belongs among
   *  messages not loaded, so it waits for them. */
  olderHistory?: boolean;
}

/** An item a run adds, placed after the history item at `after`. */
interface RunInsert {
  after: number;
  /** The run's place in `runs`, which keeps ties in order. */
  order: number;
  /** Breaks a tie within one run: the higher rank sits later. */
  rank: number;
  items: TranscriptItem[];
}

/** The owner's note that a fallback model answered a step. */
function modelNoteText(note: ModelFallback): string {
  return `Answered by ${note.provider} ${note.model} because ${note.primary.model} ${note.reason}.`;
}

function stepNumber(stepId: string): number {
  const number = Number(stepId.slice(stepId.lastIndexOf(':') + 1));
  return Number.isFinite(number) ? number : Number.MAX_SAFE_INTEGER;
}

/**
 * The steps of a run a fallback model answered, from the run's record and
 * from its stream (a step that is still going is only on the stream). A run
 * that stays on the fallback says so once: a step repeating the previous
 * note's model and reason adds none, unless a step the primary answered came
 * between them.
 */
function modelNotes(live: LiveRun): { stepId: string; note: TranscriptItem }[] {
  const { run } = live;
  /** `null` for a recorded step the agent's own model answered. */
  const byStep = new Map<string, ModelFallback | null>();
  for (const step of run.steps)
    byStep.set(
      step.stepId,
      step.provider && step.model && step.fallbackReason
        ? {
            stepId: step.stepId,
            provider: step.provider,
            model: step.model,
            reason: step.fallbackReason,
            primary: { provider: run.provider, model: run.model },
          }
        : null,
    );
  for (const fallback of live.fallbacks ?? [])
    if (!byStep.get(fallback.stepId)) byStep.set(fallback.stepId, fallback);
  const notes: { stepId: string; note: TranscriptItem }[] = [];
  let previous: ModelFallback | null = null;
  for (const [, fallback] of [...byStep.entries()].sort(
    ([left], [right]) => stepNumber(left) - stepNumber(right),
  )) {
    if (!fallback) {
      previous = null;
      continue;
    }
    if (
      previous &&
      previous.provider === fallback.provider &&
      previous.model === fallback.model &&
      previous.reason === fallback.reason
    )
      continue;
    previous = fallback;
    notes.push({
      stepId: fallback.stepId,
      note: {
        kind: 'model-note',
        key: `model-note:${run.id}:${fallback.stepId}`,
        text: modelNoteText(fallback),
      },
    });
  }
  return notes;
}

/**
 * The transcript (spec §15.2): the history's own items, unchanged, with
 * the session's runs and sends placed among and after them.
 *
 * - A run with committed messages gets its outcome after its last item.
 * - A finished run with nothing committed (it failed, stopped, or was
 *   interrupted before any message) sits where it happened: before the
 *   first history item whose first message was created after it.
 * - Runs still going, and finished runs this page saw stream (their
 *   messages are on their way), stay at the end, followed by the sends
 *   the daemon has not accepted yet.
 */
export function placeRuns(
  history: TranscriptHistory,
  input: LiveInput,
): TranscriptItem[] {
  const { lastOfRun, lastOfStep, stoppedRuns, startedAtMs } = history;
  const oldestLoaded = history.oldestLoadedMs;
  const olderHistory = input.olderHistory ?? true;
  // Older saved messages may predate durable origin metadata. Use an exact
  // trusted run link while that record remains available, never input text or
  // owner-selectable request-key prefixes.
  const recoveries = (input.runs ?? []).filter((live) => live.run.recoveryOf);
  const recoveryIds = new Set(recoveries.map((live) => live.run.id));
  const recoveryRequests = new Set(
    recoveries.flatMap((live) =>
      live.run.idempotencyKey ? [live.run.idempotencyKey] : [],
    ),
  );
  const items = history.items.map((item): TranscriptItem => {
    if (item.kind !== 'message' || item.message.role !== 'User') return item;
    const metadata = metadataOf(item.message);
    if (metadata.steer === true) return item;
    const runId = messageRunId(item.message);
    const requestId = stringField(metadata, 'clientRequestId');
    const recovered =
      runId !== null
        ? recoveryIds.has(runId)
        : requestId !== null && recoveryRequests.has(requestId);
    return recovered
      ? { kind: 'model-note', key: item.key, text: RECOVERY_WORK_NOTICE }
      : item;
  });
  /** The item a run created at `at` goes after. */
  const anchorFor = (at: number): number => {
    const next = startedAtMs.findIndex(
      (started) => started !== null && started > at,
    );
    return (next < 0 ? startedAtMs.length : next) - 1;
  };
  const inserts: RunInsert[] = [];
  const tail: TranscriptItem[] = [];
  for (const [order, live] of (input.runs ?? []).entries()) {
    const { run } = live;
    const outcome: TranscriptItem = {
      kind: 'outcome',
      key: `outcome:${run.id}`,
      run,
    };
    const after = lastOfRun.get(run.id);
    const notes = modelNotes(live);
    if (after !== undefined) {
      // Each note follows the model call it is about.
      for (const { stepId, note } of notes)
        inserts.push({
          after: lastOfStep.get(stepId) ?? after,
          order,
          rank: 0,
          items: [note],
        });
      const itemsForRun: TranscriptItem[] = [];
      if (
        hasOutcome(run) &&
        !(run.status === 'cancelled' && stoppedRuns.has(run.id))
      )
        itemsForRun.push(outcome);
      const hasLinkedReply = history.items.some(
        (item) =>
          item.kind === 'message' &&
          item.message.role === 'Assistant' &&
          item.message.id === run.replyMessageId &&
          messageRunId(item.message) === run.id,
      );
      if (canReviewCompletedRun(run, hasLinkedReply))
        itemsForRun.push({
          kind: 'learning-review',
          key: `learning-review:${run.id}`,
          run,
        });
      if (itemsForRun.length > 0)
        inserts.push({ after, order, rank: 1, items: itemsForRun });
      continue;
    }
    // A queued message the owner cancelled leaves nothing behind.
    if (run.status === 'cancelled' && run.startedAtMs === null) continue;
    const shown: TranscriptItem[] = [
      { kind: 'run', key: `run:${run.id}`, live },
      ...notes.map(({ note }) => note),
      ...(hasOutcome(run) ? [outcome] : []),
    ];
    const streamed = live.steps.length > 0 || live.tools.length > 0;
    if (!isTerminalRunStatus(run.status) || streamed) {
      tail.push(...shown);
      continue;
    }
    const loaded =
      oldestLoaded === null || run.createdAtMs >= oldestLoaded || !olderHistory;
    if (hasOutcome(run) && loaded)
      inserts.push({
        after: anchorFor(run.createdAtMs),
        order,
        rank: 1,
        items: shown,
      });
  }
  // Descending by anchor so an earlier splice never shifts a later one's
  // target index; descending by original order within a tie so splicing
  // (which pushes the most-recently-inserted item back) still lands the
  // ties in their original relative order (spec §15.2).
  inserts.sort(
    (left, right) =>
      right.after - left.after ||
      right.order - left.order ||
      right.rank - left.rank,
  );
  for (const insert of inserts)
    items.splice(insert.after + 1, 0, ...insert.items);
  items.push(...tail);
  for (const pending of input.pending ?? [])
    items.push({ kind: 'pending', key: `pending:${pending.key}`, pending });
  return items;
}

/** The session view's transcript (spec §15.2): history with tool steps
 *  grouped, then the runs and sends not yet in history. */
export function buildTranscript(input: TranscriptInput): TranscriptItem[] {
  return placeRuns(buildHistory(input), input);
}
