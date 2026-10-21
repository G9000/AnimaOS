import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { DaemonHttpError, type WorkflowRun } from '@animaOS-SWARM/sdk';
import { daemon } from '../lib/daemon-api';
import type { AgentDetail } from '../lib/types';
import { WorkspaceWorkflows } from './WorkspaceWorkflows';

const agents: AgentDetail[] = ['Alpha', 'Beta'].map((name) => ({
  id: name.toLowerCase(),
  name,
  provider: 'test',
  model: 'test',
  status: 'Idle',
  toolNames: [],
  messages: [],
  created_at_ms: 0,
  token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
}));
function run(status: WorkflowRun['status'] = 'awaiting_approval'): WorkflowRun {
  return {
    id: 'flow',
    agentId: 'alpha',
    title: 'Launch plan',
    requestKey: 'saved',
    status,
    revision: 4,
    currentStep: 0,
    steps: [
      {
        id: 'step',
        title: 'Check plan',
        kind:
          status === 'awaiting_input'
            ? 'input'
            : status === 'needs_review' || status === 'running'
              ? 'task'
              : 'approval',
        prompt: 'Confirm the launch scope',
        status,
        jobId: null,
        attempt: 1,
        attempts: ['job-old'],
        result: 'Saved output <script>',
      },
    ],
    decisions: [],
    error: null,
    createdAtMs: 1,
    updatedAtMs: 2,
  };
}
beforeEach(() => sessionStorage.clear());
afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

it('approves the saved checkpoint then refreshes persisted state', async () => {
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([run()])
    .mockResolvedValue([run('completed')]);
  const resume = vi
    .spyOn(daemon, 'resumeWorkflow')
    .mockResolvedValue(run('completed'));
  render(<WorkspaceWorkflows agents={agents} online />);
  expect(await screen.findByText('Saved output <script>')).toBeVisible();
  expect(screen.getByText(/job-old/)).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Approve checkpoint' }));
  await waitFor(() =>
    expect(resume).toHaveBeenCalledWith('alpha', 'flow', {
      revision: 4,
      action: 'approve',
    }),
  );
  await waitFor(() => expect(list).toHaveBeenCalledTimes(2));
  expect(await screen.findByText('Completed')).toBeVisible();
});

it('requires a bounded answer and explicit review acknowledgement', async () => {
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValue([run('awaiting_input')]);
  const resume = vi
    .spyOn(daemon, 'resumeWorkflow')
    .mockResolvedValue(run('completed'));
  const view = render(<WorkspaceWorkflows agents={agents} online />);
  const answer = await screen.findByRole('button', { name: 'Submit answer' });
  expect(answer).toBeDisabled();
  fireEvent.change(screen.getByLabelText('Your answer'), {
    target: { value: 'Owner choice' },
  });
  fireEvent.click(answer);
  await waitFor(() =>
    expect(resume).toHaveBeenCalledWith('alpha', 'flow', {
      revision: 4,
      action: 'answer',
      value: 'Owner choice',
    }),
  );
  view.unmount();
  list.mockResolvedValue([run('needs_review')]);
  render(<WorkspaceWorkflows agents={agents} online />);
  expect(
    await screen.findByRole('button', { name: 'Retry task' }),
  ).toBeDisabled();
  expect(
    screen.getByRole('button', { name: 'Record verified result' }),
  ).toBeDisabled();
  fireEvent.click(
    screen.getByLabelText(/I have reviewed the uncertain effects/),
  );
  fireEvent.change(screen.getByLabelText('Verified result'), {
    target: { value: 'Checked receipt; completed once' },
  });
  fireEvent.click(
    screen.getByRole('button', { name: 'Record verified result' }),
  );
  await waitFor(() =>
    expect(resume).toHaveBeenLastCalledWith('alpha', 'flow', {
      revision: 4,
      action: 'resolve',
      value: 'Checked receipt; completed once',
      acknowledgeUncertain: true,
    }),
  );
});

it('retains the original decision payload across refresh, lost response, and remount', async () => {
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValue([run('awaiting_input')]);
  const resume = vi
    .spyOn(daemon, 'resumeWorkflow')
    .mockRejectedValueOnce(new Error('Connection lost'))
    .mockResolvedValue(run('completed'));
  const view = render(<WorkspaceWorkflows agents={agents} online />);
  fireEvent.change(await screen.findByLabelText('Your answer'), {
    target: { value: 'Exact original answer' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Submit answer' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Connection lost');
  view.unmount();
  list.mockResolvedValue([{ ...run('awaiting_input'), revision: 8 }]);
  render(<WorkspaceWorkflows agents={agents} online />);
  fireEvent.click(
    await screen.findByRole('button', { name: 'Retry saved decision' }),
  );
  await waitFor(() => expect(resume).toHaveBeenCalledTimes(2));
  expect(resume.mock.calls[1]).toEqual(resume.mock.calls[0]);
});

it('disables offline mutations and ignores pre-disconnect responses after reconnect', async () => {
  let finish!: (value: WorkflowRun[]) => void;
  let fresh!: (value: WorkflowRun[]) => void;
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    )
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          fresh = resolve;
        }),
    );
  const create = vi.spyOn(daemon, 'createWorkflow');
  const view = render(<WorkspaceWorkflows agents={agents} online />);
  view.rerender(<WorkspaceWorkflows agents={agents} online={false} />);
  expect(screen.getByText(/Offline/)).toBeVisible();
  expect(
    screen.getByRole('button', { name: 'Create workflow' }),
  ).toBeDisabled();
  expect(
    screen.getByRole('button', { name: 'Refresh workflows' }),
  ).toBeDisabled();
  view.rerender(<WorkspaceWorkflows agents={agents} online />);
  await act(async () => finish([run()]));
  expect(screen.queryByText('Launch plan')).not.toBeInTheDocument();
  expect(screen.getByText('Loading workflows…')).toBeVisible();
  expect(list.mock.calls[0][1]?.signal?.aborted).toBe(true);
  await act(async () => fresh([]));
  expect(screen.getByText('No workflows yet for this agent.')).toBeVisible();
  expect(create).not.toHaveBeenCalled();
});

it('guards stale membership and mutation completions when changing agents', async () => {
  let finish!: (value: WorkflowRun) => void;
  const list = vi.spyOn(daemon, 'listWorkflows').mockResolvedValue([run()]);
  vi.spyOn(daemon, 'resumeWorkflow').mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const view = render(<WorkspaceWorkflows agents={agents} online />);
  fireEvent.click(
    await screen.findByRole('button', { name: 'Approve checkpoint' }),
  );
  list.mockResolvedValue([]);
  view.rerender(<WorkspaceWorkflows agents={[agents[1]]} online />);
  await screen.findByText('No workflows yet for this agent.');
  await act(async () => finish(run('completed')));
  expect(screen.queryByText('Launch plan')).not.toBeInTheDocument();
  expect(list).toHaveBeenCalledTimes(2);
  expect(list).toHaveBeenLastCalledWith('beta', expect.anything());
});

it('requires fresh review after a definitive revision conflict', async () => {
  vi.spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([run('needs_review')])
    .mockResolvedValue([{ ...run('needs_review'), revision: 6 }]);
  const resume = vi
    .spyOn(daemon, 'resumeWorkflow')
    .mockRejectedValue(new DaemonHttpError(409, { error: 'Workflow changed' }));
  render(<WorkspaceWorkflows agents={agents} online />);
  fireEvent.click(
    await screen.findByLabelText(/I have reviewed the uncertain effects/),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Retry task' }));
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Workflow changed',
  );
  await waitFor(() =>
    expect(
      screen.getByLabelText(/I have reviewed the uncertain effects/),
    ).not.toBeChecked(),
  );
  expect(resume).toHaveBeenCalledTimes(1);
  expect(
    screen.queryByRole('button', { name: 'Retry saved decision' }),
  ).not.toBeInTheDocument();
});

it('creates ordered tasks, checkpoints and questions without granting workflow tools', async () => {
  vi.spyOn(daemon, 'listWorkflows').mockResolvedValue([]);
  const create = vi.spyOn(daemon, 'createWorkflow').mockResolvedValue(run());
  render(<WorkspaceWorkflows agents={agents} online />);
  await screen.findByText('No workflows yet for this agent.');
  fireEvent.change(screen.getByLabelText('Workflow title'), {
    target: { value: 'Launch' },
  });
  fireEvent.change(screen.getByLabelText('Step 1 title'), {
    target: { value: 'Draft' },
  });
  fireEvent.change(screen.getByLabelText('Step 1 prompt'), {
    target: { value: 'Write draft' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Add step' }));
  fireEvent.change(screen.getByLabelText('Step 2 kind'), {
    target: { value: 'approval' },
  });
  fireEvent.change(screen.getByLabelText('Step 2 title'), {
    target: { value: 'Review' },
  });
  fireEvent.change(screen.getByLabelText('Step 2 prompt'), {
    target: { value: 'Approve draft?' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Add step' }));
  fireEvent.change(screen.getByLabelText('Step 3 kind'), {
    target: { value: 'input' },
  });
  fireEvent.change(screen.getByLabelText('Step 3 title'), {
    target: { value: 'Audience' },
  });
  fireEvent.change(screen.getByLabelText('Step 3 prompt'), {
    target: { value: 'Who is this for?' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Create workflow' }));
  await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
  expect(create.mock.calls[0][0]).toBe('alpha');
  expect(create.mock.calls[0][1]).toMatchObject({
    title: 'Launch',
    requestKey: expect.any(String),
    steps: [
      { title: 'Draft', kind: 'task', prompt: 'Write draft' },
      { title: 'Review', kind: 'approval', prompt: 'Approve draft?' },
      { title: 'Audience', kind: 'input', prompt: 'Who is this for?' },
    ],
  });
});

it('shows waiting capacity honestly and bounds automatic polling', async () => {
  vi.useFakeTimers();
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValue([run('running')]);
  render(<WorkspaceWorkflows agents={agents} online />);
  await act(async () => {
    await Promise.resolve();
  });
  expect(screen.getByText('Waiting for an execution slot')).toBeVisible();
  for (let index = 0; index < 65; index++)
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
  expect(list).toHaveBeenCalledTimes(60);
  expect(screen.getByText(/Automatic refresh paused/)).toBeVisible();
});

it('replays an uncertain creation with its immutable definition and key after remount', async () => {
  vi.spyOn(daemon, 'listWorkflows').mockResolvedValue([]);
  const create = vi
    .spyOn(daemon, 'createWorkflow')
    .mockRejectedValueOnce(new Error('Response lost'))
    .mockResolvedValue(run());
  const view = render(<WorkspaceWorkflows agents={agents} online />);
  await screen.findByText('No workflows yet for this agent.');
  fireEvent.change(screen.getByLabelText('Workflow title'), {
    target: { value: 'Exact creation' },
  });
  fireEvent.change(screen.getByLabelText('Step 1 title'), {
    target: { value: 'Draft' },
  });
  fireEvent.change(screen.getByLabelText('Step 1 prompt'), {
    target: { value: 'Write once' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Create workflow' }));
  expect(await screen.findByRole('alert')).toHaveTextContent('Response lost');
  expect(screen.getByLabelText('Step 1 prompt')).toBeDisabled();
  view.unmount();
  render(<WorkspaceWorkflows agents={agents} online />);
  await screen.findByText('No workflows yet for this agent.');
  fireEvent.click(screen.getByRole('button', { name: 'Retry saved workflow' }));
  await waitFor(() => expect(create).toHaveBeenCalledTimes(2));
  expect(create.mock.calls[1]).toEqual(create.mock.calls[0]);
});

it('enforces UTF-8 prompt and owner input bounds, total definition size, and ordered step edits', async () => {
  vi.spyOn(daemon, 'listWorkflows').mockResolvedValue([run('awaiting_input')]);
  const create = vi.spyOn(daemon, 'createWorkflow').mockResolvedValue(run());
  render(<WorkspaceWorkflows agents={agents} online />);
  await screen.findByLabelText('Your answer');
  fireEvent.change(screen.getByLabelText('Your answer'), {
    target: { value: '😀'.repeat(2049) },
  });
  expect(screen.getByRole('button', { name: 'Submit answer' })).toBeDisabled();
  fireEvent.change(screen.getByLabelText('Workflow title'), {
    target: { value: 'Launch' },
  });
  fireEvent.change(screen.getByLabelText('Step 1 title'), {
    target: { value: 'First' },
  });
  fireEvent.change(screen.getByLabelText('Step 1 prompt'), {
    target: { value: '😀'.repeat(2049) },
  });
  expect(
    screen.getByRole('button', { name: 'Create workflow' }),
  ).toBeDisabled();
  fireEvent.change(screen.getByLabelText('Step 1 prompt'), {
    target: { value: 'a'.repeat(8192) },
  });
  for (let number = 2; number <= 4; number++) {
    fireEvent.click(screen.getByRole('button', { name: 'Add step' }));
    fireEvent.change(screen.getByLabelText(`Step ${number} title`), {
      target: { value: `Step ${number}` },
    });
    fireEvent.change(screen.getByLabelText(`Step ${number} prompt`), {
      target: { value: 'a'.repeat(8192) },
    });
  }
  expect(
    screen.getByRole('button', { name: 'Create workflow' }),
  ).toBeDisabled();
  expect(
    screen.getByText(
      'The complete definition must fit within 32768 UTF-8 bytes.',
    ),
  ).toBeVisible();
  fireEvent.click(screen.getByRole('button', { name: 'Remove step 4' }));
  fireEvent.click(screen.getByRole('button', { name: 'Move step 2 up' }));
  expect(screen.getByLabelText('Step 1 title')).toHaveValue('Step 2');
  fireEvent.click(screen.getByRole('button', { name: 'Create workflow' }));
  await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
  expect(create.mock.calls[0][1].steps.map((step) => step.title)).toEqual([
    'Step 2',
    'First',
    'Step 3',
  ]);
});

it('cancels with the inspected revision and does not describe a load failure as empty', async () => {
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([run()])
    .mockRejectedValue(new Error('Store unavailable'));
  const cancel = vi
    .spyOn(daemon, 'cancelWorkflow')
    .mockResolvedValue(run('cancelled'));
  render(<WorkspaceWorkflows agents={agents} online />);
  fireEvent.click(
    await screen.findByRole('button', { name: 'Cancel workflow' }),
  );
  await waitFor(() =>
    expect(cancel).toHaveBeenCalledWith('alpha', 'flow', { revision: 4 }),
  );
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Store unavailable',
  );
  expect(
    screen.queryByText('No workflows yet for this agent.'),
  ).not.toBeInTheDocument();
  expect(list).toHaveBeenCalledTimes(2);
});
