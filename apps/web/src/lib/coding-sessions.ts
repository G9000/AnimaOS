// Tool results from app sessions (a terminal app the agent drives) and ACP
// sessions (a coding agent driven over the Agent Client Protocol). The daemon
// returns them as JSON; anything that does not read as one falls back to the
// plain result.

export type AppSessionStatus = 'running' | 'exited' | 'stopped';

export interface AppSessionView {
  kind: 'app_session';
  id: string;
  command: string;
  cwd: string;
  status: AppSessionStatus;
  exitCode: number | null;
  startedAtMs: number | null;
  rows: string[];
  output: string;
  outcome: string | null;
}

export type AcpSessionStatus =
  | 'running'
  | 'idle'
  | 'waiting_for_you'
  | 'exited'
  | 'stopped';

export type AcpStepStatus = 'pending' | 'in_progress' | 'completed' | 'failed';

export interface AcpPlanEntry {
  content: string;
  status: 'pending' | 'in_progress' | 'completed';
}

export interface AcpStep {
  id: string;
  title: string;
  toolKind: string;
  status: AcpStepStatus;
  /** The owner's answer when the coding agent asked first. */
  decision: 'allowed' | 'denied' | 'cancelled' | null;
}

export interface AcpSessionView {
  kind: 'acp_session';
  id: string;
  command: string;
  cwd: string;
  agentName: string | null;
  status: AcpSessionStatus;
  stopReason: string | null;
  plan: AcpPlanEntry[];
  steps: AcpStep[];
  /** What the coding agent said last. */
  message: string;
  /** A permission it asked for that has no answer yet. */
  waitingOn: { title: string; toolKind: string } | null;
}

export type CodingSessionView = AppSessionView | AcpSessionView;

const APP_STATUSES: readonly string[] = ['running', 'exited', 'stopped'];
const ACP_STATUSES: readonly string[] = [
  'running',
  'idle',
  'waiting_for_you',
  'exited',
  'stopped',
];
const STEP_STATUSES: readonly string[] = [
  'pending',
  'in_progress',
  'completed',
  'failed',
];
const PLAN_STATUSES: readonly string[] = ['pending', 'in_progress', 'completed'];
const DECISIONS: readonly string[] = ['allowed', 'denied', 'cancelled'];

type Json = Record<string, unknown>;

function isObject(value: unknown): value is Json {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function text(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function oneOf<T extends string>(
  value: unknown,
  allowed: readonly string[],
): T | null {
  return typeof value === 'string' && allowed.includes(value)
    ? (value as T)
    : null;
}

function parseTime(value: unknown): number | null {
  if (typeof value !== 'string') return null;
  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : null;
}

function parseApp(json: Json): AppSessionView | null {
  const status = oneOf<AppSessionStatus>(json.status, APP_STATUSES);
  if (typeof json.id !== 'string' || !json.id || !status) return null;
  const screen = isObject(json.screen) ? json.screen : null;
  const rows = Array.isArray(screen?.rows)
    ? screen.rows.filter((row): row is string => typeof row === 'string')
    : [];
  // Trailing blank rows are the unused part of the terminal.
  while (rows.length > 0 && rows[rows.length - 1].trim() === '') rows.pop();
  return {
    kind: 'app_session',
    id: json.id,
    command: text(json.command),
    cwd: text(json.cwd),
    status,
    exitCode: typeof json.exitCode === 'number' ? json.exitCode : null,
    startedAtMs: parseTime(json.startedAt),
    rows,
    output: text(json.output),
    outcome: typeof json.outcome === 'string' ? json.outcome : null,
  };
}

function parseAcp(json: Json): AcpSessionView | null {
  const status = oneOf<AcpSessionStatus>(json.status, ACP_STATUSES);
  if (typeof json.id !== 'string' || !json.id || !status) return null;
  const events = Array.isArray(json.events) ? json.events.filter(isObject) : [];
  let plan: AcpPlanEntry[] = [];
  const steps = new Map<string, AcpStep>();
  const asked = new Map<string, { title: string; toolKind: string }>();
  let message: string[] = [];
  let messageOpen = false;
  for (const event of events) {
    switch (event.type) {
      case 'agent_message':
        if (!messageOpen) message = [];
        message.push(text(event.text));
        messageOpen = true;
        continue;
      case 'agent_thought':
        // A thought between message chunks does not end the message.
        continue;
      case 'plan':
        plan = (Array.isArray(event.entries) ? event.entries : [])
          .filter(isObject)
          .map((entry) => ({
            content: text(entry.content),
            status:
              oneOf<AcpPlanEntry['status']>(entry.status, PLAN_STATUSES) ??
              'pending',
          }))
          .filter((entry) => entry.content.trim() !== '');
        break;
      case 'tool_call': {
        const id = text(event.id);
        if (!id) break;
        steps.set(id, {
          id,
          title: text(event.title) || 'A step',
          toolKind: text(event.toolKind),
          status:
            oneOf<AcpStepStatus>(event.status, STEP_STATUSES) ?? 'pending',
          decision: steps.get(id)?.decision ?? null,
        });
        break;
      }
      case 'tool_call_update': {
        const known = steps.get(text(event.id));
        const next = oneOf<AcpStepStatus>(event.status, STEP_STATUSES);
        if (known && next) steps.set(known.id, { ...known, status: next });
        break;
      }
      case 'permission_asked':
        asked.set(text(event.toolCallId), {
          title: text(event.title) || 'A step',
          toolKind: text(event.toolKind),
        });
        break;
      case 'permission_answered': {
        const id = text(event.toolCallId);
        asked.delete(id);
        const decision = oneOf<NonNullable<AcpStep['decision']>>(
          event.decision,
          DECISIONS,
        );
        const known = steps.get(id);
        if (known && decision) steps.set(id, { ...known, decision });
        break;
      }
      default:
        break;
    }
    messageOpen = false;
  }
  const agent = isObject(json.agent) ? json.agent : null;
  const waiting = [...asked.values()].at(-1) ?? null;
  return {
    kind: 'acp_session',
    id: json.id,
    command: text(json.command),
    cwd: text(json.cwd),
    agentName: text(agent?.name).trim() || null,
    status,
    stopReason: typeof json.stopReason === 'string' ? json.stopReason : null,
    plan,
    steps: [...steps.values()],
    message: message.join('').trim(),
    waitingOn: status === 'exited' || status === 'stopped' ? null : waiting,
  };
}

/** A coding agent's later snapshot on top of an earlier one: each read only
 *  carries what happened since the last, so plan, steps and the last message
 *  carry over until something newer replaces them. */
export function mergeAcpSnapshots(
  earlier: AcpSessionView,
  later: AcpSessionView,
): AcpSessionView {
  const steps = new Map(earlier.steps.map((step) => [step.id, step]));
  for (const step of later.steps) {
    const known = steps.get(step.id);
    steps.set(step.id, { ...step, decision: step.decision ?? known?.decision ?? null });
  }
  return {
    ...later,
    agentName: later.agentName ?? earlier.agentName,
    plan: later.plan.length > 0 ? later.plan : earlier.plan,
    steps: [...steps.values()],
    message: later.message || earlier.message,
  };
}

/** An app or ACP session snapshot from a tool result, or null when the
 *  result is something else, cut short or malformed. */
export function parseCodingSession(
  result: string | null,
): CodingSessionView | null {
  if (!result) return null;
  const trimmed = result.trimStart();
  if (!trimmed.startsWith('{')) return null;
  let json: unknown;
  try {
    json = JSON.parse(trimmed);
  } catch {
    return null;
  }
  if (!isObject(json)) return null;
  if (json.kind === 'app_session') return parseApp(json);
  if (json.kind === 'acp_session') return parseAcp(json);
  return null;
}

/** "~/code/uploader" for a path under a home folder. */
export function shortPath(path: string): string {
  return path.replace(/^(?:\/Users|\/home)\/[^/]+(?=\/|$)/, '~');
}

/** "2:14" or "1:02:14". */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, '0');
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, '0')}:${seconds}`
    : `${minutes}:${seconds}`;
}
