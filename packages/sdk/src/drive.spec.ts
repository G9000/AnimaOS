import { describe, expect, it } from 'vitest';
import { createDaemonClient, type ConnectDriveInput } from './index.js';

describe('selected Drive owner transport', () => {
  it('encodes owner scopes and preserves status/consent envelopes with IDs only in the body', async () => {
    const requests: { url: string; init?: RequestInit }[] = [];
    const connector = { id: 'drive /?', type: 'gdrive' };
    const payload = {
      configured: true,
      connector,
      consentUrl: 'https://accounts.google.com/o/oauth2/v2/auth?state=opaque',
    };
    const api = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        requests.push({ url: String(url), init });
        return Response.json(payload);
      },
    }).connectors;
    const input: ConnectDriveInput = { selectedFileIds: ['File_1', 'File-2'] };
    expect(await api.driveStatus('agent /?')).toEqual(payload);
    expect(await api.connectDrive('agent /?', input)).toEqual(payload);
    await expect(
      api.disconnectDrive('agent /?', 'drive /?'),
    ).resolves.toBeUndefined();
    const path = '/api/agents/agent%20%2F%3F/connectors/gdrive';
    expect(requests.map((r) => r.url)).toEqual([
      path,
      path,
      `${path}/drive%20%2F%3F`,
    ]);
    expect(requests.map((r) => r.init?.method)).toEqual([
      undefined,
      'POST',
      'DELETE',
    ]);
    expect(JSON.parse(requests[1].init!.body as string)).toEqual(input);
    expect(requests[2].init?.body).toBeUndefined();
  });
  it('does not retry consent/disconnect failures or expose a browser callback exchange method', async () => {
    let requests = 0;
    const api = createDaemonClient({
      baseUrl: '',
      fetch: async () => {
        requests++;
        return Response.json({ error: 'conflict' }, { status: 409 });
      },
    }).connectors;
    await expect(
      api.connectDrive('a', { selectedFileIds: ['File_1'] }),
    ).rejects.toMatchObject({ status: 409 });
    await expect(api.disconnectDrive('a', 'c')).rejects.toMatchObject({
      status: 409,
    });
    expect(requests).toBe(2);
    expect(api).not.toHaveProperty('completeDriveConnect');
  });
});
