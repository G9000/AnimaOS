import { describe, expect, it } from 'vitest';
import { createDaemonClient } from './index.js';

describe('selected memory publications', () => {
  it('preserves selection IDs and rereads after an uncertain admission without replaying', async () => {
    const calls: { url: string; method: string; body: unknown }[] = [];
    const saved = { id: 'publication', state: 'queued', revision: 1 };
    const client = createDaemonClient({ baseUrl: '', fetch: async (url, init) => {
      calls.push({ url: String(url), method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : null });
      if (init?.method === 'POST') throw new TypeError('Synthetic acknowledgement lost');
      return Response.json(saved);
    } });
    const selection = { publicationId: 'publication', localId: 'local', expectedContent: 'A selected synthetic fact', title: 'A fact', source: 'Explicit owner selection', basis: 'owner_statement' as const, delivery: 'proposal' as const };
    await expect(client.agentFiles.shareMemory('agent/one', selection)).rejects.toBeDefined();
    expect(await client.agentFiles.memoryPublication('agent/one', 'publication')).toEqual(saved);
    expect(calls).toEqual([
      { url: '/api/agents/agent%2Fone/memory-publications', method: 'POST', body: selection },
      { url: '/api/agents/agent%2Fone/memory-publications/publication', method: 'GET', body: null },
    ]);
  });

  it('carries revision-checked control requests and abortable list reads', async () => {
    const controller = new AbortController();
    const calls: { url: string; init?: RequestInit }[] = [];
    const client = createDaemonClient({ baseUrl: '', fetch: async (url, init) => {
      calls.push({ url: String(url), init }); return Response.json({});
    } });
    await client.agentFiles.memoryPublications('a', { signal: controller.signal });
    await client.agentFiles.controlMemoryPublication('a', 'p/?', { expectedRevision: 4, action: 'resume' });
    expect(calls[0]?.init?.signal).toBe(controller.signal);
    expect(calls[1]?.url).toBe('/api/agents/a/memory-publications/p%2F%3F/control');
    expect(JSON.parse(String(calls[1]?.init?.body))).toEqual({ expectedRevision: 4, action: 'resume' });
  });
});
