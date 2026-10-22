import { useEffect, useState } from 'react';
import type { SessionContextUsage, UsageTotals } from '@animaos/sdk';

import { daemon } from '../lib/daemon-api';

/** A session's usage totals, read with the session record. The last value
 *  stays while the next read is in flight or fails (the header shows no
 *  error), but never carries over to another session. */
export function useSessionDetails(
  target: { agentId: string; sessionId: string } | null,
  refreshKey: number | string,
  enabled = true,
): {
  usage: UsageTotals | null;
  contextUsage: SessionContextUsage | undefined;
  settled: boolean;
} {
  const agentId = target?.agentId ?? null;
  const sessionId = target?.sessionId ?? null;
  const key = agentId && sessionId ? `${agentId}\n${sessionId}` : null;
  const [read, setRead] = useState<{
    key: string;
    usage: UsageTotals | null;
    contextUsage: SessionContextUsage | undefined;
  } | null>(null);

  useEffect(() => {
    if (!enabled || !agentId || !sessionId || !key) return;
    let current = true;
    daemon
      .getSession(agentId, sessionId)
      .then((session) => {
        if (!current) return;
        const usage = session.usage ?? null;
        const contextUsage = session.contextUsage;
        // Nothing new keeps the state, so a quiet header does not render.
        setRead((previous) =>
          previous
            ? previous.key === key &&
              JSON.stringify(previous.usage) === JSON.stringify(usage) &&
              JSON.stringify(previous.contextUsage) ===
                JSON.stringify(contextUsage)
              ? previous
              : { key, usage, contextUsage }
            : { key, usage, contextUsage },
        );
      })
      .catch(() => {
        if (!current) return;
        // Lifetime usage can remain useful, but a failed current read must
        // not present its previous context measurement as current.
        setRead((previous) => ({
          key,
          usage: previous?.key === key ? previous.usage : null,
          contextUsage: undefined,
        }));
      });
    return () => {
      current = false;
    };
  }, [agentId, sessionId, key, refreshKey, enabled]);

  const current = read?.key === key ? read : null;
  return {
    usage: current?.usage ?? null,
    contextUsage: enabled ? current?.contextUsage : undefined,
    settled: enabled && current !== null,
  };
}

/** Preserve the existing totals-only API for other consumers. */
export function useSessionUsage(
  target: { agentId: string; sessionId: string } | null,
  refreshKey: number,
): UsageTotals | null {
  return useSessionDetails(target, refreshKey).usage;
}
