import { beforeEach, expect, it, vi } from 'vitest';
import { ownerFetch } from './owner-fetch';
import { clearOwnerApiKey, setOwnerApiKey } from './owner-api-key';
beforeEach(() => { localStorage.clear(); clearOwnerApiKey(); vi.restoreAllMocks(); });
it('sends session credentials in headers for a same-origin private request', async () => {
  localStorage.setItem('anima.consoleSession', 'session-half'); setOwnerApiKey('global-key');
  const fetch = vi.fn().mockResolvedValue(new Response('{}')); vi.stubGlobal('fetch', fetch);
  await ownerFetch('/api/agents', { headers: { accept: 'application/json' } });
  const [url, init] = fetch.mock.calls[0];
  expect(url).toBe('/api/agents');
  expect(new Headers(init.headers).get('x-anima-console-session')).toBe('session-half');
  expect(new Headers(init.headers).get('x-api-key')).toBe('global-key');
  expect(init).toMatchObject({ credentials: 'same-origin', redirect: 'error' });
});
it('never forwards owner headers to another origin', async () => {
  localStorage.setItem('anima.consoleSession', 'secret');
  const fetch = vi.fn(); vi.stubGlobal('fetch', fetch);
  await expect(ownerFetch('https://outside.example/api/agents')).rejects.toThrow(/origin/);
  expect(fetch).not.toHaveBeenCalled();
});
