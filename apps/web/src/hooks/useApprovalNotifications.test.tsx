import { renderHook } from '@testing-library/react';
import type { Approval } from '@animaos/sdk';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { useApprovalNotifications } from './useApprovalNotifications';

const approval = (id: string) =>
  ({ id, agentId: 'agent-main', tool: 'bash' }) as Approval;
const name = () => 'Nova';

function setHidden(hidden: boolean) {
  Object.defineProperty(document, 'hidden', {
    configurable: true,
    get: () => hidden,
  });
}

afterEach(() => setHidden(false));

describe('useApprovalNotifications', () => {
  it('notifies only for approvals that arrive while the tab is in the background', () => {
    const notify = vi.fn(() => true);
    const { rerender } = renderHook(
      ({ list }) => useApprovalNotifications(list, name, notify),
      { initialProps: { list: [approval('waiting-on-open')] } },
    );
    expect(notify).not.toHaveBeenCalled();

    setHidden(true);
    rerender({ list: [approval('waiting-on-open'), approval('new')] });
    expect(notify).toHaveBeenCalledTimes(1);
    expect(notify).toHaveBeenCalledWith('Nova needs your approval', {
      body: 'Wants to use bash. Open anima to allow or deny it.',
      tag: 'approval:new',
    });

    rerender({ list: [approval('new')] });
    expect(notify).toHaveBeenCalledTimes(1);
  });
});
