import type {
  LearningPolicy,
  LearningProposal,
  LearningReview,
  Run,
} from '@animaos/sdk';

/** A finished task is worth looking back at from this many tool calls. */
export const REVIEW_MIN_ACTIONS = 3;

/** What the review line under a finished task shows. In Free mode lessons
 *  are kept at once and each can be undone; in Strict mode they wait in the
 *  Inbox. A used budget is a quiet cost guard, never a dead button. */
export type TaskReviewView =
  | { kind: 'none' }
  | { kind: 'offer'; remaining: number | null; limit: number | null }
  | { kind: 'used'; nextAtMs: number | null }
  | { kind: 'reviewing' }
  | { kind: 'learned'; lessons: LearningProposal[] }
  | { kind: 'suggested'; count: number }
  | { kind: 'nothing' }
  | { kind: 'unfinished'; reason: string };

export function taskReviewView(
  review: LearningReview | undefined,
  policy: LearningPolicy | null,
): TaskReviewView {
  if (review) {
    // An automatic review that found nothing, or failed, stays silent.
    const manual = review.trigger === 'manual';
    switch (review.state) {
      case 'queued':
      case 'running':
        return { kind: 'reviewing' };
      case 'completed': {
        const kept = review.proposals.filter((p) => p.memoryId);
        if (kept.length > 0) return { kind: 'learned', lessons: kept };
        const waiting = review.proposals.filter(
          (p) => p.decision === 'pending',
        ).length;
        if (waiting > 0) return { kind: 'suggested', count: waiting };
        return review.proposals.length === 0 && manual
          ? { kind: 'nothing' }
          : { kind: 'none' };
      }
      case 'cancelled':
        return manual
          ? { kind: 'unfinished', reason: 'it was cancelled' }
          : { kind: 'none' };
      default:
        if (!manual) return { kind: 'none' };
        return {
          kind: 'unfinished',
          reason:
            review.error?.code === 'review_timeout'
              ? 'the model took too long'
              : 'something went wrong',
        };
    }
  }
  // Free mode reviews long tasks by itself; there is nothing to press.
  if (policy === null || policy.mode !== 'strict') return { kind: 'none' };
  if (policy.remaining === 0)
    return { kind: 'used', nextAtMs: policy.nextAvailableAtMs ?? null };
  return {
    kind: 'offer',
    remaining: policy.remaining ?? null,
    limit: policy.dailyLimit,
  };
}

/** Whether the daemon reviews this run by itself once it finishes. */
export function autoReviews(run: Run, policy: LearningPolicy | null): boolean {
  if (policy === null || (policy.mode === 'strict' && !policy.autoEnabled))
    return false;
  return (
    (run.actionReceipts?.length ?? 0) >= (policy.autoMinActions ?? Infinity)
  );
}

/** "3:42 PM", or "tomorrow at 3:42 PM", in the owner's own time zone. */
export function nextReviewTime(atMs: number, now = Date.now()): string {
  const at = new Date(atMs);
  const time = at.toLocaleTimeString([], {
    hour: 'numeric',
    minute: '2-digit',
  });
  return at.toDateString() === new Date(now).toDateString()
    ? time
    : `tomorrow at ${time}`;
}

/** The learning routes the chat's review lines use. */
export interface TaskReviewApi {
  policy(
    agentId: string,
    options?: { signal?: AbortSignal },
  ): Promise<LearningPolicy>;
  listReviews(
    agentId: string,
    options?: { signal?: AbortSignal },
  ): Promise<LearningReview[]>;
  requestReview(agentId: string, runId: string): Promise<LearningReview>;
  undoLesson(
    agentId: string,
    reviewId: string,
    proposalId: string,
    input: { expectedVersion: number },
  ): Promise<LearningReview>;
}
