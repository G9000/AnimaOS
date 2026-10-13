import type { DaemonClient } from './client.js';
import type { RunTokenUsage } from './runs.js';

export type LearningReviewTrigger = 'manual' | 'automatic';
export type LearningReviewState =
  | 'queued'
  | 'running'
  | 'completed'
  | 'cancelled'
  | 'interrupted'
  | 'failed';
export type LearningProposalDecision = 'pending' | 'dismissed' | 'acknowledged';

export interface LearningProposal {
  id: string;
  observation: string;
  suggestion: string;
  anchors: string[];
  decision: LearningProposalDecision;
  decisionVersion: number;
}

export interface LearningReview {
  id: string;
  agentId: string;
  sourceRunId: string;
  sessionId: string;
  trigger: LearningReviewTrigger;
  policyVersion: number;
  evidenceDigest: string;
  evidenceBrief: string;
  evidenceAnchors: string[];
  state: LearningReviewState;
  reservedAtMs: number;
  startedAtMs?: number | null;
  finishedAtMs?: number | null;
  model: string;
  provider?: string | null;
  usage?: RunTokenUsage | null;
  usageComplete: boolean;
  error?: { code: string; message: string } | null;
  proposals: LearningProposal[];
}

export interface LearningPolicy {
  autoEnabled: boolean;
  dailyLimit: number;
  pendingLimit: number;
  timeoutMs: number;
  /** The transport may remain active after its returned-result deadline. */
  physicalReviewerActive?: boolean;
}

export type LearningDecision = Extract<
  LearningProposalDecision,
  'dismissed' | 'acknowledged'
>;

export class LearningClient {
  constructor(private readonly client: DaemonClient) {}

  async policy(
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<LearningPolicy> {
    return this.client.requestJson<LearningPolicy>(
      `${agentPath(agentId)}/learning-policy`,
      { signal: options.signal },
    );
  }

  async setPolicy(
    agentId: string,
    input: { autoEnabled: boolean },
  ): Promise<LearningPolicy> {
    return this.client.requestJson<LearningPolicy>(
      `${agentPath(agentId)}/learning-policy`,
      { method: 'PATCH', body: input },
    );
  }

  async requestReview(agentId: string, runId: string): Promise<LearningReview> {
    const response = await this.client.requestJson<{ review: LearningReview }>(
      `${agentPath(agentId)}/runs/${encodeURIComponent(runId)}/learning-review`,
      { method: 'POST', body: {} },
    );
    return response.review;
  }

  async listReviews(
    agentId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<LearningReview[]> {
    const response = await this.client.requestJson<{
      reviews: LearningReview[];
    }>(`${agentPath(agentId)}/learning-reviews`, { signal: options.signal });
    return response.reviews;
  }

  async getReview(
    agentId: string,
    reviewId: string,
    options: { signal?: AbortSignal } = {},
  ): Promise<LearningReview> {
    const response = await this.client.requestJson<{ review: LearningReview }>(
      `${agentPath(agentId)}/learning-reviews/${encodeURIComponent(reviewId)}`,
      { signal: options.signal },
    );
    return response.review;
  }

  async cancelReview(
    agentId: string,
    reviewId: string,
  ): Promise<LearningReview> {
    const response = await this.client.requestJson<{ review: LearningReview }>(
      `${agentPath(agentId)}/learning-reviews/${encodeURIComponent(reviewId)}/cancel`,
      { method: 'POST', body: {} },
    );
    return response.review;
  }

  async decideProposal(
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: { decision: LearningDecision; expectedVersion: number },
  ): Promise<LearningReview> {
    const response = await this.client.requestJson<{ review: LearningReview }>(
      `${agentPath(agentId)}/learning-reviews/${encodeURIComponent(reviewId)}/proposals/${encodeURIComponent(proposalId)}`,
      { method: 'PATCH', body: input },
    );
    return response.review;
  }
}

function agentPath(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}`;
}
