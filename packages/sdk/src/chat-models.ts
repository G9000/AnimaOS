import type { DaemonClient } from './client.js';

export type ChatReasoningEffort = 'none' | 'minimal' | 'low' | 'medium' | 'high' | 'xhigh' | 'max';
export type ChatModelPreference = { mode: 'inherit' } | {
  mode: 'override'; provider: string; model: string; reasoningEffort?: ChatReasoningEffort;
};
export interface ChatModelPreferences { primary: ChatModelPreference; reviewer: ChatModelPreference }
export interface ResolvedChatModel { provider: string; model: string; reasoningEffort?: ChatReasoningEffort }
export interface ChatModelOption extends ResolvedChatModel { reasoningEfforts: ChatReasoningEffort[] }
export interface ChatModelPreferencesView {
  preferences: ChatModelPreferences;
  primary: ResolvedChatModel;
  reviewer: ResolvedChatModel | null;
  reviewerUnavailableReason: string | null;
  availableModels: ChatModelOption[];
  appliesTo: 'next_turn';
  clearGeneration: number;
}
export type ChatReviewStatus = 'running' | 'completed' | 'failed' | 'cancelled' | 'unavailable' | 'interrupted';
export interface ChatReview {
  id: string; requestId: string; sourceMessageId: string; sourceRunId: string | null;
  evidenceDigest: string; focus: string | null; reviewer: ResolvedChatModel | null;
  status: ChatReviewStatus; createdAtMs: number; finishedAtMs: number | null;
  findings: string | null; error: string | null; evidenceLimited: boolean;
}
export interface ChatReviewInput { messageId?: string; focus?: string; requestId: string; clearGeneration?: number }

/** Owner controls scoped to one conversation. Reviews never update the primary model. */
export class ChatModels {
  constructor(private readonly client: DaemonClient) {}
  private base(agentId: string, sessionId: string): string {
    return `/api/agents/${encodeURIComponent(agentId)}/sessions/${encodeURIComponent(sessionId)}`;
  }
  get(agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<ChatModelPreferencesView> {
    return this.client.requestJson(`${this.base(agentId, sessionId)}/model-preferences`, { signal: options.signal });
  }
  update(agentId: string, sessionId: string, preferences: Partial<ChatModelPreferences> & { clearGeneration?: number }): Promise<ChatModelPreferencesView> {
    return this.client.requestJson(`${this.base(agentId, sessionId)}/model-preferences`, { method: 'PATCH', body: preferences });
  }
  async reviews(agentId: string, sessionId: string, options: { signal?: AbortSignal } = {}): Promise<ChatReview[]> {
    const result = await this.client.requestJson<{ reviews: ChatReview[] }>(`${this.base(agentId, sessionId)}/reviews`, { signal: options.signal });
    return result.reviews;
  }
  async review(agentId: string, sessionId: string, input: ChatReviewInput): Promise<ChatReview> {
    const result = await this.client.requestJson<{ review: ChatReview }>(`${this.base(agentId, sessionId)}/reviews`, { method: 'POST', body: input });
    return result.review;
  }
  async cancelReview(agentId: string, sessionId: string, reviewId: string): Promise<ChatReview> {
    const result = await this.client.requestJson<{ review: ChatReview }>(`${this.base(agentId, sessionId)}/reviews/${encodeURIComponent(reviewId)}/cancel`, { method: 'POST' });
    return result.review;
  }
}
