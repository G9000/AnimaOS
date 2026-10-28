import { act, renderHook, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AgentEvent, Project, ProjectsClient } from '@animaos/sdk';
import { useProjects } from './useProjects';

const events = vi.hoisted(() => ({ status: 'open', listener: undefined as undefined | ((event: AgentEvent) => void) }));
vi.mock('./useAgentEvents', () => ({ useAgentEvents: (_agent: string, listener: (event: AgentEvent) => void) => {
  events.listener = listener; return { status: events.status };
} }));
vi.mock('../lib/daemon-api', () => ({ daemon: { projects: {} } }));
const project: Project = { id: 'project_one', title: 'Shared brief', revision: 3, accessEpoch: 1, status: 'active',
  members: [{ agentId: 'a', generation: 1, joinedAtMs: 1 }, { agentId: 'b', generation: 1, joinedAtMs: 1 }],
  createdAtMs: 1, updatedAtMs: 2, chatCount: 0, resourceCount: 0, instructions: 'Selected instructions', resources: [] };
function client() {
  return { list: vi.fn().mockResolvedValue([project]), get: vi.fn().mockResolvedValue(project),
    chats: vi.fn().mockResolvedValue({ chats: [], nextCursor: null, projectRevision: 3, accessEpoch: 1, memberGeneration: 1 }),
    previewResource: vi.fn().mockResolvedValue({ projectId: project.id, projectRevision: 3, accessEpoch: 1, memberGeneration: 1,
      resourceId: 'resource_one', state: 'ready', text: 'Shared source payload' }),
    context: vi.fn().mockResolvedValue({ projectId: project.id, projectRevision: 3, accessEpoch: 1, memberGeneration: 1,
      estimatedTokens: 10, basis: 'current_preview', resources: [] }),
  };
}
describe('project scope lifetime', () => {
  beforeEach(() => { events.status = 'open'; events.listener = undefined; });
  it('removes cached shared payloads on a disconnect', async () => {
    const api = client();
    const { result, rerender } = renderHook(() => useProjects('a', project.id, true, api as unknown as ProjectsClient));
    await waitFor(() => expect(result.current.project?.id).toBe(project.id));
    await act(() => result.current.readPreview('resource_one'));
    expect(result.current.preview?.text).toBe('Shared source payload');
    events.status = 'reconnecting'; rerender();
    expect(result.current.project).toBeNull(); expect(result.current.preview).toBeNull(); expect(result.current.chats).toEqual([]);
    await waitFor(() => expect(result.current.loading).toBe(false));
  });
  it('ignores a donor preview that resolves after a member invalidation', async () => {
    const api = client(); let resolve!: (value: unknown) => void;
    api.previewResource.mockImplementation(() => new Promise((done) => { resolve = done; }));
    const { result } = renderHook(() => useProjects('a', project.id, true, api as unknown as ProjectsClient));
    await waitFor(() => expect(result.current.project).not.toBeNull());
    let pending!: Promise<void>; act(() => { pending = result.current.readPreview('resource_one'); });
    act(() => events.listener?.({ type: 'project.updated' } as AgentEvent));
    await act(async () => { resolve({ projectId: project.id, projectRevision: 3, accessEpoch: 1, memberGeneration: 1,
      resourceId: 'resource_one', state: 'ready', text: 'Revoked payload' }); await pending; });
    expect(result.current.preview).toBeNull();
  });
  it('does not reuse a late read when switching member agents within one project', async () => {
    const api = client(); let resolve!: (value: unknown) => void;
    api.get.mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
    const { result, rerender } = renderHook(({ agent }) => useProjects(agent, project.id, true, api as unknown as ProjectsClient), { initialProps: { agent: 'a' } });
    await waitFor(() => expect(api.get).toHaveBeenCalledTimes(1));
    api.get.mockResolvedValue({ ...project, title: 'Current member view' });
    rerender({ agent: 'b' });
    await waitFor(() => expect(result.current.project?.title).toBe('Current member view'));
    await act(async () => resolve({ ...project, title: 'Stale member view' }));
    expect(result.current.project?.title).toBe('Current member view');
  });
  it('rejects coverage from a different member generation', async () => {
    const api = client(); api.context.mockResolvedValue({ projectId: project.id, projectRevision: 3, accessEpoch: 1,
      memberGeneration: 9, estimatedTokens: 10, basis: 'current_preview', resources: [] });
    const { result } = renderHook(() => useProjects('a', project.id, true, api as unknown as ProjectsClient));
    await waitFor(() => expect(result.current.project).not.toBeNull());
    await act(() => result.current.readCoverage());
    expect(result.current.coverage).toBeNull();
  });
});
