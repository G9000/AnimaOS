import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Approval, LearningReview, Session } from '@animaOS-SWARM/sdk';

import type { DoneToday } from '../components/inbox/NeedsYouPanel';
import { daemon, type TelegramConnector } from '../lib/daemon-api';
import { doneActions, needsYouItems } from '../lib/needs-you';

/** Sessions read for "Done today": the most recently active ones. */
const MAX_DONE_SESSIONS = 20;

function startOfToday(now = new Date()): number {
  return new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
}

/** The "Needs you" inbox's data: what waits on the owner, live, and what
 *  the agent did today, read when asked for. */
export function useNeedsYou({
  agentId,
  online,
  approvals,
  telegram,
  sessions,
  refreshKey,
}: {
  agentId: string | null;
  online: boolean;
  approvals: readonly Approval[];
  telegram: readonly TelegramConnector[];
  sessions: readonly Session[];
  /** Changes when skills or the stream's epoch change: reviews are re-read. */
  refreshKey: string;
}) {
  const [reviews, setReviews] = useState<LearningReview[]>([]);
  const [done, setDone] = useState<DoneToday>({ state: 'loading' });
  const doneRequest = useRef(0);

  useEffect(() => {
    setReviews([]);
    if (!agentId || !online) return;
    const controller = new AbortController();
    daemon
      .listLearningReviews(agentId, { signal: controller.signal })
      .then(setReviews)
      // Reviews are optional here: Skills shows their errors.
      .catch(() => undefined);
    return () => controller.abort();
  }, [agentId, online, refreshKey]);

  const items = useMemo(
    () => needsYouItems({ approvals, telegram, reviews }),
    [approvals, telegram, reviews],
  );

  const sessionsRef = useRef(sessions);
  sessionsRef.current = sessions;
  const loadDone = useCallback(async () => {
    const request = ++doneRequest.current;
    if (!agentId || !online) {
      setDone({ state: 'error', message: 'Reconnect to see today’s activity.' });
      return;
    }
    setDone({ state: 'loading' });
    const since = startOfToday();
    const today = sessionsRef.current
      .filter((session) => session.lastActivityAtMs >= since)
      .sort((left, right) => right.lastActivityAtMs - left.lastActivityAtMs)
      .slice(0, MAX_DONE_SESSIONS);
    try {
      const runs = (
        await Promise.all(
          today.map((session) =>
            daemon.sessionRuns(session.agentId, session.id, { limit: 50 }),
          ),
        )
      )
        .flat()
        .filter((run) => (run.finishedAtMs ?? run.createdAtMs) >= since);
      if (request !== doneRequest.current) return;
      setDone({
        state: 'ready',
        actions: doneActions(runs).filter((action) => action.atMs >= since),
      });
    } catch {
      if (request !== doneRequest.current) return;
      setDone({
        state: 'error',
        message: 'Could not read today’s activity. Try again.',
      });
    }
  }, [agentId, online]);

  return { items, done, loadDone };
}
