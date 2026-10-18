import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import {
  daemon,
  type DriveStatus,
  type ConnectDriveInput,
} from '../lib/daemon-api';
import {
  googleDriveConsentUrl,
  INVALID_DRIVE_CONSENT,
  safeDriveError,
} from '../lib/drive-selection';
import { useConnectorLifetime } from './useConnectorLifetime';

export const DRIVE_CONSENT_POLL_MS = 15_000;
const MAX_CONSENT_POLLS = 40;
interface StatusRead {
  generation: number;
  revision: number;
  promise: Promise<void>;
}

/** Mounted only on the Drive setup surface. Pending consent polling is bounded. */
export function useDriveIntegrations(
  agentId: string | null,
  online = true,
  epoch = 0,
  disabled = false,
) {
  const scope = useConnectorLifetime(agentId, online, epoch);
  const allowed = useRef(!disabled);
  allowed.current = !disabled;
  const lifetime = useRef(0);
  const revision = useRef(0);
  const activeAgent = useRef(agentId);
  const locked = useRef(false);
  const read = useRef<StatusRead | null>(null);
  const [owner, setOwner] = useState(agentId);
  const [status, setStatus] = useState<DriveStatus | null>(null);
  const [consentUrl, setConsentUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(Boolean(agentId));
  const [available, setAvailable] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useLayoutEffect(() => {
    lifetime.current++;
    revision.current++;
    activeAgent.current = agentId;
    locked.current = false;
    read.current = null;
    setOwner(agentId);
    setStatus(null);
    setConsentUrl(null);
    setLoading(Boolean(agentId) && online);
    setAvailable(false);
    setBusy(false);
    setError(null);
    return () => {
      lifetime.current++;
      revision.current++;
      activeAgent.current = null;
    };
  }, [agentId, online, epoch]);

  const refreshStatus = useCallback(
    async (preserveError = false): Promise<void> => {
      if (
        !scope.isCurrent(scope.token) ||
        !agentId ||
        activeAgent.current !== agentId ||
        locked.current
      )
        return;
      const generation = lifetime.current;
      while (read.current?.generation === generation) {
        const previous = read.current;
        await previous.promise;
        if (
          generation !== lifetime.current ||
          !scope.isCurrent(scope.token) ||
          activeAgent.current !== agentId ||
          locked.current
        )
          return;
        if (previous.revision === revision.current) return;
      }
      const requestRevision = revision.current;
      const claim: StatusRead = {
        generation,
        revision: requestRevision,
        promise: Promise.resolve(),
      };
      read.current = claim;
      setLoading(true);
      claim.promise = (async () => {
        try {
          const next = await daemon.driveStatus(agentId);
          if (
            generation !== lifetime.current ||
            !scope.isCurrent(scope.token) ||
            requestRevision !== revision.current
          )
            return;
          setStatus(next);
          setAvailable(true);
          if (next.connector?.status !== 'pairing') setConsentUrl(null);
          if (!preserveError) setError(null);
        } catch (caught) {
          if (
            generation !== lifetime.current ||
            !scope.isCurrent(scope.token) ||
            requestRevision !== revision.current
          )
            return;
          setAvailable(false);
          setConsentUrl(null);
          setError((current) =>
            preserveError && current ? current : safeDriveError(caught),
          );
        } finally {
          if (read.current === claim) read.current = null;
          if (generation === lifetime.current && scope.isCurrent(scope.token))
            setLoading(false);
        }
      })();
      await claim.promise;
    },
    [agentId, scope.token, scope.isCurrent],
  );
  useEffect(() => {
    void refreshStatus();
  }, [refreshStatus]);
  const visible = owner === agentId && scope.isCurrent(scope.token);
  const pendingId =
    visible && status?.connector?.status === 'pairing'
      ? status.connector.id
      : null;
  useEffect(() => {
    if (!pendingId) return;
    let polls = 0;
    const timer = setInterval(() => {
      if (++polls >= MAX_CONSENT_POLLS) clearInterval(timer);
      if (
        !locked.current &&
        read.current === null &&
        document.visibilityState !== 'hidden'
      )
        void refreshStatus(true);
    }, DRIVE_CONSENT_POLL_MS);
    return () => clearInterval(timer);
  }, [pendingId, consentUrl, refreshStatus]);

  const action = useCallback(
    async (
      run: (
        apply: (next: DriveStatus, url?: string | null) => void,
      ) => Promise<void>,
    ) => {
      if (
        !scope.isCurrent(scope.token) ||
        !allowed.current ||
        !agentId ||
        activeAgent.current !== agentId ||
        locked.current
      )
        return false;
      const generation = lifetime.current;
      const requestRevision = ++revision.current;
      locked.current = true;
      setBusy(true);
      setAvailable(false);
      setError(null);
      setConsentUrl(null);
      const current = () =>
        scope.isCurrent(scope.token) &&
        generation === lifetime.current &&
        requestRevision === revision.current;
      try {
        await run((next, url = null) => {
          if (current()) {
            setStatus(next);
            setConsentUrl(url);
          }
        });
        return current();
      } catch (caught) {
        if (current()) setError(safeDriveError(caught));
        return false;
      } finally {
        if (current()) {
          locked.current = false;
          setBusy(false);
          await refreshStatus(true);
        }
      }
    },
    [agentId, refreshStatus, scope.token, scope.isCurrent],
  );
  return {
    status: visible ? status : null,
    consentUrl: visible ? consentUrl : null,
    loading,
    available: visible && available,
    busy,
    error,
    refresh: () => refreshStatus(),
    connect: (input: ConnectDriveInput) =>
      action(async (apply) => {
        const result = await daemon.connectDrive(agentId!, input);
        const url = googleDriveConsentUrl(result.consentUrl);
        if (!url) throw new Error(INVALID_DRIVE_CONSENT);
        apply({ configured: true, connector: result.connector }, url);
      }),
    disconnect: (connectorId: string) =>
      action(async () => {
        await daemon.disconnectDrive(agentId!, connectorId);
      }),
  };
}
