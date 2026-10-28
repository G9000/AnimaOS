import {
  createContext,
  useCallback,
  useMemo,
  useState,
  type ReactNode,
} from 'react';

/** Where a connection comes from, so the catalogue can group by provider. */
export const SERVICE_GROUPS = [
  { id: 'google', label: 'Google Workspace' },
  { id: 'microsoft', label: 'Microsoft 365' },
  { id: 'email', label: 'Email' },
  { id: 'chat', label: 'Chat apps' },
] as const;

export type ServiceGroup = (typeof SERVICE_GROUPS)[number]['id'];

/** One connection as the catalogue sees it. */
export interface ServiceEntry {
  id: string;
  title: string;
  summary: string;
  group: ServiceGroup;
  /** The status the service itself reports, before any wording is softened. */
  status: string;
  icon: ReactNode;
}

export interface ServiceDirectory {
  entries: readonly ServiceEntry[];
  /** The service whose settings the owner asked to open from the catalogue. */
  request: string | null;
  report(entry: ServiceEntry): void;
  forget(id: string): void;
  ask(id: string): void;
  clear(): void;
}

/** Null outside a connections page: a tile then stands alone. */
export const ServiceDirectoryContext = createContext<ServiceDirectory | null>(
  null,
);

/** The directory a connections page offers to the tiles inside it. */
export function useServiceDirectory(): ServiceDirectory {
  const [entries, setEntries] = useState<Record<string, ServiceEntry>>({});
  const [request, setRequest] = useState<string | null>(null);
  const report = useCallback((entry: ServiceEntry) => {
    setEntries((current) => {
      const known = current[entry.id];
      if (
        known &&
        known.title === entry.title &&
        known.summary === entry.summary &&
        known.group === entry.group &&
        known.status === entry.status
      )
        return current;
      return { ...current, [entry.id]: entry };
    });
  }, []);
  const forget = useCallback((id: string) => {
    setEntries((current) => {
      if (!(id in current)) return current;
      const { [id]: _removed, ...rest } = current;
      return rest;
    });
  }, []);
  const ask = useCallback((id: string) => setRequest(id), []);
  const clear = useCallback(() => setRequest(null), []);
  return useMemo(
    () => ({
      entries: Object.values(entries),
      request,
      report,
      forget,
      ask,
      clear,
    }),
    [entries, request, report, forget, ask, clear],
  );
}

/** How a connection stands, as the small square beside its status. */
export type ServiceTone = 'ok' | 'idle' | 'attention';

/** The tone a status line names; anything unknown reads as not set up. */
export function serviceTone(status: string): ServiceTone {
  if (/^(connected|configured)$/i.test(status)) return 'ok';
  if (
    /reconnect|reauth|consent|unavailable|waiting|awaiting|token|error|degraded|elsewhere/i.test(
      status,
    )
  )
    return 'attention';
  return 'idle';
}

/** A connection is listed on the page once it is connected or needs the
 *  owner's attention; everything else waits in the catalogue. */
export function isListed(status: string): boolean {
  return serviceTone(status) !== 'idle';
}

/** Whether the service has said where it stands yet. */
export function isSettled(status: string): boolean {
  return status !== '' && !/^(checking|loading)/i.test(status);
}

/** The status as the owner reads it: not set up is simply not connected. */
export function serviceStatusLabel(status: string): string {
  if (status === '') return 'Checking…';
  return status === 'Setup required' ? 'Not connected' : status;
}
