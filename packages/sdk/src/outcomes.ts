import type { DaemonClient } from './client.js';

export type OutcomeJson = null | boolean | number | string | OutcomeJson[] | { [key: string]: OutcomeJson };

/** Public checks request evidence; they never grant access or execution authority. */
export type OutcomeCriterion =
  | { kind: 'file_text'; id: string; path: string; contains: string }
  | { kind: 'file_sha256'; id: string; path: string; sha256: string }
  | { kind: 'file_json'; id: string; path: string; pointer: string; expected: OutcomeJson }
  | { kind: 'response_text'; id: string; contains: string }
  | { kind: 'document'; id: string; path: string; check: 'complete_rendering' | 'recalculated_without_errors' }
  | { kind: 'artifact_inspection'; id: string; reportSha256: string; locator: string; subjectSha256: string }
  | { kind: 'quality'; id: string; rubric: string };

export interface OutcomeDefinition {
  criteria: OutcomeCriterion[];
  /** 0–2 automatic corrections, still subject to existing execution limits. Default: 2. */
  maxCorrections?: number;
  /** Meeting criteria and owner acceptance remain separate. Default: report_only. */
  acceptance?: 'report_only' | 'owner_required';
}

export type OutcomeStatus =
  | 'pending' | 'checking' | 'correcting' | 'criteria_met' | 'awaiting_owner'
  | 'accepted' | 'needs_input' | 'needs_review' | 'exhausted' | 'stopped';

export interface OutcomeObservation {
  criterionId: string;
  verdict: 'met' | 'not_met' | 'inconclusive';
  subjectSha256?: string | null;
  reportSha256?: string | null;
  locator?: string | null;
  detail: string;
}

export interface OutcomeIteration {
  ordinal: number;
  bindingSha256: string;
  semanticSha256: string;
  replySha256: string;
  observations: OutcomeObservation[];
}

/** Daemon-owned evidence for an opt-in run; runtime completion alone is not a pass. */
export interface RunOutcome {
  definition: OutcomeDefinition;
  revision: number;
  rootRunId: string;
  taskSha256: string;
  sourceSha256: string;
  modelSha256: string;
  workspaceSha256: string;
  iterations: OutcomeIteration[];
  consumedCorrections: number;
  status: OutcomeStatus;
  reportSha256?: string | null;
  acceptedReportSha256?: string | null;
  repairPaths: string[];
  terminalReplySha256?: string | null;
}

export interface AcceptOutcomeInput {
  revision: number;
  reportSha256: string;
}

export class OutcomesClient {
  constructor(private readonly client: DaemonClient) {}

  async close(agentId: string, runId: string, input: { revision: number }): Promise<RunOutcome> {
    const response = await this.client.requestJson<{ outcome: RunOutcome }>(`${outcomePath(agentId, runId)}/close`, { method: 'POST', body: input });
    return response.outcome;
  }

  async get(agentId: string, runId: string, options: { signal?: AbortSignal } = {}): Promise<RunOutcome> {
    const response = await this.client.requestJson<{ outcome: RunOutcome }>(outcomePath(agentId, runId), { signal: options.signal });
    return response.outcome;
  }

  /** Accepts only the displayed revision and report. A stale report is a conflict. */
  async accept(agentId: string, runId: string, input: AcceptOutcomeInput): Promise<RunOutcome> {
    const response = await this.client.requestJson<{ outcome: RunOutcome }>(`${outcomePath(agentId, runId)}/accept`, { method: 'POST', body: input });
    return response.outcome;
  }
}

function outcomePath(agentId: string, runId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/runs/${encodeURIComponent(runId)}/outcome`;
}
