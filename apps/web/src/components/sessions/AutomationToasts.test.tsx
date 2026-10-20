import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AutomationMessageMeta } from '@animaOS-SWARM/sdk';

import {
  TOAST_AUTO_DISMISS_MS,
  dismissToast,
  makeToast,
  pushToast,
  type AutomationToast,
} from '../../lib/automation-toasts';
import { automationFixture } from '../../test/automations';
import { AutomationToasts } from './AutomationToasts';

const FIRED = Date.UTC(2026, 9, 21, 14, 0);
const base = {
  kind: 'automation' as const,
  automationId: 'rem',
  automationName: 'Call the bank',
  reminder: true,
  body: 'Their cut-off is 3:00 PM.',
  delivery: 'main' as const,
  runId: 'run-1',
  firedAtMs: FIRED,
};
const reminder: AutomationMessageMeta = base;
const digest: AutomationMessageMeta = {
  ...base,
  automationId: 'digest',
  automationName: 'Inbox digest',
  reminder: false,
};
const note: AutomationMessageMeta = {
  kind: 'automationNote',
  automationId: 'weekly',
  automationName: 'Weekly review',
  body: 'text',
  delivery: 'discord',
  destinationLabel: '#home',
  destinationSessionId: 'discord:home',
  runId: 'run-2',
  firedAtMs: FIRED,
};
const due = automationFixture('rem', {
  trigger: { type: 'once', atMs: FIRED },
  enabled: false,
  lastFiredAtMs: FIRED,
});

function Harness({
  initial,
  automations = [due],
  onOpen = vi.fn(),
  onDone = vi.fn().mockResolvedValue(true),
  onSnooze = vi.fn().mockResolvedValue(true),
}: {
  initial: AutomationToast[];
  automations?: typeof due[];
  onOpen?: (toast: AutomationToast) => void;
  onDone?: (automation: typeof due) => Promise<boolean>;
  onSnooze?: (automation: typeof due, minutes: number) => Promise<boolean>;
}) {
  const [toasts, setToasts] = useState(initial);
  return (
    <AutomationToasts
      toasts={toasts}
      automations={automations}
      loaded
      online
      onDismiss={(id) => setToasts((current) => dismissToast(current, id))}
      onOpen={onOpen}
      onDone={onDone}
      onSnooze={onSnooze}
    />
  );
}

beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

describe('the toast stack', () => {
  it('renders nothing without toasts', () => {
    const { container } = render(<Harness initial={[]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('shows a reminder with Done, Snooze 15 min and Main chat, and keeps it', () => {
    render(<Harness initial={[makeToast('m1', reminder)]} />);
    const toast = screen.getByRole('article', { name: 'Reminder Call the bank' });
    expect(within(toast).getByText(/Reminder · /)).toBeVisible();
    expect(within(toast).getByText('Their cut-off is 3:00 PM.')).toBeVisible();
    expect(within(toast).getByRole('button', { name: /Done/ })).toBeVisible();
    expect(
      within(toast).getByRole('button', { name: /Snooze 15 min/ }),
    ).toBeVisible();
    expect(
      within(toast).getByRole('button', { name: 'Main chat →' }),
    ).toBeVisible();
    expect(within(toast).getByText('Stays until you act')).toBeVisible();

    act(() => {
      vi.advanceTimersByTime(TOAST_AUTO_DISMISS_MS * 10);
    });
    expect(
      screen.getByRole('article', { name: 'Reminder Call the bank' }),
    ).toBeVisible();
  });

  it('leaves a report by itself after eight seconds', () => {
    render(<Harness initial={[makeToast('m2', digest)]} />);
    expect(screen.getByRole('article', { name: 'Inbox digest posted' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Show →' })).toBeVisible();

    act(() => {
      vi.advanceTimersByTime(TOAST_AUTO_DISMISS_MS - 1);
    });
    expect(screen.getByRole('article', { name: 'Inbox digest posted' })).toBeVisible();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(screen.queryByRole('article')).toBeNull();
  });

  it('holds a quiet toast while the pointer is on it, then waits again', () => {
    render(<Harness initial={[makeToast('m3', note)]} />);
    const toast = screen.getByRole('article', { name: 'Weekly review sent to Discord' });
    expect(within(toast).getByRole('button', { name: 'Open →' })).toBeVisible();

    act(() => {
      vi.advanceTimersByTime(5_000);
    });
    fireEvent.mouseEnter(toast);
    act(() => {
      vi.advanceTimersByTime(TOAST_AUTO_DISMISS_MS * 3);
    });
    expect(screen.getByRole('article', { name: 'Weekly review sent to Discord' })).toBeVisible();

    fireEvent.mouseLeave(toast);
    act(() => {
      vi.advanceTimersByTime(TOAST_AUTO_DISMISS_MS);
    });
    expect(screen.queryByRole('article')).toBeNull();
  });

  it('stacks the newest on top and shows three at most', () => {
    let stack: AutomationToast[] = [];
    for (const [id, name] of [
      ['1', 'One'],
      ['2', 'Two'],
      ['3', 'Three'],
      ['4', 'Four'],
    ])
      stack = pushToast(stack, makeToast(id, { ...digest, automationName: name }));
    render(<Harness initial={stack} />);
    const titles = screen
      .getAllByRole('heading', { level: 3 })
      .map((heading) => heading.textContent);
    expect(titles).toEqual(['Four posted', 'Three posted', 'Two posted']);
  });

  it('opens what a toast announced and clears it', async () => {
    vi.useRealTimers();
    const user = userEvent.setup();
    const onOpen = vi.fn();
    const toast = makeToast('m4', digest);
    render(<Harness initial={[toast]} onOpen={onOpen} />);
    await user.click(screen.getByRole('button', { name: 'Show →' }));
    expect(onOpen).toHaveBeenCalledWith(toast);
  });

  it('closes a toast with its close button', async () => {
    vi.useRealTimers();
    const user = userEvent.setup();
    render(<Harness initial={[makeToast('m5', reminder)]} />);
    await user.click(screen.getByRole('button', { name: 'Dismiss reminder' }));
    expect(screen.queryByRole('article')).toBeNull();
  });

  it('answers Done and Snooze from the toast and clears it', async () => {
    vi.useRealTimers();
    const user = userEvent.setup();
    const onDone = vi.fn().mockResolvedValue(true);
    const onSnooze = vi.fn().mockResolvedValue(true);
    const { unmount } = render(
      <Harness initial={[makeToast('m6', reminder)]} onDone={onDone} onSnooze={onSnooze} />,
    );
    await user.click(screen.getByRole('button', { name: /Done/ }));
    expect(onDone).toHaveBeenCalledWith(due);
    await vi.waitFor(() => expect(screen.queryByRole('article')).toBeNull());
    unmount();

    render(
      <Harness initial={[makeToast('m7', reminder)]} onDone={onDone} onSnooze={onSnooze} />,
    );
    await user.click(screen.getByRole('button', { name: /Snooze 15 min/ }));
    expect(onSnooze).toHaveBeenCalledWith(due, 15);
    await vi.waitFor(() => expect(screen.queryByRole('article')).toBeNull());
  });

  it('drops a reminder toast once it is no longer due', () => {
    const done = automationFixture('rem', {
      trigger: { type: 'once', atMs: FIRED },
      enabled: false,
      lastFiredAtMs: FIRED,
      doneAtMs: FIRED + 1,
    });
    render(
      <Harness initial={[makeToast('m8', reminder)]} automations={[done]} />,
    );
    expect(screen.queryByRole('article')).toBeNull();
  });
});
