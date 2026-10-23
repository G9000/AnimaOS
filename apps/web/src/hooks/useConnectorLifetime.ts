import { useCallback, useLayoutEffect, useMemo, useRef } from 'react';
import { useLatest } from './useLatest';

interface ConnectorLifetime {
  owner: string | null;
  online: boolean;
  epoch: number;
}

/** Invalidate captured work on selection commits, before passive effects run. */
export function useConnectorLifetime(
  owner: string | null,
  online = true,
  epoch = 0,
) {
  const token = useMemo<ConnectorLifetime>(
    () => ({ owner, online, epoch }),
    [owner, online, epoch],
  );
  const latest = useLatest(token);
  const mounted = useRef(true);
  useLayoutEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const isCurrent = useCallback(
    (captured: ConnectorLifetime) =>
      mounted.current && captured.online && latest.current === captured,
    [latest],
  );
  // A parent-owned busy lock can settle after panel navigation, but never after
  // an owner, epoch or online transition. This does not permit data publication.
  const matches = useCallback(
    (captured: ConnectorLifetime) =>
      captured.online && latest.current === captured,
    [latest],
  );
  return { token, isCurrent, matches };
}
