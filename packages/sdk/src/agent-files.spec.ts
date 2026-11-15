import { describe, expect, it } from 'vitest';
import {
  createDaemonClient,
  DaemonHttpError,
  type AgentFilesPreview,
  type MemoryProviderBinding,
} from './index.js';

describe('agent file preview client', () => {
  it('converts the legacy binding and rereads an uncertain save without retrying it', async () => {
    const providerBinding: MemoryProviderBinding = {
      version: 1, provider: 'mother', connectionId: 'garden', workspaceId: 'workspace',
      generation: 'generation', subject: 'agent', publication: 'selected',
      personaSource: 'local', accessMode: 'companion',
    };
    const requests: { method: string; body?: unknown }[] = [];
    const saved = { revision: 'after', binding: null, providerBinding, status: 'configured', connections: [] };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (_url, init) => {
        requests.push({ method: init?.method ?? 'GET', body: init?.body ? JSON.parse(String(init.body)) : undefined });
        if (init?.method === 'PUT') throw new TypeError('Synthetic lost acknowledgement');
        return Response.json(saved);
      },
    });
    await expect(client.agentFiles.saveWorkspaceContext('agent', {
      expectedRevision: 'before', binding: null, providerBinding,
    })).rejects.toBeDefined();
    expect(await client.agentFiles.workspaceContext('agent')).toEqual(saved);
    expect(requests).toEqual([
      { method: 'PUT', body: { expectedRevision: 'before', binding: null, providerBinding } },
      { method: 'GET', body: undefined },
    ]);
  });

  it('distinguishes omitted additive settings from an explicit provider disable', async () => {
    const bodies: unknown[] = [];
    const client = createDaemonClient({ baseUrl: '', fetch: async (_url, init) => {
      bodies.push(JSON.parse(String(init?.body)));
      return Response.json({});
    } });
    await client.agentFiles.saveWorkspaceContext('agent', { expectedRevision: 'r1', binding: null });
    await client.agentFiles.saveWorkspaceContext('agent', { expectedRevision: 'r2', binding: null, providerBinding: null });
    expect(bodies).toEqual([
      { expectedRevision: 'r1', binding: null },
      { expectedRevision: 'r2', binding: null, providerBinding: null },
    ]);
  });

  it('reads and disables a workspace binding with the exact owner revision', async () => {
    const requests: { url: string; method?: string; body?: unknown }[] = [];
    const view = {
      revision: 'config-r1',
      binding: null,
      status: 'independent',
      connections: [],
    };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        requests.push({
          url: String(url),
          method: init?.method ?? 'GET',
          body: init?.body ? JSON.parse(String(init.body)) : undefined,
        });
        return Response.json(view);
      },
    });
    expect(await client.agentFiles.workspaceContext('agent/one')).toEqual(view);
    expect(
      await client.agentFiles.saveWorkspaceContext('agent/one', {
        expectedRevision: 'config-r1',
        binding: null,
      }),
    ).toEqual(view);
    expect(requests).toEqual([
      {
        url: '/api/agents/agent%2Fone/workspace-context',
        method: 'GET',
        body: undefined,
      },
      {
        url: '/api/agents/agent%2Fone/workspace-context',
        method: 'PUT',
        body: { expectedRevision: 'config-r1', binding: null },
      },
    ]);
  });

  it('reads the effective personality and saves its exact revision', async () => {
    const calls: {
      url: string;
      method?: string;
      body?: unknown;
      signal?: AbortSignal | null;
    }[] = [];
    const signal = new AbortController().signal;
    const personality = {
      source: 'profile_soul',
      path: '.animaos-swarm/agent-profiles/61/SOUL.md',
      content: 'Be curious.',
      revision: 'r1',
      maxChars: 16000,
    };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        calls.push({
          url: String(url),
          method: init?.method,
          body: init?.body ? JSON.parse(String(init.body)) : undefined,
          signal: init?.signal,
        });
        return Response.json(personality);
      },
    });
    expect(await client.agentFiles.personality('a/b', { signal })).toEqual(
      personality,
    );
    expect(
      await client.agentFiles.savePersonality('a/b', {
        content: 'Be curious.',
        revision: 'r1',
      }),
    ).toEqual(personality);
    expect(calls[0]).toMatchObject({
      url: '/api/agents/a%2Fb/personality',
      signal,
    });
    expect(calls[1]).toMatchObject({
      url: '/api/agents/a%2Fb/personality',
      method: 'PUT',
      body: { content: 'Be curious.', revision: 'r1' },
    });
  });

  it('preserves a stale personality conflict for the editor', async () => {
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async () =>
        Response.json(
          { error: 'Personality changed; reload before saving' },
          { status: 409 },
        ),
    });
    await expect(
      client.agentFiles.savePersonality('a', {
        content: 'draft',
        revision: 'old',
      }),
    ).rejects.toMatchObject({ status: 409 });
  });
  it('encodes the agent ID, propagates cancellation and returns content-free metadata', async () => {
    const signal = new AbortController().signal;
    const manifest: AgentFilesPreview = {
      mode: 'current_preview',
      availability: 'unavailable_workspace',
      capturedAtMs: 1,
      snapshotDigest: 'fixture-digest',
      identitySource: 'legacy',
      projectSourceKind: null,
      estimatedTokens: 0,
      entries: [],
    };
    let request:
      | { url: string; signal?: AbortSignal | null; body?: RequestInit['body'] }
      | undefined;
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        request = { url: String(url), signal: init?.signal, body: init?.body };
        return Response.json(manifest);
      },
    });
    expect(await client.agentFiles.preview('a/b?#', { signal })).toEqual(
      manifest,
    );
    expect(request).toEqual({
      url: '/api/agents/a%2Fb%3F%23/agent-files',
      signal,
      body: undefined,
    });
  });

  it.each([401, 403, 404, 500])('preserves HTTP %s errors', async (status) => {
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async () => Response.json({ error: 'unavailable' }, { status }),
    });
    await expect(client.agentFiles.preview('a')).rejects.toMatchObject({
      name: 'DaemonHttpError',
      status,
    });
    await expect(client.agentFiles.preview('a')).rejects.toBeInstanceOf(
      DaemonHttpError,
    );
  });

  it('preserves aborts without translating them into connection errors', async () => {
    const client = createDaemonClient({
      fetch: async () => {
        throw new DOMException('Aborted', 'AbortError');
      },
    });
    await expect(client.agentFiles.preview('a')).rejects.toMatchObject({
      name: 'AbortError',
    });
  });
});
