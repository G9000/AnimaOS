import { useCallback, useEffect, useRef, useState } from 'react';
import { StatusTooOldError } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';

/** One bounded status probe per connection/snapshot; never polls Health. */
export function useDaemonCompatibility({
  enabled,
  epoch,
}: {
  enabled: boolean;
  epoch: number;
}) {
  const [statusUnsupported, setStatusUnsupported] = useState(false);
  const generation = useRef(0);
  const check = useCallback(async () => {
    if (!enabled) return;
    const mine = ++generation.current;
    try {
      await daemon.status();
      if (mine === generation.current) setStatusUnsupported(false);
    } catch (error) {
      if (mine === generation.current && error instanceof StatusTooOldError)
        setStatusUnsupported(true);
    }
  }, [enabled]);
  useEffect(() => {
    void check();
    return () => {
      generation.current += 1;
    };
  }, [check, epoch]);
  return { statusUnsupported, check };
}
