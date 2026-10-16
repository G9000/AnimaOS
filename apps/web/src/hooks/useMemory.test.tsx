import { act, renderHook, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { DaemonHttpError } from '@animaOS-SWARM/sdk';

import { COMPANION_UNREACHABLE } from '../lib/approvals';
import { daemon } from '../lib/daemon-api';
import { ENTITY_GONE, FACT_GONE, MEMORY_GONE } from '../lib/memory';
import {
  entityFixture,
  factFixture,
  memoryFixture,
  relationshipFixture,
} from '../test/memory';
import { useMemory } from './useMemory';

const deleted = {
  id: 'm1',
  removedRelationships: 0,
  updatedRelationships: 0,
  updatedFacts: 0,
};

beforeEach(() => {
  vi.spyOn(daemon, 'recentMemories').mockResolvedValue([memoryFixture('m1')]);
  vi.spyOn(daemon, 'searchMemories').mockResolvedValue([
    { ...memoryFixture('m2'), score: 0.8 },
  ]);
  vi.spyOn(daemon, 'listFacts').mockResolvedValue([factFixture('f1')]);
  vi.spyOn(daemon, 'listMemoryEntities').mockResolvedValue([
    entityFixture('e1'),
  ]);
  vi.spyOn(daemon, 'listMemoryRelationships').mockResolvedValue([
    relationshipFixture('r1'),
  ]);
  vi.spyOn(daemon, 'updateMemory').mockResolvedValue(memoryFixture('m1'));
  vi.spyOn(daemon, 'deleteMemory').mockResolvedValue(deleted);
  vi.spyOn(daemon, 'replaceFact').mockResolvedValue({
    fact: factFixture('f2'),
    superseded: factFixture('f1', { status: 'superseded' }),
  });
  vi.spyOn(daemon, 'deleteFact').mockResolvedValue({ id: 'f1' });
  vi.spyOn(daemon, 'deleteMemoryEntity').mockResolvedValue({
    kind: 'external',
    id: 'e1',
    removedRelationships: 0,
    removedFacts: 0,
  });
  vi.spyOn(daemon, 'traceMemory').mockResolvedValue({
    memory: memoryFixture('m1'),
    relationships: [],
    entities: [],
  });
});

afterEach(() => {
  vi.restoreAllMocks();
});

const online = { agentId: 'agent-main', epoch: 1, enabled: true };

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

function readCount() {
  return [
    daemon.recentMemories,
    daemon.searchMemories,
    daemon.listFacts,
    daemon.listMemoryEntities,
    daemon.listMemoryRelationships,
  ].reduce((total, method) => total + vi.mocked(method).mock.calls.length, 0);
}

async function loaded(options = online) {
  const hook = renderHook((props) => useMemory(props), {
    initialProps: options,
  });
  await waitFor(() => expect(hook.result.current.loaded).toBe(true));
  return hook;
}

describe('useMemory', () => {
  it('loads the recent memories, facts, entities, and relationships for the companion', async () => {
    const { result } = await loaded();
    expect(result.current.memories.map((m) => m.id)).toEqual(['m1']);
    expect(result.current.facts.map((f) => f.id)).toEqual(['f1']);
    expect(result.current.entities.map((e) => e.id)).toEqual(['e1']);
    expect(result.current.relationships.map((r) => r.id)).toEqual(['r1']);
    expect(result.current.error).toBeNull();
    expect(daemon.recentMemories).toHaveBeenCalledWith('agent-main', 200);
    // Facts belong to the owner: they are not read for one agent.
    expect(daemon.listFacts).toHaveBeenCalledWith({
      includeInactive: false,
      limit: 500,
    });
    expect(daemon.listMemoryEntities).toHaveBeenCalledWith(200);
    expect(daemon.listMemoryRelationships).toHaveBeenCalledWith(
      'agent-main',
      200,
    );
  });

  it('search reads the matching memories and an empty search returns to the recent list', async () => {
    const { result } = await loaded();

    act(() => result.current.search('tea'));
    await waitFor(() =>
      expect(result.current.memories.map((m) => m.id)).toEqual(['m2']),
    );
    expect(result.current.query).toBe('tea');
    expect(daemon.searchMemories).toHaveBeenCalledWith(
      'tea',
      'agent-main',
      200,
    );

    act(() => result.current.search(''));
    await waitFor(() =>
      expect(result.current.memories.map((m) => m.id)).toEqual(['m1']),
    );
  });

  it('includeReplaced reads the facts again with includeInactive', async () => {
    const { result } = await loaded();

    act(() => result.current.setIncludeReplaced(true));

    await waitFor(() =>
      expect(daemon.listFacts).toHaveBeenLastCalledWith({
        includeInactive: true,
        limit: 500,
      }),
    );
    expect(result.current.includeReplaced).toBe(true);
  });

  it('a stale read is ignored', async () => {
    let releaseSlow: (value: ReturnType<typeof memoryFixture>[]) => void = () =>
      undefined;
    vi.mocked(daemon.recentMemories).mockImplementationOnce(
      () => new Promise((resolve) => (releaseSlow = resolve)),
    );
    const hook = renderHook((props) => useMemory(props), {
      initialProps: online,
    });
    await waitFor(() => expect(daemon.recentMemories).toHaveBeenCalledTimes(1));
    vi.mocked(daemon.recentMemories).mockResolvedValue([
      memoryFixture('fresh'),
    ]);

    hook.rerender({ ...online, epoch: 2 });
    await waitFor(() =>
      expect(hook.result.current.memories.map((m) => m.id)).toEqual(['fresh']),
    );
    await act(async () => {
      releaseSlow([memoryFixture('stale')]);
    });

    expect(hook.result.current.memories.map((m) => m.id)).toEqual(['fresh']);
  });

  it('StrictMode effect replay starts a live batch and ignores the first batch', async () => {
    const firstBatch = deferred<ReturnType<typeof memoryFixture>[]>();
    vi.mocked(daemon.recentMemories)
      .mockReturnValueOnce(firstBatch.promise)
      .mockResolvedValueOnce([memoryFixture('strict-fresh')]);
    const hook = renderHook((props) => useMemory(props), {
      initialProps: online,
      reactStrictMode: true,
    });

    await waitFor(() => expect(daemon.recentMemories).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(hook.result.current.memories.map((memory) => memory.id)).toEqual([
        'strict-fresh',
      ]),
    );
    await act(async () => {
      firstBatch.resolve([memoryFixture('strict-stale')]);
      await firstBatch.promise;
    });

    expect(hook.result.current.loaded).toBe(true);
    expect(hook.result.current.memories.map((memory) => memory.id)).toEqual([
      'strict-fresh',
    ]);
  });

  it('a failed list keeps the others and sets the error', async () => {
    const { result } = await loaded();
    vi.mocked(daemon.listFacts).mockRejectedValue(
      new DaemonHttpError(503, { error: 'Memory is busy' }),
    );
    vi.mocked(daemon.recentMemories).mockResolvedValue([memoryFixture('m9')]);

    act(() => result.current.refresh());

    await waitFor(() => expect(result.current.error).toBe('Memory is busy'));
    expect(result.current.memories.map((m) => m.id)).toEqual(['m9']);
    expect(result.current.facts.map((f) => f.id)).toEqual(['f1']);
  });

  it('an empty reload keeps the empty arrays', async () => {
    vi.mocked(daemon.recentMemories).mockResolvedValue([]);
    vi.mocked(daemon.listFacts).mockResolvedValue([]);
    vi.mocked(daemon.listMemoryEntities).mockResolvedValue([]);
    vi.mocked(daemon.listMemoryRelationships).mockResolvedValue([]);
    const { result } = await loaded();
    const before = {
      memories: result.current.memories,
      facts: result.current.facts,
      entities: result.current.entities,
      relationships: result.current.relationships,
    };

    act(() => result.current.refresh());
    await waitFor(() => expect(daemon.recentMemories).toHaveBeenCalledTimes(2));
    await act(async () => undefined);

    expect(result.current.memories).toBe(before.memories);
    expect(result.current.facts).toBe(before.facts);
    expect(result.current.entities).toBe(before.entities);
    expect(result.current.relationships).toBe(before.relationships);
  });

  it('edit sends the patch and reloads', async () => {
    const { result } = await loaded();
    const memory = memoryFixture('m1');

    let ok = false;
    await act(async () => {
      ok = await result.current.edit(memory, { content: 'New text' });
    });

    expect(ok).toBe(true);
    expect(daemon.updateMemory).toHaveBeenCalledWith('m1', {
      content: 'New text',
    });
    expect(daemon.recentMemories).toHaveBeenCalledTimes(2);
  });

  it.each([
    ['search', 'success', null],
    ['search', 'conflict', new DaemonHttpError(409, { error: 'review edit' })],
    ['includeReplaced', 'success', null],
    [
      'includeReplaced',
      'conflict',
      new DaemonHttpError(409, { error: 'review edit' }),
    ],
  ] as const)(
    'a deferred edit %s reloads the current view after %s',
    async (viewChange, _outcome, failure) => {
      const hook = await loaded();
      const mutation = deferred<ReturnType<typeof memoryFixture>>();
      vi.mocked(daemon.updateMemory).mockReturnValueOnce(mutation.promise);
      let pending!: Promise<boolean>;
      act(() => {
        pending = hook.result.current.edit(memoryFixture('m1'), {
          content: 'submitted once',
        });
      });

      if (viewChange === 'search') {
        vi.mocked(daemon.searchMemories)
          .mockResolvedValueOnce([memoryFixture('filtered-first')])
          .mockResolvedValueOnce([memoryFixture('filtered-after-edit')]);
        act(() => hook.result.current.search('tea'));
        await waitFor(() =>
          expect(
            hook.result.current.memories.map((memory) => memory.id),
          ).toEqual(['filtered-first']),
        );
      } else {
        vi.mocked(daemon.listFacts)
          .mockResolvedValueOnce([
            factFixture('replacement-first', { status: 'superseded' }),
          ])
          .mockResolvedValueOnce([
            factFixture('replacement-after-edit', { status: 'superseded' }),
          ]);
        act(() => hook.result.current.setIncludeReplaced(true));
        await waitFor(() =>
          expect(hook.result.current.facts.map((fact) => fact.id)).toEqual([
            'replacement-first',
          ]),
        );
      }

      await act(async () => {
        if (failure) mutation.reject(failure);
        else mutation.resolve(memoryFixture('m1'));
        await mutation.promise.catch(() => undefined);
      });
      await expect(pending).resolves.toBe(!failure);

      if (viewChange === 'search') {
        await waitFor(() =>
          expect(
            hook.result.current.memories.map((memory) => memory.id),
          ).toEqual(['filtered-after-edit']),
        );
        expect(daemon.searchMemories).toHaveBeenLastCalledWith(
          'tea',
          'agent-main',
          200,
        );
        expect(daemon.recentMemories).toHaveBeenCalledTimes(1);
      } else {
        await waitFor(() =>
          expect(hook.result.current.facts.map((fact) => fact.id)).toEqual([
            'replacement-after-edit',
          ]),
        );
        expect(daemon.listFacts).toHaveBeenLastCalledWith({
          includeInactive: true,
          limit: 500,
        });
      }
      expect(daemon.updateMemory).toHaveBeenCalledTimes(1);
      if (failure) {
        expect(hook.result.current.error).toBe('review edit');
      }
    },
  );

  it('a 404 on remove says the memory is gone and reloads', async () => {
    const { result } = await loaded();
    vi.mocked(daemon.deleteMemory).mockRejectedValue(
      new DaemonHttpError(404, { error: 'memory not found' }),
    );

    let ok = true;
    await act(async () => {
      ok = await result.current.remove(memoryFixture('m1'));
    });

    expect(ok).toBe(false);
    expect(result.current.error).toBe(MEMORY_GONE);
    expect(result.current.errorStatus).toBe(404);
    expect(daemon.recentMemories).toHaveBeenCalledTimes(2);
  });

  it('a stale memory deletion sends reviewed text and reloads on conflict', async () => {
    const { result } = await loaded();
    const reviewed = memoryFixture('m1', { content: 'Reviewed wording 🫖' });
    vi.mocked(daemon.deleteMemory).mockRejectedValue(
      new DaemonHttpError(409, {
        error: 'Memory text changed; review it again',
      }),
    );

    let ok = true;
    await act(async () => {
      ok = await result.current.remove(reviewed);
    });

    expect(ok).toBe(false);
    expect(daemon.deleteMemory).toHaveBeenCalledWith(
      'm1',
      'Reviewed wording 🫖',
    );
    expect(result.current.error).toBe('Memory text changed; review it again');
    expect(result.current.errorStatus).toBe(409);
    expect(daemon.recentMemories).toHaveBeenCalledTimes(2);
  });

  it('remove, replaceFact, removeFact, and removeEntity call the daemon and reload', async () => {
    const { result } = await loaded();
    const reads = () => vi.mocked(daemon.recentMemories).mock.calls.length;

    await act(async () => {
      expect(await result.current.remove(memoryFixture('m1'))).toBe(true);
    });
    expect(daemon.deleteMemory).toHaveBeenCalledWith('m1', 'Memory m1');
    expect(reads()).toBe(2);

    await act(async () => {
      expect(
        await result.current.replaceFact(factFixture('f1'), 'Long answers'),
      ).toBe(true);
    });
    expect(daemon.replaceFact).toHaveBeenCalledWith('f1', 'Long answers');
    expect(reads()).toBe(3);

    await act(async () => {
      expect(await result.current.removeFact(factFixture('f1'))).toBe(true);
    });
    expect(daemon.deleteFact).toHaveBeenCalledWith('f1');
    expect(reads()).toBe(4);

    await act(async () => {
      expect(
        await result.current.removeEntity(
          entityFixture('e1', { kind: 'user' }),
        ),
      ).toBe(true);
    });
    expect(daemon.deleteMemoryEntity).toHaveBeenCalledWith('user', 'e1');
    expect(reads()).toBe(5);
    expect(result.current.error).toBeNull();
  });

  it('answers a 404 on a fact or an entity with its own text and a 409 with the daemon text', async () => {
    const { result } = await loaded();
    vi.mocked(daemon.deleteFact).mockRejectedValue(
      new DaemonHttpError(404, { error: 'fact not found' }),
    );
    await act(async () => {
      await result.current.removeFact(factFixture('f1'));
    });
    expect(result.current.error).toBe(FACT_GONE);

    vi.mocked(daemon.deleteMemoryEntity).mockRejectedValue(
      new DaemonHttpError(404, { error: 'entity not found' }),
    );
    await act(async () => {
      await result.current.removeEntity(entityFixture('e1'));
    });
    expect(result.current.error).toBe(ENTITY_GONE);

    vi.mocked(daemon.deleteMemoryEntity).mockRejectedValue(
      new DaemonHttpError(409, {
        error: 'This entity still has memories; delete them first',
      }),
    );
    await act(async () => {
      await result.current.removeEntity(entityFixture('e1'));
    });
    expect(result.current.error).toBe(
      'This entity still has memories; delete them first',
    );
    expect(result.current.errorStatus).toBe(409);
  });

  it('a network failure says the companion is unreachable', async () => {
    const { result } = await loaded();
    vi.mocked(daemon.updateMemory).mockRejectedValue(new TypeError('offline'));

    await act(async () => {
      expect(await result.current.edit(memoryFixture('m1'), {})).toBe(false);
    });

    expect(result.current.error).toBe(COMPANION_UNREACHABLE);
    expect(result.current.errorStatus).toBeNull();
  });

  it('trace returns the evidence or null', async () => {
    const { result } = await loaded();

    let trace: Awaited<ReturnType<typeof result.current.trace>> = null;
    await act(async () => {
      trace = await result.current.trace(memoryFixture('m1'));
    });
    expect(trace).toEqual(expect.objectContaining({ relationships: [] }));
    expect(daemon.recentMemories).toHaveBeenCalledTimes(1);

    vi.mocked(daemon.traceMemory).mockRejectedValue(
      new DaemonHttpError(404, { error: 'memory not found' }),
    );
    await act(async () => {
      trace = await result.current.trace(memoryFixture('m1'));
    });
    expect(trace).toBeNull();
    expect(result.current.error).toBe(MEMORY_GONE);
    // A memory that is gone is dropped from the lists.
    expect(daemon.recentMemories).toHaveBeenCalledTimes(2);
  });

  it('a refused action’s message goes when the owner reads again', async () => {
    const refusal = new DaemonHttpError(409, { error: 'Still has memories' });
    const { result } = await loaded();

    for (const read of [
      () => result.current.refresh(),
      () => result.current.search('tea'),
      () => result.current.setIncludeReplaced(true),
    ]) {
      vi.mocked(daemon.deleteMemoryEntity).mockRejectedValue(refusal);
      await act(async () => {
        await result.current.removeEntity(entityFixture('e1'));
      });
      expect(result.current.error).toBe('Still has memories');
      expect(result.current.errorStatus).toBe(409);

      act(read);
      await waitFor(() => expect(result.current.error).toBeNull());
      expect(result.current.errorStatus).toBeNull();
    }
  });

  it('nothing is read while offline or without an agent', async () => {
    const hook = renderHook((props) => useMemory(props), {
      initialProps: { agentId: 'agent-main', epoch: 1, enabled: false },
    });
    hook.rerender({ agentId: null, epoch: 1, enabled: true });
    await act(async () => undefined);

    expect(daemon.recentMemories).not.toHaveBeenCalled();
    expect(daemon.listFacts).not.toHaveBeenCalled();
    expect(hook.result.current.loaded).toBe(false);
  });

  it('offline refresh and search do not issue reads, including from an initially offline hook', async () => {
    const hook = renderHook((props) => useMemory(props), {
      initialProps: { agentId: 'agent-main', epoch: 1, enabled: false },
    });

    act(() => {
      hook.result.current.refresh();
      hook.result.current.search('offline query');
    });

    expect(readCount()).toBe(0);
    expect(hook.result.current.loaded).toBe(false);
    expect(hook.result.current.memories).toEqual([]);
  });

  it.each([
    ['success', null],
    ['conflict', new DaemonHttpError(409, { error: 'changed elsewhere' })],
  ] as const)(
    'a submitted edit settling with %s after disconnect is not acknowledged and causes no read',
    async (_label, failure) => {
      const { result, rerender } = await loaded();
      const mutation = deferred<ReturnType<typeof memoryFixture>>();
      vi.mocked(daemon.updateMemory).mockReturnValueOnce(mutation.promise);
      let pending!: Promise<boolean>;
      act(() => {
        pending = result.current.edit(memoryFixture('m1'), {
          content: 'submitted once',
        });
      });
      expect(daemon.updateMemory).toHaveBeenCalledTimes(1);
      const readsBeforeDisconnect = readCount();

      rerender({ ...online, enabled: false });
      await act(async () => {
        if (failure) mutation.reject(failure);
        else mutation.resolve(memoryFixture('m1'));
        await mutation.promise.catch(() => undefined);
      });

      await expect(pending).resolves.toBe(false);
      expect(daemon.updateMemory).toHaveBeenCalledTimes(1);
      expect(readCount()).toBe(readsBeforeDisconnect);
      expect(result.current.error).toBeNull();
      expect(result.current.loaded).toBe(false);
      expect(result.current.memories).toEqual([]);
    },
  );

  it('a callback captured before disconnect cannot submit a stale action', async () => {
    const hook = await loaded();
    const staleEdit = hook.result.current.edit;
    hook.rerender({ ...online, enabled: false });

    await act(async () => {
      await expect(
        staleEdit(memoryFixture('m1'), { content: 'stale' }),
      ).resolves.toBe(false);
    });

    expect(daemon.updateMemory).not.toHaveBeenCalled();
    expect(readCount()).toBe(4);
  });

  it('late list success and error cannot publish across offline and same-value reconnect', async () => {
    const oldMemories = deferred<ReturnType<typeof memoryFixture>[]>();
    const oldFacts = deferred<ReturnType<typeof factFixture>[]>();
    const oldEntities = deferred<ReturnType<typeof entityFixture>[]>();
    const oldRelationships =
      deferred<ReturnType<typeof relationshipFixture>[]>();
    vi.mocked(daemon.recentMemories).mockReturnValueOnce(oldMemories.promise);
    vi.mocked(daemon.listFacts).mockReturnValueOnce(oldFacts.promise);
    vi.mocked(daemon.listMemoryEntities).mockReturnValueOnce(
      oldEntities.promise,
    );
    vi.mocked(daemon.listMemoryRelationships).mockReturnValueOnce(
      oldRelationships.promise,
    );
    const hook = renderHook((props) => useMemory(props), {
      initialProps: online,
    });
    await waitFor(() => expect(readCount()).toBe(4));

    hook.rerender({ ...online, enabled: false });
    expect(hook.result.current.loaded).toBe(false);
    expect(hook.result.current.memories).toEqual([]);
    expect(hook.result.current.facts).toEqual([]);
    expect(hook.result.current.entities).toEqual([]);
    expect(hook.result.current.relationships).toEqual([]);

    await act(async () => {
      oldMemories.resolve([memoryFixture('old')]);
      oldFacts.reject(new DaemonHttpError(503, { error: 'old read failed' }));
      oldEntities.resolve([entityFixture('old-entity')]);
      oldRelationships.resolve([relationshipFixture('old-relationship')]);
      await Promise.allSettled([
        oldMemories.promise,
        oldFacts.promise,
        oldEntities.promise,
        oldRelationships.promise,
      ]);
    });
    expect(hook.result.current.memories).toEqual([]);
    expect(hook.result.current.error).toBeNull();

    vi.mocked(daemon.recentMemories).mockResolvedValueOnce([
      memoryFixture('fresh'),
    ]);
    hook.rerender(online);
    expect(hook.result.current.loaded).toBe(false);
    expect(hook.result.current.memories).toEqual([]);
    await waitFor(() =>
      expect(hook.result.current.memories.map((memory) => memory.id)).toEqual([
        'fresh',
      ]),
    );
    expect(daemon.recentMemories).toHaveBeenCalledTimes(2);
    expect(hook.result.current.error).toBeNull();
  });

  it.each([
    ['agent', { ...online, agentId: 'agent-next' }],
    ['epoch', { ...online, epoch: 2 }],
  ] as const)(
    'a pending read cannot publish after an independent %s change',
    async (_label, nextOptions) => {
      const hook = await loaded();
      const oldRead = deferred<ReturnType<typeof memoryFixture>[]>();
      vi.mocked(daemon.recentMemories).mockReturnValueOnce(oldRead.promise);
      act(() => hook.result.current.refresh());
      await waitFor(() =>
        expect(daemon.recentMemories).toHaveBeenCalledTimes(2),
      );

      vi.mocked(daemon.recentMemories).mockResolvedValueOnce([
        memoryFixture('new-context'),
      ]);
      hook.rerender(nextOptions);
      expect(hook.result.current.loaded).toBe(false);
      expect(hook.result.current.memories).toEqual([]);
      await waitFor(() =>
        expect(hook.result.current.memories.map((memory) => memory.id)).toEqual(
          ['new-context'],
        ),
      );

      await act(async () => {
        oldRead.resolve([memoryFixture('old-context')]);
        await oldRead.promise;
      });
      expect(hook.result.current.memories.map((memory) => memory.id)).toEqual([
        'new-context',
      ]);
    },
  );

  it.each([
    ['success', null],
    ['error', new DaemonHttpError(503, { error: 'old trace failed' })],
    ['404', new DaemonHttpError(404, { error: 'memory not found' })],
  ] as const)(
    'late trace %s after disconnect cannot publish evidence, errors, or recovery reads',
    async (_label, failure) => {
      const hook = await loaded();
      const traceResult =
        deferred<Awaited<ReturnType<typeof daemon.traceMemory>>>();
      vi.mocked(daemon.traceMemory).mockReturnValueOnce(traceResult.promise);
      let pending!: Promise<
        Awaited<ReturnType<typeof hook.result.current.trace>>
      >;
      act(() => {
        pending = hook.result.current.trace(memoryFixture('m1'));
      });
      const readsBeforeDisconnect = readCount();
      hook.rerender({ ...online, enabled: false });

      await act(async () => {
        if (failure) traceResult.reject(failure);
        else
          traceResult.resolve({
            memory: memoryFixture('m1'),
            relationships: [],
            entities: [],
          });
        await traceResult.promise.catch(() => undefined);
      });

      await expect(pending).resolves.toBeNull();
      expect(readCount()).toBe(readsBeforeDisconnect);
      expect(hook.result.current.error).toBeNull();
      expect(hook.result.current.memories).toEqual([]);
    },
  );

  it.each([
    ['success', null],
    ['conflict', new DaemonHttpError(409, { error: 'changed elsewhere' })],
  ] as const)(
    'a submitted edit settling with %s after unmount causes no recovery read or stale result',
    async (_label, failure) => {
      const hook = await loaded();
      const mutation = deferred<ReturnType<typeof memoryFixture>>();
      vi.mocked(daemon.updateMemory).mockReturnValueOnce(mutation.promise);
      let pending!: Promise<boolean>;
      act(() => {
        pending = hook.result.current.edit(memoryFixture('m1'), {
          content: 'submitted once',
        });
      });
      const readsBeforeUnmount = readCount();
      hook.unmount();

      if (failure) mutation.reject(failure);
      else mutation.resolve(memoryFixture('m1'));
      await expect(pending).resolves.toBe(false);
      expect(daemon.updateMemory).toHaveBeenCalledTimes(1);
      expect(readCount()).toBe(readsBeforeUnmount);
    },
  );
});
