import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { WorkflowRun } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import type { AgentDetail } from '../../lib/types';
import { ChatWorkflows } from './ChatWorkflows';
import { WorkspaceWorkflows } from '../WorkspaceWorkflows';

const agent: AgentDetail = {
  id: 'alpha',
  name: 'Alpha',
  provider: 'test',
  model: 'test',
  status: 'Idle',
  toolNames: ['create_workflow', 'list_workflows'],
  messages: [],
  created_at_ms: 0,
  token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
};
function run(
  status: WorkflowRun['status'] = 'awaiting_approval',
  revision = 4,
): WorkflowRun {
  return {
    id: 'flow',
    agentId: 'alpha',
    title: 'Launch plan',
    requestKey: 'saved',
    status,
    revision,
    currentStep: 0,
    steps: [
      {
        id: 'step',
        title: 'Check plan',
        kind: status === 'awaiting_input' ? 'input' : 'approval',
        prompt: 'Confirm the launch scope',
        status,
        jobId: 'job-saved',
        attempt: 1,
        attempts: ['job-saved'],
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

it('approves and answers in chat, then hides the completed workflow', async () => {
  vi.spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([run()])
    .mockResolvedValueOnce([run('awaiting_input', 5)])
    .mockResolvedValue([run('completed', 6)]);
  const resume = vi.spyOn(daemon, 'resumeWorkflow').mockResolvedValue(run());
  const create = vi.spyOn(daemon, 'createWorkflow');
  render(<ChatWorkflows agent={agent} online refreshKey="activity:1" />);
  expect(await screen.findByText('Saved output <script>')).toBeVisible();
  expect(screen.getByText('Job: job-saved')).toBeVisible();
  expect(
    screen.queryByRole('button', { name: 'Create workflow' }),
  ).not.toBeInTheDocument();
  expect(screen.queryByLabelText('Workflow title')).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Approve checkpoint' }));
  const answer = await screen.findByLabelText('Your answer');
  fireEvent.change(answer, { target: { value: 'Owner answer' } });
  fireEvent.click(screen.getByRole('button', { name: 'Submit answer' }));
  await waitFor(() =>
    expect(resume).toHaveBeenLastCalledWith('alpha', 'flow', {
      revision: 5,
      action: 'answer',
      value: 'Owner answer',
    }),
  );
  await waitFor(() =>
    expect(
      screen.queryByRole('article', { name: 'Launch plan' }),
    ).not.toBeInTheDocument(),
  );
  expect(
    screen.queryByRole('region', { name: 'Active workflows' }),
  ).not.toBeInTheDocument();
  expect(screen.queryByText('Completed')).not.toBeInTheDocument();
  expect(document.querySelector('script')).toBeNull();
  expect(create).not.toHaveBeenCalled();
});

it('keeps saved owner controls after either chat creation grant is revoked', async () => {
  vi.spyOn(daemon, 'listWorkflows').mockResolvedValue([run()]);
  const resume = vi.spyOn(daemon, 'resumeWorkflow').mockResolvedValue(run());
  render(
    <ChatWorkflows
      agent={{ ...agent, toolNames: ['list_workflows'] }}
      online
    />,
  );
  expect(screen.queryByText(/Allow workflows in chat/)).not.toBeInTheDocument();
  fireEvent.click(
    await screen.findByRole('button', { name: 'Approve checkpoint' }),
  );
  await waitFor(() =>
    expect(resume).toHaveBeenCalledWith('alpha', 'flow', {
      revision: 4,
      action: 'approve',
    }),
  );
});

it('hides template management with either template grant state while keeping checkpoint controls', async () => {
  vi.spyOn(daemon, 'listWorkflows').mockResolvedValue([run()]);
  const view = render(
    <ChatWorkflows
      agent={{
        ...agent,
        toolNames: [
          ...agent.toolNames,
          'save_workflow_template',
          'list_workflow_templates',
        ],
      }}
      online
    />,
  );
  expect(
    screen.queryByText(/Allow workflow templates in chat/),
  ).not.toBeInTheDocument();
  expect(
    await screen.findByRole('button', { name: 'Approve checkpoint' }),
  ).toBeEnabled();
  view.rerender(
    <ChatWorkflows
      agent={{
        ...agent,
        toolNames: [
          ...agent.toolNames,
          'save_workflow_template',
          'list_workflow_templates',
          'run_workflow_template',
        ],
      }}
      online
    />,
  );
  expect(
    screen.queryByText(/Enable “Allow workflow templates in chat”/),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByText(/Ask in chat to save a reusable template/),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByText('Reusable templates', { selector: 'summary' }),
  ).not.toBeInTheDocument();
});

it('refreshes an initially empty list when chat activity changes', async () => {
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([])
    .mockResolvedValue([run()]);
  const view = render(<ChatWorkflows agent={agent} online refreshKey={1} />);
  await act(async () => {
    await Promise.resolve();
  });
  expect(view.container).toBeEmptyDOMElement();
  view.rerender(<ChatWorkflows agent={agent} online refreshKey={2} />);
  expect(
    await screen.findByRole('button', { name: 'Approve checkpoint' }),
  ).toBeEnabled();
  expect(list).toHaveBeenCalledTimes(2);
});

it('replays the exact saved decision after an uncertain response and remount despite a newer revision', async () => {
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValue([run('awaiting_input')]);
  const resume = vi
    .spyOn(daemon, 'resumeWorkflow')
    .mockRejectedValueOnce(new Error('Connection lost'))
    .mockResolvedValue(run('completed'));
  const view = render(<ChatWorkflows agent={agent} online />);
  fireEvent.change(await screen.findByLabelText('Your answer'), {
    target: { value: 'Exact answer' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Submit answer' }));
  await screen.findByRole('alert');
  view.unmount();
  list.mockResolvedValue([run('awaiting_input', 8)]);
  render(<ChatWorkflows agent={agent} online />);
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: 'Retry saved decision' }),
    ).toBeEnabled(),
  );
  fireEvent.click(screen.getByRole('button', { name: 'Retry saved decision' }));
  await waitFor(() => expect(resume).toHaveBeenCalledTimes(2));
  expect(resume.mock.calls[1]).toEqual(resume.mock.calls[0]);
});

it('ignores stale responses through agent changes and disconnect/reconnect', async () => {
  let stale!: (runs: WorkflowRun[]) => void;
  let fresh!: (runs: WorkflowRun[]) => void;
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          stale = resolve;
        }),
    )
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          fresh = resolve;
        }),
    )
    .mockResolvedValue([]);
  const view = render(<ChatWorkflows agent={agent} online />);
  view.rerender(<ChatWorkflows agent={agent} online={false} />);
  expect(view.container).toBeEmptyDOMElement();
  view.rerender(
    <ChatWorkflows agent={{ ...agent, id: 'beta', name: 'Beta' }} online />,
  );
  await act(async () => stale([run()]));
  expect(screen.queryByText('Launch plan')).not.toBeInTheDocument();
  await act(async () => fresh([]));
  expect(view.container).toBeEmptyDOMElement();
  expect(list).toHaveBeenLastCalledWith('beta', expect.anything());
});

it('renders nothing while initially loading and when only completed or cancelled history exists', async () => {
  let finish!: (runs: WorkflowRun[]) => void;
  vi.spyOn(daemon, 'listWorkflows').mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const view = render(<ChatWorkflows agent={agent} online />);
  expect(view.container).toBeEmptyDOMElement();
  await act(async () =>
    finish([
      { ...run('completed'), id: 'done', title: 'Old completed run' },
      { ...run('cancelled'), id: 'cancelled', title: 'Old cancelled run' },
    ]),
  );
  expect(view.container).toBeEmptyDOMElement();
});

it('shows only active workflows and checkpoints from a mixed saved list', async () => {
  const active: WorkflowRun['status'][] = [
    'running',
    'awaiting_approval',
    'awaiting_input',
    'needs_review',
  ];
  vi.spyOn(daemon, 'listWorkflows').mockResolvedValue([
    ...active.map((status) => ({ ...run(status), id: status, title: status })),
    { ...run('completed'), id: 'done', title: 'Old completed run' },
    { ...run('cancelled'), id: 'cancelled', title: 'Old cancelled run' },
  ]);
  render(<ChatWorkflows agent={agent} online />);
  expect(
    await screen.findByRole('region', { name: 'Active workflows' }),
  ).toBeVisible();
  for (const title of active)
    expect(screen.getByRole('article', { name: title })).toBeVisible();
  expect(screen.queryByText('Old completed run')).not.toBeInTheDocument();
  expect(screen.queryByText('Old cancelled run')).not.toBeInTheDocument();
  expect(screen.queryByText('Reusable templates')).not.toBeInTheDocument();
  expect(
    screen.getByRole('button', { name: 'Approve checkpoint' }),
  ).toBeEnabled();
  expect(screen.getByLabelText('Your answer')).toBeEnabled();
  expect(
    screen.getByLabelText(/I have reviewed the uncertain effects/),
  ).toBeEnabled();
});

it('hides a cancelled workflow immediately after the owner cancels it', async () => {
  vi.spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([run()])
    .mockResolvedValue([run('cancelled', 5)]);
  const cancel = vi
    .spyOn(daemon, 'cancelWorkflow')
    .mockResolvedValue(run('cancelled', 5));
  const view = render(<ChatWorkflows agent={agent} online />);
  fireEvent.click(
    await screen.findByRole('button', { name: 'Cancel workflow' }),
  );
  await waitFor(() => expect(view.container).toBeEmptyDOMElement());
  expect(cancel).toHaveBeenCalledWith('alpha', 'flow', { revision: 4 });
});

it('retains an unresolved owner decision even when the saved run has already completed', async () => {
  sessionStorage.setItem(
    'anima:workflow-command:alpha',
    JSON.stringify({
      type: 'resume',
      id: 'flow',
      input: { revision: 4, action: 'approve' },
    }),
  );
  vi.spyOn(daemon, 'listWorkflows').mockResolvedValue([run('completed', 5)]);
  const resume = vi
    .spyOn(daemon, 'resumeWorkflow')
    .mockResolvedValue(run('completed', 5));
  const view = render(<ChatWorkflows agent={agent} online />);
  const retry = await screen.findByRole('button', {
    name: 'Retry saved decision',
  });
  await waitFor(() => expect(retry).toBeEnabled());
  expect(screen.queryByText('Launch plan')).not.toBeInTheDocument();
  fireEvent.click(retry);
  await waitFor(() => expect(view.container).toBeEmptyDOMElement());
  expect(resume).toHaveBeenCalledWith('alpha', 'flow', {
    revision: 4,
    action: 'approve',
  });
});

it('shows a load failure with an explicit retry without presenting history or templates', async () => {
  vi.spyOn(daemon, 'listWorkflows').mockRejectedValue(
    new Error('Store unavailable'),
  );
  render(<ChatWorkflows agent={agent} online />);
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Store unavailable',
  );
  expect(
    screen.getByRole('button', { name: 'Refresh workflows' }),
  ).toBeEnabled();
  expect(
    screen.queryByText('No workflows yet for this agent.'),
  ).not.toBeInTheDocument();
  expect(screen.queryByText('Reusable templates')).not.toBeInTheDocument();
});

it('hides a running workflow when automatic polling observes completion', async () => {
  vi.useFakeTimers();
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([run('running')])
    .mockResolvedValue([run('completed', 5)]);
  const view = render(<ChatWorkflows agent={agent} online />);
  await act(async () => {
    await Promise.resolve();
  });
  expect(screen.getByRole('article', { name: 'Launch plan' })).toBeVisible();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  expect(view.container).toBeEmptyDOMElement();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(10000);
  });
  expect(list).toHaveBeenCalledTimes(2);
});

it('requires acknowledgement before recording a verified result from chat', async () => {
  vi.spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([run('needs_review')])
    .mockResolvedValue([run('completed', 5)]);
  const resume = vi
    .spyOn(daemon, 'resumeWorkflow')
    .mockResolvedValue(run('completed', 5));
  const view = render(<ChatWorkflows agent={agent} online />);
  fireEvent.change(await screen.findByLabelText('Verified result'), {
    target: { value: 'Owner verified the result' },
  });
  expect(
    screen.getByRole('button', { name: 'Record verified result' }),
  ).toBeDisabled();
  fireEvent.click(
    screen.getByLabelText(/I have reviewed the uncertain effects/),
  );
  fireEvent.click(
    screen.getByRole('button', { name: 'Record verified result' }),
  );
  await waitFor(() => expect(view.container).toBeEmptyDOMElement());
  expect(resume).toHaveBeenCalledWith('alpha', 'flow', {
    revision: 4,
    action: 'resolve',
    value: 'Owner verified the result',
    acknowledgeUncertain: true,
  });
});

it('refreshes the still-mounted chat when the owner creates a workflow under Work', async () => {
  let created = false;
  vi.spyOn(daemon, 'listWorkflows').mockImplementation(async () =>
    created ? [run()] : [],
  );
  vi.spyOn(daemon, 'createWorkflow').mockImplementation(async () => {
    created = true;
    return run();
  });
  render(
    <>
      <div data-testid="chat-surface">
        <ChatWorkflows agent={agent} online />
      </div>
      <div data-testid="work-surface">
        <WorkspaceWorkflows agents={[agent]} online />
      </div>
    </>,
  );
  const chat = screen.getByTestId('chat-surface');
  const work = within(screen.getByTestId('work-surface'));
  await work.findByText('No workflows yet for this agent.');
  expect(chat).toBeEmptyDOMElement();
  fireEvent.change(work.getByLabelText('Workflow title'), {
    target: { value: 'New workflow' },
  });
  fireEvent.change(work.getByLabelText('Step 1 title'), {
    target: { value: 'Check' },
  });
  fireEvent.change(work.getByLabelText('Step 1 prompt'), {
    target: { value: 'Check once' },
  });
  fireEvent.click(work.getByRole('button', { name: 'Create workflow' }));
  expect(
    await within(chat).findByRole('article', { name: 'Launch plan' }),
  ).toBeVisible();
  expect(
    within(chat).queryByText('Reusable templates'),
  ).not.toBeInTheDocument();
});

it('refreshes only the originating agent chat after a creation resolves following a Work agent switch', async () => {
  const beta = { ...agent, id: 'beta', name: 'Beta' };
  let created = false;
  let finish!: () => void;
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockImplementation(async (id) =>
      created && id === 'alpha' ? [run()] : [],
    );
  const create = vi.spyOn(daemon, 'createWorkflow').mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = () => {
          created = true;
          resolve(run());
        };
      }),
  );
  render(
    <>
      <div data-testid="alpha-chat">
        <ChatWorkflows agent={agent} online />
      </div>
      <div data-testid="beta-chat">
        <ChatWorkflows agent={beta} online />
      </div>
      <div data-testid="work-surface">
        <WorkspaceWorkflows agents={[agent, beta]} online />
      </div>
    </>,
  );
  const work = within(screen.getByTestId('work-surface'));
  await work.findByText('No workflows yet for this agent.');
  fireEvent.change(work.getByLabelText('Workflow title'), {
    target: { value: 'Alpha work' },
  });
  fireEvent.change(work.getByLabelText('Step 1 title'), {
    target: { value: 'Check' },
  });
  fireEvent.change(work.getByLabelText('Step 1 prompt'), {
    target: { value: 'Check once' },
  });
  fireEvent.click(work.getByRole('button', { name: 'Create workflow' }));
  await waitFor(() => expect(create).toHaveBeenCalledTimes(1));
  fireEvent.change(work.getByRole('combobox', { name: 'Agent' }), {
    target: { value: 'beta' },
  });
  await work.findByText('No workflows yet for this agent.');
  const betaReads = list.mock.calls.filter(([id]) => id === 'beta').length;
  await act(async () => finish());
  expect(
    await within(screen.getByTestId('alpha-chat')).findByRole('article', {
      name: 'Launch plan',
    }),
  ).toBeVisible();
  expect(screen.getByTestId('beta-chat')).toBeEmptyDOMElement();
  expect(list.mock.calls.filter(([id]) => id === 'beta')).toHaveLength(
    betaReads,
  );
});
