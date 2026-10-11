import type {
  AgentSnapshot,
  Approval,
  AgentEvent,
  DaemonStatus,
  Run,
  Session,
  SessionMessage,
  SnapshotRun,
  UploadedAttachment,
} from '@animaOS-SWARM/sdk';

export const ORIGIN = 'http://anima-console.test';
export const AGENT = 'companion';
export interface ConsoleFixtureState {
  agents: AgentSnapshot[];
  sessions: Session[];
  messages: Record<string, SessionMessage[]>;
  runs: Run[];
  approvals: Approval[];
  attachments: UploadedAttachment[];
  snapshotRuns: SnapshotRun[];
  status: DaemonStatus;
  configured: boolean;
}
export function agent(id = AGENT, role = 'lead'): AgentSnapshot {
  return {
    state: {
      id,
      name: id === AGENT ? 'Anima' : 'Retained helper',
      status: 'idle',
      createdAtMs: id === AGENT ? 2 : 1,
      config: {
        bio: null,
        lore: null,
        knowledge: null,
        topics: null,
        adjectives: null,
        style: null,
        system: null,
        plugins: null,
        name: 'Anima',
        provider: 'openai',
        model: 'test-model',
        settings: {
          temperature: null,
          maxTokens: null,
          timeoutMs: null,
          maxRetries: null,
          maxToolIterations: null,
          additional: { workspaceRole: role },
        },
        tools: [],
      },
      tokenUsage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
    },
    messages: [],
    messageCount: 0,
    eventCount: 0,
    lastTask: null,
  };
}
export function createState(empty = false): ConsoleFixtureState {
  return {
    agents: empty ? [] : [agent(), agent('helper', 'helper')],
    sessions: [],
    messages: {},
    runs: [],
    approvals: [],
    attachments: [],
    snapshotRuns: [],
    status: statusFixture({
      connectors: [],
      runs: { running: 0, queued: 0, byStatus: {} },
    }),
    configured: !empty,
  };
}
export function session(id = 'chat:e2e', title = 'New chat'): Session {
  return sessionFixture(id, { agentId: AGENT, title });
}
export function queuedRun(
  sessionId: string,
  input: Run['input'],
  idempotencyKey: string,
): Run {
  return {
    id: 'run_1',
    agentId: AGENT,
    sessionId,
    source: 'web',
    sourceRef: null,
    idempotencyKey,
    status: 'queued',
    input,
    createdAtMs: 10,
    startedAtMs: null,
    finishedAtMs: null,
    error: null,
    stop: null,
    toolsStarted: [],
    steps: [],
    usage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
    model: 'test-model',
    provider: 'openai',
    parentRunId: null,
    replyMessageId: null,
  };
}
export function snapshot(state: ConsoleFixtureState): AgentEvent {
  return {
    type: 'stream.snapshot',
    agentId: AGENT,
    seq: 1,
    at: 1,
    runs: state.snapshotRuns,
    approvals: state.approvals.filter((a) => a.status === 'pending'),
  };
}

/** A read-write chat of `agent-main` with every derived field. */
export function sessionFixture(
  id: string,
  overrides: Partial<Session> = {},
): Session {
  return {
    id,
    agentId: 'agent-main',
    roomId: id,
    kind: 'chat',
    origin: 'web',
    title: 'New chat',
    titleSource: 'first_message',
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

export function statusFixture(
  overrides: Partial<DaemonStatus> = {},
): DaemonStatus {
  return {
    version: '0.9.1',
    buildRevision: 'abc1234',
    startedAtMs: 0,
    nowMs: 11_100_000,
    uptimeSeconds: 11_100,
    readiness: { status: 'ready', issues: [] },
    storage: {
      persistenceMode: 'sqlite',
      controlPlane: 'json',
      controlPlaneDurability: 'durable',
      history: {
        store: 'sqlite',
        ephemeral: false,
        healthy: true,
        pendingFlush: 3,
        usageQueued: 0,
        lastError: null,
        failingSinceMs: null,
        flushErrors: 0,
      },
    },
    providers: [
      { id: 'openai', label: 'OpenAI', configured: true },
      { id: 'anthropic', label: 'Anthropic', configured: false },
    ],
    connectors: [
      {
        id: 'telegram-1',
        agentId: 'agent-main',
        type: 'telegram',
        status: 'ready',
        enabled: true,
      },
    ],
    automations: { total: 4, enabled: 3, failing: 0, failuresTotal: 0 },
    approvals: { pending: 0 },
    runs: { running: 1, queued: 2, byStatus: { completed: 5, running: 1 } },
    events: { subscribers: 2, laggedEvents: 0 },
    logs: { buffered: 100, newestSeq: 100 },
    limits: {
      maxRequestBytes: 1_000_000,
      maxConcurrentRuns: 4,
      maxRunsPerAgent: 2,
      queuedRunsPerAgent: 8,
      maxBackgroundProcesses: 4,
      logBufferLines: 2_000,
      eventBuffer: 1_000,
    },
    ...overrides,
  };
}
