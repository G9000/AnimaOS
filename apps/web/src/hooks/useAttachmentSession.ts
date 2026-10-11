import { useRef } from 'react';
import type { Session } from '@animaOS-SWARM/sdk';
import { daemon } from '../lib/daemon-api';

/** Both uploads and a new chat's first send use this single session creation. */
export function useAttachmentSession({
  chatKey,
  agentId,
  session,
}: {
  chatKey: string | null;
  agentId: string | null;
  session: Session | null;
}) {
  const pending = useRef(new Map<string, Promise<Session>>());
  const ensureSession = (): Promise<Session> => {
    if (session) return Promise.resolve(session);
    if (!chatKey || !agentId)
      return Promise.reject(new Error('No active chat.'));
    const key = `${agentId}\u0000${chatKey}`;
    const existing = pending.current.get(key);
    if (existing) return existing;
    const request = daemon.createSession(agentId);
    pending.current.set(key, request);
    void request.catch(() => {
      if (pending.current.get(key) === request) pending.current.delete(key);
    });
    return request;
  };
  const forget = (conversation: string) => {
    for (const key of pending.current.keys())
      if (key.endsWith(`\u0000${conversation}`)) pending.current.delete(key);
  };
  return { ensureSession, forget };
}
