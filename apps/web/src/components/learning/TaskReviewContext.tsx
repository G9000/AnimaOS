import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import type {
  LearningPolicy,
  LearningProposal,
  LearningReview,
  Run,
} from '@animaos/sdk';

import { daemon } from '../../lib/daemon-api';
import { learningReviewsChanged } from '../../lib/inbox-events';
import type { TaskReviewApi } from '../../lib/task-reviews';

/** How often a queued or running review is re-read until it settles. */
const POLL_MS = 2_000;

const daemonApi: TaskReviewApi = {
  policy: (agentId, options) => daemon.learningPolicy(agentId, options),
  listReviews: (agentId, options) =>
    daemon.listLearningReviews(agentId, options),
  requestReview: (agentId, runId) =>
    daemon.requestLearningReview(agentId, runId),
  undoLesson: (agentId, reviewId, proposalId, input) =>
    daemon.undoLearningLesson(agentId, reviewId, proposalId, input),
};

export interface TaskReviews {
  /** `null` until read, or when the daemon could not say. */
  policy: LearningPolicy | null;
  reviewFor(runId: string): LearningReview | undefined;
  /** Resolves to a readable error, or `null` when the review started or the
   *  budget turned out to be used (the line then says so by itself). */
  request(run: Run): Promise<string | null>;
  /** Forgets a kept lesson; resolves to a readable error or `null`. */
  undo(
    review: LearningReview,
    lesson: LearningProposal,
  ): Promise<string | null>;
  /** Re-reads soon: an automatic review starts just after its task. */
  refreshSoon(): void;
}

const Context = createContext<TaskReviews | null>(null);

export const useTaskReviews = () => useContext(Context);

function pending(review: LearningReview): boolean {
  return review.state === 'queued' || review.state === 'running';
}

/** One read of the review budget and reviews for every line in a transcript. */
export function TaskReviewProvider({
  agentId,
  active,
  api = daemonApi,
  children,
}: {
  agentId: string;
  /** The transcript shows a review line; nothing is read otherwise. */
  active: boolean;
  api?: TaskReviewApi;
  children: ReactNode;
}) {
  const [policy, setPolicy] = useState<LearningPolicy | null>(null);
  const [reviews, setReviews] = useState<LearningReview[]>([]);
  const [tick, setTick] = useState(0);
  const apiRef = useRef(api);
  apiRef.current = api;
  /** Reviews seen queued or running, so their settling can be told. */
  const running = useRef(new Set<string>());

  const refresh = useCallback(() => setTick((value) => value + 1), []);

  useEffect(() => {
    setPolicy(null);
    setReviews([]);
    running.current = new Set();
  }, [agentId]);

  useEffect(() => {
    if (!active) return;
    const controller = new AbortController();
    const { signal } = controller;
    void Promise.allSettled([
      apiRef.current.policy(agentId, { signal }),
      apiRef.current.listReviews(agentId, { signal }),
    ]).then(([nextPolicy, nextReviews]) => {
      if (signal.aborted) return;
      if (nextPolicy.status === 'fulfilled') setPolicy(nextPolicy.value);
      if (nextReviews.status === 'fulfilled') setReviews(nextReviews.value);
    });
    return () => controller.abort();
  }, [agentId, active, tick]);

  // Keep re-reading while a review runs, and tell the Inbox when one settles.
  useEffect(() => {
    let changed = false;
    for (const review of reviews)
      if (pending(review)) running.current.add(review.id);
      else if (running.current.delete(review.id)) changed = true;
    if (changed) learningReviewsChanged();
    if (!active || !reviews.some(pending)) return;
    const timer = window.setTimeout(refresh, POLL_MS);
    return () => window.clearTimeout(timer);
  }, [reviews, active, refresh]);

  const soon = useRef<number | null>(null);
  const refreshSoon = useCallback(() => {
    if (soon.current !== null) return;
    soon.current = window.setTimeout(() => {
      soon.current = null;
      refresh();
    }, POLL_MS);
  }, [refresh]);
  useEffect(
    () => () => {
      if (soon.current !== null) window.clearTimeout(soon.current);
    },
    [],
  );

  const request = useCallback(
    async (run: Run): Promise<string | null> => {
      try {
        const review = await apiRef.current.requestReview(agentId, run.id);
        setReviews((current) => [
          review,
          ...current.filter((item) => item.id !== review.id),
        ]);
        refresh();
        return null;
      } catch (cause) {
        let next: LearningPolicy | null = null;
        try {
          next = await apiRef.current.policy(agentId);
          setPolicy(next);
        } catch {
          // The original error is the one to show.
        }
        if (next?.remaining === 0) return null;
        const message = cause instanceof Error ? cause.message : '';
        return message
          ? `Couldn’t review this task: ${message}`
          : 'Couldn’t review this task. Try again in a moment.';
      }
    },
    [agentId, refresh],
  );

  const undo = useCallback(
    async (
      review: LearningReview,
      lesson: LearningProposal,
    ): Promise<string | null> => {
      try {
        const next = await apiRef.current.undoLesson(
          agentId,
          review.id,
          lesson.id,
          { expectedVersion: lesson.decisionVersion },
        );
        setReviews((current) =>
          current.map((item) => (item.id === next.id ? next : item)),
        );
        learningReviewsChanged();
        return null;
      } catch (cause) {
        refresh();
        const message = cause instanceof Error ? cause.message : '';
        return message
          ? `Couldn’t undo that lesson: ${message}`
          : 'Couldn’t undo that lesson. Try again in a moment.';
      }
    },
    [agentId, refresh],
  );

  const value = useMemo<TaskReviews>(() => {
    const bySource = new Map<string, LearningReview>();
    for (const review of reviews)
      if (!bySource.has(review.sourceRunId))
        bySource.set(review.sourceRunId, review);
    return {
      policy,
      reviewFor: (runId) => bySource.get(runId),
      request,
      undo,
      refreshSoon,
    };
  }, [policy, reviews, request, undo, refreshSoon]);

  return <Context.Provider value={value}>{children}</Context.Provider>;
}
