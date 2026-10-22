import type {
  AgentEvent,
  AgentSummary,
  Approval,
  ApprovalDecisionInput,
  ClarificationRequest,
  ClarificationAnswerInput,
  Run,
  Session,
  SessionMessagePage,
  SessionPage,
  StartRunInput,
  StartRunResult,
} from '@animaos/sdk';

export interface TuiSessionApi {
  review?: TuiReviewApi;
  listAgents(signal?: AbortSignal): Promise<AgentSummary[]>;
  listSessions(
    agentId: string,
    cursor?: string,
    signal?: AbortSignal,
  ): Promise<SessionPage>;
  createSession(agentId: string, title?: string): Promise<Session>;
  getMessages(
    agentId: string,
    sessionId: string,
    before?: string,
    signal?: AbortSignal,
  ): Promise<SessionMessagePage>;
  listRuns(
    agentId: string,
    sessionId: string,
    signal?: AbortSignal,
  ): Promise<Run[]>;
  startRun(
    agentId: string,
    sessionId: string,
    input: StartRunInput,
    idempotencyKey: string,
  ): Promise<StartRunResult>;
  stopRun(agentId: string, runId: string): Promise<Run>;
  streamEvents(agentId: string, signal: AbortSignal): AsyncIterable<AgentEvent>;
}

/** Owner review operations; execution and authorization remain in the daemon. */
export interface TuiReviewApi {
  listPendingApprovals(signal?: AbortSignal): Promise<Approval[]>;
  decideApproval(id: string, input: ApprovalDecisionInput): Promise<Approval>;
  listClarifications(
    agentId: string,
    sessionId: string,
    signal?: AbortSignal,
  ): Promise<ClarificationRequest[]>;
  answerClarification(
    agentId: string,
    sessionId: string,
    id: string,
    input: ClarificationAnswerInput,
  ): Promise<ClarificationRequest>;
}
