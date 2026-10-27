import { expect, it } from 'vitest';
import { createDaemonClient } from './client.js';

it('encodes account search paths, preserves source consent and unwraps cited items', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const source = { connectorId: 'gmail/a', generation: 'generation/a' };
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return Response.json(
        String(url).includes('/items/')
          ? { item: { citationId: 'cite/a' } }
          : init?.method === 'PUT'
            ? { source }
            : { sources: [source], hits: [], pollIntervalMs: 120000 },
      );
    },
  });
  const signal = new AbortController().signal;
  expect(
    (await client.connectedSearch.sources('agent/a', { signal })).sources,
  ).toEqual([source]);
  expect(
    await client.connectedSearch.setSource('agent/a', 'gmail/a', {
      enabled: true,
      expectedGeneration: 'generation/a',
    }),
  ).toEqual(source);
  await client.connectedSearch.refresh('agent/a', { connectorId: 'gmail/a' });
  await client.connectedSearch.query(
    'agent/a',
    { query: 'invoice', kinds: ['mail'], connectorIds: ['gmail/a'], limit: 10 },
    { signal },
  );
  expect(
    await client.connectedSearch.item('agent/a', 'cite/a', { signal }),
  ).toEqual({ citationId: 'cite/a' });
  expect(requests.map((r) => r.url)).toEqual([
    '/api/agents/agent%2Fa/connected-search/sources',
    '/api/agents/agent%2Fa/connected-search/sources/gmail%2Fa',
    '/api/agents/agent%2Fa/connected-search/refresh',
    '/api/agents/agent%2Fa/connected-search/query',
    '/api/agents/agent%2Fa/connected-search/items/cite%2Fa',
  ]);
  expect(requests[0].init?.signal).toBe(signal);
  expect(requests[3].init?.signal).toBe(signal);
  expect(
    requests.slice(1, 4).map((r) => JSON.parse(r.init?.body as string)),
  ).toEqual([
    { enabled: true, expectedGeneration: 'generation/a' },
    { connectorId: 'gmail/a' },
    { query: 'invoice', kinds: ['mail'], connectorIds: ['gmail/a'], limit: 10 },
  ]);
});

it('retains rule revisions and exact bindings without retrying rejected writes', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return Response.json(
        String(url).endsWith('/events')
          ? { events: [{ workflowId: 'flow' }] }
          : !init?.method
            ? { rules: [{ id: 'rule/a' }] }
            : { id: 'rule/a', revision: 2 },
      );
    },
  });
  expect(await client.accountWorkflows.list('agent/a')).toEqual([
    { id: 'rule/a' },
  ]);
  const input = {
    name: 'Invoice',
    connectorId: 'gmail/a',
    eventKind: 'mailObserved' as const,
    templateSlug: 'invoice',
    templateRevision: 1,
    filters: { titleContains: 'invoice' },
    inputs: { document: { type: 'event' as const, field: 'text' as const } },
    enabled: true,
  };
  await client.accountWorkflows.create('agent/a', input);
  await client.accountWorkflows.update('agent/a', 'rule/a', {
    revision: 2,
    enabled: false,
  });
  expect(await client.accountWorkflows.events('agent/a', 'rule/a')).toEqual([
    { workflowId: 'flow' },
  ]);
  await client.accountWorkflows.delete('agent/a', 'rule/a', { revision: 3 });
  expect(requests.map((r) => r.url)).toEqual([
    '/api/agents/agent%2Fa/account-workflow-rules',
    '/api/agents/agent%2Fa/account-workflow-rules',
    '/api/agents/agent%2Fa/account-workflow-rules/rule%2Fa',
    '/api/agents/agent%2Fa/account-workflow-rules/rule%2Fa/events',
    '/api/agents/agent%2Fa/account-workflow-rules/rule%2Fa',
  ]);
  expect(JSON.parse(requests[1].init?.body as string)).toEqual(input);
  expect(requests[2].init?.method).toBe('PATCH');
  expect(requests[4].init?.method).toBe('DELETE');
  let calls = 0;
  const rejected = createDaemonClient({
    baseUrl: '',
    fetch: async () => {
      calls++;
      return Response.json({ error: 'Rule changed; refresh' }, { status: 409 });
    },
  });
  await expect(
    rejected.accountWorkflows.update('a', 'r', { revision: 1, enabled: true }),
  ).rejects.toMatchObject({ status: 409 });
  expect(calls).toBe(1);
});
