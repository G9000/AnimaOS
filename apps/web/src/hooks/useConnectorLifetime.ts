import { useCallback, useLayoutEffect, useRef } from 'react';

interface ConnectorLifetime {
  owner: string | null;
  online: boolean;
  epoch: number;
}

/** Invalidate captured work during render, before a late promise can publish. */
export function useConnectorLifetime(
  owner: string | null,
  online = true,
  epoch = 0,
) {
  const latest = useRef<ConnectorLifetime>({ owner, online, epoch });
  if (
    latest.current.owner !== owner ||
    latest.current.online !== online ||
    latest.current.epoch !== epoch
  )
    latest.current = { owner, online, epoch };
  const token = latest.current;
  const mounted = useRef(true);
  useLayoutEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, [token]);
  const isCurrent = useCallback(
    (captured: ConnectorLifetime) =>
      mounted.current && captured.online && latest.current === captured,
    [],
  );
  // A parent-owned busy lock can settle after panel navigation, but never after
  // an owner, epoch or online transition. This does not permit data publication.
  const matches = useCallback(
    (captured: ConnectorLifetime) =>
      captured.online && latest.current === captured,
    [],
  );
  return { token, isCurrent, matches };
}
