import { describe, expect, it } from 'vitest';
import { createDaemonClient } from './client.js';
import { ChatModels } from './chat-models.js';

describe('chat model controls', () => {
  it('encodes session scope and keeps review operations distinct from primary selection', async () => {
    const calls: { url: string; method?: string; body?: unknown }[] = [];
    const client = createDaemonClient({ baseUrl: '', fetch: async (url, init) => {
      calls.push({ url: String(url), method: init?.method, body: init?.body ? JSON.parse(String(init.body)) : undefined });
      return Response.json({ preferences: { primary: { mode: 'inherit' }, reviewer: { mode: 'inherit' } }, reviews: [], review: { id: 'r' } });
    } });
    const models = new ChatModels(client);
    const preferences = { primary: { mode: 'inherit' as const }, reviewer: { mode: 'override' as const, provider: 'openai', model: 'gpt-5.4' } };
    await models.update('a/b', 'chat:one?', preferences);
    await models.review('a/b', 'chat:one?', { messageId: 'answer', focus: 'Check assumptions', requestId: 'request-1' });
    await models.cancelReview('a/b', 'chat:one?', 'r/1');
    expect(calls).toEqual([
      { url: '/api/agents/a%2Fb/sessions/chat%3Aone%3F/model-preferences', method: 'PATCH', body: preferences },
      { url: '/api/agents/a%2Fb/sessions/chat%3Aone%3F/reviews', method: 'POST', body: { messageId: 'answer', focus: 'Check assumptions', requestId: 'request-1' } },
      { url: '/api/agents/a%2Fb/sessions/chat%3Aone%3F/reviews/r%2F1/cancel', method: 'POST', body: undefined },
    ]);
  });

  it('forwards read cancellation and retains distinct unavailable errors', async () => {
    const signal = new AbortController().signal;
    let seen: AbortSignal | null | undefined;
    const models = new ChatModels(createDaemonClient({ baseUrl: '', fetch: async (_url, init) => {
      seen = init?.signal;
      return Response.json({ error: 'Choose an available independent reviewer' }, { status: 409 });
    } }));
    await expect(models.get('a', 'chat:one', { signal })).rejects.toMatchObject({ status: 409 });
    expect(seen).toBe(signal);
  });
});
