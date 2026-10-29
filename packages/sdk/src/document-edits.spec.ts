import { expect, it } from 'vitest';
import { createDaemonClient } from './client.js';
import { DocumentEditsClient } from './document-edits.js';

it('scopes review and cancellation to the exact owner, session and proposal', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = new DocumentEditsClient(createDaemonClient({ baseUrl: '', fetch: async (url, init) => {
    requests.push({ url: String(url), init });
    return Response.json(init?.method === 'DELETE' ? { version: 1, proposal_id: 'proposal/a', cancelled: true } : { version: 1 });
  }}));
  const signal = new AbortController().signal;
  await client.review('proposal/a', 'agent/a', 'chat:1', { signal });
  await client.cancel('proposal/a', 'agent/a', 'chat:1');
  expect(requests.map(({ url }) => url)).toEqual([
    '/api/document-edits/proposal%2Fa?agent_id=agent%2Fa&session_id=chat%3A1',
    '/api/document-edits/proposal%2Fa?agent_id=agent%2Fa&session_id=chat%3A1',
  ]);
  expect(requests[0].init?.signal).toBe(signal);
  expect(requests[1].init?.method).toBe('DELETE');
  expect(requests[1].init?.body).toBeUndefined();
});

it('requests exact workspace inventory with owner scope, cursor and cancellation', async () => {
  let request: { url: string; init?: RequestInit } | undefined;
  const client = new DocumentEditsClient(createDaemonClient({ baseUrl: '', fetch: async (url, init) => {
    request = { url: String(url), init };
    return Response.json({ version: 1, targets: [] });
  }}));
  const signal = new AbortController().signal;
  await client.inspectWorkspace('artifacts/report a.html', 'agent/a', 'chat:1', { cursor: 'next/a', signal });
  expect(request?.url).toBe('/api/document-edits/source?agent_id=agent%2Fa&session_id=chat%3A1&file_path=artifacts%2Freport+a.html&cursor=next%2Fa');
  expect(request?.init?.signal).toBe(signal);
});
