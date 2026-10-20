import { describe, expect, it } from 'vitest';
import { createDaemonClient, type SystemStatus } from './index.js';

describe('system client', () => {
  it('reads the app status from GET /api/system', async () => {
    const calls: { url: string; init?: RequestInit }[] = [];
    const unsupervised: SystemStatus = { supervised: false, version: '1.2.3' };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        calls.push({ url: String(url), init });
        return Response.json(unsupervised);
      },
    });

    expect(await client.system.status()).toEqual(unsupervised);
    expect(calls).toHaveLength(1);
    expect(calls[0].url).toBe('/api/system');
    expect(calls[0].init?.method ?? 'GET').toBe('GET');
  });

  it('posts an action with its force flag', async () => {
    const calls: { url: string; init?: RequestInit }[] = [];
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        calls.push({ url: String(url), init });
        return Response.json({ supervised: false, version: '1.2.3' });
      },
    });

    await client.system.action('stop', { force: true });
    await client.system.action('check-update');

    expect(calls[0].url).toBe('/api/system/actions/stop');
    expect(calls[0].init?.method).toBe('POST');
    expect(calls[0].init?.body).toBe('{"force":true}');
    expect(calls[1].url).toBe('/api/system/actions/check-update');
    expect(calls[1].init?.method).toBe('POST');
  });
});
