import { useCallback, useEffect, useRef, useState } from 'react';
import {
  isTerminalRunStatus,
  type AgentEvent,
  type AgentSummary,
  type Run,
  type RunMode,
  type Session,
  type SessionMessage,
} from '@animaos/sdk';
import type { TuiSessionApi } from '../session-api.js';
import {
  clearSessionRefreshRequest,
  createSessionEventState,
  mergeSessionMessages,
  reduceSessionEvent,
  type SessionEventState,
} from '../session-state.js';

const RECONNECT_BASE_DELAY_MS = 250;
const RECONNECT_MAX_DELAY_MS = 8_000;
const RECONNECT_RESET_AFTER_MS = 30_000;
const RECONNECT_MAX_ATTEMPT = 5;
const STOPPABLE_RUN_STATUSES = new Set<Run['status']>([
  'queued',
  'running',
  'awaiting_approval',
]);

export interface UseSessionWorkspaceOptions {
  api?: TuiSessionApi | null;
  online?: boolean;
  onEvent?: (event: AgentEvent) => void;
}

export interface UseSessionWorkspaceResult {
  agents: AgentSummary[];
  selectedAgentId: string | null;
  selectedAgent: AgentSummary | null;
  agentSelectionRequired: boolean;
  sessions: Session[];
  sessionCursor: string | undefined;
  nextSessionCursor: string | null;
  activeSession: Session | null;
  messages: SessionMessage[];
  messagesCursor: string | null;
  recentRuns: Run[];
  runs: Run[];
  eventState: SessionEventState | null;
  runMode: RunMode;
  streamStatus: 'disconnected' | 'connecting' | 'connected' | 'reconnecting';
  loadingAgents: boolean;
  loadingSessions: boolean;
  loadingSession: boolean;
  loadingEarlierMessages: boolean;
  error: string | null;
  canRetry: boolean;
  isOffline: boolean;
  sessionActionsDisabled: boolean;
  stopRequestedRunIds: string[];
  canStopActiveRun: boolean;
  activeRun: Run | null;
  selectAgent(agentId: string): Promise<boolean>;
  loadNextSessions(): Promise<boolean>;
  refreshSessions(): void;
  openSession(session: Session | string): Promise<boolean>;
  createSession(title?: string): Promise<Session | null>;
  leaveSession(): void;
  loadEarlierMessages(): Promise<boolean>;
  setRunMode(mode: RunMode): void;
  sendMessage(text: string): Promise<boolean>;
  stopActiveRun(): Promise<boolean>;
  retry(): void;
  clearError(): void;
}

interface SessionOpenRequest {
  id: number;
  session: Session;
}

interface OlderMessageRequest {
  id: number;
  before: string;
}

interface RefreshRequest {
  id: number;
  agentId: string;
  sessionId: string;
}

interface StoredStreamSnapshot {
  agentId: string;
  event: Extract<AgentEvent, { type: 'stream.snapshot' }>;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const SESSION_MISSING_MESSAGE =
  /session.{0,30}(not found|deleted)|(?:not found|deleted).{0,30}session/i;

function hasSessionSpecificMissingMessage(error: unknown): boolean {
  if (!error || typeof error !== 'object') return false;
  const candidate = error as {
    message?: unknown;
  };
  return (
    typeof candidate.message === 'string' &&
    SESSION_MISSING_MESSAGE.test(candidate.message)
  );
}

function isExplicitMissingSessionError(error: unknown): boolean {
  return hasSessionSpecificMissingMessage(error);
}

function isMissingSessionError(error: unknown): boolean {
  if (error && typeof error === 'object') {
    const candidate = error as {
      status?: unknown;
      statusCode?: unknown;
    };
    if (candidate.status === 404 || candidate.statusCode === 404) return true;
  }
  return hasSessionSpecificMissingMessage(error);
}

function createSessionReference(agentId: string, sessionId: string): Session {
  return {
    id: sessionId,
    agentId,
    roomId: sessionId,
    kind: 'chat',
    origin: 'web',
    title: sessionId,
    titleSource: 'system',
    createdAtMs: Date.now(),
    lastActivityAtMs: Date.now(),
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
  };
}

function createIdempotencyKey(): string {
  if (typeof globalThis.crypto?.randomUUID === 'function') {
    return globalThis.crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}-${Math.random().toString(36).slice(2)}`;
}

function dedupeSessions(current: Session[], incoming: Session[]): Session[] {
  const byId = new Map(current.map((session) => [session.id, session]));
  for (const session of incoming) byId.set(session.id, session);
  return [...byId.values()];
}

function dedupeRuns(current: Run[], incoming: Run[]): Run[] {
  const byId = new Map(current.map((run) => [run.id, run]));
  for (const run of incoming) byId.set(run.id, run);
  return [...byId.values()].sort(
    (left, right) => right.createdAtMs - left.createdAtMs,
  );
}

function createLoadedEventState(
  agentId: string,
  sessionId: string,
  messages: SessionMessage[],
  runs: Run[],
): SessionEventState {
  const state = createSessionEventState(agentId, sessionId, messages);
  return {
    ...state,
    runs,
    runSessionIds: Object.fromEntries(
      runs.map((run) => [run.id, run.sessionId]),
    ),
    runAgentIds: Object.fromEntries(runs.map((run) => [run.id, run.agentId])),
  };
}

function replaceEventStateRuns(
  state: SessionEventState,
  runs: Run[],
): SessionEventState {
  return {
    ...state,
    runs,
    runSessionIds: {
      ...state.runSessionIds,
      ...Object.fromEntries(runs.map((run) => [run.id, run.sessionId])),
    },
    runAgentIds: {
      ...state.runAgentIds,
      ...Object.fromEntries(runs.map((run) => [run.id, run.agentId])),
    },
  };
}

function waitForReconnect(signal: AbortSignal, delayMs: number): Promise<void> {
  if (signal.aborted) return Promise.resolve();
  return new Promise((resolve) => {
    const timeout = setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve();
    }, delayMs);
    const onAbort = () => {
      clearTimeout(timeout);
      signal.removeEventListener('abort', onAbort);
      resolve();
    };
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

export function useSessionWorkspace({
  api,
  online = true,
  onEvent,
}: UseSessionWorkspaceOptions = {}): UseSessionWorkspaceResult {
  const onEventRef = useRef(onEvent);
  onEventRef.current = onEvent;
  const [agents, setAgents] = useState<AgentSummary[]>([]);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [sessionCursor, setSessionCursor] = useState<string | undefined>();
  const [nextSessionCursor, setNextSessionCursor] = useState<string | null>(
    null,
  );
  const [activeSession, setActiveSession] = useState<Session | null>(null);
  const [eventState, setEventState] = useState<SessionEventState | null>(null);
  const [messagesCursor, setMessagesCursor] = useState<string | null>(null);
  const [recentRuns, setRecentRuns] = useState<Run[]>([]);
  const [runMode, setRunMode] = useState<RunMode>('queue');
  const [streamStatus, setStreamStatus] =
    useState<UseSessionWorkspaceResult['streamStatus']>('disconnected');
  const [loadingAgents, setLoadingAgents] = useState(false);
  const [loadingSessions, setLoadingSessions] = useState(false);
  const [loadingSession, setLoadingSession] = useState(false);
  const [loadingEarlierMessages, setLoadingEarlierMessages] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sessionListRevision, setSessionListRevision] = useState(0);
  const [agentLoadRevision, setAgentLoadRevision] = useState(0);
  const [openRequest, setOpenRequest] = useState<SessionOpenRequest | null>(
    null,
  );
  const [olderMessageRequest, setOlderMessageRequest] =
    useState<OlderMessageRequest | null>(null);
  const [refreshRequest, setRefreshRequest] = useState<RefreshRequest | null>(
    null,
  );
  const [stopRequestedRunIds, setStopRequestedRunIds] = useState<string[]>([]);

  const mountedRef = useRef(false);
  const selectedAgentIdRef = useRef(selectedAgentId);
  const activeSessionIdRef = useRef(activeSession?.id ?? null);
  const eventStateRef = useRef(eventState);
  const openRequestRef = useRef(openRequest);
  const openRequestIdRef = useRef(0);
  const olderMessageRequestIdRef = useRef(0);
  const refreshRequestIdRef = useRef(0);
  const sessionCursorRef = useRef(sessionCursor);
  const nextSessionCursorRef = useRef(nextSessionCursor);
  const messagesCursorRef = useRef(messagesCursor);
  const recentRunsRef = useRef(recentRuns);
  const stopRequestedRunIdsRef = useRef(stopRequestedRunIds);
  const latestSnapshotRef = useRef<StoredStreamSnapshot | null>(null);

  selectedAgentIdRef.current = selectedAgentId;
  activeSessionIdRef.current = activeSession?.id ?? null;
  eventStateRef.current = eventState;
  sessionCursorRef.current = sessionCursor;
  nextSessionCursorRef.current = nextSessionCursor;
  messagesCursorRef.current = messagesCursor;
  recentRunsRef.current = recentRuns;
  stopRequestedRunIdsRef.current = stopRequestedRunIds;

  const commitEventState = useCallback((next: SessionEventState | null) => {
    eventStateRef.current = next;
    setEventState(next);
  }, []);

  const clearSessionContext = useCallback(() => {
    activeSessionIdRef.current = null;
    commitEventState(null);
    setActiveSession(null);
    setMessagesCursor(null);
    setRecentRuns([]);
    recentRunsRef.current = [];
    setStopRequestedRunIds([]);
    stopRequestedRunIdsRef.current = [];
  }, [commitEventState]);

  const invalidatePendingOpen = useCallback(
    (agentId: string, sessionId: string) => {
      const pending = openRequestRef.current;
      if (
        !pending ||
        pending.session.agentId !== agentId ||
        pending.session.id !== sessionId
      ) {
        return false;
      }

      openRequestIdRef.current += 1;
      openRequestRef.current = null;
      setOpenRequest((request) =>
        request?.id === pending.id ? null : request,
      );
      setLoadingSession(false);
      return true;
    },
    [],
  );

  const scheduleRefresh = useCallback((agentId: string, sessionId: string) => {
    setRefreshRequest({
      id: ++refreshRequestIdRef.current,
      agentId,
      sessionId,
    });
  }, []);

  const refreshSessions = useCallback(() => {
    if (!online || !api || !selectedAgentIdRef.current) return;
    setSessionCursor(undefined);
    sessionCursorRef.current = undefined;
    setSessionListRevision((revision) => revision + 1);
  }, [api, online]);

  const recoverMissingSession = useCallback(
    (agentId: string, sessionId: string) => {
      if (!mountedRef.current || selectedAgentIdRef.current !== agentId) {
        return false;
      }

      const isActiveSession = activeSessionIdRef.current === sessionId;
      const pendingOpen = openRequestRef.current;
      const isPendingOpen =
        pendingOpen?.session.agentId === agentId &&
        pendingOpen.session.id === sessionId;
      if (!isActiveSession && !isPendingOpen) return false;

      invalidatePendingOpen(agentId, sessionId);
      if (isActiveSession) {
        clearSessionContext();
        setOlderMessageRequest(null);
        setLoadingEarlierMessages(false);
        setRefreshRequest(null);
      }
      refreshSessions();
      setError(
        'This session was deleted. Reload the session list and choose another.',
      );
      return true;
    },
    [clearSessionContext, invalidatePendingOpen, refreshSessions],
  );

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    if (!api || !online) {
      setLoadingAgents(false);
      return;
    }

    const controller = new AbortController();
    let disposed = false;
    setLoadingAgents(true);

    void api
      .listAgents(controller.signal)
      .then((loadedAgents) => {
        if (disposed || controller.signal.aborted) return;
        setAgents(loadedAgents);
        const currentId = selectedAgentIdRef.current;
        if (
          currentId &&
          loadedAgents.some((agent) => agent.state.id === currentId)
        ) {
          return;
        }

        const autoSelectedId =
          loadedAgents.length === 1 ? loadedAgents[0].state.id : null;
        if (currentId !== autoSelectedId) {
          latestSnapshotRef.current = null;
          openRequestIdRef.current += 1;
          openRequestRef.current = null;
          setOpenRequest(null);
        }
        selectedAgentIdRef.current = autoSelectedId;
        setSelectedAgentId(autoSelectedId);
        setLoadingSessions(Boolean(autoSelectedId));
        setSessions([]);
        setSessionCursor(undefined);
        sessionCursorRef.current = undefined;
        setNextSessionCursor(null);
        nextSessionCursorRef.current = null;
        clearSessionContext();
      })
      .catch((loadError: unknown) => {
        if (disposed || controller.signal.aborted) return;
        setError(errorMessage(loadError));
      })
      .finally(() => {
        if (!disposed) setLoadingAgents(false);
      });

    return () => {
      disposed = true;
      controller.abort();
    };
  }, [agentLoadRevision, api, clearSessionContext, online]);

  useEffect(() => {
    if (!api || !online || !selectedAgentId) {
      setLoadingSessions(false);
      return;
    }

    const agentId = selectedAgentId;
    const cursor = sessionCursor;
    const controller = new AbortController();
    let disposed = false;
    setLoadingSessions(true);

    void api
      .listSessions(agentId, cursor, controller.signal)
      .then((page) => {
        if (
          disposed ||
          controller.signal.aborted ||
          selectedAgentIdRef.current !== agentId
        ) {
          return;
        }
        const visibleSessions = page.sessions.filter(
          (session) =>
            session.agentId === agentId &&
            session.kind === 'chat' &&
            !session.archived,
        );
        setSessions((current) =>
          cursor === undefined
            ? dedupeSessions([], visibleSessions)
            : dedupeSessions(current, visibleSessions),
        );
        setNextSessionCursor(page.nextCursor);
        nextSessionCursorRef.current = page.nextCursor;
      })
      .catch((loadError: unknown) => {
        if (disposed || controller.signal.aborted) return;
        setError(errorMessage(loadError));
      })
      .finally(() => {
        if (!disposed) setLoadingSessions(false);
      });

    return () => {
      disposed = true;
      controller.abort();
    };
  }, [api, online, selectedAgentId, sessionCursor, sessionListRevision]);

  useEffect(() => {
    const request = openRequest;
    const agentId = selectedAgentId;
    if (!request || !api || !online || !agentId) {
      setLoadingSession(false);
      return;
    }

    const controller = new AbortController();
    let disposed = false;
    setLoadingSession(true);

    void Promise.all([
      api.getMessages(
        agentId,
        request.session.id,
        undefined,
        controller.signal,
      ),
      api.listRuns(agentId, request.session.id, controller.signal),
    ])
      .then(([page, runs]) => {
        if (
          disposed ||
          controller.signal.aborted ||
          selectedAgentIdRef.current !== agentId ||
          openRequestIdRef.current !== request.id ||
          openRequestRef.current?.id !== request.id
        ) {
          return;
        }
        let nextState = createLoadedEventState(
          agentId,
          request.session.id,
          page.messages,
          runs,
        );
        const latestSnapshot = latestSnapshotRef.current;
        if (latestSnapshot?.agentId === agentId) {
          nextState = reduceSessionEvent(
            nextState,
            latestSnapshot.event,
            agentId,
          );
          nextState = replaceEventStateRuns(
            nextState,
            dedupeRuns(nextState.runs, runs),
          );
        }
        activeSessionIdRef.current = request.session.id;
        setActiveSession(request.session);
        commitEventState(nextState);
        setMessagesCursor(page.nextBefore);
        messagesCursorRef.current = page.nextBefore;
        setRecentRuns(runs);
        recentRunsRef.current = runs;
        const pendingStops = runs
          .filter(
            (run) => run.stop !== null && !isTerminalRunStatus(run.status),
          )
          .map((run) => run.id);
        setStopRequestedRunIds(pendingStops);
        stopRequestedRunIdsRef.current = pendingStops;
        setError(null);
        openRequestRef.current = null;
        setOpenRequest((current) =>
          current?.id === request.id ? null : current,
        );
        setLoadingSession(false);
      })
      .catch((loadError: unknown) => {
        if (
          disposed ||
          controller.signal.aborted ||
          openRequestIdRef.current !== request.id ||
          openRequestRef.current?.id !== request.id
        ) {
          return;
        }
        if (isMissingSessionError(loadError)) {
          recoverMissingSession(agentId, request.session.id);
        } else {
          setError(errorMessage(loadError));
        }
        openRequestRef.current = null;
        setOpenRequest((current) =>
          current?.id === request.id ? null : current,
        );
        setLoadingSession(false);
      })
      .finally(() => {
        if (!disposed && openRequestRef.current?.id === request.id) {
          setLoadingSession(false);
        }
      });

    return () => {
      disposed = true;
      controller.abort();
    };
  }, [
    api,
    clearSessionContext,
    commitEventState,
    online,
    openRequest,
    recoverMissingSession,
    selectedAgentId,
  ]);

  useEffect(() => {
    const request = olderMessageRequest;
    const agentId = selectedAgentId;
    const sessionId = activeSession?.id;
    if (!request || !api || !online || !agentId || !sessionId) {
      setLoadingEarlierMessages(false);
      return;
    }

    const controller = new AbortController();
    let disposed = false;
    setLoadingEarlierMessages(true);

    void api
      .getMessages(agentId, sessionId, request.before, controller.signal)
      .then((page) => {
        if (
          disposed ||
          controller.signal.aborted ||
          selectedAgentIdRef.current !== agentId ||
          activeSessionIdRef.current !== sessionId
        ) {
          return;
        }
        const current = eventStateRef.current;
        if (
          !current ||
          current.agentId !== agentId ||
          current.sessionId !== sessionId
        ) {
          return;
        }
        commitEventState({
          ...current,
          messages: mergeSessionMessages(current.messages, page.messages),
        });
        setMessagesCursor(page.nextBefore);
        messagesCursorRef.current = page.nextBefore;
        setError(null);
        setOlderMessageRequest((next) =>
          next?.id === request.id ? null : next,
        );
      })
      .catch((loadError: unknown) => {
        if (disposed || controller.signal.aborted) return;
        if (isMissingSessionError(loadError)) {
          recoverMissingSession(agentId, sessionId);
        } else if (
          selectedAgentIdRef.current === agentId &&
          activeSessionIdRef.current === sessionId
        ) {
          setError(errorMessage(loadError));
        }
        setOlderMessageRequest((next) =>
          next?.id === request.id ? null : next,
        );
      })
      .finally(() => {
        if (!disposed) setLoadingEarlierMessages(false);
      });

    return () => {
      disposed = true;
      controller.abort();
    };
  }, [
    activeSession?.id,
    api,
    commitEventState,
    olderMessageRequest,
    online,
    recoverMissingSession,
    selectedAgentId,
  ]);

  useEffect(() => {
    const request = refreshRequest;
    if (
      !request ||
      !api ||
      !online ||
      request.agentId !== selectedAgentId ||
      request.sessionId !== activeSession?.id
    ) {
      return;
    }

    const controller = new AbortController();
    let disposed = false;

    void Promise.all([
      api.getMessages(
        request.agentId,
        request.sessionId,
        undefined,
        controller.signal,
      ),
      api.listRuns(request.agentId, request.sessionId, controller.signal),
    ])
      .then(([page, runs]) => {
        if (
          disposed ||
          controller.signal.aborted ||
          selectedAgentIdRef.current !== request.agentId ||
          activeSessionIdRef.current !== request.sessionId
        ) {
          return;
        }
        const current = eventStateRef.current;
        if (
          !current ||
          current.agentId !== request.agentId ||
          current.sessionId !== request.sessionId
        ) {
          return;
        }
        const merged = mergeSessionMessages(current.messages, page.messages);
        const reconciled = replaceEventStateRuns(current, runs);
        const cleared = clearSessionRefreshRequest({
          ...current,
          messages: merged,
          runs: reconciled.runs,
          runSessionIds: reconciled.runSessionIds,
          runAgentIds: reconciled.runAgentIds,
        });
        commitEventState(cleared);
        setMessagesCursor(page.nextBefore);
        messagesCursorRef.current = page.nextBefore;
        setRecentRuns(runs);
        recentRunsRef.current = runs;
        setStopRequestedRunIds((currentIds) => {
          const terminalIds = new Set(
            runs
              .filter((run) => isTerminalRunStatus(run.status))
              .map((run) => run.id),
          );
          const nextIds = currentIds.filter((id) => !terminalIds.has(id));
          stopRequestedRunIdsRef.current = nextIds;
          return nextIds;
        });
      })
      .catch((loadError: unknown) => {
        if (disposed || controller.signal.aborted) return;
        if (isMissingSessionError(loadError)) {
          recoverMissingSession(request.agentId, request.sessionId);
        } else {
          setError(errorMessage(loadError));
        }
      });

    return () => {
      disposed = true;
      controller.abort();
    };
  }, [
    activeSession?.id,
    api,
    commitEventState,
    online,
    refreshRequest,
    recoverMissingSession,
    selectedAgentId,
  ]);

  useEffect(() => {
    const agentId = selectedAgentId;
    if (!api || !online || !agentId) {
      setStreamStatus('disconnected');
      return;
    }

    const controller = new AbortController();
    let disposed = false;
    let hasConnected = false;
    let reconnectAttempt = 0;

    const applyEvent = (event: AgentEvent) => {
      onEventRef.current?.(event);
      if (
        event.type === 'stream.snapshot' &&
        event.agentId === agentId &&
        selectedAgentIdRef.current === agentId
      ) {
        latestSnapshotRef.current = { agentId, event };
      }
      const current = eventStateRef.current;
      const pendingOpen = openRequestRef.current;
      const pendingOpenDeleted =
        pendingOpen?.session.agentId === event.agentId &&
        pendingOpen.session.id === event.sessionId;
      const activeSessionDeleted =
        activeSessionIdRef.current === event.sessionId;
      if (
        event.type === 'session.deleted' &&
        event.agentId === agentId &&
        selectedAgentIdRef.current === agentId &&
        (activeSessionDeleted || pendingOpenDeleted)
      ) {
        if (event.sessionId) recoverMissingSession(agentId, event.sessionId);
        return;
      }
      if (!current) return;

      const next = reduceSessionEvent(current, event, agentId);
      if (next !== current) commitEventState(next);
      if (next.refreshRequired && !current.refreshRequired) {
        // A resync invalidates this iterator. Its authoritative refresh runs
        // after the iterator closes and before the next subscription starts.
        return;
      }
      if (
        (event.type === 'run.completed' ||
          event.type === 'run.failed' ||
          event.type === 'run.cancelled' ||
          event.type === 'run.interrupted') &&
        isTerminalRunStatus(event.run.status)
      ) {
        scheduleRefresh(agentId, current.sessionId);
        setStopRequestedRunIds((ids) => {
          const nextIds = ids.filter((id) => id !== event.run.id);
          stopRequestedRunIdsRef.current = nextIds;
          return nextIds;
        });
      }
    };

    const refreshCurrentSession = async () => {
      const sessionId = activeSessionIdRef.current;
      if (!sessionId || controller.signal.aborted) return;
      try {
        const [page, runs] = await Promise.all([
          api.getMessages(agentId, sessionId, undefined, controller.signal),
          api.listRuns(agentId, sessionId, controller.signal),
        ]);
        if (
          disposed ||
          controller.signal.aborted ||
          selectedAgentIdRef.current !== agentId ||
          activeSessionIdRef.current !== sessionId
        ) {
          return;
        }
        const current = eventStateRef.current;
        if (
          !current ||
          current.agentId !== agentId ||
          current.sessionId !== sessionId
        )
          return;
        const merged = mergeSessionMessages(current.messages, page.messages);
        const reconciled = replaceEventStateRuns(current, runs);
        commitEventState(
          clearSessionRefreshRequest({ ...reconciled, messages: merged }),
        );
        setMessagesCursor(page.nextBefore);
        messagesCursorRef.current = page.nextBefore;
        setRecentRuns(runs);
        recentRunsRef.current = runs;
        setStopRequestedRunIds((currentIds) => {
          const terminalIds = new Set(
            runs
              .filter((run) => isTerminalRunStatus(run.status))
              .map((run) => run.id),
          );
          const nextIds = currentIds.filter((id) => !terminalIds.has(id));
          stopRequestedRunIdsRef.current = nextIds;
          return nextIds;
        });
      } catch (loadError) {
        if (disposed || controller.signal.aborted) return;
        if (isMissingSessionError(loadError)) {
          recoverMissingSession(agentId, sessionId);
        } else {
          setError(errorMessage(loadError));
        }
      }
    };

    const runStream = async () => {
      while (!disposed && !controller.signal.aborted) {
        setStreamStatus(hasConnected ? 'reconnecting' : 'connecting');
        let connectedAtMs: number | null = null;
        try {
          const stream = api.streamEvents(agentId, controller.signal);
          connectedAtMs = Date.now();
          setStreamStatus('connected');
          hasConnected = true;
          let resyncRequested = false;
          for await (const event of stream) {
            if (disposed || controller.signal.aborted) break;
            applyEvent(event);
            if (
              event.type === 'stream.resync' &&
              event.agentId === agentId &&
              eventStateRef.current?.refreshRequired
            ) {
              resyncRequested = true;
              break;
            }
          }
          if (disposed || controller.signal.aborted) break;
          setStreamStatus('reconnecting');
          await refreshCurrentSession();
          if (resyncRequested) {
            const current = eventStateRef.current;
            if (current?.refreshRequired)
              commitEventState(clearSessionRefreshRequest(current));
          }
        } catch (streamError) {
          if (disposed || controller.signal.aborted) break;
          setError(errorMessage(streamError));
          setStreamStatus('reconnecting');
          await refreshCurrentSession();
        }
        if (
          connectedAtMs !== null &&
          Date.now() - connectedAtMs >= RECONNECT_RESET_AFTER_MS
        ) {
          reconnectAttempt = 0;
        }
        const delayMs = Math.min(
          RECONNECT_BASE_DELAY_MS * 2 ** reconnectAttempt,
          RECONNECT_MAX_DELAY_MS,
        );
        reconnectAttempt = Math.min(
          reconnectAttempt + 1,
          RECONNECT_MAX_ATTEMPT,
        );
        await waitForReconnect(controller.signal, delayMs);
      }
    };

    void runStream();

    return () => {
      disposed = true;
      controller.abort();
      if (mountedRef.current) setStreamStatus('disconnected');
    };
  }, [
    api,
    commitEventState,
    online,
    recoverMissingSession,
    scheduleRefresh,
    selectedAgentId,
  ]);

  const selectAgent = useCallback(
    async (agentId: string): Promise<boolean> => {
      if (
        !api ||
        !online ||
        !agents.some((agent) => agent.state.id === agentId)
      ) {
        return false;
      }
      if (selectedAgentIdRef.current === agentId) return true;
      latestSnapshotRef.current = null;
      openRequestIdRef.current += 1;
      openRequestRef.current = null;
      selectedAgentIdRef.current = agentId;
      setSelectedAgentId(agentId);
      setLoadingSessions(true);
      setSessions([]);
      setSessionCursor(undefined);
      sessionCursorRef.current = undefined;
      setNextSessionCursor(null);
      nextSessionCursorRef.current = null;
      clearSessionContext();
      setOpenRequest(null);
      setOlderMessageRequest(null);
      setRefreshRequest(null);
      setError(null);
      return true;
    },
    [agents, api, clearSessionContext, online],
  );

  const loadNextSessions = useCallback(async (): Promise<boolean> => {
    const cursor = nextSessionCursorRef.current;
    if (
      !api ||
      !online ||
      !selectedAgentIdRef.current ||
      !cursor ||
      loadingSessions
    ) {
      return false;
    }
    sessionCursorRef.current = cursor;
    setSessionCursor(cursor);
    return true;
  }, [api, loadingSessions, online]);

  const openSession = useCallback(
    async (sessionOrId: Session | string): Promise<boolean> => {
      const agentId = selectedAgentIdRef.current;
      if (!api || !online || !agentId) return false;
      const session =
        typeof sessionOrId === 'string'
          ? (sessions.find((candidate) => candidate.id === sessionOrId) ??
            createSessionReference(agentId, sessionOrId))
          : sessionOrId;
      if (
        session.agentId !== agentId ||
        session.kind !== 'chat' ||
        session.archived
      ) {
        return false;
      }
      const request = { id: ++openRequestIdRef.current, session };
      openRequestRef.current = request;
      setOlderMessageRequest(null);
      setLoadingEarlierMessages(false);
      setOpenRequest(request);
      return true;
    },
    [api, online, sessions],
  );

  const createSession = useCallback(
    async (title?: string): Promise<Session | null> => {
      const agentId = selectedAgentIdRef.current;
      if (!api || !online || !agentId) return null;
      try {
        const created = await api.createSession(agentId, title);
        if (!mountedRef.current || selectedAgentIdRef.current !== agentId)
          return null;
        setError(null);
        setSessionCursor(undefined);
        sessionCursorRef.current = undefined;
        setSessionListRevision((revision) => revision + 1);
        await openSession(created);
        return created;
      } catch (createError) {
        if (mountedRef.current && selectedAgentIdRef.current === agentId) {
          setError(errorMessage(createError));
        }
        return null;
      }
    },
    [api, online, openSession],
  );

  const leaveSession = useCallback(() => {
    openRequestIdRef.current += 1;
    openRequestRef.current = null;
    clearSessionContext();
    setOpenRequest(null);
  }, [clearSessionContext]);

  const loadEarlierMessages = useCallback(async (): Promise<boolean> => {
    const before = messagesCursorRef.current;
    if (
      !api ||
      !online ||
      !selectedAgentIdRef.current ||
      !activeSessionIdRef.current ||
      !before ||
      loadingEarlierMessages
    ) {
      return false;
    }
    setOlderMessageRequest({ id: ++olderMessageRequestIdRef.current, before });
    return true;
  }, [api, loadingEarlierMessages, online]);

  const sendMessage = useCallback(
    async (text: string): Promise<boolean> => {
      const agentId = selectedAgentIdRef.current;
      const sessionId = activeSessionIdRef.current;
      const trimmedText = text.trim();
      if (
        !api ||
        !online ||
        !agentId ||
        !sessionId ||
        !activeSession?.capabilities.send ||
        !trimmedText
      ) {
        return false;
      }
      const mode = runMode;
      try {
        const result = await api.startRun(
          agentId,
          sessionId,
          { text: trimmedText, mode },
          createIdempotencyKey(),
        );
        if (
          !mountedRef.current ||
          selectedAgentIdRef.current !== agentId ||
          activeSessionIdRef.current !== sessionId
        ) {
          return false;
        }
        setRecentRuns((current) => {
          const next = dedupeRuns(current, [result.run]);
          recentRunsRef.current = next;
          return next;
        });
        const current = eventStateRef.current;
        if (current?.agentId === agentId && current.sessionId === sessionId) {
          const next = {
            ...current,
            runs: dedupeRuns(current.runs, [result.run]),
            runSessionIds: {
              ...current.runSessionIds,
              [result.run.id]: result.run.sessionId,
            },
            runAgentIds: {
              ...current.runAgentIds,
              [result.run.id]: result.run.agentId,
            },
          };
          commitEventState(next);
        }
        scheduleRefresh(agentId, sessionId);
        setError(null);
        return true;
      } catch (sendError) {
        if (mountedRef.current && isMissingSessionError(sendError)) {
          recoverMissingSession(agentId, sessionId);
        } else if (
          mountedRef.current &&
          selectedAgentIdRef.current === agentId &&
          activeSessionIdRef.current === sessionId
        ) {
          setError(errorMessage(sendError));
        }
        return false;
      }
    },
    [
      activeSession,
      api,
      commitEventState,
      online,
      recoverMissingSession,
      runMode,
      scheduleRefresh,
    ],
  );

  const stopActiveRun = useCallback(async (): Promise<boolean> => {
    const agentId = selectedAgentIdRef.current;
    const sessionId = activeSessionIdRef.current;
    if (
      !api ||
      !online ||
      !agentId ||
      !sessionId ||
      !activeSession?.capabilities.stop
    ) {
      return false;
    }
    const currentRuns = dedupeRuns(
      recentRunsRef.current,
      eventStateRef.current?.runs ?? [],
    );
    const alreadyRequested = new Set(stopRequestedRunIdsRef.current);
    const run = currentRuns.find(
      (candidate) =>
        candidate.sessionId === sessionId &&
        STOPPABLE_RUN_STATUSES.has(candidate.status) &&
        candidate.stop === null &&
        !alreadyRequested.has(candidate.id),
    );
    if (!run) return false;

    try {
      const stopped = await api.stopRun(agentId, run.id);
      if (
        !mountedRef.current ||
        selectedAgentIdRef.current !== agentId ||
        activeSessionIdRef.current !== sessionId
      ) {
        return false;
      }
      setRecentRuns((current) => {
        const next = dedupeRuns(current, [stopped]);
        recentRunsRef.current = next;
        return next;
      });
      const current = eventStateRef.current;
      if (current?.agentId === agentId && current.sessionId === sessionId) {
        const next = {
          ...current,
          runs: dedupeRuns(current.runs, [stopped]),
          runSessionIds: {
            ...current.runSessionIds,
            [stopped.id]: stopped.sessionId,
          },
          runAgentIds: {
            ...current.runAgentIds,
            [stopped.id]: stopped.agentId,
          },
        };
        commitEventState(next);
      }
      if (!isTerminalRunStatus(stopped.status)) {
        setStopRequestedRunIds((ids) => {
          const next = ids.includes(stopped.id) ? ids : [...ids, stopped.id];
          stopRequestedRunIdsRef.current = next;
          return next;
        });
      }
      scheduleRefresh(agentId, sessionId);
      setError(null);
      return true;
    } catch (stopError) {
      if (mountedRef.current && isExplicitMissingSessionError(stopError)) {
        recoverMissingSession(agentId, sessionId);
      } else if (
        mountedRef.current &&
        selectedAgentIdRef.current === agentId &&
        activeSessionIdRef.current === sessionId
      ) {
        setError(errorMessage(stopError));
      }
      return false;
    }
  }, [
    activeSession,
    api,
    commitEventState,
    online,
    recoverMissingSession,
    scheduleRefresh,
  ]);

  const retry = useCallback(() => {
    if (!api || !online) return;
    const agentId = selectedAgentIdRef.current;
    const sessionId = activeSessionIdRef.current;
    setError(null);
    if (agentId && sessionId) {
      scheduleRefresh(agentId, sessionId);
      return;
    }
    if (agentId) {
      refreshSessions();
      return;
    }
    setAgentLoadRevision((revision) => revision + 1);
  }, [api, online, refreshSessions, scheduleRefresh]);

  const clearError = useCallback(() => setError(null), []);

  const liveRuns = eventState?.runs ?? [];
  const runs = dedupeRuns(recentRuns, liveRuns);
  const stopRequested = new Set(stopRequestedRunIds);
  const activeRun =
    runs
      .filter(
        (run) =>
          run.sessionId === activeSession?.id &&
          STOPPABLE_RUN_STATUSES.has(run.status),
      )
      .sort((left, right) => right.createdAtMs - left.createdAtMs)[0] ?? null;
  const canStopActiveRun = Boolean(
    activeSession?.capabilities.stop &&
    activeRun &&
    activeRun.stop === null &&
    !stopRequested.has(activeRun.id),
  );

  return {
    agents,
    selectedAgentId,
    selectedAgent:
      agents.find((agent) => agent.state.id === selectedAgentId) ?? null,
    agentSelectionRequired: agents.length > 1 && selectedAgentId === null,
    sessions,
    sessionCursor,
    nextSessionCursor,
    activeSession,
    messages: eventState?.messages ?? [],
    messagesCursor,
    recentRuns,
    runs,
    eventState,
    runMode,
    streamStatus,
    loadingAgents,
    loadingSessions,
    loadingSession,
    loadingEarlierMessages,
    error,
    canRetry: error !== null,
    isOffline: !online,
    sessionActionsDisabled: !api || !online || selectedAgentId === null,
    stopRequestedRunIds,
    canStopActiveRun,
    activeRun,
    selectAgent,
    loadNextSessions,
    refreshSessions,
    openSession,
    createSession,
    leaveSession,
    loadEarlierMessages,
    setRunMode,
    sendMessage,
    stopActiveRun,
    retry,
    clearError,
  };
}
