import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, it, vi } from 'vitest';
import type { AgentTasks } from '@animaOS-SWARM/sdk';
import {
  daemon,
  type DaemonSchedule,
  type TelegramConnector,
} from '../lib/daemon-api';
import { AgentProactiveView, AgentTasksView } from './AgentWork';

afterEach(() => vi.restoreAllMocks());

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function taskList(content: string, revision = 'original'): AgentTasks {
  return {
    revision,
    tasks: [{ content, activeForm: content, status: 'pending' }],
  };
}

it('retains a saved cancelled status while editing an existing task', async () => {
  const saved: AgentTasks = {
    revision: 'cancelled',
    tasks: [
      {
        id: 'stopped',
        content: 'Stopped work',
        activeForm: 'Stopping work',
        status: 'cancelled',
      },
    ],
  };
  vi.spyOn(daemon, 'agentTasks').mockResolvedValue(saved);
  const update = vi
    .spyOn(daemon, 'updateAgentTasks')
    .mockImplementation(async (_id, payload) => payload);
  render(<AgentTasksView agentId="researcher" name="Researcher" online />);
  expect(await screen.findByLabelText('Task 1 status')).toHaveValue(
    'cancelled',
  );
  expect(screen.getByRole('option', { name: 'Cancelled' })).toBeDisabled();
  fireEvent.change(screen.getByLabelText('Task 1'), {
    target: { value: 'Stopped work with saved notes' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  await waitFor(() =>
    expect(update).toHaveBeenCalledWith('researcher', {
      revision: saved.revision,
      tasks: [
        {
          ...saved.tasks[0],
          content: 'Stopped work with saved notes',
          activeForm: 'Stopped work with saved notes',
        },
      ],
    }),
  );
});

it('FP03 gives an ID-aware appended task one stable identity across failed save retries', async () => {
  const generated = '12345678-1234-1234-1234-123456789012';
  const uuid = vi.spyOn(crypto, 'randomUUID').mockReturnValue(generated);
  const saved: AgentTasks = {
    revision: 'structured',
    tasks: [
      {
        id: 'root',
        content: 'Root work',
        activeForm: 'Working',
        status: 'pending',
      },
    ],
  };
  vi.spyOn(daemon, 'agentTasks').mockResolvedValue(saved);
  const update = vi
    .spyOn(daemon, 'updateAgentTasks')
    .mockRejectedValueOnce(new Error('Save unavailable'))
    .mockImplementation(async (_id, payload) => payload);
  render(<AgentTasksView agentId="researcher" name="Researcher" online />);
  fireEvent.change(await screen.findByLabelText('New agent task'), {
    target: { value: 'New work' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Add task' }));
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Save unavailable',
  );
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  await waitFor(() => expect(update).toHaveBeenCalledTimes(2));
  const first = update.mock.calls[0][1];
  expect(first.tasks[1]).toEqual({
    id: generated,
    content: 'New work',
    activeForm: 'New work',
    status: 'pending',
  });
  expect(update.mock.calls[1][1]).toEqual(first);
  expect(uuid).toHaveBeenCalledTimes(1);
});

function telegramConnector(approved: boolean): TelegramConnector {
  return {
    id: 'tg-1',
    agentId: 'agent-main',
    roomId: 'telegram:tg-1',
    type: 'telegram',
    bot: { id: '1', username: 'nova_bot', displayName: 'Nova' },
    approvedChat: approved
      ? { id: '42', kind: 'private', title: null, username: 'owner' }
      : null,
    pendingPairing: null,
    status: 'ready',
    enabled: true,
    createdAtMs: 1,
    updatedAtMs: 1,
  };
}

function createdSchedule(target: DaemonSchedule['target']): DaemonSchedule {
  return {
    id: 'schedule-1',
    importIdempotencyKey: null,
    agentId: 'agent-main',
    prompt: 'Check my goals',
    trigger: { type: 'interval', intervalMs: 3_600_000 },
    enabled: true,
    target,
    nextDueAtMs: 2,
    lastFiredAtMs: null,
    lastOutcome: null,
    createdAtMs: 1,
    updatedAtMs: 1,
  };
}

it('keeps Telegram delivery off until a chat is approved, then creates a Telegram check-in', async () => {
  const user = userEvent.setup();
  vi.spyOn(daemon, 'listSchedules').mockResolvedValue({ schedules: [] });
  vi.spyOn(daemon, 'listConnectors')
    .mockResolvedValueOnce({ connectors: [telegramConnector(false)] })
    .mockResolvedValue({ connectors: [telegramConnector(true)] });
  const create = vi.spyOn(daemon, 'createSchedule').mockResolvedValue({
    schedule: createdSchedule({ type: 'connector', connectorId: 'tg-1' }),
  });
  render(<AgentProactiveView agentId="agent-main" name="Nova" />);

  await screen.findByText(
    'Proactive work is off. No schedules configured for Nova.',
  );
  const target = screen.getByRole('combobox', { name: 'Deliver to' });
  expect(target).toHaveValue('workspace');
  expect(screen.getByRole('option', { name: 'Telegram' })).toBeDisabled();
  expect(target).toHaveAccessibleDescription(
    'Approve a Telegram chat in Connectors to deliver check-ins there.',
  );

  await user.click(screen.getByRole('button', { name: 'Refresh schedules' }));
  await waitFor(() =>
    expect(screen.getByRole('option', { name: 'Telegram' })).toBeEnabled(),
  );
  await user.selectOptions(target, 'telegram');
  await user.type(
    screen.getByRole('textbox', { name: 'Proactive instructions' }),
    'Check my goals',
  );
  await user.click(screen.getByRole('button', { name: 'Enable schedule' }));

  expect(create).toHaveBeenCalledWith('agent-main', {
    prompt: 'Check my goals',
    trigger: { type: 'interval', intervalMs: 3_600_000 },
    target: { type: 'connector', connectorId: 'tg-1' },
    enabled: true,
  });
});

it('shows schedules without waiting for connectors and says when they cannot be loaded', async () => {
  vi.spyOn(daemon, 'listSchedules').mockResolvedValue({ schedules: [] });
  let failConnectors!: (reason: unknown) => void;
  vi.spyOn(daemon, 'listConnectors').mockReturnValue(
    new Promise((_, reject) => {
      failConnectors = reject;
    }),
  );
  render(<AgentProactiveView agentId="agent-main" name="Nova" />);

  expect(
    await screen.findByText(
      'Proactive work is off. No schedules configured for Nova.',
    ),
  ).toBeVisible();
  expect(screen.getByRole('option', { name: 'Telegram' })).toBeDisabled();

  await act(async () => failConnectors(new Error('connectors unavailable')));
  expect(
    screen.getByRole('combobox', { name: 'Deliver to' }),
  ).toHaveAccessibleDescription(
    'Connectors could not be loaded, so Telegram delivery is unavailable. Refresh schedules to try again.',
  );
  expect(screen.getByRole('option', { name: 'Telegram' })).toBeDisabled();
  expect(screen.queryByText(/Approve a Telegram chat/)).not.toBeInTheDocument();
});

it('creates workspace check-ins by default', async () => {
  const user = userEvent.setup();
  vi.spyOn(daemon, 'listSchedules').mockResolvedValue({ schedules: [] });
  vi.spyOn(daemon, 'listConnectors').mockResolvedValue({
    connectors: [telegramConnector(true)],
  });
  const create = vi
    .spyOn(daemon, 'createSchedule')
    .mockResolvedValue({ schedule: createdSchedule({ type: 'workspace' }) });
  render(<AgentProactiveView agentId="agent-main" name="Nova" />);

  await user.type(
    await screen.findByRole('textbox', { name: 'Proactive instructions' }),
    'Check my goals',
  );
  await user.click(screen.getByRole('button', { name: 'Enable schedule' }));

  expect(create).toHaveBeenCalledWith(
    'agent-main',
    expect.objectContaining({ target: { type: 'workspace' } }),
  );
});

it('retains the task draft when a newer list causes a conflict', async () => {
  vi.spyOn(daemon, 'agentTasks').mockResolvedValue({
    tasks: [],
    revision: 'old',
  });
  const update = vi
    .spyOn(daemon, 'updateAgentTasks')
    .mockRejectedValue(
      new Error('Tasks changed. Refresh before saving again.'),
    );
  render(<AgentTasksView agentId="researcher" name="Researcher" online />);
  fireEvent.change(await screen.findByLabelText('New agent task'), {
    target: { value: 'Research competitors' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Add task' }));
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Tasks changed.');
  expect(screen.getByLabelText('Task 1')).toHaveValue('Research competitors');
  expect(update).toHaveBeenCalledWith('researcher', {
    revision: 'old',
    tasks: [
      {
        content: 'Research competitors',
        activeForm: 'Research competitors',
        status: 'pending',
      },
    ],
  });
});

it('allows drafting but prevents saving while the agent is running', async () => {
  vi.spyOn(daemon, 'agentTasks').mockResolvedValue({
    tasks: [],
    revision: 'old',
  });
  render(
    <AgentTasksView agentId="researcher" name="Researcher" online running />,
  );
  fireEvent.change(await screen.findByLabelText('New agent task'), {
    target: { value: 'Research competitors' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Add task' }));
  expect(screen.getByRole('button', { name: 'Save tasks' })).toBeDisabled();
});

it('does not read or dispatch task actions while initially offline', () => {
  const read = vi
    .spyOn(daemon, 'agentTasks')
    .mockResolvedValue(taskList('Saved'));
  const update = vi.spyOn(daemon, 'updateAgentTasks');
  render(
    <AgentTasksView agentId="researcher" name="Researcher" online={false} />,
  );

  expect(screen.getByText(/offline|disconnected/i)).toBeVisible();
  const refresh = screen.getByRole('button', { name: 'Refresh tasks' });
  expect(refresh).toBeDisabled();
  fireEvent.click(refresh);
  expect(read).not.toHaveBeenCalled();
  expect(update).not.toHaveBeenCalled();
  expect(screen.queryByDisplayValue('Saved')).not.toBeInTheDocument();
});

it.each([
  ['success', 'offline'],
  ['error', 'offline'],
  ['success', 'reconnected'],
  ['error', 'reconnected'],
] as const)(
  'ignores a late read %s while %s and performs one fresh reconnect read',
  async (outcome, phase) => {
    const old = deferred<AgentTasks>();
    const fresh = deferred<AgentTasks>();
    const read = vi
      .spyOn(daemon, 'agentTasks')
      .mockReturnValueOnce(old.promise)
      .mockReturnValueOnce(fresh.promise);
    const view = render(
      <AgentTasksView agentId="researcher" name="Researcher" online />,
    );
    expect(read).toHaveBeenCalledTimes(1);
    view.rerender(
      <AgentTasksView agentId="researcher" name="Researcher" online={false} />,
    );
    expect(
      screen.getByRole('button', { name: 'Refresh tasks' }),
    ).toBeDisabled();

    const reconnect = () => {
      view.rerender(
        <AgentTasksView agentId="researcher" name="Researcher" online />,
      );
      expect(read).toHaveBeenCalledTimes(2);
      expect(screen.getByText('Loading tasks…')).toBeVisible();
    };
    if (phase === 'reconnected') reconnect();
    await act(async () => {
      if (outcome === 'success') old.resolve(taskList('Obsolete tasks'));
      else old.reject(new Error('Obsolete read failure'));
      await old.promise.catch(() => undefined);
    });
    expect(
      screen.queryByDisplayValue('Obsolete tasks'),
    ).not.toBeInTheDocument();
    expect(screen.queryByText('Obsolete read failure')).not.toBeInTheDocument();
    if (phase === 'offline') reconnect();

    await act(async () => fresh.resolve(taskList('Fresh tasks', 'fresh')));
    expect(await screen.findByLabelText('Task 1')).toHaveValue('Fresh tasks');
    expect(screen.getByLabelText('Task 1')).toBeEnabled();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(read).toHaveBeenCalledTimes(2);
  },
);

it('retains same-agent task edits and unfinished text read-only until an unchanged revision is revalidated', async () => {
  const fresh = deferred<AgentTasks>();
  const read = vi
    .spyOn(daemon, 'agentTasks')
    .mockResolvedValueOnce(taskList('Saved task'))
    .mockReturnValueOnce(fresh.promise);
  const update = vi
    .spyOn(daemon, 'updateAgentTasks')
    .mockResolvedValue(taskList('Owner draft', 'saved'));
  const view = render(
    <AgentTasksView agentId="researcher" name="Researcher" online />,
  );
  fireEvent.change(await screen.findByLabelText('Task 1'), {
    target: { value: 'Owner draft' },
  });
  fireEvent.change(screen.getByLabelText('New agent task'), {
    target: { value: 'Unfinished next task' },
  });
  view.rerender(
    <AgentTasksView agentId="researcher" name="Researcher" online={false} />,
  );

  expect(screen.getByLabelText('Task 1')).toHaveValue('Owner draft');
  expect(screen.getByLabelText('New agent task')).toHaveValue(
    'Unfinished next task',
  );
  expect(screen.getByText(/unsaved/i)).toBeVisible();
  for (const control of [
    screen.getByLabelText('Task 1'),
    screen.getByLabelText('New agent task'),
    screen.getByLabelText('Task 1 status'),
    screen.getByRole('button', { name: 'Remove task 1' }),
    screen.getByRole('button', { name: 'Add task' }),
    screen.getByRole('button', { name: 'Save tasks' }),
    screen.getByRole('button', { name: 'Refresh tasks' }),
  ])
    expect(control).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  fireEvent.submit(screen.getByLabelText('New agent task').closest('form')!);
  expect(screen.queryByLabelText('Task 2')).not.toBeInTheDocument();
  expect(update).not.toHaveBeenCalled();

  view.rerender(
    <AgentTasksView agentId="researcher" name="Researcher" online />,
  );
  expect(read).toHaveBeenCalledTimes(2);
  expect(screen.getByLabelText('Task 1')).toBeDisabled();
  expect(screen.getByLabelText('New agent task')).toHaveValue(
    'Unfinished next task',
  );
  expect(screen.getByRole('button', { name: 'Save tasks' })).toBeDisabled();
  await act(async () => fresh.resolve(taskList('Saved task')));

  expect(screen.getByLabelText('Task 1')).toHaveValue('Owner draft');
  expect(screen.getByLabelText('New agent task')).toHaveValue(
    'Unfinished next task',
  );
  expect(screen.getByRole('button', { name: 'Save tasks' })).toBeEnabled();
  expect(update).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  await screen.findByRole('button', { name: 'Tasks saved' });
  expect(update).toHaveBeenCalledExactlyOnceWith('researcher', {
    tasks: taskList('Owner draft').tasks,
    revision: 'original',
  });
});

it('keeps the original draft baseline after a changed reconnect revision until explicit discard uses the fetched list', async () => {
  const fresh = deferred<AgentTasks>();
  const read = vi
    .spyOn(daemon, 'agentTasks')
    .mockResolvedValueOnce(taskList('Original task'))
    .mockReturnValueOnce(fresh.promise);
  const update = vi.spyOn(daemon, 'updateAgentTasks');
  const view = render(
    <AgentTasksView agentId="researcher" name="Researcher" online />,
  );
  fireEvent.change(await screen.findByLabelText('Task 1'), {
    target: { value: 'Owner draft' },
  });
  fireEvent.change(screen.getByLabelText('New agent task'), {
    target: { value: 'Still composing' },
  });
  view.rerender(
    <AgentTasksView agentId="researcher" name="Researcher" online={false} />,
  );
  view.rerender(
    <AgentTasksView agentId="researcher" name="Researcher" online />,
  );
  await act(async () =>
    fresh.resolve(taskList('Changed in another room', 'new')),
  );

  expect(screen.getByLabelText('Task 1')).toHaveValue('Owner draft');
  expect(screen.getByLabelText('New agent task')).toHaveValue(
    'Still composing',
  );
  expect(screen.getByRole('alert')).toHaveTextContent(/changed|review/i);
  expect(screen.getByRole('button', { name: 'Save tasks' })).toBeDisabled();
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  expect(update).not.toHaveBeenCalled();
  fireEvent.click(
    screen.getByRole('button', { name: 'Discard draft and load saved tasks' }),
  );

  expect(screen.getByLabelText('Task 1')).toHaveValue(
    'Changed in another room',
  );
  expect(screen.getByLabelText('New agent task')).toHaveValue('');
  expect(screen.getByRole('button', { name: 'Tasks saved' })).toBeDisabled();
  expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  expect(read).toHaveBeenCalledTimes(2);
  expect(update).not.toHaveBeenCalled();
});

it('requires explicit discard when manually refreshing a dirty task draft', async () => {
  const read = vi
    .spyOn(daemon, 'agentTasks')
    .mockResolvedValueOnce(taskList('Original task'))
    .mockResolvedValueOnce(taskList('Latest saved task', 'new'));
  render(<AgentTasksView agentId="researcher" name="Researcher" online />);
  fireEvent.change(await screen.findByLabelText('Task 1'), {
    target: { value: 'Owner draft' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Refresh tasks' }));
  expect(screen.getByLabelText('Task 1')).toHaveValue('Owner draft');
  fireEvent.click(
    await screen.findByRole('button', {
      name: 'Discard draft and load saved tasks',
    }),
  );

  expect(screen.getByLabelText('Task 1')).toHaveValue('Latest saved task');
  expect(read).toHaveBeenCalledTimes(2);
});

it('refreshes a clean list normally', async () => {
  const read = vi
    .spyOn(daemon, 'agentTasks')
    .mockResolvedValueOnce(taskList('Original task'))
    .mockResolvedValueOnce(taskList('Latest saved task', 'new'));
  render(<AgentTasksView agentId="researcher" name="Researcher" online />);
  await screen.findByDisplayValue('Original task');
  fireEvent.click(screen.getByRole('button', { name: 'Refresh tasks' }));

  expect(await screen.findByDisplayValue('Latest saved task')).toBeEnabled();
  expect(read).toHaveBeenCalledTimes(2);
  expect(
    screen.queryByRole('button', {
      name: 'Discard draft and load saved tasks',
    }),
  ).not.toBeInTheDocument();
});

it.each(['success', 'conflict', 'error'] as const)(
  'retains one original pending PUT across reconnect and ignores its obsolete %s',
  async (outcome) => {
    const pending = deferred<AgentTasks>();
    const read = vi
      .spyOn(daemon, 'agentTasks')
      .mockResolvedValue(taskList('Original task'));
    const update = vi
      .spyOn(daemon, 'updateAgentTasks')
      .mockReturnValue(pending.promise);
    const busy = vi.fn();
    const view = render(
      <AgentTasksView
        agentId="researcher"
        name="Researcher"
        online
        onBusyChange={busy}
      />,
    );
    fireEvent.change(await screen.findByLabelText('Task 1'), {
      target: { value: 'Submitted once' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
    expect(busy).toHaveBeenLastCalledWith(true);
    view.rerender(
      <AgentTasksView
        agentId="researcher"
        name="Researcher"
        online={false}
        onBusyChange={busy}
      />,
    );
    view.rerender(
      <AgentTasksView
        agentId="researcher"
        name="Researcher"
        online
        onBusyChange={busy}
      />,
    );
    await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
    expect(screen.getByLabelText('Task 1')).toHaveValue('Submitted once');
    const save = screen.getByRole('button', {
      name: /Saving tasks|Save tasks/,
    });
    expect(save).toBeDisabled();
    fireEvent.click(save);
    expect(update).toHaveBeenCalledExactlyOnceWith('researcher', {
      tasks: taskList('Submitted once').tasks,
      revision: 'original',
    });

    await act(async () => {
      if (outcome === 'success')
        pending.resolve(taskList('Obsolete acknowledgment', 'saved'));
      else
        pending.reject(
          new Error(
            outcome === 'conflict'
              ? 'Obsolete conflict'
              : 'Obsolete save error',
          ),
        );
      await pending.promise.catch(() => undefined);
    });
    expect(screen.getByLabelText('Task 1')).toHaveValue('Submitted once');
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(update).toHaveBeenCalledTimes(1);
    expect(read).toHaveBeenCalledTimes(2);
    expect(screen.getByRole('button', { name: 'Save tasks' })).toBeEnabled();
  },
);

it('settles busy ownership on unmount without allowing an old completion to clear a newer editor save', async () => {
  const old = deferred<AgentTasks>();
  const current = deferred<AgentTasks>();
  const read = vi
    .spyOn(daemon, 'agentTasks')
    .mockResolvedValue(taskList('Saved task'));
  const update = vi
    .spyOn(daemon, 'updateAgentTasks')
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(current.promise);
  const busy = vi.fn();
  const first = render(
    <AgentTasksView
      agentId="researcher"
      name="Researcher"
      online
      onBusyChange={busy}
    />,
  );
  fireEvent.change(await screen.findByLabelText('Task 1'), {
    target: { value: 'Researcher submission' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  first.unmount();
  expect(busy).toHaveBeenLastCalledWith(false);

  render(
    <AgentTasksView
      agentId="writer"
      name="Writer"
      online
      onBusyChange={busy}
    />,
  );
  fireEvent.change(await screen.findByLabelText('Task 1'), {
    target: { value: 'Writer submission' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  expect(busy).toHaveBeenLastCalledWith(true);
  const callsDuringCurrentSave = busy.mock.calls.length;
  await act(async () =>
    old.resolve(taskList('Researcher submission', 'old-saved')),
  );

  expect(busy).toHaveBeenCalledTimes(callsDuringCurrentSave);
  expect(screen.getByLabelText('Task 1')).toHaveValue('Writer submission');
  expect(screen.getByRole('button', { name: 'Saving tasks…' })).toBeDisabled();
  expect(update).toHaveBeenNthCalledWith(1, 'researcher', {
    tasks: taskList('Researcher submission').tasks,
    revision: 'original',
  });
  expect(update).toHaveBeenNthCalledWith(2, 'writer', {
    tasks: taskList('Writer submission').tasks,
    revision: 'original',
  });
  await act(async () =>
    current.resolve(taskList('Writer submission', 'new-saved')),
  );
  expect(busy).toHaveBeenLastCalledWith(false);
  expect(screen.getByRole('button', { name: 'Tasks saved' })).toBeDisabled();
  expect(read).toHaveBeenCalledTimes(2);
});

it('drops foreign drafts on agent change and ignores a late old-agent refresh', async () => {
  const old = deferred<AgentTasks>();
  const fresh = deferred<AgentTasks>();
  const read = vi
    .spyOn(daemon, 'agentTasks')
    .mockResolvedValueOnce(taskList('Researcher task'))
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(fresh.promise);
  const view = render(
    <AgentTasksView agentId="researcher" name="Researcher" online />,
  );
  fireEvent.change(await screen.findByLabelText('Task 1'), {
    target: { value: 'Researcher draft' },
  });
  fireEvent.change(screen.getByLabelText('New agent task'), {
    target: { value: 'Researcher unfinished task' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Refresh tasks' }));
  view.rerender(<AgentTasksView agentId="writer" name="Writer" online />);
  expect(
    screen.queryByDisplayValue('Researcher draft'),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByDisplayValue('Researcher unfinished task'),
  ).not.toBeInTheDocument();
  await act(async () => fresh.resolve(taskList('Writer task', 'writer')));
  expect(screen.getByLabelText('Task 1')).toHaveValue('Writer task');
  expect(screen.getByLabelText('New agent task')).toHaveValue('');
  await act(async () => old.resolve(taskList('Late researcher task', 'late')));
  expect(screen.getByLabelText('Task 1')).toHaveValue('Writer task');
  expect(read).toHaveBeenNthCalledWith(3, 'writer');
  expect(read).toHaveBeenCalledTimes(3);
});

it('keeps the live root StrictMode read usable while ignoring the replayed read', async () => {
  const replay = deferred<AgentTasks>();
  const live = deferred<AgentTasks>();
  const read = vi
    .spyOn(daemon, 'agentTasks')
    .mockReturnValueOnce(replay.promise)
    .mockReturnValueOnce(live.promise);
  const update = vi
    .spyOn(daemon, 'updateAgentTasks')
    .mockResolvedValue(taskList('Strict draft', 'strict-saved'));
  render(<AgentTasksView agentId="researcher" name="Researcher" online />, {
    reactStrictMode: true,
  });
  expect(read).toHaveBeenCalledTimes(2);
  await act(async () => live.resolve(taskList('Live task', 'live')));
  fireEvent.change(screen.getByLabelText('Task 1'), {
    target: { value: 'Strict draft' },
  });
  await act(async () =>
    replay.resolve(taskList('Discarded replay task', 'replay')),
  );
  expect(screen.getByLabelText('Task 1')).toHaveValue('Strict draft');
  fireEvent.click(screen.getByRole('button', { name: 'Save tasks' }));
  await screen.findByRole('button', { name: 'Tasks saved' });
  expect(update).toHaveBeenCalledExactlyOnceWith('researcher', {
    tasks: taskList('Strict draft').tasks,
    revision: 'live',
  });
  expect(read).toHaveBeenCalledTimes(2);
});

it('says a check-in the owner stopped was stopped by the owner', async () => {
  vi.spyOn(daemon, 'listSchedules').mockResolvedValue({
    schedules: [
      {
        ...createdSchedule({ type: 'workspace' }),
        lastFiredAtMs: 3,
        lastOutcome: { status: 'stopped', occurredAtMs: 3, errorCode: null },
      },
    ],
  });
  vi.spyOn(daemon, 'listConnectors').mockResolvedValue({ connectors: [] });
  render(<AgentProactiveView agentId="agent-main" name="Nova" />);
  expect(
    await screen.findByText('Last result: Stopped by owner'),
  ).toBeVisible();
});
