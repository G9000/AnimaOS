import { describe, expect, it, vi } from 'vitest';
import { ProjectsClient } from './projects.js';
import type { ProjectKnowledgeSearch, ProjectKnowledgeStatus } from './projects.js';
import type { DaemonClient } from './client.js';
import { createDaemonClient } from './client.js';

function fixture() {
  const requestJson = vi.fn().mockResolvedValue({ project: { id: 'shared' }, projects: [], projectSummary: { id: 'shared' } });
  return { client: new ProjectsClient({ requestJson } as unknown as DaemonClient), requestJson };
}

describe('ProjectsClient', () => {
  it('previews a citation page from its identity and current member scope without a caller page or source URL', async () => {
    const fetch = vi.fn(async () => new Response(new Uint8Array([1, 2, 3]), {
      headers: { 'content-type': 'image/png' },
    }));
    const client = createDaemonClient({ baseUrl: '', fetch }).projects;
    const signal = new AbortController().signal;
    const page = await client.previewKnowledgeCitationPage('p/a', 'opaque/a', 'agent one', { signal });
    expect(page.type).toBe('image/png');
    const [url, init] = fetch.mock.calls[0] as unknown as [string, RequestInit];
    expect(url).toBe('/api/projects/p%2Fa/knowledge/citations/opaque%2Fa/page?asAgentId=agent+one');
    expect(init.signal).toBe(signal);
    expect(init.cache).toBe('no-store');
    expect(new Headers(init.headers).get('accept')).toBe('image/png');
  });

  it('shares only a selected connected citation with explicit project consent and a revision witness', async () => {
    const { client, requestJson } = fixture();
    const signal = new AbortController().signal;
    await client.connectedSourceChoices('p/a', 'b', { query: 'annual & report', signal });
    expect(requestJson).toHaveBeenLastCalledWith('/api/projects/p%2Fa/knowledge/connected-sources?asAgentId=b&query=annual+%26+report', { signal });
    const input = { expectedRevision: 5, requestKey: 'connected-grant', asAgentId: 'b', ownerAgentId: 'a', connectorId: 'drive-one', citationId: 'selected-citation', shareWithProject: true as const };
    expect(await client.addConnectedSource('p/a', input)).toEqual({ id: 'shared' });
    expect(requestJson).toHaveBeenLastCalledWith('/api/projects/p%2Fa/knowledge/connected-sources', { method: 'POST', body: input });
  });
  it('keeps profile project identity separate from the explicit requesting member', async () => {
    const { client, requestJson } = fixture();
    const controller = new AbortController();
    await client.get('project:a/b', 'agent a', { signal: controller.signal });
    expect(requestJson).toHaveBeenCalledWith('/api/projects/project%3Aa%2Fb?asAgentId=agent+a', { signal: controller.signal });
    await client.list('agent-b');
    expect(requestJson).toHaveBeenLastCalledWith('/api/projects?memberAgentId=agent-b&status=active', { signal: undefined });
  });

  it('retries creation with the exact caller key and never invents a replacement key', async () => {
    const { client, requestJson } = fixture();
    const input = { requestKey: 'durable-create', title: 'Research', memberAgentIds: ['a', 'b'] };
    await client.create(input);
    await client.create(input);
    expect(requestJson.mock.calls[0]).toEqual(requestJson.mock.calls[1]);
    expect(requestJson).toHaveBeenCalledWith('/api/projects', { method: 'POST', body: input });
  });

  it('keeps member replacement, exact source identity and CAS revision in mutation bodies', async () => {
    const { client, requestJson } = fixture();
    await client.setMembers('p', { expectedRevision: 4, agentIds: ['b'] });
    expect(requestJson).toHaveBeenCalledWith('/api/projects/p/members', { method: 'PUT', body: { expectedRevision: 4, agentIds: ['b'] } });
    const source = { expectedRevision: 5, requestKey: 'pin-one', asAgentId: 'b',
      target: { kind: 'chat_excerpt' as const, ownerAgentId: 'a', sessionId: 'chat:one', clearGeneration: 2, messageIds: ['reply-1'] },
      role: 'reference' as const, useInContext: true };
    await client.addResource('p', source);
    expect(requestJson).toHaveBeenLastCalledWith('/api/projects/p/resources', { method: 'POST', body: source });
  });

  it('source preview can address only a stored resource, never an arbitrary donor message', async () => {
    const { client, requestJson } = fixture();
    await client.previewResource('p', 'r/a', 'b');
    expect(requestJson).toHaveBeenCalledWith('/api/projects/p/resources/r%2Fa/preview?asAgentId=b', {});
    await client.manage();
    expect(requestJson).toHaveBeenLastCalledWith('/api/projects/manage', {});
  });

  it('grants exactly the chosen folder through the dedicated revision-checked endpoint', async () => {
    const { client, requestJson } = fixture();
    const input = { expectedRevision: 4, requestKey: 'folder-grant', asAgentId: 'b', rootPath: 'D:\\Research', label: 'Research' };
    expect(await client.addKnowledgeFolder('p/a', input)).toEqual({ id: 'shared' });
    expect(requestJson).toHaveBeenLastCalledWith('/api/projects/p%2Fa/knowledge/sources', { method: 'POST', body: input });
  });

  it('preserves knowledge bindings and passages in the documented response envelopes', async () => {
    const binding = { projectRevision: 4, accessEpoch: 2, memberGeneration: 3 };
    const hit = { citationId: 'opaque', resourceId: 'r', documentId: 'd', title: 'Annual plan', locator: 'Page 2', excerpt: 'Relevant evidence' };
    const status: ProjectKnowledgeStatus = { ...binding, sources: [{ resourceId: 'r', status: 'partial', documentCount: 2, indexedAtMs: 20, generation: 1, omittedCount: 1, error: 'One file is busy' }] };
    const search: ProjectKnowledgeSearch = { ...status, hits: [hit], retrieval: {
      mode: 'hybrid', semanticState: 'partial', provider: 'fastembed', model: 'local-model',
      indexedWindows: 3, eligibleWindows: 5, omittedWindows: 2,
    } };
    const legacySearch: ProjectKnowledgeSearch = { ...status, hits: [] };
    const responses = [status, status, search, { ...binding, ...hit, text: 'Verified bounded passage' }, legacySearch];
    const fetch = vi.fn(async () => Response.json(responses.shift()));
    const client = createDaemonClient({ baseUrl: '', fetch }).projects;
    const signal = new AbortController().signal;
    expect(await client.knowledgeStatus('p', 'a', { signal })).toEqual(status);
    expect(await client.refreshKnowledge('p', { asAgentId: 'a', resourceId: 'r' }, { signal })).toEqual(status);
    expect(await client.searchKnowledge('p', 'a', { query: 'annual & plan', limit: 8, signal })).toEqual(search);
    expect(await client.openKnowledgeCitation('p', 'opaque/a', 'a', { signal })).toEqual({ ...binding, ...hit, text: 'Verified bounded passage' });
    expect(await client.searchKnowledge('p', 'a', { query: 'legacy', signal })).toEqual(legacySearch);
    const urls = fetch.mock.calls.map((call) => String((call as unknown as [string])[0]));
    expect(urls).toEqual([
      '/api/projects/p/knowledge/status?asAgentId=a', '/api/projects/p/knowledge/refresh',
      '/api/projects/p/knowledge/search?asAgentId=a&query=annual+%26+plan&limit=8',
      '/api/projects/p/knowledge/citations/opaque%2Fa?asAgentId=a',
      '/api/projects/p/knowledge/search?asAgentId=a&query=legacy',
    ]);
    for (const call of fetch.mock.calls) expect((call as unknown as [string, RequestInit])[1].signal).toBe(signal);
  });
});
