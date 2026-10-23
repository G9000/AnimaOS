import { startTransition, Suspense, useLayoutEffect, useState } from 'react';
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import type { AgentTasks, WorkflowRun } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';
import type { AgentDetail } from '../lib/types';
import { AgentProactiveView, AgentTasksView } from './AgentWork';
import { WorkspaceHub } from './WorkspaceHub';
import { WorkspaceWorkflows } from './WorkspaceWorkflows';

const agents: AgentDetail[] = [
  {
    id: 'alpha',
    name: 'Alpha',
    provider: 'test',
    model: 'test',
    status: 'Idle',
    toolNames: [],
    messages: [],
    created_at_ms: 0,
    token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
  },
];
const tasks: AgentTasks = {
  revision: 'current',
  tasks: [
    { content: 'Current task', activeForm: 'Working', status: 'pending' },
  ],
};
const suspended = new Promise<void>(() => {});
function RenderGate({ blocked }: { blocked: boolean }) {
  if (blocked) throw suspended;
  return null;
}
function WorkHarness({ surface }: { surface: 'tasks' | 'hub' | 'workflows' }) {
  const [next, setNext] = useState(false);
  return (
    <>
      <button onClick={() => startTransition(() => setNext(true))}>
        Disconnect
      </button>
      <Suspense fallback="Loading next view">
        {surface === 'tasks' ? (
          <AgentTasksView agentId="alpha" name="Alpha" online={!next} />
        ) : surface === 'hub' ? (
          <WorkspaceHub agents={agents} online={!next} initialSection="Tasks" />
        ) : (
          <WorkspaceWorkflows agents={agents} online={!next} />
        )}
        <RenderGate blocked={next} />
      </Suspense>
    </>
  );
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}
async function interruptRender() {
  await act(async () => {
    fireEvent.click(screen.getByRole('button', { name: 'Disconnect' }));
  });
  expect(screen.queryByText('Loading next view')).not.toBeInTheDocument();
}

beforeEach(() => sessionStorage.clear());
afterEach(() => vi.restoreAllMocks());

it('keeps the committed task editor read live through an interrupted disconnect', async () => {
  const pending = deferred<AgentTasks>();
  vi.spyOn(daemon, 'agentTasks').mockReturnValue(pending.promise);
  render(<WorkHarness surface="tasks" />);
  await interruptRender();
  await act(async () => pending.resolve(tasks));
  expect(screen.getByLabelText('Task 1')).toHaveValue('Current task');
});

it('keeps the committed task aggregate read live through an interrupted disconnect', async () => {
  const pending = deferred<AgentTasks>();
  vi.spyOn(daemon, 'agentTasks').mockReturnValue(pending.promise);
  render(<WorkHarness surface="hub" />);
  await interruptRender();
  await act(async () => pending.resolve(tasks));
  expect(screen.getByText('Current task')).toBeVisible();
});

it('keeps the committed workflow read live through an interrupted disconnect', async () => {
  const pending = deferred<WorkflowRun[]>();
  vi.spyOn(daemon, 'listWorkflows').mockReturnValue(pending.promise);
  render(<WorkHarness surface="workflows" />);
  await interruptRender();
  await act(async () => pending.resolve([]));
  expect(screen.getByText('No workflows yet for this agent.')).toBeVisible();
});

it('clears the previous agent schedule error before committing the next agent view', async () => {
  const pending = deferred<{ schedules: [] }>();
  vi.spyOn(daemon, 'listSchedules')
    .mockRejectedValueOnce(new Error('Previous agent error'))
    .mockReturnValue(pending.promise);
  vi.spyOn(daemon, 'listConnectors').mockResolvedValue({ connectors: [] });
  const observed: Array<string | null> = [];
  function Harness() {
    const [next, setNext] = useState(false);
    useLayoutEffect(() => {
      if (next) observed.push(screen.queryByRole('alert')?.textContent ?? null);
    }, [next]);
    return (
      <>
        <button onClick={() => setNext(true)}>Switch agent</button>
        <AgentProactiveView agentId={next ? 'beta' : 'alpha'} name="Agent" />
      </>
    );
  }
  render(<Harness />);
  expect(await screen.findByRole('alert')).toHaveTextContent(
    'Previous agent error',
  );
  fireEvent.click(screen.getByRole('button', { name: 'Switch agent' }));
  expect(observed).toEqual([null]);
  await act(async () => pending.resolve({ schedules: [] }));
});
