import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { Automation, AutomationRun } from '@animaos/sdk';

import { daemon } from '../../lib/daemon-api';
import { AutomationHistory } from './AutomationHistory';

const schedule: Automation = {
  id: 'schedule-1',
  agentId: 'agent-1',
  name: 'Morning check-in',
  prompt: 'Review the morning plan',
  trigger: { type: 'interval', intervalMs: 60_000 },
  activeHours: null,
  enabled: false,
  target: { type: 'workspace' },
  nextDueAtMs: 20,
  lastFiredAtMs: 10,
  doneAtMs: null,
  removesAtMs: null,
  lastOutcome: {
    status: 'error',
    occurredAtMs: 10,
    errorCode: 'schedule_run_interrupted',
  },
  running: false,
  createdBy: { kind: 'owner' },
  preset: null,
  counters: { runs: 1, failures: 1, consecutiveFailures: 1 },
  importIdempotencyKey: null,
  createdAtMs: 1,
  updatedAtMs: 10,
};

function occurrence(overrides: Partial<AutomationRun> = {}): AutomationRun {
  return {
    id: 'occurrence-1',
    scheduleId: schedule.id,
    agentId: schedule.agentId,
    firedAtMs: 10,
    finishedAtMs: 12,
    outcome: 'failed',
    runId: 'run-1',
    sessionId: 'schedule:schedule-1',
    errorCode: 'schedule_run_interrupted',
    manual: false,
    ...overrides,
  };
}

type HistoryProps = Parameters<typeof AutomationHistory>[0];
type PropsWithOnline = HistoryProps;

function view(props: PropsWithOnline) {
  return <AutomationHistory {...props} />;
}

function props(overrides: Partial<PropsWithOnline> = {}): PropsWithOnline {
  return {
    agentId: schedule.agentId,
    automation: schedule,
    version: 1,
    online: true,
    onClose: vi.fn(),
    onShowInChat: vi.fn(),
    ...overrides,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((accept, decline) => {
    resolve = accept;
    reject = decline;
  });
  return { promise, resolve, reject };
}

describe('AutomationHistory', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('marks the interrupted occurrence for review and preserves its warning beside later success', async () => {
    vi.spyOn(daemon, 'automationHistory').mockResolvedValue([
      occurrence({
        id: 'later-success',
        firedAtMs: 20,
        outcome: 'spoke',
        errorCode: null,
      }),
      occurrence(),
    ]);

    render(view(props()));

    const warning = await screen.findByText('Needs review');
    expect(warning).toBeVisible();
    expect(
      screen.getByText(/actions may have (already )?happened/i),
    ).toBeVisible();
    expect(screen.getByText('Replied')).toBeVisible();
    expect(screen.getByText('schedule_run_interrupted')).toBeVisible();
  });

  it('shows an interrupted occurrence in the main chat by its run', async () => {
    const user = userEvent.setup();
    const onShowInChat = vi.fn();
    vi.spyOn(daemon, 'automationHistory').mockResolvedValue([occurrence()]);

    render(view(props({ onShowInChat })));
    await user.click(await screen.findByRole('button', { name: 'Show in chat' }));

    expect(onShowInChat).toHaveBeenCalledTimes(1);
    expect(onShowInChat).toHaveBeenCalledWith({
      automationId: 'schedule-1',
      runId: 'run-1',
    });
    expect(screen.queryByRole('button', { name: 'Review thread' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Open' })).toBeNull();
  });

  it('keeps the safety text meaningful without a thread to review', async () => {
    vi.spyOn(daemon, 'automationHistory').mockResolvedValue([
      occurrence({ sessionId: null }),
    ]);

    render(view(props()));

    expect(await screen.findByText(/check what it did in the main chat/i)).toBeVisible();
    expect(screen.queryByText(/saved thread/i)).not.toBeInTheDocument();
  });

  it('offers nothing to show for a run that had nothing to report', async () => {
    vi.spyOn(daemon, 'automationHistory').mockResolvedValue([
      occurrence({ outcome: 'silent', errorCode: null }),
    ]);

    render(view(props()));

    await screen.findByText('Nothing to report');
    expect(screen.queryByRole('button', { name: 'Show in chat' })).toBeNull();
  });

  it('keeps ordinary failure and stopped occurrences on their existing labels', async () => {
    vi.spyOn(daemon, 'automationHistory').mockResolvedValue([
      occurrence({
        id: 'stopped',
        outcome: 'stopped',
        errorCode: 'schedule_stopped',
      }),
      occurrence({ id: 'failed', errorCode: 'schedule_run_failed' }),
    ]);

    render(view(props()));

    expect(await screen.findByText('Failed')).toBeVisible();
    expect(screen.getByText('Stopped')).toBeVisible();
    expect(screen.queryByText('Needs review')).not.toBeInTheDocument();
  });

  it('hides prior rows and aborts the prior request when only the schedule changes', async () => {
    const pending = deferred<AutomationRun[]>();
    const history = vi
      .spyOn(daemon, 'automationHistory')
      .mockResolvedValueOnce([occurrence({ errorCode: 'old-history' })])
      .mockReturnValueOnce(pending.promise);
    const first = props();
    const { rerender } = render(view(first));
    expect(await screen.findByText('old-history')).toBeVisible();
    const previousSignal = history.mock.calls[0]?.[2]?.signal;

    rerender(
      view(
        props({
          automation: {
            ...schedule,
            id: 'schedule-2',
            name: 'Evening check-in',
          },
          version: 1,
        }),
      ),
    );

    expect(screen.queryByText('old-history')).not.toBeInTheDocument();
    expect(screen.getByText('Loading…')).toBeVisible();
    expect(previousSignal?.aborted).toBe(true);
    expect(history).toHaveBeenLastCalledWith(
      'agent-1',
      'schedule-2',
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  it('hides prior rows and aborts the prior request when only the agent changes', async () => {
    const staleSuccess = deferred<AutomationRun[]>();
    const current = deferred<AutomationRun[]>();
    const history = vi
      .spyOn(daemon, 'automationHistory')
      .mockResolvedValueOnce([occurrence({ errorCode: 'old-history' })])
      .mockReturnValueOnce(staleSuccess.promise)
      .mockReturnValueOnce(current.promise);
    const { rerender } = render(view(props()));
    expect(await screen.findByText('old-history')).toBeVisible();

    rerender(
      view(
        props({
          agentId: 'agent-2',
          automation: schedule,
        }),
      ),
    );
    expect(screen.queryByText('old-history')).not.toBeInTheDocument();
    expect(screen.getByText('Loading…')).toBeVisible();
    expect(history.mock.calls[0]?.[2]?.signal?.aborted).toBe(true);

    rerender(view(props({ agentId: 'agent-2', version: 2 })));
    expect(screen.queryByText('old-history')).not.toBeInTheDocument();
    expect(screen.getByText('Loading…')).toBeVisible();
    expect(history.mock.calls[1]?.[2]?.signal?.aborted).toBe(true);
    await act(async () =>
      current.resolve([
        occurrence({
          id: 'current',
          agentId: 'agent-2',
          errorCode: 'current-error',
        }),
      ]),
    );
    expect(await screen.findByText('current-error')).toBeVisible();

    await act(async () =>
      staleSuccess.resolve([occurrence({ errorCode: 'stale-success' })]),
    );
    expect(screen.getByText('current-error')).toBeVisible();
    expect(screen.queryByText('stale-success')).not.toBeInTheDocument();
  });

  it('hides prior rows and aborts the prior request when only history version changes', async () => {
    const staleError = deferred<AutomationRun[]>();
    const history = vi
      .spyOn(daemon, 'automationHistory')
      .mockResolvedValueOnce([occurrence({ errorCode: 'old-history' })])
      .mockReturnValueOnce(staleError.promise);
    const first = props();
    const { rerender } = render(view(first));
    expect(await screen.findByText('old-history')).toBeVisible();
    rerender(view(props({ version: 2 })));
    expect(screen.queryByText('old-history')).not.toBeInTheDocument();
    expect(screen.getByText('Loading…')).toBeVisible();
    expect(history.mock.calls[0]?.[2]?.signal?.aborted).toBe(true);

    const errorCurrent = deferred<AutomationRun[]>();
    history.mockReturnValueOnce(errorCurrent.promise);
    rerender(view(props({ version: 3 })));
    expect(screen.queryByText('old-history')).not.toBeInTheDocument();
    expect(history.mock.calls[1]?.[2]?.signal?.aborted).toBe(true);
    await act(async () =>
      errorCurrent.resolve([occurrence({ errorCode: 'fresh' })]),
    );
    await act(async () => staleError.reject(new Error('stale failure')));
    expect(screen.getByText('fresh')).toBeVisible();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('does not request history when first opened offline and refreshes after reconnect', async () => {
    const history = vi
      .spyOn(daemon, 'automationHistory')
      .mockResolvedValue([occurrence({ errorCode: 'after-reconnect' })]);
    const { rerender } = render(view(props({ online: false })));

    expect(screen.getByText(/offline/i)).toBeVisible();
    expect(history).not.toHaveBeenCalled();

    rerender(view(props({ online: true })));
    expect(await screen.findByText('after-reconnect')).toBeVisible();
    expect(history).toHaveBeenCalledTimes(1);
  });

  it('keeps Close available while loading, empty, offline, or failed', async () => {
    const user = userEvent.setup();
    const pending = deferred<AutomationRun[]>();
    vi.spyOn(daemon, 'automationHistory')
      .mockReturnValueOnce(pending.promise)
      .mockResolvedValueOnce([])
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValue([]);
    const onClose = vi.fn();
    const current = props({ onClose });
    const { rerender } = render(view(current));

    expect(screen.getByText('Loading…')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Close history' }));
    expect(onClose).toHaveBeenCalledTimes(1);

    rerender(view(props({ ...current, version: 2 })));
    expect(await screen.findByText('It has not run yet.')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Close history' }));

    rerender(view(props({ ...current, version: 3 })));
    expect(await screen.findByRole('alert')).toBeVisible();
    await user.click(screen.getByRole('button', { name: 'Close history' }));
    expect(onClose).toHaveBeenCalledTimes(3);

    rerender(view(props({ ...current, version: 4, online: false })));
    expect(screen.getByRole('button', { name: 'Close history' })).toBeVisible();
  });
});
