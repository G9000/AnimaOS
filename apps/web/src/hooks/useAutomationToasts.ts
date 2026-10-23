import { useCallback, useEffect, useRef, useState } from 'react';
import { automationMessageMeta, type AgentEvent } from '@animaos/sdk';

import {
  dismissToast,
  makeToast,
  notifyToast,
  pushToast,
  shouldToast,
  type AutomationToast,
  type ChatAttention,
} from '../lib/automation-toasts';
import { daemon } from '../lib/daemon-api';
import { useAgentEvents } from './useAgentEvents';
import { useLatest } from './useLatest';
import { useRenderScope } from './useRenderScope';

/** The daemon commits a message and then announces it; a read that races the
 *  announcement tries again after these waits. */
export const TOAST_READ_RETRY_MS: readonly number[] = [0, 400, 1_200];
/** Messages read to find the announced one (it is among the newest). */
const NEWEST_MESSAGES = 20;

export interface AutomationToastsOptions {
  agentId: string | null;
  /** The main chat's session; null until it is known. */
  mainSessionId: string | null;
  enabled: boolean;
  /** What the owner is looking at, read when a message arrives. */
  attention: () => ChatAttention;
  /** Called for each toast shown (a system notification by default). */
  notify?: (toast: AutomationToast) => void;
}

/**
 * Toasts for what automations say in the main chat while the owner is not
 * looking at the bottom of it. The companion's event stream (the one stream
 * every view shares) announces each message of every session with
 * `message.created`; one in the main chat is read, and an `automation` or
 * `automationNote` message becomes a toast unless the owner is already
 * looking at it. Only messages that arrive live are announced: a page that
 * loads later shows them in the chat itself.
 */
export function useAutomationToasts({
  agentId,
  mainSessionId,
  enabled,
  attention,
  notify = notifyToast,
}: AutomationToastsOptions) {
  const [toasts, setToasts] = useState<AutomationToast[]>([]);
  const latestRef = useLatest({
    agentId,
    mainSessionId,
    enabled,
    attention,
    notify,
  });
  const scope = useRenderScope(agentId, mainSessionId);
  const [previousScope, setPreviousScope] = useState(scope);
  if (previousScope !== scope) {
    setPreviousScope(scope);
    setToasts([]);
  }
  const seen = useRef(new Set<string>());
  const timers = useRef(new Set<number>());

  useEffect(() => {
    const pending = timers.current;
    return () => {
      for (const timer of pending) window.clearTimeout(timer);
      pending.clear();
    };
  }, []);
  // Another companion's chat starts clean.
  useEffect(() => {
    seen.current.clear();
  }, [agentId, mainSessionId]);

  const announce = useCallback(
    function announceMessage(messageId: string, attempt = 0) {
      const { agentId: agent, mainSessionId: session } = latestRef.current;
      if (!agent || !session) return;
      void daemon
        .sessionMessages(agent, session, { limit: NEWEST_MESSAGES })
        .then((page) => page.messages.find((item) => item.id === messageId))
        .catch(() => undefined)
        .then((message) => {
          const current = latestRef.current;
          if (current.agentId !== agent || current.mainSessionId !== session)
            return;
          if (!message) {
            const wait = TOAST_READ_RETRY_MS[attempt + 1];
            if (wait === undefined) return;
            const timer = window.setTimeout(() => {
              timers.current.delete(timer);
              announceMessage(messageId, attempt + 1);
            }, wait);
            timers.current.add(timer);
            return;
          }
          const meta = automationMessageMeta(message);
          if (!meta || seen.current.has(message.id)) return;
          seen.current.add(message.id);
          if (!shouldToast(current.attention())) return;
          const toast = makeToast(message.id, meta);
          setToasts((existing) => pushToast(existing, toast));
          current.notify(toast);
        });
    },
    [latestRef],
  );

  const onEvent = useCallback(
    (event: AgentEvent) => {
      if (event.type !== 'message.created' || event.role !== 'assistant')
        return;
      const {
        agentId: agent,
        mainSessionId: session,
        enabled: on,
      } = latestRef.current;
      if (!on || event.agentId !== agent || event.sessionId !== session) return;
      announce(event.messageId);
    },
    [announce, latestRef],
  );
  useAgentEvents(agentId, onEvent);

  const dismiss = useCallback(
    (id: string) => setToasts((existing) => dismissToast(existing, id)),
    [],
  );
  return { toasts, dismiss };
}
