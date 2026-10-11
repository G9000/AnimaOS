import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import {
  daemon,
  type DiscordConnector,
  type CreateDiscordConnectorInput,
} from '../lib/daemon-api';
import { safeDiscordError } from '../lib/discord';

export const DISCORD_STATUS_POLL_MS = 10_000;
interface StatusRead {
  generation: number;
  revision: number;
  promise: Promise<void>;
}

/** Separate native Discord state; stale responses cannot cross owner selections. */
export function useDiscordIntegrations(agentId: string | null, active = false) {
  const lifetime = useRef(0);
  const mutation = useRef(0);
  const activeAgent = useRef(agentId);
  const statusRead = useRef<StatusRead | null>(null);
  const inFlight = useRef(false);
  const [owner, setOwner] = useState(agentId);
  const [connectors, setConnectors] = useState<DiscordConnector[]>([]);
  const [loading, setLoading] = useState(Boolean(agentId));
  const [available, setAvailable] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  useLayoutEffect(() => {
    lifetime.current++;
    mutation.current++;
    activeAgent.current = agentId;
    inFlight.current = false;
    statusRead.current = null;
    setOwner(agentId);
    setConnectors([]);
    setBusy(null);
    setError(null);
    setAvailable(false);
    setLoading(Boolean(agentId));
    return () => {
      lifetime.current++;
      mutation.current++;
      activeAgent.current = null;
    };
  }, [agentId]);
  const readStatus = useCallback(
    async (preserveError = false): Promise<void> => {
      if (!agentId || activeAgent.current !== agentId || inFlight.current)
        return;
      const generation = lifetime.current;
      // Share current reads. Mutations invalidate older reads; wait for one
      // to settle before the required post-mutation read, never overlap them.
      while (statusRead.current?.generation === generation) {
        const previous = statusRead.current;
        await previous.promise;
        if (
          generation !== lifetime.current ||
          activeAgent.current !== agentId ||
          inFlight.current
        )
          return;
        if (previous.revision === mutation.current) return;
      }
      const revision = mutation.current;
      const claim: StatusRead = {
        generation,
        revision,
        promise: Promise.resolve(),
      };
      statusRead.current = claim;
      setLoading(true);
      claim.promise = (async () => {
        try {
          const listed = await daemon.discordConnectors(agentId);
          if (generation !== lifetime.current || revision !== mutation.current)
            return;
          setConnectors(listed);
          setAvailable(true);
          if (!preserveError) setError(null);
        } catch (caught) {
          if (generation !== lifetime.current || revision !== mutation.current)
            return;
          setAvailable(false);
          setError((current) =>
            preserveError && current ? current : safeDiscordError(caught),
          );
        } finally {
          if (statusRead.current === claim) statusRead.current = null;
          if (generation === lifetime.current) setLoading(false);
        }
      })();
      await claim.promise;
    },
    [agentId],
  );
  useEffect(() => {
    void readStatus();
  }, [readStatus]);
  useEffect(() => {
    if (!active || !agentId) return;
    void readStatus(true);
    const timer = setInterval(() => {
      if (!inFlight.current && statusRead.current === null)
        void readStatus(true);
    }, DISCORD_STATUS_POLL_MS);
    return () => clearInterval(timer);
  }, [active, agentId, readStatus]);
  const action = useCallback(
    async (
      kind: string,
      run: () => Promise<DiscordConnector | void>,
      removedId?: string,
    ) => {
      if (!agentId || activeAgent.current !== agentId || inFlight.current)
        return false;
      const generation = lifetime.current;
      const revision = ++mutation.current;
      inFlight.current = true;
      setBusy(kind);
      setError(null);
      setAvailable(false);
      try {
        const connector = await run();
        if (generation !== lifetime.current || revision !== mutation.current)
          return false;
        setConnectors((current) =>
          connector
            ? [...current.filter((c) => c.id !== connector.id), connector]
            : current.filter((c) => c.id !== removedId),
        );
        return true;
      } catch (caught) {
        if (generation === lifetime.current && revision === mutation.current)
          setError(safeDiscordError(caught));
        return false;
      } finally {
        if (generation === lifetime.current && revision === mutation.current) {
          inFlight.current = false;
          setBusy(null);
          // Failed requests may still commit lifecycle changes. Keep their
          // safe error while refreshing authoritative state.
          await readStatus(true);
        }
      }
    },
    [agentId, readStatus],
  );
  return {
    connectors: owner === agentId ? connectors : [],
    loading,
    available: owner === agentId && available,
    busy,
    error,
    refresh: () => readStatus(),
    create: (input: CreateDiscordConnectorInput) =>
      action('create', () => daemon.createDiscordConnector(agentId!, input)),
    activate: (id: string) =>
      action('activate', () => daemon.activateDiscordConnector(agentId!, id)),
    replace: (id: string, token: string) =>
      action('replace', () =>
        daemon.replaceDiscordCredential(agentId!, id, token),
      ),
    setEnabled: (id: string, enabled: boolean) =>
      action('enabled', () => daemon.setDiscordEnabled(agentId!, id, enabled)),
    restart: (id: string) =>
      action('restart', () => daemon.restartDiscordConnector(agentId!, id)),
    retry: (id: string) =>
      action('retry', () => daemon.retryDiscordDelivery(agentId!, id)),
    suppress: (id: string) =>
      action('suppress', () =>
        daemon.suppressDiscordUnknownDelivery(agentId!, id),
      ),
    disconnect: (id: string) =>
      action(
        'disconnect',
        () => daemon.deleteDiscordConnector(agentId!, id),
        id,
      ),
  };
}
