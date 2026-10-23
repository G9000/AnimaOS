import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import type { Run } from '@animaos/sdk';

import { daemon } from '../lib/daemon-api';
import { useLatest } from './useLatest';

/** Ledger runs a session view reads for outcomes its stream no longer
 *  carries: failures, stops, and restarts (spec §4.1, §15.2). */
export const SESSION_RUNS_LIMIT = 20;

const NO_RUNS: Run[] = [];

function httpStatus(error: unknown): unknown {
  return typeof error === 'object' && error !== null && 'status' in error
    ? error.status
    : undefined;
}

export interface SessionLedger {
  /** The session's recent runs, newest first. */
  runs: Run[];
  /** The number (in `started` order) of the read on screen; null until
   *  this session's first read lands. */
  landed: number | null;
  /** Reads begun so far, for any session: a read numbered above this
   *  began after now. */
  started: () => number;
  /** Reads the ledger directly without an intermediate render. */
  refresh: () => Promise<void>;
}

/** The session's ledger runs and which read they came from; read again
 *  when `refreshKey` or the session changes. A failed read keeps what the
 *  view shows, and the newest read wins. */
export function useSessionLedger(
  agentId: string | null,
  sessionId: string | null,
  refreshKey = 0,
): SessionLedger {
  const key = agentId && sessionId ? `${agentId}\u0000${sessionId}` : null;
  const [loaded, setLoaded] = useState<{
    key: string;
    runs: Run[];
    landed: number;
  } | null>(null);
  const readsRef = useRef(0);
  const active = useRef<AbortController | null>(null);
  const targetRef = useLatest({ key, agentId, sessionId });
  const started = useCallback(() => readsRef.current, []);
  const refresh = useCallback(async () => {
    const { key, agentId, sessionId } = targetRef.current;
    if (!key || !agentId || !sessionId) return;
    const read = ++readsRef.current;
    // A read the view no longer wants (another session opened, a newer
    // read began, or the view closed) is aborted, not just ignored (T20).
    const controller = new AbortController();
    active.current?.abort();
    active.current = controller;
    try {
      const runs = await daemon.sessionRuns(agentId, sessionId, {
        limit: SESSION_RUNS_LIMIT,
        signal: controller.signal,
      });
      if (!controller.signal.aborted) setLoaded({ key, runs, landed: read });
    } catch (caught) {
      // A daemon without the route, or a deleted session, has none.
      if (!controller.signal.aborted && httpStatus(caught) === 404)
        setLoaded({ key, runs: [], landed: read });
    }
  }, [targetRef]);
  useEffect(() => {
    // Updates follow the awaited daemon response, including its failure.
    // eslint-disable-next-line react/set-state-in-effect
    void refresh();
    return () => {
      active.current?.abort();
    };
  }, [key, refresh, refreshKey]);
  return useMemo(
    () =>
      loaded && loaded.key === key
        ? { runs: loaded.runs, landed: loaded.landed, started, refresh }
        : { runs: NO_RUNS, landed: null, started, refresh },
    [loaded, key, started, refresh],
  );
}

/** The session's recent runs, newest first; read again when `refreshKey`
 *  changes. A failed read keeps what the view shows. */
export function useSessionRuns(
  agentId: string | null,
  sessionId: string | null,
  refreshKey = 0,
): Run[] {
  return useSessionLedger(agentId, sessionId, refreshKey).runs;
}
