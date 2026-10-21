import type { DaemonClient } from './client.js';
import type { BrowserSelection } from './browser.js';

export type RunStatus =
  | 'queued'
  | 'running'
  | 'awaiting_approval'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted';

export type RunSource =
  | 'web'
  | 'api'
  | 'telegram'
  | 'discord'
  | 'email'
  | 'schedule'
  | 'job'
  | 'delegation'
  | 'peer';

export interface RunTokenUsage {
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
}

/** A bounded tool intent/outcome record. Started does not prove dispatch or effects. */
export interface ActionReceipt {
  toolCallId: string;
  name: string;
  status: 'started' | 'succeeded' | 'failed';
  startedAtMs: number;
  finishedAtMs: number | null;
  durationMs: number | null;
  argumentsPreview: string;
  argumentsTruncated: boolean;
  resultPreview: string | null;
  resultTruncated: boolean;
  durability: 'persistent' | 'memory';
}

/** Durable completion disposition. Acceptance and queueing do not prove execution. */
export type HelperCompletionDisposition =
  | { state: 'manual' }
  | { state: 'awaiting_result'; reservedRunId: string }
  | { state: 'awaiting_parent'; reservedRunId: string }
  | { state: 'queued'; runId: string }
  | { state: 'settled'; runId: string }
  | { state: 'review_required'; reason: string; runId?: string }
  | { state: 'suppressed'; reason: string; runId?: string };

/** Response-only helper lineage; private launch identities and steer content are omitted. */
export interface AsyncHelperSummary {
  ownerAgentId: string;
  ownerSessionId: string;
  parentRunId: string;
  completion: HelperCompletionDisposition;
  pendingSteerCount: number;
  archiveUnconsumedCount: number;
}

/** Typed host provenance for evidence returned to the original companion chat. */
export interface HelperCompletionOrigin {
  helperRunId: string;
  launchParentRunId: string;
}

export interface RunRecoverySummary {
  sourceRunId: string;
  fingerprint: string;
  eligible: boolean;
  reviewRequired: boolean;
  unknownActionCount: number;
  remainingAttempts: number;
  continuationRunId: string | null;
}

export interface ContinueRecoveryInput {
  fingerprint: string;
  reviewed?: boolean;
}

/** A ledger run (spec §4.1). */
export interface Run {
  id: string;
  agentId: string;
  sessionId: string;
  source: RunSource;
  sourceRef: string | null;
  /** The `Idempotency-Key` the run was accepted or started with, so a client
   *  matches its own sends; `null` for a run without one. */
  idempotencyKey: string | null;
  status: RunStatus;
  input: { text: string; attachmentIds: string[]; skill: string | null };
  createdAtMs: number;
  startedAtMs: number | null;
  finishedAtMs: number | null;
  error: { code: string; message: string } | null;
  stop: { requestedAtMs: number } | null;
  toolsStarted: string[];
  steps: {
    stepId: string;
    usage: RunTokenUsage;
    /** The provider and model that answered, when a fallback did instead of
     *  the run's own; absent otherwise. */
    provider?: string;
    model?: string;
    /** Why the run's own model did not answer, e.g. "was rate limited". */
    fallbackReason?: string;
  }[];
  usage: RunTokenUsage;
  model: string;
  provider: string | null;
  parentRunId: string | null;
  recovery?: {
    fingerprint: string;
    continuationRunId: string;
    reviewedAtMs: number;
  };
  recoveryOf?: string;
  recoveryAttempt?: number;
  /** The committed final reply once the run completed. */
  replyMessageId: string | null;
  /** Native action witnesses, absent on runs saved before receipt support. */
  actionReceipts?: ActionReceipt[];
  /** Absent for legacy runs and ordinary owner turns. */
  asyncHelper?: AsyncHelperSummary;
  /** Generated helper evidence, never a newly authored owner request. */
  helperCompletion?: HelperCompletionOrigin;
}

/** `queue` waits behind the session's earlier messages; `steer` joins its
 *  active run before the next model call (spec §4.2, §4.7). */
export type RunMode = 'queue' | 'steer';

export interface StartRunInput {
  text: string;
  attachmentIds?: string[];
  skill?: string;
  mode?: RunMode;
  /** Plan mode: the agent shows its plan and waits for a go before a job
   *  that changes things. Left out, the message is a Do message. */
  plan?: boolean;
  /** Explicit owner opt-in for a new immediately eligible Chat run; replay never renews it. */
  browserSelection?: BrowserSelection;
  /** The sender's IANA time zone; the daemon keeps it as the owner's, so
   *  "7 p.m." means their evening in every channel. */
  timeZone?: string;
}

export interface StartRunResult {
  run: Run;
  /** Present when the message joined the session's active run. */
  steer?: { status: 'pending' };
}

const TERMINAL: ReadonlySet<RunStatus> = new Set([
  'completed',
  'failed',
  'cancelled',
  'interrupted',
]);

export function isTerminalRunStatus(status: RunStatus): boolean {
  return TERMINAL.has(status);
}

export class RunsClient {
  constructor(private readonly client: DaemonClient) {}

  /** Inspect immutable source evidence before authorizing one linked continuation. */
  recovery(
    agentId: string,
    runId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<RunRecoverySummary> {
    return this.client.requestJson<RunRecoverySummary>(
      `${runPath(agentId, runId)}/recovery`,
      { signal: options.signal },
    );
  }

  /** Exact repeats return the saved continuation. Current grants apply to the new run. */
  async continueRecovery(
    agentId: string,
    runId: string,
    input: ContinueRecoveryInput,
  ): Promise<Run> {
    const response = await this.client.requestJson<{ run: Run }>(
      `${runPath(agentId, runId)}/recovery`,
      { method: 'POST', body: input },
    );
    return response.run;
  }

  /** Accepts a message into a session (spec §4.2). The same key within 24
   *  hours returns the original run and creates nothing. */
  async start(
    agentId: string,
    sessionId: string,
    input: StartRunInput,
    options: { idempotencyKey: string; signal?: AbortSignal },
  ): Promise<StartRunResult> {
    return this.client.requestJson<StartRunResult>(
      `${sessionPath(agentId, sessionId)}/runs`,
      {
        method: 'POST',
        body: input,
        headers: { 'idempotency-key': options.idempotencyKey },
        signal: options.signal,
      },
    );
  }

  /** Stops a run (spec §4.6); stopping a finished run changes nothing. */
  async stop(agentId: string, runId: string): Promise<Run> {
    const response = await this.client.requestJson<{ run: Run }>(
      `${runPath(agentId, runId)}/stop`,
      { method: 'POST' },
    );
    return response.run;
  }

  async get(
    agentId: string,
    runId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<Run> {
    const response = await this.client.requestJson<{ run: Run }>(
      runPath(agentId, runId),
      { signal: options.signal },
    );
    return response.run;
  }

  /** The session's runs the daemon's ledger holds, newest first. */
  async listForSession(
    agentId: string,
    sessionId: string,
    options: { limit?: number; signal?: AbortSignal } = {},
  ): Promise<Run[]> {
    const query =
      options.limit !== undefined ? `?limit=${String(options.limit)}` : '';
    const response = await this.client.requestJson<{ runs: Run[] }>(
      `${sessionPath(agentId, sessionId)}/runs${query}`,
      { signal: options.signal },
    );
    return response.runs;
  }
}

function sessionPath(agentId: string, sessionId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}`;
}

function runPath(agentId: string, runId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/runs/${encodeURIComponent(runId)}`;
}
