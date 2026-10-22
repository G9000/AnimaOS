import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import {
  DaemonHttpError,
  type AutomationCardMeta,
  type AutomationNoteMeta,
} from '@animaos/sdk';

import type { AutomationCardActions } from '../../lib/automation-cards';
import { automationFixture } from '../../test/automations';
import { AutomationMessageCard } from './AutomationMessageCard';

const FIRED = Date.UTC(2026, 9, 21, 14, 0);

const reminder: AutomationCardMeta = {
  kind: 'automation',
  automationId: 'rem',
  automationName: 'Call the bank',
  reminder: true,
  body: 'Their cut-off is 3:00 PM.',
  delivery: 'main',
  runId: 'run-1',
  firedAtMs: FIRED,
};
const report: AutomationCardMeta = {
  ...reminder,
  automationId: 'brief',
  automationName: 'Morning brief',
  reminder: false,
  body: 'Light day. The bank call is at 2:00 PM.',
};
const note: AutomationNoteMeta = {
  kind: 'automationNote',
  automationId: 'weekly',
  automationName: 'Weekly review',
  body: 'The whole review, as sent.',
  delivery: 'discord',
  destinationLabel: '#home',
  destinationSessionId: 'discord:home',
  destinationMessageId: 'message-9',
  runId: 'run-2',
  firedAtMs: FIRED,
};

const due = automationFixture('rem', {
  trigger: { type: 'once', atMs: FIRED },
  enabled: false,
  lastFiredAtMs: FIRED,
});

function actions(
  overrides: Partial<AutomationCardActions> = {},
): AutomationCardActions {
  return {
    automations: [due],
    loaded: true,
    online: true,
    onDone: vi.fn().mockResolvedValue(true),
    onSnooze: vi.fn().mockResolvedValue(true),
    onOpenSession: vi.fn(),
    ...overrides,
  };
}

describe('a due reminder card', () => {
  it('says it needs you and offers Done, Snooze 15 min, and Later', async () => {
    const user = userEvent.setup();
    const card = actions();
    render(<AutomationMessageCard meta={reminder} actions={card} />);
    const article = screen.getByRole('article', { name: 'Reminder Call the bank' });

    expect(within(article).getByText('Needs you')).toBeVisible();
    expect(within(article).getByText('Their cut-off is 3:00 PM.')).toBeVisible();
    expect(article).toHaveAttribute('data-state', 'due');

    await user.click(within(article).getByRole('button', { name: /Done/ }));
    expect(card.onDone).toHaveBeenCalledWith(due);
    await user.click(
      within(article).getByRole('button', { name: /Snooze 15 min/ }),
    );
    expect(card.onSnooze).toHaveBeenCalledWith(due, 15);
  });

  it('offers one hour and three hours behind Later', async () => {
    const user = userEvent.setup();
    const card = actions();
    render(<AutomationMessageCard meta={reminder} actions={card} />);

    expect(screen.queryByRole('group', { name: 'Snooze for' })).toBeNull();
    await user.click(screen.getByRole('button', { name: /Later/ }));
    const later = screen.getByRole('group', { name: 'Snooze for' });
    await user.click(within(later).getByRole('button', { name: '3 hours' }));
    expect(card.onSnooze).toHaveBeenCalledWith(due, 180);
    expect(
      within(screen.getByRole('article')).queryByRole('group', {
        name: 'Snooze for',
      }),
    ).toBeNull();
  });

  it('mentions Telegram only when a copy went there', () => {
    const { rerender } = render(
      <AutomationMessageCard meta={reminder} actions={actions()} />,
    );
    expect(screen.queryByText(/Also sent to Telegram/i)).toBeNull();
    rerender(
      <AutomationMessageCard
        meta={{ ...reminder, delivery: 'telegram' }}
        actions={actions()}
      />,
    );
    expect(screen.getByText(/Also sent to Telegram/i)).toBeVisible();
  });

  it('shows the daemon message when it refuses, and allows another try', async () => {
    const user = userEvent.setup();
    const card = actions({
      onDone: vi
        .fn()
        .mockRejectedValueOnce(
          new DaemonHttpError(409, { error: 'Already done elsewhere' }),
        )
        .mockResolvedValue(true),
    });
    render(<AutomationMessageCard meta={reminder} actions={card} />);

    await user.click(screen.getByRole('button', { name: /Done/ }));
    expect(await screen.findByText('Already done elsewhere')).toBeVisible();
    expect(screen.getByRole('button', { name: /Done/ })).toBeEnabled();
    await user.click(screen.getByRole('button', { name: /Done/ }));
    expect(card.onDone).toHaveBeenCalledTimes(2);
  });

  it('disables the answers while the daemon is offline', () => {
    render(<AutomationMessageCard meta={reminder} actions={actions({ online: false })} />);
    expect(screen.getByRole('button', { name: /Done/ })).toBeDisabled();
    expect(screen.getByRole('button', { name: /Snooze 15 min/ })).toBeDisabled();
  });
});

describe('a reminder card after it was answered', () => {
  it('says when it will pop up again once snoozed, without actions', () => {
    const snoozed = automationFixture('rem', {
      trigger: { type: 'once', atMs: FIRED },
      enabled: true,
      lastFiredAtMs: FIRED,
      nextDueAtMs: FIRED + 900_000,
    });
    render(
      <AutomationMessageCard
        meta={reminder}
        actions={actions({ automations: [snoozed] })}
      />,
    );
    const article = screen.getByRole('article', { name: 'Reminder Call the bank' });
    expect(article).toHaveAttribute('data-state', 'snoozed');
    expect(within(article).getByText(/Snoozed until .* · pops up again then/)).toBeVisible();
    expect(within(article).queryByRole('button')).toBeNull();
    expect(within(article).queryByText('Needs you')).toBeNull();
  });

  it('says when it was done, faded, without actions', () => {
    const done = automationFixture('rem', {
      trigger: { type: 'once', atMs: FIRED },
      enabled: false,
      lastFiredAtMs: FIRED,
      doneAtMs: FIRED + 240_000,
    });
    render(
      <AutomationMessageCard meta={reminder} actions={actions({ automations: [done] })} />,
    );
    const article = screen.getByRole('article', { name: 'Reminder Call the bank' });
    expect(article).toHaveAttribute('data-state', 'done');
    expect(within(article).getByText(/^Done at /)).toBeVisible();
    expect(within(article).queryByRole('button')).toBeNull();
  });

  it('reads as cleared, without actions, once the daemon removed the reminder', () => {
    render(
      <AutomationMessageCard meta={reminder} actions={actions({ automations: [] })} />,
    );
    const article = screen.getByRole('article', { name: 'Reminder Call the bank' });
    expect(article).toHaveAttribute('data-state', 'gone');
    expect(within(article).queryByRole('button')).toBeNull();
  });

  it('shows no actions before the automation list has loaded', () => {
    render(
      <AutomationMessageCard
        meta={reminder}
        actions={actions({ loaded: false, automations: [] })}
      />,
    );
    expect(screen.getByText('Their cut-off is 3:00 PM.')).toBeVisible();
    expect(screen.queryByRole('button')).toBeNull();
  });
});

describe('a report card', () => {
  it('shows the name, its trigger, and the body', () => {
    const daily = automationFixture('brief', {
      trigger: { type: 'daily', hour: 9, minute: 0, timeZone: 'UTC' },
    });
    render(
      <AutomationMessageCard meta={report} actions={actions({ automations: [daily] })} />,
    );
    const article = screen.getByRole('article', { name: 'Automation Morning brief' });
    expect(within(article).getByText('Morning brief')).toBeVisible();
    expect(within(article).getByText(/^Automation · daily at 09:00/)).toBeVisible();
    expect(
      within(article).getByText('Light day. The bank call is at 2:00 PM.'),
    ).toBeVisible();
    expect(within(article).queryByRole('button')).toBeNull();
    expect(within(article).queryByText('Needs you')).toBeNull();
  });

  it('shows the name and body without a known trigger', () => {
    render(<AutomationMessageCard meta={report} />);
    expect(screen.getByText('Automation')).toBeVisible();
  });

  it('shows text as text with invisible characters made visible', () => {
    const sneaky = { ...report, automationName: `Brief${String.fromCodePoint(0x202e)}` };
    render(<AutomationMessageCard meta={sneaky} />);
    expect(screen.getByText('Brief⟨U+202E⟩')).toBeVisible();
  });
});

describe('a Discord note', () => {
  it('is one collapsed row that expands to the full text', async () => {
    const user = userEvent.setup();
    render(<AutomationMessageCard meta={note} actions={actions()} />);

    expect(screen.getByText(/Weekly review went to Discord · #home/)).toBeVisible();
    expect(screen.queryByText('The whole review, as sent.')).toBeNull();
    const toggle = screen.getByRole('button', {
      name: 'Show what Weekly review sent',
    });
    expect(toggle).toHaveAttribute('aria-expanded', 'false');

    await user.click(toggle);
    expect(screen.getByText('The whole review, as sent.')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Hide what Weekly review sent' }),
    ).toHaveAttribute('aria-expanded', 'true');
  });

  it('opens its thread at the message that was sent', async () => {
    const user = userEvent.setup();
    const card = actions();
    render(<AutomationMessageCard meta={note} actions={card} />);
    await user.click(screen.getByRole('button', { name: 'Open thread' }));
    expect(card.onOpenSession).toHaveBeenCalledWith('discord:home', 'message-9');
  });

  it('hides Open thread without a destination session', () => {
    const { destinationSessionId, destinationMessageId, ...bare } = note;
    void destinationSessionId;
    void destinationMessageId;
    render(<AutomationMessageCard meta={bare} actions={actions()} />);
    expect(screen.queryByRole('button', { name: 'Open thread' })).toBeNull();
    expect(screen.getByText(/Weekly review went to Discord/)).toBeVisible();
  });
});
