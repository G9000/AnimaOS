import type { DaemonClient } from './client.js';

export type Calculation =
  | { op: 'input'; key: string }
  | { op: 'constant'; value: number }
  | {
      op: 'add' | 'subtract' | 'multiply' | 'divide';
      left: Calculation;
      right: Calculation;
    };
export interface CalculatorInput {
  key: string;
  label: string;
  value: number;
  min?: number;
  max?: number;
}
export type ChatResultPayload =
  | {
      kind: 'chart';
      chartType: 'bar' | 'line';
      labels: string[];
      series: { label: string; values: number[] }[];
    }
  | { kind: 'comparison'; columns: string[]; rows: string[][] }
  | { kind: 'document'; format: 'text' | 'markdown'; text: string }
  | {
      kind: 'calculator';
      inputs: CalculatorInput[];
      expression: Calculation;
      outputLabel: string;
    };

/** Display data and source provenance, never tool execution authority. */
export interface ChatResult {
  id: string;
  revision: number;
  agentId: string;
  sessionId: string;
  clearGeneration: number;
  title: string;
  payload: ChatResultPayload;
  pinned: boolean;
  createdAtMs: number;
  updatedAtMs: number;
  sourceRunId: string;
  sourceToolCallId: string;
  requestKey: string;
}
export interface SaveChatResultInput {
  id?: string;
  expectedRevision: number;
  requestKey: string;
  title: string;
  payload: ChatResultPayload;
}
export interface ChatResultsResponse {
  clearGeneration: number;
  results: ChatResult[];
}
export interface ChatResultResponse {
  result: ChatResult;
}
export interface PinChatResultInput {
  clearGeneration: number;
  expectedRevision: number;
  pinned: boolean;
}

export class ChatResultsClient {
  constructor(private readonly client: DaemonClient) {}
  list(
    agentId: string,
    sessionId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<ChatResultsResponse> {
    return this.client.requestJson(path(agentId, sessionId), options);
  }
  save(
    agentId: string,
    sessionId: string,
    clearGeneration: number,
    result: SaveChatResultInput,
  ): Promise<ChatResultResponse> {
    return this.client.requestJson(path(agentId, sessionId), {
      method: 'POST',
      body: { clearGeneration, result },
    });
  }
  pin(
    agentId: string,
    sessionId: string,
    id: string,
    input: PinChatResultInput,
  ): Promise<ChatResultResponse> {
    return this.client.requestJson(
      `${path(agentId, sessionId)}/${encodeURIComponent(id)}`,
      { method: 'PATCH', body: input },
    );
  }
}
function path(agentId: string, sessionId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}/results`;
}
