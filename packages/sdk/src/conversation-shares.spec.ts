import { expect, it } from 'vitest';
import { DaemonClient } from './client.js';
import { ConversationSharesClient, SharedConversationClient } from './conversation-shares.js';

it('keeps viewer credentials out of URLs and owner transport, cookies, and referrers', async () => {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = new SharedConversationClient({
    baseUrl: 'http://127.0.0.1:8080/',
    viewerId: 'viewer-one',
    secret: 'separate-secret',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return Response.json({ title: 'Selected', messages: [], nextBefore: null });
    },
  });
  await client.messages('share-one', { before: 'message/one' });
  expect(requests[0].url).toBe('http://127.0.0.1:8080/api/shared/conversations/share-one/messages?before=message%2Fone');
  expect(requests[0].init).toMatchObject({
    method: 'GET', credentials: 'omit', cache: 'no-store', referrerPolicy: 'no-referrer',
    headers: { 'x-anima-viewer-id': 'viewer-one', 'x-anima-viewer-secret': 'separate-secret' },
  });
  expect(new Headers(requests[0].init?.headers).has('authorization')).toBe(false);
});

it('does not follow redirects with viewer credentials', async () => {
  let redirect: RequestInit['redirect'];
  const client = new SharedConversationClient({
    viewerId: 'viewer', secret: 'secret',
    fetch: async (_url, init) => { redirect = init?.redirect; return Response.json({}); },
  });
  await client.get('share');
  expect(redirect).toBe('error');
});

it('owner operations use only the selected conversation and return credentials once', async () => {
  const requests: { url: string; method?: string; body?: RequestInit['body'] }[] = [];
  const shares = new ConversationSharesClient(new DaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), method: init?.method, body: init?.body });
      return Response.json({ grants: [] });
    },
  }));
  await shares.list('agent/a', 'chat:one');
  await shares.create('agent/a', 'chat:one', { viewerName: 'Colleague', clearGeneration: 3 });
  await shares.revoke('agent/a', 'chat:one', 'share-one');
  expect(requests.map(({ url, method }) => [url, method ?? 'GET'])).toEqual([
    ['/api/agents/agent%2Fa/sessions/chat%3Aone/shares', 'GET'],
    ['/api/agents/agent%2Fa/sessions/chat%3Aone/shares', 'POST'],
    ['/api/agents/agent%2Fa/sessions/chat%3Aone/shares/share-one', 'DELETE'],
  ]);
  expect(requests[1].body).toBe(JSON.stringify({ viewerName: 'Colleague', clearGeneration: 3 }));
});
