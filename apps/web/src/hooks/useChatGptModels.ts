import { useCallback, useEffect, useRef, useState } from 'react';
import {
  createDaemonClient,
  type ChatGptModelsSnapshot,
} from '@animaos/sdk';

const client = createDaemonClient({ baseUrl: '' }).chatgpt;
const readError =
  'Model metadata is unavailable. You can enter a custom model.';
const refreshError =
  'Model metadata refresh could not be confirmed. Availability is unverified.';

/** Local picker state only. Mount/reconciliation GETs never trigger an upstream refresh. */
export function useChatGptModels(scopeKey: string, enabled = true) {
  const context = useRef({ scopeKey, enabled });
  if (
    context.current.scopeKey !== scopeKey ||
    context.current.enabled !== enabled
  ) {
    context.current = { scopeKey, enabled };
  }
  const owner = context.current;
  const sequence = useRef(0);
  const active = useRef<AbortController | null>(null);
  const refreshing = useRef(false);
  const [result, setResult] = useState<{
    owner: typeof owner;
    snapshot: ChatGptModelsSnapshot | null;
    busy: boolean;
    error: string | null;
  } | null>(null);
  const [expired, setExpired] = useState<ChatGptModelsSnapshot | null>(null);

  const request = useCallback(
    async (refresh: boolean) => {
      if (
        !enabled ||
        context.current !== owner ||
        (refresh && refreshing.current)
      )
        return;
      const version = ++sequence.current;
      active.current?.abort();
      const controller = new AbortController();
      active.current = controller;
      refreshing.current = refresh;
      const current = () =>
        context.current === owner &&
        sequence.current === version &&
        !controller.signal.aborted;
      setResult((previous) => ({
        owner,
        snapshot: previous?.owner === owner ? previous.snapshot : null,
        busy: true,
        error: null,
      }));
      try {
        const snapshot = refresh
          ? await client.refreshModels({ signal: controller.signal })
          : await client.models({ signal: controller.signal });
        if (current()) setResult({ owner, snapshot, busy: false, error: null });
      } catch {
        if (!current()) return;
        if (refresh) {
          // Reconcile a possibly completed mutation with GET; never replay POST.
          setResult({ owner, snapshot: null, busy: true, error: refreshError });
          try {
            const snapshot = await client.models({ signal: controller.signal });
            if (current())
              setResult({ owner, snapshot, busy: false, error: refreshError });
          } catch {
            if (current())
              setResult({
                owner,
                snapshot: null,
                busy: false,
                error: refreshError,
              });
          }
        } else {
          setResult({ owner, snapshot: null, busy: false, error: readError });
        }
      } finally {
        if (context.current === owner && sequence.current === version) {
          refreshing.current = false;
          active.current = null;
        }
      }
    },
    [enabled, owner],
  );

  useEffect(() => {
    refreshing.current = false;
    if (enabled) void request(false);
    return () => {
      sequence.current += 1;
      active.current?.abort();
      active.current = null;
      refreshing.current = false;
    };
  }, [enabled, request]);

  const snapshot = result?.owner === owner && enabled ? result.snapshot : null;
  useEffect(() => {
    if (
      !snapshot ||
      (snapshot.state !== 'ready' && snapshot.state !== 'empty') ||
      snapshot.expiresAtMs === null
    )
      return;
    const timer = window.setTimeout(
      () => setExpired(snapshot),
      Math.max(0, Math.min(300_000, snapshot.expiresAtMs - Date.now())),
    );
    return () => window.clearTimeout(timer);
  }, [snapshot]);
  return {
    snapshot:
      snapshot && expired === snapshot
        ? { ...snapshot, state: 'stale' as const }
        : snapshot,
    busy: enabled && (result?.owner !== owner || result?.busy === true),
    error: result?.owner === owner ? result.error : null,
    refresh: () => request(true),
    /** Re-reads the snapshot without contacting ChatGPT, e.g. after the
     *  account was connected somewhere this picker did not see. */
    reload: () => request(false),
  };
}
