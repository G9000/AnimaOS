import { describe, expect, it, vi } from 'vitest';
import { createDaemonClient, DaemonHttpError } from './client.js';

describe('InteractionsClient', () => {
  it('answers a side question through its own scoped route', async () => {
    const result = {
      answer: 'A snapshot answer',
      snapshotAtMs: 4,
      snapshotThroughMessageId: 'm1',
    };
    const fetch = vi.fn(async (_url: unknown, _init?: RequestInit) =>
      Response.json(result),
    );
    const client = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    });
    expect(client).toHaveProperty('interactions');
    expect(
      await client.interactions.sideQuestion('agent /', 'chat:1', 'Why?'),
    ).toEqual(result);
    expect(fetch.mock.calls[0]?.[0]).toBe(
      '/api/agents/agent%20%2F/sessions/chat%3A1/side-question',
    );
    expect(JSON.parse(String(fetch.mock.calls[0]?.[1]?.body))).toEqual({
      question: 'Why?',
    });
  });

  it('submits answers with the displayed revision and complete request scope', async () => {
    const fetch = vi.fn(async (_url: unknown, _init?: RequestInit) =>
      Response.json({ clarification: { id: 'ask /', revision: 2 } }),
    );
    const client = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    });
    expect(client).toHaveProperty('interactions');
    const input = {
      revision: 1,
      answers: [{ questionId: 'q0', choiceIds: ['c1'], text: null }],
    };
    expect(await client.interactions.answer('a', 's', 'ask /', input)).toEqual({
      id: 'ask /',
      revision: 2,
    });
    expect(fetch.mock.calls[0]?.[0]).toBe(
      '/api/agents/a/sessions/s/clarifications/ask%20%2F/answer',
    );
    expect(JSON.parse(String(fetch.mock.calls[0]?.[1]?.body))).toEqual(input);
  });

  it('reads pending questions after a remount and forwards cancellation', async () => {
    const fetch = vi.fn(async (_url: unknown, _init?: RequestInit) =>
      Response.json({ clarifications: [] }),
    );
    const client = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    });
    expect(client).toHaveProperty('interactions');
    const controller = new AbortController();
    expect(
      await client.interactions.list('a', 's', { signal: controller.signal }),
    ).toEqual([]);
    expect(fetch.mock.calls[0]?.[0]).toBe(
      '/api/agents/a/sessions/s/clarifications',
    );
    expect(fetch.mock.calls[0]?.[1]?.signal).toBe(controller.signal);
  });

  it('returns a stale-answer conflict without resubmitting under a new revision', async () => {
    const fetch = vi.fn(async () =>
      Response.json(
        { error: 'This question changed; reload it before answering' },
        { status: 409 },
      ),
    );
    const client = createDaemonClient({
      baseUrl: '',
      fetch: fetch as typeof globalThis.fetch,
    });
    await expect(
      client.interactions.answer('a', 's', 'ask', { revision: 1, answers: [] }),
    ).rejects.toMatchObject({ status: 409 } satisfies Partial<DaemonHttpError>);
    expect(fetch).toHaveBeenCalledTimes(1);
  });
});
