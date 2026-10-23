import { describe, expect, it } from 'vitest';

import { createDaemonClient } from './index.js';
import type { McpConnection } from './mcp.js';

function connection(overrides: Partial<McpConnection> = {}): McpConnection {
  return {
    id: 'connection/1',
    agentId: 'agent/a',
    label: 'Synthetic server',
    transport: 'streamableHttp',
    stdio: null,
    endpoint: 'https://example.test/mcp',
    enabled: false,
    version: 1,
    hasBearerCredential: false,
    reviewedTools: [],
    state: 'disconnected',
    wireGeneration: 0,
    discoveryId: null,
    candidates: [],
    physicalCallsActive: false,
    error: null,
    ...overrides,
  };
}

function harness(respond: (url: string, init?: RequestInit) => Response) {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return respond(String(url), init);
    },
  });
  return { mcp: client.mcp, requests };
}

describe('MCP owner client', () => {
  it('saves a bearer credential through the versioned owner PUT without echoing the token', async () => {
    const saved = connection({ version: 4, hasBearerCredential: true });
    const token = 'synthetic-m65-token';
    const { mcp, requests } = harness(() =>
      Response.json({ connection: saved }),
    );

    const result = await mcp.saveBearerCredential('agent/a', 'connection/1', {
      expectedVersion: 3,
      bearerToken: token,
    });
    expect(result).toEqual(saved);
    expect(JSON.stringify(result)).not.toContain(token);
    expect(requests).toHaveLength(1);
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/mcp-connections/connection%2F1/credential',
    );
    expect(requests[0].init?.method).toBe('PUT');
    expect(JSON.parse(requests[0].init!.body as string)).toEqual({
      expectedVersion: 3,
      bearerToken: token,
    });
  });

  it('clears a bearer credential through the versioned owner DELETE without retry', async () => {
    const cleared = connection({ version: 5, hasBearerCredential: false });
    const { mcp, requests } = harness(() =>
      Response.json({ connection: cleared }),
    );

    expect(
      await mcp.clearBearerCredential('agent/a', 'connection/1', {
        expectedVersion: 4,
      }),
    ).toEqual(cleared);
    expect(requests).toHaveLength(1);
    expect(requests[0].url).toBe(
      '/api/agents/agent%2Fa/mcp-connections/connection%2F1/credential',
    );
    expect(requests[0].init?.method).toBe('DELETE');
    expect(JSON.parse(requests[0].init!.body as string)).toEqual({
      expectedVersion: 4,
    });
  });

  it('preserves exact HTTP create and sends only the immutable stdio launch body with empty response endpoint', async () => {
    const stdio = {
      executable: 'C:\\synthetic\\mcp-server.exe',
      args: ['--mode', 'one argument with spaces', '"literal quotes"'],
    };
    const local = connection({ transport: 'stdio', stdio, endpoint: '' });
    const { mcp, requests } = harness((_url, init) => {
      const body = init?.body ? JSON.parse(init.body as string) : {};
      return Response.json({ connection: body.stdio ? local : connection() });
    });
    const httpInput = { label: 'HTTP', endpoint: 'https://example.test/mcp' };
    expect(await mcp.create('agent/a', httpInput)).toMatchObject({
      transport: 'streamableHttp',
      stdio: null,
    });
    expect(await mcp.create('agent/a', { label: 'Local', stdio })).toEqual(
      local,
    );
    expect(JSON.parse(requests[0].init!.body as string)).toEqual(httpInput);
    expect(JSON.parse(requests[1].init!.body as string)).toEqual({
      label: 'Local',
      stdio,
    });
    expect(requests).toHaveLength(2);
    expect(requests.map(({ init }) => init?.method)).toEqual(['POST', 'POST']);
  });

  it('uses label and enable patches for stdio without sending launch fields or starting a process', async () => {
    const value = connection({
      transport: 'stdio',
      stdio: { executable: '/synthetic/mcp-server', args: [] },
      endpoint: '',
      version: 3,
    });
    const { mcp, requests } = harness(() =>
      Response.json({ connection: value }),
    );
    expect(await mcp.get('agent/a', 'connection/1')).toEqual(value);
    expect(
      await mcp.update('agent/a', 'connection/1', {
        expectedVersion: 2,
        label: 'Renamed',
        enabled: true,
      }),
    ).toEqual(value);
    expect(JSON.parse(requests[1].init!.body as string)).toEqual({
      expectedVersion: 2,
      label: 'Renamed',
      enabled: true,
    });
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/agent%2Fa/mcp-connections/connection%2F1',
      '/api/agents/agent%2Fa/mcp-connections/connection%2F1',
    ]);
  });

  it('uses encoded paths, exact bodies, envelopes, and canonical grant projection', async () => {
    const current = connection({
      version: 3,
      discoveryId: 'discovery:1',
      candidates: [
        {
          remoteName: 'echo',
          description: 'Echo a value',
          inputSchema: { type: 'object' },
          outputSchema: null,
          descriptorDigest: 'sha256:descriptor',
        },
      ],
    });
    const reviewed = {
      ...current.candidates[0]!,
      alias: 'mcp_echo',
    };
    const calls: string[] = [];
    const { mcp, requests } = harness((url, init) => {
      calls.push(`${init?.method ?? 'GET'} ${url}`);
      if (init?.method === 'DELETE') return new Response(null, { status: 204 });
      if (url.endsWith('/mcp-connections')) {
        if (init?.method === 'POST')
          return Response.json({ connection: current }, { status: 201 });
        return Response.json({ connections: [current] });
      }
      if (url.endsWith('/grants/mcp_echo%2Frecord')) {
        return Response.json({
          connection: { ...current, version: 4, reviewedTools: [reviewed] },
          granted: true,
          toolNames: ['read_file', 'mcp_echo/record'],
        });
      }
      if (url.endsWith('/review'))
        return Response.json({
          connection: { ...current, reviewedTools: [reviewed] },
        });
      if (url.endsWith('/disconnect'))
        return Response.json({
          connection: { ...current, state: 'disconnected' },
        });
      return Response.json({ connection: current });
    });

    expect(await mcp.list('agent/a')).toEqual([current]);
    expect(await mcp.get('agent/a', 'connection/1')).toEqual(current);
    expect(
      await mcp.create('agent/a', {
        label: 'Synthetic server',
        endpoint: 'https://example.test/mcp',
      }),
    ).toEqual(current);
    expect(
      await mcp.update('agent/a', 'connection/1', {
        expectedVersion: 1,
        enabled: true,
      }),
    ).toEqual(current);
    expect(
      await mcp.connect('agent/a', 'connection/1', { expectedVersion: 1 }),
    ).toEqual(current);
    expect(
      await mcp.discover('agent/a', 'connection/1', { expectedVersion: 2 }),
    ).toEqual(current);
    expect(
      await mcp.review('agent/a', 'connection/1', {
        expectedVersion: 2,
        discoveryId: 'discovery:1',
        descriptorDigest: 'sha256:descriptor',
      }),
    ).toMatchObject({ reviewedTools: [reviewed] });

    const grant = await mcp.setGrant(
      'agent/a',
      'connection/1',
      'mcp_echo/record',
      {
        expectedVersion: 3,
        descriptorDigest: 'sha256:descriptor',
        granted: true,
      },
    );
    expect(grant).toEqual({
      connection: { ...current, version: 4, reviewedTools: [reviewed] },
      granted: true,
      toolNames: ['read_file', 'mcp_echo/record'],
    });
    expect(
      await mcp.disconnect('agent/a', 'connection/1', { expectedVersion: 4 }),
    ).toMatchObject({
      state: 'disconnected',
    });
    await mcp.remove('agent/a', 'connection/1', { expectedVersion: 4 });

    expect(calls).toEqual([
      'GET /api/agents/agent%2Fa/mcp-connections',
      'GET /api/agents/agent%2Fa/mcp-connections/connection%2F1',
      'POST /api/agents/agent%2Fa/mcp-connections',
      'PATCH /api/agents/agent%2Fa/mcp-connections/connection%2F1',
      'POST /api/agents/agent%2Fa/mcp-connections/connection%2F1/connect',
      'POST /api/agents/agent%2Fa/mcp-connections/connection%2F1/discover',
      'POST /api/agents/agent%2Fa/mcp-connections/connection%2F1/review',
      'PATCH /api/agents/agent%2Fa/mcp-connections/connection%2F1/grants/mcp_echo%2Frecord',
      'POST /api/agents/agent%2Fa/mcp-connections/connection%2F1/disconnect',
      'DELETE /api/agents/agent%2Fa/mcp-connections/connection%2F1',
    ]);
    expect(requests.map(({ init }) => init?.body)).toEqual([
      undefined,
      undefined,
      JSON.stringify({
        label: 'Synthetic server',
        endpoint: 'https://example.test/mcp',
      }),
      JSON.stringify({ expectedVersion: 1, enabled: true }),
      JSON.stringify({ expectedVersion: 1 }),
      JSON.stringify({ expectedVersion: 2 }),
      JSON.stringify({
        expectedVersion: 2,
        discoveryId: 'discovery:1',
        descriptorDigest: 'sha256:descriptor',
      }),
      JSON.stringify({
        expectedVersion: 3,
        descriptorDigest: 'sha256:descriptor',
        granted: true,
      }),
      JSON.stringify({ expectedVersion: 4 }),
      JSON.stringify({ expectedVersion: 4 }),
    ]);
  });

  it('forwards AbortSignals only on list and get reads', async () => {
    const { mcp, requests } = harness(() => Response.json({ connections: [] }));
    const controller = new AbortController();
    await mcp.list('a', { signal: controller.signal });
    await mcp.get('a', 'c', { signal: controller.signal });
    expect(requests.map(({ init }) => init?.signal)).toEqual([
      controller.signal,
      controller.signal,
    ]);
  });

  it('preserves nullable server fields and returns the server version without inference', async () => {
    const value = connection({
      version: 8,
      error: { code: 'unavailable', message: 'Connection status unavailable' },
      reviewedTools: [
        {
          alias: 'mcp_echo',
          remoteName: 'echo',
          description: 'Echo',
          inputSchema: { type: 'object' },
          outputSchema: null,
          descriptorDigest: 'digest',
        },
      ],
    });
    const { mcp } = harness(() => Response.json({ connection: value }));
    expect(await mcp.get('agent/a', 'connection/1')).toEqual(value);
  });

  it('passes through the unchanged connection version for a checked no-op grant', async () => {
    const value = connection({ version: 7 });
    const { mcp } = harness(() =>
      Response.json({
        connection: value,
        granted: false,
        toolNames: ['read_file'],
      }),
    );

    expect(
      await mcp.setGrant('agent/a', 'connection/1', 'mcp_echo', {
        expectedVersion: 7,
        descriptorDigest: 'digest',
        granted: false,
      }),
    ).toEqual({ connection: value, granted: false, toolNames: ['read_file'] });
  });

  it('does not retry a stale grant mutation or alter its error response', async () => {
    let attempts = 0;
    const { mcp } = harness(() => {
      attempts += 1;
      return Response.json(
        { error: 'MCP connection state changed' },
        { status: 409 },
      );
    });

    await expect(
      mcp.setGrant('agent/a', 'connection/1', 'mcp_echo', {
        expectedVersion: 4,
        descriptorDigest: 'digest',
        granted: false,
      }),
    ).rejects.toMatchObject({
      status: 409,
      body: { error: 'MCP connection state changed' },
    });
    expect(attempts).toBe(1);
  });
});
