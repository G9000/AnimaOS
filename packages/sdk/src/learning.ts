import type { DaemonClient } from './client.js';
import type { SkillDraft } from './skills.js';
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
/** `free`: long tasks are reviewed by themselves and lessons are kept at
 *  once, with Undo. `strict`: every lesson waits for the owner. */
export type LearningMode = 'free' | 'strict';

export interface LearningProposal {
  id: string;
  observation: string;
  suggestion: string;
  anchors: string[];
  decision: LearningProposalDecision;
  decisionVersion: number;
  skillDraftId?: string | null;
  /** The memory a Free-mode lesson was kept as; Undo forgets it. */
  memoryId?: string | null;
}

export interface LearningSkillDraftInput {
  expectedVersion: number;
  evidenceDigest: string;
  skill: {
    slug: string;
    name: string;
    description: string;
    body: string;
    expectedHash: string | null;
  };
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
  mode?: LearningMode;
  autoEnabled: boolean;
  dailyLimit: number;
  pendingLimit: number;
  timeoutMs: number;
  /** The transport may remain active after its returned-result deadline. */
  physicalReviewerActive?: boolean;
  /** The rolling window `dailyLimit` applies to (24 hours). */
  windowMs?: number;
  /** Workspace-wide reviews still available in the current window. */
  remaining?: number;
  /** When the next review becomes available; `null` while one is. */
  nextAvailableAtMs?: number | null;
  /** Automatic review only follows tasks with this many tool calls. */
  autoMinActions?: number;
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

  /** Forgets a kept Free-mode lesson: its memory is deleted, the proposal
   *  dismissed. */
  async undoLesson(
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: { expectedVersion: number },
  ): Promise<LearningReview> {
    const response = await this.client.requestJson<{ review: LearningReview }>(
      `${agentPath(agentId)}/learning-reviews/${encodeURIComponent(reviewId)}/proposals/${encodeURIComponent(proposalId)}/undo`,
      { method: 'POST', body: input },
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

  async createSkillDraft(
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: LearningSkillDraftInput,
  ): Promise<SkillDraft> {
    const response = await this.client.requestJson<{ draft: SkillDraft }>(
      `${agentPath(agentId)}/learning-reviews/${encodeURIComponent(reviewId)}/proposals/${encodeURIComponent(proposalId)}/skill-draft`,
      { method: 'POST', body: input },
    );
    return response.draft;
  }
}

function agentPath(agentId: string): string {
  return `/api/agents/${encodeURIComponent(agentId)}`;
}
