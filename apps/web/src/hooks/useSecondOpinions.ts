import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { ChatModels, ChatReview } from '@animaos/sdk';
import { useLatest } from './useLatest';
import { useResetOnChange } from './useResetOnChange';

export type SecondOpinionApi = Pick<
  ChatModels,
  'reviews' | 'review' | 'cancelReview'
>;

/** What a reply shows and does about its second opinion. */
export interface SecondOpinions {
  /** The newest review of this reply, if any. */
  reviewFor(messageId: string): ChatReview | null;
  /** Asks the reviewer model about this reply. */
  request(messageId: string): void;
  cancel(reviewId: string): void;
  /** A review is starting or one is still running in this chat. */
  busy: boolean;
  error: string | null;
}

const errorText = (caught: unknown) =>
  caught instanceof Error ? caught.message : String(caught);

/**
 * Second opinions for one chat: another model reviews a saved reply on the
 * owner's request. Reviews are read once, and again every second while one
 * runs; `refreshKey` reads them again (a /review started elsewhere).
 */
export function useSecondOpinions(
  api: SecondOpinionApi | undefined,
  agentId: string,
  sessionId: string,
  clearGeneration: number,
  refreshKey: unknown = 0,
): SecondOpinions | undefined {
  const [reviews, setReviews] = useState<ChatReview[]>([]);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scope = `${agentId}\0${sessionId}\0${clearGeneration}`;
  const current = useLatest(scope);
  const requests = useRef(new Map<string, string>());
  useResetOnChange([scope], () => {
    setReviews([]);
    setStarting(false);
    setError(null);
    requests.current = new Map();
  });
  const running = reviews.some((row) => row.status === 'running');

  useEffect(() => {
    if (!api || !sessionId) return;
    let live = true;
    const controller = new AbortController();
    const read = () =>
      api.reviews(agentId, sessionId, { signal: controller.signal }).then(
        (rows) => {
          if (live) {
            setReviews(rows);
            setError(null);
          }
        },
        (caught: unknown) => {
          if (live && !controller.signal.aborted) setError(errorText(caught));
        },
      );
    void read();
    const timer = running ? setInterval(() => void read(), 1000) : null;
    return () => {
      live = false;
      if (timer) clearInterval(timer);
      controller.abort();
    };
  }, [api, agentId, sessionId, clearGeneration, running, refreshKey]);

  const request = useCallback(
    (messageId: string) => {
      if (!api) return;
      const asked = scope;
      // A retry of the same reply reuses its request id, so the daemon
      // starts it once.
      const requestId =
        requests.current.get(messageId) ?? crypto.randomUUID();
      requests.current.set(messageId, requestId);
      setStarting(true);
      setError(null);
      void api
        .review(agentId, sessionId, { requestId, messageId, clearGeneration })
        .then(
          (row) => {
            if (current.current !== asked) return;
            requests.current.delete(messageId);
            setReviews((rows) => [
              ...rows.filter((old) => old.id !== row.id),
              row,
            ]);
          },
          (caught: unknown) => {
            if (current.current === asked) setError(errorText(caught));
          },
        )
        .finally(() => {
          if (current.current === asked) setStarting(false);
        });
    },
    [api, agentId, sessionId, clearGeneration, scope, current],
  );

  const cancel = useCallback(
    (reviewId: string) => {
      if (!api) return;
      const asked = scope;
      void api.cancelReview(agentId, sessionId, reviewId).then(
        (row) => {
          if (current.current === asked)
            setReviews((rows) =>
              rows.map((old) => (old.id === reviewId ? row : old)),
            );
        },
        (caught: unknown) => {
          if (current.current === asked) setError(errorText(caught));
        },
      );
    },
    [api, agentId, sessionId, scope, current],
  );

  const latest = useMemo(() => {
    const byMessage = new Map<string, ChatReview>();
    for (const row of reviews) {
      const seen = byMessage.get(row.sourceMessageId);
      if (!seen || seen.createdAtMs <= row.createdAtMs)
        byMessage.set(row.sourceMessageId, row);
    }
    return byMessage;
  }, [reviews]);
  const reviewFor = useCallback(
    (messageId: string) => latest.get(messageId) ?? null,
    [latest],
  );

  return useMemo(
    () =>
      api
        ? { reviewFor, request, cancel, busy: starting || running, error }
        : undefined,
    [api, reviewFor, request, cancel, starting, running, error],
  );
}
