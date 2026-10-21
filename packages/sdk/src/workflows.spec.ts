import { expect, it } from 'vitest';
import {
  createDaemonClient,
  DaemonHttpError,
  type WorkflowRun,
} from './index.js';

const workflow: WorkflowRun = {
  id: 'flow/one',
  agentId: 'agent/a',
  title: 'Review proposal',
  requestKey: 'stable',
  status: 'awaiting_approval',
  revision: 2,
  currentStep: 0,
  steps: [
    {
      id: 'review',
      title: 'Review',
      kind: 'approval',
      prompt: 'Publish?',
      status: 'awaiting_approval',
      jobId: null,
      attempt: 0,
      attempts: [],
      result: null,
    },
  ],
  decisions: [],
  createdAtMs: 1,
  updatedAtMs: 2,
  error: null,
};

it('encodes workflow paths, unwraps only the collection, and preserves exact decisions', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return Response.json(
        !init?.method && String(url).endsWith('/workflows')
          ? { workflows: [workflow] }
          : workflow,
      );
    },
  });
  const signal = new AbortController().signal;
  expect(await client.workflows.list('agent/a', { signal })).toEqual([
    workflow,
  ]);
  expect(await client.workflows.get('agent/a', 'flow/one', { signal })).toEqual(
    workflow,
  );
  const input = {
    title: 'Review proposal',
    requestKey: 'stable',
    steps: [
      {
        id: 'review',
        title: 'Review',
        kind: 'approval' as const,
        prompt: 'Publish?',
      },
    ],
  };
  expect(await client.workflows.create('agent/a', input)).toEqual(workflow);
  const decision = {
    revision: 2,
    action: 'resolve' as const,
    value: 'Owner verified',
    acknowledgeUncertain: true,
  };
  expect(
    await client.workflows.resume('agent/a', 'flow/one', decision),
  ).toEqual(workflow);
  await client.workflows.resume('agent/a', 'flow/one', decision);
  expect(
    await client.workflows.cancel('agent/a', 'flow/one', { revision: 3 }),
  ).toEqual(workflow);
  expect(requests.map(({ url }) => url)).toEqual([
    '/api/agents/agent%2Fa/workflows',
    '/api/agents/agent%2Fa/workflows/flow%2Fone',
    '/api/agents/agent%2Fa/workflows',
    '/api/agents/agent%2Fa/workflows/flow%2Fone/resume',
    '/api/agents/agent%2Fa/workflows/flow%2Fone/resume',
    '/api/agents/agent%2Fa/workflows/flow%2Fone/cancel',
  ]);
  expect(requests[0].init?.signal).toBe(signal);
  expect(requests[1].init?.signal).toBe(signal);
  expect(
    requests.slice(2).map(({ init }) => JSON.parse(init?.body as string)),
  ).toEqual([input, decision, decision, { revision: 3 }]);
  expect(requests.slice(2).every(({ init }) => init?.method === 'POST')).toBe(
    true,
  );
});

it('propagates stale decision and unavailable persistence errors without retrying', async () => {
  for (const status of [409, 503]) {
    let calls = 0;
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async () => {
        calls += 1;
        return Response.json({ error: 'Saved state unavailable' }, { status });
      },
    });
    await expect(
      client.workflows.resume('a', 'w', { revision: 2, action: 'approve' }),
    ).rejects.toMatchObject({ status, message: 'Saved state unavailable' });
    expect(calls).toBe(1);
  }
  expect(DaemonHttpError).toBeDefined();
});
