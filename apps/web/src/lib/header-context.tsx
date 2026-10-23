import {
  createContext,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from 'react';
import type { SessionSummary } from '@animaos/sdk';

/** What the conversation tells the top bar: the day at the top of the screen
 *  and what anima kept from earlier. The chat publishes it, the shell shows
 *  it, so a long history needs no row of its own. */
export interface HeaderContextState {
  day: string | null;
  summary: SessionSummary | null;
}

interface Publisher {
  setDay: (day: string | null) => void;
  setSummary: (summary: SessionSummary | null) => void;
}

const StateContext = createContext<HeaderContextState | null>(null);
const PublisherContext = createContext<Publisher | null>(null);

export function HeaderContextProvider({ children }: { children: ReactNode }) {
  const [day, setDay] = useState<string | null>(null);
  const [summary, setSummary] = useState<SessionSummary | null>(null);
  const state = useMemo(() => ({ day, summary }), [day, summary]);
  // Setters are stable, so publishing never re-renders the publisher.
  const publisher = useMemo(() => ({ setDay, setSummary }), []);
  return (
    <PublisherContext.Provider value={publisher}>
      <StateContext.Provider value={state}>{children}</StateContext.Provider>
    </PublisherContext.Provider>
  );
}

/** The published context, or null outside the shell. */
export function useHeaderContextState(): HeaderContextState | null {
  return useContext(StateContext);
}

/** Publishes the day at the top of the conversation; clears it on unmount. */
export function usePublishHeaderDay(day: string | null): void {
  const publisher = useContext(PublisherContext);
  useEffect(() => {
    if (!publisher) return;
    publisher.setDay(day);
    return () => publisher.setDay(null);
  }, [publisher, day]);
}

/** Publishes what anima kept from earlier. Returns whether the header took it;
 *  a view rendered outside the shell has no header and shows it itself. */
export function usePublishHeaderSummary(
  summary: SessionSummary | null,
): boolean {
  const publisher = useContext(PublisherContext);
  useEffect(() => {
    if (!publisher) return;
    publisher.setSummary(summary);
    return () => publisher.setSummary(null);
  }, [publisher, summary]);
  return publisher !== null;
}
