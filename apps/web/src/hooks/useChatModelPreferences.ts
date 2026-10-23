import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  ChatModels,
  ChatModelPreference,
  ChatModelPreferencesView,
  Session,
} from '@animaos/sdk';
import { useLatest } from './useLatest';
import { useRenderScope } from './useRenderScope';

interface PreferenceScope {
  active: boolean;
  nextIntent: number;
  lastAdoption: number;
  pending: number;
  writeVersion: number;
  readVersion: number;
  queue: Promise<void>;
}

export function useChatModelPreferences(
  session: Session | null,
  api: ChatModels | undefined,
  refreshKey: unknown,
) {
  const agentId = session?.agentId ?? '';
  const sessionId = session?.id ?? '';
  const scope = session
    ? `${session.agentId}\0${session.id}\0${session.clearGeneration ?? 0}`
    : '';
  const eligible = session?.kind === 'chat' && api !== undefined;
  const generation = session?.clearGeneration ?? 0;
  // Returning to the same chat after leaving it creates a new identity. An old
  // callback cannot become current merely because its string scope matches again.
  const context = useRenderScope(scope, eligible, api);
  const latestContext = useLatest(context);
  const operations = useRef(new WeakMap<object, PreferenceScope>());
  const stateFor = useCallback((identity: object): PreferenceScope => {
    let state = operations.current.get(identity);
    if (!state) {
      state = {
        active: true,
        nextIntent: 0,
        lastAdoption: 0,
        pending: 0,
        writeVersion: 0,
        readVersion: 0,
        queue: Promise.resolve(),
      };
      operations.current.set(identity, state);
    }
    return state;
  }, []);
  const [saved, setSaved] = useState<{
    context: typeof context;
    view: ChatModelPreferencesView;
  } | null>(null);
  const [activity, setActivity] = useState<{
    context: typeof context;
    busy: boolean;
    error: string | null;
  } | null>(null);
  const current = () =>
    eligible && stateFor(context).active && latestContext.current === context;
  const matchesGeneration = (view: ChatModelPreferencesView) =>
    view.clearGeneration === generation;
  useEffect(() => {
    stateFor(context).active = true;
    return () => {
      stateFor(context).active = false;
    };
  }, [context, stateFor]);
  useEffect(() => {
    if (!eligible || !api) return;
    let ignore = false;
    const requestVersion = ++stateFor(context).readVersion;
    const writeVersion = stateFor(context).writeVersion;
    const pendingAtStart = stateFor(context).pending !== 0;
    const controller = new AbortController();
    api
      .get(agentId, sessionId, { signal: controller.signal })
      .then((view) => {
        // Reads never invalidate writes. A GET that overlaps any mutation cannot
        // replace the mutation's authoritative response, even if it resolves last.
        if (
          !ignore &&
          stateFor(context).active &&
          latestContext.current === context &&
          view.clearGeneration === generation &&
          !pendingAtStart &&
          stateFor(context).pending === 0 &&
          stateFor(context).readVersion === requestVersion &&
          stateFor(context).writeVersion === writeVersion
        )
          setSaved({ context, view });
      })
      .catch(() => {
        /* The explicit controls report unavailable/old daemon errors. */
      });
    return () => {
      ignore = true;
      controller.abort();
    };
  }, [
    context,
    refreshKey,
    eligible,
    api,
    agentId,
    sessionId,
    generation,
    latestContext,
    stateFor,
  ]);
  function adopt(view: ChatModelPreferencesView) {
    if (!current() || !matchesGeneration(view)) return;
    stateFor(context).lastAdoption = ++stateFor(context).nextIntent;
    ++stateFor(context).writeVersion;
    setSaved({ context, view });
    setActivity({
      context,
      busy: stateFor(context).pending !== 0,
      error: null,
    });
  }
  async function update(primary: ChatModelPreference) {
    if (!session || !api || !current()) return;
    const intent = ++stateFor(context).nextIntent;
    const requestedPrimary = { ...primary };
    ++stateFor(context).pending;
    ++stateFor(context).writeVersion;
    setActivity({ context, busy: true, error: null });
    // The API has no conditional-write revision. Serial dispatch preserves the
    // owner's choice order at the server as well as in this hook's local state.
    const operation = stateFor(context).queue.then(async () => {
      try {
        if (!current() || intent < stateFor(context).lastAdoption) return;
        const view = await api.update(session.agentId, session.id, {
          primary: requestedPrimary,
          clearGeneration: generation,
        });
        if (
          current() &&
          intent >= stateFor(context).lastAdoption &&
          matchesGeneration(view)
        )
          setSaved({ context, view });
      } catch (caught) {
        if (current() && intent === stateFor(context).nextIntent)
          setActivity({
            context,
            busy: true,
            error: caught instanceof Error ? caught.message : String(caught),
          });
      } finally {
        --stateFor(context).pending;
        ++stateFor(context).writeVersion;
        if (current())
          setActivity((previous) => ({
            context,
            busy: stateFor(context).pending !== 0,
            error: previous?.context === context ? previous.error : null,
          }));
      }
    });
    stateFor(context).queue = operation.catch(() => undefined);
    await operation;
  }
  return {
    view: eligible && saved?.context === context ? saved.view : null,
    busy: activity?.context === context ? activity.busy : false,
    error: activity?.context === context ? activity.error : null,
    update,
    adopt,
  };
}
