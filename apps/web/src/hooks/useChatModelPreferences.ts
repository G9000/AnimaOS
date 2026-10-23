import { useEffect, useMemo, useRef, useState } from 'react';
import type { ChatModels, ChatModelPreference, ChatModelPreferencesView, Session } from '@animaos/sdk';

interface PreferenceScope {
  active: boolean;
  nextIntent: number;
  lastAdoption: number;
  pending: number;
  writeVersion: number;
  readVersion: number;
  queue: Promise<void>;
}

export function useChatModelPreferences(session: Session | null, api: ChatModels | undefined, refreshKey: unknown) {
  const agentId = session?.agentId ?? '';
  const sessionId = session?.id ?? '';
  const scope = session ? `${session.agentId}\0${session.id}\0${session.clearGeneration ?? 0}` : '';
  const eligible = session?.kind === 'chat' && api !== undefined;
  const generation = session?.clearGeneration ?? 0;
  // Returning to the same chat after leaving it creates a new identity. An old
  // callback cannot become current merely because its string scope matches again.
  const context = useMemo<PreferenceScope>(() => ({ active: true, nextIntent: 0, lastAdoption: 0, pending: 0, writeVersion: 0, readVersion: 0, queue: Promise.resolve() }), [scope, eligible, api]);
  const latestContext = useRef(context);
  latestContext.current = context;
  const [saved, setSaved] = useState<{ context: PreferenceScope; view: ChatModelPreferencesView } | null>(null);
  const [activity, setActivity] = useState<{ context: PreferenceScope; busy: boolean; error: string | null } | null>(null);
  const current = () => eligible && context.active && latestContext.current === context;
  const matchesGeneration = (view: ChatModelPreferencesView) => view.clearGeneration === generation;
  useEffect(() => {
    context.active = true;
    return () => { context.active = false; };
  }, [context]);
  useEffect(() => {
    if (!eligible || !api) return;
    let ignore = false;
    const requestVersion = ++context.readVersion;
    const writeVersion = context.writeVersion;
    const pendingAtStart = context.pending !== 0;
    const controller = new AbortController();
    api.get(agentId, sessionId, { signal: controller.signal }).then(view => {
      // Reads never invalidate writes. A GET that overlaps any mutation cannot
      // replace the mutation's authoritative response, even if it resolves last.
      if (!ignore && context.active && latestContext.current === context && view.clearGeneration === generation && !pendingAtStart && context.pending === 0 && context.readVersion === requestVersion && context.writeVersion === writeVersion) setSaved({ context, view });
    }).catch(() => { /* The explicit controls report unavailable/old daemon errors. */ });
    return () => { ignore = true; controller.abort(); };
  }, [context, refreshKey, eligible, api, agentId, sessionId, generation]);
  function adopt(view: ChatModelPreferencesView) {
    if (!current() || !matchesGeneration(view)) return;
    context.lastAdoption = ++context.nextIntent;
    ++context.writeVersion;
    setSaved({ context, view });
    setActivity({ context, busy: context.pending !== 0, error: null });
  }
  async function update(primary: ChatModelPreference) {
    if (!session || !api || !current()) return;
    const intent = ++context.nextIntent;
    const requestedPrimary = { ...primary };
    ++context.pending;
    ++context.writeVersion;
    setActivity({ context, busy: true, error: null });
    // The API has no conditional-write revision. Serial dispatch preserves the
    // owner's choice order at the server as well as in this hook's local state.
    const operation = context.queue.then(async () => {
      try {
        if (!current() || intent < context.lastAdoption) return;
        const view = await api.update(session.agentId, session.id, { primary: requestedPrimary, clearGeneration: generation });
        if (current() && intent >= context.lastAdoption && matchesGeneration(view)) setSaved({ context, view });
      } catch (caught) {
        if (current() && intent === context.nextIntent) setActivity({ context, busy: true, error: caught instanceof Error ? caught.message : String(caught) });
      } finally {
        --context.pending;
        ++context.writeVersion;
        if (current()) setActivity(previous => ({ context, busy: context.pending !== 0, error: previous?.context === context ? previous.error : null }));
      }
    });
    context.queue = operation.catch(() => undefined);
    await operation;
  }
  return { view: eligible && saved?.context === context ? saved.view : null, busy: activity?.context === context ? activity.busy : false, error: activity?.context === context ? activity.error : null, update, adopt };
}
