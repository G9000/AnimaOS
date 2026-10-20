import type { DaemonClient } from './client.js';

export interface ClarificationChoice {
  id: string;
  label: string;
}
export interface ClarificationQuestion {
  id: string;
  question: string;
  choices: ClarificationChoice[];
  multiSelect: boolean;
}
export interface ClarificationAnswer {
  questionId: string;
  choiceIds: string[];
  /** Free text, including an answer to the Other option. */
  text: string | null;
}
export interface ClarificationResponse {
  answers: ClarificationAnswer[];
  answeredAtMs: number;
}
export interface ClarificationRequest {
  id: string;
  agentId: string;
  sessionId: string;
  runId: string;
  toolCallId: string;
  questions: ClarificationQuestion[];
  revision: number;
  createdAtMs: number;
  status: 'pending' | 'answered' | 'cancelled';
  response: ClarificationResponse | null;
}
export interface ClarificationAnswerInput {
  revision: number;
  answers: ClarificationAnswer[];
}
export interface SideQuestionAnswer {
  answer: string;
  snapshotAtMs: number;
  snapshotThroughMessageId: string | null;
}

/** Detached questions and versioned answers; neither endpoint grants tool authority. */
export class InteractionsClient {
  constructor(private readonly client: DaemonClient) {}

  sideQuestion(
    agentId: string,
    sessionId: string,
    question: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<SideQuestionAnswer> {
    return this.client.requestJson(
      `${sessionPath(agentId, sessionId)}/side-question`,
      {
        method: 'POST',
        body: { question },
        signal: options.signal,
      },
    );
  }

  async list(
    agentId: string,
    sessionId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<ClarificationRequest[]> {
    const result = await this.client.requestJson<{
      clarifications: ClarificationRequest[];
    }>(`${sessionPath(agentId, sessionId)}/clarifications`, {
      signal: options.signal,
    });
    return result.clarifications;
  }

  async answer(
    agentId: string,
    sessionId: string,
    requestId: string,
    input: ClarificationAnswerInput,
    options: { signal?: AbortSignal } = {},
  ): Promise<ClarificationRequest> {
    const result = await this.client.requestJson<{
      clarification: ClarificationRequest;
    }>(
      `${sessionPath(agentId, sessionId)}/clarifications/${encodeURIComponent(requestId)}/answer`,
      {
        method: 'POST',
        body: input,
        signal: options.signal,
      },
    );
    return result.clarification;
  }
}

function sessionPath(agentId: string, sessionId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}`;
}
