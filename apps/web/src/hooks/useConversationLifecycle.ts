import { useEffect, useRef } from 'react';
import type { Session, SessionMessage } from '@animaos/sdk';
import { importLegacyCheckins } from '../lib/checkins';
import { daemon } from '../lib/daemon-api';
import { sessionKey } from '../lib/session-groups';
/** Browser migration and read receipts scoped to the visible conversation. */
export function useConversationLifecycle({
  agentId,
  activeSession,
  conversationHidden,
  messages,
  refreshSessions,
  setLegacyMigrationError,
}: {
  agentId: string | null;
  activeSession: Session | null;
  conversationHidden: boolean;
  messages: readonly SessionMessage[];
  refreshSessions: () => void;
  setLegacyMigrationError: (error: string | null) => void;
}) {
  useEffect(() => {
    if (!agentId) return;
    let current = true;
    void importLegacyCheckins(agentId)
      .then((result) => {
        if (!current) return;
        if (result.malformed > 0) {
          setLegacyMigrationError(
            `${result.malformed} legacy check-in record could not be imported and was kept in this browser.`,
          );
        } else if (result.imported > 0) {
          setLegacyMigrationError(null);
        }
      })
      .catch(() => {
        if (current)
          setLegacyMigrationError(
            'Legacy check-ins could not be imported. They remain in this browser for retry.',
          );
      });
    return () => {
      current = false;
    };
  }, [agentId, setLegacyMigrationError]);

  // Opening an unread session marks it read up to its newest message, but
  // not while a page hides it.
  const markedReadRef = useRef(new Map<string, number>());
  useEffect(() => {
    if (conversationHidden || !activeSession?.unread || messages.length === 0)
      return;
    const newest = messages[messages.length - 1].createdAtMs;
    const key = sessionKey(activeSession);
    if ((markedReadRef.current.get(key) ?? 0) >= newest) return;
    markedReadRef.current.set(key, newest);
    void daemon
      .updateSession(activeSession.agentId, activeSession.id, {
        lastReadAtMs: newest,
      })
      .then(
        () => refreshSessions(),
        () => markedReadRef.current.delete(key),
      );
  }, [activeSession, conversationHidden, messages, refreshSessions]);
}
