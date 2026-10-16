import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  AgentRelationship,
  Memory,
  MemoryEntity,
  MemoryEvidenceTrace,
  MemoryFact,
  MemoryPatch,
} from '@animaOS-SWARM/sdk';
import { MAX_FACTS_SHOWN } from '@animaOS-SWARM/sdk';

import { daemon } from '../lib/daemon-api';
import {
  ENTITY_GONE,
  FACT_GONE,
  MEMORY_GONE,
  MEMORY_PAGE_LIMIT,
  memoryErrorMessage,
  type MemoryEntry,
} from '../lib/memory';

export interface MemoryOptions {
  /** The companion whose memory is shown; null until it is known. */
  agentId: string | null;
  /** `LiveState.epoch`: bumped by every snapshot and resync. */
  epoch: number;
  /** False while the daemon is offline: nothing is read. */
  enabled: boolean;
}

export interface MemoryView {
  memories: MemoryEntry[];
  facts: MemoryFact[];
  entities: MemoryEntity[];
  relationships: AgentRelationship[];
  loaded: boolean;
  error: string | null;
  /** The HTTP status of the failed action, when the daemon refused it. */
  errorStatus: number | null;
  query: string;
  includeReplaced: boolean;
  /** Reads the list again with this search ('' is the recent list). */
  search: (query: string) => void;
  setIncludeReplaced: (value: boolean) => void;
  refresh: () => void;
  /** Each answers true when the daemon took it, after the lists were read
   *  again. */
  edit: (memory: Memory, patch: MemoryPatch) => Promise<boolean>;
  remove: (memory: Memory) => Promise<boolean>;
  trace: (memory: Memory) => Promise<MemoryEvidenceTrace | null>;
  replaceFact: (fact: MemoryFact, value: string) => Promise<boolean>;
  removeFact: (fact: MemoryFact) => Promise<boolean>;
  removeEntity: (entity: MemoryEntity) => Promise<boolean>;
}

interface MemoryContext extends MemoryOptions {
  generation: number;
}

interface MemorySnapshot {
  context: MemoryContext;
  memories: MemoryEntry[];
  facts: MemoryFact[];
  entities: MemoryEntity[];
  relationships: AgentRelationship[];
  loaded: boolean;
  readError: string | null;
  actionError: string | null;
  actionStatus: number | null;
}

const EMPTY_MEMORIES: MemoryEntry[] = [];
const EMPTY_FACTS: MemoryFact[] = [];
const EMPTY_ENTITIES: MemoryEntity[] = [];
const EMPTY_RELATIONSHIPS: AgentRelationship[] = [];

/** An empty list that is empty again keeps its array, so a harness that goes
 *  online with nothing remembered does not render again (the M5 I4 lesson). */
function keepEmpty<T>(previous: T[], next: T[]): T[] {
  return previous.length === 0 && next.length === 0 ? previous : next;
}

function sameInputs(context: MemoryContext, options: MemoryOptions): boolean {
  return (
    context.agentId === options.agentId &&
    context.enabled === options.enabled &&
    context.epoch === options.epoch
  );
}

function emptySnapshot(context: MemoryContext): MemorySnapshot {
  return {
    context,
    memories: EMPTY_MEMORIES,
    facts: EMPTY_FACTS,
    entities: EMPTY_ENTITIES,
    relationships: EMPTY_RELATIONSHIPS,
    loaded: false,
    readError: null,
    actionError: null,
    actionStatus: null,
  };
}

/** Memory reads and actions belong to one mounted owner/online/epoch context. */
export function useMemory({
  agentId,
  epoch,
  enabled,
}: MemoryOptions): MemoryView {
  const sequence = useRef(0);
  const mounted = useRef(true);
  const contextRef = useRef<MemoryContext | null>(null);
  const inputs = { agentId, epoch, enabled };

  if (!contextRef.current || !sameInputs(contextRef.current, inputs)) {
    const generation = (contextRef.current?.generation ?? 0) + 1;
    contextRef.current = { ...inputs, generation };
    // Invalidate reads as soon as a new context is rendered. The visible view
    // is projected from the context-bound snapshot below, before effects run.
    sequence.current += 1;
  }

  const context = contextRef.current!;
  const isCurrent = useCallback(
    (origin: MemoryContext) =>
      mounted.current &&
      contextRef.current === origin &&
      origin.enabled &&
      Boolean(origin.agentId),
    [],
  );

  const [snapshot, setSnapshot] = useState<MemorySnapshot>(() =>
    emptySnapshot(context),
  );
  const [query, setQuery] = useState('');
  const [includeReplaced, setIncludeReplaced] = useState(false);
  const queryRef = useRef(query);
  queryRef.current = query;
  const includeReplacedRef = useRef(includeReplaced);
  includeReplacedRef.current = includeReplaced;

  const currentSnapshot =
    snapshot.context === context ? snapshot : emptySnapshot(context);

  const load = useCallback(async () => {
    if (!isCurrent(context) || !context.agentId) return;

    // Freeze the current filters at dispatch. A mutation completion calls
    // this only after its owner context is revalidated, so it reloads the
    // latest same-owner view without following an obsolete closure's query.
    const requestQuery = queryRef.current;
    const requestIncludeReplaced = includeReplacedRef.current;
    const requestSequence = ++sequence.current;
    const results = await Promise.allSettled([
      requestQuery
        ? daemon.searchMemories(
            requestQuery,
            context.agentId,
            MEMORY_PAGE_LIMIT,
          )
        : daemon.recentMemories(context.agentId, MEMORY_PAGE_LIMIT),
      // Facts belong to the owner, not to one agent: a fact whose evidence
      // memories were deleted, or that only a helper's memories evidence,
      // is still in use.
      daemon.listFacts({
        includeInactive: requestIncludeReplaced,
        limit: MAX_FACTS_SHOWN,
      }),
      daemon.listMemoryEntities(MEMORY_PAGE_LIMIT),
      daemon.listMemoryRelationships(context.agentId, MEMORY_PAGE_LIMIT),
    ]);

    if (!isCurrent(context) || requestSequence !== sequence.current) {
      return;
    }

    const [memoriesResult, factsResult, entitiesResult, relationshipsResult] =
      results;
    setSnapshot((previous) => {
      if (!isCurrent(context) || requestSequence !== sequence.current) {
        return previous;
      }
      const base =
        previous.context === context ? previous : emptySnapshot(context);
      const failed = results.find((result) => result.status === 'rejected');
      return {
        ...base,
        memories:
          memoriesResult.status === 'fulfilled'
            ? keepEmpty(base.memories, memoriesResult.value)
            : base.memories,
        facts:
          factsResult.status === 'fulfilled'
            ? keepEmpty(base.facts, factsResult.value)
            : base.facts,
        entities:
          entitiesResult.status === 'fulfilled'
            ? keepEmpty(base.entities, entitiesResult.value)
            : base.entities,
        relationships:
          relationshipsResult.status === 'fulfilled'
            ? keepEmpty(base.relationships, relationshipsResult.value)
            : base.relationships,
        loaded: true,
        readError: failed
          ? memoryErrorMessage((failed as PromiseRejectedResult).reason).message
          : null,
      };
    });
  }, [context, isCurrent]);

  // Mount validity must be established before the read effect. React Strict
  // Mode replays effects after cleanup; the replayed load must see mounted=true.
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      sequence.current += 1;
    };
  }, []);

  useEffect(() => {
    if (!context.enabled || !context.agentId) return;
    void load();
  }, [context, includeReplaced, load, query]);

  const clearActionError = useCallback(() => {
    if (!isCurrent(context)) return;
    setSnapshot((previous) => {
      if (!isCurrent(context)) return previous;
      const base =
        previous.context === context ? previous : emptySnapshot(context);
      return { ...base, actionError: null, actionStatus: null };
    });
  }, [context, isCurrent]);

  const search = useCallback(
    (next: string) => {
      if (!isCurrent(context)) return;
      clearActionError();
      setQuery(next);
    },
    [clearActionError, context, isCurrent],
  );

  const setReplaced = useCallback(
    (value: boolean) => {
      if (!isCurrent(context)) return;
      clearActionError();
      setIncludeReplaced(value);
    },
    [clearActionError, context, isCurrent],
  );

  const refresh = useCallback(() => {
    if (!isCurrent(context)) return;
    clearActionError();
    void load();
  }, [clearActionError, context, isCurrent, load]);

  const act = useCallback(
    async (work: () => Promise<unknown>, gone: string) => {
      // A saved callback from a prior render cannot dispatch an action into a
      // new owner context. Once sent, however, the original mutation settles
      // exactly once and is never redirected or retried.
      if (!isCurrent(context)) return false;
      clearActionError();
      try {
        await work();
      } catch (caught) {
        if (!isCurrent(context)) return false;
        const { message, status } = memoryErrorMessage(caught);
        setSnapshot((previous) => {
          if (!isCurrent(context)) return previous;
          const base =
            previous.context === context ? previous : emptySnapshot(context);
          return {
            ...base,
            actionError: status === 404 ? gone : message,
            actionStatus: status,
          };
        });
        if ((status === 404 || status === 409) && isCurrent(context)) {
          await load();
        }
        return false;
      }

      if (!isCurrent(context)) return false;
      await load();
      return isCurrent(context);
    },
    [clearActionError, context, isCurrent, load],
  );

  const edit = useCallback(
    (memory: Memory, patch: MemoryPatch) =>
      act(() => daemon.updateMemory(memory.id, patch), MEMORY_GONE),
    [act],
  );
  const remove = useCallback(
    (memory: Memory) =>
      act(() => daemon.deleteMemory(memory.id, memory.content), MEMORY_GONE),
    [act],
  );
  const replaceFact = useCallback(
    (fact: MemoryFact, value: string) =>
      act(() => daemon.replaceFact(fact.id, value), FACT_GONE),
    [act],
  );
  const removeFact = useCallback(
    (fact: MemoryFact) => act(() => daemon.deleteFact(fact.id), FACT_GONE),
    [act],
  );
  const removeEntity = useCallback(
    (entity: MemoryEntity) =>
      act(() => daemon.deleteMemoryEntity(entity.kind, entity.id), ENTITY_GONE),
    [act],
  );

  const trace = useCallback(
    async (memory: Memory): Promise<MemoryEvidenceTrace | null> => {
      if (!isCurrent(context)) return null;
      clearActionError();
      try {
        const evidence = await daemon.traceMemory(memory.id);
        return isCurrent(context) ? evidence : null;
      } catch (caught) {
        if (!isCurrent(context)) return null;
        const { message, status } = memoryErrorMessage(caught);
        setSnapshot((previous) => {
          if (!isCurrent(context)) return previous;
          const base =
            previous.context === context ? previous : emptySnapshot(context);
          return {
            ...base,
            actionError: status === 404 ? MEMORY_GONE : message,
            actionStatus: status,
          };
        });
        if (status === 404 && isCurrent(context)) await load();
        return null;
      }
    },
    [clearActionError, context, isCurrent, load],
  );

  return {
    memories:
      currentSnapshot.context === context
        ? currentSnapshot.memories
        : EMPTY_MEMORIES,
    facts: currentSnapshot.facts,
    entities: currentSnapshot.entities,
    relationships: currentSnapshot.relationships,
    loaded: currentSnapshot.loaded,
    error: currentSnapshot.actionError ?? currentSnapshot.readError,
    errorStatus: currentSnapshot.actionError
      ? currentSnapshot.actionStatus
      : null,
    query,
    includeReplaced,
    search,
    setIncludeReplaced: setReplaced,
    refresh,
    edit,
    remove,
    trace,
    replaceFact,
    removeFact,
    removeEntity,
  };
}
