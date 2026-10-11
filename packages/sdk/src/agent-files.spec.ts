import { describe, expect, it } from 'vitest';
import {
  createDaemonClient,
  DaemonHttpError,
  type AgentFilesPreview,
} from './index.js';

describe('agent file preview client', () => {
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
