import { Text } from 'ink';
import { useLayoutEffect } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import type {
  AgentEvent,
  AgentSummary,
  Run,
  Session,
  SessionMessage,
} from '@animaos/sdk';
import type { TuiSessionApi } from '../session-api.js';
import { cleanupInk, flushInk, renderInk } from '../test-harness.js';
import {
  useSessionWorkspace,
  type UseSessionWorkspaceResult,
} from './use-session-workspace.js';

afterEach(() => {
  cleanupInk();
  vi.clearAllMocks();
  vi.useRealTimers();
});

const agentA = createAgent('agent-a');
const agentB = createAgent('agent-b');
const sessionA = createSession('session-a', 'agent-a');
const sessionB = createSession('session-b', 'agent-a');

function createAgent(id: string): AgentSummary {
  return {
    state: {
      id,
      name: id,
      status: 'idle',
      config: {} as AgentSummary['state']['config'],
      createdAtMs: 1,
      tokenUsage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
    },
    messageCount: 0,
    eventCount: 0,
    lastTask: null,
  };
}

function createSession(
  id: string,
  agentId: string,
  overrides: Partial<Session> = {},
): Session {
  return {
    id,
    agentId,
    roomId: id,
    kind: 'chat',
    origin: 'web',
    title: id,
    titleSource: 'owner',
    createdAtMs: 1,
    lastActivityAtMs: 1,
    lastReadAtMs: null,
    archived: false,
    parentSessionId: null,
    parentRunId: null,
    parentAgentId: null,
    summary: null,
    contextTrimmed: null,
    compactionError: null,
    messageCount: 0,
    preview: null,
    activeRuns: 0,
    pendingApprovals: 0,
    unread: false,
    capabilities: {
      send: true,
      steer: true,
      stop: true,
      rename: true,
      archive: true,
      delete: true,
      compact: true,
      export: true,
    },
    ...overrides,
  };
}

function createMessage(
  id: string,
  text: string,
  createdAtMs = 1,
): SessionMessage {
  return {
    id,
    role: 'assistant',
    text,
    attachments: [],
    metadata: {},
    createdAtMs,
  };
}

function createRun(
  id: string,
  sessionId: string,
  overrides: Partial<Run> = {},
): Run {
  return {
    id,
    agentId: 'agent-a',
    sessionId,
    source: 'web',
    sourceRef: null,
    idempotencyKey: null,
    status: 'running',
    input: { text: 'hello', attachmentIds: [], skill: null },
    createdAtMs: 1,
    startedAtMs: 1,
    finishedAtMs: null,
    error: null,
    stop: null,
    toolsStarted: [],
    steps: [],
    usage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
    model: 'test-model',
    provider: 'test-provider',
    parentRunId: null,
    replyMessageId: null,
    ...overrides,
  };
}

async function settle() {
  await flushInk();
  await flushInk();
}

async function flushMicrotasks() {
  for (let index = 0; index < 10; index += 1) await Promise.resolve();
}

async function waitForCondition(condition: () => boolean, description: string) {
  const deadline = Date.now() + 5_000;
  while (!condition()) {
    if (Date.now() >= deadline) {
      throw new Error(`Timed out waiting for ${description}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 10));
    await settle();
  }
}

function neverStream(signal: AbortSignal): AsyncIterable<AgentEvent> {
  return {
    async *[Symbol.asyncIterator]() {
      if (signal.aborted) return;
      await new Promise<void>((resolve) => {
        signal.addEventListener('abort', () => resolve(), { once: true });
      });
      yield* [];
    },
  };
}

function createApi(overrides: Partial<TuiSessionApi> = {}): TuiSessionApi {
  return {
    listAgents: vi.fn().mockResolvedValue([agentA]),
    listSessions: vi
      .fn()
      .mockResolvedValue({ sessions: [sessionA], nextCursor: null }),
    createSession: vi.fn().mockResolvedValue(sessionB),
    getMessages: vi.fn().mockResolvedValue({ messages: [], nextBefore: null }),
    listRuns: vi.fn().mockResolvedValue([]),
    startRun: vi
      .fn()
      .mockResolvedValue({ run: createRun('run-new', 'session-a') }),
    stopRun: vi
      .fn()
      .mockImplementation(async (_agentId: string, runId: string) =>
        createRun(runId, 'session-a', { stop: { requestedAtMs: 10 } }),
      ),
    streamEvents: vi
      .fn()
      .mockImplementation((_agentId: string, signal: AbortSignal) =>
        neverStream(signal),
      ),
    ...overrides,
  };
}

function renderWorkspace(api: TuiSessionApi, online = true) {
  const current: { value: UseSessionWorkspaceResult | null } = { value: null };
  function Probe() {
    const workspace = useSessionWorkspace({ api, online });
    useLayoutEffect(() => {
      current.value = workspace;
    }, [workspace]);
    return (
      <Text>
        {`agent=${workspace.selectedAgentId ?? 'none'}|session=${workspace.activeSession?.id ?? 'none'}|messages=${workspace.messages.length}|offline=${String(workspace.isOffline)}|stream=${workspace.streamStatus}`}
      </Text>
    );
  }

  return { rendered: renderInk(<Probe />), current };
}

function stateOf(current: { value: UseSessionWorkspaceResult | null }) {
  if (!current.value) throw new Error('Hook has not rendered');
  return current.value;
}

async function openLoadedSession(
  current: { value: UseSessionWorkspaceResult | null },
  sessionId: string,
) {
  await waitForCondition(
    () =>
      stateOf(current).selectedAgentId !== null &&
      !stateOf(current).loadingAgents &&
      !stateOf(current).loadingSessions,
    'the selected agent and session list',
  );
  expect(await stateOf(current).openSession(sessionId)).toBe(true);
  await waitForCondition(
    () =>
      stateOf(current).activeSession?.id === sessionId &&
      !stateOf(current).loadingSession,
    'the selected session transcript',
  );
}

class EventChannel implements AsyncIterable<AgentEvent> {
  private readonly pending: Array<
    (result: IteratorResult<AgentEvent>) => void
  > = [];
  private readonly buffered: AgentEvent[] = [];
  private ended = false;

  [Symbol.asyncIterator](): AsyncIterator<AgentEvent> {
    return {
      next: () => {
        const value = this.buffered.shift();
        if (value) return Promise.resolve({ value, done: false });
        if (this.ended)
          return Promise.resolve({ value: undefined, done: true });
        return new Promise((resolve) => this.pending.push(resolve));
      },
      return: async () => {
        this.close();
        return { value: undefined, done: true };
      },
    };
  }

  push(event: AgentEvent) {
    const waiter = this.pending.shift();
    if (waiter) waiter({ value: event, done: false });
    else this.buffered.push(event);
  }

  close() {
    this.ended = true;
    for (const waiter of this.pending.splice(0)) {
      waiter({ value: undefined, done: true });
    }
  }
}

describe('useSessionWorkspace', () => {
  it('leaves agent selection empty when there are no agents', async () => {
    const api = createApi({ listAgents: vi.fn().mockResolvedValue([]) });
    const { current } = renderWorkspace(api);

    await settle();

    expect(stateOf(current).agents).toEqual([]);
    expect(stateOf(current).selectedAgentId).toBeNull();
    expect(api.listSessions).not.toHaveBeenCalled();
  });

  it('auto-selects the only agent and requires a choice when several exist', async () => {
    const single = renderWorkspace(createApi());
    await settle();
    expect(stateOf(single.current).selectedAgentId).toBe('agent-a');

    const multipleApi = createApi({
      listAgents: vi.fn().mockResolvedValue([agentA, agentB]),
    });
    const multiple = renderWorkspace(multipleApi);
    await settle();

    expect(stateOf(multiple.current).agents).toHaveLength(2);
    expect(stateOf(multiple.current).selectedAgentId).toBeNull();
    expect(multipleApi.listSessions).not.toHaveBeenCalled();

    await stateOf(multiple.current).selectAgent('agent-b');
    await settle();
    expect(stateOf(multiple.current).selectedAgentId).toBe('agent-b');
    expect(multipleApi.listSessions).toHaveBeenCalledWith(
      'agent-b',
      undefined,
      expect.any(AbortSignal),
    );
  });

  it('lists only active chat sessions and follows the returned page cursor', async () => {
    const archived = createSession('archived', 'agent-a', { archived: true });
    const telegram = createSession('telegram', 'agent-a', { kind: 'telegram' });
    const later = createSession('later', 'agent-a');
    const listSessions = vi
      .fn()
      .mockResolvedValueOnce({
        sessions: [sessionA, archived, telegram],
        nextCursor: 'cursor-2',
      })
      .mockResolvedValueOnce({ sessions: [later], nextCursor: null });
    const api = createApi({ listSessions });
    const { current } = renderWorkspace(api);
    await waitForCondition(
      () =>
        !stateOf(current).loadingSessions &&
        stateOf(current).nextSessionCursor === 'cursor-2',
      'the initial session page',
    );

    expect(stateOf(current).sessions.map(({ id }) => id)).toEqual([
      'session-a',
    ]);
    expect(stateOf(current).nextSessionCursor).toBe('cursor-2');

    await stateOf(current).loadNextSessions();
    await waitForCondition(
      () =>
        stateOf(current).sessions.some(({ id }) => id === 'later') &&
        !stateOf(current).loadingSessions,
      'the next session page',
    );

    expect(listSessions).toHaveBeenNthCalledWith(
      2,
      'agent-a',
      'cursor-2',
      expect.any(AbortSignal),
    );
    expect(stateOf(current).sessions.map(({ id }) => id)).toEqual([
      'session-a',
      'later',
    ]);
    expect(stateOf(current).nextSessionCursor).toBeNull();
  });

  it('opens persisted messages and recent runs and keeps the older-message cursor', async () => {
    const persisted = createMessage('msg-1', 'persisted answer');
    const recentRun = createRun('run-recent', 'session-a', {
      status: 'completed',
    });
    const api = createApi({
      getMessages: vi
        .fn()
        .mockResolvedValue({ messages: [persisted], nextBefore: 'before-1' }),
      listRuns: vi.fn().mockResolvedValue([recentRun]),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    expect(stateOf(current).activeSession?.id).toBe('session-a');
    expect(stateOf(current).messages).toEqual([persisted]);
    expect(stateOf(current).recentRuns).toEqual([recentRun]);
    expect(stateOf(current).messagesCursor).toBe('before-1');
    expect(api.getMessages).toHaveBeenCalledWith(
      'agent-a',
      'session-a',
      undefined,
      expect.any(AbortSignal),
    );
  });

  it('loads older messages using the returned before cursor', async () => {
    const older = createMessage('msg-old', 'older', 1);
    const newer = createMessage('msg-new', 'newer', 2);
    const getMessages = vi
      .fn()
      .mockResolvedValueOnce({ messages: [newer], nextBefore: 'before-old' })
      .mockResolvedValueOnce({ messages: [older], nextBefore: null });
    const api = createApi({ getMessages });
    const { current } = renderWorkspace(api);
    await settle();
    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () => stateOf(current).messagesCursor === 'before-old',
      'the initial message page',
    );
    await stateOf(current).loadEarlierMessages();
    await waitForCondition(
      () =>
        stateOf(current).messages.some(({ id }) => id === 'msg-old') &&
        !stateOf(current).loadingEarlierMessages,
      'the older message page',
    );

    expect(getMessages).toHaveBeenNthCalledWith(
      2,
      'agent-a',
      'session-a',
      'before-old',
      expect.any(AbortSignal),
    );
    expect(stateOf(current).messages.map(({ id }) => id)).toEqual([
      'msg-old',
      'msg-new',
    ]);
    expect(stateOf(current).messagesCursor).toBeNull();
  });

  it('leaves a missing session after an older-message 404 and invalidates its pending reopen', async () => {
    let resolvePendingOpen!: (value: {
      messages: SessionMessage[];
      nextBefore: string | null;
    }) => void;
    const pendingOpen = new Promise<{
      messages: SessionMessage[];
      nextBefore: string | null;
    }>((resolve) => {
      resolvePendingOpen = resolve;
    });
    let rejectOlderMessages!: (reason: unknown) => void;
    const pendingOlderMessages = new Promise<{
      messages: SessionMessage[];
      nextBefore: string | null;
    }>((_resolve, reject) => {
      rejectOlderMessages = reject;
    });
    let messageRead = 0;
    const getMessages = vi.fn().mockImplementation(() => {
      messageRead += 1;
      if (messageRead === 2) return pendingOpen;
      if (messageRead === 3) return pendingOlderMessages;
      return Promise.resolve({
        messages: [createMessage('newer', 'current transcript')],
        nextBefore: 'before-old',
      });
    });
    const listSessions = vi
      .fn()
      .mockResolvedValue({ sessions: [sessionA], nextCursor: null });
    const api = createApi({ getMessages, listSessions });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () => vi.mocked(getMessages).mock.calls.length === 2,
      'the pending transcript request',
    );
    const pendingOpenSignal = vi.mocked(getMessages).mock.calls[1][3];
    await stateOf(current).loadEarlierMessages();
    await settle();

    rejectOlderMessages(Object.assign(new Error('not found'), { status: 404 }));
    await settle();
    expect(stateOf(current).activeSession).toBeNull();
    expect(pendingOpenSignal?.aborted).toBe(true);

    resolvePendingOpen({
      messages: [createMessage('deleted-session', 'must not restore')],
      nextBefore: null,
    });
    await settle();

    expect(stateOf(current).activeSession).toBeNull();
    expect(stateOf(current).messages).toEqual([]);
    expect(stateOf(current).error).toMatch(/deleted/i);
    expect(listSessions).toHaveBeenCalledTimes(2);
  });

  it('aborts a pending older-message request as soon as another session is opened', async () => {
    let resolveOlder!: (value: {
      messages: SessionMessage[];
      nextBefore: string | null;
    }) => void;
    const pendingOlder = new Promise<{
      messages: SessionMessage[];
      nextBefore: string | null;
    }>((resolve) => {
      resolveOlder = resolve;
    });
    const getMessages = vi
      .fn()
      .mockResolvedValueOnce({ messages: [], nextBefore: 'before-old' })
      .mockReturnValueOnce(pendingOlder)
      .mockResolvedValueOnce({ messages: [], nextBefore: null });
    const api = createApi({
      listSessions: vi.fn().mockResolvedValue({
        sessions: [sessionA, sessionB],
        nextCursor: null,
      }),
      getMessages,
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');
    await stateOf(current).loadEarlierMessages();
    await settle();
    const olderSignal = vi.mocked(getMessages).mock.calls[1][3];

    await openLoadedSession(current, 'session-b');

    expect(olderSignal?.aborted).toBe(true);
    resolveOlder({
      messages: [createMessage('stale-older', 'must not apply')],
      nextBefore: null,
    });
    await settle();
    expect(stateOf(current).activeSession?.id).toBe('session-b');
    expect(stateOf(current).messages.map(({ id }) => id)).not.toContain(
      'stale-older',
    );
  });

  it('creates a session, refreshes the list, and opens its saved transcript', async () => {
    const createSession = vi.fn().mockResolvedValue(sessionB);
    const listSessions = vi
      .fn()
      .mockResolvedValueOnce({ sessions: [sessionA], nextCursor: null })
      .mockResolvedValueOnce({
        sessions: [sessionA, sessionB],
        nextCursor: null,
      });
    const getMessages = vi.fn().mockResolvedValue({
      messages: [createMessage('msg-b', 'new chat')],
      nextBefore: null,
    });
    const api = createApi({ createSession, getMessages, listSessions });
    const { current } = renderWorkspace(api);
    await settle();
    const created = await stateOf(current).createSession('New chat');
    await waitForCondition(
      () =>
        stateOf(current).activeSession?.id === 'session-b' &&
        stateOf(current).sessions.some(({ id }) => id === 'session-b'),
      'the created session and refreshed list',
    );

    expect(createSession).toHaveBeenCalledWith('agent-a', 'New chat');
    expect(stateOf(current).sessions.map(({ id }) => id)).toContain(
      'session-b',
    );
    expect(created?.id).toBe('session-b');
    expect(stateOf(current).activeSession?.id).toBe('session-b');
    expect(stateOf(current).messages.map(({ text }) => text)).toEqual([
      'new chat',
    ]);
  });

  it('retains the current transcript and exposes a recoverable error after a failed open', async () => {
    const persisted = createMessage('msg-1', 'keep this transcript');
    const getMessages = vi
      .fn()
      .mockResolvedValueOnce({ messages: [persisted], nextBefore: null })
      .mockRejectedValueOnce(new Error('temporary read failure'));
    const api = createApi({ getMessages });
    const { current } = renderWorkspace(api);
    await settle();
    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () => stateOf(current).activeSession?.id === 'session-a',
      'the original transcript',
    );
    await stateOf(current).openSession('session-b');
    await waitForCondition(
      () => stateOf(current).error !== null && !stateOf(current).loadingSession,
      'the failed open to finish',
    );

    expect(stateOf(current).activeSession?.id).toBe('session-a');
    expect(stateOf(current).messages).toEqual([persisted]);
    expect(stateOf(current).error).toMatch(/temporary read failure/i);
    expect(stateOf(current).canRetry).toBe(true);
  });

  it('returns to the session list and refreshes it when the active session no longer exists', async () => {
    const getMessages = vi
      .fn()
      .mockResolvedValueOnce({ messages: [], nextBefore: null })
      .mockRejectedValueOnce(
        Object.assign(new Error('session not found'), { status: 404 }),
      );
    const listSessions = vi
      .fn()
      .mockResolvedValue({ sessions: [sessionA], nextCursor: null });
    const api = createApi({ getMessages, listSessions });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');
    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () =>
        stateOf(current).activeSession === null &&
        stateOf(current).error !== null,
      'the missing active session to be cleared',
    );

    expect(stateOf(current).activeSession).toBeNull();
    expect(stateOf(current).error).toMatch(/session.*not found|deleted/i);
    expect(listSessions).toHaveBeenCalledTimes(2);
  });

  it('preserves the current transcript when opening a different missing session', async () => {
    const persisted = createMessage('msg-1', 'keep this transcript');
    const getMessages = vi
      .fn()
      .mockResolvedValueOnce({ messages: [persisted], nextBefore: null })
      .mockRejectedValueOnce(
        Object.assign(new Error('session not found'), { status: 404 }),
      );
    const api = createApi({ getMessages });
    const { current } = renderWorkspace(api);
    await settle();
    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () => stateOf(current).activeSession?.id === 'session-a',
      'the original session',
    );
    await stateOf(current).openSession('session-b');
    await waitForCondition(
      () => stateOf(current).error !== null && !stateOf(current).loadingSession,
      'the missing session error',
    );

    expect(stateOf(current).activeSession?.id).toBe('session-a');
    expect(stateOf(current).messages).toEqual([persisted]);
    expect(stateOf(current).error).toMatch(/deleted/i);
  });

  it('invalidates a matching pending open when a refresh reports the session missing', async () => {
    let rejectRefresh!: (reason: unknown) => void;
    const pendingRefresh = new Promise<{
      messages: SessionMessage[];
      nextBefore: string | null;
    }>((_resolve, reject) => {
      rejectRefresh = reject;
    });
    let resolvePendingOpen!: (value: {
      messages: SessionMessage[];
      nextBefore: string | null;
    }) => void;
    const pendingOpen = new Promise<{
      messages: SessionMessage[];
      nextBefore: string | null;
    }>((resolve) => {
      resolvePendingOpen = resolve;
    });
    let messageRead = 0;
    const getMessages = vi.fn().mockImplementation(() => {
      messageRead += 1;
      if (messageRead === 2) return pendingRefresh;
      if (messageRead === 3) return pendingOpen;
      return Promise.resolve({ messages: [], nextBefore: null });
    });
    const api = createApi({ getMessages });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    expect(await stateOf(current).sendMessage('schedule refresh')).toBe(true);
    await waitForCondition(
      () => vi.mocked(getMessages).mock.calls.length === 2,
      'scheduled session refresh',
    );

    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () => vi.mocked(getMessages).mock.calls.length === 3,
      'the pending transcript request',
    );
    const pendingOpenSignal = vi.mocked(getMessages).mock.calls[2][3];
    expect(pendingOpenSignal?.aborted).toBe(false);

    rejectRefresh(
      Object.assign(new Error('session not found'), { status: 404 }),
    );
    await settle();

    expect(stateOf(current).activeSession).toBeNull();
    expect(pendingOpenSignal?.aborted).toBe(true);
    resolvePendingOpen({
      messages: [createMessage('deleted-session-message', 'must not restore')],
      nextBefore: null,
    });
    await settle();

    expect(stateOf(current).activeSession).toBeNull();
    expect(stateOf(current).messages).toEqual([]);
    expect(stateOf(current).error).toMatch(/deleted/i);
  });

  it('disables session-changing actions while offline', async () => {
    const api = createApi();
    const { current } = renderWorkspace(api, false);
    await settle();

    expect(stateOf(current).isOffline).toBe(true);
    expect(stateOf(current).sessionActionsDisabled).toBe(true);
    expect(await stateOf(current).openSession('session-a')).toBe(false);
    expect(await stateOf(current).createSession()).toBeNull();
    expect(await stateOf(current).loadNextSessions()).toBe(false);
    expect(api.createSession).not.toHaveBeenCalled();
    expect(api.getMessages).not.toHaveBeenCalled();
    expect(api.streamEvents).not.toHaveBeenCalled();
  });

  it('sends with the selected queue or steer mode and a fresh idempotency key', async () => {
    const startRun = vi
      .fn()
      .mockResolvedValueOnce({ run: createRun('run-1', 'session-a') })
      .mockResolvedValueOnce({ run: createRun('run-2', 'session-a') });
    const api = createApi({ startRun });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    await stateOf(current).sendMessage('first');
    await stateOf(current).setRunMode('steer');
    await settle();
    await stateOf(current).sendMessage('second');

    expect(startRun).toHaveBeenNthCalledWith(
      1,
      'agent-a',
      'session-a',
      expect.objectContaining({ text: 'first', mode: 'queue' }),
      expect.any(String),
    );
    expect(startRun).toHaveBeenNthCalledWith(
      2,
      'agent-a',
      'session-a',
      expect.objectContaining({ text: 'second', mode: 'steer' }),
      expect.any(String),
    );
    const firstKey = startRun.mock.calls[0][3];
    const secondKey = startRun.mock.calls[1][3];
    expect(firstKey).toBeTruthy();
    expect(secondKey).toBeTruthy();
    expect(secondKey).not.toBe(firstKey);
  });

  it('does not report a send failure after switching to another session', async () => {
    let rejectStartRun!: (reason: unknown) => void;
    const pendingStartRun = new Promise<{ run: Run }>((_resolve, reject) => {
      rejectStartRun = reject;
    });
    const startRun = vi.fn().mockReturnValueOnce(pendingStartRun);
    const api = createApi({
      listSessions: vi.fn().mockResolvedValue({
        sessions: [sessionA, sessionB],
        nextCursor: null,
      }),
      startRun,
    });
    const { current } = renderWorkspace(api);
    await settle();
    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () => stateOf(current).activeSession?.id === 'session-a',
      'the sending session',
    );

    const sendResult = stateOf(current).sendMessage('pending send');
    await waitForCondition(
      () => startRun.mock.calls.length === 1,
      'the pending send to start',
    );
    await stateOf(current).openSession('session-b');
    await waitForCondition(
      () => stateOf(current).activeSession?.id === 'session-b',
      'the new session',
    );
    rejectStartRun(new Error('stale send failure'));
    expect(await sendResult).toBe(false);
    await settle();

    expect(stateOf(current).activeSession?.id).toBe('session-b');
    expect(stateOf(current).error).toBeNull();
  });

  it('leaves a session when sending reports that it was deleted', async () => {
    const startRun = vi
      .fn()
      .mockRejectedValueOnce(
        Object.assign(new Error('session not found'), { status: 404 }),
      );
    const listSessions = vi
      .fn()
      .mockResolvedValue({ sessions: [sessionA], nextCursor: null });
    const api = createApi({ startRun, listSessions });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    expect(await stateOf(current).sendMessage('hello')).toBe(false);
    await waitForCondition(
      () =>
        stateOf(current).activeSession === null &&
        stateOf(current).error !== null,
      'the deleted session to be cleared',
    );

    expect(stateOf(current).activeSession).toBeNull();
    expect(stateOf(current).error).toMatch(/deleted/i);
    expect(listSessions).toHaveBeenCalledTimes(2);
  });

  it('ignores a send 404 for a session that is no longer active or pending', async () => {
    let rejectStartRun!: (reason: unknown) => void;
    const pendingStartRun = new Promise<{ run: Run }>((_resolve, reject) => {
      rejectStartRun = reject;
    });
    const listSessions = vi.fn().mockResolvedValue({
      sessions: [sessionA, sessionB],
      nextCursor: null,
    });
    const api = createApi({
      listSessions,
      startRun: vi.fn().mockReturnValueOnce(pendingStartRun),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () => stateOf(current).activeSession?.id === 'session-a',
      'the sending session',
    );

    const sendResult = stateOf(current).sendMessage('pending send');
    await waitForCondition(
      () => vi.mocked(api.startRun).mock.calls.length === 1,
      'the pending send to start',
    );
    await stateOf(current).openSession('session-b');
    await waitForCondition(
      () => stateOf(current).activeSession?.id === 'session-b',
      'the new session',
    );
    rejectStartRun(
      Object.assign(new Error('session not found'), { status: 404 }),
    );
    expect(await sendResult).toBe(false);
    await settle();

    expect(stateOf(current).activeSession?.id).toBe('session-b');
    expect(stateOf(current).error).toBeNull();
    expect(listSessions).toHaveBeenCalledTimes(1);
  });

  it('stops only a stoppable run and keeps stop requested pending until terminal state', async () => {
    const running = createRun('run-live', 'session-a');
    const stopAccepted = createRun('run-live', 'session-a', {
      stop: { requestedAtMs: 9 },
    });
    const terminal = createRun('run-done', 'session-a', {
      status: 'completed',
    });
    const channels: EventChannel[] = [];
    const api = createApi({
      listRuns: vi.fn().mockResolvedValue([running, terminal]),
      stopRun: vi.fn().mockResolvedValue(stopAccepted),
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    expect(await stateOf(current).stopActiveRun()).toBe(true);
    await settle();
    expect(api.stopRun).toHaveBeenCalledTimes(1);
    expect(api.stopRun).toHaveBeenCalledWith('agent-a', 'run-live');
    expect(stateOf(current).stopRequestedRunIds).toContain('run-live');
    expect(stateOf(current).stopRequestedRunIds).not.toContain('run-done');
    expect(await stateOf(current).stopActiveRun()).toBe(false);
    expect(api.stopRun).toHaveBeenCalledTimes(1);

    channels.at(-1)?.push({
      type: 'run.cancelled',
      agentId: 'agent-a',
      sessionId: 'session-a',
      seq: 2,
      at: 20,
      run: createRun('run-live', 'session-a', {
        status: 'cancelled',
        stop: { requestedAtMs: 9 },
      }),
    });
    await settle();

    expect(stateOf(current).stopRequestedRunIds).not.toContain('run-live');
  });

  it('keeps the chat open when stopping a missing run returns 404', async () => {
    const listSessions = vi
      .fn()
      .mockResolvedValue({ sessions: [sessionA], nextCursor: null });
    const api = createApi({
      listRuns: vi.fn().mockResolvedValue([createRun('run-live', 'session-a')]),
      listSessions,
      stopRun: vi
        .fn()
        .mockRejectedValue(
          Object.assign(new Error('run not found'), { status: 404 }),
        ),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    expect(await stateOf(current).stopActiveRun()).toBe(false);
    await waitForCondition(
      () => stateOf(current).error !== null,
      'the missing-run error',
    );

    expect(stateOf(current).activeSession?.id).toBe('session-a');
    expect(stateOf(current).error).toMatch(/run not found/i);
    expect(stateOf(current).canRetry).toBe(true);
    expect(listSessions).toHaveBeenCalledTimes(1);
  });

  it('does not report a stop failure after switching to another session', async () => {
    let rejectStopRun!: (reason: unknown) => void;
    const pendingStopRun = new Promise<Run>((_resolve, reject) => {
      rejectStopRun = reject;
    });
    const api = createApi({
      listSessions: vi.fn().mockResolvedValue({
        sessions: [sessionA, sessionB],
        nextCursor: null,
      }),
      listRuns: vi
        .fn()
        .mockImplementation(async (_agentId: string, sessionId: string) =>
          sessionId === 'session-a' ? [createRun('run-live', sessionId)] : [],
        ),
      stopRun: vi.fn().mockReturnValueOnce(pendingStopRun),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    const stopResult = stateOf(current).stopActiveRun();
    await waitForCondition(
      () => vi.mocked(api.stopRun).mock.calls.length === 1,
      'the stop request to start',
    );
    await openLoadedSession(current, 'session-b');
    rejectStopRun(new Error('stale stop failure'));
    expect(await stopResult).toBe(false);
    await settle();

    expect(stateOf(current).activeSession?.id).toBe('session-b');
    expect(stateOf(current).error).toBeNull();
  });

  it('restores snapshot state and associates helper events through their run on the selected stream', async () => {
    const channels: EventChannel[] = [];
    const api = createApi({
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');
    const channel = channels.at(-1);
    if (!channel) throw new Error('Expected the selected agent event stream');

    channel.push({
      type: 'stream.snapshot',
      agentId: 'agent-a',
      seq: 1,
      at: 10,
      runs: [
        {
          run: createRun('run-helper', 'session-a', {
            agentId: 'agent-helper',
          }),
          stepId: 'helper-step',
          text: 'working',
          textOffset: 0,
          tools: [
            {
              stepId: 'helper-step',
              toolCallId: 'helper-tool',
              name: 'delegate',
              argumentsPreview: '{}',
              argumentsTruncated: false,
              status: 'running',
              durationMs: null,
              resultPreview: null,
              truncated: false,
            },
          ],
        },
      ],
      approvals: [],
    });
    channel.push({
      type: 'run.progress',
      agentId: 'agent-helper',
      runId: 'run-helper',
      seq: 2,
      at: 11,
      phase: 'waiting for approval',
    });
    await settle();

    expect(stateOf(current).eventState?.runs.map(({ id }) => id)).toContain(
      'run-helper',
    );
    expect(stateOf(current).eventState?.steps['helper-step'].text).toBe(
      'working',
    );
    expect(
      Object.values(stateOf(current).eventState?.tools ?? {}),
    ).toMatchObject([
      expect.objectContaining({
        runId: 'run-helper',
        toolCallId: 'helper-tool',
      }),
    ]);
    expect(stateOf(current).eventState?.progressByRun['run-helper']).toBe(
      'waiting for approval',
    );
  });

  it('reloads authoritative messages and runs before reconnecting after an iterator closes', async () => {
    const channels: EventChannel[] = [];
    const callOrder: string[] = [];
    const api = createApi({
      getMessages: vi.fn().mockImplementation(async () => {
        callOrder.push('messages');
        return { messages: [], nextBefore: null };
      }),
      listRuns: vi.fn().mockImplementation(async () => {
        callOrder.push('runs');
        return [];
      }),
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          callOrder.push('stream');
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');
    callOrder.length = 0;
    channels.at(-1)?.close();
    await waitForCondition(
      () => vi.mocked(api.streamEvents).mock.calls.length > 1,
      'stream reconnection after iterator close',
    );
    await settle();

    expect(callOrder.indexOf('messages')).toBeGreaterThanOrEqual(0);
    expect(callOrder.indexOf('runs')).toBeGreaterThanOrEqual(0);
    expect(callOrder.lastIndexOf('stream')).toBeGreaterThan(
      callOrder.lastIndexOf('runs'),
    );
    expect(stateOf(current).streamStatus).toBe('connected');
  });

  it('backs off repeated stream reconnects with an increasing capped delay', async () => {
    const channels: EventChannel[] = [];
    const api = createApi({
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    renderWorkspace(api);
    await settle();
    expect(api.streamEvents).toHaveBeenCalledTimes(1);

    vi.useFakeTimers();
    const delays = [250, 500, 1_000, 2_000, 4_000, 8_000, 8_000];
    for (const [index, delay] of delays.entries()) {
      const channel = channels[index];
      if (!channel) throw new Error('Expected a stream before closing it');
      channel.close();
      await flushMicrotasks();

      await vi.advanceTimersByTimeAsync(delay - 1);
      await flushMicrotasks();
      expect(api.streamEvents).toHaveBeenCalledTimes(index + 1);

      await vi.advanceTimersByTimeAsync(1);
      await flushMicrotasks();
      expect(api.streamEvents).toHaveBeenCalledTimes(index + 2);
    }
    vi.useRealTimers();
  });

  it('resets reconnect backoff after a stream stays connected for 30 seconds', async () => {
    const channels: EventChannel[] = [];
    const api = createApi({
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    renderWorkspace(api);
    await settle();
    expect(api.streamEvents).toHaveBeenCalledTimes(1);

    vi.useFakeTimers();
    channels[0]?.close();
    await flushMicrotasks();
    await vi.advanceTimersByTimeAsync(250);
    await flushMicrotasks();
    expect(api.streamEvents).toHaveBeenCalledTimes(2);

    await vi.advanceTimersByTimeAsync(30_000);
    channels[1]?.close();
    await flushMicrotasks();
    await vi.advanceTimersByTimeAsync(249);
    await flushMicrotasks();
    expect(api.streamEvents).toHaveBeenCalledTimes(2);

    await vi.advanceTimersByTimeAsync(1);
    await flushMicrotasks();
    expect(api.streamEvents).toHaveBeenCalledTimes(3);
    vi.useRealTimers();
  });

  it('reloads authoritative state before reconnecting after stream resync', async () => {
    const channels: EventChannel[] = [];
    const callOrder: string[] = [];
    const api = createApi({
      getMessages: vi.fn().mockImplementation(async () => {
        callOrder.push('messages');
        return { messages: [], nextBefore: null };
      }),
      listRuns: vi.fn().mockImplementation(async () => {
        callOrder.push('runs');
        return [];
      }),
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          callOrder.push('stream');
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');
    callOrder.length = 0;
    channels.at(-1)?.push({
      type: 'stream.resync',
      agentId: 'agent-a',
      seq: 2,
      at: 20,
      missed: 3,
    });
    await waitForCondition(
      () => vi.mocked(api.streamEvents).mock.calls.length > 1,
      'stream reconnection after resync',
    );
    await settle();

    expect(callOrder.indexOf('messages')).toBeGreaterThanOrEqual(0);
    expect(callOrder.indexOf('runs')).toBeGreaterThanOrEqual(0);
    expect(callOrder.lastIndexOf('stream')).toBeGreaterThan(
      callOrder.lastIndexOf('runs'),
    );
    expect(stateOf(current).streamStatus).toBe('connected');
  });

  it('replaces stale live runs with authoritative runs after resync', async () => {
    const running = createRun('run-live', 'session-a');
    const terminal = createRun('run-live', 'session-a', {
      status: 'completed',
      finishedAtMs: 20,
    });
    const channels: EventChannel[] = [];
    const listRuns = vi
      .fn()
      .mockResolvedValueOnce([running])
      .mockResolvedValueOnce([terminal]);
    const api = createApi({
      listRuns,
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    channels[0]?.push({
      type: 'stream.resync',
      agentId: 'agent-a',
      seq: 2,
      at: 20,
      missed: 1,
    });
    await waitForCondition(
      () => vi.mocked(api.streamEvents).mock.calls.length > 1,
      'stream reconnection after authoritative run refresh',
    );
    await settle();

    expect(stateOf(current).eventState?.runs).toEqual([terminal]);
    expect(stateOf(current).runs).toEqual([terminal]);
    expect(stateOf(current).canStopActiveRun).toBe(false);
    expect(await stateOf(current).stopActiveRun()).toBe(false);
    expect(api.stopRun).not.toHaveBeenCalled();
  });

  it('keeps one agent stream across session switches and reapplies its latest snapshot', async () => {
    const channels: EventChannel[] = [];
    const api = createApi({
      listSessions: vi.fn().mockResolvedValue({
        sessions: [sessionA, sessionB],
        nextCursor: null,
      }),
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    const channel = channels[0];
    if (!channel) throw new Error('Expected the selected agent event stream');

    channel.push({
      type: 'stream.snapshot',
      agentId: 'agent-a',
      seq: 1,
      at: 10,
      runs: [
        {
          run: createRun('run-helper', 'session-a', {
            agentId: 'agent-helper',
          }),
          stepId: 'helper-step',
          text: 'working',
          textOffset: 0,
          tools: [
            {
              stepId: 'helper-step',
              toolCallId: 'helper-tool',
              name: 'delegate',
              argumentsPreview: '{}',
              argumentsTruncated: false,
              status: 'running',
              durationMs: null,
              resultPreview: null,
              truncated: false,
            },
          ],
        },
        {
          run: createRun('run-session-b', 'session-b'),
          stepId: 'session-b-step',
          text: 'session B work',
          textOffset: 0,
          tools: [],
        },
      ],
      approvals: [],
    });
    await settle();

    await openLoadedSession(current, 'session-a');
    expect(stateOf(current).eventState?.steps['helper-step'].text).toBe(
      'working',
    );
    expect(
      Object.values(stateOf(current).eventState?.tools ?? {}),
    ).toMatchObject([
      expect.objectContaining({
        runId: 'run-helper',
        toolCallId: 'helper-tool',
      }),
    ]);

    await openLoadedSession(current, 'session-b');

    expect(api.streamEvents).toHaveBeenCalledTimes(1);
    expect(channels).toHaveLength(1);
    expect(stateOf(current).eventState?.runs.map(({ id }) => id)).toEqual([
      'run-session-b',
    ]);
    expect(stateOf(current).eventState?.steps['session-b-step'].text).toBe(
      'session B work',
    );
  });

  it('keeps fresh terminal runs when reopening a session with a stale cached snapshot', async () => {
    const running = createRun('run-cached', 'session-a');
    const terminal = createRun('run-cached', 'session-a', {
      status: 'completed',
      finishedAtMs: 30,
    });
    const channels: EventChannel[] = [];
    let sessionAReads = 0;
    const listRuns = vi
      .fn()
      .mockImplementation(async (_agentId: string, sessionId: string) => {
        if (sessionId !== 'session-a') return [];
        sessionAReads += 1;
        return sessionAReads === 1 ? [running] : [terminal];
      });
    const api = createApi({
      listSessions: vi.fn().mockResolvedValue({
        sessions: [sessionA, sessionB],
        nextCursor: null,
      }),
      listRuns,
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    const channel = channels[0];
    if (!channel) throw new Error('Expected the selected agent event stream');

    channel.push({
      type: 'stream.snapshot',
      agentId: 'agent-a',
      seq: 1,
      at: 10,
      runs: [
        {
          run: running,
          stepId: 'cached-step',
          text: 'cached progress',
          textOffset: 0,
          tools: [
            {
              stepId: 'cached-step',
              toolCallId: 'cached-tool',
              name: 'lookup',
              argumentsPreview: '{}',
              argumentsTruncated: false,
              status: 'running',
              durationMs: null,
              resultPreview: null,
              truncated: false,
            },
          ],
        },
      ],
      approvals: [],
    });
    await settle();

    await openLoadedSession(current, 'session-a');
    await openLoadedSession(current, 'session-b');
    channel.push({
      type: 'run.completed',
      agentId: 'agent-a',
      sessionId: 'session-a',
      seq: 2,
      at: 30,
      run: terminal,
    });
    await settle();

    await openLoadedSession(current, 'session-a');

    expect(
      listRuns.mock.calls.filter(([, sessionId]) => sessionId === 'session-a'),
    ).toHaveLength(2);
    expect(stateOf(current).eventState?.runs).toEqual([terminal]);
    expect(stateOf(current).eventState?.steps['cached-step'].text).toBe(
      'cached progress',
    );
    expect(
      Object.values(stateOf(current).eventState?.tools ?? {}),
    ).toMatchObject([
      expect.objectContaining({
        runId: 'run-cached',
        toolCallId: 'cached-tool',
      }),
    ]);
    expect(stateOf(current).canStopActiveRun).toBe(false);
    expect(await stateOf(current).stopActiveRun()).toBe(false);
    expect(api.stopRun).not.toHaveBeenCalled();
  });

  it('aborts pending session loads when the active session changes and ignores stale results', async () => {
    let resolveMessagesA!: (value: {
      messages: SessionMessage[];
      nextBefore: string | null;
    }) => void;
    const firstMessageLoad = new Promise<{
      messages: SessionMessage[];
      nextBefore: string | null;
    }>((resolve) => {
      resolveMessagesA = resolve;
    });
    const getMessages = vi
      .fn()
      .mockReturnValueOnce(firstMessageLoad)
      .mockResolvedValue({ messages: [], nextBefore: null });
    const api = createApi({
      listSessions: vi.fn().mockResolvedValue({
        sessions: [sessionA, sessionB],
        nextCursor: null,
      }),
      getMessages,
    });
    const { current } = renderWorkspace(api);
    await settle();
    await stateOf(current).openSession('agent-a-session');
    await settle();
    const loadSignal = vi.mocked(getMessages).mock.calls[0][3];
    await stateOf(current).openSession('another-session');
    await settle();

    expect(loadSignal?.aborted).toBe(true);
    resolveMessagesA({
      messages: [createMessage('stale', 'must not apply')],
      nextBefore: null,
    });
    await settle();
    expect(stateOf(current).messages.map(({ id }) => id)).not.toContain(
      'stale',
    );
    expect(stateOf(current).activeSession?.id).toBe('another-session');
  });

  it('aborts the prior agent session-list request when the selected agent changes', async () => {
    const api = createApi({
      listAgents: vi.fn().mockResolvedValue([agentA, agentB]),
      listSessions: vi
        .fn()
        .mockResolvedValue({ sessions: [sessionA], nextCursor: null }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await stateOf(current).selectAgent('agent-a');
    await settle();
    const firstAgentSignal = vi.mocked(api.listSessions).mock.calls[0][2];

    await stateOf(current).selectAgent('agent-b');
    await settle();

    expect(firstAgentSignal?.aborted).toBe(true);
    expect(vi.mocked(api.listSessions).mock.calls.at(-1)?.[0]).toBe('agent-b');
  });

  it('invalidates a pending open when its session is deleted', async () => {
    let resolveMessages!: (value: {
      messages: SessionMessage[];
      nextBefore: string | null;
    }) => void;
    const pendingMessages = new Promise<{
      messages: SessionMessage[];
      nextBefore: string | null;
    }>((resolve) => {
      resolveMessages = resolve;
    });
    const channels: EventChannel[] = [];
    const listSessions = vi
      .fn()
      .mockResolvedValue({ sessions: [sessionA], nextCursor: null });
    const getMessages = vi.fn().mockReturnValueOnce(pendingMessages);
    const api = createApi({
      listSessions,
      getMessages,
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await stateOf(current).openSession('session-a');
    await waitForCondition(
      () => vi.mocked(getMessages).mock.calls.length === 1,
      'the pending transcript request',
    );
    const loadSignal = vi.mocked(getMessages).mock.calls[0][3];
    const channel = channels[0];
    if (!channel) throw new Error('Expected the selected agent event stream');

    channel.push({
      type: 'session.deleted',
      agentId: 'agent-a',
      sessionId: 'session-a',
      seq: 1,
      at: 10,
    });
    await settle();
    resolveMessages({
      messages: [createMessage('deleted-message', 'must not restore')],
      nextBefore: null,
    });
    await settle();

    expect(loadSignal?.aborted).toBe(true);
    expect(stateOf(current).activeSession).toBeNull();
    expect(stateOf(current).messages).toEqual([]);
    expect(stateOf(current).error).toMatch(/deleted/i);
    expect(listSessions).toHaveBeenCalledTimes(2);
  });

  it('leaves a deleted active session, reloads the list, and shows a recoverable message', async () => {
    const channels: EventChannel[] = [];
    const listSessions = vi
      .fn()
      .mockResolvedValue({ sessions: [sessionA], nextCursor: null });
    const api = createApi({
      listSessions,
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');
    const channel = channels.at(-1);
    if (!channel) throw new Error('Expected the selected agent event stream');

    channel.push({
      type: 'session.deleted',
      agentId: 'agent-a',
      sessionId: 'session-a',
      seq: 1,
      at: 10,
    });
    await settle();

    expect(stateOf(current).activeSession).toBeNull();
    expect(listSessions).toHaveBeenCalledTimes(2);
    expect(stateOf(current).error).toMatch(/deleted/i);
    expect(stateOf(current).canRetry).toBe(true);
  });

  it('ignores a session deletion event from a different agent stream', async () => {
    const channels: EventChannel[] = [];
    const listSessions = vi
      .fn()
      .mockResolvedValue({ sessions: [sessionA], nextCursor: null });
    const api = createApi({
      listSessions,
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          const channel = new EventChannel();
          channels.push(channel);
          signal.addEventListener('abort', () => channel.close(), {
            once: true,
          });
          return channel;
        }),
    });
    const { current } = renderWorkspace(api);
    await settle();
    await openLoadedSession(current, 'session-a');

    channels[0]?.push({
      type: 'session.deleted',
      agentId: 'agent-b',
      sessionId: 'session-a',
      seq: 1,
      at: 10,
    });
    await settle();

    expect(stateOf(current).activeSession?.id).toBe('session-a');
    expect(listSessions).toHaveBeenCalledTimes(1);
    expect(stateOf(current).error).toBeNull();
  });

  it('aborts the selected-agent event stream on unmount', async () => {
    let streamSignal: AbortSignal | undefined;
    const api = createApi({
      streamEvents: vi
        .fn()
        .mockImplementation((_agentId: string, signal: AbortSignal) => {
          streamSignal = signal;
          return neverStream(signal);
        }),
    });
    const { rendered } = renderWorkspace(api);
    await settle();
    expect(streamSignal?.aborted).toBe(false);

    rendered.unmount();
    await settle();

    expect(streamSignal?.aborted).toBe(true);
  });
});
