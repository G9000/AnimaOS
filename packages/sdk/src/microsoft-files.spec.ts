import { expect, it } from 'vitest';
import { createDaemonClient } from './client.js';
import { MicrosoftFilesClient } from './microsoft-files.js';

it('scopes file browsing and selection to an encoded owner and connector', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = new MicrosoftFilesClient(createDaemonClient({ baseUrl: '', fetch: async (url, init) => {
    requests.push({ url: String(url), init });
    return Response.json(init?.method === 'PUT' ? { connector: { id: 'files/a' } } : init?.method === 'DELETE' ? { disconnected: true } : { files: [], nextCursor: null });
  }}));
  const signal = new AbortController().signal;
  await client.files('agent/a', 'files/a', { folderKey: 'folder/a', cursor: 'cursor/a' }, { signal });
  await client.select('agent/a', 'files/a', { expectedGeneration: 'g1', expectedSelectionRevision: 2, fileKeys: ['key1'] });
  await client.disconnect('agent/a', 'files/a', { expectedGeneration: 'g1' });
  expect(requests.map((request) => request.url)).toEqual([
    '/api/agents/agent%2Fa/connectors/microsoft-files/files%2Fa/files?folderKey=folder%2Fa&cursor=cursor%2Fa',
    '/api/agents/agent%2Fa/connectors/microsoft-files/files%2Fa/selection',
    '/api/agents/agent%2Fa/connectors/microsoft-files/files%2Fa',
  ]);
  expect(requests[0].init?.signal).toBe(signal);
  expect(JSON.parse(requests[1].init?.body as string)).toEqual({ expectedGeneration: 'g1', expectedSelectionRevision: 2, fileKeys: ['key1'] });
  expect(requests[2].init?.method).toBe('DELETE');
});
