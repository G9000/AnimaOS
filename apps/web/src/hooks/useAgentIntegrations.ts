import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';

import { daemon, type TelegramConnector } from '../lib/daemon-api';
import { safeIntegrationError } from '../lib/telegram';
import { useConnectorLifetime } from './useConnectorLifetime';

type ConnectorBusy =
  'connect' | 'replace' | 'approve' | 'restart' | 'disconnect' | null;

/** The companion's connectors and their setup actions (Connectors page). */
export function useAgentIntegrations(
  agentId: string | null,
  online = true,
  epoch = 0,
) {
  const { token, isCurrent } = useConnectorLifetime(agentId, online, epoch);
  const generation = useRef(0);
  const connectorMutation = useRef(0);
  const connectorBusyRef = useRef(false);
  const [owner, setOwner] = useState(agentId);
  const [connectors, setConnectors] = useState<TelegramConnector[]>([]);
  const [loading, setLoading] = useState(Boolean(agentId));
  const [connectorBusy, setConnectorBusy] = useState<ConnectorBusy>(null);
  const [connectorError, setConnectorError] = useState<string | null>(null);
  const [previousToken, setPreviousToken] = useState(token);
  if (previousToken !== token) {
    setPreviousToken(token);
    setOwner(agentId);
    setConnectors([]);
    setConnectorBusy(null);
    setConnectorError(null);
    setLoading(Boolean(agentId) && online);
  }

  useLayoutEffect(() => {
    generation.current += 1;
    connectorMutation.current += 1;
    connectorBusyRef.current = false;
  }, [agentId, online, epoch]);

  const refresh = useCallback(async () => {
    if (!isCurrent(token) || !agentId || connectorBusyRef.current) return;
    const requestGeneration = generation.current;
    const connectorRequest = connectorMutation.current;
    try {
      const { connectors: listed } = await daemon.listConnectors(agentId);
      if (
        requestGeneration !== generation.current ||
        !isCurrent(token) ||
        connectorRequest !== connectorMutation.current
      )
        return;
      setConnectors(listed);
      setConnectorError(null);
    } catch (caught) {
      if (
        requestGeneration !== generation.current ||
        !isCurrent(token) ||
        connectorRequest !== connectorMutation.current
      )
        return;
      setConnectorError(safeIntegrationError(caught));
    } finally {
      if (requestGeneration === generation.current && isCurrent(token))
        setLoading(false);
    }
  }, [agentId, token, isCurrent]);
  const refreshWithLoading = useCallback(() => {
    setLoading(true);
    return refresh();
  }, [refresh]);

  useEffect(() => {
    // State changes only after the awaited connector response, including failures.
    // eslint-disable-next-line react/set-state-in-effect
    void refresh();
  }, [refresh]);

  const connectorAction = useCallback(
    async <T>(
      kind: Exclude<ConnectorBusy, null>,
      action: () => Promise<T>,
      apply: (value: T) => void,
    ) => {
      if (!isCurrent(token) || !agentId || connectorBusyRef.current)
        return false;
      const lifetime = generation.current;
      const mutation = ++connectorMutation.current;
      setConnectorBusy(kind);
      connectorBusyRef.current = true;
      setConnectorError(null);
      try {
        const value = await action();
        if (
          lifetime !== generation.current ||
          !isCurrent(token) ||
          mutation !== connectorMutation.current
        )
          return false;
        apply(value);
        return true;
      } catch (error) {
        if (
          lifetime === generation.current &&
          isCurrent(token) &&
          mutation === connectorMutation.current
        ) {
          setConnectorError(safeIntegrationError(error));
        }
        return false;
      } finally {
        if (
          lifetime === generation.current &&
          isCurrent(token) &&
          mutation === connectorMutation.current
        ) {
          connectorBusyRef.current = false;
          setConnectorBusy(null);
        }
      }
    },
    [agentId, token, isCurrent],
  );

  const upsertConnector = (connector: TelegramConnector) =>
    setConnectors((current) => [
      ...current.filter((item) => item.id !== connector.id),
      connector,
    ]);

  const connectTelegram = useCallback(
    (token: string) =>
      connectorAction(
        'connect',
        () => daemon.createTelegramConnector(agentId!, token),
        ({ connector }) => upsertConnector(connector),
      ),
    [agentId, connectorAction],
  );
  const replaceTelegram = useCallback(
    (connectorId: string, token: string) =>
      connectorAction(
        'replace',
        () => daemon.replaceTelegramCredential(agentId!, connectorId, token),
        ({ connector }) => upsertConnector(connector),
      ),
    [agentId, connectorAction],
  );
  const approvePairing = useCallback(
    (connectorId: string, chatId: string) =>
      connectorAction(
        'approve',
        () => daemon.approveTelegramPairing(agentId!, connectorId, chatId),
        ({ connector }) => upsertConnector(connector),
      ),
    [agentId, connectorAction],
  );
  const restartTelegram = useCallback(
    (connectorId: string) =>
      connectorAction(
        'restart',
        () => daemon.restartTelegramConnector(agentId!, connectorId),
        ({ connector }) => upsertConnector(connector),
      ),
    [agentId, connectorAction],
  );
  const disconnectTelegram = useCallback(
    (connectorId: string) =>
      connectorAction(
        'disconnect',
        () => daemon.deleteTelegramConnector(agentId!, connectorId),
        () =>
          setConnectors((current) =>
            current.filter((item) => item.id !== connectorId),
          ),
      ),
    [agentId, connectorAction],
  );

  const visible = owner === agentId && isCurrent(token);
  return {
    connectors: visible ? connectors : [],
    loading,
    connectorBusy,
    connectorError,
    refresh: refreshWithLoading,
    connectTelegram,
    replaceTelegram,
    approvePairing,
    restartTelegram,
    disconnectTelegram,
  };
}
