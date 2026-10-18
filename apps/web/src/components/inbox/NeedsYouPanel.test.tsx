import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Approval } from '@animaOS-SWARM/sdk';

import type { TelegramConnector } from '../../lib/daemon-api';
import type { NeedsYouItem } from '../../lib/needs-you';
import { NeedsYouPanel, type DoneToday } from './NeedsYouPanel';

const approval = {
  id: 'ap-1',
  agentId: 'agent-1',
  sessionId: 'chat:main',
  tool: 'propose_plan',
  class: 'write',
  arguments: JSON.stringify({
    title: 'Lisbon trip',
    steps: ['Compare flights', 'Book the flight'],
  }),
  createdAtMs: Date.now(),
  revision: 3,
} as Approval;

const items: NeedsYouItem[] = [
  { kind: 'approve', id: 'approval:ap-1', atMs: Date.now(), approval },
  {
    kind: 'pair',
    id: 'pair:tg-1',
    atMs: Date.now(),
    connector: { id: 'tg-1' } as TelegramConnector,
    chatId: '101',
    chatName: 'julio',
  },
];

function renderPanel(
  overrides: Partial<Parameters<typeof NeedsYouPanel>[0]> = {},
) {
  const props = {
    items,
    agentName: 'anima',
    decideApproval: vi.fn().mockResolvedValue(null),
    approvePairing: vi.fn().mockResolvedValue(true),
    openSession: vi.fn(),
    openPage: vi.fn(),
    done: { state: 'loading' } as DoneToday,
    onShowDone: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<NeedsYouPanel {...props} />);
  return props;
}

describe('NeedsYouPanel', () => {
  it('starts a plan with the approval’s own decision and approves a Telegram chat', async () => {
    const user = userEvent.setup();
    const props = renderPanel();
    const list = screen.getByRole('list', { name: 'Waiting for you' });
    expect(within(list).getByText('Plan: Lisbon trip')).toBeVisible();

    await user.click(screen.getByRole('button', { name: 'Start plan' }));
    expect(props.decideApproval).toHaveBeenCalledWith(approval, {
      decision: 'allow_once',
    });

    await user.click(screen.getByRole('button', { name: 'Approve chat' }));
    expect(props.approvePairing).toHaveBeenCalledWith('tg-1', '101');
  });

  it('shows why a decision failed next to the item', async () => {
    const user = userEvent.setup();
    renderPanel({
      decideApproval: vi
        .fn()
        .mockResolvedValue('This approval is no longer waiting.'),
    });
    await user.click(screen.getByRole('button', { name: 'Not now' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This approval is no longer waiting.',
    );
  });

  it('opens the chat an approval came from and closes', async () => {
    const user = userEvent.setup();
    const props = renderPanel();
    await user.click(screen.getByRole('button', { name: 'Open chat' }));
    expect(props.openSession).toHaveBeenCalledWith('agent-1', 'chat:main');
    expect(props.onClose).toHaveBeenCalled();
  });

  it('reads today’s activity when Done today opens', async () => {
    const user = userEvent.setup();
    const props = renderPanel({
      done: {
        state: 'ready',
        actions: [
          {
            id: 'a',
            runId: 'run-1',
            sessionId: 'chat:main',
            atMs: Date.now(),
            title: 'Changed a file',
            detail: 'anima.toml',
            failed: false,
          },
        ],
      },
    });
    await user.click(screen.getByRole('tab', { name: /Done today/ }));
    expect(props.onShowDone).toHaveBeenCalled();
    const done = screen.getByRole('list', { name: 'Done today' });
    expect(within(done).getByText('Changed a file')).toBeVisible();
    expect(within(done).getByText('anima.toml')).toBeVisible();
  });

  it('says plainly when nothing waits', () => {
    renderPanel({ items: [] });
    expect(screen.getByText(/Nothing needs you right now/)).toBeVisible();
  });
});
