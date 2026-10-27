import { describe, expect, it } from 'vitest';
import { createDaemonClient, DaemonHttpError } from './client.js';

function harness(respond: (url: string, init?: RequestInit) => Response) {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return respond(String(url), init);
    },
  });
  return { client, requests };
}

describe('ordinary-run browser owner client', () => {
  it('reads setup and profiles without starting a browser', async () => {
    const { client, requests } = harness((url) =>
      url.endsWith('/browser-setup')
        ? Response.json({
            available: false,
            reason: 'runtime_missing',
            setupCommand: 'bun run browser:setup',
          })
        : Response.json({ profiles: [] }),
    );
    const controller = new AbortController();
    expect(
      await client.browserScopes.setup('agent/a', {
        signal: controller.signal,
      }),
    ).toEqual({
      available: false,
      reason: 'runtime_missing',
      setupCommand: 'bun run browser:setup',
    });
    expect(
      await client.browserScopes.profiles('agent/a', {
        signal: controller.signal,
      }),
    ).toEqual([]);
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/browser-setup',
      '/api/agents/agent%2Fa/browser-profiles',
    ]);
    expect(
      requests.every(
        ({ init }) =>
          init?.signal === controller.signal && !init?.body && !init?.method,
      ),
    ).toBe(true);
  });

  it('sends reviewed destinations and exact owner versions without retrying conflicts', async () => {
    const { client, requests } = harness((url) => {
      if (url.endsWith('/revoke')) return Response.json({ revoked: true });
      if (url.endsWith('/resume'))
        return Response.json({ error: 'changed' }, { status: 409 });
      if (url.endsWith('/browser-destinations'))
        return Response.json({ policy: { id: 'destination' } });
      return Response.json({
        scope: { id: 'scope-a', version: 4, ownerControl: true },
      });
    });
    await client.browserScopes.createDestination('agent/a', {
      startUrl: 'https://example.com/form?q=one',
      allowedOrigins: ['https://example.com'],
      authenticated: true,
    });
    await client.browserScopes.revokeDestination('agent/a', 'destination/a', {
      expectedVersion: 1,
    });
    await client.browserScopes.takeover('agent/a', 'run/b', {
      expectedVersion: 3,
    });
    await expect(
      client.browserScopes.resume('agent/a', 'run/b', { expectedVersion: 4 }),
    ).rejects.toBeInstanceOf(DaemonHttpError);
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/browser-destinations',
      '/api/agents/agent%2Fa/browser-destinations/destination%2Fa/revoke',
      '/api/agents/agent%2Fa/runs/run%2Fb/browser-scope/takeover',
      '/api/agents/agent%2Fa/runs/run%2Fb/browser-scope/resume',
    ]);
    expect(
      requests.map(({ init }) => JSON.parse(init?.body as string)),
    ).toEqual([
      {
        startUrl: 'https://example.com/form?q=one',
        allowedOrigins: ['https://example.com'],
        authenticated: true,
      },
      { expectedVersion: 1 },
      { expectedVersion: 3 },
      { expectedVersion: 4 },
    ]);
    expect(requests.every(({ init }) => init?.method === 'POST')).toBe(true);
  });

  it('carries dedicated profiles through versioned sign-in, scope saves and run selection', async () => {
    const { client, requests } = harness(() =>
      Response.json({ profile: { id: 'profile/a' }, scope: {}, run: {} }),
    );
    await client.browserScopes.createProfile('a', { policyId: 'site' });
    await client.browserScopes.startProfileLogin('a', 'profile/a', {
      expectedRevision: 2,
      routeId: 'start',
    });
    await client.browserScopes.finishProfileLogin('a', 'profile/a', {
      expectedRevision: 3,
    });
    await client.browserScopes.revokeProfile('a', 'profile/a', {
      expectedRevision: 4,
    });
    await client.browserScopes.save('a', 'r', {
      policyId: 'site',
      profileId: 'profile/a',
      selectedInputIds: [],
      enabled: true,
    });
    await client.runs.start(
      'a',
      'chat/a',
      {
        text: 'Go',
        browserSelection: {
          policyId: 'site',
          profileId: 'profile/a',
          selectedInputIds: [],
        },
      },
      { idempotencyKey: 'profile-message' },
    );
    expect(requests.slice(0, 4).map(({ url }) => url)).toEqual([
      '/api/agents/a/browser-profiles',
      '/api/agents/a/browser-profiles/profile%2Fa/sign-in/start',
      '/api/agents/a/browser-profiles/profile%2Fa/sign-in/finish',
      '/api/agents/a/browser-profiles/profile%2Fa/revoke',
    ]);
    expect(
      requests.map(({ init }) => JSON.parse(init?.body as string)),
    ).toEqual([
      { policyId: 'site' },
      { expectedRevision: 2, routeId: 'start' },
      { expectedRevision: 3 },
      { expectedRevision: 4 },
      {
        policyId: 'site',
        profileId: 'profile/a',
        selectedInputIds: [],
        enabled: true,
      },
      {
        text: 'Go',
        browserSelection: {
          policyId: 'site',
          profileId: 'profile/a',
          selectedInputIds: [],
        },
      },
    ]);
  });
  it('reads server scopes and policies with encoded ownership and cancellation', async () => {
    const { client, requests } = harness((url) => {
      if (url.endsWith('/browser-policies'))
        return Response.json({ policies: [] });
      if (url.endsWith('/browser-scopes')) return Response.json({ scopes: [] });
      return Response.json({ scope: null });
    });
    expect(client.browserScopes).toBeDefined();
    const controller = new AbortController();
    expect(
      await client.browserScopes.get('agent/a', 'run/b', {
        signal: controller.signal,
      }),
    ).toBeNull();
    expect(
      await client.browserScopes.list('agent/a', { signal: controller.signal }),
    ).toEqual([]);
    expect(
      await client.browserScopes.policies('agent/a', {
        signal: controller.signal,
      }),
    ).toEqual([]);
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/runs/run%2Fb/browser-scope',
      '/api/agents/agent%2Fa/browser-scopes',
      '/api/agents/agent%2Fa/browser-policies',
    ]);
    expect(
      requests.every(({ init }) => init?.signal === controller.signal),
    ).toBe(true);
    expect(requests.every(({ init }) => !init?.body && !init?.method)).toBe(
      true,
    );
  });

  it('pins exact CAS and selected inputs without retrying a failed Stop', async () => {
    const { client, requests } = harness((url) =>
      url.endsWith('/stop')
        ? Response.json({ error: 'Browser scope changed' }, { status: 409 })
        : Response.json({ scope: { id: 'scope-a', version: 3 } }),
    );
    expect(client.browserScopes).toBeDefined();
    await client.browserScopes.save('agent/a', 'run/b', {
      policyId: 'local_quotes',
      selectedInputIds: ['att-input'],
      enabled: false,
      expectedVersion: 2,
    });
    await expect(
      client.browserScopes.stop('agent/a', 'run/b', { expectedVersion: 3 }),
    ).rejects.toBeInstanceOf(DaemonHttpError);
    expect(requests).toHaveLength(2);
    expect(requests[0].init?.method).toBe('PUT');
    expect(JSON.parse(requests[0].init?.body as string)).toEqual({
      policyId: 'local_quotes',
      selectedInputIds: ['att-input'],
      enabled: false,
      expectedVersion: 2,
    });
    expect(requests[1].url).toBe(
      '/api/agents/agent%2Fa/runs/run%2Fb/browser-scope/stop',
    );
    expect(JSON.parse(requests[1].init?.body as string)).toEqual({
      expectedVersion: 3,
    });
  });

  it('leaves initial CAS absent and carries literal chat input with explicit scope selection', async () => {
    const { client, requests } = harness(() =>
      Response.json({ scope: null, run: { id: 'original-run' } }),
    );
    expect(client.browserScopes).toBeDefined();
    await client.browserScopes.save('a', 'r', {
      policyId: 'quotes',
      selectedInputIds: [],
      enabled: true,
    });
    await client.runs.start(
      'a',
      'chat/a',
      {
        text: 'choose "one" \\ literal\n',
        attachmentIds: ['att-input'],
        browserSelection: {
          policyId: 'quotes',
          selectedInputIds: ['att-input'],
        },
      },
      { idempotencyKey: 'same-message' },
    );
    expect(JSON.parse(requests[0].init?.body as string)).not.toHaveProperty(
      'expectedVersion',
    );
    expect(JSON.parse(requests[1].init?.body as string)).toEqual({
      text: 'choose "one" \\ literal\n',
      attachmentIds: ['att-input'],
      browserSelection: { policyId: 'quotes', selectedInputIds: ['att-input'] },
    });
    expect(requests[1].init?.headers).toMatchObject({
      'idempotency-key': 'same-message',
    });
    expect(requests).toHaveLength(2);
  });
});
