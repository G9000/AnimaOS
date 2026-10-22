import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import type { Session } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';

interface Resolution {
  agentId: string | null;
  session: Session | null;
  pending: Promise<Session> | null;
  attempted: boolean;
}

function resolution(agentId: string | null): Resolution {
  return { agentId, session: null, pending: null, attempted: false };
}

function validMainSession(session: Session, agentId: string): boolean {
  return (
    session != null &&
    typeof session.id === 'string' &&
    session.id.length > 0 &&
    typeof session.roomId === 'string' &&
    session.roomId.length > 0 &&
    session.agentId === agentId &&
    session.kind === 'chat' &&
    session.origin === 'web' &&
    session.archived === false &&
    session.mainChat === true &&
    session.parentSessionId === null &&
    session.parentRunId === null &&
    session.parentAgentId === null
  );
}

/** One resolution shared by home, attachment admission, and first send. */
export function useMainChat({
  agentId,
  online,
}: {
  agentId: string | null;
  online: boolean;
}) {
  const current = useRef(resolution(agentId));
  const connected = useRef(online);
  const mounted = useRef(false);
  const [state, setState] = useState({
    agentId,
    session: null as Session | null,
    loading: false,
    error: null as string | null,
  });

  useLayoutEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useLayoutEffect(() => {
    connected.current = online;
    if (current.current.agentId === agentId) return;
    current.current = resolution(agentId);
    setState({ agentId, session: null, loading: false, error: null });
  }, [agentId, online]);

  const ensureSession = useCallback((): Promise<Session> => {
    const active = current.current;
    const activeAgentId = active.agentId;
    if (!mounted.current || !activeAgentId)
      return Promise.reject(new Error('No active companion.'));
    if (active.session) return Promise.resolve(active.session);
    if (active.pending) return active.pending;
    if (!connected.current)
      return Promise.reject(
        new Error('The daemon is offline. Reconnect to open your main chat.'),
      );

    active.attempted = true;
    setState({
      agentId: activeAgentId,
      session: null,
      loading: true,
      error: null,
    });
    const isCurrent = () => mounted.current && current.current === active;
    const request = daemon
      .ensureMainSession(activeAgentId)
      .then((session) => {
        if (!isCurrent())
          throw new Error(
            'The active companion changed before its main chat was ready.',
          );
        if (!validMainSession(session, activeAgentId))
          throw new Error(
            'The daemon returned an invalid main chat. Retry or update the daemon.',
          );
        active.session = session;
        setState({
          agentId: activeAgentId,
          session,
          loading: false,
          error: null,
        });
        return session;
      })
      .catch((error: unknown) => {
        if (isCurrent())
          setState({
            agentId: activeAgentId,
            session: null,
            loading: false,
            error: error instanceof Error ? error.message : String(error),
          });
        throw error;
      })
      .finally(() => {
        if (active.pending === request) active.pending = null;
      });
    active.pending = request;
    return request;
  }, []);

  useEffect(() => {
    if (!agentId || !online || current.current.attempted) return;
    void ensureSession().catch(() => {
      /* The visible state owns the error. */
    });
  }, [agentId, online, ensureSession]);

  const retry = useCallback(() => {
    void ensureSession().catch(() => {
      /* The visible state owns the error. */
    });
  }, [ensureSession]);

  const visible = state.agentId === agentId;
  return {
    session: visible ? state.session : null,
    loading: visible && state.loading,
    error: visible ? state.error : null,
    ensureSession,
    retry,
  };
}
