import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { DaemonHttpError } from '@animaOS-SWARM/sdk';

import { COMPANION_UNREACHABLE } from '../../lib/approvals';
import { automationFixture } from '../../test/automations';
import { AutomationNoticeCard } from './AutomationNoticeCard';

const made = automationFixture('schedule-1', { name: 'Stretch' });

describe('AutomationNoticeCard', () => {
  it("shows the daemon's message when Undo is refused, and allows another try", async () => {
    const user = userEvent.setup();
    const onUndo = vi
      .fn()
      .mockRejectedValueOnce(
        new DaemonHttpError(409, {
          error: 'This automation is already running',
        }),
      )
      .mockResolvedValue(true);
    render(<AutomationNoticeCard automation={made} onUndo={onUndo} />);
    const card = screen.getByRole('note', { name: 'Automation Stretch' });

    await user.click(within(card).getByRole('button', { name: 'Undo' }));
    expect(
      await within(card).findByText('This automation is already running'),
    ).toBeInTheDocument();
    expect(within(card).getByRole('button', { name: 'Undo' })).toBeEnabled();

    await user.click(within(card).getByRole('button', { name: 'Undo' }));
    expect(onUndo).toHaveBeenCalledTimes(2);
    expect(
      within(card).queryByText('This automation is already running'),
    ).toBeNull();
  });

  it('names the zone of the next run, the schedule’s own when it has one', () => {
    // 2026-10-19 19:00 in New York.
    const evening = Date.UTC(2026, 9, 19, 23, 0);
    render(
      <AutomationNoticeCard
        automation={automationFixture('schedule-2', {
          name: 'Call mom',
          trigger: {
            type: 'cron',
            expression: '0 19 * * *',
            timeZone: 'America/New_York',
          },
          nextDueAtMs: evening,
        })}
      />,
    );
    const card = screen.getByRole('note', { name: 'Automation Call mom' });
    expect(card).toHaveTextContent(/next (?:Oct\s19|19\sOct)/);
    expect(card).toHaveTextContent(/07:00\sPM/i);
    expect(card).toHaveTextContent(/(?:EDT|GMT-4)/);
  });

  it('shows event readiness without turning the absent due time into a date', () => {
    render(
      <AutomationNoticeCard
        automation={automationFixture('event-1', {
          name: 'Changed files',
          trigger: { type: 'event', topic: 'files.changed' },
          nextDueAtMs: Number.MAX_VALUE,
        })}
      />,
    );
    const card = screen.getByRole('note', { name: 'Automation Changed files' });
    expect(card).toHaveTextContent(
      'when files.changed occurs · waiting for event',
    );
    expect(card).not.toHaveTextContent('Invalid Date');
  });

  it('says the companion is unreachable when Undo fails any other way', async () => {
    const user = userEvent.setup();
    const onUndo = vi.fn().mockRejectedValue(new TypeError('fetch failed'));
    render(<AutomationNoticeCard automation={made} onUndo={onUndo} />);

    await user.click(screen.getByRole('button', { name: 'Undo' }));
    expect(await screen.findByText(COMPANION_UNREACHABLE)).toBeInTheDocument();
  });
});
