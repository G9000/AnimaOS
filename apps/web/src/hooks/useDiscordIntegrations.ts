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
  type DiscordConnectorSettings,
  type DiscordBotCheck,
} from '../lib/daemon-api';
import { safeDiscordError } from '../lib/discord';
import { useConnectorLifetime } from './useConnectorLifetime';

export const DISCORD_STATUS_POLL_MS = 10_000;
interface StatusRead {
  generation: number;
  revision: number;
  promise: Promise<void>;
}

/** Separate native Discord state; stale responses cannot cross owner selections. */
export function useDiscordIntegrations(
  agentId: string | null,
  active = false,
  online = true,
  epoch = 0,
) {
  const { token, isCurrent } = useConnectorLifetime(agentId, online, epoch);
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
  const [previousToken, setPreviousToken] = useState(token);
  if (previousToken !== token) {
    setPreviousToken(token);
    setOwner(agentId);
    setConnectors([]);
    setBusy(null);
    setError(null);
    setAvailable(false);
    setLoading(Boolean(agentId) && online);
  }
  useLayoutEffect(() => {
    lifetime.current++;
    mutation.current++;
    activeAgent.current = agentId;
    inFlight.current = false;
    statusRead.current = null;
    return () => {
      activeAgent.current = null;
    };
  }, [agentId, online, epoch]);
  const readStatus = useCallback(
    async (preserveError = false): Promise<void> => {
      if (
        !isCurrent(token) ||
        !agentId ||
        activeAgent.current !== agentId ||
        inFlight.current
      )
        return;
      const generation = lifetime.current;
      // Share current reads. Mutations invalidate older reads; wait for one
      // to settle before the required post-mutation read, never overlap them.
      while (statusRead.current?.generation === generation) {
        const previous = statusRead.current;
        await previous.promise;
        if (
          generation !== lifetime.current ||
          !isCurrent(token) ||
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
          if (
            !isCurrent(token) ||
            generation !== lifetime.current ||
            revision !== mutation.current
          )
            return;
          setConnectors(listed);
          setAvailable(true);
          if (!preserveError) setError(null);
        } catch (caught) {
          if (
            !isCurrent(token) ||
            generation !== lifetime.current ||
            revision !== mutation.current
          )
            return;
          setAvailable(false);
          setError((current) =>
            preserveError && current ? current : safeDiscordError(caught),
          );
        } finally {
          if (statusRead.current === claim) statusRead.current = null;
          if (generation === lifetime.current && isCurrent(token))
            setLoading(false);
        }
      })();
      await claim.promise;
    },
    [agentId, token, isCurrent],
  );
  useEffect(() => {
    void readStatus();
  }, [readStatus]);
  useEffect(() => {
    if (!online || !active || !agentId) return;
    void readStatus(true);
    const timer = setInterval(() => {
      if (!inFlight.current && statusRead.current === null)
        void readStatus(true);
    }, DISCORD_STATUS_POLL_MS);
    return () => clearInterval(timer);
  }, [active, agentId, online, readStatus]);
  const action = useCallback(
    async (
      kind: string,
      run: () => Promise<DiscordConnector | void>,
      removedId?: string,
    ) => {
      if (
        !isCurrent(token) ||
        !agentId ||
        activeAgent.current !== agentId ||
        inFlight.current
      )
        return false;
      const generation = lifetime.current;
      const revision = ++mutation.current;
      inFlight.current = true;
      setBusy(kind);
      setError(null);
      setAvailable(false);
      try {
        const connector = await run();
        if (
          !isCurrent(token) ||
          generation !== lifetime.current ||
          revision !== mutation.current
        )
          return false;
        setConnectors((current) =>
          connector
            ? [...current.filter((c) => c.id !== connector.id), connector]
            : current.filter((c) => c.id !== removedId),
        );
        return true;
      } catch (caught) {
        if (
          isCurrent(token) &&
          generation === lifetime.current &&
          revision === mutation.current
        )
          setError(safeDiscordError(caught));
        return false;
      } finally {
        if (
          isCurrent(token) &&
          generation === lifetime.current &&
          revision === mutation.current
        ) {
          inFlight.current = false;
          setBusy(null);
          // Failed requests may still commit lifecycle changes. Keep their
          // safe error while refreshing authoritative state.
          await readStatus(true);
        }
      }
    },
    [agentId, readStatus, token, isCurrent],
  );
  const check = async (botToken: string): Promise<DiscordBotCheck | null> => {
    if (
      !isCurrent(token) ||
      !agentId ||
      activeAgent.current !== agentId ||
      inFlight.current
    )
      return null;
    const generation = lifetime.current;
    const revision = ++mutation.current;
    inFlight.current = true;
    setBusy('check');
    setError(null);
    try {
      const checked = await daemon.checkDiscordBot(agentId, botToken);
      return isCurrent(token) &&
        generation === lifetime.current &&
        revision === mutation.current
        ? checked
        : null;
    } catch (caught) {
      if (
        isCurrent(token) &&
        generation === lifetime.current &&
        revision === mutation.current
      )
        setError(safeDiscordError(caught));
      return null;
    } finally {
      if (
        isCurrent(token) &&
        generation === lifetime.current &&
        revision === mutation.current
      ) {
        inFlight.current = false;
        setBusy(null);
        await readStatus(true);
      }
    }
  };
  return {
    connectors: owner === agentId && isCurrent(token) ? connectors : [],
    loading,
    available: owner === agentId && isCurrent(token) && available,
    busy,
    error,
    refresh: () => readStatus(),
    check,
    configure: (
      id: string,
      settings: DiscordConnectorSettings,
      approvedUserIds: string[],
    ) =>
      action('configure', () =>
        daemon.configureDiscordConnector(
          agentId!,
          id,
          settings,
          approvedUserIds,
        ),
      ),
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
