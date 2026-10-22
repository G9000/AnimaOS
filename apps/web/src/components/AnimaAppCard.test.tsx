import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import {
  DaemonConnectionError,
  DaemonHttpError,
  type SupervisorStatusJson,
  type SystemInFlight,
  type SystemStatus,
} from '@animaos/sdk';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { AnimaAppCard } from './AnimaAppCard';

const STOPPED_COPY =
  'Anima is stopped. Start it from the tray or run `anima start`.';

const noWork: SystemInFlight = {
  chatRuns: 0,
  queuedRuns: 0,
  swarmRuns: 0,
  helperRuns: 0,
  jobs: 0,
  backgroundProcesses: 0,
  appSessions: 0,
  acpSessions: 0,
};

function supervised(
  overrides: Partial<SupervisorStatusJson> = {},
): SupervisorStatusJson {
  return {
    version: '0.3.0',
    state: { kind: 'running' },
    serverUrl: 'http://127.0.0.1:8787',
    workspace: '/Users/me/anima-workspace',
    workspaceProblem: null,
    dataDir: '/Users/me/.animaos-swarm/data',
    autostart: true,
    connectors: 1,
    pending: null,
    update: { available: null, lastError: null },
    ...overrides,
  };
}

function draining(
  inFlight: Partial<SystemInFlight>,
  overrides: Partial<SupervisorStatusJson> = {},
): SupervisorStatusJson {
  return supervised({
    state: {
      kind: 'draining',
      action: 'stop',
      inFlight: { ...noWork, ...inFlight },
    },
    pending: 'stop',
    ...overrides,
  });
}

function mockClient(status: SystemStatus) {
  return {
    status: vi.fn().mockResolvedValue(status),
    action: vi.fn().mockResolvedValue(status),
  };
}

async function renderCard(client: ReturnType<typeof mockClient>) {
  render(<AnimaAppCard client={client} />);
  await act(async () => {});
}

beforeEach(() => {
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  vi.useRealTimers();
});

describe('AnimaAppCard', () => {
  it('shows the app facts and its buttons while running', async () => {
    await renderCard(mockClient(supervised()));

    expect(screen.getByRole('region', { name: 'Anima app' })).toBeVisible();
    expect(screen.getByText('v0.3.0')).toBeVisible();
    expect(screen.getByText('Running')).toBeVisible();
    expect(screen.getByText('/Users/me/anima-workspace')).toBeVisible();
    expect(screen.getByText('/Users/me/.animaos-swarm/data')).toBeVisible();
    expect(screen.getByText('On')).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Check for updates' }),
    ).toBeVisible();
    expect(screen.getByRole('button', { name: 'Restart' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Stop' })).toBeVisible();
  });

  it('warns about a workspace problem', async () => {
    await renderCard(
      mockClient(
        supervised({ workspace: null, workspaceProblem: 'Folder is missing' }),
      ),
    );
    expect(screen.getByText('Folder is missing')).toBeVisible();
  });

  it('offers the available update and installs it', async () => {
    const client = mockClient(
      supervised({ update: { available: '0.4.0', lastError: null } }),
    );
    await renderCard(client);

    await userEvent
      .setup({ advanceTimers: vi.advanceTimersByTime })
      .click(screen.getByRole('button', { name: 'Update to v0.4.0' }));
    expect(client.action).toHaveBeenCalledWith('update');
  });

  it('checks for updates when none is known', async () => {
    const client = mockClient(supervised());
    await renderCard(client);

    await userEvent
      .setup({ advanceTimers: vi.advanceTimersByTime })
      .click(screen.getByRole('button', { name: 'Check for updates' }));
    expect(client.action).toHaveBeenCalledWith('check-update');
  });

  it('shows the drain with the work still going', async () => {
    await renderCard(mockClient(draining({ chatRuns: 2, jobs: 1 })));

    expect(screen.getByText('Waiting for 3 tasks to finish')).toBeVisible();
    expect(screen.getByText('Agent runs: 2')).toBeVisible();
    expect(screen.getByText('Jobs: 1')).toBeVisible();
    expect(screen.queryByText(/Swarm runs/)).toBeNull();
    expect(screen.getByRole('button', { name: 'Stop now' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Restart' })).toBeNull();
  });

  it('counts native learning, repair, command, and sign-in work during a drain', async () => {
    await renderCard(
      mockClient(
        draining({
          memoryReviews: 1,
          doctorRepairs: 1,
          standingIntentChanges: 1,
          executionJobs: 1,
          mcpOperations: 1,
          browserProfileLogins: 1,
        }),
      ),
    );

    expect(screen.getByText('Waiting for 6 tasks to finish')).toBeVisible();
    for (const label of [
      'Learning reviews',
      'Repairs',
      'Saved instruction changes',
      'Commands',
      'Connected tool operations',
      'Browser sign-ins',
    ]) {
      expect(screen.getByText(`${label}: 1`)).toBeVisible();
    }
    expect(screen.getByRole('button', { name: 'Stop now' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeVisible();
  });

  it('names what happens after the drain', async () => {
    await renderCard(mockClient(draining({ jobs: 1 })));

    expect(screen.getByText('Anima will stop.')).toBeVisible();
  });

  it('offers Quit now while a quit waits for work', async () => {
    await renderCard(
      mockClient(
        draining(
          { jobs: 1 },
          {
            state: {
              kind: 'draining',
              action: 'quit',
              inFlight: { ...noWork, jobs: 1 },
            },
            pending: 'quit',
          },
        ),
      ),
    );

    expect(screen.getByText('Anima will quit.')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Quit now' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Stop now' })).toBeNull();
  });

  it('Stop now forces the draining action', async () => {
    const client = mockClient(draining({ chatRuns: 2, jobs: 1 }));
    await renderCard(client);

    await userEvent
      .setup({ advanceTimers: vi.advanceTimersByTime })
      .click(screen.getByRole('button', { name: 'Stop now' }));
    expect(client.action).toHaveBeenCalledWith('stop', { force: true });
  });

  it('Cancel ends the drain, and hides while the cancel is confirmed', async () => {
    const client = mockClient(draining({ jobs: 1 }));
    await renderCard(client);
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });

    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(client.action).toHaveBeenCalledWith('cancel');

    client.status.mockResolvedValue(
      draining({ jobs: 1 }, { pending: 'cancel' }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(screen.getByText('Cancelling…')).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Cancel' })).toBeNull();
  });

  it('shows the stopped message after Stop succeeds', async () => {
    const client = mockClient(supervised());
    client.action.mockResolvedValue(supervised({ state: { kind: 'stopped' } }));
    await renderCard(client);

    await userEvent
      .setup({ advanceTimers: vi.advanceTimersByTime })
      .click(screen.getByRole('button', { name: 'Stop' }));

    expect(client.action).toHaveBeenCalledWith('stop');
    expect(screen.getByText(STOPPED_COPY)).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Restart' })).toBeNull();
  });

  it('keeps showing the drain when Stop starts one', async () => {
    const client = mockClient(supervised());
    client.action.mockResolvedValue(draining({ jobs: 2 }));
    await renderCard(client);

    await userEvent
      .setup({ advanceTimers: vi.advanceTimersByTime })
      .click(screen.getByRole('button', { name: 'Stop' }));

    expect(screen.getByText('Waiting for 2 tasks to finish')).toBeVisible();
    expect(screen.queryByText(STOPPED_COPY)).toBeNull();
  });

  it('shows the stopped message when the status stops answering after a Stop', async () => {
    const client = mockClient(supervised());
    client.action.mockResolvedValue(draining({ jobs: 1 }));
    await renderCard(client);

    await userEvent
      .setup({ advanceTimers: vi.advanceTimersByTime })
      .click(screen.getByRole('button', { name: 'Stop' }));
    client.status.mockRejectedValue(
      new DaemonConnectionError('', new TypeError('fetch failed')),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    expect(screen.getByText(STOPPED_COPY)).toBeVisible();
  });

  it('shows the stopped message when Quit drops the connection', async () => {
    const client = mockClient(supervised());
    client.action.mockRejectedValue(
      new DaemonConnectionError('', new TypeError('fetch failed')),
    );
    await renderCard(client);

    // Quit is not a button here; a drain's forced quit is the same call.
    client.status.mockResolvedValue(
      draining(
        { jobs: 1 },
        {
          state: {
            kind: 'draining',
            action: 'quit',
            inFlight: { ...noWork, jobs: 1 },
          },
          pending: 'quit',
        },
      ),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    await userEvent
      .setup({ advanceTimers: vi.advanceTimersByTime })
      .click(screen.getByRole('button', { name: 'Quit now' }));

    expect(client.action).toHaveBeenCalledWith('quit', { force: true });
    expect(screen.getByText(STOPPED_COPY)).toBeVisible();
  });

  it('shows a refused action as text', async () => {
    const client = mockClient(supervised());
    client.action.mockRejectedValue(
      new DaemonHttpError(409, { error: 'Another action is in progress' }),
    );
    await renderCard(client);

    await userEvent
      .setup({ advanceTimers: vi.advanceTimersByTime })
      .click(screen.getByRole('button', { name: 'Restart' }));

    expect(screen.getByRole('alert')).toHaveTextContent(
      'Another action is in progress',
    );
    expect(screen.getByRole('button', { name: 'Restart' })).toBeEnabled();
  });

  it('shows a crash and when it retries', async () => {
    await renderCard(
      mockClient(
        supervised({
          state: { kind: 'crashed', message: 'port in use', retryInSecs: 4 },
        }),
      ),
    );
    expect(screen.getByText('Crashed')).toBeVisible();
    expect(screen.getByText(/port in use/)).toBeVisible();
    expect(screen.getByText(/4 seconds/)).toBeVisible();
  });

  it('polls the status every two seconds', async () => {
    const client = mockClient(supervised());
    await renderCard(client);
    expect(client.status).toHaveBeenCalledTimes(1);

    client.status.mockResolvedValue(
      supervised({ state: { kind: 'updating', stage: 'Downloading' } }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });

    expect(client.status).toHaveBeenCalledTimes(2);
    expect(screen.getByText(/Updating/)).toBeVisible();
    expect(screen.getByRole('button', { name: 'Stop' })).toBeDisabled();
  });

  it('shows only the version without the Anima app', async () => {
    await renderCard(mockClient({ supervised: false, version: '0.3.0' }));

    expect(
      screen.getByText('v0.3.0 (running without the Anima app)'),
    ).toBeVisible();
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('renders nothing when the daemon has no system route', async () => {
    const client = mockClient(supervised());
    client.status.mockRejectedValue(new DaemonHttpError(404, {}));
    const { container } = render(<AnimaAppCard client={client} />);
    await act(async () => {});

    expect(container).toBeEmptyDOMElement();
  });

  it('says so when the supervisor is not responding', async () => {
    const client = mockClient(supervised());
    client.status.mockRejectedValue(
      new DaemonHttpError(502, {
        error: 'The Anima supervisor is not responding',
      }),
    );
    await renderCard(client);

    expect(screen.getByRole('alert')).toHaveTextContent(
      'The Anima supervisor is not responding',
    );
  });

  it('renders nothing while the daemon is unreachable', async () => {
    const client = mockClient(supervised());
    client.status.mockRejectedValue(
      new DaemonConnectionError('', new TypeError('fetch failed')),
    );
    const { container } = render(<AnimaAppCard client={client} />);
    await act(async () => {});

    expect(container).toBeEmptyDOMElement();
  });
});
