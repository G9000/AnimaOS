import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AgentMemory, Run, Session, UsageSummary } from '@animaOS-SWARM/sdk';

import { daemon } from '../../lib/daemon-api';
import { WeeklyReport } from './WeeklyReport';

afterEach(() => {
  vi.restoreAllMocks();
});

const now = Date.now();
const session = {
  id: 'chat:main',
  agentId: 'agent-1',
  lastActivityAtMs: now,
} as Session;

describe('WeeklyReport', () => {
  it('reads the week within the daemon’s page size, shows it, and forgets a memory', async () => {
    const user = userEvent.setup();
    const runs = vi.spyOn(daemon, 'sessionRuns').mockResolvedValue([
      {
        id: 'run-1',
        sessionId: 'chat:main',
        status: 'failed',
        createdAtMs: now - 1000,
        finishedAtMs: now - 1000,
        error: { code: 'model', message: 'The model key was rejected.' },
        actionReceipts: [],
      } as unknown as Run,
    ]);
    vi.spyOn(daemon, 'usageSummary').mockResolvedValue({
      totals: { costMicros: 1_250_000, unpricedCalls: 0 },
      groups: [],
    } as unknown as UsageSummary);
    vi.spyOn(daemon, 'recentAgentMemories').mockResolvedValue({
      memories: [
        { id: 'm1', content: 'Prefers window seats', createdAt: now - 500 } as AgentMemory,
      ],
    });
    const forget = vi.spyOn(daemon, 'deleteMemory').mockResolvedValue(undefined as never);
    const openSession = vi.fn();
    const onClose = vi.fn();

    render(
      <WeeklyReport
        agentId="agent-1"
        agentName="anima"
        sessions={[session]}
        openSession={openSession}
        onClose={onClose}
      />,
    );

    const dialog = await screen.findByRole('dialog', { name: 'Weekly report' });
    expect(await within(dialog).findByText('$1.25')).toBeVisible();
    expect(runs).toHaveBeenCalledWith('agent-1', 'chat:main', { limit: 50 });
    expect(within(dialog).getByText('The model key was rejected.')).toBeVisible();

    await user.click(
      within(dialog).getByRole('button', { name: 'Forget: Prefers window seats' }),
    );
    expect(forget).toHaveBeenCalledWith('m1', 'Prefers window seats');
    expect(await within(dialog).findByText('Forgotten')).toBeVisible();

    await user.click(within(dialog).getByRole('button', { name: 'Open chat' }));
    expect(openSession).toHaveBeenCalledWith('chat:main');
    expect(onClose).toHaveBeenCalled();
  });

  it('says so when the week cannot be read', async () => {
    vi.spyOn(daemon, 'sessionRuns').mockRejectedValue(new Error('offline'));
    vi.spyOn(daemon, 'usageSummary').mockResolvedValue(null as never);
    vi.spyOn(daemon, 'recentAgentMemories').mockResolvedValue({ memories: [] });
    render(
      <WeeklyReport
        agentId="agent-1"
        agentName="anima"
        sessions={[session]}
        openSession={vi.fn()}
        onClose={vi.fn()}
      />,
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not read this week’s activity',
    );
  });
});
