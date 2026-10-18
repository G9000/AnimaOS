import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { Approval } from '@animaOS-SWARM/sdk';

import { ApprovalCard } from './ApprovalCard';

function planApproval(args: object): Approval {
  return {
    id: 'ap-plan',
    agentId: 'agent-1',
    sessionId: 'chat:main',
    runId: 'run-1',
    toolCallId: 'call-1',
    tool: 'propose_plan',
    class: 'write',
    arguments: JSON.stringify(args),
    argumentsTruncated: false,
    suggestedMatcher: { kind: 'any', value: '' },
    matcherKinds: ['any'],
    createdAtMs: 1,
    expiresAtMs: Date.now() + 60_000,
    status: 'pending',
    revision: 2,
    resolution: null,
  };
}

const lisbon = planApproval({
  title: 'Lisbon trip, May 14–19',
  steps: [
    { action: 'Compare flights' },
    { action: 'Book the flight you pick', asks_first: true },
  ],
  estimate: 'about 6 min · ~$0.40',
});

describe('plan approvals', () => {
  it('shows the plan as steps and starts it with Allow once', async () => {
    const user = userEvent.setup();
    const onDecide = vi.fn().mockResolvedValue(null);
    render(<ApprovalCard approval={lisbon} onDecide={onDecide} canPersist />);
    expect(
      screen.getByRole('region', { name: 'Plan: Lisbon trip, May 14–19' }),
    ).toBeVisible();
    expect(screen.getByText('Book the flight you pick')).toBeVisible();
    expect(screen.getByText('Asks first')).toBeVisible();
    expect(screen.getByText('about 6 min · ~$0.40')).toBeVisible();

    await user.click(screen.getByRole('button', { name: 'Start plan' }));
    expect(onDecide).toHaveBeenCalledWith(lisbon, { decision: 'allow_once' });
    expect(await screen.findByRole('status')).toHaveTextContent('Plan started');
  });

  it('sends changes back as a denial the agent reads', async () => {
    const user = userEvent.setup();
    const onDecide = vi.fn().mockResolvedValue(null);
    render(<ApprovalCard approval={lisbon} onDecide={onDecide} />);
    expect(
      screen.queryByRole('button', { name: 'Don’t ask for plans' }),
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Change it' }));
    const send = screen.getByRole('button', { name: 'Send changes' });
    expect(send).toBeDisabled();
    await user.type(
      screen.getByLabelText('What should change?'),
      'Telegram, not email',
    );
    await user.click(send);
    expect(onDecide).toHaveBeenCalledWith(lisbon, {
      decision: 'deny',
      note: 'Change the plan: Telegram, not email',
    });
  });

  it('falls back to the plain approval when the plan was cut short', () => {
    render(
      <ApprovalCard
        approval={{ ...lisbon, arguments: '{"title":"Lisbon","steps":[' }}
      />,
    );
    expect(
      screen.getByRole('region', { name: 'Approval needed: propose_plan' }),
    ).toBeVisible();
  });
});
