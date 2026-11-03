import { useEffect } from 'react';

import { APPEARANCE_CHANGED } from '../lib/appearance-preferences';
import { CHAT_BACKGROUND_CHANGED } from '../lib/chat-background';
import { daemon } from '../lib/daemon-api';
import {
  applyingDaemonLook,
  localLook,
  syncLookFromDaemon,
} from '../lib/owner-look';
import { useAgentEvents } from './useAgentEvents';

/** How long a run of switch changes settles before it is saved. */
const SAVE_DELAY_MS = 250;

/**
 * Keeps this console's look in step with the daemon's, where chat can
 * change it: loads it when the console comes online or back into view, and
 * again whenever the companion finishes a `change_look`; saves the owner's
 * own switches (the Appearance page) back to it. A failed load or save
 * leaves this browser's look as it is.
 */
export function useOwnerLookSync(agentId: string | null, online: boolean) {
  useEffect(() => {
    if (!online) return;
    let timer: number | undefined;
    // An unsaved switch change is newer than what the daemon holds.
    const load = () => {
      if (timer === undefined)
        void syncLookFromDaemon().catch(() => undefined);
    };
    load();
    const onVisible = () => {
      if (!document.hidden) load();
    };
    const onLocalChange = () => {
      if (applyingDaemonLook()) return;
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = undefined;
        void daemon.setLook(localLook()).catch(() => undefined);
      }, SAVE_DELAY_MS);
    };
    window.addEventListener('focus', load);
    document.addEventListener('visibilitychange', onVisible);
    window.addEventListener(APPEARANCE_CHANGED, onLocalChange);
    window.addEventListener(CHAT_BACKGROUND_CHANGED, onLocalChange);
    return () => {
      window.clearTimeout(timer);
      window.removeEventListener('focus', load);
      document.removeEventListener('visibilitychange', onVisible);
      window.removeEventListener(APPEARANCE_CHANGED, onLocalChange);
      window.removeEventListener(CHAT_BACKGROUND_CHANGED, onLocalChange);
    };
  }, [online]);

  useAgentEvents(online ? agentId : null, (event) => {
    if (
      event.type === 'tool.finished' &&
      event.name === 'change_look' &&
      event.status === 'success'
    )
      void syncLookFromDaemon().catch(() => undefined);
  });
}
