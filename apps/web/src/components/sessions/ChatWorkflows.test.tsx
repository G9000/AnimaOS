import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { WorkflowRun } from '@animaOS-SWARM/sdk';
import { daemon } from '../../lib/daemon-api';
import type { AgentDetail } from '../../lib/types';
import { ChatWorkflows } from './ChatWorkflows';

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

it('approves, answers and shows a collapsed completed card with persisted safe result and job identity', async () => {
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
  const summary = (
    await screen.findByText('Completed', { selector: 'summary span' })
  ).closest('summary');
  expect(summary).not.toBeNull();
  expect(summary?.parentElement).not.toHaveAttribute('open');
  fireEvent.click(summary!);
  expect(screen.getByText('Saved output <script>')).toBeVisible();
  expect(screen.getByText('Saved job attempts: job-saved')).toBeVisible();
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
  expect(screen.getByText(/Allow workflows in chat/)).toBeVisible();
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

it('explains the independent template permission and keeps owner controls available', async () => {
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
    screen.getByText(/Enable “Allow workflow templates in chat”/),
  ).toBeVisible();
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
    screen.getByText(/Ask in chat to save a reusable template/),
  ).toBeVisible();
  expect(
    screen.getByText('Reusable templates', { selector: 'summary' }),
  ).toBeVisible();
});

it('refreshes an initially empty list when chat activity changes', async () => {
  const list = vi
    .spyOn(daemon, 'listWorkflows')
    .mockResolvedValueOnce([])
    .mockResolvedValue([run()]);
  const view = render(<ChatWorkflows agent={agent} online refreshKey={1} />);
  await screen.findByText('No workflows yet for this agent.');
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
  expect(
    screen.getByRole('button', { name: 'Refresh workflows' }),
  ).toBeDisabled();
  view.rerender(
    <ChatWorkflows agent={{ ...agent, id: 'beta', name: 'Beta' }} online />,
  );
  await act(async () => stale([run()]));
  expect(screen.queryByText('Launch plan')).not.toBeInTheDocument();
  await act(async () => fresh([]));
  expect(
    screen.getByRole('region', { name: 'Workflows for Beta' }),
  ).toBeVisible();
  expect(screen.getByText('No workflows yet for this agent.')).toBeVisible();
  expect(list).toHaveBeenLastCalledWith('beta', expect.anything());
});
