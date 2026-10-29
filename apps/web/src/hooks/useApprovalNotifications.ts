import { useEffect, useRef } from 'react';
import type { Approval } from '@animaos/sdk';
import { showSystemNotification } from '../lib/system-notifications';

/**
 * A desktop notification when an agent starts waiting for the owner's
 * approval while this tab is in the background. Approvals already waiting
 * when the page opened, or that arrive while the owner is looking, are not
 * announced: the page shows them.
 */
export function useApprovalNotifications(
  approvals: readonly Approval[],
  agentName: (agentId: string) => string,
  notify: typeof showSystemNotification = showSystemNotification,
) {
  const seen = useRef(new Set<string>());
  useEffect(() => {
    for (const approval of approvals) {
      if (seen.current.has(approval.id)) continue;
      seen.current.add(approval.id);
      if (typeof document === 'undefined' || !document.hidden) continue;
      notify(`${agentName(approval.agentId)} needs your approval`, {
        body: `Wants to use ${approval.tool}. Open anima to allow or deny it.`,
        tag: `approval:${approval.id}`,
      });
    }
  }, [approvals, agentName, notify]);
}
