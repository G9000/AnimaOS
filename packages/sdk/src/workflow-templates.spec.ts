import { expect, it } from 'vitest';
import { createDaemonClient } from './client.js';
import { WorkflowTemplatesClient } from './workflows.js';

it('workflow templates encode ownership paths and retain exact save/run/delete payloads', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const template = { slug: 'daily', revision: 1 };
  const client = new WorkflowTemplatesClient(
    createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        requests.push({ url: String(url), init });
        return Response.json(
          !init?.method && String(url).endsWith('/workflow-templates')
            ? { templates: [template] }
            : template,
        );
      },
    }),
  );
  const signal = new AbortController().signal;
  expect(await client.list('agent/a', { signal })).toEqual([template]);
  await client.get('agent/a', 'daily/b', { signal });
  const save = {
    slug: 'daily',
    revision: 0,
    title: 'Brief ${topic}',
    parameters: [{ name: 'topic', default: 'news' }],
    steps: [
      {
        id: 'review',
        title: 'Review',
        kind: 'approval' as const,
        prompt: 'Approve ${topic}?',
      },
    ],
  };
  await client.save('agent/a', save);
  const run = {
    revision: 1,
    requestKey: 'stable-request',
    inputs: { topic: 'Benign <script>' },
  };
  await client.run('agent/a', 'daily/b', run);
  await client.run('agent/a', 'daily/b', run);
  await client.delete('agent/a', 'daily/b', { revision: 1 });
  expect(requests.map(({ url }) => url)).toEqual([
    '/api/agents/agent%2Fa/workflow-templates',
    '/api/agents/agent%2Fa/workflow-templates/daily%2Fb',
    '/api/agents/agent%2Fa/workflow-templates',
    '/api/agents/agent%2Fa/workflow-templates/daily%2Fb/run',
    '/api/agents/agent%2Fa/workflow-templates/daily%2Fb/run',
    '/api/agents/agent%2Fa/workflow-templates/daily%2Fb',
  ]);
  expect(
    requests.slice(2).map(({ init }) => JSON.parse(init?.body as string)),
  ).toEqual([save, run, run, { revision: 1 }]);
  expect(requests[0].init?.signal).toBe(signal);
  expect(requests[5].init?.method).toBe('DELETE');
});

it('workflow templates expose conflicts without automatically retrying a run', async () => {
  let calls = 0;
  const client = new WorkflowTemplatesClient(
    createDaemonClient({
      baseUrl: '',
      fetch: async () => {
        calls++;
        return Response.json({ error: 'Refresh template' }, { status: 409 });
      },
    }),
  );
  await expect(
    client.run('a', 'brief', { revision: 1, requestKey: 'x', inputs: {} }),
  ).rejects.toMatchObject({ status: 409 });
  expect(calls).toBe(1);
});
