import { describe, expect, it } from 'vitest';
import { DaemonClient } from './client.js';
import { ChatResultsClient } from './results.js';

describe('conversation results client', () => {
  it('encodes exact owner paths and sends revision and generation on pin', async () => {
    const requests: { url: string; init?: RequestInit }[] = [];
    const client = new ChatResultsClient(
      new DaemonClient({
        baseUrl: '',
        fetch: async (url, init) => {
          requests.push({ url: String(url), init });
          return Response.json({
            results: [],
            clearGeneration: 4,
            result: { id: 'result' },
          });
        },
      }),
    );
    await client.list('agent/a', 'chat:one');
    await client.pin('agent/a', 'chat:one', 'id/a', {
      clearGeneration: 4,
      expectedRevision: 2,
      pinned: true,
    });
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/sessions/chat%3Aone/results',
    );
    expect(requests[1].url).toBe(
      '/api/agents/agent%2Fa/sessions/chat%3Aone/results/id%2Fa',
    );
    expect(requests[1].init?.method).toBe('PATCH');
    expect(JSON.parse(String(requests[1].init?.body))).toEqual({
      clearGeneration: 4,
      expectedRevision: 2,
      pinned: true,
    });
  });
});
