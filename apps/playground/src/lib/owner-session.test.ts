import { beforeEach, expect, it, vi } from 'vitest';

beforeEach(() => { vi.resetModules(); localStorage.clear(); window.history.replaceState(null, '', '/'); vi.unstubAllGlobals(); });

it('claims a launch code before probing owner access with the session header', async () => {
  window.history.replaceState(null, '', '/?anima-login=one-use-launch');
  const fetch = vi.fn().mockImplementation((url: string) => Promise.resolve(Response.json(url === '/api/system/session' ? { token: 'signed-header' } : {})));
  vi.stubGlobal('fetch', fetch);
  const { verifyOwnerSession } = await import('./owner-session');
  expect(await verifyOwnerSession()).toBe(200);
  expect(fetch.mock.calls[0][0]).toBe('/api/system/session');
  expect(fetch.mock.calls[1][0]).toBe('/api/capabilities');
  expect(new Headers(fetch.mock.calls[1][1].headers).get('x-anima-console-session')).toBe('signed-header');
  expect(window.location.search).toBe('');
});

it('streams events through fetch with owner headers and supports cancellation', async () => {
  localStorage.setItem('anima.consoleSession', 'signed-header');
  const fetch = vi.fn().mockResolvedValue(new Response('event: swarm:completed\ndata: {"status":"done"}\n\n', { headers: { 'content-type': 'text/event-stream' } }));
  vi.stubGlobal('fetch', fetch);
  const { ownerClient } = await import('./owner-session');
  const stream = ownerClient.subscribe('/api/swarms/one/events');
  expect((await stream.next()).value).toMatchObject({ event: 'swarm:completed' });
  expect(new Headers(fetch.mock.calls[0][1].headers).get('x-anima-console-session')).toBe('signed-header');
  await stream.return();
});
