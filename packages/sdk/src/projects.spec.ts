import { describe, expect, it, vi } from 'vitest';
import { ProjectsClient } from './projects.js';
import type { DaemonClient } from './client.js';

function fixture() {
  const requestJson = vi.fn().mockResolvedValue({ project: { id: 'shared' }, projects: [], projectSummary: { id: 'shared' } });
  return { client: new ProjectsClient({ requestJson } as unknown as DaemonClient), requestJson };
}

describe('ProjectsClient', () => {
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
});
