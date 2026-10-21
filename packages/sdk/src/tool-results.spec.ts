import { expect, it } from 'vitest';
import { DaemonClient, DaemonHttpError } from './client.js';
import { ToolResultsClient } from './tool-results.js';

it('encodes owned artifact paths, bounded paging and cancellation', async () => {
  const requests: { url: string; signal: AbortSignal | null | undefined }[] =
    [];
  const page = {
    artifact: { id: 'tr_one' },
    text: 'saved text',
    offset: 8,
    nextOffset: 12,
  };
  const client = new ToolResultsClient(
    new DaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        requests.push({ url: String(url), signal: init?.signal });
        return Response.json(page);
      },
    }),
  );
  const signal = new AbortController().signal;
  expect(
    await client.read('agent/a', 'result/id', {
      offset: 8,
      limit: 128,
      signal,
    }),
  ).toEqual(page);
  expect(requests).toEqual([
    {
      url: '/api/agents/agent%2Fa/tool-results/result%2Fid?offset=8&limit=128',
      signal,
    },
  ]);
});

it('downloads the full stored payload and exposes unavailable storage', async () => {
  let requested = '';
  const client = new ToolResultsClient(
    new DaemonClient({
      baseUrl: '',
      fetch: async (url) => {
        requested = String(url);
        return new Response('full payload');
      },
    }),
  );
  expect(await (await client.download('agent', 'artifact')).text()).toBe(
    'full payload',
  );
  expect(requested).toBe('/api/agents/agent/tool-results/artifact/download');
  const unavailable = new ToolResultsClient(
    new DaemonClient({
      baseUrl: '',
      fetch: async () =>
        Response.json({ error: 'unavailable' }, { status: 503 }),
    }),
  );
  await expect(unavailable.read('agent', 'artifact')).rejects.toBeInstanceOf(
    DaemonHttpError,
  );
});
