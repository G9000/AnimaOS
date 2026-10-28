import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Project, ProjectsClient } from '@animaos/sdk';
import { DaemonHttpError } from '@animaos/sdk';
import { KNOWLEDGE_POLL_MS, useProjectKnowledge } from './useProjectKnowledge';

vi.mock('../lib/daemon-api', () => ({ daemon: { projects: {} } }));
const project: Project = { id: 'p', title: 'Research', revision: 4, accessEpoch: 2, status: 'active',
  members: [{ agentId: 'a', generation: 3, joinedAtMs: 1 }], resources: [], instructions: '',
  createdAtMs: 1, updatedAtMs: 1, resourceCount: 1, chatCount: 0 };
const binding = { projectRevision: 4, accessEpoch: 2, memberGeneration: 3 };
const hit = { citationId: 'c', resourceId: 'r', documentId: 'd', title: 'Research.pdf', locator: 'Page 8', excerpt: 'Evidence excerpt' };
function client() {
  return { knowledgeStatus: vi.fn().mockResolvedValue({ ...binding, sources: [] }),
    searchKnowledge: vi.fn().mockResolvedValue({ ...binding, sources: [], hits: [hit] }),
    openKnowledgeCitation: vi.fn().mockResolvedValue({ ...binding, ...hit, text: 'Verified passage' }),
    refreshKnowledge: vi.fn().mockResolvedValue({ ...binding, sources: [] }) };
}
describe('project knowledge lifetime', () => {
  afterEach(() => vi.useRealTimers());
  it('clears and aborts a late search when project revision changes', async () => {
    const api = client(); let resolve!: (value: unknown) => void;
    api.searchKnowledge.mockImplementation(() => new Promise((done) => { resolve = done; }));
    const { result, rerender } = renderHook(({ selected }) => useProjectKnowledge('a', selected, true, api as unknown as ProjectsClient, vi.fn()),
      { initialProps: { selected: project as Project | null } });
    let pending!: Promise<void>;
    act(() => { pending = result.current.search('evidence'); });
    const signal = api.searchKnowledge.mock.calls[0][2].signal as AbortSignal;
    rerender({ selected: { ...project, revision: 5 } });
    expect(signal.aborted).toBe(true);
    await act(async () => { resolve({ ...binding, sources: [], hits: [hit] }); await pending; });
    expect(result.current.hits).toEqual([]);
  });

  it.each(['accessEpoch', 'memberGeneration'] as const)('refuses a citation with mismatched %s', async (field) => {
    const api = client(); const invalidate = vi.fn();
    api.openKnowledgeCitation.mockResolvedValue({ ...binding, ...hit, text: 'Stale passage', [field]: 99 });
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, invalidate));
    await act(() => result.current.openCitation('c'));
    expect(result.current.citation).toBeNull(); expect(invalidate).toHaveBeenCalled();
  });

  it('drops cached passages immediately on access loss and ignores a late citation', async () => {
    const api = client(); let resolve!: (value: unknown) => void;
    api.openKnowledgeCitation.mockImplementation(() => new Promise((done) => { resolve = done; }));
    const { result, rerender } = renderHook(({ selected }) => useProjectKnowledge('a', selected, true, api as unknown as ProjectsClient, vi.fn()),
      { initialProps: { selected: project as Project | null } });
    await act(() => result.current.search('evidence'));
    expect(result.current.hits).toHaveLength(1);
    let pending!: Promise<void>; act(() => { pending = result.current.openCitation('c'); });
    rerender({ selected: null });
    expect(result.current.hits).toEqual([]); expect(result.current.citation).toBeNull();
    await act(async () => { resolve({ ...binding, ...hit, text: 'Revoked passage' }); await pending; });
    expect(result.current.citation).toBeNull();
  });

  it('clears old search and citation content before a refresh', async () => {
    const api = client();
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, vi.fn()));
    await act(() => result.current.search('evidence')); await act(() => result.current.openCitation('c'));
    expect(result.current.citation?.text).toBe('Verified passage');
    await act(() => result.current.refresh('r'));
    expect(result.current.hits).toEqual([]); expect(result.current.citation).toBeNull();
    expect(api.refreshKnowledge).toHaveBeenCalledWith('p', { asAgentId: 'a', resourceId: 'r' }, { signal: expect.any(AbortSignal) });
  });

  it('loads source coverage with the exact selected member', async () => {
    const api = client();
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, vi.fn()));
    await waitFor(() => expect(result.current.status).not.toBeNull());
    expect(api.knowledgeStatus).toHaveBeenCalledWith('p', 'a', { signal: expect.any(AbortSignal) });
  });

  it('aborts reads from a replaced transport even when the project identity is unchanged', async () => {
    const first = client(); const second = client(); let resolve!: (value: unknown) => void;
    first.searchKnowledge.mockImplementation(() => new Promise((done) => { resolve = done; }));
    const { result, rerender } = renderHook(({ api }) => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, vi.fn()),
      { initialProps: { api: first } });
    let pending!: Promise<void>; act(() => { pending = result.current.search('evidence'); });
    const signal = first.searchKnowledge.mock.calls[0][2].signal as AbortSignal;
    rerender({ api: second });
    expect(signal.aborted).toBe(true);
    await act(async () => { resolve({ ...binding, sources: [], hits: [hit] }); await pending; });
    expect(result.current.hits).toEqual([]);
  });

  it('clears results and rechecks project access when a changed citation is refused', async () => {
    const api = client(); const invalidate = vi.fn();
    api.openKnowledgeCitation.mockRejectedValue(new DaemonHttpError(409, { error: 'Source changed. Refresh its index.' }));
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, invalidate));
    await act(() => result.current.search('evidence'));
    await act(() => result.current.openCitation('c'));
    expect(result.current.hits).toEqual([]); expect(result.current.citation).toBeNull();
    expect(result.current.error).toBe('Source changed. Refresh its index.');
    expect(invalidate).toHaveBeenCalled();
  });

  it('polls only while indexing and clears evidence when its generation changes', async () => {
    vi.useFakeTimers();
    const api = client(); const source = { resourceId: 'r', status: 'indexing', documentCount: 1, generation: 1, omittedCount: 0, indexedAtMs: null };
    api.knowledgeStatus.mockResolvedValueOnce({ ...binding, sources: [source] })
      .mockResolvedValue({ ...binding, sources: [{ ...source, status: 'ready', generation: 2 }] });
    api.searchKnowledge.mockResolvedValue({ ...binding, sources: [source], hits: [hit] });
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, vi.fn()));
    await act(async () => { await Promise.resolve(); });
    await act(() => result.current.search('evidence'));
    expect(result.current.hits).toHaveLength(1);
    await act(async () => { await vi.advanceTimersByTimeAsync(KNOWLEDGE_POLL_MS); });
    expect(api.knowledgeStatus).toHaveBeenCalledTimes(2);
    expect(result.current.hits).toEqual([]);
    await act(async () => { await vi.advanceTimersByTimeAsync(KNOWLEDGE_POLL_MS * 3); });
    expect(api.knowledgeStatus).toHaveBeenCalledTimes(2);
  });

  it.each([1, 2])('does not publish delayed generation %s status over a newer accepted search', async (olderGeneration) => {
    const api = client(); let resolve!: (value: unknown) => void;
    const source = { resourceId: 'r', status: 'ready', documentCount: 1, generation: 2, omittedCount: 0, indexedAtMs: 1 };
    api.knowledgeStatus.mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
    api.searchKnowledge.mockResolvedValue({ ...binding, sources: [source], hits: [hit] });
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, vi.fn()));
    await act(() => result.current.search('evidence'));
    expect(result.current.hits).toEqual([hit]);
    await act(async () => resolve({ ...binding, sources: [{ ...source, status: 'indexing', generation: olderGeneration }] }));
    expect(result.current.hits).toEqual([hit]);
    expect(result.current.status?.sources[0].generation).toBe(2);
    expect(result.current.status?.sources[0].status).toBe('ready');
  });

  it('rejects a lower source generation until the authorization scope has reset', async () => {
    const api = client();
    const source = { resourceId: 'r', status: 'ready', documentCount: 1, generation: 2, omittedCount: 0, indexedAtMs: 1 };
    api.knowledgeStatus.mockResolvedValue({ ...binding, sources: [source] });
    api.refreshKnowledge.mockResolvedValue({ ...binding, sources: [{ ...source, generation: 1 }] });
    const { result, rerender } = renderHook(({ selected }) => useProjectKnowledge('a', selected, true, api as unknown as ProjectsClient, vi.fn()),
      { initialProps: { selected: project as Project | null } });
    await waitFor(() => expect(result.current.status?.sources[0].generation).toBe(2));
    await act(() => result.current.refresh('r'));
    expect(result.current.status?.sources[0].generation).toBe(2);
    rerender({ selected: null });
    api.knowledgeStatus.mockResolvedValue({ ...binding, sources: [{ ...source, generation: 1 }] });
    rerender({ selected: project });
    await waitFor(() => expect(result.current.status?.sources[0].generation).toBe(1));
  });

  it('ignores an older status failure after a newer search has succeeded', async () => {
    const api = client(); let reject!: (error: unknown) => void;
    const source = { resourceId: 'r', status: 'ready', documentCount: 1, generation: 2, omittedCount: 0, indexedAtMs: 1 };
    api.knowledgeStatus.mockImplementationOnce(() => new Promise((_done, fail) => { reject = fail; }));
    api.searchKnowledge.mockResolvedValue({ ...binding, sources: [source], hits: [hit] });
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, vi.fn()));
    await act(() => result.current.search('evidence'));
    await act(async () => reject(new Error('Older status transport failed')));
    expect(result.current.hits).toEqual([hit]);
    expect(result.current.status?.sources[0].status).toBe('ready'); expect(result.current.error).toBeNull();
  });

  it.each(['response', 'failure'])('ignores an older refresh %s after a newer search has succeeded', async (completion) => {
    const api = client(); let resolve!: (value: unknown) => void; let reject!: (error: unknown) => void;
    const source = { resourceId: 'r', status: 'ready', documentCount: 1, generation: 2, omittedCount: 0, indexedAtMs: 1 };
    api.knowledgeStatus.mockResolvedValue({ ...binding, sources: [source] });
    api.searchKnowledge.mockResolvedValue({ ...binding, sources: [source], hits: [hit] });
    api.refreshKnowledge.mockImplementationOnce(() => new Promise((done, fail) => { resolve = done; reject = fail; }));
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, vi.fn()));
    await waitFor(() => expect(result.current.status?.sources[0].generation).toBe(2));
    let refreshing!: Promise<void>; act(() => { refreshing = result.current.refresh('r'); });
    await act(() => result.current.search('evidence'));
    await act(async () => {
      if (completion === 'response') resolve({ ...binding, sources: [{ ...source, status: 'indexing' }] });
      else reject(new Error('Older refresh transport failed'));
      await refreshing;
    });
    expect(result.current.hits).toEqual([hit]);
    expect(result.current.status?.sources[0].status).toBe('ready'); expect(result.current.error).toBeNull();
    expect(result.current.refreshing).toBe(false);
  });

  it.each(['scope change', 'unmount'])('keeps a slow refresh usable through polling and aborts every request on %s', async (cleanup) => {
    vi.useFakeTimers();
    const api = client();
    const source = { resourceId: 'r', status: 'indexing', documentCount: 1, generation: 1, omittedCount: 0, indexedAtMs: null };
    const refreshed = { ...binding, sources: [{ ...source, status: 'ready', generation: 2 }] };
    const pending: { signal: AbortSignal; finish: (value: unknown) => void }[] = [];
    const hold = (signal: AbortSignal) => new Promise((finish) => pending.push({ signal, finish }));
    api.knowledgeStatus.mockResolvedValueOnce({ ...binding, sources: [source] }).mockResolvedValueOnce(refreshed)
      .mockImplementation((_id, _agent, options) => hold(options.signal));
    api.refreshKnowledge.mockImplementation((_id, _input, options) => hold(options.signal));
    const { result, rerender, unmount } = renderHook(({ selected }) => useProjectKnowledge('a', selected, true, api as unknown as ProjectsClient, vi.fn()),
      { initialProps: { selected: project as Project | null } });
    await act(async () => { await Promise.resolve(); });
    let firstRefresh!: Promise<void>; act(() => { firstRefresh = result.current.refresh('r'); });
    expect(result.current.refreshing).toBe(true);
    await act(async () => { await vi.advanceTimersByTimeAsync(KNOWLEDGE_POLL_MS); });
    expect(pending[0].signal.aborted).toBe(false);
    expect(result.current.status?.sources[0].status).toBe('ready');
    await act(async () => { pending[0].finish({ ...binding, sources: [source] }); await firstRefresh; });
    expect(result.current.refreshing).toBe(false); expect(result.current.status?.sources[0].status).toBe('ready');

    api.refreshKnowledge.mockResolvedValueOnce({ ...binding, sources: [{ ...source, generation: 2 }] });
    await act(() => result.current.refresh('r'));
    api.searchKnowledge.mockImplementation((_id, _agent, options) => hold(options.signal));
    api.openKnowledgeCitation.mockImplementation((_id, _citation, _agent, options) => hold(options.signal));
    const inFlight: Promise<void>[] = [];
    act(() => { inFlight.push(result.current.refresh('r'), result.current.search('evidence'), result.current.openCitation('c')); });
    await act(async () => { await vi.advanceTimersByTimeAsync(KNOWLEDGE_POLL_MS); });
    const active = pending.slice(1);
    expect(active).toHaveLength(4); expect(active.every((request) => !request.signal.aborted)).toBe(true);
    const expectedState = cleanup === 'scope change'
      ? { status: null, hits: [], citation: null, refreshing: false }
      : { status: result.current.status, hits: result.current.hits, citation: result.current.citation, refreshing: result.current.refreshing };
    if (cleanup === 'scope change') rerender({ selected: null }); else unmount();
    expect(active.every((request) => request.signal.aborted)).toBe(true);
    await act(async () => {
      for (const request of active) request.finish({ ...refreshed, ...hit, hits: [hit], text: 'Late passage' });
      await Promise.all(inFlight);
    });
    expect({ status: result.current.status, hits: result.current.hits, citation: result.current.citation,
      refreshing: result.current.refreshing }).toEqual(expectedState);
  });

  it('fences an older ready poll before accepting a refresh indexing acknowledgement and continuing to poll', async () => {
    vi.useFakeTimers();
    const api = client();
    const source = { resourceId: 'r', status: 'ready', documentCount: 1, generation: 1, omittedCount: 0, indexedAtMs: 1 };
    let oldStatus!: (value: unknown) => void; let acknowledge!: (value: unknown) => void;
    api.knowledgeStatus.mockImplementationOnce(() => new Promise((done) => { oldStatus = done; }))
      .mockResolvedValue({ ...binding, sources: [{ ...source, generation: 2 }] });
    api.refreshKnowledge.mockImplementationOnce(() => new Promise((done) => { acknowledge = done; }));
    const { result } = renderHook(() => useProjectKnowledge('a', project, true, api as unknown as ProjectsClient, vi.fn()));
    let refreshing!: Promise<void>; act(() => { refreshing = result.current.refresh('r'); });
    await act(async () => oldStatus({ ...binding, sources: [source] }));
    await act(async () => { acknowledge({ ...binding, sources: [{ ...source, status: 'indexing' }] }); await refreshing; });
    expect(result.current.refreshing).toBe(false);
    expect(result.current.status?.sources[0].status).toBe('indexing');
    await act(async () => { await vi.advanceTimersByTimeAsync(KNOWLEDGE_POLL_MS); });
    expect(result.current.status?.sources[0]).toMatchObject({ generation: 2, status: 'ready' });
    expect(api.knowledgeStatus).toHaveBeenCalledTimes(2);
  });
});
